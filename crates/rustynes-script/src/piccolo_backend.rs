//! Experimental pure-Rust Lua backend (`piccolo`) for `wasm32`.
//!
//! Compiled only behind the off-by-default `script-wasm` feature. piccolo is a
//! pure-Rust, stackless Lua VM (no C toolchain), so it links into the
//! `wasm32-unknown-unknown` frontend where the native mlua/`cc` path cannot.
//!
//! ## Explicitly NOT byte-parity with mlua (ADR 0012)
//!
//! piccolo is a *different* VM with a different (incomplete) Lua 5.4
//! implementation, a different GC, and its own fuel accounting. This backend is
//! therefore **not** bit-identical to the mlua backend, and that is acceptable:
//! scripts are observational / overlay + *gated* writes, and are NEVER part of
//! the framebuffer / audio determinism oracle (the `AccuracyCoin` / `nestest` /
//! TAS / netplay contract). See `docs/adr/0012-wasm-lua-piccolo-backend.md`.
//!
//! ## What it supports vs. not
//!
//! Supported (the observational subset piccolo hosts cleanly):
//! - `emu.read` / `emu.peek` / `emu.readRange` — served from a per-frame
//!   snapshot of the CPU address space (so a callback's view is internally
//!   consistent and needs no live `&mut Nes` inside the `'static` piccolo
//!   callback — which is what lets this backend avoid all `unsafe`).
//! - `emu.cpu` / `emu.frame` / `emu.cycle` — from the same per-frame snapshot.
//! - `emu.log` + `print` — captured to the host log.
//! - `emu.onFrame(fn)` — re-invoked once per emulated frame.
//! - `emu.drawText` / `drawRect` / `drawPixel` — overlay draws (host-rendered).
//! - `emu.pause` / `saveState` / `loadState` — queued control commands.
//! - `emu.write(addr, val)` / `emu.setInput(port, buttons)` — gated identically
//!   to mlua via `set_writes_locked`; writes are buffered and applied to the
//!   live `Nes` by the host AFTER the frame's callbacks run (so a same-frame
//!   `read` after a `write` observes the new value via the snapshot).
//!
//! Native-only limitations (documented; registered as no-ops so a portable
//! script that calls them does not error):
//! - `emu.onExec` / `emu.onRead` / `emu.onWrite` — the per-access replay needs
//!   the core's exec / access logs and a hot per-event Lua re-entry that this
//!   first experimental cut does not wire up.
//! - `emu.onNmi` / `emu.onIrq` — the per-interrupt replay, same rationale.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use piccolo::{Callback, CallbackReturn, Closure, Executor, Fuel, Lua, Table, Value};
use rustynes_core::Nes;

use crate::backend::VmBackend;
use crate::types::{ControlCmd, DEFAULT_INSTRUCTION_BUDGET, DrawCmd, MAX_QUEUED_CMDS, ScriptError};

/// A buffered `emu.write(addr, val)` (applied to the live `Nes` after the
/// frame's callbacks run). Kept separate from `ControlCmd` because the host
/// applies these directly (they are RAM pokes, not host-control actions).
type PendingWrites = Rc<RefCell<Vec<(u16, u8)>>>;

/// Per-frame snapshot of the readable CPU state a script can query. Shared into
/// every `Callback::from_fn` closure as an `Rc` (piccolo callbacks are
/// `'static + Fn`, so host state must be interior-mutable + `'static`; an `Rc`
/// is exactly that — no GC rooting, no `unsafe`).
#[derive(Default)]
struct Snapshot {
    /// The full 64 KiB CPU address space, captured at frame start via `peek`.
    /// `emu.read` / `readRange` serve from this; a same-frame `emu.write`
    /// updates it so a subsequent read observes the new value.
    mem: Vec<u8>,
    /// CPU registers (a, x, y, s, p, pc) at frame start.
    cpu: [u32; 6],
    /// `nes.frame()`.
    frame: u64,
    /// `nes.cycle()`.
    cycle: u64,
}

/// Push `cmd` into a host-drained queue unless it is already at the per-frame
/// cap (mirrors the mlua backend's `MAX_QUEUED_CMDS` guard).
fn push_capped<T>(q: &Rc<RefCell<Vec<T>>>, cmd: T) {
    let mut q = q.borrow_mut();
    if q.len() < MAX_QUEUED_CMDS {
        q.push(cmd);
    }
}

/// Best-effort piccolo `Value` -> display string for the log sink.
fn value_to_string(v: Value<'_>) -> String {
    match v {
        Value::String(s) => String::from_utf8_lossy(s.as_bytes()).into_owned(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Nil => "nil".to_owned(),
        Value::Table(_) => "table".to_owned(),
        Value::Function(_) => "function".to_owned(),
        Value::UserData(_) => "userdata".to_owned(),
        Value::Thread(_) => "thread".to_owned(),
    }
}

/// Cap the `u64` instruction budget to piccolo's `i32` fuel domain.
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
fn fuel_for(budget: u64) -> i32 {
    budget.min(i32::MAX as u64) as i32
}

/// Wrap a Lua-supplied integer into the 16-bit CPU address space as a `usize`
/// index into the 64 KiB snapshot. Sign- and width-safe (no `as` casts that
/// clippy flags): mask first in `i64`, then the value is provably `0..=0xFFFF`.
fn mask_addr(addr: i64) -> usize {
    usize::try_from(addr & 0xFFFF).unwrap_or(0)
}

/// Truncate a Lua-supplied integer to a byte (`0..=0xFF`).
fn mask_byte(val: i64) -> u8 {
    u8::try_from(val & 0xFF).unwrap_or(0)
}

/// Truncate a Lua-supplied integer to a 32-bit `0xRRGGBBAA` color, defaulting
/// to opaque white when the script omitted the argument.
fn mask_color(color: Option<i64>) -> u32 {
    color.map_or(0xFFFF_FFFF, |c| u32::try_from(c & 0xFFFF_FFFF).unwrap_or(0))
}

/// Truncate a Lua-supplied integer to an `i32` overlay coordinate.
fn coord(v: i64) -> i32 {
    i32::try_from(v.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).unwrap_or(0)
}

/// The experimental piccolo backend.
pub struct PiccoloBackend {
    lua: Lua,
    /// Captured `print` / `emu.log` output, drained by the host.
    log: Rc<RefCell<Vec<String>>>,
    /// Control actions queued this frame (drained by the host).
    controls: Rc<RefCell<Vec<ControlCmd>>>,
    /// Overlay draw commands queued this frame (drained by the host).
    draws: Rc<RefCell<Vec<DrawCmd>>>,
    /// Buffered `emu.write` RAM pokes, applied after the frame's callbacks.
    pending_writes: PendingWrites,
    /// The per-frame readable-state snapshot (see [`Snapshot`]).
    snapshot: Rc<RefCell<Snapshot>>,
    /// Registered `emu.onFrame` callbacks. `StashedFunction` keeps each
    /// script-supplied function GC-rooted across the gaps between frames (the
    /// piccolo equivalent of mlua's registry keys).
    frame_cbs: Rc<RefCell<Vec<piccolo::StashedFunction>>>,
    /// Number of registered `onFrame` callbacks (host UI / tests).
    frame_count: Rc<Cell<usize>>,
    /// Per-frame fuel budget (maps the instruction budget onto piccolo fuel).
    budget: Rc<Cell<u64>>,
    /// When `true`, `emu.write` AND `emu.setInput` are silent no-ops (locked /
    /// deterministic session) — gated identically to the mlua backend.
    writes_locked: Rc<Cell<bool>>,
}

impl PiccoloBackend {
    /// Install the persistent `emu` table + `print` redirect.
    ///
    /// `&mut self`: piccolo's `Lua::enter` borrows the arena mutably. The `Rc`
    /// clones below all complete before `self.lua.enter`, so the brief shared
    /// borrows of the other fields don't overlap the `&mut self.lua`.
    #[allow(clippy::too_many_lines)] // one Callback::from_fn per API entry.
    fn install_prelude(&mut self) {
        let log = Rc::clone(&self.log);
        let controls = Rc::clone(&self.controls);
        let draws = Rc::clone(&self.draws);
        let pending_writes = Rc::clone(&self.pending_writes);
        let snapshot = Rc::clone(&self.snapshot);
        let frame_cbs = Rc::clone(&self.frame_cbs);
        let frame_count = Rc::clone(&self.frame_count);
        let writes_locked = Rc::clone(&self.writes_locked);

        self.lua.enter(|ctx| {
            let emu = Table::new(&ctx);

            // emu.log(...) — join args with tabs, append to the host buffer.
            {
                let log = Rc::clone(&log);
                let log_fn = Callback::from_fn(&ctx, move |_ctx, _ex, mut stack| {
                    let mut parts = Vec::with_capacity(stack.len());
                    for v in stack.drain(..) {
                        parts.push(value_to_string(v));
                    }
                    log.borrow_mut().push(parts.join("\t"));
                    Ok(CallbackReturn::Return)
                });
                emu.set(ctx, "log", log_fn).ok();
                // Redirect base `print` to the same sink.
                ctx.set_global("print", log_fn).ok();
            }

            // emu.read(addr) / emu.peek(addr) — from the snapshot (no side effects).
            {
                let snapshot = Rc::clone(&snapshot);
                let f = Callback::from_fn(&ctx, move |ctx, _ex, mut stack| {
                    let addr = stack.consume::<i64>(ctx).unwrap_or(0);
                    let snap = snapshot.borrow();
                    let val = snap.mem.get(mask_addr(addr)).copied().unwrap_or(0);
                    stack.replace(ctx, i64::from(val));
                    Ok(CallbackReturn::Return)
                });
                emu.set(ctx, "read", f).ok();
                emu.set(ctx, "peek", f).ok();
            }

            // emu.readRange(addr, len) — 1-based array from the snapshot.
            {
                let snapshot = Rc::clone(&snapshot);
                let f = Callback::from_fn(&ctx, move |ctx, _ex, mut stack| {
                    let (addr, len) = stack.consume::<(i64, i64)>(ctx).unwrap_or((0, 0));
                    let len = len.clamp(0, 0x1_0000);
                    let snap = snapshot.borrow();
                    let out = Table::new(&ctx);
                    for i in 0..len {
                        // `wrapping_add` so a near-i64::MAX `addr` can't panic on
                        // overflow; `mask_addr` folds the result into 0..=0xFFFF
                        // anyway (gemini, PR #76).
                        let val = snap
                            .mem
                            .get(mask_addr(addr.wrapping_add(i)))
                            .copied()
                            .unwrap_or(0);
                        // 1-based, matching the mlua backend's array convention.
                        out.set(ctx, i + 1, i64::from(val)).ok();
                    }
                    stack.replace(ctx, out);
                    Ok(CallbackReturn::Return)
                });
                emu.set(ctx, "readRange", f).ok();
            }

            // emu.write(addr, val) — gated; buffered + reflected in the snapshot.
            {
                let pending_writes = Rc::clone(&pending_writes);
                let snapshot = Rc::clone(&snapshot);
                let writes_locked = Rc::clone(&writes_locked);
                let f = Callback::from_fn(&ctx, move |ctx, _ex, mut stack| {
                    let (addr, val) = stack.consume::<(i64, i64)>(ctx).unwrap_or((0, 0));
                    if !writes_locked.get() {
                        let addr = mask_addr(addr);
                        let val = mask_byte(val);
                        if let Some(slot) = snapshot.borrow_mut().mem.get_mut(addr) {
                            *slot = val;
                        }
                        #[allow(clippy::cast_possible_truncation)] // addr <= 0xFFFF
                        push_capped(&pending_writes, (addr as u16, val));
                    }
                    Ok(CallbackReturn::Return)
                });
                emu.set(ctx, "write", f).ok();
            }

            // emu.cpu() — registers snapshot as a table.
            {
                let snapshot = Rc::clone(&snapshot);
                let f = Callback::from_fn(&ctx, move |ctx, _ex, mut stack| {
                    let snap = snapshot.borrow();
                    let t = Table::new(&ctx);
                    for (k, v) in [
                        ("a", snap.cpu[0]),
                        ("x", snap.cpu[1]),
                        ("y", snap.cpu[2]),
                        ("s", snap.cpu[3]),
                        ("p", snap.cpu[4]),
                        ("pc", snap.cpu[5]),
                    ] {
                        t.set(ctx, k, i64::from(v)).ok();
                    }
                    stack.replace(ctx, t);
                    Ok(CallbackReturn::Return)
                });
                emu.set(ctx, "cpu", f).ok();
            }

            // emu.frame() / emu.cycle() — from the snapshot.
            {
                let snapshot = Rc::clone(&snapshot);
                let f = Callback::from_fn(&ctx, move |ctx, _ex, mut stack| {
                    #[allow(clippy::cast_possible_wrap)]
                    let v = snapshot.borrow().frame as i64;
                    stack.replace(ctx, v);
                    Ok(CallbackReturn::Return)
                });
                emu.set(ctx, "frame", f).ok();
            }
            {
                let snapshot = Rc::clone(&snapshot);
                let f = Callback::from_fn(&ctx, move |ctx, _ex, mut stack| {
                    #[allow(clippy::cast_possible_wrap)]
                    let v = snapshot.borrow().cycle as i64;
                    stack.replace(ctx, v);
                    Ok(CallbackReturn::Return)
                });
                emu.set(ctx, "cycle", f).ok();
            }

            // Control commands: emu.pause / saveState / loadState / setInput.
            {
                let controls = Rc::clone(&controls);
                let f = Callback::from_fn(&ctx, move |_ctx, _ex, _stack| {
                    push_capped(&controls, ControlCmd::Pause);
                    Ok(CallbackReturn::Return)
                });
                emu.set(ctx, "pause", f).ok();
            }
            {
                let controls = Rc::clone(&controls);
                let f = Callback::from_fn(&ctx, move |ctx, _ex, mut stack| {
                    let slot = stack.consume::<i64>(ctx).unwrap_or(0);
                    push_capped(&controls, ControlCmd::SaveState(mask_byte(slot)));
                    Ok(CallbackReturn::Return)
                });
                emu.set(ctx, "saveState", f).ok();
            }
            {
                let controls = Rc::clone(&controls);
                let f = Callback::from_fn(&ctx, move |ctx, _ex, mut stack| {
                    let slot = stack.consume::<i64>(ctx).unwrap_or(0);
                    push_capped(&controls, ControlCmd::LoadState(mask_byte(slot)));
                    Ok(CallbackReturn::Return)
                });
                emu.set(ctx, "loadState", f).ok();
            }
            {
                let controls = Rc::clone(&controls);
                let writes_locked = Rc::clone(&writes_locked);
                let f = Callback::from_fn(&ctx, move |ctx, _ex, mut stack| {
                    let (port, buttons) = stack.consume::<(i64, i64)>(ctx).unwrap_or((0, 0));
                    // Gated identically to emu.write (T-110-E2).
                    if !writes_locked.get() {
                        push_capped(
                            &controls,
                            ControlCmd::SetInput {
                                port: mask_byte(port),
                                buttons: mask_byte(buttons),
                            },
                        );
                    }
                    Ok(CallbackReturn::Return)
                });
                emu.set(ctx, "setInput", f).ok();
            }

            // Overlay draws.
            {
                let draws = Rc::clone(&draws);
                let f = Callback::from_fn(&ctx, move |ctx, _ex, mut stack| {
                    let (x, y, text, color) = stack
                        .consume::<(i64, i64, piccolo::String, Option<i64>)>(ctx)
                        .map(|(x, y, s, c)| {
                            (x, y, String::from_utf8_lossy(s.as_bytes()).into_owned(), c)
                        })
                        .unwrap_or((0, 0, String::new(), None));
                    push_capped(
                        &draws,
                        DrawCmd::Text {
                            x: coord(x),
                            y: coord(y),
                            color: mask_color(color),
                            text,
                        },
                    );
                    Ok(CallbackReturn::Return)
                });
                emu.set(ctx, "drawText", f).ok();
            }
            {
                let draws = Rc::clone(&draws);
                let f = Callback::from_fn(&ctx, move |ctx, _ex, mut stack| {
                    let (x, y, w, h, color) = stack
                        .consume::<(i64, i64, i64, i64, Option<i64>)>(ctx)
                        .unwrap_or((0, 0, 0, 0, None));
                    push_capped(
                        &draws,
                        DrawCmd::Rect {
                            x: coord(x),
                            y: coord(y),
                            w: coord(w),
                            h: coord(h),
                            color: mask_color(color),
                        },
                    );
                    Ok(CallbackReturn::Return)
                });
                emu.set(ctx, "drawRect", f).ok();
            }
            {
                let draws = Rc::clone(&draws);
                let f = Callback::from_fn(&ctx, move |ctx, _ex, mut stack| {
                    let (x, y, color) = stack
                        .consume::<(i64, i64, Option<i64>)>(ctx)
                        .unwrap_or((0, 0, None));
                    push_capped(
                        &draws,
                        DrawCmd::Pixel {
                            x: coord(x),
                            y: coord(y),
                            color: mask_color(color),
                        },
                    );
                    Ok(CallbackReturn::Return)
                });
                emu.set(ctx, "drawPixel", f).ok();
            }

            // emu.onFrame(fn) — stash the supplied function for per-frame replay.
            {
                let frame_cbs = Rc::clone(&frame_cbs);
                let frame_count = Rc::clone(&frame_count);
                let f = Callback::from_fn(&ctx, move |ctx, _ex, stack| {
                    if let Value::Function(func) = stack.get(0) {
                        frame_cbs.borrow_mut().push(ctx.stash(func));
                        frame_count.set(frame_count.get() + 1);
                    }
                    Ok(CallbackReturn::Return)
                });
                emu.set(ctx, "onFrame", f).ok();
            }

            // Native-only callbacks: registered as no-ops on the piccolo backend
            // (a documented limitation — see the module doc + ADR 0012). A
            // portable script that calls them does not error; the handler simply
            // does nothing on wasm.
            for name in ["onExec", "onRead", "onWrite", "onNmi", "onIrq"] {
                let f = Callback::from_fn(&ctx, |_ctx, _ex, _stack| Ok(CallbackReturn::Return));
                emu.set(ctx, name, f).ok();
            }

            ctx.set_global("emu", emu).ok();
        });
    }

    /// Refresh the per-frame readable-state snapshot from the live `Nes`.
    fn snapshot_state(&self, nes: &mut Nes) {
        let mut snap = self.snapshot.borrow_mut();
        // Persistent 64 KiB buffer overwritten in place (no per-frame clear/grow
        // churn — gemini, PR #76); the 65536 `peek`s are the inherent cost.
        if snap.mem.len() != 0x1_0000 {
            snap.mem.resize(0x1_0000, 0);
        }
        // Zip a u16 address range with the buffer — no enumerate()/`as u16`
        // cast + clippy suppression (gemini #76).
        for (addr, slot) in (0u16..=0xFFFF).zip(snap.mem.iter_mut()) {
            *slot = nes.peek(addr);
        }
        let c = nes.cpu();
        snap.cpu = [
            u32::from(c.a),
            u32::from(c.x),
            u32::from(c.y),
            u32::from(c.s),
            u32::from(c.p.bits()),
            u32::from(c.pc),
        ];
        snap.frame = nes.frame();
        snap.cycle = nes.cycle();
    }

    /// Run `executor` to completion under one frame's fuel budget. Returns
    /// `Ok(())` on completion, [`ScriptError::Budget`] if the fuel ran out, or
    /// [`ScriptError::Lua`] if the script raised.
    fn drive(&mut self, executor: &piccolo::StashedExecutor) -> Result<(), ScriptError> {
        let fuel_budget = fuel_for(self.budget.get());
        let finished = self.lua.enter(|ctx| {
            let ex = ctx.fetch(executor);
            let mut fuel = Fuel::with(fuel_budget);
            loop {
                // `step` returns `true` when the executor can make no more
                // progress (finished or errored).
                if ex.step(ctx, &mut fuel) {
                    break true;
                }
                if !fuel.should_continue() {
                    break false;
                }
            }
        });
        if finished {
            self.lua
                .execute::<()>(executor)
                .map_err(|e| ScriptError::Lua(e.to_string()))
        } else {
            Err(ScriptError::Budget)
        }
    }
}

impl VmBackend for PiccoloBackend {
    fn new() -> Result<Self, ScriptError> {
        // `Lua::core()` is the sandbox-friendly env: base language + string /
        // table / math / coroutine, but NO `io` / `os` / `require`.
        let mut engine = Self {
            lua: Lua::core(),
            log: Rc::new(RefCell::new(Vec::new())),
            controls: Rc::new(RefCell::new(Vec::new())),
            draws: Rc::new(RefCell::new(Vec::new())),
            pending_writes: Rc::new(RefCell::new(Vec::new())),
            snapshot: Rc::new(RefCell::new(Snapshot::default())),
            frame_cbs: Rc::new(RefCell::new(Vec::new())),
            frame_count: Rc::new(Cell::new(0)),
            budget: Rc::new(Cell::new(DEFAULT_INSTRUCTION_BUDGET)),
            writes_locked: Rc::new(Cell::new(false)),
        };
        // Forbid dynamic compilation from within a script (defence-in-depth;
        // `core()` has no `dofile`/`loadfile`, but `load` exists).
        engine.lua.enter(|ctx| {
            for name in ["load", "loadstring", "dofile", "loadfile", "collectgarbage"] {
                ctx.set_global(name, Value::Nil).ok();
            }
        });
        engine.install_prelude();
        Ok(engine)
    }

    fn load(&mut self, src: &str) -> Result<(), ScriptError> {
        let src = src.to_owned();
        let executor = self
            .lua
            .try_enter(|ctx| {
                let closure = Closure::load(ctx, Some("script"), src.as_bytes())?;
                Ok(ctx.stash(Executor::start(ctx, closure.into(), ())))
            })
            .map_err(|e| ScriptError::Lua(e.to_string()))?;
        self.drive(&executor)
    }

    fn on_frame(&mut self, nes: &mut Nes) -> Result<(), ScriptError> {
        // Refresh the readable snapshot (CPU regs + frame/cycle + 64 KiB).
        self.snapshot_state(nes);
        self.pending_writes.borrow_mut().clear();

        // Invoke each registered onFrame callback in registration order, each
        // under its own per-frame fuel budget.
        let n = self.frame_cbs.borrow().len();
        let mut result = Ok(());
        for idx in 0..n {
            let frame_cbs = Rc::clone(&self.frame_cbs);
            let exec = self.lua.try_enter(|ctx| {
                let func = ctx.fetch(&frame_cbs.borrow()[idx]);
                Ok(ctx.stash(Executor::start(ctx, func, ())))
            });
            let exec = match exec {
                Ok(e) => e,
                Err(e) => {
                    result = Err(ScriptError::Lua(e.to_string()));
                    break;
                }
            };
            if let Err(e) = self.drive(&exec) {
                result = Err(e);
                break;
            }
        }

        // Apply the frame's buffered RAM pokes to the live Nes (gated writes
        // were already dropped at the source). Done here, where we hold the
        // `&mut Nes`, so the piccolo callbacks never need a live pointer.
        for (addr, val) in self.pending_writes.borrow_mut().drain(..) {
            nes.poke_ram(addr, val);
        }
        result
    }

    fn set_instruction_budget(&self, budget: u64) {
        self.budget.set(budget);
    }

    fn set_writes_locked(&self, locked: bool) {
        self.writes_locked.set(locked);
    }

    fn drain_log(&self) -> Vec<String> {
        std::mem::take(&mut self.log.borrow_mut())
    }

    fn drain_controls(&self) -> Vec<ControlCmd> {
        std::mem::take(&mut self.controls.borrow_mut())
    }

    fn drain_draws(&self) -> Vec<DrawCmd> {
        std::mem::take(&mut self.draws.borrow_mut())
    }

    // The per-access / per-interrupt callbacks are no-ops on this backend, so
    // the host never needs to enable the corresponding core logs.
    fn needs_exec_log(&self) -> bool {
        false
    }

    fn needs_access_log(&self) -> bool {
        false
    }

    fn needs_interrupt_log(&self) -> bool {
        false
    }

    fn frame_callback_count(&self) -> usize {
        self.frame_count.get()
    }
}

#[cfg(test)]
mod piccolo_tests {
    //! Backend-specific tests for the documented divergences (ADR 0012). The
    //! shared cross-backend tests live in `lib.rs` and already run against this
    //! backend; these cover what is unique to piccolo (deferred writes, the
    //! `Budget` error variant, the no-op native-only callbacks). piccolo is
    //! pure Rust, so they run on the native test host.

    use super::*;

    /// A minimal NROM ROM whose reset vector loops `JMP $C000`.
    fn synth_rom() -> Vec<u8> {
        let mut bytes = vec![b'N', b'E', b'S', 0x1A, 1, 1, 0, 0];
        bytes.resize(16, 0);
        let mut prg = vec![0u8; 16 * 1024];
        prg[0] = 0x4C;
        prg[1] = 0x00;
        prg[2] = 0xC0;
        let len = prg.len();
        prg[len - 4] = 0x00;
        prg[len - 3] = 0xC0;
        bytes.extend_from_slice(&prg);
        bytes.resize(16 + 16 * 1024 + 8 * 1024, 0);
        bytes
    }

    #[test]
    fn write_is_deferred_but_lands() {
        // The piccolo backend cannot hold a live `&mut Nes` inside a `'static`
        // callback, so `emu.write` is buffered and applied AFTER the frame's
        // callbacks (the documented divergence, ADR 0012). The snapshot is
        // updated in-place, so a SAME-frame read-after-write sees the new value;
        // the live `Nes` receives the poke once `on_frame` returns.
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = PiccoloBackend::new().expect("engine");
        eng.load(
            r"
            emu.onFrame(function()
                emu.write(0x10, 0x42)
                emu.log('mem10=' .. emu.read(0x10))
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        // Same-frame read sees the buffered value via the snapshot.
        assert_eq!(eng.drain_log(), vec!["mem10=66"]);
        // And the deferred poke landed on the live Nes after the callback.
        assert_eq!(nes.peek(0x10), 0x42);
    }

    #[test]
    fn deferred_write_is_gated_when_locked() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = PiccoloBackend::new().expect("engine");
        eng.set_writes_locked(true);
        eng.load("emu.onFrame(function() emu.write(0x20, 0x99) end)")
            .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert_eq!(nes.peek(0x20), 0x00, "write must be dropped when locked");
    }

    #[test]
    fn runaway_loop_hits_the_fuel_budget() {
        let mut eng = PiccoloBackend::new().expect("engine");
        eng.set_instruction_budget(50_000);
        let err = eng.load("while true do end").unwrap_err();
        assert!(
            matches!(err, ScriptError::Budget),
            "piccolo surfaces a runaway as Budget, got {err:?}"
        );
    }

    #[test]
    fn native_only_callbacks_are_noops_not_errors() {
        // A portable script that registers onExec/onRead/onWrite/onNmi/onIrq must
        // not error on wasm — they are documented no-ops (ADR 0012).
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = PiccoloBackend::new().expect("engine");
        eng.load(
            r"
            emu.onExec(0xC000, function() end)
            emu.onRead(0x10, function() end)
            emu.onWrite(0x2000, function() end)
            emu.onNmi(function() end)
            emu.onIrq(function() end)
            emu.onFrame(function() emu.log('ok') end)
            ",
        )
        .expect("load");
        // The host never enables the core logs for this backend.
        assert!(!eng.needs_exec_log());
        assert!(!eng.needs_access_log());
        assert!(!eng.needs_interrupt_log());
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert!(eng.drain_log().contains(&"ok".to_owned()));
    }
}

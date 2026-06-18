//! Native Lua 5.4 backend (vendored [`mlua`]).
//!
//! This is the reference backend and the only one compiled on native targets
//! (behind the crate's default `mlua-backend` feature, which the frontend's
//! native `scripting` feature pulls in). It is byte-identical to the v1.1.0
//! engine — the code below was lifted verbatim from the original `lib.rs`, with
//! only the shared host-facing types (`ControlCmd` / `DrawCmd` / `ScriptError`
//! / `DEFAULT_INSTRUCTION_BUDGET`) hoisted into `crate::types`.
//!
//! See the module doc on [`crate::backend`] for the contract.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Function, HookTriggers, Lua, RegistryKey, StdLib, Table, VmState};
use rustynes_core::Nes;

use crate::backend::VmBackend;
use crate::types::{ControlCmd, DEFAULT_INSTRUCTION_BUDGET, DrawCmd, MAX_QUEUED_CMDS, ScriptError};

/// A stack-allocated bitset over the 16-bit CPU address space (8 KiB, no heap),
/// used to gate the hot per-frame replay loops: membership is an O(1) bit test
/// with no `RefCell` borrow or allocation per event (gemini #58).
struct AddrBits([u64; 1024]);

impl AddrBits {
    /// Build a bitset of the keys registered in `map` (one cheap borrow).
    fn from_keys(map: &AddrCallbacks) -> Self {
        let mut bits = [0u64; 1024];
        for &addr in map.borrow().keys() {
            // `>> 6` / `& 63` (not `/ 64` / `% 64`) — idiomatic + no div/mod even
            // in debug builds (gemini #59).
            bits[usize::from(addr >> 6)] |= 1u64 << (addr & 63);
        }
        Self(bits)
    }

    /// Build a bitset only if `map` has any entries (skips the 8 KiB zero-init
    /// when the corresponding callback type is unused — gemini/Copilot #59).
    fn from_keys_opt(map: &AddrCallbacks) -> Option<Self> {
        if map.borrow().is_empty() {
            None
        } else {
            Some(Self::from_keys(map))
        }
    }

    #[inline]
    fn contains(&self, addr: u16) -> bool {
        self.0[usize::from(addr >> 6)] & (1u64 << (addr & 63)) != 0
    }
}

/// A set of callbacks registered against CPU addresses (`onExec`/`onRead`/
/// `onWrite`), stored as Lua registry keys so they live entirely Rust-side —
/// **not** in a script-visible global. A script therefore cannot inspect,
/// clobber, or inject junk into the registry, which removes the whole class of
/// "malformed registry value crashes the host pump" issues at the source.
type AddrCallbacks = Rc<RefCell<HashMap<u16, Vec<RegistryKey>>>>;

/// Push `cmd` into a host-drained queue unless it is already at the per-frame
/// cap.
fn push_capped<T>(q: &Rc<RefCell<Vec<T>>>, cmd: T) {
    let mut q = q.borrow_mut();
    if q.len() < MAX_QUEUED_CMDS {
        q.push(cmd);
    }
}

/// Side-effect-free read of `len` CPU-space bytes starting at `addr` (wrapping
/// the 16-bit space). Shared by `emu.readRange` and `memory:read_range` (B1).
/// `len` is capped to the 64 KiB address space so an unbounded request can't OOM
/// the host; `wrapping_add` avoids a debug-build overflow panic.
fn read_range_cpu(nes_cell: &RefCell<&mut Nes>, addr: u32, len: u32) -> mlua::Result<Vec<u8>> {
    if len > 0x1_0000 {
        return Err(mlua::Error::RuntimeError(
            "read range length cannot exceed 65536".into(),
        ));
    }
    let mut out = Vec::with_capacity(len as usize);
    let mut nes = nes_cell.borrow_mut();
    for i in 0..len {
        #[allow(clippy::cast_possible_truncation)]
        let a = (addr.wrapping_add(i) & 0xFFFF) as u16;
        out.push(nes.peek(a));
    }
    Ok(out)
}

/// Resolve the registry-key callbacks registered at `addr` into live Lua
/// `Function` handles (empty when none). Collecting up front releases the
/// `RefCell` borrow before any callback runs, so a callback that registers a
/// new one can't trip a re-borrow.
fn fns_at(lua: &Lua, map: &AddrCallbacks, addr: u16) -> mlua::Result<Vec<Function>> {
    let borrow = map.borrow();
    borrow.get(&addr).map_or_else(
        || Ok(Vec::new()),
        |keys| {
            keys.iter()
                .map(|k| lua.registry_value::<Function>(k))
                .collect()
        },
    )
}

/// Resolve the `onFrame` registry keys into live `Function` handles (collected
/// up front so the `RefCell` borrow is released before any callback runs).
fn fns_for_frame(lua: &Lua, frame: &Rc<RefCell<Vec<RegistryKey>>>) -> mlua::Result<Vec<Function>> {
    frame
        .borrow()
        .iter()
        .map(|k| lua.registry_value::<Function>(k))
        .collect()
}

/// A sandboxed Lua scripting engine bound to one emulator session.
pub struct MluaBackend {
    lua: Lua,
    /// Captured `print` / `emu.log` output, drained by the host for display.
    log: Rc<RefCell<Vec<String>>>,
    /// Control actions a script requested this frame (drained by the host).
    controls: Rc<RefCell<Vec<ControlCmd>>>,
    /// Overlay draw commands a script issued this frame (drained by the host).
    draws: Rc<RefCell<Vec<DrawCmd>>>,
    /// `onFrame` callbacks (Lua registry keys; Rust-side, not script-visible).
    frame_cbs: Rc<RefCell<Vec<RegistryKey>>>,
    /// `onExec(addr, fn)` callbacks, keyed by CPU address.
    exec_cbs: AddrCallbacks,
    /// `onRead(addr, fn)` callbacks, keyed by CPU address.
    read_cbs: AddrCallbacks,
    /// `onWrite(addr, fn)` callbacks, keyed by CPU address.
    write_cbs: AddrCallbacks,
    /// `onNmi(fn)` callbacks (Lua registry keys; Rust-side, not script-visible).
    /// Output-only; replayed once per NMI service entry in the interrupt log.
    nmi_cbs: Rc<RefCell<Vec<RegistryKey>>>,
    /// `onIrq(fn)` callbacks (Lua registry keys; Rust-side, not script-visible).
    /// Output-only; replayed once per IRQ/BRK service entry in the interrupt log.
    irq_cbs: Rc<RefCell<Vec<RegistryKey>>>,
    /// Per-frame instruction counter (reset each `on_frame`); the VM hook
    /// trips a Lua runtime error when it crosses `budget`.
    instr_count: Rc<Cell<u64>>,
    /// Per-frame instruction budget.
    budget: Rc<Cell<u64>>,
    /// When `true`, `emu.write` AND `emu.setInput` are silent no-ops
    /// (deterministic / locked session). Shared as an `Rc<Cell<_>>` so both the
    /// per-frame-scoped `write` accessor and the persistent `setInput` prelude
    /// function read the live value — so `setInput` is gated identically to
    /// `write` (T-110-E2), not merely at the host.
    writes_locked: Rc<Cell<bool>>,
    /// v1.5.0 B4 — `emu:on_breakpoint(addr, fn)` callbacks, keyed by CPU
    /// address. Observational: replayed from the per-frame exec-PC log exactly
    /// like `onExec` (the host arms the exec log when any are registered), so a
    /// breakpoint never intercepts mid-instruction or mutates deterministic
    /// state — it reports the PC after the fact.
    breakpoint_cbs: AddrCallbacks,
    /// v1.5.0 B4 — `emu:pause_at_frame(n)` targets. Each `on_frame` whose frame
    /// count has reached a target queues a `ControlCmd::Pause` and drops it.
    /// Observational control (the host applies the pause); never mutates the
    /// deterministic run.
    pause_frames: Rc<RefCell<Vec<u64>>>,
    /// v1.5.0 B3 — in-memory save-state slots populated by `emu:save_state(slot)`
    /// (read-only `Nes::snapshot`, always allowed) and consumed by
    /// `emu:load_state(slot)` (`Nes::restore`, GATED like `emu.write`). Distinct
    /// from the host's on-disk numbered slots: these live in the script engine
    /// for the session and are never persisted, so a TAS/analysis script can
    /// checkpoint + roll back without touching the user's save files.
    state_slots: Rc<RefCell<HashMap<u8, Vec<u8>>>>,
    /// v1.5.0 B4 — `sym:name(addr)` lookup (`address -> label`), pushed by the
    /// host from the debugger's loaded symbols. Read-only; never deterministic.
    sym_by_addr: Rc<RefCell<HashMap<u16, String>>>,
    /// v1.5.0 B4 — `sym:addr(name)` reverse lookup (`label -> address`). Built
    /// alongside [`Self::sym_by_addr`]; last writer wins on a duplicate label.
    sym_by_name: Rc<RefCell<HashMap<String, u16>>>,
}

impl MluaBackend {
    /// Install the persistent `emu` table: the callback registry, `emu.onFrame`,
    /// `emu.log`, and a `print` redirect. The live-`Nes` accessors (`read` /
    /// `write` / `cpu` / `frame` / `cycle`) are (re)bound per frame in
    /// [`Self::on_frame`] via a scope.
    #[allow(clippy::too_many_lines)] // one create_function per API entry.
    fn install_prelude(&self) -> Result<(), ScriptError> {
        let emu = self.lua.create_table()?;

        // emu.log(msg) — append to the host-visible buffer.
        let log = Rc::clone(&self.log);
        let log_fn = self
            .lua
            .create_function(move |_, msg: mlua::Variadic<mlua::Value>| {
                let mut parts = Vec::with_capacity(msg.len());
                for v in msg.iter() {
                    parts.push(value_to_string(v));
                }
                log.borrow_mut().push(parts.join("\t"));
                Ok(())
            })?;
        emu.set("log", log_fn.clone())?;

        // Control commands (collected; the host applies + gates them). Each
        // queue is capped per frame so a script can't grow host memory without
        // bound (Copilot #47); excess commands in one frame are dropped.
        let controls = Rc::clone(&self.controls);
        emu.set(
            "pause",
            self.lua.create_function(move |_, ()| {
                push_capped(&controls, ControlCmd::Pause);
                Ok(())
            })?,
        )?;
        let controls = Rc::clone(&self.controls);
        emu.set(
            "saveState",
            self.lua.create_function(move |_, slot: u8| {
                push_capped(&controls, ControlCmd::SaveState(slot));
                Ok(())
            })?,
        )?;
        let controls = Rc::clone(&self.controls);
        emu.set(
            "loadState",
            self.lua.create_function(move |_, slot: u8| {
                push_capped(&controls, ControlCmd::LoadState(slot));
                Ok(())
            })?,
        )?;
        let controls = Rc::clone(&self.controls);
        let setinput_locked = Rc::clone(&self.writes_locked);
        emu.set(
            "setInput",
            self.lua
                .create_function(move |_, (port, buttons): (u8, u8)| {
                    // T-110-E2 — gated IDENTICALLY to `emu.write`: under a locked
                    // session (netplay / TAS replay / RA-hardcore) the command is
                    // dropped at the source, so it can never reach the host's
                    // late-latch and perturb a deterministic / replayed run.
                    if !setinput_locked.get() {
                        push_capped(&controls, ControlCmd::SetInput { port, buttons });
                    }
                    Ok(())
                })?,
        )?;

        // Overlay draw commands (collected; the host renders them via egui).
        let draws = Rc::clone(&self.draws);
        emu.set(
            "drawText",
            self.lua.create_function(
                move |_, (x, y, text, color): (i32, i32, String, Option<u32>)| {
                    push_capped(
                        &draws,
                        DrawCmd::Text {
                            x,
                            y,
                            color: color.unwrap_or(0xFFFF_FFFF),
                            text,
                        },
                    );
                    Ok(())
                },
            )?,
        )?;
        let draws = Rc::clone(&self.draws);
        emu.set(
            "drawRect",
            self.lua.create_function(
                move |_, (x, y, w, h, color): (i32, i32, i32, i32, Option<u32>)| {
                    push_capped(
                        &draws,
                        DrawCmd::Rect {
                            x,
                            y,
                            w,
                            h,
                            color: color.unwrap_or(0xFFFF_FFFF),
                        },
                    );
                    Ok(())
                },
            )?,
        )?;
        let draws = Rc::clone(&self.draws);
        emu.set(
            "drawPixel",
            self.lua
                .create_function(move |_, (x, y, color): (i32, i32, Option<u32>)| {
                    push_capped(
                        &draws,
                        DrawCmd::Pixel {
                            x,
                            y,
                            color: color.unwrap_or(0xFFFF_FFFF),
                        },
                    );
                    Ok(())
                })?,
        )?;

        // Callback registrars. The handles are stored Rust-side as Lua registry
        // keys (NOT in a script-visible global), so a script can register but
        // can never inspect / clobber / inject junk into the registry — the
        // whole "malformed registry value crashes the host" class is gone.
        let frame = Rc::clone(&self.frame_cbs);
        emu.set(
            "onFrame",
            self.lua.create_function(move |lua, f: Function| {
                frame.borrow_mut().push(lua.create_registry_value(f)?);
                Ok(())
            })?,
        )?;
        // T-110-E1 — onNmi / onIrq: per-interrupt-service callbacks, replayed
        // each frame from the core's committed interrupt-service log (the same
        // Rust-side registry-key storage as onFrame, so a script can register
        // but never inspect / clobber the registry). Output-only: the callback
        // observes the service vector but cannot mutate emulation.
        for (name, list) in [("onNmi", &self.nmi_cbs), ("onIrq", &self.irq_cbs)] {
            let list = Rc::clone(list);
            emu.set(
                name,
                self.lua.create_function(move |lua, f: Function| {
                    list.borrow_mut().push(lua.create_registry_value(f)?);
                    Ok(())
                })?,
            )?;
        }
        // Address-keyed registrars (`emu.onExec`/`onRead`/`onWrite`, dot form).
        for (name, map) in [
            ("onExec", &self.exec_cbs),
            ("onRead", &self.read_cbs),
            ("onWrite", &self.write_cbs),
        ] {
            let map = Rc::clone(map);
            emu.set(
                name,
                self.lua
                    .create_function(move |lua, (addr, f): (i64, Function)| {
                        let key = lua.create_registry_value(f)?;
                        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                        let addr = (addr & 0xFFFF) as u16;
                        map.borrow_mut().entry(addr).or_default().push(key);
                        Ok(())
                    })?,
            )?;
        }

        // v1.5.0 B4 — `emu:on_breakpoint(addr, fn)` (colon form; the leading
        // `self` table is ignored). Same `(addr, fn)` Rust-side registry-key
        // storage as `onExec`, replayed from the same per-frame exec-PC log — an
        // observational breakpoint that reports the PC after the frame, never an
        // intercept and never a state mutation.
        let bp_map = Rc::clone(&self.breakpoint_cbs);
        emu.set(
            "on_breakpoint",
            self.lua.create_function(
                move |lua, (_this, addr, f): (mlua::Value, i64, Function)| {
                    let key = lua.create_registry_value(f)?;
                    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                    let addr = (addr & 0xFFFF) as u16;
                    bp_map.borrow_mut().entry(addr).or_default().push(key);
                    Ok(())
                },
            )?,
        )?;

        // v1.5.0 B4 — `emu:pause_at_frame(n)` (colon form; `self` ignored):
        // record a frame target; the next `on_frame` to reach it queues a Pause
        // control and drops the target.
        let pause_frames = Rc::clone(&self.pause_frames);
        emu.set(
            "pause_at_frame",
            self.lua
                .create_function(move |_, (_this, n): (mlua::Value, i64)| {
                    let target = u64::try_from(n).unwrap_or(0);
                    let mut pf = pause_frames.borrow_mut();
                    // Cap so a runaway loop can't grow host memory without bound.
                    if pf.len() < MAX_QUEUED_CMDS {
                        pf.push(target);
                    }
                    Ok(())
                })?,
        )?;

        // v1.5.0 B4 — the `sym` table: read-only symbol-label queries against the
        // host-pushed debugger symbol map. `sym:name(addr)` / `sym:addr(name)`
        // (colon form; the leading `self` table is ignored). Both lookups are
        // pure reads of the engine-side maps — never deterministic state.
        let sym = self.lua.create_table()?;
        let by_addr = Rc::clone(&self.sym_by_addr);
        sym.set(
            "name",
            self.lua
                .create_function(move |_, (_this, addr): (mlua::Value, i64)| {
                    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                    let addr = (addr & 0xFFFF) as u16;
                    Ok(by_addr.borrow().get(&addr).cloned())
                })?,
        )?;
        let by_name = Rc::clone(&self.sym_by_name);
        sym.set(
            "addr",
            self.lua
                .create_function(move |_, (_this, name): (mlua::Value, String)| {
                    Ok(by_name.borrow().get(&name).copied())
                })?,
        )?;
        self.lua.globals().set("sym", sym)?;

        self.lua.globals().set("emu", &emu)?;

        // v1.6.0 B3 — the `joypad` table (colon-call). `joypad:set` mirrors
        // `emu.setInput` (the same gated `ControlCmd::SetInput` path); the
        // per-frame `joypad:get(port)` read is bound in the frame scope (it
        // needs the live `Nes`), exactly as `emu.read` is.
        let joypad = self.lua.create_table()?;
        {
            let controls = Rc::clone(&self.controls);
            let locked = Rc::clone(&self.writes_locked);
            joypad.set(
                "set",
                self.lua.create_function(
                    move |_, (_this, port, buttons): (mlua::Value, u8, u8)| {
                        // Gated identically to `emu.setInput`: dropped under a
                        // locked / replayed session.
                        if !locked.get() {
                            push_capped(&controls, ControlCmd::SetInput { port, buttons });
                        }
                        Ok(())
                    },
                )?,
            )?;
        }
        self.lua.globals().set("joypad", &joypad)?;

        // Redirect base `print` to the same sink.
        self.lua.globals().set("print", log_fn)?;
        Ok(())
    }

    /// Install the per-frame instruction-budget hook (and reset the counter).
    fn arm_hook(&self) -> Result<(), ScriptError> {
        self.instr_count.set(0);
        let count = Rc::clone(&self.instr_count);
        let budget = Rc::clone(&self.budget);
        // mlua 0.11 makes `set_hook` fallible. The instruction-budget hook is
        // the sandbox's runaway-script guard, so a failed install is surfaced
        // as an error rather than silently leaving scripts uncapped.
        self.lua.set_hook(
            HookTriggers::new().every_nth_instruction(10_000),
            move |_lua, _debug| {
                let n = count.get() + 10_000;
                count.set(n);
                if n > budget.get() {
                    Err(mlua::Error::RuntimeError(
                        "script exceeded the per-frame instruction budget".into(),
                    ))
                } else {
                    Ok(VmState::Continue)
                }
            },
        )?;
        Ok(())
    }
}

impl VmBackend for MluaBackend {
    fn new() -> Result<Self, ScriptError> {
        // Only the pure, side-effect-free standard libraries.
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::COROUTINE,
            mlua::LuaOptions::default(),
        )?;

        let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let controls: Rc<RefCell<Vec<ControlCmd>>> = Rc::new(RefCell::new(Vec::new()));
        let draws: Rc<RefCell<Vec<DrawCmd>>> = Rc::new(RefCell::new(Vec::new()));
        let instr_count = Rc::new(Cell::new(0u64));
        let budget = Rc::new(Cell::new(DEFAULT_INSTRUCTION_BUDGET));

        // Remove the unsafe base globals the sandbox must not expose.
        {
            let g = lua.globals();
            for name in [
                "load",
                "loadfile",
                "dofile",
                "loadstring",
                "collectgarbage",
                "require",
                "package",
                "io",
                "os",
                "debug",
            ] {
                g.set(name, mlua::Value::Nil)?;
            }
        }

        let engine = Self {
            lua,
            log,
            controls,
            draws,
            frame_cbs: Rc::new(RefCell::new(Vec::new())),
            exec_cbs: Rc::new(RefCell::new(HashMap::new())),
            read_cbs: Rc::new(RefCell::new(HashMap::new())),
            write_cbs: Rc::new(RefCell::new(HashMap::new())),
            nmi_cbs: Rc::new(RefCell::new(Vec::new())),
            irq_cbs: Rc::new(RefCell::new(Vec::new())),
            instr_count,
            budget,
            writes_locked: Rc::new(Cell::new(false)),
            breakpoint_cbs: Rc::new(RefCell::new(HashMap::new())),
            pause_frames: Rc::new(RefCell::new(Vec::new())),
            state_slots: Rc::new(RefCell::new(HashMap::new())),
            sym_by_addr: Rc::new(RefCell::new(HashMap::new())),
            sym_by_name: Rc::new(RefCell::new(HashMap::new())),
        };
        engine.install_prelude()?;
        Ok(engine)
    }

    fn set_instruction_budget(&self, budget: u64) {
        self.budget.set(budget);
    }

    fn set_writes_locked(&self, locked: bool) {
        self.writes_locked.set(locked);
    }

    fn set_symbols(&self, pairs: &[(u16, String)]) {
        let mut by_addr = self.sym_by_addr.borrow_mut();
        let mut by_name = self.sym_by_name.borrow_mut();
        by_addr.clear();
        by_name.clear();
        for (addr, name) in pairs {
            by_addr.insert(*addr, name.clone());
            by_name.insert(name.clone(), *addr);
        }
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

    fn needs_exec_log(&self) -> bool {
        // `on_breakpoint` replays from the same per-frame exec-PC log as
        // `onExec`, so either kind of registration arms it (B4).
        !self.exec_cbs.borrow().is_empty() || !self.breakpoint_cbs.borrow().is_empty()
    }

    fn needs_access_log(&self) -> bool {
        !self.read_cbs.borrow().is_empty() || !self.write_cbs.borrow().is_empty()
    }

    fn needs_interrupt_log(&self) -> bool {
        !self.nmi_cbs.borrow().is_empty() || !self.irq_cbs.borrow().is_empty()
    }

    fn load(&mut self, src: &str) -> Result<(), ScriptError> {
        self.arm_hook()?;
        let r = self.lua.load(src).exec().map_err(ScriptError::from);
        self.lua.remove_hook();
        r
    }

    #[allow(clippy::too_many_lines)] // scoped accessor binding + replay loops.
    fn on_frame(&mut self, nes: &mut Nes) -> Result<(), ScriptError> {
        let frame = nes.frame();
        let cycle = nes.cycle();
        let writes_locked = self.writes_locked.get();

        // Snapshot the just-finished frame's exec PCs + bus accesses (owned, so
        // they don't tie up the `nes` borrow inside the scope) for the
        // onExec / onRead / onWrite replay. `exec_log` is the dedicated
        // per-frame log (cleared each frame) — NOT the rolling trace buffer, so
        // there are no stale / duplicate PCs (gemini #47). Both are empty unless
        // the matching callbacks are registered (so the gate is free when off).
        let want_exec = self.needs_exec_log();
        let want_access = self.needs_access_log();
        let want_interrupt = self.needs_interrupt_log();
        let exec_pcs: Vec<u16> = if want_exec {
            nes.exec_log().to_vec()
        } else {
            Vec::new()
        };
        let accesses: Vec<(bool, u16, u8)> = if want_access {
            nes.accesses()
                .iter()
                .map(|a| (a.write, a.addr, a.value))
                .collect()
        } else {
            Vec::new()
        };
        // Snapshot this frame's committed interrupt services (owned, so the
        // `nes` borrow is free inside the scope). Each is `(is_nmi, vector)`;
        // replayed through onNmi / onIrq below. Empty unless onNmi/onIrq exist.
        let interrupts: Vec<(bool, u16)> = if want_interrupt {
            nes.interrupt_log()
                .iter()
                .map(|i| (i.is_nmi, i.vector))
                .collect()
        } else {
            Vec::new()
        };

        // v1.5.0 B2 — read-only cart / system metadata, captured once before the
        // scope (cheap; all `const`/O(1) on the core). `sha256` is the lowercase
        // hex of the ROM's SHA-256 (matches the host's save-state directory key).
        let cart_mapper_id = nes.mapper_id();
        let cart_prg_size = nes.prg_rom_len() as u64;
        let cart_chr_size = nes.chr_rom_len() as u64;
        let cart_region: &'static str = match nes.region() {
            rustynes_core::Region::Pal => "PAL",
            rustynes_core::Region::Dendy => "Dendy",
            rustynes_core::Region::Ntsc => "NTSC",
        };
        let cart_sha256 = {
            let mut s = String::with_capacity(64);
            for b in nes.rom_sha256() {
                use core::fmt::Write as _;
                let _ = write!(s, "{b:02x}");
            }
            s
        };

        let nes_cell = RefCell::new(nes);
        let lua = &self.lua;
        // Rust-side callback registries (clones of the `Rc`s) — used inside the
        // scope without aliasing `self`.
        let frame_cbs = Rc::clone(&self.frame_cbs);
        let exec_cbs = Rc::clone(&self.exec_cbs);
        let read_cbs = Rc::clone(&self.read_cbs);
        let write_cbs = Rc::clone(&self.write_cbs);
        let nmi_cbs = Rc::clone(&self.nmi_cbs);
        let irq_cbs = Rc::clone(&self.irq_cbs);
        let breakpoint_cbs = Rc::clone(&self.breakpoint_cbs);
        let state_slots = Rc::clone(&self.state_slots);
        let controls = Rc::clone(&self.controls);
        // v1.5.0 B4 — drain the pause-at-frame targets reached this frame, OUTSIDE
        // the scope (no `nes` access): each reached target queues a Pause control.
        {
            let mut pf = self.pause_frames.borrow_mut();
            if !pf.is_empty() {
                pf.retain(|&target| {
                    if frame >= target {
                        push_capped(&controls, ControlCmd::Pause);
                        false
                    } else {
                        true
                    }
                });
            }
        }

        self.instr_count.set(0);
        let count = Rc::clone(&self.instr_count);
        let budget = Rc::clone(&self.budget);
        // mlua 0.11: `set_hook` is fallible. Surface a failed install as an
        // error rather than silently running the frame's callbacks without the
        // runaway-script budget guard (see arm_hook).
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(10_000),
            move |_lua, _debug| {
                let n = count.get() + 10_000;
                count.set(n);
                if n > budget.get() {
                    Err(mlua::Error::RuntimeError(
                        "script exceeded the per-frame instruction budget".into(),
                    ))
                } else {
                    Ok(VmState::Continue)
                }
            },
        )?;

        let result = lua.scope(|scope| {
            let emu: Table = lua.globals().get("emu")?;

            let read =
                scope.create_function(|_, addr: u16| Ok(nes_cell.borrow_mut().peek(addr)))?;
            emu.set("read", read)?;

            // v1.6.0 B3 — bind the per-frame `joypad:get(port)` read against the
            // live `Nes` (`joypad:set` was installed on the table in the prelude).
            // Returns the latched standard-controller bitmask (port 0=P1 .. 3),
            // side-effect-free (reads the latch, not the shift register).
            let joypad: Table = lua.globals().get("joypad")?;
            joypad.set(
                "get",
                scope.create_function(|_, (_this, port): (mlua::Value, u16)| {
                    Ok(nes_cell.borrow().controller_buttons((port & 0x03) as usize))
                })?,
            )?;

            let read_range = scope.create_function(|_, (addr, len): (u32, u32)| {
                read_range_cpu(&nes_cell, addr, len)
            })?;
            emu.set("readRange", read_range)?;

            let write = scope.create_function(|_, (addr, val): (u16, u8)| {
                if !writes_locked {
                    nes_cell.borrow_mut().poke_ram(addr, val);
                }
                Ok(())
            })?;
            emu.set("write", write)?;

            let cpu = scope.create_function(|lua, ()| {
                let nes = nes_cell.borrow();
                let c = nes.cpu();
                let t = lua.create_table()?;
                t.set("a", c.a)?;
                t.set("x", c.x)?;
                t.set("y", c.y)?;
                t.set("s", c.s)?;
                t.set("p", c.p.bits())?;
                t.set("pc", c.pc)?;
                Ok(t)
            })?;
            emu.set("cpu", cpu)?;

            emu.set("frame", frame)?;
            emu.set("cycle", cycle)?;

            // v1.5.0 B1 — the `memory` table: explicit CPU + PPU space access.
            // Colon-call form (`memory:peek(addr)`); the leading `self` table is
            // ignored. Reads use the side-effect-free debug-peek path ($2002 does
            // NOT clear VBL, $2007 does NOT advance the read buffer), so observing
            // memory never perturbs the deterministic run. `poke`/`write_range`
            // are GATED identically to `emu.write` (system RAM only, dropped under
            // a locked / replayed session).
            let memory = lua.create_table()?;
            memory.set(
                "peek",
                scope.create_function(|_, (_this, addr): (mlua::Value, u16)| {
                    Ok(nes_cell.borrow_mut().peek(addr))
                })?,
            )?;
            memory.set(
                "peek_ppu",
                scope.create_function(|_, (_this, addr): (mlua::Value, u16)| {
                    // PPU bus is $0000-$3FFF (mirrored to $4000); mask to 14 bits.
                    Ok(nes_cell.borrow_mut().ppu_bus_peek(addr & 0x3FFF))
                })?,
            )?;
            memory.set(
                "read_range",
                scope.create_function(|_, (_this, addr, len): (mlua::Value, u32, u32)| {
                    read_range_cpu(&nes_cell, addr, len)
                })?,
            )?;
            memory.set(
                "read_range_ppu",
                scope.create_function(|_, (_this, addr, len): (mlua::Value, u32, u32)| {
                    if len > 0x4000 {
                        return Err(mlua::Error::RuntimeError(
                            "memory:read_range_ppu length cannot exceed 16384".into(),
                        ));
                    }
                    let mut out = Vec::with_capacity(len as usize);
                    let mut nes = nes_cell.borrow_mut();
                    for i in 0..len {
                        #[allow(clippy::cast_possible_truncation)]
                        let a = (addr.wrapping_add(i) & 0x3FFF) as u16;
                        out.push(nes.ppu_bus_peek(a));
                    }
                    Ok(out)
                })?,
            )?;
            // v1.6.0 B3 — sized reads. Two CPU-bus `peek`s composed little- or
            // big-endian; observational (peek never perturbs the run), the
            // common TAS-script need for 16-bit values (positions, timers).
            memory.set(
                "read_u16_le",
                scope.create_function(|_, (_this, addr): (mlua::Value, u16)| {
                    let mut nes = nes_cell.borrow_mut();
                    Ok(u16::from_le_bytes([
                        nes.peek(addr),
                        nes.peek(addr.wrapping_add(1)),
                    ]))
                })?,
            )?;
            memory.set(
                "read_u16_be",
                scope.create_function(|_, (_this, addr): (mlua::Value, u16)| {
                    let mut nes = nes_cell.borrow_mut();
                    Ok(u16::from_be_bytes([
                        nes.peek(addr),
                        nes.peek(addr.wrapping_add(1)),
                    ]))
                })?,
            )?;
            // v1.6.0 B3 — OAM domain (sprite memory). The third read domain
            // alongside CPU (`peek`) and PPU (`peek_ppu`). `Nes::oam_byte` reads
            // one byte without copying the whole 256-byte array, so iterating
            // OAM in a script doesn't pay a full snapshot per access. Index
            // wraps to 8 bits.
            memory.set(
                "read_oam",
                scope.create_function(|_, (_this, index): (mlua::Value, u16)| {
                    #[allow(clippy::cast_possible_truncation)]
                    Ok(nes_cell.borrow().oam_byte((index & 0xFF) as u8))
                })?,
            )?;
            memory.set(
                "poke",
                scope.create_function(|_, (_this, addr, val): (mlua::Value, u16, u8)| {
                    if !writes_locked {
                        nes_cell.borrow_mut().poke_ram(addr, val);
                    }
                    Ok(())
                })?,
            )?;
            memory.set(
                "write_range",
                scope.create_function(|_, (_this, addr, bytes): (mlua::Value, u32, Vec<u8>)| {
                    if !writes_locked {
                        let mut nes = nes_cell.borrow_mut();
                        for (i, b) in bytes.iter().enumerate() {
                            #[allow(clippy::cast_possible_truncation)]
                            let a = (addr.wrapping_add(i as u32) & 0xFFFF) as u16;
                            nes.poke_ram(a, *b);
                        }
                    }
                    Ok(())
                })?,
            )?;
            lua.globals().set("memory", &memory)?;

            // v1.5.0 B2 — the `cart` table: read-only cart / system queries
            // (colon form; `self` ignored). All values are captured pre-scope,
            // so these are cheap constant returns that never touch the core.
            let cart = lua.create_table()?;
            cart.set(
                "mapper_id",
                lua.create_function(move |_, _this: mlua::Value| Ok(cart_mapper_id))?,
            )?;
            cart.set(
                "prg_size",
                lua.create_function(move |_, _this: mlua::Value| Ok(cart_prg_size))?,
            )?;
            cart.set(
                "chr_size",
                lua.create_function(move |_, _this: mlua::Value| Ok(cart_chr_size))?,
            )?;
            cart.set(
                "region",
                lua.create_function(move |_, _this: mlua::Value| Ok(cart_region))?,
            )?;
            cart.set("frame", frame)?;
            {
                let sha = cart_sha256.clone();
                cart.set(
                    "sha256",
                    lua.create_function(move |_, _this: mlua::Value| Ok(sha.clone()))?,
                )?;
            }
            lua.globals().set("cart", &cart)?;

            // v1.5.0 B3 — in-memory save-state slots on `emu`. `save_state(slot)`
            // is a read-only `Nes::snapshot` (always allowed). `load_state(slot)`
            // applies a stored snapshot via `Nes::restore` and is GATED IDENTICALLY
            // to `emu.write`: under a locked / replayed session it is a silent
            // no-op, so a deterministic / netplay / RA-hardcore run is unperturbed.
            // Distinct from the host's on-disk numbered slots (`emu.saveState`).
            emu.set(
                "save_state",
                scope.create_function(|_, (_this, slot): (mlua::Value, u8)| {
                    let blob = nes_cell.borrow().snapshot();
                    state_slots.borrow_mut().insert(slot, blob);
                    Ok(())
                })?,
            )?;
            emu.set(
                "load_state",
                scope.create_function(|_, (_this, slot): (mlua::Value, u8)| {
                    if writes_locked {
                        return Ok(false);
                    }
                    let blob = state_slots.borrow().get(&slot).cloned();
                    // A restore failure (e.g. an empty slot mid-session) is
                    // surfaced as `false`, never a host crash.
                    blob.map_or_else(
                        || Ok(false),
                        |blob| Ok(nes_cell.borrow_mut().restore(&blob).is_ok()),
                    )
                })?,
            )?;

            // Invoke every registered onFrame callback (from the Rust-side
            // registry — scripts cannot touch or corrupt it).
            for f in fns_for_frame(lua, &frame_cbs)? {
                f.call::<()>(())?;
            }

            // Replay this frame's committed interrupt services through
            // onNmi(vector) / onIrq(vector). Output-only; in service order.
            // `fns_for_frame` works for these flat Vec<RegistryKey> lists too.
            if !interrupts.is_empty() {
                for (is_nmi, vector) in &interrupts {
                    let list = if *is_nmi { &nmi_cbs } else { &irq_cbs };
                    for f in fns_for_frame(lua, list)? {
                        f.call::<()>(*vector)?;
                    }
                }
            }

            // Gate the hot replay loops on a stack-allocated bitset of the
            // registered addresses (covers the full 16-bit space in 8 KiB, no
            // heap allocation, no per-event `RefCell` borrow). Built only when
            // the matching loop will run, so there is zero cost unless a script
            // registers the corresponding callback (gemini #47/#57/#58). The
            // loops run ~15k (exec) + ~60k (access) times per frame.

            // Replay this frame's exec PCs through onExec(addr).
            if !exec_pcs.is_empty() {
                let active = AddrBits::from_keys(&exec_cbs);
                for pc in &exec_pcs {
                    if active.contains(*pc) {
                        for f in fns_at(lua, &exec_cbs, *pc)? {
                            f.call::<()>(*pc)?;
                        }
                    }
                }
            }

            // v1.5.0 B4 — replay this frame's exec PCs through on_breakpoint(pc).
            // Same observational exec-log replay as onExec, kept on a separate map
            // so a script can use breakpoints and onExec independently. Built only
            // when a breakpoint is registered (zero cost otherwise).
            if !exec_pcs.is_empty() && !breakpoint_cbs.borrow().is_empty() {
                let active = AddrBits::from_keys(&breakpoint_cbs);
                for pc in &exec_pcs {
                    if active.contains(*pc) {
                        for f in fns_at(lua, &breakpoint_cbs, *pc)? {
                            f.call::<()>(*pc)?;
                        }
                    }
                }
            }

            // Replay this frame's bus accesses through onRead/onWrite(addr, value).
            // Build each bitset only for a callback type that's actually in use
            // (most scripts watch only reads OR writes) — gemini/Copilot #59.
            if !accesses.is_empty() {
                let active_read = AddrBits::from_keys_opt(&read_cbs);
                let active_write = AddrBits::from_keys_opt(&write_cbs);
                // Resolve the `Option` discriminant once (these are 8 KiB enums);
                // the per-access check is then a cheap `Option<&AddrBits>` pointer
                // test rather than re-reading the discriminant 60k times (gemini #61).
                let (active_read, active_write) = (active_read.as_ref(), active_write.as_ref());
                for (is_write, addr, value) in &accesses {
                    let (active, map) = if *is_write {
                        (active_write, &write_cbs)
                    } else {
                        (active_read, &read_cbs)
                    };
                    if active.is_some_and(|a| a.contains(*addr)) {
                        for f in fns_at(lua, map, *addr)? {
                            f.call::<()>((*addr, *value))?;
                        }
                    }
                }
            }
            Ok(())
        });

        lua.remove_hook();
        result.map_err(ScriptError::from)
    }

    fn frame_callback_count(&self) -> usize {
        self.frame_cbs.borrow().len()
    }
}

/// Best-effort `Value` -> display string for the log sink.
fn value_to_string(v: &mlua::Value) -> String {
    match v {
        mlua::Value::String(s) => s.to_string_lossy(),
        mlua::Value::Integer(i) => i.to_string(),
        mlua::Value::Number(n) => n.to_string(),
        mlua::Value::Boolean(b) => b.to_string(),
        mlua::Value::Nil => "nil".to_owned(),
        // Tables / functions / userdata: render `tostring`-style ("table",
        // "function", ...) rather than a noisy `{:?}` debug dump (L4).
        other => other.type_name().to_owned(),
    }
}

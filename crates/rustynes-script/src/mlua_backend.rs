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

        self.lua.globals().set("emu", &emu)?;
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
        !self.exec_cbs.borrow().is_empty()
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

            let read_range = scope.create_function(|_, (addr, len): (u32, u32)| {
                // Cap to the 64 KiB CPU address space — an unbounded `len` would
                // otherwise let a script OOM the host (gemini/Copilot #46).
                // `wrapping_add` avoids a debug-build overflow panic.
                if len > 0x1_0000 {
                    return Err(mlua::Error::RuntimeError(
                        "emu.readRange length cannot exceed 65536".into(),
                    ));
                }
                let mut out = Vec::with_capacity(len as usize);
                let mut nes = nes_cell.borrow_mut();
                for i in 0..len {
                    out.push(nes.peek((addr.wrapping_add(i) & 0xFFFF) as u16));
                }
                Ok(out)
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

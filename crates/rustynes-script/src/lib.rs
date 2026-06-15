//! Sandboxed Lua 5.4 scripting engine for `RustyNES` (v1.1.0, Workstream E).
//!
//! Embeds [`mlua`] (vendored Lua 5.4) and exposes a small, Mesen2 / FCEUX-style
//! `emu` API to user scripts: read / write CPU-bus memory, inspect CPU
//! registers + the frame / cycle counters, log messages, and register
//! per-frame callbacks. The engine is **driven by the host** (the frontend),
//! never the other way around: the host calls [`ScriptEngine::on_frame`] once
//! per emulated frame, which binds the live-`Nes` accessors and invokes every
//! registered Lua `onFrame` handler.
//!
//! ## Determinism + safety
//!
//! - The default build does **not** pull this crate in (the frontend's
//!   `scripting` feature is off by default), so the shipped emulator is
//!   byte-identical and carries no Lua/`cc` dependency unless scripting is
//!   explicitly enabled.
//! - **Sandbox:** only the `table` / `string` / `math` / `coroutine` standard
//!   libraries load — no `io`, `os`, `package`, `require`, `debug`. The unsafe
//!   base globals (`load`, `loadfile`, `dofile`, `loadstring`, `collectgarbage`)
//!   are removed. `print` is kept but redirected to the captured log.
//! - **Budget guard:** an instruction-count hook aborts a callback that runs
//!   away (default [`DEFAULT_INSTRUCTION_BUDGET`] VM instructions per frame).
//! - **Write gating:** when the host sets [`ScriptEngine::set_writes_locked`]
//!   (netplay / TAS replay / RA-hardcore), `emu.write` becomes a silent no-op,
//!   so a script cannot perturb a deterministic / locked session — the same
//!   policy as the Game Genie / raw-RAM cheat path.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use mlua::{HookTriggers, Lua, StdLib, Table, VmState};
use rustynes_core::Nes;

/// Default per-frame VM-instruction budget. A callback that exceeds this is
/// aborted with a Lua runtime error (a runaway-loop backstop) — surfaced as
/// [`ScriptError::Lua`].
///
/// The host pumps the engine while holding the emulator lock (callbacks need
/// live `Nes` access), so this budget also bounds how long a runaway script can
/// stall emulation (M2). 1M VM instructions is ~10 ms worst case — well above
/// any legitimate per-frame script (real HUD/watch logic is well under 10k
/// instructions/frame), but tight enough that a runaway is cut off within a
/// frame or two rather than freezing the emulator. Raise it via
/// [`ScriptEngine::set_instruction_budget`] for unusual workloads.
pub const DEFAULT_INSTRUCTION_BUDGET: u64 = 1_000_000;

/// Max control / draw commands queued per frame (drained by the host). A script
/// can't grow host memory without bound; excess commands in one frame are
/// dropped (Copilot #47).
const MAX_QUEUED_CMDS: usize = 8192;

/// Push `cmd` into a host-drained queue unless it is already at the per-frame
/// cap.
fn push_capped<T>(q: &Rc<RefCell<Vec<T>>>, cmd: T) {
    let mut q = q.borrow_mut();
    if q.len() < MAX_QUEUED_CMDS {
        q.push(cmd);
    }
}

/// Read `t[key]` as a table, returning `None` if it is absent, `nil`, a
/// non-table value, or the lookup itself errors (e.g. a hostile `_G` metatable).
/// All registry access goes through this so a script can never error the host
/// pump by clobbering the registry with a junk value (M1 / gemini+Copilot #52).
fn table_field<K: mlua::IntoLua>(t: &Table, key: K) -> Option<Table> {
    t.get::<mlua::Value>(key).ok()?.as_table().cloned()
}

/// Errors from loading or running a script.
#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    /// The Lua chunk failed to load (syntax error), a callback raised, or the
    /// per-frame instruction budget was exceeded (a Lua runtime error).
    #[error("lua error: {0}")]
    Lua(#[from] mlua::Error),
}

/// A control action a script requested (`emu.pause` / `saveState` / ...).
///
/// Drained by the host after [`ScriptEngine::on_frame`] and applied to the
/// emulator. Collected (not applied inline) so the host stays the single owner
/// of emulator-control + can gate state-mutating actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlCmd {
    /// `emu.pause()` — request the host pause emulation.
    Pause,
    /// `emu.saveState(slot)` — save to a numbered slot.
    SaveState(u8),
    /// `emu.loadState(slot)` — load from a numbered slot.
    LoadState(u8),
    /// `emu.setInput(port, buttons)` — override a controller's button bitmask
    /// for the next frame (`port` 0/1; `buttons` is the standard NES bitmask).
    SetInput {
        /// Controller port (0 = P1, 1 = P2).
        port: u8,
        /// Standard NES button bitmask (A,B,Select,Start,Up,Down,Left,Right).
        buttons: u8,
    },
}

/// One overlay draw command (`emu.drawText` / `drawRect` / `drawPixel`).
///
/// Drained by the host each frame and rendered through the egui pass. Pixel
/// coordinates are in NES framebuffer space (256x240). `color` is `0xRRGGBBAA`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawCmd {
    /// Text at `(x, y)`.
    Text {
        /// X (px).
        x: i32,
        /// Y (px).
        y: i32,
        /// `0xRRGGBBAA`.
        color: u32,
        /// The string.
        text: String,
    },
    /// Filled rectangle.
    Rect {
        /// X (px).
        x: i32,
        /// Y (px).
        y: i32,
        /// Width (px).
        w: i32,
        /// Height (px).
        h: i32,
        /// `0xRRGGBBAA`.
        color: u32,
    },
    /// A single pixel.
    Pixel {
        /// X (px).
        x: i32,
        /// Y (px).
        y: i32,
        /// `0xRRGGBBAA`.
        color: u32,
    },
}

/// A sandboxed Lua scripting engine bound to one emulator session.
pub struct ScriptEngine {
    lua: Lua,
    /// Captured `print` / `emu.log` output, drained by the host for display.
    log: Rc<RefCell<Vec<String>>>,
    /// Control actions a script requested this frame (drained by the host).
    controls: Rc<RefCell<Vec<ControlCmd>>>,
    /// Overlay draw commands a script issued this frame (drained by the host).
    draws: Rc<RefCell<Vec<DrawCmd>>>,
    /// Per-frame instruction counter (reset each `on_frame`); the VM hook
    /// trips a Lua runtime error when it crosses `budget`.
    instr_count: Rc<Cell<u64>>,
    /// Per-frame instruction budget.
    budget: Rc<Cell<u64>>,
    /// When `true`, `emu.write` is a no-op (deterministic / locked session).
    writes_locked: bool,
}

impl ScriptEngine {
    /// Build a fresh sandboxed engine (no script loaded yet).
    ///
    /// # Errors
    ///
    /// Returns [`ScriptError`] if the sandbox prelude fails to install.
    pub fn new() -> Result<Self, ScriptError> {
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
            instr_count,
            budget,
            writes_locked: false,
        };
        engine.install_prelude()?;
        Ok(engine)
    }

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
        emu.set(
            "setInput",
            self.lua
                .create_function(move |_, (port, buttons): (u8, u8)| {
                    push_capped(&controls, ControlCmd::SetInput { port, buttons });
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

        self.lua.globals().set("emu", &emu)?;
        // Redirect base `print` to the same sink.
        self.lua.globals().set("print", log_fn)?;

        // Callback registries + the on* registrars, written in Lua to keep
        // handles entirely Lua-side (no Rust RegistryKey juggling). The exec /
        // read / write tables are address-keyed lists of callbacks.
        self.lua
            .load(
                r"
                __rustynes = { frame = {}, exec = {}, read = {}, write = {} }
                function emu.onFrame(f)
                    assert(type(f) == 'function', 'emu.onFrame expects a function')
                    __rustynes.frame[#__rustynes.frame + 1] = f
                end
                local function reg(tbl, addr, f)
                    assert(type(f) == 'function', 'callback must be a function')
                    addr = addr & 0xFFFF
                    tbl[addr] = tbl[addr] or {}
                    tbl[addr][#tbl[addr] + 1] = f
                end
                function emu.onExec(addr, f)  reg(__rustynes.exec,  addr, f) end
                function emu.onRead(addr, f)  reg(__rustynes.read,  addr, f) end
                function emu.onWrite(addr, f) reg(__rustynes.write, addr, f) end
                ",
            )
            .exec()?;
        Ok(())
    }

    /// Set the per-frame VM-instruction budget (runaway-loop guard).
    pub fn set_instruction_budget(&self, budget: u64) {
        self.budget.set(budget);
    }

    /// Gate `emu.write`: when `true` (netplay / TAS replay / RA-hardcore) writes
    /// are silently dropped so a script cannot perturb a locked session.
    pub const fn set_writes_locked(&mut self, locked: bool) {
        self.writes_locked = locked;
    }

    /// Drain captured log / `print` output (oldest first).
    pub fn drain_log(&self) -> Vec<String> {
        std::mem::take(&mut self.log.borrow_mut())
    }

    /// Drain the control actions requested since the last call. The host
    /// applies (and gates) them after [`Self::on_frame`].
    pub fn drain_controls(&self) -> Vec<ControlCmd> {
        std::mem::take(&mut self.controls.borrow_mut())
    }

    /// Drain the overlay draw commands issued this frame (host renders them).
    pub fn drain_draws(&self) -> Vec<DrawCmd> {
        std::mem::take(&mut self.draws.borrow_mut())
    }

    /// `true` if any `onExec` callback is registered — the host should enable
    /// [`rustynes_core::Nes::set_exec_logging`] so the next frame's exec PCs
    /// are captured for replay.
    ///
    /// # Errors
    ///
    /// Returns [`ScriptError::Lua`] if the registry table is malformed.
    pub fn needs_exec_log(&self) -> Result<bool, ScriptError> {
        self.subtable_has_entries("exec")
    }

    /// `true` if any `onRead`/`onWrite` callback is registered — the host
    /// should enable [`rustynes_core::Nes::set_access_logging`].
    ///
    /// # Errors
    ///
    /// Returns [`ScriptError::Lua`] if a registry table is malformed.
    pub fn needs_access_log(&self) -> Result<bool, ScriptError> {
        Ok(self.subtable_has_entries("read")? || self.subtable_has_entries("write")?)
    }

    /// Resilient access to the internal `__rustynes` callback registry (M1).
    ///
    /// The registry lives as a Lua global, so a buggy / hostile script could
    /// reassign it (`__rustynes = nil`). Rather than let that error out the
    /// whole pump, every accessor goes through here: a missing or wrong-typed
    /// registry resolves to `None`, which the callers treat as "no callbacks
    /// registered" — so a script can only disable *its own* callbacks, never
    /// break the host loop. (A deeper isolation would move the registry into the
    /// protected Lua registry / Rust-side storage; this graceful degradation
    /// closes the actual failure mode at far lower risk.)
    fn registry_table(&self) -> Option<Table> {
        table_field(&self.lua.globals(), "__rustynes")
    }

    /// Fetch a `__rustynes.<name>` address-keyed sub-table, or `None` if the
    /// registry or sub-table is missing / wrong-typed.
    fn registry_subtable(&self, name: &str) -> Option<Table> {
        table_field(&self.registry_table()?, name)
    }

    /// Whether `__rustynes.<name>` (an address-keyed table) holds any callback.
    /// A missing registry is "no callbacks" (M1), not an error.
    fn subtable_has_entries(&self, name: &str) -> Result<bool, ScriptError> {
        let Some(t) = self.registry_subtable(name) else {
            return Ok(false);
        };
        match t.pairs::<mlua::Value, mlua::Value>().next() {
            Some(Ok(_)) => Ok(true),
            Some(Err(e)) => Err(ScriptError::Lua(e)),
            None => Ok(false),
        }
    }

    /// Snapshot the integer (address) keys of `__rustynes.<name>` into a Rust
    /// set, so the per-frame replay can gate the (expensive) Lua table lookup
    /// behind an O(1) Rust check — avoiding a Lua FFI crossing for every one of
    /// the ~15k exec PCs / ~60k bus accesses that has no callback (gemini #47).
    /// A missing registry yields an empty set (M1).
    fn active_addrs(&self, name: &str) -> Result<HashSet<u16>, ScriptError> {
        let mut set = HashSet::new();
        let Some(t) = self.registry_subtable(name) else {
            return Ok(set);
        };
        // Iterate keys as generic values: a junk non-table value parked at an
        // address must not error the key scan (gemini #52). The gate only needs
        // the address; the replay validates that the slot is actually a table.
        for pair in t.pairs::<u32, mlua::Value>() {
            let (addr, _) = pair?;
            set.insert((addr & 0xFFFF) as u16);
        }
        Ok(set)
    }

    /// Load (and execute the top level of) a Lua script. Top-level code
    /// typically registers callbacks via `emu.onFrame(...)`.
    ///
    /// # Errors
    ///
    /// Returns [`ScriptError::Lua`] on a syntax or top-level runtime error.
    pub fn load(&self, src: &str) -> Result<(), ScriptError> {
        self.arm_hook();
        let r = self.lua.load(src).exec().map_err(ScriptError::from);
        self.lua.remove_hook();
        r
    }

    /// Install the per-frame instruction-budget hook (and reset the counter).
    fn arm_hook(&self) {
        self.instr_count.set(0);
        let count = Rc::clone(&self.instr_count);
        let budget = Rc::clone(&self.budget);
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
        );
    }

    /// Run one emulated frame's worth of scripting: bind the live-`Nes`
    /// accessors and invoke every registered `onFrame` callback.
    ///
    /// `read` / `readRange` / `cpu` observe `nes`; `write` pokes system RAM
    /// (unless writes are locked). The accessors are valid only for the
    /// duration of a callback (they borrow `nes`), so a script must do its
    /// work inside `emu.onFrame`.
    ///
    /// # Errors
    ///
    /// Returns [`ScriptError`] if a callback raises or busts the budget.
    #[allow(clippy::too_many_lines)] // scoped accessor binding + replay loops.
    pub fn on_frame(&mut self, nes: &mut Nes) -> Result<(), ScriptError> {
        let frame = nes.frame();
        let cycle = nes.cycle();
        let writes_locked = self.writes_locked;

        // Snapshot the just-finished frame's exec PCs + bus accesses (owned, so
        // they don't tie up the `nes` borrow inside the scope) for the
        // onExec / onRead / onWrite replay. `exec_log` is the dedicated
        // per-frame log (cleared each frame) — NOT the rolling trace buffer, so
        // there are no stale / duplicate PCs (gemini #47). Both are empty unless
        // the host enabled the matching log per `needs_exec_log` / `..access..`.
        let exec_addrs = self.active_addrs("exec")?;
        let read_addrs = self.active_addrs("read")?;
        let write_addrs = self.active_addrs("write")?;
        let exec_pcs: Vec<u16> = if exec_addrs.is_empty() {
            Vec::new()
        } else {
            nes.exec_log().to_vec()
        };
        let accesses: Vec<(bool, u16, u8)> = if read_addrs.is_empty() && write_addrs.is_empty() {
            Vec::new()
        } else {
            nes.accesses()
                .iter()
                .map(|a| (a.write, a.addr, a.value))
                .collect()
        };

        let nes_cell = RefCell::new(nes);
        let lua = &self.lua;

        self.instr_count.set(0);
        let count = Rc::clone(&self.instr_count);
        let budget = Rc::clone(&self.budget);
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
        );

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

            // The internal registry; a script that clobbered `__rustynes` (even
            // with a hostile `_G` metatable) simply has no callbacks this frame
            // (M1 — graceful via `table_field`, never an error; Copilot #52).
            let Some(registry) = table_field(&lua.globals(), "__rustynes") else {
                return Ok(());
            };

            // Invoke every registered onFrame callback.
            if let Some(frame_cbs) = table_field(&registry, "frame") {
                for cb in frame_cbs.sequence_values::<mlua::Function>() {
                    cb?.call::<()>(())?;
                }
            }

            // Replay this frame's exec PCs through onExec(addr). The Rust-side
            // `exec_addrs` set gates the Lua lookup: only PCs with a registered
            // callback cross the FFI boundary (gemini #47).
            if let (false, Some(exec_t)) = (exec_pcs.is_empty(), table_field(&registry, "exec")) {
                for pc in &exec_pcs {
                    if !exec_addrs.contains(pc) {
                        continue;
                    }
                    // `table_field`: a slot a callback unregistered (or overwrote
                    // with a non-table) *this frame* leaves the snapshot set
                    // stale — skip it rather than crash (gemini #49/#52).
                    if let Some(cbs) = table_field(&exec_t, *pc) {
                        for cb in cbs.sequence_values::<mlua::Function>() {
                            cb?.call::<()>(*pc)?;
                        }
                    }
                }
            }

            // Replay this frame's bus accesses through onRead/onWrite(addr, value),
            // gated the same way.
            if !accesses.is_empty() {
                let read_t = table_field(&registry, "read");
                let write_t = table_field(&registry, "write");
                for (is_write, addr, value) in &accesses {
                    let (set, t) = if *is_write {
                        (&write_addrs, &write_t)
                    } else {
                        (&read_addrs, &read_t)
                    };
                    if !set.contains(addr) {
                        continue;
                    }
                    let Some(t) = t.as_ref() else { continue };
                    // `table_field`: tolerate a slot unregistered / overwritten
                    // with a non-table mid-frame (gemini #49/#52).
                    if let Some(cbs) = table_field(t, *addr) {
                        for cb in cbs.sequence_values::<mlua::Function>() {
                            cb?.call::<()>((*addr, *value))?;
                        }
                    }
                }
            }
            Ok(())
        });

        lua.remove_hook();
        result.map_err(ScriptError::from)
    }

    /// Number of registered `onFrame` callbacks (for the host UI / tests). A
    /// missing / clobbered registry counts as zero (M1).
    #[must_use]
    pub fn frame_callback_count(&self) -> usize {
        self.registry_subtable("frame").map_or(0, |t| t.raw_len())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal NROM ROM whose reset vector loops `JMP $C000`.
    fn synth_rom() -> Vec<u8> {
        let mut bytes = vec![b'N', b'E', b'S', 0x1A, 1, 1, 0, 0];
        bytes.resize(16, 0);
        let mut prg = vec![0u8; 16 * 1024];
        prg[0] = 0x4C; // JMP $C000
        prg[1] = 0x00;
        prg[2] = 0xC0;
        let len = prg.len();
        prg[len - 4] = 0x00; // reset vector lo
        prg[len - 3] = 0xC0; // reset vector hi
        bytes.extend_from_slice(&prg);
        bytes.resize(16 + 16 * 1024 + 8 * 1024, 0); // 8 KiB CHR
        bytes
    }

    #[test]
    fn loads_and_runs_on_frame_callback() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            count = 0
            emu.onFrame(function() count = count + 1; emu.log('tick ' .. count) end)
            ",
        )
        .expect("load");
        assert_eq!(eng.frame_callback_count(), 1);
        for _ in 0..3 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
        }
        let log = eng.drain_log();
        assert_eq!(log, vec!["tick 1", "tick 2", "tick 3"]);
    }

    #[test]
    fn memory_read_and_write_round_trip() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
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
        assert_eq!(eng.drain_log(), vec!["mem10=66"]);
        assert_eq!(nes.peek(0x10), 0x42);
    }

    #[test]
    fn writes_are_gated_when_locked() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.set_writes_locked(true);
        eng.load("emu.onFrame(function() emu.write(0x20, 0x99) end)")
            .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert_eq!(nes.peek(0x20), 0x00, "write must be dropped when locked");
    }

    #[test]
    fn cpu_state_is_exposed() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load("emu.onFrame(function() emu.log('pc=' .. emu.cpu().pc) end)")
            .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert!(eng.drain_log()[0].starts_with("pc="));
    }

    #[test]
    fn sandbox_blocks_io_os_and_loaders() {
        let eng = ScriptEngine::new().expect("engine");
        for probe in [
            "return io.open('/etc/passwd')",
            "return os.execute('echo hi')",
            "return require('os')",
            "return load('return 1')",
            "return dofile('/etc/passwd')",
            "return package.path",
        ] {
            assert!(eng.load(probe).is_err(), "sandbox must reject: {probe}");
        }
    }

    #[test]
    fn runaway_loop_hits_the_budget() {
        let eng = ScriptEngine::new().expect("engine");
        eng.set_instruction_budget(100_000);
        let err = eng.load("while true do end").unwrap_err();
        assert!(matches!(err, ScriptError::Lua(_)));
    }

    /// NROM whose boot loop writes `$2000` each iteration:
    /// `LDA #$80; STA $2000; JMP $C000`.
    fn synth_writing_rom() -> Vec<u8> {
        let mut bytes = vec![b'N', b'E', b'S', 0x1A, 1, 1, 0, 0];
        bytes.resize(16, 0);
        let mut prg = vec![0u8; 16 * 1024];
        prg[0..8].copy_from_slice(&[0xA9, 0x80, 0x8D, 0x00, 0x20, 0x4C, 0x00, 0xC0]);
        let len = prg.len();
        prg[len - 4] = 0x00; // reset vector -> $C000
        prg[len - 3] = 0xC0;
        bytes.extend_from_slice(&prg);
        bytes.resize(16 + 16 * 1024 + 8 * 1024, 0);
        bytes
    }

    #[test]
    fn on_write_fires_from_the_access_log() {
        let mut nes = Nes::from_rom(&synth_writing_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            hits = 0
            emu.onWrite(0x2000, function(addr, val) hits = hits + 1 end)
            emu.onFrame(function() emu.log('hits=' .. hits) end)
            ",
        )
        .expect("load");
        assert!(eng.needs_access_log().unwrap());
        assert!(!eng.needs_exec_log().unwrap());
        // The host enables the access log per `needs_access_log`.
        nes.set_access_logging(true);
        let mut saw = false;
        for _ in 0..4 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
            if eng.drain_log().iter().any(|l| l != "hits=0") {
                saw = true;
                break;
            }
        }
        assert!(saw, "onWrite($2000) should fire from the bus-access log");
    }

    #[test]
    fn on_exec_fires_from_the_exec_log() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        // The boot loop sits at $C000 (JMP $C000); onExec there must fire.
        eng.load(
            r"
            seen = false
            emu.onExec(0xC000, function(pc) seen = true end)
            emu.onFrame(function() if seen then emu.log('exec') end end)
            ",
        )
        .expect("load");
        assert!(eng.needs_exec_log().unwrap());
        nes.set_exec_logging(true);
        let mut saw = false;
        for _ in 0..4 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
            if eng.drain_log().contains(&"exec".to_owned()) {
                saw = true;
                break;
            }
        }
        assert!(saw, "onExec($C000) should fire from the exec log");
    }

    #[test]
    fn unregistering_a_callback_mid_frame_does_not_crash() {
        // gemini #49: the active-address set is snapshotted before onFrame runs,
        // so a callback that clears its own registry slot during the frame leaves
        // the set stale. The replay must skip the now-`Nil` slot, not crash.
        let mut nes = Nes::from_rom(&synth_writing_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.onWrite(0x2000, function(addr, val) end)
            emu.onFrame(function()
                -- Drop the write callback registry entry mid-frame.
                __rustynes.write[0x2000] = nil
                emu.log('ok')
            end)
            ",
        )
        .expect("load");
        nes.set_access_logging(true);
        nes.run_frame();
        // Must not return an error (no FromLua crash on the stale Nil slot).
        eng.on_frame(&mut nes)
            .expect("replay tolerates mid-frame unregister");
        assert!(eng.drain_log().contains(&"ok".to_owned()));
    }

    #[test]
    fn clobbering_the_registry_does_not_error_the_host(/* M1 */) {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        // A hostile/buggy script nukes the internal registry. It must only
        // disable its own callbacks — never error out the host pump.
        eng.load(
            r"
            emu.onExec(0xC000, function() end)
            emu.onFrame(function() __rustynes = nil end)
            ",
        )
        .expect("load");
        nes.set_exec_logging(true);
        nes.run_frame();
        eng.on_frame(&mut nes)
            .expect("a clobbered registry must not error on_frame");
        // After the clobber, the engine reports no callbacks (graceful).
        assert!(!eng.needs_exec_log().unwrap());
        nes.run_frame();
        eng.on_frame(&mut nes)
            .expect("still fine on the next frame");
    }

    #[test]
    fn junk_value_at_a_callback_address_does_not_crash(/* gemini #52 */) {
        // A script parks a non-table value where a callback table is expected.
        // The replay must skip it (via `table_field`), not error on a FromLua
        // conversion.
        let mut nes = Nes::from_rom(&synth_writing_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.onWrite(0x2000, function() end)
            -- Overwrite the callback list with a number.
            __rustynes.write[0x2000] = 42
            emu.onFrame(function() emu.log('ok') end)
            ",
        )
        .expect("load");
        nes.set_access_logging(true);
        nes.run_frame();
        eng.on_frame(&mut nes)
            .expect("a non-table callback slot must not error on_frame");
        assert!(eng.drain_log().contains(&"ok".to_owned()));
    }

    #[test]
    fn control_and_draw_commands_are_queued() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.onFrame(function()
                emu.pause()
                emu.saveState(2)
                emu.setInput(0, 0x81)
                emu.drawText(10, 20, 'HP: 3', 0xFF0000FF)
                emu.drawRect(0, 0, 8, 8)
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");

        let controls = eng.drain_controls();
        assert!(controls.contains(&ControlCmd::Pause));
        assert!(controls.contains(&ControlCmd::SaveState(2)));
        assert!(controls.contains(&ControlCmd::SetInput {
            port: 0,
            buttons: 0x81
        }));
        let draws = eng.drain_draws();
        assert_eq!(draws.len(), 2);
        assert!(matches!(&draws[0], DrawCmd::Text { text, .. } if text == "HP: 3"));
        // Drained — a second drain is empty.
        assert!(eng.drain_controls().is_empty());
    }
}

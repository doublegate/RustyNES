//! The VM-binding contract every Lua backend implements.
//!
//! `rustynes-script` exposes ONE public `ScriptEngine` type.
//!
//! Its host-facing surface — load a chunk, pump once per emulated frame, drain
//! log / control / draw queues, register the `emu.*` callbacks, and honour the
//! write-gate — is defined here as the [`VmBackend`] trait. Two backends
//! implement it:
//!
//! - **mlua** (native, the `mlua-backend` feature) — vendored Lua 5.4 (C). The
//!   reference backend; byte-identical to the v1.1.0 engine. Implements the
//!   FULL contract (`onFrame` / `onExec` / `onRead` / `onWrite` / `onNmi` /
//!   `onIrq`, gated `write` + `setInput`, the per-frame instruction-budget
//!   hook).
//! - **piccolo** (experimental, the `script-wasm` feature) — a pure-Rust Lua
//!   VM that compiles to `wasm32-unknown-unknown` with no C toolchain. It is
//!   **explicitly not byte-parity** with mlua (a different VM) — which is fine,
//!   because scripts are observational / overlay + gated writes and are NEVER
//!   part of the framebuffer/audio determinism oracle (see ADR 0012). It
//!   implements the OBSERVATIONAL subset that piccolo can host cleanly:
//!   `emu.read` / `peek` / `readRange`, `emu.cpu` / `frame` / `cycle`,
//!   `emu.log` + `print`, `emu.onFrame`, the overlay draws, and the gated
//!   `emu.write` / `emu.setInput` + control commands. The per-access
//!   (`onExec`/`onRead`/`onWrite`) and per-interrupt (`onNmi`/`onIrq`) replay
//!   callbacks are a documented native-only limitation on the piccolo backend
//!   (they are registered as no-ops so a portable script does not error).
//!
//! The trait is a **compile-time** contract, not a `dyn` object: piccolo's
//! `gc-arena` `'gc` lifetime makes a trait object impractical, and exactly one
//! backend is ever compiled for a given target, so `ScriptEngine` selects the
//! concrete type with `#[cfg]` rather than boxing.

use rustynes_core::Nes;

use crate::types::{
    ClientCmd, ControlCmd, DrawCmd, ScriptError, TasCellDecor, TasCmd, TasSnapshot,
};
#[cfg(feature = "script-ipc")]
use crate::types::{CommCmd, CommResult};

/// The host-facing contract a Lua VM backend fulfils.
///
/// The public `ScriptEngine` is a thin newtype over the selected implementor;
/// the host only ever names `ScriptEngine`, `ControlCmd`, `DrawCmd`, and
/// `ScriptError`.
///
/// A backend MUST keep its callback registry Rust-side / not script-visible,
/// sandbox the standard library (no `io` / `os` / `load` / `require`), and gate
/// `write` + `setInput` on [`VmBackend::set_writes_locked`].
pub trait VmBackend: Sized {
    /// Build a fresh sandboxed engine (no script loaded yet).
    ///
    /// # Errors
    /// Returns [`ScriptError`] if the sandbox prelude fails to install.
    fn new() -> Result<Self, ScriptError>;

    /// Load (and execute the top level of) a Lua script. Top-level code
    /// typically registers callbacks via `emu.onFrame(...)`.
    ///
    /// # Errors
    /// Returns [`ScriptError`] on a syntax or top-level runtime error, or if the
    /// load exceeded the instruction budget.
    fn load(&mut self, src: &str) -> Result<(), ScriptError>;

    /// Run one emulated frame's worth of scripting against `nes`.
    ///
    /// # Errors
    /// Returns [`ScriptError`] if a callback raises or busts the budget.
    fn on_frame(&mut self, nes: &mut Nes) -> Result<(), ScriptError>;

    /// Set the per-frame VM-instruction / fuel budget (runaway-loop guard).
    fn set_instruction_budget(&self, budget: u64);

    /// Gate `emu.write` AND `emu.setInput`: when `true` both are silently
    /// dropped so a script cannot perturb a locked / replayed session.
    fn set_writes_locked(&self, locked: bool);

    /// v1.5.0 Workstream B (B4) — replace the script-visible symbol table the
    /// `sym:addr(name)` / `sym:name(addr)` queries resolve against. The host
    /// pushes the debugger's loaded symbols (`address -> label`) here; each pair
    /// is `(address, label)`. Read-only on the script side; never touches
    /// deterministic emulator state. The default no-op suits a backend that does
    /// not host the dev/TAS symbol API (the experimental piccolo backend).
    fn set_symbols(&self, _pairs: &[(u16, String)]) {}

    /// Drain captured log / `print` output (oldest first).
    fn drain_log(&self) -> Vec<String>;

    /// Drain the control actions requested since the last call.
    fn drain_controls(&self) -> Vec<ControlCmd>;

    /// Drain the overlay draw commands issued this frame.
    fn drain_draws(&self) -> Vec<DrawCmd>;

    /// `true` if any `onExec` callback is registered (host enables the exec log).
    fn needs_exec_log(&self) -> bool;

    /// `true` if any `onRead`/`onWrite` callback is registered (host enables the
    /// access log).
    fn needs_access_log(&self) -> bool;

    /// `true` if any `onNmi`/`onIrq` callback is registered (host enables the
    /// interrupt log).
    fn needs_interrupt_log(&self) -> bool;

    /// Number of registered `onFrame` callbacks (for the host UI / tests).
    fn frame_callback_count(&self) -> usize;

    // ---- v1.7.0 "Forge" Workstream B — scriptable TAStudio + Lua parity ----
    //
    // These extend the contract with the `tastudio.*` namespace (B1/B2) and the
    // Mesen2-parity surface (B3). They all carry a default that suits a backend
    // that does NOT host the dev/TAS surface (the experimental piccolo wasm
    // backend), exactly like `set_symbols`: the native mlua backend overrides
    // them, piccolo inherits the no-ops (the same native-only carve-out as the
    // per-access / per-interrupt callbacks; ADR 0012).

    /// B1 — push a read-only snapshot of the host's live `TAStudio` editor so
    /// the `tastudio.*` query API resolves against current editor state. The
    /// host pushes this each frame before [`VmBackend::on_frame`]. No-op default.
    fn set_tas_snapshot(&self, _snapshot: TasSnapshot) {}

    /// B1 — drain the `TAStudio` editor actions a script requested this frame
    /// (`tastudio.*` mutators). The host applies + gates them. Empty default.
    fn drain_tas_commands(&self) -> Vec<TasCmd> {
        Vec::new()
    }

    /// B2 — query the per-cell decoration a script's `onqueryitem*` callbacks
    /// produce for piano-roll cell `(frame, column)`. Returns the default
    /// (no decoration) unless a backend hosts the callbacks. `column` is the
    /// host's grid column index (0 = frame#, 1 = P1 buttons, ...).
    ///
    /// # Errors
    /// Returns [`ScriptError`] if a callback raises.
    fn query_tas_cell(&self, _frame: usize, _column: u32) -> Result<TasCellDecor, ScriptError> {
        Ok(TasCellDecor::default())
    }

    /// B2 — clear the icon cache (`tastudio.clearIconCache()` requested it).
    /// Returns whether a script asked to clear it since the last drain.
    fn take_clear_icon_cache(&self) -> bool {
        false
    }

    /// B2 — invoke the registered `ongreenzoneinvalidated(fn)` callbacks,
    /// passing the first invalidated frame. The host calls this after an edit.
    ///
    /// # Errors
    /// Returns [`ScriptError`] if a callback raises.
    fn fire_greenzone_invalidated(&self, _first_frame: usize) -> Result<(), ScriptError> {
        Ok(())
    }

    /// B2 — invoke the registered `onbranchload(fn)` callbacks, passing the
    /// loaded branch index. The host calls this after a branch loads.
    ///
    /// # Errors
    /// Returns [`ScriptError`] if a callback raises.
    fn fire_branch_load(&self, _index: usize) -> Result<(), ScriptError> {
        Ok(())
    }

    /// B3 — `true` if any `tastudio.onqueryitem*` callback is registered, so the
    /// host knows to call [`VmBackend::query_tas_cell`] while painting the grid.
    fn needs_tas_cell_query(&self) -> bool {
        false
    }

    // ---- v2.1.10 "Creator Tools" (B9) — host-fired lifecycle events ----
    //
    // `reset` / `spriteZeroHit` / `codeBreak` are registered via
    // `emu.addEventCallback(fn, "<name>")` but — unlike `startFrame` / `endFrame`
    // which the engine fires from its own per-frame pump — these are driven by a
    // host signal (a soft-reset, the per-frame sprite-0 hit verdict, a debugger
    // break). The host calls the matching `fire_*` after the frame pump. Each is
    // observational (no live `Nes`), mirroring the greenzone / branch events. The
    // defaults suit a backend that does not host them (the piccolo wasm backend,
    // where they are registered as no-ops per ADR 0012).

    /// Invoke the registered `reset` event callbacks (soft-reset / power-cycle).
    ///
    /// # Errors
    /// Returns [`ScriptError`] if a callback raises.
    fn fire_reset(&self) -> Result<(), ScriptError> {
        Ok(())
    }

    /// Invoke the registered `spriteZeroHit` event callbacks, passing the frame
    /// the hit occurred on.
    ///
    /// # Errors
    /// Returns [`ScriptError`] if a callback raises.
    fn fire_sprite_zero_hit(&self, _frame: usize) -> Result<(), ScriptError> {
        Ok(())
    }

    /// Invoke the registered `codeBreak` event callbacks, passing the PC the
    /// break occurred at.
    ///
    /// # Errors
    /// Returns [`ScriptError`] if a callback raises.
    fn fire_code_break(&self, _pc: u16) -> Result<(), ScriptError> {
        Ok(())
    }

    /// `true` if any `reset` event callback is registered (host fires `fire_reset`
    /// only when so).
    fn needs_reset_event(&self) -> bool {
        false
    }

    /// `true` if any `spriteZeroHit` event callback is registered (host does the
    /// per-frame PPUSTATUS check + `fire_sprite_zero_hit` only when so).
    fn needs_sprite_zero_hit_event(&self) -> bool {
        false
    }

    /// `true` if any `codeBreak` event callback is registered (host fires
    /// `fire_code_break` on a debugger break only when so).
    fn needs_code_break_event(&self) -> bool {
        false
    }

    /// B3 — set the per-script sandboxed data directory returned by
    /// `emu.getScriptDataFolder()` (`None` clears it). No-op default.
    fn set_script_data_folder(&self, _path: Option<String>) {}

    /// v1.7.0 "Forge" E2 — drain the `client.*` automation verbs requested this
    /// frame. The default empties nothing (a backend that does not host the
    /// `client` table, e.g. the experimental piccolo backend).
    fn drain_clients(&self) -> Vec<ClientCmd> {
        Vec::new()
    }

    /// v1.7.0 "Forge" E1 — drain the host-mediated `comm.*` IPC requests issued
    /// this frame. Default empty (the `comm` table is mlua-only + `script-ipc`).
    #[cfg(feature = "script-ipc")]
    fn drain_comm(&self) -> Vec<CommCmd> {
        Vec::new()
    }

    /// v1.7.0 "Forge" E1 — deliver a host-fulfilled [`CommResult`] back to the
    /// engine (surfaced to the script via `comm.receive()`). Default no-op.
    #[cfg(feature = "script-ipc")]
    fn push_comm_result(&self, _result: CommResult) {}

    /// v1.7.0 "Forge" E3 — snapshot the `userdata.*` KV store (sorted by key).
    /// Default empty (a backend without the `userdata` table).
    fn userdata_snapshot(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// v1.7.0 "Forge" E3 — restore the `userdata.*` KV store from a snapshot.
    /// Default no-op.
    fn userdata_restore(&self, _pairs: &[(String, String)]) {}
}

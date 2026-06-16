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

use crate::types::{ControlCmd, DrawCmd, ScriptError};

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
}

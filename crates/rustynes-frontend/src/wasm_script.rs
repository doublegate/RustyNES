//! v1.2.0 Workstream F4 — JS bridge for the EXPERIMENTAL wasm Lua engine.
//!
//! The browser hands a `.lua` source string to the running emulator the same
//! way the touch overlay hands input (`wasm_touch`): a thread-local buffer set
//! from JS via a `#[wasm_bindgen]` export, drained by the `App` each frame.
//! winit owns the `App` behind its event loop, so a direct JS->App call is not
//! possible — the thread-local is the lightweight, gesture-safe bridge.
//!
//! This is gated behind the off-by-default `script-wasm` feature and drives the
//! pure-Rust piccolo backend, which is **explicitly not byte-parity** with the
//! native mlua engine (scripts are observational/overlay + gated writes, never
//! part of the determinism oracle). See `docs/adr/0012-wasm-lua-piccolo-backend.md`.

use std::cell::RefCell;

use wasm_bindgen::prelude::*;

thread_local! {
    /// Pending Lua source set from JS, drained (taken) by the `App` on the next
    /// produced frame. `None` = nothing to load (the steady state).
    static PENDING_SCRIPT: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// JS bridge: load (or replace) the running Lua script with `src`.
///
/// Call from the browser console / page UI:
/// `window.wasm_bindgen.rustynes_load_script("emu.onFrame(function() ... end)")`.
/// The `App` picks it up on the next produced frame and (re)builds a fresh
/// piccolo engine.
#[wasm_bindgen]
pub fn rustynes_load_script(src: &str) {
    PENDING_SCRIPT.with(|s| *s.borrow_mut() = Some(src.to_owned()));
}

/// JS bridge: stop + unload the running script.
#[wasm_bindgen]
pub fn rustynes_stop_script() {
    // An empty source is the sentinel for "stop": the `App` treats `Some("")`
    // as an unload request rather than a (no-op) empty chunk.
    PENDING_SCRIPT.with(|s| *s.borrow_mut() = Some(String::new()));
}

/// Drain the pending script source set from JS (returns `None` when nothing is
/// pending). Called by the `App` once per produced frame.
#[must_use]
pub fn take_pending() -> Option<String> {
    PENDING_SCRIPT.with(|s| s.borrow_mut().take())
}

//! v1.7.0 "Forge" beta.5 Workstream H6 — shared wasm32 browser Gamepad input.
//!
//! This module is gated `#[cfg(target_arch = "wasm32")]` and is the bridge
//! between the browser's **Gamepad API** (`navigator.getGamepads()`) and BOTH
//! wasm frontends — the lightweight canvas-2D embed (`wasm.rs`) and the
//! unified winit/wgpu path (`app.rs` via `frame_inputs`).
//!
//! ## Why a thread-local + JS poll, not a `web-sys` poll here
//!
//! It mirrors [`crate::wasm_touch`]: the Gamepad API has no event for the
//! button state, so a frame-cadence poll is required. The JS in
//! `web/index.html` polls `navigator.getGamepads()` on every
//! `requestAnimationFrame`, folds the **standard-gamepad-mapping** buttons +
//! axes into a [`rustynes_core::Buttons`] mask, and calls the exported
//! [`rustynes_gamepad_set_buttons`] bridge. Keeping the poll in JS costs ZERO
//! Rust binary weight beyond the bridge fn (no `web-sys` `Gamepad` glue), and
//! the wasm side just reads the latest mask at its existing late-latch point.
//!
//! ## Mapping (Xbox-style, P1)
//!
//! The browser "standard" gamepad mapping is the Xbox layout, matching the
//! native USB-gamepad binding (`docs/frontend.md`): South (A) = NES A, West
//! (X) = NES B, Start = Start, Back/Select = Select, D-pad + left stick =
//! the four directions. The JS side does the button→mask translation; this
//! module only stores + exposes the resulting mask.
//!
//! ## Determinism
//!
//! The gamepad mask is read at the SAME deterministic late-latch a real
//! keypress / the touch overlay enters (it is OR'd into the routed port in
//! `App::frame_inputs` and into the per-frame `set_buttons` in the canvas
//! rAF loop), so it is recorded/replayed identically by TAS movies + netplay.
//! Gamepad input adds NO new determinism surface — it is just another source
//! of the same per-frame button snapshot.

use core::cell::Cell;

use rustynes_core::Buttons;
use wasm_bindgen::prelude::*;

thread_local! {
    /// Latest standard-controller button mask polled from the browser Gamepad
    /// API, in the [`Buttons`] bit layout (`A=1, B=2, SELECT=4, START=8,
    /// UP=16, DOWN=32, LEFT=64, RIGHT=128`). `0` = no pad / nothing held
    /// (byte-identical default).
    static GAMEPAD_BUTTONS: Cell<u8> = const { Cell::new(0) };
}

/// JS bridge: set the current Gamepad button mask for player 1.
///
/// Called from the `index.html` rAF poll. `mask` is the [`Buttons`] bit
/// layout; bits outside the 8 controller buttons are ignored by
/// [`Buttons::from_bits_truncate`].
#[wasm_bindgen]
pub fn rustynes_gamepad_set_buttons(mask: u8) {
    GAMEPAD_BUTTONS.with(|b| b.set(mask));
}

/// The current gamepad button mask as [`Buttons`] (empty if no pad / nothing
/// held).
#[must_use]
pub fn gamepad_buttons() -> Buttons {
    Buttons::from_bits_truncate(GAMEPAD_BUTTONS.with(Cell::get))
}

//! v1.2.0 Workstream F1/F2 — shared wasm32 on-screen touch input.
//!
//! This module is gated `#[cfg(target_arch = "wasm32")]` and is the single
//! bridge between the browser's Pointer-Events touch overlay (translucent
//! DOM/CSS in `web/index.html`) and BOTH wasm frontends — the lightweight
//! canvas-2D embed (`wasm.rs`) and the unified winit/wgpu path (`app.rs` via
//! `frame_inputs`).
//!
//! ## Why a thread-local, not a `web-sys` listener here
//!
//! The overlay is plain DOM: the JS in `index.html` translates `pointerdown`
//! / `pointerup` / `pointercancel` on each control into a button mask and
//! calls the exported [`rustynes_touch_set_buttons`] /
//! [`rustynes_touch_set_power_pad`] / [`rustynes_touch_set_target_port`] /
//! [`rustynes_touch_set_power_pad_active`] functions. Keeping the event
//! plumbing in JS costs ZERO Rust binary weight beyond the bridge fns (no
//! extra `web-sys` event-listener glue), and the wasm side just reads the
//! latest mask each frame.
//!
//! ## Determinism
//!
//! The touch mask is read at the SAME deterministic late-latch point a real
//! keypress enters:
//!
//! - `wasm-winit` ORs it into [`crate::emu::FrameInputs`] in
//!   `App::frame_inputs`, so it flows through `EmuCore::latch` exactly like a
//!   keyboard bit — and is therefore recorded/replayed identically by TAS
//!   movies and netplay.
//! - `wasm-canvas` ORs it into the per-frame `set_buttons` / `set_power_pad`
//!   call in its rAF loop (the canvas embed has no movie/netplay surface).
//!
//! Touch input adds NO new determinism surface: it is just another source of
//! the same per-frame button snapshot.

use core::cell::Cell;

use rustynes_core::Buttons;
use wasm_bindgen::prelude::*;

thread_local! {
    /// Latest standard-controller button mask from the touch overlay, in the
    /// [`Buttons`] bit layout (`A=1, B=2, SELECT=4, START=8, UP=16, DOWN=32,
    /// LEFT=64, RIGHT=128`). `0` = nothing held (byte-identical default).
    static TOUCH_BUTTONS: Cell<u8> = const { Cell::new(0) };
    /// Latest Power Pad mat button mask (bit `i` = mat button `i+1`, 0..=11).
    static TOUCH_POWER_PAD: Cell<u16> = const { Cell::new(0) };
    /// Which port the touch buttons drive (0 = player 1 / `$4016`, 1 = player
    /// 2 / `$4017`, 2 = player 3, 3 = player 4 on a Four Score). Defaults to
    /// player 1.
    static TOUCH_TARGET_PORT: Cell<u8> = const { Cell::new(0) };
    /// Whether the Power Pad is the active touch device. When set, the
    /// `wasm-winit` latch + the `wasm-canvas` loop feed `set_power_pad(1, ..)`
    /// each frame (which self-attaches the mat on port 1). `false` = the no
    /// Power Pad path, byte-identical to a build without this feature.
    static TOUCH_POWER_PAD_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// JS bridge: set the current standard-controller touch button mask.
///
/// Called from the `index.html` overlay on every `pointerdown` / `pointerup`
/// / `pointercancel`. `mask` is the [`Buttons`] bit layout; bits outside the
/// 8 controller buttons are ignored by [`Buttons::from_bits_truncate`].
#[wasm_bindgen]
pub fn rustynes_touch_set_buttons(mask: u8) {
    TOUCH_BUTTONS.with(|b| b.set(mask));
}

/// JS bridge: set the current Power Pad mat button mask (bit `i` = mat button
/// `i+1`, 0..=11).
#[wasm_bindgen]
pub fn rustynes_touch_set_power_pad(mask: u16) {
    TOUCH_POWER_PAD.with(|p| p.set(mask));
}

/// JS bridge: route the touch buttons to a target port (0..=3 → player 1..4;
/// out-of-range values clamp to player 1).
#[wasm_bindgen]
pub fn rustynes_touch_set_target_port(port: u8) {
    TOUCH_TARGET_PORT.with(|p| p.set(if port <= 3 { port } else { 0 }));
}

/// JS bridge: enable/disable the Power Pad as the active touch device.
#[wasm_bindgen]
pub fn rustynes_touch_set_power_pad_active(active: bool) {
    TOUCH_POWER_PAD_ACTIVE.with(|a| a.set(active));
}

/// The current touch button mask as [`Buttons`] (empty if nothing held).
#[must_use]
pub fn touch_buttons() -> Buttons {
    Buttons::from_bits_truncate(TOUCH_BUTTONS.with(Cell::get))
}

/// The current target port for the touch buttons (0..=3).
#[must_use]
pub fn touch_target_port() -> usize {
    TOUCH_TARGET_PORT.with(Cell::get) as usize
}

/// The current Power Pad mat mask.
#[must_use]
pub fn touch_power_pad() -> u16 {
    TOUCH_POWER_PAD.with(Cell::get)
}

/// Whether the Power Pad is the active touch device.
#[must_use]
pub fn touch_power_pad_active() -> bool {
    TOUCH_POWER_PAD_ACTIVE.with(Cell::get)
}

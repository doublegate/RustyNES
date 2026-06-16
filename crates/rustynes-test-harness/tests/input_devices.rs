//! Non-standard input device integration tests (v2.1.0): the Arkanoid "Vaus"
//! paddle and the NES Zapper light gun.
//!
//! ## Vaus paddle
//!
//! Driven against the in-tree `vaus-test` ROM (Damian Yerrick, all-permissive
//! license; documents the NES wiring `$4017 D3: Button`, `$4017 D4: Position
//! (8 bits, MSB first)`). The ROM is interactive — it renders an on-screen
//! paddle whose position tracks the controller / pot — so there is no `$6000`
//! status protocol. We therefore attach a Vaus on port 1, drive scripted
//! potentiometer positions, run the ROM, and snapshot the framebuffer FNV-1a.
//!
//! The regression gate is two-fold:
//!  1. The framebuffer hash is deterministic (a stable snapshot).
//!  2. Two DIFFERENT paddle positions produce DIFFERENT framebuffers — proving
//!     the ROM actually reads the Vaus serial data through our `$4017` overlay
//!     path. A "left" position and a "right" position must not hash equal.
//!
//! ## Zapper
//!
//! No redistributable pass/fail Zapper ROM exists, so the Zapper is verified
//! via the unit tests in `rustynes-core::input_device` (synthetic framebuffers ->
//! light-detect / trigger bit logic). The test here is a light wiring smoke:
//! attaching a Zapper and reading via the public API does not panic and the
//! default (no light, no trigger) byte is sane.

#![cfg(feature = "test-roms")]

mod common;

use common::{fnv1a64, rom_path};
use std::fs;

use rustynes_core::Nes;
use rustynes_core::input_device::{InputDevice, VausState, ZapperState};

const VAUS_ROM: &str = "nes-test-roms/vaus-test/vaus-test.nes";

/// Run the Vaus ROM with a paddle attached on port 1, holding `position` and
/// `fire` for the whole run, and return the framebuffer FNV-1a hash.
fn run_vaus(position: u8, fire: bool, frames: u64) -> u64 {
    let path = rom_path(VAUS_ROM);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse vaus: {e}"));
    // Attach a Vaus on the player-2 port ($4017).
    nes.set_paddle(1, position, fire);
    for _ in 0..frames {
        // Re-apply each frame (the frontend would do this from live input).
        nes.set_paddle(1, position, fire);
        nes.run_frame();
    }
    fnv1a64(nes.framebuffer())
}

#[test]
fn vaus_rom_responds_to_paddle_position() {
    // The vaus-test demo only tracks the knob WHILE the controller's
    // select/fire button is held (per its README: "press the A button on one
    // of the controllers, and then ... twist the pot"). The Vaus's single fire
    // button is that enable; with it held, the on-screen paddle follows the
    // knob value. Run two extreme positions with fire held.
    const FRAMES: u64 = 120;
    let left = run_vaus(0x10, true, FRAMES);
    let right = run_vaus(0xF0, true, FRAMES);
    // The on-screen paddle/value tracks the pot, so the two extreme positions
    // MUST render differently — this is the proof the ROM reads the Vaus
    // serial knob data through the $4017 overlay path. (With fire released the
    // demo ignores the knob, so both would render identically — a false pass.)
    assert_ne!(
        left, right,
        "Vaus paddle position must change the rendered framebuffer \
         (left=0x{left:016x} right=0x{right:016x}); if equal, the ROM is not \
         seeing the paddle serial data on $4017"
    );
}

#[test]
fn vaus_rom_framebuffer_is_deterministic() {
    // Same scripted input twice -> identical framebuffer (determinism contract
    // holds with a device attached: the device state is part of the input).
    const FRAMES: u64 = 120;
    let a = run_vaus(0x80, true, FRAMES);
    let b = run_vaus(0x80, true, FRAMES);
    assert_eq!(a, b, "Vaus run must be deterministic for identical input");
}

#[test]
fn vaus_rom_position_snapshots() {
    // Stable framebuffer-hash snapshots for two paddle positions (fire held so
    // the demo tracks the knob). Regression gate against accidental changes to
    // the Vaus serial read path or the renderer.
    const FRAMES: u64 = 120;
    let left = run_vaus(0x10, true, FRAMES);
    let right = run_vaus(0xF0, true, FRAMES);
    insta::assert_snapshot!(
        "vaus_pos0x10_fire_f120",
        format!("rom={VAUS_ROM} pos=0x10 fire=true frames={FRAMES} fnv1a64={left:016x}")
    );
    insta::assert_snapshot!(
        "vaus_pos0xF0_fire_f120",
        format!("rom={VAUS_ROM} pos=0xF0 fire=true frames={FRAMES} fnv1a64={right:016x}")
    );
}

// ===========================================================================
// PaddleTest3 (3GenGames / Aaron Bottegal, all-permissive) — v2.2.x coverage.
//
// PROTOCOL FINDING (verified against the ROM's `PaddleTest.asm`): this ROM
// reads the Vaus on PORT 0 ($4016), not the wiki's $4017 port. It strobes
// $4016 then reads bit 4 ($AND #$10) eight times MSb-first, and renders the
// decoded knob value on screen ("Value: NN") plus a paddle sprite that tracks
// the value and a button-state block. We attach a Vaus on port 0, drive two
// scripted positions, and assert that the two positions render DIFFERENTLY
// (proving the ROM reads our serial knob data through the $4016 overlay path)
// plus a stable framebuffer-hash snapshot for each. Probing confirmed
// position 0x20 renders "Value: 20" and 0xE0 renders "Value: E0".
// ===========================================================================

const PADDLE_ROM: &str = "nes-test-roms/PaddleTest3/PaddleTest.nes";

/// Run the `PaddleTest` ROM with a Vaus on PORT 0 ($4016) holding `position`
/// and fire for the whole run, returning the framebuffer FNV-1a hash.
fn run_paddle(position: u8, frames: u64) -> u64 {
    let path = rom_path(PADDLE_ROM);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse paddle: {e}"));
    for _ in 0..frames {
        // Port 0 ($4016) — PaddleTest reads the knob there. Fire held so the
        // button block lights up and the value is sampled.
        nes.set_paddle(0, position, true);
        nes.run_frame();
    }
    fnv1a64(nes.framebuffer())
}

#[test]
fn paddle_test_rom_responds_to_position() {
    const FRAMES: u64 = 180;
    let lo = run_paddle(0x20, FRAMES);
    let hi = run_paddle(0xE0, FRAMES);
    // The on-screen value + paddle sprite track the knob, so two distinct
    // positions MUST render differently — the proof the ROM reads the Vaus
    // serial data through the $4016 overlay path (a "Value: ND" / not-detected
    // screen would hash identically regardless of position = a false pass).
    assert_ne!(
        lo, hi,
        "PaddleTest must render differently for paddle 0x20 vs 0xE0 \
         (lo=0x{lo:016x} hi=0x{hi:016x}); if equal, the ROM is not seeing the \
         paddle serial data on $4016"
    );
}

#[test]
fn paddle_test_rom_deterministic() {
    const FRAMES: u64 = 180;
    let a = run_paddle(0x80, FRAMES);
    let b = run_paddle(0x80, FRAMES);
    assert_eq!(
        a, b,
        "PaddleTest run must be deterministic for identical input"
    );
}

#[test]
fn paddle_test_rom_position_snapshots() {
    const FRAMES: u64 = 180;
    let lo = run_paddle(0x20, FRAMES);
    let hi = run_paddle(0xE0, FRAMES);
    insta::assert_snapshot!(
        "paddle_test_pos0x20_f180",
        format!("rom={PADDLE_ROM} port=0 pos=0x20 fire=true frames={FRAMES} fnv1a64={lo:016x}")
    );
    insta::assert_snapshot!(
        "paddle_test_pos0xE0_f180",
        format!("rom={PADDLE_ROM} port=0 pos=0xE0 fire=true frames={FRAMES} fnv1a64={hi:016x}")
    );
}

#[test]
fn zapper_attach_smoke() {
    // Attaching a Zapper and updating it through the public API must not panic,
    // and the default reads (no light, no trigger) must be sane. The actual
    // light-detect logic is unit-tested in rustynes-core::input_device.
    let path = rom_path(VAUS_ROM); // any small ROM works for the wiring smoke
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse: {e}"));
    nes.set_zapper(1, 128, 120, false);
    assert!(matches!(
        nes.bus_expansion_device_kind(1),
        Some(DeviceKind::Zapper)
    ));
    // Run a few frames; light sampling happens at end-of-frame and must not panic.
    for _ in 0..3 {
        nes.set_zapper(1, 128, 120, false);
        nes.run_frame();
    }
    // Detach -> back to standard controller path.
    nes.set_expansion_device(1, None);
    assert!(nes.bus_expansion_device_kind(1).is_none());
}

/// Local enum so the test can assert the attached device kind without leaking
/// internal state. Mirrors `InputDevice`'s discriminant.
#[derive(Debug, PartialEq, Eq)]
enum DeviceKind {
    Zapper,
    Vaus,
    PowerPad,
    SnesMouse,
    FamilyKeyboard,
}

trait DeviceKindExt {
    fn bus_expansion_device_kind(&self, port: usize) -> Option<DeviceKind>;
}

impl DeviceKindExt for Nes {
    fn bus_expansion_device_kind(&self, port: usize) -> Option<DeviceKind> {
        match self.expansion_device(port) {
            Some(InputDevice::Zapper(_)) => Some(DeviceKind::Zapper),
            Some(InputDevice::Vaus(_)) => Some(DeviceKind::Vaus),
            Some(InputDevice::PowerPad(_)) => Some(DeviceKind::PowerPad),
            Some(InputDevice::SnesMouse(_)) => Some(DeviceKind::SnesMouse),
            Some(InputDevice::FamilyKeyboard(_)) => Some(DeviceKind::FamilyKeyboard),
            None => None,
        }
    }
}

#[test]
fn input_device_state_types_constructible() {
    // Light compile/use check that the public state types + enum are reachable.
    let _ = InputDevice::Vaus(VausState::new());
    let _ = InputDevice::Zapper(ZapperState::new());
    let _ = InputDevice::PowerPad(rustynes_core::PowerPadState::new());
    let _ = InputDevice::SnesMouse(rustynes_core::SnesMouseState::new());
    let _ = InputDevice::FamilyKeyboard(rustynes_core::FamilyKeyboardState::new());
}

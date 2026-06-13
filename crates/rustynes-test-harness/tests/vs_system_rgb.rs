//! Nintendo Vs. System (mapper 99) in-game RGB-PPU verification (v2.6.0).
//!
//! v2.5.0 shipped the 2C03/2C04/2C05 RGB-PPU device but had no Vs. ROMs to
//! game-verify it. v2.6.0 adds mapper 99 — the robust, mapper-driven Vs.
//! System signal (no licensed home game uses mapper 99, so it is immune to
//! the byte-7 `0x0A` corruption trap that mislabels many No-Intro home-NES
//! dumps as PlayChoice-10). A mapper-99 cart is forced to
//! `ConsoleType::VsSystem` + the 2C03 RGB PPU at parse time.
//!
//! These tests boot the user-supplied Vs. dumps at
//! `tests/roms/external/vs-system/` (gitignored; never committed). They are
//! gated on `commercial-roms` so CI never depends on non-distributable
//! assets. The assertion is hardware-grounded: every colour the Vs. cart
//! renders must come from the 2C03 master palette, and at least one rendered
//! colour must be one the composite 2C02 palette could NOT produce — proving
//! the frame is genuinely RGB-routed, not the legacy composite path.

#![cfg(feature = "commercial-roms")]

mod common;

use std::collections::HashSet;
use std::path::PathBuf;

use common::external::external_rom_path;
use rustynes_core::rustynes_ppu::{nes_color_to_rgba, palette_color_to_rgba, PpuPalette};
use rustynes_core::{Buttons, Nes};

/// The full RGBA set the composite 2C02 can emit across all 64 colour
/// indices and all 8 emphasis combinations.
fn composite_2c02_rgba_set() -> HashSet<[u8; 4]> {
    let mut set = HashSet::new();
    for idx in 0u8..64 {
        for emph in 0u8..8 {
            set.insert(palette_color_to_rgba(
                PpuPalette::Composite2C02,
                idx,
                emph & 0x01 != 0,
                emph & 0x02 != 0,
                emph & 0x04 != 0,
            ));
        }
    }
    // The base table (`nes_color_to_rgba`) without emphasis is a subset of the
    // above; included defensively in case a path skips emphasis.
    for idx in 0u8..64 {
        set.insert(nes_color_to_rgba(idx));
    }
    set
}

/// The full RGBA set the 2C03 RGB PPU can emit across all 64 colour indices
/// and all 8 emphasis combinations.
fn rgb_2c03_rgba_set() -> HashSet<[u8; 4]> {
    let mut set = HashSet::new();
    for idx in 0u8..64 {
        for emph in 0u8..8 {
            set.insert(palette_color_to_rgba(
                PpuPalette::Rgb2C03,
                idx,
                emph & 0x01 != 0,
                emph & 0x02 != 0,
                emph & 0x04 != 0,
            ));
        }
    }
    set
}

/// Boot a Vs. ROM with a coin inserted (Vs. games sit on an attract /
/// insert-coin screen until a coin is latched), returning the final
/// framebuffer.
fn boot_with_coin(rel: &str, frames: u64) -> Vec<u8> {
    let path: PathBuf = external_rom_path(rel);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut nes = Nes::from_rom(&bytes).expect("Vs. ROM must parse");
    assert!(
        nes.is_vs_system(),
        "mapper-99 cart must resolve to ConsoleType::VsSystem"
    );
    // Default DIP bank 0; insert a coin on acceptor #1 a few times during the
    // run so the game leaves its attract loop.
    for f in 0..frames {
        // Pulse a coin every ~120 frames (the acceptor reads true for a short
        // hardware window; we clear it after a couple of frames).
        if f % 120 == 30 {
            nes.insert_coin(0);
        }
        if f % 120 == 34 {
            nes.clear_coin();
        }
        // Tap Start occasionally too (some Vs. titles also want Start).
        let btn = if f % 120 == 60 {
            Buttons::START
        } else {
            Buttons::empty()
        };
        nes.set_buttons(0, btn);
        nes.run_frame();
    }
    nes.framebuffer().to_vec()
}

/// Assert a Vs. framebuffer is genuinely RGB-routed through the 2C03 palette.
fn assert_rgb_routed(label: &str, fb: &[u8]) {
    assert_eq!(fb.len() % 4, 0, "{label}: framebuffer must be RGBA");
    let colours: HashSet<[u8; 4]> = fb
        .chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect();

    // Non-blank: a crashed / black boot shows <= 4 colours.
    assert!(
        colours.len() > 4,
        "{label}: framebuffer is blank/crashed ({} colours)",
        colours.len()
    );

    let p2c03 = rgb_2c03_rgba_set();
    let composite = composite_2c02_rgba_set();

    // Every rendered colour must be a member of the 2C03 palette.
    for c in &colours {
        assert!(
            p2c03.contains(c),
            "{label}: colour {c:?} is not in the 2C03 palette — not RGB-routed"
        );
    }

    // At least one rendered colour must be impossible under the composite
    // 2C02 palette — proving this is the RGB path, not the legacy composite
    // path masquerading.
    let rgb_only = colours.iter().any(|c| !composite.contains(c));
    assert!(
        rgb_only,
        "{label}: every colour is also a composite-2C02 colour — RGB routing not proven"
    );
}

#[test]
fn vs_excitebike_renders_via_2c03_palette() {
    let fb = boot_with_coin("vs-system/VS Excitebike.nes", 300);
    assert_rgb_routed("VS Excitebike", &fb);
}

#[test]
fn vs_clu_clu_land_renders_via_2c03_palette() {
    let fb = boot_with_coin("vs-system/VS Clu Clu Land.nes", 300);
    assert_rgb_routed("VS Clu Clu Land", &fb);
}

/// The 2C03 palette must be a genuinely different colour set from the
/// composite 2C02 — otherwise the "RGB-only colour" check above is vacuous.
#[test]
fn palettes_2c03_and_composite_differ() {
    let p2c03 = rgb_2c03_rgba_set();
    let composite = composite_2c02_rgba_set();
    let only_in_2c03 = p2c03.difference(&composite).count();
    assert!(
        only_in_2c03 > 0,
        "2C03 and composite-2C02 palettes are identical sets"
    );
}

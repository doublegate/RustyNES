//! v2.9.8 — the opt-in Famicom console model ([`rustynes_core::ConsoleModel`])
//! on the commercial dump that motivated it.
//!
//! The *999-in-1* multicart (mapper 212) clears its nametable through
//! `$2006`/`$2007` at about CPU cycle 27,400 after power-on. On a front-loading
//! NES that is inside the PPU's documented warm-up (about 29,658 cycles, `NESdev`
//! "PPU power up state"), so both `$2006` writes are ignored, the fill goes
//! somewhere else, and the menu is drawn over a nametable still holding tile
//! `$00`, the font's "0". On a Famicom the PPU's `/RESET` is tied to 5 V and it
//! leaves its warm-up about a frame before the CPU starts (same page,
//! §Famicom), so the same writes land. The cart is a Famicom-market pirate, so
//! the Famicom behaviour is the one it was written against.
//!
//! This test pins both halves: the default (NES) model keeps today's output,
//! and the Famicom model clears the nametable to the cart's own blank tile.
//! It reads emulator state only, never ROM bytes, and commits nothing.
//!
//! ```text
//! cargo test -p rustynes-test-harness --release \
//!     --features commercial-roms,test-roms --test famicom_console -- --nocapture
//! ```
//!
//! Set `RUSTYNES_FAMICOM_DUMP=<dir>` to write the two frames as PNGs.

#![cfg(feature = "commercial-roms")]

mod common;

use common::external::external_rom_path;
use rustynes_core::{ConsoleModel, Nes};

/// The dump the model was added for (gitignored, local only).
const ROM: &str = "mapper-212-Multicart212/999-in-1 [p1][!].nes";

/// Long enough for the menu to settle; the cart draws it within its first
/// few dozen frames.
const FRAMES: u32 = 300;

/// Boot `bytes` under `model` and return (the first nametable's tile bytes,
/// the final framebuffer).
fn boot(bytes: &[u8], model: ConsoleModel) -> (Vec<u8>, Vec<u8>) {
    let mut nes = Nes::from_rom(bytes).expect("999-in-1 parses");
    // Applied straight after construction, as the frontend does.
    nes.set_console_model(model);
    for _ in 0..FRAMES {
        let _ = nes.run_frame();
    }
    // Mapper 212 selects horizontal or vertical mirroring only, and under
    // both the nametable at $2000 is CIRAM's first 1 KiB; its first 960
    // bytes are the tile map (the rest is the attribute table).
    let tiles = nes.vram()[..0x3C0].to_vec();
    (tiles, nes.run_frame().to_vec())
}

// 960 bytes, counted once per model: the `bytecount` crate the lint suggests
// would be a new dependency for no measurable gain.
#[allow(clippy::naive_bytecount)]
fn count(tiles: &[u8], tile: u8) -> usize {
    tiles.iter().filter(|&&t| t == tile).count()
}

#[test]
fn famicom_model_lets_999_in_1_clear_its_nametable() {
    let path = external_rom_path(ROM);
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("skip: {} not present", path.display());
        return;
    };

    let (nes_tiles, nes_fb) = boot(&bytes, ConsoleModel::Nes);
    let (fc_tiles, fc_fb) = boot(&bytes, ConsoleModel::Famicom);

    if let Ok(dir) = std::env::var("RUSTYNES_FAMICOM_DUMP") {
        let dir = std::path::Path::new(&dir);
        let _ = std::fs::create_dir_all(dir);
        common::write_png(&dir.join("999in1_nes.png"), &nes_fb).expect("png");
        common::write_png(&dir.join("999in1_famicom.png"), &fc_fb).expect("png");
    }

    let nes_zero = count(&nes_tiles, 0x00);
    let fc_zero = count(&fc_tiles, 0x00);
    eprintln!("tile $00 in the first nametable: NES {nes_zero} / 960, Famicom {fc_zero} / 960");

    // NES model (default, unchanged): the clear was dropped, so every cell
    // the menu does not draw keeps the power-on tile $00 (measured 388 of
    // 960 on 2026-10-01; the menu text covers most of the rest).
    assert!(
        nes_zero > 300,
        "NES model: the unwritten cells should still be tile $00 ({nes_zero})"
    );
    // Famicom model: the clear landed, so tile $00 survives only where the
    // menu itself draws a "0" (measured 2).
    assert!(
        fc_zero < 20,
        "Famicom model: the nametable should have been cleared ({fc_zero})"
    );
    assert_ne!(nes_fb, fc_fb, "the two models must render differently");
}

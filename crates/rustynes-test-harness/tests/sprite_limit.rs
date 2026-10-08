//! `T-SPRITE-LIMIT` (v3.1.0, FE-02, D22): "disable sprite limit".
//!
//! `Nes::set_sprite_limit_disabled(true)` draws the sprites beyond the eighth
//! on a scanline. It is render-only, and these tests pin that both ways:
//!
//! * **the game sees nothing.** With the option on, every CPU cycle, every
//!   work-RAM byte and every audio sample matches the option off, frame by
//!   frame, and blargg's five sprite-overflow ROMs (which read the overflow
//!   flag that evaluation sets) still pass;
//! * **the picture does change.** On a ROM built here that keeps sixteen
//!   opaque sprites on one line, the framebuffers differ: without that the
//!   first test would pass for an option that does nothing.
//!
//! No committed ROM could serve as that stimulus, measured: `spritecans`
//! never puts more than seven sprites on a line, and `NEStress` crowds 62
//! onto lines 1-8 but with a transparent tile, so both drew identically with
//! the option on (and the extra sprites WERE fetched on `NEStress`, 54 of
//! them, which is how the blind spot was told apart from a broken feature).
//!
//! Plus the carriage in `HardwareOptions` and a mid-frame snapshot round trip
//! (the extra sprites for the next line are PPU snapshot v13 state).

#![cfg(feature = "test-roms")]

mod common;

use std::fs;

use common::{fnv1a64, rom_path};
use rustynes_core::{HardwareOptions, Nes};

/// A 16 KiB NROM program that keeps sixteen opaque 8x8 sprites on scanline
/// 101 (`Y = 100`, `X = 0, 16, .., 240`, tile 1, palette `$3F11 = $30`),
/// re-sent by OAM DMA every frame with sprites shown and the background off.
/// The hardware draws the left eight; the option adds the right eight.
fn crowded_rom() -> Vec<u8> {
    #[rustfmt::skip]
    let code: [u8; 99] = [
        0x78, 0xD8, 0xA2, 0xFF, 0x9A,             // SEI CLD LDX #$FF TXS
        0x2C, 0x02, 0x20, 0x10, 0xFB,             // vblank 1
        0x2C, 0x02, 0x20, 0x10, 0xFB,             // vblank 2
        0xA9, 0xFF, 0xA2, 0x00,                   // LDA #$FF LDX #0
        0x9D, 0x00, 0x02, 0xE8, 0xD0, 0xFA,       // fill $0200-$02FF with $FF
        0xA2, 0x00, 0xA0, 0x00,                   // LDX #0 LDY #0
        0xA9, 0x64, 0x9D, 0x00, 0x02,             // Y = 100
        0xA9, 0x01, 0x9D, 0x01, 0x02,             // tile 1
        0xA9, 0x00, 0x9D, 0x02, 0x02,             // attributes 0
        0x98, 0x9D, 0x03, 0x02,                   // X = Y register
        0x18, 0x69, 0x10, 0xA8,                   // Y register += 16
        0x8A, 0x18, 0x69, 0x04, 0xAA,             // X register += 4
        0xE0, 0x40, 0xD0, 0xE0,                   // 16 sprites
        0xA9, 0x3F, 0x8D, 0x06, 0x20,             // $2006 = $3F
        0xA9, 0x11, 0x8D, 0x06, 0x20,             // $2006 = $11
        0xA9, 0x30, 0x8D, 0x07, 0x20,             // $3F11 = $30
        0x2C, 0x02, 0x20, 0x10, 0xFB,             // loop: wait for vblank
        0xA9, 0x00, 0x8D, 0x03, 0x20,             // OAMADDR = 0
        0xA9, 0x02, 0x8D, 0x14, 0x40,             // OAM DMA from $0200
        0xA9, 0x14, 0x8D, 0x01, 0x20,             // show sprites (and the left column)
        0x4C, 0x4C, 0xC0,                         // JMP loop ($C04C)
    ];
    let mut rom = vec![b'N', b'E', b'S', 0x1A, 1, 1, 0, 0];
    rom.resize(16, 0);
    let mut prg = vec![0xEAu8; 16 * 1024];
    prg[..code.len()].copy_from_slice(&code);
    // NMI / RESET / IRQ vectors all point at $C000.
    prg[0x3FFA..].copy_from_slice(&[0x00, 0xC0, 0x00, 0xC0, 0x00, 0xC0]);
    rom.extend_from_slice(&prg);
    let mut chr = vec![0u8; 8 * 1024];
    // Tile 1: low plane all set, high plane clear = colour 1, opaque.
    chr[0x10..0x18].fill(0xFF);
    rom.extend_from_slice(&chr);
    rom
}

fn boot(rom: &str) -> Nes {
    if rom == CROWDED {
        return Nes::from_rom(&crowded_rom()).expect("the built ROM parses");
    }
    let path = rom_path(rom);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rom}: {e}"))
}

/// Marker for [`crowded_rom`] in [`boot`].
const CROWDED: &str = "<built: sixteen sprites on one line>";

/// Per frame: (framebuffer hash, RAM hash, audio hash, CPU cycle).
fn trace(disabled: bool, frames: u32) -> Vec<(u64, u64, u64, u64)> {
    let mut nes = boot(CROWDED);
    nes.set_sprite_limit_disabled(disabled);
    (0..frames)
        .map(|_| {
            nes.run_frame();
            let mut audio = Vec::new();
            for s in nes.drain_audio() {
                audio.extend_from_slice(&s.to_le_bytes());
            }
            (
                fnv1a64(nes.framebuffer()),
                fnv1a64(nes.bus().ram_bytes()),
                fnv1a64(&audio),
                nes.cycle(),
            )
        })
        .collect()
}

#[test]
fn off_by_default() {
    assert!(!boot(CROWDED).sprite_limit_disabled());
}

#[test]
fn the_option_changes_the_picture_and_nothing_the_game_can_see() {
    let frames = 300;
    let stock = trace(false, frames);
    let all = trace(true, frames);
    let mut differing_frames = 0;
    for (f, (a, b)) in stock.iter().zip(&all).enumerate() {
        assert_eq!(
            (a.1, a.2, a.3),
            (b.1, b.2, b.3),
            "frame {f}: RAM, audio or CPU cycle moved with the sprite-limit option on; \
             it must be render-only"
        );
        if a.0 != b.0 {
            differing_frames += 1;
        }
    }
    assert!(
        differing_frames > frames / 2,
        "only {differing_frames} of {frames} frames drew differently; \
         the option is not reaching the picture"
    );
}

#[test]
fn the_sprite_overflow_suite_passes_with_the_option_on() {
    for rom in [
        "1.Basics",
        "2.Details",
        "3.Timing",
        "4.Obscure",
        "5.Emulator",
    ] {
        let path = rom_path(&format!("blargg/sprite_overflow_tests/{rom}.nes"));
        let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let r = rustynes_test_harness::run_nes_blargg_with(&bytes, 600, &|nes| {
            nes.set_sprite_limit_disabled(true);
        })
        .expect("runs");
        assert_eq!(r.status, 0, "{rom} with the option on: {}", r.message);
    }
}

#[test]
fn the_option_rides_in_hardware_options() {
    let mut nes = boot(CROWDED);
    let stock = HardwareOptions::capture(&nes);
    nes.set_sprite_limit_disabled(true);
    let opts = HardwareOptions::capture(&nes);
    assert!(opts.sprite_limit_disabled);
    let back = HardwareOptions::read_from(&mut rustynes_core::BinReader::new(&opts.to_bytes()))
        .expect("decodes");
    assert_eq!(back, opts);
    assert_eq!(stock.differences(&opts), vec!["sprite limit"]);
    let mut other = boot(CROWDED);
    opts.apply_live(&mut other).expect("apply");
    assert!(other.sprite_limit_disabled());
}

#[test]
fn a_mid_frame_snapshot_keeps_the_next_lines_extra_sprites() {
    let mut nes = boot(CROWDED);
    nes.set_sprite_limit_disabled(true);
    for _ in 0..120 {
        nes.run_frame();
    }
    // Into the visible frame, between a line's sprite fetch and the next
    // line's pixels.
    while nes.bus().ppu().scanline() < 100 {
        nes.step_instruction();
    }
    // Snapshot where line 101's extra sprites are already fetched (the
    // fetch runs at dots 257-320 of line 100), or the test is blind.
    while nes.bus().ppu().extra_sprite_count() == 0 {
        nes.step_instruction();
    }
    assert_eq!(nes.bus().ppu().extra_sprite_count(), 8);
    let _ = nes.drain_audio();
    let snap = nes.snapshot();
    let run = |nes: &mut Nes| {
        let mut h = Vec::new();
        for _ in 0..3 {
            nes.run_frame();
            h.extend_from_slice(&fnv1a64(nes.framebuffer()).to_le_bytes());
        }
        fnv1a64(&h)
    };
    let first = run(&mut nes);
    nes.restore(&snap).expect("own snapshot restores");
    assert!(
        nes.sprite_limit_disabled(),
        "configuration survives a restore"
    );
    let second = run(&mut nes);
    assert_eq!(
        first, second,
        "a restore mid-frame must redraw the same frames"
    );
}

//! Region timing validation (T-73-005 PAL, T-73-006 Dendy; Phase 7 Sprint 3).
//!
//! Validates that PAL and Dendy timing is driven by the region — not inferred
//! from NTSC constants — by building region-flagged NES 2.0 ROMs and measuring
//! the frame structure through the public `Nes` API.
//!
//! The frame length is the cleanest region discriminator: a frame is
//! `scanlines * 341 / (cpu_divider / ppu_divider)` CPU cycles. NTSC (262 lines,
//! 3:1) ≈ 29,780; Dendy (312 lines, 3:1) ≈ 35,464; PAL (312 lines, **3.2:1**)
//! ≈ 33,247 — the PAL CPU runs at 1.662 MHz against the 50 Hz / 312-line frame.
//! Rendering is disabled in the synth ROM, so the NTSC odd-frame dot-skip never
//! fires and frame length is constant in every region (the skip itself is
//! unit-tested in `rustynes-ppu`).
//!
//! The CPU:PPU clock ratio: under the **R1 master clock** (`mc-r1-full-cpu`, the
//! v2.0 default) the ratio is region-exact — 3:1 NTSC/Dendy, 3.2:1 PAL — so PAL
//! measures the hardware-true 33,247. The legacy integer-lockstep path
//! (`--no-default-features`) approximates PAL at 3:1 (35,464); the PAL
//! expectations below switch on the feature accordingly. History:
//! `docs/audit/pal-dendy-validation-inventory-2026-05-24.md`.

#![cfg(feature = "test-roms")]

use rustynes_core::{Nes, Region};

/// Expected mean PAL CPU cycles/frame: the hardware-true 3.2:1 (312*341/3.2 ≈
/// 33,247) under the R1 master clock; the legacy 3:1 approximation (≈ 35,464)
/// on the non-R1 build.
const PAL_FRAME_CYCLES: core::ops::RangeInclusive<u64> = 33_246..=33_249;

/// Build a minimal NES 2.0 NROM with the given region byte (byte 12 bits 0-1:
/// 0=NTSC, 1=PAL, 2=Multi, 3=Dendy). NES 2.0 is signalled by byte 7 = 0x08.
fn synth_nrom_region(region_byte: u8) -> Vec<u8> {
    let prg_kib = 16usize;
    let chr_kib = 8usize;
    let mut bytes = Vec::with_capacity(16 + prg_kib * 1024 + chr_kib * 1024);
    bytes.extend_from_slice(b"NES\x1A");
    bytes.push(u8::try_from(prg_kib / 16).unwrap()); // byte 4: PRG 16 KiB units
    bytes.push(u8::try_from(chr_kib / 8).unwrap()); // byte 5: CHR 8 KiB units
    bytes.push(0); // byte 6: flags6
    bytes.push(0x08); // byte 7: NES 2.0 marker (bits 2-3 = 0b10)
    bytes.push(0); // byte 8: mapper MSB + submapper
    bytes.push(0); // byte 9: PRG/CHR size MSB nibbles (0 = small ROM)
    bytes.push(0); // byte 10: PRG-RAM/EEPROM
    bytes.push(0); // byte 11: CHR-RAM
    bytes.push(region_byte); // byte 12: timing/region
    bytes.extend_from_slice(&[0u8; 3]); // bytes 13-15

    // PRG: JMP $C000 forever + vectors -> $C000.
    let mut prg = vec![0u8; prg_kib * 1024];
    prg[0] = 0x4C;
    prg[1] = 0x00;
    prg[2] = 0xC0;
    let len = prg.len();
    prg[len - 6] = 0x00; // NMI
    prg[len - 5] = 0xC0;
    prg[len - 4] = 0x00; // RESET
    prg[len - 3] = 0xC0;
    prg[len - 2] = 0x00; // IRQ
    prg[len - 1] = 0xC0;
    bytes.extend_from_slice(&prg);
    bytes.extend_from_slice(&vec![0u8; chr_kib * 1024]);
    bytes
}

/// Number of frames to average over. Per-frame CPU-cycle counts carry ±2
/// instruction-overshoot slop (the JMP loop doesn't align to the dot-exact
/// frame boundary); averaging over many frames recovers the exact rate.
const N: u64 = 64;

/// Mean CPU cycles per frame with rendering disabled (after a 2-frame warm-up
/// to clear the boot partial frame).
fn mean_frame_cycles(region_byte: u8) -> (Region, u64) {
    let rom = synth_nrom_region(region_byte);
    let mut nes = Nes::from_rom(&rom).expect("parse + boot");
    let region = nes.region();
    nes.run_frame();
    nes.run_frame();
    let before = nes.cycle();
    for _ in 0..N {
        nes.run_frame();
    }
    (region, (nes.cycle() - before) / N)
}

#[test]
fn ntsc_frame_is_262_scanlines() {
    let (region, mean) = mean_frame_cycles(0x00);
    assert_eq!(region, Region::Ntsc);
    // 262 * 341 / 3 ≈ 29,780.67.
    assert!(
        (29_779..=29_782).contains(&mean),
        "NTSC mean frame should be ~29,780 CPU cycles, got {mean}"
    );
}

#[test]
fn pal_frame_is_312_scanlines() {
    // T-73-005: PAL region must drive 312 scanlines, not NTSC's 262.
    let (region, mean) = mean_frame_cycles(0x01);
    assert_eq!(region, Region::Pal, "byte 12 = 1 parses as PAL");
    // R1: 312 * 341 / 3.2 ≈ 33,247 (hardware-true). Legacy: 3:1 ≈ 35,464.
    assert!(
        PAL_FRAME_CYCLES.contains(&mean),
        "PAL mean frame should be in {PAL_FRAME_CYCLES:?} CPU cycles, got {mean}"
    );
}

#[test]
fn dendy_frame_is_312_scanlines() {
    // T-73-006: Dendy is 312 scanlines like PAL (VBL position differs, which
    // is unit-tested in rustynes-ppu via `ppu_region_constants_match_hardware`).
    let (region, mean) = mean_frame_cycles(0x03);
    assert_eq!(region, Region::Dendy, "byte 12 = 3 parses as Dendy");
    assert!(
        (35_463..=35_465).contains(&mean),
        "Dendy mean frame should be ~35,464 CPU cycles, got {mean}"
    );
}

#[test]
fn pal_and_dendy_frames_are_longer_than_ntsc() {
    // The region-driven scanline count (312 vs 262) makes PAL/Dendy frames
    // ~19% longer than NTSC — the structural consequence the gate guards.
    let (_, ntsc) = mean_frame_cycles(0x00);
    let (_, pal) = mean_frame_cycles(0x01);
    let (_, dendy) = mean_frame_cycles(0x03);
    // Both 312-line regions exceed NTSC's 262-line frame — the structural
    // consequence the gate guards, true in both clock models.
    assert!(pal > ntsc, "PAL ({pal}) must exceed NTSC ({ntsc})");
    assert!(dendy > ntsc, "Dendy ({dendy}) must exceed NTSC ({ntsc})");
    // Under the R1 master clock the ratios are region-exact, so PAL (3.2:1) has
    // FEWER CPU cycles/frame than Dendy (3:1) despite the shared 312-line count;
    // the legacy 3:1 path treats them identically.
    assert!(
        dendy > pal,
        "Dendy 3:1 ({dendy}) must exceed PAL 3.2:1 ({pal}) in CPU cycles/frame"
    );
}

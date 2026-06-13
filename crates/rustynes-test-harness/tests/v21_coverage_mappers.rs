//! Boot-smoke tests for the v2.1.0 coverage mappers — Tier 1 (70, 88, 206,
//! 73, 16, 159, 18) and Tier 2 (64, 65, 67, 68, 78, 118, 210).
//!
//! There are no redistributable behavioral test fixtures for these mappers
//! (`holy_mapperel` covers only the already-implemented set, and no committed
//! commercial ROM uses them). Verification here is therefore **boot-smoke
//! only**: build a synthetic minimal iNES ROM declaring the mapper, run it
//! through the full `Nes` for ~60 frames, and assert no panic. Per-mapper
//! register/IRQ behaviour is unit-tested in `rustynes-mappers` against the nesdev
//! spec; this file proves the parse + dispatch + run-loop integration.
#![cfg(feature = "test-roms")]
#![allow(clippy::doc_markdown)]

use rustynes_core::Nes;

/// Build a minimal iNES 1.0 ROM for `mapper_num` with `prg_banks_16k` 16 KiB
/// PRG banks and either CHR-ROM (`chr_banks_8k > 0`) or CHR-RAM (0). The PRG
/// is filled with `JMP $C000` at the reset vector target so the CPU spins
/// harmlessly. `submapper` is written into the NES 2.0 byte 8 high nibble and
/// byte 7 is flagged NES 2.0 when `submapper != 0`.
fn synth_rom(mapper_num: u16, submapper: u8, prg_banks_16k: usize, chr_banks_8k: usize) -> Vec<u8> {
    let prg_size = prg_banks_16k * 16 * 1024;
    let chr_size = chr_banks_8k * 8 * 1024;
    let mut bytes = Vec::with_capacity(16 + prg_size + chr_size);
    bytes.extend_from_slice(b"NES\x1A");
    bytes.push(u8::try_from(prg_banks_16k).unwrap()); // byte 4: PRG 16 KiB units
    bytes.push(u8::try_from(chr_banks_8k).unwrap()); // byte 5: CHR 8 KiB units

    let m_lo = (mapper_num & 0x0F) as u8;
    let m_mid = ((mapper_num >> 4) & 0x0F) as u8;
    // byte 6: low mapper nibble in bits 4-7 + flags (vertical mirroring bit 0).
    bytes.push((m_lo << 4) | 0x01);
    // byte 7: high mapper nibble in bits 4-7; NES 2.0 marker (bits 2-3 = 10)
    // only when a submapper is needed.
    let nes2 = if submapper != 0 { 0x08 } else { 0x00 };
    bytes.push((m_mid << 4) | nes2);
    // byte 8: submapper (high nibble) + mapper MSB (low nibble = 0).
    bytes.push(submapper << 4);
    bytes.extend_from_slice(&[0u8; 7]); // bytes 9-15

    // PRG payload: every bank starts with JMP $C000; both the reset and IRQ
    // vectors (in the last bank) point at $C000 so the CPU loops forever and
    // any mapper-driven IRQ is serviced into the same spin.
    let mut prg = vec![0u8; prg_size];
    for bank in 0..prg_banks_16k {
        let base = bank * 16 * 1024;
        prg[base] = 0x4C; // JMP abs
        prg[base + 1] = 0x00;
        prg[base + 2] = 0xC0;
    }
    let len = prg.len();
    prg[len - 6] = 0x00; // NMI low
    prg[len - 5] = 0xC0; // NMI high
    prg[len - 4] = 0x00; // RESET low
    prg[len - 3] = 0xC0; // RESET high
    prg[len - 2] = 0x00; // IRQ low
    prg[len - 1] = 0xC0; // IRQ high
    bytes.extend_from_slice(&prg);

    // CHR-ROM (if any).
    bytes.extend(core::iter::repeat_n(0u8, chr_size));
    bytes
}

fn boot_smoke(rom: &[u8], expected_mapper: u16) {
    let (cart, _mapper) = rustynes_core::rustynes_mappers::parse(rom).expect("ROM must parse");
    assert_eq!(cart.mapper_id, expected_mapper);
    let mut nes = Nes::from_rom(rom).expect("parse + boot");
    for _ in 0..60 {
        nes.run_frame();
    }
}

#[test]
fn mapper_70_bandai_discrete_boots() {
    // 8 banks PRG, 1 bank (8 KiB) CHR-ROM.
    boot_smoke(&synth_rom(70, 0, 8, 1), 70);
}

#[test]
fn mapper_88_namco118_a16_boots() {
    boot_smoke(&synth_rom(88, 0, 8, 2), 88);
}

#[test]
fn mapper_206_dxrom_boots() {
    boot_smoke(&synth_rom(206, 0, 8, 2), 206);
}

#[test]
fn mapper_73_vrc3_boots() {
    // VRC3 has 8 KiB CHR-RAM only; supply no CHR-ROM.
    boot_smoke(&synth_rom(73, 0, 8, 0), 73);
}

#[test]
fn mapper_16_bandai_fcg_sub0_boots() {
    boot_smoke(&synth_rom(16, 0, 8, 2), 16);
}

#[test]
fn mapper_16_bandai_fcg_sub5_lz93d50_boots() {
    boot_smoke(&synth_rom(16, 5, 8, 2), 16);
}

#[test]
fn mapper_159_bandai_lz93d50_x24c01_boots() {
    boot_smoke(&synth_rom(159, 0, 8, 2), 159);
}

#[test]
fn mapper_18_jaleco_ss88006_boots() {
    boot_smoke(&synth_rom(18, 0, 16, 4), 18);
}

// ----- Tier 2 -----

#[test]
fn mapper_64_rambo1_boots() {
    // 16 banks PRG (256 KiB), 4 banks (8 KiB) CHR.
    boot_smoke(&synth_rom(64, 0, 16, 4), 64);
}

#[test]
fn mapper_65_irem_h3001_boots() {
    boot_smoke(&synth_rom(65, 0, 16, 4), 65);
}

#[test]
fn mapper_67_sunsoft3_boots() {
    boot_smoke(&synth_rom(67, 0, 8, 2), 67);
}

#[test]
fn mapper_68_sunsoft4_boots() {
    boot_smoke(&synth_rom(68, 0, 8, 4), 68);
}

#[test]
fn mapper_68_sunsoft4_nt_rom_mode_boots() {
    // Drive a few frames after enabling CHR-ROM nametable mode is exercised
    // by the unit tests; here we just confirm the default boot path.
    boot_smoke(&synth_rom(68, 0, 8, 2), 68);
}

#[test]
fn mapper_78_holy_diver_default_boots() {
    // No submapper -> Holy Diver (H/V) variant.
    boot_smoke(&synth_rom(78, 0, 8, 2), 78);
}

#[test]
fn mapper_78_cosmo_carrier_sub1_boots() {
    // Submapper 1 -> Cosmo Carrier (single-screen) variant.
    boot_smoke(&synth_rom(78, 1, 8, 2), 78);
}

#[test]
fn mapper_118_txsrom_boots() {
    boot_smoke(&synth_rom(118, 0, 16, 4), 118);
}

#[test]
fn mapper_119_tqrom_boots() {
    // TQROM is MMC3 + mixed CHR (64 KiB CHR-ROM + 8 KiB CHR-RAM). Declare a
    // full 64 KiB CHR-ROM (8 x 8 KiB units); the 8 KiB CHR-RAM is allocated
    // internally by the mapper.
    boot_smoke(&synth_rom(119, 0, 16, 8), 119);
}

#[test]
fn mapper_210_namco175_sub1_boots() {
    boot_smoke(&synth_rom(210, 1, 16, 4), 210);
}

#[test]
fn mapper_210_namco340_sub2_boots() {
    boot_smoke(&synth_rom(210, 2, 16, 4), 210);
}

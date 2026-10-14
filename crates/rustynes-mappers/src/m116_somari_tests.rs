// SPDX-License-Identifier: GPL-3.0-or-later
//! Register-decode tests for `m116_somari.rs`, written from
//! `INES_Mapper_116.md`, `VRC2_and_VRC4.md`, `MMC1.md` and `MMC3.md`.

use alloc::vec::Vec;

/// An iNES 1.0 image for mapper 116: every 8 KiB PRG bank and every 1 KiB
/// CHR bank starts with its own bank number (low, high).
fn ines_image(prg_16k: u8, chr_8k: u8) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"NES\x1A");
    v.push(prg_16k);
    v.push(chr_8k);
    v.push(0x40); // mapper low nibble 4
    v.push(0x70); // mapper high nibble 7 -> 0x74 = 116
    v.extend_from_slice(&[0u8; 8]);
    for b in 0..usize::from(prg_16k) * 2 {
        let mut bank = alloc::vec![0u8; 0x2000];
        bank[0] = b as u8;
        bank[1] = (b >> 8) as u8;
        v.extend_from_slice(&bank);
    }
    for b in 0..usize::from(chr_8k) * 8 {
        let mut bank = alloc::vec![0u8; 0x400];
        bank[0] = b as u8;
        bank[1] = (b >> 8) as u8;
        v.extend_from_slice(&bank);
    }
    v
}

#[test]
fn m116_parses() {
    assert!(
        crate::parse(&ines_image(16, 32)).is_ok(),
        "mapper 116 must construct"
    );
}

use super::*;

fn prg_image(banks_8k: usize) -> Box<[u8]> {
    let mut v = vec![0u8; banks_8k * PRG_BANK_8K];
    for b in 0..banks_8k {
        v[b * PRG_BANK_8K] = b as u8;
        v[b * PRG_BANK_8K + 1] = (b >> 8) as u8;
    }
    v.into_boxed_slice()
}

fn chr_image(banks_1k: usize) -> Box<[u8]> {
    let mut v = vec![0u8; banks_1k * CHR_BANK_1K];
    for b in 0..banks_1k {
        v[b * CHR_BANK_1K] = b as u8;
        v[b * CHR_BANK_1K + 1] = (b >> 8) as u8;
    }
    v.into_boxed_slice()
}

/// 256 KiB PRG (32 x 8 KiB), 512 KiB CHR (512 x 1 KiB): Huang-1.
fn board() -> Somari116 {
    Somari116::new(prg_image(32), chr_image(512), Mirroring::Vertical).expect("valid")
}

fn prg_at(m: &mut Somari116, addr: u16) -> usize {
    m.cpu_read(addr) as usize | (m.cpu_read(addr + 1) as usize) << 8
}

fn chr_at(m: &mut Somari116, addr: u16) -> usize {
    m.ppu_read(addr) as usize | (m.ppu_read(addr + 1) as usize) << 8
}

/// The MMC1 serial port: five writes, LSB first.
fn mmc1_write(m: &mut Somari116, addr: u16, value: u8) {
    for i in 0..5 {
        m.cpu_write(addr, (value >> i) & 1);
    }
}

fn clock_a12(m: &mut Somari116) {
    m.notify_a12(false);
    for _ in 0..4 {
        m.notify_cpu_cycle();
    }
    m.notify_a12(true);
    m.notify_cpu_cycle();
}

/// The supervisor answers where `addr & $E100 == $4100`.
#[test]
fn m116_supervisor_decode() {
    for (addr, hit) in [
        (0x4100u16, true),
        (0x41FF, true),
        (0x5F00, true),
        (0x4200, false),
        (0x5000, false),
        (0x6100, false),
    ] {
        let mut m = board();
        m.cpu_write(addr, 0x01);
        assert_eq!(m.mode() == Mode::Mmc3, hit, "{addr:#06x}");
    }
}

/// Power-on is VRC2 mode with every CHR select at `$FF`.
#[test]
fn m116_powers_on_in_vrc2_mode_with_chr_ff() {
    let mut m = board();
    assert_eq!(m.mode(), Mode::Vrc2);
    for slot in 0..8u16 {
        assert_eq!(chr_at(&mut m, slot * 0x400), 0xFF, "slot {slot}");
    }
}

/// VRC2b: PRG 0 at `$8xxx`, mirroring at `$9xxx`, PRG 1 at `$Axxx`, each in
/// its whole 4 KiB (*Garō Densetsu Special* writes the mirrors); the last
/// 16 KiB fixed.
#[test]
fn m116_vrc2_prg_and_mirroring_answer_in_their_whole_page() {
    let mut m = board();
    m.cpu_write(0x8FFF, 0x05);
    m.cpu_write(0xA7A3, 0x09);
    assert_eq!(prg_at(&mut m, 0x8000), 5);
    assert_eq!(prg_at(&mut m, 0xA000), 9);
    assert_eq!(prg_at(&mut m, 0xC000), 30);
    assert_eq!(prg_at(&mut m, 0xE000), 31);
    m.cpu_write(0x9ABC, 0x01);
    assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    m.cpu_write(0x9000, 0x02); // bit 1 ignored on the VRC2
    assert_eq!(m.current_mirroring(), Mirroring::Vertical);
}

/// VRC2b CHR selects: A12 and A1 pick the select, A0 the nibble, and the
/// VRC2 has only four high bits.
#[test]
fn m116_vrc2_chr_selects() {
    let mut m = board();
    for (lo, hi, window) in [
        (0xB000u16, 0xB001u16, 0x0000u16),
        (0xB002, 0xB003, 0x0400),
        (0xC000, 0xC001, 0x0800),
        (0xC002, 0xC003, 0x0C00),
        (0xD000, 0xD001, 0x1000),
        (0xD002, 0xD003, 0x1400),
        (0xE000, 0xE001, 0x1800),
        (0xE002, 0xE003, 0x1C00),
    ] {
        m.cpu_write(lo, 0x3A); // low nibble A
        m.cpu_write(hi, 0x15); // bit 4 ignored: high nibble 5
        assert_eq!(chr_at(&mut m, window), 0x5A, "{lo:#06x}/{hi:#06x}");
    }
}

/// MMC3 mode: a stock MMC3 with its IRQ, which drives the line only while
/// MMC3 mode is selected.
#[test]
fn m116_mmc3_mode_and_its_irq() {
    let mut m = board();
    m.cpu_write(0x4100, 0x01);
    m.cpu_write(0x8000, 6);
    m.cpu_write(0x8001, 0x07);
    m.cpu_write(0x8000, 2);
    m.cpu_write(0x8001, 0x33);
    assert_eq!(prg_at(&mut m, 0x8000), 7);
    assert_eq!(prg_at(&mut m, 0xE000), 31);
    assert_eq!(chr_at(&mut m, 0x1000), 0x33);
    m.cpu_write(0xA000, 1);
    assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    m.cpu_write(0xC000, 1);
    m.cpu_write(0xC001, 0);
    m.cpu_write(0xE001, 0);
    clock_a12(&mut m);
    clock_a12(&mut m);
    assert!(m.irq_pending());
    m.cpu_write(0x4100, 0x00);
    assert!(!m.irq_pending(), "VRC2 mode: the MMC3 is deselected");
    m.cpu_write(0x4100, 0x01);
    assert!(m.irq_pending(), "its IRQ state survives the switch");
}

/// "All three simulated chips have their separate registers": each chip's
/// banks come back when it is selected again.
#[test]
fn m116_each_chip_keeps_its_own_registers() {
    let mut m = board();
    m.cpu_write(0x8000, 0x05); // VRC2 PRG 0
    m.cpu_write(0x4100, 0x01);
    m.cpu_write(0x8000, 6);
    m.cpu_write(0x8001, 0x09); // MMC3 R6
    m.cpu_write(0x4100, 0x02);
    mmc1_write(&mut m, 0xE000, 0x03); // MMC1 PRG
    assert_eq!(prg_at(&mut m, 0x8000), 6, "MMC1: 16 KiB bank 3");
    m.cpu_write(0x4100, 0x00);
    assert_eq!(prg_at(&mut m, 0x8000), 5, "VRC2 again");
    m.cpu_write(0x4100, 0x01);
    assert_eq!(prg_at(&mut m, 0x8000), 9, "MMC3 again");
    m.cpu_write(0x4100, 0x03);
    assert_eq!(prg_at(&mut m, 0x8000), 6, "mode 3 is MMC1 too");
}

/// MMC1 mode: PRG mode 3 (switch `$8000`, last bank at `$C000`), 4 KiB and
/// 8 KiB CHR modes, and the control register's mirroring.
#[test]
fn m116_mmc1_mode_banks() {
    let mut m = board();
    m.cpu_write(0x4100, 0x02);
    mmc1_write(&mut m, 0x8000, 0x1F); // control: PRG mode 3, CHR 4 KiB, H
    mmc1_write(&mut m, 0xE000, 0x02);
    assert_eq!(prg_at(&mut m, 0x8000), 4);
    assert_eq!(prg_at(&mut m, 0xC000), 30);
    mmc1_write(&mut m, 0xA000, 0x05);
    mmc1_write(&mut m, 0xC000, 0x09);
    assert_eq!(chr_at(&mut m, 0x0000), 5 * 4);
    assert_eq!(chr_at(&mut m, 0x1C00), 9 * 4 + 3);
    assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    mmc1_write(&mut m, 0x8000, 0x02); // PRG mode 0 (32 KiB), CHR 8 KiB, V
    assert_eq!(prg_at(&mut m, 0x8000), 4);
    assert_eq!(prg_at(&mut m, 0xC000), 6);
    // 8 KiB mode: CHR 0's low bit is ignored, A12 picks the half:
    // (5 & $1E) | 1 = 4 KiB bank 5 = 1 KiB bank 20 at `$1000`.
    assert_eq!(chr_at(&mut m, 0x1000), 20);
    assert_eq!(chr_at(&mut m, 0x0000), 16);
    assert_eq!(m.current_mirroring(), Mirroring::Vertical);
}

/// "When switching to MMC1 mode, the serial shift register is reset": three
/// stray bits written before a switch away and back do not corrupt the next
/// five-write sequence. Re-selecting MMC1 from MMC1 (mode 2 -> 3) is not a
/// switch.
#[test]
fn m116_switching_to_mmc1_resets_the_shift_register() {
    let mut m = board();
    m.cpu_write(0x4100, 0x02);
    for _ in 0..3 {
        m.cpu_write(0xE000, 1);
    }
    m.cpu_write(0x4100, 0x01);
    m.cpu_write(0x4100, 0x02);
    mmc1_write(&mut m, 0xE000, 0x02);
    assert_eq!(prg_at(&mut m, 0x8000), 4, "a clean five-write commit");

    let mut m = board();
    m.cpu_write(0x4100, 0x02);
    for _ in 0..3 {
        m.cpu_write(0xE000, 1);
    }
    m.cpu_write(0x4100, 0x03); // still MMC1: no reset
    m.cpu_write(0xE000, 0);
    m.cpu_write(0xE000, 0); // fifth bit: commits 0b00111
    assert_eq!(prg_at(&mut m, 0x8000), 14, "the shift continued");
}

/// The supervisor's bit 2 is CHR A18 in every mode.
#[test]
fn m116_outer_chr_bank_in_every_mode() {
    let mut m = board();
    m.cpu_write(0xB000, 0x04);
    m.cpu_write(0xB001, 0x00);
    m.cpu_write(0x4100, 0x04);
    assert_eq!(chr_at(&mut m, 0x0000), 0x104, "VRC2");
    m.cpu_write(0x4100, 0x05);
    m.cpu_write(0x8000, 2);
    m.cpu_write(0x8001, 0x21);
    assert_eq!(chr_at(&mut m, 0x1000), 0x121, "MMC3");
    m.cpu_write(0x4100, 0x06);
    mmc1_write(&mut m, 0x8000, 0x1C);
    mmc1_write(&mut m, 0xA000, 0x03);
    assert_eq!(chr_at(&mut m, 0x0000), 0x100 + 12, "MMC1");
}

/// Huang-2 (PRG-ROM and CHR-ROM both 128 KiB): the MMC1 PRG register is
/// written one bit to the left. Huang-1 is not.
#[test]
fn m116_huang2_shifts_the_mmc1_prg_register() {
    let mut m = Somari116::new(prg_image(16), chr_image(128), Mirroring::Vertical).expect("valid");
    assert!(m.huang2);
    m.cpu_write(0x4100, 0x02);
    mmc1_write(&mut m, 0xE000, 0x06);
    assert_eq!(prg_at(&mut m, 0x8000), 6, "written 6 = bank 3");
    let mut m = board();
    assert!(!m.huang2);
    m.cpu_write(0x4100, 0x02);
    mmc1_write(&mut m, 0xE000, 0x06);
    assert_eq!(prg_at(&mut m, 0x8000), 12, "written 6 = bank 6");
}

#[test]
fn m116_state_round_trip() {
    let mut m = board();
    m.cpu_write(0x8000, 0x05);
    m.cpu_write(0xB001, 0x03);
    m.cpu_write(0x4100, 0x05);
    m.cpu_write(0x8000, 6);
    m.cpu_write(0x8001, 0x09);
    m.ppu_write(0x2123, 0x44);
    let blob = m.save_state();
    let mut n = board();
    n.load_state(&blob).expect("round trip");
    assert_eq!(n.save_state(), blob);
    assert_eq!(n.ppu_read(0x2123), 0x44);
    let mut bad = blob.clone();
    bad[13..17].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(board().load_state(&bad).is_err());
}

/// A state whose MMC1 part is refused must leave the board as it was: the MMC3
/// part used to be restored first and kept when the MMC1 part then failed,
/// mixing new MMC3 state with the old MMC1, VRC2 and RAM (`CodeRabbit` on #608).
#[test]
fn m116_a_refused_mmc1_part_leaves_the_mmc3_untouched() {
    let mut src = board();
    // MMC3 mode first: in the power-on VRC2 mode these writes never reach the
    // MMC3, the two boards' MMC3 states would be equal, and the test could not
    // see the bug (its first draft did exactly that; mutation found it).
    src.cpu_write(0x4100, 0x01);
    src.cpu_write(0x8000, 0x06); // MMC3 bank select: R6
    src.cpu_write(0x8001, 0x03);
    let mut bad = src.save_state();
    let mmc3_len = u32::from_le_bytes([bad[13], bad[14], bad[15], bad[16]]) as usize;
    bad[HEAD + mmc3_len] = 0xEE; // the MMC1 blob's version byte

    let mut m = board();
    let before = m.save_state();
    assert!(m.load_state(&bad).is_err());
    assert_eq!(m.save_state(), before, "nothing restored");
}

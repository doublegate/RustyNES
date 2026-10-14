// SPDX-License-Identifier: GPL-3.0-or-later
//! Register-decode tests for `mmc3_boards_ext.rs`.

use alloc::vec::Vec;

/// A NES 2.0 image for `mapper`.`sub`: every 8 KiB PRG bank and every 1 KiB
/// CHR bank starts with its own bank number (low, high).
fn nes2_image(mapper: u16, sub: u8, prg_16k: u8, chr_8k: u8) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"NES\x1A");
    v.push(prg_16k);
    v.push(chr_8k);
    v.push(((mapper & 0x0F) << 4) as u8);
    v.push((mapper & 0xF0) as u8 | 0x08);
    v.push(((mapper >> 8) & 0x0F) as u8 | (sub << 4));
    v.extend_from_slice(&[0u8; 7]);
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
fn m114_and_m182_parse() {
    for (id, sub) in [(114u16, 0u8), (114, 1), (182, 0)] {
        assert!(
            crate::parse(&nes2_image(id, sub, 16, 32)).is_ok(),
            "mapper {id}.{sub} must construct"
        );
    }
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

fn board(kind: ExtBoard, prg_banks: usize, chr_banks: usize) -> Mmc3BoardExt {
    Mmc3BoardExt::new(
        kind,
        prg_image(prg_banks),
        chr_image(chr_banks),
        Mirroring::Vertical,
        0,
    )
    .expect("valid sizes")
}

fn prg_at(m: &mut Mmc3BoardExt, addr: u16) -> usize {
    m.cpu_read(addr) as usize | (m.cpu_read(addr + 1) as usize) << 8
}

fn chr_at(m: &mut Mmc3BoardExt, addr: u16) -> usize {
    m.ppu_read(addr) as usize | (m.ppu_read(addr + 1) as usize) << 8
}

/// One filtered A12 rise (the MMC3 needs three M2 cycles of A12 low).
fn clock_a12(m: &mut Mmc3BoardExt) {
    m.notify_a12(false);
    for _ in 0..4 {
        m.notify_cpu_cycle();
    }
    m.notify_a12(true);
    // The MMC3 core asserts a counter-zero IRQ one CPU cycle after the
    // rise (`irq_assert_pending_next_cycle`); let that cycle pass.
    m.notify_cpu_cycle();
}

const M114_0: ExtBoard = ExtBoard::M114 { sub1: false };
const M114_1: ExtBoard = ExtBoard::M114 { sub1: true };

// ---------------------------------------------------------------------------
// Mapper 114 (`INES_Mapper_114.md`).
// ---------------------------------------------------------------------------

/// Submapper 0: bank select is written at `$A000`, bank data at `$C000`, and
/// the written index `w` means MMC3 register `0 3 1 5 6 7 2 4`[w]. Each case
/// is a literal row of the page's table, not a lookup in the code's.
#[test]
fn m114_sub0_index_and_address_scramble_follow_the_page() {
    // (written index, MMC3 register meant, where to observe it)
    for (written, reg, window, is_prg) in [
        (4u8, 6u8, 0x8000u16, true),
        (5, 7, 0xA000, true),
        (0, 0, 0x0000, false),
        (2, 1, 0x0800, false),
        (6, 2, 0x1000, false),
        (1, 3, 0x1400, false),
        (7, 4, 0x1800, false),
        (3, 5, 0x1C00, false),
    ] {
        let mut m = board(M114_0, 32, 256);
        m.cpu_write(0xA000, written); // real $8000
        m.cpu_write(0xC000, 0x12); // real $8001
        let got = if is_prg {
            prg_at(&mut m, window)
        } else {
            chr_at(&mut m, window)
        };
        assert_eq!(got, 0x12, "written index {written} must reach R{reg}");
        // The plain MMC3 addresses do not select banks on this board:
        // `$8000` is `$A001` and `$8001` is `$A000` (mirroring).
        m.cpu_write(0x8000, 6);
        m.cpu_write(0x8001, 0x03);
        assert_eq!(
            m.current_mirroring(),
            Mirroring::Horizontal,
            "$8001 is $A000"
        );
    }
}

/// Submapper 0: the bank-select mode bits (6 and 7) pass through unchanged,
/// so PRG mode 1 moves R6 to `$C000` and CHR mode 1 moves R2 to `$0000`.
#[test]
fn m114_sub0_mode_bits_are_not_scrambled() {
    let mut m = board(M114_0, 32, 256);
    m.cpu_write(0xA000, 0x40 | 4); // PRG mode 1, R6
    m.cpu_write(0xC000, 0x09);
    assert_eq!(prg_at(&mut m, 0xC000), 9);
    assert_eq!(prg_at(&mut m, 0x8000), 30, "second-last bank at $8000");
    m.cpu_write(0xA000, 0x80 | 6); // CHR mode 1, R2
    m.cpu_write(0xC000, 0x21);
    assert_eq!(chr_at(&mut m, 0x0000), 0x21);
}

/// Submapper 0 IRQ: latch at `$A001` (real `$C000`), reload at `$C001`,
/// disable/enable at `$E000`/`$E001` (unscrambled). The MMC3A behaviour the
/// page names: a latch of 0 never raises the IRQ.
#[test]
fn m114_sub0_irq_registers_and_mmc3a_zero_latch() {
    let mut m = board(M114_0, 32, 256);
    m.cpu_write(0xA001, 2); // latch
    m.cpu_write(0xC001, 0); // reload
    m.cpu_write(0xE001, 0); // enable
    clock_a12(&mut m); // reload -> 2
    clock_a12(&mut m); // 1
    assert!(!m.irq_pending());
    clock_a12(&mut m); // 0 -> IRQ
    assert!(m.irq_pending(), "latch 2 fires on the third clock");
    m.cpu_write(0xE000, 0); // disable + acknowledge
    assert!(!m.irq_pending());

    let mut m = board(M114_0, 32, 256);
    m.cpu_write(0xA001, 0);
    m.cpu_write(0xC001, 0);
    m.cpu_write(0xE001, 0);
    // The `$C001` reload itself raises one IRQ on both revisions
    // (`MMC3.md`: the alternate revision checks the 1->0 transition
    // "whether from decrementing or reloading"); acknowledge it.
    clock_a12(&mut m);
    m.cpu_write(0xE000, 0);
    m.cpu_write(0xE001, 0);
    for _ in 0..8 {
        clock_a12(&mut m);
    }
    assert!(!m.irq_pending(), "MMC3A: a latch of 0 disables the IRQ");
}

/// Submapper 1: `$8000`→`$A001`, `$8001`→`$8001`, `$A000`→`$8000`,
/// `$A001`→`$C001`, `$C000`→`$A000`, `$C001`→`$C000`; written index `w`
/// means `0 2 5 3 6 1 7 4`[w].
#[test]
fn m114_sub1_scramble_follows_the_page() {
    for (written, window, is_prg) in [
        (4u8, 0x8000u16, true), // R6
        (6, 0xA000, true),      // R7
        (0, 0x0000, false),     // R0
        (5, 0x0800, false),     // R1
        (1, 0x1000, false),     // R2
        (3, 0x1400, false),     // R3
        (7, 0x1800, false),     // R4
        (2, 0x1C00, false),     // R5
    ] {
        let mut m = board(M114_1, 32, 256);
        m.cpu_write(0xA000, written);
        m.cpu_write(0x8001, 0x0A);
        let got = if is_prg {
            prg_at(&mut m, window)
        } else {
            chr_at(&mut m, window)
        };
        assert_eq!(got, 0x0A, "written index {written} at {window:#06x}");
    }
    let mut m = board(M114_1, 32, 256);
    m.cpu_write(0xC000, 1);
    assert_eq!(
        m.current_mirroring(),
        Mirroring::Horizontal,
        "$C000 is $A000"
    );
    // IRQ latch at `$C001` (real `$C000`), reload at `$A001` (real `$C001`).
    m.cpu_write(0xC001, 1);
    m.cpu_write(0xA001, 0);
    m.cpu_write(0xE001, 0);
    clock_a12(&mut m);
    clock_a12(&mut m);
    assert!(m.irq_pending(), "latch 1 fires on the second clock");
}

/// `$6000` `M.S. BBBb`: with M set, a 16 KiB bank replaces both halves
/// (NROM-128); with S also set, CPU A14 replaces bit 0 (NROM-256).
#[test]
fn m114_nrom_override() {
    let mut m = board(M114_0, 32, 256);
    m.cpu_write(0x6000, 0x80 | 0x03);
    for (addr, bank) in [(0x8000, 6), (0xA000, 7), (0xC000, 6), (0xE000, 7)] {
        assert_eq!(prg_at(&mut m, addr), bank, "NROM-128 at {addr:#06x}");
    }
    m.cpu_write(0x6000, 0xA0 | 0x03);
    for (addr, bank) in [(0x8000, 4), (0xA000, 5), (0xC000, 6), (0xE000, 7)] {
        assert_eq!(prg_at(&mut m, addr), bank, "NROM-256 at {addr:#06x}");
    }
    m.cpu_write(0x6000, 0x23); // M clear: MMC3 banks, bits 0-3/5 ignored.
    assert_eq!(prg_at(&mut m, 0xE000), 31);
    assert_eq!(prg_at(&mut m, 0xC000), 30);
}

/// `$6001` bit 0 is CHR A18, and both `$600x` registers work with the
/// MMC3's WRAM disabled ("not connected to the MMC3 clone's WRAM
/// interface").
#[test]
fn m114_outer_chr_bank_ignores_the_wram_bits() {
    let mut m = board(M114_0, 32, 512);
    m.cpu_write(0x8000, 0x00); // real $A001: WRAM disabled
    m.cpu_write(0xA000, 6); // R2
    m.cpu_write(0xC000, 0x05);
    m.cpu_write(0x6001, 0x01);
    assert_eq!(chr_at(&mut m, 0x1000), 0x105);
    m.cpu_write(0x6001, 0x00);
    assert_eq!(chr_at(&mut m, 0x1000), 0x005);
    m.cpu_write(0x6000, 0x80);
    assert_eq!(prg_at(&mut m, 0xC000), 0, "$6000 taken with WRAM disabled");
}

/// 182 is dispatched as 114 submapper 0; 114 submapper 1 is its own variant.
#[test]
fn m182_is_m114_submapper_0() {
    let (_, m) = crate::parse(&nes2_image(182, 0, 16, 32)).expect("182");
    assert_eq!(m.debug_info().mapper_id, 114);
    assert!(m.debug_info().name.contains("114.0"));
    let (_, m) = crate::parse(&nes2_image(114, 1, 16, 32)).expect("114.1");
    assert!(m.debug_info().name.contains("114.1"));
}

/// The state round-trips, and a state for one scramble variant does not load
/// onto the other.
#[test]
fn m114_state_round_trip_and_variant_refusal() {
    let mut m = board(M114_0, 32, 256);
    m.cpu_write(0x6000, 0xA5);
    m.cpu_write(0x6001, 0x01);
    m.cpu_write(0xA000, 4);
    m.cpu_write(0xC000, 0x07);
    let blob = m.save_state();
    let mut n = board(M114_0, 32, 256);
    n.load_state(&blob).expect("same variant");
    assert_eq!(n.save_state(), blob);
    assert!(board(M114_1, 32, 256).load_state(&blob).is_err());
    let mut bad = blob.clone();
    bad[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(board(M114_0, 32, 256).load_state(&bad).is_err());
}

// ---------------------------------------------------------------------------
// Mapper 215 (`INES_Mapper_215.md`).
// ---------------------------------------------------------------------------

#[test]
fn m215_parses() {
    for sub in [0u8, 1] {
        assert!(
            crate::parse(&nes2_image(215, sub, 32, 64)).is_ok(),
            "mapper 215.{sub} must construct"
        );
    }
}

const M215_0: ExtBoard = ExtBoard::M215 { sub1: false };
const M215_1: ExtBoard = ExtBoard::M215 { sub1: true };

/// Each `$5007` pattern, pinned to literal cells of the page's two tables:
/// where the bank select and bank data must be written, and which register
/// a written index then reaches.
#[test]
fn m215_scramble_patterns_follow_the_page() {
    // (pattern, select addr, data addr, written index, observe at, is PRG)
    for (pat, sel, data, written, window, is_prg) in [
        // 0, 5, 6, 7: the identity. Index 6 is R6.
        (0u8, 0x8000u16, 0x8001u16, 6u8, 0x8000u16, true),
        (5, 0x8000, 0x8001, 6, 0x8000, true),
        (6, 0x8000, 0x8001, 6, 0x8000, true),
        (7, 0x8000, 0x8001, 6, 0x8000, true),
        // 1: `$A000` is `$8000`, `$C000` is `$8001`; written 4 means 7.
        (1, 0xA000, 0xC000, 4, 0xA000, true),
        // 1: written 2 means 6.
        (1, 0xA000, 0xC000, 2, 0x8000, true),
        // 2: addresses unscrambled; written 1 means 5 (R5 at `$1C00`).
        (2, 0x8000, 0x8001, 1, 0x1C00, false),
        // 3: `$8001` is `$8000`, `$A000` is `$8001`; written 1 means 6.
        (3, 0x8001, 0xA000, 1, 0x8000, true),
        // 3: written 4 means 5.
        (3, 0x8001, 0xA000, 4, 0x1C00, false),
        // 4: `$A000` is `$8000`, `$8001` stays `$8001`; written 4 means 6.
        (4, 0xA000, 0x8001, 4, 0x8000, true),
        // 4: written 1 means 2 (R2 at `$1000`).
        (4, 0xA000, 0x8001, 1, 0x1000, false),
    ] {
        let mut m = board(M215_0, 32, 256);
        m.cpu_write(0x5007, pat);
        m.cpu_write(sel, written);
        m.cpu_write(data, 0x0D);
        let got = if is_prg {
            prg_at(&mut m, window)
        } else {
            chr_at(&mut m, window)
        };
        assert_eq!(got, 0x0D, "pattern {pat}, written index {written}");
    }
}

/// Pattern 3 moves the IRQ registers: `$8000` is `$C001` (reload), `$E001`
/// is `$C000` (latch), `$C001` is `$E001` (enable). Pattern 1 moves the
/// mirroring register to `$8001` (`$A000`).
#[test]
fn m215_scramble_moves_irq_and_mirroring_registers() {
    let mut m = board(M215_0, 32, 256);
    m.cpu_write(0x5007, 3);
    m.cpu_write(0xE001, 1); // latch = 1
    m.cpu_write(0x8000, 0); // reload
    m.cpu_write(0xC001, 0); // enable
    clock_a12(&mut m);
    clock_a12(&mut m);
    assert!(
        m.irq_pending(),
        "pattern 3: latch 1 fires on the second clock"
    );

    let mut m = board(M215_0, 32, 256);
    m.cpu_write(0x5007, 1);
    m.cpu_write(0x8001, 1);
    assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
}

/// The page gives no IRQ revision; the MMC3 default (Sharp) applies, so a
/// latch of 0 fires on every clock.
#[test]
fn m215_irq_is_the_sharp_mmc3() {
    let mut m = board(M215_0, 32, 256);
    m.cpu_write(0xC000, 0);
    m.cpu_write(0xC001, 0);
    m.cpu_write(0xE001, 0);
    clock_a12(&mut m);
    m.cpu_write(0xE000, 0);
    m.cpu_write(0xE001, 0);
    clock_a12(&mut m);
    assert!(m.irq_pending(), "Sharp: a latch of 0 fires every clock");
}

/// `$5001` powers up as `$xF` (the last outer bank: the reset vector is
/// reachable), a console reset restores it, and `$5000` / `$5007` keep their
/// values across the reset.
#[test]
fn m215_outer_bank_power_up_and_reset() {
    let mut m = board(M215_0, 128, 1024);
    assert_eq!(prg_at(&mut m, 0xE000), 127, "power-up: last 256 KiB");
    m.cpu_write(0x5001, 0x00);
    m.cpu_write(0x5000, 0x80);
    m.cpu_write(0x5007, 0x04);
    assert_eq!(prg_at(&mut m, 0xE000), 1, "outer 0, NROM bank 0");
    m.reset();
    assert_eq!(m.regs[1], 0x0F, "$5001 back to its power-up value");
    assert_eq!(m.regs[0], 0x80, "$5000 kept");
    assert_eq!(m.regs[2], 0x04, "$5007 kept");
}

/// `$5000` is 114's NROM override (mask `$F007`, so `$5FF8` reaches it), and
/// bit 6 makes PRG A17 / CHR A17 come from `$5001` bits 4 / 5.
#[test]
fn m215_nrom_override_and_128k_mode() {
    let mut m = board(M215_0, 128, 1024);
    m.cpu_write(0x5001, 0x00);
    m.cpu_write(0x5FF8, 0xA0 | 0x05); // NROM-256, bank 5 -> 4/5
    assert_eq!(prg_at(&mut m, 0x8000), 8);
    assert_eq!(prg_at(&mut m, 0xC000), 10);
    m.cpu_write(0x5000, 0x00);
    m.cpu_write(0x8000, 6);
    m.cpu_write(0x8001, 0x13); // MMC3 PRG A17 set
    m.cpu_write(0x8000, 2);
    m.cpu_write(0x8001, 0x85); // MMC3 CHR A17 set
    assert_eq!(prg_at(&mut m, 0x8000), 0x13, "256 KiB mode: MMC3 A17");
    assert_eq!(chr_at(&mut m, 0x1000), 0x85);
    m.cpu_write(0x5000, 0x40); // 128 KiB mode, $5001 bits 4/5 clear
    assert_eq!(prg_at(&mut m, 0x8000), 0x03, "A17 from $5001 bit 4 (0)");
    assert_eq!(chr_at(&mut m, 0x1000), 0x05, "A17 from $5001 bit 5 (0)");
    m.cpu_write(0x5001, 0x30);
    assert_eq!(prg_at(&mut m, 0x8000), 0x13, "A17 from $5001 bit 4 (1)");
    assert_eq!(chr_at(&mut m, 0x1000), 0x85, "A17 from $5001 bit 5 (1)");
}

/// The outer bank layouts: submapper 0 `..cp CCPP`, submapper 1 `..cp P.PP`
/// over `CCC.` (PRG A20 = bit 3, CHR A18-A20 = bits 1-3).
#[test]
fn m215_outer_bank_layouts() {
    // 2 MiB PRG (256 x 8 KiB), 2 MiB CHR (2048 x 1 KiB).
    let mut m = board(M215_0, 256, 2048);
    m.cpu_write(0x8000, 6);
    m.cpu_write(0x8001, 0x01);
    m.cpu_write(0x8000, 2);
    m.cpu_write(0x8001, 0x01);
    m.cpu_write(0x5001, 0x0E); // PRG A19 (bit 1), CHR A18-A19 = 3
    assert_eq!(prg_at(&mut m, 0x8000), 0x01 | (2 << 5));
    assert_eq!(chr_at(&mut m, 0x1000), 0x01 | (3 << 8));
    m.cpu_write(0x5001, 0x08);
    assert_eq!(prg_at(&mut m, 0x8000), 0x01, "sub 0: bit 3 is CHR only");

    let mut m = board(M215_1, 256, 2048);
    m.cpu_write(0x8000, 6);
    m.cpu_write(0x8001, 0x01);
    m.cpu_write(0x8000, 2);
    m.cpu_write(0x8001, 0x01);
    m.cpu_write(0x5001, 0x08); // PRG A20, CHR A20
    assert_eq!(prg_at(&mut m, 0x8000), 0x01 | (4 << 5));
    assert_eq!(chr_at(&mut m, 0x1000), 0x01 | (4 << 8));
    m.cpu_write(0x5001, 0x03); // PRG A18-A19, CHR A18
    assert_eq!(prg_at(&mut m, 0x8000), 0x01 | (3 << 5));
    assert_eq!(chr_at(&mut m, 0x1000), 0x01 | (1 << 8));
}

#[test]
fn m215_state_round_trip_and_variant_refusal() {
    let mut m = board(M215_0, 64, 512);
    m.cpu_write(0x5000, 0x45);
    m.cpu_write(0x5001, 0x21);
    m.cpu_write(0x5007, 0x03);
    let blob = m.save_state();
    let mut n = board(M215_0, 64, 512);
    n.load_state(&blob).expect("same variant");
    assert_eq!(n.save_state(), blob);
    assert!(board(M215_1, 64, 512).load_state(&blob).is_err());
    assert!(board(M114_0, 64, 512).load_state(&blob).is_err());
}

// ---------------------------------------------------------------------------
// Mapper 197 (`INES_Mapper_197.md`).
// ---------------------------------------------------------------------------

#[test]
fn m197_parses() {
    for sub in [0u8, 1, 2, 3] {
        assert!(
            crate::parse(&nes2_image(197, sub, 16, 64)).is_ok(),
            "mapper 197.{sub} must construct"
        );
    }
}

/// Load R0-R5 with `0x11, 0x21, 0x30, 0x40, 0x50, 0x60` (R0/R1 odd, so the
/// page's `AND $FE` / `OR $01` are visible) and R6 with 3.
fn m197_load_regs(m: &mut Mmc3BoardExt, mode: u8) {
    for (r, v) in [
        (0u8, 0x11u8),
        (1, 0x21),
        (2, 0x30),
        (3, 0x40),
        (4, 0x50),
        (5, 0x60),
        (6, 3),
    ] {
        m.cpu_write(0x8000, mode | r);
        m.cpu_write(0x8001, v);
    }
}

/// The page's "Real PPU Addr -> MMC3 reg" table for each submapper, row by
/// row. A row's MMC3 register value lands on CHR-ROM A11-A18 and PPU A10 on
/// A10, so the 1 KiB ROM bank is `value * 2 + A10`.
#[test]
fn m197_chr_wiring_follows_the_page_tables() {
    let (r0, r1, r2, r3, r4, r5) = (0x11usize, 0x21, 0x30, 0x40, 0x50, 0x60);
    let tables: [(u8, [usize; 4]); 3] = [
        // sub 0: $0000/$0400 R0&$FE, $0800/$0C00 R0|1, $1000/$1400 R2, $1800/$1C00 R3.
        (0, [r0 & 0xFE, r0 | 1, r2, r3]),
        // sub 1: R1&$FE, R1|1, R4, R5.
        (1, [r1 & 0xFE, r1 | 1, r4, r5]),
        // sub 2: R0&$FE, R1|1, R2, R5.
        (2, [r0 & 0xFE, r1 | 1, r2, r5]),
    ];
    for (sub, regs) in tables {
        let mut m = board(ExtBoard::M197 { sub }, 16, 512);
        m197_load_regs(&mut m, 0);
        for (i, &v) in regs.iter().enumerate() {
            let base = (i as u16) * 0x800;
            assert_eq!(chr_at(&mut m, base), v * 2, "sub {sub} at {base:#06x}");
            assert_eq!(
                chr_at(&mut m, base + 0x400),
                v * 2 + 1,
                "sub {sub} at {:#06x}",
                base + 0x400
            );
        }
    }
}

/// `$8000` bit 7 is not on the page, but the MMC3 still has it and PA12 is
/// PPU A12, so it swaps which registers each pattern table sees: inverted,
/// submapper 0's `$0000` reads R2 and `$1000` reads R0.
#[test]
fn m197_chr_inversion_still_swaps_pattern_tables() {
    let mut m = board(ExtBoard::M197 { sub: 0 }, 16, 512);
    m197_load_regs(&mut m, 0x80);
    assert_eq!(chr_at(&mut m, 0x0000), 0x30 * 2);
    assert_eq!(chr_at(&mut m, 0x0800), 0x40 * 2);
    assert_eq!(chr_at(&mut m, 0x1000), 0x10 * 2);
    assert_eq!(chr_at(&mut m, 0x1800), 0x11 * 2);
}

/// "PRG-ROM, nametable mirroring and IRQs function as in mapper 4": R6 at
/// `$8000`, `$A000` mirroring, and the Sharp IRQ (a latch of 0 fires every
/// clock).
#[test]
fn m197_prg_mirroring_and_irq_are_mapper_4() {
    let mut m = board(ExtBoard::M197 { sub: 0 }, 16, 512);
    m197_load_regs(&mut m, 0);
    assert_eq!(prg_at(&mut m, 0x8000), 3);
    assert_eq!(prg_at(&mut m, 0xE000), 15);
    m.cpu_write(0xA000, 1);
    assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    m.cpu_write(0xC000, 0);
    m.cpu_write(0xC001, 0);
    m.cpu_write(0xE001, 0);
    clock_a12(&mut m);
    m.cpu_write(0xE000, 0);
    m.cpu_write(0xE001, 0);
    clock_a12(&mut m);
    assert!(m.irq_pending());
}

/// Submapper 3: `$6000-$7FFF` `.... S..P` on the WRAM interface. With S set
/// PRG A17 = P; with S clear it is the MMC3's. A write while `$A001` forbids
/// it is dropped. Submapper 0 has no such register.
#[test]
fn m197_sub3_outer_prg_register() {
    let mut m = board(ExtBoard::M197 { sub: 3 }, 32, 512);
    m.cpu_write(0x8000, 6);
    m.cpu_write(0x8001, 0x13);
    m.cpu_write(0xA001, 0x00); // WRAM disabled
    m.cpu_write(0x6000, 0x10);
    assert_eq!(prg_at(&mut m, 0x8000), 0x13, "dropped while disabled");
    m.cpu_write(0xA001, 0xC0); // enabled, write-protected
    m.cpu_write(0x7FFF, 0x10);
    assert_eq!(prg_at(&mut m, 0x8000), 0x13, "dropped while protected");
    m.cpu_write(0xA001, 0x80);
    m.cpu_write(0x7FFF, 0x10);
    assert_eq!(prg_at(&mut m, 0x8000), 0x03, "S=1, P=0");
    m.cpu_write(0x6000, 0x11);
    m.cpu_write(0x8001, 0x03);
    assert_eq!(prg_at(&mut m, 0x8000), 0x13, "S=1, P=1");
    m.cpu_write(0x6000, 0x01);
    assert_eq!(prg_at(&mut m, 0x8000), 0x03, "S=0: the MMC3's A17");

    let mut m = board(ExtBoard::M197 { sub: 0 }, 32, 512);
    m.cpu_write(0x8000, 6);
    m.cpu_write(0x8001, 0x03);
    m.cpu_write(0xA001, 0x80);
    m.cpu_write(0x6000, 0x11);
    assert_eq!(prg_at(&mut m, 0x8000), 0x03, "sub 0 has no outer register");
}

#[test]
fn m197_state_round_trip_and_variant_refusal() {
    let mut m = board(ExtBoard::M197 { sub: 3 }, 32, 512);
    m.cpu_write(0xA001, 0x80);
    m.cpu_write(0x6000, 0x11);
    m197_load_regs(&mut m, 0);
    let blob = m.save_state();
    let mut n = board(ExtBoard::M197 { sub: 3 }, 32, 512);
    n.load_state(&blob).expect("same variant");
    assert_eq!(n.save_state(), blob);
    assert!(
        board(ExtBoard::M197 { sub: 0 }, 32, 512)
            .load_state(&blob)
            .is_err()
    );
    // An unknown submapper is wired as submapper 0.
    let m = board(ExtBoard::M197 { sub: 9 }, 32, 512);
    assert_eq!(m.board, ExtBoard::M197 { sub: 0 });
}

// ---------------------------------------------------------------------------
// Mapper 165 (`INES_Mapper_165.md`, Disch's notes).
// ---------------------------------------------------------------------------

#[test]
fn m165_parses() {
    assert!(
        crate::parse(&nes2_image(165, 0, 32, 16)).is_ok(),
        "mapper 165 must construct"
    );
}

/// Load R0 = `$08`, R1 = `$0C`, R2 = `$10`, R4 = `$14` (4 KiB pages 2, 3, 4,
/// 5 at a page = R >> 2) and R6 = 3.
fn m165_load_regs(m: &mut Mmc3BoardExt) {
    for (r, v) in [(0u8, 0x08u8), (1, 0x0C), (2, 0x10), (4, 0x14), (6, 3)] {
        m.cpu_write(0x8000, r);
        m.cpu_write(0x8001, v);
    }
}

/// "`$0000`: R0 or R1, `$1000`: R2 or R4", picked by the MMC2 latches; a
/// page is the register over 4, so 1 KiB bank = page * 4 + slot. The latch
/// switches after the read that hits the trigger (`MMC2.md`), so that read
/// still comes from the old page.
#[test]
fn m165_latches_select_the_mmc2_style_pages() {
    let mut m = board(ExtBoard::M165, 32, 128);
    m165_load_regs(&mut m);
    // Power-on latches are $FD: R0 and R2.
    assert_eq!(chr_at(&mut m, 0x0000), 8);
    assert_eq!(chr_at(&mut m, 0x0C00), 11);
    assert_eq!(chr_at(&mut m, 0x1000), 16);
    // $0FE8 sets latch 0 to $FE: the read itself is from R0's page.
    assert_eq!(m.ppu_read(0x0FE8), m.chr[11 * CHR_BANK_1K + 0x3E8]);
    assert_eq!(chr_at(&mut m, 0x0000), 12, "R1 after $0FE8");
    // $0FD8 sets it back.
    m.ppu_read(0x0FD8);
    assert_eq!(chr_at(&mut m, 0x0000), 8, "R0 after $0FD8");
    // Latch 1 responds to the whole $1FE8-$1FEF / $1FD8-$1FDF ranges.
    m.ppu_read(0x1FEC);
    assert_eq!(chr_at(&mut m, 0x1000), 20, "R4 after $1FEC");
    assert_eq!(chr_at(&mut m, 0x0000), 8, "latch 0 untouched");
    m.ppu_read(0x1FDF);
    assert_eq!(chr_at(&mut m, 0x1000), 16, "R2 after $1FDF");
}

/// Latch 0 "only responds to one address" (`MMC2.md`): `$0FE9` and `$0FE0`
/// do not switch it.
#[test]
fn m165_latch_0_triggers_on_exact_addresses_only() {
    let mut m = board(ExtBoard::M165, 32, 128);
    m165_load_regs(&mut m);
    m.ppu_read(0x0FE9);
    m.ppu_read(0x0FE0);
    assert_eq!(chr_at(&mut m, 0x0000), 8);
    m.ppu_read(0x0FE8);
    assert_eq!(chr_at(&mut m, 0x0000), 12);
}

/// "CHR page 0 is CHR-RAM": one 4 KiB page, the same RAM in either window;
/// a ROM page ignores writes.
#[test]
fn m165_page_0_is_chr_ram() {
    let mut m = board(ExtBoard::M165, 32, 128);
    m165_load_regs(&mut m);
    m.ppu_write(0x0123, 0x5A);
    assert_ne!(m.ppu_read(0x0123), 0x5A, "page 2 is ROM");
    m.cpu_write(0x8000, 0);
    m.cpu_write(0x8001, 0x03); // R0 = 3: page 0
    m.ppu_write(0x0123, 0x5A);
    assert_eq!(m.ppu_read(0x0123), 0x5A);
    m.cpu_write(0x8000, 2);
    m.cpu_write(0x8001, 0x00); // R2 = 0: page 0 at $1000 too
    assert_eq!(m.ppu_read(0x1123), 0x5A, "the same 4 KiB page");
    assert_eq!(m.chr_phys(0x1123), None, "RAM has no ROM offset");
}

/// The page ignores `$8000` bit 7 (CHR inversion): the notes' diagram has
/// R0/R1 at `$0000` and R2/R4 at `$1000` unconditionally.
#[test]
fn m165_chr_inversion_has_no_effect() {
    let mut m = board(ExtBoard::M165, 32, 128);
    m165_load_regs(&mut m);
    m.cpu_write(0x8000, 0x80);
    assert_eq!(chr_at(&mut m, 0x0000), 8);
    assert_eq!(chr_at(&mut m, 0x1000), 16);
}

/// PRG, mirroring, SRAM enable and IRQ are the MMC3's; the board has 8 KiB
/// of work RAM without being told.
#[test]
fn m165_prg_wram_mirroring_and_irq_are_mmc3() {
    let mut m = board(ExtBoard::M165, 32, 128);
    m165_load_regs(&mut m);
    assert_eq!(prg_at(&mut m, 0x8000), 3);
    assert_eq!(prg_at(&mut m, 0xE000), 31);
    m.cpu_write(0xA000, 1);
    assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    assert_eq!(m.sram().len(), 0x2000);
    m.cpu_write(0xA001, 0x80);
    m.cpu_write(0x7FFF, 0x77);
    assert_eq!(m.cpu_read(0x7FFF), 0x77);
    m.cpu_write(0xA001, 0x00);
    assert!(m.cpu_read_unmapped(0x7FFF), "SRAM disabled by $A001");
    m.cpu_write(0xC000, 1);
    m.cpu_write(0xC001, 0);
    m.cpu_write(0xE001, 0);
    clock_a12(&mut m);
    clock_a12(&mut m);
    assert!(m.irq_pending());
}

/// A CHR read can switch banks, so the board says so, and the latches and
/// the CHR-RAM page are in the save state.
#[test]
fn m165_impure_reads_and_state_round_trip() {
    let mut m = board(ExtBoard::M165, 32, 128);
    assert!(!m.chr_reads_are_pure());
    assert!(board(M114_0, 32, 128).chr_reads_are_pure());
    m165_load_regs(&mut m);
    m.ppu_read(0x0FE8);
    m.ppu_read(0x1FE8);
    m.cpu_write(0x8000, 1);
    m.cpu_write(0x8001, 0x00); // R1 = page 0 (RAM), selected by latch 0
    m.ppu_write(0x0042, 0x99);
    let blob = m.save_state();
    let mut n = board(ExtBoard::M165, 32, 128);
    n.load_state(&blob).expect("same board");
    assert_eq!(n.save_state(), blob);
    assert_eq!(
        n.ppu_read(0x0042),
        0x99,
        "latch 0 = $FE and the RAM restored"
    );
    assert!(
        board(ExtBoard::M197 { sub: 0 }, 32, 128)
            .load_state(&blob)
            .is_err()
    );
}

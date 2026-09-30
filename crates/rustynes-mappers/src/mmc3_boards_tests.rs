// SPDX-License-Identifier: GPL-3.0-or-later
//! Register-decode tests for `mmc3_boards.rs`, one group per board, each
//! written from the board's NESdev page (named on the group).
//!
//! The ROM images are synthetic: every 8 KiB PRG bank and every 1 KiB CHR
//! bank starts with its own bank number (low byte, then high byte), so a
//! read at the start of a window says which bank the board selected.

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

fn board(kind: Board, prg_banks: usize, chr_banks: usize) -> Mmc3Board {
    Mmc3Board::new(
        kind,
        prg_image(prg_banks),
        chr_image(chr_banks),
        Mirroring::Vertical,
        0,
        0,
    )
    .expect("valid sizes")
}

fn prg_at(m: &mut Mmc3Board, addr: u16) -> usize {
    m.cpu_read(addr) as usize | (m.cpu_read(addr + 1) as usize) << 8
}

fn chr_at(m: &mut Mmc3Board, addr: u16) -> usize {
    m.ppu_read(addr) as usize | (m.ppu_read(addr + 1) as usize) << 8
}

/// Write MMC3 bank register `r` (R0-R7) with `$8000` mode bits clear.
fn mmc3_reg(m: &mut Mmc3Board, r: u8, v: u8) {
    m.cpu_write(0x8000, r);
    m.cpu_write(0x8001, v);
}

/// One filtered A12 rise (the MMC3 needs three M2 cycles of A12 low).
fn clock_a12(m: &mut Mmc3Board) {
    m.notify_a12(false);
    for _ in 0..4 {
        m.notify_cpu_cycle();
    }
    m.notify_a12(true);
}

// ---------------------------------------------------------------------------
// Mapper 37 (`INES_Mapper_037.md`): the outer latch in the PRG-RAM window.
// ---------------------------------------------------------------------------

/// The page's table, row by row, for an inner bank with MMC3 PRG A16 set
/// (R6 = `$0F`) and clear (R6 = `$03`): PRG A16 = Q0·Q1 + Q2·M16, A17 = Q2.
#[test]
fn m37_outer_latch_follows_the_page_table() {
    let mut m = board(Board::M37, 32, 256);
    m.cpu_write(0xA001, 0x80); // PRG-RAM enabled, writable.
    for (value, want_m16, want_no_m16) in [
        (0u8, 7, 3),
        (1, 7, 3),
        (2, 7, 3),
        (3, 15, 11),
        (4, 31, 19),
        (5, 31, 19),
        (6, 31, 19),
        (7, 31, 27),
    ] {
        m.cpu_write(0x6000, value);
        mmc3_reg(&mut m, 6, 0x0F);
        assert_eq!(prg_at(&mut m, 0x8000), want_m16, "latch {value}, M16=1");
        mmc3_reg(&mut m, 6, 0x03);
        assert_eq!(prg_at(&mut m, 0x8000), want_no_m16, "latch {value}, M16=0");
        // CHR A17 = Q2: the second 128 KiB for latch values 4-7.
        mmc3_reg(&mut m, 2, 0x05);
        let want_chr = 0x05 | if value >= 4 { 0x80 } else { 0 };
        assert_eq!(chr_at(&mut m, 0x1000), want_chr, "latch {value} CHR");
    }
}

/// "you will need to enable writes to PRG-RAM to update it": a write with
/// `$A001` disabled or write-protected does not reach the latch.
#[test]
fn m37_latch_obeys_the_mmc3_prg_ram_bits() {
    let mut m = board(Board::M37, 32, 256);
    mmc3_reg(&mut m, 6, 0x03);
    m.cpu_write(0xA001, 0x00);
    m.cpu_write(0x6000, 0x04);
    assert_eq!(prg_at(&mut m, 0x8000), 3, "disabled: the write is dropped");
    m.cpu_write(0xA001, 0xC0);
    m.cpu_write(0x6000, 0x04);
    assert_eq!(prg_at(&mut m, 0x8000), 3, "write-protected: dropped");
    m.cpu_write(0xA001, 0x80);
    m.cpu_write(0x6000, 0x04);
    assert_eq!(prg_at(&mut m, 0x8000), 19, "enabled: taken");
}

/// The latch is not readable (open bus), and the CIC reset clears it.
#[test]
fn m37_latch_is_write_only_and_cleared_by_reset() {
    let mut m = board(Board::M37, 32, 256);
    assert!(m.cpu_read_unmapped(0x6000));
    m.cpu_write(0xA001, 0x80);
    m.cpu_write(0x6000, 0x07);
    mmc3_reg(&mut m, 6, 0x03);
    assert_eq!(prg_at(&mut m, 0x8000), 27);
    m.reset();
    assert_eq!(prg_at(&mut m, 0x8000), 3, "reset returns to the menu block");
}

// ---------------------------------------------------------------------------
// Mapper 47 (`INES_Mapper_047.md`): 128 KiB PRG + 128 KiB CHR blocks.
// ---------------------------------------------------------------------------

#[test]
fn m47_block_bit_selects_prg_and_chr_block() {
    let mut m = board(Board::M47, 32, 256);
    mmc3_reg(&mut m, 6, 0x13);
    mmc3_reg(&mut m, 2, 0x85);
    assert_eq!(prg_at(&mut m, 0x8000), 0x03, "inner PRG masked to 128 KiB");
    assert_eq!(chr_at(&mut m, 0x1000), 0x05, "inner CHR masked to 128 KiB");
    assert_eq!(prg_at(&mut m, 0xE000), 0x0F, "fixed bank of block 0");
    m.cpu_write(0xA001, 0x80);
    m.cpu_write(0x7FFF, 0x01);
    assert_eq!(prg_at(&mut m, 0x8000), 0x13);
    assert_eq!(chr_at(&mut m, 0x1000), 0x85);
    assert_eq!(prg_at(&mut m, 0xE000), 0x1F, "fixed bank of block 1");
    m.cpu_write(0xA001, 0xC0);
    m.cpu_write(0x6000, 0x00);
    assert_eq!(prg_at(&mut m, 0x8000), 0x13, "write-protected: dropped");
}

// ---------------------------------------------------------------------------
// Mapper 45 (`INES_Mapper_045.md`): GA23C.
// ---------------------------------------------------------------------------

fn m45_outer(m: &mut Mmc3Board, regs: [u8; 4]) {
    for r in regs {
        m.cpu_write(0x6000, r);
    }
}

#[test]
fn m45_prg_and_or_follow_the_page() {
    // 4 MiB of PRG, so PRG A19-A22 are all observable.
    let mut m = board(Board::M45, 512, 8);
    // PRG-OR $10, PRG-AND inverted $30 (a 128 KiB inner window).
    m45_outer(&mut m, [0x00, 0x10, 0x0F, 0x30]);
    mmc3_reg(&mut m, 6, 0x23);
    assert_eq!(prg_at(&mut m, 0x8000), 0x13, "(R6 & $0F) | $10");
    assert_eq!(
        prg_at(&mut m, 0xE000),
        0x1F,
        "the fixed bank inside the window"
    );
    // Register 1 bits 6-7 are PRG A19-A20; register 2 bits 6-7 A21-A22.
    m.cpu_write(0x6001, 0);
    m45_outer(&mut m, [0x00, 0x90, 0x4F, 0x30]);
    assert_eq!(prg_at(&mut m, 0x8000), 0x100 | 0x80 | 0x13);
}

#[test]
fn m45_chr_and_or_follow_the_page() {
    let mut m = board(Board::M45, 16, 1024);
    // CHR-AND $E = 128 KiB (7 MMC3 bits), CHR-OR $00, CHR A18-A19 = 2.
    m45_outer(&mut m, [0x00, 0x00, 0x2E, 0x00]);
    mmc3_reg(&mut m, 2, 0xC5);
    assert_eq!(chr_at(&mut m, 0x1000), 0x200 | 0x45);
    // CHR-AND $7 and below take no MMC3 bits at all: the CHR-OR alone.
    m.cpu_write(0x6001, 0);
    m45_outer(&mut m, [0x33, 0x00, 0x07, 0x00]);
    assert_eq!(chr_at(&mut m, 0x1000), 0x33);
    assert_eq!(chr_at(&mut m, 0x0000), 0x33);
}

#[test]
fn m45_lock_holds_until_6001() {
    let mut m = board(Board::M45, 64, 8);
    mmc3_reg(&mut m, 6, 0x00);
    m45_outer(&mut m, [0x00, 0x08, 0x0F, 0x40 | 0x38]);
    assert_eq!(prg_at(&mut m, 0x8000), 0x08);
    // Locked: a whole new set of writes is ignored.
    m45_outer(&mut m, [0x00, 0x20, 0x0F, 0x00]);
    assert_eq!(
        prg_at(&mut m, 0x8000),
        0x08,
        "locked registers do not change"
    );
    // `$6001` releases the lock and restarts at register 0.
    m.cpu_write(0x6001, 0x00);
    m45_outer(&mut m, [0x00, 0x20, 0x0F, 0x00]);
    assert_eq!(prg_at(&mut m, 0x8000), 0x20);
}

#[test]
fn m45_soft_reset_clears_the_outer_registers() {
    let mut m = board(Board::M45, 64, 8);
    mmc3_reg(&mut m, 6, 0x00);
    m45_outer(&mut m, [0x00, 0x08, 0x0F, 0x40]);
    m.reset();
    assert_eq!(prg_at(&mut m, 0x8000), 0x00);
    // The write index restarted too: the next write is register 0.
    m45_outer(&mut m, [0x00, 0x10, 0x0F, 0x00]);
    assert_eq!(prg_at(&mut m, 0x8000), 0x10);
}

#[test]
fn m45_dip_switch_reads_on_d0() {
    let mut m = board(Board::M45, 16, 8);
    assert!(!m.cpu_read_unmapped(0x5010));
    assert_eq!(m.cpu_read_driven_mask(0x5010), 0x01);
    assert_eq!(m.cpu_read(0x5010) & 1, 1, "switch 0: $5010 reads 1");
    assert_eq!(m.cpu_read(0x5020) & 1, 0, "switch 0: $5020 reads 0");
    m.set_dip(7);
    assert_eq!(m.cpu_read(0x5800) & 1, 1, "switch 7: $5800 reads 1");
    assert_eq!(m.cpu_read(0x5400) & 1, 0);
}

// ---------------------------------------------------------------------------
// Mapper 12 (`INES_Mapper_012.md`): SL-5020B.
// ---------------------------------------------------------------------------

#[test]
fn m12_chr_a18_is_chosen_by_ppu_a12() {
    let mut m = board(Board::M12, 16, 512);
    mmc3_reg(&mut m, 0, 0x04);
    mmc3_reg(&mut m, 2, 0x09);
    m.cpu_write(0x4100, 0x10); // A18=0 for $0000, A18=1 for $1000.
    assert_eq!(chr_at(&mut m, 0x0000), 0x04);
    assert_eq!(chr_at(&mut m, 0x1000), 0x109);
    m.cpu_write(0x4100, 0x01);
    assert_eq!(chr_at(&mut m, 0x0000), 0x104);
    assert_eq!(chr_at(&mut m, 0x1000), 0x09);
    // Outside the ASIC: MMC3 CHR mode (`$8000` bit 7) does not affect it.
    m.cpu_write(0x8000, 0x80);
    assert_eq!(chr_at(&mut m, 0x0000) >> 8, 1, "$0000 still uses the A bit");
}

#[test]
fn m12_register_decodes_on_mask_e100() {
    let mut m = board(Board::M12, 16, 512);
    mmc3_reg(&mut m, 0, 0x04);
    m.cpu_write(0x5FFF, 0x01); // $5FFF & $E100 = $4100.
    assert_eq!(chr_at(&mut m, 0x0000), 0x104);
    m.cpu_write(0x4200, 0x00); // $4200 & $E100 = $4000: not the register.
    assert_eq!(chr_at(&mut m, 0x0000), 0x104);
    assert!(!m.cpu_read_unmapped(0x4100));
    assert!(m.cpu_read_unmapped(0x4200));
}

/// The MMC3A's alternate IRQ: with the latch at 0 it never fires, where the
/// Sharp MMC3 fires every clock (`MMC3.md`, "IRQ Specifics").
#[test]
fn m12_uses_the_mmc3a_alternate_irq() {
    for (kind, want) in [(Board::M12, false), (Board::M37, true)] {
        let mut m = board(kind, 16, 512);
        m.cpu_write(0xC000, 0);
        m.cpu_write(0xE001, 0);
        clock_a12(&mut m);
        clock_a12(&mut m);
        assert_eq!(m.irq_pending(), want, "{kind:?}");
    }
}

// ---------------------------------------------------------------------------
// Mappers 74 / 191 / 192 / 194: fixed CHR-RAM overlays.
// ---------------------------------------------------------------------------

/// Put `bank` in R2 (1 KiB at PPU `$1000`), write a marker there and read it
/// back. Returns `(read_after_write, rom_marker_before)`.
fn overlay_probe(kind: Board, bank: u8) -> (u8, usize) {
    let mut m = board(kind, 16, 256);
    mmc3_reg(&mut m, 2, bank);
    let before = chr_at(&mut m, 0x1000);
    m.ppu_write(0x1002, 0xA5);
    (m.ppu_read(0x1002), before)
}

#[test]
fn waixing_overlays_map_the_documented_banks_to_ram() {
    for (kind, ram_banks, rom_banks) in [
        (Board::M74, &[8u8, 9][..], &[7u8, 10][..]),
        (Board::M192, &[8, 9, 10, 11], &[7, 12]),
        (Board::M194, &[0, 1], &[2, 3]),
        (Board::M191, &[0x80, 0x81], &[0x00, 0x01]),
    ] {
        for &b in ram_banks {
            let (read, _) = overlay_probe(kind, b);
            assert_eq!(read, 0xA5, "{kind:?} bank {b:#x} is RAM");
        }
        for &b in rom_banks {
            let (read, before) = overlay_probe(kind, b);
            assert_eq!(read, 0, "{kind:?} bank {b:#x} is ROM: the write is dropped");
            assert_eq!(before, b as usize, "{kind:?} bank {b:#x} reads CHR-ROM");
        }
    }
}

/// 74 / 192 are "effectively the same" with 2 vs 4 KiB of RAM: bank 8 and
/// bank 10 are the same RAM on 74 (2 KiB) and different RAM on 192 (4 KiB).
#[test]
fn m74_and_m192_differ_only_in_ram_size() {
    let mut m74 = board(Board::M74, 16, 256);
    mmc3_reg(&mut m74, 2, 8);
    m74.ppu_write(0x1000, 0x11);
    mmc3_reg(&mut m74, 2, 9);
    m74.ppu_write(0x1000, 0x22);
    mmc3_reg(&mut m74, 2, 8);
    assert_eq!(m74.ppu_read(0x1000), 0x11);
    let mut m192 = board(Board::M192, 16, 256);
    mmc3_reg(&mut m192, 2, 8);
    m192.ppu_write(0x1000, 0x11);
    mmc3_reg(&mut m192, 2, 10);
    m192.ppu_write(0x1000, 0x33);
    mmc3_reg(&mut m192, 2, 8);
    assert_eq!(
        m192.ppu_read(0x1000),
        0x11,
        "bank 10 is distinct RAM on 192"
    );
}

#[test]
fn waixing_boards_carry_mmc3_work_ram() {
    let mut m = board(Board::M74, 16, 256);
    assert_eq!(m.sram().len(), WRAM_8K);
    m.cpu_write(0x6123, 0x5A);
    assert_eq!(m.cpu_read(0x6123), 0x5A);
    m.cpu_write(0xA001, 0x00);
    assert!(m.cpu_read_unmapped(0x6123), "disabled RAM floats");
}

// ---------------------------------------------------------------------------
// Mapper 195 (`INES_Mapper_195.md`): PPU-write-selected CHR-RAM.
// ---------------------------------------------------------------------------

fn m195_is_ram(m: &mut Mmc3Board, bank: u8) -> bool {
    mmc3_reg(m, 2, bank);
    let before = m.ppu_read(0x1003);
    m.ppu_write(0x1003, before ^ 0xFF);
    let after = m.ppu_read(0x1003);
    m.ppu_write(0x1003, before);
    after == before ^ 0xFF
}

#[test]
fn m195_power_on_selection_is_28_to_2b() {
    let mut m = board(Board::M195, 16, 256);
    for b in 0x28..=0x2B {
        assert!(m195_is_ram(&mut m, b), "bank {b:#x}");
    }
    assert!(!m195_is_ram(&mut m, 0x27));
    assert!(!m195_is_ram(&mut m, 0x2C));
}

#[test]
fn m195_a_write_to_a_rom_bank_selects_the_ram_window() {
    let mut m = board(Board::M195, 16, 256);
    for (select, ram, rom) in [
        (0x82u8, 0x00u8, 0x28u8),
        (0x88, 0x4C, 0x00),
        (0x8A, 0x67, 0x4C),
        (0xC0, 0x46, 0x48),
        (0xC2, 0x7D, 0x46),
        (0xC8, 0x0A, 0x7C),
    ] {
        mmc3_reg(&mut m, 3, select);
        m.ppu_write(0x1400, 0); // A write to the ROM bank `select`.
        assert!(
            m195_is_ram(&mut m, ram),
            "after {select:#x}: {ram:#x} is RAM"
        );
        assert!(
            !m195_is_ram(&mut m, rom),
            "after {select:#x}: {rom:#x} is ROM"
        );
    }
    // `$CA`, and anything with bit 4 set: CHR-ROM only.
    for select in [0xCAu8, 0x90] {
        mmc3_reg(&mut m, 3, select);
        m.ppu_write(0x1400, 0);
        for b in [0x00u8, 0x28, 0x46, 0x4C] {
            assert!(!m195_is_ram(&mut m, b), "after {select:#x}: {b:#x} is ROM");
        }
    }
}

/// "CHR A13 is not connected": `$80` and `$82` select the same 4 KiB.
#[test]
fn m195_80_and_82_share_the_same_ram() {
    let mut m = board(Board::M195, 16, 256);
    mmc3_reg(&mut m, 2, 0x28);
    m.ppu_write(0x1000, 0x77);
    mmc3_reg(&mut m, 3, 0x82);
    m.ppu_write(0x1400, 0);
    mmc3_reg(&mut m, 2, 0x00);
    assert_eq!(m.ppu_read(0x1000), 0x77);
}

// ---------------------------------------------------------------------------
// Mapper 121 (`INES_Mapper_121.md`): Kasheng A9711 / A9713.
// ---------------------------------------------------------------------------

#[test]
fn m121_protection_array_is_indexed_by_the_last_5000_write() {
    let mut m = board(Board::M121, 32, 256);
    for (i, want) in [0x83u8, 0x83, 0x42, 0x00].into_iter().enumerate() {
        m.cpu_write(0x5000, i as u8);
        assert_eq!(m.cpu_read(0x5000), want, "index {i}");
    }
    assert!(!m.cpu_read_unmapped(0x5ABC));
}

#[test]
fn m121_8003_index_applies_the_bit_reversed_latch() {
    let mut m = board(Board::M121, 32, 256);
    mmc3_reg(&mut m, 7, 0x01);
    // Latch $01 -> reversed low six bits = $20; bank $20 & $1F = 0 on 256 KiB.
    // Use $02 -> $10 instead.
    m.cpu_write(0x8001, 0x02);
    m.cpu_write(0x8003, 0x28);
    assert_eq!(prg_at(&mut m, 0xC000), 0x10, "index $28: $C000 override");
    // Sticky: a later $8001 updates the same slot.
    m.cpu_write(0x8001, 0x04); // reversed: $08
    assert_eq!(prg_at(&mut m, 0xC000), 0x08);
    // $2C with a zero value leaves $E000 alone.
    m.cpu_write(0x8001, 0x00);
    m.cpu_write(0x8003, 0x2C);
    assert_eq!(prg_at(&mut m, 0xE000), 0x1F);
    // An invalid index restores normal MMC3 banking.
    m.cpu_write(0x8003, 0x00);
    assert_eq!(
        prg_at(&mut m, 0xC000),
        0x1E,
        "back to the MMC3's fixed bank"
    );
}

#[test]
fn m121_a9713_outer_bank_at_5180() {
    let mut m = board(Board::M121, 64, 512);
    mmc3_reg(&mut m, 6, 0x03);
    mmc3_reg(&mut m, 2, 0x05);
    m.cpu_write(0x5180, 0x80);
    assert_eq!(prg_at(&mut m, 0x8000), 0x23);
    assert_eq!(chr_at(&mut m, 0x1000), 0x105);
    m.cpu_write(0x5100, 0x80); // $5100 & $F180 = $5100: not the register.
    assert_eq!(prg_at(&mut m, 0x8000), 0x23);
}

#[test]
fn m121_a9711_chr_a18_follows_ppu_a12() {
    let mut m = board(Board::M121, 32, 512);
    mmc3_reg(&mut m, 0, 0x04);
    mmc3_reg(&mut m, 2, 0x09);
    // Mode 0 (power-on): CHR A18 = inverted PPU A12.
    assert_eq!(chr_at(&mut m, 0x0000), 0x104);
    assert_eq!(chr_at(&mut m, 0x1000), 0x009);
    // Mode 1: CHR A18 = PPU A12. The same `$8000` write also sets the
    // MMC3's CHR mode, so R0 now sits at `$1000` and R2 at `$0000`.
    m.cpu_write(0x8000, 0x80);
    assert_eq!(chr_at(&mut m, 0x1000), 0x104);
    assert_eq!(chr_at(&mut m, 0x0000), 0x009);
}

#[test]
fn reverse_low_six_matches_the_page() {
    assert_eq!(reverse_low_six(0x01), 0x20);
    assert_eq!(reverse_low_six(0x03), 0x30);
    assert_eq!(reverse_low_six(0xC1), 0xE0, "the top two bits pass through");
}

// ---------------------------------------------------------------------------
// Save states.
// ---------------------------------------------------------------------------

#[test]
fn every_board_round_trips_its_state() {
    for kind in [
        Board::M12,
        Board::M37,
        Board::M45,
        Board::M47,
        Board::M74,
        Board::M121,
        Board::M191,
        Board::M192,
        Board::M194,
        Board::M195,
        Board::M4T9552,
        Board::M249,
    ] {
        let mut a = board(kind, 64, 512);
        a.cpu_write(0xA001, 0x80);
        a.cpu_write(0x6000, 0x05);
        a.cpu_write(0x4100, 0x11);
        a.cpu_write(0x5180, 0x80);
        a.cpu_write(0x8001, 0x02);
        a.cpu_write(0x8003, 0x28);
        mmc3_reg(&mut a, 2, 0x81);
        a.ppu_write(0x1000, 0x5C);
        mmc3_reg(&mut a, 6, 0x05);
        let blob = a.save_state();
        let mut b = board(kind, 64, 512);
        b.load_state(&blob)
            .unwrap_or_else(|e| panic!("{kind:?}: {e:?}"));
        for addr in [0x8000u16, 0xA000, 0xC000, 0xE000] {
            assert_eq!(
                prg_at(&mut a, addr),
                prg_at(&mut b, addr),
                "{kind:?} {addr:#x}"
            );
        }
        for addr in (0..0x2000u16).step_by(0x400) {
            assert_eq!(
                chr_at(&mut a, addr),
                chr_at(&mut b, addr),
                "{kind:?} {addr:#x}"
            );
        }
        assert_eq!(b.save_state(), blob, "{kind:?}: re-save is identical");
        // A blob for another board is refused, not misread.
        let other = if kind == Board::M37 {
            Board::M47
        } else {
            Board::M37
        };
        assert!(board(other, 64, 512).load_state(&blob).is_err(), "{kind:?}");
    }
}

// ---------------------------------------------------------------------------
// T9552 (`T9552.md`): mapper 4 submapper 5 and mapper 249.
// ---------------------------------------------------------------------------

/// The page's worked example: mapper 249, `$5000 = $02`, PRG bank `$02`
/// (PRG A14) selected, "PRG A14 becomes PRG A16, so PRG Bank `$08` in the ROM
/// file it is".
#[test]
fn t9552_the_pages_worked_example() {
    let mut m = board(Board::M249, 32, 256);
    m.cpu_write(0x5000, 0x02);
    mmc3_reg(&mut m, 6, 0x02);
    assert_eq!(prg_at(&mut m, 0x8000), 0x08);
}

/// Submapper 5 files are in the `$5000 = $02` order: that setting is the
/// identity, and every game writes it at once.
#[test]
fn t9552_submapper5_is_identity_at_02() {
    let mut m = board(Board::M4T9552, 32, 256);
    m.cpu_write(0x5000, 0x02);
    for bank in [0x00u8, 0x03, 0x05, 0x0A, 0x1E] {
        mmc3_reg(&mut m, 6, bank);
        assert_eq!(prg_at(&mut m, 0x8000), bank as usize, "PRG {bank:#x}");
    }
    for bank in [0x00u8, 0x04, 0x31, 0xC5] {
        mmc3_reg(&mut m, 2, bank);
        assert_eq!(chr_at(&mut m, 0x1000), bank as usize, "CHR {bank:#x}");
    }
}

/// At power-on (`$5000 = 0`) a submapper 5 board is scrambled: MMC3 A14
/// sits in column 0's last row, and column 2's last row is A17.
#[test]
fn t9552_power_on_pattern_scrambles() {
    let mut m = board(Board::M4T9552, 32, 256);
    mmc3_reg(&mut m, 6, 0x02); // PRG A14
    assert_eq!(prg_at(&mut m, 0x8000), 0x10, "A14 -> A17");
    mmc3_reg(&mut m, 6, 0x01); // A13 is not scrambled
    assert_eq!(prg_at(&mut m, 0x8000), 0x01);
    // The fixed banks (all ones) are the same under every pattern.
    assert_eq!(prg_at(&mut m, 0xE000), 0x1F);
    // CHR, pattern 0: MMC3 A12 (1 KiB bank bit 2) sits in row 1; column 2's
    // row 1 is A13 (bank bit 3).
    mmc3_reg(&mut m, 2, 0x04);
    assert_eq!(chr_at(&mut m, 0x1000), 0x08);
}

#[test]
fn t9552_register_decodes_on_mask_f000() {
    let mut m = board(Board::M249, 32, 256);
    m.cpu_write(0x5FFF, 0x02);
    mmc3_reg(&mut m, 6, 0x02);
    assert_eq!(prg_at(&mut m, 0x8000), 0x08);
    m.cpu_write(0x4FFF, 0x00); // not the register
    assert_eq!(prg_at(&mut m, 0x8000), 0x08);
}

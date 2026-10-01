// SPDX-License-Identifier: GPL-3.0-or-later
//! Mapper 218 ("Magic Floor"): the iNES header selects the CIRAM A10 source.
//!
//! The board has no CHR chip; the console's 2 KiB `CIRAM` serves as both the
//! pattern tables and the nametables, and the header's flags-6 bits 0 and 3
//! choose which PPU address line drives CIRAM A10 (the nesdev page `INES_Mapper_218`):
//!
//! | flags 6 | CIRAM A10 | effect |
//! | --- | --- | --- |
//! | `$A1` | PPU A10 | 2 KiB CHR-RAM shared with two nametables |
//! | `$A0` | PPU A11 | 2 KiB CHR-RAM shared with two nametables |
//! | `$A8` | PPU A12 | 1 KiB per pattern table, one screen (bank 0) |
//! | `$A9` | PPU A13 | 1 KiB CHR-RAM (bank 0), one screen (bank 1) |
//!
//! The generic parser folds bit 0 away once bit 3 is set (`FourScreen`), so
//! the two single-screen wirings can only be told apart from the raw byte.
//! Before v2.9.8 both fell back to the A10 wiring, so *Magic Floor* (`$A9`)
//! drew its nametable over its own tiles.

/// A 16 KiB-PRG, CHR-RAM mapper-218 iNES image with flags 6 = `flags6`.
fn image(flags6: u8) -> Vec<u8> {
    let mut v = vec![0u8; 16 + 0x4000];
    v[0..4].copy_from_slice(b"NES\x1A");
    v[4] = 1;
    v[5] = 0;
    v[6] = flags6;
    v[7] = 0xD0;
    v
}

#[test]
fn four_screen_and_vertical_bits_select_ppu_a13() {
    let (_cart, mut m) = rustynes_mappers::parse(&image(0xA9)).unwrap();
    m.ppu_write(0x0000, 0x11);
    m.ppu_write(0x2000, 0x33);
    // Pattern space is all CIRAM bank 0; the nametable is bank 1.
    assert_eq!(
        m.ppu_read(0x0400),
        0x11,
        "$0400 must alias $0000 (A10 unused)"
    );
    assert_eq!(
        m.ppu_read(0x1000),
        0x11,
        "$1000 must alias $0000 (A12 unused)"
    );
    assert_eq!(
        m.ppu_read(0x0000),
        0x11,
        "the nametable write must not land in CHR"
    );
    assert_eq!(m.ppu_read(0x2400), 0x33);
}

#[test]
fn four_screen_without_vertical_bit_selects_ppu_a12() {
    let (_cart, mut m) = rustynes_mappers::parse(&image(0xA8)).unwrap();
    m.ppu_write(0x0000, 0x11);
    m.ppu_write(0x1000, 0x22);
    assert_eq!(m.ppu_read(0x0400), 0x11);
    assert_eq!(m.ppu_read(0x1400), 0x22);
    assert_eq!(
        m.ppu_read(0x2400),
        0x11,
        "nametables share bank 0 with pattern table 0"
    );
}

#[test]
fn two_screen_headers_keep_a10_and_a11() {
    let (_c, mut v) = rustynes_mappers::parse(&image(0xA1)).unwrap();
    v.ppu_write(0x0400, 0x44);
    assert_eq!(v.ppu_read(0x2400), 0x44, "$A1: CIRAM A10 = PPU A10");
    let (_c, mut h) = rustynes_mappers::parse(&image(0xA0)).unwrap();
    h.ppu_write(0x0800, 0x55);
    assert_eq!(h.ppu_read(0x2800), 0x55, "$A0: CIRAM A10 = PPU A11");
}

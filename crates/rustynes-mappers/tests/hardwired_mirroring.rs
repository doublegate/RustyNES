// SPDX-License-Identifier: GPL-3.0-or-later
//! Which boards may take a per-game mirroring correction.
//!
//! `Mapper::has_hardwired_mirroring` gates `Nes::set_mirroring_override`: the
//! per-game database may correct a header's mirroring bit only on a board
//! whose mirroring is fixed by solder pads. Forcing a static layout onto a
//! board that switches mirroring itself corrupts it (the `AxROM` / Wizards &
//! Warriors case in `mapper.rs`), so the default is `false`, and a board is
//! listed here only when its nesdev wiki page says the mirroring is fixed.
//!
//! Core audit section 5.6 found this gate `false` on several fixed-mirroring
//! boards, so a wrong header on those could never be corrected. v2.7.2 set it
//! from the wiki, board by board:
//!
//! | mapper | board | wiki evidence (`nesdev_wiki/`) |
//! | --- | --- | --- |
//! | 11 | Color Dreams | `Color_Dreams`: "Fixed H or V, controlled by solder pads" |
//! | 13 | CPROM | `CPROM`: "Nametable mirroring: Vertical" |
//! | 34 | BNROM / NINA-001 | `INES_Mapper_034`: "Fixed V" |
//! | 38 | Bit Corp. | `INES_Mapper_038`: "V (hardwired)" |
//! | 70 | Bandai 74*161 | `INES_Mapper_070`: the 1-screen variant is mapper 152 |
//! | 79 | NINA-03 / NINA-06 | `NINA_003_006`: "Fixed H or V, controlled by solder pads" |
//! | 87 | Jaleco | `INES_Mapper_087`: "Fixed H or V, controlled by solder pads" |
//! | 94 | UN1ROM | `INES_Mapper_094`: "Solder pads select vertical or horizontal mirroring" |
//! | 180 | Nichibutsu UNROM (7408) | `INES_Mapper_180`: "Fixed H or V, controlled by solder pads" |
//! | 184 | Sunsoft-1 | `INES_Mapper_184`: no mirroring control documented |
//!
//! Kept `false`, with the reason:
//! - **71** (Camerica): the model decodes the Fire Hawk single-screen bit on
//!   every cart, so it controls its own mirroring.
//! - **152**: a mirroring register.
//! - **70 with a four-screen header**: the pre-152 encoding of the
//!   single-screen variant ("alternative nametables" bit), so the board is
//!   not known to be fixed.
//! - **86, 101, 140**: the vendored wiki pages state no mirroring, so they
//!   keep the safe default until one does.

/// An iNES 1.0 image for `mapper` with `prg16` x 16 KiB PRG and `chr8` x
/// 8 KiB CHR (0 = CHR-RAM), vertical mirroring unless `four_screen`.
fn image(mapper: u8, prg16: u8, chr8: u8, four_screen: bool) -> Vec<u8> {
    let mut h = [0u8; 16];
    h[0..4].copy_from_slice(b"NES\x1A");
    h[4] = prg16;
    h[5] = chr8;
    h[6] = ((mapper & 0x0F) << 4) | 0x01 | if four_screen { 0x08 } else { 0 };
    h[7] = mapper & 0xF0;
    let mut v = h.to_vec();
    v.resize(
        16 + usize::from(prg16) * 0x4000 + usize::from(chr8) * 0x2000,
        0,
    );
    v
}

/// Boards constrain their ROM sizes (CPROM wants 32 KiB PRG and CHR-RAM,
/// `AxROM` 8 KiB of CHR), so try common layouts until one parses.
fn hardwired(mapper: u8, four_screen: bool) -> bool {
    for (prg16, chr8) in [(8, 4), (2, 0), (8, 0), (8, 1), (2, 1), (4, 2), (16, 16)] {
        if let Ok((_c, m)) = rustynes_mappers::parse(&image(mapper, prg16, chr8, four_screen)) {
            return m.has_hardwired_mirroring();
        }
    }
    panic!("mapper {mapper}: no test layout parses");
}

#[test]
fn wiki_fixed_mirroring_boards_accept_a_header_correction() {
    let wrong: Vec<u8> = [11u8, 13, 34, 38, 70, 79, 87, 94, 180, 184]
        .into_iter()
        .filter(|&n| !hardwired(n, false))
        .collect();
    assert!(
        wrong.is_empty(),
        "fixed-mirroring boards still refusing a correction: {wrong:?}"
    );
}

#[test]
fn boards_that_switch_mirroring_refuse_it() {
    for n in [1u8, 4, 7, 71, 152] {
        assert!(
            !hardwired(n, false),
            "mapper {n} switches its own mirroring"
        );
    }
    assert!(
        !hardwired(70, true),
        "a four-screen mapper-70 header is the 152 variant"
    );
}

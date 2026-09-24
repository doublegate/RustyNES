// SPDX-License-Identifier: GPL-3.0-or-later
//! Boards whose nesdev wiki page documents `$6000-$7FFF` RAM must have it.
//!
//! v2.7.2 made a board with nothing at `$6000-$7FFF` read open bus there
//! instead of a made-up `$00` (core audit §5.5). Comparing the commercial-ROM
//! suite on `main` and on that change exposed five boards whose MODEL had no
//! RAM there while the hardware does. For them `$00` and open bus are equally
//! wrong -- writes vanished either way -- and the fix is the RAM:
//!
//! | mapper | wiki (`nesdev_wiki/INES_Mapper_NNN.xhtml`) | save |
//! | --- | --- | --- |
//! | 156 | "CPU $6000-$7FFF: 8 KiB RAM" | if the header says battery |
//! | 177 | "8 KiB of battery-backed WRAM at CPU $6000-$7FFF" | always |
//! | 227 | the FW-01 variant "adds 8 KiB battery-backed WRAM" | present only with a battery header |
//! | 241 | "8 KiB of WRAM at CPU $6000-$7FFF that can be battery-backed" | if the header says battery |
//! | 245 | "an MMC3 clone with 8 KiB of battery-backed WRAM" | always |

// Byte patterns: keeping the low byte is the intent.
#![allow(clippy::cast_possible_truncation)]

use rustynes_mappers::parse;

/// iNES 1.0 image, 256 KiB PRG, with or without the battery bit, and either
/// CHR-RAM or 128 KiB of CHR-ROM (mapper 156 requires ROM).
fn image(mapper: u8, battery: bool, chr_rom: bool) -> Vec<u8> {
    let mut h = [0u8; 16];
    h[0..4].copy_from_slice(b"NES\x1A");
    h[4] = 16;
    h[5] = if chr_rom { 16 } else { 0 };
    h[6] = ((mapper & 0x0F) << 4) | if battery { 0x02 } else { 0 };
    h[7] = mapper & 0xF0;
    let mut v = h.to_vec();
    v.extend((0..16usize * 0x4000).map(|i| (i as u8) | 0x80));
    if chr_rom {
        v.extend((0..16usize * 0x2000).map(|i| i as u8));
    }
    v
}

fn build(mapper: u8, battery: bool) -> Box<dyn rustynes_mappers::Mapper> {
    parse(&image(mapper, battery, false))
        .or_else(|_| parse(&image(mapper, battery, true)))
        .map_or_else(|e| panic!("mapper {mapper}: {e:?}"), |(_c, m)| m)
}

fn holds_writes(m: &mut Box<dyn rustynes_mappers::Mapper>) -> bool {
    for a in 0x6000u16..=0x7FFF {
        m.cpu_write(a, (a as u8) ^ 0x3C);
    }
    (0x6000u16..=0x7FFF).all(|a| !m.cpu_read_unmapped(a) && m.cpu_read(a) == (a as u8) ^ 0x3C)
}

#[test]
fn documented_wram_holds_writes_and_is_saved_per_the_wiki() {
    // (mapper, RAM without a battery header?, saved without a battery header?)
    for (n, ram_always, saved_always) in [
        (156u8, true, false),
        (177, true, true),
        (227, false, false),
        (241, true, false),
        (245, true, true),
    ] {
        let mut m = build(n, true);
        assert!(
            holds_writes(&mut m),
            "mapper {n}: battery header -> RAM at $6000"
        );
        assert_eq!(
            m.sram().len(),
            0x2000,
            "mapper {n}: battery header -> 8 KiB save"
        );

        let mut m = build(n, false);
        assert_eq!(
            holds_writes(&mut m),
            ram_always,
            "mapper {n}: RAM without a battery header"
        );
        assert_eq!(
            m.sram().len(),
            if saved_always { 0x2000 } else { 0 },
            "mapper {n}: save without a battery header"
        );
    }
}

#[test]
fn a_save_state_from_before_the_ram_still_loads() {
    for n in [156u8, 177, 227, 241, 245] {
        let mut m = build(n, true);
        m.cpu_write(0x6000, 0x5A);
        let blob = m.save_state();
        let mut fresh = build(n, true);
        fresh
            .load_state(&blob)
            .unwrap_or_else(|e| panic!("{n}: current blob: {e:?}"));
        assert_eq!(fresh.cpu_read(0x6000), 0x5A, "mapper {n}: RAM round-trips");

        // The same blob without its trailing 8 KiB, stamped with the previous
        // version where the board versions its blob (227 shares a file-wide
        // version and is told apart by length).
        let mut old = blob[..blob.len() - 0x2000].to_vec();
        if n != 227 {
            old[0] -= 1;
        }
        let mut o = build(n, true);
        o.load_state(&old)
            .unwrap_or_else(|e| panic!("{n}: pre-v2.7.2 blob: {e:?}"));
        assert_eq!(
            o.cpu_read(0x6000),
            0,
            "mapper {n}: an old blob loads with the RAM zeroed"
        );
    }
}

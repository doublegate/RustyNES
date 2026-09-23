// SPDX-License-Identifier: GPL-3.0-or-later
//! A board with nothing at `$6000-$7FFF` must let the bus float there.
//!
//! The CPU bus keeps an open-bus latch: an access nothing drives returns the
//! last value on the data bus (`nesdev_wiki/Open_bus_behavior.xhtml`). A
//! mapper tells the bus that through `Mapper::cpu_read_unmapped`. The trait's
//! default treats all of `$6000-$FFFF` as mapped, so a board with no RAM there
//! that did not override the hook read back a made-up `$00` instead of open bus.
//! Core audit section 5.5 named four such boards; this test does not trust the
//! list.
//!
//! It builds every NES 2.0 mapper number with **no** PRG-RAM and no battery,
//! then looks at the window. A board whose every `$6000-$7FFF` read returns
//! `$00` while it still reports the window mapped is inventing a byte, unless
//! it is listed in [`DRIVES_ZERO`] with the reason.

// Every `as u8` builds a byte pattern where keeping the low byte is the intent.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use rustynes_mappers::parse;

/// Boards that genuinely drive `$00` somewhere in `$6000-$7FFF` with no RAM,
/// each with its reason. Empty unless a board's documentation says so.
const DRIVES_ZERO: &[(u16, &str)] = &[(
    142,
    "Kaiser KS7032: the model reads a zero work RAM at $6000 unless register 4 \
     selects ROM, while nesdev_wiki/INES_Mapper_142 gives that window an 8 KiB \
     switchable PRG-ROM bank. A modelling divergence, not open bus: core \
     ledger F-09, left for a release with a game to check it against",
)];

fn image(mapper: u16, chr_rom: bool) -> Vec<u8> {
    let chr_units: u8 = if chr_rom { 16 } else { 0 };
    let mut h = [0u8; 16];
    h[0..4].copy_from_slice(b"NES\x1A");
    h[4] = 16;
    h[5] = chr_units;
    h[6] = (((mapper & 0x0F) as u8) << 4) | 0x01;
    h[7] = ((mapper & 0xF0) as u8) | 0b0000_1000;
    h[8] = ((mapper >> 8) & 0x0F) as u8;
    h[10] = 0; // no PRG-RAM, no PRG-NVRAM
    if !chr_rom {
        h[11] = 7; // 8 KiB CHR-RAM
    }
    let mut v = h.to_vec();
    v.extend((0..16usize * 0x4000).map(|i| (i as u8) | 0x80));
    v.extend((0..usize::from(chr_units) * 0x2000).map(|i| i as u8));
    v
}

#[test]
fn a_window_reported_unmapped_has_nothing_behind_it() {
    // The converse, which guards the v2.7.2 default. The default floats the
    // window exactly when `sram()` is empty, so a board with no exposed save
    // RAM that still drives data there (ROM, readable registers, RAM that
    // `sram()` fails to expose) and does not override the hook would read open
    // bus where the game expects its data. Boards WITH save RAM are not the
    // default's business: they either keep it mapped or override the hook on
    // purpose (FME-7 floats a selected-but-disabled RAM window, and its
    // `cpu_read` returns a discarded ROM byte there, which the trait allows).
    let mut offenders = Vec::new();
    for (mapper, chr_rom) in (0u16..4096).flat_map(|n| [(n, true), (n, false)]) {
        let Ok((_cart, mut m)) = parse(&image(mapper, chr_rom)) else {
            continue;
        };
        if !m.sram().is_empty() {
            continue;
        }
        for a in 0x6000u16..=0x7FFF {
            m.cpu_write(a, 0xA5);
        }
        let mut data = 0usize;
        for a in 0x6000u16..=0x7FFF {
            if m.cpu_read_unmapped(a) && m.cpu_read(a) != 0 {
                data += 1;
            }
        }
        if data > 0 {
            offenders.push(format!(
                "{mapper} (chr_rom={chr_rom}): {data} bytes of data"
            ));
        }
    }
    offenders.dedup();
    assert!(
        offenders.is_empty(),
        "{} board(s) report $6000-$7FFF unmapped while driving data there:\n  {}",
        offenders.len(),
        offenders.join("\n  ")
    );
}

#[test]
fn an_empty_prg_ram_window_is_open_bus_not_zero() {
    let mut constructed = 0usize;
    let mut offenders = Vec::new();
    for (mapper, chr_rom) in (0u16..4096).flat_map(|n| [(n, true), (n, false)]) {
        let Ok((_cart, mut m)) = parse(&image(mapper, chr_rom)) else {
            continue;
        };
        constructed += 1;
        if DRIVES_ZERO.iter().any(|(n, _)| *n == mapper) {
            continue;
        }
        // A board with save RAM that reads $00 is usually reading real RAM
        // behind a write protect (MMC5's protect pair blocks writes, not
        // reads), which is not open bus. Only a board with NO RAM is asked.
        if !m.sram().is_empty() {
            continue;
        }
        let claims_mapped = (0x6000u16..=0x7FFF).any(|a| !m.cpu_read_unmapped(a));
        if !claims_mapped {
            continue;
        }
        // Write a non-zero marker first: RAM (even RAM the header did not
        // ask for) then reads back non-zero, so an all-zero window really is
        // nothing driving the bus.
        for a in 0x6000u16..=0x7FFF {
            m.cpu_write(a, 0xA5);
        }
        let mut all_zero = true;
        for a in 0x6000u16..=0x7FFF {
            if !m.cpu_read_unmapped(a) && m.cpu_read(a) != 0 {
                all_zero = false;
                break;
            }
        }
        if all_zero {
            offenders.push(format!(
                "{mapper} {} (chr_rom={chr_rom})",
                m.debug_info().name
            ));
        }
    }
    offenders.dedup();
    assert!(constructed > 200, "only {constructed} images built");
    assert!(
        offenders.is_empty(),
        "{} board(s) read $00 at $6000-$7FFF with no RAM instead of reporting it unmapped:\n  {}",
        offenders.len(),
        offenders.join("\n  ")
    );
}

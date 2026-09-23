// SPDX-License-Identifier: GPL-3.0-or-later
//! Every battery-backed board must expose its save memory through `sram()`.
//!
//! A host reads a cartridge's battery save through `Nes::sram()` and restores
//! it through `sram_mut()`; today the host that does is the libretro core,
//! which hands the slice to `RetroArch` as `RETRO_MEMORY_SAVE_RAM` (the `.srm`
//! file). The trait's default returns an EMPTY slice, so a mapper that holds
//! save RAM but does not override the pair loses the player's save on every
//! exit, with no error anywhere: the game works, the save "succeeds", and the
//! file is empty. The core audit found five such boards (IMP-08/09/10, section
//! 5.1e); this test found a sixth.
//!
//! This test does not trust a list of boards. It builds every NES 2.0 mapper
//! number (0-4095) from an NES 2.0 image with the battery bit set and 8 KiB
//! of PRG-NVRAM declared, writes a marker through the CPU bus across
//! `$6000-$7FFF`, and requires that marker to appear in `sram()`. A board that
//! genuinely has no CPU-visible save RAM there -- its save memory is behind a
//! serial protocol, or it has none -- must be listed in [`NO_CPU_WINDOW`] with
//! the reason, so a NEW mapper that forgets `sram()` fails here by default.

// Every `as u8` here builds a byte PATTERN from an address or index -- ROM
// fill, marker values -- where keeping only the low byte is the intent.
#![allow(clippy::cast_possible_truncation)]

use rustynes_mappers::parse;

/// PRG-ROM and CHR-ROM sizes large enough for every board's bank arithmetic.
const PRG_16K_UNITS: u8 = 16; // 256 KiB
const CHR_8K_UNITS: u8 = 16; // 128 KiB

/// An NES 2.0 image: `mapper` (12-bit), battery bit set, 8 KiB PRG-NVRAM, and
/// either `CHR_8K_UNITS` of CHR-ROM or none (CHR-RAM). Both are tried, because
/// several boards refuse one of the two -- Multicart 15 rejects any CHR-ROM --
/// and a board the parser refuses is a board this test silently never checks.
fn image(mapper: u16, chr_rom: bool) -> Vec<u8> {
    let chr_units = if chr_rom { CHR_8K_UNITS } else { 0 };
    let mut h = [0u8; 16];
    h[0..4].copy_from_slice(b"NES\x1A");
    h[4] = PRG_16K_UNITS;
    h[5] = chr_units;
    // Byte 6: mapper low nibble, battery (bit 1). Byte 7: mapper mid nibble,
    // NES 2.0 identifier (bits 2-3 = 0b10). Byte 8: mapper high nibble.
    set_mapper(&mut h, mapper);
    // Byte 10: PRG-RAM (low nibble) and PRG-NVRAM (high nibble) as 64 << n.
    // 64 << 7 = 8 KiB of battery-backed RAM.
    h[10] = 7 << 4;
    let prg = usize::from(PRG_16K_UNITS) * 0x4000;
    let chr = usize::from(chr_units) * 0x2000;
    let mut v = h.to_vec();
    // Distinct, non-zero ROM bytes so no marker can be mistaken for ROM.
    v.extend((0..prg).map(|i| (i as u8) | 0x80));
    v.extend((0..chr).map(|i| i as u8));
    v
}

/// Write a 12-bit mapper number into an NES 2.0 header (bytes 6-8), keeping the
/// battery and NES 2.0 identifier bits. The sweep patches one template image
/// per CHR variant with this instead of rebuilding 384 KiB for every number.
fn set_mapper(h: &mut [u8], mapper: u16) {
    h[6] = (((mapper & 0x0F) as u8) << 4) | 0b0000_0010;
    h[7] = ((mapper & 0xF0) as u8) | 0b0000_1000;
    h[8] = ((mapper >> 8) & 0x0F) as u8;
}

/// Boards to skip in the generic loop, each with the reason. Empty today: the
/// two boards the loop cannot reach -- Bandai FCG's EEPROM behind I2C and Taito
/// X1-005's `$A3`-gated RAM -- are unreachable because their writes never land,
/// which the loop already treats as "nothing to check", and each has its own
/// test below.
const NO_CPU_WINDOW: &[(u16, &str)] = &[];

/// How many images must reach writable RAM for the loop to count as having
/// checked anything. Measured when the range became 0..4096 (v2.7.1): 296
/// images built, 43 reaching RAM.
///
/// The other 146 mapper numbers build but show this loop no writable RAM:
/// many have none, and the rest gate it behind a board-specific enable (MMC5's
/// protect registers, FME-7's RAM-enable bit) that a blind write sweep never
/// unlocks. For those the loop checks NOTHING, and the eprintln in the test
/// names them. They were cross-checked statically instead when this was
/// written: every mapper source holding RAM overrides `sram()` except Kaiser
/// (FDS conversions, no battery), mapper 42 (ROM at `$6000`), mapper 30 (saves
/// by self-flashing) and the Vs. `DualSystem` shared RAM. Board-specific tests for
/// the gated boards belong with v2.7.2's mapper-RAM work. Lower this floor only
/// with a reason.
const CHECKED_FLOOR: usize = 43;

#[test]
fn every_battery_board_exposes_its_save_memory() {
    let mut constructed = 0usize;
    let mut checked = 0usize;
    let mut unreached = Vec::new();
    let mut failures = Vec::new();
    // NES 2.0 mapper numbers are 12-bit, so this is the whole space: 0..4096.
    // It stopped at 512 until review on #548 pointed at mapper 513, which the
    // parser dispatches and the loop therefore never built.
    let mut templates = [image(0, true), image(0, false)];
    for (mapper, chr_rom) in (0u16..4096).flat_map(|n| [(n, true), (n, false)]) {
        let img = &mut templates[usize::from(!chr_rom)];
        set_mapper(img, mapper);
        let Ok((cart, mut m)) = parse(img) else {
            continue;
        };
        constructed += 1;
        if NO_CPU_WINDOW.iter().any(|(n, _)| *n == mapper) {
            continue;
        }
        let _ = cart;
        // Write a distinct marker at every address of the window; many boards
        // gate RAM behind an enable register, so a board that ignores the
        // writes is not by itself a failure -- only a board whose writes land
        // somewhere readable while `sram()` stays empty or unchanged is.
        let before = m.sram().to_vec();
        let marker = |addr: u16| 0x5A ^ (addr as u8) ^ ((addr >> 8) as u8).rotate_left(3);
        for addr in 0x6000u16..=0x7FFF {
            m.cpu_write(addr, marker(addr));
        }
        // Read back only after ALL writes, and require at least 64 surviving
        // markers. A single matching read proves nothing: a board that models
        // open bus returns the address's high byte, which equals a marker by
        // coincidence about once per 256 addresses (~32 hits). Real RAM, even
        // Taito X1-005's 128 bytes, returns at least 128.
        let surviving = (0x6000u16..=0x7FFF)
            .filter(|&a| m.cpu_read(a) == marker(a))
            .count();
        if surviving < 64 {
            // No writable RAM reachable without board-specific setup. Recorded,
            // not failed: it is a hole in what this loop can see, printed below
            // so it is visible, and bounded by the `checked` floor.
            unreached.push(mapper);
            continue;
        }
        checked += 1;
        let after = m.sram();
        if after.is_empty() || after == before.as_slice() {
            failures.push(format!(
                "mapper {mapper} (chr_rom={chr_rom}): {surviving} bytes of $6000-$7FFF hold writes, but sram() is {}",
                if after.is_empty() { "EMPTY" } else { "unchanged by them" }
            ));
            continue;
        }
        // The restore direction: a loaded save goes in through `sram_mut()`,
        // and the game must then read it. The slice-to-address mapping is
        // board-specific (banking), so fill ALL of it with one value and
        // require the window to show that value at least as often as the
        // write sweep survived -- a `sram_mut()` that returns a detached copy
        // fails this even though `sram()` passed above.
        m.sram_mut().fill(0xC3);
        let restored = (0x6000u16..=0x7FFF)
            .filter(|&a| m.cpu_read(a) == 0xC3)
            .count();
        if restored < 64 {
            failures.push(format!(
                "mapper {mapper} (chr_rom={chr_rom}): sram() reflects CPU writes, but a value \
                 restored through sram_mut() reaches only {restored} addresses of $6000-$7FFF"
            ));
        }
    }
    unreached.dedup();
    eprintln!(
        "battery_sram_exposed: {constructed} images built, {checked} with reachable RAM checked; \
         {} mapper numbers built but with no RAM this loop can reach: {unreached:?}",
        unreached.len()
    );
    assert!(
        constructed > 200,
        "only {constructed} images constructed; the image is wrong"
    );
    assert!(
        checked >= CHECKED_FLOOR,
        "only {checked} images reached writable RAM (floor {CHECKED_FLOOR}); a change to the \
         image or the reachability test has made this loop check less than it did"
    );
    assert!(
        failures.is_empty(),
        "{} board(s) would lose their battery save:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// Taito X1-005 (mapper 80): 128 bytes at `$7F00-$7FFF`, unlocked by writing
/// `$A3` to BOTH `$7EF8` and `$7EF9`. The generic loop cannot reach it without
/// that unlock.
#[test]
fn taito_x1_005_ram_is_the_battery_save() {
    let (_c, mut m) = parse(&image(80, true)).unwrap();
    m.cpu_write(0x7EF8, 0xA3);
    m.cpu_write(0x7EF9, 0xA3);
    for i in 0..128u16 {
        m.cpu_write(0x7F00 + i, 0xC0 ^ i as u8);
    }
    let sram = m.sram();
    assert_eq!(sram.len(), 128, "the whole on-chip RAM is the save");
    assert!(
        (0..128).all(|i| sram[i] == 0xC0 ^ i as u8),
        "writes reach sram()"
    );

    // And a restored save is what the game then reads.
    m.sram_mut()[5] = 0x77;
    assert_eq!(m.cpu_read(0x7F05), 0x77);
}

/// Bandai FCG with a serial EEPROM (mapper 16 / 159): there is no CPU window;
/// the EEPROM is written through an I2C protocol. What matters for the save is
/// that `sram()` IS the EEPROM, in both directions.
#[test]
fn bandai_fcg_eeprom_is_the_battery_save() {
    for mapper in [16u16, 159] {
        let Ok((_c, mut m)) = parse(&image(mapper, true)) else {
            panic!("mapper {mapper} should construct");
        };
        let len = m.sram().len();
        assert!(
            len == 128 || len == 256,
            "mapper {mapper}: EEPROM exposed, got {len} bytes"
        );
        m.sram_mut()[3] = 0x9E;
        assert_eq!(
            m.sram()[3],
            0x9E,
            "mapper {mapper}: a restored save persists in the EEPROM"
        );
    }
}

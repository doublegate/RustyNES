// SPDX-License-Identifier: GPL-3.0-or-later
//! A CHR-RAM byte written at a pattern-table address reads back at that
//! address (`T-VRC4-CHRRAM-BANKING`, v3.3.0).
//!
//! A mapper's CHR bank outputs drive the address lines of whatever chip sits
//! on the CHR bus. When that chip is RAM, a PPU write goes through the same
//! banking as a PPU read, so writing a byte and reading the same address
//! straight back returns it, whatever the bank registers hold. Several boards
//! broke that: they read CHR-RAM through the bank registers and wrote it at
//! `addr % len`, unbanked, so a game that uploads tiles and then draws them
//! read back a different 1 KiB page. VRC2 and VRC4 were found first (#608's
//! review, on mapper 252); this test found the rest by asking every board the
//! same question.
//!
//! The image is NES 2.0 with 8 KiB of CHR-RAM and no CHR-ROM, so every
//! pattern-table address is RAM-backed on a board that takes CHR-RAM at all.
//! The registers are left at power-on, which is enough: the defect needs only
//! a bank register whose value is not the slot's own index, and most boards
//! power on with every slot at bank 0.

// Byte and address patterns: keeping the low bits is the intent.
#![allow(clippy::cast_possible_truncation)]

use std::panic::{self, AssertUnwindSafe};
use std::sync::Mutex;

use rustynes_mappers::parse;

/// Every board that builds from a CHR-RAM image of submapper `sub`.
fn boards(sub: u8) -> Vec<(u16, Box<dyn rustynes_mappers::Mapper>)> {
    let mut out = Vec::new();
    for mapper in 0u16..4096 {
        let img = image(mapper, sub);
        if let Ok(Ok((_cart, m))) = panic::catch_unwind(|| parse(&img)) {
            out.push((mapper, m));
        }
    }
    out
}

/// NES 2.0 image of `mapper` submapper `sub`: 128 KiB of PRG-ROM, no
/// CHR-ROM, 8 KiB of CHR-RAM, horizontal mirroring.
fn image(mapper: u16, sub: u8) -> Vec<u8> {
    let mut h = [0u8; 16];
    h[0..4].copy_from_slice(b"NES\x1A");
    h[4] = 8; // 8 x 16 KiB PRG-ROM
    h[5] = 0; // no CHR-ROM
    h[6] = ((mapper & 0x0F) as u8) << 4;
    h[7] = (mapper & 0xF0) as u8 | 0x08; // NES 2.0
    h[8] = ((mapper >> 8) as u8 & 0x0F) | (sub << 4);
    h[11] = 0x07; // CHR-RAM 64 << 7 = 8 KiB
    let mut v = h.to_vec();
    v.extend((0..8 * 0x4000usize).map(|i| (i ^ (i >> 8)) as u8));
    v
}

/// Why a board's pattern tables are not all ordinary RAM at power-on, so a
/// write is not expected to read straight back at every address, or `None`.
/// Each reason is the board's own documented behaviour.
const fn exempt(mapper: u16, sub: u8) -> Option<&'static str> {
    match (mapper, sub) {
        // FFE: CHR-RAM is write-protected in latch modes 4-7. Mapper 6's
        // submapper is its power-on latch mode (`n & 7`), and mapper 8 is
        // "a synonym of Mapper 6 submapper 4" (`ffe_super_magic_card.rs`,
        // `chr_writable`).
        (6, s) if s & 0x04 != 0 => Some("FFE latch mode 4-7: CHR write-protected"),
        (8, _) => Some("FFE latch mode 4: CHR write-protected"),
        // TQROM: pattern tables are CHR-ROM unless a bank's bit 6 selects the
        // board's CHR-RAM, and every bank selects ROM at power-on.
        (119, _) => Some("TQROM: every bank on CHR-ROM at power-on"),
        _ => None,
    }
}

/// The panic hook is process-wide and the two tests run on parallel
/// threads: each silences it while boards run under `catch_unwind`, so
/// without this one test's report could be swallowed by the other's silent
/// hook.
static HOOK: Mutex<()> = Mutex::new(());

/// Xorshift64: a fixed, dependency-free stimulus sequence.
struct Rng(u64);

impl Rng {
    const fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn a_chr_ram_write_reads_back_at_the_same_address() {
    let _serial = HOOK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));

    // Every submapper: a submapper can select a different board (34 is
    // BNROM at 0 and NINA-001 at 1), and each is its own model.
    let mut built = 0usize;
    let mut failures = Vec::new();
    for (sub, mapper, mut m) in
        (0u8..16).flat_map(|sub| boards(sub).into_iter().map(move |(id, m)| (sub, id, m)))
    {
        if exempt(mapper, sub).is_some() {
            continue;
        }
        built += 1;
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            let mut bad = Vec::new();
            for addr in 0u16..0x2000 {
                let v = (addr.wrapping_mul(7) ^ (addr >> 8)) as u8 ^ 0x5A;
                m.ppu_write(addr, v);
                let got = m.ppu_read(addr);
                if got != v {
                    bad.push(addr);
                }
            }
            bad
        }));
        match result {
            Ok(bad) if bad.is_empty() => {}
            Ok(bad) => failures.push(format!(
                "mapper {mapper}.{sub}: {} of 8192 addresses do not read back (first ${:04X})",
                bad.len(),
                bad[0]
            )),
            Err(_) => failures.push(format!("mapper {mapper}.{sub}: panicked")),
        }
    }
    panic::set_hook(default_hook);

    assert!(built > 150, "only {built} boards took a CHR-RAM image");
    assert!(
        failures.is_empty(),
        "{} of {built} boards:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The same defect behind banks that are not at power-on, where the strict
/// test above cannot reach (VRC6 powers on with each slot at its own index,
/// so an unbanked write lands where a banked read finds it). Random register
/// writes scramble the banks first. A board may now be in a write-protected
/// mode, or have a slot on ROM, so a write that changes nothing is allowed;
/// what is not allowed is a write that changes some pattern byte but not the
/// one written, which is exactly an unbanked write under banked reads.
#[test]
fn a_chr_ram_write_never_lands_somewhere_else() {
    let _serial = HOOK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));

    let mut failures = Vec::new();
    let mut checked = 0usize;
    for (mapper, mut m) in boards(0) {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ u64::from(mapper));
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            let mut bad = Vec::new();
            for _round in 0..3 {
                for _ in 0..48 {
                    let r = rng.next();
                    let addr = 0x4020 + (r % (0x1_0000 - 0x4020)) as u16;
                    m.cpu_write(addr, (r >> 32) as u8);
                }
                for _ in 0..24 {
                    let r = rng.next();
                    let addr = (r % 0x2000) as u16;
                    let before: Vec<u8> = (0..0x2000).map(|a| m.ppu_peek(a)).collect();
                    let v = before[usize::from(addr)] ^ 0xA5;
                    m.ppu_write(addr, v);
                    let changed = (0..0x2000u16).any(|a| m.ppu_peek(a) != before[usize::from(a)]);
                    if changed && m.ppu_peek(addr) != v {
                        bad.push(addr);
                    }
                }
            }
            bad
        }));
        checked += 1;
        match result {
            Ok(bad) if bad.is_empty() => {}
            Ok(bad) => failures.push(format!(
                "mapper {mapper}: {} writes landed elsewhere (first ${:04X})",
                bad.len(),
                bad[0]
            )),
            Err(_) => failures.push(format!("mapper {mapper}: panicked")),
        }
    }
    panic::set_hook(default_hook);

    assert!(checked > 150, "only {checked} boards took a CHR-RAM image");
    assert!(
        failures.is_empty(),
        "{} of {checked} boards:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// SPDX-License-Identifier: GPL-3.0-or-later
//! A multicart's 32 KiB PRG window must not index past an undersized PRG-ROM.
//!
//! v2.9.0 re-audit NC-01. Seven discrete multicart boards (iNES mappers 46,
//! 57, 58, 61, 62, 202 and 212) computed their 32 KiB bank count as
//! `(prg_rom.len() / 32K).max(1)`, reduced the bank modulo that count, and
//! indexed `prg_rom[bank * 32K + off]` with `off` up to `$7FFF`:
//!
//! - with a 16 KiB PRG-ROM the count clamps to 1, bank 0 is used, and any
//!   `off >= $4000` is past the end;
//! - 202 and 212 count 16 KiB banks and keep an even bank for their 32 KiB
//!   mode, so an ODD bank count (48 or 80 KiB) puts the window's second half
//!   past the end as well.
//!
//! The constructors accept those sizes (an undersized dump, a hack, a header
//! typo), so the first write that selects 32 KiB mode followed by a fetch from
//! `$C000-$FFFF` panicked: the emulator thread on native, the tab on wasm32.
//! The fix indexes modulo the ROM length, which is what a smaller part in a
//! larger window does on hardware (the missing address lines leave it
//! mirrored) and is the identity for every image that ran before.
//!
//! The stimulus is the re-audit's `writes` probe, made deterministic: random
//! register writes over `$6000-$FFFF`, each followed by reads spread over the
//! whole PRG window.

// Byte and address patterns: keeping the low bits is the intent.
#![allow(clippy::cast_possible_truncation)]

use std::panic::{self, AssertUnwindSafe};

use rustynes_mappers::parse;

/// iNES 1.0 image: `prg16` x 16 KiB of PRG-ROM and 8 KiB of CHR-ROM, or
/// CHR-RAM for mapper 61 (whose constructor requires it).
fn image(mapper: u8, prg16: u8) -> Vec<u8> {
    let chr_rom = mapper != 61;
    let mut h = [0u8; 16];
    h[0..4].copy_from_slice(b"NES\x1A");
    h[4] = prg16;
    h[5] = u8::from(chr_rom);
    h[6] = (mapper & 0x0F) << 4;
    h[7] = mapper & 0xF0;
    let mut v = h.to_vec();
    v.extend((0..usize::from(prg16) * 0x4000).map(|i| (i ^ (i >> 8)) as u8));
    if chr_rom {
        v.extend((0..0x2000usize).map(|i| i as u8));
    }
    v
}

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
fn multicart_32k_windows_stay_inside_an_undersized_prg_rom() {
    // Keep the default hook quiet while each board runs under `catch_unwind`;
    // the failures are collected and reported together below.
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));

    let mut failures = Vec::new();
    for mapper in [46u8, 57, 58, 61, 62, 202, 212] {
        for prg16 in [1u8, 3, 5] {
            let img = image(mapper, prg16);
            // The finding is about sizes the constructors ACCEPT; a size a
            // board refuses cannot reach a fetch, but it must be reported
            // rather than silently shrinking the matrix.
            let (_cart, mut m) = match parse(&img) {
                Ok(built) => built,
                Err(e) => {
                    failures.push(format!("mapper {mapper}, {prg16} x 16 KiB: refused: {e}"));
                    continue;
                }
            };
            let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (u64::from(mapper) << 8) ^ u64::from(prg16));
            let run = panic::catch_unwind(AssertUnwindSafe(|| {
                // Every write address once (the address-latched boards decode
                // their mode and bank from it), then random writes (the
                // value-latched ones, 46 and 57, decode the data).
                for a in 0x6000..=0xFFFFu16 {
                    m.cpu_write(a, (a >> 8) as u8 ^ a as u8);
                    for r in [0x8000, 0xBFFF, 0xC000, 0xFFFF] {
                        let _ = m.cpu_read(r);
                    }
                }
                for _ in 0..20_000 {
                    let r = rng.next();
                    let addr =
                        0x6000 | (r as u16 & 0x9FFF) | if r & (1 << 20) != 0 { 0x8000 } else { 0 };
                    m.cpu_write(addr, (r >> 32) as u8);
                    for a in [
                        0x8000 | (r >> 40) as u16,
                        0xC000 | ((r >> 24) as u16 & 0x3FFF),
                        0xFFFC,
                        0xBFFF,
                    ] {
                        let _ = m.cpu_read(a);
                    }
                }
            }));
            if run.is_err() {
                failures.push(format!(
                    "mapper {mapper}, {} KiB PRG",
                    u32::from(prg16) * 16
                ));
            }
        }
    }

    panic::set_hook(default_hook);
    assert!(
        failures.is_empty(),
        "a PRG fetch indexed past an undersized PRG-ROM: {failures:?}"
    );
}

/// The image's PRG byte at offset `i`, as [`image`] writes it.
const fn prg_byte(i: usize) -> u8 {
    (i ^ (i >> 8)) as u8
}

/// `CodeRabbit` on #561: the test above proves only that nothing panics, so an
/// in-bounds implementation returning the WRONG bytes would pass it. A 16 KiB
/// part in a 32 KiB window mirrors: every address reads ROM offset
/// `addr & $3FFF`, whatever mode or bank the registers select, because the
/// missing address lines leave nothing else to read. Asserted byte for byte.
#[test]
fn an_undersized_prg_rom_mirrors_in_every_mode() {
    for mapper in [46u8, 57, 58, 61, 62, 202, 212] {
        let (_cart, mut m) = parse(&image(mapper, 1)).expect("16 KiB is accepted");
        let mut rng = Rng(0xD1B5_4A32_D192_ED03 ^ u64::from(mapper));
        for _ in 0..4_000 {
            let r = rng.next();
            let addr = 0x6000 | (r as u16 & 0x9FFF) | if r & (1 << 20) != 0 { 0x8000 } else { 0 };
            m.cpu_write(addr, (r >> 32) as u8);
            let off = (r >> 40) as usize & 0x3FFF;
            for base in [0x8000u16, 0xC000] {
                let got = m.cpu_read(base | off as u16);
                assert_eq!(
                    got,
                    prg_byte(off),
                    "mapper {mapper}: ${:04X} after writing ${addr:04X}",
                    base | off as u16
                );
            }
        }
    }
}

/// NES 2.0 image with an 8 KiB PRG-ROM (exponent-multiplier size form,
/// 2^13 x 1) and 8 KiB of CHR-ROM.
fn image_prg8k(mapper: u16) -> Vec<u8> {
    let mut h = [0u8; 16];
    h[0..4].copy_from_slice(b"NES\x1A");
    h[4] = 13 << 2; // exponent 13, multiplier 1
    h[5] = 1;
    h[6] = ((mapper & 0x0F) as u8) << 4;
    h[7] = (mapper & 0xF0) as u8 | 0x08; // NES 2.0
    h[8] = (mapper >> 8) as u8 & 0x0F;
    h[9] = 0x0F; // PRG size MSB nibble $F: the exponent form
    let mut v = h.to_vec();
    v.extend((0..0x2000usize).map(prg_byte));
    v.extend((0..0x2000usize).map(|i| i as u8));
    v
}

/// `CodeRabbit` on #561 (outside the diff): the discrete boards' 16 KiB
/// helper had the same defect NC-01 fixed in the 32 KiB one. The constructor
/// accepts any multiple of 8 KiB, and an 8 KiB PRG-ROM clamped the 16 KiB bank
/// count to 1 and indexed a 16 KiB window, so a fetch from the window's upper
/// half panicked. Every board on that helper, driven through every write
/// address; the reads must also mirror, as an 8 KiB part does.
#[test]
fn an_8k_prg_rom_mirrors_in_a_16k_window() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let mut failures = Vec::new();
    for mapper in [51u16, 104, 290, 301] {
        let (_cart, mut m) = match parse(&image_prg8k(mapper)) {
            Ok(built) => built,
            Err(e) => {
                failures.push(format!("mapper {mapper}: refused: {e}"));
                continue;
            }
        };
        let run = panic::catch_unwind(AssertUnwindSafe(|| {
            let mut wrong = None;
            for a in 0x6000..=0xFFFFu16 {
                m.cpu_write(a, (a >> 8) as u8 ^ a as u8);
                for r in [0x8000u16, 0xA000, 0xBFFF, 0xC000, 0xE000, 0xFFFF] {
                    let want = prg_byte(usize::from(r) & 0x1FFF);
                    if m.cpu_read(r) != want && wrong.is_none() {
                        wrong = Some((a, r));
                    }
                }
            }
            wrong
        }));
        match run {
            Err(_) => failures.push(format!("mapper {mapper}: panicked")),
            Ok(Some((a, r))) => failures.push(format!(
                "mapper {mapper}: ${r:04X} misread after writing ${a:04X}"
            )),
            Ok(None) => {}
        }
    }
    panic::set_hook(default_hook);
    assert!(failures.is_empty(), "an 8 KiB PRG-ROM: {failures:?}");
}

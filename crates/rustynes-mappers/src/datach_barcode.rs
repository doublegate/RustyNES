// SPDX-License-Identifier: GPL-3.0-or-later
//! The Bandai Datach barcode reader: EAN / JAN symbol encoding and the
//! deterministic serial stream the Datach main unit presents to the CPU.
//!
//! # What the hardware does (`nesdev_wiki/INES_Mapper_157.xhtml`)
//!
//! The Datach main unit plugs into the Famicom cartridge port and carries a
//! one-dimensional barcode scanner. The page describes its interface in one
//! sentence: "the barcode scanner just returns a sequence of light and dark
//! periods from a one-dimesional barcode as the user moves it past the
//! sensor", read in **bit 3** of any CPU read of `$6000-$7FFF` (mask `$E000`;
//! bit 4 of the same read is the serial EEPROMs' data line). There is no
//! command register, no handshake and no buffering: the CPU samples one level
//! at a time and measures the widths of the runs itself. The page also says
//! "the game only expects UPCs to be scanned, so an emulator may wish to
//! provide an interface to convert a series of digits into a UPC". The cards
//! Bandai printed for the Datach carry JAN codes, which are EAN-13 / EAN-8
//! symbols (the UPC family's 13- and 8-digit forms), so that is what this
//! module draws.
//!
//! # The symbol (public EAN / JAN specification)
//!
//! The digit patterns, guard bars and first-digit parity table below are the
//! published EAN-13 / EAN-8 symbology (GS1 General Specifications; the same
//! tables appear in every reference on the symbology). A symbol is a sequence
//! of equal-width **modules**, each either dark (a bar) or light (a space):
//!
//! - EAN-13: start guard `101`, six left-half digits (each 7 modules, drawn
//!   from the L or G set as the first digit's parity pattern selects), centre
//!   guard `01010`, six right-half digits (R set), end guard `101`: **95**
//!   modules. The first digit has no bars of its own; it is encoded only in
//!   the L/G parity pattern of digits 2-7.
//! - EAN-8: start guard, four L digits, centre guard, four R digits, end
//!   guard: **67** modules.
//!
//! The last digit of either form is a check digit: weight the other digits
//! 3, 1, 3, 1, ... from the right, sum, and the check digit brings the sum to
//! a multiple of ten. [`Barcode::parse`] rejects a code whose check digit is
//! wrong, with the digit it expected, so a mistyped card number fails at the
//! boundary instead of inside the game.
//!
//! # The serial stream (this project's model; the page gives no timing)
//!
//! The page states neither the polarity of bit 3 nor any timing, and no other
//! public document found does either (the `NESdev` wiki and forum searched
//! 2026-10-08). The choices here were therefore settled **black-box, against
//! the games themselves**, by presenting candidate streams to the staged
//! Datach dumps at a screen waiting for a card and watching what the game
//! did. No other emulator's source was consulted (the page's FCEUX link was
//! deliberately not followed). The model and the measurements behind it:
//!
//! - **Polarity:** bit 3 reads **1 while the sensor sees light** (a space or
//!   the card's white margin) and **0 over a dark bar** ([`DARK_LEVEL`]).
//!   With the opposite polarity none of the three games that were brought to
//!   a scan prompt reacted to any code at any speed.
//! - **Idle:** with no card passing, bit 3 reads the **dark** level, 0: an
//!   empty slot reflects no light. Idle-light was tried first and *Battle
//!   Rush* then showed only one of its three title-menu items and ignored the
//!   A button; with idle-dark it shows all three and proceeds.
//! - **Timing:** every module lasts [`MODULE_CYCLES`] CPU cycles, and the
//!   symbol is framed by [`QUIET_MODULES`] light modules before and after
//!   (the card's white margin, which includes the symbology's quiet zones).
//!   The window each game accepted, with these margins: *Dragon Ball Z:
//!   Gekitou Tenkaichi Budoukai* (Karin's room) 200-3,000 cycles per module,
//!   *SD Gundam Wars* (Barcode Lab) 200-3,000, *J.League Super Top Players*
//!   (team select) 400-8,000; 5,000 and 8,000 failed in the first two and
//!   100 in all three. 1,000 sits inside all three windows, and a whole
//!   EAN-13 swipe then takes (95 + 2 x 128) x 1,000 = 351,000 cycles, about
//!   0.2 s, a plausible hand swipe. Shorter margins narrowed the windows:
//!   with 32 light modules *Dragon Ball Z* needed at least 700 cycles per
//!   module. A real swipe is neither uniform nor exactly this speed; a
//!   uniform, deterministic sweep is what a reproducible emulator can offer,
//!   and the games measure run widths relative to one another.
//!
//! Time is counted in CPU cycles, never wall-clock time: the stream advances
//! from the mapper's per-M2 hook, so a scan is part of the deterministic
//! emulation (same scan at the same cycle, same frames) and its in-flight
//! position is part of the mapper's save state.

use alloc::vec::Vec;
use core::fmt;

/// Bit 3 of a `$6000-$7FFF` read while the sensor sees a **dark** module
/// (a bar) or no card at all (idle). A light module (a space or the card's
/// margin) reads the opposite level.
///
/// Measured, not documented: see the module preamble.
pub const DARK_LEVEL: bool = false;

/// CPU cycles each module of the symbol is presented for.
///
/// Measured, not documented: see the module preamble and
/// `docs/mappers.md` (mapper 157) for the sweep that settled it.
pub const MODULE_CYCLES: u32 = 1000;

/// Light modules presented before and after the symbol (the card's margin).
///
/// The margin contains the symbology's quiet zones (at least 11 modules left
/// and 7 right of an EAN-13). Measured, not documented: see the module
/// preamble.
pub const QUIET_MODULES: u32 = 128;

/// Why a barcode string was refused at the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarcodeError {
    /// The string held no digits at all.
    Empty,
    /// A character other than an ASCII digit, at this character index
    /// (spaces and hyphens, which cards print as separators, are accepted
    /// and skipped).
    NotADigit {
        /// Character index in the input string.
        index: usize,
        /// The offending character.
        found: char,
    },
    /// Neither 13 (EAN-13 / JAN-13) nor 8 (EAN-8 / JAN-8) digits.
    WrongLength(usize),
    /// The last digit is not the check digit the others require.
    BadCheckDigit {
        /// The check digit the first 12 / 7 digits require.
        expected: u8,
        /// The check digit the string carried.
        found: u8,
    },
}

impl fmt::Display for BarcodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Empty => write!(f, "no digits"),
            Self::NotADigit { index, found } => {
                write!(f, "character {index} ('{found}') is not a digit")
            }
            Self::WrongLength(n) => write!(f, "{n} digits; a JAN/EAN code has 13 or 8"),
            Self::BadCheckDigit { expected, found } => {
                write!(f, "check digit is {found}, expected {expected}")
            }
        }
    }
}

/// A validated EAN-13 or EAN-8 (JAN) code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Barcode {
    digits: [u8; 13],
    len: u8,
}

/// Left-half odd-parity ("L") digit patterns, most significant bit first,
/// 1 = dark module.
const L_CODES: [u8; 10] = [
    0b000_1101, 0b001_1001, 0b001_0011, 0b011_1101, 0b010_0011, 0b011_0001, 0b010_1111, 0b011_1011,
    0b011_0111, 0b000_1011,
];

/// Right-half ("R") patterns: the bitwise complement of the L patterns.
const fn r_code(d: u8) -> u8 {
    !L_CODES[d as usize] & 0x7F
}

/// Left-half even-parity ("G") patterns: the R patterns mirrored.
const fn g_code(d: u8) -> u8 {
    r_code(d).reverse_bits() >> 1
}

/// EAN-13 first-digit parity: for digits 2-7, bit 5 (digit 2) .. bit 0
/// (digit 7), set where the digit uses the G set.
const FIRST_DIGIT_PARITY: [u8; 10] = [
    0b00_0000, 0b00_1011, 0b00_1101, 0b00_1110, 0b01_0011, 0b01_1001, 0b01_1100, 0b01_0101,
    0b01_0110, 0b01_1010,
];

impl Barcode {
    /// Parse and validate a 13- or 8-digit code. Spaces and hyphens are
    /// skipped (cards print the digits in groups).
    ///
    /// # Errors
    ///
    /// [`BarcodeError`] for an empty string, a non-digit character, a digit
    /// count other than 13 or 8, or a wrong check digit.
    pub fn parse(text: &str) -> Result<Self, BarcodeError> {
        let mut digits = [0u8; 13];
        let mut len = 0usize;
        for (index, c) in text.chars().enumerate() {
            match c {
                ' ' | '-' => {}
                '0'..='9' => {
                    if len < digits.len() {
                        digits[len] = c as u8 - b'0';
                    }
                    len += 1;
                }
                found => return Err(BarcodeError::NotADigit { index, found }),
            }
        }
        match len {
            0 => return Err(BarcodeError::Empty),
            8 | 13 => {}
            n => return Err(BarcodeError::WrongLength(n)),
        }
        let expected = check_digit(&digits[..len - 1]);
        let found = digits[len - 1];
        if expected != found {
            return Err(BarcodeError::BadCheckDigit { expected, found });
        }
        // `len` is 8 or 13 here.
        let len = u8::try_from(len).map_err(|_| BarcodeError::WrongLength(len))?;
        Ok(Self { digits, len })
    }

    /// The digits, 13 or 8 of them.
    #[must_use]
    pub fn digits(&self) -> &[u8] {
        &self.digits[..usize::from(self.len)]
    }

    /// The symbol's modules, left to right, `true` = dark (a bar): 95 for
    /// EAN-13, 67 for EAN-8. Quiet zones are not included.
    #[must_use]
    pub fn modules(&self) -> Vec<bool> {
        let mut out = Vec::with_capacity(95);
        let mut push = |pattern: u8, width: u32| {
            for i in (0..width).rev() {
                out.push(pattern >> i & 1 != 0);
            }
        };
        let d = self.digits();
        push(0b101, 3);
        if d.len() == 13 {
            let parity = FIRST_DIGIT_PARITY[usize::from(d[0])];
            for (i, &digit) in d[1..7].iter().enumerate() {
                let g = parity >> (5 - i) & 1 != 0;
                push(
                    if g {
                        g_code(digit)
                    } else {
                        L_CODES[usize::from(digit)]
                    },
                    7,
                );
            }
            push(0b01010, 5);
            for &digit in &d[7..13] {
                push(r_code(digit), 7);
            }
        } else {
            for &digit in &d[..4] {
                push(L_CODES[usize::from(digit)], 7);
            }
            push(0b01010, 5);
            for &digit in &d[4..8] {
                push(r_code(digit), 7);
            }
        }
        push(0b101, 3);
        out
    }
}

impl fmt::Display for Barcode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &d in self.digits() {
            write!(f, "{d}")?;
        }
        Ok(())
    }
}

/// The EAN check digit for `body` (the digits before it): weights 3, 1, 3,
/// ... from the right.
fn check_digit(body: &[u8]) -> u8 {
    let sum: u32 = body
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| u32::from(d) * if i % 2 == 0 { 3 } else { 1 })
        .sum();
    ((10 - sum % 10) % 10) as u8
}

/// The scanner's serial output as the CPU sees it, advanced one CPU cycle
/// at a time.
///
/// Holds the modules of the scan in flight (quiet zones included), the
/// index of the module now under the sensor, and the cycles left on it.
/// Idle (`modules` empty) it reads the dark level.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BarcodeReader {
    /// Levels of the scan in flight, `true` = dark, quiet zones included.
    modules: Vec<bool>,
    /// Index into `modules` of the module under the sensor.
    pos: u32,
    /// CPU cycles left before `pos` advances.
    remaining: u32,
}

impl BarcodeReader {
    /// Begin presenting `code`, replacing any scan in flight. The first
    /// light quiet-zone module starts on the next CPU cycle.
    pub fn scan(&mut self, code: &Barcode) {
        let quiet = QUIET_MODULES as usize;
        let mut modules = Vec::with_capacity(2 * quiet + 95);
        modules.resize(quiet, false);
        modules.extend(code.modules());
        modules.resize(modules.len() + quiet, false);
        self.modules = modules;
        self.pos = 0;
        self.remaining = MODULE_CYCLES;
    }

    /// Whether a scan is in flight.
    #[must_use]
    pub const fn is_scanning(&self) -> bool {
        !self.modules.is_empty()
    }

    /// Advance one CPU cycle. Ends the scan after the last quiet module.
    pub fn tick(&mut self) {
        if self.modules.is_empty() {
            return;
        }
        self.remaining -= 1;
        if self.remaining == 0 {
            self.pos += 1;
            if self.pos as usize >= self.modules.len() {
                self.modules.clear();
                self.pos = 0;
            } else {
                self.remaining = MODULE_CYCLES;
            }
        }
    }

    /// Bit 3 of a `$6000-$7FFF` read: [`DARK_LEVEL`] over a bar and when
    /// idle, its complement over a space or the card's margin.
    #[must_use]
    pub fn level(&self) -> bool {
        // Idle: no card in the slot, nothing reflects, the sensor sees dark.
        let dark = self.modules.get(self.pos as usize).copied().unwrap_or(true);
        if dark { DARK_LEVEL } else { !DARK_LEVEL }
    }

    /// Append the reader's state to a mapper save-state blob:
    /// `u16` module count, `u32` position, `u32` cycles left, then one byte
    /// per eight modules (LSB first).
    pub fn save(&self, out: &mut Vec<u8>) {
        // At most 95 + 2 * QUIET_MODULES modules, far inside `u16`.
        let count = u16::try_from(self.modules.len()).unwrap_or(u16::MAX);
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&self.pos.to_le_bytes());
        out.extend_from_slice(&self.remaining.to_le_bytes());
        for chunk in self.modules.chunks(8) {
            let mut b = 0u8;
            for (i, &m) in chunk.iter().enumerate() {
                b |= u8::from(m) << i;
            }
            out.push(b);
        }
    }

    /// Restore from the front of `data` (the layout [`Self::save`] writes),
    /// returning the bytes consumed, or `None` if `data` is too short or
    /// describes an impossible position.
    pub fn load(&mut self, data: &[u8]) -> Option<usize> {
        let header = data.get(..10)?;
        let count = usize::from(u16::from_le_bytes([header[0], header[1]]));
        let pos = u32::from_le_bytes([header[2], header[3], header[4], header[5]]);
        let remaining = u32::from_le_bytes([header[6], header[7], header[8], header[9]]);
        // `scan` builds an EAN-8 (67 modules) or EAN-13 (95) symbol between two
        // quiet zones and `tick` empties the list when the scan ends, so those
        // are the only lengths a reader can hold. Anything else is a corrupt
        // or hostile blob, refused before anything changes (CodeRabbit on
        // #607: it used to accept any count up to 65,535).
        let quiet = 2 * QUIET_MODULES as usize;
        if count != 0 && count != 67 + quiet && count != 95 + quiet {
            return None;
        }
        let bytes = count.div_ceil(8);
        let packed = data.get(10..10 + bytes)?;
        if count > 0 && (pos as usize >= count || remaining == 0 || remaining > MODULE_CYCLES) {
            return None;
        }
        self.modules = (0..count)
            .map(|i| packed[i / 8] >> (i % 8) & 1 != 0)
            .collect();
        self.pos = if count == 0 { 0 } else { pos };
        self.remaining = if count == 0 { 0 } else { remaining };
        Some(10 + bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::String;
    use alloc::vec::Vec;

    #[test]
    fn a_saved_scan_of_any_other_length_is_refused() {
        let mut r = BarcodeReader::default();
        r.scan(&Barcode::parse("4006381333931").unwrap());
        let mut blob = Vec::new();
        r.save(&mut blob);
        let mut fresh = BarcodeReader::default();
        assert_eq!(fresh.load(&blob), Some(blob.len()), "a real scan loads");
        // 1,000 modules, with packed bytes to match, so only the count is wrong.
        let mut bad = Vec::new();
        bad.extend_from_slice(&1000u16.to_le_bytes());
        bad.extend_from_slice(&0u32.to_le_bytes());
        bad.extend_from_slice(&MODULE_CYCLES.to_le_bytes());
        bad.resize(10 + 1000usize.div_ceil(8), 0);
        let mut other = BarcodeReader::default();
        assert_eq!(other.load(&bad), None);
        assert!(!other.is_scanning(), "nothing restored");
    }

    fn as_text(modules: &[bool]) -> String {
        modules.iter().map(|&m| if m { '1' } else { '0' }).collect()
    }

    #[test]
    fn check_digit_is_validated_with_the_digit_it_expected() {
        // 4006381333931: the symbology's customary worked example.
        assert!(Barcode::parse("4006381333931").is_ok());
        assert_eq!(
            Barcode::parse("4006381333932"),
            Err(BarcodeError::BadCheckDigit {
                expected: 1,
                found: 2
            })
        );
        // EAN-8 96385074: weights 3,1,3,1,3,1,3 over 9638507 = 27+6+9+8+15+0+21 = 86.
        assert!(Barcode::parse("96385074").is_ok());
        assert_eq!(
            Barcode::parse("96385075"),
            Err(BarcodeError::BadCheckDigit {
                expected: 4,
                found: 5
            })
        );
    }

    #[test]
    fn malformed_strings_are_refused_with_a_typed_reason() {
        assert_eq!(Barcode::parse(""), Err(BarcodeError::Empty));
        assert_eq!(Barcode::parse(" - "), Err(BarcodeError::Empty));
        assert_eq!(
            Barcode::parse("40063813339x1"),
            Err(BarcodeError::NotADigit {
                index: 11,
                found: 'x'
            })
        );
        assert_eq!(
            Barcode::parse("123456789012"),
            Err(BarcodeError::WrongLength(12))
        );
        assert_eq!(
            Barcode::parse("12345678901234"),
            Err(BarcodeError::WrongLength(14))
        );
        // Grouping separators are skipped.
        assert_eq!(
            Barcode::parse("4 006381 333931").unwrap().digits(),
            &[4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1]
        );
    }

    /// The EAN-13 symbol for 4006381333931, written out by hand from the
    /// published tables (first digit 4 selects parity LGLLGG for digits 2-7):
    ///
    /// ```text
    /// 101                                  start
    /// 0 L 0001101   0 G 0100111   6 L 0101111
    /// 3 L 0111101   8 G 0001001   1 G 0110011
    /// 01010                                centre
    /// 3 R 1000010   3 R 1000010   3 R 1000010
    /// 9 R 1110100   3 R 1000010   1 R 1100110
    /// 101                                  end
    /// ```
    #[test]
    fn ean13_modules_match_the_published_tables() {
        let code = Barcode::parse("4006381333931").unwrap();
        let want = concat!(
            "101", "0001101", "0100111", "0101111", "0111101", "0001001", "0110011", "01010",
            "1000010", "1000010", "1000010", "1110100", "1000010", "1100110", "101",
        );
        assert_eq!(as_text(&code.modules()), want);
        assert_eq!(want.len(), 95);
    }

    /// Every digit in every set, typed out from the published EAN tables
    /// (L = odd parity left half, G = even parity left half, R = right half),
    /// and the first-digit parity table. The worked examples below exercise
    /// only some digits per set; this pins all thirty patterns and all ten
    /// parity rows, so a corrupted table entry cannot hide behind a code
    /// that happens not to use it.
    #[test]
    fn every_digit_pattern_and_parity_row_matches_the_published_tables() {
        const L: [&str; 10] = [
            "0001101", "0011001", "0010011", "0111101", "0100011", "0110001", "0101111", "0111011",
            "0110111", "0001011",
        ];
        const G: [&str; 10] = [
            "0100111", "0110011", "0011011", "0100001", "0011101", "0111001", "0000101", "0010001",
            "0001001", "0010111",
        ];
        const R: [&str; 10] = [
            "1110010", "1100110", "1101100", "1000010", "1011100", "1001110", "1010000", "1000100",
            "1001000", "1110100",
        ];
        const PARITY: [&str; 10] = [
            "LLLLLL", "LLGLGG", "LLGGLG", "LLGGGL", "LGLLGG", "LGGLLG", "LGGGLL", "LGLGLG",
            "LGLGGL", "LGGLGL",
        ];
        let bits = |p: u8| -> String {
            (0..7)
                .rev()
                .map(|i| if p >> i & 1 != 0 { '1' } else { '0' })
                .collect()
        };
        for d in 0..10u8 {
            assert_eq!(bits(L_CODES[usize::from(d)]), L[usize::from(d)], "L {d}");
            assert_eq!(bits(g_code(d)), G[usize::from(d)], "G {d}");
            assert_eq!(bits(r_code(d)), R[usize::from(d)], "R {d}");
            let row: String = (0..6)
                .map(|i| {
                    if FIRST_DIGIT_PARITY[usize::from(d)] >> (5 - i) & 1 != 0 {
                        'G'
                    } else {
                        'L'
                    }
                })
                .collect();
            assert_eq!(row, PARITY[usize::from(d)], "parity row {d}");
        }
    }

    /// EAN-8 96385074: start, L 9 6 3 8, centre, R 5 0 7 4, end.
    #[test]
    fn ean8_modules_match_the_published_tables() {
        let code = Barcode::parse("96385074").unwrap();
        let want = concat!(
            "101", "0001011", "0101111", "0111101", "0110111", "01010", "1001110", "1110010",
            "1000100", "1011100", "101",
        );
        assert_eq!(as_text(&code.modules()), want);
        assert_eq!(want.len(), 67);
    }

    /// The stream the CPU samples: quiet light, the symbol module by module,
    /// quiet light, then idle -- each module exactly [`MODULE_CYCLES`] cycles,
    /// read through [`DARK_LEVEL`].
    #[test]
    fn the_reader_presents_each_module_for_module_cycles() {
        let code = Barcode::parse("96385074").unwrap();
        let mut r = BarcodeReader::default();
        assert!(!r.is_scanning());
        assert_eq!(r.level(), DARK_LEVEL, "idle reads dark: an empty slot");
        r.scan(&code);
        let mut seen = String::new();
        let mut cycles = 0u32;
        while r.is_scanning() {
            if cycles.is_multiple_of(MODULE_CYCLES) {
                seen.push(if r.level() == DARK_LEVEL { '1' } else { '0' });
            } else {
                // Within a module the level never changes.
                assert_eq!(seen.ends_with('1'), r.level() == DARK_LEVEL);
            }
            r.tick();
            cycles += 1;
        }
        let quiet = "0".repeat(QUIET_MODULES as usize);
        assert_eq!(seen, format!("{quiet}{}{quiet}", as_text(&code.modules())));
        assert_eq!(cycles, (67 + 2 * QUIET_MODULES) * MODULE_CYCLES);
        assert_eq!(r.level(), DARK_LEVEL, "idle (dark) again after the scan");
    }

    #[test]
    fn reader_state_round_trips_mid_scan_and_refuses_garbage() {
        let mut r = BarcodeReader::default();
        r.scan(&Barcode::parse("4006381333931").unwrap());
        for _ in 0..(40 * MODULE_CYCLES + 17) {
            r.tick();
        }
        let mut blob = Vec::new();
        r.save(&mut blob);
        let mut back = BarcodeReader::default();
        assert_eq!(back.load(&blob), Some(blob.len()));
        assert_eq!(back, r);
        // A position past the end is refused.
        let mut bad = blob.clone();
        bad[2..6].copy_from_slice(&10_000u32.to_le_bytes());
        assert_eq!(BarcodeReader::default().load(&bad), None);
        assert_eq!(BarcodeReader::default().load(&blob[..5]), None);
        // An idle reader round-trips to idle.
        let mut idle = Vec::new();
        BarcodeReader::default().save(&mut idle);
        assert_eq!(idle.len(), 10);
        let mut back = r.clone();
        assert_eq!(back.load(&idle), Some(10));
        assert!(!back.is_scanning());
    }
}

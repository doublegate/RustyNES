//! Game Genie cheat-code decoding.
//!
//! The Game Genie is a pass-through cartridge adapter that substitutes bytes
//! the console reads from PRG-ROM (`$8000-$FFFF`). A 6-character code is an
//! unconditional `(address, data)` substitution; an 8-character code adds a
//! `compare` byte so the substitution only fires when the original byte
//! matches (this lets one code target a specific bank in a mirrored address).
//!
//! Codes are a runtime overlay applied on the CPU read path in
//! [`crate::LockstepBus`]; they are **not** part of emulation state (not
//! serialized into save states), so with no codes active every read is
//! byte-identical to a build without this feature — the determinism contract
//! is preserved. The frontend persists the user's code strings per-ROM.
//!
//! The 6/8-character decode is the canonical NES Game Genie algorithm
//! (nesdev wiki "Game Genie"). This is a clean-room reimplementation
//! cross-checked against the reference codes `YYKPOYZZ` (The Legend of Zelda
//! — `$9F41`, data `$77`, compare `$22`) and `SXIOPO` (Super Mario Bros.
//! infinite lives — `$91D9`, data `$AD`, no compare), and against the
//! structure of `TetaNES` `tetarustynes-core/src/genie.rs` (MIT).

use alloc::string::String;
use core::fmt;

/// Error returned when a Game Genie code string cannot be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenieError {
    /// The code was not 6 or 8 characters long.
    InvalidLength(usize),
    /// The code contained a letter outside the 16-letter Game Genie alphabet
    /// (`A P Z L G I T Y E O X U K S V N`, case-insensitive).
    InvalidCharacter(char),
}

impl fmt::Display for GenieError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength(n) => {
                write!(f, "Game Genie code must be 6 or 8 characters, found {n}")
            }
            Self::InvalidCharacter(c) => write!(f, "invalid Game Genie character '{c}'"),
        }
    }
}

impl core::error::Error for GenieError {}

/// A decoded Game Genie code: an address in `$8000-$FFFF`, a substitute data
/// byte, and an optional compare byte (present only for 8-character codes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenieCode {
    code: String,
    addr: u16,
    data: u8,
    compare: Option<u8>,
}

impl GenieCode {
    /// Decode a 6- or 8-character Game Genie code.
    ///
    /// The input is case-insensitive; [`code`](Self::code) returns the
    /// canonical upper-case form.
    ///
    /// # Errors
    ///
    /// Returns [`GenieError::InvalidLength`] if the code is not 6 or 8
    /// characters, or [`GenieError::InvalidCharacter`] if it contains a
    /// letter outside the Game Genie alphabet.
    pub fn new(code: &str) -> Result<Self, GenieError> {
        let len = code.chars().count();
        if len != 6 && len != 8 {
            return Err(GenieError::InvalidLength(len));
        }
        let mut hex = [0u8; 8];
        for (i, c) in code.chars().enumerate() {
            hex[i] = letter_to_nibble(c).ok_or(GenieError::InvalidCharacter(c))?;
        }

        // Address bits are shuffled identically for 6- and 8-character codes.
        let addr = 0x8000
            + (((u16::from(hex[3]) & 7) << 12)
                | ((u16::from(hex[5]) & 7) << 8)
                | ((u16::from(hex[4]) & 8) << 8)
                | ((u16::from(hex[2]) & 7) << 4)
                | ((u16::from(hex[1]) & 8) << 4)
                | (u16::from(hex[4]) & 7)
                | (u16::from(hex[3]) & 8));

        // The data byte's high "bank" bit comes from hex[5] in a 6-char code
        // and hex[7] in an 8-char code; an 8-char code also carries a compare.
        let (data, compare) = if len == 6 {
            let data = ((hex[1] & 7) << 4) | ((hex[0] & 8) << 4) | (hex[0] & 7) | (hex[5] & 8);
            (data, None)
        } else {
            let data = ((hex[1] & 7) << 4) | ((hex[0] & 8) << 4) | (hex[0] & 7) | (hex[7] & 8);
            let compare = ((hex[7] & 7) << 4) | ((hex[6] & 8) << 4) | (hex[6] & 7) | (hex[5] & 8);
            (data, Some(compare))
        };

        Ok(Self {
            code: code.to_ascii_uppercase(),
            addr,
            data,
            compare,
        })
    }

    /// The canonical upper-case code string.
    #[must_use]
    // clippy::missing_const_for_fn is a false positive here: `&self.code`
    // requires a non-const `String -> str` deref coercion (E0015).
    #[allow(clippy::missing_const_for_fn)]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The PRG address (`$8000-$FFFF`) this code substitutes.
    #[must_use]
    pub const fn addr(&self) -> u16 {
        self.addr
    }

    /// The substitute data byte.
    #[must_use]
    pub const fn data(&self) -> u8 {
        self.data
    }

    /// The compare byte (8-character codes only).
    #[must_use]
    pub const fn compare(&self) -> Option<u8> {
        self.compare
    }

    /// Apply this code to a byte read from [`addr`](Self::addr).
    ///
    /// For a 6-character code the substitute [`data`](Self::data) always
    /// replaces `original`. For an 8-character code the substitution only
    /// fires when `original` equals the compare byte; otherwise the original
    /// byte passes through (so the code targets one specific bank).
    #[must_use]
    pub const fn read(&self, original: u8) -> u8 {
        match self.compare {
            Some(c) if original != c => original,
            _ => self.data,
        }
    }
}

impl fmt::Display for GenieCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.code)
    }
}

/// Map a Game Genie letter to its 4-bit nibble (case-insensitive).
const fn letter_to_nibble(c: char) -> Option<u8> {
    Some(match c.to_ascii_uppercase() {
        'A' => 0x0,
        'P' => 0x1,
        'Z' => 0x2,
        'L' => 0x3,
        'G' => 0x4,
        'I' => 0x5,
        'T' => 0x6,
        'Y' => 0x7,
        'E' => 0x8,
        'O' => 0x9,
        'X' => 0xA,
        'U' => 0xB,
        'K' => 0xC,
        'S' => 0xD,
        'V' => 0xE,
        'N' => 0xF,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letter_table_matches_known_bit_string() {
        // SXIOPO decodes to bits 1101 1010 0101 1001 0001 1001 (nesdev /
        // widely-published SMB example), i.e. nibbles D A 5 9 1 9.
        let nibbles: alloc::vec::Vec<u8> = "SXIOPO"
            .chars()
            .map(|c| letter_to_nibble(c).unwrap())
            .collect();
        assert_eq!(nibbles, alloc::vec![0xD, 0xA, 0x5, 0x9, 0x1, 0x9]);
    }

    #[test]
    fn decodes_eight_char_code_with_compare() {
        // The Legend of Zelda "8 hearts" code (TetaNES reference).
        let gc = GenieCode::new("YYKPOYZZ").unwrap();
        assert_eq!(gc.addr(), 0x9F41);
        assert_eq!(gc.data(), 0x77);
        assert_eq!(gc.compare(), Some(0x22));
        // Substitution fires only when the original matches the compare byte.
        assert_eq!(gc.read(0x22), 0x77);
        assert_eq!(gc.read(0x00), 0x00);
    }

    #[test]
    fn decodes_six_char_code_without_compare() {
        // Super Mario Bros. infinite lives.
        let gc = GenieCode::new("SXIOPO").unwrap();
        assert_eq!(gc.addr(), 0x91D9);
        assert_eq!(gc.data(), 0xAD);
        assert_eq!(gc.compare(), None);
        // No compare: always substitutes.
        assert_eq!(gc.read(0x00), 0xAD);
        assert_eq!(gc.read(0xFF), 0xAD);
    }

    #[test]
    fn case_insensitive_and_canonicalized() {
        let gc = GenieCode::new("sxiopo").unwrap();
        assert_eq!(gc.code(), "SXIOPO");
        assert_eq!(gc.addr(), 0x91D9);
    }

    #[test]
    fn rejects_bad_length_and_characters() {
        assert_eq!(
            GenieCode::new("ABC").unwrap_err(),
            GenieError::InvalidLength(3)
        );
        assert_eq!(
            GenieCode::new("ABCDEFG").unwrap_err(),
            GenieError::InvalidLength(7)
        );
        // 'W' is not in the Game Genie alphabet.
        assert_eq!(
            GenieCode::new("WXIOPO").unwrap_err(),
            GenieError::InvalidCharacter('W')
        );
    }

    #[test]
    fn all_addresses_land_in_prg_window() {
        // Every decoded code must address $8000-$FFFF.
        for code in [
            "AAAAAA", "NNNNNN", "AAAAAAAA", "NNNNNNNN", "SXIOPO", "YYKPOYZZ",
        ] {
            assert!(GenieCode::new(code).unwrap().addr() >= 0x8000);
        }
    }
}

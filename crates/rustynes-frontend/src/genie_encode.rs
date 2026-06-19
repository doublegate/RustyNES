//! Game Genie **encoder** + `.tbl` text-table parser (v1.7.0 "Forge"
//! Workstream H9 power-user niceties).
//!
//! # Encoder
//!
//! The emulation core ([`rustynes_core::genie`]) only ever **decodes** a code
//! string into an `(address, data, compare)` substitution. A power user who
//! knows the raw `(address, data[, compare])` they want — e.g. from a RAM
//! search or a disassembly — needs the **inverse**: produce the canonical
//! 6- or 8-character Game Genie code. This module is that inverse, and it is
//! frontend-only + pure: it never touches the core, and every code it produces
//! round-trips back through [`rustynes_core::GenieCode::new`] to the exact same
//! substitution (a property the unit tests assert against the core decoder).
//!
//! The bit shuffle is the canonical NES Game Genie algorithm (nesdev wiki
//! "Game Genie"), the exact inverse of the core's `GenieCode::new` shuffle.
//!
//! # `.tbl` text tables
//!
//! A `.tbl` file maps ROM byte values to display glyphs (the de-facto
//! community format used by ROM-hacking tools and TAS authors: `XX=glyph`
//! lines, where `XX` is a hex byte). [`Table`] parses one and renders a byte
//! stream into readable text — handy in the hex editor / RAM search when a
//! game uses a non-ASCII character encoding.

use std::collections::BTreeMap;

/// The 16-letter Game Genie alphabet, indexed by nibble value (`0x0..=0xF`).
/// The exact inverse of `rustynes_core::genie`'s `letter_to_nibble`.
const ALPHABET: [char; 16] = [
    'A', 'P', 'Z', 'L', 'G', 'I', 'T', 'Y', 'E', 'O', 'X', 'U', 'K', 'S', 'V', 'N',
];

/// Encode a 6-character (no-compare) Game Genie code for `addr` / `data`.
///
/// `addr` is masked into the `$8000-$FFFF` PRG window (only the low 15 bits
/// carry; the `$8000` base is implicit in the format), and `data` is a full
/// byte. The returned string is the canonical upper-case code.
///
/// This is the exact inverse of the core's 6-character decode, so
/// `GenieCode::new(&encode_6(addr, data))` yields `(addr, data, None)`.
#[must_use]
pub fn encode_6(addr: u16, data: u8) -> String {
    let mut hex = [0u8; 6];
    write_addr_nibbles(&mut hex, addr);
    // data byte (inverse of: data = (h1&7)<<4 | (h0&8)<<4 | (h0&7) | (h5&8))
    hex[0] |= ((data >> 4) & 8) | (data & 7);
    hex[1] |= (data >> 4) & 7;
    hex[5] |= data & 8;
    nibbles_to_string(&hex)
}

/// Encode an 8-character (with-compare) Game Genie code for `addr` / `data` /
/// `compare`.
///
/// The exact inverse of the core's 8-character decode, so
/// `GenieCode::new(&encode_8(addr, data, cmp))` yields `(addr, data,
/// Some(cmp))`.
#[must_use]
pub fn encode_8(addr: u16, data: u8, compare: u8) -> String {
    let mut hex = [0u8; 8];
    write_addr_nibbles(&mut hex, addr);
    // data byte (8-char: high bit lives in hex[7] not hex[5]).
    hex[0] |= ((data >> 4) & 8) | (data & 7);
    hex[1] |= (data >> 4) & 7;
    hex[7] |= data & 8;
    // compare byte: cmp = (h7&7)<<4 | (h6&8)<<4 | (h6&7) | (h5&8)
    hex[7] |= (compare >> 4) & 7;
    hex[6] |= ((compare >> 4) & 8) | (compare & 7);
    hex[5] |= compare & 8;
    nibbles_to_string(&hex)
}

/// Write the address nibbles (shared by the 6- and 8-character forms) into the
/// destination nibble array. The inverse of the core's `addr` shuffle:
///
/// ```text
/// addr = 0x8000
///   + (((h3 & 7) << 12) | ((h5 & 7) << 8) | ((h4 & 8) << 8)
///      | ((h2 & 7) << 4) | ((h1 & 8) << 4) | (h4 & 7) | (h3 & 8))
/// ```
fn write_addr_nibbles(hex: &mut [u8], addr: u16) {
    let a = addr & 0x7FFF; // the $8000 base is implicit in the encoding.
    hex[3] |= ((a >> 12) & 7) as u8; // bits 12..15
    hex[5] |= ((a >> 8) & 7) as u8; // bits 8..11
    hex[4] |= ((a >> 8) & 8) as u8; // bit 11 (the &8 term)
    hex[2] |= ((a >> 4) & 7) as u8; // bits 4..7
    hex[1] |= ((a >> 4) & 8) as u8; // bit 7 (the &8 term)
    hex[4] |= (a & 7) as u8; // bits 0..3
    hex[3] |= (a & 8) as u8; // bit 3 (the &8 term)
}

/// Map a nibble array to the canonical Game Genie code string.
fn nibbles_to_string(hex: &[u8]) -> String {
    hex.iter().map(|&n| ALPHABET[(n & 0xF) as usize]).collect()
}

/// A parsed `.tbl` text table: a byte → glyph map.
///
/// Lines are `XX=glyph` (`XX` = a two-hex-digit byte; `glyph` = the rest of the
/// line, which MAY be empty or multi-character). Blank lines and `#`/`;`
/// comment lines are ignored, as are malformed lines (so a partial table is
/// still usable). Multi-byte (`XXYY=...`) table entries are out of scope (the
/// common single-byte form covers the NES use case).
#[derive(Debug, Clone, Default)]
pub struct Table {
    map: BTreeMap<u8, String>,
}

impl Table {
    /// Parse a `.tbl` file's text.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut map = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim_end_matches(['\r', '\n']);
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }
            // Split on the FIRST '=' so the glyph can itself contain '='.
            let Some(eq) = line.find('=') else { continue };
            let (key, value) = line.split_at(eq);
            let value = &value[1..]; // drop the '='
            let key = key.trim();
            // Single-byte entries only (two hex digits).
            if key.len() != 2 {
                continue;
            }
            let Ok(byte) = u8::from_str_radix(key, 16) else {
                continue;
            };
            map.insert(byte, value.to_string());
        }
        Self { map }
    }

    /// Number of byte→glyph entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// `true` if the table has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The glyph for a single byte, if mapped.
    #[must_use]
    pub fn glyph(&self, byte: u8) -> Option<&str> {
        self.map.get(&byte).map(String::as_str)
    }

    /// Render a byte stream into display text. Unmapped bytes are shown as
    /// `<XX>` (their hex value in angle brackets) so nothing is silently lost.
    #[must_use]
    pub fn render(&self, bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        for &b in bytes {
            match self.glyph(b) {
                Some(g) => out.push_str(g),
                None => {
                    let _ = write!(out, "<{b:02X}>");
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_core::GenieCode;

    #[test]
    fn encode_6_round_trips_through_core_decoder() {
        // Sweep a spread of addresses + data; each encoded code must decode
        // back to the same substitution via the authoritative core decoder.
        for &addr in &[0x8000u16, 0x91D9, 0x9F41, 0xC1A3, 0xFFFF, 0xABCD] {
            for &data in &[0x00u8, 0xAD, 0x77, 0xFF, 0x5A, 0x13] {
                let code = encode_6(addr, data);
                assert_eq!(code.chars().count(), 6, "6-char code length");
                let decoded = GenieCode::new(&code).expect("encoded code decodes");
                assert_eq!(decoded.addr(), addr, "addr round-trips for {code}");
                assert_eq!(decoded.data(), data, "data round-trips for {code}");
                assert_eq!(decoded.compare(), None, "6-char has no compare");
            }
        }
    }

    #[test]
    fn encode_8_round_trips_through_core_decoder() {
        for &addr in &[0x8000u16, 0x9F41, 0xC1A3, 0xFFFF, 0x1234 | 0x8000] {
            for &data in &[0x00u8, 0x77, 0xFF, 0x5A] {
                for &cmp in &[0x00u8, 0x22, 0xAB, 0xFF] {
                    let code = encode_8(addr, data, cmp);
                    assert_eq!(code.chars().count(), 8, "8-char code length");
                    let decoded = GenieCode::new(&code).expect("encoded code decodes");
                    assert_eq!(decoded.addr(), addr, "addr round-trips for {code}");
                    assert_eq!(decoded.data(), data, "data round-trips for {code}");
                    assert_eq!(
                        decoded.compare(),
                        Some(cmp),
                        "compare round-trips for {code}"
                    );
                }
            }
        }
    }

    #[test]
    fn known_reference_codes() {
        // SMB infinite lives: $91D9, data $AD, no compare = SXIOPO. (The
        // address bits used by the format are uniquely determined here, so the
        // canonical string is exact.)
        assert_eq!(encode_6(0x91D9, 0xAD), "SXIOPO");

        // Zelda: $9F41, data $77, compare $22. The published string is
        // YYKPOYZZ; our encoder emits the canonical-letter form YYGPOYZZ.
        // These differ only in an address nibble bit the decode's shuffle does
        // NOT read, so BOTH decode to the identical substitution — which is the
        // real correctness property (encode is the decode's inverse).
        let ours = encode_8(0x9F41, 0x77, 0x22);
        let theirs = GenieCode::new("YYKPOYZZ").unwrap();
        let mine = GenieCode::new(&ours).unwrap();
        assert_eq!(mine.addr(), theirs.addr());
        assert_eq!(mine.data(), theirs.data());
        assert_eq!(mine.compare(), theirs.compare());
        assert_eq!(
            (mine.addr(), mine.data(), mine.compare()),
            (0x9F41, 0x77, Some(0x22))
        );
    }

    #[test]
    fn tbl_parses_and_renders() {
        let text = "# a comment\n\
                    ; another comment\n\
                    00=A\n\
                    01=B\n\
                    20= \n\
                    0A=hello\n\
                    bad line\n\
                    GG=skip\n";
        let t = Table::parse(text);
        assert_eq!(t.len(), 4, "four valid single-byte entries");
        assert_eq!(t.glyph(0x00), Some("A"));
        assert_eq!(t.glyph(0x20), Some(" "));
        assert_eq!(t.glyph(0x0A), Some("hello"));
        assert_eq!(t.glyph(0xFF), None);
        assert_eq!(t.render(&[0x00, 0x01, 0xFF]), "AB<FF>");
    }
}

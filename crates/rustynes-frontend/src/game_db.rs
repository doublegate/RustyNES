#![allow(clippy::doc_markdown)] // iNES / CRC32 / PRG / CHR / TetaNES are domain terms.

//! Per-game database (v1.1.0 beta.1, Workstream B, T-110-B4).
//!
//! A CRC32-keyed table of per-game corrections, vendored from TetaNES'
//! `game_database.txt` (see `game_database.txt` for the attribution + format).
//! RustyNES currently consumes the **mirroring** column: at ROM load the
//! frontend computes the ROM's CRC32 and, if the DB lists it, applies a
//! nametable-mirroring override via [`rustynes_core::Nes::set_mirroring_override`]
//! — a load-time fix for ROMs whose iNES header carries the wrong mirroring
//! flag.
//!
//! This is **frontend-only**: the core test suites (AccuracyCoin, the commercial
//! oracle, `nestest`) construct the `Nes` directly and never consult this DB, so
//! they stay byte-identical. The override is deterministic (same CRC ⇒ same
//! mirroring) and persisted in the save-state, so netplay + rollback stay
//! consistent. Both peers in a netplay session resolve the same override from
//! the shared ROM.
//!
//! ## Key
//!
//! The key is the CRC32 of the PRG-ROM concatenated with the CHR-ROM, **excluding
//! the 16-byte iNES header (and the 512-byte trainer, if present)** — the
//! standard "ROM CRC" used by TetaNES / Nestopia. iNES-1.0 sizing is used; NES
//! 2.0 ROMs with extended sizes simply don't match (no override — safe).

use std::sync::OnceLock;

use rustynes_core::rustynes_mappers::Mirroring;

/// The vendored database text (CRC + correction columns; `#` lines are comments).
const DB_TEXT: &str = include_str!("game_database.txt");

/// Parsed `(crc32, mirroring)` table, sorted by CRC for binary search. Built
/// once on first use. Only rows with a recognized mirroring are kept.
fn db() -> &'static [(u32, Mirroring)] {
    static DB: OnceLock<Vec<(u32, Mirroring)>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut rows: Vec<(u32, Mirroring)> = DB_TEXT.lines().filter_map(parse_row).collect();
        rows.sort_unstable_by_key(|&(crc, _)| crc);
        rows.dedup_by_key(|&mut (crc, _)| crc);
        rows
    })
}

/// Parse one `CRC, Region, Mapper, Sub-Mapper, ChrBanks, PrgRomBanks,
/// PrgRamBanks, Battery, Mirroring, Title` row into `(crc32, mirroring)`.
/// Returns `None` for comment / blank / malformed lines.
fn parse_row(line: &str) -> Option<(u32, Mirroring)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut fields = line.split(',');
    let crc = u32::from_str_radix(fields.next()?.trim(), 16).ok()?;
    // Mirroring is the 9th field (index 8): skip 7 between CRC and it.
    let mirroring = fields.nth(7)?.trim();
    let mirroring = match mirroring {
        "Horizontal" => Mirroring::Horizontal,
        "Vertical" => Mirroring::Vertical,
        "FourScreen" => Mirroring::FourScreen,
        "SingleScreenA" => Mirroring::SingleScreenA,
        "SingleScreenB" => Mirroring::SingleScreenB,
        // Unknown / mapper-controlled rows carry no useful static override.
        _ => return None,
    };
    Some((crc, mirroring))
}

/// Look up a ROM's mirroring correction by CRC32, or `None` if not listed.
#[must_use]
pub fn mirroring_for_crc(crc: u32) -> Option<Mirroring> {
    let db = db();
    db.binary_search_by_key(&crc, |&(c, _)| c)
        .ok()
        .map(|i| db[i].1)
}

/// Compute the "ROM CRC" of an iNES image: CRC32 of PRG-ROM + CHR-ROM.
///
/// Excludes the 16-byte header and any 512-byte trainer. Returns `None` if the
/// bytes are not a plausible iNES image or are too short for the header-declared
/// sizes.
#[must_use]
pub fn rom_crc32(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 16 || &bytes[0..4] != b"NES\x1A" {
        return None;
    }
    let prg = (bytes[4] as usize) * 16 * 1024;
    let chr = (bytes[5] as usize) * 8 * 1024;
    let trainer: usize = if bytes[6] & 0x04 != 0 { 512 } else { 0 };
    let start = 16 + trainer;
    let end = start.checked_add(prg)?.checked_add(chr)?;
    if prg == 0 || end > bytes.len() {
        return None;
    }
    Some(crc32(&bytes[start..end]))
}

/// IEEE CRC-32 (reflected, polynomial `0xEDB8_8320`) — the zip/PNG CRC, matching
/// TetaNES' `compute_crc32`. Table-less; runs once per ROM load.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_known_vector() {
        // The CRC-32 of "123456789" is the standard 0xCBF43926 check value.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn db_parses_and_is_sorted() {
        let db = db();
        assert!(!db.is_empty(), "vendored DB must parse some rows");
        assert!(
            db.windows(2).all(|w| w[0].0 < w[1].0),
            "DB must be strictly sorted by CRC (binary-search invariant)"
        );
        // A known entry from the vendored file: Mega Man 3 (Europe) 0x1388B3 = Vertical.
        assert_eq!(mirroring_for_crc(0x0013_88B3), Some(Mirroring::Vertical));
    }

    #[test]
    fn lookup_miss_is_none() {
        assert_eq!(mirroring_for_crc(0xDEAD_BEEF), None);
    }

    #[test]
    fn rom_crc32_rejects_non_ines() {
        assert_eq!(rom_crc32(b"not a rom"), None);
        assert_eq!(rom_crc32(&[]), None);
    }

    #[test]
    fn rom_crc32_computes_over_prg_chr() {
        // Minimal iNES: header (16) + 1x16KB PRG + 1x8KB CHR, all 0xAA.
        let mut rom = vec![0u8; 16 + 16 * 1024 + 8 * 1024];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1; // 1 PRG bank
        rom[5] = 1; // 1 CHR bank
        for b in &mut rom[16..] {
            *b = 0xAA;
        }
        let crc = rom_crc32(&rom).expect("valid iNES");
        // CRC over the 24KB of 0xAA, independent of the header bytes.
        let expected = crc32(&vec![0xAAu8; 16 * 1024 + 8 * 1024]);
        assert_eq!(crc, expected);
    }
}

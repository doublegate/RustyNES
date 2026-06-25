//! UNIF (`.unf` / `.unif`) cartridge-container parser (v1.6.0 Workstream E2).
//!
//! UNIF is a chunked container that, unlike iNES, carries **no mapper number** —
//! it identifies the cartridge by a **board-name string** in its `MAPR` chunk.
//! This module parses the container (header + length-prefixed chunks), assembles
//! the PRG/CHR banks, and resolves the board name to an iNES mapper id via
//! [`board_to_mapper`] so the result can be routed through the existing mapper
//! dispatch. It unlocks the pirate / multicart / homebrew dumps that exist only
//! as UNIF (no iNES equivalent).
//!
//! # Container layout
//!
//! ```text
//! HEADER (32 bytes):
//!     magic    : "UNIF"     (4 bytes)
//!     revision : u32 LE      (4 bytes)
//!     reserved : 24 bytes    (zero)
//! CHUNKS (repeat to EOF):
//!     id     : 4 ASCII bytes  (e.g. "MAPR", "PRG0", "CHR0", "MIRR", "BATR")
//!     length : u32 LE
//!     data   : `length` bytes
//! ```
//!
//! Chunk IDs are case-sensitive 4-byte tags. The ones this loader consumes:
//! `MAPR` (board name, NUL-terminated ASCII), `PRG0`..`PRGF` / `CHR0`..`CHRF`
//! (ROM banks, concatenated in ascending index order), `MIRR` (1-byte mirroring
//! mode), `BATR` (presence ⇒ battery-backed), `TVCI` (1-byte TV system). Other
//! chunks (`NAME`, `DINF`, `PCK?`, `CCK?`, `READ`, …) are skipped. This is the
//! pure container parse; building a [`crate::Cartridge`] + the mapper from the
//! resolved fields is the loader's job.
//!
//! `no_std` + `alloc` only (the chip stack constraint). Sources for the board
//! table: Mesen2 + puNES `src/core/unif.c`, cross-checked against
//! `docs/mappers.md` (see `scripts/coverage/UNIF_BOARD_MAP.md`).

use alloc::{string::String, vec::Vec};

use crate::cartridge::{Mirroring, Region};
use thiserror::Error;

/// Magic prefix of every UNIF container — the first 4 bytes of a `.unf` file.
pub const UNIF_MAGIC: &[u8; 4] = b"UNIF";

/// Fixed UNIF header length: magic(4) + revision(4) + reserved(24).
const UNIF_HEADER_LEN: usize = 32;

/// Errors from parsing a UNIF container.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnifError {
    /// Shorter than the 32-byte fixed header.
    #[error("UNIF truncated: header needs {UNIF_HEADER_LEN} bytes, got {0}")]
    HeaderTruncated(usize),
    /// The magic prefix is not `"UNIF"`.
    #[error("UNIF magic mismatch: expected \"UNIF\", got {0:?}")]
    BadMagic([u8; 4]),
    /// A chunk's declared length runs past the end of the file.
    #[error("UNIF chunk {id} at offset {offset} declares {len} bytes past EOF")]
    ChunkOverrun {
        /// 4-char chunk id.
        id: String,
        /// Byte offset of the chunk header.
        offset: usize,
        /// Declared payload length.
        len: usize,
    },
    /// No `MAPR` board-name chunk was present.
    #[error("UNIF has no MAPR board-name chunk")]
    NoMapr,
    /// The `MAPR` board name does not resolve to a known iNES mapper.
    #[error("UNIF board name {0:?} is not a known/implemented board")]
    UnknownBoard(String),
}

/// A parsed + resolved UNIF image, ready to build a [`crate::Cartridge`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifImage {
    /// The raw `MAPR` board name (NUL trimmed), e.g. `"NES-NROM"`.
    pub board: String,
    /// iNES mapper id resolved from [`Self::board`] via [`board_to_mapper`].
    pub mapper_id: u16,
    /// PRG-ROM: `PRG0`..`PRGF` chunks concatenated in ascending index order.
    pub prg_rom: Vec<u8>,
    /// CHR-ROM: `CHR0`..`CHRF` concatenated (empty ⇒ the board uses CHR-RAM).
    pub chr_rom: Vec<u8>,
    /// Initial mirroring from the `MIRR` chunk (default horizontal).
    pub mirroring: Mirroring,
    /// `true` if a `BATR` chunk was present (battery-backed save RAM).
    pub has_battery: bool,
    /// Region from the `TVCI` chunk (NTSC default; "both" maps to NTSC).
    pub region: Region,
}

/// Resolve a UNIF `MAPR` board name to its iNES mapper id, or `None` if the
/// board is unknown / not implemented by `RustyNES`.
///
/// Matching is case-insensitive and tolerant of the standard UNIF vendor
/// prefixes (`NES-`, `HVC-`, `UNL-`, `BMC-`, `BTL-`) — `"NES-NROM"` and
/// `"NROM"` both resolve to mapper 0. The table is the `RustyNES`-implemented
/// subset (porting an unimplemented board would only fail later in dispatch);
/// it is not the full UNIF board universe.
#[must_use]
pub fn board_to_mapper(board: &str) -> Option<u16> {
    // Strip the NUL terminator(s) first, THEN trim — otherwise trailing
    // whitespace hidden behind a `\0` (e.g. "NES-NROM \0") would survive.
    let upper = board.trim_end_matches('\0').trim().to_ascii_uppercase();
    // Try the name as-is, then with a leading vendor prefix stripped.
    if let Some(m) = lookup_board(&upper) {
        return Some(m);
    }
    for prefix in ["NES-", "HVC-", "UNL-", "BMC-", "BTL-"] {
        if let Some(rest) = upper.strip_prefix(prefix)
            && let Some(m) = lookup_board(rest)
        {
            return Some(m);
        }
    }
    None
}

/// Exact (already-uppercased) board-name lookup. Ported from the
/// `UNIF_BOARD_MAP` in `scripts/coverage/coverage.py` (Mesen2 + puNES, checked
/// vs `docs/mappers.md`).
// Arms are grouped by vendor (Nintendo / Konami / Bandai / Sachen / ...) for
// provenance and readability; some distinct board families intentionally share
// a mapper id (e.g. several boards resolve to MMC3 = 4), so identical-body arms
// are deliberately kept separate rather than merged.
// The board table is one large flat match by design (a name→number lookup);
// the v1.8.9 breadth pass pushed it past the default line cap, but splitting it
// would only obscure the per-vendor grouping.
#[allow(clippy::match_same_arms, clippy::too_many_lines)]
fn lookup_board(b: &str) -> Option<u16> {
    Some(match b {
        // Nintendo discrete / first-party
        "NROM" | "NROM-128" | "NROM-256" | "RROM" | "RROM-128" => 0,
        "SLROM" | "SKROM" | "SAROM" | "SBROM" | "SCROM" | "SEROM" | "SFROM" | "SGROM" | "SHROM"
        | "SJROM" | "SKROM-MMC1B2" | "SLROM-MMC1B2" | "SNROM" | "SOROM" | "SUROM" | "SXROM"
        | "SL1ROM" => 1,
        "UNROM" | "UOROM" => 2,
        "UNROM-512-8" | "UNROM-512-16" | "UNROM-512-32" => 30,
        "CNROM" => 3,
        "CPROM" => 13,
        "TLROM" | "TSROM" | "TKROM" | "TKSROM" | "TBROM" | "TFROM" | "TGROM" | "TNROM"
        | "TVROM" | "TEROM" | "B4" | "HKROM" => 4,
        "TLSROM" => 118,
        "TQROM" => 119,
        "TR1ROM" => 64,
        "DRROM" => 206,
        "EKROM" | "ELROM" | "ETROM" | "EWROM" => 5,
        "AMROM" | "ANROM" | "AN1ROM" | "AOROM" => 7,
        "PNROM" | "PEEOROM" => 9,
        "FJROM" | "FKROM" => 10,
        "GNROM" | "MHROM" => 66,
        "BNROM" | "NINA-001" | "NINA-002" => 34,
        "NINA-03" | "NINA-06" => 79,
        "NINA-07" => 11,
        // Konami VRC
        "KONAMI-VRC-1" => 75,
        "KONAMI-VRC-2" => 23,
        "KONAMI-VRC-3" => 73,
        "KONAMI-VRC-4" => 21,
        "KONAMI-VRC-6" => 24,
        "KONAMI-VRC-7" | "VRC7" => 85,
        // Nanjing / Waixing / pirate-ish
        "MMC3" => 4,
        "MAPPER245" => 245,
        // Color Dreams / Wisdom Tree
        "COLORDREAMS" | "CDREAM" => 11,
        // Bandai
        "BANDAI-74*161/161/32" => 152,
        "BANDAI-FCG" | "BANDAI-LZ93D50" | "BANDAI-LZ93D50+24C02" => 16,
        "BANDAI-LZ93D50+24C01" => 159,
        // Sunsoft
        "SUNSOFT_UNROM" => 93,
        "SUNSOFT-1" => 184,
        "SUNSOFT-2" => 89,
        "SUNSOFT-3" => 67,
        "SUNSOFT-4" | "NTBROM" => 68,
        "SUNSOFT-5B" | "SUNSOFT-FME-7" => 69,
        "JF-16" => 78,
        // Irem
        "IREM-G101" => 32,
        "IREM-H3001" => 65,
        "IREM-74*161/161/21/138" => 77,
        "IREM-HOLYDIVER" => 78,
        "HVC-UN1ROM" => 94,
        // Jaleco
        "JALECO-JF-11" | "JALECO-JF-14" => 140,
        "JALECO-JF-13" => 86,
        "JALECO-JF-16" => 78,
        "JALECO-JF-17" => 72,
        "JALECO-JF-19" => 92,
        "JALECO-SS88006" => 18,
        // Namco
        "NAMCOT-3433" | "NAMCOT-3443" | "NAMCOT-3453" => 88,
        "NAMCOT-3446" => 76,
        "NAMCOT-163" => 19,
        "NAMCOT-175" | "NAMCOT-340" => 210,
        // Taito
        "TAITO-TC0190FMC" | "TAITO-TC0190FMR" => 33,
        "TC0190FMC+PAL16R4" => 48,
        "TAITO-X1-005" => 80,
        "TAITO-X1-017" => 82,
        // Camerica / Codemasters
        "CAMERICA-BF9093" | "CAMERICA-ALGN" | "BF9097" => 71,
        "CAMERICA-BF9096" | "CAMERICA-ALGQ" => 232,
        // AVE
        "AVE-NINA-01" | "AVE-NINA-02" => 34,
        "AVE-NINA-03" | "AVE-NINA-06" => 79,
        // Sachen — board-suffix-disambiguated 8259 family (verified vs puNES/Mesen2)
        "SACHEN-8259A" => 141,
        "SACHEN-8259B" => 138,
        "SACHEN-8259C" => 139,
        "SACHEN-8259D" => 137,
        "SACHEN-74LS374N" => 150,
        "SA-016-1M" => 79,
        "SA-72007" => 145,
        "SA-72008" => 133,
        "SA-NROM" => 143,
        "SA-0036" => 149,
        "SA-0037" => 148,
        "TCA01" => 143,
        "TCU01" | "TC-U01-1.5M" => 147,
        // Misc multicarts / homebrew
        "GTROM" | "CHEAPOCABRA" => 111,
        "ACTION52" => 228,
        "CALTRON6IN1" => 41,
        "MAGICFLOOR" => 218,
        "RET-CUFROM" => 29,
        // --- v1.8.9 "Backlog" beta.6 UNIF board-map breadth: well-known board
        // names mapping to families RustyNES already implements. Cross-checked
        // against Mesen2 `UnifLoader.cpp` + FCEUX `unif.cpp`.
        // NTDEC / TXC / discrete BMC (sprint13 + existing families).
        "11160" => 299,
        "N625092" => 221,
        "22211" => 132,
        "43272" | "WAIXING-FW01" => 227,
        "603-5052" => 238,
        "8157" => 301,
        "GK-192" => 58,
        "SC-127" => 35,
        "TEK90" => 90,
        "FS304" => 162,
        "NTD-03" => 290,
        "42IN1RESETSWITCH" => 226,
        "NOVELDIAMOND9999999IN1" => 201,
        // Sachen (TXC protection / 9602 ASIC).
        "SA-002" => 136,
        "SA-9602B" => 513,
        // FK23C / COOLBOY / MINDKIDS reusable-ASIC BMC.
        "FK23C" | "FK23CA" | "SUPER24IN1SC03" => 176,
        "COOLBOY" | "MINDKIDS" => 268,
        // Kaiser FDS-conversion ASIC family.
        "KS7032" => 142,
        "KS7017" => 303,
        "KS7031" => 305,
        "KS7016" => 306,
        "KS7013B" => 312,
        // Unlicensed discrete BMC multicarts.
        "60311C" => 289,
        "810544-C-A1" => 261,
        "830425C-4391T" => 320,
        "830118C" => 348,
        "K-3046" => 336,
        "G-146" => 349,
        "BS-5" => 286,
        _ => return None,
    })
}

/// Parse a UNIF container into a resolved [`UnifImage`].
///
/// # Errors
///
/// Returns [`UnifError`] for a bad magic, a truncated header, a chunk whose
/// declared length overruns the file, a missing `MAPR` chunk, or a board name
/// that does not resolve to a known/implemented mapper. Never panics on
/// malformed input.
pub fn parse_unif(bytes: &[u8]) -> Result<UnifImage, UnifError> {
    if bytes.len() < UNIF_HEADER_LEN {
        return Err(UnifError::HeaderTruncated(bytes.len()));
    }
    let mut magic = [0u8; 4];
    magic.copy_from_slice(&bytes[..4]);
    if &magic != UNIF_MAGIC {
        return Err(UnifError::BadMagic(magic));
    }

    let mut board: Option<String> = None;
    // PRG/CHR banks indexed 0..=15 ('0'..'9','A'..'F'), assembled in order.
    let mut prg_banks: [Option<Vec<u8>>; 16] = Default::default();
    let mut chr_banks: [Option<Vec<u8>>; 16] = Default::default();
    let mut mirroring = Mirroring::Horizontal;
    let mut has_battery = false;
    let mut region = Region::Ntsc;

    let mut off = UNIF_HEADER_LEN;
    while off + 8 <= bytes.len() {
        let id = &bytes[off..off + 4];
        let len = u32::from_le_bytes([
            bytes[off + 4],
            bytes[off + 5],
            bytes[off + 6],
            bytes[off + 7],
        ]) as usize;
        let data_start = off + 8;
        let data_end = data_start.checked_add(len).filter(|&e| e <= bytes.len());
        let Some(data_end) = data_end else {
            return Err(UnifError::ChunkOverrun {
                id: String::from_utf8_lossy(id).into_owned(),
                offset: off,
                len,
            });
        };
        let data = &bytes[data_start..data_end];

        match id {
            b"MAPR" => {
                // NUL-terminated ASCII board name.
                let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
                board = Some(String::from_utf8_lossy(&data[..end]).into_owned());
            }
            b"MIRR" => {
                // 0=H, 1=V, 2=mirror-all (treat as single-screen → H), 3=four
                // screen, 4=four-screen variant. Map conservatively.
                mirroring = match data.first().copied().unwrap_or(0) {
                    1 => Mirroring::Vertical,
                    3 | 4 => Mirroring::FourScreen,
                    _ => Mirroring::Horizontal,
                };
            }
            b"BATR" => has_battery = true,
            b"TVCI" => {
                region = match data.first().copied().unwrap_or(0) {
                    1 => Region::Pal,
                    _ => Region::Ntsc, // 0 = NTSC, 2 = "both" → NTSC
                };
            }
            _ => {
                if id[..3] == *b"PRG"
                    && let Some(slot) = hex_nibble(id[3])
                {
                    prg_banks[slot as usize] = Some(data.to_vec());
                } else if id[..3] == *b"CHR"
                    && let Some(slot) = hex_nibble(id[3])
                {
                    chr_banks[slot as usize] = Some(data.to_vec());
                }
                // Anything else (NAME/DINF/READ/PCK?/CCK?/…) is ignored.
            }
        }
        off = data_end;
    }

    let board = board.ok_or(UnifError::NoMapr)?;
    let mapper_id =
        board_to_mapper(&board).ok_or_else(|| UnifError::UnknownBoard(board.clone()))?;

    let mut prg_rom = Vec::new();
    for bank in prg_banks.into_iter().flatten() {
        prg_rom.extend_from_slice(&bank);
    }
    let mut chr_rom = Vec::new();
    for bank in chr_banks.into_iter().flatten() {
        chr_rom.extend_from_slice(&bank);
    }

    Ok(UnifImage {
        board,
        mapper_id,
        prg_rom,
        chr_rom,
        mirroring,
        has_battery,
        region,
    })
}

/// Synthesize an equivalent **NES 2.0** image from a parsed UNIF, so the
/// standard [`crate::parse`] path builds the [`crate::Cartridge`] + mapper with
/// zero duplicated mapper construction.
///
/// PRG is zero-padded to a 16 KiB multiple and CHR to 8 KiB; empty CHR encodes
/// a CHR-RAM board (8 KiB CHR-RAM via NES 2.0 byte 11). NES 2.0 (not iNES 1.0)
/// is used deliberately: it preserves the region (byte 12) the `TVCI` chunk
/// gave us, and its byte-9 size MSB nibbles represent the large multicart PRG
/// banks (> 255 × 16 KiB) that iNES 1.0 cannot. Every board in the table maps
/// to a mapper id ≤ 255, well within range.
// Every `as u8` below extracts a masked header byte-field (`& 0xFF` / `& 0x0F`),
// so the truncation is the intended field slice, not a lossy cast.
#[allow(clippy::cast_possible_truncation)]
#[must_use]
pub fn unif_to_ines(img: &UnifImage) -> Vec<u8> {
    const PRG_UNIT: usize = 16 * 1024;
    const CHR_UNIT: usize = 8 * 1024;

    let mut prg = img.prg_rom.clone();
    let prg_rem = prg.len() % PRG_UNIT;
    if prg_rem != 0 {
        prg.resize(prg.len() + (PRG_UNIT - prg_rem), 0);
    }
    let mut chr = img.chr_rom.clone();
    let chr_rem = chr.len() % CHR_UNIT;
    if chr_rem != 0 {
        chr.resize(chr.len() + (CHR_UNIT - chr_rem), 0);
    }
    let prg_banks = prg.len() / PRG_UNIT;
    let chr_banks = chr.len() / CHR_UNIT;
    let mapper = img.mapper_id;

    let mirror_bits: u8 = match img.mirroring {
        Mirroring::Vertical => 0x01,
        Mirroring::FourScreen => 0x08,
        _ => 0x00, // Horizontal / single-screen
    };

    let mut h = [0u8; 16];
    h[0..4].copy_from_slice(b"NES\x1A");
    h[4] = (prg_banks & 0xFF) as u8; // PRG size LSB (16 KiB units)
    h[5] = (chr_banks & 0xFF) as u8; // CHR size LSB (8 KiB units)
    h[6] = (((mapper & 0x0F) as u8) << 4) | mirror_bits | (u8::from(img.has_battery) << 1);
    // byte 7: mapper bits 4-7 in the high nibble; bits 2-3 = 0b10 (NES 2.0
    // marker); console type = 0 (NES).
    h[7] = ((((mapper >> 4) & 0x0F) as u8) << 4) | 0x08;
    h[8] = ((mapper >> 8) & 0x0F) as u8; // mapper bits 8-11 (submapper nibble = 0)
    h[9] = (((prg_banks >> 8) & 0x0F) as u8) | ((((chr_banks >> 8) & 0x0F) as u8) << 4);
    // byte 10: PRG-RAM (low nibble) / PRG-NVRAM (high nibble) size shift
    // (size = 64 << shift). NES 2.0 byte 10 is authoritative — the iNES-1.0
    // "default 8 KiB for MMC1/3/5" heuristic does NOT apply to a NES-2.0 image —
    // so declare 8 KiB save/work RAM (64 << 7) unconditionally, as NVRAM when
    // the board is battery-backed and as volatile PRG-RAM otherwise. Mappers
    // that need none simply leave it unused; mappers that need it (MMC1/3/5
    // save data + work RAM) would otherwise get zero and fail to save / boot.
    h[10] = if img.has_battery { 0x70 } else { 0x07 };
    // byte 11: CHR-RAM size shift (size = 64 << shift). 8 KiB = 64 << 7 when the
    // board ships no CHR-ROM; 0 (no CHR-RAM) when it does.
    h[11] = if chr.is_empty() { 0x07 } else { 0x00 };
    h[12] = match img.region {
        Region::Pal => 1,
        Region::Multi => 2,
        Region::Dendy => 3,
        Region::Ntsc => 0,
    };

    let mut out = Vec::with_capacity(16 + prg.len() + chr.len());
    out.extend_from_slice(&h);
    out.extend_from_slice(&prg);
    out.extend_from_slice(&chr);
    out
}

/// Map the trailing byte of a `PRG?`/`CHR?` chunk id (`'0'..='9'`, `'A'..='F'`,
/// `'a'..='f'`) to its bank slot 0..=15. `None` for any other byte.
const fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// Build a synthetic UNIF blob: header + the given chunks (id, data).
    fn build_unif(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(UNIF_MAGIC);
        v.extend_from_slice(&7u32.to_le_bytes()); // revision
        v.extend_from_slice(&[0u8; 24]); // reserved
        for (id, data) in chunks {
            v.extend_from_slice(*id);
            v.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
            v.extend_from_slice(data);
        }
        v
    }

    #[test]
    fn board_resolution_bare_and_prefixed_and_unknown() {
        assert_eq!(board_to_mapper("NROM"), Some(0));
        assert_eq!(board_to_mapper("NES-NROM"), Some(0));
        assert_eq!(board_to_mapper("HVC-SLROM"), Some(1));
        assert_eq!(board_to_mapper("UNL-SACHEN-8259A"), Some(141));
        assert_eq!(board_to_mapper("sachen-8259d"), Some(137)); // case-insensitive
        assert_eq!(board_to_mapper("KONAMI-VRC-2"), Some(23));
        assert_eq!(board_to_mapper("BANDAI-LZ93D50+24C01"), Some(159));
        assert_eq!(board_to_mapper("DEFINITELY-NOT-A-BOARD"), None);
    }

    #[test]
    fn v1_8_9_added_boards_resolve_to_implemented_mappers() {
        // Every board added in the v1.8.9 "Backlog" beta.6 breadth pass must
        // resolve to a family RustyNES implements. Bare + UNL-/BMC-prefixed.
        let cases: &[(&str, u16)] = &[
            // NTDEC / TXC / discrete BMC (sprint13 + existing).
            ("11160", 299),
            ("N625092", 221),
            ("22211", 132),
            ("43272", 227),
            ("WAIXING-FW01", 227),
            ("603-5052", 238),
            ("8157", 301),
            ("GK-192", 58),
            ("SC-127", 35),
            ("TEK90", 90),
            ("FS304", 162),
            ("NTD-03", 290),
            ("42IN1RESETSWITCH", 226),
            ("NOVELDIAMOND9999999IN1", 201),
            // Sachen.
            ("SA-002", 136),
            ("SA-9602B", 513),
            // FK23C / COOLBOY / MINDKIDS.
            ("FK23C", 176),
            ("FK23CA", 176),
            ("SUPER24IN1SC03", 176),
            ("COOLBOY", 268),
            ("MINDKIDS", 268),
            // Kaiser.
            ("KS7032", 142),
            ("KS7017", 303),
            ("KS7031", 305),
            ("KS7016", 306),
            ("KS7013B", 312),
            // Unlicensed discrete BMC.
            ("60311C", 289),
            ("810544-C-A1", 261),
            ("830425C-4391T", 320),
            ("830118C", 348),
            ("K-3046", 336),
            ("G-146", 349),
            ("BS-5", 286),
            // Nintendo discrete aliases newly recognized.
            ("SL1ROM", 1),
            ("TEROM", 4),
            ("NTBROM", 68),
        ];
        for &(board, mapper) in cases {
            assert_eq!(
                board_to_mapper(board),
                Some(mapper),
                "board {board:?} should resolve to mapper {mapper}"
            );
            // The standard UNL- vendor prefix must resolve identically.
            let prefixed = alloc::format!("UNL-{board}");
            assert_eq!(
                board_to_mapper(&prefixed),
                Some(mapper),
                "prefixed board {prefixed:?} should resolve to mapper {mapper}"
            );
        }
    }

    #[test]
    fn sachen_8259_variants_resolve_distinctly() {
        // The one place a naive "8259 -> one mapper" guess is wrong.
        assert_eq!(board_to_mapper("SACHEN-8259A"), Some(141));
        assert_eq!(board_to_mapper("SACHEN-8259B"), Some(138));
        assert_eq!(board_to_mapper("SACHEN-8259C"), Some(139));
        assert_eq!(board_to_mapper("SACHEN-8259D"), Some(137));
    }

    #[test]
    fn parse_minimal_nrom_unif() {
        let prg = vec![0xEAu8; 16 * 1024];
        let chr = vec![0x55u8; 8 * 1024];
        let blob = build_unif(&[
            (b"MAPR", b"NES-NROM\0".to_vec()),
            (b"PRG0", prg.clone()),
            (b"CHR0", chr.clone()),
            (b"MIRR", vec![1]), // vertical
            (b"BATR", vec![]),
        ]);
        let img = parse_unif(&blob).expect("parse");
        assert_eq!(img.board, "NES-NROM");
        assert_eq!(img.mapper_id, 0);
        assert_eq!(img.prg_rom, prg);
        assert_eq!(img.chr_rom, chr);
        assert_eq!(img.mirroring, Mirroring::Vertical);
        assert!(img.has_battery);
        assert_eq!(img.region, Region::Ntsc);
    }

    #[test]
    fn multiple_prg_chr_banks_concatenate_in_index_order() {
        let blob = build_unif(&[
            (b"MAPR", b"UNROM\0".to_vec()),
            (b"PRG1", vec![0x11; 16 * 1024]), // out of order on purpose
            (b"PRG0", vec![0x00; 16 * 1024]),
            (b"CHR0", vec![]),
        ]);
        let img = parse_unif(&blob).expect("parse");
        assert_eq!(img.mapper_id, 2);
        assert_eq!(img.prg_rom.len(), 32 * 1024);
        // PRG0 (0x00) must come before PRG1 (0x11) regardless of file order.
        assert_eq!(img.prg_rom[0], 0x00);
        assert_eq!(img.prg_rom[16 * 1024], 0x11);
        assert!(img.chr_rom.is_empty(), "no CHR banks => CHR-RAM board");
    }

    #[test]
    fn rejects_bad_magic_and_truncation_without_panicking() {
        assert!(matches!(
            parse_unif(&[0u8; 10]),
            Err(UnifError::HeaderTruncated(10))
        ));
        let mut blob = build_unif(&[(b"MAPR", b"NROM\0".to_vec())]);
        blob[..4].copy_from_slice(b"NESM");
        assert!(matches!(parse_unif(&blob), Err(UnifError::BadMagic(_))));
    }

    #[test]
    fn rejects_missing_mapr_and_unknown_board() {
        let no_mapr = build_unif(&[(b"PRG0", vec![0; 16 * 1024])]);
        assert_eq!(parse_unif(&no_mapr), Err(UnifError::NoMapr));
        let unknown = build_unif(&[(b"MAPR", b"WHO-KNOWS\0".to_vec())]);
        assert!(matches!(
            parse_unif(&unknown),
            Err(UnifError::UnknownBoard(_))
        ));
    }

    #[test]
    fn rejects_chunk_length_overrun() {
        let mut blob = build_unif(&[(b"MAPR", b"NROM\0".to_vec())]);
        // Append a PRG0 header claiming a huge length with no data.
        blob.extend_from_slice(b"PRG0");
        blob.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        assert!(matches!(
            parse_unif(&blob),
            Err(UnifError::ChunkOverrun { .. })
        ));
    }

    #[test]
    fn unif_parses_through_the_cartridge_path() {
        // A UNIF blob must load via the top-level `parse()` (UNIF-magic
        // dispatch -> synthesize NES 2.0 -> standard parse) and yield the right
        // Cartridge + a constructed mapper.
        let prg = vec![0xEAu8; 16 * 1024];
        let chr = vec![0x55u8; 8 * 1024];
        let blob = build_unif(&[
            (b"MAPR", b"NES-NROM\0".to_vec()),
            (b"PRG0", prg.clone()),
            (b"CHR0", chr.clone()),
            (b"MIRR", vec![1]), // vertical
            (b"BATR", vec![]),
            (b"TVCI", vec![1]), // PAL
        ]);
        let (cart, _mapper) = crate::parse(&blob).expect("UNIF loads via the cartridge path");
        assert_eq!(cart.mapper_id, 0);
        assert_eq!(&*cart.prg_rom, &prg[..]);
        assert_eq!(&*cart.chr_rom, &chr[..]);
        assert_eq!(cart.mirroring, Mirroring::Vertical);
        assert!(cart.has_battery);
        assert!(cart.is_nes2, "the synthesized image is NES 2.0");
        assert_eq!(
            cart.region,
            Region::Pal,
            "TVCI region survives the synthesis"
        );
    }

    #[test]
    fn unif_nrom_matches_the_equivalent_ines() {
        // The Cartridge a UNIF NROM produces must equal the one the equivalent
        // hand-built iNES NROM produces (same PRG/CHR/mapper/mirroring).
        let prg = vec![0x42u8; 16 * 1024];
        let chr = vec![0x99u8; 8 * 1024];
        let unif = build_unif(&[
            (b"MAPR", b"NROM\0".to_vec()),
            (b"PRG0", prg.clone()),
            (b"CHR0", chr.clone()),
        ]);
        let (uc, _) = crate::parse(&unif).expect("unif");
        // Equivalent iNES 1.0 NROM: 1x16 KiB PRG, 1x8 KiB CHR, mapper 0, horiz.
        let mut ines = vec![b'N', b'E', b'S', 0x1A, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        ines.extend_from_slice(&prg);
        ines.extend_from_slice(&chr);
        let (ic, _) = crate::parse(&ines).expect("ines");
        assert_eq!(uc.mapper_id, ic.mapper_id);
        assert_eq!(uc.prg_rom, ic.prg_rom);
        assert_eq!(uc.chr_rom, ic.chr_rom);
        assert_eq!(uc.mirroring, ic.mirroring);
    }

    #[test]
    fn unif_chr_ram_board_synthesizes_chr_ram() {
        // No CHR chunk => CHR-RAM board: empty CHR-ROM, non-zero CHR-RAM.
        let blob = build_unif(&[
            (b"MAPR", b"UNROM\0".to_vec()),
            (b"PRG0", vec![0u8; 16 * 1024]),
        ]);
        let (cart, _) = crate::parse(&blob).expect("unif");
        assert_eq!(cart.mapper_id, 2);
        assert!(cart.uses_chr_ram(), "no CHR chunk => CHR-RAM board");
        assert!(cart.chr_ram_size >= 8 * 1024, "8 KiB CHR-RAM synthesized");
    }

    #[test]
    fn unif_save_ram_board_synthesizes_prg_ram() {
        // An MMC1 SNROM board with a battery must get PRG-(N)RAM — the NES 2.0
        // byte-10 fix (a save-data board would otherwise get zero PRG-RAM).
        let blob = build_unif(&[
            (b"MAPR", b"SNROM\0".to_vec()),
            (b"PRG0", vec![0u8; 16 * 1024]),
            (b"BATR", vec![]),
        ]);
        let (cart, _) = crate::parse(&blob).expect("unif");
        assert_eq!(cart.mapper_id, 1, "SNROM => MMC1");
        assert!(cart.has_battery);
        assert!(
            cart.prg_ram_size >= 8 * 1024,
            "battery MMC1 board must get >= 8 KiB PRG-RAM, got {}",
            cart.prg_ram_size
        );
    }
}

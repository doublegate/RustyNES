#![allow(clippy::doc_markdown)] // iNES / CRC32 / PRG / CHR / TetaNES are domain terms.

//! Per-game database (v1.1.0 beta.1, Workstream B, T-110-B4).
//!
//! A CRC32-keyed table of per-game corrections, vendored from TetaNES'
//! `game_database.txt` (see `game_database.txt` for the attribution + format).
//! RustyNES currently consumes the **mirroring** column: at ROM load the
//! frontend computes the ROM's CRC32 and, if the DB lists it, applies a
//! nametable-mirroring override via [`rustynes_core::Nes::set_mirroring_override`]
//! — a load-time fix for ROMs whose iNES header carries the wrong mirroring
//! flag. The mapper, submapper and region columns are applied to the header
//! itself before the core parses it ([`load_time_entry`] then
//! [`apply_header_overrides`]); since v2.9.8 the region reaches iNES 1.0 images
//! (by promoting the header to the equivalent NES 2.0 one) and no longer
//! rewrites a NES 2.0 header's own.
//!
//! Every platform applies it the same way since v2.9.8, through [`correct_rom`]
//! (the header, before the core parses it) and [`correct_console`] (the
//! mirroring, on the built console): the desktop and both browser builds, the
//! Android / iOS bridge (`rustynes-mobile`), the libretro core and the coverage
//! harness. Before v2.9.8 the mobile bridge and the libretro core applied none
//! of it. It is still a **host** concern, not a core one: the core test suites
//! (AccuracyCoin, the commercial oracle, `nestest`) construct the `Nes` directly
//! and never consult this DB, so they stay byte-identical. The override is deterministic (same CRC ⇒ same
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

use rustynes_core::rustynes_mappers::{Mirroring, Region};

/// The vendored database text (CRC + correction columns; `#` lines are comments).
const DB_TEXT: &str = include_str!("game_database.txt");

/// A per-game database entry: a set of optional corrections keyed by ROM CRC32.
///
/// Each field is an *override* — `None` means "no correction, use the ROM's iNES
/// header." `mirroring` is applied post-construction via
/// [`rustynes_core::Nes::set_mirroring_override`]; `region` / `mapper` /
/// `submapper` are applied by patching the iNES header bytes before the core
/// parses them (see [`apply_header_overrides`]). This is **frontend-only**: the
/// core test suites construct the `Nes` directly and never consult the DB, so
/// `AccuracyCoin` / the commercial oracle stay byte-identical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameDbEntry {
    /// CRC32 of PRG-ROM + CHR-ROM (header/trainer excluded) — the key.
    pub crc: u32,
    /// Region / timing override (NTSC / PAL / Dendy).
    pub region: Option<Region>,
    /// iNES mapper-id override (for a ROM with a wrong header mapper).
    pub mapper: Option<u16>,
    /// NES 2.0 submapper override.
    pub submapper: Option<u8>,
    /// Nametable-mirroring override.
    pub mirroring: Option<Mirroring>,
    /// Game title (display only).
    pub title: String,
}

/// Parsed vendored table, sorted by CRC for binary search. Built once on first
/// use. Rows with no usable correction field are still kept (title-only) so the
/// editor can show the game name.
fn db() -> &'static [GameDbEntry] {
    static DB: OnceLock<Vec<GameDbEntry>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut rows: Vec<GameDbEntry> = DB_TEXT
            .lines()
            .filter_map(parse_row)
            .map(|mut e| {
                if is_multi_region_title(&e.title) {
                    e.region = None;
                }
                e
            })
            .collect();
        rows.sort_unstable_by_key(|e| e.crc);
        rows.dedup_by_key(|e| e.crc);
        rows.shrink_to_fit();
        rows
    })
}

/// `true` when a vendored row's title names one image released in both a
/// 60 Hz and a 50 Hz market -- the No-Intro tag `(USA, Europe)` and its kin.
///
/// ## Region encoding in the vendored table (v2.9.8)
///
/// The Region column holds only `NTSC` or `PAL` (2,305 and 377 rows); there is
/// no `Dendy` row and no token for "multiple regions", which NES 2.0 has (byte
/// 12 value 2) and which the core plays at NTSC timing. The table still records
/// the release set, in the No-Intro title: nine PAL rows are titled
/// `(USA, Europe)` -- Ice Climber, Gumshoe, Kid Icarus, Baseball and five more.
/// Each is ONE image sold in both markets, so the row's `PAL` is a choice the
/// column was forced to make, not a fact about the image; the NES 2.0 headers on
/// the staged copies of two of them (Ice Climber, Gumshoe) say "multiple
/// region". These nine are the only multi-market titles in the table, and all
/// nine are PAL rows. Honouring the
/// column would play the iNES 1.0 copy of an image at PAL timing and its NES 2.0
/// copy at NTSC. Such a row therefore carries no region correction, and an
/// iNES 1.0 copy keeps the NTSC default -- what "multiple region" means to the
/// core.
///
/// `Dendy` is accepted from the user overlay (`parse_region`) and promoted like
/// PAL; the vendored table never produces it.
fn is_multi_region_title(title: &str) -> bool {
    const SIXTY_HZ: [&str; 3] = ["USA", "Japan", "World"];
    title.split('(').skip(1).any(|group| {
        let tag = group.split(')').next().unwrap_or_default();
        let regions: Vec<&str> = tag.split(',').map(str::trim).collect();
        regions.len() > 1
            && regions.contains(&"Europe")
            && regions.iter().any(|r| SIXTY_HZ.contains(r))
    })
}

/// Map a database region token to a [`Region`], or `None` if unrecognized.
fn parse_region(token: &str) -> Option<Region> {
    match token {
        "NTSC" => Some(Region::Ntsc),
        "PAL" => Some(Region::Pal),
        "Dendy" => Some(Region::Dendy),
        _ => None,
    }
}

/// Map a database mirroring token to a [`Mirroring`], or `None` if unrecognized
/// (e.g. mapper-controlled rows that carry no usable static override).
fn parse_mirroring(token: &str) -> Option<Mirroring> {
    match token {
        "Horizontal" => Some(Mirroring::Horizontal),
        "Vertical" => Some(Mirroring::Vertical),
        "FourScreen" => Some(Mirroring::FourScreen),
        "SingleScreenA" => Some(Mirroring::SingleScreenA),
        "SingleScreenB" => Some(Mirroring::SingleScreenB),
        _ => None,
    }
}

/// Parse one `CRC, Region, Mapper, Sub-Mapper, ChrBanks, PrgRomBanks,
/// PrgRamBanks, Battery, Mirroring, Title` row into a [`GameDbEntry`]. Returns
/// `None` for comment / blank / malformed lines. The title is the final field
/// and may contain commas (it is split off with `splitn`).
fn parse_row(line: &str) -> Option<GameDbEntry> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    // 10 columns; the 10th (title) keeps any embedded commas.
    let fields: Vec<&str> = line.splitn(10, ',').map(str::trim).collect();
    if fields.len() < 9 {
        return None;
    }
    let crc = u32::from_str_radix(fields[0], 16).ok()?;
    let region = parse_region(fields[1]);
    // A Mapper column of `0` means "unspecified", NOT "force NROM".
    //
    // The vendored table carries `0` as its default for rows nobody filled in,
    // and there is no separate empty marker, so a genuine "this ROM really is
    // NROM" correction is indistinguishable from an unfilled field. Applying it
    // is therefore never safe: on a ROM whose header already says 0 the override
    // is a no-op, and on any other ROM it DESTROYS a correct header.
    //
    // That destruction was live. Measured over the staged corpus: 12 ROMs -- every
    // Sachen board in it (133, 143, 145, 146, 147, 148, 149, 150) -- were being
    // rewritten to mapper 0 and then rejected by NROM's size check, e.g.
    // `Sidewinder` (SA-72007, mapper 145) whose row reads
    // `80D63472, PAL, 0, 0, ...`. It reached users, not just the harness: the
    // frontend has called `apply_header_overrides` since v1.2.0. It surfaced only
    // when v2.3.4 put the same load path under the coverage sweep.
    //
    // This is the second time this vendored table has force-applied a field it
    // should not have; the first was the mirroring column freezing Wizards &
    // Warriors (ADR 0031), fixed the same way -- by refusing to apply an override
    // that cannot be distinguished from "no data".
    let mapper = fields[2].parse::<u16>().ok().filter(|&m| m != 0);
    let submapper = fields[3].parse::<u8>().ok();
    let mirroring = parse_mirroring(fields[8]);
    let title = fields
        .get(9)
        .map(|t| t.trim_matches('"').to_string())
        .unwrap_or_default();
    Some(GameDbEntry {
        crc,
        region,
        mapper,
        submapper,
        mirroring,
        title,
    })
}

/// Look up a ROM's effective database entry by CRC32 — the **user overlay**
/// (editable, persisted via the in-app ROM-DB editor) takes precedence over the
/// vendored base; `None` if listed in neither.
///
/// Returns an owned entry because the user overlay is mutable (behind a
/// `RwLock`). The vendored base alone is reachable via [`vendored_entry`].
#[must_use]
pub fn entry_for_crc(crc: u32) -> Option<GameDbEntry> {
    if let Ok(overlay) = user_overlay().read()
        && let Ok(i) = overlay.binary_search_by_key(&crc, |e| e.crc)
    {
        return Some(overlay[i].clone());
    }
    vendored_entry(crc).cloned()
}

/// The entry the **load path** applies to a ROM whose header-excluded CRC32 is
/// `crc` and whose 16-byte header is `header`.
///
/// The user overlay is returned as it is: an entry the user wrote is a
/// deliberate correction of *this* image. A **vendored** row, though, is applied
/// to a NES 2.0 header without its `mapper` and `submapper` columns.
///
/// ## Why a NES 2.0 header wins over the vendored table
///
/// The vendored table was compiled for iNES 1.0 images, whose mapper byte is
/// often wrong and which have no submapper at all; that is the gap it fills
/// (Seicross needs mapper 185 submapper 4, and its GoodNES dump cannot say so).
/// It also records, for many boards, a *compatible* mapper rather than the real
/// one -- mapper 140 as 66, 150 as 243, 152 as 70, 159 as 16 -- which is
/// harmless only where the substitute decodes the registers the game writes. A
/// NES 2.0 header is a later, deliberate statement about the image, and on the
/// staged corpus every NES 2.0 dump the table rewrote either rendered the same
/// or rendered correctly only with its own header. Measured at v2.9.8 over the
/// 32 such dumps: 20 rendered identically, 2 differed only in a blinking prompt,
/// and in the other 10 the NES 2.0 header was right and the table wrong. Youkai
/// Club writes its bank register at `$6000`,
/// which mapper 66 does not decode, so it stalled on a blue screen; the Sachen
/// lightgun 2-in-1 rendered garbage as 243 and its title as 150; the three
/// mapper-159 Bandai games rendered a grey screen as 16; Gegege no Kitarou 2,
/// Saint Seiya, Bakushou!! Jinsei Gekijou 3, Fan Kong Jing Ying and
/// Mississippi Satsujin Jiken rendered wrong or blank under the substitute.
///
/// The mapper, submapper and (v2.9.8) region are withheld. The mirroring column
/// is unchanged here (mirroring is applied separately, and only to a board with
/// hardwired mirroring).
///
/// ## Region (v2.9.8)
///
/// The region follows the same rule: a NES 2.0 header states it (byte 12), so a
/// vendored row never rewrites it, while an iNES 1.0 header has no region the
/// core reads and takes the row's. Before v2.9.8 it ran the other way round on
/// both counts -- the row DID rewrite NES 2.0 byte 12, and for iNES 1.0 it wrote
/// only byte 9 bit 0, which the core ignores, so the row reached no iNES 1.0
/// game at all. Measured on the staged corpus, the NES 2.0 half retimed eight
/// dumps: Funblaster Pak (Australia), whose header says PAL, was forced to
/// NTSC; two Chinese titles whose headers say Dendy were forced to NTSC; and
/// five whose headers say "multiple region" were forced to PAL. The iNES 1.0
/// half now promotes the header so the row arrives (see
/// [`apply_header_overrides`]).
#[must_use]
pub fn load_time_entry(crc: u32, header: &[u8]) -> Option<GameDbEntry> {
    if let Ok(overlay) = user_overlay().read()
        && let Ok(i) = overlay.binary_search_by_key(&crc, |e| e.crc)
    {
        return Some(overlay[i].clone());
    }
    let mut entry = vendored_entry(crc)?.clone();
    let is_nes2 = header.len() >= 16 && &header[0..4] == b"NES\x1A" && (header[7] & 0x0C) == 0x08;
    if is_nes2 {
        entry.mapper = None;
        entry.submapper = None;
        entry.region = None;
    }
    Some(entry)
}

/// Look up a ROM's entry in the **vendored base only** (ignoring the user
/// overlay) — used by the editor to show "reset to default".
#[must_use]
pub fn vendored_entry(crc: u32) -> Option<&'static GameDbEntry> {
    let db = db();
    db.binary_search_by_key(&crc, |e| e.crc)
        .ok()
        .map(|i| &db[i])
}

/// Look up a ROM's mirroring correction by CRC32, or `None` if not listed (or
/// listed without a usable mirroring override). Thin wrapper over
/// [`entry_for_crc`], kept for the existing load path.
#[must_use]
pub fn mirroring_for_crc(crc: u32) -> Option<Mirroring> {
    entry_for_crc(crc).and_then(|e| e.mirroring)
}

// ---------------------------------------------------------------------------
// User overlay (v1.2.0 Workstream B) — editable per-game corrections persisted
// to the data dir, overriding the vendored base by CRC. Behind a `RwLock` so
// the in-app ROM-DB editor can refresh it live. The core test suites never
// touch this (frontend-only), so the determinism firewall holds.
// ---------------------------------------------------------------------------

/// Data directory the user overlay is read from, if an application set one.
///
/// v2.3.4 — injected rather than derived. This crate was extracted from
/// `rustynes-frontend`, where the path came straight from
/// `config::Config::default_data_dir()`. A library that reaches for the running
/// user's config directory cannot be linked into a test harness without making
/// its results depend on whatever that developer happens to have saved
/// locally, so the application now supplies the directory and anything that
/// does not (the coverage harness) simply gets no overlay.
static OVERLAY_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Point the user overlay at `dir`. Call once, at application start.
///
/// Returns `false` if a directory was already set (the first call wins), so a
/// late second caller cannot silently change which corrections are in force.
///
/// # Call this BEFORE the first lookup
///
/// The overlay file is read once, lazily, on the first [`entry_for_crc`] /
/// [`mirroring_for_crc`] call, and cached for the life of the process. Setting
/// the directory after that point returns `true` -- the `OnceLock` really was
/// empty -- while having no effect on anything resolved, because the overlay is
/// already loaded. There is no error to observe. `App::new` therefore calls this
/// before it applies any header override, and the contract is pinned by
/// `tests/overlay_dir.rs`.
pub fn set_overlay_dir(dir: std::path::PathBuf) -> bool {
    OVERLAY_DIR.set(dir).is_ok()
}

/// Path to the user-overlay file (`game_db_user.txt` in the data dir).
///
/// `None` when no application set a directory — which is the correct answer for
/// a test harness: the vendored table only, reproducibly.
fn overlay_path() -> Option<std::path::PathBuf> {
    OVERLAY_DIR.get().map(|d| d.join("game_db_user.txt"))
}

/// The lazily-loaded, live-editable user overlay (sorted by CRC).
fn user_overlay() -> &'static std::sync::RwLock<Vec<GameDbEntry>> {
    static OVERLAY: OnceLock<std::sync::RwLock<Vec<GameDbEntry>>> = OnceLock::new();
    OVERLAY.get_or_init(|| std::sync::RwLock::new(load_overlay()))
}

fn load_overlay() -> Vec<GameDbEntry> {
    let Some(path) = overlay_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut rows: Vec<GameDbEntry> = text.lines().filter_map(parse_row).collect();
    rows.sort_unstable_by_key(|e| e.crc);
    rows.dedup_by_key(|e| e.crc);
    rows
}

/// Insert or replace a user-overlay entry and persist the overlay to disk.
///
/// # Errors
///
/// Returns the underlying I/O error if the overlay file can't be written.
pub fn upsert_user_entry(entry: GameDbEntry) -> std::io::Result<()> {
    {
        let mut ov = user_overlay()
            .write()
            .expect("game-db overlay lock poisoned");
        match ov.binary_search_by_key(&entry.crc, |e| e.crc) {
            Ok(i) => ov[i] = entry,
            Err(i) => ov.insert(i, entry),
        }
    }
    persist_overlay()
}

/// Remove a user-overlay entry (reverting to the vendored base) and persist.
///
/// # Errors
///
/// Returns the underlying I/O error if the overlay file can't be written.
pub fn remove_user_entry(crc: u32) -> std::io::Result<()> {
    {
        let mut ov = user_overlay()
            .write()
            .expect("game-db overlay lock poisoned");
        if let Ok(i) = ov.binary_search_by_key(&crc, |e| e.crc) {
            ov.remove(i);
        }
    }
    persist_overlay()
}

fn persist_overlay() -> std::io::Result<()> {
    let Some(path) = overlay_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Snapshot the rows under a minimal lock, then serialize + write lock-free.
    let entries: Vec<GameDbEntry> = user_overlay()
        .read()
        .expect("game-db overlay lock poisoned")
        .clone();
    let mut out = String::from(
        "# RustyNES per-game user overrides (v1.2.0). Edited via Tools -> ROM Database.\n\
         # Columns: CRC, Region, Mapper, Sub-Mapper, ChrBanks, PrgRomBanks, PrgRamBanks, \
         Battery, Mirroring, Title\n",
    );
    for e in &entries {
        out.push_str(&serialize_row(e));
        out.push('\n');
    }
    // Atomic write: serialize to a sibling temp file, then rename over the
    // target. A crash/kill mid-write can't truncate or corrupt the user overlay
    // (rename is atomic on the same filesystem) — Gemini review, PR #74.
    let tmp = path.with_extension("txt.tmp");
    std::fs::write(&tmp, out)?;
    std::fs::rename(&tmp, &path)
}

/// Serialize a [`GameDbEntry`] back to the 10-column row format `parse_row`
/// reads (unused columns left empty). Round-trips through `parse_row`.
fn serialize_row(e: &GameDbEntry) -> String {
    let region = match e.region {
        Some(Region::Ntsc) => "NTSC",
        Some(Region::Pal) => "PAL",
        Some(Region::Dendy) => "Dendy",
        _ => "",
    };
    let mapper = e.mapper.map(|m| m.to_string()).unwrap_or_default();
    let sub = e.submapper.map(|s| s.to_string()).unwrap_or_default();
    let mirroring = match e.mirroring {
        Some(Mirroring::Horizontal) => "Horizontal",
        Some(Mirroring::Vertical) => "Vertical",
        Some(Mirroring::FourScreen) => "FourScreen",
        Some(Mirroring::SingleScreenA) => "SingleScreenA",
        Some(Mirroring::SingleScreenB) => "SingleScreenB",
        _ => "",
    };
    format!(
        "{:08X}, {region}, {mapper}, {sub}, , , , , {mirroring}, \"{}\"",
        e.crc, e.title
    )
}

/// Apply a database entry's `region` / `mapper` / `submapper` corrections.
///
/// Rewrites the iNES (or NES 2.0) header bytes of `bytes` in place, *before* the
/// core parses them. Mirroring is **not** applied here — it goes through the
/// post-construction [`rustynes_core::Nes::set_mirroring_override`] setter.
///
/// This keeps the determinism firewall intact: only the frontend patches the
/// header, the core sees a normal iNES image, and the CRC key (PRG+CHR, header
/// excluded) is unchanged so the lookup is stable across the patch.
///
/// A region reaches the core through the header too (v2.9.8). On a NES 2.0
/// header it is byte 12. On an iNES 1.0 header a PAL or Dendy region rewrites
/// the header as the NES 2.0 header of the same board, region in byte 12 -- see
/// `ines1_as_nes2` for why that, and not byte 9, and how "the same board" is
/// checked. Because the correction lives in the bytes, every load path that
/// runs this function (desktop, browser, the coverage harness) and everything
/// that later rebuilds from those bytes (a power cycle re-parses them) sees it.
///
/// Only the 16-byte header is ever rewritten, and the ROM identity that names
/// the frontend's save-state directory and `.sav` file (`Nes::rom_sha256`)
/// leaves the header out (v2.9.8), so no correction here renames a game's
/// saves. The Vs. System database matches that same identity first (its
/// whole-image key, `Nes::image_sha256`, is only the fallback), so a header
/// correction cannot cost a Vs. cart its palette or DIP row either; and Vs.
/// carts are never promoted.
///
/// Returns `true` if any byte was changed (the caller may want to log it).
pub fn apply_header_overrides(bytes: &mut [u8], entry: &GameDbEntry) -> bool {
    if bytes.len() < 16 || &bytes[0..4] != b"NES\x1A" {
        return false;
    }
    let mut is_nes2 = (bytes[7] & 0x0C) == 0x08;
    let mut changed = false;

    if let Some(mapper) = entry.mapper {
        // iNES: low nibble in flags6[7:4], high nibble in flags7[7:4].
        // NES 2.0 adds bits 8-11 in byte 8[3:0].
        let lo = (mapper & 0x0F) as u8;
        let mid = ((mapper >> 4) & 0x0F) as u8;
        let new6 = (bytes[6] & 0x0F) | (lo << 4);
        let new7 = (bytes[7] & 0x0F) | (mid << 4);
        if new6 != bytes[6] || new7 != bytes[7] {
            bytes[6] = new6;
            bytes[7] = new7;
            changed = true;
        }
        if is_nes2 {
            let hi = ((mapper >> 8) & 0x0F) as u8;
            let new8 = (bytes[8] & 0xF0) | hi;
            if new8 != bytes[8] {
                bytes[8] = new8;
                changed = true;
            }
        }
    }

    if let Some(submapper) = entry.submapper
        && submapper != 0
    {
        // The submapper field only exists in NES 2.0 headers. A correction that
        // sets a non-zero submapper on an iNES-1.0 ROM (e.g. Seicross, whose
        // GoodNES dump is iNES-1.0 mapper 185 sub 0 but is really sub 4 — the
        // CHR-disable copy-protection variant) must first PROMOTE the header to
        // NES 2.0 by setting byte-7 bits 2-3 to `10`, otherwise the core's iNES
        // path forces submapper 0 and the override is silently dropped.
        if !is_nes2 {
            bytes[7] = (bytes[7] & 0xF3) | 0x08;
            // Promotion reinterprets header fields the iNES-1.0 spec leaves
            // undefined: byte-8 low nibble becomes mapper bits 8-11, and bytes
            // 9-15 become NES-2.0 fields (PRG/CHR-RAM sizes, sub-mapper, CPU/PPU
            // timing, etc.). Legacy dumps routinely carry garbage there, so
            // sanitize before the core parses the now-NES-2.0 header — otherwise
            // a stray byte-8 low nibble would silently change the mapper number
            // (e.g. push mapper 185 to 185 + (garbage << 8)) and stale bytes
            // 9-15 would fabricate RAM/timing fields. The mapper high nibble is
            // already 0 for every mapper this promotion path targets (< 256).
            bytes[8] &= 0xF0;
            bytes[9..16].fill(0);
            is_nes2 = true;
            changed = true;
        }
        let new8 = (bytes[8] & 0x0F) | ((submapper & 0x0F) << 4);
        if new8 != bytes[8] {
            bytes[8] = new8;
            changed = true;
        }
    }

    if let Some(region) = entry.region {
        if is_nes2 {
            // NES 2.0 region is byte 12, low two bits (0 = NTSC, 1 = PAL,
            // 2 = multi, 3 = Dendy).
            let code = match region {
                Region::Pal => 1,
                Region::Dendy => 3,
                _ => 0,
            };
            let new12 = (bytes[12] & 0xFC) | code;
            if new12 != bytes[12] {
                bytes[12] = new12;
                changed = true;
            }
        } else if let Some(header) = (region != Region::Ntsc)
            .then(|| ines1_as_nes2(bytes, region))
            .flatten()
        {
            // iNES 1.0 has no region field the core reads: byte 9 bit 0 is the
            // TV-system flag on paper, but raw dumps carry junk there, so the
            // header parser ignores it (`docs/cartridge-format.md`). A non-NTSC
            // region therefore travels the only way the core can see it: the
            // header is rewritten as the NES 2.0 header that describes the SAME
            // board, with the region in byte 12. `ines1_as_nes2` builds that
            // header and proves the board is the same before returning it.
            bytes[..16].copy_from_slice(&header);
            changed = true;
        } else {
            // NTSC (the iNES 1.0 default, so nothing to promote), or a PAL /
            // Dendy region whose NES 2.0 form would not build the same board
            // (`ines1_as_nes2` returned `None`; an arcade cart, or a header too
            // short to parse). Keep the pre-v2.9.8 write of the iNES 1.0
            // TV-system flag, byte 9 bit 0: the core ignores it, so the image
            // runs at NTSC timing, and the bytes -- and so the ROM hash that
            // keys save states and `.sav` files -- stay what they were.
            let bit = u8::from(matches!(region, Region::Pal | Region::Dendy));
            let new9 = (bytes[9] & 0xFE) | bit;
            if new9 != bytes[9] {
                bytes[9] = new9;
                changed = true;
            }
        }
    }

    changed
}

/// The NES 2.0 header that describes the same board as the iNES 1.0 image
/// `bytes`, with `region` in byte 12 -- or `None` when no such header exists.
///
/// ## Why promote, rather than read byte 9
///
/// iNES 1.0's only region signal is byte 9 bit 0, and the core deliberately
/// ignores it: the old dump tools that wrote "DiskDude!" over bytes 7-15 set it
/// (`'s'` is `0x73`), and clean headers carry it with nothing to say whether it
/// is meant -- on the staged corpus three pirate multicart images the table
/// does not list (mapper 60's 4-in-1, mapper 133's 21-in-1, mapper 200's
/// 7-in-1) have a clean header with `0x01` there. Honouring the bit would
/// retime every such dump on a guess. The game
/// database is the source that *does* know, so it states the region the way
/// the core already reads one: NES 2.0 byte 12.
///
/// ## Why the result is verified, not just encoded
///
/// NES 2.0 states what iNES 1.0 leaves to heuristics -- PRG-RAM and CHR-RAM
/// sizes -- and the mapper crate reads a few fields differently depending on
/// which format it parsed. iNES 1.0 reports 8 KiB of PRG-RAM for every image;
/// the v2.9.6 MMC3 multicart boards (37, 45, 47, 12, ...) ignore that and take
/// their own default, because a multicart keeps a register where the RAM would
/// be, while a NES 2.0 header's stated size is taken at its word. A promotion
/// that wrote "8 KiB" would put RAM on Super Mario Bros. + Tetris + Nintendo
/// World Cup's board, where the real cart has none.
///
/// So the candidate headers are tried in order, each is parsed into a board,
/// and the first whose board is indistinguishable from the iNES 1.0 one is
/// returned: same cartridge identity, mirroring, console type and battery, and a
/// mapper with the same serialized state, debug view, capability flags and RAM
/// sizes (see `same_board`). The candidates are:
///
/// 1. the iNES 1.0 heuristics written out -- 8 KiB of PRG-RAM (battery-backed
///    when byte 6 says so) and 8 KiB of CHR-RAM when there is no CHR-ROM;
/// 2. no RAM stated at all, which is what makes a board fall back to its own
///    default, as the iNES 1.0 path does for the MMC3 multicarts.
///
/// If neither builds the same board the region is refused rather than bought
/// with a different board. Measured over every mapper id the crate builds by
/// `promotion_never_changes_the_board` in this file.
///
/// Vs. System and PlayChoice-10 carts are refused outright: those cabinets are
/// NTSC hardware (their RGB PPUs exist only in NTSC timing), so a PAL row on one
/// describes the home release that shares its PRG/CHR, not the arcade board.
/// PlayChoice-10 Baseball is the staged case.
fn ines1_as_nes2(bytes: &[u8], region: Region) -> Option<[u8; 16]> {
    use rustynes_core::rustynes_mappers::{ConsoleType, parse, parse_header};

    // NES 2.0 RAM sizes are `64 << shift` bytes; 8 KiB is shift 7.
    const SHIFT_8K: u8 = 7;

    let h = parse_header(bytes).ok()?;
    if h.is_nes2 {
        return None;
    }
    let original = parse(bytes).ok()?;
    if original.0.console_type != ConsoleType::Nes {
        return None;
    }
    let region_code = match region {
        Region::Pal => 1,
        Region::Multi => 2,
        Region::Dendy => 3,
        Region::Ntsc => 0,
    };

    // The fields every candidate shares. The mapper number is the one the
    // iNES 1.0 parse settled on -- after its dirty-tail rule -- written cleanly;
    // iNES 1.0 has 8 mapper bits, so NES 2.0's bits 8-11 (byte 8) are zero, and
    // so is the submapper. Byte 9 (the size MSBs) is zero because iNES 1.0 sizes
    // are the plain byte-4/5 counts. Bytes 13-15 (Vs. type, misc ROMs, default
    // expansion device) are zero: an iNES 1.0 home cart has none of them.
    let mut header = [0u8; 16];
    header[..4].copy_from_slice(&bytes[..4]);
    header[4] = bytes[4];
    header[5] = bytes[5];
    #[allow(clippy::cast_possible_truncation)] // iNES 1.0 mapper ids are < 256
    let mapper = h.mapper_id as u8;
    header[6] = ((mapper & 0x0F) << 4) | (bytes[6] & 0x0F);
    header[7] = (mapper & 0xF0) | 0x08;
    header[12] = region_code;

    // Byte 10's low nibble is volatile PRG-RAM and its high nibble PRG-NVRAM;
    // the boards allocate the two together (`Header::prg_ram_window`), so
    // either place gives the same window, and the
    // battery flag (byte 6 bit 1, carried over above) is what the boards read.
    let prg_ram = if h.has_battery {
        SHIFT_8K << 4
    } else {
        SHIFT_8K
    };
    let chr_ram = if h.chr_size == 0 { SHIFT_8K } else { 0 };

    for (byte10, byte11) in [(prg_ram, chr_ram), (0, 0)] {
        header[10] = byte10;
        header[11] = byte11;
        let mut image = bytes.to_vec();
        image[..16].copy_from_slice(&header);
        if let Ok(promoted) = parse(&image)
            && same_board(&original, &promoted)
        {
            return Some(header);
        }
    }
    None
}

/// `true` when two parses of one ROM built boards no game could tell apart.
///
/// Compares everything the core takes from a parse except the fields a
/// promotion is *meant* to change (`region`, `is_nes2`) and the two RAM sizes
/// the cartridge record carries for display only (the mapper's own RAM is
/// compared through its state instead). The mapper comparison is by observable
/// output: the serialized state holds the registers, every RAM the board
/// allocated and the variant fields that select behaviour (a submapper, say),
/// and the debug view, capability flags and mirroring answers cover the rest.
fn same_board(
    a: &(
        rustynes_core::rustynes_mappers::Cartridge,
        Box<dyn rustynes_core::rustynes_mappers::Mapper>,
    ),
    b: &(
        rustynes_core::rustynes_mappers::Cartridge,
        Box<dyn rustynes_core::rustynes_mappers::Mapper>,
    ),
) -> bool {
    let ((ca, ma), (cb, mb)) = (a, b);
    ca.mapper_id == cb.mapper_id
        && ca.submapper == cb.submapper
        && ca.mirroring == cb.mirroring
        && ca.console_type == cb.console_type
        && ca.vs_ppu_type == cb.vs_ppu_type
        && ca.vs_dual_system == cb.vs_dual_system
        && ca.has_battery == cb.has_battery
        && ca.has_trainer == cb.has_trainer
        && ca.prg_rom == cb.prg_rom
        && ca.chr_rom == cb.chr_rom
        && ma.caps() == mb.caps()
        && ma.has_hardwired_mirroring() == mb.has_hardwired_mirroring()
        && ma.current_mirroring() == mb.current_mirroring()
        && ma.sram().len() == mb.sram().len()
        && ma.save_data().len() == mb.save_data().len()
        && ma.save_state() == mb.save_state()
        && format!("{:?}", ma.debug_info()) == format!("{:?}", mb.debug_info())
}

/// Stage one of the load-time correction path every platform shares (v2.9.8):
/// rewrite `bytes`' header with the database's region / mapper / submapper
/// corrections, before the core parses it.
///
/// Returns the header-excluded CRC32 the lookup used -- the key for stage two
/// ([`correct_console`]) and for any correction a host stacks on top (the
/// desktop's per-game `<rom>.json` overlay) -- or `None` when `bytes` is not an
/// iNES image. `None` is not an error: an FDS disk (`FDS\x1A`) or an NSF
/// (`NESM\x1A`) has no header to correct and loads through its own
/// constructor, and a malformed cartridge is reported by the core's parser.
///
/// ## One path, every platform
///
/// The desktop, the browser (both web builds), the Android and iOS bridge
/// (`rustynes-mobile`), the libretro core and the coverage harness all call
/// this function and [`correct_console`], in that order: correct the bytes,
/// build the console from them, correct the console. Before v2.9.8 only the
/// desktop, the browser and the harness corrected anything; the mobile bridge
/// and the libretro core handed the raw image to the core, so every database
/// fix -- Seicross's submapper, the region promotion, the NES 2.0 guard in
/// [`load_time_entry`] -- was absent there. That was the fourth load path to
/// miss the corrections (the CLI, the harness and the browser were the first
/// three), which is why the two stages now live here rather than in each host.
///
/// The lookup is [`load_time_entry`]: the user overlay when a host configured
/// one ([`set_overlay_dir`]; only the desktop does), else the vendored table
/// with its NES 2.0 guard. Idempotent: a second pass over corrected bytes finds
/// the same CRC and changes nothing, because the corrected header is either the
/// same iNES 1.0 header or a NES 2.0 one the guard leaves alone.
pub fn correct_rom(bytes: &mut [u8]) -> Option<u32> {
    let crc = rom_crc32(bytes)?;
    if let Some(entry) = load_time_entry(crc, bytes) {
        apply_header_overrides(bytes, &entry);
    }
    Some(crc)
}

/// Stage two of the shared load-time correction path (v2.9.8).
///
/// Applies the database's nametable-mirroring correction to a console built
/// from bytes that went through [`correct_rom`], whose returned CRC is `crc`.
///
/// Mirroring is not a header field the database rewrites; it is a
/// post-construction override ([`rustynes_core::Nes::set_mirroring_override`]),
/// and only on a board whose mirroring is **hardwired**. Forcing a static
/// mirroring onto a mapper that switches its own (MMC1/3/5, `AxROM`, VRC, ...)
/// corrupts its rendering -- Wizards & Warriors' row froze the game (ADR 0031)
/// -- so the guard is the load-bearing half, not the lookup.
///
/// Returns `true` when an override was applied. A host running a Vs.
/// `DualSystem` cabinet calls it on each of the two consoles (v2.9.8; until
/// then no host corrected a cabinet's consoles at all).
pub fn correct_console(nes: &mut rustynes_core::Nes, crc: u32) -> bool {
    if let Some(m) = mirroring_for_crc(crc)
        && nes.mapper_has_hardwired_mirroring()
    {
        nes.set_mirroring_override(Some(m));
        return true;
    }
    false
}

/// Test support for the hosts' load-path tests (feature `test-support`):
/// build an iNES image whose header-excluded CRC32 is a chosen database key.
///
/// Every row of the vendored table names a commercial dump, which is never
/// committed, so a test that wants to see a correction arrive through a host's
/// load path needs bytes the database matches without the dump. CRC32 is
/// affine over GF(2), so four chosen bytes at the end of the body reach any
/// CRC; the rest of the image is zero.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    /// The vendored row the hosts' load-path tests use.
    ///
    /// `51C51C35, PAL, 3, 0, 4, 2, 0, false, Vertical, "Gradius (Europe).nes"`.
    /// It carries one correction of each kind the shared path applies: a
    /// mapper (3, CNROM), a region (PAL, which on an iNES 1.0 image travels as
    /// a promotion to NES 2.0) and a hardwired mirroring (vertical).
    pub const GRADIUS_EUROPE_CRC: u32 = 0x51C5_1C35;

    /// An iNES 1.0 image the database matches as Gradius (Europe).
    ///
    /// Every field the row corrects is stated wrongly: mapper 0, horizontal mirroring,
    /// and no region the core reads. 32 KiB PRG + 32 KiB CHR, which NROM
    /// refuses (it takes at most 8 KiB of CHR-ROM), so an uncorrected load
    /// fails outright and a corrected one builds CNROM.
    #[must_use]
    pub fn gradius_europe_with_a_wrong_header() -> Vec<u8> {
        let mut header = [0u8; 16];
        header[..4].copy_from_slice(b"NES\x1A");
        header[4] = 2; // 2 x 16 KiB PRG-ROM
        header[5] = 4; // 4 x 8 KiB CHR-ROM
        image_with_crc(header, GRADIUS_EUROPE_CRC)
    }

    /// An image with `header` (16 bytes; the PRG / CHR counts in bytes 4 and 5
    /// set the body length) whose header-excluded CRC32 is `target_crc`.
    ///
    /// # Panics
    ///
    /// If the header declares no PRG-ROM (the CRC is then undefined), or if the
    /// forged CRC does not come out as `target_crc` (checked, not assumed).
    #[must_use]
    pub fn image_with_crc(header: [u8; 16], target_crc: u32) -> Vec<u8> {
        let body = usize::from(header[4]) * 16 * 1024 + usize::from(header[5]) * 8 * 1024;
        assert!(body >= 4, "the header declares no PRG-ROM");
        let mut image = header.to_vec();
        image.resize(16 + body, 0);
        // The register after the prefix (everything but the last four bytes),
        // before the final inversion.
        let mut prefix = 0xFFFF_FFFFu32;
        for &b in &image[16..16 + body - 4] {
            prefix = step(prefix, b);
        }
        // Walk the wanted final register back four bytes. Each table entry's
        // top byte is unique, so the register's top byte names the index the
        // forward step used; the index goes in the low byte of the earlier
        // register, which is then the bytes XORed with the prefix register.
        let mut reg = !target_crc;
        for _ in 0..4 {
            let idx = (0..=255u32)
                .find(|&i| table(i) >> 24 == reg >> 24)
                .expect("the CRC-32 table's top bytes are a permutation");
            reg = ((reg ^ table(idx)) << 8) | idx;
        }
        image[16 + body - 4..].copy_from_slice(&(reg ^ prefix).to_le_bytes());
        assert_eq!(
            super::rom_crc32(&image),
            Some(target_crc),
            "CRC forging failed"
        );
        image
    }

    /// One table entry of the reflected CRC-32 (polynomial `0xEDB8_8320`).
    fn table(i: u32) -> u32 {
        let mut c = i;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
        c
    }

    /// One forward byte step of the reflected CRC-32 register.
    fn step(reg: u32, byte: u8) -> u32 {
        (reg >> 8) ^ table((reg ^ u32::from(byte)) & 0xFF)
    }
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

/// v2.1.3 — compute the **full-file** CRC32 of an iNES image: the CRC32 of the
/// whole `.nes` file **including** the 16-byte header (and any trainer).
///
/// This is the **No-Intro / libretro** convention — the key their DAT / cheat
/// databases index by. It differs from [`rom_crc32`] (which excludes the
/// header), so the Game Genie picklist tries BOTH keys: the header-excluded key
/// matches the curated starter rows, and this full-file key matches the
/// thousands of dump variants ingested from libretro-database. Returns `None`
/// when the bytes are not a plausible iNES image — it applies the SAME
/// header-declared-size plausibility checks as [`rom_crc32`] (magic, non-zero
/// PRG, and a file long enough for header + trainer + PRG + CHR), so a truncated
/// or impossibly-sized file does not get a CRC that could spuriously match the
/// cheat database.
#[must_use]
pub fn rom_crc32_full(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 16 || &bytes[0..4] != b"NES\x1A" {
        return None;
    }
    let prg = (bytes[4] as usize) * 16 * 1024;
    let chr = (bytes[5] as usize) * 8 * 1024;
    let trainer: usize = if bytes[6] & 0x04 != 0 { 512 } else { 0 };
    let end = (16 + trainer).checked_add(prg)?.checked_add(chr)?;
    if prg == 0 || end > bytes.len() {
        return None;
    }
    Some(crc32(bytes))
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
    /// A Mapper column of `0` is "unspecified", not "force NROM" -- applying it
    /// destroys a correct header. Regression for the 12 Sachen ROMs the vendored
    /// table was rewriting to mapper 0 and thereby making unloadable.
    #[test]
    fn a_zero_mapper_column_is_not_an_override() {
        let row = "80D63472, PAL, 0, 0, 2, 1, 0, false, Horizontal, \"Sidewinder (Asia) (PAL) (Unl).nes\"";
        let entry = parse_row(row).expect("row parses");
        assert_eq!(
            entry.mapper, None,
            "a 0 in the Mapper column must not become an override"
        );

        // ... and it must therefore leave a Sachen header alone. Mapper 145 lives
        // as low nibble 1 in byte 6 and high nibble 9 in byte 7.
        let mut rom = vec![0u8; 16];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[6] = 0x10;
        rom[7] = 0x90;
        apply_header_overrides(&mut rom, &entry);
        assert_eq!(rom[6], 0x10, "byte 6 mapper nibble must survive");
        assert_eq!(rom[7], 0x90, "byte 7 mapper nibble must survive");
        assert_eq!((rom[6] >> 4) | (rom[7] & 0xF0), 145);
    }

    /// The guard must not disarm real overrides.
    #[test]
    fn a_non_zero_mapper_column_still_overrides() {
        let row = "DEADBEEF, NTSC, 4, 0, 8, 8, 0, false, Vertical, \"Something.nes\"";
        let entry = parse_row(row).expect("row parses");
        assert_eq!(entry.mapper, Some(4));

        let mut rom = vec![0u8; 16];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[6] = 0x10;
        rom[7] = 0x90;
        assert!(apply_header_overrides(&mut rom, &entry));
        assert_eq!((rom[6] >> 4) | (rom[7] & 0xF0), 4, "mapper 145 -> 4");
    }

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
            db.windows(2).all(|w| w[0].crc < w[1].crc),
            "DB must be strictly sorted by CRC (binary-search invariant)"
        );
        // A known entry from the vendored file: Mega Man 3 (Europe) 0x1388B3 =
        // Vertical mirroring, PAL region, mapper 4.
        assert_eq!(mirroring_for_crc(0x0013_88B3), Some(Mirroring::Vertical));
        let entry = entry_for_crc(0x0013_88B3).expect("Mega Man 3 (Europe) listed");
        assert_eq!(entry.region, Some(Region::Pal));
        assert_eq!(entry.mapper, Some(4));
        assert!(entry.title.contains("Mega Man 3"));
    }

    #[test]
    fn lookup_miss_is_none() {
        assert_eq!(mirroring_for_crc(0xDEAD_BEEF), None);
    }

    #[test]
    fn header_overrides_rewrite_mapper_and_region() {
        // Minimal iNES 1.0 header (mapper 0, NTSC). Override to mapper 4 + PAL.
        let mut rom = vec![0u8; 16 + 16 * 1024 + 8 * 1024];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1; // 1 PRG bank
        rom[5] = 1; // 1 CHR bank
        let entry = GameDbEntry {
            crc: 0,
            region: Some(Region::Pal),
            mapper: Some(4),
            submapper: None,
            mirroring: None,
            title: "Test".into(),
        };
        assert!(apply_header_overrides(&mut rom, &entry));
        // Mapper 4: low nibble in flags6[7:4], high nibble (0) in flags7[7:4].
        assert_eq!(rom[6] >> 4, 4, "mapper low nibble");
        assert_eq!(rom[7] >> 4, 0, "mapper high nibble");
        // v2.9.8: a PAL region promotes the iNES 1.0 header to NES 2.0 and lands
        // in byte 12 -- byte 9 bit 0 is a flag the core never reads.
        assert_eq!(rom[7] & 0x0C, 0x08, "promoted to NES 2.0");
        assert_eq!(rom[12] & 0x03, 1, "NES 2.0 PAL region code");
        assert_eq!(rom[9], 0, "no size MSBs: iNES 1.0 sizes carried over");
        // Re-applying the same override is now a no-op (idempotent).
        assert!(!apply_header_overrides(&mut rom, &entry));
    }

    #[test]
    fn seicross_entry_corrects_protection_submapper() {
        // Seicross (Japan) CRC 0x0F05FF0A is iNES mapper 185 sub 0 in its GoodNES
        // dump, but is really the sub-4 CHR-disable copy-protection variant
        // (enabled iff latch bits 0-1 == 0). The DB carries the correction.
        let entry = entry_for_crc(0x0F05_FF0A).expect("Seicross (Japan) listed");
        assert_eq!(entry.mapper, Some(185));
        assert_eq!(entry.submapper, Some(4));
    }

    /// A 16-byte header for `mapper`, NES 2.0 (`nes2`) or iNES 1.0.
    fn header_for(mapper: u16, nes2: bool) -> [u8; 16] {
        let mut h = [0u8; 16];
        h[0..4].copy_from_slice(b"NES\x1A");
        h[4] = 8; // 128 KiB PRG
        h[5] = 4; // 32 KiB CHR
        h[6] = ((mapper & 0x0F) as u8) << 4;
        h[7] = (mapper & 0xF0) as u8 | if nes2 { 0x08 } else { 0x00 };
        h
    }

    #[test]
    fn vendored_mapper_never_overrides_a_nes2_header() {
        // Youkai Club (Japan), headerless CRC32 6BC65D7E. The board is Jaleco's
        // JF-11/14 (mapper 140, register at $6000-$7FFF); the vendored row says
        // mapper 66 (GNROM, register at $8000-$FFFF), so the game's $6000 bank
        // writes went nowhere and it sat on a blue screen. Its staged dump is a
        // NES 2.0 image that says 140, which is right.
        const YOUKAI_CLUB: u32 = 0x6BC6_5D7E;
        let vendored = vendored_entry(YOUKAI_CLUB).expect("Youkai Club listed");
        assert_eq!(vendored.mapper, Some(66), "premise: the row disagrees");

        // A NES 2.0 header is authoritative: the row's mapper and submapper
        // are dropped, so the header is left as it is.
        let nes2 = header_for(140, true);
        let entry = load_time_entry(YOUKAI_CLUB, &nes2).expect("row still found");
        assert_eq!(entry.mapper, None, "NES 2.0 mapper must not be rewritten");
        assert_eq!(
            entry.submapper, None,
            "NES 2.0 submapper must not be rewritten"
        );
        let mut rom = nes2.to_vec();
        assert!(!apply_header_overrides(&mut rom, &entry));
        assert_eq!(&rom[..], &nes2[..]);

        // An iNES 1.0 header is what the table exists to correct, and it still
        // does (Seicross, v2.3.4, is the case that has to keep working).
        let ines = header_for(140, false);
        let entry = load_time_entry(YOUKAI_CLUB, &ines).expect("row found");
        assert_eq!(entry.mapper, Some(66), "iNES 1.0 corrections still apply");
    }

    /// A whole iNES 1.0 image for `mapper` with `prg16` x 16 KiB of PRG-ROM and
    /// `chr8` x 8 KiB of CHR-ROM (0 = CHR-RAM). The ROM bytes are a counter so
    /// no two banks are alike.
    fn ines1_rom(mapper: u8, prg16: u8, chr8: u8) -> Vec<u8> {
        let mut rom = vec![0u8; 16];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = prg16;
        rom[5] = chr8;
        rom[6] = (mapper & 0x0F) << 4;
        rom[7] = mapper & 0xF0;
        let body = usize::from(prg16) * 16 * 1024 + usize::from(chr8) * 8 * 1024;
        #[allow(clippy::cast_possible_truncation)] // a deliberate byte counter
        rom.extend((0..body).map(|i| (i ^ (i >> 8)) as u8));
        rom
    }

    fn region_entry(region: Region) -> GameDbEntry {
        GameDbEntry {
            crc: 0,
            region: Some(region),
            mapper: None,
            submapper: None,
            mirroring: None,
            title: String::new(),
        }
    }

    /// v2.9.8 — the database's region reaches the core for an iNES 1.0 image.
    ///
    /// Until v2.9.8 a PAL row wrote header byte 9 bit 0, which the core's header
    /// parser ignores by design (raw dumps carry junk there), so every PAL game
    /// in an iNES 1.0 header ran at NTSC timing. Pin Bot (Europe), TQROM, was the
    /// case that showed it: its CHR-RAM upload is sized for the PAL vblank, and
    /// at NTSC it overran into rendering and garbled the title.
    #[test]
    fn a_pal_row_reaches_the_core_on_an_ines1_header() {
        for (mapper, prg16, chr8) in [(0u8, 1u8, 1u8), (1, 8, 0), (119, 8, 8)] {
            let mut rom = ines1_rom(mapper, prg16, chr8);
            assert!(apply_header_overrides(&mut rom, &region_entry(Region::Pal)));
            let nes = rustynes_core::Nes::from_rom(&rom).expect("promoted image parses");
            assert_eq!(
                nes.region(),
                rustynes_core::Region::Pal,
                "mapper {mapper}: the PAL row must reach the core"
            );
        }
    }

    /// Every board the mapper crate builds from an iNES 1.0 image is promoted to
    /// PAL without changing the board. `ines1_as_nes2` refuses a promotion whose
    /// board differs, so what this pins is the other half: that no board is
    /// refused, i.e. that one of the two candidate headers always reproduces it.
    /// A new board whose iNES 1.0 construction no NES 2.0 header can express
    /// fails here, by id, rather than silently losing its PAL timing.
    #[test]
    fn promotion_never_changes_the_board() {
        use rustynes_core::rustynes_mappers::parse;
        let mut refused = Vec::new();
        let mut promoted = 0usize;
        for mapper in 0..=255u8 {
            for (prg16, chr8) in [(2u8, 1u8), (8, 0), (8, 8), (16, 16), (32, 32)] {
                for battery in [false, true] {
                    let mut rom = ines1_rom(mapper, prg16, chr8);
                    rom[6] |= u8::from(battery) << 1;
                    let Ok((cart, _)) = parse(&rom) else {
                        continue; // this layout is not a valid image of the board
                    };
                    if cart.console_type != rustynes_core::rustynes_mappers::ConsoleType::Nes {
                        continue; // arcade boards are refused by design
                    }
                    match ines1_as_nes2(&rom, Region::Pal) {
                        Some(header) => {
                            rom[..16].copy_from_slice(&header);
                            let nes = rustynes_core::Nes::from_rom(&rom).expect("parses");
                            assert_eq!(nes.region(), rustynes_core::Region::Pal);
                            promoted += 1;
                        }
                        None => refused.push((mapper, prg16, chr8, battery)),
                    }
                }
            }
        }
        assert!(
            promoted > 500,
            "the sweep must exercise most boards ({promoted})"
        );
        assert!(
            refused.is_empty(),
            "boards refused a PAL promotion: {refused:?}"
        );
    }

    /// The case the second candidate exists for. Mapper 37 (Super Mario Bros. +
    /// Tetris + Nintendo World Cup, a PAL-only release) is one of the MMC3
    /// multicart boards that ignore iNES 1.0's nominal 8 KiB of PRG-RAM; a NES
    /// 2.0 header that stated 8 KiB would put RAM where the board has its outer
    /// bank register. The promotion must state none.
    #[test]
    fn a_multicart_promotion_does_not_invent_work_ram() {
        let mut rom = ines1_rom(37, 8, 16);
        let header = ines1_as_nes2(&rom, Region::Pal).expect("mapper 37 promotes");
        assert_eq!(header[10], 0, "no PRG-RAM stated for the multicart");
        rom[..16].copy_from_slice(&header);
        let (_, mapper) = rustynes_core::rustynes_mappers::parse(&rom).expect("parses");
        assert!(mapper.sram().is_empty(), "mapper 37 has no work RAM");
    }

    /// Arcade carts keep NTSC: a PlayChoice-10 or Vs. System board exists only
    /// in NTSC timing, so a PAL row on one names the home release that shares
    /// its PRG/CHR. The image is left as it was apart from the byte-9 flag.
    #[test]
    fn an_arcade_cart_is_never_promoted() {
        for byte7 in [0x01u8, 0x02] {
            let mut rom = ines1_rom(0, 2, 1);
            rom[7] = byte7; // clean iNES 1.0 Vs. System / PlayChoice-10 marker
            assert_eq!(ines1_as_nes2(&rom, Region::Pal), None);
            apply_header_overrides(&mut rom, &region_entry(Region::Pal));
            assert_eq!(rom[7], byte7, "the arcade marker survives");
            let nes = rustynes_core::Nes::from_rom(&rom).expect("parses");
            assert_eq!(nes.region(), rustynes_core::Region::Ntsc);
        }
        // Mapper 99 is Vs.-only and is forced to the Vs. System by its id, so
        // both parses agree on the console type and only the explicit arcade
        // refusal keeps it NTSC.
        let mut rom = ines1_rom(99, 2, 2);
        assert_eq!(ines1_as_nes2(&rom, Region::Pal), None);
        apply_header_overrides(&mut rom, &region_entry(Region::Pal));
        let nes = rustynes_core::Nes::from_rom(&rom).expect("parses");
        assert_eq!(nes.region(), rustynes_core::Region::Ntsc);
    }

    /// NES 2.0 states its own region, so a vendored row's region is withheld
    /// along with its mapper and submapper; an iNES 1.0 header takes it.
    #[test]
    fn a_nes2_header_keeps_its_own_region() {
        // Pin Bot (Europe): headerless CRC32 9247C38D, a PAL row.
        const PIN_BOT_EUROPE: u32 = 0x9247_C38D;
        let row = vendored_entry(PIN_BOT_EUROPE).expect("Pin Bot (Europe) listed");
        assert_eq!(row.region, Some(Region::Pal), "premise: a PAL row");
        let nes2 = header_for(119, true);
        let entry = load_time_entry(PIN_BOT_EUROPE, &nes2).expect("row found");
        assert_eq!(entry.region, None, "NES 2.0 region must not be rewritten");
        let ines = header_for(119, false);
        let entry = load_time_entry(PIN_BOT_EUROPE, &ines).expect("row found");
        assert_eq!(entry.region, Some(Region::Pal), "iNES 1.0 takes the row");
    }

    /// A row titled for both markets carries no region: its PAL column is a
    /// choice the table had to make for an image sold in both. A Europe-only row
    /// keeps PAL.
    #[test]
    fn a_multi_market_row_carries_no_region() {
        // Baseball (USA, Europe), AFDCBD24 -- the PRG/CHR of the staged
        // PlayChoice-10 Baseball.
        let baseball = vendored_entry(0xAFDC_BD24).expect("Baseball listed");
        assert!(baseball.title.contains("(USA, Europe)"));
        assert_eq!(baseball.region, None);
        // Pin Bot (Europe) keeps its PAL row.
        let pin_bot = vendored_entry(0x9247_C38D).expect("Pin Bot (Europe) listed");
        assert_eq!(pin_bot.region, Some(Region::Pal));
        assert!(is_multi_region_title("Ice Climber (USA, Europe).nes"));
        assert!(!is_multi_region_title("Pin Bot (Europe).nes"));
        assert!(!is_multi_region_title("Sidewinder (Asia) (PAL) (Unl).nes"));
        assert!(!is_multi_region_title("Gyromite (World).nes"));
    }

    #[test]
    fn submapper_override_promotes_ines1_to_nes2() {
        // A non-zero submapper correction on an iNES-1.0 ROM must promote the
        // header to NES 2.0 (byte-7 bits 2-3 = 0b10) so the core actually reads
        // the submapper (the iNES path forces submapper 0). This is what makes
        // the Seicross sub-4 correction reach mapper 185.
        let mut rom = vec![0u8; 16 + 32 * 1024 + 8 * 1024];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 2; // 32 KiB PRG
        rom[5] = 1; // 8 KiB CHR
        rom[6] = 0xB1; // mapper-185 low nibble (9) + vertical
        rom[7] = 0xB0; // mapper-185 mid nibble; iNES 1.0 (bits 2-3 = 00)
        // Legacy iNES-1.0 garbage in the bytes NES 2.0 reinterprets: byte-8 low
        // nibble (becomes mapper bits 8-11 -> would push mapper 185 to 0x509)
        // and bytes 9-15 (become PRG/CHR-RAM sizes, timing, etc.).
        rom[8] = 0x0F;
        for b in &mut rom[9..16] {
            *b = 0xCC;
        }
        assert_eq!((rom[7] & 0x0C), 0, "starts as iNES 1.0");
        let entry = GameDbEntry {
            crc: 0,
            region: None,
            mapper: None,
            submapper: Some(4),
            mirroring: None,
            title: "Seicross-like".into(),
        };
        assert!(apply_header_overrides(&mut rom, &entry));
        assert_eq!(rom[7] & 0x0C, 0x08, "promoted to NES 2.0");
        assert_eq!(rom[8] >> 4, 4, "submapper 4 landed in byte 8");
        // Promotion sanitized the reinterpreted bytes: mapper bits 8-11 (byte-8
        // low nibble) are zero so the mapper number stays 185, and bytes 9-15
        // are zeroed so no phantom NES-2.0 RAM/timing fields are fabricated.
        assert_eq!(
            rom[8] & 0x0F,
            0,
            "mapper hi-nibble cleared (mapper stays 185)"
        );
        assert!(rom[9..16].iter().all(|&b| b == 0), "bytes 9-15 sanitized");
        // A zero-submapper override is a no-op (no spurious promotion).
        let mut rom2 = rom.clone();
        rom2[7] = 0xB0; // back to iNES 1.0
        rom2[8] = 0;
        let entry0 = GameDbEntry {
            submapper: Some(0),
            ..entry
        };
        assert!(!apply_header_overrides(&mut rom2, &entry0));
        assert_eq!(rom2[7] & 0x0C, 0, "sub-0 override does not promote");
    }

    #[test]
    fn overlay_row_round_trips_through_parse() {
        // The editor serializes user-overlay entries with `serialize_row`; they
        // must parse back identically via `parse_row` (same on-disk format as
        // the vendored DB).
        let entry = GameDbEntry {
            crc: 0x0013_88B3,
            region: Some(Region::Pal),
            mapper: Some(4),
            submapper: Some(1),
            mirroring: Some(Mirroring::Vertical),
            title: "Mega Man 3 (Europe) (Rev A).nes".into(),
        };
        let row = serialize_row(&entry);
        let parsed = parse_row(&row).expect("serialized row parses");
        assert_eq!(parsed, entry);

        // A sparse entry (only mirroring) also round-trips (empty columns -> None).
        let sparse = GameDbEntry {
            crc: 0xABCD_1234,
            region: None,
            mapper: None,
            submapper: None,
            mirroring: Some(Mirroring::Horizontal),
            title: "Homebrew".into(),
        };
        assert_eq!(parse_row(&serialize_row(&sparse)), Some(sparse));
    }

    #[test]
    fn header_overrides_noop_for_non_ines() {
        let mut not_a_rom = b"not a rom".to_vec();
        let entry = GameDbEntry {
            crc: 0,
            region: Some(Region::Pal),
            mapper: Some(4),
            submapper: None,
            mirroring: None,
            title: String::new(),
        };
        assert!(!apply_header_overrides(&mut not_a_rom, &entry));
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

    /// The shared load-time path (v2.9.8) delivers all three kinds of
    /// correction a row carries: the mapper and the region through the header
    /// ([`correct_rom`]), the hardwired mirroring through the console
    /// ([`correct_console`]). The hosts' own tests drive the same image through
    /// their load paths; this one pins the two stages themselves.
    #[test]
    fn the_shared_path_delivers_mapper_region_and_mirroring() {
        use rustynes_core::Nes;
        use rustynes_core::rustynes_mappers::Mirroring;

        let raw = test_support::gradius_europe_with_a_wrong_header();
        assert!(
            Nes::from_rom(&raw).is_err(),
            "premise: uncorrected, the image is NROM with 32 KiB of CHR, which NROM refuses"
        );

        let mut bytes = raw;
        let crc = correct_rom(&mut bytes).expect("an iNES image");
        assert_eq!(crc, test_support::GRADIUS_EUROPE_CRC);
        let mut nes = Nes::from_rom(&bytes).expect("the corrected image builds");
        // The mapper is read from the corrected header, not `Nes::mapper_id`:
        // that reports the board's own debug view, which CNROM does not
        // override (it answers 0). Building at all is the board-level proof --
        // NROM refuses this image.
        let header = rustynes_core::rustynes_mappers::parse_header(&bytes).expect("parses");
        assert_eq!(header.mapper_id, 3, "the row's mapper");
        assert_eq!(nes.region(), rustynes_core::Region::Pal, "the row's region");
        assert_eq!(nes.mirroring_override(), None, "stage two has not run yet");
        assert!(correct_console(&mut nes, crc));
        assert_eq!(nes.mirroring_override(), Some(Mirroring::Vertical));

        // Idempotent: a second pass over corrected bytes changes nothing (the
        // desktop's startup path can reach the correction twice).
        let mut again = bytes.clone();
        assert_eq!(correct_rom(&mut again), Some(crc));
        assert_eq!(again, bytes);
    }

    /// Not an iNES image: nothing to correct, and `None` rather than an error,
    /// so an FDS disk or an NSF still reaches its own constructor.
    #[test]
    fn the_shared_path_leaves_other_formats_alone() {
        let mut fds = b"FDS\x1A\x01".to_vec();
        fds.resize(64, 0);
        let before = fds.clone();
        assert_eq!(correct_rom(&mut fds), None);
        assert_eq!(fds, before);
        let mut nsf = b"NESM\x1A\x01".to_vec();
        nsf.resize(128, 0);
        assert_eq!(correct_rom(&mut nsf), None);
    }
}

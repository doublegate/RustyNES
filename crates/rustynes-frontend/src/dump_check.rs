//! `rustynes dump-check`: a ROM dump's verification and quality report
//! (v3.5.0, `T-PRESERVE-DUMP-VERIFY`, D33).
//!
//! A dump can be wrong in ways an emulator hides. A copier that read past the
//! end of a mask ROM produces an image twice its real size whose halves are
//! identical (an *overdump*). A header written by hand, or by an old tool, can
//! name the wrong board, the wrong mirroring, or CHR-ROM where the cartridge has
//! CHR-RAM; the per-game database then corrects it at load and the user never
//! learns the file is wrong. This module reports all of that for one file,
//! headless, from five independent checks:
//!
//! 1. **Hashes** of the bytes after the 16-byte header (trainer included, the
//!    span No-Intro's headerless DATs and the NES 2.0 database's `<rom>` cover)
//!    and of the whole file: SHA-256 and CRC-32 of each.
//! 2. **A No-Intro DAT the user supplies** (`--dat`, Logiqx XML as DAT-o-MATIC
//!    exports it). RustyNES never ships one. A SHA-256 match is "verified"; a
//!    DAT row without SHA-256 can only be matched by CRC-32 and size, which is
//!    reported as the weaker claim it is. Both the headerless and the headered
//!    convention are tried, and the report says which matched.
//! 3. **The header against the NES 2.0 database the user supplies**
//!    (`--nes20db`, NewRisingSun's `nes20db.xml`; never shipped either), keyed
//!    by the headerless CRC-32 and size, field by field. An iNES 1.0 header is
//!    compared only on the fields iNES 1.0 can express. Independently, the
//!    **built-in game database** (vendored, always available) is asked whether
//!    it would correct this header at load: `rustynes_gamedb::correct_rom`, the
//!    exact load-time path, run on a copy.
//! 4. **Overdump detection**: PRG-ROM and CHR-ROM are each tested for a
//!    power-of-two period, halving while the two halves are byte-identical, down
//!    to 8 KiB. A repeat is reported as *suspected*: a board whose mask ROM is
//!    genuinely repeated is indistinguishable from bytes alone, which is what
//!    the DAT check settles.
//! 5. **The header against the board's observed behaviour**: the core runs the
//!    image as its header describes it, with no input, for a bounded number of
//!    frames, with the bus-access log on (`debug-hooks`, which the frontend
//!    always links), and three contradictions are counted: register writes to
//!    `$8000-$FFFF` on a header that says NROM (mapper 0 has no registers);
//!    pattern-table writes through `$2006`/`$2007` on a header that says
//!    CHR-ROM (which ignores them; the board probably has CHR-RAM); and
//!    `$6000-$7FFF` writes on a NES 2.0 NROM header that declares no PRG-RAM.
//!
//! # What it does not establish
//!
//! A clean report means nothing checked disagreed, not that the dump is good.
//! Without a DAT the dump is unverified, whatever else passed. The behaviour
//! check sees only what the program does with no input in its first frames
//! (most boards set up their banks and upload CHR at once, but a game may touch
//! RAM or CHR only later), and it models the PPU address from the CPU's
//! register writes, ignoring rendering's own increments of `v`. The three
//! behaviour rules are the ones whose contradiction is unambiguous from bus
//! traffic; mirroring, IRQs and most boards' register maps are not judged. Host
//! names, DAT names and database comments are reported as given.
//!
//! # Input handling
//!
//! Every string the report takes from an input file (DAT game names, the
//! database's names and mirroring values) and the ROM path are passed through
//! `printable`, which turns each control character into a visible `\u{..}`
//! escape, so a crafted file cannot inject terminal escape sequences (ESC,
//! CSI U+009B) into the output.
//!
//! The DAT and the database are untrusted files. They are read with a size cap,
//! parsed by a small tag scanner that never panics on malformed text (it skips
//! what it cannot read, and refuses a file in which it finds no game at all),
//! and every number in them is parsed with an explicit failure path.

// "NewRisingSun", "DAT-o-MATIC", "No-Intro", "iNES" and the register names
// are prose, not code identifiers.
#![allow(clippy::doc_markdown)]

use std::fmt::Write as _;
use std::path::Path;

use rustynes_core::Nes;
use rustynes_core::rustynes_mappers::parse_header;
use sha2::{Digest, Sha256};

/// Files larger than this are refused (ROM, DAT and database alike). The
/// largest NES 2.0 image the format can describe is far smaller than a DAT,
/// and the NES 2.0 database is about 5 MiB.
pub const MAX_INPUT_BYTES: u64 = 64 * 1024 * 1024;

/// The default number of frames the behaviour check runs (ten seconds NTSC).
pub const DEFAULT_FRAMES: u32 = 600;

/// Pattern-table bytes written before the CHR-RAM rule fires. A CHR-RAM game
/// uploads its tiles at boot, kilobytes of them; a CHR-ROM game may still
/// write a few bytes there by accident (measured on the local dump corpus at
/// 120 frames: 1942 96 bytes, Vs. Super Mario Bros. 129, Dragon Warrior 16),
/// which the ROM ignores. 1 KiB is above every such case and below any upload.
const CHR_WRITE_THRESHOLD: u32 = 1024;

/// Boards that never carry CHR-RAM beside CHR-ROM, so a CHR-ROM header on
/// them plus a pattern-table upload is a contradiction: NROM (0), MMC1 (1),
/// UxROM (2), CNROM (3), MMC3 (4; TQROM, which has both, is mapper 119),
/// AxROM (7), MMC2 (9), MMC4 (10), Color Dreams (11), GxROM (66). Every other
/// board is not judged: several (TQROM, the Waixing MMC3 clones, UNROM 512,
/// many multicarts) map CHR-RAM and CHR-ROM together, and the corpus run that
/// set this list flagged eight of them before it existed.
const CHR_ROM_ONLY_BOARDS: &[u16] = &[0, 1, 2, 3, 4, 7, 9, 10, 11, 66];

/// The smallest PRG or CHR block the overdump check halves down to.
const OVERDUMP_FLOOR: usize = 8 * 1024;

/// How a finding bears on the dump, in increasing order of severity. The
/// report's verdict is its worst finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Not a defect: the dump is simply not confirmed (no DAT row matched).
    Unverified,
    /// The header disagrees with a database or with the board's behaviour.
    WrongHeader,
    /// The bytes themselves are wrong: an overdump, a truncation, trailing
    /// data, or an image the core refuses.
    BadDump,
}

impl Severity {
    /// The short tag printed before a finding and as the verdict.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unverified => "UNVERIFIED",
            Self::WrongHeader => "WRONG HEADER",
            Self::BadDump => "BAD DUMP",
        }
    }
}

/// One problem the report found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// How it bears on the dump.
    pub severity: Severity,
    /// Which check produced it (`dat`, `nes20db`, `gamedb`, `overdump`,
    /// `size`, `board`, `core`).
    pub check: &'static str,
    /// What was found, in one sentence with the evidence.
    pub message: String,
}

/// The result of matching against a user-supplied DAT.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DatMatch {
    /// No DAT was supplied.
    NotSupplied,
    /// A row's SHA-256 matched. `headered` says the DAT hashes whole files.
    Verified {
        /// The DAT's game name.
        game: String,
        /// `true` when the match was on the whole file, header included.
        headered: bool,
    },
    /// A row without SHA-256 matched on CRC-32 and size: a weaker claim.
    CrcOnly {
        /// The DAT's game name.
        game: String,
        /// `true` when the match was on the whole file, header included.
        headered: bool,
    },
    /// No row matched.
    NotListed {
        /// How many ROM rows the DAT holds (so "not listed" in an empty or
        /// wrong DAT is visible as such).
        rows: usize,
    },
}

/// The result of comparing the header with the NES 2.0 database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Nes20Match {
    /// No database was supplied.
    NotSupplied,
    /// The image is not an iNES / NES 2.0 file, so there is no header.
    NotApplicable,
    /// No entry has this headerless CRC-32 and size.
    NotListed,
    /// An entry matched; `diffs` lists every field that disagrees (empty when
    /// all agree). `skipped` names fields an iNES 1.0 header cannot express.
    Listed {
        /// The entry's name (the database's comment).
        name: String,
        /// `(field, header value, database value)` for each disagreement.
        diffs: Vec<(&'static str, String, String)>,
        /// Fields not compared because the header is iNES 1.0.
        skipped: &'static [&'static str],
        /// `false` when the board switches its own mirroring, so the header's
        /// mirroring bit is unused and was not compared.
        mirroring_compared: bool,
    },
}

/// What the behaviour run observed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BoardObservation {
    /// Frames run.
    pub frames: u32,
    /// CPU writes to `$8000-$FFFF`, and the first one `(addr, value)`.
    pub rom_writes: u32,
    /// The first `$8000-$FFFF` write.
    pub first_rom_write: Option<(u16, u8)>,
    /// `$2007` writes while the modelled PPU address was in `$0000-$1FFF`.
    pub chr_writes: u32,
    /// CPU writes to `$6000-$7FFF`.
    pub wram_writes: u32,
}

/// The complete report for one file.
#[derive(Clone, Debug)]
pub struct DumpReport {
    /// File length in bytes.
    pub len: usize,
    /// `Some(true)` NES 2.0, `Some(false)` iNES 1.0, `None` not an iNES image
    /// (FDS, NSF, UNIF, or not a ROM).
    pub nes2: Option<bool>,
    /// A one-line description of the header (`None` when there is none).
    pub header_summary: Option<String>,
    /// SHA-256 of the bytes after the header (the whole file when headerless).
    pub sha256_headerless: [u8; 32],
    /// CRC-32 of the same bytes.
    pub crc_headerless: u32,
    /// SHA-256 of the whole file.
    pub sha256_full: [u8; 32],
    /// CRC-32 of the whole file.
    pub crc_full: u32,
    /// The DAT result.
    pub dat: DatMatch,
    /// The NES 2.0 database result.
    pub nes20db: Nes20Match,
    /// The behaviour run, when the core accepted the image.
    pub board: Option<BoardObservation>,
    /// Everything found, in check order.
    pub findings: Vec<Finding>,
}

impl DumpReport {
    /// The worst finding's severity, or `None` when nothing was found.
    #[must_use]
    pub fn verdict(&self) -> Option<Severity> {
        self.findings.iter().map(|f| f.severity).max()
    }
}

/// A No-Intro (Logiqx XML) DAT, reduced to what matching needs.
#[derive(Clone, Debug, Default)]
pub struct Dat {
    rows: Vec<DatRow>,
}

#[derive(Clone, Debug)]
struct DatRow {
    game: String,
    size: Option<u64>,
    crc: Option<u32>,
    sha256: Option<[u8; 32]>,
}

impl Dat {
    /// Parse a Logiqx XML DAT (`<datafile>` of `<game>` or `<machine>`
    /// elements, each with `<rom size crc sha256 .../>` children).
    ///
    /// # Errors
    /// When the text holds no `<rom>` row inside a `<game>` / `<machine>`
    /// (the file is not such a DAT; a clrmamepro text DAT, for one, is not
    /// read).
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut rows = Vec::new();
        let mut game: Option<String> = None;
        for item in XmlScanner::new(text) {
            match item {
                XmlItem::Open {
                    name: "game" | "machine",
                    attrs,
                    ..
                } => {
                    game = Some(printable(&attr(&attrs, "name").unwrap_or_default()));
                }
                XmlItem::Close("game" | "machine") => game = None,
                XmlItem::Open {
                    name: "rom", attrs, ..
                } => {
                    if let Some(g) = &game {
                        rows.push(DatRow {
                            game: g.clone(),
                            size: attr(&attrs, "size").and_then(|s| s.trim().parse().ok()),
                            crc: attr(&attrs, "crc").and_then(|s| parse_hex_u32(&s)),
                            sha256: attr(&attrs, "sha256").and_then(|s| parse_hex_32(&s)),
                        });
                    }
                }
                _ => {}
            }
        }
        if rows.is_empty() {
            return Err(
                "no <rom> rows inside <game> or <machine> elements: not a Logiqx XML DAT \
                 (No-Intro's DAT-o-MATIC exports one)"
                    .to_string(),
            );
        }
        Ok(Self { rows })
    }

    /// How many ROM rows the DAT holds.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the DAT holds no rows (never true for a parsed DAT).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    fn lookup(&self, spans: &[(&[u8], [u8; 32], u32, bool)]) -> DatMatch {
        // SHA-256 first, over both conventions, before any CRC match.
        for &(_, sha, _, headered) in spans {
            if let Some(r) = self.rows.iter().find(|r| r.sha256 == Some(sha)) {
                return DatMatch::Verified {
                    game: r.game.clone(),
                    headered,
                };
            }
        }
        for &(bytes, _, crc, headered) in spans {
            let len = bytes.len() as u64;
            if let Some(r) = self
                .rows
                .iter()
                .find(|r| r.sha256.is_none() && r.crc == Some(crc) && r.size == Some(len))
            {
                return DatMatch::CrcOnly {
                    game: r.game.clone(),
                    headered,
                };
            }
        }
        DatMatch::NotListed {
            rows: self.rows.len(),
        }
    }
}

/// The NES 2.0 database (`nes20db.xml`), reduced to the fields a header holds.
#[derive(Clone, Debug, Default)]
pub struct Nes20Db {
    games: Vec<Nes20Game>,
}

#[derive(Clone, Debug, Default)]
struct Nes20Game {
    name: String,
    rom_size: Option<u64>,
    rom_crc: Option<u32>,
    prg_size: Option<u64>,
    chr_size: u64,
    prg_ram: u64,
    prg_nvram: u64,
    chr_ram: u64,
    chr_nvram: u64,
    trainer: bool,
    mapper: Option<u16>,
    submapper: Option<u8>,
    mirroring: Option<String>,
    battery: Option<bool>,
    console_type: Option<u8>,
    region: Option<u8>,
    vs_hardware: Option<u8>,
    vs_ppu: Option<u8>,
    expansion: Option<u8>,
}

impl Nes20Db {
    /// Parse `nes20db.xml`: `<game>` elements whose first comment names the
    /// game and whose children (`prgrom`, `chrrom`, `rom`, `trainer`,
    /// `prgram`, `prgnvram`, `chrram`, `chrnvram`, `pcb`, `console`, `vs`,
    /// `expansion`) carry the header's fields as attributes.
    ///
    /// # Errors
    /// When no `<game>` with a `<rom crc32 size>` child is found.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut games = Vec::new();
        let mut cur: Option<Nes20Game> = None;
        for item in XmlScanner::new(text) {
            match item {
                XmlItem::Open {
                    name: "game",
                    self_closing,
                    ..
                } => {
                    cur = Some(Nes20Game::default());
                    if self_closing {
                        cur = None;
                    }
                }
                XmlItem::Close("game") => {
                    if let Some(g) = cur.take()
                        && g.rom_crc.is_some()
                        && g.rom_size.is_some()
                    {
                        games.push(g);
                    }
                }
                XmlItem::Comment(text) => {
                    if let Some(g) = cur.as_mut()
                        && g.name.is_empty()
                    {
                        g.name = printable(text.trim());
                    }
                }
                XmlItem::Open { name, attrs, .. } => {
                    if let Some(g) = cur.as_mut() {
                        g.apply(name, &attrs);
                    }
                }
                XmlItem::Close(_) => {}
            }
        }
        if games.is_empty() {
            return Err(
                "no <game> with a <rom crc32 size> child: not the NES 2.0 database (nes20db.xml)"
                    .to_string(),
            );
        }
        Ok(Self { games })
    }

    /// How many games the database holds.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.games.len()
    }

    /// Whether the database holds no games (never true for a parsed one).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.games.is_empty()
    }

    fn find(&self, crc: u32, size: u64) -> Option<&Nes20Game> {
        self.games
            .iter()
            .find(|g| g.rom_crc == Some(crc) && g.rom_size == Some(size))
    }
}

impl Nes20Game {
    fn apply(&mut self, element: &str, attrs: &[(String, String)]) {
        let num = |k: &str| attr(attrs, k).and_then(|v| v.trim().parse::<u64>().ok());
        let small = |k: &str| attr(attrs, k).and_then(|v| v.trim().parse::<u8>().ok());
        match element {
            "rom" => {
                self.rom_size = num("size");
                self.rom_crc = attr(attrs, "crc32").and_then(|s| parse_hex_u32(&s));
            }
            "prgrom" => self.prg_size = num("size"),
            "chrrom" => self.chr_size = num("size").unwrap_or(0),
            "prgram" => self.prg_ram = num("size").unwrap_or(0),
            "prgnvram" => self.prg_nvram = num("size").unwrap_or(0),
            "chrram" => self.chr_ram = num("size").unwrap_or(0),
            "chrnvram" => self.chr_nvram = num("size").unwrap_or(0),
            "trainer" => self.trainer = true,
            "pcb" => {
                self.mapper = attr(attrs, "mapper").and_then(|v| v.trim().parse().ok());
                self.submapper = small("submapper");
                self.mirroring = attr(attrs, "mirroring").map(|m| printable(&m));
                self.battery = small("battery").map(|b| b != 0);
            }
            "console" => {
                self.console_type = small("type");
                self.region = small("region");
            }
            "vs" => {
                self.vs_hardware = small("hardware");
                self.vs_ppu = small("ppu");
            }
            "expansion" => self.expansion = small("type"),
            _ => {}
        }
    }
}

/// Fields an iNES 1.0 header cannot express, so they are not compared.
const INES1_SKIPPED: &[&str] = &[
    "submapper",
    "PRG-RAM",
    "PRG-NVRAM",
    "CHR-RAM",
    "CHR-NVRAM",
    "console type",
    "region",
    "Vs. hardware",
    "Vs. PPU",
    "expansion device",
];

/// NES 2.0 RAM size nibble -> bytes (`0` means none; else `64 << n`).
const fn ram_size(nibble: u8) -> u64 {
    if nibble == 0 { 0 } else { 64u64 << nibble }
}

/// Compare the 16-byte header with a database entry, field by field.
// One flat comparison per header field, in the header's order; splitting it
// would only scatter the list a reader checks against the NES 2.0 layout.
#[allow(clippy::too_many_lines)]
fn header_diffs(
    h: &[u8; 16],
    g: &Nes20Game,
    mirroring_switched: bool,
) -> (Vec<(&'static str, String, String)>, bool) {
    let mut diffs = Vec::new();
    let Ok(parsed) = parse_header(h) else {
        return (diffs, false);
    };
    let nes2 = parsed.is_nes2;
    let mut cmp = |field: &'static str, header: String, db: Option<String>| {
        if let Some(db) = db
            && db != header
        {
            diffs.push((field, header, db));
        }
    };
    cmp(
        "mapper",
        parsed.mapper_id.to_string(),
        g.mapper.map(|m| m.to_string()),
    );
    cmp(
        "PRG-ROM",
        parsed.prg_size.to_string(),
        g.prg_size.map(|s| s.to_string()),
    );
    cmp(
        "CHR-ROM",
        parsed.chr_size.to_string(),
        Some(g.chr_size.to_string()),
    );
    let header_mirroring = if h[6] & 0x08 != 0 {
        "4"
    } else if h[6] & 0x01 != 0 {
        "V"
    } else {
        "H"
    };
    // "1" (14 entries in the 2021-12-25 database) is not one of the header's
    // three encodings, so it is not compared; nor is mirroring at all on a
    // board that switches its own (the header bit is then unused).
    if !mirroring_switched {
        cmp(
            "mirroring",
            header_mirroring.to_string(),
            g.mirroring.clone().filter(|m| m != "1"),
        );
    }
    cmp(
        "battery",
        (h[6] & 0x02 != 0).to_string(),
        g.battery.map(|b| b.to_string()),
    );
    cmp(
        "trainer",
        (h[6] & 0x04 != 0).to_string(),
        Some(g.trainer.to_string()),
    );
    if nes2 {
        cmp(
            "submapper",
            parsed.submapper.to_string(),
            g.submapper.map(|s| s.to_string()),
        );
        cmp(
            "PRG-RAM",
            ram_size(h[10] & 0x0F).to_string(),
            Some(g.prg_ram.to_string()),
        );
        cmp(
            "PRG-NVRAM",
            ram_size(h[10] >> 4).to_string(),
            Some(g.prg_nvram.to_string()),
        );
        cmp(
            "CHR-RAM",
            ram_size(h[11] & 0x0F).to_string(),
            Some(g.chr_ram.to_string()),
        );
        cmp(
            "CHR-NVRAM",
            ram_size(h[11] >> 4).to_string(),
            Some(g.chr_nvram.to_string()),
        );
        // The database numbers console types 0-2 as byte 7 does and the
        // extended types (3 and up) as byte 13's low nibble.
        let console = if h[7] & 0x03 == 0x03 {
            h[13] & 0x0F
        } else {
            h[7] & 0x03
        };
        cmp(
            "console type",
            console.to_string(),
            g.console_type.map(|c| c.to_string()),
        );
        cmp(
            "region",
            (h[12] & 0x03).to_string(),
            g.region.map(|r| r.to_string()),
        );
        if h[7] & 0x03 == 0x01 {
            cmp(
                "Vs. hardware",
                (h[13] >> 4).to_string(),
                g.vs_hardware.map(|v| v.to_string()),
            );
            cmp(
                "Vs. PPU",
                (h[13] & 0x0F).to_string(),
                g.vs_ppu.map(|v| v.to_string()),
            );
        }
        // 0 is "unspecified" in NES 2.0: a header that does not say is not
        // one that disagrees.
        if h[15] & 0x3F != 0 {
            cmp(
                "expansion device",
                (h[15] & 0x3F).to_string(),
                g.expansion.map(|e| e.to_string()),
            );
        }
    }
    (diffs, nes2)
}

/// The smallest power-of-two period of `data`, halving while the two halves
/// are identical, never below `floor`. Equal to `data.len()` when it does not
/// repeat.
#[must_use]
pub fn repeat_period(data: &[u8], floor: usize) -> usize {
    let mut size = data.len();
    while size >= floor * 2 && size.is_multiple_of(2) && data[..size / 2] == data[size / 2..size] {
        size /= 2;
    }
    size
}

fn kib(bytes: usize) -> String {
    if bytes.is_multiple_of(1024) {
        format!("{} KiB", bytes / 1024)
    } else {
        format!("{bytes} bytes")
    }
}

/// Inputs to [`check`] besides the ROM bytes.
#[derive(Clone, Copy, Debug)]
pub struct CheckOptions<'a> {
    /// A parsed No-Intro DAT, if the user supplied one.
    pub dat: Option<&'a Dat>,
    /// The parsed NES 2.0 database, if the user supplied it.
    pub nes20db: Option<&'a Nes20Db>,
    /// Frames for the behaviour run (`0` skips it).
    pub frames: u32,
}

/// Check one ROM image and build its report. Pure: no file access, so the
/// planted fixtures in the tests reach every path.
// The checks run in the order the report prints them, each a few lines; the
// length is the header summary's formatting, kept beside the header parse.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn check(rom: &[u8], opts: &CheckOptions<'_>) -> DumpReport {
    let mut findings = Vec::new();
    let is_ines = rom.len() >= 16 && rom[..4] == *b"NES\x1A";
    let headerless: &[u8] = if is_ines { &rom[16..] } else { rom };
    let sha256_headerless: [u8; 32] = Sha256::digest(headerless).into();
    let sha256_full: [u8; 32] = Sha256::digest(rom).into();
    let crc_headerless = rustynes_gamedb::crc32(headerless);
    let crc_full = rustynes_gamedb::crc32(rom);

    // --- DAT -----------------------------------------------------------
    let dat = opts.dat.map_or(DatMatch::NotSupplied, |d| {
        let mut spans = vec![(headerless, sha256_headerless, crc_headerless, false)];
        if is_ines {
            spans.push((rom, sha256_full, crc_full, true));
        }
        d.lookup(&spans)
    });
    match &dat {
        DatMatch::NotListed { rows } => findings.push(Finding {
            severity: Severity::Unverified,
            check: "dat",
            message: format!(
                "no row of the supplied DAT ({rows} rows) has this dump's SHA-256 (or, for a row \
                 without one, its CRC-32 and size), headerless or headered: a bad dump, a hack, \
                 or a dump this DAT does not list"
            ),
        }),
        DatMatch::NotSupplied | DatMatch::Verified { .. } | DatMatch::CrcOnly { .. } => {}
    }

    // --- Header, size, overdump, databases, behaviour (iNES only) -------
    let mut nes2 = None;
    let mut header_summary = None;
    let mut nes20db = if opts.nes20db.is_some() {
        Nes20Match::NotApplicable
    } else {
        Nes20Match::NotSupplied
    };
    let mut board = None;
    if is_ines {
        let header: [u8; 16] = rom[..16].try_into().expect("length checked");
        match parse_header(&header) {
            Err(e) => findings.push(Finding {
                severity: Severity::BadDump,
                check: "size",
                message: format!("the header does not parse: {e}"),
            }),
            Ok(h) => {
                nes2 = Some(h.is_nes2);
                header_summary = Some(format!(
                    "{}, mapper {}{}, PRG-ROM {}, {}, {} mirroring{}{}",
                    if h.is_nes2 { "NES 2.0" } else { "iNES 1.0" },
                    h.mapper_id,
                    if h.is_nes2 {
                        format!(".{}", h.submapper)
                    } else {
                        String::new()
                    },
                    kib(h.prg_size),
                    if h.chr_size == 0 {
                        "CHR-RAM".to_string()
                    } else {
                        format!("CHR-ROM {}", kib(h.chr_size))
                    },
                    if h.four_screen {
                        "four-screen"
                    } else if header[6] & 1 != 0 {
                        "vertical"
                    } else {
                        "horizontal"
                    },
                    if h.has_battery { ", battery" } else { "" },
                    if h.has_trainer { ", trainer" } else { "" },
                ));
                size_and_overdump(rom, &h, &mut findings);
                if let Some(db) = opts.nes20db {
                    // Whether the board switches its own mirroring, from the
                    // core's board model (`false` when the core refuses it).
                    let switched =
                        Nes::from_rom(rom).is_ok_and(|n| !n.mapper_has_hardwired_mirroring());
                    nes20db = compare_nes20db(
                        db,
                        &header,
                        headerless,
                        crc_headerless,
                        switched,
                        &mut findings,
                    );
                }
                gamedb_correction(rom, &mut findings);
                if opts.frames > 0 {
                    board = observe_board(rom, &h, opts.frames, &mut findings);
                }
            }
        }
    }

    DumpReport {
        len: rom.len(),
        nes2,
        header_summary,
        sha256_headerless,
        crc_headerless,
        sha256_full,
        crc_full,
        dat,
        nes20db,
        board,
        findings,
    }
}

fn size_and_overdump(
    rom: &[u8],
    h: &rustynes_core::rustynes_mappers::Header,
    findings: &mut Vec<Finding>,
) {
    let trainer = if h.has_trainer { 512 } else { 0 };
    let prg_start = 16 + trainer;
    let chr_start = prg_start + h.prg_size;
    let declared = chr_start + h.chr_size;
    if rom.len() < declared {
        findings.push(Finding {
            severity: Severity::BadDump,
            check: "size",
            message: format!(
                "truncated: the header declares {declared} bytes (header, {}PRG-ROM {}, CHR-ROM \
                 {}) but the file has {}",
                if trainer > 0 { "trainer, " } else { "" },
                kib(h.prg_size),
                kib(h.chr_size),
                rom.len()
            ),
        });
        return;
    }
    let extra = rom.len() - declared;
    // NES 2.0 declares miscellaneous ROMs, which follow CHR-ROM. iNES 1.0
    // declares only one thing there: byte 7 bit 1, PlayChoice-10, whose 8 KiB
    // hint-screen ROM plus the 16-byte key and 16-byte decryption data follow
    // CHR-ROM (8,224 bytes; the corpus run found eight PlayChoice dumps
    // flagged before this). Anything else trailing is not part of the
    // cartridge.
    let playchoice_data = !h.is_nes2 && rom[7] & 0x02 != 0 && extra == 8192 + 32;
    if extra > 0 && !(h.is_nes2 && h.misc_rom_count > 0) && !playchoice_data {
        findings.push(Finding {
            severity: Severity::BadDump,
            check: "size",
            message: format!(
                "{extra} bytes follow the PRG-ROM and CHR-ROM the header declares, and the \
                 header declares no miscellaneous ROM: trailing data that is not part of the \
                 cartridge (or a header that understates a ROM's size)"
            ),
        });
    }
    for (what, start, size) in [
        ("PRG-ROM", prg_start, h.prg_size),
        ("CHR-ROM", chr_start, h.chr_size),
    ] {
        if size == 0 {
            continue;
        }
        let period = repeat_period(&rom[start..start + size], OVERDUMP_FLOOR);
        if period < size {
            findings.push(Finding {
                severity: Severity::BadDump,
                check: "overdump",
                message: format!(
                    "suspected overdump: {what} is {} whose content repeats every {}, so the \
                     chip is probably {} read {}x over (a board with a genuinely repeated mask \
                     ROM looks the same; a DAT match settles it)",
                    kib(size),
                    kib(period),
                    kib(period),
                    size / period
                ),
            });
        }
    }
}

fn compare_nes20db(
    db: &Nes20Db,
    header: &[u8; 16],
    headerless: &[u8],
    crc: u32,
    mirroring_switched: bool,
    findings: &mut Vec<Finding>,
) -> Nes20Match {
    let Some(g) = db.find(crc, headerless.len() as u64) else {
        return Nes20Match::NotListed;
    };
    let (diffs, nes2) = header_diffs(header, g, mirroring_switched);
    for (field, have, want) in &diffs {
        findings.push(Finding {
            severity: Severity::WrongHeader,
            check: "nes20db",
            message: format!(
                "header {field} is {have}, the NES 2.0 database says {want} (\"{}\")",
                g.name
            ),
        });
    }
    Nes20Match::Listed {
        name: g.name.clone(),
        diffs,
        skipped: if nes2 { &[] } else { INES1_SKIPPED },
        mirroring_compared: !mirroring_switched,
    }
}

/// Run the built-in database's load-time correction on a copy and report
/// every header byte it would change.
fn gamedb_correction(rom: &[u8], findings: &mut Vec<Finding>) {
    // `correct_rom` keys by the PRG + CHR CRC, so it needs the whole image;
    // only the header can change, so compare that. The caller has checked
    // that `rom` is an iNES image (at least 16 bytes).
    let mut whole = rom.to_vec();
    if rustynes_gamedb::correct_rom(&mut whole).is_none() {
        return;
    }
    let corrected = &whole[..16];
    let changed: Vec<String> = rom[..16]
        .iter()
        .zip(corrected)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, (a, b))| format!("byte {i}: ${a:02X} -> ${b:02X}"))
        .collect();
    if !changed.is_empty() {
        let after = parse_header(corrected).map_or_else(
            |_| String::new(),
            |h| format!(" (mapper {}.{})", h.mapper_id, h.submapper),
        );
        findings.push(Finding {
            severity: Severity::WrongHeader,
            check: "gamedb",
            message: format!(
                "RustyNES's built-in game database corrects this header at load{after}: {}",
                changed.join(", ")
            ),
        });
    }
}

/// The CPU-side model of the PPU's address register: enough to tell which
/// `$2007` accesses land in the pattern tables.
#[derive(Default)]
struct PpuAddrModel {
    v: u16,
    t_hi: u8,
    second_write: bool,
    increment_32: bool,
}

impl PpuAddrModel {
    /// Feed one CPU access; returns `true` for a `$2007` write that targets
    /// `$0000-$1FFF`.
    fn access(&mut self, write: bool, addr: u16, value: u8) -> bool {
        if !(0x2000..0x4000).contains(&addr) {
            return false;
        }
        match (addr & 7, write) {
            (0, true) => self.increment_32 = value & 0x04 != 0,
            (2, false) => self.second_write = false,
            (5, true) => self.second_write = !self.second_write,
            (6, true) => {
                if self.second_write {
                    self.v = (u16::from(self.t_hi) << 8) | u16::from(value);
                } else {
                    self.t_hi = value & 0x3F;
                }
                self.second_write = !self.second_write;
            }
            (7, _) => {
                let chr = write && (self.v & 0x3FFF) < 0x2000;
                self.v = self.v.wrapping_add(if self.increment_32 { 32 } else { 1 }) & 0x7FFF;
                return chr;
            }
            _ => {}
        }
        false
    }
}

fn observe_board(
    rom: &[u8],
    h: &rustynes_core::rustynes_mappers::Header,
    frames: u32,
    findings: &mut Vec<Finding>,
) -> Option<BoardObservation> {
    let mut nes = match Nes::from_rom(rom) {
        Ok(n) => n,
        Err(e) => {
            findings.push(Finding {
                severity: Severity::BadDump,
                check: "core",
                message: format!("the core refuses this image as its header describes it: {e}"),
            });
            return None;
        }
    };
    nes.set_access_logging(true);
    let mut obs = BoardObservation {
        frames,
        ..BoardObservation::default()
    };
    let mut ppu = PpuAddrModel::default();
    for _ in 0..frames {
        nes.run_frame();
        for a in nes.accesses() {
            if a.write && a.addr >= 0x8000 {
                obs.rom_writes += 1;
                obs.first_rom_write.get_or_insert((a.addr, a.value));
            }
            if a.write && (0x6000..0x8000).contains(&a.addr) {
                obs.wram_writes += 1;
            }
            if ppu.access(a.write, a.addr, a.value) {
                obs.chr_writes += 1;
            }
        }
    }
    if h.mapper_id == 0
        && obs.rom_writes > 0
        && let Some((addr, value)) = obs.first_rom_write
    {
        findings.push(Finding {
            severity: Severity::WrongHeader,
            check: "board",
            message: format!(
                "the header says NROM (mapper 0), which has no registers, but in {frames} frames \
                 the program wrote to $8000-$FFFF {} times (first ${addr:04X} = ${value:02X}): \
                 it expects a board with bank registers",
                obs.rom_writes
            ),
        });
    }
    if h.chr_size > 0
        && CHR_ROM_ONLY_BOARDS.contains(&h.mapper_id)
        && obs.chr_writes >= CHR_WRITE_THRESHOLD
    {
        findings.push(Finding {
            severity: Severity::WrongHeader,
            check: "board",
            message: format!(
                "the header declares CHR-ROM ({}), which ignores writes, but in {frames} frames \
                 the program wrote {} bytes into the pattern tables through $2007: the board \
                 probably has CHR-RAM",
                kib(h.chr_size),
                obs.chr_writes
            ),
        });
    }
    if h.is_nes2
        && h.mapper_id == 0
        && h.prg_ram_size == 0
        && h.prg_nvram_size == 0
        && obs.wram_writes > 0
    {
        findings.push(Finding {
            severity: Severity::WrongHeader,
            check: "board",
            message: format!(
                "the NES 2.0 header declares no PRG-RAM, but in {frames} frames the program \
                 wrote to $6000-$7FFF {} times: it expects work RAM there",
                obs.wram_writes
            ),
        });
    }
    Some(obs)
}

/// Read a file with the [`MAX_INPUT_BYTES`] cap.
///
/// # Errors
/// An I/O error, or a file over the cap.
pub fn read_capped(path: &Path) -> Result<Vec<u8>, String> {
    let len = std::fs::metadata(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?
        .len();
    if len > MAX_INPUT_BYTES {
        return Err(format!(
            "{} is {len} bytes, over the {MAX_INPUT_BYTES}-byte limit",
            path.display()
        ));
    }
    std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

/// Render the report as the text `rustynes dump-check` prints.
// One `writeln!` per report line, in print order.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn render(path: &str, r: &DumpReport) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "Dump check: {} ({} bytes)", printable(path), r.len);
    let _ = writeln!(
        s,
        "  header        {}",
        match (r.header_summary.as_deref(), r.nes2) {
            (Some(summary), _) => summary,
            (None, None) => "none (not an iNES / NES 2.0 image: only the hashes and the DAT apply)",
            (None, Some(_)) => "unreadable",
        }
    );
    let span = if r.nes2.is_some() {
        "after the header"
    } else {
        "whole file"
    };
    let _ = writeln!(
        s,
        "  SHA-256       {} ({span})",
        hex::encode(r.sha256_headerless)
    );
    let _ = writeln!(s, "  CRC-32        {:08X} ({span})", r.crc_headerless);
    if r.nes2.is_some() {
        let _ = writeln!(
            s,
            "  SHA-256       {} (whole file)",
            hex::encode(r.sha256_full)
        );
        let _ = writeln!(s, "  CRC-32        {:08X} (whole file)", r.crc_full);
    }
    let conv = |headered: bool| {
        if headered {
            "whole file, headered DAT"
        } else {
            "headerless"
        }
    };
    let _ = match &r.dat {
        DatMatch::NotSupplied => writeln!(
            s,
            "  DAT           not supplied (--dat): the dump is unverified"
        ),
        DatMatch::Verified { game, headered } => {
            writeln!(
                s,
                "  DAT           VERIFIED by SHA-256 ({}): \"{game}\"",
                conv(*headered)
            )
        }
        DatMatch::CrcOnly { game, headered } => writeln!(
            s,
            "  DAT           CRC-32 and size match ({}), the row has no SHA-256: \"{game}\"",
            conv(*headered)
        ),
        DatMatch::NotListed { rows } => {
            writeln!(s, "  DAT           not listed ({rows} rows searched)")
        }
    };
    let _ = match &r.nes20db {
        Nes20Match::NotSupplied => writeln!(s, "  NES 2.0 DB    not supplied (--nes20db)"),
        Nes20Match::NotApplicable => writeln!(s, "  NES 2.0 DB    not applicable (no header)"),
        Nes20Match::NotListed => writeln!(s, "  NES 2.0 DB    not listed"),
        Nes20Match::Listed {
            name,
            diffs,
            skipped,
            mirroring_compared,
        } => writeln!(
            s,
            "  NES 2.0 DB    \"{name}\": {}{}{}",
            if diffs.is_empty() {
                "every compared field agrees".to_string()
            } else {
                format!("{} field(s) disagree", diffs.len())
            },
            if skipped.is_empty() {
                String::new()
            } else {
                format!(
                    " ({} not compared: iNES 1.0 cannot express them)",
                    skipped.len()
                )
            },
            if *mirroring_compared {
                ""
            } else {
                " (mirroring not compared: the board switches it)"
            }
        ),
    };
    if let Some(b) = &r.board {
        let _ = writeln!(
            s,
            "  board         {} frames: {} $8000-$FFFF writes, {} pattern-table writes, {} \
             $6000-$7FFF writes",
            b.frames, b.rom_writes, b.chr_writes, b.wram_writes
        );
    }
    if r.findings.is_empty() {
        let _ = writeln!(s, "Findings: none");
    } else {
        let _ = writeln!(s, "Findings ({}):", r.findings.len());
        for f in &r.findings {
            let _ = writeln!(s, "  [{}] {}: {}", f.check, f.severity.label(), f.message);
        }
    }
    let verdict = match r.verdict() {
        None if matches!(r.dat, DatMatch::Verified { .. }) => "GOOD (verified against the DAT)",
        None => "NO PROBLEM FOUND (not verified: supply a DAT)",
        Some(sev) => sev.label(),
    };
    let _ = writeln!(s, "Verdict: {verdict}");
    s
}

// ---------------------------------------------------------------------------
// A minimal XML tag scanner: enough for Logiqx DATs and nes20db.xml, never
// panicking on malformed input. Not a validating parser.
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum XmlItem<'a> {
    Open {
        name: &'a str,
        attrs: Vec<(String, String)>,
        self_closing: bool,
    },
    Close(&'a str),
    Comment(&'a str),
}

struct XmlScanner<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> XmlScanner<'a> {
    const fn new(text: &'a str) -> Self {
        Self { text, pos: 0 }
    }
}

impl<'a> Iterator for XmlScanner<'a> {
    type Item = XmlItem<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let rest = self.text.get(self.pos..)?;
            let lt = rest.find('<')?;
            let start = self.pos + lt;
            let tag = &self.text[start..];
            if let Some(body) = tag.strip_prefix("<!--") {
                let end = body.find("-->")?;
                self.pos = start + 4 + end + 3;
                return Some(XmlItem::Comment(&body[..end]));
            }
            if tag.starts_with("<?") || tag.starts_with("<!") {
                let end = tag.find('>')?;
                self.pos = start + end + 1;
                continue;
            }
            if let Some(body) = tag.strip_prefix("</") {
                let end = body.find('>')?;
                self.pos = start + 2 + end + 1;
                return Some(XmlItem::Close(body[..end].trim()));
            }
            // An opening tag: the name, then attributes up to an unquoted '>'.
            let body = &tag[1..];
            let name_len = body
                .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                .unwrap_or(body.len());
            let name = &body[..name_len];
            let mut i = name_len;
            let bytes = body.as_bytes();
            let mut attrs = Vec::new();
            let mut self_closing = false;
            loop {
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i >= bytes.len() {
                    self.pos = self.text.len();
                    return None;
                }
                match bytes[i] {
                    b'>' => {
                        i += 1;
                        break;
                    }
                    b'/' => {
                        self_closing = true;
                        i += 1;
                        continue;
                    }
                    _ => {}
                }
                let key_start = i;
                while i < bytes.len()
                    && !bytes[i].is_ascii_whitespace()
                    && !matches!(bytes[i], b'=' | b'>' | b'/')
                {
                    i += 1;
                }
                let key = &body[key_start..i];
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'=' {
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if i < bytes.len() && matches!(bytes[i], b'"' | b'\'') {
                        let q = bytes[i];
                        let v_start = i + 1;
                        let Some(len) = bytes[v_start..].iter().position(|&b| b == q) else {
                            self.pos = self.text.len();
                            return None;
                        };
                        attrs.push((key.to_string(), unescape(&body[v_start..v_start + len])));
                        i = v_start + len + 1;
                    }
                } else if key.is_empty() {
                    // A byte that starts nothing: skip it rather than loop.
                    i += 1;
                }
            }
            self.pos = start + 1 + i;
            if name.is_empty() {
                continue;
            }
            return Some(XmlItem::Open {
                name,
                attrs,
                self_closing,
            });
        }
    }
}

/// Make a string from an input file safe to print on a terminal: every
/// control character (C0, DEL and C1, `char::is_control`) becomes a visible
/// `\u{..}` escape. A crafted DAT or database could otherwise put ESC or CSI
/// (U+009B) sequences in a game name and drive the terminal that shows the
/// report. Applied once, where such a string enters the report (DAT game
/// names, database names and mirroring values, the ROM path), so every later
/// use is already clean.
fn printable(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_control() {
            let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    out
}

fn attr(attrs: &[(String, String)], key: &str) -> Option<String> {
    attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

/// Decode the five predefined entities and numeric character references;
/// anything else is kept as written.
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(semi) = rest.find(';') else { break };
        let ent = &rest[1..semi];
        let decoded = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .or_else(|| ent.strip_prefix("#X"))
                .map_or_else(
                    || ent.strip_prefix('#').and_then(|d| d.parse::<u32>().ok()),
                    |h| u32::from_str_radix(h, 16).ok(),
                )
                .and_then(char::from_u32),
        };
        if let Some(c) = decoded {
            out.push(c);
            rest = &rest[semi + 1..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

fn parse_hex_u32(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() || s.len() > 8 {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}

fn parse_hex_32(s: &str) -> Option<[u8; 32]> {
    let v = hex::decode(s.trim()).ok()?;
    v.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Planted fixtures: tiny NROM programs built here, never a dump. ---

    /// An NROM-128 image (16 KiB PRG at $C000, mirrored at $8000) whose code
    /// is `program` at $C000, with NMI/IRQ vectors at an RTI and RESET at
    /// $C000, and `chr` as CHR-ROM (empty = CHR-RAM).
    fn nrom(program: &[u8], chr_banks: u8, flags6: u8) -> Vec<u8> {
        let mut prg = vec![0u8; 16 * 1024];
        prg[..program.len()].copy_from_slice(program);
        prg[0x3FF0] = 0x40; // RTI at $FFF0
        prg[0x3FFA..0x3FFC].copy_from_slice(&[0xF0, 0xFF]); // NMI
        prg[0x3FFC..0x3FFE].copy_from_slice(&[0x00, 0xC0]); // RESET
        prg[0x3FFE..0x4000].copy_from_slice(&[0xF0, 0xFF]); // IRQ
        // Distinct data in the first half so the PRG does not repeat.
        for (i, b) in prg[0x100..0x2000].iter_mut().enumerate() {
            *b = u8::try_from((i * 7 + 3) % 256).unwrap_or(0);
        }
        let mut rom = b"NES\x1A".to_vec();
        rom.extend_from_slice(&[1, chr_banks, flags6, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        rom.extend_from_slice(&prg);
        for i in 0..usize::from(chr_banks) * 8 * 1024 {
            rom.push(u8::try_from(i % 251).unwrap_or(0));
        }
        rom
    }

    /// Wait for vblank twice, then loop forever.
    const IDLE: &[u8] = &[
        0x2C, 0x02, 0x20, 0x10, 0xFB, // bit $2002; bpl
        0x2C, 0x02, 0x20, 0x10, 0xFB, // bit $2002; bpl
        0x4C, 0x0A, 0xC0, // jmp $C00A
    ];

    /// Wait for vblank twice, point the PPU at $0000, write 1 KiB through
    /// $2007 (a CHR-RAM upload: 4 x 256), then loop.
    const CHR_UPLOAD: &[u8] = &[
        0x2C, 0x02, 0x20, 0x10, 0xFB, // $C000 bit $2002; bpl $C000
        0x2C, 0x02, 0x20, 0x10, 0xFB, // $C005 bit $2002; bpl $C005
        0xA9, 0x00, // $C00A lda #0
        0x8D, 0x06, 0x20, // $C00C sta $2006
        0x8D, 0x06, 0x20, // $C00F sta $2006
        0xA0, 0x04, // $C012 ldy #4
        0xA2, 0x00, // $C014 ldx #0
        0x8E, 0x07, 0x20, // $C016 stx $2007
        0xE8, // $C019 inx
        0xD0, 0xFA, // $C01A bne $C016
        0x88, // $C01C dey
        0xD0, 0xF5, // $C01D bne $C014
        0x4C, 0x1F, 0xC0, // $C01F jmp $C01F
    ];

    /// Write a bank number to $8000 (a register an NROM board does not have).
    const BANK_WRITE: &[u8] = &[
        0xA9, 0x01, // lda #1
        0x8D, 0x00, 0x80, // sta $8000
        0x4C, 0x05, 0xC0, // jmp $C005
    ];

    /// Write $6000 (work RAM), then loop.
    const WRAM_WRITE: &[u8] = &[
        0xA9, 0x01, // $C000 lda #1
        0x8D, 0x00, 0x60, // $C002 sta $6000
        0x4C, 0x05, 0xC0, // $C005 jmp $C005
    ];

    fn sha_hex(b: &[u8]) -> String {
        hex::encode(Sha256::digest(b))
    }

    fn dat_for(rom: &[u8], name: &str) -> Dat {
        let text = format!(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE datafile>\n<datafile><header><name>t</name></header>\n\
             <game name=\"{name}\"><description>{name}</description>\
             <rom name=\"x.nes\" size=\"{}\" crc=\"{:08X}\" sha256=\"{}\" status=\"verified\"/></game>\n\
             <game name=\"Other &amp; Co\"><rom name=\"y.nes\" size=\"16\" crc=\"00000000\"/></game>\n\
             </datafile>",
            rom.len() - 16,
            rustynes_gamedb::crc32(&rom[16..]),
            sha_hex(&rom[16..])
        );
        Dat::parse(&text).expect("dat")
    }

    fn run(rom: &[u8], dat: Option<&Dat>, db: Option<&Nes20Db>) -> DumpReport {
        check(
            rom,
            &CheckOptions {
                dat,
                nes20db: db,
                frames: 30,
            },
        )
    }

    /// The plan's gate: a good dump, an overdump and a wrong header each get
    /// the right verdict.
    #[test]
    fn planted_good_dump_is_verified_and_clean() {
        let rom = nrom(IDLE, 1, 0x01);
        let dat = dat_for(&rom, "Good (World)");
        let r = run(&rom, Some(&dat), None);
        assert_eq!(r.findings, vec![], "{}", render("good", &r));
        assert_eq!(
            r.dat,
            DatMatch::Verified {
                game: "Good (World)".into(),
                headered: false
            }
        );
        assert!(render("good", &r).contains("Verdict: GOOD"));
    }

    #[test]
    fn planted_overdump_is_a_bad_dump() {
        // The good program's 16 KiB PRG read twice into a 32 KiB image.
        let good = nrom(IDLE, 1, 0x01);
        let mut rom = good[..16].to_vec();
        rom[4] = 2;
        rom.extend_from_slice(&good[16..16 + 16 * 1024]);
        rom.extend_from_slice(&good[16..16 + 16 * 1024]);
        rom.extend_from_slice(&good[16 + 16 * 1024..]);
        let r = run(&rom, None, None);
        assert_eq!(
            r.verdict(),
            Some(Severity::BadDump),
            "{}",
            render("over", &r)
        );
        let f = r
            .findings
            .iter()
            .find(|f| f.check == "overdump")
            .expect("overdump finding");
        assert!(
            f.message
                .contains("PRG-ROM is 32 KiB whose content repeats every 16 KiB"),
            "{}",
            f.message
        );
        // And the good one's 16 KiB PRG is not flagged.
        assert!(
            run(&good, None, None)
                .findings
                .iter()
                .all(|f| f.check != "overdump")
        );
    }

    #[test]
    fn planted_chr_ram_game_with_a_chr_rom_header_is_a_wrong_header() {
        let rom = nrom(CHR_UPLOAD, 1, 0x01);
        let r = run(&rom, None, None);
        assert_eq!(
            r.verdict(),
            Some(Severity::WrongHeader),
            "{}",
            render("chr", &r)
        );
        assert_eq!(r.board.as_ref().map(|b| b.chr_writes), Some(1024));
        assert!(
            r.findings
                .iter()
                .any(|f| f.check == "board" && f.message.contains("CHR-RAM"))
        );
        // The same program with a CHR-RAM header is consistent.
        let ok = nrom(CHR_UPLOAD, 0, 0x01);
        assert_eq!(run(&ok, None, None).findings, vec![]);
    }

    #[test]
    fn planted_bank_switching_game_with_an_nrom_header_is_a_wrong_header() {
        let rom = nrom(BANK_WRITE, 1, 0x01);
        let r = run(&rom, None, None);
        assert_eq!(
            r.verdict(),
            Some(Severity::WrongHeader),
            "{}",
            render("bank", &r)
        );
        let f = r
            .findings
            .iter()
            .find(|f| f.check == "board")
            .expect("board finding");
        assert!(f.message.contains("first $8000 = $01"), "{}", f.message);
    }

    #[test]
    fn a_header_that_disagrees_with_the_nes20db_entry_is_named_field_by_field() {
        let rom = nrom(IDLE, 1, 0x01); // vertical
        let db = Nes20Db::parse(&format!(
            "<nes20db><game><!-- Planted\\Good.nes -->\
             <prgrom size=\"16384\" crc32=\"0\"/><chrrom size=\"8192\" crc32=\"0\"/>\
             <rom size=\"{}\" crc32=\"{:08X}\"/>\
             <pcb mapper=\"0\" submapper=\"0\" mirroring=\"H\" battery=\"1\"/>\
             <console type=\"0\" region=\"0\"/><expansion type=\"1\"/></game></nes20db>",
            rom.len() - 16,
            rustynes_gamedb::crc32(&rom[16..])
        ))
        .expect("db");
        let r = run(&rom, None, Some(&db));
        let Nes20Match::Listed {
            diffs,
            skipped,
            name,
            mirroring_compared,
        } = &r.nes20db
        else {
            panic!("{:?}", r.nes20db)
        };
        assert_eq!(name, "Planted\\Good.nes");
        let fields: Vec<_> = diffs.iter().map(|d| d.0).collect();
        assert_eq!(fields, ["mirroring", "battery"]);
        assert_eq!(*skipped, INES1_SKIPPED);
        assert!(*mirroring_compared, "NROM's mirroring is hardwired");
        assert_eq!(r.verdict(), Some(Severity::WrongHeader));
        // The matching header has no diffs.
        let mut fixed = rom;
        fixed[6] = 0x02; // horizontal, battery
        let db2 = Nes20Db::parse(&format!(
            "<nes20db><game><!-- G --><prgrom size=\"16384\"/><chrrom size=\"8192\"/>\
             <rom size=\"{}\" crc32=\"{:08X}\"/><pcb mapper=\"0\" mirroring=\"H\" battery=\"1\"/>\
             </game></nes20db>",
            fixed.len() - 16,
            rustynes_gamedb::crc32(&fixed[16..])
        ))
        .unwrap();
        let r2 = check(
            &fixed,
            &CheckOptions {
                dat: None,
                nes20db: Some(&db2),
                frames: 0,
            },
        );
        assert!(
            matches!(&r2.nes20db, Nes20Match::Listed { diffs, .. } if diffs.is_empty()),
            "{:?}",
            r2.nes20db
        );
    }

    #[test]
    fn a_nes2_header_is_compared_on_its_extra_fields() {
        let mut rom = nrom(IDLE, 1, 0x01);
        rom[7] = 0x08; // NES 2.0
        rom[10] = 0x07; // PRG-RAM 8 KiB
        rom[12] = 0x01; // PAL
        rom[15] = 0x02; // Four Score (the database says 1, standard controllers)
        let db = Nes20Db::parse(&format!(
            "<nes20db><game><!-- N2 --><prgrom size=\"16384\"/><chrrom size=\"8192\"/>\
             <rom size=\"{}\" crc32=\"{:08X}\"/><pcb mapper=\"0\" submapper=\"0\" mirroring=\"V\" battery=\"0\"/>\
             <console type=\"0\" region=\"0\"/><expansion type=\"1\"/></game></nes20db>",
            rom.len() - 16,
            rustynes_gamedb::crc32(&rom[16..])
        ))
        .unwrap();
        let r = check(
            &rom,
            &CheckOptions {
                dat: None,
                nes20db: Some(&db),
                frames: 0,
            },
        );
        let Nes20Match::Listed { diffs, skipped, .. } = &r.nes20db else {
            panic!()
        };
        let got: Vec<_> = diffs
            .iter()
            .map(|d| (d.0, d.1.as_str(), d.2.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("PRG-RAM", "8192", "0"),
                ("region", "1", "0"),
                ("expansion device", "2", "1")
            ]
        );
        assert_eq!(*skipped, &[] as &[&str]);
    }

    #[test]
    fn size_findings_truncated_and_trailing() {
        let rom = nrom(IDLE, 1, 0x01);
        let r = run(&rom[..rom.len() - 100], None, None);
        assert!(
            r.findings
                .iter()
                .any(|f| f.check == "size" && f.message.starts_with("truncated"))
        );
        let mut long = rom;
        long.extend_from_slice(&[0; 64]);
        let r = run(&long, None, None);
        assert!(
            r.findings
                .iter()
                .any(|f| f.check == "size" && f.message.starts_with("64 bytes follow"))
        );
    }

    #[test]
    fn dat_conventions_and_weaker_crc_match() {
        let rom = nrom(IDLE, 1, 0x01);
        // A headered DAT: whole-file hashes.
        let text = format!(
            "<datafile><game name=\"H\"><rom size=\"{}\" crc=\"{:08x}\" sha256=\"{}\"/></game></datafile>",
            rom.len(),
            rustynes_gamedb::crc32(&rom),
            sha_hex(&rom)
        );
        let r = run(&rom, Some(&Dat::parse(&text).unwrap()), None);
        assert_eq!(
            r.dat,
            DatMatch::Verified {
                game: "H".into(),
                headered: true
            }
        );
        // No SHA-256 in the row: CRC and size only.
        let text = format!(
            "<datafile><machine name='C'><rom size='{}' crc='{:08X}'/></machine></datafile>",
            rom.len() - 16,
            rustynes_gamedb::crc32(&rom[16..])
        );
        let r = run(&rom, Some(&Dat::parse(&text).unwrap()), None);
        assert_eq!(
            r.dat,
            DatMatch::CrcOnly {
                game: "C".into(),
                headered: false
            }
        );
        assert_eq!(r.findings, vec![]);
        assert!(render("c", &r).contains("NO PROBLEM FOUND (not verified"));
        // A wrong size with the right CRC is not a match.
        let text = format!(
            "<datafile><game name='C'><rom size='1' crc='{:08X}'/></game></datafile>",
            rustynes_gamedb::crc32(&rom[16..])
        );
        let r = run(&rom, Some(&Dat::parse(&text).unwrap()), None);
        assert_eq!(r.dat, DatMatch::NotListed { rows: 1 });
        assert_eq!(r.verdict(), Some(Severity::Unverified));
    }

    #[test]
    fn a_headerless_image_gets_only_the_hashes_and_the_dat() {
        let fds = b"\x01*NINTENDO-HVC*\x01rest of a disk".to_vec();
        let text = format!(
            "<datafile><game name='Disk'><rom size='{}' sha256='{}'/></game></datafile>",
            fds.len(),
            sha_hex(&fds)
        );
        let r = run(&fds, Some(&Dat::parse(&text).unwrap()), None);
        assert_eq!(r.nes2, None);
        assert!(r.board.is_none());
        assert!(matches!(
            r.dat,
            DatMatch::Verified {
                headered: false,
                ..
            }
        ));
        assert!(render("d", &r).contains("not an iNES / NES 2.0 image"));
    }

    #[test]
    fn repeat_period_halves_only_while_identical_and_not_below_the_floor() {
        let a: Vec<u8> = (0..8192u32)
            .map(|i| u8::try_from((i * 13) % 256).unwrap_or(0))
            .collect();
        let doubled = [a.clone(), a.clone()].concat();
        assert_eq!(repeat_period(&doubled, 8192), 8192);
        assert_eq!(
            repeat_period(&[doubled.clone(), doubled.clone()].concat(), 8192),
            8192
        );
        assert_eq!(repeat_period(&a, 8192), 8192);
        let mut almost = doubled;
        almost[16383] ^= 1;
        assert_eq!(repeat_period(&almost, 8192), 16384);
        assert_eq!(repeat_period(&vec![0; 16384], 8192), 8192);
    }

    #[test]
    fn the_ppu_address_model_counts_only_pattern_table_writes() {
        let mut m = PpuAddrModel::default();
        // $2006 = $3F00 (palette), one write: not CHR.
        assert!(!m.access(true, 0x2006, 0x3F));
        assert!(!m.access(true, 0x2006, 0x00));
        assert!(!m.access(true, 0x2007, 0x0F));
        // $2006 = $1FFF: one CHR write, then $2000: not.
        m.access(true, 0x2006, 0x1F);
        m.access(true, 0x2006, 0xFF);
        assert!(m.access(true, 0x2007, 1));
        assert!(!m.access(true, 0x2007, 1));
        // A $2002 read resets the latch: the next $2006 is a first write.
        m.access(true, 0x2006, 0x00);
        m.access(false, 0x2002, 0);
        m.access(true, 0x2006, 0x00);
        m.access(true, 0x2006, 0x10);
        assert!(m.access(true, 0x200F, 0)); // a mirror of $2007, at $0010
        // Increment by 32 from $2000 bit 2.
        m.access(true, 0x2000, 0x04);
        m.access(true, 0x2006, 0x1F);
        m.access(true, 0x2006, 0xE0);
        assert!(m.access(true, 0x2007, 0)); // $1FE0
        assert!(!m.access(true, 0x2007, 0)); // $2000
    }

    /// Corpus-driven exemptions (each found as a false positive on the local
    /// dump corpus before it existed).
    #[test]
    fn exemptions_playchoice_data_unspecified_expansion_switched_mirroring() {
        // iNES 1.0 PlayChoice-10: 8,224 bytes of hint ROM + key data follow.
        let mut pc10 = nrom(IDLE, 1, 0x01);
        pc10[7] = 0x02;
        pc10.extend_from_slice(&[0x55; 8192 + 32]);
        assert!(
            run(&pc10, None, None)
                .findings
                .iter()
                .all(|f| f.check != "size")
        );
        // ...but only that amount, and only with the flag.
        let mut odd = pc10.clone();
        odd.push(0);
        assert!(
            run(&odd, None, None)
                .findings
                .iter()
                .any(|f| f.check == "size")
        );

        let db_for = |rom: &[u8], mirroring: &str, expansion: u8| {
            Nes20Db::parse(&format!(
                "<nes20db><game><!-- G --><prgrom size=\"16384\"/><chrrom size=\"8192\"/>\
                 <rom size=\"{}\" crc32=\"{:08X}\"/><pcb mapper=\"{}\" submapper=\"0\" \
                 mirroring=\"{mirroring}\" battery=\"0\"/><console type=\"0\" region=\"0\"/>\
                 <expansion type=\"{expansion}\"/></game></nes20db>",
                rom.len() - 16,
                rustynes_gamedb::crc32(&rom[16..]),
                (rom[6] >> 4) | (rom[7] & 0xF0)
            ))
            .unwrap()
        };
        // NES 2.0 expansion device 0 (unspecified) is not a disagreement.
        let mut n2 = nrom(IDLE, 1, 0x01);
        n2[7] = 0x08;
        let r = check(
            &n2,
            &CheckOptions {
                dat: None,
                nes20db: Some(&db_for(&n2, "V", 1)),
                frames: 0,
            },
        );
        assert!(
            matches!(&r.nes20db, Nes20Match::Listed { diffs, .. } if diffs.is_empty()),
            "{:?}",
            r.nes20db
        );
        n2[15] = 0x02; // a header that does say: compared
        let r = check(
            &n2,
            &CheckOptions {
                dat: None,
                nes20db: Some(&db_for(&n2, "V", 1)),
                frames: 0,
            },
        );
        assert!(
            matches!(&r.nes20db, Nes20Match::Listed { diffs, .. } if diffs.len() == 1),
            "{:?}",
            r.nes20db
        );

        // MMC1 (mapper 1) switches its mirroring: the header bit is unused.
        let mmc1 = nrom(IDLE, 1, 0x11); // mapper 1, vertical
        let r = check(
            &mmc1,
            &CheckOptions {
                dat: None,
                nes20db: Some(&db_for(&mmc1, "H", 1)),
                frames: 0,
            },
        );
        assert!(
            matches!(&r.nes20db, Nes20Match::Listed { diffs, mirroring_compared: false, .. } if diffs.is_empty()),
            "{:?}",
            r.nes20db
        );
    }

    #[test]
    fn the_chr_ram_rule_judges_only_boards_without_both() {
        // CNROM (3) cannot carry CHR-RAM beside CHR-ROM: judged.
        let mut cnrom = nrom(CHR_UPLOAD, 1, 0x31);
        cnrom[7] = 0x00;
        assert!(
            run(&cnrom, None, None)
                .findings
                .iter()
                .any(|f| f.check == "board")
        );
        // Mapper 87 is not on the list: not judged, whatever it writes.
        let mut m87 = nrom(CHR_UPLOAD, 1, 0x71);
        m87[7] = 0x50;
        let r = run(&m87, None, None);
        assert_eq!(r.board.as_ref().map(|b| b.chr_writes), Some(1024));
        assert!(
            r.findings.iter().all(|f| f.check != "board"),
            "{:?}",
            r.findings
        );
        // Below the threshold: not judged on a listed board either.
        const { assert!(CHR_WRITE_THRESHOLD > 129) };
    }

    #[test]
    fn work_ram_writes_contradict_only_a_nes2_nrom_header_without_prg_ram() {
        let mut rom = nrom(WRAM_WRITE, 1, 0x01);
        rom[7] = 0x08; // NES 2.0, PRG-RAM nibble 0: none
        let r = run(&rom, None, None);
        assert_eq!(r.board.as_ref().map(|b| b.wram_writes), Some(1));
        assert!(
            r.findings
                .iter()
                .any(|f| f.check == "board" && f.message.contains("no PRG-RAM")),
            "{:?}",
            r.findings
        );
        rom[10] = 0x07; // 8 KiB declared: consistent
        assert!(
            run(&rom, None, None)
                .findings
                .iter()
                .all(|f| f.check != "board")
        );
        // iNES 1.0 cannot say "none", so it is not judged.
        let ines1 = nrom(WRAM_WRITE, 1, 0x01);
        assert!(
            run(&ines1, None, None)
                .findings
                .iter()
                .all(|f| f.check != "board")
        );
    }

    #[test]
    fn a_row_whose_sha256_differs_is_not_a_crc_match() {
        let rom = nrom(IDLE, 1, 0x01);
        let text = format!(
            "<datafile><game name='X'><rom size='{}' crc='{:08X}' sha256='{}'/></game></datafile>",
            rom.len() - 16,
            rustynes_gamedb::crc32(&rom[16..]),
            "00".repeat(32)
        );
        let r = run(&rom, Some(&Dat::parse(&text).unwrap()), None);
        assert_eq!(r.dat, DatMatch::NotListed { rows: 1 });
    }

    /// A crafted DAT or database cannot put a raw control character (an
    /// ESC sequence, a C1 CSI) into the printed report.
    #[test]
    fn file_strings_render_with_no_raw_control_character() {
        let rom = nrom(IDLE, 1, 0x01);
        let evil = "Evil&#x1b;[2J&#x9b;31m&#7;";
        let dat = Dat::parse(&format!(
            "<datafile><game name='{evil}'><rom size='{}' sha256='{}'/></game></datafile>",
            rom.len() - 16,
            sha_hex(&rom[16..])
        ))
        .unwrap();
        let db = Nes20Db::parse(&format!(
            "<nes20db><game><!-- N\u{1b}[2J\u{9b}x --><prgrom size='16384'/>\
             <rom size='{}' crc32='{:08X}'/><pcb mapper='0' mirroring='H\u{1b}'/></game></nes20db>",
            rom.len() - 16,
            rustynes_gamedb::crc32(&rom[16..])
        ))
        .unwrap();
        let r = check(
            &rom,
            &CheckOptions {
                dat: Some(&dat),
                nes20db: Some(&db),
                frames: 0,
            },
        );
        let out = render("rom\u{1b}[2J.nes", &r);
        assert!(!out.chars().any(|c| c.is_control() && c != '\n'), "{out:?}");
        assert!(out.contains("Evil\\u{1b}[2J\\u{9b}31m\\u{7}"), "{out}");
        assert!(out.contains("N\\u{1b}[2J\\u{9b}x"), "{out}");
        assert!(out.contains("rom\\u{1b}[2J.nes"), "{out}");
    }

    #[test]
    fn the_xml_scanner_survives_malformed_input() {
        for junk in [
            "",
            "<",
            "<<<>>>",
            "<game",
            "<game name=\"x",
            "<!-- unterminated",
            "</",
            "<a b=c d>",
            "<rom size=\"9999999999999999999999\" crc=\"zz\" sha256=\"00\"/>",
            "&#xFFFFFFFF;",
            "<game name='&#0;&bogus;&'><rom size='1'/></game>",
        ] {
            let _ = XmlScanner::new(junk).count();
            let _ = Dat::parse(junk);
            let _ = Nes20Db::parse(junk);
        }
        assert_eq!(unescape("a&amp;b&lt;&#65;&#x42;&zz;&"), "a&b<AB&zz;&");
        assert!(Dat::parse("<datafile></datafile>").is_err());
        assert!(Nes20Db::parse("<nes20db><game></game></nes20db>").is_err());
    }
}

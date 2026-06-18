//! v1.2.0 beta.2 (Workstream C3) — HD-pack / mod loader.
//! v1.3.0 (Workstream E1) — `<condition>` memory-gating + `<background>` regions.
//!
//! Loads a Mesen-style HD-pack (a folder or a `.zip` containing a `hires.txt`)
//! and substitutes hi-res replacement tiles at blit time. The v1.2.0 first cut
//! handled ONLY unconditional CHR-hash tile replacement; v1.3.0 Workstream E1
//! adds the two deferred Mesen capabilities:
//!
//! - `<condition>` declarations — at minimum **memoryCheck** /
//!   **memoryCheckConstant** (a `(watched[addr] & mask) <op> value` test against a
//!   per-frame snapshot of watched CPU/PPU addresses) and **frameRange** (a
//!   `frame % period >= offset` test). The cheap per-tile checks that read data
//!   already present in the PPU `HdTileSource` telemetry are also supported:
//!   **hmirror** / **vmirror** (sprite H/V flip) and **sppalette** (sprite
//!   palette group).
//! - `<tile>` rules may now carry a trailing **condition-name reference**
//!   (comma- or `&`-joined for AND); the substitution is gated on all referenced
//!   conditions holding.
//! - `<background>` rules — a full-screen (or rectangular) replacement image
//!   alpha-blended under/over the tile pass, optionally gated on a condition and
//!   ordered by priority.
//!
//! v1.6.0 "Studio" Workstream H — **HD audio**: the `<bgm>` / `<sfx>`
//! declarations are now parsed here (see [`crate::hd_audio`]). They name an
//! external OGG track keyed by an `(album, track)` selector the game chooses at
//! run time via the `$4100` HD-pack audio-control register. The decode + mixer
//! live in [`crate::hd_audio`]; this module only surfaces the parsed
//! declarations (so the loader can decode them) — the audio path is entirely
//! frontend-side + output-only and never touches the compositor / framebuffer.
//!
//! Still SKIPPED (not full Mesen parity — see `docs/adr/0014`): the
//! position/tile/sprite spatial conditions (TileNearby/TileAtPos,
//! SpriteNearby/SpriteAtPos, PositionCheckX/Y), `bgpriority` (the PPU telemetry
//! has no background-priority bit yet), `<overlay>`, `<addition>`/`<fallback>`/
//! `<options>`, the full blend/priority/parallax compositor, and the per-track
//! `<bgmCondition>` gate (the `$4100` selector drives BGM/SFX instead).
//! Unsupported rules are ignored rather than rejected, so a real pack still
//! loads (just with the unsupported rules inert).
//!
//! ## Determinism
//!
//! This module is presentation-only and native-only. It consumes the PPU's
//! feature-gated [`rustynes_core::rustynes_ppu::HdTileSource`] telemetry (which
//! is itself output-only) and a per-frame snapshot of the finite set of watched
//! memory addresses referenced by the parsed conditions. Both are reads of
//! already-deterministic state taken at PRODUCE time (under the emu lock); the
//! compositor itself only reads them. It mutates no emulation state and adds no
//! determinism surface. When no pack is loaded — or the `hd-pack` feature is off
//! — the presentation is byte-identical to the stock build.

#![cfg(feature = "hd-pack")]

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use rustynes_core::rustynes_ppu::{HD_TILE_NONE, HdTileSource};

use crate::gfx::{NES_H, NES_W};
use crate::hd_audio::{HdAudioDecl, TrackKind, parse_audio_decl};

/// NES tiles are 8x8.
const TILE: usize = 8;
/// Visible tile grid: 32 columns x 30 rows.
const COLS: usize = NES_W as usize / TILE; // 32
const ROWS: usize = NES_H as usize / TILE; // 30

/// Bit 31 of a watched-memory address marks a PPU-space (vs CPU-space) address,
/// matching Mesen's `HdPackBaseMemoryCondition::PpuMemoryMarker`.
pub const PPU_MEMORY_MARKER: u32 = 0x8000_0000;

/// A decoded replacement image (RGBA8, row-major).
#[derive(Debug)]
struct ReplacementImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// A comparison operator for a memory / range condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CmpOp {
    Eq,
    Ne,
    Gt,
    Lt,
    Le,
    Ge,
}

impl CmpOp {
    fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "==" => Self::Eq,
            "!=" => Self::Ne,
            ">" => Self::Gt,
            "<" => Self::Lt,
            "<=" => Self::Le,
            ">=" => Self::Ge,
            _ => return None,
        })
    }

    const fn apply(self, a: u8, b: u8) -> bool {
        match self {
            Self::Eq => a == b,
            Self::Ne => a != b,
            Self::Gt => a > b,
            Self::Lt => a < b,
            Self::Le => a <= b,
            Self::Ge => a >= b,
        }
    }
}

/// The kind of an HD-pack `<condition>`.
#[derive(Debug, Clone)]
enum ConditionKind {
    /// `(watched[addr] & mask) <op> operand`. `addr` carries bit 31 as the
    /// PPU-vs-CPU marker ([`PPU_MEMORY_MARKER`]).
    MemoryCheckConstant {
        addr: u32,
        op: CmpOp,
        operand: u8,
        mask: u8,
    },
    /// `(watched[a] & mask) <op> (watched[b] & mask)`.
    MemoryCheck {
        addr_a: u32,
        addr_b: u32,
        op: CmpOp,
        mask: u8,
    },
    /// `frame % period >= offset` (Mesen `frameRange`).
    FrameRange { period: u32, offset: u32 },
    /// Per-tile: sprite horizontal flip (Mesen `hmirror`).
    HMirror,
    /// Per-tile: sprite vertical flip (Mesen `vmirror`).
    VMirror,
    /// Per-tile: sprite palette group equals `id` (Mesen `sppalette`).
    SpritePalette { id: u8 },
}

/// A named, parsed condition.
#[derive(Debug, Clone)]
struct Condition {
    #[allow(dead_code)]
    name: String,
    kind: ConditionKind,
}

/// One CHR-hash tile-replacement rule, optionally gated on conditions.
#[derive(Debug, Clone)]
struct TileRule {
    /// Index into [`HdPack::images`].
    image: usize,
    /// Top-left of the replacement rectangle inside the image (in pixels).
    x: u32,
    y: u32,
    /// Conditions that must ALL hold for the substitution to apply (AND).
    /// Indices into [`HdPack::conditions`]. Empty = unconditional.
    conditions: Vec<usize>,
}

/// A `<background>` region: a replacement image (full-screen or a rectangle)
/// alpha-blended into the output, optionally gated on conditions, ordered by
/// `priority` (lower draws first; higher sits on top).
#[derive(Debug, Clone)]
struct BackgroundRegion {
    /// Index into [`HdPack::images`].
    image: usize,
    /// Destination top-left in NES pixel space (before upscale).
    x: i32,
    y: i32,
    /// Draw priority (Mesen's `<background>` priority field; default 0). Higher
    /// = drawn later = on top.
    priority: i32,
    /// Conditions that must ALL hold for the region to render (AND). Empty =
    /// always.
    conditions: Vec<usize>,
}

/// A per-frame snapshot of the watched memory addresses referenced by the
/// pack's conditions.
///
/// Mirrors Mesen's `HdScreenInfo::WatchedAddressValues`. Keyed by the
/// (marker-tagged) address so a memoryCheck reads exactly the byte the produce
/// path captured.
#[derive(Debug, Clone, Default)]
pub struct WatchedMemory {
    values: HashMap<u32, u8>,
}

impl WatchedMemory {
    /// An empty snapshot (no watched addresses).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `value` for the (marker-tagged) `addr`.
    pub fn set(&mut self, addr: u32, value: u8) {
        self.values.insert(addr, value);
    }

    fn get(&self, addr: u32) -> u8 {
        self.values.get(&addr).copied().unwrap_or(0)
    }
}

/// v1.5.0 "Lens" Workstream A4 — one gating condition's name + whether it held
/// this frame, for the per-pixel inspector.
#[derive(Debug, Clone)]
pub struct ConditionTrace {
    /// The `<condition>` name referenced by the matched/candidate tile rule.
    pub name: String,
    /// Whether the condition evaluated true this frame.
    pub held: bool,
}

/// v1.5.0 "Lens" Workstream A4 — the per-pixel HD-pack composition trace
/// returned by [`HdCompositor::inspect_pixel`]. Display-only.
// The bool fields (is_sprite / flip_h / flip_v / matched) are independent
// per-pixel report flags, not state worth a bitfield.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct PixelInspection {
    /// NES pixel X coordinate (`0..256`).
    pub x: u32,
    /// NES pixel Y coordinate (`0..240`).
    pub y: u32,
    /// Base (stock) RGBA at this pixel.
    pub base: [u8; 4],
    /// Final (composited) RGBA at this pixel.
    pub final_rgba: [u8; 4],
    /// The dominant tile's CHR base address, or [`HD_TILE_NONE`].
    pub chr_addr: u16,
    /// Whether the dominant tile came from a sprite.
    pub is_sprite: bool,
    /// Sprite horizontal flip.
    pub flip_h: bool,
    /// Sprite vertical flip.
    pub flip_v: bool,
    /// The tile's palette group (0..=3).
    pub palette: u8,
    /// The Mesen CHR hash of the dominant tile (`None` if no tile here).
    pub chr_hash: Option<u32>,
    /// Whether a replacement rule's conditions all held (a substitution applied).
    pub matched: bool,
    /// The replacement image index of the matched / candidate rule (`None` if no
    /// rule keys this hash).
    pub replacement_image: Option<usize>,
    /// The gating conditions of the reported rule (the matched one, else the last
    /// candidate) with their per-frame outcomes.
    pub conditions: Vec<ConditionTrace>,
}

/// A loaded HD-pack: the upscale factor, the decoded replacement images, the
/// CHR-hash -> rule map, the parsed conditions, and the background regions.
#[derive(Debug)]
pub struct HdPack {
    /// Integer upscale factor (`<scale>`); clamped to `1..=8`.
    scale: u32,
    /// Decoded replacement images, indexed by [`TileRule::image`].
    images: Vec<ReplacementImage>,
    /// Tile rules keyed by the Mesen CHR hash. A given hash may have multiple
    /// rules (different condition sets); the first whose conditions all hold
    /// wins, then any unconditional fallback.
    tiles: HashMap<u32, Vec<TileRule>>,
    /// Pattern-table bank image references (`<patternTable>`), retained for
    /// completeness. Not consulted by the hash-keyed substitution.
    pattern_tables: Vec<String>,
    /// Parsed conditions, referenced by index from rules / backgrounds.
    conditions: Vec<Condition>,
    /// Background regions, sorted by ascending priority (draw order).
    backgrounds: Vec<BackgroundRegion>,
    /// The set of distinct watched memory addresses (marker-tagged) referenced
    /// by all memoryCheck conditions. The produce path snapshots exactly these.
    watched_addresses: Vec<u32>,
    /// v1.6.0 H — parsed `<bgm>` / `<sfx>` HD-audio declarations. Decoded +
    /// mixed by [`crate::hd_audio`] (frontend, output-only); empty for a
    /// video-only pack.
    audio_decls: Vec<HdAudioDecl>,
}

impl HdPack {
    /// The upscale factor (replacement tile edge = `8 * scale`).
    #[must_use]
    pub const fn scale(&self) -> u32 {
        self.scale
    }

    /// Number of tile rules parsed (across all hashes).
    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.tiles.values().map(Vec::len).sum()
    }

    /// Number of `<patternTable>` references parsed (diagnostic).
    #[must_use]
    pub const fn pattern_table_count(&self) -> usize {
        self.pattern_tables.len()
    }

    /// Number of `<condition>` declarations parsed (diagnostic).
    #[must_use]
    pub const fn condition_count(&self) -> usize {
        self.conditions.len()
    }

    /// Number of `<background>` regions parsed (diagnostic).
    #[must_use]
    pub const fn background_count(&self) -> usize {
        self.backgrounds.len()
    }

    /// v1.6.0 H — the parsed `<bgm>` / `<sfx>` HD-audio declarations. The loader
    /// decodes these (relative to the pack folder) into [`crate::hd_audio`]
    /// tracks. Empty for a video-only pack.
    #[must_use]
    pub fn audio_decls(&self) -> &[HdAudioDecl] {
        &self.audio_decls
    }

    /// The distinct watched memory addresses (marker-tagged with bit 31 for PPU
    /// space) the produce path must snapshot each frame for memoryCheck
    /// conditions. Empty when the pack uses no memory conditions.
    #[must_use]
    pub fn watched_addresses(&self) -> &[u32] {
        &self.watched_addresses
    }

    /// Load an HD-pack from a folder containing `hires.txt`, or from a `.zip`
    /// archive containing one. Returns `None` if no `hires.txt` is found or it
    /// parses to zero usable rules.
    #[must_use]
    pub fn load(path: &Path) -> Option<Self> {
        let is_zip = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
        if is_zip {
            Self::load_zip(path)
        } else {
            Self::load_folder(path)
        }
    }

    fn load_folder(dir: &Path) -> Option<Self> {
        let hires = std::fs::read_to_string(dir.join("hires.txt")).ok()?;
        let parsed = parse_hires(&hires);
        let mut images = Vec::with_capacity(parsed.image_names.len());
        for name in &parsed.image_names {
            // Reject any name that would escape the pack dir (path traversal).
            let decoded = sanitize_image_name(name)
                .and_then(|safe| std::fs::read(dir.join(safe)).ok())
                .and_then(|b| decode_png(&b));
            images.push(decoded);
        }
        Self::finish(parsed, images)
    }

    fn load_zip(path: &Path) -> Option<Self> {
        let file = std::fs::File::open(path).ok()?;
        let mut archive = zip::ZipArchive::new(file).ok()?;
        // Find the `hires.txt` entry (allow it to live in a subfolder).
        let hires_name = (0..archive.len()).find_map(|i| {
            let e = archive.by_index(i).ok()?;
            let name = e.name().to_string();
            Path::new(&name)
                .file_name()
                .is_some_and(|f| f.eq_ignore_ascii_case("hires.txt"))
                .then_some(name)
        })?;
        // The archive prefix (subfolder) the `hires.txt` lives under, so image
        // refs resolve relative to it.
        let prefix = Path::new(&hires_name)
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_default();
        let hires = {
            let mut e = archive.by_name(&hires_name).ok()?;
            let mut s = String::new();
            e.read_to_string(&mut s).ok()?;
            s
        };
        let parsed = parse_hires(&hires);
        let mut images = Vec::with_capacity(parsed.image_names.len());
        for name in &parsed.image_names {
            // Reject any name that would escape the pack prefix (path traversal):
            // only a plain final component is honoured, joined under the prefix.
            let decoded = sanitize_image_name(name).and_then(|safe| {
                let joined = prefix.join(safe);
                let entry_name = joined.to_string_lossy().replace('\\', "/");
                read_zip_entry(&mut archive, &entry_name)
                    .or_else(|| read_zip_entry(&mut archive, safe))
                    .and_then(|b| decode_png(&b))
            });
            images.push(decoded);
        }
        Self::finish(parsed, images)
    }

    fn finish(parsed: ParsedHires, images: Vec<Option<ReplacementImage>>) -> Option<Self> {
        // Drop rules whose image failed to decode; reindex the surviving images.
        let mut remap = vec![usize::MAX; images.len()];
        let mut kept: Vec<ReplacementImage> = Vec::new();
        for (i, img) in images.into_iter().enumerate() {
            if let Some(img) = img {
                remap[i] = kept.len();
                kept.push(img);
            }
        }
        let valid_img = |i: usize| -> Option<usize> {
            let &new_idx = remap.get(i)?;
            (new_idx != usize::MAX).then_some(new_idx)
        };

        // Resolve tile rules, dropping those whose image is gone. Rules carrying
        // a condition that failed to resolve are dropped (their gate is unknown).
        let mut tiles: HashMap<u32, Vec<TileRule>> = HashMap::new();
        let mut rule_count = 0usize;
        for (hash, rule) in parsed.tiles {
            let Some(new_idx) = valid_img(rule.image) else {
                continue;
            };
            tiles.entry(hash).or_default().push(TileRule {
                image: new_idx,
                x: rule.x,
                y: rule.y,
                conditions: rule.conditions,
            });
            rule_count += 1;
        }
        // Put unconditional rules last so conditional variants get first refusal.
        for rules in tiles.values_mut() {
            rules.sort_by_key(|r| r.conditions.is_empty());
        }

        // Resolve background regions (drop ones with a missing image).
        let mut backgrounds: Vec<BackgroundRegion> = Vec::new();
        for bg in parsed.backgrounds {
            let Some(new_idx) = valid_img(bg.image) else {
                continue;
            };
            backgrounds.push(BackgroundRegion {
                image: new_idx,
                x: bg.x,
                y: bg.y,
                priority: bg.priority,
                conditions: bg.conditions,
            });
        }
        backgrounds.sort_by_key(|b| b.priority);

        // A pack with no tile rules, no background regions, AND no HD-audio
        // declarations is useless. (v1.6.0 H: an audio-only pack is valid.)
        if rule_count == 0 && backgrounds.is_empty() && parsed.audio_decls.is_empty() {
            return None;
        }

        // Collect the distinct watched addresses from memoryCheck conditions.
        let mut watched: Vec<u32> = Vec::new();
        for c in &parsed.conditions {
            match &c.kind {
                ConditionKind::MemoryCheckConstant { addr, .. } => {
                    if !watched.contains(addr) {
                        watched.push(*addr);
                    }
                }
                ConditionKind::MemoryCheck { addr_a, addr_b, .. } => {
                    for a in [addr_a, addr_b] {
                        if !watched.contains(a) {
                            watched.push(*a);
                        }
                    }
                }
                _ => {}
            }
        }

        Some(Self {
            scale: parsed.scale.clamp(1, 8),
            images: kept,
            tiles,
            pattern_tables: parsed.pattern_tables,
            conditions: parsed.conditions,
            backgrounds,
            watched_addresses: watched,
            audio_decls: parsed.audio_decls,
        })
    }

    /// Evaluate a single condition by index against the current frame state.
    fn eval_condition(
        &self,
        idx: usize,
        watched: &WatchedMemory,
        frame: u32,
        rec: HdTileSource,
    ) -> bool {
        let Some(cond) = self.conditions.get(idx) else {
            // An unresolved condition reference fails closed.
            return false;
        };
        match &cond.kind {
            ConditionKind::MemoryCheckConstant {
                addr,
                op,
                operand,
                mask,
            } => {
                let a = watched.get(*addr) & *mask;
                op.apply(a, *operand & *mask)
            }
            ConditionKind::MemoryCheck {
                addr_a,
                addr_b,
                op,
                mask,
            } => {
                let a = watched.get(*addr_a) & *mask;
                let b = watched.get(*addr_b) & *mask;
                op.apply(a, b)
            }
            ConditionKind::FrameRange { period, offset } => {
                if *period == 0 {
                    return false;
                }
                (frame % *period) >= *offset
            }
            ConditionKind::HMirror => rec.is_sprite && rec.flip_h,
            ConditionKind::VMirror => rec.is_sprite && rec.flip_v,
            ConditionKind::SpritePalette { id } => rec.is_sprite && rec.palette == *id,
        }
    }

    /// Whether ALL of `conditions` hold (AND); empty = always true.
    fn all_hold(
        &self,
        conditions: &[usize],
        watched: &WatchedMemory,
        frame: u32,
        rec: HdTileSource,
    ) -> bool {
        conditions
            .iter()
            .all(|&i| self.eval_condition(i, watched, frame, rec))
    }
}

/// Sanitize a replacement-image filename parsed from `hires.txt` against path
/// traversal: a malicious pack must not be able to reference `../../etc/passwd`
/// or an absolute path and escape the pack directory. We accept ONLY a plain
/// final path component (no separators, no `..`, not absolute) and return it;
/// anything else is rejected (`None`).
fn sanitize_image_name(name: &str) -> Option<&str> {
    if name.is_empty() {
        return None;
    }
    // Reject absolute paths and any embedded path separators (forward or back).
    if name.contains('/') || name.contains('\\') {
        return None;
    }
    // Defence in depth: reject any `..` traversal component and Windows drive
    // / device prefixes (`:` appears in `C:` / `\\?\` style paths).
    if name == ".." || name == "." || name.contains(':') {
        return None;
    }
    Some(name)
}

/// Maximum bytes read from a single HD-pack zip entry (a replacement PNG). Caps
/// a zip bomb / corrupt archive before it can OOM us — replacement images are at
/// most a few MiB. Mirrors `app.rs::extract_rom_from_zip`'s cap: both the
/// declared size AND the actual read are bounded, since the declared size can lie.
const MAX_HD_ENTRY_BYTES: u64 = 64 * 1024 * 1024;

fn read_zip_entry(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<Vec<u8>> {
    let e = archive.by_name(name).ok()?;
    if e.size() > MAX_HD_ENTRY_BYTES {
        return None;
    }
    let cap = usize::try_from(e.size()).unwrap_or(0);
    let mut buf = Vec::with_capacity(cap);
    e.take(MAX_HD_ENTRY_BYTES).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Decode a PNG to RGBA8.
fn decode_png(bytes: &[u8]) -> Option<ReplacementImage> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let (w, h) = (info.width, info.height);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity((w * h * 4) as usize);
            for px in buf.chunks_exact(3) {
                out.extend_from_slice(&[px[0], px[1], px[2], 0xFF]);
            }
            out
        }
        png::ColorType::Grayscale => {
            let mut out = Vec::with_capacity((w * h * 4) as usize);
            for &g in &buf {
                out.extend_from_slice(&[g, g, g, 0xFF]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity((w * h * 4) as usize);
            for px in buf.chunks_exact(2) {
                out.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
            out
        }
        png::ColorType::Indexed => return None, // unexpanded palette; skip.
    };
    Some(ReplacementImage {
        width: w,
        height: h,
        rgba,
    })
}

/// A parsed tile rule before image-index reindex (image is an index into
/// `image_names`, conditions are unresolved indices into `conditions`).
struct ParsedTileRule {
    image: usize,
    x: u32,
    y: u32,
    conditions: Vec<usize>,
}

/// A parsed background region before image reindex.
struct ParsedBackground {
    image: usize,
    x: i32,
    y: i32,
    priority: i32,
    conditions: Vec<usize>,
}

/// Intermediate parse result before image decode + reindex.
struct ParsedHires {
    scale: u32,
    image_names: Vec<String>,
    pattern_tables: Vec<String>,
    /// chrHash -> rule (image index into `image_names`).
    tiles: Vec<(u32, ParsedTileRule)>,
    conditions: Vec<Condition>,
    backgrounds: Vec<ParsedBackground>,
    /// v1.6.0 H — parsed `<bgm>` / `<sfx>` HD-audio declarations.
    audio_decls: Vec<HdAudioDecl>,
}

/// Parse the supported subset of a Mesen `hires.txt`.
///
/// Mesen's format is line-oriented; each line is `<tag>` followed by
/// comma-separated fields. We recognize `<ver>` / `<scale>` / `<patternTable>`
/// headers, `<img>NAME`, `<condition>`, `<tile>`, and `<background>`. Lines we
/// do not recognize (overlays, audio, options) are ignored; malformed lines are
/// skipped.
#[allow(clippy::too_many_lines)]
fn parse_hires(src: &str) -> ParsedHires {
    let mut scale = 1u32;
    let mut image_names: Vec<String> = Vec::new();
    let mut name_to_idx: HashMap<String, usize> = HashMap::new();
    let mut pattern_tables: Vec<String> = Vec::new();
    let mut tiles: Vec<(u32, ParsedTileRule)> = Vec::new();
    let mut conditions: Vec<Condition> = Vec::new();
    let mut cond_name_to_idx: HashMap<String, usize> = HashMap::new();
    let mut backgrounds: Vec<ParsedBackground> = Vec::new();
    let mut audio_decls: Vec<HdAudioDecl> = Vec::new();

    // First pass over `<condition>`/`<img>` so forward-referenced names resolve.
    // We process declarations in two passes: conditions + images first, then
    // tiles + backgrounds (which reference them). Mesen files declare in order,
    // but a two-pass parse is robust to ordering.
    for raw in src.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let Some((tag, rest)) = split_tag(line) else {
            continue;
        };
        match tag {
            "scale" => {
                if let Ok(v) = rest.trim().parse::<u32>() {
                    scale = v;
                }
            }
            "patternTable" => {
                let name = rest.trim();
                if !name.is_empty() {
                    pattern_tables.push(name.to_string());
                }
            }
            "img" => {
                let name = rest.trim();
                if !name.is_empty() {
                    intern_name(&mut image_names, &mut name_to_idx, name);
                }
            }
            "condition" => {
                if let Some(cond) = parse_condition(rest)
                    && !cond_name_to_idx.contains_key(&cond.name)
                {
                    cond_name_to_idx.insert(cond.name.clone(), conditions.len());
                    conditions.push(cond);
                }
            }
            // v1.6.0 H — HD-audio track declarations. No condition/image refs,
            // so parse them in this first pass; the loader decodes the files.
            "bgm" => {
                if let Some(d) = parse_audio_decl(TrackKind::Bgm, rest) {
                    audio_decls.push(d);
                }
            }
            "sfx" => {
                if let Some(d) = parse_audio_decl(TrackKind::Sfx, rest) {
                    audio_decls.push(d);
                }
            }
            _ => {}
        }
    }

    // Second pass: tiles + backgrounds, resolving condition + image refs.
    for raw in src.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let Some((tag, rest)) = split_tag(line) else {
            continue;
        };
        match tag {
            "tile" => {
                if let Some((hash, img_name, x, y, cond_refs)) = parse_tile_fields(rest) {
                    let image = intern_name(&mut image_names, &mut name_to_idx, &img_name);
                    let conditions = resolve_condition_refs(&cond_refs, &cond_name_to_idx);
                    // Skip a tile rule that names a condition we never parsed.
                    if conditions.is_none() {
                        continue;
                    }
                    tiles.push((
                        hash,
                        ParsedTileRule {
                            image,
                            x,
                            y,
                            conditions: conditions.unwrap_or_default(),
                        },
                    ));
                }
            }
            "background" => {
                if let Some((img_name, x, y, priority, cond_refs)) = parse_background_fields(rest) {
                    let image = intern_name(&mut image_names, &mut name_to_idx, &img_name);
                    let conditions = resolve_condition_refs(&cond_refs, &cond_name_to_idx);
                    if conditions.is_none() {
                        continue;
                    }
                    backgrounds.push(ParsedBackground {
                        image,
                        x,
                        y,
                        priority,
                        conditions: conditions.unwrap_or_default(),
                    });
                }
            }
            _ => {}
        }
    }

    ParsedHires {
        scale,
        image_names,
        pattern_tables,
        tiles,
        conditions,
        backgrounds,
        audio_decls,
    }
}

/// Intern an image name, returning its declaration-order index.
fn intern_name(names: &mut Vec<String>, idx: &mut HashMap<String, usize>, name: &str) -> usize {
    if let Some(&i) = idx.get(name) {
        return i;
    }
    let i = names.len();
    names.push(name.to_string());
    idx.insert(name.to_string(), i);
    i
}

/// Resolve a list of condition-name references to indices. Returns `None` if ANY
/// name is unknown (so the caller can drop the rule), `Some(vec)` otherwise
/// (empty vec = no conditions).
fn resolve_condition_refs(names: &[String], map: &HashMap<String, usize>) -> Option<Vec<usize>> {
    let mut out = Vec::with_capacity(names.len());
    for n in names {
        out.push(*map.get(n)?);
    }
    Some(out)
}

/// Split a `<tag>rest` line into `(tag, rest)`. Returns `None` if not a tag line.
fn split_tag(line: &str) -> Option<(&str, &str)> {
    let line = line.strip_prefix('<')?;
    let close = line.find('>')?;
    Some((&line[..close], &line[close + 1..]))
}

/// Parse a hex (`0x`-optional) or decimal integer.
fn parse_int(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(hex) = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .or_else(|| s.strip_prefix('$'))
    {
        return u32::from_str_radix(hex, 16).ok();
    }
    s.parse::<u32>().ok()
}

/// Parse a memory-address operand: Mesen prefixes a PPU-space address with `@`
/// (and internally sets bit 31). A leading `@` here tags [`PPU_MEMORY_MARKER`].
fn parse_mem_addr(s: &str) -> Option<u32> {
    let s = s.trim();
    let (ppu, body) = s.strip_prefix('@').map_or((false, s), |rest| (true, rest));
    let addr = parse_int(body)? & 0xFFFF;
    Some(if ppu { addr | PPU_MEMORY_MARKER } else { addr })
}

/// Parse a `<condition>` line: `NAME,TYPE,args...`.
///
/// Supported `TYPE`s: `memoryCheck` / `ppuMemoryCheck`, `memoryCheckConstant` /
/// `ppuMemoryCheckConstant`, `frameRange`, `hmirror`, `vmirror`, `sppalette`.
/// Any other type is ignored (returns `None`).
fn parse_condition(rest: &str) -> Option<Condition> {
    let fields: Vec<&str> = rest.split(',').map(str::trim).collect();
    if fields.len() < 2 {
        return None;
    }
    let name = fields[0];
    if name.is_empty() {
        return None;
    }
    let ty = fields[1];
    let kind = match ty {
        // memoryCheckConstant: NAME,type,addr,op,operand[,mask]
        "memoryCheckConstant" | "ppuMemoryCheckConstant" => {
            if fields.len() < 5 {
                return None;
            }
            let mut addr = parse_mem_addr(fields[2])?;
            if ty.starts_with("ppu") {
                addr |= PPU_MEMORY_MARKER;
            }
            let op = CmpOp::parse(fields[3])?;
            let operand = u8::try_from(parse_int(fields[4])? & 0xFF).ok()?;
            let mask = fields
                .get(5)
                .and_then(|m| parse_int(m))
                .and_then(|m| u8::try_from(m & 0xFF).ok())
                .unwrap_or(0xFF);
            ConditionKind::MemoryCheckConstant {
                addr,
                op,
                operand,
                mask,
            }
        }
        // memoryCheck: NAME,type,addrA,op,addrB[,mask]
        "memoryCheck" | "ppuMemoryCheck" => {
            if fields.len() < 5 {
                return None;
            }
            let mut addr_a = parse_mem_addr(fields[2])?;
            let op = CmpOp::parse(fields[3])?;
            let mut addr_b = parse_mem_addr(fields[4])?;
            if ty.starts_with("ppu") {
                addr_a |= PPU_MEMORY_MARKER;
                addr_b |= PPU_MEMORY_MARKER;
            }
            let mask = fields
                .get(5)
                .and_then(|m| parse_int(m))
                .and_then(|m| u8::try_from(m & 0xFF).ok())
                .unwrap_or(0xFF);
            ConditionKind::MemoryCheck {
                addr_a,
                addr_b,
                op,
                mask,
            }
        }
        // frameRange: NAME,frameRange,period,offset
        "frameRange" => {
            if fields.len() < 4 {
                return None;
            }
            let period = parse_int(fields[2])?;
            let offset = parse_int(fields[3])?;
            ConditionKind::FrameRange { period, offset }
        }
        "hmirror" => ConditionKind::HMirror,
        "vmirror" => ConditionKind::VMirror,
        // sppalette: NAME,sppalette,id
        "sppalette" => {
            let id = fields
                .get(2)
                .and_then(|s| parse_int(s))
                .and_then(|v| u8::try_from(v & 0x03).ok())
                .unwrap_or(0);
            ConditionKind::SpritePalette { id }
        }
        _ => return None, // unsupported condition type: ignored (inert).
    };
    Some(Condition {
        name: name.to_string(),
        kind,
    })
}

/// Split a condition-reference field on `&` or `+` (Mesen joins multiple
/// conditions for AND), trimming and dropping empties.
fn split_condition_refs(field: &str) -> Vec<String> {
    field
        .split(['&', '+'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Parse the comma-separated fields of a `<tile>` rule into
/// `(chrHash, imageName, x, y, conditionRefs)`.
///
/// Accepts both `hash,image,x,y[,conditions]` and `image,x,y,hash[,conditions]`
/// (disambiguated by whether field 0 parses as a hex hash), where the optional
/// 5th field is a condition-name reference (`&`/`+`-joined for AND). An empty
/// 5th field means unconditional.
fn parse_tile_fields(rest: &str) -> Option<(u32, String, u32, u32, Vec<String>)> {
    let fields: Vec<&str> = rest.split(',').map(str::trim).collect();
    if fields.len() < 4 {
        return None;
    }
    let cond_refs = fields
        .get(4)
        .map_or_else(Vec::new, |f| split_condition_refs(f));

    let parse_hash = |s: &str| -> Option<u32> {
        let s = s.trim_start_matches("0x").trim_start_matches("0X");
        u32::from_str_radix(s, 16).ok()
    };

    // Form A: hash,image,x,y[,cond]
    if let Some(hash) = parse_hash(fields[0])
        && !fields[1].is_empty()
        && let (Ok(x), Ok(y)) = (fields[2].parse::<u32>(), fields[3].parse::<u32>())
    {
        return Some((hash, fields[1].to_string(), x, y, cond_refs));
    }
    // Form B: image,x,y,hash[,cond]
    if let Some(hash) = parse_hash(fields[3])
        && !fields[0].is_empty()
        && let (Ok(x), Ok(y)) = (fields[1].parse::<u32>(), fields[2].parse::<u32>())
    {
        return Some((hash, fields[0].to_string(), x, y, cond_refs));
    }
    None
}

/// Parse a `<background>` line into `(imageName, x, y, priority, conditionRefs)`.
///
/// Mesen's documented form is `<background>image[,brightness][,condition]`, with
/// the image full-screen at the origin. We accept the additive `RustyNES` form
/// `<background>image[,x,y][,priority][,condition]` (x/y/priority optional and
/// positional) so packs can also place a region; bare `<background>image` =
/// full-screen at (0,0), priority 0, unconditional. A trailing non-numeric field
/// is taken as the condition reference.
fn parse_background_fields(rest: &str) -> Option<(String, i32, i32, i32, Vec<String>)> {
    let fields: Vec<&str> = rest.split(',').map(str::trim).collect();
    if fields.is_empty() || fields[0].is_empty() {
        return None;
    }
    let image = fields[0].to_string();
    let mut x = 0i32;
    let mut y = 0i32;
    let mut priority = 0i32;
    let mut cond_refs: Vec<String> = Vec::new();

    // Walk the trailing fields: numeric ones fill x, y, priority in order; the
    // first non-numeric, non-empty one is the condition reference.
    let mut numeric_seen = 0;
    for f in &fields[1..] {
        if f.is_empty() {
            continue;
        }
        if let Ok(v) = f.parse::<i32>() {
            match numeric_seen {
                0 => x = v,
                1 => y = v,
                _ => priority = v,
            }
            numeric_seen += 1;
        } else {
            cond_refs = split_condition_refs(f);
            break;
        }
    }
    Some((image, x, y, priority, cond_refs))
}

// =============================================================================
// CRC32 (Mesen tile-hash compatible — standard reflected CRC-32, poly 0xEDB88320).
// =============================================================================

/// Compute the standard reflected CRC-32 of `bytes` (poly `0xEDB88320`), the
/// hash Mesen uses to key HD-pack tile replacements over a tile's 16 CHR bytes.
#[must_use]
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

// =============================================================================
// Per-frame HD compositor.
// =============================================================================

/// The HD compositor.
///
/// Turns the NES framebuffer + the PPU tile-source telemetry into an upscaled
/// RGBA buffer with replacement tiles blitted over the nearest-neighbour upscale
/// of the base image, gating tile substitution + background regions on the
/// pack's conditions against a per-frame snapshot of the watched memory
/// addresses + the frame counter.
pub struct HdCompositor {
    pack: HdPack,
    /// Reusable output buffer (`scale*256 x scale*240` RGBA8).
    out: Vec<u8>,
    out_w: u32,
    out_h: u32,
    /// CHR-hash cache keyed on `(chr_addr, flip_h, flip_v)` -> hash, refreshed
    /// per frame. Avoids re-reading + re-hashing 16 CHR bytes for repeated tiles.
    hash_cache: HashMap<(u16, bool, bool), u32>,
    /// Monotonic frame counter for `frameRange` conditions. Advances once per
    /// [`Self::composite`]; presentation-only, never serialized.
    frame: u32,
}

impl HdCompositor {
    /// Build a compositor for a loaded pack.
    #[must_use]
    pub fn new(pack: HdPack) -> Self {
        let scale = pack.scale();
        let out_w = NES_W * scale;
        let out_h = NES_H * scale;
        Self {
            pack,
            out: vec![0u8; (out_w * out_h * 4) as usize],
            out_w,
            out_h,
            hash_cache: HashMap::new(),
            frame: 0,
        }
    }

    /// Output dimensions (`scale*256`, `scale*240`).
    #[must_use]
    pub const fn dimensions(&self) -> (u32, u32) {
        (self.out_w, self.out_h)
    }

    /// The most recently composited HD RGBA8 frame.
    #[must_use]
    // `Vec::as_slice` is not const-stable on the pinned 1.86 toolchain.
    #[allow(clippy::missing_const_for_fn)]
    pub fn frame(&self) -> &[u8] {
        &self.out
    }

    /// The loaded pack (diagnostic access).
    #[must_use]
    pub const fn pack(&self) -> &HdPack {
        &self.pack
    }

    /// The watched memory addresses (marker-tagged) the produce path must
    /// snapshot each frame for this pack's memory conditions.
    #[must_use]
    pub fn watched_addresses(&self) -> &[u32] {
        self.pack.watched_addresses()
    }

    /// Composite one frame.
    ///
    /// `framebuffer` is the NES RGBA8 image (256x240x4). `tile_source` is the
    /// PPU's per-pixel [`HdTileSource`] telemetry (256x240). `watched` is the
    /// per-frame snapshot of the watched memory addresses (captured under the emu
    /// lock at produce time). `chr_peek(addr)` returns the CHR byte at a PPU
    /// pattern-space address — used to hash a tile's 16 CHR bytes for the
    /// replacement lookup. Returns the upscaled RGBA8 buffer.
    pub fn composite(
        &mut self,
        framebuffer: &[u8],
        tile_source: &[HdTileSource],
        watched: &WatchedMemory,
        mut chr_peek: impl FnMut(u16) -> u8,
    ) -> &[u8] {
        debug_assert_eq!(framebuffer.len(), (NES_W * NES_H * 4) as usize);
        debug_assert_eq!(tile_source.len(), (NES_W * NES_H) as usize);
        let scale = self.pack.scale as usize;
        let out_w = self.out_w as usize;
        let frame = self.frame;

        // 1) Nearest-neighbour upscale of the base framebuffer.
        for y in 0..NES_H as usize {
            for x in 0..NES_W as usize {
                let src = (y * NES_W as usize + x) * 4;
                let px = &framebuffer[src..src + 4];
                for sy in 0..scale {
                    let row = (y * scale + sy) * out_w;
                    for sx in 0..scale {
                        let dst = (row + x * scale + sx) * 4;
                        self.out[dst..dst + 4].copy_from_slice(px);
                    }
                }
            }
        }

        // 2) Low-priority (priority < 0) background regions render UNDER the
        //    tile pass; non-negative priority renders OVER (after) it. This lets
        //    a pack place a backdrop behind replacement tiles or an overlay above.
        Self::draw_backgrounds(
            &self.pack,
            &mut self.out,
            self.out_h as usize,
            scale,
            out_w,
            watched,
            frame,
            true, // under = priority < 0
        );

        // 3) Per 8x8 cell, resolve the dominant tile identity and, if a gated
        //    replacement exists for its CHR hash, blit the hi-res image over the
        //    upscaled base. The cell's identity is taken from its top-left pixel
        //    (scrolling shifts whole tiles by < 8px; this keys on the aligned
        //    grid, like Mesen's BG path).
        self.hash_cache.clear();
        for cell_y in 0..ROWS {
            for cell_x in 0..COLS {
                let px = cell_y * TILE * NES_W as usize + cell_x * TILE;
                let rec = tile_source[px];
                if rec.chr_addr == HD_TILE_NONE {
                    continue;
                }
                let key = (rec.chr_addr, rec.flip_h, rec.flip_v);
                let hash = if let Some(&h) = self.hash_cache.get(&key) {
                    h
                } else {
                    let h = hash_tile(rec, &mut chr_peek);
                    self.hash_cache.insert(key, h);
                    h
                };
                let Some(rules) = self.pack.tiles.get(&hash) else {
                    continue;
                };
                // First rule whose conditions all hold wins (unconditional rules
                // are sorted last, so a conditional variant gets first refusal).
                let Some(rule) = rules
                    .iter()
                    .find(|r| self.pack.all_hold(&r.conditions, watched, frame, rec))
                else {
                    continue;
                };
                let Some(img) = self.pack.images.get(rule.image) else {
                    continue;
                };
                blit_replacement(
                    &mut self.out,
                    out_w,
                    self.out_h as usize,
                    cell_x,
                    cell_y,
                    scale,
                    img,
                    rule,
                );
            }
        }

        // 4) Non-negative-priority background regions render OVER the tile pass.
        Self::draw_backgrounds(
            &self.pack,
            &mut self.out,
            self.out_h as usize,
            scale,
            out_w,
            watched,
            frame,
            false, // over = priority >= 0
        );

        self.frame = self.frame.wrapping_add(1);
        &self.out
    }

    /// v1.5.0 "Lens" Workstream A4 — per-pixel HD-pack composition trace.
    ///
    /// Resolves what the compositor did at NES pixel `(px, py)` (in the unscaled
    /// 256x240 space): the dominant tile's CHR identity + Mesen hash, the
    /// replacement rule that matched (if any) with the gating condition names +
    /// whether each held, the base (stock) RGBA, and the final (composited) RGBA.
    /// Mirrors the per-cell logic in [`Self::composite`] but for one cell only.
    ///
    /// Display-only: reads the same already-deterministic snapshots `composite`
    /// consumed; mutates nothing. Returns `None` if the coordinate is off-screen.
    #[must_use]
    pub fn inspect_pixel(
        &self,
        px: u32,
        py: u32,
        framebuffer: &[u8],
        tile_source: &[HdTileSource],
        watched: &WatchedMemory,
        mut chr_peek: impl FnMut(u16) -> u8,
    ) -> Option<PixelInspection> {
        if px >= NES_W || py >= NES_H {
            return None;
        }
        // Keep the u32 coords for the report; usize copies for indexing.
        let (ux, uy) = (px as usize, py as usize);
        // The composite() that just ran advanced `self.frame`; the values it used
        // were for the prior count.
        let frame = self.frame.wrapping_sub(1);

        // Base (stock NES) pixel.
        let bsrc = (uy * NES_W as usize + ux) * 4;
        let base = [
            framebuffer[bsrc],
            framebuffer[bsrc + 1],
            framebuffer[bsrc + 2],
            framebuffer[bsrc + 3],
        ];
        // Final (composited) pixel: nearest-neighbour, so the cell's top-left
        // scaled pixel is representative.
        let scale = self.pack.scale as usize;
        let fx = ux * scale;
        let fy = uy * scale;
        let fsrc = (fy * self.out_w as usize + fx) * 4;
        let final_rgba = [
            self.out[fsrc],
            self.out[fsrc + 1],
            self.out[fsrc + 2],
            self.out[fsrc + 3],
        ];

        // Dominant tile identity for the containing 8x8 cell (composite keys on
        // the cell's top-left pixel).
        let cell_x = ux / TILE;
        let cell_y = uy / TILE;
        let cell_px = cell_y * TILE * NES_W as usize + cell_x * TILE;
        let rec = tile_source[cell_px];

        let mut out = PixelInspection {
            x: px,
            y: py,
            base,
            final_rgba,
            chr_addr: rec.chr_addr,
            is_sprite: rec.is_sprite,
            flip_h: rec.flip_h,
            flip_v: rec.flip_v,
            palette: rec.palette,
            chr_hash: None,
            matched: false,
            replacement_image: None,
            conditions: Vec::new(),
        };
        if rec.chr_addr == HD_TILE_NONE {
            return Some(out);
        }
        let hash = hash_tile(rec, &mut chr_peek);
        out.chr_hash = Some(hash);
        let Some(rules) = self.pack.tiles.get(&hash) else {
            return Some(out);
        };
        // Walk the rules in priority order (conditional first); record the gating
        // condition outcomes for whichever rule we report on, and mark `matched`
        // once one holds (mirroring composite()'s `find`).
        for rule in rules {
            let conds: Vec<ConditionTrace> = rule
                .conditions
                .iter()
                .map(|&i| ConditionTrace {
                    name: self
                        .pack
                        .conditions
                        .get(i)
                        .map_or_else(|| "?".to_string(), |c| c.name.clone()),
                    held: self.pack.eval_condition(i, watched, frame, rec),
                })
                .collect();
            let holds = conds.iter().all(|c| c.held);
            // Report the first rule that holds; otherwise keep the last rule's
            // trace so the user can see why nothing matched.
            out.conditions = conds;
            out.replacement_image = Some(rule.image);
            if holds {
                out.matched = true;
                break;
            }
        }
        Some(out)
    }

    /// Alpha-blit the background regions of one priority half (under = priority
    /// `< 0`, over = priority `>= 0`) whose conditions hold. Taken as an
    /// associated fn so the `&self.pack` read and the `&mut self.out` write are
    /// disjoint, non-overlapping borrows.
    ///
    /// A region's per-tile condition state is taken from a default (origin) tile:
    /// memory / frameRange conditions don't depend on tile state, and per-tile
    /// conditions on a full-screen background are an unusual pack choice.
    #[allow(clippy::too_many_arguments)]
    fn draw_backgrounds(
        pack: &HdPack,
        out: &mut [u8],
        out_h: usize,
        scale: usize,
        out_w: usize,
        watched: &WatchedMemory,
        frame: u32,
        under: bool,
    ) {
        for bg in &pack.backgrounds {
            if under != (bg.priority < 0) {
                continue;
            }
            if !pack.all_hold(&bg.conditions, watched, frame, HdTileSource::default()) {
                continue;
            }
            let Some(img) = pack.images.get(bg.image) else {
                continue;
            };
            blit_background(out, out_h, out_w, scale, img, bg);
        }
    }
}

/// Alpha-blit one background region into `out`.
// scale (≤ 8) + the source pixel indices are small + bounded, so the i64 casts
// used to do signed destination-bounds math can never wrap.
#[allow(clippy::cast_possible_wrap)]
fn blit_background(
    out: &mut [u8],
    out_h: usize,
    out_w: usize,
    scale: usize,
    img: &ReplacementImage,
    bg: &BackgroundRegion,
) {
    let img_w = img.width as usize;
    let img_h = img.height as usize;
    let scale_i = scale as i64;
    // Destination origin in upscaled space (i64 to avoid overflow / wrap).
    let ox = i64::from(bg.x) * scale_i;
    let oy = i64::from(bg.y) * scale_i;
    for sy in 0..img_h {
        let dy = oy + sy as i64;
        if dy < 0 {
            continue;
        }
        let Ok(dy) = usize::try_from(dy) else {
            continue;
        };
        if dy >= out_h {
            break;
        }
        for sx in 0..img_w {
            let dx = ox + sx as i64;
            if dx < 0 {
                continue;
            }
            let Ok(dx) = usize::try_from(dx) else {
                continue;
            };
            if dx >= out_w {
                break;
            }
            let s = (sy * img_w + sx) * 4;
            let a = img.rgba[s + 3];
            if a == 0 {
                continue; // fully transparent.
            }
            let d = (dy * out_w + dx) * 4;
            if a == 0xFF {
                out[d..d + 4].copy_from_slice(&img.rgba[s..s + 4]);
            } else {
                // Source-over alpha blend (premultiply-free, u16 math).
                let inv = 255 - u16::from(a);
                for c in 0..3 {
                    let src = u16::from(img.rgba[s + c]) * u16::from(a);
                    let dstc = u16::from(out[d + c]) * inv;
                    // Round to nearest (+127) instead of truncating; overflow-safe
                    // since the max numerator is 255*255 + 127 = 65152 < u16::MAX.
                    out[d + c] = u8::try_from((src + dstc + 127) / 255).unwrap_or(0xFF);
                }
                // Leave dst alpha opaque (the base upscale is opaque).
                out[d + 3] = 0xFF;
            }
        }
    }
}

/// Hash a tile's 16 CHR bytes (Mesen-compatible CRC32) from the raw, *unflipped*
/// pattern bytes. Mesen keys tile replacement on the tile's CHR content, and H/V
/// flips are applied later by the renderer, so the hash deliberately reads the
/// pattern bytes straight from CHR and does NOT consult `rec.flip_h` / `flip_v`
/// — a flipped sprite hashes to the same key as its unflipped tile dump.
fn hash_tile(rec: HdTileSource, chr_peek: &mut impl FnMut(u16) -> u8) -> u32 {
    let base = rec.chr_addr & 0x1FF0;
    let mut bytes = [0u8; 16];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = chr_peek(base + u16::try_from(i).unwrap_or(0));
    }
    crc32(&bytes)
}

/// Blit a replacement image rectangle over the upscaled base for one 8x8 cell.
/// The replacement rectangle is `8*scale` square at `(rule.x, rule.y)` in the
/// image. Out-of-bounds source pixels are skipped (leaving the base upscale).
/// Fully-transparent source pixels (alpha 0) are skipped so packs can mark
/// see-through regions.
#[allow(clippy::too_many_arguments)]
fn blit_replacement(
    out: &mut [u8],
    out_w: usize,
    out_h: usize,
    cell_x: usize,
    cell_y: usize,
    scale: usize,
    img: &ReplacementImage,
    rule: &TileRule,
) {
    let edge = TILE * scale; // replacement tile edge in pixels.
    let img_w = img.width as usize;
    let img_h = img.height as usize;
    for ry in 0..edge {
        let sy = rule.y as usize + ry;
        if sy >= img_h {
            break;
        }
        let dy = cell_y * edge + ry;
        if dy >= out_h {
            break;
        }
        for rx in 0..edge {
            let sx = rule.x as usize + rx;
            if sx >= img_w {
                break;
            }
            let dx = cell_x * edge + rx;
            if dx >= out_w {
                break;
            }
            let s = (sy * img_w + sx) * 4;
            if img.rgba[s + 3] == 0 {
                continue; // transparent.
            }
            let d = (dy * out_w + dx) * 4;
            out[d..d + 4].copy_from_slice(&img.rgba[s..s + 4]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_known_vectors() {
        // Standard CRC-32 of the empty string is 0; of "123456789" is 0xCBF43926.
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn parses_scale_and_unconditional_tile() {
        let src = "<ver>100\n\
                   <scale>2\n\
                   <patternTable>bank0.png\n\
                   <img>tiles.png\n\
                   <tile>0a1b2c3d,tiles.png,16,0\n";
        let parsed = parse_hires(src);
        assert_eq!(parsed.scale, 2);
        assert_eq!(parsed.pattern_tables, vec!["bank0.png".to_string()]);
        assert_eq!(parsed.tiles.len(), 1);
        let (hash, rule) = &parsed.tiles[0];
        assert_eq!(*hash, 0x0a1b_2c3d);
        assert_eq!(rule.x, 16);
        assert_eq!(rule.y, 0);
        assert!(rule.conditions.is_empty());
        // image name "tiles.png" was interned (also referenced by <img>).
        assert_eq!(parsed.image_names, vec!["tiles.png".to_string()]);
    }

    #[test]
    fn parses_image_first_tile_form() {
        // image,x,y,hash
        let src = "<scale>1\n<tile>sheet.png,8,24,deadbeef\n";
        let parsed = parse_hires(src);
        assert_eq!(parsed.tiles.len(), 1);
        let (hash, rule) = &parsed.tiles[0];
        assert_eq!(*hash, 0xdead_beef);
        assert_eq!(rule.x, 8);
        assert_eq!(rule.y, 24);
    }

    #[test]
    fn ignores_unknown_tags() {
        let src = "<overlay>x,y,z\n<bgmCondition>a\n";
        let parsed = parse_hires(src);
        assert!(parsed.tiles.is_empty());
        assert_eq!(parsed.scale, 1);
    }

    #[test]
    fn parses_bgm_and_sfx_audio_decls() {
        let src = "<scale>1\n\
                   <bgm>0,1,title.ogg\n\
                   <sfx>0,2,jump.ogg\n\
                   <bgmCondition>ignored\n";
        let parsed = parse_hires(src);
        assert_eq!(parsed.audio_decls.len(), 2);
        assert_eq!(parsed.audio_decls[0].kind, TrackKind::Bgm);
        assert_eq!(parsed.audio_decls[0].track, 1);
        assert_eq!(parsed.audio_decls[0].file, "title.ogg");
        assert_eq!(parsed.audio_decls[1].kind, TrackKind::Sfx);
        assert_eq!(parsed.audio_decls[1].track, 2);
        // The unrecognized `<bgmCondition>` is ignored, not an audio decl.
    }

    #[test]
    fn split_tag_basic() {
        assert_eq!(split_tag("<scale>2"), Some(("scale", "2")));
        assert_eq!(split_tag("no tag"), None);
    }

    #[test]
    fn sanitize_image_name_rejects_traversal() {
        // Plain final components are accepted unchanged.
        assert_eq!(sanitize_image_name("tiles.png"), Some("tiles.png"));
        // Path traversal / absolute / separator / drive forms are all rejected.
        assert_eq!(sanitize_image_name("../../etc/passwd"), None);
        assert_eq!(sanitize_image_name("/etc/passwd"), None);
        assert_eq!(sanitize_image_name("sub/dir/tiles.png"), None);
        assert_eq!(sanitize_image_name("..\\..\\windows\\system32"), None);
        assert_eq!(sanitize_image_name(".."), None);
        assert_eq!(sanitize_image_name("C:\\evil.png"), None);
        assert_eq!(sanitize_image_name(""), None);
    }

    // ---- v1.3.0 E1: condition + background parse/eval ----

    #[test]
    fn parses_memory_check_constant_condition() {
        let src = "<condition>lives,memoryCheckConstant,0x0075,>=,3\n";
        let parsed = parse_hires(src);
        assert_eq!(parsed.conditions.len(), 1);
        assert_eq!(parsed.conditions[0].name, "lives");
        match parsed.conditions[0].kind {
            ConditionKind::MemoryCheckConstant {
                addr,
                op,
                operand,
                mask,
            } => {
                assert_eq!(addr, 0x0075);
                assert_eq!(op, CmpOp::Ge);
                assert_eq!(operand, 3);
                assert_eq!(mask, 0xFF);
            }
            _ => panic!("wrong kind"),
        }
    }

    #[test]
    fn parses_ppu_marker_and_mask() {
        // ppu* type OR a leading @ both set the PPU marker bit; explicit mask.
        let src = "<condition>p,ppuMemoryCheckConstant,0x3F00,==,0x0F,0x3F\n";
        let parsed = parse_hires(src);
        match parsed.conditions[0].kind {
            ConditionKind::MemoryCheckConstant {
                addr,
                operand,
                mask,
                ..
            } => {
                assert_eq!(addr & PPU_MEMORY_MARKER, PPU_MEMORY_MARKER);
                assert_eq!(addr & 0xFFFF, 0x3F00);
                assert_eq!(operand, 0x0F);
                assert_eq!(mask, 0x3F);
            }
            _ => panic!("wrong kind"),
        }
    }

    #[test]
    fn parses_memory_check_two_operand() {
        let src = "<condition>cmp,memoryCheck,0x10,!=,0x11,0x0F\n";
        let parsed = parse_hires(src);
        match parsed.conditions[0].kind {
            ConditionKind::MemoryCheck {
                addr_a,
                addr_b,
                op,
                mask,
            } => {
                assert_eq!(addr_a, 0x10);
                assert_eq!(addr_b, 0x11);
                assert_eq!(op, CmpOp::Ne);
                assert_eq!(mask, 0x0F);
            }
            _ => panic!("wrong kind"),
        }
    }

    #[test]
    fn parses_frame_range_condition() {
        let src = "<condition>blink,frameRange,60,30\n";
        let parsed = parse_hires(src);
        match parsed.conditions[0].kind {
            ConditionKind::FrameRange { period, offset } => {
                assert_eq!(period, 60);
                assert_eq!(offset, 30);
            }
            _ => panic!("wrong kind"),
        }
    }

    #[test]
    fn conditional_tile_references_condition() {
        let src = "<condition>lives,memoryCheckConstant,0x0075,>=,3\n\
                   <tile>0a1b2c3d,tiles.png,16,0,lives\n";
        let parsed = parse_hires(src);
        assert_eq!(parsed.tiles.len(), 1);
        assert_eq!(parsed.tiles[0].1.conditions, vec![0]);
    }

    #[test]
    fn tile_with_unknown_condition_is_dropped() {
        // The condition name was never declared -> the rule's gate is unknown.
        let src = "<tile>0a1b2c3d,tiles.png,16,0,missing\n";
        let parsed = parse_hires(src);
        assert!(parsed.tiles.is_empty());
    }

    #[test]
    fn tile_with_anded_conditions() {
        let src = "<condition>a,frameRange,2,1\n\
                   <condition>b,frameRange,4,2\n\
                   <tile>deadbeef,t.png,0,0,a&b\n";
        let parsed = parse_hires(src);
        assert_eq!(parsed.tiles[0].1.conditions, vec![0, 1]);
    }

    #[test]
    fn parses_background_full_screen_and_region() {
        let src = "<background>bg.png\n\
                   <background>panel.png,16,32,1,nightcond\n\
                   <condition>nightcond,frameRange,2,1\n";
        let parsed = parse_hires(src);
        assert_eq!(parsed.backgrounds.len(), 2);
        assert_eq!(parsed.backgrounds[0].x, 0);
        assert_eq!(parsed.backgrounds[0].y, 0);
        assert!(parsed.backgrounds[0].conditions.is_empty());
        assert_eq!(parsed.backgrounds[1].x, 16);
        assert_eq!(parsed.backgrounds[1].y, 32);
        assert_eq!(parsed.backgrounds[1].priority, 1);
        assert_eq!(parsed.backgrounds[1].conditions, vec![0]);
    }

    // ---- condition evaluation ----

    /// Build a tiny one-condition pack for evaluation tests (no images needed).
    fn pack_with_condition(kind: ConditionKind) -> HdPack {
        HdPack {
            scale: 1,
            images: Vec::new(),
            tiles: HashMap::new(),
            pattern_tables: Vec::new(),
            conditions: vec![Condition {
                name: "c".into(),
                kind,
            }],
            backgrounds: Vec::new(),
            watched_addresses: Vec::new(),
            audio_decls: Vec::new(),
        }
    }

    #[test]
    fn eval_memory_check_constant_all_operators() {
        let mut wm = WatchedMemory::new();
        wm.set(0x10, 0x05);
        let cases = [
            (CmpOp::Eq, 5, true),
            (CmpOp::Eq, 6, false),
            (CmpOp::Ne, 6, true),
            (CmpOp::Gt, 4, true),
            (CmpOp::Gt, 5, false),
            (CmpOp::Lt, 6, true),
            (CmpOp::Le, 5, true),
            (CmpOp::Ge, 5, true),
            (CmpOp::Ge, 6, false),
        ];
        for (op, operand, want) in cases {
            let pack = pack_with_condition(ConditionKind::MemoryCheckConstant {
                addr: 0x10,
                op,
                operand,
                mask: 0xFF,
            });
            let got = pack.eval_condition(0, &wm, 0, HdTileSource::default());
            assert_eq!(got, want, "op {op:?} operand {operand}");
        }
    }

    #[test]
    fn eval_memory_check_constant_mask() {
        let mut wm = WatchedMemory::new();
        wm.set(0x20, 0b1010_0101);
        // Masking to the low nibble: 0x05 == 0x05.
        let pack = pack_with_condition(ConditionKind::MemoryCheckConstant {
            addr: 0x20,
            op: CmpOp::Eq,
            operand: 0x05,
            mask: 0x0F,
        });
        assert!(pack.eval_condition(0, &wm, 0, HdTileSource::default()));
        // Unmasked it would be 0xA5 != 0x05.
        let pack2 = pack_with_condition(ConditionKind::MemoryCheckConstant {
            addr: 0x20,
            op: CmpOp::Eq,
            operand: 0x05,
            mask: 0xFF,
        });
        assert!(!pack2.eval_condition(0, &wm, 0, HdTileSource::default()));
    }

    #[test]
    fn eval_memory_check_two_operand() {
        let mut wm = WatchedMemory::new();
        wm.set(0x30, 0x07);
        wm.set(0x31, 0x07);
        let pack = pack_with_condition(ConditionKind::MemoryCheck {
            addr_a: 0x30,
            addr_b: 0x31,
            op: CmpOp::Eq,
            mask: 0xFF,
        });
        assert!(pack.eval_condition(0, &wm, 0, HdTileSource::default()));
        wm.set(0x31, 0x08);
        assert!(!pack.eval_condition(0, &wm, 0, HdTileSource::default()));
    }

    #[test]
    fn eval_ppu_marker_address_distinct_from_cpu() {
        // The same low address in CPU vs PPU space is a distinct watched key.
        let mut wm = WatchedMemory::new();
        wm.set(0x0075, 0x01); // CPU $0075
        wm.set(0x0075 | PPU_MEMORY_MARKER, 0x09); // PPU $0075
        let cpu = pack_with_condition(ConditionKind::MemoryCheckConstant {
            addr: 0x0075,
            op: CmpOp::Eq,
            operand: 0x01,
            mask: 0xFF,
        });
        let ppu = pack_with_condition(ConditionKind::MemoryCheckConstant {
            addr: 0x0075 | PPU_MEMORY_MARKER,
            op: CmpOp::Eq,
            operand: 0x09,
            mask: 0xFF,
        });
        assert!(cpu.eval_condition(0, &wm, 0, HdTileSource::default()));
        assert!(ppu.eval_condition(0, &wm, 0, HdTileSource::default()));
    }

    #[test]
    fn eval_frame_range_boundaries() {
        // period 60, offset 30: holds for frame%60 in [30, 59].
        let pack = pack_with_condition(ConditionKind::FrameRange {
            period: 60,
            offset: 30,
        });
        let wm = WatchedMemory::new();
        assert!(!pack.eval_condition(0, &wm, 29, HdTileSource::default()));
        assert!(pack.eval_condition(0, &wm, 30, HdTileSource::default()));
        assert!(pack.eval_condition(0, &wm, 59, HdTileSource::default()));
        assert!(!pack.eval_condition(0, &wm, 60, HdTileSource::default())); // wraps to 0
        assert!(pack.eval_condition(0, &wm, 90, HdTileSource::default())); // 90%60=30
    }

    #[test]
    fn eval_per_tile_mirror_and_palette() {
        let wm = WatchedMemory::new();
        let rec = HdTileSource {
            is_sprite: true,
            flip_h: true,
            palette: 2,
            ..HdTileSource::default()
        };

        let h = pack_with_condition(ConditionKind::HMirror);
        assert!(h.eval_condition(0, &wm, 0, rec));
        let v = pack_with_condition(ConditionKind::VMirror);
        assert!(!v.eval_condition(0, &wm, 0, rec));
        let sp = pack_with_condition(ConditionKind::SpritePalette { id: 2 });
        assert!(sp.eval_condition(0, &wm, 0, rec));
        let sp_no = pack_with_condition(ConditionKind::SpritePalette { id: 1 });
        assert!(!sp_no.eval_condition(0, &wm, 0, rec));

        // A background pixel never satisfies the sprite-only conditions.
        let bg = HdTileSource::default();
        assert!(!h.eval_condition(0, &wm, 0, bg));
    }

    #[test]
    fn unresolved_condition_index_fails_closed() {
        let pack = pack_with_condition(ConditionKind::HMirror);
        // Index 5 doesn't exist.
        assert!(!pack.eval_condition(5, &WatchedMemory::new(), 0, HdTileSource::default()));
    }

    // ---- end-to-end compositing with gating ----

    /// Make a solid-colour RGBA image of the given size.
    fn solid_image(w: u32, h: u32, rgba: [u8; 4]) -> ReplacementImage {
        ReplacementImage {
            width: w,
            height: h,
            rgba: rgba
                .iter()
                .copied()
                .cycle()
                .take((w * h * 4) as usize)
                .collect(),
        }
    }

    /// A black NES framebuffer + a tile-source whose top-left cell points at a
    /// known CHR address; everything else transparent.
    fn one_tile_scene(chr_addr: u16) -> (Vec<u8>, Vec<HdTileSource>) {
        let fb = vec![0u8; (NES_W * NES_H * 4) as usize];
        let mut ts = vec![HdTileSource::default(); (NES_W * NES_H) as usize];
        // top-left cell (pixel 0).
        ts[0] = HdTileSource {
            chr_addr,
            palette: 0,
            is_sprite: false,
            flip_h: false,
            flip_v: false,
        };
        (fb, ts)
    }

    /// CHR bytes (all zero) hash for the `hash_tile` path.
    fn zero_chr_hash() -> u32 {
        crc32(&[0u8; 16])
    }

    #[test]
    fn tile_substitution_gated_by_condition() {
        let hash = zero_chr_hash();
        let red = solid_image(8, 8, [0xFF, 0, 0, 0xFF]);
        // One conditional rule for `hash`, gated on condition 0 (memoryCheck).
        let mut tiles = HashMap::new();
        tiles.insert(
            hash,
            vec![TileRule {
                image: 0,
                x: 0,
                y: 0,
                conditions: vec![0],
            }],
        );
        let pack = HdPack {
            scale: 1,
            images: vec![red],
            tiles,
            pattern_tables: Vec::new(),
            conditions: vec![Condition {
                name: "c".into(),
                kind: ConditionKind::MemoryCheckConstant {
                    addr: 0x10,
                    op: CmpOp::Eq,
                    operand: 0x01,
                    mask: 0xFF,
                },
            }],
            backgrounds: Vec::new(),
            watched_addresses: vec![0x10],
            audio_decls: Vec::new(),
        };
        let mut comp = HdCompositor::new(pack);
        let (fb, ts) = one_tile_scene(0x0000);

        // Condition FALSE -> no substitution (top-left pixel stays black).
        let mut wm = WatchedMemory::new();
        wm.set(0x10, 0x00);
        let out = comp.composite(&fb, &ts, &wm, |_| 0);
        assert_eq!(&out[0..4], &[0, 0, 0, 0]);

        // Condition TRUE -> red substitution at the top-left pixel.
        let mut wm2 = WatchedMemory::new();
        wm2.set(0x10, 0x01);
        let out = comp.composite(&fb, &ts, &wm2, |_| 0);
        assert_eq!(&out[0..4], &[0xFF, 0, 0, 0xFF]);
    }

    #[test]
    fn background_region_renders_only_when_condition_holds() {
        // A green full-screen background gated on a memory condition.
        let green = solid_image(NES_W, NES_H, [0, 0xFF, 0, 0xFF]);
        let pack = HdPack {
            scale: 1,
            images: vec![green],
            tiles: HashMap::new(),
            pattern_tables: Vec::new(),
            conditions: vec![Condition {
                name: "show".into(),
                kind: ConditionKind::MemoryCheckConstant {
                    addr: 0x40,
                    op: CmpOp::Ne,
                    operand: 0x00,
                    mask: 0xFF,
                },
            }],
            backgrounds: vec![BackgroundRegion {
                image: 0,
                x: 0,
                y: 0,
                priority: 1,
                conditions: vec![0],
            }],
            watched_addresses: vec![0x40],
            audio_decls: Vec::new(),
        };
        let mut comp = HdCompositor::new(pack);
        let fb = vec![0u8; (NES_W * NES_H * 4) as usize];
        let ts = vec![HdTileSource::default(); (NES_W * NES_H) as usize];

        // Condition false -> background not drawn (stays black).
        let mut off = WatchedMemory::new();
        off.set(0x40, 0x00);
        let out = comp.composite(&fb, &ts, &off, |_| 0);
        assert_eq!(&out[0..4], &[0, 0, 0, 0]);

        // Condition true -> green covers the frame.
        let mut on = WatchedMemory::new();
        on.set(0x40, 0x01);
        let out = comp.composite(&fb, &ts, &on, |_| 0);
        assert_eq!(&out[0..4], &[0, 0xFF, 0, 0xFF]);
    }

    #[test]
    fn unconditional_tile_rule_still_applies() {
        // Regression: a rule with no conditions must still substitute (the
        // v1.2.0 behaviour).
        let hash = zero_chr_hash();
        let blue = solid_image(8, 8, [0, 0, 0xFF, 0xFF]);
        let mut tiles = HashMap::new();
        tiles.insert(
            hash,
            vec![TileRule {
                image: 0,
                x: 0,
                y: 0,
                conditions: Vec::new(),
            }],
        );
        let pack = HdPack {
            scale: 1,
            images: vec![blue],
            tiles,
            pattern_tables: Vec::new(),
            conditions: Vec::new(),
            backgrounds: Vec::new(),
            watched_addresses: Vec::new(),
            audio_decls: Vec::new(),
        };
        let mut comp = HdCompositor::new(pack);
        let (fb, ts) = one_tile_scene(0x0000);
        let out = comp.composite(&fb, &ts, &WatchedMemory::new(), |_| 0);
        assert_eq!(&out[0..4], &[0, 0, 0xFF, 0xFF]);
    }

    #[test]
    fn watched_addresses_collected_from_conditions() {
        let src = "<condition>a,memoryCheckConstant,0x0075,==,3\n\
                   <condition>b,memoryCheck,0x10,!=,0x11\n\
                   <condition>c,ppuMemoryCheckConstant,0x3F00,==,0x0F\n\
                   <condition>f,frameRange,2,1\n\
                   <tile>deadbeef,t.png,0,0,a\n";
        // Need an image so finish() keeps the rule; fabricate via load path is
        // heavy, so test the collection logic on the parsed conditions directly.
        let parsed = parse_hires(src);
        // 4 conditions parsed; frameRange contributes no watched address.
        assert_eq!(parsed.conditions.len(), 4);
        let mut watched: Vec<u32> = Vec::new();
        for cond in &parsed.conditions {
            match &cond.kind {
                ConditionKind::MemoryCheckConstant { addr, .. } => watched.push(*addr),
                ConditionKind::MemoryCheck { addr_a, addr_b, .. } => {
                    watched.push(*addr_a);
                    watched.push(*addr_b);
                }
                _ => {}
            }
        }
        assert!(watched.contains(&0x0075));
        assert!(watched.contains(&0x10));
        assert!(watched.contains(&0x11));
        assert!(watched.contains(&(0x3F00 | PPU_MEMORY_MARKER)));
        assert_eq!(watched.len(), 4); // frameRange added none.
    }
}

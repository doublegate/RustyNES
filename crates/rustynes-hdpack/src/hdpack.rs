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
//! feature-gated [`rustynes_ppu::HdTileSource`] telemetry (which
//! is itself output-only) and a per-frame snapshot of the finite set of watched
//! memory addresses referenced by the parsed conditions. Both are reads of
//! already-deterministic state taken at PRODUCE time (under the emu lock); the
//! compositor itself only reads them. It mutates no emulation state and adds no
//! determinism surface. When no pack is loaded — or the `hd-pack` feature is off
//! — the presentation is byte-identical to the stock build.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use rustynes_ppu::{HD_CHR_RAM, HD_TILE_NONE, HdTileSource};

use crate::hd_audio::{HdAudioDecl, TrackKind, parse_audio_decl};

/// NES visible framebuffer dimensions (were the frontend's `gfx` constants).
const NES_W: u32 = 256;
const NES_H: u32 = 240;

/// NES tiles are 8x8.
const TILE: usize = 8;

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
    /// The 8x8 cell's screen pixel X (`cell_x * 8`) `<op> value`. Mesen
    /// `positionCheckX` / `originPositionCheckX` — identical here because the
    /// composite keys on the 8-aligned cell grid, so a tile's position and its
    /// origin coincide.
    PositionCheckX { op: CmpOp, value: u8 },
    /// The 8x8 cell's screen pixel Y (`cell_y * 8`) `<op> value`. Mesen
    /// `positionCheckY` / `originPositionCheckY`.
    PositionCheckY { op: CmpOp, value: u8 },
    /// The 8x8 tile `(dx, dy)` pixels away from this cell has CHR tile index
    /// `tile`. Mesen `tileNearby`. Palette matching is intentionally dropped:
    /// the telemetry carries the palette *group* (0..=3), not the four resolved
    /// colours Mesen compares, so we gate on the tile index alone (effectively
    /// Mesen's `ignorePalette`). See `docs/adr/0014`.
    TileNearby { dx: i32, dy: i32, tile: u8 },
    /// The 8x8 cell `(dx, dy)` pixels away is sourced from a sprite. Mesen
    /// `spriteNearby`.
    SpriteNearby { dx: i32, dy: i32 },
}

/// Per-cell spatial context for the position / neighbour conditions: the current
/// 8x8 cell's grid coordinates and the whole frame's per-pixel tile-source slice
/// (so a `tileNearby` / `spriteNearby` can look up a relative cell).
#[derive(Clone, Copy)]
struct SpatialCtx<'a> {
    cell_x: usize,
    cell_y: usize,
    tile_source: &'a [HdTileSource],
}

impl SpatialCtx<'_> {
    /// The tile-source record of the cell `(dx, dy)` *pixels* away (the offsets
    /// are 8-aligned in practice), or `None` if it falls off-screen.
    fn nearby(self, dx: i32, dy: i32) -> Option<HdTileSource> {
        // `try_from` after the offset rejects negatives (off the left/top edge);
        // the `>=` checks reject the right/bottom edge. No signed casts of the
        // screen dims (clippy::cast_possible_wrap).
        let x = usize::try_from(i32::try_from(self.cell_x * TILE).ok()? + dx).ok()?;
        let y = usize::try_from(i32::try_from(self.cell_y * TILE).ok()? + dy).ok()?;
        if x >= NES_W as usize || y >= NES_H as usize {
            return None;
        }
        self.tile_source.get(y * NES_W as usize + x).copied()
    }
}

/// The CHR tile number (0..=255) carried by a [`HdTileSource::chr_addr`]
/// (`tile = (addr >> 4) & 0xFF`).
const fn tile_index(chr_addr: u16) -> u8 {
    ((chr_addr >> 4) & 0xFF) as u8
}

/// A named, parsed condition.
#[derive(Debug, Clone)]
struct Condition {
    #[allow(dead_code)]
    name: String,
    kind: ConditionKind,
    /// When set, the condition's result is logically inverted. Mesen declares a
    /// matching `!name` inverted twin for every `<condition>`, referenced from a
    /// `[!name]` line prefix.
    inverted: bool,
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
    /// Mesen tile `brightness` (`stof * 255`, `255` = identity), applied to the
    /// sampled replacement texel.
    brightness: i32,
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
    /// Draw priority (Mesen's `<background>` priority field; default 10 when the
    /// field is absent, matching Mesen). Higher = drawn later = on top.
    priority: i32,
    /// Conditions that must ALL hold for the region to render (AND). Empty =
    /// always.
    conditions: Vec<usize>,
    /// Mesen background `brightness` (`stof * 255`, `255` = identity).
    brightness: i32,
    /// Mesen background `blendMode` (field 7; default `Alpha`).
    blend_mode: BlendMode,
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
    /// v1.8.9 — `<overscan>` crop margins in NES pixels `[top, right, bottom,
    /// left]` (Mesen `<overscan>Top,Right,Bottom,Left`). The composite output is
    /// `(256-left-right) x (240-top-bottom)`, scaled. All-zero = no crop.
    overscan: [u32; 4],
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
            Self::load_zip_from_reader(std::fs::File::open(path).ok()?)
        } else {
            Self::load_folder(path)
        }
    }

    /// Load an HD-pack from in-memory `.zip` bytes — for hosts with no filesystem
    /// path (e.g. an Android SAF stream). Same parsing as [`Self::load`]'s zip
    /// branch; the bytes are read through a `Cursor`.
    #[must_use]
    pub fn load_from_zip_bytes(data: &[u8]) -> Option<Self> {
        Self::load_zip_from_reader(std::io::Cursor::new(data))
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

    fn load_zip_from_reader<R: std::io::Read + std::io::Seek>(reader: R) -> Option<Self> {
        let mut archive = zip::ZipArchive::new(reader).ok()?;
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
                brightness: rule.brightness,
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
                brightness: bg.brightness,
                blend_mode: bg.blend_mode,
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
            overscan: parsed.overscan,
        })
    }

    /// Evaluate a single condition by index against the current frame state.
    fn eval_condition(
        &self,
        idx: usize,
        watched: &WatchedMemory,
        frame: u32,
        rec: HdTileSource,
        spatial: SpatialCtx,
    ) -> bool {
        let Some(cond) = self.conditions.get(idx) else {
            // An unresolved condition reference fails closed.
            return false;
        };
        let held = match &cond.kind {
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
            ConditionKind::PositionCheckX { op, value } => {
                // cell_x * 8 <= 248, always fits u8.
                op.apply(
                    u8::try_from(spatial.cell_x * TILE).unwrap_or(u8::MAX),
                    *value,
                )
            }
            ConditionKind::PositionCheckY { op, value } => op.apply(
                u8::try_from(spatial.cell_y * TILE).unwrap_or(u8::MAX),
                *value,
            ),
            ConditionKind::TileNearby { dx, dy, tile } => spatial
                .nearby(*dx, *dy)
                .is_some_and(|t| t.chr_addr != HD_TILE_NONE && tile_index(t.chr_addr) == *tile),
            ConditionKind::SpriteNearby { dx, dy } => {
                spatial.nearby(*dx, *dy).is_some_and(|t| t.is_sprite)
            }
        };
        held ^ cond.inverted
    }

    /// Whether ALL of `conditions` hold (AND); empty = always true.
    fn all_hold(
        &self,
        conditions: &[usize],
        watched: &WatchedMemory,
        frame: u32,
        rec: HdTileSource,
        spatial: SpatialCtx,
    ) -> bool {
        conditions
            .iter()
            .all(|&i| self.eval_condition(i, watched, frame, rec, spatial))
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

fn read_zip_entry<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<Vec<u8>> {
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

/// A parsed tile rule before image-index reindex (image is the `<img>`
/// declaration index, conditions are resolved indices into `conditions`).
struct ParsedTileRule {
    image: usize,
    x: u32,
    y: u32,
    conditions: Vec<usize>,
    brightness: i32,
}

/// A parsed background region before image reindex.
struct ParsedBackground {
    image: usize,
    x: i32,
    y: i32,
    priority: i32,
    conditions: Vec<usize>,
    brightness: i32,
    blend_mode: BlendMode,
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
    /// v1.8.9 — `<overscan>` crop `[top, right, bottom, left]` (NES pixels).
    overscan: [u32; 4],
}

/// Strip a leading `[Cond1&Cond2&...]` condition prefix off a `hires.txt` line
/// (Mesen attaches per-line conditions this way, AND-joined). Returns the
/// `&`-split condition names (with any leading `!` inversion marker kept on the
/// name so it can resolve against an inverted condition) and the remainder of
/// the line. If there is no prefix, the name list is empty.
fn split_line_conditions(line: &str) -> (Vec<String>, &str) {
    let Some(rest) = line.strip_prefix('[') else {
        return (Vec::new(), line);
    };
    let Some(end) = rest.find(']') else {
        // Malformed prefix: treat the whole line as having no condition prefix.
        return (Vec::new(), line);
    };
    let names = rest[..end]
        .split('&')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    (names, rest[end + 1..].trim_start())
}

/// Parse the supported subset of a real Mesen `hires.txt` (format `<ver>` up to
/// the current Mesen revision, e.g. 100..=200).
///
/// Mesen's format is line-oriented; each line is optionally prefixed with a
/// `[Cond1&Cond2]` condition list and then a `<tag>` followed by comma-separated
/// fields. We recognize `<ver>` / `<scale>` / `<patternTable>` / `<options>` /
/// `<supportedRom>` headers, `<img>NAME` (indexed by declaration order),
/// `<condition>`, `<tile>`, and `<background>`. Lines we do not recognize
/// (overlays, additions, fallbacks, patches) are ignored; malformed lines are
/// skipped. The real `<tile>` layout is
/// `bitmapIndex,tileData,palette,x,y,brightness,defaultTile[,chrBankPage,tileIndex]`,
/// and the tile match key is the CRC-32 of the 16-byte CHR bitmap (`tileData`) —
/// the exact key [`HdCompositor::composite`] computes from the live CHR snapshot.
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
    let mut overscan = [0u32; 4];

    // First pass over `<condition>` / `<img>` (and the headers + audio decls) so
    // forward-referenced names resolve. `<img>` indices are declaration order, so
    // they MUST be interned in this first pass before any `<tile>` references one.
    // Tiles + backgrounds (which reference conditions + images) are resolved in
    // the second pass. A condition / image declaration is never itself behind a
    // condition prefix, so the prefix is stripped + ignored for these tags.
    for raw in src.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        // A condition / image / header / audio declaration is never itself
        // behind a `[...]` condition prefix (only `<tile>` / `<background>`
        // rules carry one), so split the tag directly off the line and skip the
        // per-line `split_line_conditions` work this first pass doesn't use.
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
                    // Mesen declares a matching inverted `!name` twin for every
                    // condition; register both so a `[!name]` prefix resolves.
                    let inv = Condition {
                        name: format!("!{}", cond.name),
                        kind: cond.kind.clone(),
                        inverted: true,
                    };
                    cond_name_to_idx.insert(cond.name.clone(), conditions.len());
                    conditions.push(cond);
                    cond_name_to_idx.insert(inv.name.clone(), conditions.len());
                    conditions.push(inv);
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
            // v1.8.9 — `<overscan>Top,Right,Bottom,Left` (NES-pixel crop margins).
            "overscan" => {
                let nums: Vec<u32> = rest
                    .split(',')
                    .filter_map(|s| s.trim().parse::<u32>().ok())
                    .collect();
                if nums.len() == 4 {
                    overscan = [nums[0], nums[1], nums[2], nums[3]];
                }
            }
            // `<ver>`, `<options>`, `<supportedRom>`, `<overscan>`, `<patch>`,
            // overlays, additions, fallbacks, etc. are accepted-and-ignored (the
            // supported-subset compositor doesn't act on them, but their presence
            // must not reject the pack).
            _ => {}
        }
    }

    // Second pass: tiles + backgrounds, resolving condition + image refs. The
    // condition list comes from the line's `[...]` prefix.
    for raw in src.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        let (prefix_conds, body) = split_line_conditions(line);
        let Some((tag, rest)) = split_tag(body) else {
            continue;
        };
        match tag {
            "tile" => {
                if let Some((hash, img_idx, x, y, brightness)) = parse_tile_fields(rest) {
                    let conditions = resolve_condition_refs(&prefix_conds, &cond_name_to_idx);
                    // Skip a tile rule that names a condition we never parsed.
                    let Some(conditions) = conditions else {
                        continue;
                    };
                    tiles.push((
                        hash,
                        ParsedTileRule {
                            image: img_idx,
                            x,
                            y,
                            conditions,
                            brightness,
                        },
                    ));
                }
            }
            "background" => {
                if let Some((img_name, x, y, priority, brightness, blend_mode)) =
                    parse_background_fields(rest)
                {
                    let image = intern_name(&mut image_names, &mut name_to_idx, &img_name);
                    let Some(conditions) = resolve_condition_refs(&prefix_conds, &cond_name_to_idx)
                    else {
                        continue;
                    };
                    backgrounds.push(ParsedBackground {
                        image,
                        x,
                        y,
                        priority,
                        conditions,
                        brightness,
                        blend_mode,
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
        overscan,
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

/// Parse a hex memory address (Mesen reads condition memory addresses with
/// `HexUtilities::FromHex`, so a bare `16` is `0x16` and `62D` is `0x62D`). A
/// leading `@` (a `RustyNES`-additive marker) also tags [`PPU_MEMORY_MARKER`].
fn parse_hex_addr(s: &str) -> Option<u32> {
    let s = s.trim();
    let (ppu, body) = s.strip_prefix('@').map_or((false, s), |rest| (true, rest));
    let body = body
        .strip_prefix("0x")
        .or_else(|| body.strip_prefix("0X"))
        .or_else(|| body.strip_prefix('$'))
        .unwrap_or(body);
    let addr = u32::from_str_radix(body, 16).ok()? & 0xFFFF;
    Some(if ppu { addr | PPU_MEMORY_MARKER } else { addr })
}

/// Parse a hex byte value (`HexUtilities::FromHex`). A value that does not fit in
/// 8 bits is REJECTED (`None`) rather than silently truncated — a condition
/// operand / mask wider than a byte is a malformed pack field, so the rule that
/// references it should be dropped instead of matching against a wrong value.
fn parse_hex_u8(s: &str) -> Option<u8> {
    let s = s.trim();
    let body = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .or_else(|| s.strip_prefix('$'))
        .unwrap_or(s);
    u32::from_str_radix(body, 16)
        .ok()
        .and_then(|v| u8::try_from(v).ok())
}

/// Parse a `<condition>` line: `NAME,TYPE,args...`.
///
/// Supported `TYPE`s: `memoryCheck` / `ppuMemoryCheck`, `memoryCheckConstant` /
/// `ppuMemoryCheckConstant`, `frameRange`, `hmirror`, `vmirror`, `sppalette` (+
/// the indexed `sppalette0..3` Mesen global-condition names). Per the real Mesen
/// loader, memory addresses + operands + masks are parsed as **hex**.
///
/// The spatial types `positionCheckX/Y`, `originPositionCheckX/Y`, `tileNearby`,
/// and `spriteNearby` are supported as of v1.8.9 (the existing per-pixel
/// tile-source telemetry carries the cell position + neighbour tiles). Still
/// unsupported (return `None`, so a gated tile is dropped): `tileAtPosition` /
/// `spriteAtPosition` (absolute coordinates) and `tileNearby`'s 32-char
/// tile-data-hash + palette-colour match forms — see `docs/adr/0014`.
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
        // memoryCheckConstant: NAME,type,addr,op,operand[,mask] (all hex).
        "memoryCheckConstant" | "ppuMemoryCheckConstant" => {
            if fields.len() < 5 {
                return None;
            }
            let mut addr = parse_hex_addr(fields[2])?;
            if ty.starts_with("ppu") {
                addr |= PPU_MEMORY_MARKER;
            }
            let op = CmpOp::parse(fields[3])?;
            let operand = parse_hex_u8(fields[4])?;
            let mask = fields.get(5).and_then(|m| parse_hex_u8(m)).unwrap_or(0xFF);
            ConditionKind::MemoryCheckConstant {
                addr,
                op,
                operand,
                mask,
            }
        }
        // memoryCheck: NAME,type,addrA,op,addrB[,mask] (addresses hex).
        "memoryCheck" | "ppuMemoryCheck" => {
            if fields.len() < 5 {
                return None;
            }
            let mut addr_a = parse_hex_addr(fields[2])?;
            let op = CmpOp::parse(fields[3])?;
            let mut addr_b = parse_hex_addr(fields[4])?;
            if ty.starts_with("ppu") {
                addr_a |= PPU_MEMORY_MARKER;
                addr_b |= PPU_MEMORY_MARKER;
            }
            let mask = fields.get(5).and_then(|m| parse_hex_u8(m)).unwrap_or(0xFF);
            ConditionKind::MemoryCheck {
                addr_a,
                addr_b,
                op,
                mask,
            }
        }
        // frameRange: NAME,frameRange,period,offset (decimal for v102+).
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
        // Mesen's indexed global-condition aliases `sppalette0`..`sppalette3`
        // (the palette group is encoded in the type name, no `id` arg field).
        "sppalette0" => ConditionKind::SpritePalette { id: 0 },
        "sppalette1" => ConditionKind::SpritePalette { id: 1 },
        "sppalette2" => ConditionKind::SpritePalette { id: 2 },
        "sppalette3" => ConditionKind::SpritePalette { id: 3 },
        // The v1.8.9 spatial conditions live in their own parser (keeps this one
        // short); an unrecognized type still parses to `None` (inert).
        other => {
            let kind = parse_spatial(other, &fields)?;
            return Some(Condition {
                name: name.to_string(),
                kind,
                inverted: false,
            });
        }
    };
    Some(Condition {
        name: name.to_string(),
        kind,
        inverted: false,
    })
}

/// Parse the v1.8.9 spatial condition types (position / tile-nearby / sprite-
/// nearby); `None` for anything else (so it's dropped as inert).
fn parse_spatial(ty: &str, fields: &[&str]) -> Option<ConditionKind> {
    Some(match ty {
        // positionCheckX/Y + originPositionCheckX/Y: NAME,type,op,value. The
        // origin variants coincide with the plain ones on the 8-aligned cell grid.
        "positionCheckX" | "originPositionCheckX" => {
            if fields.len() < 4 {
                return None;
            }
            let op = CmpOp::parse(fields[2])?;
            let value = u8::try_from(parse_int(fields[3])? & 0xFF).ok()?;
            ConditionKind::PositionCheckX { op, value }
        }
        "positionCheckY" | "originPositionCheckY" => {
            if fields.len() < 4 {
                return None;
            }
            let op = CmpOp::parse(fields[2])?;
            let value = u8::try_from(parse_int(fields[3])? & 0xFF).ok()?;
            ConditionKind::PositionCheckY { op, value }
        }
        // tileNearby: NAME,tileNearby,x,y,tileIndex(hex)[,palette][,ignorePalette].
        // The 32-char tile-data-hash form and the palette match are unsupported
        // (the telemetry has no tile-data hash and only the palette group), so a
        // rule using them parses to `None` and is dropped.
        "tileNearby" => {
            if fields.len() < 5 {
                return None;
            }
            let dx = fields[2].parse::<i32>().ok()?;
            let dy = fields[3].parse::<i32>().ok()?;
            let tile = parse_hex_u8(fields[4])?;
            ConditionKind::TileNearby { dx, dy, tile }
        }
        // spriteNearby: NAME,spriteNearby,x,y,... — only the offset matters to us.
        "spriteNearby" => {
            if fields.len() < 4 {
                return None;
            }
            let dx = fields[2].parse::<i32>().ok()?;
            let dy = fields[3].parse::<i32>().ok()?;
            ConditionKind::SpriteNearby { dx, dy }
        }
        _ => return None,
    })
}

/// Parse the comma-separated fields of a real Mesen `<tile>` rule into
/// `(chrKey, bitmapIndex, x, y)`.
///
/// The real `<ver>`>=100 layout is
/// `bitmapIndex,tileData,palette,x,y,brightness,defaultTile[,chrBankPage,tileIndex]`
/// — e.g. `31,00000000000000007F3F1F0F07030100,0F162736,80,224,1,N,4018065946,231`:
///
/// - field 0 = `bitmapIndex` (index into the `<img>` declarations),
/// - field 1 = `tileData` (32 hex chars = the tile's 16 CHR bytes; CHR-RAM form),
/// - field 2 = `palette` (the tile's 4-colour palette, hex = Mesen `PaletteColors`)
///   — a first-class part of the tile identity (`HdTileKey`), so the key is
///   `CalculateHash(palette ++ tileData)`,
/// - field 3 = `x`, field 4 = `y` (the replacement rectangle's top-left),
/// - field 5 = `brightness`, field 6 = `defaultTile` (Y/N — a `Y` tile matches
///   regardless of palette, so it is keyed under the `0xFFFFFFFF` palette
///   wildcard),
/// - trailing `chrBankPage,tileIndex` for CHR-RAM tiles — informational.
///
/// The conditions come from the line's `[...]` prefix, not a trailing field.
/// Returns `None` for a CHR-ROM (short index) tile — that path needs the absolute
/// CHR offset and is a follow-up — or a malformed line.
/// Parse a Mesen brightness field (`stof * 255`, default identity `255`). Bounded.
fn parse_brightness(field: Option<&str>) -> i32 {
    field
        .and_then(|s| s.trim().parse::<f32>().ok())
        .map_or(BRIGHTNESS_IDENTITY, |f| {
            #[allow(clippy::cast_possible_truncation)]
            let v = (f * 255.0) as i32;
            v.clamp(0, 255 * 64)
        })
}

/// Parse a Mesen `blendMode` field name (`Add` / `Subtract`, else `Alpha`).
fn parse_blend_mode(field: Option<&str>) -> BlendMode {
    match field.map(str::trim) {
        Some("Add") => BlendMode::Add,
        Some("Subtract") => BlendMode::Subtract,
        _ => BlendMode::Alpha,
    }
}

fn parse_tile_fields(rest: &str) -> Option<(u32, usize, u32, u32, i32)> {
    let fields: Vec<&str> = rest.split(',').map(str::trim).collect();
    // Need at least bitmapIndex, tileData, palette, x, y.
    if fields.len() < 5 {
        return None;
    }
    let bitmap_index = usize::try_from(parse_int(fields[0])?).ok()?;
    // field 2 = palette (Mesen `PaletteColors`, hex). A `defaultTile` (field 6 ==
    // "Y") is palette-agnostic: key it under the `0xFFFFFFFF` wildcard so the
    // lookup's second stage finds it regardless of the live palette.
    let palette = u32::from_str_radix(fields[2], 16).unwrap_or(0xFFFF_FFFF);
    let is_default = fields.get(6).is_some_and(|f| f.eq_ignore_ascii_case("Y"));
    let key_palette = if is_default { 0xFFFF_FFFF } else { palette };
    let key = if fields[1].len() >= 32 {
        // CHR-RAM: 32-hex `tileData` = the 16 CHR bytes -> content key.
        let mut tile_data = [0u8; 16];
        for (i, b) in tile_data.iter_mut().enumerate() {
            *b = u8::from_str_radix(fields[1].get(i * 2..i * 2 + 2)?, 16).ok()?;
        }
        chr_ram_key(key_palette, &tile_data)
    } else {
        // CHR-ROM: a short field is the absolute tile INDEX (hex for v104+ packs;
        // Mesen `TileIndex`). Key by `TileIndex ^ palette`.
        let tile_index = u32::from_str_radix(fields[1], 16).ok()?;
        chr_rom_key(tile_index, key_palette)
    };
    let x = parse_int(fields[3])?;
    let y = parse_int(fields[4])?;
    // field 5 = brightness (applied to the sampled texel).
    let brightness = parse_brightness(fields.get(5).copied());
    Some((key, bitmap_index, x, y, brightness))
}

/// Parse a real Mesen `<background>` line into `(imageName, x, y, priority)`.
///
/// The real layout is
/// `name,brightness[,hScroll,vScroll][,priority][,left,top][,blendMode]` — e.g.
/// `CreditsFlashFixRed.png,1,1,1,11`: name, brightness=1, hScroll=1, vScroll=1,
/// priority=11. The conditions come from the line's `[...]` prefix.
///
/// `RustyNES`'s compositor places the background at `(left, top)` and orders by
/// `priority` (Mesen's default priority is 10 when the field is absent; `<` it
/// draws under, `>=` over — here we map the Mesen priority straight through and
/// the compositor's under/over split keys on a signed comparison, so a default
/// Mesen background renders OVER the tile pass, matching Mesen). Scroll ratios +
/// blend mode are parsed-and-ignored (subset). A bare `name` with no priority
/// field is accepted (full-screen, the Mesen default priority 10).
fn parse_background_fields(rest: &str) -> Option<(String, i32, i32, i32, i32, BlendMode)> {
    let fields: Vec<&str> = rest.split(',').map(str::trim).collect();
    if fields.is_empty() || fields[0].is_empty() {
        return None;
    }
    let image = fields[0].to_string();
    // field 1 = brightness (ignored). fields 2,3 = scroll ratios (ignored).
    // field 4 = priority (v106+; Mesen default 10 when absent). fields 5,6 =
    // left,top.
    let priority = fields
        .get(4)
        .and_then(|p| p.parse::<i32>().ok())
        .unwrap_or(10);
    let x = fields
        .get(5)
        .and_then(|p| p.parse::<i32>().ok())
        .unwrap_or(0);
    let y = fields
        .get(6)
        .and_then(|p| p.parse::<i32>().ok())
        .unwrap_or(0);
    // field 1 = brightness; field 7 = blendMode (Mesen `<background>` layout).
    let brightness = parse_brightness(fields.get(1).copied());
    let blend_mode = parse_blend_mode(fields.get(7).copied());
    Some((image, x, y, priority, brightness, blend_mode))
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
    hash_cache: HashMap<(u16, bool, bool, u32), (u32, u32)>,
    /// Monotonic frame counter for `frameRange` conditions. Advances once per
    /// [`Self::composite`]; presentation-only, never serialized.
    frame: u32,
    /// v1.8.9 — `<overscan>` crop in NES pixels: left/top origin + the visible
    /// width/height. The composite skips cropped-out pixels and shifts the rest
    /// so the output is the cropped region (`crop_w*scale x crop_h*scale`).
    ov_left: u32,
    ov_top: u32,
    crop_w: u32,
    crop_h: u32,
}

impl HdCompositor {
    /// Build a compositor for a loaded pack.
    #[must_use]
    pub fn new(pack: HdPack) -> Self {
        let scale = pack.scale();
        // `<overscan>` = [top, right, bottom, left]; crop, clamped to >= 1 cell.
        let [top, right, bottom, left] = pack.overscan;
        let crop_w = NES_W.saturating_sub(left + right).max(1);
        let crop_h = NES_H.saturating_sub(top + bottom).max(1);
        let out_w = crop_w * scale;
        let out_h = crop_h * scale;
        Self {
            pack,
            out: vec![0u8; (out_w * out_h * 4) as usize],
            out_w,
            out_h,
            hash_cache: HashMap::new(),
            frame: 0,
            ov_left: left,
            ov_top: top,
            crop_w,
            crop_h,
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
    #[allow(clippy::too_many_lines)] // upscale + under-bgs + per-pixel tiles + over-bgs.
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
        // `<overscan>` crop origin + visible extent (NES pixels).
        let ov_left = self.ov_left as usize;
        let ov_top = self.ov_top as usize;
        let crop_w = self.crop_w as usize;
        let crop_h = self.crop_h as usize;

        // 1) Nearest-neighbour upscale of the base framebuffer (overscan-cropped:
        //    skip cropped-out pixels, shift the rest to the output origin).
        for y in ov_top..ov_top + crop_h {
            for x in ov_left..ov_left + crop_w {
                let src = (y * NES_W as usize + x) * 4;
                let px = &framebuffer[src..src + 4];
                let (ox, oy) = (x - ov_left, y - ov_top);
                for sy in 0..scale {
                    let row = (oy * scale + sy) * out_w;
                    for sx in 0..scale {
                        let dst = (row + ox * scale + sx) * 4;
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
            i64::from(self.ov_left),
            i64::from(self.ov_top),
        );

        // 3) PER PIXEL (Mesen's renderer model): for each NES pixel, resolve the
        //    tile that produced it and sample the matched replacement at the
        //    pixel's own texel offset (`offset_x`/`offset_y`). Unlike a per-cell
        //    blit, this tracks fine-X/Y scroll and sprite position pixel-for-pixel
        //    so the HD tile sits exactly over the original (no offset / drag).
        //    The tile-key hash is still computed once per distinct tile identity
        //    (cached), so the per-pixel cost is a hash-map lookup + sample.
        self.hash_cache.clear();
        let out_h = self.out_h as usize;
        // Overscan-cropped: iterate the visible region; `tile_source` is still
        // indexed in full-frame NES coords (conditions key on the NES position).
        for y in ov_top..ov_top + crop_h {
            for x in ov_left..ov_left + crop_w {
                let rec = tile_source[y * NES_W as usize + x];
                if rec.chr_addr == HD_TILE_NONE {
                    continue;
                }
                // Tile identity (incl. palette) -> (exact, wildcard) keys, cached.
                let cache_key = (rec.chr_addr, rec.flip_h, rec.flip_v, rec.palette_colors);
                let (exact, wild) = if let Some(&kw) = self.hash_cache.get(&cache_key) {
                    kw
                } else {
                    let kw = tile_keys(rec, &mut chr_peek);
                    self.hash_cache.insert(cache_key, kw);
                    kw
                };
                // Two-stage lookup (Mesen `GetMatchingTile`): exact palette+content
                // key first, then the palette-agnostic default key.
                let Some(rules) = self
                    .pack
                    .tiles
                    .get(&exact)
                    .or_else(|| self.pack.tiles.get(&wild))
                else {
                    continue;
                };
                // First rule whose conditions all hold wins (unconditional rules
                // sorted last). Spatial conditions key on the containing 8x8 cell.
                let spatial = SpatialCtx {
                    cell_x: x / TILE,
                    cell_y: y / TILE,
                    tile_source,
                };
                let Some(rule) = rules.iter().find(|r| {
                    self.pack
                        .all_hold(&r.conditions, watched, frame, rec, spatial)
                }) else {
                    continue;
                };
                let Some(img) = self.pack.images.get(rule.image) else {
                    continue;
                };
                // Sample the replacement at the texel this pixel maps to (flips are
                // already baked into offset_x/offset_y), writing the scale*scale
                // block. Transparent texels (alpha 0) leave the upscaled base.
                let img_w = img.width as usize;
                let img_h = img.height as usize;
                let src_tx = rule.x as usize + usize::from(rec.offset_x) * scale;
                let src_ty = rule.y as usize + usize::from(rec.offset_y) * scale;
                for sub_y in 0..scale {
                    let sy = src_ty + sub_y;
                    let dy = (y - ov_top) * scale + sub_y;
                    if sy >= img_h || dy >= out_h {
                        continue;
                    }
                    for sub_x in 0..scale {
                        let sx = src_tx + sub_x;
                        let dx = (x - ov_left) * scale + sub_x;
                        if sx >= img_w || dx >= out_w {
                            continue;
                        }
                        let soff = (sy * img_w + sx) * 4;
                        let alpha = img.rgba[soff + 3];
                        if alpha == 0 {
                            continue;
                        }
                        // v1.8.9 — alpha-BLEND partial-alpha texels (soft edges)
                        // instead of a hard binary cutout, with the tile's
                        // brightness applied (Mesen DrawTile). Tiles are Alpha mode.
                        let doff = (dy * out_w + dx) * 4;
                        blend_over(
                            &mut self.out,
                            doff,
                            &img.rgba[soff..soff + 4],
                            alpha,
                            BlendMode::Alpha,
                            rule.brightness,
                        );
                    }
                }
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
            i64::from(self.ov_left),
            i64::from(self.ov_top),
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
        let spatial = SpatialCtx {
            cell_x,
            cell_y,
            tile_source,
        };

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
        let (exact, wild) = tile_keys(rec, &mut chr_peek);
        out.chr_hash = Some(exact);
        let Some(rules) = self
            .pack
            .tiles
            .get(&exact)
            .or_else(|| self.pack.tiles.get(&wild))
        else {
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
                    held: self.pack.eval_condition(i, watched, frame, rec, spatial),
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
        ov_left: i64,
        ov_top: i64,
    ) {
        // Full-screen backgrounds aren't tied to a cell, so spatial conditions
        // (position / nearby) have no cell to anchor on and fail closed here.
        let no_spatial = SpatialCtx {
            cell_x: 0,
            cell_y: 0,
            tile_source: &[],
        };
        for bg in &pack.backgrounds {
            if under != (bg.priority < 0) {
                continue;
            }
            if !pack.all_hold(
                &bg.conditions,
                watched,
                frame,
                HdTileSource::default(),
                no_spatial,
            ) {
                continue;
            }
            let Some(img) = pack.images.get(bg.image) else {
                continue;
            };
            blit_background(out, out_h, out_w, scale, img, bg, ov_left, ov_top);
        }
    }
}

/// Alpha-blit one background region into `out`.
// scale (≤ 8) + the source pixel indices are small + bounded, so the i64 casts
// used to do signed destination-bounds math can never wrap.
#[allow(clippy::cast_possible_wrap, clippy::too_many_arguments)]
fn blit_background(
    out: &mut [u8],
    out_h: usize,
    out_w: usize,
    scale: usize,
    img: &ReplacementImage,
    bg: &BackgroundRegion,
    ov_left: i64,
    ov_top: i64,
) {
    let img_w = img.width as usize;
    let img_h = img.height as usize;
    let scale_i = scale as i64;
    // Destination origin in upscaled space (i64 to avoid overflow / wrap),
    // shifted by the overscan crop origin.
    let ox = (i64::from(bg.x) - ov_left) * scale_i;
    let oy = (i64::from(bg.y) - ov_top) * scale_i;
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
            blend_over(out, d, &img.rgba[s..s + 4], a, bg.blend_mode, bg.brightness);
        }
    }
}

/// HD-pack blend mode for a replacement layer (Mesen `HdPackBlendMode`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BlendMode {
    /// Standard source-over alpha (the default).
    Alpha,
    /// Additive: `out = min(255, out + in)`.
    Add,
    /// Subtractive: `out = max(0, out - in)`.
    Subtract,
}

/// Identity brightness (Mesen stores `stof(field) * 255`, so `255` ~= 1.0x).
const BRIGHTNESS_IDENTITY: i32 = 255;

/// Mesen `AdjustBrightness`: `min(255, (brightness * (v + 1)) >> 8)`. With
/// `brightness == 255` this is ~identity; lower dims, higher brightens.
fn adjust_brightness(v: u8, brightness: i32) -> u8 {
    u8::try_from(((brightness * (i32::from(v) + 1)) >> 8).clamp(0, 255)).unwrap_or(0xFF)
}

/// Blend a 4-byte RGBA `src` (alpha `a`) onto the opaque base `out[d..d+4]` under
/// `mode`, after applying `brightness` to the source RGB. `Alpha` is a
/// premultiply-free, round-to-nearest source-over blend; `Add`/`Subtract` are
/// saturating. Used by both the tile blit and the background blit.
fn blend_over(out: &mut [u8], d: usize, src: &[u8], a: u8, mode: BlendMode, brightness: i32) {
    let src_rgb = [
        adjust_brightness(src[0], brightness),
        adjust_brightness(src[1], brightness),
        adjust_brightness(src[2], brightness),
    ];
    match mode {
        BlendMode::Alpha => {
            if a == 0xFF {
                out[d..d + 3].copy_from_slice(&src_rgb);
            } else {
                let inv = 255 - u16::from(a);
                for (ch, &sval) in src_rgb.iter().enumerate() {
                    let num = u16::from(sval) * u16::from(a) + u16::from(out[d + ch]) * inv + 127;
                    out[d + ch] = u8::try_from(num / 255).unwrap_or(0xFF);
                }
            }
        }
        BlendMode::Add => {
            for (ch, &sval) in src_rgb.iter().enumerate() {
                out[d + ch] = u8::try_from((i32::from(out[d + ch]) + i32::from(sval)).min(255))
                    .unwrap_or(0xFF);
            }
        }
        BlendMode::Subtract => {
            for (ch, &sval) in src_rgb.iter().enumerate() {
                out[d + ch] =
                    u8::try_from((i32::from(out[d + ch]) - i32::from(sval)).max(0)).unwrap_or(0);
            }
        }
    }
    // The base upscale is opaque; keep dst alpha opaque after any mode.
    out[d + 3] = 0xFF;
}

/// Mesen's tile-key hash (`HdTileKey::CalculateHash`, `HdData.h:56-68`): an
/// additive rolling hash with a rotate-left-2 over little-endian u32 chunks.
/// This is NOT CRC-32 — it is the exact function the real Mesen `<tile>` keys
/// (and CHR-RAM packs like Zelda) were generated with, so `RustyNES` must mirror
/// it bit-for-bit or no tile matches.
fn calculate_hash(key: &[u8]) -> u32 {
    let mut result: u32 = 0;
    for chunk in key.chunks_exact(4) {
        let val = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        result = result.wrapping_add(val).rotate_left(2);
    }
    result
}

/// The CHR-RAM tile key: `CalculateHash(PaletteColors(4 LE) ++ TileData(16))` —
/// 20 bytes, palette first, exactly Mesen's `HdTileKey` memory layout for a
/// CHR-RAM tile (`HdData.h:16-18,36-37`). Palette is a first-class part of the
/// identity; the palette-agnostic *default* key passes `0xFFFFFFFF`.
fn chr_ram_key(palette_colors: u32, tile_data: &[u8; 16]) -> u32 {
    let mut buf = [0u8; 20];
    buf[0..4].copy_from_slice(&palette_colors.to_le_bytes());
    buf[4..20].copy_from_slice(tile_data);
    calculate_hash(&buf)
}

/// The live tile's `(exact_key, default_key)` for the two-stage lookup: the
/// exact key uses the pixel's actual palette, the default key uses the
/// `0xFFFFFFFF` palette wildcard (matching pack tiles flagged `defaultTile`).
/// Reads the raw, *unflipped* CHR bytes (flips are applied later by the
/// renderer, so a flipped sprite keys to the same tile dump).
fn tile_keys(rec: HdTileSource, chr_peek: &mut impl FnMut(u16) -> u8) -> (u32, u32) {
    // CHR-ROM tiles are keyed by their absolute tile index (Mesen `TileIndex ^
    // PaletteColors`), not by content — the pack stores the index, not 16 bytes.
    if rec.chr_tile_index != HD_CHR_RAM {
        return (
            chr_rom_key(rec.chr_tile_index, rec.palette_colors),
            chr_rom_key(rec.chr_tile_index, 0xFFFF_FFFF),
        );
    }
    let base = rec.chr_addr & 0x1FF0;
    let mut bytes = [0u8; 16];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = chr_peek(base + u16::try_from(i).unwrap_or(0));
    }
    (
        chr_ram_key(rec.palette_colors, &bytes),
        chr_ram_key(0xFFFF_FFFF, &bytes),
    )
}

/// The CHR-ROM tile key — Mesen `HdTileKey::GetHashCode` for a CHR-ROM tile is
/// `(uint32_t)TileIndex ^ PaletteColors` (`HdData.h:39`), used directly as the
/// map key (no `CalculateHash`). The palette-agnostic default key passes
/// `0xFFFFFFFF`.
const fn chr_rom_key(tile_index: u32, palette_colors: u32) -> u32 {
    tile_index ^ palette_colors
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty spatial context for the non-spatial condition tests (no cell, no
    /// neighbour slice). The spatial conditions get their own tests below.
    const SP: SpatialCtx = SpatialCtx {
        cell_x: 0,
        cell_y: 0,
        tile_source: &[],
    };

    #[test]
    fn crc32_matches_known_vectors() {
        // Standard CRC-32 of the empty string is 0; of "123456789" is 0xCBF43926.
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn calculate_hash_matches_mesen_rotate_add() {
        // Mesen's CalculateHash: additive + rotate-left-2 over LE u32 chunks.
        // empty -> 0; one chunk val=1 -> (0+1) rl2 = 4; two chunks {1,0} ->
        // ((1 rl2)=4 then (4+0) rl2)=16.
        assert_eq!(calculate_hash(&[]), 0);
        assert_eq!(calculate_hash(&[1, 0, 0, 0]), 4);
        assert_eq!(calculate_hash(&[1, 0, 0, 0, 0, 0, 0, 0]), 16);
    }

    #[test]
    fn chr_ram_key_is_palette_sensitive_and_deterministic() {
        let td = [0x12u8; 16];
        assert_ne!(chr_ram_key(0x0010_2030, &td), chr_ram_key(0x0040_5060, &td));
        assert_eq!(chr_ram_key(0xABCD, &td), chr_ram_key(0xABCD, &td));
    }

    #[test]
    fn tile_rule_keys_on_palette_and_default_wildcard() {
        let td_hex = "000102030405060708090A0B0C0D0E0F";
        let mut td = [0u8; 16];
        for (i, b) in td.iter_mut().enumerate() {
            *b = u8::from_str_radix(&td_hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        // bitmapIndex, tileData, palette, x, y, brightness, defaultTile=N.
        let line = format!("0,{td_hex},0F162736,16,32,1,N");
        let (key, _, x, y, _) = parse_tile_fields(&line).unwrap();
        assert_eq!(key, chr_ram_key(0x0F16_2736, &td));
        assert_eq!((x, y), (16, 32));
        // defaultTile=Y -> keyed under the 0xFFFFFFFF palette wildcard.
        let dflt = format!("0,{td_hex},0F162736,16,32,1,Y");
        let (dkey, ..) = parse_tile_fields(&dflt).unwrap();
        assert_eq!(dkey, chr_ram_key(0xFFFF_FFFF, &td));
    }

    #[test]
    fn chr_rom_tile_keys_on_index_and_palette() {
        // A CHR-ROM rec (chr_tile_index != HD_CHR_RAM) keys by TileIndex ^ palette,
        // NOT by content (chr_peek is never consulted).
        let rec = HdTileSource {
            palette_colors: 0x0F16_2736,
            chr_tile_index: 42,
            ..HdTileSource::default()
        };
        let mut peek = |_a: u16| panic!("CHR-ROM must not read CHR content");
        let (exact, wild) = tile_keys(rec, &mut peek);
        assert_eq!(exact, chr_rom_key(42, 0x0F16_2736));
        assert_eq!(wild, chr_rom_key(42, 0xFFFF_FFFF));
    }

    #[test]
    fn parses_chr_rom_tile_index_form() {
        // bitmapIndex, tileIndex(hex), palette, x, y, brightness, defaultTile.
        let (key, _, x, y, _) = parse_tile_fields("0,2A,0F162736,16,32,1,N").unwrap();
        assert_eq!(key, chr_rom_key(0x2A, 0x0F16_2736));
        assert_eq!((x, y), (16, 32));
        // defaultTile=Y -> the palette wildcard.
        let (dkey, ..) = parse_tile_fields("0,2A,0F162736,16,32,1,Y").unwrap();
        assert_eq!(dkey, chr_rom_key(0x2A, 0xFFFF_FFFF));
    }

    #[test]
    fn overscan_parses_and_crops_dimensions() {
        // <overscan>Top,Right,Bottom,Left.
        let parsed = parse_hires("<overscan>8,16,8,16\n");
        assert_eq!(parsed.overscan, [8, 16, 8, 16]);
        // The compositor output is (256-left-right) x (240-top-bottom), scaled.
        let mut pack = pack_with_condition(ConditionKind::HMirror);
        pack.scale = 2;
        pack.overscan = [8, 16, 8, 16];
        let comp = HdCompositor::new(pack);
        assert_eq!(comp.dimensions(), ((256 - 32) * 2, (240 - 16) * 2));
    }

    /// The 32-hex `tileData` of all-zero CHR bytes (the common blank tile) and
    /// the CRC-32 key it maps to — used across the real-format parse tests.
    const ZERO_TILE_DATA: &str = "00000000000000000000000000000000";

    #[test]
    fn parses_scale_and_unconditional_tile() {
        // Real Mesen <ver>106 layout: bitmapIndex,tileData,palette,x,y,bright,def.
        let src = format!(
            "<ver>106\n\
             <scale>2\n\
             <patternTable>bank0.png\n\
             <img>tiles.png\n\
             <tile>0,{ZERO_TILE_DATA},0F162736,16,0,1,N\n"
        );
        let parsed = parse_hires(&src);
        assert_eq!(parsed.scale, 2);
        assert_eq!(parsed.pattern_tables, vec!["bank0.png".to_string()]);
        assert_eq!(parsed.tiles.len(), 1);
        let (hash, rule) = &parsed.tiles[0];
        // The match key is `chr_ram_key(palette, tileData)` (palette 0F162736).
        assert_eq!(*hash, chr_ram_key(0x0F16_2736, &[0u8; 16]));
        assert_eq!(rule.image, 0, "bitmap index 0 = first <img>");
        assert_eq!(rule.x, 16);
        assert_eq!(rule.y, 0);
        assert!(rule.conditions.is_empty());
        assert_eq!(parsed.image_names, vec!["tiles.png".to_string()]);
    }

    #[test]
    fn parses_real_chr_ram_tile_with_trailing_bank_index() {
        // A real Zelda-pack line: the trailing chrBankPage,tileIndex must not
        // break parsing, and x/y come from fields 3/4 (NOT a condition ref).
        let src = "<img>Chr_0.png\n<tile>0,00000000000000007F3F1F0F07030100,0F162736,80,224,1,N,4018065946,231\n";
        let parsed = parse_hires(src);
        assert_eq!(parsed.tiles.len(), 1);
        let (hash, rule) = &parsed.tiles[0];
        let mut bytes = [0u8; 16];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = u8::from_str_radix(&"00000000000000007F3F1F0F07030100"[i * 2..i * 2 + 2], 16)
                .unwrap();
        }
        assert_eq!(*hash, chr_ram_key(0x0F16_2736, &bytes));
        assert_eq!(rule.x, 80);
        assert_eq!(rule.y, 224);
    }

    #[test]
    fn line_condition_prefix_gates_tile() {
        // The condition is a `[...]` PREFIX, not a trailing field.
        let src = format!(
            "<condition>flag,memoryCheckConstant,10,==,9\n\
             [flag]<tile>0,{ZERO_TILE_DATA},00000000,0,0,1,N\n"
        );
        let parsed = parse_hires(&src);
        assert_eq!(parsed.tiles.len(), 1);
        // memoryCheckConstant declares a base + an inverted twin (indices 0,1).
        assert_eq!(parsed.tiles[0].1.conditions, vec![0]);
    }

    #[test]
    fn inverted_condition_prefix_resolves() {
        let src = format!(
            "<condition>flag,memoryCheckConstant,10,==,9\n\
             [!flag]<tile>0,{ZERO_TILE_DATA},00000000,0,0,1,N\n"
        );
        let parsed = parse_hires(&src);
        assert_eq!(parsed.tiles.len(), 1);
        // !flag is index 1 (the inverted twin).
        assert_eq!(parsed.tiles[0].1.conditions, vec![1]);
        assert!(parsed.conditions[1].inverted);
    }

    #[test]
    fn tile_gated_on_unsupported_condition_is_dropped() {
        // tileAtPosition (absolute coords) is still outside RustyNES's telemetry
        // -> the rule is dropped. (tileNearby / positionCheck are supported now.)
        let src = format!(
            "<condition>at,tileAtPosition,0,0,{ZERO_TILE_DATA},0F123712\n\
             [at]<tile>0,{ZERO_TILE_DATA},00000000,0,0,1,N\n"
        );
        let parsed = parse_hires(&src);
        assert!(
            parsed.tiles.is_empty(),
            "unsupported condition -> drop rule"
        );
    }

    #[test]
    fn ignores_unknown_tags() {
        let src = "<overlay>x,y,z\n<bgmCondition>a\n<supportedRom>DEADBEEF\n<options>disableOriginalTiles\n";
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
        // The base condition + its inverted `!lives` twin.
        assert_eq!(parsed.conditions.len(), 2);
        assert_eq!(parsed.conditions[0].name, "lives");
        assert_eq!(parsed.conditions[1].name, "!lives");
        assert!(parsed.conditions[1].inverted);
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
    fn parses_indexed_sppalette_global_conditions() {
        // Mesen's indexed `sppalette0..3` global-condition aliases (the palette
        // group is in the type name, no `id` arg field).
        for id in 0u8..=3 {
            let src = format!("<condition>p{id},sppalette{id}\n");
            let parsed = parse_hires(&src);
            match parsed.conditions[0].kind {
                ConditionKind::SpritePalette { id: got } => assert_eq!(got, id),
                _ => panic!("sppalette{id} should parse to SpritePalette {{ id: {id} }}"),
            }
        }
    }

    #[test]
    fn parse_hex_u8_rejects_out_of_range() {
        // A value that fits in 8 bits parses; a wider value is rejected (not
        // silently truncated to its low byte).
        assert_eq!(parse_hex_u8("FF"), Some(0xFF));
        assert_eq!(parse_hex_u8("0x7F"), Some(0x7F));
        assert_eq!(parse_hex_u8("100"), None); // 0x100 does not fit in a u8
        assert_eq!(parse_hex_u8("1FF"), None);
    }

    #[test]
    fn condition_with_out_of_range_operand_is_dropped() {
        // An operand wider than a byte makes the whole condition unparseable, so
        // a tile gated on it is dropped rather than matching a truncated value.
        let src = "<condition>bad,memoryCheckConstant,0x10,==,0x1FF\n";
        let parsed = parse_hires(src);
        assert!(
            parsed.conditions.is_empty(),
            "out-of-range operand -> condition rejected"
        );
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
        let src = format!(
            "<condition>lives,memoryCheckConstant,75,>=,3\n\
             <img>tiles.png\n\
             [lives]<tile>0,{ZERO_TILE_DATA},00000000,16,0,1,N\n"
        );
        let parsed = parse_hires(&src);
        assert_eq!(parsed.tiles.len(), 1);
        assert_eq!(parsed.tiles[0].1.conditions, vec![0]);
    }

    #[test]
    fn tile_with_unknown_condition_is_dropped() {
        // The condition name was never declared -> the rule's gate is unknown.
        let src = format!("[missing]<tile>0,{ZERO_TILE_DATA},00000000,16,0,1,N\n");
        let parsed = parse_hires(&src);
        assert!(parsed.tiles.is_empty());
    }

    #[test]
    fn tile_with_anded_conditions() {
        // frameRange a,b each register a base + inverted twin: a=0, b=2.
        let src = format!(
            "<condition>a,frameRange,2,1\n\
             <condition>b,frameRange,4,2\n\
             [a&b]<tile>0,{ZERO_TILE_DATA},00000000,0,0,1,N\n"
        );
        let parsed = parse_hires(&src);
        assert_eq!(parsed.tiles[0].1.conditions, vec![0, 2]);
    }

    #[test]
    fn parses_background_full_screen_and_region() {
        // Real Mesen <background> layout: name,brightness,hScroll,vScroll,
        // priority,left,top — conditions are a `[...]` line prefix.
        let src = "<background>bg.png,1\n\
                   [nightcond]<background>panel.png,1,1,1,3,16,32\n\
                   <condition>nightcond,frameRange,2,1\n";
        let parsed = parse_hires(src);
        assert_eq!(parsed.backgrounds.len(), 2);
        assert_eq!(parsed.backgrounds[0].x, 0);
        assert_eq!(parsed.backgrounds[0].y, 0);
        // No priority field present -> Mesen default priority 10.
        assert_eq!(parsed.backgrounds[0].priority, 10);
        assert!(parsed.backgrounds[0].conditions.is_empty());
        assert_eq!(parsed.backgrounds[1].priority, 3);
        assert_eq!(parsed.backgrounds[1].x, 16);
        assert_eq!(parsed.backgrounds[1].y, 32);
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
                inverted: false,
            }],
            backgrounds: Vec::new(),
            watched_addresses: Vec::new(),
            audio_decls: Vec::new(),
            overscan: [0; 4],
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
            let got = pack.eval_condition(0, &wm, 0, HdTileSource::default(), SP);
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
        assert!(pack.eval_condition(0, &wm, 0, HdTileSource::default(), SP));
        // Unmasked it would be 0xA5 != 0x05.
        let pack2 = pack_with_condition(ConditionKind::MemoryCheckConstant {
            addr: 0x20,
            op: CmpOp::Eq,
            operand: 0x05,
            mask: 0xFF,
        });
        assert!(!pack2.eval_condition(0, &wm, 0, HdTileSource::default(), SP));
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
        assert!(pack.eval_condition(0, &wm, 0, HdTileSource::default(), SP));
        wm.set(0x31, 0x08);
        assert!(!pack.eval_condition(0, &wm, 0, HdTileSource::default(), SP));
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
        assert!(cpu.eval_condition(0, &wm, 0, HdTileSource::default(), SP));
        assert!(ppu.eval_condition(0, &wm, 0, HdTileSource::default(), SP));
    }

    #[test]
    fn eval_frame_range_boundaries() {
        // period 60, offset 30: holds for frame%60 in [30, 59].
        let pack = pack_with_condition(ConditionKind::FrameRange {
            period: 60,
            offset: 30,
        });
        let wm = WatchedMemory::new();
        assert!(!pack.eval_condition(0, &wm, 29, HdTileSource::default(), SP));
        assert!(pack.eval_condition(0, &wm, 30, HdTileSource::default(), SP));
        assert!(pack.eval_condition(0, &wm, 59, HdTileSource::default(), SP));
        assert!(!pack.eval_condition(0, &wm, 60, HdTileSource::default(), SP)); // wraps to 0
        assert!(pack.eval_condition(0, &wm, 90, HdTileSource::default(), SP)); // 90%60=30
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
        assert!(h.eval_condition(0, &wm, 0, rec, SP));
        let v = pack_with_condition(ConditionKind::VMirror);
        assert!(!v.eval_condition(0, &wm, 0, rec, SP));
        let sp = pack_with_condition(ConditionKind::SpritePalette { id: 2 });
        assert!(sp.eval_condition(0, &wm, 0, rec, SP));
        let sp_no = pack_with_condition(ConditionKind::SpritePalette { id: 1 });
        assert!(!sp_no.eval_condition(0, &wm, 0, rec, SP));

        // A background pixel never satisfies the sprite-only conditions.
        let bg = HdTileSource::default();
        assert!(!h.eval_condition(0, &wm, 0, bg, SP));
    }

    #[test]
    fn unresolved_condition_index_fails_closed() {
        let pack = pack_with_condition(ConditionKind::HMirror);
        // Index 5 doesn't exist.
        assert!(!pack.eval_condition(5, &WatchedMemory::new(), 0, HdTileSource::default(), SP));
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
            palette_colors: 0,
            offset_x: 0,
            offset_y: 0,
            chr_tile_index: HD_CHR_RAM,
        };
        (fb, ts)
    }

    /// The CHR-RAM tile key (palette 0, all-zero CHR) the composite computes for
    /// the test cell above — what a pack rule must be keyed under to match.
    fn zero_chr_hash() -> u32 {
        chr_ram_key(0, &[0u8; 16])
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
                brightness: BRIGHTNESS_IDENTITY,
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
                inverted: false,
            }],
            backgrounds: Vec::new(),
            watched_addresses: vec![0x10],
            audio_decls: Vec::new(),
            overscan: [0; 4],
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
                inverted: false,
            }],
            backgrounds: vec![BackgroundRegion {
                image: 0,
                x: 0,
                y: 0,
                priority: 1,
                conditions: vec![0],
                brightness: BRIGHTNESS_IDENTITY,
                blend_mode: BlendMode::Alpha,
            }],
            watched_addresses: vec![0x40],
            audio_decls: Vec::new(),
            overscan: [0; 4],
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
                brightness: BRIGHTNESS_IDENTITY,
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
            overscan: [0; 4],
        };
        let mut comp = HdCompositor::new(pack);
        let (fb, ts) = one_tile_scene(0x0000);
        let out = comp.composite(&fb, &ts, &WatchedMemory::new(), |_| 0);
        assert_eq!(&out[0..4], &[0, 0, 0xFF, 0xFF]);
    }

    #[test]
    fn watched_addresses_collected_from_conditions() {
        let src = "<condition>a,memoryCheckConstant,75,==,3\n\
                   <condition>b,memoryCheck,10,!=,11\n\
                   <condition>c,ppuMemoryCheckConstant,3F00,==,0F\n\
                   <condition>f,frameRange,2,1\n";
        let parsed = parse_hires(src);
        // 4 declared conditions, each with an inverted twin -> 8 entries.
        assert_eq!(parsed.conditions.len(), 8);
        // Collect distinct watched addresses (dedup across base + twin).
        let mut watched: Vec<u32> = Vec::new();
        for cond in &parsed.conditions {
            let mut add = |a: u32| {
                if !watched.contains(&a) {
                    watched.push(a);
                }
            };
            match &cond.kind {
                ConditionKind::MemoryCheckConstant { addr, .. } => add(*addr),
                ConditionKind::MemoryCheck { addr_a, addr_b, .. } => {
                    add(*addr_a);
                    add(*addr_b);
                }
                _ => {}
            }
        }
        // Addresses are hex: 75=0x75, 10=0x10, 11=0x11, 3F00 in PPU space.
        assert!(watched.contains(&0x0075));
        assert!(watched.contains(&0x10));
        assert!(watched.contains(&0x11));
        assert!(watched.contains(&(0x3F00 | PPU_MEMORY_MARKER)));
        assert_eq!(watched.len(), 4); // frameRange added none.
    }

    // ---- v1.7.0 G5: real Mesen <ver>106 parse regression ----

    /// A tiny but real-format `hires.txt`: a `<ver>106` header, options, two
    /// `<img>` declarations, a `memoryCheckConstant` condition, two
    /// unconditional `<tile>`s, one condition-gated `<tile>` (via `[...]`
    /// prefix), and a `<background>`. Mirrors the structure of a real Zelda /
    /// SMB HD pack without shipping any copyrighted asset. The PNGs it names do
    /// NOT exist, so it exercises the PARSER (`parse_hires`), not the full
    /// image-decoding `load()` path.
    const SAMPLE_VER106: &str = "\
<ver>106
<scale>2
<overscan>0,0,0,0
<options>disableOriginalTiles
<supportedRom>DAB79C84934F9AA5DB4E7DAD390E5D0C12443FA2
<img>Chr_0.png
<img>Chr_1.png
<condition>SaveSlot1,memoryCheckConstant,16,==,0
# a real Mesen comment / section marker line
<tile>0,00000000000000007F3F1F0F07030100,0F162736,80,224,1,N,4018065946,231
<tile>1,0000000000A854FF0000000000A85400,FF022230,48,128,1,N,1011652562,134
[SaveSlot1]<tile>0,54A800000000000054A8000000000000,FF022230,48,144,1,N
<background>Backdrop.png,1,0,0,11
";

    #[test]
    fn parses_real_ver106_sample_to_nonzero_rules() {
        let parsed = parse_hires(SAMPLE_VER106);
        assert_eq!(parsed.scale, 2);
        // Two <img> declarations interned in order.
        assert_eq!(parsed.image_names[0], "Chr_0.png");
        assert_eq!(parsed.image_names[1], "Chr_1.png");
        // Three <tile> rules survive (none gated on an unsupported condition).
        assert_eq!(parsed.tiles.len(), 3, "all three real tiles parse");
        // The bitmap indices are honoured (fields[0] = <img> index).
        assert_eq!(parsed.tiles[0].1.image, 0);
        assert_eq!(parsed.tiles[1].1.image, 1);
        // x/y come from fields 3/4, NOT mis-read as condition refs.
        assert_eq!((parsed.tiles[0].1.x, parsed.tiles[0].1.y), (80, 224));
        assert_eq!((parsed.tiles[1].1.x, parsed.tiles[1].1.y), (48, 128));
        // The third tile is gated on the SaveSlot1 condition (index 0).
        assert_eq!(parsed.tiles[2].1.conditions, vec![0]);
        // memoryCheckConstant 16 = hex 0x16 watched address; one background.
        assert_eq!(parsed.backgrounds.len(), 1);
        assert_eq!(parsed.backgrounds[0].priority, 11);
        // The match key is `chr_ram_key(palette, tileData)` (palette 0F162736).
        let mut td = [0u8; 16];
        for (i, b) in td.iter_mut().enumerate() {
            *b = u8::from_str_radix(&"00000000000000007F3F1F0F07030100"[i * 2..i * 2 + 2], 16)
                .unwrap();
        }
        assert_eq!(parsed.tiles[0].0, chr_ram_key(0x0F16_2736, &td));
    }

    #[test]
    fn loader_key_matches_runtime_key_with_palette() {
        // The loader key parsed from a `<tile>` line MUST equal the live key the
        // compositor computes for a cell with the SAME palette + CHR bytes — the
        // contract that makes a real pack substitute. Palette is part of both.
        let chr: [u8; 16] = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7F, 0x3F, 0x1F, 0x0F, 0x07, 0x03,
            0x01, 0x00,
        ];
        let palette = 0x0F16_2736u32;
        let line = "0,00000000000000007F3F1F0F07030100,0F162736,0,0,1,N";
        let (loader_key, ..) = parse_tile_fields(line).unwrap();
        let rec = HdTileSource {
            chr_addr: 0,
            palette_colors: palette,
            ..HdTileSource::default()
        };
        let mut chr_peek = |a: u16| chr[usize::from(a & 0x0F)];
        let (runtime_key, _) = tile_keys(rec, &mut chr_peek);
        assert_eq!(loader_key, runtime_key);
    }

    /// v1.7.1 (#3) — the full runtime contract: a `<tile>` rule whose `tileData`
    /// is the CRC of a known 16-byte CHR tile MUST substitute when the live
    /// compositor hashes those same 16 CHR bytes (read via `chr_peek`), and a
    /// tile whose CHR bytes do NOT match any rule key MUST fall through to the
    /// original. This is the loader-key ↔ runtime-key alignment the #3 fix
    /// depends on, exercised end-to-end through `composite` (the GPU blit itself
    /// is maintainer-manual; this drives the CPU compositor only).
    #[test]
    fn real_format_tile_substitutes_when_chr_matches_and_falls_through_otherwise() {
        // A real Mesen <ver>106 tile line: tileData = the 16 CHR bytes at a given
        // pattern address. Place those bytes in a synthetic CHR snapshot at that
        // address so the live hash reproduces the loader key.
        const TILE_DATA: &str = "00000000000000007F3F1F0F07030100";
        let mut chr_bytes = [0u8; 16];
        for (i, b) in chr_bytes.iter_mut().enumerate() {
            *b = u8::from_str_radix(&TILE_DATA[i * 2..i * 2 + 2], 16).unwrap();
        }
        // The pack rule is keyed exactly as the live cell will be: palette 0
        // (the one_tile_scene rec) + these CHR bytes.
        let key = chr_ram_key(0, &chr_bytes);

        // Build the pack with a single unconditional rule keyed by that tileData,
        // mapping to a solid-red replacement image.
        let red = solid_image(8, 8, [0xFF, 0, 0, 0xFF]);
        let mut tiles = HashMap::new();
        tiles.insert(
            key,
            vec![TileRule {
                image: 0,
                x: 0,
                y: 0,
                conditions: Vec::new(),
                brightness: BRIGHTNESS_IDENTITY,
            }],
        );
        let pack = HdPack {
            scale: 1,
            images: vec![red],
            tiles,
            pattern_tables: Vec::new(),
            conditions: Vec::new(),
            backgrounds: Vec::new(),
            watched_addresses: Vec::new(),
            audio_decls: Vec::new(),
            overscan: [0; 4],
        };
        let mut comp = HdCompositor::new(pack);

        // The top-left cell shows the tile at CHR base 0x0040 (a non-zero address,
        // so the `& 0x1FF0` masking + `chr_peek` offset are exercised, not a
        // degenerate 0x0000 case). A `let` (not a `const` item) keeps clippy's
        // `items_after_statements` happy now that statements precede it.
        let base: u16 = 0x0040;
        let (fb, ts) = one_tile_scene(base);

        // A CHR snapshot that holds `chr_bytes` exactly at [base..base+16].
        let mut chr = vec![0u8; 0x2000];
        chr[base as usize..base as usize + 16].copy_from_slice(&chr_bytes);
        let peek = |addr: u16| chr.get((addr & 0x1FFF) as usize).copied().unwrap_or(0);

        // Matching CHR -> the rule substitutes (top-left pixel becomes red).
        let out = comp.composite(&fb, &ts, &WatchedMemory::new(), peek);
        assert_eq!(
            &out[0..4],
            &[0xFF, 0, 0, 0xFF],
            "the loader key and the live hash must agree so the tile substitutes"
        );

        // Now zero the CHR at that address: the live hash no longer matches the
        // rule key, so the cell falls through to the (black) original.
        let chr2 = vec![0u8; 0x2000];
        // (chr2 already all-zero -> a different CRC than `key`.)
        let peek2 = |addr: u16| chr2.get((addr & 0x1FFF) as usize).copied().unwrap_or(0);
        let out = comp.composite(&fb, &ts, &WatchedMemory::new(), peek2);
        assert_eq!(
            &out[0..4],
            &[0, 0, 0, 0],
            "a non-matching CHR tile must NOT substitute"
        );
    }

    // LOCAL-ONLY verification against a real copyrighted pack. Never committed to
    // run in CI: gated on `RUSTYNES_HDPACK_LOCAL` pointing at a folder with a
    // `hires.txt`. Run: RUSTYNES_HDPACK_LOCAL=/path cargo test ... -- --ignored
    #[test]
    #[ignore = "needs a local (copyrighted) HD pack via RUSTYNES_HDPACK_LOCAL"]
    fn local_real_pack_parses_nonzero() {
        let Ok(dir) = std::env::var("RUSTYNES_HDPACK_LOCAL") else {
            return;
        };
        let text = std::fs::read_to_string(std::path::Path::new(&dir).join("hires.txt")).unwrap();
        let parsed = parse_hires(&text);
        eprintln!(
            "parsed: scale={} imgs={} tiles={} conds={} bgs={}",
            parsed.scale,
            parsed.image_names.len(),
            parsed.tiles.len(),
            parsed.conditions.len(),
            parsed.backgrounds.len()
        );
        assert!(
            !parsed.tiles.is_empty(),
            "real pack must parse to >0 tile rules"
        );
        // Full load (decodes the PNGs that live alongside hires.txt).
        let pack = HdPack::load(std::path::Path::new(&dir)).expect("real pack loads");
        eprintln!("loaded rule_count={}", pack.rule_count());
        assert!(pack.rule_count() > 0);
    }

    // ---- spatial conditions (v1.8.9) ----

    fn full_tile_source() -> Vec<HdTileSource> {
        vec![HdTileSource::default(); (NES_W * NES_H) as usize]
    }

    #[test]
    fn position_check_x_gates_on_cell_pixel() {
        // cell_x*8 >= 128  <=>  cell_x >= 16.
        let pack = pack_with_condition(ConditionKind::PositionCheckX {
            op: CmpOp::Ge,
            value: 128,
        });
        let rec = HdTileSource::default();
        let at = |cx| SpatialCtx {
            cell_x: cx,
            cell_y: 0,
            tile_source: &[],
        };
        assert!(pack.eval_condition(0, &WatchedMemory::new(), 0, rec, at(16)));
        assert!(!pack.eval_condition(0, &WatchedMemory::new(), 0, rec, at(15)));
    }

    #[test]
    fn position_check_y_gates_on_cell_pixel() {
        // cell_y*8 < 16  <=>  cell_y < 2.
        let pack = pack_with_condition(ConditionKind::PositionCheckY {
            op: CmpOp::Lt,
            value: 16,
        });
        let rec = HdTileSource::default();
        let at = |cy| SpatialCtx {
            cell_x: 0,
            cell_y: cy,
            tile_source: &[],
        };
        assert!(pack.eval_condition(0, &WatchedMemory::new(), 0, rec, at(1)));
        assert!(!pack.eval_condition(0, &WatchedMemory::new(), 0, rec, at(2)));
    }

    #[test]
    fn tile_nearby_matches_neighbour_tile_index() {
        let mut ts = full_tile_source();
        // Place a known tile one cell to the right of (0,0): pixel (8, 0).
        // chr_addr 0x0A0 -> tile index (0x0A0 >> 4) & 0xFF = 0x0A.
        ts[8] = HdTileSource {
            chr_addr: 0x0A0,
            ..HdTileSource::default()
        };
        let ctx = SpatialCtx {
            cell_x: 0,
            cell_y: 0,
            tile_source: &ts,
        };
        let rec = HdTileSource::default();
        let hit = pack_with_condition(ConditionKind::TileNearby {
            dx: 8,
            dy: 0,
            tile: 0x0A,
        });
        assert!(hit.eval_condition(0, &WatchedMemory::new(), 0, rec, ctx));
        // Wrong index, and an off-screen neighbour, both fail closed.
        let miss = pack_with_condition(ConditionKind::TileNearby {
            dx: 8,
            dy: 0,
            tile: 0x0B,
        });
        assert!(!miss.eval_condition(0, &WatchedMemory::new(), 0, rec, ctx));
        let off = pack_with_condition(ConditionKind::TileNearby {
            dx: -8,
            dy: 0,
            tile: 0x0A,
        });
        assert!(!off.eval_condition(0, &WatchedMemory::new(), 0, rec, ctx));
    }

    #[test]
    fn sprite_nearby_detects_a_sprite_cell() {
        let pack = pack_with_condition(ConditionKind::SpriteNearby { dx: 8, dy: 0 });
        let rec = HdTileSource::default();
        let mut sprite_ts = full_tile_source();
        sprite_ts[8] = HdTileSource {
            chr_addr: 0x010,
            is_sprite: true,
            ..HdTileSource::default()
        };
        let sprite_ctx = SpatialCtx {
            cell_x: 0,
            cell_y: 0,
            tile_source: &sprite_ts,
        };
        assert!(pack.eval_condition(0, &WatchedMemory::new(), 0, rec, sprite_ctx));
        // A background neighbour (is_sprite = false) does not satisfy it.
        let mut bg_ts = full_tile_source();
        bg_ts[8] = HdTileSource {
            chr_addr: 0x010,
            is_sprite: false,
            ..HdTileSource::default()
        };
        let bg_ctx = SpatialCtx {
            cell_x: 0,
            cell_y: 0,
            tile_source: &bg_ts,
        };
        assert!(!pack.eval_condition(0, &WatchedMemory::new(), 0, rec, bg_ctx));
    }

    #[test]
    fn parses_spatial_condition_types() {
        assert!(matches!(
            parse_condition("c,positionCheckX,>=,80").unwrap().kind,
            ConditionKind::PositionCheckX {
                op: CmpOp::Ge,
                value: 80
            }
        ));
        // origin* maps to the plain position check on the cell grid.
        assert!(matches!(
            parse_condition("c,originPositionCheckY,<,10").unwrap().kind,
            ConditionKind::PositionCheckY { .. }
        ));
        assert!(matches!(
            parse_condition("c,tileNearby,8,0,0A").unwrap().kind,
            ConditionKind::TileNearby {
                dx: 8,
                dy: 0,
                tile: 0x0A
            }
        ));
        assert!(matches!(
            parse_condition("c,spriteNearby,-8,0,00,0").unwrap().kind,
            ConditionKind::SpriteNearby { dx: -8, dy: 0 }
        ));
        // Absolute-position variants remain unsupported (dropped).
        assert!(parse_condition("c,tileAtPosition,0,0,0A,0").is_none());
    }
}

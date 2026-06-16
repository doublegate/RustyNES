//! v1.2.0 beta.2 (Workstream C3) — HD-pack / mod loader (minimal first cut).
//!
//! Loads a Mesen-style HD-pack (a folder or a `.zip` containing a `hires.txt`)
//! and substitutes hi-res replacement tiles at blit time. This is the *minimal*
//! first cut: it parses ONLY the subset of `hires.txt` needed for unconditional
//! CHR-hash tile replacement —
//!
//! - `<scale>` — the integer upscale factor (replacement tiles are `8*scale`
//!   square),
//! - `<patternTable>` — the CHR bank image references (currently parsed and
//!   retained for completeness; the minimal cut keys substitution on the tile
//!   CHR hash, not the bank image),
//! - `<tile>` rules with NO condition — `(chrHash, image, x, y, ...)` — mapping
//!   a 32-bit Mesen CHR hash to a rectangle inside a replacement image.
//!
//! Everything else (conditions, `<condition>`, palette keys, `<background>`,
//! `<overlay>`, HD audio, `<bgmCondition>`, etc.) is intentionally SKIPPED — it
//! is out of scope for v1.2.0 and ignored rather than rejected, so a real pack
//! still loads (just with the unsupported rules inert).
//!
//! ## Determinism
//!
//! This module is presentation-only and native-only. It consumes the PPU's
//! feature-gated [`rustynes_core::rustynes_ppu::HdTileSource`] telemetry (which
//! is itself output-only) and produces an upscaled RGBA framebuffer. It mutates
//! no emulation state. When no pack is loaded — or the `hd-pack` feature is off
//! — the presentation is byte-identical to the stock build.

#![cfg(feature = "hd-pack")]

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use rustynes_core::rustynes_ppu::{HD_TILE_NONE, HdTileSource};

use crate::gfx::{NES_H, NES_W};

/// NES tiles are 8x8.
const TILE: usize = 8;
/// Visible tile grid: 32 columns x 30 rows.
const COLS: usize = NES_W as usize / TILE; // 32
const ROWS: usize = NES_H as usize / TILE; // 30

/// A decoded replacement image (RGBA8, row-major).
#[derive(Debug)]
struct ReplacementImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// One unconditional CHR-hash tile-replacement rule.
#[derive(Debug, Clone)]
struct TileRule {
    /// Index into [`HdPack::images`].
    image: usize,
    /// Top-left of the replacement rectangle inside the image (in pixels).
    x: u32,
    y: u32,
}

/// A loaded HD-pack: the upscale factor, the decoded replacement images, and
/// the unconditional CHR-hash -> rule map.
#[derive(Debug)]
pub struct HdPack {
    /// Integer upscale factor (`<scale>`); clamped to `1..=8`.
    scale: u32,
    /// Decoded replacement images, indexed by [`TileRule::image`].
    images: Vec<ReplacementImage>,
    /// Unconditional tile rules keyed by the Mesen CHR hash.
    tiles: HashMap<u32, TileRule>,
    /// Pattern-table bank image references (`<patternTable>`), retained for
    /// completeness. Not consulted by the minimal hash-keyed substitution.
    pattern_tables: Vec<String>,
}

impl HdPack {
    /// The upscale factor (replacement tile edge = `8 * scale`).
    #[must_use]
    pub const fn scale(&self) -> u32 {
        self.scale
    }

    /// Number of unconditional tile rules parsed.
    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.tiles.len()
    }

    /// Number of `<patternTable>` references parsed (diagnostic).
    #[must_use]
    pub const fn pattern_table_count(&self) -> usize {
        self.pattern_tables.len()
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
        let mut tiles = HashMap::new();
        for (hash, rule) in parsed.tiles {
            let Some(&new_idx) = remap.get(rule.image) else {
                continue;
            };
            if new_idx == usize::MAX {
                continue;
            }
            tiles.insert(
                hash,
                TileRule {
                    image: new_idx,
                    x: rule.x,
                    y: rule.y,
                },
            );
        }
        if tiles.is_empty() {
            return None;
        }
        Some(Self {
            scale: parsed.scale.clamp(1, 8),
            images: kept,
            tiles,
            pattern_tables: parsed.pattern_tables,
        })
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

/// Intermediate parse result before image decode + reindex.
struct ParsedHires {
    scale: u32,
    image_names: Vec<String>,
    pattern_tables: Vec<String>,
    /// chrHash -> (rule with image index into `image_names`).
    tiles: Vec<(u32, TileRule)>,
}

/// Parse the supported subset of a Mesen `hires.txt`.
///
/// Mesen's format is line-oriented; each line is `<tag>` followed by
/// comma-separated fields. We recognize:
///
/// - `<ver>` / `<scale>` / `<patternTable>` headers,
/// - `<img>NAME` — a replacement-image filename (index in declaration order),
/// - `<tile>` rules of the form `<tile>[chrBankPage],tileX,tileY,chrHash,...`
///   — but we accept the documented unconditional form
///   `<tile>image,x,y,chrHash` and the alternative `<tile>chrHash,image,x,y`
///   and pick fields by sniffing (see [`parse_tile_fields`]).
///
/// Lines we do not recognize (conditions, backgrounds, audio, options) are
/// ignored. Malformed lines are skipped.
fn parse_hires(src: &str) -> ParsedHires {
    let mut scale = 1u32;
    let mut image_names: Vec<String> = Vec::new();
    let mut name_to_idx: HashMap<String, usize> = HashMap::new();
    let mut pattern_tables: Vec<String> = Vec::new();
    let mut tiles: Vec<(u32, TileRule)> = Vec::new();

    let mut intern = |name: &str| -> usize {
        if let Some(&i) = name_to_idx.get(name) {
            return i;
        }
        let i = image_names.len();
        image_names.push(name.to_string());
        name_to_idx.insert(name.to_string(), i);
        i
    };

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
                // `<patternTable>NAME` (an image holding the dumped CHR banks).
                let name = rest.trim();
                if !name.is_empty() {
                    pattern_tables.push(name.to_string());
                }
            }
            "img" => {
                // `<img>NAME` — register a replacement image in declaration order.
                let name = rest.trim();
                if !name.is_empty() {
                    intern(name);
                }
            }
            "tile" => {
                if let Some((hash, img_name, x, y)) = parse_tile_fields(rest) {
                    let image = intern(&img_name);
                    tiles.push((hash, TileRule { image, x, y }));
                }
            }
            _ => {} // condition / background / overlay / options / audio: skip.
        }
    }

    ParsedHires {
        scale,
        image_names,
        pattern_tables,
        tiles,
    }
}

/// Split a `<tag>rest` line into `(tag, rest)`. Returns `None` if not a tag line.
fn split_tag(line: &str) -> Option<(&str, &str)> {
    let line = line.strip_prefix('<')?;
    let close = line.find('>')?;
    Some((&line[..close], &line[close + 1..]))
}

/// Parse the comma-separated fields of a `<tile>` rule into
/// `(chrHash, imageName, x, y)`, accepting only the UNCONDITIONAL forms.
///
/// Mesen's documented tile line is:
///   `<tile>[chrBankPage],tileX,tileY,[palette],[brightness],[default],[hash]`
/// but real packs vary. The minimal cut keys purely on the CHR hash. We accept:
///
/// - `image,x,y,hash`  (image name first), and
/// - `hash,image,x,y`  (hash first),
///
/// disambiguated by whether the first field parses as a hex hash. A trailing
/// condition reference (a non-numeric extra field) marks a CONDITIONAL rule,
/// which the minimal cut SKIPS (returns `None`).
fn parse_tile_fields(rest: &str) -> Option<(u32, String, u32, u32)> {
    let fields: Vec<&str> = rest.split(',').map(str::trim).collect();
    if fields.len() < 4 {
        return None;
    }
    // A 5th+ field that is non-empty and not a plain number indicates a
    // condition / palette key -> out of scope, skip.
    if fields.len() > 4 && fields[4..].iter().any(|f| !f.is_empty()) {
        return None;
    }

    let parse_hash = |s: &str| -> Option<u32> {
        let s = s.trim_start_matches("0x").trim_start_matches("0X");
        u32::from_str_radix(s, 16).ok()
    };

    // Form A: hash,image,x,y
    if let Some(hash) = parse_hash(fields[0])
        && !fields[1].is_empty()
        && let (Ok(x), Ok(y)) = (fields[2].parse::<u32>(), fields[3].parse::<u32>())
    {
        return Some((hash, fields[1].to_string(), x, y));
    }
    // Form B: image,x,y,hash
    if let Some(hash) = parse_hash(fields[3])
        && !fields[0].is_empty()
        && let (Ok(x), Ok(y)) = (fields[1].parse::<u32>(), fields[2].parse::<u32>())
    {
        return Some((hash, fields[0].to_string(), x, y));
    }
    None
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

/// The HD compositor: turns the NES framebuffer + the PPU tile-source telemetry
/// into an upscaled RGBA buffer with replacement tiles blitted over the
/// nearest-neighbour upscale of the base image.
pub struct HdCompositor {
    pack: HdPack,
    /// Reusable output buffer (`scale*256 x scale*240` RGBA8).
    out: Vec<u8>,
    out_w: u32,
    out_h: u32,
    /// CHR-hash cache keyed on `(chr_addr, flip_h, flip_v)` -> hash, refreshed
    /// per frame. Avoids re-reading + re-hashing 16 CHR bytes for repeated tiles.
    hash_cache: HashMap<(u16, bool, bool), u32>,
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

    /// Composite one frame.
    ///
    /// `framebuffer` is the NES RGBA8 image (256x240x4). `tile_source` is the
    /// PPU's per-pixel [`HdTileSource`] telemetry (256x240). `chr_peek(addr)`
    /// returns the CHR byte at a PPU pattern-space address — used to hash a
    /// tile's 16 CHR bytes for the replacement lookup. Returns the upscaled
    /// RGBA8 buffer.
    pub fn composite(
        &mut self,
        framebuffer: &[u8],
        tile_source: &[HdTileSource],
        mut chr_peek: impl FnMut(u16) -> u8,
    ) -> &[u8] {
        debug_assert_eq!(framebuffer.len(), (NES_W * NES_H * 4) as usize);
        debug_assert_eq!(tile_source.len(), (NES_W * NES_H) as usize);
        let scale = self.pack.scale as usize;
        let out_w = self.out_w as usize;

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

        // 2) Per 8x8 cell, resolve the dominant tile identity and, if a
        //    replacement exists for its CHR hash, blit the hi-res image over the
        //    upscaled base. The cell's identity is taken from its top-left pixel
        //    (scrolling shifts whole tiles by < 8px; the minimal cut keys on the
        //    aligned grid, like Mesen's BG path).
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
                let Some(rule) = self.pack.tiles.get(&hash) else {
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
        &self.out
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
    fn skips_conditional_tile_rules() {
        // A 5th non-empty field marks a condition -> out of scope, skipped.
        let src = "<tile>0a1b2c3d,tiles.png,16,0,myCondition\n";
        let parsed = parse_hires(src);
        assert!(parsed.tiles.is_empty());
    }

    #[test]
    fn ignores_unknown_tags() {
        let src = "<condition>x,y,z\n<background>bg.png\n<bgmCondition>a\n";
        let parsed = parse_hires(src);
        assert!(parsed.tiles.is_empty());
        assert_eq!(parsed.scale, 1);
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
}

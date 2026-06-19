//! v1.7.0 "Forge" Workstream G5 — HD-Pack Builder.
//!
//! v1.2.0–v1.6.0 *play* Mesen-style HD-packs (see [`crate::hdpack`]); this module
//! *authors* them. It is an in-emulator recorder: while active it observes the
//! same per-frame PPU tile-source telemetry + CHR snapshot the compositor
//! consumes, accumulates the set of distinct background/sprite tiles the running
//! game actually draws (keyed by the Mesen CRC-32 of each tile's 16 CHR bytes —
//! the exact key [`crate::hdpack`] substitutes on), captures each tile's native
//! 8x8 RGBA pixels once, and on finish emits a Mesen-compatible HD-pack: a packed
//! `tiles.png` tile sheet plus a `hires.txt` manifest with one `<tile>` rule per
//! distinct tile. An artist then repaints the sheet at hi-res and the result
//! loads straight back through [`crate::hdpack`].
//!
//! ## Determinism — output-only, byte-identical
//!
//! Like the loader, this is presentation-only and native-only. It reads the
//! already-deterministic [`HdTileSource`] telemetry + framebuffer + CHR snapshot
//! the present path captured under the emu lock; it mutates no emulation state,
//! adds no determinism surface, and is never serialized into a save-state. When
//! no recording is active — or the `hd-pack` feature is off — the build is
//! byte-identical to stock. See `docs/adr/0017-hd-pack-builder.md`.

#![cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]

use std::collections::HashMap;
use std::path::Path;

use rustynes_core::rustynes_ppu::{HD_TILE_NONE, HdTileSource};

use crate::gfx::{NES_H, NES_W};
use crate::hdpack::crc32;

/// NES tiles are 8x8.
const TILE: usize = 8;
/// Visible tile grid columns (256 / 8).
const COLS: usize = NES_W as usize / TILE; // 32
/// Visible tile grid rows (240 / 8).
const ROWS: usize = NES_H as usize / TILE; // 30

/// The number of tile columns in the emitted sheet. Mesen packs tiles into a
/// grid; 16 columns keeps the sheet a familiar pattern-table-like layout and
/// bounds its width regardless of how many distinct tiles a game uses.
const SHEET_COLS: usize = 16;

/// A captured distinct tile: its native 8x8 RGBA pixels and the slot it occupies
/// in the emitted sheet.
struct CapturedTile {
    /// 8*8*4 = 256 RGBA bytes, row-major, top-left origin (un-flipped — the same
    /// orientation [`crate::hdpack`] expects, since H/V flip is re-applied by the
    /// renderer and the CRC key ignores flips).
    rgba: [u8; TILE * TILE * 4],
    /// Insertion order, used to lay the tile out in the sheet grid.
    index: usize,
    /// The tile's 16 raw CHR bytes (un-flipped), emitted as the real Mesen
    /// `<tile>` `tileData` field (32 hex chars). This IS Mesen's match key.
    chr: [u8; 16],
}

/// Errors produced when emitting an HD-pack.
#[derive(Debug)]
pub enum BuilderError {
    /// No tiles were captured (recording never saw a rendered frame).
    Empty,
    /// A filesystem error writing the pack.
    Io(std::io::Error),
    /// The PNG encoder rejected the sheet.
    Png(String),
}

impl std::fmt::Display for BuilderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("HD-pack builder captured no tiles"),
            Self::Io(e) => write!(f, "HD-pack builder I/O error: {e}"),
            Self::Png(e) => write!(f, "HD-pack builder PNG error: {e}"),
        }
    }
}

impl std::error::Error for BuilderError {}

impl From<std::io::Error> for BuilderError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// The in-emulator HD-pack recorder.
///
/// Built once when the user starts a recording, fed [`Self::observe`] each
/// presented frame, then drained with [`Self::write_pack`].
pub struct HdPackBuilder {
    /// Distinct tiles keyed by the Mesen CRC-32 of their 16 CHR bytes.
    tiles: HashMap<u32, CapturedTile>,
    /// Number of frames observed (informational; surfaced in the manifest).
    frames: u64,
    /// The hi-res scale to declare in `hires.txt`. Recording captures 1x native
    /// tiles; the artist repaints at this scale. Mesen's default is 1, but the
    /// builder records the configured factor so the manifest is ready to grow.
    scale: u32,
}

impl Default for HdPackBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl HdPackBuilder {
    /// Create an empty recorder with the default declared scale of 1.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tiles: HashMap::new(),
            frames: 0,
            scale: 1,
        }
    }

    /// Number of distinct tiles captured so far.
    #[must_use]
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Number of frames observed so far.
    #[must_use]
    pub const fn frame_count(&self) -> u64 {
        self.frames
    }

    /// Observe one presented frame. `framebuffer` is the stock 256x240 RGBA8 NES
    /// image (NOT the HD-composited output — we want the native pixels). `tiles`
    /// is the per-pixel [`HdTileSource`] telemetry (parallel to `framebuffer`),
    /// and `chr_peek` reads a CHR byte from the per-frame `$0000..=$1FFF` snapshot
    /// (exactly the closure [`crate::hdpack::HdCompositor::composite`] is fed).
    ///
    /// For each visible 8x8 cell whose dominant pixel references a real tile, the
    /// tile's CRC-32 key is computed and — if not already captured — its native
    /// 8x8 RGBA pixels are lifted out of `framebuffer` and stored. The first
    /// observation of a given tile wins; later identical-CRC tiles are ignored
    /// (a tile drawn under a different palette hashes the same, mirroring the
    /// loader's flip-agnostic, palette-agnostic CRC key).
    pub fn observe(
        &mut self,
        framebuffer: &[u8],
        tiles: &[HdTileSource],
        mut chr_peek: impl FnMut(u16) -> u8,
    ) {
        // Guard against a short/missing buffer (e.g. a black no-ROM frame).
        let want = NES_W as usize * NES_H as usize;
        if framebuffer.len() < want * 4 || tiles.len() < want {
            return;
        }
        self.frames = self.frames.wrapping_add(1);

        let mut chr = [0u8; 16];
        for cell_y in 0..ROWS {
            for cell_x in 0..COLS {
                // The compositor keys a cell on its top-left pixel; match that.
                let cell_px = (cell_y * TILE) * NES_W as usize + cell_x * TILE;
                let rec = tiles[cell_px];
                if rec.chr_addr == HD_TILE_NONE {
                    continue;
                }
                // Mesen CRC key over the tile's 16 *un-flipped* CHR bytes.
                let base = rec.chr_addr & 0x1FF0;
                for (i, b) in chr.iter_mut().enumerate() {
                    *b = chr_peek(base + u16::try_from(i).unwrap_or(0));
                }
                let key = crc32(&chr);
                if self.tiles.contains_key(&key) {
                    continue;
                }
                let index = self.tiles.len();
                let rgba = lift_cell(framebuffer, cell_x, cell_y);
                self.tiles.insert(key, CapturedTile { rgba, index, chr });
            }
        }
    }

    /// Render the captured tiles into a packed RGBA8 sheet, returning
    /// `(width, height, rgba)`. Tiles are laid out in insertion order across
    /// [`SHEET_COLS`] columns. Empty slots (none, since every index is filled)
    /// would be transparent. Each tile occupies an 8x8 cell at native scale.
    #[must_use]
    fn render_sheet(&self) -> (u32, u32, Vec<u8>) {
        let n = self.tiles.len();
        let rows = n.div_ceil(SHEET_COLS).max(1);
        let w = SHEET_COLS * TILE;
        let h = rows * TILE;
        let mut sheet = vec![0u8; w * h * 4];
        for tile in self.tiles.values() {
            let col = tile.index % SHEET_COLS;
            let row = tile.index / SHEET_COLS;
            let ox = col * TILE;
            let oy = row * TILE;
            for ty in 0..TILE {
                for tx in 0..TILE {
                    let src = (ty * TILE + tx) * 4;
                    let dst = ((oy + ty) * w + (ox + tx)) * 4;
                    sheet[dst..dst + 4].copy_from_slice(&tile.rgba[src..src + 4]);
                }
            }
        }
        (
            u32::try_from(w).unwrap_or(0),
            u32::try_from(h).unwrap_or(0),
            sheet,
        )
    }

    /// Build the `hires.txt` manifest text for the captured tiles in the **real
    /// Mesen `<ver>106` format** — one `<tile>` rule per distinct tile, mapping
    /// the tile's 16 CHR bytes (`tileData`, 32 hex chars — Mesen's match key) to
    /// its `(x, y)` slot in `tiles.png`. The grammar this emits is exactly what
    /// [`crate::hdpack`]'s loader reads and what real Mesen tooling consumes:
    /// `<tile>bitmapIndex,tileData,palette,x,y,brightness,defaultTile`.
    ///
    /// `bitmapIndex` is `0` (the single `<img>tiles.png` declaration). `palette`
    /// is an all-zero placeholder — `RustyNES`'s CRC-of-CHR match key is
    /// palette-agnostic (a documented subset of Mesen's palette-discriminated
    /// keying), so the field is emitted for format compliance only. `brightness`
    /// is `1` (full) and `defaultTile` is `N`.
    #[must_use]
    fn manifest_text(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        // `<ver>106` is the real Mesen HD-pack format this builder + loader speak.
        out.push_str("<ver>106\n");
        let _ = writeln!(out, "<scale>{}", self.scale);
        // A single tile sheet, referenced as bitmap index 0 by the rules below.
        out.push_str("<img>tiles.png\n");
        // A friendly provenance comment (ignored by the parser, which skips
        // non-`<...>` lines). RustyNES emits the observed frame count.
        let _ = writeln!(
            out,
            "// RustyNES HD-Pack Builder: {} frame(s) observed",
            self.frames
        );
        // Emit tiles in insertion order for a stable, diff-friendly manifest.
        let mut ordered: Vec<(&u32, &CapturedTile)> = self.tiles.iter().collect();
        ordered.sort_by_key(|(_, t)| t.index);
        // Reused across tiles so the 32-char hex buffer is allocated once, not
        // once per tile.
        let mut tile_data = String::with_capacity(32);
        for (_key, tile) in ordered {
            let col = tile.index % SHEET_COLS;
            let row = tile.index / SHEET_COLS;
            let x = col * TILE;
            let y = row * TILE;
            // tileData = the 16 CHR bytes as 32 upper-case hex chars (Mesen form).
            tile_data.clear();
            for b in tile.chr {
                let _ = write!(tile_data, "{b:02X}");
            }
            // bitmapIndex,tileData,palette,x,y,brightness,defaultTile
            let _ = writeln!(out, "<tile>0,{tile_data},00000000,{x},{y},1,N");
        }
        out
    }

    /// Write the captured pack to `dir`: a `tiles.png` sheet + a `hires.txt`
    /// manifest. The directory is created if absent.
    ///
    /// # Errors
    ///
    /// [`BuilderError::Empty`] if no tiles were captured, [`BuilderError::Io`] on
    /// a filesystem error, or [`BuilderError::Png`] if the sheet fails to encode.
    pub fn write_pack(&self, dir: &Path) -> Result<usize, BuilderError> {
        if self.tiles.is_empty() {
            return Err(BuilderError::Empty);
        }
        std::fs::create_dir_all(dir)?;
        let (w, h, rgba) = self.render_sheet();
        let png_bytes = encode_png(w, h, &rgba)?;
        std::fs::write(dir.join("tiles.png"), png_bytes)?;
        std::fs::write(dir.join("hires.txt"), self.manifest_text())?;
        Ok(self.tiles.len())
    }
}

/// Lift the native 8x8 RGBA pixels of the `(cell_x, cell_y)` visible cell out of
/// the 256x240 framebuffer.
fn lift_cell(framebuffer: &[u8], cell_x: usize, cell_y: usize) -> [u8; TILE * TILE * 4] {
    let mut out = [0u8; TILE * TILE * 4];
    let ox = cell_x * TILE;
    let oy = cell_y * TILE;
    for ty in 0..TILE {
        for tx in 0..TILE {
            let sx = ox + tx;
            let sy = oy + ty;
            let src = (sy * NES_W as usize + sx) * 4;
            let dst = (ty * TILE + tx) * 4;
            if src + 4 <= framebuffer.len() {
                out[dst..dst + 4].copy_from_slice(&framebuffer[src..src + 4]);
            }
        }
    }
    out
}

/// Encode an RGBA8 image to PNG bytes using the same `png` crate the loader
/// decodes replacement images with.
fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, BuilderError> {
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buf, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| BuilderError::Png(e.to_string()))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| BuilderError::Png(e.to_string()))?;
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a framebuffer + tile-source where cell (cx, cy) shows tile `chr`
    /// with a distinctive solid colour, and a CHR snapshot where each tile index
    /// has unique bytes (so distinct tiles get distinct CRCs).
    fn synth_frame(cells: &[(usize, usize, u16, [u8; 4])]) -> (Vec<u8>, Vec<HdTileSource>) {
        let n = NES_W as usize * NES_H as usize;
        let mut fb = vec![0u8; n * 4];
        let mut ts = vec![HdTileSource::default(); n];
        for s in &mut ts {
            s.chr_addr = HD_TILE_NONE;
        }
        for &(cx, cy, chr, colour) in cells {
            for ty in 0..TILE {
                for tx in 0..TILE {
                    let px = (cy * TILE + ty) * NES_W as usize + (cx * TILE + tx);
                    fb[px * 4..px * 4 + 4].copy_from_slice(&colour);
                    ts[px].chr_addr = chr;
                }
            }
        }
        (fb, ts)
    }

    /// CHR peek where each 16-byte tile slot holds bytes derived from its index,
    /// so two different `chr_addr`s hash to different CRC keys.
    fn chr_for(addr: u16) -> u8 {
        let tile = ((addr >> 4) & 0xFF) as u8;
        let byte = (addr & 0xF) as u8;
        tile.wrapping_mul(31).wrapping_add(byte)
    }

    #[test]
    fn captures_distinct_tiles_once() {
        let mut b = HdPackBuilder::new();
        let (fb, ts) = synth_frame(&[
            (0, 0, 0x0000, [255, 0, 0, 255]),
            (1, 0, 0x0010, [0, 255, 0, 255]),
            (2, 0, 0x0000, [0, 0, 255, 255]), // same chr as cell 0 -> deduped
        ]);
        b.observe(&fb, &ts, chr_for);
        assert_eq!(
            b.tile_count(),
            2,
            "two distinct tiles, third deduped by CRC"
        );
        assert_eq!(b.frame_count(), 1);

        // A second identical frame must not grow the set.
        b.observe(&fb, &ts, chr_for);
        assert_eq!(b.tile_count(), 2);
        assert_eq!(b.frame_count(), 2);
    }

    #[test]
    fn empty_recording_errors_on_write() {
        let b = HdPackBuilder::new();
        let dir = std::env::temp_dir().join("rustynes-hdpack-builder-empty-test");
        assert!(matches!(b.write_pack(&dir), Err(BuilderError::Empty)));
    }

    #[test]
    fn manifest_has_one_tile_rule_per_capture() {
        let mut b = HdPackBuilder::new();
        let (fb, ts) = synth_frame(&[
            (0, 0, 0x0000, [255, 0, 0, 255]),
            (1, 0, 0x0010, [0, 255, 0, 255]),
        ]);
        b.observe(&fb, &ts, chr_for);
        let text = b.manifest_text();
        let tile_lines: Vec<&str> = text.lines().filter(|l| l.starts_with("<tile>")).collect();
        assert_eq!(tile_lines.len(), 2);
        assert!(text.contains("<ver>106"));
        assert!(text.contains("<img>tiles.png"));
        // Each tile rule is the real Mesen form:
        // `<tile>bitmapIndex,tileData(32 hex),palette,x,y,brightness,defaultTile`.
        for l in &tile_lines {
            let fields: Vec<&str> = l.trim_start_matches("<tile>").split(',').collect();
            assert_eq!(fields.len(), 7, "real Mesen <tile> has 7 fields: {l}");
            assert_eq!(fields[0], "0", "bitmap index 0 (single tiles.png)");
            assert_eq!(fields[1].len(), 32, "tileData is 16 CHR bytes = 32 hex");
            assert!(
                fields[1].chars().all(|c| c.is_ascii_hexdigit()),
                "tileData hex"
            );
            assert_eq!(fields[5], "1", "brightness");
            assert_eq!(fields[6], "N", "defaultTile");
        }
    }

    #[test]
    fn round_trip_pack_loads_back() {
        // Capture a tiny pack, write it, and confirm the loader parses it.
        let mut b = HdPackBuilder::new();
        let (fb, ts) = synth_frame(&[
            (0, 0, 0x0000, [10, 20, 30, 255]),
            (1, 0, 0x0010, [40, 50, 60, 255]),
            (0, 1, 0x0020, [70, 80, 90, 255]),
        ]);
        b.observe(&fb, &ts, chr_for);
        assert_eq!(b.tile_count(), 3);

        let dir = std::env::temp_dir().join(format!(
            "rustynes-hdpack-builder-roundtrip-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let written = b.write_pack(&dir).expect("write pack");
        assert_eq!(written, 3);
        assert!(dir.join("tiles.png").exists());
        assert!(dir.join("hires.txt").exists());

        // The loader must accept the emitted pack and key all three tiles.
        let pack = crate::hdpack::HdPack::load(&dir).expect("loader accepts builder output");
        let comp = crate::hdpack::HdCompositor::new(pack);
        // The pack declares scale 1.
        assert_eq!(comp.dimensions(), (NES_W, NES_H));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn short_framebuffer_is_ignored() {
        let mut b = HdPackBuilder::new();
        // A too-short buffer (e.g. a black no-ROM present) must be a no-op.
        b.observe(&[0u8; 16], &[HdTileSource::default(); 4], |_| 0);
        assert_eq!(b.tile_count(), 0);
        assert_eq!(b.frame_count(), 0);
    }
}

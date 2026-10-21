//! Observation layers: what an agent can see of the machine.
//!
//! `T-AI-PLAY` (v4.6.0) builds its layered observations on these, so each is
//! a pure function of the machine's current state that never advances it:
//!
//! - **the frame**, as a PNG of the 256x240 RGBA framebuffer;
//! - **the nametables as tile text**, one hex byte per tile, read through the
//!   PPU bus with no side effect (so the mapper's mirroring and any
//!   cartridge nametable RAM are honoured exactly as the PPU would see them);
//! - **OAM**, the 64 sprites decoded into position, tile, palette and flips.
//!
//! A text layer is chosen over an image for the nametable and OAM because a
//! language model reads "tile `$24` at row 12, column 5" exactly, where an
//! image of the same data must first be perceived; the frame image is there
//! for what text cannot carry.

use std::fmt::Write as _;

use rustynes_core::Nes;

/// Framebuffer width in pixels.
pub const WIDTH: u32 = 256;
/// Framebuffer height in pixels.
pub const HEIGHT: u32 = 240;

/// Encode an RGBA8 framebuffer as a PNG.
///
/// # Errors
///
/// A buffer of the wrong size, or an encoder failure (both are bugs, not
/// agent errors, and are reported rather than panicking).
pub fn png_rgba(rgba: &[u8]) -> Result<Vec<u8>, String> {
    let expected = (WIDTH * HEIGHT * 4) as usize;
    if rgba.len() != expected {
        return Err(format!(
            "framebuffer is {} bytes, expected {expected}",
            rgba.len()
        ));
    }
    let mut out = Vec::with_capacity(64 * 1024);
    {
        let mut enc = png::Encoder::new(&mut out, WIDTH, HEIGHT);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// One nametable (`index` 0-3 at `$2000 + $400 * index`) as text: 30 rows of
/// 32 tile bytes, then the 8x8 attribute table, then the scroll registers.
#[must_use]
pub fn nametable_text(nes: &mut Nes, index: u8) -> String {
    let base = 0x2000u16 + 0x400 * u16::from(index & 3);
    let mut s = String::with_capacity(4096);
    let _ = writeln!(
        s,
        "nametable {index} (${base:04X}): 30 rows x 32 tiles, hex tile indices"
    );
    s.push_str("    ");
    for col in 0..32 {
        let _ = write!(s, "{col:02} ");
    }
    s.push('\n');
    for row in 0..30u16 {
        let _ = write!(s, "{row:02}: ");
        for col in 0..32u16 {
            let t = nes.ppu_bus_peek(base + row * 32 + col);
            let _ = write!(s, "{t:02X} ");
        }
        s.push('\n');
    }
    s.push_str("attributes (8x8, each byte = four 2x2-tile quadrants' palettes):\n");
    for row in 0..8u16 {
        let _ = write!(s, "{row}: ");
        for col in 0..8u16 {
            let a = nes.ppu_bus_peek(base + 0x3C0 + row * 8 + col);
            let _ = write!(s, "{a:02X} ");
        }
        s.push('\n');
    }
    let p = nes.ppu_snapshot();
    let _ = writeln!(
        s,
        "ppu: ctrl={:02X} mask={:02X} v={:04X} t={:04X} fine_x={} bg_pattern=${:04X} scanline={} dot={}",
        p.ctrl, p.mask, p.v, p.t, p.fine_x, p.bg_pattern_base, p.scanline, p.dot
    );
    s
}

/// OAM as text: one line per sprite, with the off-screen ones (`Y >= $EF`)
/// counted rather than listed, since a typical frame parks most of the 64
/// there and listing them buries the visible ones.
#[must_use]
pub fn oam_text(nes: &Nes) -> String {
    let oam = nes.oam();
    let p = nes.ppu_snapshot();
    let mut s = String::with_capacity(2048);
    let _ = writeln!(
        s,
        "OAM: 64 sprites, size {}, pattern ${:04X}; columns: index y x tile palette priority flip",
        if p.sprite_size_16 { "8x16" } else { "8x8" },
        p.sprite_pattern_base
    );
    let mut hidden = 0;
    for i in 0..64 {
        let [y, tile, attr, x] = [oam[i * 4], oam[i * 4 + 1], oam[i * 4 + 2], oam[i * 4 + 3]];
        if y >= 0xEF {
            hidden += 1;
            continue;
        }
        let flip = match (attr & 0x40 != 0, attr & 0x80 != 0) {
            (false, false) => "-",
            (true, false) => "h",
            (false, true) => "v",
            (true, true) => "hv",
        };
        let prio = if attr & 0x20 != 0 { "behind" } else { "front" };
        let _ = writeln!(
            s,
            "{i:02} y={y:3} x={x:3} tile={tile:02X} pal={} {prio} {flip}",
            attr & 3
        );
    }
    let _ = writeln!(s, "{hidden} sprites off-screen (y >= $EF)");
    s
}

/// The CPU register file as one line.
#[must_use]
pub fn cpu_text(nes: &Nes) -> String {
    let c = nes.cpu_snapshot();
    format!(
        "cpu: pc={:04X} a={:02X} x={:02X} y={:02X} s={:02X} p={:02X} cycles={} jammed={} frame={}",
        c.pc,
        c.a,
        c.x,
        c.y,
        c.s,
        c.p,
        c.cycles,
        c.jammed,
        nes.frame()
    )
}

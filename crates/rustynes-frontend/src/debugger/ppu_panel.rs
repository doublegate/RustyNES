#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::missing_const_for_fn,
    clippy::suboptimal_flops,
    clippy::items_after_statements,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::needless_pass_by_ref_mut
)]
//! PPU panel — registers + nametable viewer + pattern table viewer +
//! palette viewer + scroll-cursor overlay (T-53-003).
//!
//! Read-only. The CHR/nametable buffers are rendered on demand into
//! egui-managed textures; per-frame work is bounded by the number of
//! sub-tabs the user has open.

use egui::ColorImage;
use rustynes_core::Nes;

use crate::emu::DebugPoke;

/// Sub-tab of the PPU panel.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Tab {
    Registers,
    Patterns,
    Nametables,
    Palette,
    /// v1.5.0 "Lens" Workstream A3 — per-scanline state-capture trace.
    Scanline,
}

/// Persistent state of the PPU panel.
pub struct PpuPanelState {
    tab: Tab,
    /// Which nametable to display (0..=3).
    nt_index: u8,
    /// Cached pattern textures (left + right).
    pattern_tex: [Option<egui::TextureHandle>; 2],
    /// Cached nametable texture (one at a time).
    nametable_tex: Option<egui::TextureHandle>,
    /// v1.7.0 "Forge" Workstream A1 — editing-tool writeback state + the
    /// one-shot poke queue. Self-contained so it merges cleanly alongside
    /// other panel work (see [`A1Edit`]).
    a1: A1Edit,
}

/// v1.7.0 "Forge" Workstream A1 — the PPU panel's editing state. Holds the
/// transient hex-entry buffers for the palette / nametable / CHR editors and
/// the queue of writeback edits drained by the debugger each frame. Edits are
/// queued here and applied through the gated post-frame poke path — never
/// directly to the running `Nes` — so determinism and the `emu.write` gate are
/// preserved.
#[derive(Default)]
struct A1Edit {
    /// Master enable for all PPU-panel editors (off by default → read-only).
    enabled: bool,
    /// Pending writeback edits, drained by [`PpuPanelState::take_pokes`].
    pending: Vec<DebugPoke>,
    /// Palette editor: the currently-selected palette index (0..32) + its
    /// pending hex text.
    pal_sel: Option<u8>,
    pal_text: String,
    /// Nametable editor: the selected (col, row) cell within the current NT +
    /// the pending tile/attribute hex text.
    nt_cell: Option<(u8, u8)>,
    nt_tile_text: String,
    nt_attr_text: String,
    /// CHR editor: the selected pattern-table address ($0000-$1FFF) + its
    /// pending hex text.
    chr_addr_text: String,
    chr_val_text: String,
}

impl A1Edit {
    /// Parse a 2-hex-digit byte from a text buffer (lenient: trims `$`/`0x`/`0X`).
    fn parse_byte(s: &str) -> Option<u8> {
        let t = s
            .trim()
            .trim_start_matches('$')
            .trim_start_matches("0x")
            .trim_start_matches("0X");
        u8::from_str_radix(t, 16).ok()
    }

    /// Parse a 16-bit address from a text buffer (lenient: trims `$`/`0x`/`0X`).
    fn parse_addr(s: &str) -> Option<u16> {
        let t = s
            .trim()
            .trim_start_matches('$')
            .trim_start_matches("0x")
            .trim_start_matches("0X");
        u16::from_str_radix(t, 16).ok()
    }
}

impl Default for PpuPanelState {
    fn default() -> Self {
        Self {
            tab: Tab::Registers,
            nt_index: 0,
            pattern_tex: [None, None],
            nametable_tex: None,
            a1: A1Edit::default(),
        }
    }
}

impl PpuPanelState {
    /// v1.7.0 "Forge" Workstream A1 — drain the queued PPU-panel writeback
    /// edits for the debugger to forward to the gated post-frame poke path.
    pub fn take_pokes(&mut self) -> Vec<DebugPoke> {
        core::mem::take(&mut self.a1.pending)
    }
}

/// Render the PPU panel.
pub fn show(ctx: &egui::Context, open: &mut bool, state: &mut PpuPanelState, nes: &mut Nes) {
    let ppu = nes.ppu_snapshot();
    egui::Window::new("PPU")
        .open(open)
        .default_pos([336.0, 64.0])
        .default_size([480.0, 420.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.tab, Tab::Registers, "Registers");
                ui.selectable_value(&mut state.tab, Tab::Patterns, "Patterns");
                ui.selectable_value(&mut state.tab, Tab::Nametables, "Nametables");
                ui.selectable_value(&mut state.tab, Tab::Palette, "Palette");
                ui.selectable_value(&mut state.tab, Tab::Scanline, "Scanline trace");
            });
            // v1.7.0 "Forge" Workstream A1 — editing master toggle. Off by
            // default → the panel is read-only (byte-identical with no edits
            // queued). On → the Palette / Nametables / Patterns tabs expose
            // their writeback editors, which queue gated post-frame pokes.
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.a1.enabled, "Edit (writeback)");
                if state.a1.enabled {
                    ui.weak("edits apply after the next frame via the gated poke path");
                }
            });
            ui.separator();
            match state.tab {
                Tab::Registers => regs_tab(ui, &ppu),
                Tab::Patterns => patterns_tab(ui, ctx, state, nes),
                Tab::Nametables => nametables_tab(ui, ctx, state, nes, &ppu),
                Tab::Palette => palette_tab(ui, ctx, state, nes),
                Tab::Scanline => scanline_tab(ui, nes),
            }
        });
}

fn regs_tab(ui: &mut egui::Ui, ppu: &rustynes_core::PpuDebugView) {
    ui.monospace(format!(
        "scanline={:>4}  dot={:>3}  frame={}",
        ppu.scanline, ppu.dot, ppu.frame
    ));
    ui.monospace(format!(
        "CTRL ${:02X}   MASK ${:02X}   STATUS ${:02X}   OAMADDR ${:02X}",
        ppu.ctrl, ppu.mask, ppu.status, ppu.oam_addr
    ));
    ui.monospace(format!(
        "v={:04X}  t={:04X}  x={}  w={}",
        ppu.v, ppu.t, ppu.fine_x, ppu.w_toggle
    ));
    ui.monospace(format!(
        "BG ${:04X}   SPR ${:04X}   sprite_size={}",
        ppu.bg_pattern_base,
        ppu.sprite_pattern_base,
        if ppu.sprite_size_16 { "8x16" } else { "8x8" }
    ));
    if ppu.nmi_line {
        ui.colored_label(egui::Color32::LIGHT_GREEN, "NMI line: asserted");
    } else {
        ui.colored_label(egui::Color32::DARK_GRAY, "NMI line: low");
    }
}

fn patterns_tab(ui: &mut egui::Ui, ctx: &egui::Context, state: &mut PpuPanelState, nes: &mut Nes) {
    ui.label("Pattern tables — left $0000, right $1000");
    ui.horizontal(|ui| {
        for table in 0..2u8 {
            let rgba = nes.pattern_table_rgba(table);
            let image = ColorImage::from_rgba_unmultiplied([128, 128], &rgba);
            let handle = state.pattern_tex[table as usize].get_or_insert_with(|| {
                ctx.load_texture(
                    format!("pt-{table}"),
                    image.clone(),
                    egui::TextureOptions::NEAREST,
                )
            });
            handle.set(image, egui::TextureOptions::NEAREST);
            ui.image((handle.id(), egui::vec2(192.0, 192.0)));
        }
    });
    // v1.5.0 "Lens" Workstream A3 — CHR -> PNG export (native only: needs the
    // `png` encoder + the `rfd` save dialog, both native-only deps). Exports the
    // 256x128 combined pattern dump (left $0000 + right $1000 side by side).
    #[cfg(not(target_arch = "wasm32"))]
    if ui.button("Export CHR to PNG...").clicked() {
        export_chr_png(nes);
    }
    // v1.7.0 "Forge" Workstream A1 — CHR byte editor. Writes one CHR-RAM byte
    // at $0000-$1FFF through the gated poke (a no-op on CHR-ROM carts, where
    // the mapper drops the write). Only active while editing.
    if state.a1.enabled {
        chr_editor(ui, &mut state.a1);
    }
}

/// v1.7.0 "Forge" Workstream A1 — the CHR byte editor: an address + value hex
/// pair that queues a gated `$0000-$1FFF` PPU-bus writeback.
fn chr_editor(ui: &mut egui::Ui, a1: &mut A1Edit) {
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("CHR addr $");
        ui.add(
            egui::TextEdit::singleline(&mut a1.chr_addr_text)
                .desired_width(48.0)
                .hint_text("0000"),
        );
        ui.label("= $");
        ui.add(
            egui::TextEdit::singleline(&mut a1.chr_val_text)
                .desired_width(36.0)
                .hint_text("00"),
        );
        if ui.button("Poke").clicked()
            && let (Some(addr), Some(val)) = (
                A1Edit::parse_addr(&a1.chr_addr_text),
                A1Edit::parse_byte(&a1.chr_val_text),
            )
            && addr < 0x2000
        {
            a1.pending.push(DebugPoke::PpuBus { addr, value: val });
        }
    });
    ui.weak("(CHR-ROM carts ignore the write; CHR-RAM carts accept it)");
}

/// v1.5.0 A3 — export the two pattern tables as one 256x128 RGBA PNG. Native
/// only. Display-only: reads the same `pattern_table_rgba` the viewer uses.
#[cfg(not(target_arch = "wasm32"))]
fn export_chr_png(nes: &mut Nes) {
    // Compose the two 128x128 tables side by side into a 256x128 RGBA buffer.
    let left = nes.pattern_table_rgba(0);
    let right = nes.pattern_table_rgba(1);
    let (w, h) = (256usize, 128usize);
    let mut combined = vec![0u8; w * h * 4];
    for y in 0..h {
        let dst = y * w * 4;
        let src = y * 128 * 4;
        combined[dst..dst + 128 * 4].copy_from_slice(&left[src..src + 128 * 4]);
        combined[dst + 128 * 4..dst + 256 * 4].copy_from_slice(&right[src..src + 128 * 4]);
    }
    let Some(path) = rfd::FileDialog::new()
        .add_filter("PNG image", &["png"])
        .set_file_name("rustynes-chr.png")
        .save_file()
    else {
        return;
    };
    let Ok(file) = std::fs::File::create(&path) else {
        return;
    };
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    if let Ok(mut writer) = encoder.write_header() {
        let _ = writer.write_image_data(&combined);
    }
}

/// v1.5.0 "Lens" Workstream A3 — per-scanline state-capture trace.
///
/// Derived entirely from the `debug-hooks` event log (`Nes::events`): the
/// scroll- and rendering-affecting PPU register writes ($2000 PPUCTRL, $2005
/// PPUSCROLL, $2006 PPUADDR, $2001 PPUMASK) are grouped by the scanline they
/// occurred on, so a user can see mid-frame scroll splits (e.g. status-bar
/// raster effects) without any new core hook. Output-only — the event log is
/// reset per frame and never perturbs emulation.
fn scanline_tab(ui: &mut egui::Ui, nes: &mut Nes) {
    ui.horizontal(|ui| {
        let mut on = nes.event_logging();
        if ui.checkbox(&mut on, "Record").changed() {
            nes.set_event_logging(on);
        }
        ui.weak("per-scanline PPU register-write trace ($2000/$2001/$2005/$2006)");
    });
    if !nes.event_logging() {
        ui.weak("(enable Record, then run/step a frame)");
        return;
    }
    // (scanline, reg_name, addr, value), filtered to the scroll/render writes.
    let mut rows: Vec<(i16, &'static str, u16, u8)> = Vec::new();
    for e in nes.events() {
        let name = match e.addr & 0x2007 {
            0x2000 if (0x2000..=0x3FFF).contains(&e.addr) => "PPUCTRL",
            0x2001 if (0x2000..=0x3FFF).contains(&e.addr) => "PPUMASK",
            0x2005 if (0x2000..=0x3FFF).contains(&e.addr) => "PPUSCROLL",
            0x2006 if (0x2000..=0x3FFF).contains(&e.addr) => "PPUADDR",
            _ => continue,
        };
        // Only writes shape the per-scanline render state; skip reads.
        if e.kind.is_read() {
            continue;
        }
        rows.push((e.scanline, name, e.addr, e.value));
    }
    ui.label(format!("{} scroll/render writes this frame", rows.len()));
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("scanline_trace")
                .striped(true)
                .num_columns(3)
                .show(ui, |ui| {
                    ui.strong("Scanline");
                    ui.strong("Register");
                    ui.strong("Value");
                    ui.end_row();
                    for (sl, name, addr, val) in rows {
                        ui.monospace(format!("{sl:>4}"));
                        ui.monospace(format!("{name} (${addr:04X})"));
                        ui.monospace(format!("${val:02X}"));
                        ui.end_row();
                    }
                });
        });
}

fn nametables_tab(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    state: &mut PpuPanelState,
    nes: &mut Nes,
    ppu: &rustynes_core::PpuDebugView,
) {
    ui.horizontal(|ui| {
        for nt in 0..4u8 {
            ui.selectable_value(&mut state.nt_index, nt, format!("NT{nt}"));
        }
    });
    let rgba = nes.nametable_rgba(state.nt_index);
    let image = ColorImage::from_rgba_unmultiplied([256, 240], &rgba);
    let handle = state.nametable_tex.get_or_insert_with(|| {
        ctx.load_texture(
            format!("nt-{}", state.nt_index),
            image.clone(),
            egui::TextureOptions::NEAREST,
        )
    });
    handle.set(image, egui::TextureOptions::NEAREST);
    let response = ui.image((handle.id(), egui::vec2(384.0, 360.0)));

    // v1.7.0 "Forge" Workstream A1 — nametable tile/attribute editor. Click a
    // cell (32x30 grid) to select it, then queue a gated writeback for the
    // tile byte (NT) and/or the 2-bit attribute (packed in the attribute
    // table). Only active while editing.
    if state.a1.enabled {
        let rect = response.rect;
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let col = (((pos.x - rect.min.x) / rect.width()) * 32.0) as i32;
            let row = (((pos.y - rect.min.y) / rect.height()) * 30.0) as i32;
            if (0..32).contains(&col) && (0..30).contains(&row) {
                let (c, r) = (col as u8, row as u8);
                state.a1.nt_cell = Some((c, r));
                // Seed the edit fields with the current tile + attribute.
                let nt_base = 0x2000u16 + u16::from(state.nt_index) * 0x400;
                let tile_addr = nt_base + u16::from(r) * 32 + u16::from(c);
                let tile = nes.ppu_bus_peek(tile_addr);
                state.a1.nt_tile_text = format!("{tile:02X}");
                let (attr_byte, shift) = nt_attr_addr_shift(nt_base, c, r);
                let attr = (nes.ppu_bus_peek(attr_byte) >> shift) & 0x03;
                state.a1.nt_attr_text = format!("{attr}");
            }
        }
        nametable_editor(ui, &mut state.a1, nes, state.nt_index);
    }

    // Overlay scroll cursor: extract coarse_x / coarse_y / fine_x / fine_y
    // from loopy `v` and the fine X register.
    let coarse_x = (ppu.v & 0x1F) as f32;
    let coarse_y = ((ppu.v >> 5) & 0x1F) as f32;
    let fine_y = (ppu.v >> 12) as f32;
    let cur_nt = ((ppu.v >> 10) & 0x03) as u8;
    if cur_nt == state.nt_index {
        let rect = response.rect;
        let scale_x = rect.width() / 256.0;
        let scale_y = rect.height() / 240.0;
        let cursor_x = rect.min.x + (coarse_x * 8.0 + f32::from(ppu.fine_x)) * scale_x;
        let cursor_y = rect.min.y + (coarse_y * 8.0 + fine_y) * scale_y;
        let painter = ui.painter_at(rect);
        painter.line_segment(
            [
                egui::pos2(cursor_x, rect.min.y),
                egui::pos2(cursor_x, rect.max.y),
            ],
            egui::Stroke::new(1.0, egui::Color32::YELLOW),
        );
        painter.line_segment(
            [
                egui::pos2(rect.min.x, cursor_y),
                egui::pos2(rect.max.x, cursor_y),
            ],
            egui::Stroke::new(1.0, egui::Color32::YELLOW),
        );
    }
}

/// v1.7.0 "Forge" Workstream A1 — for nametable base `nt_base` and a (col,row)
/// cell, return the attribute-table byte address and the 2-bit shift within it.
/// The attribute table starts at `nt_base + $3C0`; one byte covers a 4x4-tile
/// block, with 2 bits per 2x2 quadrant.
fn nt_attr_addr_shift(nt_base: u16, col: u8, row: u8) -> (u16, u8) {
    let attr_addr = nt_base + 0x3C0 + (u16::from(row) / 4) * 8 + (u16::from(col) / 4);
    let quad = ((row % 4) / 2) * 2 + ((col % 4) / 2);
    (attr_addr, quad * 2)
}

/// v1.7.0 "Forge" Workstream A1 — the nametable cell editor (tile byte +
/// 2-bit attribute). Queues gated writebacks; the attribute write reads the
/// current attribute byte, replaces the selected quadrant's 2 bits, and writes
/// the merged byte back.
fn nametable_editor(ui: &mut egui::Ui, a1: &mut A1Edit, nes: &mut Nes, nt_index: u8) {
    ui.separator();
    let Some((col, row)) = a1.nt_cell else {
        ui.weak("Click a nametable cell above to edit its tile / attribute.");
        return;
    };
    let nt_base = 0x2000u16 + u16::from(nt_index) * 0x400;
    ui.horizontal(|ui| {
        ui.monospace(format!("Cell ({col},{row})"));
        ui.label("tile:");
        ui.add(
            egui::TextEdit::singleline(&mut a1.nt_tile_text)
                .desired_width(36.0)
                .hint_text("00"),
        );
        ui.label("attr:");
        ui.add(
            egui::TextEdit::singleline(&mut a1.nt_attr_text)
                .desired_width(24.0)
                .hint_text("0"),
        );
        if ui.button("Apply").clicked() {
            if let Some(tile) = A1Edit::parse_byte(&a1.nt_tile_text) {
                let addr = nt_base + u16::from(row) * 32 + u16::from(col);
                a1.pending.push(DebugPoke::PpuBus { addr, value: tile });
            }
            if let Some(attr) = A1Edit::parse_byte(&a1.nt_attr_text) {
                let (attr_addr, shift) = nt_attr_addr_shift(nt_base, col, row);
                // Read-modify-write the 2-bit quadrant within the byte. Reading
                // the live byte here is side-effect-free (`ppu_bus_peek`); the
                // merged byte is queued for the gated poke.
                let cur = nes.ppu_bus_peek(attr_addr);
                let merged = (cur & !(0x03 << shift)) | ((attr & 0x03) << shift);
                a1.pending.push(DebugPoke::PpuBus {
                    addr: attr_addr,
                    value: merged,
                });
            }
        }
    });
}

fn palette_tab(ui: &mut egui::Ui, _ctx: &egui::Context, state: &mut PpuPanelState, nes: &Nes) {
    let pal = nes.palette_ram();
    let editing = state.a1.enabled;
    ui.label("Background palette");
    for row in 0..2u8 {
        ui.horizontal(|ui| {
            for pi in 0..4u8 {
                let idx = (row * 16 + pi * 4) as usize;
                draw_palette_strip(ui, &mut state.a1, &pal, idx, editing);
            }
        });
        // Second row of BG palettes is offset 16..32 (sprite palettes).
        if row == 0 {
            ui.add_space(4.0);
            ui.label("Sprite palette");
        }
    }
    if editing {
        palette_editor(ui, &mut state.a1, &pal);
    }
}

/// v1.7.0 "Forge" Workstream A1 — the palette-entry editor. Shows the selected
/// index and a hex field; "Apply" queues a gated `$3F00+idx` writeback (the
/// 6-bit palette mask + sprite-zero mirroring are applied in the core poke).
fn palette_editor(ui: &mut egui::Ui, a1: &mut A1Edit, pal: &[u8; 32]) {
    ui.separator();
    let Some(sel) = a1.pal_sel else {
        ui.weak("Click a palette swatch above to edit it.");
        return;
    };
    ui.horizontal(|ui| {
        ui.monospace(format!(
            "Palette ${sel:02X} (currently ${:02X})",
            pal[sel as usize] & 0x3F
        ));
        ui.label("new:");
        ui.add(
            egui::TextEdit::singleline(&mut a1.pal_text)
                .desired_width(40.0)
                .hint_text("3F"),
        );
        if ui.button("Apply").clicked()
            && let Some(v) = A1Edit::parse_byte(&a1.pal_text)
        {
            a1.pending.push(DebugPoke::PpuBus {
                addr: 0x3F00 | u16::from(sel),
                value: v & 0x3F,
            });
        }
    });
}

fn draw_palette_strip(
    ui: &mut egui::Ui,
    a1: &mut A1Edit,
    pal: &[u8; 32],
    base: usize,
    editing: bool,
) {
    ui.horizontal(|ui| {
        for j in 0..4 {
            let pal_idx = (base + j) as u8;
            let idx = pal[base + j] & 0x3F;
            let [r, g, b, _] = rustynes_core::rustynes_ppu::nes_color_to_rgba(idx);
            let color = egui::Color32::from_rgb(r, g, b);
            let sense = if editing {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            };
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), sense);
            ui.painter().rect_filled(rect, 2.0, color);
            // Highlight the selected swatch while editing.
            if editing && a1.pal_sel == Some(pal_idx) {
                ui.painter().rect_stroke(
                    rect,
                    2.0,
                    egui::Stroke::new(2.0, egui::Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("{idx:02X}"),
                egui::FontId::monospace(9.0),
                egui::Color32::from_black_alpha(220),
            );
            if editing && resp.clicked() {
                a1.pal_sel = Some(pal_idx);
                a1.pal_text = format!("{idx:02X}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_byte_accepts_hex_prefixes() {
        assert_eq!(A1Edit::parse_byte("$3F"), Some(0x3F));
        assert_eq!(A1Edit::parse_byte("0x3f"), Some(0x3F));
        assert_eq!(A1Edit::parse_byte("0X3F"), Some(0x3F));
        assert_eq!(A1Edit::parse_byte("3f"), Some(0x3F));
    }

    #[test]
    fn parse_addr_accepts_hex_prefixes() {
        assert_eq!(A1Edit::parse_addr("$2006"), Some(0x2006));
        assert_eq!(A1Edit::parse_addr("0x2006"), Some(0x2006));
        assert_eq!(A1Edit::parse_addr("0X2006"), Some(0x2006));
    }
}

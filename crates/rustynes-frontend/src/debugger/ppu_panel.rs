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
}

impl Default for PpuPanelState {
    fn default() -> Self {
        Self {
            tab: Tab::Registers,
            nt_index: 0,
            pattern_tex: [None, None],
            nametable_tex: None,
        }
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
            ui.separator();
            match state.tab {
                Tab::Registers => regs_tab(ui, &ppu),
                Tab::Patterns => patterns_tab(ui, ctx, state, nes),
                Tab::Nametables => nametables_tab(ui, ctx, state, nes, &ppu),
                Tab::Palette => palette_tab(ui, ctx, nes),
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

fn palette_tab(ui: &mut egui::Ui, _ctx: &egui::Context, nes: &Nes) {
    let pal = nes.palette_ram();
    ui.label("Background palette");
    for row in 0..2u8 {
        ui.horizontal(|ui| {
            for pi in 0..4u8 {
                let idx = (row * 16 + pi * 4) as usize;
                draw_palette_strip(ui, &pal, idx);
            }
        });
        // Second row of BG palettes is offset 16..32 (sprite palettes).
        if row == 0 {
            ui.add_space(4.0);
            ui.label("Sprite palette");
        }
    }
}

fn draw_palette_strip(ui: &mut egui::Ui, pal: &[u8; 32], base: usize) {
    ui.horizontal(|ui| {
        for j in 0..4 {
            let idx = pal[base + j] & 0x3F;
            let [r, g, b, _] = rustynes_core::rustynes_ppu::nes_color_to_rgba(idx);
            let color = egui::Color32::from_rgb(r, g, b);
            let (rect, _resp) =
                ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, color);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("{idx:02X}"),
                egui::FontId::monospace(9.0),
                egui::Color32::from_black_alpha(220),
            );
        }
    });
}

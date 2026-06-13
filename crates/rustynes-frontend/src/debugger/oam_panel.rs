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
//! OAM panel — sprite list + visual grid (T-53-004).

use egui::ColorImage;
use rustynes_core::Nes;

/// Persistent state of the OAM panel.
#[derive(Default)]
pub struct OamPanelState {
    /// Cached visual texture.
    visual_tex: Option<egui::TextureHandle>,
}

pub fn show(ctx: &egui::Context, open: &mut bool, state: &mut OamPanelState, nes: &mut Nes) {
    let oam = nes.oam();
    let ppu = nes.ppu_snapshot();
    egui::Window::new("OAM")
        .open(open)
        .default_pos([16.0, 480.0])
        .default_size([520.0, 460.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.label(format!(
                "{} sprites — {}",
                64,
                if ppu.sprite_size_16 { "8x16" } else { "8x8" }
            ));
            ui.separator();
            // Sprite list (scrollable).
            egui::ScrollArea::vertical()
                .id_salt("oam-list")
                .max_height(240.0)
                .show(ui, |ui| {
                    for i in 0..64usize {
                        let off = i * 4;
                        let y = oam[off];
                        let tile = oam[off + 1];
                        let attr = oam[off + 2];
                        let x = oam[off + 3];
                        let palette = attr & 0x03;
                        let priority = if attr & 0x20 != 0 { "bg" } else { "fg" };
                        let flip = match attr & 0xC0 {
                            0x40 => "h",
                            0x80 => "v",
                            0xC0 => "hv",
                            _ => "-",
                        };
                        ui.monospace(format!(
                            "#{i:02}  x={x:3} y={y:3}  tile=${tile:02X}  pal={palette}  pri={priority}  flip={flip}"
                        ));
                    }
                });
            ui.separator();
            // Visual: render the 64 sprites onto a 8x8 grid of 16x16 cells
            // (one tile each — we don't fetch the full 8x16 in this view).
            let rgba = render_sprite_grid(nes, &oam, ppu.sprite_pattern_base);
            let image = ColorImage::from_rgba_unmultiplied([128, 128], &rgba);
            let handle = state.visual_tex.get_or_insert_with(|| {
                ctx.load_texture("oam-grid", image.clone(), egui::TextureOptions::NEAREST)
            });
            handle.set(image, egui::TextureOptions::NEAREST);
            ui.image((handle.id(), egui::vec2(256.0, 256.0)));
        });
}

fn render_sprite_grid(nes: &mut Nes, oam: &[u8; 256], spr_base: u16) -> Vec<u8> {
    // 8x8 sprite tiles laid out 8 across × 8 down → 64 tiles → 64x64 px,
    // upscaled into a 128x128 buffer (2x nearest).
    let mut out = vec![0u8; 128 * 128 * 4];
    for i in 0..64usize {
        let tile = oam[i * 4 + 1];
        let attr = oam[i * 4 + 2];
        let palette = (attr & 0x03) + 4; // sprite palette
        let grid_x = (i % 8) as u16;
        let grid_y = (i / 8) as u16;
        for row in 0..8u16 {
            let lo = nes.ppu_bus_peek(spr_base + u16::from(tile) * 16 + row);
            let hi = nes.ppu_bus_peek(spr_base + u16::from(tile) * 16 + row + 8);
            for col in 0..8u16 {
                let bit = 7 - col;
                let p = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
                let final_idx = if p == 0 {
                    nes.ppu_bus_peek(0x3F00)
                } else {
                    nes.ppu_bus_peek(0x3F00 + u16::from(palette) * 4 + u16::from(p))
                };
                let rgba = rustynes_core::rustynes_ppu::nes_color_to_rgba(final_idx & 0x3F);
                // 2x scaling.
                let base_px = (grid_x * 8 + col) * 2;
                let base_py = (grid_y * 8 + row) * 2;
                for dy in 0..2u16 {
                    for dx in 0..2u16 {
                        let x = base_px + dx;
                        let y = base_py + dy;
                        let off = ((y as usize) * 128 + x as usize) * 4;
                        out[off..off + 4].copy_from_slice(&rgba);
                    }
                }
            }
        }
    }
    out
}

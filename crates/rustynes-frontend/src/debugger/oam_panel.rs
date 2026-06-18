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

use crate::emu::DebugPoke;

/// Persistent state of the OAM panel.
#[derive(Default)]
pub struct OamPanelState {
    /// Cached visual texture.
    visual_tex: Option<egui::TextureHandle>,
    /// v1.7.0 "Forge" Workstream A1 — editing state (master toggle, selected
    /// sprite, the four per-byte hex entry buffers, and the one-shot poke
    /// queue). Self-contained for clean merges.
    a1: OamEdit,
}

/// v1.7.0 "Forge" Workstream A1 — OAM editing state. Edits queue gated
/// post-frame OAM-byte pokes (`DebugPoke::Oam`); the panel never writes the
/// running `Nes` directly, preserving determinism + the `emu.write` gate.
#[derive(Default)]
struct OamEdit {
    /// Master enable (off by default → read-only, byte-identical).
    enabled: bool,
    /// The selected sprite index (0..64).
    sel: Option<u8>,
    /// Per-byte hex entry buffers: [Y, tile, attr, X].
    bytes: [String; 4],
    /// Pending writeback edits, drained by [`OamPanelState::take_pokes`].
    pending: Vec<DebugPoke>,
}

impl OamPanelState {
    /// v1.7.0 "Forge" Workstream A1 — drain the queued OAM writeback edits.
    pub fn take_pokes(&mut self) -> Vec<DebugPoke> {
        core::mem::take(&mut self.a1.pending)
    }
}

/// Parse a 2-hex-digit byte (lenient: trims `$`/`0x`).
fn parse_byte(s: &str) -> Option<u8> {
    let t = s.trim().trim_start_matches('$').trim_start_matches("0x");
    u8::from_str_radix(t, 16).ok()
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
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} sprites — {}",
                    64,
                    if ppu.sprite_size_16 { "8x16" } else { "8x8" }
                ));
                // v1.7.0 "Forge" Workstream A1 — editing master toggle. Off by
                // default → read-only (byte-identical with no edits queued).
                ui.checkbox(&mut state.a1.enabled, "Edit (writeback)");
            });
            ui.separator();
            // Sprite list (scrollable). While editing, each row is clickable to
            // select the sprite for the editor below.
            let editing = state.a1.enabled;
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
                        let text = format!(
                            "#{i:02}  x={x:3} y={y:3}  tile=${tile:02X}  pal={palette}  pri={priority}  flip={flip}"
                        );
                        if editing {
                            let selected = state.a1.sel == Some(i as u8);
                            if ui
                                .selectable_label(selected, egui::RichText::new(text).monospace())
                                .clicked()
                            {
                                state.a1.sel = Some(i as u8);
                                state.a1.bytes = [
                                    format!("{y:02X}"),
                                    format!("{tile:02X}"),
                                    format!("{attr:02X}"),
                                    format!("{x:02X}"),
                                ];
                            }
                        } else {
                            ui.monospace(text);
                        }
                    }
                });
            if editing {
                oam_editor(ui, &mut state.a1);
            }
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

/// v1.7.0 "Forge" Workstream A1 — the sprite-byte editor (Y / tile / attr / X).
/// "Apply" queues a gated OAM-byte writeback for each field that parses.
fn oam_editor(ui: &mut egui::Ui, a1: &mut OamEdit) {
    ui.separator();
    let Some(sel) = a1.sel else {
        ui.weak("Click a sprite row above to edit it.");
        return;
    };
    let labels = ["Y", "tile", "attr", "X"];
    ui.horizontal(|ui| {
        ui.monospace(format!("Sprite #{sel:02}"));
        for (b, label) in labels.iter().enumerate() {
            ui.label(format!("{label}:"));
            ui.add(
                egui::TextEdit::singleline(&mut a1.bytes[b])
                    .desired_width(28.0)
                    .hint_text("00"),
            );
        }
        if ui.button("Apply").clicked() {
            for b in 0..4u8 {
                if let Some(v) = parse_byte(&a1.bytes[b as usize]) {
                    a1.pending.push(DebugPoke::Oam {
                        idx: sel * 4 + b,
                        value: v,
                    });
                }
            }
        }
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

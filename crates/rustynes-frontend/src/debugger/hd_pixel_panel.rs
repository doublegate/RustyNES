//! HD-pack per-pixel inspector (v1.5.0 "Lens" Workstream A4).
//!
//! A read-only egui window (native + `hd-pack`) that traces the HD-pack
//! composition for a chosen NES pixel: the dominant tile's CHR identity + Mesen
//! hash, the replacement rule that matched (with its gating `<condition>` names
//! and whether each held this frame — ADR 0014), the base (stock) vs final
//! (composited) colour, and a blend slider for an original/mod preview value.
//!
//! Reference: `ref-proj/GeraNES/.../GeraNESApp.ModPixelInspectorWindowUI.inl`
//! (UX intent only; an independent Rust/egui reimplementation).
//!
//! Builds on the v1.4.0 HD-pack tile-source export + the v1.5.0
//! [`crate::hdpack::HdCompositor::inspect_pixel`] query. Display-only: it reads
//! the same already-deterministic per-frame snapshots `composite` consumed and
//! mutates nothing, so the determinism contract is unaffected. The whole module
//! is compiled only with `--features hd-pack` on native.

use egui::{Color32, Sense, Vec2};

use crate::gfx::{NES_H, NES_W};
use crate::hdpack::{HdCompositor, PixelInspection, WatchedMemory};
use rustynes_core::rustynes_ppu::HdTileSource;

/// Persistent state of the HD pixel inspector.
#[derive(Clone)]
pub struct HdPixelPanelState {
    /// Selected NES pixel (the pinned coordinate the report describes).
    px: u32,
    py: u32,
    /// Original/mod blend (0.0 = original, 1.0 = mod). A preview control; the
    /// app may read it to blend its preview.
    blend: f32,
}

impl Default for HdPixelPanelState {
    fn default() -> Self {
        Self {
            px: 128,
            py: 120,
            blend: 1.0,
        }
    }
}

impl HdPixelPanelState {
    /// The current original/mod blend value (0.0..=1.0).
    #[must_use]
    pub const fn blend(&self) -> f32 {
        self.blend
    }
}

/// A small RGBA swatch + hex label.
fn swatch(ui: &mut egui::Ui, label: &str, rgba: [u8; 4]) {
    ui.horizontal(|ui| {
        let (rect, _r) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::hover());
        ui.painter().rect_filled(
            rect,
            2.0,
            Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]),
        );
        ui.monospace(format!(
            "{label}: #{:02X}{:02X}{:02X}{:02X}",
            rgba[0], rgba[1], rgba[2], rgba[3]
        ));
    });
}

/// Render the HD pixel inspector. The app passes the live compositor + the same
/// per-frame snapshots it fed `composite`, plus a `chr_peek` over the captured
/// CHR snapshot. `pack_active` is `false` when no HD-pack is loaded.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    state: &mut HdPixelPanelState,
    pack_active: bool,
    compositor: Option<&HdCompositor>,
    framebuffer: &[u8],
    tile_source: &[HdTileSource],
    watched: &WatchedMemory,
    chr: &[u8],
) {
    egui::Window::new("HD Pixel Inspector")
        .open(open)
        .default_size([360.0, 440.0])
        .resizable(true)
        .show(ctx, |ui| {
            if !pack_active {
                ui.weak("Load an HD pack (Tools -> HD Pack) to inspect composition.");
                return;
            }
            let Some(comp) = compositor else {
                ui.weak("Load an HD pack (Tools -> HD Pack) to inspect composition.");
                return;
            };

            ui.horizontal(|ui| {
                ui.label("X");
                ui.add(egui::DragValue::new(&mut state.px).range(0..=(NES_W - 1)));
                ui.label("Y");
                ui.add(egui::DragValue::new(&mut state.py).range(0..=(NES_H - 1)));
            });
            ui.horizontal(|ui| {
                ui.label("Blend (orig <-> mod)");
                ui.add(egui::Slider::new(&mut state.blend, 0.0..=1.0));
            });
            ui.separator();

            // Verify the snapshots are the expected sizes before querying.
            let fb_ok = framebuffer.len() == (NES_W * NES_H * 4) as usize;
            let ts_ok = tile_source.len() == (NES_W * NES_H) as usize;
            if !fb_ok || !ts_ok {
                ui.weak("(waiting for a composited frame...)");
                return;
            }

            let chr_peek = |addr: u16| chr.get((addr & 0x1FFF) as usize).copied().unwrap_or(0);
            let Some(rep) = comp.inspect_pixel(
                state.px,
                state.py,
                framebuffer,
                tile_source,
                watched,
                chr_peek,
            ) else {
                ui.weak("(pixel off-screen)");
                return;
            };
            report(ui, &rep);
        });
}

/// Render the per-pixel composition report.
fn report(ui: &mut egui::Ui, rep: &PixelInspection) {
    ui.monospace(format!("Pixel ({}, {})", rep.x, rep.y));
    swatch(ui, "base ", rep.base);
    swatch(ui, "final", rep.final_rgba);
    ui.separator();

    if rep.chr_addr == 0xFFFF {
        ui.weak("Transparent / universal-background pixel (no tile).");
        return;
    }
    ui.monospace(format!(
        "tile CHR ${:04X}  {}  pal {}",
        rep.chr_addr,
        if rep.is_sprite { "sprite" } else { "bg" },
        rep.palette
    ));
    if rep.is_sprite {
        ui.monospace(format!("flip H {}  flip V {}", rep.flip_h, rep.flip_v));
    }
    if let Some(h) = rep.chr_hash {
        ui.monospace(format!("CHR hash {h:08X}"));
    }
    ui.separator();

    match (rep.replacement_image, rep.matched) {
        (None, _) => {
            ui.colored_label(
                Color32::from_gray(0xA0),
                "No replacement rule keys this tile hash.",
            );
        }
        (Some(img), true) => {
            ui.colored_label(
                Color32::from_rgb(0x60, 0xC0, 0x60),
                format!("Replacement APPLIED (image #{img})."),
            );
        }
        (Some(img), false) => {
            ui.colored_label(
                Color32::from_rgb(0xE0, 0xC0, 0x40),
                format!("Rule for image #{img} gated off (a condition failed)."),
            );
        }
    }

    if !rep.conditions.is_empty() {
        ui.label("Conditions:");
        for c in &rep.conditions {
            let (col, mark) = if c.held {
                (Color32::from_rgb(0x60, 0xC0, 0x60), "hold")
            } else {
                (Color32::from_rgb(0xE0, 0x60, 0x60), "fail")
            };
            ui.colored_label(col, format!("  {} [{mark}]", c.name));
        }
    } else if rep.replacement_image.is_some() {
        ui.weak("  (unconditional rule)");
    }
}

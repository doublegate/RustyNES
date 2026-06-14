//! Event viewer panel (v1.1.0 beta.2, Workstream C, T-110-C3).
//!
//! Plots this frame's CPU-write events (PPU `$2000-$3FFF`, APU `$4000-$4017`,
//! mapper `$4020-$FFFF`) on a scanline (rows) × dot (columns) grid, coloured by
//! kind — a quick way to see *when* in the frame a game touches scroll / mapper
//! / APU registers. Built on the core's `debug-hooks` event log (output-only,
//! reset per frame), so determinism is unaffected.

use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};
use rustynes_core::{EventKind, Nes};

/// NES frame extents for the grid: dots `0..=340`, scanlines `-1..=260`
/// (pre-render .. post-render), i.e. 341 × 262 cells.
const DOTS: f32 = 341.0;
const LINES: f32 = 262.0;

/// Event viewer panel state (stateless today).
#[derive(Default)]
pub struct EventPanelState;

const fn kind_color(kind: EventKind) -> Color32 {
    match kind {
        EventKind::PpuWrite => Color32::from_rgb(0x40, 0xC0, 0xFF), // cyan
        EventKind::ApuWrite => Color32::from_rgb(0xF0, 0xD0, 0x40), // yellow
        EventKind::MapperWrite => Color32::from_rgb(0xE0, 0x60, 0xE0), // magenta
    }
}

/// Render the event viewer.
#[allow(clippy::many_single_char_names)] // local geometric coords (w/h/x/y/p).
pub fn show(ctx: &egui::Context, open: &mut bool, _state: &mut EventPanelState, nes: &mut Nes) {
    egui::Window::new("Events")
        .open(open)
        .default_size([360.0, 300.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let mut on = nes.event_logging();
                if ui.checkbox(&mut on, "Record").changed() {
                    nes.set_event_logging(on);
                }
                ui.label(format!("{} events/frame", nes.events().len()));
            });
            ui.horizontal(|ui| {
                ui.colored_label(kind_color(EventKind::PpuWrite), "PPU $2000-3FFF");
                ui.colored_label(kind_color(EventKind::ApuWrite), "APU $4000-4017");
                ui.colored_label(kind_color(EventKind::MapperWrite), "Mapper");
            });
            ui.separator();

            // Copy the events so the painter closure doesn't hold a borrow of `nes`.
            let events: Vec<(EventKind, i16, u16)> = nes
                .events()
                .iter()
                .map(|e| (e.kind, e.scanline, e.dot))
                .collect();

            // A grid sized to keep the 341:262 aspect within the available area.
            let avail = ui.available_size();
            let w = avail.x.max(64.0);
            let h = (w * LINES / DOTS).min(avail.y.max(64.0));
            let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
            let p = ui.painter_at(rect);
            p.rect_filled(rect, 2.0, Color32::from_rgb(0x10, 0x10, 0x14));
            p.rect_stroke(rect, 2.0, Stroke::new(1.0, Color32::from_gray(0x50)));
            // Mark the start of the visible region (scanline 0) with a faint line.
            let vis_y = rect.top() + (1.0 / LINES) * rect.height();
            p.line_segment(
                [
                    Pos2::new(rect.left(), vis_y),
                    Pos2::new(rect.right(), vis_y),
                ],
                Stroke::new(1.0, Color32::from_gray(0x30)),
            );
            for (kind, scanline, dot) in events {
                // scanline -1 (pre-render) -> row 0; 0..=260 -> rows 1..=261.
                let row = (f32::from(scanline) + 1.0).clamp(0.0, LINES - 1.0);
                let x = rect.left() + (f32::from(dot) / DOTS) * rect.width();
                let y = rect.top() + (row / LINES) * rect.height();
                p.rect_filled(
                    Rect::from_min_size(Pos2::new(x, y), Vec2::new(2.0, 2.0)),
                    0.0,
                    kind_color(kind),
                );
            }
            if !nes.event_logging() {
                ui.weak("(enable Record, then run/step a frame)");
            }
        });
}

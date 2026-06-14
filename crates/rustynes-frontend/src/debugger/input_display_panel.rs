//! Input-display overlay (v1.1.0 beta.1, Workstream B) — a live controller HUD.
//!
//! A floating, read-only tool panel that draws a stylized NES controller per
//! active player with each currently-held button highlighted. Purely a
//! presentation aid: it reads the held-button snapshot the app pushes via
//! [`crate::debugger::DebuggerOverlay::set_input_display`] (the same
//! winit-thread `InputState` the emulator is fed), so it touches neither the
//! core nor the produce path and has no determinism impact.

use egui::{Color32, Pos2, Rect, Rounding, Sense, Stroke, Vec2};
use rustynes_core::Buttons;

/// Input-display panel state. Stateless today (the held buttons are pushed in
/// each frame), but kept as a struct for parity with the other panels and so
/// future options (e.g. per-player toggles) have a home.
#[derive(Default)]
pub struct InputDisplayPanelState;

/// Lit (held) vs idle button fill.
const LIT: Color32 = Color32::from_rgb(0x46, 0xC0, 0x50);
const IDLE: Color32 = Color32::from_rgb(0x3A, 0x3A, 0x3A);
const BODY: Color32 = Color32::from_rgb(0x20, 0x20, 0x24);
const OUTLINE: Color32 = Color32::from_rgb(0x70, 0x70, 0x78);

/// One controller diagram is this many logical points (before any DPI scale).
const PAD_W: f32 = 184.0;
const PAD_H: f32 = 84.0;

/// Render the input-display window. `pads[..players]` are the held buttons for
/// each active player (P1, P2, and — with Four Score — P3/P4).
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    _state: &mut InputDisplayPanelState,
    pads: &[Buttons],
    players: usize,
) {
    egui::Window::new("Input Display")
        .open(open)
        .resizable(false)
        .show(ctx, |ui| {
            let n = players.clamp(1, pads.len().min(4)).max(1);
            for p in 0..n {
                ui.label(egui::RichText::new(format!("Player {}", p + 1)).strong());
                draw_pad(ui, pads.get(p).copied().unwrap_or_default());
                if p + 1 < n {
                    ui.add_space(6.0);
                }
            }
        });
}

/// Draw a single NES controller with `held` buttons lit.
#[allow(clippy::many_single_char_names)] // local geometric coords (x/y/t/l/r).
fn draw_pad(ui: &mut egui::Ui, held: Buttons) {
    let (rect, _resp) = ui.allocate_exact_size(Vec2::new(PAD_W, PAD_H), Sense::hover());
    let p = ui.painter_at(rect);
    let o = rect.min;
    let at = |x: f32, y: f32| Pos2::new(o.x + x, o.y + y);
    let fill = |b: Buttons| if held.contains(b) { LIT } else { IDLE };

    // Controller body.
    p.rect_filled(rect, Rounding::same(8.0), BODY);
    p.rect_stroke(rect, Rounding::same(8.0), Stroke::new(1.0, OUTLINE));

    // D-pad cross (left side). Centre at (44, 42); arm thickness 16, length 16.
    let cx = 44.0;
    let cy = 42.0;
    let t = 8.0; // half-thickness
    let l = 14.0; // arm length
    let arm = |b: Buttons, min: Pos2, max: Pos2| {
        p.rect_filled(Rect::from_min_max(min, max), Rounding::same(2.0), fill(b));
    };
    // Centre square.
    p.rect_filled(
        Rect::from_min_max(at(cx - t, cy - t), at(cx + t, cy + t)),
        Rounding::ZERO,
        IDLE,
    );
    arm(Buttons::UP, at(cx - t, cy - t - l), at(cx + t, cy - t));
    arm(Buttons::DOWN, at(cx - t, cy + t), at(cx + t, cy + t + l));
    arm(Buttons::LEFT, at(cx - t - l, cy - t), at(cx - t, cy + t));
    arm(Buttons::RIGHT, at(cx + t, cy - t), at(cx + t + l, cy + t));

    // Select / Start (centre), small rounded pills.
    let pill = |b: Buttons, x: f32| {
        p.rect_filled(
            Rect::from_min_max(at(x, cy - 4.0), at(x + 20.0, cy + 4.0)),
            Rounding::same(4.0),
            fill(b),
        );
    };
    pill(Buttons::SELECT, 78.0);
    pill(Buttons::START, 104.0);
    p.text(
        at(88.0, cy + 12.0),
        egui::Align2::CENTER_CENTER,
        "SEL",
        egui::FontId::proportional(8.0),
        OUTLINE,
    );
    p.text(
        at(114.0, cy + 12.0),
        egui::Align2::CENTER_CENTER,
        "STA",
        egui::FontId::proportional(8.0),
        OUTLINE,
    );

    // B / A buttons (right side), round.
    let r = 11.0;
    p.circle_filled(at(150.0, cy), r, fill(Buttons::B));
    p.circle_filled(at(170.0, cy), r, fill(Buttons::A));
    p.text(
        at(150.0, cy),
        egui::Align2::CENTER_CENTER,
        "B",
        egui::FontId::proportional(11.0),
        Color32::WHITE,
    );
    p.text(
        at(170.0, cy),
        egui::Align2::CENTER_CENTER,
        "A",
        egui::FontId::proportional(11.0),
        Color32::WHITE,
    );
}

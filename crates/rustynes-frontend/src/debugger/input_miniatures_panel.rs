//! Input Miniatures overlay (v1.5.0 "Lens" Workstream A1).
//!
//! A live, read-only egui panel that draws a small diagram of every connected
//! input device — the standard pads on each port, plus whatever non-standard
//! device occupies the player-2 / expansion port (Zapper, Arkanoid Vaus, SNES
//! mouse, Power Pad / Family Trainer mat, Family BASIC / Subor keyboard, Konami
//! / Bandai Hyper Shot) — with real-time button / axis feedback. With the Four
//! Score it shows all four standard pads (multitap).
//!
//! Reference: `ref-proj/GeraNES/.../GeraNESApp.InputMiniaturesOverlayUI.inl`
//! (UX/layout intent only; this is an independent Rust/egui reimplementation).
//!
//! Frontend-only: it reads the same live host-side input snapshot the emulator
//! is fed (pushed each frame via
//! [`crate::debugger::DebuggerOverlay::set_input_miniatures`]), so it touches
//! neither the core nor the produce path and has no determinism impact.

// Lots of small geometric layout coords (x/y/w/h/o/p/r/c/t/l) in the device
// drawing code; the single-char-names lint isn't actionable here (matches the
// other debugger panels).
#![allow(clippy::many_single_char_names)]

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};
use rustynes_core::Buttons;

/// Active (pressed) vs idle element fill (GeraNES-inspired muted palette).
const ACTIVE: Color32 = Color32::from_rgb(0xC4, 0x2C, 0x24);
const IDLE: Color32 = Color32::from_rgb(0x49, 0x43, 0x37);
const CARD: Color32 = Color32::from_rgb(0x21, 0x1F, 0x1C);
const OUTLINE: Color32 = Color32::from_rgb(0x70, 0x68, 0x58);
const LABEL: Color32 = Color32::from_rgb(0xD8, 0xCF, 0xBE);

/// The live state of the player-2 / expansion device, captured by the app each
/// frame. Frontend-only; mirrors only what the miniatures panel needs to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ExpansionMini {
    /// No expansion device (standard controller on port 2).
    #[default]
    None,
    /// Zapper light gun.
    Zapper {
        /// Trigger held.
        trigger: bool,
        /// The light sensor currently sees the screen.
        on_screen: bool,
    },
    /// Arkanoid Vaus paddle.
    Vaus {
        /// Paddle travel, `0..=255`.
        knob: u8,
        /// Fire button held.
        button: bool,
    },
    /// SNES serial mouse.
    SnesMouse {
        /// Left button held.
        left: bool,
        /// Right button held.
        right: bool,
        /// Latest X motion delta (clamped to `-127..=127`).
        dx: i16,
        /// Latest Y motion delta (clamped to `-127..=127`).
        dy: i16,
    },
    /// Power Pad / Family Trainer mat.
    PowerPad {
        /// The 12-button mat mask (bits `0..12`).
        mask: u16,
        /// `true` if this is the Bandai Family Trainer (vs the NES Power Pad).
        family_trainer: bool,
    },
    /// Family BASIC / Subor keyboard.
    Keyboard {
        /// Count of currently-pressed keys.
        pressed: u8,
        /// `true` if this is the Subor keyboard (vs the Famicom Family BASIC).
        subor: bool,
    },
    /// Konami Hyper Shot (2-player Run/Jump).
    KonamiHyperShot {
        /// Player 1 Run held.
        p1_run: bool,
        /// Player 1 Jump held.
        p1_jump: bool,
        /// Player 2 Run held.
        p2_run: bool,
        /// Player 2 Jump held.
        p2_jump: bool,
    },
    /// Bandai Hyper Shot punching-bag controller.
    BandaiHyperShot {
        /// The 8-sensor mask (bits `0..8`).
        mask: u8,
    },
}

/// The per-frame input-miniatures snapshot the app pushes to the debugger.
#[derive(Clone, Debug)]
pub struct MiniaturesSnapshot {
    /// Held buttons per standard pad (P1..P4). `players` says how many are live.
    pub pads: [Buttons; 4],
    /// Number of standard pads to draw (2, or 4 with the Four Score).
    pub players: usize,
    /// The port-2 / expansion device, if any (drawn in place of pad P2 when set).
    pub expansion: ExpansionMini,
}

impl Default for MiniaturesSnapshot {
    fn default() -> Self {
        Self {
            pads: [Buttons::empty(); 4],
            players: 2,
            expansion: ExpansionMini::None,
        }
    }
}

/// Panel state — stateless today (the snapshot is pushed each frame).
#[derive(Default)]
pub struct InputMiniaturesPanelState;

/// Render the Input Miniatures overlay window.
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    _state: &mut InputMiniaturesPanelState,
    snap: &MiniaturesSnapshot,
) {
    egui::Window::new("Input Miniatures")
        .open(open)
        .resizable(false)
        .show(ctx, |ui| {
            // P1 standard pad.
            label(ui, "P1");
            draw_pad(ui, snap.pads.first().copied().unwrap_or_default());
            ui.add_space(6.0);

            // Port 2: either the expansion device or the standard P2 pad.
            match snap.expansion {
                ExpansionMini::None => {
                    label(ui, "P2");
                    draw_pad(ui, snap.pads.get(1).copied().unwrap_or_default());
                    // P3 / P4 (Four Score multitap).
                    if snap.players >= 3 {
                        for p in 2..snap.players.min(4) {
                            ui.add_space(6.0);
                            label(ui, &format!("P{}", p + 1));
                            draw_pad(ui, snap.pads.get(p).copied().unwrap_or_default());
                        }
                    }
                }
                exp => draw_expansion(ui, exp),
            }
        });
}

/// A device label line.
fn label(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).strong().color(LABEL));
}

/// Fill for a boolean (held) element.
const fn fill(active: bool) -> Color32 {
    if active { ACTIVE } else { IDLE }
}

/// A standard NES controller, this many logical points.
const PAD_W: f32 = 184.0;
const PAD_H: f32 = 84.0;

/// Draw a single NES controller with `held` buttons lit.
#[allow(clippy::many_single_char_names)] // local geometric coords (x/y/t/l/r).
fn draw_pad(ui: &mut egui::Ui, held: Buttons) {
    let (rect, _resp) = ui.allocate_exact_size(Vec2::new(PAD_W, PAD_H), Sense::hover());
    let p = ui.painter_at(rect);
    let o = rect.min;
    let at = |x: f32, y: f32| Pos2::new(o.x + x, o.y + y);
    // v1.5.0 I5 — mirror the Input Display per-group lit palette here so the two
    // overlays stay consistent: D-pad green, Select/Start yellow, B/A Nintendo
    // red. Non-standard devices (Zapper / Vaus / mouse / mat / keyboard) keep the
    // generic `ACTIVE` fill via `fill(..)`.
    let bfill = |b: Buttons, lit: Color32| {
        if held.contains(b) { lit } else { IDLE }
    };

    p.rect_filled(rect, CornerRadius::same(8), CARD);
    p.rect_stroke(
        rect,
        CornerRadius::same(8),
        Stroke::new(1.0, OUTLINE),
        egui::StrokeKind::Inside,
    );

    // D-pad cross (left side).
    let (cx, cy, t, l) = (44.0, 42.0, 8.0, 14.0);
    let arm = |b: Buttons, min: Pos2, max: Pos2| {
        p.rect_filled(
            Rect::from_min_max(min, max),
            CornerRadius::same(2),
            bfill(b, crate::input_colors::LIT_DPAD),
        );
    };
    p.rect_filled(
        Rect::from_min_max(at(cx - t, cy - t), at(cx + t, cy + t)),
        CornerRadius::ZERO,
        IDLE,
    );
    arm(Buttons::UP, at(cx - t, cy - t - l), at(cx + t, cy - t));
    arm(Buttons::DOWN, at(cx - t, cy + t), at(cx + t, cy + t + l));
    arm(Buttons::LEFT, at(cx - t - l, cy - t), at(cx - t, cy + t));
    arm(Buttons::RIGHT, at(cx + t, cy - t), at(cx + t + l, cy + t));

    // Select / Start pills.
    let pill = |b: Buttons, x: f32| {
        p.rect_filled(
            Rect::from_min_max(at(x, cy - 4.0), at(x + 20.0, cy + 4.0)),
            CornerRadius::same(4),
            bfill(b, crate::input_colors::LIT_STARTSEL),
        );
    };
    pill(Buttons::SELECT, 78.0);
    pill(Buttons::START, 104.0);

    // B / A buttons (right side).
    let r = 11.0;
    p.circle_filled(
        at(146.0, cy),
        r,
        bfill(Buttons::B, crate::input_colors::LIT_AB),
    );
    p.circle_filled(
        at(170.0, cy),
        r,
        bfill(Buttons::A, crate::input_colors::LIT_AB),
    );
    p.text(
        at(146.0, cy),
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

/// Allocate a card rectangle, paint its frame, and return the painter + origin.
fn card(ui: &mut egui::Ui, w: f32, h: f32) -> (egui::Painter, Pos2) {
    let (rect, _resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(6), CARD);
    p.rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.0, OUTLINE),
        egui::StrokeKind::Inside,
    );
    (p, rect.min)
}

/// Draw the expansion device diagram for port 2.
fn draw_expansion(ui: &mut egui::Ui, exp: ExpansionMini) {
    match exp {
        ExpansionMini::None => {}
        ExpansionMini::Zapper { trigger, on_screen } => {
            label(ui, "Zapper");
            let (p, o) = card(ui, 140.0, 48.0);
            // Light-sensor strip (lit when it sees the screen).
            p.rect_filled(
                Rect::from_min_size(o + Vec2::new(10.0, 8.0), Vec2::new(80.0, 12.0)),
                CornerRadius::same(2),
                fill(on_screen),
            );
            // Trigger.
            p.rect_filled(
                Rect::from_min_size(o + Vec2::new(40.0, 26.0), Vec2::new(60.0, 14.0)),
                CornerRadius::same(3),
                fill(trigger),
            );
            p.text(
                o + Vec2::new(115.0, 14.0),
                egui::Align2::CENTER_CENTER,
                "sight",
                egui::FontId::proportional(9.0),
                LABEL,
            );
        }
        ExpansionMini::Vaus { knob, button } => {
            label(ui, "Arkanoid");
            let (p, o) = card(ui, 140.0, 44.0);
            // Knob slider track.
            let track = Rect::from_min_size(o + Vec2::new(10.0, 20.0), Vec2::new(96.0, 4.0));
            p.rect_filled(track, CornerRadius::same(2), IDLE);
            let kx = 10.0 + (f32::from(knob) / 255.0) * 96.0;
            p.circle_filled(o + Vec2::new(kx, 22.0), 6.0, ACTIVE);
            // Button.
            p.circle_filled(o + Vec2::new(124.0, 22.0), 7.0, fill(button));
        }
        ExpansionMini::SnesMouse {
            left,
            right,
            dx,
            dy,
        } => {
            label(ui, "SNES Mouse");
            let (p, o) = card(ui, 120.0, 48.0);
            p.rect_filled(
                Rect::from_min_size(o + Vec2::new(10.0, 8.0), Vec2::new(44.0, 18.0)),
                CornerRadius::same(3),
                fill(left),
            );
            p.rect_filled(
                Rect::from_min_size(o + Vec2::new(66.0, 8.0), Vec2::new(44.0, 18.0)),
                CornerRadius::same(3),
                fill(right),
            );
            p.text(
                o + Vec2::new(60.0, 38.0),
                egui::Align2::CENTER_CENTER,
                format!("dx {dx:>4}  dy {dy:>4}"),
                egui::FontId::monospace(9.0),
                LABEL,
            );
        }
        ExpansionMini::PowerPad {
            mask,
            family_trainer,
        } => {
            label(
                ui,
                if family_trainer {
                    "Family Trainer"
                } else {
                    "Power Pad"
                },
            );
            draw_mat(ui, 4, 3, u32::from(mask));
        }
        ExpansionMini::Keyboard { pressed, subor } => {
            label(ui, if subor { "Subor KB" } else { "Family BASIC" });
            let (p, o) = card(ui, 120.0, 40.0);
            // A small key grid lit by count (visual proxy; the matrix is large).
            for (i, slot) in (0..18).enumerate() {
                let lit = (slot as u8) < pressed;
                let col = i % 9;
                let row = i / 9;
                let x = 8.0 + col as f32 * 12.0;
                let y = 8.0 + row as f32 * 12.0;
                p.rect_filled(
                    Rect::from_min_size(o + Vec2::new(x, y), Vec2::new(9.0, 9.0)),
                    CornerRadius::same(1),
                    fill(lit),
                );
            }
            p.text(
                o + Vec2::new(115.0, 20.0),
                egui::Align2::RIGHT_CENTER,
                format!("{pressed}"),
                egui::FontId::monospace(10.0),
                LABEL,
            );
        }
        ExpansionMini::KonamiHyperShot {
            p1_run,
            p1_jump,
            p2_run,
            p2_jump,
        } => {
            label(ui, "Konami Hyper Shot");
            let (p, o) = card(ui, 150.0, 46.0);
            let btn = |p: &egui::Painter, x: f32, y: f32, on: bool, t: &str| {
                p.circle_filled(o + Vec2::new(x, y), 8.0, fill(on));
                p.text(
                    o + Vec2::new(x, y + 14.0),
                    egui::Align2::CENTER_CENTER,
                    t,
                    egui::FontId::proportional(8.0),
                    LABEL,
                );
            };
            btn(&p, 30.0, 12.0, p1_run, "P1 RUN");
            btn(&p, 80.0, 12.0, p1_jump, "P1 JMP");
            btn(&p, 30.0, 32.0, p2_run, "P2 RUN");
            btn(&p, 80.0, 32.0, p2_jump, "P2 JMP");
        }
        ExpansionMini::BandaiHyperShot { mask } => {
            label(ui, "Bandai Hyper Shot");
            draw_mat(ui, 4, 2, u32::from(mask));
        }
    }
}

/// Draw a `cols x rows` button mat lit by a bitmask (bit `row*cols + col`).
fn draw_mat(ui: &mut egui::Ui, cols: usize, rows: usize, mask: u32) {
    let cell = 16.0;
    let gap = 5.0;
    let w = cols as f32 * (cell + gap) + gap;
    let h = rows as f32 * (cell + gap) + gap;
    let (p, o) = card(ui, w, h);
    let mut count = 0u32;
    for r in 0..rows {
        for c in 0..cols {
            let bit = r * cols + c;
            // Guard the shift: a future caller passing rows*cols > 32 would
            // otherwise panic in debug builds on the overflowing `1 << bit`.
            let on = bit < 32 && mask & (1u32 << bit) != 0;
            if on {
                count += 1;
            }
            let x = gap + c as f32 * (cell + gap);
            let y = gap + r as f32 * (cell + gap);
            p.rect_filled(
                Rect::from_min_size(o + Vec2::new(x, y), Vec2::new(cell, cell)),
                CornerRadius::same(2),
                fill(on),
            );
        }
    }
    if count > 0 {
        p.text(
            o + Vec2::new(w - 4.0, 8.0),
            egui::Align2::RIGHT_CENTER,
            format!("{count}"),
            egui::FontId::monospace(9.0),
            LABEL,
        );
    }
}

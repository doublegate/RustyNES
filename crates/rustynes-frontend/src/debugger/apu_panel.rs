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
//! APU panel — per-channel waveform scope (T-53-005).
//!
//! Each redraw appends the current per-channel output samples into a
//! rolling ring buffer; the scope plots the last N samples per channel.
//! The tap is lightweight (5 `u8` reads per polled frame) and only runs
//! when the panel is visible.

use rustynes_core::Nes;

const SCOPE_LEN: usize = 256;

/// Persistent state of the APU panel.
#[derive(Debug, Default)]
pub struct ApuPanelState {
    /// Rolling per-channel sample histories.
    pulse1: ScopeRing,
    pulse2: ScopeRing,
    triangle: ScopeRing,
    noise: ScopeRing,
    dmc: ScopeRing,
}

#[derive(Debug)]
struct ScopeRing {
    buf: [f32; SCOPE_LEN],
    head: usize,
}

impl Default for ScopeRing {
    fn default() -> Self {
        Self {
            buf: [0.0; SCOPE_LEN],
            head: 0,
        }
    }
}

impl ScopeRing {
    fn push(&mut self, v: f32) {
        self.buf[self.head] = v;
        self.head = (self.head + 1) % SCOPE_LEN;
    }
    /// Returns samples in chronological order (oldest first).
    fn snapshot(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(SCOPE_LEN);
        for i in 0..SCOPE_LEN {
            out.push(self.buf[(self.head + i) % SCOPE_LEN]);
        }
        out
    }
}

pub fn show(ctx: &egui::Context, open: &mut bool, state: &mut ApuPanelState, nes: &mut Nes) {
    let apu = nes.apu_snapshot();
    state.pulse1.push(f32::from(apu.pulse1) / 15.0);
    state.pulse2.push(f32::from(apu.pulse2) / 15.0);
    state.triangle.push(f32::from(apu.triangle) / 15.0);
    state.noise.push(f32::from(apu.noise) / 15.0);
    state.dmc.push(f32::from(apu.dmc) / 127.0);

    egui::Window::new("APU")
        .open(open)
        .default_pos([560.0, 480.0])
        .default_size([420.0, 360.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.monospace(format!(
                    "P1 {:>2}  P2 {:>2}  TRI {:>2}  NSE {:>2}  DMC {:>3}",
                    apu.pulse1, apu.pulse2, apu.triangle, apu.noise, apu.dmc
                ));
                if apu.frame_irq {
                    ui.colored_label(egui::Color32::YELLOW, "FRAME-IRQ");
                }
                if apu.dmc_irq {
                    ui.colored_label(egui::Color32::ORANGE, "DMC-IRQ");
                }
            });
            ui.separator();
            scope(ui, "Pulse 1", &state.pulse1, egui::Color32::LIGHT_BLUE);
            scope(ui, "Pulse 2", &state.pulse2, egui::Color32::LIGHT_GREEN);
            scope(ui, "Triangle", &state.triangle, egui::Color32::LIGHT_YELLOW);
            scope(ui, "Noise", &state.noise, egui::Color32::LIGHT_RED);
            scope(ui, "DMC", &state.dmc, egui::Color32::WHITE);
        });
}

fn scope(ui: &mut egui::Ui, label: &str, ring: &ScopeRing, color: egui::Color32) {
    ui.label(label);
    let samples = ring.snapshot();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, egui::Color32::from_black_alpha(180));
    let width = rect.width();
    let height = rect.height();
    let mut points = Vec::with_capacity(samples.len());
    for (i, &s) in samples.iter().enumerate() {
        let x = rect.min.x + (i as f32 / samples.len() as f32) * width;
        // Sample is 0..=1; we plot it as the inverted Y axis.
        let y = rect.max.y - s.clamp(0.0, 1.0) * height;
        points.push(egui::pos2(x, y));
    }
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.0, color)));
}

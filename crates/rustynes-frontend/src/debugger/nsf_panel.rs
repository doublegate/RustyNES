//! NSF music-player panel (v1.1.0 beta.2, Workstream D, T-110-D1).
//!
//! Shown automatically when an `.nsf` file is loaded (the file carries no PPU
//! program, so the framebuffer behind this window is blank). Displays the
//! file's title / artist / copyright (parsed from the header at load time and
//! stashed in [`NsfPanelState`]) and a track selector that drives
//! [`rustynes_core::Nes::nsf_set_song`].
//!
//! ## Per-channel waveform scope (v1.5.0 "Lens" Workstream C3)
//!
//! Below the track controls, a per-channel oscilloscope plots the live APU
//! output: pulse 1/2, triangle, noise, and DMC, each sampled from the
//! read-only `Nes::apu_snapshot()` DAC levels once per redraw (the same tap
//! the APU debugger panel uses). When the loaded NSF drives an expansion-audio
//! chip (VRC6/VRC7/FME-7/Namco 163/MMC5/FDS), the chip name is surfaced — the
//! expansion channels are summed into the master mix the standard APU already
//! plays. This is output-only eye-candy: it samples a copy for display and
//! changes no synthesis, so the deterministic audio is unaffected.

use rustynes_core::Nes;

/// Number of samples retained per channel scope (one per redraw, ~60 Hz).
const SCOPE_LEN: usize = 256;

/// NSF panel state. The metadata strings are populated by the frontend at load
/// time (the core mapper does not retain them); the live track index is read
/// straight from the `Nes`.
#[derive(Default)]
pub struct NsfPanelState {
    /// Song title (NSF header `$0E`).
    pub title: String,
    /// Artist (NSF header `$2E`).
    pub artist: String,
    /// Copyright holder (NSF header `$4E`).
    pub copyright: String,
    /// v1.5.0 C3 — per-channel scope ring buffers (pulse1/2, triangle, noise,
    /// DMC). Output-only display state.
    pulse1: ScopeRing,
    pulse2: ScopeRing,
    triangle: ScopeRing,
    noise: ScopeRing,
    dmc: ScopeRing,
}

/// A small rolling sample history for one channel scope.
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
}

impl NsfPanelState {
    /// Replace the displayed metadata (called when a new NSF is loaded).
    pub fn set_metadata(&mut self, title: String, artist: String, copyright: String) {
        self.title = title;
        self.artist = artist;
        self.copyright = copyright;
    }
}

/// Render the NSF player window.
#[allow(
    clippy::needless_pass_by_ref_mut,
    clippy::cast_precision_loss,
    clippy::too_many_lines
)]
pub fn show(ctx: &egui::Context, open: &mut bool, state: &mut NsfPanelState, nes: &mut Nes) {
    let total = nes.nsf_song_count();
    // v1.5.0 C3 — sample the live per-channel DAC levels (read-only) so the
    // scope appends one column per redraw.
    let apu = nes.apu_snapshot();
    state.pulse1.push(f32::from(apu.pulse1) / 15.0);
    state.pulse2.push(f32::from(apu.pulse2) / 15.0);
    state.triangle.push(f32::from(apu.triangle) / 15.0);
    state.noise.push(f32::from(apu.noise) / 15.0);
    state.dmc.push(f32::from(apu.dmc) / 127.0);
    let expansion = nes.expansion_audio_chip();

    egui::Window::new("NSF Player")
        .open(open)
        .default_size([340.0, 440.0])
        .resizable(true)
        .show(ctx, |ui| {
            if total == 0 {
                ui.weak("No NSF loaded.");
                return;
            }

            // A `fn` (not a closure) so the borrowed `&str` return lifetime elides
            // to the input — no per-frame heap allocation in the UI render loop.
            fn show_or_dash(s: &str) -> &str {
                if s.is_empty() { "—" } else { s }
            }
            egui::Grid::new("nsf_meta").num_columns(2).show(ui, |ui| {
                ui.strong("Title");
                ui.label(show_or_dash(&state.title));
                ui.end_row();
                ui.strong("Artist");
                ui.label(show_or_dash(&state.artist));
                ui.end_row();
                ui.strong("Copyright");
                ui.label(show_or_dash(&state.copyright));
                ui.end_row();
            });
            ui.separator();

            let current = nes.nsf_current_song();
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("Track {} / {total}", current + 1)).strong());
            });
            ui.horizontal(|ui| {
                // saturating prev/next; selection restarts the track via init.
                if ui
                    .add_enabled(current > 0, egui::Button::new("⏮ Prev"))
                    .clicked()
                {
                    nes.nsf_set_song(current - 1);
                }
                if ui
                    .add_enabled(current + 1 < total, egui::Button::new("Next ⏭"))
                    .clicked()
                {
                    nes.nsf_set_song(current + 1);
                }
                if ui.button("⟲ Restart").clicked() {
                    nes.nsf_set_song(current);
                }
            });

            // A direct track picker for files with many songs.
            if total > 1 {
                ui.add_space(4.0);
                let mut sel = current;
                let last = total - 1;
                if ui
                    .add(egui::Slider::new(&mut sel, 0..=last).text("song index"))
                    .changed()
                {
                    nes.nsf_set_song(sel);
                }
            }

            ui.separator();

            // --- v1.5.0 C3 — per-channel waveform scope ---
            ui.strong("Channel scope");
            scope(ui, "Pulse 1", &state.pulse1, egui::Color32::LIGHT_BLUE);
            scope(ui, "Pulse 2", &state.pulse2, egui::Color32::LIGHT_GREEN);
            scope(ui, "Triangle", &state.triangle, egui::Color32::LIGHT_YELLOW);
            scope(ui, "Noise", &state.noise, egui::Color32::LIGHT_RED);
            scope(ui, "DMC", &state.dmc, egui::Color32::WHITE);
            if let Some(chip) = expansion {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label("Expansion:");
                    ui.colored_label(egui::Color32::from_rgb(0xC0, 0x90, 0xF0), chip);
                });
                ui.weak("Expansion channels are summed into the master mix above.");
            }

            ui.add_space(4.0);
            ui.weak("Audio plays through the standard APU; NSF files carry no video.");
            ui.weak(
                "Tempo \u{2248} NTSC 60 Hz (vblank-driven); non-60 Hz tunes play slightly off.",
            );
        });
}

/// Draw one channel's rolling waveform into a fixed-height strip.
#[allow(clippy::cast_precision_loss)]
fn scope(ui: &mut egui::Ui, label: &str, ring: &ScopeRing, color: egui::Color32) {
    ui.label(label);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, egui::Color32::from_black_alpha(180));
    let width = rect.width();
    let height = rect.height();
    let mut points = Vec::with_capacity(SCOPE_LEN);
    for i in 0..SCOPE_LEN {
        // chronological order (oldest first): start at head.
        let s = ring.buf[(ring.head + i) % SCOPE_LEN];
        let x = rect.min.x + (i as f32 / SCOPE_LEN as f32) * width;
        // Sample is 0..=1; plot on the inverted Y axis.
        let y = rect.max.y - s.clamp(0.0, 1.0) * height;
        points.push(egui::pos2(x, y));
    }
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.0, color)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_ring_wraps_and_keeps_latest() {
        let mut r = ScopeRing::default();
        for i in 0..(SCOPE_LEN + 5) {
            r.push(i as f32);
        }
        // After SCOPE_LEN+5 pushes the head wrapped; the newest sample is the
        // one just before the head.
        let newest = r.buf[(r.head + SCOPE_LEN - 1) % SCOPE_LEN];
        assert!((newest - (SCOPE_LEN as f32 + 4.0)).abs() < f32::EPSILON);
    }
}

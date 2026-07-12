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
//!
//! v1.8.9 adds depth to the scope: a **master (mixed) scope** plotting the
//! combined per-redraw level, and a row of **per-channel peak VU meters** (the
//! recent max level of each channel's ring). Both derive from the same
//! already-sampled DAC copies, so they remain output-only and determinism-neutral.
//!
//! v2.1.6 "Expansion Audio" adds an **expansion-channel scope + VU** (the raw
//! on-cart VRC6/VRC7/FDS/MMC5/N163/5B contribution, [`Nes::apu_snapshot`]'s
//! `external` tap) and factors the scope/VU/ring primitives out into the shared
//! [`super::audio_scope`] module (also used by the Audio Mixer panel).

use rustynes_core::Nes;

use super::audio_scope::{ScopeRing, scope, vu_meter};

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
    /// v1.8.9 — the combined (mixed) per-redraw level, for a master scope.
    master: ScopeRing,
    /// v2.1.6 — the raw on-cart expansion-audio contribution (VRC6/VRC7/FDS/
    /// MMC5/N163/5B), sampled from the read-only `external` DAC tap. Silent
    /// (flat) when the loaded NSF drives no expansion chip.
    external: ScopeRing,
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
    let p1 = f32::from(apu.pulse1) / 15.0;
    let p2 = f32::from(apu.pulse2) / 15.0;
    let tri = f32::from(apu.triangle) / 15.0;
    let noi = f32::from(apu.noise) / 15.0;
    let dmc = f32::from(apu.dmc) / 127.0;
    // v2.1.6 — the raw expansion-audio contribution. `external` is a small
    // signed sample (~[-0.5, 0.5] scale, like a mixed channel); take its
    // magnitude and clamp for the 0..=1 scope/VU convention. Guard non-finite
    // first (a NaN self isn't pinned by `f32::clamp`) so it maps to silence
    // instead of a garbage trace point.
    let ext = if apu.external.is_finite() {
        (apu.external.abs() * 2.0).min(1.0)
    } else {
        0.0
    };
    state.pulse1.push(p1);
    state.pulse2.push(p2);
    state.triangle.push(tri);
    state.noise.push(noi);
    state.dmc.push(dmc);
    state.external.push(ext);
    // v1.8.9 — the combined (averaged) level for the master scope. v2.1.6 —
    // include the expansion channel so the master reflects all six sources.
    state.master.push((p1 + p2 + tri + noi + dmc + ext) / 6.0);
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
            // v1.8.9 — master (mixed) scope + per-channel peak VU meters.
            ui.add_space(2.0);
            scope(
                ui,
                "Master (mix)",
                &state.master,
                egui::Color32::from_rgb(0xFF, 0xC0, 0x40),
            );
            ui.add_space(2.0);
            ui.strong("Levels");
            vu_meter(ui, "P1 ", state.pulse1.peak(), egui::Color32::LIGHT_BLUE);
            vu_meter(ui, "P2 ", state.pulse2.peak(), egui::Color32::LIGHT_GREEN);
            vu_meter(
                ui,
                "Tri",
                state.triangle.peak(),
                egui::Color32::LIGHT_YELLOW,
            );
            vu_meter(ui, "Noi", state.noise.peak(), egui::Color32::LIGHT_RED);
            vu_meter(ui, "DMC", state.dmc.peak(), egui::Color32::WHITE);
            if let Some(chip) = expansion {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label("Expansion:");
                    ui.colored_label(egui::Color32::from_rgb(0xC0, 0x90, 0xF0), chip);
                });
                // v2.1.6 — the expansion chip's own scope + VU (raw contribution).
                scope(
                    ui,
                    chip,
                    &state.external,
                    egui::Color32::from_rgb(0xC0, 0x90, 0xF0),
                );
                vu_meter(
                    ui,
                    "Ext",
                    state.external.peak(),
                    egui::Color32::from_rgb(0xC0, 0x90, 0xF0),
                );
                ui.weak("Expansion channels are summed into the master mix above.");
            }

            ui.add_space(4.0);
            ui.weak("Audio plays through the standard APU; NSF files carry no video.");
            ui.weak(
                "Tempo \u{2248} NTSC 60 Hz (vblank-driven); non-60 Hz tunes play slightly off.",
            );
        });
}

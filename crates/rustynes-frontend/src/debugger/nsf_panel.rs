//! NSF music-player panel (v1.1.0 beta.2, Workstream D, T-110-D1).
//!
//! Shown automatically when an `.nsf` file is loaded (the file carries no PPU
//! program, so the framebuffer behind this window is blank). Displays the
//! file's title / artist / copyright (parsed from the header at load time and
//! stashed in [`NsfPanelState`]) and a track selector that drives
//! [`rustynes_core::Nes::nsf_set_song`].

use rustynes_core::Nes;

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
// State is read-only today (metadata is pushed in via `set_metadata`), but the
// `&mut` keeps the signature uniform with the other chip panels.
#[allow(clippy::needless_pass_by_ref_mut)]
pub fn show(ctx: &egui::Context, open: &mut bool, state: &mut NsfPanelState, nes: &mut Nes) {
    let total = nes.nsf_song_count();
    egui::Window::new("NSF Player")
        .open(open)
        .default_size([320.0, 200.0])
        .resizable(true)
        .show(ctx, |ui| {
            if total == 0 {
                ui.weak("No NSF loaded.");
                return;
            }

            let show_or_dash = |s: &str| {
                if s.is_empty() {
                    "—".to_owned()
                } else {
                    s.to_owned()
                }
            };
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

            ui.add_space(4.0);
            ui.weak("Audio plays through the standard APU; NSF files carry no video.");
        });
}

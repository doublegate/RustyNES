//! APU debug window.
//!
//! Note: Detailed APU state access requires additional core API.
//! This is a placeholder showing basic audio info.

use egui::Context;
use rustynes_core::Console;

/// Render the APU debug window.
pub fn render(ctx: &Context, open: &mut bool, console: &Option<Console>) {
    egui::Window::new("APU Debug")
        .open(open)
        .resizable(true)
        .default_width(350.0)
        .show(ctx, |ui| {
            if let Some(ref cons) = console {
                ui.heading("Audio Info");
                egui::Grid::new("apu_info")
                    .num_columns(2)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Frame:");
                        ui.monospace(format!("{}", cons.frame_count()));
                        ui.end_row();

                        ui.label("Total Cycles:");
                        ui.monospace(format!("{}", cons.cycles()));
                        ui.end_row();

                        let samples = cons.audio_samples();
                        ui.label("Sample Buffer:");
                        ui.monospace(format!("{} samples", samples.len()));
                        ui.end_row();
                    });

                ui.add_space(10.0);

                // Show channel status header (placeholder)
                ui.heading("Channels");
                ui.label("APU channels: Pulse 1, Pulse 2, Triangle, Noise, DMC");

                ui.add_space(10.0);

                ui.label("Note: Detailed APU channel state access");
                ui.label("requires additional core API support.");
            } else {
                ui.label("No ROM loaded");
            }
        });
}

//! PPU debug window.
//!
//! Note: Detailed PPU state access requires additional core API.
//! This is a placeholder showing basic frame info.

use egui::Context;
use rustynes_core::Console;

/// Render the PPU debug window.
pub fn render(ctx: &Context, open: &mut bool, console: &Option<Console>) {
    egui::Window::new("PPU Debug")
        .open(open)
        .resizable(true)
        .default_width(350.0)
        .show(ctx, |ui| {
            if let Some(ref cons) = console {
                ui.heading("Frame Info");
                egui::Grid::new("ppu_info")
                    .num_columns(2)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Frame:");
                        ui.monospace(format!("{}", cons.frame_count()));
                        ui.end_row();

                        ui.label("Total Cycles:");
                        ui.monospace(format!("{}", cons.cycles()));
                        ui.end_row();
                    });

                ui.add_space(10.0);

                ui.label("Note: Detailed PPU register access");
                ui.label("requires additional core API support.");
            } else {
                ui.label("No ROM loaded");
            }
        });
}

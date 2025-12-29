//! PPU debug window.
//!
//! Shows cycle-level PPU state including scanline, dot position,
//! scroll registers, and rendering status.

use egui::Context;
use rustynes_core::Console;

/// Render the PPU debug window.
pub fn render(ctx: &Context, open: &mut bool, console: &Option<Console>) {
    egui::Window::new("PPU Debug")
        .open(open)
        .resizable(true)
        .default_width(350.0)
        .show(ctx, |ui| {
            if let Some(cons) = console {
                let bus = cons.bus();
                let ppu = &bus.ppu;

                // Timing section
                ui.heading("Timing");
                egui::Grid::new("ppu_timing")
                    .num_columns(2)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Scanline:");
                        let scanline = ppu.scanline();
                        let scanline_desc = match scanline {
                            0..=239 => "Visible",
                            240 => "Post-render",
                            241..=260 => "VBlank",
                            261 => "Pre-render",
                            _ => "Unknown",
                        };
                        ui.monospace(format!("{scanline} ({scanline_desc})"));
                        ui.end_row();

                        ui.label("Dot:");
                        ui.monospace(format!("{} / 341", ppu.dot()));
                        ui.end_row();

                        ui.label("Frame:");
                        ui.monospace(format!("{}", cons.frame_count()));
                        ui.end_row();

                        ui.label("Total Cycles:");
                        ui.monospace(format!("{}", cons.cycles()));
                        ui.end_row();
                    });

                ui.add_space(10.0);

                // Scroll registers section
                ui.heading("Scroll Registers");
                egui::Grid::new("ppu_scroll")
                    .num_columns(2)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("VRAM Addr (v):");
                        ui.monospace(format!("${:04X}", ppu.vram_addr()));
                        ui.end_row();

                        ui.label("Temp Addr (t):");
                        ui.monospace(format!("${:04X}", ppu.temp_vram_addr()));
                        ui.end_row();

                        ui.label("Fine X:");
                        ui.monospace(format!("{}", ppu.fine_x()));
                        ui.end_row();

                        ui.label("Coarse X:");
                        ui.monospace(format!("{}", ppu.coarse_x()));
                        ui.end_row();

                        ui.label("Coarse Y:");
                        ui.monospace(format!("{}", ppu.coarse_y()));
                        ui.end_row();

                        ui.label("Fine Y:");
                        ui.monospace(format!("{}", ppu.fine_y()));
                        ui.end_row();
                    });

                ui.add_space(10.0);

                // Mid-scanline detection
                ui.heading("Status");
                egui::Grid::new("ppu_status")
                    .num_columns(2)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Mid-Scanline Write:");
                        let mid_write = ppu.mid_scanline_write_detected();
                        let color = if mid_write {
                            egui::Color32::YELLOW
                        } else {
                            egui::Color32::GRAY
                        };
                        ui.label(
                            egui::RichText::new(if mid_write { "DETECTED" } else { "None" })
                                .color(color)
                                .monospace(),
                        );
                        ui.end_row();

                        if mid_write {
                            ui.label("Last V Before:");
                            ui.monospace(format!("${:04X}", ppu.last_v_before_update()));
                            ui.end_row();
                        }
                    });
            } else {
                ui.label("No ROM loaded");
            }
        });
}

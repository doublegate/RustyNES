//! CPU debug window.

use egui::Context;
use rustynes_core::Console;

/// Render the CPU debug window.
pub fn render(ctx: &Context, open: &mut bool, console: &Option<Console>) {
    egui::Window::new("CPU Debug")
        .open(open)
        .resizable(true)
        .default_width(300.0)
        .show(ctx, |ui| {
            if let Some(ref cons) = console {
                let cpu = cons.cpu();

                // Registers section
                ui.heading("Registers");
                egui::Grid::new("cpu_registers")
                    .num_columns(2)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("PC:");
                        ui.monospace(format!("${:04X}", cpu.pc));
                        ui.end_row();

                        ui.label("A:");
                        ui.monospace(format!("${:02X} ({})", cpu.a, cpu.a));
                        ui.end_row();

                        ui.label("X:");
                        ui.monospace(format!("${:02X} ({})", cpu.x, cpu.x));
                        ui.end_row();

                        ui.label("Y:");
                        ui.monospace(format!("${:02X} ({})", cpu.y, cpu.y));
                        ui.end_row();

                        ui.label("SP:");
                        ui.monospace(format!("${:02X}", cpu.sp));
                        ui.end_row();

                        ui.label("P:");
                        ui.monospace(format!("${:02X}", cpu.status.bits()));
                        ui.end_row();
                    });

                ui.add_space(10.0);

                // Status flags section
                ui.heading("Status Flags");
                ui.horizontal(|ui| {
                    let p = cpu.status.bits();
                    flag_label(ui, "N", (p & 0x80) != 0);
                    flag_label(ui, "V", (p & 0x40) != 0);
                    flag_label(ui, "-", true);
                    flag_label(ui, "B", (p & 0x10) != 0);
                    flag_label(ui, "D", (p & 0x08) != 0);
                    flag_label(ui, "I", (p & 0x04) != 0);
                    flag_label(ui, "Z", (p & 0x02) != 0);
                    flag_label(ui, "C", (p & 0x01) != 0);
                });

                ui.add_space(10.0);

                // Cycle counter
                ui.heading("Timing");
                egui::Grid::new("cpu_timing")
                    .num_columns(2)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Cycles:");
                        ui.monospace(format!("{}", cpu.cycles));
                        ui.end_row();

                        ui.label("Frame:");
                        ui.monospace(format!("{}", cons.frame_count()));
                        ui.end_row();
                    });
            } else {
                ui.label("No ROM loaded");
            }
        });
}

/// Render a flag label with color indication.
fn flag_label(ui: &mut egui::Ui, name: &str, set: bool) {
    let color = if set {
        egui::Color32::GREEN
    } else {
        egui::Color32::GRAY
    };
    ui.label(egui::RichText::new(name).color(color).monospace());
}

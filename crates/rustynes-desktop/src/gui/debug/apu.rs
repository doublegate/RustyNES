//! APU debug window.
//!
//! Shows cycle-level APU state including timing, sample buffer status,
//! IRQ state, and DMC output level.

use crate::audio::AudioOutput;
use egui::Context;
use rustynes_core::Console;

/// Render the APU debug window.
#[allow(clippy::too_many_lines)]
pub fn render(
    ctx: &Context,
    open: &mut bool,
    console: &Option<Console>,
    audio: &Option<AudioOutput>,
) {
    egui::Window::new("APU Debug")
        .open(open)
        .resizable(true)
        .default_width(350.0)
        .show(ctx, |ui| {
            if let Some(cons) = console {
                let bus = cons.bus();
                let apu = &bus.apu;

                // Timing section
                ui.heading("Timing");
                egui::Grid::new("apu_timing")
                    .num_columns(2)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("APU Cycles:");
                        ui.monospace(format!("{}", apu.cycles()));
                        ui.end_row();

                        ui.label("Frame:");
                        ui.monospace(format!("{}", cons.frame_count()));
                        ui.end_row();

                        ui.label("CPU Cycles:");
                        ui.monospace(format!("{}", cons.cycles()));
                        ui.end_row();
                    });

                ui.add_space(10.0);

                // IRQ Status section
                ui.heading("IRQ Status");
                egui::Grid::new("apu_irq")
                    .num_columns(2)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("IRQ Pending:");
                        let irq = apu.irq_pending();
                        let color = if irq {
                            egui::Color32::RED
                        } else {
                            egui::Color32::GRAY
                        };
                        ui.label(
                            egui::RichText::new(if irq { "YES" } else { "No" })
                                .color(color)
                                .monospace(),
                        );
                        ui.end_row();
                    });

                ui.add_space(10.0);

                // DMC section
                ui.heading("DMC Channel");
                egui::Grid::new("apu_dmc")
                    .num_columns(2)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Output Level:");
                        let dmc_out = apu.dmc_output();
                        ui.monospace(format!("{dmc_out} / 127"));
                        ui.end_row();

                        // Show DMC level bar
                        ui.label("Level:");
                        let progress = f32::from(dmc_out) / 127.0;
                        ui.add(egui::ProgressBar::new(progress));
                        ui.end_row();
                    });

                ui.add_space(10.0);

                // Sample buffer section
                ui.heading("Sample Buffer");
                egui::Grid::new("apu_samples")
                    .num_columns(2)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        let samples = cons.audio_samples();
                        ui.label("Pending Samples:");
                        ui.monospace(format!("{}", samples.len()));
                        ui.end_row();

                        // Audio output buffer stats if available
                        if let Some(audio_out) = audio {
                            let stats = audio_out.latency_stats();

                            ui.label("Buffer Fill:");
                            let fill_color = if stats.is_healthy {
                                egui::Color32::GREEN
                            } else {
                                egui::Color32::YELLOW
                            };
                            ui.label(
                                egui::RichText::new(format!("{:.1}%", stats.buffer_fill * 100.0))
                                    .color(fill_color)
                                    .monospace(),
                            );
                            ui.end_row();

                            ui.label("Latency:");
                            ui.monospace(format!("{:.1} ms", stats.latency_ms));
                            ui.end_row();

                            ui.label("Underruns:");
                            let underrun_color = if stats.underruns > 0 {
                                egui::Color32::RED
                            } else {
                                egui::Color32::GRAY
                            };
                            ui.label(
                                egui::RichText::new(format!("{}", stats.underruns))
                                    .color(underrun_color)
                                    .monospace(),
                            );
                            ui.end_row();

                            ui.label("Health:");
                            let health_color = if stats.is_healthy {
                                egui::Color32::GREEN
                            } else {
                                egui::Color32::YELLOW
                            };
                            ui.label(
                                egui::RichText::new(if stats.is_healthy { "OK" } else { "WARN" })
                                    .color(health_color)
                                    .monospace(),
                            );
                            ui.end_row();
                        }
                    });

                ui.add_space(10.0);

                // Channels overview
                ui.heading("Channels");
                ui.horizontal(|ui| {
                    channel_indicator(ui, "P1", true);
                    channel_indicator(ui, "P2", true);
                    channel_indicator(ui, "Tri", true);
                    channel_indicator(ui, "Noi", true);
                    channel_indicator(ui, "DMC", true);
                });
            } else {
                ui.label("No ROM loaded");
            }
        });
}

/// Render a channel status indicator.
fn channel_indicator(ui: &mut egui::Ui, name: &str, enabled: bool) {
    let color = if enabled {
        egui::Color32::GREEN
    } else {
        egui::Color32::DARK_GRAY
    };
    ui.label(egui::RichText::new(name).color(color).monospace());
}

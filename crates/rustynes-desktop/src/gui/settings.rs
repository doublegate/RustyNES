//! Settings dialog implementation.

use crate::audio::AudioOutput;
use crate::config::Config;
use egui::Context;

/// Render the settings window.
#[allow(clippy::too_many_lines)]
pub fn render_settings(
    ctx: &Context,
    open: &mut bool,
    config: &mut Config,
    audio: &Option<AudioOutput>,
) {
    egui::Window::new("Settings")
        .open(open)
        .resizable(true)
        .default_width(400.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                // Video Settings
                ui.collapsing("Video", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Window Scale:");
                        egui::ComboBox::from_id_salt("scale")
                            .selected_text(format!("{}x", config.video.scale))
                            .show_ui(ui, |ui| {
                                for scale in 1..=8 {
                                    ui.selectable_value(
                                        &mut config.video.scale,
                                        scale,
                                        format!("{scale}x"),
                                    );
                                }
                            });
                    });

                    ui.checkbox(&mut config.video.fullscreen, "Fullscreen");
                    ui.checkbox(&mut config.video.vsync, "VSync");
                    ui.checkbox(
                        &mut config.video.pixel_aspect_correction,
                        "8:7 Pixel Aspect Ratio (NES Native)",
                    );
                    ui.checkbox(&mut config.video.show_fps, "Show FPS Counter");
                });

                ui.add_space(10.0);

                // Audio Settings
                ui.collapsing("Audio", |ui| {
                    let mut muted = audio.as_ref().map_or(
                        config.audio.muted,
                        super::super::audio::AudioOutput::is_muted,
                    );
                    if ui.checkbox(&mut muted, "Mute").changed() {
                        if let Some(audio) = audio {
                            audio.set_muted(muted);
                        }
                        config.audio.muted = muted;
                    }

                    ui.horizontal(|ui| {
                        ui.label("Volume:");
                        let mut volume = audio.as_ref().map_or(
                            config.audio.volume,
                            super::super::audio::AudioOutput::volume,
                        );
                        if ui
                            .add(egui::Slider::new(&mut volume, 0.0..=1.0).show_value(true))
                            .changed()
                        {
                            if let Some(audio) = audio {
                                audio.set_volume(volume);
                            }
                            config.audio.volume = volume;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Sample Rate:");
                        egui::ComboBox::from_id_salt("sample_rate")
                            .selected_text(format!("{} Hz", config.audio.sample_rate))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut config.audio.sample_rate,
                                    44100,
                                    "44100 Hz",
                                );
                                ui.selectable_value(
                                    &mut config.audio.sample_rate,
                                    48000,
                                    "48000 Hz",
                                );
                                ui.selectable_value(
                                    &mut config.audio.sample_rate,
                                    96000,
                                    "96000 Hz",
                                );
                            });
                    });

                    ui.horizontal(|ui| {
                        ui.label("Buffer Size:");
                        egui::ComboBox::from_id_salt("buffer_size")
                            .selected_text(format!("{} samples", config.audio.buffer_size))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut config.audio.buffer_size,
                                    512,
                                    "512 samples",
                                );
                                ui.selectable_value(
                                    &mut config.audio.buffer_size,
                                    1024,
                                    "1024 samples",
                                );
                                ui.selectable_value(
                                    &mut config.audio.buffer_size,
                                    2048,
                                    "2048 samples",
                                );
                                ui.selectable_value(
                                    &mut config.audio.buffer_size,
                                    4096,
                                    "4096 samples",
                                );
                            });
                    });
                });

                ui.add_space(10.0);

                // Input Settings
                ui.collapsing("Input - Player 1", |ui| {
                    render_key_bindings(ui, &mut config.input.player1_keyboard, "p1");
                });

                ui.add_space(10.0);

                ui.collapsing("Input - Player 2", |ui| {
                    render_key_bindings(ui, &mut config.input.player2_keyboard, "p2");
                });

                ui.add_space(10.0);

                // Debug Settings
                ui.collapsing("Debug", |ui| {
                    ui.checkbox(&mut config.debug.enabled, "Enable Debug Mode");
                    ui.checkbox(&mut config.debug.show_cpu, "Show CPU Window on Start");
                    ui.checkbox(&mut config.debug.show_ppu, "Show PPU Window on Start");
                    ui.checkbox(&mut config.debug.show_apu, "Show APU Window on Start");
                    ui.checkbox(&mut config.debug.show_memory, "Show Memory Viewer on Start");
                });

                ui.add_space(20.0);

                // Buttons
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked()
                        && let Err(e) = config.save()
                    {
                        log::error!("Failed to save config: {e}");
                    }

                    if ui.button("Reset to Defaults").clicked() {
                        *config = Config::default();
                    }
                });
            });
        });
}

/// Render key binding inputs for a player.
fn render_key_bindings(
    ui: &mut egui::Ui,
    bindings: &mut crate::config::KeyboardBindings,
    id_prefix: &str,
) {
    egui::Grid::new(format!("{id_prefix}_bindings"))
        .num_columns(2)
        .spacing([20.0, 4.0])
        .show(ui, |ui| {
            ui.label("A Button:");
            ui.text_edit_singleline(&mut bindings.a);
            ui.end_row();

            ui.label("B Button:");
            ui.text_edit_singleline(&mut bindings.b);
            ui.end_row();

            ui.label("Select:");
            ui.text_edit_singleline(&mut bindings.select);
            ui.end_row();

            ui.label("Start:");
            ui.text_edit_singleline(&mut bindings.start);
            ui.end_row();

            ui.label("Up:");
            ui.text_edit_singleline(&mut bindings.up);
            ui.end_row();

            ui.label("Down:");
            ui.text_edit_singleline(&mut bindings.down);
            ui.end_row();

            ui.label("Left:");
            ui.text_edit_singleline(&mut bindings.left);
            ui.end_row();

            ui.label("Right:");
            ui.text_edit_singleline(&mut bindings.right);
            ui.end_row();
        });
}

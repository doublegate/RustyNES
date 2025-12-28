//! Menu bar implementation.

use super::GuiState;
use crate::audio::AudioOutput;
use crate::config::Config;
use egui::Context;
use log::{error, info};
use rustynes_core::Console;

/// Render the main menu bar.
#[allow(clippy::too_many_lines, deprecated)]
pub fn render_menu_bar(
    ctx: &Context,
    state: &mut GuiState,
    config: &mut Config,
    console: &mut Option<Console>,
    audio: &Option<AudioOutput>,
    paused: &mut bool,
) {
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            // File menu
            ui.menu_button("File", |ui| {
                if ui.button("Open ROM...").clicked() {
                    state.file_dialog_pending = true;
                    open_file_dialog(console, config);
                    ui.close();
                }

                ui.separator();

                ui.menu_button("Recent ROMs", |ui| {
                    if config.recent_roms.paths.is_empty() {
                        ui.label("No recent ROMs");
                    } else {
                        for path in &config.recent_roms.paths.clone() {
                            let name = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("Unknown");
                            if ui.button(name).clicked() {
                                if let Err(e) = load_rom(path, console) {
                                    error!("Failed to load ROM: {e}");
                                }
                                ui.close();
                            }
                        }
                    }
                });

                ui.separator();

                if ui.button("Exit").clicked() {
                    std::process::exit(0);
                }
            });

            // Emulation menu
            ui.menu_button("Emulation", |ui| {
                let has_rom = console.is_some();

                if ui
                    .add_enabled(
                        has_rom,
                        egui::Button::new(if *paused { "Resume" } else { "Pause" }),
                    )
                    .clicked()
                {
                    *paused = !*paused;
                    ui.close();
                }

                if ui
                    .add_enabled(has_rom, egui::Button::new("Reset"))
                    .clicked()
                {
                    if let Some(cons) = console {
                        cons.reset();
                        info!("Console reset");
                    }
                    ui.close();
                }
            });

            // Options menu
            ui.menu_button("Options", |ui| {
                // Video submenu
                ui.menu_button("Video", |ui| {
                    ui.checkbox(&mut config.video.fullscreen, "Fullscreen");
                    ui.checkbox(&mut config.video.vsync, "VSync");
                    ui.checkbox(
                        &mut config.video.pixel_aspect_correction,
                        "8:7 Aspect Ratio",
                    );
                    ui.checkbox(&mut config.video.show_fps, "Show FPS");

                    ui.separator();

                    ui.label("Scale:");
                    for scale in 1..=6 {
                        if ui
                            .radio_value(&mut config.video.scale, scale, format!("{scale}x"))
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                });

                // Audio submenu
                ui.menu_button("Audio", |ui| {
                    let is_muted = audio.as_ref().map_or(
                        config.audio.muted,
                        super::super::audio::AudioOutput::is_muted,
                    );
                    let mut muted = is_muted;
                    if ui.checkbox(&mut muted, "Mute").changed() {
                        if let Some(audio) = audio {
                            audio.set_muted(muted);
                        }
                        config.audio.muted = muted;
                    }

                    ui.separator();

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

                ui.separator();

                if ui.button("Settings...").clicked() {
                    state.settings_open = true;
                    ui.close();
                }
            });

            // Debug menu
            ui.menu_button("Debug", |ui| {
                ui.checkbox(&mut config.debug.enabled, "Enable Debug Mode");

                ui.separator();

                ui.add_enabled(
                    config.debug.enabled,
                    egui::Checkbox::new(&mut state.debug.cpu, "CPU Window"),
                );
                ui.add_enabled(
                    config.debug.enabled,
                    egui::Checkbox::new(&mut state.debug.ppu, "PPU Window"),
                );
                ui.add_enabled(
                    config.debug.enabled,
                    egui::Checkbox::new(&mut state.debug.apu, "APU Window"),
                );
                ui.add_enabled(
                    config.debug.enabled,
                    egui::Checkbox::new(&mut state.debug.memory, "Memory Viewer"),
                );
            });

            // Help menu
            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard Shortcuts").clicked() {
                    // TODO: Show keyboard shortcuts window
                    ui.close();
                }

                ui.separator();

                if ui.button("About").clicked() {
                    state.about_open = true;
                    ui.close();
                }
            });
        });
    });
}

/// Open a file dialog to select a ROM.
fn open_file_dialog(console: &mut Option<Console>, config: &mut Config) {
    let file = rfd::FileDialog::new()
        .add_filter("NES ROMs", &["nes", "NES"])
        .add_filter("All Files", &["*"])
        .pick_file();

    if let Some(path) = file {
        if let Err(e) = load_rom(&path, console) {
            error!("Failed to load ROM: {e}");
        } else {
            config.recent_roms.add(path);
        }
    }
}

/// Load a ROM file into the console.
fn load_rom(path: &std::path::Path, console: &mut Option<Console>) -> anyhow::Result<()> {
    let rom_data = std::fs::read(path)?;
    let cons = Console::from_rom_bytes(&rom_data)?;
    *console = Some(cons);
    info!("Loaded ROM: {}", path.display());
    Ok(())
}

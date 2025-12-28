//! Menu bar implementation with keyboard shortcuts.

use super::{GuiState, StatusMessage};
use crate::audio::AudioOutput;
use crate::config::Config;
use egui::Context;
use log::{error, info};
use rustynes_core::Console;

/// Render the main menu bar.
#[allow(clippy::too_many_lines)]
pub fn render_menu_bar(
    ctx: &Context,
    state: &mut GuiState,
    config: &mut Config,
    console: &mut Option<Console>,
    audio: &Option<AudioOutput>,
    paused: &mut bool,
) {
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            // File menu
            ui.menu_button("File", |ui| {
                if ui
                    .add(egui::Button::new("Open ROM...").shortcut_text("Ctrl+O"))
                    .on_hover_text("Open a NES ROM file")
                    .clicked()
                {
                    state.file_dialog_pending = true;
                    open_file_dialog(console, config, state);
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
                                    state.set_error(format!("Failed to load ROM: {e}"));
                                } else {
                                    state.rom_name = Some(name.to_string());
                                    state.set_status(StatusMessage::success(format!(
                                        "Loaded: {name}"
                                    )));
                                }
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui
                            .button("Clear Recent")
                            .on_hover_text("Clear the recent ROMs list")
                            .clicked()
                        {
                            config.recent_roms.paths.clear();
                            ui.close();
                        }
                    }
                });

                ui.separator();

                if ui
                    .add(egui::Button::new("Exit").shortcut_text("Ctrl+Q"))
                    .on_hover_text("Exit RustyNES")
                    .clicked()
                {
                    std::process::exit(0);
                }
            });

            // Emulation menu
            ui.menu_button("Emulation", |ui| {
                let has_rom = console.is_some();

                if ui
                    .add_enabled(
                        has_rom,
                        egui::Button::new(if *paused { "Resume" } else { "Pause" })
                            .shortcut_text("Ctrl+P"),
                    )
                    .on_hover_text(if *paused {
                        "Resume emulation"
                    } else {
                        "Pause emulation"
                    })
                    .clicked()
                {
                    *paused = !*paused;
                    state.set_status(StatusMessage::info(if *paused {
                        "Emulation paused"
                    } else {
                        "Emulation resumed"
                    }));
                    ui.close();
                }

                if ui
                    .add_enabled(has_rom, egui::Button::new("Reset").shortcut_text("Ctrl+R"))
                    .on_hover_text("Reset the console (soft reset)")
                    .clicked()
                {
                    if let Some(cons) = console {
                        cons.reset();
                        state.set_status(StatusMessage::info("Console reset"));
                        info!("Console reset");
                    }
                    ui.close();
                }
            });

            // Options menu
            ui.menu_button("Options", |ui| {
                // Video submenu
                ui.menu_button("Video", |ui| {
                    ui.checkbox(&mut config.video.fullscreen, "Fullscreen")
                        .on_hover_text("Toggle fullscreen mode");
                    ui.checkbox(&mut config.video.vsync, "VSync")
                        .on_hover_text("Enable vertical sync");
                    ui.checkbox(
                        &mut config.video.pixel_aspect_correction,
                        "8:7 Aspect Ratio",
                    )
                    .on_hover_text("Apply NES native pixel aspect ratio");
                    ui.checkbox(&mut config.video.show_fps, "Show FPS")
                        .on_hover_text("Show FPS overlay");

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
                    let is_muted = audio
                        .as_ref()
                        .map_or(config.audio.muted, crate::audio::AudioOutput::is_muted);
                    let mut muted = is_muted;
                    if ui
                        .checkbox(&mut muted, "Mute")
                        .on_hover_text("Toggle audio mute")
                        .changed()
                    {
                        if let Some(audio) = audio {
                            audio.set_muted(muted);
                        }
                        config.audio.muted = muted;
                    }

                    ui.separator();

                    ui.label("Volume:");
                    let mut volume = audio
                        .as_ref()
                        .map_or(config.audio.volume, crate::audio::AudioOutput::volume);
                    if ui
                        .add(egui::Slider::new(&mut volume, 0.0..=1.0).show_value(true))
                        .on_hover_text("Adjust volume")
                        .changed()
                    {
                        if let Some(audio) = audio {
                            audio.set_volume(volume);
                        }
                        config.audio.volume = volume;
                    }
                });

                ui.separator();

                if ui
                    .add(egui::Button::new("Settings...").shortcut_text("Ctrl+,"))
                    .on_hover_text("Open settings dialog")
                    .clicked()
                {
                    state.settings_open = true;
                    ui.close();
                }
            });

            // Debug menu
            ui.menu_button("Debug", |ui| {
                ui.checkbox(&mut config.debug.enabled, "Enable Debug Mode")
                    .on_hover_text("Toggle debug mode (F1)");

                ui.separator();

                ui.add_enabled(
                    config.debug.enabled,
                    egui::Checkbox::new(&mut state.debug.cpu, "CPU Window"),
                )
                .on_hover_text("Show CPU debug window");
                ui.add_enabled(
                    config.debug.enabled,
                    egui::Checkbox::new(&mut state.debug.ppu, "PPU Window"),
                )
                .on_hover_text("Show PPU debug window");
                ui.add_enabled(
                    config.debug.enabled,
                    egui::Checkbox::new(&mut state.debug.apu, "APU Window"),
                )
                .on_hover_text("Show APU debug window");
                ui.add_enabled(
                    config.debug.enabled,
                    egui::Checkbox::new(&mut state.debug.memory, "Memory Viewer"),
                )
                .on_hover_text("Show memory viewer");
            });

            // Help menu
            ui.menu_button("Help", |ui| {
                if ui
                    .button("Keyboard Shortcuts")
                    .on_hover_text("View keyboard shortcuts")
                    .clicked()
                {
                    state.show_shortcuts = true;
                    ui.close();
                }

                ui.separator();

                if ui.button("About").on_hover_text("About RustyNES").clicked() {
                    state.about_open = true;
                    ui.close();
                }
            });
        });
    });
}

/// Open a file dialog to select a ROM.
fn open_file_dialog(console: &mut Option<Console>, config: &mut Config, state: &mut GuiState) {
    let file = rfd::FileDialog::new()
        .add_filter("NES ROMs", &["nes", "NES"])
        .add_filter("All Files", &["*"])
        .pick_file();

    if let Some(path) = file {
        let rom_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        if let Err(e) = load_rom(&path, console) {
            state.set_error(format!("Failed to load ROM: {e}"));
            error!("Failed to load ROM: {e}");
        } else {
            config.recent_roms.add(path);
            state.rom_name = Some(rom_name.clone());
            state.set_status(StatusMessage::success(format!("Loaded: {rom_name}")));
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

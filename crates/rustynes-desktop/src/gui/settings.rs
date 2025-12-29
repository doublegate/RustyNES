//! Settings dialog implementation with tabbed layout.

use super::{GuiState, SettingsTab};
use crate::audio::AudioOutput;
use crate::config::{AppTheme, Config};
use egui::Context;

/// Render the settings window with tabbed layout.
#[allow(clippy::too_many_lines)]
pub fn render_settings(
    ctx: &Context,
    state: &mut GuiState,
    config: &mut Config,
    audio: Option<&AudioOutput>,
) {
    egui::Window::new("Settings")
        .open(&mut state.settings_open)
        .resizable(true)
        .default_width(450.0)
        .min_width(400.0)
        .show(ctx, |ui| {
            // Tab bar
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.settings_tab, SettingsTab::Video, "Video");
                ui.selectable_value(&mut state.settings_tab, SettingsTab::Audio, "Audio");
                ui.selectable_value(&mut state.settings_tab, SettingsTab::Input, "Input");
                ui.selectable_value(&mut state.settings_tab, SettingsTab::Advanced, "Advanced");
            });

            ui.separator();

            // Tab content
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match state.settings_tab {
                    SettingsTab::Video => render_video_settings(ui, config),
                    SettingsTab::Audio => render_audio_settings(ui, config, audio),
                    SettingsTab::Input => render_input_settings(ui, config),
                    SettingsTab::Advanced => render_advanced_settings(ui, config),
                });

            ui.separator();

            // Action buttons
            ui.horizontal(|ui| {
                if ui
                    .button("Save")
                    .on_hover_text("Save settings to disk")
                    .clicked()
                {
                    if let Err(e) = config.save() {
                        log::error!("Failed to save config: {e}");
                    } else {
                        log::info!("Settings saved");
                    }
                }

                if ui
                    .button("Reset to Defaults")
                    .on_hover_text("Reset all settings to their default values")
                    .clicked()
                {
                    *config = Config::default();
                }
            });
        });
}

/// Render video settings tab.
fn render_video_settings(ui: &mut egui::Ui, config: &mut Config) {
    ui.heading("Video Settings");
    ui.add_space(8.0);

    // Theme selection
    ui.horizontal(|ui| {
        ui.label("Theme:");
        egui::ComboBox::from_id_salt("theme")
            .selected_text(config.video.theme.display_name())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut config.video.theme, AppTheme::Light, "Light")
                    .on_hover_text("Light theme with bright backgrounds");
                ui.selectable_value(&mut config.video.theme, AppTheme::Dark, "Dark")
                    .on_hover_text("Dark theme (default)");
                ui.selectable_value(&mut config.video.theme, AppTheme::System, "System")
                    .on_hover_text("Follow system theme preference");
            });
    });

    ui.add_space(8.0);

    // Window scale
    ui.horizontal(|ui| {
        ui.label("Window Scale:");
        egui::ComboBox::from_id_salt("scale")
            .selected_text(format!("{}x", config.video.scale))
            .show_ui(ui, |ui| {
                for scale in 1..=8 {
                    ui.selectable_value(&mut config.video.scale, scale, format!("{scale}x"));
                }
            });
    })
    .response
    .on_hover_text("Scale the NES display (256x240) by this factor");

    ui.add_space(8.0);

    // Checkboxes with tooltips
    ui.checkbox(&mut config.video.fullscreen, "Fullscreen")
        .on_hover_text("Start in fullscreen mode");

    ui.checkbox(&mut config.video.vsync, "VSync").on_hover_text(
        "Synchronize frame rate with monitor refresh rate to prevent screen tearing",
    );

    ui.checkbox(
        &mut config.video.pixel_aspect_correction,
        "8:7 Pixel Aspect Ratio (NES Native)",
    )
    .on_hover_text(
        "Apply the NES's native 8:7 pixel aspect ratio for accurate display proportions",
    );

    ui.checkbox(&mut config.video.show_fps, "Show FPS Counter")
        .on_hover_text("Display frames per second overlay");
}

/// Render audio settings tab.
fn render_audio_settings(ui: &mut egui::Ui, config: &mut Config, audio: Option<&AudioOutput>) {
    ui.heading("Audio Settings");
    ui.add_space(8.0);

    // Mute checkbox
    let mut muted = audio.map_or(config.audio.muted, crate::audio::AudioOutput::is_muted);
    if ui
        .checkbox(&mut muted, "Mute Audio")
        .on_hover_text("Disable all audio output")
        .changed()
    {
        if let Some(audio) = audio {
            audio.set_muted(muted);
        }
        config.audio.muted = muted;
    }

    ui.add_space(8.0);

    // Volume slider
    ui.horizontal(|ui| {
        ui.label("Volume:");
        let mut volume = audio.map_or(config.audio.volume, crate::audio::AudioOutput::volume);
        if ui
            .add(
                egui::Slider::new(&mut volume, 0.0..=1.0)
                    .show_value(true)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            )
            .on_hover_text("Master volume level")
            .changed()
        {
            if let Some(audio) = audio {
                audio.set_volume(volume);
            }
            config.audio.volume = volume;
        }
    });

    ui.add_space(8.0);

    // Sample rate
    ui.horizontal(|ui| {
        ui.label("Sample Rate:");
        egui::ComboBox::from_id_salt("sample_rate")
            .selected_text(format!("{} Hz", config.audio.sample_rate))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut config.audio.sample_rate, 44100, "44100 Hz")
                    .on_hover_text("Standard CD quality");
                ui.selectable_value(&mut config.audio.sample_rate, 48000, "48000 Hz")
                    .on_hover_text("Standard professional audio quality");
                ui.selectable_value(&mut config.audio.sample_rate, 96000, "96000 Hz")
                    .on_hover_text("High-resolution audio");
            });
    })
    .response
    .on_hover_text("Audio output sample rate (requires restart)");

    ui.add_space(8.0);

    // Buffer size
    ui.horizontal(|ui| {
        ui.label("Buffer Size:");
        egui::ComboBox::from_id_salt("buffer_size")
            .selected_text(format!("{} samples", config.audio.buffer_size))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut config.audio.buffer_size, 512, "512 (Low latency)")
                    .on_hover_text("Lowest latency, may cause crackling on slower systems");
                ui.selectable_value(&mut config.audio.buffer_size, 1024, "1024")
                    .on_hover_text("Good balance of latency and stability");
                ui.selectable_value(&mut config.audio.buffer_size, 2048, "2048 (Balanced)")
                    .on_hover_text("Default - good balance for most systems");
                ui.selectable_value(&mut config.audio.buffer_size, 4096, "4096 (High stability)")
                    .on_hover_text("Most stable, higher latency");
            });
    })
    .response
    .on_hover_text("Larger buffers reduce crackling but increase latency (requires restart)");
}

/// Render input settings tab.
fn render_input_settings(ui: &mut egui::Ui, config: &mut Config) {
    ui.heading("Input Settings");
    ui.add_space(8.0);

    // Player 1 bindings
    ui.collapsing("Player 1 - Keyboard", |ui| {
        render_key_bindings(ui, &mut config.input.player1_keyboard, "p1");
    });

    ui.add_space(8.0);

    // Player 2 bindings
    ui.collapsing("Player 2 - Keyboard", |ui| {
        render_key_bindings(ui, &mut config.input.player2_keyboard, "p2");
    });

    ui.add_space(16.0);

    // Gamepad info
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Gamepad Support").strong());
        ui.label("(via gilrs)");
    });
    ui.label("Gamepads are automatically detected when connected.");
    ui.label("Button mapping follows standard NES layout.");
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
            ui.text_edit_singleline(&mut bindings.a)
                .on_hover_text("Key for NES A button");
            ui.end_row();

            ui.label("B Button:");
            ui.text_edit_singleline(&mut bindings.b)
                .on_hover_text("Key for NES B button");
            ui.end_row();

            ui.label("Select:");
            ui.text_edit_singleline(&mut bindings.select)
                .on_hover_text("Key for NES Select button");
            ui.end_row();

            ui.label("Start:");
            ui.text_edit_singleline(&mut bindings.start)
                .on_hover_text("Key for NES Start button");
            ui.end_row();

            ui.label("Up:");
            ui.text_edit_singleline(&mut bindings.up)
                .on_hover_text("Key for D-Pad Up");
            ui.end_row();

            ui.label("Down:");
            ui.text_edit_singleline(&mut bindings.down)
                .on_hover_text("Key for D-Pad Down");
            ui.end_row();

            ui.label("Left:");
            ui.text_edit_singleline(&mut bindings.left)
                .on_hover_text("Key for D-Pad Left");
            ui.end_row();

            ui.label("Right:");
            ui.text_edit_singleline(&mut bindings.right)
                .on_hover_text("Key for D-Pad Right");
            ui.end_row();
        });

    ui.add_space(8.0);
    if ui
        .button("Reset to Defaults")
        .on_hover_text("Reset key bindings to default values")
        .clicked()
    {
        if id_prefix == "p1" {
            *bindings = crate::config::KeyboardBindings::player1_defaults();
        } else {
            *bindings = crate::config::KeyboardBindings::player2_defaults();
        }
    }
}

/// Render advanced/debug settings tab.
fn render_advanced_settings(ui: &mut egui::Ui, config: &mut Config) {
    ui.heading("Advanced Settings");
    ui.add_space(8.0);

    // Debug section
    ui.label(egui::RichText::new("Debug Options").strong());
    ui.add_space(4.0);

    ui.checkbox(&mut config.debug.enabled, "Enable Debug Mode")
        .on_hover_text("Enable debug windows and overlays");

    ui.add_enabled(
        config.debug.enabled,
        egui::Checkbox::new(&mut config.debug.show_cpu, "Show CPU Window on Start"),
    )
    .on_hover_text("Automatically open CPU debug window when starting");

    ui.add_enabled(
        config.debug.enabled,
        egui::Checkbox::new(&mut config.debug.show_ppu, "Show PPU Window on Start"),
    )
    .on_hover_text("Automatically open PPU debug window when starting");

    ui.add_enabled(
        config.debug.enabled,
        egui::Checkbox::new(&mut config.debug.show_apu, "Show APU Window on Start"),
    )
    .on_hover_text("Automatically open APU debug window when starting");

    ui.add_enabled(
        config.debug.enabled,
        egui::Checkbox::new(&mut config.debug.show_memory, "Show Memory Viewer on Start"),
    )
    .on_hover_text("Automatically open memory viewer when starting");

    ui.add_space(16.0);

    // Recent ROMs management
    ui.label(egui::RichText::new("Recent ROMs").strong());
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label(format!(
            "{} ROM(s) in recent list",
            config.recent_roms.paths.len()
        ));
        if ui
            .button("Clear Recent ROMs")
            .on_hover_text("Remove all ROMs from the recent list")
            .clicked()
        {
            config.recent_roms.paths.clear();
        }
    });

    ui.add_space(16.0);

    // Version info
    ui.label(egui::RichText::new("Application Info").strong());
    ui.add_space(4.0);
    ui.label("Version: 0.8.4");
    ui.label("Rust Edition: 2024");
    ui.hyperlink_to(
        "GitHub Repository",
        "https://github.com/doublegate/RustyNES",
    );
}

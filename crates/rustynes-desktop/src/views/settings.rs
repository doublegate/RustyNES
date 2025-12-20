//! Settings view with tabbed interface for configuration.

use iced::widget::{button, checkbox, column, container, pick_list, row, slider, text};
use iced::{Alignment, Element, Length};

use crate::config::{AppConfig, CrtPreset, Region, ScalingMode};
use crate::message::Message;
use crate::theme::ThemeVariant;
use crate::view::SettingsTab;

/// Render settings view
pub fn view(config: &AppConfig, current_tab: SettingsTab) -> Element<'_, Message> {
    let tabs = row![
        tab_button(current_tab, SettingsTab::Emulation),
        tab_button(current_tab, SettingsTab::Video),
        tab_button(current_tab, SettingsTab::Audio),
        tab_button(current_tab, SettingsTab::Input),
    ]
    .spacing(10)
    .padding(10);

    // Add theme selector at the top
    let theme_selector = container(
        row![
            text("Theme:").width(Length::Fixed(80.0)),
            pick_list(
                ThemeVariant::all(),
                Some(config.app.theme),
                Message::UpdateTheme
            )
            .width(Length::Fixed(150.0)),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .padding(10),
    );

    let content = match current_tab {
        SettingsTab::Emulation => emulation_settings(config),
        SettingsTab::Video => video_settings(config),
        SettingsTab::Audio => audio_settings(config),
        SettingsTab::Input => input_settings(config),
    };

    let buttons = row![
        button("Reset to Defaults").on_press(Message::ResetSettingsToDefaults),
        iced::widget::Space::with_width(Length::Fill),
        button("Close").on_press(Message::CloseSettings),
    ]
    .spacing(10)
    .padding(10);

    container(
        column![
            theme_selector,
            iced::widget::horizontal_rule(1),
            tabs,
            iced::widget::horizontal_rule(1),
            container(content)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(20),
            iced::widget::horizontal_rule(1),
            buttons,
        ]
        .width(Length::Fixed(700.0))
        .height(Length::Fixed(650.0)),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

/// Render tab button
fn tab_button(selected: SettingsTab, tab: SettingsTab) -> iced::widget::Button<'static, Message> {
    let style = if selected == tab {
        iced::widget::button::primary
    } else {
        iced::widget::button::secondary
    };

    button(text(tab.to_string()))
        .style(style)
        .on_press(Message::SelectSettingsTab(tab))
}

/// Render emulation settings tab
fn emulation_settings(config: &AppConfig) -> Element<'_, Message> {
    column![
        text("Emulation Settings").size(20),
        iced::widget::vertical_space().height(20),
        // Speed
        row![
            text("Speed:").width(Length::Fixed(150.0)),
            slider(
                0.25..=3.0,
                config.emulation.speed,
                Message::UpdateEmulationSpeed
            )
            .step(0.25),
            text(format!("{:.2}x", config.emulation.speed)).width(Length::Fixed(60.0)),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        // Region
        row![
            text("Region:").width(Length::Fixed(150.0)),
            pick_list(
                &[Region::NTSC, Region::PAL][..],
                Some(config.emulation.region),
                Message::UpdateRegion
            ),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        iced::widget::vertical_space().height(20),
        // Rewind
        checkbox("Enable Rewind", config.emulation.rewind_enabled).on_toggle(Message::ToggleRewind),
        // Rewind buffer size (only if enabled)
        if config.emulation.rewind_enabled {
            Element::from(
                row![
                    text("Buffer Size:").width(Length::Fixed(150.0)),
                    slider(
                        60.0..=3600.0,
                        config.emulation.rewind_buffer_size as f64,
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        |v| Message::UpdateRewindBufferSize(v as usize)
                    )
                    .step(60.0),
                    text(format!(
                        "{:.1}s",
                        config.emulation.rewind_buffer_size as f32 / 60.0
                    ))
                    .width(Length::Fixed(60.0)),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
        } else {
            Element::from(iced::widget::Space::new(0, 0))
        },
    ]
    .spacing(15)
    .into()
}

/// Render video settings tab
fn video_settings(config: &AppConfig) -> Element<'_, Message> {
    column![
        text("Video Settings").size(20),
        iced::widget::vertical_space().height(20),
        // Scaling mode
        row![
            text("Scaling Mode:").width(Length::Fixed(150.0)),
            pick_list(
                &[
                    ScalingMode::AspectRatio4x3,
                    ScalingMode::PixelPerfect,
                    ScalingMode::IntegerScaling,
                    ScalingMode::Stretch,
                ][..],
                Some(config.video.scaling_mode),
                Message::UpdateScalingMode
            ),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        // VSync
        checkbox("VSync", config.video.vsync).on_toggle(Message::ToggleVSync),
        iced::widget::vertical_space().height(20),
        // CRT Shader
        checkbox("CRT Shader", config.video.crt_shader).on_toggle(Message::ToggleCrtShader),
        // CRT preset (only if shader enabled)
        if config.video.crt_shader {
            Element::from(
                row![
                    text("CRT Preset:").width(Length::Fixed(150.0)),
                    pick_list(
                        &[CrtPreset::Subtle, CrtPreset::Moderate, CrtPreset::Authentic,][..],
                        Some(config.video.crt_preset),
                        Message::UpdateCrtPreset
                    ),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
        } else {
            Element::from(iced::widget::Space::new(0, 0))
        },
        iced::widget::vertical_space().height(20),
        text("Overscan Cropping").size(16),
        // Overscan sliders
        row![
            text("Top:").width(Length::Fixed(80.0)),
            slider(0..=16, config.video.overscan.top, |v| {
                Message::UpdateOverscanTop(v)
            })
            .step(1u32),
            text(format!("{}px", config.video.overscan.top)).width(Length::Fixed(50.0)),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        row![
            text("Bottom:").width(Length::Fixed(80.0)),
            slider(0..=16, config.video.overscan.bottom, |v| {
                Message::UpdateOverscanBottom(v)
            })
            .step(1u32),
            text(format!("{}px", config.video.overscan.bottom)).width(Length::Fixed(50.0)),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        row![
            text("Left:").width(Length::Fixed(80.0)),
            slider(0..=16, config.video.overscan.left, |v| {
                Message::UpdateOverscanLeft(v)
            })
            .step(1u32),
            text(format!("{}px", config.video.overscan.left)).width(Length::Fixed(50.0)),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        row![
            text("Right:").width(Length::Fixed(80.0)),
            slider(0..=16, config.video.overscan.right, |v| {
                Message::UpdateOverscanRight(v)
            })
            .step(1u32),
            text(format!("{}px", config.video.overscan.right)).width(Length::Fixed(50.0)),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    ]
    .spacing(15)
    .into()
}

/// Render audio settings tab
fn audio_settings(config: &AppConfig) -> Element<'_, Message> {
    column![
        text("Audio Settings").size(20),
        iced::widget::vertical_space().height(20),
        // Audio enabled
        checkbox("Audio Output", config.audio.enabled).on_toggle(Message::ToggleAudio),
        // Sample rate
        row![
            text("Sample Rate:").width(Length::Fixed(150.0)),
            pick_list(
                &[44100, 48000, 96000][..],
                Some(config.audio.sample_rate),
                Message::UpdateSampleRate
            ),
            text("Hz"),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        // Volume
        row![
            text("Master Volume:").width(Length::Fixed(150.0)),
            slider(0.0..=1.0, config.audio.volume, Message::UpdateVolume).step(0.01),
            text(format!("{:.0}%", config.audio.volume * 100.0)).width(Length::Fixed(60.0)),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        // Buffer size
        row![
            text("Buffer Size:").width(Length::Fixed(150.0)),
            pick_list(
                &[512, 1024, 2048, 4096][..],
                Some(config.audio.buffer_size),
                Message::UpdateBufferSize
            ),
            text("samples"),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        iced::widget::vertical_space().height(20),
        text("Lower buffer size = lower latency, higher CPU usage")
            .size(12)
            .color(iced::Color::from_rgb(0.6, 0.6, 0.6)),
    ]
    .spacing(15)
    .into()
}

/// Render input settings tab
fn input_settings(config: &AppConfig) -> Element<'_, Message> {
    column![
        text("Input Settings").size(20),
        iced::widget::vertical_space().height(20),
        text("Player 1 Keyboard").size(16),
        key_mapping_grid(&config.input.keyboard_p1, 1),
        iced::widget::vertical_space().height(20),
        text("Player 2 Keyboard").size(16),
        key_mapping_grid(&config.input.keyboard_p2, 2),
        iced::widget::vertical_space().height(20),
        text("Gamepad").size(16),
        row![
            text("Analog Deadzone:").width(Length::Fixed(150.0)),
            slider(
                0.0..=0.5,
                config.input.gamepad_deadzone,
                Message::UpdateGamepadDeadzone
            )
            .step(0.05),
            text(format!("{:.2}", config.input.gamepad_deadzone)).width(Length::Fixed(60.0)),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    ]
    .spacing(15)
    .into()
}

/// Render keyboard mapping grid
fn key_mapping_grid(mapping: &crate::config::KeyboardMapping, player: u8) -> Element<'_, Message> {
    column![
        key_mapping_row("Up", &mapping.up, player, "up"),
        key_mapping_row("Down", &mapping.down, player, "down"),
        key_mapping_row("Left", &mapping.left, player, "left"),
        key_mapping_row("Right", &mapping.right, player, "right"),
        key_mapping_row("A", &mapping.a, player, "a"),
        key_mapping_row("B", &mapping.b, player, "b"),
        key_mapping_row("Select", &mapping.select, player, "select"),
        key_mapping_row("Start", &mapping.start, player, "start"),
    ]
    .spacing(8)
    .into()
}

/// Render keyboard mapping row
fn key_mapping_row<'a>(
    label: &'a str,
    current_key: &'a str,
    player: u8,
    button_name: &'static str,
) -> Element<'a, Message> {
    row![
        text(format!("{label}:")).width(Length::Fixed(80.0)),
        button(text(current_key))
            .on_press(Message::RemapKey {
                player,
                button: button_name.to_string()
            })
            .width(Length::Fixed(120.0)),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

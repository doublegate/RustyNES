//! Performance metrics tracking and overlay UI.
//!
//! Provides FPS monitoring, frame timing, and performance overlay
//! that can be toggled with F3 key.

use std::collections::VecDeque;
use std::time::Instant;

use iced::widget::{column, container, text};
use iced::{Color, Element, Length};

use crate::message::Message;

/// Performance metrics tracker
#[derive(Debug)]
pub struct PerformanceMetrics {
    /// Frames per second
    fps: f32,

    /// Frame time in milliseconds
    frame_time_ms: f32,

    /// Input latency in milliseconds (estimated)
    input_latency_ms: f32,

    /// Run-ahead overhead in microseconds
    runahead_overhead_us: u64,

    /// Audio buffer fill percentage (0.0-1.0)
    audio_buffer_fill: f32,

    /// Frame timing history (last 60 frames)
    frame_times: VecDeque<f32>,

    /// Last frame timestamp
    last_frame: Instant,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            fps: 60.0,
            frame_time_ms: 16.67,
            input_latency_ms: 33.33, // Default: 2 frames without run-ahead
            runahead_overhead_us: 0,
            audio_buffer_fill: 0.5,
            frame_times: VecDeque::with_capacity(60),
            last_frame: Instant::now(),
        }
    }
}

impl PerformanceMetrics {
    /// Update metrics with new frame
    #[allow(dead_code)] // Will be used when emulation loop is active
    pub fn update_frame(&mut self, runahead_enabled: bool, runahead_overhead_us: u64) {
        let now = Instant::now();
        let frame_time = now.duration_since(self.last_frame).as_secs_f32() * 1000.0;
        self.last_frame = now;

        // Update frame time history
        self.frame_times.push_back(frame_time);
        if self.frame_times.len() > 60 {
            self.frame_times.pop_front();
        }

        // Calculate average frame time
        let avg_frame_time: f32 =
            self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32;
        self.frame_time_ms = avg_frame_time;

        // Calculate FPS
        #[allow(clippy::float_cmp)]
        if avg_frame_time != 0.0 {
            self.fps = 1000.0 / avg_frame_time;
        }

        // Update input latency based on run-ahead state
        self.input_latency_ms = if runahead_enabled {
            16.67 // 1 frame with run-ahead
        } else {
            33.33 // 2 frames without run-ahead
        };

        // Update run-ahead overhead
        self.runahead_overhead_us = runahead_overhead_us;
    }

    /// Update audio buffer fill percentage
    #[allow(dead_code)] // Will be used when audio is implemented
    pub fn update_audio_buffer(&mut self, fill: f32) {
        self.audio_buffer_fill = fill.clamp(0.0, 1.0);
    }

    /// Get FPS
    #[allow(dead_code)] // Available for future use
    pub fn fps(&self) -> f32 {
        self.fps
    }

    /// Get frame time in milliseconds
    #[allow(dead_code)] // Available for future use
    pub fn frame_time_ms(&self) -> f32 {
        self.frame_time_ms
    }

    /// Get input latency in milliseconds
    #[allow(dead_code)] // Available for future use
    pub fn input_latency_ms(&self) -> f32 {
        self.input_latency_ms
    }

    /// Get run-ahead overhead in microseconds
    #[allow(dead_code)] // Available for future use
    pub fn runahead_overhead_us(&self) -> u64 {
        self.runahead_overhead_us
    }

    /// Get audio buffer fill percentage
    #[allow(dead_code)] // Available for future use
    pub fn audio_buffer_fill(&self) -> f32 {
        self.audio_buffer_fill
    }

    /// Render metrics overlay
    pub fn view(&self, visible: bool) -> Element<'_, Message> {
        if !visible {
            return iced::widget::Space::new(Length::Shrink, Length::Shrink).into();
        }

        // Color code FPS based on performance
        let fps_color = if self.fps >= 58.0 {
            Color::from_rgb(0.0, 1.0, 0.0) // Green: good
        } else if self.fps >= 50.0 {
            Color::from_rgb(1.0, 1.0, 0.0) // Yellow: acceptable
        } else {
            Color::from_rgb(1.0, 0.0, 0.0) // Red: poor
        };

        container(
            column![
                text(format!("FPS: {:.1}", self.fps))
                    .size(14)
                    .color(fps_color),
                text(format!("Frame: {:.2}ms", self.frame_time_ms)).size(14),
                text(format!("Latency: {:.2}ms", self.input_latency_ms)).size(14),
                text(format!("Run-Ahead: {}μs", self.runahead_overhead_us)).size(14),
                text(format!(
                    "Audio Buffer: {:.0}%",
                    self.audio_buffer_fill * 100.0
                ))
                .size(14),
                text("F3: Toggle Overlay")
                    .size(12)
                    .color(Color::from_rgb(0.6, 0.6, 0.6)),
            ]
            .spacing(4)
            .padding(8),
        )
        .style(|_theme: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(Color::from_rgba(
                0.0, 0.0, 0.0, 0.7,
            ))),
            border: iced::Border {
                color: Color::from_rgb(0.3, 0.3, 0.3),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
    }
}

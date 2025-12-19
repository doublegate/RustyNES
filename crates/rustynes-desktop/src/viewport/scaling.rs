//! Viewport scaling modes for NES display.
//!
//! Provides multiple aspect ratio and scaling options to match
//! different display preferences and historical accuracy levels.

use iced::{Rectangle, Size};

/// Scaling modes for NES viewport
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Variants will be used in settings UI (M6-S4)
pub enum ScalingMode {
    /// 4:3 aspect ratio (classic CRT television)
    AspectRatio4x3,
    /// 8:7 pixel aspect ratio (authentic NES pixels)
    PixelPerfect,
    /// Integer scaling (2x, 3x, 4x, etc.) for sharp pixels
    IntegerScaling,
    /// Stretch to fill entire window
    Stretch,
}

impl std::fmt::Display for ScalingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PixelPerfect => write!(f, "Pixel Perfect (8:7)"),
            Self::AspectRatio4x3 => write!(f, "4:3 Aspect Ratio"),
            Self::IntegerScaling => write!(f, "Integer Scaling"),
            Self::Stretch => write!(f, "Stretch to Fill"),
        }
    }
}

/// Calculate viewport rectangle based on window size and scaling mode
///
/// # Arguments
/// * `window_size` - Available window space
/// * `mode` - Desired scaling mode
///
/// # Returns
/// Rectangle defining the viewport position and size within the window
#[allow(dead_code)] // Will be used when implementing manual viewport sizing
pub fn calculate_viewport(window_size: Size, mode: ScalingMode) -> Rectangle {
    const NES_WIDTH: f32 = 256.0;
    const NES_HEIGHT: f32 = 240.0;

    match mode {
        ScalingMode::AspectRatio4x3 => {
            // Maintain 4:3 aspect ratio (CRT television standard)
            let aspect = 4.0 / 3.0;
            let (width, height) = if window_size.width / window_size.height > aspect {
                // Window is wider than 4:3, constrain by height
                (window_size.height * aspect, window_size.height)
            } else {
                // Window is taller than 4:3, constrain by width
                (window_size.width, window_size.width / aspect)
            };

            Rectangle {
                x: (window_size.width - width) / 2.0,
                y: (window_size.height - height) / 2.0,
                width,
                height,
            }
        }

        ScalingMode::PixelPerfect => {
            // 8:7 pixel aspect ratio (authentic NES)
            // NES pixels are slightly wider than they are tall
            let pixel_aspect = 8.0 / 7.0;
            let aspect = (NES_WIDTH / NES_HEIGHT) * pixel_aspect;

            let (width, height) = if window_size.width / window_size.height > aspect {
                (window_size.height * aspect, window_size.height)
            } else {
                (window_size.width, window_size.width / aspect)
            };

            Rectangle {
                x: (window_size.width - width) / 2.0,
                y: (window_size.height - height) / 2.0,
                width,
                height,
            }
        }

        ScalingMode::IntegerScaling => {
            // Integer multiples only (2x, 3x, 4x, etc.)
            // Ensures perfectly sharp pixels with no sub-pixel rendering
            let max_scale_x = (window_size.width / NES_WIDTH).floor();
            let max_scale_y = (window_size.height / NES_HEIGHT).floor();
            let scale = max_scale_x.min(max_scale_y).max(1.0);

            let width = NES_WIDTH * scale;
            let height = NES_HEIGHT * scale;

            Rectangle {
                x: (window_size.width - width) / 2.0,
                y: (window_size.height - height) / 2.0,
                width,
                height,
            }
        }

        ScalingMode::Stretch => {
            // Fill entire window (may distort aspect ratio)
            Rectangle {
                x: 0.0,
                y: 0.0,
                width: window_size.width,
                height: window_size.height,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tolerance for floating-point comparisons
    const EPSILON: f32 = 0.001;

    /// Helper to compare floats with tolerance
    fn assert_float_eq(actual: f32, expected: f32, msg: &str) {
        assert!(
            (actual - expected).abs() < EPSILON,
            "{msg}: expected {expected}, got {actual}"
        );
    }

    #[test]
    fn test_stretch_mode() {
        let window = Size::new(800.0, 600.0);
        let viewport = calculate_viewport(window, ScalingMode::Stretch);

        assert_float_eq(viewport.x, 0.0, "x position");
        assert_float_eq(viewport.y, 0.0, "y position");
        assert_float_eq(viewport.width, 800.0, "width");
        assert_float_eq(viewport.height, 600.0, "height");
    }

    #[test]
    fn test_integer_scaling() {
        let window = Size::new(800.0, 600.0);
        let viewport = calculate_viewport(window, ScalingMode::IntegerScaling);

        // 800 / 256 = 3.125, floor = 3
        // 600 / 240 = 2.5, floor = 2
        // Scale = min(3, 2) = 2
        assert_float_eq(viewport.width, 256.0 * 2.0, "width");
        assert_float_eq(viewport.height, 240.0 * 2.0, "height");
    }

    #[test]
    fn test_centered_viewport() {
        let window = Size::new(1000.0, 1000.0);
        let viewport = calculate_viewport(window, ScalingMode::IntegerScaling);

        // Viewport should be centered
        assert!(viewport.x > 0.0);
        assert!(viewport.y > 0.0);
        assert_float_eq(viewport.x, (1000.0 - viewport.width) / 2.0, "centered x");
        assert_float_eq(viewport.y, (1000.0 - viewport.height) / 2.0, "centered y");
    }
}

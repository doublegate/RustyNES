//! NES game viewport with wgpu rendering.
//!
//! This module provides a custom Iced widget for rendering NES frames
//! with pixel-perfect scaling and multiple aspect ratio modes.

mod scaling;
mod texture;
mod widget;

#[allow(unused_imports)]
pub use scaling::calculate_viewport;
pub use scaling::ScalingMode;
#[allow(unused_imports)]
pub use texture::{NesSampler, NesTexture};
pub use widget::GameViewport;

//! Application icon (v1.1.0).
//!
//! The icon PNG is embedded at build time and decoded for two consumers: the
//! winit window icon (taskbar / title bar) and the in-app About dialog.
//! Native-only — `png` lives in the `cfg(not(wasm))` dependency table, and a
//! browser tab has no window icon. The icon is purely cosmetic, so every step
//! degrades to `None` on failure rather than panicking.

/// The 256x256 app icon, embedded from `assets/RustyNES_Icon/`.
const ICON_PNG: &[u8] = include_bytes!("../../../assets/RustyNES_Icon/icon-256.png");

/// Decode the embedded icon to `(rgba8, width, height)`. `None` on any failure.
fn decode() -> Option<(Vec<u8>, u32, u32)> {
    let decoder = png::Decoder::new(ICON_PNG);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 0xFF])
            .collect(),
        // The shipped icon is 8-bit RGBA; anything else is unexpected.
        _ => return None,
    };
    Some((rgba, info.width, info.height))
}

/// The winit window icon, or `None` if decoding fails.
pub fn window_icon() -> Option<winit::window::Icon> {
    let (rgba, w, h) = decode()?;
    winit::window::Icon::from_rgba(rgba, w, h).ok()
}

/// The icon as an egui [`egui::ColorImage`] (straight alpha preserved, so the
/// rounded corners stay transparent over the dialog) for the About window.
pub fn about_color_image() -> Option<egui::ColorImage> {
    let (rgba, w, h) = decode()?;
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        &rgba,
    ))
}

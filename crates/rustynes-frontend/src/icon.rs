//! Application icons (v1.1.0; split in v3.7.1).
//!
//! Two PNGs are embedded at build time, one per consumer:
//!
//! * the winit **window icon** (taskbar / title bar on Windows and Linux;
//!   macOS ignores it and takes the bundle's `.icns`): the "RN" cartridge
//!   monogram from `assets/RustyNES_Icon_Pack/`, the art every platform's
//!   launcher, installer and web icon uses since v3.7.1;
//! * the in-app **About** dialog: the earlier gear-and-peripherals artwork
//!   (`assets/RustyNES_Icon/`, the controller in a gear with the console,
//!   cartridge, Zapper and R.O.B. in the corners), which the maintainer
//!   kept for that one place.
//!
//! Native-only — `png` lives in the `cfg(not(wasm))` dependency table, and a
//! browser tab has no window icon. The icons are purely cosmetic, so every
//! step degrades to `None` on failure rather than panicking.

/// The 256x256 window icon (transparent background), from the icon pack.
const WINDOW_ICON_PNG: &[u8] = include_bytes!(
    "../../../assets/RustyNES_Icon_Pack/transparent/icon/rustynes-icon-transparent-256.png"
);

/// The 256x256 About-dialog artwork, from `assets/RustyNES_Icon/`.
const ABOUT_ICON_PNG: &[u8] = include_bytes!("../../../assets/RustyNES_Icon/icon-256.png");

/// Decode an embedded icon to `(rgba8, width, height)`. `None` on any failure.
fn decode(png_bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    // png 0.18 requires the decoder source to be `Read + Seek`; `&[u8]` is only
    // `Read`, so wrap it in a `Cursor`.
    let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 0xFF])
            .collect(),
        // The shipped icon is 8-bit RGBA; anything else is unexpected.
        _ => return None,
    };
    Some((rgba, info.width, info.height))
}

/// The winit window icon, or `None` if decoding fails.
pub fn window_icon() -> Option<winit::window::Icon> {
    let (rgba, w, h) = decode(WINDOW_ICON_PNG)?;
    winit::window::Icon::from_rgba(rgba, w, h).ok()
}

/// The icon as an egui [`egui::ColorImage`] (straight alpha preserved, so the
/// rounded corners stay transparent over the dialog) for the About window.
pub fn about_color_image() -> Option<egui::ColorImage> {
    let (rgba, w, h) = decode(ABOUT_ICON_PNG)?;
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        &rgba,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both embedded icons decode (each is 8-bit RGBA, the one format
    /// `decode` takes, so a re-export in another depth would fall back to no
    /// icon at all), at 256x256, and they are DIFFERENT pictures: the window
    /// takes the icon pack's art, while the About dialog keeps the earlier art
    /// (v3.7.1, maintainer's choice). Pointing both at one file passes every
    /// other check.
    #[test]
    fn the_window_and_about_icons_are_both_decodable_and_distinct() {
        let (win, ww, wh) = decode(WINDOW_ICON_PNG).expect("window icon decodes");
        let (about, aw, ah) = decode(ABOUT_ICON_PNG).expect("About icon decodes");
        assert_eq!((ww, wh), (256, 256));
        assert_eq!((aw, ah), (256, 256));
        assert_ne!(win, about, "the window and About icons are different art");
        // The pack's icon has a transparent background (its corners), the
        // About art an opaque rounded tile with transparent corners too, so
        // check the window icon is the pack's by a corner AND its centre.
        assert_eq!(win[3], 0, "the window icon's corner is transparent");
        let centre = ((128 * 256 + 128) * 4 + 3) as usize;
        assert_eq!(win[centre], 0xFF, "the window icon's centre is opaque");
    }
}

//! `FontAwesome` menu icons (v1.2.0 Workstream H3).
//!
//! Adds Font Awesome 6 Free **Solid** glyphs in front of the menu-bar labels,
//! mirroring the `GeraNES` `withMenuIcon` model (an icon before each top menu and
//! its items). This is a pure cosmetic layer: the glyphs are font codepoints
//! prepended to existing labels, so if the font fails to register the labels
//! still render as plain text (egui falls back to the proportional font for any
//! glyph the icon font lacks) — it never crashes.
//!
//! ## Font asset
//!
//! The icon font is `assets/fonts/fa-solid-900.ttf` (Font Awesome 6 Free Solid
//! v6.5.2), embedded at build time via [`include_bytes!`]. Font Awesome Free's
//! desktop font files are licensed **SIL OFL-1.1** (see
//! `assets/fonts/LICENSE-FontAwesome.txt`); `deny.toml` already allows OFL-1.1.
//!
//! The full `.ttf` is ~410 KiB raw but is overwhelmingly composed of glyph
//! outlines we never reference; it compresses well and, measured against the
//! wasm deploy size-budget gate (`scripts/wasm_size_budget.sh`), fits inside the
//! 5 MiB budget on the browser build too — so the **same full font** ships on
//! native and both wasm flavours (no per-target subsetting needed). Should that
//! ever change, only [`FA_SOLID_TTF`] would become `cfg(target_arch = "wasm32")`
//! -gated to a trimmed subset; the glyph constants below are target-independent.
//!
//! ## Registering with egui
//!
//! [`install`] is called once after the egui [`egui::Context`] is created (in
//! `DebuggerOverlay::new`). It appends the font under a named family and wires
//! it as the **last** fallback for both the proportional and monospace families
//! so the regular UI text is unaffected and only the (private-use-area) icon
//! codepoints resolve to it.

/// Font Awesome 6 Free Solid (v6.5.2), embedded at build time.
///
/// OFL-1.1; license text ships alongside in
/// `assets/fonts/LICENSE-FontAwesome.txt`.
pub const FA_SOLID_TTF: &[u8] = include_bytes!("../assets/fonts/fa-solid-900.ttf");

/// The egui font-family key under which the icon font is registered.
const FA_FAMILY: &str = "fa-solid";

/// Font Awesome 6 Free **Solid** glyph codepoints used by the menu bar.
///
/// Each is the Unicode scalar value of a FA Solid icon (the same glyphs `GeraNES`
/// uses in `FontAwesomeIcons.h`). They live in the Unicode Private Use Area, so
/// they never collide with real text and degrade to a missing-glyph box (or
/// nothing) if the font is unavailable.
pub mod glyph {
    /// `file` — top-level File menu.
    pub const FILE: char = '\u{f15b}';
    /// `folder-open` — Open ROM / Import / recent entries.
    pub const FOLDER_OPEN: char = '\u{f07c}';
    /// `folder` — folder-based actions (Open Recent submenu).
    pub const FOLDER: char = '\u{f07b}';
    /// `clock-rotate-left` — Open Recent / history.
    pub const CLOCK_ROTATE_LEFT: char = '\u{f1da}';
    /// `floppy-disk` — save state / save-slot actions.
    pub const FLOPPY_DISK: char = '\u{f0c7}';
    /// `download` — load state / import.
    pub const DOWNLOAD: char = '\u{f019}';
    /// `camera`-like image; FA Solid `image` glyph for screenshots.
    pub const IMAGE: char = '\u{f03e}';
    /// `clipboard` — copy / logs / event viewer.
    pub const CLIPBOARD: char = '\u{f328}';
    /// `right-from-bracket` — Quit / exit.
    pub const RIGHT_FROM_BRACKET: char = '\u{f2f5}';
    /// `calculator` — top-level Emulation menu.
    pub const CALCULATOR: char = '\u{f1ec}';
    /// `pause` — pause/resume.
    pub const PAUSE: char = '\u{f04c}';
    /// `rotate-right` — reset / power-cycle.
    pub const ROTATE_RIGHT: char = '\u{f2f9}';
    /// `play` — frame advance / fast-forward / play.
    pub const PLAY: char = '\u{f04b}';
    /// `stop` — stop recording / stop playback.
    pub const STOP: char = '\u{f04d}';
    /// `forward`/sliders — run-ahead & speed sliders.
    pub const SLIDERS: char = '\u{f1de}';
    /// `globe` — region.
    pub const GLOBE: char = '\u{f0ac}';
    /// `coins`-like; FA Solid `circle-dollar`/coin for Vs. insert-coin.
    pub const COINS: char = '\u{f51e}';
    /// `wrench` — top-level Tools menu.
    pub const WRENCH: char = '\u{f0ad}';
    /// `wand-magic-sparkles` — cheats / improvements.
    pub const WAND_MAGIC_SPARKLES: char = '\u{e2ca}';
    /// `video` — TAS movies / replay.
    pub const VIDEO: char = '\u{f03d}';
    /// `wifi` — netplay.
    pub const WIFI: char = '\u{f1eb}';
    /// `trophy`-like; FA Solid `trophy` for `RetroAchievements`.
    pub const TROPHY: char = '\u{f091}';
    /// `gauge`/chart — performance monitor.
    pub const GAUGE: char = '\u{f624}';
    /// `gamepad` — input display / input section.
    pub const GAMEPAD: char = '\u{f11b}';
    /// `database` — ROM database.
    pub const DATABASE: char = '\u{f1c0}';
    /// `puzzle-piece` — Mod / HD-pack menu.
    pub const PUZZLE_PIECE: char = '\u{f12e}';
    /// `file-zipper` — ZIP pack.
    pub const FILE_ZIPPER: char = '\u{f1c6}';
    /// `xmark` — clear / unload / close.
    pub const XMARK: char = '\u{f00d}';
    /// `eye` — top-level View menu.
    pub const EYE: char = '\u{f06e}';
    /// `gear` — Settings / Advanced.
    pub const GEAR: char = '\u{f013}';
    /// `palette` — theme.
    pub const PALETTE: char = '\u{f53f}';
    /// `tv` — display / aspect / overscan / window size.
    pub const TV: char = '\u{f26c}';
    /// `expand` — fullscreen.
    pub const EXPAND: char = '\u{f065}';
    /// `bars` — show/hide menu bar.
    pub const BARS: char = '\u{f0c9}';
    /// `bug` — top-level Debug menu / CPU debugger.
    pub const BUG: char = '\u{f188}';
    /// `microchip` — PPU / chip viewers.
    pub const MICROCHIP: char = '\u{f2db}';
    /// `memory` — memory viewer.
    pub const MEMORY: char = '\u{f538}';
    /// `volume-high` — APU / audio.
    pub const VOLUME_HIGH: char = '\u{f028}';
    /// `music` — NSF player.
    pub const MUSIC: char = '\u{f001}';
    /// `code` — Lua scripting.
    pub const CODE: char = '\u{f121}';
    /// `circle-question` — top-level Help menu.
    pub const CIRCLE_QUESTION: char = '\u{f059}';
    /// `keyboard` — keyboard shortcuts.
    pub const KEYBOARD: char = '\u{f11c}';
    /// `circle-info` — About.
    pub const CIRCLE_INFO: char = '\u{f05a}';
}

/// Prefix a menu label with a Font Awesome glyph + a separating space.
///
/// The returned `String` is `"<glyph> <label>"`. When the font is missing the
/// glyph renders as a fallback box (or nothing), so the label text is always
/// legible — there is no crash path.
#[must_use]
pub fn label(icon: char, text: &str) -> String {
    // FA glyphs are multi-byte in UTF-8 (e.g. U+F15B = 3 bytes); size for the
    // glyph's real byte length + the separator space (gemini, PR #76).
    let mut s = String::with_capacity(text.len() + icon.len_utf8() + 1);
    s.push(icon);
    s.push(' ');
    s.push_str(text);
    s
}

/// Register the embedded Font Awesome Solid font with the egui context.
///
/// Idempotent-friendly: call once after the [`egui::Context`] is created. Adds
/// the font as the trailing fallback for the proportional + monospace families
/// so only the icon codepoints resolve to it and ordinary text is untouched.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        FA_FAMILY.to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(FA_SOLID_TTF)),
    );
    // Append (not prepend) so the default UI fonts keep priority for ordinary
    // text; the icon glyphs live in the PUA, which the default fonts lack, so
    // they fall through to this font.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push(FA_FAMILY.to_owned());
    }
    ctx.set_fonts(fonts);
}

//! v1.7.0 "Forge" Workstream H5 — frontend internationalization (i18n).
//!
//! `RustyNES`'s one systemic gap was that every user-facing string was a hard
//! literal. This module adds a *lightweight, compile-time string-catalog*
//! layer so the UI can be localized without touching the deterministic core.
//!
//! ## Design (see ADR 0023)
//!
//! - **Compile-time catalogs, no runtime I/O.** Every translation is a
//!   `&'static str` baked into the binary via plain `match` arms. There is no
//!   TOML/RON/Fluent parsing at startup, no embedded data file to load, and —
//!   critically for the wasm build — no `unic-langid` / `fluent` / ICU
//!   machinery to bloat the bundle past the 5 MiB Pages budget
//!   (`scripts/wasm_size_budget.sh`). A `&'static str` table compiles to read-
//!   only data the linker can dead-strip if unused; the whole layer costs a few
//!   KiB of string bytes. This is wasm-safe (`no_std`-shaped, no `std::fs`).
//! - **English is the default and the fallback.** [`Locale::English`] is the
//!   [`Default`], and [`tr`] falls back to the English arm for any key a
//!   non-English catalog has not translated yet. The English values are the
//!   *verbatim* strings the UI rendered before this module existed, so with the
//!   default locale every label is byte-identical to v1.6.0.
//! - **Incremental conversion.** Only the high-visibility surfaces (menu bar,
//!   Settings tabs/headers, status bar, common dialog buttons) are wired
//!   through [`tr`] in this change. Deeper panels keep their literals; the
//!   conversion pattern (replace `"Foo"` with `tr(Key::Foo)` and add the key to
//!   both catalogs) is documented in `docs/frontend.md` and applied gradually.
//!
//! ## Runtime selection
//!
//! The active locale is a process-global (`CURRENT_LOCALE`) seeded from the
//! `[ui] locale` config field at startup and updated by the Settings language
//! picker. egui re-renders the shell every frame, so a change takes effect on
//! the next frame with no explicit invalidation. Reads are a single relaxed
//! atomic load — cheap enough to call once per rendered string.

use core::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

/// The set of UI locales `RustyNES` ships catalogs for.
///
/// English is the default + fallback. Spanish (`es`) is included as a real
/// second locale to prove the mechanism end-to-end; further locales are added
/// by appending a variant here and a `match` arm to each catalog.
///
/// Serialized lowercase (`"english"` / `"spanish"`), so a hand-edited config or
/// an older config that omits the field both resolve correctly (the missing
/// field falls back to [`Locale::English`] via `#[serde(default)]` on the
/// `[ui] locale` config key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    /// English (United States). The default and the fallback for any key a
    /// non-English catalog has not translated.
    #[default]
    English,
    /// Spanish (Español).
    Spanish,
}

impl Locale {
    /// Human-readable, *native-language* label for the language picker (each
    /// language names itself, the convention for language selectors).
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Spanish => "Español",
        }
    }

    /// All locales in display order — single source of truth for the Settings
    /// language combo box so it never drifts from the enum.
    #[must_use]
    pub const fn all() -> [Self; 2] {
        [Self::English, Self::Spanish]
    }

    /// Stable numeric tag for the atomic global (round-trips through
    /// [`Locale::from_u8`]).
    #[must_use]
    const fn as_u8(self) -> u8 {
        match self {
            Self::English => 0,
            Self::Spanish => 1,
        }
    }

    /// Inverse of [`Locale::as_u8`]; any unknown byte falls back to English so a
    /// corrupted global can never panic.
    #[must_use]
    const fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Spanish,
            _ => Self::English,
        }
    }
}

/// Process-global active locale, stored as the [`Locale::as_u8`] tag.
///
/// Defaults to English (`0`) so any string resolved before [`set_locale`] runs
/// (e.g. a very early log line) is English — identical to the pre-i18n binary.
static CURRENT_LOCALE: AtomicU8 = AtomicU8::new(0);

/// Set the process-global active locale.
///
/// Called once at startup from the `[ui] locale` config value, and again
/// whenever the user picks a language in Settings. A relaxed store is
/// sufficient: egui reads it on the next frame and there is no cross-thread
/// ordering dependency on the value.
pub fn set_locale(locale: Locale) {
    CURRENT_LOCALE.store(locale.as_u8(), Ordering::Relaxed);
}

/// The current process-global active locale.
#[must_use]
pub fn current_locale() -> Locale {
    Locale::from_u8(CURRENT_LOCALE.load(Ordering::Relaxed))
}

/// Translatable string keys.
///
/// Each variant maps to a `&'static str` in every catalog. Adding a key means
/// adding an arm to the `english` catalog (the verbatim source string) and to
/// each non-English catalog (or letting it fall back to English until
/// translated).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Key {
    // ----- Menu bar: top-level menus -----
    /// "File" top menu.
    MenuFile,
    /// "Emulation" top menu.
    MenuEmulation,
    /// "Tools" top menu.
    MenuTools,
    /// "View" top menu.
    MenuView,
    /// "Debug" top menu.
    MenuDebug,
    /// "Help" top menu.
    MenuHelp,

    // ----- Menu bar: common items -----
    /// "Open ROM..." item.
    MenuOpenRom,
    /// "Open Recent" submenu.
    MenuOpenRecent,
    /// "No recent ROMs" placeholder.
    MenuNoRecentRoms,
    /// "Save States" submenu.
    MenuSaveStates,
    /// "Quit" item.
    MenuQuit,
    /// "Theme" submenu (View).
    MenuTheme,
    /// "Window Size" submenu (View).
    MenuWindowSize,

    // ----- Settings window: tabs + section headers -----
    /// Settings window title.
    SettingsTitle,
    /// "Video" settings tab.
    SettingsTabVideo,
    /// "Shaders" settings tab.
    SettingsTabShaders,
    /// "Audio" settings tab.
    SettingsTabAudio,
    /// "Input" settings tab.
    SettingsTabInput,
    /// "Emulation" settings tab.
    SettingsTabEmulation,
    /// "Display" section heading (Video tab).
    SettingsHeadingDisplay,
    /// "Accessibility" section heading (Video tab).
    SettingsHeadingAccessibility,
    /// "Theme:" label.
    SettingsTheme,
    /// "Language:" label (the i18n picker).
    SettingsLanguage,

    // ----- Status bar -----
    /// "No ROM loaded" status.
    StatusNoRom,
    /// "Idle" status.
    StatusIdle,
    /// "Running" status.
    StatusRunning,
    /// "Paused" status.
    StatusPaused,
    /// "Netplay" status.
    StatusNetplay,

    // ----- Common dialog buttons -----
    /// "OK" button.
    ButtonOk,
    /// "Cancel" button.
    ButtonCancel,
    /// "Save" button.
    ButtonSave,
    /// "Load" button.
    ButtonLoad,
    /// "Apply" button.
    ButtonApply,
    /// "Reset" button.
    ButtonReset,
}

/// English catalog — the verbatim source strings. This is also the fallback for
/// every other locale, so it must define a value for *every* [`Key`].
//
// `match_same_arms`: two distinct keys may currently share a translation (e.g.
// the "Emulation" menu and the "Emulation" settings tab), but they are
// independent strings that can diverge in another locale or a later edit, so
// each keeps its own arm rather than being merged.
#[allow(clippy::match_same_arms)]
const fn english(key: Key) -> &'static str {
    match key {
        Key::MenuFile => "File",
        Key::MenuEmulation => "Emulation",
        Key::MenuTools => "Tools",
        Key::MenuView => "View",
        Key::MenuDebug => "Debug",
        Key::MenuHelp => "Help",

        Key::MenuOpenRom => "Open ROM...",
        Key::MenuOpenRecent => "Open Recent",
        Key::MenuNoRecentRoms => "No recent ROMs",
        Key::MenuSaveStates => "Save States",
        Key::MenuQuit => "Quit",
        Key::MenuTheme => "Theme",
        Key::MenuWindowSize => "Window Size",

        Key::SettingsTitle => "Settings",
        Key::SettingsTabVideo => "Video",
        Key::SettingsTabShaders => "Shaders",
        Key::SettingsTabAudio => "Audio",
        Key::SettingsTabInput => "Input",
        Key::SettingsTabEmulation => "Emulation",
        Key::SettingsHeadingDisplay => "Display",
        Key::SettingsHeadingAccessibility => "Accessibility",
        Key::SettingsTheme => "Theme:",
        Key::SettingsLanguage => "Language:",

        Key::StatusNoRom => "No ROM loaded",
        Key::StatusIdle => "Idle",
        Key::StatusRunning => "Running",
        Key::StatusPaused => "Paused",
        Key::StatusNetplay => "Netplay",

        Key::ButtonOk => "OK",
        Key::ButtonCancel => "Cancel",
        Key::ButtonSave => "Save",
        Key::ButtonLoad => "Load",
        Key::ButtonApply => "Apply",
        Key::ButtonReset => "Reset",
    }
}

/// Spanish catalog. Returns `None` for any key not yet translated, which [`tr`]
/// resolves to the English value (fallback). Keeping the un-translated keys out
/// of the table — rather than duplicating the English string — makes the
/// fallback explicit and the translation coverage auditable.
//
// `match_same_arms`: see `english` — distinct keys keep independent arms even
// when they currently share a translation.
#[allow(clippy::match_same_arms)]
const fn spanish(key: Key) -> Option<&'static str> {
    match key {
        Key::MenuFile => Some("Archivo"),
        Key::MenuEmulation => Some("Emulación"),
        Key::MenuTools => Some("Herramientas"),
        Key::MenuView => Some("Ver"),
        Key::MenuDebug => Some("Depurar"),
        Key::MenuHelp => Some("Ayuda"),

        Key::MenuOpenRom => Some("Abrir ROM..."),
        Key::MenuOpenRecent => Some("Abrir reciente"),
        Key::MenuNoRecentRoms => Some("Sin ROMs recientes"),
        Key::MenuSaveStates => Some("Estados guardados"),
        Key::MenuQuit => Some("Salir"),
        Key::MenuTheme => Some("Tema"),
        Key::MenuWindowSize => Some("Tamaño de ventana"),

        Key::SettingsTitle => Some("Configuración"),
        Key::SettingsTabVideo => Some("Vídeo"),
        // "Shaders" and "Audio" are kept untranslated by returning `None`: both
        // are loanwords used verbatim in Spanish UIs, so they fall through to
        // the English catalog. This also keeps the English-fallback path live
        // and unit-tested (see `missing_key_falls_back_to_english`).
        Key::SettingsTabShaders | Key::SettingsTabAudio => None,
        Key::SettingsTabInput => Some("Controles"),
        Key::SettingsTabEmulation => Some("Emulación"),
        Key::SettingsHeadingDisplay => Some("Pantalla"),
        Key::SettingsHeadingAccessibility => Some("Accesibilidad"),
        Key::SettingsTheme => Some("Tema:"),
        Key::SettingsLanguage => Some("Idioma:"),

        Key::StatusNoRom => Some("Sin ROM cargada"),
        Key::StatusIdle => Some("Inactivo"),
        Key::StatusRunning => Some("En ejecución"),
        Key::StatusPaused => Some("Pausado"),
        Key::StatusNetplay => Some("Juego en red"),

        Key::ButtonOk => Some("Aceptar"),
        Key::ButtonCancel => Some("Cancelar"),
        Key::ButtonSave => Some("Guardar"),
        Key::ButtonLoad => Some("Cargar"),
        Key::ButtonApply => Some("Aplicar"),
        Key::ButtonReset => Some("Restablecer"),
    }
}

/// Resolve `key` in `locale`, falling back to English for any key the locale's
/// catalog has not translated. English itself always resolves directly.
#[must_use]
pub const fn tr_in(locale: Locale, key: Key) -> &'static str {
    match locale {
        Locale::English => english(key),
        Locale::Spanish => match spanish(key) {
            Some(s) => s,
            None => english(key),
        },
    }
}

/// Resolve `key` in the current process-global locale (see [`set_locale`]),
/// with English fallback. This is the primary entry point the UI calls.
#[must_use]
pub fn tr(key: Key) -> &'static str {
    tr_in(current_locale(), key)
}

/// Ergonomic wrapper around [`tr`]: `t!(MenuFile)` == `tr(Key::MenuFile)`.
///
/// Keeps call sites terse without importing the `Key` enum everywhere. Use
/// [`tr`] directly when a `Key` value is computed dynamically.
#[macro_export]
macro_rules! t {
    ($key:ident) => {
        $crate::i18n::tr($crate::i18n::Key::$key)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_default() {
        assert_eq!(Locale::default(), Locale::English);
    }

    #[test]
    fn default_locale_strings_are_verbatim() {
        // The English catalog must reproduce the pre-i18n literals byte-for-byte
        // so the shipped default UI is unchanged. Asserted via `tr_in` against
        // the explicit English locale (and `english` directly) so this test
        // never touches the process-global `CURRENT_LOCALE` — keeping it sound
        // under parallel test execution.
        assert_eq!(tr_in(Locale::English, Key::MenuFile), "File");
        assert_eq!(tr_in(Locale::English, Key::MenuEmulation), "Emulation");
        assert_eq!(tr_in(Locale::English, Key::MenuHelp), "Help");
        assert_eq!(tr_in(Locale::English, Key::SettingsTitle), "Settings");
        assert_eq!(
            tr_in(Locale::English, Key::SettingsHeadingDisplay),
            "Display"
        );
        assert_eq!(tr_in(Locale::English, Key::StatusRunning), "Running");
        assert_eq!(tr_in(Locale::English, Key::ButtonReset), "Reset");
        // The same strings via the per-locale catalog const fn.
        assert_eq!(english(Key::MenuFile), "File");
        assert_eq!(english(Key::ButtonReset), "Reset");
    }

    #[test]
    fn second_locale_translates() {
        assert_eq!(tr_in(Locale::Spanish, Key::MenuFile), "Archivo");
        assert_eq!(tr_in(Locale::Spanish, Key::StatusPaused), "Pausado");
        assert_eq!(tr_in(Locale::Spanish, Key::ButtonCancel), "Cancelar");
    }

    #[test]
    fn missing_key_falls_back_to_english() {
        // `Shaders`/`Audio` are intentionally untranslated in the Spanish
        // catalog (`spanish` returns `None`), so they MUST resolve to the
        // verbatim English value via the `None => english(key)` fallback arm.
        assert_eq!(spanish(Key::SettingsTabShaders), None);
        assert_eq!(spanish(Key::SettingsTabAudio), None);
        assert_eq!(tr_in(Locale::Spanish, Key::SettingsTabShaders), "Shaders");
        assert_eq!(tr_in(Locale::Spanish, Key::SettingsTabAudio), "Audio");
        assert_eq!(
            tr_in(Locale::Spanish, Key::SettingsTabShaders),
            english(Key::SettingsTabShaders),
        );
        // No resolved string is ever empty, in either locale, for any catalog
        // key — the fallback guarantees a value.
        for key in [
            Key::MenuFile,
            Key::SettingsTabShaders,
            Key::ButtonOk,
            Key::StatusIdle,
        ] {
            assert!(!tr_in(Locale::Spanish, key).is_empty());
            assert!(!tr_in(Locale::English, key).is_empty());
        }
    }

    #[test]
    fn locale_tag_round_trips() {
        for loc in Locale::all() {
            assert_eq!(Locale::from_u8(loc.as_u8()), loc);
        }
        // Unknown byte falls back to English (never panics).
        assert_eq!(Locale::from_u8(200), Locale::English);
    }

    #[test]
    fn macro_matches_tr() {
        // The `t!` macro expands to `tr(Key::..)`, which reads the process-
        // global locale. Validate the expansion against `tr_in(current_locale(),
        // ..)` — the exact value `tr` resolves to — WITHOUT calling `set_locale`,
        // so the test reads but never mutates the global and is sound under
        // parallel execution regardless of which locale happens to be active.
        let loc = current_locale();
        assert_eq!(t!(MenuFile), tr_in(loc, Key::MenuFile));
        assert_eq!(t!(ButtonReset), tr_in(loc, Key::ButtonReset));
    }
}

//! v1.7.0 "Forge" beta.5 Workstream H6 — `?settings=` share-links.
//!
//! This module is gated `#[cfg(target_arch = "wasm32")]`. It serializes a
//! **curated subset** of the runtime [`crate::config::Config`] to a compact,
//! URL-safe base64 blob so a user can share their browser viewing setup (NTSC
//! / CRT knobs, palette-correction, overscan crop, theme/zoom, audio gain) as
//! a single `?settings=…` link. On load, the blob is decoded and applied over
//! the default config; a "Copy share link" affordance re-encodes the live
//! config for sharing.
//!
//! ## Why a subset, not the whole `Config`
//!
//! The full `Config` carries machine-local state that has no business in a
//! shared URL — recent-ROM paths, a saved `RetroAchievements` login token, HD-
//! pack filesystem paths, keybindings — and would bloat the blob far past a
//! sane URL length. [`ShareSettings`] captures only the presentation/display
//! fields that are meaningful + safe to transplant to another machine. It is
//! its own serde type (every field `#[serde(default)]`) so a link minted by an
//! older or newer build still decodes — unknown keys are ignored, missing keys
//! take the default. See ADR 0022 for the format + versioning posture.
//!
//! ## Safety
//!
//! [`decode`] is hardened against a malformed or oversized query value: the
//! base64 is length-capped before decoding, and a parse failure yields `None`
//! (the app silently keeps its defaults) rather than propagating an error.

use core::cell::RefCell;

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::config::{AppTheme, Config};

thread_local! {
    /// The latest live [`ShareSettings`] snapshot published by the `App` each
    /// frame, so the JS-callable [`rustynes_share_link`] can mint a link
    /// reflecting the user's current settings without a JS↔App round-trip.
    static LIVE_SHARE: RefCell<ShareSettings> = RefCell::new(ShareSettings::default());
}

/// Publish the live config's shareable subset into the thread-local snapshot.
///
/// Called every frame by the `App`, so it is change-checked: the shareable
/// subset is only rebuilt (and the `ntsc_filter` `String` only re-cloned) when
/// a relevant field actually changed. The steady state is a field-by-field
/// equality check against the stored snapshot — `String` compares by slice, so
/// it allocates nothing.
pub fn publish_live(config: &Config) {
    LIVE_SHARE.with(|s| {
        // Cheap dirty-check: compare the live config's shareable fields against
        // the stored snapshot without building a fresh `ShareSettings` first.
        if s.borrow().matches_config(config) {
            return;
        }
        *s.borrow_mut() = ShareSettings::from_config(config);
    });
}

/// JS bridge: return a full share URL for the current live settings.
///
/// Returns an empty string if the page location is unavailable. Called from
/// the "Copy share link" button in `web/index.html` (which copies the result
/// to the clipboard).
#[wasm_bindgen]
#[must_use]
pub fn rustynes_share_link() -> String {
    LIVE_SHARE.with(|s| {
        let share = s.borrow();
        let blob = share.encode();
        web_sys::window()
            .map(|w| w.location())
            .and_then(|loc| {
                let origin = loc.origin().ok()?;
                let pathname = loc.pathname().ok()?;
                Some(format!("{origin}{pathname}?settings={blob}"))
            })
            .unwrap_or_default()
    })
}

/// Maximum accepted length of the raw `?settings=` value (base64url chars).
/// A legitimate blob is a few hundred bytes; this cap (8 KiB) stops a
/// pathological URL from forcing a large allocation in `atob`.
const MAX_SHARE_LEN: usize = 8 * 1024;

/// Validate + clamp a numeric field decoded from an untrusted share link.
///
/// Returns `fallback` when `value` is non-finite (`NaN` / `±Infinity`),
/// otherwise `value` clamped to `[min, max]`. Used by [`ShareSettings::apply_to`]
/// so a malformed/malicious `?settings=` blob can never push a float field
/// out of the range its settings-UI slider enforces.
#[must_use]
const fn sanitize_f32(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if !value.is_finite() {
        fallback
    } else if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Bit-exact `f32` equality (sidesteps the `float_cmp` lint). Used by
/// [`ShareSettings::matches_config`] for the per-frame publish dirty-check,
/// where "unchanged" means literally the same bits.
#[must_use]
const fn bits_eq(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits()
}

/// Format version embedded in the blob. Bumped only on a breaking field-shape
/// change; readers tolerate any version (serde-default the unknowns), so this
/// is informational + future-proofing, not a hard gate. (ADR 0022.)
const SHARE_VERSION: u8 = 1;

/// The curated, shareable subset of [`Config`].
///
/// Every field is `#[serde(default)]`, so a blob from a different build (with
/// fields added or removed) still round-trips: unknown keys are ignored on
/// decode, absent keys fall back to the live default.
// A DTO mirroring `Config`'s shareable toggles; the bools map 1:1 to existing
// config flags, so grouping them into a sub-struct would only add indirection.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ShareSettings {
    /// Blob format version (see [`SHARE_VERSION`]).
    #[serde(default)]
    pub v: u8,
    // --- Presentation (graphics) ---
    /// `[graphics] ntsc_filter` — `"off"` / `"composite"` / `"rgb"` / `"composite-rt"`.
    #[serde(default)]
    pub ntsc_filter: String,
    /// `[graphics] crt_filter`.
    #[serde(default)]
    pub crt_filter: bool,
    /// `[graphics] crt_scanline`.
    #[serde(default)]
    pub crt_scanline: f32,
    /// `[graphics] ntsc_contrast`.
    #[serde(default)]
    pub ntsc_contrast: f32,
    /// `[graphics] ntsc_saturation`.
    #[serde(default)]
    pub ntsc_saturation: f32,
    /// `[graphics] ntsc_brightness`.
    #[serde(default)]
    pub ntsc_brightness: f32,
    /// `[graphics] ntsc_hue`.
    #[serde(default)]
    pub ntsc_hue: f32,
    /// `[graphics] hide_overscan`.
    #[serde(default)]
    pub hide_overscan: bool,
    // --- Display (ui) ---
    /// `[ui] theme`.
    #[serde(default)]
    pub theme: AppTheme,
    /// `[ui] pixel_aspect_correction` (8:7).
    #[serde(default)]
    pub pixel_aspect_correction: bool,
    /// `[ui] zoom_factor`.
    #[serde(default)]
    pub zoom_factor: f32,
    /// `[ui] show_fps`.
    #[serde(default)]
    pub show_fps: bool,
    // --- Audio ---
    /// `[audio] volume` (master gain).
    #[serde(default)]
    pub volume: f32,
}

impl ShareSettings {
    /// Capture the shareable subset from a live [`Config`].
    #[must_use]
    pub fn from_config(c: &Config) -> Self {
        Self {
            v: SHARE_VERSION,
            ntsc_filter: c.graphics.ntsc_filter.clone(),
            crt_filter: c.graphics.crt_filter,
            crt_scanline: c.graphics.crt_scanline,
            ntsc_contrast: c.graphics.ntsc_contrast,
            ntsc_saturation: c.graphics.ntsc_saturation,
            ntsc_brightness: c.graphics.ntsc_brightness,
            ntsc_hue: c.graphics.ntsc_hue,
            hide_overscan: c.graphics.hide_overscan,
            theme: c.ui.theme,
            pixel_aspect_correction: c.ui.pixel_aspect_correction,
            zoom_factor: c.ui.zoom_factor,
            show_fps: c.ui.show_fps,
            volume: c.audio.volume,
        }
    }

    /// Whether this snapshot already reflects the live `Config`'s shareable
    /// fields. Used by [`publish_live`] to skip rebuilding (and re-cloning the
    /// `ntsc_filter` `String`) when nothing changed. Allocates nothing — the
    /// `String` comparison is a byte-slice compare.
    ///
    /// Float fields compare bit-exact (`to_bits`), which is what we want for a
    /// "did this exact value change since last publish" dirty-check (and it
    /// sidesteps the `float_cmp` lint).
    #[must_use]
    fn matches_config(&self, c: &Config) -> bool {
        self.v == SHARE_VERSION
            && self.ntsc_filter == c.graphics.ntsc_filter
            && self.crt_filter == c.graphics.crt_filter
            && bits_eq(self.crt_scanline, c.graphics.crt_scanline)
            && bits_eq(self.ntsc_contrast, c.graphics.ntsc_contrast)
            && bits_eq(self.ntsc_saturation, c.graphics.ntsc_saturation)
            && bits_eq(self.ntsc_brightness, c.graphics.ntsc_brightness)
            && bits_eq(self.ntsc_hue, c.graphics.ntsc_hue)
            && self.hide_overscan == c.graphics.hide_overscan
            && self.theme == c.ui.theme
            && self.pixel_aspect_correction == c.ui.pixel_aspect_correction
            && bits_eq(self.zoom_factor, c.ui.zoom_factor)
            && self.show_fps == c.ui.show_fps
            && bits_eq(self.volume, c.audio.volume)
    }

    /// Apply the shareable subset over a [`Config`] in place. Only the curated
    /// fields are touched; everything else keeps the destination config's value.
    ///
    /// A share link is untrusted input: a malformed or malicious blob could
    /// carry `NaN`, negative, or absurdly large float values that would corrupt
    /// layout (`zoom_factor`) or audio (`volume`), or push the NTSC/CRT knobs
    /// off-scale. Each numeric field is therefore validated through
    /// [`sanitize_f32`] — non-finite values are rejected (the destination keeps
    /// its current value) and finite values are clamped to the same range the
    /// settings UI enforces for that field.
    pub fn apply_to(&self, c: &mut Config) {
        // Only adopt a known filter token; an unknown/garbage value would leave
        // the renderer in a confused state, so fall back to keeping the current.
        if matches!(
            self.ntsc_filter.as_str(),
            "off" | "composite" | "rgb" | "composite-rt"
        ) {
            c.graphics.ntsc_filter.clone_from(&self.ntsc_filter);
        }
        c.graphics.crt_filter = self.crt_filter;
        c.graphics.crt_scanline =
            sanitize_f32(self.crt_scanline, 0.0, 1.0, c.graphics.crt_scanline);
        c.graphics.ntsc_contrast =
            sanitize_f32(self.ntsc_contrast, -1.0, 1.0, c.graphics.ntsc_contrast);
        c.graphics.ntsc_saturation =
            sanitize_f32(self.ntsc_saturation, -1.0, 1.0, c.graphics.ntsc_saturation);
        c.graphics.ntsc_brightness = sanitize_f32(
            self.ntsc_brightness,
            -100.0,
            100.0,
            c.graphics.ntsc_brightness,
        );
        c.graphics.ntsc_hue = sanitize_f32(self.ntsc_hue, -180.0, 180.0, c.graphics.ntsc_hue);
        c.graphics.hide_overscan = self.hide_overscan;
        c.ui.theme = self.theme;
        c.ui.pixel_aspect_correction = self.pixel_aspect_correction;
        c.ui.zoom_factor = sanitize_f32(
            self.zoom_factor,
            crate::config::UiConfig::ZOOM_MIN,
            crate::config::UiConfig::ZOOM_MAX,
            c.ui.zoom_factor,
        );
        c.ui.show_fps = self.show_fps;
        c.audio.volume = sanitize_f32(self.volume, 0.0, 1.0, c.audio.volume);
    }

    /// Encode to a compact URL-safe base64 blob (TOML body → base64url).
    #[must_use]
    pub fn encode(&self) -> String {
        // TOML is already a workspace dep + the on-disk config format, so reusing
        // it keeps the shape consistent with `Config`. A serialize failure (which
        // a flat struct of primitives won't hit) degrades to an empty blob.
        let toml = toml::to_string(self).unwrap_or_default();
        crate::wasm_io::base64url_encode(toml.as_bytes())
    }

    /// Decode from a `?settings=` value, guarded against malformed / oversized
    /// input. `None` if the value is too long, not valid base64url, or not
    /// valid UTF-8 TOML for [`ShareSettings`].
    #[must_use]
    pub fn decode(raw: &str) -> Option<Self> {
        if raw.is_empty() || raw.len() > MAX_SHARE_LEN {
            return None;
        }
        let bytes = crate::wasm_io::base64url_decode(raw)?;
        let text = core::str::from_utf8(&bytes).ok()?;
        toml::from_str::<Self>(text).ok()
    }
}

/// Read the `?settings=` value from the current page URL, if present + valid.
///
/// Parses `window.location.search`. Returns `None` when there is no `settings`
/// parameter or it fails the [`ShareSettings::decode`] guards.
#[must_use]
pub fn settings_from_url() -> Option<ShareSettings> {
    let search = web_sys::window()?.location().search().ok()?;
    let raw = query_param(&search, "settings")?;
    ShareSettings::decode(&raw)
}

/// Apply any `?settings=` override over a fresh default [`Config`] for the wasm
/// boot path. Always returns a usable config (the default when no/invalid blob).
#[must_use]
pub fn config_from_url_or_default() -> Config {
    let mut config = Config::default();
    if let Some(share) = settings_from_url() {
        share.apply_to(&mut config);
        crate::wasm_io::log("applied settings from ?settings= share link");
    }
    config
}

/// Minimal `key=value` extractor for a `?a=b&c=d` query string (leading `?`
/// tolerated). Avoids pulling a URL-parsing dep for one parameter. The value is
/// returned undecoded — the share blob is already URL-safe base64, so it needs
/// no percent-decoding.
fn query_param(search: &str, key: &str) -> Option<String> {
    let q = search.strip_prefix('?').unwrap_or(search);
    for pair in q.split('&') {
        if let Some((k, v)) = pair.split_once('=')
            && k == key
        {
            return Some(v.to_owned());
        }
    }
    None
}

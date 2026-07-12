//! Marquee CRT shader stack + raw-signal decode (v2.1.9 "Presentation & Signal").
//!
//! New presentation shaders added in the v2.1.9 B6 (CRT stack) and P4 (raw
//! composite) workstreams, kept in **separate WGSL files** from the existing
//! ladder (`CRT_WGSL`, `NTSC_LMP_WGSL`, `BISQWIT_WGSL` in `lib.rs`) so they
//! compose additively and don't collide with concurrent work on the base
//! blitter. Each is a single fullscreen fragment pass over the shared
//! rect/crop letterbox uniform convention documented on [`super::CRT_WGSL`]; the
//! CRT trio ([`CRT_ROYALE_WGSL`], [`CRT_GUEST_WGSL`], [`MEGATRON_WGSL`]) share a
//! 64-byte `rect/crop/params/aux` uniform block, and [`SIGNAL_DECODE_WGSL`]
//! consumes the palette-INDEX framebuffer (an `R16Uint`-style texture) so it can
//! reconstruct the true 2C02 composite signal (see `rustynes-ppu::raw_signal`).
//!
//! These are opt-in: the shipped default presentation (plain blit / the existing
//! CRT) is unchanged, so the default framebuffer stays byte-identical.

/// CRT-Royale — single-pass WGSL port.
///
/// Gaussian luminance-scaled beam, selectable phosphor mask, gamma-correct
/// scanlines, barrel curvature. See the file header for the model and the
/// shared CRT-stack uniform layout.
pub const CRT_ROYALE_WGSL: &str = include_str!("crt_royale.wgsl");

/// crt-guest-advanced / guest-dr-venom — single-pass WGSL port (power-shaped
/// beam, halation glow, selectable mask, curvature).
pub const CRT_GUEST_WGSL: &str = include_str!("crt_guest.wgsl");

/// Sony Megatron — single-pass WGSL port (per-subpixel phosphor lighting,
/// selectable mask, gamma-correct beam, an HDR headroom hook with SDR Reinhard
/// tone-map fallback).
pub const MEGATRON_WGSL: &str = include_str!("megatron.wgsl");

/// Raw NTSC signal-decode pass (P4).
///
/// Reconstructs the 2C02's two-level chroma square wave from the palette-index
/// framebuffer and demodulates it, so signal-domain artifacts (composite bleed,
/// dot crawl, dither transparency) survive.
pub const SIGNAL_DECODE_WGSL: &str = include_str!("signal_decode.wgsl");

/// A stable identifier for each shader in the v2.1.9 CRT stack.
///
/// Lets a host (the desktop frontend, the Android renderer) select one by name
/// from config or a per-game preset without hard-coding the WGSL source at the
/// call site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrtStackShader {
    /// [`CRT_ROYALE_WGSL`].
    CrtRoyale,
    /// [`CRT_GUEST_WGSL`].
    CrtGuest,
    /// [`MEGATRON_WGSL`].
    Megatron,
    /// [`SIGNAL_DECODE_WGSL`] (samples the index texture, not the RGBA one).
    SignalDecode,
}

impl CrtStackShader {
    /// Every shader in the stack, in a stable order (for UI enumeration / tests).
    pub const ALL: [Self; 4] = [
        Self::CrtRoyale,
        Self::CrtGuest,
        Self::Megatron,
        Self::SignalDecode,
    ];

    /// The WGSL source for this shader.
    #[must_use]
    pub const fn wgsl(self) -> &'static str {
        match self {
            Self::CrtRoyale => CRT_ROYALE_WGSL,
            Self::CrtGuest => CRT_GUEST_WGSL,
            Self::Megatron => MEGATRON_WGSL,
            Self::SignalDecode => SIGNAL_DECODE_WGSL,
        }
    }

    /// A short, stable, lowercase slug used in config files and per-game presets.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::CrtRoyale => "crt-royale",
            Self::CrtGuest => "crt-guest",
            Self::Megatron => "megatron",
            Self::SignalDecode => "signal-decode",
        }
    }

    /// A human-readable display name for menus.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::CrtRoyale => "CRT-Royale",
            Self::CrtGuest => "CRT Guest Advanced",
            Self::Megatron => "Sony Megatron (HDR)",
            Self::SignalDecode => "Raw NTSC Signal Decode",
        }
    }

    /// `true` when this pass samples the palette-INDEX texture (a `texture_2d<u32>`
    /// at binding 0) rather than the decoded RGBA texture — the host must bind the
    /// index framebuffer and use the index-uniform layout for these.
    #[must_use]
    pub const fn samples_index_texture(self) -> bool {
        matches!(self, Self::SignalDecode)
    }

    /// Resolve a config/preset slug back to a shader, `None` if unrecognised.
    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.slug() == slug)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shader_has_nonempty_source() {
        for s in CrtStackShader::ALL {
            assert!(!s.wgsl().is_empty(), "{s:?} has empty WGSL");
            assert!(!s.slug().is_empty());
            assert!(!s.display_name().is_empty());
        }
    }

    #[test]
    fn slug_roundtrips() {
        for s in CrtStackShader::ALL {
            assert_eq!(CrtStackShader::from_slug(s.slug()), Some(s));
        }
        assert_eq!(CrtStackShader::from_slug("nope"), None);
    }

    #[test]
    fn only_signal_decode_samples_index() {
        for s in CrtStackShader::ALL {
            assert_eq!(s.samples_index_texture(), s == CrtStackShader::SignalDecode);
        }
    }
}

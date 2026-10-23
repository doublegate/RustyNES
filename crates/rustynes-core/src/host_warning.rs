//! v3.5.0 (`FE-06`) — the host-facing warning codes every frontend shares.
//!
//! A [`HostWarning`] is something a host (the desktop, the browser, the
//! Android and iOS apps, a downstream embedder) should tell the player, as a
//! stable machine-readable code rather than a pre-composed sentence, so each
//! host can render it in the player's language. [`HostWarning::message`] is
//! the canonical English text for a host that does not localise.
//!
//! The enum was introduced in the mobile bridge (v2.0.3, `rustynes-mobile`),
//! which exports it to Kotlin and Swift. Until v3.5.0 it lived there, and the
//! desktop and browser frontends, which cannot depend on the `UniFFI` bridge,
//! were said to stay "in parity" by carrying identical English literals of
//! their own. They carried none: neither warning has a desktop or browser
//! producer (see below), so there was nothing to keep in parity, only a
//! second place a future warning's text would have had to be copied to. The
//! definition now lives here, at the surface every host already depends on;
//! the bridge re-exports this type to Kotlin / Swift unchanged (a `UniFFI`
//! remote enum; the generated bindings differ only in doc comments), and a desktop or browser
//! warning added later takes its code and its English from this one place.
//!
//! **Which hosts produce which code today:**
//!
//! | code | mobile | desktop / browser |
//! |---|---|---|
//! | [`HostWarning::PreTimebaseMovie`] | no longer (v2.9.8: older movies are refused) | never |
//! | [`HostWarning::RecoveredFromInternalError`] | the bridge's poisoned-lock recovery | never: release builds abort on panic (`panic = "abort"`), so there is no lock left to recover |
//!
//! Grow it by adding variants (each with its own `message` arm); never
//! repurpose one, so a host's per-code localisation table cannot silently
//! change meaning under it. The bridge's remote declaration must list the
//! same variants, which the compiler enforces.

use alloc::string::String;

/// A machine-readable code for a host-facing warning. See the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostWarning {
    /// A `.rnm` recorded on a pre-v2.0.0 "Timebase" build was loaded: its
    /// recorded input replays faithfully, but exact framebuffer / audio
    /// reproduction is not guaranteed across the engine-timebase boundary
    /// (ADR 0028).
    ///
    /// **No longer produced (v2.9.8).** `.rnm` format 3 records the emulation
    /// options and every older movie is refused with an error that says to
    /// re-record it, so a pre-v2.0.0 movie never reaches playback. The variant
    /// stays so hosts' localisation tables keep their meaning.
    PreTimebaseMovie,
    /// v2.7.4 (frontend audit MOB-07) — a call panicked inside the mobile
    /// bridge. The panic was contained (the mobile libraries unwind), but it
    /// may have left the emulator mid-update, so any netplay session was ended
    /// and the machine is frozen: the app stays open, the picture holds, and
    /// no further emulated cycle runs until the player reopens the game, power
    /// cycles, or loads a save state.
    RecoveredFromInternalError,
}

impl HostWarning {
    /// Every code, in declaration order (for hosts building a localisation
    /// table, and for the tests that pin each message).
    pub const ALL: [Self; 2] = [Self::PreTimebaseMovie, Self::RecoveredFromInternalError];

    /// The canonical English text for this code.
    ///
    /// Byte-identical to what the mobile bridge queued before v2.0.3 (and has
    /// returned from its legacy `drain_warnings` since), so a host that does
    /// not localise observes no change.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::PreTimebaseMovie => "this movie was recorded on a pre-v2.0.0 build -- \
                 input replay proceeds, but exact framebuffer/audio reproduction is not \
                 guaranteed across the engine-timebase boundary (see ADR 0028)"
                .into(),
            Self::RecoveredFromInternalError => "RustyNES stopped the game after an internal \
                 error. Any netplay session was ended. Reopen the game or load a save \
                 state to continue."
                .into(),
        }
    }
}

impl core::fmt::Display for HostWarning {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.message())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// The English text is part of the contract (a non-localising host shows
    /// it verbatim), so each one is pinned in full, as written before the
    /// enum moved here from the mobile bridge.
    #[test]
    fn each_code_keeps_its_english_text() {
        assert_eq!(
            HostWarning::PreTimebaseMovie.message(),
            "this movie was recorded on a pre-v2.0.0 build -- input replay proceeds, but exact \
             framebuffer/audio reproduction is not guaranteed across the engine-timebase \
             boundary (see ADR 0028)"
        );
        assert_eq!(
            HostWarning::RecoveredFromInternalError.message(),
            "RustyNES stopped the game after an internal error. Any netplay session was ended. \
             Reopen the game or load a save state to continue."
        );
        for w in HostWarning::ALL {
            assert_eq!(format!("{w}"), w.message(), "Display is the message");
        }
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
//! What `RetroAchievements` hardcore mode forbids (v3.4.0, `T-RA-HARDCORE`).
//!
//! # Why this file is shared by path
//!
//! Two hosts enforce hardcore: the desktop frontend (`rustynes-frontend`) and
//! the mobile bridge (`rustynes-mobile`). The bridge depends on this crate; the
//! frontend depends on it only behind its optional `retroachievements` feature,
//! because this crate links the rcheevos C library. The rule itself is pure
//! data, so it lives in this one dependency-free file: `rustynes-ra` compiles
//! it as [`crate::hardcore`], and the frontend's `session_policy` includes the
//! same file with `#[path]`, so its gates compile (and refuse nothing) in a
//! build without RA. One table, two hosts, no copy to drift.
//!
//! # The rule, and where it comes from
//!
//! The requirements are RA's own: the "Hardcore Compliance Requirements" page
//! (<https://docs.retroachievements.org/general/hardcore-compliance-requirements.html>,
//! section B "Hardcore Rules Enforcement" and section G "Auto-fail
//! Criteria"), the FAQ's "What is Hardcore mode?"
//! (<https://docs.retroachievements.org/general/faq.html>), and the
//! `rc_client` integration guide's "Enabling hardcore" section
//! (<https://github.com/RetroAchievements/rcheevos/wiki/rc_client-integration>).
//! Each [`Feature`] names the clause its decision rests on ([`Feature::rule`]).
//!
//! | feature | hardcore | RA's wording |
//! | --- | --- | --- |
//! | cheats (Game Genie, raw RAM cheats) | refused | "Cheats are disabled", including mountable devices such as the Game Genie |
//! | rewind | refused | "Rewind is disabled" |
//! | slowdown (speed below 100%) | refused | "Slowdown and frame advance are disabled" |
//! | frame advance | refused | same |
//! | save-state load | refused | "Loading save states is ALWAYS blocked" |
//! | memory viewer / editor, RAM search, pokes | refused | "Memory editors, debuggers, and/or scripting/TAS/recorded input playback are strictly prohibited" |
//! | debugger windows | refused | same |
//! | Lua scripting | refused | same |
//! | movie (TAS) playback, `TAStudio` | refused | same |
//! | input bots (`BasicBot`, the latency oracle) | refused | same: automated input and state restores |
//! | restoring saved achievement progress | refused | a resumed session "must drop to Casual mode" |
//! | save-state save | allowed | "Users should be allowed to create save states while in hardcore" |
//! | movie recording | allowed | `rc_client` guide: "recording inputs is okay" |
//! | fast forward | allowed | FAQ: "the only exception to altering speed is fast-forward" |
//! | reset, power cycle | allowed | what the console itself offers; `rc_client_reset` follows |
//! | run-ahead | allowed | not listed by RA (it shows a later frame sooner; it does not slow the game) |
//! | netplay | allowed | not listed by RA |
//!
//! Switching hardcore ON for a running game is not a feature but a
//! transition, and RA requires it to "result in a full reset of the game"
//! (auto-fail otherwise): each host power-cycles the console when hardcore
//! takes effect over a running game. Switching it OFF is allowed mid-session.
//!
//! Pausing is allowed but throttled: rcheevos' `rc_client_can_pause` decides,
//! and both hosts already ask it before a pause.

/// A host feature RA's hardcore rules decide on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    /// Game Genie codes and raw RAM cheats, from any route (the Cheats panel,
    /// the per-ROM cheat file, a script's `client.addcheat`, the mobile API).
    Cheats,
    /// The rewind gesture.
    Rewind,
    /// An emulation speed below 100%.
    Slowdown,
    /// Stepping one frame while paused.
    FrameAdvance,
    /// Restoring a save state (every route; see also `session_policy`).
    LoadState,
    /// Viewing or editing emulated memory: the Memory and Memory Compare
    /// panels, debugger pokes, the mobile memory read/poke calls.
    MemoryEditor,
    /// The debugger's inspector windows (CPU, PPU, OAM, APU, mapper, trace,
    /// watch/breakpoints, event viewer) and their mobile counterparts.
    Debugger,
    /// Running a Lua script.
    Scripting,
    /// Playing back a recorded input movie, including `TAStudio`.
    MoviePlayback,
    /// Tools that drive the emulator with generated input or restore its
    /// state behind the player's back: `BasicBot`, the latency oracle, the
    /// atlas and divergence tools.
    InputBot,
    /// Restoring achievement progress (hit counts) saved by an earlier
    /// session when a game is identified.
    ProgressRestore,
    /// Saving a state (allowed; the state just cannot be loaded in hardcore).
    SaveState,
    /// Recording an input movie (allowed).
    MovieRecording,
    /// Fast forward (allowed).
    FastForward,
    /// A soft reset or a power cycle (allowed).
    ResetOrPowerCycle,
    /// Run-ahead input-latency reduction (allowed; not listed by RA).
    RunAhead,
    /// A netplay session (allowed; not listed by RA).
    Netplay,
}

impl Feature {
    /// Every feature, in table order (for the audit tests and the docs).
    pub const ALL: [Self; 17] = [
        Self::Cheats,
        Self::Rewind,
        Self::Slowdown,
        Self::FrameAdvance,
        Self::LoadState,
        Self::MemoryEditor,
        Self::Debugger,
        Self::Scripting,
        Self::MoviePlayback,
        Self::InputBot,
        Self::ProgressRestore,
        Self::SaveState,
        Self::MovieRecording,
        Self::FastForward,
        Self::ResetOrPowerCycle,
        Self::RunAhead,
        Self::Netplay,
    ];

    /// Whether hardcore mode refuses this feature.
    ///
    /// Written as an exhaustive `match` with no wildcard, so a new variant
    /// cannot be added without deciding it here.
    #[must_use]
    pub const fn refused_in_hardcore(self) -> bool {
        match self {
            Self::Cheats
            | Self::Rewind
            | Self::Slowdown
            | Self::FrameAdvance
            | Self::LoadState
            | Self::MemoryEditor
            | Self::Debugger
            | Self::Scripting
            | Self::MoviePlayback
            | Self::InputBot
            | Self::ProgressRestore => true,
            Self::SaveState
            | Self::MovieRecording
            | Self::FastForward
            | Self::ResetOrPowerCycle
            | Self::RunAhead
            | Self::Netplay => false,
        }
    }

    /// The feature's name as the status line shows it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cheats => "Cheats",
            Self::Rewind => "Rewind",
            Self::Slowdown => "Slowdown",
            Self::FrameAdvance => "Frame advance",
            Self::LoadState => "Load state",
            Self::MemoryEditor => "Memory editor",
            Self::Debugger => "Debugger",
            Self::Scripting => "Lua scripting",
            Self::MoviePlayback => "Movie playback",
            Self::InputBot => "Input tools",
            Self::ProgressRestore => "Saved achievement progress",
            Self::SaveState => "Save state",
            Self::MovieRecording => "Movie recording",
            Self::FastForward => "Fast forward",
            Self::ResetOrPowerCycle => "Reset",
            Self::RunAhead => "Run-ahead",
            Self::Netplay => "Netplay",
        }
    }

    /// The RA clause the decision rests on (see the module table).
    #[must_use]
    pub const fn rule(self) -> &'static str {
        match self {
            Self::Cheats => "compliance B: cheats are disabled (Game Genie included)",
            Self::Rewind => "compliance B: rewind is disabled",
            Self::Slowdown | Self::FrameAdvance => {
                "compliance B: slowdown and frame advance are disabled"
            }
            Self::LoadState => "compliance B: loading save states is always blocked",
            Self::MemoryEditor
            | Self::Debugger
            | Self::Scripting
            | Self::MoviePlayback
            | Self::InputBot => {
                "compliance B: memory editors, debuggers, scripting, TAS and \
                 recorded input playback are prohibited"
            }
            Self::ProgressRestore => "compliance B: a resumed session must drop to Casual",
            Self::SaveState => "compliance B: creating save states is allowed",
            Self::MovieRecording => "rc_client guide: recording inputs is okay",
            Self::FastForward => "FAQ: fast-forward is the one allowed speed change",
            Self::ResetOrPowerCycle => "console behaviour; rc_client_reset follows",
            Self::RunAhead | Self::Netplay => "not listed by RA",
        }
    }
}

/// Whether `feature` must be refused now, given whether hardcore is active.
#[must_use]
pub const fn refuses(feature: Feature, hardcore: bool) -> bool {
    hardcore && feature.refused_in_hardcore()
}

/// The status-line text for a refused `feature`, in the same shape as the
/// timeline refusals ("Load state disabled (hardcore)").
#[must_use]
pub fn refusal_message(feature: Feature) -> String {
    format!("{} disabled (hardcore)", feature.label())
}

/// The emulation-speed factor the pacer may use under `hardcore`.
///
/// Under hardcore any factor below 1.0 (slowdown) is raised to 1.0, while fast
/// forward (above 1.0) is kept, because RA allows it. Outside hardcore the
/// factor is returned as is.
#[must_use]
pub fn paced_speed(speed: f32, hardcore: bool) -> f32 {
    if hardcore && speed < 1.0 { 1.0 } else { speed }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The audit table, written out. Each row is RA's decision for one
    /// feature; flipping any arm of `refused_in_hardcore` fails this test.
    #[test]
    fn every_feature_matches_the_ra_table() {
        let expected = [
            (Feature::Cheats, true),
            (Feature::Rewind, true),
            (Feature::Slowdown, true),
            (Feature::FrameAdvance, true),
            (Feature::LoadState, true),
            (Feature::MemoryEditor, true),
            (Feature::Debugger, true),
            (Feature::Scripting, true),
            (Feature::MoviePlayback, true),
            (Feature::InputBot, true),
            (Feature::ProgressRestore, true),
            (Feature::SaveState, false),
            (Feature::MovieRecording, false),
            (Feature::FastForward, false),
            (Feature::ResetOrPowerCycle, false),
            (Feature::RunAhead, false),
            (Feature::Netplay, false),
        ];
        assert_eq!(expected.len(), Feature::ALL.len(), "a feature has no row");
        for (feature, refused) in expected {
            assert_eq!(feature.refused_in_hardcore(), refused, "{feature:?}");
            assert_eq!(refuses(feature, true), refused, "{feature:?} in hardcore");
            assert!(!refuses(feature, false), "{feature:?} refused in casual");
        }
    }

    #[test]
    fn slowdown_is_raised_to_full_speed_and_fast_forward_is_kept() {
        assert!((paced_speed(0.5, true) - 1.0).abs() < f32::EPSILON);
        assert!((paced_speed(0.25, true) - 1.0).abs() < f32::EPSILON);
        assert!((paced_speed(2.0, true) - 2.0).abs() < f32::EPSILON);
        assert!((paced_speed(0.5, false) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn refusals_read_like_the_timeline_ones() {
        assert_eq!(
            refusal_message(Feature::FrameAdvance),
            "Frame advance disabled (hardcore)"
        );
        assert_eq!(
            refusal_message(Feature::MoviePlayback),
            "Movie playback disabled (hardcore)"
        );
    }
}

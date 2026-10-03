// SPDX-License-Identifier: GPL-3.0-or-later
//! Which timeline-changing actions a running session refuses (v2.9.9, NF-11).
//!
//! # Why this is its own module
//!
//! A `.rnm` movie records controller input and nothing else
//! (`rustynes_core::FrameInput`), and a netplay session exchanges controller
//! input and nothing else. Anything that changes the machine OUTSIDE that
//! input stream — a Reset, a Power Cycle, an FDS disk swap, a state load —
//! therefore breaks whichever of the two owns the timeline:
//!
//! - **during a recording** the action is not in the movie, so the saved file
//!   no longer replays the run it claims to (and an attested recording would
//!   carry an attestation of a run its own input cannot reproduce);
//! - **during playback** the replay runs on from a state it never recorded;
//! - **during netplay** the action happens on ONE peer, and the periodic
//!   checksum then reports a desync.
//!
//! The menu has greyed these items since v1.2.0 (`ui_shell`'s
//! `hw_interactive` / `rom_interactive`), but a greyed menu item is only one of
//! the ways to reach an action: the bound hotkey, the Save-States manager, the
//! browser grid and a script reach the same handlers without passing through
//! the menu, and the v2.9.9 re-audit found five of them unguarded. The rule
//! therefore lives here, as one pure decision every dispatch site asks, so a
//! new route to an action cannot forget half of it.
//!
//! # The rule
//!
//! | action | movie | netplay | RA hardcore |
//! | --- | --- | --- | --- |
//! | Reset, Power Cycle | refused | refused | allowed |
//! | disk swap / insert / eject | refused | refused | allowed |
//! | state load | refused | refused | refused |
//!
//! Reset and Power Cycle stay legal in hardcore because rcheevos treats them
//! as legitimate (they are what a real console offers); a state load is the
//! one thing hardcore exists to forbid. A disk swap is an ordinary act of
//! play on a real Famicom, so hardcore allows it too.
//!
//! When more than one owner refuses, the hardcore reason is reported first
//! (it is the one the player cannot end on their own), then the movie, then
//! netplay — the order the pre-v2.9.9 load sites already reported in.
//!
//! Nothing here reads the `App`, a lock or a clock, so the rule is tested
//! natively below.

/// An action that changes the machine outside the controller-input stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineAction {
    /// A soft Reset (the console's reset button).
    Reset,
    /// A Power Cycle (off, then on: power-on RAM, a fresh session).
    PowerCycle,
    /// Any FDS disk change: cycle sides, insert a specific side, or eject.
    DiskSwap,
    /// Restoring a save state (slot, manager, browser grid, or a script).
    LoadState,
}

impl TimelineAction {
    /// The action's name as the status line shows it ("Reset disabled ...").
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Reset => "Reset",
            Self::PowerCycle => "Power Cycle",
            Self::DiskSwap => "Disk swap",
            Self::LoadState => "Load state",
        }
    }
}

/// Who owns the timeline right now. Each flag is read by the caller from the
/// live session (`EmuCore::movie`, the netplay driver, the RA session).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)] // three independent owners, not a state machine
pub struct SessionOwners {
    /// A movie is recording or playing back.
    pub movie: bool,
    /// A netplay session (player, not spectator) is active.
    pub netplay: bool,
    /// `RetroAchievements` hardcore mode is enforcing its restrictions.
    pub hardcore: bool,
}

/// Why an action was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// RA hardcore forbids it.
    Hardcore,
    /// A movie is recording or playing back.
    Movie,
    /// A netplay session is active.
    Netplay,
}

impl Refusal {
    /// The status-line text for refusing `action` for this reason. The load
    /// texts are the ones the menu and hotkey have shown since v2.7.0, so a
    /// player sees the same words whichever route they took.
    #[must_use]
    pub fn message(self, action: TimelineAction) -> String {
        let what = action.label();
        match self {
            Self::Hardcore => format!("{what} disabled (hardcore)"),
            Self::Movie => format!("{what} disabled during movie"),
            Self::Netplay => format!("{what} disabled during netplay"),
        }
    }
}

/// Whether `action` must be refused under `owners`, and why. `None` means the
/// action may run. See the module docs for the table this implements.
#[must_use]
pub const fn refusal(action: TimelineAction, owners: SessionOwners) -> Option<Refusal> {
    if owners.hardcore && matches!(action, TimelineAction::LoadState) {
        return Some(Refusal::Hardcore);
    }
    if owners.movie {
        return Some(Refusal::Movie);
    }
    if owners.netplay {
        return Some(Refusal::Netplay);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [TimelineAction; 4] = [
        TimelineAction::Reset,
        TimelineAction::PowerCycle,
        TimelineAction::DiskSwap,
        TimelineAction::LoadState,
    ];

    #[test]
    fn nothing_is_refused_with_no_owner() {
        for a in ALL {
            assert_eq!(refusal(a, SessionOwners::default()), None, "{a:?}");
        }
    }

    #[test]
    fn a_movie_or_netplay_refuses_every_action() {
        for a in ALL {
            let movie = SessionOwners {
                movie: true,
                ..SessionOwners::default()
            };
            let netplay = SessionOwners {
                netplay: true,
                ..SessionOwners::default()
            };
            assert_eq!(refusal(a, movie), Some(Refusal::Movie), "{a:?}");
            assert_eq!(refusal(a, netplay), Some(Refusal::Netplay), "{a:?}");
        }
    }

    #[test]
    fn hardcore_refuses_a_load_only() {
        let hc = SessionOwners {
            hardcore: true,
            ..SessionOwners::default()
        };
        assert_eq!(
            refusal(TimelineAction::LoadState, hc),
            Some(Refusal::Hardcore)
        );
        for a in [
            TimelineAction::Reset,
            TimelineAction::PowerCycle,
            TimelineAction::DiskSwap,
        ] {
            assert_eq!(refusal(a, hc), None, "{a:?}");
        }
    }

    #[test]
    fn hardcore_is_reported_before_the_movie_and_the_movie_before_netplay() {
        let all = SessionOwners {
            movie: true,
            netplay: true,
            hardcore: true,
        };
        assert_eq!(
            refusal(TimelineAction::LoadState, all),
            Some(Refusal::Hardcore)
        );
        assert_eq!(refusal(TimelineAction::Reset, all), Some(Refusal::Movie));
    }

    #[test]
    fn the_load_messages_keep_their_pre_v2_9_9_wording() {
        assert_eq!(
            Refusal::Hardcore.message(TimelineAction::LoadState),
            "Load state disabled (hardcore)"
        );
        assert_eq!(
            Refusal::Movie.message(TimelineAction::LoadState),
            "Load state disabled during movie"
        );
    }
}

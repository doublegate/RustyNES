// SPDX-License-Identifier: GPL-3.0-or-later
//! Placing a CPU bus access on a chosen dot of its CPU cycle.
//!
//! Shared by the v2.6.18 diagnostics (`access_dot_derivation`,
//! `subtest_placement_probe`), which both need to convert "put the access in
//! dot N" into the `*_PHI_OFFSET` / `*_PHI_BACKOFF` pair the sweep knobs
//! actually take. It lived in both files identically until a review pointed
//! that out; two copies of an arithmetic conversion is exactly the shape that
//! drifts silently once one of them is corrected.
//!
//! # Why a dot and not a master clock
//!
//! `Bus::run_ppu_to` advances the PPU in whole dots, so a sub-dot change to a
//! split cannot be observed by anything: an NTSC CPU cycle is three dots and
//! the only question is which of the three an access lands in. The offsets are
//! denominated in master clocks, so this converts between the two — and it
//! lands each access on the dot's FIRST master clock, since any point inside
//! the dot is equivalent.

/// NTSC master clocks per PPU dot.
pub const MC_PER_DOT: i32 = 4;

/// The shipped effective advance for a READ, in master clocks:
/// `div / 2 - PPU_OFFSET` minus the `PPU_OFFSET` `run_ppu_to` subtracts
/// = 6 - 1 - 1 = 4, i.e. dot 1.
pub const READ_BASE_MC: i32 = 4;

/// The shipped effective advance for a WRITE: 6 + 1 - 1 = 6 mc — still dot 1.
/// The half-dot between this and [`READ_BASE_MC`] is invisible, which is the
/// whole finding these diagnostics exist to record.
pub const WRITE_BASE_MC: i32 = 6;

/// `(offset, backoff)` placing an access whose shipped advance is `base_mc`
/// into `dot`.
///
/// The offsets can only add, so a dot earlier than the shipped one is reachable
/// only through the backoff — which is why the backoff knobs exist at all, and
/// why three of the nine placement cells were unmeasurable before them.
#[must_use]
pub fn place(base_mc: i32, dot: i32) -> (u8, u8) {
    let delta = dot * MC_PER_DOT - base_mc;
    if delta >= 0 {
        (u8::try_from(delta).expect("offset fits in u8"), 0)
    } else {
        (0, u8::try_from(-delta).expect("backoff fits in u8"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dot_earlier_than_shipped_needs_the_backoff() {
        // Dot 1 is where both accesses already sit: no adjustment either way.
        assert_eq!(place(READ_BASE_MC, 1), (0, 0));
        assert_eq!(place(WRITE_BASE_MC, 1), (0, 2), "write sits mid-dot");
        // Dot 2 is reachable by adding.
        assert_eq!(place(READ_BASE_MC, 2), (4, 0));
        assert_eq!(place(WRITE_BASE_MC, 2), (2, 0));
        // Dot 0 is reachable ONLY by subtracting — the case the offsets alone
        // could not express.
        assert_eq!(place(READ_BASE_MC, 0), (0, 4));
        assert_eq!(place(WRITE_BASE_MC, 0), (0, 6));
    }

    #[test]
    fn every_knob_value_inside_one_dot_is_the_same_placement() {
        for off in 0..MC_PER_DOT {
            assert_eq!((READ_BASE_MC + off) / MC_PER_DOT, 1);
        }
        assert_eq!((READ_BASE_MC + MC_PER_DOT) / MC_PER_DOT, 2);
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
//! The battery-save write policy, shared by the desktop `.sav` file
//! (`battery_save`) and the browser's `IndexedDB` record (`web_battery`)
//! (v2.9.7 "Tandem", plan item 4).
//!
//! # Why this is its own module
//!
//! v2.7.3 (FE-01) wrote the policy for the desktop only, inside
//! `battery_save.rs`, which is native-only because it writes files. The web
//! build needs the same three decisions and has no filesystem, so the
//! decisions moved here, where both targets compile them, and the desktop
//! module became a thin file binding over this one. One policy, one set of
//! tests: a change to "when is a save due" cannot land on one platform and
//! silently not on the other.
//!
//! # The three decisions
//!
//! 1. **Which cartridges persist at all** — [`persists`](crate::battery_policy::persists): the header's battery
//!    bit AND a non-empty [`rustynes_core::Nes::save_data`]. Keyed on the bit,
//!    never on `sram()` being non-empty: NROM exposes 8 KiB of work RAM for
//!    every image, and MMC1 / MMC3 allocate RAM by default, so a volatile
//!    cartridge keyed on the slice would acquire a save it never had (review
//!    on #549). `save_data()`, not `sram()`: on a self-flashable board (GTROM,
//!    a flashable UNROM 512) the save is the flash image (v2.9.6).
//! 2. **When a comparison is due** — at most once per
//!    [`CHECK_PERIOD_FRAMES`](crate::battery_policy::CHECK_PERIOD_FRAMES)
//!    produced frames, or at once when forced (unload, ROM switch, exit, and on
//!    the web a page hide). A game that uses its battery RAM as scratch memory
//!    then costs one write a second, not sixty.
//! 3. **Whether a write is needed** — only when the live bytes differ from the
//!    last write that SUCCEEDED. The core keeps no save-RAM dirty latch, and
//!    adding one would change the `Mapper` API for every board; equality with
//!    the last write is exact and needs nothing from the core. A failed write
//!    leaves the baseline where it was, so the next comparison retries, and
//!    only the first failure of a run is reported (agy round 2 on #551).
//!
//! Nothing here performs I/O or reads a clock, so the policy is pure and every
//! rule above is tested natively below, for both platforms at once.

use rustynes_core::Nes;

/// How often, in produced frames, the periodic flush compares the live save
/// RAM with the last write. One second of NTSC play.
pub const CHECK_PERIOD_FRAMES: u32 = 60;

/// Whether `nes` is a cartridge whose save RAM is persisted: the battery bit
/// is set and [`Nes::save_data`] is non-empty. See the module docs, rule 1.
#[must_use]
pub fn persists(nes: &Nes) -> bool {
    nes.has_battery() && !nes.save_data().is_empty()
}

/// The write policy for one cartridge's battery RAM: the baseline (the bytes
/// last written or read), the period counter, and the failure latch.
///
/// Storage-agnostic: the caller owns WHERE the bytes go (a file, an `IndexedDB`
/// record) and reports each write's outcome with [`Self::written`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatteryPolicy {
    /// The bytes last written to (or read from) the store. "Clean" means the
    /// live save RAM equals this.
    last: Vec<u8>,
    /// Produced frames since the last periodic comparison.
    frames: u32,
    /// The last write failed. Lets [`Self::written`] report only the FIRST
    /// failure of a run, so a full disk or quota is shown to the player once
    /// rather than every second.
    failing: bool,
}

impl BatteryPolicy {
    /// A policy whose baseline is `baseline`: the bytes the store holds now.
    ///
    /// Take it AFTER any stored save was loaded into the cartridge, so an
    /// attached session never rewrites the bytes it just read.
    #[must_use]
    pub const fn new(baseline: Vec<u8>) -> Self {
        Self {
            last: baseline,
            frames: 0,
            failing: false,
        }
    }

    /// Count one produced frame and report whether a comparison is due: every
    /// [`CHECK_PERIOD_FRAMES`] calls, or at once when `force` is set. A due
    /// comparison restarts the period.
    pub const fn tick(&mut self, force: bool) -> bool {
        if !force {
            self.frames += 1;
            if self.frames < CHECK_PERIOD_FRAMES {
                return false;
            }
        }
        self.frames = 0;
        true
    }

    /// Whether `live` differs from the last successful write.
    #[must_use]
    pub fn differs(&self, live: &[u8]) -> bool {
        live != self.last.as_slice()
    }

    /// The bytes to write now, if any: a copy of `live` when a comparison is
    /// due ([`Self::tick`]) and `live` differs from the last write
    /// ([`Self::differs`]). The copy lets the caller write without holding
    /// the emulator.
    pub fn due(&mut self, live: &[u8], force: bool) -> Option<Vec<u8>> {
        (self.tick(force) && self.differs(live)).then(|| live.to_vec())
    }

    /// Record the outcome of writing `bytes`: on success they become the
    /// baseline and the failure latch clears; on failure the baseline stays,
    /// so the next comparison retries.
    ///
    /// Returns `true` only for the first failure after a success (or after
    /// the policy was created), so the caller can tell the player once.
    pub fn written(&mut self, bytes: Vec<u8>, ok: bool) -> bool {
        if ok {
            self.last = bytes;
            self.failing = false;
            false
        } else {
            !core::mem::replace(&mut self.failing, true)
        }
    }

    /// The bytes last written (or read). The value a stored save is
    /// compared against.
    #[must_use]
    pub fn baseline(&self) -> &[u8] {
        &self.last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NROM, 16 KiB PRG, 8 KiB CHR, with or without the battery bit; the
    /// program is an endless `JMP $C000`.
    fn rom(battery: bool) -> Vec<u8> {
        let mut v = b"NES\x1A".to_vec();
        v.extend_from_slice(&[1, 1, if battery { 0x02 } else { 0 }, 0]);
        v.resize(16, 0);
        let mut prg = vec![0xEAu8; 0x4000];
        prg[..3].copy_from_slice(&[0x4C, 0x00, 0xC0]);
        prg[0x3FFC] = 0x00;
        prg[0x3FFD] = 0xC0;
        v.extend_from_slice(&prg);
        v.extend(core::iter::repeat_n(0u8, 0x2000));
        v
    }

    #[test]
    fn only_a_battery_cartridge_persists() {
        let with = Nes::from_rom(&rom(true)).unwrap();
        let without = Nes::from_rom(&rom(false)).unwrap();
        assert!(!without.sram().is_empty(), "NROM exposes work RAM anyway");
        assert!(persists(&with));
        assert!(!persists(&without), "the battery bit decides, not sram()");
    }

    #[test]
    fn a_comparison_is_due_once_a_period_or_when_forced() {
        let mut p = BatteryPolicy::new(vec![0; 4]);
        let dirty = [1u8; 4];
        let mut due = 0;
        for _ in 0..(3 * CHECK_PERIOD_FRAMES) {
            if p.due(&dirty, false).is_some() {
                due += 1;
            }
        }
        assert_eq!(due, 3, "once per period, although always dirty");
        assert_eq!(p.due(&dirty, true), Some(dirty.to_vec()), "force: now");
    }

    #[test]
    fn a_clean_save_is_never_due() {
        let mut p = BatteryPolicy::new(vec![7; 4]);
        assert!(p.due(&[7; 4], true).is_none());
    }

    #[test]
    fn a_success_moves_the_baseline_and_a_failure_does_not() {
        let mut p = BatteryPolicy::new(vec![0; 2]);
        let bytes = p.due(&[5, 6], true).unwrap();
        assert!(p.written(bytes.clone(), false), "first failure: report");
        assert_eq!(p.baseline(), &[0, 0], "baseline kept, so it retries");
        assert!(!p.written(bytes.clone(), false), "second failure: quiet");
        assert!(!p.written(bytes, true), "success reports nothing");
        assert_eq!(p.baseline(), &[5, 6]);
        assert!(p.due(&[5, 6], true).is_none(), "and is now clean");
        assert!(
            p.written(vec![1, 1], false),
            "a new run of failures reports"
        );
    }
}

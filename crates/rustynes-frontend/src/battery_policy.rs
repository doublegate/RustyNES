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
//!    last write that SUCCEEDED. Equality with the last write is exact and
//!    needs nothing from the core. A failed write leaves the baseline where
//!    it was, so the next comparison retries, and only the first failure of a
//!    run is reported (agy round 2 on #551). Since v3.2.0 the two
//!    self-flashable boards, whose save is the whole PRG image, also report a
//!    dirty flag, and while it says "clean" the comparison is skipped; since
//!    v3.3.0 the policy clears that flag each time it compares
//!    ([`due_and_mark_clean`](crate::battery_policy::BatteryPolicy::due_and_mark_clean)),
//!    and stops trusting it after a failed write until one succeeds.
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
    /// Whether a board's "clean" report ([`Nes::is_save_data_dirty`] =
    /// `Some(false)`) may skip the comparison. True until a write fails: the
    /// board was marked clean when that write's bytes were taken
    /// ([`Self::due_and_mark_clean`]), so after a failure it reads clean while
    /// the store is stale, and only a byte comparison finds the difference.
    /// The next successful write restores it.
    trust_clean: bool,
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
            trust_clean: true,
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
    pub fn differs(&self, live: &[u8], is_dirty: Option<bool>) -> bool {
        if is_dirty == Some(false) && self.trust_clean {
            false
        } else {
            live != self.last.as_slice()
        }
    }

    /// The bytes to write now, if any: a copy of `live` when a comparison is
    /// due ([`Self::tick`]) and `live` differs from the last write
    /// ([`Self::differs`]). The copy lets the caller write without holding
    /// the emulator.
    pub fn due(&mut self, live: &[u8], is_dirty: Option<bool>, force: bool) -> Option<Vec<u8>> {
        (self.tick(force) && self.differs(live, is_dirty)).then(|| live.to_vec())
    }

    /// Record the outcome of writing `bytes`: on success they become the
    /// baseline and the failure latch clears; on failure the baseline stays,
    /// so the next comparison retries.
    ///
    /// Returns `true` only for the first failure after a success (or after
    /// the policy was created), so the caller can tell the player once.
    pub fn written(&mut self, bytes: Vec<u8>, ok: bool) -> bool {
        self.trust_clean = ok;
        if ok {
            self.last = bytes;
            self.failing = false;
            false
        } else {
            !core::mem::replace(&mut self.failing, true)
        }
    }

    /// Stop trusting the board's "clean" report until the next write reports
    /// back, so comparisons run on bytes. For a caller with several writes in
    /// flight: one that finishes out of order may have landed after a newer
    /// one, leaving the store older than the clean report implies.
    pub const fn distrust_clean(&mut self) {
        self.trust_clean = false;
    }

    /// [`Self::due`] for a cartridge the caller holds mutably, which also
    /// marks a tracking board's save data clean whenever the comparison ran
    /// (`T-SAVE-DIRTY-CLEAR`, v3.3.0). At that instant the live bytes are
    /// either the copy returned for writing or equal to the baseline, so
    /// "clean" is true of them; every later change sets the flag again, and a
    /// failed write stops [`Self::differs`] trusting it until a write
    /// succeeds. Before v3.3.0 nothing cleared the flag, so after a board's
    /// first change its hint never skipped a comparison again: on the
    /// self-flashable boards that is a 512 KiB comparison a second.
    pub fn due_and_mark_clean(&mut self, nes: &mut Nes, force: bool) -> Option<Vec<u8>> {
        if !self.tick(force) {
            return None;
        }
        let dirty = nes.is_save_data_dirty();
        let live = nes.save_data();
        let write = self.differs(live, dirty).then(|| live.to_vec());
        if dirty == Some(true) {
            nes.clear_save_data_dirty();
        }
        write
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
            if p.due(&dirty, None, false).is_some() {
                due += 1;
            }
        }
        assert_eq!(due, 3, "once per period, although always dirty");
        assert_eq!(
            p.due(&dirty, None, true),
            Some(dirty.to_vec()),
            "force: now"
        );
    }

    #[test]
    fn a_clean_save_is_never_due() {
        let mut p = BatteryPolicy::new(vec![7; 4]);
        assert!(p.due(&[7; 4], None, true).is_none());
    }

    /// v3.2.0: a board that tracks its save data (`Some`) lets the policy
    /// skip the byte comparison while it reports clean; untracked (`None`) and
    /// dirty (`Some(true)`) boards are compared as before. The skip is only
    /// sound because such a board marks every change dirty
    /// (`flash_boards_mark_every_save_data_change_dirty` in the mappers).
    #[test]
    fn a_board_reporting_clean_skips_the_comparison() {
        let mut p = BatteryPolicy::new(vec![0; 4]);
        assert!(
            p.due(&[1; 4], Some(false), true).is_none(),
            "clean: skipped"
        );
        assert_eq!(p.due(&[1; 4], Some(true), true), Some(vec![1; 4]), "dirty");
        assert_eq!(p.due(&[1; 4], None, true), Some(vec![1; 4]), "untracked");
    }

    /// `T-SAVE-DIRTY-CLEAR`: a GTROM board (mapper 111 with the battery bit,
    /// whose save data is its 512 KiB flash) is marked clean by every
    /// comparison, so its "clean" report skips the next one; a failed write
    /// must not be lost to that report, so the policy compares again until a
    /// write succeeds.
    #[test]
    fn a_comparison_marks_the_board_clean_and_a_failed_write_is_retried() {
        let mut img = b"NES\x1A".to_vec();
        img.extend_from_slice(&[32, 0, 0xF2, 0x60]); // 512 KiB PRG, mapper 111, battery
        img.resize(16, 0);
        img.resize(16 + 512 * 1024, 0xEA);
        let mut nes = Nes::from_rom(&img).unwrap();
        assert_eq!(
            nes.is_save_data_dirty(),
            Some(false),
            "fixture: a tracking board"
        );
        let mut p = BatteryPolicy::new(nes.save_data().to_vec());

        nes.save_data_mut()[5] ^= 0xFF;
        let bytes = p.due_and_mark_clean(&mut nes, true).expect("changed");
        assert_eq!(nes.is_save_data_dirty(), Some(false), "marked clean");
        assert!(
            p.due_and_mark_clean(&mut nes, true).is_none(),
            "in flight: once"
        );

        assert!(p.written(bytes, false), "the write failed");
        let retry = p.due_and_mark_clean(&mut nes, true);
        assert!(
            retry.is_some(),
            "a failed write is retried although the board reads clean"
        );
        assert!(!p.written(retry.unwrap(), true));
        assert!(
            p.due_and_mark_clean(&mut nes, true).is_none(),
            "clean after success"
        );
        assert_eq!(p.baseline(), nes.save_data());
    }

    #[test]
    fn a_success_moves_the_baseline_and_a_failure_does_not() {
        let mut p = BatteryPolicy::new(vec![0; 2]);
        let bytes = p.due(&[5, 6], None, true).unwrap();
        assert!(p.written(bytes.clone(), false), "first failure: report");
        assert_eq!(p.baseline(), &[0, 0], "baseline kept, so it retries");
        assert!(!p.written(bytes.clone(), false), "second failure: quiet");
        assert!(!p.written(bytes, true), "success reports nothing");
        assert_eq!(p.baseline(), &[5, 6]);
        assert!(p.due(&[5, 6], None, true).is_none(), "and is now clean");
        assert!(
            p.written(vec![1, 1], false),
            "a new run of failures reports"
        );
    }
}

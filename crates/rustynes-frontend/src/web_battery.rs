// SPDX-License-Identifier: GPL-3.0-or-later
//! Cartridge battery RAM in the browser (v2.9.7 "Tandem", plan item 4).
//!
//! The desktop has persisted battery saves to `<data_dir>/battery/<sha>.sav`
//! since v2.7.3 (FE-01). The web build had nothing: a player who saved in
//! *Zelda* and reloaded the tab found the save gone, although the same page
//! already kept save STATES in `IndexedDB` (`wasm_idb`). This module is the
//! browser's counterpart of `battery_save`: the same write policy
//! ([`crate::battery_policy`]), bound to an `IndexedDB` record instead of a file.
//!
//! # Why the logic is here and the I/O is not
//!
//! `IndexedDB` is asynchronous and exists only in a browser, so it cannot run in
//! a native test. Everything that DECIDES — which cartridge persists, whether
//! the stored bytes are usable, when a write is due, what a finished write
//! means — is in this module, which compiles natively under `cfg(test)` and is
//! tested below. The wasm-only glue in `wasm_idb` only moves bytes: it reads a
//! record, writes a record, and reports the outcome back here. That glue has
//! no headless test; it is exercised by the manual browser check recorded in
//! the v2.9.7 notes.
//!
//! # Restore before the first frame
//!
//! A game reads its battery RAM during boot, typically in the first few
//! frames. The desktop loads the `.sav` synchronously under the emulator lock,
//! so the first frame already sees it. The browser cannot: the read resolves
//! on a later turn of the event loop. So a battery cartridge enters
//! [`WebBattery::is_pending`] when it is installed, and `EmuCore` produces no
//! frame while it is pending. The read resolves into [`WebBattery::restore`],
//! which loads the bytes and releases the gate. A read cannot hold the gate
//! for ever: every outcome of it (a record, no record, a failure) is delivered
//! to `restore`, and each one releases.
//!
//! # Never clobber a save that was not read
//!
//! The rule from the desktop carries over unchanged: when the stored record
//! exists but cannot be used (the store could not be read, or its length is
//! not this cartridge's save size), nothing is loaded and nothing will be
//! written for this session. The record survives for a later, working, load.
//!
//! # Ordering of writes
//!
//! Writes are asynchronous too, so two could be outstanding at once and, in
//! principle, finish in either order. The periodic write therefore waits for
//! the previous one to finish ([`WebBattery::due`] returns nothing while one is
//! in flight; the next period catches any change). Only a forced write (page
//! hide, ROM switch, the start of a movie or netplay session) is issued while
//! another is in flight, and only when the live bytes differ from the ones
//! already on their way. `IndexedDB` processes open requests for one database in
//! the order they were made and runs overlapping read-write transactions in
//! the order they were created, so the later write lands last.

use rustynes_core::Nes;

use crate::battery_policy::{BatteryPolicy, persists};
use crate::save_state::hex_sha256;

/// The `IndexedDB` key of a ROM's battery record: `"<rom_sha256_hex>:battery"`.
///
/// It lives in the existing `save-states` object store of the `rustynes`
/// database, beside the save-state slots (`"<hex>:slot<N>"`, see
/// `wasm_idb::idb_key`). A separate object store would be the tidier schema,
/// but creating one means opening the database at version 2, and an open at a
/// new version is BLOCKED for as long as any other tab holds the database
/// open at version 1. Every build before this one opened it without an
/// `onversionchange` handler, so an old tab never lets go: the new tab's open
/// would wait until the user closed the old one, and with it the battery
/// restore that gates emulation. A key suffix needs no version change and
/// cannot collide with a slot key, because every slot key ends in `:slot`
/// and a digit.
#[must_use]
pub fn battery_key(rom_sha256: &[u8; 32]) -> String {
    format!("{}:battery", hex_sha256(rom_sha256))
}

/// The `localStorage` key of a ROM's battery record, used only where
/// `IndexedDB` is unavailable (some private-browsing modes). Distinct from the
/// save-state fallback keys (`rustynes-save-<hex>-slot<N>`).
#[must_use]
pub fn localstorage_battery_key(rom_sha256: &[u8; 32]) -> String {
    format!("rustynes-battery-{}", hex_sha256(rom_sha256))
}

/// A battery write taken by [`WebBattery::due`]: the ROM it belongs to and a
/// copy of the save RAM, so it can be stored without holding the emulator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebBatteryWrite {
    sha: [u8; 32],
    bytes: Vec<u8>,
}

impl WebBatteryWrite {
    /// The ROM hash the write belongs to.
    #[must_use]
    pub const fn rom_sha256(&self) -> &[u8; 32] {
        &self.sha
    }

    /// The bytes to store.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The `IndexedDB` key to store them under ([`battery_key`]).
    #[must_use]
    pub fn key(&self) -> String {
        battery_key(&self.sha)
    }
}

/// Why a stored battery record was not loaded. The session then runs without
/// persistence, and the record is left as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreError {
    /// The store could not be read (the reason, from the browser glue).
    Unreadable(String),
    /// The record's length is not this cartridge's save size.
    WrongSize {
        /// The record's length.
        found: usize,
        /// The cartridge's `save_data()` length.
        expected: usize,
    },
}

impl core::fmt::Display for RestoreError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unreadable(why) => write!(f, "the browser store could not be read ({why})"),
            Self::WrongSize { found, expected } => write!(
                f,
                "the stored save is {found} bytes, but this cartridge saves {expected}"
            ),
        }
    }
}

/// What [`WebBattery::restore`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Restored {
    /// The stored save was loaded into the cartridge; saving is armed.
    Loaded,
    /// Nothing was stored for this ROM; saving is armed from power-on RAM.
    Empty,
    /// The read belonged to a ROM (or a session) that is no longer the one
    /// waiting for it. Nothing changed.
    Stale,
    /// A record exists but could not be used. Nothing was loaded, nothing will
    /// be written, and the gate is released so the game still runs.
    Refused(RestoreError),
}

/// Where one cartridge's browser battery save is in its life.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum State {
    /// Nothing is persisted: no ROM, a cartridge without a battery, a refused
    /// record, or a session (movie, netplay) that owns the save RAM.
    #[default]
    Off,
    /// A battery cartridge is installed and its stored save is being read.
    /// Emulation waits.
    Pending {
        /// The ROM whose record is being read.
        sha: [u8; 32],
    },
    /// Saving is armed.
    Armed {
        /// The ROM this save belongs to.
        sha: [u8; 32],
        /// The shared write policy (baseline, period, failure latch).
        policy: BatteryPolicy,
        /// The bytes of the newest write that has not reported back yet.
        in_flight: Option<Vec<u8>>,
    },
}

/// One browser session's battery save: the state machine the wasm frontend
/// drives from its load path, its frame loop and its page-hide handler.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WebBattery {
    state: State,
}

impl WebBattery {
    /// No cartridge bound.
    #[must_use]
    pub const fn new() -> Self {
        Self { state: State::Off }
    }

    /// Bind a freshly installed cartridge. Returns its ROM hash when it has a
    /// save to persist: the caller then reads the stored record and hands the
    /// outcome to [`Self::restore`]. Until then [`Self::is_pending`] is true
    /// and no frame may be produced. Returns `None` (and binds nothing) for a
    /// cartridge without a battery.
    pub fn begin(&mut self, nes: &Nes) -> Option<[u8; 32]> {
        if persists(nes) {
            let sha = *nes.rom_sha256();
            self.state = State::Pending { sha };
            Some(sha)
        } else {
            self.state = State::Off;
            None
        }
    }

    /// Whether a stored save is still being read. Emulation must not produce
    /// a frame while this is true: the game would boot on power-on RAM, see no
    /// save, and could start a new file over it.
    #[must_use]
    pub const fn is_pending(&self) -> bool {
        matches!(self.state, State::Pending { .. })
    }

    /// Whether saving is armed.
    #[must_use]
    pub const fn is_armed(&self) -> bool {
        matches!(self.state, State::Armed { .. })
    }

    /// Deliver the outcome of reading `sha`'s record: `Ok(Some(bytes))` for a
    /// record, `Ok(None)` for none, `Err(reason)` when the store could not be
    /// read. Loads the bytes into `nes` when they fit, and in every case but
    /// [`Restored::Stale`] releases the pending gate.
    pub fn restore(
        &mut self,
        nes: &mut Nes,
        sha: [u8; 32],
        stored: Result<Option<Vec<u8>>, String>,
    ) -> Restored {
        // Only the read this state is waiting for may land, and only into the
        // ROM it was taken for: the user can load another game, or start a
        // movie, while the read is in flight.
        let waiting = matches!(self.state, State::Pending { sha: s } if s == sha);
        if !waiting || *nes.rom_sha256() != sha {
            return Restored::Stale;
        }
        let expected = nes.save_data().len();
        let outcome = match stored {
            Ok(Some(bytes)) if bytes.len() == expected => {
                nes.save_data_mut().copy_from_slice(&bytes);
                Restored::Loaded
            }
            Ok(Some(bytes)) => Restored::Refused(RestoreError::WrongSize {
                found: bytes.len(),
                expected,
            }),
            Ok(None) => Restored::Empty,
            Err(why) => Restored::Refused(RestoreError::Unreadable(why)),
        };
        self.state = match outcome {
            // The baseline is taken AFTER the load, so an attached session
            // never rewrites the bytes it just read.
            Restored::Loaded | Restored::Empty => State::Armed {
                sha,
                policy: BatteryPolicy::new(nes.save_data().to_vec()),
                in_flight: None,
            },
            Restored::Refused(_) | Restored::Stale => State::Off,
        };
        outcome
    }

    /// The write that is due now, if any. Without `force`: at most once per
    /// [`crate::battery_policy::CHECK_PERIOD_FRAMES`] calls (one call per
    /// produced frame), and never while an earlier write is in flight. With
    /// `force`: now, unless the live bytes equal the last successful write or
    /// the write already in flight. Report the outcome with [`Self::written`].
    pub fn due(&mut self, nes: &Nes, force: bool) -> Option<WebBatteryWrite> {
        let State::Armed {
            sha,
            policy,
            in_flight,
        } = &mut self.state
        else {
            return None;
        };
        if nes.rom_sha256() != sha {
            return None;
        }
        let live = nes.save_data();
        let bytes = match in_flight {
            Some(pending) if force => (live != pending.as_slice()).then(|| live.to_vec())?,
            Some(_) => return None,
            None => policy.due(live, force)?,
        };
        *in_flight = Some(bytes.clone());
        Some(WebBatteryWrite { sha: *sha, bytes })
    }

    /// Record a finished [`WebBatteryWrite`]. On success its bytes become the
    /// baseline; on failure the baseline stays and the next comparison
    /// retries. A write for another ROM (the game changed while it was in
    /// flight) is ignored.
    ///
    /// Returns `true` for the first failure of a run, so the caller can tell
    /// the player once.
    pub fn written(&mut self, write: WebBatteryWrite, ok: bool) -> bool {
        let State::Armed {
            sha,
            policy,
            in_flight,
        } = &mut self.state
        else {
            return false;
        };
        if write.sha != *sha {
            return false;
        }
        // An older write finishing behind a newer forced one leaves the newer
        // one in flight.
        if in_flight.as_deref() == Some(write.bytes.as_slice()) {
            *in_flight = None;
        }
        policy.written(write.bytes, ok)
    }

    /// Stop persisting for the rest of this ROM session WITHOUT writing: the
    /// live save RAM now belongs to a movie or netplay session (see
    /// `EmuCore::start_sandboxed_session`). A read still in flight is
    /// discarded when it lands ([`Restored::Stale`]). Returns whether anything
    /// was bound.
    pub fn release(&mut self) -> bool {
        !matches!(core::mem::take(&mut self.state), State::Off)
    }

    /// The final write for the outgoing cartridge (forced, and only when the
    /// save changed), then unbind. Call before a new ROM replaces `nes`.
    pub fn detach(&mut self, nes: Option<&Nes>) -> Option<WebBatteryWrite> {
        let write = nes.and_then(|nes| self.due(nes, true));
        self.state = State::Off;
        write
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::battery_policy::CHECK_PERIOD_FRAMES;

    /// NROM with the battery bit (or without). The program writes `$A5` to
    /// `$6000`, then increments `$6001` forever, so the save RAM keeps
    /// changing (the same program `battery_save`'s tests run):
    ///
    /// ```text
    /// C000: A9 A5     LDA #$A5
    ///       8D 00 60  STA $6000
    /// C005: EE 01 60  INC $6001
    ///       4C 05 C0  JMP $C005
    /// ```
    pub fn rom(battery: bool) -> Vec<u8> {
        let mut v = b"NES\x1A".to_vec();
        v.extend_from_slice(&[1, 1, if battery { 0x02 } else { 0 }, 0]);
        v.resize(16, 0);
        let mut prg = vec![0xEAu8; 0x4000];
        prg[..11].copy_from_slice(&[
            0xA9, 0xA5, 0x8D, 0x00, 0x60, 0xEE, 0x01, 0x60, 0x4C, 0x05, 0xC0,
        ]);
        prg[0x3FFC] = 0x00;
        prg[0x3FFD] = 0xC0;
        v.extend_from_slice(&prg);
        v.extend(core::iter::repeat_n(0u8, 0x2000));
        v
    }

    /// Run `n` frames. One `run_frame` straight after power-on was measured
    /// NOT to reach the program's first store (the save RAM was still blank,
    /// and `Nes::frame()` did not advance), so tests that need the save RAM
    /// changed run three, as `battery_save`'s tests always have.
    fn run(nes: &mut Nes, n: usize) {
        for _ in 0..n {
            nes.run_frame();
        }
    }

    /// Bind and restore from an empty store: armed, from power-on RAM.
    fn armed(nes: &mut Nes) -> WebBattery {
        let mut b = WebBattery::new();
        let sha = b.begin(nes).expect("a battery cart persists");
        assert_eq!(b.restore(nes, sha, Ok(None)), Restored::Empty);
        b
    }

    #[test]
    fn the_key_is_per_rom_and_cannot_meet_a_slot_key() {
        let a = [0xABu8; 32];
        let b = [0xCDu8; 32];
        assert_ne!(battery_key(&a), battery_key(&b));
        assert_eq!(battery_key(&a), format!("{}:battery", "ab".repeat(32)));
        // Every save-state slot key is `<hex>:slot<N>` (wasm_idb::idb_key).
        assert!(!battery_key(&a).contains(":slot"));
        assert_ne!(localstorage_battery_key(&a), battery_key(&a));
        assert!(localstorage_battery_key(&a).starts_with("rustynes-battery-"));
    }

    #[test]
    fn a_stored_save_is_loaded_and_releases_the_gate() {
        let mut nes = Nes::from_rom(&rom(true)).unwrap();
        let mut b = WebBattery::new();
        let sha = b.begin(&nes).unwrap();
        assert!(b.is_pending(), "emulation waits for the read");
        let mut stored = vec![0u8; nes.save_data().len()];
        stored[..2].copy_from_slice(&[0x5A, 0x3C]);
        assert_eq!(
            b.restore(&mut nes, sha, Ok(Some(stored.clone()))),
            Restored::Loaded
        );
        assert!(!b.is_pending() && b.is_armed());
        assert_eq!(nes.save_data(), stored.as_slice(), "the save came back");
        assert!(b.due(&nes, true).is_none(), "and is not written back");
    }

    #[test]
    fn a_cart_without_a_battery_never_gates() {
        let nes = Nes::from_rom(&rom(false)).unwrap();
        let mut b = WebBattery::new();
        assert_eq!(b.begin(&nes), None);
        assert!(!b.is_pending() && !b.is_armed());
    }

    #[test]
    fn an_unusable_record_is_neither_loaded_nor_overwritten() {
        let size = Nes::from_rom(&rom(true)).unwrap().save_data().len();
        // Too short, too long (a record that merely STARTS with a valid save
        // is still not this cartridge's), and an unreadable store.
        for stored in [
            Ok(Some(vec![0x11u8; 100])),
            Ok(Some(vec![0x11u8; size + 1])),
            Err("blocked".to_string()),
        ] {
            let mut nes = Nes::from_rom(&rom(true)).unwrap();
            let mut b = WebBattery::new();
            let sha = b.begin(&nes).unwrap();
            assert!(matches!(
                b.restore(&mut nes, sha, stored),
                Restored::Refused(_)
            ));
            assert!(!b.is_pending(), "the game still runs");
            assert_eq!(nes.save_data()[0], 0, "nothing was loaded");
            run(&mut nes, 3);
            assert!(b.due(&nes, true).is_none(), "and nothing will be written");
        }
    }

    #[test]
    fn a_read_for_another_rom_is_stale() {
        let mut nes = Nes::from_rom(&rom(true)).unwrap();
        let mut b = WebBattery::new();
        let sha = b.begin(&nes).unwrap();
        let mut other = sha;
        other[0] ^= 1;
        let full = Ok(Some(vec![0x77u8; nes.save_data().len()]));
        assert_eq!(b.restore(&mut nes, other, full.clone()), Restored::Stale);
        assert!(b.is_pending(), "still waiting for its own read");
        assert_eq!(nes.save_data()[0], 0);
        // A released session (a movie started) discards its read.
        assert!(b.release());
        assert_eq!(b.restore(&mut nes, sha, full), Restored::Stale);
        assert_eq!(nes.save_data()[0], 0);
    }

    #[test]
    fn the_periodic_write_follows_the_shared_policy() {
        let mut nes = Nes::from_rom(&rom(true)).unwrap();
        let mut b = armed(&mut nes);
        let mut writes = 0;
        for _ in 0..(3 * CHECK_PERIOD_FRAMES) {
            nes.run_frame();
            if let Some(w) = b.due(&nes, false) {
                writes += 1;
                assert_eq!(w.key(), battery_key(nes.rom_sha256()));
                assert!(!b.written(w, true));
            }
        }
        assert_eq!(writes, 3, "one write per period, not one per frame");
    }

    #[test]
    fn no_periodic_write_while_one_is_in_flight() {
        let mut nes = Nes::from_rom(&rom(true)).unwrap();
        let mut b = armed(&mut nes);
        run(&mut nes, 3);
        let first = b.due(&nes, true).expect("the program wrote its RAM");
        for _ in 0..(2 * CHECK_PERIOD_FRAMES) {
            nes.run_frame();
            assert!(b.due(&nes, false).is_none(), "the first has not landed");
        }
        // A forced write goes out anyway when the bytes moved on...
        let second = b.due(&nes, true).expect("page hide: newer bytes");
        assert!(
            b.due(&nes, true).is_none(),
            "...but not the same bytes twice"
        );
        // The older write lands behind the newer one: the newer stays in
        // flight, so periodic writes stay paused until it reports.
        assert!(!b.written(first, true));
        for _ in 0..CHECK_PERIOD_FRAMES {
            assert!(b.due(&nes, false).is_none());
        }
        assert!(!b.written(second, true));
        run(&mut nes, 3);
        let mut resumed = false;
        for _ in 0..CHECK_PERIOD_FRAMES {
            resumed |= b.due(&nes, false).is_some();
        }
        assert!(resumed, "periodic writes resume once nothing is in flight");
    }

    #[test]
    fn a_failed_write_is_retried_and_reported_once() {
        let mut nes = Nes::from_rom(&rom(true)).unwrap();
        let mut b = armed(&mut nes);
        run(&mut nes, 3);
        let w = b.due(&nes, true).unwrap();
        assert!(b.written(w, false), "first failure: report");
        let w = b.due(&nes, true).expect("the baseline did not move: retry");
        assert!(!b.written(w, false), "second failure: quiet");
    }

    #[test]
    fn detach_writes_only_a_change_and_unbinds() {
        let mut nes = Nes::from_rom(&rom(true)).unwrap();
        let mut b = armed(&mut nes);
        assert!(
            b.clone().detach(Some(&nes)).is_none(),
            "unchanged: no write"
        );
        run(&mut nes, 3);
        let w = b.detach(Some(&nes)).expect("the final write");
        assert_eq!(w.bytes()[0], 0xA5);
        assert!(!b.is_armed());
        assert!(!b.written(w, true), "a write after detach is ignored");
    }
}

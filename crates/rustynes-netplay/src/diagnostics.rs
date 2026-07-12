//! Desync diagnostics — observational telemetry for a [`RollbackSession`].
//!
//! [`RollbackSession`]: crate::session::RollbackSession
//!
//! When two (or more) peers exchange periodic confirmed-frame state checksums
//! (the [`Checksum`](crate::message::NetMessage::Checksum) message), a mismatch
//! is a fatal [`NetplayError::Desync`](crate::session::NetplayError::Desync).
//! Before that fatal exit — and on every *matching* comparison too — the
//! session records the comparison here so the frontend can surface a
//! GeraNES-style `DesyncMonitor` view: a rolling CRC-match history, the
//! consecutive-mismatch count, the first frame that diverged, and the most
//! recent local-vs-remote CRC pair.
//!
//! This module is **purely observational**. It only ever *reads* values the
//! session already computed (the canonical gameplay digest + the peer's
//! reported digest) and stores copies; it never feeds back into the rollback
//! algorithm, the checksum exchange, or the emulator. So it cannot change
//! correctness or perturb the determinism contract — disabling it would leave
//! every produced frame, checksum, and rollback byte-identical.

use std::collections::VecDeque;

/// The graded desync verdict derived from the recent comparison history.
///
/// This is the **clear desync surface** the frontend renders: rather than force
/// the panel to re-derive a verdict from the raw counters, [`DesyncDiagnostics`]
/// exposes one enum that folds them together with a hysteresis threshold, so a
/// single out-of-order or momentarily-late peer checksum does not flash a
/// false "desynced" banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesyncStatus {
    /// Every comparison so far matched — the peers are in lockstep.
    InSync,
    /// At least one frame has ever mismatched, but the current consecutive-run
    /// is below the confirm threshold: either a transient (a match has since
    /// reset the run to 0, leaving a sticky historical mismatch) or a
    /// still-building run that has not yet crossed into a confirmed desync.
    Suspect {
        /// The current consecutive-mismatch run (0 if the last compare matched).
        consecutive: u32,
        /// The earliest frame that ever diverged.
        first_desync_frame: u32,
    },
    /// The consecutive-mismatch run has reached the confirm threshold: this is a
    /// real, sustained divergence. Once entered it is **sticky** — a desync is
    /// unrecoverable for a rollback session (the peers can never re-converge
    /// without a full state resync), so the surface never silently downgrades a
    /// confirmed [`Desynced`](Self::Desynced) back to [`Suspect`](Self::Suspect).
    Desynced {
        /// The earliest frame that diverged.
        first_desync_frame: u32,
    },
}

/// One recorded confirmed-frame checksum comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrcCompare {
    /// The confirmed frame whose checksums were compared.
    pub frame: u32,
    /// Our canonical (combined) gameplay digest for the frame.
    pub local: u64,
    /// The peer's reported digest for the frame.
    pub remote: u64,
    /// `true` if `local == remote` (in sync at this frame).
    pub matched: bool,
    /// `true` if the framebuffer-only hashes matched.
    ///
    /// When `!matched` but `same_framebuffer`, only the cumulative cycle term
    /// diverged (a timing bug); when `!matched` and `!same_framebuffer`, the
    /// rendered picture itself diverged (a state bug). Always `true` for a
    /// matched compare.
    pub same_framebuffer: bool,
}

/// A rolling, allocation-bounded record of recent confirmed-frame checksum
/// comparisons, plus the derived desync status.
///
/// Held by a [`RollbackSession`](crate::session::RollbackSession) and surfaced
/// read-only to the frontend's netplay panel.
///
/// The history is a fixed-capacity ring ([`Self::CAPACITY`]); the
/// first-desync frame and the consecutive-mismatch counter are sticky scalars
/// that survive eviction from the ring, so the diagnostics stay meaningful for
/// a long-running session.
#[derive(Clone, Debug)]
pub struct DesyncDiagnostics {
    /// Bounded ring of the most recent comparisons (oldest first).
    history: VecDeque<CrcCompare>,
    /// Total comparisons recorded (matched + mismatched), across all time —
    /// not just those still in the ring.
    total: u64,
    /// Total mismatched comparisons recorded, across all time.
    mismatches: u64,
    /// The earliest frame whose checksums disagreed, if any has yet.
    first_desync_frame: Option<u32>,
    /// Consecutive mismatches ending at the most recent comparison (reset to 0
    /// by any match). A nonzero value means the session is currently diverged.
    consecutive_mismatches: u32,
    /// Peak consecutive-mismatch run ever observed. Drives the sticky
    /// [`DesyncStatus::Desynced`] verdict: once the run has *ever* reached
    /// [`desync_threshold`](Self::desync_threshold) the session is treated as
    /// confirmed-desynced even if a later stray match briefly resets the live
    /// run (a rollback desync is unrecoverable, so the verdict must not flap).
    peak_consecutive: u32,
    /// How many consecutive mismatches confirm a real desync (hysteresis). A
    /// single reordered / late peer checksum stays [`DesyncStatus::Suspect`].
    desync_threshold: u32,
    /// The most recent comparison, for the "local vs remote CRC" readout.
    last: Option<CrcCompare>,
}

impl Default for DesyncDiagnostics {
    fn default() -> Self {
        Self::new()
    }
}

impl DesyncDiagnostics {
    /// Maximum comparisons retained in the rolling history ring. At the default
    /// 30-frame checksum interval (~2 per second) this is ~32 s of history.
    pub const CAPACITY: usize = 64;

    /// Default hysteresis: how many *consecutive* mismatching comparisons
    /// confirm a real desync (vs. a single reordered / late peer checksum).
    ///
    /// A confirmed-frame checksum is only exchanged every `checksum_interval`
    /// frames and covers a *confirmed* frame, so a legitimate one-off mismatch
    /// is nearly impossible on a correct implementation — but a burst-reordered
    /// pair of `Checksum` messages can momentarily disagree before the deferred
    /// `compare_pending_checksums` pass reconciles them. Requiring **3** in a
    /// row (~1.5 s at the default interval) rejects that transient while still
    /// declaring a genuine divergence promptly.
    pub const DEFAULT_DESYNC_THRESHOLD: u32 = 3;

    /// A fresh, empty diagnostics record with the default desync threshold.
    #[must_use]
    pub fn new() -> Self {
        Self::with_threshold(Self::DEFAULT_DESYNC_THRESHOLD)
    }

    /// A fresh, empty diagnostics record with an explicit confirm threshold
    /// (consecutive mismatches before [`DesyncStatus::Desynced`]). A `0` is
    /// treated as `1` (the very first mismatch confirms).
    #[must_use]
    pub fn with_threshold(desync_threshold: u32) -> Self {
        Self {
            history: VecDeque::with_capacity(Self::CAPACITY),
            total: 0,
            mismatches: 0,
            first_desync_frame: None,
            consecutive_mismatches: 0,
            peak_consecutive: 0,
            desync_threshold: desync_threshold.max(1),
            last: None,
        }
    }

    /// Record one confirmed-frame checksum comparison.
    ///
    /// `local`/`remote` are the combined gameplay digests; `local_fb`/`remote_fb`
    /// are the framebuffer-only hashes (used only to classify a mismatch). This
    /// is the single mutation point and is called by the session at each compare
    /// site (the live `Checksum` ingest and the deferred
    /// `compare_pending_checksums` pass). It is idempotent in spirit: the session
    /// compares each confirmed frame at most once, so no frame is double-counted.
    pub fn record(&mut self, frame: u32, local: u64, remote: u64, local_fb: u64, remote_fb: u64) {
        let matched = local == remote;
        let entry = CrcCompare {
            frame,
            local,
            remote,
            matched,
            same_framebuffer: local_fb == remote_fb,
        };
        self.total += 1;
        if matched {
            self.consecutive_mismatches = 0;
        } else {
            self.mismatches += 1;
            self.consecutive_mismatches = self.consecutive_mismatches.saturating_add(1);
            self.peak_consecutive = self.peak_consecutive.max(self.consecutive_mismatches);
            self.first_desync_frame = Some(self.first_desync_frame.map_or(frame, |f| f.min(frame)));
        }
        if self.history.len() == Self::CAPACITY {
            self.history.pop_front();
        }
        self.history.push_back(entry);
        self.last = Some(entry);
    }

    /// `true` if no mismatch has ever been recorded.
    #[must_use]
    pub const fn in_sync(&self) -> bool {
        self.first_desync_frame.is_none()
    }

    /// The graded [`DesyncStatus`] verdict — the frontend's single desync
    /// surface. Applies the hysteresis threshold and the sticky-once-confirmed
    /// rule (see [`DesyncStatus`]).
    #[must_use]
    pub const fn status(&self) -> DesyncStatus {
        match self.first_desync_frame {
            None => DesyncStatus::InSync,
            Some(first) => {
                // Confirmed if the run has EVER reached the threshold (sticky):
                // a rollback desync cannot recover, so never downgrade it.
                if self.peak_consecutive >= self.desync_threshold {
                    DesyncStatus::Desynced {
                        first_desync_frame: first,
                    }
                } else {
                    DesyncStatus::Suspect {
                        consecutive: self.consecutive_mismatches,
                        first_desync_frame: first,
                    }
                }
            }
        }
    }

    /// `true` once the consecutive-mismatch run has ever reached the confirm
    /// threshold — i.e. [`status`](Self::status) is
    /// [`DesyncStatus::Desynced`]. Convenience for a boolean gate.
    #[must_use]
    pub const fn is_desynced(&self) -> bool {
        matches!(self.status(), DesyncStatus::Desynced { .. })
    }

    /// The confirm threshold in effect (consecutive mismatches → confirmed
    /// desync).
    #[must_use]
    pub const fn desync_threshold(&self) -> u32 {
        self.desync_threshold
    }

    /// Peak consecutive-mismatch run ever observed (survives a later match).
    #[must_use]
    pub const fn peak_consecutive_mismatches(&self) -> u32 {
        self.peak_consecutive
    }

    /// The earliest frame whose checksums disagreed, if any.
    #[must_use]
    pub const fn first_desync_frame(&self) -> Option<u32> {
        self.first_desync_frame
    }

    /// Consecutive mismatches ending at the most recent comparison (0 if the
    /// last compared frame matched).
    #[must_use]
    pub const fn consecutive_mismatches(&self) -> u32 {
        self.consecutive_mismatches
    }

    /// Total comparisons recorded across the whole session.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }

    /// Total mismatched comparisons recorded across the whole session.
    #[must_use]
    pub const fn mismatches(&self) -> u64 {
        self.mismatches
    }

    /// The most recent comparison (for the local-vs-remote CRC readout), if any.
    #[must_use]
    pub const fn last(&self) -> Option<CrcCompare> {
        self.last
    }

    /// The rolling history (oldest first), newest at the back.
    pub fn history(&self) -> impl ExactSizeIterator<Item = &CrcCompare> {
        self.history.iter()
    }

    /// Number of entries currently in the rolling history ring.
    #[must_use]
    pub fn history_len(&self) -> usize {
        self.history.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_is_in_sync() {
        let d = DesyncDiagnostics::new();
        assert!(d.in_sync());
        assert_eq!(d.first_desync_frame(), None);
        assert_eq!(d.consecutive_mismatches(), 0);
        assert_eq!(d.total(), 0);
        assert_eq!(d.mismatches(), 0);
        assert_eq!(d.last(), None);
        assert_eq!(d.history_len(), 0);
    }

    #[test]
    fn matched_compares_stay_in_sync() {
        let mut d = DesyncDiagnostics::new();
        for f in (0..150).step_by(30) {
            d.record(f, 0xAAAA, 0xAAAA, 0x1111, 0x1111);
        }
        assert!(d.in_sync());
        assert_eq!(d.first_desync_frame(), None);
        assert_eq!(d.consecutive_mismatches(), 0);
        assert_eq!(d.total(), 5);
        assert_eq!(d.mismatches(), 0);
        let last = d.last().expect("a compare was recorded");
        assert!(last.matched);
        assert_eq!(last.frame, 120);
    }

    #[test]
    fn first_mismatch_records_desync_frame() {
        let mut d = DesyncDiagnostics::new();
        d.record(30, 0x1, 0x1, 0, 0); // match
        d.record(60, 0x2, 0x9, 0, 0); // first desync
        d.record(90, 0x3, 0x8, 0, 0); // another desync (later frame)
        assert!(!d.in_sync());
        assert_eq!(d.first_desync_frame(), Some(60), "earliest divergent frame");
        assert_eq!(d.consecutive_mismatches(), 2);
        assert_eq!(d.mismatches(), 2);
        assert_eq!(d.total(), 3);
    }

    #[test]
    fn first_desync_frame_is_the_minimum_even_out_of_order() {
        // Frames can be compared out of order (a burst of confirmations); the
        // first-desync frame must be the minimum mismatching frame, not the
        // first one recorded.
        let mut d = DesyncDiagnostics::new();
        d.record(90, 0x3, 0x8, 0, 0);
        d.record(60, 0x2, 0x9, 0, 0);
        assert_eq!(d.first_desync_frame(), Some(60));
    }

    #[test]
    fn consecutive_counter_resets_on_match() {
        let mut d = DesyncDiagnostics::new();
        d.record(30, 1, 9, 0, 0); // mismatch
        d.record(60, 2, 8, 0, 0); // mismatch
        assert_eq!(d.consecutive_mismatches(), 2);
        d.record(90, 3, 3, 0, 0); // match -> reset
        assert_eq!(d.consecutive_mismatches(), 0);
        // A later mismatch starts the run over, but first_desync_frame is sticky.
        d.record(120, 4, 7, 0, 0);
        assert_eq!(d.consecutive_mismatches(), 1);
        assert_eq!(d.first_desync_frame(), Some(30));
    }

    #[test]
    fn same_framebuffer_classifies_mismatch_kind() {
        let mut d = DesyncDiagnostics::new();
        // Same picture, divergent combined digest -> timing/cycle desync.
        d.record(30, 0xAA, 0xBB, 0x77, 0x77);
        let e = d.last().unwrap();
        assert!(!e.matched);
        assert!(e.same_framebuffer, "fb hashes equal -> timing divergence");
        // Divergent picture too -> state desync.
        d.record(60, 0xAA, 0xBB, 0x77, 0x88);
        let e = d.last().unwrap();
        assert!(!e.same_framebuffer, "fb hashes differ -> state divergence");
    }

    #[test]
    fn history_is_bounded_but_scalars_survive_eviction() {
        let mut d = DesyncDiagnostics::new();
        // A mismatch early, then enough matches to evict it from the ring.
        d.record(0, 1, 9, 0, 0); // mismatch at frame 0
        let extra = u32::try_from(DesyncDiagnostics::CAPACITY).unwrap() + 10;
        for i in 1..=extra {
            d.record(i * 30, 5, 5, 0, 0);
        }
        // The ring is capped...
        assert_eq!(d.history_len(), DesyncDiagnostics::CAPACITY);
        // ...the early mismatch has been evicted from the ring...
        assert!(d.history().all(|e| e.frame != 0));
        // ...but the sticky first-desync frame + lifetime totals survive.
        assert_eq!(d.first_desync_frame(), Some(0));
        assert_eq!(d.mismatches(), 1);
        assert_eq!(
            usize::try_from(d.total()).unwrap(),
            DesyncDiagnostics::CAPACITY + 11
        );
        // The most recent compare matched, so the consecutive run is 0.
        assert_eq!(d.consecutive_mismatches(), 0);
    }

    #[test]
    fn status_applies_hysteresis_then_confirms_and_sticks() {
        let mut d = DesyncDiagnostics::with_threshold(3);
        assert_eq!(d.status(), DesyncStatus::InSync);

        // One mismatch: suspect, not confirmed (below threshold 3).
        d.record(30, 1, 9, 0, 0);
        assert_eq!(
            d.status(),
            DesyncStatus::Suspect {
                consecutive: 1,
                first_desync_frame: 30
            }
        );
        assert!(!d.is_desynced());

        // A match resets the live run but leaves the sticky first-desync frame:
        // still merely suspect (a transient).
        d.record(60, 5, 5, 0, 0);
        assert_eq!(
            d.status(),
            DesyncStatus::Suspect {
                consecutive: 0,
                first_desync_frame: 30
            }
        );

        // Three consecutive mismatches reach the threshold → confirmed desync.
        d.record(90, 1, 2, 0, 0);
        d.record(120, 1, 2, 0, 0);
        d.record(150, 1, 2, 0, 0);
        assert_eq!(
            d.status(),
            DesyncStatus::Desynced {
                first_desync_frame: 30
            }
        );
        assert!(d.is_desynced());
        assert_eq!(d.peak_consecutive_mismatches(), 3);

        // A later stray match must NOT downgrade a confirmed desync (sticky).
        d.record(180, 7, 7, 0, 0);
        assert_eq!(
            d.status(),
            DesyncStatus::Desynced {
                first_desync_frame: 30
            }
        );
    }

    #[test]
    fn threshold_zero_is_treated_as_one() {
        let mut d = DesyncDiagnostics::with_threshold(0);
        assert_eq!(d.desync_threshold(), 1);
        d.record(30, 1, 2, 0, 0);
        assert!(d.is_desynced(), "first mismatch confirms at threshold 1");
    }

    #[test]
    fn history_preserves_order_oldest_first() {
        let mut d = DesyncDiagnostics::new();
        d.record(30, 0, 0, 0, 0);
        d.record(60, 0, 0, 0, 0);
        d.record(90, 0, 0, 0, 0);
        let frames: Vec<u32> = d.history().map(|e| e.frame).collect();
        assert_eq!(frames, vec![30, 60, 90]);
    }
}

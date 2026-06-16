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

    /// A fresh, empty diagnostics record.
    #[must_use]
    pub fn new() -> Self {
        Self {
            history: VecDeque::with_capacity(Self::CAPACITY),
            total: 0,
            mismatches: 0,
            first_desync_frame: None,
            consecutive_mismatches: 0,
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
    fn history_preserves_order_oldest_first() {
        let mut d = DesyncDiagnostics::new();
        d.record(30, 0, 0, 0, 0);
        d.record(60, 0, 0, 0, 0);
        d.record(90, 0, 0, 0, 0);
        let frames: Vec<u32> = d.history().map(|e| e.frame).collect();
        assert_eq!(frames, vec![30, 60, 90]);
    }
}

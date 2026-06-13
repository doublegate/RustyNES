//! Rewind ring buffer.
//!
//! Per `to-dos/phase-5-frontend-tooling/sprint-2-save-rewind.md` T-52-004 /
//! T-52-005. The rewind ring captures one entry per emulated frame, then
//! lets the caller step backwards through history one entry at a time. The
//! frontend's `F5`-held UX uses [`Self::pop_back`] to restore the previous
//! state on every redraw.
//!
//! # Memory budget
//!
//! Target: 60 s @ 60 fps in ≤ 32 MiB. That leaves ~9 KiB per snapshot.
//! Raw chip state (CPU + PPU + APU + mappers minus large arrays) is on
//! the order of a few KiB; the heavy hitters are the framebuffer
//! (256×240×4 = 245,760 B) and PRG-RAM (up to 32 KiB on MMC3/MMC5).
//!
//! Strategy: every `keyframe_period` frames we store a full LZ4-compressed
//! snapshot; in between we store an LZ4-compressed XOR delta against the
//! most recent keyframe. NES screen content changes slowly, so the deltas
//! compress aggressively (most bytes are 0).
//!
//! # Restore semantics
//!
//! [`Self::pop_back`] returns the most recent buffered snapshot bytes
//! (after delta-applying against the keyframe) and removes that entry from
//! the ring. Calling repeatedly walks back in time. Once the ring is
//! drained, [`Self::pop_back`] returns `None`.

// `alloc::collections::VecDeque` is the same type as `std::collections::VecDeque`
// (std re-exports from alloc). Using the alloc path keeps this module portable
// to `#![no_std]` consumers. See `docs/architecture.md` §no_std migration. The
// remaining blocker for actual `#![no_std]` on `rustynes-core` is `thiserror = "1.0"`
// (std-only); upgrade to `thiserror = "2.0"` (core::error::Error) is tracked
// separately.
extern crate alloc;
use alloc::collections::VecDeque;
use alloc::{boxed::Box, string::String, vec::Vec};
use alloc::{format, vec};

use lz4_flex::block::{compress_prepend_size, decompress_size_prepended};
use thiserror::Error;

/// Default ring capacity in bytes — 60 s × 60 fps with our typical
/// compression ratio fits comfortably under 32 MiB.
pub const REWIND_DEFAULT_MAX_BYTES: usize = 32 * 1024 * 1024;

/// Default keyframe period (1 keyframe per second of capture).
pub const REWIND_DEFAULT_KEYFRAME_PERIOD: u32 = 60;

/// Errors raised by [`RewindRing::pop_back`] when an entry can't be
/// reconstructed.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RewindError {
    /// The keyframe needed to delta-apply this entry was already evicted.
    #[error("rewind keyframe missing for delta entry")]
    MissingKeyframe,
    /// LZ4 decompression failed (corrupt entry).
    #[error("rewind LZ4 decompress: {0}")]
    Decompress(String),
    /// Snapshot lengths after delta-application don't match.
    #[error("rewind delta length mismatch: keyframe {kf} bytes, delta {dl} bytes")]
    LengthMismatch {
        /// Keyframe length.
        kf: usize,
        /// Delta length.
        dl: usize,
    },
}

#[derive(Debug, Clone)]
enum Body {
    /// LZ4-compressed full snapshot. Decompresses to the raw snapshot bytes.
    Keyframe(Box<[u8]>),
    /// LZ4-compressed XOR delta against the most recent keyframe.
    /// Decompresses to a `Vec<u8>` of the same length as the keyframe;
    /// applying byte-XOR with the keyframe reconstructs the snapshot.
    Delta(Box<[u8]>),
}

/// One ring entry: a frame index + compressed body.
#[derive(Debug, Clone)]
struct Entry {
    frame: u64,
    body: Body,
    /// Index of the keyframe entry this delta refers to. Same as `self`'s
    /// position for keyframes; otherwise the index of the most recent
    /// keyframe at capture time. We resolve by walking backward from the
    /// delta to find the closest keyframe still in the buffer (deque
    /// shuffling makes any cached index unstable).
    is_keyframe: bool,
    /// Approximate in-memory size in bytes of this entry. Used by the
    /// `max_bytes` eviction policy.
    approx_bytes: usize,
}

/// Rewind ring buffer.
///
/// Configure once via [`Self::new`] (or [`Self::default`]); call
/// [`Self::push`] after each emulated frame; call [`Self::pop_back`] to
/// step backwards in time during rewind playback.
#[derive(Debug)]
pub struct RewindRing {
    entries: VecDeque<Entry>,
    /// Soft byte cap — entries are evicted from the front until satisfied.
    max_bytes: usize,
    /// Current accumulated byte count (sum of `approx_bytes`).
    cur_bytes: usize,
    /// Captures since the last keyframe (counts both kinds).
    since_keyframe: u32,
    /// Keyframe interval — every Nth call to [`Self::push`] forces a
    /// keyframe.
    keyframe_period: u32,
    /// The most recent keyframe's decompressed snapshot bytes, cached so
    /// that subsequent deltas can be reconstructed without re-decompressing
    /// it for every push.
    last_keyframe_decoded: Option<Vec<u8>>,
    /// v2.8.0 Phase 3 — reused scratch for the per-push XOR delta (kills a
    /// ~250 KiB allocation on every non-keyframe capture).
    delta_scratch: Vec<u8>,
}

impl Default for RewindRing {
    fn default() -> Self {
        Self::new(REWIND_DEFAULT_MAX_BYTES, REWIND_DEFAULT_KEYFRAME_PERIOD)
    }
}

impl RewindRing {
    /// New ring with `max_bytes` of in-memory budget and a keyframe every
    /// `keyframe_period` captures.
    ///
    /// `keyframe_period` of 0 is treated as 1 (every entry is a keyframe).
    #[must_use]
    pub fn new(max_bytes: usize, keyframe_period: u32) -> Self {
        Self {
            entries: VecDeque::new(),
            max_bytes,
            cur_bytes: 0,
            since_keyframe: 0,
            keyframe_period: keyframe_period.max(1),
            last_keyframe_decoded: None,
            delta_scratch: Vec::new(),
        }
    }

    /// Number of buffered entries (each one is a frame).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if no frames are buffered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Approximate memory footprint in bytes.
    #[must_use]
    pub const fn bytes_used(&self) -> usize {
        self.cur_bytes
    }

    /// Drop every entry.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.cur_bytes = 0;
        self.since_keyframe = 0;
        self.last_keyframe_decoded = None;
    }

    /// Capture one frame's worth of state.
    ///
    /// `frame` is informational; pass the emulator's frame counter so
    /// debug output can label entries.
    pub fn push(&mut self, frame: u64, snapshot: &[u8]) {
        // Decide keyframe vs delta. Force keyframe on first push, when
        // there's no keyframe to delta against, or every Nth push.
        let force_keyframe =
            self.last_keyframe_decoded.is_none() || self.since_keyframe + 1 >= self.keyframe_period;
        let is_keyframe = force_keyframe;

        let body_bytes: Box<[u8]>;
        let approx;

        if is_keyframe {
            // Compress the full snapshot.
            let compressed = compress_prepend_size(snapshot);
            approx = compressed.len();
            body_bytes = compressed.into_boxed_slice();
            // Stash the decoded keyframe so future deltas can XOR against it.
            self.last_keyframe_decoded = Some(snapshot.to_vec());
            self.since_keyframe = 0;
        } else {
            // Build a XOR delta against the cached keyframe. If lengths
            // disagree (e.g. the snapshot grew because a chip's state shape
            // changed mid-run, which shouldn't happen for a stable build),
            // emit a keyframe instead.
            let kf = self
                .last_keyframe_decoded
                .as_ref()
                .expect("keyframe cached on non-first push");
            if kf.len() != snapshot.len() {
                let compressed = compress_prepend_size(snapshot);
                approx = compressed.len();
                body_bytes = compressed.into_boxed_slice();
                self.last_keyframe_decoded = Some(snapshot.to_vec());
                self.since_keyframe = 0;
                self.entries.push_back(Entry {
                    frame,
                    body: Body::Keyframe(body_bytes),
                    is_keyframe: true,
                    approx_bytes: approx,
                });
                self.cur_bytes += approx;
                self.evict_to_budget();
                return;
            }
            // v2.8.0 Phase 3 — reuse the scratch (resize is a no-op in
            // steady state; snapshot shape is stable within a run).
            // Phase 4b — zip instead of indexing: no per-byte bounds
            // checks, so LLVM auto-vectorizes the XOR (pure integer —
            // trivially bit-identical).
            self.delta_scratch.resize(snapshot.len(), 0);
            for ((slot, &s), &k) in self.delta_scratch.iter_mut().zip(snapshot).zip(kf.iter()) {
                *slot = s ^ k;
            }
            let compressed = compress_prepend_size(&self.delta_scratch);
            approx = compressed.len();
            body_bytes = compressed.into_boxed_slice();
            self.since_keyframe += 1;
        }

        let body = if is_keyframe {
            Body::Keyframe(body_bytes)
        } else {
            Body::Delta(body_bytes)
        };
        self.entries.push_back(Entry {
            frame,
            body,
            is_keyframe,
            approx_bytes: approx,
        });
        let _ = snapshot.len(); // placeholder for future schema use
        self.cur_bytes += approx;
        self.evict_to_budget();
    }

    /// Pop the most recent entry and return its decoded snapshot bytes.
    ///
    /// Returns `None` when the ring is empty. Walking back across a
    /// keyframe boundary is allowed; once the keyframe itself is consumed,
    /// the next pop reads its predecessor (which must itself be a
    /// keyframe — by construction, since deltas refer forward to a
    /// keyframe).
    ///
    /// # Errors
    ///
    /// Returns [`RewindError`] when an entry can't be reconstructed
    /// (corrupt LZ4 payload, etc.).
    pub fn pop_back(&mut self) -> Option<Result<Vec<u8>, RewindError>> {
        let entry = self.entries.pop_back()?;
        self.cur_bytes = self.cur_bytes.saturating_sub(entry.approx_bytes);
        let result = self.decode_entry(&entry);

        // Maintain `last_keyframe_decoded`: after popping, find the new
        // most-recent keyframe if any, decode it, and cache.
        self.refresh_keyframe_cache();

        Some(result)
    }

    /// Borrow the most recent entry's frame number, if any.
    #[must_use]
    pub fn back_frame(&self) -> Option<u64> {
        self.entries.back().map(|e| e.frame)
    }

    fn decode_entry(&self, entry: &Entry) -> Result<Vec<u8>, RewindError> {
        match &entry.body {
            Body::Keyframe(b) => {
                decompress_size_prepended(b).map_err(|e| RewindError::Decompress(format!("{e}")))
            }
            Body::Delta(b) => {
                let kf = self
                    .last_keyframe_decoded
                    .as_ref()
                    .ok_or(RewindError::MissingKeyframe)?;
                let delta = decompress_size_prepended(b)
                    .map_err(|e| RewindError::Decompress(format!("{e}")))?;
                if delta.len() != kf.len() {
                    return Err(RewindError::LengthMismatch {
                        kf: kf.len(),
                        dl: delta.len(),
                    });
                }
                let mut out = vec![0u8; kf.len()];
                for (i, slot) in out.iter_mut().enumerate() {
                    *slot = kf[i] ^ delta[i];
                }
                Ok(out)
            }
        }
    }

    fn refresh_keyframe_cache(&mut self) {
        // Find the rightmost keyframe and decode it. If none, drop the cache.
        let kf_idx = self.entries.iter().rposition(|e| e.is_keyframe);
        match kf_idx {
            Some(i) => {
                let entry = self.entries.get(i).cloned().expect("indexed entry exists");
                let decoded = match &entry.body {
                    Body::Keyframe(b) => decompress_size_prepended(b).ok(),
                    Body::Delta(_) => None,
                };
                self.last_keyframe_decoded = decoded;
            }
            None => self.last_keyframe_decoded = None,
        }
    }

    fn evict_to_budget(&mut self) {
        while self.cur_bytes > self.max_bytes {
            let Some(front) = self.entries.pop_front() else {
                self.cur_bytes = 0;
                return;
            };
            self.cur_bytes = self.cur_bytes.saturating_sub(front.approx_bytes);
            // After evicting we may have orphaned deltas at the front
            // (leading deltas without a keyframe). Drop those.
            while let Some(e) = self.entries.front() {
                if e.is_keyframe {
                    break;
                }
                let bytes = e.approx_bytes;
                self.entries.pop_front();
                self.cur_bytes = self.cur_bytes.saturating_sub(bytes);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_payload(size: usize, fill: u8) -> Vec<u8> {
        vec![fill; size]
    }

    #[test]
    fn push_and_pop_back_roundtrips() {
        let mut r = RewindRing::new(1024 * 1024, 4);
        let p1 = make_payload(256, 0x11);
        let p2 = make_payload(256, 0x22);
        r.push(1, &p1);
        r.push(2, &p2);
        assert_eq!(r.len(), 2);
        let got2 = r.pop_back().unwrap().unwrap();
        assert_eq!(got2, p2);
        let got1 = r.pop_back().unwrap().unwrap();
        assert_eq!(got1, p1);
        assert!(r.is_empty());
        assert!(r.pop_back().is_none());
    }

    #[test]
    fn keyframe_period_inserts_keyframes() {
        let mut r = RewindRing::new(1024 * 1024, 3);
        for i in 0..7u8 {
            r.push(u64::from(i), &make_payload(64, i));
        }
        // Indexes 0, 3, 6 are keyframes.
        assert!(r.entries[0].is_keyframe);
        assert!(!r.entries[1].is_keyframe);
        assert!(!r.entries[2].is_keyframe);
        assert!(r.entries[3].is_keyframe);
        assert!(!r.entries[4].is_keyframe);
        assert!(!r.entries[5].is_keyframe);
        assert!(r.entries[6].is_keyframe);
    }

    #[test]
    fn delta_round_trip_through_keyframe_boundary() {
        let mut r = RewindRing::new(1024 * 1024, 3);
        let mut payloads = Vec::new();
        for i in 0..10u8 {
            let p = make_payload(64, i);
            payloads.push(p.clone());
            r.push(u64::from(i), &p);
        }
        for i in (0..10u8).rev() {
            let got = r.pop_back().unwrap().unwrap();
            assert_eq!(got, payloads[usize::from(i)], "frame {i}");
        }
        assert!(r.is_empty());
    }

    #[test]
    fn budget_eviction_drops_oldest_first() {
        // Tiny budget that holds maybe 2 small entries. Use random-ish
        // payloads so LZ4 can't crush them into a couple of bytes.
        let mut r = RewindRing::new(80, 1); // every entry is a keyframe
        for i in 0..5u8 {
            // Pseudo-random fill: each byte unique within the payload.
            let mut p = vec![0u8; 256];
            for (j, slot) in p.iter_mut().enumerate() {
                *slot = u8::try_from(j & 0xFF)
                    .unwrap_or(0)
                    .wrapping_mul(31)
                    .wrapping_add(i.wrapping_mul(17));
            }
            r.push(u64::from(i), &p);
        }
        assert!(r.bytes_used() <= 80);
        assert!(r.len() < 5);
    }

    #[test]
    fn clear_resets_state() {
        let mut r = RewindRing::default();
        for i in 0..5u8 {
            r.push(u64::from(i), &make_payload(64, i));
        }
        r.clear();
        assert_eq!(r.len(), 0);
        assert_eq!(r.bytes_used(), 0);
    }
}

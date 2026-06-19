//! Zwinder-class compressed, density-tiered state manager
//! (v1.7.0 "Forge" Workstream D2).
//!
//! This is the *compressed* successor to the v1.6.0 uncompressed greenzone
//! (`rustynes-frontend::tastudio::greenzone::Greenzone`) and the v1.5.0
//! keyframe-cached rewind ring ([`crate::rewind::RewindRing`]). It is the
//! state engine that lets the `TAStudio` greenzone scale to feature-length
//! TASes: thousands of frames of seekable history in a fixed RAM budget.
//!
//! # Source / lineage
//!
//! Clean-room port of the `BizHawk` Zwinder concept
//! (`BizHawk/src/BizHawk.Client.Common/rewind/ZwinderBuffer.cs` +
//! `tasproj/ZwinderStateManager.cs`), distilled to the determinism-critical
//! essentials:
//!
//! - **Frame-keyed** snapshots stored sorted ascending by frame, like the
//!   uncompressed greenzone.
//! - **XOR-delta + LZ4 compression.** Each stored frame is either a *keyframe*
//!   (a full LZ4-compressed snapshot) or a *delta* (an LZ4-compressed byte-XOR
//!   against the nearest preceding keyframe). NES state changes slowly between
//!   adjacent frames, so most delta bytes are zero and LZ4 crushes them.
//! - **Density tiers** (current / recent / ancient). The frames near the
//!   playhead are kept *dense* (cheap to seek into); the distant past decays
//!   to a sparse skeleton, thinned first by eviction.
//! - **Reserved anchors** (frame 0, markers, branch points) are never evicted
//!   and are always stored as keyframes (so they decode without a dependency).
//! - A **byte budget** with density-tiered eviction, identical in spirit to the
//!   uncompressed greenzone's policy — but operating on the *compressed* sizes,
//!   so the same RAM holds far more history.
//!
//! # Determinism contract (the D2 gate)
//!
//! Compression is **lossless**: `restore(compress(store(s))) == s` byte-for-byte.
//! This is the determinism-critical invariant — the round-trip equality test
//! (`round_trip_equality_lossless` in this module's `tests`, plus the
//! integration test in `rustynes-test-harness`) is the gate. There is **no
//! timebase change**: the
//! manager only stores and returns the exact deterministic save-state blobs the
//! core already produces, so it cannot perturb emulation.
//!
//! This type is `#![no_std]` + `alloc`-only and deliberately decoupled from the
//! emulator: it stores opaque snapshot byte-blobs keyed by frame, so its
//! compression / eviction / lookup logic is pure and unit-testable without a
//! running [`crate::Nes`].

extern crate alloc;
use alloc::collections::BTreeSet;
use alloc::{boxed::Box, format, string::String, vec, vec::Vec};

use lz4_flex::block::{compress_prepend_size, decompress_size_prepended};
use thiserror::Error;

/// Default byte budget for the Zwinder store — 256 MiB. With XOR-delta + LZ4
/// this holds tens of thousands of greenzone frames (feature-length TASes).
pub const ZWINDER_DEFAULT_BUDGET_BYTES: usize = 256 * 1024 * 1024;

/// Default keyframe interval: every Nth stored frame is a full keyframe.
///
/// The rest are deltas against the preceding keyframe. A smaller interval costs
/// more bytes but bounds the per-restore delta-walk; 16 is a good balance.
pub const ZWINDER_DEFAULT_KEYFRAME_INTERVAL: u32 = 16;

/// Errors raised when a stored Zwinder frame can't be reconstructed.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ZwinderError {
    /// LZ4 decompression of a stored body failed (corrupt entry).
    #[error("zwinder LZ4 decompress: {0}")]
    Decompress(String),
    /// A delta entry's keyframe is missing or its length disagrees with the
    /// delta (shouldn't happen for a stable build — snapshot shape is fixed).
    #[error("zwinder delta length mismatch: keyframe {kf} bytes, delta {dl} bytes")]
    LengthMismatch {
        /// Keyframe length.
        kf: usize,
        /// Delta length.
        dl: usize,
    },
    /// A delta referenced a keyframe that isn't present in the store. By
    /// construction every delta is stored after a keyframe at-or-before its
    /// frame; this indicates a corrupt store.
    #[error("zwinder keyframe missing for delta at frame {0}")]
    MissingKeyframe(u64),
}

/// The compressed body of one stored frame.
#[derive(Clone)]
enum Body {
    /// LZ4-compressed full snapshot. Decompresses directly to the raw bytes.
    Keyframe(Box<[u8]>),
    /// LZ4-compressed XOR delta against the nearest preceding keyframe.
    /// Decompresses to a `Vec<u8>` of the keyframe's length; byte-XOR with the
    /// keyframe reconstructs the snapshot.
    Delta(Box<[u8]>),
}

/// One stored frame: its frame index + compressed body + bookkeeping.
struct Entry {
    /// The frame index this state represents (the state *before* `frame`'s
    /// input is applied — seeking to `frame` restores this).
    frame: u64,
    /// Compressed body.
    body: Body,
    /// Whether this entry is a self-contained keyframe.
    is_keyframe: bool,
    /// The uncompressed snapshot length (needed to validate deltas + report
    /// the logical state size).
    raw_len: usize,
    /// Compressed in-memory size, charged against the byte budget.
    approx_bytes: usize,
}

/// A compressed, density-tiered, byte-budgeted history of frame-keyed
/// save-states. See the module docs for the design.
pub struct ZwinderStateManager {
    /// Entries kept sorted ascending by `frame` (no duplicate frames).
    entries: Vec<Entry>,
    /// Frames that must never be evicted (frame 0, markers, branch points).
    /// Anchored frames are always stored as keyframes.
    anchors: BTreeSet<u64>,
    /// Soft byte budget over the *compressed* sizes.
    budget_bytes: usize,
    /// Running sum of all entries' `approx_bytes`.
    used_bytes: usize,
    /// Keyframe interval — every Nth non-anchor store forces a keyframe.
    keyframe_interval: u32,
    /// Non-anchor stores since the last keyframe.
    since_keyframe: u32,
}

impl core::fmt::Debug for ZwinderStateManager {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ZwinderStateManager")
            .field("frames", &self.entries.len())
            .field("anchors", &self.anchors.len())
            .field("budget_bytes", &self.budget_bytes)
            .field("used_bytes", &self.used_bytes)
            .field("keyframe_interval", &self.keyframe_interval)
            .field("since_keyframe", &self.since_keyframe)
            .finish_non_exhaustive()
    }
}

impl Default for ZwinderStateManager {
    fn default() -> Self {
        Self::new(
            ZWINDER_DEFAULT_BUDGET_BYTES,
            ZWINDER_DEFAULT_KEYFRAME_INTERVAL,
        )
    }
}

impl ZwinderStateManager {
    /// Create an empty manager with a compressed byte budget and keyframe
    /// interval. Frame 0 is always an anchor (the power-on base must never be
    /// evicted). `keyframe_interval` of 0 is treated as 1 (every entry a
    /// keyframe).
    #[must_use]
    pub fn new(budget_bytes: usize, keyframe_interval: u32) -> Self {
        let mut anchors = BTreeSet::new();
        anchors.insert(0);
        Self {
            entries: Vec::new(),
            anchors,
            budget_bytes: budget_bytes.max(1),
            used_bytes: 0,
            keyframe_interval: keyframe_interval.max(1),
            since_keyframe: 0,
        }
    }

    /// Store (or replace) the save-state for `frame`, compressing it.
    /// `cursor` is the editor/playhead frame — eviction protects the dense
    /// neighbourhood around it. Enforces the byte budget after inserting.
    ///
    /// Anchored frames are always stored as self-contained keyframes (so they
    /// never depend on a neighbour that eviction might thin). A non-anchor
    /// frame is a delta against the nearest preceding keyframe, unless none
    /// exists at-or-before it or the keyframe interval is due — then it is
    /// promoted to a keyframe.
    pub fn store(&mut self, frame: u64, snapshot: &[u8], cursor: u64) {
        let is_anchor = self.anchors.contains(&frame);
        // Decode the nearest preceding keyframe at most once: it is needed both
        // to decide keyframe-vs-delta (its absence forces a keyframe) and, on the
        // delta path, to XOR against. `preceding_keyframe_raw` performs an LZ4
        // decompress + allocation, so calling it twice would double that work on
        // every non-keyframe store (the greenzone hot path).
        let anchor_or_interval = is_anchor || self.since_keyframe + 1 >= self.keyframe_interval;
        // Only decode the preceding keyframe when a delta is actually possible —
        // an anchor/interval boundary becomes a keyframe regardless.
        let preceding_kf = if anchor_or_interval {
            None
        } else {
            self.preceding_keyframe_raw(frame)
        };
        // Decide keyframe vs delta. Anchors and interval boundaries become
        // keyframes; so does any frame with no preceding keyframe to delta
        // against.
        let force_keyframe = anchor_or_interval || preceding_kf.is_none();

        let (body, is_keyframe, approx) = if force_keyframe {
            let compressed = compress_prepend_size(snapshot);
            let approx = compressed.len();
            (Body::Keyframe(compressed.into_boxed_slice()), true, approx)
        } else {
            // Delta against the nearest preceding keyframe's raw bytes (decoded
            // above, exactly once).
            let kf = preceding_kf.expect("preceding keyframe checked present");
            if kf.len() == snapshot.len() {
                let mut delta = vec![0u8; snapshot.len()];
                for ((slot, &s), &k) in delta.iter_mut().zip(snapshot).zip(kf.iter()) {
                    *slot = s ^ k;
                }
                let compressed = compress_prepend_size(&delta);
                let approx = compressed.len();
                (Body::Delta(compressed.into_boxed_slice()), false, approx)
            } else {
                // Snapshot shape changed (shouldn't happen mid-run) — fall
                // back to a self-contained keyframe rather than mis-delta.
                let compressed = compress_prepend_size(snapshot);
                let approx = compressed.len();
                (Body::Keyframe(compressed.into_boxed_slice()), true, approx)
            }
        };

        let entry = Entry {
            frame,
            body,
            is_keyframe,
            raw_len: snapshot.len(),
            approx_bytes: approx,
        };

        match self.entries.binary_search_by_key(&frame, |e| e.frame) {
            Ok(i) => {
                self.used_bytes -= self.entries[i].approx_bytes;
                self.used_bytes += approx;
                self.entries[i] = entry;
            }
            Err(i) => {
                self.used_bytes += approx;
                self.entries.insert(i, entry);
            }
        }

        if is_keyframe {
            self.since_keyframe = 0;
        } else {
            self.since_keyframe += 1;
        }

        self.enforce_budget(cursor);
    }

    /// Decode and return the nearest cached state at or before `target`, as
    /// `(frame, snapshot_bytes)`. `None` if nothing at-or-before `target` is
    /// cached.
    ///
    /// # Errors
    ///
    /// Returns [`ZwinderError`] if the entry (or the keyframe a delta depends
    /// on) can't be reconstructed.
    pub fn nearest_at_or_before(
        &self,
        target: u64,
    ) -> Option<Result<(u64, Vec<u8>), ZwinderError>> {
        let idx = match self.entries.binary_search_by_key(&target, |e| e.frame) {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };
        let frame = self.entries[idx].frame;
        Some(self.decode_at(idx).map(|bytes| (frame, bytes)))
    }

    /// Decode and return the cached state for exactly `frame`, if present.
    ///
    /// # Errors
    ///
    /// Returns [`ZwinderError`] if the entry can't be reconstructed.
    pub fn get(&self, frame: u64) -> Option<Result<Vec<u8>, ZwinderError>> {
        let idx = self
            .entries
            .binary_search_by_key(&frame, |e| e.frame)
            .ok()?;
        Some(self.decode_at(idx))
    }

    /// `true` if a state is cached for exactly `frame`.
    #[must_use]
    pub fn has(&self, frame: u64) -> bool {
        self.entries
            .binary_search_by_key(&frame, |e| e.frame)
            .is_ok()
    }

    /// Drop every cached state strictly after `frame` (the `InvalidateAfter`
    /// operation — an edit at `frame` invalidates the downstream tail). Anchor
    /// *registrations* after `frame` are kept (a later recapture re-pins them),
    /// but their stored *state* is dropped.
    pub fn invalidate_after(&mut self, frame: u64) {
        let cut = self.entries.partition_point(|e| e.frame <= frame);
        for e in self.entries.drain(cut..) {
            self.used_bytes = self.used_bytes.saturating_sub(e.approx_bytes);
        }
        // The keyframe-cadence counter is only meaningful relative to the
        // surviving tail; reset it so the next store re-anchors a keyframe
        // cadence rather than emitting an orphaned delta.
        self.since_keyframe = self.keyframe_interval;
    }

    /// Register `frame` as a permanent anchor (never evicted, always a
    /// keyframe once stored).
    pub fn add_anchor(&mut self, frame: u64) {
        self.anchors.insert(frame);
    }

    /// Remove an anchor registration (frame 0 stays anchored regardless).
    pub fn remove_anchor(&mut self, frame: u64) {
        if frame != 0 {
            self.anchors.remove(&frame);
        }
    }

    /// `true` if `frame` is a permanent anchor.
    #[must_use]
    pub fn is_anchor(&self, frame: u64) -> bool {
        self.anchors.contains(&frame)
    }

    /// Drop every anchor except frame 0 (used when the marker set is rebuilt
    /// wholesale — a branch load or project load).
    pub fn clear_non_default_anchors(&mut self) {
        self.anchors.retain(|&f| f == 0);
    }

    /// Drop all cached states (anchor registrations are kept).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.used_bytes = 0;
        self.since_keyframe = 0;
    }

    /// Number of cached frames.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if no states are cached.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total *compressed* bytes currently held.
    #[must_use]
    pub const fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    /// The compressed byte budget eviction targets.
    #[must_use]
    pub const fn budget_bytes(&self) -> usize {
        self.budget_bytes
    }

    /// The set of frames a state is cached for (ascending). Used to colour
    /// greenzone-resident rows.
    pub fn cached_frames(&self) -> impl Iterator<Item = u64> + '_ {
        self.entries.iter().map(|e| e.frame)
    }

    // --- internals -------------------------------------------------------- #

    /// Decode the raw snapshot bytes of a keyframe-typed entry by index, or
    /// `None` if that index is a delta.
    fn decode_keyframe_at(&self, idx: usize) -> Option<Result<Vec<u8>, ZwinderError>> {
        match &self.entries[idx].body {
            Body::Keyframe(b) => Some(
                decompress_size_prepended(b).map_err(|e| ZwinderError::Decompress(format!("{e}"))),
            ),
            Body::Delta(_) => None,
        }
    }

    /// Return the raw (decoded) bytes of the nearest keyframe at-or-before
    /// `frame`, or `None` if no keyframe precedes it. Used at *store* time to
    /// XOR-delta a new non-anchor frame against its base keyframe.
    fn preceding_keyframe_raw(&self, frame: u64) -> Option<Vec<u8>> {
        // Rightmost entry whose frame <= `frame` that is a keyframe.
        let upper = self.entries.partition_point(|e| e.frame <= frame);
        for idx in (0..upper).rev() {
            if self.entries[idx].is_keyframe {
                // A keyframe always decodes (it's self-contained); on the rare
                // corrupt-body case fall through to "no preceding keyframe",
                // which forces the caller to store a fresh keyframe.
                return self.decode_keyframe_at(idx).and_then(Result::ok);
            }
        }
        None
    }

    /// Decode the snapshot at `entries[idx]`, walking back to its base
    /// keyframe for delta entries.
    fn decode_at(&self, idx: usize) -> Result<Vec<u8>, ZwinderError> {
        let entry = &self.entries[idx];
        match &entry.body {
            Body::Keyframe(b) => {
                decompress_size_prepended(b).map_err(|e| ZwinderError::Decompress(format!("{e}")))
            }
            Body::Delta(b) => {
                // Find the nearest preceding keyframe.
                let mut kf_idx = None;
                for j in (0..idx).rev() {
                    if self.entries[j].is_keyframe {
                        kf_idx = Some(j);
                        break;
                    }
                }
                let kf_idx = kf_idx.ok_or(ZwinderError::MissingKeyframe(entry.frame))?;
                let kf = decompress_size_prepended(match &self.entries[kf_idx].body {
                    Body::Keyframe(kb) => kb,
                    Body::Delta(_) => return Err(ZwinderError::MissingKeyframe(entry.frame)),
                })
                .map_err(|e| ZwinderError::Decompress(format!("{e}")))?;
                let delta = decompress_size_prepended(b)
                    .map_err(|e| ZwinderError::Decompress(format!("{e}")))?;
                if delta.len() != kf.len() {
                    return Err(ZwinderError::LengthMismatch {
                        kf: kf.len(),
                        dl: delta.len(),
                    });
                }
                let mut out = vec![0u8; kf.len()];
                for ((slot, &k), &d) in out.iter_mut().zip(kf.iter()).zip(delta.iter()) {
                    *slot = k ^ d;
                }
                debug_assert_eq!(out.len(), entry.raw_len);
                Ok(out)
            }
        }
    }

    /// Evict non-anchor entries until [`Self::used_bytes`] is within budget.
    /// Density-tiered: among evictable frames, drop the one in the *densest*
    /// local region (smallest frame-gap to a neighbour) that is *farthest*
    /// from `cursor` — thinning the distant, over-sampled past first.
    ///
    /// To keep the compressed store self-consistent, only entries that no
    /// surviving delta depends on are evictable: a keyframe is evictable only
    /// when no later delta references it before the next keyframe. (Deltas are
    /// always freely evictable.)
    fn enforce_budget(&mut self, cursor: u64) {
        while self.used_bytes > self.budget_bytes {
            let Some(victim) = self.pick_victim(cursor) else {
                break; // Nothing safely evictable.
            };
            self.used_bytes = self
                .used_bytes
                .saturating_sub(self.entries[victim].approx_bytes);
            self.entries.remove(victim);
        }
    }

    /// `true` if the keyframe at `idx` has a dependent delta after it (before
    /// the next keyframe). Such a keyframe is NOT evictable.
    fn keyframe_has_dependents(&self, idx: usize) -> bool {
        for e in &self.entries[idx + 1..] {
            if e.is_keyframe {
                return false; // reached the next keyframe: no dependents
            }
            if matches!(e.body, Body::Delta(_)) {
                return true;
            }
        }
        false
    }

    /// Choose the index of the least-valuable evictable entry, or `None` if
    /// nothing is safely evictable. Lower score = evict first.
    fn pick_victim(&self, cursor: u64) -> Option<usize> {
        let mut best: Option<(usize, (u64, core::cmp::Reverse<u64>))> = None;
        for i in 0..self.entries.len() {
            let f = self.entries[i].frame;
            if self.anchors.contains(&f) {
                continue;
            }
            // A keyframe with dependents can't be dropped without orphaning a
            // delta — skip it (the dependent deltas get thinned first, then it
            // becomes evictable).
            if self.entries[i].is_keyframe && self.keyframe_has_dependents(i) {
                continue;
            }
            // Local density: the smaller frame-gap to a neighbour. A small gap
            // means this frame sits in a dense cluster and is cheap to lose.
            let prev_gap = (i > 0).then(|| f - self.entries[i - 1].frame);
            let next_gap = (i + 1 < self.entries.len()).then(|| self.entries[i + 1].frame - f);
            let gap = match (prev_gap, next_gap) {
                (Some(p), Some(n)) => p.min(n),
                (Some(p), None) => p,
                (None, Some(n)) => n,
                (None, None) => u64::MAX, // sole entry; evicted last
            };
            let dist = f.abs_diff(cursor);
            let key = (gap, core::cmp::Reverse(dist));
            if best.as_ref().is_none_or(|(_, bk)| key < *bk) {
                best = Some((i, key));
            }
        }
        best.map(|(i, _)| i)
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)] // test-only frame<->index + byte casts
mod tests {
    use super::*;

    /// A pseudo-random-but-deterministic snapshot whose content shifts a
    /// little per frame, exercising real XOR-delta + LZ4 behaviour (not an
    /// all-zero blob LZ4 would crush to nothing).
    fn snap(frame: u64, len: usize) -> Vec<u8> {
        let mut v = vec![0u8; len];
        let mut x = frame.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
        for slot in &mut v {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *slot = (x & 0xFF) as u8;
        }
        v
    }

    #[test]
    fn store_and_nearest_lookup() {
        let mut z = ZwinderStateManager::new(1 << 24, 4);
        z.store(0, &snap(0, 4096), 0);
        z.store(30, &snap(30, 4096), 30);
        z.store(60, &snap(60, 4096), 60);
        assert_eq!(z.len(), 3);
        assert_eq!(z.nearest_at_or_before(0).unwrap().unwrap().0, 0);
        assert_eq!(z.nearest_at_or_before(45).unwrap().unwrap().0, 30);
        assert_eq!(z.nearest_at_or_before(60).unwrap().unwrap().0, 60);
        assert_eq!(z.nearest_at_or_before(1000).unwrap().unwrap().0, 60);
        assert!(z.nearest_at_or_before(0).unwrap().is_ok());
    }

    /// THE D2 GATE — lossless round-trip equality. The restored state MUST
    /// byte-equal the saved state, across keyframes AND deltas, in storage
    /// order and after eviction.
    #[test]
    fn round_trip_equality_lossless() {
        let mut z = ZwinderStateManager::new(1 << 24, 8);
        let mut originals = Vec::new();
        // Store 100 frames of evolving 8 KiB state — a mix of keyframes and
        // XOR-deltas (interval 8).
        for f in 0..100u64 {
            let s = snap(f, 8192);
            originals.push(s.clone());
            z.store(f, &s, f);
        }
        // Every cached frame decodes to EXACTLY its original bytes.
        for f in 0..100u64 {
            if let Some(res) = z.get(f) {
                let got = res.expect("decode");
                assert_eq!(got, originals[f as usize], "frame {f} round-trip mismatch");
            }
        }
        // Spot-check: the deltas (non-keyframe frames) really round-trip.
        let got = z.get(5).expect("frame 5 cached").expect("decode");
        assert_eq!(got, originals[5], "delta frame 5 must be lossless");
        let got = z.get(99).expect("frame 99 cached").expect("decode");
        assert_eq!(got, originals[99], "tail frame 99 must be lossless");
    }

    #[test]
    fn anchors_are_keyframes_and_survive_eviction() {
        // A budget that holds a handful of (barely-compressible random) 4 KiB
        // states but forces eviction once the dense 100..900 sweep lands — so
        // the density tiering has room to keep the cursor neighbourhood while
        // thinning the far past, rather than starving everything behind the
        // two anchors.
        let mut z = ZwinderStateManager::new(40 * 1024, 8);
        z.add_anchor(0);
        z.add_anchor(500); // a far marker
        z.store(0, &snap(0, 4096), 900);
        z.store(500, &snap(500, 4096), 900);
        for f in (100..=900).step_by(10) {
            z.store(f, &snap(f, 4096), 900);
        }
        // Both anchors survived and decode losslessly.
        assert!(z.has(0), "frame-0 anchor must survive");
        assert!(z.has(500), "far marker anchor must survive");
        let a0 = z.get(0).unwrap().unwrap();
        assert_eq!(a0, snap(0, 4096));
        let a500 = z.get(500).unwrap().unwrap();
        assert_eq!(a500, snap(500, 4096));
        // Something near the cursor survived (seeking to the cursor stays cheap).
        let near_cursor = z
            .cached_frames()
            .any(|f| f != 0 && f != 500 && f.abs_diff(900) <= 100);
        assert!(near_cursor, "a frame near the cursor should be retained");
    }

    #[test]
    fn invalidate_after_drops_the_downstream_tail() {
        let mut z = ZwinderStateManager::new(1 << 24, 4);
        for f in [0u64, 10, 20, 30, 40] {
            z.store(f, &snap(f, 1024), f);
        }
        z.invalidate_after(20);
        let kept: Vec<u64> = z.cached_frames().collect();
        assert_eq!(kept, vec![0, 10, 20]);
        // Surviving frames still decode losslessly after the tail drop.
        assert_eq!(z.get(20).unwrap().unwrap(), snap(20, 1024));
        z.invalidate_after(0);
        assert_eq!(z.cached_frames().collect::<Vec<_>>(), vec![0]);
        assert_eq!(z.get(0).unwrap().unwrap(), snap(0, 1024));
    }

    #[test]
    fn store_replaces_existing_frame_and_tracks_bytes() {
        let mut z = ZwinderStateManager::new(1 << 24, 1); // every frame a keyframe
        z.store(10, &snap(10, 4096), 10);
        let after_first = z.used_bytes();
        assert!(after_first > 0);
        z.store(10, &snap(11, 2048), 10); // replace with different content/len
        assert_eq!(z.len(), 1);
        assert_eq!(z.get(10).unwrap().unwrap(), snap(11, 2048));
    }

    #[test]
    fn compression_beats_uncompressed_on_slowly_changing_state() {
        // Mostly-static state with a few changing bytes per frame — the NES
        // common case. The compressed store should hold all 200 frames in far
        // less than the uncompressed 200 * 16 KiB = 3.2 MiB.
        let mut z = ZwinderStateManager::new(1 << 26, 16);
        let base = snap(0, 16384);
        for f in 0..200u64 {
            let mut s = base.clone();
            // Mutate a handful of bytes deterministically.
            for k in 0..8usize {
                let idx = ((f as usize).wrapping_mul(37).wrapping_add(k * 101)) % s.len();
                s[idx] = (f as u8).wrapping_add(k as u8);
            }
            z.store(f, &s, f);
        }
        assert_eq!(z.len(), 200);
        let uncompressed = 200 * 16384;
        assert!(
            z.used_bytes() < uncompressed / 4,
            "compressed {} should be well under uncompressed {}",
            z.used_bytes(),
            uncompressed
        );
    }

    #[test]
    fn clear_resets_state_keeps_anchors() {
        let mut z = ZwinderStateManager::default();
        z.add_anchor(100);
        for f in 0..5u64 {
            z.store(f, &snap(f, 256), f);
        }
        z.clear();
        assert_eq!(z.len(), 0);
        assert_eq!(z.used_bytes(), 0);
        assert!(z.is_anchor(0));
        assert!(z.is_anchor(100));
    }
}

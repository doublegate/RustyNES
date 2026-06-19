//! Density-tiered, RAM-budgeted save-state history for the `TAStudio` editor
//! (v1.6.0 Workstream A1 — the "greenzone"; v1.7.0 Workstream D2 — compressed).
//!
//! The greenzone is the set of frames a save-state currently exists for, so any
//! frame can be returned to by "load the nearest cached state at-or-before the
//! target, then re-emulate forward" (the seek primitive lives in
//! [`super::TasEditor`]). This mirrors `BizHawk` `TAStudio`'s `PagedStateManager`
//! and FCEUX `TASEditor`'s greenzone, distilled to the essentials:
//!
//! - **Frame-keyed** snapshots, stored sorted ascending by frame.
//! - **Anchors** (frame 0, markers, branch points) are never evicted.
//! - A **byte budget** with **density-tiered eviction**: when storage exceeds
//!   the budget, the least-valuable non-anchor keyframe is dropped — preferring
//!   to thin *dense* regions *far* from the cursor, so the recently-visited
//!   neighbourhood stays dense (cheap to seek into) while the distant past
//!   decays to a sparse skeleton.
//! - **`invalidate_after(frame)`**: an edit at `frame` drops every cached state
//!   strictly after it (the decouple-edit-from-state insight — editing never
//!   touches states directly, it only invalidates the now-stale tail).
//!
//! # v1.7.0 "Forge" Workstream D2 — compression
//!
//! As of v1.7.0 the greenzone delegates its storage to the **Zwinder-class
//! compressed, density-tiered state manager**
//! ([`rustynes_core::ZwinderStateManager`]): snapshots are kept as XOR-deltas +
//! LZ4 against periodic keyframes, with the same reserved-anchor +
//! density-tiered-eviction policy operating on the *compressed* sizes. This is
//! the depth that scales the greenzone to feature-length TASes (the same RAM
//! holds far more history). Compression is **lossless** — a stored state
//! decodes byte-for-byte to what was stored (the D2 round-trip-equality gate),
//! so the deterministic seek/replay contract is unchanged.
//!
//! This type is deliberately decoupled from the emulator: it stores opaque
//! snapshot byte-blobs ([`rustynes_core::Nes::snapshot`]) keyed by frame, so its
//! eviction / invalidation / lookup logic is pure and unit-testable without a
//! running `Nes`. Determinism is unaffected: the blobs are exactly the
//! deterministic save-states the core already produces.

use rustynes_core::ZwinderStateManager;

/// A density-tiered, byte-budgeted, **compressed** history of frame-keyed
/// save-states. A thin `usize`-frame adapter over the core's
/// [`ZwinderStateManager`] (which keys by `u64`).
pub struct Greenzone {
    /// The compressed, density-tiered state engine (XOR-delta + LZ4 + reserved
    /// anchors). Frame keys are `usize` at this boundary, `u64` inside.
    inner: ZwinderStateManager,
}

// The `usize`<->`u64` frame-key conversions at this adapter boundary are exact
// on every target we ship (frame counts never approach 2^32, let alone 2^64).
#[allow(clippy::cast_possible_truncation)]
impl Greenzone {
    /// Create an empty greenzone with the given soft byte budget (over the
    /// *compressed* sizes). Frame 0 is always an anchor (the start state must
    /// never be evicted). The keyframe interval is the manager default.
    #[must_use]
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            inner: ZwinderStateManager::new(
                budget_bytes,
                rustynes_core::ZWINDER_DEFAULT_KEYFRAME_INTERVAL,
            ),
        }
    }

    /// Store (or replace) the save-state for `frame` (compressing it). `cursor`
    /// is the editor's current frame position — eviction protects the
    /// neighbourhood around it. Enforces the (compressed) byte budget.
    // `bytes` stays owned (`Vec<u8>`) for source-compatibility with the prior
    // uncompressed greenzone API + every caller (they hand over a fresh
    // `nes.snapshot()`); the compressing backend borrows it.
    #[allow(clippy::needless_pass_by_value)]
    pub fn store(&mut self, frame: usize, bytes: Vec<u8>, cursor: usize) {
        self.inner.store(frame as u64, &bytes, cursor as u64);
    }

    /// The nearest cached state at or before `target`, decompressed, as
    /// `(frame, bytes)`. `None` if nothing at or before `target` is cached.
    ///
    /// # Panics
    ///
    /// Panics if a stored state fails to decompress — impossible for the
    /// lossless codec on blobs this build produced (the D2 round-trip gate),
    /// so a failure is a logic bug, surfaced loudly rather than silently
    /// mis-seeking.
    #[must_use]
    pub fn nearest_at_or_before(&self, target: usize) -> Option<(usize, Vec<u8>)> {
        self.inner.nearest_at_or_before(target as u64).map(|res| {
            let (frame, bytes) = res.expect("greenzone state decompresses losslessly");
            (frame as usize, bytes)
        })
    }

    /// `true` if a state is cached for exactly `frame`.
    #[must_use]
    pub fn has(&self, frame: usize) -> bool {
        self.inner.has(frame as u64)
    }

    /// Drop every cached state strictly after `frame` (the `InvalidateAfter`
    /// operation — an edit at `frame` invalidates the downstream greenzone).
    /// Anchors after `frame` are dropped too (their cached *state* is stale);
    /// the anchor *frame* registration is kept so a later recapture re-pins it.
    pub fn invalidate_after(&mut self, frame: usize) {
        self.inner.invalidate_after(frame as u64);
    }

    /// Register `frame` as a permanent anchor (frame 0, a marker, a branch
    /// point). Anchored frames are never evicted by the budget.
    pub fn add_anchor(&mut self, frame: usize) {
        self.inner.add_anchor(frame as u64);
    }

    /// Remove an anchor registration (frame 0 stays anchored regardless).
    pub fn remove_anchor(&mut self, frame: usize) {
        self.inner.remove_anchor(frame as u64);
    }

    /// `true` if `frame` is a permanent anchor (never evicted).
    #[must_use]
    pub fn is_anchor(&self, frame: usize) -> bool {
        self.inner.is_anchor(frame as u64)
    }

    /// Drop every anchor except frame 0 (the permanent power-on base). Used when
    /// the marker set is rebuilt wholesale — a marker shift, a branch load, or a
    /// project load — so stale anchors from the previous marker set don't
    /// accumulate and starve the eviction budget.
    pub fn clear_non_default_anchors(&mut self) {
        self.inner.clear_non_default_anchors();
    }

    /// Drop all cached states (e.g. on loading a new project / power-cycle).
    /// Anchor registrations are kept.
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Number of cached keyframes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.inner.len()
    }

    /// `true` if no states are cached.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Total *compressed* bytes currently held in cached snapshots.
    #[must_use]
    pub const fn used_bytes(&self) -> usize {
        self.inner.used_bytes()
    }

    /// The (compressed) byte budget eviction targets.
    #[must_use]
    pub const fn budget_bytes(&self) -> usize {
        self.inner.budget_bytes()
    }

    /// The set of frames a state is currently cached for (ascending). Used by
    /// the piano-roll to colour greenzone-resident rows.
    pub fn cached_frames(&self) -> impl Iterator<Item = usize> + '_ {
        self.inner.cached_frames().map(|f| f as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Varied, deterministic content so the compressed backend exercises real
    // XOR-delta + LZ4 behaviour (a uniform blob would crush to nothing).
    fn blob(frame: usize, n: usize) -> Vec<u8> {
        let mut v = vec![0u8; n];
        let mut x = (frame as u64)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(1);
        for slot in &mut v {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *slot = (x & 0xFF) as u8;
        }
        v
    }

    #[test]
    fn store_and_nearest_lookup_is_lossless() {
        let mut gz = Greenzone::new(1 << 24);
        gz.store(0, blob(0, 4096), 0);
        gz.store(30, blob(30, 4096), 30);
        gz.store(60, blob(60, 4096), 60);
        assert_eq!(gz.len(), 3);
        // Exact and between lookups resolve to the nearest <= target, and the
        // returned bytes round-trip losslessly (the D2 contract at the adapter).
        let (f, b) = gz.nearest_at_or_before(0).unwrap();
        assert_eq!((f, b), (0, blob(0, 4096)));
        assert_eq!(gz.nearest_at_or_before(45).unwrap().0, 30);
        assert_eq!(gz.nearest_at_or_before(45).unwrap().1, blob(30, 4096));
        assert_eq!(gz.nearest_at_or_before(60).unwrap().0, 60);
        assert_eq!(gz.nearest_at_or_before(1000).unwrap().0, 60);
        assert_eq!(gz.nearest_at_or_before(1000).unwrap().1, blob(60, 4096));
    }

    #[test]
    fn nearest_before_first_is_none() {
        let mut gz = Greenzone::new(1 << 24);
        gz.store(50, blob(50, 1024), 50);
        assert!(gz.nearest_at_or_before(49).is_none());
        assert_eq!(gz.nearest_at_or_before(50).unwrap().0, 50);
    }

    #[test]
    fn store_replaces_existing_frame() {
        let mut gz = Greenzone::new(1 << 24);
        gz.store(10, blob(10, 4096), 10);
        assert_eq!(gz.len(), 1);
        gz.store(10, blob(11, 2048), 10); // replace with different content
        assert_eq!(gz.len(), 1);
        assert_eq!(gz.nearest_at_or_before(10).unwrap().1, blob(11, 2048));
    }

    #[test]
    fn invalidate_after_drops_the_downstream_tail() {
        let mut gz = Greenzone::new(1 << 24);
        for f in [0usize, 10, 20, 30, 40] {
            gz.store(f, blob(f, 1024), f);
        }
        gz.invalidate_after(20);
        let kept: Vec<usize> = gz.cached_frames().collect();
        assert_eq!(kept, vec![0, 10, 20]);
        // Surviving frames still round-trip losslessly after the tail drop.
        assert_eq!(gz.nearest_at_or_before(20).unwrap().1, blob(20, 1024));
        // Editing frame 0's input invalidates everything after frame -1, i.e.
        // (frame.saturating_sub(1)) -> invalidate_after(0) keeps only frame 0.
        gz.invalidate_after(0);
        assert_eq!(gz.cached_frames().collect::<Vec<_>>(), vec![0]);
    }

    #[test]
    fn eviction_respects_budget_and_keeps_anchors() {
        // A compressed budget that forces the dense 100..900 sweep to thin but
        // leaves room for the cursor neighbourhood + the two anchors.
        let mut gz = Greenzone::new(40 * 1024);
        gz.add_anchor(0);
        gz.add_anchor(1000); // a far marker — must survive eviction
        gz.store(0, blob(0, 4096), 900);
        gz.store(1000, blob(1000, 4096), 900);
        for f in (100..=900).step_by(50) {
            gz.store(f, blob(f, 4096), 900);
        }
        assert!(
            gz.used_bytes() <= gz.budget_bytes(),
            "compressed budget {} must be honoured (used {})",
            gz.budget_bytes(),
            gz.used_bytes()
        );
        // Both anchors survived and decode losslessly.
        assert!(gz.has(0), "frame-0 anchor must survive");
        assert!(gz.has(1000), "far marker anchor must survive");
        assert_eq!(gz.nearest_at_or_before(0).unwrap().1, blob(0, 4096));
        assert_eq!(gz.nearest_at_or_before(1000).unwrap().1, blob(1000, 4096));
        // Something near the cursor (900) survived — seeking stays cheap.
        let near_cursor = gz
            .cached_frames()
            .any(|f| f != 0 && f != 1000 && f.abs_diff(900) <= 100);
        assert!(near_cursor, "a keyframe near the cursor should be retained");
    }

    #[test]
    fn anchors_are_never_evicted() {
        let mut gz = Greenzone::new(2 * 1024); // tight
        gz.add_anchor(0);
        gz.store(0, blob(0, 4096), 0); // anchor, over budget alone
        gz.store(10, blob(10, 4096), 0); // non-anchor
        // The anchor stays even though it alone exceeds the budget (anchors are
        // never evicted / the loop never spins forever).
        assert!(gz.has(0));
    }
}

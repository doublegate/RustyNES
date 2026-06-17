//! Density-tiered, RAM-budgeted save-state history for the `TAStudio` editor
//! (v1.6.0 Workstream A1 — the "greenzone").
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
//! This type is deliberately decoupled from the emulator: it stores opaque
//! snapshot byte-blobs ([`rustynes_core::Nes::snapshot`]) keyed by frame, so its
//! eviction / invalidation / lookup logic is pure and unit-testable without a
//! running `Nes`. Determinism is unaffected: the blobs are exactly the
//! deterministic save-states the core already produces.

use std::collections::BTreeSet;

/// One cached emulator save-state, keyed by the frame index it represents
/// (the state *before* `frame`'s input is applied — i.e. seeking to `frame`
/// restores this and is ready to apply frame `frame`'s input).
#[derive(Clone)]
struct Keyframe {
    frame: usize,
    bytes: Vec<u8>,
}

/// A density-tiered, byte-budgeted history of frame-keyed save-states.
pub struct Greenzone {
    /// Keyframes, kept sorted ascending by `frame` (no duplicate frames).
    frames: Vec<Keyframe>,
    /// Frames that must never be evicted (frame 0, markers, branch points).
    anchors: BTreeSet<usize>,
    /// Soft byte budget; eviction keeps [`Self::used_bytes`] at or under this.
    budget_bytes: usize,
    /// Running sum of all stored snapshot byte lengths.
    used_bytes: usize,
}

impl Greenzone {
    /// Create an empty greenzone with the given soft byte budget. Frame 0 is
    /// always an anchor (the start state must never be evicted).
    #[must_use]
    pub fn new(budget_bytes: usize) -> Self {
        let mut anchors = BTreeSet::new();
        anchors.insert(0);
        Self {
            frames: Vec::new(),
            anchors,
            budget_bytes: budget_bytes.max(1),
            used_bytes: 0,
        }
    }

    /// Store (or replace) the save-state for `frame`. `cursor` is the editor's
    /// current frame position — eviction protects the neighbourhood around it.
    /// Enforces the byte budget after inserting.
    pub fn store(&mut self, frame: usize, bytes: Vec<u8>, cursor: usize) {
        let added = bytes.len();
        match self.frames.binary_search_by_key(&frame, |k| k.frame) {
            Ok(i) => {
                // Replace an existing keyframe at this frame.
                self.used_bytes -= self.frames[i].bytes.len();
                self.used_bytes += added;
                self.frames[i].bytes = bytes;
            }
            Err(i) => {
                self.used_bytes += added;
                self.frames.insert(i, Keyframe { frame, bytes });
            }
        }
        self.enforce_budget(cursor);
    }

    /// The nearest cached state at or before `target`, as `(frame, bytes)`.
    /// `None` if nothing at or before `target` is cached.
    #[must_use]
    pub fn nearest_at_or_before(&self, target: usize) -> Option<(usize, &[u8])> {
        // Rightmost keyframe whose frame <= target.
        let idx = match self.frames.binary_search_by_key(&target, |k| k.frame) {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };
        let k = &self.frames[idx];
        Some((k.frame, &k.bytes))
    }

    /// `true` if a state is cached for exactly `frame`.
    #[must_use]
    pub fn has(&self, frame: usize) -> bool {
        self.frames
            .binary_search_by_key(&frame, |k| k.frame)
            .is_ok()
    }

    /// Drop every cached state strictly after `frame` (the `InvalidateAfter`
    /// operation — an edit at `frame` invalidates the downstream greenzone).
    /// Anchors after `frame` are dropped too (their cached *state* is stale);
    /// the anchor *frame* registration is kept so a later recapture re-pins it.
    pub fn invalidate_after(&mut self, frame: usize) {
        // First stale index = first keyframe whose frame > `frame`.
        let cut = self.frames.partition_point(|k| k.frame <= frame);
        for k in self.frames.drain(cut..) {
            self.used_bytes -= k.bytes.len();
        }
    }

    /// Register `frame` as a permanent anchor (frame 0, a marker, a branch
    /// point). Anchored frames are never evicted by the budget.
    pub fn add_anchor(&mut self, frame: usize) {
        self.anchors.insert(frame);
    }

    /// Remove an anchor registration (frame 0 stays anchored regardless).
    pub fn remove_anchor(&mut self, frame: usize) {
        if frame != 0 {
            self.anchors.remove(&frame);
        }
    }

    /// Drop all cached states (e.g. on loading a new project / power-cycle).
    /// Anchor registrations are kept.
    pub fn clear(&mut self) {
        self.frames.clear();
        self.used_bytes = 0;
    }

    /// Number of cached keyframes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.frames.len()
    }

    /// `true` if no states are cached.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Total bytes currently held in cached snapshots.
    #[must_use]
    pub const fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    /// The byte budget eviction targets.
    #[must_use]
    pub const fn budget_bytes(&self) -> usize {
        self.budget_bytes
    }

    /// The set of frames a state is currently cached for (ascending). Used by
    /// the piano-roll to colour greenzone-resident rows.
    pub fn cached_frames(&self) -> impl Iterator<Item = usize> + '_ {
        self.frames.iter().map(|k| k.frame)
    }

    // --- eviction --------------------------------------------------------- #

    /// Evict non-anchor keyframes until [`Self::used_bytes`] is within budget.
    /// Density-tiered: among evictable frames, drop the one in the *densest*
    /// local region (smallest gap to its neighbours) that is *farthest* from
    /// `cursor` — thinning the distant, over-sampled past first and leaving the
    /// cursor neighbourhood dense.
    fn enforce_budget(&mut self, cursor: usize) {
        while self.used_bytes > self.budget_bytes {
            let Some(victim) = self.pick_victim(cursor) else {
                // Nothing evictable (everything left is an anchor): stop rather
                // than loop forever — anchors may legitimately exceed budget.
                break;
            };
            self.used_bytes -= self.frames[victim].bytes.len();
            self.frames.remove(victim);
        }
    }

    /// Choose the index of the least-valuable evictable keyframe, or `None` if
    /// every remaining keyframe is an anchor. Lower score = evict first.
    fn pick_victim(&self, cursor: usize) -> Option<usize> {
        // Eviction key per candidate: `(gap, Reverse(dist))`. We evict the
        // MINIMUM key — i.e. the smallest local gap (densest region) and, among
        // equal gaps, the largest distance from the cursor (farthest past).
        let mut best: Option<(usize, (usize, std::cmp::Reverse<usize>))> = None;
        for i in 0..self.frames.len() {
            let f = self.frames[i].frame;
            if self.anchors.contains(&f) {
                continue;
            }
            // Local density: the gap to the nearer neighbour. A small gap means
            // this frame sits in a dense cluster and is cheap to lose (a seek
            // only has to re-emulate a few extra frames). Edge keyframes use the
            // one neighbour they have.
            let prev_gap = (i > 0).then(|| f - self.frames[i - 1].frame);
            let next_gap = (i + 1 < self.frames.len()).then(|| self.frames[i + 1].frame - f);
            let gap = match (prev_gap, next_gap) {
                (Some(p), Some(n)) => p.min(n),
                (Some(p), None) => p,
                (None, Some(n)) => n,
                (None, None) => usize::MAX, // sole keyframe; only evicted last
            };
            let dist = f.abs_diff(cursor);
            let key = (gap, std::cmp::Reverse(dist));
            if best.as_ref().is_none_or(|(_, bk)| key < *bk) {
                best = Some((i, key));
            }
        }
        best.map(|(i, _)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(n: usize) -> Vec<u8> {
        vec![0xAB; n]
    }

    #[test]
    fn store_and_nearest_lookup() {
        let mut gz = Greenzone::new(1 << 20);
        gz.store(0, blob(10), 0);
        gz.store(30, blob(10), 30);
        gz.store(60, blob(10), 60);
        assert_eq!(gz.len(), 3);
        // Exact and between lookups resolve to the nearest <= target.
        assert_eq!(gz.nearest_at_or_before(0).unwrap().0, 0);
        assert_eq!(gz.nearest_at_or_before(45).unwrap().0, 30);
        assert_eq!(gz.nearest_at_or_before(60).unwrap().0, 60);
        assert_eq!(gz.nearest_at_or_before(1000).unwrap().0, 60);
    }

    #[test]
    fn nearest_before_first_is_none() {
        let mut gz = Greenzone::new(1 << 20);
        gz.store(50, blob(10), 50);
        assert!(gz.nearest_at_or_before(49).is_none());
        assert_eq!(gz.nearest_at_or_before(50).unwrap().0, 50);
    }

    #[test]
    fn store_replaces_existing_frame_and_tracks_bytes() {
        let mut gz = Greenzone::new(1 << 20);
        gz.store(10, blob(100), 10);
        assert_eq!(gz.used_bytes(), 100);
        gz.store(10, blob(40), 10); // replace
        assert_eq!(gz.len(), 1);
        assert_eq!(gz.used_bytes(), 40);
    }

    #[test]
    fn invalidate_after_drops_the_downstream_tail() {
        let mut gz = Greenzone::new(1 << 20);
        for f in [0usize, 10, 20, 30, 40] {
            gz.store(f, blob(10), f);
        }
        gz.invalidate_after(20);
        let kept: Vec<usize> = gz.cached_frames().collect();
        assert_eq!(kept, vec![0, 10, 20]);
        assert_eq!(gz.used_bytes(), 30);
        // Editing frame 0's input invalidates everything after frame -1, i.e.
        // (frame.saturating_sub(1)) -> invalidate_after(0) keeps only frame 0.
        gz.invalidate_after(0);
        assert_eq!(gz.cached_frames().collect::<Vec<_>>(), vec![0]);
    }

    #[test]
    fn eviction_respects_budget_and_keeps_anchors() {
        // Budget for 5 blobs of 100 bytes.
        let mut gz = Greenzone::new(500);
        gz.add_anchor(0);
        gz.add_anchor(1000); // a far marker — must survive eviction
        // Store frame 0 (anchor), the far anchor, and many dense frames near a
        // cursor at 900.
        gz.store(0, blob(100), 900);
        gz.store(1000, blob(100), 900);
        for f in (100..=900).step_by(50) {
            gz.store(f, blob(100), 900);
        }
        // Budget honoured (anchors may push us a little over, but here 500 is
        // tight so we should be at/under once dense frames are thinned).
        assert!(
            gz.used_bytes() <= gz.budget_bytes() + 200,
            "used {} should be near budget {}",
            gz.used_bytes(),
            gz.budget_bytes()
        );
        // Both anchors survived.
        assert!(gz.has(0), "frame-0 anchor must survive");
        assert!(gz.has(1000), "far marker anchor must survive");
        // Something near the cursor (900) survived — seeking to the cursor stays
        // cheap.
        let near_cursor = gz
            .cached_frames()
            .any(|f| !(f == 0 || f == 1000) && f.abs_diff(900) <= 100);
        assert!(near_cursor, "a keyframe near the cursor should be retained");
    }

    #[test]
    fn sole_nonanchor_is_evictable_but_anchors_never_are() {
        let mut gz = Greenzone::new(50); // smaller than one blob
        gz.add_anchor(0);
        gz.store(0, blob(100), 0); // anchor, over budget alone
        gz.store(10, blob(100), 0); // non-anchor
        // The non-anchor must be evicted; the anchor stays even though it alone
        // exceeds the budget (we never evict anchors / loop forever).
        assert!(gz.has(0));
        assert!(!gz.has(10));
    }
}

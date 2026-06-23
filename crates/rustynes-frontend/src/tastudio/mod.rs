//! `TAStudio` — the piano-roll TAS *editor* (v1.6.0 Workstream A).
//!
//! Where [`crate::movie_ui`] *records* and *plays* a linear `.rnm` movie, the
//! `TAStudio` editor lets you *create* an input file frame-by-frame: edit any
//! frame's input in a grid and the emulator re-seeks to it instantly using a
//! cached save-state history (the [`Greenzone`]). This module is the editor's
//! model + the determinism-critical seek/edit plumbing (Workstream A1); the
//! egui piano-roll grid (A2) and branches/markers/projects (A4) layer on top.
//!
//! The model is the three decoupled structures the reference TAS tools
//! (`BizHawk` `TAStudio`, FCEUX `TASEditor`) use:
//!
//! 1. **Input log** — the movie itself, one [`FrameInput`] per frame.
//! 2. **Greenzone** — the frame-keyed save-state cache ([`Greenzone`]).
//! 3. *(Lag log — per-frame "did the game poll input", surfaced from the core's
//!    `debug-hooks` lag flag; wired in with the grid in A2.)*
//!
//! The load-bearing insight is that **editing input never touches states
//! directly** — it calls [`TasEditor::set_input`], which invalidates the
//! greenzone *after* the edited frame. The greenzone then rebuilds naturally as
//! the user seeks/plays forward (re-emulation). Because the editor drives the
//! exact same deterministic `set_buttons` + `run_frame` path the live emulator
//! uses, a seek re-derives state **bit-identically** to having played there —
//! the determinism contract is unchanged (proven by the seek round-trip test).

mod greenzone;

pub use greenzone::Greenzone;

use std::collections::BTreeMap;

use rustynes_core::{Buttons, FrameInput, Movie, Nes, Region, StartPoint};
use thiserror::Error;

/// Magic prefix of a `.rnmproj` `TAStudio` project file.
pub const RNMPROJ_MAGIC: &[u8; 8] = b"RNMPROJ1";

/// Current `.rnmproj` format version.
pub const RNMPROJ_VERSION: u16 = 1;

/// Errors decoding a `.rnmproj` project file.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum RnmProjError {
    /// The blob ran out before a field finished decoding.
    #[error("rnmproj truncated at offset {0}")]
    Truncated(usize),
    /// The magic prefix is not `"RNMPROJ1"`.
    #[error("rnmproj magic mismatch")]
    BadMagic,
    /// The format version is newer than this build understands.
    #[error("rnmproj version {got} not supported (max {max})")]
    UnsupportedVersion {
        /// Version read from the file.
        got: u16,
        /// Highest version this build accepts.
        max: u16,
    },
    /// A UTF-8 marker label was malformed.
    #[error("rnmproj marker label is not valid UTF-8")]
    BadLabel,
}

// --- `.rnmproj` binary codec helpers ------------------------------------- #

/// Write a length / frame index as a `u32` LE. Frame counts and lengths are far
/// below `u32::MAX` (4 billion frames ≈ years of gameplay) in any real project.
#[allow(clippy::cast_possible_truncation)]
fn write_len(w: &mut Vec<u8>, n: usize) {
    w.extend_from_slice(&(n as u32).to_le_bytes());
}

/// Write a length-prefixed byte blob.
fn write_bytes(w: &mut Vec<u8>, b: &[u8]) {
    write_len(w, b.len());
    w.extend_from_slice(b);
}

/// Write a length-prefixed input log (3 bytes per frame: p1, p2, expansion).
fn write_input_log(w: &mut Vec<u8>, log: &[FrameInput]) {
    write_len(w, log.len());
    for f in log {
        w.push(f.p1.bits());
        w.push(f.p2.bits());
        w.push(f.expansion);
    }
}

/// Write a length-prefixed marker map (`frame` then a length-prefixed label).
fn write_markers(w: &mut Vec<u8>, m: &BTreeMap<usize, String>) {
    write_len(w, m.len());
    for (&frame, label) in m {
        write_len(w, frame);
        write_bytes(w, label.as_bytes());
    }
}

/// A bounds-checked cursor reader over a `.rnmproj` blob — every read either
/// advances within bounds or returns [`RnmProjError::Truncated`] (never panics).
struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    const fn new(b: &'a [u8]) -> Self {
        Self { b, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], RnmProjError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or(RnmProjError::Truncated(self.pos))?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u16(&mut self) -> Result<u16, RnmProjError> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }

    fn len(&mut self) -> Result<usize, RnmProjError> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
    }

    fn bytes(&mut self) -> Result<Vec<u8>, RnmProjError> {
        let n = self.len()?;
        Ok(self.take(n)?.to_vec())
    }

    /// Bytes not yet consumed. Used to bound `with_capacity` against an
    /// untrusted length field: each list element consumes at least one byte, so
    /// capacity can never usefully exceed this — preventing an OOM/DoS from a
    /// corrupt or hand-edited `.rnmproj`.
    const fn remaining(&self) -> usize {
        self.b.len() - self.pos
    }

    fn input_log(&mut self) -> Result<Vec<FrameInput>, RnmProjError> {
        let n = self.len()?;
        // Each frame is 3 bytes; never pre-allocate beyond what the blob holds.
        let mut v = Vec::with_capacity(n.min(self.remaining() / 3));
        for _ in 0..n {
            let s = self.take(3)?;
            v.push(FrameInput {
                p1: Buttons::from_bits_truncate(s[0]),
                p2: Buttons::from_bits_truncate(s[1]),
                expansion: s[2],
            });
        }
        Ok(v)
    }

    fn markers(&mut self) -> Result<BTreeMap<usize, String>, RnmProjError> {
        let n = self.len()?;
        let mut m = BTreeMap::new();
        for _ in 0..n {
            let frame = self.len()?;
            let label = String::from_utf8(self.bytes()?).map_err(|_| RnmProjError::BadLabel)?;
            m.insert(frame, label);
        }
        Ok(m)
    }
}

/// Default greenzone byte budget (256 MiB) — generous for desktop TAS work
/// while bounded. Tunable by the frontend; the eviction policy keeps the
/// cursor neighbourhood dense within whatever budget is set.
pub const DEFAULT_GREENZONE_BUDGET: usize = 256 * 1024 * 1024;

/// Default keyframe spacing while seeking/playing: store a save-state every N
/// frames so any later seek only re-emulates at most N-1 frames forward.
pub const DEFAULT_CAPTURE_INTERVAL: usize = 60;

/// A forkable timeline (`BizHawk` `TasBranch` / FCEUX "Bookmark").
///
/// A complete snapshot of the project at the moment it was forked — the input
/// log, the markers, the cursor frame, and the emulator save-state there.
/// Loading a branch restores all of them. (A framebuffer thumbnail is a
/// frontend concern, added when the grid lands.)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    /// The frame the branch was forked at (the cursor at save time).
    pub frame: usize,
    /// The full input log at fork time.
    pub input_log: Vec<FrameInput>,
    /// The markers at fork time.
    pub markers: BTreeMap<usize, String>,
    /// The emulator save-state at [`Self::frame`].
    pub state: Vec<u8>,
}

/// The `TAStudio` editor model: an editable input log over a frame-keyed
/// save-state greenzone, with deterministic seek + edit-invalidation.
pub struct TasEditor {
    /// The editable movie — one input per frame, index = frame number.
    input_log: Vec<FrameInput>,
    /// Frame-keyed save-state cache enabling instant seeking.
    greenzone: Greenzone,
    /// Current edit/playback position (frame index, `0..=len`).
    cursor: usize,
    /// Keyframe spacing captured while seeking/stepping forward.
    capture_interval: usize,
    /// The lag log — the third `TAStudio` structure. `lag_log[f] == true` means
    /// frame `f` was a *lag frame*: the running program polled no controller
    /// port that frame (derived from the core's `debug-hooks` poll flag, which
    /// the frontend always has on). Re-derived on re-emulation; truncated past
    /// an edit, exactly like the greenzone. Sparse: only frames actually played
    /// have an entry, so `lag_at` returns `None` for not-yet-emulated frames.
    lag_log: Vec<bool>,
    /// Named frame labels (the piano-roll's marker rows), keyed by frame. Each
    /// marked frame is also pinned as a greenzone anchor so jumping to it is
    /// instant. Markers shift with their frames on insert / delete.
    markers: BTreeMap<usize, String>,
    /// Saved forkable timelines (A4). Each is a full project snapshot the user
    /// can branch off and return to.
    branches: Vec<Branch>,
    /// Monotonic edit counter bumped on every mutation that changes a field a
    /// [`crate::app`]-built `TasSnapshot` reads (input log, lag log, greenzone,
    /// cursor, markers, branches). The script-host snapshot push reads it via
    /// [`Self::revision`] to skip the per-frame clone of the whole editor when
    /// nothing changed (an idle `TAStudio` costs no allocation).
    revision: u64,
    /// TAS re-record tally — bumped once per *input-log* edit (set / insert /
    /// delete a frame's input), the value `.fm2` / `.bk2` exports surface as the
    /// `rerecordCount` header (via [`Self::to_movie`]). Distinct from `revision`,
    /// which also bumps on lag-log / cursor / marker / branch churn. Saturates at
    /// `u32::MAX` (4 billion re-records is not a real session).
    rerecord_count: u32,
}

impl TasEditor {
    /// Create an editor over an empty input log. `nes` must already be at the
    /// project's start state (typically a fresh [`Nes::power_cycle`]); its
    /// snapshot is pinned as the frame-0 anchor so seeks always have a base.
    #[must_use]
    pub fn new(nes: &Nes, budget_bytes: usize, capture_interval: usize) -> Self {
        let mut greenzone = Greenzone::new(budget_bytes);
        greenzone.add_anchor(0);
        greenzone.store(0, nes.snapshot(), 0);
        Self {
            input_log: Vec::new(),
            greenzone,
            cursor: 0,
            capture_interval: capture_interval.max(1),
            lag_log: Vec::new(),
            markers: BTreeMap::new(),
            branches: Vec::new(),
            revision: 0,
            rerecord_count: 0,
        }
    }

    /// Bump the TAS re-record tally (saturating). Called by each input-log edit.
    const fn bump_rerecord(&mut self) {
        self.rerecord_count = self.rerecord_count.saturating_add(1);
    }

    /// The current TAS re-record count (input edits so far). Surfaced in the
    /// `.fm2` / `.bk2` `rerecordCount` header via [`Self::to_movie`].
    #[must_use]
    pub const fn rerecord_count(&self) -> u32 {
        self.rerecord_count
    }

    /// Seed the re-record tally from a loaded movie (so editing an imported
    /// `.fm2` / `.bk2` / `.rnm` continues counting from its recorded value).
    pub const fn set_rerecord_count(&mut self, count: u32) {
        self.rerecord_count = count;
    }

    /// Bump the snapshot-relevant edit counter (see [`Self::revision`]). Called
    /// by every mutation a host-built `TasSnapshot` would observe.
    const fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// The current edit revision — a monotonic counter the host compares
    /// frame-to-frame to decide whether to rebuild the (cloned) script snapshot.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Record frame `f`'s lag verdict from the core's poll flag (called right
    /// after `nes.run_frame()`). The frontend always builds the core with
    /// `debug-hooks`, so the flag is available unconditionally.
    fn record_lag(&mut self, f: usize, nes: &Nes) {
        if f >= self.lag_log.len() {
            self.lag_log.resize(f + 1, false);
        }
        self.lag_log[f] = !nes.was_input_polled_this_frame();
        self.bump();
    }

    /// Create an editor seeded from an existing input log (e.g. a loaded `.rnm`
    /// / imported `.fm2`). `nes` must be at the start state (frame 0).
    #[must_use]
    pub fn from_inputs(
        nes: &Nes,
        inputs: Vec<FrameInput>,
        budget_bytes: usize,
        capture_interval: usize,
    ) -> Self {
        let mut editor = Self::new(nes, budget_bytes, capture_interval);
        editor.input_log = inputs;
        editor.bump();
        editor
    }

    /// Number of frames in the input log.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.input_log.len()
    }

    /// `true` if the input log is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.input_log.is_empty()
    }

    /// The current edit/playback frame position.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// The input recorded at `frame`, or `None` past the end of the log.
    #[must_use]
    pub fn input_at(&self, frame: usize) -> Option<FrameInput> {
        self.input_log.get(frame).copied()
    }

    /// v1.8.9 — extract a `len`-frame input macro starting at `from` (frames past
    /// the end of the log read as empty), for the input-macro / pattern bank.
    #[must_use]
    pub fn extract_macro(&self, from: usize, len: usize) -> Vec<FrameInput> {
        (0..len)
            .map(|i| self.input_at(from.saturating_add(i)).unwrap_or_default())
            .collect()
    }

    /// v1.8.9 — stamp an input-macro pattern into the log starting at `start`
    /// (one [`Self::set_input`] per frame; each changed frame counts as a
    /// re-record). Returns the number of frames written.
    pub fn stamp_macro(&mut self, start: usize, frames: &[FrameInput]) -> usize {
        for (i, f) in frames.iter().enumerate() {
            self.set_input(start.saturating_add(i), *f);
        }
        frames.len()
    }

    /// Borrow the full input log (for export / the piano-roll grid).
    #[must_use]
    pub fn input_log(&self) -> &[FrameInput] {
        &self.input_log
    }

    /// Build a portable [`Movie`] from the current input log, carrying the TAS
    /// [`Self::rerecord_count`] into the movie (and thus the `.fm2` / `.bk2`
    /// `rerecordCount` header on export). `TAStudio` projects always replay from
    /// power-on; `region` and `rom_sha256` come from the running console.
    #[must_use]
    pub fn to_movie(&self, region: Region, rom_sha256: [u8; 32]) -> Movie {
        Movie {
            region,
            rom_sha256,
            start: StartPoint::PowerOn,
            frames: self.input_log.clone(),
            rerecord_count: self.rerecord_count,
        }
    }

    /// The greenzone (for the piano-roll's row colouring / diagnostics).
    #[must_use]
    pub const fn greenzone(&self) -> &Greenzone {
        &self.greenzone
    }

    /// Whether frame `f` was a lag frame (the program polled no controller that
    /// frame), or `None` if `f` has not been emulated yet. The piano-roll uses
    /// this to shade lag rows.
    #[must_use]
    pub fn lag_at(&self, f: usize) -> Option<bool> {
        self.lag_log.get(f).copied()
    }

    /// Number of lag frames recorded so far (the classic TAS lag count).
    #[must_use]
    pub fn lag_count(&self) -> usize {
        self.lag_log.iter().filter(|&&l| l).count()
    }

    /// Set (or rename) a named marker at `frame`, pinning it as a greenzone
    /// anchor so navigating back to it stays instant.
    pub fn set_marker(&mut self, frame: usize, label: impl Into<String>) {
        self.markers.insert(frame, label.into());
        self.greenzone.add_anchor(frame);
        self.bump();
    }

    /// Remove the marker at `frame` (dropping its greenzone anchor unless it is
    /// frame 0, which is always anchored). No-op if `frame` is unmarked.
    pub fn remove_marker(&mut self, frame: usize) {
        if self.markers.remove(&frame).is_some() {
            self.greenzone.remove_anchor(frame);
            self.bump();
        }
    }

    /// The marker label at `frame`, if any.
    #[must_use]
    pub fn marker_at(&self, frame: usize) -> Option<&str> {
        self.markers.get(&frame).map(String::as_str)
    }

    /// All markers in ascending frame order, as `(frame, label)`.
    pub fn markers(&self) -> impl Iterator<Item = (usize, &str)> + '_ {
        self.markers.iter().map(|(&f, l)| (f, l.as_str()))
    }

    /// The nearest marked frame strictly after `from` ("next marker" nav).
    #[must_use]
    pub fn next_marker(&self, from: usize) -> Option<usize> {
        self.markers
            .range(from.saturating_add(1)..)
            .next()
            .map(|(&f, _)| f)
    }

    /// The nearest marked frame strictly before `from` ("prev marker" nav).
    #[must_use]
    pub fn prev_marker(&self, from: usize) -> Option<usize> {
        self.markers.range(..from).next_back().map(|(&f, _)| f)
    }

    /// Rebuild the marker map after a frame insert / delete at `at`: every
    /// marker at-or-after the edit shifts by `delta` (+1 insert, -1 delete) and
    /// re-pins its greenzone anchor. A delete removes the marker landing on the
    /// deleted frame. Stale anchors past the edit are harmless (the greenzone is
    /// invalidated from the edit point anyway).
    fn shift_markers(&mut self, at: usize, delta: isize) {
        let old = std::mem::take(&mut self.markers);
        // Rebuild the anchor set from scratch so the shifted-away frames' stale
        // anchors don't accumulate in the greenzone.
        self.greenzone.clear_non_default_anchors();
        for (frame, label) in old {
            let new_frame = if frame < at {
                frame
            } else if delta < 0 && frame == at {
                continue; // marker on the deleted frame is dropped
            } else {
                frame.wrapping_add_signed(delta)
            };
            self.markers.insert(new_frame, label);
            self.greenzone.add_anchor(new_frame);
        }
    }

    /// Fork a new branch from the current state: snapshot the input log,
    /// markers, cursor, and `nes`'s save-state into a stored [`Branch`].
    /// `nes` MUST be at the cursor frame (the caller seeks first). Returns the
    /// new branch's index.
    pub fn create_branch(&mut self, nes: &Nes) -> usize {
        self.branches.push(Branch {
            frame: self.cursor,
            input_log: self.input_log.clone(),
            markers: self.markers.clone(),
            state: nes.snapshot(),
        });
        self.bump();
        self.branches.len() - 1
    }

    /// Number of saved branches.
    #[must_use]
    pub const fn branch_count(&self) -> usize {
        self.branches.len()
    }

    /// Borrow branch `idx`, if it exists.
    #[must_use]
    pub fn branch(&self, idx: usize) -> Option<&Branch> {
        self.branches.get(idx)
    }

    /// Delete branch `idx` (later branches shift down by one). No-op if absent.
    pub fn delete_branch(&mut self, idx: usize) {
        if idx < self.branches.len() {
            self.branches.remove(idx);
            self.bump();
        }
    }

    /// Load branch `idx`: restore its input log, markers, cursor, and emulator
    /// save-state. The greenzone is reset to the durable frame-0 anchor (the
    /// power-on state is input-independent) plus the branch's own state pinned
    /// at its frame, so seeking within the branch stays cheap; the lag log is
    /// dropped past the branch frame (re-derived on replay). Returns `false`
    /// (leaving `nes` untouched) if `idx` is out of range or the state is
    /// malformed.
    pub fn load_branch(&mut self, idx: usize, nes: &mut Nes) -> bool {
        let Some(b) = self.branches.get(idx).cloned() else {
            return false;
        };
        if nes.restore(&b.state).is_err() {
            return false;
        }
        self.input_log = b.input_log;
        self.markers = b.markers;
        self.cursor = b.frame;
        // Keep frame 0 (power-on, input-independent), drop the rest + every
        // stale marker anchor, then pin the branch state at its frame.
        self.greenzone.clear_non_default_anchors();
        self.greenzone.invalidate_after(0);
        self.greenzone.store(b.frame, b.state, b.frame);
        self.lag_log.truncate(b.frame.min(self.lag_log.len()));
        // Re-anchor the branch's markers (collect first to avoid borrowing self
        // across the mutable greenzone call).
        let marker_frames: Vec<usize> = self.markers.keys().copied().collect();
        for f in marker_frames {
            self.greenzone.add_anchor(f);
        }
        self.bump();
        true
    }

    /// Serialize the project — input log + markers + branches — to a `.rnmproj`
    /// byte blob. Deterministic (identical project ⇒ identical bytes). The
    /// greenzone and lag log are NOT serialized; they re-derive on load + seek.
    #[must_use]
    pub fn to_rnmproj(&self) -> Vec<u8> {
        let mut w = Vec::new();
        w.extend_from_slice(RNMPROJ_MAGIC);
        w.extend_from_slice(&RNMPROJ_VERSION.to_le_bytes());
        write_input_log(&mut w, &self.input_log);
        write_markers(&mut w, &self.markers);
        write_len(&mut w, self.branches.len());
        for b in &self.branches {
            write_len(&mut w, b.frame);
            write_input_log(&mut w, &b.input_log);
            write_markers(&mut w, &b.markers);
            write_bytes(&mut w, &b.state);
        }
        w
    }

    /// Load a `.rnmproj` blob: replace the input log / markers / branches, reset
    /// the greenzone to the durable frame-0 anchor (the power-on state set at
    /// construction stays valid — it is ROM-derived) + re-anchor the loaded
    /// markers, clear the lag log, and reset the cursor. The caller seeks to
    /// re-warm. On a malformed blob returns `Err` WITHOUT mutating the editor.
    ///
    /// # Errors
    /// [`RnmProjError`] for a bad magic, a too-new version, a truncated blob, or
    /// a non-UTF-8 marker label.
    pub fn load_rnmproj(&mut self, bytes: &[u8]) -> Result<(), RnmProjError> {
        let mut r = Reader::new(bytes);
        if r.take(8)? != RNMPROJ_MAGIC {
            return Err(RnmProjError::BadMagic);
        }
        let version = r.u16()?;
        if version > RNMPROJ_VERSION {
            return Err(RnmProjError::UnsupportedVersion {
                got: version,
                max: RNMPROJ_VERSION,
            });
        }
        let input_log = r.input_log()?;
        let markers = r.markers()?;
        let branch_count = r.len()?;
        // Bound the pre-allocation against the blob size (each branch needs at
        // least 4 length/frame u32s = 16 bytes); guards against an OOM from a
        // corrupt/hand-edited branch count.
        let mut branches = Vec::with_capacity(branch_count.min(r.remaining() / 16));
        for _ in 0..branch_count {
            let frame = r.len()?;
            let b_input = r.input_log()?;
            let b_markers = r.markers()?;
            let state = r.bytes()?;
            branches.push(Branch {
                frame,
                input_log: b_input,
                markers: b_markers,
                state,
            });
        }
        // Commit only after a fully-successful parse (no partial mutation).
        self.input_log = input_log;
        self.markers = markers;
        self.branches = branches;
        // Drop stale marker anchors before re-pinning the loaded set.
        self.greenzone.clear_non_default_anchors();
        self.greenzone.invalidate_after(0);
        let marker_frames: Vec<usize> = self.markers.keys().copied().collect();
        for f in marker_frames {
            self.greenzone.add_anchor(f);
        }
        self.lag_log.clear();
        self.cursor = 0;
        self.bump();
        Ok(())
    }

    /// Set the input at `frame`, growing the log with default (no-button)
    /// frames if the edit is past the current end. Invalidates the greenzone
    /// after `frame` — every cached state downstream of the edit is now stale
    /// and will be rebuilt on the next seek. Returns `true` if the stored input
    /// actually changed (so the caller can skip a redundant re-seek).
    pub fn set_input(&mut self, frame: usize, input: FrameInput) -> bool {
        if frame >= self.input_log.len() {
            self.input_log.resize(frame + 1, FrameInput::default());
        }
        if self.input_log[frame] == input {
            return false;
        }
        self.input_log[frame] = input;
        // The state *before* `frame` is unaffected; the state after applying
        // `frame`'s input (keyframe `frame+1` onward) is stale. Frame `frame`'s
        // own lag verdict can change (different input -> different execution),
        // so drop the lag log from `frame` onward too.
        self.greenzone.invalidate_after(frame);
        self.lag_log.truncate(frame);
        self.bump();
        self.bump_rerecord();
        true
    }

    /// Insert a blank frame at `frame`, shifting later inputs down by one.
    /// Invalidates the greenzone from the insertion point.
    pub fn insert_frame(&mut self, frame: usize) {
        let at = frame.min(self.input_log.len());
        self.input_log.insert(at, FrameInput::default());
        self.greenzone.invalidate_after(at.saturating_sub(1));
        self.lag_log.truncate(at);
        self.shift_markers(at, 1);
        self.bump();
        self.bump_rerecord();
    }

    /// Delete the frame at `frame`, shifting later inputs up by one. No-op past
    /// the end. Invalidates the greenzone from the deletion point.
    pub fn delete_frame(&mut self, frame: usize) {
        if frame < self.input_log.len() {
            self.input_log.remove(frame);
            self.greenzone.invalidate_after(frame.saturating_sub(1));
            self.lag_log.truncate(frame);
            self.shift_markers(frame, -1);
            self.cursor = self.cursor.min(self.input_log.len());
            self.bump();
            self.bump_rerecord();
        }
    }

    /// Deterministically seek `nes` to `target` (clamped to the log length).
    /// Restores the nearest cached state at or before `target`, then replays
    /// the input log forward to `target`, capturing keyframes every
    /// `capture_interval` frames along the way. Post-seek state is
    /// **bit-identical** to having played linearly to `target` (it drives the
    /// same `set_buttons` + `run_frame` path). `target == len` seeks to the end.
    ///
    /// O(distance to the nearest keyframe) frames of emulation — at most
    /// `capture_interval - 1` once the neighbourhood is warm.
    pub fn seek(&mut self, nes: &mut Nes, target: usize) {
        let target = target.min(self.input_log.len());
        let start = if let Some((frame, bytes)) = self.greenzone.nearest_at_or_before(target) {
            // The frame-0 anchor is always present, so this branch is the
            // normal path. A malformed blob would be a logic bug, not user
            // input, so surface it loudly. The greenzone now decompresses the
            // state losslessly (v1.7.0 D2) and hands back an owned blob.
            nes.restore(&bytes)
                .expect("greenzone holds only states this build produced");
            frame
        } else {
            // Defensive: no cached base (shouldn't happen — frame 0 is
            // anchored at construction). Fall back to a power-cycle.
            nes.power_cycle();
            0
        };
        for f in start..target {
            let input = self.input_log.get(f).copied().unwrap_or_default();
            nes.set_buttons(0, input.p1);
            nes.set_buttons(1, input.p2);
            nes.run_frame();
            self.record_lag(f, nes);
            let next = f + 1;
            if next.is_multiple_of(self.capture_interval) && !self.greenzone.has(next) {
                self.greenzone.store(next, nes.snapshot(), target);
            }
        }
        self.cursor = target;
        self.bump();
    }

    /// Append `input` as the next frame and advance `nes` one frame by it
    /// (the recording / live-drive path). Captures a keyframe per interval.
    pub fn record_frame(&mut self, nes: &mut Nes, input: FrameInput) {
        let frame = self.cursor;
        if frame >= self.input_log.len() {
            self.input_log.resize(frame + 1, FrameInput::default());
        }
        self.input_log[frame] = input;
        // Editing here invalidates any stale downstream cache.
        self.greenzone.invalidate_after(frame);
        nes.set_buttons(0, input.p1);
        nes.set_buttons(1, input.p2);
        nes.run_frame();
        self.record_lag(frame, nes);
        self.cursor = frame + 1;
        if self.cursor.is_multiple_of(self.capture_interval) && !self.greenzone.has(self.cursor) {
            self.greenzone
                .store(self.cursor, nes.snapshot(), self.cursor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_core::Buttons;

    /// Minimal NROM that loops forever (same fixture shape as the movie tests).
    fn synth_nrom() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"NES\x1A");
        bytes.push(1);
        bytes.push(1);
        bytes.push(0);
        bytes.push(0);
        bytes.extend_from_slice(&[0u8; 8]);
        let mut prg = vec![0u8; 16 * 1024];
        prg[0] = 0x4C; // JMP $C000
        prg[1] = 0x00;
        prg[2] = 0xC0;
        let len = prg.len();
        prg[len - 4] = 0x00;
        prg[len - 3] = 0xC0;
        prg[len - 6] = 0x00;
        prg[len - 5] = 0xC0;
        prg[len - 2] = 0x00;
        prg[len - 1] = 0xC0;
        bytes.extend_from_slice(&prg);
        bytes.extend_from_slice(&vec![0u8; 8 * 1024]);
        bytes
    }

    fn varied_inputs(n: usize) -> Vec<FrameInput> {
        (0..n)
            .map(|i| {
                let i = u8::try_from(i % 256).unwrap();
                FrameInput::new(
                    Buttons::from_bits_truncate(i.wrapping_mul(37)),
                    Buttons::from_bits_truncate(i.wrapping_mul(101).rotate_left(3)),
                )
            })
            .collect()
    }

    /// Review finding — the edit `revision` bumps on every snapshot-relevant
    /// mutation and stays put when nothing changed, so the host can skip the
    /// per-frame snapshot rebuild for an idle editor.
    #[test]
    fn revision_tracks_snapshot_relevant_mutations() {
        let mut nes = Nes::from_rom(&synth_nrom()).unwrap();
        nes.power_cycle();
        let mut ed = TasEditor::new(&nes, 1 << 20, 16);

        let r0 = ed.revision();
        // A real edit bumps.
        assert!(ed.set_input(0, FrameInput::new(Buttons::A, Buttons::empty())));
        let r1 = ed.revision();
        assert!(r1 > r0, "set_input must bump the revision");

        // A no-op set_input (same value) does NOT bump.
        assert!(!ed.set_input(0, FrameInput::new(Buttons::A, Buttons::empty())));
        assert_eq!(ed.revision(), r1, "an unchanged set_input must not bump");

        // A marker edit bumps; a no-op removemarker does not.
        ed.set_marker(0, "start");
        let r2 = ed.revision();
        assert!(r2 > r1);
        ed.remove_marker(999); // unmarked frame -> no-op
        assert_eq!(ed.revision(), r2, "a no-op removemarker must not bump");

        // A seek bumps (cursor / greenzone / lag can change).
        ed.seek(&mut nes, 1);
        assert!(ed.revision() > r2, "seek must bump the revision");
    }

    #[test]
    fn rerecord_count_tracks_input_edits_only() {
        let nes = {
            let mut n = Nes::from_rom(&synth_nrom()).unwrap();
            n.power_cycle();
            n
        };
        let mut ed = TasEditor::new(&nes, 1 << 20, 16);
        assert_eq!(ed.rerecord_count(), 0);

        // A real input edit bumps the re-record tally.
        assert!(ed.set_input(0, FrameInput::new(Buttons::A, Buttons::empty())));
        assert_eq!(ed.rerecord_count(), 1);

        // A no-op set_input (unchanged value) must NOT bump.
        assert!(!ed.set_input(0, FrameInput::new(Buttons::A, Buttons::empty())));
        assert_eq!(ed.rerecord_count(), 1);

        // A second change, then an insert + a delete: each is one re-record.
        assert!(ed.set_input(0, FrameInput::new(Buttons::B, Buttons::empty())));
        ed.insert_frame(0);
        ed.delete_frame(0);
        assert_eq!(ed.rerecord_count(), 4);

        // A non-input edit (a marker) bumps `revision`, NOT the re-record tally.
        ed.set_marker(0, "x");
        assert_eq!(ed.rerecord_count(), 4);

        // `to_movie` carries the tally into the exported movie (and thus the
        // `.fm2` / `.bk2` rerecordCount header).
        assert_eq!(ed.to_movie(Region::Ntsc, [0u8; 32]).rerecord_count, 4);

        // Seeding from a loaded movie continues the tally from its value.
        ed.set_rerecord_count(1000);
        assert_eq!(ed.rerecord_count(), 1000);
        assert_eq!(ed.to_movie(Region::Ntsc, [0u8; 32]).rerecord_count, 1000);
    }

    #[test]
    fn stamp_and_extract_macro_round_trip() {
        let nes = {
            let mut n = Nes::from_rom(&synth_nrom()).unwrap();
            n.power_cycle();
            n
        };
        let mut ed = TasEditor::new(&nes, 1 << 20, 16);
        let pattern = vec![
            FrameInput::new(Buttons::A, Buttons::empty()),
            FrameInput::new(Buttons::empty(), Buttons::empty()),
            FrameInput::new(Buttons::A | Buttons::RIGHT, Buttons::empty()),
        ];
        // Stamp at frame 5; the log grows to fit, earlier frames stay default.
        assert_eq!(ed.stamp_macro(5, &pattern), 3);
        assert_eq!(ed.input_at(5), Some(pattern[0]));
        assert_eq!(ed.input_at(7), Some(pattern[2]));
        assert_eq!(ed.input_at(0), Some(FrameInput::default()));
        // Extracting the same range reproduces the pattern; past the end is empty.
        assert_eq!(ed.extract_macro(5, 3), pattern);
        assert_eq!(ed.extract_macro(100, 2), vec![FrameInput::default(); 2]);
    }

    #[test]
    fn new_editor_anchors_frame_zero() {
        let nes = {
            let mut n = Nes::from_rom(&synth_nrom()).unwrap();
            n.power_cycle();
            n
        };
        let ed = TasEditor::new(&nes, 1 << 20, 16);
        assert!(ed.is_empty());
        assert_eq!(ed.cursor(), 0);
        // Frame 0 is cached so an immediate seek has a base.
        assert!(ed.greenzone().has(0));
    }

    /// The headline determinism guarantee: seeking to frame K via the greenzone
    /// lands on EXACTLY the framebuffer + cycle a linear replay of K frames
    /// produces. Seek re-derives; it never approximates.
    #[test]
    fn seek_is_bit_identical_to_linear_replay() {
        let rom = synth_nrom();
        let inputs = varied_inputs(200);

        // Linear reference: power-on + apply inputs 0..137.
        let mut linear = Nes::from_rom(&rom).unwrap();
        linear.power_cycle();
        for f in &inputs[..137] {
            linear.set_buttons(0, f.p1);
            linear.set_buttons(1, f.p2);
            linear.run_frame();
        }
        let ref_fb = linear.framebuffer().to_vec();
        let ref_cycle = linear.cycle();

        // Editor: seed the log, build a greenzone by seeking to the end, then
        // seek BACK to 137 (forcing a load-nearest-keyframe + short replay).
        let mut nes = Nes::from_rom(&rom).unwrap();
        nes.power_cycle();
        let mut ed = TasEditor::from_inputs(&nes, inputs, 1 << 24, 20);
        ed.seek(&mut nes, 200); // warm the greenzone across the whole movie
        ed.seek(&mut nes, 137); // seek back — uses a cached keyframe <= 137
        assert_eq!(ed.cursor(), 137);
        assert_eq!(
            nes.framebuffer(),
            ref_fb.as_slice(),
            "framebuffer must match"
        );
        assert_eq!(nes.cycle(), ref_cycle, "cycle count must match");
    }

    #[test]
    fn editing_input_invalidates_downstream_greenzone() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).unwrap();
        nes.power_cycle();
        let mut ed = TasEditor::from_inputs(&nes, varied_inputs(120), 1 << 24, 20);
        ed.seek(&mut nes, 120); // cache keyframes at 20,40,60,80,100,120
        assert!(ed.greenzone().cached_frames().any(|f| f == 100));
        // Edit frame 50 — everything cached after 50 must be invalidated.
        ed.set_input(50, FrameInput::new(Buttons::A, Buttons::empty()));
        let after: Vec<usize> = ed.greenzone().cached_frames().collect();
        assert!(
            after.iter().all(|&f| f <= 50),
            "no stale state past the edit: {after:?}"
        );
        assert!(after.contains(&0), "frame 0 anchor retained");
    }

    #[test]
    fn set_input_growing_past_end_is_a_change() {
        let nes = {
            let mut n = Nes::from_rom(&synth_nrom()).unwrap();
            n.power_cycle();
            n
        };
        let mut ed = TasEditor::new(&nes, 1 << 20, 16);
        assert!(ed.set_input(10, FrameInput::new(Buttons::A, Buttons::empty())));
        assert_eq!(ed.len(), 11);
        assert_eq!(ed.input_at(10).unwrap().p1, Buttons::A);
        // Setting the identical value again reports "no change".
        assert!(!ed.set_input(10, FrameInput::new(Buttons::A, Buttons::empty())));
    }

    #[test]
    fn record_frame_appends_and_advances() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).unwrap();
        nes.power_cycle();
        let mut ed = TasEditor::new(&nes, 1 << 24, 8);
        for f in &varied_inputs(25) {
            ed.record_frame(&mut nes, *f);
        }
        assert_eq!(ed.len(), 25);
        assert_eq!(ed.cursor(), 25);
        // Keyframes were captured at the interval (8, 16, 24).
        let cached: Vec<usize> = ed.greenzone().cached_frames().collect();
        for k in [8usize, 16, 24] {
            assert!(
                cached.contains(&k),
                "expected a keyframe at {k}: {cached:?}"
            );
        }
    }

    /// NROM whose program reads `$4016` every loop iteration (`LDA $4016; JMP`),
    /// so it polls input every frame — the non-lag counterpart to `synth_nrom`.
    fn synth_polling_nrom() -> Vec<u8> {
        let mut rom = synth_nrom();
        // PRG payload starts 16 bytes in (iNES header). Overwrite $C000 with
        // `LDA $4016` (AD 16 40) then `JMP $C000` (4C 00 C0).
        let prg = &mut rom[16..16 + 6];
        prg.copy_from_slice(&[0xAD, 0x16, 0x40, 0x4C, 0x00, 0xC0]);
        rom
    }

    #[test]
    fn lag_log_marks_frames_with_no_input_poll() {
        // synth_nrom never touches $4016/$4017 -> every frame is a lag frame.
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).unwrap();
        nes.power_cycle();
        let mut ed = TasEditor::new(&nes, 1 << 24, 8);
        for f in &varied_inputs(12) {
            ed.record_frame(&mut nes, *f);
        }
        for f in 0..12 {
            assert_eq!(ed.lag_at(f), Some(true), "frame {f} polled no input");
        }
        assert_eq!(ed.lag_count(), 12);
        assert_eq!(ed.lag_at(12), None, "unplayed frame has no lag verdict");
    }

    #[test]
    fn polling_rom_records_no_lag_frames() {
        // A program that reads $4016 every loop polls input each frame.
        let rom = synth_polling_nrom();
        let mut nes = Nes::from_rom(&rom).unwrap();
        nes.power_cycle();
        let mut ed = TasEditor::new(&nes, 1 << 24, 20);
        ed.seek(&mut nes, 0); // no-op; build via record
        for _ in 0..10 {
            ed.record_frame(&mut nes, FrameInput::default());
        }
        // Steady state: every frame after the boot/reset frame reaches the poll
        // loop and reads $4016, so none are lag. (Frame 0 — the reset frame,
        // before the program's poll loop is entered — may register as a lag
        // frame, exactly as FCEUX/BizHawk count the power-on frame. The lag log
        // faithfully reports the core's per-frame poll verdict either way.)
        let steady_lags: Vec<usize> = (1..10).filter(|&f| ed.lag_at(f) == Some(true)).collect();
        assert_eq!(
            steady_lags,
            Vec::<usize>::new(),
            "a polling program must not lag in steady state: {steady_lags:?}"
        );
        assert_eq!(ed.lag_at(5), Some(false));
    }

    #[test]
    fn editing_truncates_the_lag_log() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).unwrap();
        nes.power_cycle();
        let mut ed = TasEditor::from_inputs(&nes, varied_inputs(30), 1 << 24, 20);
        ed.seek(&mut nes, 30);
        assert_eq!(ed.lag_at(20), Some(true));
        // Editing frame 10 drops the lag log from frame 10 onward.
        ed.set_input(10, FrameInput::new(Buttons::A, Buttons::empty()));
        assert_eq!(ed.lag_at(9), Some(true), "pre-edit lag retained");
        assert_eq!(ed.lag_at(10), None, "lag from the edit onward is dropped");
        assert_eq!(ed.lag_at(20), None);
    }

    #[test]
    fn markers_set_get_remove_and_navigate() {
        let nes = {
            let mut n = Nes::from_rom(&synth_nrom()).unwrap();
            n.power_cycle();
            n
        };
        let mut ed = TasEditor::new(&nes, 1 << 20, 16);
        ed.set_marker(10, "start of level 1");
        ed.set_marker(50, "boss");
        ed.set_marker(30, "midpoint");
        assert_eq!(ed.marker_at(10), Some("start of level 1"));
        assert_eq!(ed.marker_at(11), None);
        // Ascending order.
        let all: Vec<(usize, &str)> = ed.markers().collect();
        assert_eq!(
            all,
            vec![(10, "start of level 1"), (30, "midpoint"), (50, "boss")]
        );
        // Navigation.
        assert_eq!(ed.next_marker(10), Some(30));
        assert_eq!(ed.next_marker(50), None);
        assert_eq!(ed.prev_marker(50), Some(30));
        assert_eq!(ed.prev_marker(10), None);
        // Marked frames are pinned as greenzone anchors.
        assert!(ed.greenzone().is_anchor(30));
        // Remove.
        ed.remove_marker(30);
        assert_eq!(ed.marker_at(30), None);
        assert_eq!(ed.next_marker(10), Some(50));
    }

    #[test]
    fn markers_shift_with_insert_and_delete() {
        let nes = {
            let mut n = Nes::from_rom(&synth_nrom()).unwrap();
            n.power_cycle();
            n
        };
        let mut ed = TasEditor::from_inputs(&nes, varied_inputs(60), 1 << 20, 16);
        ed.set_marker(5, "a");
        ed.set_marker(20, "b");
        // Insert a frame at 10: markers >= 10 shift +1 (5 stays, 20 -> 21).
        ed.insert_frame(10);
        assert_eq!(ed.marker_at(5), Some("a"));
        assert_eq!(ed.marker_at(21), Some("b"));
        assert_eq!(ed.marker_at(20), None);
        // Delete frame 5 (the marked frame): "a" is dropped, "b" shifts 21 -> 20.
        ed.delete_frame(5);
        assert_eq!(
            ed.marker_at(5),
            None,
            "marker on the deleted frame is dropped"
        );
        assert_eq!(ed.marker_at(20), Some("b"));
    }

    #[test]
    fn branches_fork_and_restore_full_project_state() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).unwrap();
        nes.power_cycle();
        let mut ed = TasEditor::from_inputs(&nes, varied_inputs(40), 1 << 24, 20);
        // Advance to frame 25 and mark it, then fork a branch there.
        ed.seek(&mut nes, 25);
        ed.set_marker(25, "fork point");
        let forked_cycle = nes.cycle();
        let forked_fb = nes.framebuffer().to_vec();
        let idx = ed.create_branch(&nes);
        assert_eq!(ed.branch_count(), 1);
        assert_eq!(ed.branch(idx).unwrap().frame, 25);

        // Diverge on "main": change input, drop the marker, extend the log.
        ed.set_input(
            10,
            FrameInput::new(Buttons::A | Buttons::B, Buttons::empty()),
        );
        ed.remove_marker(25);
        ed.set_input(60, FrameInput::new(Buttons::START, Buttons::empty()));
        assert!(ed.marker_at(25).is_none());
        assert_eq!(ed.len(), 61);

        // Load the branch — input log, markers, cursor, and nes state restored.
        assert!(ed.load_branch(idx, &mut nes));
        assert_eq!(ed.cursor(), 25);
        assert_eq!(ed.len(), 40, "branch input log restored");
        assert_eq!(
            ed.marker_at(25),
            Some("fork point"),
            "branch markers restored"
        );
        assert_eq!(nes.cycle(), forked_cycle, "nes restored to the fork state");
        assert_eq!(nes.framebuffer(), forked_fb.as_slice());

        // Seeking forward within the restored branch stays bit-identical to a
        // direct linear replay of the branch's input log.
        ed.seek(&mut nes, 40);
        let (direct_fb, direct_cycle) = {
            let mut n2 = Nes::from_rom(&rom).unwrap();
            n2.power_cycle();
            for f in &varied_inputs(40) {
                n2.set_buttons(0, f.p1);
                n2.set_buttons(1, f.p2);
                n2.run_frame();
            }
            (n2.framebuffer().to_vec(), n2.cycle())
        };
        assert_eq!(
            nes.framebuffer(),
            direct_fb.as_slice(),
            "branch replay bit-identical"
        );
        assert_eq!(nes.cycle(), direct_cycle);
    }

    #[test]
    fn delete_branch_removes_it() {
        let nes = {
            let mut n = Nes::from_rom(&synth_nrom()).unwrap();
            n.power_cycle();
            n
        };
        let mut ed = TasEditor::new(&nes, 1 << 20, 16);
        ed.create_branch(&nes);
        ed.create_branch(&nes);
        assert_eq!(ed.branch_count(), 2);
        ed.delete_branch(0);
        assert_eq!(ed.branch_count(), 1);
        ed.delete_branch(5); // out of range — no-op
        assert_eq!(ed.branch_count(), 1);
    }

    #[test]
    fn rnmproj_round_trips_input_markers_and_branches() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).unwrap();
        nes.power_cycle();
        let mut a = TasEditor::from_inputs(&nes, varied_inputs(30), 1 << 24, 20);
        a.set_marker(5, "alpha");
        a.set_marker(18, "beta");
        a.seek(&mut nes, 12);
        a.create_branch(&nes); // branch at 12 with the current input/markers/state
        let bytes = a.to_rnmproj();

        // Load into a fresh editor (same ROM power-on pinned at frame 0).
        let mut fresh = Nes::from_rom(&rom).unwrap();
        fresh.power_cycle();
        let mut b = TasEditor::new(&fresh, 1 << 24, 20);
        b.load_rnmproj(&bytes).expect("round-trip");
        assert_eq!(b.input_log(), a.input_log());
        assert_eq!(b.marker_at(5), Some("alpha"));
        assert_eq!(b.marker_at(18), Some("beta"));
        assert_eq!(b.branch_count(), 1);
        assert_eq!(b.branch(0), a.branch(0), "branch survives round-trip");
        assert_eq!(b.cursor(), 0, "cursor resets on load");
    }

    #[test]
    fn load_rnmproj_rejects_malformed_blobs_without_mutating() {
        let nes = {
            let mut n = Nes::from_rom(&synth_nrom()).unwrap();
            n.power_cycle();
            n
        };
        let mut ed = TasEditor::from_inputs(&nes, varied_inputs(5), 1 << 20, 16);
        let before = ed.input_log().to_vec();
        assert_eq!(ed.load_rnmproj(b"NOTMAGIC"), Err(RnmProjError::BadMagic));
        assert!(matches!(
            ed.load_rnmproj(&[]),
            Err(RnmProjError::Truncated(_))
        ));
        // A valid-magic-but-truncated body also errors, and the editor is intact.
        let mut short = RNMPROJ_MAGIC.to_vec();
        short.extend_from_slice(&RNMPROJ_VERSION.to_le_bytes());
        // claim 99 input frames but provide none
        short.extend_from_slice(&99u32.to_le_bytes());
        assert!(matches!(
            ed.load_rnmproj(&short),
            Err(RnmProjError::Truncated(_))
        ));
        assert_eq!(
            ed.input_log(),
            before.as_slice(),
            "editor unmutated on error"
        );
    }
}

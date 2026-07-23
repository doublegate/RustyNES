//! Scrubbable full-session timeline over the rewind ring, with
//! export-last-N-seconds-as-`.rnm` (v1.7.0 "Forge" Workstream D1).
//!
//! Mesen2's `HistoryViewer` spins a *second* emulator over the rewind ring so
//! the user can scrub the whole session on a timeline and pull a clip out of
//! it. `RustyNES` already has the deterministic substrate this needs — the
//! per-frame rewind ring ([`rustynes_core::RewindRing`], driven by
//! `Nes::rewind_capture`) and the deterministic movie format
//! ([`rustynes_core::Movie`], `.rnm`). This module is the **output-only**
//! bookkeeping layer that ties them together:
//!
//! - It records the per-frame **input stream** (`FrameInput` for both standard
//!   ports) in lock-step with the emulator's frame counter — the *same* inputs
//!   the rewind ring's snapshots were produced under, so a `.rnm` exported from
//!   here replays bit-identically.
//! - It periodically stashes a lightweight **start-anchor save-state** (every
//!   `anchor_period` frames) so an exported clip can begin from a real
//!   state at-or-before the requested window rather than only from power-on.
//! - It exposes a **scrubbable timeline** model (the span of recorded frames +
//!   which frames have a rewind snapshot) for the egui timeline widget to draw,
//!   and **`export_last_seconds`** which assembles a [`Movie`] covering exactly
//!   the trailing N seconds.
//!
//! # Determinism
//!
//! This type never touches the deterministic core's per-frame output: it only
//! *observes* the inputs the frontend already latched and *copies* the
//! save-state blobs the core already produces. It cannot perturb emulation —
//! exactly the property that makes the exported `.rnm` replay deterministically
//! (the movie format embeds the start state + the exact input stream, and
//! [`rustynes_core::MoviePlayer`] re-applies them frame-for-frame).
//!
//! Native + `wasm-winit` (the `.rnm` *bytes* are produced here; the frontend
//! routes them to a file dialog on native / a Blob download on wasm).

use std::collections::VecDeque;

use rustynes_core::{FrameInput, Movie, Nes, Region, StartPoint};

/// Default frames of input/anchor history to retain — 60 s @ 60 fps.
///
/// The rewind ring's own byte budget bounds how far back snapshots actually
/// reach; this caps the parallel input log so it can't outgrow it unboundedly.
pub const HISTORY_DEFAULT_MAX_FRAMES: usize = 60 * 60;

/// Default anchor period: stash a start-state every 60 frames (~1 s). Small
/// enough that an exported clip starts close to the requested boundary; large
/// enough that the anchor blobs stay cheap.
pub const HISTORY_DEFAULT_ANCHOR_PERIOD: u32 = 60;

/// One recorded frame of input, tagged with the monotonic record index it was
/// captured at and the emulator frame number reported at capture (for display
/// only — the emulator's `frame()` is not strictly monotone-by-1 at boot, so
/// ordering / eviction / export key off `seq`).
#[derive(Clone, Copy, Debug)]
struct InputRec {
    /// Monotonic record index (0, 1, 2, … one per [`HistoryViewer::record_frame`]).
    seq: u64,
    /// The emulator's `frame()` at capture — a timeline label only.
    nes_frame: u64,
    input: FrameInput,
}

/// A periodically-stashed start-state, so an exported clip can begin from a
/// real save-state at-or-before the export window.
#[derive(Clone)]
struct Anchor {
    /// The monotonic record index this state was stashed at (the state *before*
    /// this record's input is applied — seeking here, then replaying the input
    /// stream from this record forward, reconstructs the session).
    seq: u64,
    /// The deterministic save-state blob (`Nes::snapshot`).
    blob: Vec<u8>,
}

/// Errors from [`HistoryViewer::export_last_seconds`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// No frames have been recorded yet — nothing to export.
    Empty,
    /// No start-anchor exists at or before the requested window, so the clip
    /// cannot be reconstructed. (Power-cycle / clear, then play forward long
    /// enough for an anchor to land.)
    NoAnchor,
}

impl core::fmt::Display for ExportError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => f.write_str("history viewer: no frames recorded yet"),
            Self::NoAnchor => {
                f.write_str("history viewer: no start-state anchor at or before the clip window")
            }
        }
    }
}

impl std::error::Error for ExportError {}

/// Records the session timeline (input stream + start anchors) alongside the
/// emulator's rewind ring, and exports trailing clips as `.rnm` movies.
#[derive(Debug)]
pub struct HistoryViewer {
    /// Per-frame input log, oldest at the front, in frame order.
    inputs: VecDeque<InputRec>,
    /// Start-state anchors, oldest at the front, in frame order.
    anchors: VecDeque<Anchor>,
    /// Max retained input frames (the anchor count tracks this proportionally).
    max_frames: usize,
    /// Stash an anchor every Nth recorded frame.
    anchor_period: u32,
    /// Recorded frames since the last anchor.
    since_anchor: u32,
    /// Next monotonic record index handed out by [`Self::record_frame`].
    next_seq: u64,
    /// The emulator region + ROM hash, captured on the first record so an
    /// exported movie carries the correct header. `None` until armed.
    region: Option<Region>,
    rom_sha256: Option<[u8; 32]>,
}

impl Default for HistoryViewer {
    fn default() -> Self {
        Self::new(HISTORY_DEFAULT_MAX_FRAMES, HISTORY_DEFAULT_ANCHOR_PERIOD)
    }
}

impl std::fmt::Debug for Anchor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Anchor")
            .field("seq", &self.seq)
            .field("blob_len", &self.blob.len())
            .finish()
    }
}

impl HistoryViewer {
    /// New, empty history retaining at most `max_frames` of input and stashing
    /// a start-anchor every `anchor_period` frames. `anchor_period` of 0 is
    /// treated as 1.
    #[must_use]
    pub fn new(max_frames: usize, anchor_period: u32) -> Self {
        Self {
            inputs: VecDeque::new(),
            anchors: VecDeque::new(),
            max_frames: max_frames.max(1),
            anchor_period: anchor_period.max(1),
            since_anchor: 0,
            next_seq: 0,
            region: None,
            rom_sha256: None,
        }
    }

    /// Record one frame. Call once per *persistent* produced frame, AFTER the
    /// frontend latched its input and BEFORE/at `run_frame` (the same point
    /// [`rustynes_core::MovieRecorder::capture`] would fire) — the captured
    /// input is exactly what the upcoming frame consumes, and matches the input
    /// the rewind snapshot for this frame was produced under.
    ///
    /// Rewind / movie-replay / run-ahead speculative frames must NOT be
    /// recorded here (the caller gates them out, exactly as it gates rewind
    /// capture), so the log stays a faithful forward timeline.
    pub fn record_frame(&mut self, nes: &Nes) {
        // Capture header metadata once (cheap; the ROM can't change mid-session
        // without a reload, which clears us).
        if self.region.is_none() {
            self.region = Some(nes.region());
            self.rom_sha256 = Some(*nes.rom_sha256());
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        let nes_frame = nes.frame();
        let input = FrameInput {
            p1: nes.buttons(0),
            p2: nes.buttons(1),
            expansion: 0,
        };
        // Stash a start-anchor on the cadence (and always on the very first
        // recorded frame, so an early export still has a base).
        if self.anchors.is_empty() || self.since_anchor + 1 >= self.anchor_period {
            self.anchors.push_back(Anchor {
                seq,
                blob: nes.snapshot(),
            });
            self.since_anchor = 0;
        } else {
            self.since_anchor += 1;
        }
        self.inputs.push_back(InputRec {
            seq,
            nes_frame,
            input,
        });
        self.evict_to_budget();
    }

    /// Drop all recorded history (on ROM load / power-cycle). Header metadata
    /// is cleared too — it is recaptured on the next [`Self::record_frame`].
    pub fn clear(&mut self) {
        self.inputs.clear();
        self.anchors.clear();
        self.since_anchor = 0;
        self.next_seq = 0;
        self.region = None;
        self.rom_sha256 = None;
    }

    /// Number of recorded input frames.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inputs.len()
    }

    /// `true` if nothing has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty()
    }

    /// The recorded **record-index** span as `(first, last)`, or `None` if
    /// empty. Eviction / export key off this monotonic index; the scrubbable
    /// timeline draws over this range and labels ticks with
    /// [`Self::nes_frame_span`].
    #[must_use]
    pub fn frame_span(&self) -> Option<(u64, u64)> {
        match (self.inputs.front(), self.inputs.back()) {
            (Some(a), Some(b)) => Some((a.seq, b.seq)),
            _ => None,
        }
    }

    /// The recorded **emulator frame-number** span as `(first, last)` for
    /// timeline labelling, or `None` if empty. (The emulator frame number is
    /// not strictly monotone-by-1 at boot, so this is display-only.)
    #[must_use]
    pub fn nes_frame_span(&self) -> Option<(u64, u64)> {
        match (self.inputs.front(), self.inputs.back()) {
            (Some(a), Some(b)) => Some((a.nes_frame, b.nes_frame)),
            _ => None,
        }
    }

    /// The record indices that currently have a start-anchor save-state
    /// (ascending). The timeline widget marks these as cheap scrub targets.
    pub fn anchor_frames(&self) -> impl Iterator<Item = u64> + '_ {
        self.anchors.iter().map(|a| a.seq)
    }

    /// Number of start-anchors retained.
    #[must_use]
    pub fn anchor_count(&self) -> usize {
        self.anchors.len()
    }

    /// Export the trailing `seconds` of the session as a [`Movie`], ready to
    /// `serialize()` to `.rnm` bytes. `fps` is the region frame rate (~60 for
    /// NTSC, ~50 for PAL/Dendy) used to convert seconds → frames.
    ///
    /// The clip begins at the nearest start-anchor at-or-before
    /// `last_frame - seconds*fps` (so it covers *at least* the requested
    /// window) and runs through the most recent recorded frame. The movie's
    /// start point is that anchor's save-state, and its input stream is every
    /// recorded `FrameInput` from the anchor frame forward — so it replays
    /// bit-identically through [`rustynes_core::MoviePlayer`].
    ///
    /// # Errors
    ///
    /// [`ExportError::Empty`] if nothing is recorded; [`ExportError::NoAnchor`]
    /// if no anchor exists at-or-before the window (e.g. the history is shorter
    /// than the rewind ring's first anchor).
    pub fn export_last_seconds(&self, seconds: f64, fps: f64) -> Result<Movie, ExportError> {
        let (_first, last) = self.frame_span().ok_or(ExportError::Empty)?;
        // Non-negative, finite, and clamped well within u64 range before the
        // cast (a clip is at most a few hours of frames; 2^53 is astronomical).
        let frames_f = (seconds.max(0.0) * fps.max(1.0)).round().clamp(0.0, 9e15);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        // clamped non-negative finite
        let window_frames = frames_f as u64;
        let want_start = last.saturating_sub(window_frames);
        self.export_from(want_start)
    }

    /// Export from the nearest anchor at-or-before record index `want_start`
    /// through the end.
    ///
    /// # Errors
    ///
    /// As [`Self::export_last_seconds`].
    pub fn export_from(&self, want_start: u64) -> Result<Movie, ExportError> {
        if self.inputs.is_empty() {
            return Err(ExportError::Empty);
        }
        // Nearest anchor at or before `want_start`. Anchors are in record order.
        let anchor = self
            .anchors
            .iter()
            .rev()
            .find(|a| a.seq <= want_start)
            // Fall back to the earliest anchor if the window predates them all
            // (the clip then simply starts a little earlier than requested).
            .or_else(|| self.anchors.front())
            .ok_or(ExportError::NoAnchor)?;

        let region = self.region.ok_or(ExportError::NoAnchor)?;
        let rom_sha256 = self.rom_sha256.ok_or(ExportError::NoAnchor)?;

        // Input frames from the anchor record forward, in order.
        let frames: Vec<FrameInput> = self
            .inputs
            .iter()
            .filter(|r| r.seq >= anchor.seq)
            .map(|r| r.input)
            .collect();

        Ok(Movie {
            region,
            rom_sha256,
            start: StartPoint::SaveState(anchor.blob.clone()),
            frames,
            // An exported rewind window is a straight capture — no re-records.
            rerecord_count: 0,
        })
    }

    /// Trim the input log + anchors back to the frame budget. Anchors are
    /// trimmed to never reference a frame older than the oldest retained input
    /// (so every anchor still has a forward input stream to export from).
    fn evict_to_budget(&mut self) {
        while self.inputs.len() > self.max_frames {
            self.inputs.pop_front();
        }
        // Drop anchors strictly older than the oldest retained input EXCEPT
        // keep at least one (so an export always has a base). The kept-oldest
        // anchor may sit slightly before the input window; `export_from`
        // tolerates that (it only emits inputs at-or-after the anchor record).
        if let Some(&InputRec { seq: oldest, .. }) = self.inputs.front() {
            while self.anchors.len() > 1 {
                let drop = self
                    .anchors
                    .get(1)
                    .is_some_and(|second| second.seq <= oldest);
                if drop {
                    self.anchors.pop_front();
                } else {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)] // test-only pseudo-input byte casts
mod tests {
    use super::*;
    use rustynes_core::{Buttons, MoviePlayer};
    use std::path::PathBuf;

    fn rom(rel: &str) -> Vec<u8> {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = manifest
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root");
        std::fs::read(root.join("tests").join("roms").join(rel))
            .unwrap_or_else(|e| panic!("read {rel}: {e}"))
    }

    fn buttons_for(frame: u64) -> Buttons {
        Buttons::from_bits_truncate((frame.wrapping_mul(2_654_435_761) >> 24) as u8)
    }

    #[test]
    fn records_span_and_anchors() {
        let bytes = rom("assorted/flowing_palette.nes");
        let mut nes = Nes::from_rom(&bytes).expect("rom parses");
        let mut hv = HistoryViewer::new(1000, 10);
        for f in 0..50u64 {
            nes.set_buttons(0, buttons_for(f));
            hv.record_frame(&nes);
            nes.run_frame();
        }
        assert_eq!(hv.len(), 50);
        // Record indices are monotonic-by-1 (decoupled from the emulator's
        // boot-time frame numbering).
        let (first, last) = hv.frame_span().expect("a span");
        assert_eq!(first, 0);
        assert_eq!(last, 49);
        // Anchor every 10 records -> record indices 0,10,20,30,40 (first forced).
        let anchors: Vec<u64> = hv.anchor_frames().collect();
        assert_eq!(anchors, vec![0, 10, 20, 30, 40]);
    }

    #[test]
    fn budget_evicts_oldest_input_but_keeps_a_base_anchor() {
        let bytes = rom("assorted/flowing_palette.nes");
        let mut nes = Nes::from_rom(&bytes).expect("rom parses");
        let mut hv = HistoryViewer::new(20, 10);
        for f in 0..60u64 {
            nes.set_buttons(0, buttons_for(f));
            hv.record_frame(&nes);
            nes.run_frame();
        }
        assert_eq!(hv.len(), 20, "input log capped at the frame budget");
        // 60 recorded (seq 0..59), budget 20 -> oldest retained record is 40.
        assert!(hv.anchor_count() >= 1);
        assert!(hv.frame_span().unwrap().0 >= 40);
    }

    /// THE D1 EXPORT CONTRACT: an exported clip replays bit-identically. We
    /// record a session, export the last ~0.5 s, then replay the movie on a
    /// fresh emulator and confirm the final framebuffer equals the live one.
    #[test]
    fn exported_clip_replays_bit_identically() {
        let bytes = rom("assorted/flowing_palette.nes");
        let mut nes = Nes::from_rom(&bytes).expect("rom parses");
        let mut hv = HistoryViewer::new(10_000, 10);

        // Record 60 forward frames with deterministic pseudo-input.
        for f in 0..60u64 {
            nes.set_buttons(0, buttons_for(f));
            hv.record_frame(&nes);
            nes.run_frame();
        }
        let live_fb = nes.framebuffer().to_vec();
        let live_cycle = nes.cycle();

        // Export the trailing ~0.5 s (30 frames @ 60 fps). The clip starts at
        // the nearest anchor <= frame (60 - 30) = 30, i.e. frame 30.
        let movie = hv.export_last_seconds(0.5, 60.0).expect("export");
        assert!(matches!(movie.start, StartPoint::SaveState(_)));
        // The clip runs from the anchor frame (<= 30) through frame 59.
        assert!(
            movie.len() >= 30,
            "clip covers at least the requested window"
        );

        // Round-trip the bytes too (the `.rnm` an export writes).
        let rnm = movie.serialize();
        let movie = Movie::deserialize(&rnm).expect("rnm round-trips");

        // Replay the clip on a fresh emulator and compare the END state.
        let mut replay = Nes::from_rom(&bytes).expect("rom parses");
        movie.seek_to_start(&mut replay).expect("seek to start");
        let mut player = MoviePlayer::new(&movie);
        while player.apply_next(&mut replay) {
            replay.run_frame();
        }
        assert_eq!(
            replay.cycle(),
            live_cycle,
            "replay end cycle must match live"
        );
        assert_eq!(
            replay.framebuffer(),
            live_fb.as_slice(),
            "replayed clip end framebuffer must be bit-identical to the live session"
        );
    }

    #[test]
    fn export_empty_is_an_error() {
        let hv = HistoryViewer::default();
        assert_eq!(hv.export_last_seconds(5.0, 60.0), Err(ExportError::Empty));
    }

    #[test]
    fn clear_resets() {
        let bytes = rom("assorted/flowing_palette.nes");
        let mut nes = Nes::from_rom(&bytes).expect("rom parses");
        let mut hv = HistoryViewer::default();
        nes.run_frame();
        hv.record_frame(&nes);
        assert!(!hv.is_empty());
        hv.clear();
        assert!(hv.is_empty());
        assert_eq!(hv.anchor_count(), 0);
    }
}

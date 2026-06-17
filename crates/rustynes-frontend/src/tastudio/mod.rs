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
//! 2. **Greenzone** — the frame-keyed save-state cache ([`greenzone`]).
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

use rustynes_core::{FrameInput, Nes};

/// Default greenzone byte budget (256 MiB) — generous for desktop TAS work
/// while bounded. Tunable by the frontend; the eviction policy keeps the
/// cursor neighbourhood dense within whatever budget is set.
pub const DEFAULT_GREENZONE_BUDGET: usize = 256 * 1024 * 1024;

/// Default keyframe spacing while seeking/playing: store a save-state every N
/// frames so any later seek only re-emulates at most N-1 frames forward.
pub const DEFAULT_CAPTURE_INTERVAL: usize = 60;

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
        }
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

    /// Borrow the full input log (for export / the piano-roll grid).
    #[must_use]
    pub fn input_log(&self) -> &[FrameInput] {
        &self.input_log
    }

    /// The greenzone (for the piano-roll's row colouring / diagnostics).
    #[must_use]
    pub const fn greenzone(&self) -> &Greenzone {
        &self.greenzone
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
        // `frame`'s input (keyframe `frame+1` onward) is stale.
        self.greenzone.invalidate_after(frame);
        true
    }

    /// Insert a blank frame at `frame`, shifting later inputs down by one.
    /// Invalidates the greenzone from the insertion point.
    pub fn insert_frame(&mut self, frame: usize) {
        let at = frame.min(self.input_log.len());
        self.input_log.insert(at, FrameInput::default());
        self.greenzone.invalidate_after(at.saturating_sub(1));
    }

    /// Delete the frame at `frame`, shifting later inputs up by one. No-op past
    /// the end. Invalidates the greenzone from the deletion point.
    pub fn delete_frame(&mut self, frame: usize) {
        if frame < self.input_log.len() {
            self.input_log.remove(frame);
            self.greenzone.invalidate_after(frame.saturating_sub(1));
            self.cursor = self.cursor.min(self.input_log.len());
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
            // input, so surface it loudly.
            nes.restore(bytes)
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
            let next = f + 1;
            if next.is_multiple_of(self.capture_interval) && !self.greenzone.has(next) {
                self.greenzone.store(next, nes.snapshot(), target);
            }
        }
        self.cursor = target;
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
}

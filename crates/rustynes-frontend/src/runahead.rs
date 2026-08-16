//! v2.8.0 Phase 3 — run-ahead: removing the game's OWN input lag.
//!
//! Most NES games sample the controller in their NMI handler and apply it
//! to gameplay one or more frames later — latency that exists on real
//! hardware. Run-ahead (bsnes pioneered it; Mesen2/RetroArch ship it)
//! removes those internal lag frames with emulation speed: each visible
//! frame, the emulator
//!
//! 1. runs ONE frame normally with the freshly latched input — this is the
//!    **persistent** frame, the real timeline (rewind capture stays on,
//!    its audio is discarded);
//! 2. saves state (the Phase 3 `snapshot_core_into` fast path — ~40 µs);
//! 3. runs `N-1` more hidden frames, then one **visible** frame whose
//!    framebuffer + audio reach the user — the screen shows the state `N`
//!    frames in the future, where the game has already reacted to the
//!    input;
//! 4. restores back to the persistent frame (`restore_quiet` — the rewind
//!    ring is untouched).
//!
//! Cost per visible frame: `N` extra `run_frame`s + one snapshot + one
//! restore (~2.7 ms for N=1 on the bench host — comfortably inside the
//! 16.64 ms NTSC budget; see `docs/benchmarks.md` §8). The persistent
//! timeline is byte-identical to a run-ahead-less run given the same
//! inputs — proven by the unit test below — so determinism, save-states,
//! movies, and netplay semantics are unaffected (run-ahead is auto-disabled
//! during netplay and movie record/playback at the call site).
//!
//! Audio continuity: every cycle outputs the audio of frame `persistent+N`,
//! so consecutive cycles produce the contiguous stream `N+1, N+2, …` — no
//! gaps, no overlaps, just shifted by the same `N` frames as the video.

use rustynes_core::Nes;

/// Scratch state for the run-ahead cycle (reused buffers — no per-frame
/// allocation in steady state).
#[derive(Debug, Default)]
pub struct RunAhead {
    /// Persistent-frame snapshot (core fast path, no thumbnail).
    snap_buf: Vec<u8>,
    /// Discard sink for muted frames' audio.
    audio_discard: Vec<f32>,
}

impl RunAhead {
    /// Phase A of the cycle: run the persistent frame + `n-1` hidden
    /// frames + the visible frame. On return, `nes` holds the VISIBLE
    /// frame's framebuffer and its un-drained audio — the caller copies
    /// the framebuffer out and drains/pushes the audio, then MUST call
    /// [`Self::finish`] to roll back to the persistent frame.
    ///
    /// `n` is the run-ahead depth (>= 1; callers route `n == 0` to a plain
    /// `run_frame`).
    pub fn run_frame_ahead(&mut self, nes: &mut Nes, n: u32) {
        debug_assert!(n >= 1);
        // The persistent frame: the real timeline. Rewind capture stays ON
        // (this is the frame the ring should hold); its audio is discarded
        // (the DAC carries the visible timeline).
        nes.run_frame();
        self.discard_audio(nes);
        nes.snapshot_core_into(&mut self.snap_buf);
        // Hidden + visible frames are off-timeline: no rewind capture.
        nes.set_rewind_capture(false);
        for _ in 1..n {
            nes.run_frame();
            self.discard_audio(nes);
        }
        // The visible frame — its framebuffer + audio are the user-facing
        // output, harvested by the caller before `finish`.
        nes.run_frame();
    }

    /// Phase B: roll back to the persistent frame and re-enable rewind
    /// capture. Call after harvesting the visible framebuffer + audio.
    ///
    /// # Debug provenance (v2.3.6 workstream 0, defect 1)
    ///
    /// The rollback is carried around the pixel-provenance and write-attribution
    /// stores rather than through them. `restore_quiet` clears both, because for
    /// its other callers — a user-driven save-state load, netplay rollback — the
    /// records describe a timeline the restore replaced, and reporting them would
    /// be confidently wrong.
    ///
    /// Run-ahead is the exception, and not because its restore is different: it
    /// is because of *when* it happens. This rollback is the last thing before
    /// the frontend releases the emulator lock, so the UI's first opportunity to
    /// look is always after it. Clearing here does not discard a stale timeline;
    /// it discards the record for the visible frame harvested three lines above,
    /// which is the exact frame on screen. Run-ahead defaults to 1, so this made
    /// the Pixel Provenance inspector show an empty report for every user from
    /// v2.3.2 until this fix.
    ///
    /// Stashing is a move of two boxed stores, and it is skipped entirely when
    /// neither is armed — which is the shipped default. `restore_quiet`'s own
    /// reasoning is untouched: it still sees `None` and still clears for
    /// everybody else.
    ///
    /// # Panics
    ///
    /// Panics if the snapshot fails to restore — impossible for a blob
    /// produced by `snapshot_core_into` on the same instance (the same
    /// guarantee netplay rollback relies on).
    pub fn finish(&mut self, nes: &mut Nes) {
        let stash = nes.take_provenance();
        nes.restore_quiet(&self.snap_buf)
            .expect("run-ahead snapshot round-trips on the same instance");
        if stash.is_armed() {
            nes.put_provenance(stash);
        }
        nes.set_rewind_capture(true);
    }

    /// Drain and discard whatever audio the last frame synthesized.
    fn discard_audio(&mut self, nes: &mut Nes) {
        // Generously sized: one NTSC frame at 192 kHz is ~3200 samples.
        if self.audio_discard.len() < 8192 {
            self.audio_discard.resize(8192, 0.0);
        }
        let _ = nes.drain_audio_into(&mut self.audio_discard);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_core::Buttons;
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

    /// Deterministic pseudo-input so the timelines exercise real button
    /// state without wall-clock dependence.
    fn buttons_for(frame: u32) -> Buttons {
        Buttons::from_bits_truncate((frame.wrapping_mul(2_654_435_761) >> 24) as u8)
    }

    /// THE Phase 3 contract: with identical inputs, the run-ahead PERSISTENT
    /// timeline is frame-for-frame identical to a plain run, and each
    /// VISIBLE frame equals the plain run one frame later (N=1).
    #[test]
    fn runahead_persistent_timeline_matches_plain_run() {
        let bytes = rom("assorted/flowing_palette.nes");
        let mut ahead = Nes::from_rom(&bytes).expect("rom parses");
        let mut plain = Nes::from_rom(&bytes).expect("rom parses");
        // Rewind armed on BOTH so the capture-suppression path is exercised.
        ahead.enable_rewind();
        plain.enable_rewind();

        let mut ra = RunAhead::default();
        let mut discard = vec![0.0f32; 8192];

        // Warm up both identically.
        for f in 0..30u32 {
            ahead.set_buttons(0, buttons_for(f));
            plain.set_buttons(0, buttons_for(f));
            ahead.run_frame();
            plain.run_frame();
            let _ = ahead.drain_audio_into(&mut discard);
            let _ = plain.drain_audio_into(&mut discard);
        }

        for f in 30..90u32 {
            let input = buttons_for(f);

            // Run-ahead cycle (N=1) on `ahead`.
            ahead.set_buttons(0, input);
            ra.run_frame_ahead(&mut ahead, 1);
            let visible_fb = ahead.framebuffer().to_vec();
            let _ = ahead.drain_audio_into(&mut discard); // visible audio
            ra.finish(&mut ahead);

            // Plain frame on `plain`.
            plain.set_buttons(0, input);
            plain.run_frame();
            let _ = plain.drain_audio_into(&mut discard);

            // Persistent state == plain state (gameplay surface: the
            // framebuffer + cumulative cycle are the proven byte-
            // deterministic comparators; full snapshots differ only in
            // audio-drain transients).
            assert_eq!(ahead.cycle(), plain.cycle(), "cycle diverged at {f}");
            assert_eq!(
                ahead.framebuffer(),
                plain.framebuffer(),
                "persistent framebuffer diverged at {f}"
            );

            // The visible frame is the plain timeline one frame ahead,
            // given the same input held for that future frame.
            let mut probe = Nes::from_rom(&bytes).expect("rom parses");
            // (Cheaper: clone `plain` via snapshot round-trip.)
            let snap = plain.snapshot();
            probe.restore(&snap).expect("restore probe");
            probe.set_buttons(0, input);
            probe.run_frame();
            assert_eq!(
                visible_fb.as_slice(),
                probe.framebuffer(),
                "visible frame != plain future frame at {f}"
            );
        }
    }

    /// v1.0.0 (UX3 BUG-3) — Game Genie codes are a runtime PRG-read overlay
    /// that lives OUTSIDE the save-state, so the run-ahead snapshot/restore
    /// (`snapshot_core_into` + `restore_quiet`, run every visible frame) must
    /// NOT drop them. Verify the code survives a full run-ahead cycle (it would
    /// be wiped every frame if restore cleared the overlay), so BUG-3 is not a
    /// run-ahead interaction.
    #[test]
    fn runahead_preserves_genie_codes() {
        let bytes = rom("assorted/flowing_palette.nes");
        let mut nes = Nes::from_rom(&bytes).expect("rom parses");
        nes.enable_rewind();
        nes.add_genie_code("SXIOPO").expect("valid 6-char code");
        assert_eq!(nes.genie_codes().count(), 1, "code added");

        let mut ra = RunAhead::default();
        let mut discard = vec![0.0f32; 8192];
        for _ in 0..5u32 {
            ra.run_frame_ahead(&mut nes, 1);
            let _ = nes.drain_audio_into(&mut discard);
            ra.finish(&mut nes);
            // The overlay must survive every snapshot/restore cycle.
            assert_eq!(
                nes.genie_codes().count(),
                1,
                "genie code dropped by a run-ahead snapshot/restore cycle"
            );
        }
    }

    /// Regression: run-ahead must stay byte-identical to a plain run on a game
    /// that toggles rendering MID-FRAME (Wizards & Warriors' sprite-0-hit
    /// status-bar split). That path exercises the per-sprite shifter-halt state,
    /// the 1-dot-delayed rendering gate, and the OAM-row-corruption arming state
    /// — all of which were unserialized before the PPU snapshot v6 tail, so the
    /// per-frame run-ahead snapshot/restore drifted them and corrupted the
    /// playfield + sprites (the desktop W&W bug). Uses a commercial dump staged
    /// under the gitignored `tests/roms/external/`; skips cleanly when absent so
    /// CI (which has no commercial ROMs) still passes. See ADR 0030 / the PPU
    /// snapshot v6 note.
    #[test]
    fn ww_runahead_matches_plain_across_a_mid_frame_split() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = manifest.parent().and_then(|p| p.parent()).unwrap();
        let path = root.join("tests/roms/external/mapper-007-AxROM/Wizards & Warriors.nes");
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("skipping: {} absent (no commercial dump)", path.display());
            return;
        };
        let mut ahead = Nes::from_rom(&bytes).expect("rom parses");
        let mut plain = Nes::from_rom(&bytes).expect("rom parses");
        ahead.enable_rewind();
        plain.enable_rewind();
        let mut ra = RunAhead::default();
        let mut discard = vec![0.0f32; 8192];
        // Warm both identically into real gameplay (repeated START past title).
        for f in 0..420u32 {
            let b = if f >= 120 && f % 60 < 2 {
                Buttons::START
            } else {
                Buttons::empty()
            };
            ahead.set_buttons(0, b);
            plain.set_buttons(0, b);
            ahead.run_frame();
            plain.run_frame();
            let _ = ahead.drain_audio_into(&mut discard);
            let _ = plain.drain_audio_into(&mut discard);
        }
        // Run-ahead (N=1) vs plain — the persistent timeline must not drift.
        for f in 420..600u32 {
            ahead.set_buttons(0, Buttons::empty());
            ra.run_frame_ahead(&mut ahead, 1);
            let _ = ahead.drain_audio_into(&mut discard);
            ra.finish(&mut ahead);
            plain.set_buttons(0, Buttons::empty());
            plain.run_frame();
            let _ = plain.drain_audio_into(&mut discard);
            assert_eq!(ahead.cycle(), plain.cycle(), "cycle diverged at f{f}");
            assert_eq!(
                ahead.framebuffer(),
                plain.framebuffer(),
                "run-ahead framebuffer diverged from plain at f{f}"
            );
        }
    }

    /// The pixel a provenance query asks about in the tests below. Screen
    /// centre, matching the inspector panel's own default coordinate.
    const PROBE_X: usize = 128;
    const PROBE_Y: usize = 120;

    /// CONTROL for [`runahead_preserves_pixel_provenance`]: without run-ahead,
    /// a plain `run_frame` leaves a populated provenance record.
    ///
    /// This exists so the sibling test's failure cannot be misread as "the
    /// assertion is wrong" or "this ROM emits nothing". `dot` is the marker
    /// because `Ppu::emit_pixel` stamps it on every emitted pixel as `x + 1`
    /// (so 129 here), while `PixelProvenanceFrame::clear` writes the `Default`
    /// record, whose `dot` is 0 — a value no real record can hold.
    #[test]
    fn plain_run_leaves_pixel_provenance_populated() {
        let bytes = rom("assorted/flowing_palette.nes");
        let mut nes = Nes::from_rom(&bytes).expect("rom parses");
        nes.set_pixel_provenance(true);
        let mut discard = vec![0.0f32; 8192];

        for _ in 0..3u32 {
            nes.run_frame();
            let _ = nes.drain_audio_into(&mut discard);
        }

        let frame = nes
            .pixel_provenance()
            .expect("armed, so the frame is allocated");
        let rec = frame.get(PROBE_X, PROBE_Y).expect("on-screen coordinate");
        assert_ne!(
            rec.dot, 0,
            "control failed: a plain run recorded nothing, so the marker used by \
             `runahead_preserves_pixel_provenance` is not valid"
        );
    }

    /// v2.3.6 workstream 0, defect 1 — run-ahead must not destroy the VISIBLE
    /// frame's provenance record before the UI can read it.
    ///
    /// `finish` rolls back to the persistent frame with `restore_quiet`, and
    /// `Nes::restore_inner` unconditionally clears both provenance stores. That
    /// clear is correct for a user-driven save-state load and for netplay
    /// rollback, and wrong here: run-ahead's rollback is the LAST thing that
    /// happens before the frontend hands the emulator lock to the UI, so the
    /// panel could only ever observe the wiped state. Since run-ahead defaults
    /// to 1, that made the shipped Pixel Provenance inspector render a
    /// complete, confident, entirely empty report for every user.
    ///
    /// The records kept are the visible frame's — one frame ahead of the
    /// restored persistent state, and exactly the frame on screen.
    #[test]
    fn runahead_preserves_pixel_provenance() {
        let bytes = rom("assorted/flowing_palette.nes");
        let mut nes = Nes::from_rom(&bytes).expect("rom parses");
        nes.enable_rewind();
        nes.set_pixel_provenance(true);
        nes.set_write_attribution(true);

        let mut ra = RunAhead::default();
        let mut discard = vec![0.0f32; 8192];

        for _ in 0..3u32 {
            ra.run_frame_ahead(&mut nes, 1);
            // The frontend harvests the visible framebuffer + audio here.
            let _ = nes.drain_audio_into(&mut discard);
            ra.finish(&mut nes);

            // ...and only THEN releases the lock, so this is the first moment
            // the UI could look.
            let frame = nes
                .pixel_provenance()
                .expect("armed, so the frame is allocated");
            let rec = frame.get(PROBE_X, PROBE_Y).expect("on-screen coordinate");
            assert_ne!(
                rec.dot, 0,
                "run-ahead's rollback wiped the visible frame's provenance: the \
                 inspector panel can never see a record"
            );
        }
    }

    /// Arming must survive the rollback too — a wiped-and-disarmed store would
    /// make the panel say "enable it, then run a frame" forever.
    #[test]
    fn runahead_preserves_the_provenance_arm() {
        let bytes = rom("assorted/flowing_palette.nes");
        let mut nes = Nes::from_rom(&bytes).expect("rom parses");
        nes.enable_rewind();
        nes.set_pixel_provenance(true);
        nes.set_write_attribution(true);

        let mut ra = RunAhead::default();
        let mut discard = vec![0.0f32; 8192];
        ra.run_frame_ahead(&mut nes, 2);
        let _ = nes.drain_audio_into(&mut discard);
        ra.finish(&mut nes);

        assert!(
            nes.pixel_provenance().is_some(),
            "pixel provenance disarmed by the run-ahead rollback"
        );
        assert!(
            nes.write_attribution().is_some(),
            "write attribution disarmed by the run-ahead rollback"
        );
    }

    /// Rewind-ring contents: run-ahead must push exactly one (persistent)
    /// frame per cycle — the hidden + visible frames never land in the
    /// ring, and the rollback must not clear it.
    #[test]
    fn runahead_rewind_ring_holds_persistent_frames_only() {
        let bytes = rom("assorted/flowing_palette.nes");
        let mut nes = Nes::from_rom(&bytes).expect("rom parses");
        nes.enable_rewind();
        let mut ra = RunAhead::default();
        let mut discard = vec![0.0f32; 8192];

        for _ in 0..10u32 {
            ra.run_frame_ahead(&mut nes, 2);
            let _ = nes.drain_audio_into(&mut discard);
            ra.finish(&mut nes);
        }
        // 10 cycles -> exactly 10 ring entries (one per persistent frame).
        assert_eq!(nes.rewind_len(), 10);
    }
}

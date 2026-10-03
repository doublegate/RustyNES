//! TAS movie recording / playback UI state (v1.4.0 Sprint 4.2).
//!
//! This is the frontend plumbing on top of the deterministic movie CORE
//! that landed in Sprint 4.1 (`rustynes_core::{Movie, MovieRecorder,
//! MoviePlayer, StartPoint}`). The core is caller-driven: the recorder's
//! `capture` reads `Nes::buttons(0/1)` and must be called AFTER the
//! frontend's `set_buttons` and BEFORE `run_frame`; the player's
//! `apply_next` sets the buttons itself and must be called BEFORE
//! `run_frame`. This module wires those two hooks into the frontend's
//! per-frame produce path (`App::produce_one_frame`) and tracks the
//! record / play / idle mode for the egui status indicator.
//!
//! Determinism is unchanged: playback drives the SAME `set_buttons` +
//! `run_frame` the live path does, so a replay re-derives every pixel and
//! sample bit-for-bit (proven by the Sprint 4.1 round-trip tests).
//!
//! # Save / load
//!
//! - **Native**: `.rnm` files via the `rfd` file dialog (the same dep the
//!   ROM-open path uses). See `App::movie_save_dialog` /
//!   `App::movie_open_dialog`.
//! - **wasm32** (since v1.6.0 Sprint 4): F6 records from a fresh power-on and
//!   stopping hands the `.rnm` bytes to a browser download
//!   (`App::handle_movie_record_toggle_wasm`); a `.rnm` chosen in the browser
//!   file picker arrives as `AppEvent::MovieLoaded` and plays back. Movies are
//!   not persisted in browser storage (no `IndexedDB`). This module is
//!   target-agnostic and holds no native-only types.
//!
//! # Emulation options (v2.9.8)
//!
//! A `.rnm` carries the [`HardwareOptions`] it was recorded with, and the
//! movie's options -- not the player's Settings -- decide how it replays:
//!
//! - **Playback** captures the player's options, lets
//!   [`Movie::seek_to_start`] apply the movie's, and re-asserts them before
//!   every frame ([`HardwareOptions::apply_live`]), so a Settings change, a
//!   cheat-panel resync or a game-database edit made mid-movie cannot reach
//!   the replayed run. [`MovieUi::stop_playback`] puts the player's options
//!   back ([`HardwareOptions::restore_after_playback`]).
//! - **Recording** captures the options once, at the start, and holds them
//!   the same way for the length of the recording, because the movie records
//!   them once. The overclock is held at stock timing while recording (the
//!   v2.9.7 rule), so a recording always says 0 extra scanlines.
//!
//! Settings changed while a movie runs therefore take effect when it stops
//! (playback) or at the next ROM load or power cycle (recording).

use rustynes_core::{HardwareOptions, Movie, MovieError, MovieRecorder, Nes};

/// A read-only port-topology + timebase snapshot for the Replay / TAS window.
///
/// v1.5.0 "Lens" Workstream C2. Frontend-only; gathered each frame from the
/// host config (`[input]`) + `Nes::region`, never touches the replay/synthesis
/// path so the determinism contract is unaffected.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplayInfo {
    /// Region label ("NTSC" / "PAL" / "Dendy").
    pub region: &'static str,
    /// Whole-Hz frame rate for the region (60 NTSC/Dendy, 50 PAL) — used for
    /// the elapsed-time readout. Display-only approximation of the precise
    /// 60.0988 / 50.007 Hz; clearly a wall-clock estimate, not a timing source.
    pub region_hz: u32,
    /// Player-1 device label (always the standard pad today).
    pub port1: &'static str,
    /// Player-2 / expansion device label (standard pad, or the attached
    /// expansion peripheral — Zapper, Vaus, SNES mouse, Power Pad, keyboard...).
    pub port2: &'static str,
    /// `true` when the Four Score 4-player adapter is active (ports multiplex
    /// P1..P4).
    pub four_score: bool,
}

/// The current movie mode the frontend is in.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MovieMode {
    /// Neither recording nor playing — live input drives the emulator.
    #[default]
    Idle,
    /// Recording: every produced frame's input is captured.
    Recording,
    /// Playing back a loaded movie: its recorded input overrides live
    /// keyboard / gamepad input until end-of-movie or stop.
    Playing,
}

/// A snapshot of the movie state for the debugger overlay's status line.
#[derive(Clone, Copy, Debug, Default)]
pub struct MovieStatus {
    /// Current mode.
    pub mode: MovieMode,
    /// Frames recorded so far (recording) or played so far (playing).
    pub cursor: usize,
    /// Total frames in the loaded movie (playing); 0 otherwise.
    pub total: usize,
}

/// Frontend movie state: at most one of recorder / playback is active at a
/// time (toggling one stops the other).
#[derive(Default)]
pub struct MovieUi {
    /// Active recorder, present iff [`MovieMode::Recording`].
    recorder: Option<MovieRecorder>,
    /// Loaded movie being played, present iff [`MovieMode::Playing`].
    ///
    /// We store the owned [`Movie`] plus a cursor rather than a
    /// `rustynes_core::MoviePlayer` because the player borrows the movie (a
    /// self-referential field would need `Pin`/unsafe). Applying the
    /// current frame inline — reading `movie.frames[cursor]` and calling
    /// `set_buttons` exactly as `MoviePlayer::apply_next` does — is
    /// equivalent and keeps the playback state owned + `Send`.
    playback: Option<Playback>,
}

/// Owned movie + playback cursor.
struct Playback {
    movie: Movie,
    cursor: usize,
    /// v2.9.8 — the player's options as they were before the movie's were
    /// applied, put back by [`MovieUi::stop_playback`].
    player_options: HardwareOptions,
}

impl MovieUi {
    /// Current mode.
    #[must_use]
    pub const fn mode(&self) -> MovieMode {
        if self.recorder.is_some() {
            MovieMode::Recording
        } else if self.playback.is_some() {
            MovieMode::Playing
        } else {
            MovieMode::Idle
        }
    }

    /// A copyable status snapshot for the debugger overlay.
    #[must_use]
    pub fn status(&self) -> MovieStatus {
        match (&self.recorder, &self.playback) {
            (Some(rec), _) => MovieStatus {
                mode: MovieMode::Recording,
                cursor: rec.len(),
                total: 0,
            },
            (None, Some(pb)) => MovieStatus {
                mode: MovieMode::Playing,
                cursor: pb.cursor,
                total: pb.movie.len(),
            },
            (None, None) => MovieStatus::default(),
        }
    }

    /// `true` while a movie is being played back (live input is overridden).
    #[must_use]
    pub const fn is_playing(&self) -> bool {
        self.playback.is_some()
    }

    /// `true` while recording.
    #[must_use]
    pub const fn is_recording(&self) -> bool {
        self.recorder.is_some()
    }

    /// Start recording from `nes`'s fresh power-on. Power-cycles `nes` and
    /// clears its cartridge RAM (v2.9.0, `rustynes_core::power_on_for_movie`)
    /// so the recording starts from the exact state a replay reconstructs.
    /// Stops any in-progress playback. No-op if already recording.
    ///
    /// v2.3.2 "Lucid": `attest` arms a replay attestation, so the saved `.rnm`
    /// carries a rolling hash of its video output that anyone can re-derive with
    /// `rustynes verify`. Callers should pass `false` while run-ahead is active
    /// — see [`Self::after_frame`] for why.
    pub fn start_recording_power_on(&mut self, nes: &mut Nes, attest: bool) {
        if self.recorder.is_some() {
            return;
        }
        // v2.9.8 — a power-on recording is the player's own run: put their
        // options back first if a movie was playing.
        self.stop_playback(Some(nes));
        // v2.9.7 rule: a recording runs at stock timing. Set before the
        // options are captured so the movie records what the frames run with.
        nes.set_extra_scanlines(0);
        // v2.9.0 — a power cycle AND cleared cartridge RAM, the state playback
        // reconstructs (`Movie::seek_to_start`); see `power_on_for_movie`.
        rustynes_core::power_on_for_movie(nes);
        let mut rec = MovieRecorder::power_on(nes);
        if attest {
            rec.enable_attestation();
        }
        self.recorder = Some(rec);
    }

    /// Start recording a *branch* from `nes`'s current state (embeds a
    /// save-state start point). Stops any in-progress playback. Used both
    /// by the dedicated branch gesture and when the user starts recording
    /// mid-game without wanting a power-on reset.
    ///
    /// v2.9.8: a branch continues the machine as it is, so a branch taken
    /// from a playing movie keeps (and records) the MOVIE's options rather
    /// than restoring the player's; the player's return at the next ROM load
    /// or power cycle, when the app re-applies its configuration.
    pub fn start_recording_branch(&mut self, nes: &mut Nes, attest: bool) {
        self.playback = None;
        nes.set_extra_scanlines(0);
        let mut rec = MovieRecorder::from_current_state(nes);
        if attest {
            rec.enable_attestation();
        }
        self.recorder = Some(rec);
    }

    /// Finish recording and return the completed [`Movie`] for the caller
    /// to serialize + save. Returns `None` if not recording.
    pub fn finish_recording(&mut self) -> Option<Movie> {
        self.recorder.take().map(MovieRecorder::finish)
    }

    /// v1.6.0 B1 — clone the movie currently being played back (for export to an
    /// external `.fm2` / `.bk2`). Returns `None` if not playing.
    #[must_use]
    pub fn playing_movie(&self) -> Option<Movie> {
        self.playback.as_ref().map(|pb| pb.movie.clone())
    }

    /// Begin playing `movie`: capture the player's options, move `nes` to
    /// the movie's start point under the movie's options
    /// ([`Movie::seek_to_start`]), and drive input from the recording.
    /// Stops any in-progress recording (and any earlier playback, whose
    /// player options are the ones kept).
    ///
    /// # Errors
    ///
    /// Whatever [`Movie::seek_to_start`] refuses (another ROM, region or
    /// header, a bad start state). `nes` and the movie state are then left
    /// exactly as they were.
    pub fn start_playback(&mut self, nes: &mut Nes, movie: Movie) -> Result<(), MovieError> {
        // A movie already playing has the player's options saved; keep those,
        // not the first movie's.
        let player_options = self.playback.as_ref().map_or_else(
            || HardwareOptions::capture(nes),
            |pb| pb.player_options.clone(),
        );
        movie.seek_to_start(nes)?;
        self.recorder = None;
        self.playback = Some(Playback {
            movie,
            cursor: 0,
            player_options,
        });
        Ok(())
    }

    /// v2.9.8 — the overclock the current movie session runs with: the
    /// playing movie's recorded value, stock timing (0) while recording, or
    /// `None` when idle (the player's own setting applies).
    #[must_use]
    pub fn session_extra_scanlines(&self) -> Option<u16> {
        if let Some(rec) = self.recorder.as_ref() {
            return Some(rec.options().extra_scanlines);
        }
        self.playback
            .as_ref()
            .map(|pb| pb.movie.options.extra_scanlines)
    }

    /// v2.9.8 — the options the current movie session holds the console to:
    /// the recording's (captured at its start) or the playing movie's, `None`
    /// when idle. The app's Power Cycle re-applies its own configuration to
    /// the cold-booted console and then these, so a movie's options -- its
    /// power-on fills included -- still win across a power cycle, as
    /// [`Self::before_frame`] makes them win on every frame.
    #[must_use]
    pub fn held_options(&self) -> Option<&HardwareOptions> {
        if let Some(rec) = self.recorder.as_ref() {
            return Some(rec.options());
        }
        self.playback.as_ref().map(|pb| &pb.movie.options)
    }

    /// v2.3.2 "Lucid" — drop the in-progress attestation, keeping the recording.
    ///
    /// Called when something rewinds the emulator underneath the recorder: a
    /// successful `rewind_step_back` restores an EARLIER state while the input
    /// log keeps its full prefix, so the frames already folded into the hash no
    /// longer describe the run the input stream encodes. The frame counts stay
    /// self-consistent, so nothing downstream would notice — `Movie::verify`
    /// would simply report `Mismatch` on an honest recording.
    ///
    /// Dropping the attestation makes that outcome "not attested" instead of
    /// "failed verification", which is the truthful one. The recording itself is
    /// unaffected. (Review catch on PR #356.)
    pub fn invalidate_attestation(&mut self) {
        if let Some(rec) = self.recorder.as_mut() {
            rec.disable_attestation();
        }
    }

    /// v2.3.2 "Lucid" — per-frame hook called AFTER `run_frame`, feeding the
    /// completed frame's video output into the attestation.
    ///
    /// A no-op unless recording with attestation armed.
    ///
    /// # Run-ahead
    ///
    /// The caller must pass the **persistent-timeline** framebuffer and must not
    /// call this for a run-ahead frame. Run-ahead presents the frame N ahead of
    /// the persistent timeline; a verification replay has no run-ahead and
    /// produces persistent frames, so attesting the presented image would record
    /// a hash that can never be reproduced.
    ///
    /// Skipping run-ahead frames leaves the attestation's frame count short of
    /// the input stream's, which `Movie::deserialize` detects and drops the tail
    /// for. That is deliberate: the failure mode is "no attestation", never "a
    /// wrong one".
    pub fn after_frame(&mut self, framebuffer: &[u8]) {
        if let Some(rec) = self.recorder.as_mut() {
            rec.attest_frame(framebuffer);
        }
    }

    /// Stop playback (control returns to live input) and put the player's
    /// options back on `nes` (v2.9.8). No-op if not playing.
    ///
    /// `nes` is `None` only where the console is already gone (a ROM swap
    /// replaced it); the next machine is built from the player's settings
    /// anyway.
    pub fn stop_playback(&mut self, nes: Option<&mut Nes>) {
        if let Some(pb) = self.playback.take()
            && let Some(nes) = nes
        {
            // The player's own codes were valid when captured from this
            // machine, so this cannot fail.
            let restored = pb.player_options.restore_after_playback(nes);
            debug_assert!(restored.is_ok(), "player options re-apply");
        }
    }

    /// v1.5.0 "Lens" Workstream C2 — deterministically seek the active playback
    /// to `target` (clamped to the movie length). Re-derives state from the
    /// movie's start point and fast-forwards by replaying the recorded inputs
    /// frame-by-frame, exactly as normal playback does — so the post-seek state
    /// is bit-identical to having played up to that frame. No new determinism
    /// surface: it drives the SAME `seek_to_start` + `set_buttons` + `run_frame`
    /// the live replay path uses. No-op (returns `false`) if not playing or the
    /// ROM mismatches.
    ///
    /// Seeking is O(target) frames of emulation; the caller should run it under
    /// the emu lock (off the UI thread budget) and restart its frame clock.
    pub fn seek_playback(&mut self, nes: &mut Nes, target: usize) -> bool {
        let Some(pb) = self.playback.as_mut() else {
            return false;
        };
        let target = target.min(pb.movie.len());
        if pb.movie.seek_to_start(nes).is_err() {
            return false;
        }
        for i in 0..target {
            let Some(input) = pb.movie.frames.get(i).copied() else {
                break;
            };
            input.apply_to(nes);
            nes.run_frame();
        }
        pb.cursor = target;
        true
    }

    /// Per-frame hook, called from `App::produce_one_frame` AFTER the
    /// frontend's live `set_buttons` and BEFORE `run_frame`.
    ///
    /// - **Recording**: captures the inputs currently held on `nes` (the
    ///   live ones the frontend just latched).
    /// - **Playing**: overrides the live input with the movie's recorded
    ///   input for this frame. Returns `false` when the movie is exhausted
    ///   so the caller can stop playback and hand control back to live
    ///   input; in every other case returns `true`.
    ///
    /// Returns `true` for the idle and recording paths.
    pub fn before_frame(&mut self, nes: &mut Nes) -> bool {
        if let Some(rec) = self.recorder.as_mut() {
            // v2.9.8 — hold the recording's options in place; the movie
            // records them once, at the start.
            let held = rec.options().apply_live(nes);
            debug_assert!(held.is_ok(), "captured options re-apply");
            rec.capture(nes);
            return true;
        }
        if let Some(pb) = self.playback.as_mut() {
            // Apply this frame's recorded input, mirroring
            // `MoviePlayer::apply_next` but against our owned movie +
            // cursor: read the frame at the cursor and drive `set_buttons`,
            // then advance. At end-of-movie return `false` (without
            // applying anything) so the caller stops playback.
            let Some(input) = pb.movie.frames.get(pb.cursor).copied() else {
                return false;
            };
            // v2.9.8 — hold the movie's options in place against anything
            // the app pushed since the last frame (a Settings change, the
            // cheat panel's per-frame resync, a game-database edit). Codes
            // were validated when the movie was parsed.
            let held = pb.movie.options.apply_live(nes);
            debug_assert!(held.is_ok(), "movie options re-apply");
            input.apply_to(nes);
            pb.cursor += 1;
            return true;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_core::{Buttons, VerifyOutcome};

    // A minimal NROM (infinite loop) so we can exercise the record/play
    // state machine end-to-end without a real game. Mirrors the core's
    // `synth_nrom` test fixture.
    fn synth_nrom() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"NES\x1A");
        bytes.push(1);
        bytes.push(1);
        bytes.push(0);
        bytes.push(0);
        bytes.extend_from_slice(&[0u8; 8]);
        let mut prg = vec![0u8; 16 * 1024];
        prg[0] = 0x4C;
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

    #[test]
    fn idle_by_default() {
        let ui = MovieUi::default();
        assert_eq!(ui.mode(), MovieMode::Idle);
        assert!(!ui.is_playing());
        assert!(!ui.is_recording());
    }

    /// v2.9.0 — recording a power-on movie starts from cleared battery RAM,
    /// the state `Movie::seek_to_start` reconstructs on playback. Before, a
    /// loaded `.sav` was recorded against and then missing on another machine
    /// (or present on playback when it was absent at record time).
    #[test]
    fn recording_from_power_on_clears_battery_ram() {
        let mut rom = synth_nrom();
        rom[6] |= 0x02; // battery-backed PRG-RAM
        let mut nes = Nes::from_rom(&rom).unwrap();
        nes.sram_mut().fill(0xC3); // a loaded .sav
        let mut ui = MovieUi::default();
        ui.start_recording_power_on(&mut nes, false);
        assert!(nes.sram().iter().all(|&b| b == 0));
    }

    #[test]
    fn record_then_finish_yields_movie() {
        let mut nes = Nes::from_rom(&synth_nrom()).unwrap();
        let mut ui = MovieUi::default();
        ui.start_recording_power_on(&mut nes, true);
        assert_eq!(ui.mode(), MovieMode::Recording);
        for _ in 0..5 {
            assert!(ui.before_frame(&mut nes));
            let fb = nes.run_frame().to_vec();
            ui.after_frame(&fb);
        }
        assert_eq!(ui.status().cursor, 5);
        let movie = ui.finish_recording().expect("a movie");
        assert_eq!(movie.len(), 5);
        assert_eq!(ui.mode(), MovieMode::Idle);
    }

    /// v2.3.4 (issue #360) — a recorded movie must actually VERIFY.
    ///
    /// Every recording test above drives `before_frame` + `run_frame` and then
    /// asserts on frame counts. None of them proved the thing v2.3.2 shipped:
    /// that the movie carries an attestation which reproduces on replay. The
    /// recorder's attestation builder stayed empty in tests because
    /// `after_frame` — the only path that calls `attest_frame` — was never
    /// invoked, so `Movie::verify` would have returned `NotAttested` for every
    /// movie these tests built, and no assertion would have noticed.
    ///
    /// This asserts the end-to-end chain on a FRESH `Nes`: record with
    /// attestation, then replay from scratch and require an exact `Match`. It
    /// is the frontend-side analogue of `rustynes verify <movie> --rom <rom>`.
    #[test]
    fn a_recorded_movie_verifies_against_a_fresh_nes() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).unwrap();
        let mut ui = MovieUi::default();
        ui.start_recording_power_on(&mut nes, true);
        for i in 0..12u8 {
            // Vary the input per frame. The fixture ROM never reads the
            // controller, so this cannot change the VIDEO -- which is exactly
            // why it matters: v2.3.2's attestation folds in the input applied
            // as well as the frames produced, so an input log EDITED WITHOUT
            // RECOMPUTING the attestation cannot pass even on a ROM that ignores
            // the pad. To be precise about what that buys: the rolling FNV-1a
            // hash is tamper-EVIDENT, not forgery-resistant -- anyone willing to
            // recompute it can produce a consistent pair. It catches accidental
            // divergence and casual edits, which is what a replay attestation is
            // for; it is not a signature. A constant-input
            // recording cannot distinguish "the right input was attested" from
            // "some input was attested", so this test presses buttons.
            nes.set_buttons(0, Buttons::from_bits_truncate(1 << (i % 8)));
            nes.set_buttons(1, Buttons::from_bits_truncate(1 << ((i + 3) % 8)));
            assert!(ui.before_frame(&mut nes));
            let fb = nes.run_frame().to_vec();
            ui.after_frame(&fb);
        }
        let movie = ui.finish_recording().expect("a movie");

        // A fresh console, not the one that recorded it: verification must
        // re-derive the run rather than observe leftover state.
        let mut fresh = Nes::from_rom(&rom).unwrap();
        let outcome = movie
            .verify(&mut fresh)
            .expect("same ROM, valid start point");
        match outcome {
            VerifyOutcome::Match { frames, .. } => {
                assert_eq!(frames, 12, "every recorded frame is attested");
            }
            VerifyOutcome::NotAttested => {
                panic!("the movie carries no attestation — after_frame never reached the recorder")
            }
            VerifyOutcome::Mismatch { expected, got, .. } => {
                panic!("a faithfully-recorded movie must verify: expected {expected}, got {got}")
            }
        }
    }

    /// The negative control for the test above, and the reason the #360 gap was
    /// detectable at all.
    ///
    /// Recording enables attestation at `start_recording_power_on`, so the
    /// finished movie carries an `Attestation` whether or not any frame reached
    /// `attest_frame` — `MovieRecorder::finish` maps the *builder*, and the
    /// builder exists from the moment attestation is enabled. A recording that
    /// never calls `after_frame` therefore does not answer `NotAttested`; it
    /// claims the empty-state hash (the FNV-1a offset basis) over zero frames.
    ///
    /// Verification must then **fail**, which is the safe direction: forgetting
    /// to attest yields a movie that cannot be passed off as verified, rather
    /// than one that silently verifies against nothing. This pins that, so a
    /// future change that made an empty attestation compare equal — or made
    /// `verify` fall back to `NotAttested` when the count is zero — would be
    /// caught here rather than in the field.
    #[test]
    fn a_recording_that_never_attests_fails_verification() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).unwrap();
        let mut ui = MovieUi::default();
        ui.start_recording_power_on(&mut nes, true);
        for _ in 0..4 {
            assert!(ui.before_frame(&mut nes));
            nes.run_frame(); // deliberately NOT attested
        }
        let movie = ui.finish_recording().expect("a movie");

        let mut fresh = Nes::from_rom(&rom).unwrap();
        match movie.verify(&mut fresh).expect("same ROM") {
            VerifyOutcome::Mismatch { frames, .. } => {
                assert_eq!(frames, 4, "the replay still runs every recorded input");
            }
            VerifyOutcome::Match { .. } => {
                panic!("a movie that attested nothing must never verify as a match")
            }
            VerifyOutcome::NotAttested => panic!(
                "recording enables attestation, so the movie carries an (empty) claim -- \
                 NotAttested would mean the builder was never created at all"
            ),
        }
    }

    #[test]
    fn playback_overrides_and_stops_at_end() {
        let rom = synth_nrom();
        // Record a short movie first.
        let mut nes = Nes::from_rom(&rom).unwrap();
        let mut ui = MovieUi::default();
        ui.start_recording_power_on(&mut nes, true);
        for _ in 0..3 {
            ui.before_frame(&mut nes);
            let fb = nes.run_frame().to_vec();
            ui.after_frame(&fb);
        }
        let movie = ui.finish_recording().unwrap();

        // Replay it.
        let mut replay = Nes::from_rom(&rom).unwrap();
        ui.start_playback(&mut replay, movie).unwrap();
        assert_eq!(ui.mode(), MovieMode::Playing);
        assert_eq!(ui.status().total, 3);

        let mut produced = 0;
        for _ in 0..10 {
            if !ui.before_frame(&mut replay) {
                break;
            }
            replay.run_frame();
            produced += 1;
        }
        assert_eq!(produced, 3, "playback runs exactly the recorded frames");
        assert_eq!(ui.status().cursor, 3);
    }

    #[test]
    fn seek_is_bit_identical_to_linear_playback() {
        // Record a movie, then prove `seek_playback(N)` lands on exactly the
        // framebuffer + cycle a linear replay of N frames produces. This is the
        // C2 determinism guarantee: seek re-derives, it does not snapshot.
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).unwrap();
        let mut ui = MovieUi::default();
        ui.start_recording_power_on(&mut nes, true);
        for _ in 0..10 {
            ui.before_frame(&mut nes);
            let fb = nes.run_frame().to_vec();
            ui.after_frame(&fb);
        }
        let movie = ui.finish_recording().unwrap();

        // Linear replay to frame 7.
        let mut linear = Nes::from_rom(&rom).unwrap();
        movie.seek_to_start(&mut linear).unwrap();
        for i in 0..7 {
            let f = movie.frames[i];
            f.apply_to(&mut linear);
            linear.run_frame();
        }
        let linear_fb = linear.framebuffer().to_vec();
        let linear_cycle = linear.cycle();

        // Seek to frame 7 from a fresh playback.
        let mut seeked = Nes::from_rom(&rom).unwrap();
        ui.start_playback(&mut seeked, movie).unwrap();
        assert!(ui.seek_playback(&mut seeked, 7));
        assert_eq!(ui.status().cursor, 7);
        assert_eq!(seeked.framebuffer(), linear_fb.as_slice());
        assert_eq!(seeked.cycle(), linear_cycle);
    }

    #[test]
    fn seek_clamps_and_noops_when_idle() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).unwrap();
        let mut ui = MovieUi::default();
        // Idle: seek is a no-op.
        assert!(!ui.seek_playback(&mut nes, 5));
        ui.start_recording_power_on(&mut nes, true);
        for _ in 0..3 {
            ui.before_frame(&mut nes);
            let fb = nes.run_frame().to_vec();
            ui.after_frame(&fb);
        }
        let movie = ui.finish_recording().unwrap();
        let mut replay = Nes::from_rom(&rom).unwrap();
        ui.start_playback(&mut replay, movie).unwrap();
        // Seeking past the end clamps to the movie length.
        assert!(ui.seek_playback(&mut replay, 999));
        assert_eq!(ui.status().cursor, 3);
    }

    #[test]
    fn starting_record_stops_playback_and_vice_versa() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).unwrap();
        let mut ui = MovieUi::default();

        // Make a 2-frame movie to play.
        ui.start_recording_power_on(&mut nes, true);
        ui.before_frame(&mut nes);
        let fb = nes.run_frame().to_vec();
        ui.after_frame(&fb);
        ui.before_frame(&mut nes);
        let fb = nes.run_frame().to_vec();
        ui.after_frame(&fb);
        let movie = ui.finish_recording().unwrap();

        let mut replay = Nes::from_rom(&rom).unwrap();
        ui.start_playback(&mut replay, movie).unwrap();
        assert!(ui.is_playing());
        // Starting a recording must drop playback.
        ui.start_recording_branch(&mut replay, true);
        assert!(ui.is_recording());
        assert!(!ui.is_playing());
        // Starting playback again must drop the recorder.
        let m2 = ui.finish_recording().unwrap();
        let mut r2 = Nes::from_rom(&rom).unwrap();
        ui.start_playback(&mut r2, m2).unwrap();
        assert!(ui.is_playing());
        assert!(!ui.is_recording());
    }

    /// v2.9.8 — a movie's options govern its playback whatever the player
    /// has set, survive a mid-movie change of the player's settings, and give
    /// way to the player's own options when playback stops.
    #[test]
    fn playback_holds_the_movie_options_then_restores_the_players() {
        use rustynes_core::ConsoleModel;
        let rom = synth_nrom();
        let mut rec = Nes::from_rom(&rom).unwrap();
        rec.set_console_model(ConsoleModel::Famicom);
        rec.set_oam_decay(true);
        let mut ui = MovieUi::default();
        ui.start_recording_power_on(&mut rec, false);
        for _ in 0..3 {
            ui.before_frame(&mut rec);
            rec.run_frame();
        }
        let movie = ui.finish_recording().unwrap();
        assert_eq!(movie.options.console_model, ConsoleModel::Famicom);
        assert!(
            movie.options.oam_decay,
            "the power cycle no longer drops it"
        );

        // The player runs the NES model with OAM decay off.
        let mut player = Nes::from_rom(&rom).unwrap();
        ui.start_playback(&mut player, movie).unwrap();
        assert_eq!(player.console_model(), ConsoleModel::Famicom);
        // A Settings push mid-movie is overridden before the next frame.
        player.set_oam_decay(false);
        assert!(ui.before_frame(&mut player));
        assert!(player.oam_decay_enabled(), "the movie's option holds");
        player.run_frame();

        ui.stop_playback(Some(&mut player));
        assert_eq!(
            player.console_model(),
            ConsoleModel::Nes,
            "player's model back"
        );
        assert!(!player.oam_decay_enabled(), "player's OAM decay back");
    }
}

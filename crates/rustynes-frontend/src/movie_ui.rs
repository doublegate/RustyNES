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
//! - **wasm32**: the movie UI is gated off for v1.4.0 (browser file
//!   download / `IndexedDB` is a follow-up); see the documented TODO in
//!   `app.rs`. The build still compiles on wasm32 — this module is
//!   target-agnostic and holds no native-only types.

use rustynes_core::{Movie, MovieRecorder, Nes};

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

    /// Start recording from `nes`'s fresh power-on. Power-cycles `nes` so
    /// the recording starts from the exact state a replay reconstructs.
    /// Stops any in-progress playback. No-op if already recording.
    pub fn start_recording_power_on(&mut self, nes: &mut Nes) {
        if self.recorder.is_some() {
            return;
        }
        self.playback = None;
        nes.power_cycle();
        self.recorder = Some(MovieRecorder::power_on(nes));
    }

    /// Start recording a *branch* from `nes`'s current state (embeds a
    /// save-state start point). Stops any in-progress playback. Used both
    /// by the dedicated branch gesture and when the user starts recording
    /// mid-game without wanting a power-on reset.
    pub fn start_recording_branch(&mut self, nes: &Nes) {
        self.playback = None;
        self.recorder = Some(MovieRecorder::from_current_state(nes));
    }

    /// Finish recording and return the completed [`Movie`] for the caller
    /// to serialize + save. Returns `None` if not recording.
    pub fn finish_recording(&mut self) -> Option<Movie> {
        self.recorder.take().map(MovieRecorder::finish)
    }

    /// Begin playing `movie`. The caller must have already
    /// [`Movie::seek_to_start`]ed `nes` to the movie's start point. Stops
    /// any in-progress recording.
    pub fn start_playback(&mut self, movie: Movie) {
        self.recorder = None;
        self.playback = Some(Playback { movie, cursor: 0 });
    }

    /// Stop playback (control returns to live input). No-op if not playing.
    pub fn stop_playback(&mut self) {
        self.playback = None;
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
            nes.set_buttons(0, input.p1);
            nes.set_buttons(1, input.p2);
            pb.cursor += 1;
            return true;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn record_then_finish_yields_movie() {
        let mut nes = Nes::from_rom(&synth_nrom()).unwrap();
        let mut ui = MovieUi::default();
        ui.start_recording_power_on(&mut nes);
        assert_eq!(ui.mode(), MovieMode::Recording);
        for _ in 0..5 {
            assert!(ui.before_frame(&mut nes));
            nes.run_frame();
        }
        assert_eq!(ui.status().cursor, 5);
        let movie = ui.finish_recording().expect("a movie");
        assert_eq!(movie.len(), 5);
        assert_eq!(ui.mode(), MovieMode::Idle);
    }

    #[test]
    fn playback_overrides_and_stops_at_end() {
        let rom = synth_nrom();
        // Record a short movie first.
        let mut nes = Nes::from_rom(&rom).unwrap();
        let mut ui = MovieUi::default();
        ui.start_recording_power_on(&mut nes);
        for _ in 0..3 {
            ui.before_frame(&mut nes);
            nes.run_frame();
        }
        let movie = ui.finish_recording().unwrap();

        // Replay it.
        let mut replay = Nes::from_rom(&rom).unwrap();
        movie.seek_to_start(&mut replay).unwrap();
        ui.start_playback(movie);
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
    fn starting_record_stops_playback_and_vice_versa() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).unwrap();
        let mut ui = MovieUi::default();

        // Make a 2-frame movie to play.
        ui.start_recording_power_on(&mut nes);
        ui.before_frame(&mut nes);
        nes.run_frame();
        ui.before_frame(&mut nes);
        nes.run_frame();
        let movie = ui.finish_recording().unwrap();

        let mut replay = Nes::from_rom(&rom).unwrap();
        movie.seek_to_start(&mut replay).unwrap();
        ui.start_playback(movie);
        assert!(ui.is_playing());
        // Starting a recording must drop playback.
        ui.start_recording_branch(&replay);
        assert!(ui.is_recording());
        assert!(!ui.is_playing());
        // Starting playback again must drop the recorder.
        let m2 = ui.finish_recording().unwrap();
        let mut r2 = Nes::from_rom(&rom).unwrap();
        m2.seek_to_start(&mut r2).unwrap();
        ui.start_playback(m2);
        assert!(ui.is_playing());
        assert!(!ui.is_recording());
    }
}

//! v1.6.0 "Studio" Workstream G — A/V (video + synchronized audio) recording.
//!
//! A **read-only frontend tap** on the already-produced output: each produced
//! NES framebuffer (256x240 RGBA8 — the exact source the screenshot path reads
//! via [`rustynes_core::Nes::framebuffer`]) plus the audio samples drained for
//! that same frame (mono `f32`, from the lock-free audio ring's producer side)
//! are streamed to an external `ffmpeg` process, which muxes them into an
//! `.mp4` / `.mkv` container.
//!
//! ## Why this does NOT touch determinism
//!
//! The recorder NEVER advances the emulator, mutates the core, or alters the
//! per-frame framebuffer / audio production. It only *copies* what
//! [`crate::emu::EmuCore::produce_one_frame`] has already produced — the same
//! data the renderer presents and the audio sink consumes. So the determinism
//! contract (same seed + ROM + input ⇒ bit-identical framebuffer + audio) is
//! unaffected, `AccuracyCoin` stays 139/139, and with the `av-record` feature off
//! (the default) this module is not compiled at all — the shipped / wasm /
//! `no_std` builds are byte-identical.
//!
//! ## Encoder approach
//!
//! We spawn `ffmpeg` and pipe two raw streams over a single stdin pipe is not
//! possible (one pipe = one stream), so we hand `ffmpeg` the audio as a
//! **separate named input** while video flows over stdin. To keep the
//! implementation simple, robust, and dependency-free we instead:
//!
//! * write **rawvideo** (`rgba`, 256x240, at the region frame rate) to
//!   `ffmpeg` over **stdin** (input 0), and
//! * write the corresponding **mono `f32le`** PCM to a small temp file that
//!   `ffmpeg` reads as input 1.
//!
//! Buffering the audio to a temp sidecar (rather than a second pipe) avoids the
//! classic two-pipe deadlock (ffmpeg blocking on one stream while we block
//! writing the other) without threads. The sidecar is muxed at `stop()` and
//! deleted. If `ffmpeg` is **absent** the recorder degrades gracefully: arming
//! fails with a clear [`AvError::FfmpegMissing`] and emulation continues
//! untouched.
//!
//! Choosing an external `ffmpeg` (over a vendored pure-Rust encoder) keeps the
//! default build free of heavy media codecs; the feature is additive +
//! off-by-default and native-only.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::gfx::{NES_H, NES_W};

/// Bytes per produced framebuffer (RGBA8, 256x240).
const FRAME_BYTES: usize = (NES_W as usize) * (NES_H as usize) * 4;

/// Errors raised while arming / driving the recorder.
#[derive(Debug)]
#[non_exhaustive]
pub enum AvError {
    /// `ffmpeg` is not on `PATH` (or failed to spawn). Recording is unavailable;
    /// emulation is unaffected.
    FfmpegMissing(io::Error),
    /// The audio sidecar temp file could not be created / written.
    Sidecar(io::Error),
    /// `ffmpeg` exited non-zero during the final mux, or the spawn pipe broke.
    Encode(String),
}

impl core::fmt::Display for AvError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::FfmpegMissing(e) => {
                write!(f, "ffmpeg not found (A/V recording unavailable): {e}")
            }
            Self::Sidecar(e) => write!(f, "A/V audio sidecar I/O failed: {e}"),
            Self::Encode(e) => write!(f, "ffmpeg encode failed: {e}"),
        }
    }
}

impl std::error::Error for AvError {}

/// The output container, inferred from the chosen file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// `.mp4` (H.264 video + AAC audio).
    Mp4,
    /// `.mkv` (Matroska; H.264 + AAC, the same codecs in a freer container).
    Mkv,
}

impl Container {
    /// Infer the container from a path's extension (case-insensitive), defaulting
    /// to [`Container::Mp4`] for anything unrecognized.
    #[must_use]
    pub fn from_path(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("mkv") => Self::Mkv,
            _ => Self::Mp4,
        }
    }
}

/// Parameters fixed when a recording is armed (constant for its lifetime).
#[derive(Debug, Clone)]
pub struct AvParams {
    /// Output container path (e.g. `<data_dir>/recordings/<rom>-<utc>.mp4`).
    pub out_path: PathBuf,
    /// Audio sample rate (Hz) — the device rate, matching the drained samples.
    pub sample_rate: u32,
    /// Video frame rate as an exact rational (`num/den`) so NTSC's
    /// 60.0988 fps / PAL's 50.007 fps stay drift-free across long recordings.
    pub fps_num: u32,
    /// Video frame rate denominator.
    pub fps_den: u32,
}

/// Build the `ffmpeg` argument vector for the given parameters.
///
/// Pure + side-effect-free so it can be unit-tested without spawning anything.
/// Input 0 is rawvideo over stdin (`pipe:0`); input 1 is the mono `f32le` audio
/// sidecar. Output is H.264 + AAC into the chosen container.
#[must_use]
pub fn ffmpeg_args(params: &AvParams, audio_sidecar: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec![
        // Overwrite the output without prompting.
        "-y".into(),
        // ---- input 0: rawvideo over stdin ----
        "-f".into(),
        "rawvideo".into(),
        "-pixel_format".into(),
        "rgba".into(),
        "-video_size".into(),
        format!("{NES_W}x{NES_H}"),
        "-framerate".into(),
        format!("{}/{}", params.fps_num, params.fps_den),
        "-i".into(),
        "pipe:0".into(),
        // ---- input 1: mono f32le PCM sidecar ----
        "-f".into(),
        "f32le".into(),
        "-ar".into(),
        params.sample_rate.to_string(),
        "-ac".into(),
        "1".into(),
        "-i".into(),
        audio_sidecar.to_string_lossy().into_owned(),
        // ---- encode ----
        "-c:v".into(),
        "libx264".into(),
        // The NES frame is tiny; a fast preset keeps the encode off the
        // critical path and is plenty for a 256x240 source.
        "-preset".into(),
        "veryfast".into(),
        // yuv420p so the output plays everywhere (rgba -> yuv420p).
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-c:a".into(),
        "aac".into(),
        // Stop at the shorter stream so a slightly-uneven A/V tail doesn't pad.
        "-shortest".into(),
    ];
    args.push(params.out_path.to_string_lossy().into_owned());
    args
}

/// An active A/V recording session.
///
/// Holds an armed `ffmpeg` child (video over stdin) plus an open audio sidecar
/// file. Dropping without [`AvRecorder::stop`] aborts the encode (the partial
/// output is left for the OS / user to clean up).
pub struct AvRecorder {
    params: AvParams,
    child: Child,
    /// Buffered writer for the mono-`f32le` audio sidecar (input 1). Taken
    /// (closed) in [`AvRecorder::stop`] before `ffmpeg` is waited on, so the
    /// muxer sees a fully-flushed file.
    audio_sidecar: Option<io::BufWriter<std::fs::File>>,
    audio_sidecar_path: PathBuf,
    /// Frames written so far (informational / status reporting).
    frames: u64,
    /// Audio samples written so far (informational).
    samples: u64,
}

impl AvRecorder {
    /// Arm a recording: create the audio sidecar and spawn `ffmpeg`. Returns
    /// [`AvError::FfmpegMissing`] (recording unavailable) if `ffmpeg` is not
    /// installed — the caller should surface a toast and carry on.
    ///
    /// # Errors
    /// Fails if the sidecar temp file cannot be created or `ffmpeg` cannot be
    /// spawned.
    pub fn start(params: AvParams) -> Result<Self, AvError> {
        // Audio sidecar lives next to the output so it shares its filesystem
        // (cheap rename/cleanup) and is unique per recording.
        let audio_sidecar_path = sidecar_path(&params.out_path);
        let sidecar_file = std::fs::File::create(&audio_sidecar_path).map_err(AvError::Sidecar)?;
        let audio_sidecar = io::BufWriter::new(sidecar_file);

        let args = ffmpeg_args(&params, &audio_sidecar_path);
        let child = Command::new("ffmpeg")
            .args(&args)
            .stdin(Stdio::piped())
            // ffmpeg is chatty on stderr; silence it (errors surface via the
            // exit status at stop()).
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                // Clean up the just-created sidecar so a failed arm leaves no trash.
                let _ = std::fs::remove_file(&audio_sidecar_path);
                AvError::FfmpegMissing(e)
            })?;

        Ok(Self {
            params,
            child,
            audio_sidecar: Some(audio_sidecar),
            audio_sidecar_path,
            frames: 0,
            samples: 0,
        })
    }

    /// Append one produced video frame (RGBA8, 256x240) to the video pipe.
    ///
    /// A short / mis-sized framebuffer is ignored (logged once by the caller);
    /// a broken pipe (ffmpeg died) returns an error so the caller can stop.
    ///
    /// # Errors
    /// Returns [`AvError::Encode`] if the ffmpeg stdin pipe is broken.
    pub fn push_video(&mut self, framebuffer: &[u8]) -> Result<(), AvError> {
        if framebuffer.len() != FRAME_BYTES {
            // Defensive: never feed ffmpeg a frame of the wrong stride.
            return Ok(());
        }
        let Some(stdin) = self.child.stdin.as_mut() else {
            return Err(AvError::Encode("ffmpeg stdin closed".into()));
        };
        stdin
            .write_all(framebuffer)
            .map_err(|e| AvError::Encode(format!("video pipe write failed: {e}")))?;
        self.frames += 1;
        Ok(())
    }

    /// Append this frame's audio samples (mono `f32`) to the sidecar.
    ///
    /// # Errors
    /// Returns [`AvError::Sidecar`] on a sidecar write failure.
    pub fn push_audio(&mut self, samples: &[f32]) -> Result<(), AvError> {
        // f32le: little-endian IEEE-754, exactly what ffmpeg's `f32le` expects.
        let Some(sidecar) = self.audio_sidecar.as_mut() else {
            return Err(AvError::Sidecar(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "audio sidecar already closed",
            )));
        };
        for &s in samples {
            sidecar
                .write_all(&s.to_le_bytes())
                .map_err(AvError::Sidecar)?;
        }
        self.samples += samples.len() as u64;
        Ok(())
    }

    /// Frames written so far.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// The output path this session writes to.
    #[must_use]
    pub fn out_path(&self) -> &Path {
        &self.params.out_path
    }

    /// Finalize: flush + close the video pipe and the audio sidecar, wait for
    /// `ffmpeg` to mux, then delete the sidecar. Consumes `self`.
    ///
    /// # Errors
    /// Returns [`AvError::Encode`] if `ffmpeg` exits non-zero.
    pub fn stop(mut self) -> Result<PathBuf, AvError> {
        // Flush + close the audio sidecar first so ffmpeg sees a complete input
        // (taking the Option drops the BufWriter, closing the file handle).
        if let Some(mut sidecar) = self.audio_sidecar.take() {
            sidecar.flush().map_err(AvError::Sidecar)?;
        }
        // Close the video pipe (EOF) so ffmpeg stops reading input 0.
        drop(self.child.stdin.take());

        let status = self
            .child
            .wait()
            .map_err(|e| AvError::Encode(format!("ffmpeg wait failed: {e}")))?;

        // Best-effort sidecar cleanup regardless of the encode outcome.
        let _ = std::fs::remove_file(&self.audio_sidecar_path);

        if status.success() {
            Ok(self.params.out_path.clone())
        } else {
            Err(AvError::Encode(format!(
                "ffmpeg exited with {status} ({} frames, {} samples)",
                self.frames, self.samples
            )))
        }
    }
}

impl Drop for AvRecorder {
    fn drop(&mut self) {
        // If the session was dropped without stop() (e.g. ROM closed mid-record
        // or the app exiting), kill ffmpeg and remove the sidecar so we don't
        // leak a zombie or a stray temp file.
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.audio_sidecar_path);
    }
}

/// Derive the audio-sidecar path for an output path: `<out>.rustynes-avtmp`.
#[must_use]
fn sidecar_path(out_path: &Path) -> PathBuf {
    let mut p = out_path.as_os_str().to_os_string();
    p.push(".rustynes-avtmp");
    PathBuf::from(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> AvParams {
        AvParams {
            out_path: PathBuf::from("/tmp/out.mp4"),
            sample_rate: 48_000,
            fps_num: 60_098_814,
            fps_den: 1_000_000,
        }
    }

    #[test]
    fn frame_bytes_matches_nes_resolution() {
        assert_eq!(FRAME_BYTES, 256 * 240 * 4);
    }

    #[test]
    fn container_inference_is_case_insensitive() {
        assert_eq!(Container::from_path(Path::new("a.mp4")), Container::Mp4);
        assert_eq!(Container::from_path(Path::new("a.MP4")), Container::Mp4);
        assert_eq!(Container::from_path(Path::new("a.mkv")), Container::Mkv);
        assert_eq!(Container::from_path(Path::new("a.MKV")), Container::Mkv);
        // Unknown / missing extension defaults to mp4.
        assert_eq!(Container::from_path(Path::new("a.avi")), Container::Mp4);
        assert_eq!(Container::from_path(Path::new("noext")), Container::Mp4);
    }

    #[test]
    fn ffmpeg_args_describe_both_inputs_and_codecs() {
        let p = params();
        let sidecar = sidecar_path(&p.out_path);
        let args = ffmpeg_args(&p, &sidecar);

        // Two inputs: rawvideo over a pipe + the f32le sidecar.
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 2);
        assert!(args.iter().any(|a| a == "pipe:0"));
        assert!(args.iter().any(|a| a == "rawvideo"));
        assert!(args.iter().any(|a| a == "rgba"));
        assert!(args.iter().any(|a| a == "f32le"));
        // Frame size + exact rational frame rate are passed through verbatim.
        assert!(args.iter().any(|a| a == "256x240"));
        assert!(args.iter().any(|a| a == "60098814/1000000"));
        // Mono audio at the device rate.
        assert!(args.iter().any(|a| a == "48000"));
        let ac_idx = args.iter().position(|a| a == "-ac").unwrap();
        assert_eq!(args[ac_idx + 1], "1");
        // Codecs + the output path land last.
        assert!(args.iter().any(|a| a == "libx264"));
        assert!(args.iter().any(|a| a == "aac"));
        assert!(args.iter().any(|a| a == "-shortest"));
        assert_eq!(args.last().unwrap(), "/tmp/out.mp4");
    }

    #[test]
    fn ffmpeg_args_audio_input_is_the_sidecar() {
        let p = params();
        let sidecar = sidecar_path(&p.out_path);
        let args = ffmpeg_args(&p, &sidecar);
        // The second `-i` is followed by the sidecar path.
        let positions: Vec<usize> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "-i")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(positions.len(), 2);
        assert_eq!(args[positions[1] + 1], sidecar.to_string_lossy());
    }

    #[test]
    fn sidecar_path_is_derived_from_output() {
        let s = sidecar_path(Path::new("/x/y/rec.mp4"));
        assert_eq!(s, PathBuf::from("/x/y/rec.mp4.rustynes-avtmp"));
    }
}

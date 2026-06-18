//! v1.6.0 "Studio" Workstream G — A/V (video + synchronized audio) recording.
//!
//! A **read-only frontend tap** on the already-produced output: each produced
//! NES framebuffer (256x240 RGBA8 — the exact source the screenshot path reads
//! via [`rustynes_core::Nes::framebuffer`]) plus the audio samples drained for
//! that same frame (mono `f32`, from the lock-free audio ring's producer side)
//! are buffered to disk and, at [`AvRecorder::stop`], muxed by an external
//! `ffmpeg` process into an `.mp4` / `.mkv` container.
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
//! ## Encoder approach — mux at stop from two COMPLETE files
//!
//! The earlier design spawned `ffmpeg` at arm time with the still-empty audio
//! sidecar passed as a regular-file `-i` input. `ffmpeg` opens a regular-file
//! input and reads it to EOF eagerly at startup — so it saw an empty (or
//! truncated) audio file before any samples had been written, producing a
//! recording with broken / missing audio. (A regular file is not a stream
//! `ffmpeg` keeps polling; only pipes/fifos behave that way.)
//!
//! The robust, dependency-free fix is to make **both** inputs complete before
//! `ffmpeg` ever runs:
//!
//! * during recording, append **rawvideo** (`rgba`, 256x240) to a video temp
//!   file and the corresponding **mono `f32le`** PCM to an audio temp file —
//!   no child process is alive, so there is no two-pipe deadlock and no
//!   read-before-write race; and
//! * at [`AvRecorder::stop`], flush both files and spawn `ffmpeg` **once** with
//!   `-i video.raw -i audio.raw`, muxing the two fully-written inputs into the
//!   output container, then delete both temps.
//!
//! `ffmpeg` availability is still probed at [`AvRecorder::start`] (a cheap
//! `ffmpeg -version` spawn) so arming fails fast and gracefully with
//! [`AvError::FfmpegMissing`] when `ffmpeg` is absent — emulation continues
//! untouched and the recorder is never armed.
//!
//! Choosing an external `ffmpeg` (over a vendored pure-Rust encoder) keeps the
//! default build free of heavy media codecs; the feature is additive +
//! off-by-default and native-only.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
    /// A temp capture file (video or audio) could not be created / written.
    Sidecar(io::Error),
    /// `ffmpeg` exited non-zero during the final mux, or it could not be spawned
    /// at stop time.
    Encode(String),
}

impl core::fmt::Display for AvError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::FfmpegMissing(e) => {
                write!(f, "ffmpeg not found (A/V recording unavailable): {e}")
            }
            Self::Sidecar(e) => write!(f, "A/V capture temp I/O failed: {e}"),
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

/// Build the `ffmpeg` argument vector for the final (stop-time) mux.
///
/// Pure + side-effect-free so it can be unit-tested without spawning anything.
/// Both inputs are **complete on-disk raw files** read at mux time: input 0 is
/// the rawvideo (`rgba`, 256x240, at the region frame rate); input 1 is the
/// mono `f32le` PCM. Output is H.264 + AAC into the chosen container.
#[must_use]
pub fn ffmpeg_args(params: &AvParams, video_raw: &Path, audio_raw: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec![
        // Overwrite the output without prompting.
        "-y".into(),
        // ---- input 0: rawvideo from the completed video temp file ----
        "-f".into(),
        "rawvideo".into(),
        "-pixel_format".into(),
        "rgba".into(),
        "-video_size".into(),
        format!("{NES_W}x{NES_H}"),
        "-framerate".into(),
        format!("{}/{}", params.fps_num, params.fps_den),
        "-i".into(),
        video_raw.to_string_lossy().into_owned(),
        // ---- input 1: mono f32le PCM from the completed audio temp file ----
        "-f".into(),
        "f32le".into(),
        "-ar".into(),
        params.sample_rate.to_string(),
        "-ac".into(),
        "1".into(),
        "-i".into(),
        audio_raw.to_string_lossy().into_owned(),
        // ---- encode ----
        "-c:v".into(),
        "libx264".into(),
        // The NES frame is tiny; a fast preset keeps the encode quick and is
        // plenty for a 256x240 source.
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
/// Buffers rawvideo + mono-`f32le` audio to two temp files while recording; no
/// child process is alive until [`AvRecorder::stop`] muxes the two completed
/// files with a single `ffmpeg` invocation. `ffmpeg` presence is verified at
/// [`AvRecorder::start`]. Dropping without [`AvRecorder::stop`] removes both
/// temp files (no encode is produced).
pub struct AvRecorder {
    params: AvParams,
    /// Buffered writer for the rawvideo capture (input 0). Taken (closed) in
    /// [`AvRecorder::stop`] before `ffmpeg` runs, so the muxer sees a fully
    /// flushed file.
    video: Option<io::BufWriter<std::fs::File>>,
    video_path: PathBuf,
    /// Buffered writer for the mono-`f32le` audio capture (input 1). Closed in
    /// [`AvRecorder::stop`] before `ffmpeg` runs.
    audio: Option<io::BufWriter<std::fs::File>>,
    audio_path: PathBuf,
    /// Frames written so far (informational / status reporting).
    frames: u64,
    /// Audio samples written so far (informational).
    samples: u64,
}

impl AvRecorder {
    /// Arm a recording: verify `ffmpeg` is available, then create the two temp
    /// capture files. Returns [`AvError::FfmpegMissing`] (recording unavailable)
    /// if `ffmpeg` is not installed — the caller should surface a toast and
    /// carry on.
    ///
    /// No `ffmpeg` child is spawned here; muxing happens once, at
    /// [`AvRecorder::stop`], from the completed temp files.
    ///
    /// # Errors
    /// Fails with [`AvError::FfmpegMissing`] if `ffmpeg` cannot be spawned, or
    /// [`AvError::Sidecar`] if a temp capture file cannot be created.
    pub fn start(params: AvParams) -> Result<Self, AvError> {
        // Probe ffmpeg up front so arming fails fast + gracefully when it is
        // absent (the actual mux runs at stop()). `-version` exits immediately.
        Self::probe_ffmpeg().map_err(AvError::FfmpegMissing)?;

        // Capture temps live next to the output so they share its filesystem
        // (cheap cleanup) and are unique per recording.
        let video_path = capture_path(&params.out_path, ".video.rustynes-avtmp");
        let audio_path = capture_path(&params.out_path, ".audio.rustynes-avtmp");

        let video_file = std::fs::File::create(&video_path).map_err(AvError::Sidecar)?;
        let audio_file = std::fs::File::create(&audio_path).map_err(|e| {
            // Don't leak the video temp if the audio temp create fails.
            let _ = std::fs::remove_file(&video_path);
            AvError::Sidecar(e)
        })?;

        Ok(Self {
            params,
            video: Some(io::BufWriter::new(video_file)),
            video_path,
            audio: Some(io::BufWriter::new(audio_file)),
            audio_path,
            frames: 0,
            samples: 0,
        })
    }

    /// Run `ffmpeg -version` to confirm the binary is on `PATH` and spawnable.
    fn probe_ffmpeg() -> Result<(), io::Error> {
        let status = Command::new("ffmpeg")
            .arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("ffmpeg -version exited {status}")))
        }
    }

    /// Append one produced video frame (RGBA8, 256x240) to the video temp file.
    ///
    /// A short / mis-sized framebuffer is ignored (logged once by the caller);
    /// a write failure returns an error so the caller can stop.
    ///
    /// # Errors
    /// Returns [`AvError::Sidecar`] if the video temp file write fails.
    pub fn push_video(&mut self, framebuffer: &[u8]) -> Result<(), AvError> {
        if framebuffer.len() != FRAME_BYTES {
            // Defensive: never feed ffmpeg a frame of the wrong stride.
            return Ok(());
        }
        let Some(video) = self.video.as_mut() else {
            return Err(AvError::Sidecar(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "video capture already closed",
            )));
        };
        video.write_all(framebuffer).map_err(AvError::Sidecar)?;
        self.frames += 1;
        Ok(())
    }

    /// Append this frame's audio samples (mono `f32`) to the audio temp file.
    ///
    /// # Errors
    /// Returns [`AvError::Sidecar`] on an audio temp write failure.
    pub fn push_audio(&mut self, samples: &[f32]) -> Result<(), AvError> {
        // f32le: little-endian IEEE-754, exactly what ffmpeg's `f32le` expects.
        let Some(audio) = self.audio.as_mut() else {
            return Err(AvError::Sidecar(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "audio capture already closed",
            )));
        };
        for &s in samples {
            audio
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

    /// Finalize: flush + close both temp capture files, spawn `ffmpeg` once to
    /// mux the two COMPLETE files, wait for it, then delete the temps. Consumes
    /// `self`.
    ///
    /// # Errors
    /// Returns [`AvError::Sidecar`] on a final flush failure, or
    /// [`AvError::Encode`] if `ffmpeg` cannot be spawned or exits non-zero.
    pub fn stop(mut self) -> Result<PathBuf, AvError> {
        // Flush + close both inputs (taking the Option drops the BufWriter,
        // closing the file handle) so ffmpeg reads two fully-written files.
        if let Some(mut video) = self.video.take() {
            video.flush().map_err(AvError::Sidecar)?;
        }
        if let Some(mut audio) = self.audio.take() {
            audio.flush().map_err(AvError::Sidecar)?;
        }

        let args = ffmpeg_args(&self.params, &self.video_path, &self.audio_path);
        let result = Command::new("ffmpeg")
            .args(&args)
            .stdin(Stdio::null())
            // ffmpeg is chatty on stderr; silence it (errors surface via the
            // exit status).
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        // Best-effort temp cleanup regardless of the encode outcome.
        let _ = std::fs::remove_file(&self.video_path);
        let _ = std::fs::remove_file(&self.audio_path);

        match result {
            Ok(status) if status.success() => Ok(self.params.out_path.clone()),
            Ok(status) => Err(AvError::Encode(format!(
                "ffmpeg exited with {status} ({} frames, {} samples)",
                self.frames, self.samples
            ))),
            Err(e) => Err(AvError::Encode(format!("ffmpeg spawn failed: {e}"))),
        }
    }
}

impl Drop for AvRecorder {
    fn drop(&mut self) {
        // If the session was dropped without stop() (e.g. ROM closed mid-record
        // or the app exiting), drop the writers and remove both temp files so
        // we don't leak stray captures. No child process is alive to reap.
        self.video.take();
        self.audio.take();
        let _ = std::fs::remove_file(&self.video_path);
        let _ = std::fs::remove_file(&self.audio_path);
    }
}

/// Derive a capture-temp path for an output path: `<out><suffix>`.
#[must_use]
fn capture_path(out_path: &Path, suffix: &str) -> PathBuf {
    let mut p = out_path.as_os_str().to_os_string();
    p.push(suffix);
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
        let video = capture_path(&p.out_path, ".video.rustynes-avtmp");
        let audio = capture_path(&p.out_path, ".audio.rustynes-avtmp");
        let args = ffmpeg_args(&p, &video, &audio);

        // Two inputs: rawvideo file + the f32le audio file (both complete on
        // disk at mux time — neither is a pipe).
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 2);
        assert!(args.iter().any(|a| a == "rawvideo"));
        assert!(args.iter().any(|a| a == "rgba"));
        assert!(args.iter().any(|a| a == "f32le"));
        // No pipe input any more — both inputs are regular files.
        assert!(!args.iter().any(|a| a == "pipe:0"));
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
    fn ffmpeg_args_inputs_are_the_video_then_audio_files() {
        let p = params();
        let video = capture_path(&p.out_path, ".video.rustynes-avtmp");
        let audio = capture_path(&p.out_path, ".audio.rustynes-avtmp");
        let args = ffmpeg_args(&p, &video, &audio);
        // The first `-i` is the video file; the second `-i` is the audio file.
        let positions: Vec<usize> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "-i")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(positions.len(), 2);
        assert_eq!(args[positions[0] + 1], video.to_string_lossy());
        assert_eq!(args[positions[1] + 1], audio.to_string_lossy());
    }

    #[test]
    fn capture_paths_are_derived_from_output_and_distinct() {
        let out = Path::new("/x/y/rec.mp4");
        let v = capture_path(out, ".video.rustynes-avtmp");
        let a = capture_path(out, ".audio.rustynes-avtmp");
        assert_eq!(v, PathBuf::from("/x/y/rec.mp4.video.rustynes-avtmp"));
        assert_eq!(a, PathBuf::from("/x/y/rec.mp4.audio.rustynes-avtmp"));
        assert_ne!(v, a);
    }
}

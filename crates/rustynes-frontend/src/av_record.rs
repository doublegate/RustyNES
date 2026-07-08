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
//! unaffected, `AccuracyCoin` stays 139/141 (the two newest upstream PPU tests are
//! known gaps), and with the `av-record` feature off
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

/// Video encoder for an A/V recording — the ffmpeg `-c:v` encoder plus its
/// constant-quality model. All produce `yuv420p` so the output plays anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoCodec {
    /// H.264 (`libx264`) — universal playback. The default.
    #[default]
    H264,
    /// H.265 / HEVC (`libx265`) — ~30% smaller at equal quality, less universal.
    H265,
    /// VP9 (`libvpx-vp9`) — royalty-free, ideal for `.mkv` / the web.
    Vp9,
}

impl VideoCodec {
    /// The ffmpeg `-c:v` encoder name.
    #[must_use]
    pub const fn encoder(self) -> &'static str {
        match self {
            Self::H264 => "libx264",
            Self::H265 => "libx265",
            Self::Vp9 => "libvpx-vp9",
        }
    }

    /// Whether this encoder takes an x264 / x265-style `-preset` (VP9 uses
    /// `-deadline` / `-cpu-used` instead).
    #[must_use]
    pub const fn uses_x26x_preset(self) -> bool {
        matches!(self, Self::H264 | Self::H265)
    }

    /// All variants, for a UI picker.
    #[must_use]
    pub const fn all() -> [Self; 3] {
        [Self::H264, Self::H265, Self::Vp9]
    }

    /// Human label for a UI picker.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::H264 => "H.264 (universal)",
            Self::H265 => "H.265 / HEVC (smaller)",
            Self::Vp9 => "VP9 (royalty-free)",
        }
    }

    /// Stable lowercase id for config persistence.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::H265 => "h265",
            Self::Vp9 => "vp9",
        }
    }

    /// Parse a config id (falls back to the default H.264 on an unknown value).
    #[must_use]
    pub fn from_id(id: &str) -> Self {
        match id {
            "h265" => Self::H265,
            "vp9" => Self::Vp9,
            _ => Self::H264,
        }
    }
}

/// x264 / x265 speed-vs-compression preset (ignored by VP9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EncodePreset {
    /// Fastest, largest files.
    Ultrafast,
    /// Fast.
    Superfast,
    /// Quick — the default (the NES frame is tiny, so encode time is trivial).
    #[default]
    Veryfast,
    /// Balanced.
    Faster,
    /// ffmpeg's default tradeoff.
    Medium,
    /// Slower, smaller files.
    Slow,
}

impl EncodePreset {
    /// The ffmpeg `-preset` token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ultrafast => "ultrafast",
            Self::Superfast => "superfast",
            Self::Veryfast => "veryfast",
            Self::Faster => "faster",
            Self::Medium => "medium",
            Self::Slow => "slow",
        }
    }

    /// All variants, fast→slow, for a UI picker.
    #[must_use]
    pub const fn all() -> [Self; 6] {
        [
            Self::Ultrafast,
            Self::Superfast,
            Self::Veryfast,
            Self::Faster,
            Self::Medium,
            Self::Slow,
        ]
    }

    /// Parse a config token (the same string `as_str` emits; falls back to the
    /// default Veryfast on an unknown value).
    #[must_use]
    pub fn from_id(id: &str) -> Self {
        match id {
            "ultrafast" => Self::Ultrafast,
            "superfast" => Self::Superfast,
            "faster" => Self::Faster,
            "medium" => Self::Medium,
            "slow" => Self::Slow,
            _ => Self::Veryfast,
        }
    }
}

/// User-tunable A/V encode options — the codec-depth picker state.
///
/// Separate from the per-recording [`AvParams`] (which also carries the output
/// path + frame/sample rates). `Copy` so the app holds it inline and reads it
/// at arm time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AvRecordOptions {
    /// Video encoder.
    pub video_codec: VideoCodec,
    /// Constant-quality factor (CRF); see [`AvParams::crf`].
    pub crf: u8,
    /// x264 / x265 preset (ignored by VP9).
    pub preset: EncodePreset,
    /// AAC audio bitrate in kbit/s.
    pub audio_bitrate_k: u32,
}

impl Default for AvRecordOptions {
    fn default() -> Self {
        Self {
            video_codec: VideoCodec::H264,
            crf: 18,
            preset: EncodePreset::Veryfast,
            audio_bitrate_k: 192,
        }
    }
}

impl AvRecordOptions {
    /// Build from persisted config fields (the codec / preset id strings plus
    /// the numeric knobs), so the Settings picker round-trips through
    /// `config.toml`. Unknown ids fall back to the defaults; CRF is clamped to a
    /// sane 0..=51 (the ffmpeg ceiling for x264/x265).
    #[must_use]
    pub fn from_parts(codec_id: &str, crf: u8, preset_id: &str, audio_bitrate_k: u32) -> Self {
        let video_codec = VideoCodec::from_id(codec_id);
        // VP9's CRF ceiling is 63; x264/x265 cap at 51.
        let max_crf = if matches!(video_codec, VideoCodec::Vp9) {
            63
        } else {
            51
        };
        Self {
            video_codec,
            crf: crf.min(max_crf),
            preset: EncodePreset::from_id(preset_id),
            audio_bitrate_k: audio_bitrate_k.clamp(32, 512),
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
    /// Video encoder (default [`VideoCodec::H264`]).
    pub video_codec: VideoCodec,
    /// Constant-quality factor (CRF): lower = better quality + larger file.
    /// `0..=51` for x264 / x265, `0..=63` for VP9; ~18 is visually lossless on
    /// the tiny NES frame. Clamped into range when the args are built.
    pub crf: u8,
    /// x264 / x265 encode preset (ignored by VP9).
    pub preset: EncodePreset,
    /// AAC audio bitrate in kbit/s (e.g. 192).
    pub audio_bitrate_k: u32,
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
    ];
    // ---- encode ----
    // Video: the selected encoder + its constant-quality (CRF) control.
    args.push("-c:v".into());
    args.push(params.video_codec.encoder().into());
    if params.video_codec.uses_x26x_preset() {
        // The NES frame is tiny, so any preset is cheap; the default is fast.
        args.push("-preset".into());
        args.push(params.preset.as_str().into());
        // CRF range is 0..=51 for x264 / x265.
        args.push("-crf".into());
        args.push(u32::from(params.crf.min(51)).to_string());
    } else {
        // VP9 constant-quality: `-b:v 0` + a `-crf` in 0..=63, plus a moderate
        // speed/quality `-cpu-used` (VP9 ignores `-preset`).
        args.push("-b:v".into());
        args.push("0".into());
        args.push("-crf".into());
        args.push(u32::from(params.crf.min(63)).to_string());
        args.push("-deadline".into());
        args.push("good".into());
        args.push("-cpu-used".into());
        args.push("2".into());
    }
    // yuv420p so the output plays everywhere (rgba -> yuv420p).
    args.push("-pix_fmt".into());
    args.push("yuv420p".into());
    // Audio: AAC at the chosen bitrate.
    args.push("-c:a".into());
    args.push("aac".into());
    args.push("-b:a".into());
    args.push(format!("{}k", params.audio_bitrate_k.max(1)));
    // Stop at the shorter stream so a slightly-uneven A/V tail doesn't pad.
    args.push("-shortest".into());
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
            video_codec: VideoCodec::H264,
            crf: 18,
            preset: EncodePreset::Veryfast,
            audio_bitrate_k: 192,
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
    fn from_parts_parses_config_ids_and_clamps() {
        let o = AvRecordOptions::from_parts("vp9", 70, "slow", 1000);
        assert_eq!(o.video_codec, VideoCodec::Vp9);
        assert_eq!(o.preset, EncodePreset::Slow);
        assert_eq!(o.crf, 63); // clamped from 70 to the VP9 ceiling (63)
        assert_eq!(o.audio_bitrate_k, 512); // clamped from 1000
        // x264/x265 cap at 51.
        assert_eq!(AvRecordOptions::from_parts("h265", 70, "slow", 192).crf, 51);
        // Unknown ids fall back to the defaults.
        let d = AvRecordOptions::from_parts("???", 18, "???", 192);
        assert_eq!(d.video_codec, VideoCodec::H264);
        assert_eq!(d.preset, EncodePreset::Veryfast);
        // The persisted id round-trips through `id()` / `as_str()`.
        assert_eq!(VideoCodec::Vp9.id(), "vp9");
        assert_eq!(EncodePreset::Slow.as_str(), "slow");
    }

    #[test]
    fn ffmpeg_args_honor_codec_options() {
        let video = Path::new("/tmp/v.raw");
        let audio = Path::new("/tmp/a.pcm");

        // H.265 with explicit CRF / preset / audio bitrate.
        let mut p = params();
        p.video_codec = VideoCodec::H265;
        p.crf = 30;
        p.preset = EncodePreset::Slow;
        p.audio_bitrate_k = 256;
        let a = ffmpeg_args(&p, video, audio);
        assert!(a.iter().any(|x| x == "libx265"));
        let preset_idx = a.iter().position(|x| x == "-preset").unwrap();
        assert_eq!(a[preset_idx + 1], "slow");
        let crf_idx = a.iter().position(|x| x == "-crf").unwrap();
        assert_eq!(a[crf_idx + 1], "30");
        let ba_idx = a.iter().position(|x| x == "-b:a").unwrap();
        assert_eq!(a[ba_idx + 1], "256k");

        // VP9 uses `-b:v 0` + `-cpu-used`, ignores `-preset`, and clamps CRF to 63.
        let mut p = params();
        p.video_codec = VideoCodec::Vp9;
        p.crf = 99; // out of range -> clamped to 63
        let a = ffmpeg_args(&p, video, audio);
        assert!(a.iter().any(|x| x == "libvpx-vp9"));
        assert!(!a.iter().any(|x| x == "-preset"));
        let bv_idx = a.iter().position(|x| x == "-b:v").unwrap();
        assert_eq!(a[bv_idx + 1], "0");
        assert!(a.iter().any(|x| x == "-cpu-used"));
        let crf_idx = a.iter().position(|x| x == "-crf").unwrap();
        assert_eq!(a[crf_idx + 1], "63");
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

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
//! unaffected, `AccuracyCoin` stays 141/141, and with the `av-record` feature off
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
    /// `.gif` (v2.1.9 — animated GIF, video-only, palette-quantized + frame-
    /// decimated). No audio track (GIF carries none).
    Gif,
    /// `.wav` (v2.1.9 — standalone 16-bit PCM audio export, no video). A quick
    /// way to capture just a tune / sound effect losslessly.
    Wav,
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
            Some("gif") => Self::Gif,
            Some("wav") => Self::Wav,
            _ => Self::Mp4,
        }
    }

    /// Whether this container carries a video stream (GIF yes, WAV no).
    #[must_use]
    pub const fn has_video(self) -> bool {
        !matches!(self, Self::Wav)
    }

    /// Whether this container carries an audio stream (GIF has none).
    #[must_use]
    pub const fn has_audio(self) -> bool {
        matches!(self, Self::Mp4 | Self::Mkv | Self::Wav)
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
    /// A/V sync offset in milliseconds (v3.5.0, `FE-03`); see
    /// [`AvParams::audio_offset_ms`].
    pub audio_offset_ms: i32,
}

impl Default for AvRecordOptions {
    fn default() -> Self {
        Self {
            video_codec: VideoCodec::H264,
            crf: 18,
            preset: EncodePreset::Veryfast,
            audio_bitrate_k: 192,
            audio_offset_ms: 0,
        }
    }
}

/// The largest A/V sync offset, either way, in milliseconds (v3.5.0).
///
/// A second is far beyond any device or player latency worth correcting, and
/// a bound keeps a hand-edited config from trimming a whole recording's
/// audio away.
pub const MAX_AUDIO_OFFSET_MS: i32 = 1000;

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
            audio_offset_ms: 0,
        }
    }

    /// Set the A/V sync offset (v3.5.0), clamped to
    /// plus or minus [`MAX_AUDIO_OFFSET_MS`], as the other knobs are clamped in
    /// [`Self::from_parts`].
    #[must_use]
    pub fn with_audio_offset_ms(mut self, offset_ms: i32) -> Self {
        self.audio_offset_ms = offset_ms.clamp(-MAX_AUDIO_OFFSET_MS, MAX_AUDIO_OFFSET_MS);
        self
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
    /// A/V sync offset in milliseconds (v3.5.0, `FE-03`), applied to the
    /// audio track at mux time. Positive delays the audio against the video
    /// (the sound comes later); negative advances it (the first `|offset|`
    /// of audio is dropped, so the sound comes earlier). `0` is the prior
    /// pipeline exactly: no extra arguments.
    ///
    /// The capture itself is already in sync: each frame's audio is the
    /// audio the core produced for that frame. The offset is for the
    /// playback side, where a TV, a capture chain or an editor adds latency
    /// to one stream and not the other. Clamped to plus or minus
    /// [`MAX_AUDIO_OFFSET_MS`] when the arguments are built. The A/V containers only: a GIF has no
    /// audio and a WAV has no video to be in sync with.
    pub audio_offset_ms: i32,
}

impl AvParams {
    /// The parameters for one recording: where it goes, the stream rates, and
    /// the persisted encode options (already parsed and clamped by
    /// [`AvRecordOptions::from_parts`] / [`AvRecordOptions::with_audio_offset_ms`]).
    #[must_use]
    pub const fn new(
        out_path: PathBuf,
        sample_rate: u32,
        fps_num: u32,
        fps_den: u32,
        opts: AvRecordOptions,
    ) -> Self {
        Self {
            out_path,
            sample_rate,
            fps_num,
            fps_den,
            video_codec: opts.video_codec,
            crf: opts.crf,
            preset: opts.preset,
            audio_bitrate_k: opts.audio_bitrate_k,
            audio_offset_ms: opts.audio_offset_ms,
        }
    }
}

/// The `ffmpeg` arguments that shift the audio track by `offset_ms`
/// (v3.5.0, `FE-03`), as two halves: options that go before the audio
/// `-i` (input options), and options that go with the output.
///
/// * `offset_ms > 0` (audio later): the output filter `adelay=<ms>`, which
///   prepends that much silence to the (mono) audio.
/// * `offset_ms < 0` (audio earlier): the input option `-ss <seconds>` on the
///   audio input, which starts reading it that far in.
/// * `0`: nothing at all, so the argument vector is byte-identical to the
///   pre-v3.5.0 pipeline.
///
/// Both are sample-exact in the decoded PCM (the input is raw `f32le`, so
/// `-ss` seeks to a sample, not a packet). Pure, so the unit tests and the
/// ffmpeg round-trip test check the same strings the recorder runs.
fn audio_offset_args(offset_ms: i32) -> (Vec<String>, Vec<String>) {
    let ms = offset_ms.clamp(-MAX_AUDIO_OFFSET_MS, MAX_AUDIO_OFFSET_MS);
    match ms.cmp(&0) {
        core::cmp::Ordering::Greater => (Vec::new(), vec!["-af".into(), format!("adelay={ms}")]),
        core::cmp::Ordering::Less => {
            let abs = ms.unsigned_abs();
            (
                vec!["-ss".into(), format!("{}.{:03}", abs / 1000, abs % 1000)],
                Vec::new(),
            )
        }
        core::cmp::Ordering::Equal => (Vec::new(), Vec::new()),
    }
}

/// Build the `ffmpeg` argument vector for the final (stop-time) mux.
///
/// Pure + side-effect-free so it can be unit-tested without spawning anything.
/// Both inputs are **complete on-disk raw files** read at mux time: input 0 is
/// the rawvideo (`rgba`, 256x240, at the region frame rate); input 1 is the
/// mono `f32le` PCM. Output is H.264 + AAC into the chosen container.
#[must_use]
pub fn ffmpeg_args(params: &AvParams, video_raw: &Path, audio_raw: &Path) -> Vec<String> {
    let container = Container::from_path(&params.out_path);
    // v2.1.9 — GIF (video-only, palette-quantized) and WAV (audio-only) take
    // dedicated arg vectors; the A/V containers keep the two-input mux below.
    match container {
        Container::Gif => return gif_args(params, video_raw),
        Container::Wav => return wav_args(params, audio_raw),
        Container::Mp4 | Container::Mkv => {}
    }
    let (offset_in, offset_out) = audio_offset_args(params.audio_offset_ms);
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
    ];
    // A negative sync offset seeks into the audio input (an input option, so
    // it goes before that input's `-i`).
    args.extend(offset_in);
    args.push("-i".into());
    args.push(audio_raw.to_string_lossy().into_owned());
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
    // A positive sync offset delays the audio (an output filter).
    args.extend(offset_out);
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

/// The GIF output frame rate cap (v2.1.9). NES frames run at ~60 fps, but GIF
/// stores per-frame centiseconds so it can't represent 60 fps cleanly and the
/// file balloons; decimating to a smooth, GIF-friendly rate keeps size sane. A
/// factor-of-2-ish cut from 60 is the conventional choice for gameplay GIFs.
const GIF_FPS: u32 = 25;

/// Build the `ffmpeg` argument vector for a GIF export (video-only).
///
/// Reads the same completed rawvideo temp file the A/V mux uses, decimates to
/// [`GIF_FPS`], and runs the standard single-pass `palettegen` / `paletteuse`
/// filtergraph (an optimized per-clip 256-colour palette + Bayer-ordered dither)
/// so the NES's already-small palette reproduces crisply. Pure + testable.
#[must_use]
fn gif_args(params: &AvParams, video_raw: &Path) -> Vec<String> {
    vec![
        "-y".into(),
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
        // Decimate to a GIF-friendly rate, build a per-clip optimized palette,
        // then apply it with ordered dither. `split` feeds the same decimated
        // stream to both palettegen and paletteuse in a single pass.
        "-filter_complex".into(),
        format!(
            "fps={GIF_FPS},split[s0][s1];[s0]palettegen=stats_mode=diff[p];\
             [s1][p]paletteuse=dither=bayer:bayer_scale=3:diff_mode=rectangle"
        ),
        "-loop".into(),
        "0".into(),
        params.out_path.to_string_lossy().into_owned(),
    ]
}

/// Build the `ffmpeg` argument vector for a WAV export (audio-only).
///
/// Reads the completed mono-`f32le` PCM temp file and transcodes to canonical
/// 16-bit little-endian PCM WAV at the capture sample rate. No video input, no
/// re-timing — a lossless-enough, universally-openable audio capture. Pure +
/// testable.
#[must_use]
fn wav_args(params: &AvParams, audio_raw: &Path) -> Vec<String> {
    vec![
        "-y".into(),
        "-f".into(),
        "f32le".into(),
        "-ar".into(),
        params.sample_rate.to_string(),
        "-ac".into(),
        "1".into(),
        "-i".into(),
        audio_raw.to_string_lossy().into_owned(),
        "-c:a".into(),
        "pcm_s16le".into(),
        params.out_path.to_string_lossy().into_owned(),
    ]
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

        // ffmpeg writes to a STAGING sibling, never to the chosen path: with
        // `-y` it truncates its output first, so an encode that fails or is
        // killed part-way would otherwise destroy a recording the user already
        // had under that name. The staged file is renamed over the target only
        // once ffmpeg exits 0 (see `publish_output`), the same
        // write-then-rename rule `atomic_write` applies to every other user
        // file.
        let staged = staging_path(&self.params.out_path);
        let mut staged_params = self.params.clone();
        staged_params.out_path.clone_from(&staged);
        let args = ffmpeg_args(&staged_params, &self.video_path, &self.audio_path);
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

        let outcome = match result {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(AvError::Encode(format!(
                "ffmpeg exited with {status} ({} frames, {} samples)",
                self.frames, self.samples
            ))),
            Err(e) => Err(AvError::Encode(format!("ffmpeg spawn failed: {e}"))),
        };
        publish_output(&staged, &self.params.out_path, outcome)?;
        Ok(self.params.out_path.clone())
    }
}

/// The sibling path ffmpeg encodes into: `<stem>.rustynes-partial.<ext>`, in
/// the output's own directory. The extension is KEPT because ffmpeg picks the
/// muxer from it; the same directory is required because `rename` is atomic
/// only within one filesystem.
#[must_use]
fn staging_path(out_path: &Path) -> PathBuf {
    let stem = out_path
        .file_stem()
        .map_or_else(|| "recording".into(), std::ffi::OsStr::to_os_string);
    let mut name = stem;
    name.push(".rustynes-partial");
    if let Some(ext) = out_path.extension() {
        name.push(".");
        name.push(ext);
    }
    out_path.with_file_name(name)
}

/// Publish a staged encode: on success rename it over `out_path`; on a failed
/// encode delete it and leave `out_path` -- possibly a previous recording --
/// exactly as it was. A successful encode whose rename fails is kept.
fn publish_output(
    staged: &Path,
    out_path: &Path,
    outcome: Result<(), AvError>,
) -> Result<(), AvError> {
    match outcome {
        // The encode itself succeeded, so on a failed rename (a locked or
        // read-only destination) the finished file is KEPT and named in the
        // error: deleting it would throw away a complete recording over a
        // problem with the target path, not with the recording.
        Ok(()) => std::fs::rename(staged, out_path).map_err(|e| {
            AvError::Encode(format!(
                "the encode finished but could not replace {}: {e}; it is kept at {}",
                out_path.display(),
                staged.display()
            ))
        }),
        Err(e) => {
            discard_staged(staged);
            Err(e)
        }
    }
}

/// Delete a staged encode that will not be published. `NotFound` is expected
/// (ffmpeg may fail before creating it); anything else leaves an orphaned
/// `*.rustynes-partial.*` beside the user's files, so it is logged by path.
fn discard_staged(staged: &Path) {
    if let Err(e) = std::fs::remove_file(staged)
        && e.kind() != io::ErrorKind::NotFound
    {
        eprintln!(
            "rustynes: could not remove the partial encode {}: {e}",
            staged.display()
        );
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
            audio_offset_ms: 0,
        }
    }

    /// v3.5.0 (`FE-03`) — a zero offset is the pre-v3.5.0 pipeline exactly.
    /// The whole vector is pinned, so an offset argument that leaks into the
    /// default recording (or any reordering) fails here.
    #[test]
    fn a_zero_sync_offset_leaves_the_mux_arguments_unchanged() {
        let args = ffmpeg_args(&params(), Path::new("/v.raw"), Path::new("/a.raw"));
        let expected = [
            "-y",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            "256x240",
            "-framerate",
            "60098814/1000000",
            "-i",
            "/v.raw",
            "-f",
            "f32le",
            "-ar",
            "48000",
            "-ac",
            "1",
            "-i",
            "/a.raw",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-shortest",
            "/tmp/out.mp4",
        ];
        assert_eq!(args, expected);
    }

    /// v3.5.0 (`FE-03`) — a positive offset delays the audio with an output
    /// filter; a negative one seeks into the audio INPUT, so its `-ss` must
    /// sit between the audio format options and the audio `-i` (before the
    /// video `-i` it would seek the video instead).
    #[test]
    fn the_sync_offset_places_its_arguments_on_the_audio_stream() {
        let mut p = params();
        p.audio_offset_ms = 150;
        let a = ffmpeg_args(&p, Path::new("/v.raw"), Path::new("/a.raw"));
        let af = a.iter().position(|x| x == "-af").expect("an audio filter");
        assert_eq!(a[af + 1], "adelay=150");
        assert!(!a.iter().any(|x| x == "-ss"));
        // The filter is an output option: after both inputs.
        let last_input = a.iter().rposition(|x| x == "-i").unwrap();
        assert!(af > last_input);

        p.audio_offset_ms = -250;
        let a = ffmpeg_args(&p, Path::new("/v.raw"), Path::new("/a.raw"));
        let ss = a.iter().position(|x| x == "-ss").expect("an input seek");
        assert_eq!(a[ss + 1], "0.250");
        assert_eq!(a[ss + 2], "-i");
        assert_eq!(a[ss + 3], "/a.raw", "the seek applies to the audio input");
        assert!(!a.iter().any(|x| x == "-af"));

        // Clamped both ways.
        p.audio_offset_ms = 5000;
        let a = ffmpeg_args(&p, Path::new("/v"), Path::new("/a"));
        assert!(a.iter().any(|x| x == "adelay=1000"));
        p.audio_offset_ms = -5000;
        let a = ffmpeg_args(&p, Path::new("/v"), Path::new("/a"));
        assert!(a.iter().any(|x| x == "1.000"));
        assert_eq!(
            AvRecordOptions::default()
                .with_audio_offset_ms(-7000)
                .audio_offset_ms,
            -MAX_AUDIO_OFFSET_MS
        );
    }

    /// GIF has no audio and WAV no video: the offset has nothing to align
    /// there and must not change either pipeline.
    #[test]
    fn the_sync_offset_does_not_touch_gif_or_wav() {
        for ext in ["gif", "wav"] {
            let mut p = params();
            p.out_path = PathBuf::from(format!("/tmp/out.{ext}"));
            let base = ffmpeg_args(&p, Path::new("/v"), Path::new("/a"));
            p.audio_offset_ms = 300;
            assert_eq!(ffmpeg_args(&p, Path::new("/v"), Path::new("/a")), base);
            p.audio_offset_ms = -300;
            assert_eq!(ffmpeg_args(&p, Path::new("/v"), Path::new("/a")), base);
        }
    }

    /// v3.5.0 (`FE-03`) — the offset measured in a real encode. A click at
    /// 0.5 s is muxed with offsets 0, +120 and -120 ms, the audio track is
    /// decoded back, and the click must move by the offset. Comparing against
    /// the zero-offset encode cancels AAC's encoder delay, which is not the
    /// offset's to correct.
    ///
    /// Needs `ffmpeg` on `PATH` and returns early, saying so, without it (the
    /// recorder itself needs it at run time; the CI images do not ship it).
    #[test]
    fn the_sync_offset_moves_the_audio_in_a_real_encode() {
        if AvRecorder::probe_ffmpeg().is_err() {
            eprintln!("skipped: ffmpeg not on PATH");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let rate = 48_000_u32;
        // 1.5 s of black video and silence with one full-scale click at 0.5 s.
        let frames = 90_usize;
        std::fs::write(dir.path().join("v.raw"), vec![0u8; FRAME_BYTES * frames]).unwrap();
        let mut pcm = vec![0f32; (rate as usize) * 3 / 2];
        let click = rate as usize / 2;
        for s in &mut pcm[click..click + 48] {
            *s = 0.9;
        }
        let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        std::fs::write(dir.path().join("a.raw"), bytes).unwrap();

        let click_at = |offset_ms: i32| -> f64 {
            let out = dir.path().join(format!("o{offset_ms}.mkv"));
            let p = AvParams {
                out_path: out.clone(),
                sample_rate: rate,
                fps_num: 60,
                fps_den: 1,
                video_codec: VideoCodec::H264,
                crf: 30,
                preset: EncodePreset::Ultrafast,
                audio_bitrate_k: 128,
                audio_offset_ms: offset_ms,
            };
            let args = ffmpeg_args(&p, &dir.path().join("v.raw"), &dir.path().join("a.raw"));
            let st = Command::new("ffmpeg")
                .args(&args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(st.success(), "mux at {offset_ms} ms: {st}");
            let dec = Command::new("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(&out)
                .args(["-map", "0:a", "-f", "f32le", "-ac", "1", "-ar"])
                .arg(rate.to_string())
                .arg("-")
                .output()
                .unwrap();
            assert!(dec.status.success(), "decode at {offset_ms} ms");
            let samples: Vec<f32> = dec
                .stdout
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                .collect();
            let peak = samples
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                .map(|(i, _)| i)
                .unwrap();
            f64::from(u32::try_from(peak).unwrap()) * 1000.0 / f64::from(rate)
        };
        let base = click_at(0);
        let later = click_at(120);
        let earlier = click_at(-120);
        eprintln!("click at {base:.2} / {later:.2} / {earlier:.2} ms");
        assert!(
            (later - base - 120.0).abs() < 3.0,
            "+120: {base} -> {later}"
        );
        assert!(
            (base - earlier - 120.0).abs() < 3.0,
            "-120: {base} -> {earlier}"
        );
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
        // v2.1.9 GIF / WAV containers.
        assert_eq!(Container::from_path(Path::new("a.gif")), Container::Gif);
        assert_eq!(Container::from_path(Path::new("a.GIF")), Container::Gif);
        assert_eq!(Container::from_path(Path::new("a.wav")), Container::Wav);
        assert_eq!(Container::from_path(Path::new("a.WAV")), Container::Wav);
        // Unknown / missing extension defaults to mp4.
        assert_eq!(Container::from_path(Path::new("a.avi")), Container::Mp4);
        assert_eq!(Container::from_path(Path::new("noext")), Container::Mp4);
        // Stream presence flags.
        assert!(Container::Gif.has_video() && !Container::Gif.has_audio());
        assert!(!Container::Wav.has_video() && Container::Wav.has_audio());
        assert!(Container::Mp4.has_video() && Container::Mp4.has_audio());
    }

    #[test]
    fn gif_args_are_video_only_with_palette_filtergraph() {
        // v2.1.9 — a `.gif` output routes to the video-only palettegen pipeline:
        // one rawvideo input, NO audio input, and the split/palettegen/paletteuse
        // filter_complex the crisp-GIF workflow requires.
        let mut p = params();
        p.out_path = PathBuf::from("/tmp/out.gif");
        let args = ffmpeg_args(&p, Path::new("/tmp/v.raw"), Path::new("/tmp/a.pcm"));
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 1, "one input");
        assert!(args.iter().any(|a| a == "rawvideo"));
        assert!(!args.iter().any(|a| a == "f32le"), "no audio input");
        assert!(!args.iter().any(|a| a == "aac"));
        let fc = args.iter().position(|a| a == "-filter_complex").unwrap();
        assert!(args[fc + 1].contains("palettegen"));
        assert!(args[fc + 1].contains("paletteuse"));
        assert!(args[fc + 1].contains("fps=25"));
        assert_eq!(args.last().unwrap(), "/tmp/out.gif");
    }

    #[test]
    fn wav_args_are_audio_only_pcm() {
        // v2.1.9 — a `.wav` output routes to the audio-only PCM export: one
        // f32le input, NO video/rawvideo input, pcm_s16le codec, no `-shortest`.
        let mut p = params();
        p.out_path = PathBuf::from("/tmp/out.wav");
        let args = ffmpeg_args(&p, Path::new("/tmp/v.raw"), Path::new("/tmp/a.pcm"));
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 1, "one input");
        assert!(args.iter().any(|a| a == "f32le"));
        assert!(!args.iter().any(|a| a == "rawvideo"), "no video input");
        assert!(args.iter().any(|a| a == "pcm_s16le"));
        assert!(args.iter().any(|a| a == "48000"));
        assert_eq!(args.last().unwrap(), "/tmp/out.wav");
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

    #[test]
    fn the_staged_encode_keeps_its_directory_and_its_extension() {
        // Same directory: rename is atomic only within one filesystem.
        // Same extension: ffmpeg chooses the muxer from it, and the GIF / WAV
        // argument vectors are selected from it too.
        let s = staging_path(Path::new("/x/y/rec.gif"));
        assert_eq!(s, PathBuf::from("/x/y/rec.rustynes-partial.gif"));
        assert_eq!(Container::from_path(&s), Container::Gif);
        let mut p = params();
        p.out_path = s;
        let args = ffmpeg_args(&p, Path::new("/v"), Path::new("/a"));
        assert_eq!(args.last().unwrap(), "/x/y/rec.rustynes-partial.gif");
    }

    #[test]
    fn a_failed_encode_leaves_the_previous_recording_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("rec.mp4");
        let staged = staging_path(&out);
        std::fs::write(&out, b"the recording the user already had").unwrap();
        std::fs::write(&staged, b"half an enc").unwrap();

        let r = publish_output(&staged, &out, Err(AvError::Encode("killed".into())));
        assert!(r.is_err());
        assert_eq!(
            std::fs::read(&out).unwrap(),
            b"the recording the user already had"
        );
        assert!(!staged.exists(), "the partial encode is cleaned up");
    }

    #[test]
    fn a_finished_encode_is_kept_when_it_cannot_be_moved_into_place() {
        // The target's directory is gone, so the rename fails; the finished
        // encode must survive and be named in the error.
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join("rec.rustynes-partial.mp4");
        std::fs::write(&staged, b"a complete recording").unwrap();
        let out = dir.path().join("missing-dir").join("rec.mp4");

        let err = publish_output(&staged, &out, Ok(())).unwrap_err();
        assert_eq!(std::fs::read(&staged).unwrap(), b"a complete recording");
        assert!(format!("{err:?}").contains("rustynes-partial"), "{err:?}");
    }

    #[test]
    fn a_successful_encode_replaces_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("rec.mp4");
        let staged = staging_path(&out);
        std::fs::write(&out, b"old").unwrap();
        std::fs::write(&staged, b"new encode").unwrap();

        publish_output(&staged, &out, Ok(())).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"new encode");
        assert!(!staged.exists());
    }
}

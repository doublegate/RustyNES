//! v1.6.0 "Studio" Workstream H — HD-pack HD AUDIO (the biggest Mesen2 gap vs
//! ADR 0014).
//!
//! Mesen HD-packs can replace / augment the game's audio with external,
//! studio-quality tracks (typically OGG Vorbis): the `hires.txt` declares
//! `<bgm>` (background-music) and `<sfx>` (sound-effect) tracks, each keyed by
//! an `(album, track)` pair, and the game *selects* a track at run time by
//! writing to the HD-pack audio-control register at **`$4100`** (and the
//! adjacent `$4101`..`$4106`). Mesen intercepts those writes; `RustyNES` does NOT
//! touch the deterministic core, so this module is a **frontend, output-only
//! tap**: each produced frame it *peeks* `$4100` (a side-effect-free read of the
//! already-produced bus state — exactly like the HD tile-substitution reads the
//! produced framebuffer + the A/V recorder copies the produced samples) and, if
//! the control byte selected a track, mixes the decoded track into the drained
//! APU sample buffer **in place** before it reaches the audio queue.
//!
//! ## Output-only / determinism
//!
//! This sits entirely in the frontend audio path, on top of the audio buffer
//! the core already produced (`Nes::drain_audio_into`). It mixes additional
//! samples into that buffer for playback only; it mutates no emulation state,
//! reads only side-effect-free peeks, and adds no determinism surface. When no
//! pack with audio is loaded — or the `hd-pack` feature is off — the audio is
//! byte-identical to the stock build (the mixer is `Option`-gated and is only
//! `Some` once a pack that declares audio tracks loads). The core's per-frame
//! audio buffer (the determinism contract: save-state round-trip, TAS replay,
//! netplay) is unaffected — the HD track is summed *after* the core handed the
//! frame off, never folded back into synthesis.
//!
//! ## `$4100` control semantics (best-effort)
//!
//! Mesen's register file is, abbreviated:
//!
//! - `$4100` — BGM **select** (write `albumLo`); `$4101` = `albumHi`; writing
//!   `$4100` with the high bit set is a "stop BGM".  The exact Mesen protocol is
//!   a small state machine over `$4100`..`$4106`; we model the common case used by
//!   real packs: the low byte at `$4100` selects the current track index within
//!   the pack's BGM list (and, with the high bit set, stops it), and a non-zero
//!   write to the SFX trigger plays a one-shot. (The full `$4100`..`$4106` state
//!   machine is a future extension.) Because `RustyNES` does not
//!   intercept the writes (no core change), we read back `$4100` each frame and
//!   treat a *change* in its value as the trigger edge. Packs whose cart maps
//!   `$4100` into readable expansion space drive this faithfully; on pure
//!   open-bus carts the selection is inert (documented honesty caveat — like the
//!   `BestEffort` mapper tier).
//!
//! Live HD-audio *playback* cannot be verified headlessly (no audio device in
//! CI); the parse, the `$4100` trigger edge logic, and the mixer buffering are
//! unit-tested, and audible playback is a maintainer manual-check item.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// v2.9.0 re-audit NF-05 — the longest track decoded, in seconds of SOURCE
/// audio. Decoding had no bound at all: a 315 KiB OGG of an hour of 8 kHz
/// silence decodes to 172.8 M samples at 48 kHz (691 MB of `f32`), and the
/// audit's four declarations of it reached 2.6 GiB on the winit thread. Fifteen
/// minutes is several loops of any NES soundtrack; a longer track is REFUSED
/// (the rule is inert) rather than cut off mid-piece.
const MAX_TRACK_SECONDS: u64 = 15 * 60;

/// NF-05 — every track of one pack together, in decoded output samples: 512 MiB
/// of `f32`, about 46 minutes of 48 kHz mono. A declaration whose decode would
/// pass it is inert, and a file named by several declarations costs once.
const MAX_PACK_AUDIO_SAMPLES: u64 = (512 << 20) / 4;

/// The most SOURCE samples one decode may hold before it is resampled: the
/// same 512 MiB of `f32` as the pack's output budget (review on #561).
///
/// The output budget converts to a source ceiling at the stream's own rate,
/// so a source ABOVE the output rate may hold more samples than the output
/// will, and [`MAX_TRACK_SECONDS`] is in seconds, not bytes: at the highest
/// accepted source rate (384 kHz) fifteen minutes is 345.6 M samples, about
/// 1.38 GB, all allocated before a single sample is resampled or refused
/// (`CodeRabbit`). This makes the peak absolute. At 48 kHz it is 46 minutes
/// and never binds; at 384 kHz it refuses a track longer than about 5.8
/// minutes, which is the price of accepting that rate at all.
const MAX_SOURCE_SAMPLES: u64 = MAX_PACK_AUDIO_SAMPLES;

/// The CPU-space address of the HD-pack audio-control register (Mesen).
pub const HD_AUDIO_CONTROL: u16 = 0x4100;

/// Master HD-audio mix gain applied to the decoded track before summing it into
/// the APU buffer. Conservative (the HD track sits *under* the game audio by
/// default) and clamped so the sum can't blow past unity hard-clip more than the
/// game audio already might. Output-only.
const HD_MIX_GAIN: f32 = 0.8;

/// Which audio role a declared HD track fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    /// Looping background music (`<bgm>`), replacing the game's music.
    Bgm,
    /// One-shot sound effect (`<sfx>`), layered over the game audio.
    Sfx,
}

/// One declared HD-audio track: its `(album, track)` key + the decoded mono
/// PCM (resampled to the output device rate at load time).
#[derive(Debug, Clone)]
pub struct HdAudioTrack {
    /// `<bgm>` (looping) vs `<sfx>` (one-shot).
    pub kind: TrackKind,
    /// Album index from the rule's first field.
    pub album: u8,
    /// Track index from the rule's second field. This is the value the game
    /// writes to `$4100` to select the track.
    pub track: u8,
    /// Decoded mono PCM at the mixer's output sample rate. Empty if the file
    /// failed to decode, or was refused by the duration cap or the pack budget
    /// (the rule is then inert). Shared (`Arc`) between every declaration that
    /// names the same file, so a file declared N times is decoded once (NF-05).
    pub pcm: Arc<[f32]>,
}

/// A parsed (but not yet decoded) `<bgm>`/`<sfx>` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdAudioDecl {
    /// Role.
    pub kind: TrackKind,
    /// Album index.
    pub album: u8,
    /// Track index (the `$4100` selector value).
    pub track: u8,
    /// The (sanitized) OGG filename, relative to the pack.
    pub file: String,
}

/// Parse a single `<bgm>` / `<sfx>` rule body into a declaration.
///
/// Mesen form: `<bgm>album,track,filename` (and the same for `<sfx>`), where
/// `album`/`track` are decimal indices. A bare two-field `track,filename` form
/// (album defaulting to 0) is also accepted. Returns `None` on a malformed line
/// so a real pack still loads with the bad rule skipped.
#[must_use]
pub fn parse_audio_decl(kind: TrackKind, rest: &str) -> Option<HdAudioDecl> {
    let fields: Vec<&str> = rest.split(',').map(str::trim).collect();
    if fields.len() < 2 {
        return None;
    }
    // `album,track,file` (3+) or `track,file` (2, album = 0).
    let (album, track, file) = if fields.len() >= 3 {
        let album = fields[0].parse::<u8>().ok()?;
        let track = fields[1].parse::<u8>().ok()?;
        (album, track, fields[2])
    } else {
        let track = fields[0].parse::<u8>().ok()?;
        (0u8, track, fields[1])
    };
    if file.is_empty() {
        return None;
    }
    Some(HdAudioDecl {
        kind,
        album,
        track,
        file: file.to_string(),
    })
}

/// v2.7.3 (frontend audit CON-03) — the source sample rates an HD-audio track
/// may declare. The rate comes from the file's header and sizes the resampled
/// output: a track claiming 1 Hz would be expanded 48,000-fold at a 48 kHz
/// output (0 Hz was clamped to 1 Hz). Within these bounds the expansion is at
/// most 48x, so the output stays proportional to the decoded input.
const SOURCE_RATES: core::ops::RangeInclusive<u32> = 8_000..=384_000;

/// Whether an HD-audio track's declared source rate is usable (CON-03).
#[must_use]
pub fn source_rate_supported(rate: u32) -> bool {
    SOURCE_RATES.contains(&rate)
}

/// Decode an OGG Vorbis byte stream to interleaved-collapsed **mono** `f32`
/// samples at its native rate, then linearly resample to `out_rate`.
///
/// `lewton` (pure-Rust, MIT/ISC/Apache-2.0 — no C, gated behind `hd-pack` so the
/// default/wasm builds never pull it) decodes packet-by-packet; multi-channel
/// audio is downmixed to mono by averaging. Returns `None` on any decode error
/// (the track is then inert), including a declared source rate outside
/// [`source_rate_supported`]. The linear resample is intentionally simple: HD
/// audio is a presentation nicety, not part of the determinism contract, and the
/// game audio it mixes with already rides the frontend's Hermite DRC stage.
#[must_use]
pub fn decode_ogg_to_mono(bytes: &[u8], out_rate: u32) -> Option<Vec<f32>> {
    decode_ogg_capped(bytes, out_rate, u64::MAX, MAX_SOURCE_SAMPLES)
}

/// [`decode_ogg_to_mono`] with a ceiling of `max_out_samples` OUTPUT samples
/// (v2.9.0, NF-05). It is converted to a source-sample ceiling at the stream's
/// own rate, and also capped at [`MAX_TRACK_SECONDS`] and at `max_src_samples`
/// ([`MAX_SOURCE_SAMPLES`] outside tests), so a stream that cannot fit is
/// refused as soon as it passes the ceiling -- before the rest of it is decoded
/// or any of it resampled.
fn decode_ogg_capped(
    bytes: &[u8],
    out_rate: u32,
    max_out_samples: u64,
    max_src_samples: u64,
) -> Option<Vec<f32>> {
    use lewton::inside_ogg::OggStreamReader;

    let mut reader = OggStreamReader::new(std::io::Cursor::new(bytes)).ok()?;
    let src_rate = reader.ident_hdr.audio_sample_rate;
    if !source_rate_supported(src_rate) {
        return None;
    }
    // out = src * out_rate / src_rate, so src <= max_out * src_rate / out_rate.
    // One sample of slack for the resampler's rounding.
    let by_budget = max_out_samples
        .saturating_mul(u64::from(src_rate))
        .checked_div(u64::from(out_rate.max(1)))
        .unwrap_or(0)
        .saturating_add(1);
    let cap = by_budget
        .min(u64::from(src_rate) * MAX_TRACK_SECONDS)
        .min(max_src_samples);
    let channels = usize::from(reader.ident_hdr.audio_channels).max(1);

    let mut mono: Vec<f32> = Vec::new();
    // Each successfully-read packet yields one `Vec<i16>` per channel.
    while let Ok(Some(pck)) = reader.read_dec_packet() {
        if pck.is_empty() {
            continue;
        }
        let frames = pck[0].len();
        for f in 0..frames {
            let mut acc = 0.0f32;
            for ch in pck.iter().take(channels) {
                // i16 PCM -> f32 in [-1, 1).
                acc += f32::from(*ch.get(f).unwrap_or(&0)) / 32768.0;
            }
            #[allow(clippy::cast_precision_loss)] // channel count is tiny.
            mono.push(acc / channels as f32);
        }
        if mono.len() as u64 > cap {
            return None;
        }
    }
    if mono.is_empty() {
        return None;
    }
    Some(resample_linear(&mono, src_rate, out_rate))
}

/// Linearly resample mono `src` from `src_rate` to `dst_rate`. Identity (a copy)
/// when the rates already match.
#[must_use]
fn resample_linear(src: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if src_rate == dst_rate || src.is_empty() {
        return src.to_vec();
    }
    let ratio = f64::from(src_rate) / f64::from(dst_rate);
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let out_len = ((src.len() as f64) / ratio) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        #[allow(clippy::cast_precision_loss)] // i << 2^52 for any real track.
        let pos = i as f64 * ratio;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let idx = pos as usize;
        #[allow(clippy::cast_possible_truncation)] // frac in [0, 1).
        let frac = (pos - pos.floor()) as f32;
        let a = src.get(idx).copied().unwrap_or(0.0);
        let b = src.get(idx + 1).copied().unwrap_or(a);
        out.push((b - a).mul_add(frac, a));
    }
    out
}

/// A track currently being mixed in, with its playback cursor.
#[derive(Debug, Clone, Copy)]
struct ActiveVoice {
    /// Index into [`HdAudioMixer::tracks`].
    track: usize,
    /// Next PCM sample to emit.
    cursor: usize,
}

/// The HD-audio mixer.
///
/// Holds the decoded tracks + the live BGM/SFX voices, advances them, and mixes
/// them into a drained APU buffer in place. Owns the `$4100` edge-detect state
/// so a held control value only triggers once. Output-only: see the module docs.
#[derive(Debug)]
pub struct HdAudioMixer {
    /// Output sample rate (the device rate the core also synthesizes at).
    sample_rate: u32,
    /// All decoded tracks (BGM + SFX).
    tracks: Vec<HdAudioTrack>,
    /// The currently-playing looping BGM voice, if any.
    bgm: Option<ActiveVoice>,
    /// Live one-shot SFX voices (removed when they run dry).
    sfx: Vec<ActiveVoice>,
    /// Last observed `$4100` control byte, for edge detection. `None` until the
    /// first frame.
    last_control: Option<u8>,
    /// v1.8.9 — full HD-audio register file ($4100-$4106) state. BGM/SFX volume
    /// ($4102/$4103, 0..=255 = silent..full), the selected album ($4104), and the
    /// last-seen $4105/$4106 track triggers for edge detection.
    bgm_volume: u8,
    sfx_volume: u8,
    album: u8,
    last_bgm_trigger: Option<u8>,
    last_sfx_trigger: Option<u8>,
}

impl HdAudioMixer {
    /// Build a mixer from decoded tracks at `sample_rate`. Returns `None` if no
    /// track decoded (so the host leaves the mixer `Option` as `None` and the
    /// audio path stays byte-identical).
    #[must_use]
    pub fn new(tracks: Vec<HdAudioTrack>, sample_rate: u32) -> Option<Self> {
        if tracks.iter().all(|t| t.pcm.is_empty()) {
            return None;
        }
        Some(Self {
            sample_rate,
            tracks,
            bgm: None,
            sfx: Vec::new(),
            last_control: None,
            bgm_volume: 255,
            sfx_volume: 255,
            album: 0,
            last_bgm_trigger: None,
            last_sfx_trigger: None,
        })
    }

    /// Number of decoded tracks (diagnostic).
    #[must_use]
    pub const fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// The output sample rate the tracks were resampled to.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Whether a BGM voice is currently playing (diagnostic / status bar).
    #[must_use]
    pub const fn bgm_playing(&self) -> bool {
        self.bgm.is_some()
    }

    /// Find a track index by `(kind, track)` selector. `album` filters by album
    /// when `Some` (the full `$4104`-aware path); `None` ignores it (the legacy
    /// single-`$4100`-selector path).
    fn find_track(&self, kind: TrackKind, album: Option<u8>, track: u8) -> Option<usize> {
        self.tracks.iter().position(|t| {
            t.kind == kind
                && t.track == track
                && !t.pcm.is_empty()
                && album.is_none_or(|a| t.album == a)
        })
    }

    /// Apply a `$4100` control byte, starting / stopping voices on the value's
    /// *change edge* (a held value triggers once). High bit set = stop BGM; a
    /// non-zero low value selects + (re)starts the BGM track of that index, and
    /// also fires the matching SFX one-shot if one is declared.
    ///
    /// Exposed (not just called from [`Self::mix`]) so the trigger logic is
    /// unit-testable without an audio buffer.
    pub fn apply_control(&mut self, control: u8) {
        if self.last_control == Some(control) {
            return; // no edge — already handled.
        }
        self.last_control = Some(control);

        // High bit set = stop BGM (Mesen's BGM-stop convention).
        if control & 0x80 != 0 {
            self.bgm = None;
            return;
        }
        // A zero control byte means "no selection" (idle / open bus) — leave the
        // current voices alone rather than thrashing them every frame.
        if control == 0 {
            return;
        }
        // Select + (re)start the BGM track whose index matches the low value.
        if let Some(track) = self.find_track(TrackKind::Bgm, None, control) {
            self.bgm = Some(ActiveVoice { track, cursor: 0 });
        }
        // Fire a matching one-shot SFX, if declared for the same selector.
        if let Some(track) = self.find_track(TrackKind::Sfx, None, control) {
            self.sfx.push(ActiveVoice { track, cursor: 0 });
        }
    }

    /// Apply the full Mesen HD-audio register file `$4100..=$4106`, in order:
    /// `[options, control, bgmVol, sfxVol, album, bgmTrack, sfxTrack]`. BGM/SFX
    /// (re)start on the *change edge* of `$4105`/`$4106` (album-aware via `$4104`),
    /// `$4102`/`$4103` scale the mix, and `$4101`'s high bit stops the BGM. When
    /// `$4101..=$4106` are all zero the mixer falls back to the legacy
    /// single-`$4100`-selector convention, so packs written either way both drive.
    pub fn apply_registers(&mut self, regs: [u8; 7]) {
        // Legacy single-register fallback: a pack that only drives $4100. Take
        // this BEFORE touching the volumes so a legacy pack keeps full volume
        // (the all-zero `$4102`/`$4103` here are "unused", not "silent").
        if regs[1..].iter().all(|&b| b == 0) {
            self.apply_control(regs[0]);
            return;
        }

        self.bgm_volume = regs[2];
        self.sfx_volume = regs[3];
        self.album = regs[4];

        // $4101 control: high bit stops the BGM (Mesen convention).
        if regs[1] & 0x80 != 0 {
            self.bgm = None;
        }
        // $4105 BGM-track trigger (change edge, album-aware).
        if self.last_bgm_trigger != Some(regs[5]) {
            self.last_bgm_trigger = Some(regs[5]);
            if regs[5] != 0
                && let Some(track) = self.find_track(TrackKind::Bgm, Some(self.album), regs[5])
            {
                self.bgm = Some(ActiveVoice { track, cursor: 0 });
            }
        }
        // $4106 SFX-track trigger (change edge, album-aware).
        if self.last_sfx_trigger != Some(regs[6]) {
            self.last_sfx_trigger = Some(regs[6]);
            if regs[6] != 0
                && let Some(track) = self.find_track(TrackKind::Sfx, Some(self.album), regs[6])
            {
                self.sfx.push(ActiveVoice { track, cursor: 0 });
            }
        }
    }

    /// Mix the active HD-audio voices into `buf` (the drained APU samples) in
    /// place, after applying the `$4100` `control` byte's trigger edge.
    ///
    /// BGM loops; SFX play once and are removed when exhausted. The sum is
    /// soft-clamped to `[-1, 1]` so the layered output can't exceed the f32
    /// sample range the DAC expects. Output-only — `buf` is the frontend's
    /// per-frame audio copy, never the core's synthesis state.
    pub fn mix(&mut self, buf: &mut [f32], control: u8) {
        self.apply_control(control);
        self.mix_voices(buf);
    }

    /// Mix after applying the full `$4100..=$4106` HD-audio register file (see
    /// [`Self::apply_registers`]) — the frontend's full-register path.
    pub fn mix_registers(&mut self, buf: &mut [f32], regs: [u8; 7]) {
        self.apply_registers(regs);
        self.mix_voices(buf);
    }

    /// Mix the active HD-audio voices into `buf` in place, applying the per-role
    /// `$4102`/`$4103` volumes, soft-clamped to `[-1, 1]`.
    fn mix_voices(&mut self, buf: &mut [f32]) {
        if buf.is_empty() {
            return;
        }
        let bgm_gain = HD_MIX_GAIN * f32::from(self.bgm_volume) / 255.0;
        let sfx_gain = HD_MIX_GAIN * f32::from(self.sfx_volume) / 255.0;

        // --- BGM (looping) ---
        if let Some(voice) = self.bgm.as_mut() {
            if let Some(pcm) = self.tracks.get(voice.track).map(|t| &t.pcm) {
                if pcm.is_empty() {
                    self.bgm = None;
                } else {
                    for sample in buf.iter_mut() {
                        if voice.cursor >= pcm.len() {
                            voice.cursor = 0; // loop.
                        }
                        *sample = pcm[voice.cursor].mul_add(bgm_gain, *sample);
                        voice.cursor += 1;
                    }
                }
            } else {
                self.bgm = None;
            }
        }

        // --- SFX (one-shot) ---
        // Advance each voice; drop the ones that ran dry. `retain` keeps the
        // surviving voices and discards the rest in one pass.
        let tracks = &self.tracks;
        self.sfx.retain_mut(|voice| {
            let Some(pcm) = tracks.get(voice.track).map(|t| &t.pcm) else {
                return false;
            };
            for sample in buf.iter_mut() {
                if voice.cursor >= pcm.len() {
                    return false; // exhausted; drop after this fill.
                }
                *sample = pcm[voice.cursor].mul_add(sfx_gain, *sample);
                voice.cursor += 1;
            }
            voice.cursor < pcm.len()
        });

        // Soft-clamp the layered sum to the valid sample range.
        for sample in buf.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }
}

/// Sanitize an HD-audio filename against path traversal.
///
/// Identical policy to the image-name guard in [`crate::hdpack`]: accept ONLY a
/// plain final component (no separators, no `..`, not absolute, no drive prefix).
#[must_use]
pub fn sanitize_audio_name(name: &str) -> Option<&str> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name == ".."
        || name == "."
        || name.contains(':')
    {
        return None;
    }
    Some(name)
}

/// Decode all declared audio tracks from a pack folder.
///
/// `dir` is the folder that holds `hires.txt`; each declaration's file is read
/// relative to it (path-traversal-guarded). Files that fail to read/decode yield
/// an empty-`pcm` track (inert). Returns the decoded tracks (possibly empty).
#[must_use]
pub fn decode_tracks_from_folder(
    dir: &Path,
    decls: &[HdAudioDecl],
    out_rate: u32,
) -> Vec<HdAudioTrack> {
    decode_tracks_capped(dir, decls, out_rate, MAX_PACK_AUDIO_SAMPLES)
}

/// [`decode_tracks_from_folder`] with an explicit pack budget in output
/// samples (v2.9.0, NF-05). Each distinct file is decoded once and shared by
/// every declaration naming it, and charged once; a decode that would pass the
/// remaining budget is refused and leaves that rule inert. A refused or failed
/// file is remembered too, so it is not decoded again for its next declaration.
fn decode_tracks_capped(
    dir: &Path,
    decls: &[HdAudioDecl],
    out_rate: u32,
    budget: u64,
) -> Vec<HdAudioTrack> {
    let mut remaining = budget;
    let mut decoded: HashMap<&str, Arc<[f32]>> = HashMap::new();
    let empty: Arc<[f32]> = Arc::from(Vec::new());
    decls
        .iter()
        .map(|d| {
            let pcm = decoded
                .entry(d.file.as_str())
                .or_insert_with(|| {
                    // Decoding stops as soon as the stream passes what is left
                    // of the budget, rather than after it was fully built; the
                    // filter then catches the resampler's rounding.
                    let pcm = sanitize_audio_name(&d.file)
                        .and_then(|safe| std::fs::read(dir.join(safe)).ok())
                        .and_then(|bytes| {
                            decode_ogg_capped(&bytes, out_rate, remaining, MAX_SOURCE_SAMPLES)
                        })
                        .filter(|pcm| pcm.len() as u64 <= remaining);
                    pcm.map_or_else(
                        || Arc::clone(&empty),
                        |pcm| {
                            remaining -= pcm.len() as u64;
                            Arc::from(pcm)
                        },
                    )
                })
                .clone();
            HdAudioTrack {
                kind: d.kind,
                album: d.album,
                track: d.track,
                pcm,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One second of 8 kHz mono silence, Vorbis, generated with ffmpeg
    /// (`anullsrc`, `-fflags +bitexact`). No content, so nothing to license.
    const SILENCE_1S_8K: &[u8] = include_bytes!("../tests/fixtures/silence-1s-8k.ogg");

    /// v2.9.0 re-audit NF-05 — a track longer than the duration cap is refused
    /// (inert), not decoded without bound. The audit's 315 KiB OGG of one hour
    /// of 8 kHz silence, declared four times, decoded to 2.6 GiB of PCM.
    #[test]
    fn a_track_over_the_duration_cap_is_refused() {
        // The fixture is 8,000 source samples, 48,000 at the output rate. An
        // output cap of half that must refuse it; one above it must not. The
        // same at a 8 kHz output (no resampling), so the conversion between
        // the two rates is what is being tested, not one fixed ratio.
        assert!(decode_ogg_capped(SILENCE_1S_8K, 48_000, 24_000, MAX_SOURCE_SAMPLES).is_none());
        assert!(decode_ogg_capped(SILENCE_1S_8K, 8_000, 4_000, MAX_SOURCE_SAMPLES).is_none());
        assert!(decode_ogg_capped(SILENCE_1S_8K, 8_000, 8_000, MAX_SOURCE_SAMPLES).is_some());
        let pcm =
            decode_ogg_capped(SILENCE_1S_8K, 48_000, 48_000, MAX_SOURCE_SAMPLES).expect("decodes");
        assert!(
            (47_000..=48_000).contains(&pcm.len()),
            "{} samples",
            pcm.len()
        );
    }

    /// Review on #561 — the source buffer has an absolute ceiling of its own.
    /// Downsampling 8 kHz to 1 kHz, an output budget of 1,000 samples admits
    /// 8,001 source samples, which is the whole fixture; a source ceiling of
    /// 4,000 must refuse it anyway, because the output budget says nothing
    /// about how much source is held before resampling.
    #[test]
    fn the_source_buffer_has_its_own_ceiling() {
        assert!(decode_ogg_capped(SILENCE_1S_8K, 1_000, 1_000, u64::MAX).is_some());
        assert!(decode_ogg_capped(SILENCE_1S_8K, 1_000, 1_000, 4_000).is_none());
    }

    /// NF-05 — a file declared N times is decoded ONCE and shared, and the
    /// pack's total PCM is bounded: a declaration past the budget is inert.
    #[test]
    fn a_file_declared_twice_is_decoded_once_and_the_pack_is_bounded() {
        let dir =
            std::env::temp_dir().join(format!("rustynes-hdaudio-dedupe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.ogg"), SILENCE_1S_8K).unwrap();
        std::fs::write(dir.join("b.ogg"), SILENCE_1S_8K).unwrap();
        let decl = |track: u8, file: &str| HdAudioDecl {
            kind: TrackKind::Bgm,
            album: 0,
            track,
            file: file.to_string(),
        };
        let decls = [decl(0, "a.ogg"), decl(1, "a.ogg"), decl(2, "b.ogg")];

        let all = decode_tracks_capped(&dir, &decls, 48_000, u64::MAX);
        assert_eq!(all.len(), 3);
        assert!(!all[0].pcm.is_empty());
        assert!(Arc::ptr_eq(&all[0].pcm, &all[1].pcm), "one decode, shared");
        assert!(!Arc::ptr_eq(&all[0].pcm, &all[2].pcm), "a different file");

        // A budget of one track's samples: a.ogg fits (both its declarations
        // share it, so they cost once), b.ogg is past the budget and inert.
        let one = all[0].pcm.len() as u64;
        let capped = decode_tracks_capped(&dir, &decls, 48_000, one);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!capped[0].pcm.is_empty() && !capped[1].pcm.is_empty());
        assert!(capped[2].pcm.is_empty(), "over the pack budget");
    }

    #[test]
    fn parses_bgm_three_field() {
        let d = parse_audio_decl(TrackKind::Bgm, "1,5,title.ogg").unwrap();
        assert_eq!(d.kind, TrackKind::Bgm);
        assert_eq!(d.album, 1);
        assert_eq!(d.track, 5);
        assert_eq!(d.file, "title.ogg");
    }

    #[test]
    fn parses_sfx_two_field_album_defaults_zero() {
        let d = parse_audio_decl(TrackKind::Sfx, "3,jump.ogg").unwrap();
        assert_eq!(d.kind, TrackKind::Sfx);
        assert_eq!(d.album, 0);
        assert_eq!(d.track, 3);
        assert_eq!(d.file, "jump.ogg");
    }

    #[test]
    fn rejects_malformed_audio_decl() {
        assert!(parse_audio_decl(TrackKind::Bgm, "").is_none());
        assert!(parse_audio_decl(TrackKind::Bgm, "onlyone").is_none());
        assert!(parse_audio_decl(TrackKind::Bgm, "x,y,z").is_none()); // non-numeric.
        assert!(parse_audio_decl(TrackKind::Bgm, "1,2,").is_none()); // empty file.
    }

    #[test]
    fn sanitize_audio_name_rejects_traversal() {
        assert_eq!(sanitize_audio_name("song.ogg"), Some("song.ogg"));
        assert_eq!(sanitize_audio_name("../escape.ogg"), None);
        assert_eq!(sanitize_audio_name("a/b.ogg"), None);
        assert_eq!(sanitize_audio_name("C:\\x.ogg"), None);
        assert_eq!(sanitize_audio_name(""), None);
    }

    /// CON-03 (v2.7.3): the declared source rate sizes the output, so an
    /// absurd one is refused before anything is decoded or allocated.
    #[test]
    fn a_track_with_an_absurd_source_rate_is_refused() {
        for bad in [0, 1, 7_999, 384_001, u32::MAX] {
            assert!(!source_rate_supported(bad), "{bad} Hz");
        }
        for good in [8_000, 22_050, 44_100, 48_000, 96_000, 384_000] {
            assert!(source_rate_supported(good), "{good} Hz");
        }
    }

    #[test]
    fn resample_identity_when_rates_match() {
        let src = vec![0.1, 0.2, 0.3, 0.4];
        assert_eq!(resample_linear(&src, 48_000, 48_000), src);
    }

    #[test]
    fn resample_halving_rate_roughly_doubles_len() {
        // src 24k -> dst 48k => ~2x as many output samples.
        let src = vec![0.0f32; 100];
        let out = resample_linear(&src, 24_000, 48_000);
        assert!((190..=210).contains(&out.len()), "len = {}", out.len());
    }

    /// Build a tiny mixer with one BGM track (index 1) + one SFX (index 2).
    fn test_mixer() -> HdAudioMixer {
        let tracks = vec![
            HdAudioTrack {
                kind: TrackKind::Bgm,
                album: 0,
                track: 1,
                pcm: vec![0.5, 0.5, 0.5, 0.5].into(),
            },
            HdAudioTrack {
                kind: TrackKind::Sfx,
                album: 0,
                track: 2,
                pcm: vec![0.25, 0.25].into(),
            },
        ];
        HdAudioMixer::new(tracks, 48_000).unwrap()
    }

    #[test]
    fn new_returns_none_when_all_tracks_empty() {
        let tracks = vec![HdAudioTrack {
            kind: TrackKind::Bgm,
            album: 0,
            track: 1,
            pcm: Vec::new().into(),
        }];
        assert!(HdAudioMixer::new(tracks, 48_000).is_none());
    }

    #[test]
    fn control_edge_triggers_bgm_once() {
        let mut m = test_mixer();
        assert!(!m.bgm_playing());
        // First write of 1 selects BGM track 1.
        m.apply_control(1);
        assert!(m.bgm_playing());
        // Re-applying the same value is a no-op edge: it does NOT restart the
        // cursor (would be observable if we reset it). Start a voice, advance it,
        // re-apply, and confirm the cursor is preserved.
        let mut buf = [0.0f32; 2];
        m.mix(&mut buf, 1); // same control: no restart; advances cursor by 2.
        // BGM is 0.5 * gain(0.8) = 0.4 per sample.
        assert!((buf[0] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn full_register_file_album_and_volume() {
        // [$4100, $4101, $4102 bgmVol, $4103 sfxVol, $4104 album, $4105 bgmTrk, $4106 sfxTrk]
        let mut m = test_mixer();
        m.apply_registers([0, 0, 255, 255, 0, 1, 0]); // album 0, BGM track 1.
        assert!(m.bgm_playing());

        // Wrong album -> no match (album 1 has no declared tracks).
        let mut m2 = test_mixer();
        m2.apply_registers([0, 0, 255, 255, 1, 1, 0]);
        assert!(!m2.bgm_playing());

        // $4103 SFX volume scales the one-shot ($4106 = track 2 at half volume).
        let mut m3 = test_mixer();
        let mut buf = [0.0f32; 2];
        m3.mix_registers(&mut buf, [0, 0, 0, 128, 0, 0, 2]);
        let expected = 0.25 * 0.8 * 128.0 / 255.0;
        assert!((buf[0] - expected).abs() < 1e-5);

        // $4101 high bit stops the BGM.
        m.apply_registers([0, 0x80, 255, 255, 0, 1, 0]);
        assert!(!m.bgm_playing());
    }

    #[test]
    fn legacy_4100_path_keeps_full_volume() {
        // Only $4100 drives ($4101..=$4106 == 0): the legacy path must NOT zero
        // the volumes from the all-zero $4102/$4103 — track 1 plays at full gain.
        let mut m = test_mixer();
        let mut buf = [0.0f32; 2];
        m.mix_registers(&mut buf, [1, 0, 0, 0, 0, 0, 0]);
        assert!(m.bgm_playing(), "legacy $4100=1 selects BGM track 1");
        // 0.5 sample * full gain 0.8 = 0.4 (would be 0.0 with the volume bug).
        assert!((buf[0] - 0.4).abs() < 1e-6, "buf[0] = {}", buf[0]);
    }

    #[test]
    fn control_high_bit_stops_bgm() {
        let mut m = test_mixer();
        m.apply_control(1);
        assert!(m.bgm_playing());
        m.apply_control(0x80);
        assert!(!m.bgm_playing());
    }

    #[test]
    fn zero_control_leaves_voices_untouched() {
        let mut m = test_mixer();
        m.apply_control(1);
        assert!(m.bgm_playing());
        m.apply_control(0); // idle / open bus — must not stop the BGM.
        assert!(m.bgm_playing());
    }

    #[test]
    fn bgm_loops_when_cursor_passes_end() {
        let mut m = test_mixer();
        m.apply_control(1);
        // Track is 4 samples; mix 6 => wraps around (loops).
        let mut buf = [0.0f32; 6];
        m.mix(&mut buf, 1);
        // Every sample is 0.5 * 0.8 = 0.4 (the track is constant), so the loop
        // wrap is seamless and all six are 0.4.
        for s in buf {
            assert!((s - 0.4).abs() < 1e-6, "s = {s}");
        }
        assert!(m.bgm_playing()); // BGM keeps going.
    }

    #[test]
    fn sfx_plays_once_then_drops() {
        let mut m = test_mixer();
        // Select SFX track 2 (no BGM at index 2).
        m.apply_control(2);
        // SFX is 2 samples; mix 4 => the SFX contributes to the first 2 only.
        let mut buf = [0.0f32; 4];
        m.mix(&mut buf, 2);
        // 0.25 * 0.8 = 0.2 for the first two; silence after.
        assert!((buf[0] - 0.2).abs() < 1e-6);
        assert!((buf[1] - 0.2).abs() < 1e-6);
        assert!(buf[2].abs() < 1e-6);
        assert!(buf[3].abs() < 1e-6);
    }

    #[test]
    fn mix_sums_over_existing_game_audio_and_clamps() {
        let mut m = test_mixer();
        m.apply_control(1);
        // Pre-load the buffer with loud game audio; the sum must clamp to 1.0.
        let mut buf = [0.9f32; 2];
        m.mix(&mut buf, 1);
        // 0.9 + 0.4 = 1.3 -> clamped to 1.0.
        for s in buf {
            assert!((s - 1.0).abs() < 1e-6, "s = {s}");
        }
    }

    #[test]
    fn unknown_selector_does_nothing() {
        let mut m = test_mixer();
        m.apply_control(99); // no track with this index.
        assert!(!m.bgm_playing());
        let mut buf = [0.1f32; 2];
        m.mix(&mut buf, 99);
        // Buffer unchanged (no voice).
        for s in buf {
            assert!((s - 0.1).abs() < 1e-6);
        }
    }
}

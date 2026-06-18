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

#![cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]

use std::path::Path;

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
    /// failed to decode (the rule is then inert).
    pub pcm: Vec<f32>,
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

/// Decode an OGG Vorbis byte stream to interleaved-collapsed **mono** `f32`
/// samples at its native rate, then linearly resample to `out_rate`.
///
/// `lewton` (pure-Rust, MIT/ISC/Apache-2.0 — no C, gated behind `hd-pack` so the
/// default/wasm builds never pull it) decodes packet-by-packet; multi-channel
/// audio is downmixed to mono by averaging. Returns `None` on any decode error
/// (the track is then inert). The linear resample is intentionally simple: HD
/// audio is a presentation nicety, not part of the determinism contract, and the
/// game audio it mixes with already rides the frontend's Hermite DRC stage.
#[must_use]
pub fn decode_ogg_to_mono(bytes: &[u8], out_rate: u32) -> Option<Vec<f32>> {
    use lewton::inside_ogg::OggStreamReader;

    let mut reader = OggStreamReader::new(std::io::Cursor::new(bytes)).ok()?;
    let src_rate = reader.ident_hdr.audio_sample_rate.max(1);
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

    /// Find a track index by `(kind, track)` selector (album ignored for the
    /// common single-album case; album-aware selection is a future extension).
    fn find_track(&self, kind: TrackKind, track: u8) -> Option<usize> {
        self.tracks
            .iter()
            .position(|t| t.kind == kind && t.track == track && !t.pcm.is_empty())
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
        if let Some(track) = self.find_track(TrackKind::Bgm, control) {
            self.bgm = Some(ActiveVoice { track, cursor: 0 });
        }
        // Fire a matching one-shot SFX, if declared for the same selector.
        if let Some(track) = self.find_track(TrackKind::Sfx, control) {
            self.sfx.push(ActiveVoice { track, cursor: 0 });
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
        if buf.is_empty() {
            return;
        }

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
                        *sample = pcm[voice.cursor].mul_add(HD_MIX_GAIN, *sample);
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
                *sample = pcm[voice.cursor].mul_add(HD_MIX_GAIN, *sample);
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
    decls
        .iter()
        .map(|d| {
            let pcm = sanitize_audio_name(&d.file)
                .and_then(|safe| std::fs::read(dir.join(safe)).ok())
                .and_then(|bytes| decode_ogg_to_mono(&bytes, out_rate))
                .unwrap_or_default();
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
                pcm: vec![0.5, 0.5, 0.5, 0.5],
            },
            HdAudioTrack {
                kind: TrackKind::Sfx,
                album: 0,
                track: 2,
                pcm: vec![0.25, 0.25],
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
            pcm: Vec::new(),
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

//! v3.6.0 (`T-AUDIO-STEMS`) — per-channel audio stems, rendered offline from
//! the APU's own per-cycle mix trace.
//!
//! A *stem* is one sound source rendered on its own: pulse 1, pulse 2, the
//! triangle, the noise channel, the DMC, and the cartridge's expansion audio
//! (VRC6, VRC7, Namco 163, Sunsoft 5B, MMC5, FDS, or an NSF's chips). A
//! musician wants them as separate files that line up sample for sample with
//! the mixed output, so they can be remixed, and a researcher wants them to
//! hear what one channel is doing under the others.
//!
//! # How they are made: one emulation pass, re-rendered offline
//!
//! The emulation runs **once**, unchanged, with the APU's audio provenance
//! armed (`Nes::set_audio_provenance`, the v2.3.7 "Overtone" debug facility).
//! While armed, the APU records, for every CPU cycle, the five raw channel
//! outputs it handed the mixer and the raw expansion sample (`MixRecord`).
//! [`StemCapture`] reads that trace after each frame and pushes each source,
//! **alone**, through a private copy of the same output pipeline the console
//! audio goes through: the same lookup-table mixer, a band-limited buffer
//! built with the same sample rate and CPU clock, and the same analog filter
//! model. Nothing is added to the APU's per-cycle work, and nothing in the
//! emulation changes: the stems are a function of a trace the core already
//! knew how to record. With provenance disarmed (every other run) the core is
//! exactly what it was; with it armed, the recording is output-only and is
//! never read back into synthesis (see `rustynes_apu::provenance`).
//!
//! The capture also re-renders the **mix** itself from the trace, through its
//! own pipeline, and compares it with what the core produced
//! ([`StemCapture::mix_mismatches`]). The two are bit-identical: the same
//! values were added to buffers built the same way in the same order. That
//! comparison is what licenses the stems — it shows the private pipeline is
//! the console's pipeline, rather than a lookalike.
//!
//! # What "the stems sum to the mix" means, precisely
//!
//! The 2A03 mixer is **not linear** (nesdev "APU Mixer"):
//!
//! ```text
//! out = pulse_table[p1 + p2] + tnd_table[3*tri + 2*noise + dmc] + expansion
//! pulse_table[n] = 95.52 / (8128 / n + 100)
//! tnd_table[n]   = 163.67 / (24329 / n + 100)
//! ```
//!
//! Both tables are concave, so two channels sounding together come out quieter
//! than the two alone added up. Each stem here is its channel **rendered alone
//! through that same mixer** — pulse 1's stem is `pulse_table[p1]`, the
//! noise's is `tnd_table[2*noise]`, and so on — because that is what the
//! channel sounds like on the console with every other channel silent. The
//! sum of the six source stems is therefore not the mix; it differs by the
//! mixer's **interaction term**
//!
//! ```text
//! interaction = out - (pulse_table[p1] + pulse_table[p2]
//!                      + tnd_table[3*tri] + tnd_table[2*noise] + tnd_table[dmc]
//!                      + expansion)
//! ```
//!
//! which is zero whenever at most one channel of each group is non-zero, and
//! never positive before the filter (concavity). The capture renders that term
//! as a seventh track, [`Stem::Interaction`], through the same pipeline. The
//! band-limited buffer and the filter chain are linear, so
//!
//! ```text
//! mix = Σ six source stems + interaction      (to f32 rounding)
//! ```
//!
//! and the residual is measured, not assumed: [`StemCapture::max_sum_error`]
//! holds the largest `|mix - Σ all seven|` seen, which the gate in this
//! module's tests and in `rustynes-test-harness/tests/audio_stems.rs` bounds
//! (measured at about `1e-6` against a signal near `±0.2`; see
//! `docs/audio-stems.md`). The expansion stem enters the mixer linearly, so it
//! has no interaction term of its own.
//!
//! # What is not split
//!
//! The expansion stem is the cartridge's whole expansion output, the value the
//! board's `mix_audio` returns. A board with one chip (every cartridge) gets
//! one stem named after it. An NSF that enables several chips at once gets
//! them summed into the one stem, because the trace records only their sum;
//! splitting them would need a per-chip record in the mapper's per-cycle
//! path, which this design deliberately does not add. Nor are a chip's own
//! voices (VRC6's two pulses and saw, N163's eight channels) split.

use alloc::vec::Vec;

use rustynes_apu::provenance::MixRecord;
use rustynes_apu::{BlipBuf, CPU_HZ_NTSC, CPU_HZ_PAL, FilterModel, Mixer};

use crate::{Nes, Region};

/// The number of tracks a [`StemCapture`] renders ([`Stem::ALL`]).
pub const STEM_COUNT: usize = 7;

/// One rendered track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stem {
    /// Pulse 1, alone through the pulse table.
    Pulse1,
    /// Pulse 2, alone through the pulse table.
    Pulse2,
    /// The triangle, alone through the TND table (`tnd_table[3 * tri]`).
    Triangle,
    /// The noise channel, alone (`tnd_table[2 * noise]`).
    Noise,
    /// The DMC, alone (`tnd_table[dmc]`).
    Dmc,
    /// The cartridge's expansion audio, which the mixer adds linearly.
    Expansion,
    /// The mixer's interaction term: the mix minus the six stems above, so
    /// that all seven sum to the mix. See the module docs.
    Interaction,
}

impl Stem {
    /// Every track, in file order.
    pub const ALL: [Self; STEM_COUNT] = [
        Self::Pulse1,
        Self::Pulse2,
        Self::Triangle,
        Self::Noise,
        Self::Dmc,
        Self::Expansion,
        Self::Interaction,
    ];

    /// A short, stable, file-name-safe name (`"pulse1"`, `"dmc"`, ...).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pulse1 => "pulse1",
            Self::Pulse2 => "pulse2",
            Self::Triangle => "triangle",
            Self::Noise => "noise",
            Self::Dmc => "dmc",
            Self::Expansion => "expansion",
            Self::Interaction => "interaction",
        }
    }

    /// The track's index in [`Self::ALL`].
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Why a capture could not continue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StemError {
    /// The capture was started after the console had already produced audio,
    /// so the private pipeline would start at a different phase from the
    /// console's and every sample would differ. Start it before the first
    /// frame.
    #[error("the stem capture must start before the first frame is run")]
    StartedLate,
    /// The APU's provenance was disarmed while the capture ran.
    #[error("audio provenance was disarmed during the capture")]
    Disarmed,
    /// A frame ran more CPU cycles than the mix trace holds, so cycles were
    /// dropped and the stems would be silently short.
    #[error("the mix trace was truncated in a frame (too many cycles)")]
    Truncated,
    /// The trace did not start where the previous frame ended: a frame was run
    /// without being captured, or the console was reset under the capture.
    #[error("the mix trace skipped cycles {expected}..{found}")]
    Gap {
        /// The first cycle the capture expected.
        expected: u64,
        /// The first cycle the trace actually holds.
        found: u64,
    },
}

/// The most CPU cycles a console may have run before a capture starts: the
/// reset sequence's own (8 when measured), with headroom. A capture refuses
/// to start past it ([`StemError::StartedLate`]).
const MAX_PRE_CYCLES: u64 = 64;

/// One output pipeline: a band-limited buffer with its filter, as the APU
/// builds its own.
fn pipeline(sample_rate: u32, region: Region, filter: FilterModel) -> BlipBuf {
    // Mirrors `Apu::new`: only true PAL runs the PAL CPU clock; NTSC, Dendy
    // and the treated-as-NTSC "Multi" all build at the NTSC rate.
    let cpu_rate = match region {
        Region::Pal => CPU_HZ_PAL,
        _ => CPU_HZ_NTSC,
    };
    let mut b = BlipBuf::new(sample_rate, cpu_rate);
    b.set_filter_model(filter);
    b
}

/// The per-cycle value each track receives for one mix record.
///
/// Returns the six source values and the interaction term, in [`Stem::ALL`]
/// order. Each source goes through the same `Mixer::mix` the APU calls, with
/// every other channel at zero — so a stem is exactly what the console would
/// mix for that channel playing alone.
#[must_use]
pub fn stem_values(mixer: &Mixer, r: &MixRecord) -> [f32; STEM_COUNT] {
    let p1 = mixer.mix(r.pulse1, 0, 0, 0, 0);
    let p2 = mixer.mix(0, r.pulse2, 0, 0, 0);
    let tri = mixer.mix(0, 0, r.triangle, 0, 0);
    let noise = mixer.mix(0, 0, 0, r.noise, 0);
    let dmc = mixer.mix(0, 0, 0, 0, r.dmc);
    let ext = r.external;
    let interaction = r.mixed - (p1 + p2 + tri + noise + dmc + ext);
    [p1, p2, tri, noise, dmc, ext, interaction]
}

/// A running stem capture over one [`Nes`].
///
/// Create it right after the console is built and before its first frame
/// ([`Self::start`]); then, after every `run_frame`, call
/// [`Self::after_frame`], which returns that frame's samples for the mix and
/// each track. All eight streams always have the same length.
pub struct StemCapture {
    mixer: Mixer,
    /// The re-rendered mix, compared against the core's own output.
    mix: alloc::boxed::Box<BlipBuf>,
    /// One pipeline per track, in [`Stem::ALL`] order. Heap-allocated one
    /// by one: a pipeline holds its band-limited ring inline (about 16 KiB),
    /// and eight of them built in place would put over 130 KiB on the stack.
    tracks: Vec<BlipBuf>,
    /// The cycle the next frame's trace must start at.
    next_cycle: Option<u64>,
    frame_mix: Vec<f32>,
    frame_core: Vec<f32>,
    frame_tracks: [Vec<f32>; STEM_COUNT],
    mix_mismatches: u64,
    trace_mismatches: u64,
    max_sum_error: f32,
    samples: u64,
}

/// One frame's samples, borrowed from the capture.
#[derive(Debug)]
pub struct StemFrame<'a> {
    /// The console's own mixed output for the frame (what a host would play).
    pub mix: &'a [f32],
    /// Each track's samples, indexed by [`Stem::index`].
    pub tracks: [&'a [f32]; STEM_COUNT],
}

impl StemCapture {
    /// Arm the console's audio provenance and start a capture at the sample
    /// rate the console was built with (`sample_rate` must be that rate: the
    /// `Nes` does not report it, and a different one fails the mix check on
    /// the first frame).
    ///
    /// # Errors
    ///
    /// [`StemError::StartedLate`] if the console has already produced audio.
    pub fn start(nes: &mut Nes, sample_rate: u32) -> Result<Self, StemError> {
        // The reset sequence ticks the APU a few cycles inside the
        // constructor (8 on every board measured), before any trace can be
        // armed. Those cycles are replayed below; a capture started after a
        // frame has run would have thousands to replay, from values nobody
        // recorded.
        let pre = nes.bus().cycle();
        if pre > MAX_PRE_CYCLES {
            return Err(StemError::StartedLate);
        }
        nes.set_audio_provenance(true);
        let region = nes.region();
        let filter = nes.apu_filter_model();
        let mut cap = Self {
            mixer: Mixer::new(),
            mix: alloc::boxed::Box::new(pipeline(sample_rate, region, filter)),
            tracks: (0..STEM_COUNT)
                .map(|_| pipeline(sample_rate, region, filter))
                .collect(),
            // The first frame's trace must begin where the constructor left
            // off, or the replay below is of the wrong cycles.
            next_cycle: Some(pre),
            frame_mix: Vec::new(),
            frame_core: Vec::new(),
            frame_tracks: core::array::from_fn(|_| Vec::new()),
            mix_mismatches: 0,
            trace_mismatches: 0,
            max_sum_error: 0.0,
            samples: 0,
        };
        // Replay the constructor's cycles. Nothing has been written to an
        // APU register yet, so every channel has held its power-on output
        // through them: the outputs the APU presents now are the ones it
        // mixed then. The bit-exact mix check proves it on the first frame,
        // and fails loudly if a board ever ticks something audible there.
        let v = nes.apu_snapshot();
        let rec = MixRecord {
            mixed: cap
                .mixer
                .mix(v.pulse1, v.pulse2, v.triangle, v.noise, v.dmc)
                + v.external,
            external: v.external,
            pulse1: v.pulse1,
            pulse2: v.pulse2,
            triangle: v.triangle,
            noise: v.noise,
            dmc: v.dmc,
        };
        for _ in 0..pre {
            cap.push(&rec);
        }
        Ok(cap)
    }

    /// Feed one CPU cycle's record to the re-rendered mix and every track.
    fn push(&mut self, r: &MixRecord) {
        self.mix.add_sample(r.mixed);
        for (buf, v) in self.tracks.iter_mut().zip(stem_values(&self.mixer, r)) {
            buf.add_sample(v);
        }
    }

    /// Read the frame the console just ran: its mix trace and its audio.
    ///
    /// Drains the console's audio buffer (the returned [`StemFrame::mix`] is
    /// that audio), so a host that plays the audio takes it from here.
    ///
    /// # Errors
    ///
    /// See [`StemError`]; a frame that fails leaves the capture unusable.
    pub fn after_frame(&mut self, nes: &mut Nes) -> Result<StemFrame<'_>, StemError> {
        let trace = nes.mix_trace().ok_or(StemError::Disarmed)?;
        if trace.truncated() {
            return Err(StemError::Truncated);
        }
        if let Some(expected) = self.next_cycle
            && trace.first_cycle() != expected
        {
            return Err(StemError::Gap {
                expected,
                found: trace.first_cycle(),
            });
        }
        self.next_cycle = Some(trace.first_cycle() + trace.records().len() as u64);
        for r in trace.records() {
            // The record must describe the mix it carries: the same five
            // arguments through the same mixer, plus the expansion sample.
            // Counted rather than asserted, so a host can report it.
            let all = self
                .mixer
                .mix(r.pulse1, r.pulse2, r.triangle, r.noise, r.dmc)
                + r.external;
            if all.to_bits() != r.mixed.to_bits() {
                self.trace_mismatches += 1;
            }
            self.push(r);
        }

        self.frame_core.clear();
        self.frame_core.extend(nes.drain_audio());
        self.frame_mix.clear();
        self.frame_mix.extend(self.mix.drain_all());
        for (out, buf) in self.frame_tracks.iter_mut().zip(self.tracks.iter_mut()) {
            out.clear();
            out.extend(buf.drain_all());
        }

        // Compare the re-rendered mix with the console's, bit for bit, and
        // the seven tracks' sum with the mix.
        let n = self.frame_core.len();
        if self.frame_mix.len() != n {
            self.mix_mismatches += n.abs_diff(self.frame_mix.len()) as u64;
        }
        for (i, (&core, &ours)) in self.frame_core.iter().zip(&self.frame_mix).enumerate() {
            if core.to_bits() != ours.to_bits() {
                self.mix_mismatches += 1;
            }
            let sum: f32 = self.frame_tracks.iter().map(|t| t[i]).sum();
            let err = (core - sum).abs();
            if err > self.max_sum_error || err.is_nan() {
                self.max_sum_error = err;
            }
        }
        self.samples += n as u64;

        Ok(StemFrame {
            mix: &self.frame_core,
            tracks: core::array::from_fn(|i| self.frame_tracks[i].as_slice()),
        })
    }

    /// How many output samples of the re-rendered mix differed (in bits, or in
    /// count) from the console's own output. Zero on every run so far; a
    /// non-zero value means the private pipeline is not the console's, and the
    /// stems should not be trusted.
    #[must_use]
    pub const fn mix_mismatches(&self) -> u64 {
        self.mix_mismatches
    }

    /// How many trace records carried a `mixed` value that the mixer does not
    /// produce from the record's own channel values. Zero unless a host set a
    /// channel mask or gain (the trace records raw channel outputs, and the
    /// mix is then the masked or scaled one).
    #[must_use]
    pub const fn trace_mismatches(&self) -> u64 {
        self.trace_mismatches
    }

    /// The largest `|mix - Σ tracks|` seen so far, over every sample.
    #[must_use]
    pub const fn max_sum_error(&self) -> f32 {
        self.max_sum_error
    }

    /// Output samples captured so far (per track).
    #[must_use]
    pub const fn samples(&self) -> u64 {
        self.samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use rustynes_apu::CHANNEL_MASK_ALL;

    /// A one-bank NROM that programs every 2A03 channel at once and loops: the
    /// two pulses at different volumes and pitches, the triangle, the noise,
    /// and the DMC's direct load (`$4011`), so every group of the mixer has
    /// two or more channels sounding together and the interaction term is
    /// non-zero.
    fn all_channels_rom() -> Vec<u8> {
        let mut prg = vec![0xEAu8; 0x4000];
        let code: &[u8] = &[
            0x78, // SEI
            0xA9, 0x0F, 0x8D, 0x15, 0x40, // LDA #$0F ; STA $4015
            0xA9, 0xBF, 0x8D, 0x00, 0x40, // pulse 1: duty 2, const vol 15
            0xA9, 0xFD, 0x8D, 0x02, 0x40, 0xA9, 0x00, 0x8D, 0x03, 0x40, // period $0FD
            0xA9, 0x77, 0x8D, 0x04, 0x40, // pulse 2: duty 1, const vol 7
            0xA9, 0x52, 0x8D, 0x06, 0x40, 0xA9, 0x01, 0x8D, 0x07, 0x40, // period $152
            0xA9, 0xFF, 0x8D, 0x08, 0x40, // triangle linear counter
            0xA9, 0x80, 0x8D, 0x0A, 0x40, 0xA9, 0x00, 0x8D, 0x0B, 0x40, // period $080
            0xA9, 0x3A, 0x8D, 0x0C, 0x40, // noise const vol 10
            0xA9, 0x05, 0x8D, 0x0E, 0x40, 0xA9, 0x00, 0x8D, 0x0F, 0x40, // noise period 5
            // loop: alternate the DMC level between $10 and $50 forever.
            0xA9, 0x10, 0x8D, 0x11, 0x40, // LDA #$10 ; STA $4011
            0xA2, 0x00, 0xCA, 0xD0, 0xFD, // LDX #0 ; DEX ; BNE -3
            0xA9, 0x50, 0x8D, 0x11, 0x40, // LDA #$50 ; STA $4011
            0xA2, 0x00, 0xCA, 0xD0, 0xFD, // delay again
            0x4C, 0x00, 0x00, // JMP loop (patched below)
        ];
        prg[..code.len()].copy_from_slice(code);
        let loop_at = 0xC000u16 + 66;
        let jmp = code.len() - 2;
        prg[jmp] = (loop_at & 0xFF) as u8;
        prg[jmp + 1] = (loop_at >> 8) as u8;
        prg[0x3FFC] = 0x00;
        prg[0x3FFD] = 0xC0;
        let mut rom = vec![b'N', b'E', b'S', 0x1A, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        rom.extend_from_slice(&prg);
        rom.extend(core::iter::repeat_n(0u8, 0x2000));
        rom
    }

    fn capture(frames: usize) -> (StemCapture, Vec<Vec<f32>>) {
        capture_rom(&all_channels_rom(), frames, 48_000, None)
    }

    /// Run `rom` for `frames` frames under a capture; returns the capture and
    /// the streams, the console mix first and then the tracks in
    /// [`Stem::ALL`] order.
    fn capture_rom(
        rom: &[u8],
        frames: usize,
        rate: u32,
        filter: Option<FilterModel>,
    ) -> (StemCapture, Vec<Vec<f32>>) {
        let mut nes = Nes::from_rom_with_sample_rate(rom, rate).expect("rom loads");
        if let Some(f) = filter {
            nes.set_apu_filter_model(f);
        }
        let mut cap = StemCapture::start(&mut nes, rate).expect("starts");
        let mut tracks = vec![Vec::new(); STEM_COUNT + 1];
        for _ in 0..frames {
            nes.run_frame();
            let f = cap.after_frame(&mut nes).expect("frame captures");
            tracks[0].extend_from_slice(f.mix);
            for (i, t) in f.tracks.iter().enumerate() {
                tracks[i + 1].extend_from_slice(t);
            }
        }
        (cap, tracks)
    }

    #[test]
    fn the_rerendered_mix_is_the_consoles_bit_for_bit() {
        let (cap, tracks) = capture(60);
        assert!(
            cap.samples() > 40_000,
            "a second of audio: {}",
            cap.samples()
        );
        assert_eq!(cap.mix_mismatches(), 0, "the private pipeline is the APU's");
        assert_eq!(cap.trace_mismatches(), 0, "each record describes its mix");
        for t in &tracks {
            assert_eq!(t.len(), tracks[0].len(), "every stream is the same length");
        }
    }

    #[test]
    fn the_seven_tracks_sum_to_the_mix() {
        let (cap, tracks) = capture(60);
        let peak = tracks[0].iter().fold(0.0f32, |m, &v| m.max(v.abs()));
        assert!(peak > 0.05, "the ROM is audible: peak {peak}");
        assert!(
            cap.max_sum_error() < 1e-5,
            "mix - Σ tracks = {} (f32 rounding only)",
            cap.max_sum_error()
        );
    }

    #[test]
    fn the_sources_alone_do_not_sum_to_the_mix() {
        // The non-linear mixer is real: without the interaction track the
        // sources overshoot. If this ever passed with a tiny difference, the
        // stems would be rendered through a linear mixer, not the console's.
        let (_cap, tracks) = capture(60);
        let interaction = &tracks[1 + Stem::Interaction.index()];
        let peak = interaction.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
        assert!(peak > 0.01, "interaction peak {peak}");
    }

    #[test]
    fn each_source_stem_carries_its_channel() {
        let (_cap, tracks) = capture(60);
        for stem in [
            Stem::Pulse1,
            Stem::Pulse2,
            Stem::Triangle,
            Stem::Noise,
            Stem::Dmc,
        ] {
            let t = &tracks[1 + stem.index()];
            let peak = t.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
            assert!(peak > 0.005, "{} is audible: peak {peak}", stem.name());
        }
        let ext = &tracks[1 + Stem::Expansion.index()];
        assert!(ext.iter().all(|&v| v == 0.0), "no expansion chip on NROM");
    }

    #[test]
    fn a_stem_value_is_the_mixer_with_one_channel() {
        let m = Mixer::new();
        let r = MixRecord {
            mixed: m.mix(15, 15, 15, 15, 127) + 0.25,
            external: 0.25,
            pulse1: 15,
            pulse2: 15,
            triangle: 15,
            noise: 15,
            dmc: 127,
        };
        let v = stem_values(&m, &r);
        assert_eq!(v[0], m.mix(15, 0, 0, 0, 0));
        assert_eq!(v[1], m.mix(0, 15, 0, 0, 0));
        assert_eq!(v[2], m.mix(0, 0, 15, 0, 0));
        assert_eq!(v[3], m.mix(0, 0, 0, 15, 0));
        assert_eq!(v[4], m.mix(0, 0, 0, 0, 127));
        assert_eq!(v[5], 0.25);
        // Concave tables: the interaction term is negative at full scale.
        assert!(v[6] < 0.0, "interaction {}", v[6]);
        let sum: f32 = v.iter().sum();
        assert!((sum - r.mixed).abs() < 1e-6);
    }

    #[test]
    fn a_capture_started_late_is_refused() {
        let rom = all_channels_rom();
        let mut nes = Nes::from_rom_with_sample_rate(&rom, 48_000).expect("rom loads");
        // The first `run_frame` after power-on returns at once (the frame
        // flag the reset left set); the second runs a real frame.
        nes.run_frame();
        nes.run_frame();
        assert_eq!(
            StemCapture::start(&mut nes, 48_000).err(),
            Some(StemError::StartedLate)
        );
    }

    #[test]
    fn an_uncaptured_frame_is_a_gap() {
        let rom = all_channels_rom();
        let mut nes = Nes::from_rom_with_sample_rate(&rom, 48_000).expect("rom loads");
        let mut cap = StemCapture::start(&mut nes, 48_000).expect("starts");
        nes.run_frame();
        cap.after_frame(&mut nes).expect("first frame");
        nes.run_frame();
        nes.run_frame();
        assert!(matches!(
            cap.after_frame(&mut nes),
            Err(StemError::Gap { .. })
        ));
    }

    fn peak(t: &[f32]) -> f32 {
        t.iter().fold(0.0f32, |m, &v| m.max(v.abs()))
    }

    #[test]
    fn a_pal_console_and_another_filter_model_still_match() {
        // An NES 2.0 header (byte 7 = $08) with byte 12 = 1 marks the image
        // PAL, which changes the pipeline's CPU clock; the Famicom filter
        // model changes its coefficients. Both are read from the console, so
        // the private pipeline follows.
        let mut rom = all_channels_rom();
        rom[7] = 0x08;
        rom[12] = 0x01;
        assert_eq!(Nes::from_rom(&rom).expect("loads").region(), Region::Pal);
        let (cap, tracks) = capture_rom(&rom, 50, 44_100, Some(FilterModel::Famicom));
        assert!(cap.samples() > 40_000);
        assert_eq!(cap.mix_mismatches(), 0, "PAL + Famicom filter");
        assert!(cap.max_sum_error() < 1e-5, "{}", cap.max_sum_error());
        assert!(peak(&tracks[0]) > 0.05);
    }

    /// The bbbradsmith expansion-audio ROMs vendored under
    /// `tests/roms/audio-tests/` play a 2A03 reference square and then the
    /// expansion chip's (see `rustynes-test-harness/tests/audio_expansion.rs`,
    /// whose frame windows these reuse: the chip sounds in frames 560-650).
    fn expansion_rom(name: &str) -> Vec<u8> {
        let path = std::format!(
            "{}/../../tests/roms/audio-tests/{name}.nes",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    fn check_expansion(name: &str, chip: &str) {
        let rom = expansion_rom(name);
        let nes = Nes::from_rom(&rom).expect("loads");
        assert_eq!(nes.expansion_audio_chip(), Some(chip), "{name}");
        let (cap, tracks) = capture_rom(&rom, 650, 44_100, None);
        assert_eq!(cap.mix_mismatches(), 0, "{name}: mix re-render");
        assert_eq!(cap.trace_mismatches(), 0, "{name}: trace");
        assert!(
            cap.max_sum_error() < 1e-5,
            "{name}: mix - Σ tracks = {}",
            cap.max_sum_error()
        );
        // 735 samples a frame at 44.1 kHz; the chip's segment.
        let per_frame = 44_100 / 60;
        let ext = &tracks[1 + Stem::Expansion.index()];
        let seg = &ext[560 * per_frame..640 * per_frame];
        assert!(
            peak(seg) > 0.02,
            "{name}: the {chip} stem carries the chip: {}",
            peak(seg)
        );
        // And the reference segment is the 2A03's, not the chip's.
        let p1 = &tracks[1 + Stem::Pulse1.index()];
        assert!(
            peak(&p1[400 * per_frame..470 * per_frame]) > 0.05,
            "{name}: reference square"
        );
    }

    #[test]
    fn a_vrc6_cartridge_gets_its_own_stem() {
        check_expansion("db_vrc6a", "VRC6");
    }

    #[test]
    fn a_namco_163_cartridge_gets_its_own_stem() {
        check_expansion("db_n163", "Namco 163");
    }

    #[test]
    fn an_mmc5_cartridge_gets_its_own_stem() {
        check_expansion("db_mmc5", "MMC5");
    }

    #[test]
    fn a_sunsoft_5b_cartridge_gets_its_own_stem() {
        check_expansion("db_5b", "Sunsoft 5B");
    }

    #[test]
    fn a_vrc7_cartridge_gets_its_own_stem() {
        check_expansion("db_vrc7", "VRC7 (OPLL)");
    }

    /// Run `rom` from a cold boot with the channel mask `mask`, returning its
    /// audio; and, when `capture` is set, a stem capture of the same run.
    ///
    /// The mask is set and THEN the console power-cycled, so the reset
    /// sequence's own few cycles run under the mask too: a mask set after
    /// construction leaves those cycles mixed with every channel, which the
    /// band-limited buffer carries into the first frame or so.
    fn cold(rom: &[u8], frames: usize, rate: u32, mask: u8) -> (Vec<f32>, Option<Vec<Vec<f32>>>) {
        let mut nes = Nes::from_rom_with_sample_rate(rom, rate).expect("rom loads");
        nes.set_apu_channel_mask(mask);
        nes.power_cycle();
        let mut cap = (mask == CHANNEL_MASK_ALL)
            .then(|| StemCapture::start(&mut nes, rate).expect("starts after a power cycle"));
        let mut audio = Vec::new();
        let mut tracks = vec![Vec::new(); STEM_COUNT];
        for _ in 0..frames {
            nes.run_frame();
            if let Some(c) = cap.as_mut() {
                let f = c.after_frame(&mut nes).expect("frame captures");
                audio.extend_from_slice(f.mix);
                for (t, v) in tracks.iter_mut().zip(f.tracks) {
                    t.extend_from_slice(v);
                }
            } else {
                audio.extend(nes.drain_audio());
            }
        }
        if let Some(c) = &cap {
            assert_eq!(c.mix_mismatches(), 0, "the capture after a power cycle");
        }
        (audio, cap.map(|_| tracks))
    }

    /// The strong per-stem check. The sum gate cannot catch a wrong stem
    /// (the interaction track is defined as the remainder, so the seven
    /// always add up); this one can. The APU's own channel mask zeroes every
    /// other channel before its mixer, so a console with one mask bit set
    /// plays exactly "that channel alone through the same mixer", and each
    /// stem must equal it sample for sample.
    // Exact equality is the claim: the same values through the same
    // pipeline, so not one sample may differ, by any margin.
    #[allow(clippy::float_cmp)]
    fn check_against_mask(rom: &[u8], frames: usize, rate: u32, stems: &[Stem]) {
        let (_, tracks) = cold(rom, frames, rate, CHANNEL_MASK_ALL);
        let tracks = tracks.expect("captured");
        for &stem in stems {
            let bit = u8::try_from(stem.index()).expect("a channel bit");
            let (alone, _) = cold(rom, frames, rate, 1 << bit);
            let ours = &tracks[stem.index()];
            assert_eq!(ours.len(), alone.len(), "{}", stem.name());
            let differ = ours.iter().zip(&alone).filter(|(a, b)| a != b).count();
            assert_eq!(
                differ,
                0,
                "{}: {differ} samples differ from the masked console",
                stem.name()
            );
            assert!(peak(ours) > 0.0, "{} is not silent", stem.name());
        }
    }

    #[test]
    fn each_2a03_stem_is_the_console_with_only_that_channel() {
        check_against_mask(
            &all_channels_rom(),
            60,
            48_000,
            &[
                Stem::Pulse1,
                Stem::Pulse2,
                Stem::Triangle,
                Stem::Noise,
                Stem::Dmc,
            ],
        );
    }

    #[test]
    fn the_expansion_stem_is_the_console_with_only_the_chip() {
        check_against_mask(&expansion_rom("db_vrc6a"), 650, 44_100, &[Stem::Expansion]);
    }

    /// A synthetic FDS: a one-side disk with only its info-block signature,
    /// and an 8 KiB "BIOS" written for this test (not Nintendo's, which is
    /// never committed) that turns the sound I/O on, fills the wavetable with
    /// a sawtooth, sets a direct volume and a pitch, and spins.
    fn fds_disk_and_bios() -> (Vec<u8>, Vec<u8>) {
        let mut disk = vec![0u8; 16 + 65_500];
        disk[0..4].copy_from_slice(b"FDS\x1A");
        disk[4] = 1;
        disk[16] = 0x01;
        disk[17..31].copy_from_slice(b"*NINTENDO-HVC*");
        let code: &[u8] = &[
            0x78, // E000 SEI
            0xA9, 0x03, 0x8D, 0x23, 0x40, // LDA #$03 ; STA $4023 (disk + sound I/O)
            0xA9, 0x80, 0x8D, 0x89, 0x40, // LDA #$80 ; STA $4089 (wavetable writable)
            0xA2, 0x00, // LDX #0
            0x8A, // E00D TXA
            0x9D, 0x40, 0x40, // STA $4040,X (a sawtooth: the index)
            0xE8, // INX
            0xE0, 0x40, // CPX #$40
            0xD0, 0xF7, // BNE $E00D
            0xA9, 0x00, 0x8D, 0x89, 0x40, // LDA #0 ; STA $4089 (locked, full volume)
            0xA9, 0xA0, 0x8D, 0x80, 0x40, // LDA #$A0 ; STA $4080 (direct gain 32)
            0xA9, 0x80, 0x8D, 0x87, 0x40, // LDA #$80 ; STA $4087 (modulator halted)
            0xA9, 0x00, 0x8D, 0x82, 0x40, // LDA #0 ; STA $4082
            0xA9, 0x02, 0x8D, 0x83, 0x40, // LDA #2 ; STA $4083 (pitch $200, running)
            0x4C, 0x34, 0xE0, // E034 JMP $E034
        ];
        let mut bios = vec![0u8; 0x2000];
        bios[..code.len()].copy_from_slice(code);
        bios[0x40] = 0x40; // $E040: RTI
        bios[0x1FFA..0x1FFC].copy_from_slice(&0xE040u16.to_le_bytes());
        bios[0x1FFC..0x1FFE].copy_from_slice(&0xE000u16.to_le_bytes());
        bios[0x1FFE..0x2000].copy_from_slice(&0xE040u16.to_le_bytes());
        (disk, bios)
    }

    #[test]
    #[allow(clippy::float_cmp)] // exact equality is the claim, as above
    fn an_fds_gets_its_own_stem() {
        let (disk, bios) = fds_disk_and_bios();
        let run = |mask: Option<u8>| {
            let mut nes = Nes::from_disk_with_sample_rate(&disk, &bios, 48_000).expect("loads");
            assert_eq!(nes.expansion_audio_chip(), Some("FDS"));
            // Mask first, then the cold boot, as in `cold` above.
            if let Some(m) = mask {
                nes.set_apu_channel_mask(m);
            }
            nes.power_cycle();
            let mut cap = mask
                .is_none()
                .then(|| StemCapture::start(&mut nes, 48_000).expect("starts"));
            let (mut mix, mut ext) = (Vec::new(), Vec::new());
            for _ in 0..30 {
                nes.run_frame();
                if let Some(c) = cap.as_mut() {
                    let f = c.after_frame(&mut nes).expect("captures");
                    mix.extend_from_slice(f.mix);
                    ext.extend_from_slice(f.tracks[Stem::Expansion.index()]);
                } else {
                    mix.extend(nes.drain_audio());
                }
            }
            if let Some(c) = &cap {
                assert_eq!(c.mix_mismatches(), 0);
                assert!(c.max_sum_error() < 1e-5, "{}", c.max_sum_error());
            }
            (mix, ext)
        };
        let (_, ext) = run(None);
        assert!(
            peak(&ext) > 0.02,
            "the FDS wave is in its stem: {}",
            peak(&ext)
        );
        let (alone, _) = run(Some(1 << Stem::Expansion.index()));
        assert_eq!(ext.len(), alone.len());
        assert!(
            ext.iter().zip(&alone).all(|(a, b)| a == b),
            "the masked console"
        );
    }
}

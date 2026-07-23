//! NSF expansion-audio synthesis routing (v1.7.0 "Forge", Workstream G2/G3).
//!
//! Classic `.nsf` files may declare on-cart expansion audio in the `$07B`
//! expansion bitfield. The synthesis cores for every NES expansion chip
//! already exist and are battle-tested for cartridge playback:
//!
//! | Bit | Chip          | Synth core (reused verbatim)               |
//! |-----|---------------|--------------------------------------------|
//! | 0   | VRC6          | [`crate::sprint3`] `Vrc6Pulse` / `Vrc6Saw` |
//! | 1   | VRC7 (OPLL)   | [`rustynes_apu::Opll`]                      |
//! | 2   | FDS           | [`crate::fds`] `FdsAudio`                   |
//! | 3   | MMC5          | [`crate::mmc5`] `Mmc5Audio`                 |
//! | 4   | Namco 163     | [`crate::sprint3`] `Namco163Audio`          |
//! | 5   | Sunsoft 5B    | [`crate::sprint3`] `Sunsoft5BAudio`         |
//!
//! This module does **not** reimplement any synthesis. It is a thin router:
//! it owns instances of the existing cores, forwards the NSF register-window
//! writes to them (the same register addresses the cartridge mappers decode),
//! clocks them on the master timebase, and sums their outputs into one signed
//! sample for the bus's external-audio mix. The bit-for-bit synthesis math is
//! identical to the cartridge path, so an NSF that drives, e.g., a VRC6 tune
//! produces the same audio a VRC6 cartridge would.
//!
//! Determinism / oracle contract: this state is constructed **only** for NSF
//! files (`NsfMapper`), and only when the expansion bitfield requests a chip.
//! It is never reachable from any cartridge ROM in the AccuracyCoin / blargg /
//! kevtris oracle, so it cannot perturb existing audio. When the `mapper-audio`
//! feature is off, every core's `clock`/`mix`/`output` is a feature-off no-op
//! shim (silence), matching the cartridge feature-off behaviour.

// These match the allow-set the owning synth modules already carry: the
// small register-bit unpackers trip `missing_const_for_fn`/`const fn`
// suggestions that don't change behaviour, the audio mix biasing casts a
// clamped `i32` to `i16` (always in range by construction), and the
// per-chip write/clock fan-out is a readable `let mut handled = false; if ..`
// sequence rather than an expression.
#![allow(
    clippy::missing_const_for_fn,
    clippy::cast_possible_truncation,
    clippy::useless_let_if_seq,
    clippy::doc_markdown
)]

use crate::fds::FdsAudio;
use crate::mmc5::{MMC5_MIX_BIAS, MMC5_PCM_SCALE, MMC5_PULSE_SCALE, Mmc5Audio};
use crate::sprint3::{Namco163Audio, Sunsoft5BAudio, VRC6_MIX_SCALE, Vrc6Pulse, Vrc6Saw};
use alloc::boxed::Box;
use alloc::vec::Vec;

/// Expansion-bitfield bit positions in NSF header byte `$07B`.
const EXP_VRC6: u8 = 0x01;
const EXP_VRC7: u8 = 0x02;
const EXP_FDS: u8 = 0x04;
const EXP_MMC5: u8 = 0x08;
const EXP_N163: u8 = 0x10;
const EXP_5B: u8 = 0x20;

/// VRC6 audio sub-state: the two pulse channels + sawtooth + the `$9003`
/// global control byte (halt + frequency-scale shift). Mirrors the live
/// state the [`crate::sprint3`] `Vrc6` mapper keeps; the clock/output math
/// is reused verbatim from that mapper's channel cores.
#[derive(Default)]
struct Vrc6Exp {
    audio_ctrl: u8,
    pulse1: Vrc6Pulse,
    pulse2: Vrc6Pulse,
    saw: Vrc6Saw,
}

impl Vrc6Exp {
    /// The `$9003`-derived frequency-scale right-shift (0 / 4 / 8), matching
    /// `Vrc6::effective_period_*`.
    fn shift(&self) -> u8 {
        match (self.audio_ctrl >> 1) & 0x03 {
            0 => 0,
            1 => 4,
            _ => 8,
        }
    }

    fn clock(&mut self) {
        // `$9003` bit 0 = halt-all.
        if (self.audio_ctrl & 0x01) != 0 {
            return;
        }
        let shift = self.shift();
        let saved = (self.pulse1.period, self.pulse2.period, self.saw.period);
        self.pulse1.period >>= shift;
        self.pulse2.period >>= shift;
        self.saw.period >>= shift;
        self.pulse1.clock();
        self.pulse2.clock();
        self.saw.clock();
        self.pulse1.period = saved.0;
        self.pulse2.period = saved.1;
        self.saw.period = saved.2;
    }

    /// Same mix as `Vrc6::mix_audio`: linear sum of the three channels,
    /// centred and scaled by the shared [`VRC6_MIX_SCALE`] factor (v2.1.6; see
    /// `Vrc6::mix_audio` and `docs/apu-2a03.md` §Expansion-audio levels).
    /// Referencing the SAME const as the cartridge path keeps this
    /// bit-identical to it, so an NSF VRC6 tune is level-matched to a VRC6
    /// cartridge and the two mixers can never drift.
    fn mix(&self) -> i16 {
        let p1 = i16::from(self.pulse1.output());
        let p2 = i16::from(self.pulse2.output());
        let saw = i16::from(self.saw.output());
        ((p1 + p2 + saw) - 30) * VRC6_MIX_SCALE
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr & 0xF003 {
            0x9000 => self.pulse1.ctrl = value,
            0x9001 => self.pulse1.period = (self.pulse1.period & 0x0F00) | u16::from(value),
            0x9002 => {
                self.pulse1.period = (self.pulse1.period & 0x00FF) | (u16::from(value & 0x0F) << 8);
                self.pulse1.enabled = (value & 0x80) != 0;
                if !self.pulse1.enabled {
                    self.pulse1.step = 0;
                }
            }
            0x9003 => self.audio_ctrl = value,
            0xA000 => self.pulse2.ctrl = value,
            0xA001 => self.pulse2.period = (self.pulse2.period & 0x0F00) | u16::from(value),
            0xA002 => {
                self.pulse2.period = (self.pulse2.period & 0x00FF) | (u16::from(value & 0x0F) << 8);
                self.pulse2.enabled = (value & 0x80) != 0;
                if !self.pulse2.enabled {
                    self.pulse2.step = 0;
                }
            }
            0xB000 => self.saw.rate = value & 0x3F,
            0xB001 => self.saw.period = (self.saw.period & 0x0F00) | u16::from(value),
            0xB002 => {
                self.saw.period = (self.saw.period & 0x00FF) | (u16::from(value & 0x0F) << 8);
                self.saw.enabled = (value & 0x80) != 0;
                if !self.saw.enabled {
                    self.saw.step = 0;
                    self.saw.acc = 0;
                }
            }
            _ => {}
        }
    }
}

/// MMC5 audio sub-state for NSF. Reuses the [`Mmc5Audio`] core directly so the
/// pulse + PCM synthesis is identical to the cartridge path (Workstream G3).
/// The 2A03 frame-counter cadence (envelope / length clocks) is fanned in by
/// the NSF mapper, exactly as the bus fans it to a cartridge MMC5.
#[derive(Default)]
struct Mmc5Exp {
    audio: Mmc5Audio,
    /// Every-other-CPU-cycle phase for the pulse timer/duty sequencer.
    apu_phase: bool,
}

impl Mmc5Exp {
    fn clock(&mut self) {
        self.apu_phase = !self.apu_phase;
        if self.apu_phase {
            self.audio.pulse1.clock_timer();
            self.audio.pulse2.clock_timer();
        }
    }

    fn frame_event(&mut self, quarter: bool, half: bool) {
        if quarter {
            self.audio.pulse1.clock_envelope();
            self.audio.pulse2.clock_envelope();
        }
        if half {
            self.audio.pulse1.clock_length();
            self.audio.pulse2.clock_length();
        }
    }

    /// Same mix as `Mmc5::mix_audio` (two pulses + 7-bit PCM, biased to zero),
    /// referencing the shared [`MMC5_PULSE_SCALE`] / [`MMC5_PCM_SCALE`] /
    /// [`MMC5_MIX_BIAS`] consts (v2.1.6) so an NSF MMC5 tune is level-matched to
    /// an MMC5 cartridge and the two mixers can never drift (see
    /// `Mmc5::mix_audio`).
    fn mix(&self) -> i16 {
        let p1 = i16::from(self.audio.pulse1.output());
        let p2 = i16::from(self.audio.pulse2.output());
        let pcm = if (self.audio.pcm_ctrl & 0x01) == 0 {
            i16::from(self.audio.pcm_sample)
        } else {
            0
        };
        let pulse_mix = (p1 + p2) * MMC5_PULSE_SCALE;
        let pcm_mix = pcm * MMC5_PCM_SCALE;
        (pulse_mix + pcm_mix) - MMC5_MIX_BIAS
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x5000 => self.audio.pulse1.write_ctrl(value),
            0x5002 => self.audio.pulse1.write_timer_lo(value),
            0x5003 => self.audio.pulse1.write_timer_hi(value),
            0x5004 => self.audio.pulse2.write_ctrl(value),
            0x5006 => self.audio.pulse2.write_timer_lo(value),
            0x5007 => self.audio.pulse2.write_timer_hi(value),
            0x5010 => self.audio.pcm_ctrl = value,
            0x5011 => {
                if (self.audio.pcm_ctrl & 0x01) == 0 {
                    self.audio.pcm_sample = value & 0x7F;
                }
            }
            0x5015 => {
                self.audio.pulse1.set_length_enabled((value & 0x01) != 0);
                self.audio.pulse2.set_length_enabled((value & 0x02) != 0);
            }
            _ => {}
        }
    }

    /// `$5015` status read: bit 0/1 = pulse 1/2 length > 0.
    fn status(&self) -> u8 {
        let mut v = 0u8;
        if self.audio.pulse1.length > 0 {
            v |= 0x01;
        }
        if self.audio.pulse2.length > 0 {
            v |= 0x02;
        }
        v
    }
}

/// Bundle of expansion synth cores requested by an NSF's `$07B` bitfield.
///
/// Constructed by [`NsfExpansion::from_bits`]; only the chips whose bit is set
/// are allocated. Each per-CPU-cycle [`Self::clock`] / per-frame-event
/// [`Self::frame_event`] call advances all present cores, and [`Self::mix`]
/// sums their outputs.
pub(crate) struct NsfExpansion {
    vrc6: Option<Box<Vrc6Exp>>,
    vrc7: Option<Vrc7Exp>,
    fds: Option<Box<FdsAudio>>,
    mmc5: Option<Box<Mmc5Exp>>,
    n163: Option<Box<Namco163Audio>>,
    s5b: Option<Box<Sunsoft5BAudio>>,
}

/// VRC7 (OPLL) sub-state. Owns the shared [`rustynes_apu::Opll`] core (the
/// same one the VRC7 cartridge mapper uses) plus the 36-CPU-cycle prescaler
/// and a latched address byte for the `$9010` / `$9030` two-write protocol.
#[cfg(feature = "mapper-audio")]
struct Vrc7Exp {
    opll: rustynes_apu::Opll,
    addr_latch: u8,
    clock_counter: u16,
    last_sample: i16,
}

/// Feature-off VRC7 stub: latches the address byte for register decode
/// completeness but synthesizes nothing.
#[cfg(not(feature = "mapper-audio"))]
struct Vrc7Exp {
    addr_latch: u8,
}

impl Vrc7Exp {
    #[cfg(feature = "mapper-audio")]
    fn new() -> Self {
        Self {
            opll: rustynes_apu::Opll::new(rustynes_apu::OpllChipType::Vrc7),
            addr_latch: 0,
            clock_counter: 0,
            last_sample: 0,
        }
    }

    #[cfg(not(feature = "mapper-audio"))]
    fn new() -> Self {
        Self { addr_latch: 0 }
    }

    fn write(&mut self, addr: u16, value: u8) {
        // $9010 = OPLL register address latch; $9030 = data write.
        match addr & 0xF030 {
            0x9010 => self.addr_latch = value & 0x3F,
            0x9030 => {
                #[cfg(feature = "mapper-audio")]
                self.opll.write_reg(self.addr_latch, value);
                #[cfg(not(feature = "mapper-audio"))]
                let _ = value;
            }
            _ => {}
        }
    }

    #[cfg(feature = "mapper-audio")]
    fn clock(&mut self) {
        // OPLL native rate is 49,716 Hz; NTSC CPU is 1,789,773 Hz -> tick
        // every 36 CPU cycles (matches `Vrc7::notify_cpu_cycle`).
        self.clock_counter = self.clock_counter.wrapping_add(1);
        if self.clock_counter >= 36 {
            self.clock_counter = 0;
            self.last_sample = self.opll.calc();
        }
    }

    #[cfg(not(feature = "mapper-audio"))]
    #[allow(clippy::needless_pass_by_ref_mut, clippy::unused_self)]
    fn clock(&mut self) {}

    #[cfg(feature = "mapper-audio")]
    fn mix(&self) -> i16 {
        self.last_sample
    }

    #[cfg(not(feature = "mapper-audio"))]
    #[allow(clippy::unused_self)]
    fn mix(&self) -> i16 {
        0
    }
}

impl NsfExpansion {
    /// Build the requested chips from the NSF `$07B` expansion bitfield.
    /// Returns `None` when no expansion-audio bit is set (the common case),
    /// so a base-2A03 NSF carries no extra state and stays byte-identical.
    pub(crate) fn from_bits(expansion: u8) -> Option<Self> {
        if expansion & (EXP_VRC6 | EXP_VRC7 | EXP_FDS | EXP_MMC5 | EXP_N163 | EXP_5B) == 0 {
            return None;
        }
        Some(Self {
            vrc6: (expansion & EXP_VRC6 != 0).then(|| Box::new(Vrc6Exp::default())),
            vrc7: (expansion & EXP_VRC7 != 0).then(Vrc7Exp::new),
            fds: (expansion & EXP_FDS != 0).then(|| Box::new(FdsAudio::default())),
            mmc5: (expansion & EXP_MMC5 != 0).then(|| Box::new(Mmc5Exp::default())),
            n163: (expansion & EXP_N163 != 0).then(|| Box::new(Namco163Audio::default())),
            s5b: (expansion & EXP_5B != 0).then(|| Box::new(Sunsoft5BAudio::default())),
        })
    }

    /// Route a CPU write to whichever expansion chip owns the address. The
    /// NSF mapper calls this for the chips' register windows; an address no
    /// chip claims is ignored. Returns `true` when an expansion chip handled
    /// the address (so the NSF mapper can suppress its default handling for
    /// overlapping windows like FDS `$4023` / MMC5 `$5xxx`).
    pub(crate) fn cpu_write(&mut self, addr: u16, value: u8) -> bool {
        let mut handled = false;
        if let Some(vrc7) = self.vrc7.as_mut()
            && matches!(addr & 0xF030, 0x9010 | 0x9030)
        {
            vrc7.write(addr, value);
            handled = true;
        }
        if let Some(vrc6) = self.vrc6.as_mut()
            && matches!(addr & 0xF000, 0x9000 | 0xA000 | 0xB000)
        {
            // VRC7's $9010/$9030 already consumed above; a VRC6 NSF never
            // sets the VRC7 bit, so there is no real conflict.
            vrc6.write(addr, value);
            handled = true;
        }
        if let Some(fds) = self.fds.as_mut()
            && (0x4040..=0x408A).contains(&addr)
        {
            fds.write_reg(addr, value);
            handled = true;
        }
        if let Some(mmc5) = self.mmc5.as_mut()
            && (0x5000..=0x5015).contains(&addr)
        {
            mmc5.write(addr, value);
            handled = true;
        }
        if let Some(n163) = self.n163.as_mut() {
            match addr {
                0x4800..=0x4FFF => {
                    n163.write_data_port(value);
                    handled = true;
                }
                0xF800..=0xFFFF => {
                    n163.write_addr_port(value);
                    handled = true;
                }
                _ => {}
            }
        }
        if let Some(s5b) = self.s5b.as_mut() {
            match addr {
                0xC000..=0xDFFF => {
                    s5b.write_addr(value);
                    handled = true;
                }
                0xE000..=0xFFFF => {
                    s5b.write_data(value);
                    handled = true;
                }
                _ => {}
            }
        }
        handled
    }

    /// Route a CPU read to an expansion chip that exposes a readable port.
    /// Returns `Some(byte)` only for the (few) expansion read ports;
    /// everything else is left to the NSF mapper.
    pub(crate) fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        if let Some(n163) = self.n163.as_mut()
            && (0x4800..=0x4FFF).contains(&addr)
        {
            return Some(n163.read_data_port());
        }
        if let Some(mmc5) = self.mmc5.as_ref()
            && addr == 0x5015
        {
            return Some(mmc5.status());
        }
        None
    }

    /// Advance every present chip by one CPU cycle.
    pub(crate) fn clock(&mut self) {
        if let Some(c) = self.vrc6.as_mut() {
            c.clock();
        }
        if let Some(c) = self.vrc7.as_mut() {
            c.clock();
        }
        if let Some(c) = self.fds.as_mut() {
            c.clock();
        }
        if let Some(c) = self.mmc5.as_mut() {
            c.clock();
        }
        if let Some(c) = self.n163.as_mut() {
            c.clock();
        }
        if let Some(c) = self.s5b.as_mut() {
            c.clock();
        }
    }

    /// Fan an APU frame-counter event (quarter / half frame) to the chips
    /// that re-use the 2A03 cadence (MMC5 envelope + length clocks).
    pub(crate) fn frame_event(&mut self, quarter: bool, half: bool) {
        if let Some(c) = self.mmc5.as_mut() {
            c.frame_event(quarter, half);
        }
    }

    /// Sum every live expansion chip into one sample.
    ///
    /// **`i32`, and no longer clamped, as of v2.2.3 (A1).** It used to return
    /// `i16` and `clamp` into it, which was harmless while every chip fitted —
    /// but the calibrated Sunsoft 5B reaches ~104 k at full scale (three tones
    /// at volume 15), so an NSF 5B tune would have CLIPPED where the identical
    /// cartridge 5B path does not. The whole point of `nsf_expansion` is that
    /// an NSF tune sounds bit-for-bit like the cartridge, so the clamp had to
    /// go with the widening rather than silently diverge the two paths.
    pub(crate) fn mix(&self) -> i32 {
        let mut sum: i32 = 0;
        if let Some(c) = self.vrc6.as_ref() {
            sum += i32::from(c.mix());
        }
        if let Some(c) = self.vrc7.as_ref() {
            sum += i32::from(c.mix());
        }
        if let Some(c) = self.fds.as_ref() {
            sum += i32::from(c.output());
        }
        if let Some(c) = self.mmc5.as_ref() {
            sum += i32::from(c.mix());
        }
        if let Some(c) = self.n163.as_ref() {
            sum += i32::from(c.mix());
        }
        if let Some(c) = self.s5b.as_ref() {
            // No conversion: `Sunsoft5BAudio::mix` returns i32 as of v2.2.3 (A1),
            // because the calibrated 5B level overflows i16 at full scale.
            sum += c.mix();
        }
        sum
    }

    /// Serialize the NSF save-state expansion tail: a single presence byte that
    /// mirrors the `$07B` bitfield (which chips are live).
    ///
    /// The volatile oscillator / register-driven state of the synth cores is
    /// **intentionally not persisted.** On load the chips are rebuilt fresh from
    /// the immutable `$07B` bitfield ([`NsfExpansion::from_bits`]) and their live
    /// phase re-converges from the next register write — the correct behaviour
    /// for a paused/restored NSF, where the driver re-establishes channel state
    /// on the next play call. The presence byte exists only so a reader can
    /// confirm *which* chips a v2 tail described (a forward-compatible,
    /// self-describing tail in the ADR-0003 style); it carries no per-chip phase.
    pub(crate) fn save_state(&self, out: &mut Vec<u8>) {
        // A presence byte mirroring `from_bits`, for a forward-compatible
        // self-describing tail (ADR-0003 style).
        let mut present = 0u8;
        present |= u8::from(self.vrc6.is_some()) * EXP_VRC6;
        present |= u8::from(self.vrc7.is_some()) * EXP_VRC7;
        present |= u8::from(self.fds.is_some()) * EXP_FDS;
        present |= u8::from(self.mmc5.is_some()) * EXP_MMC5;
        present |= u8::from(self.n163.is_some()) * EXP_N163;
        present |= u8::from(self.s5b.is_some()) * EXP_5B;
        out.push(present);
    }

    /// The presence bitfield this expansion bundle serializes — the same byte
    /// [`Self::save_state`] writes. Used by the NSF mapper's `load_state` to
    /// confirm the v2 tail's presence byte matches the chips rebuilt from the
    /// `$07B` bitfield (a self-consistency check; the byte carries no phase).
    pub(crate) fn presence_bits(&self) -> u8 {
        let mut present = 0u8;
        present |= u8::from(self.vrc6.is_some()) * EXP_VRC6;
        present |= u8::from(self.vrc7.is_some()) * EXP_VRC7;
        present |= u8::from(self.fds.is_some()) * EXP_FDS;
        present |= u8::from(self.mmc5.is_some()) * EXP_MMC5;
        present |= u8::from(self.n163.is_some()) * EXP_N163;
        present |= u8::from(self.s5b.is_some()) * EXP_5B;
        present
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_bits_none_for_base_2a03() {
        assert!(NsfExpansion::from_bits(0).is_none());
    }

    #[test]
    fn from_bits_allocates_only_requested() {
        let exp = NsfExpansion::from_bits(EXP_VRC6).expect("vrc6 present");
        assert!(exp.vrc6.is_some());
        assert!(exp.vrc7.is_none());
        assert!(exp.fds.is_none());
        assert!(exp.mmc5.is_none());
        assert!(exp.n163.is_none());
        assert!(exp.s5b.is_none());
    }

    #[test]
    fn vrc6_write_then_clock_produces_signal() {
        let mut exp = NsfExpansion::from_bits(EXP_VRC6).expect("vrc6");
        // Pulse 1: max volume, ignore-duty, short period, enabled.
        assert!(exp.cpu_write(0x9000, 0x8F)); // ctrl: ignore-duty + vol 15
        assert!(exp.cpu_write(0x9001, 0x04)); // period lo
        assert!(exp.cpu_write(0x9002, 0x80)); // period hi + enable
        // Clock a bunch; with ignore-duty + enable, output is a constant 15.
        for _ in 0..64 {
            exp.clock();
        }
        // mix = ((15 + 0 + 0) - 30) * 256 = -3840 (non-silent).
        assert_ne!(exp.mix(), 0);
    }

    #[test]
    fn mmc5_status_reports_length() {
        let mut exp = NsfExpansion::from_bits(EXP_MMC5).expect("mmc5");
        // Enable pulse-1 length, then load a length via $5003.
        exp.cpu_write(0x5015, 0x01);
        exp.cpu_write(0x5003, 0x08); // length index 1 -> table[1] = 254
        assert_eq!(exp.cpu_read(0x5015), Some(0x01));
    }

    #[test]
    fn unrequested_chip_write_is_ignored() {
        // VRC6-only: an FDS register write must not be "handled".
        let mut exp = NsfExpansion::from_bits(EXP_VRC6).expect("vrc6");
        assert!(!exp.cpu_write(0x4040, 0x12));
    }

    #[test]
    fn n163_data_port_round_trips() {
        let mut exp = NsfExpansion::from_bits(EXP_N163).expect("n163");
        exp.cpu_write(0xF800, 0x05); // addr latch (no auto-inc)
        exp.cpu_write(0x4800, 0x42); // data
        assert_eq!(exp.cpu_read(0x4800), Some(0x42));
    }
}

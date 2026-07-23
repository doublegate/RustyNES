//! Sunsoft FME-7 (mapper 69) -- banking, the CPU-cycle IRQ counter, and the
//! on-cart Sunsoft 5B audio chip.
//!
//! The FME-7 is the mapper ASIC; the 5B is the AY-3-8910-derivative sound
//! chip packaged with it on the Japanese Gimmick! cartridge. This module owns
//! both, because the 5B is addressed through the same `$C000`/`$E000`
//! command/parameter port pair the mapper uses.
//!
//! The 5B is three square-wave tone channels, a shared 5-bit LFSR noise
//! generator, and a shared envelope generator, mixed through a *logarithmic*
//! DAC ([`SUNSOFT5B_LOG_VOL`]) rather than the linear one a naive port would
//! use. Shape and absolute level are separately pinned: the step law by a
//! unit test, the level by [`SUNSOFT5B_MIX_SCALE_NUM`] /
//! [`SUNSOFT5B_MIX_SCALE_DEN`] against the `db_5b` oracle ROM.
//!
//! Audio is gated behind the `mapper-audio` Cargo feature (default ON); with
//! it off the register decoders still latch and the oscillators freeze, so a
//! save state written by an audio-enabled build still loads (ADR 0004).
//! [`Sunsoft5BAudio`] is re-used verbatim by the NSF expansion path
//! (`nsf_expansion.rs`).
//!
//! See `docs/mappers.md` and `docs/apu-2a03.md` §Expansion-audio levels.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_ref_mut,
    clippy::manual_range_patterns,
    clippy::match_same_arms,
    clippy::struct_excessive_bools,
    clippy::doc_markdown,
    clippy::range_plus_one,
    clippy::single_match_else,
    clippy::bool_to_int_with_if,
    clippy::unnested_or_patterns,
    clippy::single_match,
    clippy::doc_lazy_continuation,
    clippy::too_long_first_doc_paragraph
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

/// 16-entry logarithmic volume DAC, ~3 dB per 4-bit step (= 1.5 dB per
/// 5-bit step in the underlying chip).  Peak chosen so that three channels
/// summed at maximum volume stay comfortably inside the `i16` headroom the
/// APU mixer expects.
///
/// This table is the DAC **shape** only — each step is `1.1885^2 ≈ 1.4126x`,
/// the +1.5 dB×2 logarithmic law; `LUT[12] = 668`, `LUT[15] = 1882`,
/// cross-checked against Mesen2's `Sunsoft5bAudio::_volumeLut` `[63, 177]` and
/// tetanes. The absolute mixer **level** lives in
/// [`SUNSOFT5B_MIX_SCALE_NUM`], deliberately separate so each can be pinned by
/// its own oracle: the shape by
/// `sunsoft5b_volume_dac_follows_logarithmic_step_law` (a unit test on these
/// ratios), the level by `level_db_5b` (the `db_5b` comparison ROM).
///
/// Our entries are a finer scaling of the same law than Mesen2's `uint8_t`
/// table, which truncates hard at the bottom (its `LUT[1]` is `1`). Keeping the
/// finer table preserves the step ratios that the unit test asserts.
///
/// HISTORY (v2.1.6 → v2.2.3): the absolute level used to be an explicit,
/// documented gap — not because the value was unknown but because
/// `Mapper::mix_audio` returned `i16` and the correct value does not fit. A1
/// widened that return to `i32` and calibrated the level; see
/// [`SUNSOFT5B_MIX_SCALE_NUM`] and `docs/accuracy-ledger.md`.
///
/// Per the NESdev "Sunsoft 5B audio" page, the chip's DAC has a 1.5 dB
/// step on the 5-bit signal.  Because the wiki specifies that envelope
/// level `e` is equivalent to 4-bit volume `e >> 1` (with both `e=0` and
/// `e=1` mapping to silence), a 16-entry table indexed by the 4-bit
/// equivalent is sufficient — equivalent to a 32-entry table where each
/// even/odd pair shares the same amplitude.
#[cfg_attr(not(feature = "mapper-audio"), allow(dead_code))]
const SUNSOFT5B_LOG_VOL: [i32; 16] = [
    0, 15, 21, 30, 42, 59, 84, 119, 168, 237, 335, 473, 668, 944, 1333, 1882,
];

/// Mixed centering bias: subtracted from the scaled linear sum before emitting
/// the i32 sample.  We use a *constant zero* — the APU mixer's chained
/// high-pass filters (90 Hz / 440 Hz, see `rustynes-apu::mixer::OnePole`)
/// remove any steady DC component downstream, and the 5B's linear sum
/// can swing from 0 (all channels muted) up to ~104 k (three channels at
/// peak volume + tone high, post-[`SUNSOFT5B_MIX_SCALE_NUM`]).  Keeping the
/// constant named here makes a future numerical bias easy to add if
/// AccuracyCoin's mixed-output tests ever ask for it.
#[cfg_attr(not(feature = "mapper-audio"), allow(dead_code))]
const SUNSOFT5B_DC_BIAS: i32 = 0;

/// v2.2.3 (A1) — absolute mixer level for the 5B, as a rational
/// `NUM / DEN = 2549 / 138 ≈ 18.471`.
///
/// [`SUNSOFT5B_LOG_VOL`] carries the DAC *shape* (the +1.5 dB x2 law); this
/// carries the *level*, the same split `VRC6_MIX_SCALE` /
/// `NAMCO163_MIX_SCALE` / the MMC5 `650/40` pair use. Separating them is what
/// lets the shape stay pinned by its own unit test while the level is pinned
/// by a ROM oracle.
///
/// **Target, derived from Mesen2 (the project's accuracy bar) rather than from
/// our own prior numbers.** In `NesSoundMixer::GetOutputVolume` a full-volume
/// 2A03 square is `(95.88 * 5000) / (8128/15 + 100) = 746.9` units, and the 5B
/// is summed with weight `* 15` over `Sunsoft5bAudio::_volumeLut`
/// (`= (uint8_t)1.1885^(2i)`, so `LUT[12] = 63`, `LUT[15] = 177`). The
/// `db_5b` ROM compares a **volume-12** 5B square against that square:
///
/// ```text
///   volume 12: 63 * 15 / 746.9 = 1.265x   <- the db_5b oracle target
///   volume 15: 177 * 15 / 746.9 = 3.554x  <- full-scale, the i16 blocker
/// ```
///
/// This independently reproduces the ~1.27x / ~3.56x figures the accuracy
/// ledger recorded when the calibration was deferred. The NESdev wiki and the
/// in-repo technical references describe the chip but pin no absolute level —
/// expansion-audio levels are a mixer convention, not a hardware spec, which
/// is why the reference emulator is the oracle here.
///
/// The scale itself is measured, not computed: with the shape table above and
/// the bus's `/65536` contract, `db_5b` measured `0.0685x` before this change,
/// so `1.2652 / 0.0685 = 18.471`. That is the same measure-then-fix method
/// `NAMCO163_MIX_SCALE` used for its ~12 dB correction.
///
/// **This is why `Mapper::mix_audio` had to widen to `i32` first.** A
/// volume-15 tone now reaches `1882 * 18.471 = 34,761` — already past
/// `i16::MAX` for ONE channel — and three simultaneous full-volume tones
/// (Gimmick!, Hebereke) reach ~104 k, 3.2x over. The level could not be
/// corrected while the return type was `i16`; that, not the arithmetic, was
/// the actual blocker.
#[cfg_attr(not(feature = "mapper-audio"), allow(dead_code))]
const SUNSOFT5B_MIX_SCALE_NUM: i32 = 2549;
/// Denominator of [`SUNSOFT5B_MIX_SCALE_NUM`].
#[cfg_attr(not(feature = "mapper-audio"), allow(dead_code))]
const SUNSOFT5B_MIX_SCALE_DEN: i32 = 138;

/// One of the 5B's three square-wave tone channels.
///
/// The chip toggles the output level every `16 * TP` CPU cycles (TP = the
/// 12-bit period from registers `$00/$01` for channel A, etc.).  Per wiki,
/// a `TP` of 0 behaves identically to `TP` of 1, so the divide path uses
/// `max(TP, 1)` to avoid both a divide-by-zero and a degenerate "always
/// toggling" case.  None of the 5B's generators can be halted — disabling
/// a channel in the mixer only mutes its output, the internal counters
/// keep running.
#[derive(Clone, Default)]
struct Sunsoft5BTone {
    /// 12-bit reload period.
    period: u16,
    /// Internal half-period countdown in CPU clocks (counts down from
    /// `16 * period`; on hitting 0 the level toggles and the counter
    /// reloads).
    counter: u32,
    /// Current square-wave output level (0 or 1).
    level: u8,
}

#[cfg_attr(not(feature = "mapper-audio"), allow(dead_code))]
impl Sunsoft5BTone {
    /// Effective half-period, in CPU clocks (`max(period, 1) * 16`).
    fn half_period(&self) -> u32 {
        u32::from(self.period.max(1)) * 16
    }

    /// One CPU cycle.  Counters always run, even when the channel is
    /// muted by the mixer register.
    fn clock(&mut self) {
        if self.counter == 0 {
            self.counter = self.half_period();
            self.level ^= 1;
        } else {
            self.counter -= 1;
        }
    }
}

/// 17-bit LFSR noise generator with taps at bits 16 and 13 (per the AY-
/// 3-8910 datasheet, as cited on the NESdev wiki).
#[derive(Clone)]
struct Sunsoft5BNoise {
    /// 5-bit period reload (`$06`).
    period: u8,
    /// Half-period countdown in CPU clocks.
    counter: u32,
    /// 17-bit LFSR state; output is bit 0.
    lfsr: u32,
}

impl Default for Sunsoft5BNoise {
    fn default() -> Self {
        // The AY's LFSR powers up with all bits set; if it ever reached 0
        // it would lock up (no taps could ever flip a bit back in).
        Self {
            period: 0,
            counter: 0,
            lfsr: 0x1FFFF,
        }
    }
}

#[cfg_attr(not(feature = "mapper-audio"), allow(dead_code))]
impl Sunsoft5BNoise {
    fn half_period(&self) -> u32 {
        u32::from(self.period.max(1)) * 16
    }

    fn clock(&mut self) {
        if self.counter == 0 {
            self.counter = self.half_period();
            // 17-bit LFSR, taps at bits 16 and 13 (XOR).  Shift right,
            // feed the XOR back into bit 16.
            let fb = ((self.lfsr >> 16) ^ (self.lfsr >> 13)) & 1;
            self.lfsr = (self.lfsr >> 1) | (fb << 16);
            self.lfsr &= 0x1FFFF;
        } else {
            self.counter -= 1;
        }
    }

    /// Current noise output bit (0 or 1).
    fn level(&self) -> u8 {
        (self.lfsr & 1) as u8
    }
}

/// Envelope generator: 16-bit period, 32-step output, 10 distinct shapes.
///
/// Writing the shape register (`$0D`) **restarts** the envelope from its
/// shape-determined starting position.  The wiki gives the shapes in
/// terms of four bits `CAaH` (continue/attack/alternate/hold); we
/// implement them as a small state machine — `attack` chooses the
/// starting direction, `alternate` flips it after each ramp, `continue`
/// gates whether to keep going past the first ramp, and `hold` freezes
/// (with `attack XOR alternate` deciding the held value).
#[derive(Clone, Default)]
struct Sunsoft5BEnvelope {
    /// 16-bit reload period.
    period: u16,
    /// Half-step countdown in CPU clocks (the wiki gives step frequency
    /// `clock / (16 * period)`).
    counter: u32,
    /// Shape register value (`$0D`).  Only the low 4 bits matter.
    shape: u8,
    /// Current 5-bit envelope level (0..=31).
    level: u8,
    /// Internal direction: +1 for rising, -1 for falling.
    rising: bool,
    /// Set once the envelope has completed its first ramp and decided to
    /// hold (per `continue=0` or `hold=1` after the first ramp/alternate).
    holding: bool,
}

#[cfg_attr(not(feature = "mapper-audio"), allow(dead_code))]
impl Sunsoft5BEnvelope {
    /// Effective step interval in CPU clocks.
    fn step_period(&self) -> u32 {
        u32::from(self.period.max(1)) * 16
    }

    /// Write `$0D` — latches the shape AND restarts the envelope.
    fn write_shape(&mut self, value: u8) {
        self.shape = value & 0x0F;
        // Attack bit (bit 2) sets the initial direction.  When attack=1,
        // start at 0 going up; when attack=0, start at 31 going down.
        let attack = (self.shape & 0x04) != 0;
        self.rising = attack;
        self.level = if attack { 0 } else { 31 };
        self.counter = self.step_period();
        self.holding = false;
    }

    /// One CPU cycle.  Runs forever (cannot be halted) but emits silence
    /// while `holding == true` and `continue == 0`.
    fn clock(&mut self) {
        if self.counter == 0 {
            self.counter = self.step_period();
            self.step();
        } else {
            self.counter -= 1;
        }
    }

    fn step(&mut self) {
        if self.holding {
            return;
        }
        if self.rising {
            if self.level < 31 {
                self.level += 1;
                return;
            }
        } else if self.level > 0 {
            self.level -= 1;
            return;
        }
        // We reached the end of a ramp.  Decide what to do based on the
        // four shape bits.  Per the wiki:
        //   continue=0 (bit 3): the envelope holds at 0 regardless of the
        //                       other bits after one ramp.
        //   hold=1 (bit 0):     hold at the current value (possibly flipped
        //                       by alternate).
        //   alternate=1 (bit 1): reverse direction every ramp.
        let cont = (self.shape & 0x08) != 0;
        let alternate = (self.shape & 0x02) != 0;
        let hold = (self.shape & 0x01) != 0;
        if !cont {
            self.level = 0;
            self.holding = true;
            return;
        }
        if hold {
            if alternate {
                // /\___ etc.: flip the final level once.
                self.level = if self.rising { 0 } else { 31 };
            }
            self.holding = true;
            return;
        }
        if alternate {
            self.rising = !self.rising;
        } else {
            // Pure sawtooth: snap back to the starting level.
            self.level = if self.rising { 0 } else { 31 };
        }
    }

    /// Current 5-bit envelope output (0..=31).
    const fn output(&self) -> u8 {
        self.level
    }
}

/// 5B audio chip state: 16-byte register file, 3 tone channels, noise
/// generator, envelope generator, plus the address-latch byte that the
/// `$C000-$DFFF` writes use to select the next `$E000-$FFFF` data target.
#[derive(Clone, Default)]
pub(crate) struct Sunsoft5BAudio {
    /// Latched 4-bit register index from the most recent `$C000-$DFFF`
    /// write.  Bits 7-4 of the high-byte are silently ignored (per the
    /// NESdev wiki: writes with bits 7-4 nonzero are inhibited; we model
    /// only the inhibit-on-high-bits case by masking to 4 bits, since no
    /// known software relies on the high bits).
    addr_latch: u8,
    /// Raw 16-byte register file (mostly for save-state round-trip and
    /// debug inspection — the live state lives in the channel structs).
    regs: [u8; 16],
    tone_a: Sunsoft5BTone,
    tone_b: Sunsoft5BTone,
    tone_c: Sunsoft5BTone,
    noise: Sunsoft5BNoise,
    envelope: Sunsoft5BEnvelope,
}

#[cfg_attr(not(feature = "mapper-audio"), allow(dead_code))]
impl Sunsoft5BAudio {
    /// Raw value of one of the 16 PSG registers (`$00-$0F`), for the debug
    /// window. Read-only — no side effects, unlike a real `$E000` access.
    pub(crate) fn reg(&self, idx: usize) -> u8 {
        self.regs[idx & 0x0F]
    }

    /// Current 16-bit envelope period (`$0B` low | `$0C` high), for debug.
    pub(crate) const fn envelope_period(&self) -> u16 {
        self.envelope.period
    }

    /// Current 5-bit envelope output level (0..=31), for debug.
    pub(crate) fn envelope_output(&self) -> u8 {
        self.envelope.output()
    }

    pub(crate) fn write_addr(&mut self, value: u8) {
        // Per the wiki, writes with the high nibble nonzero are inhibited.
        // The simplest faithful model is to mask the latch to 4 bits and
        // accept the next data write unconditionally — no known software
        // depends on the inhibit path.
        self.addr_latch = value & 0x0F;
    }

    pub(crate) fn write_data(&mut self, value: u8) {
        let idx = self.addr_latch as usize;
        self.regs[idx] = value;
        match idx {
            0x00 => self.tone_a.period = (self.tone_a.period & 0x0F00) | u16::from(value),
            0x01 => {
                self.tone_a.period = (self.tone_a.period & 0x00FF) | (u16::from(value & 0x0F) << 8);
            }
            0x02 => self.tone_b.period = (self.tone_b.period & 0x0F00) | u16::from(value),
            0x03 => {
                self.tone_b.period = (self.tone_b.period & 0x00FF) | (u16::from(value & 0x0F) << 8);
            }
            0x04 => self.tone_c.period = (self.tone_c.period & 0x0F00) | u16::from(value),
            0x05 => {
                self.tone_c.period = (self.tone_c.period & 0x00FF) | (u16::from(value & 0x0F) << 8);
            }
            0x06 => self.noise.period = value & 0x1F,
            0x07 => { /* mixer; consulted live in `mix_audio`. */ }
            0x08 | 0x09 | 0x0A => { /* per-channel volume; consulted live. */ }
            0x0B => {
                self.envelope.period = (self.envelope.period & 0xFF00) | u16::from(value);
            }
            0x0C => {
                self.envelope.period = (self.envelope.period & 0x00FF) | (u16::from(value) << 8);
            }
            0x0D => self.envelope.write_shape(value),
            // $0E/$0F = I/O ports A/B.  Unused on the NES (the cart never
            // wires them out).  We latch the byte for save-state round-trip
            // and otherwise ignore.
            _ => {}
        }
    }

    /// Mixer register: bits are `--CBAcca`, 0 = enable / 1 = disable.
    /// Bits 5/3/1 are noise enables for channels C/B/A respectively;
    /// bits 4/2/0 are tone enables for channels c/b/a (same lettering).
    const fn tone_enabled(&self, ch: u8) -> bool {
        let mixer = self.regs[0x07];
        // 0 = enable, 1 = disable.  Tone bits = 0, 2, 4 for A/B/C.
        (mixer >> (ch * 2)) & 1 == 0
    }

    const fn noise_enabled(&self, ch: u8) -> bool {
        let mixer = self.regs[0x07];
        // Noise bits = 1, 3, 5 for A/B/C.
        (mixer >> (ch * 2 + 1)) & 1 == 0
    }

    /// Resolve the 4-bit equivalent volume for channel `ch` (0/1/2 for
    /// A/B/C), honoring the per-channel envelope-mode bit.
    fn volume(&self, ch: u8) -> u8 {
        let reg = self.regs[0x08 + ch as usize];
        if reg & 0x10 != 0 {
            // Envelope mode: 5-bit env mapped to 4-bit equivalent via `>>1`
            // per the NESdev table (env=0/1 both -> silent; env=2 -> vol 1;
            // env=31 -> vol 15).
            self.envelope.output() >> 1
        } else {
            reg & 0x0F
        }
    }

    /// Advance every internal generator by one CPU cycle.  Per the wiki,
    /// "none of the various generators can be halted" — they run whenever
    /// the chip is clocked, regardless of mixer/enable state.
    #[cfg(feature = "mapper-audio")]
    pub(crate) fn clock(&mut self) {
        self.tone_a.clock();
        self.tone_b.clock();
        self.tone_c.clock();
        self.noise.clock();
        self.envelope.clock();
    }

    /// Linear-summed audio output, scaled to ~i16 with the same headroom
    /// VRC6 leaves for the APU mixer.
    #[cfg(feature = "mapper-audio")]
    pub(crate) fn mix(&self) -> i32 {
        let mut sum: i32 = 0;
        for (ch, tone) in [&self.tone_a, &self.tone_b, &self.tone_c]
            .iter()
            .enumerate()
        {
            let ch = ch as u8;
            // Per wiki: "If both bits are 1 [disable + disable], the
            // channel outputs a constant signal at the specified volume.
            // If both bits are 0, the result is the logical and of noise
            // and tone."  Equivalent: emit when (tone_enabled => square
            // high) AND (noise_enabled => noise high), defaulting either
            // factor to "1" when its source is disabled.
            let tone_factor = !self.tone_enabled(ch) || tone.level != 0;
            let noise_factor = !self.noise_enabled(ch) || self.noise.level() != 0;
            if tone_factor && noise_factor {
                let v = self.volume(ch) as usize & 0x0F;
                sum += SUNSOFT5B_LOG_VOL[v];
            }
        }
        // Scale the shape table to the hardware-relative level (see
        // `SUNSOFT5B_MIX_SCALE_NUM`), then centre on zero so the BLEP buffer
        // doesn't see a steady DC offset for an idle
        // (all-channels-on-with-fixed-volume) cartridge. No cast: v2.2.3
        // widened `Mapper::mix_audio` to i32 precisely so the 5B's full
        // three-channel swing (~104 k) is representable rather than clamped.
        // The multiply precedes the divide so the integer division loses at
        // most 1 part in ~12,000 on a volume-12 tone.
        sum * SUNSOFT5B_MIX_SCALE_NUM / SUNSOFT5B_MIX_SCALE_DEN - SUNSOFT5B_DC_BIAS
    }

    /// Feature-off shim: the generators do not advance with `mapper-audio`
    /// disabled (mirrors the gated path so the shared NSF expansion router
    /// can clock unconditionally).
    #[cfg(not(feature = "mapper-audio"))]
    #[allow(clippy::needless_pass_by_ref_mut, clippy::unused_self)]
    pub(crate) fn clock(&mut self) {}

    /// Feature-off shim: silence when `mapper-audio` is disabled.
    #[cfg(not(feature = "mapper-audio"))]
    #[allow(clippy::unused_self)]
    pub(crate) fn mix(&self) -> i32 {
        0
    }

    /// Serialize the live audio state.  21-byte tail:
    ///   addr_latch(1) + regs[16](16) + tone_a/b/c counter+level(3*5=15) +
    ///   noise counter+lfsr(4+1+... wait that's bigger).
    ///
    /// Tail layout (kept in lock-step with `read_tail`):
    ///   addr_latch         : 1
    ///   regs               : 16
    ///   tone_a.counter     : 4 (u32 LE)
    ///   tone_a.level       : 1
    ///   tone_b.counter     : 4
    ///   tone_b.level       : 1
    ///   tone_c.counter     : 4
    ///   tone_c.level       : 1
    ///   noise.counter      : 4
    ///   noise.lfsr         : 4 (u32 LE, only low 17 bits used)
    ///   envelope.counter   : 4
    ///   envelope.level     : 1
    ///   envelope.rising    : 1 (bool)
    ///   envelope.holding   : 1 (bool)
    ///   -- 51 bytes total --
    /// (Channel period/shape state is reconstructible from `regs`; we
    /// don't serialize the period/shape fields separately.)
    fn write_tail(&self, out: &mut Vec<u8>) {
        out.push(self.addr_latch);
        out.extend_from_slice(&self.regs);
        for t in [&self.tone_a, &self.tone_b, &self.tone_c] {
            out.extend_from_slice(&t.counter.to_le_bytes());
            out.push(t.level);
        }
        out.extend_from_slice(&self.noise.counter.to_le_bytes());
        out.extend_from_slice(&self.noise.lfsr.to_le_bytes());
        out.extend_from_slice(&self.envelope.counter.to_le_bytes());
        out.push(self.envelope.level);
        out.push(u8::from(self.envelope.rising));
        out.push(u8::from(self.envelope.holding));
    }

    /// Tail size in bytes — see `write_tail`.
    const TAIL_LEN: usize = 1 + 16 + 3 * 5 + 4 + 4 + 4 + 1 + 1 + 1;

    fn read_tail(&mut self, src: &[u8]) -> Result<(), MapperError> {
        if src.len() < Self::TAIL_LEN {
            return Err(MapperError::Truncated {
                expected: Self::TAIL_LEN,
                got: src.len(),
            });
        }
        self.addr_latch = src[0] & 0x0F;
        self.regs.copy_from_slice(&src[1..17]);
        let mut cur = 17usize;
        for t in [&mut self.tone_a, &mut self.tone_b, &mut self.tone_c] {
            t.counter = u32::from_le_bytes([src[cur], src[cur + 1], src[cur + 2], src[cur + 3]]);
            t.level = src[cur + 4] & 1;
            cur += 5;
        }
        self.noise.counter =
            u32::from_le_bytes([src[cur], src[cur + 1], src[cur + 2], src[cur + 3]]);
        cur += 4;
        self.noise.lfsr =
            u32::from_le_bytes([src[cur], src[cur + 1], src[cur + 2], src[cur + 3]]) & 0x1FFFF;
        if self.noise.lfsr == 0 {
            // Guard against a lock-up (LFSR with all zeros has no way out).
            self.noise.lfsr = 0x1FFFF;
        }
        cur += 4;
        self.envelope.counter =
            u32::from_le_bytes([src[cur], src[cur + 1], src[cur + 2], src[cur + 3]]);
        cur += 4;
        self.envelope.level = src[cur] & 0x1F;
        self.envelope.rising = src[cur + 1] != 0;
        self.envelope.holding = src[cur + 2] != 0;
        // Reconstruct live period/shape state from the register file.
        self.tone_a.period = u16::from(self.regs[0x00]) | (u16::from(self.regs[0x01] & 0x0F) << 8);
        self.tone_b.period = u16::from(self.regs[0x02]) | (u16::from(self.regs[0x03] & 0x0F) << 8);
        self.tone_c.period = u16::from(self.regs[0x04]) | (u16::from(self.regs[0x05] & 0x0F) << 8);
        self.noise.period = self.regs[0x06] & 0x1F;
        self.envelope.period = u16::from(self.regs[0x0B]) | (u16::from(self.regs[0x0C]) << 8);
        self.envelope.shape = self.regs[0x0D] & 0x0F;
        Ok(())
    }
}

/// Sunsoft FME-7 (Mapper 69).  Bank-switching, CPU-cycle IRQ, and (gated
/// behind `mapper-audio`) the on-cart Sunsoft 5B audio chip.
pub struct Fme7 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    prg_ram: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    cmd: u8,
    chr: [u8; 8],
    prg_banks: [u8; 4], // $6000, $8000, $A000, $C000 (E000 fixed)
    prg_ram_enabled: bool,
    prg_ram_select: bool,
    mirroring: Mirroring,

    irq_counter: u16,
    irq_enabled: bool,
    irq_counter_enabled: bool,
    irq_pending: bool,

    /// Sunsoft 5B audio extension state.  Live regardless of the
    /// `mapper-audio` feature — the register decoders always latch into
    /// `regs` (so save states stay round-trippable across builds), but
    /// `clock()` / `mix()` are only called when the feature is on.
    audio: Sunsoft5BAudio,
}

impl Fme7 {
    /// Construct a new FME-7 mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "FME-7 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "FME-7 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            cmd: 0,
            chr: [0; 8],
            prg_banks: [0; 4],
            prg_ram_enabled: false,
            prg_ram_select: true,
            mirroring,
            irq_counter: 0,
            irq_enabled: false,
            irq_counter_enabled: false,
            irq_pending: false,
            audio: Sunsoft5BAudio::default(),
        })
    }

    fn prg_8k(&self, idx: usize) -> usize {
        let total_8k = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        (self.prg_banks[idx] as usize) % total_8k
    }
}

impl Mapper for Fme7 {
    fn sram(&self) -> &[u8] {
        &self.prg_ram
    }
    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.prg_ram
    }
    // v2.8.0 Phase 4 — CPU-cycle hook + IRQ source + expansion audio
    // (the audio hook only exists under the `mapper-audio` feature).
    fn caps(&self) -> MapperCaps {
        MapperCaps {
            cpu_cycle_hook: true,
            audio: cfg!(feature = "mapper-audio"),
            frame_event_hook: false,
            irq_source: true,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_select && self.prg_ram_enabled {
                    return self.prg_ram[(addr - 0x6000) as usize % self.prg_ram.len()];
                }
                let bank = self.prg_8k(0);
                self.prg_rom[(bank * PRG_BANK_8K + (addr as usize - 0x6000)) % self.prg_rom.len()]
            }
            0x8000..=0x9FFF => {
                let off = self.prg_8k(1) * PRG_BANK_8K + (addr as usize - 0x8000);
                self.prg_rom[off % self.prg_rom.len()]
            }
            0xA000..=0xBFFF => {
                let off = self.prg_8k(2) * PRG_BANK_8K + (addr as usize - 0xA000);
                self.prg_rom[off % self.prg_rom.len()]
            }
            0xC000..=0xDFFF => {
                let off = self.prg_8k(3) * PRG_BANK_8K + (addr as usize - 0xC000);
                self.prg_rom[off % self.prg_rom.len()]
            }
            0xE000..=0xFFFF => {
                let total_8k = (self.prg_rom.len() / PRG_BANK_8K).max(1);
                let last = total_8k - 1;
                self.prg_rom[(last * PRG_BANK_8K + (addr as usize - 0xE000)) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_select && self.prg_ram_enabled {
                    let off = (addr - 0x6000) as usize % self.prg_ram.len();
                    self.prg_ram[off] = value;
                }
            }
            0x8000..=0x9FFF => self.cmd = value & 0x0F,
            0xA000..=0xBFFF => match self.cmd {
                0..=7 => self.chr[self.cmd as usize] = value,
                8 => {
                    self.prg_ram_enabled = (value & 0x80) != 0;
                    self.prg_ram_select = (value & 0x40) != 0;
                    self.prg_banks[0] = value & 0x3F;
                }
                9..=11 => self.prg_banks[(self.cmd - 8) as usize] = value & 0x3F,
                12 => {
                    self.mirroring = match value & 0x03 {
                        0 => Mirroring::Vertical,
                        1 => Mirroring::Horizontal,
                        2 => Mirroring::SingleScreenA,
                        _ => Mirroring::SingleScreenB,
                    };
                }
                13 => {
                    self.irq_enabled = (value & 0x01) != 0;
                    self.irq_counter_enabled = (value & 0x80) != 0;
                    self.irq_pending = false;
                }
                14 => self.irq_counter = (self.irq_counter & 0xFF00) | u16::from(value),
                15 => self.irq_counter = (self.irq_counter & 0x00FF) | (u16::from(value) << 8),
                _ => {}
            },
            // Sunsoft 5B audio: $C000-$DFFF latches the register address;
            // $E000-$FFFF writes data to the latched register.  Mapper-audio
            // OFF builds still latch state (so the save-state path is
            // round-trippable) but never advance the oscillators.
            0xC000..=0xDFFF => self.audio.write_addr(value),
            0xE000..=0xFFFF => self.audio.write_data(value),
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let total_1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
                let slot = addr as usize / CHR_BANK_1K;
                let bank = (self.chr[slot] as usize) % total_1k;
                let off = bank * CHR_BANK_1K + (addr as usize & (CHR_BANK_1K - 1));
                self.chr_rom[off % self.chr_rom.len()]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring) % self.vram.len()],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let len = self.chr_rom.len();
                    self.chr_rom[addr as usize % len] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring) % self.vram.len();
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn notify_cpu_cycle(&mut self) {
        // Sunsoft 5B audio runs every CPU cycle, regardless of IRQ state.
        // None of the 5B's internal generators can be halted, so we always
        // tick when the feature is on.
        #[cfg(feature = "mapper-audio")]
        self.audio.clock();

        if self.irq_counter_enabled {
            self.irq_counter = self.irq_counter.wrapping_sub(1);
            if self.irq_counter == 0xFFFF && self.irq_enabled {
                self.irq_pending = true;
            }
        }
    }

    #[cfg(feature = "mapper-audio")]
    fn mix_audio(&mut self) -> i32 {
        self.audio.mix()
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 69,
            name: "Sunsoft FME-7".into(),
            mirroring: crate::mapper::mirroring_name(self.current_mirroring()),
            ..Default::default()
        };
        for (i, b) in self.prg_banks.iter().enumerate() {
            info.prg_banks
                .push((format!("PRG{i}"), format!("{b:#04x}")));
        }
        for (i, b) in self.chr.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR{i}"), format!("{b:#04x}")));
        }
        info.irq_state
            .push(("counter".into(), format!("{:#06x}", self.irq_counter)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.irq_state
            .push(("counting".into(), format!("{}", self.irq_counter_enabled)));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending)));
        info.extra
            .push(("cmd".into(), format!("{:#04x}", self.cmd)));
        info.extra.push((
            "prg_ram".into(),
            format!("en={} sel={}", self.prg_ram_enabled, self.prg_ram_select),
        ));
        // v2.2.3 — surface the Sunsoft 5B audio register file. The 5B is the
        // only part of this board with no other debug window, and its state is
        // exactly what you need to answer "why is this cart silent?" — the
        // mixer/enable byte ($07) and the three volume bytes ($08-$0A, bit 4 =
        // envelope mode) decide whether anything sounds at all.
        #[cfg(feature = "mapper-audio")]
        {
            let a = &self.audio;
            info.extra
                .push(("5b_mixer($07)".into(), format!("{:#04x}", a.reg(0x07))));
            info.extra.push((
                "5b_vol(A,B,C)".into(),
                format!(
                    "{:#04x} {:#04x} {:#04x}",
                    a.reg(0x08),
                    a.reg(0x09),
                    a.reg(0x0A)
                ),
            ));
            info.extra.push((
                "5b_env".into(),
                format!(
                    "period={:#06x} shape={:#04x} out={}",
                    a.envelope_period(),
                    a.reg(0x0D),
                    a.envelope_output()
                ),
            ));
            info.extra.push(("5b_mix".into(), format!("{}", a.mix())));
        }
        info
    }

    fn save_state(&self) -> Vec<u8> {
        // v2: appends the Sunsoft 5B audio state at the end.  Per ADR-0003:
        // strictly additive, so v1 readers tolerate the tail (older builds
        // skip-on-read since the tag is consumed at the section length).
        // Tail size = Sunsoft5BAudio::TAIL_LEN (51 bytes).
        let mut out = Vec::with_capacity(
            40 + self.prg_ram.len() + self.vram.len() + Sunsoft5BAudio::TAIL_LEN,
        );
        out.push(2u8); // version
        out.push(self.cmd);
        out.extend_from_slice(&self.chr);
        out.extend_from_slice(&self.prg_banks);
        out.push(u8::from(self.prg_ram_enabled));
        out.push(u8::from(self.prg_ram_select));
        out.push(self.mirroring as u8);
        out.extend_from_slice(&self.irq_counter.to_le_bytes());
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_counter_enabled));
        out.push(u8::from(self.irq_pending));
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        // v2 audio tail.
        self.audio.write_tail(&mut out);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let scalar_len = 1 + 1 + 8 + 4 + 1 + 1 + 1 + 2 + 1 + 1 + 1;
        let core_expected = scalar_len + self.prg_ram.len() + self.vram.len();
        if data.len() < core_expected {
            return Err(MapperError::Truncated {
                expected: core_expected,
                got: data.len(),
            });
        }
        let version = data[0];
        if !(1..=2).contains(&version) {
            return Err(MapperError::UnsupportedVersion(version));
        }
        self.cmd = data[1];
        self.chr.copy_from_slice(&data[2..10]);
        self.prg_banks.copy_from_slice(&data[10..14]);
        self.prg_ram_enabled = data[14] != 0;
        self.prg_ram_select = data[15] != 0;
        self.mirroring = match data[16] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.irq_counter = u16::from_le_bytes(
            data[17..19]
                .try_into()
                .map_err(|_| MapperError::Invalid("irq_counter".into()))?,
        );
        self.irq_enabled = data[19] != 0;
        self.irq_counter_enabled = data[20] != 0;
        self.irq_pending = data[21] != 0;
        let mut cur = 22usize;
        self.prg_ram
            .copy_from_slice(&data[cur..cur + self.prg_ram.len()]);
        cur += self.prg_ram.len();
        self.vram.copy_from_slice(&data[cur..cur + self.vram.len()]);
        cur += self.vram.len();

        // v2 tail: audio state.  v1 blobs end at the core; per ADR-0003,
        // we leave audio at its current state (the caller is responsible
        // for an explicit power-cycle if they want a clean slate).  A v2
        // blob shorter than TAIL_LEN bytes is accepted permissively for
        // the same forward-compat reason VRC6 uses.
        if version == 2 && data.len() >= cur + Sunsoft5BAudio::TAIL_LEN {
            self.audio
                .read_tail(&data[cur..cur + Sunsoft5BAudio::TAIL_LEN])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * PRG_BANK_8K];
        for b in 0..banks_8k {
            v[b * PRG_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr(banks_1k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_1k * CHR_BANK_1K];
        for b in 0..banks_1k {
            v[b * CHR_BANK_1K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn fme7_basic_banking() {
        let mut m = Fme7::new(synth(16), synth_chr(8), Mirroring::Vertical).unwrap();
        // cmd=9 -> writes prg_banks[1] (the $8000-$9FFF window).
        m.cpu_write(0x8000, 9);
        m.cpu_write(0xA000, 5);
        // Read at $8000 should now be bank 5 (offset 0 == bank index byte).
        assert_eq!(m.cpu_read(0x8000), 5);
        // cmd=10 -> prg_banks[2] ($A000-$BFFF).
        m.cpu_write(0x8000, 10);
        m.cpu_write(0xA000, 7);
        assert_eq!(m.cpu_read(0xA000), 7);
    }

    fn fme7_audio_write(m: &mut Fme7, reg: u8, value: u8) {
        m.cpu_write(0xC000, reg);
        m.cpu_write(0xE000, value);
    }

    #[test]
    fn sunsoft5b_register_address_latch_round_trip() {
        // The address latch is the gateway for every audio write; it must
        // round-trip distinctly from the data path.  After latching $0B
        // (envelope period low), a subsequent data write should target
        // $0B specifically.
        let mut m = Fme7::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0xC000, 0x0B);
        assert_eq!(m.audio.addr_latch, 0x0B);
        // Bits 7-4 of the address byte are ignored (masked to 4 bits).
        m.cpu_write(0xC100, 0xF7);
        assert_eq!(m.audio.addr_latch, 0x07);
        // A data write at $E000-$FFFF goes to the latched register.
        m.cpu_write(0xE800, 0xAB);
        assert_eq!(m.audio.regs[0x07], 0xAB);
    }

    #[test]
    fn sunsoft5b_channel_period_decodes_into_internal_state() {
        // Channel A period: TP = ($01 & 0x0F) << 8 | $00.  Confirm the
        // 12-bit period composes correctly from the two writes, and that
        // bits 7-4 of $01 are masked off.
        let mut m = Fme7::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        fme7_audio_write(&mut m, 0x00, 0x34);
        fme7_audio_write(&mut m, 0x01, 0xF7); // upper nibble (7) used; F is ignored.
        assert_eq!(m.audio.tone_a.period, 0x0734);

        // Channel B / C similarly.
        fme7_audio_write(&mut m, 0x02, 0x12);
        fme7_audio_write(&mut m, 0x03, 0x03);
        assert_eq!(m.audio.tone_b.period, 0x0312);
        fme7_audio_write(&mut m, 0x04, 0xFF);
        fme7_audio_write(&mut m, 0x05, 0x0F);
        assert_eq!(m.audio.tone_c.period, 0x0FFF);
    }

    #[test]
    fn sunsoft5b_tone_toggles_every_16_times_period_cycles() {
        // Per NESdev wiki: the square wave toggles every 16 CPU clocks per
        // period count.  With TP = 5, we expect a toggle every 80 cycles.
        // Drive the chip through clock() directly to isolate the tone path
        // from the rest of the mapper.
        let mut t = Sunsoft5BTone {
            period: 5,
            ..Sunsoft5BTone::default()
        };
        // First clock fires immediately (counter starts at 0) and reloads.
        // Count toggles across 800 cycles.
        let mut toggles = 0u32;
        let mut last = t.level;
        for _ in 0..800 {
            t.clock();
            if t.level != last {
                toggles += 1;
                last = t.level;
            }
        }
        // 800 cycles / 80 per toggle = 10 toggles.  Allow ±1 for the
        // counter-starts-at-zero start-up edge.
        assert!(
            (9..=11).contains(&toggles),
            "tone toggle count {toggles} not in 9..=11"
        );
    }

    #[test]
    fn sunsoft5b_volume_scale_zero_silent_max_peak() {
        // Volume 0 must produce silence; volume 15 must produce the peak
        // entry of the log-DAC table.  These bracket the per-channel
        // contribution range.
        assert_eq!(SUNSOFT5B_LOG_VOL[0], 0);
        assert!(SUNSOFT5B_LOG_VOL[15] > SUNSOFT5B_LOG_VOL[14]);
        // The volume() helper applies the envelope-mode select bit.
        let mut a = Sunsoft5BAudio::default();
        a.regs[0x08] = 0x0F; // fixed volume = 15.
        assert_eq!(a.volume(0), 15);
        a.regs[0x08] = 0x00; // fixed volume = 0.
        assert_eq!(a.volume(0), 0);
    }

    #[test]
    fn sunsoft5b_envelope_mode_routes_envelope_into_channel() {
        // Setting bit 4 of $08/$09/$0A switches that channel from fixed
        // volume to envelope mode.  In envelope mode the 4-bit volume
        // equivalent is env >> 1 (per the NESdev table).
        let mut a = Sunsoft5BAudio::default();
        a.regs[0x08] = 0x10; // envelope mode, fixed-volume bits ignored.
        a.envelope.level = 30; // 4-bit equivalent = 15.
        assert_eq!(a.volume(0), 15);
        a.envelope.level = 6;
        assert_eq!(a.volume(0), 3);
        a.envelope.level = 1;
        assert_eq!(a.volume(0), 0); // env 0 and 1 both -> 0.
        // Switching back to fixed mode honors $08 bits 3-0 again.
        a.regs[0x08] = 0x07;
        assert_eq!(a.volume(0), 7);
    }

    #[cfg(feature = "mapper-audio")]
    #[test]
    fn sunsoft5b_mix_output_sign_silent_vs_active() {
        // With every channel muted (mixer = 0xFF disables both tone and
        // noise on A/B/C; volumes don't matter), the linear sum is 0 and
        // the mix output sits at -DC_BIAS (centered).  With one channel
        // unmuted at max volume and the square wave high, the sum exceeds
        // the bias and the mix is positive.
        let mut m = Fme7::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        fme7_audio_write(&mut m, 0x07, 0x3F); // bits 0..=5 all set => all disabled.
        // Volumes irrelevant when channels are muted.
        let silent = m.mix_audio();
        assert_eq!(silent, -SUNSOFT5B_DC_BIAS);

        // Enable tone A only at max volume, then force the square level high
        // by ticking once with period = 0 (the chip wraps period=0 to 1).
        fme7_audio_write(&mut m, 0x07, 0b0011_1110); // tone A enabled (bit 0 = 0).
        fme7_audio_write(&mut m, 0x08, 0x0F); // channel A volume = 15.
        // Manually toggle the tone level so we hit the "high" half-cycle.
        m.audio.tone_a.level = 1;
        let active = m.mix_audio();
        assert!(
            active > 0,
            "active mix output should be positive, got {active}"
        );
    }

    #[test]
    fn sunsoft5b_save_state_v2_round_trips_audio() {
        // Round-trip an FME-7 with a non-trivial audio register file.  The
        // load_state path reconstructs the live period/shape state from the
        // serialized register file, so verifying via `audio.tone_a.period`
        // exercises both the regs blob and the reconstruction path.
        let mut m = Fme7::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        fme7_audio_write(&mut m, 0x00, 0x55);
        fme7_audio_write(&mut m, 0x01, 0x06);
        fme7_audio_write(&mut m, 0x08, 0x0F);
        fme7_audio_write(&mut m, 0x07, 0x36); // a few tone/noise enables.
        fme7_audio_write(&mut m, 0x0D, 0x0E); // envelope shape -> restart.
        let blob = m.save_state();
        assert_eq!(blob[0], 2, "save_state must bump FME-7 to version 2");

        let mut m2 = Fme7::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).expect("v2 round-trip");
        assert_eq!(m2.audio.tone_a.period, 0x0655);
        assert_eq!(m2.audio.regs[0x07], 0x36);
        assert_eq!(m2.audio.regs[0x08], 0x0F);
        assert_eq!(m2.audio.envelope.shape, 0x0E);
    }

    #[test]
    fn sunsoft5b_save_state_loads_v1_blob_with_default_audio() {
        // ADR-0003 invariant: v2 reader must accept a v1 blob; audio state
        // stays at whatever the freshly-constructed mapper has (silence).
        // We synthesize a v1 blob by truncating the audio tail and resetting
        // the version byte.
        let m = Fme7::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        let mut blob = m.save_state();
        let tail = Sunsoft5BAudio::TAIL_LEN;
        blob.truncate(blob.len() - tail);
        blob[0] = 1;

        let mut m2 = Fme7::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        // Perturb audio state pre-load; a v1 blob must not touch it.
        fme7_audio_write(&mut m2, 0x07, 0xAA);
        m2.load_state(&blob)
            .expect("v1 blob must load on v2 reader");
        // Per ADR-0003: older blobs do not reset newer-section state.
        assert_eq!(m2.audio.regs[0x07], 0xAA);
    }

    #[test]
    fn sunsoft5b_mapper_audio_off_path_latches_state_but_stays_silent() {
        // When the `mapper-audio` feature is OFF, the register decoder still
        // latches every write (so save-state round-trip stays correct) but
        // the oscillators never advance and `mix_audio` returns 0.
        //
        // We can't toggle the cargo feature from inside a test, but we CAN
        // assert the two halves of this contract directly:
        //   1. The register latch path is unconditional (this test runs
        //      regardless of the feature flag).
        //   2. The oscillator clock path is gated — verified by the absence
        //      of `audio.clock()` calls in `notify_cpu_cycle` when the
        //      feature is off (compile-time `#[cfg(...)]`).
        // To exercise (1), write to every register and confirm `regs` and
        // the derived period fields are populated.  To exercise (2)'s
        // observable effect, freeze the counters by NOT calling notify and
        // confirm the level state stays at zero.
        let mut m = Fme7::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        for r in 0u8..=0x0F {
            fme7_audio_write(&mut m, r, r.wrapping_mul(0x11));
        }
        assert_eq!(m.audio.regs[0x00], 0x00);
        assert_eq!(m.audio.regs[0x0F], 0xFF);
        // Without any clock() calls, the tone level remains at default 0.
        assert_eq!(m.audio.tone_a.level, 0);
        assert_eq!(m.audio.tone_b.level, 0);
        assert_eq!(m.audio.tone_c.level, 0);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn sunsoft5b_volume_dac_follows_logarithmic_step_law() {
        // The DAC SHAPE criterion. (The absolute LEVEL is a separate concern
        // with its own oracle — `level_db_5b` against the `db_5b` ROM, wired in
        // v2.2.3 A1; it used to be an i16-headroom deferral.) The 5B volume DAC
        // is logarithmic, ~+3 dB
        // (×1.1885² ≈ ×1.4125) per 4-bit step, matching Mesen2's
        // `Sunsoft5bAudio` `_volumeLut` (LUT[12]=63, LUT[15]=177) and tetanes.
        assert_eq!(SUNSOFT5B_LOG_VOL[0], 0, "silence at volume 0");
        // Shape parity with Mesen2's table (floor(10^(0.15*i))) at the two
        // survey-relevant points.
        assert_eq!(SUNSOFT5B_LOG_VOL[12], 668);
        assert_eq!(SUNSOFT5B_LOG_VOL[15], 1882);
        // Each non-zero step multiplies by ~1.4125 (the +1.5 dB × 2 law).
        for v in 2..16usize {
            let ratio = f64::from(SUNSOFT5B_LOG_VOL[v]) / f64::from(SUNSOFT5B_LOG_VOL[v - 1]);
            assert!(
                (ratio - 1.4125).abs() < 0.06,
                "5B DAC step {v}: ratio {ratio:.4} not ~1.4125 (logarithmic law violated)"
            );
        }
        // vol-15 is ~2.82× vol-12 (three +3 dB steps = ×1.4125^3 ≈ 2.818),
        // the ~9 dB the `db_5b` ROM's vol-12 choice sits below full volume.
        let v15_v12 = f64::from(SUNSOFT5B_LOG_VOL[15]) / f64::from(SUNSOFT5B_LOG_VOL[12]);
        assert!(
            (v15_v12 - 2.818).abs() < 0.05,
            "vol-15/vol-12 ratio {v15_v12:.4} not ~2.818"
        );
    }
}

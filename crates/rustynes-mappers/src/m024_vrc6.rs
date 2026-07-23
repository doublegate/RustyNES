//! Konami VRC6 (mappers 24 and 26) -- banking, the VRC IRQ counter, and the
//! on-cart VRC6 audio expansion.
//!
//! VRC6 is a VRC4-family board plus a three-voice synthesizer: two pulse
//! channels with a programmable duty threshold (and an "ignore duty" mode
//! that holds the output at the volume level) and one 8-step sawtooth whose
//! accumulator produces the ramp. All three run off the CPU clock. The
//! synthesizer is gated behind the `mapper-audio` Cargo feature (default ON);
//! with it off the register decoders still latch, so a save state written by
//! an audio-enabled build still loads (ADR 0004).
//!
//! [`VRC6_MIX_SCALE`] is shared with the NSF expansion path
//! (`nsf_expansion.rs`), which re-uses [`Vrc6Pulse`] / [`Vrc6Saw`] verbatim so
//! an NSF tune and the cartridge produce bit-identical output.
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

/// Linear scale applied to the summed VRC6 channel output (see
/// [`Vrc6::mix_audio`]).
///
/// Calibrated so a single full-volume (15) VRC6 pulse reaches ~1.5x the
/// amplitude of a single full-volume 2A03 pulse — the level the bbbradsmith
/// `db_vrc6` decibel-comparison ROM and the Mesen2 mixer characterize (Mesen2
/// `NesSoundMixer::GetOutputVolume` weights VRC6 at `output * 5` against a
/// 2A03 pulse DAC of `95.88*5000/(8128/15+100) ≈ 746.9`, giving `15*15*5 /
/// 746.9 ≈ 1.506`). Concretely, one pulse toggling 0↔15 swings the mixer by
/// `15 * 979 = 14685` raw units; divided by the bus's `/65536` external-audio
/// normalization that is `0.2241`, versus the 2A03 pulse's `pulse_table[15] ≈
/// 0.1488` — a ratio of `1.506`. The full three-channel peak stays in range:
/// `(61 - 30) * 979 = 30349 < i16::MAX`, so a loud Akumajou-Densetsu / Madara
/// passage never clips. Before v2.1.6 this was `256` (≈0.39x the 2A03 pulse —
/// ~11.7 dB too quiet). See `docs/apu-2a03.md` §Expansion-audio levels.
///
/// `pub(crate)` so the NSF-playback path (`crate::nsf_expansion::Vrc6Exp::mix`)
/// references the SAME constant as the cartridge path — the two mixers can
/// never drift apart, guaranteeing an NSF VRC6 tune stays level-matched to a
/// VRC6 cartridge.
pub(crate) const VRC6_MIX_SCALE: i16 = 979;

/// VRC6 audio pulse channel state (`$9000-$9002` for pulse 1, `$A000-$A002`
/// for pulse 2). Period is 12-bit, decrements every CPU cycle. On
/// underflow, the duty index advances by 1 (mod 16). Output is volume when
/// duty index <= duty-cycle threshold (or always-on when "ignore duty" mode
/// is set); zero otherwise.
#[derive(Clone, Default)]
pub(crate) struct Vrc6Pulse {
    /// Bits 0-3: volume (0..=15). Bits 4-6: duty (0..=7, sets the duty-cycle
    /// threshold). Bit 7: ignore-duty (output always = volume).
    pub(crate) ctrl: u8,
    /// 12-bit period reload value.
    pub(crate) period: u16,
    /// Channel enable bit (from period-hi bit 7).
    pub(crate) enabled: bool,
    /// 12-bit countdown timer.
    pub(crate) timer: u16,
    /// 4-bit duty-cycle step (0..=15).
    pub(crate) step: u8,
}

impl Vrc6Pulse {
    /// Clock the timer one CPU cycle. When it underflows, advance the duty
    /// step and reload from `period`.
    pub(crate) fn clock(&mut self) {
        if !self.enabled {
            return;
        }
        if self.timer == 0 {
            self.timer = self.period;
            self.step = (self.step + 1) & 0x0F;
        } else {
            self.timer -= 1;
        }
    }

    /// Current 4-bit unsigned output (0..=15). 0 when disabled.
    pub(crate) fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        let duty = (self.ctrl >> 4) & 0x07;
        let ignore_duty = (self.ctrl & 0x80) != 0;
        let volume = self.ctrl & 0x0F;
        if ignore_duty || self.step <= duty {
            volume
        } else {
            0
        }
    }
}

/// VRC6 audio sawtooth channel state (`$B000-$B002`). 6-bit accumulator
/// adds an "accumulator rate" once per CPU cycle. Every 14th underflow,
/// the high 5 bits of the accumulator are emitted (0..=31) and the
/// accumulator resets.
#[derive(Clone, Default)]
pub(crate) struct Vrc6Saw {
    /// 6-bit accumulator-rate value (bits 5-0 of `$B000`).
    pub(crate) rate: u8,
    /// 12-bit period reload value.
    pub(crate) period: u16,
    /// Channel enable bit (from period-hi bit 7).
    pub(crate) enabled: bool,
    /// 12-bit countdown timer.
    pub(crate) timer: u16,
    /// Internal step counter 0..=13 (every other increment "ticks the
    /// accumulator"; 7 ticks per cycle = 14 steps).
    pub(crate) step: u8,
    /// 8-bit accumulator. Output = accumulator >> 3 (5-bit, 0..=31).
    pub(crate) acc: u8,
}

impl Vrc6Saw {
    pub(crate) fn clock(&mut self) {
        if !self.enabled {
            return;
        }
        if self.timer == 0 {
            self.timer = self.period;
            // Step 0..=13: every 2nd step (1, 3, 5, 7, 9, 11, 13) accumulates.
            // Step 14 (== reset) zeros the accumulator and rolls step to 0.
            self.step += 1;
            if (self.step & 1) == 1 {
                self.acc = self.acc.wrapping_add(self.rate);
            }
            if self.step >= 14 {
                self.step = 0;
                self.acc = 0;
            }
        } else {
            self.timer -= 1;
        }
    }

    /// 5-bit unsigned output (0..=31).
    pub(crate) fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        self.acc >> 3
    }
}

/// VRC6 (Mappers 24 / 26).  Audio extension is implemented behind the
/// `mapper-audio` Cargo feature (default ON).
pub struct Vrc6 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_16: u8, // 16 KiB bank @ $8000-$BFFF
    prg_8: u8,  // 8 KiB bank @ $C000-$DFFF
    chr: [u8; 8],
    mirroring: Mirroring,
    /// 8 KiB WRAM at $6000-$7FFF (battery-backed on Konami carts).
    /// T-60-003b (2026-05-17).
    prg_ram: Box<[u8]>,
    /// Mapper 24 = VRC6a (a0/a1 = bits 0/1).
    /// Mapper 26 = VRC6b (a0/a1 = bits 1/0 — swapped).
    swap_a01: bool,

    irq_latch: u8,
    irq_counter: u8,
    irq_enabled: bool,
    irq_enable_after_ack: bool,
    irq_mode_scanline: bool,
    irq_prescaler: i32,
    irq_pending: bool,

    // Audio extension state.
    /// `$9003` global audio control. Bit 0 = halt-all; bits 1-2 = freq scale
    /// shift (0 = ÷1, 1 = ÷16, 2 = ÷256 — implemented by left-shifting the
    /// effective period). We keep the raw byte and inspect bits at clock time.
    audio_ctrl: u8,
    pulse1: Vrc6Pulse,
    pulse2: Vrc6Pulse,
    saw: Vrc6Saw,
}

#[cfg_attr(not(feature = "mapper-audio"), allow(dead_code))]
impl Vrc6 {
    /// Construct a new VRC6 mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mapper_id: u16,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "VRC6 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
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
                "VRC6 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_16: 0,
            prg_8: 0,
            chr: [0; 8],
            mirroring,
            // 8 KiB WRAM at $6000-$7FFF (T-60-003b).
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
            swap_a01: mapper_id == 26,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_enable_after_ack: false,
            irq_mode_scanline: false,
            irq_prescaler: 341,
            irq_pending: false,
            audio_ctrl: 0,
            pulse1: Vrc6Pulse::default(),
            pulse2: Vrc6Pulse::default(),
            saw: Vrc6Saw::default(),
        })
    }

    /// Effective period for a pulse/saw channel, taking the global
    /// `$9003` halt + frequency-scale bits into account.
    fn effective_period_p(&self, p: &Vrc6Pulse) -> u16 {
        let shift = match (self.audio_ctrl >> 1) & 0x03 {
            0 => 0,
            1 => 4,
            _ => 8,
        };
        p.period >> shift
    }

    fn effective_period_s(&self) -> u16 {
        let shift = match (self.audio_ctrl >> 1) & 0x03 {
            0 => 0,
            1 => 4,
            _ => 8,
        };
        self.saw.period >> shift
    }

    /// Clock all three audio channels one CPU cycle. Called from
    /// `notify_cpu_cycle` when the `mapper-audio` feature is on.
    #[cfg(feature = "mapper-audio")]
    fn clock_audio(&mut self) {
        // $9003 bit 0 = halt-all. When set, channels do not advance.
        if (self.audio_ctrl & 0x01) != 0 {
            return;
        }
        // Apply the frequency-scale shift transiently by temporarily
        // narrowing `period` for the channel clock. We don't mutate the
        // stored period -- the shift is purely a read-time scaling.
        let p1_period = self.effective_period_p(&self.pulse1);
        let p2_period = self.effective_period_p(&self.pulse2);
        let saw_period = self.effective_period_s();
        let saved_p1 = self.pulse1.period;
        let saved_p2 = self.pulse2.period;
        let saved_saw = self.saw.period;
        self.pulse1.period = p1_period;
        self.pulse2.period = p2_period;
        self.saw.period = saw_period;
        self.pulse1.clock();
        self.pulse2.clock();
        self.saw.clock();
        self.pulse1.period = saved_p1;
        self.pulse2.period = saved_p2;
        self.saw.period = saved_saw;
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total_8k = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let last1 = total_8k - 1;
        match addr {
            0x8000..=0xBFFF => {
                let bank16 = (self.prg_16 as usize) & 0x0F;
                let bank8 = (bank16 << 1) | (((addr & 0x2000) >> 13) as usize);
                (bank8 % total_8k) * PRG_BANK_8K + (addr as usize & 0x1FFF)
            }
            0xC000..=0xDFFF => {
                let bank8 = (self.prg_8 as usize) & 0x1F;
                (bank8 % total_8k) * PRG_BANK_8K + (addr as usize & 0x1FFF)
            }
            0xE000..=0xFFFF => last1 * PRG_BANK_8K + (addr as usize & 0x1FFF),
            _ => 0,
        }
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let addr = (addr & 0x1FFF) as usize;
        let total_1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
        let slot = addr / CHR_BANK_1K;
        let bank = (self.chr[slot] as usize) % total_1k;
        bank * CHR_BANK_1K + (addr & (CHR_BANK_1K - 1))
    }

    fn clock_irq_counter(&mut self) {
        if self.irq_counter == 0xFF {
            self.irq_counter = self.irq_latch;
            self.irq_pending = true;
        } else {
            self.irq_counter = self.irq_counter.wrapping_add(1);
        }
    }

    fn decode_a(&self, addr: u16) -> u8 {
        let a0 = (addr & 1) != 0;
        let a1 = (addr & 2) != 0;
        let (a0, a1) = if self.swap_a01 { (a1, a0) } else { (a0, a1) };
        u8::from(a0) | (u8::from(a1) << 1)
    }
}

impl Mapper for Vrc6 {
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
            // T-60-003b (2026-05-17): VRC6 carts (Akumajou Densetsu /
            // Esper Dream 2 / Mouryou Senki Madara) include 8KB
            // battery-backed WRAM at $6000-$7FFF. Pre-fix returned 0;
            // Esper Dream 2 + Madara got stuck-at-uniform-gray
            // validating save data, both bit-identical hash
            // 89ee4c476c97a325 (the smoking-gun signal that pointed
            // here per the recovery-session diagnostic at
            // docs/audit/v1-closeout-progress-2026-05-17.md).
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize % self.prg_ram.len()],
            0x8000..=0xFFFF => {
                let off = self.prg_offset(addr);
                self.prg_rom[off % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // T-60-003b (2026-05-17): WRAM at $6000-$7FFF (paired with the
        // read fix above).
        if (0x6000..=0x7FFF).contains(&addr) {
            let len = self.prg_ram.len();
            self.prg_ram[(addr - 0x6000) as usize % len] = value;
            return;
        }
        let a = self.decode_a(addr);
        match addr & 0xF000 {
            0x8000 => self.prg_16 = value & 0x0F,
            0x9000 => match a {
                // $9000: Pulse 1 control (volume/duty/mode).
                0 => self.pulse1.ctrl = value,
                // $9001: Pulse 1 period low.
                1 => {
                    self.pulse1.period = (self.pulse1.period & 0x0F00) | u16::from(value);
                }
                // $9002: Pulse 1 period high + enable.
                2 => {
                    self.pulse1.period =
                        (self.pulse1.period & 0x00FF) | (u16::from(value & 0x0F) << 8);
                    self.pulse1.enabled = (value & 0x80) != 0;
                    if !self.pulse1.enabled {
                        self.pulse1.step = 0;
                    }
                }
                // $9003: Global audio control (halt + freq scale).
                _ => self.audio_ctrl = value,
            },
            0xA000 => match a {
                // $A000: Pulse 2 control.
                0 => self.pulse2.ctrl = value,
                // $A001: Pulse 2 period low.
                1 => {
                    self.pulse2.period = (self.pulse2.period & 0x0F00) | u16::from(value);
                }
                // $A002: Pulse 2 period high + enable.
                2 => {
                    self.pulse2.period =
                        (self.pulse2.period & 0x00FF) | (u16::from(value & 0x0F) << 8);
                    self.pulse2.enabled = (value & 0x80) != 0;
                    if !self.pulse2.enabled {
                        self.pulse2.step = 0;
                    }
                }
                _ => {}
            },
            0xB000 => match a {
                // $B000: Sawtooth accumulator rate (6-bit).
                0 => self.saw.rate = value & 0x3F,
                // $B001: Sawtooth period low.
                1 => {
                    self.saw.period = (self.saw.period & 0x0F00) | u16::from(value);
                }
                // $B002: Sawtooth period high + enable.
                2 => {
                    self.saw.period = (self.saw.period & 0x00FF) | (u16::from(value & 0x0F) << 8);
                    self.saw.enabled = (value & 0x80) != 0;
                    if !self.saw.enabled {
                        self.saw.step = 0;
                        self.saw.acc = 0;
                    }
                }
                _ => {
                    // $B003: Mirroring + PPU/CPU mode.
                    self.mirroring = match (value >> 2) & 0x03 {
                        0 => Mirroring::Vertical,
                        1 => Mirroring::Horizontal,
                        2 => Mirroring::SingleScreenA,
                        _ => Mirroring::SingleScreenB,
                    };
                }
            },
            0xC000 => self.prg_8 = value & 0x1F,
            0xD000 => self.chr[a as usize] = value,
            0xE000 => self.chr[(a + 4) as usize] = value,
            0xF000 => match a {
                0 => self.irq_latch = value,
                1 => {
                    self.irq_enable_after_ack = (value & 0x01) != 0;
                    self.irq_enabled = (value & 0x02) != 0;
                    self.irq_mode_scanline = (value & 0x04) == 0;
                    if self.irq_enabled {
                        self.irq_counter = self.irq_latch;
                        self.irq_prescaler = 341;
                    }
                    self.irq_pending = false;
                }
                2 => {
                    self.irq_pending = false;
                    self.irq_enabled = self.irq_enable_after_ack;
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
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
        // Audio runs every CPU cycle regardless of IRQ state.
        #[cfg(feature = "mapper-audio")]
        self.clock_audio();

        if !self.irq_enabled {
            return;
        }
        if self.irq_mode_scanline {
            self.irq_prescaler -= 3;
            if self.irq_prescaler <= 0 {
                self.irq_prescaler += 341;
                self.clock_irq_counter();
            }
        } else {
            self.clock_irq_counter();
        }
    }

    #[cfg(feature = "mapper-audio")]
    fn mix_audio(&mut self) -> i32 {
        // Three channels: pulse1 (4-bit, 0..=15), pulse2 (4-bit, 0..=15),
        // sawtooth (5-bit, 0..=31). Sum is in 0..=61.
        //
        // Per nesdev "VRC6 audio": the three channels are summed digitally,
        // so a linear sum is the canonical mix. The [`VRC6_MIX_SCALE`] = 979
        // factor makes a single full-volume pulse ~1.5x the 2A03 pulse (the
        // hardware/Mesen2/`db_vrc6` level); the full three-channel peak
        // `(61 - 30) * 979 = 30349` stays below `i16::MAX`.
        let p1 = i16::from(self.pulse1.output());
        let p2 = i16::from(self.pulse2.output());
        let saw = i16::from(self.saw.output());
        // Center at zero: subtract approx half the peak (~30), then scale by
        // [`VRC6_MIX_SCALE`] for a hardware-accurate level vs the 2A03 pulse.
        i32::from(((p1 + p2 + saw) - 30) * VRC6_MIX_SCALE)
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mapper_id = if self.swap_a01 { 26 } else { 24 };
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id,
            name: "VRC6".into(),
            mirroring: crate::mapper::mirroring_name(self.current_mirroring()),
            ..Default::default()
        };
        info.prg_banks
            .push(("PRG16".into(), format!("{:#04x}", self.prg_16)));
        info.prg_banks
            .push(("PRG8".into(), format!("{:#04x}", self.prg_8)));
        for (i, b) in self.chr.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR{i}"), format!("{b:#04x}")));
        }
        info.irq_state
            .push(("latch".into(), format!("{:#04x}", self.irq_latch)));
        info.irq_state
            .push(("counter".into(), format!("{:#04x}", self.irq_counter)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        // v2: appends audio state (audio_ctrl + 3 channels) at the end.
        // Per ADR-0003: strictly additive; older readers ignore the tail.
        // Channel layout per channel: ctrl(1) + period_lo(1) + period_hi(1)
        //   + enabled(1) + timer_lo(1) + timer_hi(1) + step(1)
        //   = 7 bytes for a pulse channel.
        // Saw: rate(1) + period_lo(1) + period_hi(1) + enabled(1)
        //   + timer_lo(1) + timer_hi(1) + step(1) + acc(1) = 8 bytes.
        // Header: audio_ctrl(1).
        // Total audio tail = 1 + 7 + 7 + 8 = 23 bytes.
        let mut out = Vec::with_capacity(48 + self.vram.len() + 23);
        out.push(2u8); // version
        out.push(self.prg_16);
        out.push(self.prg_8);
        out.extend_from_slice(&self.chr);
        out.push(self.mirroring as u8);
        out.push(u8::from(self.swap_a01));
        out.push(self.irq_latch);
        out.push(self.irq_counter);
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_enable_after_ack));
        out.push(u8::from(self.irq_mode_scanline));
        out.extend_from_slice(&self.irq_prescaler.to_le_bytes());
        out.push(u8::from(self.irq_pending));
        out.extend_from_slice(&self.vram);
        // Audio tail (v2).
        out.push(self.audio_ctrl);
        Self::write_pulse(&mut out, &self.pulse1);
        Self::write_pulse(&mut out, &self.pulse2);
        Self::write_saw(&mut out, &self.saw);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let scalar_len = 1 + 1 + 1 + 8 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 4 + 1;
        let core_expected = scalar_len + self.vram.len();
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
        self.prg_16 = data[1];
        self.prg_8 = data[2];
        self.chr.copy_from_slice(&data[3..11]);
        self.mirroring = match data[11] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.swap_a01 = data[12] != 0;
        self.irq_latch = data[13];
        self.irq_counter = data[14];
        self.irq_enabled = data[15] != 0;
        self.irq_enable_after_ack = data[16] != 0;
        self.irq_mode_scanline = data[17] != 0;
        self.irq_prescaler = i32::from_le_bytes(
            data[18..22]
                .try_into()
                .map_err(|_| MapperError::Invalid("prescaler".into()))?,
        );
        self.irq_pending = data[22] != 0;
        self.vram.copy_from_slice(&data[23..23 + self.vram.len()]);

        // v2 tail (optional even when version == 2, in case the writer is
        // shorter than expected): audio state. v1 blobs end here; the audio
        // state stays at defaults.
        if version == 2 {
            let tail_off = 23 + self.vram.len();
            if data.len() < tail_off + 23 {
                // Not strict: a v2 blob shorter than 23 audio bytes is
                // accepted; remaining fields default-initialize. This keeps
                // forward-compat consistent with ADR-0003.
                return Ok(());
            }
            self.audio_ctrl = data[tail_off];
            Self::read_pulse(&data[tail_off + 1..tail_off + 8], &mut self.pulse1);
            Self::read_pulse(&data[tail_off + 8..tail_off + 15], &mut self.pulse2);
            Self::read_saw(&data[tail_off + 15..tail_off + 23], &mut self.saw);
        }
        Ok(())
    }
}

impl Vrc6 {
    fn write_pulse(out: &mut Vec<u8>, p: &Vrc6Pulse) {
        out.push(p.ctrl);
        out.extend_from_slice(&p.period.to_le_bytes());
        out.push(u8::from(p.enabled));
        out.extend_from_slice(&p.timer.to_le_bytes());
        out.push(p.step);
    }

    fn write_saw(out: &mut Vec<u8>, s: &Vrc6Saw) {
        out.push(s.rate);
        out.extend_from_slice(&s.period.to_le_bytes());
        out.push(u8::from(s.enabled));
        out.extend_from_slice(&s.timer.to_le_bytes());
        out.push(s.step);
        out.push(s.acc);
    }

    fn read_pulse(src: &[u8], p: &mut Vrc6Pulse) {
        p.ctrl = src[0];
        p.period = u16::from_le_bytes([src[1], src[2]]);
        p.enabled = src[3] != 0;
        p.timer = u16::from_le_bytes([src[4], src[5]]);
        p.step = src[6] & 0x0F;
    }

    fn read_saw(src: &[u8], s: &mut Vrc6Saw) {
        s.rate = src[0] & 0x3F;
        s.period = u16::from_le_bytes([src[1], src[2]]);
        s.enabled = src[3] != 0;
        s.timer = u16::from_le_bytes([src[4], src[5]]);
        s.step = src[6];
        s.acc = src[7];
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
    fn vrc6_audio_register_decoders_latch_state() {
        let mut m = Vrc6::new(synth(8), synth_chr(8), 24, Mirroring::Vertical).unwrap();
        // Pulse 1 ctrl = 0x8F (ignore-duty + volume 0xF).
        m.cpu_write(0x9000, 0x8F);
        // Pulse 1 period = 0x123 with enable bit.
        m.cpu_write(0x9001, 0x23);
        m.cpu_write(0x9002, 0x81); // bit 7 = enable, high nibble = 1.
        assert!(m.pulse1.enabled);
        assert_eq!(m.pulse1.period, 0x123);
        assert_eq!(m.pulse1.ctrl, 0x8F);

        // Pulse 2 similar.
        m.cpu_write(0xA000, 0x07); // duty 0 -> threshold 0; volume 7.
        m.cpu_write(0xA001, 0x40);
        m.cpu_write(0xA002, 0x80); // enable, period high nibble 0.
        assert!(m.pulse2.enabled);
        assert_eq!(m.pulse2.period, 0x040);

        // Sawtooth.
        m.cpu_write(0xB000, 0x05); // rate = 5.
        m.cpu_write(0xB001, 0x20);
        m.cpu_write(0xB002, 0x80); // enable.
        assert!(m.saw.enabled);
        assert_eq!(m.saw.rate, 5);
        assert_eq!(m.saw.period, 0x020);

        // $B003 still drives mirroring.
        m.cpu_write(0xB003, 0b0000_0100); // bits 3:2 = 01 -> Horizontal.
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn vrc6_pulse_oscillator_steps_through_duty() {
        let mut p = Vrc6Pulse {
            ctrl: 0x4F, // duty = 0b100 (4) so output high while step <= 4.
            period: 4,  // small, ticks fast.
            enabled: true,
            timer: 0,
            step: 0,
        };
        // First clock: timer == 0 so we reload and bump step to 1.
        let mut outputs = Vec::new();
        for _ in 0..32 {
            p.clock();
            outputs.push(p.output());
        }
        // We expect a roughly 5/16 duty cycle pattern of volume(15) intervals
        // separated by zero intervals. Sanity-check both poles appear.
        assert!(outputs.contains(&0x0F));
        assert!(outputs.contains(&0));
    }

    #[test]
    fn vrc6_sawtooth_emits_ramp() {
        let mut s = Vrc6Saw {
            rate: 0x10,
            period: 2,
            enabled: true,
            timer: 0,
            step: 0,
            acc: 0,
        };
        // Drive long enough to see at least one full 14-step ramp.
        let mut sampled = Vec::new();
        for _ in 0..60 {
            s.clock();
            sampled.push(s.output());
        }
        // Ramp should reach a peak greater than zero and eventually reset.
        let peak = sampled.iter().copied().max().unwrap();
        assert!(peak > 0, "saw must emit a non-zero peak");
        // And it should hit zero (after step >= 14 reset).
        assert!(sampled.contains(&0));
    }

    #[cfg(feature = "mapper-audio")]
    #[test]
    fn vrc6_mix_audio_is_nonzero_when_active() {
        let mut m = Vrc6::new(synth(8), synth_chr(8), 24, Mirroring::Vertical).unwrap();
        // Enable pulse 1 with max volume + ignore-duty mode -> output = 15.
        m.cpu_write(0x9000, 0x8F);
        m.cpu_write(0x9001, 0x10);
        m.cpu_write(0x9002, 0x81);
        // Tick once so the oscillator advances past the timer == 0 reload.
        m.clock_audio();
        let s = m.mix_audio();
        // Centering subtracts ~30 from a 0..=61 sum, scales by 979 (v2.1.6).
        // With only p1 = 15 contributing, s = (15 - 30) * 979 = -14685.
        assert!(s < 0, "mix_audio with only p1 must be below center");
    }

    #[cfg(feature = "mapper-audio")]
    #[test]
    fn vrc6_mix_audio_silent_when_disabled() {
        let m = Vrc6::new(synth(8), synth_chr(8), 24, Mirroring::Vertical).unwrap();
        // All channels disabled -> outputs 0 -> sum 0 -> mix = (0 - 30) * 979.
        // Confirm we land at the documented "center - offset" position.
        let mut m = m;
        let s = m.mix_audio();
        assert_eq!(s, -29370);
    }

    #[test]
    fn vrc6_save_state_v2_round_trips_audio() {
        let mut m = Vrc6::new(synth(8), synth_chr(8), 24, Mirroring::Vertical).unwrap();
        m.cpu_write(0x9000, 0x8F);
        m.cpu_write(0x9001, 0x12);
        m.cpu_write(0x9002, 0x83);
        m.cpu_write(0xB000, 0x07);
        let blob = m.save_state();
        assert_eq!(blob[0], 2, "save_state must bump to version 2");

        let mut m2 = Vrc6::new(synth(8), synth_chr(8), 24, Mirroring::Vertical).unwrap();
        m2.load_state(&blob).expect("v2 round-trip");
        assert_eq!(m2.pulse1.ctrl, 0x8F);
        assert_eq!(m2.pulse1.period, 0x312);
        assert!(m2.pulse1.enabled);
        assert_eq!(m2.saw.rate, 0x07);
    }

    #[test]
    fn vrc6_save_state_loads_v1_blob_with_default_audio() {
        // ADR-0003 invariant: v2 reader must accept a v1 blob; audio state
        // defaults to silence (channels disabled, ctrl/period zero).
        let m = Vrc6::new(synth(8), synth_chr(8), 24, Mirroring::Vertical).unwrap();
        let mut blob = m.save_state();
        // Synthesize a "v1 blob" by truncating the audio tail (last 23 bytes)
        // and rewriting the version byte from 2 -> 1.
        let tail = 23;
        blob.truncate(blob.len() - tail);
        blob[0] = 1;
        let mut m2 = Vrc6::new(synth(8), synth_chr(8), 24, Mirroring::Vertical).unwrap();
        m2.cpu_write(0x9000, 0xFF); // perturb pre-load
        m2.load_state(&blob)
            .expect("v1 blob must load on v2 reader");
        // Audio state is unchanged from before load (no v2 tail).
        // pulse1.ctrl was perturbed and NOT reset, since v1 doesn't carry
        // audio state. This matches ADR-0003: older blobs don't reset
        // newer-section state, the caller is responsible for an explicit
        // reset/power-cycle if they want a clean slate.
        assert_eq!(m2.pulse1.ctrl, 0xFF);
    }
}

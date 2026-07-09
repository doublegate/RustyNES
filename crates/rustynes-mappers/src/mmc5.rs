//! MMC5 (iNES mapper 5) — v0 implementation.
//!
//! See `docs/mappers.md` §MMC5 and the nesdev wiki page
//! <https://www.nesdev.org/wiki/MMC5>.
//!
//! # Scope (v0 + v1)
//!
//! Implemented:
//! - Register layout `$5000-$5FFF` (banking, mirroring, ExRAM, IRQ).
//! - 4 PRG banking modes (`$5100`): 32K / 16K+16K / 16K+8K+8K / 4x8K.
//! - 4 CHR banking modes (`$5101`): 8K / 4K+4K / 4x2K / 8x1K.
//! - Per-1KiB nametable mirroring control via `$5105` (NT_A / NT_B / ExRAM /
//!   fill).
//! - 8 KiB PRG-RAM (single bank) at `$6000-$7FFF` with the PRG-RAM protect
//!   pair `$5102` / `$5103` (two-write unlock).
//! - ExRAM 1 KiB at `$5C00-$5FFF` with mode select via `$5104`:
//!   - Mode 00: extra nametable. ExRAM serves the byte directly via
//!     `Mapper::nametable_fetch`; the PPU bypasses CIRAM for those tables.
//!   - Mode 01 (ExGrafix): per-tile attribute + per-tile CHR bank via
//!     `Mapper::peek_ex_attribute`; the PPU overrides the AT-derived
//!     palette and the BG pattern fetch routes through the latched 4 KiB
//!     bank.
//!   - Mode 10: general-purpose ExRAM. CPU read/write to `$5C00-$5FFF`.
//!   - Mode 11: read-only ExRAM (CPU writes ignored).
//! - 4-byte fill mode (`$5105` per-NT selector 0b11). Nametable byte reads
//!   in those tables return `$5106` (fill tile); attribute byte reads
//!   return `$5107` low 2 bits replicated 4 ways.
//! - Dual sprite vs. background CHR banks. The PPU's sprite tile fetch
//!   path calls `Mapper::ppu_read_sprite`, which we resolve through the
//!   eight 1 KiB sprite-CHR bank registers (`$5120-$5127`). BG fetches
//!   continue to use `$5128-$512B`. In 8x16 sprite mode this matches the
//!   documented MMC5 behavior; in 8x8 mode the registers tend to be
//!   programmed identically.
//! - Scanline IRQ at PPU cycle 4 of each visible scanline. The scanline
//!   counter ticks via `Mapper::notify_scanline_start`; the in-frame flag
//!   is cleared on vertical blank (`Mapper::notify_vblank`).
//! - 8x8 multiply unit at `$5205` / `$5206`.
//!
//! Implemented (continued):
//! - Vertical split-screen mode (`$5200`-`$5202`). The PPU queries
//!   [`Mapper::bg_split_state`] at each BG fetch-group boundary; when the
//!   current tile column falls inside the alt region (determined by `$5200`
//!   bit 6 + bits 4-0), the mapper supplies a synthesized NT/AT address
//!   anchored at ExRAM (rather than the loopy-v derivation) plus the
//!   `$5202` 4 KiB CHR bank and an alt fine-Y from `$5201` + the current
//!   scanline. Castlevania III (J) uses this for its independently-scrolled
//!   status bar.
//!
//! Implemented (continued, audio):
//! - MMC5 audio extension at `$5000-$5015` (Track C2). Two pulse-wave
//!   channels (`$5000-$5007`) modelled on the 2A03 pulse but with no
//!   sweep unit; one raw 7-bit PCM channel via `$5010` (mode bit) +
//!   `$5011` (sample). Used by Castlevania III: Dracula's Curse
//!   (Japan / PAL) and Just Breed. Envelope and length-counter
//!   sub-units share the 2A03 frame-counter cadence — the bus fans
//!   the APU frame events out via [`crate::mapper::Mapper::notify_frame_event`].
//!   Behind the `mapper-audio` Cargo feature (default ON); when off,
//!   register decoders still latch state (save-state round-trip
//!   preserved) but oscillators do not advance and `mix_audio`
//!   returns silence.
//!
//! # Register layout (v0 cheat sheet)
//!
//! | Range            | Purpose                                                |
//! |------------------|--------------------------------------------------------|
//! | `$5000-$5015`    | Audio: 2 pulse + raw PCM (Track C2)                    |
//! | `$5100`          | PRG mode (low 2 bits)                                  |
//! | `$5101`          | CHR mode (low 2 bits)                                  |
//! | `$5102`          | PRG-RAM protect 1: must equal `0b01`                   |
//! | `$5103`          | PRG-RAM protect 2: must equal `0b10`                   |
//! | `$5104`          | ExRAM mode (low 2 bits)                                |
//! | `$5105`          | Nametable mapping (4 x 2 bits)                         |
//! | `$5106`          | Fill-mode tile (active)                                |
//! | `$5107`          | Fill-mode attribute (active)                           |
//! | `$5113`          | PRG-RAM bank select @ `$6000-$7FFF`                    |
//! | `$5114-$5117`    | PRG bank select 0..3 @ `$8000-$FFFF`                   |
//! | `$5120-$5127`    | Sprite CHR banks (active — used for 8x16 sprite fetch)  |
//! | `$5128-$512B`    | BG CHR banks (active)                                  |
//! | `$5130`          | CHR-bank upper bits (active)                            |
//! | `$5200-$5202`    | Vertical split-screen mode / scroll / CHR bank         |
//! | `$5203`          | Scanline IRQ compare value                             |
//! | `$5204`          | Scanline IRQ status (read) / enable (write bit 7)      |
//! | `$5205-$5206`    | 8x8 multiplier (factor1 / factor2)                     |
//! | `$5C00-$5FFF`    | ExRAM (1 KiB)                                          |
//! | `$6000-$7FFF`    | PRG-RAM (banked via `$5113`)                           |
//! | `$8000-$FFFF`    | PRG-ROM (banked per `$5100` mode)                      |

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::struct_excessive_bools,
    clippy::match_same_arms,
    clippy::manual_range_patterns,
    clippy::too_many_arguments,
    clippy::useless_let_if_seq,
    clippy::doc_markdown,
    clippy::if_not_else,
    clippy::nonminimal_bool,
    clippy::cognitive_complexity
)]

use crate::cartridge::Mirroring;
use crate::mapper::{
    BgSplitState, ExAttribute, Mapper, MapperCaps, MapperError, MapperFrameEvents,
};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_8K: usize = 0x2000;
const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_1K: usize = 0x0400;
const PRG_RAM_BANK: usize = 0x2000;
const EXRAM_SIZE: usize = 0x0400;
const NAMETABLE_SIZE: usize = 0x0400;

const SAVE_STATE_VERSION: u8 = 4;

/// 32-entry length-counter lookup table (same as the 2A03 APU).
/// Indexed by the top 5 bits of `$5003` / `$5007` writes.
const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

/// 4-duty x 8-step duty waveform table, identical to the 2A03 pulse channel.
const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0], // 12.5%
    [0, 1, 1, 0, 0, 0, 0, 0], // 25.0%
    [0, 1, 1, 1, 1, 0, 0, 0], // 50.0%
    [1, 0, 0, 1, 1, 1, 1, 1], // 25.0% negated
];

/// MMC5 audio extension state. See top-level module docs §"Implemented
/// (continued, audio)" for protocol details.
///
/// Layout & semantics follow nesdev wiki "MMC5 audio":
/// - `$5000-$5003`: Pulse 1 (control, unused, timer-lo, length+timer-hi).
///   Identical to APU `$4000-$4003` but with NO sweep unit at `$5001`.
/// - `$5004-$5007`: Pulse 2 (same shape).
/// - `$5010`: PCM control (bit 0: 0 = write-mode / output PCM, 1 = read-mode
///   / silenced; bit 7: IRQ enable — not modelled in v0, no IRQ source on
///   our side).
/// - `$5011`: PCM 8-bit raw sample (only the low 7 bits contribute to output
///   per Mesen2 / nesdev: writing `$00` mutes the channel, which programs
///   use as the canonical silence value).
/// - `$5015`: Status. Bit 0 = pulse-1 length > 0; bit 1 = pulse-2 length > 0.
///   On write, bits 0/1 enable/disable each pulse length counter (same
///   contract as `$4015` for the 2A03 pulses; no DMC bit since there's no
///   DMC).
#[derive(Debug, Clone, Default)]
pub(crate) struct Mmc5Audio {
    pub(crate) pulse1: Mmc5Pulse,
    pub(crate) pulse2: Mmc5Pulse,
    /// `$5010` raw byte. Bit 0 = read-mode (silences PCM); bit 7 = PCM IRQ
    /// enable (not modelled).
    pub(crate) pcm_ctrl: u8,
    /// `$5011` last write — 7-bit linear PCM level (low 7 bits used).
    pub(crate) pcm_sample: u8,
}

/// MMC5 audio pulse channel. Same architecture as the 2A03 pulse (duty
/// sequencer + 11-bit timer + envelope + length counter), but with NO
/// sweep unit. Length and envelope tick on the APU frame-counter events
/// fanned out via [`crate::mapper::Mapper::notify_frame_event`].
#[derive(Debug, Clone, Default)]
pub(crate) struct Mmc5Pulse {
    /// Duty selection (bits 6-7 of `$5000` / `$5004`).
    duty: u8,
    /// Length-halt + envelope-loop bit (bit 5 of `$5000` / `$5004`).
    halt: bool,
    /// Envelope constant-volume flag (bit 4 of `$5000` / `$5004`).
    envelope_constant: bool,
    /// Envelope volume (constant) or decay period (decay rate) — bits 0-3
    /// of `$5000` / `$5004`.
    envelope_volume_or_period: u8,
    /// 11-bit timer reload period (from `$5002`/`$5003` low+high writes).
    timer_period: u16,
    /// Internal countdown timer.
    timer: u16,
    /// 3-bit step into `DUTY_TABLE`.
    step: u8,
    /// Length counter (5-bit lookup -> 0..=254). 0 mutes the channel.
    pub(crate) length: u8,
    /// Length-counter channel-enable from `$5015` writes.
    length_enabled: bool,
    /// Envelope start flag (set on `$5003`/`$5007` write).
    envelope_start: bool,
    /// Envelope divider countdown.
    envelope_divider: u8,
    /// Envelope decay level (0..=15).
    envelope_decay: u8,
}

impl Mmc5Pulse {
    /// `$5000` / `$5004` write: duty + length-halt + envelope.
    pub(crate) fn write_ctrl(&mut self, value: u8) {
        self.duty = (value >> 6) & 0x03;
        self.halt = (value & 0x20) != 0;
        self.envelope_constant = (value & 0x10) != 0;
        self.envelope_volume_or_period = value & 0x0F;
    }

    /// `$5002` / `$5006` write: timer-period low byte.
    pub(crate) fn write_timer_lo(&mut self, value: u8) {
        self.timer_period = (self.timer_period & 0xFF00) | u16::from(value);
    }

    /// `$5003` / `$5007` write: length-load + timer-period high 3 bits.
    /// Also resets the duty step and primes the envelope.
    pub(crate) fn write_timer_hi(&mut self, value: u8) {
        self.timer_period = (self.timer_period & 0x00FF) | (u16::from(value & 0x07) << 8);
        if self.length_enabled {
            self.length = LENGTH_TABLE[(value >> 3) as usize];
        }
        self.step = 0;
        self.envelope_start = true;
    }

    /// `$5015` write: per-channel length-enable. Clearing the bit forces
    /// the length count to 0 (same contract as `$4015` for the 2A03).
    pub(crate) fn set_length_enabled(&mut self, enabled: bool) {
        self.length_enabled = enabled;
        if !enabled {
            self.length = 0;
        }
    }

    /// One APU clock (every other CPU cycle) — advance the timer / duty
    /// sequencer. Caller is responsible for the every-other-cycle gating.
    pub(crate) fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            self.step = (self.step + 1) & 0x07;
        } else {
            self.timer -= 1;
        }
    }

    /// Quarter-frame: clock envelope.
    pub(crate) fn clock_envelope(&mut self) {
        if self.envelope_start {
            self.envelope_start = false;
            self.envelope_decay = 15;
            self.envelope_divider = self.envelope_volume_or_period;
        } else if self.envelope_divider == 0 {
            self.envelope_divider = self.envelope_volume_or_period;
            if self.envelope_decay > 0 {
                self.envelope_decay -= 1;
            } else if self.halt {
                self.envelope_decay = 15;
            }
        } else {
            self.envelope_divider -= 1;
        }
    }

    /// Half-frame: clock length counter (no sweep — MMC5 pulses have no
    /// sweep unit).
    pub(crate) fn clock_length(&mut self) {
        if !self.halt && self.length > 0 {
            self.length -= 1;
        }
    }

    /// Effective envelope output volume (0..=15) — constant or decay.
    fn envelope_output(&self) -> u8 {
        if self.envelope_constant {
            self.envelope_volume_or_period
        } else {
            self.envelope_decay
        }
    }

    /// True iff the channel is currently muted: length 0, timer period < 8,
    /// or duty waveform low. Matches the 2A03 pulse gating except for the
    /// absent sweep mute.
    fn muted(&self) -> bool {
        self.length == 0 || self.timer_period < 8
    }

    /// Per-cycle 4-bit output (0..=15). 0 when muted.
    pub(crate) fn output(&self) -> u8 {
        if self.muted() || DUTY_TABLE[self.duty as usize][self.step as usize] == 0 {
            0
        } else {
            self.envelope_output()
        }
    }
}

impl Mmc5Audio {
    /// Encode the audio state to a save-state tail. Versioned by the
    /// surrounding MMC5 save-state (see `SAVE_STATE_VERSION`). Layout per
    /// pulse: ctrl-byte(1) + halt(1) + envelope-constant(1) +
    /// envelope-volume(1) + timer_period(2) + timer(2) + step(1) +
    /// length(1) + length_enabled(1) + envelope_start(1) +
    /// envelope_divider(1) + envelope_decay(1) = 14 bytes.
    /// Plus pcm_ctrl(1) + pcm_sample(1) = 2 bytes.
    /// Total audio tail = 2 * 14 + 2 = 30 bytes.
    const TAIL_LEN: usize = 30;

    fn write_tail(&self, out: &mut Vec<u8>) {
        Self::write_pulse(out, &self.pulse1);
        Self::write_pulse(out, &self.pulse2);
        out.push(self.pcm_ctrl);
        out.push(self.pcm_sample);
    }

    fn write_pulse(out: &mut Vec<u8>, p: &Mmc5Pulse) {
        // Re-emit the source register bytes for ctrl so a round-trip stays
        // self-describing. We don't store the literal $5000 byte; instead
        // we serialize the decoded fields directly (same shape as VRC6).
        let ctrl = (p.duty << 6)
            | (u8::from(p.halt) << 5)
            | (u8::from(p.envelope_constant) << 4)
            | (p.envelope_volume_or_period & 0x0F);
        out.push(ctrl);
        out.push(u8::from(p.halt));
        out.push(u8::from(p.envelope_constant));
        out.push(p.envelope_volume_or_period);
        out.extend_from_slice(&p.timer_period.to_le_bytes());
        out.extend_from_slice(&p.timer.to_le_bytes());
        out.push(p.step);
        out.push(p.length);
        out.push(u8::from(p.length_enabled));
        out.push(u8::from(p.envelope_start));
        out.push(p.envelope_divider);
        out.push(p.envelope_decay);
    }

    fn read_tail(&mut self, data: &[u8]) -> Result<(), MapperError> {
        if data.len() != Self::TAIL_LEN {
            return Err(MapperError::Invalid(format!(
                "MMC5 audio tail expected {} bytes, got {}",
                Self::TAIL_LEN,
                data.len()
            )));
        }
        Self::read_pulse(&data[0..14], &mut self.pulse1);
        Self::read_pulse(&data[14..28], &mut self.pulse2);
        self.pcm_ctrl = data[28];
        self.pcm_sample = data[29];
        Ok(())
    }

    fn read_pulse(data: &[u8], p: &mut Mmc5Pulse) {
        // We re-derive the decoded fields from the ctrl byte to match the
        // write side; the ctrl byte itself is informational. The next
        // bytes carry the authoritative state.
        let ctrl = data[0];
        p.duty = (ctrl >> 6) & 0x03;
        p.halt = data[1] != 0;
        p.envelope_constant = data[2] != 0;
        p.envelope_volume_or_period = data[3] & 0x0F;
        p.timer_period = u16::from_le_bytes([data[4], data[5]]);
        p.timer = u16::from_le_bytes([data[6], data[7]]);
        p.step = data[8] & 0x07;
        p.length = data[9];
        p.length_enabled = data[10] != 0;
        p.envelope_start = data[11] != 0;
        p.envelope_divider = data[12];
        p.envelope_decay = data[13] & 0x0F;
    }
}

/// Per-1 KiB nametable source as decoded from `$5105`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NtSource {
    /// CIRAM bank 0 (logical NT_A).
    CiramA,
    /// CIRAM bank 1 (logical NT_B).
    CiramB,
    /// On-cart ExRAM (only meaningful when `$5104` is mode 0 or 1).
    ExRam,
    /// Fill-mode (deferred — currently returns 0).
    Fill,
}

impl NtSource {
    const fn from_bits(b: u8) -> Self {
        match b & 0x03 {
            0 => Self::CiramA,
            1 => Self::CiramB,
            2 => Self::ExRam,
            _ => Self::Fill,
        }
    }
}

/// `$5114-$5117` PRG bank slot. Bit 7 selects ROM (1) vs. RAM (0); the low
/// 7 bits index 8 KiB pages within the selected medium.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PrgSlot {
    /// Raw register value (we keep this for save-state symmetry).
    raw: u8,
}

impl PrgSlot {
    const fn is_rom(self) -> bool {
        // For `$5117` (the fixed-last slot) MMC5 forces ROM regardless of
        // bit 7; the caller handles that. For the other slots the bit
        // selects ROM (1) vs PRG-RAM (0).
        (self.raw & 0x80) != 0
    }
    const fn page(self) -> usize {
        (self.raw & 0x7F) as usize
    }
}

/// MMC5 mapper (iNES mapper 5).
pub struct Mmc5 {
    // === ROM / RAM storage ===
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    prg_ram: Box<[u8]>,
    /// 2 KiB on-cart CIRAM (indexed via `nametable_address`).
    /// Sized to the maximum the bus exposes (2 KiB) — extra nametables go
    /// through ExRAM, not VRAM.
    vram: Box<[u8]>,
    /// 1 KiB ExRAM. Always allocated; access pattern depends on `$5104`.
    exram: [u8; EXRAM_SIZE],

    // === CPU bus state ===
    /// `$5100` low 2 bits.
    prg_mode: u8,
    /// `$5101` low 2 bits.
    chr_mode: u8,
    /// `$5102` last write (low 2 bits), and `$5103` last write — both must
    /// match the magic pair to unlock PRG-RAM writes.
    prg_ram_protect_1: u8,
    prg_ram_protect_2: u8,
    /// `$5104` low 2 bits.
    exram_mode: u8,
    /// `$5105` raw byte (4 fields of 2 bits, low->high = NT0..NT3).
    nametable_map: u8,
    /// `$5106` fill-mode tile (deferred).
    fill_tile: u8,
    /// `$5107` fill-mode attribute (deferred — bottom 2 bits replicated).
    fill_attr: u8,
    /// `$5113` PRG-RAM bank (low 7 bits used; only one 8 KiB PRG-RAM bank
    /// supported in v0 — high bits ignored).
    prg_ram_bank: u8,
    /// `$5114-$5117` PRG bank registers.
    prg_banks: [PrgSlot; 4],
    /// `$5128-$512B` BG CHR bank registers (also serve sprites in v0).
    /// 10-bit values: low 8 bits from the register, high 2 bits from `$5130`
    /// (deferred — currently always 0).
    bg_chr_banks: [u16; 4],
    /// `$5120-$5127` sprite CHR bank registers — stored for save-state
    /// symmetry but not used in v0 (deferred). Eight registers because
    /// the sprite CHR layout is up to 8x1 KiB in CHR mode 3.
    sprite_chr_banks: [u16; 8],
    /// `$5130` upper 2 bits for CHR bank registers (deferred — value
    /// preserved but high bits not applied to the bank index).
    chr_upper: u8,
    /// Last register set written to (BG `$5128-$512B` or sprite
    /// `$5120-$5127`). Per nesdev, in 8x16 sprite mode the most-recent of
    /// these write groups determines which bank set the BG fetch path
    /// consults — but for our purposes BG always reads from BG and
    /// sprites always read from sprite, so this is purely informational.
    /// We keep it for save-state diagnostic continuity.
    last_chr_write_was_sprite: bool,
    /// MMC5 ExGrafix per-tile CHR bank latch. Set by `peek_ex_attribute`
    /// at NT-fetch time, consumed by the next BG `ppu_read` call(s).
    /// 4 KiB bank index (combined with the in-tile 12-bit offset).
    /// `None` outside ExGrafix mode 01 or before the first tile latch.
    ex_chr_bank_latch: Option<u16>,

    // === Vertical split-screen ($5200-$5202) ===
    /// `$5200` bit 7 — split enable.
    split_enable: bool,
    /// `$5200` bit 6 — split side: `false` = alt region occupies tile
    /// columns `< split_tile` (left); `true` = alt region occupies tile
    /// columns `>= split_tile` (right).
    split_side_right: bool,
    /// `$5200` bits 4-0 — split tile column (0..=31).
    split_tile: u8,
    /// `$5201` — vertical scroll within the alt region (0..=239 useful).
    split_v_scroll: u8,
    /// `$5202` — 4 KiB CHR bank for the alt region's BG pattern fetches.
    split_chr_bank: u8,
    /// 4 KiB CHR bank latch for the current BG fetch group when split is
    /// active. Set by `bg_split_state` at the NT-byte boundary; cleared on
    /// any subsequent non-split fetch. Distinct from `ex_chr_bank_latch`
    /// (split takes precedence when both would apply).
    split_chr_bank_latch: Option<u8>,

    // === Scanline IRQ state ===
    /// `$5203` compare value.
    irq_compare: u8,
    /// True when `$5204` bit 7 is set.
    irq_enabled: bool,
    /// True when the IRQ "pending" latch is set (read at `$5204` bit 7,
    /// cleared by reading `$5204`).
    irq_pending: bool,
    /// "In-frame" flag — set on the first rendered scanline after VBL,
    /// cleared on VBL. Read at `$5204` bit 6.
    in_frame: bool,
    /// Internal scanline counter; ticks each rendered scanline.
    scanline_counter: u8,

    // === Multiplier ===
    /// `$5205` factor 1.
    mul_a: u8,
    /// `$5206` factor 2.
    mul_b: u8,

    // === Mirroring (last-resort fallback for headers / save state) ===
    /// MMC5 has fully runtime-controlled mirroring; this field is a derived
    /// summary for `current_mirroring` (callers like the save-state loader
    /// or debuggers).
    current_mirroring_summary: Mirroring,

    // === Audio extension ($5000-$5015) ===
    /// 2 pulse channels + raw PCM. See [`Mmc5Audio`] docs.
    audio: Mmc5Audio,
    /// APU-phase toggle. MMC5 pulses (like the 2A03 pulses) clock their
    /// timer / duty sequencer every other CPU cycle. We toggle this on
    /// each `notify_cpu_cycle`.
    audio_apu_phase: bool,
}

impl Mmc5 {
    /// Construct a new MMC5 mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 8 KiB. CHR-RAM is selected
    /// when `chr_rom` is empty; otherwise CHR-ROM length must be a multiple
    /// of 1 KiB. `prg_ram_bytes == 0` selects the default 8 KiB.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        initial_mirroring: Mirroring,
        prg_ram_bytes: usize,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "MMC5 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            // Some carts use 8 KiB CHR-RAM with MMC5; allocate a default.
            vec![0u8; 8 * CHR_BANK_1K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "MMC5 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        let prg_ram_size = if prg_ram_bytes == 0 {
            PRG_RAM_BANK
        } else {
            prg_ram_bytes
        };
        let total_prg_pages = prg_rom.len() / PRG_BANK_8K;
        let last_page = (total_prg_pages.saturating_sub(1)) as u8;

        // Power-on defaults: PRG mode 3 (4x8K), `$5117` -> last bank ROM,
        // other slots zero. CHR mode 0 (single 8K bank).
        let mut prg_banks = [PrgSlot::default(); 4];
        prg_banks[3] = PrgSlot {
            raw: 0x80 | (last_page & 0x7F),
        };

        Ok(Self {
            prg_rom,
            chr,
            chr_is_ram,
            prg_ram: vec![0u8; prg_ram_size].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            exram: [0u8; EXRAM_SIZE],
            prg_mode: 3,
            chr_mode: 0,
            prg_ram_protect_1: 0,
            prg_ram_protect_2: 0,
            exram_mode: 0,
            // Default mirroring (vertical) is `0b01_00_01_00` = NT_B, NT_A,
            // NT_B, NT_A — but real MMC5 power-on is undefined. Use a
            // mirroring of "all NT_A" which is the most defensive default.
            nametable_map: 0,
            fill_tile: 0,
            fill_attr: 0,
            prg_ram_bank: 0,
            prg_banks,
            bg_chr_banks: [0; 4],
            sprite_chr_banks: [0; 8],
            chr_upper: 0,
            last_chr_write_was_sprite: false,
            ex_chr_bank_latch: None,
            split_enable: false,
            split_side_right: false,
            split_tile: 0,
            split_v_scroll: 0,
            split_chr_bank: 0,
            split_chr_bank_latch: None,
            irq_compare: 0,
            irq_enabled: false,
            irq_pending: false,
            in_frame: false,
            scanline_counter: 0,
            mul_a: 0,
            mul_b: 0,
            current_mirroring_summary: initial_mirroring,
            audio: Mmc5Audio::default(),
            audio_apu_phase: false,
        })
    }

    /// True iff the PRG-RAM protect "magic pair" is currently unlocked.
    /// Hardware: `$5102` low 2 bits == 0b10 AND `$5103` low 2 bits == 0b01.
    /// Any other value locks the RAM; the typical unlock sequence is two
    /// adjacent writes of the right values.
    fn prg_ram_writable(&self) -> bool {
        (self.prg_ram_protect_1 & 0x03) == 0b10 && (self.prg_ram_protect_2 & 0x03) == 0b01
    }

    /// Resolve a CPU PRG address (`$8000-$FFFF`) to either a ROM byte offset
    /// or, for PRG-RAM mapped into the window (PRG modes that allow it),
    /// a `(slot, offset)` indication. Returns `Some(byte)` if PRG-RAM was
    /// hit; `None` if the caller should fall through to ROM.
    fn read_prg_window(&self, addr: u16) -> u8 {
        let (slot, slot_size, region_off) = self.prg_window_lookup(addr);
        let raw = self.prg_banks[slot];
        // `$5117` is forced ROM regardless of bit 7.
        let force_rom = slot == 3;
        if force_rom || raw.is_rom() {
            // ROM path. The slot indexes 8 KiB pages, but in larger windows
            // (16 K / 32 K) the low bits of `page` are masked to align.
            let page = raw.page();
            let (page, mask) = match slot_size {
                PRG_BANK_8K => (page, !0usize),
                PRG_BANK_16K => (page & !1, !0usize),
                PRG_BANK_32K => (page & !3, !0usize),
                _ => (page, !0usize),
            };
            let base = (page * PRG_BANK_8K) & mask;
            let off = (base + region_off) % self.prg_rom.len();
            self.prg_rom[off]
        } else {
            // PRG-RAM at this slot. v0 supports a single 8 KiB PRG-RAM bank;
            // the page bits are ignored.
            if region_off < self.prg_ram.len() {
                self.prg_ram[region_off & (self.prg_ram.len() - 1)]
            } else {
                0
            }
        }
    }

    /// Decode a CPU PRG address into `(slot_index, slot_size, offset_within_region)`
    /// where `slot_index` is the index into `self.prg_banks` and
    /// `slot_size` is the size of the *physical* bank window (8/16/32 K).
    fn prg_window_lookup(&self, addr: u16) -> (usize, usize, usize) {
        let off16 = (addr - 0x8000) as usize; // 0..0x8000
        match self.prg_mode & 0x03 {
            0 => {
                // 32 K window driven by `$5117` (slot 3, but page bits & ~3).
                (3, PRG_BANK_32K, off16)
            }
            1 => {
                // 16 K + 16 K. `$5115` (slot 1) -> $8000-$BFFF (page & ~1);
                // `$5117` (slot 3) -> $C000-$FFFF (page & ~1).
                if off16 < PRG_BANK_16K {
                    (1, PRG_BANK_16K, off16)
                } else {
                    (3, PRG_BANK_16K, off16 - PRG_BANK_16K)
                }
            }
            2 => {
                // 16 K + 8 K + 8 K. `$5115` (slot 1) -> $8000-$BFFF (page & ~1);
                // `$5116` (slot 2) -> $C000-$DFFF; `$5117` (slot 3) -> $E000-$FFFF.
                if off16 < PRG_BANK_16K {
                    (1, PRG_BANK_16K, off16)
                } else if off16 < PRG_BANK_16K + PRG_BANK_8K {
                    (2, PRG_BANK_8K, off16 - PRG_BANK_16K)
                } else {
                    (3, PRG_BANK_8K, off16 - PRG_BANK_16K - PRG_BANK_8K)
                }
            }
            _ => {
                // Mode 3: 8 K x 4. `$5114` -> $8000; `$5115` -> $A000;
                // `$5116` -> $C000; `$5117` -> $E000.
                let slot = off16 / PRG_BANK_8K; // 0..=3
                (slot, PRG_BANK_8K, off16 % PRG_BANK_8K)
            }
        }
    }

    /// Write into the PRG window. PRG-RAM writes honor the protect pair;
    /// ROM writes are silently dropped.
    fn write_prg_window(&mut self, addr: u16, value: u8) {
        let (slot, _slot_size, region_off) = self.prg_window_lookup(addr);
        // `$5117` is always ROM; never writable.
        if slot == 3 {
            return;
        }
        let raw = self.prg_banks[slot];
        if raw.is_rom() {
            return;
        }
        if !self.prg_ram_writable() {
            return;
        }
        if region_off < self.prg_ram.len() {
            let len = self.prg_ram.len();
            self.prg_ram[region_off & (len - 1)] = value;
        }
    }

    /// CHR fetch flavors. The PPU uses different bank-register banks for
    /// sprite vs. BG fetches when 8x16 sprites are enabled.
    ///
    /// Note: outside 8x16 mode real MMC5 hardware unifies the two register
    /// banks (sprite writes update BG too). We approximate that by reading
    /// the BG registers for both BG and sprite fetches in 8x8 sprite mode.
    /// In 8x16 mode, sprite fetches use the sprite bank registers
    /// (`$5120-$5127`) and BG fetches use BG (`$5128-$512B`).
    ///
    /// `Sprite` callers should pass `kind = ChrFetchKind::Sprite`. The
    /// 8x8-vs-8x16 decision is taken by the caller (the PPU is responsible
    /// for routing only sprite-tile fetches into `Sprite` regardless).
    /// In MMC5's interpretation, when 8x16 mode is *off*, BG and sprite
    /// fetches share the BG bank set; we model this by always using the
    /// BG bank set unless 8x16 is on.
    ///
    /// Caller: PPU sprite tile fetch path.
    fn chr_offset_sprite(&self, addr: u16) -> usize {
        // In 8x16 mode there are 8 sprite-CHR registers (`$5120-$5127`)
        // and they always behave as 1 KiB banks regardless of `$5101` —
        // per nesdev §"CHR Bank Switching": in 8x16 mode, sprite tile
        // pattern fetches always access CHR via the eight sprite bank
        // registers indexed by the high 3 bits of the pattern address.
        //
        // We assume the PPU only routes sprite *tile* fetches here; the
        // mode decision is captured by checking whether the BG path is
        // being driven differently. To stay simple and faithful, when
        // sprite_chr_banks[*] match the BG group (i.e. user wrote to the
        // BG group only), this will produce equivalent results.
        let a = (addr & 0x1FFF) as usize;
        let slot = (a / CHR_BANK_1K) & 0x07;
        let bank = self.sprite_chr_banks[slot] as usize;
        let total_banks_1k = self.chr.len() / CHR_BANK_1K;
        let mask = total_banks_1k.saturating_sub(1);
        let bank = bank & mask;
        let byte_off_within_bank = a & (CHR_BANK_1K - 1);
        bank * CHR_BANK_1K + byte_off_within_bank
    }

    /// Resolve a PPU CHR address (`$0000-$1FFF`) to a byte offset in `chr`,
    /// using the BG bank registers (`$5128-$512B`).
    ///
    /// In MMC5 ExGrafix mode (`$5104` mode 01) the per-tile CHR bank
    /// latched at the most recent NT-byte fetch overrides the standard
    /// BG bank decoding; we apply that override here.
    fn chr_offset(&self, addr: u16) -> usize {
        // Vertical split-screen override: take precedence over both
        // ExGrafix and standard BG bank decoding. 4 KiB bank from $5202
        // (latched at NT-fetch time by `bg_split_state`).
        if let Some(bank4k) = self.split_chr_bank_latch {
            let total_banks_1k = self.chr.len() / CHR_BANK_1K;
            let mask = total_banks_1k.saturating_sub(1);
            let bank_1k = ((bank4k as usize) * 4) & mask;
            let off_within_4k = (addr & 0x0FFF) as usize;
            return bank_1k * CHR_BANK_1K + off_within_4k;
        }
        // ExGrafix override: per-tile 4 KiB bank from the latch.
        if let Some(bank4k) = self.ex_chr_bank_latch {
            let total_banks_1k = self.chr.len() / CHR_BANK_1K;
            let mask = total_banks_1k.saturating_sub(1);
            // Bank is in 4 KiB units; convert to 1 KiB index.
            let bank_1k = ((bank4k as usize) * 4) & mask;
            let off_within_4k = (addr & 0x0FFF) as usize;
            return bank_1k * CHR_BANK_1K + off_within_4k;
        }

        let addr = (addr & 0x1FFF) as usize;
        let total_banks_1k = self.chr.len() / CHR_BANK_1K;
        let mask = total_banks_1k.saturating_sub(1);
        let mode = self.chr_mode & 0x03;

        // Each CHR mode lays out the 8 KiB pattern window from a small set
        // of bank registers. We always use the "BG" bank set in v0.
        // Bank-register selection per nesdev:
        //   Mode 0 (8 K):     register $512B drives all 8 KiB.
        //   Mode 1 (4+4 K):   $5129 drives $0000-$0FFF, $512B drives $1000-$1FFF.
        //   Mode 2 (2 K x 4): $5128/$5129/$512A/$512B drive each 2 KiB tile of
        //                     the BG window.
        //   Mode 3 (1 K x 8): $5128/$5129/$512A/$512B repeat: each register's
        //                     value is used as a 1 KiB bank, but only four
        //                     registers exist for the BG side, so banks repeat
        //                     with the second 4 KiB mirroring the first.
        //
        // We model the four BG registers indexed 0..=3 corresponding to
        // `$5128`..`$512B`.
        let (slot_size_in_1k, bank_index) = match mode {
            0 => {
                // Single 8 KiB bank. Use register index 3 ($512B), mask
                // bottom 3 bits (8K = 8 x 1K).
                let bank = (self.bg_chr_banks[3] as usize) & !0x07;
                (8usize, bank)
            }
            1 => {
                // Two 4 KiB banks: low half from register 1 ($5129),
                // high half from register 3 ($512B). Mask bottom 2 bits.
                let reg = if addr < 0x1000 { 1 } else { 3 };
                let bank = (self.bg_chr_banks[reg] as usize) & !0x03;
                (4usize, bank)
            }
            2 => {
                // Four 2 KiB banks. Region index = (addr / 0x800) & 3.
                let reg = (addr / 0x0800) & 0x03;
                let bank = (self.bg_chr_banks[reg] as usize) & !0x01;
                (2usize, bank)
            }
            _ => {
                // Eight 1 KiB banks. With only 4 BG registers the pattern
                // repeats (per nesdev: BG fetches in 1K mode take `$5128
                // + (slot & 3)` for each 1K slot).
                let slot = (addr / CHR_BANK_1K) & 0x03;
                let bank = self.bg_chr_banks[slot] as usize;
                (1usize, bank)
            }
        };

        let bank = bank_index & mask;
        let byte_off_within_bank = addr & ((slot_size_in_1k * CHR_BANK_1K) - 1);
        bank * CHR_BANK_1K + byte_off_within_bank
    }

    /// Decode the per-1KiB nametable source for logical table 0..=3.
    fn nt_source(&self, table: u8) -> NtSource {
        let bits = (self.nametable_map >> ((table & 0x03) * 2)) & 0x03;
        NtSource::from_bits(bits)
    }

    /// Resolve a PPU nametable address `$2000-$3EFF` to either
    /// (a) a CIRAM offset 0..0x800, or (b) an ExRAM offset, or (c) fill mode.
    fn nt_resolve(&self, addr: u16) -> (NtSource, usize) {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE as u16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let src = self.nt_source(table);
        (src, local)
    }

    /// True iff ExRAM is currently configured for use as a nametable
    /// (mode 0 or 1 — extended attributes is also a "nametable-mapped"
    /// mode for routing purposes; the per-tile-attribute interpretation
    /// is what's deferred).
    fn exram_is_nametable(&self) -> bool {
        matches!(self.exram_mode & 0x03, 0 | 1)
    }
}

impl Mapper for Mmc5 {
    fn sram(&self) -> &[u8] {
        &self.prg_ram
    }
    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.prg_ram
    }
    // v2.8.0 Phase 4 — MMC5: CPU-cycle hook + IRQ + frame-counter-
    // cadenced audio envelopes (+ expansion audio under `mapper-audio`).
    fn caps(&self) -> MapperCaps {
        MapperCaps {
            cpu_cycle_hook: true,
            audio: cfg!(feature = "mapper-audio"),
            frame_event_hook: true,
            irq_source: true,
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // MMC5 maps almost the entire `$5000-$5FFF` window: audio at
        // `$5000-$5015`, ExGfx config at `$5100-$5107`, PRG bank regs
        // at `$5113-$5117`, CHR bank regs at `$5120-$512B`, upper-CHR
        // bits at `$5130`, multiplier at `$5205-$5206`, scanline IRQ
        // at `$5203-$5204`, split-screen at `$5200-$5207`, and ExRAM
        // at `$5C00-$5FFF`. The `$4020-$4FFF` range is not mapped
        // (per the default impl convention).
        (0x4020..=0x4FFF).contains(&addr)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        // v1.4.0 Workstream F (F2): PRG-ROM/RAM fetches at `$8000-$FFFF`
        // dominate `cpu_read` (every opcode + operand fetch on an MMC5 cart),
        // while the register/ExRAM arms only fire on explicit `$5xxx` accesses.
        // Short-circuit the hot case before the register-range match so the
        // common path is one compare, not a walk of the `$5xxx` decision tree.
        // Byte-identical to the `0x8000..=0xFFFF` match arm below.
        if addr >= 0x8000 {
            return self.read_prg_window(addr);
        }
        match addr {
            // Audio status (`$5015`): bit 0 = pulse-1 length > 0, bit 1 =
            // pulse-2 length > 0. No DMC bit (MMC5 has no DMC).
            0x5015 => {
                let mut v = 0u8;
                if self.audio.pulse1.length > 0 {
                    v |= 0x01;
                }
                if self.audio.pulse2.length > 0 {
                    v |= 0x02;
                }
                v
            }

            // Other audio range registers are write-only on real hardware
            // ($5000-$5014 except $5011 in PCM read-mode — which would be
            // a CPU-side sample-delivery port we don't model in v0).
            // Falls through to the catch-all $5000-$5FFF "open bus = 0".

            // Multiplier readback — most-significant byte and
            // least-significant byte of the 16-bit product.
            0x5205 => {
                let prod = u16::from(self.mul_a) * u16::from(self.mul_b);
                (prod & 0xFF) as u8
            }
            0x5206 => {
                let prod = u16::from(self.mul_a) * u16::from(self.mul_b);
                ((prod >> 8) & 0xFF) as u8
            }

            // IRQ status (and ack on read).
            0x5204 => {
                let mut v = 0u8;
                if self.irq_pending {
                    v |= 0x80;
                }
                if self.in_frame {
                    v |= 0x40;
                }
                self.irq_pending = false;
                v
            }

            // ExRAM CPU read window. Modes 10/11 are CPU-readable; modes
            // 00/01 are *also* CPU-readable per nesdev (writes are restricted
            // depending on rendering state, but reads always succeed).
            0x5C00..=0x5FFF => {
                let off = (addr - 0x5C00) as usize;
                self.exram[off]
            }

            // Other registers in `$5000-$5FFF` are write-only on real
            // hardware — return 0 (open bus is approximated as zero;
            // the lockstep bus latches its own open-bus value).
            0x5000..=0x5FFF => 0,

            // PRG-RAM at `$6000-$7FFF` (always 8 KiB; `$5113` selects
            // a bank, but v0 only supports the default single bank).
            0x6000..=0x7FFF => {
                let off = (addr - 0x6000) as usize;
                if off < self.prg_ram.len() {
                    self.prg_ram[off]
                } else {
                    0
                }
            }

            // PRG-ROM / PRG-RAM windowed by `$5114-$5117`.
            0x8000..=0xFFFF => self.read_prg_window(addr),

            _ => 0,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            // === Audio extension ($5000-$5015) ===
            // Pulse 1 (`$5000-$5003`). $5001 (sweep slot) is unused — MMC5
            // pulses have no sweep unit; write is silently absorbed so
            // round-tripping a memcpy of `$5000-$5003` stays no-op.
            0x5000 => self.audio.pulse1.write_ctrl(value),
            0x5001 => {} // no sweep unit on MMC5 pulse channels.
            0x5002 => self.audio.pulse1.write_timer_lo(value),
            0x5003 => self.audio.pulse1.write_timer_hi(value),
            // Pulse 2 (`$5004-$5007`).
            0x5004 => self.audio.pulse2.write_ctrl(value),
            0x5005 => {} // no sweep unit.
            0x5006 => self.audio.pulse2.write_timer_lo(value),
            0x5007 => self.audio.pulse2.write_timer_hi(value),
            // $5008-$500F: unused on real hardware (open bus). Absorb writes.
            0x5008..=0x500F => {}
            // $5010: PCM control. Bit 0 = mode select (0 = write/output;
            // 1 = read-mode/CPU-side sample delivery, which silences PCM
            // output). Bit 7 = PCM IRQ enable (not modelled — we have no
            // PCM-side IRQ source).
            0x5010 => {
                self.audio.pcm_ctrl = value;
            }
            // $5011: PCM data. In write-mode (`$5010` bit 0 = 0), the low
            // 7 bits drive the PCM channel output level. In read-mode the
            // write is ignored.
            0x5011 => {
                if (self.audio.pcm_ctrl & 0x01) == 0 {
                    self.audio.pcm_sample = value & 0x7F;
                }
            }
            // $5012-$5014: unused.
            0x5012..=0x5014 => {}
            // $5015: per-channel length-enable. Bit 0 -> pulse 1, bit 1 ->
            // pulse 2. Other bits ignored (no DMC).
            0x5015 => {
                self.audio.pulse1.set_length_enabled((value & 0x01) != 0);
                self.audio.pulse2.set_length_enabled((value & 0x02) != 0);
            }

            // PRG mode.
            0x5100 => {
                self.prg_mode = value & 0x03;
            }
            // CHR mode.
            0x5101 => {
                self.chr_mode = value & 0x03;
            }
            // PRG-RAM protect (two-write magic pair).
            0x5102 => {
                self.prg_ram_protect_1 = value;
            }
            0x5103 => {
                self.prg_ram_protect_2 = value;
            }
            // ExRAM mode.
            0x5104 => {
                self.exram_mode = value & 0x03;
                // Switching modes invalidates any cached ExGrafix CHR
                // bank latch.
                self.ex_chr_bank_latch = None;
            }
            // Nametable mapping.
            0x5105 => {
                self.nametable_map = value;
                // Update mirroring summary for `current_mirroring`.
                self.current_mirroring_summary = nt_summary(value);
            }
            // Fill mode (deferred but stored).
            0x5106 => {
                self.fill_tile = value;
            }
            0x5107 => {
                self.fill_attr = value & 0x03;
            }
            // PRG-RAM bank select (v0: only the default single bank).
            0x5113 => {
                self.prg_ram_bank = value & 0x7F;
            }
            // PRG bank select 0..3 -> $5114..$5117.
            0x5114..=0x5117 => {
                let idx = (addr - 0x5114) as usize;
                self.prg_banks[idx] = PrgSlot { raw: value };
            }
            // Sprite CHR banks. Used for sprite tile fetches when 8x16
            // sprites are enabled.
            0x5120..=0x5127 => {
                let idx = (addr - 0x5120) as usize;
                self.sprite_chr_banks[idx] = u16::from(value) | (u16::from(self.chr_upper) << 8);
                self.last_chr_write_was_sprite = true;
            }
            // BG CHR banks. Always used for BG tile fetches.
            0x5128..=0x512B => {
                let idx = (addr - 0x5128) as usize;
                self.bg_chr_banks[idx] = u16::from(value) | (u16::from(self.chr_upper) << 8);
                self.last_chr_write_was_sprite = false;
            }
            // Upper CHR bank bits (deferred — stored only).
            0x5130 => {
                self.chr_upper = value & 0x03;
            }
            // Vertical split-screen mode / scroll / CHR bank.
            // $5200 (mode): bit 7 = enable, bit 6 = side (0 = left, 1 = right),
            //               bits 4-0 = split tile column (0..=31).
            0x5200 => {
                self.split_enable = (value & 0x80) != 0;
                self.split_side_right = (value & 0x40) != 0;
                self.split_tile = value & 0x1F;
            }
            // $5201: vertical scroll within the alt region.
            0x5201 => {
                self.split_v_scroll = value;
            }
            // $5202: 4 KiB CHR bank for the alt region's BG pattern fetches.
            0x5202 => {
                self.split_chr_bank = value;
            }
            // Scanline IRQ compare value.
            0x5203 => {
                self.irq_compare = value;
            }
            // Scanline IRQ enable.
            0x5204 => {
                self.irq_enabled = (value & 0x80) != 0;
            }
            // Multiplier inputs.
            0x5205 => {
                self.mul_a = value;
            }
            0x5206 => {
                self.mul_b = value;
            }
            // ExRAM CPU writes. Behavior depends on mode and rendering
            // state; v0 simplifies:
            //   Mode 00/01: writable always (nametable mode — real hardware
            //               only allows during rendering, but games tend
            //               to update during VBL too; we accept writes).
            //   Mode 10:    writable always (general RAM).
            //   Mode 11:    read-only (writes ignored).
            0x5C00..=0x5FFF => {
                let off = (addr - 0x5C00) as usize;
                if (self.exram_mode & 0x03) != 0b11 {
                    self.exram[off] = value;
                }
            }
            // PRG-RAM at `$6000-$7FFF`.
            0x6000..=0x7FFF => {
                if self.prg_ram_writable() {
                    let off = (addr - 0x6000) as usize;
                    if off < self.prg_ram.len() {
                        self.prg_ram[off] = value;
                    }
                }
            }
            // PRG window writes (PRG-RAM banks may be mapped here).
            0x8000..=0xFFFF => self.write_prg_window(addr, value),

            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
                let len = self.chr.len();
                self.chr[off % len]
            }
            0x2000..=0x3EFF => {
                // Fill / ExRAM / CIRAM is decided by `$5105`. The lockstep
                // bus's PPU calls `peek_nametable` first; this `ppu_read`
                // path is only used by the test bus and direct mapper
                // probes, so we still service the same logic here.
                let (src, local) = self.nt_resolve(addr);
                match src {
                    NtSource::CiramA => self.vram[local],
                    NtSource::CiramB => self.vram[NAMETABLE_SIZE + local],
                    NtSource::ExRam => {
                        if self.exram_is_nametable() {
                            self.exram[local]
                        } else {
                            0
                        }
                    }
                    NtSource::Fill => {
                        if local < 0x03C0 {
                            self.fill_tile
                        } else {
                            let a = self.fill_attr & 0x03;
                            (a << 6) | (a << 4) | (a << 2) | a
                        }
                    }
                }
            }
            _ => 0,
        }
    }

    fn ppu_read_sprite(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x1FFF;
        let off = self.chr_offset_sprite(addr);
        let len = self.chr.len();
        self.chr[off % len]
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    let len = self.chr.len();
                    self.chr[off % len] = value;
                }
            }
            0x2000..=0x3EFF => {
                let (src, local) = self.nt_resolve(addr);
                match src {
                    NtSource::CiramA => self.vram[local] = value,
                    NtSource::CiramB => self.vram[NAMETABLE_SIZE + local] = value,
                    NtSource::ExRam => {
                        if self.exram_is_nametable() {
                            self.exram[local] = value;
                        }
                    }
                    NtSource::Fill => {
                        // Fill mode is a read-only synthetic surface; ignore.
                    }
                }
            }
            _ => {}
        }
    }

    fn nametable_address(&self, addr: u16) -> u16 {
        // CIRAM-bound mapping. For ExRAM and fill mode, the PPU consults
        // `nametable_fetch` first (returning a synthesized byte and
        // bypassing the CIRAM read entirely); this path is only used as a
        // fallback / for callers that have not adopted that hook.
        let (src, local) = self.nt_resolve(addr);
        let bank = match src {
            NtSource::CiramA | NtSource::ExRam | NtSource::Fill => 0,
            NtSource::CiramB => 1,
        };
        (bank * NAMETABLE_SIZE + local) as u16
    }

    fn nametable_fetch(&mut self, addr: u16) -> Option<u8> {
        // Vertical split-screen: when the current BG fetch group has been
        // marked as inside the alt region (split CHR bank latched), the
        // NT and AT bytes come from ExRAM regardless of the $5105 mapping
        // for the synthesized $2000-$23FF address the PPU passes in.
        if self.split_chr_bank_latch.is_some() {
            let local = (addr as usize) & (NAMETABLE_SIZE - 1);
            return Some(self.exram[local & (EXRAM_SIZE - 1)]);
        }
        let (src, local) = self.nt_resolve(addr);
        match src {
            NtSource::CiramA | NtSource::CiramB => None,
            NtSource::ExRam => {
                // ExRAM-as-nametable: synthesize the byte directly from
                // ExRAM. In ExGrafix mode (mode 01) the byte is also used
                // as a per-tile attribute by `peek_ex_attribute`; here we
                // just return the raw byte for any nametable / AT read.
                if self.exram_is_nametable() {
                    Some(self.exram[local])
                } else {
                    // ExRAM not configured as nametable (modes 10/11) but
                    // `$5105` points there — return open-bus 0.
                    Some(0)
                }
            }
            NtSource::Fill => {
                // Fill mode: the 32x30 nametable region returns the fill
                // tile (`$5106`); the 64-byte attribute region returns the
                // 2-bit fill attribute (`$5107`) replicated 4 ways.
                if local < 0x03C0 {
                    Some(self.fill_tile)
                } else {
                    let a = self.fill_attr & 0x03;
                    Some((a << 6) | (a << 4) | (a << 2) | a)
                }
            }
        }
    }

    fn nametable_write(&mut self, addr: u16, value: u8) -> bool {
        let (src, local) = self.nt_resolve(addr);
        match src {
            NtSource::CiramA | NtSource::CiramB => {
                // Defer to the PPU's CIRAM write.
                false
            }
            NtSource::ExRam => {
                if self.exram_is_nametable() {
                    self.exram[local] = value;
                }
                // Even when ExRAM-as-NT is not active for the current
                // mode, `$5105` pointing at ExRAM masks the CIRAM write —
                // real hardware drops it on the floor. We absorb it.
                true
            }
            NtSource::Fill => {
                // Fill mode: writes are dropped (read-only synthetic).
                true
            }
        }
    }

    fn peek_ex_attribute(&mut self, v: u16) -> Option<ExAttribute> {
        // ExGrafix is `$5104` mode 01.
        if (self.exram_mode & 0x03) != 0b01 {
            // Clear the chr-bank latch so subsequent BG fetches use the
            // standard BG bank registers.
            self.ex_chr_bank_latch = None;
            return None;
        }
        // The current tile within the active nametable is encoded in the
        // low 12 bits of v: low 5 = coarse-X, next 5 = coarse-Y, next 2 =
        // nametable select. ExRAM is 1 KiB and indexed by the same 10-bit
        // tile coordinate (32 cols * 30 rows = 960; ExRAM is 1024 — we
        // mod by 1024 to avoid OOB on the unused 64 entries).
        let coarse_x = (v & 0x001F) as usize;
        let coarse_y = ((v >> 5) & 0x001F) as usize;
        let tile_idx = (coarse_y * 32 + coarse_x) & (EXRAM_SIZE - 1);
        let byte = self.exram[tile_idx];
        // Bits 7-6 = palette (2 bits).
        let palette = (byte >> 6) & 0x03;
        // Bits 5-0 = upper 6 bits of the CHR bank for this tile.
        // Combined with `$5130` upper 2 bits (low 2 bits of `chr_upper`)
        // shifted left by 6 to form an 8-bit raw bank — but per nesdev
        // the ExGrafix CHR bank is 4 KiB units, with `$5130` bits 1-0
        // as the topmost 2 of an 8-bit bank index.
        let bank_low6 = u16::from(byte & 0x3F);
        let bank_high2 = u16::from(self.chr_upper & 0x03) << 6;
        let bank4k = bank_low6 | bank_high2;
        // Latch internally so the BG pattern fetches in this tile use it.
        self.ex_chr_bank_latch = Some(bank4k);
        Some(ExAttribute {
            palette,
            chr_bank: bank4k,
        })
    }

    fn bg_split_state(&mut self, scanline_y: u16, coarse_x: u16) -> Option<BgSplitState> {
        if !self.split_enable {
            // Drop any stale CHR latch from the previous tile group.
            self.split_chr_bank_latch = None;
            return None;
        }
        // Decide whether this tile column is in the alt region.
        // `split_side_right == false` (bit 6 = 0): alt region = columns < split_tile.
        // `split_side_right == true`              : alt region = columns >= split_tile.
        let cx = (coarse_x & 0x1F) as u8;
        let split_tile = self.split_tile & 0x1F;
        let in_alt = if self.split_side_right {
            cx >= split_tile
        } else {
            cx < split_tile
        };
        if !in_alt {
            self.split_chr_bank_latch = None;
            return None;
        }

        // Compute the alt region's logical row from the current scanline +
        // $5201 vertical scroll. The alt region is a flat 32x30 nametable
        // backed by ExRAM (`$5C00-$5FBF` for NT bytes, `$5FC0-$5FFF` for
        // attributes). Scrolling wraps at 240.
        let y_in_region = (u16::from(self.split_v_scroll) + scanline_y) % 240;
        let coarse_y = (y_in_region / 8) & 0x1F;
        let fine_y = (y_in_region % 8) as u8;

        // Synthesize an NT byte address inside the $2000-$23FF window — the
        // PPU's `peek_nametable` (calling `nametable_fetch`) will route this
        // to ExRAM (the alt region is *always* sourced from ExRAM, regardless
        // of $5105 — see nesdev MMC5 §"Vertical split mode").
        //
        // We anchor the address inside NT0 so the PPU's loopy-style decoding
        // (`coarse_y * 32 + coarse_x`) lands at the correct ExRAM index.
        // Our `nametable_fetch` below treats split-active addresses inside
        // NT0 specially.
        let nt_addr = 0x2000 | (coarse_y << 5) | u16::from(cx);
        let at_addr = 0x23C0 | ((coarse_y >> 2) << 3) | u16::from(cx >> 2);

        // Latch the $5202 4 KiB CHR bank for the pattern fetches in this
        // 8-dot group.
        self.split_chr_bank_latch = Some(self.split_chr_bank);

        Some(BgSplitState {
            nt_addr,
            at_addr,
            fine_y,
            chr_bank: self.split_chr_bank,
        })
    }

    fn current_mirroring(&self) -> Mirroring {
        self.current_mirroring_summary
    }

    fn notify_a12(&mut self, _level: bool) {
        // MMC5 does NOT use A12 for IRQ — it has its own scanline detector.
        // Intentionally empty.
    }

    fn notify_cpu_cycle(&mut self) {
        // No CPU-cycle IRQ for MMC5. However, the audio extension's two
        // pulse channels tick their 11-bit timer / duty sequencer every
        // *other* CPU cycle — same as the 2A03 pulses. Envelope &
        // length-counter clocks arrive via `notify_frame_event` and are
        // handled separately.
        #[cfg(feature = "mapper-audio")]
        {
            self.audio_apu_phase = !self.audio_apu_phase;
            if self.audio_apu_phase {
                self.audio.pulse1.clock_timer();
                self.audio.pulse2.clock_timer();
            }
        }
    }

    fn notify_frame_event(&mut self, events: MapperFrameEvents) {
        // MMC5 pulse channels share the 2A03 frame-counter cadence. Quarter
        // frame -> envelope clock; half frame -> length clock. No sweep.
        // Without the `mapper-audio` feature the channels do not advance,
        // but `length` still decrements (cheap, no-effect) since `output`
        // is gated independently — keeping this branchless under the
        // feature-OFF build is harmless. We still feature-gate explicitly
        // to make the audio surface a no-op under the off path.
        #[cfg(feature = "mapper-audio")]
        {
            if events.quarter {
                self.audio.pulse1.clock_envelope();
                self.audio.pulse2.clock_envelope();
            }
            if events.half {
                self.audio.pulse1.clock_length();
                self.audio.pulse2.clock_length();
            }
        }
        #[cfg(not(feature = "mapper-audio"))]
        {
            let _ = events;
        }
    }

    #[cfg(feature = "mapper-audio")]
    fn mix_audio(&mut self) -> i16 {
        // Two pulse outputs (each 0..=15) plus one 7-bit PCM level.
        //
        // Scaling rationale (NOTE: match Mesen2 convention — the nesdev
        // wiki page §"MMC5 audio" notes the pulses were intended to be at
        // the same gain as the 2A03 pulses; the PCM channel sits at
        // roughly half that gain because the 7-bit raw level lacks the
        // dynamic range of the proper DAC). We pick a linear scale that
        // matches the VRC6 mix in absolute magnitude:
        //   pulse range: 0..=15 -> contribute up to ~ 15 * 256 = 3840.
        //   PCM range:   0..=127 -> contribute up to ~ 127 * 16 ≈ 2032.
        // Sum peak ~ 9712; we center on zero by subtracting half-range so
        // the output sits in roughly +/- 4800 -- 1/8 of i16::MAX, in the
        // same ballpark as the VRC6 mixer (`((sum-30) * 256)`).
        //
        // PCM is silenced when `$5010` bit 0 = 1 (read-mode) -- the chip
        // is then sourcing samples back to the CPU rather than outputting.
        let p1 = i16::from(self.audio.pulse1.output());
        let p2 = i16::from(self.audio.pulse2.output());
        let pcm = if (self.audio.pcm_ctrl & 0x01) == 0 {
            i16::from(self.audio.pcm_sample) // 0..=127
        } else {
            0
        };
        // Linear sum, biased to zero. Half-range = ~(15+15)*256/2 + 127*16/2 = ~4856.
        let pulse_mix = (p1 + p2) * 256; // 0..=7680
        let pcm_mix = pcm * 16; // 0..=2032
        (pulse_mix + pcm_mix) - 4800
    }

    fn notify_scanline_start(&mut self) {
        // First rendered scanline after VBL: enter "in-frame" state and
        // reset the scanline counter to 0.
        if !self.in_frame {
            self.in_frame = true;
            self.scanline_counter = 0;
            // The compare register can match scanline 0 too — handle below.
        } else {
            self.scanline_counter = self.scanline_counter.wrapping_add(1);
        }
        if self.scanline_counter == self.irq_compare && self.irq_compare != 0 {
            self.irq_pending = true;
        }
    }

    fn notify_vblank(&mut self) {
        // Vertical blank: clear the in-frame flag (the next rendered line
        // re-enters "in-frame" via notify_scanline_start).
        self.in_frame = false;
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending && self.irq_enabled
    }

    fn irq_acknowledge(&mut self) {
        // MMC5 acks via reading $5204; we don't ack here.
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 5,
            name: "MMC5".into(),
            mirroring: crate::mapper::mirroring_name(self.current_mirroring()),
            ..Default::default()
        };
        info.prg_banks
            .push(("mode".into(), format!("{}", self.prg_mode)));
        for (i, slot) in self.prg_banks.iter().enumerate() {
            info.prg_banks
                .push((format!("$5114+{i}"), format!("{:#04x}", slot.raw)));
        }
        info.chr_banks
            .push(("mode".into(), format!("{}", self.chr_mode)));
        for (i, b) in self.bg_chr_banks.iter().enumerate() {
            info.chr_banks.push((format!("BG{i}"), format!("{b:#04x}")));
        }
        for (i, b) in self.sprite_chr_banks.iter().enumerate() {
            info.chr_banks.push((format!("SP{i}"), format!("{b:#04x}")));
        }
        info.irq_state
            .push(("compare".into(), format!("{:#04x}", self.irq_compare)));
        info.irq_state
            .push(("scanline".into(), format!("{:#04x}", self.scanline_counter)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending)));
        info.irq_state
            .push(("in_frame".into(), format!("{}", self.in_frame)));
        info.extra
            .push(("nt_map".into(), format!("{:#04x}", self.nametable_map)));
        info.extra
            .push(("exram_mode".into(), format!("{}", self.exram_mode)));
        info.extra.push((
            "split".into(),
            format!(
                "en={} tile={} v={} chr={}",
                self.split_enable, self.split_tile, self.split_v_scroll, self.split_chr_bank
            ),
        ));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            64 + self.prg_ram.len() + self.vram.len() + self.exram.len() + self.chr.len(),
        );
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_mode);
        out.push(self.chr_mode);
        out.push(self.prg_ram_protect_1);
        out.push(self.prg_ram_protect_2);
        out.push(self.exram_mode);
        out.push(self.nametable_map);
        out.push(self.fill_tile);
        out.push(self.fill_attr);
        out.push(self.prg_ram_bank);
        for slot in &self.prg_banks {
            out.push(slot.raw);
        }
        for &b in &self.bg_chr_banks {
            out.extend_from_slice(&b.to_le_bytes());
        }
        for &b in &self.sprite_chr_banks {
            out.extend_from_slice(&b.to_le_bytes());
        }
        out.push(self.chr_upper);
        out.push(u8::from(self.last_chr_write_was_sprite));
        // ExGrafix CHR bank latch: 1 byte tag (0/1 = absent/present)
        // followed by 2 bytes of bank value.
        if let Some(b) = self.ex_chr_bank_latch {
            out.push(1);
            out.extend_from_slice(&b.to_le_bytes());
        } else {
            out.push(0);
            out.extend_from_slice(&[0u8, 0u8]);
        }
        // Vertical split-screen state ($5200-$5202) — added in v3.
        // 6 bytes total: split_enable | split_side_right | split_tile |
        //                split_v_scroll | split_chr_bank | latch_tag(+1 byte value).
        out.push(u8::from(self.split_enable));
        out.push(u8::from(self.split_side_right));
        out.push(self.split_tile);
        out.push(self.split_v_scroll);
        out.push(self.split_chr_bank);
        if let Some(b) = self.split_chr_bank_latch {
            out.push(1);
            out.push(b);
        } else {
            out.push(0);
            out.push(0);
        }
        out.push(self.irq_compare);
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_pending));
        out.push(u8::from(self.in_frame));
        out.push(self.scanline_counter);
        out.push(self.mul_a);
        out.push(self.mul_b);
        out.push(self.current_mirroring_summary as u8);
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.exram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        // v4 tail: audio extension state (30 bytes). Per ADR-0003 the bump
        // from v3 -> v4 is required (additive trailing field that the
        // expected-length check would reject otherwise). v3 blobs are
        // accepted for forward-compat in `load_state` below.
        out.push(u8::from(self.audio_apu_phase));
        self.audio.write_tail(&mut out);
        out
    }

    #[allow(clippy::too_many_lines)]
    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_part = if self.chr_is_ram { self.chr.len() } else { 0 };
        // Scalar layout:
        //   1 (version) + 9 (prg_mode..prg_ram_bank) + 4 (prg_banks)
        //   + 8 (4 * 2 bytes BG) + 16 (8 * 2 bytes sprite)
        //   + 1 (chr_upper) + 1 (last_chr_write_was_sprite)
        //   + 1 (ex_chr_bank_latch tag) + 2 (ex_chr_bank_latch value)
        //   + 1 (irq_compare) + 1 (irq_enabled) + 1 (irq_pending)
        //   + 1 (in_frame) + 1 (scanline_counter) + 1 (mul_a) + 1 (mul_b)
        //   + 1 (mirroring_summary)
        // v3 adds 7 bytes for vertical split state:
        //   split_enable + split_side_right + split_tile + split_v_scroll
        //   + split_chr_bank + split_chr_bank_latch (tag + value)
        let scalar_len: usize =
            1 + 9 + 4 + 8 + 16 + 1 + 1 + 1 + 2 + 7 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1;
        let core_expected =
            scalar_len + self.prg_ram.len() + self.vram.len() + self.exram.len() + chr_part;
        // v3: no audio tail. v4: audio tail of 1 (audio_apu_phase) +
        // `Mmc5Audio::TAIL_LEN` bytes. We accept both for forward compat
        // per ADR-0003.
        let version = if data.is_empty() { 0 } else { data[0] };
        let expected = match version {
            3 => core_expected,
            4 => core_expected + 1 + Mmc5Audio::TAIL_LEN,
            _ => core_expected, // best-effort; rejected below by version check
        };
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if !(3..=SAVE_STATE_VERSION).contains(&version) {
            return Err(MapperError::UnsupportedVersion(version));
        }
        let mut cur = 1usize;
        self.prg_mode = data[cur];
        cur += 1;
        self.chr_mode = data[cur];
        cur += 1;
        self.prg_ram_protect_1 = data[cur];
        cur += 1;
        self.prg_ram_protect_2 = data[cur];
        cur += 1;
        self.exram_mode = data[cur];
        cur += 1;
        self.nametable_map = data[cur];
        cur += 1;
        self.fill_tile = data[cur];
        cur += 1;
        self.fill_attr = data[cur];
        cur += 1;
        self.prg_ram_bank = data[cur];
        cur += 1;
        for slot in &mut self.prg_banks {
            slot.raw = data[cur];
            cur += 1;
        }
        for b in &mut self.bg_chr_banks {
            *b = u16::from_le_bytes([data[cur], data[cur + 1]]);
            cur += 2;
        }
        for b in &mut self.sprite_chr_banks {
            *b = u16::from_le_bytes([data[cur], data[cur + 1]]);
            cur += 2;
        }
        self.chr_upper = data[cur];
        cur += 1;
        self.last_chr_write_was_sprite = data[cur] != 0;
        cur += 1;
        // ExGrafix CHR bank latch: tag + 2-byte value.
        let tag = data[cur];
        cur += 1;
        let bank_lo = data[cur];
        let bank_hi = data[cur + 1];
        cur += 2;
        self.ex_chr_bank_latch = if tag != 0 {
            Some(u16::from_le_bytes([bank_lo, bank_hi]))
        } else {
            None
        };
        // v3: vertical split state.
        self.split_enable = data[cur] != 0;
        cur += 1;
        self.split_side_right = data[cur] != 0;
        cur += 1;
        self.split_tile = data[cur] & 0x1F;
        cur += 1;
        self.split_v_scroll = data[cur];
        cur += 1;
        self.split_chr_bank = data[cur];
        cur += 1;
        let split_tag = data[cur];
        cur += 1;
        let split_lat = data[cur];
        cur += 1;
        self.split_chr_bank_latch = if split_tag != 0 {
            Some(split_lat)
        } else {
            None
        };
        self.irq_compare = data[cur];
        cur += 1;
        self.irq_enabled = data[cur] != 0;
        cur += 1;
        self.irq_pending = data[cur] != 0;
        cur += 1;
        self.in_frame = data[cur] != 0;
        cur += 1;
        self.scanline_counter = data[cur];
        cur += 1;
        self.mul_a = data[cur];
        cur += 1;
        self.mul_b = data[cur];
        cur += 1;
        self.current_mirroring_summary = match data[cur] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => {
                return Err(MapperError::Invalid(format!(
                    "unknown mirroring tag {other}"
                )));
            }
        };
        cur += 1;
        let prg_ram_len = self.prg_ram.len();
        self.prg_ram.copy_from_slice(&data[cur..cur + prg_ram_len]);
        cur += prg_ram_len;
        let vram_len = self.vram.len();
        self.vram.copy_from_slice(&data[cur..cur + vram_len]);
        cur += vram_len;
        let exram_len = self.exram.len();
        self.exram.copy_from_slice(&data[cur..cur + exram_len]);
        cur += exram_len;
        if self.chr_is_ram {
            let chr_len = self.chr.len();
            self.chr.copy_from_slice(&data[cur..cur + chr_len]);
            cur += chr_len;
        }
        // v4 audio tail (optional for v3 blobs — defaulted to silent).
        if version >= 4 {
            self.audio_apu_phase = data[cur] != 0;
            cur += 1;
            self.audio
                .read_tail(&data[cur..cur + Mmc5Audio::TAIL_LEN])?;
        } else {
            // v3 blob: silence the audio extension (channels disabled,
            // PCM at zero, length counters cleared). This keeps cross-
            // version load deterministic.
            self.audio = Mmc5Audio::default();
            self.audio_apu_phase = false;
        }
        Ok(())
    }
}

/// Decode `$5105` into a coarse `Mirroring` summary for `current_mirroring`.
/// MMC5 supports per-1KiB nametable mapping that `Mirroring` cannot fully
/// represent; we collapse common cases:
///   `0b01_00_01_00` (NT_A/B/A/B) -> Vertical
///   `0b00_00_01_01` (NT_A/A/B/B) -> Horizontal (well, the inverse of...)
///   `0b00_00_00_00` -> SingleScreenA
///   `0b01_01_01_01` -> SingleScreenB
/// For more exotic mappings we report `MapperControlled`.
fn nt_summary(byte: u8) -> Mirroring {
    match byte {
        0x44 /* 0b01_00_01_00 */ => Mirroring::Vertical,
        0x50 /* 0b01_01_00_00 */ => Mirroring::Horizontal,
        0x00 => Mirroring::SingleScreenA,
        0x55 /* 0b01_01_01_01 */ => Mirroring::SingleScreenB,
        _ => Mirroring::MapperControlled,
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * PRG_BANK_8K];
        for b in 0..banks_8k {
            // Mark the start of each 8 KiB page with its bank index so we
            // can verify banking math.
            v[b * PRG_BANK_8K] = b as u8;
            // Mark the last byte of each page with bank index XOR 0xFF.
            v[(b + 1) * PRG_BANK_8K - 1] = !(b as u8);
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

    fn fresh(prg_banks: usize, chr_banks: usize) -> Mmc5 {
        Mmc5::new(
            synth_prg(prg_banks),
            synth_chr(chr_banks),
            Mirroring::Vertical,
            0,
        )
        .unwrap()
    }

    #[test]
    fn power_on_defaults_to_prg_mode_3_with_last_bank_at_e000() {
        let mut m = fresh(8, 8);
        // Default: PRG mode 3 (4x8K), $5117 -> last bank ROM. $E000 should
        // read bank 7's first byte (= 7).
        assert_eq!(m.cpu_read(0xE000), 7);
        // Last byte of $FFFF window:
        assert_eq!(m.cpu_read(0xFFFF), !7u8);
    }

    #[test]
    fn prg_mode_0_uses_single_32k_bank() {
        let mut m = fresh(8, 8);
        // Mode 0: 32K driven by $5117 (page bits & ~3).
        m.cpu_write(0x5100, 0);
        // Set $5117 to bank 4 (which gets masked to 4 since 4 & ~3 == 4).
        m.cpu_write(0x5117, 0x80 | 4);
        // $8000 -> page 4, $A000 -> page 5, $C000 -> page 6, $E000 -> page 7.
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xA000), 5);
        assert_eq!(m.cpu_read(0xC000), 6);
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn prg_mode_1_two_16k_banks() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5100, 1);
        // $5115 -> low 16K (page & ~1), $5117 -> high 16K (page & ~1).
        m.cpu_write(0x5115, 0x80 | 2); // page 2 -> 2 & ~1 = 2
        m.cpu_write(0x5117, 0x80 | 5); // page 5 -> 5 & ~1 = 4
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xA000), 3);
        assert_eq!(m.cpu_read(0xC000), 4);
        assert_eq!(m.cpu_read(0xE000), 5);
    }

    #[test]
    fn prg_mode_2_16k_plus_8k_plus_8k() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5100, 2);
        m.cpu_write(0x5115, 0x80 | 2); // 16K @ $8000
        m.cpu_write(0x5116, 0x80 | 5); // 8K @ $C000
        m.cpu_write(0x5117, 0x80 | 7); // 8K @ $E000
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xA000), 3);
        assert_eq!(m.cpu_read(0xC000), 5);
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn prg_mode_3_four_8k_banks() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5100, 3);
        m.cpu_write(0x5114, 0x80 | 1);
        m.cpu_write(0x5115, 0x80 | 3);
        m.cpu_write(0x5116, 0x80 | 5);
        m.cpu_write(0x5117, 0x80 | 7);
        assert_eq!(m.cpu_read(0x8000), 1);
        assert_eq!(m.cpu_read(0xA000), 3);
        assert_eq!(m.cpu_read(0xC000), 5);
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn slot_5117_is_always_rom() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5100, 3);
        // Try to mark $5117 as RAM by clearing bit 7.
        m.cpu_write(0x5117, 7); // page 7, no bit-7 set ("RAM" bit)
        // Should still read ROM bank 7.
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn chr_mode_0_8k_bank_via_512b() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5101, 0); // 8K mode
        // $512B drives 8K (low 3 bits ignored: bank & ~7).
        m.cpu_write(0x512B, 4); // page 4 -> bank 4 (4 & ~7 = 0). Hmm!
        // 4 & ~7 == 0, so $0000 reads bank 0. Use 8 instead -> 8 & ~7 = 8 -> mask.
        // With 8 1K banks of CHR, we need banks 0..=7. 4 & ~7 = 0 yields bank 0.
        assert_eq!(m.ppu_read(0x0000), 0);
        // $1000 (1K slot 4 within 8K window) -> bank 0 + 4 = 4.
        assert_eq!(m.ppu_read(0x1000), 4);
    }

    #[test]
    fn chr_mode_1_two_4k_banks() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5101, 1); // 4+4K mode
        // $5129 drives low 4K, $512B drives high 4K. Bank value masked & ~3.
        m.cpu_write(0x5129, 4); // 4 & ~3 = 4
        m.cpu_write(0x512B, 0); // 0 & ~3 = 0
        assert_eq!(m.ppu_read(0x0000), 4); // low 4K starts at bank 4
        assert_eq!(m.ppu_read(0x0400), 5);
        assert_eq!(m.ppu_read(0x1000), 0); // high 4K starts at bank 0
        assert_eq!(m.ppu_read(0x1400), 1);
    }

    #[test]
    fn chr_mode_2_four_2k_banks() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5101, 2);
        m.cpu_write(0x5128, 0); // $0000-$07FF -> bank 0 (& ~1) | 0
        m.cpu_write(0x5129, 2); // $0800-$0FFF -> bank 2
        m.cpu_write(0x512A, 4); // $1000-$17FF -> bank 4
        m.cpu_write(0x512B, 6); // $1800-$1FFF -> bank 6
        assert_eq!(m.ppu_read(0x0000), 0);
        assert_eq!(m.ppu_read(0x0800), 2);
        assert_eq!(m.ppu_read(0x1000), 4);
        assert_eq!(m.ppu_read(0x1800), 6);
    }

    #[test]
    fn chr_mode_3_eight_1k_banks() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5101, 3);
        // BG side has 4 registers; the second 4K mirrors the first.
        m.cpu_write(0x5128, 1);
        m.cpu_write(0x5129, 3);
        m.cpu_write(0x512A, 5);
        m.cpu_write(0x512B, 7);
        assert_eq!(m.ppu_read(0x0000), 1);
        assert_eq!(m.ppu_read(0x0400), 3);
        assert_eq!(m.ppu_read(0x0800), 5);
        assert_eq!(m.ppu_read(0x0C00), 7);
    }

    #[test]
    fn nametable_mapping_routes_per_1kib() {
        let mut m = fresh(8, 8);
        // 0b00_01_10_11 -> NT0=A, NT1=B, NT2=ExRAM, NT3=Fill.
        // Note: low 2 bits = NT0, etc.
        m.cpu_write(0x5105, 0b11_10_01_00);
        assert_eq!(m.nt_source(0), NtSource::CiramA);
        assert_eq!(m.nt_source(1), NtSource::CiramB);
        assert_eq!(m.nt_source(2), NtSource::ExRam);
        assert_eq!(m.nt_source(3), NtSource::Fill);
    }

    #[test]
    fn exram_mode_10_general_ram_readback() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5104, 0b10);
        m.cpu_write(0x5C00, 0xAB);
        m.cpu_write(0x5DEF, 0xCD);
        assert_eq!(m.cpu_read(0x5C00), 0xAB);
        assert_eq!(m.cpu_read(0x5DEF), 0xCD);
    }

    #[test]
    fn exram_mode_11_is_read_only() {
        let mut m = fresh(8, 8);
        // First populate via mode 10.
        m.cpu_write(0x5104, 0b10);
        m.cpu_write(0x5C00, 0x42);
        // Switch to read-only.
        m.cpu_write(0x5104, 0b11);
        m.cpu_write(0x5C00, 0xFF); // ignored
        assert_eq!(m.cpu_read(0x5C00), 0x42);
    }

    #[test]
    fn exram_mode_00_used_as_nametable_via_5105() {
        let mut m = fresh(8, 8);
        // ExRAM mode = 00 (extra nametable).
        m.cpu_write(0x5104, 0b00);
        // Map NT0 to ExRAM.
        m.cpu_write(0x5105, 0b11_10_01_10); // NT0 = ExRAM (10)
        // Write through PPU bus to $2000 (NT0).
        m.ppu_write(0x2000, 0xAA);
        assert_eq!(m.ppu_read(0x2000), 0xAA);
        // ExRAM should reflect this too via CPU read.
        // Need to be in mode 10 to read CPU side cleanly — we left
        // ExRAM mode = 00, but CPU reads from $5C00-$5FFF still work.
        assert_eq!(m.cpu_read(0x5C00), 0xAA);
    }

    #[test]
    fn prg_ram_protect_pair_must_be_unlocked() {
        let mut m = fresh(8, 8);
        // Default lock state -> writes to $6000 are dropped.
        m.cpu_write(0x6000, 0x11);
        assert_eq!(m.cpu_read(0x6000), 0x00);
        // Unlock: $5102 = 0x02, $5103 = 0x01.
        m.cpu_write(0x5102, 0x02);
        m.cpu_write(0x5103, 0x01);
        m.cpu_write(0x6000, 0x22);
        assert_eq!(m.cpu_read(0x6000), 0x22);
        // Re-lock by writing the wrong value to $5102.
        m.cpu_write(0x5102, 0x00);
        m.cpu_write(0x6000, 0x33);
        // Stays at 0x22.
        assert_eq!(m.cpu_read(0x6000), 0x22);
    }

    #[test]
    fn multiplier_returns_8x8_to_16_product() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5205, 0x10);
        m.cpu_write(0x5206, 0x20);
        // 0x10 * 0x20 = 0x200; low byte = 0x00; high byte = 0x02.
        assert_eq!(m.cpu_read(0x5205), 0x00);
        assert_eq!(m.cpu_read(0x5206), 0x02);

        m.cpu_write(0x5205, 0xFF);
        m.cpu_write(0x5206, 0xFF);
        // 0xFE01.
        assert_eq!(m.cpu_read(0x5205), 0x01);
        assert_eq!(m.cpu_read(0x5206), 0xFE);
    }

    #[test]
    fn scanline_irq_enters_in_frame_and_increments() {
        let mut m = fresh(8, 8);
        // Compare value = 3, IRQ enabled.
        m.cpu_write(0x5203, 3);
        m.cpu_write(0x5204, 0x80);
        // Pretend the PPU starts scanlines.
        // First call -> in_frame=true, counter=0.
        m.notify_scanline_start();
        assert!(m.in_frame);
        assert_eq!(m.scanline_counter, 0);
        // Three more -> counter=3 -> match -> irq_pending.
        m.notify_scanline_start();
        m.notify_scanline_start();
        m.notify_scanline_start();
        assert_eq!(m.scanline_counter, 3);
        assert!(m.irq_pending);
        assert!(m.irq_pending());
    }

    #[test]
    fn vblank_clears_in_frame_flag() {
        let mut m = fresh(8, 8);
        m.notify_scanline_start();
        assert!(m.in_frame);
        m.notify_vblank();
        assert!(!m.in_frame);
        // Next scanline_start re-enters in_frame and resets counter.
        m.notify_scanline_start();
        assert!(m.in_frame);
        assert_eq!(m.scanline_counter, 0);
    }

    #[test]
    fn reading_5204_acks_pending_and_returns_status() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5203, 1);
        m.cpu_write(0x5204, 0x80);
        m.notify_scanline_start(); // counter=0; not match
        m.notify_scanline_start(); // counter=1; match
        assert!(m.irq_pending);
        let v = m.cpu_read(0x5204);
        assert!((v & 0x80) != 0); // status bit was set
        // Read should clear the pending latch.
        assert!(!m.irq_pending);
        assert!(!m.irq_pending());
    }

    #[test]
    fn irq_disabled_no_assert_to_cpu() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5203, 1);
        // Don't write $5204 enable bit.
        m.notify_scanline_start();
        m.notify_scanline_start();
        // The internal latch may set, but irq_pending() (CPU-visible) is gated.
        assert!(!m.irq_pending());
    }

    // ------------------------------------------------------------------
    // Feature tests: fill mode, dual CHR for sprites, ExGrafix.
    // ------------------------------------------------------------------

    #[test]
    fn fill_mode_returns_fill_tile_in_nametable_region() {
        let mut m = fresh(8, 8);
        // Set NT0 -> Fill (0b11 in low 2 bits of $5105).
        m.cpu_write(0x5105, 0x03);
        m.cpu_write(0x5106, 0xAB);
        m.cpu_write(0x5107, 0x02);
        // Within the 32x30 NT byte region (offsets 0..0x3C0) -> fill tile.
        assert_eq!(m.nametable_fetch(0x2000), Some(0xAB));
        assert_eq!(m.nametable_fetch(0x2200), Some(0xAB));
        // Within the AT region (offset 0x3C0..0x400) -> 4-way replicated
        // 2-bit fill attr. attr=2 -> 0b10101010 = 0xAA.
        assert_eq!(m.nametable_fetch(0x23C0), Some(0xAA));
        assert_eq!(m.nametable_fetch(0x23FF), Some(0xAA));
    }

    #[test]
    fn fill_mode_writes_are_dropped() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5105, 0x03); // NT0 -> Fill
        m.cpu_write(0x5106, 0x11);
        // Writes via the nametable hook are absorbed and dropped.
        let consumed = m.nametable_write(0x2000, 0xFF);
        assert!(consumed);
        // Read still returns the fill tile.
        assert_eq!(m.nametable_fetch(0x2000), Some(0x11));
    }

    #[test]
    fn fill_mode_only_affects_selected_nametables() {
        let mut m = fresh(8, 8);
        // NT0 = Fill, NT1 = CIRAM_A, NT2 = CIRAM_B, NT3 = Fill.
        m.cpu_write(0x5105, 0b11_01_00_11);
        m.cpu_write(0x5106, 0x55);
        // NT1 and NT2 do not synthesize.
        assert_eq!(m.nametable_fetch(0x2400), None);
        assert_eq!(m.nametable_fetch(0x2800), None);
        // NT0 + NT3 do.
        assert_eq!(m.nametable_fetch(0x2000), Some(0x55));
        assert_eq!(m.nametable_fetch(0x2C00), Some(0x55));
    }

    #[test]
    fn exram_nametable_fetch_returns_exram_byte() {
        let mut m = fresh(8, 8);
        // ExRAM mode 00 + NT0 -> ExRAM.
        m.cpu_write(0x5104, 0b00);
        m.cpu_write(0x5105, 0b11_10_01_10);
        // Stash a value in ExRAM.
        m.exram[0x10] = 0x77;
        assert_eq!(m.nametable_fetch(0x2010), Some(0x77));
    }

    #[test]
    fn exram_nametable_write_routes_into_exram() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5104, 0b00);
        m.cpu_write(0x5105, 0b00_00_00_10); // NT0 = ExRAM
        let consumed = m.nametable_write(0x2042, 0x33);
        assert!(consumed);
        assert_eq!(m.exram[0x42], 0x33);
    }

    #[test]
    fn ciram_nametable_writes_pass_through() {
        let mut m = fresh(8, 8);
        // Default $5105 = 0 -> all NT_A.
        // Writes should NOT be absorbed; the PPU is responsible for CIRAM.
        let consumed = m.nametable_write(0x2000, 0xCC);
        assert!(!consumed);
    }

    #[test]
    fn dual_chr_for_sprites_uses_sprite_bank_set() {
        let mut m = fresh(8, 8);
        // Set sprite CHR registers to bank=2,3,4,5,6,7,0,1 (1K each).
        for i in 0..8u8 {
            m.cpu_write(0x5120 + u16::from(i), (i + 2) & 0x07);
        }
        // BG CHR registers to all 0 -> BG fetches read bank 0..3 (mode 3).
        m.cpu_write(0x5101, 3); // 1K x 8 mode
        for i in 0..4u8 {
            m.cpu_write(0x5128 + u16::from(i), 0);
        }
        // BG fetch at $0000 -> bg bank 0 -> CHR offset 0 -> first byte = 0.
        assert_eq!(m.ppu_read(0x0000), 0);
        // Sprite fetch at $0000 -> sprite bank 2 -> first byte of bank 2.
        assert_eq!(m.ppu_read_sprite(0x0000), 2);
        // Sprite fetch at $0400 -> sprite bank 3 -> first byte of bank 3.
        assert_eq!(m.ppu_read_sprite(0x0400), 3);
        // Sprite fetch at $1C00 -> sprite bank 1 -> first byte of bank 1.
        assert_eq!(m.ppu_read_sprite(0x1C00), 1);
    }

    #[test]
    fn ex_attribute_returns_none_outside_mode_01() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5104, 0b00); // not ExGrafix
        assert_eq!(m.peek_ex_attribute(0), None);
        m.cpu_write(0x5104, 0b10);
        assert_eq!(m.peek_ex_attribute(0), None);
    }

    #[test]
    fn ex_attribute_decodes_palette_and_chr_bank_in_mode_01() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5104, 0b01); // ExGrafix
        // ExRAM byte for tile (coarse_x=2, coarse_y=3): index 3*32+2 = 98.
        m.exram[98] = 0b11_001010; // palette = 3, bank low = 0x0A
        // v: low 5 = coarse_x = 2; bits 5..9 = coarse_y = 3.
        let v = (3u16 << 5) | 2;
        let ex = m.peek_ex_attribute(v).unwrap();
        assert_eq!(ex.palette, 3);
        assert_eq!(ex.chr_bank, 0x0A);
        // The latch is now stored on the mapper.
        assert_eq!(m.ex_chr_bank_latch, Some(0x0A));
    }

    #[test]
    fn ex_attribute_chr_override_routes_chr_fetch() {
        // CHR with 16 banks of 1K (4 banks of 4K).
        let mut m = fresh(8, 16);
        // Mark each bank's first byte uniquely (already done by synth_chr).
        m.cpu_write(0x5104, 0b01); // ExGrafix
        // Tile at coarse (0, 0): ExRAM[0] -> palette=0, bank low6=2 (4K bank 2).
        m.exram[0] = 0b00_000010;
        let v = 0u16;
        let _ = m.peek_ex_attribute(v); // sets ex_chr_bank_latch = Some(2)
        // BG fetch at $0000 -> chr_offset uses 4K bank 2 -> 1K bank 8 -> bank index 8.
        assert_eq!(m.ppu_read(0x0000), 8);
        // BG fetch at $0400 (1K offset 1024 within the 4K bank) -> 1K bank 9.
        assert_eq!(m.ppu_read(0x0400), 9);
    }

    #[test]
    fn ex_attribute_clears_latch_when_leaving_mode() {
        let mut m = fresh(8, 16);
        m.cpu_write(0x5104, 0b01);
        m.exram[0] = 0b00_000011;
        let _ = m.peek_ex_attribute(0);
        assert_eq!(m.ex_chr_bank_latch, Some(3));
        // Leave mode -> next peek returns None and clears the latch.
        m.cpu_write(0x5104, 0b00);
        assert_eq!(m.ex_chr_bank_latch, None);
        assert_eq!(m.peek_ex_attribute(0), None);
    }

    #[test]
    fn save_load_round_trip() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5100, 2);
        m.cpu_write(0x5114, 0x80 | 1);
        m.cpu_write(0x5115, 0x80 | 3);
        m.cpu_write(0x5116, 0x80 | 5);
        m.cpu_write(0x5117, 0x80 | 7);
        m.cpu_write(0x5101, 1);
        m.cpu_write(0x5128, 1);
        m.cpu_write(0x5129, 2);
        m.cpu_write(0x512A, 3);
        m.cpu_write(0x512B, 4);
        m.cpu_write(0x5104, 0b10);
        m.cpu_write(0x5C00, 0xDE);
        m.cpu_write(0x5203, 42);
        m.cpu_write(0x5204, 0x80);
        m.cpu_write(0x5205, 0x07);
        m.cpu_write(0x5206, 0x09);

        let blob = m.save_state();
        let mut other = fresh(8, 8);
        other.load_state(&blob).unwrap();

        assert_eq!(other.prg_mode, m.prg_mode);
        assert_eq!(other.chr_mode, m.chr_mode);
        assert_eq!(other.bg_chr_banks, m.bg_chr_banks);
        for i in 0..4 {
            assert_eq!(other.prg_banks[i].raw, m.prg_banks[i].raw);
        }
        assert_eq!(other.exram_mode, m.exram_mode);
        assert_eq!(other.exram[0x000], 0xDE);
        assert_eq!(other.irq_compare, 42);
        assert!(other.irq_enabled);
        assert_eq!(other.mul_a, 0x07);
        assert_eq!(other.mul_b, 0x09);
        // Multiplier readback.
        assert_eq!(other.cpu_read(0x5205), 0x3F);
        assert_eq!(other.cpu_read(0x5206), 0x00);
    }

    // ------------------------------------------------------------------
    // Vertical split-screen ($5200-$5202).
    // ------------------------------------------------------------------

    #[test]
    fn split_registers_default_to_disabled() {
        let mut m = fresh(8, 8);
        assert!(!m.split_enable);
        assert_eq!(m.split_tile, 0);
        assert_eq!(m.split_v_scroll, 0);
        assert_eq!(m.split_chr_bank, 0);
        // bg_split_state returns None when disabled.
        assert_eq!(m.bg_split_state(0, 0), None);
        assert_eq!(m.bg_split_state(100, 15), None);
    }

    #[test]
    fn split_5200_decodes_enable_side_and_tile() {
        let mut m = fresh(8, 8);
        // Enable, left side (bit 6 = 0), split column 10.
        m.cpu_write(0x5200, 0x80 | 0x0A);
        assert!(m.split_enable);
        assert!(!m.split_side_right);
        assert_eq!(m.split_tile, 0x0A);
        // Enable, right side (bit 6 = 1), split column 16.
        m.cpu_write(0x5200, 0x80 | 0x40 | 0x10);
        assert!(m.split_enable);
        assert!(m.split_side_right);
        assert_eq!(m.split_tile, 0x10);
        // Disable (bit 7 = 0).
        m.cpu_write(0x5200, 0x00);
        assert!(!m.split_enable);
    }

    #[test]
    fn split_5201_and_5202_store_raw_values() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5201, 0xC8); // = 200
        m.cpu_write(0x5202, 0x03);
        assert_eq!(m.split_v_scroll, 0xC8);
        assert_eq!(m.split_chr_bank, 0x03);
    }

    #[test]
    fn split_left_side_alt_region_is_columns_below_split_tile() {
        let mut m = fresh(8, 8);
        // Enable + left side + split at tile 10.
        m.cpu_write(0x5200, 0x80 | 0x0A);
        m.cpu_write(0x5201, 0); // no v-scroll
        m.cpu_write(0x5202, 5);
        // Columns 0..=9 are alt; 10..=31 are main.
        assert!(m.bg_split_state(0, 0).is_some());
        assert!(m.bg_split_state(0, 9).is_some());
        assert!(m.bg_split_state(0, 10).is_none());
        assert!(m.bg_split_state(0, 31).is_none());
    }

    #[test]
    fn split_right_side_alt_region_is_columns_at_or_above_split_tile() {
        let mut m = fresh(8, 8);
        // Enable + right side + split at tile 10.
        m.cpu_write(0x5200, 0x80 | 0x40 | 0x0A);
        // Columns 10..=31 are alt; 0..=9 are main.
        assert!(m.bg_split_state(0, 0).is_none());
        assert!(m.bg_split_state(0, 9).is_none());
        assert!(m.bg_split_state(0, 10).is_some());
        assert!(m.bg_split_state(0, 31).is_some());
    }

    #[test]
    fn split_state_supplies_nt_at_addresses_and_fine_y() {
        let mut m = fresh(8, 8);
        // Enable + left side + split at tile 16.
        m.cpu_write(0x5200, 0x80 | 16);
        // V-scroll = 16 -> first alt scanline 0 lands at logical row 16,
        // i.e. coarse_y = 2, fine_y = 0.
        m.cpu_write(0x5201, 16);
        m.cpu_write(0x5202, 7);
        // Coarse-X = 5, scanline = 0.
        let s = m.bg_split_state(0, 5).unwrap();
        assert_eq!(s.chr_bank, 7);
        assert_eq!(s.fine_y, 0);
        // NT addr: NT0 + coarse_y=2, coarse_x=5 -> $2000 | (2 << 5) | 5 = $2045.
        assert_eq!(s.nt_addr, 0x2045);
        // AT addr: $23C0 | ((2 >> 2) << 3) | (5 >> 2) = $23C0 | 0 | 1 = $23C1.
        assert_eq!(s.at_addr, 0x23C1);
    }

    #[test]
    fn split_v_scroll_wraps_at_240() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5200, 0x80); // enable, left, split = 0 (alt covers cols < 0 = none!)
        // Use split tile 31 so cx=0 is in alt.
        m.cpu_write(0x5200, 0x80 | 31);
        m.cpu_write(0x5201, 230);
        m.cpu_write(0x5202, 0);
        // Scanline 20 -> y_in_region = (230 + 20) % 240 = 10 -> coarse_y=1, fine_y=2.
        let s = m.bg_split_state(20, 0).unwrap();
        assert_eq!(s.fine_y, 2);
        // coarse_y = 1 -> NT addr = $2000 | (1 << 5) | 0 = $2020.
        assert_eq!(s.nt_addr, 0x2020);
    }

    #[test]
    fn split_state_latches_chr_bank_for_subsequent_bg_fetch() {
        let mut m = fresh(8, 16);
        // ExGrafix off; split on, left, split=16, bank=2 (4 KiB).
        m.cpu_write(0x5200, 0x80 | 16);
        m.cpu_write(0x5202, 2);
        // Trigger split state at coarse_x=0 (alt region).
        let _ = m.bg_split_state(0, 0).unwrap();
        assert_eq!(m.split_chr_bank_latch, Some(2));
        // BG fetch at $0000 -> 4 KiB bank 2 -> 1 KiB bank 8 -> first byte = 8.
        assert_eq!(m.ppu_read(0x0000), 8);
        // BG fetch at $0400 -> 1 KiB bank 9.
        assert_eq!(m.ppu_read(0x0400), 9);
    }

    #[test]
    fn split_state_clears_latch_outside_alt_region() {
        let mut m = fresh(8, 16);
        m.cpu_write(0x5200, 0x80 | 16); // left, split=16
        m.cpu_write(0x5202, 2);
        let _ = m.bg_split_state(0, 0); // alt
        assert!(m.split_chr_bank_latch.is_some());
        // Now a tile outside the alt region clears the latch.
        let _ = m.bg_split_state(0, 20);
        assert_eq!(m.split_chr_bank_latch, None);
    }

    #[test]
    fn split_disable_drops_latch_and_returns_none() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5200, 0x80 | 16);
        m.cpu_write(0x5202, 1);
        let _ = m.bg_split_state(0, 0);
        assert!(m.split_chr_bank_latch.is_some());
        // Disable.
        m.cpu_write(0x5200, 0x00);
        assert!(!m.split_enable);
        let s = m.bg_split_state(0, 0);
        assert!(s.is_none());
        assert_eq!(m.split_chr_bank_latch, None);
    }

    #[test]
    fn split_nametable_fetch_reads_exram_when_active() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5200, 0x80 | 31); // alt covers cols 0..=30
        m.cpu_write(0x5201, 16); // v-scroll 16 -> first alt scanline at row 16
        m.cpu_write(0x5202, 0);
        // Activate split for the tile at cx=5, scanline=0 (alt region).
        // y_in_region = (16 + 0) % 240 = 16; coarse_y = 2; NT addr =
        // $2000 | (2 << 5) | 5 = $2045 -> ExRAM index 0x45.
        let s = m.bg_split_state(0, 5).unwrap();
        assert_eq!(s.nt_addr, 0x2045);
        // Stash a byte at the ExRAM index the synthesized NT addr maps to.
        m.exram[0x45] = 0xC3;
        // The PPU would now call nametable_fetch with $2045; we should get
        // 0xC3 from ExRAM regardless of $5105.
        assert_eq!(m.nametable_fetch(0x2045), Some(0xC3));
    }

    #[test]
    fn split_nametable_fetch_falls_back_to_normal_when_inactive() {
        let mut m = fresh(8, 8);
        // No split activation -> nametable_fetch follows $5105.
        // Default $5105=0 -> all NT_A -> nametable_fetch returns None.
        assert!(m.split_chr_bank_latch.is_none());
        assert_eq!(m.nametable_fetch(0x2000), None);
    }

    #[test]
    fn split_save_load_round_trip_v3() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5200, 0x80 | 0x40 | 0x0C); // enable, right, tile=12
        m.cpu_write(0x5201, 137);
        m.cpu_write(0x5202, 6);
        // Drive a split fetch so the latch is set.
        let _ = m.bg_split_state(0, 20);
        assert!(m.split_chr_bank_latch.is_some());

        let blob = m.save_state();
        // Version byte should be the current SAVE_STATE_VERSION (v4 after
        // the Track C2 MMC5 audio landing; was v3 when this test landed).
        assert_eq!(blob[0], SAVE_STATE_VERSION);
        let mut other = fresh(8, 8);
        other.load_state(&blob).unwrap();
        assert!(other.split_enable);
        assert!(other.split_side_right);
        assert_eq!(other.split_tile, 12);
        assert_eq!(other.split_v_scroll, 137);
        assert_eq!(other.split_chr_bank, 6);
        assert_eq!(other.split_chr_bank_latch, Some(6));
    }

    #[test]
    fn split_takes_precedence_over_exgrafix() {
        let mut m = fresh(8, 16);
        // ExGrafix mode + populate ExRAM[0] = bank 3.
        m.cpu_write(0x5104, 0b01);
        m.exram[0] = 0b00_000011;
        // Also enable split, left, tile=16, bank=2.
        m.cpu_write(0x5200, 0x80 | 16);
        m.cpu_write(0x5202, 2);
        // Activate split (cx=0).
        let _ = m.bg_split_state(0, 0).unwrap();
        // Split CHR latch should win over the (otherwise-applied) ExGrafix latch.
        assert!(m.split_chr_bank_latch.is_some());
        // BG fetch at $0000 -> 4 KiB bank 2 -> 1 KiB bank 8.
        assert_eq!(m.ppu_read(0x0000), 8);
    }

    // -----------------------------------------------------------------
    // MMC5 audio extension (Track C2) — $5000-$5015.
    // -----------------------------------------------------------------

    #[test]
    fn audio_5000_write_decodes_duty_volume_halt() {
        let mut m = fresh(8, 8);
        // 0x9F = 1001_1111 -> duty=2 (bits 6-7), halt=0 (bit 5)... wait,
        // bit 5 is 0 in 0x9F. The task description claims halt=set; that
        // matches 0xBF or 0xB5. We test the canonical decoder: bit 5 set
        // means halt+loop. Use 0xBF: 1011_1111 -> duty=2, halt=1, const=1,
        // vol=15.
        m.cpu_write(0x5000, 0xBF);
        assert_eq!(m.audio.pulse1.duty, 2);
        assert!(m.audio.pulse1.halt);
        assert!(m.audio.pulse1.envelope_constant);
        assert_eq!(m.audio.pulse1.envelope_volume_or_period, 15);
    }

    #[test]
    fn audio_5003_loads_length_counter_via_lookup() {
        let mut m = fresh(8, 8);
        // Enable pulse 1 length counter (per $5015 contract).
        m.cpu_write(0x5015, 0x01);
        // value = (idx << 3) | timer_hi_3bits. idx 4 -> 40.
        m.cpu_write(0x5003, 4 << 3);
        assert_eq!(m.audio.pulse1.length, 40);
        // Disabled channel does NOT load length.
        m.cpu_write(0x5015, 0x00);
        m.cpu_write(0x5003, 5 << 3); // index 5 -> 4
        assert_eq!(m.audio.pulse1.length, 0);
    }

    #[test]
    fn audio_5015_status_reflects_length_and_write_enables() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5015, 0x03);
        // Load lengths via $5003 / $5007.
        m.cpu_write(0x5003, 4 << 3);
        m.cpu_write(0x5007, 4 << 3);
        // Both pulses now have length > 0.
        let s = m.cpu_read(0x5015);
        assert_eq!(s & 0x03, 0x03);
        // Disable pulse 2 -> length cleared, status drops bit 1.
        m.cpu_write(0x5015, 0x01);
        let s = m.cpu_read(0x5015);
        assert_eq!(s & 0x03, 0x01);
    }

    #[test]
    fn audio_timer_period_assembled_from_5002_5003() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5015, 0x01);
        m.cpu_write(0x5002, 0xAB);
        // $5003 low 3 bits = timer high, top 5 bits = length idx.
        m.cpu_write(0x5003, (3u8 << 3) | 0x05);
        // 11-bit assembled period = 0x5AB.
        assert_eq!(m.audio.pulse1.timer_period, 0x05AB);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn audio_pulse_muted_when_timer_period_below_8() {
        let mut m = fresh(8, 8);
        // Enable pulse 1, constant volume 15, no halt.
        m.cpu_write(0x5015, 0x01);
        m.cpu_write(0x5000, 0b0001_1111); // duty=0, halt=0, const=1, vol=15
        m.cpu_write(0x5002, 0x07); // period 7 (< 8) -> muted
        m.cpu_write(0x5003, 4 << 3); // length nonzero
        // Advance enough CPU cycles to let the duty sequencer step many
        // times. With period < 8 the muted-rule keeps output at 0.
        for _ in 0..64 {
            m.notify_cpu_cycle();
        }
        // mix_audio biases to -4800 baseline (no audio active). The
        // pulse-1 muted check applies regardless of duty step.
        assert!(m.audio.pulse1.muted());
        // With both pulses muted and pcm=0, the bias is the only term.
        assert_eq!(m.mix_audio(), -4800);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn audio_pulse_timer_advances_step_every_period_plus_one_cycles() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5015, 0x01);
        // Pulse 1 with period 2 (the smallest non-muted), duty 2 (50%),
        // const vol 15.
        m.cpu_write(0x5000, 0b1001_1111);
        m.cpu_write(0x5002, 0x08); // timer-lo 8 -> period 8 (>= 8 = not muted)
        m.cpu_write(0x5003, 4 << 3);
        // notify_cpu_cycle clocks the timer every OTHER cycle (APU rate).
        // After 18 CPU cycles we expect ~ (18 / 2) / (period+1) = 1 step
        // increment from step 0 -> step 1 (with some integer rounding).
        let start = m.audio.pulse1.step;
        for _ in 0..18 {
            m.notify_cpu_cycle();
        }
        let after = m.audio.pulse1.step;
        // At least one duty step should have advanced.
        assert_ne!(start, after);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn audio_5011_pcm_write_latches_sample_and_mix_reflects_it() {
        let mut m = fresh(8, 8);
        // PCM in write-mode (default, $5010 bit 0 = 0). Sample 64 -> mix
        // contribution 64 * 16 = 1024 above the bias.
        m.cpu_write(0x5010, 0x00);
        m.cpu_write(0x5011, 64);
        assert_eq!(m.audio.pcm_sample, 64);
        // Pulses are silent (no length loaded). Expected mix = pcm_mix - bias.
        let mix = m.mix_audio();
        assert_eq!(mix, 64 * 16 - 4800);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn audio_5010_read_mode_silences_pcm() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5011, 100); // would mix to 100*16 above bias
        let mix_write_mode = m.mix_audio();
        // Switch to read-mode: PCM output is silenced.
        m.cpu_write(0x5010, 0x01);
        let mix_read_mode = m.mix_audio();
        // In read-mode the PCM contribution drops to 0, so mix returns to
        // the bias only. The delta should equal the pre-switch PCM
        // contribution.
        assert_ne!(mix_write_mode, mix_read_mode);
        assert_eq!(mix_read_mode, -4800);
        // Also: writes to $5011 in read-mode are dropped.
        m.cpu_write(0x5011, 50);
        assert_eq!(m.audio.pcm_sample, 100);
    }

    #[test]
    fn audio_save_load_v3_blob_defaults_audio_to_silent() {
        // Build a fake v3 blob (pre-audio MMC5 save). We do this by
        // crafting a v4 blob, then stripping the audio tail and rewriting
        // the version byte to v3. load_state should accept it and reset
        // the audio fields to defaults.
        let mut m = fresh(8, 8);
        // Populate some audio state in `m` so we can verify it gets
        // CLEARED by the v3 load.
        m.cpu_write(0x5015, 0x03);
        m.cpu_write(0x5003, 4 << 3);
        m.cpu_write(0x5011, 0x55);
        assert!(m.audio.pulse1.length > 0);
        assert_eq!(m.audio.pcm_sample, 0x55);

        // Take a v4 snapshot, strip the audio tail (1 + TAIL_LEN bytes),
        // rewrite version byte to 3.
        let mut blob = m.save_state();
        let tail_len = 1 + Mmc5Audio::TAIL_LEN;
        for _ in 0..tail_len {
            blob.pop();
        }
        blob[0] = 3;

        // Load into a fresh MMC5: audio should default to silent.
        let mut other = fresh(8, 8);
        // Pre-load some audio state in `other` to make sure load clears it.
        other.cpu_write(0x5015, 0x03);
        other.cpu_write(0x5003, 4 << 3);
        other.cpu_write(0x5011, 0x77);
        other.load_state(&blob).unwrap();
        assert_eq!(other.audio.pulse1.length, 0);
        assert_eq!(other.audio.pulse2.length, 0);
        assert_eq!(other.audio.pcm_sample, 0);
        assert_eq!(other.audio.pcm_ctrl, 0);
    }

    #[test]
    fn audio_save_load_v4_round_trip_preserves_audio_state() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x5015, 0x03);
        m.cpu_write(0x5000, 0xBF); // pulse1 ctrl
        m.cpu_write(0x5002, 0xCD);
        m.cpu_write(0x5003, (5u8 << 3) | 0x07);
        m.cpu_write(0x5004, 0x95);
        m.cpu_write(0x5006, 0x42);
        m.cpu_write(0x5007, (2u8 << 3) | 0x01);
        m.cpu_write(0x5011, 0x42);
        let blob = m.save_state();
        // First byte = current version.
        assert_eq!(blob[0], SAVE_STATE_VERSION);

        let mut other = fresh(8, 8);
        other.load_state(&blob).unwrap();
        assert_eq!(other.audio.pulse1.duty, 2);
        assert_eq!(other.audio.pulse1.timer_period, 0x7CD);
        assert_eq!(other.audio.pulse2.duty, 2);
        assert_eq!(other.audio.pulse2.timer_period, 0x142);
        assert_eq!(other.audio.pcm_sample, 0x42);
    }

    #[test]
    #[cfg(not(feature = "mapper-audio"))]
    fn audio_feature_off_latches_state_but_mixes_silent() {
        // With the feature off, the register decoders still latch state
        // (so save-state round-trip stays compatible across feature-flag
        // builds), but the oscillators do not advance and mix_audio
        // returns 0.
        let mut m = fresh(8, 8);
        m.cpu_write(0x5015, 0x03);
        m.cpu_write(0x5000, 0xBF);
        m.cpu_write(0x5011, 0x7F);
        // State latched.
        assert_eq!(m.audio.pulse1.duty, 2);
        assert!(m.audio.pulse1.envelope_constant);
        assert_eq!(m.audio.pcm_sample, 0x7F);
        // mix_audio: the default impl returns 0 under the feature-off
        // build (we do not provide a `mix_audio` override).
        assert_eq!(m.mix_audio(), 0);
        // Timer / step does not advance even under many notify_cpu_cycle calls.
        let s = m.audio.pulse1.step;
        for _ in 0..16 {
            m.notify_cpu_cycle();
        }
        assert_eq!(m.audio.pulse1.step, s);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn audio_frame_event_quarter_clocks_envelope_half_clocks_length() {
        let mut m = fresh(8, 8);
        // Enable pulse 1; set non-constant volume, period >= 8, length on.
        m.cpu_write(0x5015, 0x01);
        m.cpu_write(0x5000, 0b0000_1111); // duty=0, halt=0, const=0, period=15
        m.cpu_write(0x5002, 0x10);
        m.cpu_write(0x5003, 5u8 << 3); // length idx 5 -> 4
        let initial_length = m.audio.pulse1.length;
        assert!(initial_length > 0);

        // Quarter-frame -> envelope clock.
        m.notify_frame_event(MapperFrameEvents {
            quarter: true,
            half: false,
        });
        // After a single quarter, the envelope_start latch is cleared and
        // decay is primed at 15. Length unchanged.
        assert!(!m.audio.pulse1.envelope_start);
        assert_eq!(m.audio.pulse1.envelope_decay, 15);
        assert_eq!(m.audio.pulse1.length, initial_length);

        // Half-frame -> length clock (decrements by 1).
        m.notify_frame_event(MapperFrameEvents {
            quarter: false,
            half: true,
        });
        assert_eq!(m.audio.pulse1.length, initial_length - 1);
    }
}

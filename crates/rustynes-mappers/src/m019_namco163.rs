//! Namco 163 (mappers 19 and 210) -- banking, the CPU-cycle IRQ counter, and
//! the on-cart Namco 163 wavetable synthesizer.
//!
//! The 163 carries 128 bytes of internal RAM that serve double duty: they
//! hold the channel register file *and* the wavetable samples themselves,
//! packed two 4-bit samples per byte. One to eight channels play from that
//! shared RAM, time-multiplexed -- so enabling more channels does not make
//! the cart louder, it divides the same output among more voices, which is
//! why the mix divides by the active channel count.
//!
//! Audio is gated behind the `mapper-audio` Cargo feature (default ON); with
//! it off the register decoders still latch (writes land in the internal RAM
//! and the address-port auto-increment still advances) so save states remain
//! portable across feature configurations (ADR 0004). [`Namco163Audio`] is
//! re-used verbatim by the NSF expansion path (`nsf_expansion.rs`).
//!
//! [`NAMCO163_MIX_SCALE`] was recalibrated in v2.1.6 (the previous value was
//! ~12 dB too quiet). See `docs/apu-2a03.md` §Expansion-audio levels.

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

/// Linear scale applied to the channel-count-averaged Namco 163 output (see
/// [`Namco163::mix_audio`] via the audio struct's `mix`).
///
/// Calibrated so a single full-volume (nibble 0↔15, volume 15) N163 square in
/// 1-channel mode reaches ~6.0x the amplitude of a single full-volume 2A03
/// pulse — the level Mesen2 (RustyNES's accuracy bar) produces and that no
/// reference emulator attenuates. Mesen2 `NesSoundMixer::GetOutputVolume`
/// weights N163 at `output * 20` against the 2A03 pulse DAC of
/// `95.88*5000/(8128/15+100) ≈ 746.9`; a full 0↔15 square has per-channel
/// `(sample-8)*volume` swing `225` (from `(0-8)*15 = -120` to `(15-8)*15 =
/// +105`) which, divided by 1 channel and weighted `*20`, is `4500` — a ratio
/// of `4500 / 746.9 ≈ 6.03`. Our path is `((sum / n) * scale) / 65536`; for the
/// same 1-channel full square the normalized swing is `225 * scale / 65536`,
/// which against the 2A03 pulse's `pulse_table[15] ≈ 0.14882` equals
/// `225 * 261 / 65536 / 0.14882 ≈ 6.02`. Peak stays representable: a single
/// full-volume channel reaches `±120 * 261 = ±31320 < i16::MAX`, and the
/// channel-count division keeps multi-voice sums bounded to the same envelope
/// (each of `n` voices only drives `1/n` of the output). Before v2.1.6 this was
/// `64` (≈1.48x — ~12 dB too quiet, an outlier no reference matched). See
/// `docs/apu-2a03.md` §Expansion-audio levels.
// Every item below is expansion-audio support: fully implemented and
// exercised whenever `mapper-audio` is on (the default build is
// dead-code-warning clean), but unreachable when the feature compiles the
// audio subsystem out. `allow(dead_code)` ONLY in that configuration —
// deliberately not `#[cfg]`, so the items still compile and any future
// non-audio caller keeps working.
#[cfg_attr(not(feature = "mapper-audio"), allow(dead_code))]
pub(crate) const NAMCO163_MIX_SCALE: i32 = 261;

/// Namco 163 on-cart wavetable synthesiser.
///
/// 1-8 simultaneous channels, each playing a 4-bit wavetable from the
/// mapper-internal 128-byte sound RAM.  Wavetable data shares the same
/// RAM as the per-channel register file: the wavetable pool conventionally
/// sits at `$00-$3F` (128 nibble-samples), and channels claim 8-byte
/// regions at the top of RAM, with channel 8 (the always-enabled channel)
/// at `$78-$7F` and channel 1 (the lowest priority) at `$40-$47`.  When
/// fewer than 8 channels are enabled, the unused channels' register
/// regions are reusable as additional wavetable storage.
///
/// Register interface (per NESdev wiki, "Namco 163 audio"):
///
/// - `$F800-$FFFF` (write): **address port**.  Bit 7 = auto-increment
///   flag; bits 6-0 = 7-bit address into the 128-byte internal RAM.
/// - `$4800-$4FFF` (read/write): **data port**.  Reads/writes the byte
///   at the latched address.  If the auto-increment flag is set, the
///   latch advances by 1 after each access, *saturating at $7F* (per
///   the wiki: "stopping at $7F" — does **not** wrap to $00).
///
/// Per-channel register layout (8 bytes each; here referenced for the
/// channel at `$78-$7F` = channel 8, but every channel's 8-byte slot
/// follows the same offsets):
///
/// | Offset | Bits   | Field                                           |
/// |--------|--------|-------------------------------------------------|
/// | +0     | 7-0    | Frequency low (bits 7-0 of 18-bit freq)         |
/// | +1     | 7-0    | Phase low (bits 7-0 of 24-bit phase accumulator)|
/// | +2     | 7-0    | Frequency mid (bits 15-8 of freq)               |
/// | +3     | 7-0    | Phase mid (bits 15-8 of phase)                  |
/// | +4     | 1-0    | Frequency high (bits 17-16 of freq)             |
/// | +4     | 7-2    | Length encoding: waveform length = `256 - (reg & 0xFC)` 4-bit samples |
/// | +5     | 7-0    | Phase high (bits 23-16 of phase)                |
/// | +6     | 7-0    | Wave start address, in 4-bit samples (nibbles)  |
/// | +7     | 3-0    | Linear volume (0..=15)                          |
/// | +7     | 6-4    | (Channel 8's `$7F` only) `C` field: number of   |
/// |        |        | enabled channels - 1 (so C=0 → 1 channel,       |
/// |        |        | C=7 → all 8 channels)                           |
///
/// Update rate: each channel updates every 15 CPU cycles.  With `n`
/// active channels, the chip cycles through them in round-robin, so
/// per-channel update rate = `CPU_clock / (15 * n)`.  We model this as
/// a 15-cycle prescaler that advances `tick_index` (mod `n`) and
/// increments only that one channel's phase per tick.
///
/// Mixing: per channel, output = `(sample - 8) * volume`, where `sample`
/// is the 4-bit nibble fetched from RAM at `(wave_addr + (phase >> 16))
/// mod L`, `L` is the per-channel wave length, and the `-8` bias makes
/// the output bipolar (range `-120..=+105`).  The chip itself does not
/// mix — channels are output one-at-a-time — but in practice emulators
/// sum the per-channel outputs and divide by the active channel count
/// (the convention recommended by the wiki and what Mesen2/FCEUX both
/// do).  The final i16 is scaled to match the headroom VRC6 leaves for
/// the APU mixer.
#[cfg(feature = "mapper-audio")]
#[derive(Clone)]
pub(crate) struct Namco163Audio {
    /// 128-byte internal sound RAM.  Shared between wavetable samples
    /// (`$00-$3F` conventionally) and per-channel register file
    /// (`$40-$7F`).
    ram: [u8; 128],
    /// 7-bit address latch (the address the next data-port access
    /// targets).
    addr_latch: u8,
    /// Auto-increment flag from the most recent `$F800-$FFFF` write.
    /// When set, data-port accesses advance `addr_latch` (saturating at
    /// `$7F` per the wiki).
    auto_inc: bool,
    /// Round-robin tick index: 0..=7.  Each 15-cycle tick advances the
    /// phase of channel `7 - tick_index` (since channel 8, at `$78-$7F`,
    /// is the *first* channel updated when only one channel is enabled).
    tick_index: u8,
    /// 15-cycle prescaler.  When it reaches 15, we update the next
    /// channel and reset.
    prescaler: u8,
}

// When the `mapper-audio` feature is OFF, the audio struct still exists
// (so save-state round-trip and the register-decoder contract stay
// identical between feature on/off builds) — but reduced to the bare
// state required for those two paths.
#[cfg(not(feature = "mapper-audio"))]
#[derive(Clone)]
pub(crate) struct Namco163Audio {
    ram: [u8; 128],
    addr_latch: u8,
    auto_inc: bool,
    tick_index: u8,
    prescaler: u8,
}

impl Default for Namco163Audio {
    fn default() -> Self {
        Self {
            ram: [0; 128],
            addr_latch: 0,
            auto_inc: false,
            tick_index: 0,
            prescaler: 0,
        }
    }
}

impl Namco163Audio {
    /// Write to the address port (`$F800-$FFFF`).  Bit 7 = auto-increment;
    /// bits 6-0 = 7-bit address into internal RAM.
    pub(crate) fn write_addr_port(&mut self, value: u8) {
        self.auto_inc = value & 0x80 != 0;
        self.addr_latch = value & 0x7F;
    }

    /// Advance the address latch if auto-increment is enabled.  Per the
    /// wiki, it saturates at `$7F` rather than wrapping back to `$00`.
    fn step_addr(&mut self) {
        if self.auto_inc && self.addr_latch < 0x7F {
            self.addr_latch += 1;
        }
    }

    /// Write to the data port (`$4800-$4FFF`).  Stores at the latched
    /// address; advances the latch when auto-increment is set.
    pub(crate) fn write_data_port(&mut self, value: u8) {
        let idx = (self.addr_latch & 0x7F) as usize;
        self.ram[idx] = value;
        self.step_addr();
    }

    /// Read from the data port (`$4800-$4FFF`).  Returns the byte at the
    /// latched address; advances the latch when auto-increment is set.
    pub(crate) fn read_data_port(&mut self) -> u8 {
        let idx = (self.addr_latch & 0x7F) as usize;
        let v = self.ram[idx];
        self.step_addr();
        v
    }

    /// Active channel count, derived from bits 6-4 of register `$7F`
    /// (`C` field): returns `C + 1` in the range `1..=8`.
    #[cfg(feature = "mapper-audio")]
    fn channel_count(&self) -> u8 {
        ((self.ram[0x7F] >> 4) & 0x07) + 1
    }

    /// Compute the 18-bit frequency value for the channel whose 8-byte
    /// register slot starts at `base` (i.e. `$78` for channel 8, `$70`
    /// for channel 7, ..., `$40` for channel 1).
    #[cfg(feature = "mapper-audio")]
    fn channel_freq(&self, base: usize) -> u32 {
        let lo = u32::from(self.ram[base]);
        let mid = u32::from(self.ram[base + 2]);
        let hi = u32::from(self.ram[base + 4] & 0x03);
        lo | (mid << 8) | (hi << 16)
    }

    /// 24-bit phase accumulator for the channel at `base`.
    #[cfg(feature = "mapper-audio")]
    fn channel_phase(&self, base: usize) -> u32 {
        let lo = u32::from(self.ram[base + 1]);
        let mid = u32::from(self.ram[base + 3]);
        let hi = u32::from(self.ram[base + 5]);
        lo | (mid << 8) | (hi << 16)
    }

    /// Write back the 24-bit phase to the channel's three phase
    /// registers.  Only bits 23..0 are retained (the value is naturally
    /// 24-bit; we mask to be safe under wrap-around).
    #[cfg(feature = "mapper-audio")]
    fn set_channel_phase(&mut self, base: usize, phase: u32) {
        let phase = phase & 0x00FF_FFFF;
        self.ram[base + 1] = (phase & 0xFF) as u8;
        self.ram[base + 3] = ((phase >> 8) & 0xFF) as u8;
        self.ram[base + 5] = ((phase >> 16) & 0xFF) as u8;
    }

    /// Wave length L (in 4-bit samples) for the channel at `base`.
    /// Per the wiki: `L = 256 - (reg[base+4] & 0xFC)`.
    #[cfg(feature = "mapper-audio")]
    fn channel_length(&self, base: usize) -> u32 {
        256u32 - u32::from(self.ram[base + 4] & 0xFC)
    }

    /// Wave start address for the channel at `base` (in nibble units —
    /// every step of `wave_addr` represents one 4-bit sample, so two
    /// nibbles per RAM byte).
    #[cfg(feature = "mapper-audio")]
    fn channel_wave_addr(&self, base: usize) -> u32 {
        u32::from(self.ram[base + 6])
    }

    /// 4-bit linear volume for the channel at `base`.
    #[cfg(feature = "mapper-audio")]
    fn channel_volume(&self, base: usize) -> u8 {
        self.ram[base + 7] & 0x0F
    }

    /// Resolve the 4-bit nibble at `nibble_addr` in the wavetable pool.
    /// Bit 0 of the address picks the high or low nibble of the
    /// corresponding RAM byte: even = low nibble, odd = high nibble.
    #[cfg(feature = "mapper-audio")]
    fn fetch_nibble(&self, nibble_addr: u32) -> u8 {
        let byte = self.ram[((nibble_addr >> 1) & 0x7F) as usize];
        if nibble_addr & 1 == 0 {
            byte & 0x0F
        } else {
            (byte >> 4) & 0x0F
        }
    }

    /// Returns the register-file base address for the i-th enabled
    /// channel (i = 0 is the always-enabled channel 8 at `$78-$7F`;
    /// i = 1 is channel 7 at `$70-$77`; ...; i = 7 is channel 1 at
    /// `$40-$47`).
    #[cfg(feature = "mapper-audio")]
    const fn channel_base(i: u8) -> usize {
        // Channel 8 = $78, channel 7 = $70, ..., channel 1 = $40.
        // base = 0x78 - i*8.
        0x78 - (i as usize) * 8
    }

    /// Advance one CPU cycle.  Every 15 cycles, round-robin to the next
    /// enabled channel and increment its phase by its 18-bit freq value.
    /// When the phase exceeds `L * 65536`, wrap around — the integer
    /// part of `phase >> 16` modulo `L` is the wavetable index.
    #[cfg(feature = "mapper-audio")]
    pub(crate) fn clock(&mut self) {
        self.prescaler = self.prescaler.wrapping_add(1);
        if self.prescaler < 15 {
            return;
        }
        self.prescaler = 0;

        let n = self.channel_count();
        // Round-robin within the active set.  tick_index counts 0..n.
        if self.tick_index >= n {
            self.tick_index = 0;
        }
        let ch = self.tick_index;
        self.tick_index = (self.tick_index + 1) % n;

        let base = Self::channel_base(ch);
        let freq = self.channel_freq(base);
        let length = self.channel_length(base);
        // Phase modulus is L * 2^16 (so that (phase >> 16) mod L stays
        // in [0, L)).  Use 64-bit math to avoid 32-bit overflow when L
        // is near 256 and freq is near 2^18.
        let modulus = u64::from(length) << 16;
        let mut phase = u64::from(self.channel_phase(base));
        phase = phase.wrapping_add(u64::from(freq));
        if modulus != 0 {
            phase %= modulus;
        }
        self.set_channel_phase(base, phase as u32);
    }

    /// Per-channel output sample, bipolar: `(nibble - 8) * volume`,
    /// range `-120..=+105`.
    #[cfg(feature = "mapper-audio")]
    fn channel_output(&self, ch: u8) -> i16 {
        let base = Self::channel_base(ch);
        let length = self.channel_length(base);
        if length == 0 {
            return 0;
        }
        let phase = self.channel_phase(base);
        let wave_addr = self.channel_wave_addr(base);
        let index = (phase >> 16) % length;
        let nibble = self.fetch_nibble(wave_addr + index);
        // -8 bias makes the output bipolar.
        let signed = i16::from(nibble) - 8;
        signed * i16::from(self.channel_volume(base))
    }

    /// Linear-summed audio output, scaled by [`NAMCO163_MIX_SCALE`] to the
    /// hardware-accurate level (v2.1.6).  Per the wiki, channels are output
    /// one-at-a-time on hardware; emulators (Mesen2, FCEUX) approximate the
    /// mix by summing channel outputs and dividing by the number of active
    /// channels.  We do the same, then scale by `261` so a single full-volume
    /// bipolar channel reaches `±31,320` — just under `i16::MAX` and, through
    /// the bus's `/65536` external contract, ~6.0x the 2A03 pulse peak (the
    /// Mesen2 `*20`-weighted `db_n163` level; see [`NAMCO163_MIX_SCALE`]).
    ///
    /// NOTE: The channel-count division matches the reference emulators'
    /// behaviour; the chip's real per-channel time-multiplexed output is
    /// effectively the same average since each channel only drives the
    /// output `1/n` of the time.  Before v2.1.6 the scale was `64` (~1.48x —
    /// ~12 dB too quiet).
    #[cfg(feature = "mapper-audio")]
    pub(crate) fn mix(&self) -> i16 {
        let n = self.channel_count();
        if n == 0 {
            return 0;
        }
        let mut sum: i32 = 0;
        for ch in 0..n {
            sum += i32::from(self.channel_output(ch));
        }
        // Per-channel range is -120..=+105; the channel-count-averaged sum has
        // the same envelope.  Scale to the Mesen2 `db_n163` level.
        ((sum / i32::from(n)) * NAMCO163_MIX_SCALE) as i16
    }

    /// Feature-off shim: the wavetable generator does not advance with
    /// `mapper-audio` disabled.
    ///
    /// Mirrors the gated `clock` above so the shared NSF expansion router
    /// (`nsf_expansion::NsfExpansion::clock`) can call it unconditionally, the
    /// same arrangement `Sunsoft5BAudio` and `FdsAudio` already had. Its
    /// absence broke `--no-default-features` outright: the router clocks every
    /// present chip with no `cfg` of its own, so with the feature off this was
    /// a hard `E0599` — the N163 was the one chip in the router missing the
    /// shim, and `mix` alone was not enough.
    #[cfg(not(feature = "mapper-audio"))]
    #[allow(clippy::needless_pass_by_ref_mut, clippy::unused_self)]
    pub(crate) fn clock(&mut self) {}

    /// `mix_audio` shim for the no-audio build.
    #[cfg(not(feature = "mapper-audio"))]
    #[allow(clippy::unused_self)]
    pub(crate) fn mix(&self) -> i16 {
        0
    }

    /// Save-state tail layout (kept lock-step with `read_tail`):
    ///   ram[128]      : 128
    ///   addr_latch    : 1
    ///   auto_inc      : 1 (bool)
    ///   tick_index    : 1
    ///   prescaler     : 1
    ///   -- 132 bytes total --
    fn write_tail(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.ram);
        out.push(self.addr_latch & 0x7F);
        out.push(u8::from(self.auto_inc));
        out.push(self.tick_index);
        out.push(self.prescaler);
    }

    /// Tail size in bytes — see `write_tail`.
    const TAIL_LEN: usize = 128 + 1 + 1 + 1 + 1;

    fn read_tail(&mut self, src: &[u8]) -> Result<(), MapperError> {
        if src.len() < Self::TAIL_LEN {
            return Err(MapperError::Truncated {
                expected: Self::TAIL_LEN,
                got: src.len(),
            });
        }
        self.ram.copy_from_slice(&src[0..128]);
        self.addr_latch = src[128] & 0x7F;
        self.auto_inc = src[129] != 0;
        self.tick_index = src[130];
        self.prescaler = src[131];
        Ok(())
    }
}

/// Namco 163 (Mapper 19).  Banking + CPU-cycle IRQ + (gated behind
/// `mapper-audio`) 1-8 channel wavetable audio.
pub struct Namco163 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    chr_is_ram: bool,
    prg_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg: [u8; 4], // 8 KiB banks: $8000, $A000, $C000, fixed $E000
    chr: [u8; 8], // 1 KiB CHR banks
    nta: [u8; 4], // 1 KiB NTA banks (CIRAM/CHR ROM swappable)
    mirroring: Mirroring,

    irq_counter: u16,
    irq_pending: bool,

    /// Audio disable bit (`$E000-$E7FF` bit 6).  When set, the
    /// N163 audio circuitry is silenced — both the per-channel clocks
    /// stop advancing and `mix_audio` returns 0.  Cleared at power-on.
    sound_disabled: bool,
    /// Namco 163 on-cart wavetable audio state.  Live regardless of the
    /// `mapper-audio` feature — the register decoders always latch into
    /// `ram` and the address-port flag/latch (so save states stay
    /// round-trippable across builds), but `clock()` / `mix()` are only
    /// driven when the feature is on.
    audio: Namco163Audio,
}

impl Namco163 {
    /// Construct a new Namco 163 mapper.
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
                "Namco163 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
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
                "Namco163 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            chr_is_ram,
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg: [0, 0, 0, 0],
            chr: [0; 8],
            nta: [0; 4],
            mirroring,
            irq_counter: 0,
            irq_pending: false,
            sound_disabled: false,
            audio: Namco163Audio::default(),
        })
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total_8k = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let last = total_8k - 1;
        let bank = match addr & 0xE000 {
            0x8000 => (self.prg[0] as usize) % total_8k,
            0xA000 => (self.prg[1] as usize) % total_8k,
            0xC000 => (self.prg[2] as usize) % total_8k,
            0xE000 => last,
            _ => 0,
        };
        bank * PRG_BANK_8K + (addr as usize & 0x1FFF)
    }
}

impl Mapper for Namco163 {
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

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // Namco 163 maps `$4800-$4FFF` (sound data port) and
        // `$5000-$5FFF` (IRQ counter low/high). The `$4020-$47FF`
        // range is unmapped.
        (0x4020..=0x47FF).contains(&addr)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // Audio data port: reads the byte at the latched address in
            // internal sound RAM, advancing the latch if auto-increment
            // is set.  Decoder runs regardless of `mapper-audio`.
            0x4800..=0x4FFF => self.audio.read_data_port(),
            0x5000..=0x57FF => {
                // IRQ counter low.
                let v = (self.irq_counter & 0xFF) as u8;
                self.irq_pending = false;
                v
            }
            0x5800..=0x5FFF => {
                let v = ((self.irq_counter >> 8) & 0x7F) as u8;
                self.irq_pending = false;
                v
            }
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize % self.prg_ram.len()],
            0x8000..=0xFFFF => {
                let off = self.prg_offset(addr);
                self.prg_rom[off % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            // Audio data port: stores at the latched address in internal
            // sound RAM, advancing the latch if auto-increment is set.
            // Decoder runs regardless of `mapper-audio`.
            0x4800..=0x4FFF => self.audio.write_data_port(value),
            0x5000..=0x57FF => {
                self.irq_counter = (self.irq_counter & 0xFF00) | u16::from(value);
                self.irq_pending = false;
            }
            0x5800..=0x5FFF => {
                self.irq_counter =
                    (self.irq_counter & 0x00FF) | ((u16::from(value) & 0x7F) << 8) | 0x8000;
                self.irq_pending = false;
            }
            0x6000..=0x7FFF => {
                let off = (addr - 0x6000) as usize % self.prg_ram.len();
                self.prg_ram[off] = value;
            }
            0x8000..=0xBFFF => {
                let slot = ((addr - 0x8000) >> 11) as usize; // 4 banks: 8000,8800,9000,9800,A000,...
                if slot < 8 {
                    self.chr[slot] = value;
                }
            }
            0xC000..=0xDFFF => {
                // Additional CHR / NTA bank selects on real hardware.
                // Not wired up here (the existing Namco163 banking model
                // pre-dates this audio work — see the comment in
                // `notify_cpu_cycle`).  Audio decoder is unaffected.
            }
            // $E000-$E7FF: PRG bank 0 select (bits 0-5) + audio-disable
            // flag (bit 6).  When bit 6 is set, the N163 audio chip is
            // silenced — see `mix_audio` / `notify_cpu_cycle`.
            0xE000..=0xE7FF => {
                self.prg[0] = value & 0x3F;
                self.sound_disabled = value & 0x40 != 0;
            }
            0xE800..=0xEFFF => self.prg[1] = value & 0x3F,
            0xF000..=0xF7FF => self.prg[2] = value & 0x3F,
            // $F800-$FFFF: audio address port (bit 7 = auto-increment,
            // bits 6-0 = 7-bit internal RAM address).  On real hardware
            // this register also gates PRG-RAM writes via the upper
            // nibble (`0100` enables writes), but no commercially-released
            // Namco 163 cartridge uses that feature in a way that affects
            // accuracy, so we model only the audio half here.  Decoder
            // runs regardless of `mapper-audio`.
            0xF800..=0xFFFF => self.audio.write_addr_port(value),
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
        // N163 audio runs every CPU cycle whenever the chip is not
        // silenced via the $E000 sound-disable bit.  None of the
        // 8 channel oscillators can be individually halted — only the
        // active-channel count and per-channel volume gate their effect
        // on the mix.
        #[cfg(feature = "mapper-audio")]
        if !self.sound_disabled {
            self.audio.clock();
        }

        if self.irq_counter & 0x8000 != 0 {
            let low = self.irq_counter & 0x7FFF;
            if low == 0x7FFF {
                self.irq_pending = true;
            } else {
                self.irq_counter = (self.irq_counter & 0x8000) | (low + 1);
            }
        }
    }

    #[cfg(feature = "mapper-audio")]
    fn mix_audio(&mut self) -> i32 {
        if self.sound_disabled {
            return 0;
        }
        i32::from(self.audio.mix())
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 19,
            name: "Namco 163".into(),
            mirroring: crate::mapper::mirroring_name(self.current_mirroring()),
            ..Default::default()
        };
        for (i, b) in self.prg.iter().enumerate() {
            info.prg_banks
                .push((format!("PRG{i}"), format!("{b:#04x}")));
        }
        for (i, b) in self.chr.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR{i}"), format!("{b:#04x}")));
        }
        for (i, b) in self.nta.iter().enumerate() {
            info.extra.push((format!("NTA{i}"), format!("{b:#04x}")));
        }
        info.irq_state
            .push(("counter".into(), format!("{:#06x}", self.irq_counter)));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        // v2 (per ADR-0003): strictly additive tail — older v1 readers
        // tolerate the additional bytes (we encode the audio at the end,
        // so the core layout is byte-identical to v1).
        // Audio tail layout:
        //   sound_disabled : 1
        //   audio block    : Namco163Audio::TAIL_LEN (132 bytes)
        //   -- 133 bytes total --
        let mut out = Vec::with_capacity(
            32 + self.prg_ram.len() + self.vram.len() + 1 + Namco163Audio::TAIL_LEN,
        );
        out.push(2u8); // version
        out.extend_from_slice(&self.prg);
        out.extend_from_slice(&self.chr);
        out.extend_from_slice(&self.nta);
        out.push(self.mirroring as u8);
        out.extend_from_slice(&self.irq_counter.to_le_bytes());
        out.push(u8::from(self.irq_pending));
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        // v2 audio tail.
        out.push(u8::from(self.sound_disabled));
        self.audio.write_tail(&mut out);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let scalar_len = 1 + 4 + 8 + 4 + 1 + 2 + 1;
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
        self.prg.copy_from_slice(&data[1..5]);
        self.chr.copy_from_slice(&data[5..13]);
        self.nta.copy_from_slice(&data[13..17]);
        self.mirroring = match data[17] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.irq_counter = u16::from_le_bytes(
            data[18..20]
                .try_into()
                .map_err(|_| MapperError::Invalid("irq_counter".into()))?,
        );
        self.irq_pending = data[20] != 0;
        let mut cur = 21usize;
        self.prg_ram
            .copy_from_slice(&data[cur..cur + self.prg_ram.len()]);
        cur += self.prg_ram.len();
        self.vram.copy_from_slice(&data[cur..cur + self.vram.len()]);
        cur += self.vram.len();

        // v2 tail: audio + sound-disable bit.  v1 blobs end at the core;
        // per ADR-0003 we leave the audio at its current state — silent
        // by default after `new()` — so the older blob loads cleanly
        // (the caller is responsible for an explicit power-cycle if they
        // want a fully-clean slate).  A v2 blob shorter than the tail is
        // accepted permissively for the same forward-compat reason VRC6
        // and FME-7 use.
        if version == 2 && data.len() >= cur + 1 + Namco163Audio::TAIL_LEN {
            self.sound_disabled = data[cur] != 0;
            cur += 1;
            self.audio
                .read_tail(&data[cur..cur + Namco163Audio::TAIL_LEN])?;
        } else if version == 1 {
            // Reset audio to power-on defaults for clean v1→v2 upgrade.
            self.sound_disabled = false;
            self.audio = Namco163Audio::default();
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
    fn namco163_irq_counter() {
        let mut m = Namco163::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        // Set counter low byte = 0xFFE, then high byte+enable.
        m.cpu_write(0x5000, 0xFE);
        m.cpu_write(0x5800, 0xFF); // sets bit 7 & 0x80 of high byte = enable.
        // Ticks until counter reaches 0x7FFF.
        for _ in 0..3 {
            m.notify_cpu_cycle();
        }
        assert!(m.irq_pending());
    }

    fn namco163_for_audio() -> Namco163 {
        Namco163::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap()
    }

    fn n163_write_ram(m: &mut Namco163, addr: u8, auto_inc: bool, value: u8) {
        // $F800 = address port (bit 7 = auto-increment, bits 6-0 = addr).
        let port = (if auto_inc { 0x80 } else { 0x00 }) | (addr & 0x7F);
        m.cpu_write(0xF800, port);
        m.cpu_write(0x4800, value);
    }

    #[test]
    fn namco163_address_port_latch_and_auto_increment() {
        let mut m = namco163_for_audio();
        // Without auto-increment: write 0x05 to addr, then 0x42 to data.
        // Latch should stay at 0x05.
        m.cpu_write(0xF800, 0x05);
        m.cpu_write(0x4800, 0x42);
        assert_eq!(m.audio.ram[0x05], 0x42);
        assert_eq!(m.audio.addr_latch, 0x05);
        assert!(!m.audio.auto_inc);

        // Second write also lands at 0x05 (latch did not advance).
        m.cpu_write(0x4800, 0x99);
        assert_eq!(m.audio.ram[0x05], 0x99);
        assert_eq!(m.audio.addr_latch, 0x05);

        // With auto-increment: write 0x80 | 0x05, then 0x55 → addr 0x05
        // gets 0x55 and latch advances to 0x06.
        m.cpu_write(0xF800, 0x80 | 0x05);
        m.cpu_write(0x4800, 0x55);
        assert_eq!(m.audio.ram[0x05], 0x55);
        assert_eq!(m.audio.addr_latch, 0x06);
        assert!(m.audio.auto_inc);

        // Next data write lands at 0x06.
        m.cpu_write(0x4800, 0x66);
        assert_eq!(m.audio.ram[0x06], 0x66);
        assert_eq!(m.audio.addr_latch, 0x07);
    }

    #[test]
    fn namco163_address_port_saturates_at_7f() {
        // Per the NESdev wiki: the auto-increment "stopping at $7F"
        // rather than wrapping.  Verify by walking the latch up to $7F
        // and then doing one more data access.
        let mut m = namco163_for_audio();
        m.cpu_write(0xF800, 0x80 | 0x7F);
        m.cpu_write(0x4800, 0xAA); // RAM[0x7F] = 0xAA, latch stays at 0x7F.
        assert_eq!(m.audio.ram[0x7F], 0xAA);
        assert_eq!(m.audio.addr_latch, 0x7F);
        // A second write also lands at 0x7F (saturation, not wrap).
        m.cpu_write(0x4800, 0xBB);
        assert_eq!(m.audio.ram[0x7F], 0xBB);
        assert_eq!(m.audio.addr_latch, 0x7F);
        assert_eq!(m.audio.ram[0x00], 0x00, "wrap to $00 must not happen");
    }

    #[test]
    fn namco163_data_port_read_round_trip() {
        // Write 0xAB at addr 0x10 with auto-increment, then read it back.
        // Read also advances the latch.
        let mut m = namco163_for_audio();
        m.cpu_write(0xF800, 0x80 | 0x10);
        m.cpu_write(0x4800, 0xAB);
        // After the write, latch is at 0x11.
        // Re-target 0x10 for the read.
        m.cpu_write(0xF800, 0x80 | 0x10);
        assert_eq!(m.cpu_read(0x4800), 0xAB);
        assert_eq!(m.audio.addr_latch, 0x11);
    }

    #[test]
    fn namco163_wavetable_nibble_unpacking() {
        // Byte 0xAB at RAM[0x10] → nibble 0x20 = 0xB (low), nibble 0x21
        // = 0xA (high).  Verifies the wavetable nibble-fetch helper.
        let mut m = namco163_for_audio();
        m.cpu_write(0xF800, 0x10);
        m.cpu_write(0x4800, 0xAB);
        assert_eq!(m.audio.ram[0x10], 0xAB);
        #[cfg(feature = "mapper-audio")]
        {
            assert_eq!(m.audio.fetch_nibble(0x20), 0x0B);
            assert_eq!(m.audio.fetch_nibble(0x21), 0x0A);
        }
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn namco163_channel_count_selection() {
        // Bits 6-4 of register $7F encode "channel count - 1".
        // C=0 → 1 channel; C=7 → 8 channels.
        let mut m = namco163_for_audio();
        for c in 0u8..=7 {
            n163_write_ram(&mut m, 0x7F, false, c << 4);
            assert_eq!(
                m.audio.channel_count(),
                c + 1,
                "C={c} should map to {} channels",
                c + 1
            );
        }
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn namco163_channel_frequency_assembly() {
        // Channel 8 lives at $78-$7F.  Write freq lo=$78, mid=$7A, hi=$7C.
        // hi register's bits 7-2 carry the wave length encoding, so we
        // pack length bits as well to exercise the mask.
        let mut m = namco163_for_audio();
        // Lo = 0x34, mid = 0x12, hi-bits = 0x02, length-bits = 0xFC
        // (length = 256 - 0xFC = 4).
        n163_write_ram(&mut m, 0x78, false, 0x34);
        n163_write_ram(&mut m, 0x7A, false, 0x12);
        n163_write_ram(&mut m, 0x7C, false, 0xFC | 0x02);

        let freq = m.audio.channel_freq(0x78);
        assert_eq!(freq, 0x02_1234, "freq = hi<<16 | mid<<8 | lo");
        let length = m.audio.channel_length(0x78);
        assert_eq!(length, 4);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn namco163_single_channel_constant_output_then_bipolar_swing() {
        // Channel 0 (the always-enabled channel at $78-$7F) with a
        // constant wavetable of 0xFF (high nibble 0xF, low nibble 0xF)
        // and volume 15 should yield output = (15 - 8) * 15 = +105.
        // Length-1 waveform means the index never moves.
        let mut m = namco163_for_audio();
        // Wavetable byte 0x10 = 0xFF → nibble 0x20 = 0xF, 0x21 = 0xF.
        n163_write_ram(&mut m, 0x10, false, 0xFF);
        // Channel 8 (the always-enabled, highest-priority channel) regs.
        // Wave addr = 0x20 (the nibble we filled).
        // Length encoding: 256 - 0xFC = 4 (chosen to keep the test
        // robust to phase, since every cycle still reads 0xF).
        // Volume = 0x0F, channel-count field = 0 (single channel).
        n163_write_ram(&mut m, 0x7C, false, 0xFC); // length=4, freq-hi=0
        n163_write_ram(&mut m, 0x7E, false, 0x20); // wave_addr
        n163_write_ram(&mut m, 0x7F, false, 0x0F); // volume=15, C=0

        let output = m.audio.channel_output(0);
        assert_eq!(output, (15 - 8) * 15, "+105 expected for nibble=15, vol=15");
        // Mix returns (sum / 1) * NAMCO163_MIX_SCALE = 105 * 261 = 27405
        // (v2.1.6 hardware-accurate 6.0x db_n163 level; was 105 * 64).
        assert_eq!(m.audio.mix(), 105 * NAMCO163_MIX_SCALE as i16);

        // Now swap the wavetable to nibble 0 — output should swing
        // negative: (0 - 8) * 15 = -120.
        m.cpu_write(0xF800, 0x10);
        m.cpu_write(0x4800, 0x00);
        assert_eq!(m.audio.channel_output(0), (0 - 8) * 15);
        assert!(m.audio.mix() < 0, "negative samples must yield <0 mix");
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn namco163_volume_zero_silences_channel() {
        // A channel with volume == 0 contributes 0 to the mix
        // regardless of the wavetable contents.
        let mut m = namco163_for_audio();
        n163_write_ram(&mut m, 0x10, false, 0xFF); // wavetable bytes
        n163_write_ram(&mut m, 0x7C, false, 0xFC); // length=4
        n163_write_ram(&mut m, 0x7E, false, 0x20); // wave_addr=0x20
        n163_write_ram(&mut m, 0x7F, false, 0x00); // vol=0, C=0
        assert_eq!(m.audio.channel_output(0), 0);
        assert_eq!(m.audio.mix(), 0);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn namco163_longwave_256_sample_wave_phase_wraps_and_reads_full_period() {
        // The `test_n163_longwave` accuracy criterion: long-period wavetables
        // (the case several emulators truncate). RustyNES uses the canonical
        // wave-length formula `L = 256 - (reg[base+4] & 0xFC)` and a 64-bit
        // phase accumulator wrapped at `L << 16`, so a full 256-sample wave and
        // a low frequency address the whole period without aliasing.
        let mut m = namco163_for_audio();
        // Fill 128 wave-RAM bytes = 256 nibbles with a ramp so every sample
        // index is distinguishable: nibble[i] = i & 0x0F.
        for byte in 0u8..0x80 {
            // low nibble = (2*byte)&0xF, high nibble = (2*byte+1)&0xF.
            let lo = (2 * byte) & 0x0F;
            let hi = (2 * byte + 1) & 0x0F;
            n163_write_ram(&mut m, byte, false, (hi << 4) | lo);
        }
        // Channel 8 ($78-$7F). N163 register layout (per Mesen `SoundReg`):
        // base+0 = freq lo, +2 = freq mid, +4 = freq hi (bits 0-1) + wave
        // length (bits 2-7), +6 = wave addr, +7 = volume. Set a frequency that
        // advances the phase by exactly one sample per clock update
        // (freq = 1<<16, i.e. freq-hi bit set) while keeping the wave length at
        // the max 256 (`256 - (reg & 0xFC)` with the length bits zero), then
        // step the wave across its full period and confirm every one of the
        // 256 sample indices is reached (no early wrap, no aliasing) — the
        // hallmark long-period behaviour.
        n163_write_ram(&mut m, 0x78, false, 0x00); // freq lo = 0
        n163_write_ram(&mut m, 0x7A, false, 0x00); // freq mid = 0
        n163_write_ram(&mut m, 0x7C, false, 0x01); // freq hi = 1 (-> 0x10000), length bits 0 -> L=256
        n163_write_ram(&mut m, 0x7E, false, 0x00); // wave_addr = 0
        n163_write_ram(&mut m, 0x7F, false, 0x0F); // volume=15, channel-count=0
        assert_eq!(
            m.audio.channel_length(0x78),
            256,
            "L must be 256, not truncated"
        );
        let mut seen = [false; 256];
        // N163 advances one channel every 15 CPU cycles; 256 samples * 15 = 3840
        // cycles cover the whole period, plus margin.
        for _ in 0..(256 * 15 + 15) {
            let idx = ((m.audio.channel_phase(0x78) >> 16) % 256) as usize;
            seen[idx] = true;
            m.audio.clock();
        }
        assert!(
            seen.iter().all(|&s| s),
            "long-period wave must reach every one of the 256 sample indices"
        );
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn namco163_clock_advances_only_active_channel() {
        // Two-channel setup: C=1, so channels 8 and 7 (bases $78, $70)
        // are active.  Set freq=0x01_0000 on channel 8 (so each tick
        // advances phase by 1 << 16) and freq=0 on channel 7.  After
        // 30 CPU cycles (= 2 audio updates), phase[ch=8] should have
        // advanced exactly once (the round-robin alternates 8/7/8/7...).
        let mut m = namco163_for_audio();
        // Channel 8 freq = 0x01_0000 → hi=01, mid=00, lo=00.
        n163_write_ram(&mut m, 0x78, false, 0x00); // freq lo
        n163_write_ram(&mut m, 0x7A, false, 0x00); // freq mid
        // length=4 (256 - 0xFC), freq-hi=01.
        n163_write_ram(&mut m, 0x7C, false, 0xFC | 0x01);
        n163_write_ram(&mut m, 0x7F, false, 0x10); // C=1 → 2 channels
        // Channel 7 freq = 0.
        n163_write_ram(&mut m, 0x70, false, 0x00);
        n163_write_ram(&mut m, 0x72, false, 0x00);
        n163_write_ram(&mut m, 0x74, false, 0xFC);

        // 15 cycles → channel 8 advances by 0x01_0000.
        for _ in 0..15 {
            m.notify_cpu_cycle();
        }
        let phase_ch8 = m.audio.channel_phase(0x78);
        // length=4, modulus = 4 << 16 = 0x40000, so 0x10000 stays.
        assert_eq!(phase_ch8, 0x0001_0000);
        let phase_ch7 = m.audio.channel_phase(0x70);
        assert_eq!(phase_ch7, 0, "ch7 must not advance on the first slot");

        // Next 15 cycles → channel 7 advances (by 0, so still 0); ch8
        // unchanged.
        for _ in 0..15 {
            m.notify_cpu_cycle();
        }
        assert_eq!(m.audio.channel_phase(0x78), 0x0001_0000);
        assert_eq!(m.audio.channel_phase(0x70), 0);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn namco163_sound_disable_bit_silences_mix() {
        // $E000 bit 6 set → audio chip is silenced.  Even with a
        // non-zero wavetable and volume, mix_audio returns 0.
        let mut m = namco163_for_audio();
        n163_write_ram(&mut m, 0x10, false, 0xFF);
        n163_write_ram(&mut m, 0x7C, false, 0xFC);
        n163_write_ram(&mut m, 0x7E, false, 0x20);
        n163_write_ram(&mut m, 0x7F, false, 0x0F);
        assert_ne!(m.mix_audio(), 0);
        // Set sound-disable: $E000 with bit 6 = 1.  Bits 0-5 also write
        // PRG bank 0; we just need the bit 6.
        m.cpu_write(0xE000, 0x40);
        assert!(m.sound_disabled);
        assert_eq!(m.mix_audio(), 0);
        // Clearing it re-enables.
        m.cpu_write(0xE000, 0x00);
        assert!(!m.sound_disabled);
        assert_ne!(m.mix_audio(), 0);
    }

    #[test]
    fn namco163_save_state_v1_loads_with_audio_defaults() {
        // A v1 (pre-audio) save-state blob should load on a v2 reader
        // with audio defaulted to silence (zero RAM, zero phase, zero
        // latch, sound_disabled=false).  Construct a synthetic v1 blob
        // by hand to exercise the backward-compat path.
        let mut donor = namco163_for_audio();
        // Mutate non-audio state so we can verify it round-trips.
        donor.prg[0] = 0x05;
        donor.chr[3] = 0x07;
        donor.nta[1] = 0x02;
        donor.irq_counter = 0x1234;
        donor.irq_pending = true;
        donor.audio.ram[0x40] = 0x99; // would normally serialize in v2

        // Build a v1 blob (no audio tail).
        let mut blob = Vec::new();
        blob.push(1u8);
        blob.extend_from_slice(&donor.prg);
        blob.extend_from_slice(&donor.chr);
        blob.extend_from_slice(&donor.nta);
        blob.push(donor.mirroring as u8);
        blob.extend_from_slice(&donor.irq_counter.to_le_bytes());
        blob.push(u8::from(donor.irq_pending));
        blob.extend_from_slice(&donor.prg_ram);
        blob.extend_from_slice(&donor.vram);

        let mut target = namco163_for_audio();
        // Pre-populate target with bogus audio state, then verify it
        // gets cleared by the v1 load path.
        target.audio.ram[0x40] = 0xAA;
        target.audio.addr_latch = 0x55;
        target.audio.auto_inc = true;
        target.sound_disabled = true;
        target.load_state(&blob).unwrap();
        assert_eq!(target.prg[0], 0x05);
        assert_eq!(target.chr[3], 0x07);
        assert_eq!(target.irq_counter, 0x1234);
        // Audio state should be default (silent).
        assert_eq!(target.audio.ram, [0u8; 128]);
        assert_eq!(target.audio.addr_latch, 0);
        assert!(!target.audio.auto_inc);
        assert!(!target.sound_disabled);
    }

    #[test]
    fn namco163_save_state_v2_round_trip() {
        // v2 → v2 round-trip preserves the full audio state.
        let mut donor = namco163_for_audio();
        n163_write_ram(&mut donor, 0x10, true, 0xAB);
        n163_write_ram(&mut donor, 0x7F, false, 0x35); // C=3 → 4 channels, vol=5
        donor.cpu_write(0xE000, 0x40); // sound disable
        let blob = donor.save_state();
        assert_eq!(blob[0], 2u8, "v2 tag expected");

        let mut target = namco163_for_audio();
        target.load_state(&blob).unwrap();
        assert_eq!(target.audio.ram[0x10], 0xAB);
        assert_eq!(target.audio.ram[0x7F], 0x35);
        assert!(target.sound_disabled);
        // addr_latch after the writes: $7F (we wrote $7F last,
        // auto_inc=false, so the latch stayed at $7F).
        assert_eq!(target.audio.addr_latch, 0x7F);
    }

    #[test]
    fn namco163_mapper_audio_off_path_latches_state_but_stays_silent() {
        // Mirrors the Sunsoft 5B feature-off test: the register decoders
        // run regardless of `mapper-audio`, so writes still land in the
        // internal RAM and the address-port latch advances.  With the
        // feature off, `notify_cpu_cycle` does not advance any phase
        // counters and `mix_audio` returns 0.
        let mut m = namco163_for_audio();
        // Address-port write + data-port write contract — works with
        // the feature off, because the decoders are unconditional.
        m.cpu_write(0xF800, 0x80 | 0x05);
        m.cpu_write(0x4800, 0x42);
        assert_eq!(m.audio.ram[0x05], 0x42);
        assert_eq!(m.audio.addr_latch, 0x06);
        assert!(m.audio.auto_inc);

        // Phase counters stay at zero whether or not we call clock()
        // (with the feature off, notify_cpu_cycle skips the clock; with
        // the feature on, we haven't touched the freq registers so the
        // phase still doesn't advance from the zero state).  Verify the
        // zero-init invariant directly.
        for _ in 0..256 {
            m.notify_cpu_cycle();
        }
        // Phase regs are at offsets +1/+3/+5 of each channel slot.
        for ch_base in (0x40..=0x78).step_by(8) {
            assert_eq!(m.audio.ram[ch_base + 1], 0, "phase lo @ {ch_base:#x}");
            assert_eq!(m.audio.ram[ch_base + 3], 0, "phase mid @ {ch_base:#x}");
            assert_eq!(m.audio.ram[ch_base + 5], 0, "phase hi @ {ch_base:#x}");
        }
    }
}

//! Konami VRC7 (mapper 85) -- banking, the VRC IRQ counter, and the on-cart
//! YM2413-derivative OPLL FM synthesizer.
//!
//! The banking and IRQ halves are ordinary VRC4-family behaviour. The audio
//! half is a cut-down OPLL: six FM channels driven from a fixed internal
//! patch ROM plus one user-programmable patch, clocked once every 36 CPU
//! cycles. The synthesizer itself lives in the shared OPLL core; this module
//! owns the mapper-side register file ([`Vrc7AudioRegs`]), the `$9010`/`$9030`
//! address/data port pair, and the `$E000` bit-7 audio-mute line.
//!
//! Audio is gated behind the `mapper-audio` Cargo feature (default ON); with
//! it off the register decoders still latch so save states remain portable
//! across feature configurations (ADR 0004). See ADR 0006 for the decision
//! record on landing VRC7 audio.
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

/// Version byte this board writes in its mapper save-state section.
///
/// **v1** (through v2.3.6) carried banking, IRQ, mirroring, PRG-RAM and the
/// *shadow* OPLL register bytes. **v2** (v2.3.7) appends the live synthesizer,
/// closing the save-state audio-continuity gap recorded in
/// `docs/accuracy-ledger.md`. `load_state` accepts both.
///
/// A build without `mapper-audio` has no synthesizer to describe, so it writes
/// **v1** and, on load, validates a v2 tail's length and ignores its contents.
/// That keeps the cross-build property this crate's feature documentation
/// promises — an audio build's save still loads in a no-audio build, and a
/// no-audio build's save still loads everywhere.
///
/// **This constant is the WRITE version only. Never key the accept set on it.**
/// `load_state` compares against the literals 1 and 2 for that reason: it once
/// compared against this constant, which is 1 here, so the condition collapsed
/// to "v1 only" and a no-audio build rejected v2 outright — the precise opposite
/// of the sentence above, which is what the code claimed to do while doing the
/// reverse. What a build can write and what it must accept are different sets,
/// and only the first varies by feature. Pinned by
/// `vrc7_load_state_accepts_a_v2_blob_on_every_build`.
#[cfg(feature = "mapper-audio")]
const VRC7_SECTION_VERSION: u8 = 2;
#[cfg(not(feature = "mapper-audio"))]
const VRC7_SECTION_VERSION: u8 = 1;

/// Bytes the v2 tail adds after the VRAM: `opll_clock_counter` (2),
/// `last_opll_sample` (2), and the self-versioned OPLL blob.
const VRC7_V2_TAIL_LEN: usize = 2 + 2 + rustynes_apu::OPLL_SNAPSHOT_LEN;

fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

/// VRC7 audio register snapshot.
///
/// Two latches: the OPLL register address (set by writes to `$9010`)
/// and the data byte (set by writes to `$9030` after `$9010`).  Per
/// ADR-0004, this is **decoded and latched but not synthesized** in
/// v0.9.x — the byte stream sits available for a future v1.x OPLL
/// integration, and save-state round-trip works in both directions
/// without an audio backend.
#[derive(Clone)]
struct Vrc7AudioRegs {
    /// Last 6-bit register address written to `$9010`.  YM2413 has 64
    /// addressable registers; VRC7 exposes a 6-channel subset.
    addr_latch: u8,
    /// Last data byte written to `$9030`.  Available for inspection /
    /// equivalence testing against a future OPLL backend.
    data_latch: u8,
    /// 64-entry shadow of the most recent data written to each OPLL
    /// register address.  A future synthesizer reads this on demand
    /// (e.g. on key-on) to seed channel state without re-running the
    /// register-write history.  Sized at 64 to match the full YM2413
    /// register space (the chip's 6 channels use $10-$15 / $20-$25 /
    /// $30-$35; instrument bytes are at $00-$07).
    regs: [u8; 64],
    /// Mirror of `$E000` bit 7 (expansion-sound silence). When set, a
    /// future synthesizer's output is forced to zero; banking + IRQ
    /// are unaffected.
    silenced: bool,
}

impl Default for Vrc7AudioRegs {
    fn default() -> Self {
        Self {
            addr_latch: 0,
            data_latch: 0,
            regs: [0u8; 64],
            silenced: false,
        }
    }
}

/// VRC7 (Mapper 85).  Banking + IRQ + (deferred per ADR-0004) FM audio
/// surface for Lagrange Point.
pub struct Vrc7 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,

    /// 8 KiB PRG bank at $8000-$9FFF.
    prg_0: u8,
    /// 8 KiB PRG bank at $A000-$BFFF.
    prg_1: u8,
    /// 8 KiB PRG bank at $C000-$DFFF.
    prg_2: u8,
    /// 1 KiB CHR banks at $0000-$1FFF (one entry per KiB).
    chr: [u8; 8],
    mirroring: Mirroring,

    // IRQ counter (identical shape to VRC6's).
    irq_latch: u8,
    irq_counter: u8,
    irq_enabled: bool,
    irq_enable_after_ack: bool,
    irq_mode_scanline: bool,
    irq_prescaler: i32,
    irq_pending: bool,

    /// PRG-RAM enable (bit 6 of `$E000`). When clear, `$6000-$7FFF`
    /// reads/writes are ignored.
    prg_ram_enable: bool,

    /// 8 KiB WRAM at `$6000-$7FFF`. Lagrange Point's boot routine runs a
    /// write-then-read-back self-test on this region (`STA ($00),Y` /
    /// `CMP ($00),Y` with `$00/$01 = $6000`); without backing storage the
    /// read-back always returned 0, the compare failed, and the game
    /// jumped to its lockup loop at `$EC2F` (blank gray screen — it never
    /// reaches CHR-RAM / nametable upload). Backed now, mirroring the
    /// VRC2/VRC4 WRAM fix (T-60-003b).
    prg_ram: Box<[u8]>,

    /// Audio register surface. Decoded and latched in v0.9.x; not yet
    /// synthesized (see ADR-0004).
    audio: Vrc7AudioRegs,

    /// OPLL FM synthesizer. Lives behind the `mapper-audio` feature
    /// to keep the no_std cross-compile cheap; when the feature is
    /// off, `mix_audio` returns 0 unconditionally (matching the
    /// pre-v1.1.0 ADR-0004 deferred state).
    #[cfg(feature = "mapper-audio")]
    opll: rustynes_apu::Opll,

    /// CPU-cycle counter for the OPLL native sample rate. NES NTSC
    /// CPU runs at 1,789,773 Hz; the OPLL native rate is 49,716 Hz.
    /// `1789773 / 49716 ≈ 35.997` — we tick the OPLL every 36 CPU
    /// cycles, which is correct to 0.008% (< 1 Hz tuning drift).
    #[cfg(feature = "mapper-audio")]
    opll_clock_counter: u16,

    /// Latest OPLL sample. The mapper holds this between OPLL ticks
    /// (every 36 CPU cycles) so `mix_audio` calls in between return
    /// the most-recent value. The APU's band-limited synthesis
    /// handles the rate conversion from OPLL's 49,716 Hz to the
    /// host sample rate.
    #[cfg(feature = "mapper-audio")]
    last_opll_sample: i16,
}

impl Vrc7 {
    /// Construct a new VRC7 mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] if the PRG-ROM size is not a
    /// non-zero multiple of 8 KiB or the CHR-ROM size is not a
    /// multiple of 1 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "VRC7 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
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
                "VRC7 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_0: 0,
            prg_1: 0,
            prg_2: 0,
            chr: [0; 8],
            mirroring,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_enable_after_ack: false,
            irq_mode_scanline: false,
            irq_prescaler: 341,
            irq_pending: false,
            prg_ram_enable: false,
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
            audio: Vrc7AudioRegs::default(),
            #[cfg(feature = "mapper-audio")]
            opll: rustynes_apu::Opll::new(rustynes_apu::OpllChipType::Vrc7),
            #[cfg(feature = "mapper-audio")]
            opll_clock_counter: 0,
            #[cfg(feature = "mapper-audio")]
            last_opll_sample: 0,
        })
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total_8k = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let last1 = total_8k - 1;
        let (bank, off_in_8k) = match addr {
            0x8000..=0x9FFF => (self.prg_0 as usize, addr as usize & 0x1FFF),
            0xA000..=0xBFFF => (self.prg_1 as usize, addr as usize & 0x1FFF),
            0xC000..=0xDFFF => (self.prg_2 as usize, addr as usize & 0x1FFF),
            0xE000..=0xFFFF => (last1, addr as usize & 0x1FFF),
            _ => return 0,
        };
        (bank % total_8k) * PRG_BANK_8K + off_in_8k
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

    /// Decode mirroring from the low 2 bits of `$E000`.  Per NESdev
    /// "VRC7": `00` = vertical, `01` = horizontal, `10` = single-screen
    /// A, `11` = single-screen B.
    fn decode_mirroring(value: u8) -> Mirroring {
        match value & 0x03 {
            0 => Mirroring::Vertical,
            1 => Mirroring::Horizontal,
            2 => Mirroring::SingleScreenA,
            _ => Mirroring::SingleScreenB,
        }
    }
}

impl Mapper for Vrc7 {
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
                // 8 KiB WRAM. Backed by storage so Lagrange Point's boot
                // RAM self-test (write then read-back) succeeds. The
                // enable bit (`$E000` bit 6) is modelled for completeness
                // but does not gate the backing store: the game toggles it
                // around the test, and real VRC7 emulators keep the WRAM
                // continuously addressable.
                self.prg_ram[(addr - 0x6000) as usize % self.prg_ram.len()]
            }
            0x8000..=0xFFFF => {
                let off = self.prg_offset(addr);
                self.prg_rom[off % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // VRC7 register decoding tolerates both A3 (`$_008`) and A4
        // (`$_010`) variants per board revision.  The high-nibble
        // selector picks the register family; within each family the
        // bank/IRQ/audio variant is chosen by bits 4-5 of the low byte.
        match addr & 0xF000 {
            0x6000 | 0x7000 => {
                // 8 KiB WRAM write (backed; see cpu_read).
                let len = self.prg_ram.len();
                self.prg_ram[(addr - 0x6000) as usize % len] = value;
            }
            0x8000 => {
                // $8000 selects PRG bank 0; $8010 / $8008 selects bank 1.
                if (addr & 0x0010) != 0 || (addr & 0x0008) != 0 {
                    self.prg_1 = value & 0x3F;
                } else {
                    self.prg_0 = value & 0x3F;
                }
            }
            0x9000 => {
                // $9000 (and $9008 mirror) -> PRG bank 2.
                // $9010 (and $9018 mirror) -> OPLL register address latch.
                // $9030 (and $9038 mirror) -> OPLL register data write.
                let sub = addr & 0x0030;
                if sub == 0x0010 {
                    self.audio.addr_latch = value & 0x3F;
                } else if sub == 0x0030 {
                    let idx = (self.audio.addr_latch & 0x3F) as usize;
                    self.audio.regs[idx] = value;
                    self.audio.data_latch = value;
                    // Forward to the OPLL synthesizer. The address was
                    // latched on the previous `$9010` write; per
                    // `Vrc7Audio.h` (Mesen2) this is the canonical
                    // shape — `WriteReg($9010, addr); WriteReg($9030, data)`.
                    // The 7-cycle inter-write delay Lagrange Point
                    // observes on real hardware is enforced by the CPU
                    // emitter; the chip latches each independently.
                    #[cfg(feature = "mapper-audio")]
                    self.opll.write_reg(self.audio.addr_latch, value);
                } else {
                    // $9000 / $9008 / $9020 / $9028 -> PRG bank 2.
                    self.prg_2 = value & 0x3F;
                }
            }
            0xA000 => {
                // CHR banks 0 / 1.
                if (addr & 0x0010) != 0 || (addr & 0x0008) != 0 {
                    self.chr[1] = value;
                } else {
                    self.chr[0] = value;
                }
            }
            0xB000 => {
                // CHR banks 2 / 3.
                if (addr & 0x0010) != 0 || (addr & 0x0008) != 0 {
                    self.chr[3] = value;
                } else {
                    self.chr[2] = value;
                }
            }
            0xC000 => {
                // CHR banks 4 / 5.
                if (addr & 0x0010) != 0 || (addr & 0x0008) != 0 {
                    self.chr[5] = value;
                } else {
                    self.chr[4] = value;
                }
            }
            0xD000 => {
                // CHR banks 6 / 7.
                if (addr & 0x0010) != 0 || (addr & 0x0008) != 0 {
                    self.chr[7] = value;
                } else {
                    self.chr[6] = value;
                }
            }
            0xE000 => {
                // $E000: mirroring (bits 1-0), WRAM enable (bit 6),
                // expansion-sound silence (bit 7).
                // $E008 / $E010: IRQ latch.
                if (addr & 0x0010) != 0 || (addr & 0x0008) != 0 {
                    self.irq_latch = value;
                } else {
                    self.mirroring = Self::decode_mirroring(value);
                    self.prg_ram_enable = (value & 0x40) != 0;
                    self.audio.silenced = (value & 0x80) != 0;
                }
            }
            0xF000 => {
                // $F000: IRQ control. $F008/$F010: IRQ acknowledge.
                if (addr & 0x0010) != 0 || (addr & 0x0008) != 0 {
                    self.irq_pending = false;
                    self.irq_enabled = self.irq_enable_after_ack;
                } else {
                    self.irq_enable_after_ack = (value & 0x01) != 0;
                    self.irq_enabled = (value & 0x02) != 0;
                    self.irq_mode_scanline = (value & 0x04) == 0;
                    if self.irq_enabled {
                        self.irq_counter = self.irq_latch;
                        self.irq_prescaler = 341;
                    }
                    self.irq_pending = false;
                }
            }
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
                    // Must go through the SAME banked offset `ppu_read` uses
                    // (`chr_offset`), not the raw PPU address — otherwise a
                    // game that banks CHR-RAM (Lagrange Point) writes tiles to
                    // one offset and reads them back from another, leaving the
                    // pattern tables effectively blank.
                    let off = self.chr_offset(addr);
                    let len = self.chr_rom.len();
                    self.chr_rom[off % len] = value;
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
        // Advance the OPLL synthesizer every 36 CPU cycles, matching
        // the NES NTSC CPU clock / OPLL native sample rate ratio.
        // Holds the produced sample in `last_opll_sample` for the
        // bus's per-APU-sample `mix_audio` calls.
        #[cfg(feature = "mapper-audio")]
        {
            self.opll_clock_counter = self.opll_clock_counter.wrapping_add(1);
            if self.opll_clock_counter >= 36 {
                self.opll_clock_counter = 0;
                self.last_opll_sample = self.opll.calc();
            }
        }

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

    /// Mix the current OPLL sample into the APU's external-audio
    /// channel. Returns 0 when the cartridge's expansion-sound
    /// silence bit (`$E000` bit 7) is set OR the `mapper-audio`
    /// feature is off; otherwise returns the most-recent OPLL
    /// sample in the i16 range [-4095, 4095] (the chip's
    /// 13-bit DAC scaled to 14-bit signed via `<< 1` in the
    /// `lookup_exp_table` final stage).
    #[cfg(feature = "mapper-audio")]
    fn mix_audio(&mut self) -> i32 {
        if self.audio.silenced {
            0
        } else {
            i32::from(self.last_opll_sample)
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 85,
            name: "VRC7".into(),
            mirroring: crate::mapper::mirroring_name(self.current_mirroring()),
            ..Default::default()
        };
        info.prg_banks
            .push(("PRG0".into(), format!("{:#04x}", self.prg_0)));
        info.prg_banks
            .push(("PRG1".into(), format!("{:#04x}", self.prg_1)));
        info.prg_banks
            .push(("PRG2".into(), format!("{:#04x}", self.prg_2)));
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
        info.extra.push((
            "audio".into(),
            "deferred (ADR-0004; mapper 85 audio = silent)".into(),
        ));
        info.extra.push((
            "audio_addr".into(),
            format!("{:#04x}", self.audio.addr_latch),
        ));
        info.extra.push((
            "audio_data".into(),
            format!("{:#04x}", self.audio.data_latch),
        ));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        // v1 layout (audio synthesis deferred per ADR-0004):
        //   version(1)
        //   prg_0 / prg_1 / prg_2 (3)
        //   chr[0..8] (8)
        //   mirroring(1) + prg_ram_enable(1)
        //   irq_latch(1) + irq_counter(1) + irq_enabled(1) +
        //   irq_enable_after_ack(1) + irq_mode_scanline(1) +
        //   irq_prescaler(4 le) + irq_pending(1)
        //   audio addr_latch(1) + data_latch(1) + silenced(1) +
        //   audio.regs[0..64] (64)
        //   vram (2 KiB)
        //
        // v2 (v2.3.7) is that commit: it appends, after the VRAM,
        //   opll_clock_counter(2 le) + last_opll_sample(2 le)
        //   + opll blob (OPLL_SNAPSHOT_LEN bytes, self-versioned)
        // closing the `docs/accuracy-ledger.md` row that recorded the FM
        // voice resuming from arbitrary envelope + phase state after a
        // rewind / rollback / TAS restore. `load_state` still accepts a v1
        // blob, which leaves the synthesizer wherever it was — the exact
        // pre-v2.3.7 behaviour, so an old save is no worse than it was.
        // version(1) + prg(3) + chr(8) + mirroring(1) + prg_ram_enable(1)
        //   + irq_latch(1) + irq_counter(1) + irq_enabled(1)
        //   + irq_enable_after_ack(1) + irq_mode_scanline(1)
        //   + irq_prescaler(4) + irq_pending(1)
        //   + audio addr_latch(1) + data_latch(1) + silenced(1) + regs(64)
        // = 1 + 3 + 8 + 1 + 1 + 5 + 5 + 67 = 91
        let scalar_len = 1 + 3 + 8 + 1 + 1 + 10 + 3 + 64;
        // The v2 tail only exists on a `mapper-audio` build, so only reserve for
        // it there — a no-audio build would otherwise over-allocate ~1.3 KiB on
        // every save for a tail it never writes.
        #[cfg(feature = "mapper-audio")]
        let mut out = Vec::with_capacity(scalar_len + self.vram.len() + VRC7_V2_TAIL_LEN);
        #[cfg(not(feature = "mapper-audio"))]
        let mut out = Vec::with_capacity(scalar_len + self.vram.len());
        out.push(VRC7_SECTION_VERSION); // version
        out.push(self.prg_0);
        out.push(self.prg_1);
        out.push(self.prg_2);
        out.extend_from_slice(&self.chr);
        out.push(self.mirroring as u8);
        out.push(u8::from(self.prg_ram_enable));
        out.push(self.irq_latch);
        out.push(self.irq_counter);
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_enable_after_ack));
        out.push(u8::from(self.irq_mode_scanline));
        out.extend_from_slice(&self.irq_prescaler.to_le_bytes());
        out.push(u8::from(self.irq_pending));
        out.push(self.audio.addr_latch);
        out.push(self.audio.data_latch);
        out.push(u8::from(self.audio.silenced));
        out.extend_from_slice(&self.audio.regs);
        out.extend_from_slice(&self.vram);
        // --- v2 tail: the live synthesizer ---
        #[cfg(feature = "mapper-audio")]
        {
            out.extend_from_slice(&self.opll_clock_counter.to_le_bytes());
            out.extend_from_slice(&self.last_opll_sample.to_le_bytes());
            out.extend_from_slice(&self.opll.snapshot());
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        // version(1) + prg(3) + chr(8) + mirroring(1) + prg_ram_enable(1)
        //   + irq_latch(1) + irq_counter(1) + irq_enabled(1)
        //   + irq_enable_after_ack(1) + irq_mode_scanline(1)
        //   + irq_prescaler(4) + irq_pending(1)
        //   + audio addr_latch(1) + data_latch(1) + silenced(1) + regs(64)
        // = 1 + 3 + 8 + 1 + 1 + 5 + 5 + 67 = 91
        let scalar_len = 1 + 3 + 8 + 1 + 1 + 10 + 3 + 64;
        let core_expected = scalar_len + self.vram.len();
        if data.len() < core_expected {
            return Err(MapperError::Truncated {
                expected: core_expected,
                got: data.len(),
            });
        }
        let version = data[0];
        // Both READABLE versions, spelled as literals — deliberately NOT
        // `VRC7_SECTION_VERSION`, which is what this WRITES and is 1 on a
        // no-audio build. Keying the accept set on the write version made the
        // condition collapse to `version != 1` there, so a no-audio build
        // REJECTED a v2 blob outright — the exact opposite of the
        // validate-then-ignore portability ADR 0004 asks for, and of what the
        // comment on `VRC7_SECTION_VERSION` claimed. What a build can write and
        // what it must accept are different sets; only the first varies by
        // feature. Caught in review by two independent bots.
        if version != 1 && version != 2 {
            return Err(MapperError::UnsupportedVersion(version));
        }

        // VALIDATE EVERYTHING BEFORE MUTATING ANYTHING.
        //
        // The v2 tail introduced a failure that can occur AFTER the core fields
        // have been assigned, which the v1 layout could not: v1 validated its
        // whole length and version up front, so once it started writing it could
        // not fail. A truncated or corrupt v2 tail used to return `Err` with
        // `prg_0`, `chr`, the IRQ state and 2 KiB of VRAM already overwritten --
        // a mapper left in a state that is neither the old one nor the new one,
        // while the caller reports the load as failed and keeps running.
        //
        // `Opll::restore` was already atomic internally, which is exactly what
        // made this easy to miss: the guarantee existed one level down and was
        // silently discarded one level up. Parse into a temporary here, so this
        // function has the same all-or-nothing property its own comments claim.
        // Caught in review; the truncation test missed it because it asserted on
        // the return value and never on the target.
        #[cfg(feature = "mapper-audio")]
        let staged_opll = if version >= 2 {
            let tail = &data[core_expected..];
            if tail.len() < VRC7_V2_TAIL_LEN {
                return Err(MapperError::Truncated {
                    expected: core_expected + VRC7_V2_TAIL_LEN,
                    got: data.len(),
                });
            }
            let mut opll = self.opll.clone();
            opll.restore(&tail[4..])
                .map_err(|e| MapperError::Invalid(format!("VRC7 OPLL state: {e}")))?;
            Some((
                u16::from_le_bytes(tail[0..2].try_into().expect("length checked above")),
                i16::from_le_bytes(tail[2..4].try_into().expect("length checked above")),
                opll,
            ))
        } else {
            None
        };
        // A no-audio build has no synthesizer to stage into, but must still
        // reject a truncated tail identically -- the same blob has to be
        // accepted or refused the same way on every build.
        #[cfg(not(feature = "mapper-audio"))]
        if version >= 2 && data.len() - core_expected < VRC7_V2_TAIL_LEN {
            return Err(MapperError::Truncated {
                expected: core_expected + VRC7_V2_TAIL_LEN,
                got: data.len(),
            });
        }

        self.prg_0 = data[1];
        self.prg_1 = data[2];
        self.prg_2 = data[3];
        self.chr.copy_from_slice(&data[4..12]);
        self.mirroring = match data[12] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.prg_ram_enable = data[13] != 0;
        self.irq_latch = data[14];
        self.irq_counter = data[15];
        self.irq_enabled = data[16] != 0;
        self.irq_enable_after_ack = data[17] != 0;
        self.irq_mode_scanline = data[18] != 0;
        self.irq_prescaler = i32::from_le_bytes(
            data[19..23]
                .try_into()
                .map_err(|_| MapperError::Invalid("prescaler".into()))?,
        );
        self.irq_pending = data[23] != 0;
        self.audio.addr_latch = data[24];
        self.audio.data_latch = data[25];
        self.audio.silenced = data[26] != 0;
        self.audio.regs.copy_from_slice(&data[27..91]);
        self.vram.copy_from_slice(&data[91..91 + self.vram.len()]);

        // --- v2 tail: the live synthesizer ---
        //
        // A v1 blob stops here. That is deliberately NOT an error and NOT a
        // reset: it is the pre-v2.3.7 behaviour, in which the synthesizer kept
        // running from whatever state it held. An old save is therefore exactly
        // as (in)accurate as it always was, rather than newly silent.
        // Commit the already-validated synthesizer. Infallible by construction:
        // every way this could fail was exercised above, before the first write.
        #[cfg(feature = "mapper-audio")]
        if let Some((counter, sample, opll)) = staged_opll {
            self.opll_clock_counter = counter;
            self.last_opll_sample = sample;
            self.opll = opll;
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

    fn vrc7_default() -> Vrc7 {
        // 8 × 8 KiB PRG (bank index byte at offset 0 of each bank to make
        // the read path observable) + 16 × 1 KiB CHR (likewise).
        Vrc7::new(synth(8), synth_chr(16), Mirroring::Vertical).unwrap()
    }

    #[test]
    fn vrc7_prg_banking_three_switchable_plus_fixed_last() {
        let mut m = vrc7_default();
        // $8000 = PRG bank 0 (window $8000-$9FFF). Pick bank 5.
        m.cpu_write(0x8000, 5);
        // $8010 = PRG bank 1 ($A000-$BFFF). Pick bank 3.
        m.cpu_write(0x8010, 3);
        // $9000 = PRG bank 2 ($C000-$DFFF). Pick bank 7.
        m.cpu_write(0x9000, 7);
        // Read at the start of each window returns the synth's bank-index
        // byte (bank index lives at offset 0 of each 8 KiB bank).
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xA000), 3);
        assert_eq!(m.cpu_read(0xC000), 7);
        // $E000-$FFFF is fixed to the LAST bank (synth has 8 banks → 7).
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn vrc7_prg_banking_accepts_a3_a4_mirror() {
        // $8008 is the A3 mirror of $8010 → both select PRG bank 1.
        let mut m = vrc7_default();
        m.cpu_write(0x8008, 4);
        assert_eq!(m.cpu_read(0xA000), 4);
        m.cpu_write(0x8010, 2);
        assert_eq!(m.cpu_read(0xA000), 2);
    }

    #[test]
    fn vrc7_chr_banking_all_eight_slots() {
        // CHR banks 0..=7 are addressable at $A000 / $A010 / $B000 /
        // $B010 / $C000 / $C010 / $D000 / $D010.  Each 1 KiB CHR bank
        // in the synth ROM carries its bank index at offset 0.
        let mut m = vrc7_default();
        let writes = [
            (0xA000u16, 1u8, 0x0000u16),
            (0xA010, 2, 0x0400),
            (0xB000, 3, 0x0800),
            (0xB010, 4, 0x0C00),
            (0xC000, 5, 0x1000),
            (0xC010, 6, 0x1400),
            (0xD000, 7, 0x1800),
            (0xD010, 8, 0x1C00),
        ];
        for (addr, bank, _) in writes {
            m.cpu_write(addr, bank);
        }
        for (_, bank, ppu_addr) in writes {
            assert_eq!(m.ppu_read(ppu_addr), bank, "CHR slot for {ppu_addr:#x}");
        }
    }

    #[test]
    fn vrc7_mirroring_decode_from_e000_low_bits() {
        let mut m = vrc7_default();
        // 00 = Vertical (the default).
        m.cpu_write(0xE000, 0b0000_0000);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        // 01 = Horizontal.
        m.cpu_write(0xE000, 0b0000_0001);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // 10 = SingleScreen A.
        m.cpu_write(0xE000, 0b0000_0010);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        // 11 = SingleScreen B.
        m.cpu_write(0xE000, 0b0000_0011);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
    }

    #[test]
    fn vrc7_irq_counter_cycle_mode_pending() {
        // CPU-cycle mode: counter increments every CPU cycle; on $FF
        // it reloads from latch and asserts IRQ.  Same shape as VRC6.
        let mut m = vrc7_default();
        // Latch: 0xFE (so we need only 2 ticks to wrap from 0xFE -> 0xFF -> 0x00 + pending).
        m.cpu_write(0xE008, 0xFE); // $E008 = IRQ latch
        // Control: enable + cycle mode (mode bit 2 = 1 means CPU cycle).
        // Bit 0 = enable_after_ack; bit 1 = enable; bit 2 = mode (1=cycle, 0=scanline).
        m.cpu_write(0xF000, 0b0000_0110);
        // After enable, counter = latch = 0xFE.  Ticking until pending:
        // 0xFE -> 0xFF (clock 1), pending fires (clock 2 reloads from latch).
        m.notify_cpu_cycle();
        assert!(!m.irq_pending(), "after 1 cycle, counter only at 0xFF");
        m.notify_cpu_cycle();
        assert!(m.irq_pending(), "after 2 cycles, pending should be set");
    }

    #[test]
    fn vrc7_irq_ack_clears_pending_and_restores_enable_state() {
        // After IRQ fires, $F010 ack clears pending and restores
        // enable from enable_after_ack.  Match the VRC6 contract.
        let mut m = vrc7_default();
        m.cpu_write(0xE008, 0xFE);
        m.cpu_write(0xF000, 0b0000_0111); // enable_after_ack=1, enable=1, cycle mode
        m.notify_cpu_cycle();
        m.notify_cpu_cycle();
        assert!(m.irq_pending());
        m.cpu_write(0xF010, 0); // ack
        assert!(!m.irq_pending());
        assert!(m.irq_enabled, "enable should be restored from after_ack");
    }

    #[test]
    fn vrc7_audio_register_latch_round_trip() {
        // Per ADR-0004 the synthesizer is deferred, but the register
        // surface must still latch state cleanly.  This test pins the
        // contract a future v1.x OPLL integration will read from.
        let mut m = vrc7_default();
        m.cpu_write(0x9010, 0x10); // OPLL register address = 0x10
        assert_eq!(m.audio.addr_latch, 0x10);
        m.cpu_write(0x9030, 0x42); // OPLL data byte
        assert_eq!(m.audio.data_latch, 0x42);
        assert_eq!(m.audio.regs[0x10], 0x42);
        // A second address+data pair: write 0x30 (channel-1 volume +
        // instrument select) then a different data byte.
        m.cpu_write(0x9010, 0x30);
        m.cpu_write(0x9030, 0x5F); // top nibble = inst 5, low nibble = vol 0xF
        assert_eq!(m.audio.regs[0x30], 0x5F);
        // Earlier write at 0x10 is preserved (independent slots).
        assert_eq!(m.audio.regs[0x10], 0x42);
    }

    #[test]
    fn vrc7_audio_custom_instrument_bytes_route_to_registers_0_through_7() {
        // The 8 custom-instrument bytes live at OPLL registers $00-$07.
        // Confirm they land in the right slots when written through
        // the $9010 / $9030 protocol.
        let mut m = vrc7_default();
        for i in 0..8u8 {
            m.cpu_write(0x9010, i);
            m.cpu_write(0x9030, 0xA0 | i); // distinct payload per slot
            assert_eq!(m.audio.regs[i as usize], 0xA0 | i);
        }
    }

    #[test]
    fn vrc7_mix_audio_silent_with_no_key_on() {
        // Sprint 1.2 (v1.1.0): OPLL is wired but no channel has been
        // keyed on — every slot's envelope sits at EG_MUTE, so every
        // OPLL sample is 0. The mix_audio output should therefore be
        // 0 across the entire register-surface scan.
        let mut m = vrc7_default();
        for reg in 0..=0x35u8 {
            m.cpu_write(0x9010, reg);
            m.cpu_write(0x9030, 0x00); // zero-fill — no key-on bits
        }
        // Tick the OPLL several times to confirm calc() also returns 0.
        for _ in 0..200 {
            m.notify_cpu_cycle();
        }
        assert_eq!(
            m.mix_audio(),
            0,
            "VRC7 mix_audio must be silent without key-on; got non-zero"
        );
    }

    #[test]
    fn vrc7_mix_audio_silenced_by_e000_bit7() {
        // Even with a keyed-on channel, the `$E000` expansion-sound
        // silence bit (bit 7) must force mix_audio to 0. Mesen2 calls
        // this the "muted" flag in Vrc7Audio.h.
        let mut m = vrc7_default();
        // Set up channel 0: instrument 1, fnum 256, block 4, key-on,
        // max volume (volume bits low = max — OPLL volume is attenuation).
        m.cpu_write(0x9010, 0x30); // $30 = inst/volume for ch 0
        m.cpu_write(0x9030, 0x10); // inst 1, volume 0 (loudest)
        m.cpu_write(0x9010, 0x10); // $10 = fnum low for ch 0
        m.cpu_write(0x9030, 0x00);
        m.cpu_write(0x9010, 0x20); // $20 = fnum high + block + key for ch 0
        m.cpu_write(0x9030, 0x35); // key-on bit set + block + fnum high
        // Tick enough cycles for the envelope to clear Damp → Attack.
        for _ in 0..16_384 {
            m.notify_cpu_cycle();
        }
        // Now flip the silence bit on `$E000`.
        m.cpu_write(0xE000, 0x80);
        assert_eq!(
            m.mix_audio(),
            0,
            "silenced VRC7 must mix to 0; got non-zero"
        );
        // Verify the OPLL still ticks (its internal state advances) —
        // re-clear silence and the audio should resume.
        m.cpu_write(0xE000, 0x00);
        // We don't assert non-zero here because the OPLL might have
        // landed on a zero-crossing this exact tick — just confirm
        // the silenced gate is the only thing stopping output.
        // (The non-zero output is covered by the next test.)
    }

    #[test]
    fn vrc7_opll_register_writes_forwarded_on_data_write() {
        // `$9030` data writes must be forwarded to the OPLL's
        // register shadow. Verifies the integration point even
        // without ticking the synth.
        let mut m = vrc7_default();
        m.cpu_write(0x9010, 0x20); // address latch = $20
        m.cpu_write(0x9030, 0x55); // data write
        // Snapshot stores the byte in both the mapper's audio.regs
        // (for save-state round-trip) and the OPLL's register shadow.
        assert_eq!(m.audio.regs[0x20], 0x55);
        #[cfg(feature = "mapper-audio")]
        assert_eq!(
            m.opll.read_reg(0x20),
            0x55,
            "OPLL register shadow should mirror $9030 writes"
        );
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn vrc7_keyed_on_channel_produces_nonzero_mix_within_one_envelope() {
        // End-to-end: configure channel 0 with VRC7 patch 1, key on,
        // run enough CPU cycles for Damp → Attack to progress past
        // EG_MUTE, and observe a non-zero mix_audio sample.
        let mut m = vrc7_default();
        // Channel 0 setup matching the OPLL unit test's manual setup.
        // $30 → bits 3-0 = volume (attenuation), bits 7-4 = instrument
        m.cpu_write(0x9010, 0x30);
        m.cpu_write(0x9030, 0x10); // inst=1, vol=0
        m.cpu_write(0x9010, 0x10);
        m.cpu_write(0x9030, 0x80); // fnum low byte
        m.cpu_write(0x9010, 0x20);
        m.cpu_write(0x9030, 0x35); // key-on + block(2) + fnum high(1)
        // Each OPLL sample = 36 CPU cycles. 16,384 CPU cycles = ~455
        // OPLL samples = ~9 ms of audio. Damp → Attack happens within
        // a few hundred OPLL samples for any non-saturated AR.
        // u32: `mix_audio` widened to i32 in v2.2.3 (A1).
        let mut peak_abs: u32 = 0;
        for _ in 0..16_384 {
            m.notify_cpu_cycle();
            let s = m.mix_audio();
            peak_abs = peak_abs.max(s.unsigned_abs());
        }
        assert!(
            peak_abs > 0,
            "expected non-zero VRC7 mix after key-on + 16k cycles; got peak_abs={peak_abs}"
        );
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn vrc7_opll_ticks_every_36_cpu_cycles() {
        // The OPLL is clocked at NES NTSC CPU rate / 36. Verify the
        // internal counter rolls over exactly on the 36th call to
        // notify_cpu_cycle by watching eg_counter (which advances
        // once per OPLL tick inside `update_slots`).
        let mut m = vrc7_default();
        // No way to read eg_counter through the public API, but we
        // CAN read opll_clock_counter via direct field access in
        // this module-local test. After 35 cycles, counter = 35;
        // after 36, counter resets to 0 and the OPLL has advanced.
        for _ in 0..35 {
            m.notify_cpu_cycle();
        }
        assert_eq!(m.opll_clock_counter, 35);
        m.notify_cpu_cycle();
        assert_eq!(
            m.opll_clock_counter, 0,
            "counter should reset on 36th cycle"
        );
    }

    #[test]
    fn vrc7_save_state_round_trip_preserves_banking_irq_and_audio_latches() {
        // v1 round-trip: configure banking, IRQ counter mid-state, and
        // audio register latches → save → reload into a fresh mapper
        // → all fields match.
        let mut m = vrc7_default();
        m.cpu_write(0x8000, 5);
        m.cpu_write(0x8010, 3);
        m.cpu_write(0x9000, 7);
        m.cpu_write(0xA000, 1);
        m.cpu_write(0xD010, 6);
        m.cpu_write(0xE000, 0b1100_0001); // Horizontal + WRAM enable + audio silenced
        m.cpu_write(0xE008, 0x80); // IRQ latch
        m.cpu_write(0xF000, 0b0000_0011); // enable + scanline mode
        // Audio register stream.
        m.cpu_write(0x9010, 0x30);
        m.cpu_write(0x9030, 0x5F);
        let blob = m.save_state();
        // v1 through v2.3.6; v2 since v2.3.7, which appends the live OPLL.
        // A build without `mapper-audio` has no synthesizer to describe and
        // still writes v1 — see `VRC7_SECTION_VERSION`.
        assert_eq!(blob[0], VRC7_SECTION_VERSION, "VRC7 save-state version tag");

        let mut target = vrc7_default();
        target.load_state(&blob).unwrap();
        assert_eq!(target.cpu_read(0x8000), 5);
        assert_eq!(target.cpu_read(0xA000), 3);
        assert_eq!(target.cpu_read(0xC000), 7);
        assert_eq!(target.ppu_read(0x0000), 1);
        assert_eq!(target.ppu_read(0x1C00), 6);
        assert_eq!(target.current_mirroring(), Mirroring::Horizontal);
        assert!(target.prg_ram_enable);
        assert!(target.audio.silenced);
        assert_eq!(target.irq_latch, 0x80);
        assert!(target.irq_enabled);
        // We wrote 0b0000_0011 → bit 2 (mode) = 0 → scanline mode is on
        // (the predicate is `(value & 0x04) == 0`).
        assert!(target.irq_mode_scanline);
        assert_eq!(target.audio.regs[0x30], 0x5F);
    }

    #[test]
    fn vrc7_save_state_rejects_unknown_version() {
        // Pre-v1 there is no VRC7 save-state; a future v1.x bumps to 2.
        // Until then, any version != 1 must be rejected cleanly.
        let m = vrc7_default();
        let mut blob = m.save_state();
        blob[0] = 99;
        let mut target = vrc7_default();
        let err = target.load_state(&blob).expect_err("must reject");
        assert!(
            matches!(err, MapperError::UnsupportedVersion(99)),
            "expected UnsupportedVersion(99), got {err:?}"
        );
    }

    #[test]
    fn vrc7_namco163_mapper_audio_off_path_latches_state_but_stays_silent() {
        // ADR-0004 invariant: register decoders unconditionally latch
        // even when the synthesizer is absent.  Confirm latching works
        // identically regardless of the `mapper-audio` feature flag
        // (the VRC7 surface does not branch on the flag — synthesis
        // is just absent in v0.9.x, period).
        let mut m = vrc7_default();
        m.cpu_write(0x9010, 0x15);
        m.cpu_write(0x9030, 0x77);
        assert_eq!(m.audio.regs[0x15], 0x77);
        // Drive a bunch of CPU cycles → no audio side-effects, but
        // IRQ counter is unaffected if not enabled.
        for _ in 0..1000 {
            m.notify_cpu_cycle();
        }
        assert_eq!(
            m.mix_audio(),
            0,
            "feature-off path must remain silent (matches feature-on for VRC7 v0.9.x)"
        );
    }

    // -----------------------------------------------------------------------
    // Save-state audio continuity (v2.3.7 — closes the accuracy-ledger row)
    // -----------------------------------------------------------------------

    /// Key a note on channel 0 with a real melodic patch, so the OPLL has
    /// non-trivial envelope + phase state to carry.
    #[cfg(feature = "mapper-audio")]
    fn key_on_channel_0(m: &mut Vrc7) {
        // $3x: high nibble = instrument (1 = the first Konami melodic patch),
        // low nibble = attenuation (0 = loudest).
        m.cpu_write(0x9010, 0x30);
        m.cpu_write(0x9030, 0x10);
        // $1x: F-number low 8 bits.
        m.cpu_write(0x9010, 0x10);
        m.cpu_write(0x9030, 0xAD);
        // $2x: bit5 sustain, bit4 key-on, bits3-1 block, bit0 F-number bit 8.
        m.cpu_write(0x9010, 0x20);
        m.cpu_write(0x9030, 0x15);
    }

    /// Run `n` CPU cycles and return every mixed sample, so two timelines can
    /// be compared as a waveform rather than as a single instant.
    #[cfg(feature = "mapper-audio")]
    fn run_capture(m: &mut Vrc7, n: usize) -> Vec<i32> {
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            m.notify_cpu_cycle();
            out.push(m.mix_audio());
        }
        out
    }

    /// **The test the ledger row existed for.** A restored VRC7 must resume the
    /// note that was playing, sample for sample.
    ///
    /// Before v2.3.7 the section carried only the shadow register bytes, so the
    /// restored synthesizer started from its power-on state and this comparison
    /// failed on the very first sample after the envelope diverged. Deleting
    /// the v2 tail from `save_state` reproduces that failure — the mutation
    /// check for this test.
    #[cfg(feature = "mapper-audio")]
    #[test]
    fn vrc7_save_state_carries_the_live_opll_so_audio_resumes_identically() {
        let mut source = vrc7_default();
        key_on_channel_0(&mut source);
        // Advance far enough that the envelope is well past attack and the
        // phase accumulators hold values no reset could coincidentally match.
        let _ = run_capture(&mut source, 20_000);

        let blob = source.save_state();
        assert_eq!(blob[0], 2, "a mapper-audio build must write section v2");

        let expected = run_capture(&mut source, 4_000);
        assert!(
            expected.iter().any(|&s| s != 0),
            "fixture produced silence — the test would pass vacuously"
        );

        let mut restored = vrc7_default();
        restored.load_state(&blob).expect("v2 blob must load");
        let got = run_capture(&mut restored, 4_000);

        assert_eq!(
            got, expected,
            "the restored VRC7 did not resume the note that was playing: the OPLL \
             envelope + phase state is not surviving the save state"
        );
    }

    /// The v2 tail is additive: a v1 blob (every save written before v2.3.7)
    /// still loads. It leaves the synthesizer untouched, which is exactly the
    /// pre-v2.3.7 behaviour — an old save is no worse than it always was.
    #[test]
    fn vrc7_load_state_still_accepts_a_v1_blob() {
        let mut source = vrc7_default();
        source.cpu_write(0x8000, 5);
        source.cpu_write(0x9010, 0x15);
        source.cpu_write(0x9030, 0x77);
        let blob = source.save_state();

        // Synthesize the v1 form: version byte 1, and no tail past the VRAM.
        let core_len = 91 + source.vram.len();
        let mut v1 = blob[..core_len].to_vec();
        v1[0] = 1;

        let mut target = vrc7_default();
        target.load_state(&v1).expect("a v1 blob must still load");
        assert_eq!(target.prg_0, 5, "v1 core fields must round-trip");
        assert_eq!(target.audio.regs[0x15], 0x77);
    }

    /// **Every build must ACCEPT a v2 blob, including one that cannot write it.**
    ///
    /// Regression for a defect two review bots caught independently: the accept
    /// check read `version != 1 && version != VRC7_SECTION_VERSION`, and
    /// `VRC7_SECTION_VERSION` is 1 on a no-audio build — so the condition
    /// collapsed to `version != 1` there and a v2 blob was rejected outright.
    /// That is the exact opposite of the validate-then-ignore portability
    /// ADR 0004 asks for, and the opposite of what the constant's own doc
    /// comment claimed.
    ///
    /// The lesson, which is why this test exists rather than a one-line diff:
    /// **what a build can WRITE and what it must ACCEPT are different sets, and
    /// only the first varies by feature.** Deriving one from the other reads as
    /// tidy and silently couples them.
    #[test]
    fn vrc7_load_state_accepts_a_v2_blob_on_every_build() {
        let mut source = vrc7_default();
        source.cpu_write(0x8000, 5);

        // On a `mapper-audio` build `save_state` already emits v2. On a no-audio
        // build it emits v1 with no tail, so synthesize the v2 shape: the load
        // path validates that tail's LENGTH on every build and reads its
        // CONTENTS only where there is a synthesizer, so zeros are correct here.
        #[cfg(feature = "mapper-audio")]
        let blob = source.save_state();
        #[cfg(not(feature = "mapper-audio"))]
        let blob = {
            let mut b = source.save_state();
            b[0] = 2;
            b.resize(b.len() + VRC7_V2_TAIL_LEN, 0);
            b
        };
        assert_eq!(blob[0], 2, "the fixture must be a v2 blob");

        let mut target = vrc7_default();
        target
            .load_state(&blob)
            .expect("a v2 blob must load on every build, whether or not it can write one");
        assert_eq!(target.prg_0, 5, "the core fields must still round-trip");
    }

    /// A v2 blob truncated inside its tail must be rejected, not partially
    /// applied. This is untrusted input: a save state is a file on disk.
    #[cfg(feature = "mapper-audio")]
    #[test]
    fn vrc7_load_state_rejects_a_truncated_v2_tail() {
        let mut source = vrc7_default();
        key_on_channel_0(&mut source);
        let _ = run_capture(&mut source, 500);
        let blob = source.save_state();

        // Give the target DIFFERENT state from the source, so a partial write
        // is observable rather than coincidentally identical.
        let mut target = vrc7_default();
        target.cpu_write(0x8000, 3);
        target.cpu_write(0x9000, 6);
        let pristine = target.save_state();

        let err = target
            .load_state(&blob[..blob.len() - 1])
            .expect_err("a truncated v2 tail must be rejected");
        assert!(
            matches!(err, MapperError::Truncated { .. }),
            "expected Truncated, got {err:?}"
        );

        // The half this test used to be missing. Returning `Err` is not enough:
        // `load_state` assigned the core fields BEFORE validating the v2 tail, so
        // a rejected load left the mapper neither in its old state nor the new
        // one, while the caller reported failure and kept running. Asserting only
        // on the return value cannot see that -- which is why review found it and
        // this test did not.
        assert_eq!(
            target.save_state(),
            pristine,
            "a rejected load mutated the mapper: load_state is not atomic"
        );
    }
}

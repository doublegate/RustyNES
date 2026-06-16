//! Bandai FCG (iNES mappers 16 and 159) implementation.
//!
//! Covers the Bandai FCG-1/FCG-2 and LZ93D50 ASICs. Banking: a 16 KiB
//! switchable PRG bank at `$8000-$BFFF` (last bank fixed at `$C000`), eight
//! independent 1 KiB CHR banks, software mirroring control, and a 16-bit
//! down-counting CPU-cycle (M2) IRQ. Some boards add a serial I²C EEPROM
//! (24C02 on mapper 16 submapper 5, X24C01 on mapper 159).
//!
//! # Register window (`nesdev_wiki/INES_Mapper_016.xhtml`)
//!
//! The register block is the same set of offsets `$0-$D`, but the decode
//! window differs by submapper:
//!
//! - Submapper 4 (FCG-1/2): registers respond at `$6000-$7FFF` (mask
//!   `$E00F`); writes to `$600B/$600C` modify the counter directly.
//! - Submapper 5 (LZ93D50): registers respond at `$8000-$FFFF` (mask
//!   `$800F`); `$800B/$800C` modify a *latch* copied to the counter on a
//!   `$800A` write; an EEPROM read appears in bit 4 of `$6000-$7FFF`.
//! - Submapper 0 (unspecified): respond in both ranges (the union).
//!
//! Mapper 159 is mapper 16 submapper 5 with a 128-byte X24C01 EEPROM
//! (instead of the 256-byte 24C02 on mapper 16).
//!
//! ## Offsets (relative to the window base, masked to `$x..F`)
//!
//! | Offset | Function                                              |
//! |--------|-------------------------------------------------------|
//! | `$0-7` | 1 KiB CHR bank N at PPU `$N*0x400`                     |
//! | `$8`   | 16 KiB PRG bank at `$8000` (low 4 bits)               |
//! | `$9`   | Mirroring (0 V, 1 H, 2 1scA, 3 1scB)                  |
//! | `$A`   | IRQ control: bit 0 enable; LZ93D50 also latch->counter |
//! | `$B`   | IRQ counter/latch low byte                            |
//! | `$C`   | IRQ counter/latch high byte                           |
//! | `$D`   | EEPROM control (LZ93D50): bit 5 SCL, bit 6 SDA, bit 7 dir |
//!
//! # EEPROM
//!
//! A minimal I²C state machine ([`Eeprom`]) for the X24C01 (159) / 24C02
//! (16) is implemented below. It models START/STOP edge detection, an 8-bit
//! shift register clocked on the SCL rising edge, the word-address/data
//! phases, and ACK pulses — enough for boot-smoke and a game's save-data
//! probe. It is **not** datasheet-timing-verified: there are no
//! redistributable behavioral test fixtures for these boards, so the EEPROM
//! is spec-implemented but verified only at the unit-test + boot-smoke
//! level (see the v2.1.0 coverage work notes).

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::struct_excessive_bools,
    clippy::missing_const_for_fn,
    clippy::doc_markdown,
    clippy::option_if_let_else
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_1K: usize = 0x0400;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// FCG board / EEPROM variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FcgVariant {
    /// Mapper 16 submapper 0: respond in both `$6000-$7FFF` and
    /// `$8000-$FFFF`; behaves as LZ93D50 (latched counter) with a 24C02.
    Both,
    /// Mapper 16 submapper 4: FCG-1/2, register window `$6000-$7FFF`,
    /// counter written directly, no EEPROM.
    Fcg,
    /// Mapper 16 submapper 5: LZ93D50, register window `$8000-$FFFF`,
    /// latched counter, optional 256-byte 24C02 EEPROM.
    Lz93d50_24c02,
    /// Mapper 159: LZ93D50 with a 128-byte X24C01 EEPROM.
    Lz93d50_24c01,
}

impl FcgVariant {
    const fn responds_low(self) -> bool {
        matches!(self, Self::Both | Self::Fcg)
    }
    const fn responds_high(self) -> bool {
        matches!(self, Self::Both | Self::Lz93d50_24c02 | Self::Lz93d50_24c01)
    }
    /// LZ93D50 latches the IRQ counter (`$x0B/$x0C` write a latch); FCG-1/2
    /// writes the counter directly.
    const fn latched_counter(self) -> bool {
        !matches!(self, Self::Fcg)
    }
    const fn eeprom_bytes(self) -> usize {
        match self {
            Self::Lz93d50_24c01 => 128,
            Self::Both | Self::Lz93d50_24c02 => 256,
            Self::Fcg => 0,
        }
    }
    /// X24C01 uses a combined 7-bit word-address byte; the 24C02 uses a
    /// device-select byte followed by a separate word-address byte.
    const fn is_x24c01(self) -> bool {
        matches!(self, Self::Lz93d50_24c01)
    }
}

/// Minimal serial I²C EEPROM (X24C01 / 24C02) state machine.
///
/// Models START/STOP detection on SDA edges while SCL is high, an 8-bit
/// MSB-first shift register clocked on the SCL rising edge, the
/// device-select / word-address / data phases, and ACK pulses. It is not a
/// cycle/timing-accurate datasheet model.
#[derive(Debug, Clone)]
struct Eeprom {
    mem: Box<[u8]>,
    is_x24c01: bool,

    last_scl: bool,
    last_sda: bool,

    state: I2cState,
    bit_count: u8,
    shift_in: u8,
    shift_out: u8,
    word_addr: u8,
    rw_read: bool,
    /// For the 24C02 write path: `true` while waiting for the word-address
    /// byte (the byte after the device-select), `false` once received.
    awaiting_word_addr: bool,
    /// The bit currently driven back onto SDA toward the CPU (high = 1).
    out_bit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum I2cState {
    Idle,
    AddrSelect,
    AckAddr,
    Read,
    AckRead,
    Write,
    AckWrite,
}

impl Eeprom {
    fn new(bytes: usize, is_x24c01: bool) -> Self {
        Self {
            mem: vec![0xFFu8; bytes.max(1)].into_boxed_slice(),
            is_x24c01,
            last_scl: false,
            last_sda: false,
            state: I2cState::Idle,
            bit_count: 0,
            shift_in: 0,
            shift_out: 0,
            word_addr: 0,
            rw_read: false,
            awaiting_word_addr: false,
            out_bit: true,
        }
    }

    fn mask(&self) -> usize {
        self.mem.len() - 1
    }

    /// Drive the EEPROM lines from the `$x00D` register. `dir_read` is the
    /// direction bit; when the host releases SDA to read, the device's
    /// `out_bit` is returned by [`Self::read_sda`].
    fn write_lines(&mut self, scl: bool, sda: bool) {
        let scl_rise = scl && !self.last_scl;
        let scl_high = scl && self.last_scl;

        if scl_high && self.last_sda != sda {
            if sda {
                // SDA rising while SCL high = STOP.
                self.state = I2cState::Idle;
            } else {
                // SDA falling while SCL high = START.
                self.state = I2cState::AddrSelect;
                self.bit_count = 0;
                self.shift_in = 0;
                self.awaiting_word_addr = false;
            }
            // Both START and STOP release SDA back to the host.
            self.out_bit = true;
        } else if scl_rise {
            self.clock_bit(sda);
        }

        self.last_scl = scl;
        self.last_sda = sda;
    }

    fn read_sda(&self) -> bool {
        self.out_bit
    }

    fn clock_bit(&mut self, sda: bool) {
        let mask = self.mask();
        match self.state {
            I2cState::Idle => {}
            I2cState::AddrSelect => {
                self.shift_in = (self.shift_in << 1) | u8::from(sda);
                self.bit_count += 1;
                if self.bit_count == 8 {
                    self.bit_count = 0;
                    self.rw_read = (self.shift_in & 1) != 0;
                    if self.is_x24c01 {
                        // Combined word-address byte (7-bit addr + R/W).
                        self.word_addr = (self.shift_in >> 1) & 0x7F;
                    } else if !self.rw_read {
                        // 24C02 write: next byte is the word address.
                        self.awaiting_word_addr = true;
                    }
                    self.state = I2cState::AckAddr;
                    self.out_bit = false; // ACK low
                }
            }
            I2cState::AckAddr => {
                if self.rw_read {
                    self.shift_out = self.mem[(self.word_addr as usize) & mask];
                    self.state = I2cState::Read;
                } else {
                    self.state = I2cState::Write;
                }
                self.bit_count = 0;
                self.shift_in = 0;
                self.out_bit = true;
            }
            I2cState::Read => {
                self.out_bit = (self.shift_out & 0x80) != 0;
                self.shift_out <<= 1;
                self.bit_count += 1;
                if self.bit_count == 8 {
                    self.bit_count = 0;
                    self.state = I2cState::AckRead;
                }
            }
            I2cState::AckRead => {
                self.word_addr = self.word_addr.wrapping_add(1);
                self.shift_out = self.mem[(self.word_addr as usize) & mask];
                self.state = I2cState::Read;
                self.bit_count = 0;
            }
            I2cState::Write => {
                self.shift_in = (self.shift_in << 1) | u8::from(sda);
                self.bit_count += 1;
                if self.bit_count == 8 {
                    self.bit_count = 0;
                    if self.awaiting_word_addr {
                        self.word_addr = self.shift_in;
                        self.awaiting_word_addr = false;
                    } else {
                        self.mem[(self.word_addr as usize) & mask] = self.shift_in;
                        self.word_addr = self.word_addr.wrapping_add(1);
                    }
                    self.state = I2cState::AckWrite;
                    self.out_bit = false; // ACK low
                }
            }
            I2cState::AckWrite => {
                self.state = I2cState::Write;
                self.bit_count = 0;
                self.shift_in = 0;
                self.out_bit = true;
            }
        }
    }
}

/// Bandai FCG mapper (iNES mappers 16 + 159).
pub struct BandaiFcg {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,

    variant: FcgVariant,

    prg_bank: u8,
    chr_banks: [u8; 8],
    mirroring: Mirroring,

    // 16-bit down-counting IRQ.
    irq_latch: u16,
    irq_counter: u16,
    irq_enabled: bool,
    irq_pending: bool,

    eeprom: Option<Eeprom>,
    // Last value written to the EEPROM control register (for save-state).
    eeprom_ctrl: u8,
}

impl BandaiFcg {
    /// Construct a new Bandai FCG mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 16 KiB; CHR-ROM must be a
    /// multiple of 1 KiB (CHR-RAM allocated as 8 KiB when empty).
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        variant: FcgVariant,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "Bandai-FCG PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; 8 * CHR_BANK_1K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Bandai-FCG CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        let eeprom = if variant.eeprom_bytes() > 0 {
            Some(Eeprom::new(variant.eeprom_bytes(), variant.is_x24c01()))
        } else {
            None
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            variant,
            prg_bank: 0,
            chr_banks: [0; 8],
            mirroring,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_pending: false,
            eeprom,
            eeprom_ctrl: 0,
        })
    }

    const fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.mirroring.physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let slot = (addr as usize / CHR_BANK_1K) & 0x07;
        let total = (self.chr.len() / CHR_BANK_1K).max(1);
        let bank = (self.chr_banks[slot] as usize) % total;
        bank * CHR_BANK_1K + (addr as usize & (CHR_BANK_1K - 1))
    }

    /// Apply a register write decoded to offset `$0-$F`.
    fn write_reg(&mut self, off: u8, value: u8) {
        match off & 0x0F {
            0x0..=0x7 => self.chr_banks[(off & 0x07) as usize] = value,
            0x8 => self.prg_bank = value & 0x0F,
            0x9 => {
                self.mirroring = match value & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SingleScreenA,
                    _ => Mirroring::SingleScreenB,
                };
            }
            0xA => {
                // IRQ control. Bit 0 = enable. Writing acknowledges.
                self.irq_pending = false;
                self.irq_enabled = (value & 0x01) != 0;
                if self.variant.latched_counter() {
                    // LZ93D50: copy latch to counter.
                    self.irq_counter = self.irq_latch;
                }
            }
            0xB => {
                if self.variant.latched_counter() {
                    self.irq_latch = (self.irq_latch & 0xFF00) | value as u16;
                } else {
                    self.irq_counter = (self.irq_counter & 0xFF00) | value as u16;
                }
            }
            0xC => {
                if self.variant.latched_counter() {
                    self.irq_latch = (self.irq_latch & 0x00FF) | ((value as u16) << 8);
                } else {
                    self.irq_counter = (self.irq_counter & 0x00FF) | ((value as u16) << 8);
                }
            }
            0xD => {
                self.eeprom_ctrl = value;
                if let Some(ee) = self.eeprom.as_mut() {
                    // Bit 5 = SCL, bit 6 = SDA (host-driven), bit 7 = dir.
                    let scl = (value & 0x20) != 0;
                    let sda = (value & 0x40) != 0;
                    ee.write_lines(scl, sda);
                }
            }
            _ => {}
        }
    }
}

impl Mapper for BandaiFcg {
    // v2.8.0 Phase 4 — CPU-cycle hook + IRQ source; no on-cart audio.
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // EEPROM read appears in bit 4 (LZ93D50). Otherwise open bus
                // (the bus's open-bus latch handles unmapped reads, but the
                // FCG drives bit 4 here).
                if let Some(ee) = self.eeprom.as_ref() {
                    let bit = u8::from(ee.read_sda());
                    return bit << 4;
                }
                0
            }
            0x8000..=0xBFFF => {
                let total = (self.prg_rom.len() / PRG_BANK_16K).max(1);
                let bank = (self.prg_bank as usize) % total;
                self.prg_rom[bank * PRG_BANK_16K + (addr - 0x8000) as usize]
            }
            0xC000..=0xFFFF => {
                let total = (self.prg_rom.len() / PRG_BANK_16K).max(1);
                let last = total - 1;
                self.prg_rom[last * PRG_BANK_16K + (addr - 0xC000) as usize]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if self.variant.responds_low() && (0x6000..=0x7FFF).contains(&addr) {
            self.write_reg((addr & 0x0F) as u8, value);
        }
        if self.variant.responds_high() && (0x8000..=0xFFFF).contains(&addr) {
            self.write_reg((addr & 0x0F) as u8, value);
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // The FCG drives bit 4 of $6000-$7FFF (EEPROM) when an EEPROM is
        // present, so that window is mapped; otherwise default behavior.
        if self.eeprom.is_some() && (0x6000..=0x7FFF).contains(&addr) {
            return false;
        }
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
                self.chr[off % self.chr.len()]
            }
            0x2000..=0x3EFF => self.vram[self.nametable_offset(addr)],
            _ => 0,
        }
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
                let off = self.nametable_offset(addr);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn notify_cpu_cycle(&mut self) {
        if !self.irq_enabled {
            return;
        }
        // Down-counter: IRQ asserts when the counter holds zero, then it
        // wraps to $FFFF and keeps counting (per the wiki: "When it holds a
        // value of zero, an IRQ is generated").
        if self.irq_counter == 0 {
            self.irq_pending = true;
        }
        self.irq_counter = self.irq_counter.wrapping_sub(1);
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let id = if matches!(self.variant, FcgVariant::Lz93d50_24c01) {
            159
        } else {
            16
        };
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: id,
            name: format!("Bandai FCG ({id})"),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.prg_banks
            .push(("PRG".into(), format!("{:#04x}", self.prg_bank)));
        for (i, b) in self.chr_banks.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR{i}"), format!("{b:#04x}")));
        }
        info.irq_state
            .push(("latch".into(), format!("{:#06x}", self.irq_latch)));
        info.irq_state
            .push(("counter".into(), format!("{:#06x}", self.irq_counter)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending)));
        info.extra.push((
            "eeprom".into(),
            match self.eeprom.as_ref() {
                Some(ee) => format!("{} bytes", ee.mem.len()),
                None => "none".into(),
            },
        ));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let ee_len = self.eeprom.as_ref().map_or(0, |e| e.mem.len());
        let mut out = Vec::with_capacity(
            18 + self.vram.len() + ee_len + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.extend_from_slice(&self.chr_banks);
        out.push(self.mirroring as u8);
        out.extend_from_slice(&self.irq_latch.to_le_bytes());
        out.extend_from_slice(&self.irq_counter.to_le_bytes());
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_pending));
        out.push(self.eeprom_ctrl);
        // EEPROM contents (if any).
        if let Some(ee) = self.eeprom.as_ref() {
            out.extend_from_slice(&ee.mem);
        }
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let ee_len = self.eeprom.as_ref().map_or(0, |e| e.mem.len());
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 18 + self.vram.len() + ee_len + need_chr;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank = data[1];
        self.chr_banks.copy_from_slice(&data[2..10]);
        self.mirroring = match data[10] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.irq_latch = u16::from_le_bytes([data[11], data[12]]);
        self.irq_counter = u16::from_le_bytes([data[13], data[14]]);
        self.irq_enabled = data[15] != 0;
        // Bytes 16+ : pending, eeprom_ctrl, eeprom mem, vram, chr.
        // We packed pending + ctrl after the fixed block; recompute cursor.
        // To keep the layout simple, re-derive: indices 16 = pending,
        // 17 = ctrl. (with_capacity sizing already accounts for the +16
        // header containing version+prg+8 chr+mir+2 latch+2 counter+enabled.)
        let mut cursor = 16;
        let pending = data[cursor];
        cursor += 1;
        let ctrl = data[cursor];
        cursor += 1;
        self.irq_pending = pending != 0;
        self.eeprom_ctrl = ctrl;
        if let Some(ee) = self.eeprom.as_mut() {
            ee.mem.copy_from_slice(&data[cursor..cursor + ee.mem.len()]);
            cursor += ee.mem.len();
        }
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr
                .copy_from_slice(&data[cursor..cursor + self.chr.len()]);
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg(banks_16k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_16k * PRG_BANK_16K];
        for b in 0..banks_16k {
            v[b * PRG_BANK_16K] = b as u8;
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
    fn lz93d50_prg_bank_and_fixed_last() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Lz93d50_24c02,
        )
        .unwrap();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 7);
        m.cpu_write(0x8008, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 7);
    }

    #[test]
    fn chr_bank_select_per_1k_slot() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Lz93d50_24c02,
        )
        .unwrap();
        m.cpu_write(0x8000, 4); // CHR slot 0 -> bank 4
        m.cpu_write(0x8004, 9); // CHR slot 4 ($1000) -> bank 9
        assert_eq!(m.ppu_read(0x0000), 4);
        assert_eq!(m.ppu_read(0x1000), 9);
    }

    #[test]
    fn mirroring_control() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Lz93d50_24c02,
        )
        .unwrap();
        m.cpu_write(0x8009, 1);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0x8009, 2);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
    }

    #[test]
    fn lz93d50_latched_irq_counts_down_to_zero() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Lz93d50_24c02,
        )
        .unwrap();
        // Latch = 3.
        m.cpu_write(0x800B, 0x03);
        m.cpu_write(0x800C, 0x00);
        // Enable + copy latch->counter.
        m.cpu_write(0x800A, 0x01);
        assert_eq!(m.irq_counter, 3);
        m.notify_cpu_cycle(); // 3 -> 2
        m.notify_cpu_cycle(); // 2 -> 1
        m.notify_cpu_cycle(); // 1 -> 0
        assert!(!m.irq_pending());
        m.notify_cpu_cycle(); // counter holds 0 -> IRQ
        assert!(m.irq_pending());
    }

    #[test]
    fn fcg_writes_counter_directly() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Fcg,
        )
        .unwrap();
        // FCG-1/2 responds in $6000-$7FFF and writes the counter directly.
        m.cpu_write(0x600B, 0x02);
        m.cpu_write(0x600C, 0x00);
        assert_eq!(m.irq_counter, 2);
        m.cpu_write(0x600A, 0x01); // enable (no latch copy)
        assert_eq!(m.irq_counter, 2);
    }

    #[test]
    fn irq_acknowledge_on_control_write() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Lz93d50_24c02,
        )
        .unwrap();
        m.irq_pending = true;
        m.cpu_write(0x800A, 0x00); // disable + ack
        assert!(!m.irq_pending());
        assert!(!m.irq_enabled);
    }

    #[test]
    fn eeprom_present_for_159_absent_for_fcg() {
        let m159 = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Lz93d50_24c01,
        )
        .unwrap();
        assert!(m159.eeprom.is_some());
        assert_eq!(m159.eeprom.as_ref().unwrap().mem.len(), 128);
        let mfcg = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Fcg,
        )
        .unwrap();
        assert!(mfcg.eeprom.is_none());
    }

    #[test]
    fn eeprom_idle_reads_high() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Vertical,
            FcgVariant::Lz93d50_24c01,
        )
        .unwrap();
        // With no I2C transaction, the device releases SDA (out_bit high) ->
        // bit 4 set.
        assert_eq!(m.cpu_read(0x6000) & 0x10, 0x10);
    }

    #[test]
    fn save_state_round_trip_with_eeprom() {
        let mut m = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Horizontal,
            FcgVariant::Lz93d50_24c02,
        )
        .unwrap();
        m.cpu_write(0x8008, 2);
        m.cpu_write(0x8000, 5);
        m.cpu_write(0x800B, 0x10);
        m.cpu_write(0x800A, 0x01);
        if let Some(ee) = m.eeprom.as_mut() {
            ee.mem[0] = 0x42;
        }
        let blob = m.save_state();
        let mut m2 = BandaiFcg::new(
            synth_prg(8),
            synth_chr(16),
            Mirroring::Horizontal,
            FcgVariant::Lz93d50_24c02,
        )
        .unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.ppu_read(0x0000), m2.ppu_read(0x0000));
        assert_eq!(m.irq_counter, m2.irq_counter);
        assert_eq!(m2.eeprom.as_ref().unwrap().mem[0], 0x42);
    }
}

//! Sprint 6 simple discrete / multicart mappers (v1.2.0 "Curator" workstream A).
//!
//! A second batch of small, well-documented multicart / discrete boards in the
//! shape of the stock discrete mappers and Sprint 5: a handful of bank-select
//! latch registers, mostly no IRQ and no on-cart audio. Banking / mirroring
//! semantics are cross-checked against the `GeraNES` reference
//! (`ref-proj/GeraNES/src/GeraNES/Mappers/Mapper0NN.h`) and the nesdev wiki.
//!
//! These are Tier-2 best-effort boards: register-decode correctness only (no
//! oracle ROM). Boards implemented here:
//!
//! - **Mapper 15** (K-1029 / 100-in-1 Contra Function 16 multicart): four PRG
//!   banking modes decoded from the low two address bits + mirroring bit.
//! - **Mapper 36** (TXC 01-22000 / Policeman): `$4100-$5FFF` (A8) register,
//!   PRG high nibble + CHR low nibble.
//! - **Mapper 39** (Subor `BNROM`-like): full-byte 32 KiB PRG select, fixed CHR.
//! - **Mapper 61** (multicart): address-decoded PRG page + 16/32 KiB mode +
//!   mirroring bit.
//! - **Mapper 62** (multicart): address+data-decoded PRG/CHR + mode + mirroring.
//! - **Mapper 72** (Jaleco `JF-17`/`JF-19`): rising-edge PRG/CHR strobe latch,
//!   with bus conflicts; upper PRG bank fixed to the last 16 KiB bank.
//! - **Mapper 77** (Irem, Napoleon Senki): 32 KiB PRG + 2 KiB CHR-ROM at
//!   `$0000`; the rest of CHR space (`$0800-$1FFF`) and the nametables are
//!   on-cart RAM (four-screen-style).
//! - **Mapper 92** (Jaleco `JF-19`-variant): like 72 but with a 5-bit PRG field.
//! - **Mapper 96** (Bandai Oeka Kids): CHR latch derived from the PPU address
//!   bus during nametable fetches; 4 KiB CHR banking.
//! - **Mapper 97** (Irem `TAM-S1`, Kaiketsu Yanchamaru): fixed last 16 KiB PRG
//!   bank at `$8000`, switchable bank at `$C000`, mirroring bit.
//! - **Mapper 132** (TXC 22211): the TXC scrambling-accumulator chip (non-JV001
//!   variant) driving a 1-bit PRG + 2-bit CHR select.
//! - **Mapper 133** (Sachen 3009): A8-decode register, 1-bit PRG + 2-bit CHR.
//! - **Mapper 145** (Sachen `SA-72007`): CHR bank from data bit 7, decoded in
//!   both the `$4100` register window and the `$6000` save-RAM window.
//! - **Mapper 146** (Sachen, `NINA-03`/mapper-79-equivalent behaviour): A8
//!   decode, PRG bit 3 + 3-bit CHR.

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_8K: usize = 0x2000;
const CHR_BANK_4K: usize = 0x1000;
const CHR_BANK_2K: usize = 0x0800;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// Shared nametable helper (mirrors the one in the other simple-mapper modules).
// ---------------------------------------------------------------------------

const fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

// ===========================================================================
// Mapper 15 — K-1029 / 100-in-1 Contra Function 16.
//
// Single register decoded across $8000-$FFFF (data + low two address bits):
//   addr bits 0-1 select the banking MODE; data holds the PRG bank, a CHR-RAM
//   mirroring bit (bit 6) and a "half-bank" bit (bit 7).
//     mode 0: 32 KiB at the 16 KiB granularity, second half = bank|1
//     mode 1: 128 KiB? upper half forced to bank|7 (UNROM-like fixed top)
//     mode 2: 8 KiB-granular ((bank<<1)|b) mirrored across the whole window
//     mode 3: single 16 KiB bank mirrored across the whole window
//   CHR is always 8 KiB RAM; CHR writes are protected in modes 0 and 3.
//   mirroring: data bit 6 (1 = horizontal, 0 = vertical). No IRQ.
// ===========================================================================

/// Mapper 15 (K-1029 / 100-in-1 Contra Function 16 multicart).
pub struct Multicart15 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    mode: u8,
    prg_bank: u8,
    half: u8,
    horizontal_mirroring: bool,
}

impl Multicart15 {
    /// Construct a new mapper 15 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB. CHR is always 8 KiB RAM (any supplied CHR-ROM is rejected).
    pub fn new(prg_rom: Box<[u8]>, chr_rom: &[u8]) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 15 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if !chr_rom.is_empty() {
            return Err(MapperError::Invalid(format!(
                "mapper 15 uses 8 KiB CHR-RAM; got {} bytes of CHR-ROM",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mode: 0,
            prg_bank: 0,
            half: 0,
            horizontal_mirroring: false,
        })
    }

    fn prg_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn read_16k(&self, bank: usize, off: usize) -> u8 {
        let bank = bank % self.prg_count_16k();
        self.prg_rom[bank * PRG_BANK_16K + off]
    }

    fn read_8k(&self, bank: usize, off: usize) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + off]
    }
}

impl Mapper for Multicart15 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return 0;
        }
        let win = (addr as usize) - 0x8000; // 0..0x8000 (the visible 32 KiB)
        let bank = self.prg_bank as usize;
        match self.mode {
            0 => {
                if win < PRG_BANK_16K {
                    self.read_16k(bank, win)
                } else {
                    self.read_16k(bank | 1, win - PRG_BANK_16K)
                }
            }
            1 => {
                if win < PRG_BANK_16K {
                    self.read_16k(bank, win)
                } else {
                    self.read_16k(bank | 7, win - PRG_BANK_16K)
                }
            }
            2 => {
                // 8 KiB-granular, mirrored across the whole window.
                let off = win & (PRG_BANK_8K - 1);
                self.read_8k((bank << 1) | (self.half as usize), off)
            }
            // mode 3: a single 16 KiB bank mirrored across the window.
            _ => {
                let off = win & (PRG_BANK_16K - 1);
                self.read_16k(bank, off)
            }
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.mode = (addr & 0x03) as u8;
            self.horizontal_mirroring = (value & 0x40) != 0;
            self.prg_bank = value & 0x3F;
            self.half = u8::from((value & 0x80) != 0);
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                // CHR-RAM write-protected in modes 0 and 3.
                if self.mode != 0 && self.mode != 3 {
                    self.chr_ram[addr as usize] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(5 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.mode);
        out.push(self.prg_bank);
        out.push(self.half);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 5 + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.mode = data[1];
        self.prg_bank = data[2];
        self.half = data[3];
        self.horizontal_mirroring = data[4] != 0;
        let mut cursor = 5;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 36 — TXC 01-22000 (Policeman).
//
// Single register decoded across $4100-$5FFF on A8 (any in-window address with
// bit 8 set): byte PPPP_CCCC selects PRG (high nibble, 32 KiB) and CHR (low
// nibble, 8 KiB). Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 36 (TXC 01-22000 / Policeman).
pub struct Txc36 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Txc36 {
    /// Construct a new mapper 36 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 36 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 36 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }
}

impl Mapper for Txc36 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // Register window $4100-$5FFF is write-only; reads there fall through to
    // open bus, so the default `cpu_read_unmapped` is correct.

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x4100..=0x5FFF).contains(&addr) && (addr & 0x0100) != 0 {
            self.prg_bank = (value >> 4) & 0x0F;
            self.chr_bank = value & 0x0F;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.mirroring);
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.vram.len();
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
        self.chr_bank = data[2];
        self.vram.copy_from_slice(&data[3..3 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 39 — Subor BNROM-like.
//
// A single 32 KiB PRG bank selected by the whole byte written anywhere in
// $8000-$FFFF (no bus conflict; the register simply latches the byte, masked to
// the available bank count). CHR is fixed: bank 0 of CHR-ROM, or 8 KiB CHR-RAM.
// Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 39 (Subor `BNROM`-like, 32 KiB PRG).
pub struct Subor39 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    mirroring: Mirroring,
}

impl Subor39 {
    /// Construct a new mapper 39 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 39 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 39 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            mirroring,
        })
    }
}

impl Mapper for Subor39 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = value;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            // CHR is fixed to bank 0 (8 KiB).
            0x0000..=0x1FFF => self.chr[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    self.chr[addr as usize] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(2 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 2 + self.vram.len() + chr_extra;
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
        let mut cursor = 2;
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

// ===========================================================================
// Mapper 61 — 0x80-style multicart.
//
// The register is decoded entirely from the absolute CPU address ($8000-$FFFF);
// data is ignored. With `A = addr`:
//   prg_page          = ((A & 0x0F) << 1) | ((A >> 5) & 0x01)
//   prg_16k_mode      =  (A & 0x10) != 0
//   horizontal_mirror =  (A & 0x80) != 0
// In 16 KiB mode the 16 KiB bank `prg_page` is mirrored across the window; in
// 32 KiB mode the 32 KiB bank `prg_page >> 1` is used. CHR is 8 KiB RAM (fixed).
// No IRQ.
// ===========================================================================

/// Mapper 61 (0x80-style multicart).
pub struct Multicart61 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_page: u8,
    prg_16k_mode: bool,
    horizontal_mirroring: bool,
}

impl Multicart61 {
    /// Construct a new mapper 61 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB. CHR is always 8 KiB RAM (any supplied CHR-ROM is rejected).
    pub fn new(prg_rom: Box<[u8]>, chr_rom: &[u8]) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 61 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if !chr_rom.is_empty() {
            return Err(MapperError::Invalid(format!(
                "mapper 61 uses 8 KiB CHR-RAM; got {} bytes of CHR-ROM",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_page: 0,
            prg_16k_mode: false,
            horizontal_mirroring: false,
        })
    }
}

impl Mapper for Multicart61 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return 0;
        }
        let win = (addr as usize) - 0x8000;
        if self.prg_16k_mode {
            let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
            let bank = (self.prg_page as usize) % count;
            let off = win & (PRG_BANK_16K - 1);
            self.prg_rom[bank * PRG_BANK_16K + off]
        } else {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = ((self.prg_page >> 1) as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + win]
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            let lo = (addr & 0x0F) as u8;
            let hi = ((addr >> 5) & 0x01) as u8;
            self.prg_page = (lo << 1) | hi;
            self.prg_16k_mode = (addr & 0x10) != 0;
            self.horizontal_mirroring = (addr & 0x80) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize] = value,
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_page);
        out.push(u8::from(self.prg_16k_mode));
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 4 + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_page = data[1];
        self.prg_16k_mode = data[2] != 0;
        self.horizontal_mirroring = data[3] != 0;
        let mut cursor = 4;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 62 — multicart.
//
// Both the CPU address and the written byte feed the register ($8000-$FFFF).
// With `A = addr` and `D = data`:
//   prg_page          = ((A & 0x3F00) >> 8) | (A & 0x40)
//   chr_bank (4-bit?) = ((A & 0x1F) << 2) | (D & 0x03)
//   prg_16k_mode      =  (A & 0x20) != 0
//   horizontal_mirror =  (A & 0x80) != 0
// In 16 KiB mode the 16 KiB bank `prg_page` is mirrored across the window; in
// 32 KiB mode bank `prg_page >> 1` is used. CHR is 8 KiB ROM banked. No IRQ.
// ===========================================================================

/// Mapper 62 (multicart).
pub struct Multicart62 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_page: u8,
    chr_bank: u8,
    prg_16k_mode: bool,
    horizontal_mirroring: bool,
}

impl Multicart62 {
    /// Construct a new mapper 62 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 62 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 62 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_page: 0,
            chr_bank: 0,
            // Seed from the header arrangement so the power-on default is sane.
            prg_16k_mode: false,
            horizontal_mirroring: mirroring == Mirroring::Horizontal,
        })
    }
}

impl Mapper for Multicart62 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return 0;
        }
        let win = (addr as usize) - 0x8000;
        if self.prg_16k_mode {
            let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
            let bank = (self.prg_page as usize) % count;
            let off = win & (PRG_BANK_16K - 1);
            self.prg_rom[bank * PRG_BANK_16K + off]
        } else {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = ((self.prg_page >> 1) as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + win]
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            let page_lo = ((addr & 0x3F00) >> 8) as u8;
            let page_hi = (addr & 0x40) as u8;
            self.prg_page = page_lo | page_hi;
            let chr_hi = ((addr & 0x1F) as u8) << 2;
            self.chr_bank = chr_hi | (value & 0x03);
            self.prg_16k_mode = (addr & 0x20) != 0;
            self.horizontal_mirroring = (addr & 0x80) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.current_mirroring());
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(5 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_page);
        out.push(self.chr_bank);
        out.push(u8::from(self.prg_16k_mode));
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 5 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_page = data[1];
        self.chr_bank = data[2];
        self.prg_16k_mode = data[3] != 0;
        self.horizontal_mirroring = data[4] != 0;
        self.vram.copy_from_slice(&data[5..5 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 72 — Jaleco JF-17 / JF-19.
//
// A write to $8000-$FFFF (with bus conflicts) carries two strobe bits:
//   bit 7 = PRG latch strobe, bit 6 = CHR latch strobe.
// On the RISING edge of each strobe the corresponding low-nibble bank field is
// latched: PRG = data & 0x0F (16 KiB), CHR = data & 0x0F (8 KiB). The lower
// 16 KiB PRG window ($8000-$BFFF) reads the latched PRG bank; the upper window
// ($C000-$FFFF) is fixed to the last 16 KiB bank. Mirroring header-fixed; no IRQ.
//
// Mapper 92 reuses this logic with a 5-bit PRG field (see `Jaleco92`).
// ===========================================================================

/// Shared register/strobe state for the Jaleco JF-17/19 family (mappers 72/92).
// The four flags each model a distinct hardware signal (CHR-RAM presence, the
// two edge-triggered latch strobes, and the JF-17-vs-JF-19 PRG layout); they
// are not a bitfield-able state and reading them as named bools is clearest.
#[allow(clippy::struct_excessive_bools)]
struct JalecoLatch {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    prev_prg_strobe: bool,
    prev_chr_strobe: bool,
    mirroring: Mirroring,
    /// Mask applied to the PRG-bank nibble (0x0F for 72, 0x1F for 92).
    prg_field_mask: u8,
    /// PRG window layout. `false` (mapper 72, JF-17): switchable bank at
    /// `$8000-$BFFF`, fixed LAST bank at `$C000-$FFFF`. `true` (mapper 92,
    /// JF-19): fixed FIRST bank at `$8000-$BFFF`, switchable bank at
    /// `$C000-$FFFF` — the reset vector lives in the fixed half, so this layout
    /// is load-bearing for boot.
    switchable_high: bool,
}

impl JalecoLatch {
    fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        prg_field_mask: u8,
        switchable_high: bool,
        id: u16,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper {id} PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper {id} CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            prev_prg_strobe: false,
            prev_chr_strobe: false,
            mirroring,
            prg_field_mask,
            switchable_high,
        })
    }

    fn prg_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn read_prg_bank(&self, bank: usize, off: usize) -> u8 {
        let bank = bank % self.prg_count_16k();
        self.prg_rom[bank * PRG_BANK_16K + off]
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }

    fn cpu_read(&self, addr: u16) -> u8 {
        let last = self.prg_count_16k() - 1;
        match addr {
            0x8000..=0xBFFF => {
                // JF-19 (mapper 92): fixed FIRST bank here. JF-17 (mapper 72):
                // switchable bank here.
                let bank = if self.switchable_high {
                    0
                } else {
                    self.prg_bank as usize
                };
                self.read_prg_bank(bank, addr as usize - 0x8000)
            }
            0xC000..=0xFFFF => {
                // JF-19: switchable bank here. JF-17: fixed LAST bank here.
                let bank = if self.switchable_high {
                    self.prg_bank as usize
                } else {
                    last
                };
                self.read_prg_bank(bank, addr as usize - 0xC000)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bus conflict: AND the written byte with the underlying PRG byte.
            let effective = value & self.cpu_read(addr);
            let prg_strobe = (effective & 0x80) != 0;
            let chr_strobe = (effective & 0x40) != 0;
            if prg_strobe && !self.prev_prg_strobe {
                self.prg_bank = effective & self.prg_field_mask;
            }
            if chr_strobe && !self.prev_chr_strobe {
                self.chr_bank = effective & 0x0F;
            }
            self.prev_prg_strobe = prg_strobe;
            self.prev_chr_strobe = chr_strobe;
        }
    }

    fn ppu_read(&self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr[off] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(5 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.push(u8::from(self.prev_prg_strobe));
        out.push(u8::from(self.prev_chr_strobe));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 5 + self.vram.len() + chr_extra;
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
        self.chr_bank = data[2];
        self.prev_prg_strobe = data[3] != 0;
        self.prev_chr_strobe = data[4] != 0;
        let mut cursor = 5;
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

/// Mapper 72 (Jaleco `JF-17`/`JF-19`).
pub struct Jaleco72 {
    inner: JalecoLatch,
}

impl Jaleco72 {
    /// Construct a new mapper 72 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        Ok(Self {
            inner: JalecoLatch::new(prg_rom, chr_rom, mirroring, 0x0F, false, 72)?,
        })
    }
}

impl Mapper for Jaleco72 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.inner.cpu_read(addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        self.inner.cpu_write(addr, value);
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        self.inner.ppu_read(addr)
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        self.inner.ppu_write(addr, value);
    }

    fn current_mirroring(&self) -> Mirroring {
        self.inner.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        self.inner.save_state()
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        self.inner.load_state(data)
    }
}

/// Mapper 92 (Jaleco `JF-19`-variant — like 72 with a 5-bit PRG field).
pub struct Jaleco92 {
    inner: JalecoLatch,
}

impl Jaleco92 {
    /// Construct a new mapper 92 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        Ok(Self {
            inner: JalecoLatch::new(prg_rom, chr_rom, mirroring, 0x1F, true, 92)?,
        })
    }
}

impl Mapper for Jaleco92 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.inner.cpu_read(addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        self.inner.cpu_write(addr, value);
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        self.inner.ppu_read(addr)
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        self.inner.ppu_write(addr, value);
    }

    fn current_mirroring(&self) -> Mirroring {
        self.inner.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        self.inner.save_state()
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        self.inner.load_state(data)
    }
}

// ===========================================================================
// Mapper 77 — Irem (Napoleon Senki).
//
// A write to $8000-$FFFF (with bus conflicts) holds [CCCC PPPP]:
//   PPPP = 32 KiB PRG bank, CCCC = 2 KiB CHR-ROM bank at $0000-$07FF.
// The CHR region $0800-$1FFF and the nametables are backed by on-cart RAM
// (the board exposes 4-screen-style VRAM). To keep this in the PPU-side hooks
// we model a contiguous 10 KiB RAM ($0800-$2FFF logically) and route the four
// nametables (indices 0..=3) into the upper 4 KiB of that RAM via the
// `nametable_fetch`/`nametable_write` hooks. No IRQ.
// ===========================================================================

/// Mapper 77 (Irem, Napoleon Senki).
pub struct Irem77 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    /// CHR RAM for $0800-$1FFF (6 KiB).
    chr_ram: Box<[u8]>,
    /// 4 KiB on-cart nametable RAM (four 1 KiB screens).
    nt_ram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
}

impl Irem77 {
    /// Construct a new mapper 77 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB or CHR-ROM is empty / not a multiple of 2 KiB.
    pub fn new(prg_rom: Box<[u8]>, chr_rom: Box<[u8]>) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 77 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_2K) {
            return Err(MapperError::Invalid(format!(
                "mapper 77 CHR-ROM size {} is not a non-zero multiple of 2 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            chr_ram: vec![0u8; 0x1800].into_boxed_slice(), // $0800-$1FFF = 6 KiB
            nt_ram: vec![0u8; 4 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_bank: 0,
        })
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
        let bank = (self.prg_bank as usize) % count;
        self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
    }

    /// Map a $2000-$3EFF nametable address to a 4 KiB on-cart RAM offset.
    const fn nt_offset(addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as usize;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        table * NAMETABLE_SIZE + local
    }
}

impl Mapper for Irem77 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.read_prg(addr)
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bus conflict: AND with the underlying PRG byte.
            let effective = value & self.read_prg(addr);
            self.prg_bank = effective & 0x0F;
            self.chr_bank = (effective >> 4) & 0x0F;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x07FF => {
                // Bottom 2 KiB: switchable CHR-ROM bank.
                let count = (self.chr_rom.len() / CHR_BANK_2K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_2K + addr as usize]
            }
            0x0800..=0x1FFF => self.chr_ram[addr as usize - 0x0800],
            0x2000..=0x3EFF => self.nt_ram[Self::nt_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        // $0000-$07FF is CHR-ROM (read-only); everything else is RAM.
        match addr {
            0x0800..=0x1FFF => self.chr_ram[addr as usize - 0x0800] = value,
            0x2000..=0x3EFF => self.nt_ram[Self::nt_offset(addr)] = value,
            _ => {}
        }
    }

    fn nametable_fetch(&mut self, addr: u16) -> Option<u8> {
        // Consume the nametable read from on-cart 4-screen RAM.
        Some(self.nt_ram[Self::nt_offset(addr)])
    }

    fn nametable_write(&mut self, addr: u16, value: u8) -> bool {
        self.nt_ram[Self::nt_offset(addr)] = value;
        true
    }

    fn current_mirroring(&self) -> Mirroring {
        Mirroring::FourScreen
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.chr_ram.len() + self.nt_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.chr_ram);
        out.extend_from_slice(&self.nt_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.chr_ram.len() + self.nt_ram.len();
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
        self.chr_bank = data[2];
        let mut cursor = 3;
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        cursor += self.chr_ram.len();
        self.nt_ram
            .copy_from_slice(&data[cursor..cursor + self.nt_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 96 — Bandai Oeka Kids.
//
// A write to $8000-$FFFF sets the 32 KiB PRG bank (bits 0-1) and the CHR outer
// bank (bit 2). The CHR INNER 4 KiB bank for the $0000 slot is selected by
// sniffing the PPU address bus: on the rising edge into a nametable fetch
// (`$2xxx`), bits 9-8 of the address become the inner bank. CHR uses 4 KiB
// banking; the $1000 slot is always (outer | 0x03). CHR is ROM (or RAM dumps).
// Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 96 (Bandai Oeka Kids).
pub struct Bandai96 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    outer_chr: u8,
    inner_chr: u8,
    last_ppu_addr: u16,
    mirroring: Mirroring,
}

impl Bandai96 {
    /// Construct a new mapper 96 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB, or CHR-ROM (when present) is not a multiple of 4 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 96 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            // Two 4 KiB CHR-RAM banks (the Oeka Kids drawing buffer).
            vec![0u8; 2 * CHR_BANK_4K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_4K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 96 CHR-ROM size {} is not a multiple of 4 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            outer_chr: 0,
            inner_chr: 0,
            last_ppu_addr: 0,
            mirroring,
        })
    }

    fn chr_count_4k(&self) -> usize {
        (self.chr.len() / CHR_BANK_4K).max(1)
    }

    fn chr_offset(&self, addr: u16) -> usize {
        // $0000 slot uses outer|inner; $1000 slot uses outer|0x03.
        let slot = (addr >> 12) & 0x01;
        let bank = if slot == 0 {
            self.outer_chr | self.inner_chr
        } else {
            self.outer_chr | 0x03
        };
        let bank = (bank as usize) % self.chr_count_4k();
        bank * CHR_BANK_4K + (addr as usize & (CHR_BANK_4K - 1))
    }
}

impl Mapper for Bandai96 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = value & 0x03;
            self.outer_chr = value & 0x04;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let masked = addr & 0x3FFF;
        // Sniff the PPU address bus: rising edge into a nametable fetch latches
        // the CHR inner bank from address bits 9-8.
        if (self.last_ppu_addr & 0x3000) != 0x2000 && (masked & 0x3000) == 0x2000 {
            self.inner_chr = ((masked >> 8) & 0x03) as u8;
        }
        self.last_ppu_addr = masked;
        match masked {
            0x0000..=0x1FFF => self.chr[self.chr_offset(masked)],
            0x2000..=0x3EFF => self.vram[nametable_offset(masked, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let masked = addr & 0x3FFF;
        if (self.last_ppu_addr & 0x3000) != 0x2000 && (masked & 0x3000) == 0x2000 {
            self.inner_chr = ((masked >> 8) & 0x03) as u8;
        }
        self.last_ppu_addr = masked;
        match masked {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(masked);
                    self.chr[off] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(masked, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(6 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.outer_chr);
        out.push(self.inner_chr);
        out.push((self.last_ppu_addr & 0xFF) as u8);
        out.push((self.last_ppu_addr >> 8) as u8);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 6 + self.vram.len() + chr_extra;
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
        self.outer_chr = data[2];
        self.inner_chr = data[3];
        self.last_ppu_addr = u16::from(data[4]) | (u16::from(data[5]) << 8);
        let mut cursor = 6;
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

// ===========================================================================
// Mapper 97 — Irem TAM-S1 (Kaiketsu Yanchamaru).
//
// A write to $8000-$FFFF holds [Mxxx xxPP_PPP]: bits 0-4 = switchable 16 KiB
// PRG bank, bit 7 = mirroring (1 = vertical, 0 = horizontal). The PRG layout is
// REVERSED relative to UNROM: $8000-$BFFF is FIXED to the LAST 16 KiB bank, and
// $C000-$FFFF is the SWITCHABLE bank. CHR is 8 KiB (ROM or RAM). No IRQ.
// ===========================================================================

/// Mapper 97 (Irem `TAM-S1`, Kaiketsu Yanchamaru).
pub struct Irem97 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    vertical_mirroring: bool,
}

impl Irem97 {
    /// Construct a new mapper 97 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 97 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 97 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            vertical_mirroring: mirroring == Mirroring::Vertical,
        })
    }

    fn prg_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }
}

impl Mapper for Irem97 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // $8000-$BFFF fixed to the last 16 KiB bank.
            0x8000..=0xBFFF => {
                let last = self.prg_count_16k() - 1;
                self.prg_rom[last * PRG_BANK_16K + (addr as usize - 0x8000)]
            }
            // $C000-$FFFF switchable.
            0xC000..=0xFFFF => {
                let bank = (self.prg_bank as usize) % self.prg_count_16k();
                self.prg_rom[bank * PRG_BANK_16K + (addr as usize - 0xC000)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = value & 0x1F;
            self.vertical_mirroring = (value & 0x80) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    self.chr[addr as usize] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.vertical_mirroring {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(3 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.vertical_mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 3 + self.vram.len() + chr_extra;
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
        self.vertical_mirroring = data[2] != 0;
        let mut cursor = 3;
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

// ===========================================================================
// Mapper 132 — TXC 22211.
//
// Driven by the TXC scrambling-accumulator chip (the non-JV001 variant). The
// chip has four internal registers written via $4100-$4103 (decoded on
// addr & 0xE103) and an output latch updated on any $8000-$FFFF write:
//   output = (accumulator & 0x0F) | ((inverter & 0x08) << 1)
// The mapper then resolves:
//   PRG (32 KiB) = (output >> 2) & 0x01
//   CHR (8 KiB)  =  output       & 0x03
// `readMapperRegister` at $4100|$4103==0x4100 returns the chip read value in
// the low nibble. Mirroring header-fixed; no IRQ.
// ===========================================================================

/// The TXC scrambling-accumulator chip (mappers 132 / 172 / 173 family). This
/// is the non-JV001 variant used by mapper 132.
#[derive(Clone, Copy, Default)]
struct TxcChip {
    accumulator: u8,
    inverter: u8,
    staging: u8,
    output: u8,
    increase: bool,
    invert: bool,
}

impl TxcChip {
    const MASK: u8 = 0x07;

    const fn output(self) -> u8 {
        self.output
    }

    const fn read(self) -> u8 {
        let invert_xor = if self.invert { 0xFF } else { 0x00 };
        (self.accumulator & Self::MASK) | ((self.inverter ^ invert_xor) & !Self::MASK)
    }

    /// `absolute` is the full CPU address of the write (e.g. `0x4100` or
    /// `0x8000`); `value` is the 4-bit-masked data already supplied by the
    /// caller for the register path.
    const fn write(&mut self, absolute: u16, value: u8) {
        if absolute < 0x8000 {
            match absolute & 0xE103 {
                0x4100 => {
                    if self.increase {
                        self.accumulator = self.accumulator.wrapping_add(1);
                    } else {
                        let invert_xor = if self.invert { 0xFF } else { 0x00 };
                        self.accumulator = ((self.accumulator & !Self::MASK)
                            | (self.staging & Self::MASK))
                            ^ invert_xor;
                    }
                }
                0x4101 => self.invert = (value & 0x01) != 0,
                0x4102 => {
                    self.staging = value & Self::MASK;
                    self.inverter = value & !Self::MASK;
                }
                0x4103 => self.increase = (value & 0x01) != 0,
                _ => {}
            }
        } else {
            // $8000+ latches the scrambled output (non-JV001 layout).
            self.output = (self.accumulator & 0x0F) | ((self.inverter & 0x08) << 1);
        }
    }
}

/// Mapper 132 (TXC 22211).
pub struct Txc132 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    txc: TxcChip,
    mirroring: Mirroring,
}

impl Txc132 {
    /// Construct a new mapper 132 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 132 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 132 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            txc: TxcChip::default(),
            mirroring,
        })
    }
}

impl Mapper for Txc132 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // The chip's read port lives at $4100-$5FFF (mapped); only the $4020-$40FF
    // gap below it is open bus. $8000-$FFFF PRG-ROM stays mapped (the trait
    // default) — a `!(...)` here would wrongly open-bus the program ROM and the
    // reset vector, so the board never boots.
    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        (0x4020..=0x40FF).contains(&addr)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x4100..=0x5FFF => {
                // GeraNES decodes the read on (addr & 0x0103) == 0x0100.
                if (addr & 0x0103) == 0x0100 {
                    self.txc.read() & 0x0F
                } else {
                    0
                }
            }
            0x8000..=0xFFFF => {
                let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
                let bank = (((self.txc.output() >> 2) & 0x01) as usize) % count;
                self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x4100..=0x5FFF).contains(&addr) || (0x8000..=0xFFFF).contains(&addr) {
            self.txc.write(addr, value & 0x0F);
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = ((self.txc.output() & 0x03) as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.mirroring);
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(7 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.txc.accumulator);
        out.push(self.txc.inverter);
        out.push(self.txc.staging);
        out.push(self.txc.output);
        out.push(u8::from(self.txc.increase));
        out.push(u8::from(self.txc.invert));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 7 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.txc.accumulator = data[1];
        self.txc.inverter = data[2];
        self.txc.staging = data[3];
        self.txc.output = data[4];
        self.txc.increase = data[5] != 0;
        self.txc.invert = data[6] != 0;
        self.vram.copy_from_slice(&data[7..7 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 133 — Sachen 3009 (and 3011).
//
// One register decoded on A8 across $4100-$5FFF: byte selects
//   PRG (32 KiB) = (value >> 2) & 0x01
//   CHR (8 KiB)  =  value       & 0x03
// Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 133 (Sachen 3009).
pub struct Sachen133 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Sachen133 {
    /// Construct a new mapper 133 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 133 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 133 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }
}

impl Mapper for Sachen133 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x4100..=0x5FFF).contains(&addr) && (addr & 0x0100) != 0 {
            self.prg_bank = (value >> 2) & 0x01;
            self.chr_bank = value & 0x03;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.mirroring);
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.vram.len();
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
        self.chr_bank = data[2];
        self.vram.copy_from_slice(&data[3..3 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 145 — Sachen SA-72007.
//
// A single CHR-bank bit (the high data bit) is decoded when the address
// satisfies (absolute & 0x4100) == 0x4100, in BOTH the $4100 register window
// and the $6000 save-RAM window:
//   CHR (8 KiB) = (value >> 7) & 0x01
// PRG is a fixed 32 KiB (bank 0). Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 145 (Sachen `SA-72007`).
pub struct Sachen145 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Sachen145 {
    /// Construct a new mapper 145 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is empty / not a multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        // Real SA-72007 dumps (e.g. "Sidewinder") are 16 KiB PRG / NROM-128-style
        // — the fixed bank is simply mirrored across the 32 KiB CPU window. Accept
        // any non-zero 16 KiB multiple (16 KiB mirrors; 32 KiB maps 1:1).
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 145 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 145 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_bank: 0,
            mirroring,
        })
    }
}

impl Mapper for Sachen145 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Fixed bank 0, mirrored across the 32 KiB window for sub-32 KiB PRG.
            self.prg_rom[(addr as usize - 0x8000) % self.prg_rom.len()]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // CHR bank decoded when (addr & 0x4100) == 0x4100 in both the register
        // ($4100-$5FFF) and save-RAM ($6000-$7FFF) windows.
        if (0x4100..=0x7FFF).contains(&addr) && (addr & 0x4100) == 0x4100 {
            self.chr_bank = (value >> 7) & 0x01;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.mirroring);
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 2 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.chr_bank = data[1];
        self.vram.copy_from_slice(&data[2..2 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 146 — Sachen (mapper-79-equivalent behaviour).
//
// Identical decode to NINA-03 (mapper 79) but Sachen wired the register into
// the $4100-$5FFF window decoded on A8 AND aliased into the $6000-$7FFF
// save-RAM window (offset by $2000). The byte selects:
//   PRG (32 KiB) = (value >> 3) & 0x01
//   CHR (8 KiB)  =  value       & 0x07
// Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 146 (Sachen, `NINA-03`/mapper-79-equivalent behaviour).
pub struct Sachen146 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Sachen146 {
    /// Construct a new mapper 146 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 146 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 146 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }

    const fn apply(&mut self, value: u8) {
        self.prg_bank = (value >> 3) & 0x01;
        self.chr_bank = value & 0x07;
    }
}

impl Mapper for Sachen146 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // $4100-$5FFF on A8, and the $6000-$7FFF save-RAM alias.
        if ((0x4100..=0x5FFF).contains(&addr) && (addr & 0x0100) != 0)
            || (0x6000..=0x7FFF).contains(&addr)
        {
            self.apply(value);
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.mirroring);
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.vram.len();
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
        self.chr_bank = data[2];
        self.vram.copy_from_slice(&data[3..3 + self.vram.len()]);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    /// 32 KiB-banked PRG: byte 0 of each 32 KiB bank holds the bank index, the
    /// rest is 0xFF (so a bus-conflict AND at offset 0 is observable while
    /// other offsets are transparent).
    fn synth_prg_32k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_32K];
        for b in 0..banks {
            v[b * PRG_BANK_32K] = b as u8;
        }
        v.into_boxed_slice()
    }

    /// 16 KiB-banked PRG: byte 0 of each 16 KiB bank holds the bank index.
    fn synth_prg_16k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_16K];
        for b in 0..banks {
            v[b * PRG_BANK_16K] = b as u8;
        }
        v.into_boxed_slice()
    }

    /// 8 KiB-banked CHR: byte 0 of each 8 KiB bank holds the bank index.
    fn synth_chr_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_8K];
        for b in 0..banks {
            v[b * CHR_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    /// 4 KiB-banked CHR: byte 0 of each 4 KiB bank holds the bank index.
    fn synth_chr_4k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_4K];
        for b in 0..banks {
            v[b * CHR_BANK_4K] = b as u8;
        }
        v.into_boxed_slice()
    }

    /// 2 KiB-banked CHR: byte 0 of each 2 KiB bank holds the bank index.
    fn synth_chr_2k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_2K];
        for b in 0..banks {
            v[b * CHR_BANK_2K] = b as u8;
        }
        v.into_boxed_slice()
    }

    // --- Mapper 15 ---------------------------------------------------------

    #[test]
    fn m15_mode0_two_16k_halves() {
        // 8 16 KiB banks = 128 KiB.
        let mut m = Multicart15::new(synth_prg_16k(8), &[]).unwrap();
        // mode 0 ($8000), prg_bank = 2, mirroring bit clear (vertical).
        m.cpu_write(0x8000, 0b0000_0010);
        assert_eq!(m.cpu_read(0x8000), 2); // low half = bank 2
        assert_eq!(m.cpu_read(0xC000), 3); // high half = bank 2|1 = 3
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m15_mirroring_bit() {
        let mut m = Multicart15::new(synth_prg_16k(8), &[]).unwrap();
        m.cpu_write(0x8000, 0b0100_0000); // bit 6 = horizontal
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m15_mode3_single_bank_mirrored() {
        let mut m = Multicart15::new(synth_prg_16k(8), &[]).unwrap();
        // mode 3 ($8003), prg_bank = 5.
        m.cpu_write(0x8003, 0b0000_0101);
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xC000), 5); // 16 KiB mirrored across the window
    }

    #[test]
    fn m15_chr_ram_write_protect() {
        let mut m = Multicart15::new(synth_prg_16k(8), &[]).unwrap();
        // mode 0 -> protected.
        m.cpu_write(0x8000, 0);
        m.ppu_write(0x0000, 0xAB);
        assert_eq!(m.ppu_read(0x0000), 0);
        // mode 2 -> writable.
        m.cpu_write(0x8002, 0);
        m.ppu_write(0x0000, 0xCD);
        assert_eq!(m.ppu_read(0x0000), 0xCD);
    }

    // --- Mapper 36 ---------------------------------------------------------

    #[test]
    fn m36_register_decodes_on_a8() {
        let mut m = Txc36::new(synth_prg_32k(4), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // $4100 has A8 set: value PPPP_CCCC. 0b0011_1010 -> PRG 3, CHR 10.
        m.cpu_write(0x4100, 0b0011_1010);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.ppu_read(0x0000), 10);
        // In-window address with A8 clear ($4200) must not latch.
        m.cpu_write(0x4200, 0b0000_0001);
        assert_eq!(m.cpu_read(0x8000), 3);
    }

    // --- Mapper 39 ---------------------------------------------------------

    #[test]
    fn m39_full_byte_selects_32k() {
        let mut m = Subor39::new(synth_prg_32k(4), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 2);
        assert_eq!(m.cpu_read(0x8000), 2);
        // No bus conflict: value sticks regardless of underlying byte.
        m.cpu_write(0xFFFF, 1);
        assert_eq!(m.cpu_read(0x8000), 1);
    }

    // --- Mapper 61 ---------------------------------------------------------

    #[test]
    fn m61_16k_mode_address_decode() {
        // 16 16 KiB banks.
        let mut m = Multicart61::new(synth_prg_16k(16), &[]).unwrap();
        // Choose addr with A&0x0F = 3, A>>5&1 = 0 -> page = 6; A&0x10 set (16k);
        // A&0x80 set (horizontal). addr = 0x8000 | 0x10 | 0x80 | 0x03 = 0x8093.
        m.cpu_write(0x8093, 0x00);
        assert_eq!(m.cpu_read(0x8000), 6);
        assert_eq!(m.cpu_read(0xC000), 6); // 16 KiB mirrored
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m61_32k_mode() {
        let mut m = Multicart61::new(synth_prg_16k(16), &[]).unwrap();
        // A&0x0F = 2, A>>5&1 = 0 -> page = 4; 32 KiB mode (A&0x10 clear).
        // 32 KiB bank = page>>1 = 2. addr = 0x8000 | 0x02 = 0x8002.
        m.cpu_write(0x8002, 0x00);
        // 32 KiB bank 2 = 16 KiB banks 4 and 5.
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xC000), 5);
    }

    // --- Mapper 62 ---------------------------------------------------------

    #[test]
    fn m62_address_and_data_decode() {
        let mut m =
            Multicart62::new(synth_prg_16k(8), synth_chr_8k(256), Mirroring::Vertical).unwrap();
        // prg_page = ((A&0x3F00)>>8) | (A&0x40); pick A bits so page small.
        // A = 0x8000 | (0x01 << 8) | 0x20(16k mode) | 0x80(horiz) | 0x05(chr lo)
        //   prg_page = 0x01, 16k mode, horizontal, chr = (5<<2)|data&3.
        let addr = 0x8000 | (0x01 << 8) | 0x20 | 0x80 | 0x05;
        m.cpu_write(addr, 0x02); // data low 2 bits = 2
        assert_eq!(m.cpu_read(0x8000), 1); // 16k bank 1
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // chr_bank = (5<<2)|2 = 22.
        assert_eq!(m.ppu_read(0x0000), 22);
    }

    // --- Mapper 72 ---------------------------------------------------------

    #[test]
    fn m72_strobe_latches_on_rising_edge() {
        let mut m = Jaleco72::new(synth_prg_16k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // PRG window offsets >0 hold 0xFF, so bus conflict is transparent there.
        // Write to $8001 (byte 0xFF). PRG strobe (bit7) rising + bank 3.
        m.cpu_write(0x8001, 0b1000_0011);
        assert_eq!(m.cpu_read(0x8000), 3);
        // Last 16 KiB bank fixed at $C000: bank 7.
        assert_eq!(m.cpu_read(0xC000), 7);
        // CHR strobe (bit6) rising + bank 5.
        m.cpu_write(0x8001, 0b0100_0101);
        assert_eq!(m.ppu_read(0x0000), 5);
    }

    #[test]
    fn m72_no_relatch_without_falling_edge() {
        let mut m = Jaleco72::new(synth_prg_16k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8001, 0b1000_0011); // latch PRG 3
        assert_eq!(m.cpu_read(0x8000), 3);
        // Strobe still high, new bank value -> must NOT re-latch.
        m.cpu_write(0x8001, 0b1000_0101);
        assert_eq!(m.cpu_read(0x8000), 3);
        // Drop strobe, then raise again -> re-latches.
        m.cpu_write(0x8001, 0b0000_0000);
        m.cpu_write(0x8001, 0b1000_0101);
        assert_eq!(m.cpu_read(0x8000), 5);
    }

    // --- Mapper 92 ---------------------------------------------------------

    #[test]
    fn m92_uses_5bit_prg_field() {
        let mut m =
            Jaleco92::new(synth_prg_16k(32), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // 5-bit PRG field: value 0b1001_0001 -> strobe + bank 0x11 = 17.
        m.cpu_write(0x8001, 0b1001_0001);
        // JF-19 layout: $8000 is the FIXED first bank (0); the switchable bank
        // appears at $C000.
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 17);
    }

    // --- Mapper 77 ---------------------------------------------------------

    #[test]
    fn m77_prg_and_2k_chr_with_bus_conflict() {
        let mut m = Irem77::new(synth_prg_32k(4), synth_chr_2k(16)).unwrap();
        // Write to $8001 (byte 0xFF, transparent). [CCCC PPPP] = 0b0011_0010.
        // PRG = 2, CHR (2 KiB at $0000) = 3.
        m.cpu_write(0x8001, 0b0011_0010);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.ppu_read(0x0000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::FourScreen);
    }

    #[test]
    fn m77_chr_ram_and_four_screen_nt() {
        let mut m = Irem77::new(synth_prg_32k(2), synth_chr_2k(8)).unwrap();
        // $0800-$1FFF is CHR-RAM.
        m.ppu_write(0x0800, 0xAB);
        assert_eq!(m.ppu_read(0x0800), 0xAB);
        // Four independent nametables in on-cart RAM via the hooks.
        assert!(m.nametable_write(0x2000, 0x11));
        assert!(m.nametable_write(0x2400, 0x22));
        assert!(m.nametable_write(0x2800, 0x33));
        assert!(m.nametable_write(0x2C00, 0x44));
        assert_eq!(m.nametable_fetch(0x2000), Some(0x11));
        assert_eq!(m.nametable_fetch(0x2400), Some(0x22));
        assert_eq!(m.nametable_fetch(0x2800), Some(0x33));
        assert_eq!(m.nametable_fetch(0x2C00), Some(0x44));
    }

    // --- Mapper 96 ---------------------------------------------------------

    #[test]
    fn m96_prg_and_outer_chr() {
        let mut m =
            Bandai96::new(synth_prg_32k(4), synth_chr_4k(8), Mirroring::Horizontal).unwrap();
        // PRG = bits 0-1, outer CHR = bit 2.
        m.cpu_write(0x8000, 0b0000_0011); // PRG 3, outer 0
        assert_eq!(m.cpu_read(0x8000), 3);
        // $1000 slot = outer|0x03 = 3.
        assert_eq!(m.ppu_read(0x1000), 3);
    }

    #[test]
    fn m96_inner_chr_latched_from_ppu_bus() {
        let mut m =
            Bandai96::new(synth_prg_32k(4), synth_chr_4k(8), Mirroring::Horizontal).unwrap();
        // outer = 0 (PRG write bit2 clear).
        m.cpu_write(0x8000, 0);
        // Approach a nametable fetch from a non-$2xxx address (e.g. a pattern
        // fetch at $0000), then fetch $2100 -> inner = (0x2100>>8)&3 = 1.
        let _ = m.ppu_read(0x0000);
        let _ = m.ppu_read(0x2100);
        // $0000 slot bank = outer|inner = 0|1 = 1.
        assert_eq!(m.ppu_read(0x0000), 1);
        // Fetch $2300 -> inner = 3. (Must re-approach from outside $2xxx.)
        let _ = m.ppu_read(0x0000);
        let _ = m.ppu_read(0x2300);
        assert_eq!(m.ppu_read(0x0000), 3);
    }

    // --- Mapper 97 ---------------------------------------------------------

    #[test]
    fn m97_fixed_first_switchable_second() {
        let mut m = Irem97::new(synth_prg_16k(8), synth_chr_8k(1), Mirroring::Horizontal).unwrap();
        // $8000-$BFFF fixed to last bank (7).
        assert_eq!(m.cpu_read(0x8000), 7);
        // Switch $C000 bank to 3, set vertical mirroring (bit 7).
        m.cpu_write(0x8000, 0b1000_0011);
        assert_eq!(m.cpu_read(0xC000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        // $8000 still fixed.
        assert_eq!(m.cpu_read(0x8000), 7);
    }

    // --- Mapper 132 --------------------------------------------------------

    #[test]
    fn m132_txc_chip_drives_banks() {
        let mut m = Txc132::new(synth_prg_32k(2), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        // Program the chip: set staging via $4102 (low 3 bits = staging,
        // high bits -> inverter), set increase off ($4103 = 0), then $4100
        // loads accumulator from staging, then $8000 latches the output.
        m.cpu_write(0x4103, 0x00); // increase = false
        m.cpu_write(0x4102, 0b0000_1011 & 0x0F); // staging = 3 (0b011), inverter = 0b1000
        m.cpu_write(0x4100, 0x00); // accumulator = staging (no invert) = 3
        m.cpu_write(0x8000, 0x00); // latch: output = (acc&0xF) | ((inv&8)<<1)
        // acc = 3, inverter low nibble 0b1000 -> (8<<1)=0x10
        // output = 3 | 0x10 = 0x13.
        // PRG = (0x13>>2)&1 = 0; CHR = 0x13&3 = 3.
        assert_eq!(m.cpu_read(0x8000), 0); // PRG bank 0
        assert_eq!(m.ppu_read(0x0000), 3); // CHR bank 3
        // Register read window is mapped (not open bus).
        assert!(!m.cpu_read_unmapped(0x4100));
    }

    // --- Mapper 133 --------------------------------------------------------

    #[test]
    fn m133_register_on_a8() {
        let mut m = Sachen133::new(synth_prg_32k(2), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        // value: PRG = (v>>2)&1, CHR = v&3. 0b0000_0111 -> PRG 1, CHR 3.
        m.cpu_write(0x4100, 0b0000_0111);
        assert_eq!(m.cpu_read(0x8000), 1);
        assert_eq!(m.ppu_read(0x0000), 3);
        // A8 clear -> no latch.
        m.cpu_write(0x4200, 0b0000_0000);
        assert_eq!(m.cpu_read(0x8000), 1);
    }

    // --- Mapper 145 --------------------------------------------------------

    #[test]
    fn m145_chr_from_data_bit7() {
        let mut m = Sachen145::new(synth_prg_32k(1), synth_chr_8k(2), Mirroring::Vertical).unwrap();
        // Default CHR bank 0.
        assert_eq!(m.ppu_read(0x0000), 0);
        // (addr & 0x4100) == 0x4100 -> $4100 qualifies. Bit 7 set -> CHR 1.
        m.cpu_write(0x4100, 0x80);
        assert_eq!(m.ppu_read(0x0000), 1);
        // Also decoded in the $6000 save-RAM window ($6100 has 0x4100 bits).
        m.cpu_write(0x6100, 0x00);
        assert_eq!(m.ppu_read(0x0000), 0);
        // PRG is fixed 32 KiB bank 0.
        assert_eq!(m.cpu_read(0x8000), 0);
    }

    // --- Mapper 146 --------------------------------------------------------

    #[test]
    fn m146_like_nina03() {
        let mut m = Sachen146::new(synth_prg_32k(2), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // value: PRG = (v>>3)&1, CHR = v&7. 0b0000_1101 -> PRG 1, CHR 5.
        m.cpu_write(0x4100, 0b0000_1101);
        assert_eq!(m.cpu_read(0x8000), 1);
        assert_eq!(m.ppu_read(0x0000), 5);
        // Save-RAM alias also latches.
        m.cpu_write(0x6000, 0b0000_0010); // PRG 0, CHR 2
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.ppu_read(0x0000), 2);
    }

    // --- Save-state round-trips (representative sample) --------------------

    #[test]
    fn save_state_round_trips() {
        // Mapper 15.
        let mut m = Multicart15::new(synth_prg_16k(8), &[]).unwrap();
        m.cpu_write(0x8001, 0b0100_0101);
        let blob = m.save_state();
        let mut m2 = Multicart15::new(synth_prg_16k(8), &[]).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.current_mirroring(), m.current_mirroring());

        // Mapper 72 (strobe state must survive).
        let mut j = Jaleco72::new(synth_prg_16k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        j.cpu_write(0x8001, 0b1100_0011); // PRG 3 + CHR 3, both strobes high
        let blob = j.save_state();
        let mut j2 =
            Jaleco72::new(synth_prg_16k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        j2.load_state(&blob).unwrap();
        assert_eq!(j2.cpu_read(0x8000), 3);
        assert_eq!(j2.ppu_read(0x0000), 3);
        // Strobe still high after restore -> a same-value write must not relatch
        // from a fresh edge.
        j2.cpu_write(0x8001, 0b1100_0101);
        assert_eq!(j2.cpu_read(0x8000), 3);

        // Mapper 132 (TXC chip state).
        let mut t = Txc132::new(synth_prg_32k(2), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        t.cpu_write(0x4103, 0x00);
        t.cpu_write(0x4102, 0x03);
        t.cpu_write(0x4100, 0x00);
        t.cpu_write(0x8000, 0x00);
        let blob = t.save_state();
        let mut t2 = Txc132::new(synth_prg_32k(2), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        t2.load_state(&blob).unwrap();
        assert_eq!(t2.ppu_read(0x0000), t.ppu_read(0x0000));

        // Mapper 96 (PPU-bus latch state).
        let mut b =
            Bandai96::new(synth_prg_32k(4), synth_chr_4k(8), Mirroring::Horizontal).unwrap();
        b.cpu_write(0x8000, 0);
        let _ = b.ppu_read(0x0000);
        let _ = b.ppu_read(0x2200);
        let blob = b.save_state();
        let mut b2 =
            Bandai96::new(synth_prg_32k(4), synth_chr_4k(8), Mirroring::Horizontal).unwrap();
        b2.load_state(&blob).unwrap();
        assert_eq!(b2.ppu_read(0x0000), b.ppu_read(0x0000));
    }
}

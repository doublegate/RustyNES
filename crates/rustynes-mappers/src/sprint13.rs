//! Sprint 13 reusable-ASIC NTDEC / TXC / BMC mappers
//! (v1.8.9 "Backlog" beta.6 mapper-breadth continuation, 168 -> 172).
//!
//! A best-effort (Tier-2) batch of NTDEC / TXC / discrete-BMC multicart boards
//! ported register-for-register from the reference emulators (`Mesen2`
//! `Ntdec/`, `Txc/`, `Unlicensed/`) and the nesdev wiki. Like `sprint5`..
//! `sprint12`, banking math is translated into direct slice indexing and every
//! bank select wraps with `% count`, so a register write can never index out of
//! bounds (no panics on register access — required for the `#![no_std]` chip
//! stack). All boards here are register-decode + save-state unit-tested only and
//! are **never** accuracy-gated (see `tier.rs` `MapperTier::BestEffort` +
//! `docs/adr/0011-mapper-tiering.md`).
//!
//! Clusters covered (NTDEC / TXC / BMC):
//!
//! - **Mapper 193** ([`NtdecTc112`]) — NTDEC TC-112 (*Fighting Hero*): a
//!   `$6000-$7FFF` four-register surface selecting one switchable 8 KiB PRG bank
//!   (the last three 8 KiB windows are fixed) plus three 2 KiB CHR selects
//!   (one paired). Ported from `Mesen2 Ntdec/NtdecTc112.h` + nesdev wiki
//!   "INES Mapper 193".
//! - **Mapper 204** ([`Bmc204`]) — a simple address-decoded NROM/UNROM 2-in-1
//!   BMC multicart: the written *address* low bits pick the 16 KiB PRG pair, the
//!   8 KiB CHR bank, and the mirroring. Ported from
//!   `Mesen2 Unlicensed/Mapper204.h`.
//! - **Mapper 221** ([`NtdecN625092`]) — NTDEC N625092 multicart: a `$8000`
//!   mode register (carrying the outer bank, NROM/UNROM split, and mirroring) +
//!   a `$C000` inner-PRG register. Ported from `Mesen2 Ntdec/Mapper221.h`.
//! - **Mapper 299** ([`Bmc11160`]) — TXC/BMC-11160: a single value-decoded
//!   `$8000-$FFFF` register selecting a 32 KiB PRG bank, an 8 KiB CHR bank
//!   (outer | inner), and the mirroring. Ported from
//!   `Mesen2 Txc/Bmc11160.h`.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::match_same_arms,
    clippy::doc_markdown,
    clippy::similar_names,
    clippy::missing_const_for_fn
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_8K: usize = 0x2000;
const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_2K: usize = 0x0800;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// Shared nametable + mirroring helpers (mirror the other simple-mapper modules).
// ---------------------------------------------------------------------------

const fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

const fn mirroring_to_byte(m: Mirroring) -> u8 {
    match m {
        Mirroring::Horizontal => 0,
        Mirroring::Vertical => 1,
        Mirroring::SingleScreenA => 2,
        Mirroring::SingleScreenB => 3,
        Mirroring::FourScreen => 4,
        Mirroring::MapperControlled => 5,
    }
}

const fn byte_to_mirroring(b: u8, fallback: Mirroring) -> Mirroring {
    match b {
        0 => Mirroring::Horizontal,
        1 => Mirroring::Vertical,
        2 => Mirroring::SingleScreenA,
        3 => Mirroring::SingleScreenB,
        4 => Mirroring::FourScreen,
        5 => Mirroring::MapperControlled,
        _ => fallback,
    }
}

/// Validate a PRG-ROM image is a non-zero multiple of 8 KiB.
fn check_prg(prg: &[u8], id: u16) -> Result<(), MapperError> {
    if prg.is_empty() || !prg.len().is_multiple_of(PRG_BANK_8K) {
        return Err(MapperError::Invalid(format!(
            "mapper {id} PRG-ROM size {} is not a non-zero multiple of 8 KiB",
            prg.len()
        )));
    }
    Ok(())
}

/// Allocate the CHR slice, falling back to an 8 KiB CHR-RAM bank when the ROM
/// ships no CHR-ROM. Returns `(chr, is_ram)`.
fn chr_or_ram(chr_rom: Box<[u8]>) -> (Box<[u8]>, bool) {
    if chr_rom.is_empty() {
        (vec![0u8; CHR_BANK_8K].into_boxed_slice(), true)
    } else {
        (chr_rom, false)
    }
}

// ===========================================================================
// NtdecTc112 (mapper 193) — NTDEC TC-112 (*Fighting Hero*).
//
// PRG: 8 KiB pages. The last three 8 KiB windows ($A000/$C000/$E000) are fixed
// to the final three banks; $8000 is the one switchable window (register 3).
// CHR: 2 KiB pages. Register 0 selects a paired 2 KiB window into the first two
// slots ($0000 + $0800), register 1 the third ($1000), register 2 the fourth
// ($1800). Registers live at $6000-$7FFF (addr & 3). Ported from Mesen2
// Ntdec/NtdecTc112.h.
// ===========================================================================

/// NTDEC TC-112 (mapper 193).
pub struct NtdecTc112 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    mirroring: Mirroring,
    /// Switchable 8 KiB PRG window for $8000.
    prg0: usize,
    /// 2 KiB CHR windows for $0000/$0800/$1000/$1800.
    chr2: [usize; 4],
}

impl NtdecTc112 {
    fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, 193)?;
        let (chr, chr_is_ram) = chr_or_ram(chr_rom);
        Ok(Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
            prg0: 0,
            chr2: [0; 4],
        })
    }

    fn prg_count_8k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_8K).max(1)
    }

    fn chr_count_2k(&self) -> usize {
        (self.chr.len() / CHR_BANK_2K).max(1)
    }
}

impl Mapper for NtdecTc112 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // The final three 8 KiB windows are fixed to the last three banks.
            0x8000..=0xFFFF => {
                let count = self.prg_count_8k();
                let slot = (addr as usize - 0x8000) / PRG_BANK_8K;
                let bank = match slot {
                    0 => self.prg0 % count,
                    _ => (count - (4 - slot)) % count, // last-3 fixed window
                };
                self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x6000..=0x7FFF).contains(&addr) {
            match addr & 0x03 {
                0 => {
                    // Paired 2 KiB CHR select into slots 0 and 1.
                    self.chr2[0] = (value >> 1) as usize;
                    self.chr2[1] = (value >> 1) as usize + 1;
                }
                1 => self.chr2[2] = (value >> 1) as usize,
                2 => self.chr2[3] = (value >> 1) as usize,
                _ => self.prg0 = value as usize,
            }
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    return self.chr[addr as usize & (CHR_BANK_8K - 1)];
                }
                let slot = (addr as usize) / CHR_BANK_2K;
                let count = self.chr_count_2k();
                let bank = self.chr2[slot] % count;
                self.chr[bank * CHR_BANK_2K + (addr as usize & (CHR_BANK_2K - 1))]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                self.chr[addr as usize & (CHR_BANK_8K - 1)] = value;
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
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(1 + 4 + 16 + 1 + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&(self.prg0 as u32).to_le_bytes());
        for c in &self.chr2 {
            out.extend_from_slice(&(*c as u32).to_le_bytes());
        }
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 1 + 4 + 16 + 1 + self.vram.len() + chr_ram;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let rd = |c: usize| {
            u32::from_le_bytes([data[c], data[c + 1], data[c + 2], data[c + 3]]) as usize
        };
        let mut c = 1;
        self.prg0 = rd(c);
        c += 4;
        for s in &mut self.chr2 {
            *s = rd(c);
            c += 4;
        }
        self.mirroring = byte_to_mirroring(data[c], self.mirroring);
        c += 1;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        }
        Ok(())
    }
}

/// Mapper 193 (NTDEC TC-112, *Fighting Hero*).
///
/// # Errors
/// [`MapperError::Invalid`] on a bad PRG size.
pub fn new_m193(
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    mirroring: Mirroring,
) -> Result<NtdecTc112, MapperError> {
    NtdecTc112::new(prg_rom, chr_rom, mirroring)
}

// ===========================================================================
// Bmc204 (mapper 204) — discrete NROM/UNROM 2-in-1 BMC multicart.
//
// The written *address* low bits select the layout: `bitMask = addr & 0x06`
// gives the 16 KiB PRG block, and (when bitMask != 0x06) `addr & 1` picks the
// inner half. Both PRG windows ($8000 + $C000) and the 8 KiB CHR window track
// the decoded page; `addr & 0x10` flips the mirroring. Ported from Mesen2
// Unlicensed/Mapper204.h.
// ===========================================================================

/// Discrete NROM/UNROM 2-in-1 BMC multicart (mapper 204).
pub struct Bmc204 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    mirroring: Mirroring,
    /// 16 KiB PRG windows for $8000 and $C000.
    prg0: usize,
    prg1: usize,
    /// 8 KiB CHR window.
    chr8: usize,
}

impl Bmc204 {
    fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, 204)?;
        let (chr, chr_is_ram) = chr_or_ram(chr_rom);
        let mut m = Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
            prg0: 0,
            prg1: 0,
            chr8: 0,
        };
        // Power-on: WriteRegister(0x8000, 0).
        m.write_addr(0x8000);
        Ok(m)
    }

    fn prg_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn chr_count_8k(&self) -> usize {
        (self.chr.len() / CHR_BANK_8K).max(1)
    }

    fn write_addr(&mut self, addr: u16) {
        let bit_mask = (addr & 0x06) as usize;
        let page = bit_mask
            + if bit_mask == 0x06 {
                0
            } else {
                (addr & 0x01) as usize
            };
        self.prg0 = page;
        self.prg1 = bit_mask
            + if bit_mask == 0x06 {
                1
            } else {
                (addr & 0x01) as usize
            };
        self.chr8 = page;
        self.mirroring = if addr & 0x10 != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        };
    }

    fn prg_byte(&self, slot16: usize, addr: u16) -> u8 {
        let count = self.prg_count_16k();
        let bank = slot16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Bmc204 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => self.prg_byte(self.prg0, addr),
            0xC000..=0xFFFF => self.prg_byte(self.prg1, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if addr >= 0x8000 {
            self.write_addr(addr);
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    return self.chr[addr as usize & (CHR_BANK_8K - 1)];
                }
                let bank = self.chr8 % self.chr_count_8k();
                self.chr[bank * CHR_BANK_8K + (addr as usize & 0x1FFF)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                self.chr[addr as usize & (CHR_BANK_8K - 1)] = value;
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
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(1 + 12 + 1 + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&(self.prg0 as u32).to_le_bytes());
        out.extend_from_slice(&(self.prg1 as u32).to_le_bytes());
        out.extend_from_slice(&(self.chr8 as u32).to_le_bytes());
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 1 + 12 + 1 + self.vram.len() + chr_ram;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let rd = |c: usize| {
            u32::from_le_bytes([data[c], data[c + 1], data[c + 2], data[c + 3]]) as usize
        };
        self.prg0 = rd(1);
        self.prg1 = rd(5);
        self.chr8 = rd(9);
        let mut c = 13;
        self.mirroring = byte_to_mirroring(data[c], self.mirroring);
        c += 1;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        }
        Ok(())
    }
}

/// Mapper 204 (discrete NROM/UNROM 2-in-1 BMC multicart).
///
/// # Errors
/// [`MapperError::Invalid`] on a bad PRG size.
pub fn new_m204(
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    mirroring: Mirroring,
) -> Result<Bmc204, MapperError> {
    Bmc204::new(prg_rom, chr_rom, mirroring)
}

// ===========================================================================
// NtdecN625092 (mapper 221) — NTDEC N625092 multicart.
//
// $8000 latches a 16-bit "mode" from the written address; $C000 latches the
// 3-bit inner PRG register. The outer bank is `(mode & 0xFC) >> 2`. When
// `mode & 0x02` the board is in UNROM-style mode (a switchable $8000 + a fixed
// $C000), with a NROM-256 sub-case when `mode & 0x0100`; otherwise both 16 KiB
// windows mirror the same NROM bank. `mode & 0x01` flips the mirroring. CHR is a
// single fixed 8 KiB window. Ported from Mesen2 Ntdec/Mapper221.h.
// ===========================================================================

/// NTDEC N625092 multicart (mapper 221).
pub struct NtdecN625092 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    mirroring: Mirroring,
    mode: u16,
    prg_reg: u8,
    /// 16 KiB PRG windows for $8000 and $C000.
    prg0: usize,
    prg1: usize,
}

impl NtdecN625092 {
    fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, 221)?;
        let (chr, chr_is_ram) = chr_or_ram(chr_rom);
        let mut m = Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
            mode: 0,
            prg_reg: 0,
            prg0: 0,
            prg1: 0,
        };
        m.update_state();
        Ok(m)
    }

    fn prg_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn update_state(&mut self) {
        let outer = ((self.mode & 0xFC) >> 2) as usize;
        let reg = self.prg_reg as usize;
        if self.mode & 0x02 != 0 {
            if self.mode & 0x0100 != 0 {
                // NROM-256 sub-case: switchable low + fixed (outer | 7) high.
                self.prg0 = outer | reg;
                self.prg1 = outer | 0x07;
            } else {
                // UNROM 2x16 KiB aligned pair (SelectPrgPage2x): the inner reg
                // (masked to even) selects a 32 KiB-aligned window.
                let b = outer | (reg & 0x06);
                self.prg0 = b;
                self.prg1 = b | 1;
            }
        } else {
            // NROM: both windows mirror the same bank.
            self.prg0 = outer | reg;
            self.prg1 = outer | reg;
        }
        self.mirroring = if self.mode & 0x01 != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        };
    }

    fn prg_byte(&self, slot16: usize, addr: u16) -> u8 {
        let count = self.prg_count_16k();
        let bank = slot16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for NtdecN625092 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => self.prg_byte(self.prg0, addr),
            0xC000..=0xFFFF => self.prg_byte(self.prg1, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        match addr & 0xC000 {
            0x8000 => {
                self.mode = addr;
                self.update_state();
            }
            0xC000 => {
                self.prg_reg = (addr & 0x07) as u8;
                self.update_state();
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[addr as usize & (CHR_BANK_8K - 1)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                self.chr[addr as usize & (CHR_BANK_8K - 1)] = value;
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
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(1 + 2 + 1 + 8 + 1 + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.mode.to_le_bytes());
        out.push(self.prg_reg);
        out.extend_from_slice(&(self.prg0 as u32).to_le_bytes());
        out.extend_from_slice(&(self.prg1 as u32).to_le_bytes());
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 1 + 2 + 1 + 8 + 1 + self.vram.len() + chr_ram;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let rd = |c: usize| {
            u32::from_le_bytes([data[c], data[c + 1], data[c + 2], data[c + 3]]) as usize
        };
        self.mode = u16::from_le_bytes([data[1], data[2]]);
        self.prg_reg = data[3];
        self.prg0 = rd(4);
        self.prg1 = rd(8);
        let mut c = 12;
        self.mirroring = byte_to_mirroring(data[c], self.mirroring);
        c += 1;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        }
        Ok(())
    }
}

/// Mapper 221 (NTDEC N625092 multicart).
///
/// # Errors
/// [`MapperError::Invalid`] on a bad PRG size.
pub fn new_m221(
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    mirroring: Mirroring,
) -> Result<NtdecN625092, MapperError> {
    NtdecN625092::new(prg_rom, chr_rom, mirroring)
}

// ===========================================================================
// Bmc11160 (mapper 299) — TXC/BMC-11160 multicart.
//
// One value-decoded $8000-$FFFF register: bits 4-6 select a 32 KiB PRG bank,
// the 8 KiB CHR bank is `(bank << 2) | (value & 0x03)`, and bit 7 flips the
// mirroring (set => vertical). Ported from Mesen2 Txc/Bmc11160.h.
// ===========================================================================

/// TXC/BMC-11160 multicart (mapper 299).
pub struct Bmc11160 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    mirroring: Mirroring,
    /// 32 KiB PRG window.
    prg32: usize,
    /// 8 KiB CHR window.
    chr8: usize,
}

impl Bmc11160 {
    fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, 299)?;
        let (chr, chr_is_ram) = chr_or_ram(chr_rom);
        let mut m = Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
            prg32: 0,
            chr8: 0,
        };
        // Power-on (Reset): WriteRegister(0x8000, 0).
        m.write_reg(0);
        Ok(m)
    }

    fn prg_count_32k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_32K).max(1)
    }

    fn chr_count_8k(&self) -> usize {
        (self.chr.len() / CHR_BANK_8K).max(1)
    }

    fn write_reg(&mut self, value: u8) {
        let bank = ((value >> 4) & 0x07) as usize;
        self.prg32 = bank;
        self.chr8 = (bank << 2) | (value as usize & 0x03);
        self.mirroring = if value & 0x80 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };
    }
}

impl Mapper for Bmc11160 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let count = self.prg_count_32k();
                let bank = self.prg32 % count;
                self.prg_rom[bank * PRG_BANK_32K + (addr as usize & 0x7FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            self.write_reg(value);
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    return self.chr[addr as usize & (CHR_BANK_8K - 1)];
                }
                let bank = self.chr8 % self.chr_count_8k();
                self.chr[bank * CHR_BANK_8K + (addr as usize & 0x1FFF)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                self.chr[addr as usize & (CHR_BANK_8K - 1)] = value;
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
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(1 + 8 + 1 + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&(self.prg32 as u32).to_le_bytes());
        out.extend_from_slice(&(self.chr8 as u32).to_le_bytes());
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 1 + 8 + 1 + self.vram.len() + chr_ram;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let rd = |c: usize| {
            u32::from_le_bytes([data[c], data[c + 1], data[c + 2], data[c + 3]]) as usize
        };
        self.prg32 = rd(1);
        self.chr8 = rd(5);
        let mut c = 9;
        self.mirroring = byte_to_mirroring(data[c], self.mirroring);
        c += 1;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        }
        Ok(())
    }
}

/// Mapper 299 (TXC/BMC-11160 multicart).
///
/// # Errors
/// [`MapperError::Invalid`] on a bad PRG size.
pub fn new_m299(
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    mirroring: Mirroring,
) -> Result<Bmc11160, MapperError> {
    Bmc11160::new(prg_rom, chr_rom, mirroring)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn prg(banks_8k: usize) -> Box<[u8]> {
        // Fill each 8 KiB bank with its index so bank routing is observable.
        let mut v = vec![0u8; banks_8k * PRG_BANK_8K];
        for (i, b) in v.chunks_mut(PRG_BANK_8K).enumerate() {
            b.fill(i as u8);
        }
        v.into_boxed_slice()
    }

    fn chr(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * CHR_BANK_8K];
        for (i, b) in v.chunks_mut(CHR_BANK_8K).enumerate() {
            b.fill(i as u8);
        }
        v.into_boxed_slice()
    }

    // ----- Mapper 193 (NTDEC TC-112) -----

    #[test]
    fn m193_last_three_prg_windows_are_fixed() {
        // 8 banks of 8 KiB. The last three 8 KiB windows must be fixed to the
        // last three banks (5,6,7) regardless of the switchable $8000 select.
        let mut m = new_m193(prg(8), chr(4), Mirroring::Vertical).unwrap();
        // $8000 defaults to bank 0.
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xA000), 5, "$A000 fixed to last-3");
        assert_eq!(m.cpu_read(0xC000), 6, "$C000 fixed to last-3");
        assert_eq!(m.cpu_read(0xE000), 7, "$E000 fixed to last-3");
        // Register 3 selects the switchable $8000 8 KiB window.
        m.cpu_write(0x6003, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        // Fixed windows are unaffected.
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn m193_chr_registers_select_2k_windows() {
        let mut m = new_m193(prg(2), chr(4), Mirroring::Vertical).unwrap();
        // reg0: paired 2 KiB select into slots 0+1. value 4 => (4>>1)=2 / 3.
        m.cpu_write(0x6000, 4);
        // chr bank N (2 KiB) of a 4x8KiB image => 16 2-KiB banks; bank 2 lives in
        // 8 KiB CHR bank 0 (banks 0..3) so its byte == 0.
        assert_eq!(m.ppu_read(0x0000), 0); // slot0 -> 2k bank 2 -> 8k bank0
        // reg1 -> slot2 ($1000); reg2 -> slot3 ($1800).
        m.cpu_write(0x6001, 8); // (8>>1)=4 -> 2k bank4 -> 8k bank1
        assert_eq!(m.ppu_read(0x1000), 1);
        m.cpu_write(0x6002, 12); // (12>>1)=6 -> 2k bank6 -> 8k bank1
        assert_eq!(m.ppu_read(0x1800), 1);
    }

    #[test]
    fn m193_chr_ram_when_no_chr_rom() {
        let mut m = new_m193(prg(2), Box::new([]), Mirroring::Vertical).unwrap();
        m.ppu_write(0x0123, 0xAB);
        assert_eq!(m.ppu_read(0x0123), 0xAB);
    }

    // ----- Mapper 204 -----

    #[test]
    fn m204_address_decode_selects_prg_and_chr() {
        let mut m = new_m204(prg(16), chr(8), Mirroring::Vertical).unwrap();
        // bitMask = addr&6 = 0; page = 0 + (addr&1) = 0.
        m.cpu_write(0x8000, 0);
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 0); // prg1 = 0 + (addr&1) = 0
        // addr 0x8007: bitMask = 6 -> page = 6+0 = 6; prg1 = 6+1 = 7.
        m.cpu_write(0x8007, 0);
        assert_eq!(m.cpu_read(0x8000), 12, "16k page 6 -> 8k bank 12");
    }

    #[test]
    fn m204_distinct_halves_in_bitmask6_mode() {
        let mut m = new_m204(prg(16), chr(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8006, 0); // bitMask 6: prg0=6, prg1=7 (16 KiB pages)
        // 16 KiB page 6 => 8 KiB bank 12 at $8000.
        assert_eq!(m.cpu_read(0x8000), 12);
        // 16 KiB page 7 => 8 KiB bank 14 at $C000.
        assert_eq!(m.cpu_read(0xC000), 14);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m204_mirroring_bit() {
        let mut m = new_m204(prg(4), chr(2), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8010, 0);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0x8000, 0);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    // ----- Mapper 221 (NTDEC N625092) -----

    #[test]
    fn m221_nrom_mode_mirrors_both_windows() {
        let mut m = new_m221(prg(16), chr(1), Mirroring::Vertical).unwrap();
        // mode = $8000 (mode&2 == 0 => NROM); outer = 0; prg_reg default 0.
        m.cpu_write(0x8000, 0);
        m.cpu_write(0xC003, 0); // inner reg = 3
        // NROM: both windows == outer|reg == 3.
        assert_eq!(m.cpu_read(0x8000), 6, "16k page 3 -> 8k bank 6");
        assert_eq!(m.cpu_read(0xC000), 6);
    }

    #[test]
    fn m221_unrom_nrom256_subcase() {
        let mut m = new_m221(prg(16), chr(1), Mirroring::Vertical).unwrap();
        // mode bits: set bit1 (UNROM) and bit8 (NROM-256 sub-case).
        // addr = 0x8000 | 0x0102 = 0x8102.
        m.cpu_write(0x8102, 0);
        m.cpu_write(0xC002, 0); // inner reg = 2
        // outer = (0x0102 & 0xFC) >> 2 = 0x00. prg0 = 0|2 = 2; prg1 = 0|7 = 7.
        assert_eq!(m.cpu_read(0x8000), 4, "16k page 2 -> 8k bank 4");
        assert_eq!(m.cpu_read(0xC000), 14, "16k page 7 -> 8k bank 14");
    }

    #[test]
    fn m221_mirroring_bit() {
        let mut m = new_m221(prg(4), chr(1), Mirroring::Horizontal).unwrap();
        m.cpu_write(0x8001, 0); // mode&1 set => horizontal
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0x8000, 0); // mode&1 clear => vertical
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    // ----- Mapper 299 (BMC-11160) -----

    #[test]
    fn m299_value_decode_selects_prg_chr_mirror() {
        let mut m = new_m299(prg(8 * 4), chr(32), Mirroring::Horizontal).unwrap();
        // 32 KiB PRG: 4 banks of 32 KiB (32 8-KiB banks / 4). value 0x10:
        // bank = (0x10>>4)&7 = 1; chr8 = (1<<2)|0 = 4; bit7 clear => horizontal.
        m.cpu_write(0x8000, 0x10);
        assert_eq!(m.cpu_read(0x8000), 4, "32k bank 1 -> 8k bank 4");
        assert_eq!(m.ppu_read(0x0000), 4, "chr 8k bank 4");
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m299_chr_low_bits_and_mirror() {
        let mut m = new_m299(prg(8 * 2), chr(16), Mirroring::Horizontal).unwrap();
        // value 0x83: bank = 0; chr8 = (0<<2)|3 = 3; bit7 set => vertical.
        m.cpu_write(0xFFFF, 0x83);
        assert_eq!(m.cpu_read(0x8000), 0, "32k bank 0");
        assert_eq!(m.ppu_read(0x0000), 3, "chr 8k bank 3");
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    // ----- save/load round-trips -----

    #[test]
    fn save_load_round_trips_all_four() {
        // m193
        let mut a = new_m193(prg(8), chr(4), Mirroring::Vertical).unwrap();
        a.cpu_write(0x6003, 5);
        a.cpu_write(0x6000, 6);
        let s = a.save_state();
        let mut b = new_m193(prg(8), chr(4), Mirroring::Vertical).unwrap();
        b.load_state(&s).unwrap();
        assert_eq!(a.cpu_read(0x8000), b.cpu_read(0x8000));
        assert_eq!(a.ppu_read(0x0000), b.ppu_read(0x0000));

        // m204
        let mut a = new_m204(prg(16), chr(8), Mirroring::Vertical).unwrap();
        a.cpu_write(0x8006, 0);
        let s = a.save_state();
        let mut b = new_m204(prg(16), chr(8), Mirroring::Horizontal).unwrap();
        b.load_state(&s).unwrap();
        assert_eq!(a.cpu_read(0xC000), b.cpu_read(0xC000));
        assert_eq!(a.current_mirroring(), b.current_mirroring());

        // m221
        let mut a = new_m221(prg(16), chr(1), Mirroring::Vertical).unwrap();
        a.cpu_write(0x8102, 0);
        a.cpu_write(0xC002, 0);
        let s = a.save_state();
        let mut b = new_m221(prg(16), chr(1), Mirroring::Horizontal).unwrap();
        b.load_state(&s).unwrap();
        assert_eq!(a.cpu_read(0x8000), b.cpu_read(0x8000));
        assert_eq!(a.cpu_read(0xC000), b.cpu_read(0xC000));

        // m299
        let mut a = new_m299(prg(8 * 4), chr(32), Mirroring::Horizontal).unwrap();
        a.cpu_write(0x8000, 0x91);
        let s = a.save_state();
        let mut b = new_m299(prg(8 * 4), chr(32), Mirroring::Horizontal).unwrap();
        b.load_state(&s).unwrap();
        assert_eq!(a.cpu_read(0x8000), b.cpu_read(0x8000));
        assert_eq!(a.ppu_read(0x0000), b.ppu_read(0x0000));
        assert_eq!(a.current_mirroring(), b.current_mirroring());
    }

    #[test]
    fn load_state_rejects_truncated_and_bad_version() {
        let m = new_m299(prg(8), chr(8), Mirroring::Horizontal).unwrap();
        let mut s = m.save_state();
        // Truncate.
        let mut t = m.save_state();
        t.pop();
        let mut m2 = new_m299(prg(8), chr(8), Mirroring::Horizontal).unwrap();
        assert!(matches!(
            m2.load_state(&t),
            Err(MapperError::Truncated { .. })
        ));
        // Bad version.
        s[0] = 0xFF;
        assert!(matches!(
            m2.load_state(&s),
            Err(MapperError::UnsupportedVersion(0xFF))
        ));
    }

    #[test]
    fn bad_prg_size_is_rejected() {
        // 100 bytes is not a multiple of 8 KiB.
        assert!(
            new_m193(
                vec![0u8; 100].into_boxed_slice(),
                chr(1),
                Mirroring::Vertical
            )
            .is_err()
        );
        assert!(
            new_m204(
                vec![0u8; 100].into_boxed_slice(),
                chr(1),
                Mirroring::Vertical
            )
            .is_err()
        );
        assert!(
            new_m221(
                vec![0u8; 100].into_boxed_slice(),
                chr(1),
                Mirroring::Vertical
            )
            .is_err()
        );
        assert!(
            new_m299(
                vec![0u8; 100].into_boxed_slice(),
                chr(1),
                Mirroring::Vertical
            )
            .is_err()
        );
    }
}

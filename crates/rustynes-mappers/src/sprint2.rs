//! Sprint 4-2 simple mappers: MMC2/MMC4, Color Dreams, CPROM, BNROM /
//! NINA-001, Camerica BF9093, VRC1.
//!
//! These are all small, no-IRQ mappers that mostly just bank-switch.
//! MMC2/MMC4 carry the tile-fetch CHR latch quirk (Punch-Out) which
//! requires a hook on PPU pattern fetches; the rest are PRG / CHR bank
//! select with optional bus-conflict semantics.
//!
//! See `docs/mappers.md` §Mapper coverage matrix.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_ref_mut,
    clippy::manual_range_patterns,
    clippy::match_same_arms,
    clippy::too_many_arguments
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_8K: usize = 0x2000;
const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_4K: usize = 0x1000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

// ---------------------------------------------------------------------------
// Shared nametable helper.
// ---------------------------------------------------------------------------

fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

// ---------------------------------------------------------------------------
// MMC2 (mapper 9) — Punch-Out!!  PRG: 8 KiB switchable @ $8000 + three
// fixed banks at $A000-$FFFF.  CHR: two 4 KiB switchable windows, each
// with two latched alternatives selected by the most recent pattern fetch
// at sentinel addresses ($0FD8/$0FE8 for window 0, $1FD8/$1FE8 for
// window 1).
// ---------------------------------------------------------------------------

/// MMC2 (Mapper 9).
pub struct Mmc2 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_lo_fd: u8,
    chr_lo_fe: u8,
    chr_hi_fd: u8,
    chr_hi_fe: u8,
    /// `false` -> use the FD bank for window 0 (`$0000-$0FFF`).
    latch_lo_fe: bool,
    /// `false` -> use the FD bank for window 1 (`$1000-$1FFF`).
    latch_hi_fe: bool,
    mirroring: Mirroring,
}

impl Mmc2 {
    /// Construct a new MMC2 mapper.
    ///
    /// PRG must be a non-zero multiple of 8 KiB; CHR-ROM is mandatory and
    /// must be a multiple of 4 KiB.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || prg_rom.len() % PRG_BANK_8K != 0 {
            return Err(MapperError::Invalid(format!(
                "MMC2 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len() % CHR_BANK_4K == 0 {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "MMC2 CHR-ROM size {} is not a multiple of 4 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_lo_fd: 0,
            chr_lo_fe: 0,
            chr_hi_fd: 0,
            chr_hi_fe: 0,
            latch_lo_fe: false,
            latch_hi_fe: false,
            mirroring,
        })
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total_8k = self.prg_rom.len() / PRG_BANK_8K;
        let last3 = total_8k.saturating_sub(3);
        let last2 = total_8k.saturating_sub(2);
        let last1 = total_8k.saturating_sub(1);
        let bank = match addr & 0xE000 {
            0x8000 => (self.prg_bank as usize) % total_8k.max(1),
            0xA000 => last3,
            0xC000 => last2,
            _ => last1, // $E000 + the implicit fallback
        };
        bank * PRG_BANK_8K + ((addr as usize) & 0x1FFF)
    }

    fn chr_offset(&mut self, addr: u16) -> usize {
        let addr = (addr & 0x1FFF) as usize;
        let total_4k = (self.chr_rom.len() / CHR_BANK_4K).max(1);
        let bank = if addr < CHR_BANK_4K {
            let b = if self.latch_lo_fe {
                self.chr_lo_fe
            } else {
                self.chr_lo_fd
            };
            (b as usize) % total_4k
        } else {
            let b = if self.latch_hi_fe {
                self.chr_hi_fe
            } else {
                self.chr_hi_fd
            };
            (b as usize) % total_4k
        };
        bank * CHR_BANK_4K + (addr & (CHR_BANK_4K - 1))
    }

    /// Update the CHR latch based on the fetched pattern address.
    /// $0FD8-$0FDF -> window 0 latch FD; $0FE8-$0FEF -> window 0 latch FE;
    /// similarly $1FD8-$1FDF / $1FE8-$1FEF for window 1.  Per nesdev wiki.
    fn update_latch(&mut self, addr: u16) {
        match addr & 0x3FF8 {
            0x0FD8 => self.latch_lo_fe = false,
            0x0FE8 => self.latch_lo_fe = true,
            0x1FD8 => self.latch_hi_fe = false,
            0x1FE8 => self.latch_hi_fe = true,
            _ => {}
        }
    }
}

impl Mapper for Mmc2 {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let off = self.prg_offset(addr);
                self.prg_rom[off % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr & 0xF000 {
            0xA000 => self.prg_bank = value & 0x0F,
            0xB000 => self.chr_lo_fd = value & 0x1F,
            0xC000 => self.chr_lo_fe = value & 0x1F,
            0xD000 => self.chr_hi_fd = value & 0x1F,
            0xE000 => self.chr_hi_fe = value & 0x1F,
            0xF000 => {
                self.mirroring = if value & 1 == 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
                let v = self.chr_rom[off % self.chr_rom.len()];
                self.update_latch(addr);
                v
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

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.vram.len());
        out.push(1u8); // version
        out.push(self.prg_bank);
        out.push(self.chr_lo_fd);
        out.push(self.chr_lo_fe);
        out.push(self.chr_hi_fd);
        out.push(self.chr_hi_fe);
        out.push(u8::from(self.latch_lo_fe));
        out.push(u8::from(self.latch_hi_fe));
        out.push(self.mirroring as u8);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 9 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != 1 {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank = data[1];
        self.chr_lo_fd = data[2];
        self.chr_lo_fe = data[3];
        self.chr_hi_fd = data[4];
        self.chr_hi_fe = data[5];
        self.latch_lo_fe = data[6] != 0;
        self.latch_hi_fe = data[7] != 0;
        self.mirroring = match data[8] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.vram.copy_from_slice(&data[9..9 + self.vram.len()]);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MMC4 (mapper 10) — like MMC2 but PRG is 16 KiB switchable + 16 KiB fixed.
// ---------------------------------------------------------------------------

/// MMC4 (Mapper 10).
pub struct Mmc4 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_lo_fd: u8,
    chr_lo_fe: u8,
    chr_hi_fd: u8,
    chr_hi_fe: u8,
    latch_lo_fe: bool,
    latch_hi_fe: bool,
    mirroring: Mirroring,
    /// 8 KiB WRAM at $6000-$7FFF (battery-backed on most MMC4 carts).
    /// T-60-003c (2026-05-17) — same root cause as the VRC2/4/6 WRAM
    /// fix in `sprint3.rs`. Fire Emblem Gaiden was stuck-at-uniform-
    /// gray for the same reason (read its save magic from WRAM at
    /// boot, got 0, stalled in save-validation).
    prg_ram: Box<[u8]>,
}

impl Mmc4 {
    /// Construct a new MMC4 mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || prg_rom.len() % PRG_BANK_16K != 0 {
            return Err(MapperError::Invalid(format!(
                "MMC4 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len() % CHR_BANK_4K == 0 {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "MMC4 CHR-ROM size {} is not a multiple of 4 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_lo_fd: 0,
            chr_lo_fe: 0,
            chr_hi_fd: 0,
            chr_hi_fe: 0,
            latch_lo_fe: false,
            latch_hi_fe: false,
            mirroring,
            // 8 KiB WRAM at $6000-$7FFF (T-60-003c).
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
        })
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total_16k = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let last = total_16k - 1;
        let bank = if (addr & 0xC000) == 0x8000 {
            (self.prg_bank as usize) % total_16k
        } else {
            last
        };
        bank * PRG_BANK_16K + ((addr as usize) & 0x3FFF)
    }

    fn chr_offset(&mut self, addr: u16) -> usize {
        let addr = (addr & 0x1FFF) as usize;
        let total_4k = (self.chr_rom.len() / CHR_BANK_4K).max(1);
        let bank = if addr < CHR_BANK_4K {
            let b = if self.latch_lo_fe {
                self.chr_lo_fe
            } else {
                self.chr_lo_fd
            };
            (b as usize) % total_4k
        } else {
            let b = if self.latch_hi_fe {
                self.chr_hi_fe
            } else {
                self.chr_hi_fd
            };
            (b as usize) % total_4k
        };
        bank * CHR_BANK_4K + (addr & (CHR_BANK_4K - 1))
    }

    fn update_latch(&mut self, addr: u16) {
        match addr & 0x3FF8 {
            0x0FD8 => self.latch_lo_fe = false,
            0x0FE8 => self.latch_lo_fe = true,
            0x1FD8 => self.latch_hi_fe = false,
            0x1FE8 => self.latch_hi_fe = true,
            _ => {}
        }
    }
}

impl Mapper for Mmc4 {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // T-60-003c (2026-05-17): MMC4 carts (Fire Emblem proper +
            // Fire Emblem Gaiden + Famicom Wars) include 8 KiB battery-
            // backed WRAM at $6000-$7FFF. Pre-fix returned 0; FE
            // Gaiden's save-validation path stalled. Same root cause
            // as the VRC2/4/6 fix in sprint3.rs.
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize % self.prg_ram.len()],
            0x8000..=0xFFFF => {
                let off = self.prg_offset(addr);
                self.prg_rom[off % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // T-60-003c (2026-05-17): WRAM at $6000-$7FFF (paired with
        // the read fix above).
        if (0x6000..=0x7FFF).contains(&addr) {
            let len = self.prg_ram.len();
            self.prg_ram[(addr - 0x6000) as usize % len] = value;
            return;
        }
        match addr & 0xF000 {
            0xA000 => self.prg_bank = value & 0x0F,
            0xB000 => self.chr_lo_fd = value & 0x1F,
            0xC000 => self.chr_lo_fe = value & 0x1F,
            0xD000 => self.chr_hi_fd = value & 0x1F,
            0xE000 => self.chr_hi_fe = value & 0x1F,
            0xF000 => {
                self.mirroring = if value & 1 == 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
                let v = self.chr_rom[off % self.chr_rom.len()];
                self.update_latch(addr);
                v
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

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.vram.len());
        out.push(1u8);
        out.push(self.prg_bank);
        out.push(self.chr_lo_fd);
        out.push(self.chr_lo_fe);
        out.push(self.chr_hi_fd);
        out.push(self.chr_hi_fe);
        out.push(u8::from(self.latch_lo_fe));
        out.push(u8::from(self.latch_hi_fe));
        out.push(self.mirroring as u8);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 9 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != 1 {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank = data[1];
        self.chr_lo_fd = data[2];
        self.chr_lo_fe = data[3];
        self.chr_hi_fd = data[4];
        self.chr_hi_fe = data[5];
        self.latch_lo_fe = data[6] != 0;
        self.latch_hi_fe = data[7] != 0;
        self.mirroring = match data[8] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.vram.copy_from_slice(&data[9..9 + self.vram.len()]);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Color Dreams (mapper 11) — bank: bits 0-1 = PRG (32K units), bits 4-7 = CHR.
// ---------------------------------------------------------------------------

/// Color Dreams (Mapper 11).
pub struct ColorDreams {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl ColorDreams {
    /// Construct a new Color Dreams mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || prg_rom.len() % (32 * 1024) != 0 {
            return Err(MapperError::Invalid(format!(
                "Color Dreams PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len() % CHR_BANK_8K == 0 {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Color Dreams CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }
}

impl Mapper for ColorDreams {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if addr < 0x8000 {
            return 0;
        }
        let total_32k = (self.prg_rom.len() / (32 * 1024)).max(1);
        let bank = (self.prg_bank as usize) % total_32k;
        let off = bank * 32 * 1024 + (addr as usize - 0x8000);
        self.prg_rom[off % self.prg_rom.len()]
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            // Bus conflict: AND with the current PRG byte at this address.
            let conflict = self.cpu_read(addr);
            let v = value & conflict;
            self.prg_bank = v & 0x03;
            self.chr_bank = (v >> 4) & 0x0F;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let total_8k = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % total_8k;
                self.chr_rom[(bank * CHR_BANK_8K + addr as usize) % self.chr_rom.len()]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring) % self.vram.len()],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.mirroring) % self.vram.len();
            self.vram[off] = value;
        } else if (0x0000..=0x1FFF).contains(&addr) && self.chr_is_ram {
            let total_8k = (self.chr_rom.len() / CHR_BANK_8K).max(1);
            let bank = (self.chr_bank as usize) % total_8k;
            let off = (bank * CHR_BANK_8K + addr as usize) % self.chr_rom.len();
            self.chr_rom[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.vram.len());
        out.push(1u8);
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
        if data[0] != 1 {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank = data[1];
        self.chr_bank = data[2];
        self.vram.copy_from_slice(&data[3..3 + self.vram.len()]);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CPROM (mapper 13) — Videomation: 4 KiB CHR-RAM banked at $1000-$1FFF;
// $0000-$0FFF fixed to bank 0 of CHR-RAM.  Full 32 KiB PRG fixed.
// ---------------------------------------------------------------------------

/// CPROM (Mapper 13).
pub struct Cprom {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>, // 16 KiB total: 4 banks of 4 KiB.
    vram: Box<[u8]>,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Cprom {
    /// Construct a new CPROM mapper (NES Time Lord uses this).
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(prg_rom: Box<[u8]>, mirroring: Mirroring) -> Result<Self, MapperError> {
        if prg_rom.len() != 32 * 1024 {
            return Err(MapperError::Invalid(format!(
                "CPROM expects 32 KiB PRG, got {} bytes",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; 16 * 1024].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_bank: 0,
            mirroring,
        })
    }
}

impl Mapper for Cprom {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if addr < 0x8000 {
            return 0;
        }
        self.prg_rom[(addr as usize - 0x8000) % self.prg_rom.len()]
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            self.chr_bank = value & 0x03;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x0FFF => self.chr_ram[addr as usize],
            0x1000..=0x1FFF => {
                let bank = (self.chr_bank as usize) & 0x03;
                let off = bank * CHR_BANK_4K + (addr as usize - 0x1000);
                self.chr_ram[off % self.chr_ram.len()]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring) % self.vram.len()],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x0FFF => self.chr_ram[addr as usize] = value,
            0x1000..=0x1FFF => {
                let bank = (self.chr_bank as usize) & 0x03;
                let off = (bank * CHR_BANK_4K + (addr as usize - 0x1000)) % self.chr_ram.len();
                self.chr_ram[off] = value;
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring) % self.vram.len();
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + self.chr_ram.len() + self.vram.len());
        out.push(1u8);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.chr_ram);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 2 + self.chr_ram.len() + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != 1 {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.chr_bank = data[1];
        self.chr_ram
            .copy_from_slice(&data[2..2 + self.chr_ram.len()]);
        let off = 2 + self.chr_ram.len();
        self.vram.copy_from_slice(&data[off..off + self.vram.len()]);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// BNROM / NINA-001 (mapper 34) — BNROM: 32 KiB PRG bank.
// NINA-001 (submapper 1): different register layout with extra CHR banks.
// We default to BNROM; NES 2.0 submapper 1 selects NINA-001.
// ---------------------------------------------------------------------------

/// Mapper 34 variant.
#[derive(Debug, Clone, Copy)]
pub enum M34Variant {
    /// BNROM: PRG-bank-only, no CHR banking.
    Bnrom,
    /// NINA-001: PRG bank @ $7FFD, CHR banks @ $7FFE / $7FFF.
    Nina001,
}

/// Mapper 34 (BNROM / NINA-001).
pub struct M34 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    prg_ram: Box<[u8]>,
    prg_bank: u8,
    chr_bank_lo: u8,
    chr_bank_hi: u8,
    variant: M34Variant,
    mirroring: Mirroring,
}

impl M34 {
    /// Construct a new M34 mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        variant: M34Variant,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || prg_rom.len() % (32 * 1024) != 0 {
            return Err(MapperError::Invalid(format!(
                "Mapper 34 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len() % CHR_BANK_4K == 0 {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Mapper 34 CHR-ROM size {} is not a multiple of 4 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
            prg_bank: 0,
            chr_bank_lo: 0,
            chr_bank_hi: 0,
            variant,
            mirroring,
        })
    }
}

impl Mapper for M34 {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize % self.prg_ram.len()],
            0x8000..=0xFFFF => {
                let total_32k = (self.prg_rom.len() / (32 * 1024)).max(1);
                let bank = (self.prg_bank as usize) % total_32k;
                self.prg_rom[(bank * 32 * 1024 + (addr as usize - 0x8000)) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match (self.variant, addr) {
            (M34Variant::Nina001, 0x7FFD) => self.prg_bank = value & 0x01,
            (M34Variant::Nina001, 0x7FFE) => self.chr_bank_lo = value & 0x0F,
            (M34Variant::Nina001, 0x7FFF) => self.chr_bank_hi = value & 0x0F,
            (_, 0x6000..=0x7FFF) => {
                let off = (addr - 0x6000) as usize % self.prg_ram.len();
                self.prg_ram[off] = value;
            }
            (M34Variant::Bnrom, 0x8000..=0xFFFF) => self.prg_bank = value,
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match (addr, self.variant) {
            (0x0000..=0x0FFF, M34Variant::Nina001) => {
                let total_4k = (self.chr.len() / CHR_BANK_4K).max(1);
                let bank = (self.chr_bank_lo as usize) % total_4k;
                self.chr[(bank * CHR_BANK_4K + addr as usize) % self.chr.len()]
            }
            (0x1000..=0x1FFF, M34Variant::Nina001) => {
                let total_4k = (self.chr.len() / CHR_BANK_4K).max(1);
                let bank = (self.chr_bank_hi as usize) % total_4k;
                self.chr[(bank * CHR_BANK_4K + (addr as usize - 0x1000)) % self.chr.len()]
            }
            (0x0000..=0x1FFF, _) => self.chr[addr as usize % self.chr.len()],
            (0x2000..=0x3EFF, _) => {
                self.vram[nametable_offset(addr, self.mirroring) % self.vram.len()]
            }
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let len = self.chr.len();
                    self.chr[addr as usize % len] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring) % self.vram.len();
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.prg_ram.len() + self.vram.len());
        out.push(1u8);
        out.push(self.prg_bank);
        out.push(self.chr_bank_lo);
        out.push(self.chr_bank_hi);
        out.push(match self.variant {
            M34Variant::Bnrom => 0,
            M34Variant::Nina001 => 1,
        });
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 5 + self.prg_ram.len() + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != 1 {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank = data[1];
        self.chr_bank_lo = data[2];
        self.chr_bank_hi = data[3];
        self.variant = match data[4] {
            0 => M34Variant::Bnrom,
            1 => M34Variant::Nina001,
            other => return Err(MapperError::Invalid(format!("variant {other}"))),
        };
        let mut cur = 5usize;
        self.prg_ram
            .copy_from_slice(&data[cur..cur + self.prg_ram.len()]);
        cur += self.prg_ram.len();
        self.vram.copy_from_slice(&data[cur..cur + self.vram.len()]);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Camerica BF9093 (mapper 71) — write-anywhere PRG bank @ $C000-$FFFF or $8000+.
// Some boards (subm 1) have mirroring control via $9000.
// ---------------------------------------------------------------------------

/// Camerica / Codemasters BF9093 (Mapper 71).
pub struct Camerica {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    mirroring: Mirroring,
    has_single_screen: bool,
}

impl Camerica {
    /// Construct a new Camerica mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        mirroring: Mirroring,
        has_single_screen: bool,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || prg_rom.len() % PRG_BANK_16K != 0 {
            return Err(MapperError::Invalid(format!(
                "Camerica PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            mirroring,
            has_single_screen,
        })
    }
}

impl Mapper for Camerica {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let total_16k = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let last = total_16k - 1;
        match addr {
            0x8000..=0xBFFF => {
                let bank = (self.prg_bank as usize) % total_16k;
                self.prg_rom[(bank * PRG_BANK_16K + (addr as usize - 0x8000)) % self.prg_rom.len()]
            }
            0xC000..=0xFFFF => {
                self.prg_rom[(last * PRG_BANK_16K + (addr as usize - 0xC000)) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x9000..=0x9FFF if self.has_single_screen => {
                self.mirroring = if value & 0x10 == 0 {
                    Mirroring::SingleScreenA
                } else {
                    Mirroring::SingleScreenB
                };
            }
            0xC000..=0xFFFF | 0x8000..=0xBFFF => self.prg_bank = value & 0x0F,
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize % self.chr_ram.len()],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring) % self.vram.len()],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let len = self.chr_ram.len();
                self.chr_ram[addr as usize % len] = value;
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring) % self.vram.len();
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.vram.len() + self.chr_ram.len());
        out.push(1u8);
        out.push(self.prg_bank);
        out.push(self.mirroring as u8);
        out.push(u8::from(self.has_single_screen));
        out.extend_from_slice(&self.chr_ram);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 4 + self.chr_ram.len() + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != 1 {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank = data[1];
        self.mirroring = match data[2] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.has_single_screen = data[3] != 0;
        let mut cur = 4usize;
        self.chr_ram
            .copy_from_slice(&data[cur..cur + self.chr_ram.len()]);
        cur += self.chr_ram.len();
        self.vram.copy_from_slice(&data[cur..cur + self.vram.len()]);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// VRC1 (mapper 75) — Konami's earliest VRC.  Three 8 KiB switchable PRG
// banks ($8000, $A000, $C000) + fixed last bank.  CHR is two 4 KiB
// switchable windows.  Mirroring + extra CHR-MSB bit via $9000.
// ---------------------------------------------------------------------------

/// VRC1 (Mapper 75).
pub struct Vrc1 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_banks: [u8; 3], // $8000, $A000, $C000
    chr_lo: u8,
    chr_hi: u8,
    chr_lo_msb: u8,
    chr_hi_msb: u8,
    mirroring: Mirroring,
}

impl Vrc1 {
    /// Construct a new VRC1 mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || prg_rom.len() % PRG_BANK_8K != 0 {
            return Err(MapperError::Invalid(format!(
                "VRC1 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len() % CHR_BANK_4K == 0 {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "VRC1 CHR-ROM size {} is not a multiple of 4 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_banks: [0, 1, 2],
            chr_lo: 0,
            chr_hi: 0,
            chr_lo_msb: 0,
            chr_hi_msb: 0,
            mirroring,
        })
    }
}

impl Mapper for Vrc1 {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let total_8k = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let last = total_8k - 1;
        let bank = match addr & 0xE000 {
            0x8000 => (self.prg_banks[0] as usize) % total_8k,
            0xA000 => (self.prg_banks[1] as usize) % total_8k,
            0xC000 => (self.prg_banks[2] as usize) % total_8k,
            0xE000 => last,
            _ => return 0,
        };
        self.prg_rom[(bank * PRG_BANK_8K + (addr as usize & 0x1FFF)) % self.prg_rom.len()]
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr & 0xF000 {
            0x8000 => self.prg_banks[0] = value & 0x0F,
            0x9000 => {
                // Mirroring (bit 0) + CHR MSB bits.
                self.mirroring = if value & 1 == 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
                self.chr_lo_msb = (value >> 1) & 1;
                self.chr_hi_msb = (value >> 2) & 1;
            }
            0xA000 => self.prg_banks[1] = value & 0x0F,
            0xC000 => self.prg_banks[2] = value & 0x0F,
            0xE000 => self.chr_lo = value & 0x0F,
            0xF000 => self.chr_hi = value & 0x0F,
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x0FFF => {
                let total_4k = (self.chr_rom.len() / CHR_BANK_4K).max(1);
                let bank = (((self.chr_lo_msb as usize) << 4) | (self.chr_lo as usize)) % total_4k;
                self.chr_rom[(bank * CHR_BANK_4K + addr as usize) % self.chr_rom.len()]
            }
            0x1000..=0x1FFF => {
                let total_4k = (self.chr_rom.len() / CHR_BANK_4K).max(1);
                let bank = (((self.chr_hi_msb as usize) << 4) | (self.chr_hi as usize)) % total_4k;
                self.chr_rom[(bank * CHR_BANK_4K + (addr as usize - 0x1000)) % self.chr_rom.len()]
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

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.vram.len());
        out.push(1u8);
        out.extend_from_slice(&self.prg_banks);
        out.push(self.chr_lo);
        out.push(self.chr_hi);
        out.push(self.chr_lo_msb);
        out.push(self.chr_hi_msb);
        out.push(self.mirroring as u8);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 9 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != 1 {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_banks.copy_from_slice(&data[1..4]);
        self.chr_lo = data[4];
        self.chr_hi = data[5];
        self.chr_lo_msb = data[6];
        self.chr_hi_msb = data[7];
        self.mirroring = match data[8] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.vram.copy_from_slice(&data[9..9 + self.vram.len()]);
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

    fn synth_chr_4k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_4K];
        for b in 0..banks {
            v[b * CHR_BANK_4K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn mmc2_swap_window_via_latch() {
        let mut m = Mmc2::new(synth(8), synth_chr_4k(4), Mirroring::Vertical).unwrap();
        m.chr_lo_fd = 0;
        m.chr_lo_fe = 1;
        // Default latch is FD -> bank 0 byte 0 = 0.
        assert_eq!(m.ppu_read(0x0000), 0);
        // Reading the FE sentinel switches to FE bank.
        let _ = m.ppu_read(0x0FE8);
        assert_eq!(m.ppu_read(0x0000), 1);
    }

    #[test]
    fn color_dreams_bus_conflict() {
        let mut prg = vec![0u8; 32 * 1024];
        // Make ROM byte at $8000 = 0x55 -> AND with 0xFF gives 0x55.
        prg[0] = 0x55;
        let m_prg: Box<[u8]> = prg.into_boxed_slice();
        let chr = vec![0u8; 8 * 1024].into_boxed_slice();
        let mut m = ColorDreams::new(m_prg, chr, Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0xFF);
        // Effective value = 0xFF & 0x55 = 0x55. PRG bank = 0x55 & 0x03 = 1.
        assert_eq!(m.prg_bank, 1);
    }

    #[test]
    fn cprom_chr_bank_select() {
        let mut m =
            Cprom::new(vec![0u8; 32 * 1024].into_boxed_slice(), Mirroring::Vertical).unwrap();
        m.ppu_write(0x1000, 0xAA); // bank 0
        m.cpu_write(0x8000, 1);
        m.ppu_write(0x1000, 0xBB); // bank 1
        m.cpu_write(0x8000, 0);
        assert_eq!(m.ppu_read(0x1000), 0xAA);
        m.cpu_write(0x8000, 1);
        assert_eq!(m.ppu_read(0x1000), 0xBB);
    }

    #[test]
    fn camerica_bank_swap() {
        let mut m = Camerica::new(synth(8 * 2), Mirroring::Vertical, false).unwrap();
        // Default: bank 0 at $8000.
        assert_eq!(m.cpu_read(0x8000), 0);
        m.cpu_write(0xC000, 5);
        // Bank 5 (16K bank index, but we have 16K chunks — total_16k = 16).
        // bank 5 at 16K offset. Let's just check it swaps from 0.
        assert_ne!(m.cpu_read(0x8000), 0);
    }

    #[test]
    fn vrc1_basic_banking() {
        let mut m = Vrc1::new(synth(8), synth_chr_4k(2), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        // $E000 is fixed last bank.
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn m34_bnrom_swap() {
        let mut m = M34::new(
            synth(8),
            Box::new([]),
            Mirroring::Vertical,
            M34Variant::Bnrom,
        )
        .unwrap();
        // Default bank 0; $8000 -> 0.
        assert_eq!(m.cpu_read(0x8000), 0);
        // Test write with conflict; 32K banks here means bank index 1 -> byte at offset 32K = bank 4 of 8K banks.
        m.cpu_write(0x8000, 1);
        // Bank 1 in 32K terms = offset 32K. PRG[32768] = byte 4 of synth(8) = 4.
        assert_eq!(m.cpu_read(0x8000), 4);
    }

    #[test]
    fn m34_nina001_variant_register_layout() {
        // T-74-001 (Phase 7): NINA-001 (mapper 34 submapper 1) uses a distinct
        // register layout from BNROM — PRG bank at $7FFD, CHR lo/hi at
        // $7FFE/$7FFF — and must NOT respond to BNROM's $8000 PRG-bank write.
        let mut m = M34::new(
            synth(8),
            synth_chr_4k(8),
            Mirroring::Vertical,
            M34Variant::Nina001,
        )
        .unwrap();
        // PRG bank via $7FFD (1-bit). Bank 1 = 32K offset = 8K-bank 4 = byte 4.
        m.cpu_write(0x7FFD, 1);
        assert_eq!(m.cpu_read(0x8000), 4, "NINA-001 PRG bank selects via $7FFD");
        // A BNROM-style $8000 write must be ignored on NINA-001.
        m.cpu_write(0x8000, 0);
        assert_eq!(m.cpu_read(0x8000), 4, "$8000 write is ignored on NINA-001");
        // CHR lo/hi banks via $7FFE / $7FFF (each tagged with its index byte).
        m.cpu_write(0x7FFE, 2);
        assert_eq!(m.ppu_read(0x0000), 2, "NINA-001 CHR lo bank via $7FFE");
        m.cpu_write(0x7FFF, 3);
        assert_eq!(m.ppu_read(0x1000), 3, "NINA-001 CHR hi bank via $7FFF");
    }
}

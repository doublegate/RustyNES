// SPDX-License-Identifier: GPL-3.0-or-later
//! Mapper 116: Gouder's SOMARI-P board (the Huang-1 / Huang-2 ASIC and a
//! PAL), which simulates three different mapper chips and switches between
//! them at run time (v3.2.0).
//!
//! **Provenance.** Written from the vendored `INES_Mapper_116.md`, and from
//! the pages of the three chips it simulates: `VRC2_and_VRC4.md` (the VRC2b
//! wiring, A0/A1), `MMC3.md` and `MMC1.md`. The MMC3 and MMC1 are the
//! project's own [`Mmc3`] and [`Mmc1`], used as register files; neither file
//! carries a provenance header. No reference-emulator source was read.
//!
//! ## The board, per the page
//!
//! - **Supervisor register** at `$4100` (mask `$E100`, write-only):
//!   `.... .CMM`. `MM` selects the simulated chip (0: VRC2, 1: MMC3, 2 and 3:
//!   MMC1); `C` is CHR A18, an outer 256 KiB CHR bank that applies in every
//!   mode.
//! - **Separate state per chip.** "All three simulated chips have their
//!   separate registers, so that when switching between modes ... the entire
//!   mapper state changes including PRG/CHR ROM switchable and fixed banks as
//!   well as IRQ state." So each chip keeps its own registers here, only the
//!   selected one receives `$8000-$FFFF` writes, and only the selected one
//!   drives the banks, the mirroring and the IRQ line.
//! - **VRC2 (VRC2b wiring):** PRG select 0 at `$8xxx`, mirroring at `$9xxx`,
//!   PRG select 1 at `$Axxx` (the page: *Garō Densetsu Special* writes to
//!   mirrors of all three, so each answers in its whole 4 KiB); 1 KiB CHR
//!   selects at `$B000-$E003` decoded by A12 and A0/A1, low nibble on A0 = 0
//!   and the VRC2's four high bits on A0 = 1; `$C000-$FFFF` fixed to the last
//!   16 KiB. "The VRC2 CHR-ROM registers are initialized on power-up to
//!   `$FF`", which *Somari*'s original release relies on.
//! - **MMC3:** a stock MMC3, with its A12 IRQ counter.
//! - **MMC1:** a stock MMC1. "When switching to MMC1 mode, the serial shift
//!   register is reset" (*Garō Densetsu Special* relies on it).
//! - **Huang-2:** the MMC1 PRG bank register is "shifted by one bit to the
//!   left" relative to a normal MMC1. No submapper exists for it; the page's
//!   heuristic (PRG-ROM and CHR-ROM each exactly 128 KiB) selects it.
//!
//! ## Choices the page leaves open
//!
//! - The supervisor's power-on value is not given. It is 0 (VRC2 mode): the
//!   page's own note that *Somari* depends on the VRC2 CHR registers'
//!   power-up value only makes sense if the board starts in VRC2 mode.
//! - The inactive chips keep receiving the A12 and CPU-cycle clocks (they are
//!   one ASIC); only their outputs are deselected.
//! - The VRC2's `$6000-$6FFF` microwire latch is not modelled; nothing on the
//!   page suggests these games use it, and the board has no WRAM.
//! - Submapper 3's reset-switched 5-in-1 (named on the page in one sentence,
//!   with no register description) is not modelled.

#![allow(clippy::cast_possible_truncation, clippy::doc_markdown)]

use crate::cartridge::Mirroring;
use crate::m001_mmc1::Mmc1;
use crate::m004_mmc3::{Mmc3, Mmc3Revision};
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError};
use crate::mmc3_boards::mirror_bank;
use alloc::string::ToString;
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_8K: usize = 0x2000;
const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_1K: usize = 0x0400;
const VRAM_SIZE: usize = 0x0800;

/// Save-state layout version for [`Somari116`].
const SAVE_STATE_VERSION: u8 = 1;

/// The VRC2 CHR registers' power-up value (`INES_Mapper_116.md`).
const VRC2_CHR_POWER_UP: u8 = 0xFF;

/// Which chip the supervisor register selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Vrc2,
    Mmc3,
    Mmc1,
}

/// Mapper 116 (SOMARI-P, Huang-1 / Huang-2). See the module docs.
pub struct Somari116 {
    prg_rom: Box<[u8]>,
    /// CHR-ROM, or 8 KiB of CHR-RAM when the image has none.
    chr: Box<[u8]>,
    chr_is_ram: bool,
    /// The console's 2 KiB of nametable RAM, mapped by the active chip's
    /// mirroring.
    vram: Box<[u8]>,
    /// The `$4100` supervisor register.
    supervisor: u8,
    /// The Huang-2 ASIC: the MMC1 PRG register is shifted left by one.
    huang2: bool,
    /// VRC2 PRG selects (`$8xxx`, `$Axxx`), 5 bits.
    vrc2_prg: [u8; 2],
    /// VRC2 mirroring (`$9xxx` bit 0: 0 vertical, 1 horizontal).
    vrc2_mirroring: u8,
    /// VRC2 1 KiB CHR selects, 8 bits each (4 low + the VRC2's 4 high).
    vrc2_chr: [u8; 8],
    /// The simulated MMC3: register file and A12 IRQ counter.
    mmc3: Mmc3,
    /// The simulated MMC1: serial port and register file.
    mmc1: Mmc1,
}

impl Somari116 {
    /// Construct the board. The Huang-2 is selected by the page's size
    /// heuristic (PRG-ROM and CHR-ROM both 128 KiB).
    ///
    /// # Errors
    ///
    /// [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR is not a multiple of 1 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 116 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if !chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            return Err(MapperError::Invalid(format!(
                "mapper 116 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        }
        let huang2 = prg_rom.len() == 128 * 1024 && chr_rom.len() == 128 * 1024;
        let chr_is_ram = chr_rom.is_empty();
        let chr = if chr_is_ram {
            vec![0u8; 8 * CHR_BANK_1K].into_boxed_slice()
        } else {
            chr_rom
        };
        // The MMC1 is a register file only: it is handed a minimal ROM it
        // never reads, and no PRG-RAM is mapped (the board has none).
        let mmc1 = Mmc1::new(
            vec![0u8; 2 * PRG_BANK_16K].into_boxed_slice(),
            Box::new([]),
            mirroring,
            0,
        )?;
        Ok(Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; VRAM_SIZE].into_boxed_slice(),
            supervisor: 0,
            huang2,
            vrc2_prg: [0; 2],
            vrc2_mirroring: 0,
            vrc2_chr: [VRC2_CHR_POWER_UP; 8],
            mmc3: Mmc3::register_core(mirroring, Mmc3Revision::Sharp),
            mmc1,
        })
    }

    const fn mode(&self) -> Mode {
        match self.supervisor & 3 {
            0 => Mode::Vrc2,
            1 => Mode::Mmc3,
            _ => Mode::Mmc1,
        }
    }

    /// The 8 KiB PRG bank at `addr` (`$8000-$FFFF`), before the mirror.
    fn prg_bank(&self, addr: u16) -> usize {
        let a13 = usize::from((addr >> 13) & 1);
        match self.mode() {
            Mode::Vrc2 => match addr & 0xE000 {
                0x8000 => usize::from(self.vrc2_prg[0] & 0x1F),
                0xA000 => usize::from(self.vrc2_prg[1] & 0x1F),
                // The last 16 KiB: the chip's all-ones bank pair.
                0xC000 => 0x1E,
                _ => 0x1F,
            },
            Mode::Mmc3 => usize::from(self.mmc3.prg_bank_raw(addr)),
            Mode::Mmc1 => {
                let (control, _, _, prg) = self.mmc1.registers();
                // Huang-2: the game writes the bank one bit to the left.
                let bank = usize::from(if self.huang2 { prg >> 1 } else { prg }) & 0x0F;
                let a14 = usize::from((addr >> 14) & 1);
                let bank16 = match (control >> 2) & 3 {
                    0 | 1 => (bank & !1) | a14,
                    2 if a14 == 0 => 0,
                    3 if a14 == 1 => 0x0F,
                    _ => bank,
                };
                (bank16 << 1) | a13
            }
        }
    }

    /// The 1 KiB CHR bank at a pattern-table address, before the mirror,
    /// with the supervisor's CHR A18 on top.
    fn chr_bank(&self, addr: u16) -> usize {
        let addr = addr & 0x1FFF;
        let inner = match self.mode() {
            Mode::Vrc2 => usize::from(self.vrc2_chr[usize::from(addr >> 10)]),
            Mode::Mmc3 => self.mmc3.chr_bank_1k(addr) & 0xFF,
            Mode::Mmc1 => {
                let (control, chr0, chr1, _) = self.mmc1.registers();
                let a12 = addr & 0x1000 != 0;
                let bank4k = if control & 0x10 == 0 {
                    // 8 KiB mode: the low bit of CHR bank 0 is ignored.
                    (chr0 & 0x1E) | u8::from(a12)
                } else if a12 {
                    chr1
                } else {
                    chr0
                };
                (usize::from(bank4k & 0x1F) << 2) | usize::from((addr >> 10) & 3)
            }
        };
        inner | (usize::from((self.supervisor >> 2) & 1) << 8)
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let bank = mirror_bank(self.chr_bank(addr), self.chr.len() / CHR_BANK_1K);
        bank * CHR_BANK_1K + (usize::from(addr) & (CHR_BANK_1K - 1))
    }

    fn vram_offset(&self, addr: u16) -> usize {
        usize::from(self.nametable_address(addr)) & (VRAM_SIZE - 1)
    }

    /// A VRC2 register write (`$8000-$FFFF` in VRC2 mode).
    fn vrc2_write(&mut self, addr: u16, value: u8) {
        match addr & 0xF000 {
            0x8000 => self.vrc2_prg[0] = value & 0x1F,
            0x9000 => self.vrc2_mirroring = value & 1,
            0xA000 => self.vrc2_prg[1] = value & 0x1F,
            0xB000..=0xE000 => {
                // Two selects per 4 KiB page, picked by A1; A0 picks the
                // nibble. "VRC2 only has 4 high bits of CHR select."
                let page = usize::from((addr >> 12) - 0xB);
                let slot = page * 2 + usize::from((addr >> 1) & 1);
                let old = self.vrc2_chr[slot];
                self.vrc2_chr[slot] = if addr & 1 == 0 {
                    (old & 0xF0) | (value & 0x0F)
                } else {
                    (old & 0x0F) | ((value & 0x0F) << 4)
                };
            }
            _ => {}
        }
    }
}

impl Mapper for Somari116 {
    fn caps(&self) -> MapperCaps {
        // The MMC3's A12 filter and the MMC1's consecutive-write check both
        // count CPU cycles; the MMC3 is an IRQ source.
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        addr < 0x8000
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if addr < 0x8000 {
            return 0;
        }
        let bank = mirror_bank(self.prg_bank(addr), self.prg_rom.len() / PRG_BANK_8K);
        self.prg_rom[bank * PRG_BANK_8K + (usize::from(addr) & 0x1FFF)]
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x4020..=0x5FFF if addr & 0xE100 == 0x4100 => {
                let was_mmc1 = self.mode() == Mode::Mmc1;
                self.supervisor = value & 0x07;
                if !was_mmc1 && self.mode() == Mode::Mmc1 {
                    self.mmc1.reset_shift_register();
                }
            }
            0x8000..=0xFFFF => match self.mode() {
                Mode::Vrc2 => self.vrc2_write(addr, value),
                Mode::Mmc3 => self.mmc3.cpu_write(addr, value),
                Mode::Mmc1 => self.mmc1.cpu_write(addr, value),
            },
            _ => {}
        }
    }

    fn chr_phys(&self, addr: u16) -> Option<u32> {
        if self.chr_is_ram {
            None
        } else {
            u32::try_from(self.chr_offset(addr & 0x1FFF)).ok()
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[self.vram_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_peek(&self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[self.vram_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                let off = self.chr_offset(addr);
                self.chr[off] = value;
            }
            0x2000..=0x3EFF => {
                let off = self.vram_offset(addr);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        match self.mode() {
            Mode::Vrc2 if self.vrc2_mirroring == 0 => Mirroring::Vertical,
            Mode::Vrc2 => Mirroring::Horizontal,
            Mode::Mmc3 => self.mmc3.current_mirroring(),
            Mode::Mmc1 => self.mmc1.current_mirroring(),
        }
    }

    fn notify_a12(&mut self, level: bool) {
        self.mmc3.notify_a12(level);
        self.mmc1.notify_a12(level);
    }

    fn notify_a12_at_sub_dot(&mut self, level: bool, sub_dot: u8) {
        self.mmc3.notify_a12_at_sub_dot(level, sub_dot);
        self.mmc1.notify_a12(level);
    }

    fn notify_cpu_cycle(&mut self) {
        self.mmc3.notify_cpu_cycle();
        self.mmc1.notify_cpu_cycle();
    }

    fn irq_pending(&self) -> bool {
        // Only the MMC3 has an IRQ, and only the selected chip drives the
        // line.
        self.mode() == Mode::Mmc3 && self.mmc3.irq_pending()
    }

    fn irq_acknowledge(&mut self) {
        self.mmc3.irq_acknowledge();
    }

    fn debug_info(&self) -> MapperDebugInfo {
        let mut info = match self.mode() {
            Mode::Mmc3 => self.mmc3.debug_info(),
            Mode::Mmc1 => self.mmc1.debug_info(),
            Mode::Vrc2 => {
                let mut info = MapperDebugInfo::default();
                for (i, p) in self.vrc2_prg.iter().enumerate() {
                    info.prg_banks
                        .push((format!("PRG{i}"), format!("{p:#04x}")));
                }
                for (i, c) in self.vrc2_chr.iter().enumerate() {
                    info.chr_banks
                        .push((format!("CHR{i}"), format!("{c:#04x}")));
                }
                info
            }
        };
        info.mapper_id = 116;
        info.name = if self.huang2 {
            "SOMARI-P Huang-2 (116)"
        } else {
            "SOMARI-P Huang-1 (116)"
        }
        .to_string();
        info.mirroring = crate::mapper::mirroring_name(self.current_mirroring());
        info.extra.push((
            "supervisor".to_string(),
            format!("{:#04x}", self.supervisor),
        ));
        info.extra
            .push(("mode".to_string(), format!("{:?}", self.mode())));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mmc3 = self.mmc3.save_state();
        let mmc1 = self.mmc1.save_state();
        let chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(HEAD + mmc3.len() + mmc1.len() + chr + VRAM_SIZE);
        out.push(SAVE_STATE_VERSION);
        out.push(self.supervisor);
        out.extend_from_slice(&self.vrc2_prg);
        out.push(self.vrc2_mirroring);
        out.extend_from_slice(&self.vrc2_chr);
        out.extend_from_slice(&(mmc3.len() as u32).to_le_bytes());
        out.extend_from_slice(&(mmc1.len() as u32).to_le_bytes());
        out.extend_from_slice(&mmc3);
        out.extend_from_slice(&mmc1);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        if data.len() < HEAD {
            return Err(MapperError::WrongLength {
                expected: HEAD,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let len_at = |i: usize| {
            u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize
        };
        let (mmc3_len, mmc1_len) = (len_at(13), len_at(17));
        let chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        // Checked: both lengths come from the blob, and a plain sum can wrap
        // on a 32-bit target, pass the check, and panic at the slice.
        let expected = HEAD
            .checked_add(mmc3_len)
            .and_then(|n| n.checked_add(mmc1_len))
            .and_then(|n| n.checked_add(chr + VRAM_SIZE));
        if expected != Some(data.len()) {
            return Err(MapperError::WrongLength {
                expected: expected.unwrap_or(usize::MAX),
                got: data.len(),
            });
        }
        let mut cur = HEAD;
        self.mmc3.load_state(&data[cur..cur + mmc3_len])?;
        cur += mmc3_len;
        self.mmc1.load_state(&data[cur..cur + mmc1_len])?;
        cur += mmc1_len;
        self.supervisor = data[1] & 0x07;
        self.vrc2_prg = [data[2] & 0x1F, data[3] & 0x1F];
        self.vrc2_mirroring = data[4] & 1;
        self.vrc2_chr.copy_from_slice(&data[5..13]);
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[cur..cur + chr]);
            cur += chr;
        }
        self.vram.copy_from_slice(&data[cur..]);
        Ok(())
    }
}

/// Fixed head of the save state: version, supervisor, two VRC2 PRG selects,
/// VRC2 mirroring, eight VRC2 CHR selects, and the two embedded chip states'
/// lengths.
const HEAD: usize = 1 + 1 + 2 + 1 + 8 + 4 + 4;

#[cfg(test)]
#[path = "m116_somari_tests.rs"]
mod tests;

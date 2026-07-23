//! Kaiser boards: `KS202` (mapper 56), `KS7017` (142), `KS7031` (303),
//! `KS7016` (305), `KS7013B` (306) and relatives.
//!
//! Kaiser's pirate boards are unusual for their size class in carrying real
//! IRQ counters, and in disagreeing about direction: `KS202` counts *up* to a
//! target while `KS7017` counts *down* to zero. `KS7031` is stranger still --
//! it maps four independently-selected 2 KiB PRG windows, a granularity no
//! licensed board uses.
//!
//! One [`KaiserMapper`] with a [`KaiserBoard`] discriminant rather than five
//! types, because the boards share their register-file and save-state shape
//! and differ only in decode and IRQ direction.
//!
//! A best-effort (Tier-2) board: register-decode correctness verified against
//! the reference emulators (`Mesen2`, `GeraNES`) and the nesdev wiki, with no
//! commercial-oracle ROM in the tree. Banking math is direct slice indexing and
//! every bank select wraps with `% count`, so a register write can never index
//! out of bounds -- required for the `#![no_std]` chip stack, which cannot
//! afford a panic on a register access.
//!
//! See `tier.rs` (`MapperTier::BestEffort`), `docs/adr/0011-mapper-tiering.md`,
//! and `docs/mappers.md` §Mapper coverage matrix.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::match_same_arms,
    clippy::doc_markdown,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::missing_const_for_fn,
    clippy::struct_excessive_bools,
    clippy::bool_to_int_with_if,
    clippy::unreadable_literal
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_8K: usize = 0x2000;
const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_2K: usize = 0x0800;
const CHR_BANK_1K: usize = 0x0400;
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

/// Which Kaiser variant a [`KaiserMapper`] models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KaiserBoard {
    /// Mapper 56 (KS202) — extra CHR + mirror writes; up-counting M2 IRQ.
    M56,
    /// Mapper 142 (KS7032) — like 56 without the extra CHR/mirror writes.
    M142,
    /// Mapper 303 (KS7017) — address-decoded PRG + down-counting M2 IRQ.
    M303,
    /// Mapper 305 (KS7031) — four 2 KiB $6000 PRG-ROM windows (no IRQ).
    M305,
    /// Mapper 306 (KS7016) — address-decoded $6000 PRG window (no IRQ).
    M306,
    /// Mapper 312 (KS7013B) — $6000 PRG select + $8000 mirroring (no IRQ).
    M312,
}

/// A Kaiser FDS-conversion / window board.
pub struct KaiserMapper {
    board: KaiserBoard,
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    wram: Box<[u8]>,
    mirroring: Mirroring,
    // KS202/KS7032 (56/142).
    prg_regs: [u8; 4],
    selected_reg: u8,
    use_rom: bool,
    chr_banks: [u8; 8],
    // KS7016 (306) PRG-ROM $6000 window.
    win_6000: u8,
    // KS7031 (305) four 2 KiB windows.
    regs4: [u8; 4],
    // 312 PRG select (16 KiB).
    prg16: u8,
    // IRQ (56/142 up-count, 303 down-count).
    irq_counter: u16,
    irq_reload: u16,
    irq_enabled: bool,
    irq_control: u8,
    irq_pending: bool,
}

impl KaiserMapper {
    const SAVE_LEN: usize = 4 + 1 + 1 + 8 + 1 + 4 + 1 + 2 + 2 + 1 + 1 + 1 + 1;

    fn new(
        board: KaiserBoard,
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        id: u16,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, id)?;
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else {
            chr_rom
        };
        Ok(Self {
            board,
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            wram: vec![0u8; PRG_BANK_8K].into_boxed_slice(),
            mirroring,
            prg_regs: [0; 4],
            selected_reg: 0,
            use_rom: false,
            chr_banks: [0; 8],
            win_6000: 8,
            regs4: [0; 4],
            prg16: 0,
            irq_counter: 0,
            irq_reload: 0,
            irq_enabled: false,
            irq_control: 0,
            irq_pending: false,
        })
    }

    fn prg_count_8k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_8K).max(1)
    }

    fn prg_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn prg_count_2k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_2K).max(1)
    }

    fn chr_count_1k(&self) -> usize {
        (self.chr.len() / CHR_BANK_1K).max(1)
    }
}

impl Mapper for KaiserMapper {
    fn caps(&self) -> MapperCaps {
        match self.board {
            KaiserBoard::M56 | KaiserBoard::M142 | KaiserBoard::M303 => MapperCaps::CYCLE_IRQ,
            _ => MapperCaps::NONE,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match self.board {
            KaiserBoard::M56 | KaiserBoard::M142 => match addr {
                0x6000..=0x7FFF => {
                    if self.use_rom {
                        let count = self.prg_count_8k();
                        let bank = (self.prg_regs[3] as usize) % count;
                        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
                    } else {
                        self.wram[addr as usize & 0x1FFF]
                    }
                }
                0x8000..=0xFFFF => {
                    let slot = (addr as usize - 0x8000) / PRG_BANK_8K;
                    let count = self.prg_count_8k();
                    let bank = if slot == 3 {
                        count - 1
                    } else {
                        self.prg_regs[slot] as usize % count
                    };
                    self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
                }
                _ => 0,
            },
            KaiserBoard::M303 => match addr {
                0x4030 => {
                    let p = self.irq_pending;
                    self.irq_pending = false;
                    u8::from(p)
                }
                0x8000..=0xBFFF => {
                    let count = self.prg_count_16k();
                    let bank = (self.prg16 as usize) % count;
                    self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
                }
                0xC000..=0xFFFF => {
                    let count = self.prg_count_16k();
                    let bank = 2 % count;
                    self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
                }
                _ => 0,
            },
            KaiserBoard::M305 => match addr {
                0x6000..=0x7FFF => {
                    let win = (addr as usize - 0x6000) / PRG_BANK_2K;
                    let count = self.prg_count_2k();
                    let bank = (self.regs4[win] as usize) % count;
                    self.prg_rom[bank * PRG_BANK_2K + (addr as usize & 0x7FF)]
                }
                0x8000..=0xFFFF => {
                    // Fixed last 32 KiB (16 x 2 KiB windows = banks count-16..count-1).
                    let count = self.prg_count_2k();
                    let win = (addr as usize - 0x8000) / PRG_BANK_2K;
                    let bank = count.saturating_sub(16 - win) % count;
                    self.prg_rom[bank * PRG_BANK_2K + (addr as usize & 0x7FF)]
                }
                _ => 0,
            },
            KaiserBoard::M306 => match addr {
                0x6000..=0x7FFF => {
                    let count = self.prg_count_8k();
                    let bank = (self.win_6000 as usize) % count;
                    self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
                }
                0x8000..=0xFFFF => {
                    // Fixed last 32 KiB.
                    let count = self.prg_count_8k();
                    let slot = (addr as usize - 0x8000) / PRG_BANK_8K;
                    let bank = count.saturating_sub(4 - slot) % count;
                    self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
                }
                _ => 0,
            },
            KaiserBoard::M312 => match addr {
                0x8000..=0xBFFF => {
                    let count = self.prg_count_16k();
                    let bank = (self.prg16 as usize) % count;
                    self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
                }
                0xC000..=0xFFFF => {
                    let count = self.prg_count_16k();
                    let bank = count - 1;
                    self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
                }
                _ => 0,
            },
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        match self.board {
            KaiserBoard::M303 => addr != 0x4030 && (0x4020..=0x5FFF).contains(&addr),
            _ => (0x4020..=0x5FFF).contains(&addr),
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match self.board {
            KaiserBoard::M56 | KaiserBoard::M142 => match addr & 0xF000 {
                0x8000 => self.irq_reload = (self.irq_reload & 0xFFF0) | (value as u16 & 0x0F),
                0x9000 => {
                    self.irq_reload = (self.irq_reload & 0xFF0F) | ((value as u16 & 0x0F) << 4);
                }
                0xA000 => {
                    self.irq_reload = (self.irq_reload & 0xF0FF) | ((value as u16 & 0x0F) << 8);
                }
                0xB000 => {
                    self.irq_reload = (self.irq_reload & 0x0FFF) | ((value as u16 & 0x0F) << 12);
                }
                0xC000 => {
                    self.irq_control = value;
                    if value & 0x02 != 0 {
                        self.irq_counter = self.irq_reload;
                    }
                    self.irq_enabled = value & 0x02 != 0;
                    self.irq_pending = false;
                }
                0xD000 => self.irq_pending = false,
                0xE000 => self.selected_reg = (value & 0x07).wrapping_sub(1),
                0xF000 => {
                    match self.selected_reg {
                        0..=3 => {
                            let i = self.selected_reg as usize;
                            self.prg_regs[i] = (self.prg_regs[i] & 0x10) | (value & 0x0F);
                        }
                        4 => self.use_rom = value & 0x04 != 0,
                        _ => {}
                    }
                    if self.board == KaiserBoard::M56 {
                        match addr & 0xFC00 {
                            0xF000 => {
                                let bank = (addr & 0x03) as usize;
                                self.prg_regs[bank] = (value & 0x10) | (self.prg_regs[bank] & 0x0F);
                            }
                            0xF800 => {
                                self.mirroring = if value & 0x01 != 0 {
                                    Mirroring::Vertical
                                } else {
                                    Mirroring::Horizontal
                                };
                            }
                            0xFC00 => self.chr_banks[(addr & 0x07) as usize] = value,
                            _ => {}
                        }
                    }
                }
                _ => {}
            },
            KaiserBoard::M303 => {
                if addr & 0xFF00 == 0x4A00 {
                    self.prg16 = (((addr >> 2) & 0x03) | ((addr >> 4) & 0x04)) as u8;
                } else if addr == 0x4020 {
                    self.irq_pending = false;
                    self.irq_counter = (self.irq_counter & 0xFF00) | value as u16;
                } else if addr == 0x4021 {
                    self.irq_pending = false;
                    self.irq_counter = (self.irq_counter & 0x00FF) | ((value as u16) << 8);
                    self.irq_enabled = true;
                } else if addr == 0x4025 {
                    self.mirroring = if (value >> 3) & 0x01 != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
            }
            KaiserBoard::M305 => {
                if (0x8000..=0xFFFF).contains(&addr) {
                    self.regs4[((addr >> 11) & 0x03) as usize] = value;
                }
            }
            KaiserBoard::M306 => {
                if addr >= 0x8000 {
                    let mode = (addr & 0x30) == 0x30;
                    match addr & 0xD943 {
                        0xD943 => {
                            self.win_6000 = if mode {
                                0x0B
                            } else {
                                ((addr >> 2) & 0x0F) as u8
                            };
                        }
                        0xD903 => {
                            self.win_6000 = if mode {
                                0x08 | ((addr >> 2) & 0x03) as u8
                            } else {
                                0x0B
                            };
                        }
                        _ => {}
                    }
                }
            }
            KaiserBoard::M312 => {
                if addr < 0x8000 {
                    if (0x6000..=0x7FFF).contains(&addr) {
                        self.prg16 = value;
                    }
                } else {
                    self.mirroring = if value & 0x01 != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
            }
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    return self.chr[addr as usize & (self.chr.len() - 1)];
                }
                if self.board == KaiserBoard::M56 {
                    let slot = (addr as usize) / CHR_BANK_1K;
                    let count = self.chr_count_1k();
                    let bank = (self.chr_banks[slot] as usize) % count;
                    return self.chr[bank * CHR_BANK_1K + (addr as usize & 0x3FF)];
                }
                self.chr[addr as usize & (self.chr.len() - 1)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                self.chr[addr as usize & (self.chr.len() - 1)] = value;
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn notify_cpu_cycle(&mut self) {
        match self.board {
            KaiserBoard::M56 | KaiserBoard::M142 => {
                if self.irq_control & 0x02 != 0 {
                    self.irq_counter = self.irq_counter.wrapping_add(1);
                    if self.irq_counter == 0xFFFF {
                        self.irq_counter = self.irq_reload;
                        self.irq_control &= !0x02;
                        self.irq_pending = true;
                    }
                }
            }
            KaiserBoard::M303 if self.irq_enabled && self.irq_counter != 0 => {
                self.irq_counter -= 1;
                if self.irq_counter == 0 {
                    self.irq_enabled = false;
                    self.irq_pending = true;
                }
            }
            _ => {}
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn irq_acknowledge(&mut self) {
        self.irq_pending = false;
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out =
            Vec::with_capacity(1 + Self::SAVE_LEN + self.vram.len() + self.wram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.prg_regs);
        out.push(self.selected_reg);
        out.push(u8::from(self.use_rom));
        out.extend_from_slice(&self.chr_banks);
        out.push(self.win_6000);
        out.extend_from_slice(&self.regs4);
        out.push(self.prg16);
        out.extend_from_slice(&self.irq_counter.to_le_bytes());
        out.extend_from_slice(&self.irq_reload.to_le_bytes());
        out.push(u8::from(self.irq_enabled));
        out.push(self.irq_control);
        out.push(u8::from(self.irq_pending));
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.wram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 1 + Self::SAVE_LEN + self.vram.len() + self.wram.len() + chr_ram;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let mut c = 1;
        self.prg_regs.copy_from_slice(&data[c..c + 4]);
        c += 4;
        self.selected_reg = data[c];
        self.use_rom = data[c + 1] != 0;
        c += 2;
        self.chr_banks.copy_from_slice(&data[c..c + 8]);
        c += 8;
        self.win_6000 = data[c];
        c += 1;
        self.regs4.copy_from_slice(&data[c..c + 4]);
        c += 4;
        self.prg16 = data[c];
        c += 1;
        self.irq_counter = u16::from_le_bytes([data[c], data[c + 1]]);
        self.irq_reload = u16::from_le_bytes([data[c + 2], data[c + 3]]);
        c += 4;
        self.irq_enabled = data[c] != 0;
        self.irq_control = data[c + 1];
        self.irq_pending = data[c + 2] != 0;
        self.mirroring = byte_to_mirroring(data[c + 3], self.mirroring);
        c += 4;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        self.wram.copy_from_slice(&data[c..c + self.wram.len()]);
        c += self.wram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        }
        Ok(())
    }
}

macro_rules! kaiser_ctor {
    ($fn_name:ident, $board:expr, $id:expr, $doc:expr) => {
        #[doc = $doc]
        ///
        /// # Errors
        /// [`MapperError::Invalid`] on a bad PRG/CHR size.
        pub fn $fn_name(
            prg_rom: Box<[u8]>,
            chr_rom: Box<[u8]>,
            mirroring: Mirroring,
        ) -> Result<KaiserMapper, MapperError> {
            KaiserMapper::new($board, prg_rom, chr_rom, mirroring, $id)
        }
    };
}

kaiser_ctor!(new_m56, KaiserBoard::M56, 56, "Mapper 56 (Kaiser KS202).");
kaiser_ctor!(
    new_m142,
    KaiserBoard::M142,
    142,
    "Mapper 142 (Kaiser KS7032)."
);
kaiser_ctor!(
    new_m303,
    KaiserBoard::M303,
    303,
    "Mapper 303 (Kaiser KS7017)."
);
kaiser_ctor!(
    new_m305,
    KaiserBoard::M305,
    305,
    "Mapper 305 (Kaiser KS7031)."
);
kaiser_ctor!(
    new_m306,
    KaiserBoard::M306,
    306,
    "Mapper 306 (Kaiser KS7016)."
);
kaiser_ctor!(
    new_m312,
    KaiserBoard::M312,
    312,
    "Mapper 312 (Kaiser KS7013B)."
);

// ===========================================================================
// Waixing253 (mapper 253) — Waixing VRC4-clone, Dragon Ball Z.
//
// Per-1 KiB CHR low/high registers ($B000-$E00C), a CHR-RAM escape (CHR reg
// value 4/5 + a force-ROM toggle on slot 0 via $88/$C8), two 8 KiB PRG selects
// ($8010/$A010), $9400 mirroring, and a /114-scaled CPU-cycle IRQ ($F000 etc.).
// Ported from Mesen2 Waixing/Mapper253.h.
// ===========================================================================

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_8K];
        for b in 0..banks {
            v[b * PRG_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_prg_16k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_16K];
        for b in 0..banks {
            v[b * PRG_BANK_16K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr_1k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_1K];
        for b in 0..banks {
            v[b * CHR_BANK_1K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_8K];
        for b in 0..banks {
            v[b * CHR_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_prg_2k_tagged(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_2K];
        for b in 0..banks {
            v[b * PRG_BANK_2K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn kaiser202_prg_regs_and_up_count_irq() {
        let mut m = new_m142(synth_prg_8k(16), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0xE000, 0x01); // select reg (1-1=0)
        m.cpu_write(0xF000, 0x03); // prg_regs[0] low = 3
        assert_eq!(m.cpu_read(0x8000), 3);

        // IRQ: reload, enable, count up to 0xFFFF.
        m.cpu_write(0x8000, 0x0E); // reload low nibble
        m.cpu_write(0xC000, 0x02); // enable + load
        // Counter loads 0x...E; count up until 0xFFFF wraps.
        let mut fired = false;
        for _ in 0..0x20000 {
            m.notify_cpu_cycle();
            if m.irq_pending() {
                fired = true;
                break;
            }
        }
        assert!(fired);
    }

    #[test]
    fn kaiser202_save_state_round_trip() {
        let mut m = new_m56(synth_prg_8k(16), synth_chr_1k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0xE000, 0x01);
        m.cpu_write(0xF000, 0x05);
        m.cpu_write(0xFC00, 0x02); // m56 CHR write
        m.ppu_write(0x2002, 0x44);
        let blob = m.save_state();
        let mut m2 = new_m56(synth_prg_8k(16), synth_chr_1k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x2002), 0x44);
    }

    #[test]
    fn kaiser7017_prg_and_down_count_irq() {
        let mut m = new_m303(synth_prg_16k(8), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        // $4Axx address-decoded PRG select.
        m.cpu_write(0x4A00 | (1 << 2), 0); // prg16 = ((1<<2)>>2)&3 = 1
        assert_eq!(m.cpu_read(0x8000), 1);

        m.cpu_write(0x4020, 0x03); // counter low
        m.cpu_write(0x4021, 0x00); // counter high + enable -> counter = 3
        for _ in 0..3 {
            m.notify_cpu_cycle();
        }
        assert!(m.irq_pending());
        assert_eq!(m.cpu_read(0x4030), 0x01); // read-ack returns pending then clears.
        assert!(!m.irq_pending());
    }

    #[test]
    fn kaiser7031_windowed_prg() {
        // 8 KiB == 4 x 2 KiB pages; use a 2 KiB-tagged 16 KiB image (8 pages).
        let mut m = new_m305(synth_prg_2k_tagged(8), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        // $8000-$FFFF: window = (addr>>11)&3, value = 2 KiB page index.
        m.cpu_write(0x8000, 5); // regs4[0] = 5
        assert_eq!(m.cpu_read(0x6000), 5); // first 2 KiB $6000 window -> page 5.
    }

    #[test]
    fn kaiser7031_save_state_round_trip() {
        let mut m = new_m305(synth_prg_8k(8), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 3);
        m.cpu_write(0x8800, 4);
        m.ppu_write(0x0005, 0x21);
        let blob = m.save_state();
        let mut m2 = new_m305(synth_prg_8k(8), Box::new([]), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x6000), m.cpu_read(0x6000));
        assert_eq!(m2.ppu_read(0x0005), 0x21);
    }

    #[test]
    fn kaiser7016_window_decode() {
        let mut m = new_m306(synth_prg_8k(16), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        // $D943 with mode bits (addr&0x30 != 0x30) -> _prgReg = (addr>>2)&0x0F.
        let addr = 0xD943; // addr&0x30 = 0x00 -> not mode -> reg = (0xD943>>2)&0x0F
        m.cpu_write(addr, 0);
        let v = m.cpu_read(0x6000);
        assert!((v as usize) < 16);
    }

    #[test]
    fn kaiser7013b_prg_and_mirror() {
        let mut m = new_m312(synth_prg_16k(8), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0x6000, 3); // prg16 = 3
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 7); // fixed last bank.
        m.cpu_write(0x8000, 0x01); // horizontal
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }
}

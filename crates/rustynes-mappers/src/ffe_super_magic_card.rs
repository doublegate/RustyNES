// SPDX-License-Identifier: GPL-3.0-or-later
//! Mappers 6, 8 and 17: games extracted from Front Fareast (FFE) Magic Card
//! and Super Magic Card disks (v3.2.0).
//!
//! Written from the vendored `NESdev` pages `INES_Mapper_006`,
//! `INES_Mapper_008`, `INES_Mapper_017` and the page they all point to for the
//! hardware, `Super_Magic_Card`; no reference-emulator source was read.
//!
//! These are not cartridge boards. They are copier RAM cartridges: the game
//! is loaded from a floppy into the Super Magic Card's 512 KiB of PRG DRAM and
//! 256 KiB of CHR-RAM, then run in "Play mode". The three iNES numbers only
//! say which state the card's registers start in:
//!
//! | iNES | `$4500` | `$42FF` (1M mode) | 2M/4M | other |
//! |---|---|---|---|---|
//! | 6, submapper *n* (iNES 1.0 = 1) | `$42` | `n << 5`, H/V from the header | disabled (`$43FF = $00`) | |
//! | 8 | as 6 submapper 4 ("a synonym of Mapper 6 submapper 4") | | | |
//! | 17 | `$47` (1 KiB CHR) | `$20`, H/V | 4M (`$43FC = $00`) | `$4504-$4507` = the last four 8 KiB banks |
//!
//! This module models the card in Play mode, which is all an extracted game
//! uses. GUI mode, pass-through mode, the parallel port and the floppy
//! controller are the card's own firmware's business and are not modelled; a
//! write that leaves Play mode is stored but changes nothing.
//!
//! ## Memory (Play mode)
//!
//! - `$5000-$5FFF`: 4 KiB scratch RAM.
//! - `$6000-$7FFF`: an 8 KiB window into 32 KiB WRAM (`$4500` bits 4-5).
//! - `$8000-$FFFF`: a 32 KiB view of the 512 KiB PRG DRAM, banked by one of
//!   the eight latch modes, the 2M mode or the 4M mode. With the DRAM
//!   write-enabled (`$42Fx` A1 = 0) a CPU write there stores into the DRAM and
//!   the latch is off.
//! - `$0000-$1FFF`: CHR-RAM in one 8 KiB bank of four, or eight 1 KiB banks
//!   (`$4510-$4517`) in 1 KiB mode, or MMC4-style 4 KiB banks.
//! - Nametables: CIRAM with the `$42Fx` mirroring (1scA, 1scB, V, H), or, with
//!   `$4500` bit 1 clear, four 1 KiB CHR-RAM pages (`$4518-$451B`).
//!
//! The image's PRG is loaded at the start of the DRAM and its CHR at the start
//! of the CHR-RAM, as the copier would.
//!
//! ## Registers
//!
//! `$42FC-$42FF` (A1 = protect, A0 and D4 = mirroring type `A0 << 1 | D4`,
//! D7-D5 = latch mode); `$43FC-$43FF` (A0 = 2M/4M off, A1 = 2M rather than
//! 4M, D1-D0 = CHR bank); `$4500` (mode: CHR 1 KiB, CIRAM nametables, MMC4
//! off, IRQ source, WRAM bank, Play, pass-through); `$4501` IRQ off and
//! acknowledge; `$4502`/`$4503` IRQ counter, `$4503` enabling it; `$4504-$4507`
//! 4M PRG banks; `$4510-$451B` 1 KiB CHR and nametable banks. Reads: `$4500`
//! and `$4501` state registers. The mirroring-type encoding is taken from the
//! two initial values the mapper pages give: `$42FF` with D4 = 0 is vertical
//! (type 2) and with D4 = 1 horizontal (type 3), which only `A0 << 1 | D4`
//! satisfies.
//!
//! ## IRQs
//!
//! The card's 16-bit up-counter, clocked by M2 or by every PPU A12 rise
//! (unfiltered), raises an IRQ when it wraps from `$FFFF` to `$0000`. And
//! because the older Magic Cards had no counter, "a few games abuse the FDS
//! Disk Data IRQ": with `$4025` bit 7 set the FDS RAM adapter the card sits on
//! raises an IRQ every 1792 master clocks (149 1/3 CPU cycles), acknowledged by
//! a `$4024` write. Both are modelled, since the adapter is always present
//! under the card.
//!
//! ## Trainers
//!
//! A Doctor-format trainer arrives as the iNES trainer. Mapper 6 loads it at
//! `$7000-$71FF` (WRAM bank 0) and, on a hard reset, "JSRs to `$7003` before
//! `JMPing` to the game's reset vector". Mapper 17 loads it at `$7000`,
//! `$5D00`, `$5E00` or `$5F00` (NES 2.0 submapper 0-3) and JMPs to it instead
//! of the reset vector. The card's BIOS did this; here the board answers the
//! power-on reset-vector fetch with the trainer's address (mapper 17), or with
//! a six-byte overlay at `$7200` holding `JSR $7003 / JMP ($FFFC)` that
//! disappears once its last byte has been fetched (mapper 6). A soft reset
//! (`Mapper::reset`) uses the ordinary vector, as the page specifies the call
//! only "on a hard reset".
//!
//! ## What the pages leave open
//!
//! - The initial `$4510-$4517` values for mapper 17 (not given): 0-7.
//! - What "the 4M registers function as mirrors of the 2M banking mode's
//!   registers" when 4M mode is off means: a `$4504-$4507` write then also
//!   loads the matching 2M register's PRG bank. In 4M mode, `$8000-$FFFF`
//!   writes still load the 2M registers, which "accept values even when ...
//!   not active", without moving the 4M banks.
//! - Whether the 2M registers latch while the DRAM is write-enabled: they do
//!   not, like the latch they overlap.

#![allow(
    clippy::cast_possible_truncation,
    clippy::missing_const_for_fn,
    clippy::struct_excessive_bools,
    clippy::too_many_lines
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError, mirroring_name};
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_DRAM: usize = 512 * 1024;
const CHR_RAM: usize = 256 * 1024;
const WRAM: usize = 32 * 1024;
const SCRATCH: usize = 4 * 1024;
const BANK_8K: usize = 0x2000;
const BANK_16K: usize = 0x4000;
const BANK_32K: usize = 0x8000;
const TRAINER_LEN: usize = 512;
/// The mapper 6 hard-reset stub's address: just past the trainer.
const STUB_ADDR: u16 = 0x7200;
/// `JSR $7003 ; JMP ($FFFC)`.
const STUB: [u8; 6] = [0x20, 0x03, 0x70, 0x6C, 0xFC, 0xFF];
/// The FDS adapter's disk IRQ period, 1792 master clocks, in thirds of a CPU
/// cycle (12 master clocks per CPU cycle: 1792 / 12 = 448 / 3).
const FDS_IRQ_THIRDS: u16 = 448;
const SAVE_STATE_VERSION: u8 = 1;

/// Which iNES number the image carried (it decides only the initial state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfeBoard {
    /// Mapper 6 with the given latch mode (NES 2.0 submapper; 1 on iNES 1.0).
    /// Mapper 8 is mapper 6 with latch mode 4.
    MagicCard(u8),
    /// Mapper 17 with the given trainer-address submapper (0-3).
    SuperMagicCard(u8),
}

/// The Super Magic Card in Play mode. See the module docs.
pub struct SuperMagicCard {
    board: FfeBoard,
    prg: Box<[u8]>,
    chr: Box<[u8]>,
    wram: Box<[u8]>,
    scratch: Box<[u8]>,
    // $42Fx: latch mode, write-protect, mirroring type.
    latch_mode: u8,
    prg_protected: bool,
    mirroring_type: u8,
    /// The latch at `$8000-$FFFF` (latch-based modes).
    latch: u8,
    // $43Fx.
    mode_2m4m_enabled: bool,
    mode_2m: bool,
    /// The common 8 KiB CHR bank (`CC`) used by the 2M and 4M modes.
    chr_cc: u8,
    /// 2M registers (`$8000/$A000/$C000/$E000`, D7-D2).
    reg_2m: [u8; 4],
    /// 4M registers (`$4504-$4507`, D5-D0).
    reg_4m: [u8; 4],
    /// `$4500`.
    smc_mode: u8,
    /// `$4510-$451B`: eight CHR and four nametable 1 KiB banks.
    chr_1k: [u8; 12],
    /// MMC4 latches for `$0000-$0FFF` and `$1000-$1FFF` (`false` = `$FD`).
    mmc4_latch: [bool; 2],
    // IRQ counter.
    irq_counter: u16,
    irq_enabled: bool,
    irq_pending: bool,
    // FDS adapter disk IRQ.
    fds_irq_enabled: bool,
    fds_irq_thirds: u16,
    fds_irq_pending: bool,
    /// Power-on reset vector override (the trainer address, or the stub).
    boot_vector: Option<u16>,
    /// The mapper 6 stub is mapped at `$7200` until its last byte is fetched.
    stub_live: bool,
    last_a12: bool,
}

impl SuperMagicCard {
    /// Build the card for `board`, loading the image's PRG into the DRAM, its
    /// CHR into the CHR-RAM and its trainer (if any) where the board's page
    /// says, then setting the registers to the page's initial values.
    ///
    /// # Errors
    ///
    /// [`MapperError::Invalid`] when the PRG image is empty, larger than the
    /// 512 KiB DRAM or not a multiple of 8 KiB, or the CHR image is larger than
    /// the 256 KiB CHR-RAM.
    pub fn new(
        board: FfeBoard,
        prg_rom: &[u8],
        chr_rom: &[u8],
        trainer: Option<&[u8]>,
        vertical: bool,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || prg_rom.len() > PRG_DRAM || !prg_rom.len().is_multiple_of(BANK_8K)
        {
            return Err(MapperError::Invalid(format!(
                "FFE PRG image size {} is not 8 KiB-aligned within the card's 512 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.len() > CHR_RAM {
            return Err(MapperError::Invalid(format!(
                "FFE CHR image size {} exceeds the card's 256 KiB of CHR-RAM",
                chr_rom.len()
            )));
        }
        let mut prg = vec![0u8; PRG_DRAM].into_boxed_slice();
        prg[..prg_rom.len()].copy_from_slice(prg_rom);
        let mut chr = vec![0u8; CHR_RAM].into_boxed_slice();
        chr[..chr_rom.len()].copy_from_slice(chr_rom);
        let mut m = Self {
            board,
            prg,
            chr,
            wram: vec![0u8; WRAM].into_boxed_slice(),
            scratch: vec![0u8; SCRATCH].into_boxed_slice(),
            latch_mode: 0,
            prg_protected: true,
            mirroring_type: 2,
            latch: 0,
            mode_2m4m_enabled: false,
            mode_2m: false,
            chr_cc: 0,
            reg_2m: [0; 4],
            reg_4m: [0; 4],
            smc_mode: 0,
            chr_1k: [0, 1, 2, 3, 4, 5, 6, 7, 0, 0, 0, 0],
            mmc4_latch: [false; 2],
            irq_counter: 0,
            irq_enabled: false,
            irq_pending: false,
            fds_irq_enabled: false,
            fds_irq_thirds: 0,
            fds_irq_pending: false,
            boot_vector: None,
            stub_live: false,
            last_a12: false,
        };
        let h_bit = if vertical { 0x00 } else { 0x10 };
        match board {
            FfeBoard::MagicCard(sub) => {
                m.cpu_write(0x4500, 0x42);
                m.cpu_write(0x42FF, ((sub & 0x07) << 5) | h_bit);
                m.cpu_write(0x43FF, 0x00);
                if let Some(t) = trainer {
                    m.wram[0x1000..0x1000 + TRAINER_LEN].copy_from_slice(&t[..TRAINER_LEN]);
                    m.boot_vector = Some(STUB_ADDR);
                    m.stub_live = true;
                }
            }
            FfeBoard::SuperMagicCard(sub) => {
                m.cpu_write(0x4500, 0x47);
                m.cpu_write(0x42FF, 0x20 | h_bit);
                m.cpu_write(0x43FC, 0x00);
                let n = (prg_rom.len() / BANK_8K) as u8;
                for i in 0..4u8 {
                    m.cpu_write(0x4504 + u16::from(i), n.wrapping_sub(4 - i));
                }
                if let Some(t) = trainer {
                    let at: u16 = match sub {
                        1 => 0x5D00,
                        2 => 0x5E00,
                        3 => 0x5F00,
                        _ => 0x7000,
                    };
                    for (i, &b) in t[..TRAINER_LEN].iter().enumerate() {
                        m.store_low(at + i as u16, b);
                    }
                    m.boot_vector = Some(at);
                }
            }
        }
        Ok(m)
    }

    /// Store into scratch RAM or WRAM (trainer loading, which crosses
    /// `$5FFF -> $6000` for submapper 3).
    fn store_low(&mut self, addr: u16, value: u8) {
        match addr {
            0x5000..=0x5FFF => self.scratch[usize::from(addr - 0x5000)] = value,
            0x6000..=0x7FFF => {
                let off = self.wram_offset(addr);
                self.wram[off] = value;
            }
            _ => {}
        }
    }

    fn wram_offset(&self, addr: u16) -> usize {
        usize::from((self.smc_mode >> 4) & 0x03) * BANK_8K + usize::from(addr & 0x1FFF)
    }

    fn chr_1k_mode(&self) -> bool {
        self.smc_mode & 0x01 != 0
    }

    fn ciram_nametables(&self) -> bool {
        self.smc_mode & 0x02 != 0
    }

    fn mmc4_mode(&self) -> bool {
        self.chr_1k_mode() && self.smc_mode & 0x04 == 0
    }

    fn irq_from_a12(&self) -> bool {
        self.smc_mode & 0x08 != 0
    }

    /// The DRAM offset for CPU `addr` (`$8000-$FFFF`) in Play mode.
    fn prg_offset(&self, addr: u16) -> usize {
        let slot = usize::from((addr >> 13) & 0x03);
        let in8 = usize::from(addr & 0x1FFF);
        if self.mode_2m4m_enabled {
            let bank = if self.mode_2m {
                usize::from(self.reg_2m[slot])
            } else {
                usize::from(self.reg_4m[slot] & 0x3F)
            };
            return (bank * BANK_8K + in8) % PRG_DRAM;
        }
        let upper = addr >= 0xC000;
        let in16 = usize::from(addr & 0x3FFF);
        let l = usize::from(self.latch);
        let bank16 = |b: usize| (b * BANK_16K + in16) % PRG_DRAM;
        let bank32 = |b: usize| (b * BANK_32K + usize::from(addr & 0x7FFF)) % PRG_DRAM;
        match self.latch_mode {
            0 => bank16(if upper { 7 } else { l & 0x07 }),
            1 => bank16(if upper { 7 } else { (l >> 2) & 0x0F }),
            2 => bank16(if upper { 15 } else { l & 0x0F }),
            3 => bank16(if upper { l & 0x0F } else { 15 }),
            4 => bank32((l >> 4) & 0x03),
            _ => bank32(3),
        }
    }

    /// The 8 KiB CHR bank in 8 KiB mode.
    fn chr_8k_bank(&self) -> usize {
        if self.mode_2m4m_enabled {
            return usize::from(self.chr_cc & 0x03);
        }
        let l = usize::from(self.latch);
        match self.latch_mode {
            1 | 4 | 5 => l & 0x03,
            3 => (l >> 4) & 0x03,
            6 => l & 0x01,
            _ => 0,
        }
    }

    /// CHR-RAM is write-protected in latch modes 4-7, whatever banking mode
    /// is active ("except CHR memory protection, which is still decided by
    /// the 1M Mode Register").
    fn chr_writable(&self) -> bool {
        self.latch_mode < 4
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let addr = usize::from(addr & 0x1FFF);
        if self.mmc4_mode() {
            let half = addr >> 12;
            let reg = half * 4 + usize::from(self.mmc4_latch[half]) * 2;
            let bank4 = usize::from(self.chr_1k[reg] >> 2);
            (bank4 * 0x1000 + (addr & 0x0FFF)) % CHR_RAM
        } else if self.chr_1k_mode() {
            usize::from(self.chr_1k[addr >> 10]) * 0x400 + (addr & 0x03FF)
        } else {
            (self.chr_8k_bank() * BANK_8K + addr) % CHR_RAM
        }
    }

    /// MMC4 mode: a pattern read of `$xFD8-$xFDF` or `$xFE8-$xFEF` switches
    /// that half's 4 KiB bank for the reads that follow.
    fn mmc4_watch(&mut self, addr: u16) {
        if !self.mmc4_mode() {
            return;
        }
        let half = usize::from((addr >> 12) & 1);
        match addr & 0x0FF8 {
            0x0FD8 => self.mmc4_latch[half] = false,
            0x0FE8 => self.mmc4_latch[half] = true,
            _ => {}
        }
    }

    fn mirroring(&self) -> Mirroring {
        match self.mirroring_type & 0x03 {
            0 => Mirroring::SingleScreenA,
            1 => Mirroring::SingleScreenB,
            2 => Mirroring::Vertical,
            _ => Mirroring::Horizontal,
        }
    }

    fn nt_chr_offset(&self, addr: u16) -> usize {
        let table = usize::from((addr >> 10) & 0x03);
        usize::from(self.chr_1k[8 + table]) * 0x400 + usize::from(addr & 0x03FF)
    }

    fn state_register_1(&self) -> u8 {
        // W = 0 (no parallel-port byte), B = 1 (button not pressed).
        0x40 | (self.latch_mode << 3)
            | ((self.mirroring_type & 0x01) << 2)
            | (u8::from(self.prg_protected) << 1)
            | (self.mirroring_type >> 1)
    }

    fn state_register_2(&self) -> u8 {
        (self.latch << 2) | (u8::from(!self.mode_2m) << 1) | u8::from(!self.mode_2m4m_enabled)
    }

    fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x42FC..=0x42FF => {
                self.prg_protected = addr & 0x02 != 0;
                self.mirroring_type = (((addr & 0x01) as u8) << 1) | ((value >> 4) & 0x01);
                self.latch_mode = value >> 5;
            }
            0x43FC..=0x43FF => {
                self.mode_2m4m_enabled = addr & 0x01 == 0;
                self.mode_2m = addr & 0x02 != 0;
                self.chr_cc = value & 0x03;
            }
            0x4500 => self.smc_mode = value,
            0x4501 => {
                self.irq_enabled = false;
                self.irq_pending = false;
            }
            0x4502 => {
                self.irq_counter = (self.irq_counter & 0xFF00) | u16::from(value);
                self.irq_pending = false;
            }
            0x4503 => {
                self.irq_counter = (self.irq_counter & 0x00FF) | (u16::from(value) << 8);
                self.irq_pending = false;
                self.irq_enabled = true;
            }
            0x4504..=0x4507 => {
                let i = usize::from(addr - 0x4504);
                self.reg_4m[i] = value & 0x3F;
                // "Mirrors of the 2M registers" whenever 4M mode is not the
                // active one.
                if !self.mode_2m4m_enabled || self.mode_2m {
                    self.reg_2m[i] = value & 0x3F;
                }
            }
            0x4510..=0x451B => self.chr_1k[usize::from(addr - 0x4510)] = value,
            _ => {}
        }
    }

    fn clock_irq(&mut self) {
        if self.irq_enabled {
            let (next, wrapped) = self.irq_counter.overflowing_add(1);
            self.irq_counter = next;
            if wrapped {
                self.irq_pending = true;
            }
        }
    }
}

impl Mapper for SuperMagicCard {
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn sram(&self) -> &[u8] {
        &self.wram
    }

    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.wram
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        !matches!(addr, 0x4500 | 0x4501 | 0x5000..=0xFFFF)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if let Some(v) = self.boot_vector
            && (addr == 0xFFFC || addr == 0xFFFD)
        {
            if addr == 0xFFFD {
                self.boot_vector = None;
            }
            return v.to_le_bytes()[usize::from(addr - 0xFFFC)];
        }
        if self.stub_live && (STUB_ADDR..STUB_ADDR + 6).contains(&addr) {
            if addr == STUB_ADDR + 5 {
                self.stub_live = false;
            }
            return STUB[usize::from(addr - STUB_ADDR)];
        }
        match addr {
            0x4500 => self.state_register_1(),
            0x4501 => self.state_register_2(),
            0x5000..=0x5FFF => self.scratch[usize::from(addr - 0x5000)],
            0x6000..=0x7FFF => self.wram[self.wram_offset(addr)],
            0x8000..=0xFFFF => self.prg[self.prg_offset(addr)],
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            // The FDS RAM adapter under the card.
            0x4024 => self.fds_irq_pending = false,
            0x4025 => {
                self.fds_irq_enabled = value & 0x80 != 0;
                self.fds_irq_thirds = 0;
                if !self.fds_irq_enabled {
                    self.fds_irq_pending = false;
                }
            }
            0x4020..=0x4FFF => self.write_register(addr, value),
            0x5000..=0x7FFF => self.store_low(addr, value),
            0x8000..=0xFFFF => {
                if self.prg_protected {
                    self.latch = value;
                    let slot = usize::from((addr >> 13) & 0x03);
                    self.reg_2m[slot] = value >> 2;
                    if self.mode_2m4m_enabled {
                        self.chr_cc = value & 0x03;
                    }
                } else {
                    let off = self.prg_offset(addr);
                    self.prg[off] = value;
                }
            }
            _ => {}
        }
    }

    fn reset(&mut self) {
        // A soft reset runs the ordinary vector; the trainer call is for a
        // hard reset only.
        self.boot_vector = None;
        self.stub_live = false;
    }

    fn chr_reads_are_pure(&self) -> bool {
        !self.mmc4_mode()
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let v = self.chr[self.chr_offset(addr)];
                self.mmc4_watch(addr);
                v
            }
            0x2000..=0x3EFF if !self.ciram_nametables() => self.chr[self.nt_chr_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_writable() => {
                let off = self.chr_offset(addr);
                self.chr[off] = value;
            }
            0x2000..=0x3EFF if !self.ciram_nametables() => {
                let off = self.nt_chr_offset(addr);
                self.chr[off] = value;
            }
            _ => {}
        }
    }

    fn nametable_fetch(&mut self, addr: u16) -> Option<u8> {
        (!self.ciram_nametables()).then(|| self.chr[self.nt_chr_offset(addr)])
    }

    fn nametable_write(&mut self, addr: u16, value: u8) -> bool {
        if self.ciram_nametables() {
            return false;
        }
        let off = self.nt_chr_offset(addr);
        self.chr[off] = value;
        true
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.ciram_nametables() {
            self.mirroring()
        } else {
            Mirroring::MapperControlled
        }
    }

    fn notify_a12(&mut self, level: bool) {
        if level && !self.last_a12 && self.irq_from_a12() {
            self.clock_irq();
        }
        self.last_a12 = level;
    }

    fn notify_cpu_cycle(&mut self) {
        if !self.irq_from_a12() {
            self.clock_irq();
        }
        if self.fds_irq_enabled {
            self.fds_irq_thirds += 3;
            if self.fds_irq_thirds >= FDS_IRQ_THIRDS {
                self.fds_irq_thirds -= FDS_IRQ_THIRDS;
                self.fds_irq_pending = true;
            }
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending || self.fds_irq_pending
    }

    fn debug_info(&self) -> MapperDebugInfo {
        let (id, name) = match self.board {
            FfeBoard::MagicCard(4) => (8, "FFE Magic Card, GNROM (8)"),
            FfeBoard::MagicCard(_) => (6, "FFE Magic Card (6)"),
            FfeBoard::SuperMagicCard(_) => (17, "FFE Super Magic Card (17)"),
        };
        let mut info = MapperDebugInfo {
            mapper_id: id,
            name: name.into(),
            mirroring: mirroring_name(self.current_mirroring()),
            ..Default::default()
        };
        let mode = if !self.mode_2m4m_enabled {
            format!("latch {}", self.latch_mode)
        } else if self.mode_2m {
            "2M".into()
        } else {
            "4M".into()
        };
        info.prg_banks.push(("mode".into(), mode));
        for slot in 0..4u16 {
            let base = 0x8000 + slot * 0x2000;
            info.prg_banks.push((
                format!("${base:04X}"),
                format!("{:#07x}", self.prg_offset(base)),
            ));
        }
        info.chr_banks.push((
            "CHR".into(),
            if self.mmc4_mode() {
                "MMC4".into()
            } else if self.chr_1k_mode() {
                format!("1K {:?}", &self.chr_1k[..8])
            } else {
                format!("8K bank {}", self.chr_8k_bank())
            },
        ));
        info.irq_state
            .push(("counter".into(), format!("{:#06x}", self.irq_counter)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.irq_state
            .push(("fds".into(), format!("{}", self.fds_irq_enabled)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            64 + self.prg.len() + self.chr.len() + self.wram.len() + self.scratch.len(),
        );
        out.push(SAVE_STATE_VERSION);
        out.push(self.latch_mode);
        out.push(u8::from(self.prg_protected));
        out.push(self.mirroring_type);
        out.push(self.latch);
        out.push(u8::from(self.mode_2m4m_enabled));
        out.push(u8::from(self.mode_2m));
        out.push(self.chr_cc);
        out.extend_from_slice(&self.reg_2m);
        out.extend_from_slice(&self.reg_4m);
        out.push(self.smc_mode);
        out.extend_from_slice(&self.chr_1k);
        out.push(u8::from(self.mmc4_latch[0]));
        out.push(u8::from(self.mmc4_latch[1]));
        out.extend_from_slice(&self.irq_counter.to_le_bytes());
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_pending));
        out.push(u8::from(self.fds_irq_enabled));
        out.extend_from_slice(&self.fds_irq_thirds.to_le_bytes());
        out.push(u8::from(self.fds_irq_pending));
        out.extend_from_slice(&self.boot_vector.unwrap_or(0).to_le_bytes());
        out.push(u8::from(self.boot_vector.is_some()));
        out.push(u8::from(self.stub_live));
        out.push(u8::from(self.last_a12));
        out.extend_from_slice(&self.prg);
        out.extend_from_slice(&self.chr);
        out.extend_from_slice(&self.wram);
        out.extend_from_slice(&self.scratch);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        const HEAD: usize = 1 + 7 + 4 + 4 + 1 + 12 + 2 + 2 + 2 + 1 + 2 + 1 + 2 + 1 + 1 + 1;
        let expected = HEAD + PRG_DRAM + CHR_RAM + WRAM + SCRATCH;
        if data.len() != expected {
            return Err(MapperError::WrongLength {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let mut c = 1;
        let mut byte = || {
            let v = data[c];
            c += 1;
            v
        };
        self.latch_mode = byte() & 0x07;
        self.prg_protected = byte() != 0;
        self.mirroring_type = byte() & 0x03;
        self.latch = byte();
        self.mode_2m4m_enabled = byte() != 0;
        self.mode_2m = byte() != 0;
        self.chr_cc = byte() & 0x03;
        for i in 0..4 {
            self.reg_2m[i] = byte();
        }
        for i in 0..4 {
            self.reg_4m[i] = byte();
        }
        self.smc_mode = byte();
        for i in 0..12 {
            self.chr_1k[i] = byte();
        }
        self.mmc4_latch = [byte() != 0, byte() != 0];
        self.irq_counter = u16::from_le_bytes([byte(), byte()]);
        self.irq_enabled = byte() != 0;
        self.irq_pending = byte() != 0;
        self.fds_irq_enabled = byte() != 0;
        self.fds_irq_thirds = u16::from_le_bytes([byte(), byte()]);
        self.fds_irq_pending = byte() != 0;
        let vector = u16::from_le_bytes([byte(), byte()]);
        self.boot_vector = (byte() != 0).then_some(vector);
        self.stub_live = byte() != 0;
        self.last_a12 = byte() != 0;
        let mut rest = &data[c..];
        for buf in [
            &mut self.prg[..],
            &mut self.chr[..],
            &mut self.wram[..],
            &mut self.scratch[..],
        ] {
            let (now, later) = rest.split_at(buf.len());
            buf.copy_from_slice(now);
            rest = later;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    /// An image through `crate::parse`: `prg_16k` x 16 KiB PRG whose every
    /// 16 KiB page opens with its index, `chr_8k` x 8 KiB CHR likewise, NES
    /// 2.0 with `submapper` when given, and an optional 512-byte trainer.
    fn image(
        mapper: u8,
        prg_16k: usize,
        chr_8k: usize,
        submapper: Option<u8>,
        trainer: bool,
    ) -> Vec<u8> {
        let mut v = vec![0u8; 16];
        v[0..4].copy_from_slice(b"NES\x1A");
        v[4] = prg_16k as u8;
        v[5] = chr_8k as u8;
        v[6] = ((mapper & 0x0F) << 4) | 0x01 | if trainer { 0x04 } else { 0 };
        v[7] = mapper & 0xF0;
        if let Some(s) = submapper {
            v[7] |= 0x08;
            v[8] = s << 4;
        }
        if trainer {
            let mut t = vec![0xEAu8; TRAINER_LEN];
            t[0] = 0x4C; // a recognisable first byte
            v.extend_from_slice(&t);
        }
        for p in 0..prg_16k {
            let mut page = vec![0u8; BANK_16K];
            page[0] = p as u8;
            page[0x2000] = 0x80 | p as u8; // second 8 KiB of the page
            page[0x3FFC] = 0x34;
            page[0x3FFD] = 0x12;
            v.extend_from_slice(&page);
        }
        for b in 0..chr_8k {
            let mut bank = vec![0u8; BANK_8K];
            bank[0] = b as u8;
            bank[0x400] = 0x40 | b as u8;
            v.extend_from_slice(&bank);
        }
        v
    }

    fn board(
        mapper: u8,
        prg_16k: usize,
        chr_8k: usize,
        submapper: Option<u8>,
        trainer: bool,
    ) -> Box<dyn Mapper> {
        parse(&image(mapper, prg_16k, chr_8k, submapper, trainer))
            .unwrap_or_else(|e| panic!("mapper {mapper} must parse: {e:?}"))
            .1
    }

    #[test]
    fn m6_ines_defaults_to_un1rom_chrsw_with_page_7_fixed() {
        let mut m = board(6, 16, 0, None, false);
        assert_eq!(m.cpu_read(0xC000), 7, "latch mode 1: page 7 fixed");
        m.cpu_write(0x8000, 0b0010_1110); // PRG 11 (D5-D2), CHR 2 (D1-D0)
        assert_eq!(m.cpu_read(0x8000), 11);
        assert_eq!(m.cpu_read(0xC000), 7);
        m.ppu_write(0x0005, 0x99);
        assert_eq!(m.ppu_read(0x0005), 0x99, "CHR-RAM writable in mode 1");
        m.cpu_write(0x8000, 0b0010_1100); // CHR 0
        assert_ne!(m.ppu_read(0x0005), 0x99, "a different 8 KiB bank");
        assert_eq!(m.current_mirroring(), Mirroring::Vertical, "header V");
    }

    #[test]
    fn m6_submappers_select_the_latch_modes() {
        // 0 UNROM, 2 UOROM, 3 reverse UOROM, 4 GNROM, 5-7 fixed 32 KiB #3.
        let mut m = board(6, 32, 0, Some(0), false);
        m.cpu_write(0x8000, 0xFD);
        assert_eq!((m.cpu_read(0x8000), m.cpu_read(0xC000)), (5, 7));
        let mut m = board(6, 32, 0, Some(2), false);
        m.cpu_write(0x8000, 0x0C);
        assert_eq!((m.cpu_read(0x8000), m.cpu_read(0xC000)), (12, 15));
        let mut m = board(6, 32, 0, Some(3), false);
        m.cpu_write(0x8000, 0x1C); // CC = 1, PPPP = 12
        assert_eq!((m.cpu_read(0x8000), m.cpu_read(0xC000)), (15, 12));
        let mut m = board(6, 32, 4, Some(4), false);
        m.cpu_write(0x8000, 0x22); // PP = 2, CC = 2
        assert_eq!((m.cpu_read(0x8000), m.cpu_read(0xC000)), (4, 5));
        assert_eq!(m.ppu_read(0x0000), 2);
        m.ppu_write(0x0000, 0x77);
        assert_eq!(m.ppu_read(0x0000), 2, "GNROM mode: CHR write-protected");
        for sub in 5..=7 {
            let mut m = board(6, 32, 4, Some(sub), false);
            m.cpu_write(0x8000, 0x03);
            assert_eq!(
                (m.cpu_read(0x8000), m.cpu_read(0xC000)),
                (6, 7),
                "sub {sub}"
            );
            let want = match sub {
                5 => 3,
                6 => 1,
                _ => 0,
            };
            assert_eq!(m.ppu_read(0x0000), want, "sub {sub} CHR");
        }
    }

    #[test]
    fn m8_is_m6_submapper_4() {
        let mut m = board(8, 8, 4, None, false);
        m.cpu_write(0x8000, 0x11); // PP = 1, CC = 1
        assert_eq!((m.cpu_read(0x8000), m.cpu_read(0xC000)), (2, 3));
        assert_eq!(m.ppu_read(0x0000), 1);
    }

    #[test]
    fn mirroring_types_and_write_enable() {
        let mut m = board(6, 16, 0, None, false);
        m.cpu_write(0x42FE, 0x20); // A1 = 1, A0 = 0, D4 = 0: type 0
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        m.cpu_write(0x42FE, 0x30); // type 1
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
        m.cpu_write(0x42FF, 0x30); // type 3
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // A1 = 0: the DRAM is writable and the latch is off.
        m.cpu_write(0x42FD, 0x20);
        m.cpu_write(0x8001, 0xAB);
        assert_eq!(m.cpu_read(0x8001), 0xAB, "written into PRG DRAM");
        m.cpu_write(0x42FF, 0x20);
        assert_eq!(m.cpu_read(0x8001), 0xAB, "latch untouched by the write");
        assert_eq!(m.cpu_read(0x4500) & 0x3F, (1 << 3) | 0x02 | 0x01);
    }

    #[test]
    fn m17_starts_in_4m_mode_with_the_last_banks_and_1k_chr() {
        let mut m = board(17, 16, 16, None, false);
        // 32 8 KiB banks: $4504-$4507 = 28..31.
        assert_eq!(
            m.cpu_read(0x8000),
            14,
            "bank 28 = 16 KiB page 14, first half"
        );
        assert_eq!(m.cpu_read(0xA000), 0x8E, "bank 29");
        assert_eq!(m.cpu_read(0xE000), 0x8F, "bank 31");
        m.cpu_write(0x4505, 3);
        assert_eq!(m.cpu_read(0xA000), 0x81);
        // $8000 writes do not move the 4M banks.
        m.cpu_write(0x8000, 0xFC);
        assert_eq!(m.cpu_read(0x8000), 14);
        // 1 KiB CHR: $4511 picks the 1 KiB at $0400.
        m.cpu_write(0x4511, 9); // 8 KiB bank 1, second 1 KiB
        assert_eq!(m.ppu_read(0x0400), 0x40 | 1);
        // 2M mode: the four $8000-$FFFF registers, D7-D2.
        m.cpu_write(0x43FE, 0x00); // A0 = 0 enable, A1 = 1: 2M
        m.cpu_write(0xC000, 5 << 2);
        assert_eq!(
            m.cpu_read(0xC000),
            0x82,
            "2M: 8 KiB bank 5 = page 2's upper half"
        );
    }

    #[test]
    fn chr_nametables_when_n_is_clear() {
        let mut m = board(17, 16, 16, None, false);
        assert_eq!(m.nametable_fetch(0x2000), None, "N = 1: CIRAM");
        m.cpu_write(0x4500, 0x45); // N = 0
        m.cpu_write(0x4518, 1); // $2000 <- CHR 1 KiB page 1
        m.cpu_write(0x451B, 8); // $2C00 <- page 8
        assert_eq!(m.nametable_fetch(0x2000), Some(0x40));
        assert_eq!(m.nametable_fetch(0x2C00), Some(1));
        assert!(m.nametable_write(0x2C01, 0x5A));
        assert_eq!(m.nametable_fetch(0x2C01), Some(0x5A));
        assert_eq!(m.current_mirroring(), Mirroring::MapperControlled);
    }

    #[test]
    fn mmc4_mode_switches_4k_banks_after_the_latch_tiles() {
        let mut m = board(17, 16, 16, None, false);
        m.cpu_write(0x4500, 0x43); // C = 1, m = 0: MMC4 mode
        m.cpu_write(0x4510, 2 << 2); // $FD bank for $0000: 4 KiB bank 2
        m.cpu_write(0x4512, 4 << 2); // $FE bank: 4 KiB bank 4 (= 8 KiB bank 2)
        assert_eq!(m.ppu_read(0x0000), 1, "4 KiB bank 2 = 8 KiB bank 1");
        let _ = m.ppu_read(0x0FE8);
        assert_eq!(m.ppu_read(0x0000), 2, "after $0FE8: the $FE bank");
        assert!(!m.chr_reads_are_pure());
        let _ = m.ppu_read(0x0FD8);
        assert_eq!(m.ppu_read(0x0000), 1);
    }

    #[test]
    fn irq_counter_wraps_and_fds_timer_fires() {
        let mut m = board(6, 16, 0, None, false);
        m.cpu_write(0x4502, 0xFE);
        m.cpu_write(0x4503, 0xFF); // $FFFE, enabled, M2 source
        m.notify_cpu_cycle();
        assert!(!m.irq_pending());
        m.notify_cpu_cycle();
        assert!(m.irq_pending(), "$FFFF -> $0000");
        m.cpu_write(0x4501, 0);
        assert!(!m.irq_pending());
        // PA12 source.
        m.cpu_write(0x4500, 0x4A);
        m.cpu_write(0x4502, 0xFF);
        m.cpu_write(0x4503, 0xFF);
        m.notify_cpu_cycle();
        assert!(!m.irq_pending(), "M2 does not clock it");
        m.notify_a12(true);
        assert!(m.irq_pending(), "one PA12 rise");
        m.cpu_write(0x4501, 0);
        // FDS adapter: 149 1/3 CPU cycles.
        m.cpu_write(0x4025, 0x80);
        for _ in 0..149 {
            m.notify_cpu_cycle();
        }
        assert!(!m.irq_pending());
        m.notify_cpu_cycle();
        assert!(m.irq_pending(), "150th cycle");
        m.cpu_write(0x4024, 0);
        assert!(!m.irq_pending(), "$4024 acknowledges");
    }

    #[test]
    fn trainers_run_on_a_hard_reset_only() {
        // Mapper 6: JSR $7003 then JMP ($FFFC).
        let mut m = board(6, 16, 0, None, true);
        assert_eq!(m.cpu_read(0x7000), 0x4C, "trainer at $7000");
        assert_eq!(m.cpu_read(0xFFFC), 0x00);
        assert_eq!(m.cpu_read(0xFFFD), 0x72, "power-on vector -> $7200 stub");
        let stub: Vec<u8> = (0..6).map(|i| m.cpu_read(0x7200 + i)).collect();
        assert_eq!(stub, STUB);
        assert_eq!(m.cpu_read(0x7200), 0, "the stub is gone");
        assert_eq!(m.cpu_read(0xFFFD), 0x12, "the game's own vector");
        // Mapper 17 submapper 2: JMP to the trainer at $5E00.
        let mut m = board(17, 16, 16, Some(2), true);
        assert_eq!(m.cpu_read(0x5E00), 0x4C);
        assert_eq!((m.cpu_read(0xFFFC), m.cpu_read(0xFFFD)), (0x00, 0x5E));
        // A soft reset uses the ordinary vector.
        let mut m = board(17, 16, 16, None, true);
        m.reset();
        assert_ne!(m.cpu_read(0xFFFD), 0x70);
    }

    #[test]
    fn state_round_trips() {
        let mut a = board(17, 16, 16, None, true);
        a.cpu_write(0x4506, 7);
        a.cpu_write(0x6000, 0x31);
        a.cpu_write(0x5123, 0x32);
        a.ppu_write(0x0100, 0x33);
        let blob = a.save_state();
        let mut b = board(17, 16, 16, None, true);
        b.load_state(&blob).unwrap();
        assert_eq!(b.save_state(), blob);
        assert_eq!(b.cpu_read(0xC000), a.cpu_read(0xC000));
        assert_eq!(b.cpu_read(0x6000), 0x31);
        assert_eq!(b.cpu_read(0x5123), 0x32);
        assert_eq!(b.ppu_read(0x0100), 0x33);
        assert!(b.load_state(&blob[1..]).is_err());
    }
}

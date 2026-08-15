//! Namco 118 / DxROM (iNES mappers 206 and 88) implementation.
//!
//! Mapper 206 is the simpler predecessor of the MMC3: a `$8000` bank-select
//! + `$8001` bank-data register pair drives six bank registers, but there is
//! **no IRQ, no A12 clocking, no PRG-RAM, and no runtime mirroring control**
//! (the nametable arrangement is hardwired from the iNES header).
//!
//! | Reg | Purpose                                         |
//! |-----|-------------------------------------------------|
//! | R0  | 2 KiB CHR bank @ PPU `$0000-$07FF`              |
//! | R1  | 2 KiB CHR bank @ PPU `$0800-$0FFF`              |
//! | R2  | 1 KiB CHR bank @ PPU `$1000-$13FF`              |
//! | R3  | 1 KiB CHR bank @ PPU `$1400-$17FF`              |
//! | R4  | 1 KiB CHR bank @ PPU `$1800-$1BFF`              |
//! | R5  | 1 KiB CHR bank @ PPU `$1C00-$1FFF`              |
//! | R6  | 8 KiB PRG bank @ CPU `$8000-$9FFF`              |
//! | R7  | 8 KiB PRG bank @ CPU `$A000-$BFFF`              |
//!
//! The CHR layout is fixed (the left pattern table always gets the two 2 KiB
//! banks, the right table the four 1 KiB banks — no MMC3-style CHR mode bit),
//! and the last two 8 KiB PRG banks are always fixed at `$C000`/`$E000`.
//!
//! Mapper 206 limits PRG to 128 KiB and CHR to 64 KiB (bank-register width:
//! only bits 0-3 exist for the 8 KiB PRG regs and bits 1-5 / 0-5 for the CHR
//! regs). We mask the register values to the ROM size at use time, which is
//! equivalent in practice.
//!
//! Mapper 88 is identical to 206 except CHR is increased to 128 KiB by wiring
//! PPU A12 to CHR A16: tiles in `$0xxx` come from the first 64 KiB and tiles
//! in `$1xxx` from the second 64 KiB. We model this with the canonical
//! implementation note from the wiki: mask the 1 KiB bank index to `$3F` and
//! OR `$40` for the four 1 KiB registers (R2-R5). An undersize ROM on a
//! mapper-88 board behaves identically to mapper 206.
//!
//! See `docs/mappers.md`, `nesdev_wiki/INES_Mapper_206.xhtml`, and
//! `nesdev_wiki/INES_Mapper_088.xhtml`.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::doc_markdown,
    clippy::doc_lazy_continuation
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

// v2 appends the mirroring byte. Mapper 154 makes mirroring MUTABLE state (the
// $8000-$FFFF bit-6 nametable select), so a v1 blob -- which never carried it --
// would silently restore the wrong CIRAM page. 88/206 keep a constant mirroring
// and are unaffected in behaviour, but share the layout.
const SAVE_STATE_VERSION: u8 = 2;

/// Board variant for the Namco 118 family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namco118Board {
    /// Mapper 206 / DxROM base: 64 KiB CHR maximum, plain bank registers.
    Dxrom,
    /// Mapper 88: PPU A12 wired to CHR A16, splitting CHR into two disjoint
    /// 64 KiB halves between the left and right pattern tables.
    M88,
    /// Mapper 154 / NAMCOT-3453: mapper 88 plus one-screen nametable control.
    ///
    /// Identical to [`Namco118Board::M88`] -- including the A12/A16 CHR split --
    /// with a single added bit that selects between the two CIRAM pages. The
    /// board exists for exactly one game, *Devil Man*.
    M154,
}

/// Namco 118 / DxROM mapper (iNES 206 + 88).
pub struct Namco118 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,

    // R0..R5 (CHR) and R6/R7 (PRG) bank registers.
    regs: [u8; 8],
    bank_select: u8,

    mirroring: Mirroring,
    fixed_4screen: bool,
    board: Namco118Board,
}

impl Namco118 {
    /// Construct a new Namco 118 / DxROM mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 8 KiB; CHR-ROM must be a
    /// multiple of 1 KiB (CHR-RAM is allocated as 8 KiB when `chr_rom` is
    /// empty).
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        board: Namco118Board,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "Namco-118 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
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
                "Namco-118 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        let fixed_4screen = matches!(mirroring, Mirroring::FourScreen);
        let vram_size = if fixed_4screen {
            4 * NAMETABLE_SIZE
        } else {
            2 * NAMETABLE_SIZE
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; vram_size].into_boxed_slice(),
            chr_is_ram,
            regs: [0; 8],
            bank_select: 0,
            mirroring,
            fixed_4screen,
            board,
        })
    }

    /// PRG: R6 @ $8000, R7 @ $A000, then the last two 8 KiB banks fixed.
    fn prg_offset(&self, addr: u16) -> usize {
        let total = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let last = total - 1;
        let second_last = total.saturating_sub(2);
        let bank = match addr & 0xE000 {
            0x8000 => (self.regs[6] as usize) % total,
            0xA000 => (self.regs[7] as usize) % total,
            0xC000 => second_last,
            0xE000 => last,
            _ => 0,
        };
        bank * PRG_BANK_8K + (addr as usize & 0x1FFF)
    }

    /// CHR: fixed layout (no mode bit). R0/R1 supply the two 2 KiB banks
    /// covering `$0000-$0FFF`; R2-R5 supply the four 1 KiB banks covering
    /// `$1000-$1FFF`. On mapper 88 the four 1 KiB regs are OR'd with `$40`
    /// (after masking to `$3F`) so they index the second CHR half.
    fn chr_offset(&self, addr: u16) -> usize {
        let addr = (addr & 0x1FFF) as usize;
        let total_1k = (self.chr.len() / CHR_BANK_1K).max(1);
        let slot = addr / CHR_BANK_1K;
        let bank_1k: usize = match slot {
            // R0 = 2 KiB @ $0000 (low bit forced even by hardware).
            0 => (self.regs[0] as usize) & !1,
            1 => ((self.regs[0] as usize) & !1) | 1,
            // R1 = 2 KiB @ $0800.
            2 => (self.regs[1] as usize) & !1,
            3 => ((self.regs[1] as usize) & !1) | 1,
            // R2-R5 = 1 KiB each @ $1000-$1FFF.
            4 => self.m88_high(self.regs[2] as usize),
            5 => self.m88_high(self.regs[3] as usize),
            6 => self.m88_high(self.regs[4] as usize),
            7 => self.m88_high(self.regs[5] as usize),
            _ => 0,
        };
        let bank = bank_1k % total_1k;
        bank * CHR_BANK_1K + (addr & (CHR_BANK_1K - 1))
    }

    /// For mapper 88, the right-pattern-table 1 KiB registers select from the
    /// second 64 KiB CHR half (`bank & $3F | $40`). For DxROM, identity.
    fn m88_high(&self, reg: usize) -> usize {
        match self.board {
            // 154 is 88 in every respect but the nametable bit, including this.
            Namco118Board::M88 | Namco118Board::M154 => (reg & 0x3F) | 0x40,
            Namco118Board::Dxrom => reg,
        }
    }

    fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        if self.fixed_4screen {
            (table as usize) * NAMETABLE_SIZE + local
        } else {
            let physical = self.mirroring.physical_bank(table);
            physical * NAMETABLE_SIZE + local
        }
    }
}

impl Mapper for Namco118 {
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
        if let 0x8000..=0xFFFF = addr {
            // Mapper 154 (NAMCOT-3453) carries a one-screen nametable select in
            // bit 6, and the wiki is explicit that it is decoded across the WHOLE
            // $8000-$FFFF range -- unlike the bank-select register it shares a
            // byte with, which the associated Namco 108 only decodes at
            // $8000-$9FFF. So this is deliberately outside the even/odd split
            // below and sees every write, including bank-data writes to odd
            // addresses and writes above $9FFF.
            if self.board == Namco118Board::M154 {
                self.mirroring = if value & 0x40 == 0 {
                    Mirroring::SingleScreenA
                } else {
                    Mirroring::SingleScreenB
                };
            }
            // Register mask $E001: even = bank-select, odd = bank-data.
            // There are no control registers in $A000-$FFFF (unlike MMC3).
            if addr & 1 == 0 {
                self.bank_select = value & 0x07;
            } else {
                self.regs[(self.bank_select & 0x07) as usize] = value;
            }
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
                self.chr[off % self.chr.len()]
            }
            0x2000..=0x3EFF => self.vram[self.nametable_offset(addr) % self.vram.len()],
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
                let off = self.nametable_offset(addr) % self.vram.len();
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn nametable_address(&self, addr: u16) -> u16 {
        let off = self.nametable_offset(addr);
        u16::try_from(off & 0x07FF).unwrap_or(0)
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let id = match self.board {
            Namco118Board::M88 => 88,
            Namco118Board::M154 => 154,
            Namco118Board::Dxrom => 206,
        };
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: id,
            name: match self.board {
                Namco118Board::M88 => "Namco 118 (88)".into(),
                Namco118Board::M154 => "NAMCOT-3453 (154)".into(),
                Namco118Board::Dxrom => "DxROM / Namco 118 (206)".into(),
            },
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.prg_banks
            .push(("R6".into(), format!("{:#04x}", self.regs[6])));
        info.prg_banks
            .push(("R7".into(), format!("{:#04x}", self.regs[7])));
        for i in 0..6 {
            info.chr_banks
                .push((format!("R{i}"), format!("{:#04x}", self.regs[i])));
        }
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            11 + self.vram.len() + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.regs);
        out.push(self.bank_select);
        out.push(match self.mirroring {
            Mirroring::Horizontal => 0,
            Mirroring::Vertical => 1,
            Mirroring::SingleScreenA => 2,
            Mirroring::SingleScreenB => 3,
            Mirroring::FourScreen => 4,
            Mirroring::MapperControlled => 5,
        });
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 11 + self.vram.len() + need_chr;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.regs.copy_from_slice(&data[1..9]);
        self.bank_select = data[9];
        self.mirroring = match data[10] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            _ => self.mirroring,
        };
        let mut cursor = 11;
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

    fn synth_prg(banks_8k: usize) -> Box<[u8]> {
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
    fn prg_last_two_banks_fixed() {
        // 8 banks of 8 KiB = 64 KiB PRG.
        let mut m = Namco118::new(
            synth_prg(8),
            synth_chr(64),
            Mirroring::Vertical,
            Namco118Board::Dxrom,
        )
        .unwrap();
        assert_eq!(m.cpu_read(0xC000), 6); // second-to-last fixed
        assert_eq!(m.cpu_read(0xE000), 7); // last fixed
    }

    #[test]
    fn prg_bank_select_r6_r7() {
        let mut m = Namco118::new(
            synth_prg(8),
            synth_chr(64),
            Mirroring::Vertical,
            Namco118Board::Dxrom,
        )
        .unwrap();
        // Select R6 = 3.
        m.cpu_write(0x8000, 6);
        m.cpu_write(0x8001, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        // Select R7 = 5.
        m.cpu_write(0x8000, 7);
        m.cpu_write(0x8001, 5);
        assert_eq!(m.cpu_read(0xA000), 5);
        // Fixed banks unchanged.
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn chr_fixed_layout_2k_then_1k() {
        let mut m = Namco118::new(
            synth_prg(8),
            synth_chr(64),
            Mirroring::Vertical,
            Namco118Board::Dxrom,
        )
        .unwrap();
        // R0 (2 KiB @ $0000): write 4 -> even-forced bank 4 at $0000.
        m.cpu_write(0x8000, 0);
        m.cpu_write(0x8001, 4);
        assert_eq!(m.ppu_read(0x0000), 4);
        assert_eq!(m.ppu_read(0x0400), 5); // 2 KiB block, second 1 KiB
        // R2 (1 KiB @ $1000): write 9.
        m.cpu_write(0x8000, 2);
        m.cpu_write(0x8001, 9);
        assert_eq!(m.ppu_read(0x1000), 9);
    }

    #[test]
    fn m88_right_table_uses_second_chr_half() {
        // 128 KiB CHR = 128 1 KiB banks.
        let mut m = Namco118::new(
            synth_prg(8),
            synth_chr(128),
            Mirroring::Vertical,
            Namco118Board::M88,
        )
        .unwrap();
        // R2 (1 KiB @ $1000): write 5 -> (5 & 0x3F) | 0x40 = 0x45 = 69.
        m.cpu_write(0x8000, 2);
        m.cpu_write(0x8001, 5);
        assert_eq!(m.ppu_read(0x1000), 69);
        // R0 (left table) is unaffected by the A16 wiring.
        m.cpu_write(0x8000, 0);
        m.cpu_write(0x8001, 2);
        assert_eq!(m.ppu_read(0x0000), 2);
    }

    #[test]
    fn no_irq() {
        let mut m = Namco118::new(
            synth_prg(8),
            synth_chr(64),
            Mirroring::Vertical,
            Namco118Board::Dxrom,
        )
        .unwrap();
        // A12 / cpu-cycle notifications must never raise an IRQ.
        for _ in 0..1000 {
            m.notify_a12(true);
            m.notify_a12(false);
            m.notify_cpu_cycle();
        }
        assert!(!m.irq_pending());
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = Namco118::new(
            synth_prg(8),
            synth_chr(64),
            Mirroring::Horizontal,
            Namco118Board::Dxrom,
        )
        .unwrap();
        m.cpu_write(0x8000, 6);
        m.cpu_write(0x8001, 2);
        m.cpu_write(0x8000, 0);
        m.cpu_write(0x8001, 8);
        let blob = m.save_state();
        let mut m2 = Namco118::new(
            synth_prg(8),
            synth_chr(64),
            Mirroring::Horizontal,
            Namco118Board::Dxrom,
        )
        .unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.ppu_read(0x0000), m2.ppu_read(0x0000));
    }

    // ---- mapper 154 (NAMCOT-3453) ------------------------------------------

    fn m154() -> Namco118 {
        Namco118::new(
            synth_prg(8),
            synth_chr(128),
            Mirroring::Vertical,
            Namco118Board::M154,
        )
        .unwrap()
    }

    #[test]
    fn m154_bit6_selects_the_one_screen_page() {
        let mut m = m154();
        m.cpu_write(0x8000, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        m.cpu_write(0x8000, 0x40);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
    }

    #[test]
    fn m154_decodes_the_nametable_bit_across_the_whole_8000_ffff_range() {
        // The wiki is explicit that this bit is present over the entire 32 KiB
        // range, unlike the bank-select register it shares a byte with. An
        // implementation that folded it into the even-address arm would miss
        // every one of these.
        for addr in [0x8001u16, 0x9FFF, 0xA000, 0xC001, 0xFFFF] {
            let mut m = m154();
            m.cpu_write(0x8000, 0x00);
            assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
            m.cpu_write(addr, 0x40);
            assert_eq!(
                m.current_mirroring(),
                Mirroring::SingleScreenB,
                "bit 6 must be decoded at ${addr:04X}"
            );
        }
    }

    #[test]
    fn m154_keeps_the_mapper_88_chr_split() {
        // 154 is 88 in every respect but the nametable bit, so the right pattern
        // table must still come from the second 64 KiB half.
        let mut a = m154();
        let mut b = Namco118::new(
            synth_prg(8),
            synth_chr(128),
            Mirroring::Vertical,
            Namco118Board::M88,
        )
        .unwrap();
        for m in [&mut a, &mut b] {
            m.cpu_write(0x8000, 0x02); // select R2
            m.cpu_write(0x8001, 0x05);
        }
        assert_eq!(a.ppu_read(0x1000), b.ppu_read(0x1000));
        assert_eq!(a.ppu_read(0x1000), (0x05u8 & 0x3F) | 0x40);
    }

    #[test]
    fn m154_bank_writes_still_work_while_setting_mirroring() {
        // The nametable bit rides along with bank-data writes, so a value with
        // bit 6 set must still land in the selected register.
        let mut m = m154();
        m.cpu_write(0x8000, 0x06); // select R6 (PRG @ $8000)
        m.cpu_write(0x8001, 0x43); // bank 3, and bit 6 set
        assert_eq!(
            m.cpu_read(0x8000),
            3,
            "the bank register still took the value"
        );
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
    }

    #[test]
    fn m154_mirroring_survives_a_save_state() {
        // Mirroring is mutable on this board, so it has to be serialized -- a v1
        // blob never carried it and would restore the wrong CIRAM page.
        let mut m = m154();
        m.cpu_write(0x8000, 0x40);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
        let blob = m.save_state();

        let mut m2 = m154();
        m2.cpu_write(0x8000, 0x00);
        assert_eq!(m2.current_mirroring(), Mirroring::SingleScreenA);
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.current_mirroring(), Mirroring::SingleScreenB);
    }

    #[test]
    fn m88_ignores_bit6_so_it_is_unchanged() {
        let mut m = Namco118::new(
            synth_prg(8),
            synth_chr(128),
            Mirroring::Vertical,
            Namco118Board::M88,
        )
        .unwrap();
        m.cpu_write(0x8000, 0x40);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
//! Mapper 235: the "Golden Game" 150-in-1 / 260-in-1 address-latch multicart
//! (v3.2.0).
//!
//! Written from the vendored `NESdev` page `INES_Mapper_235` (The Mad
//! Dumper's notes plus the page's own description of the four TTL chips); no
//! reference-emulator source was read. Like other multicarts, "the address
//! written to is used instead of the actual data written":
//!
//! ```text
//! A15            A0
//! 1xMP RNBB xxxA AAAA
//!   || |||| |   +++++- 32 KiB page within the selected 1 MiB ROM
//!   || ||++----------- B: ROM socket (A8 = bit 0, A9 = bit 1)
//!   || |+------------- N: 1 = one-screen (CIRAM A10 held low: 1scA)
//!   || +-------------- R: 0 = 32 KiB at $8000, 1 = one 16 KiB half twice
//!   |+---------------- P: which 16 KiB half when R = 1
//!   +----------------- M: mirroring, 0 = horizontal, 1 = vertical
//! ```
//!
//! The page's chip description fixes the details: a 74'153 mux picks PRG A14
//! from CPU A14 (R = 0) or P (R = 1), and a second mux picks CIRAM A10 from
//! PPU A11 (M = 0) or PPU A10 (M = 1) with N as its enable, so N forces page
//! A. "All eleven bits of the register seem to be cleared on reset", so the
//! console RESET ([`Mapper::reset`]) clears it, as does power-on. CHR is 8 KiB
//! of CHR-RAM. No IRQ.
//!
//! **Sockets and the image.** The board has four 1 MiB ROM sockets; the
//! page's cartridge has two filled and says the valid selections are "ROM 0
//! and ROM 2 (i.e.: 00b = ROM 0, 10b = ROM 1)": socket 2 holds the second
//! chip. An image stores the filled chips in order, so this model maps:
//!
//! - a 4 MiB image: all four sockets, B indexing the image's 1 MiB quarters;
//! - a 2 MiB image: sockets 0 and 2 (the page's cartridge), sockets 1 and 3
//!   empty;
//! - 1 MiB or less: socket 0 only, the image wrapped within it.
//!
//! An empty socket drives nothing, so the CPU reads open bus there.

#![allow(clippy::cast_possible_truncation, clippy::missing_const_for_fn)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError, mirroring_name};
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
const SOCKET: usize = 1024 * 1024;
const CHR_RAM_8K: usize = 0x2000;
const SAVE_STATE_VERSION: u8 = 1;

/// Mapper 235 ("Golden Game" multicart). See the module docs.
pub struct GoldenGame235 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    /// The latched write address (A0-A13; A14 is don't-care, A15 always 1).
    reg: u16,
}

impl GoldenGame235 {
    /// Construct the board with the register cleared.
    ///
    /// # Errors
    ///
    /// [`MapperError::Invalid`] when PRG is not a non-zero multiple of 32 KiB
    /// or CHR-ROM is present (the board has CHR-RAM only).
    pub fn new(prg_rom: Box<[u8]>, chr_rom: &[u8]) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 235 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if !chr_rom.is_empty() {
            return Err(MapperError::Invalid(
                "mapper 235 has CHR-RAM only; this image carries CHR-ROM".into(),
            ));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_RAM_8K].into_boxed_slice(),
            vram: vec![0u8; 0x800].into_boxed_slice(),
            reg: 0,
        })
    }

    /// The image's 1 MiB chip behind socket `b`, or `None` for an empty one.
    fn chip(&self, b: usize) -> Option<usize> {
        match self.prg_rom.len() {
            l if l >= 4 * SOCKET => Some(b),
            l if l >= 2 * SOCKET => b.is_multiple_of(2).then_some(b / 2),
            _ => (b == 0).then_some(0),
        }
    }

    /// The image offset for CPU `addr`, or `None` when the socket is empty.
    fn prg_offset(&self, addr: u16) -> Option<usize> {
        let b = usize::from((self.reg >> 8) & 0x03);
        let chip = self.chip(b)?;
        let page = usize::from(self.reg & 0x1F);
        let a14 = if self.reg & 0x0800 != 0 {
            usize::from(self.reg & 0x1000 != 0) // R = 1: P
        } else {
            usize::from(addr & 0x4000 != 0) // R = 0: CPU A14
        };
        let off = chip * SOCKET + page * PRG_BANK_32K + a14 * PRG_BANK_16K;
        Some(off % self.prg_rom.len() + (usize::from(addr) & 0x3FFF))
    }

    fn mirroring(&self) -> Mirroring {
        if self.reg & 0x0400 != 0 {
            Mirroring::SingleScreenA
        } else if self.reg & 0x2000 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        }
    }

    fn nt_offset(&self, addr: u16) -> usize {
        let table = ((addr - 0x2000) / 0x400) & 0x03;
        self.mirroring().physical_bank(table as u8) * 0x400 + (usize::from(addr) & 0x3FF)
    }
}

impl Mapper for GoldenGame235 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    /// "All eleven bits of the register seem to be cleared on reset."
    fn reset(&mut self) {
        self.reg = 0;
    }

    fn may_unmap_prg_rom(&self) -> bool {
        // An image smaller than 4 MiB leaves sockets empty: they float.
        true
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        match addr {
            0x4020..=0x7FFF => true,
            0x8000..=0xFFFF => self.prg_offset(addr).is_none(),
            _ => false,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if addr >= 0x8000 {
            self.prg_offset(addr).map_or(0, |o| self.prg_rom[o])
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if addr >= 0x8000 {
            self.reg = addr & 0x3FFF;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[usize::from(addr)],
            0x2000..=0x3EFF => self.vram[self.nt_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_peek(&self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[usize::from(addr)],
            0x2000..=0x3EFF => self.vram[self.nt_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[usize::from(addr)] = value,
            0x2000..=0x3EFF => {
                let off = self.nt_offset(addr);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn nametable_address(&self, addr: u16) -> u16 {
        u16::try_from(self.nt_offset(addr)).unwrap_or(0)
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring()
    }

    fn debug_info(&self) -> MapperDebugInfo {
        let mut info = MapperDebugInfo {
            mapper_id: 235,
            name: "Golden Game multicart (235)".into(),
            mirroring: mirroring_name(self.mirroring()),
            ..Default::default()
        };
        info.prg_banks
            .push(("socket".into(), format!("{}", (self.reg >> 8) & 0x03)));
        info.prg_banks
            .push(("page".into(), format!("{:#04x}", self.reg & 0x1F)));
        info.prg_banks.push((
            "16K".into(),
            if self.reg & 0x0800 != 0 {
                if self.reg & 0x1000 != 0 {
                    "upper"
                } else {
                    "lower"
                }
            } else {
                "no (32 KiB)"
            }
            .into(),
        ));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.reg.to_le_bytes());
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::WrongLength {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.reg = u16::from_le_bytes([data[1], data[2]]) & 0x3FFF;
        let v = 3 + self.vram.len();
        self.vram.copy_from_slice(&data[3..v]);
        self.chr_ram.copy_from_slice(&data[v..]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    /// A mapper 235 image through `crate::parse` with `mib` MiB of PRG (NES
    /// 2.0, so 4 MiB is expressible); every 16 KiB page opens with its index
    /// low byte and the next byte with its index high byte.
    fn board(mib: usize) -> Box<dyn Mapper> {
        let pages = mib * 64;
        let mut v = vec![0u8; 16];
        v[0..4].copy_from_slice(b"NES\x1A");
        v[4] = (pages & 0xFF) as u8;
        v[5] = 0;
        v[6] = 0xB0; // 235 = 0xEB: low nibble 0xB, horizontal
        v[7] = 0xE8; // high nibble 0xE, NES 2.0
        v[9] = (pages >> 8) as u8; // PRG size MSB
        for p in 0..pages {
            let mut page = vec![0u8; PRG_BANK_16K];
            page[0] = p as u8;
            page[1] = (p >> 8) as u8;
            v.extend_from_slice(&page);
        }
        parse(&v)
            .unwrap_or_else(|e| panic!("mapper 235 must parse: {e:?}"))
            .1
    }

    fn page_at(m: &mut dyn Mapper, addr: u16) -> usize {
        usize::from(m.cpu_read(addr)) | (usize::from(m.cpu_read(addr + 1)) << 8)
    }

    /// The register address for the named fields (the page's letters).
    #[allow(clippy::fn_params_excessive_bools, clippy::many_single_char_names)]
    fn reg(m: bool, p: bool, r: bool, n: bool, b: u16, a: u16) -> u16 {
        0x8000
            | (u16::from(m) << 13)
            | (u16::from(p) << 12)
            | (u16::from(r) << 11)
            | (u16::from(n) << 10)
            | (b << 8)
            | a
    }

    #[test]
    fn m235_32k_and_16k_modes_from_the_address() {
        let mut m = board(4);
        assert_eq!(page_at(m.as_mut(), 0x8000), 0, "power-on: page 0");
        assert_eq!(page_at(m.as_mut(), 0xC000), 1);
        m.cpu_write(reg(false, false, false, false, 0, 5), 0xFF);
        assert_eq!(page_at(m.as_mut(), 0x8000), 10, "32 KiB page 5");
        assert_eq!(page_at(m.as_mut(), 0xC000), 11);
        m.cpu_write(reg(false, true, true, false, 0, 5), 0);
        assert_eq!(page_at(m.as_mut(), 0x8000), 11, "R = 1, P = 1: upper half");
        assert_eq!(page_at(m.as_mut(), 0xC000), 11, "twice");
        m.cpu_write(reg(false, false, true, false, 0, 5), 0);
        assert_eq!(page_at(m.as_mut(), 0xC000), 10, "P = 0: lower half");
        // A14 of the write address is don't-care.
        m.cpu_write(reg(false, false, false, false, 0, 3) | 0x4000, 0);
        assert_eq!(page_at(m.as_mut(), 0x8000), 6);
    }

    #[test]
    fn m235_sockets_on_4_2_and_1_mib_images() {
        let mut m = board(4);
        m.cpu_write(reg(false, false, false, false, 3, 1), 0);
        assert_eq!(page_at(m.as_mut(), 0x8000), 3 * 64 + 2, "4 MiB: socket 3");
        let mut m = board(2);
        m.cpu_write(reg(false, false, false, false, 2, 1), 0);
        assert_eq!(
            page_at(m.as_mut(), 0x8000),
            64 + 2,
            "2 MiB: socket 2 = chip 1"
        );
        m.cpu_write(reg(false, false, false, false, 1, 1), 0);
        assert!(m.cpu_read_unmapped(0x8000), "socket 1 empty: open bus");
        assert!(m.cpu_read_unmapped(0xFFFC));
        let mut m = board(1);
        m.cpu_write(reg(false, false, false, false, 2, 0), 0);
        assert!(m.cpu_read_unmapped(0x8000), "1 MiB: only socket 0");
        m.cpu_write(reg(false, false, false, false, 0, 0), 0);
        assert!(!m.cpu_read_unmapped(0x8000));
    }

    #[test]
    fn m235_mirroring_m_and_one_screen_n() {
        let mut m = board(2);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal, "M = 0");
        m.cpu_write(reg(true, false, false, false, 0, 0), 0);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical, "M = 1");
        m.cpu_write(reg(true, false, false, true, 0, 0), 0);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA, "N wins");
        m.cpu_write(reg(false, false, false, true, 0, 0), 0);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
    }

    #[test]
    fn m235_reset_clears_the_register() {
        let mut m = board(2);
        m.cpu_write(reg(true, true, true, true, 2, 9), 0);
        m.reset();
        assert_eq!(page_at(m.as_mut(), 0x8000), 0);
        assert_eq!(page_at(m.as_mut(), 0xC000), 1);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m235_chr_ram_and_state_round_trip() {
        let mut a = board(2);
        a.cpu_write(reg(true, false, false, false, 2, 7), 0);
        a.ppu_write(0x1FFF, 0x3C);
        a.ppu_write(0x2C00, 0x5D);
        let blob = a.save_state();
        let mut b = board(2);
        b.load_state(&blob).unwrap();
        assert_eq!(b.save_state(), blob);
        assert_eq!(page_at(b.as_mut(), 0x8000), page_at(a.as_mut(), 0x8000));
        assert_eq!(b.ppu_read(0x1FFF), 0x3C);
        assert_eq!(b.ppu_read(0x2C00), 0x5D);
    }

    #[test]
    fn m235_rejects_chr_rom_and_odd_prg() {
        assert!(GoldenGame235::new(vec![0; 0x4000].into(), &[]).is_err());
        assert!(GoldenGame235::new(vec![0; 0x8000].into(), &[0; 0x2000]).is_err());
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
//! Mapper 252: Waixing's VRC4-clone board for *San Guo Zhi* (v3.2.0).
//!
//! Written from the vendored `NESdev` page `INES_Mapper_252`; no
//! reference-emulator source was read. The board carries Waixing's Chinese
//! localization of Namcot's *Sangokushi: Chuugen no Hasha*. Per the page it
//! "uses a clone of the Konami VRC4, with the chip's A0 connected to CPU A2
//! and the chip's A1 connected to CPU A3 (`VRC4e`, Mapper 23 submapper 2). It
//! has 8 KiB of PRG-RAM at CPU `$6000-$7FFF`, and 1 KiB CHR pages 6 and 7 are
//! CHR-RAM."
//!
//! So the board is the project's own [`Vrc4`] (`m021_vrc4.rs`) wired as
//! mapper 23 submapper 2, which already supplies the PRG and CHR banking, the
//! mirroring register, the IRQ counter and the 8 KiB PRG-RAM, plus 2 KiB of
//! CHR-RAM: any 1 KiB PPU slot whose CHR bank register holds 6 reads and
//! writes the first 1 KiB of that RAM, and one holding 7 the second, instead
//! of CHR-ROM pages 6 and 7. That is the same "these bank numbers are RAM"
//! arrangement the page compares with the Waixing MMC3 boards (74, 192, 194,
//! 195).
//!
//! Mapper 253 is "the same, except that it's 1 KiB CHR pages 4 and 5 that are
//! CHR-RAM". The project's mapper 253 (`waixing.rs`) is an older, separately
//! derived model and is deliberately not reused here.
//!
//! The page states only the bank numbers, not how many bits of the VRC4's CHR
//! register take part in the match. This model compares the whole register
//! value (`== 6` / `== 7`), the plain reading of "pages 6 and 7"; *San Guo
//! Zhi* boots and plays under it.

use crate::cartridge::Mirroring;
use crate::m021_vrc4::Vrc4;
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError};
use alloc::{boxed::Box, format, vec, vec::Vec};

const CHR_BANK_1K: usize = 0x0400;
/// The two CHR bank numbers that select CHR-RAM instead of CHR-ROM.
const RAM_PAGES: [u8; 2] = [6, 7];
const SAVE_STATE_VERSION: u8 = 1;

/// Mapper 252 (Waixing VRC4 clone with CHR-RAM pages 6 and 7).
pub struct Waixing252 {
    inner: Vrc4,
    /// 2 KiB of CHR-RAM: pages 6 and 7.
    chr_ram: Box<[u8]>,
}

impl Waixing252 {
    /// Construct the board around a `VRC4e` (mapper 23 submapper 2) core.
    ///
    /// # Errors
    ///
    /// As [`Vrc4::new`]: PRG a non-zero multiple of 8 KiB, CHR-ROM a multiple
    /// of 1 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        Ok(Self {
            inner: Vrc4::new(prg_rom, chr_rom, 23, 2, mirroring)?,
            chr_ram: vec![0u8; 2 * CHR_BANK_1K].into_boxed_slice(),
        })
    }

    /// The CHR-RAM offset for pattern address `addr`, when its slot's bank is
    /// page 6 or 7.
    fn ram_offset(&self, addr: u16) -> Option<usize> {
        let slot = usize::from((addr >> 10) & 0x07);
        let bank = self.inner.chr_reg(slot);
        RAM_PAGES
            .iter()
            .position(|&p| p == bank)
            .map(|page| page * CHR_BANK_1K + usize::from(addr & 0x03FF))
    }
}

impl Mapper for Waixing252 {
    fn caps(&self) -> MapperCaps {
        self.inner.caps()
    }

    fn sram(&self) -> &[u8] {
        self.inner.sram()
    }

    fn sram_mut(&mut self) -> &mut [u8] {
        self.inner.sram_mut()
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        self.inner.cpu_read_unmapped(addr)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.inner.cpu_read(addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        self.inner.cpu_write(addr, value);
    }

    fn chr_phys(&self, addr: u16) -> Option<u32> {
        // CHR-RAM pages are content-hashed by the HD-pack path; ROM pages
        // fall back to it too, as the wrapped VRC4 does.
        let _ = addr;
        None
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        if addr < 0x2000
            && let Some(off) = self.ram_offset(addr)
        {
            return self.chr_ram[off];
        }
        self.inner.ppu_read(addr)
    }

    fn ppu_peek(&self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        if addr < 0x2000
            && let Some(off) = self.ram_offset(addr)
        {
            return self.chr_ram[off];
        }
        self.inner.ppu_peek(addr)
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if addr < 0x2000 {
            if let Some(off) = self.ram_offset(addr) {
                self.chr_ram[off] = value;
            }
            // Pages other than 6 and 7 are CHR-ROM: a write there goes nowhere.
            return;
        }
        self.inner.ppu_write(addr, value);
    }

    fn notify_cpu_cycle(&mut self) {
        self.inner.notify_cpu_cycle();
    }

    fn irq_pending(&self) -> bool {
        self.inner.irq_pending()
    }

    fn irq_acknowledge(&mut self) {
        self.inner.irq_acknowledge();
    }

    fn current_mirroring(&self) -> Mirroring {
        self.inner.current_mirroring()
    }

    fn debug_info(&self) -> MapperDebugInfo {
        let mut info = self.inner.debug_info();
        info.mapper_id = 252;
        info.name = "Waixing VRC4 clone, CHR-RAM pages 6/7 (252)".into();
        info.extra.push((
            "CHR-RAM slots".into(),
            format!(
                "{:?}",
                (0..8)
                    .filter(|&s| RAM_PAGES.contains(&self.inner.chr_reg(s)))
                    .collect::<Vec<_>>()
            ),
        ));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let inner = self.inner.save_state();
        let mut out = Vec::with_capacity(1 + self.chr_ram.len() + inner.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.chr_ram);
        out.extend_from_slice(&inner);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let head = 1 + self.chr_ram.len();
        if data.len() < head {
            return Err(MapperError::WrongLength {
                expected: head,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        // The inner blob validates itself before anything is written to the
        // core; the CHR-RAM is copied only once it has.
        self.inner.load_state(&data[head..])?;
        self.chr_ram.copy_from_slice(&data[1..head]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    /// A mapper 252 image through `crate::parse`: 256 KiB PRG, 128 KiB CHR
    /// whose every 1 KiB page opens with its index.
    fn board() -> Box<dyn Mapper> {
        let mut v = vec![0u8; 16];
        v[0..4].copy_from_slice(b"NES\x1A");
        v[4] = 16;
        v[5] = 16;
        v[6] = 0xC1; // 252 = 0xFC: low nibble 0xC, vertical
        v[7] = 0xF0;
        for b in 0..32u8 {
            let mut bank = vec![0u8; 0x2000];
            bank[0] = b;
            v.extend_from_slice(&bank);
        }
        for p in 0..128u8 {
            let mut page = vec![0u8; CHR_BANK_1K];
            page[0] = p;
            v.extend_from_slice(&page);
        }
        parse(&v)
            .unwrap_or_else(|e| panic!("mapper 252 must parse: {e:?}"))
            .1
    }

    /// Write a CHR bank register through the `VRC4e` decode: chip A0 = CPU A2,
    /// chip A1 = CPU A3. `base` is `$B000`..`$E000`, `second` picks the odd
    /// slot of the pair.
    fn chr(m: &mut dyn Mapper, base: u16, second: bool, value: u8) {
        let a1 = if second { 0x08 } else { 0 };
        m.cpu_write(base | a1, value & 0x0F); // low nibble (chip A0 = 0)
        m.cpu_write(base | a1 | 0x04, value >> 4); // high nibble (chip A0 = 1)
    }

    #[test]
    fn m252_vrc4e_decode_selects_chr_rom_pages() {
        let mut m = board();
        chr(m.as_mut(), 0xB000, false, 0x25);
        chr(m.as_mut(), 0xB000, true, 0x31);
        chr(m.as_mut(), 0xE000, true, 0x7F);
        assert_eq!(m.ppu_read(0x0000), 0x25);
        assert_eq!(m.ppu_read(0x0400), 0x31, "A3 selects the second slot");
        assert_eq!(m.ppu_read(0x1C00), 0x7F);
    }

    #[test]
    fn m252_pages_6_and_7_are_chr_ram_and_others_are_rom() {
        let mut m = board();
        chr(m.as_mut(), 0xB000, false, 6);
        chr(m.as_mut(), 0xB000, true, 7);
        chr(m.as_mut(), 0xC000, false, 5);
        chr(m.as_mut(), 0xC000, true, 4);
        m.ppu_write(0x0010, 0xAA); // slot 0 -> page 6 (RAM)
        m.ppu_write(0x0410, 0xBB); // slot 1 -> page 7 (RAM)
        m.ppu_write(0x0810, 0xCC); // slot 2 -> page 5 (ROM: ignored)
        assert_eq!(m.ppu_read(0x0010), 0xAA);
        assert_eq!(m.ppu_read(0x0410), 0xBB);
        assert_eq!(m.ppu_read(0x0810), 0x00, "ROM page 5 is not writable");
        assert_eq!(m.ppu_read(0x0800), 5, "ROM page 5");
        assert_eq!(m.ppu_read(0x0C00), 4, "ROM page 4 (253's RAM, not 252's)");
        // The same RAM page seen through another slot.
        chr(m.as_mut(), 0xE000, false, 7);
        assert_eq!(m.ppu_read(0x1810), 0xBB, "page 7 is one 1 KiB of RAM");
        // A bank number that only shares low bits with 6 is ROM.
        chr(m.as_mut(), 0xB000, false, 0x16);
        assert_eq!(m.ppu_read(0x0000), 0x16);
    }

    #[test]
    fn m252_prg_ram_and_irq_come_from_the_vrc4() {
        let mut m = board();
        m.cpu_write(0x6123, 0x5A);
        assert_eq!(m.cpu_read(0x6123), 0x5A, "8 KiB PRG-RAM");
        assert_eq!(m.sram().len(), 0x2000);
        // IRQ latch $FF, enable, M = 1 (cycle mode, `VRC_IRQ`): fires on the
        // next CPU cycle. Control is at chip A1 = CPU A3.
        m.cpu_write(0xF000, 0x0F);
        m.cpu_write(0xF004, 0x0F);
        m.cpu_write(0xF008, 0x06);
        assert!(!m.irq_pending());
        m.notify_cpu_cycle();
        assert!(m.irq_pending());
        assert_eq!(m.caps(), MapperCaps::CYCLE_IRQ);
    }

    #[test]
    fn m252_state_round_trips_with_chr_ram() {
        let mut a = board();
        chr(a.as_mut(), 0xD000, false, 6);
        a.ppu_write(0x1000, 0x42);
        a.cpu_write(0x6000, 0x99);
        let blob = a.save_state();
        let mut b = board();
        b.load_state(&blob).unwrap();
        assert_eq!(b.save_state(), blob);
        assert_eq!(b.ppu_read(0x1000), 0x42);
        assert_eq!(b.cpu_read(0x6000), 0x99);
        assert!(b.load_state(&blob[..10]).is_err());
    }
}

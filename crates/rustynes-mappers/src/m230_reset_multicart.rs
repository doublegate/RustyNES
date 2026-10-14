// SPDX-License-Identifier: GPL-3.0-or-later
//! Mapper 230: the reset-switched *22-in-1* multicart (v3.2.0).
//!
//! Written from the vendored `NESdev` page `INES_Mapper_230`, which carries
//! Disch's notes; no reference-emulator source was read. The cartridge holds
//! two PRG chips back to back in the image: chip 0, 128 KiB, is *Contra*;
//! chip 1, 512 KiB, is the multicart. "Performing a Soft Reset switches
//! between them" -- the console's RESET button is the mode switch, delivered
//! to the board through [`Mapper::reset`].
//!
//! ```text
//! Contra mode     $8000-$FFFF: [.... .PPP]   16 KiB P at $8000, chip-0 page 7
//!                                            fixed at $C000; mirroring vertical
//! Multicart mode  $8000-$FFFF: [.MOP PPPP]   M: mirroring (0 = H, 1 = V)
//!                                            O: PRG mode (0 = 32 KiB at
//!                                               P >> 1, 1 = 16 KiB P twice)
//!                                            P: 16 KiB page in chip 1
//! ```
//!
//! CHR is 8 KiB of CHR-RAM. No IRQ.
//!
//! **What the notes leave open, and the choices made:**
//!
//! - *Power-on mode.* The notes say only that a reset toggles it. This board
//!   powers on in Contra mode, chip 0 being the first chip in the image; a
//!   power cycle rebuilds the mapper, so it starts there again.
//! - *The register across a toggle.* "A soft reset changes which chip is used
//!   as well as other stuff relating to the mode", with no detail, so a reset
//!   clears the register to 0 in the new mode.
//! - *Mirroring field polarity in Contra mode.* "Mirroring is always Vert in
//!   Contra Mode", so the register's bit 6 is ignored there.
//! - *O polarity.* The PRG-setup table lists "Multi Mode 0" as one 32 KiB
//!   window and "Multi Mode 1" as two 16 KiB windows holding the same page.
//! - *Image layout.* "chip 0 = 128k PRG (offset 0x00010-0x2000F), chip 1 =
//!   512k PRG (offset 0x20010-0xA000F)": the first 128 KiB of PRG, then the
//!   next 512 KiB. Other image sizes wrap each chip's page into its own region.

#![allow(clippy::cast_possible_truncation, clippy::missing_const_for_fn)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError, mirroring_name};
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_16K: usize = 0x4000;
const CHR_RAM_8K: usize = 0x2000;
/// Chip 0 (*Contra*) is the first 128 KiB of the image.
const CHIP0_LEN: usize = 128 * 1024;
const SAVE_STATE_VERSION: u8 = 1;

/// Mapper 230 (22-in-1, reset-switched). See the module docs.
pub struct ResetMulticart230 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    /// `true` in multicart mode (chip 1), `false` in Contra mode (chip 0).
    multicart: bool,
    /// The last `$8000-$FFFF` write.
    reg: u8,
}

impl ResetMulticart230 {
    /// Construct the board in Contra mode.
    ///
    /// # Errors
    ///
    /// [`MapperError::Invalid`] when PRG is not larger than the 128 KiB Contra
    /// chip and a multiple of 16 KiB, or CHR-ROM is present (the board has
    /// CHR-RAM only).
    pub fn new(prg_rom: Box<[u8]>, chr_rom: &[u8]) -> Result<Self, MapperError> {
        if prg_rom.len() <= CHIP0_LEN || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 230 PRG-ROM size {} is not 128 KiB of Contra plus a multicart chip",
                prg_rom.len()
            )));
        }
        if !chr_rom.is_empty() {
            return Err(MapperError::Invalid(
                "mapper 230 has CHR-RAM only; this image carries CHR-ROM".into(),
            ));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_RAM_8K].into_boxed_slice(),
            vram: vec![0u8; 0x800].into_boxed_slice(),
            multicart: false,
            reg: 0,
        })
    }

    /// The image offset of the 16 KiB page `page` of the current chip.
    fn page_base(&self, page: usize) -> usize {
        if self.multicart {
            let chip1_pages = (self.prg_rom.len() - CHIP0_LEN) / PRG_BANK_16K;
            CHIP0_LEN + (page % chip1_pages) * PRG_BANK_16K
        } else {
            (page % (CHIP0_LEN / PRG_BANK_16K)) * PRG_BANK_16K
        }
    }

    /// The 16 KiB page mapped at `addr` (`$8000-$FFFF`).
    fn page_at(&self, addr: u16) -> usize {
        let upper = addr >= 0xC000;
        if self.multicart {
            let p = usize::from(self.reg & 0x1F);
            if self.reg & 0x20 != 0 {
                p // O = 1: the same 16 KiB page in both halves
            } else {
                (p & !1) | usize::from(upper) // O = 0: 32 KiB at P >> 1
            }
        } else if upper {
            7 // Contra: chip-0 page 7 fixed at $C000
        } else {
            usize::from(self.reg & 0x07)
        }
    }

    fn mirroring(&self) -> Mirroring {
        if self.multicart && self.reg & 0x40 == 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn nt_offset(&self, addr: u16) -> usize {
        let table = ((addr - 0x2000) / 0x400) & 0x03;
        self.mirroring().physical_bank(table as u8) * 0x400 + (usize::from(addr) & 0x3FF)
    }
}

impl Mapper for ResetMulticart230 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    /// The console RESET toggles between Contra and the multicart and clears
    /// the register (see the module docs).
    fn reset(&mut self) {
        self.multicart = !self.multicart;
        self.reg = 0;
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if addr >= 0x8000 {
            self.prg_rom[self.page_base(self.page_at(addr)) + (usize::from(addr) & 0x3FFF)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            self.reg = value;
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
            mapper_id: 230,
            name: "22-in-1 reset multicart (230)".into(),
            mirroring: mirroring_name(self.mirroring()),
            ..Default::default()
        };
        info.prg_banks.push((
            "mode".into(),
            if self.multicart {
                "multicart"
            } else {
                "Contra"
            }
            .into(),
        ));
        info.prg_banks
            .push(("$8000".into(), format!("{:#04x}", self.page_at(0x8000))));
        info.prg_banks
            .push(("$C000".into(), format!("{:#04x}", self.page_at(0xC000))));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(u8::from(self.multicart));
        out.push(self.reg);
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
        self.multicart = data[1] != 0;
        self.reg = data[2];
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

    /// The 22-in-1 geometry through `crate::parse`: 640 KiB PRG (Contra 128 +
    /// multicart 512), CHR-RAM. Every 16 KiB page opens with its image index.
    fn board() -> Box<dyn Mapper> {
        let mut v = vec![0u8; 16];
        v[0..4].copy_from_slice(b"NES\x1A");
        v[4] = 40;
        v[5] = 0;
        v[6] = 0x61; // 230 = 0xE6: low nibble 6, vertical
        v[7] = 0xE0;
        for p in 0..40u8 {
            let mut page = vec![0u8; PRG_BANK_16K];
            page[0] = p;
            v.extend_from_slice(&page);
        }
        parse(&v)
            .unwrap_or_else(|e| panic!("mapper 230 must parse: {e:?}"))
            .1
    }

    #[test]
    fn m230_powers_on_in_contra_mode_with_page_7_fixed() {
        let mut m = board();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 7, "chip-0 page 7 fixed");
        m.cpu_write(0x8000, 0xFD); // [.... .PPP]: only 3 bits
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xC000), 7);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical, "always V");
    }

    #[test]
    fn m230_reset_switches_to_the_multicart_chip_and_back() {
        let mut m = board();
        m.cpu_write(0x8000, 0x03);
        m.reset();
        // Register cleared, O = 0: 32 KiB at chip-1 pages 0/1 = image 8/9.
        assert_eq!(m.cpu_read(0x8000), 8);
        assert_eq!(m.cpu_read(0xC000), 9);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal, "M = 0");
        m.reset();
        assert_eq!(m.cpu_read(0x8000), 0, "back to Contra, register cleared");
        assert_eq!(m.cpu_read(0xC000), 7);
    }

    #[test]
    fn m230_multicart_register_decodes_m_o_p() {
        let mut m = board();
        m.reset();
        m.cpu_write(0x8000, 0x05); // O = 0, P = 5: 32 KiB pages 4/5
        assert_eq!(m.cpu_read(0x8000), 8 + 4);
        assert_eq!(m.cpu_read(0xC000), 8 + 5);
        m.cpu_write(0xFFFF, 0x20 | 0x05); // O = 1: page 5 twice
        assert_eq!(m.cpu_read(0x8000), 8 + 5);
        assert_eq!(m.cpu_read(0xC000), 8 + 5);
        m.cpu_write(0x8000, 0x40 | 0x1F); // M = 1, P = 31: last chip-1 pair
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        assert_eq!(m.cpu_read(0x8000), 8 + 30);
        assert_eq!(m.cpu_read(0xC000), 8 + 31);
    }

    #[test]
    fn m230_chr_ram_and_state_round_trip() {
        let mut a = board();
        a.reset();
        a.cpu_write(0x8000, 0x26);
        a.ppu_write(0x0123, 0x77);
        a.ppu_write(0x2400, 0x11);
        let blob = a.save_state();
        let mut b = board();
        b.load_state(&blob).unwrap();
        assert_eq!(b.save_state(), blob);
        assert_eq!(b.cpu_read(0x8000), a.cpu_read(0x8000));
        assert_eq!(b.ppu_read(0x0123), 0x77);
        assert_eq!(b.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m230_rejects_chr_rom_and_a_contra_only_image() {
        assert!(ResetMulticart230::new(vec![0; 0x20000].into(), &[]).is_err());
        assert!(ResetMulticart230::new(vec![0; 0xA0000].into(), &[0; 0x2000]).is_err());
    }
}

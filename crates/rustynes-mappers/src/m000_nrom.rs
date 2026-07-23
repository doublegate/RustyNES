//! NROM (iNES mapper 0) implementation.
//!
//! NROM is the trivial mapper: no bank registers, no banking, no IRQ. PRG-ROM
//! is either 16 KiB (mirrored across `$8000-$BFFF` and `$C000-$FFFF`) or
//! 32 KiB (filling `$8000-$FFFF`). CHR-ROM is a fixed 8 KiB window at
//! `$0000-$1FFF`; CHR-RAM variants substitute 8 KiB of writable RAM.
//!
//! Per `docs/mappers.md` §Mapper coverage matrix, NROM covers ~247 commercial
//! titles and is the baseline against which the rest of the mapper suite is
//! validated.

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

/// PRG-RAM size for cartridges that include it (Family Basic and a few
/// homebrews). Standard NROM has no PRG-RAM, but we always allocate the
/// 8 KiB window so reads/writes to `$6000-$7FFF` don't fall off the edge.
pub const NROM_PRG_RAM_SIZE: usize = 0x2000;

/// CHR-RAM size for the CHR-RAM variant of NROM. Always 8 KiB.
pub const NROM_CHR_RAM_SIZE: usize = 0x2000;

const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// NROM mapper state.
///
/// `prg_rom` and `chr_rom` are owned by the mapper rather than borrowed from
/// `Cartridge`. The cart-owned ROM bytes are passed in by value (boxed slice)
/// at construction; `Cartridge` retains its own copy as well so the rest of
/// the system can introspect ROM contents without going through the mapper.
pub struct Nrom {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    prg_ram: Box<[u8]>,
    /// Internal nametable VRAM (2 KiB). The console actually owns this in real
    /// hardware and the cartridge selects mirroring; we hold it here for now
    /// so the mapper can self-contain its PPU read/write surface until the
    /// PPU lands.
    vram: Box<[u8]>,
    chr_is_ram: bool,
    mirroring: Mirroring,
}

impl Nrom {
    /// Construct a new NROM mapper.
    ///
    /// `prg_rom` must be 16 KiB or 32 KiB. CHR-RAM is selected when `chr_rom`
    /// is empty; otherwise CHR-ROM must be exactly 8 KiB.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when sizes don't match the NROM
    /// constraints, since NROM has no banking to compensate.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.len() != PRG_BANK_16K && prg_rom.len() != PRG_BANK_32K {
            return Err(MapperError::Invalid(format!(
                "NROM expects 16 KiB or 32 KiB PRG-ROM, got {} bytes",
                prg_rom.len()
            )));
        }

        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; NROM_CHR_RAM_SIZE].into_boxed_slice()
        } else if chr_rom.len() == NROM_CHR_RAM_SIZE {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "NROM expects 8 KiB CHR-ROM, got {} bytes",
                chr_rom.len()
            )));
        };

        Ok(Self {
            prg_rom,
            chr,
            prg_ram: vec![0u8; NROM_PRG_RAM_SIZE].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            mirroring,
        })
    }

    /// Map a PPU address in `$2000-$3EFF` to an offset in the 2 KiB internal
    /// VRAM, applying the configured mirroring.
    const fn nametable_offset(&self, addr: u16) -> usize {
        // $2000-$3EFF mirrors. Internal VRAM is 2 KiB (two physical pages).
        let table = ((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = match self.mirroring {
            Mirroring::Horizontal => match table {
                0 | 1 => 0,
                _ => 1,
            },
            Mirroring::Vertical => match table {
                0 | 2 => 0,
                _ => 1,
            },
            Mirroring::SingleScreenA => 0,
            Mirroring::SingleScreenB => 1,
            // Four-screen and mapper-controlled fall back to vertical layout
            // here. NROM doesn't legally use either; this avoids panics on
            // misconfigured headers.
            Mirroring::FourScreen | Mirroring::MapperControlled => match table {
                0 | 2 => 0,
                _ => 1,
            },
        };
        physical * NAMETABLE_SIZE + local
    }
}

impl Mapper for Nrom {
    fn sram(&self) -> &[u8] {
        &self.prg_ram
    }
    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.prg_ram
    }
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xFFFF => {
                let idx = (addr - 0x8000) as usize;
                if self.prg_rom.len() == PRG_BANK_16K {
                    // Mirror 16 KiB across the full $8000-$FFFF window.
                    self.prg_rom[idx & (PRG_BANK_16K - 1)]
                } else {
                    self.prg_rom[idx]
                }
            }
            // $4020-$5FFF on stock NROM is open bus; report 0 here. The bus
            // layer will overlay open-bus once it owns the latch.
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if let 0x6000..=0x7FFF = addr {
            self.prg_ram[(addr - 0x6000) as usize] = value;
        }
        // Writes to PRG-ROM ($8000-$FFFF) are silently ignored on NROM.
    }

    fn chr_phys(&self, addr: u16) -> Option<u32> {
        // NROM CHR is a single unbanked 8 KiB window, so the absolute offset is
        // the pattern-space address itself. `None` for the CHR-RAM variant.
        if self.chr_is_ram {
            None
        } else {
            Some(u32::from(addr & 0x1FFF))
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[addr as usize],
            0x2000..=0x3EFF => {
                let off = self.nametable_offset(addr);
                self.vram[off]
            }
            // $3F00-$3FFF is palette RAM; owned by the PPU, not the mapper.
            // Returning 0 here matches the behavior expected of mappers that
            // never see palette accesses (PPU short-circuits).
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
                // CHR-ROM writes are ignored.
            }
            0x2000..=0x3EFF => {
                let off = self.nametable_offset(addr);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    // NROM has fixed solder-pad mirroring — a game-DB header correction is valid.
    fn has_hardwired_mirroring(&self) -> bool {
        true
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + self.prg_ram.len() + self.vram.len() + self.chr.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 1 + self.prg_ram.len() + self.vram.len() + need_chr;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let mut cursor = 1;
        self.prg_ram
            .copy_from_slice(&data[cursor..cursor + self.prg_ram.len()]);
        cursor += self.prg_ram.len();
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

    fn synth_prg_16k() -> Box<[u8]> {
        // Fill with i % 256 so we can verify the addressing across mirrors.
        (0..PRG_BANK_16K)
            .map(|i| (i & 0xFF) as u8)
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    fn synth_prg_32k() -> Box<[u8]> {
        (0..PRG_BANK_32K)
            .map(|i| ((i >> 4) & 0xFF) as u8)
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    fn synth_chr_8k() -> Box<[u8]> {
        (0..NROM_CHR_RAM_SIZE)
            .map(|i| ((i ^ 0xA5) & 0xFF) as u8)
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    #[test]
    fn nrom_16k_prg_mirrors_across_full_window() {
        let mut m = Nrom::new(synth_prg_16k(), synth_chr_8k(), Mirroring::Horizontal).unwrap();
        // $8000 mirrors $C000.
        for off in [0u16, 1, 0xFF, 0x1234, 0x3FFF] {
            let lo = m.cpu_read(0x8000 + off);
            let hi = m.cpu_read(0xC000 + off);
            assert_eq!(lo, hi, "mirror differs at offset {off:#06x}");
            assert_eq!(lo, (off & 0xFF) as u8);
        }
    }

    #[test]
    fn nrom_32k_prg_does_not_mirror() {
        let mut m = Nrom::new(synth_prg_32k(), synth_chr_8k(), Mirroring::Vertical).unwrap();
        for off in 0u16..0x8000u16 {
            let want = ((off as usize >> 4) & 0xFF) as u8;
            assert_eq!(m.cpu_read(0x8000 + off), want);
        }
    }

    #[test]
    fn nrom_chr_rom_writes_ignored() {
        let mut m = Nrom::new(synth_prg_16k(), synth_chr_8k(), Mirroring::Horizontal).unwrap();
        let before = m.ppu_read(0x0123);
        m.ppu_write(0x0123, before.wrapping_add(1));
        assert_eq!(m.ppu_read(0x0123), before);
    }

    #[test]
    fn nrom_chr_ram_writes_round_trip() {
        let mut m = Nrom::new(
            synth_prg_16k(),
            Vec::new().into_boxed_slice(),
            Mirroring::Horizontal,
        )
        .unwrap();
        for addr in [0x0000u16, 0x07FF, 0x1234, 0x1FFF] {
            m.ppu_write(addr, addr as u8);
        }
        for addr in [0x0000u16, 0x07FF, 0x1234, 0x1FFF] {
            assert_eq!(m.ppu_read(addr), addr as u8);
        }
    }

    #[test]
    fn nrom_prg_ram_round_trip() {
        let mut m = Nrom::new(synth_prg_16k(), synth_chr_8k(), Mirroring::Horizontal).unwrap();
        m.cpu_write(0x6000, 0xAB);
        m.cpu_write(0x6FFF, 0xCD);
        m.cpu_write(0x7FFF, 0xEF);
        assert_eq!(m.cpu_read(0x6000), 0xAB);
        assert_eq!(m.cpu_read(0x6FFF), 0xCD);
        assert_eq!(m.cpu_read(0x7FFF), 0xEF);
    }

    #[test]
    fn nrom_horizontal_mirroring() {
        let mut m = Nrom::new(synth_prg_16k(), synth_chr_8k(), Mirroring::Horizontal).unwrap();
        // Top row: $2000-$23FF and $2400-$27FF -> physical bank 0.
        m.ppu_write(0x2000, 0x11);
        assert_eq!(m.ppu_read(0x2400), 0x11);
        // Bottom row: $2800-$2BFF and $2C00-$2FFF -> physical bank 1.
        m.ppu_write(0x2800, 0x22);
        assert_eq!(m.ppu_read(0x2C00), 0x22);
        // Top vs bottom must not alias.
        assert_ne!(m.ppu_read(0x2000), m.ppu_read(0x2800));
    }

    #[test]
    fn nrom_vertical_mirroring() {
        let mut m = Nrom::new(synth_prg_16k(), synth_chr_8k(), Mirroring::Vertical).unwrap();
        m.ppu_write(0x2000, 0x33);
        assert_eq!(m.ppu_read(0x2800), 0x33);
        m.ppu_write(0x2400, 0x44);
        assert_eq!(m.ppu_read(0x2C00), 0x44);
        assert_ne!(m.ppu_read(0x2000), m.ppu_read(0x2400));
    }

    #[test]
    fn nrom_save_state_round_trip_chr_ram() {
        let mut m = Nrom::new(
            synth_prg_16k(),
            Vec::new().into_boxed_slice(),
            Mirroring::Horizontal,
        )
        .unwrap();
        m.cpu_write(0x6010, 0xBE);
        m.ppu_write(0x0010, 0xEF);
        m.ppu_write(0x2010, 0xCA);
        let blob = m.save_state();

        let mut m2 = Nrom::new(
            synth_prg_16k(),
            Vec::new().into_boxed_slice(),
            Mirroring::Horizontal,
        )
        .unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x6010), 0xBE);
        assert_eq!(m2.ppu_read(0x0010), 0xEF);
        assert_eq!(m2.ppu_read(0x2010), 0xCA);
    }

    #[test]
    fn nrom_rejects_bad_prg_size() {
        let prg = vec![0u8; 0x2000].into_boxed_slice();
        assert!(Nrom::new(prg, synth_chr_8k(), Mirroring::Horizontal).is_err());
    }
}

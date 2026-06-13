//! Konami VS (iNES mapper 151) implementation.
//!
//! Mapper 151 is **Konami's VRC1 silicon mounted on a Nintendo Vs. System
//! arcade board** (Vs. Gradius / GVS VS. TKO Boxing). The bank-switching
//! behaviour is byte-for-byte VRC1 (mapper 75): three switchable 8 KiB PRG
//! banks at `$8000`/`$A000`/`$C000` with a fixed last bank, two 4 KiB CHR
//! windows, a CHR-MSB bit per window, and `$9000`-bit-0 H/V mirroring control.
//!
//! The only thing that distinguishes mapper 151 from mapper 75 is the *console*
//! it runs on: the Vs. board replaces the 2C02 composite PPU with a 2C03 RGB
//! PPU. That platform detail is applied in [`crate::parse`] (which forces
//! [`crate::ConsoleType::VsSystem`] + the 2C03 RGB PPU for a mapper-151 cart,
//! exactly as it does for mapper 99). This wrapper exists so mapper 151 is a
//! first-class, dispatch-visible mapper family that reuses the proven VRC1
//! core rather than re-implementing it.
//!
//! See `docs/mappers.md` §Mapper coverage matrix.

#![allow(clippy::doc_markdown)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError};
use crate::sprint2::Vrc1;
use alloc::{boxed::Box, vec::Vec};

/// Konami VS (Mapper 151) — VRC1 on Vs. System hardware.
pub struct KonamiVs {
    inner: Vrc1,
}

impl KonamiVs {
    /// Construct a new Konami VS (mapper 151) mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on a PRG / CHR size mismatch (forwarded
    /// from the VRC1 core).
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        Ok(Self {
            inner: Vrc1::new(prg_rom, chr_rom, mirroring)?,
        })
    }
}

impl Mapper for KonamiVs {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.inner.cpu_read(addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        self.inner.cpu_write(addr, value);
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        self.inner.ppu_read(addr)
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        self.inner.ppu_write(addr, value);
    }

    fn current_mirroring(&self) -> Mirroring {
        self.inner.current_mirroring()
    }

    fn save_state(&self) -> Vec<u8> {
        self.inner.save_state()
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        self.inner.load_state(data)
    }

    fn debug_info(&self) -> MapperDebugInfo {
        let mut info = self.inner.debug_info();
        info.mapper_id = 151;
        info.name = "Konami VS / VRC1 (151)".into();
        info
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;
    use alloc::vec;

    const PRG_BANK_8K: usize = 0x2000;
    const CHR_BANK_4K: usize = 0x1000;

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
    fn delegates_vrc1_banking() {
        let mut m = KonamiVs::new(synth(8), synth_chr_4k(2), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        // $E000 is the fixed last bank.
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn debug_info_reports_mapper_151() {
        let m = KonamiVs::new(synth(8), synth_chr_4k(2), Mirroring::Vertical).unwrap();
        assert_eq!(m.debug_info().mapper_id, 151);
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = KonamiVs::new(synth(8), synth_chr_4k(2), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 5);
        m.cpu_write(0x9000, 0x01); // mirroring -> Horizontal
        let blob = m.save_state();
        let mut m2 = KonamiVs::new(synth(8), synth_chr_4k(2), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.current_mirroring(), m2.current_mirroring());
    }
}

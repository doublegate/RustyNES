//! Camerica / Codemasters BF9093 and relatives (mapper 71).
//!
//! A UxROM-shaped board: a 16 KiB PRG bank selected at `$C000-$FFFF` with the
//! last bank fixed, CHR-RAM, and no IRQ. Nothing else answers in
//! `$8000-$BFFF` on the BF9093. The Fire Hawk variant (the BF9097)
//! additionally decodes a single-screen mirroring bit at `$8000-$9FFF`;
//! detected on submapper 0, it answers only at `$9000-$9FFF`. Sources: the
//! `NESdev` page `INES_Mapper_071` and Kevin Horton's "Camerica PCBs and
//! Mappers" notes.
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

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

/// Camerica / Codemasters BF9093 (Mapper 71).
pub struct Camerica {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    mirroring: Mirroring,
    /// The `$9000` single-screen register is live (Fire Hawk's BF9097).
    /// Saved state: detection can turn it on mid-run.
    has_single_screen: bool,
    /// Turn the register on at the first `$9000-$9FFF` write (submapper 0;
    /// see [`Self::with_single_screen_detection`]). Configuration from the
    /// header, not state, so it is not saved.
    detect_single_screen: bool,
    /// The board is the BF9097 by its header (NES 2.0 submapper 1), whose
    /// register also answers at `$8000-$8FFF`. Configuration, not saved.
    bf9097: bool,
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
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
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
            detect_single_screen: false,
            bf9097: has_single_screen,
        })
    }

    /// Enable Fire Hawk detection: the board keeps its header mirroring
    /// until the first write to `$9000-$9FFF`, then behaves as the BF9097,
    /// whose register there selects single-screen mirroring.
    ///
    /// nesdev "INES Mapper 071": only Fire Hawk has the register, it writes
    /// it only at `$9000`, and "for compatibility without using a submapper"
    /// a board starts with fixed mirroring and applies single-screen once
    /// `$9000-$9FFF` is written. NES 2.0 submapper 1 names Fire Hawk
    /// outright (the `has_single_screen` constructor flag). Used for
    /// submapper 0 only (maintainer ruling, 2026-10-13) -- which covers every
    /// iNES 1.0 image, where the submapper reads as 0. Added in v3.2.0.
    #[must_use]
    pub const fn with_single_screen_detection(mut self, on: bool) -> Self {
        self.detect_single_screen = on;
        self
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
        // Kevin Horton's Camerica notes and the NESdev page agree: "Writing
        // anywhere to C000-FFFFh will switch in a 16K PRG ROM bank", and the
        // BF9093 has no register at `$8000-$BFFF` (`T-MAPPER71-DECODE`,
        // v3.3.0; that range selected the bank too before). The BF9097's
        // mirroring register spans `$8000-$9FFF` ("Writing anywhere in the
        // 8000-9FFFh area will select a nametable"), which submapper 1 names;
        // submapper 0's detection keeps the page's `$9000-$9FFF`, so another
        // game's `$00` to `$8000` at startup cannot switch it on.
        let mirroring_register = match addr {
            0x8000..=0x8FFF => self.bf9097,
            0x9000..=0x9FFF => self.has_single_screen || self.detect_single_screen,
            _ => false,
        };
        if mirroring_register {
            self.has_single_screen = true;
            self.mirroring = if value & 0x10 == 0 {
                Mirroring::SingleScreenA
            } else {
                Mirroring::SingleScreenB
            };
        } else if addr >= 0xC000 {
            self.prg_bank = value & 0x0F;
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

    fn ppu_peek(&self, addr: u16) -> u8 {
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
            return Err(MapperError::WrongLength {
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

#[cfg(test)]
mod tests {
    use super::*;

    const PRG_BANK_8K: usize = 0x2000;

    fn synth(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * PRG_BANK_8K];
        for b in 0..banks_8k {
            v[b * PRG_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
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

    /// nesdev "INES Mapper 071" / "NES 2.0 submappers" (071): only Fire Hawk's
    /// BF9097 has the `$9000` single-screen register, NES 2.0 submapper 1
    /// names it, and "for compatibility without using a submapper" a board
    /// starts with its header mirroring and switches to single-screen once
    /// `$9000-$9FFF` is written. Maintainer ruling (2026-10-13): that
    /// detection applies on submapper 0 only.
    #[test]
    fn submapper_0_detects_fire_hawk_single_screen_on_a_9000_write() {
        let mut m = Camerica::new(synth(16), Mirroring::Vertical, false)
            .unwrap()
            .with_single_screen_detection(true);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical, "header first");
        m.cpu_write(0x9000, 0x10);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
        m.cpu_write(0x9000, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        assert_eq!(m.cpu_read(0x8000), 0, "a $9000 write is not a bank select");
        // The detected mode survives a state round trip.
        let state = m.save_state();
        let mut other = Camerica::new(synth(16), Mirroring::Vertical, false)
            .unwrap()
            .with_single_screen_detection(true);
        other.load_state(&state).unwrap();
        assert_eq!(other.current_mirroring(), Mirroring::SingleScreenA);
    }

    /// `T-MAPPER71-DECODE`: the bank select is `$C000-$FFFF` only. Kevin
    /// Horton's Camerica notes, for all three chips: "Writing anywhere to
    /// C000-FFFFh will switch in a 16K PRG ROM bank at 8000-BFFFh", and the
    /// BF9093 has no other register; the `NESdev` page agrees ("Bank select
    /// ($C000-$FFFF)") and names *Micro Machines* and *Ultimate Stuntman* as
    /// writing `$00` to `$8000` at startup. Until v3.3.0 a write anywhere in
    /// `$8000-$BFFF` selected the bank too.
    #[test]
    fn only_c000_to_ffff_selects_the_bank() {
        for sub1 in [false, true] {
            let mut m = Camerica::new(synth(16), Mirroring::Vertical, sub1).unwrap();
            m.cpu_write(0xC000, 3);
            assert_eq!(m.cpu_read(0x8000), 6, "16 KiB bank 3 (8 KiB bank 6)");
            for addr in [0x8000u16, 0x9000, 0xA000, 0xBFFF] {
                m.cpu_write(addr, 5);
                assert_eq!(
                    m.cpu_read(0x8000),
                    6,
                    "a ${addr:04X} write is no bank select"
                );
            }
            m.cpu_write(0xFFFF, 5);
            assert_eq!(m.cpu_read(0x8000), 10, "$FFFF selects 16 KiB bank 5");
        }
    }

    /// The BF9097 (*Fire Hawk*, submapper 1) decodes its mirroring register
    /// across `$8000-$9FFF` ("Writing anywhere in the 8000-9FFFh area will
    /// select a nametable. Bit 4 of the byte written selects it", Kevin
    /// Horton; `NESdev`: "Mirroring ($8000-$9FFF)"). Submapper 0's detection
    /// keeps the narrower `$9000-$9FFF` the page gives for compatibility, so
    /// another game's `$00` to `$8000` cannot turn it on.
    #[test]
    fn submapper_1_mirroring_spans_8000_to_9fff() {
        let mut m = Camerica::new(synth(16), Mirroring::Vertical, true).unwrap();
        m.cpu_write(0x8000, 0x10);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
        m.cpu_write(0x9FFF, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        let mut d = Camerica::new(synth(16), Mirroring::Vertical, false)
            .unwrap()
            .with_single_screen_detection(true);
        d.cpu_write(0x8000, 0x10);
        assert_eq!(
            d.current_mirroring(),
            Mirroring::Vertical,
            "no detection at $8000"
        );
    }

    /// Submapper 1 (Fire Hawk) has the register from power-on.
    #[test]
    fn submapper_1_has_the_register_from_power_on() {
        let mut m = Camerica::new(synth(16), Mirroring::Vertical, true).unwrap();
        m.cpu_write(0x9000, 0x10);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
    }

    /// The header wiring in `parse`: an iNES 1.0 image (submapper 0) detects,
    /// NES 2.0 submapper 1 has the register from power-on, and NES 2.0
    /// submapper 2 (no meaning assigned) keeps its header mirroring.
    #[test]
    fn parse_wires_detection_from_the_submapper() {
        fn rom(nes2_sub: Option<u8>) -> Vec<u8> {
            let mut r = vec![0u8; 16 + 128 * 1024];
            r[..4].copy_from_slice(b"NES\x1A");
            r[4] = 8; // 128 KiB PRG, CHR-RAM
            r[6] = 0x71; // mapper 71 low nibble, vertical
            r[7] = 0x40; // mapper 71 high nibble
            if let Some(sub) = nes2_sub {
                r[7] |= 0x08;
                r[8] = sub << 4;
            }
            r
        }
        for (sub, expect) in [
            (None, Mirroring::SingleScreenB),
            (Some(1), Mirroring::SingleScreenB),
            (Some(2), Mirroring::Vertical),
        ] {
            let (_cart, mut m) = crate::parse(&rom(sub)).expect("mapper 71 parses");
            m.cpu_write(0x9000, 0x10);
            assert_eq!(m.current_mirroring(), expect, "submapper {sub:?}");
        }
    }

    /// Any other submapper keeps its header mirroring: no detection.
    #[test]
    fn without_detection_a_9000_write_keeps_the_header_mirroring() {
        let mut m = Camerica::new(synth(16), Mirroring::Vertical, false).unwrap();
        m.cpu_write(0x9000, 0x10);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }
}

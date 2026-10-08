//! Nintendo MMC4 (`FxROM`, mapper 10) -- Fire Emblem, Famicom Wars.
//!
//! Carries the same *tile-fetch CHR latch* as the MMC2 in `m009_mmc2.rs`: a
//! pattern fetch of tile `$FD` or `$FE` latches that pattern half to one of two
//! CHR banks, switching CHR mid-scanline without CPU involvement.
//!
//! It differs from MMC2 in PRG layout -- 16 KiB switchable plus 16 KiB fixed,
//! rather than 8 KiB plus three fixed -- and in carrying battery-backed
//! PRG-RAM, which is why the save-bearing Konami titles live on this board.
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
const CHR_BANK_4K: usize = 0x1000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

/// Version byte this board writes in its mapper save-state section.
///
/// **v1** (through v2.9.1) carried the PRG bank, the four CHR bank registers,
/// both latches, the mirroring and the 2 KiB nametable RAM -- and nothing
/// else. The 8 KiB PRG-RAM at `$6000-$7FFF` (the *Fire Emblem* battery save
/// RAM) and, on a cartridge with no CHR-ROM, the 8 KiB CHR-RAM were left out,
/// and the `.rns` container has no other section that carries cartridge RAM
/// -- so every save-state load, rewind step, run-ahead frame and netplay
/// rollback kept whatever RAM the running game held instead of the saved one
/// (the v2.9.2 cartridge-RAM sweep; the same omission core audit AUD-02 found
/// on the Konami VRC boards). **v2** appends the PRG-RAM, then the CHR-RAM
/// when present. Since v2.9.8 (ADR 0042)
/// `load_state` reads v2 only and refuses a v1 blob, which it used to load
/// with the RAM left untouched.
const MMC4_SECTION_VERSION: u8 = 2;

fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

/// MMC4 (Mapper 10).
pub struct Mmc4 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_lo_fd: u8,
    chr_lo_fe: u8,
    chr_hi_fd: u8,
    chr_hi_fe: u8,
    latch_lo_fe: bool,
    latch_hi_fe: bool,
    mirroring: Mirroring,
    /// 8 KiB WRAM at $6000-$7FFF (battery-backed on most MMC4 carts).
    /// T-60-003c (2026-05-17) — same root cause as the VRC2/4/6 WRAM
    /// fix in `m022_vrc2.rs` / `m021_vrc4.rs`. Fire Emblem Gaiden was stuck-at-uniform-
    /// gray for the same reason (read its save magic from WRAM at
    /// boot, got 0, stalled in save-validation).
    prg_ram: Box<[u8]>,
}

impl Mmc4 {
    /// Construct a new MMC4 mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "MMC4 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_4K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "MMC4 CHR-ROM size {} is not a multiple of 4 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_lo_fd: 0,
            chr_lo_fe: 0,
            chr_hi_fd: 0,
            chr_hi_fe: 0,
            latch_lo_fe: false,
            latch_hi_fe: false,
            mirroring,
            // 8 KiB WRAM at $6000-$7FFF (T-60-003c).
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
        })
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total_16k = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let last = total_16k - 1;
        let bank = if (addr & 0xC000) == 0x8000 {
            (self.prg_bank as usize) % total_16k
        } else {
            last
        };
        bank * PRG_BANK_16K + ((addr as usize) & 0x3FFF)
    }

    fn chr_offset(&mut self, addr: u16) -> usize {
        let addr = (addr & 0x1FFF) as usize;
        let total_4k = (self.chr_rom.len() / CHR_BANK_4K).max(1);
        let bank = if addr < CHR_BANK_4K {
            let b = if self.latch_lo_fe {
                self.chr_lo_fe
            } else {
                self.chr_lo_fd
            };
            (b as usize) % total_4k
        } else {
            let b = if self.latch_hi_fe {
                self.chr_hi_fe
            } else {
                self.chr_hi_fd
            };
            (b as usize) % total_4k
        };
        bank * CHR_BANK_4K + (addr & (CHR_BANK_4K - 1))
    }

    fn update_latch(&mut self, addr: u16) {
        match addr & 0x3FF8 {
            0x0FD8 => self.latch_lo_fe = false,
            0x0FE8 => self.latch_lo_fe = true,
            0x1FD8 => self.latch_hi_fe = false,
            0x1FE8 => self.latch_hi_fe = true,
            _ => {}
        }
    }
}

impl Mapper for Mmc4 {
    /// v3.1.0: not pure -- a read of tile $FD/$FE switches its CHR latch, so the PPU's display-only
    /// "disable sprite limit" reads are not made on this board.
    fn chr_reads_are_pure(&self) -> bool {
        false
    }

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
            // T-60-003c (2026-05-17): MMC4 carts (Fire Emblem proper +
            // Fire Emblem Gaiden + Famicom Wars) include 8 KiB battery-
            // backed WRAM at $6000-$7FFF. Pre-fix returned 0; FE
            // Gaiden's save-validation path stalled. Same root cause
            // as the VRC2/4/6 fix in m022_vrc2.rs / m021_vrc4.rs / m024_vrc6.rs.
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize % self.prg_ram.len()],
            0x8000..=0xFFFF => {
                let off = self.prg_offset(addr);
                self.prg_rom[off % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // T-60-003c (2026-05-17): WRAM at $6000-$7FFF (paired with
        // the read fix above).
        if (0x6000..=0x7FFF).contains(&addr) {
            let len = self.prg_ram.len();
            self.prg_ram[(addr - 0x6000) as usize % len] = value;
            return;
        }
        match addr & 0xF000 {
            0xA000 => self.prg_bank = value & 0x0F,
            0xB000 => self.chr_lo_fd = value & 0x1F,
            0xC000 => self.chr_lo_fe = value & 0x1F,
            0xD000 => self.chr_hi_fd = value & 0x1F,
            0xE000 => self.chr_hi_fe = value & 0x1F,
            0xF000 => {
                self.mirroring = if value & 1 == 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
                let v = self.chr_rom[off % self.chr_rom.len()];
                self.update_latch(addr);
                v
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring) % self.vram.len()],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    let len = self.chr_rom.len();
                    self.chr_rom[off % len] = value;
                }
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
        let mut out = Vec::with_capacity(16 + self.vram.len() + self.ram_block_len());
        out.push(MMC4_SECTION_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_lo_fd);
        out.push(self.chr_lo_fe);
        out.push(self.chr_hi_fd);
        out.push(self.chr_hi_fe);
        out.push(u8::from(self.latch_lo_fe));
        out.push(u8::from(self.latch_hi_fe));
        out.push(self.mirroring as u8);
        out.extend_from_slice(&self.vram);
        // --- v2 tail: the on-cart RAM (see `MMC4_SECTION_VERSION`) ---
        out.extend_from_slice(&self.prg_ram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let version = data.first().copied().unwrap_or(0);
        // Only the current layout is read (v2.9.8, ADR 0042). A v1 blob, which
        // stopped before the RAM block, is refused rather than loaded with the
        // RAM left as it was.
        if version != MMC4_SECTION_VERSION {
            return Err(MapperError::UnsupportedVersion(version));
        }
        let ram_len = self.ram_block_len();
        // The whole length is validated before the first field is written.
        let core_len = 9 + self.vram.len();
        let expected = core_len + ram_len;
        if data.len() != expected {
            return Err(MapperError::WrongLength {
                expected,
                got: data.len(),
            });
        }
        self.prg_bank = data[1];
        self.chr_lo_fd = data[2];
        self.chr_lo_fe = data[3];
        self.chr_hi_fd = data[4];
        self.chr_hi_fe = data[5];
        self.latch_lo_fe = data[6] != 0;
        self.latch_hi_fe = data[7] != 0;
        self.mirroring = match data[8] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.vram.copy_from_slice(&data[9..core_len]);
        let (prg, chr) = data[core_len..].split_at(self.prg_ram.len());
        self.prg_ram.copy_from_slice(prg);
        if self.chr_is_ram {
            self.chr_rom.copy_from_slice(chr);
        }
        Ok(())
    }
}

impl Mmc4 {
    /// Bytes the v2 tail adds: the 8 KiB PRG-RAM, plus the 8 KiB CHR-RAM
    /// when the cartridge has no CHR-ROM. Derived from the loaded ROM, so a
    /// save and its load (same ROM, checked by the `.rns` hash tag) agree.
    fn ram_block_len(&self) -> usize {
        self.prg_ram.len()
            + if self.chr_is_ram {
                self.chr_rom.len()
            } else {
                0
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth_prg(banks_16k: usize) -> Box<[u8]> {
        vec![0u8; banks_16k * PRG_BANK_16K].into_boxed_slice()
    }

    fn synth_chr_4k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_4K];
        for b in 0..banks {
            v[b * CHR_BANK_4K] = b as u8;
        }
        v.into_boxed_slice()
    }

    /// v2.9.2 cartridge-RAM sweep: the section carries the 8 KiB PRG-RAM
    /// (the battery save RAM) and, on a board with no CHR-ROM, the 8 KiB
    /// CHR-RAM. The whole-machine pin is
    /// `rustynes_core::nes::tests::every_board_snapshot_carries_cartridge_ram`.
    #[test]
    fn mmc4_save_state_carries_prg_ram_and_chr_ram() {
        let mut m = Mmc4::new(synth_prg(8), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0x6000, 0x5A);
        m.cpu_write(0x7FFF, 0xA5);
        m.chr_rom[0x0000] = 0x11;
        m.chr_rom[0x1FFF] = 0x22;
        let blob = m.save_state();
        let mut m2 = Mmc4::new(synth_prg(8), Box::new([]), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).expect("round-trip");
        assert_eq!(m2.cpu_read(0x6000), 0x5A);
        assert_eq!(m2.cpu_read(0x7FFF), 0xA5);
        assert_eq!(m2.chr_rom[0x0000], 0x11);
        assert_eq!(m2.chr_rom[0x1FFF], 0x22);
    }

    /// v2.9.8 (ADR 0042): a v1 blob (no RAM tail, written through v2.9.1)
    /// is refused. Until then it loaded and left the RAM as it was.
    #[test]
    fn mmc4_v1_blob_is_refused() {
        let mut m = Mmc4::new(synth_prg(8), synth_chr_4k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0xA000, 3);
        let core_len = 9 + m.vram.len();
        let mut v1 = m.save_state()[..core_len].to_vec();
        v1[0] = 1;
        let mut m2 = Mmc4::new(synth_prg(8), synth_chr_4k(8), Mirroring::Vertical).unwrap();
        assert!(matches!(
            m2.load_state(&v1),
            Err(MapperError::UnsupportedVersion(1))
        ));
    }

    /// A v2 blob one byte short (inside the RAM tail) is rejected.
    #[test]
    fn mmc4_truncated_ram_tail_is_rejected() {
        let m = Mmc4::new(synth_prg(8), synth_chr_4k(8), Mirroring::Vertical).unwrap();
        let blob = m.save_state();
        let mut m2 = Mmc4::new(synth_prg(8), synth_chr_4k(8), Mirroring::Vertical).unwrap();
        let err = m2
            .load_state(&blob[..blob.len() - 1])
            .expect_err("a truncated v2 blob must be rejected");
        assert!(matches!(err, MapperError::WrongLength { .. }), "{err:?}");
    }
}

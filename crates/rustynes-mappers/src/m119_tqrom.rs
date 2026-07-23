//! TQROM (iNES mapper 119) implementation.
//!
//! TQROM is a Nintendo board built around a stock MMC3, used by *Pin\*Bot* and
//! *High Speed*. It is byte-for-byte the MMC3 (mapper 4) for PRG banking, the
//! A12-edge scanline IRQ, and mirroring — the *only* difference is a **mixed
//! CHR address space**: the board carries **64 KiB of CHR-ROM plus 8 KiB of
//! CHR-RAM**, and each 1 KiB CHR bank chooses between them at fetch time.
//!
//! Per `nesdev_wiki/INES_Mapper_119.xhtml` / "TQROM": the MMC3 CHR bank
//! registers are 8-bit, and **bit 6 of the resolved 1 KiB bank number selects
//! the memory**:
//!
//! - bit 6 **clear** → CHR-ROM (the low 6 bits index the 64 KiB = 64 banks),
//! - bit 6 **set**   → CHR-RAM (8 KiB = 8 banks, so the low 3 bits index it).
//!
//! CHR writes only land when the selected bank addresses CHR-RAM; a write to a
//! CHR-ROM-selected bank is ignored (it is ROM).
//!
//! Because the ROM/RAM split is invisible to the stock MMC3 (which masks every
//! bank into a single CHR slice), this mapper **embeds an [`Mmc3`]** holding the
//! CHR-ROM and delegates PRG / IRQ / mirroring to it verbatim, but takes over
//! the pattern-table (`$0000-$1FFF`) read/write path: it asks the embedded
//! MMC3 for the *raw* 1 KiB bank number ([`Mmc3::chr_bank_1k`]) and routes the
//! access to its own CHR-ROM or CHR-RAM accordingly. It also snoops the
//! bank-select / bank-data writes — but only to forward them; the MMC3 keeps
//! the authoritative registers, so `chr_bank_1k` reflects the live banking.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::doc_markdown
)]

use crate::cartridge::Mirroring;
use crate::m004_mmc3::Mmc3;
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError};
use alloc::format;
use alloc::string::ToString;
use alloc::{boxed::Box, vec, vec::Vec};

const CHR_BANK_1K: usize = 0x0400;
const CHR_RAM_SIZE: usize = 8 * CHR_BANK_1K; // 8 KiB
/// Bit 6 of a resolved 1 KiB CHR bank number selects CHR-RAM (set) vs
/// CHR-ROM (clear).
const CHR_RAM_SELECT: usize = 0x40;

const SAVE_STATE_VERSION: u8 = 1;

/// TQROM mapper (iNES mapper 119): MMC3 core with a mixed 64 KiB CHR-ROM +
/// 8 KiB CHR-RAM address space.
pub struct Tqrom {
    inner: Mmc3,
    chr_ram: Box<[u8]>,
}

impl Tqrom {
    /// Construct a new TQROM mapper.
    ///
    /// `prg_rom` / `chr_rom` follow [`Mmc3::new`]; `chr_rom` is the 64 KiB
    /// CHR-ROM (TQROM is never CHR-RAM-only — the 8 KiB CHR-RAM is always
    /// allocated here in addition). `prg_ram_bytes == 0` selects the default
    /// 8 KiB.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch (propagated from the
    /// embedded MMC3).
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        initial_mirroring: Mirroring,
        prg_ram_bytes: usize,
    ) -> Result<Self, MapperError> {
        let inner = Mmc3::new(
            prg_rom,
            chr_rom,
            initial_mirroring,
            prg_ram_bytes,
            crate::m004_mmc3::Mmc3Revision::Sharp,
        )?;
        Ok(Self {
            inner,
            chr_ram: vec![0u8; CHR_RAM_SIZE].into_boxed_slice(),
        })
    }

    /// Resolve a pattern-table address (`$0000-$1FFF`) to whether it selects
    /// CHR-RAM and the byte offset within the selected memory.
    fn resolve_chr(&self, addr: u16) -> (bool, usize) {
        let bank = self.inner.chr_bank_1k(addr);
        let offset_in_bank = (addr as usize) & (CHR_BANK_1K - 1);
        if bank & CHR_RAM_SELECT != 0 {
            // CHR-RAM: low 3 bits index the 8 KiB (8 banks).
            let ram_bank = bank & 0x07;
            (true, ram_bank * CHR_BANK_1K + offset_in_bank)
        } else {
            // CHR-ROM: low 6 bits index the 64 KiB (64 banks).
            let rom_bank = bank & 0x3F;
            (false, rom_bank * CHR_BANK_1K + offset_in_bank)
        }
    }
}

impl Mapper for Tqrom {
    // v2.8.0 Phase 4 — CPU-cycle hook + IRQ source; no on-cart audio.
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.inner.cpu_read(addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        self.inner.cpu_write(addr, value);
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        self.inner.cpu_read_unmapped(addr)
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let a = addr & 0x3FFF;
        if a < 0x2000 {
            let (is_ram, off) = self.resolve_chr(a);
            if is_ram {
                self.chr_ram[off % self.chr_ram.len()]
            } else {
                // Read CHR-ROM through the embedded MMC3, which holds the
                // 64 KiB CHR slice. The MMC3 masks the bank against its own
                // CHR size; for a 64 KiB CHR-ROM that mask is identical to
                // our 6-bit `& 0x3F`, so this yields the correct byte.
                self.inner.ppu_read(a)
            }
        } else {
            // Nametable reads delegate to the MMC3 (mirroring + 4-screen).
            self.inner.ppu_read(a)
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let a = addr & 0x3FFF;
        if a < 0x2000 {
            let (is_ram, off) = self.resolve_chr(a);
            if is_ram {
                let len = self.chr_ram.len();
                self.chr_ram[off % len] = value;
            }
            // CHR-ROM-selected bank: write ignored (it is ROM).
        } else {
            self.inner.ppu_write(a, value);
        }
    }

    fn nametable_address(&self, addr: u16) -> u16 {
        self.inner.nametable_address(addr)
    }

    fn current_mirroring(&self) -> Mirroring {
        self.inner.current_mirroring()
    }

    fn notify_a12(&mut self, level: bool) {
        self.inner.notify_a12(level);
    }

    fn notify_a12_at_sub_dot(&mut self, level: bool, sub_dot: u8) {
        self.inner.notify_a12_at_sub_dot(level, sub_dot);
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

    fn debug_info(&self) -> MapperDebugInfo {
        let mut info = self.inner.debug_info();
        info.mapper_id = 119;
        info.name = "TQROM (119)".into();
        for slot in 0u16..8 {
            let (is_ram, _) = self.resolve_chr(slot * CHR_BANK_1K as u16);
            info.chr_banks.push((
                format!("slot{slot}"),
                if is_ram { "RAM" } else { "ROM" }.to_string(),
            ));
        }
        info
    }

    fn save_state(&self) -> Vec<u8> {
        // Our own small header (version + CHR-RAM) followed by the embedded
        // MMC3's full save state.
        let inner = self.inner.save_state();
        let mut out = Vec::with_capacity(1 + self.chr_ram.len() + inner.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.chr_ram);
        out.extend_from_slice(&inner);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let header = 1 + self.chr_ram.len();
        if data.len() < header {
            return Err(MapperError::Truncated {
                expected: header,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.chr_ram.copy_from_slice(&data[1..header]);
        self.inner.load_state(&data[header..])
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;
    use alloc::vec;

    const PRG_BANK_8K: usize = 0x2000;

    fn synth_prg(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * PRG_BANK_8K];
        for b in 0..banks_8k {
            v[b * PRG_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    /// 64 KiB CHR-ROM (64 1 KiB banks), each bank's first byte = bank number.
    fn synth_chr_rom() -> Box<[u8]> {
        let mut v = vec![0u8; 64 * CHR_BANK_1K];
        for b in 0..64 {
            v[b * CHR_BANK_1K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn fresh() -> Tqrom {
        Tqrom::new(synth_prg(8), synth_chr_rom(), Mirroring::Vertical, 0).unwrap()
    }

    fn select_write(m: &mut Tqrom, reg: u8, value: u8) {
        m.cpu_write(0x8000, reg);
        m.cpu_write(0x8001, value);
    }

    #[test]
    fn delegates_prg_banking_to_mmc3() {
        let mut m = fresh();
        // Last 8 KiB bank fixed at $E000, exactly like MMC3.
        assert_eq!(m.cpu_read(0xE000), 7);
        select_write(&mut m, 6, 3); // R6 = 3
        assert_eq!(m.cpu_read(0x8000), 3);
    }

    #[test]
    fn bit6_clear_reads_chr_rom() {
        let mut m = fresh();
        // CHR mode 0: R2 maps the 1 KiB slot at $1000. Set R2 = 5 (bit 6
        // clear) -> CHR-ROM bank 5, whose first byte is 5.
        select_write(&mut m, 2, 5);
        assert_eq!(m.ppu_read(0x1000), 5, "bit-6-clear bank reads CHR-ROM");
    }

    #[test]
    fn bit6_set_reads_and_writes_chr_ram() {
        let mut m = fresh();
        // R2 = 0x40 -> bit 6 set -> CHR-RAM bank 0. Initially zero.
        select_write(&mut m, 2, 0x40);
        assert_eq!(m.ppu_read(0x1000), 0, "fresh CHR-RAM bank reads zero");
        // Writes land in CHR-RAM and read back.
        m.ppu_write(0x1000, 0xAB);
        assert_eq!(m.ppu_read(0x1000), 0xAB, "CHR-RAM is writable");
        // A different CHR-RAM bank (0x40 | 1) is independent storage.
        select_write(&mut m, 2, 0x41);
        assert_eq!(m.ppu_read(0x1000), 0, "CHR-RAM bank 1 distinct from bank 0");
        m.ppu_write(0x1000, 0xCD);
        assert_eq!(m.ppu_read(0x1000), 0xCD);
        // Back to bank 0: original value preserved.
        select_write(&mut m, 2, 0x40);
        assert_eq!(m.ppu_read(0x1000), 0xAB);
    }

    #[test]
    fn chr_rom_writes_are_ignored() {
        let mut m = fresh();
        // R2 = 5 (CHR-ROM bank 5). A write must be ignored (ROM).
        select_write(&mut m, 2, 5);
        m.ppu_write(0x1000, 0xFF);
        assert_eq!(m.ppu_read(0x1000), 5, "write to a CHR-ROM bank is ignored");
    }

    #[test]
    fn chr_ram_bank_index_uses_low_3_bits() {
        let mut m = fresh();
        // Bank 0x40 | 7 selects CHR-RAM bank 7 (the last of the 8 KiB).
        select_write(&mut m, 2, 0x47);
        m.ppu_write(0x1000, 0x77);
        assert_eq!(m.ppu_read(0x1000), 0x77);
        // 0x40 | 0x0F masks to the same low-3-bits = 7 bank.
        select_write(&mut m, 2, 0x4F);
        assert_eq!(
            m.ppu_read(0x1000),
            0x77,
            "CHR-RAM bank index is the low 3 bits of the register"
        );
    }

    #[test]
    fn irq_delegates_to_mmc3() {
        let mut m = fresh();
        m.cpu_write(0xC000, 3);
        m.cpu_write(0xC001, 0);
        m.cpu_write(0xE001, 0);
        for _ in 0..5 {
            m.notify_a12(false);
            for _ in 0..4 {
                m.notify_cpu_cycle();
            }
            m.notify_a12(true);
        }
        assert!(m.irq_pending(), "IRQ counter behaves like MMC3");
    }

    #[test]
    fn mirroring_register_toggles_h_v() {
        let mut m = fresh();
        m.cpu_write(0xA000, 0);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        m.cpu_write(0xA000, 1);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn save_load_round_trip_preserves_chr_ram() {
        let mut m = fresh();
        select_write(&mut m, 6, 3);
        select_write(&mut m, 2, 0x40); // CHR-RAM bank 0
        m.ppu_write(0x1000, 0x5A);
        m.cpu_write(0xC000, 0x10);
        let blob = m.save_state();
        let mut other = fresh();
        other.load_state(&blob).unwrap();
        assert_eq!(other.cpu_read(0x8000), m.cpu_read(0x8000));
        // CHR-RAM round-trips.
        other.cpu_write(0x8000, 2);
        other.cpu_write(0x8001, 0x40);
        assert_eq!(other.ppu_read(0x1000), 0x5A);
    }
}

//! TxSROM / MMC3-TLSROM (iNES mapper 118) implementation.
//!
//! TxSROM (TKSROM + TLSROM boards) is a standard Nintendo MMC3 used in a
//! nonstandard way: the CHR A17 line is wired directly to CIRAM A10 instead of
//! the MMC3's own CIRAM A10 output. The practical effect is that **bit 7 of
//! each CHR bank register selects which physical nametable backs the
//! corresponding nametable slot** — programs choose per-slot mirroring exactly
//! the way they choose CHR banks (`nesdev_wiki/INES_Mapper_118.xhtml`).
//!
//! Everything else — PRG/CHR banking, the A12-edge scanline IRQ, the `$C000`
//! reload / `$E000` enable protocol — is byte-for-byte the MMC3, so this
//! mapper **embeds an [`Mmc3`]** and delegates all of its behaviour, snooping
//! only the bank-select / bank-data writes to maintain its own copy of the six
//! CHR registers (and the CHR A12-inversion mode bit) so it can derive the
//! per-slot nametable mapping. The `$A000` mirroring register is a no-op on
//! these boards (its effect is bypassed by the CHR-A17 wiring).
//!
//! # Per-slot nametable mapping
//!
//! With CHR mode 0 (`$8000` bit 7 = 0), the two 2 KiB CHR banks (R0/R1) cover
//! pattern `$0000-$0FFF`; bit 7 of R0 selects the nametable for slots 0/1
//! (`$2000`/`$2400`), bit 7 of R1 selects slots 2/3 (`$2800`/`$2C00`). With
//! CHR mode 1 (bit 7 = 1) the four 1 KiB banks (R2-R5) cover `$0000-$0FFF`,
//! and bit 7 of R2/R3/R4/R5 selects the nametable for slots 0/1/2/3
//! individually (enabling all four mirroring layouts).

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::doc_markdown
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError};
use crate::mmc3::Mmc3;
use alloc::format;
use alloc::{boxed::Box, vec::Vec};

const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// TxSROM / TLSROM mapper (iNES mapper 118).
pub struct TxSrom {
    inner: Mmc3,
    // Snooped copies of the six CHR bank registers + the selected register
    // index + the CHR mode bit, used only to derive per-slot nametable
    // mirroring. (The MMC3 owns the authoritative copies for banking/IRQ.)
    chr_regs: [u8; 6],
    bank_select: u8,
    chr_mode: bool,
}

impl TxSrom {
    /// Construct a new TxSROM mapper. Arguments mirror [`Mmc3::new`] minus the
    /// revision (TxSROM is always a stock MMC3).
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
            crate::mmc3::Mmc3Revision::Sharp,
        )?;
        Ok(Self {
            inner,
            chr_regs: [0; 6],
            bank_select: 0,
            chr_mode: false,
        })
    }

    /// Resolve a nametable slot (0..=3) to a physical CIRAM bank (0 or 1)
    /// using bit 7 of the CHR bank register that maps to that slot.
    fn nt_bank(&self, slot: u8) -> usize {
        // The register whose bit 7 controls this slot depends on the CHR mode.
        let reg = if self.chr_mode {
            // Mode 1: four 1 KiB banks R2-R5 cover $0000-$0FFF; one per slot.
            match slot {
                0 => self.chr_regs[2],
                1 => self.chr_regs[3],
                2 => self.chr_regs[4],
                _ => self.chr_regs[5],
            }
        } else {
            // Mode 0: two 2 KiB banks R0/R1 cover $0000-$0FFF; R0 -> slots
            // 0/1, R1 -> slots 2/3.
            if slot < 2 {
                self.chr_regs[0]
            } else {
                self.chr_regs[1]
            }
        };
        usize::from((reg & 0x80) != 0)
    }
}

impl Mapper for TxSrom {
    // v2.8.0 Phase 4 — CPU-cycle hook + IRQ source; no on-cart audio.
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.inner.cpu_read(addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // Snoop the bank-select / bank-data registers for the nametable bits.
        if let 0x8000..=0x9FFF = addr {
            if addr & 1 == 0 {
                self.bank_select = value & 0x07;
                self.chr_mode = (value & 0x80) != 0;
            } else {
                let idx = (self.bank_select & 0x07) as usize;
                if idx < 6 {
                    self.chr_regs[idx] = value;
                }
            }
        }
        self.inner.cpu_write(addr, value);
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        self.inner.cpu_read_unmapped(addr)
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        self.inner.ppu_read(addr)
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        self.inner.ppu_write(addr, value);
    }

    fn nametable_address(&self, addr: u16) -> u16 {
        // Per-slot nametable selection via CHR bank bit 7 (the CHR-A17 ->
        // CIRAM-A10 wiring). Overrides the MMC3's H/V mirroring entirely.
        let table = (((addr.wrapping_sub(0x2000)) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = addr & (NAMETABLE_SIZE_U16 - 1);
        let bank = self.nt_bank(table) as u16;
        bank * NAMETABLE_SIZE_U16 + local
    }

    fn current_mirroring(&self) -> Mirroring {
        // The effective mirroring is per-slot and dynamic; report
        // MapperControlled so the bus uses our `nametable_address` override.
        Mirroring::MapperControlled
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
        info.mapper_id = 118;
        info.name = "TxSROM / TLSROM (118)".into();
        info.mirroring = "MapperControlled";
        for slot in 0u8..4 {
            info.extra
                .push((format!("NT{slot}"), format!("{}", self.nt_bank(slot))));
        }
        info
    }

    fn save_state(&self) -> Vec<u8> {
        // Our own small header (version + snooped state) followed by the
        // embedded MMC3's full save state.
        let inner = self.inner.save_state();
        let mut out = Vec::with_capacity(1 + 6 + 1 + 1 + inner.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.chr_regs);
        out.push(self.bank_select);
        out.push(u8::from(self.chr_mode));
        out.extend_from_slice(&inner);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        const HEADER: usize = 1 + 6 + 1 + 1;
        if data.len() < HEADER {
            return Err(MapperError::Truncated {
                expected: HEADER,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.chr_regs.copy_from_slice(&data[1..7]);
        self.bank_select = data[7];
        self.chr_mode = data[8] != 0;
        self.inner.load_state(&data[HEADER..])
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;
    use alloc::vec;

    fn synth_prg(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * 0x2000];
        for b in 0..banks_8k {
            v[b * 0x2000] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr(banks_1k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_1k * 0x0400];
        for b in 0..banks_1k {
            v[b * 0x0400] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn fresh() -> TxSrom {
        TxSrom::new(synth_prg(8), synth_chr(64), Mirroring::Vertical, 0).unwrap()
    }

    fn select_write(m: &mut TxSrom, reg: u8, value: u8) {
        m.cpu_write(0x8000, reg);
        m.cpu_write(0x8001, value);
    }

    #[test]
    fn delegates_prg_banking_to_mmc3() {
        let mut m = fresh();
        // Last bank fixed at $E000 just like MMC3.
        assert_eq!(m.cpu_read(0xE000), 7);
        select_write(&mut m, 6, 3); // R6 = 3
        assert_eq!(m.cpu_read(0x8000), 3);
    }

    #[test]
    fn reports_mapper_controlled_mirroring() {
        let m = fresh();
        assert_eq!(m.current_mirroring(), Mirroring::MapperControlled);
    }

    #[test]
    fn mode0_nametable_bits_from_r0_r1() {
        let mut m = fresh();
        // CHR mode 0 (bit 7 clear). R0 bit 7 set -> slots 0/1 use bank 1.
        select_write(&mut m, 0, 0x80); // R0 = $80
        select_write(&mut m, 1, 0x00); // R1 = $00
                                       // Slot 0 ($2000) and slot 1 ($2400) -> bank 1.
        assert_eq!(m.nametable_address(0x2000) >> 10, 1);
        assert_eq!(m.nametable_address(0x2400) >> 10, 1);
        // Slot 2 ($2800) and slot 3 ($2C00) -> bank 0 (R1 bit 7 clear).
        assert_eq!(m.nametable_address(0x2800) >> 10, 0);
        assert_eq!(m.nametable_address(0x2C00) >> 10, 0);
    }

    #[test]
    fn mode1_nametable_bits_from_r2_r5() {
        let mut m = fresh();
        // CHR mode 1: set bit 7 of $8000 to enable. Then R2-R5 each control
        // one slot.
        m.cpu_write(0x8000, 0x80 | 2); // mode 1, select R2
        m.cpu_write(0x8001, 0x80); // R2 bit 7 -> slot 0 bank 1
        m.cpu_write(0x8000, 0x80 | 3); // select R3
        m.cpu_write(0x8001, 0x00); // R3 -> slot 1 bank 0
        m.cpu_write(0x8000, 0x80 | 4);
        m.cpu_write(0x8001, 0x80); // R4 -> slot 2 bank 1
        m.cpu_write(0x8000, 0x80 | 5);
        m.cpu_write(0x8001, 0x00); // R5 -> slot 3 bank 0
        assert_eq!(m.nametable_address(0x2000) >> 10, 1);
        assert_eq!(m.nametable_address(0x2400) >> 10, 0);
        assert_eq!(m.nametable_address(0x2800) >> 10, 1);
        assert_eq!(m.nametable_address(0x2C00) >> 10, 0);
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
        assert!(m.irq_pending());
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = fresh();
        select_write(&mut m, 0, 0x80);
        select_write(&mut m, 6, 3);
        m.cpu_write(0xC000, 0x10);
        let blob = m.save_state();
        let mut m2 = fresh();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.nametable_address(0x2000), m2.nametable_address(0x2000));
    }
}

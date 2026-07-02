//! Nintendo Vs. System (iNES mapper 99) implementation.
//!
//! The Vs. `UniSystem` cartridge board is electrically a fixed-PRG board (8 KiB,
//! 16 KiB, or 2x16 KiB = 32 KiB of PRG-ROM mapped straight into `$8000-$FFFF`)
//! with an 8 KiB switchable CHR-ROM bank. The defining quirk of the board is
//! that the CHR bank select is **bit 2 of the value written to `$4016`** (the
//! Vs. coin/CHR register) — not a `$8000-$FFFF` write like most CHR-banked
//! boards. The single CHR-select bit picks between the first two 8 KiB CHR
//! banks; a cart with only 8 KiB of CHR ignores it.
//!
//! The `$4016` write is shared with controller strobing (the standard NES
//! `OUT0` line), so the bus forwards every `$4016` write to the mapper *in
//! addition to* committing the controller strobe — see
//! `rustynes_core::bus` `$4016` write handling. Only mapper 99 consumes it; every
//! other mapper's `cpu_write` ignores the `$4016` address.
//!
//! The Vs. System replaces the 2C02 composite PPU with an RGB PPU
//! (2C03 / 2C04-000x / 2C05). RustyNES routes the RGB palette selection
//! through the NES 2.0 header (`ConsoleType::VsSystem` + `VsPpuType`); the
//! `crate::parse` dispatch promotes a mapper-99 cart to `VsSystem` + the most
//! common 2C03 RGB PPU when the header does not already carry a resolved
//! Vs. PPU type (iNES 1.0 has no byte-13). The mapper itself only handles
//! banking; the palette lives in the PPU.
//!
//! Mirroring is fixed from the iNES header. There is no IRQ.
//!
//! See `docs/mappers.md` §Vs. System and `nesdev_wiki/INES_Mapper_099.xhtml`.

#![allow(clippy::cast_possible_truncation, clippy::doc_markdown)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;
/// v2.0.0 beta.5: the save-state layout emitted when the Vs. `DualSystem`
/// shared WRAM is provisioned (v1 + the 2 KiB WRAM tail). `UniSystem` carts
/// keep emitting v1 byte-identically.
const SAVE_STATE_VERSION_DUAL: u8 = 2;

/// Nintendo Vs. System mapper (iNES mapper 99).
pub struct VsSystem {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    /// 8 KiB CHR bank index (only bit 0 is meaningful — `$4016` bit 2).
    chr_bank: u8,
    mirroring: Mirroring,
    /// v2.0.0 beta.5 (Vs. `DualSystem`): this console's COPY of the cabinet's
    /// shared 2 KiB work RAM at `$6000-$7FFF` (mirrored across the 8 KiB
    /// window — MAME: `map(0x6000, 0x67ff).mirror(0x1800)`). `None` on
    /// `UniSystem` carts — their `$6000` window keeps the pre-`DualSystem`
    /// behavior byte-identically. Provisioned by
    /// [`Mapper::enable_vs_dual_wram`] from the `VsDualSystem` wrapper on
    /// BOTH consoles; the wrapper converges the two copies by draining
    /// [`Mapper::take_vs_dual_wram_writes`] into the partner's
    /// [`Mapper::apply_vs_dual_wram_write`] after every stepped instruction
    /// (MAME's fully-shared `.share("nvram")` at soft-lockstep granularity).
    dual_wram: Option<Box<[u8]>>,
    /// v2.0.0 beta.5: the shared-WRAM write log the wrapper drains toward
    /// the partner console. Transient (always drained within the stepping
    /// loop) — deliberately NOT serialized in the save state.
    dual_wram_log: Vec<(u16, u8)>,
    /// v2.0.0 beta.5: `true` on the `DualSystem` SUB console's mapper
    /// instance — banks the second PRG half + the upper CHR pages (the
    /// two CPUs run different programs). Cabinet wiring, applied by the
    /// wrapper at construction (like the bus's sub identity); not
    /// serialized (it survives restore because restore never rebuilds the
    /// mapper, and a fresh wrapper re-applies it in `from_rom`).
    dual_sub: bool,
}

impl VsSystem {
    /// Construct a new Vs. System mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 8 KiB (the common boards are
    /// 8 KiB, 16 KiB, or 32 KiB). CHR-RAM is selected when `chr_rom` is empty;
    /// otherwise CHR-ROM length must be a multiple of 8 KiB.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when the sizes don't match the
    /// constraints.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "Vs. System PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Vs. System expects an 8 KiB multiple of CHR; got {} bytes",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            chr_bank: 0,
            mirroring,
            dual_wram: None,
            dual_wram_log: Vec::new(),
            dual_sub: false,
        })
    }

    const fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.mirroring.physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let bank_count = (self.chr.len() / CHR_BANK_8K).max(1);
        // v2.0.0 beta.5: the DualSystem SUB console banks the second CHR
        // half (Mesen2 `VsSystem.h`: `chrOuter = IsVsMainConsole() ? 0 : 2`,
        // OR'd with the `$4016`-bit-2 select). Wraps modulo the bank count,
        // so a 16 KiB (or smaller) CHR — where main and sub share tiles —
        // resolves to the same banks either way.
        let outer = if self.dual_sub { 2 } else { 0 };
        let bank = ((self.chr_bank as usize) | outer) % bank_count;
        bank * CHR_BANK_8K + (addr as usize & (CHR_BANK_8K - 1))
    }
}

impl Mapper for VsSystem {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Fixed PRG: the cart's PRG-ROM is mapped straight into
            // `$8000-$FFFF`, mirrored down for 8/16 KiB carts.
            //
            // v2.0.0 beta.5: the DualSystem SUB console runs the SECOND
            // 32 KiB PRG half — the two CPUs execute DIFFERENT programs
            // (MAME `balonfgt`: the `sub` region's `6d`/`6a` ROMs differ
            // from the main's `1d`/`1a` by CRC; Mesen2 `VsSystem.h`:
            // `prgOuter = IsVsMainConsole() ? 0 : 4` in 8 KiB pages). The
            // modulo keeps a 32 KiB (main-half-only) dump wrapping onto
            // the same program either way.
            let off = (addr - 0x8000) as usize + if self.dual_sub { 0x8000 } else { 0 };
            self.prg_rom[off % self.prg_rom.len()]
        } else if (0x6000..=0x7FFF).contains(&addr) {
            // v2.0.0 beta.5: the DualSystem's shared 2 KiB WRAM, mirrored
            // across the 8 KiB window. UniSystem carts (`dual_wram: None`)
            // keep the pre-existing 0 return byte-identically.
            self.dual_wram
                .as_ref()
                .map_or(0, |w| w[(addr as usize - 0x6000) % w.len()])
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // The CHR bank select is bit 2 of the `$4016` write (the Vs.
        // coin/CHR register). The bus forwards every `$4016` write to the
        // mapper alongside the standard controller strobe; we consume only
        // bit 2 here. Writes to `$8000-$FFFF` have no banking effect on this
        // board (PRG is fixed).
        if addr == 0x4016 {
            self.chr_bank = (value >> 2) & 0x01;
        } else if (0x6000..=0x7FFF).contains(&addr) {
            // v2.0.0 beta.5: DualSystem shared-WRAM write (see cpu_read).
            // Written to this console's copy AND logged for the wrapper to
            // replay into the partner's copy (the fully-shared MAME model).
            if let Some(w) = self.dual_wram.as_mut() {
                let len = w.len();
                let off = (addr as usize - 0x6000) % len;
                w[off] = value;
                #[allow(clippy::cast_possible_truncation)] // off < 0x800
                self.dual_wram_log.push((off as u16, value));
            }
        }
    }

    fn enable_vs_dual_wram(&mut self) {
        if self.dual_wram.is_none() {
            // The DualSystem cabinet's shared RAM is 2 KiB (nesdev
            // "Vs. System"; MAME maps the same 2 KiB `.share("nvram")`).
            self.dual_wram = Some(vec![0u8; 0x0800].into_boxed_slice());
        }
    }

    fn set_vs_dual_sub(&mut self) {
        self.dual_sub = true;
    }

    fn take_vs_dual_wram_writes(&mut self) -> Vec<(u16, u8)> {
        core::mem::take(&mut self.dual_wram_log)
    }

    fn apply_vs_dual_wram_write(&mut self, offset: u16, value: u8) {
        // Partner-console replay: lands in this copy WITHOUT re-logging
        // (a log entry here would echo back and forth forever).
        if let Some(w) = self.dual_wram.as_mut() {
            let len = w.len();
            w[offset as usize % len] = value;
        }
    }

    fn take_vs_dual_wram(&mut self) -> Option<Box<[u8]>> {
        self.dual_wram.take()
    }

    fn set_vs_dual_wram(&mut self, wram: Box<[u8]>) {
        self.dual_wram = Some(wram);
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // PRG is mapped from `$8000`; the `$4020-$5FFF` window remains open
        // bus (same as the NROM-class default).
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[self.nametable_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr[off] = value;
                }
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

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 99,
            name: "Vs. System (99)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.chr_banks
            .push(("CHR ($4016 bit2)".into(), format!("{:#04x}", self.chr_bank)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        // v1 layout: [version=1, chr_bank, vram, chr-if-ram].
        // v2 layout (v2.0.0 beta.5, emitted only when the DualSystem shared
        // WRAM is provisioned): [version=2, chr_bank, vram, chr-if-ram,
        // wram(2 KiB)]. UniSystem carts keep emitting v1 byte-identically.
        let wram_len = self.dual_wram.as_ref().map_or(0, |w| w.len());
        let mut out = Vec::with_capacity(
            2 + self.vram.len() + if self.chr_is_ram { self.chr.len() } else { 0 } + wram_len,
        );
        out.push(if self.dual_wram.is_some() {
            SAVE_STATE_VERSION_DUAL
        } else {
            SAVE_STATE_VERSION
        });
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        if let Some(w) = self.dual_wram.as_ref() {
            out.extend_from_slice(w);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let version = *data.first().unwrap_or(&0);
        let need_wram = match version {
            SAVE_STATE_VERSION => 0,
            SAVE_STATE_VERSION_DUAL => 0x0800,
            v => return Err(MapperError::UnsupportedVersion(v)),
        };
        let expected = 2 + self.vram.len() + need_chr + need_wram;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        self.chr_bank = data[1];
        let mut cursor = 2;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr
                .copy_from_slice(&data[cursor..cursor + self.chr.len()]);
            cursor += self.chr.len();
        }
        if need_wram > 0 {
            // Re-provision on restore even if the live instance had not been
            // through `enable_vs_dual_wram` yet (a fresh Nes restored from a
            // dual snapshot).
            let mut w = vec![0u8; need_wram].into_boxed_slice();
            w.copy_from_slice(&data[cursor..cursor + need_wram]);
            self.dual_wram = Some(w);
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * CHR_BANK_8K];
        for b in 0..banks_8k {
            v[b * CHR_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * CHR_BANK_8K];
        for b in 0..banks_8k {
            v[b * CHR_BANK_8K] = 0xA0 | b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn prg_fixed_and_mirrored() {
        // 8 KiB PRG mirrors across the whole $8000-$FFFF window.
        let mut m = VsSystem::new(synth_prg(1), synth_chr(2), Mirroring::Horizontal).unwrap();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xA000), 0); // mirror of bank 0
        assert_eq!(m.cpu_read(0xE000), 0);
    }

    #[test]
    fn prg_32k_maps_straight() {
        // 32 KiB PRG (4x8 KiB) maps straight: bank b at $8000 + b*8K.
        let mut m = VsSystem::new(synth_prg(4), synth_chr(2), Mirroring::Vertical).unwrap();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xA000), 1);
        assert_eq!(m.cpu_read(0xC000), 2);
        assert_eq!(m.cpu_read(0xE000), 3);
    }

    #[test]
    fn chr_bank_select_via_4016_bit2() {
        let mut m = VsSystem::new(synth_prg(2), synth_chr(2), Mirroring::Horizontal).unwrap();
        // Default bank 0.
        assert_eq!(m.ppu_read(0x0000), 0xA0);
        // $4016 bit 2 set -> CHR bank 1.
        m.cpu_write(0x4016, 0b0000_0100);
        assert_eq!(m.ppu_read(0x0000), 0xA1);
        // $4016 bit 2 clear -> back to bank 0. Other bits (controller strobe)
        // are ignored by the mapper.
        m.cpu_write(0x4016, 0b0000_0001);
        assert_eq!(m.ppu_read(0x0000), 0xA0);
    }

    #[test]
    fn cpu_write_8000_does_not_bank() {
        // A $8000-$FFFF write must NOT change the CHR bank on this board.
        let mut m = VsSystem::new(synth_prg(2), synth_chr(2), Mirroring::Horizontal).unwrap();
        m.cpu_write(0x8000, 0xFF);
        assert_eq!(m.ppu_read(0x0000), 0xA0); // still bank 0
    }

    #[test]
    fn single_chr_bank_ignores_select() {
        // A cart with only 8 KiB CHR wraps the bank to 0 regardless of bit 2.
        let mut m = VsSystem::new(synth_prg(2), synth_chr(1), Mirroring::Horizontal).unwrap();
        m.cpu_write(0x4016, 0b0000_0100);
        assert_eq!(m.ppu_read(0x0000), 0xA0);
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = VsSystem::new(synth_prg(2), synth_chr(2), Mirroring::Vertical).unwrap();
        m.cpu_write(0x4016, 0b0000_0100);
        let blob = m.save_state();
        let mut m2 = VsSystem::new(synth_prg(2), synth_chr(2), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.ppu_read(0x0000), m2.ppu_read(0x0000));
    }
}

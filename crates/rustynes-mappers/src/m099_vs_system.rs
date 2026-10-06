// SPDX-License-Identifier: GPL-3.0-or-later
//
// Provenance: the Vs. DualSystem sub-console banking (the second CHR half via an outer bank of 2, the second PRG half via an outer 8 KiB page of 4) is derived from Mesen2 (GPL-3.0-or-later) `VsSystem.h` (`chrOuter` / `prgOuter`), whose expressions the in-file comments quote. Classified as derived in v2.7.1 (core audit section 6.2). See docs/originality-and-provenance.md (Section 1)
// and NOTICE for the complete, audited derivation record.

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
//! The CPU board also carries 2 KiB of RAM at `$6000-$7FFF` (mirrored across
//! the 8 KiB window). On a `UniSystem` cabinet the primary CPU's `$4016` bit 1
//! (`OUT1`) decides whether it sees that RAM (1) or open bus (0); the
//! `DualSystem` wrapper shares one copy between both consoles instead.
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

/// v2.0.0 beta.5: the save-state layout emitted when the Vs. `DualSystem`
/// shared WRAM is provisioned (the v1 layout + the 2 KiB WRAM tail).
const SAVE_STATE_VERSION_DUAL: u8 = 2;
/// v2.9.8: the `UniSystem` layout once the board's 2 KiB RAM is modelled
/// (the v1 layout + the `OUT1` latch + the 2 KiB RAM tail). Layout 1 itself
/// (no RAM) is refused since v2.9.8 (ADR 0042); it used to load with the RAM
/// cleared and `OUT1` low.
const SAVE_STATE_VERSION_UNI_RAM: u8 = 3;
/// The board's work RAM at `$6000-$7FFF`: 2 KiB, mirrored across the 8 KiB
/// window (nesdev "Vs. System" and `INES_Mapper_099`).
const WRAM_SIZE: usize = 0x0800;

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
    /// [`Mapper::drain_vs_dual_wram_writes`] into the partner's
    /// [`Mapper::apply_vs_dual_wram_write`] after every stepped instruction
    /// (MAME's fully-shared `.share("nvram")` at soft-lockstep granularity).
    dual_wram: Option<Box<[u8]>>,
    /// v2.9.8: the same 2 KiB RAM on a `UniSystem` cabinet (`dual_wram:
    /// None`). The MDS-0x CPU board carries it in both cabinet types; on the
    /// primary CPU, `$4016` bit 1 (`OUT1`) decides whether the CPU sees it
    /// (1) or open bus (0) (nesdev "Vs. System", `$4016` write). Before
    /// v2.9.8 a `UniSystem` cart read 0 here and dropped every write, so
    /// *Vs. Super Mario Bros.*, which keeps its state in this RAM, never left
    /// its first frame.
    uni_wram: Box<[u8]>,
    /// v2.9.8: the last `$4016` write's bit 1 (`OUT1`) -- on a `UniSystem`
    /// cabinet, whether the CPU owns `uni_wram`. Low at power-on.
    out1: bool,
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
            uni_wram: vec![0u8; WRAM_SIZE].into_boxed_slice(),
            out1: false,
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
            // across the 8 KiB window. v2.9.8: a UniSystem cart reads its
            // own copy while `OUT1` is high; while it is low the bus reports
            // open bus through `cpu_read_unmapped`, and this 0 is unused.
            let off = (addr as usize - 0x6000) % WRAM_SIZE;
            match self.dual_wram.as_ref() {
                Some(w) => w[off % w.len()],
                None if self.out1 => self.uni_wram[off],
                None => 0,
            }
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
            // v2.9.8: bit 1 (`OUT1`) arbitrates the work RAM.
            self.out1 = (value & 0x02) != 0;
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
            } else if self.out1 {
                // v2.9.8: UniSystem -- the write lands only while the CPU
                // owns the RAM.
                self.uni_wram[(addr as usize - 0x6000) % WRAM_SIZE] = value;
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

    fn drain_vs_dual_wram_writes(&mut self, dst: &mut Vec<(u16, u8)>) {
        // `Vec::append` moves every element into `dst` but leaves
        // `dual_wram_log` at length 0 WITH its allocated capacity intact
        // for the next batch of writes (unlike `mem::take`, which would
        // hand the caller the backing allocation and leave this log at
        // capacity 0 -- forcing a fresh heap allocation on the very next
        // write). Called after every stepped instruction on a
        // `DualSystem` cart via `pump_comms`.
        dst.append(&mut self.dual_wram_log);
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
        // v2.7.2 (core audit §5.5): `$6000-$7FFF` is "2 KiB RAM, swappable
        // between CPUs (open bus when not available)"
        // (`nesdev_wiki/INES_Mapper_099.xhtml`). The DualSystem's shared RAM is
        // not a battery save, so this keys on its presence, not on `sram()`.
        // v2.9.8: on a UniSystem cabinet it is "not available" exactly while
        // `OUT1` is low.
        (matches!(addr, 0x6000..=0x7FFF) && self.dual_wram.is_none() && !self.out1) || {
            // PRG is mapped from `$8000`; the `$4020-$5FFF` window remains open
            // bus (same as the NROM-class default).
            (0x4020..=0x5FFF).contains(&addr)
        }
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
        // wram(2 KiB)].
        // v3 layout (v2.9.8, every UniSystem cart): [version=3, chr_bank,
        // vram, chr-if-ram, out1, wram(2 KiB)]. v1 is neither emitted nor
        // loaded (refused since v2.9.8, ADR 0042).
        let mut out = Vec::with_capacity(
            3 + self.vram.len() + if self.chr_is_ram { self.chr.len() } else { 0 } + WRAM_SIZE,
        );
        out.push(if self.dual_wram.is_some() {
            SAVE_STATE_VERSION_DUAL
        } else {
            SAVE_STATE_VERSION_UNI_RAM
        });
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        if let Some(w) = self.dual_wram.as_ref() {
            out.extend_from_slice(w);
        } else {
            out.push(u8::from(self.out1));
            out.extend_from_slice(&self.uni_wram);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let version = *data.first().unwrap_or(&0);
        let (need_wram, need_uni) = match version {
            SAVE_STATE_VERSION_DUAL => (WRAM_SIZE, 0),
            SAVE_STATE_VERSION_UNI_RAM => (0, 1 + WRAM_SIZE),
            v => return Err(MapperError::UnsupportedVersion(v)),
        };
        let expected = 2 + self.vram.len() + need_chr + need_wram + need_uni;
        if data.len() != expected {
            return Err(MapperError::WrongLength {
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
        // `dual_wram_log` is transient (never serialized — see its field
        // doc) and MUST be dropped on restore regardless of layout version:
        // any writes logged before the restore point are now stale relative
        // to the just-loaded `dual_wram` contents, and replaying them into
        // the partner console afterward would silently corrupt its copy of
        // the shared RAM with pre-restore data.
        self.dual_wram_log.clear();
        if need_wram > 0 {
            // Re-provision on restore even if the live instance had not been
            // through `enable_vs_dual_wram` yet (a fresh Nes restored from a
            // dual snapshot).
            let mut w = vec![0u8; need_wram].into_boxed_slice();
            w.copy_from_slice(&data[cursor..cursor + need_wram]);
            self.dual_wram = Some(w);
        } else {
            // v3 (UniSystem) layout: drop any dual WRAM the live
            // instance may have been carrying so in-memory state matches the
            // versioned layout just loaded (a UniSystem snapshot never
            // described dual-WRAM state, so none should survive the restore).
            self.dual_wram = None;
        }
        if need_uni > 0 {
            self.out1 = data[cursor] != 0;
            self.uni_wram
                .copy_from_slice(&data[cursor + 1..cursor + 1 + WRAM_SIZE]);
        } else {
            // A DualSystem snapshot carries the shared WRAM instead; the
            // UniSystem RAM and its arbitration latch return to power-on.
            self.out1 = false;
            self.uni_wram.fill(0);
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

    #[test]
    fn unisystem_ram_follows_4016_bit1() {
        // nesdev "Vs. System" ($4016 write, bit 1): "On the primary CPU only,
        // controls which CPU can access 2 KiB of shared RAM mapped in the
        // $6000-$7FFF region. When 1: the primary CPU has access ... When 0:
        // ... the primary CPU sees open bus." INES_Mapper_099: "CPU
        // $6000-$7FFF: 2 KiB RAM, swappable between CPUs (open bus when not
        // available)". Vs. Super Mario Bros. keeps its state there.
        let mut m = VsSystem::new(synth_prg(4), synth_chr(2), Mirroring::Vertical).unwrap();
        // Power-on: the 2A03 OUT latch is clear, so the RAM is not ours.
        assert!(m.cpu_read_unmapped(0x6000));
        // OUT1 = 1 (with OUT2 = CHR bank 1): the primary owns the RAM.
        m.cpu_write(0x4016, 0b0000_0110);
        assert!(!m.cpu_read_unmapped(0x6000));
        m.cpu_write(0x6123, 0x5A);
        assert_eq!(m.cpu_read(0x6123), 0x5A);
        // 2 KiB, mirrored across the 8 KiB window.
        assert_eq!(m.cpu_read(0x6923), 0x5A);
        assert_eq!(m.cpu_read(0x7923), 0x5A);
        // A controller strobe that keeps OUT1 set keeps the RAM.
        m.cpu_write(0x4016, 0b0000_0011);
        assert_eq!(m.cpu_read(0x6123), 0x5A);
        // OUT1 = 0: open bus, and writes do not land.
        m.cpu_write(0x4016, 0b0000_0000);
        assert!(m.cpu_read_unmapped(0x6123));
        m.cpu_write(0x6123, 0x00);
        m.cpu_write(0x4016, 0b0000_0010);
        assert_eq!(m.cpu_read(0x6123), 0x5A);
        // The RAM and the OUT1 latch survive a save state.
        let blob = m.save_state();
        let mut m2 = VsSystem::new(synth_prg(4), synth_chr(2), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert!(!m2.cpu_read_unmapped(0x6123));
        assert_eq!(m2.cpu_read(0x6123), 0x5A);
    }
}

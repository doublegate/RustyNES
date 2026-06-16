//! MMC1 (iNES mapper 1) implementation.
//!
//! See `docs/mappers.md` §Mapper coverage matrix and §MMC1; see
//! `ref-docs/research-report.md` §MMC1 for the source material.
//!
//! MMC1 is a serial mapper: writes to `$8000-$FFFF` are accumulated in a 5-bit
//! shift register over five consecutive CPU writes. The fifth write commits
//! the assembled value to one of four internal registers selected by bits
//! 14-13 of the destination address (`$8000-$9FFF` -> control, `$A000-$BFFF`
//! -> CHR0, `$C000-$DFFF` -> CHR1, `$E000-$FFFF` -> PRG). A write whose data
//! has bit 7 set resets the shift register and ORs the control register
//! with `$0C` (forcing PRG mode 3 = "fix last bank @ $C000").
//!
//! Consecutive-write bug: real hardware inhibits the register from accepting
//! a second write on the CPU cycle immediately following an accepted write.
//! Bill & Ted's Excellent Adventure relies on this. We track the cycle
//! counter via [`Mapper::notify_cpu_cycle`] and drop writes that fall on the
//! cycle right after another accepted write.
//!
//! The default revision when the cartridge header lacks an NES 2.0 submapper
//! byte is **Sharp** (project policy: Star Trek: 25th Anniversary requires
//! the Sharp variant). NES 2.0 submapper 5 selects SEROM/SHROM/SH1ROM (no
//! PRG-RAM), but at the level of MMC1 register semantics those are
//! observationally equivalent.

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_4K: usize = 0x1000;
const CHR_BANK_8K: usize = 0x2000;
const PRG_RAM_DEFAULT: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// MMC1 mapper.
pub struct Mmc1 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    prg_ram: Box<[u8]>,
    /// Internal nametable VRAM (2 KiB) — owned by the console, but we keep it
    /// here for now to mirror the NROM stop-gap until the PPU integration in
    /// Sprint 2-1 moves CIRAM into the PPU.
    vram: Box<[u8]>,
    chr_is_ram: bool,

    // MMC1 internal registers (5 bits each).
    control: u8, // mirror, prg-mode, chr-mode
    chr0: u8,    // CHR bank 0 ($A000-$BFFF write target)
    chr1: u8,    // CHR bank 1 ($C000-$DFFF write target)
    prg: u8,     // PRG bank ($E000-$FFFF write target)

    // 5-write protocol shift register + count.
    shift: u8,
    shift_count: u8,

    // Cycle of the most recently accepted register write (for the consecutive-
    // write bug). `u64::MAX` means "no prior write to inhibit on".
    last_write_cycle: u64,
    cpu_cycle: u64,
}

impl Mmc1 {
    /// Construct a new MMC1 mapper.
    ///
    /// `prg_rom` must be a multiple of 16 KiB (typical sizes: 32 KiB up to
    /// 512 KiB for SXROM). CHR-RAM is selected when `chr_rom` is empty;
    /// otherwise CHR-ROM length must be a non-zero multiple of 4 KiB.
    /// `prg_ram_bytes` of 0 selects the default 8 KiB.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when sizes don't match.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        initial_mirroring: Mirroring,
        prg_ram_bytes: usize,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || prg_rom.len() % PRG_BANK_16K != 0 {
            return Err(MapperError::Invalid(format!(
                "MMC1 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len() % CHR_BANK_4K == 0 {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "MMC1 CHR-ROM size {} is not a multiple of 4 KiB",
                chr_rom.len()
            )));
        };

        let prg_ram_size = if prg_ram_bytes == 0 {
            PRG_RAM_DEFAULT
        } else {
            prg_ram_bytes
        };

        // Initial control: PRG mode 3 (fix last bank at $C000-$FFFF), CHR
        // mode 0 (8 KiB), single-screen-A. Per MMC1 power-on per nesdev wiki:
        // "Common sense suggests $0C as a likely starting value because it
        // forces $C000-$FFFF to be the last 16 KiB of PRG and the
        // initial mirroring is undefined; software should set it itself."
        // We start with mirroring derived from the iNES header byte (rather
        // than letting the mapper pick), matching most test ROMs that don't
        // set the control register before issuing reads.
        let initial_control = match initial_mirroring {
            Mirroring::SingleScreenA => 0x0C,
            Mirroring::SingleScreenB => 0x0D,
            Mirroring::Horizontal => 0x0F,
            // Vertical / FourScreen / MapperControlled: default to vertical
            // layout (a sensible neutral pick for headers that don't strictly
            // belong to MMC1 like four-screen).
            _ => 0x0E,
        };

        Ok(Self {
            prg_rom,
            chr,
            prg_ram: vec![0u8; prg_ram_size].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            control: initial_control,
            chr0: 0,
            chr1: 0,
            prg: 0,
            shift: 0x10, // bit 4 set marks "5 writes still needed"
            shift_count: 0,
            last_write_cycle: u64::MAX,
            cpu_cycle: 0,
        })
    }

    /// Map a PPU address in `$2000-$3EFF` to a 2 KiB-VRAM offset using the
    /// current mirroring mode (control bits 1-0).
    fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.current_mirroring().physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }

    /// Number of 16 KiB PRG banks the cartridge holds.
    const fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_16K
    }

    /// Resolve a CPU read at `$8000-$FFFF` into a PRG-ROM byte.
    fn map_prg(&self, addr: u16) -> u8 {
        let prg_mode = (self.control >> 2) & 0x03;
        let bank_count = self.prg_bank_count();
        // PRG bank register is 4 bits in standard MMC1 (16 banks max). For
        // SUROM / SXROM the high bit selects the 256 KiB chip; this is
        // approximated by allowing all 5 bits to address up to 32 banks
        // (512 KiB total). High bit of CHR bank also feeds into PRG bank
        // select on SUROM, but we leave the more exotic behavior for a
        // dedicated Phase 4 sweep — at the level of "instr_test_v5 boots,"
        // the linear interpretation is sufficient.
        let prg_bank = self.prg & 0x0F;

        let (bank_low, bank_high): (usize, usize) = match prg_mode {
            0 | 1 => {
                // 32 KiB switch: bank-low = prg & 0xE, bank-high = bank-low + 1
                let bl = (prg_bank & 0x0E) as usize;
                (bl, bl + 1)
            }
            2 => {
                // First bank fixed to bank 0; second selectable
                (0, prg_bank as usize)
            }
            _ => {
                // Mode 3: first selectable; second fixed to last
                (prg_bank as usize, bank_count - 1)
            }
        };
        let bank_count = bank_count.max(1);
        let bank_low = bank_low % bank_count;
        let bank_high = bank_high % bank_count;

        let offset_in_bank = (addr - 0x8000) as usize & (PRG_BANK_16K - 1);
        let bank = if (addr & 0x4000) == 0 {
            bank_low
        } else {
            bank_high
        };
        self.prg_rom[bank * PRG_BANK_16K + offset_in_bank]
    }

    /// Resolve a PPU read at `$0000-$1FFF` into a CHR byte.
    fn map_chr(&self, addr: u16) -> usize {
        let chr_mode_8k = (self.control & 0x10) == 0;
        if chr_mode_8k {
            // 8 KiB CHR bank: CHR0 selects, low bit forced to 0.
            let bank_count = (self.chr.len() / CHR_BANK_8K).max(1);
            let bank = ((self.chr0 as usize) >> 1) % bank_count;
            bank * CHR_BANK_8K + (addr as usize & (CHR_BANK_8K - 1))
        } else {
            // 4 KiB banks
            let bank_count = (self.chr.len() / CHR_BANK_4K).max(1);
            let bank = if addr < 0x1000 {
                self.chr0 as usize
            } else {
                self.chr1 as usize
            };
            let bank = bank % bank_count;
            bank * CHR_BANK_4K + (addr as usize & (CHR_BANK_4K - 1))
        }
    }

    /// Apply a completed 5-bit write to the appropriate internal register.
    const fn commit(&mut self, addr: u16, value: u8) {
        match addr & 0xE000 {
            0x8000 => self.control = value,
            0xA000 => self.chr0 = value,
            0xC000 => self.chr1 = value,
            // 0xE000
            _ => self.prg = value,
        }
    }
}

impl Mapper for Mmc1 {
    // v2.8.0 Phase 4 — MMC1 overrides ONLY notify_cpu_cycle (the
    // consecutive-write throttle); it has no IRQ and no audio.
    fn caps(&self) -> MapperCaps {
        MapperCaps {
            cpu_cycle_hook: true,
            audio: false,
            frame_event_hook: false,
            irq_source: false,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                let idx = (addr - 0x6000) as usize;
                if idx < self.prg_ram.len() {
                    self.prg_ram[idx]
                } else {
                    0
                }
            }
            0x8000..=0xFFFF => self.map_prg(addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x7FFF => {
                let idx = (addr - 0x6000) as usize;
                if idx < self.prg_ram.len() {
                    self.prg_ram[idx] = value;
                }
            }
            0x8000..=0xFFFF => {
                // Consecutive-write bug: ignore writes on the cycle
                // immediately following another accepted write.
                if self.last_write_cycle != u64::MAX
                    && self.cpu_cycle == self.last_write_cycle.wrapping_add(1)
                {
                    return;
                }
                self.last_write_cycle = self.cpu_cycle;

                if value & 0x80 != 0 {
                    // Reset: clear shift; OR control with $0C (force PRG mode 3).
                    self.shift = 0x10;
                    self.shift_count = 0;
                    self.control |= 0x0C;
                    return;
                }
                // Shift bit 0 of value into bit 4 of shift, sliding right.
                // After 5 writes, bit 0 of (original) shift is the LSB of
                // the latched value; equivalently: low 5 bits, LSB first.
                let new_lsb = value & 0x01;
                self.shift = (self.shift >> 1) | (new_lsb << 4);
                self.shift_count += 1;
                if self.shift_count == 5 {
                    let latched = self.shift & 0x1F;
                    self.commit(addr, latched);
                    self.shift = 0x10;
                    self.shift_count = 0;
                }
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.map_chr(addr);
                self.chr[off]
            }
            0x2000..=0x3EFF => {
                let off = self.nametable_offset(addr);
                self.vram[off]
            }
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.map_chr(addr);
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

    fn notify_cpu_cycle(&mut self) {
        self.cpu_cycle = self.cpu_cycle.wrapping_add(1);
    }

    fn current_mirroring(&self) -> Mirroring {
        match self.control & 0x03 {
            0 => Mirroring::SingleScreenA,
            1 => Mirroring::SingleScreenB,
            2 => Mirroring::Vertical,
            _ => Mirroring::Horizontal,
        }
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 1,
            name: "MMC1".into(),
            mirroring: crate::mapper::mirroring_name(self.current_mirroring()),
            ..Default::default()
        };
        info.prg_banks
            .push(("PRG".into(), format!("{:#04x}", self.prg)));
        info.chr_banks
            .push(("CHR0".into(), format!("{:#04x}", self.chr0)));
        info.chr_banks
            .push(("CHR1".into(), format!("{:#04x}", self.chr1)));
        info.extra
            .push(("control".into(), format!("{:#04x}", self.control)));
        info.extra.push((
            "shift".into(),
            format!("{:#04x} (count {})", self.shift, self.shift_count),
        ));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        // Tagged blob: [version, control, chr0, chr1, prg, shift, count,
        //   prg_ram..., vram..., chr_if_ram...]
        let mut out = Vec::with_capacity(
            7 + self.prg_ram.len()
                + self.vram.len()
                + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.push(self.control);
        out.push(self.chr0);
        out.push(self.chr1);
        out.push(self.prg);
        out.push(self.shift);
        out.push(self.shift_count);
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 7 + self.prg_ram.len() + self.vram.len() + need_chr;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.control = data[1];
        self.chr0 = data[2];
        self.chr1 = data[3];
        self.prg = data[4];
        self.shift = data[5];
        self.shift_count = data[6];
        let mut cursor = 7;
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

    fn synth_prg(banks: usize) -> Box<[u8]> {
        // Each bank starts with a marker byte equal to the bank index, then
        // address-low rolls. Lets us check which bank an address resolves to.
        let mut v = vec![0u8; banks * PRG_BANK_16K];
        for b in 0..banks {
            for o in 0..PRG_BANK_16K {
                v[b * PRG_BANK_16K + o] = if o == 0 { b as u8 } else { (o & 0xFF) as u8 };
            }
        }
        v.into_boxed_slice()
    }

    fn synth_chr(banks_4k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_4k * CHR_BANK_4K];
        for b in 0..banks_4k {
            for o in 0..CHR_BANK_4K {
                v[b * CHR_BANK_4K + o] = if o == 0 { b as u8 } else { (o ^ 0x55) as u8 };
            }
        }
        v.into_boxed_slice()
    }

    /// Issue the 5 bit-writes that latch `value` into the register bank
    /// selected by the high two address bits.
    fn write5(m: &mut Mmc1, addr: u16, value: u8) {
        for i in 0..5 {
            let bit = (value >> i) & 1;
            // Drive notify_cpu_cycle far enough between each write so the
            // consecutive-write bug never fires in tests.
            for _ in 0..3 {
                m.notify_cpu_cycle();
            }
            m.cpu_write(addr, bit);
        }
    }

    #[test]
    fn mmc1_default_is_prg_mode_3_last_bank_at_c000() {
        // Build a 4-bank PRG (64 KiB). Default control sets PRG mode 3:
        // bank 0 selectable @ $8000, last bank fixed @ $C000.
        let mut m = Mmc1::new(synth_prg(4), synth_chr(2), Mirroring::Vertical, 0).unwrap();
        // $C000 should map to bank 3 (last); marker byte = 3.
        assert_eq!(m.cpu_read(0xC000), 3);
        // $8000 starts at bank 0 (prg register defaults to 0).
        assert_eq!(m.cpu_read(0x8000), 0);
    }

    #[test]
    fn mmc1_prg_register_switches_first_bank_in_mode_3() {
        let mut m = Mmc1::new(synth_prg(4), synth_chr(2), Mirroring::Vertical, 0).unwrap();
        // Set PRG bank 2.
        write5(&mut m, 0xE000, 2);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 3); // last bank still fixed
    }

    #[test]
    fn mmc1_prg_mode_2_fixes_first_bank_at_8000() {
        let mut m = Mmc1::new(synth_prg(4), synth_chr(2), Mirroring::Vertical, 0).unwrap();
        // Control: prg-mode = 2 (bits 3-2 = 10), chr-mode = 0, mirror = horiz (3).
        write5(&mut m, 0x8000, 0b0_1011);
        // PRG register selects bank for $C000. Set to 1.
        write5(&mut m, 0xE000, 1);
        assert_eq!(m.cpu_read(0x8000), 0); // fixed bank 0
        assert_eq!(m.cpu_read(0xC000), 1); // selectable
    }

    #[test]
    fn mmc1_prg_mode_0_or_1_switches_32k() {
        let mut m = Mmc1::new(synth_prg(4), synth_chr(2), Mirroring::Vertical, 0).unwrap();
        // Control: prg-mode = 0 (bits 3-2 = 00), chr-mode = 0, mirror = horiz.
        write5(&mut m, 0x8000, 0b0_0011);
        // PRG register = 2 -> 32 KiB switch picks (bank 2, bank 3).
        write5(&mut m, 0xE000, 2);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 3);
    }

    #[test]
    fn mmc1_chr_mode_4k_switches_independent_banks() {
        let mut m = Mmc1::new(synth_prg(2), synth_chr(4), Mirroring::Vertical, 0).unwrap();
        // Control: chr-mode = 1 (bit 4 set), prg-mode = 3, mirror = horiz.
        write5(&mut m, 0x8000, 0b1_1111);
        // CHR0 = bank 1, CHR1 = bank 2.
        write5(&mut m, 0xA000, 1);
        write5(&mut m, 0xC000, 2);
        assert_eq!(m.ppu_read(0x0000), 1);
        assert_eq!(m.ppu_read(0x1000), 2);
    }

    #[test]
    fn mmc1_chr_mode_8k_uses_chr0_only() {
        let mut m = Mmc1::new(synth_prg(2), synth_chr(4), Mirroring::Vertical, 0).unwrap();
        // chr-mode = 0 (bit 4 clear), prg-mode = 3, mirror = horiz.
        write5(&mut m, 0x8000, 0b0_1111);
        // CHR0 = 2 -> 8 KiB bank starts at CHR4K-bank (2 >> 1) * 8 KiB = bank index 1.
        write5(&mut m, 0xA000, 2);
        assert_eq!(m.ppu_read(0x0000), 2); // 4K-bank index 2 -> first byte
        assert_eq!(m.ppu_read(0x1000), 3); // next 4K-bank
    }

    #[test]
    fn mmc1_reset_bit_forces_prg_mode_3() {
        let mut m = Mmc1::new(synth_prg(4), synth_chr(2), Mirroring::Vertical, 0).unwrap();
        // Set prg-mode = 0 first.
        write5(&mut m, 0x8000, 0b0_0011);
        // Now write reset bit.
        m.cpu_write(0x8000, 0x80);
        // Control should have been ORed with $0C. With our prior control of
        // 0b0_0011 (= 0x03), after | 0x0C = 0x0F (mode 3, mirror horiz).
        // Last bank should be fixed at $C000.
        assert_eq!(m.cpu_read(0xC000), 3);
    }

    #[test]
    fn mmc1_consecutive_write_bug_drops_second_write() {
        let mut m = Mmc1::new(synth_prg(4), synth_chr(2), Mirroring::Vertical, 0).unwrap();
        // Write bit 0 of shift. Then on the very next cycle, write bit 1 —
        // hardware drops it.
        m.notify_cpu_cycle();
        m.cpu_write(0xE000, 1); // accepted; shift = 0x10 -> 0x18 (bit 4 from new_lsb)
        m.notify_cpu_cycle();
        m.cpu_write(0xE000, 1); // dropped (cycle = last_write+1)
        // Continue with 4 more accepted writes.
        for _ in 0..4 {
            for _ in 0..3 {
                m.notify_cpu_cycle();
            }
            m.cpu_write(0xE000, 0);
        }
        // We accepted bits: 1, 0, 0, 0, 0 -> latched value = 0b00001 = 1.
        // PRG register = 1, mode 3 default, so $8000 -> bank 1.
        assert_eq!(m.cpu_read(0x8000), 1);
    }

    #[test]
    fn mmc1_mirroring_switches() {
        let mut m = Mmc1::new(synth_prg(2), synth_chr(2), Mirroring::Vertical, 0).unwrap();
        // single-screen-A
        write5(&mut m, 0x8000, 0b0_1100);
        m.ppu_write(0x2000, 0xAA);
        assert_eq!(m.ppu_read(0x2400), 0xAA);
        assert_eq!(m.ppu_read(0x2800), 0xAA);
        assert_eq!(m.ppu_read(0x2C00), 0xAA);
        // single-screen-B
        write5(&mut m, 0x8000, 0b0_1101);
        m.ppu_write(0x2000, 0xBB); // writes physical bank 1
        // All four nametables now alias to bank 1.
        assert_eq!(m.ppu_read(0x2400), 0xBB);
        // vertical
        write5(&mut m, 0x8000, 0b0_1110);
        m.ppu_write(0x2000, 0x11); // bank 0
        m.ppu_write(0x2400, 0x22); // bank 1
        assert_eq!(m.ppu_read(0x2800), 0x11); // mirror of $2000
        assert_eq!(m.ppu_read(0x2C00), 0x22); // mirror of $2400
        // horizontal
        write5(&mut m, 0x8000, 0b0_1111);
        m.ppu_write(0x2000, 0x33); // bank 0
        m.ppu_write(0x2800, 0x44); // bank 1
        assert_eq!(m.ppu_read(0x2400), 0x33); // mirror of $2000
        assert_eq!(m.ppu_read(0x2C00), 0x44); // mirror of $2800
    }

    #[test]
    fn mmc1_save_state_round_trip() {
        let mut m = Mmc1::new(synth_prg(2), synth_chr(2), Mirroring::Vertical, 0).unwrap();
        write5(&mut m, 0xE000, 1);
        m.cpu_write(0x6010, 0xCC);
        m.ppu_write(0x2000, 0xDD);
        let blob = m.save_state();
        let mut m2 = Mmc1::new(synth_prg(2), synth_chr(2), Mirroring::Vertical, 0).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x6010), 0xCC);
        assert_eq!(m2.ppu_read(0x2000), 0xDD);
        // PRG register should have round-tripped.
        assert_eq!(m2.cpu_read(0x8000), 1);
    }
}

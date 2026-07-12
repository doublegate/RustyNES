//! Fuzz target for `rustynes_ppu::Ppu` CPU-facing register I/O.
//!
//! Drives the $2000–$2007 register file (`cpu_write_register` /
//! `cpu_read_register`) with an arbitrary stream of `(register, value)` pairs.
//! This is the CPU's untrusted-from-the-PPU's-view boundary: PPUCTRL/PPUMASK
//! side effects, the PPUADDR/PPUSCROLL write-toggle (`w`) latch, the PPUDATA
//! VRAM read buffer + auto-increment, OAMADDR/OAMDATA, and the PPUSTATUS read
//! that clears vblank + resets the toggle. Validates no panics / OOB indexing
//! for any sequence of register accesses.
//!
//! Run with:
//!     cargo install cargo-fuzz
//!     cargo +nightly fuzz run ppu_reg_io
//!
//! Per `docs/testing-strategy.md` §Layer 5.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rustynes_ppu::{Ppu, PpuBus, PpuRegion};

/// Minimal `PpuBus`: an 8 KiB CHR/pattern-table window backing every PPU-space
/// read/write. Nametable mirroring uses the trait's default `nametable_address`.
/// This is exactly the surface `cpu_read_register`/`cpu_write_register` reach
/// through for PPUDATA ($2007) — no mapper side effects, no A12 notifications.
struct FuzzPpuBus {
    vram: Box<[u8; 0x4000]>,
}

impl PpuBus for FuzzPpuBus {
    fn ppu_read(&mut self, addr: u16) -> u8 {
        self.vram[(addr & 0x3FFF) as usize]
    }
    fn ppu_write(&mut self, addr: u16, value: u8) {
        self.vram[(addr & 0x3FFF) as usize] = value;
    }
}

fuzz_target!(|data: &[u8]| {
    // Each access is a (register, value) pair; anything shorter is uninteresting.
    if data.len() < 2 {
        return;
    }

    // Alternate NTSC / PAL by the first byte so both timebases' register paths
    // are covered across runs.
    let region = if data[0] & 1 == 0 {
        PpuRegion::Ntsc
    } else {
        PpuRegion::Pal
    };
    let mut ppu = Ppu::new(region);
    let mut bus = FuzzPpuBus {
        vram: Box::new([0u8; 0x4000]),
    };

    // Walk the remaining bytes as (reg, value) pairs. The low 3 bits select the
    // register ($2000..$2007); the high bit of the reg byte chooses read vs.
    // write, so the stream freely interleaves both directions.
    let mut i = 1;
    while i + 1 < data.len() {
        let sel = data[i];
        let value = data[i + 1];
        let reg = sel & 0x07;
        if sel & 0x80 == 0 {
            ppu.cpu_write_register(reg, value, &mut bus);
        } else {
            let _ = ppu.cpu_read_register(reg, &mut bus);
        }
        i += 2;
    }
});

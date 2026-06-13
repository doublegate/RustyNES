//! Fuzz target for mapper register writes.
//!
//! For every supported mapper id, synthesizes the smallest plausible iNES ROM
//! and then feeds arbitrary `(addr, value)` write sequences to the mapper.
//! Validates no panics / OOB indexing across bank-register, IRQ-counter, and
//! audio-register write paths.
//!
//! Particularly catches:
//!   - OOB indexing in mapper bank tables (PRG/CHR bank selectors).
//!   - Arithmetic overflow in IRQ counters (MMC3, MMC5, VRC4/6, FME-7, N163).
//!   - Bad mirroring / nametable layout writes.
//!
//! Run with:
//!     cargo install cargo-fuzz
//!     cargo +nightly fuzz run mapper_writes
//!
//! Per `docs/testing-strategy.md` §Layer 5.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rustynes_mappers::{parse, Mapper};

/// Build a minimal iNES 1.0 ROM with the given mapper id, 32 KiB PRG, 8 KiB CHR.
fn synth_rom(mapper_id: u8) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16 + 32 * 1024 + 8 * 1024);
    // 16 byte iNES 1.0 header: magic + sizes + flags6/7 + 8 reserved bytes.
    // flags6: mapper low nibble << 4 + horizontal mirroring (bit 0 = 0).
    // flags7: mapper high nibble << 4 (no other bits).
    bytes.extend_from_slice(b"NES\x1A");
    bytes.push(2);
    bytes.push(1);
    bytes.push((mapper_id & 0x0F) << 4);
    bytes.push(mapper_id & 0xF0);
    bytes.extend_from_slice(&[0u8; 8]);
    bytes.extend(core::iter::repeat(0u8).take(32 * 1024 + 8 * 1024));
    bytes
}

const SUPPORTED_MAPPERS: &[u8] = &[
    0,  // NROM
    1,  // MMC1
    2,  // UxROM
    3,  // CNROM
    4,  // MMC3
    5,  // MMC5
    7,  // AxROM
    9,  // MMC2
    10, // MMC4
    11, // ColorDreams
    13, // CPROM
    19, // Namco 163
    21, // VRC4 (alt)
    22, // VRC2
    23, // VRC2/4
    24, // VRC6
    25, // VRC2/4
    26, // VRC6
    34, // BNROM / NINA-001
    66, // GxROM
    69, // FME-7
    71, // Camerica
    75, // VRC1
];

fn build_mapper(mapper_id: u8) -> Option<Box<dyn Mapper>> {
    let rom = synth_rom(mapper_id);
    parse(&rom).ok().map(|(_cart, m)| m)
}

fuzz_target!(|data: &[u8]| {
    // First byte (mod len(SUPPORTED_MAPPERS)) selects which mapper.  Remaining
    // bytes are (addr_hi, addr_lo, value) triples.
    if data.is_empty() {
        return;
    }
    let mapper_id = SUPPORTED_MAPPERS[(data[0] as usize) % SUPPORTED_MAPPERS.len()];
    let Some(mut mapper) = build_mapper(mapper_id) else {
        return;
    };

    let mut i = 1;
    while i + 2 < data.len() {
        let addr = u16::from(data[i]) | (u16::from(data[i + 1]) << 8);
        let value = data[i + 2];
        // Only $4020-$FFFF is the cartridge window on the CPU side; clip
        // there so we don't shoot into the PPU/APU register space.
        let addr_cpu = addr | 0x4020;
        mapper.cpu_write(addr_cpu, value);
        // Mix in a sprinkling of PPU bus pokes too (pattern table is $0000-$1FFF,
        // nametables $2000-$3EFF -- the mapper only writes CHR-RAM and some
        // mappers absorb nametable writes via `nametable_write`).
        let addr_ppu = addr & 0x3FFF;
        mapper.ppu_write(addr_ppu, value);
        // Tickle the IRQ-clocking notifications so VRC/N163/FME-7 IRQ
        // counters are exercised.
        mapper.notify_cpu_cycle();
        if (value & 0x80) != 0 {
            // Bit 7 of the data byte randomly drives A12 high; bit 6 low.
            mapper.notify_a12((value & 0x40) != 0);
        }
        if (value & 0x20) != 0 {
            mapper.notify_scanline_start();
        }
        if (value & 0x10) != 0 {
            mapper.notify_vblank();
        }
        // Periodic reads to flush bank-table accesses.
        let _ = mapper.cpu_read(addr_cpu);
        let _ = mapper.ppu_read(addr_ppu);
        i += 3;
    }
});

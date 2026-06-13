//! Fuzz target for `rustynes_mappers::parse`.
//!
//! Validates that ANY 0..=65,536-byte input is handled by returning either
//! `Ok((Cartridge, Box<dyn Mapper>))` or a typed `RomError` -- never a panic,
//! OOB read, or hang. The cartridge parser is the highest-value fuzz target
//! because the iNES / NES 2.0 header is an untrusted, attacker-controlled
//! input surface.
//!
//! Run with:
//!     cargo install cargo-fuzz
//!     cargo +nightly fuzz run cartridge_parser
//!
//! Per `docs/testing-strategy.md` §Layer 5.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Cap to 64 KiB so the fuzzer doesn't get distracted by gigantic inputs
    // that exceed any plausible iNES file size.  Real-world NES ROMs top
    // out around 1 MiB but the header itself plus a few KiB exercises every
    // dispatch path.
    if data.len() > 65_536 {
        return;
    }
    // Smoke the parser.  Mapper trait objects are constructed for every
    // valid mapper id; we exercise a single CPU read at $8000 plus a PPU
    // read at $0000 to flush the slice indexing in the bank-table paths.
    if let Ok((_cart, mut mapper)) = rustynes_mappers::parse(data) {
        let _ = mapper.cpu_read(0x8000);
        let _ = mapper.cpu_read(0xC000);
        let _ = mapper.cpu_read(0xFFFF);
        let _ = mapper.ppu_read(0x0000);
        let _ = mapper.ppu_read(0x1FFF);
    }
});

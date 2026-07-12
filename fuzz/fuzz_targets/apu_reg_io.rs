//! Fuzz target for `rustynes_apu::Apu` CPU-facing register I/O.
//!
//! Drives the $4000–$4017 register file (`write_register`) plus the $4015
//! status read (`read_status`) with an arbitrary stream of `(register, value)`
//! pairs. This exercises the pulse/triangle/noise/DMC register decoders, the
//! length-counter halt/reload write ordering, the frame-counter mode+IRQ-inhibit
//! write ($4017), the channel-enable mask ($4015 write), and the status read
//! that clears the frame IRQ. `write_register` / `read_status` take no bus (the
//! DMC sample fetch only happens during clocking), so this is a self-contained
//! untrusted-input boundary. Validates no panics / OOB indexing.
//!
//! Run with:
//!     cargo install cargo-fuzz
//!     cargo +nightly fuzz run apu_reg_io
//!
//! Per `docs/testing-strategy.md` §Layer 5.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rustynes_apu::{Apu, Region};

fuzz_target!(|data: &[u8]| {
    // Each access is a (register-selector, value) pair; shorter is uninteresting.
    if data.len() < 2 {
        return;
    }

    // Alternate NTSC / PAL by the first byte so both frame-counter step tables
    // are covered across runs. A fixed 48 kHz output rate is irrelevant to the
    // register-decode paths under test.
    let region = if data[0] & 1 == 0 {
        Region::Ntsc
    } else {
        Region::Pal
    };
    let mut apu = Apu::new(region, 48_000);

    // Map the selector byte across the full $4000..=$4017 register window (24
    // registers) and treat the top bit as a read/write toggle so the stream
    // interleaves status reads with writes.
    let mut i = 1;
    while i + 1 < data.len() {
        let sel = data[i];
        let value = data[i + 1];
        if sel & 0x80 != 0 {
            // A status read ($4015) — clears the frame IRQ flag.
            let _ = apu.read_status();
        } else {
            let addr = 0x4000 + u16::from(sel % 24);
            apu.write_register(addr, value);
        }
        i += 2;
    }
});

//! Fuzz target for `rustynes_cpu::Cpu::step`.
//!
//! Drives the 256-opcode dispatch (incl. unofficial / JAM-class) by feeding
//! arbitrary RAM contents and arbitrary register seeds.  Validates no panics
//! or OOB indexing for any input.
//!
//! Run with:
//!     cargo install cargo-fuzz
//!     cargo +nightly fuzz run cpu_step
//!
//! Per `docs/testing-strategy.md` §Layer 5.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rustynes_cpu::{Bus, Cpu, Status};

/// Flat 64 KiB RAM bus with no PPU/APU side-effects.  Reads return whatever
/// the fuzzer seeded; writes update the same byte array (so self-modifying
/// fuzz cases continue to make forward progress).  NMI / IRQ are tied low.
struct FuzzBus {
    ram: Box<[u8; 0x1_0000]>,
}

impl Bus for FuzzBus {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.ram[addr as usize]
    }
    fn cpu_write(&mut self, addr: u16, value: u8) {
        self.ram[addr as usize] = value;
    }
}

fuzz_target!(|data: &[u8]| {
    // We need at least a small seed: 7 bytes of CPU register state +
    // 0..=65,536 RAM bytes.  Anything shorter is uninteresting.
    if data.len() < 7 {
        return;
    }
    let mut cpu = Cpu::new();
    cpu.a = data[0];
    cpu.x = data[1];
    cpu.y = data[2];
    cpu.s = data[3];
    cpu.p = Status::from_bits_truncate(data[4]);
    cpu.pc = u16::from(data[5]) | (u16::from(data[6]) << 8);

    let mut ram: Box<[u8; 0x1_0000]> = Box::new([0u8; 0x1_0000]);
    // Fill RAM with the remainder of the fuzz input (truncate / pad to 64 KiB).
    let bytes = &data[7..];
    let n = bytes.len().min(ram.len());
    ram[..n].copy_from_slice(&bytes[..n]);
    let mut bus = FuzzBus { ram };

    // Cap steps so the fuzzer doesn't hang on a tight loop.  64 instructions
    // is enough to exercise instruction-internal state machines (branches,
    // page-cross fixups, indirect-X fetches) without burning time.
    for _ in 0..64 {
        if cpu.is_jammed() {
            break;
        }
        let _cycles = cpu.step(&mut bus);
    }
});

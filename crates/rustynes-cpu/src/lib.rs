//! RustyNES CPU - Cycle-accurate 6502 emulation
//!
//! This crate provides a cycle-accurate implementation of the MOS 6502 CPU
//! as used in the Nintendo Entertainment System (NES). It includes:
//!
//! - All 256 opcodes (151 official + 105 unofficial)
//! - Cycle-accurate timing
//! - Complete interrupt handling (NMI, IRQ, BRK, RESET)
//! - All addressing modes
//! - Zero unsafe code
//!
//! # Example
//!
//! ```no_run
//! use rustynes_cpu::{Cpu, Bus};
//!
//! // Implement the Bus trait for your system
//! struct MyBus {
//!     memory: [u8; 0x10000],
//! }
//!
//! impl Bus for MyBus {
//!     fn read(&mut self, addr: u16) -> u8 {
//!         self.memory[addr as usize]
//!     }
//!
//!     fn write(&mut self, addr: u16, value: u8) {
//!         self.memory[addr as usize] = value;
//!     }
//! }
//!
//! fn main() {
//!     let mut cpu = Cpu::new();
//!     let mut bus = MyBus { memory: [0; 0x10000] };
//!
//!     // Set RESET vector to 0x8000
//!     bus.memory[0xFFFC] = 0x00;
//!     bus.memory[0xFFFD] = 0x80;
//!
//!     // Reset CPU
//!     cpu.reset(&mut bus);
//!
//!     // Execute instructions
//!     loop {
//!         let cycles = cpu.step(&mut bus);
//!         // Execute cycles CPU cycles worth of other system components
//!     }
//! }
//! ```
//!
//! # Accuracy
//!
//! This implementation is designed to pass:
//! - nestest.nes golden log
//! - blargg's cpu_timing_test6
//! - All TASVideos accuracy tests
//!
//! # Architecture
//!
//! - **Modular Design**: CPU, PPU, APU, and mappers are separate crates
//! - **Strong Typing**: Newtype pattern for addresses and flags
//! - **Safe Code**: Zero unsafe blocks (except for FFI in other crates)
//! - **Trait-Based**: `Bus` trait allows flexible memory systems
//!
//! # Feature Flags
//!
//! Currently no optional features. All functionality is included by default.

// Lints are configured in the workspace Cargo.toml
// This ensures consistent settings across all crates

mod addressing;
mod bus;
mod cpu;
pub mod ines;
mod instructions;
mod opcodes;
pub mod state;
mod status;
pub mod trace;

// Public exports
pub use addressing::AddressingMode;
pub use bus::{Bus, CpuBus};
pub use cpu::Cpu;
pub use ines::{INesHeader, INesRom};
pub use status::StatusFlags;
pub use trace::CpuTracer;

// Re-export for convenience
pub use opcodes::OPCODE_TABLE;

#[cfg(test)]
mod tests {
    use super::*;

    struct TestBus {
        memory: Vec<u8>,
    }

    impl TestBus {
        fn new() -> Self {
            Self {
                memory: vec![0; 0x10000],
            }
        }

        fn load_program(&mut self, start: u16, program: &[u8]) {
            let start = start as usize;
            self.memory[start..start + program.len()].copy_from_slice(program);
        }
    }

    impl Bus for TestBus {
        fn read(&mut self, addr: u16) -> u8 {
            self.memory[addr as usize]
        }

        fn write(&mut self, addr: u16, value: u8) {
            self.memory[addr as usize] = value;
        }
    }

    #[test]
    fn test_lda_immediate() {
        let mut cpu = Cpu::new();
        let mut bus = TestBus::new();

        // LDA #$42
        bus.load_program(0x8000, &[0xA9, 0x42]);
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        cpu.reset(&mut bus);
        assert_eq!(cpu.pc, 0x8000);

        let cycles = cpu.step(&mut bus);
        assert_eq!(cycles, 2);
        assert_eq!(cpu.a, 0x42);
        assert_eq!(cpu.pc, 0x8002);
    }

    #[test]
    fn test_tax_transfer() {
        let mut cpu = Cpu::new();
        let mut bus = TestBus::new();

        // LDA #$42, TAX
        bus.load_program(0x8000, &[0xA9, 0x42, 0xAA]);
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        cpu.reset(&mut bus);

        cpu.step(&mut bus); // LDA
        assert_eq!(cpu.a, 0x42);

        cpu.step(&mut bus); // TAX
        assert_eq!(cpu.x, 0x42);
    }

    #[test]
    fn test_adc_carry() {
        let mut cpu = Cpu::new();
        let mut bus = TestBus::new();

        // LDA #$FF, ADC #$01
        bus.load_program(0x8000, &[0xA9, 0xFF, 0x69, 0x01]);
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        cpu.reset(&mut bus);

        cpu.step(&mut bus); // LDA #$FF
        assert_eq!(cpu.a, 0xFF);

        cpu.step(&mut bus); // ADC #$01
        assert_eq!(cpu.a, 0x00); // Wraps to 0
        assert!(cpu.status.contains(StatusFlags::ZERO));
        assert!(cpu.status.contains(StatusFlags::CARRY));
    }

    #[test]
    fn test_branch_not_taken() {
        let mut cpu = Cpu::new();
        let mut bus = TestBus::new();

        // LDA #$00, BNE +2
        bus.load_program(0x8000, &[0xA9, 0x00, 0xD0, 0x02]);
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        cpu.reset(&mut bus);

        cpu.step(&mut bus); // LDA #$00 (sets Z flag)
        let cycles = cpu.step(&mut bus); // BNE (not taken because Z=1)

        assert_eq!(cycles, 2); // Branch not taken
        assert_eq!(cpu.pc, 0x8004);
    }

    #[test]
    fn test_branch_taken_same_page() {
        let mut cpu = Cpu::new();
        let mut bus = TestBus::new();

        // LDA #$01, BNE +2
        bus.load_program(0x8000, &[0xA9, 0x01, 0xD0, 0x02]);
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        cpu.reset(&mut bus);

        cpu.step(&mut bus); // LDA #$01 (clears Z flag)
        let cycles = cpu.step(&mut bus); // BNE (taken, same page)

        assert_eq!(cycles, 3); // Branch taken, same page
        assert_eq!(cpu.pc, 0x8006);
    }

    #[test]
    fn test_jsr_rts() {
        let mut cpu = Cpu::new();
        let mut bus = TestBus::new();

        // JSR $8010, ..., RTS at $8010
        bus.load_program(0x8000, &[0x20, 0x10, 0x80]);
        bus.memory[0x8010] = 0x60; // RTS
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        cpu.reset(&mut bus);

        let old_sp = cpu.sp;
        cpu.step(&mut bus); // JSR
        assert_eq!(cpu.pc, 0x8010);
        assert_eq!(cpu.sp, old_sp.wrapping_sub(2)); // Pushed 2 bytes

        cpu.step(&mut bus); // RTS
        assert_eq!(cpu.pc, 0x8003); // Returns to next instruction
        assert_eq!(cpu.sp, old_sp); // SP restored
    }

    #[test]
    fn test_unofficial_lax() {
        let mut cpu = Cpu::new();
        let mut bus = TestBus::new();

        // LAX $42 (Zero Page)
        bus.load_program(0x8000, &[0xA7, 0x42]);
        bus.memory[0x0042] = 0x55;
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        cpu.reset(&mut bus);

        let cycles = cpu.step(&mut bus);
        assert_eq!(cycles, 3);
        assert_eq!(cpu.a, 0x55);
        assert_eq!(cpu.x, 0x55);
    }

    #[test]
    fn test_stack_operations() {
        let mut cpu = Cpu::new();
        let mut bus = TestBus::new();

        // LDA #$42, PHA, LDA #$00, PLA
        bus.load_program(0x8000, &[0xA9, 0x42, 0x48, 0xA9, 0x00, 0x68]);
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        cpu.reset(&mut bus);

        cpu.step(&mut bus); // LDA #$42
        let sp = cpu.sp;
        cpu.step(&mut bus); // PHA
        assert_eq!(cpu.sp, sp.wrapping_sub(1));

        cpu.step(&mut bus); // LDA #$00
        assert_eq!(cpu.a, 0x00);

        cpu.step(&mut bus); // PLA
        assert_eq!(cpu.a, 0x42);
        assert_eq!(cpu.sp, sp);
    }
}

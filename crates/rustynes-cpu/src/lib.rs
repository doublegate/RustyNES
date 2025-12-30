//! Cycle-accurate MOS 6502 CPU emulator for NES.
//!
//! This crate provides a cycle-accurate emulation of the MOS 6502 CPU
//! as used in the Nintendo Entertainment System (NES). It supports:
//!
//! - All 256 opcodes (official and unofficial)
//! - Cycle-accurate timing with per-cycle state machine execution
//! - Proper interrupt handling (NMI, IRQ, BRK) with correct timing
//! - DMA support for OAM and DMC transfers
//! - Page boundary crossing penalty cycles
//!
//! # Architecture
//!
//! The CPU uses a trait-based abstraction for memory access via the [`Bus`] trait,
//! allowing it to be integrated with any memory subsystem.
//!
//! # Example
//!
//! ```no_run
//! use rustynes_cpu::{Cpu, Bus};
//!
//! struct SimpleBus {
//!     memory: [u8; 65536],
//! }
//!
//! impl Bus for SimpleBus {
//!     fn read(&mut self, addr: u16) -> u8 {
//!         self.memory[addr as usize]
//!     }
//!
//!     fn write(&mut self, addr: u16, value: u8) {
//!         self.memory[addr as usize] = value;
//!     }
//! }
//!
//! let mut bus = SimpleBus { memory: [0; 65536] };
//! let mut cpu = Cpu::new();
//! cpu.reset(&mut bus);
//! cpu.step(&mut bus);
//! ```

#![warn(missing_docs)]

mod addressing;
mod cpu;
mod instructions;
mod status;

pub use addressing::AddrMode;
pub use cpu::{Bus, Cpu, CpuState, Interrupt};
pub use status::Status;

/// CPU error types.
#[derive(Debug, Clone, thiserror::Error)]
pub enum CpuError {
    /// Invalid opcode encountered.
    #[error("Invalid opcode: 0x{0:02X} at address 0x{1:04X}")]
    InvalidOpcode(u8, u16),
}

/// Result type for CPU operations.
pub type Result<T> = std::result::Result<T, CpuError>;

/// Interrupt vector addresses.
pub mod vectors {
    /// NMI (Non-Maskable Interrupt) vector address.
    pub const NMI: u16 = 0xFFFA;
    /// Reset vector address.
    pub const RESET: u16 = 0xFFFC;
    /// IRQ/BRK vector address.
    pub const IRQ: u16 = 0xFFFE;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestBus {
        memory: [u8; 65536],
    }

    impl TestBus {
        fn new() -> Self {
            Self { memory: [0; 65536] }
        }

        fn load_program(&mut self, addr: u16, program: &[u8]) {
            for (i, &byte) in program.iter().enumerate() {
                self.memory[addr as usize + i] = byte;
            }
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
    fn test_cpu_reset() {
        let mut bus = TestBus::new();
        // Set reset vector to 0x8000
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);

        assert_eq!(cpu.pc(), 0x8000);
        assert_eq!(cpu.sp(), 0xFD);
        assert!(cpu.status().contains(Status::I));
        assert!(cpu.status().contains(Status::U));
    }

    #[test]
    fn test_lda_immediate() {
        let mut bus = TestBus::new();
        // LDA #$42
        bus.load_program(0x8000, &[0xA9, 0x42]);
        // Set reset vector
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus);

        assert_eq!(cpu.a(), 0x42);
        assert_eq!(cpu.pc(), 0x8002);
        assert!(!cpu.status().contains(Status::Z));
        assert!(!cpu.status().contains(Status::N));
    }

    #[test]
    fn test_lda_zero_flag() {
        let mut bus = TestBus::new();
        // LDA #$00
        bus.load_program(0x8000, &[0xA9, 0x00]);
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus);

        assert_eq!(cpu.a(), 0x00);
        assert!(cpu.status().contains(Status::Z));
        assert!(!cpu.status().contains(Status::N));
    }

    #[test]
    fn test_lda_negative_flag() {
        let mut bus = TestBus::new();
        // LDA #$80
        bus.load_program(0x8000, &[0xA9, 0x80]);
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus);

        assert_eq!(cpu.a(), 0x80);
        assert!(!cpu.status().contains(Status::Z));
        assert!(cpu.status().contains(Status::N));
    }

    #[test]
    fn test_sta_zero_page() {
        let mut bus = TestBus::new();
        // LDA #$42, STA $10
        bus.load_program(0x8000, &[0xA9, 0x42, 0x85, 0x10]);
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA
        cpu.step(&mut bus); // STA

        assert_eq!(bus.memory[0x10], 0x42);
    }

    #[test]
    fn test_adc_no_carry() {
        let mut bus = TestBus::new();
        // LDA #$10, ADC #$20
        bus.load_program(0x8000, &[0xA9, 0x10, 0x69, 0x20]);
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA
        cpu.step(&mut bus); // ADC

        assert_eq!(cpu.a(), 0x30);
        assert!(!cpu.status().contains(Status::C));
        assert!(!cpu.status().contains(Status::V));
    }

    #[test]
    fn test_adc_with_carry() {
        let mut bus = TestBus::new();
        // LDA #$FF, ADC #$02
        bus.load_program(0x8000, &[0xA9, 0xFF, 0x69, 0x02]);
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA
        cpu.step(&mut bus); // ADC

        assert_eq!(cpu.a(), 0x01);
        assert!(cpu.status().contains(Status::C));
    }

    #[test]
    fn test_jmp_absolute() {
        let mut bus = TestBus::new();
        // JMP $8010
        bus.load_program(0x8000, &[0x4C, 0x10, 0x80]);
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus);

        assert_eq!(cpu.pc(), 0x8010);
    }

    #[test]
    fn test_jsr_and_rts() {
        let mut bus = TestBus::new();
        // JSR $8010
        bus.load_program(0x8000, &[0x20, 0x10, 0x80]);
        // RTS at $8010
        bus.memory[0x8010] = 0x60;
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);

        let initial_sp = cpu.sp();
        cpu.step(&mut bus); // JSR

        assert_eq!(cpu.pc(), 0x8010);
        assert_eq!(cpu.sp(), initial_sp.wrapping_sub(2));

        cpu.step(&mut bus); // RTS

        assert_eq!(cpu.pc(), 0x8003);
        assert_eq!(cpu.sp(), initial_sp);
    }

    #[test]
    fn test_branch_taken() {
        let mut bus = TestBus::new();
        // LDA #$00, BEQ +$05
        bus.load_program(0x8000, &[0xA9, 0x00, 0xF0, 0x05]);
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA
        cpu.step(&mut bus); // BEQ

        // PC should be at 0x8004 + 0x05 = 0x8009
        assert_eq!(cpu.pc(), 0x8009);
    }

    #[test]
    fn test_branch_not_taken() {
        let mut bus = TestBus::new();
        // LDA #$01, BEQ +$05
        bus.load_program(0x8000, &[0xA9, 0x01, 0xF0, 0x05]);
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA
        cpu.step(&mut bus); // BEQ

        // Branch not taken, PC should be at 0x8004
        assert_eq!(cpu.pc(), 0x8004);
    }

    #[test]
    fn test_push_and_pull() {
        let mut bus = TestBus::new();
        // LDA #$42, PHA, LDA #$00, PLA
        bus.load_program(0x8000, &[0xA9, 0x42, 0x48, 0xA9, 0x00, 0x68]);
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA #$42
        cpu.step(&mut bus); // PHA
        cpu.step(&mut bus); // LDA #$00
        assert_eq!(cpu.a(), 0x00);
        cpu.step(&mut bus); // PLA
        assert_eq!(cpu.a(), 0x42);
    }

    #[test]
    fn test_cycle_count() {
        let mut bus = TestBus::new();
        // LDA #$42 (2 cycles)
        bus.load_program(0x8000, &[0xA9, 0x42]);
        bus.memory[0xFFFC] = 0x00;
        bus.memory[0xFFFD] = 0x80;

        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);

        let cycles_before = cpu.cycles();
        cpu.step(&mut bus);
        let cycles_after = cpu.cycles();

        // LDA immediate takes 2 cycles
        assert_eq!(cycles_after - cycles_before, 2);
    }
}

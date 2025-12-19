//! Opcode definitions and lookup tables for the 6502 CPU.
//!
//! This module contains all 256 opcodes (151 official + 105 unofficial)
//! with their mnemonics, addressing modes, base cycle counts, and page crossing penalties.

use crate::addressing::AddressingMode;

/// Opcode information structure.
#[derive(Debug, Clone, Copy)]
pub struct OpcodeInfo {
    /// Instruction mnemonic (for debugging)
    pub mnemonic: &'static str,
    /// Addressing mode
    pub addr_mode: AddressingMode,
    /// Base cycle count
    pub cycles: u8,
    /// Whether this instruction can take an extra cycle on page crossing
    pub page_cross_penalty: bool,
    /// Whether this is an unofficial opcode
    pub unofficial: bool,
}

/// Complete 256-entry opcode lookup table.
///
/// Indexed by opcode byte (0x00-0xFF). Includes both official and unofficial opcodes.
pub const OPCODE_TABLE: [OpcodeInfo; 256] = [
    // 0x00-0x0F
    OpcodeInfo {
        mnemonic: "BRK",
        addr_mode: AddressingMode::Implied,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x00
    OpcodeInfo {
        mnemonic: "ORA",
        addr_mode: AddressingMode::IndexedIndirectX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x01
    OpcodeInfo {
        mnemonic: "JAM",
        addr_mode: AddressingMode::Implied,
        cycles: 0,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x02 (unofficial)
    OpcodeInfo {
        mnemonic: "SLO",
        addr_mode: AddressingMode::IndexedIndirectX,
        cycles: 8,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x03 (unofficial)
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x04 (unofficial)
    OpcodeInfo {
        mnemonic: "ORA",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x05
    OpcodeInfo {
        mnemonic: "ASL",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x06
    OpcodeInfo {
        mnemonic: "SLO",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x07 (unofficial)
    OpcodeInfo {
        mnemonic: "PHP",
        addr_mode: AddressingMode::Implied,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x08
    OpcodeInfo {
        mnemonic: "ORA",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x09
    OpcodeInfo {
        mnemonic: "ASL",
        addr_mode: AddressingMode::Accumulator,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x0A
    OpcodeInfo {
        mnemonic: "ANC",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x0B (unofficial)
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x0C (unofficial)
    OpcodeInfo {
        mnemonic: "ORA",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x0D
    OpcodeInfo {
        mnemonic: "ASL",
        addr_mode: AddressingMode::Absolute,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x0E
    OpcodeInfo {
        mnemonic: "SLO",
        addr_mode: AddressingMode::Absolute,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x0F (unofficial)
    // 0x10-0x1F
    OpcodeInfo {
        mnemonic: "BPL",
        addr_mode: AddressingMode::Relative,
        cycles: 2,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x10
    OpcodeInfo {
        mnemonic: "ORA",
        addr_mode: AddressingMode::IndirectIndexedY,
        cycles: 5,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x11
    OpcodeInfo {
        mnemonic: "JAM",
        addr_mode: AddressingMode::Implied,
        cycles: 0,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x12 (unofficial)
    OpcodeInfo {
        mnemonic: "SLO",
        addr_mode: AddressingMode::IndirectIndexedY,
        cycles: 8,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x13 (unofficial)
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x14 (unofficial)
    OpcodeInfo {
        mnemonic: "ORA",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x15
    OpcodeInfo {
        mnemonic: "ASL",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x16
    OpcodeInfo {
        mnemonic: "SLO",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x17 (unofficial)
    OpcodeInfo {
        mnemonic: "CLC",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x18
    OpcodeInfo {
        mnemonic: "ORA",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x19
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x1A (unofficial)
    OpcodeInfo {
        mnemonic: "SLO",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x1B (unofficial)
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: true,
    }, // 0x1C (unofficial)
    OpcodeInfo {
        mnemonic: "ORA",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x1D
    OpcodeInfo {
        mnemonic: "ASL",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x1E
    OpcodeInfo {
        mnemonic: "SLO",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x1F (unofficial)
    // 0x20-0x2F
    OpcodeInfo {
        mnemonic: "JSR",
        addr_mode: AddressingMode::Absolute,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x20
    OpcodeInfo {
        mnemonic: "AND",
        addr_mode: AddressingMode::IndexedIndirectX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x21
    OpcodeInfo {
        mnemonic: "JAM",
        addr_mode: AddressingMode::Implied,
        cycles: 0,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x22 (unofficial)
    OpcodeInfo {
        mnemonic: "RLA",
        addr_mode: AddressingMode::IndexedIndirectX,
        cycles: 8,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x23 (unofficial)
    OpcodeInfo {
        mnemonic: "BIT",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x24
    OpcodeInfo {
        mnemonic: "AND",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x25
    OpcodeInfo {
        mnemonic: "ROL",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x26
    OpcodeInfo {
        mnemonic: "RLA",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x27 (unofficial)
    OpcodeInfo {
        mnemonic: "PLP",
        addr_mode: AddressingMode::Implied,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x28
    OpcodeInfo {
        mnemonic: "AND",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x29
    OpcodeInfo {
        mnemonic: "ROL",
        addr_mode: AddressingMode::Accumulator,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x2A
    OpcodeInfo {
        mnemonic: "ANC",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x2B (unofficial)
    OpcodeInfo {
        mnemonic: "BIT",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x2C
    OpcodeInfo {
        mnemonic: "AND",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x2D
    OpcodeInfo {
        mnemonic: "ROL",
        addr_mode: AddressingMode::Absolute,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x2E
    OpcodeInfo {
        mnemonic: "RLA",
        addr_mode: AddressingMode::Absolute,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x2F (unofficial)
    // 0x30-0x3F
    OpcodeInfo {
        mnemonic: "BMI",
        addr_mode: AddressingMode::Relative,
        cycles: 2,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x30
    OpcodeInfo {
        mnemonic: "AND",
        addr_mode: AddressingMode::IndirectIndexedY,
        cycles: 5,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x31
    OpcodeInfo {
        mnemonic: "JAM",
        addr_mode: AddressingMode::Implied,
        cycles: 0,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x32 (unofficial)
    OpcodeInfo {
        mnemonic: "RLA",
        addr_mode: AddressingMode::IndirectIndexedY,
        cycles: 8,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x33 (unofficial)
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x34 (unofficial)
    OpcodeInfo {
        mnemonic: "AND",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x35
    OpcodeInfo {
        mnemonic: "ROL",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x36
    OpcodeInfo {
        mnemonic: "RLA",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x37 (unofficial)
    OpcodeInfo {
        mnemonic: "SEC",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x38
    OpcodeInfo {
        mnemonic: "AND",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x39
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x3A (unofficial)
    OpcodeInfo {
        mnemonic: "RLA",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x3B (unofficial)
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: true,
    }, // 0x3C (unofficial)
    OpcodeInfo {
        mnemonic: "AND",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x3D
    OpcodeInfo {
        mnemonic: "ROL",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x3E
    OpcodeInfo {
        mnemonic: "RLA",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x3F (unofficial)
    // 0x40-0x4F
    OpcodeInfo {
        mnemonic: "RTI",
        addr_mode: AddressingMode::Implied,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x40
    OpcodeInfo {
        mnemonic: "EOR",
        addr_mode: AddressingMode::IndexedIndirectX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x41
    OpcodeInfo {
        mnemonic: "JAM",
        addr_mode: AddressingMode::Implied,
        cycles: 0,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x42 (unofficial)
    OpcodeInfo {
        mnemonic: "SRE",
        addr_mode: AddressingMode::IndexedIndirectX,
        cycles: 8,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x43 (unofficial)
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x44 (unofficial)
    OpcodeInfo {
        mnemonic: "EOR",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x45
    OpcodeInfo {
        mnemonic: "LSR",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x46
    OpcodeInfo {
        mnemonic: "SRE",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x47 (unofficial)
    OpcodeInfo {
        mnemonic: "PHA",
        addr_mode: AddressingMode::Implied,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x48
    OpcodeInfo {
        mnemonic: "EOR",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x49
    OpcodeInfo {
        mnemonic: "LSR",
        addr_mode: AddressingMode::Accumulator,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x4A
    OpcodeInfo {
        mnemonic: "ALR",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x4B (unofficial)
    OpcodeInfo {
        mnemonic: "JMP",
        addr_mode: AddressingMode::Absolute,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x4C
    OpcodeInfo {
        mnemonic: "EOR",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x4D
    OpcodeInfo {
        mnemonic: "LSR",
        addr_mode: AddressingMode::Absolute,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x4E
    OpcodeInfo {
        mnemonic: "SRE",
        addr_mode: AddressingMode::Absolute,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x4F (unofficial)
    // 0x50-0x5F
    OpcodeInfo {
        mnemonic: "BVC",
        addr_mode: AddressingMode::Relative,
        cycles: 2,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x50
    OpcodeInfo {
        mnemonic: "EOR",
        addr_mode: AddressingMode::IndirectIndexedY,
        cycles: 5,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x51
    OpcodeInfo {
        mnemonic: "JAM",
        addr_mode: AddressingMode::Implied,
        cycles: 0,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x52 (unofficial)
    OpcodeInfo {
        mnemonic: "SRE",
        addr_mode: AddressingMode::IndirectIndexedY,
        cycles: 8,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x53 (unofficial)
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x54 (unofficial)
    OpcodeInfo {
        mnemonic: "EOR",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x55
    OpcodeInfo {
        mnemonic: "LSR",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x56
    OpcodeInfo {
        mnemonic: "SRE",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x57 (unofficial)
    OpcodeInfo {
        mnemonic: "CLI",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x58
    OpcodeInfo {
        mnemonic: "EOR",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x59
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x5A (unofficial)
    OpcodeInfo {
        mnemonic: "SRE",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x5B (unofficial)
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: true,
    }, // 0x5C (unofficial)
    OpcodeInfo {
        mnemonic: "EOR",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x5D
    OpcodeInfo {
        mnemonic: "LSR",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x5E
    OpcodeInfo {
        mnemonic: "SRE",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x5F (unofficial)
    // 0x60-0x6F
    OpcodeInfo {
        mnemonic: "RTS",
        addr_mode: AddressingMode::Implied,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x60
    OpcodeInfo {
        mnemonic: "ADC",
        addr_mode: AddressingMode::IndexedIndirectX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x61
    OpcodeInfo {
        mnemonic: "JAM",
        addr_mode: AddressingMode::Implied,
        cycles: 0,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x62 (unofficial)
    OpcodeInfo {
        mnemonic: "RRA",
        addr_mode: AddressingMode::IndexedIndirectX,
        cycles: 8,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x63 (unofficial)
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x64 (unofficial)
    OpcodeInfo {
        mnemonic: "ADC",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x65
    OpcodeInfo {
        mnemonic: "ROR",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x66
    OpcodeInfo {
        mnemonic: "RRA",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x67 (unofficial)
    OpcodeInfo {
        mnemonic: "PLA",
        addr_mode: AddressingMode::Implied,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x68
    OpcodeInfo {
        mnemonic: "ADC",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x69
    OpcodeInfo {
        mnemonic: "ROR",
        addr_mode: AddressingMode::Accumulator,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x6A
    OpcodeInfo {
        mnemonic: "ARR",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x6B (unofficial)
    OpcodeInfo {
        mnemonic: "JMP",
        addr_mode: AddressingMode::Indirect,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x6C
    OpcodeInfo {
        mnemonic: "ADC",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x6D
    OpcodeInfo {
        mnemonic: "ROR",
        addr_mode: AddressingMode::Absolute,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x6E
    OpcodeInfo {
        mnemonic: "RRA",
        addr_mode: AddressingMode::Absolute,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x6F (unofficial)
    // 0x70-0x7F
    OpcodeInfo {
        mnemonic: "BVS",
        addr_mode: AddressingMode::Relative,
        cycles: 2,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x70
    OpcodeInfo {
        mnemonic: "ADC",
        addr_mode: AddressingMode::IndirectIndexedY,
        cycles: 5,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x71
    OpcodeInfo {
        mnemonic: "JAM",
        addr_mode: AddressingMode::Implied,
        cycles: 0,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x72 (unofficial)
    OpcodeInfo {
        mnemonic: "RRA",
        addr_mode: AddressingMode::IndirectIndexedY,
        cycles: 8,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x73 (unofficial)
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x74 (unofficial)
    OpcodeInfo {
        mnemonic: "ADC",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x75
    OpcodeInfo {
        mnemonic: "ROR",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x76
    OpcodeInfo {
        mnemonic: "RRA",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x77 (unofficial)
    OpcodeInfo {
        mnemonic: "SEI",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x78
    OpcodeInfo {
        mnemonic: "ADC",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x79
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x7A (unofficial)
    OpcodeInfo {
        mnemonic: "RRA",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x7B (unofficial)
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: true,
    }, // 0x7C (unofficial)
    OpcodeInfo {
        mnemonic: "ADC",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x7D
    OpcodeInfo {
        mnemonic: "ROR",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x7E
    OpcodeInfo {
        mnemonic: "RRA",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x7F (unofficial)
    // 0x80-0x8F
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x80 (unofficial)
    OpcodeInfo {
        mnemonic: "STA",
        addr_mode: AddressingMode::IndexedIndirectX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x81
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x82 (unofficial)
    OpcodeInfo {
        mnemonic: "SAX",
        addr_mode: AddressingMode::IndexedIndirectX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x83 (unofficial)
    OpcodeInfo {
        mnemonic: "STY",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x84
    OpcodeInfo {
        mnemonic: "STA",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x85
    OpcodeInfo {
        mnemonic: "STX",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x86
    OpcodeInfo {
        mnemonic: "SAX",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x87 (unofficial)
    OpcodeInfo {
        mnemonic: "DEY",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x88
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x89 (unofficial)
    OpcodeInfo {
        mnemonic: "TXA",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x8A
    OpcodeInfo {
        mnemonic: "XAA",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x8B (unofficial, unstable)
    OpcodeInfo {
        mnemonic: "STY",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x8C
    OpcodeInfo {
        mnemonic: "STA",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x8D
    OpcodeInfo {
        mnemonic: "STX",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x8E
    OpcodeInfo {
        mnemonic: "SAX",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x8F (unofficial)
    // 0x90-0x9F
    OpcodeInfo {
        mnemonic: "BCC",
        addr_mode: AddressingMode::Relative,
        cycles: 2,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0x90
    OpcodeInfo {
        mnemonic: "STA",
        addr_mode: AddressingMode::IndirectIndexedY,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x91
    OpcodeInfo {
        mnemonic: "JAM",
        addr_mode: AddressingMode::Implied,
        cycles: 0,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x92 (unofficial)
    OpcodeInfo {
        mnemonic: "SHA",
        addr_mode: AddressingMode::IndirectIndexedY,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x93 (unofficial, unstable)
    OpcodeInfo {
        mnemonic: "STY",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x94
    OpcodeInfo {
        mnemonic: "STA",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x95
    OpcodeInfo {
        mnemonic: "STX",
        addr_mode: AddressingMode::ZeroPageY,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x96
    OpcodeInfo {
        mnemonic: "SAX",
        addr_mode: AddressingMode::ZeroPageY,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x97 (unofficial)
    OpcodeInfo {
        mnemonic: "TYA",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x98
    OpcodeInfo {
        mnemonic: "STA",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x99
    OpcodeInfo {
        mnemonic: "TXS",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x9A
    OpcodeInfo {
        mnemonic: "TAS",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x9B (unofficial, unstable)
    OpcodeInfo {
        mnemonic: "SHY",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x9C (unofficial, unstable)
    OpcodeInfo {
        mnemonic: "STA",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0x9D
    OpcodeInfo {
        mnemonic: "SHX",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x9E (unofficial, unstable)
    OpcodeInfo {
        mnemonic: "SHA",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0x9F (unofficial, unstable)
    // 0xA0-0xAF
    OpcodeInfo {
        mnemonic: "LDY",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xA0
    OpcodeInfo {
        mnemonic: "LDA",
        addr_mode: AddressingMode::IndexedIndirectX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xA1
    OpcodeInfo {
        mnemonic: "LDX",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xA2
    OpcodeInfo {
        mnemonic: "LAX",
        addr_mode: AddressingMode::IndexedIndirectX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xA3 (unofficial)
    OpcodeInfo {
        mnemonic: "LDY",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xA4
    OpcodeInfo {
        mnemonic: "LDA",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xA5
    OpcodeInfo {
        mnemonic: "LDX",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xA6
    OpcodeInfo {
        mnemonic: "LAX",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xA7 (unofficial)
    OpcodeInfo {
        mnemonic: "TAY",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xA8
    OpcodeInfo {
        mnemonic: "LDA",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xA9
    OpcodeInfo {
        mnemonic: "TAX",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xAA
    OpcodeInfo {
        mnemonic: "LXA",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xAB (unofficial, unstable)
    OpcodeInfo {
        mnemonic: "LDY",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xAC
    OpcodeInfo {
        mnemonic: "LDA",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xAD
    OpcodeInfo {
        mnemonic: "LDX",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xAE
    OpcodeInfo {
        mnemonic: "LAX",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xAF (unofficial)
    // 0xB0-0xBF
    OpcodeInfo {
        mnemonic: "BCS",
        addr_mode: AddressingMode::Relative,
        cycles: 2,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0xB0
    OpcodeInfo {
        mnemonic: "LDA",
        addr_mode: AddressingMode::IndirectIndexedY,
        cycles: 5,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0xB1
    OpcodeInfo {
        mnemonic: "JAM",
        addr_mode: AddressingMode::Implied,
        cycles: 0,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xB2 (unofficial)
    OpcodeInfo {
        mnemonic: "LAX",
        addr_mode: AddressingMode::IndirectIndexedY,
        cycles: 5,
        page_cross_penalty: true,

        unofficial: true,
    }, // 0xB3 (unofficial)
    OpcodeInfo {
        mnemonic: "LDY",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xB4
    OpcodeInfo {
        mnemonic: "LDA",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xB5
    OpcodeInfo {
        mnemonic: "LDX",
        addr_mode: AddressingMode::ZeroPageY,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xB6
    OpcodeInfo {
        mnemonic: "LAX",
        addr_mode: AddressingMode::ZeroPageY,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xB7 (unofficial)
    OpcodeInfo {
        mnemonic: "CLV",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xB8
    OpcodeInfo {
        mnemonic: "LDA",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0xB9
    OpcodeInfo {
        mnemonic: "TSX",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xBA
    OpcodeInfo {
        mnemonic: "LAS",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: true,
    }, // 0xBB (unofficial, unstable)
    OpcodeInfo {
        mnemonic: "LDY",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0xBC
    OpcodeInfo {
        mnemonic: "LDA",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0xBD
    OpcodeInfo {
        mnemonic: "LDX",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0xBE
    OpcodeInfo {
        mnemonic: "LAX",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: true,
    }, // 0xBF (unofficial)
    // 0xC0-0xCF
    OpcodeInfo {
        mnemonic: "CPY",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xC0
    OpcodeInfo {
        mnemonic: "CMP",
        addr_mode: AddressingMode::IndexedIndirectX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xC1
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xC2 (unofficial)
    OpcodeInfo {
        mnemonic: "DCP",
        addr_mode: AddressingMode::IndexedIndirectX,
        cycles: 8,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xC3 (unofficial)
    OpcodeInfo {
        mnemonic: "CPY",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xC4
    OpcodeInfo {
        mnemonic: "CMP",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xC5
    OpcodeInfo {
        mnemonic: "DEC",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xC6
    OpcodeInfo {
        mnemonic: "DCP",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xC7 (unofficial)
    OpcodeInfo {
        mnemonic: "INY",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xC8
    OpcodeInfo {
        mnemonic: "CMP",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xC9
    OpcodeInfo {
        mnemonic: "DEX",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xCA
    OpcodeInfo {
        mnemonic: "AXS",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xCB (unofficial)
    OpcodeInfo {
        mnemonic: "CPY",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xCC
    OpcodeInfo {
        mnemonic: "CMP",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xCD
    OpcodeInfo {
        mnemonic: "DEC",
        addr_mode: AddressingMode::Absolute,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xCE
    OpcodeInfo {
        mnemonic: "DCP",
        addr_mode: AddressingMode::Absolute,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xCF (unofficial)
    // 0xD0-0xDF
    OpcodeInfo {
        mnemonic: "BNE",
        addr_mode: AddressingMode::Relative,
        cycles: 2,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0xD0
    OpcodeInfo {
        mnemonic: "CMP",
        addr_mode: AddressingMode::IndirectIndexedY,
        cycles: 5,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0xD1
    OpcodeInfo {
        mnemonic: "JAM",
        addr_mode: AddressingMode::Implied,
        cycles: 0,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xD2 (unofficial)
    OpcodeInfo {
        mnemonic: "DCP",
        addr_mode: AddressingMode::IndirectIndexedY,
        cycles: 8,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xD3 (unofficial)
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xD4 (unofficial)
    OpcodeInfo {
        mnemonic: "CMP",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xD5
    OpcodeInfo {
        mnemonic: "DEC",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xD6
    OpcodeInfo {
        mnemonic: "DCP",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xD7 (unofficial)
    OpcodeInfo {
        mnemonic: "CLD",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xD8
    OpcodeInfo {
        mnemonic: "CMP",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0xD9
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xDA (unofficial)
    OpcodeInfo {
        mnemonic: "DCP",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xDB (unofficial)
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: true,
    }, // 0xDC (unofficial)
    OpcodeInfo {
        mnemonic: "CMP",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0xDD
    OpcodeInfo {
        mnemonic: "DEC",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xDE
    OpcodeInfo {
        mnemonic: "DCP",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xDF (unofficial)
    // 0xE0-0xEF
    OpcodeInfo {
        mnemonic: "CPX",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xE0
    OpcodeInfo {
        mnemonic: "SBC",
        addr_mode: AddressingMode::IndexedIndirectX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xE1
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xE2 (unofficial)
    OpcodeInfo {
        mnemonic: "ISB",
        addr_mode: AddressingMode::IndexedIndirectX,
        cycles: 8,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xE3 (unofficial)
    OpcodeInfo {
        mnemonic: "CPX",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xE4
    OpcodeInfo {
        mnemonic: "SBC",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 3,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xE5
    OpcodeInfo {
        mnemonic: "INC",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xE6
    OpcodeInfo {
        mnemonic: "ISB",
        addr_mode: AddressingMode::ZeroPage,
        cycles: 5,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xE7 (unofficial)
    OpcodeInfo {
        mnemonic: "INX",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xE8
    OpcodeInfo {
        mnemonic: "SBC",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xE9
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xEA
    OpcodeInfo {
        mnemonic: "SBC",
        addr_mode: AddressingMode::Immediate,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xEB (unofficial, same as 0xE9)
    OpcodeInfo {
        mnemonic: "CPX",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xEC
    OpcodeInfo {
        mnemonic: "SBC",
        addr_mode: AddressingMode::Absolute,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xED
    OpcodeInfo {
        mnemonic: "INC",
        addr_mode: AddressingMode::Absolute,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xEE
    OpcodeInfo {
        mnemonic: "ISB",
        addr_mode: AddressingMode::Absolute,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xEF (unofficial)
    // 0xF0-0xFF
    OpcodeInfo {
        mnemonic: "BEQ",
        addr_mode: AddressingMode::Relative,
        cycles: 2,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0xF0
    OpcodeInfo {
        mnemonic: "SBC",
        addr_mode: AddressingMode::IndirectIndexedY,
        cycles: 5,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0xF1
    OpcodeInfo {
        mnemonic: "JAM",
        addr_mode: AddressingMode::Implied,
        cycles: 0,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xF2 (unofficial)
    OpcodeInfo {
        mnemonic: "ISB",
        addr_mode: AddressingMode::IndirectIndexedY,
        cycles: 8,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xF3 (unofficial)
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xF4 (unofficial)
    OpcodeInfo {
        mnemonic: "SBC",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 4,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xF5
    OpcodeInfo {
        mnemonic: "INC",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xF6
    OpcodeInfo {
        mnemonic: "ISB",
        addr_mode: AddressingMode::ZeroPageX,
        cycles: 6,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xF7 (unofficial)
    OpcodeInfo {
        mnemonic: "SED",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xF8
    OpcodeInfo {
        mnemonic: "SBC",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0xF9
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::Implied,
        cycles: 2,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xFA (unofficial)
    OpcodeInfo {
        mnemonic: "ISB",
        addr_mode: AddressingMode::AbsoluteY,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xFB (unofficial)
    OpcodeInfo {
        mnemonic: "NOP",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: true,
    }, // 0xFC (unofficial)
    OpcodeInfo {
        mnemonic: "SBC",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 4,
        page_cross_penalty: true,

        unofficial: false,
    }, // 0xFD
    OpcodeInfo {
        mnemonic: "INC",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: false,
    }, // 0xFE
    OpcodeInfo {
        mnemonic: "ISB",
        addr_mode: AddressingMode::AbsoluteX,
        cycles: 7,
        page_cross_penalty: false,

        unofficial: true,
    }, // 0xFF (unofficial)
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcode_table_size() {
        assert_eq!(OPCODE_TABLE.len(), 256);
    }

    #[test]
    fn test_known_opcodes() {
        // Official opcodes
        assert_eq!(OPCODE_TABLE[0x00].mnemonic, "BRK");
        assert_eq!(OPCODE_TABLE[0xA9].mnemonic, "LDA");
        assert_eq!(OPCODE_TABLE[0xEA].mnemonic, "NOP");

        // Unofficial opcodes
        assert_eq!(OPCODE_TABLE[0xA7].mnemonic, "LAX");
        assert_eq!(OPCODE_TABLE[0x87].mnemonic, "SAX");
        assert_eq!(OPCODE_TABLE[0xC7].mnemonic, "DCP");
    }

    #[test]
    fn test_cycle_counts() {
        assert_eq!(OPCODE_TABLE[0xA9].cycles, 2); // LDA immediate
        assert_eq!(OPCODE_TABLE[0xAD].cycles, 4); // LDA absolute
        assert_eq!(OPCODE_TABLE[0x00].cycles, 7); // BRK
    }

    #[test]
    fn test_page_cross_penalties() {
        assert!(OPCODE_TABLE[0xBD].page_cross_penalty); // LDA absolute,X
        assert!(!OPCODE_TABLE[0x8D].page_cross_penalty); // STA absolute
    }
}

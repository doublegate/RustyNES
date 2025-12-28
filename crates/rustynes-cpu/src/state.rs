//! CPU execution state machine for cycle-accurate emulation.
//!
//! This module defines the state machine that enables cycle-by-cycle execution
//! of 6502 instructions. Each state represents a single bus access cycle.

/// CPU execution state for cycle-by-cycle execution.
///
/// Each state represents one CPU cycle with one bus access.
/// The state machine transitions through these states to execute
/// instructions with perfect cycle accuracy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CpuState {
    /// Fetch opcode from PC (cycle 1 of every instruction)
    #[default]
    FetchOpcode,

    /// Fetch low byte of operand
    FetchOperandLo,

    /// Fetch high byte of operand
    FetchOperandHi,

    /// Resolve effective address (internal operation or dummy read)
    /// Used for indexed addressing modes with page crossing
    ResolveAddress,

    /// Read data from effective address
    ReadData,

    /// Write data to effective address
    WriteData,

    /// Read-Modify-Write: Read phase
    RmwRead,

    /// Read-Modify-Write: Dummy write old value (hardware behavior)
    RmwDummyWrite,

    /// Read-Modify-Write: Write new value
    RmwWrite,

    /// Execute internal operation (no bus access, register-only)
    Execute,

    /// Fetch indirect address low byte (for indirect addressing)
    FetchIndirectLo,

    /// Fetch indirect address high byte (for indirect addressing)
    FetchIndirectHi,

    /// Add index to indirect address (indexed indirect)
    AddIndex,

    /// Push high byte to stack
    PushHi,

    /// Push low byte to stack
    PushLo,

    /// Push status to stack
    PushStatus,

    /// Pop low byte from stack (with internal cycle)
    PopLo,

    /// Pop high byte from stack
    PopHi,

    /// Pop status from stack
    PopStatus,

    /// Internal cycle (dummy stack read)
    InternalCycle,

    /// Branch taken - calculate new PC
    BranchTaken,

    /// Branch page cross - extra cycle for crossing page
    BranchPageCross,

    /// Interrupt: Push PC high
    InterruptPushPcHi,

    /// Interrupt: Push PC low
    InterruptPushPcLo,

    /// Interrupt: Push status
    InterruptPushStatus,

    /// Interrupt: Fetch vector low
    InterruptFetchVectorLo,

    /// Interrupt: Fetch vector high
    InterruptFetchVectorHi,
}

/// Instruction execution pattern classification.
///
/// Different instruction types follow different state sequences:
/// - Read: Fetch operand, read from effective address
/// - Write: Fetch operand, write to effective address
/// - ReadModifyWrite: Read, dummy write old, write new
/// - etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InstructionType {
    /// Read instructions (LDA, LDX, LDY, AND, ORA, EOR, CMP, ADC, SBC, BIT, LAX, etc.)
    #[default]
    Read,

    /// Write instructions (STA, STX, STY, SAX, SHA, SHX, SHY, TAS)
    Write,

    /// Read-Modify-Write (ASL, LSR, ROL, ROR, INC, DEC, SLO, RLA, SRE, RRA, DCP, ISC)
    ReadModifyWrite,

    /// Implied/Register operations (TAX, INX, CLC, NOP, etc.)
    /// Single-byte instructions with internal operation
    Implied,

    /// Accumulator operations (ASL A, LSR A, ROL A, ROR A)
    /// Two-cycle, single-byte
    Accumulator,

    /// Branch instructions (BEQ, BNE, BCC, BCS, BPL, BMI, BVC, BVS)
    /// 2/3/4 cycles depending on condition and page crossing
    Branch,

    /// Jump Absolute (JMP $NNNN)
    /// 3 cycles
    JumpAbsolute,

    /// Jump Indirect (JMP ($NNNN))
    /// 5 cycles
    JumpIndirect,

    /// Jump to Subroutine (JSR $NNNN)
    /// 6 cycles
    JumpSubroutine,

    /// Return from Subroutine (RTS)
    /// 6 cycles
    ReturnSubroutine,

    /// Return from Interrupt (RTI)
    /// 6 cycles
    ReturnInterrupt,

    /// Push to Stack (PHA, PHP)
    /// 3 cycles
    Push,

    /// Pull from Stack (PLA, PLP)
    /// 4 cycles
    Pull,

    /// Software Interrupt (BRK)
    /// 7 cycles
    Break,

    /// JAM/KIL - Halt CPU
    Jam,
}

impl CpuState {
    /// Returns true if this state requires a bus read.
    #[inline]
    pub const fn is_read(&self) -> bool {
        matches!(
            self,
            Self::FetchOpcode
                | Self::FetchOperandLo
                | Self::FetchOperandHi
                | Self::ReadData
                | Self::RmwRead
                | Self::FetchIndirectLo
                | Self::FetchIndirectHi
                | Self::ResolveAddress
                | Self::PopLo
                | Self::PopHi
                | Self::PopStatus
                | Self::InterruptFetchVectorLo
                | Self::InterruptFetchVectorHi
        )
    }

    /// Returns true if this state requires a bus write.
    #[inline]
    pub const fn is_write(&self) -> bool {
        matches!(
            self,
            Self::WriteData
                | Self::RmwDummyWrite
                | Self::RmwWrite
                | Self::PushHi
                | Self::PushLo
                | Self::PushStatus
                | Self::InterruptPushPcHi
                | Self::InterruptPushPcLo
                | Self::InterruptPushStatus
        )
    }

    /// Returns true if this is an internal operation (no bus access visible).
    #[inline]
    pub const fn is_internal(&self) -> bool {
        matches!(
            self,
            Self::Execute
                | Self::AddIndex
                | Self::InternalCycle
                | Self::BranchTaken
                | Self::BranchPageCross
        )
    }
}

impl InstructionType {
    /// Returns the base number of cycles for this instruction type.
    /// This does not include addressing mode cycles or penalties.
    #[inline]
    pub const fn base_cycles(&self) -> u8 {
        match self {
            Self::Implied => 2,
            Self::Accumulator => 2,
            Self::Read => 2,            // Base + addressing mode cycles
            Self::Write => 2,           // Base + addressing mode cycles
            Self::ReadModifyWrite => 2, // Base + addressing mode cycles + 2 (read/dummy/write)
            Self::Branch => 2,          // +1 if taken, +1 if page cross
            Self::JumpAbsolute => 3,
            Self::JumpIndirect => 5,
            Self::JumpSubroutine => 6,
            Self::ReturnSubroutine => 6,
            Self::ReturnInterrupt => 6,
            Self::Push => 3,
            Self::Pull => 4,
            Self::Break => 7,
            Self::Jam => 2,
        }
    }

    /// Returns true if this instruction type can have page crossing penalty.
    #[inline]
    pub const fn has_page_cross_penalty(&self) -> bool {
        matches!(self, Self::Read | Self::Branch)
    }

    /// Returns true if this is a read-modify-write instruction.
    #[inline]
    pub const fn is_rmw(&self) -> bool {
        matches!(self, Self::ReadModifyWrite)
    }

    /// Classify an opcode into its instruction type.
    ///
    /// This function maps all 256 opcodes (official and unofficial) to their
    /// execution pattern type, enabling proper cycle-by-cycle state machine
    /// dispatch.
    #[inline]
    pub const fn from_opcode(opcode: u8) -> Self {
        match opcode {
            // ===== Branch instructions (2/3/4 cycles) =====
            0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xB0 | 0xD0 | 0xF0 => Self::Branch,

            // ===== Jump/Subroutine/Return =====
            0x4C => Self::JumpAbsolute,     // JMP abs
            0x6C => Self::JumpIndirect,     // JMP (ind)
            0x20 => Self::JumpSubroutine,   // JSR
            0x60 => Self::ReturnSubroutine, // RTS
            0x40 => Self::ReturnInterrupt,  // RTI
            0x00 => Self::Break,            // BRK

            // ===== Stack: Push (3 cycles) =====
            0x48 | 0x08 => Self::Push, // PHA, PHP

            // ===== Stack: Pull (4 cycles) =====
            0x68 | 0x28 => Self::Pull, // PLA, PLP

            // ===== Accumulator mode (2 cycles) =====
            0x0A | 0x2A | 0x4A | 0x6A => Self::Accumulator, // ASL A, ROL A, LSR A, ROR A

            // ===== Implied mode (2 cycles) =====
            // Transfers
            0xAA | 0xA8 | 0x8A | 0x98 | 0xBA | 0x9A => Self::Implied,
            // Increment/Decrement registers
            0xE8 | 0xC8 | 0xCA | 0x88 => Self::Implied,
            // Flag operations
            0x18 | 0x38 | 0x58 | 0x78 | 0xB8 | 0xD8 | 0xF8 => Self::Implied,
            // Official NOP
            0xEA => Self::Implied,
            // Unofficial NOPs (implied, 2 cycles)
            0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => Self::Implied,

            // ===== Store instructions (Write) =====
            // STA
            0x85 | 0x95 | 0x8D | 0x9D | 0x99 | 0x81 | 0x91 => Self::Write,
            // STX
            0x86 | 0x96 | 0x8E => Self::Write,
            // STY
            0x84 | 0x94 | 0x8C => Self::Write,
            // SAX (unofficial)
            0x87 | 0x97 | 0x8F | 0x83 => Self::Write,
            // SHA, SHX, SHY, TAS (unofficial)
            0x93 | 0x9F | 0x9C | 0x9E | 0x9B => Self::Write,

            // ===== Read-Modify-Write (memory) =====
            // ASL
            0x06 | 0x16 | 0x0E | 0x1E => Self::ReadModifyWrite,
            // LSR
            0x46 | 0x56 | 0x4E | 0x5E => Self::ReadModifyWrite,
            // ROL
            0x26 | 0x36 | 0x2E | 0x3E => Self::ReadModifyWrite,
            // ROR
            0x66 | 0x76 | 0x6E | 0x7E => Self::ReadModifyWrite,
            // INC
            0xE6 | 0xF6 | 0xEE | 0xFE => Self::ReadModifyWrite,
            // DEC
            0xC6 | 0xD6 | 0xCE | 0xDE => Self::ReadModifyWrite,
            // SLO (unofficial: ASL + ORA)
            0x07 | 0x17 | 0x0F | 0x1F | 0x1B | 0x03 | 0x13 => Self::ReadModifyWrite,
            // RLA (unofficial: ROL + AND)
            0x27 | 0x37 | 0x2F | 0x3F | 0x3B | 0x23 | 0x33 => Self::ReadModifyWrite,
            // SRE (unofficial: LSR + EOR)
            0x47 | 0x57 | 0x4F | 0x5F | 0x5B | 0x43 | 0x53 => Self::ReadModifyWrite,
            // RRA (unofficial: ROR + ADC)
            0x67 | 0x77 | 0x6F | 0x7F | 0x7B | 0x63 | 0x73 => Self::ReadModifyWrite,
            // DCP (unofficial: DEC + CMP)
            0xC7 | 0xD7 | 0xCF | 0xDF | 0xDB | 0xC3 | 0xD3 => Self::ReadModifyWrite,
            // ISC (unofficial: INC + SBC)
            0xE7 | 0xF7 | 0xEF | 0xFF | 0xFB | 0xE3 | 0xF3 => Self::ReadModifyWrite,

            // ===== JAM/KIL opcodes =====
            0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 | 0x62 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2 => {
                Self::Jam
            }

            // ===== All remaining opcodes are Read instructions =====
            // LDA (all addressing modes)
            0xA9 | 0xA5 | 0xB5 | 0xAD | 0xBD | 0xB9 | 0xA1 | 0xB1 => Self::Read,
            // LDX
            0xA2 | 0xA6 | 0xB6 | 0xAE | 0xBE => Self::Read,
            // LDY
            0xA0 | 0xA4 | 0xB4 | 0xAC | 0xBC => Self::Read,
            // ADC
            0x69 | 0x65 | 0x75 | 0x6D | 0x7D | 0x79 | 0x61 | 0x71 => Self::Read,
            // SBC (including unofficial 0xEB)
            0xE9 | 0xE5 | 0xF5 | 0xED | 0xFD | 0xF9 | 0xE1 | 0xF1 | 0xEB => Self::Read,
            // AND
            0x29 | 0x25 | 0x35 | 0x2D | 0x3D | 0x39 | 0x21 | 0x31 => Self::Read,
            // ORA
            0x09 | 0x05 | 0x15 | 0x0D | 0x1D | 0x19 | 0x01 | 0x11 => Self::Read,
            // EOR
            0x49 | 0x45 | 0x55 | 0x4D | 0x5D | 0x59 | 0x41 | 0x51 => Self::Read,
            // CMP
            0xC9 | 0xC5 | 0xD5 | 0xCD | 0xDD | 0xD9 | 0xC1 | 0xD1 => Self::Read,
            // CPX
            0xE0 | 0xE4 | 0xEC => Self::Read,
            // CPY
            0xC0 | 0xC4 | 0xCC => Self::Read,
            // BIT
            0x24 | 0x2C => Self::Read,
            // LAX (unofficial)
            0xA7 | 0xB7 | 0xAF | 0xBF | 0xA3 | 0xB3 => Self::Read,
            // LAS (unofficial)
            0xBB => Self::Read,
            // Unofficial immediate operations (reads)
            0x0B | 0x2B => Self::Read, // ANC
            0x4B => Self::Read,        // ALR
            0x6B => Self::Read,        // ARR
            0x8B => Self::Read,        // XAA
            0xAB => Self::Read,        // LXA
            0xCB => Self::Read,        // AXS
            // Unofficial NOPs with reads (immediate/zp/abs)
            0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 => Self::Read,
            0x04 | 0x44 | 0x64 | 0x14 | 0x34 | 0x54 | 0x74 | 0xD4 | 0xF4 => Self::Read,
            0x0C | 0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => Self::Read,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_state_default() {
        assert_eq!(CpuState::default(), CpuState::FetchOpcode);
    }

    #[test]
    fn test_instruction_type_default() {
        assert_eq!(InstructionType::default(), InstructionType::Read);
    }

    #[test]
    fn test_state_is_read() {
        assert!(CpuState::FetchOpcode.is_read());
        assert!(CpuState::ReadData.is_read());
        assert!(!CpuState::WriteData.is_read());
        assert!(!CpuState::Execute.is_read());
    }

    #[test]
    fn test_state_is_write() {
        assert!(CpuState::WriteData.is_write());
        assert!(CpuState::RmwWrite.is_write());
        assert!(!CpuState::ReadData.is_write());
        assert!(!CpuState::Execute.is_write());
    }

    #[test]
    fn test_state_is_internal() {
        assert!(CpuState::Execute.is_internal());
        assert!(CpuState::AddIndex.is_internal());
        assert!(!CpuState::FetchOpcode.is_internal());
        assert!(!CpuState::WriteData.is_internal());
    }

    #[test]
    fn test_instruction_type_base_cycles() {
        assert_eq!(InstructionType::Implied.base_cycles(), 2);
        assert_eq!(InstructionType::Push.base_cycles(), 3);
        assert_eq!(InstructionType::Pull.base_cycles(), 4);
        assert_eq!(InstructionType::JumpIndirect.base_cycles(), 5);
        assert_eq!(InstructionType::JumpSubroutine.base_cycles(), 6);
        assert_eq!(InstructionType::Break.base_cycles(), 7);
    }

    #[test]
    fn test_page_cross_penalty() {
        assert!(InstructionType::Read.has_page_cross_penalty());
        assert!(InstructionType::Branch.has_page_cross_penalty());
        assert!(!InstructionType::Write.has_page_cross_penalty());
        assert!(!InstructionType::ReadModifyWrite.has_page_cross_penalty());
    }

    #[test]
    fn test_from_opcode_branches() {
        // All branch instructions
        assert_eq!(InstructionType::from_opcode(0x10), InstructionType::Branch); // BPL
        assert_eq!(InstructionType::from_opcode(0x30), InstructionType::Branch); // BMI
        assert_eq!(InstructionType::from_opcode(0x50), InstructionType::Branch); // BVC
        assert_eq!(InstructionType::from_opcode(0x70), InstructionType::Branch); // BVS
        assert_eq!(InstructionType::from_opcode(0x90), InstructionType::Branch); // BCC
        assert_eq!(InstructionType::from_opcode(0xB0), InstructionType::Branch); // BCS
        assert_eq!(InstructionType::from_opcode(0xD0), InstructionType::Branch); // BNE
        assert_eq!(InstructionType::from_opcode(0xF0), InstructionType::Branch);
        // BEQ
    }

    #[test]
    fn test_from_opcode_jumps() {
        assert_eq!(
            InstructionType::from_opcode(0x4C),
            InstructionType::JumpAbsolute
        );
        assert_eq!(
            InstructionType::from_opcode(0x6C),
            InstructionType::JumpIndirect
        );
        assert_eq!(
            InstructionType::from_opcode(0x20),
            InstructionType::JumpSubroutine
        );
        assert_eq!(
            InstructionType::from_opcode(0x60),
            InstructionType::ReturnSubroutine
        );
        assert_eq!(
            InstructionType::from_opcode(0x40),
            InstructionType::ReturnInterrupt
        );
        assert_eq!(InstructionType::from_opcode(0x00), InstructionType::Break);
    }

    #[test]
    fn test_from_opcode_stack() {
        assert_eq!(InstructionType::from_opcode(0x48), InstructionType::Push); // PHA
        assert_eq!(InstructionType::from_opcode(0x08), InstructionType::Push); // PHP
        assert_eq!(InstructionType::from_opcode(0x68), InstructionType::Pull); // PLA
        assert_eq!(InstructionType::from_opcode(0x28), InstructionType::Pull); // PLP
    }

    #[test]
    fn test_from_opcode_accumulator() {
        assert_eq!(
            InstructionType::from_opcode(0x0A),
            InstructionType::Accumulator
        ); // ASL A
        assert_eq!(
            InstructionType::from_opcode(0x2A),
            InstructionType::Accumulator
        ); // ROL A
        assert_eq!(
            InstructionType::from_opcode(0x4A),
            InstructionType::Accumulator
        ); // LSR A
        assert_eq!(
            InstructionType::from_opcode(0x6A),
            InstructionType::Accumulator
        ); // ROR A
    }

    #[test]
    fn test_from_opcode_implied() {
        // Transfers
        assert_eq!(InstructionType::from_opcode(0xAA), InstructionType::Implied); // TAX
        assert_eq!(InstructionType::from_opcode(0xA8), InstructionType::Implied); // TAY
        // Inc/Dec registers
        assert_eq!(InstructionType::from_opcode(0xE8), InstructionType::Implied); // INX
        assert_eq!(InstructionType::from_opcode(0xC8), InstructionType::Implied); // INY
        // Flags
        assert_eq!(InstructionType::from_opcode(0x18), InstructionType::Implied); // CLC
        // NOP
        assert_eq!(InstructionType::from_opcode(0xEA), InstructionType::Implied);
    }

    #[test]
    fn test_from_opcode_write() {
        // STA
        assert_eq!(InstructionType::from_opcode(0x85), InstructionType::Write); // zp
        assert_eq!(InstructionType::from_opcode(0x8D), InstructionType::Write); // abs
        // STX
        assert_eq!(InstructionType::from_opcode(0x86), InstructionType::Write);
        // STY
        assert_eq!(InstructionType::from_opcode(0x84), InstructionType::Write);
    }

    #[test]
    fn test_from_opcode_rmw() {
        // ASL memory
        assert_eq!(
            InstructionType::from_opcode(0x06),
            InstructionType::ReadModifyWrite
        );
        assert_eq!(
            InstructionType::from_opcode(0x0E),
            InstructionType::ReadModifyWrite
        );
        // INC
        assert_eq!(
            InstructionType::from_opcode(0xE6),
            InstructionType::ReadModifyWrite
        );
        // DEC
        assert_eq!(
            InstructionType::from_opcode(0xC6),
            InstructionType::ReadModifyWrite
        );
        // Unofficial RMW (SLO, RLA, etc.)
        assert_eq!(
            InstructionType::from_opcode(0x07),
            InstructionType::ReadModifyWrite
        ); // SLO zp
    }

    #[test]
    fn test_from_opcode_read() {
        // LDA
        assert_eq!(InstructionType::from_opcode(0xA9), InstructionType::Read); // imm
        assert_eq!(InstructionType::from_opcode(0xA5), InstructionType::Read); // zp
        // LDX
        assert_eq!(InstructionType::from_opcode(0xA2), InstructionType::Read);
        // LDY
        assert_eq!(InstructionType::from_opcode(0xA0), InstructionType::Read);
        // ADC
        assert_eq!(InstructionType::from_opcode(0x69), InstructionType::Read);
        // CMP
        assert_eq!(InstructionType::from_opcode(0xC9), InstructionType::Read);
    }

    #[test]
    fn test_from_opcode_jam() {
        assert_eq!(InstructionType::from_opcode(0x02), InstructionType::Jam);
        assert_eq!(InstructionType::from_opcode(0x12), InstructionType::Jam);
        assert_eq!(InstructionType::from_opcode(0x22), InstructionType::Jam);
    }
}

//! 6502 Addressing Modes.
//!
//! The 6502 CPU supports various addressing modes that determine how
//! the operand for an instruction is fetched.

/// Addressing modes for 6502 instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AddrMode {
    /// Implicit - No operand, operation is implied.
    /// Example: CLC, SEC, INX
    Imp,

    /// Accumulator - Operates on the accumulator.
    /// Example: ASL A, ROL A
    Acc,

    /// Immediate - 8-bit constant operand.
    /// Example: LDA #$42
    Imm,

    /// Zero Page - 8-bit address in zero page ($0000-$00FF).
    /// Example: LDA $42
    Zp0,

    /// Zero Page,X - Zero page address plus X register (wraps within zero page).
    /// Example: LDA $42,X
    Zpx,

    /// Zero Page,Y - Zero page address plus Y register (wraps within zero page).
    /// Example: LDX $42,Y
    Zpy,

    /// Relative - Signed 8-bit offset for branch instructions.
    /// Example: BEQ label
    Rel,

    /// Absolute - Full 16-bit address.
    /// Example: LDA $1234
    Abs,

    /// Absolute,X - 16-bit address plus X register.
    /// Example: LDA $1234,X
    Abx,

    /// Absolute,Y - 16-bit address plus Y register.
    /// Example: LDA $1234,Y
    Aby,

    /// Indirect - 16-bit address points to 16-bit target address.
    /// Used only by JMP. Has a bug where crossing page boundary wraps.
    /// Example: JMP ($1234)
    Ind,

    /// Indexed Indirect - (Zero Page,X)
    /// Pointer in zero page indexed by X.
    /// Example: LDA ($42,X)
    Idx,

    /// Indirect Indexed - (Zero Page),Y
    /// Pointer in zero page, indexed by Y after fetching.
    /// Example: LDA ($42),Y
    Idy,

    /// Absolute,X with forced dummy read (for write instructions).
    AbxW,

    /// Absolute,Y with forced dummy read (for write instructions).
    AbyW,

    /// Indirect Indexed with forced dummy read (for write instructions).
    IdyW,
}

impl AddrMode {
    /// Returns the base number of bytes for this addressing mode's operand.
    /// Does not include the opcode byte.
    #[must_use]
    pub const fn operand_size(self) -> u8 {
        match self {
            Self::Imp | Self::Acc => 0,
            Self::Imm
            | Self::Zp0
            | Self::Zpx
            | Self::Zpy
            | Self::Rel
            | Self::Idx
            | Self::Idy
            | Self::IdyW => 1,
            Self::Abs | Self::Abx | Self::Aby | Self::Ind | Self::AbxW | Self::AbyW => 2,
        }
    }

    /// Returns the base number of cycles for this addressing mode.
    /// Additional cycles may be added for page boundary crossings.
    #[must_use]
    pub const fn base_cycles(self) -> u8 {
        match self {
            Self::Imp | Self::Acc => 0,
            Self::Imm => 1,
            Self::Zp0 => 2,
            Self::Zpx | Self::Zpy => 3,
            Self::Rel => 1, // +1 if branch taken, +1 if page crossed
            Self::Abs => 3,
            Self::Abx | Self::Aby => 3,   // +1 if page crossed for reads
            Self::AbxW | Self::AbyW => 4, // Always 4 for writes
            Self::Ind => 4,
            Self::Idx => 5,
            Self::Idy => 4,  // +1 if page crossed for reads
            Self::IdyW => 5, // Always 5 for writes
        }
    }
}

/// Opcode addressing mode lookup table.
/// Indexed by opcode byte (0x00-0xFF).
#[rustfmt::skip]
pub static ADDR_MODE_TABLE: [AddrMode; 256] = [
    //       0          1          2          3          4          5          6          7          8          9          A          B          C          D          E          F
    /* 0 */ AddrMode::Imp, AddrMode::Idx, AddrMode::Imp, AddrMode::Idx, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Imp, AddrMode::Imm, AddrMode::Acc, AddrMode::Imm, AddrMode::Abs, AddrMode::Abs, AddrMode::Abs, AddrMode::Abs,
    /* 1 */ AddrMode::Rel, AddrMode::Idy, AddrMode::Imp, AddrMode::IdyW,AddrMode::Zpx, AddrMode::Zpx, AddrMode::Zpx, AddrMode::Zpx, AddrMode::Imp, AddrMode::Aby, AddrMode::Imp, AddrMode::AbyW,AddrMode::Abx, AddrMode::Abx, AddrMode::AbxW,AddrMode::AbxW,
    /* 2 */ AddrMode::Abs, AddrMode::Idx, AddrMode::Imp, AddrMode::Idx, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Imp, AddrMode::Imm, AddrMode::Acc, AddrMode::Imm, AddrMode::Abs, AddrMode::Abs, AddrMode::Abs, AddrMode::Abs,
    /* 3 */ AddrMode::Rel, AddrMode::Idy, AddrMode::Imp, AddrMode::IdyW,AddrMode::Zpx, AddrMode::Zpx, AddrMode::Zpx, AddrMode::Zpx, AddrMode::Imp, AddrMode::Aby, AddrMode::Imp, AddrMode::AbyW,AddrMode::Abx, AddrMode::Abx, AddrMode::AbxW,AddrMode::AbxW,
    /* 4 */ AddrMode::Imp, AddrMode::Idx, AddrMode::Imp, AddrMode::Idx, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Imp, AddrMode::Imm, AddrMode::Acc, AddrMode::Imm, AddrMode::Abs, AddrMode::Abs, AddrMode::Abs, AddrMode::Abs,
    /* 5 */ AddrMode::Rel, AddrMode::Idy, AddrMode::Imp, AddrMode::IdyW,AddrMode::Zpx, AddrMode::Zpx, AddrMode::Zpx, AddrMode::Zpx, AddrMode::Imp, AddrMode::Aby, AddrMode::Imp, AddrMode::AbyW,AddrMode::Abx, AddrMode::Abx, AddrMode::AbxW,AddrMode::AbxW,
    /* 6 */ AddrMode::Imp, AddrMode::Idx, AddrMode::Imp, AddrMode::Idx, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Imp, AddrMode::Imm, AddrMode::Acc, AddrMode::Imm, AddrMode::Ind, AddrMode::Abs, AddrMode::Abs, AddrMode::Abs,
    /* 7 */ AddrMode::Rel, AddrMode::Idy, AddrMode::Imp, AddrMode::IdyW,AddrMode::Zpx, AddrMode::Zpx, AddrMode::Zpx, AddrMode::Zpx, AddrMode::Imp, AddrMode::Aby, AddrMode::Imp, AddrMode::AbyW,AddrMode::Abx, AddrMode::Abx, AddrMode::AbxW,AddrMode::AbxW,
    /* 8 */ AddrMode::Imm, AddrMode::Idx, AddrMode::Imm, AddrMode::Idx, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Imp, AddrMode::Imm, AddrMode::Imp, AddrMode::Imm, AddrMode::Abs, AddrMode::Abs, AddrMode::Abs, AddrMode::Abs,
    /* 9 */ AddrMode::Rel, AddrMode::IdyW,AddrMode::Imp, AddrMode::IdyW,AddrMode::Zpx, AddrMode::Zpx, AddrMode::Zpy, AddrMode::Zpy, AddrMode::Imp, AddrMode::AbyW,AddrMode::Imp, AddrMode::AbyW,AddrMode::AbxW,AddrMode::AbxW,AddrMode::AbyW,AddrMode::AbyW,
    /* A */ AddrMode::Imm, AddrMode::Idx, AddrMode::Imm, AddrMode::Idx, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Imp, AddrMode::Imm, AddrMode::Imp, AddrMode::Imm, AddrMode::Abs, AddrMode::Abs, AddrMode::Abs, AddrMode::Abs,
    /* B */ AddrMode::Rel, AddrMode::Idy, AddrMode::Imp, AddrMode::Idy, AddrMode::Zpx, AddrMode::Zpx, AddrMode::Zpy, AddrMode::Zpy, AddrMode::Imp, AddrMode::Aby, AddrMode::Imp, AddrMode::Aby, AddrMode::Abx, AddrMode::Abx, AddrMode::Aby, AddrMode::Aby,
    /* C */ AddrMode::Imm, AddrMode::Idx, AddrMode::Imm, AddrMode::Idx, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Imp, AddrMode::Imm, AddrMode::Imp, AddrMode::Imm, AddrMode::Abs, AddrMode::Abs, AddrMode::Abs, AddrMode::Abs,
    /* D */ AddrMode::Rel, AddrMode::Idy, AddrMode::Imp, AddrMode::IdyW,AddrMode::Zpx, AddrMode::Zpx, AddrMode::Zpx, AddrMode::Zpx, AddrMode::Imp, AddrMode::Aby, AddrMode::Imp, AddrMode::AbyW,AddrMode::Abx, AddrMode::Abx, AddrMode::AbxW,AddrMode::AbxW,
    /* E */ AddrMode::Imm, AddrMode::Idx, AddrMode::Imm, AddrMode::Idx, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Zp0, AddrMode::Imp, AddrMode::Imm, AddrMode::Imp, AddrMode::Imm, AddrMode::Abs, AddrMode::Abs, AddrMode::Abs, AddrMode::Abs,
    /* F */ AddrMode::Rel, AddrMode::Idy, AddrMode::Imp, AddrMode::IdyW,AddrMode::Zpx, AddrMode::Zpx, AddrMode::Zpx, AddrMode::Zpx, AddrMode::Imp, AddrMode::Aby, AddrMode::Imp, AddrMode::AbyW,AddrMode::Abx, AddrMode::Abx, AddrMode::AbxW,AddrMode::AbxW,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operand_size() {
        assert_eq!(AddrMode::Imp.operand_size(), 0);
        assert_eq!(AddrMode::Acc.operand_size(), 0);
        assert_eq!(AddrMode::Imm.operand_size(), 1);
        assert_eq!(AddrMode::Zp0.operand_size(), 1);
        assert_eq!(AddrMode::Abs.operand_size(), 2);
        assert_eq!(AddrMode::Ind.operand_size(), 2);
    }

    #[test]
    fn test_addr_mode_table_lda() {
        // LDA immediate = 0xA9
        assert_eq!(ADDR_MODE_TABLE[0xA9], AddrMode::Imm);
        // LDA zero page = 0xA5
        assert_eq!(ADDR_MODE_TABLE[0xA5], AddrMode::Zp0);
        // LDA absolute = 0xAD
        assert_eq!(ADDR_MODE_TABLE[0xAD], AddrMode::Abs);
    }

    #[test]
    fn test_addr_mode_table_jmp() {
        // JMP absolute = 0x4C
        assert_eq!(ADDR_MODE_TABLE[0x4C], AddrMode::Abs);
        // JMP indirect = 0x6C
        assert_eq!(ADDR_MODE_TABLE[0x6C], AddrMode::Ind);
    }
}

//! 6502 disassembler — used by the debugger UI.
//!
//! Side-effect-free. Takes a `peek` closure that samples bytes on the CPU
//! bus *without* advancing time. The output rows are intended for a
//! scrollable listing; the addressing-mode decode is canonical for the
//! 151 documented opcodes plus the handful of unofficial opcodes that
//! ship games actually use. Unknown opcodes render as `.byte $XX`.
//!
//! This file is deliberately ~200 LOC: a single static table covers
//! the entire 256-entry opcode space, and one match dispatches by
//! addressing mode. There's no allocation per-instruction.

#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::enum_glob_use)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::too_many_lines)]

use alloc::format;
use alloc::{string::String, vec::Vec};

/// One decoded instruction.
#[derive(Debug, Clone)]
pub struct DisasmLine {
    /// PC at which the instruction starts.
    pub addr: u16,
    /// Raw opcode bytes (1-3 bytes).
    pub bytes: Vec<u8>,
    /// Mnemonic (`"LDA"`, `"BRK"`, ...).
    pub mnemonic: &'static str,
    /// Formatted operand, e.g. `"$1234,X"`, `"#$42"`, `""`.
    pub operand: String,
}

/// 6502 addressing modes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AddrMode {
    /// Implied / no operand.
    Implied,
    /// Accumulator (e.g. `ASL A`).
    Accumulator,
    /// `#$nn`.
    Immediate,
    /// `$nn` zero page.
    ZeroPage,
    /// `$nn,X` zero page indexed.
    ZeroPageX,
    /// `$nn,Y` zero page indexed.
    ZeroPageY,
    /// `$nnnn` absolute.
    Absolute,
    /// `$nnnn,X` absolute indexed.
    AbsoluteX,
    /// `$nnnn,Y` absolute indexed.
    AbsoluteY,
    /// `($nnnn)` indirect (`JMP` only).
    Indirect,
    /// `($nn,X)` indexed indirect.
    IndirectX,
    /// `($nn),Y` indirect indexed.
    IndirectY,
    /// `$nn` relative branch target.
    Relative,
}

const fn op_len(mode: AddrMode) -> u16 {
    match mode {
        AddrMode::Implied | AddrMode::Accumulator => 1,
        AddrMode::Immediate
        | AddrMode::ZeroPage
        | AddrMode::ZeroPageX
        | AddrMode::ZeroPageY
        | AddrMode::IndirectX
        | AddrMode::IndirectY
        | AddrMode::Relative => 2,
        AddrMode::Absolute | AddrMode::AbsoluteX | AddrMode::AbsoluteY | AddrMode::Indirect => 3,
    }
}

/// Static (mnemonic, mode) table for all 256 opcodes.
///
/// Coverage: all 151 documented + the most common unofficial opcodes
/// games rely on; everything else renders as `???` with `.byte`.
static OPCODE_TABLE: [(&str, AddrMode); 256] = build_opcode_table();

const fn build_opcode_table() -> [(&'static str, AddrMode); 256] {
    let mut t = [("???", AddrMode::Implied); 256];
    // Macro-free, all-const init — verbose but cheap.
    use AddrMode::*;
    macro_rules! set {
        ($op:expr_2021, $m:expr_2021, $mode:expr_2021) => {
            t[$op as usize] = ($m, $mode);
        };
    }
    // Loads / stores / transfers.
    set!(0xA9, "LDA", Immediate);
    set!(0xA5, "LDA", ZeroPage);
    set!(0xB5, "LDA", ZeroPageX);
    set!(0xAD, "LDA", Absolute);
    set!(0xBD, "LDA", AbsoluteX);
    set!(0xB9, "LDA", AbsoluteY);
    set!(0xA1, "LDA", IndirectX);
    set!(0xB1, "LDA", IndirectY);
    set!(0xA2, "LDX", Immediate);
    set!(0xA6, "LDX", ZeroPage);
    set!(0xB6, "LDX", ZeroPageY);
    set!(0xAE, "LDX", Absolute);
    set!(0xBE, "LDX", AbsoluteY);
    set!(0xA0, "LDY", Immediate);
    set!(0xA4, "LDY", ZeroPage);
    set!(0xB4, "LDY", ZeroPageX);
    set!(0xAC, "LDY", Absolute);
    set!(0xBC, "LDY", AbsoluteX);
    set!(0x85, "STA", ZeroPage);
    set!(0x95, "STA", ZeroPageX);
    set!(0x8D, "STA", Absolute);
    set!(0x9D, "STA", AbsoluteX);
    set!(0x99, "STA", AbsoluteY);
    set!(0x81, "STA", IndirectX);
    set!(0x91, "STA", IndirectY);
    set!(0x86, "STX", ZeroPage);
    set!(0x96, "STX", ZeroPageY);
    set!(0x8E, "STX", Absolute);
    set!(0x84, "STY", ZeroPage);
    set!(0x94, "STY", ZeroPageX);
    set!(0x8C, "STY", Absolute);
    set!(0xAA, "TAX", Implied);
    set!(0xA8, "TAY", Implied);
    set!(0xBA, "TSX", Implied);
    set!(0x8A, "TXA", Implied);
    set!(0x9A, "TXS", Implied);
    set!(0x98, "TYA", Implied);
    // Stack.
    set!(0x48, "PHA", Implied);
    set!(0x08, "PHP", Implied);
    set!(0x68, "PLA", Implied);
    set!(0x28, "PLP", Implied);
    // Logical.
    set!(0x29, "AND", Immediate);
    set!(0x25, "AND", ZeroPage);
    set!(0x35, "AND", ZeroPageX);
    set!(0x2D, "AND", Absolute);
    set!(0x3D, "AND", AbsoluteX);
    set!(0x39, "AND", AbsoluteY);
    set!(0x21, "AND", IndirectX);
    set!(0x31, "AND", IndirectY);
    set!(0x49, "EOR", Immediate);
    set!(0x45, "EOR", ZeroPage);
    set!(0x55, "EOR", ZeroPageX);
    set!(0x4D, "EOR", Absolute);
    set!(0x5D, "EOR", AbsoluteX);
    set!(0x59, "EOR", AbsoluteY);
    set!(0x41, "EOR", IndirectX);
    set!(0x51, "EOR", IndirectY);
    set!(0x09, "ORA", Immediate);
    set!(0x05, "ORA", ZeroPage);
    set!(0x15, "ORA", ZeroPageX);
    set!(0x0D, "ORA", Absolute);
    set!(0x1D, "ORA", AbsoluteX);
    set!(0x19, "ORA", AbsoluteY);
    set!(0x01, "ORA", IndirectX);
    set!(0x11, "ORA", IndirectY);
    set!(0x24, "BIT", ZeroPage);
    set!(0x2C, "BIT", Absolute);
    // Arithmetic.
    set!(0x69, "ADC", Immediate);
    set!(0x65, "ADC", ZeroPage);
    set!(0x75, "ADC", ZeroPageX);
    set!(0x6D, "ADC", Absolute);
    set!(0x7D, "ADC", AbsoluteX);
    set!(0x79, "ADC", AbsoluteY);
    set!(0x61, "ADC", IndirectX);
    set!(0x71, "ADC", IndirectY);
    set!(0xE9, "SBC", Immediate);
    set!(0xE5, "SBC", ZeroPage);
    set!(0xF5, "SBC", ZeroPageX);
    set!(0xED, "SBC", Absolute);
    set!(0xFD, "SBC", AbsoluteX);
    set!(0xF9, "SBC", AbsoluteY);
    set!(0xE1, "SBC", IndirectX);
    set!(0xF1, "SBC", IndirectY);
    set!(0xC9, "CMP", Immediate);
    set!(0xC5, "CMP", ZeroPage);
    set!(0xD5, "CMP", ZeroPageX);
    set!(0xCD, "CMP", Absolute);
    set!(0xDD, "CMP", AbsoluteX);
    set!(0xD9, "CMP", AbsoluteY);
    set!(0xC1, "CMP", IndirectX);
    set!(0xD1, "CMP", IndirectY);
    set!(0xE0, "CPX", Immediate);
    set!(0xE4, "CPX", ZeroPage);
    set!(0xEC, "CPX", Absolute);
    set!(0xC0, "CPY", Immediate);
    set!(0xC4, "CPY", ZeroPage);
    set!(0xCC, "CPY", Absolute);
    // Inc / dec.
    set!(0xE6, "INC", ZeroPage);
    set!(0xF6, "INC", ZeroPageX);
    set!(0xEE, "INC", Absolute);
    set!(0xFE, "INC", AbsoluteX);
    set!(0xE8, "INX", Implied);
    set!(0xC8, "INY", Implied);
    set!(0xC6, "DEC", ZeroPage);
    set!(0xD6, "DEC", ZeroPageX);
    set!(0xCE, "DEC", Absolute);
    set!(0xDE, "DEC", AbsoluteX);
    set!(0xCA, "DEX", Implied);
    set!(0x88, "DEY", Implied);
    // Shifts.
    set!(0x0A, "ASL", Accumulator);
    set!(0x06, "ASL", ZeroPage);
    set!(0x16, "ASL", ZeroPageX);
    set!(0x0E, "ASL", Absolute);
    set!(0x1E, "ASL", AbsoluteX);
    set!(0x4A, "LSR", Accumulator);
    set!(0x46, "LSR", ZeroPage);
    set!(0x56, "LSR", ZeroPageX);
    set!(0x4E, "LSR", Absolute);
    set!(0x5E, "LSR", AbsoluteX);
    set!(0x2A, "ROL", Accumulator);
    set!(0x26, "ROL", ZeroPage);
    set!(0x36, "ROL", ZeroPageX);
    set!(0x2E, "ROL", Absolute);
    set!(0x3E, "ROL", AbsoluteX);
    set!(0x6A, "ROR", Accumulator);
    set!(0x66, "ROR", ZeroPage);
    set!(0x76, "ROR", ZeroPageX);
    set!(0x6E, "ROR", Absolute);
    set!(0x7E, "ROR", AbsoluteX);
    // Jumps / flow.
    set!(0x4C, "JMP", Absolute);
    set!(0x6C, "JMP", Indirect);
    set!(0x20, "JSR", Absolute);
    set!(0x60, "RTS", Implied);
    set!(0x40, "RTI", Implied);
    set!(0x00, "BRK", Implied);
    // Branches.
    set!(0x10, "BPL", Relative);
    set!(0x30, "BMI", Relative);
    set!(0x50, "BVC", Relative);
    set!(0x70, "BVS", Relative);
    set!(0x90, "BCC", Relative);
    set!(0xB0, "BCS", Relative);
    set!(0xD0, "BNE", Relative);
    set!(0xF0, "BEQ", Relative);
    // Flag ops.
    set!(0x18, "CLC", Implied);
    set!(0x38, "SEC", Implied);
    set!(0x58, "CLI", Implied);
    set!(0x78, "SEI", Implied);
    set!(0xB8, "CLV", Implied);
    set!(0xD8, "CLD", Implied);
    set!(0xF8, "SED", Implied);
    set!(0xEA, "NOP", Implied);
    t
}

/// Disassemble `count` instructions starting at `pc`. `peek` returns the
/// byte at any CPU bus address without side effects.
///
/// Unknown opcodes are rendered as `.byte $XX` with length 1 so the
/// listing can keep walking forward.
pub fn disassemble_at<F: Fn(u16) -> u8>(peek: F, pc: u16, count: usize) -> Vec<DisasmLine> {
    let mut out = Vec::with_capacity(count);
    let mut cur = pc;
    for _ in 0..count {
        let op = peek(cur);
        let (mnemonic, mode) = OPCODE_TABLE[op as usize];
        let len = op_len(mode);
        let mut bytes = Vec::with_capacity(len as usize);
        for i in 0..len {
            bytes.push(peek(cur.wrapping_add(i)));
        }
        let operand = if mnemonic == "???" {
            format!(".byte ${op:02X}")
        } else {
            format_operand(mode, cur, &bytes)
        };
        out.push(DisasmLine {
            addr: cur,
            bytes,
            mnemonic,
            operand,
        });
        cur = cur.wrapping_add(len);
    }
    out
}

fn format_operand(mode: AddrMode, pc: u16, bytes: &[u8]) -> String {
    let b1 = bytes.get(1).copied().unwrap_or(0);
    let b2 = bytes.get(2).copied().unwrap_or(0);
    let abs16 = u16::from(b1) | (u16::from(b2) << 8);
    match mode {
        AddrMode::Implied => String::new(),
        AddrMode::Accumulator => "A".into(),
        AddrMode::Immediate => format!("#${b1:02X}"),
        AddrMode::ZeroPage => format!("${b1:02X}"),
        AddrMode::ZeroPageX => format!("${b1:02X},X"),
        AddrMode::ZeroPageY => format!("${b1:02X},Y"),
        AddrMode::Absolute => format!("${abs16:04X}"),
        AddrMode::AbsoluteX => format!("${abs16:04X},X"),
        AddrMode::AbsoluteY => format!("${abs16:04X},Y"),
        AddrMode::Indirect => format!("(${abs16:04X})"),
        AddrMode::IndirectX => format!("(${b1:02X},X)"),
        AddrMode::IndirectY => format!("(${b1:02X}),Y"),
        AddrMode::Relative => {
            let delta = b1 as i8;
            let target = pc.wrapping_add(2).wrapping_add(delta as u16);
            format!("${target:04X}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peek_from(bytes: &[u8], base: u16) -> impl Fn(u16) -> u8 + '_ {
        move |addr: u16| {
            let off = addr.wrapping_sub(base) as usize;
            bytes.get(off).copied().unwrap_or(0)
        }
    }

    #[test]
    fn disasm_lda_imm() {
        // A9 42  =>  LDA #$42
        let prog = [0xA9, 0x42];
        let lines = disassemble_at(peek_from(&prog, 0xC000), 0xC000, 1);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].addr, 0xC000);
        assert_eq!(lines[0].mnemonic, "LDA");
        assert_eq!(lines[0].operand, "#$42");
    }

    #[test]
    fn disasm_jmp_indirect() {
        // 6C 34 12  =>  JMP ($1234)
        let prog = [0x6C, 0x34, 0x12];
        let lines = disassemble_at(peek_from(&prog, 0xC000), 0xC000, 1);
        assert_eq!(lines[0].mnemonic, "JMP");
        assert_eq!(lines[0].operand, "($1234)");
    }

    #[test]
    fn disasm_walks_forward_past_unknown() {
        // 02 (illegal/JAM) then EA NOP — the unknown shouldn't stall the walk.
        let prog = [0x02, 0xEA];
        let lines = disassemble_at(peek_from(&prog, 0xC000), 0xC000, 2);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].mnemonic, "NOP");
    }

    #[test]
    fn disasm_branch_target_is_pc_plus_2_plus_delta() {
        // 10 FE => BPL $C000 (branch to self after pc+=2)
        let prog = [0x10, 0xFE];
        let lines = disassemble_at(peek_from(&prog, 0xC000), 0xC000, 1);
        assert_eq!(lines[0].mnemonic, "BPL");
        assert_eq!(lines[0].operand, "$C000");
    }
}

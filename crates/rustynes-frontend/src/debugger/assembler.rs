#![allow(clippy::cast_possible_truncation)]
//! v1.7.0 "Forge" Workstream A1/A3 — a tiny inline 6502 assembler for the CPU
//! debugger panel.
//!
//! Assembles one source line (e.g. `LDA #$42`, `STA $0200,X`, `JMP ($1234)`,
//! `BNE $C010`) into its opcode bytes. The opcode-encoding table is **derived
//! at runtime from the canonical disassembler** (`rustynes_core::rustynes_cpu::disassemble_at`)
//! rather than hand-maintained, so the assembler can never drift from the CPU
//! core's decode: for every opcode `0x00..=0xFF` we disassemble a synthetic
//! instruction, read back the mnemonic + addressing mode, and record the
//! `(mnemonic, mode) -> opcode` mapping. Only the documented (non-`???`)
//! opcodes are accepted, and ambiguous official mnemonics map to their lowest
//! opcode (standard assembler behavior).
//!
//! The assembled bytes are returned to the caller, which queues them through
//! the SAME gated post-frame poke path the editing tools use — the assembler
//! itself never writes the `Nes`, so determinism + the `emu.write` gate hold.

/// 6502 addressing mode, re-derived from the operand the disassembler emits.
/// (The CPU crate's own `AddrMode` is private; this is the assembler-side
/// classification, recovered from the public disassembly format.)
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Implied,
    Accumulator,
    Immediate,
    ZeroPage,
    ZeroPageX,
    ZeroPageY,
    Absolute,
    AbsoluteX,
    AbsoluteY,
    Indirect,
    IndirectX,
    IndirectY,
    Relative,
}

impl Mode {
    /// Number of operand bytes (excluding the opcode).
    const fn operand_len(self) -> usize {
        match self {
            Self::Implied | Self::Accumulator => 0,
            Self::Immediate
            | Self::ZeroPage
            | Self::ZeroPageX
            | Self::ZeroPageY
            | Self::IndirectX
            | Self::IndirectY
            | Self::Relative => 1,
            Self::Absolute | Self::AbsoluteX | Self::AbsoluteY | Self::Indirect => 2,
        }
    }
}

/// One opcode-table entry.
#[derive(Clone, Copy)]
struct OpEntry {
    mnemonic: &'static str,
    mode: Mode,
    opcode: u8,
}

/// Build the `(mnemonic, mode) -> opcode` table by introspecting the canonical
/// disassembler once. Branch (`Relative`) opcodes are detected by their
/// mnemonic since a disassembled relative target reads as an absolute `$xxxx`.
fn build_table() -> Vec<OpEntry> {
    const BRANCHES: [&str; 8] = ["BPL", "BMI", "BVC", "BVS", "BCC", "BCS", "BNE", "BEQ"];
    let mut table = Vec::with_capacity(256);
    for opcode in 0u16..=0xFF {
        // Disassemble this opcode followed by two known operand bytes at a
        // known PC; the disassembler classifies the addressing mode for us.
        let prog = [opcode as u8, 0x34u8, 0x12u8];
        let lines = rustynes_core::rustynes_cpu::disassemble_at(
            |a| {
                prog.get(a.wrapping_sub(0x8000) as usize)
                    .copied()
                    .unwrap_or(0)
            },
            0x8000,
            1,
        );
        let Some(line) = lines.first() else { continue };
        if line.mnemonic == "???" {
            continue; // undocumented / unknown opcode — not assemblable.
        }
        let mode = if BRANCHES.contains(&line.mnemonic) {
            Mode::Relative
        } else {
            classify_operand(&line.operand)
        };
        table.push(OpEntry {
            mnemonic: line.mnemonic,
            mode,
            opcode: opcode as u8,
        });
    }
    table
}

/// Map a disassembled operand string back to its addressing mode.
fn classify_operand(op: &str) -> Mode {
    let op = op.trim();
    if op.is_empty() {
        return Mode::Implied;
    }
    if op == "A" {
        return Mode::Accumulator;
    }
    if op.starts_with('#') {
        return Mode::Immediate;
    }
    if op.starts_with('(') {
        if op.ends_with(",X)") {
            return Mode::IndirectX;
        }
        if op.ends_with("),Y") {
            return Mode::IndirectY;
        }
        return Mode::Indirect;
    }
    // Width is encoded in the hex-digit count ($XX vs $XXXX).
    let hex_digits = op.chars().filter(char::is_ascii_hexdigit).count();
    let wide = hex_digits > 2;
    if op.ends_with(",X") {
        return if wide {
            Mode::AbsoluteX
        } else {
            Mode::ZeroPageX
        };
    }
    if op.ends_with(",Y") {
        return if wide {
            Mode::AbsoluteY
        } else {
            Mode::ZeroPageY
        };
    }
    if wide { Mode::Absolute } else { Mode::ZeroPage }
}

/// The parsed operand of a source line: a mode + a numeric value.
struct ParsedOperand {
    mode: Mode,
    value: u16,
}

/// Parse a hex/decimal number token (accepts `$NN`, `0xNN`, or decimal).
fn parse_num(tok: &str) -> Option<u16> {
    let t = tok.trim();
    if let Some(h) = t
        .strip_prefix('$')
        .or_else(|| t.strip_prefix("0x"))
        .or_else(|| t.strip_prefix("0X"))
    {
        return u16::from_str_radix(h, 16).ok();
    }
    t.parse::<u16>().ok()
}

/// Parse the operand portion of a source line into a `(mode, value)`. Branch
/// targets are resolved by the caller (which knows the instruction PC).
fn parse_operand(operand: &str) -> Option<ParsedOperand> {
    let op = operand.trim();
    if op.is_empty() {
        return Some(ParsedOperand {
            mode: Mode::Implied,
            value: 0,
        });
    }
    if op.eq_ignore_ascii_case("A") {
        return Some(ParsedOperand {
            mode: Mode::Accumulator,
            value: 0,
        });
    }
    if let Some(rest) = op.strip_prefix('#') {
        return parse_num(rest).map(|v| ParsedOperand {
            mode: Mode::Immediate,
            value: v,
        });
    }
    if let Some(inner) = op.strip_prefix('(') {
        // (zp,X) | (zp),Y | (abs)
        if let Some(zp) = inner.strip_suffix(",X)").map(str::trim) {
            return parse_num(zp).map(|v| ParsedOperand {
                mode: Mode::IndirectX,
                value: v,
            });
        }
        // `($zp),Y` — after the leading `(` is stripped, `inner` is `$zp),Y`:
        // strip the `,Y` suffix, then the closing `)`.
        if let Some(zp) = inner
            .strip_suffix(",Y")
            .or_else(|| inner.strip_suffix(",y"))
            .and_then(|s| s.strip_suffix(')'))
        {
            return parse_num(zp.trim()).map(|v| ParsedOperand {
                mode: Mode::IndirectY,
                value: v,
            });
        }
        if let Some(abs) = inner.strip_suffix(')').map(str::trim) {
            return parse_num(abs).map(|v| ParsedOperand {
                mode: Mode::Indirect,
                value: v,
            });
        }
        return None;
    }
    // Indexed or plain. Width inferred from the value (<= 0xFF → zero page),
    // except we keep absolute when the user wrote 3+ hex digits.
    let wrote_wide = op.starts_with('$') && op.chars().filter(char::is_ascii_hexdigit).count() > 2;
    if let Some(base) = op.strip_suffix(",X").or_else(|| op.strip_suffix(",x")) {
        return parse_num(base).map(|v| ParsedOperand {
            mode: if wrote_wide || v > 0xFF {
                Mode::AbsoluteX
            } else {
                Mode::ZeroPageX
            },
            value: v,
        });
    }
    if let Some(base) = op.strip_suffix(",Y").or_else(|| op.strip_suffix(",y")) {
        return parse_num(base).map(|v| ParsedOperand {
            mode: if wrote_wide || v > 0xFF {
                Mode::AbsoluteY
            } else {
                Mode::ZeroPageY
            },
            value: v,
        });
    }
    parse_num(op).map(|v| ParsedOperand {
        mode: if wrote_wide || v > 0xFF {
            Mode::Absolute
        } else {
            Mode::ZeroPage
        },
        value: v,
    })
}

/// Assemble one source line at `pc` into its opcode bytes.
///
/// `pc` is needed to compute relative-branch displacements. Returns `Err` with
/// a human-readable reason on any parse / encode failure (unknown mnemonic,
/// invalid operand, branch out of range, zero-page-only mnemonic given an
/// absolute operand, etc.).
///
/// # Errors
/// Returns a message describing why the line could not be assembled.
pub fn assemble_line(line: &str, pc: u16) -> Result<Vec<u8>, String> {
    let table = build_table();
    let line = line.trim();
    if line.is_empty() {
        return Err("empty line".into());
    }
    // Split "MNEMONIC operand..." (operand may contain spaces inside, e.g.
    // none for 6502 — but tolerate them by re-joining).
    let mut parts = line.splitn(2, char::is_whitespace);
    let mnemonic = parts.next().unwrap_or("").to_ascii_uppercase();
    let operand = parts.next().unwrap_or("").trim().to_string();
    let parsed = parse_operand(&operand).ok_or_else(|| format!("bad operand: {operand:?}"))?;

    // For a branch, the parsed (Absolute/ZeroPage) mode must become Relative.
    let want_mode = if BRANCH_MNEMONICS.contains(&mnemonic.as_str()) {
        Mode::Relative
    } else {
        parsed.mode
    };

    // Find the opcode for this (mnemonic, mode). If the mnemonic exists only in
    // absolute form but the operand parsed as zero-page (value <= 0xFF), retry
    // with the absolute mode (standard assembler widening).
    let entry = table
        .iter()
        .find(|e| e.mnemonic == mnemonic && e.mode == want_mode)
        .or_else(|| {
            let widened = match want_mode {
                Mode::ZeroPage => Some(Mode::Absolute),
                Mode::ZeroPageX => Some(Mode::AbsoluteX),
                Mode::ZeroPageY => Some(Mode::AbsoluteY),
                _ => None,
            };
            widened.and_then(|wm| {
                table
                    .iter()
                    .find(|e| e.mnemonic == mnemonic && e.mode == wm)
            })
        })
        .ok_or_else(|| format!("no opcode for {mnemonic} with that addressing mode"))?;

    let mut bytes = vec![entry.opcode];
    match entry.mode {
        Mode::Implied | Mode::Accumulator => {}
        Mode::Relative => {
            // Displacement from the byte AFTER the 2-byte branch instruction.
            let target = parsed.value;
            let next = pc.wrapping_add(2);
            let disp = i32::from(target) - i32::from(next);
            if !(-128..=127).contains(&disp) {
                return Err(format!("branch target ${target:04X} out of range"));
            }
            bytes.push(disp as i8 as u8);
        }
        m if m.operand_len() == 1 => bytes.push(parsed.value as u8),
        _ => {
            bytes.push(parsed.value as u8);
            bytes.push((parsed.value >> 8) as u8);
        }
    }
    Ok(bytes)
}

const BRANCH_MNEMONICS: [&str; 8] = ["BPL", "BMI", "BVC", "BVS", "BCC", "BCS", "BNE", "BEQ"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembles_common_instructions() {
        // LDA #$42
        assert_eq!(assemble_line("LDA #$42", 0x0000).unwrap(), vec![0xA9, 0x42]);
        // STA $0200,X (absolute,X)
        assert_eq!(
            assemble_line("STA $0200,X", 0x0000).unwrap(),
            vec![0x9D, 0x00, 0x02]
        );
        // JMP ($1234) (indirect)
        assert_eq!(
            assemble_line("JMP ($1234)", 0x0000).unwrap(),
            vec![0x6C, 0x34, 0x12]
        );
        // NOP (implied)
        assert_eq!(assemble_line("NOP", 0x0000).unwrap(), vec![0xEA]);
        // INX (implied)
        assert_eq!(assemble_line("INX", 0x0000).unwrap(), vec![0xE8]);
        // LDA $10 (zero page)
        assert_eq!(assemble_line("LDA $10", 0x0000).unwrap(), vec![0xA5, 0x10]);
    }

    #[test]
    fn branch_displacement() {
        // BNE forward: target $C010 from PC $C000 → disp = 0x10 - 2 = 0x0E.
        assert_eq!(
            assemble_line("BNE $C010", 0xC000).unwrap(),
            vec![0xD0, 0x0E]
        );
        // BNE backward: target $C000 from PC $C010 → next=$C012, disp=-0x12.
        assert_eq!(
            assemble_line("BNE $C000", 0xC010).unwrap(),
            vec![0xD0, (-0x12i8) as u8]
        );
        // Out of range.
        assert!(assemble_line("BNE $E000", 0xC000).is_err());
    }

    #[test]
    fn round_trips_through_disassembler() {
        // Assemble then disassemble a handful and compare the mnemonic.
        for (src, pc) in [("LDX #$00", 0u16), ("STA $0300", 0), ("CMP ($20),Y", 0)] {
            let bytes = assemble_line(src, pc).unwrap();
            let lines = rustynes_core::rustynes_cpu::disassemble_at(
                |a| bytes.get(a as usize).copied().unwrap_or(0),
                0,
                1,
            );
            assert_eq!(lines[0].mnemonic, src.split_whitespace().next().unwrap());
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(assemble_line("FOO #$01", 0).is_err());
        assert!(assemble_line("", 0).is_err());
    }
}

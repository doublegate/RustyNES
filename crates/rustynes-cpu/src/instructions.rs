//! 6502 Instruction implementations.
//!
//! This module contains the implementations of all 256 opcodes,
//! including official and unofficial (undocumented) instructions.

use crate::cpu::{Bus, Cpu};
use crate::status::Status;
use crate::vectors;

/// Instruction function type.
pub type InstrFn = fn(&mut Cpu, &mut dyn Bus);

// ============================================================================
// Official Instructions
// ============================================================================

/// ADC - Add with Carry
fn adc(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let value = cpu.read_byte(bus, cpu.operand_addr());
    add(cpu, value);
}

/// Common add operation used by ADC and SBC
fn add(cpu: &mut Cpu, value: u8) {
    let a = u16::from(cpu.a());
    let v = u16::from(value);
    let c = u16::from(cpu.status().contains(Status::C) as u8);

    let result = a.wrapping_add(v).wrapping_add(c);
    let result8 = result as u8;

    // Set carry if overflow occurred
    cpu.status.set_flag(Status::C, result > 0xFF);

    // Set overflow if sign bit is incorrect
    // Overflow occurs when both inputs have the same sign but the result has a different sign
    cpu.status
        .set_flag(Status::V, (!(a ^ v) & (a ^ result)) & 0x80 != 0);

    cpu.a = result8;
    cpu.set_zn(result8);
}

/// AND - Logical AND
fn and(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let value = cpu.read_byte(bus, cpu.operand_addr());
    cpu.a &= value;
    cpu.set_zn(cpu.a);
}

/// ASL - Arithmetic Shift Left (Accumulator)
fn asl_acc(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    let value = cpu.a;
    cpu.status.set_flag(Status::C, value & 0x80 != 0);
    cpu.a = value << 1;
    cpu.set_zn(cpu.a);
}

/// ASL - Arithmetic Shift Left (Memory)
fn asl_mem(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let addr = cpu.operand_addr();
    let value = cpu.read_byte(bus, addr);
    // Dummy write (RMW instruction)
    cpu.write_byte(bus, addr, value);

    cpu.status.set_flag(Status::C, value & 0x80 != 0);
    let result = value << 1;
    cpu.write_byte(bus, addr, result);
    cpu.set_zn(result);
}

/// BCC - Branch if Carry Clear
fn bcc(cpu: &mut Cpu, bus: &mut dyn Bus) {
    branch(cpu, bus, !cpu.status().contains(Status::C));
}

/// BCS - Branch if Carry Set
fn bcs(cpu: &mut Cpu, bus: &mut dyn Bus) {
    branch(cpu, bus, cpu.status().contains(Status::C));
}

/// BEQ - Branch if Equal (Zero flag set)
fn beq(cpu: &mut Cpu, bus: &mut dyn Bus) {
    branch(cpu, bus, cpu.status().contains(Status::Z));
}

/// BIT - Bit Test
fn bit(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let value = cpu.read_byte(bus, cpu.operand_addr());
    cpu.status.set_flag(Status::Z, cpu.a & value == 0);
    cpu.status.set_flag(Status::V, value & 0x40 != 0);
    cpu.status.set_flag(Status::N, value & 0x80 != 0);
}

/// BMI - Branch if Minus (Negative flag set)
fn bmi(cpu: &mut Cpu, bus: &mut dyn Bus) {
    branch(cpu, bus, cpu.status().contains(Status::N));
}

/// BNE - Branch if Not Equal (Zero flag clear)
fn bne(cpu: &mut Cpu, bus: &mut dyn Bus) {
    branch(cpu, bus, !cpu.status().contains(Status::Z));
}

/// BPL - Branch if Plus (Negative flag clear)
fn bpl(cpu: &mut Cpu, bus: &mut dyn Bus) {
    branch(cpu, bus, !cpu.status().contains(Status::N));
}

/// BRK - Force Interrupt
fn brk(cpu: &mut Cpu, bus: &mut dyn Bus) {
    // PC has already been incremented past opcode
    cpu.pc = cpu.pc.wrapping_add(1); // Skip padding byte

    // Push PC and status
    cpu.push_word(bus, cpu.pc);

    // Check for NMI hijacking (if NMI occurs between cycles 4-5)
    let vector = if cpu.nmi_pending {
        cpu.nmi_pending = false;
        cpu.nmi_triggered = false;
        vectors::NMI
    } else {
        vectors::IRQ
    };

    // Push status with B flag set
    let status_byte = cpu.status.to_stack_byte(true);
    cpu.push_byte(bus, status_byte);

    // Set I flag
    cpu.status.set_flag(Status::I, true);

    // Load vector
    let lo = cpu.read_byte(bus, vector);
    let hi = cpu.read_byte(bus, vector + 1);
    cpu.pc = u16::from_le_bytes([lo, hi]);
}

/// BVC - Branch if Overflow Clear
fn bvc(cpu: &mut Cpu, bus: &mut dyn Bus) {
    branch(cpu, bus, !cpu.status().contains(Status::V));
}

/// BVS - Branch if Overflow Set
fn bvs(cpu: &mut Cpu, bus: &mut dyn Bus) {
    branch(cpu, bus, cpu.status().contains(Status::V));
}

/// CLC - Clear Carry Flag
fn clc(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.status.set_flag(Status::C, false);
}

/// CLD - Clear Decimal Mode
fn cld(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.status.set_flag(Status::D, false);
}

/// CLI - Clear Interrupt Disable
fn cli(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.status.set_flag(Status::I, false);
}

/// CLV - Clear Overflow Flag
fn clv(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.status.set_flag(Status::V, false);
}

/// CMP - Compare Accumulator
fn cmp(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let value = cpu.read_byte(bus, cpu.operand_addr());
    compare(cpu, cpu.a, value);
}

/// CPX - Compare X Register
fn cpx(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let value = cpu.read_byte(bus, cpu.operand_addr());
    compare(cpu, cpu.x, value);
}

/// CPY - Compare Y Register
fn cpy(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let value = cpu.read_byte(bus, cpu.operand_addr());
    compare(cpu, cpu.y, value);
}

/// Common compare operation
fn compare(cpu: &mut Cpu, reg: u8, value: u8) {
    cpu.status.set_flag(Status::C, reg >= value);
    cpu.status.set_flag(Status::Z, reg == value);
    cpu.status
        .set_flag(Status::N, reg.wrapping_sub(value) & 0x80 != 0);
}

/// DEC - Decrement Memory
fn dec(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let addr = cpu.operand_addr();
    let value = cpu.read_byte(bus, addr);
    // Dummy write (RMW instruction)
    cpu.write_byte(bus, addr, value);

    let result = value.wrapping_sub(1);
    cpu.write_byte(bus, addr, result);
    cpu.set_zn(result);
}

/// DEX - Decrement X Register
fn dex(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.x = cpu.x.wrapping_sub(1);
    cpu.set_zn(cpu.x);
}

/// DEY - Decrement Y Register
fn dey(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.y = cpu.y.wrapping_sub(1);
    cpu.set_zn(cpu.y);
}

/// EOR - Exclusive OR
fn eor(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let value = cpu.read_byte(bus, cpu.operand_addr());
    cpu.a ^= value;
    cpu.set_zn(cpu.a);
}

/// INC - Increment Memory
fn inc(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let addr = cpu.operand_addr();
    let value = cpu.read_byte(bus, addr);
    // Dummy write (RMW instruction)
    cpu.write_byte(bus, addr, value);

    let result = value.wrapping_add(1);
    cpu.write_byte(bus, addr, result);
    cpu.set_zn(result);
}

/// INX - Increment X Register
fn inx(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.x = cpu.x.wrapping_add(1);
    cpu.set_zn(cpu.x);
}

/// INY - Increment Y Register
fn iny(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.y = cpu.y.wrapping_add(1);
    cpu.set_zn(cpu.y);
}

/// JMP - Jump (Absolute)
fn jmp_abs(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.pc = cpu.operand_addr();
}

/// JMP - Jump (Indirect)
fn jmp_ind(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.pc = cpu.operand_addr();
}

/// JSR - Jump to Subroutine
fn jsr(cpu: &mut Cpu, bus: &mut dyn Bus) {
    // Note: fetch_operand for Abs mode already read the target address
    // and advanced PC past the operand bytes

    // Internal operation (dummy cycle)
    cpu.tick(bus);

    // Push return address (PC - 1, pointing to last byte of JSR instruction)
    // PC is now past the operand, so PC - 1 points to the high byte of target address
    cpu.push_word(bus, cpu.pc.wrapping_sub(1));

    // Jump to target address (already fetched into operand_addr)
    cpu.pc = cpu.operand_addr;
}

/// LDA - Load Accumulator
fn lda(cpu: &mut Cpu, bus: &mut dyn Bus) {
    cpu.a = cpu.read_byte(bus, cpu.operand_addr());
    cpu.set_zn(cpu.a);
}

/// LDX - Load X Register
fn ldx(cpu: &mut Cpu, bus: &mut dyn Bus) {
    cpu.x = cpu.read_byte(bus, cpu.operand_addr());
    cpu.set_zn(cpu.x);
}

/// LDY - Load Y Register
fn ldy(cpu: &mut Cpu, bus: &mut dyn Bus) {
    cpu.y = cpu.read_byte(bus, cpu.operand_addr());
    cpu.set_zn(cpu.y);
}

/// LSR - Logical Shift Right (Accumulator)
fn lsr_acc(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    let value = cpu.a;
    cpu.status.set_flag(Status::C, value & 0x01 != 0);
    cpu.a = value >> 1;
    cpu.set_zn(cpu.a);
}

/// LSR - Logical Shift Right (Memory)
fn lsr_mem(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let addr = cpu.operand_addr();
    let value = cpu.read_byte(bus, addr);
    // Dummy write (RMW instruction)
    cpu.write_byte(bus, addr, value);

    cpu.status.set_flag(Status::C, value & 0x01 != 0);
    let result = value >> 1;
    cpu.write_byte(bus, addr, result);
    cpu.set_zn(result);
}

/// NOP - No Operation
fn nop(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    // Do nothing
    let _ = cpu;
}

/// NOP with read (unofficial NOPs that read memory)
fn nop_read(cpu: &mut Cpu, bus: &mut dyn Bus) {
    // Read and discard
    let _ = cpu.read_byte(bus, cpu.operand_addr());
}

/// ORA - Logical Inclusive OR
fn ora(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let value = cpu.read_byte(bus, cpu.operand_addr());
    cpu.a |= value;
    cpu.set_zn(cpu.a);
}

/// PHA - Push Accumulator
fn pha(cpu: &mut Cpu, bus: &mut dyn Bus) {
    cpu.push_byte(bus, cpu.a);
}

/// PHP - Push Processor Status
fn php(cpu: &mut Cpu, bus: &mut dyn Bus) {
    // B and U flags are set when pushing
    let status_byte = cpu.status.to_stack_byte(true);
    cpu.push_byte(bus, status_byte);
}

/// PLA - Pull Accumulator
fn pla(cpu: &mut Cpu, bus: &mut dyn Bus) {
    // Dummy read
    cpu.tick(bus);
    cpu.a = cpu.pop_byte(bus);
    cpu.set_zn(cpu.a);
}

/// PLP - Pull Processor Status
fn plp(cpu: &mut Cpu, bus: &mut dyn Bus) {
    // Dummy read
    cpu.tick(bus);
    let value = cpu.pop_byte(bus);
    cpu.status = Status::from_stack_byte(value);
}

/// ROL - Rotate Left (Accumulator)
fn rol_acc(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    let value = cpu.a;
    let carry = cpu.status.contains(Status::C) as u8;
    cpu.status.set_flag(Status::C, value & 0x80 != 0);
    cpu.a = (value << 1) | carry;
    cpu.set_zn(cpu.a);
}

/// ROL - Rotate Left (Memory)
fn rol_mem(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let addr = cpu.operand_addr();
    let value = cpu.read_byte(bus, addr);
    // Dummy write (RMW instruction)
    cpu.write_byte(bus, addr, value);

    let carry = cpu.status.contains(Status::C) as u8;
    cpu.status.set_flag(Status::C, value & 0x80 != 0);
    let result = (value << 1) | carry;
    cpu.write_byte(bus, addr, result);
    cpu.set_zn(result);
}

/// ROR - Rotate Right (Accumulator)
fn ror_acc(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    let value = cpu.a;
    let carry = (cpu.status.contains(Status::C) as u8) << 7;
    cpu.status.set_flag(Status::C, value & 0x01 != 0);
    cpu.a = (value >> 1) | carry;
    cpu.set_zn(cpu.a);
}

/// ROR - Rotate Right (Memory)
fn ror_mem(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let addr = cpu.operand_addr();
    let value = cpu.read_byte(bus, addr);
    // Dummy write (RMW instruction)
    cpu.write_byte(bus, addr, value);

    let carry = (cpu.status.contains(Status::C) as u8) << 7;
    cpu.status.set_flag(Status::C, value & 0x01 != 0);
    let result = (value >> 1) | carry;
    cpu.write_byte(bus, addr, result);
    cpu.set_zn(result);
}

/// RTI - Return from Interrupt
fn rti(cpu: &mut Cpu, bus: &mut dyn Bus) {
    // Dummy read
    cpu.tick(bus);

    // Pull status
    let status = cpu.pop_byte(bus);
    cpu.status = Status::from_stack_byte(status);

    // Pull PC
    cpu.pc = cpu.pop_word(bus);
}

/// RTS - Return from Subroutine
fn rts(cpu: &mut Cpu, bus: &mut dyn Bus) {
    // Dummy read
    cpu.tick(bus);

    // Pull PC
    cpu.pc = cpu.pop_word(bus);

    // Increment PC
    cpu.tick(bus);
    cpu.pc = cpu.pc.wrapping_add(1);
}

/// SBC - Subtract with Carry
fn sbc(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let value = cpu.read_byte(bus, cpu.operand_addr());
    // SBC is ADC with inverted operand
    add(cpu, !value);
}

/// SEC - Set Carry Flag
fn sec(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.status.set_flag(Status::C, true);
}

/// SED - Set Decimal Flag
fn sed(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.status.set_flag(Status::D, true);
}

/// SEI - Set Interrupt Disable
fn sei(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.status.set_flag(Status::I, true);
}

/// STA - Store Accumulator
fn sta(cpu: &mut Cpu, bus: &mut dyn Bus) {
    cpu.write_byte(bus, cpu.operand_addr(), cpu.a);
}

/// STX - Store X Register
fn stx(cpu: &mut Cpu, bus: &mut dyn Bus) {
    cpu.write_byte(bus, cpu.operand_addr(), cpu.x);
}

/// STY - Store Y Register
fn sty(cpu: &mut Cpu, bus: &mut dyn Bus) {
    cpu.write_byte(bus, cpu.operand_addr(), cpu.y);
}

/// TAX - Transfer Accumulator to X
fn tax(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.x = cpu.a;
    cpu.set_zn(cpu.x);
}

/// TAY - Transfer Accumulator to Y
fn tay(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.y = cpu.a;
    cpu.set_zn(cpu.y);
}

/// TSX - Transfer Stack Pointer to X
fn tsx(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.x = cpu.sp;
    cpu.set_zn(cpu.x);
}

/// TXA - Transfer X to Accumulator
fn txa(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.a = cpu.x;
    cpu.set_zn(cpu.a);
}

/// TXS - Transfer X to Stack Pointer
fn txs(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.sp = cpu.x;
    // TXS does not affect flags
}

/// TYA - Transfer Y to Accumulator
fn tya(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    cpu.a = cpu.y;
    cpu.set_zn(cpu.a);
}

/// Common branch operation
fn branch(cpu: &mut Cpu, bus: &mut dyn Bus, condition: bool) {
    if condition {
        // Branch delays IRQ during its last clock
        if cpu.run_irq && !cpu.prev_run_irq {
            cpu.run_irq = false;
        }

        // Dummy read (cycle for branch taken)
        cpu.tick(bus);

        let offset = cpu.operand_value as i8;
        let new_pc = cpu.pc.wrapping_add(offset as u16);

        // Extra cycle if page boundary crossed
        if (cpu.pc & 0xFF00) != (new_pc & 0xFF00) {
            cpu.tick(bus);
        }

        cpu.pc = new_pc;
    }
}

// ============================================================================
// Unofficial/Undocumented Instructions
// ============================================================================

/// AAC (ANC) - AND byte with accumulator, then copy N to C
fn aac(cpu: &mut Cpu, bus: &mut dyn Bus) {
    cpu.a &= cpu.read_byte(bus, cpu.operand_addr());
    cpu.set_zn(cpu.a);
    cpu.status
        .set_flag(Status::C, cpu.status.contains(Status::N));
}

/// AAX (SAX) - AND X register with accumulator, store result
fn aax(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let value = cpu.a & cpu.x;
    cpu.write_byte(bus, cpu.operand_addr(), value);
}

/// ARR - AND byte with accumulator, then rotate one bit right
fn arr(cpu: &mut Cpu, bus: &mut dyn Bus) {
    cpu.a &= cpu.read_byte(bus, cpu.operand_addr());
    let carry = cpu.status.contains(Status::C) as u8;
    cpu.a = (cpu.a >> 1) | (carry << 7);
    cpu.set_zn(cpu.a);

    cpu.status.set_flag(Status::C, cpu.a & 0x40 != 0);
    cpu.status
        .set_flag(Status::V, ((cpu.a & 0x40) ^ ((cpu.a & 0x20) << 1)) != 0);
}

/// ASR (ALR) - AND byte with accumulator, then shift right one bit
fn asr(cpu: &mut Cpu, bus: &mut dyn Bus) {
    cpu.a &= cpu.read_byte(bus, cpu.operand_addr());
    cpu.status.set_flag(Status::C, cpu.a & 0x01 != 0);
    cpu.a >>= 1;
    cpu.set_zn(cpu.a);
}

/// ATX (LXA) - AND byte with accumulator, then transfer to X
fn atx(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let value = cpu.read_byte(bus, cpu.operand_addr());
    cpu.a = value;
    cpu.x = cpu.a;
    cpu.set_zn(cpu.a);
}

/// AXS (SBX) - AND X register with accumulator and store result in X, minus value
fn axs(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let value = cpu.read_byte(bus, cpu.operand_addr());
    let and_result = cpu.a & cpu.x;
    cpu.status.set_flag(Status::C, and_result >= value);
    cpu.x = and_result.wrapping_sub(value);
    cpu.set_zn(cpu.x);
}

/// DCP - Decrement memory, then compare with accumulator
fn dcp(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let addr = cpu.operand_addr();
    let value = cpu.read_byte(bus, addr);
    cpu.write_byte(bus, addr, value); // Dummy write

    let result = value.wrapping_sub(1);
    cpu.write_byte(bus, addr, result);
    compare(cpu, cpu.a, result);
}

/// ISB (ISC) - Increment memory, then subtract from accumulator
fn isb(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let addr = cpu.operand_addr();
    let value = cpu.read_byte(bus, addr);
    cpu.write_byte(bus, addr, value); // Dummy write

    let result = value.wrapping_add(1);
    cpu.write_byte(bus, addr, result);
    add(cpu, !result);
}

/// HLT (KIL/JAM) - Halt the processor
fn hlt(cpu: &mut Cpu, _bus: &mut dyn Bus) {
    // Decrement PC to keep re-executing this instruction
    cpu.pc = cpu.pc.wrapping_sub(1);
    log::warn!("CPU halted at 0x{:04X}", cpu.pc);
}

/// LAR (LAS) - AND memory with stack pointer, transfer to A, X, and SP
fn lar(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let value = cpu.read_byte(bus, cpu.operand_addr());
    let result = value & cpu.sp;
    cpu.a = result;
    cpu.x = result;
    cpu.sp = result;
    cpu.set_zn(result);
}

/// LAX - Load A and X with memory
fn lax(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let value = cpu.read_byte(bus, cpu.operand_addr());
    cpu.a = value;
    cpu.x = value;
    cpu.set_zn(value);
}

/// RLA - ROL memory, then AND with accumulator
fn rla(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let addr = cpu.operand_addr();
    let value = cpu.read_byte(bus, addr);
    cpu.write_byte(bus, addr, value); // Dummy write

    let carry = cpu.status.contains(Status::C) as u8;
    cpu.status.set_flag(Status::C, value & 0x80 != 0);
    let result = (value << 1) | carry;
    cpu.write_byte(bus, addr, result);

    cpu.a &= result;
    cpu.set_zn(cpu.a);
}

/// RRA - ROR memory, then ADC with accumulator
fn rra(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let addr = cpu.operand_addr();
    let value = cpu.read_byte(bus, addr);
    cpu.write_byte(bus, addr, value); // Dummy write

    let carry = (cpu.status.contains(Status::C) as u8) << 7;
    cpu.status.set_flag(Status::C, value & 0x01 != 0);
    let result = (value >> 1) | carry;
    cpu.write_byte(bus, addr, result);

    add(cpu, result);
}

/// SLO (ASO) - ASL memory, then ORA with accumulator
fn slo(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let addr = cpu.operand_addr();
    let value = cpu.read_byte(bus, addr);
    cpu.write_byte(bus, addr, value); // Dummy write

    cpu.status.set_flag(Status::C, value & 0x80 != 0);
    let result = value << 1;
    cpu.write_byte(bus, addr, result);

    cpu.a |= result;
    cpu.set_zn(cpu.a);
}

/// SRE (LSE) - LSR memory, then EOR with accumulator
fn sre(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let addr = cpu.operand_addr();
    let value = cpu.read_byte(bus, addr);
    cpu.write_byte(bus, addr, value); // Dummy write

    cpu.status.set_flag(Status::C, value & 0x01 != 0);
    let result = value >> 1;
    cpu.write_byte(bus, addr, result);

    cpu.a ^= result;
    cpu.set_zn(cpu.a);
}

/// SXA (SHX/XAS) - Store X & (high byte of address + 1) at address
fn sxa(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let addr = cpu.operand_addr();
    let hi = ((addr >> 8) as u8).wrapping_add(1);
    let value = cpu.x & hi;
    cpu.write_byte(bus, addr, value);
}

/// SYA (SHY/SAY) - Store Y & (high byte of address + 1) at address
fn sya(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let addr = cpu.operand_addr();
    let hi = ((addr >> 8) as u8).wrapping_add(1);
    let value = cpu.y & hi;
    cpu.write_byte(bus, addr, value);
}

/// XAA (ANE) - Unstable: (A | const) & X & M
fn xaa(cpu: &mut Cpu, bus: &mut dyn Bus) {
    // The constant varies by CPU, typically 0xEE
    let value = cpu.read_byte(bus, cpu.operand_addr());
    cpu.a = (cpu.a | 0xEE) & cpu.x & value;
    cpu.set_zn(cpu.a);
}

/// XAS (TAS/SHS) - Store A & X in SP, store A & X & (high byte + 1) in memory
fn xas(cpu: &mut Cpu, bus: &mut dyn Bus) {
    cpu.sp = cpu.a & cpu.x;
    let addr = cpu.operand_addr();
    let hi = ((addr >> 8) as u8).wrapping_add(1);
    let value = cpu.a & cpu.x & hi;
    cpu.write_byte(bus, addr, value);
}

/// AXA (SHA/AHX) - Store A & X & (high byte + 1) at address
fn axa(cpu: &mut Cpu, bus: &mut dyn Bus) {
    let addr = cpu.operand_addr();
    let hi = ((addr >> 8) as u8).wrapping_add(1);
    let value = cpu.a & cpu.x & hi;
    cpu.write_byte(bus, addr, value);
}

// ============================================================================
// Opcode Table
// ============================================================================

/// Opcode lookup table.
/// Indexed by opcode byte (0x00-0xFF).
#[rustfmt::skip]
pub static OPCODE_TABLE: [InstrFn; 256] = [
    //       0          1          2          3          4          5          6          7          8          9          A          B          C          D          E          F
    /* 0 */ brk,       ora,       hlt,       slo,       nop_read,  ora,       asl_mem,   slo,       php,       ora,       asl_acc,   aac,       nop_read,  ora,       asl_mem,   slo,
    /* 1 */ bpl,       ora,       hlt,       slo,       nop_read,  ora,       asl_mem,   slo,       clc,       ora,       nop,       slo,       nop_read,  ora,       asl_mem,   slo,
    /* 2 */ jsr,       and,       hlt,       rla,       bit,       and,       rol_mem,   rla,       plp,       and,       rol_acc,   aac,       bit,       and,       rol_mem,   rla,
    /* 3 */ bmi,       and,       hlt,       rla,       nop_read,  and,       rol_mem,   rla,       sec,       and,       nop,       rla,       nop_read,  and,       rol_mem,   rla,
    /* 4 */ rti,       eor,       hlt,       sre,       nop_read,  eor,       lsr_mem,   sre,       pha,       eor,       lsr_acc,   asr,       jmp_abs,   eor,       lsr_mem,   sre,
    /* 5 */ bvc,       eor,       hlt,       sre,       nop_read,  eor,       lsr_mem,   sre,       cli,       eor,       nop,       sre,       nop_read,  eor,       lsr_mem,   sre,
    /* 6 */ rts,       adc,       hlt,       rra,       nop_read,  adc,       ror_mem,   rra,       pla,       adc,       ror_acc,   arr,       jmp_ind,   adc,       ror_mem,   rra,
    /* 7 */ bvs,       adc,       hlt,       rra,       nop_read,  adc,       ror_mem,   rra,       sei,       adc,       nop,       rra,       nop_read,  adc,       ror_mem,   rra,
    /* 8 */ nop_read,  sta,       nop_read,  aax,       sty,       sta,       stx,       aax,       dey,       nop_read,  txa,       xaa,       sty,       sta,       stx,       aax,
    /* 9 */ bcc,       sta,       hlt,       axa,       sty,       sta,       stx,       aax,       tya,       sta,       txs,       xas,       sya,       sta,       sxa,       axa,
    /* A */ ldy,       lda,       ldx,       lax,       ldy,       lda,       ldx,       lax,       tay,       lda,       tax,       atx,       ldy,       lda,       ldx,       lax,
    /* B */ bcs,       lda,       hlt,       lax,       ldy,       lda,       ldx,       lax,       clv,       lda,       tsx,       lar,       ldy,       lda,       ldx,       lax,
    /* C */ cpy,       cmp,       nop_read,  dcp,       cpy,       cmp,       dec,       dcp,       iny,       cmp,       dex,       axs,       cpy,       cmp,       dec,       dcp,
    /* D */ bne,       cmp,       hlt,       dcp,       nop_read,  cmp,       dec,       dcp,       cld,       cmp,       nop,       dcp,       nop_read,  cmp,       dec,       dcp,
    /* E */ cpx,       sbc,       nop_read,  isb,       cpx,       sbc,       inc,       isb,       inx,       sbc,       nop,       sbc,       cpx,       sbc,       inc,       isb,
    /* F */ beq,       sbc,       hlt,       isb,       nop_read,  sbc,       inc,       isb,       sed,       sbc,       nop,       isb,       nop_read,  sbc,       inc,       isb,
];

/// Get the instruction name for an opcode.
#[must_use]
#[allow(dead_code)]
pub fn opcode_name(opcode: u8) -> &'static str {
    #[rustfmt::skip]
    const NAMES: [&str; 256] = [
        //       0       1       2       3       4       5       6       7       8       9       A       B       C       D       E       F
        /* 0 */ "BRK", "ORA", "HLT", "SLO", "NOP", "ORA", "ASL", "SLO", "PHP", "ORA", "ASL", "AAC", "NOP", "ORA", "ASL", "SLO",
        /* 1 */ "BPL", "ORA", "HLT", "SLO", "NOP", "ORA", "ASL", "SLO", "CLC", "ORA", "NOP", "SLO", "NOP", "ORA", "ASL", "SLO",
        /* 2 */ "JSR", "AND", "HLT", "RLA", "BIT", "AND", "ROL", "RLA", "PLP", "AND", "ROL", "AAC", "BIT", "AND", "ROL", "RLA",
        /* 3 */ "BMI", "AND", "HLT", "RLA", "NOP", "AND", "ROL", "RLA", "SEC", "AND", "NOP", "RLA", "NOP", "AND", "ROL", "RLA",
        /* 4 */ "RTI", "EOR", "HLT", "SRE", "NOP", "EOR", "LSR", "SRE", "PHA", "EOR", "LSR", "ASR", "JMP", "EOR", "LSR", "SRE",
        /* 5 */ "BVC", "EOR", "HLT", "SRE", "NOP", "EOR", "LSR", "SRE", "CLI", "EOR", "NOP", "SRE", "NOP", "EOR", "LSR", "SRE",
        /* 6 */ "RTS", "ADC", "HLT", "RRA", "NOP", "ADC", "ROR", "RRA", "PLA", "ADC", "ROR", "ARR", "JMP", "ADC", "ROR", "RRA",
        /* 7 */ "BVS", "ADC", "HLT", "RRA", "NOP", "ADC", "ROR", "RRA", "SEI", "ADC", "NOP", "RRA", "NOP", "ADC", "ROR", "RRA",
        /* 8 */ "NOP", "STA", "NOP", "SAX", "STY", "STA", "STX", "SAX", "DEY", "NOP", "TXA", "XAA", "STY", "STA", "STX", "SAX",
        /* 9 */ "BCC", "STA", "HLT", "AXA", "STY", "STA", "STX", "SAX", "TYA", "STA", "TXS", "XAS", "SYA", "STA", "SXA", "AXA",
        /* A */ "LDY", "LDA", "LDX", "LAX", "LDY", "LDA", "LDX", "LAX", "TAY", "LDA", "TAX", "ATX", "LDY", "LDA", "LDX", "LAX",
        /* B */ "BCS", "LDA", "HLT", "LAX", "LDY", "LDA", "LDX", "LAX", "CLV", "LDA", "TSX", "LAS", "LDY", "LDA", "LDX", "LAX",
        /* C */ "CPY", "CMP", "NOP", "DCP", "CPY", "CMP", "DEC", "DCP", "INY", "CMP", "DEX", "AXS", "CPY", "CMP", "DEC", "DCP",
        /* D */ "BNE", "CMP", "HLT", "DCP", "NOP", "CMP", "DEC", "DCP", "CLD", "CMP", "NOP", "DCP", "NOP", "CMP", "DEC", "DCP",
        /* E */ "CPX", "SBC", "NOP", "ISB", "CPX", "SBC", "INC", "ISB", "INX", "SBC", "NOP", "SBC", "CPX", "SBC", "INC", "ISB",
        /* F */ "BEQ", "SBC", "HLT", "ISB", "NOP", "SBC", "INC", "ISB", "SED", "SBC", "NOP", "ISB", "NOP", "SBC", "INC", "ISB",
    ];
    NAMES[opcode as usize]
}

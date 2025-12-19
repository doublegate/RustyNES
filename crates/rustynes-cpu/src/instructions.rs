//! 6502 instruction implementations.
//!
//! This module contains the actual execution logic for all 256 opcodes,
//! including both official (151) and unofficial (105) instructions.

use crate::addressing::AddressingMode;
use crate::bus::Bus;
use crate::cpu::Cpu;
use crate::status::StatusFlags;

impl Cpu {
    //
    // ========== LOAD/STORE INSTRUCTIONS ==========
    //

    /// LDA - Load Accumulator
    pub(crate) fn lda(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let (value, page_crossed) = self.read_operand(bus, mode);
        self.a = value;
        self.set_zn(value);
        u8::from(page_crossed)
    }

    /// LDX - Load X Register
    pub(crate) fn ldx(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let (value, page_crossed) = self.read_operand(bus, mode);
        self.x = value;
        self.set_zn(value);
        u8::from(page_crossed)
    }

    /// LDY - Load Y Register
    pub(crate) fn ldy(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let (value, page_crossed) = self.read_operand(bus, mode);
        self.y = value;
        self.set_zn(value);
        u8::from(page_crossed)
    }

    /// STA - Store Accumulator
    pub(crate) fn sta(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        self.write_operand(bus, mode, self.a);
        0
    }

    /// STX - Store X Register
    pub(crate) fn stx(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        self.write_operand(bus, mode, self.x);
        0
    }

    /// STY - Store Y Register
    pub(crate) fn sty(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        self.write_operand(bus, mode, self.y);
        0
    }

    //
    // ========== TRANSFER INSTRUCTIONS ==========
    //

    /// TAX - Transfer A to X
    pub(crate) fn tax(&mut self) -> u8 {
        self.x = self.a;
        self.set_zn(self.x);
        0
    }

    /// TAY - Transfer A to Y
    pub(crate) fn tay(&mut self) -> u8 {
        self.y = self.a;
        self.set_zn(self.y);
        0
    }

    /// TXA - Transfer X to A
    pub(crate) fn txa(&mut self) -> u8 {
        self.a = self.x;
        self.set_zn(self.a);
        0
    }

    /// TYA - Transfer Y to A
    pub(crate) fn tya(&mut self) -> u8 {
        self.a = self.y;
        self.set_zn(self.a);
        0
    }

    /// TSX - Transfer SP to X
    pub(crate) fn tsx(&mut self) -> u8 {
        self.x = self.sp;
        self.set_zn(self.x);
        0
    }

    /// TXS - Transfer X to SP
    pub(crate) fn txs(&mut self) -> u8 {
        self.sp = self.x;
        0
    }

    //
    // ========== STACK INSTRUCTIONS ==========
    //

    /// PHA - Push Accumulator
    pub(crate) fn pha(&mut self, bus: &mut impl Bus) -> u8 {
        self.push(bus, self.a);
        0
    }

    /// PHP - Push Processor Status
    pub(crate) fn php(&mut self, bus: &mut impl Bus) -> u8 {
        self.push(bus, self.status.to_stack_byte(true)); // B=1, U=1
        0
    }

    /// PLA - Pull Accumulator
    pub(crate) fn pla(&mut self, bus: &mut impl Bus) -> u8 {
        self.a = self.pop(bus);
        self.set_zn(self.a);
        0
    }

    /// PLP - Pull Processor Status
    pub(crate) fn plp(&mut self, bus: &mut impl Bus) -> u8 {
        let value = self.pop(bus);
        self.status = StatusFlags::from_stack_byte(value);
        0
    }

    //
    // ========== ARITHMETIC INSTRUCTIONS ==========
    //

    /// ADC - Add with Carry
    pub(crate) fn adc(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let (value, page_crossed) = self.read_operand(bus, mode);
        self.adc_impl(value);
        u8::from(page_crossed)
    }

    /// SBC - Subtract with Carry
    pub(crate) fn sbc(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let (value, page_crossed) = self.read_operand(bus, mode);
        self.sbc_impl(value);
        u8::from(page_crossed)
    }

    /// ADC implementation (shared with unofficial opcodes)
    pub(crate) fn adc_impl(&mut self, value: u8) {
        let carry = u16::from(self.status.contains(StatusFlags::CARRY));
        let a = u16::from(self.a);
        let m = u16::from(value);
        let sum = a + m + carry;

        let result = (sum & 0xFF) as u8;

        // Set flags
        self.status.set(StatusFlags::CARRY, sum > 0xFF);
        self.status
            .set(StatusFlags::OVERFLOW, (a ^ sum) & (m ^ sum) & 0x80 != 0);
        self.set_zn(result);

        self.a = result;
    }

    /// SBC implementation (shared with unofficial opcodes)
    pub(crate) fn sbc_impl(&mut self, value: u8) {
        // SBC is equivalent to ADC with inverted value
        self.adc_impl(!value);
    }

    //
    // ========== INCREMENT/DECREMENT INSTRUCTIONS ==========
    //

    /// INC - Increment Memory
    pub(crate) fn inc(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let result = mode.resolve(self.pc, self.x, self.y, bus);
        self.pc = self.pc.wrapping_add(u16::from(mode.operand_bytes()));

        let addr = result.addr;
        let value = bus.read(addr);
        bus.write(addr, value); // Dummy write
        let new_value = value.wrapping_add(1);
        bus.write(addr, new_value);
        self.set_zn(new_value);
        0
    }

    /// DEC - Decrement Memory
    pub(crate) fn dec(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let result = mode.resolve(self.pc, self.x, self.y, bus);
        self.pc = self.pc.wrapping_add(u16::from(mode.operand_bytes()));

        let addr = result.addr;
        let value = bus.read(addr);
        bus.write(addr, value); // Dummy write
        let new_value = value.wrapping_sub(1);
        bus.write(addr, new_value);
        self.set_zn(new_value);
        0
    }

    /// INX - Increment X
    pub(crate) fn inx(&mut self) -> u8 {
        self.x = self.x.wrapping_add(1);
        self.set_zn(self.x);
        0
    }

    /// INY - Increment Y
    pub(crate) fn iny(&mut self) -> u8 {
        self.y = self.y.wrapping_add(1);
        self.set_zn(self.y);
        0
    }

    /// DEX - Decrement X
    pub(crate) fn dex(&mut self) -> u8 {
        self.x = self.x.wrapping_sub(1);
        self.set_zn(self.x);
        0
    }

    /// DEY - Decrement Y
    pub(crate) fn dey(&mut self) -> u8 {
        self.y = self.y.wrapping_sub(1);
        self.set_zn(self.y);
        0
    }

    //
    // ========== LOGICAL INSTRUCTIONS ==========
    //

    /// AND - Logical AND
    pub(crate) fn and(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let (value, page_crossed) = self.read_operand(bus, mode);
        self.a &= value;
        self.set_zn(self.a);
        u8::from(page_crossed)
    }

    /// ORA - Logical OR
    pub(crate) fn ora(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let (value, page_crossed) = self.read_operand(bus, mode);
        self.a |= value;
        self.set_zn(self.a);
        u8::from(page_crossed)
    }

    /// EOR - Exclusive OR
    pub(crate) fn eor(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let (value, page_crossed) = self.read_operand(bus, mode);
        self.a ^= value;
        self.set_zn(self.a);
        u8::from(page_crossed)
    }

    /// BIT - Bit Test
    pub(crate) fn bit(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let (value, _) = self.read_operand(bus, mode);
        let result = self.a & value;

        self.status.set(StatusFlags::ZERO, result == 0);
        self.status.set(StatusFlags::NEGATIVE, value & 0x80 != 0);
        self.status.set(StatusFlags::OVERFLOW, value & 0x40 != 0);
        0
    }

    //
    // ========== SHIFT/ROTATE INSTRUCTIONS ==========
    //

    /// ASL - Arithmetic Shift Left (Accumulator)
    pub(crate) fn asl_acc(&mut self) -> u8 {
        self.status.set(StatusFlags::CARRY, self.a & 0x80 != 0);
        self.a <<= 1;
        self.set_zn(self.a);
        0
    }

    /// ASL - Arithmetic Shift Left (Memory)
    pub(crate) fn asl(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let result = mode.resolve(self.pc, self.x, self.y, bus);
        self.pc = self.pc.wrapping_add(u16::from(mode.operand_bytes()));

        let addr = result.addr;
        let value = bus.read(addr);
        bus.write(addr, value); // Dummy write
        self.status.set(StatusFlags::CARRY, value & 0x80 != 0);
        let new_value = value << 1;
        bus.write(addr, new_value);
        self.set_zn(new_value);
        0
    }

    /// LSR - Logical Shift Right (Accumulator)
    pub(crate) fn lsr_acc(&mut self) -> u8 {
        self.status.set(StatusFlags::CARRY, self.a & 0x01 != 0);
        self.a >>= 1;
        self.set_zn(self.a);
        0
    }

    /// LSR - Logical Shift Right (Memory)
    pub(crate) fn lsr(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let result = mode.resolve(self.pc, self.x, self.y, bus);
        self.pc = self.pc.wrapping_add(u16::from(mode.operand_bytes()));

        let addr = result.addr;
        let value = bus.read(addr);
        bus.write(addr, value); // Dummy write
        self.status.set(StatusFlags::CARRY, value & 0x01 != 0);
        let new_value = value >> 1;
        bus.write(addr, new_value);
        self.set_zn(new_value);
        0
    }

    /// ROL - Rotate Left (Accumulator)
    pub(crate) fn rol_acc(&mut self) -> u8 {
        let carry_in = u8::from(self.status.contains(StatusFlags::CARRY));
        self.status.set(StatusFlags::CARRY, self.a & 0x80 != 0);
        self.a = (self.a << 1) | carry_in;
        self.set_zn(self.a);
        0
    }

    /// ROL - Rotate Left (Memory)
    pub(crate) fn rol(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let result = mode.resolve(self.pc, self.x, self.y, bus);
        self.pc = self.pc.wrapping_add(u16::from(mode.operand_bytes()));

        let addr = result.addr;
        let value = bus.read(addr);
        bus.write(addr, value); // Dummy write
        let carry_in = u8::from(self.status.contains(StatusFlags::CARRY));
        self.status.set(StatusFlags::CARRY, value & 0x80 != 0);
        let new_value = (value << 1) | carry_in;
        bus.write(addr, new_value);
        self.set_zn(new_value);
        0
    }

    /// ROR - Rotate Right (Accumulator)
    pub(crate) fn ror_acc(&mut self) -> u8 {
        let carry_in = if self.status.contains(StatusFlags::CARRY) {
            0x80
        } else {
            0x00
        };
        self.status.set(StatusFlags::CARRY, self.a & 0x01 != 0);
        self.a = (self.a >> 1) | carry_in;
        self.set_zn(self.a);
        0
    }

    /// ROR - Rotate Right (Memory)
    pub(crate) fn ror(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let result = mode.resolve(self.pc, self.x, self.y, bus);
        self.pc = self.pc.wrapping_add(u16::from(mode.operand_bytes()));

        let addr = result.addr;
        let value = bus.read(addr);
        bus.write(addr, value); // Dummy write
        let carry_in = if self.status.contains(StatusFlags::CARRY) {
            0x80
        } else {
            0x00
        };
        self.status.set(StatusFlags::CARRY, value & 0x01 != 0);
        let new_value = (value >> 1) | carry_in;
        bus.write(addr, new_value);
        self.set_zn(new_value);
        0
    }

    //
    // ========== COMPARE INSTRUCTIONS ==========
    //

    /// CMP - Compare Accumulator
    pub(crate) fn cmp(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let (value, page_crossed) = self.read_operand(bus, mode);
        self.compare(self.a, value);
        u8::from(page_crossed)
    }

    /// CPX - Compare X Register
    pub(crate) fn cpx(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let (value, _) = self.read_operand(bus, mode);
        self.compare(self.x, value);
        0
    }

    /// CPY - Compare Y Register
    pub(crate) fn cpy(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let (value, _) = self.read_operand(bus, mode);
        self.compare(self.y, value);
        0
    }

    /// Compare helper function
    fn compare(&mut self, register: u8, value: u8) {
        let result = register.wrapping_sub(value);
        self.status.set(StatusFlags::CARRY, register >= value);
        self.set_zn(result);
    }

    //
    // ========== BRANCH INSTRUCTIONS ==========
    //

    /// BPL - Branch if Positive
    pub(crate) fn bpl(&mut self, bus: &mut impl Bus) -> u8 {
        self.branch(bus, !self.status.contains(StatusFlags::NEGATIVE))
    }

    /// BMI - Branch if Minus
    pub(crate) fn bmi(&mut self, bus: &mut impl Bus) -> u8 {
        self.branch(bus, self.status.contains(StatusFlags::NEGATIVE))
    }

    /// BVC - Branch if Overflow Clear
    pub(crate) fn bvc(&mut self, bus: &mut impl Bus) -> u8 {
        self.branch(bus, !self.status.contains(StatusFlags::OVERFLOW))
    }

    /// BVS - Branch if Overflow Set
    pub(crate) fn bvs(&mut self, bus: &mut impl Bus) -> u8 {
        self.branch(bus, self.status.contains(StatusFlags::OVERFLOW))
    }

    /// BCC - Branch if Carry Clear
    pub(crate) fn bcc(&mut self, bus: &mut impl Bus) -> u8 {
        self.branch(bus, !self.status.contains(StatusFlags::CARRY))
    }

    /// BCS - Branch if Carry Set
    pub(crate) fn bcs(&mut self, bus: &mut impl Bus) -> u8 {
        self.branch(bus, self.status.contains(StatusFlags::CARRY))
    }

    /// BNE - Branch if Not Equal
    pub(crate) fn bne(&mut self, bus: &mut impl Bus) -> u8 {
        self.branch(bus, !self.status.contains(StatusFlags::ZERO))
    }

    /// BEQ - Branch if Equal
    pub(crate) fn beq(&mut self, bus: &mut impl Bus) -> u8 {
        self.branch(bus, self.status.contains(StatusFlags::ZERO))
    }

    /// Branch helper function
    fn branch(&mut self, bus: &mut impl Bus, condition: bool) -> u8 {
        let offset = bus.read(self.pc) as i8;
        self.pc = self.pc.wrapping_add(1);

        if !condition {
            return 0; // Not taken: 0 extra cycles
        }

        let old_pc = self.pc;
        let new_pc = self.pc.wrapping_add_signed(i16::from(offset));
        self.pc = new_pc;

        // +1 cycle for branch taken
        // +1 more cycle if page crossed
        let page_crossed = (old_pc & 0xFF00) != (new_pc & 0xFF00);
        1 + u8::from(page_crossed)
    }

    //
    // ========== JUMP/SUBROUTINE INSTRUCTIONS ==========
    //

    /// JMP - Jump Absolute
    pub(crate) fn jmp_abs(&mut self, bus: &mut impl Bus) -> u8 {
        self.pc = bus.read_u16(self.pc);
        0
    }

    /// JMP - Jump Indirect (with page boundary bug)
    pub(crate) fn jmp_ind(&mut self, bus: &mut impl Bus) -> u8 {
        let ptr = bus.read_u16(self.pc);

        // 6502 bug: JMP ($xxFF) reads high byte from $xx00 instead of $xx+1 00
        let lo = bus.read(ptr);
        let hi_addr = if ptr & 0xFF == 0xFF {
            ptr & 0xFF00 // Wrap to start of same page
        } else {
            ptr + 1
        };
        let hi = bus.read(hi_addr);

        self.pc = u16::from_le_bytes([lo, hi]);
        0
    }

    /// JSR - Jump to Subroutine
    pub(crate) fn jsr(&mut self, bus: &mut impl Bus) -> u8 {
        let target = bus.read_u16(self.pc);
        self.pc = self.pc.wrapping_add(1); // JSR pushes PC-1
        self.push_u16(bus, self.pc);
        self.pc = target;
        0
    }

    /// RTS - Return from Subroutine
    pub(crate) fn rts(&mut self, bus: &mut impl Bus) -> u8 {
        self.pc = self.pop_u16(bus);
        self.pc = self.pc.wrapping_add(1);
        0
    }

    /// RTI - Return from Interrupt
    pub(crate) fn rti(&mut self, bus: &mut impl Bus) -> u8 {
        let p = self.pop(bus);
        self.status = StatusFlags::from_stack_byte(p);
        self.pc = self.pop_u16(bus);
        0
    }

    /// BRK - Force Interrupt
    pub(crate) fn brk(&mut self, bus: &mut impl Bus) -> u8 {
        self.pc = self.pc.wrapping_add(1); // BRK increments PC by 2 total
        self.push_u16(bus, self.pc);
        self.push(bus, self.status.to_stack_byte(true)); // B=1 for BRK
        self.status.insert(StatusFlags::INTERRUPT_DISABLE);
        self.pc = bus.read_u16(0xFFFE); // IRQ/BRK vector
        0
    }

    //
    // ========== FLAG INSTRUCTIONS ==========
    //

    /// CLC - Clear Carry
    pub(crate) fn clc(&mut self) -> u8 {
        self.status.remove(StatusFlags::CARRY);
        0
    }

    /// SEC - Set Carry
    pub(crate) fn sec(&mut self) -> u8 {
        self.status.insert(StatusFlags::CARRY);
        0
    }

    /// CLI - Clear Interrupt Disable
    pub(crate) fn cli(&mut self) -> u8 {
        self.status.remove(StatusFlags::INTERRUPT_DISABLE);
        0
    }

    /// SEI - Set Interrupt Disable
    pub(crate) fn sei(&mut self) -> u8 {
        self.status.insert(StatusFlags::INTERRUPT_DISABLE);
        0
    }

    /// CLV - Clear Overflow
    pub(crate) fn clv(&mut self) -> u8 {
        self.status.remove(StatusFlags::OVERFLOW);
        0
    }

    /// CLD - Clear Decimal (no effect on NES)
    pub(crate) fn cld(&mut self) -> u8 {
        self.status.remove(StatusFlags::DECIMAL);
        0
    }

    /// SED - Set Decimal (no effect on NES)
    pub(crate) fn sed(&mut self) -> u8 {
        self.status.insert(StatusFlags::DECIMAL);
        0
    }

    /// NOP - No Operation
    pub(crate) fn nop(&mut self) -> u8 {
        0
    }

    //
    // ========== UNOFFICIAL INSTRUCTIONS ==========
    //

    /// LAX - Load A and X
    pub(crate) fn lax(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let (value, page_crossed) = self.read_operand(bus, mode);
        self.a = value;
        self.x = value;
        self.set_zn(value);
        u8::from(page_crossed)
    }

    /// SAX - Store A AND X
    pub(crate) fn sax(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let value = self.a & self.x;
        self.write_operand(bus, mode, value);
        0
    }

    /// DCP - Decrement and Compare
    pub(crate) fn dcp(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let result = mode.resolve(self.pc, self.x, self.y, bus);
        self.pc = self.pc.wrapping_add(u16::from(mode.operand_bytes()));

        let addr = result.addr;
        let value = bus.read(addr);
        bus.write(addr, value); // Dummy write
        let new_value = value.wrapping_sub(1);
        bus.write(addr, new_value);
        self.compare(self.a, new_value);
        0
    }

    /// ISC - Increment and Subtract with Carry
    pub(crate) fn isc(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let result = mode.resolve(self.pc, self.x, self.y, bus);
        self.pc = self.pc.wrapping_add(u16::from(mode.operand_bytes()));

        let addr = result.addr;
        let value = bus.read(addr);
        bus.write(addr, value); // Dummy write
        let new_value = value.wrapping_add(1);
        bus.write(addr, new_value);
        self.sbc_impl(new_value);
        0
    }

    /// SLO - Shift Left and OR
    pub(crate) fn slo(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let result = mode.resolve(self.pc, self.x, self.y, bus);
        self.pc = self.pc.wrapping_add(u16::from(mode.operand_bytes()));

        let addr = result.addr;
        let value = bus.read(addr);
        bus.write(addr, value); // Dummy write
        self.status.set(StatusFlags::CARRY, value & 0x80 != 0);
        let shifted = value << 1;
        bus.write(addr, shifted);
        self.a |= shifted;
        self.set_zn(self.a);
        0
    }

    /// RLA - Rotate Left and AND
    pub(crate) fn rla(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let result = mode.resolve(self.pc, self.x, self.y, bus);
        self.pc = self.pc.wrapping_add(u16::from(mode.operand_bytes()));

        let addr = result.addr;
        let value = bus.read(addr);
        bus.write(addr, value); // Dummy write
        let carry_in = u8::from(self.status.contains(StatusFlags::CARRY));
        self.status.set(StatusFlags::CARRY, value & 0x80 != 0);
        let rotated = (value << 1) | carry_in;
        bus.write(addr, rotated);
        self.a &= rotated;
        self.set_zn(self.a);
        0
    }

    /// SRE - Shift Right and XOR
    pub(crate) fn sre(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let result = mode.resolve(self.pc, self.x, self.y, bus);
        self.pc = self.pc.wrapping_add(u16::from(mode.operand_bytes()));

        let addr = result.addr;
        let value = bus.read(addr);
        bus.write(addr, value); // Dummy write
        self.status.set(StatusFlags::CARRY, value & 0x01 != 0);
        let shifted = value >> 1;
        bus.write(addr, shifted);
        self.a ^= shifted;
        self.set_zn(self.a);
        0
    }

    /// RRA - Rotate Right and Add with Carry
    pub(crate) fn rra(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let result = mode.resolve(self.pc, self.x, self.y, bus);
        self.pc = self.pc.wrapping_add(u16::from(mode.operand_bytes()));

        let addr = result.addr;
        let value = bus.read(addr);
        bus.write(addr, value); // Dummy write
        let carry_in = if self.status.contains(StatusFlags::CARRY) {
            0x80
        } else {
            0x00
        };
        self.status.set(StatusFlags::CARRY, value & 0x01 != 0);
        let rotated = (value >> 1) | carry_in;
        bus.write(addr, rotated);
        self.adc_impl(rotated);
        0
    }

    /// ANC - AND with Carry
    pub(crate) fn anc(&mut self, bus: &mut impl Bus) -> u8 {
        let value = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        self.a &= value;
        self.set_zn(self.a);
        self.status.set(
            StatusFlags::CARRY,
            self.status.contains(StatusFlags::NEGATIVE),
        );
        0
    }

    /// ALR - AND then Logical Shift Right
    pub(crate) fn alr(&mut self, bus: &mut impl Bus) -> u8 {
        let value = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        self.a &= value;
        self.status.set(StatusFlags::CARRY, self.a & 0x01 != 0);
        self.a >>= 1;
        self.set_zn(self.a);
        0
    }

    /// ARR - AND then Rotate Right
    pub(crate) fn arr(&mut self, bus: &mut impl Bus) -> u8 {
        let value = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        self.a &= value;
        let carry_in = if self.status.contains(StatusFlags::CARRY) {
            0x80
        } else {
            0x00
        };
        self.a = (self.a >> 1) | carry_in;
        self.set_zn(self.a);

        // Complex flag behavior
        self.status.set(StatusFlags::CARRY, self.a & 0x40 != 0);
        self.status.set(
            StatusFlags::OVERFLOW,
            ((self.a >> 6) ^ (self.a >> 5)) & 1 != 0,
        );
        0
    }

    /// XAA - Unstable AND X with immediate
    pub(crate) fn xaa(&mut self, bus: &mut impl Bus) -> u8 {
        let value = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        // Magic constant: 0xEE is most common
        self.a = (self.a | 0xEE) & self.x & value;
        self.set_zn(self.a);
        0
    }

    /// LXA - Unstable Load A and X
    pub(crate) fn lxa(&mut self, bus: &mut impl Bus) -> u8 {
        let value = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        // Magic constant: 0xEE is most common
        self.a = (self.a | 0xEE) & value;
        self.x = self.a;
        self.set_zn(self.a);
        0
    }

    /// AXS - AND X with A, then subtract
    pub(crate) fn axs(&mut self, bus: &mut impl Bus) -> u8 {
        let value = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        let temp = self.a & self.x;
        let result = temp.wrapping_sub(value);
        self.status.set(StatusFlags::CARRY, temp >= value);
        self.x = result;
        self.set_zn(result);
        0
    }

    /// SHA - Store A AND X AND (H+1) [unstable]
    pub(crate) fn sha(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let result = mode.resolve(self.pc, self.x, self.y, bus);
        self.pc = self.pc.wrapping_add(u16::from(mode.operand_bytes()));

        let addr = result.addr;
        let addr_hi = ((addr >> 8) as u8).wrapping_add(1);
        let value = self.a & self.x & addr_hi;
        bus.write(addr, value);
        0
    }

    /// SHY - Store Y AND (H+1) [unstable]
    pub(crate) fn shy(&mut self, bus: &mut impl Bus) -> u8 {
        let lo = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        let hi = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);

        let base = u16::from_le_bytes([lo, hi]);
        let addr = base.wrapping_add(u16::from(self.x));
        let addr_hi = ((addr >> 8) as u8).wrapping_add(1);
        let value = self.y & addr_hi;
        bus.write(addr, value);
        0
    }

    /// SHX - Store X AND (H+1) [unstable]
    pub(crate) fn shx(&mut self, bus: &mut impl Bus) -> u8 {
        let lo = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        let hi = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);

        let base = u16::from_le_bytes([lo, hi]);
        let addr = base.wrapping_add(u16::from(self.y));
        let addr_hi = ((addr >> 8) as u8).wrapping_add(1);
        let value = self.x & addr_hi;
        bus.write(addr, value);
        0
    }

    /// TAS - Transfer A AND X to SP, store A AND X AND (H+1) [unstable]
    pub(crate) fn tas(&mut self, bus: &mut impl Bus) -> u8 {
        let lo = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        let hi = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);

        self.sp = self.a & self.x;
        let base = u16::from_le_bytes([lo, hi]);
        let addr = base.wrapping_add(u16::from(self.y));
        let addr_hi = ((addr >> 8) as u8).wrapping_add(1);
        let value = self.a & self.x & addr_hi;
        bus.write(addr, value);
        0
    }

    /// LAS - Load A, X, and SP [unstable]
    pub(crate) fn las(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let (value, page_crossed) = self.read_operand(bus, mode);
        let result = value & self.sp;
        self.a = result;
        self.x = result;
        self.sp = result;
        self.set_zn(result);
        u8::from(page_crossed)
    }

    /// NOP with read (for addressing modes that read memory)
    pub(crate) fn nop_read(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> u8 {
        let (_, page_crossed) = self.read_operand(bus, mode);
        u8::from(page_crossed)
    }

    /// JAM - Halt CPU
    pub(crate) fn jam(&mut self) -> u8 {
        self.jammed = true;
        self.pc = self.pc.wrapping_sub(1); // Stay on JAM instruction
        0
    }
}

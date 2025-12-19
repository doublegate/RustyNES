//! 6502 CPU core implementation.
//!
//! This module contains the main CPU structure with all registers,
//! the instruction execution loop, interrupt handling, and stack operations.

use crate::addressing::AddressingMode;
use crate::bus::Bus;
use crate::opcodes::OPCODE_TABLE;
use crate::status::StatusFlags;

/// NES 6502 CPU
///
/// Cycle-accurate implementation of the MOS 6502 as used in the NES.
/// All timing follows the NESdev Wiki specifications.
#[derive(Debug)]
pub struct Cpu {
    /// Accumulator register
    pub a: u8,
    /// X index register
    pub x: u8,
    /// Y index register
    pub y: u8,
    /// Program counter
    pub pc: u16,
    /// Stack pointer (points to $0100-$01FF)
    pub sp: u8,
    /// Status flags
    pub status: StatusFlags,
    /// Total cycles executed
    pub cycles: u64,
    /// Stall cycles (for DMA)
    pub stall: u8,
    /// NMI pending flag
    nmi_pending: bool,
    /// IRQ line state
    irq_pending: bool,
    /// CPU jammed (halt opcodes)
    pub(crate) jammed: bool,
}

impl Cpu {
    /// Create a new CPU in power-on state.
    ///
    /// # Power-on State
    /// - A, X, Y: undefined (set to 0)
    /// - SP: $FD (after RESET pulls 3 bytes)
    /// - P: $34 (IRQ disabled)
    /// - PC: Read from RESET vector $FFFC-$FFFD
    pub fn new() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            pc: 0,
            sp: 0xFD,
            status: StatusFlags::from_bits_truncate(0x34), // I flag set, U flag set
            cycles: 0,
            stall: 0,
            nmi_pending: false,
            irq_pending: false,
            jammed: false,
        }
    }

    /// Reset the CPU.
    ///
    /// Simulates the RESET interrupt sequence:
    /// - SP decremented by 3 (no writes)
    /// - I flag set
    /// - PC loaded from RESET vector ($FFFC-$FFFD)
    /// - Takes 7 cycles
    pub fn reset(&mut self, bus: &mut impl Bus) {
        self.sp = self.sp.wrapping_sub(3);
        self.status.insert(StatusFlags::INTERRUPT_DISABLE);
        self.pc = bus.read_u16(0xFFFC);
        self.cycles += 7;
        self.nmi_pending = false;
        self.irq_pending = false;
        self.jammed = false;
    }

    /// Execute one instruction and return cycles taken.
    ///
    /// Handles interrupt polling and instruction execution.
    /// Returns the number of CPU cycles consumed.
    pub fn step(&mut self, bus: &mut impl Bus) -> u8 {
        // Handle DMA stalls
        if self.stall > 0 {
            self.stall -= 1;
            self.cycles += 1;
            return 1;
        }

        // Check if CPU is jammed
        if self.jammed {
            self.cycles += 1;
            return 1;
        }

        // Handle interrupts (polled on last cycle of previous instruction)
        if self.nmi_pending {
            self.nmi_pending = false;
            return self.handle_nmi(bus);
        }

        if self.irq_pending && !self.status.contains(StatusFlags::INTERRUPT_DISABLE) {
            return self.handle_irq(bus);
        }

        // Fetch opcode
        let opcode = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);

        // Look up opcode info
        let info = &OPCODE_TABLE[opcode as usize];

        // Execute instruction
        let extra_cycles = self.execute_opcode(opcode, info.addr_mode, bus);

        // Calculate total cycles
        let total_cycles = info.cycles + extra_cycles;
        self.cycles += u64::from(total_cycles);

        total_cycles
    }

    /// Trigger NMI (Non-Maskable Interrupt).
    ///
    /// NMI is edge-triggered - call this when NMI line transitions from high to low.
    pub fn trigger_nmi(&mut self) {
        self.nmi_pending = true;
    }

    /// Set IRQ line state.
    ///
    /// IRQ is level-triggered - will fire every instruction while line is low and I=0.
    pub fn set_irq(&mut self, active: bool) {
        self.irq_pending = active;
    }

    /// Get total cycles executed.
    pub fn get_cycles(&self) -> u64 {
        self.cycles
    }

    /// Check if CPU is jammed (halted).
    pub fn is_jammed(&self) -> bool {
        self.jammed
    }

    /// Handle NMI interrupt (7 cycles).
    fn handle_nmi(&mut self, bus: &mut impl Bus) -> u8 {
        self.push_u16(bus, self.pc);
        self.push(bus, self.status.to_stack_byte(false)); // B=0 for interrupts
        self.status.insert(StatusFlags::INTERRUPT_DISABLE);
        self.pc = bus.read_u16(0xFFFA); // NMI vector
        7
    }

    /// Handle IRQ interrupt (7 cycles).
    fn handle_irq(&mut self, bus: &mut impl Bus) -> u8 {
        self.push_u16(bus, self.pc);
        self.push(bus, self.status.to_stack_byte(false)); // B=0 for interrupts
        self.status.insert(StatusFlags::INTERRUPT_DISABLE);
        self.pc = bus.read_u16(0xFFFE); // IRQ vector
        7
    }

    /// Execute a single opcode.
    ///
    /// Returns extra cycles taken (for page crossing, branches, etc.).
    fn execute_opcode(&mut self, opcode: u8, addr_mode: AddressingMode, bus: &mut impl Bus) -> u8 {
        match opcode {
            // Load/Store
            0xA9 => self.lda(bus, addr_mode),
            0xA5 | 0xB5 | 0xAD | 0xBD | 0xB9 | 0xA1 | 0xB1 => self.lda(bus, addr_mode),
            0xA2 => self.ldx(bus, addr_mode),
            0xA6 | 0xB6 | 0xAE | 0xBE => self.ldx(bus, addr_mode),
            0xA0 => self.ldy(bus, addr_mode),
            0xA4 | 0xB4 | 0xAC | 0xBC => self.ldy(bus, addr_mode),
            0x85 | 0x95 | 0x8D | 0x9D | 0x99 | 0x81 | 0x91 => self.sta(bus, addr_mode),
            0x86 | 0x96 | 0x8E => self.stx(bus, addr_mode),
            0x84 | 0x94 | 0x8C => self.sty(bus, addr_mode),

            // Transfer
            0xAA => self.tax(),
            0xA8 => self.tay(),
            0x8A => self.txa(),
            0x98 => self.tya(),
            0xBA => self.tsx(),
            0x9A => self.txs(),

            // Stack
            0x48 => self.pha(bus),
            0x08 => self.php(bus),
            0x68 => self.pla(bus),
            0x28 => self.plp(bus),

            // Arithmetic
            0x69 | 0x65 | 0x75 | 0x6D | 0x7D | 0x79 | 0x61 | 0x71 => self.adc(bus, addr_mode),
            0xE9 | 0xE5 | 0xF5 | 0xED | 0xFD | 0xF9 | 0xE1 | 0xF1 | 0xEB => {
                self.sbc(bus, addr_mode)
            }

            // Increment/Decrement
            0xE6 | 0xF6 | 0xEE | 0xFE => self.inc(bus, addr_mode),
            0xC6 | 0xD6 | 0xCE | 0xDE => self.dec(bus, addr_mode),
            0xE8 => self.inx(),
            0xC8 => self.iny(),
            0xCA => self.dex(),
            0x88 => self.dey(),

            // Logic
            0x29 | 0x25 | 0x35 | 0x2D | 0x3D | 0x39 | 0x21 | 0x31 => self.and(bus, addr_mode),
            0x09 | 0x05 | 0x15 | 0x0D | 0x1D | 0x19 | 0x01 | 0x11 => self.ora(bus, addr_mode),
            0x49 | 0x45 | 0x55 | 0x4D | 0x5D | 0x59 | 0x41 | 0x51 => self.eor(bus, addr_mode),
            0x24 | 0x2C => self.bit(bus, addr_mode),

            // Shift/Rotate
            0x0A => self.asl_acc(),
            0x06 | 0x16 | 0x0E | 0x1E => self.asl(bus, addr_mode),
            0x4A => self.lsr_acc(),
            0x46 | 0x56 | 0x4E | 0x5E => self.lsr(bus, addr_mode),
            0x2A => self.rol_acc(),
            0x26 | 0x36 | 0x2E | 0x3E => self.rol(bus, addr_mode),
            0x6A => self.ror_acc(),
            0x66 | 0x76 | 0x6E | 0x7E => self.ror(bus, addr_mode),

            // Compare
            0xC9 | 0xC5 | 0xD5 | 0xCD | 0xDD | 0xD9 | 0xC1 | 0xD1 => self.cmp(bus, addr_mode),
            0xE0 | 0xE4 | 0xEC => self.cpx(bus, addr_mode),
            0xC0 | 0xC4 | 0xCC => self.cpy(bus, addr_mode),

            // Branch
            0x10 => self.bpl(bus),
            0x30 => self.bmi(bus),
            0x50 => self.bvc(bus),
            0x70 => self.bvs(bus),
            0x90 => self.bcc(bus),
            0xB0 => self.bcs(bus),
            0xD0 => self.bne(bus),
            0xF0 => self.beq(bus),

            // Jump/Subroutine
            0x4C => self.jmp_abs(bus),
            0x6C => self.jmp_ind(bus),
            0x20 => self.jsr(bus),
            0x60 => self.rts(bus),
            0x40 => self.rti(bus),
            0x00 => self.brk(bus),

            // Flags
            0x18 => self.clc(),
            0x38 => self.sec(),
            0x58 => self.cli(),
            0x78 => self.sei(),
            0xB8 => self.clv(),
            0xD8 => self.cld(),
            0xF8 => self.sed(),
            0xEA => self.nop(),

            // Unofficial opcodes
            0xA7 | 0xB7 | 0xAF | 0xBF | 0xA3 | 0xB3 => self.lax(bus, addr_mode),
            0x87 | 0x97 | 0x8F | 0x83 => self.sax(bus, addr_mode),
            0xC7 | 0xD7 | 0xCF | 0xDF | 0xDB | 0xC3 | 0xD3 => self.dcp(bus, addr_mode),
            0xE7 | 0xF7 | 0xEF | 0xFF | 0xFB | 0xE3 | 0xF3 => self.isc(bus, addr_mode),
            0x07 | 0x17 | 0x0F | 0x1F | 0x1B | 0x03 | 0x13 => self.slo(bus, addr_mode),
            0x27 | 0x37 | 0x2F | 0x3F | 0x3B | 0x23 | 0x33 => self.rla(bus, addr_mode),
            0x47 | 0x57 | 0x4F | 0x5F | 0x5B | 0x43 | 0x53 => self.sre(bus, addr_mode),
            0x67 | 0x77 | 0x6F | 0x7F | 0x7B | 0x63 | 0x73 => self.rra(bus, addr_mode),
            0x0B | 0x2B => self.anc(bus),
            0x4B => self.alr(bus),
            0x6B => self.arr(bus),
            0x8B => self.xaa(bus),
            0xAB => self.lxa(bus),
            0xCB => self.axs(bus),
            0x93 | 0x9F => self.sha(bus, addr_mode),
            0x9C => self.shy(bus),
            0x9E => self.shx(bus),
            0x9B => self.tas(bus),
            0xBB => self.las(bus, addr_mode),

            // Unofficial NOPs
            0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => self.nop(),
            0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 => self.nop_read(bus, addr_mode),
            0x04 | 0x44 | 0x64 | 0x14 | 0x34 | 0x54 | 0x74 | 0xD4 | 0xF4 => {
                self.nop_read(bus, addr_mode)
            }
            0x0C | 0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => self.nop_read(bus, addr_mode),

            // JAM/KIL opcodes - halt CPU
            0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 | 0x62 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2 => {
                self.jam()
            }
        }
    }

    /// Push byte to stack.
    pub(crate) fn push(&mut self, bus: &mut impl Bus, value: u8) {
        bus.write(0x0100 | u16::from(self.sp), value);
        self.sp = self.sp.wrapping_sub(1);
    }

    /// Pop byte from stack.
    pub(crate) fn pop(&mut self, bus: &mut impl Bus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        bus.read(0x0100 | u16::from(self.sp))
    }

    /// Push 16-bit value to stack (high byte first).
    pub(crate) fn push_u16(&mut self, bus: &mut impl Bus, value: u16) {
        self.push(bus, (value >> 8) as u8);
        self.push(bus, (value & 0xFF) as u8);
    }

    /// Pop 16-bit value from stack (low byte first).
    pub(crate) fn pop_u16(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.pop(bus);
        let hi = self.pop(bus);
        u16::from_le_bytes([lo, hi])
    }

    /// Read operand based on addressing mode.
    pub(crate) fn read_operand(&mut self, bus: &mut impl Bus, mode: AddressingMode) -> (u8, bool) {
        let result = mode.resolve(self.pc, self.x, self.y, bus);
        self.pc = self.pc.wrapping_add(u16::from(mode.operand_bytes()));

        let value = match mode {
            AddressingMode::Accumulator => self.a,
            _ => bus.read(result.addr),
        };

        (value, result.page_crossed)
    }

    /// Write to address from addressing mode.
    pub(crate) fn write_operand(&mut self, bus: &mut impl Bus, mode: AddressingMode, value: u8) {
        let result = mode.resolve(self.pc, self.x, self.y, bus);
        self.pc = self.pc.wrapping_add(u16::from(mode.operand_bytes()));

        match mode {
            AddressingMode::Accumulator => self.a = value,
            _ => bus.write(result.addr, value),
        }
    }

    /// Set Zero and Negative flags based on value.
    pub(crate) fn set_zn(&mut self, value: u8) {
        self.status.set_zn(value);
    }
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestBus {
        memory: [u8; 0x10000],
    }

    impl TestBus {
        fn new() -> Self {
            Self {
                memory: [0; 0x10000],
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
    fn test_cpu_new() {
        let cpu = Cpu::new();
        assert_eq!(cpu.a, 0);
        assert_eq!(cpu.x, 0);
        assert_eq!(cpu.y, 0);
        assert_eq!(cpu.sp, 0xFD);
        assert!(cpu.status.contains(StatusFlags::INTERRUPT_DISABLE));
    }

    #[test]
    fn test_cpu_reset() {
        let mut cpu = Cpu::new();
        let mut bus = TestBus::new();

        // Set RESET vector
        bus.write(0xFFFC, 0x00);
        bus.write(0xFFFD, 0x80);

        cpu.reset(&mut bus);

        assert_eq!(cpu.pc, 0x8000);
        assert!(cpu.status.contains(StatusFlags::INTERRUPT_DISABLE));
        assert_eq!(cpu.cycles, 7);
    }

    #[test]
    fn test_stack_operations() {
        let mut cpu = Cpu::new();
        let mut bus = TestBus::new();

        cpu.sp = 0xFF;

        // Push byte
        cpu.push(&mut bus, 0x42);
        assert_eq!(cpu.sp, 0xFE);
        assert_eq!(bus.read(0x01FF), 0x42);

        // Pop byte
        let value = cpu.pop(&mut bus);
        assert_eq!(value, 0x42);
        assert_eq!(cpu.sp, 0xFF);

        // Push/pop u16
        cpu.push_u16(&mut bus, 0x1234);
        assert_eq!(cpu.sp, 0xFD);
        let value = cpu.pop_u16(&mut bus);
        assert_eq!(value, 0x1234);
        assert_eq!(cpu.sp, 0xFF);
    }
}

//! 6502 CPU core implementation.
//!
//! This module contains the main CPU structure with all registers,
//! the instruction execution loop, interrupt handling, and stack operations.

use crate::addressing::AddressingMode;
use crate::bus::Bus;
use crate::opcodes::OPCODE_TABLE;
use crate::state::{CpuState, InstructionType};
use crate::status::StatusFlags;

/// NES 6502 CPU
///
/// Cycle-accurate implementation of the MOS 6502 as used in the NES.
/// All timing follows the NESdev Wiki specifications.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)] // Bools are appropriate for CPU flags
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
    pub(crate) nmi_pending: bool,
    /// IRQ line state
    pub(crate) irq_pending: bool,
    /// I flag value sampled at start of instruction (for interrupt polling)
    /// IRQ check uses this instead of current I flag to implement proper timing
    pub(crate) prev_irq_inhibit: bool,
    /// Suppress NMI check for one instruction (set after BRK completes)
    /// This ensures the first instruction of the interrupt handler executes
    /// before checking for another NMI (required for nmi_and_brk test)
    pub(crate) suppress_nmi_next: bool,
    /// CPU jammed (halt opcodes)
    pub jammed: bool,

    // ===== Cycle-by-cycle state machine fields =====
    /// Current execution state in the state machine
    state: CpuState,
    /// Current opcode being executed
    current_opcode: u8,
    /// Current instruction type (for dispatch)
    instr_type: InstructionType,
    /// Current addressing mode
    current_addr_mode: AddressingMode,
    /// Low byte of operand (fetched during FetchOperandLo)
    operand_lo: u8,
    /// High byte of operand (fetched during FetchOperandHi)
    operand_hi: u8,
    /// Calculated effective address
    effective_addr: u16,
    /// Base address before indexing (for page cross detection)
    base_addr: u16,
    /// Temporary value for RMW operations
    temp_value: u8,
    /// Branch offset (signed, for branch instructions)
    branch_offset: i8,
    /// Indicates if current instruction crosses a page boundary
    page_crossed: bool,
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
            status: StatusFlags::from_bits_truncate(0x24), // I flag set, U flag set
            cycles: 0,
            stall: 0,
            nmi_pending: false,
            irq_pending: false,
            prev_irq_inhibit: true,
            suppress_nmi_next: false,
            jammed: false,
            // Cycle-by-cycle state machine fields
            state: CpuState::default(),
            current_opcode: 0,
            instr_type: InstructionType::default(),
            current_addr_mode: AddressingMode::Implied,
            operand_lo: 0,
            operand_hi: 0,
            effective_addr: 0,
            base_addr: 0,
            temp_value: 0,
            branch_offset: 0,
            page_crossed: false,
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
        self.prev_irq_inhibit = true;
        self.jammed = false;
        // Reset state machine to ready for next instruction
        self.state = CpuState::FetchOpcode;
        self.current_opcode = 0;
        self.instr_type = InstructionType::default();
        self.current_addr_mode = AddressingMode::Implied;
        self.operand_lo = 0;
        self.operand_hi = 0;
        self.effective_addr = 0;
        self.base_addr = 0;
        self.temp_value = 0;
        self.branch_offset = 0;
        self.page_crossed = false;
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

        // Sample I flag at start of this instruction (for next instruction's IRQ check)
        let current_irq_inhibit = self.status.contains(StatusFlags::INTERRUPT_DISABLE);

        // Check for NMI (Non-Maskable Interrupt) - Edge triggered
        // NMI can be suppressed for one instruction after BRK completes
        if self.nmi_pending && !self.suppress_nmi_next {
            self.nmi_pending = false;
            // NMI sets I flag, so we must treat previous as inhibited to prevent immediate IRQ
            self.prev_irq_inhibit = true;
            return self.handle_nmi(bus);
        }

        // Clear NMI suppression flag (applies for one instruction only)
        if self.suppress_nmi_next {
            self.suppress_nmi_next = false;
        }

        // Check for IRQ (Maskable Interrupt) - Level triggered
        // IRQ is ignored if I flag is set (Interrupt Disable).
        // The check uses `prev_irq_inhibit` to model the 1-instruction latency
        // of instructions that change the I flag (CLI, SEI, PLP, RTI).
        if self.irq_pending && !self.prev_irq_inhibit {
            // Entering ISR sets I flag, so we must treat previous as inhibited
            self.prev_irq_inhibit = true;
            return self.handle_irq(bus);
        }

        // Update prev_irq_inhibit for next instruction
        self.prev_irq_inhibit = current_irq_inhibit;

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

    /// Check if IRQ is pending.
    #[must_use]
    pub fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    /// Get total cycles executed.
    pub fn get_cycles(&self) -> u64 {
        self.cycles
    }

    /// Check if CPU is jammed (halted).
    pub fn is_jammed(&self) -> bool {
        self.jammed
    }

    /// Get current CPU state (for debugging/testing).
    pub fn get_state(&self) -> CpuState {
        self.state
    }

    // =========================================================================
    // CYCLE-ACCURATE EXECUTION
    // =========================================================================

    /// Execute exactly one CPU cycle.
    ///
    /// This is the core of cycle-accurate emulation. Each call advances the CPU
    /// by exactly one cycle, enabling perfect PPU/APU synchronization.
    ///
    /// Returns `true` when an instruction boundary is reached (ready for next instruction).
    pub fn tick(&mut self, bus: &mut impl Bus) -> bool {
        // Handle DMA stalls (OAM DMA, DMC DMA)
        if self.stall > 0 {
            self.stall -= 1;
            self.cycles += 1;
            return false;
        }

        // Handle jammed CPU
        if self.jammed {
            self.cycles += 1;
            return false;
        }

        self.cycles += 1;

        // Dispatch based on current state
        match self.state {
            CpuState::FetchOpcode => self.tick_fetch_opcode(bus),
            CpuState::FetchOperandLo => self.tick_fetch_operand_lo(bus),
            CpuState::FetchOperandHi => self.tick_fetch_operand_hi(bus),
            CpuState::ResolveAddress => self.tick_resolve_address(bus),
            CpuState::ReadData => self.tick_read_data(bus),
            CpuState::WriteData => self.tick_write_data(bus),
            CpuState::RmwRead => self.tick_rmw_read(bus),
            CpuState::RmwDummyWrite => self.tick_rmw_dummy_write(bus),
            CpuState::RmwWrite => self.tick_rmw_write(bus),
            CpuState::Execute => self.tick_execute(bus),
            CpuState::FetchIndirectLo => self.tick_fetch_indirect_lo(bus),
            CpuState::FetchIndirectHi => self.tick_fetch_indirect_hi(bus),
            CpuState::AddIndex => self.tick_add_index(bus),
            CpuState::PushHi => self.tick_push_hi(bus),
            CpuState::PushLo => self.tick_push_lo(bus),
            CpuState::PushStatus => self.tick_push_status(bus),
            CpuState::PopLo => self.tick_pop_lo(bus),
            CpuState::PopHi => self.tick_pop_hi(bus),
            CpuState::PopStatus => self.tick_pop_status(bus),
            CpuState::InternalCycle => self.tick_internal_cycle(bus),
            CpuState::BranchTaken => self.tick_branch_taken(bus),
            CpuState::BranchPageCross => self.tick_branch_page_cross(bus),
            CpuState::InterruptPushPcHi => self.tick_interrupt_push_pc_hi(bus),
            CpuState::InterruptPushPcLo => self.tick_interrupt_push_pc_lo(bus),
            CpuState::InterruptPushStatus => self.tick_interrupt_push_status(bus),
            CpuState::InterruptFetchVectorLo => self.tick_interrupt_fetch_vector_lo(bus),
            CpuState::InterruptFetchVectorHi => self.tick_interrupt_fetch_vector_hi(bus),
        }
    }

    /// Fetch opcode cycle (cycle 1 of every instruction).
    fn tick_fetch_opcode(&mut self, bus: &mut impl Bus) -> bool {
        // Sample I flag at start of this instruction (will be used for NEXT instruction's IRQ check)
        let current_irq_inhibit = self.status.contains(StatusFlags::INTERRUPT_DISABLE);

        // Check for pending interrupts (polled on last cycle of previous instruction)
        // NMI is not affected by I flag, but can be suppressed for one instruction after BRK
        if self.nmi_pending && !self.suppress_nmi_next {
            self.nmi_pending = false;
            self.prev_irq_inhibit = current_irq_inhibit;
            // Start interrupt sequence - dummy read of current PC
            let _ = bus.read(self.pc);
            self.state = CpuState::InterruptPushPcHi;
            // Store NMI vector address for later
            self.effective_addr = 0xFFFA;
            return false;
        }

        // Clear NMI suppression flag (applies for one instruction only)
        if self.suppress_nmi_next {
            self.suppress_nmi_next = false;
        }

        // IRQ uses the I flag from the PREVIOUS instruction (prev_irq_inhibit)
        // This implements the one-instruction delay after CLI/PLP/RTI
        if self.irq_pending && !self.prev_irq_inhibit {
            self.prev_irq_inhibit = current_irq_inhibit;
            // Start interrupt sequence - dummy read of current PC
            let _ = bus.read(self.pc);
            self.state = CpuState::InterruptPushPcHi;
            // Store IRQ vector address for later
            self.effective_addr = 0xFFFE;
            return false;
        }

        // Update prev_irq_inhibit for next instruction
        self.prev_irq_inhibit = current_irq_inhibit;

        // Fetch opcode from PC
        self.current_opcode = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);

        // Look up opcode info
        let info = &OPCODE_TABLE[self.current_opcode as usize];
        self.current_addr_mode = info.addr_mode;
        self.instr_type = InstructionType::from_opcode(self.current_opcode);

        // Reset state for new instruction
        self.operand_lo = 0;
        self.operand_hi = 0;
        self.effective_addr = 0;
        self.base_addr = 0;
        self.temp_value = 0;
        self.branch_offset = 0;
        self.page_crossed = false;

        // Determine next state based on addressing mode and instruction type
        self.state = self.next_state_after_fetch();

        // Check if this is a 2-cycle implied/accumulator instruction
        matches!(
            self.instr_type,
            InstructionType::Implied | InstructionType::Accumulator
        ) && self.state == CpuState::Execute
    }

    /// Determine next state after opcode fetch based on addressing mode.
    fn next_state_after_fetch(&self) -> CpuState {
        match self.current_addr_mode {
            // Implied and Accumulator: just need Execute cycle
            AddressingMode::Implied | AddressingMode::Accumulator => CpuState::Execute,

            // Immediate: fetch single byte operand
            AddressingMode::Immediate => CpuState::FetchOperandLo,

            // Zero Page: fetch single byte address
            AddressingMode::ZeroPage | AddressingMode::ZeroPageX | AddressingMode::ZeroPageY => {
                CpuState::FetchOperandLo
            }

            // Absolute: fetch two byte address
            AddressingMode::Absolute | AddressingMode::AbsoluteX | AddressingMode::AbsoluteY => {
                CpuState::FetchOperandLo
            }

            // Indirect (JMP only): fetch two byte pointer
            AddressingMode::Indirect => CpuState::FetchOperandLo,

            // Indexed Indirect (X): fetch zero page base
            AddressingMode::IndexedIndirectX => CpuState::FetchOperandLo,

            // Indirect Indexed (Y): fetch zero page pointer
            AddressingMode::IndirectIndexedY => CpuState::FetchOperandLo,

            // Relative (branches): fetch offset
            AddressingMode::Relative => CpuState::FetchOperandLo,
        }
    }

    // =========================================================================
    // STATE HANDLERS - These will be implemented progressively
    // =========================================================================

    fn tick_fetch_operand_lo(&mut self, bus: &mut impl Bus) -> bool {
        self.operand_lo = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);

        match self.current_addr_mode {
            // Immediate mode: operand is the value itself
            AddressingMode::Immediate => {
                self.effective_addr = self.pc.wrapping_sub(1);
                self.temp_value = self.operand_lo;
                self.state = self.next_state_for_instruction_type();
            }

            // Zero Page modes
            AddressingMode::ZeroPage => {
                self.effective_addr = u16::from(self.operand_lo);
                self.state = self.next_state_for_instruction_type();
            }
            AddressingMode::ZeroPageX => {
                self.base_addr = u16::from(self.operand_lo);
                self.state = CpuState::AddIndex;
            }
            AddressingMode::ZeroPageY => {
                self.base_addr = u16::from(self.operand_lo);
                self.state = CpuState::AddIndex;
            }

            // Absolute modes: need high byte
            AddressingMode::Absolute
            | AddressingMode::AbsoluteX
            | AddressingMode::AbsoluteY
            | AddressingMode::Indirect => {
                self.state = CpuState::FetchOperandHi;
            }

            // Indexed Indirect (X): fetch from zero page
            AddressingMode::IndexedIndirectX => {
                self.base_addr = u16::from(self.operand_lo);
                self.state = CpuState::AddIndex;
            }

            // Indirect Indexed (Y): fetch low byte of pointer
            AddressingMode::IndirectIndexedY => {
                self.base_addr = u16::from(self.operand_lo);
                self.state = CpuState::FetchIndirectLo;
            }

            // Relative (branches): operand is signed offset
            AddressingMode::Relative => {
                self.branch_offset = self.operand_lo as i8;
                // Check branch condition
                if self.check_branch_condition() {
                    self.state = CpuState::BranchTaken;
                } else {
                    // Branch not taken - instruction complete
                    self.state = CpuState::FetchOpcode;
                    return true;
                }
            }

            _ => {
                self.state = CpuState::FetchOpcode;
            }
        }
        false
    }

    fn tick_fetch_operand_hi(&mut self, bus: &mut impl Bus) -> bool {
        self.operand_hi = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);

        let addr = u16::from_le_bytes([self.operand_lo, self.operand_hi]);

        match self.current_addr_mode {
            AddressingMode::Absolute => {
                self.effective_addr = addr;
                match self.instr_type {
                    InstructionType::JumpAbsolute => {
                        // JMP absolute: set PC and done
                        self.pc = self.effective_addr;
                        self.state = CpuState::FetchOpcode;
                        return true;
                    }
                    InstructionType::JumpSubroutine => {
                        // JSR: internal cycle, then push return address
                        self.state = CpuState::InternalCycle;
                    }
                    _ => {
                        self.state = self.next_state_for_instruction_type();
                    }
                }
            }
            AddressingMode::AbsoluteX => {
                self.base_addr = addr;
                let indexed = addr.wrapping_add(u16::from(self.x));
                self.effective_addr = indexed;
                self.page_crossed = (addr & 0xFF00) != (indexed & 0xFF00);

                // For writes and RMW: always do dummy read (ResolveAddress)
                // For reads: only if page crossed
                match self.instr_type {
                    InstructionType::Write | InstructionType::ReadModifyWrite => {
                        self.state = CpuState::ResolveAddress;
                    }
                    _ => {
                        if self.page_crossed {
                            self.state = CpuState::ResolveAddress;
                        } else {
                            self.state = self.next_state_for_instruction_type();
                        }
                    }
                }
            }
            AddressingMode::AbsoluteY => {
                self.base_addr = addr;
                let indexed = addr.wrapping_add(u16::from(self.y));
                self.effective_addr = indexed;
                self.page_crossed = (addr & 0xFF00) != (indexed & 0xFF00);

                match self.instr_type {
                    InstructionType::Write | InstructionType::ReadModifyWrite => {
                        self.state = CpuState::ResolveAddress;
                    }
                    _ => {
                        if self.page_crossed {
                            self.state = CpuState::ResolveAddress;
                        } else {
                            self.state = self.next_state_for_instruction_type();
                        }
                    }
                }
            }
            AddressingMode::Indirect => {
                // JMP indirect: fetch low byte of target address
                self.base_addr = addr;
                self.state = CpuState::FetchIndirectLo;
            }
            _ => {
                self.state = CpuState::FetchOpcode;
            }
        }
        false
    }

    fn tick_resolve_address(&mut self, bus: &mut impl Bus) -> bool {
        // Dummy read from incorrect address (before page fix)
        // This is the hardware behavior for indexed addressing
        let incorrect_addr = (self.base_addr & 0xFF00) | (self.effective_addr & 0x00FF);
        let _ = bus.read(incorrect_addr);

        self.state = self.next_state_for_instruction_type();
        false
    }

    fn tick_read_data(&mut self, bus: &mut impl Bus) -> bool {
        self.temp_value = bus.read(self.effective_addr);
        self.state = CpuState::Execute;
        false
    }

    fn tick_write_data(&mut self, bus: &mut impl Bus) -> bool {
        // Execute the write instruction
        let value = self.execute_write_instruction();
        bus.write(self.effective_addr, value);
        self.state = CpuState::FetchOpcode;
        true
    }

    fn tick_rmw_read(&mut self, bus: &mut impl Bus) -> bool {
        self.temp_value = bus.read(self.effective_addr);
        self.state = CpuState::RmwDummyWrite;
        false
    }

    fn tick_rmw_dummy_write(&mut self, bus: &mut impl Bus) -> bool {
        // Write back the original value (hardware behavior)
        bus.write(self.effective_addr, self.temp_value);
        self.state = CpuState::RmwWrite;
        false
    }

    fn tick_rmw_write(&mut self, bus: &mut impl Bus) -> bool {
        // Execute the RMW operation and write result
        let result = self.execute_rmw_instruction();
        bus.write(self.effective_addr, result);
        self.state = CpuState::FetchOpcode;
        true
    }

    fn tick_execute(&mut self, bus: &mut impl Bus) -> bool {
        // Execute the instruction logic (for implied/accumulator or after read)
        match self.instr_type {
            InstructionType::Implied => {
                // Dummy read of next byte
                let _ = bus.read(self.pc);
                self.execute_implied_instruction();
            }
            InstructionType::Accumulator => {
                let _ = bus.read(self.pc);
                self.execute_accumulator_instruction();
            }
            InstructionType::Read => {
                self.execute_read_instruction();
            }
            _ => {}
        }
        self.state = CpuState::FetchOpcode;
        true
    }

    fn tick_fetch_indirect_lo(&mut self, bus: &mut impl Bus) -> bool {
        match self.current_addr_mode {
            AddressingMode::IndirectIndexedY => {
                // Read low byte of pointer from zero page
                self.operand_lo = bus.read(self.base_addr);
                self.state = CpuState::FetchIndirectHi;
            }
            AddressingMode::Indirect => {
                // JMP indirect: read low byte of target
                self.operand_lo = bus.read(self.base_addr);
                self.state = CpuState::FetchIndirectHi;
            }
            AddressingMode::IndexedIndirectX => {
                // Read low byte from (base + X) in zero page
                let ptr = self.effective_addr as u8;
                self.operand_lo = bus.read(u16::from(ptr));
                self.state = CpuState::FetchIndirectHi;
            }
            _ => {
                self.state = CpuState::FetchOpcode;
            }
        }
        false
    }

    fn tick_fetch_indirect_hi(&mut self, bus: &mut impl Bus) -> bool {
        match self.current_addr_mode {
            AddressingMode::IndirectIndexedY => {
                // Read high byte from (base + 1) with zero page wrap
                let ptr_hi = self.base_addr.wrapping_add(1) as u8;
                self.operand_hi = bus.read(u16::from(ptr_hi));

                let ptr_addr = u16::from_le_bytes([self.operand_lo, self.operand_hi]);
                let indexed = ptr_addr.wrapping_add(u16::from(self.y));
                self.base_addr = ptr_addr;
                self.effective_addr = indexed;
                self.page_crossed = (ptr_addr & 0xFF00) != (indexed & 0xFF00);

                match self.instr_type {
                    InstructionType::Write | InstructionType::ReadModifyWrite => {
                        self.state = CpuState::ResolveAddress;
                    }
                    _ => {
                        if self.page_crossed {
                            self.state = CpuState::ResolveAddress;
                        } else {
                            self.state = self.next_state_for_instruction_type();
                        }
                    }
                }
            }
            AddressingMode::Indirect => {
                // JMP indirect: read high byte with page wrap bug
                let ptr_lo = self.base_addr as u8;
                let ptr_hi_addr = (self.base_addr & 0xFF00) | u16::from(ptr_lo.wrapping_add(1));
                self.operand_hi = bus.read(ptr_hi_addr);

                self.effective_addr = u16::from_le_bytes([self.operand_lo, self.operand_hi]);
                self.pc = self.effective_addr;
                self.state = CpuState::FetchOpcode;
                return true;
            }
            AddressingMode::IndexedIndirectX => {
                // Read high byte from (base + X + 1) with zero page wrap
                let ptr = (self.effective_addr as u8).wrapping_add(1);
                self.operand_hi = bus.read(u16::from(ptr));
                self.effective_addr = u16::from_le_bytes([self.operand_lo, self.operand_hi]);
                self.state = self.next_state_for_instruction_type();
            }
            _ => {
                self.state = CpuState::FetchOpcode;
            }
        }
        false
    }

    fn tick_add_index(&mut self, bus: &mut impl Bus) -> bool {
        // Dummy read from base address
        let _ = bus.read(self.base_addr);

        match self.current_addr_mode {
            AddressingMode::ZeroPageX => {
                self.effective_addr = u16::from((self.base_addr as u8).wrapping_add(self.x));
                self.state = self.next_state_for_instruction_type();
            }
            AddressingMode::ZeroPageY => {
                self.effective_addr = u16::from((self.base_addr as u8).wrapping_add(self.y));
                self.state = self.next_state_for_instruction_type();
            }
            AddressingMode::IndexedIndirectX => {
                // Calculate pointer address with wrap
                self.effective_addr = u16::from((self.base_addr as u8).wrapping_add(self.x));
                self.state = CpuState::FetchIndirectLo;
            }
            _ => {
                self.state = CpuState::FetchOpcode;
            }
        }
        false
    }

    fn tick_push_hi(&mut self, bus: &mut impl Bus) -> bool {
        let value = (self.pc >> 8) as u8;
        bus.write(0x0100 | u16::from(self.sp), value);
        self.sp = self.sp.wrapping_sub(1);
        self.state = CpuState::PushLo;
        false
    }

    fn tick_push_lo(&mut self, bus: &mut impl Bus) -> bool {
        let value = (self.pc & 0xFF) as u8;
        bus.write(0x0100 | u16::from(self.sp), value);
        self.sp = self.sp.wrapping_sub(1);

        match self.instr_type {
            InstructionType::JumpSubroutine => {
                // JSR: set PC to target address
                self.pc = self.effective_addr;
                self.state = CpuState::FetchOpcode;
                return true;
            }
            InstructionType::Break => {
                self.state = CpuState::PushStatus;
            }
            _ => {
                self.state = CpuState::FetchOpcode;
            }
        }
        false
    }

    fn tick_push_status(&mut self, bus: &mut impl Bus) -> bool {
        match self.instr_type {
            InstructionType::Push => {
                // PHP: push status with B flag set
                let value = self.status.to_stack_byte(true);
                bus.write(0x0100 | u16::from(self.sp), value);
                self.sp = self.sp.wrapping_sub(1);
                self.state = CpuState::FetchOpcode;
                return true;
            }
            InstructionType::Break => {
                // BRK: check for NMI hijacking
                // If NMI is pending, it hijacks BRK by using the NMI vector instead of IRQ/BRK vector
                // IMPORTANT: B flag is ALWAYS set to 1 when pushed from BRK, even when NMI hijacks!
                // This allows software to detect NMI hijacking by checking B=1 in the NMI handler.
                // Reference: Mesen2 NesCpu.cpp BRK(), NESdev wiki "6502 BRK and B bit"
                let nmi_hijack = self.nmi_pending;
                if nmi_hijack {
                    self.nmi_pending = false;
                }

                // Push status with B flag ALWAYS set (even when NMI hijacks)
                let value = self.status.to_stack_byte(true);
                bus.write(0x0100 | u16::from(self.sp), value);
                self.sp = self.sp.wrapping_sub(1);
                self.status.insert(StatusFlags::INTERRUPT_DISABLE);

                // Suppress NMI check for one instruction after BRK completes
                // This ensures the first instruction of the handler executes before checking for NMI
                // Reference: Mesen2 NesCpu.cpp BRK() "_prevNeedNmi = false"
                self.suppress_nmi_next = true;

                // Use NMI vector if hijacked, IRQ/BRK vector otherwise
                self.effective_addr = if nmi_hijack { 0xFFFA } else { 0xFFFE };
                self.state = CpuState::InterruptFetchVectorLo;
            }
            _ => {
                self.state = CpuState::FetchOpcode;
            }
        }
        false
    }

    fn tick_pop_lo(&mut self, bus: &mut impl Bus) -> bool {
        // Internal cycle: increment SP
        self.sp = self.sp.wrapping_add(1);
        let _ = bus.read(0x0100 | u16::from(self.sp));

        match self.instr_type {
            InstructionType::Pull => {
                self.state = CpuState::Execute;
            }
            InstructionType::ReturnSubroutine => {
                self.operand_lo = bus.read(0x0100 | u16::from(self.sp));
                self.state = CpuState::PopHi;
            }
            InstructionType::ReturnInterrupt => {
                // First pop is status
                self.state = CpuState::PopStatus;
            }
            _ => {
                self.state = CpuState::FetchOpcode;
            }
        }
        false
    }

    fn tick_pop_hi(&mut self, bus: &mut impl Bus) -> bool {
        self.sp = self.sp.wrapping_add(1);
        self.operand_hi = bus.read(0x0100 | u16::from(self.sp));

        match self.instr_type {
            InstructionType::ReturnSubroutine => {
                self.pc = u16::from_le_bytes([self.operand_lo, self.operand_hi]);
                self.state = CpuState::InternalCycle;
            }
            InstructionType::ReturnInterrupt => {
                self.pc = u16::from_le_bytes([self.operand_lo, self.operand_hi]);
                self.state = CpuState::FetchOpcode;
                return true;
            }
            _ => {
                self.state = CpuState::FetchOpcode;
            }
        }
        false
    }

    fn tick_pop_status(&mut self, bus: &mut impl Bus) -> bool {
        let value = bus.read(0x0100 | u16::from(self.sp));
        self.status = StatusFlags::from_stack_byte(value);

        // Match RTI behavior from instructions.rs:
        // If RTI restores I=1 (Disabled), interrupts must be blocked immediately for the NEXT instruction.
        if self.status.contains(StatusFlags::INTERRUPT_DISABLE) {
            self.prev_irq_inhibit = true;
        }

        self.sp = self.sp.wrapping_add(1);
        self.operand_lo = bus.read(0x0100 | u16::from(self.sp));
        self.state = CpuState::PopHi;
        false
    }

    fn tick_internal_cycle(&mut self, bus: &mut impl Bus) -> bool {
        // Dummy read
        let _ = bus.read(0x0100 | u16::from(self.sp));

        match self.instr_type {
            InstructionType::JumpSubroutine => {
                // After internal cycle, push return address
                self.state = CpuState::PushHi;
            }
            InstructionType::ReturnSubroutine => {
                // Increment PC (RTS returns to addr+1)
                self.pc = self.pc.wrapping_add(1);
                self.state = CpuState::FetchOpcode;
                return true;
            }
            InstructionType::Push => {
                // PHA: after reading, push value
                match self.current_opcode {
                    0x48 => {
                        // PHA
                        bus.write(0x0100 | u16::from(self.sp), self.a);
                        self.sp = self.sp.wrapping_sub(1);
                    }
                    0x08 => {
                        // PHP is handled in PushStatus
                        self.state = CpuState::PushStatus;
                        return false;
                    }
                    _ => {}
                }
                self.state = CpuState::FetchOpcode;
                return true;
            }
            InstructionType::Pull => {
                // After internal cycle, read from stack
                self.sp = self.sp.wrapping_add(1);
                self.temp_value = bus.read(0x0100 | u16::from(self.sp));
                match self.current_opcode {
                    0x68 => {
                        // PLA
                        self.a = self.temp_value;
                        self.set_zn(self.a);
                    }
                    0x28 => {
                        // PLP
                        self.status = StatusFlags::from_stack_byte(self.temp_value);
                    }
                    _ => {}
                }
                self.state = CpuState::FetchOpcode;
                return true;
            }
            _ => {
                self.state = CpuState::FetchOpcode;
            }
        }
        false
    }

    fn tick_branch_taken(&mut self, bus: &mut impl Bus) -> bool {
        // Dummy read during branch taken
        let _ = bus.read(self.pc);

        let old_pc = self.pc;
        self.pc = self.pc.wrapping_add(self.branch_offset as u16);

        // Check for page crossing
        if (old_pc & 0xFF00) == (self.pc & 0xFF00) {
            self.state = CpuState::FetchOpcode;
            true
        } else {
            self.state = CpuState::BranchPageCross;
            false
        }
    }

    fn tick_branch_page_cross(&mut self, bus: &mut impl Bus) -> bool {
        // Dummy read during page crossing fix
        let _ = bus.read(
            (self.pc & 0x00FF) | ((self.pc.wrapping_sub(self.branch_offset as u16)) & 0xFF00),
        );
        self.state = CpuState::FetchOpcode;
        true
    }

    fn tick_interrupt_push_pc_hi(&mut self, bus: &mut impl Bus) -> bool {
        let value = (self.pc >> 8) as u8;
        bus.write(0x0100 | u16::from(self.sp), value);
        self.sp = self.sp.wrapping_sub(1);
        self.state = CpuState::InterruptPushPcLo;
        false
    }

    fn tick_interrupt_push_pc_lo(&mut self, bus: &mut impl Bus) -> bool {
        let value = (self.pc & 0xFF) as u8;
        bus.write(0x0100 | u16::from(self.sp), value);
        self.sp = self.sp.wrapping_sub(1);
        self.state = CpuState::InterruptPushStatus;
        false
    }

    fn tick_interrupt_push_status(&mut self, bus: &mut impl Bus) -> bool {
        // Interrupts push status with B=0
        let value = self.status.to_stack_byte(false);
        bus.write(0x0100 | u16::from(self.sp), value);
        self.sp = self.sp.wrapping_sub(1);
        self.status.insert(StatusFlags::INTERRUPT_DISABLE);
        self.state = CpuState::InterruptFetchVectorLo;
        false
    }

    fn tick_interrupt_fetch_vector_lo(&mut self, bus: &mut impl Bus) -> bool {
        self.operand_lo = bus.read(self.effective_addr);
        self.state = CpuState::InterruptFetchVectorHi;
        false
    }

    fn tick_interrupt_fetch_vector_hi(&mut self, bus: &mut impl Bus) -> bool {
        self.operand_hi = bus.read(self.effective_addr.wrapping_add(1));
        self.pc = u16::from_le_bytes([self.operand_lo, self.operand_hi]);
        self.state = CpuState::FetchOpcode;
        true
    }

    // =========================================================================
    // HELPER METHODS
    // =========================================================================

    /// Determine next state based on instruction type.
    fn next_state_for_instruction_type(&self) -> CpuState {
        match self.instr_type {
            InstructionType::Read => CpuState::ReadData,
            InstructionType::Write => CpuState::WriteData,
            InstructionType::ReadModifyWrite => CpuState::RmwRead,
            InstructionType::Implied | InstructionType::Accumulator => CpuState::Execute,
            InstructionType::Push => CpuState::InternalCycle,
            InstructionType::Pull => CpuState::InternalCycle,
            _ => CpuState::Execute,
        }
    }

    /// Check if branch condition is met for current opcode.
    fn check_branch_condition(&self) -> bool {
        match self.current_opcode {
            0x10 => !self.status.contains(StatusFlags::NEGATIVE), // BPL
            0x30 => self.status.contains(StatusFlags::NEGATIVE),  // BMI
            0x50 => !self.status.contains(StatusFlags::OVERFLOW), // BVC
            0x70 => self.status.contains(StatusFlags::OVERFLOW),  // BVS
            0x90 => !self.status.contains(StatusFlags::CARRY),    // BCC
            0xB0 => self.status.contains(StatusFlags::CARRY),     // BCS
            0xD0 => !self.status.contains(StatusFlags::ZERO),     // BNE
            0xF0 => self.status.contains(StatusFlags::ZERO),      // BEQ
            _ => false,
        }
    }

    /// Execute an implied instruction (register-only operations).
    fn execute_implied_instruction(&mut self) {
        match self.current_opcode {
            // Transfers
            0xAA => {
                self.x = self.a;
                self.set_zn(self.x);
            } // TAX
            0xA8 => {
                self.y = self.a;
                self.set_zn(self.y);
            } // TAY
            0x8A => {
                self.a = self.x;
                self.set_zn(self.a);
            } // TXA
            0x98 => {
                self.a = self.y;
                self.set_zn(self.a);
            } // TYA
            0xBA => {
                self.a = self.sp;
                self.set_zn(self.a);
            } // TSX
            0x9A => {
                self.sp = self.x;
            } // TXS

            // Increment/Decrement
            0xE8 => {
                self.x = self.x.wrapping_add(1);
                self.set_zn(self.x);
            } // INX
            0xC8 => {
                self.y = self.y.wrapping_add(1);
                self.set_zn(self.y);
            } // INY
            0xCA => {
                self.x = self.x.wrapping_sub(1);
                self.set_zn(self.x);
            } // DEX
            0x88 => {
                self.y = self.y.wrapping_sub(1);
                self.set_zn(self.y);
            } // DEY

            // Flags
            0x18 => {
                self.status.remove(StatusFlags::CARRY);
            } // CLC
            0x38 => {
                self.status.insert(StatusFlags::CARRY);
            } // SEC
            0x58 => {
                self.status.remove(StatusFlags::INTERRUPT_DISABLE);
            } // CLI
            0x78 => {
                self.status.insert(StatusFlags::INTERRUPT_DISABLE);
            } // SEI
            0xB8 => {
                self.status.remove(StatusFlags::OVERFLOW);
            } // CLV
            0xD8 => {
                self.status.remove(StatusFlags::DECIMAL);
            } // CLD
            0xF8 => {
                self.status.insert(StatusFlags::DECIMAL);
            } // SED

            // NOP (official and unofficial)
            0xEA | 0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => {}

            _ => {}
        }
    }

    /// Execute an accumulator instruction (ASL A, LSR A, ROL A, ROR A).
    fn execute_accumulator_instruction(&mut self) {
        match self.current_opcode {
            0x0A => {
                // ASL A
                let carry = (self.a & 0x80) != 0;
                self.a <<= 1;
                self.status.set(StatusFlags::CARRY, carry);
                self.set_zn(self.a);
            }
            0x4A => {
                // LSR A
                let carry = (self.a & 0x01) != 0;
                self.a >>= 1;
                self.status.set(StatusFlags::CARRY, carry);
                self.set_zn(self.a);
            }
            0x2A => {
                // ROL A
                let old_carry = self.status.contains(StatusFlags::CARRY);
                let new_carry = (self.a & 0x80) != 0;
                self.a = (self.a << 1) | u8::from(old_carry);
                self.status.set(StatusFlags::CARRY, new_carry);
                self.set_zn(self.a);
            }
            0x6A => {
                // ROR A
                let old_carry = self.status.contains(StatusFlags::CARRY);
                let new_carry = (self.a & 0x01) != 0;
                self.a = (self.a >> 1) | (u8::from(old_carry) << 7);
                self.status.set(StatusFlags::CARRY, new_carry);
                self.set_zn(self.a);
            }
            _ => {}
        }
    }

    /// Execute a read instruction using self.temp_value.
    #[allow(clippy::too_many_lines)]
    fn execute_read_instruction(&mut self) {
        let value = self.temp_value;
        match self.current_opcode {
            // LDA
            0xA9 | 0xA5 | 0xB5 | 0xAD | 0xBD | 0xB9 | 0xA1 | 0xB1 => {
                self.a = value;
                self.set_zn(self.a);
            }
            // LDX
            0xA2 | 0xA6 | 0xB6 | 0xAE | 0xBE => {
                self.x = value;
                self.set_zn(self.x);
            }
            // LDY
            0xA0 | 0xA4 | 0xB4 | 0xAC | 0xBC => {
                self.y = value;
                self.set_zn(self.y);
            }
            // ADC
            0x69 | 0x65 | 0x75 | 0x6D | 0x7D | 0x79 | 0x61 | 0x71 => {
                self.do_adc(value);
            }
            // SBC (including unofficial 0xEB)
            0xE9 | 0xE5 | 0xF5 | 0xED | 0xFD | 0xF9 | 0xE1 | 0xF1 | 0xEB => {
                self.do_sbc(value);
            }
            // AND
            0x29 | 0x25 | 0x35 | 0x2D | 0x3D | 0x39 | 0x21 | 0x31 => {
                self.a &= value;
                self.set_zn(self.a);
            }
            // ORA
            0x09 | 0x05 | 0x15 | 0x0D | 0x1D | 0x19 | 0x01 | 0x11 => {
                self.a |= value;
                self.set_zn(self.a);
            }
            // EOR
            0x49 | 0x45 | 0x55 | 0x4D | 0x5D | 0x59 | 0x41 | 0x51 => {
                self.a ^= value;
                self.set_zn(self.a);
            }
            // CMP
            0xC9 | 0xC5 | 0xD5 | 0xCD | 0xDD | 0xD9 | 0xC1 | 0xD1 => {
                self.do_compare(self.a, value);
            }
            // CPX
            0xE0 | 0xE4 | 0xEC => {
                self.do_compare(self.x, value);
            }
            // CPY
            0xC0 | 0xC4 | 0xCC => {
                self.do_compare(self.y, value);
            }
            // BIT
            0x24 | 0x2C => {
                self.status.set(StatusFlags::ZERO, (self.a & value) == 0);
                self.status.set(StatusFlags::OVERFLOW, (value & 0x40) != 0);
                self.status.set(StatusFlags::NEGATIVE, (value & 0x80) != 0);
            }
            // LAX (unofficial)
            0xA7 | 0xB7 | 0xAF | 0xBF | 0xA3 | 0xB3 => {
                self.a = value;
                self.x = value;
                self.set_zn(self.a);
            }
            // LAS (unofficial)
            0xBB => {
                let result = value & self.sp;
                self.a = result;
                self.x = result;
                self.sp = result;
                self.set_zn(result);
            }
            // ANC (unofficial)
            0x0B | 0x2B => {
                self.a &= value;
                self.set_zn(self.a);
                self.status.set(StatusFlags::CARRY, (self.a & 0x80) != 0);
            }
            // ALR (unofficial)
            0x4B => {
                self.a &= value;
                let carry = (self.a & 0x01) != 0;
                self.a >>= 1;
                self.status.set(StatusFlags::CARRY, carry);
                self.set_zn(self.a);
            }
            // ARR (unofficial)
            0x6B => {
                self.a &= value;
                let old_carry = self.status.contains(StatusFlags::CARRY);
                self.a = (self.a >> 1) | (u8::from(old_carry) << 7);
                self.set_zn(self.a);
                self.status.set(StatusFlags::CARRY, (self.a & 0x40) != 0);
                self.status.set(
                    StatusFlags::OVERFLOW,
                    ((self.a & 0x40) ^ ((self.a & 0x20) << 1)) != 0,
                );
            }
            // XAA (unofficial, unstable)
            0x8B => {
                self.a = (self.a | 0xEE) & self.x & value;
                self.set_zn(self.a);
            }
            // LXA (unofficial)
            0xAB => {
                self.a = (self.a | 0xEE) & value;
                self.x = self.a;
                self.set_zn(self.a);
            }
            // AXS (unofficial)
            0xCB => {
                let temp = (self.a & self.x).wrapping_sub(value);
                self.status
                    .set(StatusFlags::CARRY, (self.a & self.x) >= value);
                self.x = temp;
                self.set_zn(self.x);
            }
            // NOPs with read (unofficial)
            0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 | 0x04 | 0x44 | 0x64 | 0x14 | 0x34 | 0x54 | 0x74
            | 0xD4 | 0xF4 | 0x0C | 0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => {
                // Do nothing - just read
            }
            _ => {}
        }
    }

    /// Execute a write instruction, returning value to write.
    fn execute_write_instruction(&self) -> u8 {
        match self.current_opcode {
            // STA
            0x85 | 0x95 | 0x8D | 0x9D | 0x99 | 0x81 | 0x91 => self.a,
            // STX
            0x86 | 0x96 | 0x8E => self.x,
            // STY
            0x84 | 0x94 | 0x8C => self.y,
            // SAX (unofficial)
            0x87 | 0x97 | 0x8F | 0x83 => self.a & self.x,
            // SHA (unofficial) - highly unstable
            0x93 | 0x9F => self.a & self.x & ((self.effective_addr >> 8) as u8).wrapping_add(1),
            // SHX (unofficial)
            0x9E => self.x & ((self.effective_addr >> 8) as u8).wrapping_add(1),
            // SHY (unofficial)
            0x9C => self.y & ((self.effective_addr >> 8) as u8).wrapping_add(1),
            // TAS (unofficial)
            0x9B => {
                // This also affects SP, but we handle value here
                self.a & self.x & ((self.effective_addr >> 8) as u8).wrapping_add(1)
            }
            _ => 0,
        }
    }

    /// Execute an RMW instruction, returning the new value.
    fn execute_rmw_instruction(&mut self) -> u8 {
        let value = self.temp_value;
        match self.current_opcode {
            // ASL
            0x06 | 0x16 | 0x0E | 0x1E => {
                let carry = (value & 0x80) != 0;
                let result = value << 1;
                self.status.set(StatusFlags::CARRY, carry);
                self.set_zn(result);
                result
            }
            // LSR
            0x46 | 0x56 | 0x4E | 0x5E => {
                let carry = (value & 0x01) != 0;
                let result = value >> 1;
                self.status.set(StatusFlags::CARRY, carry);
                self.set_zn(result);
                result
            }
            // ROL
            0x26 | 0x36 | 0x2E | 0x3E => {
                let old_carry = self.status.contains(StatusFlags::CARRY);
                let new_carry = (value & 0x80) != 0;
                let result = (value << 1) | u8::from(old_carry);
                self.status.set(StatusFlags::CARRY, new_carry);
                self.set_zn(result);
                result
            }
            // ROR
            0x66 | 0x76 | 0x6E | 0x7E => {
                let old_carry = self.status.contains(StatusFlags::CARRY);
                let new_carry = (value & 0x01) != 0;
                let result = (value >> 1) | (u8::from(old_carry) << 7);
                self.status.set(StatusFlags::CARRY, new_carry);
                self.set_zn(result);
                result
            }
            // INC
            0xE6 | 0xF6 | 0xEE | 0xFE => {
                let result = value.wrapping_add(1);
                self.set_zn(result);
                result
            }
            // DEC
            0xC6 | 0xD6 | 0xCE | 0xDE => {
                let result = value.wrapping_sub(1);
                self.set_zn(result);
                result
            }
            // SLO (unofficial: ASL + ORA)
            0x07 | 0x17 | 0x0F | 0x1F | 0x1B | 0x03 | 0x13 => {
                let carry = (value & 0x80) != 0;
                let result = value << 1;
                self.status.set(StatusFlags::CARRY, carry);
                self.a |= result;
                self.set_zn(self.a);
                result
            }
            // RLA (unofficial: ROL + AND)
            0x27 | 0x37 | 0x2F | 0x3F | 0x3B | 0x23 | 0x33 => {
                let old_carry = self.status.contains(StatusFlags::CARRY);
                let new_carry = (value & 0x80) != 0;
                let result = (value << 1) | u8::from(old_carry);
                self.status.set(StatusFlags::CARRY, new_carry);
                self.a &= result;
                self.set_zn(self.a);
                result
            }
            // SRE (unofficial: LSR + EOR)
            0x47 | 0x57 | 0x4F | 0x5F | 0x5B | 0x43 | 0x53 => {
                let carry = (value & 0x01) != 0;
                let result = value >> 1;
                self.status.set(StatusFlags::CARRY, carry);
                self.a ^= result;
                self.set_zn(self.a);
                result
            }
            // RRA (unofficial: ROR + ADC)
            0x67 | 0x77 | 0x6F | 0x7F | 0x7B | 0x63 | 0x73 => {
                let old_carry = self.status.contains(StatusFlags::CARRY);
                let new_carry = (value & 0x01) != 0;
                let result = (value >> 1) | (u8::from(old_carry) << 7);
                self.status.set(StatusFlags::CARRY, new_carry);
                self.do_adc(result);
                result
            }
            // DCP (unofficial: DEC + CMP)
            0xC7 | 0xD7 | 0xCF | 0xDF | 0xDB | 0xC3 | 0xD3 => {
                let result = value.wrapping_sub(1);
                self.do_compare(self.a, result);
                result
            }
            // ISC (unofficial: INC + SBC)
            0xE7 | 0xF7 | 0xEF | 0xFF | 0xFB | 0xE3 | 0xF3 => {
                let result = value.wrapping_add(1);
                self.do_sbc(result);
                result
            }
            _ => value,
        }
    }

    /// Perform ADC operation.
    fn do_adc(&mut self, value: u8) {
        let carry = u16::from(self.status.contains(StatusFlags::CARRY));
        let sum = u16::from(self.a) + u16::from(value) + carry;
        let result = sum as u8;

        self.status.set(StatusFlags::CARRY, sum > 0xFF);
        self.status.set(
            StatusFlags::OVERFLOW,
            (!(self.a ^ value) & (self.a ^ result) & 0x80) != 0,
        );
        self.a = result;
        self.set_zn(self.a);
    }

    /// Perform SBC operation.
    fn do_sbc(&mut self, value: u8) {
        // SBC is equivalent to ADC with the value inverted
        self.do_adc(!value);
    }

    /// Perform compare operation.
    fn do_compare(&mut self, register: u8, value: u8) {
        let result = register.wrapping_sub(value);
        self.status.set(StatusFlags::CARRY, register >= value);
        self.set_zn(result);
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
            0xAA => self.tax(bus),
            0xA8 => self.tay(bus),
            0x8A => self.txa(bus),
            0x98 => self.tya(bus),
            0xBA => self.tsx(bus),
            0x9A => self.txs(bus),

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
            0xE8 => self.inx(bus),
            0xC8 => self.iny(bus),
            0xCA => self.dex(bus),
            0x88 => self.dey(bus),

            // Logic
            0x29 | 0x25 | 0x35 | 0x2D | 0x3D | 0x39 | 0x21 | 0x31 => self.and(bus, addr_mode),
            0x09 | 0x05 | 0x15 | 0x0D | 0x1D | 0x19 | 0x01 | 0x11 => self.ora(bus, addr_mode),
            0x49 | 0x45 | 0x55 | 0x4D | 0x5D | 0x59 | 0x41 | 0x51 => self.eor(bus, addr_mode),
            0x24 | 0x2C => self.bit(bus, addr_mode),

            // Shift/Rotate
            0x0A => self.asl_acc(bus),
            0x06 | 0x16 | 0x0E | 0x1E => self.asl(bus, addr_mode),
            0x4A => self.lsr_acc(bus),
            0x46 | 0x56 | 0x4E | 0x5E => self.lsr(bus, addr_mode),
            0x2A => self.rol_acc(bus),
            0x26 | 0x36 | 0x2E | 0x3E => self.rol(bus, addr_mode),
            0x6A => self.ror_acc(bus),
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
            0x18 => self.clc(bus),
            0x38 => self.sec(bus),
            0x58 => self.cli(bus),
            0x78 => self.sei(bus),
            0xB8 => self.clv(bus),
            0xD8 => self.cld(bus),
            0xF8 => self.sed(bus),
            0xEA => self.nop(bus),

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
            0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => self.nop(bus),
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

        // Perform dummy read for indexed addressing modes with page crossing
        // This matches hardware behavior where CPU reads from incorrect address
        // before applying the page boundary correction
        if result.page_crossed {
            match mode {
                AddressingMode::AbsoluteX
                | AddressingMode::AbsoluteY
                | AddressingMode::IndirectIndexedY => {
                    // Calculate the incorrect address (before page fix)
                    // Take high byte from base, low byte from final address
                    let incorrect_addr = (result.base_addr & 0xFF00) | (result.addr & 0x00FF);
                    let _ = bus.read(incorrect_addr);
                }
                _ => {}
            }
        }

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

        // Perform dummy write for indexed addressing modes
        // Write operations ALWAYS perform a dummy write (unconditional, not just on page crossing)
        match mode {
            AddressingMode::AbsoluteX
            | AddressingMode::AbsoluteY
            | AddressingMode::IndirectIndexedY => {
                // Calculate the incorrect address (before page fix)
                let incorrect_addr = (result.base_addr & 0xFF00) | (result.addr & 0x00FF);
                // Dummy write to incorrect address (this is what hardware does)
                bus.write(incorrect_addr, value);
            }
            _ => {}
        }

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

//! Core CPU implementation.
//!
//! This module contains the main CPU struct and its implementation,
//! including cycle-accurate execution and interrupt handling.

use crate::addressing::{ADDR_MODE_TABLE, AddrMode};
use crate::instructions::OPCODE_TABLE;
use crate::status::Status;
use crate::vectors;

/// Memory bus trait for CPU memory access.
///
/// Implement this trait to connect the CPU to a memory subsystem.
/// All memory operations go through this trait, allowing for
/// memory-mapped I/O and proper bus timing.
pub trait Bus {
    /// Read a byte from the given address.
    fn read(&mut self, addr: u16) -> u8;

    /// Write a byte to the given address.
    fn write(&mut self, addr: u16, value: u8);

    /// Called when a CPU cycle occurs.
    /// Override this for cycle-accurate PPU/APU synchronization.
    #[inline]
    fn on_cpu_cycle(&mut self) {}

    /// Read without side effects (for debugging).
    /// Default implementation calls `read`.
    fn peek(&self, addr: u16) -> u8
    where
        Self: Sized,
    {
        // Default: we can't peek without mutable access
        let _ = addr;
        0
    }
}

/// Interrupt types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interrupt {
    /// Non-Maskable Interrupt.
    Nmi,
    /// Interrupt Request (maskable).
    Irq,
    /// Software interrupt (BRK instruction).
    Brk,
    /// Reset signal.
    Reset,
}

/// CPU state for save states and debugging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CpuState {
    /// Program counter.
    pub pc: u16,
    /// Stack pointer.
    pub sp: u8,
    /// Accumulator.
    pub a: u8,
    /// X register.
    pub x: u8,
    /// Y register.
    pub y: u8,
    /// Status register.
    pub status: Status,
    /// Total CPU cycles executed.
    pub cycles: u64,
    /// Current opcode being executed.
    pub opcode: u8,
    /// NMI pending flag.
    pub nmi_pending: bool,
    /// IRQ pending flag.
    pub irq_pending: bool,
}

/// MOS 6502 CPU.
///
/// This is a cycle-accurate implementation of the 6502 CPU as used
/// in the Nintendo Entertainment System. It supports all 256 opcodes
/// including unofficial/undocumented instructions.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct Cpu {
    /// Program counter.
    pub pc: u16,
    /// Stack pointer (offset from $0100).
    pub sp: u8,
    /// Accumulator register.
    pub a: u8,
    /// X index register.
    pub x: u8,
    /// Y index register.
    pub y: u8,
    /// Processor status register.
    pub status: Status,
    /// Total CPU cycles executed.
    pub cycles: u64,

    // Instruction execution state
    /// Current opcode being executed.
    pub(crate) opcode: u8,
    /// Addressing mode for current instruction.
    pub(crate) addr_mode: AddrMode,
    /// Operand address (for memory-addressing modes).
    pub(crate) operand_addr: u16,
    /// Fetched operand value.
    pub(crate) operand_value: u8,

    // Interrupt state
    /// NMI line state (active low on real hardware).
    pub(crate) nmi_pending: bool,
    /// Previous NMI state for edge detection.
    pub(crate) prev_nmi: bool,
    /// IRQ line state.
    pub(crate) irq_pending: bool,
    /// Whether to run IRQ at end of instruction.
    pub(crate) run_irq: bool,
    /// Previous run_irq state (for edge detection).
    pub(crate) prev_run_irq: bool,
    /// NMI should be triggered.
    pub(crate) nmi_triggered: bool,

    // DMA state
    /// OAM DMA in progress.
    pub(crate) oam_dma_pending: bool,
    /// OAM DMA page address.
    pub(crate) oam_dma_page: u8,
    /// DMC DMA stall cycles.
    pub(crate) dmc_stall_cycles: u8,

    /// Last value on the data bus (for open bus behavior).
    pub(crate) last_bus_value: u8,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    /// Stack base address ($0100).
    const STACK_BASE: u16 = 0x0100;

    /// Create a new CPU in its initial power-on state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pc: 0,
            sp: 0xFD,
            a: 0,
            x: 0,
            y: 0,
            status: Status::POWER_ON,
            cycles: 0,
            opcode: 0,
            addr_mode: AddrMode::Imp,
            operand_addr: 0,
            operand_value: 0,
            nmi_pending: false,
            prev_nmi: false,
            irq_pending: false,
            run_irq: false,
            prev_run_irq: false,
            nmi_triggered: false,
            oam_dma_pending: false,
            oam_dma_page: 0,
            dmc_stall_cycles: 0,
            last_bus_value: 0,
        }
    }

    /// Reset the CPU.
    ///
    /// This simulates the reset signal, loading the PC from the reset vector
    /// and initializing registers to their reset state.
    pub fn reset(&mut self, bus: &mut impl Bus) {
        // Reset takes 7 cycles
        for _ in 0..7 {
            self.tick(bus);
        }

        // Load PC from reset vector
        let lo = bus.read(vectors::RESET);
        let hi = bus.read(vectors::RESET + 1);
        self.pc = u16::from_le_bytes([lo, hi]);

        // Reset state
        self.sp = 0xFD;
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.status = Status::POWER_ON;

        // Clear interrupt state
        self.nmi_pending = false;
        self.prev_nmi = false;
        self.irq_pending = false;
        self.run_irq = false;
        self.prev_run_irq = false;
        self.nmi_triggered = false;

        // Clear DMA state
        self.oam_dma_pending = false;
        self.dmc_stall_cycles = 0;
    }

    /// Execute one CPU cycle (tick).
    ///
    /// This is the lowest-level execution method, advancing the CPU
    /// by exactly one cycle. Used for cycle-accurate synchronization.
    #[inline]
    pub fn tick(&mut self, bus: &mut (impl Bus + ?Sized)) {
        self.cycles = self.cycles.wrapping_add(1);
        bus.on_cpu_cycle();
    }

    /// Execute one complete instruction.
    ///
    /// Returns the number of cycles consumed by the instruction.
    pub fn step(&mut self, bus: &mut impl Bus) -> u8 {
        let start_cycles = self.cycles;

        // Handle DMA if pending
        if self.oam_dma_pending {
            self.execute_oam_dma(bus);
        }

        // Handle DMC DMA stalls
        while self.dmc_stall_cycles > 0 {
            self.dmc_stall_cycles -= 1;
            self.tick(bus);
        }

        // Check for pending interrupts
        if self.prev_run_irq || self.nmi_triggered {
            self.handle_interrupt(bus);
        } else {
            // Fetch and execute instruction
            self.fetch_and_execute(bus);
        }

        // Handle interrupt detection at end of instruction
        self.detect_interrupts();

        (self.cycles - start_cycles) as u8
    }

    /// Fetch and execute a single instruction.
    fn fetch_and_execute(&mut self, bus: &mut impl Bus) {
        // Fetch opcode
        self.opcode = self.read_byte(bus, self.pc);
        self.pc = self.pc.wrapping_add(1);

        // Get addressing mode
        self.addr_mode = ADDR_MODE_TABLE[self.opcode as usize];

        // Fetch operand based on addressing mode
        self.fetch_operand(bus);

        // Execute instruction
        let instruction = OPCODE_TABLE[self.opcode as usize];
        instruction(self, bus);
    }

    /// Fetch operand based on current addressing mode.
    #[allow(clippy::too_many_lines)]
    fn fetch_operand(&mut self, bus: &mut impl Bus) {
        match self.addr_mode {
            AddrMode::Imp => {
                // Dummy read
                self.read_byte(bus, self.pc);
            }
            AddrMode::Acc => {
                // Dummy read
                self.read_byte(bus, self.pc);
                self.operand_value = self.a;
            }
            AddrMode::Imm => {
                // Just set the address - instruction will do the read
                self.operand_addr = self.pc;
                self.pc = self.pc.wrapping_add(1);
            }
            AddrMode::Zp0 => {
                self.operand_addr = u16::from(self.read_byte(bus, self.pc));
                self.pc = self.pc.wrapping_add(1);
            }
            AddrMode::Zpx => {
                let base = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
                // Dummy read during index calculation
                self.read_byte(bus, u16::from(base));
                self.operand_addr = u16::from(base.wrapping_add(self.x));
            }
            AddrMode::Zpy => {
                let base = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
                // Dummy read during index calculation
                self.read_byte(bus, u16::from(base));
                self.operand_addr = u16::from(base.wrapping_add(self.y));
            }
            AddrMode::Rel => {
                self.operand_value = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
            }
            AddrMode::Abs => {
                let lo = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
                let hi = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
                self.operand_addr = u16::from_le_bytes([lo, hi]);
            }
            AddrMode::Abx => {
                let lo = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
                let hi = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
                let base = u16::from_le_bytes([lo, hi]);
                self.operand_addr = base.wrapping_add(u16::from(self.x));

                // Page crossing check - only adds cycle for read instructions
                if Cpu::page_crossed(base, self.operand_addr) {
                    // Dummy read at wrong address
                    self.read_byte(bus, (base & 0xFF00) | (self.operand_addr & 0x00FF));
                }
            }
            AddrMode::AbxW => {
                let lo = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
                let hi = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
                let base = u16::from_le_bytes([lo, hi]);
                self.operand_addr = base.wrapping_add(u16::from(self.x));

                // Always do dummy read for write instructions
                self.read_byte(bus, (base & 0xFF00) | (self.operand_addr & 0x00FF));
            }
            AddrMode::Aby => {
                let lo = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
                let hi = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
                let base = u16::from_le_bytes([lo, hi]);
                self.operand_addr = base.wrapping_add(u16::from(self.y));

                // Page crossing check
                if Cpu::page_crossed(base, self.operand_addr) {
                    self.read_byte(bus, (base & 0xFF00) | (self.operand_addr & 0x00FF));
                }
            }
            AddrMode::AbyW => {
                let lo = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
                let hi = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
                let base = u16::from_le_bytes([lo, hi]);
                self.operand_addr = base.wrapping_add(u16::from(self.y));

                // Always do dummy read for write instructions
                self.read_byte(bus, (base & 0xFF00) | (self.operand_addr & 0x00FF));
            }
            AddrMode::Ind => {
                let lo = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
                let hi = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
                let ptr = u16::from_le_bytes([lo, hi]);

                // JMP indirect bug: wraps within page
                let lo = self.read_byte(bus, ptr);
                let hi_addr = if ptr & 0x00FF == 0x00FF {
                    ptr & 0xFF00 // Wrap within page
                } else {
                    ptr + 1
                };
                let hi = self.read_byte(bus, hi_addr);
                self.operand_addr = u16::from_le_bytes([lo, hi]);
            }
            AddrMode::Idx => {
                let base = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
                // Dummy read
                self.read_byte(bus, u16::from(base));
                let ptr = base.wrapping_add(self.x);

                // Read pointer (wraps within zero page)
                let lo = self.read_byte(bus, u16::from(ptr));
                let hi = self.read_byte(bus, u16::from(ptr.wrapping_add(1)));
                self.operand_addr = u16::from_le_bytes([lo, hi]);
            }
            AddrMode::Idy => {
                let ptr = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);

                // Read pointer (wraps within zero page)
                let lo = self.read_byte(bus, u16::from(ptr));
                let hi = self.read_byte(bus, u16::from(ptr.wrapping_add(1)));
                let base = u16::from_le_bytes([lo, hi]);
                self.operand_addr = base.wrapping_add(u16::from(self.y));

                // Page crossing check
                if Cpu::page_crossed(base, self.operand_addr) {
                    self.read_byte(bus, (base & 0xFF00) | (self.operand_addr & 0x00FF));
                }
            }
            AddrMode::IdyW => {
                let ptr = self.read_byte(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);

                let lo = self.read_byte(bus, u16::from(ptr));
                let hi = self.read_byte(bus, u16::from(ptr.wrapping_add(1)));
                let base = u16::from_le_bytes([lo, hi]);
                self.operand_addr = base.wrapping_add(u16::from(self.y));

                // Always do dummy read for write instructions
                self.read_byte(bus, (base & 0xFF00) | (self.operand_addr & 0x00FF));
            }
        }
    }

    /// Handle interrupt (NMI or IRQ).
    fn handle_interrupt(&mut self, bus: &mut impl Bus) {
        // Dummy reads (same as BRK without incrementing PC)
        self.read_byte(bus, self.pc);
        self.read_byte(bus, self.pc);

        // Push PC and status to stack
        self.push_word(bus, self.pc);

        // Determine vector based on interrupt type
        let (vector, is_nmi) = if self.nmi_triggered {
            self.nmi_triggered = false;
            (vectors::NMI, true)
        } else {
            (vectors::IRQ, false)
        };

        // Push status (B flag clear for hardware interrupts)
        let status_byte = self.status.to_stack_byte(false);
        self.push_byte(bus, status_byte);

        // Set I flag
        self.status.set_flag(Status::I, true);

        // Load vector
        let lo = self.read_byte(bus, vector);
        let hi = self.read_byte(bus, vector + 1);
        self.pc = u16::from_le_bytes([lo, hi]);

        // Clear NMI if it was an NMI
        if is_nmi {
            self.nmi_pending = false;
        }

        self.prev_run_irq = false;
        self.run_irq = false;
    }

    /// Detect pending interrupts at end of instruction.
    fn detect_interrupts(&mut self) {
        // NMI edge detection
        if self.nmi_pending && !self.prev_nmi {
            self.nmi_triggered = true;
        }
        self.prev_nmi = self.nmi_pending;

        // IRQ level detection
        self.prev_run_irq = self.run_irq;
        self.run_irq = self.irq_pending && !self.status.contains(Status::I);
    }

    /// Execute OAM DMA transfer.
    fn execute_oam_dma(&mut self, bus: &mut impl Bus) {
        self.oam_dma_pending = false;

        // Dummy cycle (get on correct cycle alignment)
        self.tick(bus);

        // Additional dummy cycle if on odd cycle
        if self.cycles & 1 == 1 {
            self.tick(bus);
        }

        let base = u16::from(self.oam_dma_page) << 8;

        // 256 read/write pairs
        for i in 0..256u16 {
            let value = self.read_byte(bus, base.wrapping_add(i));
            self.write_byte(bus, 0x2004, value);
        }
    }

    // Memory access helpers

    /// Read a byte from memory with cycle counting.
    #[inline]
    pub(crate) fn read_byte(&mut self, bus: &mut (impl Bus + ?Sized), addr: u16) -> u8 {
        self.tick(bus);
        let value = bus.read(addr);
        self.last_bus_value = value;
        value
    }

    /// Write a byte to memory with cycle counting.
    #[inline]
    pub(crate) fn write_byte(&mut self, bus: &mut (impl Bus + ?Sized), addr: u16, value: u8) {
        self.tick(bus);
        self.last_bus_value = value;
        bus.write(addr, value);
    }

    /// Push a byte to the stack.
    #[inline]
    pub(crate) fn push_byte(&mut self, bus: &mut (impl Bus + ?Sized), value: u8) {
        self.write_byte(bus, Self::STACK_BASE | u16::from(self.sp), value);
        self.sp = self.sp.wrapping_sub(1);
    }

    /// Pop a byte from the stack.
    #[inline]
    pub(crate) fn pop_byte(&mut self, bus: &mut (impl Bus + ?Sized)) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.read_byte(bus, Self::STACK_BASE | u16::from(self.sp))
    }

    /// Push a 16-bit word to the stack (high byte first).
    #[inline]
    pub(crate) fn push_word(&mut self, bus: &mut (impl Bus + ?Sized), value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.push_byte(bus, hi);
        self.push_byte(bus, lo);
    }

    /// Pop a 16-bit word from the stack (low byte first).
    #[inline]
    pub(crate) fn pop_word(&mut self, bus: &mut (impl Bus + ?Sized)) -> u16 {
        let lo = self.pop_byte(bus);
        let hi = self.pop_byte(bus);
        u16::from_le_bytes([lo, hi])
    }

    /// Check if two addresses are on different pages.
    #[inline]
    const fn page_crossed(addr1: u16, addr2: u16) -> bool {
        (addr1 & 0xFF00) != (addr2 & 0xFF00)
    }

    // Flag helpers

    /// Set the Zero and Negative flags based on a value.
    #[inline]
    pub(crate) fn set_zn(&mut self, value: u8) {
        self.status.set_zn(value);
    }

    // Public accessors

    /// Get the program counter.
    #[inline]
    #[must_use]
    pub const fn pc(&self) -> u16 {
        self.pc
    }

    /// Set the program counter.
    #[inline]
    pub fn set_pc(&mut self, value: u16) {
        self.pc = value;
    }

    /// Get the stack pointer.
    #[inline]
    #[must_use]
    pub const fn sp(&self) -> u8 {
        self.sp
    }

    /// Set the stack pointer.
    #[inline]
    pub fn set_sp(&mut self, value: u8) {
        self.sp = value;
    }

    /// Get the accumulator.
    #[inline]
    #[must_use]
    pub const fn a(&self) -> u8 {
        self.a
    }

    /// Set the accumulator (updates Z/N flags).
    #[inline]
    pub fn set_a(&mut self, value: u8) {
        self.a = value;
        self.set_zn(value);
    }

    /// Get the X register.
    #[inline]
    #[must_use]
    pub const fn x(&self) -> u8 {
        self.x
    }

    /// Set the X register (updates Z/N flags).
    #[inline]
    pub fn set_x(&mut self, value: u8) {
        self.x = value;
        self.set_zn(value);
    }

    /// Get the Y register.
    #[inline]
    #[must_use]
    pub const fn y(&self) -> u8 {
        self.y
    }

    /// Set the Y register (updates Z/N flags).
    #[inline]
    pub fn set_y(&mut self, value: u8) {
        self.y = value;
        self.set_zn(value);
    }

    /// Get the status register.
    #[inline]
    #[must_use]
    pub const fn status(&self) -> Status {
        self.status
    }

    /// Set the status register.
    #[inline]
    pub fn set_status(&mut self, value: Status) {
        self.status = value;
    }

    /// Get the total number of cycles executed.
    #[inline]
    #[must_use]
    pub const fn cycles(&self) -> u64 {
        self.cycles
    }

    /// Get the operand address for the current instruction.
    #[inline]
    #[must_use]
    pub const fn operand_addr(&self) -> u16 {
        self.operand_addr
    }

    /// Get the last value on the data bus.
    #[inline]
    #[must_use]
    pub const fn last_bus_value(&self) -> u8 {
        self.last_bus_value
    }

    /// Trigger an NMI.
    #[inline]
    pub fn trigger_nmi(&mut self) {
        self.nmi_pending = true;
    }

    /// Clear NMI.
    #[inline]
    pub fn clear_nmi(&mut self) {
        self.nmi_pending = false;
    }

    /// Set IRQ pending state.
    #[inline]
    pub fn set_irq(&mut self, pending: bool) {
        self.irq_pending = pending;
    }

    /// Check if IRQ is pending.
    #[inline]
    #[must_use]
    pub const fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    /// Start an OAM DMA transfer.
    #[inline]
    pub fn start_oam_dma(&mut self, page: u8) {
        self.oam_dma_pending = true;
        self.oam_dma_page = page;
    }

    /// Add DMC DMA stall cycles.
    #[inline]
    pub fn add_dmc_stall(&mut self, cycles: u8) {
        self.dmc_stall_cycles = self.dmc_stall_cycles.saturating_add(cycles);
    }

    /// Get the current CPU state.
    #[must_use]
    pub const fn state(&self) -> CpuState {
        CpuState {
            pc: self.pc,
            sp: self.sp,
            a: self.a,
            x: self.x,
            y: self.y,
            status: self.status,
            cycles: self.cycles,
            opcode: self.opcode,
            nmi_pending: self.nmi_pending,
            irq_pending: self.irq_pending,
        }
    }

    /// Load CPU state.
    pub fn load_state(&mut self, state: CpuState) {
        self.pc = state.pc;
        self.sp = state.sp;
        self.a = state.a;
        self.x = state.x;
        self.y = state.y;
        self.status = state.status;
        self.cycles = state.cycles;
        self.opcode = state.opcode;
        self.nmi_pending = state.nmi_pending;
        self.irq_pending = state.irq_pending;
    }
}

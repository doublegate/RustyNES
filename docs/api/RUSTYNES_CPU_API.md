# RustyNES CPU Crate API Reference

**Crate:** `rustynes-cpu`
**Version:** 0.1.0
**License:** MIT/Apache-2.0

The `rustynes-cpu` crate provides a standalone, cycle-accurate MOS 6502 CPU implementation designed for NES emulation but reusable for other 6502-based systems (Commodore 64, Apple II, Atari 2600, etc.).

---

## Table of Contents

- [Quick Start](#quick-start)
- [Core Types](#core-types)
- [CPU Struct](#cpu-struct)
- [Bus Trait](#bus-trait)
- [Status Register](#status-register)
- [Addressing Modes](#addressing-modes)
- [Interrupts](#interrupts)
- [Debug Interface](#debug-interface)
- [Standalone Usage](#standalone-usage)
- [Error Handling](#error-handling)
- [Examples](#examples)

---

## Quick Start

```rust
use rustynes_cpu::{Cpu, Bus, StatusFlags};

// Implement the Bus trait for your memory system
struct SimpleBus {
    ram: [u8; 65536],
}

impl Bus for SimpleBus {
    fn read(&mut self, addr: u16) -> u8 {
        self.ram[addr as usize]
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.ram[addr as usize] = value;
    }
}

fn main() {
    let mut bus = SimpleBus { ram: [0; 65536] };
    let mut cpu = Cpu::new();

    // Load program at $8000
    bus.ram[0x8000] = 0xA9; // LDA #$42
    bus.ram[0x8001] = 0x42;
    bus.ram[0x8002] = 0x85; // STA $00
    bus.ram[0x8003] = 0x00;
    bus.ram[0x8004] = 0x00; // BRK

    // Set reset vector
    bus.ram[0xFFFC] = 0x00;
    bus.ram[0xFFFD] = 0x80;

    // Reset and run
    cpu.reset(&mut bus);

    while !cpu.is_halted() {
        cpu.step(&mut bus);
    }
}
```

---

## Core Types

### Address Types

```rust
/// 16-bit memory address
pub type Address = u16;

/// 8-bit data value
pub type Byte = u8;

/// CPU cycle count (unsigned)
pub type Cycles = u64;
```

### Register Set

```rust
/// CPU register file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registers {
    /// Accumulator (A)
    pub a: u8,

    /// Index Register X
    pub x: u8,

    /// Index Register Y
    pub y: u8,

    /// Stack Pointer (always $01xx)
    pub sp: u8,

    /// Program Counter (16-bit)
    pub pc: u16,

    /// Processor Status (flags)
    pub p: StatusFlags,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            pc: 0,
            p: StatusFlags::INTERRUPT_DISABLE | StatusFlags::UNUSED,
        }
    }
}
```

---

## CPU Struct

### Definition

```rust
/// MOS 6502 CPU emulator
pub struct Cpu {
    /// CPU registers
    pub regs: Registers,

    /// Total cycles executed
    cycles: Cycles,

    /// Cycles remaining for current instruction
    cycles_remaining: u8,

    /// Pending NMI flag
    nmi_pending: bool,

    /// Pending IRQ flag
    irq_pending: bool,

    /// CPU halted (JAM instruction)
    halted: bool,
}
```

### Constructor

```rust
impl Cpu {
    /// Create a new CPU in reset state
    pub fn new() -> Self {
        Self {
            regs: Registers::default(),
            cycles: 0,
            cycles_remaining: 0,
            nmi_pending: false,
            irq_pending: false,
            halted: false,
        }
    }

    /// Create CPU with specific initial state (for testing)
    pub fn with_state(regs: Registers) -> Self {
        Self {
            regs,
            cycles: 0,
            cycles_remaining: 0,
            nmi_pending: false,
            irq_pending: false,
            halted: false,
        }
    }
}
```

### Core Methods

```rust
impl Cpu {
    /// Execute CPU reset sequence (7 cycles)
    ///
    /// 1. Set interrupt disable flag
    /// 2. Decrement stack pointer by 3 (no writes)
    /// 3. Read reset vector from $FFFC/$FFFD
    /// 4. Jump to reset vector address
    pub fn reset(&mut self, bus: &mut impl Bus) {
        self.regs.p.insert(StatusFlags::INTERRUPT_DISABLE);
        self.regs.sp = self.regs.sp.wrapping_sub(3);

        let lo = bus.read(0xFFFC);
        let hi = bus.read(0xFFFD);
        self.regs.pc = u16::from_le_bytes([lo, hi]);

        self.cycles = 7;
        self.halted = false;
    }

    /// Execute one CPU instruction
    ///
    /// Returns the number of cycles consumed by this instruction.
    pub fn step(&mut self, bus: &mut impl Bus) -> u8 {
        if self.halted {
            return 1;
        }

        // Handle pending interrupts
        if self.nmi_pending {
            self.handle_nmi(bus);
            self.nmi_pending = false;
            return 7;
        }

        if self.irq_pending && !self.regs.p.contains(StatusFlags::INTERRUPT_DISABLE) {
            self.handle_irq(bus);
            return 7;
        }

        // Fetch and execute instruction
        let opcode = self.fetch_byte(bus);
        let cycles = self.execute(bus, opcode);

        self.cycles += cycles as u64;
        cycles
    }

    /// Execute multiple cycles (for synchronization)
    ///
    /// Returns actual cycles executed (may exceed target).
    pub fn run_cycles(&mut self, bus: &mut impl Bus, target_cycles: u64) -> u64 {
        let start = self.cycles;
        while self.cycles - start < target_cycles && !self.halted {
            self.step(bus);
        }
        self.cycles - start
    }

    /// Get total cycles executed since creation
    pub fn total_cycles(&self) -> Cycles {
        self.cycles
    }

    /// Check if CPU is halted (JAM instruction)
    pub fn is_halted(&self) -> bool {
        self.halted
    }
}
```

### Interrupt Methods

```rust
impl Cpu {
    /// Trigger NMI (edge-triggered, highest priority)
    pub fn trigger_nmi(&mut self) {
        self.nmi_pending = true;
    }

    /// Set IRQ line state (level-triggered)
    pub fn set_irq(&mut self, active: bool) {
        self.irq_pending = active;
    }

    /// Check if NMI is pending
    pub fn nmi_pending(&self) -> bool {
        self.nmi_pending
    }

    /// Check if IRQ is pending (and not masked)
    pub fn irq_pending(&self) -> bool {
        self.irq_pending && !self.regs.p.contains(StatusFlags::INTERRUPT_DISABLE)
    }
}
```

---

## Bus Trait

The CPU communicates with the outside world through the `Bus` trait.

### Definition

```rust
/// Memory bus interface for CPU
pub trait Bus {
    /// Read a byte from the specified address
    fn read(&mut self, addr: u16) -> u8;

    /// Write a byte to the specified address
    fn write(&mut self, addr: u16, value: u8);

    /// Read without side effects (for debugging)
    fn peek(&self, addr: u16) -> u8 {
        // Default: return 0 (implement for proper debugging)
        0
    }

    /// Called on each CPU cycle (for cycle-accurate PPU/APU sync)
    fn tick(&mut self) {
        // Optional: implement for cycle-level synchronization
    }
}
```

### NES Bus Implementation Example

```rust
pub struct NesBus {
    ram: [u8; 0x800],           // 2KB internal RAM
    ppu: Ppu,                    // PPU (registers at $2000-$2007)
    apu: Apu,                    // APU (registers at $4000-$4017)
    mapper: Box<dyn Mapper>,     // Cartridge mapper
    controller1: Controller,     // Player 1 controller
    controller2: Controller,     // Player 2 controller
}

impl Bus for NesBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => self.ppu.read_register(addr & 0x0007),
            0x4000..=0x4013 => self.apu.read_register(addr),
            0x4014 => 0, // OAM DMA (write-only)
            0x4015 => self.apu.read_status(),
            0x4016 => self.controller1.read(),
            0x4017 => self.controller2.read(),
            0x4018..=0x401F => 0, // Normally disabled
            0x4020..=0xFFFF => self.mapper.read_prg(addr),
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize] = value,
            0x2000..=0x3FFF => self.ppu.write_register(addr & 0x0007, value),
            0x4000..=0x4013 => self.apu.write_register(addr, value),
            0x4014 => self.trigger_oam_dma(value),
            0x4015 => self.apu.write_status(value),
            0x4016 => self.controller1.write(value),
            0x4017 => self.apu.write_frame_counter(value),
            0x4018..=0x401F => { /* Disabled */ }
            0x4020..=0xFFFF => self.mapper.write_prg(addr, value),
        }
    }

    fn tick(&mut self) {
        // Run 3 PPU cycles per CPU cycle
        self.ppu.tick();
        self.ppu.tick();
        self.ppu.tick();

        // Run APU cycle
        self.apu.tick();
    }
}
```

---

## Status Register

### StatusFlags Bitflags

```rust
use bitflags::bitflags;

bitflags! {
    /// CPU status register flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct StatusFlags: u8 {
        /// Carry flag (C)
        const CARRY             = 0b0000_0001;
        /// Zero flag (Z)
        const ZERO              = 0b0000_0010;
        /// Interrupt disable (I)
        const INTERRUPT_DISABLE = 0b0000_0100;
        /// Decimal mode (D) - ignored on NES
        const DECIMAL           = 0b0000_1000;
        /// Break command (B) - only set on stack
        const BREAK             = 0b0001_0000;
        /// Unused (always 1)
        const UNUSED            = 0b0010_0000;
        /// Overflow flag (V)
        const OVERFLOW          = 0b0100_0000;
        /// Negative flag (N)
        const NEGATIVE          = 0b1000_0000;
    }
}
```

### Status Methods

```rust
impl StatusFlags {
    /// Set or clear a flag based on a condition
    pub fn set_flag(&mut self, flag: StatusFlags, condition: bool) {
        if condition {
            self.insert(flag);
        } else {
            self.remove(flag);
        }
    }

    /// Update Zero and Negative flags based on a value
    pub fn update_zn(&mut self, value: u8) {
        self.set_flag(StatusFlags::ZERO, value == 0);
        self.set_flag(StatusFlags::NEGATIVE, value & 0x80 != 0);
    }

    /// Convert to byte for stack push (with B and U set)
    pub fn to_stack_byte(&self, brk: bool) -> u8 {
        let mut byte = self.bits();
        byte |= StatusFlags::UNUSED.bits();
        if brk {
            byte |= StatusFlags::BREAK.bits();
        }
        byte
    }

    /// Convert from byte popped from stack (B ignored)
    pub fn from_stack_byte(byte: u8) -> Self {
        Self::from_bits_truncate(byte) | StatusFlags::UNUSED
    }
}
```

---

## Addressing Modes

```rust
/// CPU addressing modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressingMode {
    /// No operand (implied)
    Implied,
    /// Accumulator register
    Accumulator,
    /// 8-bit immediate value
    Immediate,
    /// Zero page address ($00-$FF)
    ZeroPage,
    /// Zero page + X register
    ZeroPageX,
    /// Zero page + Y register
    ZeroPageY,
    /// 16-bit absolute address
    Absolute,
    /// Absolute + X register
    AbsoluteX,
    /// Absolute + Y register
    AbsoluteY,
    /// Indirect (JMP only)
    Indirect,
    /// Indexed indirect (X)
    IndexedIndirectX,
    /// Indirect indexed (Y)
    IndirectIndexedY,
    /// Relative branch offset
    Relative,
}

impl AddressingMode {
    /// Get operand size in bytes
    pub fn operand_size(&self) -> u8 {
        match self {
            Self::Implied | Self::Accumulator => 0,
            Self::Immediate | Self::ZeroPage | Self::ZeroPageX |
            Self::ZeroPageY | Self::IndexedIndirectX |
            Self::IndirectIndexedY | Self::Relative => 1,
            Self::Absolute | Self::AbsoluteX | Self::AbsoluteY |
            Self::Indirect => 2,
        }
    }

    /// Check if this mode can have a page crossing penalty
    pub fn can_page_cross(&self) -> bool {
        matches!(
            self,
            Self::AbsoluteX | Self::AbsoluteY |
            Self::IndirectIndexedY | Self::Relative
        )
    }
}
```

---

## Interrupts

### Interrupt Handling

```rust
impl Cpu {
    /// Handle NMI (Non-Maskable Interrupt)
    fn handle_nmi(&mut self, bus: &mut impl Bus) {
        // Push PC and status
        self.push_word(bus, self.regs.pc);
        self.push_byte(bus, self.regs.p.to_stack_byte(false));

        // Set interrupt disable
        self.regs.p.insert(StatusFlags::INTERRUPT_DISABLE);

        // Read NMI vector
        let lo = bus.read(0xFFFA);
        let hi = bus.read(0xFFFB);
        self.regs.pc = u16::from_le_bytes([lo, hi]);
    }

    /// Handle IRQ (Interrupt Request)
    fn handle_irq(&mut self, bus: &mut impl Bus) {
        // Push PC and status
        self.push_word(bus, self.regs.pc);
        self.push_byte(bus, self.regs.p.to_stack_byte(false));

        // Set interrupt disable
        self.regs.p.insert(StatusFlags::INTERRUPT_DISABLE);

        // Read IRQ vector
        let lo = bus.read(0xFFFE);
        let hi = bus.read(0xFFFF);
        self.regs.pc = u16::from_le_bytes([lo, hi]);
    }

    /// Handle BRK instruction
    fn execute_brk(&mut self, bus: &mut impl Bus) {
        self.regs.pc = self.regs.pc.wrapping_add(1);

        // Push PC and status (with B flag set)
        self.push_word(bus, self.regs.pc);
        self.push_byte(bus, self.regs.p.to_stack_byte(true));

        // Set interrupt disable
        self.regs.p.insert(StatusFlags::INTERRUPT_DISABLE);

        // Read IRQ/BRK vector
        let lo = bus.read(0xFFFE);
        let hi = bus.read(0xFFFF);
        self.regs.pc = u16::from_le_bytes([lo, hi]);
    }
}
```

### Interrupt Vectors

| Vector | Address | Purpose |
|--------|---------|---------|
| NMI | $FFFA-$FFFB | Non-maskable interrupt |
| RESET | $FFFC-$FFFD | Reset/power-on |
| IRQ/BRK | $FFFE-$FFFF | IRQ and BRK instruction |

---

## Debug Interface

### Disassembly

```rust
/// Disassembled instruction
#[derive(Debug, Clone)]
pub struct Disassembly {
    /// Address of instruction
    pub addr: u16,
    /// Raw bytes
    pub bytes: Vec<u8>,
    /// Mnemonic (e.g., "LDA")
    pub mnemonic: String,
    /// Operand string (e.g., "$4400,X")
    pub operand: String,
    /// Addressing mode
    pub mode: AddressingMode,
}

impl Cpu {
    /// Disassemble instruction at address
    pub fn disassemble(&self, bus: &impl Bus, addr: u16) -> Disassembly {
        let opcode = bus.peek(addr);
        let (mnemonic, mode) = Self::decode_opcode(opcode);
        let size = mode.operand_size() as u16 + 1;

        let mut bytes = Vec::with_capacity(size as usize);
        for i in 0..size {
            bytes.push(bus.peek(addr.wrapping_add(i)));
        }

        let operand = self.format_operand(bus, addr + 1, mode);

        Disassembly {
            addr,
            bytes,
            mnemonic: mnemonic.to_string(),
            operand,
            mode,
        }
    }

    /// Disassemble range of instructions
    pub fn disassemble_range(
        &self,
        bus: &impl Bus,
        start: u16,
        count: usize,
    ) -> Vec<Disassembly> {
        let mut result = Vec::with_capacity(count);
        let mut addr = start;

        for _ in 0..count {
            let disasm = self.disassemble(bus, addr);
            addr = addr.wrapping_add(disasm.bytes.len() as u16);
            result.push(disasm);
        }

        result
    }
}
```

### Trace Logging

```rust
/// CPU trace entry (nestest.log format)
#[derive(Debug, Clone)]
pub struct TraceEntry {
    pub pc: u16,
    pub opcode: u8,
    pub operand_bytes: [u8; 2],
    pub disassembly: String,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub p: u8,
    pub sp: u8,
    pub cycles: u64,
}

impl Cpu {
    /// Generate trace entry for current instruction
    pub fn trace(&self, bus: &impl Bus) -> TraceEntry {
        let disasm = self.disassemble(bus, self.regs.pc);

        TraceEntry {
            pc: self.regs.pc,
            opcode: disasm.bytes[0],
            operand_bytes: [
                disasm.bytes.get(1).copied().unwrap_or(0),
                disasm.bytes.get(2).copied().unwrap_or(0),
            ],
            disassembly: format!("{} {}", disasm.mnemonic, disasm.operand),
            a: self.regs.a,
            x: self.regs.x,
            y: self.regs.y,
            p: self.regs.p.bits(),
            sp: self.regs.sp,
            cycles: self.cycles,
        }
    }

    /// Format trace entry as nestest.log line
    pub fn format_nestest_log(&self, bus: &impl Bus) -> String {
        let entry = self.trace(bus);
        format!(
            "{:04X}  {:02X} {:02X} {:02X}  {:28}A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X} CYC:{}",
            entry.pc,
            entry.opcode,
            entry.operand_bytes[0],
            entry.operand_bytes[1],
            entry.disassembly,
            entry.a,
            entry.x,
            entry.y,
            entry.p,
            entry.sp,
            entry.cycles % 341, // PPU cycle within scanline
        )
    }
}
```

### State Inspection

```rust
impl Cpu {
    /// Get current register state
    pub fn registers(&self) -> &Registers {
        &self.regs
    }

    /// Get mutable register access (for debugging/cheats)
    pub fn registers_mut(&mut self) -> &mut Registers {
        &mut self.regs
    }

    /// Get stack contents
    pub fn get_stack(&self, bus: &impl Bus) -> Vec<u8> {
        let mut stack = Vec::new();
        let start = (self.regs.sp as u16).wrapping_add(1);
        for offset in 0..=(0xFF - self.regs.sp) {
            let addr = 0x0100 | start.wrapping_add(offset as u16);
            stack.push(bus.peek(addr));
        }
        stack
    }
}
```

---

## Standalone Usage

### For Commodore 64 Emulation

```rust
use rustynes_cpu::{Cpu, Bus, StatusFlags};

struct C64Bus {
    ram: [u8; 65536],
    kernal_rom: [u8; 8192],
    basic_rom: [u8; 8192],
    char_rom: [u8; 4096],
    io_enabled: bool,
    bank_config: u8,
}

impl Bus for C64Bus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000 => self.bank_config, // Processor port
            0x0001 => self.bank_config, // Processor port direction
            0x0002..=0x9FFF => self.ram[addr as usize],
            0xA000..=0xBFFF => {
                if self.bank_config & 0x03 != 0 {
                    self.basic_rom[(addr - 0xA000) as usize]
                } else {
                    self.ram[addr as usize]
                }
            }
            0xC000..=0xCFFF => self.ram[addr as usize],
            0xD000..=0xDFFF => {
                if self.io_enabled {
                    self.read_io(addr)
                } else if self.bank_config & 0x04 != 0 {
                    self.char_rom[(addr - 0xD000) as usize]
                } else {
                    self.ram[addr as usize]
                }
            }
            0xE000..=0xFFFF => {
                if self.bank_config & 0x02 != 0 {
                    self.kernal_rom[(addr - 0xE000) as usize]
                } else {
                    self.ram[addr as usize]
                }
            }
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000 | 0x0001 => self.bank_config = value,
            0xD000..=0xDFFF if self.io_enabled => self.write_io(addr, value),
            _ => self.ram[addr as usize] = value,
        }
    }
}
```

### For Apple II Emulation

```rust
struct AppleIIBus {
    ram: [u8; 49152],        // 48KB main RAM
    aux_ram: [u8; 49152],    // 48KB aux RAM (IIe)
    rom: [u8; 12288],        // 12KB ROM
    language_card: [u8; 16384],
    soft_switches: SoftSwitches,
}

impl Bus for AppleIIBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0xBFFF => {
                if self.soft_switches.aux_read {
                    self.aux_ram[addr as usize]
                } else {
                    self.ram[addr as usize]
                }
            }
            0xC000..=0xC0FF => self.read_io(addr),
            0xC100..=0xCFFF => self.read_slot_rom(addr),
            0xD000..=0xFFFF => {
                if self.soft_switches.lc_read_enable {
                    self.language_card[(addr - 0xD000) as usize]
                } else {
                    self.rom[(addr - 0xD000) as usize]
                }
            }
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0xBFFF => {
                if self.soft_switches.aux_write {
                    self.aux_ram[addr as usize] = value;
                } else {
                    self.ram[addr as usize] = value;
                }
            }
            0xC000..=0xC0FF => self.write_io(addr, value),
            0xD000..=0xFFFF if self.soft_switches.lc_write_enable => {
                self.language_card[(addr - 0xD000) as usize] = value;
            }
            _ => { /* ROM/write-protected */ }
        }
    }
}
```

---

## Error Handling

```rust
use thiserror::Error;

/// CPU error types
#[derive(Debug, Error)]
pub enum CpuError {
    #[error("Invalid opcode: ${0:02X}")]
    InvalidOpcode(u8),

    #[error("CPU halted (JAM instruction at ${0:04X})")]
    Halted(u16),

    #[error("Stack overflow")]
    StackOverflow,

    #[error("Stack underflow")]
    StackUnderflow,
}

/// Result type for CPU operations
pub type CpuResult<T> = Result<T, CpuError>;
```

---

## Examples

### Running nestest.nes

```rust
use rustynes_cpu::{Cpu, Bus};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};

fn run_nestest(rom_path: &str, log_path: &str) -> std::io::Result<()> {
    let mut bus = load_nestest_rom(rom_path)?;
    let mut cpu = Cpu::new();

    // Set PC to automated test start
    cpu.registers_mut().pc = 0xC000;

    let mut log_file = File::create(log_path)?;

    // Run until test completion ($C66E)
    while cpu.registers().pc != 0xC66E {
        // Write trace line before execution
        writeln!(log_file, "{}", cpu.format_nestest_log(&bus))?;
        cpu.step(&mut bus);
    }

    // Check result at $02 (0x00 = pass)
    let result = bus.read(0x02);
    if result == 0 {
        println!("nestest PASSED");
    } else {
        println!("nestest FAILED with code: {:02X}", result);
    }

    Ok(())
}
```

### Cycle-Accurate Execution

```rust
use rustynes_cpu::{Cpu, Bus};

/// Run CPU synchronized with other components
fn run_synchronized(
    cpu: &mut Cpu,
    bus: &mut impl Bus,
    frame_cycles: u64,
) {
    let target = cpu.total_cycles() + frame_cycles;

    while cpu.total_cycles() < target {
        let cycles = cpu.step(bus);

        // Synchronize with PPU (3:1 ratio)
        for _ in 0..cycles * 3 {
            // PPU tick is called in bus.tick()
        }
    }
}
```

---

## References

- [6502.org](http://www.6502.org/) - 6502 microprocessor resource
- [Visual6502](http://visual6502.org/) - Transistor-level simulation
- [NESdev Wiki: CPU](https://www.nesdev.org/wiki/CPU) - NES-specific behavior
- [CPU_6502_SPECIFICATION.md](../cpu/CPU_6502_SPECIFICATION.md) - Complete opcode reference

---

**Related Documents:**
- [CORE_API.md](CORE_API.md) - Main emulator API
- [CPU_6502_SPECIFICATION.md](../cpu/CPU_6502_SPECIFICATION.md)
- [CPU_TIMING_REFERENCE.md](../cpu/CPU_TIMING_REFERENCE.md)

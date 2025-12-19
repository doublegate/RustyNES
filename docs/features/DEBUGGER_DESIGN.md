# RustyNES Integrated Debugger Design

Comprehensive debugger architecture for NES development, reverse engineering, and emulator testing.

## Overview

The RustyNES debugger provides professional-grade debugging capabilities inspired by Mesen's debugging suite, enabling ROM hackers, homebrew developers, and accuracy testers to inspect and manipulate emulator state at cycle-level precision.

## Architecture

### Module Structure

```
crates/rustynes-debugger/src/
├── mod.rs              # Public API
├── breakpoints.rs      # Breakpoint system
├── watchpoints.rs      # Memory watch system
├── disassembler.rs     # 6502 disassembly
├── trace.rs            # Instruction tracing
├── memory_viewer.rs    # Memory inspection
├── state_inspector.rs  # CPU/PPU/APU state
├── profiler.rs         # Performance analysis
├── labels.rs           # Symbol management
├── scripted.rs         # Conditional breakpoints
└── ui/
    ├── mod.rs
    ├── cpu_panel.rs
    ├── ppu_panel.rs
    ├── memory_panel.rs
    └── trace_panel.rs
```

### Core Types

```rust
use std::collections::{HashMap, HashSet};

/// Main debugger controller
pub struct Debugger {
    /// Breakpoint manager
    breakpoints: BreakpointManager,
    /// Memory watchpoints
    watchpoints: WatchpointManager,
    /// Instruction tracer
    tracer: InstructionTracer,
    /// Symbol/label database
    labels: LabelDatabase,
    /// Disassembler instance
    disasm: Disassembler,
    /// Profiler for performance analysis
    profiler: Profiler,
    /// Current debug state
    state: DebugState,
    /// Event log
    event_log: Vec<DebugEvent>,
}

/// Debug state machine
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DebugState {
    /// Normal run without debugging
    Running,
    /// Single-stepping through instructions
    Stepping,
    /// Paused at breakpoint or manually
    Paused,
    /// Running until specific condition
    RunningUntil(StopCondition),
}

/// Conditions that cause debugger to pause
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StopCondition {
    /// Run until specific PC address
    Address(u16),
    /// Run until scanline
    Scanline(u16),
    /// Run until frame number
    Frame(u64),
    /// Run until NMI
    Nmi,
    /// Run until IRQ
    Irq,
}

/// Debug event for logging
#[derive(Clone)]
pub struct DebugEvent {
    pub timestamp: u64,
    pub frame: u64,
    pub scanline: u16,
    pub cycle: u16,
    pub event_type: DebugEventType,
}

#[derive(Clone)]
pub enum DebugEventType {
    BreakpointHit(BreakpointId),
    WatchpointTriggered(WatchpointId, u8, u8), // old, new
    NmiOccurred,
    IrqOccurred,
    IllegalOpcode(u8),
    BankSwitch { mapper_reg: u8, old: u8, new: u8 },
}
```

## Breakpoint System

### Breakpoint Types

```rust
/// Unique breakpoint identifier
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BreakpointId(u32);

/// Breakpoint configuration
#[derive(Clone)]
pub struct Breakpoint {
    pub id: BreakpointId,
    pub enabled: bool,
    pub bp_type: BreakpointType,
    pub condition: Option<BreakpointCondition>,
    pub hit_count: u32,
    pub hit_count_target: Option<u32>,
    pub log_message: Option<String>,
    pub temporary: bool, // Auto-delete after hit
}

#[derive(Clone)]
pub enum BreakpointType {
    /// Break on program counter reaching address
    Program(u16),
    /// Break on program counter in range
    ProgramRange { start: u16, end: u16 },
    /// Break on specific opcode running
    Opcode(u8),
    /// Break on reading from address
    ReadAddress(u16),
    /// Break on writing to address
    WriteAddress(u16),
    /// Break on reading/writing address range
    AddressRange {
        start: u16,
        end: u16,
        access: AccessType,
    },
    /// Break on PPU address access
    PpuAddress(u16),
    /// Break on specific scanline
    Scanline(u16),
    /// Break on scanline and cycle
    ScanlineCycle { scanline: u16, cycle: u16 },
    /// Break on mapper register write
    MapperRegister(u8),
    /// Break on IRQ
    Irq,
    /// Break on NMI
    Nmi,
    /// Break on reset
    Reset,
}

#[derive(Clone, Copy)]
pub enum AccessType {
    Read,
    Write,
    ReadWrite,
}

/// Conditional expression for breakpoints
#[derive(Clone)]
pub enum BreakpointCondition {
    /// Simple register comparison
    Register(RegisterCondition),
    /// Memory value comparison
    Memory(MemoryCondition),
    /// Logical AND of conditions
    And(Box<BreakpointCondition>, Box<BreakpointCondition>),
    /// Logical OR of conditions
    Or(Box<BreakpointCondition>, Box<BreakpointCondition>),
    /// Scripted condition (Lua expression)
    Script(String),
}

#[derive(Clone)]
pub struct RegisterCondition {
    pub register: CpuRegister,
    pub comparison: Comparison,
    pub value: u8,
}

#[derive(Clone, Copy)]
pub enum CpuRegister {
    A, X, Y, S, P, PCLow, PCHigh,
}

#[derive(Clone, Copy)]
pub enum Comparison {
    Equal,
    NotEqual,
    LessThan,
    LessOrEqual,
    GreaterThan,
    GreaterOrEqual,
    BitSet(u8),    // value & mask != 0
    BitClear(u8),  // value & mask == 0
}

#[derive(Clone)]
pub struct MemoryCondition {
    pub address: u16,
    pub comparison: Comparison,
    pub value: u8,
}
```

### Breakpoint Manager

```rust
pub struct BreakpointManager {
    breakpoints: HashMap<BreakpointId, Breakpoint>,
    next_id: u32,
    /// Fast lookup tables for hot path
    program_breakpoints: HashSet<u16>,
    read_breakpoints: HashSet<u16>,
    write_breakpoints: HashSet<u16>,
    scanline_breakpoints: HashSet<u16>,
    opcode_breakpoints: HashSet<u8>,
    break_on_irq: bool,
    break_on_nmi: bool,
}

impl BreakpointManager {
    pub fn new() -> Self {
        Self {
            breakpoints: HashMap::new(),
            next_id: 1,
            program_breakpoints: HashSet::new(),
            read_breakpoints: HashSet::new(),
            write_breakpoints: HashSet::new(),
            scanline_breakpoints: HashSet::new(),
            opcode_breakpoints: HashSet::new(),
            break_on_irq: false,
            break_on_nmi: false,
        }
    }

    /// Add a new breakpoint
    pub fn add(&mut self, bp_type: BreakpointType) -> BreakpointId {
        let id = BreakpointId(self.next_id);
        self.next_id += 1;

        let breakpoint = Breakpoint {
            id,
            enabled: true,
            bp_type: bp_type.clone(),
            condition: None,
            hit_count: 0,
            hit_count_target: None,
            log_message: None,
            temporary: false,
        };

        self.update_lookup_tables(&bp_type, true);
        self.breakpoints.insert(id, breakpoint);
        id
    }

    /// Add breakpoint with condition
    pub fn add_conditional(
        &mut self,
        bp_type: BreakpointType,
        condition: BreakpointCondition,
    ) -> BreakpointId {
        let id = self.add(bp_type);
        if let Some(bp) = self.breakpoints.get_mut(&id) {
            bp.condition = Some(condition);
        }
        id
    }

    /// Remove a breakpoint
    pub fn remove(&mut self, id: BreakpointId) -> bool {
        if let Some(bp) = self.breakpoints.remove(&id) {
            self.update_lookup_tables(&bp.bp_type, false);
            true
        } else {
            false
        }
    }

    /// Enable or disable a breakpoint
    pub fn set_enabled(&mut self, id: BreakpointId, enabled: bool) {
        if let Some(bp) = self.breakpoints.get_mut(&id) {
            if bp.enabled != enabled {
                bp.enabled = enabled;
                self.update_lookup_tables(&bp.bp_type, enabled);
            }
        }
    }

    /// Fast check if address has program breakpoint
    #[inline]
    pub fn has_program_bp(&self, addr: u16) -> bool {
        self.program_breakpoints.contains(&addr)
    }

    /// Fast check if address has read breakpoint
    #[inline]
    pub fn has_read_bp(&self, addr: u16) -> bool {
        self.read_breakpoints.contains(&addr)
    }

    /// Fast check if address has write breakpoint
    #[inline]
    pub fn has_write_bp(&self, addr: u16) -> bool {
        self.write_breakpoints.contains(&addr)
    }

    /// Check if scanline has breakpoint
    #[inline]
    pub fn has_scanline_bp(&self, scanline: u16) -> bool {
        self.scanline_breakpoints.contains(&scanline)
    }

    /// Evaluate breakpoint condition
    pub fn evaluate_condition(
        &self,
        bp: &Breakpoint,
        cpu: &CpuState,
        bus: &Bus,
    ) -> bool {
        match &bp.condition {
            None => true,
            Some(cond) => self.evaluate_expr(cond, cpu, bus),
        }
    }

    fn evaluate_expr(
        &self,
        cond: &BreakpointCondition,
        cpu: &CpuState,
        bus: &Bus,
    ) -> bool {
        match cond {
            BreakpointCondition::Register(rc) => {
                let reg_val = match rc.register {
                    CpuRegister::A => cpu.a,
                    CpuRegister::X => cpu.x,
                    CpuRegister::Y => cpu.y,
                    CpuRegister::S => cpu.s,
                    CpuRegister::P => cpu.p,
                    CpuRegister::PCLow => (cpu.pc & 0xFF) as u8,
                    CpuRegister::PCHigh => (cpu.pc >> 8) as u8,
                };
                self.compare(reg_val, rc.comparison, rc.value)
            }
            BreakpointCondition::Memory(mc) => {
                let mem_val = bus.peek(mc.address);
                self.compare(mem_val, mc.comparison, mc.value)
            }
            BreakpointCondition::And(a, b) => {
                self.evaluate_expr(a, cpu, bus) && self.evaluate_expr(b, cpu, bus)
            }
            BreakpointCondition::Or(a, b) => {
                self.evaluate_expr(a, cpu, bus) || self.evaluate_expr(b, cpu, bus)
            }
            BreakpointCondition::Script(_expr) => {
                // Evaluated by Lua scripting engine
                true
            }
        }
    }

    fn compare(&self, value: u8, cmp: Comparison, target: u8) -> bool {
        match cmp {
            Comparison::Equal => value == target,
            Comparison::NotEqual => value != target,
            Comparison::LessThan => value < target,
            Comparison::LessOrEqual => value <= target,
            Comparison::GreaterThan => value > target,
            Comparison::GreaterOrEqual => value >= target,
            Comparison::BitSet(mask) => (value & mask) != 0,
            Comparison::BitClear(mask) => (value & mask) == 0,
        }
    }

    fn update_lookup_tables(&mut self, bp_type: &BreakpointType, add: bool) {
        match bp_type {
            BreakpointType::Program(addr) => {
                if add {
                    self.program_breakpoints.insert(*addr);
                } else {
                    self.program_breakpoints.remove(addr);
                }
            }
            BreakpointType::ReadAddress(addr) => {
                if add {
                    self.read_breakpoints.insert(*addr);
                } else {
                    self.read_breakpoints.remove(addr);
                }
            }
            BreakpointType::WriteAddress(addr) => {
                if add {
                    self.write_breakpoints.insert(*addr);
                } else {
                    self.write_breakpoints.remove(addr);
                }
            }
            BreakpointType::Scanline(sl) => {
                if add {
                    self.scanline_breakpoints.insert(*sl);
                } else {
                    self.scanline_breakpoints.remove(sl);
                }
            }
            BreakpointType::Opcode(op) => {
                if add {
                    self.opcode_breakpoints.insert(*op);
                } else {
                    self.opcode_breakpoints.remove(op);
                }
            }
            BreakpointType::Irq => self.break_on_irq = add,
            BreakpointType::Nmi => self.break_on_nmi = add,
            _ => {}
        }
    }
}
```

## Watchpoint System

```rust
/// Unique watchpoint identifier
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct WatchpointId(u32);

/// Memory watchpoint
#[derive(Clone)]
pub struct Watchpoint {
    pub id: WatchpointId,
    pub enabled: bool,
    pub address: u16,
    pub size: u16,          // Number of bytes to watch
    pub watch_type: WatchType,
    pub condition: Option<WatchCondition>,
    pub log_only: bool,     // Log but don't break
}

#[derive(Clone, Copy)]
pub enum WatchType {
    Read,
    Write,
    ReadWrite,
    Change, // Only break when value changes
}

#[derive(Clone)]
pub struct WatchCondition {
    pub comparison: Comparison,
    pub value: u8,
    pub compare_previous: bool, // Compare to previous value
}

pub struct WatchpointManager {
    watchpoints: HashMap<WatchpointId, Watchpoint>,
    next_id: u32,
    /// Previous values for change detection
    previous_values: HashMap<u16, u8>,
    /// Fast lookup by address
    address_map: HashMap<u16, Vec<WatchpointId>>,
}

impl WatchpointManager {
    pub fn new() -> Self {
        Self {
            watchpoints: HashMap::new(),
            next_id: 1,
            previous_values: HashMap::new(),
            address_map: HashMap::new(),
        }
    }

    /// Add watchpoint
    pub fn add(&mut self, address: u16, size: u16, watch_type: WatchType) -> WatchpointId {
        let id = WatchpointId(self.next_id);
        self.next_id += 1;

        let watchpoint = Watchpoint {
            id,
            enabled: true,
            address,
            size,
            watch_type,
            condition: None,
            log_only: false,
        };

        // Add to address lookup
        for offset in 0..size {
            let addr = address.wrapping_add(offset);
            self.address_map.entry(addr).or_default().push(id);
        }

        self.watchpoints.insert(id, watchpoint);
        id
    }

    /// Check if read should trigger watchpoint
    pub fn check_read(&self, address: u16) -> Option<&Watchpoint> {
        self.address_map.get(&address).and_then(|ids| {
            ids.iter().find_map(|id| {
                let wp = self.watchpoints.get(id)?;
                if wp.enabled && matches!(wp.watch_type, WatchType::Read | WatchType::ReadWrite) {
                    Some(wp)
                } else {
                    None
                }
            })
        })
    }

    /// Check if write should trigger watchpoint
    pub fn check_write(&mut self, address: u16, old_value: u8, new_value: u8) -> Option<&Watchpoint> {
        // Update previous value tracking
        self.previous_values.insert(address, old_value);

        self.address_map.get(&address).and_then(|ids| {
            ids.iter().find_map(|id| {
                let wp = self.watchpoints.get(id)?;
                if !wp.enabled {
                    return None;
                }

                let type_match = matches!(
                    wp.watch_type,
                    WatchType::Write | WatchType::ReadWrite | WatchType::Change
                );

                if !type_match {
                    return None;
                }

                // For Change type, only trigger if value actually changed
                if matches!(wp.watch_type, WatchType::Change) && old_value == new_value {
                    return None;
                }

                // Check condition if present
                if let Some(cond) = &wp.condition {
                    let compare_val = if cond.compare_previous { old_value } else { new_value };
                    let matches = match cond.comparison {
                        Comparison::Equal => compare_val == cond.value,
                        Comparison::NotEqual => compare_val != cond.value,
                        _ => true,
                    };
                    if !matches {
                        return None;
                    }
                }

                Some(wp)
            })
        })
    }
}
```

## 6502 Disassembler

```rust
/// Disassembler for 6502 code
pub struct Disassembler {
    /// Symbol database for labeled output
    labels: LabelDatabase,
}

/// Disassembled instruction
#[derive(Clone)]
pub struct DisassembledInstruction {
    pub address: u16,
    pub bytes: Vec<u8>,
    pub mnemonic: &'static str,
    pub operand: String,
    pub addressing_mode: AddressingMode,
    pub cycles: u8,
    pub label: Option<String>,
    pub comment: Option<String>,
}

#[derive(Clone, Copy)]
pub enum AddressingMode {
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

impl Disassembler {
    pub fn new(labels: LabelDatabase) -> Self {
        Self { labels }
    }

    /// Disassemble single instruction at address
    pub fn disassemble_at(&self, bus: &Bus, addr: u16) -> DisassembledInstruction {
        let opcode = bus.peek(addr);
        let (mnemonic, mode, cycles) = OPCODE_TABLE[opcode as usize];
        let operand_size = self.operand_size(mode);

        let mut bytes = vec![opcode];
        for i in 1..=operand_size {
            bytes.push(bus.peek(addr.wrapping_add(i)));
        }

        let operand = self.format_operand(mode, &bytes[1..], addr);
        let label = self.labels.get_label(addr);

        DisassembledInstruction {
            address: addr,
            bytes,
            mnemonic,
            operand,
            addressing_mode: mode,
            cycles,
            label,
            comment: None,
        }
    }

    /// Disassemble range of addresses
    pub fn disassemble_range(
        &self,
        bus: &Bus,
        start: u16,
        end: u16,
    ) -> Vec<DisassembledInstruction> {
        let mut result = Vec::new();
        let mut addr = start;

        while addr <= end {
            let instr = self.disassemble_at(bus, addr);
            let size = instr.bytes.len() as u16;
            result.push(instr);
            addr = addr.wrapping_add(size);
            if addr < start {
                break; // Wrapped around
            }
        }

        result
    }

    fn operand_size(&self, mode: AddressingMode) -> u16 {
        match mode {
            AddressingMode::Implied | AddressingMode::Accumulator => 0,
            AddressingMode::Immediate
            | AddressingMode::ZeroPage
            | AddressingMode::ZeroPageX
            | AddressingMode::ZeroPageY
            | AddressingMode::IndirectX
            | AddressingMode::IndirectY
            | AddressingMode::Relative => 1,
            AddressingMode::Absolute
            | AddressingMode::AbsoluteX
            | AddressingMode::AbsoluteY
            | AddressingMode::Indirect => 2,
        }
    }

    fn format_operand(&self, mode: AddressingMode, bytes: &[u8], addr: u16) -> String {
        match mode {
            AddressingMode::Implied => String::new(),
            AddressingMode::Accumulator => "A".to_string(),
            AddressingMode::Immediate => format!("#${:02X}", bytes[0]),
            AddressingMode::ZeroPage => {
                let zp = bytes[0];
                self.format_address(zp as u16, format!("${:02X}", zp))
            }
            AddressingMode::ZeroPageX => {
                let zp = bytes[0];
                self.format_address(zp as u16, format!("${:02X},X", zp))
            }
            AddressingMode::ZeroPageY => {
                let zp = bytes[0];
                self.format_address(zp as u16, format!("${:02X},Y", zp))
            }
            AddressingMode::Absolute => {
                let abs = u16::from_le_bytes([bytes[0], bytes[1]]);
                self.format_address(abs, format!("${:04X}", abs))
            }
            AddressingMode::AbsoluteX => {
                let abs = u16::from_le_bytes([bytes[0], bytes[1]]);
                self.format_address(abs, format!("${:04X},X", abs))
            }
            AddressingMode::AbsoluteY => {
                let abs = u16::from_le_bytes([bytes[0], bytes[1]]);
                self.format_address(abs, format!("${:04X},Y", abs))
            }
            AddressingMode::Indirect => {
                let abs = u16::from_le_bytes([bytes[0], bytes[1]]);
                format!("(${:04X})", abs)
            }
            AddressingMode::IndirectX => {
                format!("(${:02X},X)", bytes[0])
            }
            AddressingMode::IndirectY => {
                format!("(${:02X}),Y", bytes[0])
            }
            AddressingMode::Relative => {
                let offset = bytes[0] as i8;
                let target = addr.wrapping_add(2).wrapping_add(offset as u16);
                self.format_address(target, format!("${:04X}", target))
            }
        }
    }

    fn format_address(&self, addr: u16, default: String) -> String {
        if let Some(label) = self.labels.get_label(addr) {
            label
        } else {
            default
        }
    }
}

/// Opcode lookup table: (mnemonic, addressing mode, cycles)
const OPCODE_TABLE: [(&str, AddressingMode, u8); 256] = {
    use AddressingMode::*;
    [
        ("BRK", Implied, 7),     // 0x00
        ("ORA", IndirectX, 6),   // 0x01
        ("???", Implied, 2),     // 0x02 - Illegal
        ("???", Implied, 2),     // 0x03 - Illegal
        ("???", Implied, 2),     // 0x04 - Illegal
        ("ORA", ZeroPage, 3),    // 0x05
        ("ASL", ZeroPage, 5),    // 0x06
        ("???", Implied, 2),     // 0x07 - Illegal
        ("PHP", Implied, 3),     // 0x08
        ("ORA", Immediate, 2),   // 0x09
        ("ASL", Accumulator, 2), // 0x0A
        // ... remaining 245 entries
        ("???", Implied, 2),     // Placeholder
        ("???", Implied, 2),
        ("???", Implied, 2),
        ("???", Implied, 2),
        ("???", Implied, 2),
        // Continue for all 256 opcodes...
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
        ("???", Implied, 2), ("???", Implied, 2), ("???", Implied, 2),
    ]
};
```

## Instruction Tracer

```rust
use std::io::Write;
use std::fs::File;

/// Instruction trace entry
#[derive(Clone)]
pub struct TraceEntry {
    pub frame: u64,
    pub scanline: u16,
    pub cycle: u16,
    pub cpu_cycle: u64,
    pub pc: u16,
    pub opcode: u8,
    pub operand1: Option<u8>,
    pub operand2: Option<u8>,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub s: u8,
    pub p: u8,
    pub disassembly: String,
}

impl TraceEntry {
    /// Format as nestest.log compatible string
    pub fn format_nestest(&self) -> String {
        let op1 = self.operand1.map(|b| format!("{:02X}", b)).unwrap_or_default();
        let op2 = self.operand2.map(|b| format!("{:02X}", b)).unwrap_or_default();

        format!(
            "{:04X}  {:02X} {:2} {:2}  {:31} A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X} CYC:{:>3}",
            self.pc,
            self.opcode,
            op1,
            op2,
            self.disassembly,
            self.a,
            self.x,
            self.y,
            self.p,
            self.s,
            self.cycle
        )
    }

    /// Format as FCEUX trace format
    pub fn format_fceux(&self) -> String {
        format!(
            "f{} {}:{:03} {:04X}:{:02X}  {} A:{:02X} X:{:02X} Y:{:02X} S:{:02X} P:{}",
            self.frame,
            self.scanline,
            self.cycle,
            self.pc,
            self.opcode,
            self.disassembly,
            self.a,
            self.x,
            self.y,
            self.s,
            self.format_flags()
        )
    }

    fn format_flags(&self) -> String {
        let mut s = String::with_capacity(8);
        s.push(if self.p & 0x80 != 0 { 'N' } else { 'n' });
        s.push(if self.p & 0x40 != 0 { 'V' } else { 'v' });
        s.push(if self.p & 0x20 != 0 { 'U' } else { 'u' });
        s.push(if self.p & 0x10 != 0 { 'B' } else { 'b' });
        s.push(if self.p & 0x08 != 0 { 'D' } else { 'd' });
        s.push(if self.p & 0x04 != 0 { 'I' } else { 'i' });
        s.push(if self.p & 0x02 != 0 { 'Z' } else { 'z' });
        s.push(if self.p & 0x01 != 0 { 'C' } else { 'c' });
        s
    }
}

/// Instruction tracer with configurable output
pub struct InstructionTracer {
    enabled: bool,
    entries: Vec<TraceEntry>,
    max_entries: usize,
    output_file: Option<File>,
    format: TraceFormat,
    /// Filter by PC range
    pc_filter: Option<(u16, u16)>,
    /// Filter by scanline range
    scanline_filter: Option<(u16, u16)>,
}

#[derive(Clone, Copy)]
pub enum TraceFormat {
    Nestest,
    Fceux,
    Mesen,
    Custom,
}

impl InstructionTracer {
    pub fn new(max_entries: usize) -> Self {
        Self {
            enabled: false,
            entries: Vec::with_capacity(max_entries.min(100_000)),
            max_entries,
            output_file: None,
            format: TraceFormat::Nestest,
            pc_filter: None,
            scanline_filter: None,
        }
    }

    /// Enable tracing
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable tracing
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Set output file for streaming traces
    pub fn set_output_file(&mut self, path: &str) -> std::io::Result<()> {
        self.output_file = Some(File::create(path)?);
        Ok(())
    }

    /// Set PC address filter
    pub fn set_pc_filter(&mut self, start: u16, end: u16) {
        self.pc_filter = Some((start, end));
    }

    /// Clear PC filter
    pub fn clear_pc_filter(&mut self) {
        self.pc_filter = None;
    }

    /// Record trace entry
    pub fn record(&mut self, entry: TraceEntry) {
        if !self.enabled {
            return;
        }

        // Apply PC filter
        if let Some((start, end)) = self.pc_filter {
            if entry.pc < start || entry.pc > end {
                return;
            }
        }

        // Apply scanline filter
        if let Some((start, end)) = self.scanline_filter {
            if entry.scanline < start || entry.scanline > end {
                return;
            }
        }

        // Write to file if streaming
        if let Some(file) = &mut self.output_file {
            let line = match self.format {
                TraceFormat::Nestest => entry.format_nestest(),
                TraceFormat::Fceux => entry.format_fceux(),
                _ => entry.format_nestest(),
            };
            let _ = writeln!(file, "{}", line);
        }

        // Store in memory buffer
        if self.entries.len() >= self.max_entries {
            self.entries.remove(0);
        }
        self.entries.push(entry);
    }

    /// Get recent trace entries
    pub fn get_entries(&self, count: usize) -> &[TraceEntry] {
        let start = self.entries.len().saturating_sub(count);
        &self.entries[start..]
    }

    /// Clear trace buffer
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Export trace to file
    pub fn export(&self, path: &str) -> std::io::Result<()> {
        let mut file = File::create(path)?;
        for entry in &self.entries {
            let line = match self.format {
                TraceFormat::Nestest => entry.format_nestest(),
                TraceFormat::Fceux => entry.format_fceux(),
                _ => entry.format_nestest(),
            };
            writeln!(file, "{}", line)?;
        }
        Ok(())
    }
}
```

## Memory Viewer

```rust
/// Memory viewer for debugging
pub struct MemoryViewer {
    /// Currently selected memory region
    region: MemoryRegion,
    /// View offset
    offset: u16,
    /// Bytes per row
    columns: usize,
    /// Highlight changed bytes
    highlight_changes: bool,
    /// Previous memory state for diff
    previous: Vec<u8>,
    /// Search results
    search_results: Vec<u16>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MemoryRegion {
    CpuRam,          // $0000-$07FF (mirrored to $1FFF)
    PpuRegisters,    // $2000-$2007 (mirrored to $3FFF)
    ApuIo,           // $4000-$401F
    Cartridge,       // $4020-$FFFF
    ChrRom,          // PPU $0000-$1FFF
    Nametables,      // PPU $2000-$3EFF
    Palettes,        // PPU $3F00-$3F1F
    OamMemory,       // 256 bytes sprite data
    FullCpu,         // Full $0000-$FFFF view
}

impl MemoryViewer {
    pub fn new() -> Self {
        Self {
            region: MemoryRegion::CpuRam,
            offset: 0,
            columns: 16,
            highlight_changes: true,
            previous: Vec::new(),
            search_results: Vec::new(),
        }
    }

    /// Get address range for current region
    pub fn get_range(&self) -> (u16, u16) {
        match self.region {
            MemoryRegion::CpuRam => (0x0000, 0x07FF),
            MemoryRegion::PpuRegisters => (0x2000, 0x2007),
            MemoryRegion::ApuIo => (0x4000, 0x401F),
            MemoryRegion::Cartridge => (0x4020, 0xFFFF),
            MemoryRegion::ChrRom => (0x0000, 0x1FFF),
            MemoryRegion::Nametables => (0x2000, 0x3EFF),
            MemoryRegion::Palettes => (0x3F00, 0x3F1F),
            MemoryRegion::OamMemory => (0x0000, 0x00FF),
            MemoryRegion::FullCpu => (0x0000, 0xFFFF),
        }
    }

    /// Format memory as hex dump
    pub fn format_hex_dump(&self, bus: &Bus, start: u16, rows: usize) -> Vec<String> {
        let mut result = Vec::with_capacity(rows);
        let mut addr = start;

        for _ in 0..rows {
            let mut line = format!("{:04X}: ", addr);
            let mut ascii = String::new();

            for col in 0..self.columns {
                let byte = bus.peek(addr.wrapping_add(col as u16));
                line.push_str(&format!("{:02X} ", byte));

                // ASCII representation
                if byte >= 0x20 && byte < 0x7F {
                    ascii.push(byte as char);
                } else {
                    ascii.push('.');
                }
            }

            line.push_str(" |");
            line.push_str(&ascii);
            line.push('|');
            result.push(line);

            addr = addr.wrapping_add(self.columns as u16);
        }

        result
    }

    /// Search for byte pattern
    pub fn search(&mut self, bus: &Bus, pattern: &[u8]) -> usize {
        self.search_results.clear();
        let (start, end) = self.get_range();

        let mut addr = start;
        while addr <= end.saturating_sub(pattern.len() as u16) {
            let mut matches = true;
            for (i, &expected) in pattern.iter().enumerate() {
                if bus.peek(addr.wrapping_add(i as u16)) != expected {
                    matches = false;
                    break;
                }
            }

            if matches {
                self.search_results.push(addr);
            }

            addr = addr.wrapping_add(1);
            if addr == 0 {
                break; // Wrapped
            }
        }

        self.search_results.len()
    }

    /// Search for value with comparison
    pub fn search_value(&mut self, bus: &Bus, cmp: Comparison, value: u8) -> usize {
        self.search_results.clear();
        let (start, end) = self.get_range();

        let mut addr = start;
        while addr <= end {
            let byte = bus.peek(addr);
            let matches = match cmp {
                Comparison::Equal => byte == value,
                Comparison::NotEqual => byte != value,
                Comparison::LessThan => byte < value,
                Comparison::GreaterThan => byte > value,
                _ => false,
            };

            if matches {
                self.search_results.push(addr);
            }

            addr = addr.wrapping_add(1);
            if addr == 0 {
                break;
            }
        }

        self.search_results.len()
    }

    /// Capture current state for diff
    pub fn capture_state(&mut self, bus: &Bus) {
        let (start, end) = self.get_range();
        self.previous.clear();

        let mut addr = start;
        while addr <= end {
            self.previous.push(bus.peek(addr));
            addr = addr.wrapping_add(1);
            if addr == 0 {
                break;
            }
        }
    }

    /// Find changed bytes since last capture
    pub fn find_changes(&self, bus: &Bus) -> Vec<(u16, u8, u8)> {
        let (start, _) = self.get_range();
        let mut changes = Vec::new();

        for (i, &old) in self.previous.iter().enumerate() {
            let addr = start.wrapping_add(i as u16);
            let new = bus.peek(addr);
            if old != new {
                changes.push((addr, old, new));
            }
        }

        changes
    }
}
```

## State Inspector

```rust
/// CPU state snapshot for debugging
#[derive(Clone)]
pub struct CpuState {
    pub pc: u16,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub s: u8,
    pub p: u8,
    pub cycle: u64,
    pub nmi_pending: bool,
    pub irq_pending: bool,
}

impl CpuState {
    /// Format status register as string
    pub fn format_flags(&self) -> String {
        format!(
            "{}{}--{}{}{}{}",
            if self.p & 0x80 != 0 { 'N' } else { '-' },
            if self.p & 0x40 != 0 { 'V' } else { '-' },
            if self.p & 0x08 != 0 { 'D' } else { '-' },
            if self.p & 0x04 != 0 { 'I' } else { '-' },
            if self.p & 0x02 != 0 { 'Z' } else { '-' },
            if self.p & 0x01 != 0 { 'C' } else { '-' },
        )
    }
}

/// PPU state snapshot
#[derive(Clone)]
pub struct PpuState {
    pub v: u16,           // Current VRAM address
    pub t: u16,           // Temporary VRAM address
    pub x: u8,            // Fine X scroll
    pub w: bool,          // Write toggle
    pub ctrl: u8,         // $2000
    pub mask: u8,         // $2001
    pub status: u8,       // $2002
    pub oam_addr: u8,     // $2003
    pub scanline: u16,
    pub cycle: u16,
    pub frame: u64,
    pub nmi_occurred: bool,
    pub sprite_0_hit: bool,
    pub sprite_overflow: bool,
}

impl PpuState {
    pub fn format_v_register(&self) -> String {
        let fine_y = (self.v >> 12) & 0x07;
        let nt = (self.v >> 10) & 0x03;
        let coarse_y = (self.v >> 5) & 0x1F;
        let coarse_x = self.v & 0x1F;
        format!(
            "V=${:04X} (FY:{} NT:{} CY:{:02} CX:{:02})",
            self.v, fine_y, nt, coarse_y, coarse_x
        )
    }
}

/// APU state snapshot
#[derive(Clone)]
pub struct ApuState {
    pub pulse1: ChannelState,
    pub pulse2: ChannelState,
    pub triangle: ChannelState,
    pub noise: ChannelState,
    pub dmc: DmcState,
    pub frame_counter: u8,
    pub frame_mode: bool, // false = 4-step, true = 5-step
    pub irq_inhibit: bool,
    pub frame_irq: bool,
}

#[derive(Clone)]
pub struct ChannelState {
    pub enabled: bool,
    pub length_counter: u8,
    pub timer: u16,
    pub period: u16,
    pub volume: u8,
    pub envelope: u8,
    pub sweep_enabled: bool,
}

#[derive(Clone)]
pub struct DmcState {
    pub enabled: bool,
    pub sample_address: u16,
    pub sample_length: u16,
    pub bytes_remaining: u16,
    pub irq_enabled: bool,
    pub irq_flag: bool,
    pub loop_flag: bool,
}

/// State inspector aggregates all component states
pub struct StateInspector {
    cpu_history: Vec<CpuState>,
    ppu_history: Vec<PpuState>,
    max_history: usize,
}

impl StateInspector {
    pub fn new(max_history: usize) -> Self {
        Self {
            cpu_history: Vec::with_capacity(max_history),
            ppu_history: Vec::with_capacity(max_history),
            max_history,
        }
    }

    /// Capture current state
    pub fn capture(&mut self, cpu: &CpuState, ppu: &PpuState) {
        if self.cpu_history.len() >= self.max_history {
            self.cpu_history.remove(0);
            self.ppu_history.remove(0);
        }
        self.cpu_history.push(cpu.clone());
        self.ppu_history.push(ppu.clone());
    }

    /// Get CPU state history
    pub fn cpu_history(&self, count: usize) -> &[CpuState] {
        let start = self.cpu_history.len().saturating_sub(count);
        &self.cpu_history[start..]
    }

    /// Get PPU state history
    pub fn ppu_history(&self, count: usize) -> &[PpuState] {
        let start = self.ppu_history.len().saturating_sub(count);
        &self.ppu_history[start..]
    }
}
```

## Label/Symbol Database

```rust
use std::collections::HashMap;

/// Label database for symbolic debugging
pub struct LabelDatabase {
    /// Address to label mapping
    labels: HashMap<u16, String>,
    /// Label to address mapping (reverse lookup)
    addresses: HashMap<String, u16>,
    /// Comments for addresses
    comments: HashMap<u16, String>,
    /// Auto-generated labels for known hardware addresses
    auto_labels: HashMap<u16, &'static str>,
}

impl LabelDatabase {
    pub fn new() -> Self {
        let mut db = Self {
            labels: HashMap::new(),
            addresses: HashMap::new(),
            comments: HashMap::new(),
            auto_labels: HashMap::new(),
        };
        db.init_hardware_labels();
        db
    }

    fn init_hardware_labels(&mut self) {
        // PPU registers
        self.auto_labels.insert(0x2000, "PPUCTRL");
        self.auto_labels.insert(0x2001, "PPUMASK");
        self.auto_labels.insert(0x2002, "PPUSTATUS");
        self.auto_labels.insert(0x2003, "OAMADDR");
        self.auto_labels.insert(0x2004, "OAMDATA");
        self.auto_labels.insert(0x2005, "PPUSCROLL");
        self.auto_labels.insert(0x2006, "PPUADDR");
        self.auto_labels.insert(0x2007, "PPUDATA");

        // APU registers
        self.auto_labels.insert(0x4000, "SQ1_VOL");
        self.auto_labels.insert(0x4001, "SQ1_SWEEP");
        self.auto_labels.insert(0x4002, "SQ1_LO");
        self.auto_labels.insert(0x4003, "SQ1_HI");
        self.auto_labels.insert(0x4004, "SQ2_VOL");
        self.auto_labels.insert(0x4005, "SQ2_SWEEP");
        self.auto_labels.insert(0x4006, "SQ2_LO");
        self.auto_labels.insert(0x4007, "SQ2_HI");
        self.auto_labels.insert(0x4008, "TRI_LINEAR");
        self.auto_labels.insert(0x400A, "TRI_LO");
        self.auto_labels.insert(0x400B, "TRI_HI");
        self.auto_labels.insert(0x400C, "NOISE_VOL");
        self.auto_labels.insert(0x400E, "NOISE_LO");
        self.auto_labels.insert(0x400F, "NOISE_HI");
        self.auto_labels.insert(0x4010, "DMC_FREQ");
        self.auto_labels.insert(0x4011, "DMC_RAW");
        self.auto_labels.insert(0x4012, "DMC_START");
        self.auto_labels.insert(0x4013, "DMC_LEN");
        self.auto_labels.insert(0x4014, "OAMDMA");
        self.auto_labels.insert(0x4015, "SND_CHN");
        self.auto_labels.insert(0x4016, "JOY1");
        self.auto_labels.insert(0x4017, "JOY2_FRAME");

        // Vectors
        self.auto_labels.insert(0xFFFA, "NMI_VECTOR");
        self.auto_labels.insert(0xFFFC, "RESET_VECTOR");
        self.auto_labels.insert(0xFFFE, "IRQ_VECTOR");
    }

    /// Add user-defined label
    pub fn add_label(&mut self, address: u16, label: String) {
        if let Some(old_label) = self.labels.insert(address, label.clone()) {
            self.addresses.remove(&old_label);
        }
        self.addresses.insert(label, address);
    }

    /// Remove label
    pub fn remove_label(&mut self, address: u16) {
        if let Some(label) = self.labels.remove(&address) {
            self.addresses.remove(&label);
        }
    }

    /// Get label for address
    pub fn get_label(&self, address: u16) -> Option<String> {
        self.labels
            .get(&address)
            .cloned()
            .or_else(|| self.auto_labels.get(&address).map(|s| s.to_string()))
    }

    /// Get address for label
    pub fn get_address(&self, label: &str) -> Option<u16> {
        self.addresses.get(label).copied()
    }

    /// Add comment for address
    pub fn add_comment(&mut self, address: u16, comment: String) {
        self.comments.insert(address, comment);
    }

    /// Get comment for address
    pub fn get_comment(&self, address: u16) -> Option<&String> {
        self.comments.get(&address)
    }

    /// Import labels from file (FCEUX .nl format)
    pub fn import_nl(&mut self, content: &str) -> Result<usize, String> {
        let mut count = 0;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Format: $ADDR#LABEL#COMMENT
            let parts: Vec<&str> = line.split('#').collect();
            if parts.len() >= 2 {
                let addr_str = parts[0].trim_start_matches('$');
                if let Ok(addr) = u16::from_str_radix(addr_str, 16) {
                    let label = parts[1].trim().to_string();
                    if !label.is_empty() {
                        self.add_label(addr, label);
                        count += 1;
                    }
                    if parts.len() >= 3 {
                        let comment = parts[2].trim().to_string();
                        if !comment.is_empty() {
                            self.add_comment(addr, comment);
                        }
                    }
                }
            }
        }
        Ok(count)
    }

    /// Export labels to FCEUX .nl format
    pub fn export_nl(&self) -> String {
        let mut result = String::new();
        let mut sorted: Vec<_> = self.labels.iter().collect();
        sorted.sort_by_key(|(addr, _)| *addr);

        for (addr, label) in sorted {
            result.push_str(&format!("${:04X}#{}#", addr, label));
            if let Some(comment) = self.comments.get(addr) {
                result.push_str(comment);
            }
            result.push('\n');
        }
        result
    }
}
```

## Performance Profiler

```rust
use std::collections::HashMap;

/// Performance profiler for identifying hotspots
pub struct Profiler {
    enabled: bool,
    /// Instruction count by address
    instruction_counts: HashMap<u16, u64>,
    /// Cycle count by address
    cycle_counts: HashMap<u16, u64>,
    /// Memory read counts
    read_counts: HashMap<u16, u64>,
    /// Memory write counts
    write_counts: HashMap<u16, u64>,
    /// Function call tracking (JSR targets)
    call_counts: HashMap<u16, u64>,
    /// Time spent in functions
    function_cycles: HashMap<u16, u64>,
    /// Call stack for timing
    call_stack: Vec<(u16, u64)>, // (return_addr, start_cycle)
}

impl Profiler {
    pub fn new() -> Self {
        Self {
            enabled: false,
            instruction_counts: HashMap::new(),
            cycle_counts: HashMap::new(),
            read_counts: HashMap::new(),
            write_counts: HashMap::new(),
            call_counts: HashMap::new(),
            function_cycles: HashMap::new(),
            call_stack: Vec::new(),
        }
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    pub fn clear(&mut self) {
        self.instruction_counts.clear();
        self.cycle_counts.clear();
        self.read_counts.clear();
        self.write_counts.clear();
        self.call_counts.clear();
        self.function_cycles.clear();
        self.call_stack.clear();
    }

    /// Record instruction run
    pub fn record_instruction(&mut self, pc: u16, cycles: u8) {
        if !self.enabled {
            return;
        }
        *self.instruction_counts.entry(pc).or_insert(0) += 1;
        *self.cycle_counts.entry(pc).or_insert(0) += cycles as u64;
    }

    /// Record memory read
    pub fn record_read(&mut self, addr: u16) {
        if !self.enabled {
            return;
        }
        *self.read_counts.entry(addr).or_insert(0) += 1;
    }

    /// Record memory write
    pub fn record_write(&mut self, addr: u16) {
        if !self.enabled {
            return;
        }
        *self.write_counts.entry(addr).or_insert(0) += 1;
    }

    /// Record JSR (function call)
    pub fn record_call(&mut self, target: u16, return_addr: u16, current_cycle: u64) {
        if !self.enabled {
            return;
        }
        *self.call_counts.entry(target).or_insert(0) += 1;
        self.call_stack.push((return_addr, current_cycle));
    }

    /// Record RTS (function return)
    pub fn record_return(&mut self, current_cycle: u64) {
        if !self.enabled {
            return;
        }
        if let Some((return_addr, start_cycle)) = self.call_stack.pop() {
            let cycles_spent = current_cycle.saturating_sub(start_cycle);
            // Attribute to the function that was called (approximation)
            if let Some(last_call) = self.call_stack.last() {
                *self.function_cycles.entry(last_call.0).or_insert(0) += cycles_spent;
            }
        }
    }

    /// Get top N instructions by count
    pub fn top_instructions(&self, n: usize) -> Vec<(u16, u64)> {
        let mut sorted: Vec<_> = self.instruction_counts.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        sorted.into_iter().take(n).map(|(&a, &c)| (a, c)).collect()
    }

    /// Get top N instructions by cycles
    pub fn top_by_cycles(&self, n: usize) -> Vec<(u16, u64)> {
        let mut sorted: Vec<_> = self.cycle_counts.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        sorted.into_iter().take(n).map(|(&a, &c)| (a, c)).collect()
    }

    /// Get memory access hotspots
    pub fn memory_hotspots(&self, n: usize) -> Vec<(u16, u64, u64)> {
        let mut combined: HashMap<u16, (u64, u64)> = HashMap::new();

        for (&addr, &count) in &self.read_counts {
            combined.entry(addr).or_insert((0, 0)).0 += count;
        }
        for (&addr, &count) in &self.write_counts {
            combined.entry(addr).or_insert((0, 0)).1 += count;
        }

        let mut sorted: Vec<_> = combined.into_iter().collect();
        sorted.sort_by(|a, b| (b.1 .0 + b.1 .1).cmp(&(a.1 .0 + a.1 .1)));
        sorted
            .into_iter()
            .take(n)
            .map(|(addr, (r, w))| (addr, r, w))
            .collect()
    }

    /// Generate profile report
    pub fn generate_report(&self, labels: &LabelDatabase) -> String {
        let mut report = String::new();

        report.push_str("=== Instruction Hotspots ===\n");
        for (addr, count) in self.top_instructions(20) {
            let label = labels.get_label(addr).unwrap_or_default();
            report.push_str(&format!("${:04X} {:20} {:>12}\n", addr, label, count));
        }

        report.push_str("\n=== Cycle Hotspots ===\n");
        for (addr, cycles) in self.top_by_cycles(20) {
            let label = labels.get_label(addr).unwrap_or_default();
            report.push_str(&format!("${:04X} {:20} {:>12}\n", addr, label, cycles));
        }

        report.push_str("\n=== Memory Access Hotspots ===\n");
        for (addr, reads, writes) in self.memory_hotspots(20) {
            let label = labels.get_label(addr).unwrap_or_default();
            report.push_str(&format!(
                "${:04X} {:20} R:{:>10} W:{:>10}\n",
                addr, label, reads, writes
            ));
        }

        report.push_str("\n=== Function Call Counts ===\n");
        let mut calls: Vec<_> = self.call_counts.iter().collect();
        calls.sort_by(|a, b| b.1.cmp(a.1));
        for (addr, count) in calls.iter().take(20) {
            let label = labels.get_label(**addr).unwrap_or_default();
            report.push_str(&format!("${:04X} {:20} {:>12}\n", addr, label, count));
        }

        report
    }
}
```

## Debugger UI Integration

```rust
use egui::{Context, Ui, Window};

/// Debugger UI panel
pub struct DebuggerUi {
    show_cpu_panel: bool,
    show_ppu_panel: bool,
    show_memory_panel: bool,
    show_trace_panel: bool,
    show_breakpoints: bool,
    show_profiler: bool,
    /// Breakpoint edit state
    new_bp_address: String,
    new_bp_type: BreakpointType,
    /// Memory viewer state
    memory_address: String,
    memory_search: String,
}

impl DebuggerUi {
    pub fn new() -> Self {
        Self {
            show_cpu_panel: true,
            show_ppu_panel: false,
            show_memory_panel: false,
            show_trace_panel: false,
            show_breakpoints: false,
            show_profiler: false,
            new_bp_address: String::new(),
            new_bp_type: BreakpointType::Program(0),
            memory_address: "0000".to_string(),
            memory_search: String::new(),
        }
    }

    /// Render CPU state panel
    pub fn cpu_panel(&mut self, ctx: &Context, cpu: &CpuState, debugger: &mut Debugger) {
        if !self.show_cpu_panel {
            return;
        }

        Window::new("CPU").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("PC: ${:04X}", cpu.pc));
                ui.label(format!("A: ${:02X}", cpu.a));
                ui.label(format!("X: ${:02X}", cpu.x));
                ui.label(format!("Y: ${:02X}", cpu.y));
            });

            ui.horizontal(|ui| {
                ui.label(format!("SP: ${:02X}", cpu.s));
                ui.label(format!("P: {}", cpu.format_flags()));
                ui.label(format!("Cycle: {}", cpu.cycle));
            });

            ui.separator();

            // Control buttons
            ui.horizontal(|ui| {
                if ui.button("Step").clicked() {
                    debugger.state = DebugState::Stepping;
                }
                if ui.button("Run").clicked() {
                    debugger.state = DebugState::Running;
                }
                if ui.button("Pause").clicked() {
                    debugger.state = DebugState::Paused;
                }
                if ui.button("Step Frame").clicked() {
                    debugger.state = DebugState::RunningUntil(StopCondition::Frame(
                        // Current frame + 1
                        0, // Would get actual frame from emulator
                    ));
                }
            });
        });
    }

    /// Render PPU state panel
    pub fn ppu_panel(&mut self, ctx: &Context, ppu: &PpuState) {
        if !self.show_ppu_panel {
            return;
        }

        Window::new("PPU").show(ctx, |ui| {
            ui.label(format!("Scanline: {} Cycle: {}", ppu.scanline, ppu.cycle));
            ui.label(format!("Frame: {}", ppu.frame));
            ui.label(ppu.format_v_register());

            ui.separator();

            ui.label(format!("CTRL: ${:02X}", ppu.ctrl));
            ui.label(format!("MASK: ${:02X}", ppu.mask));
            ui.label(format!("STATUS: ${:02X}", ppu.status));

            ui.horizontal(|ui| {
                ui.label(format!("NMI: {}", ppu.nmi_occurred));
                ui.label(format!("Sprite0: {}", ppu.sprite_0_hit));
                ui.label(format!("Overflow: {}", ppu.sprite_overflow));
            });
        });
    }

    /// Render memory viewer panel
    pub fn memory_panel(
        &mut self,
        ctx: &Context,
        viewer: &mut MemoryViewer,
        bus: &Bus,
    ) {
        if !self.show_memory_panel {
            return;
        }

        Window::new("Memory").show(ctx, |ui| {
            // Region selector
            ui.horizontal(|ui| {
                ui.label("Region:");
                egui::ComboBox::from_id_salt("mem_region")
                    .selected_text(format!("{:?}", viewer.region))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut viewer.region, MemoryRegion::CpuRam, "CPU RAM");
                        ui.selectable_value(&mut viewer.region, MemoryRegion::FullCpu, "Full CPU");
                        ui.selectable_value(&mut viewer.region, MemoryRegion::ChrRom, "CHR ROM");
                        ui.selectable_value(&mut viewer.region, MemoryRegion::Nametables, "Nametables");
                        ui.selectable_value(&mut viewer.region, MemoryRegion::Palettes, "Palettes");
                    });
            });

            // Address input
            ui.horizontal(|ui| {
                ui.label("Go to:");
                ui.text_edit_singleline(&mut self.memory_address);
                if ui.button("Go").clicked() {
                    if let Ok(addr) = u16::from_str_radix(&self.memory_address, 16) {
                        viewer.offset = addr;
                    }
                }
            });

            // Hex dump
            ui.separator();
            let dump = viewer.format_hex_dump(bus, viewer.offset, 16);
            for line in dump {
                ui.monospace(line);
            }

            // Search
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Search:");
                ui.text_edit_singleline(&mut self.memory_search);
                if ui.button("Find").clicked() {
                    // Parse and search
                }
            });
        });
    }

    /// Render trace panel
    pub fn trace_panel(&mut self, ctx: &Context, tracer: &InstructionTracer) {
        if !self.show_trace_panel {
            return;
        }

        Window::new("Trace").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Enable").clicked() {
                    // Enable tracing
                }
                if ui.button("Disable").clicked() {
                    // Disable tracing
                }
                if ui.button("Clear").clicked() {
                    // Clear trace
                }
                if ui.button("Export").clicked() {
                    // Export to file
                }
            });

            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
                for entry in tracer.get_entries(100) {
                    ui.monospace(entry.format_nestest());
                }
            });
        });
    }

    /// Render breakpoint manager panel
    pub fn breakpoints_panel(
        &mut self,
        ctx: &Context,
        breakpoints: &mut BreakpointManager,
    ) {
        if !self.show_breakpoints {
            return;
        }

        Window::new("Breakpoints").show(ctx, |ui| {
            // Add new breakpoint
            ui.horizontal(|ui| {
                ui.label("Address:");
                ui.text_edit_singleline(&mut self.new_bp_address);
                if ui.button("Add").clicked() {
                    if let Ok(addr) = u16::from_str_radix(&self.new_bp_address, 16) {
                        breakpoints.add(BreakpointType::Program(addr));
                        self.new_bp_address.clear();
                    }
                }
            });

            ui.separator();

            // List breakpoints
            let bp_list: Vec<_> = breakpoints.breakpoints.values().cloned().collect();
            for bp in bp_list {
                ui.horizontal(|ui| {
                    let mut enabled = bp.enabled;
                    if ui.checkbox(&mut enabled, "").changed() {
                        breakpoints.set_enabled(bp.id, enabled);
                    }
                    ui.label(format!("{:?}", bp.bp_type));
                    ui.label(format!("Hits: {}", bp.hit_count));
                    if ui.button("X").clicked() {
                        breakpoints.remove(bp.id);
                    }
                });
            }
        });
    }
}
```

## Integration with Emulator

```rust
/// Debugger integration hook for emulator core
pub trait DebuggableEmulator {
    /// Called before each CPU instruction
    fn on_cpu_step(&mut self, debugger: &mut Debugger) -> bool;
    /// Called on memory read
    fn on_memory_read(&mut self, addr: u16, debugger: &mut Debugger);
    /// Called on memory write
    fn on_memory_write(&mut self, addr: u16, old: u8, new: u8, debugger: &mut Debugger);
    /// Called at start of each scanline
    fn on_scanline(&mut self, scanline: u16, debugger: &mut Debugger) -> bool;
    /// Get current CPU state
    fn cpu_state(&self) -> CpuState;
    /// Get current PPU state
    fn ppu_state(&self) -> PpuState;
    /// Peek memory without side effects
    fn peek(&self, addr: u16) -> u8;
    /// Poke memory for debugging
    fn poke(&mut self, addr: u16, val: u8);
}

impl Debugger {
    /// Check if we should break before running instruction
    pub fn should_break(&mut self, cpu: &CpuState, bus: &Bus) -> bool {
        match self.state {
            DebugState::Paused => true,
            DebugState::Stepping => {
                self.state = DebugState::Paused;
                true
            }
            DebugState::Running => {
                // Check program breakpoints
                if self.breakpoints.has_program_bp(cpu.pc) {
                    self.state = DebugState::Paused;
                    return true;
                }
                false
            }
            DebugState::RunningUntil(cond) => {
                match cond {
                    StopCondition::Address(addr) if cpu.pc == addr => {
                        self.state = DebugState::Paused;
                        true
                    }
                    _ => false,
                }
            }
        }
    }

    /// Check read breakpoint
    pub fn check_read(&mut self, addr: u16, cpu: &CpuState, bus: &Bus) -> bool {
        if self.breakpoints.has_read_bp(addr) {
            // Find and evaluate the breakpoint
            for bp in self.breakpoints.breakpoints.values() {
                if matches!(bp.bp_type, BreakpointType::ReadAddress(a) if a == addr) {
                    if bp.enabled && self.breakpoints.evaluate_condition(bp, cpu, bus) {
                        self.state = DebugState::Paused;
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check write breakpoint
    pub fn check_write(
        &mut self,
        addr: u16,
        old: u8,
        new: u8,
        cpu: &CpuState,
        bus: &Bus,
    ) -> bool {
        // Check watchpoints
        if let Some(wp) = self.watchpoints.check_write(addr, old, new) {
            if !wp.log_only {
                self.state = DebugState::Paused;
                return true;
            }
        }

        // Check write breakpoints
        if self.breakpoints.has_write_bp(addr) {
            self.state = DebugState::Paused;
            return true;
        }

        false
    }
}
```

## Keyboard Shortcuts

| Action | Shortcut |
|--------|----------|
| Step | F10 |
| Step Into | F11 |
| Step Out | Shift+F11 |
| Run | F5 |
| Pause | F6 / Break |
| Toggle Breakpoint | F9 |
| Run to Cursor | Ctrl+F10 |
| Go to Address | Ctrl+G |
| Find | Ctrl+F |
| Toggle CPU Panel | Ctrl+1 |
| Toggle PPU Panel | Ctrl+2 |
| Toggle Memory Panel | Ctrl+3 |
| Toggle Trace Panel | Ctrl+4 |

## Source Files

```
crates/rustynes-debugger/
├── Cargo.toml
└── src/
    ├── mod.rs              # Re-exports, Debugger struct
    ├── breakpoints.rs      # BreakpointManager, BreakpointType
    ├── watchpoints.rs      # WatchpointManager
    ├── disassembler.rs     # Disassembler, OPCODE_TABLE
    ├── trace.rs            # InstructionTracer, TraceEntry
    ├── memory_viewer.rs    # MemoryViewer, MemoryRegion
    ├── state_inspector.rs  # CpuState, PpuState, ApuState
    ├── labels.rs           # LabelDatabase
    ├── profiler.rs         # Profiler
    └── ui/
        ├── mod.rs          # DebuggerUi
        ├── cpu_panel.rs
        ├── ppu_panel.rs
        ├── memory_panel.rs
        └── trace_panel.rs
```

## Implementation Checklist

### Core Debugger
- [ ] Debugger state machine
- [ ] Event logging system
- [ ] Integration hooks

### Breakpoints
- [ ] Program counter breakpoints
- [ ] Memory read/write breakpoints
- [ ] Conditional expressions
- [ ] Hit count tracking
- [ ] Temporary breakpoints

### Watchpoints
- [ ] Value change detection
- [ ] Range watching
- [ ] Conditional triggers

### Disassembler
- [ ] Full opcode table
- [ ] Label substitution
- [ ] Comment integration

### Tracing
- [ ] nestest.log format
- [ ] FCEUX format
- [ ] Streaming to file
- [ ] Filter by PC/scanline

### Memory Viewer
- [ ] Hex dump display
- [ ] Region switching
- [ ] Search functionality
- [ ] Change highlighting

### Profiler
- [ ] Instruction counting
- [ ] Cycle attribution
- [ ] Memory access tracking
- [ ] Function timing

### UI
- [ ] CPU state panel
- [ ] PPU state panel
- [ ] Memory viewer panel
- [ ] Trace log panel
- [ ] Breakpoint manager
- [ ] Keyboard shortcuts

## References

- FCEUX debugger documentation
- Mesen debugging features
- NESdev Wiki debugging techniques
- Visual 6502 for opcode timing verification

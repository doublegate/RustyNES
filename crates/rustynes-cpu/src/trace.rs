//! CPU trace logging for nestest.log-compatible output.
//!
//! This module provides functionality to generate execution traces matching
//! the nestest golden log format, essential for CPU validation.

use crate::addressing::AddressingMode;
use crate::bus::Bus;
use crate::cpu::Cpu;
use crate::opcodes::OPCODE_TABLE;
use std::fmt::Write;

/// Trace entry representing a single instruction execution.
#[derive(Debug, Clone)]
pub struct TraceEntry {
    /// Program counter
    pub pc: u16,
    /// Opcode byte
    pub opcode: u8,
    /// Operand bytes (0-2 bytes)
    pub operand_bytes: Vec<u8>,
    /// Disassembled instruction string
    pub disassembly: String,
    /// Accumulator register
    pub a: u8,
    /// X register
    pub x: u8,
    /// Y register
    pub y: u8,
    /// Status register
    pub p: u8,
    /// Stack pointer
    pub sp: u8,
    /// Total CPU cycles
    pub cycles: u64,
}

impl TraceEntry {
    /// Format the trace entry in nestest.log format.
    ///
    /// Format: PC  OPCODE_BYTES  DISASM    A:XX X:XX Y:XX P:XX SP:XX CYC:XXXXX
    pub fn format(&self) -> String {
        // Format opcode bytes
        let mut bytes_str = String::new();
        let opcode = self.opcode;
        write!(bytes_str, "{opcode:02X}").unwrap();
        for byte in &self.operand_bytes {
            write!(bytes_str, " {byte:02X}").unwrap();
        }

        // Unofficial opcodes have the * prefix "steal" one space from bytes field
        // Official: bytes=10 chars, disasm=32 chars
        // Unofficial: bytes=9 chars, disasm=33 chars (starts with *)
        let bytes_width = if self.disassembly.starts_with('*') {
            9
        } else {
            10
        };
        let bytes_field = format!("{bytes_str:<bytes_width$}");

        // Disassembly field is always formatted to fit (32-33 chars)
        let disasm_width = if self.disassembly.starts_with('*') {
            33
        } else {
            32
        };
        let disasm_field = format!("{:<width$}", self.disassembly, width = disasm_width);

        format!(
            "{:04X}  {}{}A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X} CYC:{}",
            self.pc,
            bytes_field,
            disasm_field,
            self.a,
            self.x,
            self.y,
            self.p,
            self.sp,
            self.cycles
        )
    }
}

/// CPU trace logger for generating nestest-compatible logs.
pub struct CpuTracer {
    entries: Vec<String>,
}

impl CpuTracer {
    /// Create a new CPU tracer.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Log the current CPU state before executing the instruction.
    ///
    /// IMPORTANT: This must be called BEFORE the instruction executes,
    /// as the log shows the state at the start of the instruction.
    pub fn trace(&mut self, cpu: &Cpu, bus: &mut impl Bus) {
        let entry = self.create_trace_entry(cpu, bus);
        self.entries.push(entry.format());
    }

    /// Get all logged entries as a single string.
    pub fn get_log(&self) -> String {
        self.entries.join("\n")
    }

    /// Get the number of logged entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the log is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Create a trace entry for the current CPU state.
    fn create_trace_entry(&self, cpu: &Cpu, bus: &mut impl Bus) -> TraceEntry {
        let pc = cpu.pc;
        let opcode = bus.read(pc);
        let opcode_info = &OPCODE_TABLE[opcode as usize];

        // Fetch operand bytes
        let operand_bytes = self.fetch_operand_bytes(pc, opcode_info.addr_mode, bus);

        // Generate disassembly
        let disassembly = self.disassemble(cpu, bus, pc, opcode, opcode_info);

        TraceEntry {
            pc,
            opcode,
            operand_bytes,
            disassembly,
            a: cpu.a,
            x: cpu.x,
            y: cpu.y,
            p: cpu.status.bits(),
            sp: cpu.sp,
            cycles: cpu.cycles,
        }
    }

    /// Fetch operand bytes for the instruction.
    fn fetch_operand_bytes(
        &self,
        pc: u16,
        addr_mode: AddressingMode,
        bus: &mut impl Bus,
    ) -> Vec<u8> {
        let num_bytes = addr_mode.operand_bytes();
        (1..=num_bytes)
            .map(|i| bus.read(pc.wrapping_add(i as u16)))
            .collect()
    }

    /// Disassemble the instruction at PC.
    #[allow(clippy::too_many_lines)]
    fn disassemble(
        &self,
        cpu: &Cpu,
        bus: &mut impl Bus,
        pc: u16,
        _opcode: u8,
        opcode_info: &crate::opcodes::OpcodeInfo,
    ) -> String {
        let mnemonic = opcode_info.mnemonic;
        let addr_mode = opcode_info.addr_mode;
        let prefix = if opcode_info.unofficial { "*" } else { "" };

        match addr_mode {
            AddressingMode::Implied => format!("{prefix}{mnemonic}"),

            AddressingMode::Accumulator => format!("{prefix}{mnemonic} A"),

            AddressingMode::Immediate => {
                let value = bus.read(pc.wrapping_add(1));
                format!("{prefix}{mnemonic} #${value:02X}")
            }

            AddressingMode::ZeroPage => {
                let addr = bus.read(pc.wrapping_add(1));
                let value = bus.read(addr as u16);
                format!("{prefix}{mnemonic} ${addr:02X} = {value:02X}")
            }

            AddressingMode::ZeroPageX => {
                let base = bus.read(pc.wrapping_add(1));
                let addr = base.wrapping_add(cpu.x);
                let value = bus.read(addr as u16);
                format!("{prefix}{mnemonic} ${base:02X},X @ {addr:02X} = {value:02X}")
            }

            AddressingMode::ZeroPageY => {
                let base = bus.read(pc.wrapping_add(1));
                let addr = base.wrapping_add(cpu.y);
                let value = bus.read(addr as u16);
                format!("{prefix}{mnemonic} ${base:02X},Y @ {addr:02X} = {value:02X}")
            }

            AddressingMode::Absolute => {
                let lo = bus.read(pc.wrapping_add(1));
                let hi = bus.read(pc.wrapping_add(2));
                let addr = u16::from_le_bytes([lo, hi]);

                // Special handling for JMP and JSR (no value read)
                if mnemonic == "JMP" || mnemonic == "JSR" {
                    format!("{prefix}{mnemonic} ${addr:04X}")
                } else {
                    let value = bus.read(addr);
                    format!("{prefix}{mnemonic} ${addr:04X} = {value:02X}")
                }
            }

            AddressingMode::AbsoluteX => {
                let lo = bus.read(pc.wrapping_add(1));
                let hi = bus.read(pc.wrapping_add(2));
                let base = u16::from_le_bytes([lo, hi]);
                let addr = base.wrapping_add(cpu.x as u16);
                let value = bus.read(addr);
                format!("{prefix}{mnemonic} ${base:04X},X @ {addr:04X} = {value:02X}")
            }

            AddressingMode::AbsoluteY => {
                let lo = bus.read(pc.wrapping_add(1));
                let hi = bus.read(pc.wrapping_add(2));
                let base = u16::from_le_bytes([lo, hi]);
                let addr = base.wrapping_add(cpu.y as u16);
                let value = bus.read(addr);
                format!("{prefix}{mnemonic} ${base:04X},Y @ {addr:04X} = {value:02X}")
            }

            AddressingMode::Indirect => {
                let lo = bus.read(pc.wrapping_add(1));
                let hi = bus.read(pc.wrapping_add(2));
                let ptr = u16::from_le_bytes([lo, hi]);

                // Read the target address (with page-wrap bug)
                let target_lo = bus.read(ptr) as u16;
                let target_hi = if (ptr & 0x00FF) == 0x00FF {
                    // Page boundary bug: read from same page
                    bus.read(ptr & 0xFF00) as u16
                } else {
                    bus.read(ptr.wrapping_add(1)) as u16
                };
                let target = (target_hi << 8) | target_lo;

                format!("{prefix}{mnemonic} (${ptr:04X}) = {target:04X}")
            }

            AddressingMode::IndexedIndirectX => {
                let base = bus.read(pc.wrapping_add(1));
                let ptr = base.wrapping_add(cpu.x);

                let lo = bus.read(ptr as u16) as u16;
                let hi = bus.read(ptr.wrapping_add(1) as u16) as u16;
                let addr = (hi << 8) | lo;
                let value = bus.read(addr);

                format!("{prefix}{mnemonic} (${base:02X},X) @ {ptr:02X} = {addr:04X} = {value:02X}")
            }

            AddressingMode::IndirectIndexedY => {
                let ptr = bus.read(pc.wrapping_add(1));

                let lo = bus.read(ptr as u16) as u16;
                let hi = bus.read(ptr.wrapping_add(1) as u16) as u16;
                let base = (hi << 8) | lo;

                let addr = base.wrapping_add(cpu.y as u16);
                let value = bus.read(addr);

                format!("{prefix}{mnemonic} (${ptr:02X}),Y = {base:04X} @ {addr:04X} = {value:02X}")
            }

            AddressingMode::Relative => {
                let offset = bus.read(pc.wrapping_add(1)) as i8;
                let target = pc.wrapping_add(2).wrapping_add(offset as u16);
                format!("{prefix}{mnemonic} ${target:04X}")
            }
        }
    }
}

impl Default for CpuTracer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::StatusFlags;

    struct TestBus {
        memory: Vec<u8>,
    }

    impl TestBus {
        fn new() -> Self {
            Self {
                memory: vec![0; 0x10000],
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
    fn test_trace_lda_immediate() {
        let mut cpu = Cpu::new();
        let mut bus = TestBus::new();
        let mut tracer = CpuTracer::new();

        cpu.pc = 0xC000;
        cpu.cycles = 7;
        cpu.a = 0x00;
        cpu.x = 0x00;
        cpu.y = 0x00;
        cpu.sp = 0xFD;
        cpu.status = StatusFlags::from_bits_truncate(0x24);

        // LDA #$42
        bus.memory[0xC000] = 0xA9;
        bus.memory[0xC001] = 0x42;

        tracer.trace(&cpu, &mut bus);
        let log = tracer.get_log();

        assert!(log.contains("C000"));
        assert!(log.contains("A9 42"));
        assert!(log.contains("LDA #$42"));
        assert!(log.contains("A:00 X:00 Y:00 P:24 SP:FD"));
        assert!(log.contains("CYC:7"));
    }

    #[test]
    fn test_trace_jmp_absolute() {
        let mut cpu = Cpu::new();
        let mut bus = TestBus::new();
        let mut tracer = CpuTracer::new();

        cpu.pc = 0xC000;
        cpu.cycles = 7;
        cpu.status = StatusFlags::from_bits_truncate(0x24);
        cpu.sp = 0xFD;

        // JMP $C5F5
        bus.memory[0xC000] = 0x4C;
        bus.memory[0xC001] = 0xF5;
        bus.memory[0xC002] = 0xC5;

        tracer.trace(&cpu, &mut bus);
        let log = tracer.get_log();

        assert!(log.contains("C000"));
        assert!(log.contains("4C F5 C5"));
        assert!(log.contains("JMP $C5F5"));
    }
}

# nestest Golden Log Reference

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** nestest.nes automation methodology and log comparison

---

## Table of Contents

- [Overview](#overview)
- [Log Format Specification](#log-format-specification)
- [Automation Mode](#automation-mode)
- [Log Generation](#log-generation)
- [Log Comparison](#log-comparison)
- [Common Divergence Points](#common-divergence-points)
- [Implementation Guide](#implementation-guide)

---

## Overview

The **nestest golden log** (`nestest.log`) is a cycle-accurate trace of CPU execution for the nestest.nes test ROM. It serves as the **definitive reference** for CPU emulation accuracy.

### Golden Log Statistics

```
Total Lines: 8,991
Starting PC: $C000
Ending PC:   $C66E
Total Cycles: ~26,560
Test Duration: ~15 milliseconds (real NES)
```

### Why nestest is Essential

- **Comprehensive:** Tests all 151 official opcodes
- **Cycle-Accurate:** Validates exact timing
- **Flag Coverage:** Tests all flag combinations
- **Addressing Modes:** Exercises all 13 addressing modes
- **Self-Contained:** Automation mode needs only CPU

---

## Log Format Specification

### Format String

```
{PC:04X}  {OPCODE_BYTES}  {DISASM:<32} A:{A:02X} X:{X:02X} Y:{Y:02X} P:{P:02X} SP:{SP:02X} CYC:{CYC}
```

### Example Log Lines

```
C000  4C F5 C5  JMP $C5F5                       A:00 X:00 Y:00 P:24 SP:FD CYC:7
C5F5  A2 00     LDX #$00                        A:00 X:00 Y:00 P:24 SP:FD CYC:10
C5F7  86 00     STX $00 = 00                    A:00 X:00 Y:00 P:26 SP:FD CYC:13
C5F9  A2 01     LDX #$01                        A:00 X:00 Y:00 P:26 SP:FD CYC:16
C5FB  86 01     STX $01 = 00                    A:00 X:01 Y:00 P:24 SP:FD CYC:19
C5FD  A9 35     LDA #$35                        A:00 X:01 Y:00 P:24 SP:FD CYC:22
C5FF  38        SEC                             A:35 X:01 Y:00 P:24 SP:FD CYC:24
C600  7A        NOP (unofficial)                A:35 X:01 Y:00 P:25 SP:FD CYC:26
C601  69 01     ADC #$01                        A:35 X:01 Y:00 P:25 SP:FD CYC:28
C603  08        PHP                             A:37 X:01 Y:00 P:24 SP:FD CYC:30
```

### Field Descriptions

#### PC (Program Counter)

```
Format: 4 hexadecimal digits, uppercase
Example: C000, C5F5, FFFC
```

#### OPCODE_BYTES

```
Format: Space-separated hex bytes (1-3 bytes), uppercase
Padding: Left-aligned in 9-character field

Examples:
"4C F5 C5 " - 3-byte instruction (JMP $C5F5)
"A2 00    " - 2-byte instruction (LDX #$00)
"EA       " - 1-byte instruction (NOP)
```

#### DISASM (Disassembly)

```
Format: Left-aligned in 32-character field
Components: MNEMONIC [OPERAND] [= VALUE]

Examples:
"JMP $C5F5                       "
"LDX #$00                        "
"STX $00 = 00                    "
"LDA $0200,Y @ 0220 = FF         " (indexed addressing shows effective address)
```

**Operand Formats:**
```
Immediate:     LDA #$42
Zero Page:     LDA $80
Zero Page,X:   LDA $80,X = FF
Absolute:      LDA $1234
Absolute,X:    LDA $1234,X @ 1244 = FF
(Indirect,X):  LDA ($80,X) @ 85 = 1234 = FF
(Indirect),Y:  LDA ($80),Y = 1234 @ 1244 = FF
```

**"= VALUE" Suffix:**
Shows value read from or written to memory:
```
STA $80 = 42    - Write 0x42 to $0080
LDA $80 = 42    - Read 0x42 from $0080
```

#### A, X, Y (Registers)

```
Format: 2 hexadecimal digits, uppercase
Values: 00-FF

Example: A:35 X:01 Y:00
```

#### P (Processor Status)

```
Format: 2 hexadecimal digits, uppercase
Bits: NV-BDIZC

Examples:
24 = 0b00100100 = --1--1-- (I=1, B=0, U=1)
25 = 0b00100101 = --1--1-1 (I=1, B=0, U=1, C=1)
```

**Flag Bits:**
```
Bit 7: N (Negative)
Bit 6: V (Overflow)
Bit 5: U (Unused, always 1)
Bit 4: B (Break, 1 in log but see notes)
Bit 3: D (Decimal, no effect on NES)
Bit 2: I (Interrupt Disable)
Bit 1: Z (Zero)
Bit 0: C (Carry)
```

**Important:** The B flag is not a physical flag. In the log, it reflects what would be pushed to the stack.

#### SP (Stack Pointer)

```
Format: 2 hexadecimal digits, uppercase
Values: 00-FF (points to $0100-$01FF)

Example: SP:FD -> Stack at $01FD
```

#### CYC (Cycle Count)

```
Format: Decimal number (variable width)
Range: 7 to ~26,560

Example: CYC:7, CYC:340, CYC:5432
```

**Cycle Count Notes:**
- Starts at 7 (after RESET sequence completes)
- Increments by instruction cycle count
- Includes page crossing penalties
- Matches PPU cycle × 3 relationship

---

## Automation Mode

### Starting Conditions

nestest automation mode bypasses the RESET vector and jumps directly to $C000:

```
Initial State (after RESET, before $C000):
PC:   $C000
A:    $00
X:    $00
Y:    $00
P:    $24 (I=1, U=1)
SP:   $FD
Cycles: 7
```

### Why Start at Cycle 7?

The RESET sequence takes 7 cycles:
```
Cycle 1-2: Internal operations
Cycle 3:   Decrement SP (no write)
Cycle 4:   Decrement SP (no write)
Cycle 5:   Decrement SP (no write), set I=1
Cycle 6:   Fetch RESET vector low from $FFFC
Cycle 7:   Fetch RESET vector high from $FFFD

After cycle 7: PC = $C000, begin nestest
```

### Completion Detection

The test completes when:
```
PC reaches $C66E (final instruction)
Status code written to $6000:
  $00 = All tests passed
  $01-$FF = Error code (test number that failed)
```

---

## Log Generation

### Implementation Example

```rust
pub struct CpuLogger {
    output: Vec<String>,
}

impl CpuLogger {
    pub fn log_instruction(&mut self, cpu: &Cpu, bus: &Bus) {
        let pc = cpu.pc;
        let opcode = bus.read(pc);
        let bytes = self.fetch_instruction_bytes(cpu, bus, opcode);
        let disasm = self.disassemble(cpu, bus, opcode);

        let line = format!(
            "{:04X}  {:<9}{:<32}A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X} CYC:{}",
            pc,
            self.format_bytes(&bytes),
            disasm,
            cpu.a,
            cpu.x,
            cpu.y,
            cpu.p.bits(),
            cpu.sp,
            cpu.cycles
        );

        self.output.push(line);
    }

    fn format_bytes(&self, bytes: &[u8]) -> String {
        let hex: Vec<String> = bytes.iter()
            .map(|b| format!("{:02X}", b))
            .collect();
        format!("{:<9}", hex.join(" "))
    }

    fn disassemble(&self, cpu: &Cpu, bus: &Bus, opcode: u8) -> String {
        let pc = cpu.pc;
        let mnemonic = OPCODE_NAMES[opcode as usize];
        let mode = ADDR_MODES[opcode as usize];

        match mode {
            AddressingMode::Implied => {
                format!("{:<32}", mnemonic)
            }
            AddressingMode::Immediate => {
                let value = bus.read(pc + 1);
                format!("{} #${:02X}{:<21}", mnemonic, value, "")
            }
            AddressingMode::ZeroPage => {
                let addr = bus.read(pc + 1);
                let value = bus.read(addr as u16);
                format!("{} ${:02X} = {:02X}{:<17}", mnemonic, addr, value, "")
            }
            AddressingMode::ZeroPageX => {
                let base = bus.read(pc + 1);
                let addr = base.wrapping_add(cpu.x);
                let value = bus.read(addr as u16);
                format!("{} ${:02X},X @ {:02X} = {:02X}{:<11}", mnemonic, base, addr, value, "")
            }
            AddressingMode::Absolute => {
                let lo = bus.read(pc + 1);
                let hi = bus.read(pc + 2);
                let addr = u16::from_le_bytes([lo, hi]);
                let value = bus.read(addr);
                format!("{} ${:04X} = {:02X}{:<15}", mnemonic, addr, value, "")
            }
            AddressingMode::AbsoluteX => {
                let lo = bus.read(pc + 1);
                let hi = bus.read(pc + 2);
                let base = u16::from_le_bytes([lo, hi]);
                let addr = base.wrapping_add(cpu.x as u16);
                let value = bus.read(addr);
                format!("{} ${:04X},X @ {:04X} = {:02X}{:<7}", mnemonic, base, addr, value, "")
            }
            // ... other addressing modes
        }
    }
}
```

### Logging Timing

**Critical:** Log BEFORE executing the instruction:

```rust
pub fn step_with_logging(&mut self, bus: &mut Bus, logger: &mut CpuLogger) -> u8 {
    // Log current state BEFORE execution
    logger.log_instruction(self, bus);

    // Execute instruction
    let cycles = self.step(bus);

    cycles
}
```

---

## Log Comparison

### Exact Match Requirements

Every character must match exactly:
- **Uppercase hex:** Use `{:02X}` not `{:02x}`
- **Padding:** Exact spacing in all fields
- **Disassembly:** Format must match precisely
- **Cycle count:** Must include page crossing penalties

### Automated Comparison

```rust
pub fn compare_logs(emulator_log: &str, golden_log: &str) -> Result<(), LogError> {
    let emu_lines: Vec<&str> = emulator_log.lines().collect();
    let gold_lines: Vec<&str> = golden_log.lines().collect();

    for (line_num, (emu, gold)) in emu_lines.iter().zip(gold_lines.iter()).enumerate() {
        if emu != gold {
            return Err(LogError::Mismatch {
                line: line_num + 1,
                expected: gold.to_string(),
                actual: emu.to_string(),
                diff: find_difference(emu, gold),
            });
        }
    }

    if emu_lines.len() != gold_lines.len() {
        return Err(LogError::LengthMismatch {
            expected: gold_lines.len(),
            actual: emu_lines.len(),
        });
    }

    Ok(())
}

fn find_difference(emu: &str, gold: &str) -> String {
    for (i, (e, g)) in emu.chars().zip(gold.chars()).enumerate() {
        if e != g {
            return format!("Position {}: expected '{}' got '{}'", i, g, e);
        }
    }
    "Length mismatch".to_string()
}
```

---

## Common Divergence Points

### Early Divergence (Lines 1-100)

**Cause:** Basic instruction errors

```
Common Issues:
- LDA not setting flags correctly
- STA affecting flags (it shouldn't!)
- ADC carry/overflow wrong
- Branch not taken when it should be
```

### Mid-Test Divergence (Lines 100-5000)

**Cause:** Addressing mode errors

```
Common Issues:
- Zero page,X wrapping: $FF,X with X=$01 should read $00 not $100
- Indexed indirect wrong: ($80,X) calculation error
- Page crossing penalty missing
- Dummy reads not performed
```

### Late Divergence (Lines 5000+)

**Cause:** Subtle timing or flag issues

```
Common Issues:
- Overflow flag calculation wrong
- Unofficial opcodes not implemented
- Stack operations (PHP/PLP) incorrect
- Interrupt flag behavior wrong
```

### Cycle Count Divergence

If everything matches except CYC:

```
Common Causes:
- Missing page crossing penalty (+1 cycle)
- Wrong base cycle count for instruction
- Branch taken timing wrong
- Starting cycle count not 7
```

---

## Implementation Guide

### Minimal nestest Runner

```rust
pub fn run_nestest_automation() -> Result<(), TestError> {
    let mut nes = Nes::new();
    let mut logger = CpuLogger::new();

    // Load nestest.nes
    nes.load_rom("nestest.nes")?;

    // Start at $C000 (automation mode)
    nes.cpu.pc = 0xC000;
    nes.cpu.cycles = 7;

    // Run until completion
    loop {
        // Log before execution
        logger.log_instruction(&nes.cpu, &nes.bus);

        // Execute instruction
        nes.step();

        // Check for completion (PC = $C66E)
        if nes.cpu.pc == 0xC66E {
            break;
        }

        // Timeout safety
        if nes.cpu.cycles > 100_000 {
            return Err(TestError::Timeout);
        }
    }

    // Check result
    let status = nes.bus.read(0x6000);
    if status == 0x00 {
        println!("nestest passed!");

        // Compare log
        let golden = include_str!("nestest.log");
        compare_logs(&logger.output.join("\n"), golden)?;

        Ok(())
    } else {
        Err(TestError::Failed(status))
    }
}
```

### Debug Output for Divergence

```rust
pub fn print_divergence(line_num: usize, expected: &str, actual: &str) {
    println!("LOG DIVERGENCE at line {}", line_num);
    println!("Expected: {}", expected);
    println!("Actual:   {}", actual);
    println!();

    // Character-by-character comparison
    for (i, (e, a)) in expected.chars().zip(actual.chars()).enumerate() {
        if e != a {
            println!("First diff at position {}: expected '{}' (0x{:02X}), got '{}' (0x{:02X})",
                     i, e, e as u8, a, a as u8);
            break;
        }
    }
}
```

---

## Related Documentation

- [TEST_ROM_GUIDE.md](TEST_ROM_GUIDE.md) - Complete test ROM inventory
- [BLARGG_TEST_MATRIX.md](BLARGG_TEST_MATRIX.md) - Blargg test suite
- [CPU_6502_SPECIFICATION.md](../cpu/CPU_6502_SPECIFICATION.md) - CPU instruction reference
- [CPU_TIMING_REFERENCE.md](../cpu/CPU_TIMING_REFERENCE.md) - Cycle timing details

---

## References

- [NESdev Wiki: Emulator Tests](https://www.nesdev.org/wiki/Emulator_tests)
- [nestest.nes and nestest.log](https://github.com/christopherpow/nes-test-roms/tree/master/other)
- [Nintendulator](http://www.qmtpro.com/~nes/nintendulator/) - Reference emulator
- nestest.txt - Test documentation

---

**Document Status:** Complete nestest golden log format and automation methodology.

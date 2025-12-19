# Debugging RustyNES

**Table of Contents**

- [Overview](#overview)
- [Built-in Debugger](#built-in-debugger)
- [Logging](#logging)
- [Common Issues](#common-issues)
- [Debugging Tools](#debugging-tools)
- [Test ROM Debugging](#test-rom-debugging)
- [Performance Profiling](#performance-profiling)

---

## Overview

RustyNES provides comprehensive debugging tools for both developers and users troubleshooting emulation issues.

---

## Built-in Debugger

### Activating the Debugger

**Compile with debugger feature**:

```bash
cargo build --features debugger
```

**Launch with debugger**:

```bash
rustynes --debug rom.nes
```

### Debugger Features

**CPU Debugger**:

- Disassembly view
- Register inspection
- Breakpoints (PC, memory read/write)
- Step execution (instruction, scanline, frame)
- Stack viewer

**PPU Debugger**:

- Nametable viewer
- Pattern table viewer
- Sprite viewer (OAM)
- Palette viewer
- VRAM inspector

**Memory Viewer**:

- CPU address space ($0000-$FFFF)
- PPU address space ($0000-$3FFF)
- Cartridge RAM/ROM

### Debugger Commands

**Execution Control**:

```
s / step    - Step one instruction
n / next    - Step over (JSR)
c / continue - Resume execution
p / pause   - Pause execution
```

**Breakpoints**:

```
bp <addr>        - Set breakpoint at address
bp del <addr>    - Delete breakpoint
bp list          - List all breakpoints
watch <addr>     - Break on memory write
```

**Inspection**:

```
r / regs         - Display CPU registers
m <addr> [len]   - Display memory
d <addr> [len]   - Disassemble from address
ppu              - Display PPU state
```

---

## Logging

### Enable Logging

**Environment variable**:

```bash
RUST_LOG=debug cargo run -- rom.nes
RUST_LOG=rustynes::cpu=trace cargo run -- rom.nes
```

### Log Levels

```
error - Critical errors only
warn  - Warnings and errors
info  - General information
debug - Detailed debugging
trace - Extremely verbose (per-instruction)
```

### Example Output

```
[INFO] Loading ROM: super_mario_bros.nes
[DEBUG] Mapper: 0 (NROM)
[DEBUG] PRG-ROM: 32KB, CHR-ROM: 8KB
[TRACE] CPU: A:00 X:00 Y:00 P:24 SP:FD PC:C000
[TRACE] CPU: Executing: LDA #$10 (2 cycles)
```

---

## Common Issues

### Graphics Glitches

**Symptoms**: Incorrect sprites, background corruption

**Debugging**:

1. Enable PPU debugger
2. Check pattern tables (correct tiles loaded?)
3. Check nametables (correct tile IDs?)
4. Verify palette RAM
5. Check PPU register writes

**Common Causes**:

- PPU timing errors
- Incorrect scrolling implementation
- CHR banking bugs (mappers)

### Audio Issues

**Symptoms**: Missing sound, distorted audio

**Debugging**:

1. Check APU register writes
2. Verify channel enable flags ($4015)
3. Check frame counter mode
4. Inspect channel waveforms

**Common Causes**:

- APU timing errors
- Incorrect mixer output
- Sample rate mismatch

### Input Not Working

**Symptoms**: Controller unresponsive

**Debugging**:

1. Log $4016/$4017 reads/writes
2. Verify strobe sequence
3. Check button state propagation

**Common Causes**:

- Missing strobe write
- Incorrect read sequence
- DPCM conflict (rare)

---

## Debugging Tools

### Trace Logging

**CPU trace** (compare with golden log):

```rust
fn log_cpu_state(&self) {
    println!(
        "{:04X}  A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X}",
        self.pc, self.a, self.x, self.y, self.p.bits(), self.sp
    );
}
```

**Compare with nestest.log**:

```bash
cargo run -- nestest.nes --log-cpu > output.log
diff output.log nestest.log.golden
```

### Memory Dump

```rust
fn dump_ram(&self) {
    for addr in 0x0000..=0x07FF {
        if addr % 16 == 0 {
            print!("\n{:04X}: ", addr);
        }
        print!("{:02X} ", self.read(addr));
    }
    println!();
}
```

### PPU State Dump

```rust
fn dump_ppu_state(&self) {
    println!("PPU Registers:");
    println!("  PPUCTRL:   {:08b}", self.ctrl);
    println!("  PPUMASK:   {:08b}", self.mask);
    println!("  PPUSTATUS: {:08b}", self.status);
    println!("  Scanline: {}, Cycle: {}", self.scanline, self.cycle);
    println!("  VRAM Addr: ${:04X}", self.vram_addr);
}
```

---

## Test ROM Debugging

### Using nestest

**Run with logging**:

```bash
cargo run --release -- tests/roms/nestest.nes --automation
```

**Expected output**:

```
[PASS] nestest automated test
All 8000+ instructions validated
```

**On failure**:

```
[FAIL] nestest: Mismatch at line 4523
Expected: A:42 X:00 Y:00 P:24 SP:FD
Got:      A:43 X:00 Y:00 P:24 SP:FD
```

### blargg Tests

**Run specific test**:

```bash
cargo test --test blargg_cpu_exec_space
```

**Interpret results**:

- Test writes status to $6000
- $00 = Pass
- $01-$FF = Failure code
- Text message in $6004+

---

## Performance Profiling

### CPU Profiling (Linux)

**Using perf**:

```bash
cargo build --release
perf record -g ./target/release/rustynes rom.nes
perf report
```

**Using flamegraph**:

```bash
cargo install flamegraph
cargo flamegraph --bin rustynes -- rom.nes
```

### Memory Profiling

**Using valgrind (massif)**:

```bash
valgrind --tool=massif ./target/release/rustynes rom.nes
ms_print massif.out.<pid>
```

---

## References

- [NesDev Wiki: Debugging](https://www.nesdev.org/wiki/Debugging)
- [nestest ROM](https://www.nesdev.org/nestest.txt)
- [blargg Test Suite](http://blargg.8bitalley.com/nes-tests/)

---

**Related Documents**:

- [TESTING.md](TESTING.md) - Test suite
- [BUILD.md](BUILD.md) - Build instructions
- [CONTRIBUTING.md](CONTRIBUTING.md) - Development workflow

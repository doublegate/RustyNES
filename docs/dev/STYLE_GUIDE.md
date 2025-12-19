# RustyNES Code Style Guide

Coding conventions and best practices for contributing to RustyNES.

## General Principles

1. **Correctness over performance** - Accuracy first, optimize later
2. **Clarity over cleverness** - Readable code is maintainable code
3. **Consistency** - Follow existing patterns in the codebase
4. **Documentation** - Public APIs must be documented

## Rust Conventions

### Edition and MSRV

```toml
[package]
edition = "2021"
rust-version = "1.75"  # Minimum Supported Rust Version
```

### Formatting

Use `rustfmt` with default settings:

```bash
cargo fmt --all
```

Configuration in `rustfmt.toml`:

```toml
edition = "2021"
max_width = 100
use_small_heuristics = "Default"
```

### Linting

Enable strict lints in `Cargo.toml`:

```toml
[lints.rust]
unsafe_code = "warn"

[lints.clippy]
pedantic = { level = "warn", priority = -1 }
nursery = { level = "warn", priority = -1 }
# Allow specific patterns used in emulation
cast_possible_truncation = "allow"
cast_sign_loss = "allow"
cast_lossless = "allow"
```

Run clippy:

```bash
cargo clippy --workspace -- -D warnings
```

## Naming Conventions

### Types

```rust
// Structs: PascalCase, noun
pub struct StatusRegister(u8);
pub struct MemoryMapper;
pub struct SpriteEvaluator;

// Enums: PascalCase, noun or adjective
pub enum AddressingMode {
    Immediate,
    ZeroPage,
    Absolute,
}

pub enum Mirroring {
    Horizontal,
    Vertical,
    SingleScreenLower,
    SingleScreenUpper,
    FourScreen,
}

// Traits: PascalCase, adjective or verb
pub trait Mapper { }
pub trait Clocked { }
pub trait Readable { }
```

### Functions and Methods

```rust
// Functions: snake_case, verb phrases
fn calculate_address(base: u16, offset: u8) -> u16 { }
fn load_rom_data(path: &Path) -> Result<Vec<u8>> { }

// Getters: no "get_" prefix
impl Cpu {
    pub fn program_counter(&self) -> u16 { self.pc }
    pub fn accumulator(&self) -> u8 { self.a }
}

// Setters: "set_" prefix
impl Cpu {
    pub fn set_program_counter(&mut self, value: u16) {
        self.pc = value;
    }
}

// Boolean queries: "is_", "has_", "can_"
fn is_page_crossed(addr1: u16, addr2: u16) -> bool { }
fn has_pending_interrupt(&self) -> bool { }
fn can_write(&self, addr: u16) -> bool { }
```

### Constants

```rust
// Constants: SCREAMING_SNAKE_CASE
const MASTER_CLOCK_NTSC: u32 = 21_477_272;
const CPU_CLOCK_DIVIDER: u32 = 12;
const PPU_CLOCK_DIVIDER: u32 = 4;

// Named bit flags
const FLAG_CARRY: u8 = 0x01;
const FLAG_ZERO: u8 = 0x02;
const FLAG_INTERRUPT_DISABLE: u8 = 0x04;
const FLAG_DECIMAL: u8 = 0x08;
const FLAG_BREAK: u8 = 0x10;
const FLAG_UNUSED: u8 = 0x20;
const FLAG_OVERFLOW: u8 = 0x40;
const FLAG_NEGATIVE: u8 = 0x80;
```

### Modules

```rust
// Modules: snake_case, descriptive
mod cpu;
mod ppu;
mod apu;
mod memory_map;
mod sprite_evaluation;
```

## Type Patterns

### Newtype Pattern for Addresses

```rust
/// 16-bit CPU address
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuAddress(pub u16);

impl CpuAddress {
    pub const fn new(addr: u16) -> Self {
        Self(addr)
    }

    pub fn wrapping_add(self, offset: u16) -> Self {
        Self(self.0.wrapping_add(offset))
    }
}

/// 14-bit PPU VRAM address
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VramAddress(u16);

impl VramAddress {
    pub fn new(addr: u16) -> Self {
        Self(addr & 0x3FFF) // Mask to 14 bits
    }

    pub fn coarse_x(&self) -> u8 {
        (self.0 & 0x001F) as u8
    }

    pub fn coarse_y(&self) -> u8 {
        ((self.0 >> 5) & 0x001F) as u8
    }

    pub fn nametable_select(&self) -> u8 {
        ((self.0 >> 10) & 0x03) as u8
    }

    pub fn fine_y(&self) -> u8 {
        ((self.0 >> 12) & 0x07) as u8
    }
}
```

### Register Types

```rust
/// CPU status register with flag accessors
#[derive(Debug, Clone, Copy, Default)]
pub struct StatusRegister(u8);

impl StatusRegister {
    pub fn carry(&self) -> bool {
        self.0 & 0x01 != 0
    }

    pub fn set_carry(&mut self, value: bool) {
        if value {
            self.0 |= 0x01;
        } else {
            self.0 &= !0x01;
        }
    }

    pub fn zero(&self) -> bool {
        self.0 & 0x02 != 0
    }

    pub fn set_zero(&mut self, value: bool) {
        if value {
            self.0 |= 0x02;
        } else {
            self.0 &= !0x02;
        }
    }

    // ... other flags
}
```

### Enum with Data

```rust
/// Addressing mode with resolved address
#[derive(Debug, Clone, Copy)]
pub enum ResolvedAddress {
    Implicit,
    Accumulator,
    Immediate(u8),
    Memory(u16),
}

impl ResolvedAddress {
    pub fn address(&self) -> Option<u16> {
        match self {
            Self::Memory(addr) => Some(*addr),
            _ => None,
        }
    }
}
```

## Documentation

### Module Documentation

```rust
//! # CPU Module
//!
//! Implementation of the MOS 6502 CPU used in the NES.
//!
//! ## Overview
//!
//! The CPU runs at 1.789773 MHz (NTSC) and supports 256 opcodes
//! (151 official + 105 unofficial).
//!
//! ## Example
//!
//! ```rust
//! use rustynes_cpu::Cpu;
//!
//! let mut cpu = Cpu::new();
//! cpu.reset(&mut bus);
//! cpu.step(&mut bus);
//! ```
```

### Function Documentation

```rust
/// Execute a single CPU instruction.
///
/// Reads the opcode at the current program counter, decodes it,
/// and executes the instruction. Updates the program counter and
/// returns the number of cycles consumed.
///
/// # Arguments
///
/// * `bus` - Mutable reference to the system bus for memory access
///
/// # Returns
///
/// The number of CPU cycles consumed by the instruction (2-7 cycles).
///
/// # Example
///
/// ```rust
/// let cycles = cpu.step(&mut bus);
/// total_cycles += cycles as u64;
/// ```
///
/// # Panics
///
/// Does not panic. Invalid opcodes are handled gracefully.
pub fn step(&mut self, bus: &mut Bus) -> u8 {
    // ...
}
```

### Inline Comments

```rust
fn execute_adc(&mut self, value: u8) {
    // ADC: Add with Carry
    // A = A + M + C
    // Affects: N, Z, C, V

    let a = self.a as u16;
    let m = value as u16;
    let c = if self.p.carry() { 1u16 } else { 0u16 };

    let result = a + m + c;

    // Set carry if result > 255
    self.p.set_carry(result > 0xFF);

    // Set overflow if sign bit is wrong
    // Overflow occurs when adding two positives gives negative,
    // or adding two negatives gives positive
    let overflow = (!(a ^ m) & (a ^ result) & 0x80) != 0;
    self.p.set_overflow(overflow);

    // Store result (truncated to 8 bits)
    self.a = result as u8;

    // Set N and Z flags based on result
    self.update_nz_flags(self.a);
}
```

## Error Handling

### Error Types

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EmulatorError {
    #[error("Invalid ROM format: {0}")]
    InvalidRom(String),

    #[error("Unsupported mapper: {0}")]
    UnsupportedMapper(u16),

    #[error("ROM file not found: {path}")]
    RomNotFound { path: String },

    #[error("Save state version mismatch: expected {expected}, got {actual}")]
    SaveStateVersion { expected: u32, actual: u32 },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, EmulatorError>;
```

### Error Propagation

```rust
// Use ? for propagation
pub fn load_rom(path: &Path) -> Result<Cartridge> {
    let data = std::fs::read(path)?;
    let header = parse_header(&data)?;
    let mapper = create_mapper(header.mapper_id)?;
    Ok(Cartridge::new(header, mapper, data))
}

// Provide context with map_err or anyhow
pub fn load_rom_with_context(path: &Path) -> Result<Cartridge> {
    let data = std::fs::read(path).map_err(|e| {
        EmulatorError::RomNotFound {
            path: path.display().to_string(),
        }
    })?;
    // ...
}
```

## Testing

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adc_no_carry() {
        let mut cpu = Cpu::new();
        cpu.a = 0x10;
        cpu.execute_adc(0x20);

        assert_eq!(cpu.a, 0x30);
        assert!(!cpu.p.carry());
        assert!(!cpu.p.zero());
        assert!(!cpu.p.negative());
    }

    #[test]
    fn test_adc_with_carry_in() {
        let mut cpu = Cpu::new();
        cpu.a = 0x10;
        cpu.p.set_carry(true);
        cpu.execute_adc(0x20);

        assert_eq!(cpu.a, 0x31);
    }

    #[test]
    fn test_adc_overflow() {
        let mut cpu = Cpu::new();
        cpu.a = 0x7F; // 127
        cpu.execute_adc(0x01);

        assert_eq!(cpu.a, 0x80); // -128 in signed
        assert!(cpu.p.overflow());
        assert!(cpu.p.negative());
    }
}
```

### Integration Tests

```rust
// tests/nestest.rs
use rustynes_core::Emulator;
use std::fs;

#[test]
fn test_nestest_rom() {
    let rom_data = fs::read("test-roms/nestest.nes").unwrap();
    let mut emu = Emulator::from_rom_data(&rom_data).unwrap();

    // Run in automated mode
    emu.cpu_mut().set_pc(0xC000);

    // Run until test completion
    for _ in 0..30_000 {
        emu.step();
    }

    // Check result at $0002 and $0003
    assert_eq!(emu.peek(0x0002), 0x00, "Official opcodes failed");
    assert_eq!(emu.peek(0x0003), 0x00, "Unofficial opcodes failed");
}
```

### Property-Based Tests

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_address_wrapping(base: u16, offset: u8) {
        let addr = CpuAddress::new(base);
        let result = addr.wrapping_add(offset as u16);

        // Should never panic and should wrap correctly
        assert_eq!(result.0, base.wrapping_add(offset as u16));
    }

    #[test]
    fn test_vram_address_masking(addr: u16) {
        let vram = VramAddress::new(addr);

        // Should always be in valid range
        assert!(vram.0 < 0x4000);
    }
}
```

## Performance Patterns

### Hot Path Optimization

```rust
impl Cpu {
    /// Main execution loop - performance critical
    #[inline(always)]
    pub fn step(&mut self, bus: &mut Bus) -> u8 {
        let opcode = self.read(bus, self.pc);
        self.pc = self.pc.wrapping_add(1);

        // Use lookup tables instead of match
        let handler = self.instruction_table[opcode as usize];
        let addr_mode = self.addressing_mode_table[opcode as usize];
        let cycles = self.cycle_table[opcode as usize];

        handler(self, bus, addr_mode);
        cycles
    }
}
```

### Avoid Allocations in Hot Paths

```rust
// Bad: allocates on every call
fn get_tile_data(&self, tile_index: u8) -> Vec<u8> {
    let mut data = Vec::with_capacity(16);
    // ...
    data
}

// Good: use fixed-size array
fn get_tile_data(&self, tile_index: u8) -> [u8; 16] {
    let mut data = [0u8; 16];
    // ...
    data
}

// Good: fill caller-provided buffer
fn get_tile_data(&self, tile_index: u8, buffer: &mut [u8; 16]) {
    // ...
}
```

### Use Bitwise Operations

```rust
// Bad: division and modulo
let coarse_x = (addr / 32) % 32;
let coarse_y = (addr / 1024) % 32;

// Good: bitwise operations
let coarse_x = (addr & 0x001F) as u8;
let coarse_y = ((addr >> 5) & 0x001F) as u8;
```

## Unsafe Code

### When Allowed

Unsafe code is permitted only for:
- FFI (rcheevos, platform APIs)
- Performance-critical paths with proven safety
- Hardware register simulation

### Documentation Requirements

```rust
/// Read from OAM memory.
///
/// # Safety
///
/// This uses unchecked array access for performance. The caller
/// must ensure `addr` is in range 0..256.
#[inline(always)]
pub unsafe fn read_oam_unchecked(&self, addr: u8) -> u8 {
    // SAFETY: addr is guaranteed to be < 256 by the u8 type,
    // and self.oam has exactly 256 elements
    *self.oam.get_unchecked(addr as usize)
}
```

### Prefer Safe Alternatives

```rust
// Instead of unsafe, use checked methods with assertions
pub fn read_oam(&self, addr: u8) -> u8 {
    self.oam[addr as usize]
}

// Or use get() with expect for better error messages
pub fn read_oam(&self, addr: u8) -> u8 {
    *self.oam.get(addr as usize).expect("OAM address out of range")
}
```

## Commit Messages

Follow conventional commits:

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation only
- `style`: Formatting, no code change
- `refactor`: Code change that neither fixes bug nor adds feature
- `perf`: Performance improvement
- `test`: Adding or correcting tests
- `chore`: Maintenance tasks

Examples:

```
feat(cpu): implement unofficial opcodes LAX and SAX

Add support for the commonly used unofficial opcodes:
- LAX: Load A and X with memory
- SAX: Store A AND X to memory

These are required for several commercial games.

Closes #42
```

```
fix(ppu): correct sprite 0 hit timing

The sprite 0 hit flag was being set one cycle early,
causing visual glitches in games like Super Mario Bros.

The fix aligns hit detection with dot 2 of the visible
scanline, matching hardware behavior.

Fixes #87
```

## Code Review Checklist

- [ ] Follows naming conventions
- [ ] Has appropriate documentation
- [ ] Includes unit tests
- [ ] No unnecessary allocations in hot paths
- [ ] Error handling is appropriate
- [ ] No clippy warnings
- [ ] Formatted with rustfmt
- [ ] Unsafe code is documented and justified
- [ ] Commit message follows conventions

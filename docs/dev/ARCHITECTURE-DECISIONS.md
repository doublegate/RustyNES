# Architecture Decisions for RustyNES Port

This document captures architecture decisions made during the port from C++ reference implementations (Mesen2, puNES) to pure Rust.

## Reference Implementation Analysis Summary

### Mesen2 (Gold Standard Accuracy)
- **CPU**: Table-driven opcode dispatch, all 256 opcodes including unofficial
- **PPU**: Template-based design for variants, dot-by-dot rendering, open bus emulation
- **APU**: Delta-based mixing, catch-up execution model, cycle-accurate frame counter
- **Timing**: Master clock divider system for accurate NTSC/PAL/Dendy

### puNES (Mapper Coverage)
- **Mappers**: 842 files covering extensive mapper library
- **Pattern**: Function pointer tables for mapper extensibility
- **IRQ**: Dedicated A12/L2F IRQ handling modules

---

## CPU Architecture Decisions

### 1. Opcode Dispatch: Table-Driven
**Decision**: Use lookup tables for addressing modes and instruction handlers.

```rust
// Addressing mode table (256 entries)
const ADDR_MODE_TABLE: [AddrMode; 256] = [...];

// Instruction type table (256 entries)
const INSTRUCTION_TABLE: [Instruction; 256] = [...];

// Cycle count table (256 entries)
const CYCLE_TABLE: [u8; 256] = [...];
```

**Rationale**: Mesen2 uses this pattern for clarity and performance. Avoids large match statements.

### 2. Cycle-Accurate State Machine
**Decision**: Implement tick() method for sub-instruction stepping.

```rust
pub struct Cpu {
    state: CpuState,
    cycle_in_instruction: u8,
    opcode: u8,
    operand_lo: u8,
    operand_hi: u8,
    effective_address: u16,
    // ...
}

enum CpuState {
    FetchOpcode,
    FetchOperandLo,
    FetchOperandHi,
    ReadEffectiveAddress,
    Execute,
    WriteResult,
    // ...
}
```

**Rationale**: Required for VBlank timing tests (ppu_02, ppu_03) and DMC DMA cycle stealing.

### 3. Interrupt Handling
**Decision**: Edge-triggered NMI, level-triggered IRQ with proper polling.

```rust
pub struct Cpu {
    nmi_pending: bool,
    nmi_previous: bool,  // For edge detection
    irq_sources: u8,     // Bitmask of IRQ sources
    interrupt_inhibit: bool,
}

enum IrqSource {
    Apu = 0x01,
    Dmc = 0x02,
    Mapper = 0x04,
    External = 0x08,
}
```

**Rationale**: Matches hardware behavior. NMI triggers on 0->1 transition, IRQ is level-sensitive.

### 4. DMA Handling
**Decision**: CPU stalls during DMA, tracked via cycle counts.

```rust
pub struct Cpu {
    oam_dma_cycles: u16,  // 513/514 cycles for OAM DMA
    dmc_dma_cycles: u8,   // 4 cycles per DMC sample
}
```

**Rationale**: OAM DMA takes 513 cycles (even) or 514 cycles (odd CPU cycle). DMC DMA steals 4 cycles.

---

## PPU Architecture Decisions

### 1. Dot-by-Dot Rendering
**Decision**: Render one dot per PPU cycle, maintain shift registers.

```rust
pub struct Ppu {
    // Internal registers
    v: u16,  // Current VRAM address (15 bits)
    t: u16,  // Temporary VRAM address (15 bits)
    x: u8,   // Fine X scroll (3 bits)
    w: bool, // Write toggle

    // Shift registers for background
    bg_shift_pattern_lo: u16,
    bg_shift_pattern_hi: u16,
    bg_shift_attr_lo: u16,
    bg_shift_attr_hi: u16,

    // Latches
    bg_next_tile_id: u8,
    bg_next_tile_attr: u8,
    bg_next_tile_lo: u8,
    bg_next_tile_hi: u8,
}
```

**Rationale**: True cycle accuracy requires dot-level simulation. Mesen2 uses identical register structure.

### 2. Sprite Evaluation with Overflow Bug
**Decision**: Emulate hardware sprite evaluation bug faithfully.

```rust
fn evaluate_sprites(&mut self) {
    // Secondary OAM clear: cycles 1-64
    // Sprite evaluation: cycles 65-256
    // With overflow bug at sprite #8
}
```

**Rationale**: Some games rely on the overflow bug behavior. Mesen2 implements this precisely.

### 3. Open Bus Behavior
**Decision**: Track last bus value for unmapped reads.

```rust
pub struct Ppu {
    open_bus: u8,
    open_bus_decay: [u8; 8],  // Per-bit decay
}
```

**Rationale**: Open bus emulation required for accuracy. Bits decay over ~600ms in hardware.

### 4. VBlank/NMI Timing
**Decision**: NMI raised at dot 1 of scanline 241, VBlank flag set at dot 0.

```rust
fn step_vblank(&mut self) {
    if self.scanline == 241 && self.dot == 1 {
        self.status.set_vblank(true);
        if self.ctrl.generate_nmi() {
            self.nmi_pending = true;
        }
    }
}
```

**Rationale**: This timing matches ppu_02 and ppu_03 test requirements.

---

## APU Architecture Decisions

### 1. Delta-Based Mixing
**Decision**: Use blip-buffer style delta accumulation for audio.

```rust
pub struct ApuTimer {
    previous_cycle: u32,
    timer: u16,
    period: u16,
    last_output: i8,
    deltas: Vec<(u32, i16)>,  // (cycle, delta)
}

impl ApuTimer {
    fn add_output(&mut self, output: i8) {
        if output != self.last_output {
            let delta = output as i16 - self.last_output as i16;
            self.deltas.push((self.previous_cycle, delta));
            self.last_output = output;
        }
    }
}
```

**Rationale**: Mesen2 uses this pattern for efficient audio generation with proper anti-aliasing.

### 2. Frame Counter Timing
**Decision**: Use accurate cycle counts for 4-step and 5-step modes.

```rust
const FRAME_COUNTER_NTSC_4STEP: [u32; 6] = [7457, 14913, 22371, 29828, 29829, 29830];
const FRAME_COUNTER_NTSC_5STEP: [u32; 6] = [7457, 14913, 22371, 29829, 37281, 37282];
const FRAME_COUNTER_PAL_4STEP: [u32; 6] = [8313, 16627, 24939, 33252, 33253, 33254];
const FRAME_COUNTER_PAL_5STEP: [u32; 6] = [8313, 16627, 24939, 33253, 41565, 41566];
```

**Rationale**: Exact cycle counts from Mesen2, verified against Blargg tests.

### 3. Length Counter Table
**Decision**: Use standard 32-entry lookup table.

```rust
const LENGTH_COUNTER_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14,
    12, 16, 24, 18, 48, 20, 96, 22, 192, 24, 72, 26, 16, 28, 32, 30,
];
```

**Rationale**: Standard NES hardware values.

### 4. DMC Sample Address Wrapping
**Decision**: Addresses wrap from $FFFF to $8000.

```rust
fn increment_dmc_address(&mut self) {
    self.current_addr = self.current_addr.wrapping_add(1);
    if self.current_addr == 0 {
        self.current_addr = 0x8000;
    }
}
```

**Rationale**: DMC samples always read from $8000-$FFFF (PRG ROM space).

---

## Mapper Architecture Decisions

### 1. Trait-Based Design
**Decision**: Define Mapper trait with optional methods for flexibility.

```rust
pub trait Mapper: Send + Sync {
    // Required
    fn read_prg(&self, addr: u16) -> u8;
    fn write_prg(&mut self, addr: u16, val: u8);
    fn read_chr(&self, addr: u16) -> u8;
    fn write_chr(&mut self, addr: u16, val: u8);
    fn mirroring(&self) -> Mirroring;

    // Optional with defaults
    fn irq_pending(&self) -> bool { false }
    fn acknowledge_irq(&mut self) {}
    fn clock(&mut self, _cpu_cycles: u8) {}
    fn ppu_bus_read(&mut self, _addr: u16) {}  // For MMC3 A12
    fn save_ram(&self) -> Option<&[u8]> { None }
    fn load_ram(&mut self, _data: &[u8]) {}
}
```

**Rationale**: Flexible trait allows each mapper to implement only what it needs.

### 2. MMC3 A12 IRQ Handling
**Decision**: Clock IRQ on A12 rising edge during PPU accesses.

```rust
impl Mmc3 {
    fn handle_a12(&mut self, addr: u16) {
        let a12 = (addr & 0x1000) != 0;
        if a12 && !self.a12_previous {
            // Rising edge - clock counter
            self.clock_irq_counter();
        }
        self.a12_previous = a12;
    }
}
```

**Rationale**: MMC3 uses PPU A12 for IRQ timing. This is critical for many games.

### 3. Bank Switching Abstraction
**Decision**: Separate PRG and CHR bank mapping logic.

```rust
pub struct BankMapper {
    prg_banks: Vec<usize>,  // Indices into PRG ROM
    chr_banks: Vec<usize>,  // Indices into CHR ROM/RAM
    prg_rom: Vec<u8>,
    chr_memory: Vec<u8>,
    chr_is_ram: bool,
}

impl BankMapper {
    fn map_prg_addr(&self, addr: u16) -> usize {
        let bank = ((addr - 0x8000) / 0x2000) as usize;
        let offset = (addr - 0x8000) % 0x2000;
        self.prg_banks[bank] * 0x2000 + offset as usize
    }
}
```

**Rationale**: Common pattern across most mappers, reduces code duplication.

---

## Bus Architecture Decisions

### 1. CpuBus Trait
**Decision**: Extend Bus with CPU cycle callback for PPU synchronization.

```rust
pub trait Bus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, val: u8);
}

pub trait CpuBus: Bus {
    fn on_cpu_cycle(&mut self);  // Called before each CPU memory access
}
```

**Rationale**: Required for cycle-accurate PPU stepping. PPU runs 3 dots per CPU cycle.

### 2. Memory Map Organization
**Decision**: Handle all memory regions in bus implementation.

```rust
impl Bus for NesBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => self.ppu.read_register(addr & 0x0007),
            0x4000..=0x4015 => self.apu.read_register(addr),
            0x4016 => self.read_controller(0),
            0x4017 => self.read_controller(1),
            0x4018..=0x401F => self.open_bus,
            0x4020..=0xFFFF => self.mapper.read_prg(addr),
        }
    }
}
```

**Rationale**: Centralized memory mapping matches NES hardware organization.

---

## Timing Model

### Master Clock
```rust
const MASTER_CLOCK_NTSC: u32 = 21_477_272;  // 21.477272 MHz
const MASTER_CLOCK_PAL: u32 = 26_601_712;   // 26.601712 MHz

const CPU_DIVIDER_NTSC: u32 = 12;  // CPU = Master / 12
const CPU_DIVIDER_PAL: u32 = 16;   // CPU = Master / 16
const PPU_DIVIDER: u32 = 4;        // PPU = Master / 4
```

### Frame Timing
```rust
const PPU_DOTS_PER_SCANLINE: u16 = 341;
const PPU_SCANLINES_NTSC: u16 = 262;
const PPU_SCANLINES_PAL: u16 = 312;
const PPU_DOTS_PER_FRAME_NTSC: u32 = 341 * 262;  // 89,342
const PPU_DOTS_PER_FRAME_PAL: u32 = 341 * 312;   // 106,392
```

---

## Code Style Decisions

### 1. No Unsafe
**Decision**: Zero unsafe code except for FFI boundaries.

**Rationale**: Rust safety guarantees, easier auditing, reference implementations prove it's achievable.

### 2. no_std Compatible
**Decision**: Core crates (cpu, ppu, apu, mappers) must be no_std compatible.

```rust
#![no_std]
extern crate alloc;

use alloc::vec::Vec;
use alloc::boxed::Box;
```

**Rationale**: Enables WebAssembly deployment and embedded targets.

### 3. Newtype Patterns
**Decision**: Use newtypes for type safety on addresses and values.

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct VramAddress(u16);

impl VramAddress {
    pub fn coarse_x(&self) -> u8 { (self.0 & 0x001F) as u8 }
    pub fn coarse_y(&self) -> u8 { ((self.0 >> 5) & 0x001F) as u8 }
    pub fn nametable_x(&self) -> bool { (self.0 & 0x0400) != 0 }
    pub fn nametable_y(&self) -> bool { (self.0 & 0x0800) != 0 }
    pub fn fine_y(&self) -> u8 { ((self.0 >> 12) & 0x0007) as u8 }
}
```

**Rationale**: Prevents mixing up similar-looking u16 values, self-documenting code.

### 4. Inline Hints
**Decision**: Use #[inline] on hot paths identified in Mesen2.

```rust
#[inline]
pub fn step(&mut self, bus: &mut impl CpuBus) -> u8 {
    // ...
}
```

**Rationale**: Matches Mesen2's `__forceinline` annotations for performance.

---

## Testing Strategy

### 1. nestest.nes Golden Log
- Compare cycle-by-cycle output against reference log
- All 256 opcodes including unofficial
- Validates addressing modes and flag behavior

### 2. Blargg Test Suites
- CPU instruction tests (timing, flags)
- PPU tests (VBlank, sprite 0, scrolling)
- APU tests (timing, channels, mixing)

### 3. Property-Based Testing
- Use proptest for edge cases
- Random instruction sequences
- Memory boundary conditions

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| 1.0 | Dec 30, 2025 | Initial architecture decisions for port |

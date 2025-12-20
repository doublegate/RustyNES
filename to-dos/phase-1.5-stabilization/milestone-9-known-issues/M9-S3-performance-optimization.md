# M9 Sprint 3: Performance Optimization

## Overview

Profile and optimize critical hot paths in CPU, PPU, and APU to achieve 20%+ performance improvement (100 FPS → 120+ FPS).

## Objectives

- [ ] Profile CPU, PPU, APU hot paths
- [ ] Optimize critical rendering loops (PPU scanline rendering)
- [ ] Reduce memory allocations (heap profiling)
- [ ] Benchmark improvements (before/after comparison)
- [ ] Ensure zero performance regressions

## Tasks

### Task 1: Profiling
- [ ] Install profiling tools (cargo-flamegraph, perf, Instruments)
- [ ] Profile full emulation loop (CPU step, PPU step, APU step)
- [ ] Identify hot paths (>10% CPU time)
- [ ] Generate flamegraph (visualize bottlenecks)
- [ ] Document baseline performance metrics

### Task 2: CPU Optimization
- [ ] Inline critical functions (opcode dispatch, addressing modes)
- [ ] Optimize opcode lookup table (reduce indirection)
- [ ] Reduce branching (branchless addressing mode calculation)
- [ ] Test with cpu-intensive games (Mega Man, Castlevania)
- [ ] Benchmark before/after (aim for 10%+ improvement)

### Task 3: PPU Optimization
- [ ] Optimize scanline rendering loop (most critical path)
- [ ] Reduce pixel processing overhead (batch operations)
- [ ] Optimize sprite rendering (early exit for transparent pixels)
- [ ] Consider SIMD for pixel blending (optional)
- [ ] Benchmark before/after (aim for 15%+ improvement)

### Task 4: Memory Optimization
- [ ] Profile heap allocations (cargo-flamegraph --alloc)
- [ ] Reduce allocations in hot paths (use stack, reuse buffers)
- [ ] Preallocate buffers (PPU framebuffer, audio buffer)
- [ ] Use object pooling for frequently allocated structures
- [ ] Benchmark memory usage (before/after)

### Task 5: Benchmarking & Validation
- [ ] Create performance benchmark suite (criterion)
- [ ] Measure FPS for different games (SMB, Zelda, Mega Man)
- [ ] Compare against baseline (v0.7.0)
- [ ] Ensure no accuracy regressions (test ROM pass rate)
- [ ] Document optimization techniques used

## Profiling Tools

### cargo-flamegraph

**Installation:**
```bash
cargo install flamegraph
```

**Usage:**
```bash
# CPU profiling
cargo flamegraph --release -p rustynes-core -- test-roms/smb.nes

# Memory profiling
cargo flamegraph --release --alloc -p rustynes-core -- test-roms/smb.nes
```

### perf (Linux)

**Usage:**
```bash
perf record -F 99 -g target/release/rustynes-desktop test-roms/smb.nes
perf report
```

### Instruments (macOS)

**Usage:**
```bash
# Profile in Xcode Instruments
instruments -t "Time Profiler" target/release/rustynes-desktop test-roms/smb.nes
```

## Optimization Techniques

### 1. Inline Critical Functions

**Before:**
```rust
fn execute_lda(&mut self, addr_mode: AddressingMode, bus: &mut Bus) {
    let addr = self.get_address(addr_mode, bus);
    self.a = self.read(bus, addr);
    self.set_zn(self.a);
}
```

**After:**
```rust
#[inline(always)]
fn execute_lda(&mut self, addr_mode: AddressingMode, bus: &mut Bus) {
    let addr = self.get_address(addr_mode, bus);
    self.a = self.read(bus, addr);
    self.set_zn(self.a);
}
```

### 2. Lookup Table Optimization

**Before:**
```rust
fn step(&mut self, bus: &mut Bus) {
    let opcode = self.read(bus, self.pc);
    let instruction = match opcode {
        0xA9 => Cpu::execute_lda_immediate,
        0xA5 => Cpu::execute_lda_zero_page,
        // ... 256 match arms
    };
    instruction(self, bus);
}
```

**After:**
```rust
const INSTRUCTION_TABLE: [fn(&mut Cpu, &mut Bus); 256] = [
    Cpu::execute_brk,  // 0x00
    Cpu::execute_ora,  // 0x01
    // ... 256 entries (compile-time initialized)
];

#[inline(always)]
fn step(&mut self, bus: &mut Bus) {
    let opcode = self.read(bus, self.pc);
    INSTRUCTION_TABLE[opcode as usize](self, bus);
}
```

### 3. Reduce Branching (Branchless)

**Before:**
```rust
fn get_address(&self, mode: AddressingMode, bus: &Bus) -> u16 {
    match mode {
        AddressingMode::Immediate => self.pc + 1,
        AddressingMode::ZeroPage => bus.read(self.pc + 1) as u16,
        AddressingMode::Absolute => {
            let lo = bus.read(self.pc + 1) as u16;
            let hi = bus.read(self.pc + 2) as u16;
            (hi << 8) | lo
        }
        // ...
    }
}
```

**After:**
```rust
// Use lookup tables instead of match (branchless)
const ADDR_MODE_TABLE: [fn(&Cpu, &Bus) -> u16; 13] = [
    Cpu::addr_immediate,
    Cpu::addr_zero_page,
    Cpu::addr_absolute,
    // ...
];

#[inline(always)]
fn get_address(&self, mode: u8, bus: &Bus) -> u16 {
    ADDR_MODE_TABLE[mode as usize](self, bus)
}
```

### 4. PPU Scanline Rendering Optimization

**Before:**
```rust
fn render_scanline(&mut self) {
    for x in 0..256 {
        let bg_pixel = self.get_background_pixel(x);
        let spr_pixel = self.get_sprite_pixel(x);
        let final_pixel = self.mix_pixels(bg_pixel, spr_pixel);
        self.framebuffer[self.scanline * 256 + x] = final_pixel;
    }
}
```

**After:**
```rust
fn render_scanline(&mut self) {
    // Preallocate buffers
    let mut bg_line = [0u8; 256];
    let mut spr_line = [0u8; 256];

    // Batch render background and sprites
    self.render_background_line(&mut bg_line);
    self.render_sprite_line(&mut spr_line);

    // Mix pixels (consider SIMD)
    let offset = self.scanline * 256;
    for x in 0..256 {
        self.framebuffer[offset + x] = self.mix_pixels(bg_line[x], spr_line[x]);
    }
}
```

### 5. Memory Allocation Reduction

**Before:**
```rust
fn get_tile_data(&self, tile_index: u8) -> Vec<u8> {
    let mut tile = Vec::with_capacity(64); // Allocation on every call!
    // ... fill tile
    tile
}
```

**After:**
```rust
struct PpuContext {
    tile_buffer: [u8; 64], // Reusable buffer
}

fn get_tile_data(&mut self, tile_index: u8) -> &[u8] {
    // Reuse preallocated buffer
    // ... fill self.tile_buffer
    &self.tile_buffer
}
```

## Benchmarking

### Criterion Benchmarks

**Setup:**
```toml
# Cargo.toml
[dev-dependencies]
criterion = "0.5"

[[bench]]
name = "emulation"
harness = false
```

**Benchmark:**
```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_cpu(c: &mut Criterion) {
    let mut cpu = Cpu::new();
    let mut bus = Bus::new();

    c.bench_function("cpu_step", |b| {
        b.iter(|| {
            cpu.step(black_box(&mut bus));
        });
    });
}

fn benchmark_ppu(c: &mut Criterion) {
    let mut ppu = Ppu::new();

    c.bench_function("ppu_scanline", |b| {
        b.iter(|| {
            ppu.render_scanline();
        });
    });
}

criterion_group!(benches, benchmark_cpu, benchmark_ppu);
criterion_main!(benches);
```

## Performance Targets

| Component | v0.7.0 Baseline | v0.8.0 Target | Improvement |
|-----------|-----------------|---------------|-------------|
| **CPU Step** | 10 ns/op | 9 ns/op | 10% |
| **PPU Scanline** | 500 ns/scanline | 425 ns/scanline | 15% |
| **Full Frame** | 16.67 ms (60 FPS) | 13.89 ms (72 FPS) | 20% |
| **Overall FPS** | 100 FPS | 120+ FPS | 20%+ |

## Test Cases

| Game | Complexity | Expected FPS (v0.8.0) |
|------|------------|-----------------------|
| Super Mario Bros. | Low | 150+ FPS |
| Zelda | Medium | 130+ FPS |
| Mega Man 2 | Medium | 125+ FPS |
| Super Mario Bros. 3 | High | 120+ FPS |
| Kirby's Adventure | High | 115+ FPS |

## Acceptance Criteria

- [ ] Profiling complete (flamegraph generated)
- [ ] Hot paths identified and optimized
- [ ] 20%+ performance improvement (100 → 120+ FPS)
- [ ] Memory allocations reduced (heap profiling shows improvement)
- [ ] Benchmarks pass (criterion)
- [ ] Zero accuracy regressions (test ROM pass rate maintained)
- [ ] Tested with 5+ games at target FPS

## Known Bottlenecks

From initial analysis:

1. **PPU Scanline Rendering** - Most critical path (expected 40-50% CPU time)
2. **CPU Opcode Dispatch** - Frequent function calls (expected 20-30% CPU time)
3. **Memory Allocations** - Frequent allocations in hot paths (expected 5-10% overhead)
4. **APU Sample Generation** - Less critical but can optimize (expected 5-10% CPU time)

## Tools & Resources

| Tool | Purpose | Link |
|------|---------|------|
| **cargo-flamegraph** | CPU/memory profiling | [GitHub](https://github.com/flamegraph-rs/flamegraph) |
| **perf** | Linux profiling | Built-in |
| **Instruments** | macOS profiling | Xcode |
| **criterion** | Benchmarking | [Docs](https://docs.rs/criterion/) |
| **valgrind/cachegrind** | Cache profiling | [Website](https://valgrind.org/) |

## Version Target

v0.8.0

# RustyNES Core Emulation Engine: Comprehensive Codebase Audit Report

## 1. Executive Summary

### 1.1 Scope of the Audit

This audit evaluates the core emulation engine of RustyNES, covering the five foundational crates that implement the cycle-accurate Nintendo Entertainment System (NES) and Famicom hardware:

- `crates/rustynes-core`: Master bus coordinator (`LockstepBus`), NES console envelope (`Nes`), save-state serialization (`save_state`), movie recorder (`movie`), and DMA controller.
- `crates/rustynes-cpu`: Ricoh 2A03/2A07 8-bit CPU core, 6502 execution pipeline, addressing mode resolvers, status register arithmetic (`Status`), and cycle timing substrate.
- `crates/rustynes-ppu`: Ricoh 2C02 Picture Processing Unit (PPU), dot-by-dot background fetch pipeline, primary and secondary OAM sprite evaluation FSMs, and framebuffer emission.
- `crates/rustynes-apu`: Ricoh 2A03 Audio Processing Unit (APU), five voice synthesis channels (Pulse 1, Pulse 2, Triangle, Noise, DMC), frame counter, and band-limited step (BLEP) decimation mixer.
- `crates/rustynes-mappers`: Board support package implementing 174 discrete and ASIC cartridge mapper families, bank switching controllers, expansion audio engines, and cartridge header parsing.

Peripheral crates containing host presentation layers, audio backends, and platform FFI boundaries (`rustynes-frontend`, `rustynes-android`, `rustynes-ios`, `rustynes-libretro`, `rustynes-netplay`, and `rustynes-cosim`) were examined solely at their direct interfaces with the core engine.

### 1.2 Audit Methodology & Clean-Room Standard

The audit was executed under a strict clean-room protocol conforming to the `GEMINI.md` Provenance & License Firewall:

- **Zero Reference Source Inspection**: No source code, header files, constants, internal tables, or comments from third-party reference emulators (such as Mesen2, puNES, FCEUX, Nestopia, higan, ares, or TriCNES) were opened, read, quoted, or transcribed.
- **Hardware Ground Truth**: Hardware specifications, timing diagrams, register quirks, and bus conflict semantics were validated exclusively against vendored public documentation (`docs/`, `ref-docs/`, `nesdev_wiki/`), official chip datasheets, and public test ROM validation suites.
- **Static Analysis & Verification**: Code inspection combined Abstract Syntax Tree (AST) analysis, borrow pattern tracing, slice indexing bounds checks, and benchmark examination. All proposed remediations were independently checked against the Rust toolchain (`cargo check --workspace` and `cargo test --workspace`).

### 1.3 Engine Architecture Overview

RustyNES v2.6.23 achieves 100.00% AccuracyCoin compliance (141/141 suite tests) through an integrated hardware design:

1. **Master-Clock Timebase**: Rather than relying on catch-up heuristics, the engine advances on a master-clock resolution timebase (12 master ticks per NTSC CPU cycle, 4 per PPU dot; 16 per PAL CPU cycle, 5 per PPU dot). Each CPU cycle is split into two halves around the physical memory access (`start_cycle` and `end_cycle`), allowing PPU dot events, mid-instruction register writes, and mapper IRQ latches to align at single-dot granularity.
2. **Bus Mutable Ownership Model**: `LockstepBus` owns all mutable peripheral state (RAM, PPU, APU, Cartridge, Mapper, Controllers, and Open-Bus floating latches). The CPU holds no peripherals and borrows `&mut Bus` during instruction stepping (`Cpu::step`), eliminating shared-ownership pointer aliasing (`Rc<RefCell<...>>`) while preserving zero-cost compile-time memory safety.
3. **2-Cycle ALE PPU Pipeline**: The PPU models the multiplexed address/data bus (AD0–AD7) and external 74LS373 octal latch clocked by Address Latch Enable (ALE). Address calculation occurs on even phases (0, 2, 4, 6), while VRAM reads occur on odd phases (1, 3, 5, 7), providing hardware-level modeling of transient bus conditions and hybrid address glitches.
4. **Polyphase Band-Limited Audio**: The APU couples a non-linear lookup-table resistive DAC with a 256-phase, 32-tap windowed-sinc Band-Limited Step (BLEP) decimator and a 3-stage analog IIR filter chain, preventing digital audio aliasing without external DSP dependencies.

### 1.4 High-Level Scorecard & Categorized Summary of Findings

| Category | Status | High | Medium | Low | Summary |
|---|---|---|---|---|---|
| **Security & Safety (R1)** | Caution | 3 | 1 | 1 | 0 `unsafe` blocks in core; critical out-of-bounds panics in PPU/APU snapshot restore; BlipBuf infinite loop/OOM; 32-bit integer overflow in save-state parsing. |
| **Performance & Scheduler (R2)** | Opportunity | 0 | 4 | 2 | Hot helpers un-inlined in 256-way `Cpu::dispatch`; branchy status flag updates; 245,760 redundant stores/frame in PPU fast path; APU heap allocation churn. |
| **Bus Architecture & Ownership (R3)** | Compliant | 0 | 2 | 4 | Single mutable ownership strictly preserved; `ApuBus` trait orphaned; 18 dead methods on `Cpu::Bus`; CPU open-bus lack of decay and partial mapper latch corruption. |
| **Mappers & Save-State Integrity** | Deficient | 4 | 3 | 1 | Battery-backed save RAM/EEPROM dropped on 5 mappers (`BandaiFcg`, `TaitoX1005`, `TxSrom`, `Tqrom`, `Multicart15`); incomplete stubs on Namco 163, MMC5, and MMC1. |
| **Provenance & Firewall (R4)** | Passed | 0 | 0 | 3 | 100% clean-room audit; derived components attributed; 3 non-derived mapper files contain emulator citations requiring sanitization. |

---

## 2. Security & Vulnerability Analysis (R1)

### 2.1 Memory Safety and Unsafe Rust Audit

A repository-wide regex scan for `\bunsafe\b` was executed across all Rust files in the five core crates (`crates/rustynes-core`, `crates/rustynes-cpu`, `crates/rustynes-ppu`, `crates/rustynes-apu`, and `crates/rustynes-mappers`).

**Result**: Exactly **0 unsafe blocks** exist within the core emulation engine. All bus transactions, register decoding, VRAM slicing, and audio DSP routines are implemented in 100% safe Rust.

However, the workspace lint configuration in root `Cargo.toml:253` specifies:

```toml
[workspace.lints.rust]
unsafe_code = "warn"
```

Because the core engine is designed to be completely free of unsafe code, setting `unsafe_code = "warn"` permits unsafe code to be introduced inadvertently in PRs or automated refactoring.

**Recommendation**: Add `#![forbid(unsafe_code)]` as an inner attribute at the crate root of all five core crates (`lib.rs`).

### 2.2 PPU Save-State Deserialization Out-of-Bounds Panic

- **Vulnerability Identifier**: CWE-129 (Improper Validation of Array Index), CWE-20 (Improper Input Validation)
- **Primary Location**: `crates/rustynes-ppu/src/snapshot.rs:676`
- **Secondary Locations**: `crates/rustynes-ppu/src/ppu.rs:3782, 3881, 4816, 5103`
- **Severity**: High

#### Vulnerability Mechanism

In `crates/rustynes-ppu/src/snapshot.rs:676`, the snapshot restoration routine reads:

```rust
self.spr_count = r.u8()?;
self.spr_zero_in_line = r.u8()? != 0;
```

`spr_count` represents the count of in-range sprites identified on secondary OAM for rendering on the active scanline. Under the 2C02 hardware architecture, secondary OAM holds at most 8 sprites (32 bytes total; 4 bytes per sprite). Consequently, all internal sprite shift registers, coordinate buffers, and attribute arrays in `Ppu` are declared as fixed-size arrays of length 8:

```rust
pub(crate) spr_halted: [bool; 8],
pub(crate) spr_x: [u8; 8],
pub(crate) spr_shift_lo: [u16; 8],
pub(crate) spr_shift_hi: [u16; 8],
pub(crate) spr_attr: [u8; 8],
```

When restoring a `.rns` save state, `r.u8()?` assigns `self.spr_count` without checking `spr_count <= 8`. If an untrusted or corrupted snapshot injects `spr_count > 8` (e.g. `9` or `255`), the snapshot deserializes without error, but the subsequent PPU dot tick panics with an index out-of-bounds error in any of four loops:

1. `ppu.rs:3782`: `for i in 0..self.spr_count as usize { self.spr_halted[i] = false; }`
2. `ppu.rs:3881`: `for i in 0..self.spr_count as usize { self.spr_halted[i] = false; }`
3. `ppu.rs:4816`: `for i in 0..self.spr_count as usize { let emit_active = self.spr_x[i] == 0 || self.spr_halted[i]; ... }`
4. `ppu.rs:5103`: `for i in 0..self.spr_count as usize { if self.spr_halted[i] || self.spr_x[i] == 0 { ... } }`

Because production release builds specify `panic = "abort"` (`Cargo.toml:289`), this defect causes an immediate process crash, creating a Denial-of-Service vector when loading untrusted `.rns` save-state files.

#### Remediation

Define `InvalidSprCount(u8)` in `PpuSnapshotError` and validate `spr_count <= 8` during deserialization.

### 2.3 APU Channel State Deserialization Panics

- **Vulnerability Identifier**: CWE-129 (Improper Validation of Array Index)
- **Primary Locations**: `crates/rustynes-apu/src/snapshot.rs:282-297, 325-326, 401, 417`
- **Secondary Locations**: `crates/rustynes-apu/src/pulse.rs:182`, `triangle.rs:140`, `mixer.rs:75-78`
- **Severity**: High

#### Vulnerability Mechanism

In `crates/rustynes-apu/src/snapshot.rs`, multiple audio channel parameters are restored directly from input bytes without range validation:

1. **Pulse Duty & Step**: `read_pulse` deserializes `duty = r.u8()?` and `step = r.u8()?` without checking `duty < 4` or `step < 8`. In `pulse.rs:182`, `Pulse::output` executes:

   ```rust
   DUTY_TABLE[self.duty as usize][self.step as usize]
   ```

   Where `const DUTY_TABLE: [[u8; 8]; 4]`. Values `duty >= 4` or `step >= 8` trigger an out-of-bounds slice panic on the next audio tick.

2. **Triangle Step**: `read_triangle` deserializes `step = r.u8()?` without checking `step < 32`. In `triangle.rs:140`, `Triangle::output` executes `TRIANGLE_TABLE[self.step as usize]`, where `const TRIANGLE_TABLE: [u8; 32]`. Values `step >= 32` panic immediately.

3. **Envelope Decay**: `read_envelope` deserializes `decay = r.u8()?` without checking `decay <= 15`. In `mixer.rs:75`, pulse outputs are summed as `p_idx = (p1 + p2) as usize`, indexing `pulse_table: [f32; 31]`. Decayed values `> 15` can yield `p_idx >= 31`, panicking on `pulse_table[p_idx]`.

4. **DMC DAC Output**: `read_dmc` deserializes `dac = r.u8()?` without checking `dac <= 127`. In `mixer.rs:76`, `t_idx = (3 * tri + 2 * noise + dmc) as usize`, indexing `tnd_table: [f32; 203]`. Values `dac > 127` can drive `t_idx >= 203`, panicking on `tnd_table[t_idx]`.

#### Remediation

Validate all channel parameters against hardware limits during deserialization (`duty < 4`, `step < 8`, `triangle_step < 32`, `decay <= 15`, `dac <= 127`).

### 2.4 BlipBuf Floating-Point Deserialization & Infinite Loop/OOM Vulnerability

- **Vulnerability Identifier**: CWE-835 (Loop with Unreachable Exit Condition), CWE-400 (Uncontrolled Resource Consumption)
- **Primary Location**: `crates/rustynes-apu/src/snapshot.rs:541-552`
- **Secondary Location**: `crates/rustynes-apu/src/blip.rs:268-271, 301`
- **Severity**: High

#### Vulnerability Mechanism

In `crates/rustynes-apu/src/snapshot.rs:541-552`, `read_blip` deserializes floating-point parameters:

```rust
fn read_blip(r: &mut R<'_>) -> Result<BlipBuf, ApuSnapshotError> {
    let sample_rate = r.u32()?;
    let cpu_rate = r.f64()?;
    let phase = r.f64()?;
    let filter = read_filter(r)?;
    let held_value = r.f32()?;
    let mut b = BlipBuf::new(sample_rate, cpu_rate);
    b.phase = phase;
    b.filter = filter;
    b.held_value = held_value;
    Ok(b)
}
```

In `crates/rustynes-apu/src/blip.rs:268-271`:

```rust
self.phase += self.step;
while self.phase >= 1.0 {
    self.phase -= 1.0;
    self.head = self.head.wrapping_add(1);
    ...
    let filtered = self.filter.process(self.integrator);
    self.samples.push(filtered);
}
```

Under IEEE-754 arithmetic:

- If a malformed save state specifies `phase = f64::INFINITY`, `self.phase >= 1.0` evaluates to `true`, and `self.phase -= 1.0` evaluates to `f64::INFINITY`.
- The `while` loop condition never terminates.
- On each iteration, `self.samples.push(filtered)` executes, appending an element to the heap vector.
- The thread enters an infinite loop, consuming 100% CPU on that core and exhausting memory until the process terminates via an Out-Of-Memory (OOM) abort.
- If `cpu_rate == 0.0` with a positive `sample_rate`, `self.step` becomes `Infinity` and triggers the same infinite-loop/OOM condition.
- If `cpu_rate < 0.0` or `sample_rate == 0`, `self.step` becomes `NaN` or a non-positive step, which prevents the emission loop and produces no samples under finite conditions.

#### Remediation

In `read_blip`, validate that `sample_rate > 0`, `cpu_rate.is_finite() && cpu_rate > 0.0`, `phase.is_finite() && (0.0..1.0).contains(&phase)`, and `held_value.is_finite()`.

### 2.5 32-Bit Integer Overflow in Save-State Header Slicing

- **Vulnerability Identifier**: CWE-190 (Integer Overflow or Wraparound)
- **Location**: `crates/rustynes-core/src/save_state.rs:504-512`
- **Severity**: Medium

#### Vulnerability Mechanism

In `crates/rustynes-core/src/save_state.rs:504-512`:

```rust
let len = u32::from_le_bytes(len_bytes) as usize;
let body_start = self.pos + 9;
let body_end = body_start + len;
if body_end > self.src.len() {
    return Some(Err(SnapshotError::SectionTruncated { ... }));
}
let body = &self.src[body_start..body_end];
self.pos = body_end;
```

In release mode (`Cargo.toml:290`), `overflow-checks = false` is active. On 32-bit architectures (`thumbv7em-none-eabihf` for embedded targets, and `wasm32-unknown-unknown` for web builds), `usize` is 32 bits wide.

If a crafted section header provides a large declared `len` (e.g. `0xFFFF_FFF8`), the addition `body_start + len` wraps around modulo $2^{32}$. For instance, with `body_start = 9`, `body_end` evaluates to `1`.

1. The truncation check evaluates `if 1 > self.src.len()`, which returns `false` whenever `self.src.len() >= 1`.
2. Execution proceeds to `let body = &self.src[9..1]`.
3. Slicing with `start > end` produces an unconditional panic: `slice index starts at 9 but ends at 1`.

#### Remediation

Replace `body_start + len` with `body_start.checked_add(len)` and reject the section if arithmetic overflow occurs.

### 2.6 Stub and Panic Macro Verification

A search across the core crates confirmed:

- `todo!()`: **0 occurrences**
- `unimplemented!()`: **0 occurrences**
- `panic!()`: **0 occurrences** in production code (confined exclusively to test suites)
- `unreachable!()`: **5 occurrences** in production code:
  - `header.rs:133`: Match on `h[12] & 0x03` (2-bit mask covering arms 0..=3).
  - `header.rs:147`: Match on `h[7] & 0x03` (2-bit mask covering arms 0..=3).
  - `ppu.rs:2903, 3141`: Match on `reg & 0x07` covering arms 0..=7.
  - `fds.rs:915`: Match on `entry & 0x07` covering arms 0..=7.

All 5 `unreachable!()` invocations occur on bitmasked variables where all possible mathematical permutations are explicitly matched. They are mathematically dead branches.

---

## 3. Performance Optimization & Scheduler Analysis (R2)

### 3.1 Cycle-Accurate Lockstep Scheduler Analysis

RustyNES uses the v2.0 master-clock R1 single-clock scheduler. Each CPU memory cycle is split into two halves around the physical bus access via `start_cycle` and `end_cycle` (`crates/rustynes-cpu/src/cpu.rs:694-754`).

#### Scheduler Substrate Hotspots

##### Hotspot A: Redundant Trait Method Polling & Integer Division

In `start_cycle` and `end_cycle`:

```rust
let div = bus.cpu_divider();
let pre = if for_read { read_split(div).0 } else { write_split(div).0 };
```

On NTSC, the CPU divider is 12 (invariant). `bus.cpu_divider()` is polled twice per CPU cycle via trait dynamic/monomorphized dispatch (3.58M calls/sec), invoking integer division (`div / 2 - 1`) repeatedly. Providing an inline fast path for NTSC divisor 12 avoids division entirely.

##### Hotspot B: Retired DMA Drain Memory Store

In `end_cycle`:

```rust
let folded = bus.take_dma_mc_consumed();
```

`bus.take_dma_mc_consumed()` (`crates/rustynes-core/src/bus.rs:4703`) executes `core::mem::take(&mut self.dma_mc_consumed)`. This writes `0` to memory on every single CPU cycle (1.79M stores/sec) to clear an accumulator that was retired in v2.0.0 beta 1 under unified DMA and is structurally zero.

##### Hotspot C: Dead Per-Dot NMI Edge Sampling

In `run_ppu_to` (`crates/rustynes-core/src/bus.rs:4619`), `self.sample_nmi_edge()` executes on every single PPU dot (5.37M times/sec) to latch `nmi_edge_latch` for `Bus::poll_nmi`. However, the live R1 CPU reads `bus.nmi_level()` directly in `handle_interrupts`. `poll_nmi` is dead code; sampling the edge on every PPU dot is unnecessary.

### 3.2 Branchless Status Register Flag Evaluation

- **File**: `crates/rustynes-cpu/src/status.rs:42-45`, `crates/rustynes-cpu/src/cpu.rs:2761-2781`
- **Mechanism**:

Currently, `Status::set_nz` delegates to `bitflags::set`:

```rust
pub fn set_nz(&mut self, value: u8) {
    self.set(Self::ZERO, value == 0);
    self.set(Self::NEGATIVE, value & 0x80 != 0);
}
```

`self.set(flag, bool)` expands to `if val { *self |= flag; } else { *self &= !flag; }`, introducing two conditional branches on almost every instruction. Similarly, `adc` evaluates 4 branches (`CARRY`, `OVERFLOW`, `ZERO`, `NEGATIVE`), and `cmp_with` evaluates 3 branches.

Because 6502 flags map to fixed bit positions (Bit 1 is `ZERO`, Bit 7 is `NEGATIVE`, Bit 0 is `CARRY`, Bit 6 is `OVERFLOW`), these can be computed branchlessly using bitwise operations:

- `set_nz`: `let z = ((value == 0) as u8) << 1; let n = value & 0x80;`
- `adc`: `let c = ((sum >> 8) as u8) & 0x01; let v = (((self.a ^ result) & (value ^ result) & 0x80) >> 1);`
- `cmp_with`: `let c = (!borrow as u8) & 0x01; let z = ((r == 0) as u8) << 1; let n = r & 0x80;`

This eliminates conditional branch predictor pressure while maintaining 100% API compatibility with `Status`.

### 3.3 Compiler Inlining and Cold-Path Outlining in `Cpu`

- **File**: `crates/rustynes-cpu/src/cpu.rs`
- **Mechanism**:

`Cpu::dispatch` spans over 1,200 lines and contains 256 match arms. None of the critical memory access helpers (`read1`, `write1`, `start_cycle`, `end_cycle`, `idle_tick`, `fetch_pc`), addressing resolvers (`addr_zp`, `addr_abs`, `addr_abs_x`), or ALU routines (`adc`, `cmp_with`, `lda`, `ldx`) carry `#[inline]`.

Because of the size of `dispatch`, LLVM's inlining cost threshold refuses to inline these helpers into match arms. For instructions performing multiple memory accesses (e.g. `INC $1234`), CPU registers (`a`, `x`, `y`, `pc`, `s`, `p`) must be spilled to the stack and reloaded on each cycle.

Furthermore, in `read1` (`cpu.rs:872-953`), rare DMC aborts and unified DMA loops are placed inline on the hot path. Outlining them into separate functions marked `#[cold] #[inline(never)]` reduces `read1` to a straight-line sequence of ~12 machine instructions, enabling LLVM to inline `read1` across all 256 opcode arms.

### 3.4 PPU Fast-Path Store Elision and Branchless Palette Silicon Gate

- **File**: `crates/rustynes-ppu/src/ppu.rs:4218, 4231-4234, 4241-4272, 6209-6215`

#### Redundant State Stores

On dots `1..=256` of visible scanlines, `tick_visible_render_fast` executes:

```rust
self.bg_reload_render = true;
self.prev_rendering_enabled = true;
self.rendering_enabled_delayed = true;
self.rendering_enabled_delayed2 = true;
```

The entry guard in `tick()` at lines 3402–3428 already asserts that all four fields are `true`. Writing them on every visible dot performs **245,760 redundant memory stores per frame** into the `Ppu` struct.

#### Phase Dispatch

Currently, `tick_visible_render_fast` evaluates two separate `match phase` blocks, an `if phase == 0`, an `if phase == 7`, and an `if dot == 256`. Consolidating into a single 8-arm `match phase` maps directly to a contiguous jump table, nesting `dot == 256` within phase 7.

#### Branchless Palette Mirroring

`palette_index` evaluates pattern matching over 4 mirror constants (`0x10`, `0x14`, `0x18`, `0x1C`). On real 2C02 silicon, address line A4 is pulled low when `A1 == 0 && A0 == 0`. In integer logic, `idx & !((((idx & 0x03) == 0) as usize) << 4)` mirrors those indices branchlessly.

### 3.5 APU BlipBuf Allocation Churn and Sweep Mute Caching

- **File**: `crates/rustynes-apu/src/blip.rs:300-303, 315-322`, `crates/rustynes-apu/src/pulse.rs:173-188`
- **Heap Allocation Churn**: `BlipBuf::drain_all` uses `core::mem::take(&mut self.samples)`, resetting capacity to 0. Every subsequent frame, `self.samples.push(filtered)` forces the vector to reallocate dynamically from 0 to 1024 elements, generating heap allocation churn for WebAssembly and mobile runtimes that drain audio per frame. Replacing `take` with `core::mem::replace(&mut self.samples, Vec::with_capacity(capacity))` eliminates per-frame allocations.
- **Unbounded Growth**: `add_sample` pushes without a capacity ceiling, leaking memory in headless benchmarks or automated test runs where audio is not drained. Enforcing `MAX_BUFFERED_SAMPLES = 16384` establishes a deterministic memory bound.
- **Pulse Sweep Mute Caching**: `Pulse::output` calls `self.muted()`, which computes `self.sweep_target()` on every CPU cycle (3.58 million calls/sec across both pulse channels). Caching `cached_muted` and updating only when registers are written or during half-frame sweep clocks (~120 Hz) reduces per-cycle arithmetic overhead.

### 3.6 Cartridge Unmapped Read Dynamic Dispatch Bypass

- **File**: `crates/rustynes-core/src/bus.rs:3954-3967`, `crates/rustynes-mappers/src/mapper.rs:43`

In `raw_cpu_read`, every CPU read in `$4020..=$FFFF` executes:

```rust
if self.mapper.cpu_read_unmapped(addr) { ... }
```

This incurs a virtual method call on `Box<dyn Mapper>` on every instruction fetch and data read. For 95% of mappers (NROM, MMC1, MMC3, UxROM, CNROM), PRG space contains no unmapped regions. Adding `pub has_unmapped_reads: bool` to `MapperCaps` (default `false`) allows `raw_cpu_read` to bypass the virtual method call for standard mappers.

---

## 4. Architectural Consistency & Bus Model Verification (R3)

### 4.1 Adherence to the Bus Mutable Ownership Mandate

The RustyNES architecture adheres strictly to the single-mutable-owner pattern:

- In `crates/rustynes-core/src/bus.rs:321-346`, `LockstepBus` encapsulates all mutable hardware state: `ram: Box<[u8; RAM_SIZE]>`, `ppu: Ppu`, `apu: Apu`, `cart: Cartridge`, `mapper: Box<dyn Mapper>`, `controllers: [Controller; 2]`, `open_bus: u8`, and `internal_data_bus: u8`.
- In `crates/rustynes-core/src/nes.rs:117-118`, `Nes` holds `cpu: Cpu` and `bus: LockstepBus`.
- During execution, `Nes::step` invokes `self.cpu.step(&mut self.bus)`.
- The CPU holds no references or pointers to peripheral structs and accesses hardware exclusively via `&mut B where B: Bus`.

### 4.2 Naming and Documentation Drift: Aliasing `Bus` and `CpuBus`

A documentation drift exists between `GEMINI.md` and the codebase:

- `GEMINI.md` refers to `rustynes-core::Bus` and states that the CPU borrows `&mut Bus` during `tick()`.
- In reality, the struct is named `LockstepBus` in `rustynes-core`, `rustynes-core` exports no type named `Bus`, and `Cpu` has no `tick()` method (it uses `step()`).
- The name `LockstepBus` is anachronistic following the retirement of the dot-lockstep scheduler in v2.0.0.

**Recommendation**: Add `pub type Bus = LockstepBus;` and `pub use rustynes_cpu::Bus as CpuBus;` in `crates/rustynes-core/src/lib.rs`.

### 4.3 Orphaned `ApuBus` Trait

In `crates/rustynes-apu/src/apu.rs:37-42`:

```rust
pub trait ApuBus {
    fn dmc_read(&mut self, addr: u16) -> u8;
}
```

A search across the repository confirms that `ApuBus` is never implemented by `LockstepBus` or any other struct, and is never invoked. DMC DMA is driven via push/pull methods on `Apu` (`dmc_dma_pending`, `dmc_dma_addr`, `complete_dmc_dma`). `ApuBus` is dead code.

**Recommendation**: Deprecate `ApuBus` with documentation clarifying that DMC DMA is managed externally.

### 4.4 Interface Bloat in `rustynes_cpu::Bus`

In `crates/rustynes-cpu/src/bus.rs:16-443`, 34 methods are defined on `pub trait Bus`. An audit against `crates/rustynes-cpu/src/cpu.rs` revealed that **22 methods are obsolete dead code** never called by `Cpu`:

- Legacy interrupt polling: `poll_nmi`, `poll_irq`, `poll_irq_at_phase` (replaced by `nmi_level` and `irq_level`).
- Legacy cycle hooks: `on_cpu_cycle`, `cpu_cycle_phi1`, `cpu_cycle_phi2` (replaced by `start_cycle`/`end_cycle`).
- Legacy memory accessors: `cpu_read`, `cpu_write` (replaced by `read`, `write`).
- Legacy DMA hooks: `dmc_dma_pending`, `dmc_dma_defer_load_entry`, `dmc_dma_step`, `dmc_dma_step_idle`, `oam_dma_pending`, `oam_dma_step`, `oam_dma_in_flight`, `oam_dma_overlap_ready`, `dmc_dma_last_was_get`, `oam_dma_overlap_cycle`, `dmc_overlap_begin`, `dmc_overlap_noop_cycle`, `dmc_overlap_get_cycle`, `dmc_overlap_realign_cycle` (replaced by `unified_dma_*`).

**Recommendation**: Mark all 22 obsolete methods with `#[deprecated]`.

### 4.5 Open-Bus Decay Asymmetry and Mapper Latch Corruption

1. **Decay Asymmetry**: `Ppu` models capacitance discharge via a 3-group decay timer (`open_bus_decay: [u32; 3]` with a 1,000,000 PPU dot reload; lines 2554–2591). In contrast, CPU `LockstepBus.open_bus` has no decay timer and holds its driven value indefinitely.
2. **Mapper Latch Corruption (Sachen 150/243)**: In `crates/rustynes-mappers/src/sachen_discrete.rs:1033, 1210`, reads to `$4101` return `(open_bus & 0xF8) | (reg[index] & 0x07)`. Because `Mapper::cpu_read` does not receive `open_bus`, it returns `reg & 0x07` with high bits zeroed. `LockstepBus::raw_cpu_read` (line 3970) then assigns `self.open_bus = v`, wiping out the high 5 bits of the floating bus.
   - **Fix**: Introduce `Mapper::cpu_read_with_open_bus(&mut self, addr: u16, open_bus: u8) -> u8` with default fallback to `self.cpu_read(addr)`.

### 4.6 In-Loop `PpuBusAdapter` Construction in `run_ppu_to`

In `crates/rustynes-core/src/bus.rs:4610-4623` (`run_ppu_to`), `PpuBusAdapter` is instantiated inside `while self.ppu_clock + ppu_div <= target` on every single PPU dot (up to 89,342 times per frame). This occurs because `self.sample_nmi_edge()` takes `&mut self`. Refactoring `sample_nmi_edge` to borrow only `last_nmi_level` and `nmi_edge_latch` allows `PpuBusAdapter` to be hoisted outside the catch-up loop.

### 4.7 Documentation Discrepancies and Error String Fixes

- **Zapper Doc & Default**: `crates/rustynes-core/src/bus.rs:460-462` documents `zapper_temporal_light` as `default **off**` and references broken link `[`Bus::set_zapper_temporal_light`]`. In line 905, it defaults to `true` (ON by default since v2.3.6), and the method belongs to `LockstepBus`.
- **Stale FDS Error**: `crates/rustynes-mappers/src/cartridge.rs:230` states `RomError::FdsUnsupported` is "planned for v2.2.0" despite FDS support being available since v2.2.0 via `Nes::from_disk`.
- **CPU Header**: `crates/rustynes-cpu/src/cpu.rs:20-24` references `Bus::on_cpu_cycle`, which was retired in v2.0.

---

## 5. Cartridge Mappers Engine & Save-State Integrity

### 5.1 Critical Battery-Backed SRAM/EEPROM Save-State Gap

- **Severity**: Critical (Data Loss)
- **Locations**:
  - `crates/rustynes-mappers/src/m016_bandai_fcg.rs:540`
  - `crates/rustynes-mappers/src/m080_taito_x1_005.rs:241`
  - `crates/rustynes-mappers/src/m118_txsrom.rs:109`
  - `crates/rustynes-mappers/src/m119_tqrom.rs:105`
  - `crates/rustynes-mappers/src/multicart_discrete.rs:134`

#### Defect Mechanism

The host frontend persists battery-backed save data to `.sav` files on disk by querying `cartridge.mapper.sram()` and writing the returned slice. On load, it populates the save buffer via `cartridge.mapper.sram_mut()`.

The default `Mapper` trait implementations return empty slices:

```rust
fn sram(&self) -> &[u8] { &[] }
fn sram_mut(&mut self) -> &mut [u8] { &mut [] }
```

Multiple mappers holding battery-backed RAM or serial EEPROM fail to implement `sram()` and `sram_mut()`:

1. **Bandai FCG (`m016_bandai_fcg.rs`)**: Contains `eeprom: Option<Eeprom>` (128-byte or 256-byte serial EEPROM). Serialized in save states, but omits `sram()`. Commercial titles such as *Dragon Ball Z: Kyoushuu! Saiyajin* and *Dragon Ball Z II* fail to persist save files to disk.
2. **Taito X1-005 (`m080_taito_x1_005.rs`)**: Contains `ram: [u8; 128]` at `$7F00-$7FFF`. Serialized in save states, but omits `sram()`. Titles such as *Kyonshiizu 2* fail to persist saves.
3. **TxSROM (`m118_txsrom.rs`) & TQROM (`m119_tqrom.rs`)**: Wrap `inner: Mmc3`, which holds `prg_ram`, but fail to delegate `sram()` / `sram_mut()` to `self.inner`. Titles such as *Ys III: Wanderers from Ys* silently fail to persist saves.
4. **Multicart 15 (`multicart_discrete.rs`)**: Allocates 8 KiB `prg_ram`, reads and writes it at `$6000-$7FFF`, but omits `sram()`.

### 5.2 Incomplete Stubs: Namco 163 CIRAM/CHR Nametable Banking

- **Location**: `crates/rustynes-mappers/src/m019_namco163.rs:604-609, 641`
- **Defect**: In `cpu_write`, registers `$C000..=$DFFF` are unhandled with a comment stating *"Additional CHR / NTA bank selects on real hardware. Not wired up here"*. In `ppu_read` (line 641), nametables are hardcoded to CIRAM mirroring.
- **Impact**: Namco 163 hardware uses `$C000`, `$C800`, `$D000`, `$D800` to select either 1 KiB CHR-ROM banks (`< 0xE0`) or CIRAM pages (`>= 0xE0`) for each nametable quadrant. Games relying on dynamic nametable switching or CHR-ROM nametables render incorrect tiles. The `self.nta: [u8; 4]` array is serialized into `.rns` save states but is never read or written in emulation.

### 5.3 Incomplete Stubs: MMC5 Multi-Bank PRG-RAM Banking

- **Location**: `crates/rustynes-mappers/src/m005_mmc5.rs:675-681, 739-742, 986-993, 1150-1156`
- **Defect**: Register `$5113` updates `self.prg_ram_bank = value & 0x7F;` (line 1081). However, in `cpu_read` (line 987) and `cpu_write` (line 1152), the offset is computed as `(addr - 0x6000) as usize`, ignoring `self.prg_ram_bank`. In `read_prg_window` (line 678), `raw.page()` is ignored when RAM is mapped into `$8000-$DFFF`. All accesses are hardcoded to bank 0.
- **Impact**: MMC5 games utilizing 16 KiB to 64 KiB PRG-RAM (such as Koei simulation titles with multiple save slots: *Nobunaga's Ambition II*, *Bandit Kings of Ancient China*) overwrite bank 0 and cannot access upper RAM banks.

### 5.4 Incomplete Stubs: MMC1 SUROM/SXROM 512 KiB PRG Banking

- **Location**: `crates/rustynes-mappers/src/m001_mmc1.rs:168`
- **Defect**: Line 168 calculates `let prg_bank = self.prg & 0x0F;`, unconditionally masking the 5-bit register with `0x0F`.
- **Impact**: Upper banks 16..31 (256 KiB..512 KiB) cannot be selected. On SUROM carts (*Dragon Warrior IV*), CHR A16 (`$A000.4`) controls PRG A18. This wiring is absent.

### 5.5 Open-Bus Floating at `$6000-$7FFF` in Discrete Boards

- **Locations**: `crates/rustynes-mappers/src/m004_mmc3.rs:473-480`, `m007_axrom.rs:81-89`, `m003_cnrom.rs:90-95`, `m002_uxrom.rs:104-118`
- **Defect**: When `!self.prg_ram_enabled` or on discrete mappers without RAM, `cpu_read(0x6000..=0x7FFF)` returns `0`. Because `cpu_read_unmapped` is not overridden, the bus emits `0x00` instead of floating to the open-bus latch. Line 1049 of `m004_mmc3.rs` tests: `assert_eq!(m.cpu_read(0x6000), 0); // returns 0 (open bus stub)`.

### 5.6 Hardwired Mirroring Gate Omissions

- **Location**: `crates/rustynes-mappers/src/mapper.rs:534`
- **Defect**: Only 4 mappers (`Nrom`, `Uxrom`, `Cnrom`, `Gxrom`) override `has_hardwired_mirroring() -> bool { true }`. Over 11 discrete fixed-mirroring mappers (`ColorDreams`, `Cprom`, `Bnrom`, `Bandai74`, `CamericaBf9093`, `Un1rom`, `Nichibutsu180`, `Sunsoft1`, `JalecoLatch` boards) default to `false`.
- **Impact**: Any header mirroring mistake in ROM dumps for these boards cannot be corrected by the per-game database (`Nes::set_mirroring_override`).

---

## 6. Strict Provenance & License Firewall Audit (R4)

### 6.1 Clean-Room Verification and Black-Box Oracle Compliance

RustyNES is licensed **GPL-3.0-or-later** because it incorporates code derived from GPL reference emulators (Mesen2 GPLv3, puNES/FCEUX/Nestopia GPLv2-or-later) per ADR 0036, credited in `docs/originality-and-provenance.md` and `NOTICE`.

Under `GEMINI.md`:
> *"Reference emulators (Mesen2, puNES, FCEUX, Nestopia, higan, ares, GeraNES, TriCNES, tetanes, …) are **black-box oracles**. You may run them and read their output (framebuffers, traces, audio, logs). You **must not** open, read, quote, or reproduce their source (`.c`/`.cpp`/`.h`/`.cs`/`.rs`), constants, tables, variable names, code ordering, or comments — not 'for reference', not once."*

All analyses and audits in this report were performed strictly via black-box examination, static analysis of the RustyNES codebase, and public NESdev documentation. No external emulator source files were opened, read, or quoted.

### 6.2 Sanitization of Emulator Source Citations in Non-Derived Mappers

The audit revealed three non-derived mapper files that cite reference emulator source files or quote C++ expressions in comments:

1. **`crates/rustynes-mappers/src/m085_vrc7.rs:362, 947`**:
   - Line 362 cites `per Vrc7Audio.h (Mesen2) this is the canonical shape`.
   - Line 947 notes `Mesen2 calls`.
   - *Action*: Sanitize comments to cite the Yamaha YM2413 / Konami 053982 application manual.
2. **`crates/rustynes-mappers/src/m099_vs_system.rs:135, 160-161, 399`**:
   - *Action*: Remove verbatim source expressions while preserving the documented chrOuter and prgOuter behavior. Cite public Nintendo Vs. DualSystem hardware documentation instead.
3. **`crates/rustynes-mappers/src/m244_cne_decathlon.rs:115`**:
   - Quotes `(Mesen2 Mapper244 / puNES mapper_244 carry the identical tables.)`.
   - *Action*: Sanitize comment to cite public test vectors.

### 6.3 Verification of Derived Component Attributions

All genuine ports in the codebase properly carry required `// Provenance:` headers:

- **PPU**: `crates/rustynes-ppu/src/ppu.rs:3` and `src/palette_gen.rs:3` disclose Mesen2, TriCNES, and ares derivations.
- **APU**: `crates/rustynes-apu/src/blip.rs:3` discloses Shay Green's `blip_buf` (LGPL-2.1-or-later). `crates/rustynes-apu/src/opll.rs:3` discloses Mitsutaka Okazaki's `emu2413` (MIT license).
- **Mappers**: 14 mapper files carrying derivations are properly attributed (`fds.rs`, `kaiser.rs`, `m016_bandai_fcg.rs`, `m035_jy_asic.rs`, `m069_sunsoft_fme7.rs`, `m176_bmc_fk23c.rs`, `m268_bmc_coolboy.rs`, `m513_sachen_9602.rs`, `mmc3_clones.rs`, `multicart_discrete.rs`, `ntdec.rs`, `sachen_discrete.rs`, `unif.rs`, and `lib.rs`).

---

## 7. Actionable Codebase Improvements Summary Table

### 7.1 Summary Table

| ID | File Path | Line Range | Category | Impact | Description |
|---|---|---|---|---|---|
| **IMP-01** | `crates/rustynes-ppu/src/snapshot.rs` | 676 | Security | High (Crash) | Validate `spr_count <= 8` in PPU snapshot restore to prevent out-of-bounds slice panics. |
| **IMP-02** | `crates/rustynes-apu/src/snapshot.rs` | 282, 325, 401, 541 | Security | High (Crash/OOM) | Range-check APU channel states (`duty`, `step`, `decay`, `dac`) and validate finite `BlipBuf` float parameters. |
| **IMP-03** | `crates/rustynes-core/src/save_state.rs` | 504–512 | Security | Medium (Panic) | Use `checked_add` when parsing section lengths to prevent 32-bit integer overflow panics. |
| **IMP-04** | `crates/rustynes-cpu/src/status.rs` | 42–46 | Performance | Medium (Throughput) | Implement branchless `set_nz`, `adc`, and `cmp_with` using bitwise flag extraction. |
| **IMP-05** | `crates/rustynes-cpu/src/cpu.rs` | 872–976 | Performance | Medium (Throughput) | Add `#[inline]` to critical memory helpers and outline rare DMA/abort loops with `#[cold] #[inline(never)]`. |
| **IMP-06** | `crates/rustynes-ppu/src/ppu.rs` | 4218, 4241, 6209 | Performance | Medium (Throughput) | Elide 245,760 redundant stores/frame, unify phase match, and use branchless `palette_index` decoding. |
| **IMP-07** | `crates/rustynes-apu/src/blip.rs` | 300–322 | Performance | Medium (Memory/Churn) | Preserve `Vec` capacity in `drain_all` and enforce `MAX_BUFFERED_SAMPLES` ceiling in `add_sample`. |
| **IMP-08** | `crates/rustynes-mappers/src/m016_bandai_fcg.rs` | 540–550 | Save-State | Critical (Data Loss) | Implement `Mapper::sram` and `sram_mut` on `BandaiFcg` to persist serial EEPROM to `.sav` disk files. |
| **IMP-09** | `crates/rustynes-mappers/src/m080_taito_x1_005.rs` | 241–250 | Save-State | Critical (Data Loss) | Implement `Mapper::sram` and `sram_mut` on `TaitoX1005` to persist 128-byte battery RAM. |
| **IMP-10** | `crates/rustynes-mappers/src/m118_txsrom.rs` | 109–120 | Save-State | Critical (Data Loss) | Forward `sram` and `sram_mut` to `inner: Mmc3` in `TxSrom` and `Tqrom`. |
| **IMP-11** | `crates/rustynes-mappers/src/m019_namco163.rs` | 604–641 | Mappers | High (Accuracy) | Implement `$C000-$DFFF` nametable registers and CIRAM/CHR-ROM nametable decoding. |
| **IMP-12** | `crates/rustynes-core/src/bus.rs` | 4610–4623 | Consistency | Low (Overhead) | Refactor `sample_nmi_edge` borrows to hoist `PpuBusAdapter` out of the inner PPU dot loop. |

---

### 7.2 Syntactically Valid Rust Improvement Snippets

#### Snippet 1: PPU Snapshot Sprite Count Validation (`crates/rustynes-ppu/src/snapshot.rs:676`)

```rust
        // Range-check spr_count: the secondary OAM holds at most 8 sprites,
        // and internal sprite arrays (spr_halted, spr_x, spr_shift_lo, spr_shift_hi)
        // are fixed to 8 elements. Values > 8 cause slice panics during dot ticking.
        let spr_count = r.u8()?;
        if spr_count > 8 {
            return Err(PpuSnapshotError::InvalidSprCount(spr_count));
        }
        self.spr_count = spr_count;
        self.spr_zero_in_line = r.u8()? != 0;
```

#### Snippet 2: APU Snapshot Channel & BlipBuf Validation (`crates/rustynes-apu/src/snapshot.rs:282, 325, 541`)

```rust
fn read_pulse(r: &mut R<'_>) -> Result<Pulse, ApuSnapshotError> {
    let duty = r.u8()?;
    let step = r.u8()?;
    if duty >= 4 || step >= 8 {
        return Err(ApuSnapshotError::InvalidPulseState { duty, step });
    }
    let timer_period = r.u16()?;
    let timer = r.u16()?;
    let envelope = read_envelope(r)?;
    let length = read_length(r)?;
    let sweep_enabled = r.bool()?;
    let sweep_period = r.u8()?;
    let sweep_negate = r.bool()?;
    let sweep_shift = r.u8()?;
    let sweep_reload = r.bool()?;
    let sweep_divider = r.u8()?;
    let is_pulse1 = r.bool()?;
    let mut p = Pulse::new(is_pulse1);
    p.duty = duty;
    p.step = step;
    p.timer_period = timer_period;
    p.timer = timer;
    p.envelope = envelope;
    p.length = length;
    p.sweep_enabled = sweep_enabled;
    p.sweep_period = sweep_period;
    p.sweep_negate = sweep_negate;
    p.sweep_shift = sweep_shift;
    p.sweep_reload = sweep_reload;
    p.sweep_divider = sweep_divider;
    Ok(p)
}

fn read_blip(r: &mut R<'_>) -> Result<BlipBuf, ApuSnapshotError> {
    let sample_rate = r.u32()?;
    let cpu_rate = r.f64()?;
    let phase = r.f64()?;
    if sample_rate == 0
        || !cpu_rate.is_finite()
        || cpu_rate <= 0.0
        || !phase.is_finite()
        || !(0.0..1.0).contains(&phase)
    {
        return Err(ApuSnapshotError::InvalidAudioParameter);
    }
    let filter = read_filter(r)?;
    let held_value = r.f32()?;
    if !held_value.is_finite() {
        return Err(ApuSnapshotError::InvalidAudioParameter);
    }
    let mut b = BlipBuf::new(sample_rate, cpu_rate);
    b.phase = phase;
    b.filter = filter;
    b.held_value = held_value;
    Ok(b)
}
```

#### Snippet 3: Checked Addition in Save-State Section Header (`crates/rustynes-core/src/save_state.rs:504-515`)

```rust
        let body_start = self.pos + 9;
        let Some(body_end) = body_start.checked_add(len) else {
            return Some(Err(SnapshotError::SectionTruncated {
                tag: tag_string(tag),
                declared: len,
                got: self.src.len().saturating_sub(body_start),
            }));
        };
        if body_end > self.src.len() {
            return Some(Err(SnapshotError::SectionTruncated {
                tag: tag_string(tag),
                declared: len,
                got: self.src.len() - body_start,
            }));
        }
        let body = &self.src[body_start..body_end];
        self.pos = body_end;
        Some(Ok(Section { tag, version, body }))
```

#### Snippet 4: Branchless Flag Calculations (`crates/rustynes-cpu/src/status.rs:42` and `cpu.rs:2761`)

```rust
impl Status {
    /// Set N and Z based on `value` branchlessly with direct bit extraction.
    #[inline]
    pub fn set_nz(&mut self, value: u8) {
        let z = ((value == 0) as u8) << 1;
        let n = value & 0x80;
        *self = Self::from_bits_retain((self.bits() & !(Self::ZERO.bits() | Self::NEGATIVE.bits())) | z | n);
    }
}

// In crates/rustynes-cpu/src/cpu.rs:
    #[inline]
    fn adc(&mut self, value: u8) {
        let carry = u16::from(self.p.contains(Status::CARRY));
        let sum = u16::from(self.a) + u16::from(value) + carry;
        let result = sum as u8;
        let c = ((sum >> 8) as u8) & 0x01;
        let v = ((self.a ^ result) & (value ^ result) & 0x80) >> 1;
        let z = ((result == 0) as u8) << 1;
        let n = result & 0x80;
        let mask = Status::CARRY.bits() | Status::OVERFLOW.bits() | Status::ZERO.bits() | Status::NEGATIVE.bits();
        self.p = Status::from_bits_retain((self.p.bits() & !mask) | c | v | z | n);
        self.a = result;
    }

    #[inline]
    fn cmp_with(&mut self, lhs: u8, rhs: u8) {
        let (r, borrow) = lhs.overflowing_sub(rhs);
        let c = (!borrow as u8) & 0x01;
        let z = ((r == 0) as u8) << 1;
        let n = r & 0x80;
        let mask = Status::CARRY.bits() | Status::ZERO.bits() | Status::NEGATIVE.bits();
        self.p = Status::from_bits_retain((self.p.bits() & !mask) | c | z | n);
    }
```

#### Snippet 5: Cold-Outlined `read1` and `write1` Helpers (`crates/rustynes-cpu/src/cpu.rs:872`)

```rust
    #[cold]
    #[inline(never)]
    fn service_dmc_abort_read<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        if bus.dmc_abort_is_get_cycle() {
            self.cycles_emitted = self.cycles_emitted.saturating_add(1);
            self.start_cycle(bus, true);
            bus.dmc_abort_halt_step(addr);
            self.end_cycle(bus, true);
        } else {
            bus.dmc_abort_cancel();
        }
    }

    #[cold]
    #[inline(never)]
    fn drain_unified_dma<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        while bus.unified_dma_pending() {
            self.cycles_emitted = self.cycles_emitted.saturating_add(1);
            self.start_cycle(bus, true);
            bus.unified_dma_cycle(addr);
            self.end_cycle(bus, true);
        }
    }

    /// Read a byte at `addr` and consume one CPU cycle with inlined hot path.
    #[inline]
    fn read1<B: Bus>(&mut self, bus: &mut B, addr: u16) -> u8 {
        if bus.dmc_abort_pending() {
            self.service_dmc_abort_read(bus, addr);
        }
        if bus.unified_dma_pending() {
            self.drain_unified_dma(bus, addr);
        }
        self.cycles_emitted = self.cycles_emitted.saturating_add(1);
        self.start_cycle(bus, true);
        let v = bus.read(addr);
        self.end_cycle(bus, true);
        v
    }
```

#### Snippet 6: PPU Fast-Path Phase Dispatch & Branchless Palette (`crates/rustynes-ppu/src/ppu.rs:4241, 6209`)

```rust
        // Unified 8-arm background fetch jump table for dots 1..=256.
        let phase = dot.wrapping_sub(1) & 7;
        match phase {
            0 => {
                self.reload_bg_shift_regs();
                self.ale_drive_nt();
            }
            1 => self.fetch_nt(bus),
            2 => self.ale_drive_at(),
            3 => self.fetch_at(bus),
            4 => self.ale_drive_bg_lo(),
            5 => self.fetch_bg_lo(bus),
            6 => self.ale_drive_bg_hi(),
            7 => {
                self.fetch_bg_hi(bus);
                self.inc_hori_v();
                if dot == 256 {
                    self.inc_vert_v();
                }
            }
            _ => unreachable!(),
        }

/// Resolve an address in `$3F00-$3FFF` to a palette RAM index branchlessly.
#[inline]
const fn palette_index(addr: u16) -> usize {
    let idx = (addr & 0x1F) as usize;
    idx & !((((idx & 0x03) == 0) as usize) << 4)
}
```

#### Snippet 7: Eliminate BlipBuf Allocation Churn and Cap Growth (`crates/rustynes-apu/src/blip.rs:300-322`)

```rust
pub const MAX_BUFFERED_SAMPLES: usize = 16_384;

impl BlipBuf {
    #[inline]
    pub fn add_sample_capped(&mut self, filtered: f32) {
        if self.samples.len() < MAX_BUFFERED_SAMPLES {
            self.samples.push(filtered);
        }
    }

    #[must_use]
    pub fn drain_all(&mut self, mut reuse_buffer: Vec<f32>) -> Vec<f32> {
        reuse_buffer.clear();
        core::mem::swap(&mut self.samples, &mut reuse_buffer);
        reuse_buffer
    }
}
```

#### Snippet 8: SRAM Trait Implementation on Bandai FCG (`crates/rustynes-mappers/src/m016_bandai_fcg.rs:540`)

```rust
// In crates/rustynes-mappers/src/m016_bandai_fcg.rs:
// Inside `impl Mapper for BandaiFcg`:

    fn sram(&self) -> &[u8] {
        self.eeprom.as_ref().map_or(&[], |ee| &ee.mem)
    }

    fn sram_mut(&mut self) -> &mut [u8] {
        self.eeprom.as_mut().map_or(&mut [], |ee| &mut ee.mem)
    }
```

#### Snippet 9: SRAM Trait Implementation on Taito X1-005 (`crates/rustynes-mappers/src/m080_taito_x1_005.rs:241`)

```rust
// In crates/rustynes-mappers/src/m080_taito_x1_005.rs:
// Inside `impl Mapper for TaitoX1005`:

    fn sram(&self) -> &[u8] {
        &self.ram
    }

    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.ram
    }
```

#### Snippet 10: SRAM Trait Delegation in TxSROM and TQROM (`crates/rustynes-mappers/src/m118_txsrom.rs:109`)

```rust
// In crates/rustynes-mappers/src/m118_txsrom.rs and m119_tqrom.rs:
// Inside `impl Mapper for TxSrom` / `impl Mapper for Tqrom`:

    fn sram(&self) -> &[u8] {
        self.inner.sram()
    }

    fn sram_mut(&mut self) -> &mut [u8] {
        self.inner.sram_mut()
    }
```

#### Snippet 11: Namco 163 Dynamic Nametable Banking (`crates/rustynes-mappers/src/m019_namco163.rs:604, 641`)

```rust
// In crates/rustynes-mappers/src/m019_namco163.rs:
// In `fn cpu_write`:
            0xC000..=0xDFFF => {
                let slot = ((addr - 0xC000) >> 11) as usize;
                if slot < 4 {
                    self.nta[slot] = value;
                }
            }

// In `fn ppu_read`:
            0x2000..=0x3EFF => {
                let slot = (((addr - 0x2000) >> 10) & 0x03) as usize;
                let reg = self.nta[slot];
                if reg < 0xE0 {
                    // CHR-ROM nametable: 1 KiB bank from CHR-ROM
                    let total_1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
                    let bank = (reg as usize) % total_1k;
                    let off = bank * CHR_BANK_1K + (addr as usize & (CHR_BANK_1K - 1));
                    self.chr_rom[off % self.chr_rom.len()]
                } else {
                    // CIRAM nametable page: bit 0 selects CIRAM page 0 or 1
                    let physical_page = (reg & 0x01) as usize;
                    let off = physical_page * NAMETABLE_SIZE + (addr as usize & (NAMETABLE_SIZE - 1));
                    self.vram[off % self.vram.len()]
                }
            }
```

#### Snippet 12: Narrow Borrow Decoupling in `run_ppu_to` (`crates/rustynes-core/src/bus.rs:3341, 4610`)

```rust
    #[inline]
    fn update_nmi_edge(last_level: &mut bool, edge_latch: &mut bool, nmi_line: bool) {
        if nmi_line && !*last_level {
            *edge_latch = true;
        }
        *last_level = nmi_line;
    }

// In LockstepBus::run_ppu_to:
        let mut adapter = PpuBusAdapter {
            mapper: self.mapper.as_mut(),
            nt_override: self.nt_mirroring_override,
            sub_dot,
            #[cfg(feature = "irq-timing-trace")]
            trace_a12_latest: None,
        };
        while self.ppu_clock + ppu_div <= target {
            adapter.sub_dot = sub_dot;
            self.ppu.tick(&mut adapter);
            Self::update_nmi_edge(
                &mut self.last_nmi_level,
                &mut self.nmi_edge_latch,
                self.ppu.nmi_line(),
            );
            self.ppu_clock += ppu_div;
            sub_dot = sub_dot.wrapping_add(1);
        }
```

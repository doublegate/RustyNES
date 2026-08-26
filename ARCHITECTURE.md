# RustyNES Architecture

**Document Version:** 2.1.0
**Last Updated:** 2026-08-20
**Applies to:** RustyNES v2.6.4 (the scheduling model is v2.0.0 "Timebase" onward)

This document fixes the high-level architecture of RustyNES. The per-subsystem specs under `docs/` (`cpu-6502.md`, `ppu-2c02.md`, `apu-2a03.md`, `mappers.md`, `scheduler.md`) take these decisions as given and elaborate one chip each. After reading this you should know the workspace shape, the scheduling model, the public boundary, and the load-bearing invariants. The canonical, always-current architecture spec is [`docs/architecture.md`](docs/architecture.md); this file is the top-level companion.

---

## Table of Contents

- [System Overview](#system-overview)
- [The Load-Bearing Decisions](#the-load-bearing-decisions)
- [Crate Structure](#crate-structure)
- [Scheduling Model](#scheduling-model)
- [Public API Surface](#public-api-surface)
- [Memory Architecture](#memory-architecture)
- [Module Boundaries and Invariants](#module-boundaries-and-invariants)
- [Concurrency Model](#concurrency-model)
- [Design Patterns](#design-patterns)
- [Performance Considerations](#performance-considerations)

---

## System Overview

RustyNES is a cycle-accurate NES emulator structured as a Cargo workspace of `rustynes-*` crates. The emulation core is a single-threaded, deterministic state machine; the frontend wraps it with `winit` + `wgpu` + `cpal` + `egui` (native and wasm32). The accuracy bar is Mesen2 / higan / ares: tight lockstep at PPU-dot resolution on a master-clock-precise timebase, expressed since v2.0.0 "Timebase" as a single canonical cycle counter with a split-around-the-access PPU catch-up (ADR 0029).

The reader should keep the [load-bearing decisions](#the-load-bearing-decisions) in mind — reading any subsystem doc without them will mislead.

---

## The Load-Bearing Decisions

These cross-cutting choices span many files and are not negotiable without re-deriving large parts of the system.

1. **Tight lockstep at PPU-dot resolution.** Of the three viable scheduling strategies (lockstep with PPU as master, catch-up at CPU-instruction granularity, hybrid), **lockstep is selected.** It is the only model that makes mid-instruction PPU events — sprite-zero hit at a precise dot, mid-scanline scroll writes, MMC3 IRQ at PPU dot 260 — trivially correct rather than requiring per-quirk patches. Mesen2 and ares both use it. The performance cost is modest (target ≤ 2 ms/frame core work).

2. **The Bus owns everything mutable.** The bus type owns the PPU, APU, mapper-via-cart, WRAM, controllers, and open-bus latch; the CPU borrows `&mut Bus` during `tick()`. Per the TetaNES postmortem, splitting the bus from the CPU was the most consequential mistake to avoid, because the alternative ("CPU holds PPU, but PPU also needs the CPU bus") creates an unwinnable borrow-checker fight.

3. **One-directional crate graph.** `rustynes-cpu` has no PPU/APU dependency; `rustynes-ppu` depends on `rustynes-mappers` only; `rustynes-apu` is independent. Each chip is fuzzable and benchmarkable in isolation, and a future non-NES 6502 host port stays structurally possible.

4. **Mapper IRQ logic lives in the mapper.** The PPU emits A12 transitions and scanline/vblank notifications; each mapper owns its own IRQ filter (MMC3's 3-falling-edges-of-M2, MMC5's scanline counter, the VRC/FME-7/N163 per-CPU-cycle counters).

5. **Determinism is a hard contract.** Same seed + ROM + input ⇒ bit-identical framebuffer and audio. No system time, thread scheduling, or OS RNG in the core. This contract is what makes save-states, rewind, TAS replay, and netplay rollback correct. Frontend-only concerns (rate control, run-ahead, pacing) stay in the frontend.

6. **Test ROMs are the spec.** When the docs and a passing test ROM disagree, the ROM wins.

---

## Crate Structure

```text
rustynes/
├── Cargo.toml                  # Workspace definition (edition 2021, MSRV 1.86)
├── crates/
│   ├── rustynes-core/          # Glue: Nes struct, run loop, scheduler, Bus,
│   │                           #   save state, region config. Re-exports chip crates.
│   ├── rustynes-cpu/           # Ricoh 2A03 / 6502 CPU. No PPU/APU deps.
│   ├── rustynes-ppu/           # 2C02 PPU. Depends on rustynes-mappers only.
│   ├── rustynes-apu/           # 2A03 APU (incl. clean-room OPLL FM synthesis).
│   ├── rustynes-mappers/       # Cartridge + Mapper trait + 51 mapper families + FDS.
│   ├── rustynes-netplay/       # GGPO-style rollback netcode (UDP + WebRTC).
│   ├── rustynes-cheevos/       # RetroAchievements (opt-in, native-only, rcheevos FFI).
│   ├── rustynes-frontend/      # winit + wgpu + cpal + egui (native) + wasm32 (web).
│   │                           #   Builds the `rustynes` binary.
│   └── rustynes-test-harness/  # Test ROM runner, golden-master comparator, oracle diff.
```

**Why this split.** `rustynes-cpu` / `rustynes-ppu` / `rustynes-mappers` stand alone so each can be fuzzed and benchmarked in isolation and so adding a mapper touches no chip code (it implements a trait). `rustynes-frontend` is separate from the core so the chip stack stays `#![no_std]` + `alloc` (cross-compiled in CI to `thumbv7em-none-eabihf`) and so the test harness never pulls `wgpu` into CI. `rustynes-core` re-exports the public types from the chip crates, so consumers (frontend, harness, embedders) need only depend on `rustynes-core`.

---

## Scheduling Model

> **v2.0.0 "Timebase" (ADR 0029) — read this before the two subsections
> below.** They describe the ORIGINAL v0.9.0 → v1.10.0 dot-lockstep design
> (`tick_one_dot`, five independently-incremented counters). That model is
> **historical**: it is kept for context, per this project's documentation
> convention, and is no longer what the shipping scheduler does.
>
> The current model is **one-clock, every-cycle-bus-access**.
> `Cpu::start_cycle` / `Cpu::end_cycle` advance `master_clock` by the region's
> φ1/φ2 split (NTSC: asymmetric 5/7 on a read, 7/5 on a write; PAL 16;
> Dendy 15) and pull the PPU to `master_clock − PPU_OFFSET` at **both** halves
> via `Bus::run_ppu_to`, one dot per iteration rather than a coarse batch — so
> the CPU's bus access lands BETWEEN the two PPU catch-ups. Every instruction
> cycle is a real bus access; there are no busless filler cycles. DMA is a
> unified, per-cycle-interleaved engine rather than a separate stepping mode.
> `LockstepBus::cycle` is the ONE canonical per-cycle counter, and
> `Cpu::cycles` / `Apu::cpu_cycle` are **assigned** from it, never independently
> incremented.
>
> The *consequence* that made lockstep the right choice is unchanged, which is
> why the decision below still stands: a mid-instruction PPU event is visible to
> the rest of the instruction without a per-quirk patch. Only the mechanism
> moved. See [`docs/adr/0029-one-clock-every-cycle-timebase.md`](docs/adr/0029-one-clock-every-cycle-timebase.md)
> and [`docs/scheduler.md`](docs/scheduler.md).

### Master clock = the PPU dot (historical — see the note above)

NTSC PPU: 5.369318 MHz. PAL/Dendy: 5.320342 MHz. The CPU advances 1/3 of the time on NTSC/Dendy and 1/3.2 on PAL. The APU clocks every other CPU cycle (once per 6 PPU dots NTSC). Region timing is `Region`-derived **data**, not a build-time fork.

### Tick structure (historical — see the note above)

```rust
Nes::run_frame() {
    while !self.frame_complete {
        self.tick_one_dot();
    }
}

Nes::tick_one_dot() {
    self.ppu.tick(&mut self.cart);          // always advances 1 dot
    if self.ppu.tick_count % cpu_divider == self.cpu_phase_offset {
        self.cpu.tick(&mut self.bus);       // advances 1 CPU cycle
        self.apu.maybe_tick();              // every other CPU cycle
    }
    // DMA controller honors any pending halt for this CPU cycle.
}
```

`cpu_phase_offset` accounts for the random initial CPU/PPU alignment at power-on (seeded so save states stay deterministic; cold reset re-randomizes). In the dot-lockstep model above, the bus fanned a CPU access out to the right device without re-syncing the PPU or APU, because both had already been advanced by the enclosing dot loop.

Under the shipped one-clock model that ordering is explicit rather than implicit: `start_cycle` pulls the PPU forward, the bus access happens, then `end_cycle` pulls it forward again. That is what makes a register write land at a defined dot instead of at whatever dot the loop happened to be on — and it is why `LockstepBus::cycle` has to be the single counter, since two counters advanced by two rules is exactly the bug the split is designed to make unrepresentable.

---

## Public API Surface

`rustynes-core` exposes the embedder-facing interface; the frontend, harness, and any future embedder consume it:

```rust
pub struct Nes { /* opaque */ }

impl Nes {
    pub fn from_rom(bytes: &[u8]) -> Result<Self, RomError>;
    pub fn from_rom_with_region(bytes: &[u8], region: Region) -> Result<Self, RomError>;
    pub fn from_disk(disk: &[u8], bios: &[u8]) -> Result<Self, RomError>;  // FDS
    pub fn reset(&mut self);
    pub fn power_cycle(&mut self);

    pub fn run_frame(&mut self) -> &Framebuffer;
    pub fn audio_samples(&mut self) -> &[i16];   // band-limited, drained on read
    pub fn set_buttons(&mut self, port: u8, buttons: Buttons);

    pub fn snapshot(&self) -> SaveState;
    pub fn restore(&mut self, state: &SaveState) -> Result<(), SaveStateError>;
}

pub enum Region { Ntsc, Pal, Dendy }
```

Internally `Nes::from_rom` delegates to `rustynes_mappers::parse(bytes) -> Result<(Cartridge, Box<dyn Mapper>), RomError>`. The tuple shape is deliberate: `Cartridge` metadata is cheap to clone and used by the bus, save-state path, and debugger, while the `Box<dyn Mapper>` is the live mutable state owned exclusively by the bus.

---

## Memory Architecture

CPU address space (16-bit):

```text
$0000-$07FF   2 KB internal RAM (mirrored to $1FFF)
$2000-$3FFF   PPU registers (8, mirrored)
$4000-$4017   APU + I/O registers
$4020-$5FFF   Expansion / mapper registers (open bus when unmapped)
$6000-$7FFF   Cartridge PRG-RAM (battery-backed where present)
$8000-$FFFF   Cartridge PRG-ROM (mapper-banked)
```

PPU address space (14-bit):

```text
$0000-$1FFF   Pattern tables (CHR-ROM/RAM, mapper-banked)
$2000-$2FFF   Nametables (2 KB internal VRAM, mapper-controlled mirroring)
$3F00-$3F1F   Palette RAM
```

Open-bus modeling belongs to the device that drives the bus: CPU-level open bus, the PPU `_io_db` latch, APU `$4015` behavior, controller reads, and mapper bus conflicts are related but **not interchangeable** — do not collapse them into one global byte unless a test proves equivalence.

---

## Module Boundaries and Invariants

- **CPU never touches PPU or APU directly** — always through the bus, which already has them synced.
- **PPU owns its 2 KB internal VRAM** but reads CHR through the mapper; nametable mirroring is mapper-controlled.
- **APU owns its register state and channel timers** and asks the CPU bus for DMC sample bytes via a DMA request channel.
- **Mappers see two independent buses** (CPU bus for `$4020-$FFFF`, PPU bus for `$0000-$3FFF`) plus PPU A12 transitions for IRQ counters.
- **The frame is complete** when the PPU finishes the post-render scanline; the frontend only consumes a complete framebuffer.
- **Region timing is data**, supplied as `Region`-derived constants — not hardcoded NTSC values in hot paths.

Per-tick invariants validated by the harness in debug mode: `ppu.tick_count` increments by exactly 1; `cpu.cycle_count` increments by 0 or 1; the APU clocks at most every other CPU cycle; the framebuffer is written only during dots 1–256 of scanlines 0–239.

---

## Concurrency Model

The core is single-threaded and deterministic. The frontend runs the cpal audio callback on the audio thread, reading from a lock-free SPSC ring buffer the run loop fills. On native builds a **dedicated emulation thread** owns the pacer + run-ahead and pings the winit event loop with completed frames, so UI/GPU/file-I-O stalls do not disturb emulation cadence; netplay falls back to a synchronous path under the emulation lock. This sidesteps the shared-mutability question for the core entirely.

---

## Design Patterns

### Newtype pattern for type safety

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)] struct CpuAddress(u16);
#[derive(Copy, Clone, Debug, PartialEq, Eq)] struct PpuAddress(u16);
// CPU and PPU addresses cannot be accidentally mixed.
```

### Trait objects for mappers

```rust
let mapper: Box<dyn Mapper> = rustynes_mappers::parse(rom)?.1;
```

`Box<dyn Mapper>` (no shared owners — the cart owns its mapper, the bus owns the cart) is the deliberate choice; benchmarks show per-call dispatch overhead is negligible relative to frame cost (see `docs/adr/0001-mapper-dispatch.md`).

### State machines with enums

DMC playback, sprite evaluation, the DMA controller, and the netplay/movie UIs are all modeled as explicit `enum` state machines, which keeps sub-cycle transitions auditable.

---

## Performance Considerations

- **Hot paths** (`Cpu::tick`, `Ppu::tick`, mapper register access) avoid allocation and prefer fixed arrays. A `MapperCaps` capability cache skips per-CPU-cycle virtual calls a mapper does not use; a `(emphasis, color) → RGBA` LUT drives pixel emit; fat LTO and auto-vectorization apply across the chip stack.
- **Determinism over micro-tricks** — any optimization must be byte-identical by construction, and frame output is gated by the commercial-ROM oracle.
- **Profiling** — `cargo bench` (criterion, per chip + full-frame) plus `perf record` on the rendering-heavy bench; target ≤ 2 ms/frame headless.

---

## Related documentation

- [`docs/architecture.md`](docs/architecture.md) — the canonical, always-current architecture spec.
- [`docs/scheduler.md`](docs/scheduler.md) — the lockstep scheduler in detail.
- [`docs/STATUS.md`](docs/STATUS.md) — per-suite pass counts + mapper matrix.
- [`OVERVIEW.md`](OVERVIEW.md) — project vision and design philosophy.
- [`docs/adr/`](docs/adr/) — Architecture Decision Records.

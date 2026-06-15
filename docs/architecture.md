# Architecture

**References:** `ref-docs/research-report.md` §Technical deep-dive,
§Architecture options, §Principal engineering challenges;
`ref-docs/nesdev-wiki-technical-report.md`; `docs/nesdev-hardware-emulation-checklist.md`.

## Purpose

This document fixes the high-level architectural decisions for RustyNES. Subsystem docs (`cpu-6502.md`, `ppu-2c02.md`, `apu-2a03.md`, `mappers.md`) take these as given and elaborate one chip each. The reader should leave this doc knowing the workspace shape, the scheduling model, the public boundaries, and the rejected alternatives.

The local Nesdev-derived report is the broad hardware source map; this document
only records architecture decisions that follow from it. When a behavior is
hardware-specific rather than architectural, keep the detailed rule in the
subsystem doc and add it to `docs/nesdev-hardware-emulation-checklist.md`.

## Decision: tight lockstep at PPU-dot resolution

Per §Architecture options of the research report, three scheduling strategies were viable: (A) tight lockstep with PPU as master, (B) catch-up at CPU-instruction granularity, (C) hybrid. **Option A is selected.** Rationale:

- The user's stated goal is cycle accuracy; lockstep is the only model that makes mid-instruction PPU events (sprite-zero hit, mid-scanline scroll writes, MMC3 IRQ at PPU dot 260) trivially correct rather than requiring per-quirk patches.
- Mesen2 and ares — the two leading accuracy-first emulators — both use lockstep; this puts our design on the same axis they have validated.
- The performance penalty is modest on modern hardware (per §Performance, target ≤ 2 ms per frame core work on a 2018-era laptop chip).

Rejected: catch-up sacrifices sub-cycle PPU edge cases that the project exists to model. Hybrid doubles the scheduler surface area for users who can already disable rewind / tweak audio quality if performance matters.

## Workspace shape (Cargo workspace, 7 crates)

```text
crates/
├── rustynes-core/           # Glue: NES struct, run loop, scheduler, save state, region config
├── rustynes-cpu/            # Ricoh 2A03 CPU (6502 + interrupt logic). No PPU/APU deps.
├── rustynes-ppu/            # 2C02 PPU. Depends on rustynes-mappers (for CHR/nametable bus).
├── rustynes-apu/            # 2A03 APU. Depends on rustynes-cpu only for shared DMC DMA hooks.
├── rustynes-mappers/        # Cartridge + mapper trait + implementations of top ~25 mappers.
├── rustynes-frontend/       # The rustynes binary: winit + wgpu + cpal + egui.
└── rustynes-test-harness/   # Test ROM runner, golden-master comparator, screenshot diff.
```

`rustynes-core` re-exports the public types from `rustynes-cpu`, `rustynes-ppu`, `rustynes-apu`, `rustynes-mappers` so that consumers (frontend, test harness, embedders) need only depend on `rustynes-core`.

### Why split this way

- **`rustynes-cpu` standalone** so it can be fuzzed and benchmarked in isolation, and so a future port to a non-NES 6502 host (Apple //, BBC Micro) is structurally possible.
- **`rustynes-ppu` standalone** for the same reason (e.g., a Famicom Disk System variant later, or a 2C03/2C04 arcade variant for Vs. System).
- **`rustynes-mappers` standalone** so that adding a mapper does not touch CPU, PPU, or APU code — it implements a trait.
- **`rustynes-frontend` separate from core** so the core remains `no_std`-friendly (with `alloc`) for future embedded ports, and so the test harness avoids pulling wgpu into CI.

Per the TetaNES postmortem (`ref-docs/research-report.md` §State of the art), splitting the bus from the CPU was the most consequential mistake to avoid: the bus type owns the PPU, APU, mapper, controllers, and WRAM, and the CPU borrows the bus during `tick()`.

## Scheduling model

### The master clock is the PPU dot

NTSC: 5.369318 MHz. PAL: 5.320342 MHz. Dendy: 5.320342 MHz (PAL pixel clock). The CPU advances 1/3 of the time on NTSC and Dendy, 1/3.2 on PAL. The APU clocks every other CPU cycle (i.e., once per 6 PPU dots NTSC).

### Tick structure

```rust
Nes::run_frame() {
    while !self.frame_complete {
        self.tick_one_dot();
    }
}

Nes::tick_one_dot() {
    self.ppu.tick(&mut self.cart);          // always advances 1 dot
    if self.ppu.tick_count % 3 == self.cpu_phase_offset {
        self.cpu.tick(&mut self.bus);       // advances 1 CPU cycle
        self.apu.maybe_tick();              // every other CPU cycle
    }
    if let Some(req) = self.dma_controller.tick() {
        self.cpu.honor_halt(req);
    }
}
```

`cpu_phase_offset` accounts for the random initial CPU/PPU alignment at power-on (per `ref-docs/research-report.md` §Technical deep-dive, the initial DMA get/put phase is random). It's a fixed offset per power-on; cold reset re-randomizes from a seeded RNG so save states are deterministic.

### Per-memory-access fanout

When the CPU performs a memory read or write, the bus fans the access out to the right device. PPU and APU do not need to be re-synced inside the bus call because they were already advanced in lockstep above.

```rust
Bus::read(addr) -> u8 {
    match addr {
        0x0000..=0x1FFF => self.wram[addr & 0x07FF],
        0x2000..=0x3FFF => self.ppu.cpu_read_register(addr & 7),
        0x4000..=0x4015 => self.apu.cpu_read_register(addr),
        0x4016 | 0x4017 => self.controllers.cpu_read(addr),
        0x4020..=0xFFFF => self.cart.cpu_read(addr),
        _ => self.open_bus,
    }
}
```

Open-bus modeling (per `ref-docs/research-report.md` §Technical deep-dive, PPU registers and `$4015`/`$4016`/`$4017` quirks) is handled inside the PPU and APU register methods, not in the bus.

## Public API surface

Crate `rustynes-core` exposes:

```rust
pub struct Nes { /* opaque */ }

impl Nes {
    pub fn from_rom(bytes: &[u8]) -> Result<Self, RomError>;
    pub fn from_rom_with_region(bytes: &[u8], region: Region) -> Result<Self, RomError>;
    pub fn reset(&mut self);
    pub fn power_cycle(&mut self);

    pub fn run_frame(&mut self) -> &Framebuffer;
    pub fn run_cycles(&mut self, cycles: u64);

    pub fn audio_samples(&mut self) -> &[i16];   // band-limited, drained on read
    pub fn set_controller(&mut self, port: u8, state: ControllerState);

    pub fn save_state(&self) -> SaveState;
    pub fn load_state(&mut self, state: &SaveState) -> Result<(), SaveStateError>;
}

pub enum Region { Ntsc, Pal, Dendy }
pub struct ControllerState { pub buttons: u8 }      // bit 0 A, bit 1 B, ..., bit 7 right
pub struct Framebuffer { pub pixels: [u8; 256*240*4] }  // RGBA8, sRGB
```

Frontend, test harness, and any future embedder consume this interface. The chip-specific crates expose richer types for tests and the debugger but consumers should prefer `rustynes-core`.

Internally, `Nes::from_rom` delegates to
`rustynes_mappers::parse(bytes) -> Result<(Cartridge, Box<dyn Mapper>), RomError>`.
The tuple shape (rather than a single struct with a `mapper` field) is
deliberate: the `Cartridge` metadata is cheap-to-clone and used by the
bus, save-state path, and debugger, while the `Box<dyn Mapper>` is the
live mutable state owned exclusively by the bus. Keeping them as separate
ownership roots lets the metadata be passed by `&` or cloned for
diagnostics without disturbing the mapper. See `docs/mappers.md` §Interfaces.

## Module boundaries and key invariants

- **CPU never touches PPU or APU directly.** Always through the bus, which already has them synced.
- **PPU owns its 2 KB internal VRAM** but reads CHR through the mapper. Nametable mirroring is mapper-controlled (the mapper supplies an offset table the PPU consults per-fetch).
- **APU owns its register state and channel timers**, and asks the CPU bus for DMC sample bytes via a DMA request channel.
- **Mappers see two independent buses**: a CPU bus (for `$4020-$FFFF` reads/writes including PRG-RAM and mapper registers) and a PPU bus (for `$0000-$1FFF` CHR and `$2000-$3FFF` nametable mirroring). Mappers also receive PPU A12 transitions for IRQ counters (MMC3, MMC5).
- **The frame is complete** when the PPU finishes the last visible scanline of the post-render scanline. The frontend only consumes a framebuffer when one is complete.
- **Open-bus state belongs to the device that drives the bus.** CPU-level open
  bus, the PPU `_io_db` latch, APU `$4015` behavior, controller reads, and
  mapper bus conflicts are related but not interchangeable. Do not collapse
  them into one global byte unless a test proves the behavior is equivalent.
- **Region timing is data, not a build-time fork.** NTSC, PAL, and Dendy differ
  in CPU/PPU ratios, frame length, PPU write-mask duration, and frame-counter
  cadence. Subsystems should receive `Region`-derived constants rather than
  hardcoding NTSC values in hot paths.

## Concurrency model

The core is single-threaded. Frontend runs the audio callback on cpal's audio thread, which reads from a lock-free SPSC ring buffer that the run-loop thread fills. Rendering (wgpu) and emulator stepping share the main thread; `winit`'s event loop drives `Nes::run_frame()`.

This sidesteps the entire shared-mutability question for the core. Rationale: emulator state changes too often for a coarse `Mutex` to be cheap, and finer locking offers no real win for a single-game-at-a-time workload.

## State invariants (must hold after every `tick_one_dot`)

- `ppu.tick_count` strictly increments by 1.
- `cpu.cycle_count` increments by 0 or 1 (1 only on the third-of-three PPU dots in NTSC/Dendy).
- `apu.cycle_count` increments at most every other CPU cycle.
- `cart.last_a12` reflects the most recent PPU A12 level, with the 3-falling-edges-of-M2 filter state owned by the mapper.
- `dma_controller.cycles_remaining` decrements by 1 for each CPU cycle while DMA is active.
- The frame buffer is written to only during dots 1-256 of scanlines 0-239.

These are the assertions the `rustynes-test-harness` validates per-tick in debug mode.

## Open questions

- **Inline vs. trait-object dispatch for mappers.** TetaNES uses `Rc<RefCell<dyn Mapper>>`. We will start with `Box<dyn Mapper>` (no shared owners — the cart owns its mapper and the bus owns the cart), and benchmark vs. an enum of all mappers in Phase 4. Decision deferred until we have profiling data.
- **`no_std` boundary** *(resolved)*. The chip stack — `rustynes-cpu`, `rustynes-ppu`, `rustynes-apu`, `rustynes-mappers`, `rustynes-core` — is `#![no_std]` + `extern crate alloc;`. `rustynes-frontend` stays `std` (it depends on `wgpu` / `winit` / `cpal` / `egui`, all of which require the standard library). The CI `no_std_check` job at `thumbv7em-none-eabihf` proves the chip stack compiles against `core + alloc` only:

  ```bash
  cargo build -p rustynes-core --target thumbv7em-none-eabihf --no-default-features
  ```

  `rustynes-core`'s `default = ["std"]` cargo feature propagates `std` to `lz4_flex`, `sha2`, `thiserror`, and `rustynes-apu` so desktop builds are unchanged. Turning `--no-default-features` off routes `rustynes-apu::mixer`'s `f32::exp` through `libm::expf` (filter-coefficient init only — not on the per-sample hot path; bit-identical for the inputs we use). `rustynes-apu`'s `std` feature is opt-in rather than defaulted, so cargo's workspace feature unification does not silently re-enable `std` across the workspace when `rustynes-frontend` is also built.
- **Save-state format stability.** Initial format will be tagged sections per chip with a version byte (no `bincode` derive on the whole struct — too fragile). Backwards compatibility across major versions is best-effort, not guaranteed.

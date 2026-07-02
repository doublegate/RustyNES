# Scheduler

**References:** `ref-docs/research-report.md` §Architecture options;
`ref-docs/nesdev-wiki-technical-report.md` §Emulator Architecture Guidance;
`docs/architecture.md` §Scheduling model; Nesdev
[DMA](https://www.nesdev.org/wiki/DMA) and
[APU Frame Counter](https://www.nesdev.org/wiki/APU_Frame_Counter).

## Purpose

The scheduler is the heart of the cycle-accurate emulator: it advances the PPU, CPU, APU, mapper IRQ counters, and DMA controller in tight lockstep at PPU-dot resolution. It lives in `crates/rustynes-core` and is the single owner of the `Nes` run loop.

> **v2.0.0 "Timebase" (promoted in beta.4):** the shipped scheduler is the
> **one-clock, every-cycle-bus-access** model — `Cpu::master_clock` advances
> by the region divider per CPU cycle (asymmetric read 5/7, write 7/5 φ1/φ2
> split on NTSC; PAL 16, Dendy 15) with the PPU pulled to
> `master_clock − PPU_OFFSET` at both half-cycles (`run_ppu_to` double
> catch-up); `LockstepBus::cycle` is the ONE canonical per-cycle counter
> (`Cpu::cycles` / `Apu::cpu_cycle` are assigned from it, never independently
> incremented); every instruction cycle is a real bus access (no busless
> cycles); DMA is the per-cycle interleaved unified engine; and the warm
> reset is a clocked sequence with the `$4017` re-write (see
> `docs/cpu-6502.md` + `docs/apu-2a03.md`). The `tick_one_dot` primer below
> describes the original dot-lockstep model and the DMA controller's rules,
> which the unified engine preserves semantically; the full doc re-baseline
> lands with the v2.0.0 rc (ADR 0017).

## Design

### One tick = one PPU dot

```rust
fn tick_one_dot(&mut self) {
    self.ppu.tick(&mut self.ppu_bus);
    self.dot_count += 1;
    if self.dot_count % 3 == self.cpu_phase {
        self.cpu_tick();
    }
}
```

`cpu_phase` is a per-power-on offset (0, 1, or 2) representing the random initial CPU/PPU alignment per real hardware. Reset does not change it. Cold power-cycle re-rolls it from a deterministic PRNG seeded by the user (default 0).

### CPU tick

```rust
fn cpu_tick(&mut self) {
    if self.dma.cycles_remaining > 0 {
        self.dma.tick(&mut self.bus);   // owns the bus during DMA
        return;
    }
    if let Some(req) = self.dma.scheduled_request() {
        if self.cpu.next_cycle_is_read() {
            self.dma.start(req);
            return;
        }
    }
    self.cpu.tick(&mut self.bus);
    if self.cpu_cycle.is_apu_cycle() {
        self.apu.tick(&mut self.apu_bus);
    }
}
```

### Bus design

The bus owns: PPU, APU, mapper (via cart), WRAM, controllers, open-bus latch. The CPU borrows `&mut Bus` during `tick()`. PPU and APU each get their own bus trait for the things they need:

- `PpuBus`: `ppu_read/write` (delegates to mapper); `notify_a12` (mapper IRQ).
- `ApuBus`: `dmc_read` (delegates to bus.read); `dmc_halt_request` (queues DMA); `raise_irq` (sets a flag the CPU consults via `bus.irq_level()`).

The mapper sees both buses via separate trait methods (`cpu_read/write`, `ppu_read/write`).

### DMA controller

A small inner struct that tracks:

- `cycles_remaining: u16` — non-zero means CPU is halted by DMA.
- `scheduled: Option<DmaRequest>` — pending DMA the controller is waiting to start.
- DMA type: `OamDma { src_page: u8 }`, `DmcDma { addr: u16, kind: Load | Reload }`.

Scheduling rules per `ref-docs/research-report.md` §DMA:

- DMA can only halt on a CPU read cycle.
- DMC DMA gets precedence over OAM DMA.
- OAM DMA: 1 halt + (0 or 1 alignment) + 256 read/write pairs = 513 or 514 cycles.
- DMC DMA: 1 halt + 1 dummy + (0 or 1 alignment) + 1 read = 3 or 4 cycles.

Load and reload DMC DMA are not interchangeable. Load DMA is scheduled after a
`$4015` enable write and reload DMA is scheduled when the DMC sample buffer
empties; the two start on different get/put phases before halt delay is
considered. The scheduler must preserve this distinction all the way to the bus
because repeated halted reads of `$2007`, `$4015`, `$4016`, and `$4017` are
observable side effects.

### Region cadence

NTSC and Dendy can use a simple 3 PPU dots per CPU cycle cadence. PAL needs a
fractional or master-clock representation because its PPU:CPU ratio is 3.2.
APU frame-counter tables also differ by region; do not scale NTSC cycle counts
for PAL.

### Frame complete

The PPU reports `frame_complete()` when it transitions from scanline 240 to 241 (i.e., the start of vertical blank). The frontend consumes the framebuffer at this point.

### Audio drain

The APU emits samples into a band-limited buffer continuously. The frontend's audio callback (on the cpal thread) reads from a ring buffer the run loop fills via `apu.drain_samples(&mut buf)` once per frame.

## Determinism

The scheduler is fully deterministic given:

- A fixed `cpu_phase` (chosen at power-cycle from a seedable PRNG).
- A fixed initial WRAM pattern (deterministic seeded fill).
- A fixed sequence of controller inputs (recorded by the test harness).
- A fixed initial DMA get/put phase.
- A fixed region timing profile and reset/power-up mode.

This guarantees that save/load round-trips and a re-played input sequence produce bit-identical framebuffer + audio output. Required for movie playback, regression tests, and netplay.

## Performance targets

- Frame cost (single-thread, no rendering): ≤ 2 ms on a 2018-era laptop x86_64 (Skylake-era).
- Frame cost including wgpu present + cpal callback: ≤ 5 ms (well under the 16.67 ms budget for 60 fps NTSC).
- Audio callback: lock-free SPSC ring buffer; never block the audio thread.

## Open questions

- **Inline vs trait dispatch.** The PPU `tick()` and CPU `tick()` are the hot paths. Initial implementation uses trait objects (`Box<dyn Mapper>`) for the mapper. If profiling shows mapper-dispatch overhead > 5%, switch to a monomorphized enum for the supported mappers.
- **SIMD for framebuffer scaling.** Initial scaling is GPU-side via wgpu (sampler with nearest filter). If we add per-pixel post-processing (CRT shader, scanline), it's GPU-side as well. No CPU SIMD planned.
- **Multithreading.** Not planned for v1.0. The single-frame work fits comfortably in one thread.

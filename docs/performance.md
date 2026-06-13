# Performance

**References:** `ref-docs/research-report.md` §Architecture options; `docs/scheduler.md` §Performance targets.

## Purpose

Set quantitative performance targets, identify expected hot paths, and lay out the profiling and optimization plan.

## Targets

| Metric | Target | Stretch |
|--------|--------|---------|
| Frame cost (NTSC, headless core) | ≤ 2 ms on 2018-era x86_64 (Skylake) | ≤ 1 ms |
| Frame cost (full frontend) | ≤ 5 ms | ≤ 3 ms |
| Cold-start to first frame | ≤ 100 ms | ≤ 50 ms |
| Save state size (uncompressed) | ≤ 64 KB typical | — |
| Save state save/load latency | ≤ 1 ms | ≤ 0.2 ms |
| Rewind buffer (60s @ 60 fps) | ≤ 32 MB | ≤ 16 MB |
| Audio underrun rate | 0 under normal operation | — |

NTSC frame is 16.639 ms (60.0988 Hz); PAL is 19.997 ms (50.007 Hz). Even the
conservative target leaves >65% budget for the OS + browser tab + other apps the
user is running.

## Measured baselines (engine v2.0 line — R1 master clock vs. legacy)

> **Engine-lineage note.** The benchmark version markers in this file
> (`v2.0.x`, `v2.8.0`) are anchors for the internal engine development line that
> produced RustyNES v1.0.0, not RustyNES releases of their own; RustyNES ships
> at v1.0.0 and these are the numbers for the technology it ships.

> **Engine v2.0.1:** the legacy integer-lockstep scheduler was removed; R1 is the only
> path. The A/B below was measured during the engine's v2.0 line and is kept as the historical
> rationale for that removal — the R1 numbers remain current; the legacy column
> is no longer reproducible. See [`docs/benchmarks.md`](benchmarks.md).

The headline measurement is the A/B between the two v2.0.0 scheduler
configurations. The full, reproducible record — methodology, host, derived
real-time factors, all benches — lives in
**[`docs/benchmarks.md`](benchmarks.md)**; this section is the summary.

Numbers from `cargo bench` on the development host (Intel Core i9-10850K @
3.6 GHz, CachyOS Linux, `powersave` governor, Rust 1.86, release profile
`opt-level = 3 lto = "thin" codegen-units = 1 panic = "abort"`), captured
2026-06-10. They are **hardware-specific**; replicate on your machine before
treating any delta as a regression, and trust the *deltas* (same host,
back-to-back Criterion baselines) over the absolute ms figures (~±3% host noise
on a shared desktop). The benches live under `crates/*/benches/` and are wired
via `[[bench]] harness = false`.

### Headline — `full_frame` (end-to-end `Nes::run_frame`, the whole scheduler)

| Workload | Legacy (integer-lockstep) | R1 (default, master clock) | Δ |
|---|---|---|---|
| `nes_run_frame_nestest`        | 3.62 ms (4.59× realtime, 276 fps) | 3.92 ms (4.25× realtime, 255 fps) | **+8.14%** |
| `nes_run_frame_flowing_palette`| 2.34 ms (7.12× realtime, 428 fps) | 2.49 ms (6.69× realtime, 402 fps) | **+6.32%** |

**R1 trades ~6–8% headless frame time for the move from 94.24% → 100.00%
AccuracyCoin** (the +5.76-point accuracy gain). Both configs clear the 16.639 ms
NTSC wall by 4.25–7.1× even on this 2020 desktop. The realtime/fps figures are
against the NTSC deadline; legacy is reachable via `--no-default-features`.

The +6–8% is **bus-side, not the CPU core** — in isolation the R1 CPU cycle
loop is *faster*:

| Bench | Legacy | R1 substrate (`mc-r1-substrate`) | Δ |
|---|---|---|---|
| `cpu_throughput::cpu_nop_step_x1000` | 1.08 µs | 0.70 µs | **−34.6%** |

R1's run-to-timestamp model batches `master_clock` advancement instead of a
per-cycle `on_cpu_cycle` callback. Since the core gets cheaper yet the frame
gets dearer, the full +6–8% comes from `LockstepBus::run_ppu_to` (master-clock
PPU catch-up) + `unified_dma_cycle` (per-cycle unified-DMA dispatch) — the
accuracy machinery, doing its job.

### Scheduler-invariant controls (identical across both configs)

| Bench | Crate | Measured | Notes |
|-------|-------|----------|-------|
| `ppu_throughput::ppu_tick_one_frame` | `rustynes-ppu` | ~497 µs per NTSC frame (89,342 dots) | Synthetic `PpuBus` returns 0xA5 for every read. PPU dot loop alone ≈ 33× realtime. |
| `mapper_dispatch::cpu_read` (NROM)   | `rustynes-mappers` | ~1.67 µs per 1024 reads | Real ROMs through `parse()`. |
| `mapper_dispatch::cpu_read` (MMC1)   | `rustynes-mappers` | ~1.51 µs per 1024 reads | Serial 5-write protocol overhead. |
| `mapper_dispatch::cpu_read` (MMC3)   | `rustynes-mappers` | ~1.51 µs per 1024 reads | A12 filter is on the write path; reads are cheap. |
| `mapper_dispatch::cpu_read` (MMC5)   | `rustynes-mappers` | ~2.28 µs per 1024 reads | Most expensive: ExRAM mode + multiple bank slots dispatched in the read path. |
| `mapper_dispatch::cpu_read` (M34)    | `rustynes-mappers` | ~1.57 µs per 1024 reads | BNROM/NINA-001 variant detection. |
| `mapper_dispatch::cpu_read` (FME-7)  | `rustynes-mappers` | ~2.78 µs per 1024 reads | Per-CPU-cycle IRQ counter tick. |
| `spectral::blip_square_wave_0_1s_ntsc` | `rustynes-apu`   | ~611 µs per 0.1 s NTSC audio (~179k samples) | BLEP synthesis ≈ 164× realtime; ~3.4 ns/sample. |
| `spectral::blip_silence_0_1s_ntsc`     | `rustynes-apu`   | ~593 µs | Drain-path baseline. |

The mapper-dispatch spread (~1.5 µs NROM → ~2.78 µs FME-7) is the evidence for
the D1 ADR on `Box<dyn Mapper>` vs. monomorphized `MapperEnum`: even the dearest
mapper is well under 1% of frame cost, so dynamic dispatch is not the
bottleneck (ADR-0001). Real frontend cost is additionally gated by wgpu
submission and cpal callback scheduling, both within the 5 ms full-frontend
target.

> Historical note: the prior v0.9.0/v1.6.0 baselines (≈2.06 ms nestest,
> ≈0.86 µs NROM) were captured on a different host (Ryzen 9) and predate the R1
> scheduler. The absolute drift vs. the numbers above is host/toolchain, not a
> branch regression; see `docs/benchmarks.md` §6.

## Hot paths (expected)

Based on architectural reasoning + cross-validation with TetaNES profiling notes (`ref-docs/research-report.md` §State of the art):

1. **PPU `tick()`** — called 89,342 times per NTSC frame (262 × 341). Each call updates loopy registers, performs at most one memory fetch, and conditionally emits one pixel.
2. **CPU `tick()`** — called ~29,780 times per NTSC frame. Most cycles are simple state updates; instruction-boundary cycles do interrupt polling.
3. **APU sample emission** — called per sample (44,100/s or 48,000/s). The blip_buf-style step convolution is O(kernel width).
4. **Mapper `cpu_read/write` and `ppu_read/write`** — called once per CPU/PPU memory access. Trait dispatch overhead is the concern.

## Optimization plan

### Always-on

- **Inlining**: mark hot functions `#[inline]`; let the compiler decide on `#[inline(always)]` only after profiling shows benefit.
- **No unnecessary allocation in the hot loop**: framebuffer is a fixed `[u8; 256*240*4]`; OAM is fixed-size; nothing in `tick()` paths calls `Vec::push` or `Box::new`.
- **Branch-free pixel composition** where it pays: BG vs sprite priority can be computed with masks rather than branches.
- **Cargo profile**: `[profile.release] opt-level = 3, lto = "thin", codegen-units = 1, panic = "abort"` for the frontend binary; library crates honor the workspace profile.

### Profile-guided

After Phase 2 (CPU + basic PPU working), run:

```
cargo build --release --profile bench
perf record --call-graph dwarf -- ./target/release/rustynes [headless 600 frames]
perf report --stdio | head -50
```

Top 5 hot functions get a focused optimization pass. Specifically watch:
- Mapper trait dispatch — if it appears in top 3, switch from `Box<dyn Mapper>` to a `MapperEnum` with all implemented mappers as variants.
- Cycle-counting overflow checks — `opt-level = 2` in dev keeps them; release strips them. Verify no UB introduced.

### Benchmarks (criterion) — landed in v0.9.0

- `crates/rustynes-cpu/benches/cpu_throughput.rs` — NOP `Cpu::step` x1000 latency.
- `crates/rustynes-ppu/benches/ppu_throughput.rs` — `Ppu::tick` x 89,342 dots (one NTSC frame).
- `crates/rustynes-mappers/benches/mapper_dispatch.rs` — `Box<dyn Mapper>::cpu_read` x1024 across NROM / MMC1 / MMC3 / MMC5 / M34 / FME-7.
- `crates/rustynes-core/benches/full_frame.rs` — end-to-end `Nes::run_frame` on `nestest.nes` (`nes_run_frame_nestest`) and the rendering-heavy CC0 `flowing_palette.nes` (`nes_run_frame_flowing_palette`, added v1.6.0).

See **"Measured baselines (v2.0.0)"** above for the values, or
[`docs/benchmarks.md`](benchmarks.md) for the full reproducible record.

**CI regression gate (landed v1.6.0).** `scripts/bench_regression_check.sh`
runs the `full_frame` benches and fails if headless frame production exceeds an
absolute ceiling (default 10 ms — 60% of the 16.67 ms NTSC deadline), wired as
the `bench` job in `.github/workflows/ci.yml`. The ceiling is deliberately
generous: shared CI runners vary run-to-run by tens of percent, so a tight
percentage-regression gate would flake; the absolute ceiling instead protects
the property that matters — headless production stays comfortably real-time —
and trips only on a gross (~3x) regression. For the tighter ~5% comparison, use
criterion baselines locally:

```bash
cargo bench -p rustynes-core --bench full_frame -- --save-baseline main
# ... make changes ...
cargo bench -p rustynes-core --bench full_frame -- --baseline main
```

Per the v1.6.0 gap-analysis plan §5, do **not** monomorphize `Box<dyn Mapper>`
to chase dispatch cost — the `mapper_dispatch` benches above measure it at <1%
of frame cost; a profile must contradict that first (ADR 0001).

### Performance-pass optimization landings (core micro-opts)

- **Mapper capability flags** (`Mapper::caps() -> MapperCaps`, cached on the
  bus): the per-CPU-cycle fan-out (`notify_cpu_cycle` / `mix_audio` /
  `notify_frame_event` / `irq_pending` — up to 4 virtual calls × ~30 k
  cycles/frame) is skipped on boards whose hooks are the default no-ops.
  Contract is mechanical (a flag is `false` only when the method is not
  overridden — skipping a no-op is provably byte-identical), pinned by
  `mapper::caps_tests` + the full oracle gauntlet. Measured: **−4.0%**
  (nestest) / **−10.1%** (flowing_palette) full-frame time. This addresses
  the per-cycle dispatch population ADR-0001's `cpu_read` benches never
  measured, without monomorphizing anything.

### Profile-guided optimization (PGO) — recipe

`scripts/pgo/run.sh` adapts Mesen2's `buildPGO.sh` flow to cargo-pgo:
instrumented build → headless training sweep (`pgo_trainer` runs the
committed ROM corpus + any user dumps in `tests/roms/external/PGOGames/`
uncapped with scripted Start-button input, ~3600 frames each) →
`cargo pgo optimize build -- -p rustynes-frontend`. Prereqs: `cargo install
cargo-pgo` + `rustup component add llvm-tools-preview`. Measure with the
criterion baselines before adopting; wire into release CI only if it
proves >3% and stable (`cargo pgo bolt` chains BOLT on Linux for a
possible extra ~2%).

## Things explicitly *not* in scope for v1.0

- **JIT recompilation** of CPU code. NES games are small enough that interpretation suffices; JIT complicates everything. (Higan/ares don't JIT either.)
- **GPU-side CPU emulation.** Out of scope.
- **Multi-threading.** The frame fits in 2 ms single-threaded; threading adds overhead and complexity for no win.
- **SIMD CPU emulation.** No vectorizable inner loop in the CPU; SIMD belongs in framebuffer post-processing if anywhere.

## Profiling tools

- `perf` + `perf report` (Linux) — primary profiler.
- `cargo-flamegraph` — visualization wrapper.
- `samply` — sampling profiler with Firefox Profiler UI; cross-platform.
- `tracing` + `tracing-tracy` — structured timing for the run loop, useful when chasing per-frame variance.

## Memory

- Core working set: < 256 KB (WRAM 2 KB, VRAM 2 KB, OAM 256 B, PPU shift regs ~ 100 B, framebuffer 240 KB, mapper state ~ 1 KB, save-state buffer 64 KB).
- Rewind ring buffer: 60 s × 60 fps × ~64 KB = ~225 MB worst case; with delta compression and only saving every Nth frame, target ≤ 32 MB.

## Open questions

- **Per-platform sample rates.** macOS often defaults to 44.1 kHz; Linux PipeWire often 48 kHz; Windows WASAPI varies. Plan: query cpal at startup, configure APU to emit at the platform rate. Cost: cache the sinc kernel for that rate.
- **VSync vs. fixed-rate.** wgpu present mode `Fifo` vs. `Mailbox`. Default to `Fifo` (vsync), let users override.
- **Frameskip under load.** If the run loop falls behind, drop video frames but never audio frames. Implementation deferred to Phase 5.

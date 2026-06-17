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
>
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

```bash
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

### v1.4.0 Workstream F — measure-first micro-opt pass (core)

All changes are zero-behavior / zero-synthesis: bit-identical framebuffer +
audio, AccuracyCoin 100% (139/139) held, the visual `visual_regression` golden
and the APU oracle (`apu_mixer` / `apu_test`) stayed byte-identical with no
snapshot re-baseline. Baseline captured with
`cargo bench -p rustynes-core --bench full_frame -- --save-baseline v1.4.0-pre`
on the `nestest` (near-idle menu) + `flowing_palette` (rendering-heavy,
full-BG-every-frame) inputs; the headline number is the rendering path.

- **F1 — PPU scanline-stable flag cache + hot-helper inlining**
  (`crates/rustynes-ppu/src/ppu.rs`). The `visible` / `pre_render` /
  `render_line` classifications are pure functions of `self.scanline` +
  `self.region`, yet the per-dot `tick` recomputed them (last-visible-line +
  prerender-line compares, ~7 branches) on all 89,342 dots/frame. They are now
  computed once when the scanline changes — detected via a
  `flags_cached_scanline` sentinel — and read from `cached_visible` /
  `cached_pre_render` / `cached_render_line` on every other dot. The cache is
  pure derived state (NOT part of the PPU save-state snapshot) and self-heals on
  reset / restore (the sentinel starts at `i16::MIN`, forcing a recompute on the
  first tick). The mid-scanline-mutable `$2001` rendering gates
  (`rendering` / `rendering_gate` / `bg_reload_render`) are deliberately NOT
  cached — they can change mid-scanline, so caching them would be observable.
  The six hot pixel-fetch / shift-register helpers (`fetch_nt` / `fetch_at` /
  `fetch_bg_lo` / `fetch_bg_hi` / `reload_bg_shift_regs` /
  `prefetch_shift_bg_regs`) are marked `#[inline]`. The v2.8.0 BLEP delta-ring
  loop (`crates/rustynes-apu/src/blip.rs`) was re-verified as still split into
  two contiguous SAXPY runs (auto-vectorizes; no change needed).
- **F2 — MMC5 `cpu_read` hot-path short-circuit**
  (`crates/rustynes-mappers/src/mmc5.rs`). PRG-ROM/RAM fetches at
  `$8000-$FFFF` dominate `cpu_read` (every opcode + operand fetch on an MMC5
  cart), while the register / ExRAM arms only fire on explicit `$5xxx`
  accesses. An early `if addr >= 0x8000 { return self.read_prg_window(addr); }`
  short-circuits the common case before the `$5xxx` range match —
  byte-identical to the `0x8000..=0xFFFF` arm it bypasses.

Measured `full_frame` deltas vs. `v1.4.0-pre` (criterion, two runs):
**`flowing_palette` −7.6% to −8.7%** (2.354 ms → ~2.16 ms), the rendering-heavy
path these opts target; **`nestest` within the noise threshold** (the near-idle
menu barely exercises the BG-fetch pipeline, so there is little to gain and the
result floats inside criterion's noise band). Net: the rendering path is
meaningfully faster and the idle path is neutral.

Dropped (kept out — no clear neutral win / determinism risk): the F2 BLEP
phase-row index cache (the row index genuinely depends on the per-sample phase,
so there is no cast to elide without reordering the `f64`→`f32`→`i32`
quantization, risking byte-identity); the F3 `parse()` mapper-id reorder (the
arms are already ascending 0/1/2/3/4-first and an integer `match` compiles to a
jump table regardless of source order); the F3 bus controller-strobe gate +
`mapper_caps.cpu_cycle_hook` (both already gated behind active flags); the F3
DMA get/put enum unification (a larger refactor with no clear neutral win).

### v1.5.0 "Lens" Workstream H — frontend pacing & audio-sync pass

Source data: a real high-refresh capture
(`perf-logs/perf-Super_Mario_Bros_nes-20260616-231215.csv`; 143.975 Hz display,
`auto`→`wallclock` pacing, Mailbox, run-ahead = 1, rewind on). The baseline is
reassuring — raw frame `cost_mean ≈ 8.5 ms` / `p99 ≈ 9.2 ms` / `max ≈ 10.2 ms`
vs. the 16.639 ms NTSC budget (~51%), so the v1.4.0 core perf pass holds and the
**core synthesis is not the bottleneck**. Every measured problem was in the
**frontend pacing/present/audio layer** (determinism-safe — pacing lives in the
frontend by contract): recurring 50–128 ms produce stalls (`produced_max`) with
climbing `catchup_bursts` (9→62) + `snap_forwards` (3→12) while cost stayed flat
(⇒ blocking/scheduling, not compute); audio `underruns` 3→12 with
`audio_queued_ms` oscillating 68–91 ms around the 60 ms target; and a blind
`gpu_ms`. This pass is measure-first and keeps the determinism contract
(AccuracyCoin 100% (139/139) + visual golden + APU oracle byte-identical after
the changes).

- **H1 — decoupled triple-buffer framebuffer handoff** (`present_buffer.rs`).
  The present (winit) thread formerly copied the 240 KiB framebuffer out of
  `EmuCore::present_fb` **under the emu mutex**, serializing the present against
  the dedicated emu thread's whole `produce_one_frame` (~8.5 ms) — so on a 144 Hz
  panel the present could block up to a full produce (the flat-cost /
  spiky-`produced_max` signature). The copy moved onto a triple buffer guarded by
  a small dedicated mutex held only for the brief copy: the emu thread publishes
  each produced frame while it already holds the emu lock; the common present path
  (no NTSC composite-rt index buffer, no HD-pack) takes the freshest frame without
  ever blocking on produce. Native + `emu-thread` only; the synchronous
  (`--no-default-features`) and wasm single-threaded paths keep the prior locked
  copy. Pure presentation-path change — the bytes published are exactly
  `nes.framebuffer()`; a concurrent producer/consumer unit test guards against
  torn frames.
- **H2 — pacer stall phase-break.** The hybrid sleep-then-spin wall-clock pacer
  (`block_until_native`: sleep to within `SPIN_MARGIN`, then busy-spin) and the
  `MAX_CATCHUP_FRAMES = 3` cap + snap-forward already existed; the 50–128 ms
  spikes were individual OS deschedules (the code already cites 10–40 ms
  descheduling and elevates the emu thread's priority to mitigate it), not runaway
  catch-up. So H2 keeps the interval rings honest rather than re-paving the pacer:
  when the gap since the last scheduled frame already exceeds the catch-up window,
  the produced/presented interval phase is broken **before** the gap is recorded,
  so one transient stall no longer dominates `produced_max` / reads as sustained
  judder. Perf-ring bookkeeping only — no pacing-behavior or determinism change.
- **H3 — reuse the rewind keyframe-cache allocation** (see the
  Performance-pass section's H3 entry above; `rustynes-core`, bit-identical).
- **H4 — audio DRC + buffer tuning.** Widened `MAX_DRC_DELTA` from the ±0.5%
  Near/RetroArch default to **±1%** (~17 cents, far below audibility): the narrow
  band could not drain a catch-up-burst over-fill (a 30 ms excess took ~10 s to
  drain at ±0.5%, so the servo perpetually lagged and eventually underran), and
  ±1% roughly doubles the drain rate so the queue tracks the target. Plus a
  one-time **+20 ms latency-target bump on high-refresh panels** (> 75 Hz, capped
  by the 250 ms clamp, never below the user's configured floor) for ring headroom
  against the larger bursts. The resampler stage changes audio *timing* only — the
  core's emitted samples (the determinism + audio-oracle contract) are untouched.
- **H5 — GPU pass timing on by default.** The `gpu-timing` feature (the
  `TIMESTAMP_QUERY`-bracketed encoder timer with async 3-deep readback) is now in
  the default native feature set, so the shipped Performance panel + perf log
  report a real `gpu_ms` instead of a blank `-`. Timestamp queries are a pure side
  channel (requested only when the adapter offers the feature; degrading to `None`
  otherwise), so the presented image is byte-identical with the feature on/off and
  the wasm builds (gated out) are unchanged. The panel's pacer-anomaly readout also
  surfaces the worst recent present gap (`presented.max_ms`).
- **H6 — high-refresh present-aligned cadence — DROPPED (measure-first).** A
  present-aligned-to-production cadence under Mailbox to smooth the 60-on-144 beat
  carries documented pacing-regression risk (`docs/frontend.md`: the deeper beat
  mitigation "needs on-device validation across real refresh rates") and has **no
  headless measurement path**, so it can't be validated under the measure-first
  rule in this environment. H1 already removes the present-blocking that amplified
  the beat, and the `presented_dups` / `produced_dropped` beat counters remain the
  diagnostic for whether the work is later warranted. Not implemented.
- **H7 — perf-log regression gate.** `scripts/perf/perf_capture.sh` drives a
  bounded windowed capture with perf logging auto-enabled (the new
  `RUSTYNES_PERF_LOG` env hook), and `scripts/perf/perf_log_check.py` parses the
  CSV and asserts `underruns` / `produced_max` / `catchup_bursts` / `snap_forwards`
  stay within bounds — turning them into a tracked, repeatable signal. Pacing /
  audio behavior only exists with the real winit present loop + cpal stream (no
  headless path — the same reason the v1.2.0 F1/F3 items are maintainer-manual), so
  the capture skips cleanly on a headless host (exit 0) and is run locally /
  on-display by the maintainer, mirroring the bench ceiling's deliberately
  non-flaky philosophy. The checker looks columns up by name, so it tracks
  `perf_log.rs::columns()` as it grows (the H8 parity guarantee). It parses the
  2026-06-16 baseline and correctly flags its 12 underruns / 128.9 ms
  `produced_max` / 62 `catchup_bursts`.
- **H8 — perf-log ↔ panel parity** (`perf_log.rs`). The exporter had drifted
  behind the panel: `gpu_ms` empty (H5), and `present_mode_fell_back` /
  `target_ms` / the DRC servo + run-ahead/rewind state unlogged. The CSV header +
  every data row are now built from one ordered `columns()` list, and a
  `csv_columns_cover_panel_metrics` test asserts every panel-surfaced
  `PerfView` metric has a column (+ no duplicate columns, + row field count ==
  header), so the exporter and the live panel can't silently drift again.

### Profile-guided optimization (PGO) — recipe

`scripts/pgo/run.sh` adapts Mesen2's `buildPGO.sh` flow to cargo-pgo:
instrumented build → headless training sweep (`pgo_trainer` runs the
committed ROM corpus + any user dumps in `tests/roms/external/PGOGames/`
uncapped with scripted Start-button input, ~3600 frames each) →
`cargo pgo optimize build -- -p rustynes-frontend`. Prereqs: `cargo install
cargo-pgo` + `rustup component add llvm-tools-preview`. The training corpus is
the seven committed CC0/MIT ROMs (`nestest`, `flowing_palette`, `oam_stress`,
`db_apu`, `AccuracyCoin`, the MMC1/MMC3 `holy_mapperel` boards) — see the
`COMMITTED` list in `crates/rustynes-test-harness/src/bin/pgo_trainer.rs`.

#### CI promotion gate — `.github/workflows/pgo.yml`

The recipe is gated into a **manual-/release-only** workflow (`PGO`), NOT the
per-PR pipeline: an instrument + train + optimized-rebuild cycle compiles the
workspace twice plus a multi-ROM sweep, far too slow for the PR gate (that's the
fast absolute-ceiling `bench` job in `ci.yml`). The `PGO` workflow triggers on
**`workflow_dispatch`** (Actions tab → *Run workflow*; optional `frames` and
`run_bolt` inputs) and on **push of a release tag (`v*`)**.

Its stages:

1. **Baseline** — `cargo bench -p rustynes-core --bench full_frame` saved as the
   `plain` Criterion baseline.
2. **PGO build** — runs `scripts/pgo/run.sh` (instrument → train → optimized
   rebuild of `rustynes-frontend`).
3. **PGO bench** — re-runs `full_frame` with the merged profile applied, saved
   as the `pgo` baseline, A/B'd against `plain` on the **same runner** back to
   back.
4. **Determinism oracle** — rebuilds + runs the full `--features test-roms`
   suite with the PGO codegen applied (`cargo pgo optimize test`):
   AccuracyCoin 139/139, `nestest` golden-log 0-diff, blargg/kevtris, the
   golden-framebuffer `visual_regression` suite, and the APU mixer/volume audio
   suites — all assert byte-exact framebuffer/audio/cycle hashes.
5. **Gate + upload** — computes the speedup and uploads the PGO binary as an
   artifact **only when promotable**.

**Promotion gate — both conditions (AND):**

- **Faster** — the PGO `full_frame` mean must beat plain release by **> 3.0%**
  (`PGO_MIN_SPEEDUP_PCT`). This is a *relative* A/B on one runner, so it is
  Criterion-stable above shared-runner noise — distinct from the `ci.yml` bench
  job's *absolute* 10 ms ceiling, which does not apply here.
- **Byte-identical** — the determinism oracle (stage 4) must pass with zero
  divergence. PGO changes inlining + code layout, not FP semantics (Rust emits
  no fast-math), but the gate **proves** it rather than assuming it: any
  framebuffer/audio/cycle-hash difference fails the stage and blocks promotion.

A failed gate is informational — it never blocks a release; `release.yml` ships
the plain-release binary independently.

#### BOLT (Linux post-link, optional)

A second Linux-only `bolt` job runs behind the **same > 3% + byte-identical
gate**, only after the PGO stage has already promoted (`needs.pgo.outputs.promotable
== 'true'`), and on `workflow_dispatch` only when the `run_bolt` input is true.
It is **best-effort**: it probes for `llvm-bolt` (PATH, then `apt-get install
bolt`) and skips cleanly if unavailable, so the workflow never hard-fails on a
runner image without BOLT. When present it chains `cargo pgo bolt build` →
re-train → `cargo pgo bolt optimize`, re-benches, re-runs the oracle, and uploads
the BOLT binary only if it too clears > 3% and stays byte-identical (a possible
extra ~2% on top of PGO).

#### How to trigger

```bash
# Manual (from a checkout with the gh CLI):
gh workflow run PGO.yml                     # default 3600 frames/ROM, no BOLT
gh workflow run PGO.yml -f frames=7200 -f run_bolt=true
# Or push a release tag (runs alongside release.yml):
git tag v1.2.0 && git push origin v1.2.0
```

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

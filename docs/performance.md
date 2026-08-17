# Performance

**References:** `ref-docs/research-report.md` §Architecture options; `docs/scheduler.md` §Performance targets.

## Purpose

Set quantitative performance targets, identify expected hot paths, and lay out the profiling and optimization plan.

## Targets

> **These are DESIGN-PHASE targets, written before the cycle-accurate core
> existed — they are aspirations, not gates.** The frame-cost row in particular
> was never met and is knowingly accepted: the implemented core measures
> **~3.9 ms** (`nes_run_frame_nestest_fast`) / **~2.5 ms** (`flowing_palette`)
> on a 2020 desktop (see "Measured" below and the v2.0.1 table). The gate that
> actually runs in CI is the **relative, same-runner regression check** (§CI
> gate), not this table. Do not treat ≤ 2 ms as a goal to optimize toward by
> trading away accuracy — the dominant costs are work the accuracy model
> requires (APU BLEP synthesis in `cpu_clock`; the per-dot loop in `Ppu::tick`),
> and the obvious levers were measured and **rejected** (v2.2.3 P3/P4 below).

| Metric | Target (aspirational) | Stretch |
|--------|--------|---------|
| Frame cost (NTSC, headless core) | ≤ 2 ms on 2018-era x86_64 (Skylake) — **not met; ~3.9 ms accepted** | ≤ 1 ms |
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
`opt-level = 3 lto = "fat" codegen-units = 1 panic = "abort"`), captured
2026-06-10. (This repo's `[profile.release]` has always been `lto = "fat"` —
see "fat-LTO vs thin-LTO release-profile A/B" below for the measured rationale.) They are **hardware-specific**; replicate on your machine before
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

> **v2.0.3 — 2-cycle-ALE PPU-fetch model promoted to default (ADR 0030).** Making
> each background VRAM access a genuine two-dot transaction (an ALE-drive dot + a
> multiplexed-bus splice on the read dot, replacing the whole-dot fetch) is now the
> only PPU fetch path. It costs **~10% over the R1 baseline**: `nes_run_frame_nestest`
> is now **~4.15 ms/frame (~4× realtime)** vs the ~3.77 ms R1 figure above. Accepted
> as the cost of AccuracyCoin **141/141** (both the "ALE + Read" `$0491` and "Hybrid
> Addresses" `$0492` PPU tests now pass on the shipped default). Still ~4× under the
> NTSC wall. This is the current headless-frame baseline for the default build.

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
- **Cargo profile**: `[profile.release] opt-level = 3, lto = "fat", codegen-units = 1, panic = "abort"` for the frontend binary; library crates honor the workspace profile. The `lto = "fat"` + `codegen-units = 1` choice is measured, not assumed — see "fat-LTO vs thin-LTO release-profile A/B" below for the byte-identical +8–21% win that justifies it.

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

**CI regression gates.** The `bench` job in `.github/workflows/ci.yml` runs
**two** gates, deliberately different in kind. Both are FULL-run only (merge /
release), not per-PR-push.

1. **Absolute ceiling** (`scripts/bench_regression_check.sh`, v1.6.0) — fails if
   headless frame production exceeds a wall-clock ceiling (default 10 ms — 60%
   of the 16.67 ms NTSC deadline). Deliberately generous, and never flaky: it
   protects the property that matters — headless production stays comfortably
   real-time.

2. **Relative same-runner A/B** (`scripts/bench_relative_check.sh`, v2.2.3 P6) —
   builds and benches the **base commit and HEAD back to back on the same
   runner**, in one job sharing one target dir, and fails if HEAD is more than
   `BENCH_MAX_REGRESSION_PCT` (default 10%) slower.

### v2.3.1 — where a frame actually goes (and why the symbol profile lies)

`scripts/perf/frame_breakdown.sh` profiles `frame_probe` and buckets samples by
**source file**, which follows inlined code back to the crate that wrote it.
Measured on nestest, 1500 frames at 1500 Hz, quiet host:

| subsystem | % of frame | top source files |
| --- | ---: | --- |
| PPU (`rustynes-ppu`) | **52.1%** | `ppu.rs` 51.7% |
| APU (`rustynes-apu`) | **18.7%** | `apu.rs` 8.4%, `frame_counter.rs` 2.1%, `blip.rs` 2.1% |
| CPU (`rustynes-cpu`) | 10.1% | `cpu.rs` 9.4%, `status.rs` 0.7% |
| Bus / scheduler coupling | 9.9% | `bus.rs` 9.9% |
| std inlined at emulator call sites | 6.7% | `range.rs` 1.9%, `uint_macros.rs` 1.6% |
| Mappers | 2.5% | `m000_nrom.rs` 1.4%, `mapper.rs` 0.8% |

**The symbol-level profile does not contain the APU at all.** Under
`lto = "fat"` + `codegen-units = 1` the APU is inlined wholesale into
`<LockstepBus as Bus>::cpu_clock`, so `perf report --no-children` shows
`Ppu::tick` 31%, `cpu_clock` 18%, `emit_pixel` 10% — and **zero**
`rustynes_apu::` symbols at any percent limit. Roughly **a fifth of the frame is
attributed to the wrong subsystem** by the naive view. `perf report --inline`
does not help: measured, it produces output byte-identical to the non-inline
report, because those frames are not recoverable as call frames.

This corrects the working figure used when the v2.3.x campaign was scoped
("PPU ~53%, CPU+bus ~39%"): the PPU share holds, but the CPU+bus share is really
CPU 10% + APU 19% + coupling 10%, and the CPU proper is a third of what it
appeared to be. Note this does *not* reopen §P4 — that experiment measured the
one remaining APU lever at a **≤1.9% ceiling** and its conclusion stands. The APU
being large and the APU being *reducible* are different claims; only the first is
established here.

`std inlined at emulator call sites` is real emulator work whose source path
belongs to the standard library. It is reported as its own line rather than
redistributed proportionally, which would invent precision the data does not
contain.

Source attribution needs DWARF, which `[profile.release]` does not emit, so the
script rebuilds the probe with `CARGO_PROFILE_RELEASE_DEBUG=2`. Debuginfo does
not change codegen, and the script prints the probe's own frame cost so that
assumption is checkable against a stock release build rather than asserted.

**Why gate 2 exists.** The ceiling answers "is the emulator still real-time?",
not "did this change make it worse". On the ~4 ms/frame the core actually runs
at, a change could get **2.5x slower and still pass** — the gate would sleep
through it. That is a real hole, not a hypothetical: this repo's own history has
a 10% swing (the v2.1.8 fast dot path) that the ceiling would not have noticed
in either direction.

**Why a percentage gate is sound now when v1.6.0 judged it too flaky.** That
judgement was right about *cross-run* comparison — this run's number against a
figure recorded on another machine — where hosted runners differ by tens of
percent. Gate 2 never does that. It compares two builds measured back to back on
one runner, so runner-to-runner variance is common-mode and cancels. This is the
identical technique `pgo.yml` has relied on since v1.2.0 for its >3% promotion
bar, and the measured back-to-back noise floor is **±0.7%** (§P2, where an
identical configuration benched against its own baseline reported "no change" on
all four workloads, p > 0.05). The 10% default is nonetheless far above that: a
CI runner is noisier than a quiet desktop, and this gate's job is to catch the
gross regression the ceiling misses, not to adjudicate a 2% micro-optimization.

The base commit is benched in a **throwaway git worktree**, never via
`git checkout` — the gate must not touch the working tree it runs in. It
**skips with exit 0** (rather than inventing a verdict) when no base commit is
resolvable: a shallow clone, a root commit, a brand-new branch whose
`github.event.before` is all-zeros, or a `workflow_dispatch` with no base at
all. The job checks out with `fetch-depth: 0` precisely so the normal case does
*not* skip.

#### v2.3.1 — the gate declines to conclude on a contended host

The common-mode cancellation above holds only while the two back-to-back runs
see a comparable machine. On a contended host they do not, and the delta stops
measuring the code. **v2.3.0 P1 is the worked example: profiled on a busy
machine it read +2%; re-measured quiet, the same commit was −5.13%.** The number
was not merely imprecise — it had the wrong sign.

The gate therefore reads criterion's own artifacts (`sample.json` +
`tukey.json`) for both runs and reports two figures per bench:

- **robust CV** — `1.4826 × MAD / median`. This is the **trigger**. Unlike
  stddev it is not itself dragged around by the outliers being measured, so it
  stays a usable yardstick on exactly the contended runs that matter.
- **outlier %** — criterion's own "Found N outliers among M measurements",
  recovered as a number. Reported as evidence only, deliberately **not** the
  trigger.

**Outlier % looks like the obvious signal and is a trap.** Criterion's fences
are IQR-derived, so a benchmark whose bulk is unusually *tight* flags a large
outlier fraction from tiny absolute excursions. Measured against this repo's own
saved baselines while building the gate:

| bench | outliers | robust CV |
| --- | --- | --- |
| `nes_run_frame_flowing_palette_fast` | **30.0%** | **0.19%** |
| `nes_run_frame_nestest` | 20.0% | 0.58% |
| `nes_run_frame_flowing_palette` | 6.0% | 1.18% |
| `nes_run_frame_nestest_fast` | **0.0%** | **2.79%** |

The two signals do not merely disagree, they invert: the run with the most
outliers is the quietest in the set, and the run with none is the noisiest.
Gating on outlier % would have refused a verdict on the best measurement here.

The CV threshold is derived, not picked: a gate cannot adjudicate an effect it
cannot resolve, so the host counts as contended once `3 × CV` exceeds
`BENCH_MAX_REGRESSION_PCT` — once the noise band is wide enough to swallow the
very regression being tested for. At the default 10% limit that is a **3.33%**
CV, overridable via `BENCH_MAX_NOISE_CV_PCT`.

When contended the gate emits **NO VERDICT** (exit 0, loudly) rather than a pass
or a fail. A clean delta on a noisy host is not evidence that nothing regressed,
any more than a dirty one is evidence that something did — reporting either
would be manufacturing a conclusion from data that cannot support one. The one
exception: a delta beyond **3× the measured CV** still FAILs, because contention
inflates a measurement but does not invent a 40% one. Gate 1's absolute ceiling
applies throughout, so declining never leaves a branch ungated.

For an ad-hoc local comparison, criterion baselines directly:

```bash
cargo bench -p rustynes-core --bench full_frame -- --save-baseline main
# ... make changes ...
cargo bench -p rustynes-core --bench full_frame -- --baseline main
```

or run the CI gate itself against any base:

```bash
scripts/bench_relative_check.sh HEAD~1
BENCH_MAX_REGRESSION_PCT=5 scripts/bench_relative_check.sh origin/main
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

### v2.0.1 legacy-flag-cleanup PR — measure-first `full_frame` re-check: no change adopted

Re-measured `cargo bench -p rustynes-core` (`full_frame`, the end-to-end
`Nes::run_frame` scheduler bench) after the `mc-r1-dmc-abort-probe` diagnostic
removal, to confirm the removal is neutral and to satisfy the standing
measure-first gate before any micro-opt. Development host (Intel Core i9-10850K),
release profile, criterion medians:

| Workload | v2.0.1 `full_frame` median | vs. 16.639 ms NTSC budget | vs. ≤ 2 ms core target |
|---|---|---|---|
| `nes_run_frame_nestest`         | **3.77 ms** | 4.42× realtime (~23% of budget) | above (as documented for the R1 master clock) |
| `nes_run_frame_flowing_palette` | **2.26 ms** | 7.37× realtime (~14% of budget) | ~13% over |

Both clear the hard 16.639 ms NTSC real-time deadline by 4.4–7.4× and are within
noise of the documented R1 baseline (3.92 / 2.49 ms) — the probe removal changed
nothing measurable (as expected: the flag was default-off, so the shipped build
never compiled it in). The `nestest` figure sits above the aspirational ≤ 2 ms
core stretch target, unchanged from and consistent with the R1 master-clock trade
already recorded in "Measured baselines" above (R1 buys +5.76 AccuracyCoin points
for ~6–8% frame time; ADR 0001 / ADR 0029).

**No optimization was adopted.** Per the standing contract (v1.7.0 H7, above):
adopt a micro-opt only on a **> 3% Criterion-stable + byte-identical** bar, and
the core has already had multiple measure-first passes (v1.4.0 F, v1.5.0 H,
v1.7.0 H7) that exhausted the neutral-win candidates. This PR is a flag-cleanup,
not a perf pass; the number is recorded as the honest current baseline and to
prove the removal is neutral, not to justify a speculative change that would risk
byte-identity for a marginal gain.

### v2.1.8 "Performance" (A2) — software palette-index -> RGBA blitter (decision: keep scalar-`u32` default)

A2 adds a frontend-only, reusable software blitter
(`crates/rustynes-frontend/src/gfx_blit.rs`) that reconstructs the RGBA frame
from the PPU's palette-index framebuffer (`&[u16]`, `(emphasis << 6) | colour`)
through the same 512-entry LUT the core emits with — so its output is
**byte-identical to `Ppu::framebuffer` by construction** (asserted by
`scalar_matches_core_lut_contract` against `build_rgba_lut`). The A2 brief called
for vectorizing this conversion with portable SIMD; the honest, measure-first
result is that it is **memory-bandwidth bound and does not vectorize
profitably**.

Method: the Criterion bench `benches/gfx_blit.rs` converts a full 256x240 frame
whose indices sweep the entire `0..512` LUT domain, comparing three
byte-identical paths — the naive per-pixel `[u8; 4]` `copy_from_slice`
(`copy4`, the shape `emit_pixel` uses), a tight scalar-`u32` gather+store, and
the `wide::u32x8` portable-SIMD path (scalar 8-wide gather + one 256-bit store).
Run on the same host as the fat-LTO A/B below (Intel Core i9-10850K, CachyOS,
Rust 1.96, release + fat-LTO bench profile, `--warm-up-time 1 --measurement-time
3`). Criterion medians:

| Path | median | throughput | vs `copy4` (Δ time, + = slower) |
|---|---|---|---|
| `copy4` (scalar reference) | 12.003 µs | 19.07 GiB/s | — |
| `u32` (scalar gather+store) | 12.034 µs | 19.02 GiB/s | **+0.3%** (within noise) |
| `simd` (`wide::u32x8`) | 12.225 µs | 18.72 GiB/s | **+1.8%** (measurably *slower*) |

All three land at ~12 µs / ~19 GiB/s — which is the single-thread DRAM
bandwidth ceiling of this host, the tell-tale signature of a memory-bound
kernel. The conversion is a **table gather** (`out[i] = lut[idx[i]]`), and no
stable-target portable SIMD has a hardware gather, so the load side stays scalar
and SIMD only widens the store; the store was never the bottleneck, so the
`wide` path is not just within noise but marginally slower (non-overlapping CIs,
~1.8% over the scalar reference — the extra shuffle/pack around a store that
`copy4`/`u32` already lower to a single move).

**Decision: the `blit` dispatcher stays scalar-`u32` on every target.** No path
clears the project's **> 3% Criterion-stable + byte-identical** adoption bar, so
the memory-bound hot loop keeps the simplest reference-equivalent form. The SIMD
variants (`blit_simd` via `wide` on desktop, `blit_simd_wasm` via
`core::arch::wasm32` `v128` under `+simd128` on wasm, both with the scalar
fallback) are implemented, **byte-identical** (guarded by
`simd_equals_scalar_byte_identical`, which asserts each target's SIMD path
byte-for-byte against the scalar reference over the full corpus for both the
composite and an RGB LUT), and remain directly callable — they are the requested
deliverable and a ready building block, just not the default because the
measurement did not justify displacing scalar. **Determinism unaffected:** the
core and its golden vectors are untouched — AccuracyCoin **141/141**,
`visual_regression` byte-identical (the shipped on-screen frame path stays
GPU-resident and does not route through this module). This is the frontend
counterpart to the "measure, don't assume" discipline the fat-LTO A/B below
applies to the release profile: there the measurement *cleared* the bar and the
choice was retained; here it *did not*, and the SIMD path is provided-but-not-adopted.

### v2.1.8 "Performance" (A4) — release wasm size/startup

The release wasm build now runs `wasm-opt -O4` (Binaryen's aggressive speed
pipeline, SIMD + bulk-memory preserved) instead of trunk's default `-Oz`,
selected via `data-wasm-opt="4"` in `crates/rustynes-frontend/web/index.html`.
`-O4` optimizes for runtime speed (the per-frame emulator hot loop) rather than
raw size; the wasm-opt pass still shrinks the wasm-bindgen output **12.7 MiB ->
11.1 MiB raw** (~13%), and the shippable bundle lands well inside the 5 MiB gzip
budget enforced by `scripts/wasm_size_budget.sh` + the CI `web` gate. Measured on
the real `trunk build --release` artifact:

| Asset | raw | gzip | brotli |
|---|---|---|---|
| `rustynes-frontend-*_bg.wasm` | 11.61 MB | 4.16 MB | 2.97 MB |
| `rustynes-frontend-*.js` (glue) | 168.7 KB | 25.7 KB | 21.3 KB |
| `sw.js` | 3.5 KB | 1.5 KB | 1.2 KB |
| **TOTAL** | 11.78 MB | **3.99 MiB** | 2.85 MiB |

**gzip total 3.99 MiB vs the 5.00 MiB budget — PASS, 1.01 MiB headroom.** Startup
uses streaming instantiation (trunk's loader calls `WebAssembly.instantiateStreaming`;
`sw.js` serves cached responses with the `application/wasm` content-type
preserved, so a warm PWA cache still streams). On code-splitting: the two heavy
optional features are already out of the wasm bundle by construction —
`scripting` (mlua) and `hd-pack` are `cfg(not(target_arch = "wasm32"))`-only, and
the lightweight `wasm-canvas` embed is the existing feature-flag split; single-
cdylib dynamic-`import()` splitting is not supported by the pinned trunk
toolchain (documented in `docs/frontend.md`).

### v2.1.5 — fat-LTO vs thin-LTO release-profile A/B (decision: retain fat)

`[profile.release]` ships `lto = "fat"` + `codegen-units = 1` (see the "Cargo
profile" bullet above and `Cargo.toml`). That has been the profile since the
v1.0.0 engine transplant, but the choice had never been backed by an in-repo
A/B on the current core — and the historical caption at the top of this file
even mis-stated it as `thin`. This pass measures the difference the shipped
profile actually buys, against the standing **> 3% Criterion-stable +
byte-identical** adoption bar.

Method: with `codegen-units = 1` and `panic = "abort"` held fixed, the release
profile was flipped between `lto = "fat"` (the shipped default) and
`lto = "thin"`, each rebuilt from clean and benched back-to-back on the same
host (Intel Core i9-10850K, CachyOS, `powersave` governor, Rust 1.96, bench
process pinned with `taskset -c 0-7`) via
`cargo bench -p rustynes-cpu -p rustynes-ppu -p rustynes-core`
(`--warm-up-time 1 --measurement-time 5`). Criterion medians:

| Bench | Crate | thin | fat (shipped) | fat vs thin |
|---|---|---|---|---|
| `cpu_throughput::cpu_nop_step_x1000` | `rustynes-cpu` | 217.5 ns | 216.8 ns | **+0.3%** (within noise) |
| `ppu_throughput::ppu_tick_one_frame` | `rustynes-ppu` | 725.6 µs | 574.5 µs | **+20.8%** |
| `full_frame::nes_run_frame_nestest` | `rustynes-core` | 4.667 ms | 4.277 ms | **+8.4%** |
| `full_frame::nes_run_frame_flowing_palette` | `rustynes-core` | 3.004 ms | 2.378 ms | **+20.8%** |

fat-LTO clears the > 3% bar decisively on every bench that spans a crate
boundary — the whole-scheduler `full_frame` paths (+8.4% / +20.8%) and the
PPU dot loop (+20.8%, which calls across into `rustynes-mappers` for every
CHR/nametable fetch). The single-crate `cpu_throughput` bench is the control:
its cycle loop links essentially one crate, so cross-crate LTO has nothing to
inline and the delta sits in the noise (+0.3%) — exactly the signature of a
*cross-crate-inlining* win, not a codegen-quality artifact.

**Byte-identity — verified, not assumed.** Both profiles were rebuilt in
**release** mode (so the actual LTO codegen is exercised, unlike a default
`cargo test` dev build) and run against the golden oracle:

```bash
cargo test --release -p rustynes-test-harness --features test-roms \
    --test accuracycoin --test visual_regression --test nestest --test apu_mixer
```

Both `lto = "fat"` and `lto = "thin"` pass byte-for-byte identically — AccuracyCoin
**141/141**, the `nestest` golden-log 0-diff, the golden-framebuffer
`visual_regression` suite, and the APU `apu_mixer`/volume audio suites all
green under each profile — confirming LTO level affects inlining and code
layout only, never the emulated framebuffer/audio/cycle hashes (Rust emits no
fast-math).

**Decision: retain `lto = "fat"`.** It was already the shipped default; this
A/B retroactively validates it well above the adoption bar at zero byte-identity
cost. No default-build change was made — this is the measured justification for
the existing profile, filling the gap the mis-stated caption had left. The one
tradeoff is release build time (fat-LTO + `codegen-units = 1` serializes the
final codegen: a clean `full_frame` bench rebuild ran ~55–80 s here); that is a
build-time-only cost paid once per release, never at runtime, and is acceptable
for the shipping binary.

#### Host-tuned / target-CPU release variants (opt-in, non-default)

The portable release build targets the baseline `x86-64` ISA so the shipped
binary runs everywhere. Two opt-in variants trade portability for a tuned
instruction set — both keep the emulated output byte-identical (Rust enables no
fast-math / FP contraction under `target-cpu`), but **verify with the oracle
suite anyway** when benchmarking with them, and never ship them as the portable
artifact:

- **`release-native` (host-tuned).** The `[profile.release-native]` profile
  (inherits `release`) exists so host-tuned objects stay out of the portable
  release cache; cargo profiles can't carry rustflags, so pair it with
  `target-cpu=native`:

  ```bash
  RUSTFLAGS="-C target-cpu=native" cargo build --profile release-native -p rustynes-frontend
  ```

- **`x86-64-v3` (portable-but-modern desktop).** A middle ground that stays
  portable across essentially all 2015-and-later x86-64 desktops (AVX2 + BMI2 +
  FMA) without pinning to one exact CPU:

  ```bash
  RUSTFLAGS="-C target-cpu=x86-64-v3" cargo build --release -p rustynes-frontend
  ```

  Useful for a self-built desktop binary; not wired into the release matrix
  (which ships the maximally-portable baseline `x86-64` build).

### v2.1.8 "Performance" A1 — specialized visible-scanline fast dot path

**Profile first (mandatory).** A `perf record` of a representative mixed
workload (the PGO training corpus — `nestest`, `flowing_palette`, `oam_stress`,
`db_apu`, `AccuracyCoin`, and the MMC1/MMC3 Holy Mapperel boards, self-driven
past their title screens) attributes frame self-time as:

| Function | Self-time |
|---|---|
| `rustynes_ppu::ppu::Ppu::tick` | **46.5%** |
| `LockstepBus::cpu_clock` | 22.5% |
| `Cpu::end_cycle` | 10.4% |
| `Cpu::read1` | 8.0% |
| `LockstepBus::raw_cpu_read` / `Cpu::dispatch` / mapper reads | remainder |

So the PPU per-dot FSM is the single dominant hot function — **the correct
target** (this also corrects a stale inference from the synthetic
`ppu_tick_one_frame` bench, whose no-op `PpuBus` and rendering-disabled default
under-represent the real per-dot cost). The overwhelming majority of `tick`'s
89,342 per-frame calls are visible-scanline background-render dots whose
surrounding event/bookkeeping branches (scanline-241 VBL set, pre-render clear,
sprite-tile fetch dots 260..=316, the OAMADDR-reset window, the dot-257
hori-copy, the PPUDATA state machine, the OAM-corruption commit, the odd-frame
skip) are all statically dead.

**Design.** A default-OFF runtime knob (`Nes::set_fast_dotloop`) gates a
specialized straight-line handler (`Ppu::tick_visible_render_fast`). When ON,
the `tick` dispatch tests a conservative guard — a visible scanline, dots
`1..=256`, rendering stably enabled (immediate == 1-dot-delayed == previous
dot), and no sub-dot disturbance (no `$2006` copy-V delay, no PPUMASK
write-delay, no PPUDATA state machine in flight, no armed/pending
OAM-corruption, warm classification cache) — and, when it holds, runs the
handler and returns. The handler executes the **identical** helper sequence the
general path would for such a dot (`tick_oam_corruption`,
`tick_sprite_eval_per_dot`, `tick_oam_bus`, `reload_bg_shift_regs`, the
`ale_drive_*` / `fetch_*` pair, `inc_hori_v` / `inc_vert_v`, `emit_pixel`,
`shift_bg`) with the dead branches elided, so it is **byte-identical by
construction**; any disturbance falls instantly back to the exact per-dot path.
The guard is ordered to short-circuit cheaply for non-covered dots (dot range →
rendering-enabled → cache/visible → the rare disturbance flags), so the knob
costs ~nothing when the fast path does not apply. Compiled out under
`ppu-state-trace` (whose end-of-tick hook must observe every dot).

**Why a per-dot specialization and not a whole-scanline batch.** The
Mesen2/tetanes-style approach batches an entire visible scanline in one
straight-line renderer. That is **architecturally precluded** here by the v2.0.0
"Timebase" lockstep every-cycle-bus-access scheduler: `LockstepBus::run_ppu_to`
is called twice per CPU cycle (split around the bus access) and advances the PPU
by **≤3 dots per CPU cycle**, and the CPU observes PPU side-effects
(A12→MMC3 IRQ at dot 260, the /NMI edge sampled between dots, sprite-0 hit and
VBL via `$2002`, `$2004` / `$2007` reads) at that 3-dot granularity. The PPU is
therefore **never invited to run a scanline uninterrupted** — a true batch would
require reintroducing the catch-up scheduler v2.0.0 deliberately removed and
would break the exact-dot event delivery. So A1 optimizes the per-dot *work*
(pruning dead branches on the hot dots), not the dot *cadence*.

**Byte-identity — proven, not assumed.** With the knob OFF (the shipped default)
the build is byte-identical to one without the field. With it ON, the
differential test `crates/rustynes-test-harness/tests/fast_dotloop_diff.rs`
runs a corpus (`nestest`, `flowing_palette`, `oam_stress`, `AccuracyCoin`, the
Holy Mapperel MMC1/MMC3 boards, and a mid-frame raster demo) through BOTH paths
and asserts bit-for-bit identical framebuffer + palette-index framebuffer +
audio + CPU-cycle count + full core snapshot, **every frame**. AccuracyCoin
holds **141/141**, `nestest` 0-diff, the `visual_regression` golden set and the
APU oracle all byte-identical.

**Measured — interleaved per-frame A/B (drift-robust).** The development host
(Intel Core i9-10850K, 20 logical cores) was under heavy concurrent build load
during this pass, which contaminates the cross-bench Criterion `full_frame`
comparison (later benches absorb the load spike). An interleaved harness that
alternates OFF/ON at **per-frame** granularity cancels that slow drift; measured
at low load, rock-stable across three rounds:

| Workload (rendering state) | exact (OFF) | fast (ON) | fast is faster by |
|---|---|---|---|
| `nestest` (rendering **enabled**, rendered menu) | ~4.54 ms/frame | ~3.98 ms/frame | **+12.3%** |
| `flowing_palette` (rendering **disabled** — 64-colour backdrop-override demo) | ~2.64 ms/frame | ~2.64 ms/frame | +0.3% (neutral) |

The +12.3% on rendering-enabled content clears the standing **>3% + byte-identical**
adoption bar decisively; the rendering-disabled demo never enters the fast path
(the guard bails at `rendering_enabled()`), so it is neutral. Real games render
the vast majority of the time, so the representative effect is the +12.3% figure.
Criterion `full_frame` baselines this pass (stock, same host): `nes_run_frame_nestest`
~4.26 ms, `nes_run_frame_flowing_palette` ~2.55 ms, `ppu_tick_one_frame` ~541 µs.

**Decision (v2.1.8): shipped default-OFF (opt-in).** The optimization is a pure,
byte-identical speedup, so per this file's convention it *could* be the default.
It is nonetheless kept **default-OFF** for this cut — it is the roadmap's single
highest-risk item, and shipping it off keeps the default build unchanged and
byte-identical while the differential test + oracle prove correctness and the
A/B proves the win. Recommended for promotion to default after maintainer review
and a clean-host Criterion confirmation.

**Decision (v2.2.3): PROMOTED TO DEFAULT.** Both conditions the v2.1.8 decision
named are now met, so the knob defaults to ON and the shipped build takes the
fast path.

*Clean-host Criterion confirmation* (quiet host, stock `cargo bench -p
rustynes-core --bench full_frame`, no concurrent build load — the contamination
that forced v2.1.8's interleaved harness):

| Workload (rendering state) | exact (OFF) | fast (ON) | Δ |
|---|---|---|---|
| `nes_run_frame_nestest` (rendering **enabled**) | 4.4343 ms | 3.9331 ms | **−11.3%** |
| `nes_run_frame_flowing_palette` (rendering **disabled**) | 2.6741 ms | 2.6723 ms | −0.07% (noise) |

This independently reproduces v2.1.8's interleaved +12.3% / neutral pair on a
different measurement method, and clears the standing **>3% + byte-identical**
bar. The rendering-disabled demo is unchanged because its guard bails at
`rendering_enabled()`; real games render nearly all the time, so −11.3% is the
representative figure.

*Byte-identity* was never in question and is not newly asserted here: it has been
held continuously since v2.1.8 by `fast_dotloop_diff.rs`, which runs both paths
over the corpus and compares framebuffer + palette-index framebuffer + audio +
CPU-cycle count + full core snapshot **every frame**. Promotion was re-verified
against the whole `--features test-roms` suite with the new default in place:
**2218 passed / 0 failed**, identical to the pre-promotion tally — AccuracyCoin
141/141, nestest 0-diff, `visual_regression` and the APU oracles unmoved.

*User surface.* The desktop frontend exposes it as
`[emulation] fast_dotloop` (Settings → Accuracy, labelled "performance, not
accuracy"), defaulted through `default_fast_dotloop()` rather than
`#[serde(default)]` so an existing on-disk config loads as `true` instead of
silently opting the user out — pinned by
`emulation_fast_dotloop_defaults_on_for_pre_v2_2_3_configs`. It is an escape
hatch, not a tuning knob: there is no accuracy reason to turn it off.
`rustynes-libretro` and `rustynes-mobile` inherit the win from the core default
and deliberately gain **no** new option — neither exposes any comparable knob
today (libretro's `CoreOptions` impl is empty), and adding each crate's first one
for a byte-identical escape hatch is not justified.

**Prior to this, the win was unreachable in practice:** `Nes::set_fast_dotloop`
had zero callers outside the core and its tests, so no shipped configuration of
any frontend could enable it.

### v2.3.3 F5 — run-ahead blows the frame budget at the shipped default (decision: FINDING, fix deferred)

The p99 gate F2 added was supposed to catch frontend stutter. Measuring it on
real hardware showed it catching the wrong thing — and, in the process, found a
real one it had been blind to.

**The gate was measuring the display.** Two clean 90 s captures reproduce
`produced_p99` = 34.4 ms with **zero** catch-up bursts, underruns and
snap-forwards — the same p99 as the worst archived run. `produced_mean_ms` is
16.64 ms (the NTSC budget) in all eight captures on file, so the emulator always
*paces* correctly. The p99 is the wall-clock pacer beating against vsync, which
v2.3.0's notes predicted; on a 120 Hz Wayland host `winit` cannot read the
refresh rate at all (`monitor unknown`), so the beat is a property of the
monitor. An absolute-millisecond p99 threshold therefore reports the host's
display configuration as a regression. p99 is now reported, not gated.

> **RETRACTED IN v2.3.3 — the numbers in this subsection were contaminated and
> the conclusion drawn from them was wrong.** The claim below that `cost_*` is
> "emulation work … therefore independent of the display" was false: until
> v2.3.3 the produce paths started their timer *before* `emu.lock()`, so
> `cost_*` included time blocked on the winit thread and was very much a
> function of the display. The corrected figures and the actual root cause are
> in **v2.3.3 F1**; the original text is kept below, struck through in intent,
> because deleting a wrong measurement hides that it was ever acted on.
>
> Corrected `cost_*` (work only), same host and ROM:
>
> | `run_ahead` | rewind | cost_mean | cost_p95 |
> | --- | --- | --- | --- |
> | 0 | off | 4.09 ms | 4.73 ms |
> | 0 | on | 4.39 ms | 6.22 ms |
> | 1 | off | 5.93 ms | 6.14 ms |
> | 1 | on | 6.11 ms | 6.31 ms |
>
> So run-ahead at the default costs ~6 ms of a 16.639 ms budget, not 24 ms, and
> "60 fps is not sustainable" was never true. **No core optimisation verdict is
> affected**: every adopt/reject decision in this document is adjudicated by
> criterion's `--baseline` change analysis on `rustynes-core`'s headless
> `full_frame` bench, which has no mutex and no winit thread and therefore
> could not have been contaminated by this bug. What *was* affected is this
> table and the frontend pacing gate thresholds derived from it.

**The signal believed to be real at the time was `cost_*`** — emulation work per
displayed frame, with the pacer's sleep excluded, and *believed* to be
independent of the display. Varying only `[input] run_ahead` on one host and one
ROM:

| `run_ahead` | cost_mean | cost_p95 | cost_p99 | produced_dropped (45 s) |
| --- | --- | --- | --- | --- |
| 0 | 3.91 ms | 4.51 ms | 5.83 ms | 10 |
| **1 (the shipped default)** | 8.50 ms | **24.15 ms** | 26.76 ms | **303** |
| 2 | 9.82 ms | **19.56 ms** | 26.34 ms | 201 |

At the **default**, the p95 *appeared* to be 24.15 ms against a 16.639 ms
budget, reading as "60 fps is not sustainable and ~300 frames drop in 45
seconds". Both readings were artefacts of the timing bug — see the retraction
above.

The mechanism is not new — run-ahead snapshots (~250 KB) and restores the core
once per displayed frame on top of running N+1 frames. What is new is the
measurement: this was assumed to be affordable and never checked at the default.

**Not fixed here.** The lever is snapshot slimming (a frame-boundary variant
that omits the 245,760-byte framebuffer, which the next `run_frame` regenerates),
and that is a core-format change with its own verification burden — not
something to improvise while cutting a release. It is the single highest-value
performance item currently known, and it now has a number attached.

`scripts/perf/perf_log_check.py` gates `cost_p95` at the frame budget and
`produced_dropped` at 60, both of which fail every run-ahead-enabled capture
above and pass every clean one.

### v2.3.3 F0 — sizing the frontend items before building any of them

The v2.3.3 "Grain" frontend campaign was scoped from code reading: three full
720 KiB framebuffer memcpys per displayed frame, a `perf.view()` doing five heap
allocations and three 600-element sorts every frame, and a `format!` storm under
the emulator lock the plan called "the single easiest win in the plan". After the
core campaign rejected ten of ten items, each was **measured first**.

| item | the claim | measured cost | % of the 16.639 ms frame |
| --- | --- | ---: | ---: |
| 1 — framebuffer copy chain | "the largest *absolute* waste" | 13.2 µs (3 × 4.39 µs) | **0.079%** |
| 4 — `perf.view()` for a closed panel | 5 heap `Vec`s + 3 sorts of ≤600 | 16.2 µs | **0.098%** |
| 3 — `mapper_info()` per redraw | "the single easiest win" | 1.37 µs | **0.008%** |
| | | **~31 µs** | **0.19%** |

Every claim is factually true. `mapper_info()` really does run ~25 `format!`
calls and four `Vec` allocations per displayed frame under `self.emu.lock()`,
discarding everything but `.name`. The copy chain really is three full
245,760-byte memcpys. They are simply *small*: **eliminating all three entirely
would recover under a fifth of one percent of a frame.**

**The denominator was the mistake, and it is worth stating plainly.** These items
were ranked by how wasteful they *look* in source — "three 720 KiB memcpys" reads
as enormous — without anyone dividing by the frame budget. A 245,760-byte memcpy
costs 4.4 µs; there are 16,639 µs in a frame. Modern memory bandwidth makes
whole-framebuffer copies cheap in a way that per-frame *counts* do not convey.

**What this means for the frontend.** The core needs ~3.78 ms of the 16.639 ms
budget, so the frontend runs with ~12.8 ms of slack. Mean frame time was never
the constraint. The frontend's real failure mode is **p99 / stutter**, which is a
question of *lock-hold windows and scheduling*, not throughput — and v2.3.0
already fixed the dominant instance of it by splitting the emulator lock out of
the blocking swapchain acquire and present. Shaving 31 µs of throughput off a
path with 12.8 ms of slack cannot move a stutter metric.

Items 1, 3 and 4 are therefore **not worth implementing for performance**. Item 3
remains defensible as allocation hygiene (~1,500 allocations/second discarded),
and item 2 (skipping the GPU upload when no new frame arrived) remains defensible
as not doing obviously-pointless work — but neither is a performance claim, and
neither should be described as one.

### v2.3.1 G7/G8/G9/G10 — inline hints, typed indices, capability gate, adapter hoist (decision: all REJECTED)

The last four campaign items. With G1–G6 the score is **ten measured, ten
rejected**, which is itself the release's finding — see the summary below.

**G7 (plan item 1) — `#[inline]` on `bus.rs`.** The plan called this "the highest
expected value in the plan" because `bus.rs` carries **zero** `#[inline]` hints
across 5,349 lines. True, but only **three** of its functions survive codegen as
symbols: `cpu_clock` (18.32%), `raw_cpu_read` (2.45%), `apply_genie` (0.12%). The
specifically-named `run_ppu_to`, `apu_advance_one`, and the twelve
`PpuBusAdapter` forwarders emit **no symbol at all** — fat LTO already inlines
every one, so hinting them instructs the compiler to do what it has done.

Hinting the two that genuinely are not inlined, measured separately because they
are opposite bets:

| candidate | `nestest` | control | verdict |
| --- | ---: | ---: | --- |
| `#[inline]` on both | **+0.60%** (p = 0.02) | −0.10% (p = 0.72) | **regression** |
| `#[inline]` on `raw_cpu_read` only | −0.98% (p = 0.00) | −0.76% (p = 0.01) | drift |

Hinting the large function *hurts* — `cpu_clock` contains the entire inlined APU,
and duplicating it at every call site costs more in I-cache than the call saved,
the same mechanism that made v2.2.3 P3 slower. Hinting the small one does
nothing. All non-`nestest` workloads flat throughout.

**This weakens, without disproving, G3's hypothesis** that v2.3.0 P1's −5.13%
came from its `#[inline]` rather than its code motion. P1's hint was on a small
per-dot *PPU* function, structurally unlike either function here, so the
hypothesis is untested rather than refuted — but two attempts to find an
inline-hint win on this core have now failed, and it should not be repeated as
though it were established.

**G8 (plan item 10) — `oam` / `ciram` as fixed arrays.** Both are `Box<[u8]>`
indexed with `& 0xFF` / `& 0x07FF`, so the bounds check is provably dead but the
type does not say so; `[u8; 0x100]` / `[u8; 0x800]` encode the length statically
and elide it with no `unsafe`. The swap is four lines — surrounding code coerces
arrays to slices transparently. Result: `nestest` −0.61% (p = 0.05) against a
control of **−0.78% (p = 0.01)**, everything else flat. The checks really were
removed; removing them bought nothing. Matches v2.2.3 P3 on the same shape.

**G9 (plan item 3) — capability-gate `bg_split_state`.** Ceiling probe skipped the
per-fetch mapper dispatch outright. Three workloads flat;
`flowing_palette_fast` moved +0.54% (p = 0.03) with a **control of +0.81%
(p = 0.00)** on that same workload. Ceiling zero, consistent with the 0.09% the
symbol carries in the profile.

**G10 (plan item 4) — hoist `PpuBusAdapter` out of the dot loop. Not implementable
under this campaign's constraints, and pointless if it were.** The plan reads the
per-dot construction as an oversight defeating vtable hoisting. It is forced: the
adapter holds `mapper: self.mapper.as_mut()`, and `self.sample_nmi_edge()` runs
in the same loop taking `&mut self`. Hoisting would hold a mutable borrow of
`self.mapper` across a call needing all of `self` — rejected by the borrow
checker. With **no `unsafe` in the chip stack** (the standing constraint), it
cannot be done without restructuring `sample_nmi_edge` onto disjoint fields. And
the profile says there is nothing to win: no `PpuBusAdapter` symbol survives
codegen, its three field moves already inlined into callers measured at zero.

---

#### Core-hot-path campaign summary: why ten of ten were rejected

Ten items, ten rejections, via **six distinct mechanisms** — the diversity is the
point, because it means this is not one bad assumption repeated:

| mechanism | items |
| --- | --- |
| LLVM already performs the transformation | G3 (sink dead derivations) |
| the premise is factually false | G2 (`repr(Rust)` ignores source order), G7 (already inlined) |
| the work is real but free — absorbed off the critical path | G4 (store buffer), G5 (predicted branches), G6 (recompute) |
| the elision is real but buys nothing | G8 (bounds checks) |
| the target is too small to matter | G9 (0.09%) |
| forbidden by the ownership model | G10 (borrow checker) |

The unifying finding: **the per-dot loop has no incidental overhead left to
reclaim.** Its ~3.78 ms is spent on work the accuracy model requires, and the
core is issue-limited on that work rather than on the bookkeeping the campaign
targeted. This corroborates the existing record rather than contradicting it —
`emit_pixel` bounds-check elision measured *slower* (P3), the SIMD blitter
measured *slower* (v2.1.8 A2), and the APU mixer lever capped at ≤1.9% (P4).

Two methodological results outlast the items themselves:

1. **The A/B/A order-bias control** (added in G2) fired on essentially every
   subsequent run and is the only reason G6 was not adopted on a −0.51%
   (p = 0.00) reading of a *shipped* configuration that measured +0.01%
   (p = 0.96) on re-run.
2. **Ceiling probes** — delete the work, knowingly breaking correctness, and
   measure the upper bound before building anything. G4, G5, G6 and G9 were each
   settled by one benchmark run instead of a day of engineering; G4 alone would
   have meant threading an opt-in flag through four consumers for a zero gain.

The remaining levers are structural, not micro-architectural: v2.3.3's frontend
copy chain (three full 720 KiB memcpys per displayed frame) and snapshot slimming
(~250 KB per run-ahead frame) are whole-buffer costs, not instruction-level ones.

### v2.3.1 G4/G5/G6 — three "obvious waste" items, all ceiling-zero (decision: REJECTED)

Measured by **ceiling probe**: rather than engineer each optimization and then
discover it was worthless, delete the work outright — knowingly breaking
correctness — and measure the upper bound any real implementation could reach.
Where the ceiling is zero, the engineering is moot and no correctness hazard is
ever introduced. This turned three multi-hour items into three benchmark runs.

| item | what the ceiling probe deleted | per-frame volume | ceiling |
| --- | --- | ---: | ---: |
| **G4** (plan item 8) | the `index_framebuffer` store in `emit_pixel` | 61,440 `u16` stores | **zero** |
| **G5** (plan item 6) | the whole open-bus decay loop in `on_cpu_cycle` | ~29,780 calls | **zero** |
| **G6** (plan item 2) | the ALE/read fetch-address recomputation | ~30,720 recomputes | **zero** |

In every case the shipped `_fast` workloads were flat and the apparent movement
on `nestest` was matched or exceeded by the run's own A/B/A control:

| item | candidate `nestest` | control `nestest` |
| --- | ---: | ---: |
| G4 | −0.82% (p = 0.01) | −0.88% (p = 0.01) |
| G5 | −0.49% (p = 0.06) | −0.51% (p = 0.05) |
| G6 run 1 | −0.89% (p = 0.00) | −0.16% (p = 0.37) |
| G6 run 2 | −0.96% (p = 0.00) | **−1.17% (p = 0.00)** |

G6 is the instructive one. Run 1 looked like the campaign's first genuine win —
**−0.51% at p = 0.00 on `nestest_fast`, a shipped configuration, with a clean
control on that workload**. Run 2 measured the same probe at **+0.01%
(p = 0.96)**, and its `nestest` control drifted −1.17%, larger than the
candidate's own −0.96%. Under the relaxed sub-3% adoption bar, run 1 alone would
have been adopted. The mandatory second run is what stopped it.

Note also that `nestest` is the FIRST workload criterion benches, so it absorbs
the most warm-up, and it is where drift shows up most consistently across every
run in this campaign. Treat a `nestest`-only result with particular suspicion.

**Why there is nothing to reclaim.** Three different mechanisms, one conclusion:

- **G4** — a line's profile share is not its marginal cost. `perf` attributes
  ~0.78% to that store, but it is a sequential `u16` write the store buffer
  absorbs off the critical path; deleting it frees nothing and the samples simply
  redistribute onto neighbours.
- **G5** — ~29,780 calls/frame sounds expensive but is three perfectly predicted
  compare-and-decrement steps on L1-resident data, which an out-of-order core
  hides entirely under other latency.
- **G6** — the recomputation is real, but it is not on the critical path either.

**G6 was also not adoptable at any speed**, which the ceiling result makes moot
but is worth recording. The read half re-derives the fetch address for
`observe_a12_addr`; `ale_splice` takes the read address's high bits from
`address_bus` (latched at the ALE dot) and its low bits from `octal_latch`, so
the recomputed value exists *specifically* to drive A12. On hardware only A7–A0
pass through the 74LS373, so the PPU drives the current full address during the
read cycle and A12 follows it. Caching would freeze A12 to the ALE dot, shifting
MMC3 IRQ timing whenever a `$2000`/`$2005`/`$2006` write lands between the two
dots. The plan item read two identical-looking expressions and inferred
redundancy; they are identical only in the common case and are *meant* to be able
to differ.

### v2.3.3 F1 — dropped frames traced to presentation, not emulation (decision: root cause found; snapshot slimming REJECTED on frame-time grounds)

Investigation opened by a maintainer report of residual stutter and dropped
frames on a 20-thread i9-10850K + RTX 3090 — a host that should never drop a
frame emulating a 1.79 MHz console. The working hypothesis on entry was that
the emulation core had an underlying cost problem and that **snapshot slimming**
(dropping the 245,760-byte framebuffer from the per-frame snapshot) was the
headline fix. Measurement rejected both halves of that premise.

**The instrument was wrong first.** The three `emu_thread` produce paths started
their timer *before* `emu.lock()`:

```rust
let t0 = Instant::now();
let mut guard = emu.lock();   // blocking billed to the emulator
```

so every millisecond the winit thread held the mutex was recorded as emulation
cost. That is why the `cost` tail pinned to almost exactly one display refresh
(16.81 / 16.92 / 17.04 / 17.35 / 17.49 ms across configurations) — the signature
of blocking, not of work. Fixed by starting the work clock after the acquire and
recording the blocking separately as `produce_wait` (new `wait_*` CSV columns).
With the split in place the measured wait is **0.00 ms at every percentile**:
there is no emulator-mutex contention at all, and the v2.3.0 lock-split holds.

**Emulation is comfortably inside budget.** With the metric corrected:

| run_ahead | rewind | work mean | work p95 | work p99 |
|---|---|---|---|---|
| 0 | off | 4.09 | 4.73 | 16.51 |
| 0 | on | 4.39 | 6.22 | 17.64 |
| 1 | off | 5.93 | 6.14 | 6.32 |
| 1 | on | 6.11 | 6.31 | 6.63 |

4.09 ms against the 16.639 ms NTSC budget is 24.6% — the design point
`docs/performance.md` already records as knowingly accepted. `produced_mean`
measured **16.64 ms in every capture taken** (nine of them): the producer hits
NTSC frame timing exactly and never misses.

**The drops are presentation-side.** `presented_mean` sits at 17.13-17.94 ms
against that perfect 16.64 ms produce, and the excess is the drop rate:
17.5 / 16.64 = 5.2% excess vs 135 dropped of ~2400 frames = 5.6%. Steady state
shows drops *and* duplicates simultaneously (~7/s and ~4/s), which is a phase
beat between two unsynchronized clocks, not a rate deficit. Confirmed
independent of the present path — `presented_mean` is 17.4 ms in all five of
Mailbox/mfl=1, Mailbox/mfl=2, Mailbox/mfl=3, Fifo/mfl=2, Immediate/mfl=2 — and
independent of the GPU, which sits at 0-1% utilization at P0 1905 MHz. A
`perf record` profile puts 94.3% of cycles in the emulation thread and 5.5% in
the render thread; the render thread is cheap, not starved, and a cycles profile
cannot see the blocking that actually matters here.

**Root cause.** Display-sync pacing never engages, so the producer free-runs on
a wallclock timer that is not phase-locked to the compositor's frame callbacks.
Two independent reasons, either of which alone is sufficient:

1. `resolve_pacing` requires the monitor refresh to be within
   `DISPLAY_SYNC_MAX_SKEW` (0.5%) of the console rate, because display-sync
   implements **one emulated frame per refresh** with no integer-divisor path.
   A 120 Hz or 144 Hz panel therefore *always* falls back to wallclock — i.e.
   most modern displays are excluded by construction.
2. Refresh detection goes solely through winit's `current_monitor()` →
   `refresh_rate_millihertz()`. On the reporting host (KDE Wayland) the
   compositor advertises **no `wl_output` global at all** (65 globals, none an
   output), so that returns `None` and the log records
   `monitor_refresh_hz = unknown`. The `wp_presentation` protocol — which
   reports exact presentation timestamps and refresh period — *is* advertised
   and is not used.

**Snapshot slimming: measured, and rejected as a frame-time fix.** The premise
was that the 245,760-byte framebuffer carried through the run-ahead
snapshot/restore was a major cost. Criterion says otherwise:

| op | cost |
|---|---|
| `snapshot_core_into` | 14.8 µs |
| `restore_quiet` | 122 µs |
| full `snapshot` (with thumbnail) | 36.4 µs |

Run-ahead at `run_ahead = 2` costs ~6.2 ms per displayed frame, of which
snapshot + restore is **~137 µs — 2.2%**. The remainder is the extra `run_frame`
calls, which are inherent to run-ahead and not removable. Removing the
framebuffer entirely would save roughly 110 µs/frame, or **0.66% of the frame
budget** — far below this project's standing >3%-same-runner adoption bar, and
it would not move the drop rate at all, because drops are not caused by
emulation cost. Slimming retains a real but *different* justification —
rewind-ring memory, where the framebuffer is ~94% of every per-frame snapshot —
and had to be argued on memory rather than on frame time.

**It was, and it shipped on that basis** (see F3 below): the rewind ring now
uses `snapshot_core_into_slim`, regenerating the image with a one-frame
re-render on restore. The frame-time rejection recorded here is unaffected and
stands — run-ahead still carries the full snapshot, because for *that* path the
0.66% never cleared the bar.

**Also found, not yet fixed:** `pacing_mode = "vrr"` on a display that is not
actually variable-refresh degrades to `presented_mean` 49.74 ms (~20 fps) with
1170 dropped frames in 40 s, and has no sustained-miss fallback of the kind
display-sync carries.

**Adopted from this investigation:** the `cost`/`wait` metric split, and the
pacing rework it made legible — see **v2.3.3 F2** below.

### v2.3.3 F3 — the judder is a produce-interval tail, and rewind causes it (root cause found)

F1 and F2 fixed the pacing, and the gates went green — exact console rate,
~0 dropped frames — while the picture was still visibly uneven. This entry is
why, and it starts with a correction: **"the emulator paces perfectly" was a
conclusion drawn from the mean.**

`produced_mean` is a textbook 16.64 ms in every capture ever taken here. The
distribution is not:

| MMC3 (Bad Dudes) | p50 | p95 | p99 | max |
| --- | --- | --- | --- | --- |
| PRODUCED interval | 17.12 | 28.19 | 47.05 | 52.38 |
| WORK (`cost`) | 7.25 | 17.42 | 29.52 | 32.35 |

Individual frames are produced 30-57 ms apart while the average is exactly
right. Average-correct but delivery-uneven is precisely what is perceived as
stutter, and it is invisible to every gate that watches a mean — including the
console-rate gate F2 added, which would pass this run.

**The render loop is not the cause.** v2.3.3 added `RenderPerf` (the first
instrumentation this path has ever had: `rui_*` egui build, `rgpu_*` GPU
encode+present, `rtot_*` whole redraw handler). Across NROM / MMC1 / MMC3 /
MMC5 the GPU phase is **0.10-0.13 ms** and the egui build **0.00 ms** on the
common hidden-overlay path. `rtot` p95 of ~16.4 ms is almost entirely the Fifo
swapchain acquire blocking to vblank — the pacing mechanism working, not cost.

**Rewind is the cause.** One ROM, one host, varying only two settings:

| | WORK p99 | PRODUCED p95 | PRODUCED p99 |
| --- | --- | --- | --- |
| `run_ahead` 2, rewind **on** | 34.92 | **31.17** | 49.26 |
| `run_ahead` 2, rewind **off** | 22.83 | **17.12** | 29.06 |
| `run_ahead` 0, rewind **on** | 24.15 | **28.46** | 30.39 |
| `run_ahead` 0, rewind **off** | 22.94 | **17.14** | 29.11 |

Rewind — **on by default** — roughly doubles the produce-interval p95,
independently of run-ahead. With it off the produce interval is near-perfect
(17.1 ms against a 16.64 ms target).

The mechanism is `RewindRing::push`, which runs inside the frame budget. Every
frame it XORs the whole ~250 KB snapshot against a cached keyframe,
LZ4-compresses the delta, and boxes the result; every 60 frames it additionally
compresses and copies the full snapshot. **The framebuffer is 245,760 of those
~250 KB — 94% of the work — and is the worst possible payload for the scheme**,
because it changes every frame, so the XOR does not zero out and the delta does
not compress.

**This reframes snapshot slimming without overturning F1.** F1 measured it as a
run-ahead frame-time optimisation and rejected it correctly: ~0.66% of the frame
budget, under the project's >3% bar. That verdict stands. What F1 did not
examine is the rewind ring, where the same change removes 94% of a *per-frame*
XOR + compress that demonstrably drives the tail. The lever is the same; the
justification is a different code path.

**Implemented, and shipped in this release.** Excluding the framebuffer changes
what a restored frame can display — rewind steps backwards and needs an image
for each — so it needed a design rather than a quick edit. The design chosen is
the first of the two sketched above: **regenerate the image by running one
frame after restore**.

Shipped shape:

| aspect | behaviour |
|---|---|
| writer | `Nes::snapshot_core_into_slim`, used by `rewind_capture` only |
| what is omitted | the 245,760-byte PPU framebuffer; every other field is written |
| restore | `rewind_step_back` runs one frame, which regenerates the image |
| user-facing save states | UNCHANGED — `.rns` still carries the full framebuffer |
| size | ~4 KiB of PPU section instead of ~250 KB |

**Compatibility contract.** A slim blob is self-describing: the marker is the
**high bit of the PPU snapshot version byte**, so the reader distinguishes slim
from full without out-of-band knowledge and no container version bump was
needed. A full blob restores the framebuffer as before; a slim one restores
every other field and leaves the framebuffer untouched, which is why the
one-frame re-render is part of the restore path and not optional. The rewind
ring is in-memory and per-session, so no on-disk format changed and no
cross-version compatibility question arises — the reason this could ship
without the `.rns` epoch handling a save-state change would have required.

**Method note, for the third time in this campaign:** the gate that would have
caught this does not exist. `produced_mean` passes, `produced_dropped` passes,
`cost_p95` passes. What fails is `produced_p95` against the frame period, which
nothing watches. A mean is not a cadence.

### v2.3.3 F2 — display-synchronised pacing, generalised (decision: ADOPTED)

The fix for F1's root cause. Three changes, each addressing one of the reasons
display-sync could never engage.

**1. Integer divisors.** Display-sync produced exactly one emulated frame per
refresh, so `resolve_pacing` demanded a panel within 0.5% of the console rate
and every 120/144 Hz host fell back to wall-clock *by construction*.
`refresh_probe::best_divisor` now finds the integer `N` with `refresh / N`
closest to the console rate: 120 Hz locks at `N = 2`, 240 Hz at `N = 4`, and
144 Hz / 75 Hz still correctly return `None` (no integer relationship exists, so
wall-clock with its small evenly-spread beat remains the better answer).

**2. Measured refresh.** Detection went solely through winit's
`current_monitor()`. Two failures showed up on the reporting host: the KDE
Wayland session advertised no `wl_output` global at all, *and* `resolve_pacing`
ran once at startup before the monitor was known and never revisited the
decision. `RefreshProbe` measures the cadence directly (median of 80 redraw
intervals under Fifo, with a stability quorum that refuses an unsteady
cadence), and completing a probe re-resolves the regime. On this host the probe
itself reports "not steady enough to trust" — but the re-resolve it triggers
picks up the now-available declared 119.991 Hz, which is the outcome that
matters.

> **Superseded — see v2.3.3 F4.** The `RefreshProbe` described here was removed.
> It held on `flowing_palette` and Super Mario Bros but failed on Bad Dudes,
> for a structural reason: a redraw interval measures the *application*, not the
> display, and the two diverge precisely when the measurement matters. The
> compositor's own `wp_presentation` report replaced it. Changes 1 and 3 of this
> entry, and everything downstream of them, are unaffected.

**3. The display governs phase; the wall clock governs rate.** The obvious
implementation — produce on every `N`th refresh — was implemented, measured,
and rejected. It makes console speed a function of render-loop reliability:
presents landed at 116-119 Hz rather than the panel's 119.991, and because
production was tied 1:2 to presents the console inherited the shortfall exactly
(`produced/presented` measured 1.996). Result: a console running **1.4-3.4%
slow** with audio underruns, outside the 0.5% skew band the regime promises.
The wall-clock schedule is therefore the rate authority (accumulated, never
rebased, so there is no drift) and the display only decides *when* within that
schedule a frame is produced. A missed present no longer slows the console.

Two bugs were found while measuring this, both worth recording because both
were self-inflicted and caught only by measurement:

- Consulting the schedule through the emulator mutex on every refresh (120+/s)
  introduced the first real lock contention in the frontend — `cost` p95 36 ms,
  console at 35 Hz. The schedule is now winit-thread-local and the hot path is
  lock-free (`wait` p99 measured 0.10-0.12 ms).
- Scaling the sustained-miss health check by the divisor made its threshold
  12.5 ms, which ordinary refresh jitter under `/2` exceeds every run; the
  regime downgraded itself in 4 of 4 captures. It gained a 600-present grace
  window so the startup transient (window mapping, shader compilation, the
  measured ~7 s GPU clock ramp from P8 to P0) cannot permanently downgrade a
  session — and then the check itself had to change, see below.

**The health check was measuring the wrong thing entirely.** Restoring the
console frame period as the threshold was still not right: on a *real* workload
it fired while display-sync was winning. On Super Mario Bros — a materially
heavier ROM than `flowing_palette`, 13.1 ms of work at `run_ahead = 2` against
9.2 ms — presented p95 sat at 25.3-27.2 ms against a 24.96 ms limit, so the
regime downgraded on run-to-run variance, stickily, for the whole session. What
it downgraded *to* was measurably worse:

| SMB, 45 s | display-sync | wall-clock fallback |
| --- | --- | --- |
| `run_ahead = 1` | 4-15 drops | 61-147 drops |
| `run_ahead = 2` | 1-5 drops | 35-71 drops |

Present jitter is the wrong instrument for display-sync, for exactly the reason
the p99 and `cost_p95` gates were wrong: it reports the host's compositor, not
whether the regime is working. Under display-sync every produced frame is
presented, so irregular presents cost evenness but never speed — the wall-clock
rate authority guarantees that. The check now tests the **console rate**
(`produced.mean_ms` against the frame period, 2% band as a structural safety
net rather than a tuning knob). VRR keeps the present-based test, because there
the failure is the opposite shape: the emulator produces correctly at 16.64 ms
while the display shows ~20 fps, which only the present series can see.

**Measured outcome** (`flowing_palette`, 45 s, `run_ahead = 2`, rewind on):

| | before | after |
|---|---|---|
| regime | wallclock (always) | display-sync /2 (held 6/6) |
| dropped frames | 135-254 | **1, 2, 1, 9, 4, 5** |
| audio underruns | 0-19 | **0** |
| console rate error | — | +0.03% to +0.12% |
| mutex wait p99 | — | 0.10-0.12 ms |

And on Super Mario Bros, after the health-check correction (4/4 held):

| SMB | `run_ahead = 1` | `run_ahead = 2` |
| --- | --- | --- |
| dropped / 45 s | 4, 8 | **1, 5** |
| console rate error | -0.10%, +0.05% | +0.21%, -0.02% |
| work mean | 9.5-10.3 ms | 13.5-13.7 ms |
| presented p95 | 18.4-19.5 ms | 26.7-27.2 ms |

The `run_ahead = 2` column is the point: a presented p95 of 27 ms would have
tripped the old threshold every time, while the regime was in fact delivering
the best drop count measured on that ROM.

**Also fixed:** `pacing_mode = "vrr"` had no sustained-miss fallback, so on a
display that is not actually variable-refresh it collapsed to 49.74 ms
presented (~20 fps) with 1170 dropped frames in 40 s and stayed there. It now
shares display-sync's health check and its sticky fallback.

**Gate corrections.** Two thresholds in `scripts/perf/perf_log_check.py` were
derived from the contaminated `cost_*` numbers and are now *reported* rather
than enforced: `cost_p95` scales with `run_ahead` by design (the shipped
`run_ahead = 2` measures 13.9-21.9 ms and is healthy), and `produced_dropped`
is a property of the display (1-9 per 45 s under display-sync, 35-131 under the
wall-clock fallback, same build and host). Gating either reports the user's
hardware as a regression — the same error the p99 gate made. What replaces them
is a **console-rate gate**: `|produced_mean - target_ms|` against a 0.5% band,
which is the emulator's own responsibility, independent of display and
`run_ahead`. Verified to fail the 3.3%-slow build from change 3 above (exit 1)
and pass every healthy capture.

### v2.3.3 F4 — sourcing the refresh from the compositor (decision: redraw-interval measurement REJECTED and removed; `wp_presentation` ADOPTED)

F2 shipped, and on `flowing_palette` and Super Mario Bros it held. On **Bad
Dudes** (MMC3) it did not: the session stayed on wall-clock pacing and the
judder with it. F2's second change — the empirical `RefreshProbe` — is why, and
the reason it fails is structural rather than a tuning problem.

**A redraw interval measures the application, not the display.** The probe
timed intervals between `RedrawRequested` under Fifo, on the assumption that
Fifo makes those the display's own tick. It does — *while the application is
keeping up*. Bad Dudes at `run_ahead = 2` takes ~14 ms/frame, and during the
startup window the GPU has not yet left its idle clock, so redraws arrive at
the rate the app can produce them. The probe reported **20.032 Hz on a
119.991 Hz panel**, which `best_divisor` then correctly refused, leaving the
session on wall-clock. The measurement is trustworthy exactly on the ROMs that
never needed it, and wrong on the ones that do.

Three attempts to rescue it were built and discarded: a retry schedule, a
deferred re-resolve on the declared refresh, and a re-resolve on winit's
surface-available event. None address the defect — no amount of re-sampling
fixes a signal that is measuring the wrong quantity — and the last two also
depend on `current_monitor()` eventually answering, which on this host it never
does. **The sampling half of `refresh_probe` is removed rather than left
disabled**, so nothing can feed a wrong number into pacing again.

**What replaced it.** `wp_presentation` is a *stable* Wayland protocol and was
being advertised by this compositor throughout the investigation, unused. Its
`presented` event carries a `refresh` argument: the compositor's own prediction,
in nanoseconds, of the next output refresh. That is the period stated by the
authority that owns it — not inferred from the client's frame cadence, and not
dependent on the `wl_output` global whose absence started all of this.
`crates/rustynes-frontend/src/wayland_presentation.rs` binds it against winit's
existing connection (`Backend::from_foreign_display` + `ObjectId::from_ptr`, the
documented libwayland-interop path and the only `unsafe` involved) and collects
24 reports on its own event queue, dispatched non-blocking so it can never stall
winit's loop.

The estimator was **not** the flawed half and is unchanged: the same median,
stability quorum and plausibility window now run over compositor-reported
periods instead of self-timed ones (`refresh_probe::estimate_hz_from_intervals`,
shared, with its tests). `best_divisor`, `effective_period`, the
phase/rate split and the console-rate fallback are all untouched by this
change — F4 replaces one input, nothing else.

The perf-log header gains **`refresh_source`** (`declared` | `presentation` |
`none`). Two captures with the same refresh but different sources are not the
same experiment, which the header previously could not express.

Cost is nil: one request per present, and both the request and the poll become
early-returns once an estimate settles. `poll` answers **once per session** by
construction — the regime cannot oscillate, which is what made the retry-based
attempt worse than doing nothing.

**Measured outcome** (16 captures, 45 s each, four ROMs, `run_ahead = 2`,
rewind on — every header asserted against the config):

| ROM | regime held | drops / 45 s | underruns | console rate |
| --- | --- | --- | --- | --- |
| Bad Dudes (MMC3) | display-sync /2, 40/40 rows, 4/4 runs | 1, 3, 1, 6 | 0 | −0.02 … −0.16% |
| Super Mario Bros | display-sync /2, 39/39, 4/4 | 7, 1, 1, 4 | 0 | ±0.05% |
| Bandit Kings (MMC5) | display-sync /2, 39-40/40, 4/4 | 1, 7, 7, 1 | 0 | ±0.04% |
| `flowing_palette` | display-sync /2, 40/40, 4/4 | 4, 3, 1, 3 | 0 | ±0.06% |

Against wall-clock baselines taken on the same host earlier the same day, Bad
Dudes at identical settings dropped **114, 141, 154, 117, 186** frames per 45 s
and never left wall-clock. It is now **1-6**. This is the ROM F2 could not fix,
and Bandit Kings is an MMC5 case that had never been captured at all.

**One criterion is NOT met, and the cause is not established.** The
`produced` interval p95 is **27-33 ms** — roughly twice the frame period — on
every ROM, and *higher* than the wall-clock baseline's 17.1-23.0 ms. A
`run_ahead = 0` control was run to test whether this is work cost hitting the
16.639 ms budget (`cost_p95` sits at 14-16 ms at `run_ahead = 2`), and **the
control is confounded and settles nothing**: its four captures ran last, after
~20 minutes of sustained back-to-back capture, and their own `cost_p95` spans
8.93-25.99 ms — a 3x spread `run_ahead` cannot produce. It reports `run_ahead
= 0` as *more* expensive than `run_ahead = 2`, which is impossible on the
merits, so run order is aliased with the variable. Resolving this needs the
A/B/A order-bias design from v2.3.1 G-series, not another one-directional
sweep.

Two things are worth separating from that open question:

- Under `/2`, `presented_mean` is 8.55-11.05 ms with 1435-2342 duplicate
  presents per capture. That is **correct**: a 60 Hz console on a 120 Hz panel
  must show each frame twice. It also means `presented_*` percentiles and
  `presented_dups` no longer measure the same quantity they did under a 1:1
  regime, so thresholds inherited from that regime do not transfer.
- At `run_ahead = 0` the console-rate error grew to −0.26…−0.59%, breaching the
  0.5% gate in one of four captures. At the shipped `run_ahead = 2` the worst
  of sixteen captures was −0.16%. Also confounded by the ordering above, and
  also worth re-measuring properly.

### v2.3.3 F6 — refresh-counted produce phase + run-ahead hysteresis (decision: PARTIAL; the reported symptom is NOT fixed)

Two defects found while chasing F4's open `produced` p95 question. Both are
real and both changes are kept; neither resolves the maintainer-reported
"left-to-right shudder" in Super Mario Bros, and that is stated plainly here
rather than inferred away.

**1. The produce phase was a marginal wall-clock decision.**
`display_produce_due` re-tested `now + slack >= next` on every refresh with
`slack` at half a *refresh* (4.167 ms) against an 8.334 ms grid, so ordinary
`RedrawRequested` jitter flipped the decision between adjacent refreshes.
The phase source is now a refresh **count** (`refreshes_since_produce >=
divisor`), with the wall clock retained as two guards — `too_early` (else a
fast refresh grid runs the console fast) and `overdue` (else a render loop
that drops refreshes runs it slow, which is the 1.4-3.4% error that got plain
"every Nth refresh" rejected in F2).

**2. The run-ahead throttle could not release without re-engaging.** It
engaged on cost *with* run-ahead (>85% of budget) but released on cost
*without* it (<40%) — two different quantities, with the band sitting between
them rather than spanning them. At depth 2 any ROM whose base cost lands
between ~28% and 40% of budget oscillates at the median window (~2 s), and
each toggle shifts the displayed frame by the run-ahead depth. Measured at
three toggles per 45 s on Bad Dudes. Release now predicts the re-enabled cost
(`p50 * (depth + 1) < 70%`).

**Measured, same A/B/A instrument before and after** (SMB, six captures per
build, `run_ahead` 0/2 alternating so drift differences out):

| | `produced` p95 median | range |
| --- | --- | --- |
| before, `run_ahead = 2` | 30.36 ms | 30.2-30.4 |
| after, `run_ahead = 2` | **27.21 ms** | 24.9-28.5 |
| before, `run_ahead = 0` | 27.27 ms | 24.0-36.5 |
| after, `run_ahead = 0` | 31.40 ms | 24.9-35.9 |

A ~10% improvement at the shipped depth, and **still ~1.6x the frame period**
against a ≤17 ms target. Two things this did not fix at all:

- The `run_ahead = 0` console-rate error is unchanged (+0.18/+0.48/+0.66%
  after, versus +0.22/+0.34/+0.60% before, one capture breaching the 0.5% gate
  in each build). The `too_early` guard is therefore **not** doing what it was
  added to do, and the fast-running cause is still unidentified.
- The throttle fix is unverified against the symptom: SMB logs **zero** toggles
  in all twelve captures across both builds, because it sits at ~81% of budget
  and never trips. It only fires on heavier ROMs.

**Conclusion: the shudder's cause remains unknown.** The p95 tail is real,
partially reduced, and not yet traced to a mechanism; the leading remaining
suspect is that `presented_mean` measures 8.55-11.05 ms rather than a clean
8.334 ms, i.e. redraws are not arriving on the refresh grid in the first place,
which no amount of produce-side scheduling can correct.

### v2.3.3 F7 — the display-sync occlusion watchdog was unreachable (decision: FIXED)

`about_to_wait` early-returns with `ControlFlow::Wait` whenever
`emu_thread_drives()` is true — the default `emu-thread` feature with netplay
off, i.e. essentially every shipped build — and that return sat **above** the
`ActivePacing::Display` branch. So under display-sync the watchdog never ran.

This matters because display-sync is self-driving: the only thing re-arming the
redraw is `display_sync_after_present`, on the success path of a present. If a
compositor stops delivering frame callbacks (window minimised or fully
occluded) there is no present, so nothing re-arms, and with a bare `Wait` there
was nothing scheduled to wake the loop either. Emulation and audio stop with
it. The watchdog exists precisely to keep them running and to re-kick the
redraw, and it could not fire.

Display-sync now falls through to its own branch, which sets a bounded
`WaitUntil` and re-arms. The stall path additionally had to be guarded: it was
written for the synchronous path and calls `produce_due_frames` on the winit
thread, which under `emu_thread_drives()` would advance the console *in
addition to* the emulation thread. It now produces only when the winit thread
actually owns production; otherwise it just restarts the redraw loop and lets
the emulation thread's own pacer do the work.

Found while investigating F6's residual, not by any test — no suite covers
"compositor stops sending frame callbacks".

### v2.3.3 F8 — splitting render WORK from vblank WAIT (decision: ADOPTED, instrument only)

`RenderPerf::total` spans the whole `RedrawRequested` handler **including the
blocking present**, so a 16 ms p95 reads identically whether the loop stalled
or simply waited for vblank — which under Fifo is correct behaviour. That
ambiguity produced a wrong conclusion in this campaign: the ~16 ms `rtot` p95
was reported as a stall and a fix was built for it before the distribution was
understood.

A `wait` series now brackets the GPU submission and present, logged as
`rwait_*`; render **work** is `rtot - rwait`, logged as `rwork_*`.

> **The first version of this instrument was wrong in two independent ways, and
> the conclusion drawn from it below has been corrected.** Both defects were
> found in the PR #357 review, not by any measurement of mine.
>
> 1. **The clock started in the wrong place.** `t_present` was taken before the
>    branch selection, so `rwait` spanned the framebuffer copy, the HD composite
>    and the whole phase-1 egui build (which holds the emulator lock) in
>    addition to the present. `rtot - rwait` therefore reduced to the produce
>    hook and the pumps — near zero by construction — and `rwait` read as a
>    stall whenever the egui pass was slow. The instrument reported almost the
>    opposite of what it was built to separate.
> 2. **The arithmetic was invalid.** `work p95` was computed as
>    `rtot p95 - rwait p95`. A difference of two percentiles is not the
>    percentile of the difference. This announced itself in the published
>    table: `work p95` sat *below* `work p50`, and percentiles cannot decrease.
>    That impossibility was in the document and went unremarked.
>
> The retracted table is kept here because deleting a wrong measurement hides
> that it was published and acted on:
>
> | `run_ahead` | work p50 | work p95 | ← RETRACTED, do not cite |
> | --- | --- | --- |
> | 0 | 0.36-0.87 ms | 0.05-0.08 ms |
> | 2 | 0.06-0.27 ms | 0.01-0.07 ms |

Both defects are fixed: the wait clock is restarted immediately before the GPU
call in each render branch (left uninitialised so the compiler proves every
path sets it), and work is now recorded **per sample** as its own ring, so its
percentiles are real.

Re-measured on SMB, six 30 s captures, A/B/A over `run_ahead` to control for
order and thermal drift — medians across each capture's per-second rows:

| `run_ahead` | work p50 | work p95 | **work p99** | wait p50 | wait p95 |
| --- | --- | --- | --- | --- | --- |
| 0 | 0.013-0.015 ms | 2.2-4.5 ms | **16.0-22.4 ms** | 1.29-2.03 ms | 16.32-16.33 ms |
| 2 | 0.011-0.012 ms | 0.04-0.06 ms | **13.1-13.5 ms** | 0.91-0.94 ms | 16.13-16.32 ms |

Percentiles now increase, as they must.

**The wait half of the original conclusion survives.** The distribution is
ordinary **triple-buffered Fifo**: p50 ~1 ms (presents returning immediately
from spare swapchain images) and p95 ~16 ms (one blocking a full refresh pair).
A p95 at twice the refresh period is the expected shape, not a defect.

**The work half does not survive, and it reverses a claim made in this
campaign.** "The render loop is healthy — 0.01-0.08 ms at p95" was an artefact
of both defects above. The loop is healthy *typically* — a 0.013 ms median is
genuinely negligible — but it has a **real tail**: at `run_ahead = 0` the p95
alone reaches 4.5 ms, and at p99 render work reaches **13-22 ms**, more than a
full frame period, in every one of the six captures. At 120 Hz a p99 is roughly
one redraw per second.

That tail was invisible to the broken instrument, which is precisely how the
render loop came to be eliminated as a suspect. **It is now an open lead, not a
diagnosis** — this campaign has already advanced and falsified five mechanisms
for the reported shudder, and a sixth is not being claimed on one measurement.
What can be said is narrow and factual: something in the redraw handler outside
the present occasionally takes longer than a frame, the previous instrument
could not have shown it, and it has not been characterised.

**Method note, and the sharper version of the earlier one.** The instrument did
earn its place — it produced a result in one measurement where five
theory-first attempts each cost a build-and-measure cycle. But the first
version of it was *wrong*, and it was wrong in a way that flattered the
conclusion being reached. Instrumenting before theorising is necessary and not
sufficient: the instrument itself needs a check, and the cheapest one available
here — do the percentiles increase? — was sitting in the published table the
whole time.

### v2.3.3 F9 — re-arm the redraw before the present (decision: REJECTED, reverted)

Hypothesis: render work is ~0.1 ms yet 5% of presents block ~16 ms, and a loop
doing 0.1 ms of work cannot miss a vblank on cost — only on ordering.

> **The "~0.1 ms" premise came from the broken F8 instrument** (see the
> retraction above); corrected, work is 0.013 ms at p50 but 13-22 ms at p99, so
> a loop that *sometimes* misses a vblank on cost is no longer excluded. **The
> REJECTION below still stands on its own evidence** — the change was measured
> at zero effect across the full A/B/A, which is a result about the change
> itself and does not depend on the premise that motivated it. The premise is
> flagged rather than the verdict revisited. Since
`display_sync_after_present` re-arms only once `present()` returns, the
sequence is present-blocks → returns just after vblank N → request redraw →
winit dispatch → render → present; if that dispatch lands past vblank N+1's
commit deadline the next present waits until N+2. Requesting the redraw before
the present should leave it already queued when the present returns.

Measured with the same A/B/A instrument, six captures per build:

| | `presented_mean` | `rwait` p95 | `produced` p95 |
| --- | --- | --- | --- |
| before, `run_ahead = 2` | 8.79 ms | 16.00 ms | 24.88 ms |
| after, `run_ahead = 2` | 8.71 ms | 16.08 ms | 24.85 ms |
| before, `run_ahead = 0` | 8.66 ms | 16.23 ms | 26.30 ms |
| after, `run_ahead = 0` | 8.65 ms | 16.30 ms | 25.38 ms |

The prediction was `rwait` p95 falling toward 8 ms. **Nothing moved** — every
figure is inside run-to-run variance. Reverted per the standing >3%
same-runner bar. The premise was wrong twice over: the 16 ms p95 is normal
triple buffering (F8), so there was no missed vblank to explain.

**Standing open question.** After F2, F4, F6, F7 and this rejection, the
maintainer-reported "left-to-right shudder" in Super Mario Bros is **not
explained**. `produced` p95 sits at ~25 ms against a perfect 16.64 ms mean,
with render work at 0.05 ms and no emulator-mutex contention. Five mechanisms
were proposed and falsified: run-ahead throttle oscillation (zero toggles on
SMB), an undriven redraw loop (it is driven, from the present success path),
16 ms render stalls (vblank wait), marginal produce-phase decisions (~10%, not
the cause), and redraw ordering (zero effect). The one approach that produced a
result was instrumenting first and theorising second.

### v2.3.3 F10 — two named suspects, both refuted; a ragged present cadence found instead (decision: MEASUREMENT; no fix proposed)

F8's correction left a 13-22 ms p99 render-work tail unexplained and the shudder
unattributed. Two suspects were named up front, instrumented, and tested. **Both
are refuted.** Naming them before measuring is what made the refutation cheap —
and is the discipline this campaign adopted after five theory-first attempts
each cost a build-and-measure cycle.

#### New instrumentation

| series / counter | what it answers |
| --- | --- |
| `tick_ok` / `tick_timeout` / `tick_dropped` | which arm of the display-regime `recv_timeout` drove each frame, and how many present ticks were dropped on the depth-1 channel |
| `rlock_*` | emulator-mutex blocking **on the winit thread** — the mirror of `wait_*`, which only ever covered the producer |
| `trace-<rom>-<utc>.csv` | one row per produce and per present: interval, and `since_present` at each present. Env-gated (`RUSTYNES_FRAME_TRACE=1`), default off |
| `scripts/perf/trace_shape.py` | lag-1 autocorrelation, consecutive-pair-sum cancellation, and refreshes-per-frame run lengths |
| `rcpu` | the winit thread's own `CLOCK_THREAD_CPUTIME_ID` across the `rwork` span, so `rwork - rcpu` is time spent **off-CPU** rather than computing |

`rwork` is now `rtot - rwait - rlock`, so it finally means work alone.

`trace_shape.py` discards the first **8 s** of every trace by default — window
mapping, shader compilation and the GPU's own P8→P0 clock ramp all produce
present hiccups that say nothing about steady-state pacing. That figure is a
heuristic tuned to the reporting host, not a constant: `--warmup-s N` overrides
it, and a host that settles sooner should lower it rather than discard valid
steady-state rows. Raising it past the capture length is reported as too-few-events,
not silently as an empty result.

#### Suspect A — the 25 ms tick watchdog: REFUTED

`DISPLAY_TICK_TIMEOUT` is 25 ms and the `produced` p95 was 25-36 ms, so the
watchdog was a candidate frame driver. Measured on SMB, six 45 s captures:

| `run_ahead` | `tick_ok` | `tick_timeout` | `tick_dropped` |
| --- | --- | --- | --- |
| 2 | 1851-1855 | **0, 0, 0** | 0 |
| 0 | 1856-1857 | 2-7 (0.1-0.4%) | 0-1 |

At the shipped default the watchdog **never fires**, and ticks are essentially
never dropped. The numeric coincidence between the timeout and the p95 was
exactly that — a coincidence. Recorded because it was compelling enough to act
on, and testing it cost one capture round.

#### Suspect B — winit-thread lock blocking: REFUTED at the shipped default

| `run_ahead` | `rlock` p95 | `rlock` p99 | `rlock` max |
| --- | --- | --- | --- |
| 2 | 0.000 ms | **0.000 ms** | 12.7-13.3 ms |
| 0 | 0.000 ms | 6.9-8.6 ms | 9.3-25.7 ms |

At `run_ahead = 2` the winit thread does not block: p99 is zero, with a handful
of isolated outliers. So the 13 ms `rwork` p99 at that setting is **not** lock
blocking — and it is not the egui build (`rui` p99 = 0.00) and not the GPU
(`rgpu` p99 = 0.15 ms) either. It remains unattributed.

At `run_ahead = 0` the winit thread *does* block, p99 6.9-8.6 ms. That is a real
finding and the reverse of the intuition (the cheaper produce blocks the
consumer more), but it is not the shipped default and not the reported symptom.

#### What the trace actually shows

Six 45 s SMB traces, post-warmup:

- **`produce` intervals are NOT alternating.** Lag-1 autocorrelation is −0.07 to
  −0.12 (independent) and consecutive pairs do not cancel (pair/single stdev
  ≈ 1.34). The produce tail is **isolated excursions**, not the alternating
  cadence a shudder implies. This rules out a whole family of explanations.
- **`present` intervals are strongly alternating** (lag-1 −0.60 to −0.68) — but
  that is *expected* at divisor 2, where presents alternate between carrying a
  new frame and repeating one. On its own it means nothing.
- **The refreshes-per-frame cadence is ragged.** At divisor 2 the healthy
  `since_present` sequence is a clean `0,1,0,1,…`, i.e. every run has length 1.
  Measured:

| capture | ragged runs (len ≥ 2) | total runs | % | longest run |
| --- | --- | --- | --- | --- |
| 1 | 193 | 3510 | 5.5% | 13 |
| 2 | 124 | 4011 | 3.1% | 5 |
| 3 | 135 | 3653 | 3.7% | 15 |
| 4 | 214 | 3849 | 5.6% | 5 |
| 5 | 169 | 3617 | 4.7% | 15 |
| 6 | 151 | 3932 | 3.8% | 6 |

**3.1-5.6% of runs are ragged, in every capture, with individual runs up to 15.**
A run of 15 means fifteen consecutive presents where the alternation broke — a
frame held across many refreshes, or the producer briefly tracking the present
rate. Roughly four such events per second.

#### The raggedness is measured in the WRONG UNIT — corrected

Pushed further, the above does **not** establish uneven delivery, and the
correction matters more than the original observation.

Per-frame **hold times** (how many presents each produced frame occupied) look
alarming — aggregated over the six captures, `{1: 1634, 2: 11087, 3: 190,
4: 3}`, i.e. **11.5% of frames occupy a single present instead of two**, with
zero produced frames ever dropped. But the mean hold is 1.83-1.95, and a correct
cadence on this panel requires exactly `16.6393 / 8.3340 = 1.9966`. The
arithmetic does not close, which is what forced a look at the present intervals
themselves:

| present interval | share |
| --- | --- |
| < 1 ms | **31.8%** |
| 1-2 ms | 9.2% |
| 2-7.5 ms | 7.6% |
| **7.5-9.5 ms (≈ one refresh)** | **1.9%** |
| 9.5-12 ms | 4.8% |
| **12-16.7 ms** | **38.9%** |
| > 16.7 ms | 5.8% |

Presents are **not quantised to the refresh grid at all**: barely 2% land near
one refresh period, while a third arrive under a millisecond apart and another
third after roughly two. Verified as the Fifo steady state, not a startup
artefact — `Mailbox` appears only in row 0 of every capture and `Fifo` from 1 s
onward, well before the 8 s warmup the analysis discards.

**That bimodal shape is the expected signature of triple-buffered Fifo, and F8
already documented it**: two presents return immediately from spare swapchain
images, the third blocks a full refresh pair. Which means `record_presented`
timestamps the moment an image is **queued**, not the moment it is **scanned
out**. Under this present mode those are different clocks, and submission
bursts while scanout stays regular.

So the hold-time metric counts **queue slots, not refreshes on screen**. It
cannot answer what the eye sees, and the "3.1-5.6% ragged runs" figure above is
a statement about submission timing, not about display cadence. It is retained
rather than deleted because it was published in this document and acted on.

#### What is actually established

- Suspect A (the 25 ms watchdog) is **refuted**: 0 fires in ~1855 ticks at the
  shipped default.
- Suspect B (winit-thread lock blocking) read as **refuted at the shipped
  default** on `rlock` p99 = 0.000 ms — but **that series was incomplete when
  the figure was taken**, and the refutation should be treated as provisional
  until re-measured. The accumulator started just before the render branches,
  while `display_sync_produce`, `pump_scripts` and `pump_watchpoints` all run
  earlier in the same measured window and each acquire the mutex: four untimed
  sites. Fixed (the accumulator now starts at the top of the window and is
  threaded through all three), found in the PR #357 review body rather than by
  any measurement here. A p99 of zero over part of a window does not establish
  a p99 of zero over the whole of it.
- The `produce` interval tail is **isolated excursions, not alternation**
  (lag-1 −0.07..−0.12) — this one is a genuine result about the producer and
  does not depend on the present clock.
- No produced frame is ever dropped (`drops = 0` in all six captures).
- The 13 ms `rwork` p99 at `run_ahead = 2` is neither lock, nor UI, nor GPU, and
  remains unattributed.

The shudder is **still unexplained**, and this round did not get closer to it —
it removed two candidates and disqualified a third line of evidence as
mis-measured.

#### The next instrument is already 90% built

Answering "what did the display actually show" needs real scanout timestamps.
`wp_presentation`'s `presented` event carries them — `tv_sec_hi`, `tv_sec_lo`,
`tv_nsec` — and `wayland_presentation.rs` already binds the protocol, receives
that event, and **destructures only `refresh`, discarding the timestamps**.
Recording them would give the true per-frame scanout cadence directly from the
compositor, in the one unit that answers the question. That is the obvious next
step and it is a small one.

### v2.3.3 F11 — the display misses 4.6% of refreshes (decision: MEASURED, in the right unit at last)

F10's raggedness was measured in queue-submission time and disqualified. This
records the same question asked of the compositor.

`wp_presentation`'s `presented` event now yields its full payload — the
`tv_sec_hi` / `tv_sec_lo` / `tv_nsec` scanout instant, the compositor's own
refresh estimate, the presentation sequence counter, and the flags — buffered
in `wayland_presentation.rs` and written to the trace as `scanout` rows. Two
structural changes were needed and are worth noting, because either omission
would have produced a feature that silently recorded nothing:

- `request_feedback` stopped issuing once the refresh estimate settled, and
  `poll` early-returned on the same flag, so **the event queue was never
  dispatched again**. Settling now terminates the *estimate*, not the event
  pump.
- Feedback is requested for every present only while tracing; the shipped path
  keeps the original stop-after-settling behaviour.

**Measured, SMB, one 25 s capture (16.1 s post-warmup, 1847 scanouts):**

| scanout interval | share |
| --- | --- |
| **1 refresh (8.334 ms)** | **96.80%** |
| 2 refreshes | 1.90% |
| 3 refreshes | 1.03% |
| 4-5 refreshes | 0.27% |

`flags = 7` on every report — `VSYNC | HW_CLOCK | HW_COMPLETION` — so these are
hardware-timed completions, not compositor estimates. (`seq` reads 0: this
compositor supplies no presentation counter, so missed refreshes are inferred
from the timestamps rather than stated.)

**The result: 89 missed refreshes out of ~1935, or 4.60%** — the display
repeated the previous image because no new one arrived in time. That is **59
cadence breaks in 16.1 s, 3.66 per second.**

Two things follow. First, the render loop delivers on 96.8% of refreshes, so
this is not a broken pacer — it is a tail. Second, at divisor 2 a missed refresh
does not merely repeat a frame, it *shifts the phase*: the frame on screen is
held for a third refresh and the following one is shown for one. A ~3.7 Hz train
of those is a plausible read of "content stepping forward and back".

**This does not close the investigation.** The scanout series records *when* the
display updated, not *what changed on it* — correlating scanouts against
produced frames is the next step and has not been done. What is now established,
in the compositor's own clock, is that the display misses 4.6% of its refreshes
under display-sync, at a rate that independently agrees with the ~4/second
figure F10 arrived at through the wrong unit. F10's number was right by
accident; this one is right by measurement.

### v2.3.3 F12 — the mechanism, found: winit-thread lock contention (decision: ROOT CAUSE IDENTIFIED; fix deferred to its own change)

F11 established that the display misses 4.6% of refreshes but could not say what
changed on screen. Two measurements close that, and the first of them overturns
an earlier conclusion in this document.

#### `rlock` was incomplete, and completing it moved the whole 13 ms tail

F10 reported `rlock` p99 = 0.000 ms and concluded the winit thread does not
block. That series covered only part of the redraw window: `display_sync_produce`,
`pump_scripts` and `pump_watchpoints` all run inside it and each acquire the
emulator mutex, untimed. With all four sites instrumented, same host, same ROM:

| series | F10 (incomplete) | complete |
| --- | --- | --- |
| `rlock` p95 | 0.000 ms | **8.707 ms** |
| `rlock` p99 | 0.000 ms | **9.008 ms** |
| `rlock` max | — | 33.194 ms |
| `rwork` p99 | **13.0 ms** | **0.109 ms** |

The unattributed 13 ms render-work tail **was lock waiting all along**, and it
moved in full once the measurement covered the window. `rwork` p99 is now 0.109
ms: the render loop genuinely does almost no work. **Suspect B is revived and
confirmed** — it was refuted on a measurement that could not see it.

Note the magnitude: `rlock` p95 of 8.707 ms against a refresh period of 8.334 ms.
The winit thread spends more than a full refresh blocked, at the 95th percentile.

#### What the display actually showed — RETRACTED, see F14

> **This subsection was wrong and its numbers must not be quoted.** The metric
> below counts refreshes between consecutive *produce* timestamps, so it is
> dominated by producer-side jitter and is **not** a measure of display
> duration. On the same captures the correct display-side metric reads **5.41%**
> of frames shown for the wrong duration where this one reads **32.96%** — a
> factor of six. The retraction, the correct numbers, and how the error was
> made are in **F14** below. The table is kept for the record.

With the trace's produce/present rows and its compositor `scanout` rows joined
through the new `anchor_mono_ns` header, this asked **how many scanouts did each
produced frame get?** At divisor 2 the answer should be exactly 2, every time.
Measured over 1817 produced frames and 3480 scanouts:

| scanouts per produced frame | share |
| --- | --- |
| 0 | 3.19% |
| 1 | 18.28% |
| 2 | 65.31% |
| 3 | 10.57% |
| 4-5 | 2.65% |

It was read at the time as "34.69% of produced frames are displayed for the
wrong length of time". It is not that. It is: 34.69% of produce *intervals*
spanned a number of refreshes other than two — a statement about when the
emulator thread ran, not about what the panel showed.

#### The chain

1. The winit thread blocks on the emulator mutex — p95 8.7 ms, more than one
   refresh period.
2. The redraw and its present therefore land late.
3. The frame misses its refresh slot: some frames get one scanout, some three,
   some none.
4. The display shows 34.7% of frames for the wrong duration and repeats 4.6% of
   refreshes.

The producer holds the mutex for the whole produce — ~9.7 ms per frame at the
shipped `run_ahead = 2` — while the winit thread needs it inside every redraw at
120 Hz. Contention is structural, not incidental.

#### Where the contention is, and why the fix is deferred

`display_sync_produce` returns *before* its acquisition on the threaded path, so
it is not the source on the default build. `pump_watchpoints` is: it takes the
lock on **every redraw**, unconditionally, before establishing whether anything
needs it — and it does real per-frame work on `nes` (heatmap refresh, call-stack
and access-counter replay, watch pump), so the lock is genuine whenever those
features are live. In the common case — overlay hidden, no watchpoints, no
logging — it is pure contention.

The fix is a cheap pre-lock predicate ("does anything here need the emulator
this frame?"), and **v2.3.0 already applied exactly this pattern one layer
over**: `EmuControl::has_rom()` exists as a lock-free atomic precisely because
`pace_frames` was blocking up to a full produce per iteration for a fact it
could read without the mutex. The same shape, unfixed one call deeper.

It is deferred to its own change rather than added here because it touches
debugger internals this campaign has not read, it belongs with its own A/B
measurement, and the instrument that would judge it is the one being landed.
`rlock` and the scanouts-per-frame histogram are now the two numbers to move.

### v2.3.3 F13 — the redraw-path lock acquisition, removed (decision: ADOPTED on its own merits; the shudder outcome is UNRESOLVED)

F12 named `pump_watchpoints` as the contention source: it took the emulator
mutex on **every redraw**, unconditionally, before establishing whether anything
needed it. Two changes:

- **A pre-lock predicate.** `DebuggerOverlay::wants_emu_pump` answers "does the
  per-frame pump actually need `&mut Nes`?" from debugger-side flags alone —
  watchpoints, breakpoints, trace, heatmap, access/exec/interrupt log
  consumers, pending step. Deliberately conservative: a false positive costs one
  lock, a false negative would silently disable a debugger feature. A
  `logs_armed` latch makes it safe to skip the pump's *disarming* pass, which
  is the non-obvious half — "nothing wants a log" is not sufficient, because
  the core could be left logging forever.
- **The call moved off the redraw path** into `post_produce_housekeeping`, inside
  the lock that path already holds. That removes the acquisition entirely rather
  than merely making it conditional — and it fixes a second defect: at divisor 2
  there are two redraws per produced frame, so the old placement **replayed each
  frame's logs twice**. Once per produced frame is the correct cadence for
  per-frame telemetry.

#### The targeted metric collapses

| | before | after |
| --- | --- | --- |
| `rlock` p95 | 8.707 ms | **0.000 ms** |
| `rlock` p99 | 9.008 ms | **0.000 ms** |
| `rlock` max | 33.194 ms | 8.943 ms |

The winit thread no longer blocks on the emulator mutex during a redraw. This is
a floor, not a small delta, and is not in doubt.

#### The outcome metric, settled by a proper A/B (decision: CONFIRMED IMPROVEMENT)

The first attempt at this comparison said the change made things **worse** —
65.31% "exactly 2" before, 51.76% after. That was one capture per side, taken in
different sessions, and it was wrong. Four consecutive captures of the fixed
build alone spanned 51.76-62.59%, a 10.83-point spread, so a single before/after
could not resolve the question in either direction.

Redone properly: both binaries rebuilt from the two adjacent commits that differ
only by this change (the fix and its parent), run **alternately** A/B/A/B rather
than in blocks — so any thermal or host-contention drift over the run is shared
equally instead of loading onto whichever configuration ran second — four
captures each, identical config, 40 s.

| | capture 1 | 2 | 3 | 4 | mean |
| --- | --- | --- | --- | --- | --- |
| **A** — before the fix | 66.05% | 66.51% | 69.36% | 69.08% | **67.75%** |
| **B** — after the fix | 74.52% | 71.33% | 76.12% | 73.10% | **73.77%** |

**+6.02 percentage points, and the ranges do not overlap** — A's best capture
(69.36%) is below B's worst (71.33%). Every B beats every A.

> **The metric in this table is the wrong one — see F14.** "Exactly 2" counts
> refreshes between consecutive *produce* instants, i.e. producer-side jitter,
> not display duration. F14 re-runs this same A/B on the display-side metric:
> the fix still leads, by a smaller margin, with one of the four pairs reversed
> and a paired **p = 0.125**. The statistical discussion immediately below is
> correct as far as it goes, and was itself a correction — but it corrected the
> test while leaving the measurand wrong.

**The statistics were first reported wrongly here, and the correction matters.**
An unpaired permutation test over all 70 possible 4/4 splits gives
p = 1/70 = 0.0143, and that is what this section originally quoted. It is the
wrong test for this design. **Strict alternation is not randomisation**: every B
ran immediately after an A, so run-order drift stays confounded with the change
and the two groups are not exchangeable — which is precisely what the unpaired
test assumes. The correct analysis is paired on adjacent A→B pairs:

| pair | 1 | 2 | 3 | 4 |
| --- | --- | --- | --- | --- |
| B − A | +8.47 | +4.82 | +6.76 | +4.02 |

All four differences are positive, mean +6.02; the exact sign-permutation test
over all 2⁴ arrangements gives **p = 1/16 = 0.0625** — again the smallest value
attainable, but at four pairs that is not conventional significance.

The honest statement is therefore weaker than the first one: the effect is
consistent in sign, consistent in magnitude, and **suggestive rather than
established**. Settling it needs counterbalanced A→B and B→A blocks (or
randomised order) with an order-bias control, which has not been run. Raised in
review on PR #361. The `rlock` collapse and the double-replay fix are
independent of this and stand on their own.

Missed refreshes are ~0.2% in both arms of this session, against 4.60% measured
in the earlier one — the host state differs enormously between sessions, which
is precisely why the blocked, cross-session comparison failed and the alternating
design was necessary. **That is the lesson worth keeping**: the ordering of the
runs was doing more work than the change being measured.

So the change is *probably* an improvement of about six points of
**produce-interval regularity** — consistent across four pairs but short of
conventional significance, pending a counterbalanced design. What it does to
the *display* is F14's subject, and the answer there is smaller and weaker.

#### The `rwork` p99 jump: leading candidate eliminated, not yet attributed

Removing the pump from the redraw handler should have *lowered* `rtot`, yet
across the fixed-build captures `rwork` p99 reads 9.1 / 9.8 / 20.5 / 27.3 /
32.4 ms, where the pre-fix builds read 0.085 / 0.109 / 7.0 ms. `rtot` p99 rose
with it (16.5 → 17-33 ms), so this is not lock time merely relabelled.

**The obvious candidate is eliminated.** The egui shell build on the
overlay-hidden path is genuinely untimed — `ui_cost` is set only on the locked
branch, so `rui` p99 reads 0.000 in every capture — which makes it the first
thing to suspect. But `render_shell` is invoked from **inside the `overlay`
closure** that `gfx.render_with_overlay` calls, i.e. after the `t_present`
restart, so it lands in `rwait` and cannot be this tail.

What remains inside the `rwork` span (redraw signal → GPU dispatch, minus lock
wait) is the framebuffer staging copy and HD-composite prep. A 240 KiB copy is
microseconds; 28-33 ms of it is not credible.

**Leading hypothesis, untested: it is not work at all — it is the winit thread
being descheduled.** `rwork` is wall time, so an OS deschedule inside the span
is indistinguishable from computation, and the variance (9-32 ms across five
captures of one build) has the shape of scheduler noise rather than of a code
path. Note this is the *same* ambiguity `rtot` carried before F8 split `rwait`
out of it — one level further down.

The instrument that would settle it is a third split: compare
`CLOCK_THREAD_CPUTIME_ID` against wall time across the span. Wall ≫ CPU means
descheduled; wall ≈ CPU means real work.

#### The deschedule probe — and the tail that was the measurement, not the program

That instrument was built: `rcpu`, the winit thread's own CPU clock differenced
across exactly the `rwork` span, so `rwork − rcpu` is time the thread was not
running. Two 40 s captures of the same binary:

| condition | `rwork` p99 | `rcpu` p99 | off-CPU |
| --- | ---: | ---: | ---: |
| idle host | 0.066 ms | 0.066 ms | ~0 |
| 20 spinning threads | 0.069 ms | 0.061 ms | 0.008 ms |

The idle row is also the instrument's own sanity check: on an unloaded host the
two clocks should agree exactly, and they do to the reported precision. Under
deliberate contention the gap opens — by 8 microseconds, three orders of
magnitude short of the tail being chased.

**Because the tail did not reproduce at all.** Neither condition showed anything
near 9–32 ms. Going back to when the tail-bearing captures were taken, each of
them ran while a `cargo build` was compiling on the same host. **The tail was an
artefact of the measurement environment, not a property of the frontend** — the
hypothesis under test (descheduling) turned out to be the right *class* of
explanation while being wrong about the cause, since the deschedules were ones
this campaign inflicted on itself.

So the F8-corrected `rwork` p99 of 13.1–22.4 ms quoted earlier in this document,
and the 9–32 ms figures above, should be read as contaminated. `rcpu` stays in
the tree: it is cheap, and it is the check that distinguishes the two the next
time a wall-clock tail appears. The operational rule it earns: **do not capture
frontend pacing telemetry while a build is running**, and treat any wall-clock
tail without a matching `rcpu` tail as environmental until proven otherwise.

The genuine, uncontaminated finding of this section is unchanged and stands on
its own instrument: `rlock` p95 8.707 ms → 0.000 ms.

#### Standing

Confirmed on the metric it targets — `rlock` goes to zero — and *suggestive* on
the one that describes what the user sees (+6.02 points, consistent in sign
across four pairs, paired p = 0.0625, design not counterbalanced). It also removes a per-redraw mutex acquisition from the
hot path and eliminates a double replay of every frame's debug logs, either of
which would justify it independently.

The **first** version of this section claimed the opposite — that the change
looked like a regression — on one capture per side. That claim was retracted
here rather than quietly edited away, because the mistake is instructive: the
comparison was confounded by run ORDER, not by the change, and the fix was to
control the order rather than to gather more data in the same broken shape.

### v2.3.3 F14 — the display metric was measuring the producer (F12 and F13 corrected)

F12 and F13 both quantified "frames shown for the wrong duration" with the same
statistic: **scanouts falling between consecutive produce timestamps**, which
should be exactly 2 at divisor 2. It is not a display metric. Both ends of that
interval are producer-side instants, so a produce that fires 3 ms early followed
by one 3 ms late scores `(1, 3)` **even when the panel showed both frames for
exactly two refreshes each**. It measures how regularly the emulator thread ran.

The display-side metric was already in the trace and was not used for this.
`since_present` is recorded *on the present*, and counts frames produced since
the previous present; at divisor D the healthy pattern is D-1 presents carrying
nothing then one carrying a frame, so the **gap between successive
frame-carrying presents is exactly D, every time**. A gap of D+1 is a frame held
a refresh too long; D-1, one too short. Value and instant both come from the
present, so nothing about producer timing can leak in.

Both, pooled over the seventeen captures on which both are computable:

| metric | wrong |
| --- | ---: |
| refreshes between consecutive produce instants (F12/F13 quoted this) | 32.96% |
| gaps between frame-carrying presents (the display-side one) | **5.41%** |

**A factor of six.** The display was ~94.6% correct while this document said
65-74%.

> **This table was itself wrong once.** The first version of this section
> reported 1.6% and "a factor of twenty", because it divided by the number of
> *presents* rather than the number of *displayed frames* — at divisor 2 that is
> a denominator twice too large. It also tested run-lengths against 1, which is
> the right test only at divisor 2 (at divisor 3 the healthy sequence
> `0,0,1,0,0,1` has runs `2,1,2,1`, so a perfectly-paced 180 Hz panel would have
> scored ~50% wrong). Both were caught in review on PR #362, and the divisor is
> now *inferred* as the modal gap rather than assumed.

#### Re-running the F13 A/B on the correct metric

Same eight captures, same pairing, display-side metric — percentage of frames
shown for the wrong duration, so **lower is better**:

| | pair 1 | 2 | 3 | 4 | mean |
| --- | --- | --- | --- | --- | --- |
| **A** — before the fix | 3.08% | 2.76% | 1.59% | 1.38% | **2.20%** |
| **B** — after the fix | 0.69% | 3.07% | 0.85% | 1.65% | **1.57%** |
| B − A | −2.39 | **+0.30** | −0.74 | **+0.26** | −0.64 |

**Two of four pairs go the wrong way.** Exact paired sign-permutation:
**p = 4/16 = 0.25** — the least significant result attainable short of a
majority reversal. On the producer-side metric the same eight captures looked
like 4/4 with p = 0.0625.

So F13's display-side claim does not survive at all: **the fix's effect on what
the panel shows is not distinguishable from noise in these captures.** The two metrics
agree on *direction* — they rank the eight captures near-identically — and
disagree on magnitude and on whether anything has been shown at all. **The
metric that produced the cleaner-looking answer was the wrong metric**, which is
the specific way this is worth remembering: it did not look like an error,
it looked like a result.

#### What this does and does not overturn

- **Overturned:** every "N% of frames are shown for the wrong duration" figure in
  F12 and F13. The real figure is 5.41% pooled, 0.69-3.08% in the A/B session and
  8.6-18.9% in the earlier ones (host state differs enormously between sessions,
  as F13 already recorded).
- **Overturned:** F13's display-side conclusion. At p = 0.25 with two of four
  pairs reversed, the fix cannot be said to have moved what the panel shows.
- **Left open, in both directions:** whether display cadence is the shudder's
  main remaining cause. F12 and F13 asserted it on numbers that were wrong; the
  corrected numbers do not settle it either way, because the sessions disagree
  by an order of magnitude (0.69-3.08% against 8.6-18.9%). Neither "it is the
  cause" nor "it is not" is supported.
- **Not overturned:** `rlock` p95 8.707 -> 0.000 ms, and the removal of a double
  debug-log replay. Both are direct measurements of what the change removes.
- **Not overturned:** the F12 chain up to and including the lock contention. Only
  its final display-side quantification was wrong.

#### Hypotheses tested and refuted on the way

Three candidate mechanisms were measured against the sixteen captures and none
survived. Recorded because a refuted mechanism is a result:

- **Presentation-path flipping.** `flags` is a constant `7`
  (`VSYNC | HW_CLOCK | HW_COMPLETION`) in every scanout of every capture — never
  `ZERO_COPY`. Constant, so it cannot be a source of *variance*. (That the
  compositor never zero-copies is a fixed extra stage, not a jitter source.)
- **Compositor sequence numbers as ground truth.** `seq` is `0` in every scanout;
  the parse was verified correct against the protocol, so this compositor simply
  does not report a presentation counter. Missed refreshes therefore remain
  *inferred* from intervals, and the analysis now says so in its output.
- **Produce margin.** If frames were landing late against the refresh deadline,
  the error rate should climb with produce phase. It does not: P(wrong) is
  24-47% across all ten phase deciles with no cliff, and the produce-to-next-
  scanout margin is 4.36 ms p50 against an 8.33 ms interval.

#### Instrument changes

- `trace_shape.py` reports `[display cadence]` as the primary result and renames
  the old statistic `refreshes between consecutive PRODUCE instants (producer
  jitter, NOT display duration)`. It also refuses to rate fewer than 100 runs,
  rather than printing `1/4 = 25.00%` beside a 3724-sample measurement.
- **The clock join is now verified, not assumed.** This module's docstring always
  said an unjoinable trace must be reported as such; that was never enforced,
  because `clock_id` was `unknown` in *every trace ever written*. The cause:
  `set_trace_anchor` runs when tracing is armed, and `PresentationClock::new`
  deliberately performs no Wayland roundtrip, so the registry — and the
  `clock_id` event that follows the bind — has not arrived yet. Fixed by
  emitting the id as a comment row once it is known (`note_clock_id`), and by
  checking the join **empirically**: shifted scanouts must span substantially
  the same interval as the produce rows. The empirical check is the load-bearing
  one — it works on the traces already on disk, which carry no id at all, and it
  would also catch a compositor that names `CLOCK_MONOTONIC` and stamps
  something else. Verified in both directions: a 60 s corrupted anchor is
  refused, an intact one reports a 13 ms skew over 39 s.

#### Where this leaves the campaign — UNRESOLVED

The honest position is that **nothing here is settled**, and it is worth being
explicit about what the corrected numbers do and do not license.

They do **not** establish that the fix improved the display: p = 0.25 with two of
four pairs reversed is a null result, not a small positive one.

They also do **not** establish that display cadence is fine and the cause lies
elsewhere. A null A/B says the *configuration difference* was not resolved by
these captures; it says nothing about the absolute level. And the absolute level
is not uniformly small — it is 0.69-3.08% in the A/B session but **8.6-18.9% in
the earlier ones, on the same binary pair**. At 18.9% roughly one frame in five
is shown for the wrong duration, which is not a residual.

That between-session spread is the largest unexplained quantity in this
document, and it dwarfs everything the A/B was trying to resolve: a
configuration difference that cannot beat p = 0.25 sits inside a session-to-
session difference of an order of magnitude. Until it is explained, "display
cadence is not the problem" and "display cadence is the problem" are both
unsupported — the sessions disagree, and no measurement here says why.

The next instrument should therefore ask **what differs between sessions**, not
push further on the A/B, which is underpowered against a source of variance this
large.

### v2.3.3 F16 — a silent wall-clock fallback, and a counter nobody read

Found while trying to VERIFY F15's instrument, which is the only reason it was
found at all: five of six launches produced all-zero readings, and the reason
turned out to be more interesting than the instrument.

**Display-sync never engaged in those sessions.** `pacing` stayed `wallclock`,
`present_mode` stayed `Mailbox`, `refresh_source` stayed `none` — for the whole
run, every run. The stakes are already on record earlier in this document: the
wall-clock pacer dropped **61-147 frames per 45 s** where display-sync dropped
**6-15**.

#### The mechanism, measured

`wp_presentation` bound correctly and its `clock_id = 1` event arrived, so the
protocol was live. But **zero `presented` reports ever came back**. The refresh
estimator needs `PRESENTATION_SAMPLES` = 24 of them before it can answer.

**State the chain precisely — the first version of this section did not.**
Discards block the *measured* refresh, and nothing else. `resolve_pacing` has a
second source it actually *prefers*: a **declared** refresh from
`current_monitor()`. Where one exists, display-sync can engage perfectly well
with discards ongoing. What happened here is the **conjunction**: this compositor
advertises no `wl_output`, so `current_monitor()` is `None` too, and with both
sources absent `refresh_hz` is `None` and display-sync cannot engage.

That distinction matters for anyone reading the counter on another system: a
rising `present_discarded` says **this surface is not being scanned out**. Whether
it also costs display-sync depends on whether a declared refresh is available.
Raised in review on PR #363, in four places at once, by both reviewers.

What arrives instead is `discarded`: composited, never scanned out. Measured on a
backgrounded window, `present_discarded` climbs by ~61 per second — **every
frame** — for the whole session.

#### The counter existed and was read by nobody

`PresentationClock::discarded()` has been there since scanout tracing landed, with
a doc comment, cumulative and correct. Nothing called it. So the entire failure
was invisible: a user whose display-sync silently never engages had no number
anywhere that said why, and neither did this campaign until it tripped over it.

It is now surfaced as `PerfView::present_discarded` and a `present_discarded`
column in the perf log. **A diagnostic that is never surfaced is not a
diagnostic** — that is the whole content of this entry, and it cost five wasted
verification captures to notice.

Two properties to read it correctly. It is **cumulative** over the life of the
presentation clock, so successive rows must be differenced for a rate. And
**zero is not proof of health**: the field reads zero both when nothing was
discarded and when there is no presentation clock at all (non-Wayland, or the
global never bound), because the call site is a `map_or(0, ...)`. Pair it with
`refresh_source` to tell those apart.

#### What this does NOT establish

Two corrections to the hypothesis this started from, both against my own first
reading:

- **It is not "sticky".** The first framing was that display-sync degrades and
  never recovers. `settled` is set **only on success**, so `request_feedback` and
  `poll` keep issuing indefinitely and the regime should engage as soon as 24
  `presented` reports accumulate. Recovery after an occlusion ends is therefore
  expected by construction — **but it has not been tested**, and "expected by
  construction" is exactly the kind of claim this campaign has had to retract
  three times already.
- **It is probably not the maintainer's shudder.** An occluded window is not a
  use case. This matters if a session *starts* occluded, or if a real transient
  occlusion during play leaves the regime wrong for longer than it should. Absent
  evidence of either, it should not be promoted to a cause.

The value delivered is narrower than a fix and worth having anyway: a failure
that was completely silent now reports itself.

#### The capture-validity gate

`perf_log_check.py` now **fails closed** on `present_discarded > 0`, alongside its
existing config-mismatch assertion. An occluded capture does not look broken —
that is the whole problem with it. It produces plausible, well-formed, entirely
misleading numbers, and this campaign spent five verification captures in that
state without noticing.

Three states, deliberately, because two would have been dishonest: `> 0` fails;
`0` with the column present passes as verified; and a capture written **before**
the column existed passes but is reported as *"validity UNKNOWN, not verified"* —
it cannot be proven valid, and saying "window was on screen" of a log that never
measured it would be a small version of exactly the error F14 is about.

Every pacing conclusion in this document that predates the column therefore
carries an unverifiable assumption: that the window was actually on screen. The
sixteen scanout-bearing captures almost certainly were — they *have* `presented`
reports, which an occluded window cannot produce — but that is an inference, not
a check.

### v2.3.3 F17 — the between-session spread is emulation budget margin

F14 left one quantity unexplained and named it the largest open question in this
document: display cadence error varied **0.69-3.08%** in one session and
**8.6-18.9%** in others, *on the same binary pair*, dwarfing anything the A/B was
trying to resolve. This is the answer, and it is not a pacing defect.

#### The result

Across **27 captures** with at least 10 s of steady state, plotting cadence error
against **what fraction of the NTSC frame budget the emulator consumes at p95**
(`cost_p95` / 16.639 ms) separates them completely:

| budget utilisation at p95 | n | cadence error |
| --- | ---: | --- |
| **< 60%** | 17 | 0.00-8.59% |
| **>= 60%** | 10 | 9.02-18.93% |

**Perfect separation — no overlap.** Pearson **r = +0.836**. The good captures
are not merely better, they are *tightly clustered*: `cost_p95` spans
8.955-9.122 ms across sixteen of them, a 0.167 ms band.

`cost` is emulation work measured **after** the mutex is in hand (the v2.3.3 F1
split), so this is not lock contention relabelled. It is the emulator taking
longer to do the same work, against a fixed deadline.

#### Two independent routes to a thin margin

| run-ahead | n | `cost_p50` | = % of budget | tail p95/p50 | cadence error |
| ---: | ---: | ---: | ---: | --- | --- |
| 0 | 3 | 5.652 ms | 34.0% | 2.39-2.97 | 9.02-15.28% |
| 1 | 21 | 8.736 ms | 52.5% | 1.03-1.88 | 0.00-17.66% |
| 2 | 3 | 12.943 ms | 77.8% | 1.04-1.05 | 14.77-18.93% |

- **BASELINE.** Run-ahead multiplies emulation work per produced frame, by
  design. On this host that is 34% of the frame budget at depth 0, 52% at 1, and
  **78% at 2**.
- **TAIL.** Host contention adds a p95 tail on top. The `ra = 0` row shows it
  most clearly: a 34% baseline with a 2.4-3.0x tail still lands over 80%.

Either route reaches the same place, and the cadence error follows the *total*,
not the cause.

#### Why this is causal and not merely correlated

Cost and cadence could in principle share a common cause — a loaded host makes
the emulator slow *and* independently disturbs presentation. **The `ra = 2`
captures separate the two, and they are the decisive evidence in this section.**

Their tail ratio is **1.04-1.05**. That is *evidence against* a large contention
tail, not proof of its absence — a uniform slowdown raises p50 and p95 together
and leaves the ratio flat, so the ratio bounds the *spread* of contention, not
its level. What it does establish is that their elevated cost is **structural**:
three emulated frames per produced frame, by design, not a spike inflicted by the
host. And they still show **14.77-18.93%** cadence error:

| capture | `cost_p50` | % of budget | tail | cadence error |
| --- | ---: | ---: | ---: | ---: |
| `023514` | 12.943 ms | 78% | 1.044 | 18.93% |
| `023655` | 12.928 ms | 78% | 1.054 | 14.77% |
| `023836` | 12.948 ms | 78% | 1.040 | 15.38% |

A *deliberate*, structural cost increase reproduces the full error. That is the
manipulation the observational captures could not provide, and it is why this
section claims causation where F13 could not.

**Scoped precisely:** it shows emulation cost is *sufficient* to produce cadence
error of this size. It does **not** show cost is the only contributor, nor that
pacing contributes nothing — these captures cannot separate a pacing component
riding along with the cost, and no measurement here attempts to.

#### What this means

**The between-session spread was never a property of the pacing code.** It is how
much frame budget was left over, and it moved for two reasons this campaign was
not tracking: which run-ahead depth the capture used, and how loaded the host was
at the time. That also explains why the F13 A/B could not resolve a configuration
difference — it was measuring a ~0.4-point effect inside a source of variance
worth ten points.

**Run-ahead 2 is structurally marginal on this host**: ~12.94 ms of a 16.639 ms
budget, leaving ~3.7 ms for everything else. That is not a bug — run-ahead buys
input latency with CPU time, and this is the price — but it is a real cost that
was not written down, and it is directly relevant to a shudder reported *at
run-ahead 1 and 2*.

#### Limits, stated plainly

- **`ra = 2` is n = 3, from one session.** The mechanism is clear and the tail
  ratios rule out contention, but three captures is three captures.
- This does **not** identify the maintainer's shudder. It explains the spread
  between *these* captures. Whether their host has the same margin is unmeasured.
- The 60% figure is a **separator observed in this data**, not a derived
  threshold. The mechanism (deadline pressure) is continuous; do not read 60% as
  a limit with physical meaning.
- Pre-F16 captures cannot be proven to have had an on-screen window (see F16), so
  each carries that unverifiable assumption.

#### What to do with it

Report budget utilisation where run-ahead is chosen, and treat `cost_p95`
approaching the frame period as the leading indicator it evidently is. The
existing perf gate already tracks `cost_p95`; what it lacked was the comparison
against 16.639 ms that turns it into a margin.

### v2.3.3 F19 — slim restore for run-ahead (decision: REJECTED, not implemented)

The proposal: run-ahead, netplay rollback and TAS seek all re-simulate frames
immediately after restoring, so the 245,760-byte framebuffer they restore is
overwritten before anyone sees it. `PPU_SNAPSHOT_SLIM_FLAG` and
`Nes::snapshot_core_into_slim` already omit it — built in F3/F4 for the rewind
ring — and nothing else uses them. Free win, apparently.

**Projected ~110 µs per restore. Measured 6.9 µs.**

| bench | full | slim | saving |
| --- | ---: | ---: | ---: |
| `nes_restore_quiet_flowing_palette` | 122.8 µs | 115.9 µs | **6.9 µs** |
| `nes_restore_quiet_mmc3` | 123.7 µs | 116.2 µs | **7.4 µs** |

Against the 2.802 ms `nes_runahead_budget` increment that is **0.25%** — an
order of magnitude under the project's >3% bar. **Rejected before
implementation.**

> The first version of this table read 8.4 µs, from a **confounded** probe: it
> booted a fresh `Nes` and ran one frame, while every other bench here uses
> `warmed_nes` (60 frames, rendering enabled, OAM and palette populated). That
> is different serialized content on the two sides of the comparison. Raised in
> review on PR #365, corrected here — the delta *shrank*, so the rejection holds
> and is slightly stronger than first reported.

#### Why the estimate was wrong, which is the part worth keeping

It came from the project's own (correct) statement that the framebuffer is **94%
of the snapshot BYTES**, and that was carried silently into a claim about
**TIME**. 245,760 bytes is ~12-25 µs of memcpy at ordinary bandwidth, so it could
never have been 94% of a 122 µs restore. The measured share is ~7%. The estimate
was off by 13x.

> **Bytes are not time.** Convert one to the other with a bandwidth figure before
> quoting a projected win.

Cost of getting this wrong, had it not been measured first: a change with a
save-state correctness hazard (after a slim `finish`, the framebuffer holds the
*visible* frame while the state is the *persistent* one, and `Nes::snapshot`
serializes the framebuffer) across three call sites, two of which needed contract
renegotiation — netplay's `gameplay_digest_parts` hashes the framebuffer as a
**desync classifier**, and TAS seek has a test asserting its framebuffer equals
linear replay's. All of that for 0.30%.

#### What it did establish

**Restore costs ~114 µs with no framebuffer at all.** So the 8.3x asymmetry
against `snapshot_core_into` (14.7 µs) is not the big payload — it is per-section
deserialization of the small structures, and the v2.8.0 fast path that halved
snapshot did nothing for it. That is a better-aimed question than the one this
section asked, and it matters on the netplay-rollback and TAS-seek paths where
restore runs many times per second. It does **not** matter for run-ahead: F18
settled that 95% of run-ahead's cost is `run_frame` itself.

The `nes_restore_quiet_slim_*` probe stays in the bench as the evidence.

### v2.3.3 F15 — the trigger is late, not the emulator

The produce interval's standard deviation tracks missed presents at **r = 0.937**
(F17's data), so the interval's variance is where the display cadence error comes
from. It has exactly three terms: how regularly the trigger is **sent**, how long
it takes to **arrive**, and how long the frame takes to **make**. Only the third
was measured.

`tick_iv` and `tick_lat` measure the first two, as two independently-ranked
series — never one derived by subtracting the other, per F8. The display tick's
channel payload changed from `()` to a `CLOCK_MONOTONIC` timestamp, which is what
makes a cross-thread hop measurable at all.

**First verified capture** (SMB, `run_ahead = 1`, display-sync /2, window on
screen and confirmed valid by F16's gate, 18 post-warmup rows):

| series | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| `tick_lat` — winit→emu hop | **0.033 ms** | **0.043 ms** | **0.050 ms** |
| `tick_iv` — between tick *sends* | 16.289 ms | **24.578 ms** | 28.637 ms |
| `produced` — resulting interval | 16.269 ms | **24.635 ms** | — |

#### The result

**The cross-thread hop is 33-50 microseconds.** It is not a contributor, and the
last completely unmeasured step in the produce chain is now measured and
eliminated. One 13.155 ms outlier appears in `tick_lat_max` across 1494 ticks —
a single scheduler hiccup, not a systematic cost.

**`produced` p95 (24.635 ms) matches `tick_iv` p95 (24.578 ms) to within 0.06
ms.** The produce tail is inherited wholesale from the trigger interval. Combined
with `rlock` = 0.000, `tick_timeout` = 0 of 1494, and `cost_p95` = 9.198 ms (55%
of budget, F17's healthy band):

> **The emulator is not late. It is asked late.**

That is the first *positive location* this campaign has produced rather than an
elimination. The remaining defect is in **when the winit thread decides to send
the tick** — `display_produce_due` and the present cadence feeding it — and every
other candidate in the chain is now measured and ruled out.

#### Why it took a working capture to say this

The instrument was written, gated, and mutation-checked hours before it produced
a single real sample: five verification attempts all read 0.000 because the
window was occluded and display-sync never engaged, which is what F16 exists to
detect. Its plumbing was pinned by tests (zero-payload guard, first-tick
suppression, take-clears) that were themselves mutation-checked — but plumbing
tests cannot verify an instrument, only that it lies in none of the ways
anticipated. The numbers above are the verification.

### v2.3.3 F18 — run-ahead's cost is the frames, and depth 3 throttles itself

F17 measured run-ahead 2 at ~78% of the frame budget and asked whether that cost
is reducible. It is not: it is emulation work, linear in depth, and the state
handling around it is noise.

**Design.** Sixteen captures, four at each depth, in a **Latin square** — each
depth appears in each round-position exactly once, so run-order drift cannot load
onto any one depth. That is the correction F13 earned: strict alternation
balances the *direction* of a monotone drift but does not buy exchangeability,
and a Latin square does. Every capture was validity-gated first (F16); **16/16
passed**, window on screen, `display-sync /2`, discard rate ≤ 1%.

| requested `ra` | n | `cost_p50` | % budget | `cost_p95` | % budget | throttled |
| ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 0 | 4 | 4.180 ms | 25.1% | 4.404 ms | 26.5% | no |
| 1 | 4 | 8.671 ms | 52.1% | 9.060 ms | 54.5% | no |
| 2 | 4 | 12.884 ms | **77.4%** | 13.385 ms | **80.4%** | no |
| 3 | 4 | 4.276 ms | 25.7% | 4.516 ms | 27.1% | **yes** |

#### The cost is linear in depth, at the core's own frame cost

Per-depth increments: **+4.491 ms** and **+4.213 ms** — equal within 6%, i.e. one
extra `run_frame` each, at the ~4.3 ms the core costs per frame. Together with
F19's measurement that snapshot + restore is ~136 µs, **run-ahead's cost is the
emulated frames and essentially nothing else.** It is the price of the feature,
not overhead around it, and the only way to reduce it is to make the core faster.

This also settles a loose end F18 was explicitly told not to build on. The
earlier increments — read off captures taken in *different sessions* — were
+3.09 ms and +4.20 ms, a 1.1 ms asymmetry that would have been an interesting
finding about run-ahead's cost structure. Controlled, they are +4.49 and +4.21.
**The asymmetry was session artefact**, exactly as suspected, and quoting it
would have sent someone hunting a structure that does not exist.

#### Depth 3 throttles itself — correct behaviour, and it looks like a bug

`ra = 3` measures identically to `ra = 0`. It is not being ignored:
`run_ahead_throttled` reads `true` in every one of its captures while
`run_ahead` still reports the requested 3. `EmuCore::update_runahead_throttle`
engages at **85% of the frame budget** and releases below 40% (hysteresis, so it
cannot oscillate when the cost drops as the extra frames stop). Depth 3 would
cost ~17.2 ms against a 16.639 ms period, so the throttle correctly refuses it
and the frames are not run.

Worth recording because the raw table reads as a defect — a requested depth with
no effect — and it is the opposite: the budget guard doing exactly its job. The
`run_ahead_throttled` column is what distinguishes the two, and any future reader
of a depth sweep needs it.

#### F15 replicates across all four depths

| `ra` | `tick_iv` p95 | `produced` p95 | `tick_lat` p95 |
| ---: | ---: | ---: | ---: |
| 0 | 17.562 ms | 17.572 ms | 0.048 ms |
| 1 | 23.814 ms | 23.782 ms | 0.044 ms |
| 2 | 25.970 ms | 26.063 ms | 0.045 ms |
| 3 | 17.586 ms | 17.581 ms | 0.046 ms |

The produce interval **is** the trigger interval at every depth, and the
cross-thread hop stays at 44-48 µs throughout. F15's conclusion — the emulator is
not late, it is asked late — is a four-condition replication, not a single
capture.

### v2.3.3 F21 — the run-ahead throttle: lower, and one step at a time (ADOPTED)

F18 measured `run_ahead = 2` at 77.4% of the frame budget with **10.7%** of
frames held for the wrong number of refreshes, while the throttle's 0.85 gate had
not fired. That band — harmful but unthrottled — is the defect. Two changes, each
A/B'd, and the first one's result is what produced the second.

#### Threshold 0.85 -> 0.75

Chosen to sit between the two MEASURED points (52.1% healthy / 77.4% harmful),
deliberately not tuned finer since the sweep has no conditions between them.
Four captures per arm at `run_ahead = 2`:

| | captures (% frames wrong) | mean | `cost_p50` |
| --- | --- | ---: | ---: |
| A — 0.85 | 7.87, 8.61, 5.88, 11.94 | 8.57% | 12.7 ms |
| B — 0.75 | 0.69, 0.61, 0.92, 0.69 | **0.73%** | 4.24 ms |

All four pairs favour B, mean −7.85 points, paired **p = 1/16**. **But look at
`cost_p50` = 4.24 ms: that is depth-0 cost.** B did not make depth 2 smoother, it
disabled run-ahead. The user asked for depth 2 and got depth 0 — two frames of
input latency traded for the cadence.

#### Step down, do not zero

The sweep says that trade was unnecessary: depth 1 costs 52.1% of budget and
shows 1.7% wrong. So the throttle now carries `runahead_throttle_steps` — how
many depths it has removed — instead of a bare "is it off" bool. Engage subtracts
**one** step and re-measures on the next median window, so a host that genuinely
cannot afford any depth still converges to 0, without the cliff. Release predicts
the cost of giving back one step using F18's per-frame-linear model
(+4.49 / +4.21 ms per depth, equal within 6%).

Three arms, Latin square so no arm owns a round-position, three captures each:

| arm | % frames wrong | mean | `cost_p50` | depth reached |
| --- | --- | ---: | ---: | ---: |
| A — 0.85 | 9.21, 6.53, 3.56 | 6.43% | 12.707 ms | 2 |
| B — 0.75 all-or-nothing | 1.15, 0.84, 0.61 | 0.87% | 4.258 ms | **0** |
| **C — 0.75 step-down** | 0.76, 0.91, 0.84 | **0.84%** | 8.556 ms | **1** |

**C matches B's cadence and keeps a frame of latency.** The per-pair differences
between C and B are +0.07, +0.23 and −0.39 points — no difference, which is the
point: C's win over B is not cadence, it is that B discarded a depth it did not
need to. `cost_p50` 8.556 ms also matches F18's independently-measured depth-1
cost of 8.671 ms, so C is demonstrably *running* at depth 1 rather than landing
near it by coincidence.

Against the unthrottled default, **6.43% -> 0.84%** of frames held for the wrong
duration.

#### What this is and is not

It is a **budget guard doing its job earlier and more gently**. It is not a fix
for the shudder: `display_produce_due` was measured before any of this and
delivers 98.3% correct holds at the shipped `run_ahead = 1` (F18's sweep), so
there was no pacing-logic defect to fix. What there was is a host that cannot
afford depth 2, and a guard that noticed too late and then over-corrected.

The thresholds are measured on ONE host. 0.75 is a separator between two observed
points, not a derived constant, and a machine with a faster core will sit
differently against it — which is the argument for keeping the guard adaptive
rather than encoding a depth limit.

### v2.3.3 F27 — the throttle was pacing itself against a fifth of a window (ADOPTED)

**Decision: ADOPTED.** The run-ahead throttle's oscillation — the one artefact in
this campaign that matches the reported "picture jumps forward and back" — is
fixed, and the mechanism is a stale statistic rather than a bad threshold.

#### The defect

F24 gated the throttle to one depth change per median window. The reasoning was
right; the constant was wrong. It used **120** frames because 120 is the number
of samples the produce-cost ring must hold before it reports a median at all,
and mistook that *minimum-to-report* for the ring's *capacity*. The ring holds
`perf::WINDOW` = **600**.

A p50 sits at index 300 of 600 samples, so 120 frames of turnover cannot move it
— not approximately, not partially. **The gate waited a fifth of a window and
called it one.** That is why F24 measured as no improvement, and why it read as a
refutation of the stale-median theory when it was an under-strength test of it.
Two later theories (a sustained-release requirement, a wider band) died against
the same under-strength gate.

#### The evidence

The per-evaluation log (F26) settled it. Transitions arrive in immediate pairs
sharing a median to three decimal places:

```text
THR check depth=2 steps=0 cost=12.958 engage_band=12.479 engage=true
THR check depth=1 steps=1 cost=12.958 engage_band=12.479 engage=true

THR eval depth=0 steps=2 cost=4.994 per_frame=4.994 pred=9.988 release=true
THR eval depth=1 steps=1 cost=4.994 per_frame=2.497 pred=7.491 release=true
```

The second line of each pair is decided on a measurement of the depth the first
line just left. In the release pair the arithmetic goes visibly wrong in the same
breath: an unchanged 4.994 ms is divided by a depth that has already changed, so
the per-frame cost *halves* without any frame having become cheaper, and each
release makes the next one look safer. That is a mechanism, not a correlation.

#### The fix, and what it measures

The gate is now expressed in terms of the ring it reads, so the two cannot drift
apart again. Three captures, SMB, `run_ahead = 2`, window verified on screen:

| | transitions | engages | releases | % frames wrong |
| --- | ---: | ---: | ---: | ---: |
| before (F22, 3 captures) | 6-7 / 24 s | 3-4 | 2 | — |
| **after (F27, 3 captures)** | **1** | **1** | **0** | **1.31%** |

The single remaining transition is the correct one: the engage at 12.65 ms, over
the 12.48 ms band, after which the state is stable for the rest of the run.

The predicate was never wrong — its input was. At depth 1 the honest median now
reads **8.670 ms**, matching F18's independently measured depth-1 cost of 8.671
ms, predicts 13.005 ms one depth up, and correctly declines to release. Before
F27 the same evaluation read 4.994 ms, a depth-0 measurement, and released.

Display-side, a `run_ahead = 2` configuration now delivers **1.31%** of frames
held for the wrong duration, against F18's measured **10.7%** at depth 2 and
**1.7%** at depth 1 — the throttle is delivering depth-1 display quality from a
depth-2 request, which is what a budget guard is for. `cost_p95` 9.04 ms (54% of
budget) puts it squarely in F17's healthy band.

#### What this does not claim

It fixes the throttle oscillation on this host. It is **not** confirmation that
the maintainer's shudder is gone — that is a subjective report on a different
machine, and the standing rule from F6 applies: the drop counters said "fixed"
once already and were wrong. What can be said is that the one measured artefact
whose signature matches the report no longer occurs.

The 600-frame window is 10 s at NTSC, so an unaffordable host takes one window
per step to converge. The counter is pre-seeded to one full window so the *first*
decision is gated only by the ring having enough samples — there is nothing stale
to wait out at power-on.

### v2.3.3 F28 — the engage arm computes instead of waiting (ADOPTED; one arm REJECTED)

**Decision: ADOPTED** for the predictive cascade; **REJECTED** for ring-reset.

F27's window is correct and it introduced a cost: `run_ahead = 3` reaches a
sustainable depth but spends ~12 s over budget getting there, because every step
waits a full 10 s for the median to turn over.

#### Why the obvious asymmetry is wrong

"Engage faster than you release" is the right instinct and the wrong mechanism.
At depth 3 the measured cost is ~17.2 ms, which exceeds the 12.48 ms engage band
at **every** depth — so a gate that merely engaged on less evidence would walk
3 → 2 → 1 → 0 and discard the whole feature. Engaging is only safe on *less
waiting* if it is done on *more information*.

The fix is to stop waiting for the ring and compute the next depth's cost
instead, using the per-frame-linear model F18 measured (+4.491 and +4.213 ms per
depth, equal within 6%) — the same model the release arm has always used. Within
one evaluation, the cascade steps while the PREDICTED cost at the reduced depth
is still over the band: 17.2/4 = 4.3 ms per frame, so depth 2 predicts 12.9
(over) and depth 1 predicts 8.6 (under). It stops at 1. Releasing still demands
a full window and a real measurement, because releasing on a stale median is the
direction that produced the F27 oscillation.

#### The A/B

Three arms, one binary (an env switch, since an earlier A/B in this campaign
compared two binaries that turned out byte-identical), `run_ahead = 3`, Latin
square. Five paired rounds for the two live arms:

| arm | converge | frames wrong | underruns |
|---|---:|---:|---:|
| window — shipped | 12.12 s | 4.82% | 0.60 |
| **predict — adopted** | **2.80 s** | **2.24%** | **0.20** |
| reset — rejected | 4.00 s | 2.02% | **1.00 (3 of 3)** |

**5/5 paired rounds favour `predict` on both convergence and cadence, exact
one-sided sign p = 0.0312** — at the floor for n = 5, which is why five rounds
were run rather than three (three floors at 0.125 and could not have reached
significance whatever it showed).

**`reset`** — clearing the produce-cost ring on every depth change, so the median
is never stale and the gate collapses to the 2 s minimum — converged faster than
the shipped arm and matched `predict` on cadence, but produced **an audio
underrun in every one of its three captures**, against 0.20 and 0.60 for the
other two. Rejected on that alone; the mechanism was not chased further.

#### Verification of the promoted default

The A/B measured `predict` through an env switch; the shipped default must
reproduce it. Two fresh captures at `run_ahead = 3`: converge **3.0 s**, final
depth **1**, two transitions, 1.33% and 0.81% of frames wrong.

At `run_ahead = 1` the change is inert by construction — the diff is **purely
additive (65 insertions, 0 deletions) inside `if engage {`**, and the shipped
default fires **zero** transitions, so the added code never executes. An
interleaved same-session A/B against `main`'s binary was run anyway and could not
resolve anything: `main` itself measured **2.93%** where it had measured 1.35%
earlier the same day, and unchanged code spanned 2.4-6.9% across captures. That
drift is consistent with F17 — cadence error tracks how much frame budget is
left, and the host had been building and capturing continuously for hours. The
no-regression claim therefore rests on the code path not executing, which is
checkable, and not on that measurement, which is not.

### v2.3.1 G3 — sink dead per-dot derivations to their use site (decision: REJECTED, reverted)

The campaign's highest-ranked *code* item, and the same transformation shape as
the adopted v2.3.0 P1. Two sites compute values they then discard:

- `tick_sprite_eval_per_dot` derives `next_line` and `sprite_height` on entry,
  but the `match self.dot` consumes them only in the `65..=256` arm — dead on
  dots 0, 1..=64 and 257..=340, i.e. **149 of 341 dots**.
- `tick_oam_bus` derives `sprite_height` and `scan` above the `cycle < 65`
  secondary-OAM-clear path that discards both — dead across a quarter of every
  visible line. (v2.3.0 P1 had already moved the `cycle == 0` return above them.)

Both were sunk to their single point of use — in the sprite-eval case, inside the
`if !self.sprite_eval_done` guard, tighter than the match arm. All inputs are
pure reads of `scanline` / `region` / `ctrl`, so byte-identical by construction.

**Correctness verified before measuring:** AccuracyCoin **100.00% over 141
assigned tests**, `visual_regression` 9/9 (golden framebuffers — the direct
byte-identity evidence), full `--features test-roms` workspace suite green,
clippy clean at `-D warnings`.

**Two independent A/B runs, and the order-bias control is the story:**

| workload | run 1 candidate | run 2 candidate |
| --- | ---: | ---: |
| `nestest` | −0.56% (p = 0.00) | −0.05% (p = 0.84) |
| `flowing_palette` | +0.20% (p = 0.33) | −0.17% (p = 0.17) |
| `nestest_fast` *(shipped)* | −0.03% (p = 0.91) | −0.14% (p = 0.48) |
| `flowing_palette_fast` *(shipped)* | −0.01% (p = 0.95) | +0.01% (p = 0.96) |

Run 1's `nestest` −0.56% at p = 0.00 looks like a small real win. It is not, and
the A/B/A control proves it directly rather than by argument: **run 2's control —
the reference benched against itself, with no code difference whatsoever —
reported `nestest` at −0.59%, p = 0.00.** The drift and the "effect" are the same
size, on the same workload, at the same significance. Run 1's control had already
flagged a −0.39% (p = 0.03) drift on `nestest_fast`.

**Rejected and reverted.** Both shipped `_fast` variants are flat across both
runs (p ≥ 0.48, intervals straddling zero).

**Why it does nothing — the generalizable finding.** LLVM already sinks pure,
side-effect-free computations past branches that do not use them. At
`opt-level = 3` with fat LTO, writing the sink by hand tells codegen nothing it
had not already worked out. The source change made explicit what the optimizer
was doing anyway.

This reframes **v2.3.0 P1**, which bundled an `#[inline]` with a hoist of exactly
this shape and measured −5.13%. The two were never separated. G3 is evidence that
the hoist half contributes ~nothing, which points at the `#[inline]` — a change
to the *inliner's cost model*, something LLVM cannot infer — as the actual source
of that win. Recorded as a hypothesis, not a conclusion: it was not re-measured
in isolation.

Both sites keep a comment marking the attempt so it is not re-tried.

### v2.3.1 G2 — `Ppu` field layout (decision: REJECTED — and it exposed a harness bug)

The campaign item asked to reorder `Ppu`'s 114 fields by access frequency,
noting the ~15 hot ones are "scattered, with a 2 KiB `rgba_lut` sitting between
the palette state and the framebuffer pointer", and called it "pure reordering —
byte-identical by construction".

**The premise is void.** `Ppu` is `#[repr(Rust)]`, so declaration order does not
determine memory layout; rustc is free to reorder and does. Probed offsets:

```text
 488  rgba_lut (2048 B)  … ends 2536
2570  v      2574  dot    2576  scanline   2578  bg_shift_lo
2580  bg_shift_hi        2582  at_shift_lo 2584  at_shift_hi
2586  flags_cached_scanline          <- 17 bytes, one cache line
2828  x
```

rustc sorts by alignment, which packs every hot `u16`/`i16` scalar contiguously
into a single cache line and puts the 2 KiB LUT *before* the whole hot cluster —
the opposite of what the item describes. Source reordering cannot move any of it.

Measured anyway, in the only form that can change layout — `#[repr(C)]`, which
forces declaration order — plus a variant moving the 256-byte `oam_decay_cycles`
(dead unless OAM decay is enabled, default-off) out from between the scroll
registers and the per-dot render state:

| run | candidate | result |
| --- | --- | --- |
| 1 | `repr(C)` | −1.84% … −2.75%, **p = 0.00 on all four** |
| 2 | `repr(C)` + cold field moved to end | no change on 3 of 4 (p ≥ 0.31) |
| 3 | `repr(C)` again | **no change on all four** (p ≥ 0.11) |

**Run 1 was wrong, and run 3 is why.** The same candidate that produced a
textbook −2% at p = 0.00 on every workload produced nothing on re-measurement.
Nothing about the code changed between them.

**Root cause — a systematic bias in `ab_check.sh`, now fixed.** The reference was
always benched FIRST and the candidate SECOND. Anything that makes the host
monotonically faster over the life of a run — page-cache warming, governor
ramping, a background job finishing, boost/thermal settling — is therefore
indistinguishable from "the candidate is faster". Run 1 followed a period of
heavy local activity (test runs, `perf record`, worktree builds); the machine was
still settling while the reference was measured and had settled by the candidate.

The fix is an **A/B/A order-bias control**: the reference is now re-benched a
third time, last, against its own first run. Whatever that reports is pure
position-in-the-run drift and is the noise floor the candidate must be read
against. The script also now states that a single run is not a result and that
anything under ~5% needs an independent second run — with this experiment as the
cautionary example.

**Item rejected.** No reproducible effect from any layout change tried. That is
also the physically sensible answer: `Ppu` is ~2,856 bytes and stays L1-resident
across a frame, so field layout has little left to buy. Layout is not where this
emulator's remaining time is.

### v2.3.1 G1 — idle-line fast path, re-measured (decision: REJECTED again, stays default-OFF)

The v2.3.x campaign predicted the default-OFF `ppu-idle-line-fast` path
(§P2, max −1.55%, below the bar) "becomes worthwhile if per-dot dispatch gets
cheaper", and v2.3.0 P1 made per-dot dispatch cheaper by −5.13%. Re-measured on
that basis. Criterion change analysis, host CPU-pinned (`taskset -c 2-5`),
2 s warm-up / 10 s measurement, feature-OFF baseline vs feature-ON:

| bench | change | p | verdict |
| --- | ---: | ---: | --- |
| `nes_run_frame_nestest` | −0.94% | 0.00 | small win |
| `nes_run_frame_flowing_palette` | **+0.98%** | 0.02 | small **regression** |
| `nes_run_frame_nestest_fast` | −0.36% | 0.29 | no change |
| `nes_run_frame_flowing_palette_fast` | +0.84% | 0.06 | no change |

**Rejected.** Nothing approaches the >3% bar, the two workloads disagree in
sign, and — decisively — **both `_fast` variants report no change, and those are
the shipped configuration** (`fast_dotloop` became the default in v2.2.3). The
feature stays implemented and default-OFF on exactly the terms §P2 set.

Worth recording that this re-measurement **disagrees in sign with §P2** on
`flowing_palette` (−1.31% then, +0.98% now). Neither is wrong so much as both are
inside the noise for an effect this size. The consistent finding across two
independent sessions is that the idle-line path moves the shipped configuration
by less than ±1.5%, with an unstable sign — which is what "does not clear the
bar" means in practice.

**Method note, which cost a wrong intermediate conclusion.** The first pass
adjudicated this from point-estimate ratios plus the v2.3.1 contention heuristic
(host contended when `3 × robustCV` exceeds the effect being tested). That
heuristic is correct for the CI *regression* gate, where the question is whether
one delta could be noise — but it is the wrong statistic for an adoption
decision taken from 100-sample means, where the relevant quantity is the
confidence interval and the standard error falls as `CV / √n` (≈0.2% here, not
2%). Applied to an adoption decision it demanded a quiet host that no desktop
provides and would have refused every verdict in this campaign.

**Adoption decisions are adjudicated by criterion's `--baseline` change analysis
(change interval + p-value), as §P2/§P3/§P4 already did.** The v2.3.1 gate keeps
its 3×CV rule for the job it was built for. Two different questions, two
different statistics; conflating them is what produced the wrong first read.

### v2.3.0 P1 — per-dot sprite-eval / OAM-bus call cost (decision: ADOPTED)

The v2.3.0 frontend-stutter investigation re-profiled the core on a quiet machine
(2% outliers, vs 39% on the first noisy attempt — a reminder that a contended
machine invalidates the baseline before it invalidates the conclusion). Excluding
~17% of samples belonging to criterion's own harness (rayon plumbing, `libm exp`,
its sorts), self-time split **PPU ~53% / CPU+bus ~39%**, with two per-dot helpers
outside every previously-examined lever: `tick_sprite_eval_per_dot` **4.45%** and
`tick_oam_bus` **3.22%**.

`perf annotate` (the same instrument that redirected P4) found the actual cost was
not the state machines themselves:

- In `tick_sprite_eval_per_dot` the two hottest instructions in the whole body were
  its own `push %rax` (5.35%) and `ret` (5.41%) — **pure call overhead**. It is
  invoked once per *eligible* dot from the fast dot path — visible dots 1..=256
  with rendering enabled, up to 61,440/frame (not all 89,342: idle lines and
  rendering-disabled paths bypass it) — and LLVM had
  declined to inline it.
- `tick_oam_bus` derived `sprite_height` (a `PpuCtrl` test) and the y-test
  reference `scan` **before** its dot-0 early-out, computing and discarding both.

Two byte-identical changes: add `#[inline]` to `tick_sprite_eval_per_dot`, and
hoist the `cycle == 0` early-out above the two derivations in `tick_oam_bus`.

| Workload | before | after | change (95% CI, p) |
|---|---|---|---|
| `nes_run_frame_nestest_fast` | 3.8987 ms | **3.7830 ms** | **−5.13%** (−5.60…−4.60, p = 0.00) |
| `nes_run_frame_flowing_palette_fast` | 2.7314 ms | **2.6354 ms** | **−3.51%** (−3.93…−3.10, p = 0.00) |

Both clear the **>3%** adoption bar on both workloads. Byte-identity verified:
AccuracyCoin **141/141** (the exact-count gate), nestest golden **0-diff**, and
`rustynes-ppu` unit tests 91/91. **Adopted.**

Note what this does *not* change: the core is still ~3.8 ms, not the aspirational
≤ 2 ms (see §Targets). The remaining bulk is work the accuracy model requires, and
the levers below were already measured and rejected.

### v2.2.3 P4 — every-cycle bus cost `cpu_clock` (decision: no change adopted)

`<LockstepBus as Bus>::cpu_clock` is the second-hottest function at **22.43%**
of frame self-time. The v2.0.0 substrate calls it once per CPU cycle and it
unconditionally runs `drain_dma(None)`, `ppu.on_cpu_cycle()`, and
`apu_advance_one()`; only `mapper.notify_cpu_cycle()` is capability-gated.
The plan proposed an idle/capability early-out mirroring that gate.

**`perf annotate` redirected the whole investigation.** The hot instructions
inside `cpu_clock` are not bus bookkeeping — they are *floating point*:

```text
6.57%  vaddss  %xmm0,%xmm3,%xmm0
4.97%  vaddss  0x4144(%rbx,%rax,4),%xmm2,%xmm0
3.63%  vsubss  0x407c(%rbx),%xmm1,%xmm0
3.37%  vmulss  0x4494(%rbx),%xmm0,%xmm0
3.27%  vminss  %xmm0,%xmm1,%xmm1
```

That is the APU, inlined through `apu_advance_one`: the non-linear mixer's two
table lookups, then `Blip::add_sample`'s finite-check / clamp / delta, then the
phase advance. The DMA and PPU hooks the plan suspected are not the cost.

**Both textbook optimizations are already implemented.** The per-channel UI gain
already short-circuits at unity (`if g == 1.0 { v }` in `scale`), so the default
build performs no gain arithmetic; and the FIR scatter is already guarded by
`if delta != 0.0`, so a cycle whose mixed output is unchanged already skips the
32-tap band-limited scatter. What remains per cycle is genuinely structural: you
cannot know the delta is zero without computing the sample, and the phase
advance **is** the output-sample clock, which must run on every CPU cycle.

**A confounded probe, recorded because the trap is reusable.** The first attempt
stubbed `mixed` to a constant and measured a 6.9-7.9% saving — apparently a huge
win. It was an artifact: with `mixed` constant, LLVM proves `delta == 0.0` and
deletes the entire FIR scatter, so the probe measured *band-limited synthesis*,
not the mixer. Any probe that alters the value flowing into `add_sample` is
measuring the synthesis path, whatever it looks like it is measuring. (Same
class of error as the P2 contaminated A/B.)

**The clean measurement.** Add a SECOND, `black_box`ed mixer evaluation whose
result is discarded, leaving the value into `add_sample` — and therefore the FIR
— completely untouched. The delta is then exactly one mixer evaluation plus its
five `output()` reads:

| build | wall clock (900 frames, 3 runs) | delta |
|---|---|---|
| baseline | 20.35 / 19.21 / 19.16 s | — |
| + one discarded mixer call | 19.58 / 19.54 / 19.59 s | **+1.9%** |

So the mixer and its channel reads cost **≤1.9%** of frame time. That is the
hard ceiling on the one remaining lever — caching the mixed sample across cycles
whose channel outputs are unchanged — and it would be realised only by a cache
that never misses, before paying for the change-detection compare itself. It
also needs new per-instance state, which under the v2.2.3 schema audit
(`snapshot_schema_audit.rs`) is a save-state schema decision rather than a local
optimization.

**Below the 3% bar. No change adopted, nothing reverted** (the probes were
throwaway). `cpu_clock` stays 22.43% because that 22.43% is the APU doing the
work the accuracy model requires.

### v2.2.3 P3 — `emit_pixel` bounds-check elision (decision: REJECTED, reverted)

`Ppu::emit_pixel` is the third-hottest function in the emulator: **9.38% of
frame self-time** in a fresh `perf record --call-graph=dwarf -F 999` over the
committed 7-ROM PGO training corpus (`tick` 29.85%, `cpu_clock` 22.43%). It had
never appeared in this document's hot-path table.

**The hypothesis, from `perf annotate` rather than from reading the source.**
The hottest instructions were not the pixel math. They clustered at the *stores*:

```text
5.37%  mov  %esi,(%rdx,%rax,1)      <- the framebuffer store
5.33%  mov  0x1e8(%rdi,%rcx,4),%esi <- the rgba_lut load
3.13%  mov  0x40(%rdi),%rdx         <- reload the buffer base pointer
2.70%  lea  (%rsi,%r8,4),%rax       }
2.15%  mov  0x38(%rdi),%rdx         } bounds-check machinery
2.09%  cmp  %rdx,%rsi               }
```

`framebuffer: Box<[u8]>` and `index_framebuffer: Box<[u16]>` carry a **runtime**
length, so the optimiser cannot prove either index in range and emits a bounds
check plus a panic path for **every pixel** — 61,440 pixels per frame, twice
each. The BG-shifter block by contrast was already auto-vectorised
(`vpbroadcastw` / `vpand` / `vpcmpeqw` / `vmovmskps`) and is not the problem.

**The candidate.** Change both fields to fixed-size boxed arrays
(`Box<[u8; FRAMEBUFFER_LEN]>`, `Box<[u16; FRAMEBUFFER_PIXELS]>`) so the length
becomes a compile-time constant, and clamp the pixel index once with a
branchless `.min(FRAMEBUFFER_PIXELS - 1)` so the optimiser can discharge both
checks. The clamp can never bind — `emit_pixel` is only reached for a visible
dot (1..=256) on a visible scanline (0..=239) — so it is byte-identical, with a
`debug_assert!` pinning the invariant. Public surface unchanged (`framebuffer()`
still returns `&[u8]`).

**Measured: it makes the shipped default SLOWER.** Same-runner Criterion A/B, a
`git worktree` at HEAD benched against the working tree through one shared
target dir:

| workload | change | p | verdict |
|---|---|---|---|
| `nes_run_frame_nestest` (exact path) | −3.10% | 0.09 | no change — CI spans zero |
| `nes_run_frame_flowing_palette` (exact) | +0.06% | 0.83 | no change |
| **`nes_run_frame_nestest_fast`** | **+4.32%** | **0.00** | **regressed** |
| **`nes_run_frame_flowing_palette_fast`** | **+3.35%** | **0.02** | **regressed** |

The two `_fast` rows are the ones that matter: P1 promoted the fast dot path to
the **default**, so those are the shipped configuration. Both regressed
significantly. The only favourable number, −3.10% on the now-non-default exact
path, is not statistically significant.

**Reverted in full.** Two lessons worth keeping. First, `perf`'s self-time
*percentage* is not a verdict: after the change `emit_pixel` measured **10.26%**,
*higher* than the 9.38% before — a share, not a duration, and the program around
it had changed. Only the wall-clock A/B settles it. Second, removing a bounds
check is not free: the check was almost perfectly predicted, whereas the `cmov`
that replaces it sits on the store's address dependency chain, and narrowing
`Box<[T]>` to `Box<[T; N]>` shrinks `Ppu` and perturbs inlining and layout
decisions across the whole hot loop. The theoretically-cheaper code lost.

**What is left on the table.** The store cluster is real and still unaddressed;
what has been ruled out is *this* way of attacking it. A structural change —
making the framebuffer per-pixel (`[[u8; 4]]`) so the RGBA write and the
index write share one index and one check — is the untried option, but it
changes a public type consumed by the frontend, libretro, mobile and the tests,
so it is a deliberate API decision rather than a micro-optimization.

### v2.2.3 P2 — specialized idle-line dot path (decision: implemented, gated OFF)

A1 covers visible dots `1..=256` — 61,440 of the 89,342 NTSC dots (68.8%). The
other **27,902 (31.2%)** still walk the full general per-dot body, and the four
parts sum exactly: the non-`1..=256` dots of the 240 visible lines — dot 0 plus
257..=340, so 85 × 240 = **20,400**; post-render line 240 — **341**; vblank
lines 241..=260 — 20 × 341 = **6,820**; and pre-render line 261 — **341**.
(20,400 + 341 + 6,820 + 341 = 27,902 = 89,342 − 61,440.) P2
attacked the cheapest slice to prove correct: the **idle line** — post-render
line 240 plus every vblank line except the VBL-set line 241, 20 of 262 lines.

**Why it looked promising.** On an idle line the general body provably reduces
to three assignments; every other branch is gated on `render_line`, `visible`,
`pre_render`, `scanline == vblank_start_line()`, or a disturbance countdown.
So ~30 predicates were being evaluated to perform three stores.

**Implementation.** `Ppu::tick_idle_line_fast` behind a guard requiring a warm
classification cache (new derived `cached_idle_line` flag), plus all three
sub-dot countdowns idle. Byte-identical by construction on A1's terms, and
pinned by `fast_dotloop_diff` — extended here with
`idle_line_fast_path_matches_exact_under_vblank_io`, which drives a
purpose-built NROM that hammers `$2000`/`$2001`/`$2006`/`$2007` for the length
of vblank so the guard's fall-through arms are *exercised* rather than assumed
(vblank is when real software does its PPU I/O, so this is the case that
matters).

**Measured — same-session Criterion A/B, feature-off vs feature-on**, noise
floor ±0.7% (established by re-running an identical configuration against its
own baseline: all four workloads `p > 0.05`):

| Workload | Δ | |
|---|---|---|
| `nes_run_frame_nestest` | +0.16% | p = 0.23, no change |
| `nes_run_frame_nestest_fast` | +0.41% | p = 0.01, marginally worse |
| `nes_run_frame_flowing_palette` | **−1.31%** | small win |
| `nes_run_frame_flowing_palette_fast` | **−1.55%** | small win |

A ~1.5% win only on **rendering-disabled** content, neutral-to-slightly-negative
on the rendering-heavy case that dominates real play. The guard runs on every dot
A1's guard does not already claim — ~28k per frame, and all 89k when rendering is
off, which is exactly why the rendering-disabled demo is where it pays — to save
work on 6,820 idle dots whose general path was already short-circuiting on a
cached bool. The two roughly cancel.

**Decision: implemented, byte-identity proven, shipped OFF behind the
`ppu-idle-line-fast` cargo feature.** It does not clear the >3% bar, so it does
not displace the default — the same outcome the A2 SIMD blitter got, for the same
reason. It is **compile-time** rather than a runtime knob precisely because the
cost *is* the per-dot guard: a runtime flag would still pay it when disabled. With
the feature off the field, the guard, and the handler are all absent, so the
default build is unchanged.

> **Methodology trap — worth not repeating.** The first A/B reported P2 as a
> **+2% to +7.3% regression** and nearly got it deleted. That measurement was
> contaminated: the "off" baseline was produced by short-circuiting the guard with
> `if false && …` while leaving the new `cached_idle_line` field in the struct. The
> field changed `Ppu`'s layout, and the layout — not the guard — moved
> `flowing_palette` by ~3%. Only a genuine feature-off build (field absent)
> compares like with like. **When A/B-ing a change that adds a struct field, the
> baseline must not carry the field**; a `cfg` gate is the honest scaffold, an
> `if false` is not.
>
> **Also learned:** the three assignments in `tick_idle_line_fast` are, given the
> guard, provably redundant — deleting any one leaves the entire differential
> suite green (verified). They are kept anyway: "same assignments, same order" is
> checkable by reading twenty lines, whereas "these stores are dead" is a
> reachability argument that must be re-derived whenever the guard moves. Note the
> corollary for anyone extending this — a negative control that deletes a *dead*
> store proves nothing. The control that actually discriminates is one that breaks
> the *classification* (treating line 241 as idle makes all four differential
> tests fail, including the new torture case).

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
  (`crates/rustynes-mappers/src/m005_mmc5.rs`). PRG-ROM/RAM fetches at
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

### v1.7.0 "Forge" Workstream H7 — tier-1 perf (measure-first): no change adopted

H7 named two candidate micro-opts from `to-dos/plans/research/v1.7.0-detail-performance.md`
(T1.2 / T1.3). The contract is the standing one: adopt only on a **>3% Criterion-stable +
byte-identical** bar. Both were measured against fresh baselines (`full_frame` on
`nestest` + `flowing_palette`; `spectral` on `blip_square_wave` + `blip_silence`) and
**neither cleared the bar — nothing was adopted.** Findings:

- **T1.2 — unified-DMA cycle fast-path.** The research premise (that
  `unified_dma_cycle` "runs unconditionally every CPU cycle") does not hold for this
  codebase: the per-cycle dispatch already sits behind a `while bus.unified_dma_pending()`
  floor in `Cpu::read1` / `idle_tick`, and `unified_dma_pending()` itself leads with the
  `pending_dmc_dma` bool short-circuit, so a no-DMA cycle already costs only three
  bool-field reads and the heavy `unified_dma_cycle_impl` is out-of-line (cold). The
  release profile is `lto = "fat"` + `codegen-units = 1`, so the gate is already inlined
  across the crate boundary at the LTO stage. An explicit `#[inline]` on
  `unified_dma_pending` measured **"no change in performance detected"** on both
  `full_frame` benches (point estimates straddling zero, p > 0.05), as fat-LTO predicts.
  **REJECTED** — not byte-identity (the change was byte-identical) but the >3% bar.
- **T1.3 — BLEP phase-row cache.** This is the same optimization the v1.4.0 F2 pass
  already evaluated and dropped (see above). It cannot win here for two compounding
  reasons: (a) `Kernel::row()` is only called on signal **edges** (`if delta != 0.0`), not
  per sample — the `blip_silence` bench (zero edges) is within noise of `blip_square_wave`,
  so the row lookup is not the hot cost; the per-sample cost is the `phase += step`
  accumulate + the integrate/emit/`filter.process` loop, which a phase-row cache does not
  touch. (b) The kernel uses `PHASES = 256` and the NTSC step is
  `44100 / 1_789_773 ≈ 0.0246`, so the quantized phase bucket advances **~6.3 rows per
  sample** — consecutive `row()` calls essentially never share a bucket, giving a
  cache hit-rate near zero. A guarded `(bucket -> row_index)` cache (byte-identical by
  construction — same bucket maps to the same index maps to the same coeffs, and the APU
  determinism test passed) showed no stable win; under measurement it only added a branch.
  **REJECTED** — byte-identical, but < 3% (and structurally a non-win).

Measurement note for any follow-up: the bench host (20 logical cores, `powersave`
governor, turbo on) carries a large run-to-run variance — even pinned (`taskset`) the
`full_frame` benches floated ±~4% same-binary, i.e. the noise floor sits at the adoption
bar, so a sub-4% win is not Criterion-stably provable on this hardware. The PGO/BOLT gate
(`pgo.yml`) remains the project's authoritative >3%-Criterion + byte-identical promotion
path; H7 leaves it unexercised because there was no candidate to promote.

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
`run_bolt` inputs) and via **`workflow_call` from `release.yml`** on a version
tag.

> **v2.2.3 — the promoted binary now actually ships.** Until this release the
> workflow also triggered directly on a `v*` tag push "so a release can consider
> shipping the PGO binary" — but nothing ever consumed the result: the gate ran,
> promoted an artifact, and the release attached the plain build regardless. The
> measured win never reached a single user. `release.yml` now *calls* this
> workflow and replaces the `x86_64-unknown-linux-gnu` asset with the promoted
> binary. The standalone tag trigger was removed at the same time, so a
> hand-pushed tag no longer starts two 90-minute PGO runs.
>
> **Scope: linux-x86_64 only.** PGO training must *run* the instrumented binary,
> so every additional target needs its own native runner doing a full train
> cycle (~90 min each). macOS-aarch64 and Windows keep shipping plain release
> builds; extending PGO to them is a separate decision with a real cost, not a
> freebie.

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
   AccuracyCoin 141/141, `nestest` golden-log 0-diff, blargg/kevtris, the
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

A failed gate is informational — it never blocks a release. The determinism
stage carries a **step-level** `continue-on-error`, so a divergence produces a
`promotable=false` verdict rather than a dead job, and the asset-replacement
job (gated on `needs.pgo.outputs.promotable == 'true'`) is simply skipped,
leaving the plain-release asset that `build` already attached exactly where it
is.

Note that `continue-on-error` **cannot** be applied to the caller job: GitHub
does not allow it on a reusable-workflow `uses:` job (only `name`, `uses`,
`with`, `secrets`, `needs`, `if`, `permissions`), and `actionlint` flags it as a
syntax error. The tolerance therefore has to live inside the called workflow.
An *infrastructure* failure (runner died, `cargo-pgo` unavailable) still marks
the run red — deliberately, since a broken PGO pipeline should be visible — but
the release assets are correct either way.

**Sequencing.** `build` attaches the plain Linux archive in ~10 minutes; the PGO
job takes up to 90. For that window the release carries the plain binary and is
then upgraded in place via `gh release upload --clobber` under the *same* asset
name (so download links do not change shape). The alternative — withholding the
whole release until PGO finishes — was rejected: a complete, downloadable
release an hour sooner is worth more than avoiding an in-place swap.

#### BOLT (Linux post-link) — evaluated, gate DISABLED, deferred (v2.3.1)

**Status: BOLT builds and optimizes correctly; its promotion gate is disabled;
BOLT itself is UNMEASURED and deferred.** This section records why, because the
gate that used to sit here reported a number that was not real.

A second Linux-only `bolt` job runs after the PGO stage has promoted
(`needs.pgo.outputs.promotable == 'true'`), and **only** on an explicit
`workflow_dispatch` with `run_bolt: true`. (Before v2.2.3 its condition admitted
any non-dispatch event, which — once `release.yml` began calling this workflow —
would have fired BOLT on every release for an artifact nothing consumes.) It
still chains `cargo pgo bolt build` → re-train → `cargo pgo bolt optimize` and
uploads the resulting binary for manual evaluation. What it no longer does is
claim a speedup.

##### Three defects, each hiding the next

Getting BOLT to run at all took three dispatches, and each fix revealed the next
problem — the earlier failure had been masking it.

1. **The probe trusted a package manager.** It ran
   `apt-get install -y bolt` and set `have_bolt=true` on exit 0. On Ubuntu the
   package named `bolt` is the **Thunderbolt 3 device manager** — an unrelated
   project that owns the name. apt installed it, exited 0, and the stage then
   died on `Cannot find llvm-bolt`. A job whose entire contract is *skip cleanly
   when the tool is missing* failed the run instead. Fixed by locating the
   binary rather than inferring it.
2. **The probe checked the binary but not the runtime.** With the tool found,
   the stage ran the whole analysis — 12,816 functions, 356,056 instrumentation
   counters — and then died on
   `BOLT-ERROR: library not found: /usr/lib/libbolt_rt_instr.a`.
   `llvm-bolt --instrument` links a runtime archive it resolves relative to its
   own prefix, which Ubuntu's packaging does not put there. This is the same
   mistake one level down: verifying one necessary condition and treating it as
   sufficient. Fixed by verifying the runtime too and linking it into place.
3. **The gate measured a binary BOLT had never touched — and reported success.**
   With 1 and 2 fixed, BOLT genuinely instrumented and optimized for the first
   time, which finally exposed this. `cargo pgo bolt optimize` accepts **no
   cargo subcommand** (its usage is `[OPTIONS] [-- <CARGO_ARGS>...]`), unlike
   `cargo pgo optimize` on the PGO side which accepts `bench`/`test`. Both BOLT
   steps had been written by analogy with the PGO stage, so both were rejected:
   `unexpected argument 'bench' found` and `unexpected argument 'test' found`.

   The determinism step failed loudly. **The bench step did not:** it swallowed
   the error with `|| cargo bench …`, fell back to a plain non-BOLT build,
   computed a ratio against the plain baseline, and wrote it to the job summary
   as *"BOLT speedup vs plain release"*. It compared plain against plain and
   reported **success**. Had that ratio happened to land above 3%, the gate
   would have promoted a BOLT binary on a measurement containing no BOLT.

##### Why fixing the CLI would not have been enough

BOLT optimizes the **`rustynes` frontend binary**. The gate benches
`rustynes-core`'s `full_frame` criterion bench — a *separate* binary BOLT never
touches. Even spelled correctly, the step would measure something unrelated to
its subject. Measuring BOLT honestly requires a harness that runs **inside** the
optimized artifact (`frame_probe` built as part of it, emitting framebuffer
hashes for the determinism half), which is a design change rather than a fix.

Both steps are therefore **disabled** (`if: false`) with this reasoning inline in
the workflow, rather than patched. A gate that cannot measure its subject is
worse than no gate, and defect 3 shows this one could actively mislead.

##### Why it is deferred rather than rebuilt

- **PGO already captures the win: measured 6.43% faster and byte-identical on
  run 31067782333, promoted, and shipping as the Linux release asset.** That is
  the profile-driven layout benefit, taken at compile time.
- **BOLT's mechanism targets a bottleneck this workload does not have.** Its
  gains come from instruction-cache and iTLB pressure in large-code-footprint
  programs — compilers, databases, browsers. RustyNES's hot path is `Ppu::tick`,
  `cpu_clock`, `emit_pixel` and a few fetch helpers: comfortably L1i-resident.
  The v2.3.1 campaign established the loop is **issue-limited on work the
  accuracy model requires**, not front-end-limited.
- **The cost is several ~90-minute CI iterations plus new harness code**, for a
  speculative marginal gain on top of PGO, in BOLT's weakest case.

If it is ever revisited, start with the cheap half: BOLT-optimize `frame_probe`
rather than the frontend binary, run it, and compare against a plain build. That
answers the question without rebuilding the gate.

#### How to trigger

```bash
# Manual (from a checkout with the gh CLI):
gh workflow run PGO.yml                     # default 3600 frames/ROM, no BOLT
# `run_bolt=true` still builds + optimizes a BOLT binary and uploads it for
# manual evaluation, but its promotion gate is DISABLED — it produces no
# speedup claim (see "BOLT ... deferred" above).
gh workflow run PGO.yml -f frames=7200 -f run_bolt=true
# Or push a release tag — `release.yml` calls PGO and ships the promoted
# binary as the linux-x86_64 asset when the gate passes:
git tag v2.2.3 && git push origin v2.2.3
```

### v2.3.5 C0 — the APU had no throughput instrument (BUILT)

v2.3.1's per-source-file attribution recovered the **APU at 18.7% of frame time**
(`apu.rs` 8.4%, `frame_counter.rs` 2.1%, `blip.rs` 2.1%) — the largest core cost
never examined, invisible in a symbol profile because fat LTO inlines the whole
APU into `cpu_clock`. That invisibility is exactly why the ten-candidate v2.3.1
hot-path sweep never touched it.

Nothing about that cost could be adjudicated, because the instrument did not
exist. `rustynes-cpu` and `rustynes-ppu` each had a `*_throughput` bench; the APU
had only `spectral.rs`, which measures blip **quality**, not per-cycle cost.
`benches/apu_throughput.rs` fills the gap: the APU is ticked one CPU cycle at a
time, exactly as the bus drives it, 29,780 cycles to the NTSC frame so the number
is directly comparable against `full_frame`.

The baseline immediately produced the finding that motivated C1:

| workload | time | |
| --- | ---: | --- |
| `apu_tick_silent_frame` | **443.02 µs** | every channel disabled |
| `apu_tick_active_frame` | **547.40 µs** | all five channels running |
| `apu_tick_active_frame_with_external` | 635.31 µs | the expansion-audio path |

**81% of the active cost is paid with every channel disabled.** The per-cycle
overhead is very largely unconditional, which is what C1 went after.

*(These are the **corrected** figures. The first version of this bench called
only `Apu::tick`, where `LockstepBus::cpu_clock` actually runs
`set_canonical_cycle` then `tick_with_external` then `dmc_tick_end` then
`promote_dmc_pending_next` on every cycle. Both PR review bots caught that
independently, and it mattered: the missing end-of-cycle pair is ~23% of the
per-cycle cost, and correcting it changed C1's measured saving -- see below. The
instrument built to adjudicate C1 was itself wrong first, which is the same
lesson F14 recorded when the display metric measured the producer.)*

*(The third row is not a clean measurement: the bench varies the external sample
inside the timed loop, so it includes that arithmetic. It is reported for shape,
not for adoption arithmetic.)*

### v2.3.5 C1 — the default-configuration APU mix (decision: ADOPTED, with an unexplained magnitude)

**The change.** `tick_with_external` evaluated, every CPU cycle at 1.789 MHz, a
`gate(bit, v)` closure per channel branching on `channel_mask`, a `scale(bit, v,
max)` closure per channel branching on `channel_gain[bit] == 1.0`, a 6-wide `f32`
array copy, and a sixth mask test for the external sum — to produce a result
identical to the ungated mix. The determinism contract says the oracle and test
ROMs never clear a mask bit or change a gain, so at the shipped default every one
of those is the identity.

C1 hoists the is-this-the-default question out of the per-cycle body and takes a
branch with none of the machinery, the same shape as the PPU fast dot path. It is
a strict specialization: `mix()` receives exactly the same five arguments, so the
output is byte-identical **by construction**. `apu_default_mix_matches_the_gated_path`
pins it anyway across 2,048 **selected** level combinations with `to_bits()`
equality, and `a_non_default_mask_or_gain_still_takes_the_gated_path` pins that
the overlay still works once the configuration stops being default.

**Measured, two full replicates, both arms rebuilt each time** (`--save-baseline`
/ `--baseline`, not criterion's implicit last-run comparison):

| workload | replicate 1 | replicate 2 | clears >3%? |
| --- | ---: | ---: | --- |
| `nes_run_frame_nestest` | **−3.65%** | **−3.30%** | yes |
| `nes_run_frame_nestest_fast` | **−4.15%** | **−3.30%** | yes |
| `nes_run_frame_flowing_palette` | −0.53% | −0.66% | no |
| `nes_run_frame_flowing_palette_fast` | −0.62% (no change) | −1.14% | no |

APU-local, same change, re-measured on the corrected bench (both arms rebuilt
back-to-back on a quiet host, load average 0.7):

| workload | baseline | C1 | delta | absolute |
| --- | ---: | ---: | ---: | ---: |
| `apu_tick_silent_frame` | 443.02 µs | 401.43 µs | **−9.39%** | −41.6 µs |
| `apu_tick_active_frame` | 547.40 µs | 507.74 µs | **−7.24%** | −39.7 µs |
| `apu_tick_active_frame_with_external` | 635.31 µs | 597.33 µs | −5.98% | −38.0 µs |

The relative win is smaller than the −15.0% / −14.3% first reported, and that
is expected: the corrected bench's denominator now includes the end-of-cycle DMC
pair, which C1 does not touch. The absolute saving also moved (~61 µs to
~40 µs) -- same source change, so the difference is the loop's own inlining
and register pressure shifting once two more calls per cycle are present.

**What does not add up, stated rather than smoothed over.** The APU does
identical work in both `full_frame` workloads — a frame is 29,780 CPU cycles
whether or not rendering is enabled — so the *absolute* saving should match. It
does not:

| workload | absolute saving |
| --- | ---: |
| `nestest` | **~124 µs** |
| `flowing_palette` | **~16 µs** |

An 8× difference from a component doing identical work, and the nestest saving is
**roughly three times the ~40 µs** the corrected standalone APU bench
attributes to the change. Correcting the bench made this gap *wider*, not
narrower: the first version of this section reported a 2× gap against a
~61 µs standalone saving, and the honest correction moves the number the
wrong way for the explanation. It is recorded that way rather than quietly
re-fitted. An
optimization cannot save more than the component it touches costs. So the win is
**not purely the closure removal**; the likely mechanism is an LTO / register-
allocation knock-on in `cpu_clock` for nestest's instruction mix — which is the
same inlining behaviour that hid the APU from the symbol profile to begin with.

Adopted because it clears the bar on the headline workload, reproduces across two
replicates (the second with arm B under *higher* load, which biases against it),
and is byte-identical. **Not** adopted on the claim that removing five closures
buys 3.3%: that story is contradicted by the project's own numbers, and the
honest position is that the mechanism is only partly understood.

Prediction recorded and wrong, for the record: this campaign expected the
shorter, rendering-heavy `flowing_palette` frame to show the *larger* relative
win, since the APU should be a bigger fraction of it. It showed essentially none.

### Workstream D (the APU at 18.7% of frame time) — CLOSED, v2.3.6

**Status: closed. Do not reopen as a per-cycle-gating campaign.** The 18.7%
figure stands — it is a real, correctly-measured subsystem attribution from
v2.3.1, recovered only because fat LTO inlines the APU into `cpu_clock` and hides
it from a symbol profile. What is settled is that the figure is **not recoverable
by gating per-cycle bookkeeping**, which is the only strategy this workstream
ever tried.

| lever | target | outcome |
|---|---|---|
| C1 | default-configuration mix specialization | **ADOPTED** v2.3.5, −3.3% to −4.2% |
| D1 | DMC end-of-cycle tick, ~23% of per-cycle cost | REJECTED — no measurable effect |
| D3 | cached C1 gain predicate | REJECTED — sign flipped between runs |
| D6 | four unconditional `length.reload()` stores | REJECTED — no measurable effect |
| D5 | `add_sample` finite-check hoist | DECLINED on inspection — swaps one per-cycle branch for another |
| D2 | `FrameCounter::tick` countdown | not measured; ceiling ~2.1% of frame |
| D4 | `Pulse::muted()` caching | not measured; would add derived state to `Pulse` |

**The generalisation, which is the reason to close rather than continue.** Three
levers — D1, D3 and D6 — were measured and all three produced nothing, for one
shared mechanical reason (C1 was also measured, and is the one that paid).
Under `lto = "fat"` with `codegen-units = 1`, the code these levers guard is
already inlined into `cpu_clock`, common-subexpression elimination has
already merged its repeated loads, and the branches being elided are
always-not-taken and therefore perfectly predicted. Replacing predictable
not-taken branches with an equivalent count of
loads and a predicate is arithmetically a wash. **"This work is inert on almost
every cycle" predicts a win only if the work is actually executed** — and under
fat LTO with perfect prediction it largely is not.

D2 and D4 are the same shape as D1, D3 and D6, so the prior for them is now a
null, not an unknown. Measuring them would cost two more quiet-host A/B pairs to
confirm what three data points already indicate, and D4 additionally carries a
`snapshot_schema_audit` registration and a recompute-on-restore obligation. They
are left unmeasured deliberately, not overlooked.

**What would justify reopening**, none of which is a variation on the above:

- A **structural** change to how the APU is clocked or how its output is
  synthesized, rather than gating around the existing per-cycle body. C1 is the
  only lever that ever paid, and it worked by specializing a whole code path, not
  by skipping bookkeeping.
- A profile taken on **different hardware or a different codegen configuration**,
  where the fat-LTO / perfect-prediction premise does not hold.
- A measurement instrument with better than the ±1-2% resolution this host
  provides, which is the floor that made three of these calls "not demonstrated"
  rather than "demonstrably zero".

### v2.3.6 D1 + D6 — DMC-idle fast path and length-reload early-out (decision: REJECTED, reverted)

**The changes.** D1 and D6 were measured together as one adoption unit, because
each alone was expected to be sub-threshold and both target the same per-cycle
bookkeeping.

**D1** — `Apu::dmc_tick_end` runs on every CPU cycle at 1.789 MHz and was the
largest untouched component of the APU's 18.7% of frame time, at roughly 23% of
per-cycle cost. Reading it against what each block requires, exactly two things
in it are unconditional hardware: the byte-timer clock and the get/put parity
flip. Everything else — the implicit-abort kill, the consume-edge transfer, the
load-delay countdown, the re-enable period block, the edge-arm suppression, the
reload arm, the `cannot_run` decrement, the delayed-`$4015` countdown — is DMA
corner-case bookkeeping gated on some piece of DMC state being non-idle, and on a
cartridge not running a DMC sample, all of it is inert on every cycle. D1 added an
idle guard between the byte-timer clock and the rest, plus a dedupe of two
identical `bits_remaining()` reads.

Deliberately **not** the maintained summary flag the v2.3.4 workstream note
proposed. A cached flag would need updating at each of the ~30 mutation sites in
`apu.rs` and `dmc.rs`, and one missed site is a silent accuracy regression in the
least testable corner of the emulator. The guard instead reads the same fields
the skipped blocks branch on, each term the negation of the condition that makes
one block act, so byte-identity is a structural property rather than a claim.

**D6** — `LengthCounter::reload` is called four times per CPU cycle. With nothing
pending it reduced to one predictable not-taken branch plus an *unconditional
store* of `new_halt` over an already-identical `halt`: four redundant stores per
cycle, each writing a byte back onto itself in a different channel's cache line.
D6 replaced that store with a compare.

**Adjudicated with `scripts/perf/ab_check.sh --base origin/main`, two independent
runs.** Host qualified first: the self-hosted PR-review runner idle, load 2.06
across 20 cores, no cargo or rustc running.

| workload | run 1 candidate | run 1 control | run 2 candidate | run 2 control |
|---|---:|---:|---:|---:|
| `nes_run_frame_nestest` | −0.24% (p = 0.50) | **−4.27%** | +0.99% | +1.37% |
| `nes_run_frame_flowing_palette` | −2.39% | **−3.37%** | +1.12% | +0.53% |
| `nes_run_frame_nestest_fast` (shipped default) | −2.91% | **−3.72%** | +1.26% | +1.13% |
| `nes_run_frame_flowing_palette_fast` (shipped default) | −3.81% | **−3.73%** | +0.58% | +0.91% |

All entries p = 0.00 unless noted. **Run 1's order-bias control failed on every
workload** at −3.4% to −4.3%, so its candidate column is not interpretable — the
apparent −3.81% on `flowing_palette_fast` is entirely accounted for by a −3.73%
drift measured with the reference benched against *itself*.

The mechanism is visible in the run-1 log and is worth recording, because it will
recur: the reference is benchmarked **immediately after a 44.9 s fat-LTO compile
across all cores**, so it measures on a hot, frequency-throttled machine while
the candidate runs once thermals have settled. Run 2 removed the confound — the
reference build was cached, and `AB_MEASUREMENT_TIME=25` widened the window —
which took the control's drift from ~4% to ~1%.

**Rejected**, on grounds that are independent of each other:

1. **The sign flips between independent runs**, negative throughout run 1 and
   positive throughout run 2. Mixed signs across runs mean the effect is not
   reproducible at all.
2. **Neither order-bias control was clean**, and run 1's failed outright.
3. **In the well-conditioned run the candidate tracks the drift.** Run 2's
   candidate and control agree to within a few tenths of a percent on every
   workload; subtract the drift and the effect is indistinguishable from zero.

**Why a null is the expected result here, in hindsight.** Release builds use
`lto = "fat"` with `codegen-units = 1`, so `dmc_tick_end` is already inlined into
`cpu_clock` and LLVM can apply common-subexpression elimination to the field
loads across the blocks the guard skips. The branches D1 elides were
always-not-taken and therefore perfectly predicted, so trading ~9 predictable
not-taken branches for 9 loads, an OR-reduction and one branch is arithmetically
a wash. D6 is the same shape at
smaller scale: a not-taken branch plus a store-to-same-value, against a load and
a compare. **"Inert on almost every cycle" predicts a large win only if the work
is actually executed; under fat LTO with perfect prediction it largely is not.**
That reasoning applies equally to D2, D4 and D5 and should temper their
expectations rather than being rediscovered three more times.

**Reverted rather than kept as a simplification.** D1 adds a 40-line guard
predicate whose correctness argument is a nine-term case analysis, sited in the
DMA timing that the `$500`/`$520`/`$540` implicit-abort battery exists to pin.
Carrying that in the least testable part of the emulator for an effect
indistinguishable from zero is a bad trade — the same call v2.2.3 P3 and v2.3.6
D3 made.

**What this does not say.** "Not measurable here" is not "no difference". This
host resolves roughly ±1-2%, so a sub-1% effect is invisible to it. The honest
claim is that D1 and D6 have no *demonstrated* benefit. Byte-identity separately
**was** established and is not in question: `dmc_dma` 1/1, `dma_timing_pin` 11/11,
the APU unit suite 143/143, AccuracyCoin 141/141 on the authoritative RAM decoder,
nestest 0-diff, and a full `--features test-roms` sweep across 127 test binaries.

**Remaining from the v2.3.4 Workstream C list.** **D5 was declined without
measurement**, on inspection rather than on a benchmark: `add_sample` cannot know
whether expansion audio is live, so keying the finite-check on the caller's
`external` argument replaces one per-cycle branch with another — a check-for-check
trade, not an elimination. **D2** (`FrameCounter::tick` as a countdown rather than
a 6-arm match) and **D4** (`Pulse::muted()` caching) remain unmeasured; note D4
would add derived state to `Pulse`, incurring the same `snapshot_schema_audit`
registration and recompute-on-restore obligations that counted against D3.

### v2.3.6 D3 — caching the C1 fast-path gain predicate (decision: REJECTED, reverted)

**The change.** v2.3.5's C1 fast path tests
`mask == CHANNEL_MASK_ALL && channel_gain == CHANNEL_GAIN_UNITY` once per CPU
cycle. The second half is a 6-wide `f32` array comparison evaluated **1.789
million times a second** to answer a question that can only change when a user
drags a mixer slider. D3 cached it in a `gain_is_unity: bool`, reducing the
per-cycle test to a `u8` compare plus a `bool` load. Byte-identical by
construction: same predicate over the same array, same branch taken.

**Adjudicated with `scripts/perf/ab_check.sh --base <D3^> --bench nes_run_frame_nestest`,
two independent runs, quiet host.**

| workload | run 1 | run 2 |
|---|---:|---:|
| `nes_run_frame_nestest` | **+1.72%** (p = 0.00) | **−0.91%** (p = 0.01) |
| `nes_run_frame_nestest_fast` (shipped default) | −0.45% (p = 0.31) | −0.70% (p = 0.05) |
| order-bias control, `_fast` | **−2.53% (p = 0.00) — FAILED** | clean |

**Rejected**, on three independent grounds, any one of which suffices:

1. **The sign flips between independent runs** on `nes_run_frame_nestest`:
   +1.72% then −0.91%, both nominally significant. Mixed signs are a rejection,
   never something to average — and mixed signs *across runs* mean the effect is
   not reproducible at all.
2. **Run 1's order-bias control failed** (`_fast` drifted −2.53% from position in
   the run alone), so run 1's candidate numbers carry at least that much
   systematic error and its small result is not interpretable.
3. **The shipped `_fast` variant never moved significantly** (p = 0.31, then
   p = 0.05). `fast_dotloop` has been default-on since v2.2.3, so a change that
   does not move `_fast` moves nothing a user runs.

This is the shape v2.3.1 G2 recorded: a textbook single-run result that
evaporates on re-run. A third run was not pursued — even the most favourable
reading is under 1%, and the change is not free: the cache is derived state that
must be kept in sync with `channel_gain`, which cost a dedicated desync test and
an entry in `snapshot_schema_audit`. Two standing obligations for an effect
indistinguishable from zero is a bad trade, so the code was reverted rather than
kept as a simplification.

**What this does not say.** "Not measurable here" is not "no difference". The
instrument's resolution on this host is roughly ±1-2%, so a sub-1% effect is
invisible to it. The honest claim is that D3 has no *demonstrated* benefit, and
the project does not carry core state on undemonstrated benefit.

**Still open from the v2.3.4 Workstream C list** at the time D3 was written: D1,
D2, D4, D5, D6. **Superseded** — D1 and D6 were subsequently measured and
rejected, and D5 declined on inspection; see the §D1 + D6 section above for the
numbers and for why a null was the expected result under fat LTO. D2 and D4
remain unmeasured.

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

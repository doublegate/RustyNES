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

### v2.3.3 F1 — sizing the frontend items before building any of them

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
and should be argued on memory, not on frame time, if it is revisited.

**Also found, not yet fixed:** `pacing_mode = "vrr"` on a display that is not
actually variable-refresh degrades to `presented_mean` 49.74 ms (~20 fps) with
1170 dropped frames in 40 s, and has no sustained-miss fallback of the kind
display-sync carries.

**Adopted from this investigation:** the `cost`/`wait` metric split, and the
pacing rework it made legible — see **v2.3.3 F2** below.

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

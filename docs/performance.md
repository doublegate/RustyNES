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
other **27,902 (31.2%)** still walk the full general per-dot body: visible dots
257..=340 (20,400), vblank lines 241..=260 (6,820), pre-render (341). P2
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

#### BOLT (Linux post-link, optional)

A second Linux-only `bolt` job runs behind the **same > 3% + byte-identical
gate**, only after the PGO stage has already promoted (`needs.pgo.outputs.promotable
== 'true'`), and **only** on an explicit `workflow_dispatch` with `run_bolt:
true`. (Before v2.2.3 its condition admitted any non-dispatch event, which —
once `release.yml` began calling this workflow — would have fired BOLT on every
release, adding ~90 minutes for an artifact nothing consumes, since the release
ships the PGO binary and not the BOLT one.)
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
# Or push a release tag — `release.yml` calls PGO and ships the promoted
# binary as the linux-x86_64 asset when the gate passes:
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

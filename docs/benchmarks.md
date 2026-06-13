# Benchmarks — R1 master clock (engine v2.0 scheduler A/B vs. the legacy integer-lockstep)

> **Engine-lineage note.** This document is a reproducible benchmark record
> captured during the internal engine development line that produced RustyNES
> v1.0.0. Its `v2.0.x` / `v2.8.0` markers are engine-line version anchors for
> the specific captured runs, **not** RustyNES releases of their own — RustyNES
> ships at v1.0.0. The measurements (R1 master-clock scheduler, the performance
> pass) all describe the technology shipping in v1.0.0.

> **Engine v2.0.1 note:** the legacy integer-lockstep scheduler was **removed** in
> v2.0.1 — R1 is now the only path. The A/B below was captured at v2.0.0 (when
> the legacy path was still reachable via `--no-default-features`) and is retained
> as the **historical justification** for keeping R1 as the sole scheduler: it
> quantifies what the removed path cost (R1 is +6–8% frame time for +5.76 pts of
> accuracy) and shows the cost is bus-side, not the CPU core. The R1 columns are
> the current numbers; the `--no-default-features` reproduce step no longer
> selects a second scheduler.


This document is the full, reproducible benchmark record for the **v2.0.0**
release, in which the **R1 master-clock scheduler became the default build**
(AccuracyCoin 100.00%) and the v1.7.0 **integer-lockstep scheduler** stayed
reachable for A/B via `--no-default-features` (AccuracyCoin 94.24%). It answers
one question quantitatively: *what does the +5.76-point accuracy gain cost in
throughput?*

Short answer: **~6–8% of headless frame time**, and the cost is entirely
bus-side accuracy work (master-clock PPU catch-up + per-cycle unified-DMA
dispatch) — the R1 CPU cycle-loop itself is **leaner** in isolation. Both
configurations clear the real-time wall by 4.25–7.1×.

For the *targets* and the optimization plan, see
[`docs/performance.md`](performance.md); this file is the *measurements*.

---

## 1. The two configurations under test

| Config | Build | Scheduler | AccuracyCoin | Role |
|---|---|---|---|---|
| **R1 (default)** | `--features mc-r1-full-cpu` | master clock: `u64` master-clock timebase, region `cpu_divider` per cycle, PPU caught up to `master_clock − ppu_offset`, per-cycle unified DMA dispatch | **100.00%** (139/139) | the shipped default scheduler |
| **Legacy (A/B)** | `rustynes-core` default features (no R1) | integer-lockstep: one PPU dot per `tick_one_dot`, CPU every 3rd dot, batched DMA spans | 94.24% (8 fail) | the v1.7.0 path, reachable via a consumer's `--no-default-features` |

The split is clean because the R1 umbrella `mc-r1-full-cpu` is **not** in
`rustynes-core`'s own default — it lives in the shipping consumers' defaults
(`rustynes-frontend`, `rustynes-test-harness`). So a single `--features mc-r1-full-cpu`
flag on a `rustynes-core` bench toggles exactly the scheduler, with nothing else
changing. (See the Phase-7/F "Option X" note in `rustynes-core/Cargo.toml`.)

---

## 2. Methodology

- **Host:** Intel Core i9-10850K @ 3.60 GHz (10C/20T, Comet Lake), CachyOS
  Linux, `powersave` cpufreq governor.
- **Toolchain:** rustc 1.86.0 (pinned in `rust-toolchain.toml`), release
  profile `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`,
  `panic = "abort"`, `overflow-checks = false` (the `bench` profile inherits
  `release`).
- **Harness:** [Criterion](https://github.com/bheisler/criterion.rs), each
  bench `[[bench]] harness = false`. `full_frame` ran at 60 samples /
  2 s warm-up / 6–8 s measurement; the micro-benches at 100 samples /
  1 s warm-up / 3–4 s measurement.
- **A/B method:** Criterion `--save-baseline legacy` then `--baseline legacy`
  on the R1 run, so the reported Δ and `p`-value are a same-host, back-to-back
  comparison.
- **Caveat — absolute vs. relative.** On a shared desktop under the
  `powersave` governor Criterion flagged 16–35% outliers, so absolute
  millisecond figures carry ~±3% host noise; **trust the deltas** (same host,
  consecutive runs, `p < 0.05`), not the absolute values across machines. The
  numbers are *hardware-specific* — replicate locally before treating any
  delta as a regression. Date captured: 2026-06-10.

### Reproduce

```bash
# Headline scheduler A/B (the only bench that exercises the whole scheduler)
cargo bench -p rustynes-core --bench full_frame -- --save-baseline legacy
cargo bench -p rustynes-core --bench full_frame --features mc-r1-full-cpu -- --baseline legacy

# CPU cycle-loop micro-bench (R1 substrate vs. legacy)
cargo bench -p rustynes-cpu  --bench cpu_throughput -- --save-baseline legacy
cargo bench -p rustynes-cpu  --bench cpu_throughput --features mc-r1-substrate -- --baseline legacy

# Scheduler-invariant controls (identical across both configs by construction)
cargo bench -p rustynes-ppu     --bench ppu_throughput
cargo bench -p rustynes-mappers --bench mapper_dispatch
cargo bench -p rustynes-apu     --bench spectral
```

### Reference frame deadlines

Cross-checked against the `CPU_HZ` constants in `crates/rustynes-apu/src/blip.rs`
(`CPU_HZ_NTSC = 1_789_773`, `CPU_HZ_PAL = 1_662_607`):

| Region | CPU clock | Cycles/frame | Refresh | **Frame deadline** |
|---|---|---|---|---|
| NTSC / Dendy | 1.789773 MHz | 29,780.5 | 60.0988 Hz | **16.639 ms** |
| PAL | 1.662607 MHz | 33,247.5 | 50.007 Hz | **19.997 ms** |

All "× realtime" / "fps headless" figures below are against the **NTSC**
16.639 ms deadline (the tighter of the two).

---

## 3. Headline — `full_frame` (end-to-end `Nes::run_frame`)

This is the only bench that drives the complete scheduler: CPU per-cycle bus
interleaving + PPU dot scheduler + APU emit + mapper dispatch + framebuffer
write. It is therefore *the* R1-vs-legacy number.

| Workload | Legacy | R1 (default) | Δ (p < 0.05) |
|---|---|---|---|
| `nes_run_frame_nestest` (near-static menu) | **3.623 ms** | **3.919 ms** | **+8.14%** |
| `nes_run_frame_flowing_palette` (render-heavy) | **2.338 ms** | **2.486 ms** | **+6.32%** |

Derived real-time headroom against the 16.639 ms NTSC wall:

| Workload · config | × realtime | fps headless | Emulated CPU clock |
|---|---|---|---|
| nestest · legacy | 4.59× | 276 fps | 8.22 MHz |
| nestest · R1 | 4.25× | 255 fps | 7.60 MHz |
| flowing_palette · legacy | 7.12× | 428 fps | 12.74 MHz |
| flowing_palette · R1 | 6.69× | 402 fps | 11.98 MHz |

> `flowing_palette` is the representative game-load figure (continuous palette
> rewrites + a full BG every frame); `nestest` sits on a near-static menu and
> paradoxically costs *more* here because its idle loop spends more cycles in
> CPU-side polling that R1 traverses per-cycle. Both clear the wall with room
> to spare even in the worst (nestest · R1) case.

**Reading:** R1 trades **~6–8% frame time** for the move from 94.24% → 100.00%
AccuracyCoin. On this 2020 desktop the default build still runs the headless
core at 4.25–6.7× real time; on PAL's 20 ms deadline the margins are ~20%
larger again.

---

## 4. Decomposition — the +6–8% is bus-side, not the CPU core

`cpu_throughput` runs `Cpu::step` against a synthetic `NopBus` (every read
returns `0xEA`, no PPU/APU/DMA). It isolates the **CPU cycle loop** with the R1
substrate (`mc-r1-substrate`) on vs. off.

| Bench | Legacy | R1 substrate | Δ (p < 0.05) |
|---|---|---|---|
| `cpu_nop_step_x1000` | 1.078 µs | **0.701 µs** | **−34.6%** |
| (derived) cycle throughput | 1.86 Gcyc/s | **2.85 Gcyc/s** | — |

The R1 CPU loop is **35% faster** in isolation: the run-to-timestamp model
batches `master_clock` advancement instead of firing a per-cycle
`on_cpu_cycle` callback with the legacy bookkeeping. (This is a synthetic
upper bound — pure 2-cycle NOPs, minimal cache pressure — not a representative
workload; its value is the *direction*, not the absolute.)

Since the CPU core gets *cheaper* under R1 yet the full frame gets *dearer*,
the entire +6–8% lives on the **bus side**:

1. **Master-clock PPU catch-up** — `LockstepBus::run_ppu_to` advances the PPU
   to `master_clock − ppu_offset` with the double catch-up, replacing the
   legacy "tick one dot, every 3rd advances CPU" lockstep.
2. **Per-cycle unified DMA dispatch** — `unified_dma_cycle` (the TriCNES-style
   single dispatch table) runs every CPU cycle, replacing the legacy
   batched-span DMA drivers.

That's the accuracy machinery doing real work; it is the *point* of R1, not
overhead to optimize away.

---

## 5. Scheduler-invariant controls

These benches do not depend on the scheduler choice (PPU dot loop, BLEP audio
synthesis, mapper dispatch are all identical across both configs by
construction). Run once; reported as the shared v2.0.0 baseline. Their stability
is itself a result — it confirms the R1 change is localized to the scheduler.

| Bench | Crate | Result | Notes |
|---|---|---|---|
| `ppu_tick_one_frame` (89,342 dots) | `rustynes-ppu` | **497 µs/frame** | synthetic `PpuBus` (`0xA5` reads); PPU dot loop alone ≈ 33× realtime |
| `blip_square_wave_0_1s_ntsc` | `rustynes-apu` | **611 µs** | 0.1 s NTSC audio (≈178,977 samples) → ≈164× realtime; ≈3.4 ns/sample |
| `blip_silence_0_1s_ntsc` | `rustynes-apu` | **593 µs** | drain-path baseline |

### `mapper_dispatch` — `Box<dyn Mapper>::cpu_read`, per 1024 reads

| Mapper | Time / 1024 reads | Notes |
|---|---|---|
| NROM (0) | 1.67 µs | baseline; no banking logic |
| MMC1 (1) | 1.51 µs | serial 5-write shift register (read path cheap) |
| MMC3 (4) | 1.51 µs | A12 filter is on the *write* path |
| MMC5 (5) | 2.28 µs | ExRAM mode + multiple bank slots in the read path |
| M34 (34) | 1.57 µs | BNROM / NINA-001 variant detection |
| FME-7 (69) | 2.78 µs | per-CPU-cycle IRQ counter tick — the dearest |

Even the most expensive (FME-7, 2.78 µs/1024 ≈ 2.7 ns/read) is **< 1% of frame
cost**, so dynamic dispatch is not the bottleneck — the
`Box<dyn Mapper>`-vs-monomorphized decision stays as recorded in
[ADR-0001](adr/0001-mapper-dispatch.md). Trigger to revisit: a `perf` profile
showing dispatch in the top 3 *and* a ≥5% `full_frame` regression attributable
to it.

---

## 6. Conclusions

- **R1 is the correct default.** 100% accuracy for a 6–8% frame-time cost that
  still leaves 4.25–6.7× real-time headroom on a 2020 desktop. The cost is
  bus-side accuracy work, cleanly attributed.
- **Legacy is the throughput option** via `--no-default-features` (~6–8%
  faster) for embedders who accept 94.24% accuracy.
- **The CPU core is not a concern** — it got faster under R1. Any future
  optimization pass should target `run_ppu_to` and `unified_dma_cycle`, not
  `Cpu::step`.
- **Absolute frame times drifted** vs. the v0.9.0 baseline in
  `docs/performance.md` (3.62 vs. 2.06 ms nestest), but that is host/toolchain
  drift on a different machine (Ryzen 9 then, i9-10850K now); it is **not** a
  branch regression — the v0.9.0 table predates R1 and was captured on other
  hardware. The CI `bench` job guards the property that matters (headless
  production stays comfortably real-time) with an absolute 10 ms ceiling.

## 7. CI regression gate

`scripts/bench_regression_check.sh` runs the `full_frame` benches and fails if
headless frame production exceeds an absolute **10 ms** ceiling (60% of the
16.639 ms NTSC deadline), wired as the `bench` job in `.github/workflows/ci.yml`.
The absolute ceiling (rather than a tight percentage gate) is deliberate: shared
runners vary tens of percent run-to-run, so a percentage gate flakes; the
ceiling instead protects "stays comfortably real-time" and trips only on a gross
(~3×) regression. For the tight ~5% comparison, use the local Criterion
baselines in §2.

---

## 8. Performance-pass Phase 0 baseline (engine line, 2026-06-12)

Re-measurement on the same host (i9-10850K, CachyOS, rustc 1.86, `powersave`
governor — kept identical to the §2 record so the columns stay comparable) at
the start of the v2.8.0 "optimized performance" work, plus the **new
`snapshot_restore` bench** (`crates/rustynes-core/benches/snapshot_restore.rs`)
that closes the never-measured "save-state ≤ 1 ms" target from
`docs/performance.md` and budgets the run-ahead feature.

### `full_frame` re-measurement

| Workload | v2.0.0 record | v2.8.0 Phase 0 | drift |
|---|---|---|---|
| `nes_run_frame_nestest` | 3.919 ms | **3.988 ms** [3.977–4.001] | +1.8% (host noise) |
| `nes_run_frame_flowing_palette` | 2.486 ms | **2.659 ms** [2.649–2.671] | +7.0% (host noise / interim releases) |

Both within the documented powersave-governor variance envelope; these are
the Phase 0 reference points all v2.8.0 optimization landings compare
against (`cargo bench -p rustynes-core --bench full_frame -- --save-baseline
v2.8.0-phase0`).

### `snapshot_restore` (NEW)

| Bench | flowing_palette (NROM) | MMC3 (`M4_P128K_CR8K`) |
|---|---|---|
| `nes_snapshot_*` (incl. THM thumbnail) | **36.0 µs** | **36.9 µs** |
| `nes_restore_*` | **124.3 µs** | **124.3 µs** |
| `nes_runahead_budget_*` (snapshot + 1 hidden frame + restore) | **2.743 ms** | **2.612 ms** |

**Readings:**

- Snapshot + restore round-trip is **~160 µs** — the `≤ 1 ms` target is met
  with 6× margin *even including* the 61 KiB thumbnail built per call.
- **Run-ahead N=1 is affordable**: one visible frame pays its own
  `run_frame` (2.7–4.0 ms) + the budget probe (≈ 2.6–2.7 ms) ≈ **5.3–6.7 ms**
  total against the 16.639 ms NTSC wall. N=2 (~9 ms worst case) also fits on
  this host.
- Restore (~124 µs) costs ~3.4× snapshot: it re-parses the section container
  and walks sections twice (bus pass + CPU pass). Not load-bearing for N=1.

### Phase 3 fast path (landed 2026-06-12)

`snapshot_core_into` (no THM thumbnail, caller-owned reused buffer) +
`restore_quiet` (no rewind-ring clear) — what run-ahead, the netplay
save-state ring, and the per-frame rewind capture now actually pay:

| Bench | flowing_palette (NROM) | MMC3 (`M4_P128K_CR8K`) |
|---|---|---|
| `nes_snapshot_core_into_*` | **14.6 µs** (was 36.0 — 2.4×) | **15.2 µs** |
| `nes_restore_quiet_*` | **124.8 µs** (parse-dominated, unchanged) | **125.7 µs** |
| `nes_runahead_budget_*` (fast path) | **2.887 ms** | **2.602 ms** |

Allocation churn removed along the way: the per-frame rewind capture no
longer builds a thumbnail-carrying ~320 KiB snapshot allocation (reused
buffer), the rewind XOR-delta scratch is reused, and netplay's resync no
longer clones the checkpoint (~250 KiB) per rollback nor allocates per
replayed frame (ring-slot reuse).

### Phase 4 — core optimization landings (2026-06-12)

Cumulative `full_frame` deltas vs the Phase 0 baseline (each landing
byte-identity-gated; the full `--features test-roms` suite + AccuracyCoin
100% re-verified at each step):

| Landing (cumulative) | nestest | flowing_palette |
|---|---|---|
| Phase 0 baseline | 3.988 ms | 2.659 ms |
| + mapper capability flags | 3.792 ms (−4.0%) | 2.379 ms (−10.1%) |
| + `emit_pixel` RGBA LUT + single 4-byte store | 3.731 ms (−5.5%) | 2.316 ms (−12.5%) |
| + `lto = "fat"` release profile | **3.315 ms (−16.0%)** | **1.958 ms (−26.0%)** |

- **Mapper capability flags** (`Mapper::caps()` cached on the bus): skips
  the per-CPU-cycle virtual fan-out (`notify_cpu_cycle` / `mix_audio` + f32
  convert / `notify_frame_event` / `irq_pending`) on boards whose hooks are
  the default no-ops — mechanically derived per mapper, pinned by
  `mapper::caps_tests`. NROM-class boards skip all four; IRQ mappers keep
  cycle+irq and still shed the audio-side pair.
- **`emit_pixel` LUT**: 512-entry `(emphasis << 6) | color` → RGBA8 table
  built from the same pure `palette_color_to_rgba` (exhaustively-tested
  equal), plus one bounds-checked 4-byte slice store instead of four
  indexed stores — per emitted pixel (61,440/frame).
- **Fat LTO**: `lto = "thin"` → `"fat"` in the release profile — the
  single biggest codegen win (~10-13 points). Slower release compiles,
  identical bytes (AccuracyCoin re-verified under the release profile).
- **Auto-vectorization unblocks** (Phase 4b): the BLEP 32-tap kernel
  scatter now runs as at most two contiguous SAXPY runs instead of
  per-tap ring-masked indexing (LLVM vectorizes it; per-slot mul-then-add
  order unchanged = bit-identical audio), and the rewind XOR-delta is
  zip-based (bounds-check-free integer SIMD).
- Also: the NTSC post-pass bind group is built once at filter construction
  instead of per frame, and `scripts/pgo/run.sh` + the `pgo_trainer` bin
  land the Mesen2-style PGO recipe (measure-first; CI adoption gated on
  >3%).

The deferred sprite-eval/`tick_oam_bus` batch fast paths and the
read/write-split hoists were evaluated and skipped: the former mutate
observable per-dot state (medium risk for unproven gain), the latter are
already folded by LLVM under fat LTO.

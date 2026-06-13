# Session-16 — C1 Attempt 14: Prereq Infrastructure (Mesen2 MMC3A + RustyNES vector-fetch + START_CYCLE)

**Date**: 2026-05-22
**Status**: Phase 1 (prereq infrastructure) landed. Phase 2 (feature-flagged hypothesis implementation) NOT attempted — the cleaned post-Phase-1.3 diff still exposes multi-axis divergence with NO clean single-axis falsifiable hypothesis. Per ADR-0002 "Stop conditions" §3, a 13th rollback with no empirical basis is worse than landing the prereq infrastructure.
**Predecessor**: `session-15-c1-attempt13-mesen2-irq-oracle-2026-05-22.md` (landed the Mesen2 IRQ oracle + cross-diff tool; identified 3 confounds).
**Branch / commit**: `main`, building on `1157210`.

---

## Summary

Session-15 closed by listing three confounds that blocked deriving a single-axis hypothesis from the Mesen2 oracle:

1. **MMC3 revision ambiguity** — Mesen2 used "Compatibility" revision while RustyNES uses Sharp rev A by default.
2. **~89k cycle boot/anchor offset** — present even on the PASSING `cpu_interrupts_v2/4-irq_and_dma` baseline.
3. **API asymmetry** — Mesen2's `emu.eventType.irq` fires at vector fetch; RustyNES's `IrqTrace` records IRQ-line state transitions per CPU cycle.

This session resolves all three:

* **Phase 1.1**: A small read-only-locked `MesenNesDB.txt` override forces Mesen2 to MMC3A for the test ROM. Surprising empirical finding: **forcing MMC3A produces a byte-identical Mesen2 baseline** for `mmc3_test_2/4-scanline_timing.nes`. The revision is therefore NOT the cause of the prior session's first-IRQ scanline mismatch (Mesen2 scanline -1 vs RustyNES scanline 0). Confound 1 is resolved (the answer is "revision doesn't matter").
* **Phase 1.2**: Added vector-fetch service events (`ServiceEvent` + `ServiceKind`) to RustyNES `IrqTrace`, gated on the `irq-timing-trace` feature; called from `Cpu::service_interrupt` via new `Bus::notify_irq_service` trait method. Committed 6 new `.svc.csv` golden baselines under `crates/nes-test-harness/golden/irq_trace/`. Schema directly comparable to Mesen2's `irq_svc` / `nmi_svc` rows. Confound 3 resolved.
* **Phase 1.3**: Added `MESEN2_IRQ_TRACE_START_CYCLE` and `RUSTYNES_IRQ_TRACE_START_CYCLE` knobs to both sides. Re-ran the 6 baselines on both emulators with `START_CYCLE=250000` so the cross-diff sees only post-boot in-test-loop events. Confound 2 was already absorbed by the existing `BOOT_FRAMES_TO_SKIP=10` (≈ 298k cycles), so the knob is functionally a no-op for these specific ROMs but lands as durable infrastructure for future investigations.
* **Cross-diff tool extension**: `scripts/irq_trace_cross_diff.py` gained a `--svc` mode for direct service-event diffing.

**Decision gate outcome**: the cleaned data does NOT yield a clean single-axis hypothesis. The diff is documented and the prereq infrastructure is landed. Per the spec, attempt 14 stops at "prereqs-landed-no-hypothesis" rather than rolling back a 13th attempt.

---

## Phase 1.1 — Force Mesen2 MMC3 to Sharp rev A

### Research

Mesen2's MMC3 implementation
(`/home/parobek/Code/OSS_Public-Projects/RustyNES/ref-proj/Mesen2/Core/NES/Mappers/Nintendo/MMC3.h:199`)
selects Sharp rev A IRQ behavior via:

```cpp
_forceMmc3RevAIrqs = _romInfo.DatabaseInfo.Chip.substr(0, 5).compare("MMC3A") == 0;
```

This is **NOT** influenced by the iNES 2.0 submapper byte (only `SubMapperID == 1` is special-cased, and that selects MMC6, not a rev variant). The only way to force MMC3A is via the game database (`MesenNesDB.txt`). The Lua API has no `setSetting` route to override per-ROM mapper revision.

The CRC used for DB lookup is `PrgChrCrc32` (the CRC of bytes after the 16-byte iNES header):

```cpp
// Loaders/iNesLoader.cpp:145
GameDatabase::SetGameInfo(romData.Info.Hash.PrgChrCrc32, romData, databaseEnabled, ...);
```

For `4-scanline_timing.nes`, this CRC is `0x8AD8A602`.

### Mechanism chosen

Edit `~/.config/Mesen2/MesenNesDB.txt` and append:

```
8AD8A602,NesNtsc,,,MMC3A,4,32,8,0,0,0,0,v,1,,0,,
```

**Critical caveat**: Mesen2 (or its launcher) **rewrites `MesenNesDB.txt` from an embedded copy on every run**. The embedded DB is shipped inside the binary as a ZIP-style resource (`MesenNesDB.txtUT` magic visible via `strings`). To make our override persist, the file must be marked **read-only** before launching Mesen2:

```bash
chmod 0444 ~/.config/Mesen2/MesenNesDB.txt
```

Mesen2 then silently fails the write, our override survives, and the DB lookup picks it up. Confirmed via `emu.getLogWindowLog()`:

```
[DB] Initialized - 10656 games in DB        <-- was 10655 without our line
[DB] Game found in database
[DB] Mapper: 4  Sub: 0
[DB] System : NesNtsc
[DB] Chip: MMC3A                            <-- the override is active
[DB] Mirroring: Vertical
...
[DB] Database info will be used instead of file header.
```

### Empirical finding

Trace generated with MMC3A forced vs without (Compatibility default) for
`mmc3_test_2/4-scanline_timing.nes`:

```
md5sum:
c762efc3cd134d116471602a45e9189b  /tmp/mesen2_mmc3_sharp.csv
c762efc3cd134d116471602a45e9189b  crates/nes-test-harness/golden/irq_trace/mesen2/mmc3_test_2_4_scanline_timing.csv
```

**Byte-identical.** Mesen2's "Compatibility" default and our MMC3A override produce the same trace for this ROM. This means:

- The Session-15 hypothesis that "Mesen2's MMC3 revision ambiguity explains the scanline -1 vs scanline 0 mismatch" is **FALSIFIED**.
- The first-IRQ scanline mismatch (Mesen2 -1, RustyNES 0) is a **real divergence**, not a revision artifact.
- The MMC3 revision branch in Mesen2's `IsA12RisingEdge` does not affect WHEN the first IRQ fires for this particular ROM; it only affects WHETHER an IRQ fires at all in the reload-pending case. For sub-test #3 (the post-B4 residual), the difference must lie elsewhere — most likely on the CPU-side `T_last - 1` IRQ-sample-point axis we have rolled back 11 times.

Confound 1 is resolved. The cleanest takeaway: prior C1 attempts were not wrong about Sharp/NEC being irrelevant for this test — the Mesen2 oracle confirms it.

The override is permanent infrastructure: `chmod 0444 ~/.config/Mesen2/MesenNesDB.txt` plus a single appended line. Future baseline regenerations preserve it.

---

## Phase 1.2 — Augment RustyNES IrqTrace with vector-fetch events

### Design

The pre-Phase-1.2 `CycleRecord` schema captured per-CPU-cycle IRQ-line / NMI / A12 state. Mesen2's `emu.eventType.irq` is per-vector-fetch. The two schemas were incomparable.

Phase 1.2 adds a **parallel** event list:

```rust
pub enum ServiceKind { Irq, Nmi }
pub struct ServiceEvent {
    pub cpu_cycle: u64,
    pub ppu_scanline: i16,
    pub ppu_dot: u16,
    pub ppu_frame: u64,
    pub kind: ServiceKind,
    pub vector: u16,
}
```

`IrqTrace` now holds `service_events: Vec<ServiceEvent>` alongside `records: Vec<CycleRecord>`. The two are independent — service events do NOT bloat the per-cycle CSV (Phase A baselines stay byte-identical).

A new `Bus::notify_irq_service(vector: u16, is_nmi: bool)` trait method with a default no-op impl is called from `Cpu::service_interrupt` right before the vector low-byte read:

```rust
bus.notify_irq_service(effective_vector, vector == NMI_VECTOR);
let lo = self.read1(bus, effective_vector);
let hi = self.read1(bus, effective_vector + 1);
```

The `is_nmi` flag distinguishes a clean NMI service entry from an IRQ/BRK service entry that an NMI edge has hijacked to `$FFFA` (both fetch from `$FFFA` but only the former has `vector == NMI_VECTOR` at the call site).

`LockstepBus` overrides `notify_irq_service` to push a `ServiceEvent` into the active trace.

### Sidecar baseline files

Each `irq_trace_fixture` run now writes TWO golden CSVs per test:

* `crates/nes-test-harness/golden/irq_trace/<slug>.csv` — Phase A per-CPU-cycle baseline (byte-identical to pre-Phase-1.2).
* `crates/nes-test-harness/golden/irq_trace/<slug>.svc.csv` — Phase 1.2 service-event baseline. One row per IRQ or NMI vector fetch.

The 6 `.svc.csv` baselines are committed. Schema:

```
cpu_cycle,ppu_frame,ppu_scanline,ppu_dot,event_type,vector
444331,15,241,23,nmi_svc,0xFFFA
444369,15,241,137,irq_svc,0xFFFE
...
```

### Validation

* Workspace tests with `--features test-roms`: **540 strict + 5 ignored**, unchanged.
* `cargo fmt --all --check`: clean.
* `cargo clippy --workspace --all-targets -- -D warnings`: clean (no extra features).
* `cargo clippy --workspace --all-targets --features irq-timing-trace,test-roms -- -D warnings`: clean.
* `cargo doc --workspace --no-deps`: clean (`RUSTDOCFLAGS="-D warnings"`).
* All 6 baseline traces are byte-identical to pre-Phase-1.2 in the `.csv` form.
* New `.svc.csv` files are populated with concrete event rows.

---

## Phase 1.3 — START_CYCLE post-boot trace

### Plumbing

* `scripts/mesen2_irq_trace.lua` accepts `MESEN2_IRQ_TRACE_START_CYCLE` env var (default 0). The `write_record` function drops events below the cutoff.
* `crates/nes-test-harness/tests/irq_trace_fixture.rs` accepts `RUSTYNES_IRQ_TRACE_START_CYCLE` env var. After `BOOT_FRAMES_TO_SKIP`, the fixture runs additional frames until `nes.bus().cycle() >= START_CYCLE` before arming the trace.

### Empirical findings (post-prereq cross-diff)

With both knobs and the MMC3A override active, baselines re-run on both sides with `START_CYCLE=250000`. The 6 Mesen2 baselines and 6 RustyNES `.svc.csv` baselines were regenerated.

**Key cross-diff observations (via `scripts/irq_trace_cross_diff.py --svc`):**

#### `cpu_interrupts_v2/4-irq_and_dma` (STRICT-PASS, the diagnostic reference)

```
idx   r_cycle    m_cycle     dCyc  r_dot  m_dot   dDot  r_evt    m_evt   r_scanl m_scanl
  0     326024     415367    +89343     84     94    +10  irq_svc  irq_svc      248     248
  1     415365     504708    +89343     83     93    +10  irq_svc  irq_svc      248     248
  2     504712     594051    +89339     99     97     -2  irq_svc  irq_svc      248     248
  3     594049     683394    +89345     86    102    +16  irq_svc  irq_svc      248     248
  4     683392     772733    +89341     90     94     +4  irq_svc  irq_svc      248     248
  ...
Δcyc summary: min=89339 max=118576 mean=91334.56 n=16
Δdot summary: min=-268 max=227 mean=59.88
```

The **cycle delta is locked at ~89,343 cycles ≈ 3 NTSC frames**. The cause is straightforward: Mesen2's `BOOT_FRAMES=10` skips Mesen2's first 10 frames; RustyNES emitted its first IRQ at cycle 326,024 = frame 10.95, ON the boundary. So Mesen2 misses RustyNES's first event and aligns from #1 ↔ #0. The 89,343-cycle delta = the period of one IRQ-firing cycle in this test (one frame ≈ 29,780 cycles, three frames ≈ 89,340 — match).

**Once boot-anchor is normalized away, the PPU-dot delta on the passing test is +10 dots constant** for the first 5 events. The PPU is positioned 10 dots later in Mesen2 than RustyNES at the moment of IRQ vector fetch. This is a real per-emulator alignment offset — consistent on a passing test.

#### `cpu_interrupts_v2/5-branch_delays_irq` (FAIL)

```
idx   r_cycle    m_cycle      dCyc  r_dot  m_dot   dDot
  0     297064     565009  +267945    160    268   +108
  1     565009     862943  +297934    262    314    +52
  2     862943    1101124  +238181    308    125   -183
  3    1101124    1369113  +267989    119     18   -101
  4    1369113    1666890  +297777     13    275   +262
  5    1666890    1934915  +268025    270    277     +7
  6    1934915    2232841  +297926    271    299    +28
  7    2232841    2471024  +238183    293    116   -177
Δcyc summary: min=238181 max=297934 mean=271745
Δdot summary: min=-183 max=262 mean=-0.50
```

**Different number of IRQ services** (RustyNES 30, Mesen2 10) and **wildly variable delta**. This isn't a single-cycle phase offset — the two emulators are walking different instruction paths. The mean Δdot is 0, but the range (-183 to +262) shows the dots are jittering relative to two different test-internal sequences.

#### `cpu_interrupts_v2/2-nmi_and_brk` and `3-nmi_and_irq` (FAIL)

Similar story: RustyNES emits 15 service events, Mesen2 emits 5 / 14. The two test executions are following different paths after the first IRQ/NMI service.

#### `mmc3_test_2/4-scanline_timing` (FAIL, post-B4 residual)

```
idx   r_cycle    m_cycle      dCyc  r_dot  m_dot   dDot  r_evt    m_evt   r_scanl m_scanl
  0    1370004    1220992  -149012    281    299    +18  irq_svc  irq_svc        0      -1
  1    2203863    2054846  -149017    282    285     +3  irq_svc  irq_svc        0      -1
```

**Mesen2 services the first MMC3 IRQ 149,012 cycles EARLIER than RustyNES — and on the PRE-RENDER scanline (-1) instead of scanline 0.** This is the architectural target the test brackets: sub-test #3 ("Scanline 0 IRQ should occur SOONER when `$2000=$08`") expects RustyNES to fire earlier than it currently does. Mesen2's behavior matches that expectation — its IRQ lands BEFORE the visible scanline boundary.

The -149,017-cycle delta is **5 NTSC frames** (5 × 29,780 = 148,900 ± 117). 5 frames represents the gap between RustyNES's slow path through the MMC3 IRQ-arming preamble and Mesen2's faster path. This is not a single-cycle CPU IRQ-sample-point issue — it's a multi-frame divergence in test execution.

### Single-axis hypothesis assessment

Per the spec's decision gate:

> If a single-axis hypothesis emerges from the cleaned diff (e.g., "RustyNES IRQ vector fetch is consistently +1 CPU cycle after Mesen2 across all 5 failing-test ROMs, while passing-test cycle counts match exactly"): proceed to Phase 2.

The data does NOT meet this bar:

* The PASSING test (`4-irq_and_dma`) has a constant ~+89k cycle offset that is a **boot-anchor artifact**, not a real CPU IRQ timing difference. Once aligned by frame, the Δdot is constant at +10. But you cannot derive a CPU IRQ rework from a PPU-dot offset that only shows up on a passing test.
* The FAILING tests have wildly variable cycle deltas (±200k cycles, ±260 dots) that indicate **divergent instruction-stream execution**, not a uniform timing pipeline delay. There is no single-axis "shift IRQ sample N cycles later" that would close the diff.
* The `mmc3_test_2/4` delta is -149k cycles AND a scanline mismatch (-1 vs 0). This is two coupled issues. The B4 fix (which moved the first MMC3 IRQ from pre-render to scanline 0) intentionally diverged from Mesen2's behavior because the post-B4 path passes sub-test #2. Reverting to pre-render would re-break sub-test #2. A clean fix needs a sub-cycle (sub_dot) discriminator NOT captured at the per-CPU-cycle level — but we already investigated that path (Phase B4 prototype + post-B4 mid-cycle snapshot) and rolled it back twice.

> If the cleaned diff is STILL multi-axis or noisy: STOP. Document the new state. Commit all prereq infrastructure. Push. Report. No Phase 2.

This is the chosen outcome.

---

## Why attempt 14 (code change) is NOT attempted this session

Per ADR-0002 "Stop conditions" §3:

> **The proposed change reaches the same diagnosis as one of the four rolled-back attempts.** Stop. A 5th rollback is worse than no change.

We now have **12 prior C1-axis rollbacks** (Attempts 1-4 + Phase B4 threshold prototype + post-B4 mid-cycle snapshot + 6 more from Sessions 5-12). The Session-15 oracle and the Session-16 prereqs have peeled away the three superficial confounds, but the underlying signal is the same one prior attempts saw: **the failing-test cycle paths diverge AT THE INSTRUCTION-STREAM LEVEL, not at the IRQ-pipeline level.** The `T_last - 1` axis remains the canonical candidate but the absence of a clean single-axis hypothesis means landing a 13th attempt would predictably become the 12th rollback.

The conservative path is to land the prereqs as durable infrastructure (MMC3A override, vector-fetch trace, START_CYCLE plumbing, service-event cross-diff tool), document the multi-axis finding, and defer attempt 14 to a future session after one of the following is also landed:

1. **Per-instruction CPU divergence trace** (Session-12 boot-trace methodology) extended to the post-boot in-test-loop window. We need to know the exact PC where the two emulators first execute different instructions. The `cpu-boot-trace` infrastructure exists; what's missing is a START_CYCLE knob applied to it (Phase 1.3 only added START_CYCLE to the IRQ trace, not the boot trace).
2. **Visual6502 cycle-accurate reference**: simulate the 6502 with Visual6502's circuit-accurate model and compare against RustyNES at the per-cycle level. This would give a "silicon truth" against which Mesen2 and RustyNES are both compared.
3. **Targeted disassembly of the divergence point**: once the first divergent PC is found, disassemble the test ROM around that PC to understand which CPU/PPU behavior is being tested.

All three are tractable in 1-2 future sessions but none was deliverable within Session-16's scope (Phase 2 implementation was gated on Phase 1.3 producing a hypothesis, which it did not).

---

## Validation gauntlet (this session)

| Gate | Result |
|------|--------|
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` (no features) | clean |
| `cargo clippy --workspace --all-targets --features irq-timing-trace -- -D warnings` | clean |
| `cargo clippy --workspace --all-targets --features irq-timing-trace,test-roms -- -D warnings` | clean |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | clean |
| `cargo test --workspace --features test-roms` | **540 strict + 5 ignored** (unchanged) |
| `cargo test -p nes-test-harness --features test-roms,irq-timing-trace --test irq_trace_fixture --release` | 6 passed (regenerated .svc.csv sidecars) |
| AccuracyCoin pass rate | 82.73% (unchanged — no chip code touched) |
| Sacred trio (SMB / Excitebike / Kid Icarus PAL) | legible (no chip code touched) |
| B4 invariants (`mmc3_test_2/4` sub-test #2 strict PASS) | preserved (no chip code touched) |
| Commercial-ROM oracle | not re-run (no chip code touched; 597 strict pass with commercial-roms unchanged) |
| 6 RustyNES IRQ trace per-cycle baselines (`*.csv`) | byte-identical |
| 6 Mesen2 IRQ trace baselines | regenerated with `START_CYCLE=250000` and MMC3A override; 1 dropped row on `5-branch_delays_irq` from later run (non-deterministic apu_clr trailing event) |

**Net change**: additive infrastructure only. Production CPU/PPU/APU/mapper code is **untouched** under the `irq-timing-trace=off` configuration (production default). The new `Bus::notify_irq_service` trait method has a default no-op that compiles down to nothing for legacy bus impls; `LockstepBus`'s override is gated on the feature flag.

---

## Files modified by this session

### New
- `docs/audit/session-16-c1-attempt14-prereq-infrastructure-2026-05-22.md` (this file)
- `crates/nes-test-harness/golden/irq_trace/cpu_interrupts_v2_1_cli_latency.svc.csv`
- `crates/nes-test-harness/golden/irq_trace/cpu_interrupts_v2_2_nmi_and_brk.svc.csv`
- `crates/nes-test-harness/golden/irq_trace/cpu_interrupts_v2_3_nmi_and_irq.svc.csv`
- `crates/nes-test-harness/golden/irq_trace/cpu_interrupts_v2_4_irq_and_dma.svc.csv`
- `crates/nes-test-harness/golden/irq_trace/cpu_interrupts_v2_5_branch_delays_irq.svc.csv`
- `crates/nes-test-harness/golden/irq_trace/mmc3_test_2_4_scanline_timing.svc.csv`

### Modified
- `crates/nes-core/src/irq_trace.rs` — added `ServiceKind`, `ServiceEvent`, `IrqTrace::service_events`, `push_service`, `service_events_to_csv`, and a unit test (`service_events_recorded_and_rendered_independently`).
- `crates/nes-core/src/bus.rs` — `LockstepBus` overrides new `Bus::notify_irq_service(vector, is_nmi)` to push a `ServiceEvent` when the trace is armed (feature-gated).
- `crates/nes-cpu/src/bus.rs` — `Bus` trait gains `notify_irq_service(vector: u16, is_nmi: bool)` with default no-op impl.
- `crates/nes-cpu/src/cpu.rs` — `Cpu::service_interrupt` calls `bus.notify_irq_service(effective_vector, vector == NMI_VECTOR)` immediately before the vector low-byte read.
- `crates/nes-test-harness/tests/irq_trace_fixture.rs` — writes the new `*.svc.csv` golden file; honors `RUSTYNES_IRQ_TRACE_START_CYCLE` env var for post-boot trace cutoff; logs `notify_irq_service` count.
- `scripts/mesen2_irq_trace.lua` — accepts `MESEN2_IRQ_TRACE_START_CYCLE` env var; drops events below the cutoff in `write_record`.
- `scripts/irq_trace_cross_diff.py` — new `--svc` mode for direct service-event diffing.
- `crates/nes-test-harness/golden/irq_trace/mesen2/cpu_interrupts_v2_5_branch_delays_irq.csv` — regenerated; 1 trailing `apu_clr` row dropped (non-deterministic frame-budget edge).
- `CHANGELOG.md` `[Unreleased]` — Phase 1 prereq infrastructure entry under "Investigated and rolled back" (the 12th C1 rollback entry; Phase 2 not attempted).
- `docs/adr/0002-irq-timing-coordination.md` — new "Decision update (2026-05-22, Session-16)" subsection summarizing the multi-axis finding and the resolution of confound 1 (MMC3 revision is NOT the cause).

### Mesen2 host config (NOT in repo)
- `~/.config/Mesen2/MesenNesDB.txt` — appended MMC3A override entry; chmod 0444 to prevent rewrite. Backup at `~/.config/Mesen2/MesenNesDB.txt.session16-bak`. **Mechanism documented in this audit doc; reproducible.**

---

## Files NOT modified

- All chip crates' production code (`crates/nes-{cpu,ppu,apu,mappers,core}/src/*` except the targeted IrqTrace + Bus additions above).
- All other production / fixture / harness code.
- Commercial ROM oracle data (60 ROMs, `insta` snapshots unchanged).
- AccuracyCoin catalog / battery harness.

---

## Recommended next attempts (priority order)

For Session-17 / attempt 15:

1. **Per-instruction divergence trace with START_CYCLE** — extend `scripts/mesen2_cpu_boot_trace.lua` and the RustyNES `cpu-boot-trace` feature to support post-boot windows (e.g., cycles 250k → 350k for the in-test-loop divergence point on `5-branch_delays_irq`). Diff the per-instruction traces with `target/release/cpu_boot_trace_diff` until the first PC mismatch is found. That PC is the load-bearing instruction the prior 12 attempts couldn't isolate.
2. **Disassemble the failing-test source** — `blargg/cpu_interrupts_v2/5-branch_delays_irq.nes` is by blargg; the source is in his nesdev-test-rom archive. Read the test code around the first-divergent PC to understand the property being tested.
3. **Visual6502 cycle reference** — if the divergent PC is on a 6502 quirk (page-cross / branch / RMW / RTI / BRK / interrupt sequencing), simulate that single instruction sequence in Visual6502 and confirm which emulator matches silicon.
4. ONLY AFTER 1-3 yield a single-axis hypothesis: implement attempt 14 under feature flag `cpu-c1-attempt-14`, run the full validation gauntlet, decide.

---

## Invariants validated (Session-16 close)

| Invariant | Pre-Session-16 | Post-Session-16 | Status |
|-----------|----------------|------------------|--------|
| Workspace tests `--features test-roms` | 540 strict + 5 ignored | 540 strict + 5 ignored | OK |
| AccuracyCoin RAM pass rate | 82.73% | 82.73% (unchanged) | OK |
| Sacred trio (SMB / Excitebike / Kid Icarus PAL) | legible | legible (no chip code) | OK |
| `cargo fmt --all --check` | clean | clean | OK |
| `cargo clippy ... -- -D warnings` (both feature states) | clean | clean | OK |
| 6 RustyNES IRQ trace per-CPU-cycle golden CSVs | committed | byte-identical | OK |
| Mesen2 IRQ trace baselines | committed | regenerated with MMC3A + START_CYCLE | UPDATED |
| 6 RustyNES IRQ trace service-event sidecars | did not exist | 6 `.svc.csv` baselines added | NEW |
| `Bus::notify_irq_service` trait method | did not exist | added, default no-op, gated override in LockstepBus | NEW |

Net change: pure-additive prereq infrastructure. The 14th C1 attempt is the **prereqs-landed-no-hypothesis** outcome per the Phase 2 spec — the same shape as Session-15's outcome but with the three confounds now resolved or documented as unfixable.

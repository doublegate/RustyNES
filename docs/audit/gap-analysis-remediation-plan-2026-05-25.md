# RustyNES v2 — Gap Analysis & Remediation/Development Plan

> **SUPERSEDED (2026-06-10).** Every milestone in this plan shipped — v1.6.0/v1.7.0
> polish, the v2.0.0 master-clock refactor (its §3 centerpiece; AccuracyCoin
> 90.65%→**100.00%**), v2.0.1 legacy removal, and FDS detection (v2.1.0). The current
> forward gap analysis + roadmap is `~/.claude/plans/toasty-noodling-scroll.md`
> (post-v2.1.0). This document is retained as a historical record of the v1.5.0-era plan.

**Date:** 2026-05-25 · **Author:** gap-analysis sweep (4 parallel research agents + reference-emulator + nesdev-wiki study)
**Current state:** v1.5.0 (2026-05-24) · AccuracyCoin **90.65%** (126/139, RAM-direct) · 60-ROM commercial oracle 60/60 · **661 strict + 10 ignored** (`--features test-roms`)
**Supersedes:** `~/.claude/plans/generate-a-new-plan-snug-starlight.md` (the v2.0.0 path written at v1.0.0; partially executed — its v1.1.0/v1.3.0/v1.4.0 milestones shipped, but the v1.2.0-accuracy/v1.5.0-polish/v1.6.0-polish sequencing is stale and Phase 7/v1.5.0 was inserted ahead of it). This document re-sequences from the *actual* v1.5.0 state.

---

## 0. Executive summary

RustyNES v2 is **well past its original "intended point."** The v1.0.0 goal (AccuracyCoin ≥ 90%) was met at 90.65%, and the project shipped through **v1.5.0**, pulling several v2.0.0-plan items (WebAssembly, TAS movies, VRC7 FM audio) in *early*. So this is not a "behind schedule" gap analysis — it is a **forward-looking remediation + development plan** with three distinct horizons:

| Horizon | Theme | Risk | Ships value? |
|---|---|---|---|
| **W0 — now (days)** | Documentation hygiene | trivial | removes confusion; the docs currently contradict themselves |
| **v1.6.0 (~2-4 wk)** | Frontend polish + quick wins (independent of accuracy) | low | yes — gamepad rebind UI, Game Genie, browser persistence, CI de-risk |
| **v2.0.0 (~3-5 mo)** | Master-clock-precise scheduling refactor → then FDS → then netplay | high (architectural) | the single load-bearing axis that closes ~6 AccuracyCoin residuals + the C1 IRQ family + PAL 3.2:1, and unblocks FDS |

**The strategic insight (confirmed against the project's own memory note "don't chase surgically"):** of the 13 remaining AccuracyCoin failures, **only one (Implied Dummy Reads) is genuinely independent** of the master-clock refactor — and even it is coupled to a DMC-DMA cascade. The rest are either *free* once the clock is precise (the C1 family, `$2002` flag timing) or *targeted-after* work that only becomes tractable at sub-cycle granularity. **Do not spend a milestone surgically chasing residuals** — that path has 17 documented rollbacks. Bank the independent frontend value in v1.6.0, then do the refactor once, properly, in v2.0.0.

---

## 1. Gap analysis findings

### 1.1 Intended-vs-actual (per original ROADMAP phase exit-criteria)

**Exceeds plan** (v2.0.0-plan items shipped early as v1.x):
- WebAssembly target → shipped **v1.3.0** (was a later v2.0-plan item).
- TAS movie recording/playback → shipped **v1.4.0**.
- VRC7 OPLL FM audio → shipped **v1.1.0** (ADR 0006 supersedes the ADR 0004 deferral).
- Phase 4 AccuracyCoin exit (≥ 80%) → exceeded; reached 90.65%.
- Phase 7 (Nesdev hardening) → met as re-scoped, +25 strict tests.

**Trails plan** (deferred to v2.0, all converging on one root cause):
- Phase 3 exit ("`cpu_interrupts_v2/*` all pass") — **not met**; 3 of 5 sub-ROMs `#[ignore]`'d.
- Phase 4 exit (`mmc3_test_2/4` #3) — unresolved; `vrc24test` permanently link-rotted (replaced by in-tree fixture).
- The snug-starlight v1.2.0 target (broad accuracy → ~97%) — **not met**; only the narrow DMC scheduler shipped as the *actual* v1.2.0. AccuracyCoin held at 90.65%.
- Phase 6 closeout exit (513 strict + C1 flipped + multi-OS smoke) — not met as written; v1.0.0 shipped with C1 ignored and T-60-004 smoke test unverified.

**Root cause of every trailing item:** the integer-PPU-dot-per-CPU-cycle lockstep scheduler cannot represent the sub-cycle CPU↔PPU phase relationships these tests probe. This is the v2.0 master-clock refactor (§3).

### 1.2 Documentation drift — the most immediately actionable gap

`docs/STATUS.md` — the *self-declared single source of truth* — **contradicts itself**. Its header (L1-16) and v1.0.0+ version-policy rows (L353-357) are current, but three large mid-document sections were never updated past the v1.0.0-rc2 / 84.17% / 545-strict era:

| # | Location | Stale value | Correct value |
|---|---|---|---|
| D1 | STATUS.md AccuracyCoin row (L121) | "84.17% … trajectory ends at 82.73%" | **90.65%** (126/139) |
| D2 | STATUS.md top-line counts (L123-126) | "545 strict … 5 ignored … 601 total" | **661 strict + 10 ignored**; 721 w/ commercial |
| D3 | STATUS.md "Known residuals (v1.0.0-rc2 → v1.0.0 final)" (L229+) | "The repo currently ships v1.0.0-rc2" | ships **v1.5.0**; target is **v2.0** |
| D4-D5 | STATUS.md version rows (L351, L362-363) | v0.9.0 "current release candidate"; "Still deferred: TAS" | v1.5.0 current; TAS shipped v1.4.0 |
| D6 | STATUS.md mapper-85 / mapper-audio / "Not supported" (L194, L220, L204) | VRC7 FM "deferred … `mix_audio` returns 0" | **FM landed v1.1.0** (ADR 0006) |
| D8 | ROADMAP.md "Done" bullet (L14) | "gated on AccuracyCoin ≥ 90% (currently 69.78%) … v0.9.0 ships first" | self-contradicts L10 (v1.0.0 released at 90.65%) |
| D9-D10 | `to-dos/phase-6-v1.0.0-final/overview.md` + `phase-6-v1-closeout/overview.md` | "OPEN … 541 strict / 82.73% … 6-sprint plan to reach 90%" | fully superseded; v1.0.0 released, C1 deferred to v2.0 |
| D11 | `to-dos/phase-7-…/overview.md` workstreams (L40-52) | "Finish the C1 axis / Close remaining residuals" | re-scoped additive-only; deferred to v2.0 |
| D12 | `docs/nesdev-hardware-emulation-checklist.md` (L55, L108) | "randomized dev mode is a v1.x TODO"; "`instr_misc` still not vendored" | both **shipped v1.5.0** |
| D13 | `docs/mappers.md` (L170) | VRC7 FM "deferred per ADR-0004 … in v0.9.x" | **FM landed v1.1.0**; ADR 0006 |
| D15 | `docs/user-guide/compatibility.md` (worst end-user drift) | "14 mappers"; MMC5/N163 audio "deferred to v1.1"; VRC7 "Not supported"; "No gamepads — keyboard only" | 15 mappers; all expansion audio landed; VRC7 FM landed; **gilrs gamepad shipped** |
| D18-D19 | ADR 0002 status "Proposed (v1.0.0)"; ADR 0003 "Proposed" | 0002 → retargeted v2.0; 0003 → de-facto **Accepted** (ADR 0008 builds on it) |

Full drift table + exact line refs + the doc-hygiene fix list are in **Workstream 0** below.

### 1.3 The real AccuracyCoin residual ledger (13 failures — corrected)

The residual list circulated in older planning carried a **stale 2026-05-17 taxonomy**. Cross-checking the v1.0.0-final closure record (STATUS.md L61-66) and the v1.1.0 measurement (`sprint-2.1-…-2026-05-25.md`), these are **already CLOSED** and are NOT among the 13: SH\* unstable stores (5), Open Bus #9, Arbitrary-Sprite-zero / Misaligned-OAM / OAM-Corruption (3), Controller Strobing/Clocking, Frame Counter IRQ.

**The actual 13** (axis · master-clock relationship):

| # | Test (suite) | Err | Root-cause axis | MC? | Fix + source |
|---|---|---|---|---|---|
| 1 | **$2002 flag timing** (Sprite Eval) | 1 | VBL bit-7 clears M2-high, sprite bits 5/6 read M2-low (~1.9 PPU cyc apart); our read is atomic | **free** (A3/A7) | fractional scheduling separates sub-phases. nesdev PPUSTATUS; `cascade-a-investigation:452` |
| 2 | **NMI Overlap BRK** (CPU Int) | 2 | C1: NMI/BRK hijack window + per-cycle IRQ-sample phase | **free** (A2/A3) | `cpu_interrupts_v2/2` surface; Mesen2 `NesCpu.cpp` |
| 3 | **NMI Overlap IRQ** (CPU Int) | 1 | C1 (same as #2) | **free** (A2/A3) | `cpu_interrupts_v2/3` surface |
| 4 | **Interrupt flag latency** (CPU Int) | 11 | C1: branch polls IRQ before cycle 4 + page-cross dummy-read accounting | **free** (A2/A3) | `cpu_interrupts_v2/5`; nesdev CPU_interrupts |
| 5 | **Stale BG Shift Regs** (PPU Misc) | 3 | shifters keep clocking from unused-NT read when BG-enable toggles mid-HBlank | targeted-after (A) | latch + continue-shift; nesdev PPU_rendering; Mesen2 `LoadTileInfo`. **needs re-baseline** |
| 6 | **Stale Sprite Shift Regs** (PPU Misc) | 3 | per-sprite counter mode (Halted/Counting) not preserved across render-disable | targeted-after (A) | `spr_mode:[…;8]` per dot. **B8b sacred-trio risk** |
| 7 | **BG Serial In** (PPU Misc) | 2 | 2-5-cyc PPUMASK delay + serial-in across cycle-7 reload boundary | targeted-after (A) | per-PPU-cycle PPUMASK pipeline. **must keep `ppu_vbl_nmi/*` green** |
| 8 | **Sprites On Scanline 0** (PPU Misc) | 2 | pre-render(261) must be `261 & 255` for in-range at dots 256-319 + dot-340 odd-skip | targeted-after (A) | Mesen2 `NesPpu.cpp:959`; `cascade-a:422` |
| 9 | **$2004 Stress** (PPU Misc) | 2 | per-dot OAMADDR + secondary-OAM-addr walk visible via `$2004` | targeted-after (A) **(defer; 14-test cascade risk)** | Mesen2 `ReadSpriteRam`; `cascade-a:446` |
| 10 | **$2007 Stress** (PPU Misc) | 2 | PPU DATA 3-cyc latency + ALE-vs-Read at sub-PPU-clock granularity | needs MC (A) | Mesen2 acknowledges its own analog limits; nesdev PPUDATA buffer |
| 11 | **Implied Dummy Reads** (CPU Beh 2) | 3 | missing cycle-2 PC dummy on 21 implied/accum opcodes, coupled to DMC-DMA halt miscalibration | **independent** (but coupled) | Steps 1+2 landed behind `cpu-implied-dummy-reads`; needs multi-axis DMC recalibration |
| 12 | **Delta Modulation Channel** (APU) | 21 | NOT sample-wrap (correct in `dmc.rs:175`); unidentified sub-test | targeted-after / v2.0 | needs Mesen2 oracle trace to pin axis |
| 13 | **APU Register Activation** (APU) | 6 | DMC/OAM-DMA bus conflict when halt addr parked inside `$4000-$401F` mid-instruction | **free** (A) | needs sub-instruction bus tracking = C1 surface; conflict-mirror attempt rolled back (cascaded 41→not_run) |

**Master-clock Sprint A directly closes #1,#2,#3,#4 and enables #10,#13 = 6 of 13.** The PPU-misc cluster (#5-#9) becomes tractable post-A with one re-baseline. #11 is the lone independent win (but cascade-risky). #12 needs an oracle trace.

### 1.4 Deferred-item master inventory

| Item | State | Target | Blocker |
|---|---|---|---|
| C1 IRQ-timing (`cpu_interrupts_v2/{2,3,5}` + `mmc3/4` #3) | `#[ignore]`'d; 17 rollbacks | **v2.0 Sprint A** | master-clock refactor |
| `$2002` sub-cycle / `$2007` rendering / stale shifters | characterization-pinned | **v2.0 Sprint A** | same |
| SH\*/internal-bus *fix* (split landed; fix deferred) | pinned | v2.0 | RDY-low rule (split already in `internal_data_bus()`) |
| DMC get/put 6/10 → 10/10 + default-on | parallel impl behind flag | v2.0 (or v1.6 fallback) | abort path / master-clock parity |
| PAL 3.2:1 CPU:PPU ratio | hardcoded 3:1 | **v2.0 Sprint A** | fractional dividers (free in MC model) |
| wasm `.rnm` movie I/O + browser save-states | inert on wasm (no `rfd`; `save_state.rs` is `std::fs`) | **v1.6.0** | `web_sys` Blob/FileReader + IndexedDB |
| FDS | scope doc only | **v2.0 Sprint B** | needs precise scheduler |
| Four Score / Zapper | standard pad only | v1.x (independent) | device routing in `controller.rs` |
| Game Genie / cheats | none | **v1.6.0** | TetaNES `genie.rs` is MIT reference |
| gilrs **rebind UI** (auto-bind shipped) | hardcoded Xbox map | **v1.6.0** | config schema + capture modal |
| macOS-15-intel runner sunset | migrated; next deadline Aug 2027 | **v1.6.0** (de-risk now) | drop x86_64-darwin or zigbuild |
| Netplay | not started (determinism met) | v2.0 Sprint C | UI + transport |
| Multi-OS release smoke (T-60-004) | open; user-driven | verification gate | user downloads artifacts |

### 1.5 Open-ticket reconciliation

- **T-60-001** (C1) — closeout says OPEN, ROADMAP says "v1.x"; **actual: deferred to v2.0**. Fix both.
- **T-60-002** (AccuracyCoin ≥ 90%) — three numbers across files (69.78 / 82.73 / 90.65); **achieved, v1.0.0 shipped**. Mark closed.
- **T-60-003a/b/c** (6 commercial ROMs) — closed, consistent ✓.
- **T-60-004** (multi-OS smoke) — still open; v1.0.0 shipped without it checked off (undocumented gate bypass). Carry to v1.6.0 release-eng.
- **T-60-005** (v1.0.0 tag) — both files stale ("SUPERSEDED"/"BLOCKED"); **v1.0.0 released 2026-05-23**.
- Phase-7 tickets (T-71…T-74) — cleanly reconciled, no contradictions.

---

## 2. The optimized remediation + development sequence

```
W0  Doc hygiene (days)  ─┐
                         ├─►  v1.6.0  Frontend polish + quick wins (~2-4 wk, low risk)
                         │       └─►  v2.0.0  Master-clock refactor → FDS → Netplay (~3-5 mo)
                         │                        (Sprint A is critical path; B/C parallel after A1-A2)
                         └─►  (CI de-risk folded into v1.6.0)
```

### Workstream 0 — Documentation hygiene (do first; ~0.5-1 day)

Cheap, high-clarity-ROI, unblocks everything by making the docs trustworthy. Concrete edits:

1. **`docs/STATUS.md`** (highest priority — it contradicts itself):
   - AccuracyCoin row (L121): `84.17%/82.73%` → **90.65% (126/139)**; move the trajectory into a labelled "historical" parenthetical.
   - Top-line counts (L123-126): `545 → 661 strict`, `5 → 10 ignored`, commercial `601 → 721`.
   - "Known residuals (v1.0.0-rc2 → v1.0.0 final)" (L229+): retitle "Known residuals (deferred to v2.0)"; delete "ships v1.0.0-rc2"; trim the C1 attempt-log prose to a pointer into the audit docs.
   - Version rows (L351, L362-363): v0.9.0 → "superseded (historical)"; remove TAS from "Still deferred"; fix the 82.73% measurement.
   - Mapper-85 / `mapper-audio` / "Not supported" (L194, L204-206, L220): VRC7 FM "deferred" → **"landed v1.1.0 (ADR 0006 supersedes 0004)"**.
2. **`to-dos/ROADMAP.md`** L14: delete the stale "gated on … 69.78% … v0.9.0 ships first" clause.
3. **`to-dos/phase-6-v1-closeout/overview.md`** + **`phase-6-v1.0.0-final/overview.md`**: add a "SUPERSEDED — v1.0.0 released 2026-05-23 at 90.65%; 90% gate cleared via Phase 1a/b/d + 3a/b; C1/residuals deferred to v2.0" banner; mark T-60-001/002/005 closed/superseded; T-60-004 the only open (verification) item.
4. **`to-dos/phase-7-…/overview.md`** L40-52: reconcile workstreams with the as-shipped additive-only re-scope.
5. **`docs/nesdev-hardware-emulation-checklist.md`** L55/L108: power-on randomization + `instr_misc` → "landed v1.5.0".
6. **`docs/mappers.md`** L170: VRC7 FM → "landed v1.1.0".
7. **`docs/user-guide/compatibility.md`** (worst end-user drift): 14 → 15 mappers; expansion audio "Supported"; VRC7 FM "Supported (v1.1.0)"; "No gamepads" → "USB gamepads via gilrs"; annotate netplay/cheats with real targets.
8. **`docs/compatibility.md`** L72: remove the TAS out-of-scope bullet (shipped v1.4.0).
9. **ADRs:** 0002 status → "Proposed; retargeted to v2.0 master-clock refactor"; 0003 → "Accepted".
10. **`docs/audit/README.md`**: expand the Contents index to acknowledge the ~60 session/phase-7/v1.3/path-* audit docs.

### Milestone v1.6.0 — Frontend polish + quick wins (~12-18 engineer-days, low risk)

All independent of each other and of the deferred accuracy axis. Ordered by risk-retirement:

| # | Task | Effort | Notes / reference |
|---|---|---|---|
| 0 | **CI: drop `x86_64-apple-darwin`** (release.yml L62-77) + ADR + release-notes wording | 0.5d | Removes the Aug-2027 time-bomb now. Option B (cargo-zigbuild cross-compile, preserves the artifact) only if Intel-Mac demand surfaces — 3-5d + SDK vendoring + framework-link risk. |
| 1 | **Game Genie / cheat support** — `crates/nes-core/src/genie.rs` (NEW, no_std+alloc) + hook in **both** `raw_cpu_read` (bus.rs:1879) and `debug_peek_cpu` (bus.rs:509) + `Nes` API + egui cheat panel + per-ROM config | 3-4d | Near-verbatim from **TetaNES `tetanes-core/src/genie.rs`** (MIT, 155 LoC, has test vectors). Determinism-safe (off = bit-identical). Disallow movie-record while codes active (or snapshot into `.rnm` header — ADR 0008 is versioned for it). |
| 2 | **gilrs in-app rebind UI** — config `[input.gamepad1/2]` schema (`#[serde(default)]` back-compat) + `parse_gamepad_button` + config-driven `HashMap<gilrs::Button,Buttons>` (replaces the hardcoded `const fn` at input.rs:514) + extend `input_rebind_panel.rs` `Slot` enum with pad + P2 rows + axis-as-dpad/deadzone | 4-6d | Clone **TetaNES `tetanes/src/nes/renderer/gui/keybinds.rs`** + `input.rs` unified-input model (Key/Button/Axis, 3 slots per action, per-device UUID identity). gilrs button rebind is app-level (`HashMap`), not `set_mapping()`. Inert on wasm (gilrs `#[cfg]`'d out). |
| 3 | **Doc-sync** — controls.md (false "no gamepad/P2") + configuration.md (false "present_mode not wired") | 0.5d | Must follow #2 so docs match shipped UI. Folds into W0. |
| 4 | **wasm `.rnm` I/O + IndexedDB save-states** — `web_sys` Blob+anchor download / `<input type=file>`+FileReader upload; gate `save_state.rs` (currently pure `std::fs`) to IndexedDB on wasm | 3-5d | Closes browser-persistence gap (movie_ui.rs:19-25 + save_state.rs native-only). Scope call: movies-only ~2d vs movies+save-states ~4-5d (recommended — same wasm-I/O abstraction; browser is otherwise stateless). |
| 5 | **Perf hygiene** — add a **real-game** headless bench input (nestest is NOP-heavy, under-represents PPU); wire **bench-regression CI gate** (≥5% `full_frame`, ≥10% `ppu`/`cpu`); one `perf` profiling pass | 1.5-2d | `performance.md:99` flags benches are baseline-only. Highest-value perf work = *prevent* regression in a codebase with 8× headroom. See §5. |

**Optional v1.6.x follow-ups:** graphics/audio settings panel (~2d); Four Score 4-player (`controller.rs` serial protocol, ~3-4d, independent of MC).

**Do NOT** in v1.6.0: monomorphize `Box<dyn Mapper>` → enum (docs prove <1% of frame cost); chase AccuracyCoin residuals surgically; attempt FDS (v2.0-blocked) or Zapper (low value).

### Milestone v2.0.0 — Master-clock refactor → FDS → Netplay (~3-5 months)

The major-version break. **Sprint A (master-clock) is the critical path**; Sprints B (FDS) and C (netplay) can run partially parallel once A1-A2 plumbing lands. Full engineering design in §3; AccuracyCoin closure mapping in §4.

---

## 3. The master-clock-precise scheduling refactor (v2.0 Sprint A — centerpiece)

### 3.1 Why dot-granular lockstep cannot close the residuals

Current model: CPU-driven, integer-PPU-dot lockstep. `Cpu::step` → `read1`/`write1` does `bus.cpu_read(addr)` **then** `idle_tick` → `tick_one_cpu_cycle` (bus.rs:883) which ticks the PPU exactly 3 dots in a `for sub_dot in 0..3` loop. **The access lands at dot 0 of 3; the 3 dots run after it.** The per-cycle CPU↔PPU phase is pinned to "access at dot 0," unmovable by any global shift — proven in `session-29-option-a-empirical-falsification.md` (a user-authorized +2-dot PPU shift left `cpu_interrupts_v2/{2,3,5}` still failing because shifting VBL-set and the `BIT $2002` read together preserves their *relative* race position).

Mesen2 splits the 12-master-clock cycle into 5-before-access / 7-after-access for a read (`NesCpu.cpp:319,296`) — a 5/12-of-a-cycle pre-access advance the integer 3-dot model literally cannot represent. The M2-phase plumbing already landed (`M2Phase` enum at `scheduler.rs:42`, `poll_irq_at_phase`, the φ1/φ2 scaffold at `cpu.rs:529` + `bus.rs:1137`) but is inert because a 1-dot-granular access can't be placed at an arbitrary master-clock sub-position. DMA get/put parity is keyed off `self.cycle & 1` (bus.rs:1266) — the wrong clock; real parity is `(master_clock / cpu_divider) & 1`. PAL 3.2 has no place to put the 0.2 in an integer-dot model.

### 3.2 Target architecture — master-clock counter + run-to-timestamp catch-up

**Adopt the Mesen2 / TetaNES master-clock model: a `u64` master clock, CPU as active driver, PPU + mapper-IRQ as passive `run_to(timestamp)` catch-up.** Reject ares cooperative-threads (needs a coroutine runtime, fights `no_std+alloc`, hard to serialize suspended stacks) and uniform sub-dot lockstep (4× the PPU dispatch for no benefit).

> **Key improvement over the prior snug-starlight A1** (`tick_master_clocks(n)` uniform ticking): the **run-to-timestamp catch-up loop is mandatory for performance**, not optional. `ppu_run_to(target)` does `while ppu.master_clock + ppu_divider <= target { ppu.tick(); ppu.master_clock += ppu_divider }` — the PPU still ticks **exactly 3 (or 3.2) dots per CPU cycle**, identical dot count to today. This is the validated TetaNES Rust precedent (`tetanes-core/src/cpu.rs:286-300` `start_cycle`/`end_cycle` → `ppu.clock_to(master_clock - PPU_OFFSET)`; `ppu.rs:1221` `clock_to`).

Concrete shape (Rust, no_std core):
```rust
struct LockstepBus {
    master_clock: u64,
    cpu_divider: u8,   // 12 NTSC / 16 PAL / 15 Dendy
    ppu_offset: u8,    // power-on CPU/PPU alignment (replaces cpu_phase_offset)
    // PPU now carries its own master_clock + ppu_divider (4 NTSC / 5 PAL / 5 Dendy)
}
```
The CPU keeps driving (`run_frame` outer loop unchanged at nes.rs:165). `read1`/`write1` become the **production** path of the already-scaffolded φ1/φ2 split: φ1 runs the PPU to `master_clock + start`; the access fans out; φ2 runs the PPU to cycle-end + does end-of-cycle bookkeeping (the `notify_cpu_cycle`/`tick_with_external`/IRQ snapshot at bus.rs:1031). **Region becomes data** (`cpu_divider`/`ppu_divider`/`ppu_offset` constants) — PAL 3.2 closes as a data change (matches `Cycle_reference_chart` and Mesen2 `NesCpu.cpp:124`).

Interrupt sampling: move the canonical sample to **φ2 of the second-to-last cycle** (`CPU_interrupts` "status at end of second-to-last cycle"; Mesen2 `EndCpuCycle` `_prevRunIrq = _runIrq` one-cycle-delay latch) — the `T_last - 1` rule ADR 0002 identified as load-bearing for `mmc3_test_2/4` #3.

**The one non-trivial borrow-checker refactor:** the per-dot `PpuBusAdapter` creation (bus.rs:961, borrowing `self.mapper.as_mut()` per dot) must move *inside* the catch-up loop so the mapper borrow releases between catch-up and the CPU's own mapper access. Tractable because the bus owns both — a scoped re-borrow per catch-up call is sound.

### 3.3 Staged sprint breakdown (dependency-ordered; effort in engineer-days)

| Sprint | Scope | Effort | Closes |
|---|---|---|---|
| **A0** | Perf trip-wire (CI bench gate, prereq) + `Region`-as-data dividers (no behavior change) | 2-3d | — (gate) |
| **A1** | Master-clock core behind `master-clock-scheduler` flag: `master_clock`+dividers on bus/PPU; `ppu_run_to` catch-up; resolve the mapper-borrow; promote φ1/φ2 to production | 5-8d | (default-off byte-identical; feature-on produces a frame) |
| **A2** | CPU interrupt sample at φ2 of 2nd-to-last cycle (`_prevRunIrq` latch) — **targeted** | 3-5d | `mmc3_test_2/4` #3; keeps `cpu_interrupts_v2/1,4` aligned |
| **A3** | `$2002`/`$2007`/VBL race via mid-cycle access — **largely free** from A1 | 3-5d | `cpu_interrupts_v2/{2,3,5}` (#2,3,4), `$2002 flag timing` (#1) |
| **A4** | DMC get/put on master-clock parity + DMA controller (abort path is targeted) | 4-6d | DMC 6/10→10/10; APU Register Activation (#13) |
| **A5** | SH\* internal-bus + RDY-low rule — **targeted**; stale shift registers (#5,#6) **free** from mid-instruction writes landing at correct dots | 3-4d | SH\* tests; #5, #6 |
| **A6** | PAL 3.2 validation — **free** from A0/A1 data | 2-3d | PAL ratio; validate sacred Kid Icarus PAL |
| **A7** | PPU-misc residuals #7, #8, #9, #10 (per-PPU-cycle PPUMASK pipeline; pre-render in-range; defer #9 if 14-test cascade reappears) | 4-6d | #7, #8, #10 (#9 conditional) |
| **A8** | Re-baseline (user auth), flip default-on, delete old `#[cfg(not)]` branches + `cpu-c1-attempt-17`/`dmc-get-put-scheduler` flags + integer `cpu_phase_offset` | 4-6d + auth | (cutover) |

Total Sprint A: **~30-46 engineer-days** (~6-9 wk). Matches ADR 0002's multi-week estimate.

### 3.4 Migration strategy

**Parallel-implementation behind a `master-clock-scheduler` cargo feature (default-off)** — exactly as ADR 0007 did for `dmc-get-put-scheduler`, NOT a clean cutover. Default-off MUST stay bit-identical (determinism contract, 60-ROM oracle 60/60, sacred trio, B4 invariant, save-state round-trip) until A8.

**The hard reality:** flipping the flag changes the absolute cycle/dot relationship for every ROM → a one-time **comprehensive oracle re-baseline** (the 3rd, after Session-8 and Session-13): 60-ROM commercial FNV-1a, 81-PNG corpus, audio_db, 6 golden IRQ traces. Needs explicit user authorization (the single biggest scheduling item). Equivalence-harness + re-baseline gate sequence:
1. Land default-off; prove bit-identical.
2. Iterate the feature-on path against **Mesen2 traces** (not the old RustyNES baseline — the point is to match silicon) using the already-landed C1 infrastructure: `scripts/mesen2_cpu_boot_trace.lua` (per-instruction), `scripts/mesen2_irq_trace.lua` (per-IRQ), the Mesen2 `EventType::PpuCycle` patch (per-PPU-cycle). Reuse the ADR-0007 equivalence-harness pattern (`dmc_get_put_equivalence.rs` match-count signal).
3. User authorization → regenerate all snapshots in one commit → flip default-on → delete old branches.

### 3.5 Sprint B (FDS, ~8-12d, after A1-A4) & Sprint C (Netplay, ~3-4wk)

- **FDS** (mapper 20 + `.fds` parser + BIOS load + 64-step wavetable audio): hard-blocked on the precise scheduler (IRQ/transfer timing). nesdev `FDS*.md` local; Mesen2 `Core/NES/Mappers/FDS/` structural-only (GPL-3.0). User supplies `disksys.rom` (never committed).
- **Netplay** (rollback netcode; determinism contract already met): groundwork (input-frame serialization + rollback hooks) is MC-independent and partly enabled by the v1.4.0 TAS determinism proof; full P2P/WebRTC + signaling is a multi-sprint effort. GGPO reference; Mesen2 `Core/Netplay/` structural-only.

---

## 4. AccuracyCoin closure trajectory by milestone

| Milestone | Tests closed | Pass rate |
|---|---|---:|
| v1.5.0 (now) | — | 90.65% (126/139) |
| v1.6.0 | (none — frontend only) or #11 Implied Dummy Reads if the DMC cascade is recalibrated (cascade-risky, optional) | 90.65% (or ~91.4%) |
| v2.0 Sprint A2-A3 | #1,#2,#3,#4 + the 4 C1 `#[ignore]` flips | ~94% |
| v2.0 Sprint A4-A5 | #13, #5, #6, DMC 6→10 | ~96% |
| v2.0 Sprint A7 | #7, #8, #10 (#9 conditional) | **~98-99%** |
| v2.0 residual | #9, #12 (oracle trace) | target 100% bar 1-2 hardest |

The 5 C1 `#[ignore]`'d strict probes drop to 1 (only `mmc3_test_2/6-MMC3_alt`, NEC-rev-B, by-design). **Verification per fix:** the per-failing-test diagnostic in `accuracycoin.rs` (RAM-direct, must monotonically increase) is the per-sprint sentinel; golden IRQ traces + `irq-timing-trace` fixture for C1; `ppu-state-trace` + `ppu_trace_diff` vs `mesen2_ppu_trace.lua` for PPU-misc; the sacred-trio bisect (`scripts/regression-bisect/`) + all 10 `ppu_vbl_nmi/*` green after every PPU sub-step.

---

## 5. Performance plan

**There is no perf problem today** — `full_frame` ~2.06 ms/frame, ~8× headroom. All perf work is pre-emptive and **measure-first**.

- **Protect headroom first:** wire the bench-regression CI gate (≥5% `full_frame`) + a **real-game** bench input (nestest is NOP-heavy). This is the highest-value perf work. (v1.6.0 task #5.)
- **Master-clock refactor risk:** the catch-up loop keeps the dot count identical → expected < 5% overhead. Re-profile with `perf` after Sprint A1/A7; keep the PPU `PpuBus` adapter monomorphic (don't box it).
- **DEFER `Box<dyn Mapper>` → enum** — measured <1% of frame cost; not the bottleneck.
- **Candidate (only if profiled hot):** framebuffer is RGBA8 (240 KB written per-pixel + uploaded/frame); the PPU only emits 64 palette indices. An 8-bit palette-index framebuffer (60 KB) + shader LUT expansion is 4× less write/upload traffic — but it's a save-state version bump + shader change, so **measure wgpu upload cost first**.
- **Trivial safe win:** `drain_audio() -> Vec<f32>` allocates per-frame (60/s); a reusable `drain_audio_into(&mut Vec<f32>)` removes it (~0.5d).

---

## 6. Risk register + stop conditions

| Risk | Mitigation |
|---|---|
| **Default-off regression** at any Sprint A step | Immediate rollback. Default-off MUST stay bit-identical until A8 (ADR 0002 + ADR 0007 invariant). |
| Borrow-checker (A1 mapper-borrow) | Confine `PpuBusAdapter` lifetime inside `ppu_run_to`; scoped re-borrow is sound (bus owns mapper). |
| `ppu_offset` calibration (A3) | Single integer sets cold-boot alignment; wrong value re-introduces the Session-13 +344-dot class. Detect early via `mesen2_cpu_boot_trace.lua` (first `SEI` at same master clock); Session-29 found a +1-cyc/+2-dot boot delta this refactor must *close*, not preserve. |
| Re-baseline masks a real regression (A8) | Gate the flip on Mesen2 per-instruction trace match (Session-17 proved `4-irq_and_dma` is byte-identical to Mesen2 — must survive) + manual sacred-trio visual inspection. **Lesson from the v1.3.x green-left-column bug: prove render output correct visually before re-baselining** (project memory). |
| PPU-misc cascade (A7, #9) | The conflict-mirror attempt cascaded 41 tests → not_run; watch the per-suite drill-down for that signature; defer #9 if it reappears. |
| B8b sacred-trio FSM-clobber (A5 #6) | Per project memory: audit every FSM write site against concurrent readers in the same scanline; bisect after each sub-step (the `63d8dea` regression surface). |
| v2.0 calendar slip | Each v1.x is independently releasable; v1.6.0 is the LTS until v2.0. Sprint A is critical path — if it stalls, ship v2.0 with FDS/netplay → v2.1. DMC get/put already banked default-off so partial value isn't lost. |
| GPL-3.0 contamination | All Mesen2/fceux/higan/ares reading is **structural-only**. TetaNES `genie.rs` is MIT (safe direct port). emu2413 (already done) was MIT. Per-file license headers verified at port time. |

---

## 7. Research index (per task)

| Task | nesdev_wiki pages | Reference-emulator source |
|---|---|---|
| Master-clock (A) | `Cycle_reference_chart`, `CPU_interrupts`, `PPU_frame_timing` | Mesen2 `NesCpu.cpp:124-323` (dividers/StartCpuCycle/MemoryRead/EndCpuCycle), `NesPpu.cpp`; **TetaNES `tetanes-core/src/cpu.rs:286-300` + `ppu.rs:1221`** (Rust precedent) |
| `$2002`/`$2007` (A3/A7) | `PPU_registers` (PPUSTATUS/PPUDATA buffer), `Visual_2C02` | Mesen2 `NesPpu.cpp` per-PPU-cycle Lua oracle (patched) |
| Sprite eval / OAM (A5/A7) | `PPU_OAM`, `PPU_sprite_evaluation` | Mesen2 `NesPpu.cpp:959,702,1018` |
| SH\*/internal bus (A5) | `Programming_with_unofficial_opcodes`, `Open_bus_behavior` | Mesen2 `NesCpu.cpp` internal bus |
| DMC/APU (A4) | `APU_DMC`, `APU_Frame_Counter` | Mesen2 `NesApu.cpp` per-cycle DMC fetch |
| Game Genie (v1.6) | `Game_Genie` (6/8-char decode) | **TetaNES `tetanes-core/src/genie.rs`** (MIT), fceux Game Genie |
| gilrs rebind (v1.6) | `Standard_controller`, `Four_score`, `Zapper` | **TetaNES `keybinds.rs` + `nes/input.rs`** |
| FDS (B) | `FDS`, `FDS_BIOS`, `FDS_audio`, `FDS_disk_format`, `FDS_file_format` | Mesen2 `Core/NES/Mappers/FDS/` (structural) |
| Netplay (C) | — | GGPO reference; Mesen2 `Core/Netplay/` (structural) |
| CI sunset (v1.6) | — | cargo-zigbuild README; macroquad zigbuild-osx guide; gilrs docs |

Per-sprint research citations land at `docs/audit/v<X>.<Y>-sprint-<N>-research-citations.md` (nesdev page + Mesen2 commit ref + web URL + retrieval date).

---

## 8. Reference-emulator clone locations

`/home/parobek/Code/OSS_Public-Projects/RustyNES/ref-proj/` → `Mesen2`, `tetanes`, `fceux`, `higan`, `ares`. nesdev wiki: `nesdev_wiki/` (6,062 files, in-project).

**License posture:** Mesen2 / fceux / higan / ares are GPL/structural-reference-only. TetaNES is MIT (the only one safe to port from directly). All Mesen2 algorithm knowledge is reimplemented clean-room.

---

## Appendix — supersession of `snug-starlight.md`

| snug-starlight milestone | Status now | This plan |
|---|---|---|
| v1.1.0 OPLL | ✅ shipped | done |
| v1.2.0 accuracy → 97% | ❌ not done (only DMC scheduler shipped as actual v1.2.0) | folded into v2.0 Sprint A (most residuals are MC-dependent) |
| v1.3.0 wasm | ✅ shipped | done |
| v1.4.0 TAS | ✅ shipped | done |
| v1.5.0 polish | ❌ slot taken by Phase 7 | → **v1.6.0** here |
| v1.6.0 accuracy 98% polish | ❌ not done | folded into v2.0 Sprint A (surgical residual-chasing rejected — 17 rollbacks) |
| v2.0.0 master-clock + FDS + netplay | pending | **§3** here, with the run-to-timestamp catch-up improvement + corrected A0-A8 staging |

Net change vs snug-starlight: (1) re-sequenced from actual v1.5.0; (2) corrected the AccuracyCoin residual ledger (5 clusters it listed are already closed); (3) replaced uniform `tick_master_clocks(n)` with the performance-preserving run-to-timestamp catch-up (TetaNES-validated); (4) added the doc-hygiene workstream and the bench-regression CI gate; (5) rejected a standalone surgical-accuracy milestone in favor of letting the master-clock refactor close the cluster.

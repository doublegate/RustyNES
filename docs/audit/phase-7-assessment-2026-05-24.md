# Phase 7 (Nesdev Accuracy Hardening) — intent vs. accomplished vs. completable

**Date:** 2026-05-24
**Author:** accuracy-hardening planning pass (pre-v1.5.0)
**Inputs:** `to-dos/phase-7-nesdev-accuracy-hardening/{overview,sprint-1..4}.md`,
`docs/nesdev-hardware-emulation-checklist.md`,
`ref-docs/nesdev-wiki-technical-report.md`, `docs/STATUS.md`,
`docs/audit/session-29-c1-axis-final-conclusion-2026-05-23.md`.

## 1. The phase-numbering reconciliation

The ROADMAP carried a self-contradiction that produced the "we skipped Phase 7"
observation:

- The **top-of-file summary** (`ROADMAP.md` "Earlier phases" line) said
  *"Phase 7 — v1.1.0 RELEASED: VRC7 OPLL FM audio."*
- The **detailed phase sections** define *"Phase 7 — Nesdev Accuracy Hardening
  (v1.x candidate)"* with its own `to-dos/phase-7-nesdev-accuracy-hardening/`
  directory, and number the shipped work **Phase 8 = v1.2.0 (DMC)**,
  **Phase 9 = v1.3.0 (wasm)**, **Phase 10 = v1.4.0 (TAS)**.

**Ground truth.** Phase 7 (Nesdev Accuracy Hardening) is a planning phase that
was **authored but never executed**. The releases v1.1.0 → v1.4.0 were sequenced
from the *separate* v2.0.0 release plan
(`~/.claude/plans/generate-a-new-plan-snug-starlight.md`), not from the ROADMAP's
Phase 7. The "Phase 7 = v1.1.0" summary line was an erroneous back-label. This
assessment treats **Phase 7 = the Nesdev Accuracy Hardening phase** and schedules
it as the next release, **v1.5.0**. The previously-pencilled "v1.5.0 frontend
polish" (gamepad rebind UI, macOS x86_64 sunset, Game Genie) shifts to a later
minor.

## 2. The hard constraint that shapes this phase

A large share of Phase 7's *accuracy* tickets (the AccuracyCoin residuals, the
C1 IRQ-sample-timing axis, the `$2002` sub-cycle flag timing, the SH\* internal
bus) are **the same architectural surface that 17 documented rollback attempts
could not move**. Session-29 empirically falsified the last surgical option
(global PPU-position shift) and concluded that closure requires the
**master-clock-precise scheduling refactor** — a breaking change explicitly
reserved for **v2.0** (`docs/audit/session-29-*`).

Therefore Phase 7's *value in the v1.x horizon* is **not** "push AccuracyCoin
past 90.65%". Chasing those residuals surgically is the trap that has burned 17
attempts and (via oracle re-baselining) once shipped a green-but-wrong build
(the v1.0.0→v1.3.0 left-column regression). Phase 7's exit criterion is
deliberately worded to be satisfiable **without** moving those residuals:

> all stock behavior is *implemented, **explicitly out of scope**, or **guarded
> by tests***.

So Phase 7 = **coverage + region validation + developer ergonomics + documented
scope closure**, holding AccuracyCoin at **90.65%** and the 60-ROM oracle / sacred
trio / B4 invariant **byte-identical**. Every change is additive (new tests, new
opt-in features, new docs) and stays off the hot paths that risk the oracle.

## 3. Per-ticket disposition

Legend: **DONE** (already shipped) · **NOW** (completable in v1.5.0, safe/additive)
· **v2.0** (requires the master-clock refactor or a risky bus rework; document &
defer).

### Sprint 1 — Source and test corpus closure

| Ticket | Disposition | Notes |
|---|---|---|
| T-71-001 source-map audit | **NOW** | Doc-only: link each checklist cluster to its subsystem doc + upstream Nesdev page. |
| T-71-002 `cpu_reset` coverage | **NOW** | ROMs *already vendored* (`sprint-2/cpu_reset_{registers,ram_after_reset}.nes`) but **unwired**. Add strict `$6000`-protocol tests. |
| T-71-003 `instr_misc` coverage | **NOW** | blargg PD; `instr_misc.nes` + `instr_timing.nes` present in the local `nes-test-roms` clone. Vendor + wire + LICENSES entry. |
| T-71-004 input-device test plan | **NOW** | Standard-pad + DMC-conflict tests now; Four Score / Zapper / NES 2.0 default-device = decision + targeted coverage or documented defer. |
| T-71-005 VRC24 fixture replacement | **NOW** | Upstream link rot is permanent. Replace with an in-tree VRC2/VRC4 register/wiring unit fixture; document the substitution. |
| T-71-006 PAL/Dendy validation inventory | **NOW** | Inventory doc feeding Sprint 3 T-73-005/006. |

### Sprint 2 — CPU, DMA, internal bus closure

| Ticket | Disposition | Notes |
|---|---|---|
| T-72-001 C1 IRQ-sample-timing bundle | **v2.0** | 17 rollbacks; Session-29 falsified surgical closure. Document deferral; keep `#[ignore]` probes. |
| T-72-002 NMI hijack / BRK vector evidence | **NOW (partial)** | Add B-flag stack-value + NMI-hijack unit tests that do **not** depend on the C1 cycle-edge (the cycle-edge sub-cases stay on the C1 axis). |
| T-72-003 internal-vs-external bus model (SH\*/TAS/LAS/XAA) | **v2.0** | Prior attempt regressed Internal Data Bus #2. Add a characterization test pinning current behavior; defer the fix. |
| T-72-004 DMC DMA side-effect bracket audit | **NOW (audit+test)** | Keep blargg `dmc_dma_during_read4` 5/5; add an audit doc + bracket tests. The *fix* is the get/put scheduler (ADR 0007, v2.0). |
| T-72-005 power-on randomization mode | **NOW** | Clean additive developer feature. Default stays deterministic for CI/save-state. |
| T-72-006 `$4015` open-bus semantics | **NOW** | Behavior already implemented (Phase D3). Document + test that `$4015` doesn't refresh external open bus. |

### Sprint 3 — PPU residuals and region variants

| Ticket | Disposition | Notes |
|---|---|---|
| T-73-001 stale BG/sprite shifter modeling | **v2.0** | Cascade-A residual axis; re-baseline trap. Characterization test only. |
| T-73-002 `$2002` sub-cycle flag timing | **v2.0** | On the C1/master-clock axis (Session-27/29). Trace-backed characterization test pinning current behavior. |
| T-73-003 `$2004`/OAMADDR rendering closure | **DONE/partial** | OAMADDR reset dots 257-320 + walks-during-eval landed (v1.0.0). Residuals are Cascade-A; document. |
| T-73-004 `$2007` rendering-time r/w | **v2.0** | Cascade-A residual; document. |
| T-73-005 PAL timing validation | **NOW** | PPU already region-aware (PAL 312 lines, no odd-frame skip). Add automated PAL gate. |
| T-73-006 Dendy timing validation | **NOW** | Dendy 312 lines, VBL@291, NTSC-style CPU cadence. Add automated Dendy gate. |
| T-73-007 PPU variant scoping (2C03/04/05/Vs) | **NOW (doc)** | Out of scope; record diagnostics in `compatibility.md`. |

### Sprint 4 — Mappers, expansion audio, platform variants

| Ticket | Disposition | Notes |
|---|---|---|
| T-74-001 NES 2.0 submapper audit | **NOW** | Fixture matrix: MMC3 A/B/C (Sharp default), VRC2/4 wiring, BNROM/NINA (M34 variant detect — already implemented), bus-conflict variants. |
| T-74-002 MMC5 deferred features | **NOW (decision)** | MMC5 *audio landed* (Track C2). Remaining = multi-bank PRG-RAM (`$5113`): small impl or documented defer. |
| T-74-003 VRC7 FM audio decision | **DONE** | OPLL landed v1.1.0 (ADR 0006). Mark resolved. |
| T-74-004 FDS platform plan | **NOW (scope doc)** | Implementation is v2.0 Sprint B. Write the scope/plan doc; defer code. |
| T-74-005 expanded input devices | **NOW (decision)** | Decide Four Score / Zapper / Famicom expansion / mic. Implement Four Score if cheap; else documented defer with NES 2.0 default-device metadata. |
| T-74-006 Vs. System / PlayChoice-10 | **NOW (doc)** | Out of scope; precise ROM-load diagnostics. |
| T-74-007 long-tail mapper policy | **NOW (doc)** | Acceptance policy: user demand + test availability + NES 2.0 metadata + maintenance cost. |

## 4. v1.5.0 = Phase 7 deliverable

**Theme:** Nesdev Accuracy Hardening — *coverage, region validation, developer
ergonomics, and documented scope closure*; accuracy residuals on the
master-clock axis explicitly deferred to v2.0 with rationale.

**Net-new (safe, additive):**
1. Wire vendored `cpu_reset` (+2 strict) and vendor+wire `instr_misc` /
   `instr_timing` (blargg PD; +tests) — Sprint 1.
2. Power-on randomization **developer mode** (opt-in seeded RAM/latch/phase
   randomization; default deterministic) — Sprint 2.
3. Automated **PAL** and **Dendy** timing-validation gates — Sprint 3.
4. CPU/DMA/`$4015` characterization + NMI-hijack/B-flag tests — Sprint 2.
5. NES 2.0 submapper fixture matrix + VRC24-replacement register fixture —
   Sprints 1/4.
6. Input-device coverage (standard pad + DMC conflict; Four Score decision) —
   Sprints 1/4.
7. Documentation closure: source-map audit, `compatibility.md` scope
   (FDS plan, Vs/PC10, PPU variants, long-tail policy, MMC5 multi-bank),
   `testing-strategy.md` corpus-gap closure, ADR refresh.

**Invariants held byte-identical:** AccuracyCoin 90.65% (126/139), 60-ROM oracle
60/60, sacred trio (SMB/Excitebike/Kid Icarus PAL), B4 invariant (first MMC3 IRQ
@ cycle 1,370,110 / scanline 0 / dot 257), `run_frame` semantics.

**Explicitly deferred to v2.0 (master-clock refactor):** C1 (T-72-001),
internal-bus SH\* fix (T-72-003), `$2002` sub-cycle fix (T-73-002),
stale-shifter fix (T-73-001), `$2007` rendering residual (T-73-004), FDS code
(T-74-004), Vs/PC10 (T-74-006).

**Exit criterion (Phase 7 overview, satisfiable as scoped):** every checklist
behavior is implemented, explicitly out of scope in `compatibility.md`, or
guarded by a test; missing Nesdev test categories are vendored-with-license or
replaced by in-tree fixtures; PAL/Dendy have automated gates; platform scope is
documented.

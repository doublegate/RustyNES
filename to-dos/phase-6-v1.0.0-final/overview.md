# Phase 6 — v1.0.0 Final (SUPERSEDED)

> **SUPERSEDED (2026-05-25).** v1.0.0 released 2026-05-23 at AccuracyCoin
> 90.65%; the six-sprint backlog below was NOT executed as written — the 90%
> gate was cleared via the Phase 1a/b/d + 3a/b closures, and the residuals
> these sprints targeted are deferred to the **v2.0 master-clock refactor**.
> Surgical residual-chasing was rejected after 17 documented rollbacks; see
> `docs/audit/gap-analysis-remediation-plan-2026-05-25.md` (§1.3 corrects the
> residual ledger — several clusters listed below are already closed).
> Retained for historical provenance only.

**Phase status:** SUPERSEDED (was OPEN; v1.0.0-rc2 tagged 2026-05-22 at HEAD
`b4e2860`). v1.0.0 final shipped 2026-05-23.

**Mission.** Close the 90% AccuracyCoin gate to promote v1.0.0-rc2 →
v1.0.0 final. Per the user's Option-B mandate (2026-05-22), the 90%
target is preserved unchanged: the project commits to reaching it over
additional sessions rather than reframing it.

## Current state (as of v1.0.0-rc2 tag)

| Metric | Value |
|---|---|
| Workspace strict pass | **541** (+ 5 `#[ignore]`'d) across 34 suites with `--features test-roms` |
| Commercial-ROM oracle | **+60 strict** with `--features test-roms,commercial-roms` (= 601 total) |
| AccuracyCoin RAM-direct | **82.73%** (108 pass + 7 pass_with_code of 139 assigned tests) |
| AccuracyCoin CI floor | 60% (`MIN_PASS_RATE` in `crates/rustynes-test-harness/tests/accuracycoin.rs`) |
| Gap to v1.0.0 90% gate | **+11 tests** (≥ 126 / 139 assigned) |
| B4 invariant | First MMC3 IRQ at cycle 1,370,110 / scanline 0 / dot 257 |
| C1 axis | 13 rollbacks since v0.9.0-rc; Session-18 finding: load-bearing axis is CPU-vs-PPU per-cycle access interleaving |

## Failing tests by cluster (24 remaining)

Per `docs/audit/session-19-accuracycoin-pivot-2026-05-22.md` and
`crates/rustynes-test-harness/tests/accuracycoin.rs` per-CI-run diagnostic:

| Cluster | # | Examples |
|---|---|---|
| C1 IRQ-timing axis | 4 | `cpu_interrupts_v2/{2,3,5}` + `mmc3_test_2/4` sub-test #3 |
| SH* unstable stores | 5 | SHA / SHX / SHY / SHS / TAS (internal-bus model) |
| Sprite-eval residuals (post-BG-fix) | 4 | `$2002 flag timing`, `Arbitrary Sprite zero`, `Misaligned OAM`, `OAM Corruption` |
| PPU misc residuals | 6 | `Stale BG/Sprite Shift Regs`, `BG Serial In`, `Sprites On Scanline 0`, `$2004 Stress`, `$2007 Stress`, `Rendering Flag Behavior` |
| APU residuals | 4 | Frame Counter IRQ #7, DMC #21, APU Register Activation, Controller Strobing |
| Implied Dummy Reads | 1 | `CPU Behavior 2 :: Implied Dummy Reads [error 3]` (Sprint 1 target; cascades into DMC DMA) |
| Open Bus residual | 1 | `Open Bus [error 9]` (internal-vs-external bus distinction) |
| **TOTAL** | **24** | |

Source: per-failing-test list printed by `cargo test -p rustynes-test-harness
--features test-roms accuracycoin --release -- --nocapture`.

## Approach

Prioritized sprints, ordered by tractability and cascade risk. Each sprint
follows the gauntlet methodology proven by Cascade A / Cascade B / Phase B4
/ Session-13:

1. Feature flag the change (`<sprint-name>` cargo feature).
2. Add a unit test reproducing the bug at the chip-FSM level (before any
   production code change). The Cascade A `crates/rustynes-ppu/src/ppu.rs`
   reproducer pattern is the template.
3. Land the production code change.
4. Run validation gauntlet (see "Validation gauntlet" below).
5. Commit + push OR revert + audit doc.
6. Re-measure AccuracyCoin. If ≥ 90%, jump to v1.0.0 final tag.

## Sprint order (contingent — each depends on prior closure)

| Sprint | Target | Est. effort | Est. yield | Cascade risk |
|---|---|---|---|---|
| **1** | Implied-Dummy + DMC DMA coordinated | 1-2 days | +1-3 | HIGH (Session-19 cascade revert) |
| **2** | APU put/get phase plumbing | 2-3 days | +1-3 | MEDIUM |
| **3** | Sprite-eval residuals (4 tests, one fix per test) | 1-2 d/test | +1-4 | HIGH (sacred-trio risk) |
| **4** | PPU misc residuals (6 tests, per-PPU-dot tooling first) | 1-3 d/test | +1-3 | HIGH |
| **5** | C1 axis attempt 17 (CPU/PPU access ordering rework) | 3-5 days | +1-4 | HIGHEST (13 prior rollbacks) |
| **6** | SH* unstable stores (internal-bus model rework) | 3-5 days | +6 | HIGHEST (DMC + open-bus shared surface) |

**Sprint 5 is intentionally LATE** in the priority order per Session-19
strategic guidance. Sprints 1-4 are higher leverage per session and the
C1 axis has 13 prior rollback attempts requiring careful re-baselining.

**Sprint 6 (SH* stores)** is the highest-yield single sprint but also
the highest cascade risk: only attempt if Sprints 1-5 have not
collectively reached 90%.

## Validation gauntlet (every sprint)

In order, every gate must stay green before commit:

1. `env -u RUSTC_WRAPPER cargo fmt --all --check`
2. `env -u RUSTC_WRAPPER cargo clippy --workspace --all-targets --features test-roms -- -D warnings`
3. `env -u RUSTC_WRAPPER RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`
4. `env -u RUSTC_WRAPPER cargo test --workspace --features test-roms` (≥ 541 strict pass)
5. `env -u RUSTC_WRAPPER cargo test --workspace --features test-roms,commercial-roms` (+60 strict, snapshots stable)
6. AccuracyCoin RAM-direct re-measure: monotonic increase, never below v0.9.x 80% target
7. B4 invariant: first MMC3 IRQ at cycle 1,370,110 / scanline 0 / dot 257
8. Sacred trio (SMB / Excitebike / Kid Icarus PAL): `scripts/regression-bisect/bisect-real-games.sh`
9. `ppu_vbl_nmi` 10/10, `sprite_hit_tests` 11/11, `sprite_overflow_tests` 5/5, `apu_test` 8/8, `apu_mixer` 4/4, `dmc_dma_during_read4` 5/5 — all strict
10. `env -u RUSTC_WRAPPER cargo build --workspace --target thumbv7em-none-eabihf --no-default-features` (no_std)

## Sprint gate conditions

- After each sprint: re-measure AccuracyCoin. If ≥ 90%, jump to v1.0.0
  final tag (sprint backlog terminates early).
- If Sprint N regresses any gate: revert the chip-stack code change, land
  only the diagnostic / audit-doc / unit-test infrastructure, document
  the rollback in CHANGELOG `[Unreleased]` → "Investigated and rolled
  back", and re-plan Sprint N before re-attempting.
- If after Sprint 6 still < 90%: STOP and re-negotiate with user. The 90%
  bar may need v1.x reframing at that point.

## Sprint files

- [Sprint 1 — Implied-Dummy + DMC coordinated](sprint-1-implied-dummy-dmc-coordinated.md)
- [Sprint 2 — APU put/get phase plumbing](sprint-2-apu-put-get-phase.md)
- [Sprint 3 — Sprite-eval residuals](sprint-3-sprite-eval-residuals.md)
- [Sprint 4 — PPU misc residuals](sprint-4-ppu-misc-residuals.md)
- [Sprint 5 — C1 axis attempt 17](sprint-5-c1-axis-attempt-17.md)
- [Sprint 6 — SH* unstable stores](sprint-6-sh-unstable-stores.md)
- [Sprint gate conditions + early-exit policy](sprint-gate-conditions.md)

## Reference docs (cold-start primer)

- `docs/audit/session-19-accuracycoin-pivot-2026-05-22.md` — the
  immediate source for this backlog. Session-19 attempted Sprint 1
  (Implied Dummy Reads) and cascade-reverted into DMC DMA. The
  audit captures the cascade structure that the coordinated Sprint 1
  fix must close.
- `docs/audit/cascade-a-investigation-2026-05-19.md` — methodology
  template for sprite-eval / PPU residuals (Sprints 3 + 4).
- `docs/audit/accuracycoin-readme-analysis-2026-05-17.md` — original
  cluster diagnosis + the 2026-05-19 addendum confirming which fixes
  flipped which tests.
- `docs/adr/0002-irq-timing-coordination.md` — C1 IRQ-timing ADR with
  "Decision (revised, 2026-05-13)" + per-session decision-update
  subsections through Session-18 (Sprint 5 primary reference).
- `docs/audit/session-17-*.md` + `docs/audit/session-18-*.md` —
  PPU-axis empirical findings that re-framed the C1 axis from CPU
  IRQ-sample-point to CPU-vs-PPU access interleaving (Sprint 5 entry
  point).
- `docs/audit/session-13-cpu-boot-fix-2026-05-21.md` — Mesen2-aligned
  cold-boot prerequisite (already landed, retained as context for
  Sprint 5's contamination-free foundation).
- `docs/STATUS.md` — single source of truth for per-suite pass count,
  mapper coverage, feature flags, version policy.

## Commit + push discipline

Each sprint commits ONLY when its validation gauntlet is green. Sprints
that cascade-revert land their diagnostic / unit-test / audit-doc
artifacts as a separate audit-only commit. Multi-commit sprints follow
the established methodology: feature-flag commit → validation → land
production code OR rollback → audit doc → push.

No force-pushes to `main`. No skipping hooks / signing. No `--amend`
after pre-commit hook failure (create a NEW commit instead). Specific
file paths for `git add`, never `git add -A`.

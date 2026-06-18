# RustyNES v2 — Real-Game Regression Recovery (post-10995f1)

**Date:** 2026-05-17
**Status:** Plan (supersedes prior C1/AccuracyCoin plan — that work is captured by the commit history on `main` and will be preserved on a recovery branch; the *task* has changed).
**Trigger:** User-reported regression: Super Mario Bros., Excitebike, and Kid Icarus (PAL) — all working perfectly at `10995f1e14112c563360cb94bc9efc3420b1fbbd` (2026-05-11) — fail in the current HEAD `9e2801e`. Symptoms: stuck on title screen after START, missing motorcycles in Excitebike, missing selection indicators in Kid Icarus.

---

## What this plan does (and does NOT do)

**Does:**
1. **Preserve** every commit between `10995f1` and current `main` HEAD `9e2801e` on a new branch so no accuracy work is lost.
2. **Reset `main` to `10995f1`** (the last known-good real-game baseline) and force-push to `origin/main` so the public branch is immediately usable again.
3. **Investigate** the regression on the new branch using a hypothesis-prioritized scientific bisect (NOT a blind binary bisect — the suspect set is small and the symptom set is specific).
4. **Fix** the load-bearing accuracy changes so they preserve real-silicon behavior on commercial games *and* keep the test-ROM accuracy gains. Where a change is provably wrong (vs. a correct fix that exposes a different bug elsewhere), revert it. Where a change is correct but interacts badly with another commit, identify and fix the interaction.
5. **Validate** with both the test-ROM suite (510 strict + AccuracyCoin floor) and the three named commercial games (SMB / Excitebike / Kid Icarus PAL) before considering the recovery branch mergeable.

**Does NOT:**
- Re-execute the C1 IRQ-timing rework or AccuracyCoin ≥ 90% work. Those are the v1.0.0 gates — they'll be planned again *after* the regression is fixed. The prior plan's deliverables (`docs/STATUS.md`, ADR-0002, etc.) are still valid as reference.
- Make any code changes during this plan (plan mode constraint). The actual execution happens after `ExitPlanMode` approval.

---

## Inventory: commits between `10995f1` and current `9e2801e` (newest first)

Mining the 32 commits since the May-11 baseline, classified by regression-risk for real games (NROM/MMC1 hot paths affecting SMB / Excitebike / Kid Icarus PAL):

| Commit  | Title | Risk | Why                                                  |
|---------|-------|------|-----------------------------------------------------|
| `9e2801e` | test(cpu_interrupts_v2) refine probe shape | NONE | test fixture only |
| `967180c` | chore(traces) regen IRQ traces | NONE | test data |
| `311d4a6` | feat(bus) invert OAM DMA alignment parity | **HIGH** | DMA cycle count change — affects every game that uses `$4014` (SMB / Excitebike / Kid Icarus all do) |
| `48b5983` | feat(cpu) M2-low CPU IRQ sample | MED | IRQ-only — SMB/Excitebike have no IRQ. Kid Icarus MMC1 has no IRQ. *Should* not affect these games but worth ruling out. |
| `0fb4ac7` | docs(STATUS) | NONE | docs |
| `b996565` | fix(ppu) sprite-Y → scanline-N+1 semantics | **HIGH** | Direct change to sprite rendering Y-coordinate semantics — could explain "missing motorcycles" / "missing selection indicators" verbatim |
| `a6624a5` | feat(ppu) $2007 during-rendering v-increment quirk | MED | Only affects games that write `$2007` during rendering (rare in title screens) |
| `593a8c7` | feat(cpu,bus,mappers) seven 6502 bus-pattern fixes | **HIGH** | Includes `$4016`/`$4017` open-bus semantics change — could break controller-bit-5/6/7 detection — could explain "stuck on title screen — START not detected" verbatim. Also page-cross dummy reads, JSR cycle order, branch dummies, STA always-dummy. |
| `9c00d3c` | feat(harness) AccuracyCoin RAM decoder | NONE | test harness |
| `401f199` | feat(mmc3) Sharp/NEC reload-pending IRQ discriminator | LOW | MMC3-only. Kid Icarus PAL is MMC1 — unaffected. |
| `064425f` `6669f1e` | docs(session-end) | NONE | docs |
| `df07ae3` | docs(c1) Phase B4 attempt rollback | NONE | docs only (revert ship) |
| `c8b7ce6` | feat(bus,cpu) phase-aware IRQ API (B2+B3) | LOW | Semantics-preserving refactor per the commit message; `poll_irq_at_phase(High)` ≡ old `poll_irq` |
| `12949c3` | refactor(scheduler) expose M2Phase | NONE | pure plumbing |
| `d7d4c98` | feat(irq-trace) two-phase IRQ sampling | NONE | gated on `irq-timing-trace` feature, default off |
| `8084435` `aa610ee` | docs(session-end) | NONE | docs |
| `874902a` | test(harness) M2-phase IRQ tracing fixture | NONE | test harness, feature-gated |
| `717cafe` | docs(adr) ADR-0002 Decision section | NONE | docs |
| `a8c31f8` | feat(apu) polyphase BLEP / windowed-sinc | LOW-MED | Audio synthesis change — could cause "sound issues" but not visual ones |
| `3c8811e` `63d8dea` `0e857db` | feat(ppu) B8 cycle-resolution sprite-eval FSM (a/b/c) | **HIGH** | Sprite evaluation cycle-by-cycle FSM. Verify whether these landed before or after `10995f1` (date check is a Phase 3 sub-step). |
| `7ca6766` | feat(vrc7) VRC7 base mapper | NONE | New mapper — no impact on SMB/Excitebike/Kid Icarus |
| `3d82b35` | feat(mmc5) MMC5 audio | NONE | MMC5-only |
| `c6a0bc2` | feat(n163) Namco 163 audio | NONE | N163-only |
| `f8393b2` | fix(dmc) bytes_remaining underflow | LOW | DMC-only |
| `b3d5a62` | feat(sunsoft5b) audio | NONE | Sunsoft 5B-only |
| `7c65718` | ci/docs no_std gate | NONE | CI |
| `d2b8fcc` | feat(test-harness) AccuracyCoin harness | NONE | test harness |
| `050ddbb` | feat(no_std) chip stack no_std | MED | Workspace-wide refactor; could have introduced a subtle behavior change. Low likelihood but worth a diff audit. |
| `68713dd` | build(deps) thiserror 1→2 | NONE | dep bump |

**Highest-prior suspects (in priority order):**

1. **`b996565` — sprite-Y semantics** — direct match to "missing motorcycles / selection indicators."
2. **`593a8c7` — $4016/$4017 open-bus bits 5-7** — direct match to "stuck on title screen after START."
3. **`311d4a6` — OAM DMA alignment parity inversion** — affects sprite refresh timing for every game.
4. **`0e857db / 63d8dea / 3c8811e` — B8 sprite-eval FSM** (if landed post-10995f1; verify in Phase 3).
5. Everything else.

---

## Phase 0 — Pre-flight (read-only, already done in this planning session)

- Enumerated 32 commits post-`10995f1`.
- Classified by hot-path risk (above table).
- Confirmed `origin/main` matches local `main` at `9e2801e` (force-push will be a clean rewind, no merge conflicts).
- Confirmed commercial ROMs are **not** in the repo (per CLAUDE.md). Recovery testing depends on the user supplying ROM paths, either under the gitignored `tests/roms/external/` or via `cargo run --release -p nes-frontend -- <path>`.
- Confirmed visual-regression snapshots at `crates/nes-test-harness/tests/snapshots/` only cover small homebrew that did NOT catch this regression.
- Identified active worktree `agent-a8d5e43d1b4173398` (locked) running task #51.

---

## Phase 1 — Preserve current state + roll main back

**One atomic sequence.** Phase 1 is the only destructive sub-section of the plan; everything else is non-destructive and reversible.

1. **Stop and remove the active sub-agent worktree.** Task #51 supersedes by this new task.
   - Load deferred tools `TaskStop` / `TaskList` / `TaskGet` via `ToolSearch`.
   - `TaskStop` task #51.
   - `git worktree unlock .claude/worktrees/agent-a8d5e43d1b4173398`.
   - `git worktree remove --force .claude/worktrees/agent-a8d5e43d1b4173398`.
   - `git branch -D worktree-agent-a8d5e43d1b4173398 worktree-agent-a789e8f2dc0447f31 worktree-agent-aa4d2d30b6518e230` (stale agent branches; the worktree command will already have cleaned the active one).

2. **Create the recovery branch from current main HEAD.** Default name proposed: **`accuracy-stabilization`** (user confirms via clarifying question). This branch preserves every commit between `10995f1..9e2801e`. *No work is lost.*
   - `git branch accuracy-stabilization 9e2801e`
   - `git push -u origin accuracy-stabilization`

3. **Reset `main` to `10995f1`.**
   - `git reset --hard 10995f1`
   - `git push --force-with-lease origin main`
   - **Warning:** force-push to `main` is destructive. The user explicitly requested it. `--force-with-lease` aborts safely if any third party has pushed in between — the maximally cautious form of the operation.

4. **Verification:**
   - `git log --oneline main -5` → top line `10995f1 chore(repo): rename GitHub URL parobek/RustyNES_v2 → doublegate/...`.
   - `git log --oneline accuracy-stabilization -5` → top line `9e2801e test(cpu_interrupts_v2)...`.
   - `git log --oneline accuracy-stabilization ^main | wc -l` → 32 (every recovery-candidate commit accounted for).
   - `git ls-remote origin main` → matches `10995f1`.

5. **Check out the recovery branch:** `git checkout accuracy-stabilization`. All further work happens here.

6. **Acceptance gates for Phase 1 (must all be green before Phase 2):**
   - `cargo build --workspace` — recovery branch builds.
   - `cargo test --workspace --features test-roms` — current test count (510 strict + 5 ignored).
   - User manual smoke (one ROM, e.g. SMB) confirms the regression *also reproduces on the recovery branch* (proves we haven't accidentally "fixed" it just by checking out the branch).

---

## Phase 2 — Build minimal repro infrastructure

The visual-regression suite at `crates/nes-test-harness/tests/visual_regression.rs` covers `sprint-2/full_palette.nes`, `sprint-2/flowing_palette.nes`, `blargg/ppu_vbl_nmi/01-vbl_basics.nes`, `blargg/instr_test_v5/01-basics.nes`. None of these stress sprite rendering or controller input the way commercial games do, and the snapshots were regenerated as the suspect commits landed — so the suite did NOT detect this regression.

Two practical strategies:

**Option A — User supplies dumps under `tests/roms/external/` (gitignored)** and we write a non-committed visual-bisect harness at `crates/nes-test-harness/tests/external_real_games.rs` (also gitignored or feature-gated). The harness:
- Runs 600 frames each of SMB / Excitebike / Kid Icarus PAL with a recorded title-screen input script (idle → START tap → gameplay).
- Captures the framebuffer FNV-1a hash at frames `{300, 600}` for each ROM.
- Bisects by re-running at each candidate commit and diffing the hash against a baseline captured at `10995f1`.
- Gated on a `commercial-roms` feature so CI never runs it.

**Option B — User-driven manual bisect:** `cargo run --release -p nes-frontend -- <rom>` at each candidate commit; user visually reports OK / BROKEN. No infrastructure needed but slow.

**Recommendation: Option A** (deterministic, automatable, reusable for future regressions). Falls back to Option B if the user prefers / can't stage ROMs.

This phase is bypassed for Option B.

---

## Phase 3 — Hypothesis-prioritized scientific bisect

Each step: revert ONE commit on `accuracy-stabilization` via `git revert -n <sha>` (so the revert sits in the working tree, not yet committed). Build. Run the validation set. Decide.

**Validation set per step:**
- `cargo test --workspace --features test-roms` (must stay green or lose ≤ small countable tests with explanation).
- The real-game smoke test (Phase 2 harness or user manual run) on SMB / Excitebike / Kid Icarus PAL.

### Step 3.1 — Revert `b996565` (sprite-Y → scanline-N+1)

- **Prediction:** Excitebike motorcycles + Kid Icarus selection indicators reappear. SMB title-screen behavior may also normalize.
- **If TRUE (sprite issue fixed):** The commit's intent was correct per nesdev wiki (sprite Y byte == "line BEFORE the sprite renders") but the implementation likely has an off-by-one. Audit the three sites in `crates/nes-ppu/src/ppu.rs` (`tick_sprite_eval_per_dot`, `fetch_sprite_tile`, `reference_eval` test fixture) against Mesen2's `Mesen2/Core/NES/PPU/NesPpu.cpp`. Special-case for pre-render (-1 vs 0) is the likely culprit.
- **If FALSE (sprite still broken):** Keep the revert *pending* and continue to Step 3.2 — the bug may be additive.

### Step 3.2 — Revert `593a8c7` (7 6502 bus-pattern fixes)

- **Prediction:** "Stuck on title screen" fixes for SMB (and possibly the others) if `$4016`/`$4017` open-bus bits 5-7 misreporting broke START detection.
- **If TRUE:** Audit the 7 patterns separately. Most likely culprits in order:
  1. `$4016/$4017` bits 5-7 computed from `self.open_bus & 0xE0` instead of the previously-hardcoded `0x40`. Re-verify against `nesdev.org/wiki/Standard_controller#$4016/$4017_reads` and Mesen2's controller emulation.
  2. NOP DOP/TOP dummy reads for `$04/$44/$64/$14/$34/$54/$74/$D4/$F4/$0C` opcodes — if SMB executes one of these opcodes with a side-effecting target address, the dummy read could have side effects.
  3. STA always-dummy for `$9D/$99/$91` — could land on a PPU/APU register and corrupt state.
- **Strategy:** Don't permanently revert the whole commit — surgically re-implement each of the 7 patterns one at a time, validating that the AccuracyCoin gains aren't lost AND SMB still works.
- **If FALSE:** Continue to Step 3.3.

### Step 3.3 — Revert `311d4a6` (OAM DMA alignment parity)

- **Prediction:** Sprite refresh timing on frames-after-DMA normalizes.
- **If TRUE:** The commit's intent (DMA-aligned-on-get vs put cycles per APU clock half, nesdev:DMA#OAM_DMA) is right under one phase convention but conflicts with the project's `M2Phase` model. Re-align rather than revert outright.
- **If FALSE:** Continue to Step 3.4.

### Step 3.4 — Verify B8 sprite-eval FSM (`0e857db / 63d8dea / 3c8811e`) date

- **First verify** whether these three commits landed before or after `10995f1` (`git log --oneline 10995f1..HEAD | grep -iE 'sprite.eval|b8'`).
- If BEFORE → skip; they're in the working baseline.
- If AFTER → strong suspect. Strategy: do NOT revert the whole feature (it shipped a 1013-case equivalence harness). Instead, add a temporary feature flag `sprite-eval-legacy` that selects the pre-FSM single-shot path. A/B-compare per-scanline / per-dot / per-OAM-byte to localize the FSM divergence.

### Step 3.5 — Other suspects (only if 3.1-3.4 don't close the gap)

- `48b5983` (M2-low IRQ sample) — only if SMB/Excitebike/Kid Icarus PAL use IRQ. Unlikely.
- `a6624a5` (`$2007` during-rendering quirk) — only if any of these games write `$2007` during active rendering.
- `a8c31f8` (polyphase BLEP) — would produce audio glitches only, not visual breakage.
- `050ddbb` (`no_std` migration) — workspace-wide refactor diff audit.

If none of these close the gap, escalate to **full `git bisect run`** over the 32-commit range with the Phase 2 harness as the test command.

---

## Phase 4 — Synthesize fixes (NOT blanket reverts)

For each commit identified by Phase 3 as load-bearing for the regression:

1. **Read the commit's original justification** in `CHANGELOG.md` `[Unreleased]` and in any cross-referenced ADR / nesdev source.
2. **Read upstream authority** — nesdev wiki for the specific behavior; Mesen2 source (via Context7 or GitHub fetch) for the canonical reference impl. Compare commit-as-implemented vs. authority.
3. **Categorize:**
   - **Wrong implementation of a real-silicon behavior** → fix it; keep the spirit, correct the off-by-one / wrong-phase / wrong-bit-mask.
   - **Correct implementation that masks a different bug elsewhere** → investigate the *other* bug.
   - **Correct implementation that conflicts with another correct change** → coordinate via an explicit phase reference (see `docs/adr/0002-irq-timing-coordination.md` for the `M2Phase` pattern).
   - **Genuinely wrong fix** → revert; document under `CHANGELOG.md` `[Unreleased]` → "Investigated and rolled back"; mark the AccuracyCoin sub-tests it was meant to fix as outstanding.

4. **Per-fix validation:**
   - `cargo fmt --all --check && cargo clippy --workspace --all-targets --features test-roms -- -D warnings`.
   - `cargo test --workspace --features test-roms` — counted test-count delta.
   - Real-game smoke — all 3 named games must work.
   - **AccuracyCoin pass rate** must not regress below the prior 69.06% / floor 0.60. If a fix forces a regression below floor, document the trade-off and ask the user (real-game playability > test-ROM pass rate, per the user's stated intent).

---

## Phase 5 — Validate + document + propose merge

Once all named games work on `accuracy-stabilization`:

1. **Full quality gate**:
   - `cargo fmt --all --check`
   - `cargo clippy --workspace --all-targets --features test-roms -- -D warnings`
   - `cargo doc --workspace --no-deps`
   - `cargo test --workspace --features test-roms` (≥ 510 strict)
   - `cargo test --workspace --no-default-features` (no_std preservation)
   - `cargo test -p nes-test-harness accuracycoin_pass_rate_meets_floor --release -- --nocapture` (floor 0.60)
   - User manual run of SMB / Excitebike / Kid Icarus PAL.

2. **Update `CHANGELOG.md` `[Unreleased]`** with a "Real-game regression recovery (2026-05-17)" section listing every commit revisited and its disposition (fix-of-fix landed / reverted / re-validated as correct).

3. **Update `docs/STATUS.md`** with the new strict / ignored counts.

4. **Propose merge to user.** Two paths:
   - **Fast-forward:** `git checkout main && git merge accuracy-stabilization --ff-only` (works if `main` is still at `10995f1`).
   - **Squash-merge:** Squash the recovery into 1-5 thematic commits on `main` for history clarity. User decides.

5. **Post-merge cleanup:** delete `accuracy-stabilization` locally and on origin; re-run all visual-regression snapshots; commit any that legitimately changed.

---

## Risks + mitigations

| Risk | Mitigation |
|------|-----------|
| Force-pushing `main` destroys collaborator work | `--force-with-lease`; single-author repo per known state |
| `accuracy-stabilization` accidentally rebases / drops commits | Push immediately after creation; verify `git log --oneline accuracy-stabilization ^main \| wc -l` = 32 before any reset |
| Phase 2 harness fragile on edge mappers | Start with SMB (NROM); add Excitebike + Kid Icarus PAL only after SMB repro'd |
| Bisect reveals MULTIPLE simultaneous bugs | Step 3 keeps each "TRUE" revert *pending*; compose until games work, then unpick which reverts are truly load-bearing |
| Phase 4 finds a "correct fix that masks an older bug" | Pause and re-plan with user before pursuing |
| AccuracyCoin pass rate regresses below 0.60 floor | Phase 4 step validates per-fix; if below floor, document trade-off and ask user |
| Regression actually in a commit we classified LOW | Step 3.5 escalates to full `git bisect run` over all 32 |
| Task #51's worktree contains uncommitted work | Worktree is an isolated experimental space; killing it loses no committed work |
| User cannot supply commercial ROM dumps | Fall back to Option B (manual frontend validation) |

---

## Open questions for the user

Asked via `AskUserQuestion` **after** plan acceptance, immediately before Phase 1 execution:

1. **Recovery branch name** — proposed `accuracy-stabilization` (alternatives offered).
2. **Phase 2 strategy** — Option A (user stages ROMs under `tests/roms/external/`, automated bisect harness) vs Option B (manual frontend validation at each candidate commit).
3. **Force-push acknowledgement** — confirm `--force-with-lease` to `origin/main` is acceptable.

---

## What runs first after `ExitPlanMode` approval

1. Load deferred tools via `ToolSearch` (`TaskStop`, `TaskList`, `TaskGet`).
2. Ask the 3 open questions above.
3. Execute Phase 1 as a single atomic sequence (stop task #51 → create branch → push → reset → force-push → verify).
4. **Pause and report Phase 1 outcome** before starting Phase 2.

Phase 1 is destructive (force-push). Phases 2-5 are non-destructive.

# `scripts/regression-bisect/` — Automated `git bisect` Workflows

Permanent home for the regression-bisect tooling that survived the
FSM-recovery escapade of May 2026 (commit `834be9e`). These scripts
used to live in `/tmp/rustynes-bisect/.bisect_run.sh` — which CachyOS
wipes on every reboot. Re-deriving them from the bisect commit
message took ~30 minutes during recovery; committing them removes
that cost for the next regression.

## Files

- **`bisect_runner.sh`** — generic `git bisect run` wrapper. Builds the
  selected harness, runs the test binary, and translates the exit code
  into bisect's three-way semantics (`0` good, `1` bad, `125` skip).
  Parameterized via env vars so a single script can drive any harness:
  `external_real_games`, `audio_tests`, `m22`, `mmc1_a12`,
  `visual_regression`, `mmc5`, `holy_mapperel`, `accuracycoin`, etc.

- **`worktree_overlay.sh`** — helper for the temp-worktree pattern. Use
  when the bisect range pre-dates the harness file's existence: it
  creates a `/tmp/rustynes-bisect-$$/` worktree at a target commit,
  copies a harness file from the CURRENT HEAD as untracked, patches
  `crates/rustynes-test-harness/Cargo.toml` to add the feature flag, and
  symlinks `tests/roms/external/` (if present) so the harness can find
  its ROMs. Exits the temp worktree cleanly via a `trap` on EXIT.

- **`README.md`** — this file.

---

## Recipe 1: real-game regression suspected

> Symptom: a commercial ROM at `tests/roms/external/` rendered fine at
> commit X but is broken at HEAD. Goal: find the first-bad commit.

The harness covers **60 commercial ROMs** across 20 mapper subdirs;
54 produce strict-pass baselines, 6 are `#[ignore]`'d at landing time
for boot-to-uniform-frame issues (see "Per-ROM `#[ignore]` policy" in
`tests/external_real_games.rs`). Bisect against the **whole** harness
(any one ROM regressing fires BAD) or a **single** ROM
(`HARNESS_FILTER=external_<mapper>_<short_name>`).

```bash
# Bisect across all 54 passing ROMs (slow but exhaustive — ~150 s per
# commit; suitable when the regression is unknown / cross-mapper).
HARNESS_TEST=external_real_games \
HARNESS_FEATURE=commercial-roms,test-roms \
./scripts/regression-bisect/bisect_runner.sh
echo $?  # expect 1 (BAD)

# Bisect a SINGLE ROM (fast — ~3 s per commit; suitable when you know
# which game broke). HARNESS_FILTER matches against the test function
# name; partial substring matches are fine.
HARNESS_TEST=external_real_games \
HARNESS_FEATURE=commercial-roms,test-roms \
HARNESS_FILTER=external_mmc3_super_mario_bros_3 \
./scripts/regression-bisect/bisect_runner.sh

# Bisect a whole MAPPER (e.g., when an MMC3 commit broke multiple
# games — the filter substring picks all 6 MMC3 ROMs).
HARNESS_TEST=external_real_games \
HARNESS_FEATURE=commercial-roms,test-roms \
HARNESS_FILTER=external_mmc3_ \
./scripts/regression-bisect/bisect_runner.sh

# Once the runner reports BAD at HEAD + GOOD at the known-good commit,
# fire the actual bisect:
git bisect start
git bisect bad   HEAD
git bisect good  <known-good-sha>
git bisect run ./scripts/regression-bisect/bisect_runner.sh
git bisect log; git bisect reset
```

The 60-ROM corpus is the load-bearing sentinel for commercial-game
regressions. Per the `feedback_emulator_fsm_mid_cycle_clobber` memory:
the parallel-impl equivalence harness that compared only END-OF-STEP
state missed the mid-scanline clobber that broke every commercial
game. The 21-ROM committed-test corpus (audio_tests / m22 / mmc1_a12)
covers the **structural** mapper-extension regression surface; the
60-ROM commercial corpus covers the **integration** surface where the
chip + mapper + audio + scheduler interactions can mis-compose without
any single chip-level test going red. Always run both during recovery.

## Recipe 2: committed test-ROM regression suspected

> Symptom: one of the harness tests (e.g. `audio_db_vrc6a`,
> `m22_vrc2a_chr_banking_0_127`) started failing. Goal: find when the
> hash drifted.

```bash
# No overlay needed — the ROMs are committed under tests/roms/.
git bisect start
git bisect bad   HEAD
git bisect good  <last-known-passing-sha>

HARNESS_TEST=audio_tests \
HARNESS_FEATURE=test-roms \
HARNESS_FILTER=audio_db_vrc6a \
git bisect run ./scripts/regression-bisect/bisect_runner.sh

git bisect log; git bisect reset
```

`HARNESS_FILTER` lets you bisect a SINGLE failing test even when other
tests in the same file have unrelated issues — passes the filter
through to `cargo test ... -- <filter>`.

## Recipe 3: harness file pre-dates the bisect range

> Symptom: you want to run a NEW harness against OLD commits. The
> harness file doesn't exist before some date, so `cargo test --test
> <name>` would compile-error on those commits — which the bisect
> would mark as SKIP and lose information.

Use the worktree-overlay pattern:

```bash
# Interactive: spawn a sub-shell in a temp worktree with the NEW
# harness file overlaid.
./scripts/regression-bisect/worktree_overlay.sh <commit> \
    --harness crates/rustynes-test-harness/tests/audio_tests.rs \
    --harness crates/rustynes-test-harness/tests/common/mod.rs \
    --feature 'test-roms = []'

# In the sub-shell:
cargo test -p rustynes-test-harness --features test-roms --test audio_tests
```

To AUTOMATE this inside `git bisect run`, write a small wrapper:

```bash
# /tmp/my-bisect.sh
#!/usr/bin/env bash
set -u
COMMIT=$(git rev-parse HEAD)
./scripts/regression-bisect/worktree_overlay.sh "$COMMIT" \
    --harness crates/rustynes-test-harness/tests/audio_tests.rs \
    --harness crates/rustynes-test-harness/tests/common/mod.rs \
    --feature 'test-roms = []' <<'OVERLAY_CMDS'
cd /tmp/rustynes-bisect-$$
HARNESS_TEST=audio_tests HARNESS_FEATURE=test-roms \
    /home/parobek/Code/RustyNES/scripts/regression-bisect/bisect_runner.sh
exit $?
OVERLAY_CMDS
```

(The actual recovery used `cargo test` directly inside the worktree;
see the commit message of `834be9e` for the exact form.)

---

## Capturing a baseline at any commit

To re-capture the framebuffer/audio baselines from a known-good
commit (e.g. after a deliberate accuracy change moves the hash):

```bash
# 1. Check out the target commit (or use worktree_overlay if you don't
#    want to disturb HEAD).
./scripts/regression-bisect/worktree_overlay.sh <good-sha>

# 2. In the sub-shell:
RUSTYNES_DUMP_FRAMES=1 INSTA_UPDATE=auto INSTA_FORCE_PASS=1 \
    cargo test -p rustynes-test-harness --features test-roms \
    --test audio_tests --test m22 --test mmc1_a12 \
    -- --test-threads=1 --nocapture

# 3. Visually verify the PNGs under /tmp/rustynes-baseline-screenshots/.

# 4. If they look correct, copy the .snap.new files back to the
#    accuracy-stabilization branch and rename to .snap.
```

### Commercial-roms baseline capture (60 ROMs, requires staged dumps)

```bash
# 1. Stage the 60 ROMs at tests/roms/external/<mapper-NNN-NAME>/...
#    (see tests/roms/external/README.md for the canonical-dump SHAs).

# 2. Run the harness with insta auto-write + PNG dump. --test-threads=1
#    keeps insta diff order deterministic and the PNG output filesystem
#    order readable.
INSTA_UPDATE=auto RUSTYNES_DUMP_FRAMES=1 \
    cargo test -p rustynes-test-harness --features commercial-roms,test-roms \
    --test external_real_games -- --test-threads=1

# 3. Promote .snap.new -> .snap (insta default workflow):
cargo insta accept
#  OR manually:
#  cd crates/rustynes-test-harness/tests/snapshots
#  for f in external_real_games__*.snap.new; do mv "$f" "${f%.new}"; done

# 4. Visually verify the PNGs at /tmp/rustynes-baseline-screenshots/external/
#    Each ROM is one PNG (or three, for the 3 StartTap-script ROMs).
#    Spot-check at least one per mapper; flag any uniform-black /
#    uniform-gray frames as suspect and `#[ignore]` the test rather
#    than locking in a baseline of broken behavior.
```

A full 60-ROM capture run takes ~150 s on dev profile. The 6 `#[ignore]`'d
tests at the time of writing (Tiny Toon Adventures 2, Fire Emblem
Gaiden, Ganbare Goemon 2, Esper Dream 2, Madara, Mr. Gimmick) boot to
uniform-color frames at f600 — likely longer intro sequences or
mapper-decoder edge cases that need separate investigation. See
`docs/testing/baselines-audit.md` for the full per-ROM rationale.

---

## Diagnostic output interpretation

`bisect_runner.sh` writes a one-line `[bisect_runner]` summary at every
exit. The shape:

```
[bisect_runner] cargo build failed (rc=101) — SKIP
[bisect_runner] cargo test PASS — GOOD
[bisect_runner] cargo test FAIL (rc=1) — BAD
```

If `git bisect run` reports `bisect run cannot continue any more`,
either:

- the bisect found the first-bad commit (check `git bisect log`), or
- too many commits in the range are SKIP (build broken). Widen the
  range with `git bisect good <earlier-sha>` and retry.

## Shell-redirection gotcha

When capturing a bisect log for later analysis:

```bash
# WRONG — sends stderr to TERMINAL, stdout to file.
git bisect run ./scripts/regression-bisect/bisect_runner.sh 2>&1 > log

# RIGHT — sends both to file.
git bisect run ./scripts/regression-bisect/bisect_runner.sh > log 2>&1
```

The first form bit the FSM-recovery bisect. Order matters: redirections
are processed LEFT-TO-RIGHT, so `2>&1` duplicates whatever stdout points
to AT THAT MOMENT, then `> file` reassigns stdout. The fix is to do the
`> file` first.

---

## Cross-references

- `tests/external_real_games.rs` — the FSM-recovery oracle. Feature-gated
  on `commercial-roms` (off by default). ROMs are user-supplied under
  `tests/roms/external/` (gitignored).
- `tests/audio_tests.rs` — bbbradsmith expansion-audio corpus, committed.
- `tests/m22.rs` — VRC2a CHR-banking, committed.
- `tests/mmc1_a12.rs` — MMC1 A12-transition control, committed.
- `tests/visual_regression.rs` — small-ROM framebuffer baselines,
  committed.
- `~/.claude/projects/-home-parobek-Code-RustyNES-v2/memory/feedback_emulator_fsm_mid_cycle_clobber.md` —
  bug-pattern lesson that motivated this entire infrastructure.
- `docs/testing/baselines-audit.md` — the static record of every baseline
  hash committed in the harness, for future-Claude / future-human
  cross-reference.

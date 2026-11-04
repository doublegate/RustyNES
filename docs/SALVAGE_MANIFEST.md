# Salvage Manifest

Record of files rescued from volatile `/tmp` into the project tree.

## 2026-06-13 — tmp-salvage (copy, not move)

Files were **copied** (originals left in `/tmp`) because they were in active
use by a concurrent Claude Code instance at salvage time.

| Source | Destination | Notes |
|---|---|---|
| `/tmp/RustyNES-synth/ux3-bugfixes-spec.md` | `salvaged/RustyNES-synth/ux3-bugfixes-spec.md` | Hand-written UX bugfix spec |
| `/tmp/RustyNES-synth/ux6-spec.md` | `salvaged/RustyNES-synth/ux6-spec.md` | Hand-written UX round-6 spec |
| `/tmp/RustyNES-synth/ux-overhaul-spec.md` | `salvaged/RustyNES-synth/ux-overhaul-spec.md` | Hand-written UX overhaul spec |
| `/tmp/RustyNES-synth/v1.0.0-release-notes.md` | `salvaged/RustyNES-synth/v1.0.0-release-notes.md` | v1.0.0 release notes draft |
| `/tmp/tests_nes_hashes.txt` | `docs/tests_nes_hashes.txt` | NES ROM SHA-256 hash manifest |
| `/tmp/active_docs_to_rename.txt` | `docs/active_docs_to_rename.txt` | Doc-rename worklist |

**Dropped as noise (not salvaged):** 13 `g*.log` cargo build/doc logs
(regenerable); 7 `hermes-*` files (belong to Hermes Agent / Undertow, not
RustyNES); `/tmp/dumps/settings.dat` (generic binary, unknown origin).

## 2026-06-15 — tmp-salvage (move, curated)

Curated rescue after the icon work + history rewrite. Files **moved** out of
`/tmp` (originals removed). Only small, non-regenerable items kept.

| Source | Destination | Notes |
|---|---|---|
| `/tmp/RustyNES-synth/doc-sync-brief.md` | `docs/archive/synth-specs/doc-sync-brief.md` | v1.0.0 synthesis doc-sync brief |
| `/tmp/RustyNES-synth/ux3-bugfixes-spec.md` | `docs/archive/synth-specs/ux3-bugfixes-spec.md` | UX round-3 bugfix spec |
| `/tmp/RustyNES-synth/ux4-docs-brief.md` | `docs/archive/synth-specs/ux4-docs-brief.md` | UX #4 docs-update brief |
| `/tmp/RustyNES-synth/ux4-spec.md` | `docs/archive/synth-specs/ux4-spec.md` | UX #4 implementation spec |
| `/tmp/RustyNES-synth/ux6-spec.md` | `docs/archive/synth-specs/ux6-spec.md` | UX round-6 spec |
| `/tmp/RustyNES-synth/ux-overhaul-spec.md` | `docs/archive/synth-specs/ux-overhaul-spec.md` | UX overhaul combined spec |
| `/tmp/RustyNES-synth/v1.0.0-release-notes.md` | `docs/archive/synth-specs/v1.0.0-release-notes.md` | v1.0.0 release notes |
| `/tmp/tests_nes_hashes.txt` | `tests/nes-rom-sha256.txt` | 486-entry NES ROM SHA-256 reference |

**Dropped as noise (not salvaged):** all 42 cargo/clippy/CI/test gate logs
(regenerable); all `fds-*` / `rustynes-fds*` / `rustynes-compat` frame-dump PNGs
(debug captures, regenerable from ROMs); `/tmp/RustyNES/` (session scratch +
`iconvenv` Python venv); `rustynes-icon-research/` (10.5 MB third-party reference
photos — re-downloadable, kept out of the public repo); the `hermes-*` files
(Hermes Agent, not RustyNES); empty/scratch `*.txt`/`*.log`.

## 2026-07-14 — tmp-salvage (copy, curated dev tooling)

Post-v2.2.0 "Capstone" rescue. Swept both `/tmp` and the Claude Code scratchpad
tree `/tmp/claude-1000/…RustyNES/…/scratchpad/` (the latter is excluded by the
salvage script's default noise list, so it was curated by hand). Files were
**copied** (originals left in `/tmp`, which the tmpfs wipes on reboot regardless).

**Correction on first pass:** the Game Genie header-robust re-key already
**shipped in PR #262** (v2.1.3 follow-up) — the repo's
`crates/rustynes-frontend/src/genie_database_headerless.tsv` (16,508 rows, wired
into `genie_db.rs`) and `scripts/gg/{gen_headerless_genie_db.py, alias_crcs.py,
nes20db.xml, README.md}` were already tracked. So the salvaged TSV was
byte-identical to the shipped file, `alias_crcs.py` was identical, and
`gen_headerless.py` was a superseded earlier draft — all three dropped as
redundant. Only the **intermediate research / verification scripts** that were
never committed with #262 are genuinely additive; they were placed **beside** the
shipped pipeline in `scripts/gg/` and committed.

| Source | Destination | Notes |
|---|---|---|
| `…/6112789e/scratchpad/gg-research/{crc_combine,alias_resolve,coverage,coverage2,inspect,verify}.py` | `scripts/gg/` (committed) | The 6 intermediate GG re-key research/verification helpers not committed with #262 — CRC32-combine derivation, alias CRC resolution, coverage accounting, output spot-check + round-trip verify |
| `…/gg-research/{genie_database_headerless.tsv, alias_crcs.py, gen_headerless.py, nes20db.xml}` | — (dropped) | Redundant: TSV + `alias_crcs.py` byte-identical to shipped #262; `gen_headerless.py` a superseded draft; `nes20db.xml` already at `scripts/gg/` (gitignored) |
| `…/6112789e/scratchpad/{batch2,convert_gg,enumerate_staged,gen_promotion,ids,mapper_scan,scan,show252,showthreads,threads}.py` + `promo_tests{,_2}.rs` | `scripts/mapper-promotion/` (untracked) | Mapper tier-promotion tooling + Rust harness — on disk for reboot survival, not committed |
| `/tmp/probe_rev.rs` | `scripts/probes/` (committed) | 2A03-revision (Rp2A03G vs H) DMA divergence probe behind the shipped v2.1.7 "Stepping" DMA-frontier work (ADR 0033) — mirrors the PR #304 gg-research precedent for post-hoc diagnostic tooling |
| `/tmp/check_dirs.rs`, `/tmp/dsp_debug_test.rs` | `scripts/probes/` (untracked) | Dropped on review: `check_dirs.rs` is RustySNES content (a false-positive cross-project match), `dsp_debug_test.rs` is self-labeled scratch — neither committed |

**Dropped as already-persisted or regenerable (not salvaged):** ~1,751
`ww-head/` and `ww-movie/` PNG frames (Wizards & Warriors debug captures — that
bug is resolved, ADR 0031); ~61 logs; all `reply*.md` (PR bot-comment replies already posted to
GitHub); all `pr_body*.md` / `*-pr.md` / `v2.2.0.md` (PR bodies + release notes
already on GitHub / committed at `.github/release-notes/`); all `*.diff` /
`280_ppu.diff` / `changelog.diff` / `roadmap.diff` / `versionplan.diff` (merged
into git history); all `*-baseline*.md` (transient doc-sync comparison snapshots);
thread `.json` dumps (transient API responses); `/tmp/holy-mapperel` git clone
(already in-repo at `tests/roms/holy_mapperel`);
`/tmp/rustynes-mkdocs-test` (6 MB) + `/tmp/rustynes-hm` (4 MB) build/test scratch;
vendored `libretro-database/` + `mkdocs-venv/` upstream/venv scripts.

## 2026-07-14 — tmp-salvage follow-up (confirm-noise sweep, `probe_rev.rs` committed)

Re-ran the dry run plus a by-hand check of `/tmp/claude-1000/`'s own nested
content — its directory name matches the salvage script's `claude-\d+`
agent-scratch exclusion pattern, so the automated scanner skips the whole
tree (including loose files sitting directly at its root) regardless of
what's inside; every prior salvage pass in this project has therefore relied
on a manual look there. Confirmed noise this pass: loose top-level files
(`status_head.md` — a stale STATUS.md snapshot superseded by the current
doc; `reply-status.txt`/`reply-agents.txt` — bot replies already posted to
GitHub; `pal_text.out`/`pal_all.out`/`ntsc_verify.out` — raw `cargo test`
captures from the already-shipped PAL APU oracle work) and
`.../swiftgen/` (regenerable UniFFI-generated Swift bindings output). The
main `/tmp` STRONG-match project directories (`/tmp/RustyNES`,
`/tmp/claude-rustynes`, `/tmp/rustynes-hm`, `/tmp/rustynes-mkdocs-test`) were
all re-confirmed as draft PR/commit/reply text already posted or merged, CI
gate logs, visual-regression dumps from already-shipped mapper regression
testing, and a built MkDocs static site — all spent/regenerable, none
salvaged. Only `scripts/probes/probe_rev.rs` was promoted from untracked
scratch to committed (see the row above) — everything else from the prior
pass stays as previously recorded.

## 2026-08-06 — pre-reboot salvage (manual, curated)

Run before rebooting for an NVIDIA driver/kernel update. The `tmp-salvage` dry
run offered **305 candidates / ~600 MB**; **3 files** were taken. The rest was
deliberately left to be wiped:

| left behind | why |
| --- | --- |
| `perf.data` (586 MB), `probe.perf`, `fp.data`, `di.data`, `bolt.log` | perf recordings and CI logs — regenerable with one command |
| ~100 `.md`/`.json` PR-review files | already posted to GitHub PR #348 |
| `fisk-*.png`, `v2p-*.png` (8 MB) | screenshots, possibly commercial-ROM framebuffers — not swept in unexamined |
| `/tmp/accuracycoin-build/` | third-party MIT source + a Windows `nesasm.exe`; vendoring is a deliberate provenance decision, not a salvage side effect |
| `agy-review.sh`, `agy-review-selftest.sh` | duplicates of files already in `scripts/` and `Local_Only-Projects/antigravity-pr-review/` |
| `p3.sh`, `p4.sh`, `probe*.sh`, `mi.rs` | extraction artifacts (YAML pulled out for `shellcheck`) and a temp probe already reverted on purpose |

Everything built during the session was **already committed** (`frame_probe.rs`,
`ab_check.sh`, `frame_breakdown.sh`, the `perf_log_check.py` p99 gate), so the
scratchpad held no unique source beyond the three below.

| file | → | note |
| --- | --- | --- |
| `reflow.py` | `scripts/release-automation/` | markdown full-width reflow; fixed the release-notes formatting. Existed nowhere else on disk. |
| `assemble.py` | `scripts/release-automation/` | HTML fragment → document (needs `bs4`). |
| `guardrails_assemble.py` | `scripts/release-automation/` | same, for the provenance-guardrails doc. |

Each file was SHA-256 verified identical to its `/tmp` source **at the moment of
copy** — that is the recovery evidence, and it covers the recovery only. All
three have been edited since: `reflow.py` was fixed (hard-break and fence
handling) and both `bs4` scripts gained input validation, in response to the
PR #349 review. The files in the tree are therefore **not** byte-identical to the
recovered artifacts, and nothing here claims they are. See
`scripts/release-automation/README-doc-tools.md` for usage.

## 2026-09-19 — tmp-salvage (copy, curated after verification)

Post-v2.6.22 sweep. `salvage.py --coverage` reached **58,370 files** and proposed
**2,090 candidates**; **37 were claimed**. The gap is not a miss — most of the
proposal was verified regenerable, and the verification is the record below.
Originals were **copied**, not moved, and every copy was `cmp`-verified
byte-identical to its source at the moment of copy.

Destination is `salvaged/`, which is **gitignored** (`.gitignore:345`). These
files survive the reboot; they do not enter git history, `docs/`, or `scripts/`,
so nothing here is walked by `release_anchor_audit.rs` or markdownlint.

### Claimed (37)

| Source | Destination | Notes |
|---|---|---|
| 13 files from session `f63c2db0` scratch | `salvaged/scripts/s-f63c2db0/` | The v2.6.22 mutation harness: `builder_m{1_half,2_inert,2_nocall,3_nobattery}.py` (the mirror-ROM mutants), `mut{,2,3,4}.py`, and the `shqtest{,2,3,4}.sh` / `bsdtest.sh` shell-quoting probes that produced the `shq()` sentinel finding. None exists at any path on HEAD. |
| 20 files from session `0b2858a5` scratch | `salvaged/scripts/s-0b2858a5/` | v2.6.20/21 one-offs: `prog53.py`, `program54.py`, `resync.py`, `to_tabs.py`, `mutate.py`, `mut.py`, `mut2.py`, `frozen.asm`, `mis.asm`, the `t_{brack,plain,who}.sh` `pgrep -f` self-match probes, `trp{,2}.sh`, `b.sh`, `p.sh`, `ci-steps.sh`, `datecheck.sh`, `batch{1,2}.sh`. (An earlier revision of this row enumerated only 16 of the 20 — `b.sh`, `p.sh`, `mut.py` and `mut2.py` were omitted from the prose while the count and the copy were correct. Caught in review on #534.) |
| `identify.txt`, `prov.txt`, `prov2.txt`, `prov3.txt` | `salvaged/docs/s-f63c2db0/` | Measurement captures. `prov.txt` (3 STALE) and `prov2.txt` (2 STALE) record `check_golden_provenance` states that **no longer exist** — the goldens were re-exported, so those two runs are not reproducible. `prov3.txt` (0 STALE) and `identify.txt` are reproducible but are the evidence behind a shipped claim. |

### Found and deliberately NOT claimed — each checked, not assumed

| Group | Count / size | Why not |
|---|---|---|
| `AccuracyCoin.asm` copies | 15 files, ~10 MB | Upstream MIT source the repo deliberately does not vendor, plus `build_*.py`-generated patched variants. Committing it contradicts the project's own rule. |
| `rtlsnap/rtl/*.sv` | 24 files, ~400 KB | **All 24 blobs are in the sibling's git object store.** The 3 that differ from HEAD trace to `61ce761` (the #22 CHR commit). Recoverable from git. |
| `objsave/`, `objsave2/` | 24 files | Verilator-generated `V*_tb.{cpp,h,mk}`. Build output. |
| `rustynes-atomic-1865690-*` | 61 files, 0–30 B | `atomic_write` test turds. |
| `user_io.cpp` | 101 KB | Third-party MiSTer framework source, not ours. |
| `build_sub_test_rom.py` | 18.4 KB | The scratch copy is the **older** one — the tracked copy has 12 lines it lacks. |
| `derive_indices.py` | 10.7 KB | Already tracked at `scripts/accuracycoin-build/derive_indices.py` on HEAD. |
| ~165 `pr*.md` / `msg*.txt` / `reply*.md` / `rel-*.md` / tag bodies | ~380 KB | Already-published text: each is the `-F` input to a commit, PR, review reply, release or tag that landed. |
| `chlog.md`, `sibnotes.md`, `sibling-notes.md`, `v2.6.21-draft.md`, `audit1.txt` | ~46 KB | Verified published by probing the tree and `gh release view`, not inferred from the filename. `sibnotes.md`/`v2.6.21-draft.md` are two drafts of one release body. |
| `header.txt` | 5.9 KB | A **superseded** draft of the `BUILD-PROVENANCE.tsv` header: it reads "since v2.6.23", a version that does not exist; the committed text corrected it to "since 2026-09-19". |
| 4 flagged git dirs | 2.3 MB, 12 MB, 2.3 MB, 1.4 MB | `ac-src`, `ac`, `tri` are clean upstream clones at exactly the vendored commits (`46199ae4`, `94f1b117`) with **0 uncommitted entries** each; `/tmp/dummy_repo` is not this project's. Nothing to hand-salvage. |
| Foreign agent scratch | 11,994 matches, 213 groups | Other projects (MARM-Stack 5,856; TarsGPT 3,524; Grok-Bot 1,913; Sawdust 402). The one tree that could plausibly hold misfiled RustyNES work — the workspace-root slug `-home-parobek-Code` — was opened by name: 9 files, all homelab config. |
| Weak matches | 1,249 files, ~511 MB `data` + 864 logs | Type+recency only; presumed-regenerable bulk. |

## 2026-09-25 — tmp-salvage (move, curated: measurement evidence only)

Post-v2.7.6 sweep. `salvage.py --coverage` reached **140,932 files** and proposed
**60,636 candidates**, 59,430 of them in this session's own agent scratch. By
maintainer choice **one directory was claimed** and moved (not copied):
`rustynes-perf-evidence-v2.7.6`, **48 files / 153 KB**, staged from the scratch
so that it lands as one unit. The move reported 1 moved, 0 duplicates, 0 errors.

This section was **written by hand after the script's own entry was lost**: an
overwrite of this file in the same session (restoring the committed copy to
unblock a rebase, with a backup that had not in fact been written) discarded
it. The facts below are from the dry-run and execution output, not memory.

### Claimed (1 directory, 48 files)

| Source | Destination | Notes |
|---|---|---|
| staged `rustynes-perf-evidence-v2.7.6/` | `salvaged/rustynes-perf-evidence-v2.7.6/` | The inputs and outputs behind `docs/performance.md` §v2.7.5 and §v2.7.6. `v2.7.5/`: the combined ceiling-probe diff, the IMP-04 / IMP-05 / palette candidates, the IMP-07 frame-then-drain bench and their `ab_check.sh` logs. `individual/`: the v2.7.6 harness (`probe.py`, `drive.sh`, `summ.py`), 12 `ab_check` logs, 7 output hashes, the §3.5b re-measurement on `spritecans.nes` (`pulse_bench.rs`, both A/B/A scripts and logs), and the 2,722-pass `test-roms` log. A `README.md` explains each part. |

### Found and deliberately NOT claimed

| Group | Size | Why not |
|---|---|---|
| Regenerable scratch bulk | ~3.3 GB | A Python venv (171 MB), two cargo target trees (0.8 GB, 1.8 GB), nine built mkdocs sites, the libretro buildbot zips (already attached to the v2.7.5 release), and ROM trace dumps. |
| PR bodies, review replies, commit-message drafts | small | Already published: each is the `-F` input to a PR, reply or commit that landed on #553-#555. |
| `/tmp/crates_diff.patch` | 20 KB | A WIP diff of `crates/` that applies in neither direction: superseded by what shipped in v2.7.5. |
| `/tmp/test_fix1.rs`, workspace-root task outputs, the holy-mapperel `hm02` one-line Python fix | small | Not chosen by the maintainer. `hm02` is a scratch clone whose only change is `collections.Sequence` becoming `collections.abc.Sequence`. |
| `/tmp/agy_env.txt` | 6 KB | An environment dump with lines matching token / key patterns. Never offered for the repository tree. |
| Foreign agent scratch | 545 matches, MARM-Stack | Another project's. |

## 2026-10-09 — tmp-salvage (move, curated by hand: session f63c2db0, v3.0.1-v3.2.0)

Deferred until the four v3.2.0 agents stopped (they read their briefs from this
scratch). The tool's own pass could not narrow the claim: with the session's
scratch as its root every path contains the project token, so all 40,580 files
matched. The list was therefore built by hand: hand-written file types (`.txt`,
`.md`, `.py`, `.sh`, `.rs`, `.toml`, `.log`, `.tsv`, `.json`, `.diff`,
`.patch`, `.sv`, `.cpp`, `.yml`, `.csv`) under 2 MiB, outside the regenerable
trees listed below. Moved with a SHA-256 check before each move: 883 files,
15,396,645 bytes, 0 identical-skipped, 0 conflicts, 0 errors.

### Claimed (883 files, 1 ROM)

| Source | Destination | Notes |
|---|---|---|
| `scratchpad/{v301,v310,v320,misc,...}` hand-written files | `salvaged/tmp-scratch/f63c2db0/<same relative path>` | Commit-message drafts (`msg-*.txt`), the v3.2.0 agent briefs (`v320/agent-brief.md`, `v320/sibling-brief.md`), probe crates' `src/` and `Cargo.toml` (`tierprobe`, `probe185`), the MiSTer mapper generators (`v320/mappers/mkmapper*.py`), gate / bless / ladder logs, PR review-reply drafts (`v310/replies*`), and the v3.0.1 seed-sweep and ledger JSON (`v301/sweep`). Resuming a stopped v3.2.0 agent needs its brief from here now, not from `/tmp`. |
| `scratchpad/v320/removed-dumps/Boogerman (Sugar Softec) (Unl) [!].zip` | `salvaged/mislabelled-dumps/m215-header_is-m114s1_Boogerman (Sugar Softec) (Unl) [!].zip` | The GoodNES copy headered as mapper 215, moved out of the corpus in v3.2.0 (it draws grey garbage as 215; the 114 submapper 1 copy with a NES 2.0 header boots). A commercial ROM: `salvaged/` is gitignored, and it must stay out of the tree. |

### Found and deliberately NOT claimed

| Group | Size | Why not |
|---|---|---|
| Clones | ~1.6 GB | `oracle-clone`, `oracle-clone-merge`, `oracle-pin`, `cleanclone`, `sprites/oracle`, `acoin/AccuracyCoin`, `wiki/Wiki_MiSTer.wiki`, `v310/lrs`, `v310/acsrc`: git repositories, recreatable from their remotes. |
| Regenerable bulk | ~2.5 GB | The `mkvenv` Python venv, cargo `target/` trees (`tgt-clean`, the probe crates'), the 1.5 GB `oddskip` perf run and the built `mkdocs-site`. |
| Copies of tracked files | ~4 MB | `v310/preedit`, `v310/split`, `v310/oam`, `v301/pinsnap` (snapshots of `CHANGELOG.md`, `app.rs` and mutant `.rs` files) and the `v310/slice-*.patch` review slices: all in git history. |
| NESdev text extracts, the TriCNES trace | ~1 MB | `misc/wiki` re-renders the vendored `nesdev_wiki/` pages; `v310/dma/tri_xc.txt` is a trace the harness regenerates. |
| Extracted commercial ROMs and frames rendered from them | small | `v320/g2`, `v320/roms290`, PNG dumps: derived from commercial dumps, never for the tree. |
| 60 loose `/tmp` patches, diffs and parse scripts | ~1.5 MB | Review-bot artifacts of RustyNES PRs (Oct 5-8): regenerable from git, the `.md` drafts already posted. Left by the maintainer's choice. |
| Foreign agent scratch | 15,425 matches in 39 groups | Other projects' (VeridianOS, MARM-Stack, KDE dry runs and more). Left by the maintainer's choice. |

## 2026-10-14 — v3.2.0 release cut, session `f63c2db0` (second pass)

Scope chosen by the maintainer: **hand-written scripts and docs only** from this
session's agent scratchpad (applied as everything hand-written, so the pre-edit
source snapshots and the two probe crates' sources are included) (`/tmp/claude-1000/<project slug>/f63c2db0-.../scratchpad`),
and none of the `/tmp` test leftovers. The tool's own scope could not express that
(its `--exclude` does not filter directory candidates, and inside the scratch tree it
ranks logs as strong), so the list was built by hand from file type, as on
2026-10-09. Copied with a byte compare before each original was removed: 93 files,
about 864 KB, 0 identical-skipped, 0 conflicts, 0 errors.

### Claimed (93 files)

| Source | Destination | Notes |
|---|---|---|
| `scratchpad/*` hand-written files (70) | `salvaged/tmp-scratch/f63c2db0/<same relative path>` | The gate runner `gates.sh`, `double-compile.sh`, `roadmap_chain.py`, `sheet.py`, `scan_scratch_clones.py`, `scan_transcripts.py`; 50 commit-message drafts (`msg-*.txt`); the v3.2.0 release-notes draft (its committed form is `.github/release-notes/v3.2.0.md`); pre-edit source snapshots from the agy-branch audit (`m019.{base,import,reverted}.rs`, `mesh_net.{base,import}.rs`, `piccolo.with-cap-and-test.rs`, `cgp.{head,fixed}.py`, `regress.head.sh`) and the AI-made palette split (`palette.split.sv`, `palsplit.patch`); id lists (`r2ids.txt`, `bless25-ids.txt`, `agy-files.txt`, `import-files.txt`, `manifest-edits.json`). |
| `scratchpad/pr598/` (19) | same relative path | Drafted replies and the close comment for PR #598. The PR's document carried an email address and a private Drive id, so this stays in the gitignored tree. |
| `scratchpad/{shotprobe,tiercount}/` (4) | same relative path | Two probe crates' `Cargo.toml` and `src/main.rs`. |

### Found and deliberately NOT claimed

| Group | Why not |
|---|---|
| Every scratchpad log (gate logs, ladders, seed sweeps, A/B runs, `verify-pin*`, Gradle) | The maintainer's scope. The off-die ladder was still writing `ladder-off-final.log` during the move. The figures that matter are in the release notes, `RustyNES.qsf` and `salvaged/rc-v3.2.0-261014/`. |
| `scratchpad/{shots,dumps2}` frame dumps | Rendered from commercial dumps; regenerable with `RUSTYNES_DUMP_FRAMES=1`. |
| `scratchpad/{bindgen-cheevos,trunk-cheevos-dist}` | wasm-bindgen and trunk build output. |
| `/tmp/rustynes-atomic-*` (143 dirs), `/tmp/RustyNES/libretro-abi-*`, `/tmp/rustynes-coverage-collide-*`, `/tmp/rustynes-gamedb-overlay-contract-2`, `/tmp/kotlin-daemon.*.log` | Test fixtures and Gradle daemon logs; the next test run recreates them. |
| `/tmp/rustynes-baseline-screenshots` | 156 coverage captures at default frame counts, superseded by the committed `screenshots/external/` set (which uses the capture overrides), and derived from commercial dumps. |
| `/tmp/authy_build` (a git clone) and about 2,500 weak matches (another project's Python venv and the like) | Not this project's. |

## 2026-10-14 — promotions out of `salvaged/`

At the maintainer's request every file in the gitignored `salvaged/` was checked
for anything that belongs permanently in a repository. Each file was hashed and
looked up as a blob in both repositories' full history (2,963 of 3,448 matched
nothing); the 520 untracked scripts and sources were then read and sorted (99
superseded, 411 one-off, 10 worth keeping). Promoted, each tested before
committing:

| Now at | Was |
|---|---|
| `scripts/release-automation/run_gates.sh` | `salvaged/tools/deps-gates.sh` and the session gate runner |
| `scripts/release-automation/branch_contained.py` | `salvaged/tools/wtcheck.py`, extended to a branch's committed lines |
| `scripts/perf/quiet_ab.sh` | the v2.9.9 / v3.0.1 A/B queue scripts |
| `fuzz/fuzz_targets/movie_import.rs` | a v2.9.9 importer probe, rewritten as a libFuzzer target |
| `to-dos/plans/research/v3.1-line-survey-*.md` | the three v3.1 line-plan surveys, verbatim |
| `RustyNES_MiSTer`: `scripts/double-compile.sh`, `tb/mutate_gate.py`, `tb/export_golden.sh`, `tb/sweep_frames.sh` | the double-compile helper and three v3.2.0 mapper-work tools |

The originals stay in `salvaged/`; its `README.md` records the triage. Logs,
evidence, bitstreams and the mislabelled ROM dumps stay local by design (each
dump is already named in `docs/mappers.md`, `CHANGELOG.md` or
`external_coverage.rs`).

## 2026-10-18 — session f63c2db0, the v3.3.0 to v3.4.0 scratch (all but `v340/`)

Run with `/tmp-salvage` during the v3.4.0 cycle. The maintainer chose the scope: this
session's own agent scratch only, strong matches only, and nothing from elsewhere in
`/tmp`. **`scratchpad/v340/` is deliberately NOT included yet**: the off-die seed
sweep was writing its log there, the queued egui A/B ran from a script there, and the
v3.4.0 release-notes draft lives there. It is salvaged after the release.

**Moved: 541 hand-written text files (10.2 MB)** into gitignored
`salvaged/tmp-scratch/f63c2db0/`, at their scratchpad-relative paths, next to the
2026-10-09 salvage of the same session. Kept to scripts, sources, notes, briefs, reply
bodies, commit messages and tables (`.md .sh .py .sv .cpp .h .rs .tsv .txt .json`
and similar). The largest groups: `offdie/{src,inst,instfix,fix}` (the v3.3.0 off-die
CHR investigation's hand-modified RTL variants, 63 files each), the scratchpad root
(150: commit messages, review-round tables, bot-review captures), `replies/` (34),
`v330/` (32), `slices/` (16). Six collided with a differing file from the 2026-10-09
salvage and were kept beside it as `<name>.from-tmp-20261018131156.<ext>`:
`msg-ios`, `msg-pin`, `msg1`, `msg2`, `msg3` (`.txt`) and `pr-oracle.md`. No errors.

**Found and deliberately left:**

| What | Why |
|---|---|
| `scratchpad/v340/` (about 22,000 candidates, 16 GB) | In use by the release; salvaged after it |
| `scratchpad/dumps2/external/` (53 PNGs) | Framebuffers of commercial games; derived from commercial ROMs, never committed |
| `scratchpad/offdie/repo-{fix,base}` and `offdie/{,f1-}obj_*` | Exported copies of the sibling's tracked tree (no `.git`; recoverable from history) and Verilator build output |
| Logs, CSVs, task `.output` files, `.rgba`/`.bin`/`.rbf` | Regenerable by rerunning the gate, or binary captures that may derive from commercial ROMs |
| `scratchpad/{qab*,bc-*}` | Self-test fixtures for `quiet_ab.sh` and the bump script (copies of tracked scripts, two with test mutations, queue state) |
| `scratchpad/wiki`, `scratchpad/dl-*` | A clean GitHub wiki clone at `c7cf397` and extracted release downloads |
| `/tmp/rustynes-atomic-*` (671 dirs), `/tmp/RustyNES/libretro-abi-*`, `/tmp/rustynes-coverage-collide-*` | Test-suite leftovers (30-byte markers) the suites recreate |
| `/tmp/rustynes-perf` | The A/B queue's live locks |
| The workspace-root agent slug (`-home-parobek-Code`, 1,611 files), `/tmp/VeridianOS`, Authy and venv trees | Other projects' |

## 2026-10-18 (later) — session f63c2db0, `scratchpad/v340/` after the release

The deferred half of the same day's salvage, under the same scope (this
session's scratch, strong matches, hand-written text only). **Moved: 300 files
(2.1 MB)** into `salvaged/tmp-scratch/f63c2db0/v340/`: experiment scripts and
notes, the Goemon and F3a probes, the egui repro's own files (the upstream
issue body for emilk/egui#8731, `main.rs`, `measure.sh`), the round-2 testbench
mutant copies (`r2tb/save`, `r2tb/zap`), commit messages and review captures. No
conflicts, no errors.

**Left:** `hack-target/` (a cargo target directory); the egui clone and every
copy of egui's source (`egui-repro/{egui-main,v036,v037,verify36,verify37,patched,main}`,
`*.orig`; third-party code); every `rtl/`, `tb/` and `sys/` copy (the mutation
runs' rsync copies of tracked trees); all logs, CSVs and binary captures; the
release-day sweep, double-compile and A/B logs, which are already archived
under `salvaged/evidence/v3.4.0-*`.

## 2026-10-18 (night) — session f63c2db0, before a reboot

Scope set by the maintainer: this session's scratch only, strong matches only,
and only what cannot be rebuilt. The dry run's 4,629 strong candidates were cut
to 29 by measurement, not by name: 505 were byte-identical to a blob in either
repository's git history; the rest were run output (812 `.out` / `.log`),
generated code, builds and downloads, text already published (94 review-reply
bodies, every commit-message and PR-body draft), and applied one-off edit
scripts. **Moved: 29 files (about 12 MB)** into
`salvaged/tmp-scratch/f63c2db0/`, relative paths kept, no conflicts:

- `v340/prune/` (25): every release body of both repositories as fetched before
  the 2026-10-18 asset delete and rename and both body rewrites, the
  intermediate states, the delete / rename plans and logs, and the rewrite
  scripts. GitHub keeps no history of a release body, so this is the only copy.
- `v340/notes/` (3): the oracle and sibling session notes and the v3.4.0 notes
  draft before publication.
- `v340/research/v4-options.md`: its companion `nes-mister-resources.md` was
  already in `salvaged/evidence/v3.4.0-research/`, byte for byte.

**Left:** the workspace-root agent slug (1,817 files) and every other project's
files; all 24,360 weak matches; the Python 3.15 venv, the MkDocs build output,
the two sibling-repo copies from the v3.4.0 off-die investigation, and the
ladder worktree copies; `live/turn1.json`, a short-lived TURN credential that
must not be kept. Earlier the same evening, 10 GB of this session's regenerable
build output (a cargo target directory and Verilator `obj_dir` trees) was
deleted to recover a full `/tmp`.

## 2026-10-23 — session f63c2db0, after the v3.5.0 release

Scope set by the maintainer: this session's scratch, curated tier. The dry run
found 60,331 files (4.9 GB) in the session scratchpad. The cut was made by
kind, not by name: build output (cargo `target` / `base-target` trees, `.o`,
`.dwo`, `.bc`, `.rmeta`, Verilator `obj_dir`, wasm `dist-*`), the CodeQL
bundle, databases and repository copies (2.5 GB), downloaded release binaries
and `.rbf` copies (on the GitHub releases and in `salvaged/evidence/v3.5.0-quartus/`),
and any PNG, snapshot, CSV or binary over 256 KB, which takes the golden IRQ
CSVs (`VERIFY=1` rebuilds them from the pinned oracle) and the 137 MB DUT dump.
**Moved: 7,395 files (166 MB)** into `salvaged/tmp-scratch/f63c2db0/`, relative
paths kept, no destination existed, no errors:

- `v350/` (7,267): the v3.5.0 agent and merge work. Oracle worktree scratch
  (`ora-mmc5irq`, `ora-desktop`, `ora-script`, `ora-movie`, `ora-mcp`,
  `ora-small`), sibling board work (`sib-mmc5`, `sib-namco-vrc6`,
  `sib-fcg-vrc7`, `sib-tier`, with the generated stimulus `.nes` files),
  `m5det` / `m5merge` / `mmc5eq` (the MMC5 detector change and merge),
  the mutation runs, gate logs, review replies for #624 and #69, the seed-sweep
  logs in `quartus/`, and the cut, review and PR-body drafts.
- loose files (90) and `mister/`, `v341/`, `cfdocs/`: v3.4.1 and v3.4.2 notes,
  merge logs, CI workflow experiments, and a Worker tail record (no credential).

Checked for credentials before writing this entry: the only hits are repository
copies of the hosted service's docs and tests, Cloudflare's own credential
documentation, and the tail record above.

**Left in /tmp:** 53,776 files (5.1 GB) of the kinds above; the 181
background-task output logs; the VeridianOS agent slug (37 files) and the
`rusty2600-target`, `spectre-target` and `octollm-target` build trees, all of
which belong to other projects; the zellij log and harness cache-state JSON.

## Run 2026-10-27 23:53:24

- Project root: `/home/parobek/Code/OSS_Public-Projects/RustyNES`
- Scanned: `/tmp`
- Project tokens: `rustynes`

| Action | Source | Destination | Category | Confidence | Reason |
|---|---|---|---|---|---|
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m1.json` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/data/m1.json` | data | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m1.json) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m10.json` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/data/m10.json` | data | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m10.json) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m2.json` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/data/m2.json` | data | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m2.json) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m3.json` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/data/m3.json` | data | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m3.json) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m4.json` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/data/m4.json` | data | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m4.json) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m5.json` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/data/m5.json` | data | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m5.json) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m6.json` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/data/m6.json` | data | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m6.json) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m7.json` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/data/m7.json` | data | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m7.json) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m8.json` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/data/m8.json` | data | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m8.json) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m9.json` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/data/m9.json` | data | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/m9.json) |
| moved-conflict | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/msg1.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/msg1.from-tmp-20261023-225057.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/msg1.txt) |
| moved-conflict | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/msg2.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/msg2.from-tmp-20261023-225301.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/msg2.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/m5latch/fda/mapper5latch490.manifest.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/mapper5latch490.manifest.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/m5latch/fda/mapper5latch490.manifest.txt) |
| skipped-duplicate | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/m5latch/fix/mapper5latch490.manifest.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/mapper5latch490.manifest.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/m5latch/fix/mapper5latch490.manifest.txt) — identical file already present |
| skipped-duplicate | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/m5latch/pre/mapper5latch490.manifest.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/mapper5latch490.manifest.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/m5latch/pre/mapper5latch490.manifest.txt) — identical file already present |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/msg1.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/msg1.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/msg1.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/msg2.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/msg2.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/msg2.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/msg.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/msg.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/msg.txt) |
| moved-conflict | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/msg2.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/msg2.from-tmp-20261023-190713.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/msg2.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/msg3.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/msg3.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/msg3.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/msg4.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/msg4.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/msg4.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/msg5.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/msg5.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/msg5.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/msg6.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/msg6.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/msg6.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/pr.md` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/pr.md` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/pr.md) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/published.md` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/published.md` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/published.md) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/r1.md` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/r1.md` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/r1.md) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/r2.md` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/r2.md` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/r2.md) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/r3.md` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/r3.md` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/r3.md) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/r4.md` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/r4.md` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/r4.md) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/r5.md` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/r5.md` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/r5.md) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/r6.md` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/r6.md` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/r6.md) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/a.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/a.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/a.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/d1.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/d1.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/d1.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/epoch8.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/epoch8.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/epoch8.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/noise.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/noise.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/noise.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/nsf.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/nsf.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/nsf.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/wav.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/wav.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/wav.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/3ds-def.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/3ds-def.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/3ds-def.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/3ds-undef.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/3ds-undef.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/3ds-undef.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/baseline.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/baseline.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/baseline.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/c1.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/c1.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/c1.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/c2.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/c2.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/c2.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/c3.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/c3.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/c3.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/c4.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/c4.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/c4.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/c5.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/c5.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/c5.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/c6.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/c6.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/c6.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/summary.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/summary.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/summary.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/msg-android.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/msg-android.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/msg-android.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/msg-ios.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/msg-ios.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/msg-ios.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants-ios.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/mutants-ios.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants-ios.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/mutants.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants2.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/mutants2.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants2.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/pkgA.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/pkgA.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/pkgA.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/stage1/CHANGELOG.md` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/CHANGELOG.md` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/stage1/CHANGELOG.md) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/stage1/ROADMAP.md` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/ROADMAP.md` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/stage1/ROADMAP.md) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/stage1/plan.md` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/plan.md` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/stage1/plan.md) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/stage1/run-sheet.md` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/run-sheet.md` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/stage1/run-sheet.md) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-clippy-full.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gate-clippy-full.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-clippy-full.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-clippy-ra.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gate-clippy-ra.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-clippy-ra.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-clippy-scripting-hd.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gate-clippy-scripting-hd.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-clippy-scripting-hd.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-clippy-scripting.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gate-clippy-scripting.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-clippy-scripting.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-clippy-wasm-canvas.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gate-clippy-wasm-canvas.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-clippy-wasm-canvas.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-clippy-wasm.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gate-clippy-wasm.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-clippy-wasm.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-clippy-ws.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gate-clippy-ws.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-clippy-ws.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-doc.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gate-doc.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-doc.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-nostd.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gate-nostd.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-nostd.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-test-audio-expansion.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gate-test-audio-expansion.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-test-audio-expansion.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-test-core-plain.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gate-test-core-plain.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-test-core-plain.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-test-core.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gate-test-core.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-test-core.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-test-frontend-bin.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gate-test-frontend-bin.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-test-frontend-bin.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-test-frontend-lib.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gate-test-frontend-lib.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gate-test-frontend-lib.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gates.out` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gates.out` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gates.out) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/lint.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/lint.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/lint.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/lint2.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/lint2.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/lint2.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/mac.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/mac.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/mac.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/pgo.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/pgo.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/post350/pgo.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/gates.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gates.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/gates.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/noise-full.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/noise-full.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/noise-full.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/noise-rerun.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/noise-rerun.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/noise-rerun.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/build-3ds.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/build-3ds.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/build-3ds.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/build-nx.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/build-nx.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/build-nx.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/build-nx2.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/build-nx2.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/build-nx2.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/build-vita.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/build-vita.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/build-vita.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/build-vita2.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/build-vita2.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/build-vita2.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/assemble-debug.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/assemble-debug.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/assemble-debug.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/baseline-gradle.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/baseline-gradle.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/baseline-gradle.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/commit1.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/commit1.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/commit1.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/commit2.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/commit2.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/commit2.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/emu5560.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/emu5560.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/emu5560.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/emu5562.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/emu5562.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/emu5562.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/emuA.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/emuA.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/emuA.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/emuB.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/emuB.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/emuB.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/g2.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/g2.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/g2.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/g3.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/g3.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/g3.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/g4.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/g4.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/g4.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/g5.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/g5.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/g5.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/check-ios-host.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/check-ios-host.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/check-ios-host.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/clippy-ios-mobile-android.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/clippy-ios-mobile-android.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/clippy-ios-mobile-android.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/clippy-ws.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/clippy-ws.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/clippy-ws.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/doc.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/doc.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/doc.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/fmt.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/fmt.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/fmt.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/gradle.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gradle.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/gradle.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/test-android-host.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/test-android-host.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/test-android-host.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/test-harness.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/test-harness.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/test-harness.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/test-mobile.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/test-mobile.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates/test-mobile.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/afterPrompt-always-proceed.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/afterPrompt-always-proceed.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/afterPrompt-always-proceed.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/audio-may-play-ignores-visible.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/audio-may-play-ignores-visible.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/audio-may-play-ignores-visible.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/blocking-write-back.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/blocking-write-back.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/blocking-write-back.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/decide-granted-asks.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/decide-granted-asks.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/decide-granted-asks.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/decide-no-device-check.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/decide-no-device-check.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/decide-no-device-check.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/decide-no-target-check.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/decide-no-target-check.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/decide-no-target-check.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/gate-host-bypassed.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/gate-host-bypassed.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/gate-host-bypassed.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/loop-ignores-visible.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/loop-ignores-visible.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/loop-ignores-visible.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/manifest-no-permission.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/manifest-no-permission.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/manifest-no-permission.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/onstop-keeps-visible.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/onstop-keeps-visible.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/onstop-keeps-visible.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/pacer-no-cap.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/pacer-no-cap.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/pacer-no-cap.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/pacer-no-floor.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/pacer-no-floor.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/pacer-no-floor.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/pacer-no-stall.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/pacer-no-stall.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/pacer-no-stall.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/pacer-no-wrap.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/pacer-no-wrap.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/pacer-no-wrap.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/pacer-stall-checked-before-floor.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/pacer-stall-checked-before-floor.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/pacer-stall-checked-before-floor.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/pacer-stall-never-resets.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/pacer-stall-never-resets.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/pacer-stall-never-resets.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/pacer-target-one-frame.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/pacer-target-one-frame.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/pacer-target-one-frame.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/pacer-wallclock-when-audio.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/pacer-wallclock-when-audio.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/pacer-wallclock-when-audio.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/queue-drop-partial.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/queue-drop-partial.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/queue-drop-partial.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/queue-overflow-no-drop.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/queue-overflow-no-drop.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/queue-overflow-no-drop.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/queue-skip-pending.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/queue-skip-pending.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/queue-skip-pending.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/refusal-not-in-sheet.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/refusal-not-in-sheet.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/refusal-not-in-sheet.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/target-36.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/target-36.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutants/target-36.log) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/verify.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/verify.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/verify.log) |
| moved-conflict | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/console-target/armv6k-nintendo-3ds/release/librustynes.a` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/misc/librustynes.from-tmp-20261023-221900.a` | misc | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/console-target/armv6k-nintendo-3ds/release/librustynes.a) |
| moved-conflict | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/console-target/armv6k-nintendo-3ds/release/librustynes.d` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/misc/librustynes.from-tmp-20261023-221900.d` | misc | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/console-target/armv6k-nintendo-3ds/release/librustynes.d) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/console-target/armv7-sony-vita-newlibeabihf/release/librustynes.a` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/misc/librustynes.a` | misc | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/console-target/armv7-sony-vita-newlibeabihf/release/librustynes.a) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/console-target/armv7-sony-vita-newlibeabihf/release/librustynes.d` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/misc/librustynes.d` | misc | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/console-target/armv7-sony-vita-newlibeabihf/release/librustynes.d) |
| moved | `/tmp/RustyNES` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/RustyNES` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2558579-0` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2558579-0` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2558579-1` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2558579-1` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2558579-12` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2558579-12` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2558579-13` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2558579-13` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2558579-14` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2558579-14` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2558579-16` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2558579-16` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2558579-17` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2558579-17` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2558579-2` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2558579-2` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2558579-20` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2558579-20` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2558579-22` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2558579-22` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2558579-25` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2558579-25` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2558579-26` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2558579-26` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2558579-29` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2558579-29` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2558579-3` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2558579-3` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2558579-4` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2558579-4` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2558579-5` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2558579-5` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2585801-0` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2585801-0` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2585801-1` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2585801-1` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2585801-11` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2585801-11` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2585801-12` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2585801-12` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2585801-13` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2585801-13` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2585801-15` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2585801-15` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2585801-16` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2585801-16` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2585801-17` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2585801-17` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2585801-18` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2585801-18` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2585801-2` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2585801-2` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2585801-25` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2585801-25` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2585801-26` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2585801-26` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2585801-3` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2585801-3` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2585801-31` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2585801-31` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2585801-4` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2585801-4` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2585801-5` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2585801-5` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2928780-0` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2928780-0` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2928780-1` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2928780-1` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2928780-11` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2928780-11` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2928780-12` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2928780-12` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2928780-13` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2928780-13` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2928780-16` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2928780-16` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2928780-17` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2928780-17` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2928780-18` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2928780-18` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2928780-19` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2928780-19` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2928780-2` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2928780-2` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2928780-22` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2928780-22` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2928780-29` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2928780-29` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2928780-3` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2928780-3` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2928780-30` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2928780-30` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2928780-7` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2928780-7` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-2928780-9` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-2928780-9` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3300604-0` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3300604-0` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3300604-1` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3300604-1` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3300604-11` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3300604-11` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3300604-13` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3300604-13` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3300604-14` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3300604-14` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3300604-15` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3300604-15` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3300604-17` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3300604-17` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3300604-20` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3300604-20` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3300604-22` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3300604-22` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3300604-25` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3300604-25` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3300604-29` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3300604-29` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3300604-3` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3300604-3` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3300604-30` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3300604-30` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3300604-5` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3300604-5` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3300604-6` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3300604-6` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3300604-8` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3300604-8` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3326649-0` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3326649-0` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3326649-1` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3326649-1` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3326649-11` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3326649-11` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3326649-13` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3326649-13` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3326649-14` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3326649-14` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3326649-16` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3326649-16` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3326649-17` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3326649-17` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3326649-2` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3326649-2` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3326649-20` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3326649-20` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3326649-22` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3326649-22` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3326649-25` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3326649-25` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3326649-29` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3326649-29` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3326649-31` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3326649-31` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3326649-5` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3326649-5` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3326649-6` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3326649-6` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-3326649-8` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-3326649-8` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-526571-0` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-526571-0` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-526571-1` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-526571-1` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-526571-11` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-526571-11` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-526571-12` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-526571-12` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-526571-13` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-526571-13` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-526571-16` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-526571-16` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-526571-17` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-526571-17` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-526571-18` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-526571-18` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-526571-2` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-526571-2` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-526571-21` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-526571-21` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-526571-25` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-526571-25` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-526571-29` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-526571-29` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-526571-30` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-526571-30` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-526571-5` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-526571-5` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-526571-6` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-526571-6` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-atomic-526571-8` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-atomic-526571-8` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-coverage-collide-clean` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-coverage-collide-clean` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-coverage-collide-differing` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-coverage-collide-differing` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-coverage-collide-identical` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-coverage-collide-identical` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/rustynes-gamedb-overlay-contract-2` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/rustynes-gamedb-overlay-contract-2` | project-dir | strong | directory name contains project token 'rustynes' |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/asm/asm.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/asm.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/asm/asm.py) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/hello.c` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/hello.c` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/hello.c) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/hello.c.993392.0.s` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/hello.c.993392.0.s` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/hello.c.993392.0.s) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/mut/run_mutants.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/run_mutants.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/mut/run_mutants.py) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/mut/snap/dap.rs` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/dap.rs` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/mut/snap/dap.rs) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/mut/snap/debuginfo/cc65.rs` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/cc65.rs` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/mut/snap/debuginfo/cc65.rs) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/mut/snap/debuginfo/dwarf.rs` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/dwarf.rs` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/mut/snap/debuginfo/dwarf.rs) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/mut/snap/engine.rs` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/engine.rs` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/mut/snap/engine.rs) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/mut/snap/gdb.rs` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/gdb.rs` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/dap/mut/snap/gdb.rs) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gates.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/gates.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/gates.py) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/mut.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/mut.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/mut.py) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/mutants.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/mutants.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/mutants.py) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/mutants2.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/mutants2.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/mutants2.py) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/mutants3.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/mutants3.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/mutants3.py) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/mutants4.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/mutants4.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/mutants4.py) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/mutants5.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/mutants5.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/mutants5.py) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/noise-test.patch` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/noise-test.patch` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/noise-test.patch) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/rip/gen.rs` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/gen.rs` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/rip/gen.rs) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/stems_tests.rs` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/stems_tests.rs` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/stems_tests.rs) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/clmerge.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/clmerge.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/clmerge.py) |
| moved-conflict | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/gates.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/gates.from-tmp-20261023-234528.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360/gates.py) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/abi_tests.preprobe.rs` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/abi_tests.preprobe.rs` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/abi_tests.preprobe.rs) |
| moved-conflict | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/gates.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/gates.from-tmp-20261023-224252.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/gates.py) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/mutate.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/mutate.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/mutate.py) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/options_full.rs` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/options_full.rs` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/options_full.rs) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/up/common_libretro.h` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/common_libretro.h` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/up/common_libretro.h) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/up/retroarch_libretro.h` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/retroarch_libretro.h` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360lr/up/retroarch_libretro.h) |
| moved-conflict | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/gates.from-tmp-20261023-231129.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/gates.py) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutate_android.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/mutate_android.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutate_android.py) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutate_ios.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/mutate_ios.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/mutate_ios.py) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/to_netplay.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/to_netplay.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/to_netplay.py) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/ui.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/ui.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v360mob/ui.py) |

**Summary:** 264 moved, 2 duplicate(s) skipped, 0 error(s).

## Run 2026-10-27 23:53:58

- Project root: `/home/parobek/Code/OSS_Public-Projects/RustyNES`
- Scanned: `/tmp`
- Project tokens: `rustynes`

| Action | Source | Destination | Category | Confidence | Reason |
|---|---|---|---|---|---|
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351-nt.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/v351-nt.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351-nt.txt) |
| moved-conflict | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351/c1.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/c1.from-tmp-20261023-223555.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351/c1.txt) |
| moved-conflict | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351/c2.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/c2.from-tmp-20261023-225307.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351/c2.txt) |
| moved-conflict | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351/c3.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/c3.from-tmp-20261023-225456.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351/c3.txt) |
| moved-conflict | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351/c4.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/c4.from-tmp-20261023-233232.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351/c4.txt) |
| moved-conflict | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351/c5.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/c5.from-tmp-20261024-010641.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351/c5.txt) |
| moved-conflict | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351/c6.txt` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/docs/c6.from-tmp-20261024-013238.txt` | docs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351/c6.txt) |
| moved | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351/full-test.log` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/logs/full-test.log` | logs | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351/full-test.log) |
| moved-conflict | `/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351/gates.py` | `/home/parobek/Code/OSS_Public-Projects/RustyNES/salvaged/scripts/gates.from-tmp-20261023-224712.py` | scripts | strong | in this project's agent scratch (f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/v351/gates.py) |

**Summary:** 9 moved, 0 duplicate(s) skipped, 0 error(s).

## Copy 2026-10-28 (by hand, v3.6.0 cut)

- Source: this session's scratchpad (`/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/`), every `.txt`, `.py`, `.md`, `.sh`, `.json` and `.mjs` file under 2 MiB.
- Destination: `salvaged/tmp-scratch/f63c2db0/`, same relative paths (gitignored).
- **Copied, not moved**: the scratchpad is left as it was. 306 files; one name collision written as `.from-tmp-20261028`, nothing overwritten.
- Evidence kept separately, also gitignored: `salvaged/evidence/v3.6.0-quartus/` (both seed sweeps, every seed's log), `salvaged/evidence/v3.6.0-ladders/` (both ladders, the three mutant runs, the golden regeneration and diff, the cut's gates and full suite, the coverage rerun, `VERIFY=1` at `ecda19e9`), `salvaged/evidence/v3.6.0-noise/external-coverage-pre-noise/` (the 558 coverage snapshots before the re-record) beside the earlier `goldens-pre-noise/` and `pitch/`.

## Move 2026-10-30 (by hand, after v3.5.1 / v3.6.0 were pushed)

- Source: this session's scratchpad (`/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/`), every regular file except `sib-ci/`.
- Destination: `salvaged/tmp-scratch/f63c2db0/scratchpad/`, same relative paths (gitignored). Script: `salvaged/tmp-scratch/f63c2db0/mv_salv-20261030.py`.
- **Moved**: 307 files, 230.7 MB; no identical duplicates; one name collision (`msg3.txt`) written as `msg3.from-tmp-20261030194031.txt`, nothing overwritten. Includes the PR body files (`prs/`), `m5latch/`, `pal/`, `v360/`, `dap/`, and the seven real `goldens-v351/mapper5latch490.*` files.
- **Not taken**: `sib-ci/` (190 MB throwaway copy of the sibling for the Verilator 5.032 lint repro; its fix is committed as `721c91b`), and 1,583 symlinks in `goldens-v351/` that point into the sibling's `goldens/` and carry no data.
- Swept and found to be other projects' work, left alone: 1,134 files under the workspace-root scratch slug (one RustyNES mention, an agy runner script), 35 under VeridianOS's slug, 573 loose `/tmp` files.

## Run 2026-11-04 (after the v3.6.0 release, by hand)

- Source: this session's scratchpad (`/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/f63c2db0-35e3-4aad-8889-12b0db5b7fc3/scratchpad/`), scope set by the maintainer on 2026-11-03: hand-written files and small logs, nothing from outside the scratch.
- Destination: `salvaged/tmp-scratch/f63c2db0/scratchpad/`, same relative paths (gitignored), by `rsync -a --backup --suffix=.from-tmp-20261104` with `--exclude` for each item under "Not taken", a COPY: the scratch was left in place.
- **Copied**: 239 new files; every other file was already there byte for byte from the 2026-10-30 run. One trap, caught and undone: seven entries in the scratch's `goldens-v351/` (`mapper5latch490.*`) were symlinks pointing INTO this destination, so the copy replaced each salvaged real file with a symlink to itself. `--backup` had kept the real files as `*.from-tmp-20261104`; each self-link was removed and its backup moved back (sizes and Oct 24 dates intact), and the destination has no dangling link. A dry run's count of changed regular files (`>f`) does not show a symlink replacing a file (`cL`): read the whole itemised list. Includes the v3.6.0 release drafts (`rel/`: commit, tag, squash and release-note texts), the seed-sweep logs (`sweep-on*.log`, `sweep-off*.log`, the 261104 sweep behind SEED 15), the release-compile logs (`rbf-v360-final.log`, `rbf-v360-nobuild.log`), the netplay fix's drafts, the `.preedit` snapshots and the salvage plan.
- **Not taken**: `goldens-v360-pin/` (9.9 GB, regenerable at oracle `916410ba`, and installed as the sibling's `goldens/`), `dl351/` and `dl360-*/` (bitstreams downloaded back for md5 checks; the originals are in the releases), `ziptest/dist/` and `ziptest/chrome-prof/` (a build output and a browser profile), `.ruff_cache/`.
- Outside the scratch, by the maintainer's choice: nothing, including `/tmp/RustyNES` (it holds `disksys.rom`, which must not be salvaged) and other projects' slugs.
- **Checked after a review question (Antigravity on #638):** the 2026-10-27 run above moved `/tmp/RustyNES`'s libretro ABI test directories into `salvaged/RustyNES/`, and each carries a `system/disksys.rom`. All 38 on disk are byte-identical, md5 `b30e42ae6989aea8b7d9ac9b55e80a80`: the synthetic 8 KiB BIOS that `crates/rustynes-libretro/src/abi_tests.rs` writes for its tests, not Nintendo's BIOS (md5 `ca30b50f880eb660a320674ed365ef7a`, `rustynes_libretro.info`). No other file under `salvaged/` matches `*disksys*`, and `/salvaged/` is gitignored, so none was ever committed.

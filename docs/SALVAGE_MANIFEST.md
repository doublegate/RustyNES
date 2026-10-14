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
session's agent scratchpad (`/tmp/claude-1000/<project slug>/f63c2db0-.../scratchpad`),
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

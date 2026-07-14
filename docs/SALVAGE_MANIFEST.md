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
| `/tmp/probe_rev.rs` | `scripts/probes/` (untracked) | 2A03-revision (Rp2A03G vs H) DMA divergence probe |
| `/tmp/check_dirs.rs`, `/tmp/dsp_debug_test.rs` | `scripts/probes/` (untracked) | Ad-hoc dev probes |

**Dropped as already-persisted or regenerable (not salvaged):** ~1,751
`ww-head/` and `ww-movie/` PNG frames (Wizards & Warriors debug captures — that
bug is resolved, ADR 0031); ~61 logs; all `reply*.md` (PR bot-comment replies already posted to
GitHub); all `pr_body*.md` / `*-pr.md` / `v2.2.0.md` (PR bodies + release notes
already on GitHub / committed at `.github/release-notes/`); all `*.diff` /
`280_ppu.diff` / `changelog.diff` / `roadmap.diff` / `versionplan.diff` (merged
into git history); all `*-baseline*.md` (transient doc-sync comparison snapshots);
thread `.json` dumps (transient API responses); `/tmp/holy-mapperel` git clone
(already vendored at `ref-proj/holy-mapperel-v0.02` + `tests/roms/holy_mapperel`);
`/tmp/rustynes-mkdocs-test` (6 MB) + `/tmp/rustynes-hm` (4 MB) build/test scratch;
vendored `libretro-database/` + `mkdocs-venv/` upstream/venv scripts.

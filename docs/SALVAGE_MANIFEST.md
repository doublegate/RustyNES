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

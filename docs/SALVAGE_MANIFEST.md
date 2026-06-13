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

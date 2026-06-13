# `docs/audit/` — Decision-Rationale Audit Documents

This directory holds **post-hoc audit reports** that capture *why*
particular decisions were made, the alternatives that were investigated
and rejected, and the provenance of large additions (e.g., the test-ROM
library buildout). It is intentionally distinct from the sibling
directories:

| Directory | Captures | Audience |
|-----------|----------|----------|
| `docs/` (top level) | **WHAT** the system does — per-subsystem specs (`cpu-6502.md`, `ppu-2c02.md`, `mappers.md`, etc.). | Future maintainers building against the spec. |
| `docs/testing/` | **HOW** to test the system — harness inventory + per-baseline `fnv1a64` hashes. | Future maintainers debugging a regression. |
| `docs/audit/` (this dir) | **WHY** specific decisions were made — provenance, rejected alternatives, iNES-mismatch tables, license-vetting outcomes. | Future Claude / future maintainers asking *"why did we pick this game / reject that ROM / lay it out this way?"* |
| `docs/adr/` | **DECIDED** architecture choices in Michael-Nygard ADR form. | Same as `docs/audit/`, but for repeating-back-out load-bearing choices. ADRs are short and decision-focused; audits are long and provenance-focused. |

## Contents

- **`rom-library-buildout-2026-05-17.md`** — full audit of the
  May 2026 buildout of `tests/roms/` to cover all 15 supported mappers
  (60 commercial ROMs into `tests/roms/external/` + 21 freely-redistributable
  ROMs into `tests/roms/audio-tests/` / `m22/` / `mmc1_a12/`). Includes
  the iNES-mapper mismatch table from header verification, the
  investigated-but-rejected ROM list with rationale per rejection, and
  the final per-mapper coverage matrix.

- **`check-mapper.sh`** — the iNES header verifier used during the
  buildout. Reads a `.nes` file, extracts mapper number from bytes 6/7/8
  (handling the iNES 2.0 high-nibble extension), and prints
  `<mapper>\t<submapper>\t<ines2>\t<file>`. Run as
  `docs/audit/check-mapper.sh path/to/rom.nes` to verify a future ROM
  addition matches the target mapper before staging it.

- **`extract-batch.sh`** — the batch-extract + verify + stage script
  used to populate `tests/roms/external/mapper-NNN-NAME/` from the
  user's `~/Dropbox/ROMs/...` zip archive. Hard-codes the Dropbox path
  and the workspace location (see header comment); intended as a
  **reference recipe**, not a portable tool. Future additions should
  re-derive a similar wrapper for their own ROM source.

- **Session / phase / patch audit docs (~60 files).** Beyond the buildout
  audit above, this directory now holds the dated decision-rationale trail for
  the C1 IRQ-timing investigation (`session-11`…`session-29*`), the Phase 7
  hardening (`phase-7-*`), the v1.3.x wasm/pacing work (`v1.3-*`, `v1.3.x-*`),
  the v1.x accuracy recon (`sprint-2.*`, `path-*`), CI notes (`ci-*`), and the
  forward gap-analysis + remediation/development plan
  (`gap-analysis-remediation-plan-2026-05-25.md`). Each filename is
  `<topic>-YYYY-MM-DD.md`; browse the directory listing for the full set.

## Why preserve the staging scripts (not just the audit doc)?

The next ROM-library extension (e.g., adding mapper-78/Cosmo Carrier or
Camerica/British games) will face the same "given a zip, verify the
mapper, stage to the right subdir" problem. The scripts are
embarrassingly simple but were re-derived twice during the buildout
session because the iNES 2.0 mapper-extension byte (byte 8 high
nibble) is easy to get wrong. Keeping them in-tree as reference is
cheaper than re-deriving them a third time.

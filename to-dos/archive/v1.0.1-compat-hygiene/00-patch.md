# v1.0.1 — compatibility + hygiene patch

A small maintenance patch on the v1.0.0 production cut: reconcile the
remaining stale roadmap framing and record the documented compatibility-gap
status. **Zero core behavior change** — AccuracyCoin stays 100.00% (139/139),
the commercial-ROM oracles stay byte-identical, and the determinism contract is
untouched. This is a docs/hygiene patch; it does not alter the emulation core.

## Hygiene audit (what the v1.0.1 plan called for vs. what was already done)

Most of the planned hygiene was completed during the v1.0.0 release work; this
patch verifies it and closes the remainder:

| Item | Status |
|---|---|
| Archive `to-dos/phase-7*` + `phase-8*` | **Already done** — the folders no longer exist; their content is the Phase 7/8 narrative in `ROADMAP.md`. |
| Re-label every `#[ignore]` with a permanent-by-design reason | **Already done** — all 24 `#[ignore]` occurrences carry explicit reasons (permanent-by-design historical pins / interactive-only / by-design-fail / expected-fail). No bare `#[ignore]`s remain. |
| `ROADMAP.md` current-state + lineage framing | **Already done** — the header banner states the single shipped tag is **v1.0.0** and frames the `v1.x`/`v2.x` markers as upstream engine lineage; the Status section reports AccuracyCoin **100.00%**. |
| Scrub stale "deferred to v2.0" / "90.65%" so the doc isn't self-contradictory | **Reconciled here** — the Phase 6 section was already labeled **SUPERSEDED** with a 100% closure note; this patch additionally flips its present-tense `[→]` ticket markers to historical `[~]` and adds a note that they record the *then-current* state, not live TODOs. The `90.65%` / `82.73%` figures are retained **as labeled engine-lineage history** (per the project's lineage-preservation policy), not deleted. |
| Dependency-advisory hygiene | **Clean** — `deny.toml` present and allows the v1.0.0 license set; no open Dependabot PRs. |
| Create `to-dos/v1.0.1-compat-hygiene/` | **This folder.** |

## Compatibility-gap status (documented; not forced)

The v1.0.0 plan flagged three long-tail compatibility items. Each is an
open-ended accuracy investigation whose fix would risk the 100% core, and two
need a commercial dump (never committed) or interactive verification. Per the
plan they are **documented, not force-fixed** in this patch:

- **Mito Koumon (mapper 89 / Sunsoft-2) — BG nametable stays empty.** Diagnosed
  as a PPU rendering-enable / setup dependency, not a banking bug. Documented in
  `docs/compatibility.md` §"Known long-tail render gap — mapper 89". The only
  mapper-89 dump on hand is this title; no regression elsewhere.
- **FDS *Kid Icarus* side-B post-registration stall.** The `$4031`
  completion-signal path needs interactive verification; the FDS drive/BIOS/IRQ
  core is functional. Tracked in `docs/compatibility.md` FDS notes.
- **GxROM (mapper 66) "Mario flashing" report.** Not reproducible headlessly
  without the specific dump (commercial, gitignored); no GxROM regression in the
  oracle or AccuracyCoin. Closed pending a concrete repro.

## Verification

- `cargo test --workspace --features test-roms` → AccuracyCoin 100% + nestest
  0-diff (unchanged: this patch touches only docs / to-dos).
- Quality gates green (this is a docs-only change; no code paths altered).

# v1.0.1 — Compatibility + Hygiene Patch

**Status:** OPEN (started 2026-06-13)
**Type:** patch release (additive / fixes only; zero core behavior change for working games)
**Hard gate:** AccuracyCoin 100% (139/139), commercial-ROM oracles byte-identical,
nestest 0-diff, all CI green (CI / Security / Pages).

## Mission

The first post-v1.0.0 release. Close the documented minor compatibility gaps and
reconcile the stale roadmap/docs that predate the v1.0.0 core, with **no change**
to already-working games.

This patch ships **before** the v1.1.0 feature work (see `../v1.1.0-features/`).

## Sprints

| Sprint | Theme | File |
|---|---|---|
| 1 | Compatibility fixes (game-specific) | `sprint-1-compat-fixes.md` |
| 2 | Doc / roadmap / test hygiene | `sprint-2-doc-test-hygiene.md` |

## Out of scope (per maintainer, 2026-06-13)

- **No accuracy-grinding.** The "v1.2.0 accuracy residuals" and "v2.0 master-clock
  refactor" tracks are retired — that work is already accomplished by the v1.0.0
  core (master clock is the only scheduler; AccuracyCoin 100%). The stale
  `phase-7`/`phase-8` plans are archived under `../archive/`.
- The remaining by-design `#[ignore]` probes are documented in place as
  permanent-by-design, not ground on.

## Exit criteria

- All Sprint-1 compat items either fixed (with a pinned regression test) or
  documented as closed/not-reproducible.
- All Sprint-2 hygiene items landed; `docs/STATUS.md` + `to-dos/ROADMAP.md` show
  the current state (no "deferred to v2.0", no stale 90.65%).
- Full gate suite green; `CHANGELOG.md` `[1.0.1]` written; tag `v1.0.1`.

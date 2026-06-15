# v1.0.1 · Sprint 2 — Doc / roadmap / test hygiene

Reconcile the stale planning + accuracy language that predates the v1.0.0 core.
The master clock is the only scheduler and AccuracyCoin is 100%, so the
"deferred to v2.0" framing and the 90.65% figures are obsolete.

## T-101-010 — Archive the stale phase plans  ✅ DONE (2026-06-13)

- Moved `phase-7-nesdev-accuracy-hardening/` + `phase-8-v1.2.0-accuracy-residuals/`
  into `../archive/`; added `../archive/README.md` explaining why (work accomplished).

## T-101-011 — Scrub stale accuracy/version language

- **`docs/STATUS.md`** — remove "deferred to v2.0 master-clock" + the stale
  `90.65%` headline (the current measured rate is AccuracyCoin **100%**), and the
  `dmc-get-put-scheduler` "promotion bundled with v2.0" note (lines ~241/249/311/379/501).
  Replace with the current state; keep clearly-marked engine-lineage history as history.
- **`to-dos/ROADMAP.md`** — update the forward roadmap so the retired v1.2.0/v2.0
  accuracy tracks are removed and the live roadmap points to the
  `v1.0.1-compat-hygiene/` and `v1.1.0-features/` folders.
- **`to-dos/README.md`** — point the "forward roadmap" at the new release folders.
- **Done when:** no live doc claims an unfinished master-clock/accuracy track; the
  authoritative state (AccuracyCoin 100%) is consistent across STATUS.md + ROADMAP.md.

## T-101-012 — Re-label `#[ignore]` tests as permanent-by-design

The ~24 `#[ignore]` occurrences across `crates/*` are now either by-design or the
documented hard-tier residuals the roadmap says "document, don't grind". Re-label
each with a clear **permanent-by-design** reason; do NOT open accuracy-grind work.

- By-design: `mmc3` NEC-rev-B alt (project defaults Sharp rev A); interactive
  `cpu_reset` full-protocol (needs externally-timed reset); live-STUN integration
  (`rustynes-netplay/tests/stun_probe.rs`, hits a public server).
- Documented residuals: `cpu_interrupts_v2` C1 sub-ROMs, `apu_reset` len/4017,
  `mmc3_test_2/4` #3, the CPU/PPU/APU unit probes that pin default-build behavior.
- **Done when:** every `#[ignore]` has a one-line permanent-by-design reason and a
  companion `*_currently_fails` probe (where applicable) so a surprise-pass still
  fails loudly; `cargo test --workspace --features test-roms -- --ignored` behaves as documented.

## T-101-013 — Refresh README / version notes

- Update `to-dos/README.md` status line for the v1.0.1 cycle; ensure CHANGELOG
  `[Unreleased]` accumulates the v1.0.1 entries.

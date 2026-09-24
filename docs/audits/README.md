# Audits

Four codebase audits drive the v2.7.x and v2.8.x lines
([ADR 0041](../adr/0041-hardware-release-is-v3.0.0.md)). Each report is kept here
**exactly as it was committed**, and each has a disposition ledger beside it that
records what was done with every finding.

| Scope | Report | Ledger | Acted on in |
| --- | --- | --- | --- |
| Core engine (`rustynes-core`/`-cpu`/`-ppu`/`-apu`/`-mappers`) | [`core-audit-report.md`](core-audit-report.md) | [`core-disposition.md`](core-disposition.md) | v2.7.x |
| Frontend, web, mobile, sandboxing | [`frontend-audit-report.md`](frontend-audit-report.md) | [`frontend-disposition.md`](frontend-disposition.md) | v2.7.x |
| Libretro core | [`libretro-audit-report.md`](libretro-audit-report.md) | [`libretro-disposition.md`](libretro-disposition.md) | v2.8.x |
| `RustyNES_MiSTer` RTL | [`rtl-audit-report.md`](rtl-audit-report.md) | [`rtl-disposition.md`](rtl-disposition.md) | v2.8.x |

## What these reports are

They were **written by an AI agent** (the Antigravity CLI, `agy`) and committed in
PR #544. They are useful and they are not authoritative. A report states a finding;
it does not establish one. Several of their recommendations also collide with
standing project rules, and the rules win.

**Calibration, 2026-09-22.** Before any release was planned around them, 26 of
their claims were read against the code:

- **Core, libretro and frontend are largely accurate at the cited lines.** The
  save-state panics, the missing battery-SRAM plumbing on five mappers, the MMC5,
  MMC1 and Namco 163 gaps, the dropped egui texture delta, the HD-pack PNG
  allocation, the missing Lua memory limit, the ignored libretro `_game`, the
  Zapper serialize-size growth and the unserialised `internal_data_bus` are all
  real.
- **The RTL report is the least reliable.** Its DC-blocker overflow and pulse-1
  sweep-mute defects are refuted, as is the DQM half of its SDRAM finding. Its
  resource table does not match the Quartus fit report (it claims 21,865 ALMs,
  394 M10K and 0 DSP; the fit says 23,013, 468 and 33). What held up: CKE raised
  with the first PRECHARGE, the arbiter byte-lane select, the unsynchronised reset,
  the MMC1 filter ordering (with its impact misdescribed), and the unconstrained
  SDRAM I/O pins.
- **Headline figures in the reports are stale in places.** For example, the core
  report says AccuracyCoin "141/141"; it has been 144/144 since v2.6.18. The
  ledgers correct such facts; the reports are not edited.

**Working rule:** trust a located Rust defect once a test fails on it; re-derive
every RTL claim and every number before acting.

## How the reports are used

**They are the working reference for every release from v2.7.0 to v3.0.0**, read alongside the plans in `to-dos/plans/` (maintainer instruction, 2026-09-23). A plan compresses each finding to a table cell; the report keeps the mechanism, the file and line locations, the failure scenario and a remediation sketch. Before triaging or fixing a finding, read its section in the report.

| Line | Reports |
| --- | --- |
| v2.7.x | [core](core-audit-report.md), [frontend](frontend-audit-report.md) |
| v2.8.x | [libretro](libretro-audit-report.md), [RTL](rtl-audit-report.md) |
| v2.9.x | all four: the v2.9.0 re-audit is diffed against them and their ledgers |

## How a finding is closed

Every finding goes **triage → pin red → fix → gate**:

1. **Triage** into the ledger with evidence: a file and line, or a command and
   its output.
2. **Pin** a test that fails on `main` first (unit test, fuzz case, test ROM,
   golden or co-simulation gate), and show it failing. The reports' "syntactically
   valid" snippets are treated as unverified suggestions.
3. **Fix** with the smallest correct change. A chip change updates its
   `docs/<subsystem>.md` in the same PR.
4. **Gate** with the full CI set, plus AccuracyCoin 144/144 and nestest 0-diff
   re-run, byte-identity on the goldens for any core or snapshot change, and the
   sibling ladder's `TOTAL:` line for any RTL change.

A fix that would break a standing rule is **escalated to the maintainer**, not
applied or dropped silently. The rules most likely to bite here are NEVER
LAUNDER, the performance adoption rule (byte-identical, plus the evidence-quality
bar in `scripts/perf/ab_check.sh`: a consistent, reproduced, clean gain on two
independent runs, adoptable even below 3%), off-by-default features, and the
ADR 0037 reference firewall.

## Verdicts

| Verdict | Meaning |
| --- | --- |
| CONFIRMED | The defect exists as described, with evidence. |
| PARTIAL | Part of the claim holds; the ledger says which part. |
| REFUTED | The code does not do what the finding says; evidence given. Closed. |
| NOT A DEFECT | Accurate description, but the behaviour is deliberate or correct. Closed. |
| REJECTED FIX | The defect is real but the recommended fix breaks a project rule; a different fix is recorded. |
| UNTRIAGED | Not yet read against the code. Nothing is planned on an untriaged finding. |
| FIXED | Closed by the PR in the ledger row, with the red-first test named. |

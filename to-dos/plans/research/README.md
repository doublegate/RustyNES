# Forward-plan research dives (v1.7.0 → v2.0.0)

Read-only reference-mining reports that fed the **v1.7.0 → v2.0.0** release plans
(the plans themselves live one level up in [`../`](../)). Each dive cross-checked
the reference emulators in `ref-proj/`, `nesdev_wiki/`, the in-repo `docs/`, and
the web against RustyNES's current state, excluding anything the in-development
v1.6.0 "Studio" cut already covers and anything that belongs to the v2.0.0
timebase axis. Kept verbatim as authored (reference, not maintained prose), so
this tree is markdownlint-exempt like `ref-docs/`.

## v1.7.0 — the next feature/tooling/accuracy/breadth tier

| File | Source cluster |
|---|---|
| [`v1.7.0-research-bizhawk-fceux.md`](v1.7.0-research-bizhawk-fceux.md) | BizHawk + FCEUX — creator/TAS/scripting/automation residue beyond v1.6.0 |
| [`v1.7.0-research-mesen2-geranes.md`](v1.7.0-research-mesen2-geranes.md) | Mesen2 + GeraNES — editing-capable tools, debugger depth, HistoryViewer, RA HUD, audio, i18n |
| [`v1.7.0-research-ares-higan-tricnes-punes.md`](v1.7.0-research-ares-higan-tricnes-punes.md) | ares/higan/TriCNES/puNES consolidated — accuracy-without-v2.0, breadth, perf |
| [`v1.7.0-detail-accuracy-ares-higan.md`](v1.7.0-detail-accuracy-ares-higan.md) | detail: dot/cycle-granular accuracy dive |
| [`v1.7.0-detail-breadth-punes-nestopia.md`](v1.7.0-detail-breadth-punes-nestopia.md) | detail: mapper/peripheral breadth dive |
| [`v1.7.0-detail-niche-and-web.md`](v1.7.0-detail-niche-and-web.md) | detail: niche subsystems + web/wasm parity |
| [`v1.7.0-detail-performance.md`](v1.7.0-detail-performance.md) | detail: measure-first performance dive |

## v1.8.0 / v1.9.0 — mobile ports

| File | Scope |
|---|---|
| [`v1.8.0-research-android.md`](v1.8.0-research-android.md) | Android app — architecture, NDK/AAB toolchain, SAF, Play policy, shared mobile-bridge |
| [`v1.9.0-research-ios.md`](v1.9.0-research-ios.md) | iOS/iPadOS app — Rust-core+SwiftUI/Metal, signing, App Store policy, JIT non-issue |

## v2.0.0 — the timebase major

| File | Scope |
|---|---|
| [`v2.0.0-research-master-clock.md`](v2.0.0-research-master-clock.md) | The one-clock + every-cycle-bus-access collapse, residual closure (R1–R5), Vs.DualSystem, breaking-API/save-state surface, perf re-baseline |

## v3.1.0 → v4.0.0 — the line-plan surveys

The three read-only surveys
[`../v3.1-to-v4.0-line-plan.md`](../v3.1-to-v4.0-line-plan.md) was drafted from
(2026-10-06/07, against `release/v3.0.1`). They lived only in session scratch
until v3.2.0 promoted them, because the plan carries their conclusions but not
their per-item evidence: of the 86 item ids in the oracle backlog's tables, 25
appeared nowhere else in `docs/`, `to-dos/`, `ROADMAP.md`, `VERSION-PLAN.md` or
`CHANGELOG.md` when they were promoted (2026-10-14). **Statuses are as of 2026-10-06**; an item may since have
shipped under a ticket name (PROV-01 became T-NTSC-PROVENANCE). Unlike the dives
above, no reference-emulator source was consulted for these; the oracle survey
records the one incidental contact under PROV-02.

| File | Scope |
|---|---|
| [`v3.1-line-survey-oracle-backlog.md`](v3.1-line-survey-oracle-backlog.md) | every open, deferred or proposed oracle item, with status, size, blocker and what it breaks |
| [`v3.1-line-survey-mister-backlog.md`](v3.1-line-survey-mister-backlog.md) | the same for the MiSTer sibling after v3.0.1 |
| [`v3.1-line-survey-ecosystem.md`](v3.1-line-survey-ecosystem.md) | the outside ecosystem to about 2027-10 (Android and iOS distribution rules, MiSTer submission, libretro), every claim source-labelled |

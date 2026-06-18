# Release plans

Verbatim copies of the per-release planning documents for the RustyNES public
**v1.2.0 → v1.6.0** line — the path that got the project to its current stage of
development. They are kept exactly as authored (historical / reference, not
maintained prose), so this folder is exempt from the markdownlint gate, the same
treatment `ref-docs/` and the archive trees get.

> **Authoritative current state lives elsewhere.** For "where the project is
> right now" read [`docs/STATUS.md`](../../docs/STATUS.md) (per-suite pass counts
> + the mapper matrix), [`CHANGELOG.md`](../../CHANGELOG.md) (per-version detail),
> and [`../ROADMAP.md`](../ROADMAP.md) (the forward roadmap). These plans are the
> *intent* captured before each release; some scope shifted during execution.

| Plan | Release | Status |
|---|---|---|
| [`v1.2.0-curator-plan.md`](v1.2.0-curator-plan.md) | v1.2.0 "Curator" — library breadth + compatibility + reach | Shipped |
| [`v1.3.0-bedrock-plan.md`](v1.3.0-bedrock-plan.md) | v1.3.0 "Bedrock" — foundation + breadth | Shipped |
| [`v1.3.0-toolchain-modernization-plan.md`](v1.3.0-toolchain-modernization-plan.md) | v1.3.0 Workstream A — edition 2024 + MSRV 1.96 + egui 0.34 / wgpu 29 | Shipped (sub-plan) |
| [`v1.4.0-fidelity-plan.md`](v1.4.0-fidelity-plan.md) | v1.4.0 "Fidelity" — compatibility + finish (+ the v1.4.1 patch) | Shipped |
| [`v1.5.0-lens-plan.md`](v1.5.0-lens-plan.md) | v1.5.0 "Lens" — insight + scriptability + creator tooling + polish | Shipped |
| [`v1.6.0-studio-plan.md`](v1.6.0-studio-plan.md) | v1.6.0 "Studio" — TAS authoring + debugger depth + accuracy + breadth | **In development** |

## Not included here

- **v1.0.0 / v1.1.0** plans — the v1.0.0 production cut was a synthesis of the
  upstream engine line (see [`docs/v1.0.0-synthesis-handoff-2026-06-13.md`](../../docs/v1.0.0-synthesis-handoff-2026-06-13.md)),
  and the v1.0.1 / v1.1.0 staging folders were archived under
  [`../archive/`](../archive/README.md).
- **Engine-lineage (`v2.x`) plans** — the inbound accuracy engine's own
  v2.0–v2.8 planning documents are upstream history (folded into the v1.0.0
  core), not part of the public-release path, so they are not copied here. Note
  that the engine-lineage "v2.0" (the master-clock work that reached AccuracyCoin
  100%) already shipped as the v1.0.0 core; the **future RustyNES v2.0.0** is the
  separate fractional-timebase refactor (ADR 0002) — see `../ROADMAP.md`.

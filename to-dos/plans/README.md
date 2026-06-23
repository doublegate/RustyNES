# Release plans

Verbatim copies of the planning documents along the RustyNES line — the public
**v1.0.0** synthesis through the in-development **v1.6.0**, the **forward plans**
(v1.7.0 → v2.0.0, in design), the upstream **engine-lineage** (`v2.x`) plans that
produced the v1.0.0 cycle-accurate core (under [`engine-lineage/`](engine-lineage/)),
and the per-release **research dives** that fed the forward plans (under
[`research/`](research/README.md)). They are kept exactly as authored (historical
/ reference, not maintained prose), so this folder is exempt from the markdownlint
gate — the same treatment `ref-docs/` and the archive trees get.

> **Authoritative current state lives elsewhere.** For "where the project is
> right now" read [`docs/STATUS.md`](../../docs/STATUS.md) (per-suite pass counts
> + the mapper matrix), [`CHANGELOG.md`](../../CHANGELOG.md) (per-version detail),
> and [`../ROADMAP.md`](../ROADMAP.md) (the forward roadmap). These plans are the
> *intent* captured before each release; some scope shifted during execution.

## Public-release plans

| Plan | Release | Status |
|---|---|---|
| [`v1.0.0-synthesis-plan.md`](v1.0.0-synthesis-plan.md) | v1.0.0 — the synthesis that harvested the engine line into the public repo | Shipped |
| [`v1.2.0-curator-plan.md`](v1.2.0-curator-plan.md) | v1.2.0 "Curator" — library breadth + compatibility + reach | Shipped |
| [`v1.3.0-bedrock-plan.md`](v1.3.0-bedrock-plan.md) | v1.3.0 "Bedrock" — foundation + breadth | Shipped |
| [`v1.3.0-toolchain-modernization-plan.md`](v1.3.0-toolchain-modernization-plan.md) | v1.3.0 Workstream A — edition 2024 + MSRV bump + the egui / wgpu / rfd dependency tier | Shipped (sub-plan) |
| [`v1.4.0-fidelity-plan.md`](v1.4.0-fidelity-plan.md) | v1.4.0 "Fidelity" — compatibility + finish (+ the v1.4.1 patch) | Shipped |
| [`v1.5.0-lens-plan.md`](v1.5.0-lens-plan.md) | v1.5.0 "Lens" — insight + scriptability + creator tooling + polish | Shipped |
| [`v1.6.0-studio-plan.md`](v1.6.0-studio-plan.md) | v1.6.0 "Studio" — TAS authoring + debugger depth + accuracy + breadth | **In development** |

> **v1.1.0** has no plan here — its staging folder was archived under
> [`../archive/`](../archive/README.md) (see `to-dos/archive/v1.1.0-features/`).

## Forward plans (in design)

Synthesized from the reference-mining dives in [`research/`](research/README.md)
(BizHawk/FCEUX, Mesen2/GeraNES, ares/higan/TriCNES/puNES + accuracy/breadth/niche/perf,
Android, iOS, and the v2.0.0 master-clock analysis), with the keystone scope
decisions locked by the maintainer. Not yet started.

| Plan | Release | Scope decision |
|---|---|---|
| [`v1.7.0-forge-plan.md`](v1.7.0-forge-plan.md) | v1.7.0 "Forge" — writable + programmable tooling, on the current scheduler | **Maximal** (full A–H) |
| [`v1.8.0-android-plan.md`](v1.8.0-android-plan.md) | v1.8.0 — Android app | **Hybrid** (Rust core + wgpu `SurfaceView` + Compose) + focused MVP; factors the shared `rustynes-mobile` bridge. **Engineering spec; the store launch is deferred — see the mobile-finalization plan below.** |
| [`v1.9.0-ios-plan.md`](v1.9.0-ios-plan.md) | v1.9.0 — iOS / iPadOS app | **SwiftUI + wgpu→Metal** reusing the shared bridge; same focused MVP. **Now the *interim* iOS foundation (sideload / TestFlight); release finalization moves to v2.0.5–v2.0.8.** |
| [`v2.0.0-master-clock-plan.md`](v2.0.0-master-clock-plan.md) | v2.0.0 "Timebase" — one-clock + every-cycle-bus-access rewrite | **Full rewrite + aggressive residual closure**; the mobile-finalization train (v2.0.1 → v2.1.0) follows it |
| [`v2.0.x-mobile-finalization-plan.md`](v2.0.x-mobile-finalization-plan.md) | v2.0.1 → v2.1.0 — mobile finalization + joint store launch | **Deferred mobile launch (maintainer, 2026-06-23):** Android final v2.0.1–v2.0.4, iOS final v2.0.5–v2.0.8, both-apps readiness v2.0.9, **joint Google Play + App Store launch v2.1.0** |

> **Versioning note:** v2.0.0 is *not* "add a master clock" — that already
> shipped (it is today's only scheduler). v2.0.0 collapses the five-counter
> substrate to one clock with every-cycle bus access, which is what makes the
> last accuracy residuals representable. It is the one release licensed to break
> save-state / cross-version determinism.
>
> **Mobile-launch note (2026-06-23):** the Android (v1.8.x) and iOS (v1.9.0) apps
> are held back from their app stores until **after v2.0.0**, then finalized
> across **v2.0.1–v2.0.8**, verified for both in **v2.0.9**, and launched
> **together at v2.1.0** (so they ship on the v2.0.0 core rather than re-releasing
> after the breaking timebase change). Until then: Android = GitHub sideload,
> iOS = TestFlight. See [`v2.0.x-mobile-finalization-plan.md`](v2.0.x-mobile-finalization-plan.md).

## Engine-lineage plans (`engine-lineage/`) — upstream history

The v1.0.0 production core descends from an extensively-planned accuracy program
(the private "RustyNES_v2" engine, whose internal milestones ran a `v2.0–v2.8`
line). These are that program's planning + research documents — folded into the
v1.0.0 core, **not** public RustyNES releases.

> **Versioning caveat:** the engine-lineage "v2.0" below (the master-clock work
> that reached AccuracyCoin 100%) **already shipped as the v1.0.0 core**. The
> *future* RustyNES **v2.0.0** is the separate fractional-timebase refactor
> (ADR 0002) — see [`../ROADMAP.md`](../ROADMAP.md). Do not conflate them.

| Plan | Topic |
|---|---|
| [`engine-lineage/v2.0.0-release-path.md`](engine-lineage/v2.0.0-release-path.md) | The engine's v2.0.0 release path (master-clock default + scheduler). |
| [`engine-lineage/accuracycoin-remediation.md`](engine-lineage/accuracycoin-remediation.md) | Remediating the last AccuracyCoin failures toward 100%. |
| [`engine-lineage/real-game-regression-recovery.md`](engine-lineage/real-game-regression-recovery.md) | Real-game rendering regression recovery. |
| [`engine-lineage/v2.0-residual-closure-strategy.md`](engine-lineage/v2.0-residual-closure-strategy.md) | Residual-closure strategy: sweeps + cross-emulator audit. |
| [`engine-lineage/dmc-dma-subcycle-rewrite.md`](engine-lineage/dmc-dma-subcycle-rewrite.md) | DMC-RDY sub-cycle DMA halt/arming rewrite. |
| [`engine-lineage/dma-cycle-count-fix-design.md`](engine-lineage/dma-cycle-count-fix-design.md) | Per-DMA Mesen-exact OAM/DMC cycle-count fix design. |
| [`engine-lineage/v2.6.0-known-gap-research.md`](engine-lineage/v2.6.0-known-gap-research.md) | v2.6.0 five-known-gap research (read-only). |
| [`engine-lineage/retroachievements-research.md`](engine-lineage/retroachievements-research.md) | RetroAchievements integration research (read-only). |
| [`engine-lineage/v2.6.0-netplay-deployment-research.md`](engine-lineage/v2.6.0-netplay-deployment-research.md) | v2.6.0 netplay deployment-gap investigation (read-only). |

Other plans in the developer-local plans dir belong to unrelated projects (a
game remaster, a system-optimization pass) and are not copied here.

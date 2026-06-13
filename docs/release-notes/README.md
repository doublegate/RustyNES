# Release notes — engine-lineage history

> **These are NOT RustyNES release notes.** RustyNES ships as **v1.0.0** (the
> production cut). The `v2.0.0` … `v2.8.0` files in this folder are the
> **upstream engine lineage**: the per-version release notes of the internal
> cycle-accurate engine line whose increments produced the v1.0.0 technology.
> They are retained verbatim as development history — the *how* behind each
> capability — and keep their original version numbers and dates as historical
> anchors. The authoritative RustyNES release record is the root
> [`CHANGELOG.md`](../../CHANGELOG.md) `[1.0.0]` section, and
> [`../STATUS.md`](../STATUS.md) is the single source of truth for version
> policy and feature state.

## Engine-lineage → RustyNES stage map

Every capability documented across these engine notes ships in **RustyNES
v1.0.0**. The engine line maps onto RustyNES's own pre-1.0 stage history as:

| Engine line | RustyNES stage | Headline capability |
|-------------|----------------|---------------------|
| (engine v1.0–v1.7) | v0.9.0 – v0.9.2 | cycle-accurate core, frontend, mappers, TAS, niceties |
| [v2.0.0](v2.0.0.md) / [v2.0.1](v2.0.1.md) | v0.9.3 | master-clock-precise scheduler → AccuracyCoin 100% (139/139) |
| [v2.1.0](v2.1.0.md) / [v2.2.0](v2.2.0.md) | v0.9.4 | +13 mappers, Vaus/Zapper; Famicom Disk System (real BIOS) |
| [v2.3.0](v2.3.0.md) / [v2.4.0](v2.4.0.md) / [v2.4.1](v2.4.1.md) / [v2.5.0](v2.5.0.md) | v0.9.5 | rollback netplay; rendering-accuracy + compatibility; Vs./PC10 + multiplayer groundwork |
| [v2.6.0](v2.6.0.md) / [v2.7.0](v2.7.0.md) / [v2.7.1](v2.7.1.md) | v0.9.6 | +11 mappers, N-peer netplay, working FDS; RetroAchievements; netplay hardening |
| [v2.8.0](v2.8.0.md) | v0.9.7 | the performance pass (display-sync pacing, audio DRC, run-ahead, dedicated emu thread) |
| *(synthesis: parent UX shell + docs + production polish)* | **v1.0.0** | the single shipped tag |

## Files

| File | Engine milestone |
|------|------------------|
| [v2.0.0.md](v2.0.0.md) | The master-clock milestone (R1 scheduler becomes default) |
| [v2.0.1.md](v2.0.1.md) | Legacy integer-lockstep scheduler removed |
| [v2.1.0.md](v2.1.0.md) | Coverage + expansion (+13 mappers, Vaus/Zapper) |
| [v2.2.0.md](v2.2.0.md) | Famicom Disk System |
| [v2.3.0.md](v2.3.0.md) | Netplay (rollback netcode) |
| [v2.4.0.md](v2.4.0.md) | Compatibility & rendering accuracy |
| [v2.4.1.md](v2.4.1.md) | VRC2a (mapper 22) register-select fix |
| [v2.5.0.md](v2.5.0.md) | Vs./PC10 + multiplayer & internet netplay groundwork |
| [v2.6.0.md](v2.6.0.md) | Vs./PC10 RGB game-verified, +11 mappers, N-peer netplay, working FDS |
| [v2.7.0.md](v2.7.0.md) | RetroAchievements + v2.6.0 gap closeout |
| [v2.7.1.md](v2.7.1.md) | Netplay hardening + live-verification patch |
| [v2.8.0.md](v2.8.0.md) | Optimized performance |

For the curious reader, these notes are the technical record of the accuracy
program. For "what does RustyNES v1.0.0 do", read
[`../STATUS.md`](../STATUS.md) and the root [`README.md`](../../README.md).

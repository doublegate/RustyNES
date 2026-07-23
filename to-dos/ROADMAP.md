# RustyNES — Roadmap

This is the entry point for project planning. Each phase below links to its overview file. Each phase contains sprints; each sprint contains tickets.

The phase bodies preserve the **engine-lineage** development history — the
internal engine line (v0.9.x → v2.x markers) whose increments produced the
RustyNES v1.0.0 technology. Those version markers are historical anchors, not
RustyNES releases of their own; the RustyNES production core shipped at
**v1.0.0**, and the **v1.1.0 → v1.10.0** feature/platform releases ship on top
of it, followed by the breaking **v2.0.0 "Timebase"** release (code-complete
as of 2026-07-03, tag pending).

**RustyNES release line:** `v0.1.0…v0.8.6` (the parent emulator) →
`v0.9.0…v0.9.7` (engine-lineage integration stages — the inbound cycle-accurate
engine being folded in, stage by stage) → **`v1.0.0`** (this synthesis: the
engine + the ported desktop-UX shell + production polish) → **`v1.1.0`
"Scriptable" → `v1.2.0` "Curator" → `v1.3.0` "Bedrock" → `v1.4.0` "Fidelity"**
(+ the `v1.4.1` patch) **→ `v1.5.0` "Lens" → `v1.6.0` "Studio" → `v1.7.0`
"Forge"** (+ the `v1.7.1` patch) **→ `v1.8.0` … `v1.8.9` "Atlas"** (the Android
platform train) **→ `v1.9.0` … `v1.9.9` "Workshop"** (the iOS/iPadOS TestFlight
train) **→ `v1.10.0` "Arcade"** (the native Libretro core) — the additive,
off-by-default feature/platform releases on that core, of which **`v1.10.0`
"Arcade" is the current shipped tag**. The forward path then lands the real
**RustyNES `v2.0.0` "Timebase"** (the one-clock/every-cycle-bus-access
scheduler collapse, ADR 0002/0029) — **code-complete on `main` as of
2026-07-03, tag pending** — then the **v2.0.1 → v2.1.0** mobile-finalization
train that launches the Android + iOS apps jointly at **v2.1.0**, and beyond.
Where the detailed sections below carry the inbound engine's own `v1.x`/`v2.x`
tags, read them as upstream engine history (its v2.0–v2.8 line), which maps
onto the integration stages roughly as: engine v1.0.0 → RustyNES v0.9.0;
v1.1.0–v1.4.0 → v0.9.1; v1.5.0–v1.7.0 → v0.9.2; v2.0.0–v2.0.1 → v0.9.3;
v2.1.0–v2.2.0 → v0.9.4; v2.3.0–v2.5.0 → v0.9.5; v2.6.0–v2.7.1 → v0.9.6;
v2.8.0 → v0.9.7; the synthesis itself = **v1.0.0**.

> **Two distinct "v2.0"s — do not conflate them (historical note, both now
> resolved).** The engine-lineage **v2.0** (the master-clock work that took
> AccuracyCoin to **100.00%**) is *upstream engine history* and shipped as the
> **v1.0.0 production core**. The forward **RustyNES v2.0.0 "Timebase"**
> (ADR 0002/0029) was a *different* milestone — the one-clock/every-cycle-bus-
> access scheduler collapse — that is now **code-complete on `main`, tag
> pending** (see "v2.0.0 'Timebase' — code-complete, tag pending" below for
> what actually shipped, including the one known gap: the MMC3 R1/R2
> IRQ-timing residual, by-design-deferred rather than closed). The engine's
> own `v1.x`/`v2.x` markers in the bullets and "Phases" sections remain
> historical anchors, **never** RustyNES release numbers.

## Status

- **Current release:** **RustyNES v2.1.0 "Fathom"** (2026-07-09) — the **accuracy-remediation** release and the first of the new "Fathom" line, a **core / desktop** cut landing **ahead of** the joint mobile store launch (**moved from v2.1.0 to v2.2.0**, so the Android + iOS apps re-release on this improved core). The deterministic core is unchanged but for one **display-only** PPU fix, so **AccuracyCoin holds 141/141 (100.00%)**, nestest 0-diff, `#![no_std]` untouched, no save-state bump. Lands: **(F1.1)** the **PPU palette backdrop-override** (rendering-disabled + `v` in `$3F00-$3FFF` → `palette[v & 0x1F]`, **byte-exact with TriCNES**; 9 snapshots re-blessed — 2 palette demos + 7 commercial games — all converging with the oracle, `external_real_games` 60/60 byte-identical); **(F1.2/F1.3)** OAM + open-bus audits regression-locked; **(F3)** the **mapper completion** — **86** families promoted BestEffort → Curated with commercial-ROM oracle evidence (57 staged + 29 GoodNES v3.23b), so the tier is **51 Core + 95 Curated + 26 BestEffort** and oracle-gated coverage rises **60 → 146** of 172 (the 26 left have no cleanly-booting dump — 16 NES 2.0 high-id + 8 no-cart + 2 jam-at-boot); **(F5)** the **MMC3 R1/R2 scanline-IRQ residual CLOSED** by-design-permanent (ADR 0002 F5.0 — differential 1-dot deficit, structurally unreachable, zero game impact), with all **20** `#[ignore]`'d tests catalogued in the new `docs/accuracy-ledger.md`; and **(F0)** doc reconciliation (MMC5 + DualSystem stale docs). Version bump (workspace `2.0.8 → 2.1.0`; mobile `MARKETING_VERSION`s unchanged — apps re-release at v2.2.0). See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.1.0]` + `docs/accuracy-ledger.md` + `to-dos/plans/v2.1.0-fathom-accuracy-remediation-plan.md`.
- **Preceding release:** **RustyNES v2.0.8 "Harbor"** (2026-07-09) — the eighth release of the **v2.0.x mobile-finalization train** and the **iOS release candidate** ("Harborlight"), the final release of the iOS finalization window (**v2.0.5 → v2.0.8**). A **host / iOS-only** cut: the cycle-accurate core is **unchanged and byte-identical to v2.0.7** (AccuracyCoin still **141/141, 100.00%**; nestest 0-diff; `#![no_std]` chip stack untouched). It stages the App Store scaffolding for v2.1.0: version-controlled **App Store Connect listing metadata** (`fastlane/metadata/ios/{en-US,es-ES}/`, mirroring the Android tree, files-only), a **dormant App Store `release` lane** in `fastlane/Fastfile` that stages the build + listing but **does not submit** (`submit_for_review: false`) and is **not** CI-wired (the interim channel stays **TestFlight**), and an **App-Review §4.7 self-audit** (no bundled/downloadable ROMs, ownership notice, searchable library, 4+ rating) in `docs/ios-v2.0.8-readiness.md`. Version bump (workspace `2.0.7 → 2.0.8`; iOS `MARKETING_VERSION → 2.0.8`). **No store submission** (that is v2.1.0); screenshots, real signing, the listing upload, and the App-Review submission are the **maintainer / v2.0.9 / v2.1.0** closeout. See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.0.8]` + `docs/ios-v2.0.8-readiness.md` + `to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md`.
- **Earlier in the train:** **RustyNES v2.0.7 "Harbor"** (2026-07-09) — the seventh release of the **v2.0.x mobile-finalization train** and the **third iOS finalization release** ("Trim"), continuing the iOS window (**v2.0.5 → v2.0.8**). A **host / iOS-only** cut: the cycle-accurate core is **unchanged and byte-identical to v2.0.6** (AccuracyCoin still **141/141, 100.00%**; nestest 0-diff; `#![no_std]` chip stack untouched). It wires the **App Store submission floor** (Apple mandates the **iOS 26 SDK / Xcode 26** for every App Store Connect upload from **2026-04-28**, so the tag-gated iOS CI now selects the newest Xcode 26.x on the runner — a build-SDK pin, non-breaking fallback on older images), **reconciles the deployment target `iOS 15.0 → 17.0`** to match the code's real API floor (`NavigationStack` iOS 16 + `.topBarTrailing` iOS 17, unguarded at 12+ sites — the prior 15.0 was never buildable), and **re-audits `PrivacyInfo.xcprivacy`** against the v2.0.6 crash reporter (no new data type / required-reason API — local-only, backup-excluded, off by default). Version bump (workspace `2.0.6 → 2.0.7`; iOS `MARKETING_VERSION → 2.0.7`). **TestFlight-only** (App Store + AltStore PAL deferred to v2.1.0); on-device profiling + the Xcode-26 archive are a **maintainer / v2.0.9** step. See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.0.7]` + `docs/ios-v2.0.7-readiness.md` + `to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md`.
- **Earlier in the train:** **RustyNES v2.0.6 "Harbor"** (2026-07-09) — the sixth release of the **v2.0.x mobile-finalization train** and the **second iOS finalization release** ("Parity"), continuing the iOS window (**v2.0.5 → v2.0.8**). A **host / iOS-only** cut: the cycle-accurate core is **unchanged and byte-identical to v2.0.5** (AccuracyCoin still **141/141, 100.00%**; nestest 0-diff; `#![no_std]` chip stack untouched), so no accuracy / save-state / determinism number moves. It adds a **new opt-in, privacy-first crash-reporting surface** (off by default — the iOS analogue of the Android v1.8.8 `CrashReporter`, closing the v1.9.9 iOS-applicable deferral): **Settings → Diagnostics** installs an uncaught-`NSException` handler that writes **local** crash logs the user can view + copy in-app — **nothing is uploaded**, so the "Data Not Collected" privacy label is unchanged (EN + ES); the handler re-checks the live opt-in at crash time so opting out stops new logs immediately. It also records the **feature-parity re-verification** of the v1.9.x host features (Game Center, CloudKit save sync, MFi controllers, capture / PiP, accessibility) against the unchanged v2.0.0 bridge surface. Version bump (workspace `2.0.5 → 2.0.6`; iOS `MARKETING_VERSION → 2.0.6`). **TestFlight-only** (App Store + AltStore PAL deferred to v2.1.0); on-device crash-capture verification is a **maintainer / v2.0.9** step. See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.0.6]` + `docs/ios-v2.0.6-readiness.md` + `to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md`.
- **Earlier in the train:** **RustyNES v2.0.5 "Harbor"** (2026-07-09) — the fifth release of the **v2.0.x mobile-finalization train** and the **first iOS finalization release** ("Landfall"), opening the iOS window (**v2.0.5 → v2.0.8**) that mirrors the Android v2.0.1 → v2.0.4 window. A **host / iOS-only** cut: the cycle-accurate core is **unchanged and byte-identical to v2.0.4** (AccuracyCoin still **141/141, 100.00%**; nestest 0-diff; `#![no_std]` chip stack untouched), so no accuracy / save-state / determinism number moves. It re-ports the frozen v1.9.9 SwiftUI / Metal app onto the v2.0.0 "Timebase" core: **(1)** the **pre-Timebase movie warning surfaced + localized on iOS** — a non-blocking notice on its own channel (multiplexed through a single alert that prefers an error when both are queued, **EN + ES**, drained via `EmulatorCore.drainWarnings()` → `NesController.drainWarningCodes()`, wording byte-identical to the Android v2.0.4 string) so loading a pre-v2.0.0 `.rnm` tells the user byte-exact framebuffer/audio reproduction isn't guaranteed across the ADR-0028 timebase change; **(2)** the **UniFFI-Swift binding surface re-confirmed** against the v2.0.0 bridge (`drainWarningCodes` / `HostWarning.preTimebaseMovie` / `moviePlay`, host-verified Swift emit); and the **version bump** (workspace `2.0.4 → 2.0.5`; iOS `MARKETING_VERSION 1.9.1 → 2.0.5`, realigned from the frozen v1.9.x default). **TestFlight-only** (App Store + AltStore PAL deferred to v2.1.0); the on-device closeout — the xcframework build on macOS (**Xcode 26 / iOS 26 SDK**), save-state migration from a v1.9.x install, and the AccuracyCoin / SMB / Zelda determinism smoke on Apple silicon — is a **maintainer / v2.0.9** step. See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.0.5]` + `docs/ios-v2.0.5-readiness.md` + `to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md`.
- **Earlier in the train:** **RustyNES v2.0.4 "Harbor" ("Slipway")** (2026-07-08) — the fourth release of the **v2.0.x mobile-finalization train** and the **Android release-candidate** milestone. A **host / Android-only** cut: the cycle-accurate core is **unchanged and byte-identical to v2.0.3** (AccuracyCoin still **141/141, 100.00%**; nestest 0-diff; `#![no_std]` chip stack untouched), so no accuracy / save-state / determinism number moves. It stages the RC scaffolding a maintainer needs to upload the Android app to a Play Console testing track: the `release` build type wired to the upload keystore with a **graceful debug-signing fallback** (keyless CI / local `assemble{Foss,Play}Release` still produces an installable — debug-signed, never shippable — RC artifact); debug-only **StrictMode** diagnostics (`DebugStrictMode`, thread + VM, log-only, `BuildConfig.DEBUG`-guarded, inert in release) as the host complement to the on-device crash-free-rate / ANR gate; version-controlled **fastlane Play Console listing metadata** (`fastlane/metadata/android/{en-US,es-ES}/`); an **R8/ProGuard final hardening review** (keep set confirmed complete, none loosened); and the **version bump** (workspace `2.0.3 → 2.0.4`; Android `versionCode 20003 → 20004` / `versionName → 2.0.4`). The `foss` flavor stays **behaviour-identical**. **No store submission** (that is v2.1.0); the on-device closeout — real-keystore signing, internal/closed testing track, crash-free-rate + ANR gate on hardware, live monetization runtime, the deferred per-feature gate migration — is a **maintainer / v2.0.9** step. See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.0.4]` + `to-dos/plans/v2.0.4-android-rc-plan.md`.
- **Earlier in the train:** **RustyNES v2.0.3 "Harbor" ("Keel")** (2026-07-08) — the third release of the **v2.0.x mobile-finalization train** and the one that makes the octal-latch accuracy work real at the shipped default. The **2-cycle-ALE PPU fetch model is promoted from the experimental `mc-ppu-2cycle-ale` flag to the unconditional, only PPU fetch path** (ADR 0030), so the shipped default now scores **AccuracyCoin 141/141 (100.00%, RAM-authoritative)** — both **"ALE + Read"** (`$0491`) and **"Hybrid Addresses"** (`$0492`) pass out of the box (previously an honest 139/141). This is the genuine two-dot fetch (even-dot ALE-drive + `octal_latch` load; odd-dot `(address & 0x3F00) | octal_latch` splice + read) where the latch *naturally* carries the stale byte (`copy_v_delay = 4` → NT splice `$2F19` for Hybrid; `$2007`-ALE overlap freeze → `$0FFF` for ALE+Read), replacing v2.0.2's whole-dot `+1 coarse-X` stand-in. **Both experiment flags retired** (`mc-ppu-2cycle-ale` + `mc-ppu-bus-addr-hybrid`); stand-in code deleted; `octal_trace` survives behind the new default-off `ppu-octal-trace`. Verified: **60-ROM oracle 60/60** with two documented re-blesses (SMB3, Uchuu Keibitai SDF — single-tile `$2006`-during-render shifts, more TriCNES-faithful, audio/cycle byte-identical), nestest 0-diff, mmc3 18/18, `ppu_sprites` 19/19; ~10% headless frame-cost rise (~4.15 ms/frame). **Save-state:** additive **`PPU_SNAPSHOT_VERSION` 4 → 5** tail (netplay-rollback determinism; pre-v5 `.rns` still load; forward-incompatible with ≤v2.0.2 but not an ADR-0028 epoch break). Also: the **Harbor Android foss/play monetization glue** (step 5 — AppLovin MAX + RevenueCat 8.10.0 `MonetizationGate`, gating/paywall/session/progress; no-op `foss` twin; both flavors assemble, dormant pending v2.0.9 on-device verify) + a **host-localizable mobile bridge-warning** API (`HostWarning` enum + `drain_warning_codes()`). See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.0.3]` + `to-dos/plans/v2.0.3-2cycle-ale-plan.md`.
- **Earlier in the train:** **RustyNES v2.0.2 "Harbor" ("Soundings")** (2026-07-08) — the second release of the **v2.0.x mobile-finalization train** and Harbor's **headline accuracy release**: the two new upstream AccuracyCoin PPU tests v2.0.1 documented as honest gaps — **"ALE + Read"** (`$0491`) and **"Hybrid Addresses"** (`$0492`) — are now **solved flag-on** by a whole-dot port of TriCNES's **octal-latch multiplexed-bus PPU model** (ADR 0030, commit `27c103c`), behind the pre-existing default-off `mc-ppu-bus-addr-hybrid` flag. **Shipped default stays honest 139/141 (98.58%), byte-identical to v2.0.1; flag-on the same build is verified 141/141 (100.00%)** (framebuffer 100%, nestest 0-diff, mmc3 A12 + IRQ all pass, `ppu_sprites` 19/19). The campaign corrected two ADR 0030 premises — **Mesen2 does NOT pass these tests** (both bytes `0x0A`; the correct oracle is TriCNES, the AccuracyCoin author's own MIT emulator, `ref-proj/TriCNES` commit `9199870`), and **a whole-dot port suffices** (the full 2-cycle-ALE refactor was not required). Per the maintainer's **refine-then-promote** decision (ADR 0030), the flag ships **default-off** in v2.0.2 and is **promoted to default (shipped 141/141) in v2.0.3** — after the Hybrid `+1 coarse-X` approximation is reworked to a first-principles latch-carry model and gated on the 60-ROM commercial byte-identity oracle. No snapshot-format bump (`PPU_SNAPSHOT_VERSION` stays 4). **This release does not claim the shipped build is 141/141, nor that the flag is promoted.** See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.0.2]` + `to-dos/plans/v2.0.2-harbor-plan.md`.
- **Earlier in the train:** **RustyNES v2.0.1 "Harbor" ("Mooring")** (2026-07-08) — the first release of the **v2.0.x mobile-finalization train** on the v2.0.0 "Timebase" core: the Android core re-port + `foss`/`play` flavor-split scaffolding (ADR 0025), the **AccuracyCoin oracle re-sync** (catalog 144→146 rows / 139→141 assigned; measured honestly at **139/141, 98.58%** — the two new upstream PPU tests "ALE + Read" / "Hybrid Addresses" documented as gaps, then solved flag-on in v2.0.2 per ADR 0030), the **CI cost optimization** (heavy suite gated to `release/*` + a weekly cron), the **dependency sweep** (uniffi 0.32 / mlua 0.12 / wgpu-naga 29.0.4 / cc 1.2.66; wgpu 30 deferred on the egui 0.35 pin), and the **`mc-r1-dmc-abort-probe` housekeeping removal**. Every core change is behaviour-neutral, so the deterministic core is byte-identical to v2.0.0: the **139 passing** AccuracyCoin tests and nestest 0-diff are unchanged — only the *denominator* grew (139→141) as the oracle re-sync added the two new upstream PPU tests. See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.0.1]` + `to-dos/plans/v2.0.1-harbor-plan.md`.
- **Preceding additive/platform release:** **RustyNES v1.10.0 "Arcade"** (2026-07-01) — the native **Libretro core** (`crates/rustynes-libretro` builds `rustynes_libretro` for RetroArch: allocation-free video, batched-audio dynamic-rate sync, WRAM/SRAM RetroAchievements maps, deterministic rollback-ready save-states) plus the egui 0.34.3 → 0.35.0 dependency-tier refresh. This is the latest in an unbroken additive/off-by-default chain running all the way back to v1.0.0: the **v1.1.0 → v1.7.1 "Forge"** desktop-feature line, the **v1.8.0 → v1.8.9 "Atlas"** Android platform train, and the **v1.9.0 → v1.9.9 "Workshop"** iOS TestFlight train (see the sub-bullets below for each). AccuracyCoin has held **100.00% (139/139)** and nestest **0-diff** through every one of these releases; mapper coverage is **172 families** (Core / Curated / BestEffort, CI honesty-gated). RustyNES ships as: a native desktop app (Linux/macOS/Windows), a WebAssembly build (browser demo), a native Android app (GitHub-sideload; Google Play deferred to v2.1.0), a native iOS/iPadOS app (TestFlight; App Store deferred to v2.1.0), and a native Libretro/RetroArch core. See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[1.10.0]`…`[1.0.0]`.
- **v2.0.0 "Timebase" — released 2026-07-03.** The forward architectural milestone this Status block used to describe as a distant, high-risk future refactor (see "The path to v2.0.0" below, now updated) has landed and shipped: the one-clock/every-cycle-bus-access scheduler promote (beta.1→beta.4, PRs #217-220), full Vs. `DualSystem` dual-console support with a real commercial-title boot (beta.5, PR #221), and the save-state/movie format break + the two capstone ADRs (rc.1, PR #222 — ADR 0028 save-state v3 + ADR 0029 the timebase architecture) are all merged to `main`. AccuracyCoin held 100% (139/139) at every gate across all five betas + rc.1. The MMC3 R1/R2 IRQ-timing residual was investigated exhaustively (21+ documented attempts total, including two dedicated 2026-07-02 campaigns) and is by-design-deferred beyond v2.0.0 with a mechanism-level explanation (ADR 0002's decision-update section) rather than closed — this is the one known gap in an otherwise complete cut. The tag + release-ceremony + binary publish are done, and the **v2.0.1 "Harbor" ("Mooring")** train now builds on it.
- **RustyNES feature/platform-release history (on the v1.0.0 core; all additive / off-by-default; AccuracyCoin held 100% (139/139) throughout):**
  - **v1.1.0 "Scriptable"** (2026-06-15) — full NES_NTSC composite + CRT/scanline shaders + `.pal` palette filters; NES Power Pad + turbo/autofire + an input-display overlay + a per-game nametable-mirroring override DB; debugger breakpoints + a cycle trace logger + an event viewer (behind `debug-hooks`); an NSF/NSFe player + a 5-band graphic EQ; and the flagship **Lua scripting engine** (`rustynes-script`, ADR 0010). See `CHANGELOG.md` `[1.1.0]`.
  - **v1.2.0 "Curator"** (2026-06-15) — library / compatibility / reach: mapper tiering (Core / Curated / BestEffort, ADR 0011) **51 → 87 families** behind a CI honesty gate; `.zip` loading + `.ips`/`.ups`/`.bps` soft-patching; a per-game DB + in-app ROM-Database editor; live NTSC knobs + a composable ShaderStack + CRT preset bank (ADR 0013) + a default-off HD-pack loader; Family BASIC keyboard / SNES mouse / Arkanoid-both-ports / Game-Genie code DB; Lua `onNmi`/`onIrq`/`setInput`; menu-bar UX + FontAwesome icons; web touch controls + Power Pad + an experimental wasm Lua piccolo backend (ADR 0012); a turn-key netplay `deploy/` bundle; and a PGO CI gate. The SMB3 World 1-1 sprite-flicker (a PPU OAM-row-corruption bug) and the Mapper 89 bus conflict were fixed. See `CHANGELOG.md` `[1.2.0]`.
  - **v1.3.0 "Bedrock"** (2026-06-16) — toolchain modernization (edition 2024 / Rust 1.96 / egui 0.34.3 + wgpu 29.0.3 + rfd 0.17.2); a frame-pacing fix; a Memory Compare panel + a menu/Settings reorg + per-setting auto-save; mapper coverage **87 → 101 families** + Vs. DualSystem header detection (NES 2.0 byte-13); HD-pack `<condition>`/`<background>` rules (ADR 0014); netplay desync diagnostics + niche peripheral aliases; and a PGO/BOLT CI gate. See `CHANGELOG.md` `[1.3.0]`.
  - **v1.4.0 "Fidelity"** (2026-06-16) + the **v1.4.1** patch (2026-06-16) — accuracy polish (triangle ultrasonic silence; the DMC-DMA ↔ controller-read conflict verified + documented); per-channel audio mixing; devtools finish (symbol-file `.sym`/`.mlb`/`.nl` loading + event breakpoints); browser QoL (wasm `.rnm` movie I/O + IndexedDB save-states); a measure-first perf pass (−8% on the rendering-heavy bench); a clap-4 styled `--help` + a `rustynes help` ratatui TUI (native-only); and mapper coverage **101 → 113 families** (boot-smoke verified). v1.4.1 added four more BestEffort boot/decode fixes (m92 / m94 / m145 / m147) + a screenshot-corpus tier reorg. See `CHANGELOG.md` `[1.4.0]` + `[1.4.1]`.
  - **v1.5.0 "Lens"** (2026-06-17) — the insight + scriptability + creator-tooling + polish release, eight additive workstreams: debugger visualization (Input Miniatures overlay, PPU event-viewer heatmap, per-scanline trace viewer, HD-pack per-pixel inspector); Lua dev/TAS API depth; creator/TAS tooling (a TASVideos compatibility pass, NSF waveform scope); frontend pacing & audio-sync perf; a native-UI overhaul + in-app Documentation pane; UX polish (named-palette editor, an "Enhancements" group with sprite-limit-disable/overclock staged-but-inert pending v2.0 per ADR 0002); accessibility (UI scaling, high-contrast + Okabe-Ito themes, keyboard-only nav); mapper breadth **113 → 123 families**; and casual-mode browser RetroAchievements *scaffolding* (ADR 0015, off-by-default `browser-cheevos`). See `CHANGELOG.md` `[1.5.0]`.
  - **v1.6.0 "Studio"** (2026-06-18) — the studio / TAS-tooling / debugger-depth / accuracy-and-breadth release: the TAStudio piano-roll TAS editor + `.fm2`/`.bk2` movie interop + Lua driving/data; Mesen2-class debugger depth (expression/conditional breakpoints + R/W/X watchpoints + a hex editor + RAM search); off-axis-accuracy verification; mapper breadth → **150 families** + the UNIF (`.unf`) loader; FDS-proper; A/V recording; HD audio; and the shader/filter ecosystem (LMP88959 NTSC/PAL + hqNx/xBRZ + constrained `.slangp`/`.cgp` import). See `CHANGELOG.md` `[1.6.0]`.
  - **v1.7.0 "Forge"** (2026-06-19) + the **v1.7.1** patch — the writable/programmable-tooling + accuracy + mapper-breadth + reach release (MAXIMAL A–H over five betas + a wave-2 reach pass): F accuracy hardening; G1 reusable-ASIC mappers **150 → 168 families**; A editing-capable tools + inline 6502 assembler; C debugger depth (callstack/step + `.dbg` source maps); B scriptable TAStudio (`tastudio.*`) + full Lua parity; E host IPC/automation behind the off-by-default `script-ipc` feature (ADR 0016); D Zwinder rewind + movie import; G2/G3 expansion-audio; G5 HD-Pack Builder (ADR 0017) + the real-Mesen `<tile>` loader fix (ADR 0018); plus the H1–H9 reach wave (browser-RA finish + RA HUD, spectator netplay, per-game `<rom>.json` overrides + DIP editor + lag counter (ADR 0019), audio depth (ADR 0020), web/wasm parity (ADRs 0021/0022), an i18n framework (ADR 0023), and the `full` maximal-native-feature build). v1.7.1 added seven bugfix/polish fixes. See `CHANGELOG.md` `[1.7.0]` + `[1.7.1]`.
  - **v1.8.0 … v1.8.9 "Atlas"** (2026-06-19 … 2026-06-20) — the **Android platform train** (the first *platform* releases; new crates `rustynes-mobile` UniFFI bridge + `rustynes-android` JNI glue + an `android/` Gradle/Compose app, ADR 0024). v1.8.0 foundation → v1.8.5 power-user (palette/HD-pack/`.zip`/movies) → v1.8.6 (Lua + RA + direct-IP/LAN netplay) → v1.8.7 "Connectivity completion" (CGNAT/TURN room-code netplay + robust hardware controllers P1–P4) → v1.8.8 "Atlas" (AGP9/Gradle9 + Window-Size-Class adaptive + edge-to-edge/Material You; EN/ES i18n; box-art library; Baseline Profiles + R8 full-mode; capture/MP4-clip + PiP/tile/shortcuts/Glance-widget; TV/Leanback + a11y; Play Games cloud-saves/achievements + Play-Integrity + update/review/vitals, all default-off) → **v1.8.9** (13-PR Dependabot consolidation; the dormant `rustynes-monetization` crate wired into the Android build, not yet live — activates at v2.1.0). See `CHANGELOG.md` `[1.8.0]`…`[1.8.9]`.
  - **v1.9.0 … v1.9.9 "Workshop"** (2026-06-25 … 2026-06-26) — the **iOS/iPadOS TestFlight train**, mirroring the Android arc release-for-release on the byte-identical core (new crates `rustynes-ios` Metal/CoreAudio shim reusing `rustynes-mobile` verbatim, ADR 0026). v1.9.0 "Sunrise" foundation (SwiftUI shell + xcframework) → v1.9.4 "Lens" (full wgpu→Metal renderer + WGSL shader stack) → v1.9.6 "Link" (Lua + RetroAchievements + LAN netplay) → v1.9.7 "Relay" (CGNAT/TURN room-code netplay + iCloud/CloudKit save-state sync) → v1.9.8 "Horizon" (accessibility + EN/ES i18n + ReplayKit + Game Center + the dormant StoreKit seam, ADR 0027 §4.7 compliance) → **v1.9.9 "Workshop"** (creator/power tools: Cheats, a FOSS-gated read-only debugger, a touch TAStudio piano-roll, foreign movie import, host-side audio-depth DSP — the final pre-Timebase readiness gate). Distributed by TestFlight only; App Store deferred to v2.1.0 alongside Google Play. See `CHANGELOG.md` `[1.9.0]`…`[1.9.9]`.
  - **v1.10.0 "Arcade"** (2026-07-01) — the native **Libretro core** (`crates/rustynes-libretro`, RetroArch integration) + the egui 0.34.3 → 0.35.0 dependency-tier refresh. See `CHANGELOG.md` `[1.10.0]`.
- **In development — the v2.0.0 tag itself.** All development work for v2.0.0 "Timebase" is merged to `main` (see the bullet above); the only remaining step is the release ceremony (pre-release gate checklist, tag, `release-auto.yml` binary publish).
- **Engine-lineage — the "optimized performance" pass** (folded into v1.0.0): a frontend + build performance pass — a Performance panel + CSV "Logging" checkbox; a lock-free SPSC audio ring + **dynamic rate control**; a **display-sync pacing matrix** (`auto|display|vrr|wallclock`) + **late input latch**; a **snapshot fast path** (36→14.6 µs) + **run-ahead** (default 1, persistent timeline byte-identical); **mapper-caps + pixel-LUT + fat-LTO + SIMD** (**−26%** rendering-heavy bench, −16% nestest); a **dedicated emulation thread** (default-ON `emu-thread`, lock-free `SharedInput`, netplay-pause TOCTOU-closed) + best-effort Linux priority elevation; and a browser **AudioWorklet** + **rAF display-sync**. See `docs/release-notes/v2.8.0.md` (engine-line detail).
- **Engine-lineage — the master-clock milestone:** the engine's v2.0 line made the R1 `u64` master clock the default (AccuracyCoin 90.65%→**100.00%**, region-exact 3.2:1 PAL via the unified DMA engine) and then removed the legacy integer-lockstep scheduler (R1 is the only path; the `mc-r1-*` flags no longer exist). See `docs/audit/v2.0-phase7f-r1-default-promotion-2026-06-10.md`.

> **The bullets that follow (down to the engine-lineage Phase 6 entry) are the
> inbound engine's own release line — its `v1.x`/`v2.x` tags + 2026-05-2x dates.
> They are *engine history*, folded into the RustyNES v1.0.0 core; they are NOT
> the RustyNES v1.x feature releases listed at the top of this Status block.**

- **Engine-lineage phase:** **engine v1.7.0 (2026-05-25)** — **niceties milestone**: Four Score 4-player support (bus `$4016`/`$4017` 24-read multiplex of 4 controllers + adapter signature; opt-in, OFF by default = byte-identical two-controller reads; a P3/P4 keyboard + gamepad rebind UI + a "Four Score" toggle), GameShark-style raw RAM cheats (`Nes::poke_ram` applied caller-side after `run_frame`, alongside the v1.6.0 Game Genie support; a `RawCheat` `$addr=$value [if $compare]` section in the cheat panel persisted per-ROM), and an in-app graphics/audio/rewind settings panel. **Additive, independent of the deferred v2.0 master-clock axis**; AccuracyCoin held **90.65%**, oracle 60/60, sacred trio + B4 byte-identical, determinism preserved. Workspace `--features test-roms`: **702 strict + 10 ignored**. See `docs/audit/gap-analysis-remediation-plan-2026-05-25.md` §2 + `CHANGELOG.md` `[1.7.0]`.
- **Engine-lineage phase:** **engine v1.6.0 (2026-05-25)** — **frontend-polish milestone** (the engine's v2.0.0 plan's original v1.5.0 content, deferred when Phase 7 took that slot). **Additive, independent of the deferred v2.0 master-clock axis**; AccuracyCoin held **90.65%**, oracle 60/60, sacred trio + B4 byte-identical, determinism preserved. Landed across 6 sprints: (0) `x86_64-apple-darwin` release target dropped (ADR 0009, Aug-2027 runner sunset); (1) Game Genie cheats (core `rustynes-core/src/genie.rs` runtime overlay — off by default, not in the save-state — + a debugger cheat panel with per-ROM persistence); (2) in-app gamepad rebinding UI (config-driven `[input.gamepad1/2]` + P2 keyboard rows + axis-as-dpad; serde default = the legacy Xbox layout); (3) controls/configuration doc-sync; (4) browser (wasm) `.rnm` movie download/upload + localStorage save-states; (5) a non-flaky frame-time regression CI gate + a rendering-heavy `flowing_palette` bench. Workspace `--features test-roms`: **688 strict + 10 ignored**. See `docs/audit/gap-analysis-remediation-plan-2026-05-25.md` + `CHANGELOG.md` `[1.6.0]`.
- **Engine-lineage phase:** Phase 7 — **engine v1.5.0 (2026-05-24)**: **Nesdev Accuracy Hardening** (the genuinely-skipped phase; see numbering note below). Coverage + region validation + developer ergonomics + documented scope closure — **additive only**, AccuracyCoin held at **90.65%**, oracle 60/60, sacred trio + B4 byte-identical. Landed across 4 sprints: (1) blargg `instr_misc`/`instr_timing`/`cpu_reset` corpus wired (+8 strict); (2) seeded power-on RAM randomization developer mode (`Nes::from_rom_with_power_on_seed`; default path unchanged) + NMI/IRQ B-flag + `$4015` open-bus guards; (3) automated PAL/Dendy timing gates (per-region constant table + frame-structure integration test); (4) VRC2/4 + M34 NINA-001 submapper fixtures (replacing the rotted `vrc24test`) + `compatibility.md` platform-scope closure (FDS plan, Vs/PC10, PPU variants, input devices, long-tail policy). Workspace `--features test-roms`: **661 strict + 10 ignored**. Deferred to v2.0 (master-clock axis): C1 IRQ-sample, `$2002` sub-cycle, SH\* internal-bus, stale-shifter, `$2007` rendering, FDS code, PAL 3.2:1 CPU:PPU ratio. See `docs/audit/phase-7-*` + `CHANGELOG.md` `[1.5.0]`.
- **Engine-lineage phase:** Phase 10 — **engine v1.4.0 (2026-05-24)**: **TAS movie recording/playback**. Deterministic `.rnm` record/replay + save-state branching (ADR 0008: `RNESMOV1` header + ROM SHA-256 + optional `.rns` start point + per-frame input stream); `MovieRecorder`/`MoviePlayer` in `rustynes-core` (no_std) + record/play/branch hotkeys (`F6`/`F7`/`F8`) + a read-only REC/PLAY egui overlay; native `.rnm` save/load (wasm I/O is a follow-up). No API break (additive `Nes::buttons` getter; `run_frame` byte-for-byte unchanged) → oracle 60/60, AccuracyCoin 90.65%, B4 + sacred trio preserved. Determinism proven by byte-identical round-trip tests; **636 strict + 8 ignored**. Clean-room from Mesen2 `Core/Shared/Movies/` + FCEUX `.fm2` + TetaNES `.replay`. Delivered across Sprints 4.1 (core) + 4.2 (frontend UI). The prior **Phase 9 — v1.3.3 RELEASED (2026-05-24)**: bug-fix patch (frontend-only; native unchanged, pixel-identical) closing two wasm/GitHub-Pages issues + a native pacing refinement — (1) wasm/Pages severe stutter + freezes (v1.3.2 regression): the wasm idle path busy-looped on `ControlFlow::Poll` alongside the rAF loop + a missing `request_redraw()` re-arm could stall it; fixed to `ControlFlow::Wait` + an unconditional rAF re-arm; (2) wasm/WebGL2 palette wrong: wgpu-hal double-encodes sRGB on the GL surface, so the GL pipeline now stays UNORM (zero conversion, matches the correct canvas-2D path); native keeps sRGB → pixel-identical; (3) native residual stutter: chunked pacer sleep + 2 ms spin margin. Both wasm fixes need browser confirmation. Workspace **616 strict + 6 ignored** (unchanged). The prior **v1.3.2 RELEASED (2026-05-24)** closed two v1.3.1 follow-ups: dead keyboard input after the config migration (`parse_keycode` legacy keycode aliases) + a first wasm rAF-pacing attempt. **v1.3.1 RELEASED (2026-05-24)** was a bug-fix patch on the v1.3.0 WebAssembly milestone with three fixes (no API break, no accuracy change): (1) green/garbage left-edge column while scrolling — BG attribute (palette) shifters were one tile out of phase with the pattern shifters (`086ce4d` regression), now 16-bit + lockstep (AccuracyCoin-neutral; PPU save-state v1→v2); (2) stutter / non-smooth framerate — configurable present mode (default `Mailbox`) + a native sleep-then-spin frame pacer replacing the jittery `ControlFlow::WaitUntil` cadence (user-confirmed smooth); (3) legacy `config.toml` now migrated in place (backup + loud summary) instead of silently dropped. MM3 MMC3 stage-select shear investigated, confirmed not-a-regression, deferred to v2.0 (C1 axis). Oracle 60/60; AccuracyCoin 90.65%; B4 + sacred trio preserved. See `CHANGELOG.md` `[1.3.1]`. **v1.3.0 (2026-05-24)** landed the WebAssembly target: `wasm32-unknown-unknown` frontend in two flavours (`wasm-winit` default = full winit+wgpu+egui, 2.12 MiB gzip; `wasm-canvas` ~316 KB embed), GitHub Pages deploy (`https://doublegate.github.io/RustyNES/`), CI `wasm` clippy job + 5 MiB size-budget gate, all Pages actions on Node 24 — delivered across Sprints 1.1 → 1.2 → 1.3 → 1.4a → 1.4b → 1.4c → 2.
- **Engine-lineage phase:** Phase 8 — **engine v1.2.0 (2026-05-24).** DMC DMA scheduler refactor landed under default-off cargo feature `dmc-get-put-scheduler` introducing Mesen2's canonical get/put cycle alternation model alongside the v1.1.0 phase-agnostic scheduler via the parallel-implementation pattern (ADR 0007). AccuracyCoin DMA cluster under flag-on: **6/10 match baseline** (closing 4 → 0 deferred to v1.2.x patches or v2.0 master-clock absorption). Default build bit-identical to v1.1.0.
- **Engine-lineage — earlier work:** **engine v1.1.0 (2026-05-25)** — VRC7 OPLL FM audio via clean-room pure-Rust port of `emu2413 v1.5.9` (MIT); ADR 0006 supersedes ADR 0004; *Lagrange Point* plays with audio. (engine v1.1.0 was an engine v2.0.0-release-plan milestone slotted between Phase 6 and Phase 8, **not** the ROADMAP's Phase 7 — see the numbering note below.) Phase 6 — **engine v1.0.0 (2026-05-23)**: AccuracyCoin gate CLEARED at 90.65% (126/139); T-60-001 C1 IRQ-timing residuals (3 `cpu_interrupts_v2` sub-ROMs + `mmc3_test_2/4` #3) deferred to the master-clock-precise scheduling refactor (Session-29 empirically falsified Option A global PPU-position shift; 17 documented rollbacks). [That engine-lineage master-clock work subsequently landed in the RustyNES v1.0.0 core, taking AccuracyCoin to 100%.]
- **Phase-numbering note:** the shipped releases v1.1.0 → v1.4.0 were sequenced from the v2.0.0 release plan and back-labelled in the detailed sections as v1.1.0 (VRC7) → Phase 8 (v1.2.0 DMC) → Phase 9 (v1.3.0 wasm) → Phase 10 (v1.4.0 TAS). **Phase 7 — Nesdev Accuracy Hardening (below) was authored but never executed**; it is now being executed as **v1.5.0**. See `docs/audit/phase-7-assessment-2026-05-24.md` for the full intent-vs-accomplished-vs-completable disposition.
- **Current state:** **RustyNES v1.10.0 "Arcade" — the latest tagged release; v2.0.0 "Timebase" is code-complete on `main` with the tag pending.** Every accuracy, compatibility, platform, netplay, RetroAchievements, FDS, Vs/PC10, and performance milestone in the engine-lineage history above is folded into the v1.0.0 core; the v1.1.0 → v1.7.x feature releases then layered (in order) the Lua scripting engine + visual filters/peripherals/devtools/NSF, the library/compatibility/reach pass, the toolchain modernization + Memory-Compare + Vs.-DualSystem detection, the accuracy-and-finish pass, the insight/scriptability/creator-tooling/polish pass, the studio/TAS-tooling/debugger-depth pass, and the writable/programmable-tooling "Forge" pass; the v1.8.x train ported the whole core to Android, the v1.9.x train ported it to iOS/iPadOS, and v1.10.0 added the native Libretro core. Mapper coverage rose **51 → 172 families** across these releases, all additive / off-by-default, with AccuracyCoin holding **100% (139/139)** the entire time. v2.0.0 then landed the one-clock/every-cycle timebase promote + full Vs. `DualSystem` support + the save-state/movie format break — the first genuinely BREAKING release since v1.0.0, by design (ADR 0028/0029). The engine-lineage version markers (v0.9.x → v2.x) in the bullets above and the phase bodies are upstream history, not RustyNES releases.

**v2.0.0 "Timebase" — code-complete, tag pending. What actually shipped (2026-07-01 → 2026-07-03):**

The forward architectural milestone this section used to describe as a distant, XL/HIGH-risk future refactor has landed, across beta.1 → beta.5 → rc.1 (PRs #217-222). What was originally scoped as workstreams A-F in `to-dos/plans/v2.0.0-master-clock-plan.md`:

- **A — the one-clock, every-cycle-bus-access timebase (beta.1 → beta.4, PRs #217-220).** Collapsed the five-counter substrate (`Cpu::master_clock`, `Cpu::cycles`, `LockstepBus::cycle`, `LockstepBus::ppu_clock`, `Apu::cpu_cycle`) to ONE canonical counter; made every CPU instruction cycle a real bus access (no busless filler cycles, matching Mesen2's `StartCpuCycle → Read → EndCpuCycle` split-around-the-access model); a cycle-accurate warm-reset sequence. Promoted to the shipped default in beta.4 (BREAKING by design, ADR 0029). AccuracyCoin held 100% (139/139) at every gate.
- **B — residual closure (beta.3 + the 2026-07-02 bounded-effort campaign).** R3 (`apu_reset/len_ctrs_enabled`) closed — reclassified as a harness bug, not a core residual. R4 (`apu_reset/4017_written`) closed via the cycle-accurate reset. R5 (DMC-DMA span) found already-closed pre-beta.1. **R1/R2 (the MMC3 IRQ-timing bracket) investigated exhaustively — 21+ documented attempts total (17 historical + 4 new on 2026-07-02) — and by-design-deferred beyond v2.0.0**, not closed: a mechanism-level finding (the bracket measures a differential interval invariant to any consistent batch re-phasing) explains why every phase/order lever has failed, and identifies the true fix as needing a genuinely finer-than-CPU-cycle scheduler granularity. See ADR 0002's 2026-07-02 decision-update for the full evidence trail and the DO-NOT-RETRY list.
- **C — full Vs. `DualSystem` dual-core support (beta.5, PR #221).** The four `DualSystem` cabinet boards (Tennis, Mahjong, Wrecking Crew, Balloon Fight) now construct and run as genuine two-console pairs via the `Emu` enum front door — core-and-test-harness-only this release (frontend dual-console rendering deferred). **Vs. Balloon Fight boots to a legible, correct attract-mode screen** on a combined dump assembled from a legitimately-owned MAME romset (the previously-circulating "GVS" dumps are provably incomplete — MAME `maincpu` region only, confirmed by CRC32 cross-reference). Wrecking Crew is inconclusive (cross-wiring demonstrably active, but no confirmed title screen); Tennis and Mahjong remain infrastructure-only (no local sub-CPU dump available). This retires the "not yet emulated" DualSystem deferral this section used to carry.
- **D — the breaking-API/save-state/doc-baseline close (rc.1, PR #222).** `CPU_SNAPSHOT_VERSION` 2→3 + `save_state::FORMAT_VERSION` 1→2 (ADR 0028 — clean rejection of pre-v2.0.0 `.rns` slot files, no migration code, per ADR 0003's own MAJOR-boundary policy) and `MOVIE_FORMAT_VERSION` 1→2 (warn-not-reject for `.rnm` movies — input replay still works, the bit-identical guarantee is flagged unverified across the boundary). ADR 0029 formalizes the one-clock timebase as the canonical architecture, superseding the dot-lockstep framing; `docs/architecture.md` got the same banner treatment `docs/scheduler.md`/`docs/cpu-6502.md`/`docs/apu-2a03.md` already had.
- **E — mapper breadth.** Frozen at **172 families** for the v2.0.0 cut (no mapper work landed in the v2.0.0 line — confirmation, not a change).
- **F — perf re-baseline.** Done in beta.4; both configurations clear the 16.639 ms NTSC deadline with wide margin.

**Remaining:** the tag + release-ceremony + binary publish. Once tagged, v2.0.0 becomes the prerequisite the mobile finalization train below has been waiting on.

**Beyond v2.0.0 — the mobile finalization train (maintainer decision, 2026-06-23; unchanged by v2.0.0's completion, just now unblocked):**

- **The Android (v1.8.x) and iOS (v1.9.x) apps ship together, after v2.0.0 — the v2.0.1 → v2.1.0 finalization train.** Both apps were deliberately held back from their app stores until the v2.0.0 "Timebase" core landed, so they can finalize and launch **together**: **v2.0.1–v2.0.4** = final Android additions/modifications/enhancements/fixes re-ported onto the v2.0.0 core; **v2.0.5–v2.0.8** = the same iOS finalization; **v2.0.9** = true correctness checks + ready-for-release verification for *both* apps; **v2.1.0** = the **joint mobile store launch** — Google Play + Apple App Store + AltStore PAL + F-Droid together. Until then the apps continue as **GitHub-sideload** (Android) and **TestFlight** (iOS v1.9.0–v1.9.9, already complete) only. Full plan: [`plans/v2.0.x-mobile-finalization-plan.md`](plans/v2.0.x-mobile-finalization-plan.md).
  - **Mobile monetization (v2.1.0).** An ad-supported freemium model — **AppLovin MAX** + **RevenueCat**, a **$3.99** one-time "Full Version / Remove Ads" unlock, +11 min × 2 rewarded ads → a 30-minute session, and six premium features. The dormant `rustynes-monetization` crate (wired into the Android build since v1.8.9) activates here.
  - **The `foss` / `play` Android flavor split (ADR 0025).** A **`foss`** flavor (default — no Google SDKs, no ads, no tracking; the F-Droid + sideload artifact) and a **`play`** flavor (all proprietary SDKs, for Google Play). The five proprietary subsystems move behind `src/play/` façades (no-op in `src/foss/`); both flavors are verified on-device at v2.0.9.
- **Beyond v2.1.0 (separate initiatives, no fixed version yet).**
  - **The R1/R2 MMC3 IRQ-timing axis** — the one open technical gap from v2.0.0 (see above). Next credible avenue per the 2026-07-02 campaign: M2-edge-precise (not CPU-cycle-integer) `gap >= 3` low-time accounting on the falling edge, an axis distinct from everything tried so far — genuinely untested, flagged for a future dedicated session rather than squeezed into any near-term release.
  - **Vs. Tennis and Vs. Mahjong DualSystem boot** — needs the missing sub-CPU program dumps (not available locally as of v2.0.0; Balloon Fight and Wrecking Crew's dumps were sourced from a legitimately-owned MAME romset).
  - **Vs. DualSystem frontend integration** — dual-console rendering + 4-port input routing; core-only as of v2.0.0.
  - **Browser / wasm Lua** maturity (the native Lua engine is feature-complete; the wasm piccolo backend, ADR 0012, is explicitly not byte-parity with native mlua).
  - **Finishing browser RetroAchievements** — the v1.5.0 scaffolding (ADR 0015, off-by-default `browser-cheevos`) needs the auth-proxy deploy, the wasm trampoline marshalling, and a live-browser verify; native RA is unaffected. Plus the live RA-account allowlisting pass with the RA team (the `RustyNES/<ver>` User-Agent is already sent; the allowlisting itself is a request, not a code change).
  - **Long-tail mapper coverage** toward the full ~300-mapper set + **100% TASVideos** compatibility.
- **Engine-lineage forward-roadmap history (folded into the v1.0.0 core; retained for context — NOT a RustyNES release plan):** the inbound engine's own roadmap completed engine v2.6.0 (Vs/PC10 RGB game-verified, +11 mappers→51, N-peer netplay, real-BIOS FDS), engine v2.7.0 (RetroAchievements via the vendored rcheevos FFI; the Vs.-System per-game DIP/2C04-palette DB; deployable browser WebRTC netplay), and engine v2.7.1 (netplay-hardening + live verification, the `power_cycle` cold-boot desync fix, the >2-player browser WebRTC mesh, RA fixes, the MMC6 PRG-RAM fix, the NTSC-filter WGSL crash fix, Vs. DualSystem detection groundwork). All of this is present in the RustyNES v1.0.0 core; stock NES is byte-identical and AccuracyCoin is 100%.
- **Done:** Phases 1-4 complete; Phase 5 Sprints 1-3 shipped — Frontend MVP, save state + rewind + TOML rebinding, egui debugger overlay (CPU/PPU/OAM/APU/memory/mapper panels + in-app rebind modal closing T-52-007), simplified Blargg-style NTSC wgsl post-pass, release workflow + README badges. **Regression-prevention buildout closed (2026-05-17):** 21-ROM permissive baselines + 60-ROM commercial-ROM oracle (54 strict + 6 ignored across 15 mappers) + 81-PNG visual corpus + permanent `scripts/regression-bisect/` tooling + `docs/audit/` decision-rationale tier. Real-game regression on SMB / Excitebike / Kid Icarus closed by the FSM dot-64 reset fix on `accuracy-stabilization` (`834be9e`). Residual accuracy gaps tracked in `CHANGELOG.md` `[Unreleased]` → "Investigated and rolled back". (Historical note: when this bullet was written, v1.0.0 was still gated on the C1 IRQ-timing rework + AccuracyCoin ≥ 90% (then 69.78%) + multi-OS smoke + the 6 ignored commercial ROMs. All of those resolved: **v1.0.0 released** (the 90.65% gate was an interim engine-lineage milestone), the 6 ROMs are strict-passing, and the master-clock refactor (the engine-lineage "v2.0" axis) **shipped as the v1.0.0 default core**, closing the C1 + sub-cycle residuals — the default build measures **AccuracyCoin 100%**. See `docs/audit/gap-analysis-remediation-plan-2026-05-25.md` for the historical trajectory.)
- **Status matrix (single source of truth):** see [`docs/STATUS.md`](../docs/STATUS.md) for the per-test-ROM-suite pass count, mapper coverage matrix, feature flag state, and version policy. This roadmap intentionally keeps a short summary only.
- **Deferred / carryover backlog:** see [`DEFERRED-AND-CARRYOVER-FEATURES.md`](DEFERRED-AND-CARRYOVER-FEATURES.md) for the consolidated catalogue of every deferred, carried-over, manual-verify, and not-yet-implemented feature (reconciled against `main`), grouped by theme with target releases and source plans/ADRs.

## Phases

> **Reminder:** the `v1.x`/`v2.x` version tags inside the Phase bodies below are
> **engine-lineage** markers (the inbound engine's own line, dated 2026-05-2x),
> retained as historical anchors. They are **not** the RustyNES v1.1.0 → v1.8.8
> feature/platform releases (dated 2026-06-1x) tracked in the Status block above,
> and the
> Phase-body "v2.0" deferrals refer to the *engine's* master-clock work that
> already shipped in the RustyNES v1.0.0 core — distinct from the forward
> **RustyNES v2.0.0** timebase refactor (ADR 0002) in "The path to v2.0.0".

### Phase 1 — Foundation

**Goal:** Empty Cargo workspace builds cleanly with CI green; cartridge parser passes round-trip tests; CPU executes the nestest golden log without diverging.

**Exit criterion:** `cargo test --workspace` green; `nestest.nes` golden-log compare passes; iNES + NES 2.0 parser handles the test ROM corpus without errors.

**Estimated duration:** 4-6 weeks

[Phase 1 overview](archive/phase-1-foundation/overview.md)

Sprints:

- [Sprint 1 — Workspace + CI + lints](archive/phase-1-foundation/sprint-1-workspace.md)
- [Sprint 2 — Cartridge parser (iNES + NES 2.0)](archive/phase-1-foundation/sprint-2-cartridge.md)
- [Sprint 3 — CPU core: official opcodes](archive/phase-1-foundation/sprint-3-cpu-official.md)
- [Sprint 4 — CPU core: unofficial opcodes + nestest](archive/phase-1-foundation/sprint-4-cpu-unofficial.md)

---

### Phase 2 — Graphics + Timing

**Goal:** PPU renders correct pictures for NROM, MMC1, UxROM, AxROM, CNROM, GxROM titles; lockstep scheduler operational; blargg PPU test ROMs pass.

**Exit criterion:** `ppu_vbl_nmi/*`, `ppu_open_bus`, `sprite_overflow_tests/*`, `oam_read`, `oam_stress` all pass; visual diff against Mesen2 reference for a curated demo set.

**Estimated duration:** 6-8 weeks

[Phase 2 overview](archive/phase-2-graphics-timing/overview.md)

Sprints:

- [Sprint 1 — PPU bus, registers, memory map](archive/phase-2-graphics-timing/sprint-1-ppu-bus.md)
- [Sprint 2 — Background rendering + scrolling](archive/phase-2-graphics-timing/sprint-2-background.md)
- [Sprint 3 — Sprite evaluation + rendering + sprite-zero hit](archive/phase-2-graphics-timing/sprint-3-sprites.md)
- [Sprint 4 — Lockstep scheduler + DMA + simple mappers (NROM, UxROM, CNROM, AxROM, GxROM, MMC1)](archive/phase-2-graphics-timing/sprint-4-scheduler-mappers.md)

---

### Phase 3 — Audio + Polish

**Goal:** APU produces correct audio; lookup-table mixer and analog filter chain in place; band-limited synthesis emits at host sample rate; CPU illegal opcodes complete.

**Exit criterion:** `apu_test/*`, `apu_mixer/*`, `dmc_dma_during_read4/*`, `cpu_interrupts_v2/*` all pass.

**Estimated duration:** 4-6 weeks

[Phase 3 overview](archive/phase-3-audio-polish/overview.md)

Sprints:

- [Sprint 1 — APU channels (pulse 1, pulse 2, triangle, noise)](archive/phase-3-audio-polish/sprint-1-apu-channels.md)
- [Sprint 2 — DMC channel + DMC DMA + frame counter](archive/phase-3-audio-polish/sprint-2-dmc-frame.md)
- [Sprint 3 — Mixer + filters + band-limited synthesis](archive/phase-3-audio-polish/sprint-3-mixer.md)

---

### Phase 4 — Mapper Coverage

**Goal:** Top-25 mappers implemented; MMC3 IRQ accuracy validated; MMC5 (no audio); audio extension mappers (VRC6, Sunsoft 5B, Namco 163) functional.

**Exit criterion:** Per-mapper boot test passes for one ROM per supported mapper; `mmc3_test_2/*`, `mmc3_irq_tests/*`, `vrc24test`, holy_mapperel pass; AccuracyCoin pass rate ≥ 80%.

**Estimated duration:** 6-8 weeks

[Phase 4 overview](archive/phase-4-mapper-coverage/overview.md)

Sprints:

- [Sprint 1 — MMC3 (the defining mid-life mapper)](archive/phase-4-mapper-coverage/sprint-1-mmc3.md)
- [Sprint 2 — MMC2/MMC4 + Color Dreams + CPROM + BNROM/NINA + Camerica + VRC1](archive/phase-4-mapper-coverage/sprint-2-misc-mappers.md)
- [Sprint 3 — VRC2/4/6 + Sunsoft FME-7 + Namco 163](archive/phase-4-mapper-coverage/sprint-3-vrc-extended.md)
- [Sprint 4 — MMC5 (without audio extension)](archive/phase-4-mapper-coverage/sprint-4-mmc5.md)

---

### Phase 5 — Frontend + Tooling

**Goal:** `rustynes` binary playable end-to-end with save state + rewind + debugger overlays + NTSC filter; CI publishes signed binaries on tag.

**Exit criterion:** Binary builds and runs on Linux/macOS/Windows; passes manual smoke test of compatibility-difficulty corpus; release pipeline green.

**Estimated duration:** 4-6 weeks

[Phase 5 overview](archive/phase-5-frontend-tooling/overview.md)

Sprints:

- [Sprint 1 — winit + wgpu + cpal frontend (minimum viable player)](archive/phase-5-frontend-tooling/sprint-1-frontend-mvp.md)
- [Sprint 2 — Save state + rewind + input bindings](archive/phase-5-frontend-tooling/sprint-2-save-rewind.md)
- [Sprint 3 — Debugger overlays (egui) + NTSC filter + release pipeline](archive/phase-5-frontend-tooling/sprint-3-debugger-release.md)

---

---

### Phase 6 — v1.0.0 Closeout (SUPERSEDED — accuracy closed by the engine-lineage master-clock work)

> **Superseded.** The engine-lineage continued past this closeout plan: the
> master-clock refactor took AccuracyCoin to **100.00% (139/139)** and the C1
> IRQ-timing + sub-cycle residuals these sprints chased were closed (or
> documented-deferred) along the way. The sprint backlog below was **not**
> executed as written; it is retained as the historical gate plan. RustyNES
> ships at **v1.0.0** with the accuracy bar fully cleared.

**Original goal (historical):** close all open v1.0.0 gates and ship the v1.0.0
tag.

**Original exit criterion (historical):** `cargo test --features test-roms`
shows the C1 `cpu_interrupts_v2/{2,3,5}` + `mmc3_test_2/4-scanline_timing`
sub-test #3 flipped + AccuracyCoin ≥ 90% + multi-OS release-artifact smoke test
green + the 6 `#[ignore]`'d commercial ROMs investigated. (All resolved by the
engine-lineage work; AccuracyCoin is now 100%.)

[Phase 6 overview](archive/phase-6-v1-closeout/overview.md)
[Phase 6 v1.0.0-final sprint backlog](archive/phase-6-v1.0.0-final/overview.md)
— ordered six-sprint plan to close the AccuracyCoin 90% gate + the 4
C1 IRQ-timing residuals (Sprint 1: Implied-Dummy + DMC coordinated;
Sprint 2: APU put/get phase; Sprint 3: sprite-eval residuals;
Sprint 4: PPU misc residuals; Sprint 5: C1 axis attempt 17;
Sprint 6: SH* unstable stores).

Tickets (informal — formal sprint files when work begins). The `[~]` markers
below are **historical**: they record each ticket's state *at this superseded
phase*, not now — all were closed or documented-deferred by the engine-lineage
master-clock work (current AccuracyCoin **100.00%**). They are not live TODOs.

- [~] **T-60-001 — Coordinated CPU/Bus/PPU IRQ-sample-timing rework
  (Track C1). DEFERRED to v1.x.** 11 independent fix attempts rolled
  back across multiple sessions; no empirical breakthrough on the
  canonical CPU `T_last - 1` IRQ-sample-point axis. Residuals:
  `cpu_interrupts_v2/{2-nmi_and_brk, 3-nmi_and_irq, 5-branch_delays_irq}`
  - `mmc3_test_2/4-scanline_timing` sub-test #3. Infrastructure landed
  (ADR-0002 Decision section + per-CPU-cycle IRQ tracing fixture + 6
  golden baseline traces + M2-phase plumbing + Phase B4 reload-pending
  discriminator). Does not affect any real game; commercial game
  compatibility intact. Carries forward to v1.x roadmap.
- [~] **T-60-002 — Push AccuracyCoin pass rate from 69.78% to ≥ 90%.
  IN PROGRESS at 82.73%** (Cascade B closed 2026-05-19 in commit
  `9b0c81c` + Cascade A partial closure 2026-05-19 via OAMADDR reset
  during dots 257-320 in `f29f7ca` + session-6 `$2004` dots 1-64 `$FF`
  in `6c2664e` + session-7 OAMADDR-walks-during-eval + $4-aligned
  `$2004` write in `c230489` + session-7 RMW ABS,X/Y unfixed-address
  dummy read in `32d5b18` + **session-8 BG-pipeline cycle-9 reload +
  post-emit shift in `086ce4d` (architectural closure of Cascade A's
  `VerifySpriteZeroHits` step-2 geometric puzzle per
  `docs/audit/cascade-a-investigation-2026-05-19.md`)**; trajectory
  `64.03% → 67.63% → 69.06% → 69.78% → 76.98% → 78.42% → 79.14% →
  79.86% → 82.73%`, exceeds CI floor of 0.60 by 22.7pp and
  **CLEARED the v0.9.x 80% target by 2.7pp**). **Cascade B
  (DMC DMA halt-cycle precision) CLOSED** — all 8 tests in "APU
  Registers and DMA tests" flipped + 3 net side-benefit flips
  elsewhere; +11 tests. **Cascade A (Sprite Zero Hit BG-pipeline
  geometry) PARTIALLY CLOSED** — the load-bearing architectural
  axis (BG shift-register cycle-9 reload + post-emit shift per
  Mesen2 + nesdev wiki) landed in session 8, flipping 4 tests
  (Sprite 0 Hit behavior, Sprite overflow behavior, Suddenly
  Resize Sprite, $2007 read w/ rendering). The remaining 24
  failing tests cluster as documented in
  `docs/audit/accuracycoin-readme-analysis-2026-05-17.md`'s
  2026-05-19 addendum +`docs/audit/cascade-a-investigation-2026-05-19.md`'s
  RESOLUTION section:
  - **Cascade A residuals — 10 tests (post-BG-pipeline-fix):** 4
    sprite-eval ($2002 flag timing, Arbitrary Sprite zero, Misaligned
    OAM behavior, OAM Corruption) + 6 PPU misc (Stale BG/Sprite
    Shift Regs, BG Serial In, Sprites On Scanline 0, $2004/$2007
    Stress Tests). Cluster gated on stale-shift-register modeling +
    post-B8 sprite-FSM interactions + $2002 sub-cycle flag timing.
    The session-8 BG-pipeline fix closed the geometric root cause
    (`VerifySpriteZeroHits` step-2) but left these subtler
    cycle-precision residuals for future sessions.
  - **C1 IRQ-timing axis — 5 tests (4 × `cpu_interrupts_v2/{2..5}` +
    `mmc3_test_2/4` sub-test #3) — DEFERRED, see T-60-001.**
  - **Internal-bus model — ~5 tests** (`CPU Behavior :: Open Bus
    [error 9]`, 5 × SH*opcodes `[error 7]`, `CPU Behavior 2 ::
    Implied Dummy Reads [error 2]`). Requires internal-vs-external
    bus model rework that previously regressed Internal Data Bus
    Test 2. The SH* tests are "Coupled to Cascade B" per audit but
    they did NOT flip when Cascade B landed — confirming SH*
    address corruption needs an explicit RDY-low-2-cycles rule
    rather than just DMC DMA halt modeling.
  - **APU residuals — 5 tests** (Frame Counter IRQ, DMC Channel,
    APU Register Activation, Controller Strobing/Clocking). Each
    is a distinct $4015 RMW / put-vs-get-cycle bracket; bundled
    with the internal-bus-model rework above.
  - **PPU residuals — 2 tests** (Rendering Flag Behavior,
    `$2007` read w/ rendering). Distinct from Cascade A.

  **Realistic v1.0.0 trajectory**: if the remaining Cascade A
  geometric residual (VerifySpriteZeroHits step-2; characterisation
  reproducer at `crates/rustynes-ppu/src/ppu.rs` landed in `b629ace`) closes
  without regressing baselines, pass rate would advance
  `79.86% → ~88%`. The v1.0.0 90% gate remains contingent on Cascade A
  full closure + C1 IRQ-timing axis. T-60-002 carries forward to v1.x
  roadmap with the 79.86% baseline.
- [x] **T-60-003a — long-intro budget extensions (CLOSED, 2026-05-17)**:
  Mr. Gimmick + Tiny Toon Adventures 2 flipped from `#[ignore]`'d to
  passing via the `LONG_INTRO_START_3600` input script (idle 3600 →
  START tap → free-run 240, captures at f3661 / f3901). Commit `7fa2c90`.
  Ignored count: `6 → 4`.
- [x] **T-60-003b/c — CLOSED (2026-05-17)**: all 4 remaining stuck
  ROMs flipped via 2 architectural mapper fixes. Root cause: VRC2 /
  VRC4 / VRC6 / MMC4 mapper impls were missing the `$6000-$7FFF`
  WRAM read/write paths. Reads returned 0; writes silently dropped.
  Konami's save-bearing titles stalled in save-validation. Fixes:
  - commit `895e426`: VRC2/VRC4/VRC6 8 KiB `prg_ram` field added +
    read/write paths in `crates/rustynes-mappers/src/vrc2_vrc4.rs` +
    `vrc6.rs`. Flipped Esper Dream 2, Mouryou Senki Madara,
    Ganbare Goemon 2.
  - commit `42f31ff`: MMC4 same pattern in
    `crates/rustynes-mappers/src/mmc2_mmc4.rs`. Flipped Fire Emblem Gaiden.

  **T-60-003 is now FULLY CLOSED — all 6 originally-stuck commercial
  ROMs strict-passing. Commercial-roms count: 60 strict + 0 ignored.**
- [ ] T-60-004 — Multi-OS release-artifact smoke test (T-51-009 carried
  forward from Phase 5 Sprint 1). The `v1.0.0-rc1` tag triggers the
  GitHub Actions release workflow which produces Linux/macOS/Windows
  artifacts. User to smoke-test each on a representative ROM (e.g.,
  nestest.nes) before promoting to `v1.0.0`. PENDING USER VERIFICATION.
- [~] **T-60-005 — `v1.0.0` tag + release notes. SUPERSEDED by
  `v1.0.0-rc2`** (2026-05-22). The rc2 tag captures the
  post-Mesen2-alignment release-candidate state with the four C1
  IRQ-timing residuals + the ~20 non-C1 AccuracyCoin residuals
  explicitly carried forward into the
  `to-dos/phase-6-v1.0.0-final/` sprint backlog. The final `v1.0.0`
  tag is gated on AccuracyCoin ≥ 90% + T-60-001 closure (4 C1
  residuals flipped). Sprint 1 of the v1.0.0-final backlog targets
  the Implied-Dummy + DMC DMA coordinated fix that Session-19 surfaced
  as the highest-leverage entry point. Prior rc1 tag remains as the
  pre-Mesen2-alignment baseline.

---

### Phase 7 — Nesdev Accuracy Hardening (COMPLETE — v1.5.0, 2026-05-24)

**Outcome:** all 4 sprints landed; +25 strict tests, AccuracyCoin held at
90.65% (additive only; the master-clock-axis residuals are explicitly deferred
to v2.0). See `docs/audit/phase-7-assessment-2026-05-24.md` + the per-sprint
audit docs (`docs/audit/phase-7-sprint-{2,3,4}-*.md`).

**Goal:** close the hardware-accuracy and documentation gaps identified by
`ref-docs/nesdev-wiki-technical-report.md` and
`docs/nesdev-hardware-emulation-checklist.md`.

**Exit criterion:** all stock NES/Famicom behaviors in the Nesdev-derived
checklist are implemented, explicitly out of scope, or guarded by tests; missing
Nesdev-indexed test categories are vendored or replaced with licensed fixtures;
PAL/Dendy and remaining AccuracyCoin residuals have automated coverage; platform
expansion scope is documented.

[Phase 7 overview](archive/phase-7-nesdev-accuracy-hardening/overview.md)

Sprints:

- [Sprint 1 — Source and test corpus closure](archive/phase-7-nesdev-accuracy-hardening/sprint-1-source-test-corpus.md)
- [Sprint 2 — CPU, DMA, and internal bus closure](archive/phase-7-nesdev-accuracy-hardening/sprint-2-cpu-dma-internal-bus.md)
- [Sprint 3 — PPU residuals and region variants](archive/phase-7-nesdev-accuracy-hardening/sprint-3-ppu-region-variants.md)
- [Sprint 4 — Mapper, expansion audio, and platform variants](archive/phase-7-nesdev-accuracy-hardening/sprint-4-mappers-expansion-platforms.md)

### Phase 8 — v1.2.0 DMC DMA Scheduler (COMPLETE; broader accuracy residuals deferred)

**Scope reconciliation:** the original v2.0.0 plan framed v1.2.0 as a broad
"accuracy residuals" milestone (sprite-eval + PPU-misc + APU edge cases +
6 ignored commercial ROMs → AccuracyCoin ~97%). What **actually shipped** as
v1.2.0 was a narrower, focused slice: the **DMC DMA get/put scheduler**
landed behind a default-off cargo feature via the parallel-implementation
pattern (ADR 0007). The broader accuracy residuals were **not** done and are
**deferred to v1.6 / v2.0** (several fall out of the v2.0 master-clock
refactor for free); AccuracyCoin remains **90.65%**, not the 97% the original
plan targeted for v1.2.0.

**Exit criterion (MET, as shipped):** v1.2.0 tag landed with
`dmc-get-put-scheduler` parallel-implementation in place (default-off),
equivalence harness shipped, AccuracyCoin DMA cluster matching v1.1.0
baseline at 6/10 under the flag (the remaining 4 — `DMA + $4015 Read`,
`DMC DMA + OAM DMA`, `Explicit/Implicit DMA Abort` — deferred to v2.0
absorption; ADR 0007 option c). Default build bit-identical to v1.1.0; no
regression to the 60-ROM oracle, sacred trio, or B4 invariant.

[Phase 8 overview](archive/phase-8-v1.2.0-accuracy-residuals/overview.md)

Sprints:

- [Sprint 3 — DMC get/put scheduler parallel implementation](archive/phase-8-v1.2.0-accuracy-residuals/sprint-3-dmc-get-put-scheduler.md)
  — Sprint 3.1-3.5 + iter 3 (DMC abort path port) all LANDED. ADR 0007 written.
  v1.2.0 tag landed 2026-05-24.

> **Deferred to v1.6 / v2.0** (tracked here so it isn't lost): (a) DMC get/put
> completion 6/10 → 10/10 + default-on promotion (ADR 0007); (b) the broader
> AccuracyCoin residuals — sprite-eval ($2002 flag timing, Arbitrary Sprite
> zero, Misaligned OAM, OAM Corruption), PPU-misc (Stale BG/Sprite shift regs,
> BG Serial In, Sprites On Scanline 0, $2004/$2007 Stress), APU edge cases
> (Frame Counter IRQ #7, DMC, Reg Activation, Controller Strobing), and the
> 6 ignored commercial ROMs (mapper-026 VRC6b pair shares one bug). Many are
> on the C1 IRQ-sample-point axis and close with the v2.0 master-clock refactor.
> See `docs/STATUS.md` version policy for the full residual list.

### Phase 9 — v1.3.0 WebAssembly Target + v1.3.1/.2/.3 patches (COMPLETE)

**Goal:** Ship a `wasm32-unknown-unknown` build of the frontend that runs in
the browser, per the v2.0.0 release plan. No API break (the chip stack is
already `no_std + alloc`).

**Exit criterion (MET):** v1.3.0 tag landed; the frontend builds for wasm32
in two flavours (`wasm-winit` default + `wasm-canvas` embed); a GitHub Pages
demo is live at `https://doublegate.github.io/RustyNES/`; CI gates a
wasm32 clippy build + a 5 MiB compressed size budget. Workspace tests
preserved (599+6 ignored); AccuracyCoin 90.65%, commercial oracle 60/60,
sacred trio + B4 invariant — all preserved bit-identically.

Sprints (all LANDED): 1.1 scaffolding → 1.2 entry point + browser host →
1.3 canvas-2D MVP → 1.4a audio + save state → 1.4b winit/wgpu/egui
unification → 1.4c audio on the unified path → 2 GitHub Pages deploy + CI
wasm32 gate + size budget. See `docs/audit/v1.3-sprint-*.md`.

**Follow-on patches (COMPLETE):** v1.3.1 (left-edge BG attribute-shifter
palette fix + native present-mode/sleep-spin stutter fix + legacy
`config.toml` migration), v1.3.2 (legacy keycode-name aliases fixing
post-migration dead input + first wasm rAF pacing attempt), v1.3.3 (wasm
`ControlFlow::Wait` + unconditional rAF heartbeat fixing the Pages
stutter/freeze regression + WebGL2 UNORM palette fix + native chunked-sleep
pacing). All frontend-only; native pixel-identical; 616 strict + 6 ignored;
AccuracyCoin 90.65% preserved. See `docs/audit/v1.3.x-*.md`.

### Phase 10 — v1.4.0 TAS Movie Recording/Playback (COMPLETE)

**Goal:** Frame-perfect input recording + playback with save-state branching,
per the v2.0.0 release plan. Exposes the already-met determinism contract
(same seed + ROM + input ⇒ bit-identical framebuffer + audio). No API break.

**Exit criterion (MET):** byte-identical record → replay (framebuffer +
audio FNV-1a + cycle count) proven by integration tests on a committed CC0
ROM; save-state-branch replays deterministically; record/play/branch UI
wired (`F6`/`F7`/`F8`); the `.rnm` movie format is versioned for
forward-compat (ADR 0008, layered on ADR 0003). 636 strict + 8 ignored;
oracle 60/60; AccuracyCoin 90.65%; B4 + sacred trio preserved.

Sprints (LANDED): **4.1** — core movie infra in `crates/rustynes-core/src/movie.rs`
(`MovieRecorder`/`MoviePlayer`, `.rnm` serialize/deserialize, the additive
read-only `Nes::buttons` hook; `run_frame` untouched), ADR 0008, +13 tests.
**4.2** — frontend `crates/rustynes-frontend/src/movie_ui.rs` (record/play/branch
hotkeys, `MovieUi` state machine in the frame loop, native `rfd` `.rnm`
save/load, read-only egui REC/PLAY overlay), +7 tests. Clean-room from
Mesen2 `Core/Shared/Movies/` (structural, GPL-3.0) + FCEUX `.fm2` + the
local TetaNES clone (`ref-proj/tetanes`) + nesdev TAS. wasm `.rnm` file I/O
deferred to a v1.4.x follow-up (UI compiles + no-ops on wasm). See
`docs/adr/0008-tas-movie-format.md`.

### Release engineering (v1.x)

- [→] **CI: `macos-15-intel` runner sunset — August 2027.** GitHub will
  decommission the `macos-15-intel` label after that date (per
  `actions/runner-images#13045`). Plan: migrate to `cargo-zigbuild`
  cross-compile from Linux, or drop `x86_64-apple-darwin` from the
  release binary matrix. Non-blocking forward reminder. The Session-22
  `macos-13` → `macos-15-intel` migration (commit `a9333ba`,
  `.github/workflows/release.yml` +
  `docs/audit/ci-release-workflow-macos-x86_64-2026-05-22.md`) resolved
  the prior deprecation; this entry tracks the next deadline.

---

## Cross-phase dependencies

- Phase 2 Sprint 4 depends on Phase 1 complete (CPU core).
- Phase 3 depends on Phase 1 (CPU) and Phase 2 Sprint 4 (scheduler) complete; Sprint 2 of Phase 3 depends on Phase 2 Sprint 4 (DMA).
- Phase 4 depends on Phase 2 (PPU) for mapper-PPU integration; Phase 3 (APU) for audio-extension mappers.
- Phase 5 depends on all previous phases.
- Phase 7 depends on the Phase 6 closeout decision for which v1.0 residuals
  carry forward. It also depends on the Nesdev checklist staying current with
  upstream source pages and local `docs/STATUS.md` pass counts.

## Open questions blocking planning

None block Phase 1. Open questions in the docs (esp. `architecture.md`, `mappers.md`) will be revisited at the start of the phase that needs them resolved.

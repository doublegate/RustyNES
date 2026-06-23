# RustyNES Monetization — Documentation Index

> **Harvested into the RustyNES repo, 2026-06-23 — read this banner first.** These docs
> were folded in from the standalone `rustynes-monetization/` scaffold. Two things are
> now true in-repo:
>
> 1. **Locations.** The Rust policy core lives at **`crates/rustynes-monetization/`** (a
>    real workspace crate — `AdPolicy`, 13 tests, UniFFI 0.31 to match `rustynes-mobile`).
>    The Android/iOS app shells are staged, customized to the `com.doublegate.rustynes`
>    namespace, at **`crates/rustynes-monetization/shells/{android,ios}/`** (reference glue
>    to wire into the live app at launch — see that folder's `README.md`). Where these docs
>    say `rustynes-monetization/core/…`, `android/…`, `ios/…`, read those in-repo locations.
> 2. **Launch timing is superseded by the v2.1.0 replan.** These docs were written when the
>    Play debut was **v1.8.8** and the `PLAY_BUILD` flavor was wired at v1.8.2. Per the
>    maintainer's 2026-06-23 mobile-launch replan, **both app-store launches are deferred to
>    v2.1.0** (Android finalized v2.0.1–v2.0.4, iOS v2.0.5–v2.0.8, both verified v2.0.9, joint
>    launch v2.1.0). So everywhere these docs say "v1.8.8 Play debut" / "flip the freemium
>    layer on at v1.8.8", read **"v2.1.0 joint launch."** The freemium layer still ships
>    **default-off behind `PLAY_BUILD`** and rides along dormant in the v1.8.x sideload builds
>    until that launch. See [`../../to-dos/plans/v2.0.x-mobile-finalization-plan.md`](../../to-dos/plans/v2.0.x-mobile-finalization-plan.md).
>
> The **monetization model** (ad-supported freemium: AppLovin MAX + RevenueCat, a one-time
> **"Full Version / Remove Ads" ($3.99)** unlock) is the chosen path — a deliberate override
> of the v1.8.0 plan's ad-free $2.99 default, recorded in `to-dos/plans/v1.8.0-android-plan.md`.

This `docs/` set is the single source of truth for shipping RustyNES as a freemium app
(AppLovin MAX ads on the free tier, removed by a RevenueCat **`premium`** entitlement) on
**Google Play** and the **Apple App Store**. Humans and Claude Code both read from here.

## Tree

```
rustynes-monetization/
├── CLAUDE.md                        # repo-root, read-first for Claude Code → points here
├── README.md                        # one-line signpost to this index
├── docs/
│   ├── README.md                    # ← you are here (index + reading order)
│   ├── platform-setup-runbook.md    # HUMAN: create accounts, dashboards, keys, products
│   ├── implementation-brief.md      # CLAUDE CODE: how to implement + compliance blockers
│   ├── pre-implementation-addendum.md  # CLAUDE CODE: ad strategy, perf, enhancements (read before building)
│   ├── recommendations.md           # CLAUDE CODE: considerations beyond baseline (design holes, robustness, polish)
│   ├── build-and-bindings.md        # build commands (cargo-ndk/cargo-swift) + FFI API table
│   └── rustynes-integration.md      # CLAUDE CODE: integration onto the real RustyNES repo (read first)
├── core/                            # shared Rust crate (AdPolicy + UniFFI scaffolding)
├── android/                         # Kotlin shell (RevenueCat + AppLovin MAX)
└── ios/                             # Swift shell (RevenueCat + AppLovin MAX)
```

## What each document is

| Doc | Audience | Purpose |
|---|---|---|
| `platform-setup-runbook.md` | **You (human)** | Step-by-step account/dashboard setup: store developer accounts, the Individual-vs-Organization decision aid (§1a), RevenueCat, AppLovin MAX, mediation networks, tax/banking, costs, and timeline. |
| `implementation-brief.md` | **Claude Code / engineer** | The *why* and the *compliance*: decisions made, provider links, store-submission blockers (16 KB `.so`, ATT, privacy manifest), testing strategy, enhancements, open decisions. |
| `pre-implementation-addendum.md` | **Claude Code / engineer** | The *resolve-before-building* layer: ad-placement strategy for a vintage-console emulator, ad-content scoping, emulator-specific performance items (thread isolation, pause/resume, preload), product enhancements, a recommended baseline config, plus remote-config tuning (§9) and the analytics funnel (§10). |
| `recommendations.md` | **Claude Code / engineer** | Considerations & suggestions *beyond* the baseline: free-tier design holes to close (session-reset, offline trap, no-save cliff), engineering robustness (graceful SDK failure, tick cadence, test-mode ads, cross-platform entitlement), and store/product polish. Points to where compliance and tuning items live in the other docs. |
| `build-and-bindings.md` | **Claude Code / engineer** | The *how*: cargo-ndk / cargo-swift / uniffi-bindgen commands, key/entitlement config, and the verbatim generated FFI API table. (Formerly the project README.) |
| `rustynes-integration.md` | **Claude Code / engineer** | **Read first.** Maps this layer onto the real RustyNES repo: the **Compose + wgpu-`SurfaceView`** hybrid app, the shared `rustynes-mobile` bridge + `rustynes-android` glue, the determinism boundary, the `cargo-ndk`/AAB toolchain, and the **v1.8.8** Play debut. The maintainer has chosen the **ad-supported** RevenueCat + AppLovin model (this repo) as the **primary** path — a deliberate override of the v1.8.0 plan's ad-free default — with a one-time **"Full Version / Remove Ads" ($3.99)** unlock. |
| `../CLAUDE.md` | **Claude Code** | Read-first index + invariants + build order. Routes to the docs above. |

## Reading order

**If you're setting up the business side (human):**
1. `platform-setup-runbook.md` — start with §1 decisions and the §1a decision aid, then
   work the critical-path sequence (§2). Begin the slow verifications (D-U-N-S, LLC, Apple
   org review) on day one.

**If you're implementing (Claude Code / engineer):**
1. `rustynes-integration.md` — **read first.** Maps this layer onto the real RustyNES repo (the
   Compose + wgpu-`SurfaceView` hybrid app, the `rustynes-mobile` bridge, the determinism
   boundary, the toolchain, the v1.8.8 Play debut). The **ad-supported** RevenueCat + AppLovin
   model (this repo) is the chosen **primary** path — a deliberate override of the v1.8.0 plan's
   ad-free default — at a one-time **"Full Version / Remove Ads" ($3.99)** unlock.
2. `../CLAUDE.md` — invariants and build order.
3. `implementation-brief.md` — decisions, compliance blockers, enhancements.
4. `pre-implementation-addendum.md` — ad strategy, content scoping, performance, the
   recommended baseline ad config to implement first, remote-config tuning (§9), and the
   analytics funnel (§10).
5. `recommendations.md` — considerations beyond the baseline (free-tier design holes,
   robustness, polish); skim before locking the free-tier UX.
6. `build-and-bindings.md` — exact commands and the FFI surface.
7. Skim `platform-setup-runbook.md` §5–§9 for the entitlement id, product ids, the
   key/credential names the code consumes, the consent-flow setup, and how to unlock the app
   for the closed-test cohort (§5a).

## The contract that ties it together

- One RevenueCat entitlement, identifier **`premium`**, is the only premium signal. It flows
  into the Rust core via `AdPolicy.set_premium(bool)`; ads and paid features derive from it.
- All ad cadence and paywall logic live in `crates/rustynes-monetization/src/monetization.rs` and are shared to both
  platforms via UniFFI — never duplicated in Kotlin/Swift.
- **Free tier is time-gated to 8 min/session** (no save states, no battery saves); a completed
  rewarded ad grants **+11 min**; the Full Version IAP removes the timer and unlocks saves. The
  play-time logic lives in the core (addendum §2c/§2f).
- Bundle/package id, the `premium` entitlement, product ids, and the SDK keys must match
  across the stores, RevenueCat, AppLovin, and the build config. The runbook's §8 table maps
  every key to where it goes.

## Status

- ✅ Core compiles; **all 13 Rust unit tests pass** (interstitial pacing + the free-tier
  play-time budget and 2-grant rewarded cap); Kotlin + Swift bindings are generated on demand
  from the crate (see `build-and-bindings.md` — the committed `core/generated/` snapshot was
  dropped as a regenerable build artifact).
- ✅ Free-tier play-time gate (8-min budget, +11 min per completed rewarded ad, 2-grant /
  +22-min cap → 30 min max) implemented in `crates/rustynes-monetization/src/monetization.rs`; `PremiumFeature` gates
  **six** features — `SaveStates` + `SaveOnExitResume` + `BatterySaves` + `FastForward` +
  `Shaders` + `Cheats` (the free tier keeps full accuracy, video, audio, input, pause, and
  in-session **rewind**; fast-forward / shaders / cheats are now premium).
- ⬜ Shell wiring still to do: the countdown UI and the run-out prompt (the rewarded-ad gate
  `RewardedGate.{kt,swift}` is implemented) — see addendum §8.
- ⬜ Open decisions (see `implementation-brief.md` §10): one-time vs subscription, free-tier
  tuning (base 8 min / reward 11 min; cap = 2 grants/session, +22 min), and the
  Individual-vs-Organization account choice (runbook §1a). The free-tier *model* (8-min
  timer, +11 min per rewarded ad, no save/battery saves) and the **`PremiumFeature` set
  (the six above)** are decided — see addendum §2c/§2f.

*Verify SDK versions, fees, and store-policy text at implementation/enrollment time — they move.*

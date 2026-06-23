# CLAUDE.md — RustyNES Mobile Monetization

Working-memory entry point for Claude Code. Read this first, then the two source docs
below before writing code.

## Read these first
0. **`docs/rustynes-integration.md`** — **read before anything else.** Maps this layer onto the
   real RustyNES repo (the Compose + wgpu-`SurfaceView` hybrid app, the shared `rustynes-mobile`
   bridge + `rustynes-android` glue, the determinism boundary, the NDK/`cargo-ndk`/AAB toolchain,
   and the **v1.8.8** Play debut). Note: the maintainer has chosen the **ad-supported** RevenueCat
   + AppLovin model (this repo) as the **primary** path — a deliberate override of the v1.8.0
   plan's ad-free default — with a one-time **"Full Version / Remove Ads" ($3.99)** unlock.
1. **`docs/implementation-brief.md`** — decisions, provider links, store-submission
   blockers, testing, enhancements, open decisions. The *why* and the *compliance*.
2. **`docs/build-and-bindings.md`** — exact build commands (cargo-ndk / cargo-swift /
   uniffi-bindgen) and the generated FFI API table. The *how*.
3. **`docs/pre-implementation-addendum.md`** — ad-placement strategy for an emulator,
   ad-content scoping, emulator-specific performance items, and the recommended baseline ad
   config. The *resolve-before-building* layer; read before wiring ad behavior.
4. **`docs/recommendations.md`** — considerations & suggestions beyond the baseline:
   free-tier design holes to close, engineering robustness, and store/product polish.
5. **`docs/platform-setup-runbook.md`** — the human account/dashboard setup (stores,
   RevenueCat, AppLovin, mediation networks). Not your job to execute, but it defines the
   keys, product ids, and the `premium` entitlement the code depends on.
6. **`docs/README.md`** — index and reading order for the whole doc set.

If any path differs in this repo, locate it before proceeding; do not reconstruct content
from memory.

## What this is
Freemium for the RustyNES mobile ports: **AppLovin MAX** ads on the free tier, removed by a
**RevenueCat** `premium` entitlement (a one-time **"Full Version / Remove Ads"** purchase,
**$3.99**). The free tier pairs interstitials at natural breaks with rewarded ads that extend an
8-minute play session (+2 min each, capped at 11 grants → 30 min); premium removes ads and the
timer and unlocks the persistence features. All monetization *logic* lives in one Rust object
(`AdPolicy`) shared to Android (Kotlin) and iOS (Swift) via **UniFFI**. The mobile app itself is a
Compose + wgpu-`SurfaceView` hybrid over the shared `rustynes-mobile` bridge (see the integration
doc). Ship **Android first (v1.8.0; Play debut v1.8.8), iOS at v1.9.0**.

## Build order
1. Fold `rustynes-monetization` (or its `monetization` module) into the existing Cargo workspace —
   **one FFI crate, one `uniffi::setup_scaffolding!()`**. `cargo test` must stay green.
2. Android native: `cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -t x86 -o android/src/main/jniLibs build --release`.
3. Android bindings: `cargo run --features=cli --bin uniffi-bindgen -- generate --library <…librustynes_monetization.so> --language kotlin --out-dir android/src/main/java`.
4. Wire Android shells (`RustyNesApp.kt`, `Billing.kt`, `AdGate.kt`); inject keys via `gradle.properties`.
5. Configure RevenueCat (`premium` entitlement + offering) and AppLovin (ad units + the recommended networks below) dashboards.
6. Android compliance (§6–§7 of the brief); ship to internal testing.
7. iOS: `cargo swift package --platforms ios --name RustyNesMonetization --release`; wire Swift shells; iOS compliance; TestFlight → App Store.

## Invariants — do not break
- **Single source of truth:** premium status flows into the core only through
  `AdPolicy.set_premium(bool)`, fed from RevenueCat `entitlements["premium"].isActive` — no
  second premium flag. The one sanctioned extra caller is a **debug-only tester override**
  (`Billing.kt`/`Billing.swift`, OR-ed into the same `set_premium` path) for local QA; it is
  compiled-inert in release, including the closed-test track. Closed-test testers are unlocked
  via a RevenueCat promotional grant or Play license testing — see brief §9a / runbook §5a.
- **All ad/feature logic stays in the Rust core.** Shells only plumb SDKs. Adding cadence or
  paywall logic in Kotlin/Swift defeats the cross-platform guarantee — put it in `monetization.rs`.
- **Inject `now_ms`** (monotonic ms) from the host; never read a clock inside the core.
- **One app, one IAP** — no separate "pro" binary.
- Call `adGate.maybeShowInterstitial()` only at natural breaks (ROM load, menu, save-state),
  never mid-frame. Call `adGate.preload()` once after SDK init.

## Free-tier model (core product rule)
- Free tier is **time-gated to 8 min per game session**, with **no save states** and **no
  battery-backed (SRAM) saves**. Premium removes the timer and unlocks both.
- Each **completed** rewarded ad grants **+2 min** of play. Grant **only** on the reward
  callback (`OnUserRewarded` / `didRewardUser`), never on load/show/dismiss. **Capped at 11
  grants/session** (max +22 min → 30 min total); the cap resets each game session.
- This logic is **implemented and tested** in the Rust core (`start_play`, `add_active_time`,
  `grant_rewarded_time` → bool w/ cap, `can_offer_rewarded`, `reward_grants_remaining`,
  `is_play_allowed`, `play_time_remaining_ms`; `base_play_ms`/`reward_play_ms`/
  `max_reward_grants_per_session` in `AdConfig`). 13 unit tests pass; bindings regenerated to
  `core/generated/`. Full host flow in `docs/pre-implementation-addendum.md` §2c/§2f.
- Drive `add_active_time(...)` from **unpaused** emulation only (stop during ads, the run-out
  prompt, and background) so paused time never burns the budget.

## Release blockers for native (Rust) apps — verify, don't skip
- **Android 16 KB page size:** the Rust `.so` must be 16 KB-aligned (NDK r28+, AGP 8.5.1+,
  linker `-Wl,-z,max-page-size=16384`). Verify `llvm-objdump -p …so | grep LOAD` shows `2**14`.
  Non-compliant bundles are blocked by Play.
- **iOS ATT:** add `NSUserTrackingUsageDescription`; request consent before MAX init; **state
  in App Store Connect review notes that you use ATT** or risk rejection.
- **iOS:** SKAdNetwork IDs (AppLovin Info.plist generator) + app `PrivacyInfo.xcprivacy` +
  nutrition labels; third-party SDK privacy manifests **and signatures** are required, so ship
  current AppLovin/adapter/RevenueCat versions (review runs binary checks; missing/unsigned → reject).
- **EEA/UK/Switzerland consent:** a Google-certified CMP integrated with the IAB TCF is required
  to serve personalized ads (EEA/UK since 16 Jan 2024, CH since 31 Jul 2024). Use MAX's Terms &
  Privacy (Google UMP) flow; consent must complete **before** MAX init. Separate from ATT.
- **Both stores:** rate Teen/12+, not child-directed (AppLovin forbids child-directed apps).
  Ship the emulator only — no bundled copyrighted ROMs.

## Mediation networks (recommended)
Run AppLovin MAX as the auction layer and enable these demand sources in the MAX dashboard:
- **Google bidding (AdMob / Ad Manager)** — usually the top demand; requires the AdMob app id
  in the Android manifest (`com.google.android.gms.ads.APPLICATION_ID`) and iOS
  `GADApplicationIdentifier`.
- **Meta Audience Network**
- **Unity Ads**
- **Pangle (ByteDance / TikTok)**

For each: add its Gradle/SPM mediation adapter, enable it in MAX, and (iOS) add its
SKAdNetwork IDs via AppLovin's Info.plist generator. Expand later with Liftoff Monetize
(Vungle), Mintegral, InMobi, or ironSource as you tune the waterfall.

## Decisions still open (ask before building purchase UI)
- One-time "Full Version / Remove Ads" (~$3.99, recommended) vs subscription.
- Exact tuning of the free-tier model: base 8 min / reward 2 min values and the 11-grant cap
  (max +22 min). (Model itself is decided above.)
- `PremiumFeature` set beyond `SaveStates` + `BatterySaves` (e.g. fast-forward, shaders, cheats).
- Initial mediation networks (defaults below).

## Conventions
- Comment style matches each language; Rust files carry full preambles.
- Keep desktop RustyNES builds unaffected — gate mobile-only deps behind a feature if needed.
- Verify SDK versions and store-policy text at implementation time; deadlines and APIs move.

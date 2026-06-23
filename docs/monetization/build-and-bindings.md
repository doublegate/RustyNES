# RustyNES — Cross-Platform Monetization Skeleton

A shared **Rust core** that owns all monetization *logic*, with thin **Android (Kotlin)**
and **iOS (Swift)** shells that own only the platform SDK *plumbing* — **RevenueCat**
for entitlements and **AppLovin MAX** for ads. Write the policy once; ship it to both
stores.

```
core/ (Rust)  ──UniFFI──▶  Kotlin bindings ──▶ android/  (RevenueCat + AppLovin MAX)
   AdPolicy               Swift  bindings ──▶ ios/      (RevenueCat + AppLovin MAX)
```

The core is the **single source of truth**. Both shells convert RevenueCat's
`CustomerInfo` into one boolean (`setPremium`) and ask the core when to show an ad
(`shouldShowInterstitial`). Ad cadence, the launch-grace window, and the paywalled
feature set live only in `crates/rustynes-monetization/src/monetization.rs`, so the two platforms cannot drift.
When a purchase completes, `setPremium(true)` makes every gate return `false`
instantly — ads stop with no app restart.

---

## Layout

```
core/                         shared Rust crate (rustynes-monetization)
  Cargo.toml                  cdylib + staticlib + lib; uniffi-bindgen bin
  uniffi.toml                 fixes Kotlin package (com.doublegate.rustynes.monetization.ffi) + Swift module (RustyNesMonetization)
  uniffi-bindgen.rs           in-crate binding generator (stable-Rust workaround)
  src/lib.rs                  crate root; uniffi::setup_scaffolding!()
  src/monetization.rs         AdPolicy / AdConfig / PremiumFeature  +  unit tests
android/
  build.gradle.kts            SDK deps, BuildConfig fields, jniLibs source set (excerpt)
  AndroidManifest.xml         permissions + AdMob app id meta-data (excerpt)
  src/main/java/app/rustynes/
    RustyNesApp.kt            Application: init MAX + RevenueCat, build core
    Billing.kt               RevenueCat wrapper → core.setPremium
    AdGate.kt                MAX interstitial gate ← core.shouldShowInterstitial
ios/
  Package.swift               SwiftPM: RustyNesMonetization + RevenueCat + AppLovinSDK
  Sources/RustyNesApp/
    RustyNesApp.swift        @main App + Monetization coordinator
    Billing.swift            RevenueCat wrapper (PurchasesDelegate)
    AdGate.swift             MAX interstitial gate (MAAdDelegate)
```

---

## Build the Rust core & generate bindings

Prerequisites: a stable Rust toolchain.

```bash
cd core
cargo test --release        # runs the pacing/entitlement unit tests
```

### Android (.so + Kotlin bindings)

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi \
                  x86_64-linux-android i686-linux-android
cargo install cargo-ndk

cd core
# 1) Build the per-ABI native libraries straight into the app's jniLibs:
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -t x86 \
  -o ../android/src/main/jniLibs build --release

# 2) Generate the Kotlin bindings (library mode reads types from the .so):
cargo run --features=cli --bin uniffi-bindgen -- generate \
  --library target/aarch64-linux-android/release/librustynes_monetization.so \
  --language kotlin --out-dir ../android/src/main/java
# → writes ../android/src/main/java/app/rustynes/ffi/rustynes_monetization.kt
```

The generated Kotlin loads the `.so` via JNA (declared in `build.gradle.kts`).

### iOS (xcframework + Swift bindings)

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
cargo install cargo-swift

cd core
# Produces ../ios/RustyNesMonetization, a Swift package containing RustyNesMonetization.swift plus
# the librustynes_monetization xcframework as a binaryTarget. Name MUST match uniffi.toml.
cargo swift package --platforms ios --name RustyNesMonetization --release
```

(Manual alternative: `cargo build --release` per iOS triple, then
`cargo run --features=cli --bin uniffi-bindgen -- generate --library <.a> --language swift`,
then `xcodebuild -create-xcframework`.)

---

## Configure the SDKs

| What | Where to get it | Where it goes |
|------|-----------------|---------------|
| RevenueCat API key (Google) | RevenueCat dashboard → Project → API keys | `gradle.properties` → `revenueCatGoogleKey` |
| RevenueCat API key (Apple)  | same, Apple key | iOS Info.plist → `REVENUECAT_API_KEY` |
| AppLovin SDK key | AppLovin dashboard → Account → General → Keys | Android `applovinSdkKey`; iOS `APPLOVIN_SDK_KEY` |
| MAX interstitial ad-unit id | AppLovin dashboard → MAX → Ad Units | `maxInterstitialAdUnitId` / `MAX_INTERSTITIAL_AD_UNIT_ID` |
| Entitlement id `"premium"` | RevenueCat → Entitlements | already referenced in code |

In RevenueCat, create one **entitlement** named `premium`, attach it to your
non-consumable "remove ads" / full-version product (or subscription), and add that
product to the **current offering**. The shells purchase
`offerings.current.availablePackages.first`.

For the AppLovin Google/AdMob adapter, keep the `com.google.android.gms.ads.APPLICATION_ID`
meta-data in `AndroidManifest.xml` and the equivalent `GADApplicationIdentifier` in the
iOS Info.plist.

---

## Generated API the shells call (verbatim)

| Rust | Kotlin | Swift |
|------|--------|-------|
| `AdPolicy::new(cfg, now_ms)` | `AdPolicy(config, nowMs: ULong)` | `AdPolicy(config:nowMs: UInt64)` |
| `default_ad_config()` | `defaultAdConfig()` | `defaultAdConfig()` |
| `set_premium(bool)` | `setPremium(premium)` | `setPremium(premium:)` |
| `is_premium()` | `isPremium()` | `isPremium()` |
| `should_show_interstitial(now_ms)` | `shouldShowInterstitial(nowMs)` | `shouldShowInterstitial(nowMs:)` |
| `notify_interstitial_shown(now_ms)` | `notifyInterstitialShown(nowMs)` | `notifyInterstitialShown(nowMs:)` |
| `feature_enabled(f)` | `featureEnabled(feature)` | `featureEnabled(feature:)` |
| `start_play()` | `startPlay()` | `startPlay()` |
| `add_active_time(delta_ms)` | `addActiveTime(deltaMs: ULong)` | `addActiveTime(deltaMs: UInt64)` |
| `can_offer_rewarded() -> bool` | `canOfferRewarded(): Boolean` | `canOfferRewarded() -> Bool` |
| `reward_grants_remaining() -> u32` | `rewardGrantsRemaining(): UInt` | `rewardGrantsRemaining() -> UInt32` |
| `grant_rewarded_time() -> bool` | `grantRewardedTime(): Boolean` | `grantRewardedTime() -> Bool` |
| `is_play_allowed() -> bool` | `isPlayAllowed(): Boolean` | `isPlayAllowed() -> Bool` |
| `play_time_remaining_ms() -> Option<u64>` | `playTimeRemainingMs(): ULong?` | `playTimeRemainingMs() -> UInt64?` |
| `AdConfig { base_play_ms, reward_play_ms, max_reward_grants_per_session, … }` | `AdConfig(…, basePlayMs: ULong, rewardPlayMs: ULong, maxRewardGrantsPerSession: UInt)` | `AdConfig(…, basePlayMs: UInt64, rewardPlayMs: UInt64, maxRewardGrantsPerSession: UInt32)` |
| `PremiumFeature::{SaveStates, SaveOnExitResume, BatterySaves, FastForward, Shaders, Cheats}` | `PremiumFeature.{SAVE_STATES, SAVE_ON_EXIT_RESUME, BATTERY_SAVES, FAST_FORWARD, SHADERS, CHEATS}` | `.saveStates / … / .fastForward / .shaders / .cheats` |

### Build-out additions (2026-06-23)

The core expansion added these to the surface above:

| Rust | Kotlin | Swift |
|------|--------|-------|
| `begin_session(session_index, now_ms)` | `beginSession(sessionIndex: UInt, nowMs: ULong)` | `beginSession(sessionIndex: UInt32, nowMs: UInt64)` |
| `can_grant_offline_grace() -> bool` | `canGrantOfflineGrace(): Boolean` | `canGrantOfflineGrace() -> Bool` |
| `grant_offline_grace() -> bool` | `grantOfflineGrace(): Boolean` | `grantOfflineGrace() -> Bool` |
| `export_progress() -> PlayProgress` | `exportProgress(): PlayProgress` | `exportProgress() -> PlayProgress` |
| `restore_progress(p)` | `restoreProgress(progress: PlayProgress)` | `restoreProgress(progress: PlayProgress)` |
| `clamp_ad_config(cfg) -> AdConfig` | `clampAdConfig(cfg: AdConfig)` | `clampAdConfig(cfg: AdConfig)` |
| `AdConfig { …, first_session_play_ms, suppress_first_session, offline_grace_ms }` | `…, firstSessionPlayMs: ULong, suppressFirstSession: Boolean, offlineGraceMs: ULong` | `…, firstSessionPlayMs: UInt64, suppressFirstSession: Bool, offlineGraceMs: UInt64` |
| `PlayProgress { budget_ms, consumed_ms, reward_grants_this_session, offline_grace_used }` | `PlayProgress(budgetMs, consumedMs, rewardGrantsThisSession, offlineGraceUsed)` | `PlayProgress(budgetMs:consumedMs:rewardGrantsThisSession:offlineGraceUsed:)` |

- **`PremiumFeature` now has six variants.** Per the 2026-06-23 "expand the premium set"
  decision, **FastForward / Shaders / Cheats are now premium** — this **overrides** the
  earlier doc stance (here and in `rustynes-integration.md` §4 / `pre-implementation-addendum.md`)
  that fast-forward stayed free. The free tier keeps full accuracy, video, audio, input,
  pause, and in-session rewind.
- **Free-tier budget:** **8-min** regular session, **30-min** generous first session
  (`first_session_play_ms`, applied when the session index is 1; interstitials suppressed in
  session #1 via `suppress_first_session`), +11 min per rewarded ad (cap 2 → 30 min on a
  regular session). Host calls `begin_session(persisted_index, now)` at launch.
- **Offline grace:** at run-out with no rewarded fill, `grant_offline_grace()` gives a
  one-time +2 min so an offline user degrades gracefully (recs §1b).
- **Kill-relaunch:** persist `export_progress()` and `restore_progress()` to keep the
  timer/cap across a process death (recs §1a/§1f).
- **Remote config:** fetch values, overlay on `default_ad_config()`, pass through
  `clamp_ad_config()` before building `AdPolicy` so a bad push can't brick the gate.

`now_ms` is monotonic milliseconds: `SystemClock.elapsedRealtime()` on Android,
`DispatchTime.now().uptimeNanoseconds / 1_000_000` on iOS (the shells use
`mach_continuous_time()` so iOS counts deep-sleep like Android).

The free-tier play-time gate (`start_play` … `play_time_remaining_ms`) implements the
budget, +11-min-per-rewarded-ad extension, and 2-grant per-session cap. Grant time
**only** from the rewarded reward callback (`OnUserRewarded` / `didRewardUser`) — see
`RewardedGate.{kt,swift}` in `shells/`. See `pre-implementation-addendum.md` §2c/§2f for
the host flow.

The UniFFI Kotlin/Swift bindings are **generated on demand** from the crate with the
library-mode commands above (the Kotlin package is `com.doublegate.rustynes.monetization.ffi`,
the Swift module `RustyNesMonetization`). Regenerate whenever the core changes; the committed
`core/generated/` snapshot from the standalone scaffold was dropped as a build artifact.

---

## Release timing note

A brand-new Play Store submission must use **Google Play Billing Library 8+** by
**Aug 31, 2026** (v9 is current). RevenueCat bundles a compliant billing library and
updates it for you, so going through RevenueCat keeps you clear of that deadline
without tracking it yourself.

---

## Where to call the gate

Call `adGate.maybeShowInterstitial()` only at natural breaks — ROM loaded, returned to
the menu, save-state taken — never mid-frame. Call `adGate.preload()` once after SDK
init. Gate premium features with `core.featureEnabled(...)`. Everything else (cadence,
grace, premium suppression) is already handled inside the core.

Premium is set only via `setPremium`, fed from RevenueCat. A RevenueCat promotional grant or a
Play license-tester test purchase both surface as `entitlements["premium"].isActive` and flow
through that same call — so unlocking the app for closed-test testers needs no app changes
(runbook §5a / brief §9a). `Billing` also carries a debug-only `TESTER_UNLOCK` override that
OR-s into `setPremium`, inert in release.

Tune cadence in `AdConfig` (`min_interval_ms`, `launch_grace_ms`); the defaults are a
4-minute interval and a 30-second launch grace, deliberately conservative for an
emulator's long play sessions.

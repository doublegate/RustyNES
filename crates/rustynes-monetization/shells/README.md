# `rustynes-monetization` platform shells (staged reference glue)

These are the **Android (Kotlin)** and **iOS (Swift)** app shells that consume the
`rustynes-monetization` Rust core (`AdPolicy`) through its UniFFI bindings. They are the
counterpart implementations that keep ad cadence + the play-time gate + the premium-feature
set **identical across both platforms** (all policy lives in the Rust core; the shells only
plumb the SDKs).

## Status: staged, not yet wired into the live build

They are **reference glue**, deliberately kept out of the compiled app for now:

- They import the proprietary **AppLovin MAX** (`com.applovin.*` / `AppLovinSDK`) and
  **RevenueCat** (`com.revenuecat.purchases.*` / `RevenueCat`) SDKs, which are **not** added
  to the live Gradle / SPM builds yet.
- They reference `BuildConfig` fields (`APPLOVIN_SDK_KEY`, `REVENUECAT_API_KEY`,
  `MAX_INTERSTITIAL_AD_UNIT_ID`, `TESTER_UNLOCK`) and an Info.plist `RUSTYNES_TESTER_UNLOCK`
  that are added at wiring time.
- Per the **2026-06-23 mobile-launch replan**, both app-store launches (and therefore the
  freemium / ad layer) are **deferred to v2.1.0** — see
  [`../../../to-dos/plans/v2.0.x-mobile-finalization-plan.md`](../../../to-dos/plans/v2.0.x-mobile-finalization-plan.md).
  The freemium layer ships **default-off behind `PLAY_BUILD`** and stays dormant in the
  v1.8.x sideload builds until then.

So nothing here compiles into the current sideload app; they wait for the v2.1.0 wiring.

## What's here

| File | Role |
|---|---|
| `android/RustyNesApp.kt` | `Application` entry point: build `AdPolicy`, init AppLovin MAX, configure RevenueCat |
| `android/Billing.kt` | RevenueCat wrapper → `AdPolicy.setPremium` (the single premium source of truth) + a debug tester override |
| `android/AdGate.kt` | AppLovin MAX interstitial lifecycle; defers every cadence decision to the core |
| `android/build.gradle.kts` | Reference Gradle dependency + `BuildConfig` field snippets to merge into `android/app/build.gradle.kts` |
| `android/AndroidManifest.xml` | Reference manifest entries (AdMob app id, network permissions) |
| `ios/RustyNesApp.swift` | iOS coordinator mirroring the Android entry point |
| `ios/Billing.swift` | RevenueCat wrapper (iOS) |
| `ios/AdGate.swift` | AppLovin MAX interstitial gate (iOS) |
| `ios/Package.swift` | Reference SPM manifest for the RevenueCat + AppLovin dependencies |

The Android shells are customized to the **`com.doublegate.rustynes.monetization`** package
and import the UniFFI-generated bindings from `com.doublegate.rustynes.monetization.ffi`
(matching `../uniffi.toml`). `BuildConfig` resolves from the app module
(`com.doublegate.rustynes.BuildConfig`).

## Wiring checklist (v2.1.0 launch)

1. Add the SDK dependencies (AppLovin MAX 13+, RevenueCat / `purchases` 8+) and the
   `BuildConfig` fields / `buildConfigField`s from `android/build.gradle.kts` into
   `android/app/build.gradle.kts` behind the **`play`** product flavor / `PLAY_BUILD` flag.
2. Copy the Kotlin shells into `android/app/src/<playFlavor>/java/com/doublegate/rustynes/monetization/`
   and adapt `RustyNesApp` wiring to the real `Application` / `MainActivity` (these shells are a
   self-contained skeleton, not a drop-in over the existing Compose app).
3. Regenerate the UniFFI Kotlin bindings from the crate (see
   [`../../../docs/monetization/build-and-bindings.md`](../../../docs/monetization/build-and-bindings.md)).
4. Gate all six `PremiumFeature`s on `feature_enabled(...)` — `SaveStates` /
   `SaveOnExitResume` / `BatterySaves` / `FastForward` / `Shaders` / `Cheats`; keep
   in-session rewind free (per `docs/monetization/rustynes-integration.md` §4).
5. iOS: mirror with the SPM manifest + the Swift shells once the iOS app exists (v2.0.5+).
6. Complete the store / dashboard setup in
   [`../../../docs/monetization/platform-setup-runbook.md`](../../../docs/monetization/platform-setup-runbook.md)
   and the compliance items in `docs/monetization/implementation-brief.md`.

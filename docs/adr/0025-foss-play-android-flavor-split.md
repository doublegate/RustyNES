# 25. A `foss` / `play` Android product-flavor split for F-Droid + Google Play

Date: 2026-06-23

## Status

Accepted (planned for **v2.1.0** — the joint mobile store launch). The decision is
locked now; the implementation is a v2.1.0 deliverable (see
`to-dos/plans/v2.0.x-mobile-finalization-plan.md`). Until then the Android app stays
single-flavor and the proprietary SDKs ride along dormant behind their `BuildConfig`
flags, as today.

## Context

The Android app (`android/app`) currently links **six groups of Google-Play-specific,
proprietary SDKs unconditionally** and gates each with a default-off `BuildConfig` flag
(the established house pattern):

- Play Billing (`billing-ktx`) — the freemium unlock (`LicenseManager`, Workstream M).
- Cast framework (`play-services-cast-framework`) — `CHROMECAST_ENABLED`.
- Play Games v2 (`play-services-games-v2`) — `PGS_ENABLED` (cloud saves + achievements).
- Play Integrity (`integrity`) — `PLAY_INTEGRITY_ENABLED`.
- In-app update / review (`app-update`, `review`) — no-op off-Play.

The 2026-06-23 monetization decision adds two more: **AppLovin MAX** (ads) and
**RevenueCat** (entitlements), the ad-supported `$3.99` "Full Version / Remove Ads"
model (it overrides the prior `$2.99`-no-ads Play-Billing model; see
`to-dos/plans/v1.8.0-android-plan.md`).

Every one of these is **Google-Play-specific**: they need Google Play Services and/or
the Play Store to function and are dead weight on any other channel. The project also
commits to a **"guaranteed sideload / F-Droid / GitHub-Releases channel"** that is
full-featured and ad-free (monetization docs §7 / §10).

Two channel facts force the architecture:

1. **F-Droid requires a FOSS build with no proprietary/Google dependencies.** An APK
   linking Play Services / Billing / Play Games would be **rejected** (or flagged
   `NonFreeDep`). F-Droid additionally **does not list apps with ads or tracking** — so
   the AppLovin build can never go on F-Droid regardless.
2. A `BuildConfig` flag gates *code execution*, not *packaging*: a flag-gated dependency
   is still in the APK and **its manifest still merges** (e.g. AppLovin/AdMob declare the
   `AD_ID` permission), so a flag-only approach cannot produce a genuinely Google-free,
   ad-free, tracking-free build.

Therefore the only way RustyNES reaches F-Droid (and ships a truly clean sideload APK) is
a build that contains **none** of the proprietary SDKs — i.e. a Gradle **product flavor**.

## Decision

Introduce a **`distribution` flavor dimension** with two flavors:

| Flavor | Contents | Channel |
|---|---|---|
| **`foss`** (default) | The pure-Rust emulator only — **no Google SDKs, no ads, no tracking**, no `AD_ID` permission | **F-Droid + GitHub-Releases sideload** |
| **`play`** | Everything proprietary: Billing, Cast, Play Games, Play Integrity, update/review, **+ AppLovin MAX + RevenueCat** (the freemium/ad layer) | **Google Play only** |

Mechanics:

- All six proprietary SDK groups move from unconditional `implementation(…)` to
  **`playImplementation(…)`**. The `foss` variant links none of them.
- The glue that touches those SDKs (`LicenseManager`/`Billing`, `CloudSave`/`PlayGames`,
  `Integrity`, `Cast`/`ChromecastSender`, in-app update/review, and the new
  `AdGate`/`RewardedGate`/RevenueCat wrapper) moves to **`src/play/`**, behind a small
  **façade**: an interface in `src/main`, a **no-op implementation in `src/foss/`**, and
  the real implementation in `src/play/`. `MainActivity` calls the façade only.
- The ad/billing manifest entries (the `AD_ID` permission, the AdMob `APPLICATION_ID`,
  AppLovin activities, the Play Games `app_id` meta-data) live in **`src/play/AndroidManifest.xml`**
  so they never merge into the `foss` manifest.
- `PLAY_BUILD` is set per-flavor (`true` for `play`, `false` for `foss`); the
  per-feature flags (`PGS_ENABLED`, …) stay as the in-`play` runtime gates the maintainer
  flips at launch.
- `foss` is `isDefault = true`, and an **`installDebug` task alias**
  (`tasks.register("installDebug"){ dependsOn("installFossDebug") }`) preserves the
  existing developer/CI command so the on-device test workflow is undisturbed.

The **`rustynes-monetization` crate itself is our own clean Rust code** (the `AdPolicy`
core — no Google, no ads), so its `.so` + UniFFI bindings are wired into the Android build
**now**, dormant; only the proprietary *glue* is flavor-scoped at v2.1.0.

## Consequences

- **F-Droid eligibility becomes real**: the `foss` flavor is a genuinely free, Google-free,
  ad-free, tracking-free build — submittable to F-Droid and the honest "clean sideload"
  channel the docs promise.
- **The `play` flavor is the only one that carries ads/tracking/IAP**, matching every
  store's expectations and keeping the `AD_ID` permission out of the sideload/F-Droid build.
- **Cost / risk**: this is the project's first product flavor — the variant matrix doubles
  (`foss`/`play` × `debug`/`release`), and five working, on-device-tested subsystems must
  migrate behind façades. It therefore requires **both flavors verified on-device** and is
  scoped as a **v2.1.0** deliverable (the launch is v2.1.0 and everything re-ports onto the
  v2.0.0 core anyway), not a v1.8.x side change.
- **Deferred until v2.1.0**: until the split lands, the proprietary SDKs remain unconditional
  and flag-gated (status quo). The `foss`-vs-`play` divergence, the F-Droid submission, and
  the on-device verification of both flavors are the v2.1.0 tasks.

Supersedes the implicit single-flavor "+ `BuildConfig` flag" posture for the proprietary
SDKs (it remains correct only until v2.1.0). Related: ADR 0024 (mobile bridge + hybrid
Android host); the monetization model in `to-dos/plans/v1.8.0-android-plan.md` +
`docs/monetization/`.

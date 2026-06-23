# RustyNES — Mobile Monetization Implementation Brief

> **Monetization model (decided).** This project's **ad-supported freemium** — **AppLovin MAX**
> ads + a **RevenueCat** `premium` entitlement, with a one-time **"Full Version / Remove Ads"
> ($3.99)** unlock — is the **primary** path. This is a deliberate maintainer override of the
> ad-free default in `to-dos/plans/v1.8.0-android-plan.md`. See `docs/rustynes-integration.md` for
> how it maps onto the real RustyNES repo (the Compose + wgpu-`SurfaceView` hybrid app, the
> `rustynes-mobile` bridge, the determinism boundary, and the v1.8.8 Play debut).

**Audience:** Claude Code (and any engineer) implementing freemium monetization in the
RustyNES mobile ports.
**Companion artifact:** the in-tree compiling skeleton (`core/`, `android/`, `ios/`) — shared Rust
`AdPolicy` core + Android/iOS shells). This document explains the *why*, the *decisions*,
the *sources*, the *store-submission blockers*, and the *follow-up enhancements* that the
skeleton intentionally leaves open. Read `docs/build-and-bindings.md` for build commands; read
this for everything around it.

> Scope note: "monetization" here = ads on the free tier + a paid tier that removes them.
> A short "beyond monetization" section at the end points at the rest of the mobile port.

---

## 1. Decisions already made (this session)

| Decision | Choice | Rationale |
|---|---|---|
| Entitlement / billing layer | **RevenueCat** (wraps Play Billing + StoreKit) | One entitlement model for both stores; RevenueCat keeps the underlying Play Billing Library current, sidestepping Google's version deadlines. |
| Ad mediation layer | **AppLovin MAX** | Strong mediation/auction for games; rewarded + interstitial + banner; one SDK fronts many demand networks. |
| Cross-language core | **Rust + UniFFI** | RustyNES is already Rust. UniFFI generates Kotlin **and** Swift bindings from one interface, so monetization *logic* is written and tested once. |
| Where logic lives | **In the Rust core** (`AdPolicy`) | Ad cadence, launch grace, premium suppression, and the paywalled-feature set cannot drift between Android and iOS because both call the same object. |
| Ad formats | **Rewarded (primary)** + sparing **interstitial** | Free tier is time-gated to 8 min/session; completed rewarded ads grant +2 min (addendum §2c/§2f). Interstitials only at game→library exit. |
| Paid model | One-time non-consumable **"Full Version / Remove Ads"** (~$3.99) | Removes the 8-min timer and unlocks save states + battery saves. Emulator users convert better on a one-time unlock than a subscription. |

The contract the shells call (verbatim generated names) is in `docs/build-and-bindings.md`'s API table.
The single source of truth for premium status is `AdPolicy.set_premium(bool)`, fed from
RevenueCat's `CustomerInfo.entitlements["premium"].isActive`.

---

## 2. Provider & reference links

**RevenueCat**
- Quickstart: https://www.revenuecat.com/docs/getting-started/quickstart
- Configuring the SDK: https://www.revenuecat.com/docs/getting-started/configuring-sdk
- Subscription / entitlement status: https://www.revenuecat.com/docs/customers/customer-info
- Android SDK reference: https://sdk.revenuecat.com/android/ (purchases-android)
- iOS SDK (SPM): https://github.com/RevenueCat/purchases-ios
- Paywalls (RevenueCatUI): https://www.revenuecat.com/docs/tools/paywalls
- Play Billing v9 migration (RevenueCat blog): https://www.revenuecat.com/blog/engineering/play-billing-v9/

**AppLovin MAX**
- Android integration (new init API): https://support.applovin.com/en/max/android/overview/integration
- iOS integration: https://support.applovin.com/en/max/ios/overview/integration
- iOS SKAdNetwork (Info.plist generator): https://support.applovin.com/en/max/ios/overview/skadnetwork
- iOS privacy: https://support.applovin.com/en/max/ios/overview/privacy
- Terms & privacy / consent (UMP) flow: https://support.applovin.com/en/max/ios/overview/terms-and-privacy-policy-flow
- Android SDK releases: https://github.com/AppLovin/AppLovin-MAX-SDK-Android/releases
- iOS Swift package: https://github.com/AppLovin/AppLovin-MAX-Swift-Package

**UniFFI**
- Repo: https://github.com/mozilla/uniffi-rs
- User guide: https://mozilla.github.io/uniffi-rs/
- Bindings generation: https://mozilla.github.io/uniffi-rs/latest/bindings.html
- cargo-swift: https://github.com/antoniusnaumann/cargo-swift
- cargo-ndk: https://github.com/bbqsrc/cargo-ndk

**Platform billing (reference even though RevenueCat fronts them)**
- Play Billing release notes: https://developer.android.com/google/play/billing/release-notes
- Play Billing deprecation/deadlines: https://developer.android.com/google/play/billing/play-developer-apis-deprecations
- StoreKit: https://developer.apple.com/documentation/storekit

**Compliance (see §6–§7)**
- Android 16 KB page sizes: https://developer.android.com/guide/practices/page-sizes
- Apple privacy & ATT: https://developer.apple.com/app-store/user-privacy-and-data-use/
- Apple privacy manifests: https://developer.apple.com/documentation/bundleresources/privacy_manifest_files

---

## 3. Integrate the core into the RustyNES workspace

The skeleton ships `rustynes-monetization` as a standalone crate. In the real repo, fold it into
the existing Cargo workspace rather than duplicating:

1. Add a `[workspace]` member (e.g. `crates/monetization` or a module inside the existing
   mobile-facing crate). Keep the emulator core and the monetization module in the **same**
   FFI crate so a single `.so` / `.a` and a single set of UniFFI bindings cover both. One
   `uniffi::setup_scaffolding!()` per crate — if the emulator already exports a UniFFI
   surface, merge the monetization `#[uniffi::export]` items into that crate instead of
   adding a second scaffolding call.
2. The mobile FFI crate's `crate-type` must include `cdylib` (Android) and `staticlib`
   (iOS) alongside `lib`. Pure desktop builds of RustyNES are unaffected.
3. Gate mobile-only deps behind a feature (e.g. `mobile`) so desktop builds don't pull
   UniFFI unnecessarily, if that matters to the existing build.
4. Keep `now_ms` host-injected (as the skeleton does). Do **not** read the clock inside the
   core — it preserves determinism and matches RustyNES's existing testability discipline.

**Build outputs:**
- Android: `cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -t x86 -o android/src/main/jniLibs build --release` → then `uniffi-bindgen ... --language kotlin`.
- iOS: `cargo swift package --platforms ios --name RustyNesMonetization --release` (or manual xcframework + `--language swift`).

---

## 4. Implementation task list (ordered)

1. **Core**: merge `monetization.rs` into the FFI crate; `cargo test` green. Decide the
   real `PremiumFeature` set (see §8/§10) — this enum is the only place the paywall is defined.
2. **Android native build**: wire `cargo-ndk` into Gradle (the Mozilla Rust Android Gradle
   plugin or a manual task), output to `jniLibs`. **Verify 16 KB alignment (§6).**
3. **Android bindings**: generate Kotlin into `app/rustynes/ffi`; add the JNA dep.
4. **Android shells**: drop in `RustyNesApp.kt`, `Billing.kt`, `AdGate.kt`; fill in real keys
   via `gradle.properties`; call `adGate.preload()` after SDK init and
   `adGate.maybeShowInterstitial()` at break points; gate features with `core.featureEnabled`.
5. **RevenueCat dashboard**: create the `premium` entitlement, the product(s), and the
   current offering; map products to the entitlement.
6. **AppLovin dashboard**: create the app + interstitial (and rewarded) ad units; add
   mediation networks (§7).
7. **Android compliance**: AD_ID permission, Data safety form, content rating, consent
   flow (§6, §7).
8. **Ship Android** to internal testing; verify entitlement flips and ad cadence on device.
9. **iOS native build**: `cargo swift` package; add to the Xcode/SwiftPM project.
10. **iOS shells**: `RustyNesApp.swift`, `Billing.swift`, `AdGate.swift`; inject keys via
    Info.plist/xcconfig.
11. **iOS compliance**: ATT string, SKAdNetwork IDs, privacy manifest, nutrition labels,
    App Review note (§6, §7).
12. **Ship iOS** ~1 week later via TestFlight → App Store.

---

## 5. The "ads vanish on the paid tier" mechanism (recap for reviewers)

There is no per-binary split and no separate "pro" app. One app, one IAP. Flow:

1. Launch → RevenueCat fetches `CustomerInfo` → shell calls `core.set_premium(active)`.
2. Every ad opportunity calls `core.should_show_interstitial(now_ms)`, which returns
   `false` whenever premium is set. The ad SDK is never even asked to show.
3. Purchase completes → RevenueCat listener/delegate fires → `core.set_premium(true)` →
   ads stop instantly, no restart.
4. Restore Purchases re-activates the entitlement on a new device/reinstall.

---

## 6. Store-submission blockers that specifically affect native (Rust) apps

These are the items most likely to **block a release** and that generic ad/IAP guides omit.

### 6a. Android — 16 KB memory page size (CRITICAL for the Rust `.so`)
New apps and updates targeting Android 15 (API 35)+ must support 16 KB page sizes on
64-bit devices; Google Play enforces this (rolled out Nov 1, 2025, one-time extension
window since closed). Any app shipping a native `.so` — which RustyNES does — must have
its shared libraries **16 KB-aligned** or Play blocks the upload.

Fix for the Rust toolchain:
- Build with **NDK r28+** (defaults the linker to a 16 KB max page size).
- Belt-and-suspenders: force the linker flag in `.cargo/config.toml`:
  ```toml
  [target.aarch64-linux-android]
  rustflags = ["-C", "link-arg=-Wl,-z,max-page-size=16384"]
  [target.armv7-linux-androideabi]
  rustflags = ["-C", "link-arg=-Wl,-z,max-page-size=16384"]
  ```
- Use **AGP 8.5.1+** so the AAB zip-aligns uncompressed `.so` files on 16 KB boundaries.
- Verify: `llvm-objdump -p librustynes_monetization.so | grep LOAD` → every `align` must be
  `2**14` (16384), not `2**12`. Or use Android Studio APK Analyzer / the Play Console
  App Bundle Explorer "Memory page size: Supports 16 KB".

### 6b. iOS — App Tracking Transparency (ATT)
- Add `NSUserTrackingUsageDescription` to Info.plist (AppLovin suggests copy like
  "This uses device info for more personalized ads and content.").
- Use AppLovin's built-in consent flow, which can present the ATT prompt; request consent
  **before** initializing the MAX SDK (the SDK records consent state at init).
- In **App Store Connect → App Review notes**, explicitly state that you use the ATT
  framework. Omitting this is a common rejection cause.
- Do not force/trick consent; respect the user's ATT answer (Guideline 5.1.1(iv)).

### 6c. iOS — SKAdNetwork / AdAttributionKit IDs
Each mediated network needs its `SKAdNetworkIdentifier` entries in Info.plist. Use
AppLovin's **Info.plist Generator** (link in §2), check every network you enable, and paste
the concatenated `SKAdNetworkItems` list. More IDs = more eligible demand = higher eCPM.

### 6d. iOS — Privacy manifest + nutrition labels
- Apple requires a privacy manifest. AppLovin, RevenueCat, and the mediation adapters ship
  their own SDK manifests, but **the app target needs its own `PrivacyInfo.xcprivacy`**
  declaring tracking (`NSPrivacyTracking = true`), tracking domains, collected data types
  (e.g. device id, advertising data, purchases), and any required-reason API usage.
- **Third-party SDK privacy manifests *and* signatures are mandatory** for listed SDKs
  (AppLovin and the ad adapters are on Apple's list) when you submit a new app, or an update
  that adds them. Apple's review now runs binary/dynamic checks against the declarations, so
  **ship current SDK/adapter versions** (older ones predate signed manifests) and confirm
  each adapter bundles one — a missing or unsigned manifest is a rejection cause.
- Fill the **App Privacy ("nutrition label")** section in App Store Connect to match what
  the ad SDK collects.

### 6e. Both stores — children / audience
AppLovin's terms **prohibit using the SDK in child-directed apps**. An NES emulator can
attract minors but is not "directed to children" if rated and marketed accordingly:
- Google Play: set Target Audience to teen/adult, complete the content rating
  questionnaire (IARC), and do **not** enroll in "Designed for Families."
- Apple: set an age rating of 12+ or higher; do not flag the app as kids-category.
- If you ever target children, ads + tracking become heavily restricted (COPPA / Families)
  and AppLovin is not permissible — a different monetization design would be required.

### 6f. Both stores — emulator policy & ROM legality
Emulators are allowed on both stores (Apple updated its guidelines in 2024 to permit retro
game console emulators; Google Play allows them). **Verify the current text** of Apple
Guideline 4.7 and Google's policy at build time. Constraints to honor:
- Ship the **emulator only** — do **not** bundle or distribute copyrighted ROMs.
- Provide a file-import path for user-supplied ROMs / homebrew (SAF on Android,
  `UIDocumentPicker` on iOS).
- Keep any "where to get games" guidance pointed at legal homebrew/public-domain sources.

### 6g. EEA / UK / Switzerland — certified CMP + IAB TCF consent (CRITICAL for ad revenue)
To serve **personalized** ads in these regions Google requires a **Google-certified Consent
Management Platform integrated with the IAB TCF** — enforced for the EEA/UK since **16 Jan
2024** and Switzerland since **31 Jul 2024**. Without it you are limited to
non-personalized / limited ads (a real revenue hit) and Google demand routed through MAX can
be disqualified. Use MAX's **Terms & Privacy Policy flow** (Google UMP) — wiring in §7 — and
**complete the consent flow before initializing the MAX SDK** (the SDK captures consent at
init). This is independent of iOS ATT (§6b): an EU iOS user needs *both* a TCF consent and
an ATT answer. The certified-CMP list and TCF version requirements move (e.g. the TCF v2.3
migration) — verify against Google's current policy at submission.

---

## 7. Mediation, consent & dashboard setup

**Fill the MAX waterfall.** MAX is only as good as the demand connected to it. Add the
adapter dependencies and enable each network in the MAX dashboard, then (iOS) add its
SKAdNetwork IDs:
- Google bidding (AdMob/Ad Manager) — usually highest demand; requires the AdMob app id in
  the Android manifest / iOS `GADApplicationIdentifier`.
- Meta Audience Network, Unity Ads, Liftoff Monetize (Vungle), Pangle, Mintegral, InMobi,
  ironSource.
- In-app bidding is configured server-side in MAX; the SDK just needs the adapter present.

**Consent (GDPR/UK/EEA).** Enable the MAX Terms & Privacy Policy flow (Google UMP):
- Android: configure in the init builder or `applovin_settings.json`.
- iOS: `AppLovin-Settings.plist` → `ConsentFlowInfo` (`ConsentFlowEnabled = YES`,
  `ConsentFlowPrivacyPolicy = <url>`), or set programmatically before init.
- Create & publish the GDPR message in the AdMob dashboard so the UMP form can display.
- If you bring your own CMP, establish consent **before** initializing MAX.

**US state privacy.** Set the "Do Not Sell" / opt-out flags via the MAX privacy APIs as
applicable.

---

## 8. Recommended enhancements (with concrete hooks)

### 8a. Rewarded ads (the free-tier engine — highest priority)
Rewarded video is opt-in, the highest-eCPM format, and store-friendly. **In RustyNES it is
the core of the free tier:** the free tier is time-gated to **8 minutes** per game session
(no save states, no battery-backed saves), and each **completed** rewarded ad grants **+2
minutes** of play. The full mechanic, core model, and host flow are in
`pre-implementation-addendum.md` §2c and §2f. Key rule: grant the time **only** on the ad
network's reward callback — Android `OnUserRewarded`, iOS `didRewardUser` — never on load,
show, or dismiss, so the grant maps to a qualifying view.

Core additions (**now implemented** in `monetization.rs`; see addendum §2f): `start_play()`,
`add_active_time(ms)`, `grant_rewarded_time()` (returns `bool`, enforces the 11-grant cap),
`can_offer_rewarded()`, `reward_grants_remaining()`, `is_play_allowed()`,
`play_time_remaining_ms()`, plus `base_play_ms` (480_000), `reward_play_ms` (120_000), and
`max_reward_grants_per_session` (11) in `AdConfig`. All 13 unit tests pass and bindings are
regenerated.

```rust
// add to monetization.rs, exported alongside AdPolicy
#[uniffi::export]
impl AdPolicy {
    /// Offer a rewarded ad whenever the free user is out of (or low on) play time.
    /// Premium users never need it.
    pub fn can_offer_rewarded(&self) -> bool {
        !self.is_premium()
    }
    // grant_rewarded_time() (addendum §2f) is what the reward callback calls to add +2 min.
}
```
Shell side: `MaxRewardedAd` (Android) / `MARewardedAd` (iOS), same delegate pattern as
`AdGate`; on the reward callback call `grant_rewarded_time()` then resume the emulator. Reuse
the same rewarded unit for optional conveniences (a session unlock of fast-forward, an extra
save-state slot, a shader trial) as a funnel toward the Full Version purchase.

### 8b. RevenueCat Paywalls (RevenueCatUI)
Instead of hand-building purchase UI, configure a paywall in the RevenueCat dashboard and
render it with `PaywallView` (SwiftUI) / `Paywall` Composable / `PaywallActivityLauncher`
(Android). It wires to the same offering the skeleton's `purchasePremium` already reads.

### 8c. Remote-tune ad cadence
`AdConfig` is passed in at construction, so source `min_interval_ms` / `launch_grace_ms` —
**and the free-tier `base_play_ms` / `reward_play_ms` / `max_reward_grants_per_session`** —
from a remote config (RevenueCat metadata or Firebase Remote Config) and adjust pacing and
the timer/cap without an app update. The full field list and an experiment plan are in
addendum **§9**; the funnel events to log are in addendum **§10**.

### 8d. Impression-level ad revenue (ILRD)
MAX emits per-impression revenue callbacks. Forward them to analytics and/or RevenueCat to
compute true LTV per cohort and to A/B the ad/no-ad and cadence variants.

### 8e. Formats to use sparingly or avoid
- **Banner ads:** poor fit over a fullscreen emulator viewport. If used, restrict to menu
  screens only — never over the game.
- **App-open ads:** easy to trip the disruptive-ads policy. If used, gate through the core
  and never on a cold start that interrupts immediate gameplay.

### 8f. A/B and experiments
RevenueCat Experiments (price/paywall) + MAX A/B (waterfall) once you have baseline data.

---

## 9. Testing strategy

- **AppLovin:** enable test mode for your device; start with test ad units; use the
  **Mediation Debugger** (`AppLovinSdk.getInstance(ctx).showMediationDebugger()` /
  `ALSdk.shared().showMediationDebugger()`) to confirm each network adapter initializes and
  to inspect the "Privacy States" the SDK logged.
- **RevenueCat / IAP sandbox:**
  - iOS: add a **StoreKit Configuration file** in Xcode for local purchase testing, or use
    App Store sandbox testers; confirm `entitlements["premium"].isActive` flips and that
    `set_premium(true)` reaches the core (ads stop without restart).
  - Android: add **license testers** in Play Console, publish to an internal track, ensure
    products are **active**; test purchase + restore + cancellation.
- **Core logic:** the Rust unit tests (13, all passing) cover premium suppression, launch
  grace, interval enforcement, mid-session upgrade, feature gating, the free-tier play-time
  budget + 11-grant cap, and `granted_entitlement_fully_unlocks_app` — which pins that a
  RevenueCat grant / sandbox purchase unlocks every gate (the contract your closed-test cohort
  relies on). Extend them as you add behavior; this is the cheapest place to catch bugs.
- **16 KB:** test on an Android 15 **16 KB** emulator system image; confirm no native crash
  and the bundle reports "Supports 16 KB."

### 9a. Granting your closed-test cohort the unlocked version
Google requires a personal account to run **≥12 testers for 14 continuous days** before
production (you're doing 15). Give them the Full Version without charging them — full
dashboard steps are in **runbook §5a**; the engineering view:
- **Nothing special in the app.** A RevenueCat **promotional grant** and a Play **license
  tester** test purchase both surface as `entitlements["premium"].isActive == true`, which
  `Billing` already forwards via `set_premium(true)`. The
  `granted_entitlement_fully_unlocks_app` test pins this. Prefer grants to just unlock the
  app; prefer license-tester purchases to validate the real billing path.
- **Debug override (local QA only, not the closed track).** `Billing.kt` / `Billing.swift`
  carry a `TESTER_UNLOCK` override that OR-s premium into the single `set_premium` path. It is
  double-gated — Android `BuildConfig.DEBUG && BuildConfig.TESTER_UNLOCK` (true only in the
  debug build type; see `build.gradle.kts`), iOS `#if DEBUG` + Info.plist
  `RUSTYNES_TESTER_UNLOCK`. It compiles to a constant `false` in release, and the closed-test
  track is a release build, so it never unlocks for the 15 testers — use a grant or license
  tester for them. The override adds **no second premium flag**: it still flows through
  `set_premium`, preserving the single-source-of-truth invariant.

---

## 10. Open decisions needed before building purchase UI

1. **Paid model:** one-time non-consumable "Full Version / Remove Ads" (recommended) vs
   subscription? Affects product setup and the paywall copy.
2. **Free-tier scope (largely decided):** the free tier keeps the full, cycle-accurate
   emulator but is **time-gated to 8 minutes per game session**, with **no save states** and
   **no battery-backed (SRAM) saves**; a **completed rewarded ad grants +2 minutes** (see
   addendum §2c/§2f). The Full Version removes the timer and unlocks save states + battery
   saves, so `PremiumFeature` includes at least `SaveStates` and `BatterySaves` (optionally
   fast-forward, shaders, cheats). Still to confirm: the base 8 min / reward 2 min values.
   Extensions are **capped at 11 per session** (max +22 min → 30 min total).
3. **Rewarded in the free tier?** Recommended yes — best eCPM and least intrusive.
4. **Initial mediation networks** to enable in MAX.
5. **Child-directed?** Recommended **no** (rate Teen/12+); enabling kids mode would forbid
   AppLovin and require a different plan.

---

## 11. Beyond monetization — mobile-port pointers

The monetization layer is one slice of porting RustyNES to mobile. For coherence, the same
shared-core/thin-shell split applies to the rest. Brief pointers (out of scope for this
brief but worth tracking):

- **Rendering:** core emits a framebuffer; present via Android `SurfaceView`/`GameActivity`
  + GLES/Vulkan, and iOS `MetalKit`/`CAMetalLayer`. Avoid copying the framebuffer across
  FFI per frame if possible — share a buffer or render natively from a pointer.
- **Audio:** Oboe (Android) / AVAudioEngine (iOS) fed by the APU sample stream; mind
  latency and the ad-display interruption (pause audio while an interstitial is up).
- **Input:** on-screen touch gamepad + hardware controllers (Android `InputDevice`,
  iOS `GameController` / MFi).
- **ROM & save I/O:** SAF / `UIDocumentPicker` for user-supplied ROMs; app-sandbox storage
  for save states and battery saves; optional cloud sync.
- **Lifecycle:** run emulation on a dedicated thread (never the UI thread); pause on
  background and when an ad is on screen; persist/restore state across interruptions.
  Monetization FFI calls are cheap and infrequent, so they can run on the UI thread.

---

## 12. Quick reference — generated FFI surface

| Rust | Kotlin (`com.doublegate.rustynes.monetization.ffi`) | Swift (`RustyNesMonetization`) |
|---|---|---|
| `AdPolicy::new(cfg, now_ms)` | `AdPolicy(config, nowMs: ULong)` | `AdPolicy(config:nowMs: UInt64)` |
| `default_ad_config()` | `defaultAdConfig()` | `defaultAdConfig()` |
| `set_premium(bool)` | `setPremium(premium)` | `setPremium(premium:)` |
| `is_premium()` | `isPremium()` | `isPremium()` |
| `should_show_interstitial(now_ms)` | `shouldShowInterstitial(nowMs)` | `shouldShowInterstitial(nowMs:)` |
| `notify_interstitial_shown(now_ms)` | `notifyInterstitialShown(nowMs)` | `notifyInterstitialShown(nowMs:)` |
| `feature_enabled(f)` | `featureEnabled(feature)` | `featureEnabled(feature:)` |
| `PremiumFeature::{SaveStates, SaveOnExitResume, BatterySaves}` | `PremiumFeature.SAVE_STATES` … | `.saveStates` … |

`now_ms` = monotonic ms: `SystemClock.elapsedRealtime()` (Android) /
`DispatchTime.now().uptimeNanoseconds / 1_000_000` (iOS).

---

*Part of the RustyNES monetization doc set (see `docs/README.md`). Verify SDK versions and
the current store policy text at implementation time — versions and deadlines move.*

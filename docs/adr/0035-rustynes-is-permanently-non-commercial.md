# 35. RustyNES is permanently non-commercial (no monetization)

Date: 2026-08-04

## Status

Accepted. **Supersedes** the monetization portions of
[ADR 0025](0025-foss-play-android-flavor-split.md) (the `play` ad/billing layer) and
**amends** [ADR 0027](0027-ios-distribution-and-app-store-compliance.md) (removes the
ad-bearing App-Store flavor, ATT, and StoreKit unlock; keeps the §4.7 ROM-compliance
rules, which remain valid for a free app).

## Context

Earlier planning (ADRs 0024–0027, the `to-dos/plans/` mobile train, and
`docs/monetization/`) laid out an ad-supported freemium model for the Android and iOS
apps: a `$3.99` one-time "Full Version / Remove Ads" unlock via **RevenueCat** + Google
**Play Billing** / Apple **StoreKit 2**, interstitial and rewarded ads via **AppLovin
MAX**, an 8-minute demo session with a rewarded-ad extension gate, and six "premium"
features — all staged in a dormant `crates/rustynes-monetization` crate wired into the
Android build, to be "activated" at a future joint store launch (variously v2.1.0, then
v2.3.0).

The maintainer has decided this is the wrong direction for the project. RustyNES is a
learning-driven hobby project, its emulation is well served for end users by the
in-RetroArch **libretro core** and by mainstream emulators, and a paid layer is at odds
with how the project is built and shared. It also complicates the honesty/provenance
posture the project cares about (see ADR 0030 and `docs/originality-and-provenance.md`).

## Decision

**RustyNES is and will remain open-source and income/profit-free, permanently.** There
is no monetization anywhere in the project: no ads, no tracking-for-revenue, no
freemium, no demo/time gate, no in-app purchase, and no paid unlock — in any build or
platform. The native Android and iOS apps are **kept as free FOSS apps** (fully
functional, no ads, no tracking); only the paid/ad layer is removed.

Concretely, in v2.2.6 "Almanac":

- The `crates/rustynes-monetization` crate and `docs/monetization/` are deleted and the
  workspace member removed. No emulation-core crate ever depended on it, so the
  deterministic core is untouched (AccuracyCoin 141/141 by construction).
- The Android paid layer is removed: `Billing.kt`, `MonetizationGate.kt`, the
  `monetization/` package (AppLovin/RevenueCat gates), paywall/demo strings, AdMob/
  AppLovin manifest entries, and the billing/ad Gradle deps + BuildConfig keys + the
  monetization cargo/uniffi tasks. `MainActivity` drops the demo/paywall/`unlocked`
  gating so every feature is unconditionally available. The `play`/`foss` flavor split
  and the free Google-Play *services* (Play Games achievements, Cast, Play Integrity,
  in-app update, cloud save) are retained.
- The iOS paid layer is removed: the StoreKit `StoreManager` (`Entitlements.swift`), the
  `appStore` monetization build channel, and billing entitlements/`project.yml` linkage.
- The ROADMAP / `docs/STATUS.md` / version plans are reframed: RustyNES is OSS/income-free
  forever; the freed v2.3.0 slot is repurposed for accuracy/fidelity work. A **free** app
  distribution (GitHub Releases sideload today; optionally a free F-Droid / App Store
  listing later) may still happen — with no monetization attached.

## Consequences

- **Positive:** the project's stated nature and its shipped artifacts finally agree; the
  provenance/honesty posture is simpler (nothing to reconcile against a paid product);
  and no code, credentials, or CI paths carry ad-SDK or billing dependencies. The
  emulation core, save-state/movie formats, and every golden vector are unchanged
  (AccuracyCoin 141/141 by construction — v2.2.6 is a doc/scaffold/app-shell change).
- **Negative / carried:** ADR 0025 is superseded and ADR 0027 amended; historical
  CHANGELOG entries that shipped with monetization scaffolding remain as history (the
  scaffolding was dormant and never activated). `rustynes-monetization` is deleted, not
  merely disabled, so re-introducing monetization would be a new, deliberate decision
  reversing this ADR — which is the intended bar.
- **Follow-up:** `docs/originality-and-provenance.md` and `NOTICE` disclose the TriCNES
  behavioral-calibration caveat (see ADR 0030) as part of the same honesty pass.

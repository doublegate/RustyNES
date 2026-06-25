# 27. iOS distribution & App Store §4.7 compliance

Date: 2026-06-25

## Status

Accepted (the v1.9.0 → v1.9.9 iOS train + the v2.1.0 joint launch). Extends ADR
0025 (the Android `foss` / `play` flavor split) to iOS.

## Context

RustyNES is shipping an iOS/iPadOS app across the **v1.9.0 → v1.9.9** TestFlight
train (ADR 0026; plan
[`../../to-dos/plans/v1.9.x-ios-train-plan.md`](../../to-dos/plans/v1.9.x-ios-train-plan.md)),
with the public App Store launch **deferred to v2.1.0** (joint with Android, after
the v2.0.0 "Timebase" rewrite, per
[`../../to-dos/plans/v2.0.x-mobile-finalization-plan.md`](../../to-dos/plans/v2.0.x-mobile-finalization-plan.md)).
A NES emulator on Apple's platforms raises distribution + compliance questions
that must be settled before the listing copy, the privacy manifest, the ROM-import
UX, and the monetization wiring are built — and they shape work as early as v1.9.0
(the first §4.7 review) and v1.9.8 (the compliance pass).

The landscape (April-2024 onward):

- **§4.7** (updated April 5, 2024, under EU DMA pressure) explicitly permits
  "retro game console and PC emulator apps" on the App Store for the first time.
  **Delta** (NES-capable) shipped April 17, 2024 and ranked #1; Provenance and
  RetroArch also ship. NES ROMs carry **no encryption**, so the legal posture is
  clean post-*Nintendo v. Yuzu* (no DMCA-circumvention question, unlike encrypted
  later-console BIOS/firmware).
- **TestFlight** is the interim channel for the whole v1.9.x train: the first
  build gets a full §4.7 review (~1–2 h), later builds usually skip review, builds
  expire after **90 days**, and up to **10,000 external testers** are allowed —
  with on-device testing required (the Simulator is insufficient).
- **AltStore PAL** (EU DMA) is a **notarization-only** self-hosted channel — no
  §4.7 content review, no business-model enforcement — the secondary outlet for
  the EU and for any feature App Review might reject.
- The established RustyNES monetization model is **ad-supported + a $3.99 one-time
  unlock** (AppLovin MAX + RevenueCat; ADR 0025), but the research strongly
  recommends a **privacy-first launch** for App-Review approval (Delta's lesson:
  emulators succeed when privacy-first / user-respecting / free-or-freemium).

## Decision

**1. Distribution sequence: TestFlight (v1.9.x) → App Store + AltStore PAL +
F-Droid + Google Play (v2.1.0).**

- **v1.9.0 → v1.9.9 = TestFlight only**, free + full-featured. The v1.9.1 patch
  wires the ~60-day re-upload cadence so external testers stay live across the
  90-day build expiry. No App Store submission until v2.1.0.
- **v2.1.0 = the joint launch.** The App Store (US + global) is primary;
  **AltStore PAL** (EU, notarization-only, self-hosted) is the secondary channel
  for the EU + any App-Review-rejected feature; a GitHub-source FOSS build is
  always available. This is the milestone the old Google-Play "Workstream P" + the
  iOS App Store launch both collapse into.

**2. §4.7 compliance, baked in from v1.9.0:**

- **No bundled or downloadable ROMs.** Zero copyrighted ROMs in the `.ipa`; ROM
  import is **user-provided only** via `UIDocumentPicker` / Files / share-sheet /
  iCloud Drive. No in-app ROM-download mechanism and **no links to ROM sites**
  (§4.7.3 / the rejection-risk list). Optionally bundle only CC0 / public-domain
  homebrew.
- **A searchable library / index** of the user's imported ROMs (§4.7.4).
- **No Nintendo branding / trademarks / official art** without license.
- **An in-app ownership notice** (shown at onboarding, v1.9.3) stating the user
  must own / legally source their ROMs.
- **Age rating** 4+ as a tool; **age-gate** any M-rated content (§4.7.5).
- **A clean privacy manifest** — `PrivacyInfo.xcprivacy` with **"Data Not
  Collected"** for the foss / TestFlight builds; a required-reason-API audit; ATT
  declared only on the ad-bearing App-Store flavor. (Authored in v1.9.8's
  compliance pass.)
- On-device testing on real hardware (iPhone 14+/iPad) is a hard gate — Apple
  rejects crashes that only appear on devices, and ~40% of rejections are
  placeholder / incomplete-feature (§2.1), so each TestFlight build must be
  feature-complete for its scope.

**3. The monetization split — extend ADR 0025 to iOS, by channel not compromise.**

Monetization is **dormant through all of v1.9.x** (TestFlight is free +
full-featured); it activates at **v2.1.0** via the existing `rustynes-monetization`
crate (the platform-agnostic `AdPolicy` core) and an iOS flavor split mirroring the
Android `foss` / `play` split:

- **The App-Store / `appstore` flavor** carries the proprietary glue:
  **StoreKit 2 + RevenueCat** (the $3.99 "Full Version / Remove Ads" unlock) plus
  **AppLovin MAX** ads gated behind **App Tracking Transparency** (ATT). Its
  privacy label discloses the ad/attribution data collection.
- **The `foss` flavor** (AltStore PAL + GitHub) links none of it — ad-free,
  tracking-free, "Data Not Collected" — the iOS counterpart of the F-Droid `foss`
  Android build.

This reconciles the research's privacy-first recommendation with the established
ad-supported model: the privacy-first build is a real, shipping channel (foss /
TestFlight), and the ad-supported build is opt-in by channel. The split + the
dormant scaffolding (behind an `APPSTORE_BUILD`-style flag, off) are staged in
v1.9.8 and flipped on at v2.1.0.

## Consequences

- **Positive.** The §4.7 posture is satisfied by construction (no bundled ROMs, no
  download path, no ROM links, ownership notice, searchable index, clean privacy
  label) — the same discipline that got Delta to #1. The no-JIT confirmation (ADR
  0026) removes the one capability bar. TestFlight de-risks the whole train before
  a single App-Review submission. AltStore PAL provides an EU + rejection-proof
  fallback. The privacy-first foss channel and the ad-supported App-Store channel
  coexist without a single-build compromise.
- **Negative.** The 90-day TestFlight expiry forces a re-upload cadence across the
  train (handled by v1.9.1). The App-Store flavor's ATT + ad SDKs add a privacy
  label and a review surface the foss build avoids. AltStore PAL requires Apple
  notarization (lighter than review, but still a step) + self-hosting +
  periodic-refresh mechanics. Policy can drift — hence the standing directive to
  re-research §4.7 + the ad-SDK / ATT rules at the start of every v1.9.x.
- **Rejected — launch on the App Store before v2.0.0.** v2.0.0 is the one breaking
  (save-state / byte-identity) release; an app launched at v1.9.x would re-port +
  re-migrate users' on-device saves the moment v2.0.0 lands. Launching after the
  re-port is the single-launch path (the maintainer's 2026-06-23 replan).
- **Rejected — a privacy-first-only model that drops ads entirely.** It would
  abandon the established ad-supported $3.99 economics (ADR 0025); the by-channel
  split keeps both without compromising either.
- **Rejected — bundling homebrew ROM packs beyond CC0/public-domain.** Any
  copyrighted content risks the §4.7 bundling rejection; user-sourced-only is the
  safe default.

# RustyNES v2.0.8 "Harbor" ("Harborlight") — iOS release-candidate readiness

Readiness record for **v2.0.8 "Harborlight"**, the **iOS release candidate** and the final
release of the iOS finalization window (**v2.0.5 → v2.0.8**). `docs/STATUS.md` remains the
per-suite source of truth; this file is the authoritative v2.0.8 summary and carries the
App-Review §4.7 self-audit.

v2.0.8 is a **host / iOS-only** cut on the byte-identical v2.0.0 "Timebase" core:
**AccuracyCoin 141/141 (100.00%)**, nestest 0-diff, the `#![no_std]` chip stack untouched.
It stages the App Store scaffolding; **no store submission happens here** (that is v2.1.0).

## 1. What landed (host-authorable)

- **App Store Connect listing metadata** — `fastlane/metadata/ios/{en-US,es-ES}/`
  (name / subtitle / promotional text / keywords / description / release notes / support +
  marketing URLs) + top-level `copyright.txt`, mirroring the Android tree, namespaced under
  `ios/` so `deliver` and `supply` do not collide. Files only, no upload. Copy is within
  App Store field limits, accurate to the app, EN + ES.
- **Dormant App Store `release` lane** (`fastlane/Fastfile`) — stages the build + listing
  via `upload_to_app_store` with `submit_for_review: false` + `automatic_release: false`
  (never auto-submits) and `skip_screenshots: true`. **Not** wired into `ios.yml`; the
  interim channel stays TestFlight (the `beta` lane) until v2.1.0.
- **Version bump** — workspace `2.0.7 → 2.0.8`; iOS `MARKETING_VERSION → 2.0.8`.

## 2. App-Review §4.7 self-audit (emulators)

| §4.7 requirement | RustyNES posture | Status |
|---|---|---|
| No bundled or downloadable ROMs | Ships zero copyrighted game data; ROMs are user-sourced via the Files / `UIDocumentPicker` / share-sheet only, copied into the sandbox by SHA-256 | PASS |
| No in-app links to acquire ROMs | The app links only to its own GitHub / docs; no ROM sources | PASS |
| No Nintendo branding / trademarked art | App icon + UI are original; the on-screen pad is drawn from original geometry | PASS |
| In-app ownership notice | First-run onboarding shows the bring-your-own-legally-owned-ROM notice; the About screen restates it | PASS |
| Searchable library / index (§4.7.4) | The ROM library grid is the searchable index of user-imported titles | PASS |
| Age rating (§4.7.5) | 4+ (the app is a tool; user-provided game content is out of scope of the app's own rating) | PASS |
| No JIT / no-W^X reliance | Pure cycle-stepped interpreter; `mlua` is interpreted Lua 5.4 (not LuaJIT); wgpu→Metal shaders compiled by Apple's toolchain — the one iOS-withheld capability is a non-issue | PASS |
| Encryption / DMCA | NES ROMs carry no encryption, so there is no anti-circumvention question post-*Nintendo v. Yuzu*; the ROM images remain copyrighted, hence the strict user-sourced-only rule above | Documented |

Precedent: Delta (NES-capable) shipped and topped the App Store under the April-2024 §4.7.
The App Store + AltStore PAL launch is deferred to v2.1.0; AltStore PAL (EU DMA,
notarization-only) is the secondary channel for any feature App Review might reject.

## 3. Verification / validation

| Gate | Result |
|---|---|
| `cargo check --workspace` (version bump + lock regen) | PASS (version-only lock diff) |
| `cargo fmt --all --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS (no issues) |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | PASS |
| `no_std` thumbv7em cross-compile | PASS |
| both wasm32 clippy gates | PASS |
| `markdownlint` (pinned v0.39.0, changed docs) | PASS |
| Listing metadata within App Store field limits | PASS (name/subtitle ≤30, keywords ≤100) |

The `Fastfile` `release` lane is dormant (not CI-invoked); the metadata is plain text. The
Swift/Xcode build + on-device validation remain the documented macOS / TestFlight
carryover (no Xcode on the Linux host; the iOS CI job is tag-gated).

## 4. On-device / maintainer closeout (v2.0.9 / v2.1.0)

- **Screenshots + the app preview** captured on device (iPhone + iPad) — cannot be
  generated on the Linux host; required before an App Store submission.
- Provision the App Store Connect API key + fastlane match; archive with **Xcode 26 / the
  iOS 26 SDK**; `fastlane ios beta` (TestFlight now) → `fastlane ios release` at v2.1.0.
- Complete the App-Review submission + the age-rating / privacy questionnaire from App
  Store Connect.
- The full v1.9.9 on-device TestFlight checklist (ROM import, save / rewind, MFi
  controller, audio interruptions, ProMotion pacing, an accurate privacy label), plus the
  v2.0.6 crash-capture and v2.0.7 iOS-17 / Xcode-26 confirmations.

## 5. Window complete

v2.0.8 closes the **iOS finalization window (v2.0.5 → v2.0.8)**:

- **v2.0.5 "Landfall"** — core re-port onto Timebase + the pre-Timebase-movie warning.
- **v2.0.6 "Parity"** — opt-in crash reporting + feature-parity re-verification.
- **v2.0.7 "Trim"** — the Xcode 26 / iOS 26 SDK submission floor + deployment-target
  reconciliation + the privacy-manifest re-audit.
- **v2.0.8 "Harborlight"** — the iOS RC (listing metadata, the dormant App Store lane, the
  §4.7 self-audit).

Next: **v2.0.9** — the joint (Android + iOS) on-device readiness pass — then **v2.1.0**,
the joint Google Play + Apple App Store + AltStore PAL + F-Droid launch. Full phasing:
`to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md` and
`to-dos/plans/v2.0.x-mobile-finalization-plan.md`.

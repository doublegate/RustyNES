# iOS App Store Connect listing metadata (v2.0.8 "Harborlight")

Version-controlled App Store Connect listing copy for the **iOS** app, staged for the
v2.1.0 launch. This mirrors the Android `fastlane/metadata/android/` tree, namespaced
under `ios/` so `fastlane deliver` (iOS) and `fastlane supply` (Android) never collide.

- `en-US/`, `es-ES/` — per-locale App Store fields: `name`, `subtitle`,
  `promotional_text`, `keywords`, `description`, `release_notes`, `support_url`,
  `marketing_url`.
- `copyright.txt` — the non-localized copyright line.

**Files only — no upload.** These are staged for a maintainer to push via the dormant
`release` lane in `fastlane/Fastfile` (`deliver` with `metadata_path: fastlane/metadata/ios`).
That lane is **not** wired into CI and is not invoked until the **v2.1.0** joint launch;
the interim iOS channel stays **TestFlight** (the `beta` lane). Screenshots + the app
preview are an on-device / maintainer capture step (they cannot be generated on the Linux
host); see `docs/ios-v2.0.8-readiness.md`.

Copy is accurate to the app (a cycle-accurate NES emulator, bring-your-own-ROMs, open
source) and flags the release-candidate / TestFlight status per Apple §4.7.

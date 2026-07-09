# RustyNES v2.0.6 "Harbor" ("Parity") — iOS feature-parity readiness

Readiness record for **v2.0.6 "Parity"**, the second iOS/iPadOS release of the v2.0.x
"Harbor" mobile-finalization train and the second of the iOS finalization window
(**v2.0.5 → v2.0.8**). `docs/STATUS.md` remains the per-suite source of truth; this file
is the authoritative v2.0.6 summary.

v2.0.6 is a **host / iOS-only** cut on the byte-identical v2.0.0 "Timebase" core:
**AccuracyCoin 141/141 (100.00%)**, nestest 0-diff, the `#![no_std]` chip stack
untouched. Every change is presentation / host-side.

## 1. What landed

### Opt-in crash reporting (`CrashReporter.swift`)

The iOS analogue of the Android v1.8.8 `CrashReporter`, closing the v1.9.9 readiness gap.

- **Privacy-first, off by default.** `AppModel.crashReportingEnabled` (a `@Published`
  UserDefaults-backed flag, **not** cloud-synced) gates everything; with it off the app
  installs no handler and writes nothing, so the **"Data Not Collected"** label and
  `PrivacyInfo.xcprivacy` are unchanged. Exposed as a **Settings → Diagnostics** toggle.
- **Local-only.** On opt-in, `CrashReporter.install(enabled:)` chains an
  `NSSetUncaughtExceptionHandler` that writes a timestamped report (app version, device,
  OS, exception name / reason, `callStackSymbols`) to
  `Application Support/RustyNES/crash-logs/`, kept to the newest 10. **Nothing is
  uploaded**; a `CrashLogsView` (list → detail → **Copy** to clipboard / **Clear**) lets
  the user read and share manually (the detail view reads the log off-main via `.task` +
  `Task.detached`). The crash-log directory is marked **excluded from iCloud / device
  backups** (`isExcludedFromBackup`), so the logs never leave the device even through a
  backup / restore — reinforcing "nothing is uploaded". New UI strings (including the
  `(unreadable)` fallback) are localized **EN + ES**.
- **Crash-handler discipline.** The handler runs on an arbitrary thread in an unstable
  post-crash process, so it does the bare minimum: all metadata (device / OS / app
  version) and the log-directory URL are **pre-resolved and cached at `install()` on the
  main thread** (`install` is `@MainActor`, since `UIDevice.current` is main-actor
  isolated — reading it from the C callback would be an isolation violation), and log
  **pruning** (directory enumeration + deletion) is done at launch, not in the handler.
  The handler only formats one string and writes one file — no `UIDevice.current`, no
  `Bundle.main.infoDictionary`, no directory creation, no enumeration.
- **Idempotent + runtime-honouring.** The handler installs once and re-checks the live
  `crashReportingEnabled` flag at crash time, so opting back out stops new logs
  immediately without needing to uninstall. It is installed at launch from
  `AppModel.init()` (a `didSet` does not run for in-init assignment, so the install is
  explicit) and lazily on the Settings toggle.
- **Honest scope.** `NSSetUncaughtExceptionHandler` catches the Objective-C / bridged
  `NSException` class only; pure-Swift traps abort via a POSIX signal it cannot see, and
  async-signal-unsafe file writes from a signal handler are unsound — so Swift-trap
  capture is left to a maintainer's third-party reporter (gated on the same flag), the
  same posture the Android side takes toward Firebase Crashlytics. Documented inline in
  `CrashReporter.swift` and in the release notes.

### Feature-parity re-verification

The v1.9.x host features were re-checked against the **v2.0.0 bridge surface** — the
timebase rewrite changed only the scheduler internals, not the `rustynes-mobile` typed
surface — and all continue to route through the unchanged bridge + the re-ported
save-state epoch:

| Feature | Bridge / host path | Status vs v2.0.0 |
|---|---|---|
| Game Center cloud saves / achievements | `GameCenterModel` (host, GameKit) | Unchanged — no bridge dependency |
| iCloud KV-store config + CloudKit `.rns` sync | `CloudConfigSync` / `CloudSaveStateSync` over the platform-agnostic `.rns` format | Unchanged — `.rns` epoch re-ported in v2.0.5 |
| MFi / hardware controllers (hot-plug, P1–P4) | `GameControllerManager` → `set_controller` late-latch | Unchanged — same late-latch bitmask |
| ReplayKit capture / share + PiP | `ScreenRecorder` (host) | Unchanged — no bridge dependency |
| Accessibility (VoiceOver, Dynamic Type, colorblind) | SwiftUI host + `AccessibilityPalettes` | Unchanged |

### Version

Workspace `2.0.5 → 2.0.6` (version-only `Cargo.lock` cascade); iOS `MARKETING_VERSION →
2.0.6` in `ios/project.yml`.

## 2. Verification / validation

### Host gate suite (all green)

| Gate | Result |
|---|---|
| `cargo check --workspace` (version bump + lock regen) | PASS (version-only lock diff) |
| `cargo fmt --all --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS (no issues) |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | PASS |
| `no_std` thumbv7em cross-compile | PASS |
| both wasm32 clippy gates | PASS |
| `Localizable.xcstrings` JSON validity (EN source + added ES) | PASS |
| `markdownlint` (pinned v0.39.0, changed docs) | PASS |

### Swift-side discipline (not host-certifiable)

New Swift **matches the SettingsView's existing SwiftUI baseline** — the file already uses
`NavigationStack`, closure-form `NavigationLink { } label: { }`, and
`ToolbarItem(placement: .topBarTrailing)` (the "Done" button), so the new `CrashLogsView` /
`CrashLogDetailView` follow that same pattern and introduce **no availability regression**.
The crash core (`NSSetUncaughtExceptionHandler` with a non-capturing `@convention(c)`
closure, `UIPasteboard`, `Button(role:)`, `.textSelection`) is iOS-15-safe.

> **Pre-existing discrepancy flagged for the maintainer (→ v2.0.7):** `ios/project.yml`
> declares `options.deploymentTarget.iOS: "15.0"`, but the pre-existing
> `NavigationStack` (iOS 16) + `.topBarTrailing` (iOS 17) usage across `SettingsView` (and
> other views) already requires a higher floor. v2.0.6 does **not** change the target
> (out of scope, and the new code merely follows the established pattern); reconciling
> `deploymentTarget` / `IPHONEOS_DEPLOYMENT_TARGET` against the real archive target is a
> v2.0.7 polish item.

New files are auto-included by xcodegen (`sources: path: RustyNES`), so no `project.yml`
source edit is needed. The Swift **compile** + on-device behaviour remain the documented
macOS / TestFlight carryover — there is no Xcode on the Linux host, and the iOS CI job is
tag-gated (§4).

## 3. On-device / maintainer carryovers (v2.0.9 / the TestFlight upload)

- Build with **Xcode 26 / the iOS 26 SDK** (the App Store submission floor — a hard gate
  at v2.0.7) and confirm the Diagnostics toggle + crash-log viewer render on device.
- Force a test `NSException` on device: confirm a local log is written, listed, readable,
  copyable, and pruned to 10; confirm toggling reporting off stops new logs.
- Re-run the v1.9.9 on-device TestFlight checklist (ROM import, save / rewind, MFi
  controller, audio interruptions, ProMotion pacing, an accurate privacy label).

## 4. Carryovers to v2.0.7 → v2.0.8

Polish / performance + the **privacy-manifest + required-reason-API audit** + the **Xcode
26 / iOS 26 SDK** submission-floor wiring + the **`deploymentTarget` reconciliation** (the
pre-existing 15.0-vs-actual discrepancy above) land in **v2.0.7**; the App Store Connect
listing + signing scaffold + §4.7 self-audit in the **v2.0.8** iOS RC. Post-Timebase
feature carryovers (FDS/NSF, the 20-band EQ, `.dbg` source maps, a cheats DB, box-art
scraping, a dedicated external-display output) remain deferred for re-evaluation at/after
v2.1.0. Full phasing: `to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md`.

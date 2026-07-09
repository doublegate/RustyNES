# RustyNES v2.0.7 "Harbor" ("Trim") — iOS polish + submission-floor readiness

Readiness record for **v2.0.7 "Trim"**, the third iOS/iPadOS release of the v2.0.x
"Harbor" mobile-finalization train and the third of the iOS finalization window
(**v2.0.5 → v2.0.8**). `docs/STATUS.md` remains the per-suite source of truth; this file
is the authoritative v2.0.7 summary.

v2.0.7 is a **host / iOS-only** cut on the byte-identical v2.0.0 "Timebase" core:
**AccuracyCoin 141/141 (100.00%)**, nestest 0-diff, the `#![no_std]` chip stack
untouched. Every change is build-config / host-side.

## 1. What landed

### App Store submission floor (iOS 26 SDK / Xcode 26)

Apple mandates building every App Store Connect upload with the **iOS 26 SDK (Xcode 26)**
from **2026-04-28** (the *build* SDK, independent of the minimum OS). `.github/workflows/
ios.yml` gains a **"Select Xcode 26"** step before the xcframework build: it picks the
newest `Xcode_26*.app` on the runner via `xcode-select`, and on an older runner image it
**warns and falls back** to the default toolchain rather than failing — so the compile
job (this workflow's real CI value) keeps running everywhere, while the release runner
uses Xcode 26. Non-breaking and guarded.

### Deployment target reconciled (iOS 15.0 → 17.0)

`ios/project.yml` declared `options.deploymentTarget.iOS: "15.0"` and a target-level
`"15.0"`, but the SwiftUI shell uses **`NavigationStack`** (iOS 16) and, at **12+
unguarded call sites**, **`ToolbarItem(placement: .topBarTrailing)`** (iOS 17) — a real
15.0 build was never possible. Both are set to **iOS 17.0** to match the code's actual API
floor. The prior discrepancy was flagged in the v2.0.6 review; v2.0.7 makes the declared
minimum OS honest.

> **Product decision surfaced for the maintainer:** iOS 17.0 is the **minimum OS**. It is
> the smallest floor consistent with the code *as written*. To support iOS 15/16, the
> `.topBarTrailing` / `NavigationStack` usages must be availability-guarded (or
> `.topBarTrailing` → `.navigationBarTrailing`) first — v2.0.7 does not silently drop
> device support, it corrects a declaration the code already contradicted.

### Privacy manifest re-audited (no change needed)

`PrivacyInfo.xcprivacy` re-audited against the **v2.0.6 crash reporter**:

- **No new collected data type.** The reporter is off by default; when on it writes
  **local** files (excluded from iCloud / device backups via `isExcludedFromBackup`) and
  uploads nothing, so no `NSPrivacyCollectedDataType` entry applies.
- **No new required-reason API.** Its only required-reason API is **UserDefaults** (the
  opt-in flag), already declared `CA92.1`. It does **not** touch file-timestamp, disk
  space, system-boot-time, or active-keyboards APIs.

The audit outcome (manifest remains accurate, no functional change) is recorded inline in
the manifest header.

### Performance / energy notes

The Metal present path + CADisplayLink ProMotion pacing were reviewed at source level; no
host-side hot-path change was warranted (the core is untouched / byte-identical). App
thinning / on-demand-resources posture and the Instruments energy/thermal capture are
inherently on-device — staged for v2.0.9.

### Version

Workspace `2.0.6 → 2.0.7` (version-only `Cargo.lock` cascade); iOS `MARKETING_VERSION →
2.0.7`.

## 2. Verification / validation

| Gate | Result |
|---|---|
| `cargo check --workspace` (version bump + lock regen) | PASS (version-only lock diff) |
| `cargo fmt --all --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS (no issues) |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | PASS |
| `no_std` thumbv7em cross-compile | PASS |
| both wasm32 clippy gates | PASS |
| `markdownlint` (pinned v0.39.0, changed docs) | PASS |

The `ios.yml` change is additive + `set -euo pipefail`-guarded (selects Xcode 26 if
present, warns otherwise). The Swift/Xcode build + on-device profiling remain the
documented macOS / TestFlight carryover (no Xcode on the Linux host; the iOS CI job is
tag-gated).

## 3. On-device / maintainer carryovers (v2.0.9 / the TestFlight upload)

- Archive with **Xcode 26 / the iOS 26 SDK**; confirm the **iOS 17.0** deployment target
  builds and runs on device, and that the Xcode-26 CI select step resolves on the release
  runner image.
- Instruments **energy / thermal** capture on ProMotion hardware; validate the app-thinning
  size posture.
- Re-run the v1.9.9 on-device TestFlight checklist.

## 4. Carryovers to v2.0.8

The **iOS release candidate**: App Store Connect listing metadata (fastlane iOS, EN + ES),
a signing scaffold with a graceful fallback, and the §4.7 App-Review self-audit (no bundled
ROMs, ownership notice, searchable library, age rating). Post-Timebase feature carryovers
(FDS/NSF, the 20-band EQ, `.dbg` source maps, a cheats DB, box-art scraping, a dedicated
external-display output) remain deferred for re-evaluation at/after v2.1.0. Full phasing:
`to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md`.

# RustyNES v2.0.5 "Harbor" ("Landfall") — iOS re-port readiness

This is the readiness record for **v2.0.5 "Landfall"**, the first iOS/iPadOS release of
the v2.0.x "Harbor" mobile-finalization train — the iOS re-port of the frozen v1.9.9 app
onto the **v2.0.0 "Timebase"** core. It is the iOS analogue of the Android v2.0.1
re-port, and opens the iOS finalization window (**v2.0.5 → v2.0.8**). `docs/STATUS.md`
remains the per-suite source of truth; this file is the authoritative v2.0.5 summary.

v2.0.5 is a **host / iOS-only** cut. The deterministic core and chip crates are
untouched; the shipped / native / `no_std` / wasm core stays **byte-identical to
v2.0.4** and **AccuracyCoin holds 141/141 (100.00%, RAM-authoritative)**, nestest is
0-diff. Every change is presentation / host-side.

## 1. What landed

- **The pre-Timebase movie warning, surfaced + localized on iOS.** The v2.0.0 timebase
  rewrite bumped the `.rns`/`.rnm` epoch (ADR 0028): a pre-v2.0.0 `.rnm` still replays
  its input on the new core, but byte-exact framebuffer / audio reproduction is no
  longer guaranteed. The v2.0.3 bridge queues a machine-readable `HostWarning` code for
  this; v2.0.5 surfaces it on iOS:
  - `EmulatorCore.drainWarnings() -> [String]` drains `NesController.drainWarningCodes()`
    and maps each `HostWarning` to a localized string (idempotent — the queue empties on
    read, so each warning surfaces exactly once).
  - `AppModel.surfaceWarnings()` is called after every successful `moviePlay` (both the
    saved-`.rnm` `playMovie(at:)` path and the foreign-movie `importForeignMovie(at:)`
    path) and routes the text to a new `@Published var warningMessage`.
  - `ContentView` presents `warningMessage` through a **single alert that multiplexes
    the error + warning channels** — it prefers the error when both are queued and
    clears only the visible channel on dismissal, so neither is dropped (two chained
    `.alert` modifiers on one view would race — SwiftUI presents only one) and a
    warning still never reads as a failure.
  - `Localizable.xcstrings` gains the EN source string + its ES translation; the English
    key and the ES copy are **byte-identical to the Android `host_warning_pre_timebase_movie`
    resource**, so both platforms surface the same wording.
- **UniFFI-Swift binding surface re-confirmed against the v2.0.0 bridge.** The generated
  Swift surface (`drainWarningCodes() -> [HostWarning]`, `HostWarning.preTimebaseMovie`,
  `moviePlay(bytes:)`) matches the new host code, verified by a host emit (see §2).
- **Version realignment.** Workspace `2.0.4 → 2.0.5` (version-only `Cargo.lock` cascade,
  18 crates). iOS `MARKETING_VERSION 1.9.1 → 2.0.5` in `ios/project.yml` (the frozen
  v1.9.x default is realigned to the workspace version); the build number stays
  CI-injected by the fastlane `beta` lane.

## 2. Verification / validation

### Host gate suite

| Gate | Result |
|---|---|
| `cargo check --workspace` (version bump + lock regen) | PASS (46 crates; version-only lock diff) |
| `cargo fmt --all --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | PASS |
| `cargo build -p rustynes-core --target thumbv7em-none-eabihf --no-default-features` (`no_std`) | PASS |
| both wasm clippy gates | PASS |
| `markdownlint` (pinned v0.39.0, changed docs) | PASS |
| **UniFFI-Swift emit** (`uniffi-bindgen generate --library <host build> --language swift`) | PASS — emits `drainWarningCodes`, `HostWarning.preTimebaseMovie`, `moviePlay(bytes:)` |
| `Localizable.xcstrings` JSON validity (EN source + added ES entry) | PASS |

The Swift-side change references only symbols confirmed present in the regenerated
bridge surface; the exhaustive `switch` over `HostWarning` is sound (the generated enum
is compiled into the app module).

### Not host-certifiable (macOS / device)

The Xcode build, the `RustyNESFFI.xcframework` assembly (`scripts/build-ios-xcframework.sh`),
and any on-device run are **not** reproducible on the Linux host and remain the documented
TestFlight carryover (§3).

## 3. On-device / maintainer carryovers (v2.0.9 / the TestFlight upload)

- Regenerate the xcframework on macOS and archive with **Xcode 26 / the iOS 26 SDK** (the
  App Store submission floor — a hard gate at v2.0.7; deployment target may stay iOS 16/17).
- **Save-state migration** from a v1.9.x install onto the v2.0.0 epoch, and the
  **AccuracyCoin / SMB / Zelda determinism smoke** on Apple silicon.
- A physical-device **TestFlight boot smoke** (iPhone 14+ / iPad); confirm the
  pre-Timebase-movie notice renders correctly under both EN and ES locales, and that a
  post-v2.0.0 `.rnm` produces **no** warning.
- The full v1.9.9 on-device TestFlight checklist (ROM import, save / rewind, MFi
  controller, audio interruptions, ProMotion pacing, an accurate privacy label).

## 4. Carryovers to v2.0.6 → v2.0.8 (unchanged from v1.9.9 §5)

Feature-parity re-verify against the v2.0.0 behaviour (Game Center, CloudKit save sync,
controllers, capture/PiP, accessibility) lands in **v2.0.6**; polish / performance + the
**privacy-manifest + required-reason-API audit** + the submission-floor wiring in
**v2.0.7**; the App Store Connect listing + signing scaffold + §4.7 self-audit in the
**v2.0.8** iOS RC. The post-Timebase feature carryovers (FDS/NSF, the 20-band EQ, `.dbg`
source maps, a cheats DB, box-art scraping, a dedicated external-display output) remain
deferred for re-evaluation at/after v2.1.0. Full phasing:
`to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md`.

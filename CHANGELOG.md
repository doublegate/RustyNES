# Changelog

This is the concise, readable summary of notable changes to RustyNES — a few
tight highlights per release. For the full per-version detail (engineering
narrative, engine lineage, ADR references, PR trains, and technical rationale),
see [CHANGELOG-FULL.md](CHANGELOG-FULL.md). The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

RustyNES's cycle-accurate emulation core arrived in v1.0.0; the `v0.9.x` rows are
the documentary lineage of how that core was built (not standalone user
releases), and `v0.1.0`–`v0.8.6` are the original pre-1.0 engine that the
cycle-accurate core later replaced.

## [Unreleased]

## [2.0.8] - 2026-07-09 - "Harbor" (iOS release candidate — "Harborlight")

- The **iOS release candidate** and the final release of the iOS finalization window
  (v2.0.5–v2.0.8), on the byte-identical v2.0.0 "Timebase" core: **AccuracyCoin
  141/141**, nestest 0-diff, the `#![no_std]` chip stack untouched. Host / iOS-only.
- **App Store Connect listing metadata staged** (files only, no upload):
  `fastlane/metadata/ios/{en-US,es-ES}/` — name, subtitle, promotional text,
  keywords, description, release notes, support / marketing URLs, plus a copyright
  line — mirroring the Android `fastlane/metadata/android/` tree, namespaced under
  `ios/` so `deliver` (iOS) and `supply` (Android) never collide.
- **Dormant App Store `release` lane** added to `fastlane/Fastfile`: it stages the
  build + listing and **does not submit** (`submit_for_review: false`,
  `automatic_release: false`). It is **not** wired into CI — the interim iOS channel
  stays **TestFlight** (the `beta` lane) until the v2.1.0 joint launch, when a
  maintainer runs it with signing provisioned.
- **App-Review §4.7 self-audit** recorded (no bundled / downloadable ROMs, no in-app
  ROM links, no Nintendo branding, in-app ownership notice, searchable library,
  4+ age rating) in `docs/ios-v2.0.8-readiness.md`.
- **Release-automation fix:** the `release-auto` workflow's global `concurrency`
  group let GitHub cancel an older *pending* release run when a newer one queued
  behind the (slow) binary build — which silently skipped a middle version during a
  rapid train (v2.0.6 was dropped between v2.0.5 and v2.0.7; both have since been
  published manually). The group is now keyed per-commit, so distinct versions
  release independently and none is ever superseded.
- Version bump: workspace `2.0.7 → 2.0.8`; iOS `MARKETING_VERSION → 2.0.8`.
- Still **TestFlight-only**; the App Store + AltStore PAL launch is the future
  **v2.1.0**. Screenshots, real signing, the listing upload, and the App-Review
  submission are the maintainer / v2.0.9 / v2.1.0 closeout.

## [2.0.7] - 2026-07-09 - "Harbor" (iOS polish + App Store submission floor — "Trim")

- The third iOS finalization release (the v2.0.5–v2.0.8 window), on the
  byte-identical v2.0.0 "Timebase" core: **AccuracyCoin 141/141**, nestest 0-diff,
  the `#![no_std]` chip stack untouched. Host / iOS-only.
- **App Store submission floor wired.** Apple mandates the **iOS 26 SDK / Xcode 26**
  for every App Store Connect upload from **2026-04-28**; the tag-gated iOS CI now
  selects the newest Xcode 26.x on the runner (falling back with a warning on older
  images, so the xcframework build still runs). This pins the **build SDK**, separate
  from the minimum OS.
- **Deployment target reconciled `iOS 15.0 → 17.0`.** The SwiftUI shell already uses
  `NavigationStack` (iOS 16) and `.topBarTrailing` (iOS 17, unguarded, 12+ sites), so
  the prior 15.0 declaration was never actually buildable; 17.0 matches the real API
  floor. (Product note: this is the minimum OS; guard those APIs to target lower.)
- **Privacy manifest re-audited** against the v2.0.6 crash reporter: it collects no
  new data type and adds no new required-reason API (UserDefaults is already
  declared; local-only, backup-excluded, off by default), so `PrivacyInfo.xcprivacy`
  needs no change — documented in-manifest.
- Performance / energy review notes (Metal / ProMotion, app thinning) captured for
  the on-device pass. Version bump: workspace `2.0.6 → 2.0.7`; iOS
  `MARKETING_VERSION → 2.0.7`.
- TestFlight-only; App Store + AltStore PAL deferred to v2.1.0. On-device profiling +
  the Xcode-26 archive are flagged for the v2.0.9 readiness pass.

## [2.0.6] - 2026-07-09 - "Harbor" (iOS feature parity — "Parity")

- The second iOS finalization release (the v2.0.5–v2.0.8 window), on the
  byte-identical v2.0.0 "Timebase" core: **AccuracyCoin 141/141**, nestest 0-diff,
  the `#![no_std]` chip stack untouched. Host / iOS-only — no accuracy / save-state /
  determinism number moves.
- **New opt-in crash-reporting surface** (privacy-first, **off by default**) — the
  iOS analogue of the Android v1.8.8 `CrashReporter`, closing the v1.9.9 readiness
  gap. Enabled from **Settings → Diagnostics**, an uncaught-`NSException` handler
  writes **local** crash logs (viewable + copyable in-app; **nothing is uploaded**,
  so the "Data Not Collected" privacy label is unchanged). The handler re-checks the
  live opt-in at crash time, so opting out stops new logs immediately. EN + ES.
- **Feature-parity re-verification** of the v1.9.x host features against the v2.0.0
  bridge (Game Center, CloudKit save sync, MFi controllers, capture / PiP,
  accessibility) — all route through the unchanged bridge surface; recorded in
  `docs/ios-v2.0.6-readiness.md`.
- Version bump: workspace `2.0.5 → 2.0.6`; iOS `MARKETING_VERSION → 2.0.6`.
- TestFlight-only; the App Store + AltStore PAL launch stays deferred to v2.1.0.
  On-device crash-capture verification is flagged for the v2.0.9 readiness pass.

## [2.0.5] - 2026-07-09 - "Harbor" (iOS re-port onto Timebase — "Landfall")

- Opens the iOS finalization window (v2.0.5–v2.0.8) of the v2.0.x "Harbor" train:
  the iOS/iPadOS app is re-ported onto the v2.0.0 "Timebase" core — the iOS
  analogue of the Android v2.0.1 re-port. Host/iOS-only; the emulation core is
  unchanged and byte-identical to v2.0.4 (AccuracyCoin 141/141, nestest 0-diff).
- The iOS host now localizes bridge warnings (device-locale strings, EN + ES) for
  the pre-Timebase movie notice: loading a pre-v2.0.0 `.rnm` still replays its
  input, but surfaces a non-blocking notice that byte-exact framebuffer/audio
  reproduction is not guaranteed across the ADR-0028 timebase change — the iOS
  analogue of the Android v2.0.4 warning, verbatim wording and shared ES copy.
- The UniFFI-Swift binding surface is re-confirmed against the v2.0.0 bridge
  (`drainWarningCodes` / `HostWarning.preTimebaseMovie`); the iOS
  `MARKETING_VERSION` is realigned from the frozen v1.9.x default to `2.0.5`.
- TestFlight-only; the App Store + AltStore PAL launch stays deferred to the
  v2.1.0 joint milestone. On-device re-port verification (save-state migration +
  the AccuracyCoin / SMB / Zelda determinism smoke on Apple silicon) is flagged
  for the v2.0.9 dual-app readiness pass.

## [2.0.4] - 2026-07-08 - "Harbor" (Android release candidate)

- Android release-candidate milestone; the emulation core is unchanged and
  byte-identical to v2.0.3 (AccuracyCoin 141/141, nestest 0-diff) — a
  host/Android-only cut.
- The Android host now localizes bridge warnings (device-locale strings, EN + ES)
  for the pre-Timebase movie notice, completing the v2.0.2–v2.0.4 carryover.
- Version-controlled Fastlane / Play Console listing metadata (EN-US, ES-ES)
  staged for a maintainer upload; release signing wired with a graceful
  debug-signing fallback; debug-only StrictMode diagnostics.
- No store submission yet (that is the future v2.1.0 joint launch); the `foss`
  flavor stays behaviour-identical.

## [2.0.3] - 2026-07-08 - "Harbor" (2-cycle-ALE promoted to default — shipped AccuracyCoin 141/141)

- The 2-cycle-ALE octal-latch PPU fetch model is promoted to the shipped default
  (ADR 0030) — **shipped AccuracyCoin is now 141/141 (100%)**; both the "ALE +
  Read" and "Hybrid Addresses" PPU tests now pass on the default build.
- Two commercial titles render more TriCNES-faithfully at a mid-render `$2006`
  scroll write — Super Mario Bros. 3 and Uchuu Keibitai SDF.
- The Android `play` flavor gains its full (still-dormant) monetization surface
  (AppLovin MAX + RevenueCat); the `foss` flavor keeps a no-op twin.
- Netplay rollback-determinism fix (new PPU snapshot v5 tail); headless frame
  cost rises ~10% (still ~4x realtime), accepted for the accuracy gain.

## [2.0.2] - 2026-07-08 - "Harbor" (octal-latch PPU model — AccuracyCoin 141/141 flag-on)

- A new octal-latch multiplexed-bus PPU model (ADR 0030) ships **default-off**:
  flag-on it reaches AccuracyCoin 141/141, while the shipped default stays
  byte-identical to v2.0.1 at its honest 139/141.
- The model faithfully reproduces the NES PPU's pin-multiplexed VRAM bus
  (74LS373-class octal latch), modeling the two corruption events behind the
  "ALE + Read" and "Hybrid Addresses" tests.
- The correct oracle was identified as TriCNES (the AccuracyCoin author's own
  emulator), not Mesen2; promotion to the default is the deliberate v2.0.3 step.

## [2.0.1] - 2026-07-08 - "Harbor" (first Android re-port onto Timebase + AccuracyCoin re-sync + housekeeping)

- First release of the v2.0.x "Harbor" mobile-finalization train: the Android app
  is re-ported onto the v2.0.0 "Timebase" core.
- The AccuracyCoin oracle is re-synced to upstream (146 rows / 141 assigned
  tests); measured honestly at 139/141 — the two new PPU tests are known,
  documented gaps.
- Structural `foss` / `play` Android flavor split scaffolding (ADR 0025): a
  default `foss` flavor with no Google SDKs, no ads, no tracking.
- CI cost optimization (the heavy suite gated to release branches); uniffi
  0.31→0.32 and mlua 0.11→0.12 dependency bumps.

## [2.0.0] - 2026-07-03 - "Timebase" (one-clock master-clock rewrite + Vs. DualSystem)

- The scheduler substrate is rewritten from a five-counter, dot-lockstep model to
  a single canonical cycle counter with every-cycle bus access and a
  split-around-the-access PPU catch-up (ADR 0002 / ADR 0029), now the only path.
- RustyNES's designated breaking release (ADR 0003): the save-state (`.rns`) and
  TAS movie (`.rnm`) format epochs bump (ADR 0028) — a pre-v2.0.0 `.rns` slot now
  fails to load with a clear error instead of silently misreading stale data.
- New core-level Vs. `DualSystem` dual-console support (`Emu::Dual`) for the four
  Vs. arcade cabinet boards — core-and-test-harness-only in this release
  (frontend wiring deferred).
- AccuracyCoin holds 100% (139/139) across all five betas + rc.1; the R1/R2 MMC3
  IRQ-timing residual is by-design-deferred beyond this release with a
  mechanism-level finding recorded in ADR 0002.

## [1.10.0] - 2026-07-01 - "Arcade" (Libretro core + dependency refresh)

- A new native Libretro core (`rustynes-libretro`) integrates RustyNES into
  RetroArch — RetroAchievements, dynamic audio sync, and deterministic
  save-state / rollback.
- The egui GUI stack moves 0.34.3 → 0.35.0 plus an in-constraint transitive
  dependency refresh; the core stays byte-identical and AccuracyCoin holds
  139/139.
- The iOS release workflow no longer fails on every tag push when the signing
  secrets are absent.

## [1.9.9] - 2026-06-26 - "Workshop" (iOS creator / power tools + readiness gate)

- The final iOS TestFlight release before the v2.0.0 core rewrite — it brings the
  desktop creator / power tools to touch and runs a full pre-freeze readiness pass.
- Cheats (a Game Genie editor + raw-RAM poke), a read-only debugger inspector, a
  touch TAStudio piano-roll, foreign movie import (`.fm2` / `.bk2` / …), a
  host-side audio-depth DSP, and symbol-map loading.
- First iOS release to extend the shared bridge (additive forwarding only); the
  core stays byte-identical and AccuracyCoin holds 139/139.

## [1.9.8] - 2026-06-26 - "Horizon" (iOS store-readiness)

- iOS store-readiness: accessibility (VoiceOver, Dynamic Type, high-contrast /
  colorblind palettes), EN / ES i18n, ReplayKit capture, Game Center, and a
  privacy-manifest pass.
- A dormant StoreKit 2 scaffold + `foss` / App-Store seam (activation deferred to
  v2.1.0).
- SwiftUI-shell only; the core stays byte-identical and AccuracyCoin holds
  139/139.

## [1.9.7] - 2026-06-25 - "Relay" (iOS connectivity completion)

- iOS connectivity completion: room-code (CGNAT / TURN) netplay, robust
  GameController hot-plug, and iCloud save-state sync (CloudKit).
- SwiftUI-shell only; the core stays byte-identical and AccuracyCoin holds
  139/139.

## [1.9.6] - 2026-06-25 - "Link" (iOS connectivity & scripting)

- Surfaces the shared bridge's Lua scripting, RetroAchievements, and direct-IP /
  LAN netplay in the iOS SwiftUI shell.
- SwiftUI-shell only; the core stays byte-identical and AccuracyCoin holds
  139/139.

## [1.9.5] - 2026-06-25 - "Curator" (iOS power-user feature port)

- iOS power-user features: TAS `.rnm` movies, custom `.pal` palettes, `.zip`
  ROMs, a per-game overrides DB, HD-pack loading, and iCloud config sync.
- The core stays byte-identical and AccuracyCoin holds 139/139.

## [1.9.4] - 2026-06-25 - "Lens" (iOS Metal renderer + shader stack)

- Completes the iOS wgpu → Metal render path: the full shared shader stack
  (None / Scanlines / CRT / NTSC / Bisqwit) with per-filter controls.
- ProMotion 60–120 Hz pacing, surface-loss / background lifecycle handling, and a
  verified CoreAudio hot path.
- The core stays byte-identical and AccuracyCoin holds 139/139.

## [1.9.3] - 2026-06-25 - "Workshop-lite" (iOS settings, save-state slots, onboarding)

- iOS settings / persistence / onboarding: a sectioned Settings form, four
  save-state slots per ROM, an in-game pill menu, first-run onboarding + About,
  and iPad multitasking polish.
- The core stays byte-identical and AccuracyCoin holds 139/139.

## [1.9.2] - 2026-06-25 - "Input" (iOS multi-touch, controllers, haptics)

- iOS input: a true multi-touch on-screen NES pad (Android-parity render),
  responsive iPhone / iPad sizing, GameController P1–P4 with remapping, and
  optional Core Haptics.
- The core stays byte-identical and AccuracyCoin holds 139/139.

## [1.9.1] - 2026-06-25 - "Patch" (iOS TestFlight cadence + dormant freemium gate)

- An iOS TestFlight build-refresh cadence (a bi-monthly cron to keep external
  testers live) and a dormant freemium-gate scaffold (fully unlocked through the
  entire v1.9.x train).
- The core stays byte-identical and AccuracyCoin holds 139/139.

## [1.9.0] - 2026-06-25 - "Sunrise" (iOS / iPadOS foundation)

- The first iOS / iPadOS release: a native SwiftUI shell over the byte-identical
  Rust core via the shared `rustynes-mobile` UniFFI bridge.
- New `rustynes-ios` shim (Metal rendering + CoreAudio), the SwiftUI app, ROM
  import, save-states / rewind / run-ahead / TAS-playback, and build / ship
  tooling (xcframework + fastlane + CI); ADRs 0026 / 0027.
- Distributed as interim TestFlight (App Store deferred to v2.1.0); the core stays
  byte-identical and AccuracyCoin holds 139/139.

## [1.8.9] - 2026-06-25 - "Backlog" (creator tooling, debugger depth, full HD-pack parity, mappers 168→172)

- Mapper breadth grows 168 → 172 families (NTDEC / TXC / discrete-BMC multicarts)
  plus ~35 more UNIF board aliases.
- Full Mesen2 HD-pack parity (the Zelda texture-mapping bug fixed; every Mesen2
  HD-pack form now implemented).
- New creator tools: a Game Genie database, a BasicBot save-state input search,
  detachable panel windows, TAS re-record counts, A/V codec depth
  (H.264 / H.265 / VP9), a desktop on-screen controls overlay, and an FDS firmware
  manager.
- A dormant mobile monetization core (`rustynes-monetization`) is added and the
  `foss` / `play` flavor split decided (ADR 0025); the core stays byte-identical
  and AccuracyCoin holds 139/139.

## [1.8.8] - 2026-06-20 - "Atlas" (Google Play launch readiness)

- Android Google-Play launch readiness: the toolchain is modernized to the
  Android 16 (API 36) target mandate (AGP 9, Gradle 9, compileSdk 37).
- Adaptive / foldable / TV layouts, a modern-UX pass (edge-to-edge, predictive
  back, splash), Material You dynamic color, and EN / ES i18n.
- A box-art ROM library with scrapers + secure secret storage, a
  performance / startup / app-size pass, and capture / share + platform surfaces
  (screenshots, MP4 clips, PiP, a Quick-Settings tile, a home-screen widget).
- Play Games cloud saves, achievements / leaderboards, and Play Integrity — all
  default-off; the core stays byte-identical and AccuracyCoin holds 139/139.

## [1.8.7] - 2026-06-20 - "Android" (Connectivity completion)

- CGNAT / TURN room-code netplay so phones on cellular (symmetric-NAT) networks
  can play.
- A robust hardware-controller input pipeline (wired USB + Bluetooth, analog
  sticks / HAT, per-port P1–P4, remapping, turbo), a controller-aware UI, and
  Chromecast prep (default-off).
- Sideload-only build; the core stays byte-identical and AccuracyCoin holds
  139/139.

## [1.8.6] - 2026-06-20 - "Android" (Connectivity & scripting)

- Lua scripting, RetroAchievements, and direct-IP / LAN netplay on Android — each
  reusing the desktop engine over the shared bridge (now connectivity-complete,
  so iOS inherits all three).
- An Open / Close ROM toggle plus a Windows CI line-ending fix; the core stays
  byte-identical and AccuracyCoin holds 139/139.

## [1.8.5] - 2026-06-20 - "Android" (Power-user features)

- Custom `.pal` palettes, compressed `.zip` ROMs, the Bisqwit composite NTSC GPU
  filter, TAS `.rnm` movies, a per-game settings DB, and HD-packs on Android.
- The HD-pack subsystem is extracted to the shared `rustynes-hdpack` crate; the
  core stays byte-identical and AccuracyCoin holds 139/139.

## [1.8.4] - 2026-06-20 - "Android" (Native wgpu renderer & shaders)

- The NES picture now draws through wgpu on a `SurfaceView` (Vulkan / GLES)
  instead of a Compose `Bitmap` blit, opt-in behind a setting.
- A shared WGSL shader stack (the new `rustynes-gfx-shaders` crate):
  None / Scanlines / CRT / NTSC with per-filter tuning sliders, plus a cheaper
  native-audio hot path.
- The core stays byte-identical and AccuracyCoin holds 139/139.

## [1.8.3] - 2026-06-20 - "Android" (Controller, casting & polish)

- An authentic NES-004 on-screen controller, cast-gameplay-to-a-TV via the
  Presentation API, per-screen-mode controller size / opacity, a controller size
  slider, and graded haptics.
- First-run onboarding, an About dialog, a Clear Recent action, a Material-3
  Settings sheet, and a four-slot save-state manager.

## [1.8.2] - 2026-06-20 - "Android" (Input & the virtual controller)

- A multi-touch virtual NES controller (simultaneous presses, D-pad diagonals,
  slide-between-buttons) whose art and touch regions resize / remap in lockstep.
- The real RustyNES adaptive app icon plus an icon wordmark refresh, and a
  `PLAY_BUILD` flag so sideload / dev builds stay full-featured.

## [1.8.1] - 2026-06-19 - "Android" (Patch)

- The free-tier demo session is shortened from 10 minutes to 8 minutes.
- Confirmed the debug "Full Unlock" override is absent from the Play (release)
  build (R8 strips the dead branches).

## [1.8.0] - 2026-06-19 - "Android" (Platform Release)

- The first platform (not accuracy) release: a complete, shippable Android app,
  verified on a Samsung Galaxy Z Fold 7.
- A new shared `rustynes-mobile` UniFFI bridge + a `rustynes-android` platform
  crate + a Jetpack Compose app + an Android CI gate (ADR 0024).
- Full on-device emulation: audio, input, save-states / SRAM, a recent-ROMs
  library, video filters (AGSL CRT / scanlines), and a foldable-aware UI.
- Freemium: a free download with a one-time $2.99 "Full Unlock" (a 10-minute
  demo); the emulated output is byte-identical between demo and paid, and the
  pure-Rust core is byte-identical on ARM (AccuracyCoin 139/139).

## [1.7.1] - 2026-06-19

- Fixed a ROM-close GPU abort in release builds and cleaned up pause / unpause
  pacing + audio underruns.
- A Help → Documentation pane overhaul (word-wrap at any scale, a collapsible
  sidebar tree); HD-pack tile substitution now applies in the debugger / tool
  render branch.
- An exhaustive README rewrite for v1.7.0 "Forge".

## [1.7.0] - 2026-06-19 - "Forge" (Feature Release)

- The maximal desktop feature release: an i18n framework (a compile-time string
  catalog + a Settings language picker, ADR 0023) shipping English + Spanish.
- Web / wasm parity: browser Lua, the File System Access API, the Gamepad API,
  PWA / offline, and `?settings=` share-links.
- Audio depth (stereo panning, reverb / crossfeed, an output device picker, a
  20-band EQ, per-context volume), per-game `<rom>.json` config overrides + a DIP
  editor + a lag-frame counter, and browser RetroAchievements completion.
- A new `full` maximal-native-feature build + a `cargo full-run` alias; the core
  stays byte-identical and AccuracyCoin holds 139/139.

## [1.6.0] - 2026-06-18 - "Studio" (Feature Release)

- A shader / filter ecosystem: LMP88959 NTSC / PAL, hqNx / xBRZ upscalers, and a
  constrained RetroArch `.slangp` / `.cgp` preset importer.
- HD-pack HD audio (`<bgm>` / `<sfx>` OGG tracks via the `$4100` register), a
  TAStudio piano-roll, `.fm2` / `.bk2` movies, and a Mesen2-style debugger.
- Mapper breadth grows to ~150 families + UNIF, proper FDS, A/V recording, and
  shaders; the core stays byte-identical and AccuracyCoin holds 139/139.

## [1.5.0] - 2026-06-17 - "Lens" (Feature Release)

- Debugger visualization devtools: an Input Miniatures overlay, a graphical PPU
  event viewer, a PPU scanline-trace viewer + CHR → PNG export, and an HD-pack
  per-pixel inspector.
- Lua API growth, TASVideos-format work, an accessibility pass, and mapper
  breadth 113 → 123 families.
- Browser RetroAchievements scaffolding (ADR 0015); the core stays byte-identical
  and AccuracyCoin holds 139/139.

## [1.4.1] - 2026-06-16

- Four more BestEffort mapper boot / decode fixes (mappers 92, 94, 145, 147)
  surfaced by the boot-smoke-against-real-dumps pass.
- The boot-smoke screenshot corpus is reorganized to mirror the per-mapper tier
  layout; the core stays byte-identical and AccuracyCoin holds 139/139.

## [1.4.0] - 2026-06-16

- "Fidelity" — the compatibility-and-finish release: accuracy polish, a
  per-channel audio mixing UI, and a devtools finish (symbol loading + event
  breakpoints).
- Browser QoL (wasm `.rnm` movies + IndexedDB save-states), a measure-first
  performance pass, and a colorful `rustynes help` TUI + styled `--help`.
- Mapper coverage 101 → 113 families (boot-smoke verified); the core stays
  byte-identical and AccuracyCoin holds 139/139.

## [1.3.0] - 2026-06-16 - "Bedrock" (Feature Release)

- Toolchain modernization: Rust edition 2024, MSRV → 1.96, and the coordinated
  egui 0.34.3 / wgpu 29.0.3 / rfd 0.17.2 / naga 25 dependency tier.
- A frame-pacing fix, a Memory Compare (cheat-hunt) panel, a reorganized menu bar,
  and auto-save-on-change Settings.
- Mapper breadth → 101 families plus Vs. DualSystem header detection, and HD-pack
  `<condition>` gating + `<background>` regions; the core stays byte-identical and
  AccuracyCoin holds 139/139.

## [1.2.0] - 2026-06-15 - "Curator" (Feature Release)

- Library breadth + compatibility + reach: mapper coverage grows 51 → 87 families
  behind a CI-enforced accuracy-tiering honesty gate.
- `.zip` ROM loading + automatic `.ips` / `.ups` / `.bps` soft-patching, a
  per-game database + in-app ROM-Database editor, live NTSC knobs, a composable
  shader stack, and a (default-off) HD-pack loader.
- New peripherals (Family BASIC keyboard, SNES mouse, Arkanoid, a Game Genie DB),
  Lua `onNmi` / `onIrq` / `setInput`, and web touch controls; the SMB3 World 1-1
  flicker is fixed. The core stays byte-identical and AccuracyCoin holds 139/139.

## [1.1.0] - 2026-06-15 - "Scriptable" (Feature Release)

- The flagship Lua scripting engine (sandboxed Lua 5.4, a Mesen2 / FCEUX-style
  `emu` API).
- Visual filters (full NTSC composite + a CRT / scanline pass + `.pal` palettes),
  input & peripherals (Power Pad, turbo / autofire, an input-display overlay), and
  debugger devtools (breakpoints, a cycle trace, an event viewer).
- An NSF / NSFe music player + a 5-band EQ; additive only, so the determinism
  contract and AccuracyCoin 100% hold.

## [1.0.0] - 2026-06-13 - "Cycle-Accurate" (Production Release)

- The first 1.0: RustyNES's emulation core is replaced wholesale with a new
  cycle-accurate, master-clock-precise engine, reaching AccuracyCoin 100.00%
  (139/139) with nestest 0-diff.
- Determinism is a hard contract (bit-identical output), band-limited BLEP audio,
  51 mapper families, Famicom Disk System, and Vs. System / PlayChoice-10 arcade
  support.
- Rollback netplay (2–4 players, native UDP + browser WebRTC), TAS movies, Game
  Genie + raw-RAM cheats, rewind, and opt-in RetroAchievements.
- A polished always-on egui desktop shell, a live in-browser WebAssembly demo, and
  a synthesized documentation set. The `v0.9.x` entries below are the documentary
  lineage of how this core was built.

## [0.9.7] - 2026-06-13 - Optimized Performance (documentary lineage)

- Documentary lineage of the cycle-accurate core (not a standalone user release):
  display-sync pacing modes, run-ahead, dynamic rate control, a dedicated
  emulation thread, browser AudioWorklet, and byte-identical core
  micro-optimizations.

## [0.9.6] - 2026-06-13 - Platform Expansion + RetroAchievements (documentary lineage)

- Documentary lineage: RetroAchievements (rcheevos), Vs. System / PlayChoice-10
  RGB support, mappers 38 → 51, and N-peer netplay (UDP + a browser WebRTC mesh),
  plus real-BIOS FDS boot and real two-instance rollback fixes.

## [0.9.5] - 2026-06-13 - Netplay (documentary lineage)

- Documentary lineage: GGPO-style rollback netplay (up to 4 players, a mesh
  transport) built on the determinism contract, plus STUN / hole-punch and Vs.
  System RGB-PPU groundwork.

## [0.9.4] - 2026-06-13 - Coverage + Input + FDS (documentary lineage)

- Documentary lineage: mappers 25 → 38, expansion input devices (the Arkanoid
  Vaus paddle, the Zapper light gun), and full Famicom Disk System support (RAM
  adaptor, per-cycle timer IRQ, writable disks, 2C33 wavetable audio).

## [0.9.3] - 2026-06-13 - Master-Clock Scheduler -> 100% Accuracy (documentary lineage)

- Documentary lineage: the master-clock-precise scheduler became the only path
  and AccuracyCoin reached 100.00% (139/139), with region-exact CPU:PPU ratios
  (3:1 NTSC / Dendy, 3.2:1 PAL).

## [0.9.2] - 2026-06-13 - Accuracy Hardening + Frontend Features (documentary lineage)

- Documentary lineage: a nesdev accuracy-hardening pass, Game Genie + raw-RAM
  cheats, Four Score support, config-driven gamepad rebinding, and browser
  save-state / movie persistence.

## [0.9.1] - 2026-06-13 - Expansion Audio + Web + TAS (documentary lineage)

- Documentary lineage: VRC7 OPLL FM audio (completing the expansion-audio
  family), the WebAssembly target, and the `.rnm` TAS movie format
  (record / playback / branching).

## [0.9.0] - 2026-06-13 - Cycle-Accurate Core Engine + Frontend MVP (documentary lineage)

- Documentary lineage baseline: the new master-clock-precise, lockstep-scheduled
  core (the Bus owns all mutable state; a one-directional dependency graph),
  band-limited audio, 15 mappers, an egui frontend MVP with rewind + a read-only
  debugger overlay, and the six-layer testing strategy.

## [0.8.6] - 2025-12-29 - Sub-Cycle Accuracy Improvements

- DMC DMA cycle stealing, NES open-bus behavior, and per-CPU-cycle mapper
  clocking; 522+ tests, a 100% Blargg pass rate.

## [0.8.5] - 2025-12-29 - Cycle-Accurate CPU/PPU Synchronization

- True cycle-accurate CPU / PPU synchronization via a `CpuBus` `on_cpu_cycle()`
  callback plus a cycle-by-cycle `cpu.tick()`; VBlank timing tests now pass with
  zero-cycle accuracy.

## [0.8.4] - 2025-12-28 - CPU/PPU Timing & Version Consistency

- The PPU is stepped before the CPU cycle for accurate `$2002` reads at the
  VBlank boundary, plus version-string and doctest fixes.

## [0.8.3] - 2025-12-28 - Critical Rendering Bug Fix

- Fixed a framebuffer showing "4 faint postage-stamp copies" by converting NES
  palette indices to RGB via the lookup table before display.

## [0.8.2] - 2025-12-28 - M10-S1 UI/UX Improvements

- Desktop GUI polish: Light / Dark / System themes, a status bar, a tabbed
  settings dialog, keyboard shortcuts, and modal dialogs.

## [0.8.1] - 2025-12-28 - M9 Known Issues Resolution (85% Complete)

- Audio improvements (two-stage decimation via rubato, A/V sync), PPU edge cases
  (sprite overflow, palette-RAM mirroring), and hot-path `#[inline]` hints.

## [0.8.0] - 2025-12-28 - Rust 2024 Edition & Dependency Modernization

- Rust 2024 Edition across all crates (MSRV 1.88), eframe / egui 0.33, cpal 0.16,
  and new rubato 0.16 high-quality resampling; no user-facing breaking changes.

## [0.7.1] - 2025-12-27 - Desktop GUI Framework Migration

- Migrated the desktop frontend from Iced + wgpu to eframe + egui, adding
  CPU / PPU / APU / memory debug windows and a settings dialog.

## [0.7.0] - 2025-12-21 - "Perfect Accuracy" (Milestone 8: Test ROM Validation Complete)

- A 100% Blargg test-ROM pass rate (CPU 22/22, PPU 25/25, APU 15/15, Mappers
  28/28 — 90 total), via a cycle-accurate CPU `tick()` state machine, PPU
  open-bus emulation, and CHR-RAM support.

## [0.6.0] - 2025-12-20 - "Accuracy Improvements" (Milestone 7: Complete + M8 Progress)

- Timing refinements across CPU / PPU / APU / bus (APU frame-counter precision, a
  hardware-accurate mixer, 513/514-cycle OAM DMA); Blargg CPU tests up to 90%.

## [0.5.0] - 2025-12-19 - "Phase 1 Complete" (Milestone 6: Desktop GUI)

- Phase 1 MVP complete: the `rustynes-desktop` app — a fully playable NES
  emulator (egui / wgpu, 60 FPS, cpal audio, keyboard + gamepad, config
  persistence), delivered ahead of schedule; 400+ tests.

## [0.4.0] - 2025-12-19 - "All Systems Go" (Milestone 5: Integration Complete)

- The `rustynes-core` integration layer connecting CPU / PPU / APU / mappers: a
  hardware-accurate bus, cycle-accurate OAM DMA, a console coordinator, and a
  save-state framework; 398 tests.

## [0.3.0] - 2025-12-19 - "Mapping the Path Forward" (Milestone 4: Mappers Complete)

- A trait-based mapper framework with the 5 key mappers (NROM, MMC1, UxROM,
  CNROM, MMC3) for 77.7% game coverage, full iNES + NES 2.0 parsing, and MMC3
  scanline IRQ.

## [0.2.0] - 2025-12-19 - "The Sound of Innovation" (Milestone 3: APU Complete)

- A complete, hardware-accurate 2A03 APU: all 5 channels, a non-linear mixer, a
  configurable resampler, and a DMC DMA interface; 150 tests.

## [0.1.0] - 2025-12-19 - "Precise. Pure. Powerful." (First Official Release)

- The first release: a cycle-accurate 6502 CPU (all 256 opcodes, a 100% nestest
  golden-log match) and a dot-level 2C02 PPU (97.8% pass rate); 144 tests.

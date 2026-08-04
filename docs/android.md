# RustyNES on Android

> **Release replan (maintainer, 2026-06-23): the Google Play *production* launch is
> deferred to v2.1.0.** The v1.8.x line keeps shipping as **GitHub-sideload** builds,
> but the Play production launch is held until after **v2.0.0 "Timebase"** — the
> Android app is finalized across **v2.0.1–v2.0.4** on the v2.0.0 core and launched
> **jointly with iOS at v2.1.0**. RustyNES is permanently open-source and income-free
> (ADR 0035): the Play-Store launch material in this doc describes a free-app launch,
> not a near-term step. See
> [`to-dos/plans/v2.0.x-mobile-finalization-plan.md`](../to-dos/plans/v2.0.x-mobile-finalization-plan.md).
>
> **Status: v1.8.0 "Android" — a working emulator, verified on hardware.** The
> shared `rustynes-mobile` bridge, the `rustynes-android` platform crate, the
> Gradle/Compose app, and the Android CI gate are in place, and the app has been
> run end-to-end on a **Samsung Galaxy Z Fold 7** (Android 16, arm64): nestest
> passes all 14 CPU suites (the core is cycle-accurate on ARM), audio plays
> through `AudioTrack`, touch + hardware-gamepad input drive P1, save-states /
> SRAM / a recent-ROMs library persist, and the app runs full-screen on both the
> cover and the unfolded inner display with the system bars auto-hidden.
> **Shipped so far:** core + video (RGBA `Bitmap` blit) + audio + touch/gamepad +
> save-states/auto-resume + pause/fast-forward/mute + SAF ROM library. **Next
> increment (documented, not yet built):** the wgpu `SurfaceView` render path +
> the NTSC/CRT shader stack (Workstream B/F-shaders) — the current `Bitmap`
> renderer is complete and performant, so wgpu is a fidelity/perf upgrade rather
> than a blocker. See the release train in
> [`to-dos/plans/v1.8.0-android-plan.md`](../to-dos/plans/v1.8.0-android-plan.md).
> Per the locked MVP, netplay / RetroAchievements / Lua remain deferred to a
> follow-up mobile point release.

## Architecture

RustyNES ports to Android the same way it ports to the browser: the pure-Rust,
byte-identical core is unchanged, and only a new *host* is written. Nothing here
touches the chip stack or the emitted-frame/sample contract, so **AccuracyCoin
139/141** (the two newest upstream PPU tests are known gaps) and the oracles stay green on host CI; the Android CI only proves
the build links + (eventually) an instrumented smoke boot.

```text
┌──────────────────────────── android/ (Gradle module) ────────────────────────────┐
│  Jetpack Compose shell (Kotlin)                                                    │
│    • SAF ROM picker, touch overlay, settings, save-state manager                   │
│    • drives the emulator through the UniFFI-generated NesController                 │
└───────────────┬───────────────────────────────────────────────┬───────────────────┘
                │ generated Kotlin bindings (UniFFI)              │ JNI (surface/audio)
        ┌───────▼─────────────────────┐                  ┌────────▼───────────────────┐
        │ rustynes-mobile (shared)    │                  │ rustynes-android            │
        │  typed control surface:     │                  │  hot glue UniFFI can't do:  │
        │  load_rom / run_frame /     │                  │  ANativeWindow → wgpu,      │
        │  set_button / save_state    │                  │  the AAudio sink,           │
        │  #[uniffi::export]          │                  │  android_main (spike)       │
        └───────────────┬─────────────┘                  └────────────┬────────────────┘
                        └──────────────────┬─────────────────────────┘
                                  ┌─────────▼──────────┐
                                  │ rustynes-core      │  byte-identical cycle-accurate
                                  │ (+ cpu/ppu/apu/...) │  core — never touched
                                  └────────────────────┘
```

- **`crates/rustynes-mobile`** — the platform-agnostic bridge. Owns the typed
  control surface over `rustynes_core::Nes` and lets UniFFI generate the Kotlin
  (Android) and Swift (iOS, v1.9.0) bindings from the `#[uniffi::export]`
  surface. `std`, host-testable, no new determinism surface. Shared verbatim with
  the future iOS host.
- **`crates/rustynes-android`** — the Android platform crate. Carries only the
  narrow JNI/surface/audio glue UniFFI can't express, all behind
  `#[cfg(target_os = "android")]`. Holds the `android_main` entry for the beta.1
  winit+wgpu+egui spike. This is the one mobile crate that carries `unsafe`.
- **`android/`** — the Gradle module: a Material-3 Jetpack Compose shell that
  drives the core through the generated `NesController`. `cargo-ndk` produces the
  per-ABI `.so`; UniFFI generates the Kotlin bindings; both are wired as Gradle
  build steps.

See [ADR 0024](adr/0024-mobile-bridge-and-hybrid-android-host.md) for the binding
and rendering decisions (UniFFI bridge plus the hybrid wgpu/Compose host).

## Determinism & data

- Input converges on the **single late-latched `Buttons` mask per port**, exactly
  as the desktop and wasm hosts do — touch and hardware gamepad are
  indistinguishable to the core, so TAS/netplay/rollback are unaffected.
- Save-states use the **platform-independent `.rns` format**, so a state saved on
  desktop loads on Android and a `.rnm` TAS replays bit-identically — desktop⇄
  Android cross-play stays valid.
- **No commercial ROMs are ever bundled.** ROM bytes come exclusively from the
  Storage Access Framework document picker (no path, raw bytes → `Nes::from_rom`).
  The app declares no storage/network permissions and collects nothing.

## Prerequisites

```bash
# Rust targets (arm64 ships; x86_64 is the emulator/CI ABI)
rustup target add aarch64-linux-android x86_64-linux-android
# cargo-ndk drives the NDK cross-compile
cargo install cargo-ndk --version "^3"
# Android SDK + NDK r27+ (16 KB-aligned .so, required for Android 15+ on Play).
# Point cargo-ndk at the NDK:
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/<version>     # e.g. 29.0.14206865
```

## Build

### Just the Rust libraries (what host/CI verifies)

```bash
# Cross-compile both crates for the shipped ABIs into a chosen output dir:
cargo ndk -t arm64-v8a -t x86_64 --platform 26 \
  build --release -p rustynes-mobile -p rustynes-android

# Generate the Kotlin bindings from the built arm64 cdylib (API is
# target-independent, so any built library is a valid source of truth):
cargo run -p rustynes-mobile --bin uniffi-bindgen -- \
  generate --library target/aarch64-linux-android/release/librustynes_mobile.so \
  --language kotlin --out-dir target/uniffi-kotlin
```

The control surface is also exercised on host: `cargo test -p rustynes-mobile`
boots a synthetic NROM image, runs frames, round-trips a save-state, and checks
the input-mask + port-validation logic — all without any commercial ROM.

### The full app (Gradle)

The Gradle module wires `cargo ndk` (→ `app/src/main/jniLibs/<abi>/`) and
`uniffi-bindgen` (→ generated Kotlin) as `preBuild` dependencies, so a normal
Gradle invocation rebuilds the native libraries and bindings as needed:

```bash
cd android
gradle wrapper --gradle-version 8.11.1   # first time, materialises ./gradlew
./gradlew :app:installDebug              # alias -> installFossDebug (the default foss flavor)
./gradlew :app:assembleFossDebug         # foss (F-Droid/sideload) debug APK — no Google SDKs
./gradlew :app:assemblePlayDebug         # play (Google Play) debug APK
./gradlew :app:bundleFossRelease         # foss release AAB (unsigned unless keystore.properties)
./gradlew :app:bundlePlayRelease         # play release AAB
```

> **v2.0.1 (ADR 0025): the `distribution` flavor dimension.** The build now splits into a
> **`foss`** flavor (default) and a **`play`** flavor, so AGP names the variant tasks
> per-flavor (`assembleFossDebug`, `bundlePlayRelease`, …). The bare `installDebug` is kept as
> an alias to `installFossDebug` so the existing dev/CI command is undisturbed; the aggregate
> `assembleDebug` / `bundleRelease` anchors still fan out to both flavors. See "No
> monetization (ADR 0035)" below.

`minSdk 26` (AAudio floor), `targetSdk 35` (Play mandate), `compileSdk 35`. Ship
ABI is `arm64-v8a`; `x86_64` is included for the emulator. The release build runs
R8 (`proguard-rules.pro` keeps the JNA + generated-binding classes).

## Signing & distribution

Release signing reads a **gitignored** `android/keystore.properties`
(`storeFile`, `storePassword`, `keyAlias`, `keyPassword`) — this is the Play App
Signing **upload key** only; Play manages the app signing key. When the file is
absent, the release build stays unsigned so CI `bundleRelease` still links.

Emulators are allowed on Google Play (RetroArch / Dolphin / PPSSPP ship there)
**given no bundled ROMs and no ROM downloader** — RustyNES is compliant by design.
A guaranteed sideload / F-Droid + GitHub-Releases channel is maintained so the
project never depends solely on Play (also the home for the optional egui-debugger
power-user build).

## No monetization (ADR 0035)

RustyNES is permanently open-source and income-free — see
[ADR 0035](adr/0035-rustynes-is-permanently-non-commercial.md). The Android app is a
**free download** with every feature unconditionally available: no ads, no tracking,
no demo/time gate, no in-app purchase, and no paid unlock. There is no `LicenseManager`
paywall, no `full_unlock` product, and no freemium ad layer — the app that was
previously planned as an ad-supported / one-time-unlock freemium model (an earlier
`docs/monetization/` design set and the `rustynes-monetization` crate) was never
shipped to a store, and that plan and crate have since been removed entirely.

**`foss` / `play` flavor split (ADR 0025, amended by ADR 0035).** The build still
splits into a **`foss`** flavor (default — no Google SDKs; the **F-Droid** +
GitHub-sideload artifact) and a **`play`** flavor (optional free Google Play
services — Play Games achievements, Cast, Play Integrity, in-app update, cloud
save). Both flavors are free with identical gameplay features; the `play` flavor
carries no billing or ad SDKs. See
[`../to-dos/plans/v2.0.x-mobile-finalization-plan.md`](../to-dos/plans/v2.0.x-mobile-finalization-plan.md)
for the historical planning record.

## CI

`.github/workflows/android.yml` runs on changes to the mobile crates / `android/`:
it installs the Android targets + cargo-ndk, cross-compiles both crates for
arm64 + x86_64, generates the Kotlin bindings (smoke), checks **16 KB ELF
alignment** on the shipped arm64 `.so`, and best-effort-bundles the AAB. It is
**not** a required check — accuracy is gated only on host CI.

## Workstream status

| Workstream | State |
|---|---|
| **A** Foundation / build (bridge + platform crate + Gradle/Compose + CI) | **Done** — on-device |
| **C** Audio (`AudioTrack` sink, blocking-write pacing) | **Done** — on-device |
| **D** Input (touch overlay + hardware gamepad → one late-latched mask) | **Done** — touch on-device |
| **E** Save-states / SRAM / auto-resume / persistable ROM library | **Done** — save/load on-device |
| **F** QoL (pause / fast-forward / mute) + responsive/foldable/immersive UI | **Done** — on-device |
| **G** Play packaging (release AAB, signing config, distribution) | Release AAB verified (R8, arm64-only); signing is maintainer-manual |
| **M** Freemium $2.99 unlock + 10-min demo gating | Removed entirely (ADR 0035) — RustyNES is permanently open-source and income-free; every feature ships unconditionally available |
| **B** wgpu `SurfaceView` + surface-loss lifecycle | **Next increment** (Bitmap blit ships now) |
| **F** shaders / palettes / per-game DB / TAS UI | **Next increment** (depends on B for shaders) |

**Room-code / online netplay (v1.8.7).** Mobile netplay landed past the locked
MVP: alongside direct-IP / LAN play, the app hosts and joins **internet** matches
by sharing a short 6-char **room code**, traversing carrier-grade NAT (CGNAT) via
STUN discovery + UDP hole punching. The bridge exposes
`np_host_room(num_players, NpNetConfig) -> room code` / `np_join_room(room_code,
NpNetConfig)`; `NpNetConfig` carries the `signaling_url`, STUN servers, and an
optional TURN trio, overridable in Settings. The same signaling + coturn stack in
`deploy/` serves both this path and the browser path; the app ships with a
**placeholder** relay URL until the maintainer hosts it. The cone-NAT hole-punch
is end-to-end and loopback/mock-verified in CI; the symmetric-NAT TURN
relay-transport hand-off is now wired (`UdpTransport::from_relay` + `NpStatus.
relayed`, mock-TURN-verified by `relay_loopback`). Only a **live verify** — a
hosted coturn + two real cellular/symmetric-NAT devices — remains. See
`docs/netplay-webrtc.md` §2.5 and the **Mobile room-code checklist** in
`deploy/README.md`.

**Deferred to a follow-up mobile point release** (per the locked MVP):
RetroAchievements (Compose login UI over rcheevos) and Lua scripting. The egui
debugger stays an optional sideload power-user overlay.

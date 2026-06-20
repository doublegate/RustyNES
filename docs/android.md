# RustyNES on Android

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
100% (139/139)** and the oracles stay green on host CI; the Android CI only proves
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
./gradlew :app:assembleDebug             # debug APK
./gradlew :app:bundleRelease             # release AAB (unsigned unless keystore.properties present)
```

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

## Monetization (freemium $2.99 unlock)

The app is a **free download** with a single **one-time, non-consumable in-app
purchase** — "Full Unlock", product id `full_unlock`, **$2.99 USD** — via Play
Billing (Workstream M, `LicenseManager`). The free tier is a time-limited demo:

- a **10-minute play session per launch** (60 s in debug builds for testing),
- **save-states disabled** (Save/Load hidden),
- **save-on-background / auto-resume disabled**, and
- **on-cart battery-backed SRAM never persisted** across a close.

Everything else — full emulation accuracy, video, audio, input, pause/fast-forward,
the ROM library — is identical to the paid tier. The emulated framebuffer/audio is
byte-identical in demo and paid; every gate is host policy over persistence + a
session clock, so the core, the `.rns` format, and AccuracyCoin are untouched.

**Maintainer-manual Play Console setup (can't be CI-self-certified):**

1. In the Play Console → Monetize → Products → **In-app products**, create a
   **managed product** with id **`full_unlock`**, set its price to **$2.99**, and
   activate it.
2. Upload a build to a **closed/internal test track** and add a
   **license-tested** account — the Play purchase flow only runs for an app
   distributed through Play, not a sideloaded APK.
3. Verify: buy → the demo timer + Unlock button disappear and Save/Load appear;
   reinstall → "Restore purchase" (or the automatic `queryPurchasesAsync` on
   launch) re-grants the entitlement (a non-consumable is owned forever).

A `BuildConfig.DEBUG`-only `DBG:unlock` toggle in the control bar exercises the
gating + timer + unlock UI on a sideloaded build without Play (the gating logic is
verified that way; only the real billing transaction needs the Console).

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
| **M** Freemium $2.99 unlock + 10-min demo gating | **Done** — gating/timer/overlay on-device; Play product maintainer-manual |
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
relay-transport hand-off and live two-cellular-device play are tracked
carryovers. See `docs/netplay-webrtc.md` §2.5 and the **Mobile room-code
checklist** in `deploy/README.md`.

**Deferred to a follow-up mobile point release** (per the locked MVP):
RetroAchievements (Compose login UI over rcheevos) and Lua scripting. The egui
debugger stays an optional sideload power-user overlay.

# iOS / iPadOS host (`rustynes-ios` + the SwiftUI app)

The iOS / iPadOS application is a thin **native SwiftUI shell** over the
byte-identical Rust core, introduced by **v1.9.0 "Sunrise"** (the foundation
slice of the v1.9.0 -> v1.9.9 TestFlight train; see
`to-dos/plans/v1.9.x-ios-train-plan.md`). It is the Apple analogue of the Android
host: **one core-binding layer (`rustynes-mobile`), two platform shims
(`rustynes-android`, `rustynes-ios`).** This doc is the implementation spec for
the iOS side; it is the **spec**, not history — update it in the same PR as a
code change. The authoritative per-release status lives in `docs/STATUS.md`.

## Architecture — the determinism boundary holds

```text
┌──────────────────────── ios/ (SwiftUI app, native) ────────────────────────┐
│  RustyNESApp · ContentView · MetalGameView · ROMLibrary · SettingsView      │
│  TouchControlsOverlay · GameControllerManager · AudioSession (AVAudioSession)│
│        │ drives the core via …                  │ hot glue via the C ABI …   │
│        ▼                                         ▼                            │
│  NesController (UniFFI-generated Swift)    rustynes_ios.h (extern "C")        │
└────────│─────────────────────────────────────────│──────────────────────────┘
         │  rustynes-mobile (UniFFI bridge)          │  rustynes-ios (this crate)
         ▼                                           ▼
   rustynes-core (no_std + alloc, byte-identical)    wgpu->Metal · cpal CoreAudio
```

- **The chip stack + the emitted-frame/sample contract are never touched.** All
  iOS work is platform shell. The desktop / native / `no_std` / wasm builds stay
  **byte-identical**, **AccuracyCoin 100% (139/139)** holds on host CI, and the
  determinism contract (same seed+ROM+input => bit-identical framebuffer+audio on
  ARM) is preserved — so desktop <-> Android <-> iOS save portability (and, once
  it lands, netplay cross-play) stays valid. The iOS app consumes the same `Nes`
  snapshot/output the oracle validates.
- **The typed control surface is UniFFI, not hand-written.** `rustynes-mobile`'s
  `#[uniffi::export]` surface (`NesController`: `runFrame() -> Data`,
  `drainAudio() -> [Float]`, `setButtons(port, mask)`, `saveState`/`loadState`,
  `reset`/`powerCycle`, region/mapper/identity queries, plus the movie / HD-pack /
  RA / netplay methods) is generated for Swift exactly as it is for Kotlin. The
  SwiftUI app drives the emulator through that generated `NesController` class.
- **`rustynes-ios` adds only the hot glue UniFFI cannot express**, reached over a
  small hand-written C ABI (`crates/rustynes-ios/include/rustynes_ios.h`): the
  Metal surface lifecycle (Workstream B) and the CoreAudio sink (Workstream C).
  It is the iOS analogue of the Android `jni_glue`.

## `rustynes-ios` crate (`crates/rustynes-ios/`)

`crate-type = ["lib", "staticlib"]`. The `staticlib` is the per-arch `.a` the app
links; a Rust `staticlib` is **self-contained** (it bundles every rlib dependency,
so `librustynes_ios.a` carries `rustynes-mobile` + `rustynes-core` too — the
xcframework links one archive). The `lib` output keeps the host build
(`cargo build --workspace`) green so CI lints the crate: **every iOS-specific
symbol is `#[cfg(target_os = "ios")]`**, so on a Linux/macOS *host* this compiles
to a near-empty shell with no Metal / CoreAudio code. The real archive is built
against `aarch64-apple-ios` / `aarch64-apple-ios-sim`.

| File | Role |
|---|---|
| `src/lib.rs` | Host-safe `core_version()` + the `#[cfg(target_os = "ios")]` module gate. |
| `src/gfx_metal.rs` | Workstream B: `MetalGfx` — wgpu->Metal blit from a `CAMetalLayer`, 8:7-PAR letterbox, the shared CRT / NTSC / Bisqwit pipelines. Byte-for-byte the Android `gfx.rs` minus the window handle. |
| `src/audio.rs` | Workstream C: `AudioSink` — a cpal CoreAudio output stream fed by a lock-free SPSC ring (mono core samples -> device channels; silence on underrun). |
| `src/ffi.rs` | The `extern "C"` seam (gfx init/resize/render/set_filter/set_index_frame/destroy + audio new/push/sample_rate/pause/resume/destroy), opaque `*mut` handles. |
| `include/rustynes_ios.h` | The C header the Swift bridging header `#include`s and the xcframework packages. |

`unsafe` is confined to this crate + the FFI glue, each site with a `// SAFETY:`
note (the same exemption `rustynes-cheevos` / `rustynes-frontend` /
`rustynes-android` take); `#![allow(unsafe_code)]` keeps the workspace pedantic /
nursery clippy gates otherwise on.

### Metal surface — the one platform difference from Android

wgpu 29's public `SurfaceTargetUnsafe` has **no** `CoreAnimationLayer` variant.
The shim builds a surface from the **`UIView` pointer** (the SwiftUI `MTKView`, a
`UIView` whose backing layer is a `CAMetalLayer`) via a raw-window-handle 0.6
`UiKit` handle:

```rust
let raw_window = RawWindowHandle::UiKit(UiKitWindowHandle::new(view_nn.cast()));
let target = wgpu::SurfaceTargetUnsafe::RawHandle {
    raw_display_handle: Some(RawDisplayHandle::UiKit(UiKitDisplayHandle::new())),
    raw_window_handle: raw_window,
};
let surface = unsafe { instance.create_surface_unsafe(target) }?;
```

wgpu-hal reads `view.layer` and drives Metal — **no `objc2` dependency** is
needed. Everything downstream (device/queue, the three shader pipelines, the
`CurrentSurfaceTexture` acquire, `Lost`/`Outdated` reconfigure-and-skip) is
identical to the Android renderer and the desktop frontend; the WGSL is the shared
`rustynes-gfx-shaders` crate (single source of truth across all three platforms).

### Audio — cpal owns the stream, Swift owns the session

`AudioSink` opens a cpal CoreAudio output stream (cpal 0.18 builds for
`aarch64-apple-ios` out of the box) fed by a lock-free SPSC ring; the cpal
callback drains the ring, fanning each mono APU sample out to the device's
channels and emitting silence on underrun. **`AVAudioSession`** (category
`.playback`, activation, interruption / route-change / silent-switch handling) is
configured **Swift-side**; on an interruption / scene-background the app calls
`rustynes_ios_audio_pause` and pauses the emulator, so there is no special
teardown. (A full Hermite DRC resampler, as on the desktop `resampler.rs`, is a
documented v1.9.x follow-up; the foundation ships the lock-free ring.) The ring is
a frontend resampler stage — the **core samples are untouched**, so the audio
oracle and cross-device save portability are preserved.

## The SwiftUI app (`ios/`)

A checked-in **XcodeGen** spec (`ios/project.yml`) generates the `.xcodeproj`
(more maintainable + reviewable than a hand-authored `.pbxproj`; CI runs
`xcodegen generate` before `xcodebuild`). The app target links the
`RustyNESFFI.xcframework` as **"Do Not Embed"** (a static xcframework is linked,
not embedded/signed) plus the system frameworks Metal / MetalKit / GameController /
AVFoundation / UIKit, and includes the generated `Generated/RustyNESCore.swift`
(the UniFFI Swift bindings) and the `RustyNES-Bridging-Header.h` (`#include
"rustynes_ios.h"`).

- **Game surface (`MetalGameView`):** a `UIViewRepresentable` hosting the
  `MTKView`. A `CADisplayLink` (`preferredFrameRateRange` 60-120 for ProMotion;
  Info.plist `CADisableMinimumFrameDurationOnPhone = true`) drives the loop: each
  tick `nes.runFrame()` -> `rustynes_ios_gfx_render(...)` and `nes.drainAudio()` ->
  `rustynes_ios_audio_push(...)`. The drawable size comes from the view; a
  bounds/scale change calls `rustynes_ios_gfx_resize`. The core emulates at the
  console rate (60.0988 Hz); the audio sink absorbs the display beat.
- **Input:** the on-screen pad and a `GameControllerManager` (`GCController`
  discovery) both converge on the same `setButtons(port, mask)` late-latch as
  desktop / wasm -> TAS / netplay identical. **(v1.9.2)** the pad is a `UIView`-backed
  **multi-touch** responder (`touchesBegan/Moved/Ended` over all active touches,
  replacing the v1.9.0 single `DragGesture` so simultaneous distant presses
  register), NES-001-styled and sized from the available geometry (iPhone / iPad /
  split-view / Stage Manager); the GameController path handles **P1-P4** with a
  persisted remap model; optional **Core Haptics** (off by default) gives light
  press feedback. All of it still funnels through the one per-port bitmask, so the
  determinism contract is untouched.
- **ROM import (`ROMLibrary`):** `UIDocumentPicker` / `.fileImporter` /
  share-sheet, security-scoped, copied into `Application Support/RustyNES/roms/`
  keyed by SHA-256 (the desktop save-identity scheme). **Never bundle commercial
  ROMs.**
- **Storage + lifecycle:** `.rns` save-states + SRAM in the sandbox (the format is
  platform-independent -> cross-device save portability); SwiftUI `ScenePhase`
  pauses the loop / audio and drops the drawable on background, rebuilding on
  foreground. `PrivacyInfo.xcprivacy` declares no data collected.

## Build pipeline (`scripts/build-ios-xcframework.sh`)

`rustup target add` the iOS targets -> `cargo build --release -p rustynes-ios`
per arch -> `lipo` the simulator arches -> generate the Swift bindings from the
device `.a` (`cargo run -p rustynes-mobile --bin uniffi-bindgen -- generate
--library … --language swift`; rename the modulemap to `module.modulemap`) ->
assemble the headers dir (`rustynes_mobileFFI.h` + `rustynes_ios.h` +
`module.modulemap`) -> `xcodebuild -create-xcframework` -> `xcodegen generate`.
CI (`.github/workflows/ios.yml`) runs this on `macos-latest`, **gated to tag
pushes (`v*`) + manual dispatch** because macOS minutes bill ~10x — the host
`ci.yml` remains the accuracy / determinism authority and is never gated on a
device toolchain. `fastlane` (`match` read-only signing + `gym` + `pilot`) uploads
to TestFlight via an App Store Connect API key. **(v1.9.1)** a `schedule:` cron
(the 1st of every other month, ~60-day cadence) re-builds + re-uploads so external
testers don't lapse — TestFlight builds expire 90 days after upload.

A **dormant freemium gate** (`ios/RustyNES/Entitlements.swift`, v1.9.1) is wired
app-wide but fully unlocked through the v1.9.x train; it is the present-but-inert
seam the v2.1.0 launch points at the shared `rustynes-monetization` crate (the full
StoreKit 2 / RevenueCat scaffolding lands at v1.9.8 "Horizon"). Entitlement state
never reaches the deterministic core.

## App Store posture (§4.7) — feasible and precedented

Apple's April-2024 Guideline 4.7 permits retro-console emulators; **Delta**
(NES-capable) shipped and topped the App Store with user-provided ROMs, and
RustyNES is a **pure interpreter** so the no-JIT rule is a non-issue (the core is
cycle-stepped, `mlua` is interpreted, wgpu shaders are toolchain-compiled). NES
ROMs carry no encryption, so there is **no DMCA anti-circumvention question**
post-*Nintendo v. Yuzu* — but a ROM image is still a copyrighted work, so legality
rests on the user lawfully owning the ROM, which is exactly why the rule is
**user-sourced-ROMs-only, no bundling, no download path, no in-app ROM links, a
clear in-app ownership notice**. Full strategy + the distribution phasing
(TestFlight through v1.9.x; App Store + AltStore PAL deferred to **v2.1.0**, joint
with Android, after the v2.0.0 "Timebase" core rewrite) live in
`docs/adr/0027-ios-distribution-and-app-store-compliance.md` and
`to-dos/plans/v2.0.x-mobile-finalization-plan.md`.

## v1.9.0 status + carryovers

v1.9.0 "Sunrise" lands the **foundation**: the `rustynes-ios` shim, the SwiftUI
app, the xcframework build + tag-gated macOS CI + fastlane, and the MVP feature
set (core + Metal video + CoreAudio + touch / controller + save-states / rewind /
run-ahead / TAS-playback). **Lua / RetroAchievements / netplay are deferred** to
the v1.9.x train (the shared bridge already exposes them, so they reduce to
SwiftUI chrome — see the train plan). The host gates (fmt / clippy `-D warnings` /
rustdoc / `no_std` / wasm) stay green and AccuracyCoin holds 139/139, because the
crate is a host shell off-device.

**Maintainer-manual carryovers** (cannot be CI-self-certified, mirroring the
Android line): an Apple Developer Program account + bundle ID; the signing secrets
(`fastlane match` private repo + the App Store Connect API key); app-icon /
launch-screen art; and the **on-device TestFlight verification** (ROM import,
save-states / rewind, MFi controller, audio + interruptions across iOS 16/17/18,
no crashes on 5-10 test ROMs, ProMotion pacing, an accurate privacy label). See
`ios/README.md` for the local-build + verification checklist.

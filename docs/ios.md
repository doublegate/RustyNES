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
  **byte-identical**, **AccuracyCoin 139/141** (the two newest upstream PPU tests are known gaps) holds on host CI, and the
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

wgpu 29's public `SurfaceTargetUnsafe` had **no** `CoreAnimationLayer` variant.
wgpu 30 adds one (behind `cfg(metal)`); the shim keeps the `UiKit` path below,
which is the one that has run on devices, rather than switch uncompiled.
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
teardown. The ring and rate control are a frontend resampler stage — the **core
samples are untouched**, so the audio oracle and cross-device save portability
are preserved.

**v2.7.4 (frontend audit IOS-02 / 03 / 08 / 09).** The ring, rate control and
channel fan-out live in the host-tested `src/audio_ring.rs`; `audio.rs` keeps
only what needs cpal.

- **Dynamic rate control.** A 4-tap Hermite resampler on the producer side,
  steered by the ring's fill (±1%, the desktop's law), holds the queue at a
  **50 ms** target. Before, there was none: the queue went wherever the host and
  audio clocks drifted, up to its 250 ms cap. Playback starts only once the
  target is queued, and waits for it again after an underrun, so a gap is one
  gap rather than a crackle.
- The callback drains a whole buffer per index load and store, not one sample
  at a time, into a buffer allocated once; extra channels get the centre average
  rather than the left image.
- **A dead stream is reported.** A fatal cpal error (device gone, audio service
  lost, or a media-services reset) sets a flag the app polls through
  `rustynes_ios_audio_is_invalid`, and the app then rebuilds the sink.
- `scripts/ios-host-typecheck.sh` (run in CI's lint job) compiles the real
  `audio.rs` and `ffi.rs` on Linux against the real cpal, with only the Metal
  renderer stubbed. Before v2.7.4 nothing on a pull request compiled either
  file; only the tag-triggered `ios.yml` did. Since v2.9.3 it also
  `cargo check`s the real `gfx_metal.rs` against the workspace's wgpu (its
  dependencies are all platform-neutral), so a wgpu API change there fails a
  pull request instead of the release build. That is a compile check only: no
  Metal surface is created, so rendering still needs a device.

### v2.7.4 host fixes (frontend audit IOS-*, MOB-05, MOB-09)

- **Battery saves** (`BatterySave.swift`): `Application Support/RustyNES/battery/<rom-sha256>.sav`,
  the desktop's and Android's rules (battery bit only; size checked before
  reading; a wrong-size file never loaded or overwritten; atomic writes). Loaded
  before the first frame, compared once a second, written on close, on a game
  switch, and inside a background task when the app backgrounds.
- **Background tasks** (`BackgroundTask.swift`, IOS-04): CloudKit uploads and the
  background battery save run inside a UIKit background task that always ends,
  including from its expiration handler (an unended task gets the app killed).
- **CloudKit** (IOS-04, IOS-10): a local slot newer than iCloud's is re-uploaded
  instead of being marked synced; an upload never overwrites a newer server copy
  and saves with `.ifServerRecordUnchanged`, so a concurrent write from another
  device fails the save rather than being lost; the per-record save result is
  checked.
- **Audio session** (IOS-08): a media-services reset re-configures the session
  and rebuilds the sink, and the core also rebuilds a sink that reports its
  stream dead. An unplugged headset pauses as its own reason, cleared by the
  next menu close (it used to stay frozen until a background / foreground cycle).
- **Gestures** (IOS-05): the bottom-edge system gesture is deferred and the home
  indicator hidden while playing.
- **Thermal state** (IOS-11): at `.serious` / `.critical` the video filter is
  suspended until the device cools.
- **Netplay** (MOB-09): host-name resolution (join, room codes) runs off the
  main actor.

None of this is compiled on this project's Linux machines; it is on the
maintainer's device checklist for v2.7.4. The Rust side is type-checked on
Linux by `scripts/ios-host-typecheck.sh`.

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
  `MTKView`. A `CADisplayLink` drives the loop: each tick `nes.runFrame()` ->
  `rustynes_ios_gfx_render(...)` and `nes.drainAudio()` ->
  `rustynes_ios_audio_push(...)`. The drawable size comes from the view; a
  bounds/scale change calls `rustynes_ios_gfx_resize`. **v2.7.4 (audit IOS-01):**
  the link asks for exactly 60 Hz (it was 60-120, preferring 120), and when it
  runs at ~60 Hz the loop runs exactly one console frame per vsync -- display
  sync, as on the desktop -- so no frame is doubled or dropped. The core then
  runs 0.16% below the console's 60.0988 Hz, which the sink's rate control
  absorbs. On any other refresh a wall-clock accumulator at 60.0988 Hz remains
  the fallback. The view is keyed to its core (`.id`), so a ROM opened from
  another app while one runs gets a fresh view rather than a frozen one.
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
  keyed by the core's ROM identity (the desktop save-identity scheme; see the
  v2.9.9 key migration below). **Never bundle commercial ROMs.**
- **Per-game keys are the core's ROM identity (v2.9.9, NF-21).** The library,
  battery saves, save-state directories, per-game overrides and the RA progress
  sidecar are keyed by `NesController.romIdentity()` (`EmulatorCore.romIdentity`):
  `Nes::rom_sha256` as hex, the bytes after the 16-byte iNES header of the
  UNPACKED image (the whole image for FDS, NSF and UNIF). The importer computes it
  with `romIdentityOfFile` (`RomIdentity.identityHex`), which needs no FDS BIOS.
  Until v2.9.9 the key was `RomIdentity.sha256Hex` of the whole file, header and
  `.zip` container included, so two dumps differing only in their header did not
  share saves and a re-zipped ROM lost them. `AppModel.openGame` moves a
  pre-v2.9.9 entry once, library first (`ROMLibrary.rekey`: the ROM copy is
  written under the new key, read back and compared, the index saved, and only
  then the old file removed; when the index cannot be saved the key is rolled
  back, the copy dropped and the old file kept), then `battery/<k>.sav`, every
  file in `states/<k>/` and `ra-progress/<k>.bin` (`RomKeyMigration.migrateFiles`),
  then the override (`GameOverrides.rekey`). The rules are Android's
  (`RomKeyMigration` in `Persistence.kt`, pinned by `RomKeyMigrationTest`): a
  store moves only into an empty key, a taken key is never overwritten (the old
  copy stays), a file counts as moved only once its original is gone, and when
  the library entry cannot move nothing else does. **A partial move is
  retried:** the rekeyed entry records the old key (`LibraryEntry.pendingLegacyKey`,
  written in the same index save), every open of an entry carrying it re-runs the
  file and override moves, and the marker is cleared only when nothing is
  pending. A run that copied a file but could not remove the original finishes
  the move next time (an identical copy under the new key is a duplicate, so the
  original goes). Re-importing a pre-v2.9.9 file returns its old entry rather
  than adding a second; if that entry's ROM file is missing, the copy just
  imported is kept as the only one and the entry is pointed at it, marked. **Not migrated:** CloudKit save-state
  records (`state-<k>-<n>`), which the next upload writes under the new key.
  **Compiled, not run on a device:** the iOS workflow built every Swift source
  for the Simulator on `c0b04195`, the review-round head (run 37242884341,
  `BUILD SUCCEEDED`); the behaviour is device rows M5-M7 of
  `docs/mobile-v2.9.3-run-sheet.md`.
- **Storage + lifecycle:** `.rns` save-states + SRAM in the sandbox (the format is
  platform-independent -> cross-device save portability); SwiftUI `ScenePhase`
  pauses the loop / audio and drops the drawable on background, rebuilding on
  foreground. `PrivacyInfo.xcprivacy` declares no data collected.

### Creator / power tools (v1.9.9 "Workshop")

The final iOS TestFlight release before the v2.0.0 core rewrite adds a
creator/power-tools set. Every piece is **additive / opt-in** and forwards to
existing core APIs through additive `rustynes-mobile` bridge functions, so with
the tools unused the app behaves as v1.9.8 and AccuracyCoin holds 139/141 (the
determinism contract is untouched; the two newest upstream PPU tests are known gaps).

- **Cheats (`CheatsView`, in-game pill menu):** add / remove / clear / list Game
  Genie codes (the core's own cheat engine applies them live to PRG reads,
  exactly like the desktop) plus a raw-RAM editor that pokes / peeks a CPU-RAM
  byte (`$0000-$1FFF`) through the existing `poke_ram` / side-effect-free `peek`
  paths. No per-frame mutation is added, so an empty cheat set is byte-identical.
- **Read-only debugger inspector (`DebuggerView`), gated OFF the App-Store
  build** via `BuildChannel` (the FOSS / TestFlight channel only, ADR 0027): a
  CPU register view (`cpu_snapshot`), a disassembly around the PC
  (`rustynes-cpu`'s `disassemble_at` over a bounded byte window), and a CPU-RAM
  hex view (`peek`). All observational (never advances / mutates the core); a
  single "Step" advances exactly one frame while the inspector holds the emulator
  paused. Optional `.sym` / `.mlb` / `.nl` symbol files annotate the disassembly
  (parsed host-side in `SymbolMap.swift` — the core exposes no symbol API and the
  mobile bridge does not depend on the frontend's parser).
- **Touch TAStudio piano-roll (`TAStudioView`):** a frame-by-frame P1 input table
  (8 button columns). "Play" injects the table one mask per frame through the
  existing bridge (`setButtons` + `runFrame`, deterministic); "Save .rnm" arms
  the core's recorder (`movie_record_from_power_on`) and replays the table so the
  captured input is written to a real native `.rnm` movie. No movie-editing API
  is added to the core.
- **Foreign movie import (`MoviePanelView` + `UIDocumentPicker`):** FCEUX `.fm2`,
  BizHawk `.bk2`, Nestopia `.fcm`, Famtasia `.fmv`, and VirtuaNES `.vmv` are
  transcoded to a native `.rnm` for the loaded game (the bridge calls the core's
  `import_fm2` / `import_bk2` / `import_fcm` / `import_fmv` / `import_vmv`,
  serialising the result), then saved + played via the existing movie surface.
  Malformed files error gracefully (bounds-checked; the `.bk2` ZIP member
  extraction caps member sizes), never crash.
- **Host audio-depth DSP (Settings -> "Audio depth"):** an **output-only** stereo
  enrichment stage (5-band EQ, pan, Schroeder reverb, headphone crossfeed) ported
  from the desktop frontend, applied in the CoreAudio callback *after* the core's
  mono master is drained — never in the synthesis. It lives in the host-safe
  `crates/rustynes-ios/src/audio_dsp.rs` (unit-tested on the host build) and is
  wired into the cpal sink (`audio.rs`) over a lock-free atomic config mailbox. A
  disabled / flat / centered config is a **bit-exact passthrough**, so the audio
  oracle and save portability are preserved.

## Build pipeline (`scripts/build-ios-xcframework.sh`)

`rustup target add` the iOS targets -> `cargo build --profile release-mobile -p rustynes-ios`
(`release` with `panic = "unwind"`, v2.7.4, so the C-ABI panic guards are live) per arch -> `lipo` the simulator arches -> generate the Swift bindings from the
device `.a` (`cargo run -p rustynes-mobile --bin uniffi-bindgen -- generate
--library … --language swift`; rename the modulemap to `module.modulemap`) ->
assemble the headers dir (`rustynes_mobileFFI.h` + `rustynes_ios.h` +
`module.modulemap`) -> `xcodebuild -create-xcframework` -> `xcodegen generate`.
CI (`.github/workflows/ios.yml`) runs this on `macos-latest`, **never on a pull
request**, because macOS minutes bill ~10x — the host `ci.yml` remains the
accuracy / determinism authority and is never gated on a device toolchain. It
runs for a release, by hand, and on the TestFlight refresh cron described below
(dormant until the `IOS_SIGNING_READY` repo variable is set). "For a release"
means `release-auto.yml`, which calls `ios.yml` (`workflow_call`) for every
release it cuts. Its `push: tags` trigger never fires for those, because the
auto-release tag is pushed with `GITHUB_TOKEN`. So no release from v2.3.9 to
v2.9.7 built the iOS host. The call is unreleased as of this writing: it was
added after v2.9.7 and first runs on the next release. Manual dispatch remains
for an on-demand run, for example on a release branch before it merges. Each run then **compiles the Swift app** for the generic iOS Simulator,
unsigned. It is the only place CI builds the Swift target, and v2.9.7 shipped
with 24 Swift compile errors because nothing did. The run also fails if the
script modified any tracked file. `fastlane` (`match` read-only signing + `gym` + `pilot`) uploads
to TestFlight via an App Store Connect API key. The upload is **gated on the
signing secrets being present** (a "Detect iOS signing secrets" step): until they
are provisioned, the xcframework build still runs (proving the iOS host compiles)
but the TestFlight upload is **skipped with a notice (green)** rather than failing
every release tag push at fastlane's `app_store_connect_api_key` step. **(v1.9.1)**
a `schedule:` cron (the 1st of every other month, ~60-day cadence) re-builds +
re-uploads so external testers don't lapse — TestFlight builds expire 90 days
after upload.

A dormant freemium gate (`ios/RustyNES/Entitlements.swift`, added v1.9.1) was wired
app-wide but fully unlocked through the v1.9.x train. RustyNES is permanently
open-source and income-free (ADR 0035): that gate, the StoreKit 2 / RevenueCat
scaffolding it anticipated, and the `appStore` monetization build channel have
since been removed entirely — the app ships fully unlocked with no entitlement
state at all.

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
with Android, after the v2.0.0 "Timebase" core rewrite — a target since retired:
as of v2.9.4 no store launch has happened, distribution is **TestFlight**, and a
free listing waits until after the v3.x hardware-verification release, which
includes the first device run) live in
`docs/adr/0027-ios-distribution-and-app-store-compliance.md` and
`to-dos/plans/v2.0.x-mobile-finalization-plan.md`.

## v1.9.x → v2.0.x status + carryovers

v1.9.0 "Sunrise" landed the **foundation**: the `rustynes-ios` shim, the SwiftUI
app, the xcframework build + tag-gated macOS CI + fastlane, and the MVP feature
set (core + Metal video + CoreAudio + touch / controller + save-states / rewind /
run-ahead / TAS-playback). The v1.9.x train then shipped the deferred connectivity
and scripting: **Lua scripting (v1.9.6), RetroAchievements (v1.9.6), and netplay
(v1.9.6/v1.9.7)** are all live (the shared bridge already exposed them, so each
reduced to SwiftUI chrome), plus iCloud save-state sync (v1.9.7) and the
Google-Play-parity polish (v1.9.8). The current line is **v1.9.9 "Workshop"** —
the creator / power-tools release (read-only debugger inspector, raw-RAM editor,
Game Genie codes, foreign-movie import `.fm2`/`.bk2`/`.fcm`/`.fmv`/`.vmv`, the
TAStudio piano-roll, custom palettes, HD-packs, and the audio-depth controls). The
host gates (fmt / clippy `-D warnings` / rustdoc / `no_std` / wasm) stay green and
AccuracyCoin held 139/141 across the v1.9.x line (measured on the v2.0.1 oracle
re-sync, where the two newest upstream PPU tests were known gaps), because the crate
is a host shell off-device.

**v2.0.5 "Landfall" — the iOS re-port onto Timebase.** The v2.0.x "Harbor" train
re-ports the frozen v1.9.9 line onto the **v2.0.0 "Timebase"** core (the iOS analogue
of the Android v2.0.1 re-port), so the current line is now **v2.0.5** and AccuracyCoin
is back to **141/141** (the shipped v2.0.3 default, unchanged by this host-only cut).
v2.0.5 surfaces + localizes (**EN + ES**) the **pre-Timebase movie warning**: loading a
pre-v2.0.0 `.rnm` still replays its input, but a non-blocking notice — drained via
`EmulatorCore.drainWarnings()` → `NesController.drainWarningCodes()`, mapping
`HostWarning.preTimebaseMovie` — tells the user byte-exact framebuffer / audio
reproduction is not guaranteed across the ADR-0028 epoch change (the iOS analogue of the
Android v2.0.4 warning; the notice uses a distinct channel from the error alert). The
iOS `MARKETING_VERSION` is realigned from the frozen v1.9.x default to `2.0.5`. iOS
finalization continues **v2.0.6 → v2.0.8** (feature parity → polish + the **Xcode 26 /
iOS 26 SDK** App Store submission floor → the iOS RC); full phasing in
`to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md`.

**v2.0.6 "Parity" — feature parity + opt-in crash reporting.** v2.0.6 closes the v1.9.9
iOS-applicable deferral for an **opt-in crash-reporting surface** (`CrashReporter.swift`,
the iOS analogue of the Android v1.8.8 `CrashReporter`): **off by default**, **Settings →
Diagnostics** installs an uncaught-`NSException` handler that writes **local** crash logs
(app version / device / OS / exception / call stack, newest 10 kept) the user can view +
copy in-app — **nothing is uploaded**, so the "Data Not Collected" label + the
`PrivacyInfo.xcprivacy` are unchanged (EN + ES). Pure-Swift signal-based traps stay a
documented maintainer / third-party-SDK option (the same posture as Android's Crashlytics
note), and the handler re-checks the live opt-in at crash time so opting out stops new logs
at once. v2.0.6 also re-verifies the v1.9.x host features (Game Center, CloudKit save sync,
MFi controllers, capture / PiP, accessibility) against the **unchanged v2.0.0 bridge
surface** — recorded in `docs/ios-v2.0.6-readiness.md`.

**v2.0.7 "Trim" — polish + the App Store submission floor.** From **2026-04-28** Apple
requires every App Store Connect upload to be built with the **iOS 26 SDK (Xcode 26)** —
the *build* SDK, not the minimum OS. The tag-gated `ios.yml` now selects the newest
`Xcode_26*.app` on the runner before building the xcframework (non-breaking: it warns and
falls back on an older image, so the compile job still runs). v2.0.7 also **reconciles the
deployment target `iOS 15.0 → 17.0`**: the shell already uses `NavigationStack` (iOS 16) +
`.topBarTrailing` (iOS 17, unguarded at 12+ sites), so the prior 15.0 was never buildable —
17.0 matches the real API floor (guard those APIs first to target lower). `PrivacyInfo`
`.xcprivacy` was **re-audited** against the v2.0.6 crash reporter and needs no change (no
new data type / required-reason API — it is local-only, backup-excluded, off by default,
and `UserDefaults` is already declared). On-device Metal/ProMotion profiling + the Xcode-26
archive are the v2.0.9 carryover. See `docs/ios-v2.0.7-readiness.md`.

**v2.0.8 "Harborlight" — the iOS release candidate.** The final release of the iOS
finalization window stages the App Store scaffolding for v2.1.0: version-controlled **App
Store Connect listing metadata** under `fastlane/metadata/ios/{en-US,es-ES}/` (name /
subtitle / promotional text / keywords / description / release notes / URLs + copyright),
mirroring the Android `fastlane/metadata/android/` tree but namespaced under `ios/` so
`deliver` (iOS) and `supply` (Android) never collide; a **dormant App Store `release`
lane** in `fastlane/Fastfile` that stages the build + listing and **does not submit**
(`submit_for_review: false`, `automatic_release: false`) and is **not** wired into
`ios.yml` — the interim channel stays **TestFlight** (the `beta` lane) until v2.1.0; and an
**App-Review §4.7 self-audit** (`docs/ios-v2.0.8-readiness.md`). Screenshots + the app
preview, real-cert signing, the listing upload, and the App-Review submission are the
maintainer / v2.0.9 / v2.1.0 closeout. This closes the v2.0.5→v2.0.8 iOS finalization
window; **v2.0.9** is the joint (Android + iOS) on-device readiness pass, and **v2.1.0**
the joint store launch. *(History: that launch never happened; see the current
status above. The fastlane scaffolding remains, dormant.)*

**FDS, NSF and the Vs. `DualSystem` cabinet (v2.9.7 "Tandem").** Until v2.9.7 the
mobile bridge was iNES / NES 2.0-only. It now tells a buffer apart by its magic, as
the desktop does, and loads **FDS disk images (`.fds`)**, **NSF / NSFe music files**
and **Vs. `DualSystem` cabinets** (two consoles, two screens). The FDS BIOS
(`disksys.rom`, 8 KiB) is Nintendo IP the app never ships: the first disk that
fails with `MobileError.missingFdsBios` raises a file picker, the chosen BIOS is
kept in Application Support (`FdsBiosStore`), and every later disk boots with it.
The pill menu gains **Flip disk** (FDS), **Next track** (NSF), **Swap screen**
(cabinet: the device shows one cabinet screen at a time) and **Insert coin** (any
Vs. machine). A cabinet's save state is the core's `RVSD` container; netplay,
movies and Lua are refused on a cabinet. FDS disk writes are exposed by the bridge
(`diskImageBytes` / `diskIsDirty`) but **not yet persisted by the app**. Settings >
Controls also gains **Cancel opposite directions** (default on). **All of this
Swift compiles** (the iOS workflow's Simulator build, most recently run
37242884341 on `c0b04195`) **and none of it has run on a device**; the checks are
rows T5-T7, T10 and T12 of `docs/mobile-v2.9.3-run-sheet.md`.

**The game database's load-time corrections (v2.9.8).** The shared bridge now
corrects a cartridge before the core parses it (`rustynes_gamedb::correct_rom`:
mapper, submapper and region fixes, the NES 2.0 guard, the PAL / Dendy
promotion of an iNES 1.0 image) and corrects the built console
(`rustynes_gamedb::correct_console`: the hardwired-mirroring override, on both
consoles of a cabinet), through the same two functions as the desktop, the
browser and the libretro core (`docs/frontend.md`, "One correction path for
every platform"). Until v2.9.8 iOS, like Android, ran every image with its dump's
header. The change is in the Rust bridge only (`build_console`), so no Swift
changes; only the vendored table is read.

**Still NOT on the iOS bridge.** The native desktop **20-band EQ** (the mobile
audio-depth panel ships a 5-band EQ) and the debugger's **`.dbg` ca65/cc65 source
maps** remain deferred to the post-v2.0.0 mobile re-port.

**Maintainer-manual carryovers** (cannot be CI-self-certified, mirroring the
Android line): an Apple Developer Program account + bundle ID; the signing secrets
(`fastlane match` private repo + the App Store Connect API key); app-icon /
launch-screen art; and the **on-device TestFlight verification** (ROM import,
save-states / rewind, MFi controller, audio + interruptions across iOS 16/17/18,
no crashes on 5-10 test ROMs, ProMotion pacing, an accurate privacy label). See
`ios/README.md` for the local-build + verification checklist.

# RustyNES for iOS / iPadOS (v1.9.0 "Sunrise")

The native SwiftUI host for RustyNES, the cycle-accurate pure-Rust NES emulator.
This is the **interim TestFlight foundation** for the iOS/iPadOS app: an
additive, off-by-default platform shell over the byte-identical Rust core. The
Rust core, the shared `rustynes-mobile` UniFFI bridge, and the `rustynes-ios`
Metal/audio glue crate are unchanged by this app.

The App Store launch is deferred until after RustyNES v2.0.0 "Timebase"; v1.9.0
ships only as a TestFlight build for on-device validation.

## Architecture

The app drives the emulator through two layers:

- **`NesController`** (UniFFI-generated, `Generated/RustyNESCore.swift`) is the
  typed control surface over the Rust core: load a ROM, set the per-port
  controller mask, run a frame, drain audio, save/restore state. It is generated
  from the Rust `rustynes-mobile` crate's `#[uniffi::export]` surface.
- **`rustynes_ios_*`** (the hand-written C glue in `rustynes_ios.h`, imported via
  the bridging header) is the hot path the typed surface cannot express: the
  wgpu/Metal renderer lifecycle and the CoreAudio output sink.

Both link out of one self-contained static archive (`librustynes_ios.a`, which
bundles `rustynes-mobile` + `rustynes-core`), packaged as
`RustyNESFFI.xcframework`.

### Swift source map

| File | Role |
| --- | --- |
| `RustyNESApp.swift` | App entry point; pauses/resumes on `ScenePhase` |
| `ContentView.swift` | Library grid and player navigation |
| `GameView.swift` | In-game screen, top bar, save-state sheet |
| `MetalGameView.swift` | Hosts the `MTKView` layer + the `CADisplayLink` loop |
| `EmulatorCore.swift` | Wraps `NesController` + the gfx/audio FFI handles |
| `AppModel.swift` | App-wide state + input fan-in |
| `AudioSession.swift` | `AVAudioSession` config + interruption handling |
| `TouchControlsOverlay.swift` | On-screen D-pad / A / B / Select / Start |
| `GameControllerManager.swift` | Hardware-gamepad mapping |
| `ROMLibrary.swift` | User-ROM import + sandbox storage keyed by SHA-256 |
| `SaveStateManager.swift` | Per-ROM `.rns` save-state slots |
| `RomIdentity.swift` | SHA-256 of ROM bytes (the stable key) |
| `NesButtons.swift` | The NES controller bitmask constants |
| `SettingsView.swift` | Filter picker, mute, about |

### Rendering and pacing

The `MTKView` is just the `CAMetalLayer` host; wgpu owns the drawable and
presents. We pass the view pointer to `rustynes_ios_gfx_init`, set
`isPaused = true` on the `MTKView` (no `MTKViewDelegate` drawing), and drive the
frame loop from a `CADisplayLink` requesting 60-120 Hz. Each tick runs one
`NesController.runFrame()`, hands the RGBA framebuffer to `rustynes_ios_gfx_render`,
and pushes drained mono audio to `rustynes_ios_audio_push`. The emulator advances
at the console rate; the audio sink's dynamic rate control absorbs the
~60.0988 Hz to 120 Hz beat. `CADisableMinimumFrameDurationOnPhone` unlocks 120 Hz
on ProMotion iPhones.

### Controller bit order

The NES button bitmask (`NesButtons.swift`) matches the core's `Buttons` bitflag
exactly (`crates/rustynes-core/src/controller.rs`), LSB-first in the 4016/4017
shift-out order: `A = 0x01`, `B = 0x02`, `Select = 0x04`, `Start = 0x08`,
`Up = 0x10`, `Down = 0x20`, `Left = 0x40`, `Right = 0x80`. The touch overlay and
the gamepad mapper both build this mask and feed it to
`NesController.setButtons(port:mask:)`.

## Building locally

Prerequisites (macOS with Xcode):

- Xcode 15 or newer.
- The Rust toolchain pinned by `rust-toolchain.toml` (1.96).
- The iOS Rust targets (the build script adds them):
  `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios`.
- XcodeGen: `brew install xcodegen`.

Build the FFI + the project, then open it in Xcode:

```bash
./scripts/build-ios-xcframework.sh
open ios/RustyNES.xcodeproj
```

The script builds the per-arch static libraries, lipos the simulator slices,
generates the Swift bindings (`Generated/RustyNESCore.swift`), assembles
`ios/RustyNESFFI.xcframework`, and runs `xcodegen generate`. Select the `RustyNES`
scheme and a simulator or device, then Run.

To ship a TestFlight build (requires the signing secrets below):

```bash
fastlane ios beta
```

## App Store compliance posture (Guideline 4.7)

RustyNES is a general-purpose NES emulator that runs **only ROM files the user
supplies and owns**:

- ROMs are imported through the document picker / Files / share sheet
  (`UIDocumentPicker` / `.fileImporter` / `onOpenURL`) and copied into the app
  sandbox keyed by SHA-256.
- **No game content is bundled** in the app, and nothing is downloaded.
- The app collects no data, does no tracking, and contacts no servers
  (`PrivacyInfo.xcprivacy` declares empty collection / tracking arrays).
- The user is responsible for the legality of the ROMs they import; the app
  facilitates only personally-owned content.

## On-device verification checklist (TestFlight)

These require an Apple device and cannot be self-certified by CI on Linux. An iOS
developer validates them before the build is promoted:

- [ ] App launches on an iPhone and an iPad (iOS 15+).
- [ ] Import a `.nes` ROM from Files; it appears in the library.
- [ ] Open a game; the picture renders at the correct aspect with letterboxing.
- [ ] Audio plays; pausing (Home / lock) silences it and returning resumes it.
- [ ] On-screen controls drive the game (D-pad diagonals, simultaneous A + B).
- [ ] A hardware controller (MFi / Xbox / DualSense) drives player 1.
- [ ] Save a state to a slot, reset, then load it back.
- [ ] Rotate the device and use Stage Manager / Split View on iPad; the drawable
      resizes without a crash.
- [ ] ProMotion devices reach 120 Hz with no judder; non-ProMotion hold 60 Hz.
- [ ] A phone-call interruption pauses and cleanly resumes.

## Maintainer carryovers

The following are intentionally left for the maintainer / an on-device build (the
documented TestFlight carryovers; this work was authored on Linux without Xcode):

- An Apple Developer Program account and an App Store Connect app record for
  `com.doublegate.rustynes`.
- Signing via fastlane match: populate the match repo once
  (`fastlane match appstore`) and set the CI secrets `ASC_KEY_ID`,
  `ASC_ISSUER_ID`, `ASC_KEY_CONTENT`, `MATCH_GIT_URL`, `MATCH_PASSWORD`, and
  `MATCH_GIT_BASIC_AUTHORIZATION`.
- App icons: `Assets.xcassets/AppIcon.appiconset` is a placeholder (no binary
  images). Add the 1024x1024 marketing icon (and any per-size assets) as a design
  task.
- On-device validation of the checklist above, then the first TestFlight
  distribution.
- The `AVAudioSession` route/interruption behaviour and the wgpu drawable resize
  path need real-hardware confirmation.

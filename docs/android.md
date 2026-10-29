# RustyNES on Android

> **Current distribution (checked 2026-10-06, v3.0.0).** The store-launch
> targets below (v2.1.0, later v2.2.0 and v2.3.0) were retired and **no store
> launch has happened**. Android ships as **GitHub-sideload** APKs. A free store
> listing (Google Play, F-Droid) waits until after the v3.x hardware-verification
> release, which includes the first device run
> ([line plan](../to-dos/plans/v2.9.4-to-v3.0.0-line-plan.md), ADR 0043). The
> paragraph that follows is the 2026-06-23 plan, kept as history.
>
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
                │ generated Kotlin bindings (UniFFI)              │ JNI (surface)
        ┌───────▼─────────────────────┐                  ┌────────▼───────────────────┐
        │ rustynes-mobile (shared)    │                  │ rustynes-android            │
        │  typed control surface:     │                  │  hot glue UniFFI can't do:  │
        │  load_rom / run_frame /     │                  │  ANativeWindow → wgpu,      │
        │  set_button / save_state    │                  │  (audio: Kotlin AudioTrack) │
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
- **Input never waits for the emulator (v2.7.4, audit MOB-01).** `set_buttons` /
  `set_button` write per-port atomics outside the controller's lock, and each
  frame latches them into the core just before it runs — the point at which a
  lock-waiting call used to land, so emulated input timing is unchanged. Before
  v2.7.4 a touch event on the UI thread waited out a whole frame (or a netplay
  rollback, a Lua callback) for the same mutex `run_frame` holds. This is shared
  with iOS: it lives in `rustynes-mobile`.
- **ROM buffers are capped at 16 MiB at the bridge (v2.7.4, audit MOB-02)**,
  compressed or not. Only zip entries were bounded before; a plain file of any
  size reached the core, which copies it whole and then again into PRG and CHR.
- **ROM images get the game database's load-time corrections (v2.9.8).** The
  bridge's `build_console` runs `rustynes_gamedb::correct_rom` on a cartridge
  before the core parses it (mapper, submapper and region fixes, the NES 2.0
  guard, the PAL / Dendy promotion of an iNES 1.0 image) and
  `rustynes_gamedb::correct_console` on the built console (the hardwired
  mirroring override; both consoles of a Vs. cabinet). These are the functions
  the desktop, the browser and the libretro core call (`docs/frontend.md`, "One
  correction path for every platform"). Until v2.9.8 the bridge applied none of
  them, so a ROM such as *Seicross* that needs its database row hung on a
  phone. Only the vendored table is read. Shared with iOS: it lives in
  `rustynes-mobile`.
- **Movies and netplay own the timeline (v2.9.9, NF-18).** Both carry controller
  input only, so the bridge refuses whatever would change the machine outside
  that stream: while a movie records or plays, or a netplay session runs (or has
  ended in an error not yet left), `reset` and `power_cycle` are no-ops and
  `load_state` returns `SaveState`; a movie cannot start during netplay
  (`movie_record_*` no-op, `movie_play` returns `Movie`) and netplay cannot start
  during a movie (`np_*` return `Netplay`). Existing error variants, so the
  generated bindings are unchanged; the hosts should grey the controls to match.
  The desktop's rule (`docs/frontend.md`, "Session lockout"). Shared with iOS.
  **v3.5.0:** an FDS disk swap (`set_disk_side`) and a Vs. coin (`insert_coin`)
  are refused too, during a netplay session (`Netplay`) and during playback
  (`Movie`), and allowed during a recording, which records them (`.rnm` format
  7). Until v3.5.0 the bridge asked nothing before either, so a coin or a swap
  happened on one peer only and the session desynced. Both now return a
  `Result`, so the generated Kotlin `setDiskSide` / `insertCoin` throw
  `MobileException` and the Swift ones `throws`; the Android media buttons show
  the reason in a toast, the iOS ones in the warning alert.
- **Per-game stores are keyed by the core's ROM identity (v2.9.9, NF-21).**
  `NesController.romIdentity()` returns `Nes::rom_sha256` as lowercase hex: the
  bytes after the 16-byte iNES header, of the UNPACKED image (the whole image for
  FDS, NSF and UNIF), the key the desktop gives slots, `.sav` and cheats.
  `romIdentityOfFile(bytes)` computes the same without building a console (a
  library import needs no FDS BIOS). Until v2.9.9 the app keyed everything by
  `sha256Hex` of the file as read, header and `.zip` container included, so two
  dumps differing only in their header did not share saves and a re-zipped ROM
  lost them. `prepareRom` and the folder import now run
  `RomKeyMigration.migrate(filesDir, legacy, identity)` (`Persistence.kt`) before
  touching any store. It moves, once, `battery/<k>.sav`, every file in
  `states/<k>/` (auto-resume, slots, thumbnails), `ra-progress/<k>.rap`,
  `boxart/<k>.png`, the `game_config.json` entry and the `library.json` entry. A
  store moves only into an EMPTY new key; a file is written atomically under the
  new key, read back and compared before the old copy is deleted; a key already
  taken is never overwritten (the old copy then stays as an orphan). A store
  counts as moved only once the old copy is deleted; when the delete fails, the
  next open (every open runs the migration, the old key recomputed from the
  file) finds the identical copy under the new key and finishes the move. Equal keys
  (an FDS disk, an NSF or a UNIF board opened unzipped) do nothing.
  `RomKeyMigrationTest` pins the rules on the JVM. **Not migrated:** Play Games
  cloud snapshots (`rns.<k>.<slot>`, remote); the next push writes the slot
  under the new key. The RA progress sidecar is now read from the same key it is
  written to (`emulator.romSha`), where it used to read the whole-file hash.
  Device rows: M1-M4 of `docs/mobile-v2.9.3-run-sheet.md`.
- Save-states use the **platform-independent `.rns` format**, so a state saved on
  desktop loads on Android and a `.rnm` TAS replays bit-identically — desktop⇄
  Android cross-play stays valid.
- **No commercial ROMs are ever bundled.** ROM bytes come exclusively from the
  Storage Access Framework document picker (no path, raw bytes → `Nes::from_rom`).
  The **`foss`** build declares no storage permission and collects nothing; its
  network permissions are `INTERNET` and, from v3.6.0, `ACCESS_LOCAL_NETWORK`
  (asked for only when LAN netplay starts; see "Android 17" below);
  the **`play`** build adds only the optional free Google Play services (Play Games,
  Cast, Play Integrity, in-app update, cloud save), which use the network and
  Google-account data — with no ads, no tracking, and no billing.

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
# Cross-compile both crates for the shipped ABIs. `release-mobile` is the
# workspace `release` profile with `panic = "unwind"` (v2.7.4, audit MOB-03):
# `release` aborts on panic, which made every panic guard on the mobile FFI
# dead code. It costs about 18% native size on arm64 and no measurable frame
# time (v2.7.4, `cargo ndk`, the same tree built both ways):
#   librustynes_android.so 11.64 MB -> 13.74 MB, librustynes_mobile.so
#   6.21 MB -> 7.32 MB; `nes_run_frame_nestest_fast` 3.947 / 3.936 ms (abort)
#   vs 3.949 / 3.940 ms (unwind), interleaved on one host.
cargo ndk -t arm64-v8a -t x86_64 --platform 26 \
  build --profile release-mobile -p rustynes-mobile -p rustynes-android

# Generate the Kotlin bindings from the built arm64 cdylib (API is
# target-independent, so any built library is a valid source of truth):
cargo run -p rustynes-mobile --bin uniffi-bindgen -- \
  generate --library target/aarch64-linux-android/release-mobile/librustynes_mobile.so \
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

`minSdk 26` (AAudio floor), `targetSdk 37` (Android 17, from v3.6.0; this line
read 35 for several releases after the build moved to 36), `compileSdk 37`. Ship
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

## F-Droid anti-features (v3.4.0 decision)

F-Droid labels an app with an anti-feature when it relies on something F-Droid
users may want to avoid. Two of them concern network services, defined on
[f-droid.org/docs/Anti-Features](https://f-droid.org/docs/Anti-Features/) and,
for the 2024 split between them,
[This Week in F-Droid, 2024-07-25](https://f-droid.org/2024/07/25/twif.html)
(both read 2026-10-16):

- **`NonFreeNet`**: "applied to apps that promote or depend entirely on a
  proprietary network service".
- **`TetheredNet`**: apps relying on services that cannot be changed. The two
  are "totally orthogonal"; a non-free but self-hostable server is
  `NonFreeNet` only.

What the `foss` build (the F-Droid artifact) can contact, each only on the
user's action:

| Service | Free server software? | Changeable by the user? | Verdict |
| --- | --- | --- | --- |
| Netplay lobby + TURN | Yes: the lobby is our GPL code (`examples/signaling_server.rs`, `services/hosted/`), the relay is any standard TURN server (the `deploy/` bundle ships coturn) | Yes: signaling URL, STUN list and TURN trio are Settings fields, and F-Droid builds without `RUSTYNES_HOSTED_BASE`, so the build has no built-in service at all | Neither |
| RetroAchievements | The site's code (RAWeb) is GPL-3.0, but RetroAchievements' [terms](https://retroachievements.org/terms) keep the achievement definitions (trigger logic) proprietary, and those definitions are what the app fetches and runs | No: the app talks to retroachievements.org and exposes no host setting | `NonFreeNet` and `TetheredNet` |
| Box art (libretro-thumbnails on GitHub; TheGamesDB with the user's own key) | Community image collection / third-party database | No | Not declared: a convenience lookup, neither promoted nor depended on; recorded so F-Droid's reviewers can decide |

**Decision:** declare **`NonFreeNet`** and **`TetheredNet`** on the F-Droid
build, both for the optional RetroAchievements integration. RustyNES does not
"depend entirely" on RetroAchievements (it is off until the user logs in, and
every feature works without it), but the app does promote it (a login screen
and achievement UI), and its content is proprietary; when the definition can be
read either way, the label is the honest default for users who avoid such
services. Netplay needs neither label: everything it uses is free software and
user-replaceable, and the F-Droid build contains no hosted endpoint.

**Where it is declared:** F-Droid's own metadata for the app (the
`AntiFeatures:` field of its file in the `fdroiddata` repository), written when
the app is submitted; this repository has no F-Droid metadata file to carry it.
The submission is the store-listing step deferred by ADR 0035. F-Droid's
reviewers make the final call; this records what RustyNES will ask for and why.

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
| **C** Audio (`AudioTrack` sink; blocking-write pacing until v3.6.0, then non-blocking writes paced by `AudioPacer`) | **Done** — on-device (the v3.6.0 pacing: emulator only, run-sheet L4) |
| **D** Input (touch overlay + hardware gamepad → one late-latched mask) | **Done** — touch on-device |
| **E** Save-states / SRAM / auto-resume / persistable ROM library | **Done** — save/load on-device |
| **F** QoL (pause / fast-forward / mute) + responsive/foldable/immersive UI | **Done** — on-device |
| **G** Play packaging (release AAB, signing config, distribution) | Release AAB verified (R8, arm64-only); signing is maintainer-manual |
| **M** Freemium $2.99 unlock + 10-min demo gating | Removed entirely (ADR 0035) — RustyNES is permanently open-source and income-free; every feature ships unconditionally available |
| **B** wgpu `SurfaceView` + surface-loss lifecycle | **Next increment** (Bitmap blit ships now) |
| **F** shaders / palettes / per-game DB / TAS UI | **Next increment** (depends on B for shaders) |

**FDS, NSF, the Vs. `DualSystem` cabinet and the SOCD switch (v2.9.7 "Tandem").**
The bridge (`rustynes-mobile`) tells a buffer apart by its leading magic, as the
desktop does: a cartridge, an **FDS disk** (`FDS\x1A` or the raw
`\x01*NINTENDO-HVC*` form), an **NSF / NSFe** file, or a **Vs. `DualSystem`
cabinet** (NES 2.0 Vs. type, or the Vs. database's SHA-256 record). The FDS BIOS
(`disksys.rom`, 8 KiB) is supplied by the host, never shipped:
`NesController.newWithFdsBios(rom, bios, rate)` / `setFdsBios(bios)`, with a disk
that has none failing as `MobileException.MissingFdsBios`. The app answers that
exception with a document picker, stores the BIOS once in
`filesDir/fds/disksys.rom` (`FdsBios` in `Persistence.kt`), and retries the disk.
The control bar gains **Disk A/B** (next side), **Track n/m** with **<** (NSF),
**Screen: left/right** (a cabinet shows one of its two screens at a time) and
**Coin** (any Vs. machine). A cabinet's save state is the core's `RVSD`
container; netplay, movies and Lua are refused on it with
`MobileException.DualSystem`. **From v3.2.0 a disk game's own save persists**:
when the game has written to its disk (`diskIsDirty`), the game loop (once a
second) and every session-ending path (background, ROM switch, close) write
`diskImageBytes` atomically (`AtomicFile`) to
`filesDir/fds-saves/<identity>.fds.sav`, the desktop's layout. The flag is
cleared BEFORE the image is copied, so a disk write landing during the save sets
it again and the next flush catches it; a failed file write is retried by the
next once-a-second check. Each key has one owner (`SaveOwners`): the key moves
to the incoming controller on the main thread, with no I/O, in the same step
that swaps the running controller, so there is no moment in which the running
disk cannot be saved. The outgoing controller keeps the right to write its own
key for its final flush only until the new owner first writes it, so no flush
can put one game's disk in another's file, or an older image over a newer one
when the same disk is reopened (`SaveOwnersTest`). The next load builds from the pristine image and
calls `bootSavedDisk`, which boots the saved copy under the PRISTINE disk's
identity, so the save, the states, RA progress and overrides keep one key (loading
the saved file as the ROM would change it at the first disk write). A bad or
foreign file is logged and the pristine disk plays. Before v3.2.0 the disk image
was not written back and a disk game's save was lost on close. Settings
gains **Cancel opposite directions** (neutral SOCD), default on, the desktop's
`[input] allow_opposing_directions = false`; off, `socdNeutral(mask, enabled =
false)` passes every mask through (`SocdTest` covers both). The device checks are
rows T1-T4, T8, T9 and T11 of `docs/mobile-v2.9.3-run-sheet.md`, all NOT RUN.

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

**Android 17 (v3.6.0, `T-ANDROID-API37`, `T-ANDROID-AUDIO-NONBLOCKING`).**
The app targets API 37. Three behaviours follow:

- **Local network permission.** Android 17 blocks traffic to and from local
  addresses for an app that targets 37 unless it holds the runtime permission
  `ACCESS_LOCAL_NETWORK` (the `NEARBY_DEVICES` group: UDP fails with `EPERM`,
  TCP times out). LAN / direct-IP **Host** and **Join** therefore go through
  `withLocalNetwork`, which asks `LocalNetworkAccess.decide(SDK_INT,
  targetSdkVersion, granted)`: below API 37, or for a lower target, it never
  asks (the permission does not exist there, and asking would be answered
  "denied"); otherwise it shows the system prompt and starts LAN play only on
  a grant. A refusal is shown in the netplay sheet, since the status line is
  drawn only on the idle screen. Room-code play is not asked for: `NatConnect`
  exchanges STUN-reflexive public addresses or a TURN relay, never a host
  candidate. A user-configured relay on a LAN address would be local traffic;
  that case is not handled. `LocalNetworkAccessTest` pins the decision, the
  manifest declaration, `targetSdk` and the wiring; run-sheet rows L1-L3.
- **Background audio.** Android 17 silences playback and refuses audio focus
  for an app with no visible activity and no while-in-use foreground service,
  without an exception. RustyNES plays only while started (on screen or in
  picture-in-picture); v2.7.4 already paused in `onStop`, and the loop now
  also refuses to start the track or ask for focus while `EmulatorHandle.visible`
  is false (`audioMayPlay`). No foreground service is added. Row L5.
- **Non-blocking audio.** `AudioPlayer.writeBytes` writes with
  `WRITE_NON_BLOCKING` through the `ByteBuffer` overload (the one valid for a
  `PCM_FLOAT` track), keeps what the track did not take (`AudioQueue`, bounded,
  oldest dropped first) and the loop sleeps `AudioPacer.delayNanos`: until the
  track holds about two NES frames (`targetFrames`), never more than two frames
  at once, and on the wall clock when the frame wrote no audio or the play head
  has stopped. The blocking write did the same pacing implicitly, on a shared
  `Dispatchers.Default` worker and uncancellably. `AudioPacingTest` runs the
  loop against a simulated DAC with millisecond truncation and 2 ms of
  scheduler jitter (no underrun after warm-up); the device drift check is L4.

**Deferred to a follow-up mobile point release** (per the locked MVP):
RetroAchievements (Compose login UI over rcheevos) and Lua scripting. The egui
debugger stays an optional sideload power-user overlay.

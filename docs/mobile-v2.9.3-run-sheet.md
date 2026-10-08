# v2.9.3 "Handset" — the mobile run sheet

> **The device run moved after v3.0.0** (maintainer, 2026-09-29). v2.9.3
> shipped with this sheet prepared and its emulator column filled; the device
> columns are still empty. When the run happens, rebuild the APKs from that
> day's `main` and record the new checksums here first, as "The build" below
> requires.

One sheet for the maintainer's device run. It carries every row of the
[v2.7.4 checklist](mobile-v2.7.4-device-checklist.md) (A1-A12, I1-I15) and its
v2.9.2 additions (A13-A15, I16-I19) unchanged, plus rows for what the
dependency refresh (#570) changed on mobile. The step and expected result for
each carried row are in that checklist; this sheet adds the build to install,
what the Android emulator has already shown, and a result column per device.

Record each row as **PASS / FAIL / NOT RUN**, with a note. A FAIL is fixed
before the release (red-first where a host test can reach it) or recorded as a
known issue.

## What is already verified, so it is not repeated

- The Rust bridge, the Android JVM unit tests (now a CI gate), both Android
  flavours compiling, and the `cargo ndk` cross-build.
- `scripts/ios-host-typecheck.sh`: the iOS-only Rust (`audio.rs`, `ffi.rs`,
  and since v2.9.3 `gfx_metal.rs`) compiles on Linux. **This is a compile
  check, not a run**: no Metal surface has been created anywhere.
- The RetroAchievements struct layout on a 32-bit `time_t` target (i686 Linux,
  run), and on armeabi-v7a (compiled only).

**Compiled, not run.** Since v2.9.8 (#578), the release workflow's iOS job builds
the app for the iOS Simulator, unsigned, on every release, so B1's compile half
is checked there. Nothing has run on a device: B1's run half and every other row
below still need one.

## Build

| # | Step | Expect |
| --- | --- | --- |
| B1 | `scripts/build-ios-xcframework.sh`, then build the Xcode project | Builds |
| B2 | Install the Android build below (`adb install -r <apk>`), or `cd android && ./gradlew :app:installFossDebug` | Installs and opens |

**Android build for this run:** see "The build" below. Check the checksum
before installing, so the result belongs to a known build.

## The build

| | |
| --- | --- |
| APK | `RustyNES-v2.9.3-pre-foss-debug.apk` (the `fossDebug` variant, application id `com.doublegate.rustynes.debug`, ABIs arm64-v8a and x86_64) |
| Built from | `1b4c5885` on `feat/v2.9.3-device-checklist` (`main` at `d8b02d2f` plus this release's commits), with a clean tree |
| SHA-256 | `01fdc7e8b65b93d4e68f9b6c576baf0e44394cf0b7d0886e9a03bc18d42a8f3a` |
| Size | 67,310,731 bytes |

**For row D5 only**, a 32-bit build: `RustyNES-v2.9.3-pre-foss-debug-armeabi-v7a.apk`,
built with `cd android && ./gradlew :app:assembleFossDebug
-PrustynesAbis=armeabi-v7a`. It carries only `lib/armeabi-v7a/` (a 32-bit ARM
ELF), so it installs on a 32-bit device, and on a 64-bit device that can still
run 32-bit apps it runs the 32-bit libraries. That is what D5 needs, since the
main APK would run the 64-bit ones.

| | |
| --- | --- |
| SHA-256 | `3590989ad131889735dd6a4d20bfba02615affddb31dbd832ac4ec9f730a1af4` |
| Size | 39,375,895 bytes |

Before recording D5, confirm the device ran the 32-bit library:
`adb shell dumpsys package com.doublegate.rustynes.debug | grep primaryCpuAbi`
should say `armeabi-v7a`. Many recent 64-bit phones can no longer run 32-bit
apps at all; on those, record D5 as NOT RUN.

A later commit to the Android app, the Rust core or the bridge makes this
build stale. Rebuild it and record the new checksum here before the device run.

## A correction to row A3

A3 says "Play with sound, press Home: sound stops at once". On a device that
supports picture-in-picture, Home with a running game **enters PiP instead**
(`onUserLeaveHint`), and the game keeps playing. That is row A6's expected
result, and it is by design. So A3 as written cannot pass on such a device,
and a FAIL there is the step's fault, not the app's. What A3 is for (AND-03:
audio stops once the app is really in the background) is tested by either of:

- turning the screen off while the game plays, or
- closing the PiP window.

With the game paused, Home does not enter PiP, which is what A4 checks.

## Rows added for the dependency refresh (#570)

PR #570 moved every mobile renderer to wgpu 30 (the frame is now presented
through the queue, with an explicit surface colour space). It also moved
RetroAchievements to rcheevos 12.5.0 with a grown user struct and
platform-sized `time_t` fields, and the Kotlin/Swift bindings to UniFFI
0.32.2. Any row that runs a game exercises the bindings; these rows target
the rest.

| # | Change | Step | Expect |
| --- | --- | --- | --- |
| D1 | wgpu 30, Android | GPU renderer on; play, rotate, background and foreground | The picture appears and returns every time (overlaps A7; record both) |
| D2 | wgpu 30, iOS | Open a game; rotate; background and foreground; change the video filter | The picture appears and returns; colours look as before |
| D3 | rcheevos 12.5.0, Android | Log in to RetroAchievements in Settings; open a game that has achievements | Login shows the user name and score; the game's achievement list loads |
| D4 | rcheevos 12.5.0, iOS | As D3 | As D3 |
| D5 | rcheevos, 32-bit Android | Install the **armeabi-v7a** APK (see The build) and confirm `primaryCpuAbi` is `armeabi-v7a`; then as D3 | As D3; this is the platform whose struct layout the `time_t` fix changed |

## Rows added for v2.9.7 "Tandem" (FDS, NSF, DualSystem, SOCD switch)

v2.9.7 gave the mobile bridge FDS disks, NSF files and the Vs. `DualSystem`
cabinet (plan items 6 and 7), and both apps a "Cancel opposite directions"
setting (item 8). The bridge half is under host test (`cargo test -p
rustynes-mobile`), and the Kotlin compiles and passes `SocdTest`. **None of the
Swift has been compiled** (no Swift toolchain on the Linux build host), and no
row below has been run on an emulator or a device. Every row is NOT RUN.

| # | Item | Platform | Step | Expect | Result |
| --- | --- | --- | --- | --- | --- |
| T1 | 6 | Android | With no BIOS stored, open an `.fds` disk image | A file picker opens; choose `disksys.rom` (8 KiB). The disk then boots to the BIOS screen and the game loads. Reopening another disk later asks nothing | NOT RUN |
| T2 | 6 | Android | Pick a file that is not 8 KiB at the T1 picker | Status reads "Not an FDS BIOS"; nothing is stored, and the next disk asks again | NOT RUN |
| T3 | 6 | Android | In a two-sided FDS game, answer its "insert side B" prompt with the control bar's **Disk** button | The label steps A, B, A; the game continues past the prompt | NOT RUN |
| T4 | 6 | Android | Open an `.nsf`; press **Track n/m >** and **<** | Music plays; the track changes and the label follows; it wraps at both ends | NOT RUN |
| T5 | 6 | iOS | As T1, opening the `.fds` through the ROM importer (it lists `.fds`, `.nsf` and `.nsfe` since #577's review round; Files' "Open in RustyNES" should offer them too). The BIOS picker is a `.fileImporter`; the first compile of `MobileError.missingFdsBios`, `NesController.newWithFdsBios`, and a second `.fileImporter` in `ContentView`) | As T1. If the ROM importer stops presenting, the two importers are competing; record it | NOT RUN |
| T6 | 6 | iOS | As T3 with the pill menu's **Flip disk** | As T3 | NOT RUN |
| T7 | 6 | iOS | As T4 with the pill menu's **Next track** | As T4 | NOT RUN |
| T8 | 7 | Android | Open a Vs. `DualSystem` dump (Vs. Tennis, Vs. Wrecking Crew, Vs. Balloon Fight or Vs. Mahjong; the Vs. database recognises iNES 1.0 dumps by SHA-256); press **Coin**, then **Screen: left**, then **Coin** on the right-hand screen | The game boots past its handshake; the button flips to the right-hand cabinet screen and back. A P3 / P4 pad drives the right-hand half. **Coin** credits the screen on show (acceptor 2 on the right) | NOT RUN |
| T9 | 7 | Android | On the T8 cabinet: save a state, play on, load it; then try Netplay | The state restores both halves; netplay refuses with "not available on a Vs. DualSystem cabinet" | NOT RUN |
| T10 | 7 | iOS | As T8 with **Insert coin** and **Swap screen** in the pill menu | As T8 | NOT RUN |
| T11 | 8 | Android | Settings > **Cancel opposite directions** off; hold Left and Right on the touch pad with two fingers, and Up on the pad plus Down on a hardware controller. Then, still holding two opposite touch directions, toggle the setting | Both pairs reach the game (a game that reacts to opposites shows it); switched back on, neither does. Toggling while held takes effect without lifting a finger. Default after a fresh install is on | NOT RUN |
| T12 | 8 | iOS | As T11 (Settings > Controls > **Cancel opposite directions**) | As T11 | NOT RUN |

FDS disk writes are not persisted by either app yet: the bridge exposes
`disk_image_bytes` / `disk_is_dirty` / `clear_disk_dirty`, but neither host
writes the image back. A game saved to disk loses the save when the app closes;
that is known, not a row to fail.

## Rows added for v2.9.9 (NF-21: the ROM-identity key migration)

Both apps now key per-game stores by the core's ROM identity
(`NesController.romIdentity()` / `romIdentityOfFile`: the bytes after the iNES
header, of the unpacked image) instead of the whole file's SHA-256, and move a
game's stores from the old key the first time it is opened. The bridge half is
under host test (`rom_identity_is_the_cores_and_ignores_the_header_and_the_zip`)
and the Android rules under `RomKeyMigrationTest`. **The Swift compiles but has
not run:** the iOS workflow built it for the Simulator on `c0b04195` (run
37242884341); only these device rows test what it does. Prepare each row by installing the PREVIOUS release (v2.9.8) first,
creating the saves under it, then installing this build over it (an upgrade, not
a fresh install).

| # | Platform | Step | Expect | Result |
| --- | --- | --- | --- | --- |
| M1 | Android | Under v2.9.8: open a battery cartridge (e.g. *Zelda*) from a `.zip`, save in game, save slot 1, set a per-game filter, favourite it in the library; background the app (auto-resume). Upgrade; open the same `.zip` | The in-game save, slot 1 (with its thumbnail), the auto-resume state and the filter are all there; the library shows ONE entry for the game, still a favourite. `filesDir/battery/` and `filesDir/states/` hold only the new key's files (`adb shell run-as com.doublegate.rustynes ls files/battery files/states`) | NOT RUN |
| M2 | Android | After M1, open the same game from the unzipped `.nes` | The same saves and slots (one key for both files); no second library entry | NOT RUN |
| M3 | Android | Under v2.9.8: import a ROM folder (library import). Upgrade; import the folder again | No duplicate entries; favourites and box art kept | NOT RUN |
| M4 | Android | RetroAchievements: under v2.9.8 make progress in a game; upgrade; open it | The progress sidecar is found (no "reset" of in-progress achievements) | NOT RUN |
| M5 | iOS | As M1: a battery game imported under v2.9.8, with an in-game save, slots 1-2, a per-game override; upgrade; open it from the library | Saves, slots, override and favourite all present; the library entry's ROM still opens; `RustyNES/roms/` holds the game under the new key only | NOT RUN |
| M6 | iOS | After M5, re-import the same file through the importer | No second library entry; the saves are still there | NOT RUN |
| M7 | iOS | Under v2.9.8 import an `.nsf` or an unzipped `.fds` (identity equals the whole-file key); upgrade; open it | Opens normally; nothing renamed (the keys are equal) | NOT RUN |

Swift changes these rows exercise (compiled in CI, not run on a device): `RomIdentity.identityHex`,
`RomKeyMigration` (`RomIdentity.swift`); `ROMLibrary.importROM` (keys by
identity, returns a pre-v2.9.9 entry for the same file instead of adding one) and
`ROMLibrary.rekey`; `GameOverrides.rekey`; `AppModel.openGame` /
`AppModel.migrateKey`; `EmulatorCore.romIdentity`. Not migrated on either
platform: cloud save-state records (Play Games snapshots, CloudKit), which the
next upload writes under the new key.

## Rows folded in from the v1.8.x Android checklist (v3.1.0, decision D25)

`to-dos/v1.8.x-on-device-verification.md` was the standing Android checklist
from v1.8.x to the v2.1.0 launch plan. v3.1.0 folds it in here and deletes it,
so one document carries the device runs. Only its rows that no row above
already covers are kept, rewritten for the current tree; its history is in git.
Dropped as covered: battery SRAM across a restart (A1), audio focus, PiP and
background (A3-A6), rotate and PiP return (A7), save-state slots (A10) and
multi-touch (A13). One row was dropped because it is now **wrong**: "the
picker offers iNES / NES 2.0 only". FDS and NSF shipped in v2.9.7 (T1-T7). Its
accuracy paragraph ("AccuracyCoin 139/141") is replaced by the rule it stated:
the device runs the host's byte-identical core, so AccuracyCoin (146/146 at
v3.1.0) is measured on the host and never on a phone, and a device that
diverges from the host's frames has a shell bug, not a core one.

| # | Platform | Step | Expect | Result |
| --- | --- | --- | --- | --- |
| G1 | Android | Import a `.nes` through the Storage Access Framework picker | It appears in the library and boots | NOT RUN |
| G2 | Android | Import a NES 2.0 ROM; open its ROM info | Mapper and region read correctly | NOT RUN |
| G3 | Android | Online, open the library | Box art auto-matches a grid entry (skip on `foss` offline) | NOT RUN |
| G4 | Android | Re-open a previously imported ROM from the library | It resumes cleanly | NOT RUN |
| G5 | Android | Hold rewind, then release | The picture runs backward smoothly, then forward | NOT RUN |
| G6 | Android | Load a `.rns` written by the desktop build of the SAME release, and write one back | Each loads on the other side | NOT RUN |
| G7 | Android | Load a `.rns` or `.rnm` from v3.0.1 or earlier | Refused with a clean message naming the reason (state layout, or emulation epoch for a movie), never a crash | NOT RUN |
| G8 | Android | Pair a hardware controller (Xbox, DualSense or MFi, Bluetooth or USB-OTG) | Binds to P1: South=A, West=B, Start, Select, D-pad | NOT RUN |
| G9 | Android | Pair two to four controllers | Each binds to its own port (the Four Score path) | NOT RUN |
| G10 | Android | Disconnect and reconnect a controller mid-game | No crash and no stuck input | NOT RUN |
| G11 | Android | Plug and unplug headphones while playing | Audio keeps playing; the audio thread does not wedge | NOT RUN |
| G12 | Android | *Super Mario Bros.*: World 1-1 through the first screen | Status bar, sprites and scroll correct, no flicker | NOT RUN |
| G13 | Android | *The Legend of Zelda*: title, overworld, then an in-game save and reload | Renders correctly; the save round-trips | NOT RUN |
| G14 | Android | From one save state, play the same inputs twice | Identical results (the determinism contract on the device) | NOT RUN |
| G15 | Android | On a tablet or unfolded foldable, then a phone | Two-pane and compact layouts both correct; the image letterboxes at any aspect | NOT RUN |
| G16 | Android | Fold and unfold mid-game; on Android TV, navigate with the D-pad | No restart on fold; every TV control reachable, and the app boots to its TV banner | NOT RUN |
| G17 | Android | `foss` flavor: inspect the merged manifest and Settings | No `AD_ID` permission, no Play Services metadata, no Billing, Play Games or Cast surface (ADR 0025) | NOT RUN |
| G18 | Android | `play` flavor (`./gradlew :app:installPlayDebug`): boot a ROM, then open Settings | Installs and plays like `foss`; its Play-Services surface (Play Games, cloud save, Cast, in-app updates) appears, and a device without Play Services still boots and plays | NOT RUN |

## Android

EMULATOR is what the `Pixel_8_API_34` emulator showed on this build, before
the device run. It is never a device result: the device column still needs
its own entry.

How the emulator column was produced (2026-09-28, Android 14, software GPU),
on the earlier build `2ed51d35` (SHA-256 `abb1fb4b...`). The build above adds
the review-thread fixes (mapper 28, NSF open bus, the APU gain guard, and
the header writer, which the apps never call; the rest is desktop-only or
docs). None of them touches the
lifecycle, audio-focus or renderer paths these rows exercise, so the column
was not re-run. That earlier build was installed over adb and driven with
`adb shell input`,
the app's own deep links (`--es com.doublegate.rustynes.extra.ACTION
open|resume`) and screenshots. Audio state came from `dumpsys audio`, CPU
from `top`, and the save files from `run-as`. A1 used
`scripts/mobile-battery-counter-rom.py`, which writes a ROM that increments
`$6000` on every power-on. On the emulator the on-screen pad is hidden,
because its built-in d-pad counts as a controller, so the menu was opened
with the pad's Guide button (`input gamepad keyevent KEYCODE_BUTTON_MODE`).
The menu's first button, **Close**, closes the game, not the menu: one early
A4 attempt was void for exactly that reason and was repeated.

Not a defect, for the record: StrictMode reports disk access on the main
thread at `MainActivity.kt:1859`. That is the debug-only `autoload.nes`
check (`BuildConfig.DEBUG`), which release builds never run.

| # | Finding | Emulator | Device |
| --- | --- | --- | --- |
| A1 | AND-09 | PASS: a battery test ROM that counts its own power-ons read 1, 2, 3 in `battery/<sha>.sav` across two `am force-stop` + reopen cycles | |
| A2 | AND-09 | NOT RUN | |
| A3 | AND-03 | PASS with the corrected step: screen off paused the AudioTrack within 1 s; waking resumed it. Home entered PiP instead (see the A3 correction) | |
| A4 | AND-03 | PASS: paused, then Home: no PiP, audio stayed paused; reopened with the game loaded and still paused (the menu reads Resume) | |
| A5 | AND-03 | NOT RUN (needs another media app) | |
| A6 | AND-03 | PASS: Home with the game running entered PiP (`mode=pinned`), and the AudioTrack stayed `started` | |
| A7 | AND-01 | PASS for rotate, sleep/wake and PiP-return, GPU renderer: 13 captures over three cycles all show the picture (36.9% of the screen in portrait, 54.9% in landscape, as the letterbox predicts). Netplay and HD-pack toggles NOT RUN | |
| A8 | AND-08 | NOT RUN to the row's one-minute step. A 30 s preliminary reading: GPU renderer paused, CPU 0.6-1.0%, against about 148% running (software GPU on the emulator); Bitmap renderer paused, 0.6% | |
| A9 | AND-04 | NOT RUN (needs a Drive account) | |
| A10 | AND-04 | PASS for function: Save wrote `states/<sha>/1.rns` and the slot shows its age; Load ran; Delete removed the file; no ANR. Stutter needs a device | |
| A11 | AND-02 | NOT RUN (stutter needs a human eye on a device) | |
| A12 | MOB-07 | NOT RUN (no crash reproduced) | |
| A13 | AUD-09 | NOT RUN (the emulator's d-pad counts as a controller, so the touch pad is hidden; multi-touch also needs a device) | |
| A14 | AUD-12 | NOT RUN | |
| A15 | AUD-11 | NOT RUN (needs an HD pack) | |
| D1 | #570 | PASS: covered by A7 (wgpu 30, Vulkan device created on the emulator's goldfish driver) | |
| D3 | #570 | NOT RUN (needs RetroAchievements credentials) | |
| D5 | #570 | NOT RUN (x86_64 emulator, which cannot run ARM code; the armeabi-v7a APK is built and checksummed for the device) | |

## iOS

| # | Finding | Device |
| --- | --- | --- |
| I1 | MOB-05 | |
| I2 | MOB-05 | |
| I3 | IOS-01 | |
| I4 | IOS-02 | |
| I5 | IOS-08 | |
| I6 | IOS-08 | |
| I7 | IOS-04 | |
| I8 | IOS-10 | |
| I9 | IOS-05 | |
| I10 | IOS-11 | |
| I11 | MOB-09 | |
| I12 | new (v2.7.4) | |
| I13 | IOS-06 | |
| I14 | MOB-09 | |
| I15 | MOB-07 | |
| I16 | AUD-10 | |
| I17 | AUD-14 | |
| I18 | AUD-14 | |
| I19 | AUD-11 | |
| D2 | #570 | |
| D4 | #570 | |

## Devices

| Platform | Device / OS | Build | Date | Notes |
| --- | --- | --- | --- | --- |
| Android | | | | |
| iOS | | | | |

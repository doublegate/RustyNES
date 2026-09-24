# v2.7.4 "Pocket" — mobile device checklist

The maintainer runs this on real hardware before the v2.7.4 PR merges (the
2026-09-22 decision recorded in the
[frontend ledger](audits/frontend-disposition.md)). It covers what automated
checks could not: every Swift change, and the Android lifecycle, audio and
renderer changes. Record a result per row (PASS / FAIL / NOT RUN, with a note);
a FAIL is fixed before merge or recorded in the release's known issues.

What IS already verified, so it is not repeated here: the Rust bridge (unit
tests, mutations), the Android JVM tests, both Android flavours compiling, the
iOS-only Rust type-checking on Linux (`scripts/ios-host-typecheck.sh`), and the
Android battery-save path on the emulator (a boot counter read 1, 2, 3 across
force-closed launches).

## Build first

| # | Step | Expect |
| --- | --- | --- |
| B1 | `scripts/build-ios-xcframework.sh`, then build the Xcode project | Builds. **This is the first compile of every Swift change in v2.7.4** |
| B2 | `cd android && ./gradlew :app:installFossDebug` on a device | Installs and opens |

## Android

A battery-backed game is needed for A1-A2 (any cartridge whose header sets the
battery bit: most RPGs and adventure games). Turn the GPU renderer on in
Settings for A6-A7.

| # | Finding | Step | Expect |
| --- | --- | --- | --- |
| A1 | AND-09 | Save in-game, force-stop the app from Settings, reopen the game | The in-game save is there |
| A2 | AND-09 | Save in-game, switch to another game, switch back | The save is there |
| A3 | AND-03 | Play with sound, press Home | Sound stops at once; reopening resumes the game where it was |
| A4 | AND-03 | Pause the game yourself, press Home, reopen | Still paused (the app does not unpause a game you paused) |
| A5 | AND-03 | Play music in another app, then open a game | The other app's music stops (focus taken); a phone call or navigation prompt silences or ducks the game |
| A6 | AND-03 | Enter picture-in-picture | The game keeps running and playing in the PiP window |
| A7 | AND-01 | With the GPU renderer on, rotate and background / foreground repeatedly, and toggle netplay or an HD pack | The picture always returns; never a black view |
| A8 | AND-08 | GPU renderer on, game paused, watch battery / CPU for a minute | No sustained CPU use while paused |
| A9 | AND-04 | Open a ROM from a cloud-backed folder (Drive) | No freeze or "not responding" while it loads |
| A10 | AND-04 | Save, load and delete slots in the save-state sheet | Works; no stutter |
| A11 | AND-02 | Play for a minute with the Bitmap renderer (the default) | No new stutter; the menus and HUD respond |
| A12 | MOB-07 | (only if a crash reproduces) | The app stays open, the game freezes, and a message says to reopen it or load a save state |

## iOS

| # | Finding | Step | Expect |
| --- | --- | --- | --- |
| I1 | MOB-05 | Save in-game in a battery game, swipe the app away, reopen | The save is there |
| I2 | MOB-05 | Save in-game, background the app, force-quit from the switcher | The save is there (the background save completed) |
| I3 | IOS-01 | Play a scrolling game on a ProMotion device for a minute | Smooth scrolling; no periodic hitch |
| I4 | IOS-02 | Play for ten minutes | No audio crackle, dropout or drift out of sync |
| I5 | IOS-08 | Unplug wired or Bluetooth headphones mid-game | The game pauses; opening and closing the menu resumes it (it used to stay frozen) |
| I6 | IOS-08 | A media-services reset has no reliable user-facing trigger; if one is available on the test setup, trigger it mid-game, otherwise record NOT RUN | Audio comes back within a couple of seconds, without reopening the game |
| I7 | IOS-04 | Save to a slot with iCloud sync on, background the app at once | The slot reaches iCloud (check from a second device) |
| I8 | IOS-10 | Two devices: save slot 1 on A, then an older save on B, sync both | The newer save wins on both; the older one does not overwrite it |
| I9 | IOS-05 | Slide a thumb off the bottom of the on-screen pad | The first swipe only reveals the home indicator; the app stays open |
| I10 | IOS-11 | Run a heavy filter (NTSC) until the device warms up | The filter switches off while hot and comes back when cool |
| I11 | MOB-09 | Join a netplay room by host name on a slow network | The interface stays responsive during the lookup |
| I12 | new | With a game running, open a `.nes` from the Files app | The new game appears and runs (it used to freeze) |
| I13 | IOS-06 | Play with a game controller, including turbo | Input as before |

## Record

| Platform | Device / OS | Result | Notes |
| --- | --- | --- | --- |
| Android | | | |
| iOS | | | |

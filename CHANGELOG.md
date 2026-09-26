# Changelog

This is the concise, readable summary of notable changes to RustyNES — a few
tight highlights per release, and **the complete list of releases**. The format
is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and the
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

**Where the deep per-release detail lives, stated accurately.**
[CHANGELOG-FULL.md](CHANGELOG-FULL.md) carries the full engineering narrative for
**v0.1.0 through v2.0.4 only** — it has not been extended since, and this line
used to advertise it as covering every version, which it does not. From **v2.0.5
onward** the equivalent depth lives in three places: the per-release notes under
[`.github/release-notes/`](.github/release-notes/) (v1.10.0 onward, with a few
gaps), the published GitHub Releases, and the per-release rows in
[VERSION-PLAN.md](VERSION-PLAN.md). `docs/STATUS.md` remains the single source of
truth for current state.

Backfilling CHANGELOG-FULL from those summaries is deliberately **not** done: the
detail it promises is the kind that has to be written while the work is fresh, and
reconstructing it after the fact would produce confident prose nobody measured.

RustyNES's cycle-accurate emulation core arrived in v1.0.0; the `v0.9.x` rows are
the documentary lineage of how that core was built (not standalone user
releases), and `v0.1.0`–`v0.8.6` are the original pre-1.0 engine that the
cycle-accurate core later replaced.

## [Unreleased]

The fifth and last release of the v2.8.x line: the MiSTer core's off-die
(SDRAM) build, which v3.0.0 ships as its secondary bitstream. The emulator
does not change. **No hardware has run any bitstream.**

### The MiSTer core

- **The SDRAM controller's power-up sequence follows the datasheet.** It
  raised the clock enable on the same edge as its first command, which the
  part ignores because it samples the clock enable one cycle earlier. The
  controller now issues a no-op first. The SDRAM model the co-simulation
  uses could not see this, so the model now registers the clock enable the
  way the part does and flags a command it would ignore.
- **Reads work at CAS latency 3.** The controller held the read mask high
  while waiting for data, which masks the data at CAS latency 3 (the
  shipped clock uses latency 2, where it happened not to matter). The model
  now applies the read mask with the part's latency; before the fix every
  read at latency 3 returned zero.
- **The SDRAM controller reads data on the edge the part presents it.**
  The co-simulation's SDRAM model delivered read data one clock later than
  the datasheet, and the controller was written to match, so every test
  passed while real hardware would have read after the part had released
  the bus. The timing analysis found it once the SDRAM pins had
  constraints: the read-data capture failed by 11 ns. The model now follows
  the datasheet, the controller samples a clock earlier (reads are one clock
  faster), and the constraints say which edge captures the data.
- **The SDRAM arbiter returns the right byte, and cannot lose a write.** A
  request arriving while the same source was being served changed which
  byte of the answer it received, and a write strobe on the cycle the
  arbiter reported "not busy" could be dropped. Both have a gate that failed
  before the fix. The second is not reachable from today's cartridge path,
  and is fixed so it cannot become reachable.
- **The SDRAM pins have timing constraints** (provisional, from the
  documented AS4C32M16SB datasheet until the SuperStation One's part is
  read at v2.9.2), and the constraints file no longer claims the framework
  constrains them.
- **The off-die bitstream builds without editing source**:
  `scripts/build-offdie.sh` in the sibling, and `OFFDIE=1
  scripts/seed-sweep.sh` for its seed sweep. An off-die bitstream holds the
  console in reset when no SDRAM is detected, and bring-up gains a MemTest
  gate before it.

## [2.8.3] - 2026-09-25 - "Rivet" (the MiSTer core's reset, area and comments, measured)

The fourth release of the v2.8.x line: the RTL audit's robustness rows for
the MiSTer core. The emulator does not change. The core's bitstream is
re-cut at fitter seed 4 (+0.443 ns setup / +0.046 ns hold). **No hardware
has run any bitstream.**

### The MiSTer core

- **Every reset in the MiSTer core is released on the clock that uses it.**
  The console, the save controller and the SDRAM path cleared asynchronously
  from sources unrelated to their clocks (the SDRAM path from the PLL's lock
  signal directly), so a release near a clock edge could let part of the
  design start a cycle before the rest, and the timing analysis could not
  check it. A synchroniser per clock now releases each reset on an edge. A
  module gate checks the release against the old wiring, which it shows
  releasing between edges.
- **The MiSTer core's CPU is about 4% smaller.** Two constructs the RTL
  audit flagged, the fetch-time opcode decode and a redundant address adder,
  were rewritten only after the fit report showed a real saving, each
  compiled on its own: 1,031.4 to 987.3 ALMs, against placement noise of
  about 2. Both are exact, by construction or by an exhaustive check. The
  audit's estimate (about 500 logic elements) was an order of magnitude high.
- **Four comments in the core's RTL said something false, and are
  corrected**: the power-on CPU/PPU lead ("measured as 2" beside the correct
  value, 1), the claim that PAL is "a parameter change", a pipeline depth of
  zero that cannot be built, and a superseded plan cited as current. A block
  indented as if gated by the PPU's dot enable, which it is not, is
  re-indented.
- **The MiSTer core's co-simulation ladder runs from a fresh checkout.**
  Run from a clean tree it silently skipped 19 gates (two third-party
  batteries looked in the wrong directory, and the mapper ROMs were never
  built), and the provenance check failed on two goldens whose ROMs are
  built later in the run. Each is fixed: from a clean tree the ladder now
  reads 164 passed, 0 failed, 1 expected failure, and its one skip is a gate
  whose hand-built ROM no generator produces (it passes where that ROM
  exists, so 165 in all). No gate reports a pass without saying what it
  compared, and every reset input in the core is checked to come from a
  synchroniser.

## [2.8.2] - 2026-09-25 - "Solder" (the MiSTer core's on-die RTL, corrected against the oracle and the wiki)

The third release of the v2.8.x line and its first RTL release: the RTL
audit's correctness rows for the MiSTer core's on-die path. One emulator
change (MMC1), which the test-roms suite, AccuracyCoin 144/144 and nestest
were re-run on; the rest is the MiSTer core, whose bitstream is re-cut at
fitter seed 4 (+0.429 ns setup / +0.097 ns hold). **No hardware has run any
bitstream.**

### The emulator

- **MMC1 never ignores a reset write.** The serial port ignores a write on
  the cycle after another, but only the data bit: the bit-7 reset always
  takes effect (nesdev MMC1). The emulator ignored the reset too, which
  *Shinsenden* (a reset on a read-modify-write's second write) needs not to
  do. The MiSTer core already had it right.

### The MiSTer core

Each fix
failed a new co-simulation gate first, and that gate catches the fix's mutant.
The RTL was written from the nesdev wiki and first-divergence reads of this
emulator's traces; no reference HDL was opened (ADR 0037).

- **An MMC3 IRQ acknowledge is not lost to a counter clock on the same
  edge.** When a `$E000` write and the A12 rise that took the counter to zero
  landed on one master clock, the counter's "pending" won, leaving an IRQ
  pending with IRQs disabled until the next `$E000`. A module gate places the
  two on one clock.
- **SNROM's PRG-RAM follows its CHR-bank enable.** SNROM boards (*The Legend
  of Zelda* and other battery MMC1 games) wire CHR A16 to a second PRG-RAM
  enable; the core ignored it, so RAM the game had disabled stayed writable.
- **The triangle and noise drop a length reload that lands on a length
  clock**, as the pulse channels already did and as this emulator does.
- **A `$2002` read on the vertical-blank dot leaves the value it returned on
  the data bus.** The latch was rebuilt from the registers, which had not yet
  set, so a following read of a write-only register saw `$00` where the
  console sees `$80`.
- **Pulse 1's sweep with negate and shift 0 (`$4001 = $08`) is now under a
  gate.** The core was already right; the gate guards the emulator's v2.7.0
  fix.
- **The RTL audit's v2.8.2 rows are closed** in
  `docs/audits/rtl-disposition.md`. Two were not defects as written (the
  MMC3 Sharp reload-to-zero, which the core already implements, and a
  `$2002` case that needs two reads one dot apart), two CPU rows show no
  effect under any gate and are left for the v2.9.0 re-audit, and one (MMC1)
  was the emulator's defect, not the core's; the ledger gains an INVERTED
  verdict for that case.

## [2.8.1] - 2026-09-25 - "Gasket" (the libretro core fits the frontends around it)

The second release of the v2.8.x line, and the last of its libretro work:
every row of the libretro audit ledger now has a verdict and evidence.
Emulation does not change (AccuracyCoin 144/144, nestest, every golden); the
libretro core's audio is about 6 dB quieter, on purpose.

### The libretro core's build, input and metadata

- **The libretro `Makefile` does what its callers ask.** `make PREFIX=/usr/local`
  no longer breaks the copy (the file prefix is now `LIB_PREFIX`),
  `platform=win` builds the Windows target instead of the host's, `DEBUG=1`
  builds and copies the debug core, and the copy follows `CARGO_TARGET_DIR`.
  Each was reproduced with `make -n` first and is pinned by a test.
- **The core declares UNIF.** It always loaded `.unf` / `.unif`, but its
  `retro_get_system_info` and its `.info` said only `nes|fds`. The copy of the
  `.info` RetroArch downloads lives in libretro-super and still says `nes|fds`
  until the upstream sync at v3.0.0 (maintainer decision, 2026-09-22).
- **`scripts/resubmit_libretro_docs_pr.sh` passes its title and body to `gh`.**
  A missing line continuation had split the command in two.
- **Four-player games work in RetroArch.** A new core option,
  `rustynes_four_score`, plugs in the Four Score adapter and reads players 3
  and 4; the core modelled the adapter but nothing could turn it on.
- **Switching a port from the Zapper back to a controller works.** The gun
  stayed on the bus, so the controller on that port did nothing.
- **Every port has button names, and the list ends where RetroArch expects.**
  Only player 1 was described, and the list ended with an empty string instead
  of the null libretro specifies, so RetroArch read past its end.
- **Input is read once per frame, one call per pad.** The core polled a second
  time and read each pad with sixteen calls where one bitmask read does.
- **Expansion audio no longer clips in the libretro core.** It mapped `0.5` to
  full scale, and Namco 163 audio reaches about `0.87`, so it hard-clipped
  where the desktop plays it clean. Full scale is now `1.0`, as on the
  desktop, which makes the libretro core about 6 dB quieter.
- **The audit's three performance proposals were measured and rejected**: the
  serialize allocation it described does not happen (the allocator reuses the
  block), and neither a one-pass blit nor a vectorised audio loop is faster.
  The numbers are in `docs/performance.md`.
- **`docs/libretro/` describes the core as it is.** The architecture and
  implementation pages still described the retired dot-lockstep scheduler, the
  old audio math and port-1-only input; both are rewritten against the code.

## [2.8.0] - 2026-09-25 - "Bulkhead" (the libretro core stops a fault at its own boundary)

The first release of the v2.8.x line: the libretro audit's findings about the
boundary between the RustyNES core and RetroArch, each shown failing in a new
C-ABI test harness before it was fixed. Emulation behaviour does not change:
AccuracyCoin 144/144, nestest and every golden are as before. The save state
gains one byte (the internal data bus), and states from earlier releases still
load.

### Save states and the libretro boundary

- **A save state followed by zero padding now loads.** `SectionIter` ends at a
  tail that is zero to the end of the blob; any non-zero byte is still read as
  section data. The libretro core is about to reserve headroom in
  `retro_serialize_size`, and before this a padded copy of its own state loaded
  or failed depending on the padding length mod 9. Every section tag is now
  pinned to four printable bytes, the invariant the rule rests on. A zero
  byte where a tag would start is therefore rejected unless everything after
  it is padding: nine zero bytes used to read as an empty section, so a crafted
  state of many of them made the padding check quadratic. No save this
  project writes contains one.
- **The 2A03's internal data bus is saved.** It is a separate latch from the
  external open bus (a DMC fetch drives only the external one), and it was
  missing from the `BUS` section, so a restore kept the running machine's
  value. Appended under the trailing-default rule: a state written before this
  release loads with it equal to the open bus. Measured beforehand: a stale
  value never changed a frame on three ROMs over 1,500 restores, because the
  next opcode fetch overwrites it first, so this completes the state rather
  than fixing an observed desync.
- **The save-state schema audit now covers the bus.** `snapshot_schema_audit`
  checked the CPU, PPU, APU and OPLL, never `LockstepBus`. Added, it failed with
  49 bus fields unaccounted for. One was a real gap (the internal data bus). The
  other 48 are classified with written reasons: configuration, host input,
  Vs. DualSystem wiring the wrapper re-drives, per-cycle scratch, telemetry, and
  two fields of the pre-v2.0.0 DMA path that only deprecated methods read.
- **The libretro core's save states work with a Zapper plugged in.** RetroArch
  sizes every save-state, rewind and run-ahead buffer from the one
  `retro_serialize_size` the core reports at load, and a Zapper attached later
  grew the state past it, so every save after selecting the light gun failed.
  The core now reserves room for the largest expansion device on both ports
  and zeroes what it does not use.
- **Unloading a game withdraws its memory maps.** RetroArch keeps a core's
  memory descriptors until the core itself is unloaded, so after closing a
  game its cheat search and RetroAchievements held pointers into freed memory.
  The core now replaces them with an empty map before freeing the console.
- **The libretro core loads from any libretro frontend.** It required
  `GET_GAME_INFO_EXT`, an optional command, because the binding it uses
  (`rust-libretro-sys` 0.3.2) reduced the standard `retro_game_info` to an
  opaque byte, so nothing else reached the core. The binding is now vendored
  with that struct written by hand (`vendor/rust-libretro-sys`, MIT, credited
  in `NOTICE`), and a frontend that refuses the command loads through the
  standard struct: its data, or the file at its path. A frontend that answers
  the command with a path but no data now takes the same route instead of
  getting an error. An FDS image is recognised by its signature as well as
  by a `.fds` extension, so one behind a path without that extension loads.
- **An internal error in the libretro core no longer closes RetroArch.** A
  panic anywhere in a frame crossed into RetroArch and aborted it, with no
  chance to write a battery save. The core is now built to unwind, and every
  callback that runs emulation stops a panic at its own boundary: the game
  stops, RetroArch keeps running, and the game's memory stays readable so its
  battery save can still be written. Reloading the game continues. The core
  also frees its buffers when RetroArch shuts it down. The unwind setting comes
  from the crate `Makefile`, the libretro buildbot and CI; a direct
  `cargo build --release` still aborts, so it now warns at build time and the
  core says so in RetroArch's log when a game loads.
- **The libretro core writes to RetroArch's log.** Its messages (loads, parse
  failures, rejected cheats, a contained panic, the abort warning) went to
  stderr, which RetroArch does not put in its own log. They now go through
  libretro's log interface, and to stderr only when a frontend offers none.
  A contained panic is logged with its own message.
- **A C-ABI test harness for the libretro core.** The core's tests now drive the
  exported `retro_*` functions as a frontend does, which is how the four fixes
  above were pinned red first.

## [2.7.6] - 2026-09-24 - "Recount" (the v2.7.5 deletions measured one at a time)

A measurement release between the v2.7.x and v2.8.x audit lines. v2.7.5
bounded six of the core audit's performance proposals only together; each is
now measured alone, on a workload that reaches it: five are zero, and the
sixth is worth at most about 0.2%, and the cheap byte-identical way to
take it measured slower. One
of them, three stores that
re-wrote values the code already guarantees, is now an assertion of that
guarantee. It also ships the
libretro buildbot work contributed in #554. No emulation behaviour changes:
the one code change is byte-identical in release builds.

### Performance record: the v2.7.5 deletions measured one at a time

- **Six performance proposals that v2.7.5 bounded only together are now each
  measured alone: five are zero.** v2.7.5 applied seven of the core audit's
  proposals in one tree and measured the combined ceiling. That bounds their sum,
  and one proposal's gain could hide behind another's code-layout loss, so
  §3.1 A, B and C, the IMP-06 stores, §3.5b and §3.6 were each re-run alone,
  twice, with the A/B/A control. Five are no faster than their controls in
  either run.
- **The sixth, the pulse sweep-mute check (§3.5b), had never been measured.**
  The benchmark ROMs are silent, and `Pulse::output` stops at the empty length
  counter before it reaches the check: counted, it ran 0 times per frame, in
  v2.7.5's combined probe and in this release's first run alike. On
  `spritecans.nes`, whose pulses run every CPU cycle (59,561 calls per frame),
  deleting the check is 0.10% and 0.14% faster in two runs against flat
  controls: a real ceiling of about 0.2%. The byte-identical way to take it,
  testing the duty step before the check, measured 0.67% slower in both runs,
  and caching the sweep target would add state to four register paths and the
  save state for at most 0.2%. Rejected, with the numbers in
  `docs/performance.md` §v2.7.6.
- **The fast render path checks its rendering history instead of re-writing
  it.** Three stores in `tick_visible_render_fast` wrote `true` into fields the
  path's entry guard already requires to be `true`. They are now a
  `debug_assert!` of that invariant: byte-identical in release, checked in every
  debug and test build, and pinned by a unit test, because no ROM in the corpus
  reaches a state that violates it.
- Recorded, not changed: the per-dot NMI edge sampling (§3.1 C) feeds a latch
  that only the deprecated `poll_nmi` reads, and the DMA drain check (§3.1 B)
  guards an accumulator only a unit-test path feeds. Both go with the
  pre-v2.0.0 methods whose removal is decided at v2.9.0.

### Libretro buildbot (contributed by @WizzardSK, #554)

- **The macOS RetroArch cores build again.** Both macOS buildbot jobs had
  failed in every pipeline: libretro's macOS templates set `SDKROOT` in a
  `before_script` that each job's own `before_script` replaced, so the
  bindings were generated against Linux headers and failed. Both jobs now set
  it themselves.
- **New buildbot targets: 32-bit Windows, 32-bit Linux, and webOS** (armv7a
  and aarch64). The first three publish to live download trees, so those
  cores reach RetroArch's updater. webOS aarch64 is a build target only for
  now: no reference core builds it yet, and the buildbot has no
  `webos/aarch64` download tree, so it does not reach the updater until one
  exists.
- The Linux aarch64 job now builds from libretro's own aarch64 template,
  which sets the linker and installs the `gcc-aarch64-linux-gnu` cross
  compiler; the job no longer spells either out itself.
- The GitHub `libretro-cross` gate also checks `i686-pc-windows-gnu`,
  `i686-unknown-linux-gnu` and `armv7-unknown-linux-gnueabi`, so the new
  32-bit jobs get a pull-request-time rehearsal.

## [2.7.5] - 2026-09-24 - "Tally" (every audit claim closed with a measurement or a reason)

The last release of the v2.7.x audit line: the core and frontend audit ledgers
now have no open row. No emulation behaviour changes, so AccuracyCoin,
nestest, and every golden are unaffected.

### Performance — the core audit's proposals

- **The audio buffer keeps its capacity between frames**, so each frame's audio
  is gathered into one allocation instead of regrowing from nothing: -0.89%
  frame time, reproduced on two runs, for the apps that take audio a frame at a
  time (the mobile apps and the web build). The desktop was already
  allocation-free here. Audio is byte-identical.
- **The other eleven hot-path changes are rejected.** Ten were measured before
  any was built (one more had been in v2.3.1), and one was closed by
  reasoning. One probe applied most of them at their maximum, deleting the work
  outright where needed, to bound what they could win; the bound was zero on
  two clean runs, and the one rewrite among them (branchless status flags) was
  also run alone, twice, and measured zero. CPU inlining and cold-path
  outlining, and the branchless palette mirror, got their own runs and measured
  zero too. The full record, including the audit premises that turned out wrong
  and a benchmark that could not see the change it was credited with measuring,
  is in `docs/performance.md`.
- **Capping the audio buffer is rejected** as a behaviour change with no user:
  every host drains each frame, and the one consumer that accumulates would
  have lost audio.

### Changed — interfaces marked for removal

- **18 dead `rustynes_cpu::Bus` methods and the `ApuBus` trait are
  `#[deprecated]`.** Found by the compiler, not by search: 5 of the audit's 22
  "dead" methods are live and were left alone, and two it missed were not.
  Removal is decided at v2.9.0 (ADR 0041).

### Fixed

- **An HD pack no longer loses capacity to a corrupt image.** A PNG whose pixel
  data failed to decode kept the budget its header had reserved, so a valid
  image later in the pack could be refused as over budget.
- **The FDS error says what to do.** Opening a Famicom Disk System image where
  only cartridges load (the mobile apps) said FDS was "planned for v2.2.0"; it
  now says the image needs the disk loader and a BIOS.
- **Architecture documentation describes the v2.0.0 scheduler.** `AGENTS.md`
  and `docs/scheduler.md` still described the retired dot-lockstep design and a
  bus type that does not exist.

### Deferred

- **The mobile per-frame framebuffer copy (MOB-06)** waits for UniFFI to
  release mutable borrowed byte buffers; the newest release, 0.32.2, has
  read-only ones only.

## [2.7.4] - 2026-09-24 - "Pocket" (the mobile apps survive what a phone does to them)

### Fixed — both mobile apps (the shared Rust bridge)

- **A crash in the emulator no longer closes the app.** The Android and iOS
  libraries were built to abort on any internal error, so one bug anywhere in
  the bridge ended the app with no save and no report -- including in the calls
  that were meant to catch it. They are now built to recover
  (`release-mobile`, +18% native size, no measurable speed cost), every native
  entry point contains a failure. A failure while a frame runs is caught
  inside the bridge itself -- the generated Swift would otherwise turn it into
  a crash, and the Android frame loop did not catch it -- so the app stays
  open, the game freezes rather than running on from a half-updated machine,
  any netplay session ends, and the app says so; reopening the game or loading
  a save state continues (frontend audit MOB-03, MOB-07).
- **Battery saves persist on Android and iOS.** An in-game save now survives a
  relaunch on both phones, with the desktop's rules: only cartridges whose
  header sets the battery bit, and a save that cannot be used is left untouched
  rather than overwritten (MOB-05, AND-09).
- **Touch and gamepad input never wait for the emulator.** A button press on the
  UI thread waited for the current frame, a netplay rollback or a Lua callback
  to finish (MOB-01).
- **Leaving a netplay screen or logging out of RetroAchievements no longer
  freezes the app** for up to 10-30 seconds when the server is unreachable. This
  also applies to the desktop (MOB-04).
- **A failed save-state load leaves the game as it was.** A save state rejected
  partway through loading left the machine half-restored, on every platform,
  desktop and libretro included (MOB-08).
- ROM files over 16 MiB are refused before they are copied (MOB-02), and
  netplay's relay-server lookup no longer stalls a frame (MOB-09).

### Fixed — Android

- **The game pauses when the app leaves the screen** and resumes when it
  returns, and the app now asks for audio focus: it goes quiet for a call, ducks
  for a navigation prompt, and gives the audio back when paused. Picture-in-
  picture keeps playing (AND-03).
- **Saves are written atomically**, so a phone that dies mid-write keeps the
  previous save instead of a truncated one -- save states, RetroAchievements
  progress, the recent-games list, the library and per-game settings (AND-05).
- Opening a game no longer reads and hashes it on the main thread, and the
  save-state screen no longer does its file work there (AND-04).
- The GPU renderer recovers after a slow surface teardown instead of staying
  black (AND-01), and no longer wakes 500 times a second while idle (AND-08).
- The game screen no longer rebuilds all of its interface on every frame (AND-02).

### Fixed — iOS

- **Audio stays in step and comes back.** The sink gained rate control, starts
  from a filled buffer, recovers after the system's media services reset, and
  an unplugged headset no longer leaves the game frozen until the app is
  backgrounded (IOS-02, IOS-03, IOS-08).
- **Smoother picture:** the display runs at 60 Hz with one emulated frame per
  refresh, instead of a 120 Hz link that periodically showed a frame twice or
  skipped one (IOS-01).
- **iCloud save states are not lost.** Uploads finish when the app is
  backgrounded, a save that failed to upload is retried instead of shown as
  synced, and an older save can no longer overwrite a newer one from another
  device (IOS-04, IOS-10).
- A swipe off the bottom of the controls no longer sends the app home (IOS-05);
  video filters pause while the device is hot (IOS-11); opening a ROM from
  another app while one is running no longer freezes the screen; netplay
  connection lookups no longer freeze the interface (MOB-09).

### Changed

- Android and iOS native libraries build with the new `release-mobile` profile
  (`release` with `panic = "unwind"`).
- New CI check: `scripts/ios-host-typecheck.sh` compiles the iOS-only Rust
  (the C-ABI shim and the audio sink) on Linux. No pull-request job compiled
  them before.

### Not verified here

Every Swift change, and the Android lifecycle, audio-focus and renderer changes,
need a real device; they are on the maintainer's per-platform checklist. The
Android battery-save path was run on the emulator.

## [2.7.3] - 2026-09-23 - "Hearth" (the desktop and web frontends keep what they are given)

### Fixed — desktop and web

- **Battery saves persist on the desktop.** A cartridge with a battery now
  keeps its in-game save in `<data_dir>/battery/<rom-sha256>.sav`: loaded when
  the ROM loads, and written when it changes (checked once a second, and always
  on ROM switch, close and exit). Until now the desktop kept no such file, and
  an in-game save survived only inside a save state. Only carts whose header
  sets the battery bit are persisted, and a file that cannot be used is left
  untouched rather than overwritten, with the reason shown in the status bar.
  The periodic write runs without holding the emulator (frontend ledger FE-01).
  New: `Nes::has_battery()`.
- **The debugger's text no longer turns to garbage after a skipped frame.**
  With the debugger or a tool panel open, a frame whose swapchain image could
  not be acquired (a resize, a lost surface, a timeout) discarded egui's font
  and image uploads, which egui sends only once. The panels then drew with
  textures the renderer never received until restart. Uploads now happen before
  the acquire, and releases after it (frontend audit DESK-01).
- **One key no longer does three things.** `M` toggled the menu bar, held the
  Famicom microphone, and pressed P3 Select, all at once; P2 Select and P3
  Right were both `L`. New defaults: the microphone is held on `N`, P3 Select
  is `,` and P2 Select is `R`. `M` still toggles the menu bar. Saved bindings
  are kept as they are (frontend audit DESK-02).
- **No more periodic audio clicks at low latency on large-period devices.**
  When the audio device's period was at least twice the configured latency
  (for example a 2048-frame period at the 20 ms setting), rate control held the
  buffer below one device callback, and the stream underran about once a
  second. The latency target now never drops below two device callbacks (frontend
  audit DESK-05; the audit's own 1024-frame example measured no underruns).
- **Audio comes back after the output device goes away.** Unplugging headphones,
  a Bluetooth drop or an audio-server restart used to leave the emulator silent
  until restart. The stream is now reopened, on the same device or the default,
  within about two seconds, taking the new device's channel layout if it
  differs (frontend audit DESK-04).
- **Fast-forward no longer floods the UI thread.** Every fast-forwarded frame
  posted its own wakeup, hundreds a second, each contending for the emulator
  lock. At most one is now pending while fast-forwarding, except with
  RetroAchievements active, which still counts every frame (frontend audit
  DESK-03).
- **The web build fits a phone screen**, instead of a fixed 512 x 480 canvas
  that overflowed it, and releases the audio worklet's temporary script URL once
  loaded (frontend audit DESK-06, DESK-07).
- **Achievement badges from earlier games are released** when another ROM loads,
  instead of staying resident for the whole session (frontend audit DESK-08).
- **Stopping an A/V recording no longer freezes the window.** The ffmpeg encode,
  seconds to minutes for a long take, ran on the UI thread. It now finishes in
  the background, reports when done, and completes before a quit (frontend
  audit SEC-05).

### Changed

- **The `[retroachievements] host` setting is gone.** Nothing ever read it, so
  editing it changed nothing. Config files that still contain it load
  unchanged, and the next save drops it (frontend audit CON-05).
- **Building the web frontend with both `wasm-canvas` and `wasm-winit`** now
  stops at compile time with the right invocation, instead of failing late at
  `wasm-bindgen` (frontend audit CON-01).

### Security

- **An HD pack can no longer exhaust memory with one image.** A PNG's declared
  size was trusted when sizing its decode buffer, so a small file could ask for
  gigabytes. Images are now refused past 16384 pixels a side or 4096 x 4096 in
  total, before anything is allocated, and a whole pack may decode at most
  1 GiB of images (frontend audit SEC-01).
- **A Lua script can no longer hang the emulator or exhaust its memory.** The
  per-frame instruction budget could be caught by `pcall`, `xpcall` or
  `coroutine.resume`, so a wrapped runaway loop ran forever holding the
  emulator lock. It now escapes every catcher, and the budget now also covers
  callbacks the host fires (`reset`, `spriteZeroHit`, `codeBreak`, the TAStudio
  events), which had none. And a loop inside a coroutine had
  no budget at all, because the hook was removed inside coroutines; it is now
  global. The script heap is capped at 64 MiB, and `emu.log` / `print` output,
  which is held outside that heap, at 8,192 lines a frame of up to 4 KiB each
  (frontend audit SEC-02, SEC-03, and FE-02, found while fixing SEC-03).
- **Script HTTP requests cannot reach the machine's own services by default.**
  With the opt-in `script-ipc` feature, `comm.httpGet` / `httpPost` fetched any
  URL, including loopback services and cloud metadata. Addresses that resolve
  to loopback, private or link-local ranges now need the host listed in
  `RUSTYNES_COMM_HTTP_ALLOW` (e.g. `localhost:8080`). Redirects are returned to
  the script instead of followed, no environment proxy is used (it would
  resolve the target where the check cannot see it), and a body over 10 MiB
  is a transport failure (`status = 0`) (frontend audit SEC-04; ADR 0016 amended).
- **An HD pack's music cannot claim an absurd sample rate.** The declared rate
  sized the resampled track, so a 1 Hz header expanded it 48,000-fold; rates
  outside 8-384 kHz now make the track inert (frontend audit CON-03).

## [2.7.2] - 2026-09-23 - "Bankroll" (every bank the cartridge has, and nothing it has not)

### Fixed — mapper memory

- **MMC1 reaches SUROM / SXROM's upper 256 KiB and banks SOROM / SXROM
  PRG-RAM.** On boards with at most 8 KiB of CHR, the CHR bank register's bit 4
  selects the 256 KiB PRG half (fixed bank included), and bits 3-2 select the
  8 KiB PRG-RAM bank. The old code read four PRG bits and no RAM bank, and
  treated SUROM's PRG line as SNROM's RAM disable. Two new holy_mapperel ROMs
  (512 KiB SUROM and SXROM, built from the v0.02 tag) went from `S*ROM`,
  `PRG RAM MISSING`, `0300` to `PASS 0000` (core audit §5.4). In 4 KiB CHR
  mode the register driving these lines is the one PPU A12 last selected, as
  the wiki warns; Bregalad's "WRAM disable scanline counter" ROM
  (`tests/roms/mmc1_a12`, previously catalogued as an inert smoke test) now
  draws its raster bar, and its snapshot was re-blessed.
- **MMC5 banks its PRG-RAM.** `$5113` and the RAM-mode `$5114-$5116` values
  now page the RAM over the wiki's 64 KiB "compatible superset", so a game gets
  the RAM it expects even when its header under-declares it (*L'Empereur* does);
  the save stays the header's battery-backed part, and a 16 KiB board saves only
  its first 8 KiB (core audit §5.3).
- **Namco 163 selects nametables through `$C000-$DFFF` and maps CIRAM as
  CHR-RAM.** Nametable quadrants can be CIRAM pages or read-only CHR-ROM pages,
  and CHR values `$E0-$FF` map console RAM unless `$E800` disables it. The
  registers power on as the header's layout, so existing games render as before
  (core audit IMP-11). The PPU reaches all of this through the nametable hooks,
  and *Mappy Kids*, whose title and town screens were garbled, now renders.
- **A board with nothing at `$6000-$7FFF` now reads open bus there, not `$00`.**
  A sweep of every mapper number found 205 board variants inventing a zero; the
  default now floats the window exactly when the board has no save RAM, and a
  converse check keeps the seven boards with ROM or registers there mapped
  (core audit §5.5).
- **Sachen registers keep the floating bits they do not drive.** A new additive
  `Mapper::cpu_read_driven_mask` lets a partial read (Sachen 150/243's 3-bit
  registers, TCA-01's 6-bit protection read) leave the rest of the byte to the
  bus's open-bus latch (core audit §4.5).
- **Kaiser KS202 (mapper 56) work RAM is writable.** Its `$6000-$7FFF` RAM had
  a read path and no write path (found by the open-bus sweep).
- **Five boards gain the `$6000-$7FFF` RAM their wiki pages document**: mappers
  156, 177, 241 and 245, and mapper 227's battery-backed FW-01 variant. The
  models had none, so writes vanished; battery-backed ones now save.
- **Ten fixed-mirroring boards accept a per-game header correction**, each
  checked against its wiki page: mappers 11, 13, 34, 38, 70 (unless the header
  says four-screen), 79, 87, 94, 180 and 184 (core audit §5.6).

### Changed — save states

- MMC1 and Namco 163 mapper blobs gain one trailing byte each (the CHR-A12 latch;
  the `$E800` CHR-RAM disables); MMC5 gains its superset RAM pages; mappers 156,
  177, 227, 241 and 245 gain their RAM. Older blobs still load. MMC1 and MMC5
  resume under the corrected banking, and new RAM starts zeroed. Namco 163
  rebuilds its nametable layout from the header, because old blobs hold the
  registers zeroed, and leaves CIRAM with the PPU for that session.

## [2.7.1] - 2026-09-23 - "Keepsake" (a save that appears to succeed now does)

### Fixed — save data

- **Six cartridge boards handed RetroArch an empty battery save.** The
  libretro core gives RetroArch `Nes::sram()` as the game's save RAM (its
  `.srm` file), and the mapper default is an empty slice. These boards kept
  their save memory elsewhere without overriding it: Bandai FCG's serial
  EEPROM (mappers 16 and 159), Taito X1-005's 128-byte RAM (80), TxSROM (118)
  and TQROM (119), which did not forward to their MMC3, Multicart 15, and
  BMC-FK23C's 32 KiB WRAM (176, and mapper 30 with CHR-ROM). The game ran and
  the save appeared to succeed; the `.srm` was empty. Five were in the core
  audit (IMP-08/09/10, §5.1e); FK23C was found by the new test, which builds
  every NES 2.0 mapper number rather than trusting a list.
- **User files are no longer truncated in place.** FDS disk saves,
  RetroAchievements progress, movies, screenshots, subtitle and history-clip
  exports, fm2/bk2 export, TAStudio projects, the memory-compare export,
  HD-pack builder output and the PPU viewer's CHR PNG export now go through the
  atomic writer, and A/V, GIF and WAV recordings are encoded to a staging file
  that replaces the target only when ffmpeg succeeds. A crash, a full disk or a
  failed encode leaves the previous file rather than a truncated one (frontend
  audit SEC-06). A test fails on any new `fs::write`, `File::create` or
  `OpenOptions` writer in the frontend outside an argued allow-list.
- **An unusable `config.toml` is kept before defaults replace it.** It is
  copied to `config.toml.corrupt.bak` first, instead of being overwritten by
  the next save, whether it fails to parse or is not valid UTF-8 (frontend
  audit CON-04).

### Known gap — desktop and mobile battery saves

- **The desktop and mobile frontends do not persist cartridge battery RAM.**
  Found while checking the entry above: nothing in `rustynes-frontend` or
  `rustynes-mobile` reads `sram()`, and the desktop app writes only the FDS
  disk sidecar. A game's in-cartridge save on those hosts survives only inside
  a save state. The core audit assumed the opposite; for mobile the frontend
  audit already records it (AND-09, MOB-05). Not fixed in this release.

### Changed — provenance

- **Three mapper files are now recorded as derived from Mesen2 / puNES.**
  `m085_vrc7.rs` (the VRC7 audio register-write path), `m099_vs_system.rs` (the
  DualSystem sub-console banking) and `m244_cne_decathlon.rs` (the PRG and CHR
  scramble tables) quote Mesen2 source expressions in their comments. The core
  audit proposed deleting those citations; that would have been laundering, so
  each file instead carries a `// Provenance:` header, a row in
  `docs/originality-and-provenance.md` §1 and a `NOTICE` entry. A new test fails
  if a header and its §1 row disagree in either direction.

### Changed — CI

- **PR checks run what a change can affect, and the heaviest wait for the
  review to finish.** Measured over the last 109 PR runs (21.6 runner-hours),
  the four largest jobs were Android's Gradle packaging (15.5 min median, and
  `continue-on-error`, so it could never fail a PR), the NDK build (11.2), the
  debug `test` (12.3) and the Pages build (6.3).
  - **Draft gate.** A PR opened as a draft runs the fast gates only; `test`,
    `test-roms`, the Android build and the Pages build run once it is marked
    ready (`ready_for_review`). A merge queue would be the native form and is
    unavailable to a user-owned repository. Measured on #547: the draft run is
    ~6 minutes of wall clock against 18-32 before. Copilot's automatic review
    does not run on a draft; requesting it (`gh pr edit --add-reviewer
    @copilot`) does.
  - **Gradle packaging** runs on `main`, on dispatch, and on a PR only when the
    Android app (`android/**`) changed. `cargo-ndk` is cached instead of
    compiled from source twice per run, and the Android cargo cache is saved
    from `main` only.
  - **`test-roms`** compiles and runs the two packages that have the feature
    instead of the whole workspace in release, and no longer waits for `lint`.
  - **Path filters derived from `cargo tree --target all`**: the libretro
    cross-compiles and the `no_std` build run on a PR only when a crate they
    compile changed, and `security.yml` runs only when a manifest, the
    lockfile, `deny.toml` or `.cargo/` changed (the weekly cron still re-checks
    advisories against an unchanged lockfile).
  - **`Clippy Security Lints` removed.** It re-ran a full workspace clippy with
    `retroachievements` and `gpu-timing`, both of which `ci.yml`'s lint job
    already covers (`gpu-timing` is a default feature).
  - **Runners pinned to Ubuntu 26.04** (`ubuntu-26.04`, `ubuntu-26.04-arm`,
    GA 2026-09-17) instead of `ubuntu-latest`, which carried a migration notice
    on every job; the next OS move is now a reviewed change.
  - **Pages on a PR** builds the wasm demo, the size budget and the handbook,
    and skips rustdoc (already gated by `lint`) and the deploy-only steps.
- **Fixed:** `android.yml` triggered on three of the eleven workspace crates
  the Android build compiles, so a change to `netplay`, `script`, `cheevos`,
  `ra`, `hdpack` or a chip crate that broke it never ran the workflow.
- **Fixed:** `web.yml` put every event in one global `pages` concurrency group
  with no cancellation, so PR builds from different PRs cancelled each other;
  PRs now get their own group. `android.yml` had no group at all and now
  cancels a superseded PR run. The Pages build job, which runs PR code, now
  holds no Pages permission: `configure-pages` moved into the deploy job with
  `pages: write` and `id-token: write`.
- **Zero warnings in the workflow logs.** A sweep of the last 150 runs' logs
  and annotations, fixed at the source where the source is ours:
  - Android: rustup's toolchain auto-install deprecation (the toolchain is now
    installed explicitly), cargo-ndk's `ANDROID_NDK_HOME`/`ANDROID_NDK_ROOT`
    mismatch (both now name the same NDK), UniFFI's missing-ktlint warning
    (`--no-format`), and two Kotlin `UNUSED_EXPRESSION` warnings in UniFFI's
    generated bindings (suppressed on the generated file's own `@Suppress`).
  - Pages: 13 MkDocs link warnings, each a dead link on the published
    handbook. A build hook (`scripts/mkdocs_repo_links.py`) rewrites links
    that leave `docs/`, or point at an excluded page such as an ADR, to their
    GitHub URLs; two anchors GitHub and MkDocs slug differently and one bare
    `../` link are fixed; `docs/agents/` and `docs/audits/` join
    `exclude_docs` and the MiSTer page joins the nav. The handbook now builds
    with `--strict`, so a new broken link fails the build. Material is pinned
    `>=9.7.5`, the release that caps MkDocs below the plugin-less 2.0.
  - `actions/deploy-pages`' own `punycode` deprecation (DEP0040) is silenced
    for that step only; Homebrew's untrusted-tap warning on iOS is removed by
    untapping the image's unused `aws/tap`.
- **Fixed:** the stock `full_frame` benches measured the fast dot path from
  v2.2.3 to now. That release made the fast path the PPU default, and the stock
  benches never selected a path, so each stock/`_fast` pair measured the same
  routine and every "exact path" figure recorded since was a fast-path figure.
  The benches now select the path explicitly; `docs/performance.md` carries the
  current numbers (nestest 4.458 ms exact / 3.950 ms shipped, −11.4%;
  flowing_palette 2.672 / 2.654 ms, neutral) and a dated note on how to read the
  affected rows. The CI relative gate now watches the shipped `_fast` pair, and
  the absolute ceiling checks all four.
- **Fixed:** the Android workflow's path filter also missed
  `rustynes-gfx-shaders`, an Android-target-only dependency that a host-target
  `cargo tree` does not list, and `.cargo/`.

## [2.7.0] - 2026-09-23 - "Palisade" (untrusted input stops at the boundary)

### Changed

- **The SuperStation One core moves to v3.0.0, and v2.7.x-v2.9.x become an audit
  line** ([ADR 0041](docs/adr/0041-hardware-release-is-v3.0.0.md)). The first
  hardware-verified FPGA core is a new deliverable class, and the MAJOR-bump rule in
  `VERSION-PLAN.md` gains that as a second trigger alongside API and save-state
  breaks. The board session moves from v2.7.0 "Shakedown" to v2.9.2, so that it
  measures the bitstream the audit lines produce rather than one they are about to
  replace. v2.7.x acts on the core and frontend audits, v2.8.x on the libretro and
  RTL audits (including the off-die SDRAM build), and v2.9.x re-audits, runs the
  final seed sweeps and takes the release-candidate `.rbf` pair to the board. The
  Shakedown plan's strands A-F carry into the v2.9.x plan with stale facts
  corrected; both v2.7.0 plans are kept and marked superseded. Features the older
  plans deferred "to v2.8+" are now deferred past v3.0.0.

### Added

- **[`docs/audits/`](docs/audits/README.md)**: the four AI-written audit reports from #544 (core, frontend,
  libretro, RTL), moved unchanged, with a disposition ledger per report. The ledgers
  start from a calibration pass that read 26 of the reports' claims against the code:
  the core, libretro and frontend reports are largely accurate at the cited lines;
  the RTL report's DC-blocker overflow and pulse-1 sweep-mute defects, the DQM half
  of its SDRAM finding, and its resource table are refuted. Every finding now goes
  triage, failing test, fix, gate.
- **Four plans:** `to-dos/plans/v2.7.x-core-frontend-audit-plan.md`,
  `v2.8.x-libretro-rtl-audit-plan.md`, `v2.9.x-final-audit-and-hardware-plan.md` and
  `v3.0.0-superstation-core-plan.md`, indexed in `to-dos/plans/README.md`.
- **`.gitignore`** covers the agent review scratch (`cr_*.json`, `cr_*.txt`,
  `fix_*.py`, `*_comments.json`, `threads.json`, `pr_*.txt`) that had accumulated
  untracked in the repository root; root-anchored, and matching no tracked file.
- **The audit reports are the working reference for every release to v3.0.0**,
  read beside the plans: the v2.7.x, v2.8.x and v2.9.x plans and
  `docs/audits/README.md` now say which reports each line works from. A plan
  compresses a finding to a table cell; the report keeps the mechanism, the
  locations and a remediation sketch.
- **Three agent notes** from the review of the re-plan (`docs/agents/`): the
  Antigravity reviewer re-files findings already refuted, so the bot ceremony
  needs a stopping rule; Copilot reviews once per PR here and Docs7 reports the
  private sibling repository's links as dead; and per-suite test counts read off
  a combined `cargo test` run are mislabelled, because the result lines are not
  in `--test` order. Plus one tooling trap: `ssh-add -l` failing does not mean
  commit signing is down.

### Fixed

- **Pulse 1 no longer mutes on the `$4001 = $08` idiom** (core ledger T-01). With
  negate on and shift 0 -- the documented way to disable the sweep -- the oracle
  computed pulse 1's target as `c - c - 1` in wrapping arithmetic, got `$FFFF`,
  and muted the channel. NESdev "APU Sweep": a negative target clamps to zero and
  negate never mutes. Red on v2.6.23 by `pulse1_negate_shift0_clamps_to_zero_and_does_not_mute`;
  a companion test shows the clamp changes nothing for shifts 1-7 over periods
  8-`$7FF`. The full `--features test-roms` suite run with this fix alone
  (2,612 passed, 0 failed) moved no golden; on the final release tree it is
  2,622 passed, 0 failed. The MiSTer sibling already followed the wiki, so the co-simulation
  ladder could not see this; v2.8.2 adds the gate.
- **A hand-edited or corrupt save state can no longer crash or hang the emulator**
  (core audit IMP-01, IMP-02, IMP-03). Every one of these restored cleanly and
  failed on the next tick, which on a release build (`panic = "abort"`) kills the
  process:
  - PPU: a sprite count above 8 (typed `InvalidSprCount`).
  - APU: twelve register-width fields -- duty, sequencer steps, the three sweep
    fields, envelope volume/divider/decay, DMC rate, bit count and DAC -- are
    bounded to their register widths (`FieldOutOfRange`, naming the field).
  - Resampler: a zero sample rate, a non-finite or non-positive CPU rate, a rate
    ratio above one host sample per CPU cycle, a phase outside `[0, 1)`, or a
    non-finite filter or held value (`InvalidResampler`). These did not panic;
    they hung the emulation thread or filled the audio with NaN.
  - Container: `SectionIter` uses `checked_add` for a section's end (a real
    overflow on 32-bit `wasm32`), and stops after its first error instead of
    returning it forever.
  - More the audit did not report, found by the new fuzz target: a restored
    PPU raster position past dot 340 or the pre-render line (the per-dot
    advance never wraps it); a restored fine X above 7 (a shift overflow),
    after which every counter and index the PPU restore loads was swept by
    reading rather than left to the fuzzer, bounding ten more; a restored
    OAM-DMA byte index at or above 256 (an overflow); a restored
    `dma_mc_consumed` that tripped a dev-profile invariant assertion; and a
    CPU master clock and PPU clock restored far apart, which made the next
    PPU catch-up run for billions of dots or never, a hang either way
    (rejected beyond 1,024 master clocks of skew).
  - From review: a restored extra-scanline countdown under a live overclock
    knob could idle the PPU for 65,535 lines. It is clamped to the knob in
    force, not rejected, because the knob is not saved and a larger one may
    have written a genuine file.

  States the emulator writes stay inside every bound, so no real save is
  rejected; the round-trip tests are the guard.

### Security

- **`#![forbid(unsafe_code)]` on the five chip crates** (`rustynes-cpu`, `-ppu`,
  `-apu`, `-mappers`, `-core`). None contained `unsafe`; this makes that a
  compile-time guarantee (core audit section 2.1).
- **The `save_state` fuzz target can now find what it exists to find.** It
  restored and stopped, and fed only raw bytes, so every deferred panic above was
  invisible to it and almost no input got past the header. It now patches a real
  snapshot of a rendering machine and runs about three scanlines after an
  accepted restore. Against the unfixed tree, with each found defect fixed in
  turn so the next could surface, it found seven defects; five were not in
  the audit. Its first patch mode had a reach defect of its own, caught in
  review: offsets were taken over the whole blob, whose 245,760-byte
  framebuffer sits in front of the APU section, so no APU field was ever
  patched. Offsets now skip the framebuffer and are 24-bit (the rest of the
  state is itself over 64 KiB), and the unfixed APU crashed in 1,193 runs.
  After the fixes: 2,400,000 executions over 8 jobs, 0 crashes, 0 timeouts.

### Notes

- **The three mapper files the core audit wanted "sanitized"** (`m085_vrc7.rs`,
  `m099_vs_system.rs`, `m244_cne_decathlon.rs`) quote Mesen2 source expressions, so
  they are derivation statements. They get `// Provenance:` headers and `NOTICE`
  entries in v2.7.1; nothing is removed.
- The re-plan itself changes no emulation behaviour and bumps no version.

## [2.6.23] - 2026-09-20 - "Pulse" (the access does not increment, it pulses the load already there)

### Fixed

- **The CHR-during-rendering divergence closes, and the cause was `v` itself.**
  `RustyNES_MiSTer`'s `chrram-live` gate had been RED for eight releases at
  **32,861 of 61,440 differing pixels** and now reports *"All 61440 pixels
  match"*; its read-side sibling `chrram-fetch` went **21,941 → 18** diverging
  fetches. `chr_wr` is 4,389 before and after, so it is the same stimulus that
  used to fail. The emulator is unchanged — this is a DUT fix — but the finding
  is the oracle's to record, because the oracle's own trace is what produced it.
- **Eleven prior variants were measured correctly and the conclusion drawn from
  them was wrong.** They swept *what* the `$2007` arm computes — composition,
  collision policy, write phase, write address, condition — and concluded "the
  `v` divergence is not in this logic", which closed the search. None swept
  *what it computes it from*. On silicon the access does not perform an
  increment of its own: it **pulses the rendering pipeline's existing load**
  (NESdev's Visual 2C02 material). Three changes, each exposing the next —
  increment the value *this dot's* load produced (21,941 → 2,646), **resolve**
  against that same value (→ 18, which stopped 33 nametable cells being written
  one coarse-X cell early), and address the CHR write from it (→ 0 pixels).
- **A nametable fetch address *is* `{2'b10, v[11:0]}`**, so the gate's own
  records already contained `v` and `--ppu-state-trace` gives the oracle's per
  dot. Two traces, one comparison, answer in the first diverging record. No new
  instrumentation was needed, and none was written.

### Added

- **`docs/agents/measurement-discipline.md` 11 → 14 findings** (#536). A
  component measured *through* a defect upstream of it reads as negligible and
  the number is honest — the CHR write address was quantified at 715 of 32,861
  pixels (2.2%) and parked on that basis; with `v` corrected the same one-line
  change was worth the entire remainder. A complete sweep over the wrong space
  terminates with a confident negative. And a per-dot trace from two models is
  not comparable until a reference event with no modelling freedom calibrates
  it.

### Notes

- **No hardware has run any bitstream.** `RustyNES_MiSTer` v2.6.23 ships
  `RustyNES_20260920.rbf`, cut at fitter seed 4 with timing closed at +0.421 ns
  setup / +0.112 ns hold, and that remains a claim about synthesis and
  simulation only. It is the bitstream intended for v2.7.0's bring-up.

- **The 31 legacy AccuracyCoin sub-test ROMs now say which test they run, and
  it is a measurement.** Their `(suite, test)` indices had been unrecorded for
  four releases, under a correct note that rebuilding on guessed indices is
  worse than a stated gap. Both ways of un-guessing them are wrong, and
  confidently: a sub-test ROM embeds its own copy of the assembly, so its
  encoded index names a row of *that* build's table, and upstream has since
  reordered suites **and** inserted tests inside them. Resolving
  `ppu-misc-stale-bg-shift-regs` (encoded 18/2) against the current assembly
  gives `$2002 flag timing`; against the stale suite map it gives `INC $4014`.
  It is neither — the ROM writes `$0483`, *Stale BG Shift Registers*.
  `subtest_identify` boots each ROM and reads which byte in `$0400-$04FF` it
  writes, which is the catalog's own key and needs no map at all. The control:
  `RustyNES_MiSTer/tb/regress.sh` independently registers 18 of these ROMs by
  an address read out of golden RAM in v2.6.4/v2.6.5, and **all 18 come back
  identical**. `BUILD-PROVENANCE.tsv` now has all 33 rows, and
  `accuracycoin_subtest_provenance.rs` re-measures every one of them — seven
  mutations of the manifest and one of the corpus path, all caught.

- **`ppu-misc-2004-stress.nes` does not run `$2004 Stress Test`.** Found by that
  sweep. It writes `$048E`, which is `$2007 Stress Test` — the same entry as
  `ppu-misc-2007-stress.nes`. The corpus holds two ROMs for one entry and
  **none** for `$2004 Stress Test` (`$048C`) while appearing to hold one.
  Neither is a registered gate, so nothing relied on the name; the collision is
  now allow-listed by name with its effect stated, so a *new* one fails instead
  of joining a category that already has members.

- **`tests/roms/AccuracyCoin/README.md` said the result addresses are "not
  always the catalog's". That is the wrong way round.** Every one of the 33 ROMs
  writes the catalog address of some entry, exactly. What varies is whether that
  entry is the one the *filename* names. The recorded `--suite N --test M`
  recipes on that page are also relative to the upstream source of their day and
  several are now stale, which the page now says rather than reading as
  commands to re-run.

## [2.6.22] - 2026-09-19 - "Rigging" (the instruments for the board, built before the board)

**v2.7.0 "Shakedown" is the session with the board, and this is not it.** A
SuperStation One is in hand and **no hardware has run any bitstream** — that
sentence is still true and every anchor asserting it is untouched. v2.6.22 is
the non-hardware half of the Shakedown plan, cut separately on a maintainer
decision so that v2.7.0 keeps meaning what its name says.

The emulation core is unchanged, so **AccuracyCoin and nestest hold by
construction**, and were re-run anyway.

### Added

- **AccuracyCoin reads back from hardware as BYTES, not as a photograph.**
  `RustyNES_MiSTer/docs/bringup.md` had stated the limit precisely — *"a
  photograph is not 149 status bytes, and reading one is a human transcribing a
  picture"* — because the result vector lives in CPU RAM at `$0300-$04FF`, which
  is not in the `$6000-$7FFF` window the save path persists.
  `scripts/accuracycoin-build/build_mirror_rom.py` patches upstream's ROM to
  copy that window to `$6000-$61FF` once the battery finishes, and sets the iNES
  battery bit — the one bit that both creates the PRG-RAM window
  (`rtl/emu.sv:609`) and arms the save controller (`rtl/emu.sv:353`), read out of
  the RTL rather than assumed.

  **The patch moves nothing, and the builder asserts it.** AccuracyCoin is full
  of cycle-exact tests sensitive to page crossings in their own code, so an
  insertion displacing later routines could flip a verdict for reasons unrelated
  to the console — and the flipped verdict would look exactly like a result. The
  diff is exactly 1 header byte, 5 at the call site (`LDA #0` / `STA $4015` ->
  `JSR` + two `NOP`s) and 29 in bank 2's end-of-bank padding; **0 bytes
  anywhere else**, and the builder refuses to emit a ROM whose budget differs.

  **The control is separate, because a budget proves nothing about answers.**
  `accuracycoin_mirror.rs` runs both ROMs through the same driver the shipped
  gate uses and requires the window byte-identical, the decoded vector identical
  entry for entry, and the mirror to reproduce the live window without being
  vacuous — then runs the real comparator over a real save file, because the
  conjunction of two verified halves is a third claim. Three mutations, all
  CAUGHT.

- **`accuracycoin_status` reads a hardware `.sav`.** A `sav:` operand lifts an
  8 KiB battery save onto the work-RAM frame the catalog addresses. The prefix
  is mandatory and a bare `.sav` is refused rather than inferred: decoding a save
  as work RAM does not fail, it reads catalog addresses out of the wrong offsets
  and returns a plausible vector.

- **`RustyNES_MiSTer/tb/check_golden_provenance.py`, run as rung 0.** Every
  golden manifest records the `rom_sha256` it was exported from, and **nothing
  had ever compared that field to anything.**

- **`RustyNES_MiSTer/docs/bringup-log.md`** — the evidence sheet the v2.7.0 gate
  demands: one row per bring-up step, the four properties, Tiers 1–4 and the two
  board-gated tickets, each with a named-artefact column and a verdict. Every
  cell reads NOT RUN, which is the accurate current state.

- **`to-dos/plans/v2.7.0-shakedown-plan.md`** — the forward plan for the board.
  It supersedes the rung-6 and rung-7 rows of `v2.7.0-mister-core-plan.md`,
  whose ladder is otherwise delivered. Two corrections went into writing it: the
  save path **cannot** carry AccuracyCoin's vector off the board (it persists
  `$6000-$7FFF`; AccuracyCoin writes `$0300-$04FF` in CPU RAM — blargg's `$6000`
  status bytes do read back, which is the narrower true claim), and **three of
  six line-number citations in the first draft were wrong** because they were
  carried from an earlier planning round rather than read from the tree.

### Changed

- **Both upstream oracles are re-synced to their newest committed version, and
  one of the two commits is a defect this project reported.** AccuracyCoin moves
  `69c8860` -> **`46199ae4`** and TriCNES `f388af0` -> **`94f1b117`**, each now
  byte-identical to upstream HEAD. The AccuracyCoin window is two commits and
  **165 differing ROM bytes**, and both are the same one-line defect in two
  places — a missing `INC <ErrorCode` between sub-tests, which made two
  sub-tests of one routine report the *same* failure code and so made `Fail(N)`
  ambiguous. `9bc42d1e` fixed it in `TEST_MisalignedOAM2Addr`; **`46199ae4`
  fixed it in `TEST_FrozenOAM2Inc2`, the instance raised upstream from here.**
  It changes no verdict, and that is measured rather than assumed:
  re-extracting `SOURCE_CATALOG.tsv` from the new `AccuracyCoin.asm` reproduces
  the committed 149-row TSV **byte-identically**, because the insertion moves
  code and not the result-address map. The battery re-runs at **144/144,
  100.00%, fail=0**, and nestest is 0-diff. TriCNES's one line is
  `CPU_SYNC = true;` in `Reset()`; `tricnes-full-src/` is byte-identical to
  upstream and the same line was applied by hand to the instrumented
  `tricnes-harness-src/`, whose delta against the full source stays at its
  established **88** instrumentation lines.
- **A sub-test manifest note is retracted, by measurement.** It said the
  misaligned-OAM2 ROM was built from a base that "PREDATES that upstream `INC`",
  and `9bc42d1e` is the commit that **added** that `INC` — the column was right
  and the prose was backwards. Settled by rebuilding rather than by reading: the
  committed ROM reproduces byte-identically from `9bc42d1e`, which also stands
  as a demonstration that these builds are reproducible. Both recorded sub-tests
  are rebuilt on `46199ae4` and validated (`$0493` Pass on frame 128, `$0495` on
  frame 82 — the same frame the previous one-byte-patched ROM reported, which is
  the cross-check that the real build and the patch select the same test). The
  one-byte relationship the manifest describes (offset `0x10B5`, `0x02` ->
  `0x03`) is now **measured** between two builder outputs rather than asserted.
  The 31 legacy sub-tests keep their stated gap: their indices were never
  recorded, and rebuilding on guessed indices is worse than a stated gap.
- **Two stale claims in the AccuracyCoin README, both checked rather than
  assumed.** It said the uppercase directory holds "a synced copy of the ROM"
  (it holds none — zero `.nes` files), and it quoted the pass rate as
  **99.31% (143 of 144)**, which stopped being true at v2.6.18. Both corrected.
- **`VERSION-PLAN.md`'s release-history table is ordered again.** It ran
  ascending v2.0.0 -> v2.5.8, then a scrambled block
  (v2.6.2, v2.6.1, v2.6.0, v2.5.9, v2.6.5), then *descending* v2.6.21 -> v2.6.3.
  **23 of its 61 rows moved**; the fix is a pure permutation (asserted as one)
  and a sort that is a **no-op on the already-correct head**, which is what
  makes it a reordering rather than a rewrite. The `(current)` marker is
  preserved and all 15 release-anchor audits pass.
- **The two open Dependabot PRs are consolidated, and the pair they touch is
  kept in lockstep.** `androidx.baselineprofile` and `benchmark-macro-junit4`
  **1.5.0-rc02 -> 1.5.0** (that line's first stable release) and
  `compose-bom` **2026.08.00 -> 2026.09.00**. The two benchmark artifacts MUST
  move together — the root build says so in a comment — and the comment beside
  the second pin had *already* drifted, naming `1.5.0-rc01` while the code read
  `rc02`; it now names the invariant instead of duplicating the version, so it
  cannot drift again. Both PRs' Android builds were verified from the **job
  log** (`BUILD SUCCESSFUL`, both `:app:bundle*Release` tasks), not from the
  check mark, because that job is `continue-on-error: true` and a broken
  Android build leaves the PR green.

- **The README is a landing page again, and `AGENTS.md` is half its former
  size.** Both carried the complete release lineage inline — a single
  **17,882-character** paragraph in `README.md` and **two** blobs totalling
  ~103,000 characters in `AGENTS.md`, one of them repeating the other. That is
  changelog material, and it was duplicated against three places that are
  actually maintained: this file (135 release sections), the 77 per-release
  notes under `.github/release-notes/`, and the published GitHub releases.
  Verified those cover every release named in the blobs before deleting
  anything. `README.md` **74,770 → 56,676 bytes**; `AGENTS.md`
  **268,033 → 139,614**. All five release anchors the audit pins are preserved
  and `release_anchor_audit` passes.

- **Four stale accuracy figures on the landing page.** The README badge read
  `AccuracyCoin 100% (141/141)` — the current figure is **144/144**, and the
  badge is not something `release_anchor_audit` can see, since it pins only
  `badge/version-v`. The same stale number appeared in the authoritative-source
  sentence, the MiSTer-line paragraph and the BibTeX citation. The one
  remaining `141/141` is v2.0.3's own historical achievement and is correct in
  context.

- **Two Roadmap claims that stopped being true.** It said the ladder was
  "128 gates green … 148 gates as of v2.6.16" (it is **152 green, 0 failed, 1
  expected failure**), and that rung 6 was "blocked on hardware: no DE10-Nano or
  SuperStation One is attached" — **a SuperStation One is now in hand.** The
  half of that sentence which is still true, that no hardware has run any
  bitstream, is kept and sharpened rather than dropped.

### Fixed

- **The AccuracyCoin catalog has 149 rows and 144 results, and the difference
  was being counted.** Upstream defines `result_DrawTest = $03FF` with its reason
  attached — *"page 3 omits the test from the all-test-result-table"* — and all
  five `Power On State` rows share that sentinel. The sharing was already
  documented on `CatalogEntry::result_addr` and acted on nowhere, so every
  consumer counted the five as results, which made the headline **a function of
  when the run was sampled**: one ROM reads `pass_with_code=16, not_run=0` at
  4500 frames and `pass_with_code=11, not_run=5` at 6600, because `$03FF` is
  scratch the results-page renderer writes and later abandons. `RESULT_DRAW_TEST`,
  `is_scored`, `scored_len` and `scored` exclude it in one place.
  **`EXPECTED_PASS_COUNT` is untouched and the gate still reports 144/144,
  100.00%, fail=0** — the count did not change, it stopped depending on the
  sampling instant, and five rows left every "entry for entry" claim that were
  never entries.

- **The `46199ae4` re-sync was half-done for a day, and nothing could see it.**
  Oracle PR #528 moved the vendored ROM and rebuilt two sub-test ROMs without
  re-exporting the sibling's goldens, so three of them recorded a `rom_sha256`
  for a ROM no longer in the tree — one naming a build-cache path since deleted.
  **Every gate stayed green, correctly:** the re-sync's prediction that it
  "changes no verdict" holds, now measured rather than quoted — the vector is
  identical entry for entry and the RAM difference is **three bytes per golden,
  all outside the catalog**, zero-page scratch and stack, `+2` each, consistent
  with upstream's two-byte insertion. In one of them the changed byte is `$0010`,
  which the assembly calls `ErrorCode`, going `$03 -> $04`: the fix itself. The
  goldens were not wrong, they were **unattributable** — a later difference could
  not have been told apart from a corpus change. All three re-exported, gates
  re-run.

  **Widening the checker's path resolution turned a skip into a finding.** Four
  of five UNRESOLVED goldens resolved once `../`-relative and scratch-path
  manifests were handled, and the fifth of the original set turned out to be
  STALE all along, hidden behind an unfindable path. A skip count is a place
  findings hide.

- **`run_battery.sh` would have reported the AccuracyCoin mirror ROM as
  `pass`.** It read `sav[0]` as a blargg verdict for any ROM; a real mirror save
  begins `00 00 00 00`, so byte 0 of `$00` would have scored the entire battery
  as a pass. The verdict is now licensed by blargg's `$DE $B0 $61` signature at
  `$6001-$6003`, and a save without it reports `vector-not-verdict` rather than
  a judgement. Measured on a real mirror save, not reasoned about.

- **The stale 121/125 incumbent figure in `docs/submission-case.md`** is
  withdrawn rather than updated. It is pre-2026-09-15, and `NES_MiSTer` took a
  burst of AccuracyCoin-driven commits on 2026-09-15/16 covering the entries this
  line closed in v2.6.18–v2.6.20. The document now makes **no comparative
  accuracy claim at all** until row F1 of the bring-up log carries a
  measurement. The README's copy was already corrected at v2.6.21; this one was
  not.

- **The feature delta is stated by us rather than found by a reviewer.**
  `submission-case.md` now carries the table: the incumbent has save states,
  cheats, palettes, Four Score, Zapper, PAL, FDS and expansion audio; this core
  has six mapper families and battery saves. On features the incumbent wins
  outright, and it is not close.

- **Two carried-forward items in the v2.7.0 plan were already closed** when the
  plan listed them — the AccuracyCoin corpus re-sync and `VERSION-PLAN.md`'s row
  order, both done in #528. They came from `CLAUDE.local.md`'s "Open" section,
  which had not been updated after that merge: a note copied forward became a
  fact.

- **`*.sweep-snapshot` is gitignored in the sibling.** `scripts/seed-sweep.sh`
  rewrites `RustyNES.qsf` and `build_id.v` while running and restores both from
  snapshots on exit, including on interrupt — but a trap cannot fire if the
  process is killed outright, and that script's own comment records an OOM kill
  doing exactly that. What survives is a copy of the `.qsf` carrying the entire
  seed-table rationale sitting beside the live one, which a `git add -A` would
  commit: two tables disagreeing about the pinned seed, which is precisely what
  `tb/check_qsf_seed.py` exists to prevent.

## [2.6.21] - 2026-09-19 - "Steward" (the board arrives, and the core is not ready for it)

### Fixed

- **Battery-backed save RAM exists.** `T-MISTER-SAVE` has been open since
  v2.6.12 and `docs/rung6-integration.md:501` still said "Scheduled for
  v2.6.13". It never landed, so `rtl/emu.sv` tied `sd_lba`, `sd_rd`, `sd_wr`,
  `sd_buff_din`, `ioctl_upload_req` and `ioctl_din` to constants and **every
  MMC1 or MMC3 game with a battery lost its save at power-off** — Zelda, Final
  Fantasy, Kirby's Adventure, Crystalis — with no sign of it in the OSD. It is
  the one board item with a user-visible data-loss cost, and a reviewer hits it
  in five minutes.

  **The protocol contradicted the plan, and the framework settled it.** The plan
  said to use `ioctl_upload_req`/`ioctl_din` and explicitly not `sd_*`.
  `Main_MiSTer/user_io.cpp:948-955` says otherwise: a `CONF_STR` file entry with
  an **`S`** after the `F` sets `opensave`, which calls `FileGenerateSavePath()`
  then `user_io_file_mount(path, 0, 1)` — so the save arrives as **vdisk 0 over
  `sd_*`**, and that one letter is what buys `<rom>.sav` naming and
  load-on-ROM-load. `F1,NES;` becomes `FS1,NES;`.

  The mechanism is `rtl/save_ctl.sv`, its own module because `rtl/emu.sv` is in
  no testbench — which is how v2.6.12 shipped a cartridge hard-wired to mapper 0
  with 142 gates green. The policy (when to save: the rising edge of
  `OSD_STATUS`) stays in `emu.sv`, which is the only file that can see it.
  `make -C tb save-gate` round-trips 8 KiB byte-for-byte and asserts five
  **refusals** — clean cartridge, zero-length file, read-only mount, no battery,
  no vdisk — which matter more than the round trip: without them a controller
  that saves unconditionally passes and rewrites `<rom>.sav` on every OSD visit.
  **Ten mutations, nine CAUGHT, one measured inert and documented at the site.**
  The gate failed three times first, all three the host model rather than the
  DUT.

  **Two more defects came out of review, and both were reachable.** There was no
  exit from `S_SAVE` but completion, and `rst_n` is `pll_locked`, which does not
  drop on a ROM load — so a host that stopped answering left the controller
  asserting `busy` forever *with the dirty flag already cleared*, since the clear
  is at the start by design so a CPU write landing mid-save leaves it raised. And
  the `S_LOAD`/`S_SAVE` arm did not look at `img_mounted` **at all**, so a
  cartridge inserted during a transfer had its mount dropped and its save never
  read for the rest of the session. A per-block watchdog ends an abandoned
  transfer, `save_failed` holds `save_pending` high for a retry, and a mount is
  captured in every state and **outranks** a pending save — writing the previous
  cartridge's RAM into the new one's file is the only outcome worse than not
  saving. Four further mutations, all CAUGHT.

- **`prg_ram` was 8 KiB of flip-flops, and the block count was never the
  variable.** Adding the second port made Quartus refuse the design outright —
  `Error (170011): Design contains 151605 blocks of type combinational node.
  However, the device contains only 83820 blocks` — and the obvious fix made it
  **worse**: merging both ports into one `always_ff` gave 159,216 blocks and
  **217 %** ALM utilisation. Each version carried a confident comment, and the
  two comments contradicted each other about whether a dual-port memory is one
  block or two. Neither was the variable.

  The fitter named the real shape in a table nobody reads: 8192 multiplexers, 8
  bits wide, **3:1** — hold, port-A data, port-B data, which is a two-write-port
  register file described exactly — while `prg_ram` was simply **absent** from
  the Analysis & Synthesis RAM Summary that listed `wram`, `prg` and `chr`.
  **What decides it is the read-during-write style**: each port's read must sit
  in the `else` branch of that port's own write, so the port reads back what it
  just wrote, because that is what an M10K port physically does. A port that
  returns the OLD value on a write cycle is not a mode the hardware has, so
  Quartus cannot map it and builds logic instead. `wram.sv` reads
  unconditionally and infers fine *because it has one port*. With Intel's
  template: `OPERATION_MODE set to BIDIR_DUAL_PORT`, 8192 × 8, zero 3:1
  multiplexers, **0 errors and 0 warnings**, 55 % ALMs, and timing closing at
  all four corners.

  **Nothing but the fitter could have found it.** Verilator accepts all three
  forms and the save gate passed on both broken ones — correct behaviour,
  unfittable hardware — and `check_rtl_subset.py` passed on all three, because
  it checks policy rather than inference. `docs/rtl-subset-policy.md` gains the
  two-port rule beside the single-port one, with the three ways to tell before
  a fit fails and the note that `quartus_map` answers all three in ~5 minutes
  against ~25 for a full compile.

- **Eighteen gate targets were not `.PHONY`, sixteen of them invoked by
  `regress.sh`.** Review named two. A target absent from `.PHONY` is satisfied
  by a **file** of that name: make prints "up to date", runs nothing, exits 0 —
  and `regress.sh` reads exit 0 as **PASS**. Demonstrated in a throwaway
  Makefile whose `save-gate` recipe exits 1: exit 2 with no such file, exit 0
  after `touch save-gate`. So a stray file turns a failing gate into a passing
  one with no output saying so. All declared, and `tb/check_phony.py` +
  `make -C tb phony-audit` keep it that way in CI, with a five-case self-test
  and a mutation that fails it by name.

- **The runbook's corpus placement contradicted its own MGL generator.** §2.5
  named only `games/NES/tests`, the **stock** core's directory, while §4.3's
  generated launchers read `games/RustyNES/tests` — so a reader following the
  runbook in order got MGLs pointing at a directory that did not exist. Both
  paths are right for their own consumer, so both are now stated, with the
  reason not to resolve it the other way: §5.3's differential test cannot
  tolerate the development corpus inside the reference core's tree.

- **The battery script named the wrong bitstream and ignored its own
  failures.** Its provenance row took the newest file in `releases/`, which
  `make deploy` never writes — it pushes `output_files/RustyNES.rbf` — so a
  hardware result could be attributed to a bitstream the console has never run.
  It now records `rbf_md5` read **off the board** and exits rather than guess.
  Both OSD commands carried `|| true`, contradicting the fail-closed contract in
  the file's own header: with no OSD edge the core writes nothing and the script
  would read an earlier run's `.sav` and report it as this one's verdict. They
  now exit, and a freshness marker means a stale save reports `needs-capture` —
  "I could not look" must not wear the shape of "it passed".

### Added

- **The CHR-during-rendering gate, and it is RED.** `docs/STATUS.md` claimed
  v2.6.20's CHR-RAM gate "closes the coverage the retrospective audit named". It
  did not: the audit named writes **while rendering is enabled**, and PROGRAM53
  writes with `PPUMASK = 0` and renders afterwards. PROGRAM54 writes mid-frame,
  and the answer is **32,861 of 61,440 pixels differ** from the oracle with
  `chr_wr` asserted 4,389 times.

  The diagnosis is recorded at `rtl/ppu2c02.sv`: `chr_wr_addr` takes
  `chr_addr_raw`, which is the **bus** address — `fetching ? bg_fetch_addr : …
  v_addr`. Outside rendering the mux falls through to `v`, so a write and a
  fetch name the same signal, which is why v2.6.11 fixed the rendering-OFF case
  and left this one. **The obvious fix is not sufficient, and that is the useful
  part**: pointing it at `v_addr` recovers only **715** of those 32,861 pixels,
  so there is at least one more mechanism. Not landed, because an unverified RTL
  change would also force a full seed re-sweep. The increment was checked first
  and is already correct — the documented simultaneous coarse-X + Y increment
  during rendering is implemented.

  Registered red on the precedent `ppu-misc-ale-read` set: a gate that is red
  for a documented reason is worth more than a question that is expensive to
  ask.

- **Two gates existed that nothing ran.** `menumask-gate` has been committed
  since v2.6.13 and appeared **nowhere** in `regress.sh` — v2.6.8's finding
  ("three of them were not run by the suite AT ALL") in a different corner. It
  and `save-gate` are both registered now.

- **The deploy loop the runbook prescribed and the repository never had** — a
  root `Makefile`, `tools/gen_mgl.sh`, `tools/run_battery.sh`. Tiers 1-4 of
  `docs/HARDWARE_TESTING.md` all depend on pushing a build and launching a ROM
  without touching the OSD, so the absence is why nothing in §5 had ever run.
  **Building to the spec found two defects in the spec**: the MGL example and
  default said `index="0"` while `emu.sv:503` gates on `1`, so every ROM would
  have gone to a slot nothing decodes — a black screen, no error, an entire
  battery reporting nothing; and the corpus path said `games/NES`, the stock
  core's directory, the one tree the differential test must not contaminate.
  Both scripts fail closed and are self-tested.

- **`scripts/accuracycoin-build/derive_indices.py`**, because the hand-written
  suite map in `build_sub_test_rom.py` was **wrong from index 14 onward**: it
  listed twenty suites with `PowerOnState` at 14, and upstream has twenty-two
  with `CPUBehavior2` at 14 and `PPUMisc` at 19. A rebuild driven by it enters
  the wrong suite and writes a plausible byte for a test nobody asked for. Of
  its four recorded targets three were right and `Implied Dummy Reads: suite=19`
  was not — it is suite **14**. The tool derives from the assembly and validates
  itself against two independently recorded answers before reporting any others.

  **Its first version emitted 145 rows for a 149-entry catalog and said nothing**
  — the shortfall raised in review as a hypothetical ("if multiple tests happen
  to share the same result address"), and measured to be already happening. All
  five `Suite_PowerOnState` tests name `result_DrawTest`, and keying a map by
  that address kept only the last: `CPU RAM`, `CPU Registers`, `PPU RAM` and
  `Palette RAM` were dropped. Upstream's own comment says what the value is —
  `result_DrawTest = $03FF ; page 3 omits the test from the
  all-test-result-table` — so **$3FF is a sentinel meaning "this test has no
  result byte", not a location**, and a caller that read a verdict there would
  get whatever the last test wrote. The rows are now a flat list carrying a
  `has_result` column, and the tool **exits** rather than emit a short catalog:
  dropping one entry gives `149 catalog entries but 148 rows -- 1 lost`. That a
  tool built to replace a silently-wrong hand-written map shipped its own silent
  shortfall is the finding worth keeping, not the four rows.

### Changed

- **`rtl/*.sv` indented with tabs**, all 22 files. MiSTer's coding guidelines —
  the page the contribution wiki links — say "Indent with tabs, not spaces", and
  30 of the 32 vendored `sys/` files already do.

- **Three documents a reviewer reads had stale numbers** that `fe71a63` fixed
  only in `HARDWARE_TESTING.md`: `README.md` 146 → **149** and 141/141 →
  **144/144**, `docs/submission-case.md` 141/141 → **144/144** — the document
  the submission email links.

- **The incumbent risk got worse while nobody looked.** `README.md` said
  `NES_MiSTer` scores 121/125 against hardware's ~121/125. On **2026-09-15/16**
  the incumbent published a burst of accuracy work. There may be **no accuracy
  headroom at all**, and the number will be re-measured against the current
  incumbent on the same corpus before any submission.
  *(Redacted at v2.6.22 under
  [ADR 0040](docs/adr/0040-public-release-metadata-is-outside-the-reference-firewall.md):
  this entry originally quoted three commit subjects and named a capability of
  the incumbent's core.)*

- **A guard that would have fired on success.** `contribution_checklist_audit.rs`
  asserted `unticked > 0` because "rung 6 needs hardware nobody here has". A
  SuperStation One is now attached, so that reason is false and the assertion
  would have turned v2.7.0's milestone into a red test. Replaced with a narrower
  one that survives the submission.

- **Expired prose, five sites.** Three task-board boxes done and never ticked
  (the `$4017` rewrite closed at **v2.6.2**; the nestest 5 M window at v2.6.7/8;
  a checklist tally stale by two), a harness comment calling two passing gates
  failing, and `mkrom.py`'s claim that PROGRAM32 "is deliberately NOT a gate".
  One audit claim was **refuted** rather than applied: `TASKS.md`'s "142 of 142"
  is a dated statement about v2.6.13, correct in context.

- **The AccuracyCoin corpus re-sync is DEFERRED with its reason.** Upstream
  `46199ae4` is this project's own issue #66 fix, accepted and closed six seconds
  after the push. It changes **165 bytes**, so all 33 sub-tests rebuild, every
  sibling golden re-exports and both sides re-verify — and it changes no verdict.
  A corpus half at `9bc42d1e` and half at `46199ae4` is the mixed-provenance
  state this project treats as evidence-destroying, so it is all or nothing.
  Measured alongside: `wine nesasm.exe` reproduces upstream's committed `.nes`
  **byte-identically**, which is the control any future rebuild needs.

### Build

- **The ladder is 152 passed, 0 failed, 1 expected failure.** The expected one
  is `chrram-live`, registered with the new `run_xfail` rather than as a plain
  gate: a permanently red gate makes the suite report "N passed, 1 failed" every
  release, and the next genuinely NEW failure prints the identical line.
  `run_xfail` runs the gate in full and **fails the suite if it starts
  passing**, so a divergence that closes cannot hide behind a stale allowance.

- **The fitter pin moves 1 -> 5, and the seed that had been pinned for seven
  releases no longer closes at all.** The RTL changed -- `save_ctl.sv`, and a
  second port taking `prg_ram` from single-port to `BIDIR_DUAL_PORT` -- so the
  table is re-derived rather than carried. Ten seeds, each from a clean
  database, all at one pinned build date, worst across all four corners:

  | seed | worst setup | worst hold | result |
  |---|---|---|---|
  | 1 | +0.307 | +0.068 | closes |
  | 2 | +0.233 | +0.078 | closes |
  | 3 | **-0.180** | +0.073 | **does not close** |
  | 4 | +0.443 | +0.079 | closes |
  | 5 | +0.259 | **+0.102** | closes — published |
  | 6 | +0.218 | +0.076 | closes |
  | 7 | +0.135 | +0.045 | closes |
  | 8 | +0.456 | +0.071 | closes |
  | 9 | +0.245 | +0.093 | closes |
  | 10 | +0.159 | +0.080 | closes |

  Hold binds on every closing seed and seed 5 takes it outright. **Seed 3 was
  the pin from v2.6.13 through v2.6.19** and the worst of v2.6.20's ten; it now
  fails outright, so a pin carried forward on an older RTL's table would have
  shipped a bitstream that does not meet timing.

  It also restores a counterexample v2.6.20 recorded as spent. That release
  noted seed 8 failing on v2.6.19's RTL and closing on v2.6.20's, and said the
  *specific* counterexample was gone while the principle stood -- that whether
  every seed closes is a property of the RTL, re-measured each time. A different
  seed fails one release later. The principle would have been unfalsifiable had
  the previous entry dropped it when its evidence expired.

- **`releases/RustyNES_20260919.rbf`** — 4,010,492 bytes, md5
  `d20c6dd3f58c58a13ca84c3b823de2a7`, 0 errors and 0 warnings, all four corners
  closing at **+0.259 ns setup / +0.102 ns hold**. The independent from-scratch
  compile at the pinned seed reproduces the sweep's seed-5 row exactly, which is
  the property v2.6.7 had to withdraw a published number over.

### Not established here

No hardware has run any bitstream. A SuperStation One is in hand and rung 6
opens at v2.7.0; this release is the pre-flight that makes bring-up start from a
base whose records are true.

## [2.6.20] - 2026-09-18 - "Telltale" (the counter had no reader, and two knobs turned out to be one decision)

### Fixed

- **`Misaligned OAM2 Address` closes: the DUT reaches 149 of 149.** The FPGA
  core's `oam2_fetch_addr` was **write-only** — a real counter, reset at dots
  63/255/339, incremented across the sprite-fetch window, whose wrap raised
  `oam2_overflowed`, and which **nothing ever read**. `$2004`'s post-fetch rest
  value was a hardcoded `sec_oam[0]`.

  AccuracyCoin states the rule in two sentences the ROM itself carries: the OAM2
  address "overflows … on dot 321", and when OAM2 is full the PPU "prevents
  further increments … frozen at index 0". Read together, the documented
  "reads OAM2[0] during 321-340" is **not a constant** — index 0 is where a
  COMPLETE window leaves the counter. An interrupted window stops elsewhere, and
  that is exactly what this entry reads.

  The fix is a one-shot at documented dot 321 assigning `sec_oam[oam2_addr_next]`,
  a combinational next-value view. The view is necessary rather than tidy:
  `oam_bus` uses the documented-minus-one convention while the counter block uses
  documented dots, so reading the register at that edge yields its pre-320 value
  (31 ordinarily) and would put sprite 7's X byte on `$2004` for dots 321-340 of
  every line of every game. In the ordinary case `31 + 1 = 0`, so every fully
  rendered scanline is byte-identical.

- **THE INCREMENT WINDOW GOES BACK TO 256, AND v2.6.19 HAD IT WRONG.** That
  release narrowed it to 258 on a review argument — "from 256 there are
  THIRTY-THREE increments and the wrap lands on dot 318" — whose arithmetic is
  right and whose conclusion is not: the 33rd candidate is suppressed by
  `!oam2_overflowed`, so both windows perform 32 increments and differ only in
  **phase**. Its own comment predicted the blind spot that hid it — "mostly
  invisible … nothing downstream can tell 318 from 320" — and it was invisible
  because **nothing read the counter**.

  The fix alone left the DUT reading `$A3`. The oracle's v2.6.18 study had
  already tabulated that byte as OAM2 index `$17`, **exactly one increment
  short** of `$18`, so the second change was named by a measurement rather than
  searched for.

- **AND THE TWO KNOBS ARE ONE DECISION.** Restoring 256 broke sprite rendering:
  **6 of 61,440 pixels** in `sprite-render` and the same 6 in `sprite-mask`,
  identical bounding box. `oam2_overflowed` is consumed by the four sprite-fetch
  read sites, so moving the wrap from dot 320 to dot 318 puts it INSIDE the
  fetch, where the live flag forces index 0 for the last sprite.

  v2.6.19 had also deleted the dot-257 freeze latch, on a reviewer's argument
  that rested explicitly on the 258 window ("with the wrap corrected to dot 320,
  reading the live flag cannot disturb a fetch that has already concluded"). So
  window-258-without-latch and window-256-with-latch are two self-consistent
  packages, and only the second is the hardware's: the oracle pairs 256 with the
  latch and passes both entries. The latch is restored, sprite fetch consumes
  it, and `$2004` reads the live counter — three consumers, two states, each
  named at the site.

  Result: `$0495` **both Pass**, and the byte at zero page `$50` is `0x06` on
  both sides — the gate passes on the right value rather than by coincidence.

### Added

- **A sub-test ROM for `Misaligned OAM2 Address`, built by a ONE-BYTE patch.**
  The entry is the last catalog row, so the full battery is 134 M cycles
  (~35 min) per attempt, and no sub-test existed (the corpus fixture named
  `sprite-eval-misaligned-oam.nes` is a **different** test, $045A).
  `build_sub_test_rom.py` injects the suite and test indices as plain
  immediates and they survive into the binary: in
  `advanced-sprite-eval-frozen-oam2-increment.nes` they sit at file offsets
  `0x10AB` (`LDY #21`) and `0x10B5` (`LDX #2`). Two independent sources agree
  that Misaligned is index 3 — `BUILD-PROVENANCE.tsv` and the catalog's own row
  order — so `0x02` → `0x03`, validated at `Pass` on frame 82 before use.
  **35 minutes became 30 seconds**, which is what made two experiments
  affordable instead of one.

- **`advanced-sprite-eval-frozen-oam2-increment` is registered as a gate.** Its
  golden already existed and it was named nowhere in `regress.sh`. It is the
  flag half of the same mechanism and the control for every step above.

- **A CHR-RAM write gate — the coverage the retrospective audit named and did
  not close.** `chr_wr` fires constantly in the gated corpus (four of six
  rung-7 mapper ROMs are CHR-RAM), and **no CHR-RAM ROM's consequence was ever
  rendered and compared**: all three framebuffer gates ship CHR-ROM by
  construction, and the mapper ROMs never enable rendering. That is how
  v2.6.11's CHR-write defect passed 141 green gates and surfaced only in a
  hand-run montage — UxROM 16,565 wrong pixels, AxROM 1,702, every CHR-ROM
  board 0.

  `mkrom.py` program 53 writes 4 KiB of patterns through `$2007` and then
  renders them, so every pixel it draws is a byte the CPU wrote. Reverting the
  exact v2.6.11 defect now fails it with **28,191 of 61,440 pixels**. The gate
  asserts `chr_wr assertions:` is **non-zero**, because a stimulus that never
  reaches the path reports a pass about nothing.

### Changed

- **The submission checklist is re-audited**, three releases after it was last
  touched. `.srf` is ticked — v2.6.19 adopted it, and the box's own plan said
  "the next release that rebuilds the bitstream". **Two count-bearing boxes were
  re-measured and are CORRECT**, recorded so a third audit does not repeat the
  work: `31 RTL files` is `rtl` + `tb` (22 + 9) and `40 HDL files` in `sys/` is
  `.sv` + `.v` + `.vhd`. Both looked expired under a narrower `find` than the
  one that wrote them — which is what that box's own last sentence warns about.

- **An expired claim in the accuracy gate.**
  `crates/rustynes-test-harness/tests/accuracycoin.rs` said "the single
  remaining failure is `Frozen OAM2 Increment`" while `KNOWN_FAILING` beside it
  has been `&[]` since v2.6.18. The list was emptied and the sentence above it
  was not.

- **Reported upstream:** `TEST_FrozenOAM2Inc` omits an `INC <ErrorCode` between
  tests 3 and 4, so both report `$0E` and `Fail(3)` cannot be told from
  `Fail(4)`. Verified against upstream `main` before filing — two `INC` sites
  against three failure exits — and it is the identical defect upstream already
  fixed for the neighbouring routine (100thCoin/AccuracyCoin#64).
  Filed as 100thCoin/AccuracyCoin#66.

### Build

- **The bitstream is rebuilt and the fitter pin moves 3 -> 1.** The RTL changed,
  so the seed table is re-derived rather than carried: ten seeds, each from a
  **clean database**, all at one pinned build date (`BUILD_DATE` is a constant
  in the design, so a sweep that crosses midnight compares two designs). All ten
  close at full effort; hold binds on every one of them (0.042-0.113 ns against
  setup's 0.070-0.645), and **seed 1 takes it outright at +0.113 ns** with no tie
  to break.

  **Seed 3 — the pin this release inherited — is now the worst of the ten**, at
  +0.042 ns. Carrying it forward would have shipped a factor of **2.7** less
  binding margin for free. v2.6.19 re-swept and the pin did not move, which
  reads as ceremony until the release where it does; this is that release, and
  it is the one-table-per-RTL rule paying for its compiles.

  One claim in that block is **weakened by measurement and said so rather than
  quietly kept**: seed 8 did not close on v2.6.19's RTL (-0.008 ns) and was the
  specific counterexample behind "the distribution is mostly positive, not
  entirely". On this RTL it closes at +0.405 and all ten pass, so the claim
  stands in principle and its evidence is gone.

- **`releases/RustyNES_20260918.rbf` ships on both repositories**, per the rule
  v2.6.7 set: the MiSTer distribution mechanism reads that path out of the
  repository, so an empty `releases/` describes an undistributable core rather
  than a cautious one. **No hardware has run it** — rung 6 still waits on a
  DE10-Nano with the SDRAM add-on and a SuperStation One, confirmed by checking
  rather than assumed.

## [2.6.19] - 2026-09-16 - "Accession" (the DUT absorbs two releases of oracle behaviour, and the seed rule catches something for the first time)

### Added

- **The FPGA core implements the OAM2 address counter, which did not exist.**
  `sec_addr`/`sec_fetch` were combinational from `dot` and served only as the
  OAM-corruption seed: no 63/255/339 reset, no overflow flag, no dot-257 latch.
  All three land, written from AccuracyCoin's own prose — a test ROM is
  stimulus, not a reference implementation, and no third-party core was read
  (ADR 0037 applies). **There is ONE live flag, `oam2_overflowed`, and sprite
  fetch reads it directly at all four read sites.** An earlier draft of this
  release latched it at dot 257 into a second `oam2_fetch_frozen`, on the
  reasoning that a wrap after 257 must not disturb a fetch already in progress
  — and this entry described that design until after it had been published,
  which is corrected here rather than quietly replaced. The latch is wrong
  about the rule it implements: AccuracyCoin says rendering re-enabled ON OR
  AFTER dot 256 leaves the fetch reading index 0, and a single sample at 257
  misses every re-enable later than 257 — one at dot 260 found the flag clear
  and fetched normally. It is also unnecessary once the increment window is the
  32 even dots 258..320 rather than 33 from 256, because the counter can then no
  longer wrap before the fetch window ENDS. Two defects, and the latch was
  masking the other one.

  The counter is live and carried across scanlines and **is not yet the read
  pointer** — the fetch still indexes by dot except when the flag forces index
  0. That gap is this release's one declared divergence, and making the fetch
  read `oam2_fetch_addr` directly is the remaining work.

- **`RustyNES.srf`**, the message-suppression file the MiSTer template ships and
  the contributing wiki lists among a core's standard files. Four rules, one per
  warning this design emits, **each attributed** — and deliberately **none of
  the template's three `"*"` wildcards**. One of them blanket-suppresses ID
  `276020`, "Inferred RAM node…", which is the message class that exposed both
  v2.6.10's 128 KB of CHR in flip-flops and v2.6.6's M10K finding. Importing it
  would blind the project to exactly the messages that have caught its two worst
  synthesis defects.

### Fixed

- **The DUT's dot-256 vertical increment acted on a mask change landing during
  dot 256.** v2.6.18 established the rule in the oracle — a `$2001` change taking
  effect *during* dot N must not act on dot N — and the FPGA core had never seen
  it. The increment was gated on the one-dot `rendering` view; the two-dot view
  already existed as `reload_render` and was consumed only by the shift reload,
  so the fix is one signal. AccuracyCoin on the DUT goes to **148 of 149**, the
  remainder being `Misaligned OAM2 Address`.

- **An overflow is an EVENT, and writing it as a STATE regressed two entries.**
  The second raise path — evaluation filling secondary OAM — was first written
  as the level `sec_idx >= 32`. `sec_idx` is cleared once a line, at the end of
  the dot-1..64 clear, so once OAM2 fills the level stays true through dots 255,
  256, 257 and 339: the dot-255 reset cleared the flag and dot 256 put it
  straight back, so dot 257 latched `frozen` on **every line carrying eight
  sprites** and every sprite fetch read OAM2[0]. `INC $4014` and `Sprites On
  Scanline 0` broke; the battery went from 2 differing entries to 3. Raised on
  the increment that reaches 32 instead, both entries recover on the same run.
  The one-minute sub-test said "both Pass" throughout — it was the instrument
  incapable of seeing it, which is why the 134 M-cycle battery is the gate.

- **A killed seed sweep reported a design failure, and could eat the `.qsf`.**
  The snapshot lived in `/tmp`, which is tmpfs — so the event most likely to kill
  the sweep can take the backup with it, and did: the trap never ran and the
  working tree was left on a machine-written `SEED`. It now sits on real disk and
  **self-heals**. Separately, a compile killed from outside was classified
  `COMPILE FAILED`, a claim about the *design*; three outcomes sharing one exit
  code are now separated by reading the log's own vocabulary. And `SEEDS=""`
  silently ran all five, because `${SEEDS:-…}` treats empty as unset.

- **`tb/quartus_clean.py` failed every compile once the `.srf` existed.** Its
  anti-vacuity guard read "no warnings at all" as "wrong file", which was right
  in a world with no `.srf` and wrong in this one. The property it protects was
  never "a warning exists" but "this log is a compile that ran", so it asks that
  directly now — the four stages a `--flow compile` must report — and still fails
  on a truncated log, an abort, and any message citing `rtl/` or `tb/`.

- **The browser build stopped opening the Netplay pane at you.** Loading a ROM
  force-opened "Netplay (browser)" on every wasm session. The force-open was
  also the only route to the lobby, because the Netplay menu item was
  `cfg(not(target_arch = "wasm32"))` — so removing it alone would have made the
  feature unreachable rather than unobtrusive. There is now a wasm menu entry in
  the same group with the same `WIFI` glyph, enabled once a ROM is loaded, and a
  source-shape gate pins both halves against each other.

### Changed

- **The fitter seed is re-derived on v2.6.19's RTL and does not move — after
  three sweeps were superseded.** `RustyNES.qsf` requires that every published
  seed table describe one RTL, so v2.6.19's added registers supersede
  v2.6.13's. Three tables preceded the one that stands:

  1. The first recompiled **in place**, so each seed was measured against the
     *residue of its predecessor's* placement database. It reported that seed 1
     fails setup by twelve picoseconds and does not close, and the conclusion
     drawn — "the one-table-per-RTL rule has caught something for the first
     time" — **is retracted**. Seed 1 closes.
  2. The second cleaned the database per seed and **crossed midnight**.
     `sys/build_id.tcl` is a pre-flow script that rewrites `build_id.v` at the
     *start* of every compile and `emu.sv` puts `BUILD_DATE` into `CONF_STR`, so
     the date is a **constant in the design**, not metadata — two compiles on
     different days are different designs, which v2.6.15 had already measured
     (pinning the date reproduced a published `.rbf` byte for byte). The seed-1
     run started at 23:54 and every other seed ran after midnight, which is why
     seed 1's sweep row and its own shipping build disagreed.

  3. The third cleaned per seed and pinned the build date, and produced a
     usable five-seed table — **which three later commits in this same release
     then superseded.** Removing the dot-257 freeze latch and moving the DC
     blocker's `FRAC` are RTL changes, and by this block's own rule an RTL
     change replaces the design, so a table measured before them describes
     something that no longer exists. Nothing flagged it; it was caught by
     re-reading the rule against the commit log.

  Re-swept on the final RTL — **ten seeds**, each from a clean database, all at
  the same pinned build date `260917`:

  | seed | 1 | 2 | **3** | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
  |---|---|---|---|---|---|---|---|---|---|---|
  | setup | +0.453 | +0.439 | +0.317 | +0.267 | +0.523 | +0.269 | +0.243 | **-0.008** | +0.256 | +0.311 |
  | hold | +0.067 | +0.098 | **+0.108** | +0.105 | +0.078 | +0.086 | +0.062 | +0.098 | +0.085 | +0.078 |

  **The pin stays at 3**, now holding the largest binding margin outright
  (+0.108 ns) rather than a tie broken on setup. Two things the wider window
  bought that five seeds could not:

  - **Seed 8 does not close** — setup -0.008 ns. This block has asserted since
    v2.6.10 that full fitter effort "moves the whole distribution across zero
    and the seed only picks where in it you land". That is **false on this
    RTL**, and only the narrow 1-5 window made it look true. The distribution
    is mostly positive, not entirely positive, and the pinned seed is therefore
    load-bearing rather than merely preferred.
  - **The third and fourth sweeps are a controlled comparison** — same script,
    same clean-per-seed discipline, the same pinned build date, differing only
    in the three RTL commits. Every row moved; seed 3 alone went +0.611/+0.103
    to +0.317/+0.108. That is the one-table-per-RTL rule *demonstrated* rather
    than asserted — which is what this release's codename claims, and what its
    first, contaminated attempt failed to earn.

  What made every stale input findable rather than dismissable as noise is that
  the build is **deterministic**: the shipping configuration produces a
  byte-identical `.rbf` across independent clean compiles. And the half that
  generalises is not about Quartus — **a measurement that confirms a rule you
  are about to publish deserves the same scepticism as one that refutes it**,
  and a measurement stays current only until the thing it measured changes.
  Every error here was invisible in any single run and obvious the moment two
  runs were compared.

- **What this release was verified against.** The co-simulation ladder is
  **147 of 147, 0 failed**, with no skipped rows. AccuracyCoin on the DUT is
  **148 of 149** — `fail=0`, coverage 149 of 149 entries executed on both sides
  — and the single differing entry is `Misaligned OAM2 Address`, the one that
  tests the read-pointer half above. Both were re-run on the FINAL RTL rather
  than inherited from earlier in the release. The emulation core is unchanged,
  so **AccuracyCoin 144/144 and nestest 0-diff hold by construction**. The
  bitstream ships as `RustyNES_20260917.rbf` (seed 3, 4,018,912 bytes),
  attached to the GitHub release on **both** repositories, and it is the one
  built from the final RTL — an earlier staged copy predated three later
  commits and was rebuilt before merging.

  **No hardware has run it.** The PPU gate compares the pre-palette index and
  the APU gate per-channel integer levels, so the palette, the video timing
  constants, the absolute audio level and its band-limiting sit downstream of
  every gate, unverified by construction.

- **`cpu_interrupts_v2` is ticked, three releases late.** `to-dos/mister/TASKS.md`
  read "DEFERRED — not started" while `docs/mister.md` had said since v2.6.15
  that the five ROMs are verdict gates. Settled by running the ladder rather than
  by reading either: all five pass, `blargg verdict $00`. Work already done and
  never ticked, for the sixth recorded time in this programme.

## [2.6.18] - 2026-09-12 - "Errata" (the recorded cause was wrong in three ways, and the last AccuracyCoin entry closes)

### Changed

- **AccuracyCoin re-synced to upstream `9bc42d1e`, and the vendored TriCNES
  commit id corrected.** Upstream shipped a one-line fix on 2026-09-11 — a
  missing `INC <ErrorCode` after test 2 of `Misaligned OAM2 Address`. It does
  not change pass/fail, but it means **every failure code that entry reported
  from test 3 onward was one too low**, so diagnostics recorded against it are
  off by one. The catalog is untouched (zero `table "..."` lines in
  `69c88608...9bc42d1e`), so the assigned count stays 144 and the battery still
  reads **143/144** — verified, not assumed.

  The vendored TriCNES source was **already** at upstream head `f388af0b`
  (byte-identical, checked), while `NOTICE` and
  `docs/originality-and-provenance.md` still cited `9199870`. Both now state
  the two ids **separately**, because they mean different things: models were
  ported from `9199870`, the vendored oracle is `f388af0b`. One id for both is
  what let the record go stale when the vendored copy moved on.

- **New sub-test ROM: `Frozen OAM2 Increment`** — the project's only remaining
  AccuracyCoin failure had **no** standalone sub-test, so every verdict on it
  was a one-bit read of a single battery status byte and a ±1-dot experiment
  was uninterpretable. Built with upstream's own `nesasm.exe` under wine (the
  author's toolchain, so the output is faithful rather than equivalent), and
  validated: it reaches `$0493` at **frame 63** and reports `0x0A` = `Fail(2)`,
  agreeing with the battery. That turns a 7000-frame question into a 63-frame
  one.

- **`sub-tests/BUILD-PROVENANCE.tsv`** records how a sub-test ROM is built.
  The `(suite, test)` indices were previously recorded nowhere, so a rebuild had
  to guess them from the filename — and a slug-matching pass over the 31
  existing ROMs mis-resolved several (`cpu-open-bus` and `open-bus` both to
  suite 0 test 6). Those 31 are therefore **deliberately not rebuilt**: guessing
  indices would silently ship ROMs entering the wrong test, which is strictly
  worse than a stated gap. The manifest records the new ROM exactly and marks
  the rest as unrecorded.

- **`build_sub_test_rom.py` resolves wine by absolute path.** It invoked a bare
  `wine`, and on this machine `/usr/local/bin/wine` is a symlink to
  `/usr/bin/firejail` that shadows the real binary — `wine --version` prints
  `firejail version 0.9.80`. The script now probes candidates and accepts only
  one that answers like wine, failing with the reason otherwise. Verified: it
  finds `/usr/bin/wine` with no `PATH` help and rebuilds the new ROM
  byte-identically.

  Reviewers split on this and the disagreement is recorded rather than
  averaged: one asked for the `PATH` result to be tried FIRST so a custom wine
  keeps its precedence, the other for `PATH` to be dropped entirely as an
  untrusted search path (CWE-426). `PATH`-first is exactly backwards for a
  script whose reason to exist is a `PATH` entry shadowing the real binary — but
  the need behind it is real, so it is served by `--wine` / `RUSTYNES_WINE`, an
  override the operator sets deliberately rather than inherits. An explicit
  override **fails hard** if it does not identify as wine, found by a negative
  control here: `--wine /usr/bin/firejail` previously fell through and built
  with something the operator had not asked for.

- **Dependency refresh: 53 crates, one Gradle train, one action, and two
  coverage gaps.** Consolidates the seven open Dependabot PRs and everything
  else the project could move, rather than merging them one at a time -- which
  is not a stylistic preference here, because three of the pins are COUPLED and
  a per-artifact merge desyncs them.

  **Cargo (53 crates).** `cargo update` to the latest 1.96-compatible versions,
  which covers all three crates of the grouped production PR (`toml` 1.1.4 ->
  1.1.6, one further than proposed; `ureq` 3.4.0 -> 3.4.1; `cc` 1.4.4 ->
  1.4.5). The coupled one is `wasm-bindgen` 0.2.127 -> 0.2.128: the CLI version
  in `crates/rustynes-frontend/web/Trunk.toml` must equal the library in
  `Cargo.lock` exactly, and a mismatch fails `trunk build` and the Pages deploy
  while **wasm clippy still passes** -- so nothing but this pin would have
  caught it. Bumped in the same change.

  **Android.** The five Gradle bumps, plus the THREE pins Dependabot could not
  know to move with them. The third was found in review, and it is this
  change's own defect: the refresh bumped `androidx.glance:glance-material3`
  to 1.3.0-alpha02 and left `androidx.glance:glance-appwidget` at alpha01 --
  one Jetpack library split across two artifacts, desynced by exactly the
  mechanism this entry is about. Corrected, and the correction is now enforced
  rather than remembered: `.github/dependabot.yml` groups the AGP artifacts,
  the benchmark/baselineprofile pair and the Glance pair, so Dependabot raises
  each set as one PR instead of leaving the halves to be matched by hand.
  The other two: `com.android.test` shares AGP's version coordinate
  (its own comment says so), and `androidx.baselineprofile` tracks
  `benchmark-macro-junit4`. Dependabot raises each artifact separately, so
  merging its `com.android.application` 9.4.0 PR alone would have left
  `com.android.test` at 9.3.2. AGP's recorded BUILD SUCCESSFUL measurement is
  deliberately left stated at 9.3.2 rather than reworded to 9.4.0: no Android
  toolchain exists on the machine that made the bump, and a measurement nobody
  re-ran must not be re-attributed to a version nobody tested it on. CI's
  Gradle bundle job is what re-establishes it.

  **GitHub Actions.** `taiki-e/install-action` 2.87.0 -> 2.87.11 (Dependabot
  proposed 2.87.5). Every other action was already at its latest major, and
  **both SHA pins already resolve to the current tag** -- `actions/checkout`
  to v7.0.1 and `dtolnay/rust-toolchain` to v1 -- checked against the API
  rather than assumed. The three `pre-commit` hook pins are likewise already
  latest.

- **Dependabot was blind to `crates/rustynes-cosim`, and had been all along.**
  That crate is excluded from the workspace on purpose, so it carries its OWN
  `Cargo.lock` which the `/` cargo entry cannot reach -- four crates had
  drifted with nothing watching them. Added a `/crates/rustynes-cosim`
  directory entry and updated the lock. The exclusion is deliberate and stays;
  the blind spot it created does not. Same shape as every "the gate does not
  reach the code" finding in this project, one layer out into the tooling.

- **The egui 0.36 / wgpu 30 hold is RE-MEASURED, not re-asserted.** The note
  said "0.36.1 is the newest on crates.io as of 2026-08". 0.36.2 shipped
  2026-09-08 and **still carries the blocker**: a three-line scratch crate
  depending on `egui-winit = "0.36.2"` with this project's exact feature set
  fails `cargo check --target wasm32-unknown-unknown` with the
  same `E0407` -- method "bytes" is not a member of trait
  `egui::DroppedFile` -- and 0.36.2's `NativeFile` still implements `bytes()`
  with no cfg gate. Recorded with the reproduction, because re-checking an
  exclusion at the version that ships rather than the one that wrote it is a
  lesson this project has already paid for -- and the isolated repro costs two
  minutes against a full migration.

### Added

- **`Frozen OAM2 Increment` — the single remaining AccuracyCoin failure — is
  now CLOSABLE, and what it costs is measured.** Not adopted: closing it trades
  one failing entry for another, so the battery still reads 143/144 and the
  shipped configuration is unchanged. What changed is that the question is no
  longer open-ended.

  **The access placement is dot-quantised.** `Bus::run_ppu_to` advances the PPU
  in whole dots, so a sub-dot change to an access split cannot be observed by
  anything — and the existing sweep had been saying so for two releases, with
  `READ=0`/`READ=2` and `WRITE=2`/`WRITE=4` producing identical counts AND
  identical gained/lost sets. The shipped read and the shipped write are in the
  **same dot**; the "half a dot" between them is invisible by construction.
  That reduces the placement question to nine cells, of which three had ever
  been measured, and all nine now are.

  **Each entry constrains a different thing.** The six NMI entries fail only at
  `read = dot2`; `Stale Sprite Shift Regs` passes at exactly the three diagonal
  cells (`read == write`); `Arbitrary Sprite zero` fails only at
  `write = dot2` with `read != dot2`; `Misaligned OAM2 Address` fails at
  exactly the four cells where the spacing is `±1`; and `Frozen OAM2 Increment`
  passes at exactly the two where the spacing is `+1`. The last two are
  contradictory as rules — and both are OAM2 entries.

  **The contradiction had a cause, and it was not the CPU.** The OAM2
  machinery's effective gate is a **conjunction** — its own mask test AND the
  enclosing 1-dot render gate. For an AND of two delayed views of one signal the
  DISABLE edge fires at the shallower depth and the RE-ENABLE edge at the deeper
  one, so the two knobs each own **one edge** and neither alone can place the
  window. The two OAM2 entries were never contradictory as rules — they were
  entangled by the nesting, and the placement grid was reading that entanglement
  through the only knob it had. Give the OAM2 term a delay and the apparent
  contradiction dissolves: both entries pass together for the first time.

  **With a dedicated four-stage history it closes.** At the shipped placement,
  `RENDER_GATE_LAG = 2` with `OAM2_GATE_LAG = 3` passes `Frozen OAM2
  Increment`, `Misaligned OAM2 Address` and `Arbitrary Sprite zero`, losing
  only `Stale Sprite Shift Regs` — 143/144 with a different single failure, and
  no access moved at all.

  **The remaining gap is one edge of one signal.** `Stale` fails at
  `Fail(5)` — the dot-339 assertion — and giving the dot-339 sprite-counter
  re-arm its own depth does NOT recover it (`SPRITE_REARM_LAG` changes outcomes
  at `render = 1`, so it reaches the path, and is inert at `render = 2`). The
  re-arm is the DISABLE edge; what is left is the re-ENABLE edge, which the
  ROM's own comments place at "around dot 161 or 162" of scanline 4.

  Every knob added here defaults to the shipped value; the default build
  compiles `const` paths and is unchanged, and `AccuracyCoin 143/144 (99.31%,
  RAM decoder)` plus nestest 0-diff are verified rather than asserted.

- **A third hypothesis refuted, and the trap that nearly hid it.** Giving the
  dot-256 vertical increment its own `$2001` depth (`SCROLL_GATE_LAG`) was the
  predicted fix: the ROM enables rendering ON dot 256 and states the vertical
  scroll is NOT incremented, and the tree's own note says our enable lands a dot
  early so `inc_vert_v()` fires and `v` ends at fine-Y 3 where the test needs 2.
  **It changes nothing** — 143/144 at depths 0, 1, 2 and 3, with nothing gained
  and nothing lost.

  The first run was inert for a different reason, and it is the **third** time
  this project has paid for the same trap: `tick_visible_render_fast` performs
  its own dot-256 `inc_vert_v()`, and `fast_dot_paths_valid()` did not exclude
  the new knob, so the sweep measured a configuration it was not in. Fixed, and
  the guard now names all three knobs. The null survived that fix, so it was
  confirmed the only way it can be — by mutation: making the hoisted block never
  increment costs **14 tests**, which proves the block runs and the knob is live.

  That matters more than the null. If the enable really landed a dot early,
  depth 2 would have skipped the increment and moved something. It did not, so
  **the recorded diagnosis is itself now in question**, and measuring the actual
  `$2001` transition dot for each of the ROM's four writes stops being an
  optional refinement.

- **Two hypotheses refuted, both recorded because they are cheap to re-form.**
  Reads do **not** split into a sample point and an effect point — the nesdev
  `NMI` page gives one instant for a `$2002` read and states the race as the
  two happening simultaneously, so there is no second knob; refuted by
  documentation rather than by a sweep. And the six NMI entries are **not** a
  one-dot alignment artifact — at the physically-unified `(dot2, dot2)`
  placement, where all three sprite entries pass, moving the VBL-set dot
  restores 0 of 6, 0 of 6 and 1 of 6, while breaking four more entries at both
  non-default values.

### Fixed

- **`Frozen OAM2 Increment` closes: AccuracyCoin reads 144/144 (100.00%, RAM
  decoder).** The last failing entry in the battery, open since the
  `Advanced Sprite Evaluation` page arrived, is closed by two dots of `$2001`
  deferral on two specific consumers — and every part of the diagnosis this
  repository had recorded for it was wrong.

  **The core was never "a dot early".** The tree's `KNOWN_FAILING` note said the
  entry failed "because a `$2001` enable on dot 256 takes effect a dot early".
  Measured per dot against the ROM's four stated writes — 242, 256, 325 and 340
  — the transition lands during dot N-1 in **all four**, on three independent
  instances of the 256 write, which means the new mask is in force from the
  **start of dot N**: exactly where the ROM says. The offset is constant, so it
  is compensable by one depth and carries no alignment dependence.

  **The failing sub-test was 4, not 2 or 3.** `TEST_FrozenOAM2Inc` has **no
  `INC <ErrorCode` between tests 3 and 4**, so both report `$0E` — the same
  missing-`INC` defect upstream had just fixed one entry over, in
  `Misaligned OAM2 Address`. Test 3 passes. The failure was test 4, the
  false-positive guard, which requires that **no** sprite-zero hit occur.

  **The OAM2 freeze machinery was never broken.** Instrumented per dot, it is
  byte-identical to the passing case throughout the fetch window
  (`addr=0, ovf=1, frozen=1`). The 809-raise end-to-end verification recorded
  earlier was accurate and was measuring the wrong thing.

  What is actually wrong is one rule at two dots — **a mask change during dot N
  must not act on dot N**:

  - **Dot 256, vertical increment.** The ROM states it directly: *"Rendering is
    enabled on dot 256, but the PPU's vertical scroll is NOT incremented."* That
    sentence only parses if the enable IS on 256. The increment now reads a new
    `rendering_enabled_delayed2` — rendering as of two dots ago — and is hoisted
    out of the shared `render_line && rendering_gate` block, because conjoining
    the two would put the DISABLE edge back on the shallower gate.
  - **Dot 339, OAM2 address reset.** The counter's gate used to conjoin the
    **live** mask, putting its disable edge a dot ahead of every other
    consumer's (`min(r, d)` for a conjunction of two delayed views). A disable
    whose effect lands during dot 339 therefore skipped the reset, left the
    "OAM2 Overflowed" flag raised, and produced precisely the sprite-zero hit
    test 4 forbids. The counter now rides the shared one-dot gate like
    everything else.

  The depth is **derived, not fitted**: the ROM over-determines it (the dot-256
  increment must not fire, the dot-257 latch must), giving `d = 257 - 255 = 2`,
  and both neighbours fail — one dot shallower is byte-identical to the unfixed
  build, one dot deeper breaks test 2.

  `Stale Sprite Shift Regs`, `Misaligned OAM2 Address` and the rest are
  unchanged; `KNOWN_FAILING` is now **empty** and `EXPECTED_PASS_COUNT` is 144.
  Save states gain a `PPU_SNAPSHOT_VERSION` **10** tail carrying the second gate
  stage — it looks derivable from stage 1 and is not, differing for exactly the
  one dot after a `$2001` rendering edge that is the whole subject.

- **The depth-2 rendering-gate pipeline froze instead of shifting, so two cells
  of the published derivation sweep measured the instrument.** Under
  `phi2-write-sweep` (a default-off study feature; the shipped build is
  unaffected, and asserted so at compile time), `tick` re-pointed
  `rendering_enabled_delayed` to `render_gate_prev2` at the top of a dot and
  then assigned `render_gate_prev2` back FROM that same field at the bottom --
  `prev2 = prev2`, freezing the stage at its power-on `false` for the whole run.
  Both `lag = 2` cells were published as **120/144** and actually read **141**
  and **140**: they measured a permanently disabled rendering gate, and that
  23-test collapse read as evidence that deeper pipelines are catastrophic when
  it was evidence of nothing. A second defect sat beside it -- the specialized
  dot paths bypass the pipeline, and their guards prove only a ONE-dot rendering
  history, so at depth >= 2 they would run a rendering-enabled body the general
  path gates off. The shift is now a named pair (`render_gate_begin_dot` /
  `render_gate_end_dot`) passing the value between them rather than leaving it
  in a field two hundred lines away, and `fast_dot_paths_valid()` excludes the
  fast paths at depth >= 2. The grid is completed from seven cells to all nine,
  because a claim of unreachability ACROSS a space has to have measured the
  space. **The conclusion is unchanged** -- the intersection is still empty and
  the shipped `(0, 1)` is still the unique 143/144 -- and it now rests on nine
  correct cells instead of seven of which two were wrong. Found in review
  against a sweep already run and written up; what kept it findable is that the
  sweep asserts its control FIRST, so `(0,1)` reading 143/144 in both the broken
  and the fixed run put the harness beyond suspicion and left the depth knob as
  the only candidate.

- **RETRACTION: v2.6.17's "two dots early" is wrong; the gap is HALF A DOT.**
  That release states the `$2001` writes land "two dots early ... 242 and 256,
  where this core applies 240 and 254", and that figure is the premise its whole
  investigation was scoped on. It is retracted, on three independent grounds.

  **Arithmetic.** `write_split(12) = (7, 5)` advances the PPU to
  `pre - PPU_OFFSET` = **6 master clocks** into the CPU cycle — 1.5 dots — where
  phi2 is 8, or 2.0. The gap is **2 master clocks, half a dot**. Those are the
  constants the shipped build compiles, not an interpretation.

  **The knob agrees.** `WRITE_PHI_OFFSET` is denominated in master clocks and the
  value expressing phi2 is **2** — the same half dot.

  **Sufficiency, which is decisive.** `Frozen OAM2 Increment` fails at the
  shipped placement and passes at `WRITE_PHI_OFFSET = 2`. If the commit were two
  dots (8 master clocks) early, a 2-master-clock change could not reach the
  correct dot. A test that flips on half a dot cannot have been two dots out.

  **Where 240 came from.** `ppu-state-trace`'s hook reads state *after* each
  dot's effects, so its records are end-of-dot: PPUMASK still holds `$18` at
  end-of-240 and `$00` at end-of-241, putting the write in dot **241** (and,
  under phi2, in 242 — exactly the dot the ROM names). A probe reading the PPU's
  dot counter at the *instant* of the CPU access reads one lower, because
  `Cpu::start_cycle` catches the PPU up before the bus access. Comparing that
  against the ROM's *effect* dot makes a one-dot gap look like two.

  **What survives:** v2.6.17's decision not to adopt phi2 was correct, and the
  mechanism is real — a 6502 commits at phi2 and this core commits half a dot
  earlier. **What changes:** a two-dot error is a structural scheduler defect; a
  half-dot error is a phase-and-rounding question, since which PPU dot a fixed
  master-clock offset falls in depends on the instruction's CPU/PPU alignment.
  That is why three writes measured across two tests cannot be satisfied by any
  one dot-quantised offset, and it reframes the remaining work.

  The retracted text is annotated **in place** in
  `to-dos/plans/v2.6.18-terminus-plan.md` rather than deleted, and the released
  v2.6.17 CHANGELOG entry is left untouched — a published version is immutable,
  and erasing the claim would erase the record that it was made. Note also that
  the plan had ALREADY recorded the half-dot insight in its
  `Arbitrary Sprite zero` section; this retraction confirms and generalises it
  rather than discovering it.

## [2.6.17] - 2026-09-11 - "Terminus" (a write lands where the cycle ENDS, and this core does not move to meet it)

### Changed

- **AccuracyCoin re-synced to upstream `69c8860` (2026-09-11), and the battery
  grew 141 -> 144 assigned tests.** The ROM, `LICENSE` and
  `SOURCE_CATALOG.tsv` are re-vendored from `100thCoin/AccuracyCoin` (MIT).
  The catalog goes 146 -> 149 rows across 20 -> 22 suites: upstream added
  three tests (`Frozen OAM2 Increment` `$0493`, `Misaligned OAM DMA` `$0494`,
  `Misaligned OAM2 Address` `$0495`) and two pages, `Advanced Background
  Evaluation` and `Advanced Sprite Evaluation`, which **re-home eleven
  existing PPU tests** out of `PPU Misc.` / `PPU Behavior` / `Sprite
  Evaluation`. A re-sync is therefore not an append — a suite-keyed baseline
  has to be regenerated rather than extended.
- **Measured 142 of 144 (98.61%) on the default build, with no regression.**
  Upstream removed no test, so 144 assigned minus the two failures is exactly
  the previous 141 plus the one new test that passes. The two gaps are
  `Advanced Sprite Evaluation :: Frozen OAM2 Increment` (error 2) and
  `:: Misaligned OAM2 Address` (error 3) — secondary-OAM address behaviour
  during sprite evaluation, which this PPU does not model.
- **The AccuracyCoin gate now pins the failing SET, not "zero failing".**
  `accuracycoin.rs` gains `KNOWN_FAILING`, an allowance that fails in BOTH
  directions: a new failure is caught because it is absent from the list, a
  *fix* is caught because it is present and no longer failing, and a swap is
  caught because the set differs while the count does not. A one-directional
  allowance hides exactly the coverage it was written to tolerate — the v2.6.9
  lesson. Both directions demonstrated by mutation.
  `accuracycoin_runahead.rs` likewise compares the failing set across depths
  rather than asserting a perfect baseline it no longer owns.
- **TriCNES re-synced to upstream `f388af0` (2026-09-10)**, from `f54d8be`
  (2026-05-05). It is the AccuracyCoin author's own emulator and the gold
  oracle for these tests, vendored in-repo under its MIT license. That window
  carries the OAM2-address and OAM-evaluation fixes matching AccuracyCoin's
  new page, the 6502 internal-data-bus fix, and Mapper 66 (GxROM). The
  instrumented cross-diff harness was carried across by a **3-way merge**
  against the exact vendored base commit (+912/-576 upstream lines, 2
  conflicts, both "inserted at the same point" and resolved by keeping both
  sides); all eight instrumentation markers verified present at identical
  counts, and the harness rebuilt clean under .NET 10.

### Added

- **`scripts/accuracycoin-build/extract_catalog.py`** — the catalog
  extraction, which until now existed only as a prose recipe in
  `tests/roms/AccuracyCoin/README.md`. Prose cannot be re-run or audited, so
  every re-sync re-derived it by hand. The script carries a `--self-test`
  (4 cases) and orders rows by upstream's `TableTable` (the ROM's own display
  order) rather than by position in the file.

### Fixed

- **`docs/STATUS.md` advertised a cargo feature that does not exist, and
  described shipped default behaviour as disabled.** Its feature table listed
  `cpu-implied-dummy-reads` as an available, default-**off** flag. Both the flag
  and the `cfg` on `Cpu::implied_dummy_read` had been deleted when the behaviour
  was promoted: every build performs the cycle-2 dummy read. That is the worse
  of the two directions a stale row can drift in — a row naming a knob that does
  not exist merely fails; a row calling the shipped default "off" invites
  someone to enable a fix that has been on for releases. The row is now marked
  removed, in the style the two v2.0.0-era retired rows already use.

  Three of the four `#[allow]`s on that helper were suppressing nothing —
  `needless_pass_by_ref_mut`, `unused_self` and `missing_const_for_fn` existed
  for the deleted OFF branch. Measured by stripping all four and re-linting:
  `inline_always` is the only finding, so it is the only one kept. Same shape as
  v2.3.9's sweep, which found 25 of 29 `allow`s suppressing nothing.

- **New gate: `feature_flag_audit.rs`**, pinning declared cargo features against
  the documented ones in both directions. It fails closed on the table heading,
  refuses a parse that yields implausibly few rows, and carries the reverse
  gap — 26 flags declared with no table row — as an explicit `UNTABLED` list
  with a reason per entry rather than as silence, because an unexplained
  omission cannot be told apart from an oversight. It **found the row above on
  its first run**, and three mutations are CAUGHT: re-marking the retired row as
  live, dropping an `UNTABLED` entry for a live flag, and renaming the table
  heading (which fails all three assertions, as fail-closed requires).

  It deliberately asserts nothing about whether a default is *right*: that is a
  measurement against the >3% adoption bar, not a property of a table. The audit
  behind it re-checked the off-by-default set and found no flag that should be
  flipped — `ppu-idle-line-fast` is the closest call and its own row records why
  (below the bar, and a slight regression on the rendering-heavy content that
  dominates real play).

- **AccuracyCoin `Misaligned OAM2 Address` now passes — 142 -> 143 of 144
  (99.31%).** `OAM2Address` is now a live counter maintained across sprite
  fetch instead of an index derived positionally from the dot
  (`((dot-257)/8)*4 + min(phase,3)`). A derived index cannot represent an
  address that fell behind, so an interval of rendering-disabled time during
  fetch was invisible to it; the counter now loses those increments exactly as
  hardware does. Implemented from AccuracyCoin's own source comments (MIT;
  stimulus, not a reference implementation), so the provenance ladder was
  never escalated past rung 1.

  The **"OAM2 Overflowed" flag turns out to be load-bearing for the counter,
  not only for the test named after it**: advancing on even dots yields 33
  candidate increments across dots 256-320, and it is the flag — raised when
  the 32nd wraps `$1F -> 0` — that suppresses the 33rd and leaves the address
  resting at 0. So the documented "`$2004` during dots 321-340 reads OAM2[0]"
  is not a special case but the ordinary end state of the counter. One
  mechanism explains both new tests.

- **SAVE-STATE EPOCH: `PPU_SNAPSHOT_VERSION` 8 -> 9.** The OAM2Address
  counter, its "OAM2 Overflowed" flag and the dot-257 freeze latch are
  serialized. The `.rns` container compares each section's version for
  EQUALITY, so **pre-v9 save states no longer load** — the same cost the v8
  bump carried, and a maintainer decision to weigh before release.

  They are serialized rather than allowlisted as derived, on the schema
  audit's own advice ("the default assumption is that these need SERIALIZING
  ... that has been the right answer three times out of three"). They look
  derived — all three re-derive at dots 63/255/339 within a scanline — and
  that reasoning is exactly inverted here: the behaviour being modelled IS
  what happens when rendering is disabled across those reset dots, so a
  snapshot taken inside such a window carries state recoverable from nothing
  else in the blob, and run-ahead snapshots every frame. The v8 tail exists
  because this same class of state cost the battery three tests under
  run-ahead before it was carried.

- **The misaligned-OAM out-of-range advance now realigns (`+4 & $FC`), pinned
  by a targeted test because NO test's verdict depends on it.** AccuracyCoin's
  README states two rules for advancing `OAMADDR` past an out-of-range sprite,
  and they differ by whether secondary OAM is full: full is "only increment by
  5" (implemented for releases, as the buggy `n+m` increment), not-full is
  "incremented by 4 and bitwise ANDed with `$FC`" — which CLEARS the byte
  index. This core advanced the sprite and carried the misaligned byte index
  forward. Both rules are invisible while `OAMADDR` is a multiple of four,
  since `m` is already 0 at every y-test, so only misaligned OAM can observe
  the difference at all.

  **Measured before adopting**: a full battery run reaches the not-full
  out-of-range case exactly **114 times** out of 56,953,944 out-of-range
  branches, and the battery is **143 of 144 before and after**, with the
  failing set identical. So no gate in this project could adjudicate the rule,
  which is why it was recorded rather than adopted when it was first checked.
  What settles it is a stimulus that reaches those 114 cases and asserts on
  them: `misaligned_oam_out_of_range_advance_follows_both_rules` drives one
  y-test from `OAMADDR = $05` (`n = 1`, `m = 1`) past an out-of-range Y and
  asserts the resulting `(n, m)` for BOTH rules — `(2, 0)` not full, `(2, 2)`
  full. It fails without the change (`left: (2, 1)` — `n` advanced, `m` did
  not), so the project's bar is met by a test rather than by a README.

- **`Frozen OAM2 Increment` is NOT closed, and the blocker turned out not to
  be a sprite gap at all.** An earlier reading of this — that it needed
  `spr_count` / `spr_zero_in_line` from an evaluation the test prevents — is
  **retracted**; both are correct (8 and true). The freeze is verified end to
  end by probe: raised **809** times across a battery run, reaching sprite
  fetch **exactly once** (the single construction that test builds), on
  scanline 196, with `secondary_oam[0]` = $C1 and all eight slots loading Y,
  tile, attr and X all equal to $C1. Every sprite-side precondition holds.

  Its detector is a sprite-zero hit, which also needs an opaque BACKGROUND
  pixel under the sprite — and on scanline 197 the background is opaque
  nowhere in x=190..205 (sprite pixels there: 51; background: 0). `v` is one
  vertical increment ahead: fine-Y reads 3 where the test needs 2, so the tile
  it placed for the hit sits a row off. The cause is a $2001 rendering-ENABLE
  landing on dot 256 taking effect one dot early, firing the dot-256 vertical
  increment hardware does not — which the ROM states outright: *"Rendering is
  enabled on dot 256, but the PPU's vertical scroll is NOT incremented."*

  Deliberately **not** patched at the dot-256 site. That would be a
  compensating edit of the shape v2.5.7 recorded, where a wrong phase had
  every window compensating for it. Stays pinned in `KNOWN_FAILING`.

### Measured and rejected

- **The $2001 write-effect alignment was investigated and the obvious fix is
  REFUTED.** Kept with its numbers, per this project's rule that a rejected
  change with its measurement is a result.

  The offset is real and systematic: this core applies the test's two $2001
  writes on dots **240** and **254** where the ROM names **242** and **256** —
  both exactly two dots early, on two independent writes. The mechanism is
  that `Cpu::start_cycle` catches the PPU up BEFORE the bus access, so a PPU
  register write lands at M2-low (the start of the CPU cycle) while a 6502
  commits a write at phi2, two dots later. That is v2.5.4's finding one layer
  out: that release found the co-simulation TESTBENCH presenting accesses on
  the second of a cycle's three dots; this is the EMULATOR applying them on
  the first.

  The fix that suggests itself — delay the rendering-enable gate so the
  dot-256 vertical increment is suppressed — does not work. A sweepable lag
  on `rendering_enabled_delayed` (shipped value 1) swept 1..4 against the
  battery, with the mask-timing-sensitive tests as controls:

  | lag | passed | Frozen OAM2 | Stale Sprite Shift Regs | Misaligned OAM2 |
  |---|---|---|---|---|
  | **1 (shipped)** | **143/144** | FAIL | pass | pass |
  | 2 | 141/144 | FAIL | FAIL | FAIL |
  | 3 | 141/144 | FAIL | FAIL | FAIL |
  | 4 | 140/144 | FAIL | FAIL | FAIL |

  No value closes `Frozen OAM2 Increment`, and every value above the shipped 1
  regresses two tests that pass today. The shipped lag is optimal; the knob
  was reverted rather than kept as dead code.

  So the pipeline GATE is not the subject — the WRITE PLACEMENT is, and moving
  it is a scheduler-level change to the `start_cycle` / `end_cycle` ordering
  (ADR 0029 territory) that shifts every PPU register write in every game. It
  needs its own version, its own ADR and its own re-baselining.

- **The write placement was then MOVED, measured against a control, and NOT
  ADOPTED — a maintainer decision, and the outcome the plan authorised in
  advance.** The diagnosis stands: a 6502 commits a write at phi2 and this
  core applies PPU register writes at M2-low, two dots early. The *change*
  does not land.

  | combination | AccuracyCoin | independent oracle |
  |---|---|---|
  | **shipped** (M2-low + the 2-stage compensation) | **143/144** | clean |
  | phi2, write alone | 141/144 | `ppu_vbl_nmi/10-even_odd_timing` FAILS `09` |
  | phi2 + dot-321 + `skip(1)` | 142/144 | clean |

  The first row is what ships. The distinction that decided it: the six
  framebuffer goldens the move shifts are BASELINES and would legitimately be
  re-blessed if phi2 were right, but `10-even_odd_timing` is a third-party ROM
  with its own verdict, and it went pass -> `09`. Re-deriving that ROM under
  phi2 from its own statement of what it measures CLOSED that regression —
  `mask_for_skip_check` needs **one** delay stage under phi2 rather than two,
  producing the identical `08 08 09 07` — which is also the proof that the
  pipeline is a compensation for the placement, as its own comment already
  said. Even so the best combination is **net −1** against what ships, and it
  costs a save-state epoch plus six re-baselines.

  **Two of the three dependent behaviours are still un-re-derived** (`Arbitrary
  Sprite zero` test 3 and `Stale Sprite Shift Regs` test 5), and adopting a
  mechanism while its dependants still compensate for the old one replaces a
  *documented* compensation with an undocumented one. So the compensation
  stays, `Frozen OAM2 Increment` stays in `KNOWN_FAILING`, and the apparatus
  is kept: `phi2-write-sweep` (default-off, `const fn` when absent, so the
  shipped build is byte-identical by construction) plus `phi2sweep.rs`, so the
  next attempt re-measures in an afternoon. The divergence is rowed in
  `docs/accuracy-ledger.md` and stated at the site in `cpu.rs`; the three
  conditions for reopening it are in the plan's *CLOSED* section.

- **`terminus_control.rs` — a first-difference control, kept as a standing
  gate.** Built to answer "did the experiment change, or did the subject?", it
  chains a rolling FNV-1a over the pre-palette framebuffer and work RAM per
  frame across three workloads at 400 frames on the DEFAULT feature set. It
  outlives the experiment that motivated it, because every future timing
  change needs the same check: the first divergence must be the cycle you
  aimed at, or something else moved.

- **One row of `SOURCE_CATALOG.tsv` had been wrong since the v2.0.1 hand
  extraction.** Running the new extractor against the asm at `71f57fb`
  reproduces the committed 146-row TSV byte-for-byte *except* "Attributes As
  Tiles", which the hand pass filed under `PPU Misc.` where upstream has it in
  `Suite_PPUBehavior`. The result address was identical, so no verdict was
  ever wrong — only the per-suite breakdown. Found by validating the tool
  against the artifact it replaces, which is the only reason it surfaced.
- **`tricnes-harness` defaulted to a ROM path that no longer exists**
  (`Commercial_Private-Projects/RustyNES_v2/...`, the pre-reorg workspace
  layout). It now resolves the ROM relative to the working directory and exits
  with a named diagnostic rather than a bare file-not-found.
- **Building the vendored TriCNES harness leaked ~1.6 MB of `bin/` + `obj/`
  into the repo.** `.gitignore`'s `!/crates/rustynes-test-harness/golden/**`
  re-inclusion un-ignored them, beside a tree whose own README promises "no
  build artifacts". Scoped ignore rules added for the two output directories.

## [2.6.16] - 2026-09-04 - "Interlock" (the arbiter's numbers describe a stimulus, not the console)

### Changed

- **The off-die memory system is measured on the console for the first time, and
  the block-level figures quoted for three releases describe a machine that does
  not exist.** `tb/sdram_arb_main.cpp` drives the arbiter with a stimulus in
  which a CHR and a PRG request arrive on the SAME cycle, and reports the fetch
  with seven cycles of margin and the CPU short of its deadline by four. Measured
  at the arbiter's own ports on off-die builds of `ppurender`, `ppusprender` and
  `mapper4mmc3064`, the console never produces that coincidence: **zero
  coincidences and a minimum separation of two cycles over fifteen million SDRAM
  cycles**, with the separations bimodal because both requesters are locked to
  one master clock at a fixed alignment. Both published numbers are wrong about
  the console, in opposite directions -- the CPU **meets** its deadline on every
  one of 240,303 requests, at **zero** margin, and the fetch reaches **23 against
  the arbiter's derived 22**. The worst values agree across all three workloads;
  the exceedance RATE — 30 of 385,539 fetches, about eight in a hundred thousand
  — is `ppurender`'s alone, because the counting was added after the other two
  had run, and quoting it as an aggregate would claim a measurement not taken. `docs/sdram.md` had asserted this distinction since v2.6.13 and
  nothing had ever put a number on either side of it.
- **The fetch's exceedance is not a defect, and the sweep is what says so.**
  `nes_top`'s `CHR_LAT` adds whole master clocks -- four SDRAM cycles each -- to
  the CHR byte, so raising it off-die asks the console directly how much spare
  the fetch has at its real worst case. `ppurender` matches the oracle on all
  61,440 pixels at +0, +1 and +2 and fails at +3 (233 pixels) and +4 (32,187).
  The failure is the control: four consecutive passes would look identical to a
  knob that never reached the compiler. So the fetch has **at least eight and
  fewer than twelve spare cycles** at a worst case of 23, and the derived budget
  of 22 understates the real tolerance by at least nine. The derivation is not
  wrong to be conservative; it is wrong to be quoted as though it were measured.
- **The shortfall was decomposed before it was acted on.** Running the same PRG
  cadence with CHR silent gives a worst PRG latency of **15 against the same
  budget of 24**, so the access itself has nine cycles of margin and every cycle
  of the synthetic shortfall is contention. That is what would make a
  slot-scheduled arbiter the right lever -- and it is why one has not been built,
  since the console's own phase already separates the two requesters.
- **A prediction this release got wrong, kept with the measurement that refuted
  it.** The arbiter gate drove both streams from one 64-byte window, where every
  access is a row hit and no alternation can force a PRECHARGE -- a geometry the
  shipped map cannot produce, since PRG and CHR sit four mebibytes apart. Moving
  the stimulus onto the real bases was expected to make the worst case worse and
  left both figures **byte-identical**, because the worst case is not an
  alternation at all: refresh precharges every bank, so the access after one is
  a miss whatever the addresses are. The geometry moves the average, not the
  bound.

- **The co-simulation suite is 148 gates, and the two configurations no longer
  report the same number.** On the die: **147 passed, 0 failed**. Off the die:
  **148 passed, 0 failed**. The difference is exactly one gate — the new
  SDRAM-latency gate, which reports **N/A** rather than SKIP on the die, because
  a gate on the SDRAM path cannot exist in a build with no SDRAM and "skipped"
  reads as something that could have run. It also makes the off-die
  configuration self-verifying: it refuses a build with no SDRAM path rather
  than reporting a maximum over zero requests, so its PASS is evidence about
  what *ran* where the stamp file and symbol count it replaces described what
  was *built* — the distinction v2.6.13 lost a whole result to.
- **The bitstream is byte-identical to v2.6.15's, and this time it was rebuilt
  rather than renamed.** A full Quartus compile produces `md5 7346a490…`, the
  artifact v2.6.15 shipped, with timing identical at all four corners (worst
  setup +0.473 ns, worst hold +0.078 ns). The premise was measured rather than
  assumed: across `rtl/`, `sys/`, the `.qsf`, the `.sdc` and `files.qip` the
  diff is **85 insertions and 10 deletions with zero non-comment lines**. That
  also settles a **fitter seed sweep, considered and declined** — `SEED`
  re-rolls placement for a given netlist, and an unchanged netlist re-measures
  the same distribution v2.6.10 already measured. It is flagged for the next
  release that changes the netlist, since the slot-scheduled arbiter would add
  logic to a design whose hold margin is +0.078 ns — with v2.6.7's correction
  attached, that of the 0.769 ns it recovered, 0.595 came from a memory change
  and only 0.174 from the seed.
- **The new latency gate parsed a failed run's numbers and called it a PASS**,
  found in review rather than by the mutation pass written for it. The gate ran
  the console and parsed its report without ever checking the subprocess return
  code, and `cpu_main.cpp` prints that report near the end and then still has
  two ways to exit non-zero after it — a broken `eval_ovf_cnt` invariant, and
  `return halted ? 1 : 0`. So a console that **jammed on an unknown opcode**
  would print a well-formed report of the latencies it saw before jamming, and
  the gate would find them inside budget and pass. Worse than a missing check
  usually is, because the numbers would be **real**. Fixed and demonstrated by
  mutation: without the check, a stub printing an in-budget report and exiting 1
  yields `PASS: the off-die memory system meets both deadlines on the console`.
  The eight mutations written for these gates covered the thresholds, the
  vacuity guard and the parse guard, and **not one asked what happens when the
  numbers are fine and the run is not**.
- **The expired figure had a fourth home, one repository over.**
  v2.6.16's first workstream corrected "140 of 142" in `rtl/emu.sv`,
  `docs/sdram.md` and `docs/rung7-mappers.md`; the same number, with the same
  "two failures are one number" gloss, was still in this repository's
  `to-dos/mister/IMPLEMENTATION_PLAN.md`. Fixing the sites a report names and
  missing the others is a shape this project has recorded before, and it does
  not stop at a repository boundary.

### Added

- **`tb/sdram_latency_gate.py`** -- the memory system's deadlines asserted on a
  real off-die console run rather than on a traffic model, wired into
  `tb/regress.sh` and reported as **N/A** rather than SKIP on an on-die build,
  because a gate on the SDRAM path cannot exist in a build with no SDRAM. It
  **refuses a vacuous pass**: zero requests on either port fails rather than
  reporting a maximum over an empty set. Demonstrated by four mutations, all
  CAUGHT, including gating CHR at the derived 22 -- which fails, and is the
  finding above stated as a test.
- **`tb/check_cart_map.py`** and **`tb/cart_map.h`** -- the cartridge's SDRAM
  bases had **four copies across three languages** (`cart_sdram.sv`'s parameter
  defaults, `emu.sv`, `tb/cpu6502_cosim.sv`, and the new header), and a
  divergence would point the `$2007` port at a different region of the part than
  fetches use -- silently, and only on a banked board. That is v2.6.13's defect
  shape exactly. Now gated in CI, with a self-test whose most important case is
  a pattern that matches **nothing**: an absent match and an agreeing file are
  otherwise indistinguishable, which is how a gate comes to assert nothing.
- **`SDRAM_SEP_PROBE=1`** on the console co-simulation -- per-port request
  separation, latency distributions and over-budget counts, gated on the console
  actually running so the power-on request pair (both ports issue together
  leaving reset, while the CPU is held) cannot report a minimum separation of
  zero for every ROM.

## [2.6.15] - 2026-09-04 - "Warrant" (the claims v2.7.0 will make become checkable)

### Fixed

- **The `.rbf` name this core shipped would have distributed NOTHING.** Carried
  since v2.6.7 as a style divergence awaiting a maintainer decision, and
  recorded in `docs/bitstream-release.md` as a trade-off that "costs nothing
  today". It is not a style item and the cost is total.
  `Distribution_MiSTer/.github/download_distribution.py` strips a datecode by
  taking the stem's **last nine characters** and requiring `_` followed by
  exactly eight digits, and `uniq_files_with_stripped_date()` then `continue`s
  -- skips outright -- any file that yields none. `RustyNES_MiSTer-v2.6.13.rbf`
  yields none, so it would never be copied into the distribution and never reach
  a user through `update_all`. **The failure mode is an accepted core that
  appears in the wiki's Cores table and ships nothing, with no error raised
  anywhere.**

  A second and independent parser applies in the firmware:
  `Main_MiSTer/file_io.cpp`'s `get_display_name()` takes the **first** literal
  `_20`, requires six characters after it, truncates the display name there and
  treats the rest as a datecode; `DirentComp()` then sorts on the truncated
  name. Two parsers, different rules, and `_YYYYMMDD` is the only form
  satisfying both -- which is why the fix is one name rather than a preference
  between two.

  So there are now **two names, because there are two audiences and only one of
  them is a parser**. `releases/` carries `RustyNES_YYYYMMDD.rbf`, the path the
  distribution reads out of the repository; the GitHub releases carry that plus
  the version-named copy, which no upstream tool parses, because a bitstream
  someone has in hand should trace to the exact release and gate results that
  produced it. The 2026-08-30 decision is **extended rather than reversed**. The
  datecode comes from the **tag's commit** rather than `date`, so rebuilding a
  tag reproduces its filename. `tb/check_rbf_name.py` carries both parsers'
  rules as one test with ten cases, nine of them mutations -- including the name
  this repository actually shipped, and `RustyNES_v2.6.14_20260904.rbf`, which
  parses and is still wrong because it multiplies into one core entry per
  release. v2.6.14's committed bitstream was **renamed, not rebuilt**.

- **Two of the four R1/R2 residuals were never IRQ-timing residuals.** ADR 0002
  has closed this over four sub-tests since v2.0.0 beta.3 -- `mmc3_test_2/4` #3,
  `mmc3_test_v1/4` #3, `mmc3_test_v1/5` #2 and `mmc3_test_v1/6` #2 -- through
  21+ documented rollbacks, a two-session bounded-effort campaign and two
  instrumentation studies. Two of those four do not depend on **when** the IRQ
  asserts, so no lever on any axis that campaign searched could ever have moved
  them.

  The corpus says so twice. `mmc3_test` and `mmc3_test_2` are the same suite
  twice over and the second is the revision: it ships an "MMC3 Operation" readme
  section the older corpus lacks entirely, and **sub-test 2 of `5-MMC3` carries
  the identical `set_test` string in both while differing by one instruction** --
  the successor inserts a SECOND `clock_counter` before the first
  `should_be_set`, so its verdict no longer lands on the `$C001`-pending reload.
  The assertion was **withdrawn by its author**. `mmc3_test_v1/6-MMC6` rests on
  the same withdrawn clock and is additionally the v1 corpus's
  **alternate-revision** ROM: its header names Crystalis, which
  `mmc3_test_2`'s readme identifies as revision A, and `mmc3_test_2/6-MMC3_alt`
  carries that header verbatim.

  Measured rather than argued. Adopting the withdrawn rule makes
  `mmc3_test_v1/5` **pass** and moves `/6` from #2 to #3, and costs **both**
  `scanline_timing` ROMs a regression from sub-test 3 to sub-test 2 -- sub-test
  2 is "should occur **later**", so the IRQ starts arriving too early on the one
  ROM pair that measures when it arrives. Reverted from a pre-edit snapshot; the
  numbers are in ADR 0002's decision update, because a rejected change with its
  measurement is a result. The R1/R2 residual is therefore
  **`mmc3_test_2/4` #3 and `mmc3_test_v1/4` #3** -- one behaviour measured twice
  -- and the closure over those two stands untouched.

- **`bump_release.py` inserted the outgoing release at the wrong END of the
  chain.** It dropped the previous release from `ROADMAP.md`'s lineage at
  v2.6.13 and again at v2.6.14; both times `release_anchor_audit` caught it and
  both times it was repaired by hand. Reproduced in a throwaway worktree, and
  the previous description was wrong in a way that matters: **the release is not
  dropped, it is relocated.** The handler inserted after the first `", on "`,
  correct for the shape it was written for and wrong for root `ROADMAP.md`,
  whose chain is thirty entries long and ends `, on the v2.0.0 "Timebase" MAJOR
  cut.` -- so the token swap took the release off the head and the insertion
  filed it beside the MAJOR cut. A lineage that skips a release **and names it
  somewhere nonsensical** is worse than one that merely skips it, because a
  reader checking for its absence finds it.

  `"Built on "` is now tried first, because it names the head; `", on "` is the
  fallback; neither present is reported. A chain ending `..., the current
  release` cannot be extended by a token swap at all -- the new link needs a
  written summary -- so the script now names every such chain and **exits 1**,
  refusing rather than forgetting. The CHAIN rewrite is **extracted** into
  `extend_chain()`, which is the load-bearing part: the selftest named "chain
  names the predecessor once" re-implemented the substitution inline, so the one
  path that had gone wrong twice was the one nothing executed.

### Added

- **`cpu_interrupts_v2` on the MiSTer DUT -- the first INDEPENDENT interrupt
  oracle.** `docs/rung5-accuracycoin.md` states the hole in its own words:
  *"Rung 4 had blargg as an independent check and it found six defects no
  self-written gate could see; rung 5 has no equivalent, and that is the single
  most important sentence in this document."* Every interrupt gate in the suite
  compares the DUT against RustyNES, so a shared error between them is invisible
  by construction. Five single-purpose ROMs, **mapper 0** (the combined ROM is
  mapper 1; the singles are not, which removes MMC1 as a variable), reporting
  through `$6000`. **All five pass**, `$00`, with the `$DE $B0 $61` signature
  and the running state both valid.

  `5-branch_delays_irq` is the sharpest: *"A taken non-page-crossing branch
  ignores IRQ during its last clock, so that next instruction executes before
  the IRQ."* That is the exact behaviour v2.6.7 changed in the **oracle**
  (caveat C6, `skip_irq_sample_q`) from documentation reasoning alone, with no
  ROM adjudicating it. This one adjudicates it, on both consoles.

  Alignment is **recorded rather than assumed**, because blargg's readme says
  `2-nmi_and_brk` *"Occasionally fails on NES due to PPU-CPU synchronization"* --
  so its verdict is alignment-sensitive by design and a pass is a pass at the
  shipped alignment, not an absolute one.

- **An accuracy gate someone else can run** (`tb/fetch-goldens.sh`, the
  `rung1-pinned` CI job). `tb/regress.sh` says in its own header that it is not
  a CI gate and cannot be, which means this project's evidence -- 142 gates at
  the version's open, **147 at its close**,
  each with a mutation record -- was a set of **documents describing checks a
  reader cannot run**. For a submission whose stated bar is *"a minimum
  reasonable bar for readability and include some evidence of quality and
  accuracy testing"*, that is the weakest available form of the strongest thing
  here. `docs/golden-fetching.md` specified the fix and carried
  `Status: NOT BUILT`; the nine opcode-group ROMs now export from a pinned
  oracle commit and compare in CI.

  It is a **subset and the job's name says so** -- no PPU, no APU, no
  AccuracyCoin, no nestest, all of which need windows in the millions. Measured
  before built, as the specification asks: the exporter builds in 7 s, the nine
  windows total 4,624 cycles. **The pin is the point**: green means green
  against one recorded oracle commit, never against the oracle's current
  behaviour, because the determinism contract says nothing about trace-format
  stability.

- **`sys/` verbatim becomes a check** (`tb/check_sys.py`,
  `tb/sys_manifest.sha256`). It is a submission requirement -- the Template's
  Readme says *"Basically it's prohibited to change any files in this folder"* --
  and it rested on **one measurement taken at v2.6.6** plus a manual procedure
  nothing ran, eight releases ago. All 57 files pinned, catching a **changed**,
  a **missing** and a **stray** file; the third mode is why the directory is
  enumerated rather than only the manifest walked, and it is not hypothetical
  (`sys/README.md` and `sys/.gitkeep` both lived there through v2.6.5). An empty
  manifest is refused rather than passing vacuously. Demonstrated on the real
  tree, each mode restored from a snapshot. Separately re-verified against
  upstream: `Template_MiSTer`'s HEAD **is** the pinned `3ea1134c`, and `diff -rq`
  reports 0 differences -- so `sys/` is both unmodified and current, measured
  separately.

- **`tb/check_qsf_seed.py`** -- the `.qsf` may publish exactly one seed sweep and
  it must name the assigned seed. It carried **two**, disagreeing about the
  pinned seed's margin by 0.155 ns, one of them quoting a number the current RTL
  cannot reproduce. The checker's first version reported four tables in a file
  with one, matching prose that mentions a seed and a slack figure; the test is
  now structural and those three lines are fixtures.

- **The build is reproducible, and the reproducibility is DAY-SCOPED.** v2.6.15
  changes no RTL, and its bitstream came out **56,680 bytes larger** with the
  timing at every corner moved. `sys/build_id.tcl` is a `PRE_FLOW_SCRIPT_FILE`
  that rewrites `build_id.v` with the **calendar date** on every compile, and
  `rtl/emu.sv` puts `BUILD_DATE` into `CONF_STR` -- so the date is a constant
  *in the design*, and two builds on different days are different designs.
  Measured rather than inferred: rebuilding with `build_id.v` pinned to `260903`
  reproduces the published artifact **exactly** (md5 `2c2fa6eb...`, 4,040,572
  bytes), while `260904` gives `7346a490...`, 4,097,252. Six characters, 56,680
  bytes -- the `.rbf` is compressed, so its size tracks placement.

  v2.6.7's result stands and its **scope was never recorded**. It also weakens
  v2.6.14's claim that its bitstream was "byte-identical to v2.6.13's ... an
  identical artifact demonstrates it": that was achieved by **renaming**, not
  rebuilding, and a rebuild on a different day would not have been identical
  through no fault of the RTL. `scripts/release-rbf.sh` now refuses to publish
  unless `BUILD_DATE` equals the datecode in the filename, which makes the name
  a reproduction key.

- **The release build rewrote the project file, and nothing would have caught
  it.** The Template's Readme warns in the second person -- "You also need to
  watch this file before you make a commit. Quartus in some conditions may
  'spit' all settings from different files into this file" -- and one compile
  appended **218 lines to a 281-line `.qsf`**: 145 `set_location_assignment`, 62
  `set_instance_assignment`, 3 `set_hps_location_assignment` and 8 globals,
  every one already supplied by the vendored `sys/` Tcl. `git status` showed the
  file modified, which is what it always shows after a release edit, so the
  pollution was indistinguishable from the intended change until the diff was
  read. `tb/check_qsf_seed.py` now refuses framework-owned assignments (three
  more mutations, thirteen cases), and `release-rbf.sh` runs it immediately
  after the compile, so the build that causes it reports it.

- **`docs/bringup.md`** -- rung 6 written before a board exists, so that when one
  arrives it costs a session. What a board buys is exactly four things, all
  downstream of every gate by construction: the palette, the video timing
  constants, the audio's absolute level, and band-limiting. A first-boot
  checklist ordered so each failure is diagnostic, and an honest limit on what an
  on-device AccuracyCoin run can establish.

### Changed

- **Eight expired claims corrected, and one that turned out not to be.**
  `docs/rung7-mappers.md` opened "the SDRAM controller is not written"; it has
  been since v2.6.13. `docs/sdram.md` said "no cartridge is served from SDRAM
  yet". `RustyNES.qsf` said all 109 pin assignments come from one script; it is
  145 from two. **The CHR budget was stated three times and two were wrong** --
  `cart_sdram.sv` carried the pre-measurement figure the arbiter's own header
  describes as one that "was never measured", and `emu.sv` gave the CPU's budget
  as 28 rather than the 24 that remain after the crossing, disagreeing with
  itself two lines later. One home now, and one disagreement (a fetch latency of
  15 against 17) is **recorded rather than harmonised**, because picking one to
  make the files agree would be inventing a measurement. Oracle side:
  `docs/STATUS.md`'s lineage carried a present-tense claim inside a historical
  entry, and `IMPLEMENTATION_PLAN.md`'s "Where the core actually is" table said
  the APU, the cartridge, the SDRAM controller and `sys/` were all "Not started"
  and the `.rbf` "Never produced".

  **The one that was not stale**: the sibling's oracle-vs-documentation ledger
  was flagged as "nine releases of entries with no changelog row", inferred from
  the entry numbers. `git log` shows its last commit **is** rung 5's own closure
  and entry 3.43 was present at v2.6.7. The changelog is correct; what it needed
  was for its silence to be legible. Re-measure before correcting, including when
  the thing being corrected is an absence.

- **`T-ORACLE-001`'s opening claim is RETRACTED, in place.** It says RustyNES
  never clocks the MMC3 counter on the pre-render line. It does:
  `mmc3_test_2/2-details` sub-test 8 is, verbatim, *"Counter should be clocked
  241 times in PPU frame"* -- 240 visible plus pre-render -- and RustyNES
  **passes it**, as it has every release. The claim came from
  `--ppu-state-trace`, which that ticket's own instrument-traps section says
  carries no CHR address column and therefore cannot see an A12 rise at all. A
  trace that cannot see the event was read as evidence the event did not happen.
  Corrected in place, and in the sibling's `regress.sh`, which restated it.

- **`T-MISTER-SAVE`'s blocker is measured rather than asserted.** v2.6.12 refuted
  its own attempt on the grounds that "every save route terminates in `hps_io`,
  which no gate here instantiates" -- a claim about whether it CAN be, untested.
  It elaborates under Verilator with a `CONF_STR` value supplied, producing
  eleven errors, nearly all `PROCASSWIRE` from one line. So the choice is a cost,
  not an impossibility, and one enabler serves three tickets.

- **The `.srf` decision is revisited before a reviewer asked**, and it corrects
  its own recorded reason. That reason said the PLL warning "carries no message
  ID at all, so there is nothing for an assignment to name" -- true of
  `MESSAGE_DISABLE`, and not of the mechanism `.srf` uses: `Template.srf`
  suppresses it with a rule keyed on ID `9999` and the literal text `RST`. It
  stays absent, for a cost rather than an impossibility.

- The sibling gains a `.coderabbit.yaml`. It reviewed under org defaults, which
  meant the reviewer knew nothing about the provenance firewall, the RTL subset
  policy, or the rule that a gate is not trusted until a mutation against it is
  CAUGHT.

### Verified, not asserted

The emulation core is **unchanged** -- the `mmc3.rs` edits are `#[ignore]`
reasons and the sibling's `.sv` edits are comments -- so **AccuracyCoin 141/141
(RAM decoder)** and **nestest 0-diff** hold by construction. The mmc3 verdicts
are byte-identical to the baseline captured before any edit, and the default
gate is 18 passed / 5 ignored.

## [2.6.14] - 2026-09-03 - "Docket" (the submission checklist becomes auditable)

### Added

- **Every box in the MiSTer contribution checklist now carries a verdict, and a
  gate keeps it that way** (`contribution_checklist_audit.rs`). The list that
  decides whether the core is ready to submit at v2.7.0 had 30 boxes, 16
  unticked, and **14 of those 16 said nothing about why**. An unticked box with
  no reason cannot be told apart from three different things: work outstanding,
  work blocked outside this repository, and **work already done and never
  ticked**. The audit found the third case **five times**, so the list had been
  reporting the project as further from submission than it was, by a fifth of
  its own length.

  The gate asserts a shape rather than a judgement: a ticked box names the
  release that settled it, an unticked one carries `BLOCKED`, `DEFERRED`,
  `DECIDED` or `CONTINGENT`. It deliberately does not rule on whether a verdict
  is *correct* -- it cannot, and pretending otherwise would be the "gate that
  passes without testing its subject" this project keeps finding. Five
  mutations: a removed verdict, a removed release tag, every box ticked, and a
  broken box syntax are all CAUGHT; removing the continuation-line folding
  produces **nine false violations**, which is what proves that rule
  load-bearing -- the verdict markers do not sit on an item's first line.

### Fixed

- **The bitstream is byte-identical to v2.6.13's**, `2c2fa6eb0f751b2c9b60fcf903f2bf3f`,
  which is the point rather than a coincidence: the only sibling change this
  release makes is a **comment** in `RustyNES.sdc`, and an identical `.rbf`
  demonstrates it. That is a stronger reproducibility statement than v2.6.7's,
  which compared a clean and an incremental compile of the **same** sources;
  here the sources differ and the output does not. Timing is unchanged at the
  binding corner — worst setup **+0.317 ns**, worst hold **+0.068 ns**, both
  Slow 1100mV -40C — with 0 errors and the warning set matching the manifest
  exactly.

  Two of those checks were briefly misread as findings, and the mistake is
  worth the line it costs: `Quartus Prime Shell was successful` is emitted **per
  sub-flow**, so it is not a completion test. Read as one, it made a
  still-running compile look finished, which put a v2.6.13 artifact under a
  v2.6.14 label — and `check_timing`'s staleness guard is what refused it, by
  noticing the report predated thirteen sources. `check_warnings` reported a
  pinned warning "no longer emitted" for the same reason: the Fitter had not yet
  written it. Both gates were right; the reading was not. The completion test is
  the footer or the wrapper's exit status.

- **The task board had the same defect, and one row worse.**
  `to-dos/mister/TASKS.md` tracks the programme against itself, and **four
  delivered items had never been ticked** — v2.6.6's whole rung-6 integration,
  the SDRAM controller, the published `.rbf` and the Home folder — so the board
  read as further behind than the releases it tracks. The hardware row still
  named **v2.6.7**, seven releases after that slot shipped something else;
  naming a slot for work blocked on absent hardware is a prediction rather than
  a plan, and the number is struck.

  The SDRAM row is the interesting one. It said the controller comes **"after a
  board exists"**, and the table above it justifies that: the controller "needs
  hardware" because "its acceptance is read/write timing against a real part".
  v2.6.13 did not wait, and accepted it against a **behavioural model written
  from the same datasheet** — tRCD, tRP, tRAS, tRC, tRFC, tRRD, tMRD and tWR
  checked, a clock period the part cannot meet refused — then ran the whole
  console against it at 142 of 142. **That is rung 7's own recorded lesson
  repeating one row below where it is written**: *before recording something as
  blocked, check the blocker applies to the whole item.* It applied to hardware
  **acceptance**, not to building the controller or verifying it against a
  documented part. Twelve open items become eight.

- **The next version's headline is specified rather than named.**
  `cpu_interrupts_v2` sat on the board as "the independent interrupt oracle, now
  reachable", which is not enough to start from. Measured: it is **five
  independent single-purpose ROMs**, they are **mapper 0** where the combined
  ROM is mapper 1 — removing MMC1 as a variable — **the oracle passes all
  five**, so a DUT gate can adjudicate, and they report through `$6000` so they
  need `PRG_RAM=1` exactly as the blargg batteries do. Deliberately not started
  here: it is a five-gate campaign with its own findings, and bolting it onto an
  audit release would blur both.

- **Two ticked boxes were ticked on evidence that had expired**, found by
  re-measuring the settled half of the list rather than only the unsettled one —
  which is v2.6.9's finding applied to a different document: a stated reason
  stops being true and nobody re-reads it.

  `RustyNES.sdc` said *"There is exactly one core clock ... so there is no second
  clock domain to constrain and no false path to cut."* True of v2.6.3's
  master-clock divider, **false since v2.6.13**, which added `clk_sdram` at 4x
  and the phase-shifted `clk_sdram_ps`; the shipped timing report names all
  three core-PLL outputs. The file's *behaviour* was never wrong —
  `derive_pll_clocks` picks up all three — only its explanation. **And the
  alarming reading of that was checked before it was written down**: it looks as
  though `sys/sys_top.sdc`'s `set_clock_groups -exclusive` might be cutting the
  `clk_sys` <-> `clk_sdram` crossing, leaving ADR 0039's safety argument
  unfalsifiable. It is not: `-exclusive` cuts paths *between* groups, all three
  outputs match one glob and land in one group, and v2.6.13's own -24.769 ns
  measurement of that crossing is only observable because those paths are
  analysed.

  `RustyNES.qsf`'s entry said the device and **all 109** pin assignments come
  from `sys/sys.tcl`. `sys.tcl` supplies 109 — including the 45 SDRAM pins,
  which is why the SDRAM work needed no pin file of its own — and stops short of
  the I/O board, so a core must also pick one of two 36-assignment variants.
  This one sources `sys_analog.tcl`. **145 in total, from two scripts.**

- **The gate had the defect it exists to catch, found in review.** A line such
  as `- [ ]missing-space` matches neither exact prefix, so the parser fell
  through to the separator branch and **dropped the box silently** — and 29
  surviving items still cleared the `>= 20` floor, so the audit would have
  passed while one box went unchecked. A malformed box is now rejected by name,
  and the floor is the current count (30) rather than a round number well below
  it. Both demonstrated by mutation: a box with the space removed, and a box
  deleted outright, are each CAUGHT.

  **The second reviewer then found the same hole one indent over.** An
  *indented* checkbox slips past a column-zero test and is folded into the
  previous item as continuation text — and because that ADDS a box rather than
  removing one, the item count stays at 30 and the floor cannot notice either.
  The check now runs on the trimmed line and rejects checkbox-shaped lines that
  are not at column zero. Two more properties came with it: `char::is_whitespace`
  instead of `' '`, so a tab-indented continuation is not read as unindented and
  does not cut its item short; and a verdict marker must be a whole word, since
  a raw substring match accepted `**BLOCKEDNESS`. Three mutations — an indented
  box and a `BLOCKEDNESS` verdict are CAUGHT, and a tab-indented continuation is
  ACCEPTED as the positive control.

  **And then the mutation record itself was found to be a transcript.** Every
  mutation above had been run at the shell -- edit the checklist, run the gate,
  restore -- so the results were real and **nothing in the repository carried
  them**: no future change could trip over them, and "demonstrated by mutation"
  rested on something nobody else could re-run. Caught in review, in a CodeRabbit
  finding posted **outside the diff range**, which is the surface a
  resolve-every-thread sweep does not reach. The cases are now **tests** -- a
  clean baseline plus eight mutations of it -- feeding synthetic documents
  through the same `parse` and `violations` the real gate uses rather than a
  reimplementation, and `violations` was extracted for exactly that reason, since
  a test that reimplements its subject agrees with itself forever. Proven against
  the production code in turn: dropping the malformed-checkbox rejection fails
  **two** of them, reverting the whitespace test fails the tab-continuation
  control, and restoring the substring verdict match fails the whole-word case.

  **And the review then found a blocking defect in the check itself.**
  `trim_start().starts_with("- [")` treats an ordinary Markdown link bullet --
  `- [NESDev documentation](https://…)` -- as a malformed checkbox, so the gate
  would have failed the first time anyone put a link in the list. There is no
  such line today, which is exactly why it was invisible: the check passed while
  being fragile to the next edit. It also missed `* [ ]` and `+ [ ]`, which
  Markdown accepts as task items just as it accepts `-`, so an indented box under
  a different bullet character would still have vanished. `is_checkbox_shaped`
  now requires a bullet, a space, and a closing bracket within two characters --
  three more tests, and two mutations: recognising only `-`, and treating any
  `[` as a checkbox, each fail two.

  **The next round found the same class a fourth time.** Matching bytes and
  asking for ` `, `x` or `X` exactly meant `- [OK]` and `- [<emoji>]` matched
  neither arm -- not an item, not an error, and silently closing the item above.
  The rule is now "a closing bracket within three CHARACTERS", counted in chars
  rather than bytes, so anything bracket-shaped is reported rather than ignored
  while a link bullet still is not. Pinned from both sides: narrowing it to one
  character fails four tests, widening it to any `]` anywhere fails the two link
  tests.

- **A checklist box that could never have been ticked honestly.** It asked that
  `docs/provenance.md` state "that no NES core was ever opened" -- and that
  document's own § *Do not self-certify* says never to assert "no third-party
  code is incorporated" or "licence-clean" as a finished claim. The box could
  only have been satisfied by writing the one sentence the project's provenance
  rules exist to prevent. The firewall half is what was actually being asked
  for; the self-certification half is **struck**, with the reason recorded in
  place.

- **`Distribution_MiSTer`'s selection rule, measured instead of paraphrased.**
  The checklist said it "selects the newest bitstream by the DATE in the
  filename", which is the wiki's wording. `Main_MiSTer/file_io.cpp` is sharper:
  `get_display_name()` searches for the literal `"_20"` and, on finding it, does
  `*p = 0` -- **truncating the display name there** -- taking the rest as
  `datecode`; absent, `datecode` becomes `"------"` and the name stays whole.
  `DirentComp()` groups by that truncated name and only compares datecodes
  within a group. `RustyNES_MiSTer-v2.6.13.rbf` has no `_20`, so **every
  released version is a separate core entry**, named
  `RustyNES_MiSTer-v2.6.13` rather than `RustyNES`, ordered alphabetically --
  which puts **v2.6.9 after v2.6.13**. It does not affect the Home folder, and
  has no effect until submission. The fix is one line, and it is not taken here:
  version-naming is a maintainer decision of 2026-08-30 with a stated rationale,
  and reversing it is not an audit's call.

- **`bump_release.py` dropped the previous release from two chains again**, in
  exactly the same two places as at v2.6.13 — `ROADMAP.md`'s "Built on" line and
  `to-dos/ROADMAP.md`'s release-line chain. Twice in consecutive releases makes
  it a script defect rather than an incident: the tool swaps the version token
  and leaves the chain, so the new release wears the old one's description and
  the old one vanishes from the lineage.

  **It is not fixed here, and the reason is that the gate makes deferring it
  safe**: `release_anchor_audit` catches both every time, by name and by file,
  and did so again before either reached a commit. Shape one is mechanically
  fixable — inserting `**vPREV "CODENAME"** and` after the "Built on" prefix needs only
  what the script already knows. Shape two is not: appending a chain entry needs
  a written summary, which is why it is manual, and the honest improvement there
  is to **refuse** rather than silently swap. Recorded with the evidence so the
  next version starts from a measurement.

  A third repair was needed for a different reason, and it is the gate's own
  documented trap: the audit finds the last version token before "the current
  release", and the new chain entry mentioned the *previous* version twice near
  its tail — so the label appeared to belong to v2.6.13. Reworded to name it
  once, at the front.

- **A claim v2.6.13 shipped, retracted.** Answering a review finding that "a
  five-mapper core" did not say which five, I wrote that the five were NROM,
  UxROM, CNROM, AxROM and MMC3 and that **MMC1 was the sixth approved family and
  not yet implemented**. Both halves are false: `rtl/cart/cart.sv` decodes mapper
  1 at three sites and `mapper1mmc1063` has been a registered gate since rung 7
  opened, and NROM landed at v2.6.3 rather than in rung 7's five. The approved
  six-family scope is **complete**. Asserted from memory inside a reply
  correcting somebody else's reading of the same line -- the project's rule is
  to verify a reviewer's claim before writing the fix, and the failure mode is
  verifying theirs and not your own. Retracted in the plan with the wrong text
  preserved, and on the public review thread.

## [2.6.13] - 2026-09-03 - "Slack" (the cartridge outgrows the die, and three consumers want the same bus)

### Added

- **An SDR SDRAM controller, a behavioural part model, and a four-way
  arbiter** in the sibling repository (`rtl/sdram.sv`, `rtl/sdram_arbiter.sv`,
  `tb/sdram_model.sv`), written from the AS4C32M16SB-7 datasheet revision 1.4.
  No third-party controller was read; ADR 0037 applies. The console does not use
  them yet -- the cartridge still answers from M10K -- but `rtl/emu.sv`
  instantiates both so they have a real driver and a real consumer. Two gates,
  `make -C tb sdram-gate` (three clock periods) and `make -C tb sdram-arb-gate`.
- **A request-contract check** in `tb/sdram_arb_tb.sv`: once the controller's
  `req` is asserted it must stay asserted, with every field unchanged, until
  `ack`. It replaces a latched column register that was measured redundant, and
  both of its clauses are demonstrated to fire.
- **A concurrent-requester phase** in the arbiter gate -- CHR, PRG and a loader
  write in flight together -- deliberately kept out of the latency measurement,
  because the loader runs before the console and folding writes into that
  traffic would let a fetch queue behind two accesses.
- **`sdram_sz` is consumed, validity bit first** (`rtl/sdram_presence.sv`,
  T-MISTER-SDRAM-SZ). The word powers up as `0x0000`, and "the HPS has not
  answered yet" is a different fact from "there is no board" -- a core that
  reads only `[1:0]` announces an absent add-on on a machine that has one, every
  time, until the HPS replies. So `absent` is deliberately **not** `!present`:
  both are false while the answer is unknown, and the arbiter's `ready` is gated
  on `usable`, so no access is ever granted against memory nothing has confirmed
  exists. The gate is **exhaustive** -- all 65,536 values against an independent
  model, since a sampled test of a pure function that small tests less for no
  saving -- and six mutations are CAUGHT, including the ticket's own defect.
- **`status_menumask` is computed rather than tied off** (`rtl/osd_menumask.sv`,
  T-MISTER-MENUMASK). The Reset entry is greyed out whenever the console is
  already held in reset -- no cartridge, or a mapper this core cannot decode --
  because a menu entry that does nothing when selected reads as a working
  feature. The ticket was filed as rung 6 on the grounds that "OSD behaviour is
  verifiable only by looking at an OSD", and that conflated two things: **the
  mask is a value this core computes**, and a gate asserts it with no display
  attached. Only whether the greyed entry *looks* right still needs eyes. Four
  mutations CAUGHT, including the `D`-prefix polarity, which was looked up
  rather than guessed.
- **A default gamepad mapping** (`jn,A,B,Select,Start`), name-identical --
  because any ergonomic remap is a claim about how a pad feels in the hand, and
  no hardware here has one attached.

- **The cartridge can leave the die, and the measurement says what it costs.**
  `cart.sv` takes a `USE_SDRAM` parameter -- the banking is identical either way,
  so a mapper verified against M10K is the same mapper on SDRAM, and what changes
  is only where the byte at a flat address is found. `rtl/cart_sdram.sv` bridges
  the console domain to the SDRAM one, and `make -C tb cart-sdram-gate` drives the
  whole path -- bridge, arbiter, controller, behavioural part -- at the console's
  real fetch rate: **14 cycles against a dot of 16**, with hits and misses, and
  the margin asserted. Configured off-die at 512 KiB PRG and 256 KiB CHR it
  compiles and takes block memory from **3,680,717 bits to 534,989**, RAM blocks
  from 468 to 84.
- **The open row.** Accesses no longer auto-precharge; a row is opened and left
  open, closed only when a request needs a different row of the same bank or
  before a refresh. A hit costs **6 cycles against 10** for a miss, and the worst
  fetch latency went from exactly the deadline to **14 against 16**. Rows are
  closed EARLY, in idle cycles, because leaving them open makes the refresh itself
  more expensive -- measured, at 19 cycles, before that was added.

### Fixed

- **The CAS latency that shipped was not the one anything tested.**
  `rtl/sdram.sv` defaulted to 3 and `rtl/emu.sv` took that default, while the
  protocol testbench pinned 3 and the arbiter testbench pinned 2 -- neither
  pinned the shipped value, and the shipped value **misses the PPU's fetch
  deadline**: 17 cycles against a budget of 16, where CL2 makes it at exactly 16.
  Table 16 gives the -7 part tCK = 10 ns at CL2 and 7 ns at CL3, so at this
  design's 11.64 ns both are legal and CL3 was simply the slower of two correct
  choices, picked as "conservative" before anything measured what a cycle was
  worth. CAS latency is now **derived from the clock period** and cannot be
  shadowed; both overrides are deleted, and the protocol gate gains an 8000 ps
  period -- below tCK(CL2) -- so CL3 stays covered without a testbench choosing
  it.
- **A request may not be a pulse.** The arbiter drove `req` for a single cycle to
  save the cycle that registering the grant costs. The controller samples `req`
  in `S_IDLE` and is not always there -- a refresh takes it away -- so a request
  pulsed during one is lost, not deferred. The gate's first honest run failed on
  the loader's very first write. The grant stays combinational, because that
  cycle is the difference between 17 and 16; the request is now held until `ack`.
- **The same value computed in two places, and the copy that wins is the one a
  mutation does not touch.** The arbiter computed the granted access's address,
  write-enable, data and mask combinationally for the grant cycle and again in
  the `always_ff` that captured them. The controller consumes the *registered*
  copy almost always, because on the grant cycle it is usually mid-refresh or in
  recovery -- so a DQM mutation came back NOT CAUGHT with the unmutated mask
  arriving at the part. Collapsed to one expression.
- **The `$2007` port asked for the RAW address, so every banked-CHR board read
  BANK ZERO.** The handshake carried `chr_addr` -- the fourteen bits the PPU
  presents -- and the bridge added the CHR region base to it. For NROM that is
  right by coincidence, because the mapper's translation is the identity there,
  which is why `ppu-misc-2007-stress` passed off-die throughout and why the
  defect shipped inside the same change that fixed the port's latency. The two
  CNROM gates could see it and did: the oracle reads `$01`, `$01`, `$02` from
  `$2007` at cycles 61, 85 and 89, and the DUT read `$00` every time.

  **The fix removes the address rather than correcting it.** `cart.sv` already
  publishes `chr_flat`, the mapper's translation, for the fetch path; the
  `$2007` request now carries no address at all and the bridge takes that same
  value, so there is one source of truth for where CHR lives instead of two that
  agree only on NROM. It is captured six cycles after the request rather than on
  its edge, because that translation is registered on the CONSOLE clock -- a
  quarter the rate of the SDRAM one -- so on the edge it still holds the previous
  dot's fetch address.

- **A ONE-CYCLE margin, found by nearly deleting the thing that provided it.**
  The `$2007` request is held twice: six cycles for the address, and then until
  the cycle after the CPU's own PRG read is answered -- the widest gap a CPU
  cycle has, affordable because this port's byte is not read for thousands of
  cycles. The second wait was built for the PRG mismatch that accompanied the
  wrong buffer byte, and once the address was fixed a control that issued at a
  flat **+7** with no deferral passed both CNROM gates, which read as the
  deferral buying nothing. It was removed. The deployed code then issued at
  **+6**, and both gates FAILED again -- `cart PRG disagrees at $C015`.

  **The control and the code differed by one cycle**, and that is the finding:
  the CPU's deadline off the die is thin enough that a single cycle of another
  requester's placement decides it. Choosing +7 because it happens to pass would
  be fitting a constant to a gate; waiting for the CPU's own completion is a
  reason, so the deferral stays and the measurement is recorded beside it. It is
  also the concrete argument for scheduling the bus rather than arbitrating it.

- **The bitstream is `releases/RustyNES_MiSTer-v2.6.13.rbf`**, md5
  `2c2fa6eb0f751b2c9b60fcf903f2bf3f`, built by Quartus Prime Lite 17.0.2 on a
  5CSEBA6U23I7 at fitter seed 3: **0 errors**, a warning set matching the pinned
  manifest exactly, and timing closed at all four corners -- worst setup
  **+0.317 ns** and worst hold **+0.068 ns**, both at Slow 1100mV -40C, which is
  the binding corner and is discovered by pattern rather than assumed. It
  supersedes v2.6.12's only by being newer; no defect is known in that one.

- **The harness could run one configuration's binary under the other's name.**
  `USE_SDRAM` reaches Verilator as a `-G` parameter rather than as a file, so
  switching it changes what the binary IS while leaving every prerequisite older
  than the target. Make then reports the target up to date and the next suite
  runs the previous configuration's build. It happened: an off-die ladder
  followed by an on-die one, with nothing edited between them, produced two logs
  from **one binary** built at 14:36, and the second was labelled on-die while
  being the off-die build. It reproduced the off-die failures exactly, which is
  what made it convincing -- and what made it nearly pass as a real result.
  Earlier pairs escaped only by accident, because an RTL edit between them made
  the sources newer.

  The build now takes a stamp file named for the value as a prerequisite, so
  switching removes the existing stamp and creates a missing one, which is newer
  than the target and forces the rebuild. Demonstrated by mutation: with the
  prerequisite removed a switch schedules **zero** rebuilds, with it, one.

- **Two inferred LATCHES, in a module that has none and cannot have any.**
  `ppu2c02.sv`'s `$2007` handshake assigns `pd_rd_addr` and `pd_fill_pend` only
  under `BUFFER_HANDSHAKE`, so in the shipped on-die build -- where that
  parameter is false -- the only assignment either of them reached was the reset
  branch. A variable that holds its previous value on every live path is a
  latch, and Quartus said so twice (warning 10240, both naming the same
  `always_ff`). **Verilator cannot see this**: it elaborates the same dead
  branches and has no opinion about what they become in gates, so the lint gate
  was green throughout. `pd_rd_req` escaped only because its clear is
  unconditional, which is the shape the other two now have. The comment beside
  them asserted the defect as a virtue -- "outside BUFFER_HANDSHAKE neither
  signal ever moves" -- which is true, and is exactly the condition that infers
  the latch. Both `tb/quartus_clean.py` and `tb/check_warnings.py` caught it
  independently, the second by the warning SET rather than a count.

- **Two files were missing from `files.qip`**, caught by the `check_qip` gate
  added last release for exactly this -- the same defect, one release later, in
  the same subsystem. `rtl/sdram.sv` first: the controller was written, linted
  and gated in simulation while the synthesis list had never heard of it.
  `rtl/sdram_arbiter.sv` went the same way when it landed. Both directions of the
  gate are mutated -- a file in `rtl/` and not in the list, and a line in the list
  naming a file that does not exist.

### Changed

- **The CHR fetch budget is PER-CONSUMER, and that is the release's real
  finding.** The whole SDRAM effort was measured against one dot of lead -- 16
  cycles -- a number read off the fetch structure, written down as conservative,
  flagged as unverified, and then used as though it had been measured. `nes_top`
  now takes a `CHR_LAT` parameter that pipelines the CHR byte, zero by default and
  the identity there; raising it until a co-simulation gate fails asks the console
  directly. The answer is not one number:

  | consumer | extra clocks | total lead | budget |
  |---|---|---|---|
  | background and sprite fetch | 6 | 7 clocks | 28 cycles |
  | **the `$2007` data port** | **1** | **2 clocks** | **8 cycles** |

  Both framebuffer gates agree to the pixel, matching at 0 through 6 and breaking
  at 7. `ppu-misc-2007-stress` passes at 0 and 1 and returns **Fail(test 2)** at
  2. The binding consumer is the one that tolerates least, so the budget is
  **eight**, not 28 and not 16.
- **Three combinational shortcuts are reverted**, having been bought against the
  wrong number: the bridge's CHR request, its return bypass and the arbiter's
  grant. Each cost real timing -- with the bridge merely BUILT and unused its
  address compare reached the arbiter's grant and cost 0.176 ns of setup. All
  three are registered, and the whole path measures 17 cycles.
- **The cartridge passes the whole ladder off the die, and still ships on it.**
  Configured off-die at 512 KiB PRG and 256 KiB CHR it compiles with 0 errors,
  takes block memory **3,680,717 bits to 534,989** and RAM blocks **468 to 84**,
  closes timing with *better* margin than the on-die build, and passes **142 of
  142** co-simulation gates -- every gate the on-die console passes, against a
  cartridge answering from a behavioural SDRAM part.

  Three consumers want a cartridge byte and each was measured by pipelining it
  until a gate failed: the background fetch has 28 cycles and uses 17; the
  `$2007` port has 8, of which four are the console-domain crossing alone; the
  CPU sampling at mc7 has 24 after that crossing, and a PRG read behind a CHR
  fetch takes **28** under the arbiter gate's densest synthetic pattern. No gate
  in the ladder reaches that pattern -- but the margin is one cycle wide, which
  the deferral experiment measured directly, so the shortfall is a real property
  of on-demand arbitration rather than an artefact of the stimulus. What removes
  it is **scheduling rather than arbitration**: the console's access pattern is
  deterministic, so each requester can have a guaranteed slot. That is a redesign
  of the arbiter, not a tuning of it.

  **The reason it is not the shipped configuration is none of the above.** An
  off-die core cannot run at all without the SDRAM add-on, so shipping it would
  make the core unusable on a bare DE10-Nano that the on-die build serves today,
  in exchange for headroom nothing yet needs -- rung 7's five mapper families fit
  on the die at 468 of 553 M10K blocks. The flip belongs to the release where a
  board actually needs the space, and it will need a fallback for a missing one.
- **The `$2007` data port gets its byte through a handshake, not over the bus**,
  and that one is closed. It tolerates two master clocks where a fetch tolerates
  seven, so half its budget is the crossing and it could never fit. `ppu2c02`
  takes a `BUFFER_HANDSHAKE` parameter and asks through the arbiter's fourth
  port, filling the read buffer when the byte arrives -- affordable because the
  CPU does not read that buffer until its next `$2007` access. **The shared bus
  is untouched**: `chr_addr` still carries the composed {v high, octal latch} at
  t4, so the access still steals the bus from a fetch and corrupts it exactly as
  the ALE and hybrid-address ROMs require. On the die the parameter is 0 and the
  old path is byte-identical. `ppu-misc-2007-stress` passes off-die with all 512
  stored results identical to the on-die run -- and that gate is the one that
  could not see the defect below.
- **Two clock-domain defects, and they are the same defect twice.** A signal
  crossing the 4:1 boundary with the wrong shape, in each direction. `pd_rd_req`
  is one DOT wide and the bridge took it as a level, re-strobing on all sixteen
  SDRAM edges: **168 requests produced 43 fills**. `pd_valid` was one SDRAM clock
  wide and the PPU samples on the console clock, invisible three times in four:
  **five requests produced ONE fill**. Neither showed as a wrong number in a
  trace -- both showed as the read buffer quietly keeping its previous value,
  which is why counting found them and reading did not.
- **The console-domain crossing is not optional.** Publishing the flat address
  combinationally carries the mapper banking -- including a runtime modulo -- into
  an 11.64 ns domain, and Quartus misses setup by **-24.769 ns**, TNS -6,615 ns,
  the failing path naming `ppu2c02|scanline[6]` to `sdram_inst|sdram_dq_out[11]`.
  A multicycle constraint is the obvious answer and is *not safe*: the controller
  samples its request every cycle, so it would be permission to act on a
  half-settled address. Registering the port is worth **-24.769 to -0.176 ns**.
- **`AUDIO_MIX` was planned, and is DECLINED on measurement.** The framework's
  `mix` blends the left and right channels into each other, and this core
  assigns `AUDIO_L` and `AUDIO_R` from the same wire -- it is mono by
  construction, so every blend setting is an identity. Adding the option would
  have put an OSD entry on screen that does nothing when selected, which is the
  `VGA_SL` defect v2.6.6 found, added deliberately this time. Recorded at the
  tie-off with the reason.
- **`LED_DISK` is correct as it stands, and was nearly "fixed".** `led_disk[1]`
  is an *override* bit: with it clear the framework drives the LED from the
  HPS's own disk activity, so zero means "show the framework's activity" and
  asserting the override would suppress it in exchange for showing nothing. It
  becomes ours to drive when the core has disk activity of its own, which is the
  battery-save path.
- **An elaboration-time `$error` is forbidden in synthesisable RTL.** A
  parameter range check written as a guarded `$error` at module scope is legal
  SystemVerilog, is accepted by Verilator, linted clean and passed all three
  protocol-gate periods -- and Quartus 17.0.2 fails to *parse* it, reporting the
  guard rather than the construct, leaving the design unbuildable. The check
  moved to the part model, where a `$fatal` is already the idiom, and
  `tb/check_rtl_subset.py` now refuses the construct; the rule is demonstrated to
  fire on it and demonstrated not to fire on the model's legitimate `$fatal`.

## [2.6.12] - 2026-09-02 - "Groundwork" (the bitstream was an NROM-only console)

Rung 7 landed five mapper families and 142 co-simulation gates verify them. The
layer that turns that RTL into a bitstream was never told.

**The emulation core is unchanged.** No chip crate changes, so AccuracyCoin
141/141 (RAM decoder) and nestest 0-diff hold by construction.

### Fixed

- **Three `nes_top` inputs were unconnected, and Quartus tied them to ground.**
  `rtl/emu.sv` never connected `cart_mapper`, `cart_prg_16k_banks` or
  `cart_chr_8k_banks`. The tool said so, three times, in a
  `Port Connectivity Checks: "emu:emu|nes_top:u_nes"` table:

  ```text
  ; cart_mapper ; Input ; Warning ; Declared by entity but not connected by
  ;             ;       ;         ; instance. ... the port will be connected to GND.
  ```

  In the shipped v2.6.11 bitstream that meant **mapper 0 for every cartridge**,
  `prg_8k_count = 0` pinning `prg_bank_sel` to zero and collapsing PRG to an
  **8 KiB window**, and CHR forced to RAM with 8 slots. The fitter's own RAM
  summary agreed exactly:

  | memory | declared | implemented before | after |
  |---|---|---|---|
  | `prg` | 256 KiB | **8 KiB** | **256 KiB** |
  | `chr` | 128 KiB | **8 KiB** | **128 KiB** |
  | `prg_ram` | 8 KiB | 8 KiB | 8 KiB |
  | work RAM | 2 KiB | 2 KiB | 2 KiB |

  The two arrays whose size depends on a tied-off input are the two that
  collapsed; the two that do not were correct. That is the mechanism, not a
  coincidence. Block memory **666,061 -> 3,680,717 bits**; RAM blocks
  **100 -> 468 of 553**. Timing still closes at all four corners (+0.225 ns
  setup, +0.105 ns hold -- a BETTER hold margin than v2.6.11's +0.078).

  **`emu.sv` said it in its own OSD string**, honestly, when it was written at
  v2.6.6: `"Unsupported mapper - this core is NROM only"`. v2.6.9 added MMC1,
  UxROM, CNROM, MMC3 and AxROM to `cart.sv`; nothing updated the layer above.
  **Two of the six titles in v2.6.11's montage are larger than the fitted
  cartridge** -- *Bad Dudes* at 128 KiB CHR and *Battletoads & Double Dragon* at
  256 KiB PRG -- so that release's claim is qualified here: true of the RTL under
  Verilator, which builds the full array, and not of the bitstream.

- **The loader could not reach past 128 KiB.** `cart_load_addr` was 17 bits
  against a 19-bit port, so a 256 KiB PRG image could not be filled even once
  the array existed. Widened, with `prg_bytes`' truncation in the CHR offset
  fixed alongside it.

- **Header byte 5 was never captured.** `emu.sv` parsed the mapper number only
  to REFUSE anything but NROM; the CHR bank count it needed to size the
  cartridge was not read at all.

### Added

- **`tb/check_pins.py`** -- fails when a module this repository declares has an
  unconnected pin. Scoped by the module we own rather than a pin-name list,
  because `sys/` modules legitimately leave dozens open (82 such warnings are
  present and correctly ignored). **Mutation: the pre-fix `emu.sv` is CAUGHT**,
  naming `nes_top` and all three pins.

- **`tb/check_warnings.py` and `tb/quartus-warnings.txt`** -- pin the warning
  SET, not the count. Three mutations, all caught: a new warning, a pinned one
  that stops firing, and an empty report. Every pinned entry carries why it is
  acceptable; all three originate in `sys/`, which may not be edited.

- **`emu.sv` is linted at all.** It was the ONE file under `rtl/` no lint target
  reached -- it needs `sys/` on the include path and a `BUILD_DATE` that only a
  Quartus compile generates, which is a define, not a reason. Verilator's own
  `PINMISSING` names the defect exactly.

### Changed

- **`hps_io`'s 25 unconnected inputs are tied off explicitly**, extending a
  precedent the file already set for `ioctl_wait`: *"a floating input that
  happens to read as zero is not the same as saying it is zero."* Every value is
  the one Quartus was already defaulting to, and that is **verified rather than
  asserted -- the bitstream is byte-identical across the change** (`f0ddb3fa...`).
  Outputs are deliberately left alone: wiring 51 of them to dummies would trade
  a warning that says "unused" for a suppressed one that says "assigned and
  never read".

- **Each tie-off says whether it is NOT APPLICABLE or NOT IMPLEMENTED**, because
  a tie-off that reads as the first when it is the second is the same defect as
  the OSD string. **Nine** `T-MISTER-*` tickets are minted for the second group
  (seven headings, three of them sharing one) -- the largest being
  **battery-backed save RAM, which is a data-loss bug**: `cart.sv` has 8 KiB of
  PRG-RAM and nothing persists it, so every MMC1/MMC3 battery game loses its
  saves at power-off.

- **Annotating the nine with a blocker turned "none of these landed" from a
  scoping choice into a measurement: not one is blocked on EFFORT.** Four are
  deferred to v2.8+ by the approved plan, three cannot be verified by anything
  in this repository, and two wait on unwritten subsystems -- so the list is the
  **rung-6 agenda**, not a backlog. `T-MISTER-SAVE` was attempted, and the
  attempt is what established it: its ticket claimed the implementation was
  unblocked and only the verification was rung 6, and that split does not
  survive `sys/hps_io.sv`. This `sys/` exposes **no OSD-close signal** to flush
  on (the nearest available, `buttons`, is already the reset button in
  `emu.sv`'s own reset expression); `hps_io.sv:152` states in its own port
  comment that `ioctl_upload_req` "must be supported on HPS side for specific
  core"; and `sd_*` and `ioctl_*` both terminate in `hps_io`, which the
  testbench does not instantiate. A save path written now would ship with zero
  evidence. The attempt is preserved as a patch rather than committed, and
  `cart.sv` deliberately carries **no** save port -- dead infrastructure for a
  feature that cannot land is worse than none, and `check_pins.py` would have
  had to be lied to in order to accept it. The nine were also first filed under
  a heading reading "cases where the DUT is measurably more accurate than
  RustyNES itself"; none of them is, and that is corrected in place rather than
  quietly moved, because a ticket is read by its heading.

### Fixed -- the new gate had the defect it was written to close

- **`tb/check_pins.py` reported a pass when the lint had not run.** Raised in
  review and **reproduced on the first try**: an empty capture and a Verilator
  run that died early both printed `PASS: no module under rtl/ has an
  unconnected pin` after examining zero warnings. The script failed closed on an
  empty MODULE set and not on empty INPUT -- an absent result reading as a clean
  one, inside the gate written to close the previous instance of it. The
  `|| true` in the recipe must stay (two `sys/` modules are Quartus
  megafunctions with no source Verilator can see, so a clean lint ALWAYS exits
  non-zero), so the discrimination moved into the script: non-empty input; every
  coded `%Error` an expected `MODMISSING`, cross-checked against Verilator's own
  error count; and **at least one `PINMISSING` warning seen**, since `sys/`
  produces 57 on every healthy run, so zero means the lint never reached the
  design. Four mutations, four different controls, all caught, real run still
  passing. Control 2 needed a second pass, found by running the real lint
  against the first version: `%Error: Exiting due to 2 error(s)` is itself a
  `%Error` naming no module, so an allowlist over every `%Error` rejected a
  HEALTHY run.

- **The control's first CI run found something worse than the report: the gate
  had been INERT on CI its entire life.** The successful run immediately before
  the control was added reads `check_pins: 0 PINMISSING warning(s) examined` and
  `PASS` -- so it was not merely capable of a false pass, it was delivering one
  on every CI run. Both causes are version skew between CI's Verilator 5.020 and
  the 5.050 here, in **opposite directions**: 5.050 writes `%Error-MODMISSING:`
  where 5.020 writes a bare `%Error:` (so the allowlist refused a HEALTHY run),
  and 5.050 writes "Instance has missing pin" where older releases write "Cell
  has missing pin" (so the warning regex matched nothing). Both wordings are
  accepted now and **CI reports 57 examined where it reported 0**. Second time
  this project has assumed the newer Verilator is the louder one. The control
  also diagnoses itself now, printing every distinct message code in the capture
  when it fires.

- **`check_pins.py` carries a `--self-test` and its verdict is not trusted
  without one**, mirroring `check_timing.py`. Six fixtures -- both Verilator
  message formats and all four failure modes -- with the judging logic
  EXTRACTED so the self-test drives the real implementation rather than a copy
  of it. Reintroducing either real defect fails it, each on the 5.020 fixture.

- **`scripts/release-rbf.sh` now feeds `check_warnings.py` both reports.**
  Recorded as what it is: today the two invocations give the **identical**
  answer, because all three warnings appear in the map report and the Fitter
  merely re-emits one. A latent gap closed, not a live defect fixed.

### Fixed -- the release tooling deleted a release, and its own gate caught it

- **`bump_release.py`'s `PERIOD` shape assumed a period ENDS the statement.**
  Its rule reads "the statement ends at the codename: nothing describes the
  release, so there is nothing to demote", which is true of one of this
  repository's two `PERIOD` anchors and false of the other. `OVERVIEW.md`
  genuinely ends its sentence; `SECURITY.md`'s period is a **separator before a
  lineage chain** (`... **v2.6.11 "Exposure"**. Built on **v2.6.10 ...`), so
  "swap and stop" replaced the head and left v2.6.11 **absent** rather than
  stale -- the exact defect v2.6.11 found and fixed in the documents that were
  not this one. Split into `PERIOD` and `PERIOD_CHAIN`, with four selftests,
  because the script's own comment records that `PERIOD` and `DATED_CODE` "were
  added without selftests and review caught it". Collapsing the classifier back
  fails three of them, one naming the defect outright.

- **Two further anchors needed hand correction and the v2.6.11 chain gates named
  both** -- a chain in `ROADMAP.md` whose first link had become v2.6.10, and one
  in `to-dos/ROADMAP.md` ending "v2.6.11, the current release".
  `a_release_line_chain_does_not_skip_a_release` is one release old and has now
  caught a real skip in each of its two releases.

### Unchanged, and stated rather than assumed

- **The co-simulation suite cannot verify this fix, and that is the finding.**
  `emu.sv` is absent from the testbench's file list, so all 142 gates exercised
  a correctly-configured cartridge -- the harness drives those ports itself.
  `cart.sv` and `nes_top.sv` are byte-unchanged. The suite is a regression check
  here, not a verification, and only synthesis could ask the question.

## [2.6.11] - 2026-09-02 - "Exposure" (a picture is a gate the ladder did not have)

All 141 gates were green and two of six commercial games rendered wrong.

**The emulation core is unchanged.** No chip crate changes, so AccuracyCoin
141/141 (RAM decoder) and nestest 0-diff hold by construction. The work is in
the sibling's RTL, in the montage that found it, and in two release-documentation
gates.

### Fixed

- **A CHR-RAM write was taking the shared-pin address built for fetches.**
  `RustyNES_MiSTer/rtl/ppu2c02.sv` presents one address to the cartridge:

  ```systemverilog
  assign chr_addr = ((dot[0] == 1'b0) || (pd_stage == 3'd4))
                      ? {chr_addr_raw[13:8], octal_latch}
                      : chr_addr_raw;
  ```

  That composite is correct and load-bearing -- it models the 2C02's shared
  address/data pins, and AccuracyCoin's `ALE + Read` and `Hybrid Addresses` both
  depend on it. It is the wrong address for a **write**: a `$2007` write must
  land at `v`, and the octal latch is not `v`. The fix separates the two paths
  rather than changing either, exposing `chr_wr_addr = chr_addr_raw` for the
  cartridge's write port and leaving the read and fetch paths untouched.

  Measured, one commercial title per supported board, DUT framebuffer against
  the oracle's over all 61,440 pixels:

  | board | game | CHR | wrong pixels |
  |---|---|---|---:|
  | NROM | 1942 | 8 KiB ROM | 0 |
  | MMC1 | Adventures of Lolo | 32 KiB ROM | 0 |
  | CNROM | Arkanoid | 16 KiB ROM | 0 |
  | MMC3 | Bad Dudes | 128 KiB ROM | 0 |
  | UxROM | 1943: The Battle of Midway | **RAM** | **16,565 -> 0** |
  | AxROM | Battletoads & Double Dragon | **RAM** | **1,702 -> 0** |

  The split is exactly CHR-ROM against CHR-RAM, which named the mechanism before
  any tracing. A **control** says it is not v2.6.10's regression: that release
  folded `chr` to a single write port and touched the same signal, and the
  pre-M10K-fix RTL differs from the oracle by the **identical 16,565 pixels**.
  The defect dates from the cartridge landing in v2.6.9.

  **Why 141 gates could not see it**, and not because the path is unreached --
  the DUT asserts `chr_wr` **9,600 times** in the Battletoads run. Only **three**
  gates compare a framebuffer, and all three ship CHR-ROM, so `chr_wr` never
  fires in the only gates that would render the consequence. Every other gate is
  CPU-side, and AccuracyCoin -- the widest gate in the suite -- is CHR-ROM too.
  The rung-7 gates' own comment says what they are for: *"these gates are about
  BANKING and nothing else."*

  **The v2.6.10 bitstream carries this defect.** A published version is
  immutable, so it cannot be back-fitted; `RustyNES_MiSTer-v2.6.11.rbf` is built
  from the corrected RTL.

- **Bus conflicts: the oracle modelled them for CNROM and the DUT did not.**
  `rtl/cart/cart.sv` said so in its header, named it as a decision, and ended
  with the right instruction -- *"If a golden ever disagrees on a write to a
  mapped address, this is the first thing to suspect."* No golden ever
  disagreed, and the reason is in the harness rather than the console:
  `mkmapper.py`'s `_rom` writes **`$FF` at the select address** so that "a bus
  conflict (modelled or not) is the identity on the written byte". Every mapper
  gate is immune to the behaviour **by construction**.

  **The rule is the wiki's**, stated outright on `Bus_conflict.xhtml`: the CPU
  and the mask ROMs both "drive a 0 more strongly than a 1", which "implies that
  an emulator should use the bitwise AND of the value from the CPU and the value
  from the ROM". What is a measurement rather than a reading is WHICH BOARD --
  the oracle's `m003_cnrom.rs` ANDs with the PRG byte, so the two consoles
  disagreed on every mapper-3 write whose value differed from the ROM byte, and
  nothing could see it.

  `mapper3cnrombusconflict066` writes to addresses holding `$01` and `$00`
  instead, with two `$FF` controls so a broken register stays distinguishable
  from a missing conflict. It **failed on two of its four probes before the fix**
  (`$03` where the oracle said `$01` and `$00`), and after it the entire 2 KiB of
  work RAM is identical, with **146 nine-field checkpoints** matching.
  `mapper3cnrom061` is unaffected in both directions -- its select address holds
  `$FF`, so the AND is the identity there. Two mutations CAUGHT: dropping the
  AND, and ANDing with a constant that satisfies one probe alone.

  **UxROM, AxROM and NROM stay unmodelled**, and for the same reason rather than
  a different one: the oracle does not model them either, so modelling them here
  would **create** a divergence rather than close one. That leaves a shared
  inaccuracy against hardware on mapper 2, named in `docs/rung7-mappers.md`
  rather than fixed on one side. The old comment justified the omission with the
  wiki's "the relevant games all work around this in software" -- which the wiki
  says about **mapper 2**, and not about mapper 3, where it was being applied.

- **Eight release leads described v2.6.10 as v2.6.9, and v2.6.9 vanished from the
  lineage.** The v2.6.10 bump moved the version token and left the summary prose
  after it, so every lead read `v2.6.10 "Inference" -- <v2.6.9's summary> ...
  Built on **v2.6.8**`. `docs/STATUS.md` was worst: it named **v2.6.10 with the
  codename "Abeyance"**. Every existing check passed, each correctly -- they pin
  the version token, and the token was right.

- **`crates/rustynes-cosim/Cargo.lock` still recorded 2.6.9.** The crate is
  excluded from the workspace, so `cargo build --workspace` never refreshes it.

### Added

- **`RustyNES_MiSTer/screenshots/montage.png`** and a "What it renders" section
  in the sibling README: six commercial titles rendered by the RTL under
  Verilator, each byte-identical to the oracle. It states what the picture is
  not -- not hardware, one frame each, and the palette applied *afterwards*,
  because what the RTL emits and what the ladder gates is the pre-palette
  `index_framebuffer`.

- **`RustyNES_MiSTer/tb/make_montage.sh`**, which takes `--verify` and **refuses
  to publish a tile that differs from the oracle**, and fails closed on a missing
  capture rather than producing a five-tile montage that still looks finished.

- **Two release-documentation gates**, both demonstrated to fail by mutation.
  Prose cannot be audited; an ordering can.
  - `a_release_line_chain_does_not_skip_a_release` -- the first release a chain
    names after the current one must be the release immediately before it in the
    CHANGELOG, derived rather than written down. It walks **every** chain in a
    file: `ROADMAP.md` carries two and the drift reached both.
  - `a_codename_near_the_current_version_is_the_changelog_codename` -- a codename
    quoted within 40 characters of the current version, past a date or a dash.
    A second check rather than a loosening of the first, so the deliberate
    "states a version only" path is unchanged.

  Three findings from building them, each recorded because each cost something:

  - **The hand-written document list was wrong on its first use.** It named six
    documents; the drift reached **eight**, and `SUPPORT.md` and `SECURITY.md`
    were not on it. The list is now DERIVED from the `ANCHORS` table -- a hand
    list silently omits exactly as a glob silently widens, and `ANCHORS` is
    already this file's single answer to "where does a release claim live".
  - **The codename gate found a false positive on its own first run**, before it
    shipped: `README.md`'s version badge puts the version inside an HTML
    attribute, where the nearest quote is a delimiter and the "codename" it
    yields is a space followed by `alt=`. The narrower rule tried first -- reject a gap containing
    `=<>/` -- did **not** catch it, because the offending gap is `-blue.svg`.
    The rule that works is that a version-to-codename gap never contains a
    **letter**.
  - **The chain gate caught `bump_release.py` itself.** Run for this release,
    the script left two documents un-demoted and two chains still headed at
    v2.6.9, and the gate named all four on its first real use. Two of the four
    turned out to be the gate's own marker being too narrow for the script's
    second link spelling (`, on **vX.Y.Z` beside `Built on **vX.Y.Z`, 22
    occurrences of the former in `AGENTS.md` alone) -- which is how that
    widening came to be measured rather than guessed.

- **The bitstream is rebuilt, every seed is swept, and "reproducible" is
  corrected while it is re-established.** Two RTL changes land, so v2.6.10's
  published `+0.531 / +0.099` describes RTL that no longer exists. The committed
  configuration built at the previous seed and **closed** (+0.162 ns setup,
  +0.064 ns hold, positive at all four corners) -- a pass, and thin, so all five
  seeds were swept and the **whole table** is published rather than its winner:
  `+0.440/+0.078`, `+0.411/+0.062`, `+0.162/+0.064`, `+0.550/+0.064`,
  `+0.361/+0.071`. All five close. **The criterion is stated: maximise the
  BINDING margin** -- hold is binding on every seed, so seed 4's larger setup
  buys margin on the constraint this design is not close on. Seed 1 ships, with
  22% more binding margin than the release opened with.

  Two builds of the identical configuration then had **identical timing at every
  corner, to the digit, and different bitstream bytes**. The framework does it
  deliberately: `sys/build_id.tcl` regenerates a gitignored `build_id.v` on every
  compile with a **date** stamp, so a build is reproducible *for a given day*.
  The two straddled midnight; a third, same-day, reproduces it exactly. v2.6.7's
  claim is **not withdrawn but qualified** -- it was true, and both of its
  compiles were same-day, and no release before this one spanned midnight.

### Unchanged, and verified rather than asserted

- **The emulation core is unchanged** -- no chip crate changed.
- **The co-simulation suite re-run twice**, because both changes reach past the
  mappers: the PPU fix touches `ppu2c02.sv` and therefore every rung-3 gate and
  AccuracyCoin's 146-entry vector, and the bus conflict changes `cart.sv`. Final
  run **142 passed, 0 failed, 0 skipped** -- 141 before the bus-conflict gate
  was added, so the suite grew by exactly the gate this release wrote.
- **Rung 6 does not close.** No DE10-Nano and no SuperStation One are attached,
  confirmed by checking rather than assumed.

## [2.6.10] - 2026-09-01 - "Inference" (the cartridge meets the synthesiser)

### Fixed

- **The rung-7 cartridge could not be synthesised, and nothing had asked.**
  v2.6.9 landed five cartridge boards verified across 141 co-simulation gates,
  and **not one of them had ever been through Quartus**. Building that release's
  bitstream is what asked, and Analysis & Synthesis refused the design:

  ```text
  Error (276003): Cannot convert all sets of registers into RAM megafunctions
  ```

  `chr` was written from **two separate `always_ff` blocks** -- the ROM load
  port and the CHR-RAM write path. An array with two writers in two different
  always blocks cannot infer as a single M10K, so Quartus kept 128 KB of CHR in
  flip-flops: **1,048,576 registers against roughly 166,000** on a
  5CSEBA6U23I7. Folding both writers into one port through an `always_comb` mux
  fixes it -- the loader takes priority, which is safe because the CPU is held
  during ROM load, so `chr_wr` cannot be asserted at the same time.

  | | before | after |
  |---|---|---|
  | Analysis & Synthesis | **unsuccessful**, 1 error (276003) | successful, 0 errors |
  | `prg` / `chr` / `prg_ram` | not inferred | all three `altsyncram` |
  | total block memory bits | -- | 666,054 |

  **This is v2.6.6's finding one layer out.** That release established that an
  M10K read is registered and rewrote 40 KiB of asynchronously-read cartridge;
  this one establishes that a correctly *registered* memory still will not infer
  if it has two writers. Both were invisible until something outside the
  simulator was asked, and **simulation cannot ask this question** -- Verilator
  accepts both forms without complaint.

### Added

- **The bitstream v2.6.9 could not produce.** `releases/RustyNES_MiSTer-v2.6.10.rbf`,
  built from this RTL, verified against the Quartus reports rather than the exit
  code, and attached to the GitHub release on both repositories. v2.6.9 shipped
  without one and disclosed why; a published version is immutable here, so the
  fix could not be back-fitted to it.

### Changed

- **The fitter was throttling itself, and the `.qsf` carried no optimisation
  assignments at all.** The first v2.6.10 compile failed timing at **-0.265 ns**
  on the framework's HDMI PLL path while the Fitter log said
  `Info (171003): ... Auto Fit compilation, which may decrease Fitter effort`.
  `FITTER_EFFORT "STANDARD FIT"` and `OPTIMIZATION_MODE "Aggressive Performance"`
  fix it. `PLACEMENT_EFFORT_MULTIPLIER` is deliberately NOT set -- it cannot be
  raised above 1.0 directly.
  - **A seed sweep found the shape, which matters more than the winner.** Under
    Auto Fit, seeds 4 and 5 measured -0.265 and -0.059, both failing. Under full
    effort **all six seeds close**, +0.027 to +0.531 -- the effort settings move
    the whole distribution across zero and the seed only picks where in it you
    land. This project had been pinned to seed 4, **the worst of the six**.
  - Pinned at **seed 3: worst setup +0.531 ns, worst hold +0.099 ns** at the
    binding corners, roughly 20x the margin the first closing build had. Stated
    honestly, that is "best of six placements", not headroom the design earned.
  - The build is **byte-identical across two independent compiles** at this
    seed (md5 `94b7d855...`), measured rather than argued.

### Unchanged, and verified rather than asserted

- **The emulation core is unchanged** -- no chip crate changed, so AccuracyCoin
  141/141 (RAM decoder) and nestest 0-diff hold by construction.
- **Co-simulation suite re-run against the new write port**, because muxing it
  changes when a CHR-RAM write lands relative to a load, and only the suite can
  say whether that is observable.

## [2.6.9] - 2026-08-31 - "Abeyance" (an exclusion hides improvement as well as regression)

### Fixed

- **The MMC3 scanline-IRQ residual closes, and it was ONE CYCLE rather than the
  one SCANLINE two earlier readings recorded.** `/IRQ` is now a **registered
  output**: the pending flag is a flip-flop and the CPU samples the pin, so what
  the CPU sees trails the counter reaching zero by one CPU cycle. blargg's
  `4-scanline_timing` goes from `$02` -- failing its FIRST assertion -- to
  `$0C`, so **sub-tests 2 through 11 now pass**, with `1-clocking`,
  `2-details` and `5-MMC3` still green and `6-MMC3_alt` still failing correctly
  as the revision this core does not model.
  - **Why one cycle, from blargg's own arithmetic rather than from tuning.** For
    sub-test 2 the ROM's `cli` is fetched at cycle 1,250,755, so `end_` runs
    cli(755-756) nop(757-758) nop(759-760) `inc irq_flag`(761-765). The filtered
    A12 rise is at 1,250,759. Asserting combinationally raises `/IRQ` at
    1,250,760 -- inside the second nop -- so the handler beats the `inc`. One
    cycle later it lands after it, which is what the ROM asks for. A 0..8-cycle
    sweep picked 1 **uniquely**, and the shape is the evidence: 0 fails at
    sub-test 2, and 2-5 overshoot to sub-test 3.
  - **It models a measured hardware effect.** lidnariq's oscilloscope
    measurement of an MMC3B reports "approximately 69ns from the first time PPU
    A12 rises to 2.4V until /IRQ falls to 1.0V", about a third of a pixel, and
    the same thread names the consequence: the delay can push the IRQ into the
    NEXT instruction for certain alignments. A sub-cycle analog delay becomes a
    whole-cycle shift at the CPU's sampling instant, which is what a registered
    output expresses at CPU-cycle resolution.
  - **Two earlier diagnoses are retracted, both refuted by ROMs.** A
    flag-vs-zero reload rule took `2-details` from passing to `Fail($07)`
    against its own sub-test 7; suppressing the pre-render A12 clock fixed
    sub-test 2 and broke sub-test 8, the 241-clock test. The "one scanline"
    framing came from diffing the DUT against the ORACLE instead of against the
    ROM's own boundary -- see the next entry for why that misleads.
  - `mapper4mmc3irq065` improves from **950 to 570** diverging cycles of
    178,676; the co-simulation suite holds at 0 failed.

- **Two PPU defects, found on the first day anything consumed A12.** Neither is
  a cartridge bug and both had been invisible for four releases, because no
  consumer of the signal existed. The MMC3 gate opened at **10,821 of 178,676
  cycles diverging**, and the chain narrowed it in five steps: the IRQ period
  was 568 CPU cycles against the oracle's 909; there were **1,445 filtered A12
  clocks where ~960 were expected**, so the filter and not the counter; a
  low-gap histogram put 480 extras at gaps of 5-6 M2 ticks; a per-dot histogram
  put **all 480 at dot 338, on EVEN scanlines only**; and the address there was
  `$1002` where a nametable address belongs.
  - **`dummy_fetch` named the RECORD dots, not the fetch.** It covered dots 337
    and 339. A fetch is two dots -- 337-338 and 339-340 -- so on the even dots
    the address fell through every window in `chr_addr_raw` onto the `v_addr`
    fallback and the PPU presented `v` itself. `v` bit 12 is `fine_y[0]`, which
    alternates every scanline, which is the exact even/odd pattern measured.
    Split into `dummy_fetch` (the record) and `dummy_fetch_addr` (the address
    window), because widening the one gate broke the fetch record.
  - **The idle dot, same root cause and a second consumer.** With the first fix
    in, the extras MOVED rather than vanished -- still 1,445, now split
    965 + 320 + 160 -- and a per-dot histogram put them all at **dot 0**, the
    only dot of a rendering line that drives no fetch and therefore the only
    other one reaching the same fallback. This file had already recorded that
    root cause for the octal latch, and fixing it there did not fix it here.

  After both: filtered clocks **1,445 -> 965, every one at dot 261** (965 =
  241 x 4 + 1, against the wiki's 241 per frame); IRQ assertions **180 -> 120**
  against the oracle's 120; interrupts taken **218 -> 158** against 158; work
  RAM byte-identical; diverging cycles **10,821 -> 950**. `3-A12_clocking`
  passing is independent confirmation, since it exercises the filter directly.

- **A "declared diagnostic" was a defect in the harness, not in the console --
  carried for seven releases behind the phrase "by design".** `apuconflict039`'s
  bus surface was excluded from comparison from v2.6.2 onward, under a note
  saying it "carries nine divergences by design and is gated on channel levels
  only". Two things were wrong with that. **Six of the nine had already closed
  and nobody could see it** -- the stream was denied in both directions, so an
  entry that improves is as invisible as one that regresses, which is this
  release's whole thesis. And the **three that remained were neither by design
  nor the DUT's**: on a cycle where the CPU is held, `tb/cpu_main.cpp` built the
  `Observable`'s `bus_data` from a stale local (`last_bus_data`) rather than
  from the RTL's own latch. Taking it from
  the latch -- `o_dma_wdata` on an OAM write, `o_bus_din` on a DMA read,
  `o_open_bus` otherwise -- makes the stream **identical on all 357,361
  overlapping cycles and all 88 checkpoints**. The local is now dead and is
  deleted. The stream is bus-gated, checkpoint-gated and channel-gated together,
  and needs no allowance at all. **"By design" is the load-bearing phrase**: it
  reads as a settled property of the thing under test, when it was a property of
  the instrument reading it, and that is what stopped anyone re-checking it.
- **The three `$4015` divergences the plan expected to be a DUT defect were the
  same harness bug.** The evidence pointed at the DUT and the reasoning was
  sound -- AccuracyCoin's own source states that reads of the APU status register
  are internal to the 2A03 and do not drive the external data bus, which is
  exactly what the oracle showed and the DUT did not. Recorded because the
  *direction* is the finding: a measurement disagreeing with a correct rule can
  indict the measurement.

### Added

- **blargg's MMC3 battery becomes a standing gate -- it had none, so the fix
  above had nothing to protect it.** `blargg-mmc3-gate` runs all six ROMs and
  compares each against **its own verdict byte**, deliberately NOT against the
  oracle: this release establishes that the oracle is the mistaken side on MMC3
  IRQ timing, so a DUT-vs-oracle comparison would enshrine that defect.
  `tb/blargg_verdict.py` refuses rather than passes when no status line is
  produced, when the `$DE $B0 $61` signature is absent, or when the ROM was
  still running at the cycle budget -- an absent verdict must never read as a
  pass. **Every expectation is asserted exactly, including the two failures**,
  so a DUT that improves fails the gate and gets looked at instead of drifting.
  Suite **135 -> 141 gates**.

- **The cartridge -- five boards, and the first thing in this core's history to
  look at PPU A12.** `rtl/cart/cart.sv` implements **UxROM (2), CNROM (3), AxROM
  (7), MMC1 (1) and MMC3 (4)** beside the existing NROM, with a runtime `mapper`
  input, 8 KiB PRG and 1 KiB CHR slots, and PRG-RAM gating for both MMC1's bit 4
  and MMC3's `$A001`. Each board has a generated stimulus ROM
  (`tb/roms/mkmapper.py`), an oracle golden, and **two** comparisons -- the
  per-cycle bus and the nine-field rolling checkpoint:

  | board | mapper | bus | checkpoints |
  |---|---|---|---|
  | UxROM | 2 | 178,677 cycles, **0 divergences** | 44 identical |
  | CNROM | 3 | 178,676, **0** | 44 identical |
  | AxROM | 7 | 178,676, **0** | 44 identical |
  | MMC1  | 1 | 178,677, **0** | 44 identical |
  | MMC3  | 4 | 178,677, **0** | 44 identical |

  **Every bank is filled with its own bank number**, and the program -- running
  from the window the board keeps fixed -- switches, reads the switchable window
  and stores what it read, so a bank switch is observable on the bus rather than
  inferred. **Every bank-select write targets an address holding `$FF`**: UxROM,
  CNROM and AxROM all have bus conflicts, which `cart.sv` does not model and on
  which emulators differ, and `value & $FF == value` makes these ROMs about
  banking and nothing else. Mutation: **9 of 9 CAUGHT**, plus one classified
  inert by byte-comparison rather than by argument.

- **`tb/obs_diff9.py` -- the instrument that was missing between the two that
  existed.** `bus_diff.py` compares FOUR fields per cycle; `ckpt_diff.py` hashes
  nine but only every 4096 cycles, and its own failure message ends "re-run that
  window with `--bus-out` on both sides to find the cycle", which until now meant
  doing it by hand. This names the first differing cycle, the field, both values,
  and the SHAPE across the whole stream -- which field, how many cycles, and
  whether it is one event or a persistent divergence. Both open divergences were
  localised with one command each. It refuses a stream pair with no overlapping
  cycles rather than reporting agreement, because an empty intersection is not
  agreement.
- **A scoped allowance replacing the binary deny -- and the PLANNED mechanism was
  refuted by its own mutation pass.** The plan specified `ckpt_diff.py
  --expect-diff`, a list of checkpoint indices known to differ. It was
  implemented, run, and **cannot work**: `checkpoint.h` chains a rolling FNV-1a,
  so one divergent cycle poisons every checkpoint after it. Allowing
  `ppuoamcorrupt052`'s first differing window moved the failure straight to the
  next one, and allowing the rest is the all-or-nothing deny it was meant to
  replace. The refutation is recorded at the site rather than the design quietly
  swapped. `obs_diff9.py --allow-cycle` puts the allowance on the **per-cycle**
  comparison, where there is no such coupling: an attributed difference costs
  **one cycle of coverage instead of seventy-one checkpoints**.
- **`ppuoamcorrupt052` re-gated at 357,360 of 357,361 cycles**, against nothing
  at all before. It differs on exactly one cycle -- 70,627, `bus_data` `$80` on
  the oracle against `$00` on the DUT -- which is ledger 3.19's documented
  OAM-corruption asymmetry, attributed there and unresolved because settling it
  needs a source neither repository has. It is allowed and named, not resolved.
- **The allowance fails BOTH ways, which is what makes it safe to adopt.** An
  allowed cycle that stops differing is a FAILURE, so a DUT that improves cannot
  leave a stale allowance quietly hiding coverage -- the exact shape of the
  defect this release opened by finding. Six mutations, all behaving as intended:
  no allowance fails and names the cycle; the real cycle passes; a matching cycle
  in the allowance fails as stale, alone and alongside the real one; a cycle
  outside the compared window is refused rather than allowed to match nothing;
  and a non-numeric argument is refused.

### Changed

- **A one-scanline MMC3 IRQ residual is ATTRIBUTED, and a fix for it was
  REFUTED by the ROM written to probe it.** On blargg's `mmc3_test_2` the DUT
  scores **4 of 6, level with the oracle and failing the same two ROMs**, one of
  them (`6-MMC3_alt`) correctly, since it is NEC rev B and this core models the
  other revision. The residual is `4-scanline_timing` #2: both consoles write
  the arming `$E001` on the SAME cycle, so the CPU halves are in lockstep, and
  the divergence is entirely which A12 rise clocks the counter -- **exactly one
  scanline**, the DUT firing from a pre-render-line clock and the oracle from
  the next line. Two mechanisms could produce that and **both are now
  eliminated, each by the sub-test written for it**:
  - suppressing the IRQ on a `$C001` **flag-driven** reload (as against a
    zero-driven one) is worth exactly one clock, and took `2-details` from
    passing to `Fail($07)`. Sub-test 7 states the opposite in its own name --
    *"IRQ should be set when non-zero and reloading to 0 after clear"*.
    Reverted from a pre-edit snapshot.
  - an extra pre-render clock is ruled out by sub-test 8 of the same ROM,
    *"Counter should be clocked 241 times in PPU frame"*, in the same
    `PPUCTRL = $08` configuration, which the DUT **passes** -- 241 being 240
    visible lines plus the pre-render line.

  So both rules the DUT implements are confirmed by ROMs written to measure
  them, which puts the residual upstream of the cartridge. Settling it needs the
  oracle's own filtered-A12 stream, which neither repository can export today.
  Recorded with its cycle, dot and scanline rather than tuned until a gate goes
  green. **`1-clocking` stayed green across the refuted change**, which is a
  reminder that a green neighbour is not evidence a change is right.
- **An unregistered gate's stated reason had expired, and is re-measured at the
  release commit.** `mapper4mmc3irq065` is deliberately not registered, and the
  comment saying so quoted 180 IRQ assertions against 158 and a first divergence
  at cycle 60,104 -- **both figures killed the same day by the two PPU fixes
  above**. Re-run: **120/120 assertions, 158/158 interrupts taken, 950 of
  178,676 cycles diverging, first at 62,166**. The counter and filter now agree
  and what remains is when the interrupt lands. An exclusion whose reason has
  gone stale is the defect v2.6.8 spent a release on, one release later and in
  this release's own new code.

- **The checkpoint deny list is down to one entry, and that entry is denied from
  the ROLLING-HASH gate specifically** rather than from comparison altogether --
  a distinction the previous list could not express, and the reason it forfeited
  whole streams.
- Documentation corrected at the sites that carried the retracted claims:
  `docs/rung6-integration.md`'s "nine divergences by design" paragraph and
  `docs/apu-oracle-vs-documentation.md` item 6.3 and its two cross-references.
  Each retraction quotes the wrong text before correcting it, so the record shows
  the claim was made rather than tidily removing it.

### Unchanged, and verified rather than asserted

- **Co-simulation suite: 141 passed, 0 failed, 0 skipped** -- from v2.6.8's 128:
  130 after the deny-list half, 135 with the five cartridge gates, and 141 with
  the six blargg MMC3 verdict gates.
- **AccuracyCoin 141/141 RE-MEASURED, not carried**: the authoritative RAM
  decoder reports `pass rate = 100.00% over 141 assigned tests`. The framebuffer
  decoder's 121 is the known-buggy one and is not the figure to quote.
- **nestest** golden-log comparison passes. **Workspace tests: 131 binaries,
  2,245 passed, 0 failed.**
- No chip crate changed, so the two accuracy figures hold by construction --
  they were re-run anyway, because "by construction" is the kind of claim this
  release exists to distrust.

- The emulation core is untouched -- no chip crate changes -- so **AccuracyCoin
  141/141 (100.00%, RAM decoder)** and nestest 0-diff hold by construction, and
  were re-run anyway.
- **Rung 6 does not close.** No DE10-Nano and no SuperStation One are attached to
  this machine, confirmed by checking the USB bus, serial devices, removable
  block devices and mounts rather than assumed. The four properties downstream of
  every gate -- the palette, the video timing constants, the audio's absolute
  level and its band-limiting -- remain without evidence until a board exists.

## [2.6.8] - 2026-08-31 - "Arrears" (a deny list is an assertion about the thing under test, and nobody re-measured it)

### Fixed

- **The nestest gate compared 265,000 cycles against a 5,062,688-cycle golden,
  and the cap was arrears rather than caution.** It was correct when written:
  v2.6.7 set it just short of caveat C6's first divergence at cycle 265,640, so
  the stopping point was named rather than hidden. C6 was then **closed inside
  the same release** and nothing re-opened the cap. Re-measured: **all 5,062,680
  overlapping cycles match** on `pc`, `bus_addr`, `bus_data` and `bus_access`.
  The window is now **derived from the golden's own size** rather than written
  down -- a literal is exactly how this gate went wrong twice, once naming
  5,002,992 against a 59,562-cycle golden and once capping at 265,000 against a
  fixed one. Coverage 264,992 -> **5,062,680 cycles, a 19x increase**, with no
  constant left to drift.
- **Caveat C4 is CLOSED, and by demonstration rather than by argument.** C4 said
  the checkpoint gate could not catch an `nmi_line` defect because no gated
  golden ever raised an NMI. **The stimulus existed the whole time**: nestest's
  golden sets `nmi_line` on **3,592** cycles and `irq_line_at_low` on **62,516**,
  and its checkpoint golden was already committed at 1,237 checkpoints. It was
  never wired in, because nestest uses `nestest-gate` rather than
  `cpu-bus-gate`, so the auto-attach could not reach it, and it was absent from
  the explicit loop's list. Wiring it took one optional variable and costs no
  extra simulation time -- it rides the same DUT invocation. Reverting
  `o.nmi_line` to the constant `false` it held before v2.6.7 is now **CAUGHT**,
  at checkpoint 28 of 1,237. The shape of that result is the whole argument for
  nine fields: **in the same run the bus gate still passed all 5,062,680
  cycles**, because it compares four and is structurally blind to the defect.
- **The widened gate caught the first thing it was pointed at, which was this
  release's own change.** The nestest window was first derived from the size of
  `obs.bin`, and that is wrong by exactly eight: the obs stream begins at cycle
  8, so its record count is eight short of the window the golden was exported
  with. `ckpt_diff.py` compares the final partial window BY CYCLE and said so --
  "checkpoint 1236 covers a different cycle -- golden through 5062687, DUT
  through 5062679" -- while **the bus gate passed the identical run**, because it
  compares *overlapping* cycles and a short stream simply overlaps less. Two
  gates, one wrong window, and only the length-sensitive one could see it. The
  window now comes from the manifest's `cpu_cycles`, which is what `regress.sh`'s
  own comment already said reproduces a golden.
- **Four of the six checkpoint deny-list entries were passing and nobody had
  looked.** A deny list is an assertion about the thing under test, and v2.6.7
  changed the thing under test twice -- the DUT's `$4017` interrupt-clear split
  and the harness's `Observable` completed at end-of-cycle with `nmi_line` wired
  to the effective line. Re-measured: `irqlat048` (22 checkpoints identical),
  `ppuvbl023` (175, through cycle 714,737), `ppuvbl024` (291, through 1,191,227)
  and `ppuvbl025` (175) all pass. `apuconflict039` (a declared diagnostic) and
  `ppuoamcorrupt052` (a genuine hash divergence) remain, each restated from a
  current measurement rather than an old note.
- **Three of those goldens were not run by the suite AT ALL** -- `ppuvbl023`,
  `ppuvbl024` and `ppuvbl025` have a committed golden, ROM and manifest and
  appeared nowhere in `regress.sh` except the deny list, which only ever
  governed the auto-attach. **So removing them from that list was INERT until
  they were also iterated**, which is the trap worth naming: retiring an
  exclusion adds coverage only if something reaches the thing excluded. Caught
  before the change shipped, by checking what iterated them rather than assuming
  the deny list was the gate.
- **`ppuspr0` was the last golden with a checkpoint stream and no nine-field
  coverage.** Measured (44 checkpoints identical) and wired, which closes the
  set: **59 goldens carrying a checkpoint stream = 40 in the explicit loop + 16
  auto-attached + nestest + 2 denied**, and nothing unreached. The
  co-simulation suite goes to **128 passed, 0 failed, 0 skipped** (from 123),
  carrying **57 nine-field comparisons** (from 51).

## [2.6.7] - 2026-08-30 - "Detent" (the bitstream becomes a published, reproducible artifact, and a published slack figure is withdrawn)

### Added

- **The MiSTer bitstream is published, from this release onward.** Every release
  now ships a `.rbf` -- committed to the sibling's `releases/` and attached as an
  asset to the GitHub release on **both** repositories. This reverses v2.6.6,
  which produced a bitstream and deliberately withheld it because no hardware had
  run it. The MiSTer distribution mechanism reads
  `releases/RustyNES_MiSTer-vX.Y.Z.rbf` out of the *repository*, so an empty
  `releases/` does not describe a cautious core -- it describes an
  undistributable one, withheld from precisely the people who own the boards this
  project does not have. The caution is relocated rather than dropped: the
  release body states that no hardware has run the bitstream, and states what the
  co-simulation ladder cannot reach by construction -- the PPU gate compares the
  *pre-palette* index and the APU gate *per-channel integer levels*, so the
  palette, the video timing constants, the audio's absolute level and its
  band-limiting all sit downstream of every gate.
- **The core presents a real MiSTer front panel, not just a video signal.** The
  `CONF_STR` gains a joystick map (`J1,A,B,Select,Start`) so the OSD's button
  definition and every standard MiSTer controller work without per-user
  remapping, and a version line built from `BUILD_DATE`. It also gains OSD **info
  lines**, wired through `hps_io`'s `info_req`/`info` pair, which exist to answer
  a question the core previously could not: this is an **NROM-only** core, and a
  cartridge on any other board used to render garbage with nothing on screen
  saying why. The loader now reads the iNES header as it streams -- mapper low
  nibble from byte 6, high nibble from byte 7, **with the "DiskDude!" guard**
  (bytes 12-15 must be zero, or byte 7's upper nibble is a corrupted signature
  rather than a mapper number, and a legitimate NROM cartridge reads as "mapper
  64") -- and holds the console in reset with an on-screen message when the board
  is unsupported. Refusing explicitly beats running wrongly.
- **`scripts/release-rbf.sh`**, so publication is repeatable rather than
  remembered. It verifies **against the reports rather than the exit code**,
  because Quartus has been observed to abort after the resource summary and still
  exit 0; it refuses a compile with errors, a missing Fitter footer, negative
  slack at any corner, or a message citing this project's own RTL.
- **The checkpoint gate is registered** (`make ckpt-gate`, ten entries in
  `regress.sh`). It compares a rolling hash of all **nine** observable fields
  where every other gate compares four, so `put_cycle`, `nmi_line` and the two
  `irq_line_*` samples are checked by nothing else -- which is how all four came
  to sit written as constant `false` in every trace the harness had produced. The
  co-simulation suite goes to **123 passed, 0 failed with ZERO skips** (from 87),
  carrying **51 checkpoint comparisons** where v2.6.6 had none.

### Fixed

- **The work RAM was 28% of the device, and the comment saying it could be
  afforded was measuring the wrong thing.** `rtl/wram.sv` held 2 KiB as
  asynchronous-read registers, arguing "2 KiB is 16,384 registers, about a tenth
  of a 5CSEBA6U23I7, which the device can afford". True of the *fit succeeding*;
  utilisation is a budget the whole design spends, and measured on this release's
  build the module was **8,503 ALMs of a 30,265-ALM design**. Converting it to an
  M10K block takes the core to **21,865 ALMs (73% -> 52%)** and worst setup slack
  from **-0.639 ns (failing) to +0.130 ns**, with the console's own Fmax going
  30.26 -> **36.19 MHz**. The module also carried a recorded revert -- "registering
  it was TRIED and reverted ... all 87 gates failed" -- whose diagnosis was about
  the **instrument**: the harness's device cross-checks ran at the top of the CPU
  cycle then, where an asynchronous read holds this cycle's byte and a registered
  one still holds the previous cycle's. v2.6.6 moved them *for exactly this
  reason, when the cartridge hit it*, and never carried the fix back. Of the
  0.769 ns recovered, 0.595 came from the memory and 0.174 from the fitter seed
  (2 -> 4); a re-seed on a design with no headroom is a lottery ticket, and three
  seeds across two releases is what riding the edge looks like.
- **The oracle polled an interrupt on a cycle nesdev says it must not, and it
  had been baked into a committed golden.** `CPU_interrupts` states that
  interrupts are polled before an instruction's second cycle "but **not** before
  the third CPU cycle on a taken branch"; `Cpu::handle_interrupts` re-armed the
  NMI edge detector there anyway. Suppressing it needs `skip_irq_sample`'s value
  on the *previous* cycle as well as this one, so the CPU gains
  `skip_irq_sample_q` -- which the snapshot audit immediately, and correctly,
  refused to let through as an allowlist entry, so **`CPU_SNAPSHOT_VERSION` goes
  3 -> 4** (one appended byte; strict-equality dispatch, so no migration path).
  nestest now matches the DUT on all **5,062,680** cycles across all nine fields.
  The old `nestest` golden encodes the defect visibly -- at cycle 265,640 it
  pushes a return address mid-branch where the corrected oracle simply executes
  on -- so it is regenerated. **Every golden was re-exported and compared, not
  argued about**: exactly **one of 98** resolvable goldens moved, and it is the
  ROM the defect was found on. AccuracyCoin **141/141 (RAM decoder)** and the
  135-binary battery are re-run after the change, so they are verified rather
  than asserted.
- **A gate's own output asserted a precondition that had expired two releases
  earlier.** `tb/bus_diff.py` printed, on every pass, that four of the nine
  observable fields were skipped because "the PPU cannot raise NMI until v2.5.8".
  v2.5.8 shipped. They were constant because the testbench never assigned them --
  a testbench omission wearing the costume of a hardware limit, which is how it
  survived. The message now states what is true: all four are populated, all four
  are compared by `ckpt_diff.py`, and this gate stays on the bus deliberately so
  that a failure in it has one meaning.
- **The release gate was reading the wrong timing corner.** It inspected only
  "Slow 1100mV 100C", the corner this project had been quoting -- and the binding
  corner on this design is **Slow -40C** (+0.108 ns setup against +0.385 at
  100C), with the worst hold at **Fast -40C** (+0.042 ns). A bitstream failing at
  a corner the gate did not read would have passed while it reported three times
  the real margin. Corners are now discovered from the report by pattern rather
  than from a fixed list, and a report yielding fewer than two is itself a
  failure.
- **The co-simulation harness sampled `irq_line_at_high` two-thirds through the
  cycle.** It built the `Observable` after eight of a CPU cycle's twelve master
  clocks, while the oracle's `_at_high` is an end-of-cycle read taken "after the
  access + DMC tick". `IRQ_PROBE_CYC` showed the frame-counter IRQ asserting on the
  **final edge** of cycle 29,827 -- invisible to a read taken eight clocks in,
  visible to one taken at the end. Both instruments were reporting exactly what
  they sampled. Checkpoint comparisons passing went **3 to 11** and failures at
  checkpoint 7 went **30 to 3**; the three that passed before were runs too short
  to reach cycle 29,827, which is what made "3 of 33" look like a scatter rather
  than one cause.

### Changed

- **Save states written before v2.6.7 no longer load** (`CPU_SNAPSHOT_VERSION`
  3 -> 4). This is ADR 0028's stated policy applied, not a lapse: the CPU
  section carries a strict-equality version check, so a stale blob is refused
  with a clear error rather than silently misinterpreted as the current layout.
  The cause is the taken-branch interrupt fix below, whose new field is genuine
  emulation state read back on the next tick; a restore that dropped it would
  resume with the NMI edge detector re-armed a cycle early on any snapshot
  landing inside a taken branch. The same shape as v2.2.3's
  `PPU_SNAPSHOT_VERSION` 8 -- a schema *gap* closed in a patch release, because
  the alternative is a save state that reloads into a subtly different console.
- **The fitter seed moves 3 -> 4**, and the reason is the work-RAM finding
  rather than the seed. See *Fixed*.
- **v2.6.6's published slack figures are withdrawn.** It stated worst setup
  +0.363 ns and worst hold +0.245 ns; two compiles of the byte-identical
  committed configuration -- one from scratch, one incremental -- produce a
  byte-identical bitstream and neither reproduces them. The corner hypothesis was
  checked first and refuted: no corner produces either value. The correct figures
  are **+0.108 ns setup and +0.042 ns hold** at the binding corner, with
  `End Point TNS` 0.000 on all 56 rows -- those being **v2.6.6's configuration,
  re-measured**, not this release's; v2.6.7 carries the Workstream B RTL fix and
  publishes **+0.086 / +0.096 at seed 3**. The build itself **is** reproducible,
  and that is now measured rather than argued -- an identical `.rbf` from a clean
  tree and from an incremental one, for both configurations, so the pinned seed
  and `NUM_PARALLEL_PROCESSORS 4` do their job. The CHANGELOG's v2.6.6 entry and its
  release notes are deliberately **not** rewritten, and the withdrawn pair stays
  visible inside the correction, because deleting it would erase the record that
  it was claimed.
- **Caveat C2 CLOSES: the `$4017` inhibit clears the interrupt at write+3**, the
  documented cycle. A `$4017` write schedules four effects and this core landed
  all four at the sequencer's maturation -- zeroing the frame counter, committing
  the mode, applying the inhibit, and clearing the interrupt. Moving all four one
  cycle later dropped blargg's 2005 APU battery from **11/11 to 4 of 11**,
  including `04.clock_jitter`, the ROM written to probe that timing. Read as
  evidence rather than as a dead end, that says the *sequencer's* maturation is
  where the ROMs want it -- so separating **only** the interrupt clear, one cycle
  later, lands it at write+3 (nesdev's figure for an APU-aligned write) while the
  frame counter's zeroing stays put. Two effects of one write, on two different
  cycles. Measured on `01-basics`: the CPU writes `$C0` at cycle 116,842 and the
  line now deasserts on the final edge of **116,845**, which is what the oracle's
  own record shows.
- **Checkpoint streams go from 11 identical to 52**, of 58 -- all sixteen
  `instr_test-v5` ROMs, all eleven blargg APU ROMs, and the three checkpoint-7
  stragglers. Both controls held: blargg stays **11 of 11**, and the per-cycle bus
  gate still matches on **all 2,680,239** overlapping cycles of `01-basics`, so
  the CPU-visible behaviour is unchanged. This is not the v2.5.7 compensation
  trap -- the interrupt *set* path was already correct (655 of 655 checkpoints
  pass, and those windows contain many set events), so only one edge of one
  signal moved.
- **The fitter seed is raised 2 → 3, and named.** The fix adds one register to the
  frame counter, which moved placement enough to land the framework's HDMI path
  at **−0.007 ns** at Slow 100C -- seven thousandths of a nanosecond, inside
  `sys/`, on logic the change does not touch. The `.qsf` has anticipated exactly
  this since v2.6.6 and prescribes raising the seed and disclosing it rather than
  re-running until it passes silently. At seed 3 every corner closes: worst setup
  **+0.086 ns**, worst hold **+0.096 ns**.

### Known issues

- **The registered checkpoint gate cannot see `nmi_line`.** Pinning the flag to
  `false` -- the original defect, restored -- comes back NOT CAUGHT, and counting
  the flag across the gated goldens shows why: **not one of them ever raises an
  NMI**. The only five stimuli in the corpus that do are `irqlat048`,
  `nmi-overlap-brk` and the three `ppuvbl` ROMs, and of the four with a
  checkpoint stream all four still differ. The gate is structurally incapable of
  catching that defect, so the mutation indicts the stimulus. Stated rather than
  implied, so a future NOT CAUGHT there is expected rather than rediscovered.
- **Six checkpoint streams still differ**, named in a deny list with their causes
  in `docs/rung6-integration.md`. `apuconflict039` is a declared diagnostic (its
  bus surface carries nine known divergences by design). The other five are open:
  `irqlat048`, `ppuoamcorrupt052`, and the three `ppuvbl` ROMs at the *same*
  checkpoint 13 -- one cause rather than three, and the only NMI-bearing stimuli
  available, so closing that cluster would also close the gate's `nmi_line` blind
  spot. They had never been measured, because the manifests
  record the path the *oracle* exported with, which for a harness-built ROM is
  relative to the oracle's working directory and unreadable here; the ad-hoc scan
  skipped them into a tally line reading "26 skipped (no manifest or ROM)".
  `regress.sh` now falls back to this repository's own copy.
- **nestest diverges from cycle 265,640, and the golden was hiding it.**
  `docs/rung3-ppu.md` records nestest as closed at **5,002,992 cycles** at v2.5.8
  and the gate's own default named that figure -- while the committed golden was
  `--frames 2`, **59,562 cycles**, two orders of magnitude shallower. The gate
  also *skipped on every run*, because its ROM path had no default while the
  battery beside it does. Re-exported at 5,062,688 cycles the DUT diverges on
  **19,224 of 5,002,984** compared cycles. **It is not this release's fix**: the
  pre-fix RTL, run in a separate worktree against the same golden, gives the
  identical 19,224. The gate now runs to **265,000** -- just short of the first
  known divergence, named rather than hidden, and 4.4x the coverage it had.
  Raising it past that is the work of closing this caveat.
- **Rung 6 does not close.** No DE10-Nano and no SuperStation One are attached to
  this machine -- confirmed by checking, not assumed. Hardware bring-up moves to
  the first release after a board exists.

## [2.6.6] - 2026-08-29 - "Chassis" (the console becomes a MiSTer core, with 0 errors, timing closed, and 111 warnings taken to three)

A chassis is the frame everything else bolts to. It is not the engine, and this
release does not touch the engine: `nes_top` computes exactly what it computed
at v2.6.5. The co-simulation suite is therefore an **acceptance criterion**
here rather than a formality -- integration work that quietly changed the core
would otherwise show up as a green bitstream and nothing else.

It earned that place immediately. The cartridge's memories had to be rewritten,
and only the suite could adjudicate the rewrite.

### The gate

| # | criterion | result |
|---|---|---|
| 1 | `sys/` is verbatim | **57 files, 0 content differences, 0 files on one side only**, against `Template_MiSTer@3ea1134c` |
| 2 | the project compiles | `quartus_sh --flow compile` exits 0 |
| 3 | a bitstream exists | `output_files/RustyNES.rbf` |
| 4 | **timing closes** | worst setup **+0.363 ns**, worst hold **+0.245 ns**, **TNS 0.000 on every clock**; the console's own clock has **+13.514 ns** and an Fmax of **30.26 MHz** against 21.477272 required |
| 5 | subset holds, warnings named | **0 errors**, and warnings **111 -> 3**: `13050`/`13051` (the framework's HDMI I2C open-drain buffer) and one unnumbered message about `sys/pll_audio`'s PLL reset. Every suppressed class is listed in the `.qsf` with its cause, and `tb/quartus_clean.py` fails if any message cites `rtl/` or `tb/` |
| 6 | **the co-simulation does not regress** | **87 passed, 0 failed** |

The fitter **seed is pinned** (`SEED 2`). One path in the framework's HDMI
domain sits within a tenth of a nanosecond of its requirement, and two compiles
of the *same* RTL -- differing only in how three unrelated SD pins were driven
-- landed it at **+0.363 ns** and **-0.086 ns**, the second reported as
`Critical Warning (332148): Timing requirements not met`. A build that closes
has to be reproducible, so the seed is recorded rather than left to luck.

### The memory rewrite, and why clause 6 exists

`cart_nrom.sv` read its arrays asynchronously, under a comment claiming that
inferred block RAM "from the source style alone". **An M10K read is
registered** -- there is no asynchronous-read block memory on this device -- so
40 KiB of cartridge stayed in registers: 393,216 of roughly 166,000 available,
and Quartus refused it outright.

The project had already written the rule down. The README has said since v2.4.3
that the core would use "synchronous read with a registered address, **no
asynchronous read anywhere**". `cart_nrom.sv` was written two releases later
doing the opposite. A rule in a README is not a control; and nothing before this
rung had ever asked Quartus about the cartridge, because v2.4.3 fitted a 2 KiB
probe module and 2 KiB fits either way.

Registering the reads is a one-master-clock latency, and whether that is visible
was **probed rather than argued**: `bus_addr` is stable for all twelve clocks of
a CPU cycle and the access lands at the seventh. The console never sees it. The
harness did, and its device cross-checks moved to where the CPU actually
samples.

Work RAM is measured separately and left asynchronous ON PURPOSE -- 2 KiB is
16,384 registers, about a tenth of the device, which it can afford. So the rule
as this release applies it is narrower than the sentence quoted above: it binds
arrays too large to live in fabric, which is the cartridge, and work RAM is a
deliberate, measured exception rather than an inconsistency. Registering it too
was tried and reverted, because the harness samples `o_wram_dout` at the access
itself and all 87 gates failed.

### Two defects that only "will this work on hardware?" would have found

**The audio would have been a DC rail.** `apu_mixer` produces what the 2A03's
resistor ladder produces: unipolar, silence at zero. Handed to the framework as
unsigned, silence maps to **-32768** -- a full-scale offset into the DAC and
into HDMI audio, not a quiet channel. Real hardware AC-couples; `audio_dc_block`
is that capacitor, a one-pole high-pass whose corner is the console's own
documented ~90 Hz.

**Two OSD options did nothing.** The menu offered "CRT 25%" and "CRT 50%" while
`VGA_SL` was tied to zero, and two aspect-ratio entries served by a module this
core does not instantiate. Both fixed by making the menu describe what exists.

### The finding worth keeping

`sys/sys_top.sdc` puts the core's clock in its own clock group by matching a
**hierarchical name pattern** -- `*|pll|pll_inst|altera_pll_i|*`. Name the PLL
anything else and the clock matches no group, every crossing to the framework's
audio, HDMI and HPS domains is analysed as synchronous, and the report shows
**-13.901 ns** of slack and **-422,601 ns** of TNS on a design whose Fmax was
already 25.75 MHz for a 21.477 MHz requirement.

Nothing warned. The compile succeeded and the Assembler reported 0 errors and 0
warnings. **A convention enforced by a glob is a convention with no error
message** -- and reading Fmax and slack together is what separates a missing
constraint from a slow design.

Full account, including the compiler crash on the 256-entry decode table and the
three fixes for it that were measured and failed: `docs/rung6-integration.md`
in the sibling repository.

### Not in this release

- **Hardware.** No DE10-Nano and no SuperStation One are attached to this
  machine, so a booting core, a synced display, audible sound and a working pad
  are **not** claimed. That is rung 6's close and it moves to v2.6.7.
- **Band-limited audio.** The DUT has a lookup-table mixer and a DC blocker; the
  emulator has BLEP. The core will alias where the emulator does not.
- **The v-copy delay depth (rung-5 caveat C1) is deferred.** Discriminating
  inside the passing window of 1-4 dots needs a stimulus sensitive to the exact
  dot, which is its own ROM and its own sweep.
- **The per-cycle AccuracyCoin gate (C2) is half-built, and the half that
  exists already found something.** A full per-cycle capture over the 4500-frame
  window is 2.0 GB per side, which is why the rung-5 gate compares verdicts and
  is blind to compensating errors. The harness now emits rolling FNV-1a
  checkpoints (`--ckpt-out`, `--ckpt-interval`, `--ckpt-from`) and
  `tb/ckpt_diff.py` compares them -- the same per-cycle information at 1/8192th
  the size.

  On its first run it failed, and correctly: **four of the nine observable
  fields had been written as constant `false` into every `.obs.bin` the DUT has
  ever produced**, because `bus_diff.py` compares four of them and nothing else
  looked. The note at the site explaining the omission had a precondition --
  "the PPU cannot yet raise one" -- that stopped being true at v2.5.8 and was
  never re-read. Three are now correct (`put_cycle` after a polarity fix,
  `nmi_line` exactly, and the IRQ pair once sourced from the effective line
  rather than the external injection pin, which is why it first diverged at
  cycle 29,827 -- a frame-counter position).

  The residual is **one CPU cycle on one signal**, twice in two hundred
  thousand. It was first written up as "the oracle samples /IRQ twice per cycle
  and the harness once"; that is the wrong mechanism and is corrected here.
  `bus.rs` says what the pair is -- `_at_low` snapshotted at cycle-start before
  the APU advances, `_at_high` read at end-of-cycle, so the two encode the
  ORDERING of a change within the cycle. At the transition the oracle's frame
  IRQ becomes visible DURING cycle 29,827 and this core's not until 29,828.

  A rung-4 question of the same family as `PPU_LEAD`, and **which side is at
  fault is not yet known**. blargg's 2005 APU battery passes **11/11** on this
  DUT, `apuirq036` included, which bounds how wrong it can be without settling
  it: those ROMs measure when the CPU *takes* the interrupt, and interrupt
  latency can absorb a one-cycle difference in when the LINE asserts. So the two
  live candidates are a trace observation point and a genuine one-cycle
  assertion difference -- and that is exactly why it is not being rushed into an
  integration release. The gate is NOT registered in `regress.sh` until it
  closes, because a gate known to be red for a reason nobody is acting on decays
  into noise.

## [2.6.5] - 2026-08-29 - "Muster" (rung 5 closes: the AccuracyCoin status vector is identical entry for entry, 146 of 146 executed)

A muster is a roll call where every name is called **and answered**. That is this
release's acceptance exactly, in two clauses: the vector agrees entry for entry,
*and* no entry is unrun on both sides. The second clause is v2.6.4's addition —
without it, an identical vector over entries that never executed is a pass, and
was one.

### The gate

```text
coverage: 146 of 146 entries executed on both sides (0 on neither, 0 on one side only)
status vectors are IDENTICAL entry for entry across all 146 entries.
```

Measured over the 4500-frame golden, 134,012,761 cycles. At the version's start
the same gate read **5 of 146 executed** and 22 differing.

### Five defects, and the shape they share

Four of the five were invisible to every gate that existed when the version
opened, and the recurring shape is **a gate agreeing about a question it was
never asked**.

**The background shift registers' reload and their shift clock need separate
gates.** With one shared gate `BG Serial In` was not merely failing, it was
*arithmetically unreachable*: reload dots are absolute, so on a render re-enable
the next reload is at most seven dots away, and the reload discards the low seven
bits — a serial-in one can never reach bit 7, on any alignment, for any stimulus.
Modelling both structures reproduces **both** measured shifter values, the
oracle's `F807` falling out of the split model without being fitted to it.

**That fix alone left the gate red.** The sprite X counters are **not** gated on
rendering, and AccuracyCoin's `Stale Sprite Shift Registers` test 2 states it
outright — "Rendering was disabled for 18 ppu cycles, but the sprite counters
were NOT halted during that time". This core froze them, so a disable/enable pair
pushed every sprite right by the width of the window. **The ROM that states the
rule passes either way**: it expects no hit at X=254, and a sprite shoved 18 dots
further right is also off the end of the line.

**The PPUADDR second-write `v <- t` copy is delayed**, and the wiki says so inside
the write sequence itself — "wait 1 to 1.5 dots after the write completes". This
core committed it in the write's own edge. Swept 1 to 4 dots (all close
`Hybrid Addresses`) against a control at 8 and 12 (both fail, which is what
proves the parameter reached the compiler); the documented minimum ships.

**The pre-render line clears secondary OAM.** The whole evaluation block —
*including* the clear — was gated on `scanline < 240`, so the pre-render line kept
what scanline 239 had left and the next frame's scanline 0 drew it. **No sprite
can ever render on scanline 0**, because OAM Y is stored one less than the display
row. A sprite-0 probe over the full battery named it in one run: 24 hits in
134 M cycles, four of them at scanline 0, one per frame.

The fifth, the octal latch holding across the read dot, is verified by exactly one
gate and was unverifiable until the fourth landed — the two compose the hybrid
address together and neither produces it alone.

### A diagnosis retracted

The residual was read as a **two-dot CPU/PPU alignment error**, from comparing
per-dot record spans across two instruments. Three configurations refute it: at
the committed alignment the two consoles execute identical `pc`, `bus_addr` and
`bus_access` for **1,695,131 cycles**, while a two-dot power-on shift moves the
first divergence back to 593,228 and takes the differing share from 5.13% to
66.80%. The "two dots" was two instruments stamping their records at different
points in the cycle — the v2.5.7 lesson, third occurrence.

### Also

`rustynes-cosim`'s `state_trace_records_carry_their_cpu_cycle` was gated on a
feature no CI step enabled, so it **ran nowhere** — a regression test the gate
could not reach, which is the shape the surrounding CI steps exist to prevent. It
now runs, and the test that is genuinely inapplicable under that feature is gated
out with its reason rather than left failing.

**The oracle changes on the default path** (a `Controller::write_strobe` owed-shift
fix), so **AccuracyCoin 141/141 (RAM decoder)** and nestest 0-diff are **verified,
not asserted**.

## [2.6.4] - 2026-08-26 - "Rubric" (the last nine AccuracyCoin disagreements close, and the gate that certified them covered 88 of 146)

A rubric is the authoritative statement of the rules, written in the margin by
the person who set the test. That is literally where all three of this release's
fixes came from.

### OAM DMA, and the SH group — the two thirds of this release that came first

`$4014` was a register the console decoded and then did nothing with: **the DUT
had never spent the 513 cycles an OAM DMA costs.** It lands here as a real bus
master — halt on a read cycle, an optional alignment cycle, then 256 read/write
pairs — implemented from `nesdev_wiki/DMA.xhtml`, with the documented **DMC-get
precedence over OAM-get** (a DMC fetch delays the OAM transfer, and costs it its
alignment as well as its slot). A DMA write now drives the open-bus latch too,
which until an OAM DMA existed there was no bus master here to do.

Its halt and alignment were **fitted to the oracle first and then corrected from
the wiki** — the correction is in the ledger rather than quietly squashed,
because "measured rather than assumed" was written about a fit.

The **`SH` group** closes in two steps, and the second was named by the residual
of the first: the stored value's AND with the address high byte is
**RDY-conditional**, and the dummy-read cycle is **addressing-mode dependent**
(`SHA (d),Y` is six cycles, so its `tcyc==3` is a pointer-high fetch, not a dummy
read). Four absolute forms closed on the first fix and `$93` alone did not, which
is what pointed at the second.

Together those took the vector from **9 differing to 3**. The three below are the
tail.

### The three entries, and where their rules actually live

v2.6.3 left nine entries differing. This release closes all nine; the three
below are the last of them. **Every one is category 1 — the implementation followed
documentation that is true and insufficient.**

- **`Open Bus`.** A read of `$4015` does not drive the data bus, and its D5 is
  open bus. Both rules are properties of the board, so both live in
  `cpu_bus.sv`. The nesdev `Open_bus_behavior` page documents the
  `$4016`/`$4017` case and says nothing about `$4015` being exempt; AccuracyCoin
  states the mechanism outright — "all the values read here are internal to the
  2A03 chip, so the data bus isn't used" — and its stimulus is `LDX #16 / LDA
  40FF,X`, the exact instruction the trace divergence had been localised to
  independently.
- **`Interrupt flag latency`.** Branches poll before cycles 2 and 4 and **never**
  before 3, so a taken branch that does not cross a page has no poll at its last
  cycle. Not in the wiki at all.
- **`NMI Overlap BRK`.** Two rules: an interrupt sequence does not poll (stated
  plainly in the wiki and simply missed here), and the BRK hijack window was one
  CPU cycle too narrow at its late edge.

### What the codes meant, and the regression that read as progress

AccuracyCoin's `TEST_Fail` reports `(ErrorCode << 2) | 2`, and the runner sets
`ErrorCode` to **1** before every test routine — so `Fail(N)` names test **N** of
that routine, one-based. Read as a zero-based index it is off by one, and that
error made a change from test 7 to test 5 read as *partial progress* when it was
a **regression** that also broke a standing gate. The description reached a code
comment before the macro was read. v2.6.3's reading that six entries "sharing one
failure code" implied one shared cause is retracted with it: the code is an index
within one routine, so two entries sharing it share nothing.

### A fix that closes one gate and opens another is a scope measurement

The first poll fix moved the poll for **every** instruction from the last cycle to
the second-to-last, on the strength of the same ROM's walk-through of `CLI`. It
closed the failing entry and regressed `apupulse026` and `blargg08`. All three
compare against the same oracle, so one model satisfies all three and a change
that cannot is not that model. Narrowed to the branch exception alone, the entry
still passes and nothing regresses.

Similarly, the hijack's comment argued at length for a delayed pending flag and
named this very AccuracyCoin entry as the case that would catch the alternative.
The entry was then run, and it says the opposite. The wrong comment is kept
beside the fix.

### Per-entry stimulus: the battery was the wrong iteration loop

`scripts/accuracycoin-build/build_sub_test_rom.py` builds a ROM that boots
straight into one catalog entry. Twenty-six were vendored in Session 23 and
nothing had been built since; two more land here — `open-bus.nes` and
`nmi-overlap-brk.nes` — and all three used by this release are **standing verdict
gates**. They reach their verdict from a cold boot with no input in 0.9M to 4.5M
cycles against the battery's 17.9M.

They are verdict gates rather than bus gates by measurement, not preference:
their per-cycle surfaces are dominated by the PPU I/O-latch difference this
project has left open, 2,331,867 of 4,467,082 cycles on one of them. A gate
cannot distinguish an open ledger item from a defect; the ROM's own verdict byte
can. `tb/subtest_verdict.py` refuses when the oracle side is not itself a pass.

Also measured: **`sub-tests/cpu-open-bus.nes` does not run `Open Bus`** — its
verdict lands at `$0407`, which the catalog assigns to *Dummy write cycles*. Off
by one table row, and nothing had caught it because a fixture's name is not
evidence about its content.

### The gate met its own wording, and the wording was not enough

With the four closed, the status vector reported **identical entry for entry
across all 146 entries** — and **58 of those entries were `NotRun` on both
sides**. The acceptance was worded so a DUT could not pass a test by skipping
it; it does not cover the case where *neither* side runs it, which is what a
short window produces.

By suite, the 600-frame golden reaches the CPU catalog and stops partway through
`CPU Interrupts`. Every APU, PPU, sprite-evaluation and PPU-misc entry was
`NotRun` — the entries rungs 3 and 4 exist for. **Measured: 4500 frames executes
all 146** (134,012,761 cycles), and that is the golden now.

`accuracycoin_status` enforces it rather than merely documenting it: it prints a
**coverage** line on every comparison and **refuses** with a non-zero exit when
any entry is `NotRun` on both sides. The predicate and the refusal are both
extracted from `main` and tested directly — a check that exists only inside
`main` is a check no test can reach, which is how this property came to be
missing.

### Widening the window found a defect on its first run

Moving the golden to 4500 frames was coverage work. The first run of the wider
window **aborted at cycle 20,636,325** — 2.8 million cycles past where every
previous run in this programme had stopped:

```text
DMA data disagrees at cycle 20636325 addr=$FFC0: rtl=$04 harness=$00
```

A halted CPU mid-`LDA $2007` held `ppu_sel` high through a DMC steal, so on the
DMC's get cycle — where the bus address is the sample address — the mux preferred
the PPU and handed the sample fetch the read buffer instead of the cartridge
byte. **The comment directly above the offending line stated the intent it
violated**: the write half of the exception was there (an OAM DMA targets
`$2004` and the PPU must see it), the read half was not. `ppu_sel` now follows
whichever master owns the bus.

It was localised with `DMA_PROBE_CYC`, a new env-gated per-cycle dump of both bus
masters — necessary because the harness's cross-checks abort the run and the
abort path does not flush `--bus-out`, so a divergence they catch cannot be read
back from the trace.

### Rung 5 does NOT close, and the reason is a DUT defect

The full-catalog comparison is unavailable, and the first explanation for that
was wrong. Measured on **both sides over the same window**, counting entries that have
written a result:

| run length | oracle | DUT |
|---|---|---|
| 17.9M | 88 | **88** |
| 20.8M | 95 | — |
| 41.7M | 117 | — |
| 60.0M | 120 | **5** |
| 100M | — | **5** |
| 134M | **146** | **5** |

The oracle climbs monotonically to the full catalog. The DUT falls to five and
stays there for seventy-four million cycles. A battery that had merely completed
a pass and restarted would show the count climbing again.

The five are the whole **Power On State** suite and nothing else — the group a
pass writes early. So the DUT restarts somewhere after 17.9M, completes Power On
State, and then **produces no further result for 74M cycles**.

**The obvious reading was a hang inside `PPU Behavior`** — it follows
`Power On State` in catalog order and had never once executed. Probing the PC
refutes it: the DUT sits in a three-cycle self-loop at `$80DF`, which the ROM
spells `INC $EC` / `JMP $80DF` — AccuracyCoin's **menu idle loop**, where the
menu spins while its NMI handler works. The DUT is back at the menu with only
the results the power-on path writes, so the console **reset**; it then idles
because START is pressed once, at frames 300-306.

A reset is a different defect to chase than a hang, and v2.6.5 chases the right
one: bracket where the result count collapses between 17.9M and 60M.

Reported as unavailable rather than as a result. Taken at face value the
comparison reads `141 of 146 entries differ`, which would be a badly wrong thing
to publish — it is one defect, not 141. The comparator does classify them as
disagreements rather than as missing coverage, which is correct: they are
`NotRun` on one side only, the exact distinction the acceptance wording exists
for.

## [2.6.3] - 2026-08-25 - "Mainspring" (the DUT runs on one master clock, and four enables that were never enabling)

### Added

- **AccuracyCoin's status vector, decoded and comparable entry for entry.**
  Rung 5's stated acceptance is a status vector comparable **entry for
  entry** — including `Skipped` and `NotRun` — between the oracle and the
  co-simulation DUT. `accuracycoin_status` is the oracle half: it reads a
  work-RAM dump, decodes it against the 146-entry catalog, and reports **by
  test rather than by address** — printing the entries that are not a clean
  `Pass` given one dump, and only the entries that disagree given two. The
  full vector is compared either way; the filtering is on the output.

  Producing one is this release's deliverable; making the two agree is
  v2.6.4. The first end-to-end DUT run reports **137 of 146 entries
  agreeing and 9 differing**, six of those sharing `Fail(code 7)` — five
  SH-group stores and Open Bus — a pattern that suggests one shared
  address-bus cause rather than six independent defects. A pass count of
  137 would have hidden that pattern; naming the entries is what the
  entry-for-entry form buys.

  **Byte-comparing 2 KiB of work RAM answers a different question, and
  answers it wrongly in both directions**: it reports scratch bytes as
  failures, and it reports two runs that never started the battery as a
  pass, because two idle title screens have identical RAM. So the tool
  refuses an all-`NotRun` vector with a non-zero exit. That case — two
  vectors agreeing on 146 entries of nothing — is exactly the shape of the
  vacuous status-address assertion v2.6.2 found in the NTSC blargg suite,
  which reported 11/11 for five minor releases while asserting nothing.

- **The golden manifest records the controller press, and its absence.**
  The frames-mode manifest gains `press_start`, written as `A:B` when a
  window was given and the literal `none` when it was not. A controller
  press changes what the ROM *executes* — an AccuracyCoin export without
  one captures an idle title screen and with one captures 88 test results
  — so a manifest omitting it describes two completely different runs
  identically. Found by needing it: the shipped `AccuracyCoin` golden
  plainly contains a pressed run, and its own manifest could not say which
  window produced it.

### Changed

- **Gradle 10 deprecations cleared in the Android build.** Four call sites, each
  verified against the artifact rather than against the warning text:

  - `AndroidSourceDirectorySet.srcDir(Any)` → the `directories` mutable set. The
    interface was read out of the **pinned AGP 9.3.2** `gradle-api` jar, not a
    cached older one: `getDirectories()` returns `Set<String>`, so it takes
    paths and the call sites pass `.path` rather than a `File`.
  - Two Kotlin DSL **delegated properties** (`by tasks.registering(Exec::class)`)
    → `tasks.register<Exec>("name")`. Gradle's own upgrade guide is explicit:
    all Kotlin DSL property delegates — `registering`, `creating`, `existing`,
    `getting` — are deprecated and scheduled for removal in Gradle 10.
  - `currentWindowAdaptiveInfo()` → `currentWindowAdaptiveInfoV2()`, confirmed
    present in `material3.adaptive` 1.3.0. Behaviour here is unchanged: both
    breakpoints tested are "at least", so a window that now reports the new L or
    XL width class still satisfies EXPANDED.

  The build also now runs with **`--warning-mode all`**, because the summary
  line ("Deprecated Gradle features were used in this build, making it
  incompatible with Gradle 10") names nothing — and a warning that cannot be
  attributed is one nobody acts on, with a deadline attached.

- **MiSTer co-simulation, rung 5 (sibling repository).** The NROM cartridge, the
  work RAM, the console's CPU bus, the controller ports and DMC DMA are landed
  and gated at **50 gates green**. The DUT's CPU is driven by the RTL bus and a
  DMC fetch is a real bus cycle by a second requester — no flat testbench array
  in either path. Assembling `nes_top` is what remains, and because that top
  level must divide the master clock — the apparatus rung 3's phase calibration
  was built on — it is a change to the timing substrate rather than a rewiring,
  and is named as its own step.

  **The ladder caught the oracle for the first time**, which is what the
  accuracy-ledger entry below records. No `rustynes-*` crate changes, so
  AccuracyCoin and nestest are untouched by construction.

- **The MiSTer DUT runs on one master clock (sibling repository).** `nes_top`
  took its clock enables as inputs and the testbench generated the dot phase; it
  now takes a single 21.477272 MHz master clock and derives `ce`, `ppu_ce` and
  `ppu_access` itself — the shape Quartus compiles.

  It is built in **RustyNES's own v2.0.0 "Timebase" shape**: two independent
  accumulators in master-clock units, never reset to one another. That is not a
  stylistic choice — a modulo-`CPU_DIV` phase counter looks equivalent on NTSC
  and cannot express PAL at all, where 16 master clocks per CPU cycle and 5 per
  dot is 3.2 dots per cycle. `ACCESS_MC` and the PPU phase offset are derived
  from the oracle's `read_split`/`write_split`, not swept. Five testbench phase
  knobs are retired: they existed to find this phase, and the answer is now
  compiled into the core.

  **It found four enables that were never enabling.** The old testbench tied `ce`
  high and pulsed the clock once per CPU cycle, so the clock did the gating the
  enable was supposed to do and any ungated `always_ff` was correct only by
  accident — under a real master clock each fires twelve times. Two were already
  known (the PPU register block at v2.5.7, the open-bus decay reload); two were
  not: the **DMC's DMA acknowledge**, where the sample pointer advanced by TWELVE
  per byte and 324,182 of 357,360 cycles diverged, and the **frame-counter IRQ
  set points**, where `fc_irq_line` rose eleven master clocks early so the CPU
  took the interrupt one instruction sooner — caught by blargg's `08.irq_timing`,
  a third-party ROM rather than our own trace agreeing with itself.

  A compensating fix was found **and rejected**: delaying the APU's IRQ by one
  cycle also gave 66 of 66 and is indistinguishable from the real fix by gate
  result. `cpu6502.sv` already implements the oracle's second-to-last-cycle
  recognition, correctly gated, so a second delay would have cancelled an
  APU-side error. Looking for a cause *after* the fix worked is what separated
  them. Suite: **66 gates green, 0 failed.**

- **The DUT's 6502 decodes all 256 opcodes, and an independent oracle now says
  so (sibling repository).** The five `SH`-group stores — `SHA` (`$93`/`$9F`),
  `TAS` (`$9B`), `SHY` (`$9C`), `SHX` (`$9E`) — close the decoder at **256 of
  256**. They are address mangling rather than arithmetic: the value stored is
  `reg & (base_high + 1)`, and on a page cross the address's own high byte
  becomes `addr_high & reg`.

  More importantly, **blargg's `instr_test-v5` battery is now a standing gate** —
  sixteen third-party ROMs, ~2.68 M cycles each, compared per cycle, **16 of 16
  exact**, taking the suite from 50 gates to **66**. Every rung-1 ROM before
  these was written inside the project, so the rung could only ask questions
  someone there thought to ask. These found **three defects the entire
  self-written corpus had missed**, and none was in the opcodes the battery was
  run to validate:

  - `RRA` fed its `ADC` stage `p[FLAG_C]`, the carry from *before* the
    instruction, instead of the one the rotate had just produced. **The
    instruction's own bus trace was identical on both sides** — read, dummy
    write, write — and only the accumulator differed, by one, surfacing nine
    cycles later in the `STA` that spilled it. A gate on the memory side of
    read-modify-write would have passed it.
  - The 8-cycle indirect read-modify-write forms addressed the indexed target
    during their *pointer* fetch cycles.
  - The PPU I/O-bus latch never decayed — a 2C02 defect reached from a CPU ROM,
    three rungs after rung 3 closed.

  The decay is implemented, and **the fitted part is disclosed in the RTL
  itself**. That the latch decays, in three independent groups, and which
  accesses refresh which group, are documented facts. The *deadline* is not: the
  wiki says 3-30 ms "faster when the PPU is warm", and RustyNES uses 558.7 ms.

  **Swept against the full 66-gate suite**, not argued: 30 ms (the documented
  upper bound) fails 9 gates, 50 ms fails 5, 100 ms 3, 200 ms 2, 300 ms 1, and
  558.7 ms is the first value that fails none. The binding constraint is one
  measurable property of one ROM — `10-branches` has a longest gap between
  group-0 refreshes of 936,697 CPU cycles, or 2,810,091 dots — and that
  prediction was **tested**: 2,809,000 dots leaves 52 divergences and 2,811,000
  is exact, so the corpus demands ≥ 523.4 ms.

  **The documentation and the corpus are therefore incompatible by a factor of
  ~17**, and this rung has no independent oracle to say which describes a 2C02.
  That is risk 6 of the Fabric plan — the oracle can be wrong — arriving as a
  measurement rather than a caveat, and the first time in this programme that
  documentation and oracle have been shown to contradict each other on a
  quantity a gate depends on. The constant stays the oracle's, stays labelled
  fitted, and stays a `localparam` so it can move when something can adjudicate.

  Still no `rustynes-*` crate changes; AccuracyCoin and nestest remain untouched
  by construction.

- **Android dependency refresh.** AGP **9.2.1 → 9.3.2** (both
  `com.android.application` and `com.android.test`), the Compose compiler plugin
  **2.3.10 → 2.3.21**, `androidx.baselineprofile` and `benchmark-macro-junit4`
  **1.5.0-alpha06 → 1.5.0-rc01**, `compose-bom` **2026.06.00 → 2026.08.00**, the
  three `material3.adaptive` artifacts **1.2.0 → 1.3.0**, `jna` **5.18.1 →
  5.19.1**, and `play-services-games-v2` **21.0.0 → 22.0.0**.

  This is the `gradle` Dependabot ecosystem added in the previous refresh doing
  exactly what it was added for: it opened five PRs on its first run, against a
  set that could not be hand-bumped safely before because nothing proposed them
  one at a time. Two of the bumps here are **not** among those five — the Compose
  compiler plugin and `baselineprofile` — because the ecosystem's
  `open-pull-requests-limit` is 5 and all five slots were taken.

  **The interlock was measured rather than assumed.** AGP 9.2.1 and 9.3.2 declare
  the *same* `kotlin-gradle-plugin` coordinate (2.2.10) in their published POMs,
  so crossing that minor does not move the Kotlin requirement. Recorded in
  `android/build.gradle.kts` alongside a second fact worth not re-deriving:
  **there is no `kotlin-gradle-plugin` version in this build to set at all.**
  AGP 9 bundles it and the standalone `org.jetbrains.kotlin.android` plugin was
  deliberately dropped, so the Compose compiler plugin is the only Kotlin
  coordinate this build controls. To move Kotlin, move AGP.

  `androidx.baselineprofile` moves **alpha → rc within the same 1.5.0 line** —
  the same supported-AGP window with fewer unknowns, not a new dependency
  decision.

  Rust, GitHub Actions and the pre-commit hooks are unchanged: re-checked at the
  same time and all already current from the previous refresh.
- **Dependency and toolchain-adjacent refresh.** 17 crates moved to their newest
  semver-compatible versions (including `cc` 1.4.3 → 1.4.4 and `log` 0.4.33 →
  0.4.34), `directories` 5 → 6, and four GitHub Actions advanced —
  `actions/setup-java` v5 → v6, `actions/download-artifact` v7 → v8,
  `taiki-e/install-action` 2.86.1 → 2.86.7, and `dtolnay/rust-toolchain`'s SHA
  pin to the current `v1`. Both action majors are ESM migrations that are
  transparent to the caller; `download-artifact` v8 additionally promotes a
  digest mismatch from a warning to an error, which is a gate getting stricter
  rather than a behaviour change. AccuracyCoin **141/141 (100.00%, RAM
  decoder)** and nestest 0-diff **verified, not asserted**.

- **`markdownlint-cli` v0.39.0 → v0.49.1, and MD060 becomes a live gate.** The
  pin had been held since v2.3.9 precisely because the newer binary reported a
  rule the pinned one did not — recorded at the time as a hazard rather than
  measured. Measured now: the rule is `MD060/table-column-style`, and its
  inferred default reads this corpus as style `compact`, producing **1,936
  findings across 122 files** and nothing else. Every one of those is a table
  the project already writes the same way, so the fix is to pin the style in
  use (`leading_and_trailing`) rather than to rewrite 122 documents or silence
  the rule: that measures **zero** findings and changes no document. The style
  is pinned rather than left `consistent`, which would accept a file whose
  tables are uniformly the other way.

- **`.pre-commit-config.yaml` hooks:** `pre-commit-hooks` v4.5.0 → v6.0.0.
  Neither hook v6 removes (`check-byte-order-marker`, `fix-encoding-pragma`) is
  used here, and both of its new minimums are met. `actionlint` is already at
  the newest release and matches the locally built binary, so it does not move.

- **Gradle wrapper 9.4.1 → 9.7.1**, and Dependabot gains the `gradle` ecosystem.
  AGP 9.2.1 / the bundled KGP 2.3.10 / `androidx.baselineprofile`
  1.5.0-alpha06 are deliberately **not** hand-bumped: they are an interlocked
  set whose pins each carry a written reason, and none is verifiable without an
  Android SDK and NDK. Dependabot proposes them one at a time against a job
  that builds, which is what makes such a bump checkable.

### Fixed

- **The Trunk `wasm_bindgen` CLI pin was stale against the lockfile** — `0.2.126`
  against a resolved `0.2.127`, drifted in #397 and never corrected. Stated
  honestly: the Pages deploy has been **passing** throughout, so this restores a
  stated invariant rather than fixing an outage.

### Held, deliberately

- **egui / egui-wgpu / egui-winit 0.36 and wgpu 30** remain held. The hold names
  `egui-winit` **0.36.1** as the broken version and 0.36.1 is still the newest
  published, so there is nothing new to re-test — the hold stands on its own
  stated terms rather than by inertia. **`naga` 30 is held with them, and now
  enforced** — it is a *direct* dependency of `rustynes-frontend` (WGSL
  validation), not merely a wgpu transitive, so Dependabot would have
  proposed it on its own and broken the hold from a direction the existing
  four ignore entries did not cover. Caught in review.
- **`getrandom`** stays at the 0.2 + 0.3 pair. Those shims exist to activate a
  wasm backend for `piccolo`'s *transitive* tree (`rand` → 0.2, `ahash` → 0.3);
  declaring 0.4 would add a third major that nothing uses.
- **Rust 1.96.0** and **Quartus 17.0.2** are unchanged. A compiler bump does not
  belong in a dependency refresh, and 1.97 is where the libretro build image's
  injected `-C ar` stops being a warning and becomes a hard error.

## [2.6.2] - 2026-08-24 - "Witness" (rung 4 closes on blargg's APU battery, and a suite that had been asserting nothing for five releases)

### Fixed

- **The NTSC `blargg_apu_2005` suite asserted nothing, and had since it was
  written.** It read the `$6000` status protocol, but these 2005-era ROMs are
  plain NROM with no PRG-RAM, so `$6000` reads back `0` forever — and `0` is
  blargg's *success* code. All eleven assertions were statements about an
  unmapped address. The identical defect was found and fixed for the PAL half of
  the same corpus in v2.1.5, whose header has described it as "a false oracle
  that validated nothing" ever since; the NTSC half was never migrated. Reusing
  the PAL fix is also wrong — these ROMs report a numeric result code rather than
  `PASSED`/`FAILED`, so the screen decoder returns `Unresolved` for all eleven.
  The suite now uses `run_nes_result_code` per the corpus's own `tests.txt`
  ("a result code of 1 always indicates that all tests were passed"), and **all
  eleven genuinely pass**, settling in 11-26 frames instead of exhausting an
  1800-frame budget. The 11/11 figure is unchanged; it is now earned.
  `vacuity_of_the_6000_protocol_on_this_corpus` pins the reason as an executable
  fact rather than a comment.

### Added

- `run_nes_result_code` / `CodeVerdict` in `rustynes-test-harness` — the third
  reader this corpus family needs, keeping `Passed`, `Failed(n)` and
  `Unresolved` distinct so an exhausted budget can never read as agreement.
- A `blargg_apu_2005` row in `docs/STATUS.md`'s suite table. The NTSC half of
  the corpus had no row at all, while the PAL half's row documented the same
  false-oracle correction in detail — which is part of why the NTSC half went
  unmigrated for five minor releases.

### Added (MiSTer co-simulation DUT — sibling repository)

- **Rung 4 closes.** blargg's `blargg_apu_2005.07.30` battery went from 0 of 11
  to **11 of 11 exact** against the DUT — the first rung-4 evidence with an
  *independent* oracle, and every one of the six root causes was invisible to
  the gates this project had written for itself. **48 gates green, 0 failed;
  56 of 56 mutations CAUGHT.**
- The frame sequencer now counts **CPU cycles** with the documented step
  positions (7457 / 14913 / 22371 / 29828 / 29829 / 29830), derived from the
  wiki's APU-cycle table plus its GET/PUT column — "3728, PUT" is CPU
  `2*3728+1`. No rounding and no per-step calibration, which the previous
  model's own comment had flagged as the sign its origin was wrong.
- Six defects: a `$4017` reset counted as a sequence wrap; the reset landing one
  CPU cycle late; `$4015` not seeing a same-cycle length clock; a sequence with
  29,831 states instead of 29,830 (a drift, not an offset); the length halt
  applying a cycle early; and the mode-1 immediate clock firing at the write
  rather than the reset landing, plus the reload drop.
- Two of the six rules are **absent from the nesdev wiki** and stated in
  blargg's own `readme.txt`: the length-halt delay and the reload drop.
- New ROMs `apuconflict039`, `apudmawrite038`, `apuinhibit040`, `apupower041`
  and `apureload042`, closing ledger items 6.3, 6.4, 3.6, 8.10 and 8.11.
  `apuirq036` is restored as a gate, demoted through 7.1 and 7.2 and now exact.

### Note

The emulation core is **unchanged**: only `rustynes-test-harness` and
documentation are touched here, so AccuracyCoin 141/141 (RAM decoder) and
nestest 0-diff hold by construction rather than by re-measurement. The
co-simulation DUT work this release accompanies lives in the sibling
`RustyNES_MiSTer` repository, where blargg's battery went from 0 of 11 to
**11 of 11** and closed rung 4.

## [2.6.1] - 2026-08-24 - "Interleave" (the DMC and its DMA cycle steal in the MiSTer co-simulation DUT, cycle-exact on the bus. The emulation core is unchanged)

The DMC reads its own samples by stopping the CPU and taking a cycle. This
release implements that in the SystemVerilog DUT and proves it cycle-exact
against the oracle's per-cycle bus, which is the criterion the plan set for this
step. The emulation core is untouched.

### Added

- **The DMC channel**: the memory reader, the 7-bit delta-modulation output unit
  with its 8-bit shift register and bits-remaining counter, the 16-entry rate
  table (the register value is an **index**, not a period), the loop flag and the
  end-of-sample IRQ.
- **The DMA cycle steal**, implemented from `nesdev_wiki/DMA.xhtml`: halt (read
  cycles only), a dummy cycle, an optional alignment cycle, then the get — with
  the load halting on a get cycle and reloads on a put. The CPU has no RDY pin;
  holding its clock enable low for a cycle removes exactly one cycle of progress
  while the APU and PPU keep running, which is what a stolen cycle is.
- **`apudmc037`**, gated twice: on channel levels and on the per-cycle bus,
  because the DMA's cost and placement reach the CPU and not the mixer.

### Fixed

- **The DMC timer ticked on the wrong APU phase**, playing every sample two CPU
  cycles early.
- **The alignment test was inverted** — the wiki conditions it on whether the
  *next* cycle is a get, not the current one, which cost the load burst a cycle.
- **The stolen cycles were not marked as DMA** in the bus trace, so 195 cycles
  were compared as ordinary reads while their timing was already correct.
- **The data bus is held across a halt**: for all 144 residual cycles the
  oracle's value was frozen for the whole burst, with only the get driving a new
  one.

### Verified

- **The bus gate is exact**: 323,661 diverging cycles to **0**. All 357,360
  overlapping cycles match on `pc`, `bus_addr`, `bus_data` and `bus_access`, and
  the DMA is 49 bursts of 195 cycles against the oracle's 49 and 195.
- **32 gates green across rungs 1-4; 46 of 46 mutations CAUGHT**, 0 NOT CAUGHT,
  0 BUILD-FAILED.
- **AccuracyCoin 141/141 (100.00%, RAM decoder)** and nestest 0-diff, by
  construction: no file under the chip crates changes.

### Known limitations

Both are recorded with the measurement that established them, in
`docs/apu-oracle-vs-documentation.md` in the sibling repository.

- **The DMA / register-read conflict is not modelled.** A sample fetch coinciding
  with a register that has read side effects corrupts the transfer on hardware,
  and the oracle reproduces it. This harness reads a flat array and has no bus to
  corrupt, so `apudmc037` deliberately avoids provoking it.
- **"DMA can only halt on CPU read cycles" is implemented but not exercised.** Of
  `apudmc037`'s 49 bursts, zero are preceded by a write cycle, and removing the
  condition produces byte-identical output.

### Retracted

- **The pre-registered risk that the DMA's stall placement was oracle-defined.**
  It was recorded before the work started, from the oracle's own comment
  describing its scheduler as calibrated. `DMA.xhtml` documents the whole
  sequence precisely, so the risk did not exist. A note that an implementation
  was *calibrated* says nothing about whether documentation exists.

## [2.6.0] - 2026-08-24 - "Assay" (the triangle, the noise channel and the sweep unit in the DUT, and an audit of what was fitted rather than derived)

An assay tests a metal to find out what it is actually made of. This release does
that to the APU rung: it asks, for every behaviour, whether it was written from
public hardware documentation or tuned until it agreed with RustyNES — and then
acts on the answer. The emulation core is untouched.

### Added

- **Triangle and noise channels** in the SystemVerilog DUT: the 32-step sequencer
  folded from a 5-bit counter, the linear counter and its reload flag, a 15-bit
  LFSR with bit-1 / bit-6 tap selection, and the 16-entry period table (the
  register value is an **index**, not a period).
- **The sweep unit**, which v2.5.9 deferred and had modelled only the mute for.
  Its period update was **absent entirely**, which made `sweep_mutes` correct and
  untestable: a period nothing updates can never reach an overflowing target.
- **`docs/apu-oracle-vs-documentation.md`** — the ledger this release is named
  for. Every place the DUT follows the oracle rather than the wiki, sorted by
  risk, each with the text it is measured against and the independent check that
  would adjudicate it. Maintained going forward, with an explicit rule: an item
  closes only when a gate exercises it **and** a mutation against it is CAUGHT.
- **Nine new gate ROMs** — `aputri028`, `apunoise029`, `apusweep030`,
  `apuquarter031`, `apuquarter032`, `apuneg033`, `apusweepdiv034`,
  `apuquarter5_035`, `apuirq036` — taking rung 4 from two to **eleven**, plus a
  `bus` mode for the mutation harness. `tb/regress.sh` runs all 30 gates across
  rungs 1–4 in one invocation.

### Fixed

- **Two errors that were cancelling each other.** The `$4017` reset delay was
  keyed on the **mode bit**, which the wiki never mentions — it keys on the
  write's APU-cycle parity — and it was exact *only* in combination with the
  5-step sequencer's step 2 held one tick off the convention its fifteen siblings
  follow. Either correction alone costs 2 cycles, in **opposite directions**;
  both together are exact. The fitted rule was not merely unfalsified, it was
  **load-bearing** for a second error. All sixteen sequencer constants are now
  uniformly documented − 1, with nothing fitted.
- **The frame IRQ's coincident-read rule was inverted**, under a source comment
  asserting the ordering was correct. A `$4015` read landing on the assertion
  edge returned 0 *and* destroyed the assertion, so the flag never came back. The
  wiki says such a read returns 1 and is not cleared.
- **The frame IRQ was asserted on only one half of its APU cycle**, where the
  wiki lists both the GET and PUT halves.
- **The APU observation point** was one cycle early — the testbench sampled
  channel levels before a cycle's clock edges while the oracle emits its record
  after. Identified from the shape of the residual: all 5,076 remaining
  divergences satisfied `dut[c] == oracle[c-1]`, unanimously.
- **The power-on APU divider phase** was inverted, verified independent of the
  above rather than merely sufficient.

### Verified

- **The oracle passes blargg's APU battery 29/29** (`apu_test` 8/8, `apu_reset`
  6/6, `apu_mixer` 4/4, `blargg_apu_2005` 11/11) — run for the first time in this
  programme, and recorded per audit item because it means different things in
  different places.
- **30 gates green across rungs 1–4; 35 of 35 mutations CAUGHT**, 0 NOT CAUGHT,
  0 BUILD-FAILED.
- **AccuracyCoin 141/141 (100.00%, RAM decoder)** and nestest 0-diff. The
  emulation core is untouched by this release, so these hold by construction —
  the changes are in the sibling DUT, the co-simulation crate, and documentation.

### Retracted

- v2.5.9 characterised its residual as a **`$4003` write-parity sensitivity**. It
  was not: it was the inverted power-on tick parity plus the observation point.
- The `apuquarter032` residual was characterised as a **rounding effect** on a
  counter that must round half-APU-cycle steps. It was not: it was one sequencer
  constant off by one, and correcting it closed the residual with no change to
  any rounding logic.

## [2.5.9] - 2026-08-24 - "Overture" (rung 4 opens: the two pulse channels, the frame counter, and four ROM defects the stimulus measurement found first)

### Added

- **Rung 4 opens.** `rtl/apu2a03.sv` in the sibling repository gains both pulse
  channels — timer, 8-step duty sequencer, length counter, envelope and the
  sweep **mute** — plus the frame counter in both modes with its IRQ, and the
  `$4015`/`$4017` register file. Written from the NESdev wiki; no emulator or
  HDL source consulted, per ADR 0037. Triangle, noise and DMC are v2.6.0/v2.6.1
  and are driven to zero.
- **The gate/diagnostic partition, written BEFORE the rung** (the sibling's
  `docs/rung4-apu.md`). The APU is the hardest chip in the console to gate
  honestly, because what it *produces* is an analog level and what an emulator
  computes is a number. **Gates:** the `$4015` read value, the `/IRQ` pin, and
  each channel's **integer** DAC input, all per CPU cycle. **Diagnostics:**
  `MixRecord`'s `f32` `mixed`/`external` (RustyNES's non-linear mixer,
  decimator and expansion gain — a modelling choice), the frame-sequencer step
  index (internal, no pin), and `apu_phase`.
- **`--apu-trace` on `nes_golden_export`**, exporting the five integer channel
  levels per CPU cycle as 16-byte records with an **explicit** cycle — the
  records are drained per frame, so an index-implied cycle would be wrong the
  moment a frame boundary shifted anything. `rustynes-cosim` enables
  `debug-hooks`; that this changes no emulated behaviour is **verified, not
  assumed** — `obs.bin`, `index_fb.bin` and `ram.bin` are byte-identical with
  and without it.
- **Two rung-4 stimulus ROMs** (`apupulse026`, `apulen027`) covering both
  pulses at different periods, duties and volumes, the frame IRQ and its
  interrupt sequence, the 4-step and 5-step modes, the inhibit flag, and a
  length counter that expires against one that is halted.

### Fixed

- **The duty sequencer counts up.** Counting down gave the right period and the
  right levels with the wrong phase — pulse 1 three sequencer steps late and
  pulse 2 five.
- **The 4-step sequence constants must be consistently 0-based.** `fc_count`
  reads V on tick V+1, so each documented step is V−1. Three of four were and
  the last was written as the wiki's own number, putting the frame IRQ **3 CPU
  cycles late** (29,830 against 29,827, measured) and the last length-counter
  clock of every frame with it.
- **`$4017` bit 7 clocks a quarter and half frame *immediately*** — a latched
  flag clocked on the next APU tick left exactly two divergent cycles at a
  length expiry.
- **The `$4017` sequencer-reset delay depends on bit 7.** The wiki says "3 or 4
  CPU clock cycles" without saying which applies when, and at this resolution
  the two are distinguishable: each constant fixed one ROM and broke the other,
  and a parity rule on `apu_phase` separated nothing because both ROMs' writes
  land on the same phase. Bit 7 — the bit that also fires the immediate clock —
  does separate them.

### Verified

- **`apulen027` is exact on both surfaces** (178,668 cycles each), and
  `apupulse026`'s bus surface is 3 of 178,668.
- **`apupulse026`'s channel levels are 1,000 of 178,668 — 500 runs of exactly
  two cycles, one per pulse edge.** A uniform one-APU-tick offset, not a
  structural fault, and a **phase sensitivity the first stimulus hid**: adding a
  five-cycle counter initialisation — an *odd* number — flipped which
  `apu_phase` the `$4003` writes land on. Two candidate fixes were tried and
  **both rejected by measurement**; the wiki is right that the period divider is
  not reset. Carried to v2.6.0 with the ROM that exposes it already written.
- **Nine of ten mutations CAUGHT.** Two were NOT CAUGHT on the first pass and
  **both indicted the stimulus rather than the gate** — a halt flag whose
  channel's length was too long to expire in the window, and an inhibit bit only
  ever set in 5-step mode where the IRQ cannot fire anyway. Both ROMs were
  changed and both are now caught. The tenth is the sweep mute's threshold,
  out of stimulus regardless (both ROMs use periods 64 and 84, so any threshold
  below 64 is inert) and belonging to the sweep unit this rung defers.
- **The stimulus measurement found four ROM defects before any gate ran** —
  length index 3 is **2**, not 254; the 6502 boots with I set, so without `CLI`
  zero IRQs are taken despite five real line edges; two channels at the same
  volume are indistinguishable in a channel-level golden; and power-on work RAM
  is **seeded, not zeroed**, so an uninitialised counter came up `0x7D` and the
  handler's `CMP #3` never matched.
- **Zero emulation-core changes.** The oracle-side diff is the excluded
  `rustynes-cosim` crate plus documentation, so **AccuracyCoin holds 141/141
  (100.00%, RAM decoder)** and nestest stays 0-diff by construction.

## [2.5.8] - 2026-08-24 - "Blanking" (VBlank, NMI and the PPUSTATUS race close rung 3 — and both fixes were deletions)

### Added

- **Rung 3 closes.** The sibling's `rtl/ppu2c02.sv` gains the VBlank flag's full
  CPU-visible behaviour: the set at 241/1, the pre-render clear, the destructive
  `$2002` read, **`suppress_vbl`** (a read landing one PPU clock before the set
  suppresses the flag — and the NMI — for that frame), and the read-on-the-set-dot
  case, which needs no register at all: the read's clear is the last assignment
  in the `always_ff` and wins over the same-edge set. **The PPU's /NMI reaches
  the CPU for the first time**, wire-ANDed with the harness injection pin.
- **Four purpose-built VBlank ROMs** (`ppuvbl022`–`025`), each with its stimulus
  **measured from the oracle's own trace before anything ran**: the read race
  (one read on *each* of dots 0/1/2 of scanline 241), NMI delivery with reads
  racing it (21 deliveries, 21 line edges, 2 race-dot reads), `$2001` toggles
  swept across pre-render dot 339 at one-dot grain, and the late NMI enable
  (`$2000` bit 7 re-enabled mid-VBL — two deliveries per frame). A first draft
  put a handler inside the power-on NOP slide and reset *executed* it — both
  sides agreed, because both read the same wrong ROM; the stimulus measurement
  (nmi_line never rose) is what caught it.
- **nestest 0-diff at 5,002,992 cycles — the 5M-cycle window, an acceptance
  criterion standing since v2.5.0, closes.** En route the window went 59,554 →
  357,360 (the harness now serves open-bus `$40` for `$4016`/`$4017`) → 5M.

### Fixed

- **The testbench's cycle split was `[2 pre-dots | access | 1 post]`; the
  oracle's is `[1 | access | 2]`** (`read_split(12) = (5,7)`). A `$2002` read
  racing the VBL set produces a **~2-dot /NMI pulse**, and the DUT's
  end-of-cycle sample sat one dot after the access where the oracle's phi2
  sample sits two — one NMI in 24 frames was missed. `PPU_LEAD=3` with
  `ACCESS_DOT=1` keeps the access on the same absolute dot and moves the cycle
  boundary; the pulse-stretcher built first was then **measured dead and
  deleted**.
- **The skip-check delay does not exist.** `ppuvbl024` caught the DUT skipping
  ten pre-renders the oracle never skipped — invisible to the bus gate for
  eight frames, because NMI delivery quantizes away single-dot drifts. The
  delay pipe samples the *pre-write* mask on the commit edge, so the oracle's
  two-PPU-clock rule plus that asymmetry lands exactly on the rendering enable
  itself: **`render_for_skip` is deleted** and the skip condition reads
  `rendering`.

### Verified

- **Twelve of twelve mutations CAUGHT**, three at exactly one divergence. The
  last — an undelayed skip check — differs on exactly one landing (an enable
  write at pre-render dot 338 of an odd frame) that is **unreachable by any
  fixed-cadence ROM**: the CPU's 3-dot quantum and the skip's 1-dot drift
  co-evolve, locking odd-frame landings to one residue mod 3. `ppuvbl024`
  breaks the cadence once — one frame branches past both writes, skips, and
  shifts the class onto dot 338 for the rest of the run. Caught at 4,792
  divergences.
- Every rung-3 gate exact: the four VBlank ROMs at 714,729 / 714,730 /
  **1,191,220** / 714,730 cycles, and every earlier surface unchanged.
- **Zero emulation-core changes** — the oracle-side diff is documentation only —
  so **AccuracyCoin holds 141/141 (100.00%, RAM decoder)** and nestest stays
  0-diff by construction.

## [2.5.7] - 2026-08-24 - "Collimation" (sprite rendering closes exact — the phase was wrong by two dots, and every window was compensating)

### Added

- **Rung 3's sprite rendering, priority, sprite-0 hit and overflow — and the
  whole rung goes exact.** The sibling's `rtl/ppu2c02.sv` gains the eight sprite
  slots, the priority mux, the left-8 masks, the sprite-0 hit with its
  documented no-hit-at-x=255 quirk, the garbage nametable fetches and the
  dots-337/339 dummy fetches — and then the release's real finding lands:
  **`PPU_LEAD=2`**. The CPU–PPU power-on phase was wrong by two dots, and it was
  invisible because two errors cancelled — the boot traces agreed on
  `scanline`/`dot` at every instruction boundary while the per-cycle mappings
  differed by exactly two, a phase offset hidden by an equal record-point
  offset. With the phase corrected, the OAM windows move from
  documented-minus-three to **documented-minus-one — registered-assignment
  semantics, no residual fudge** — and every gate in the rung reports **zero
  divergences for the first time**: `ppuspr019/020/021` 119,115 cycles each,
  `ppuscroll` 49,998, `ppusprender` 119,114, `ppusprite` 59,993, `ppuregs`
  12,841 records, fetch traces 7,058 and 88,685, all three index framebuffers
  61,440 pixels, `nestest` 59,554.
- **The odd-frame skipped dot**, and the gate that can see it. `ppuspr020`'s
  last divergence was a *drift* — first frame exact, second frame one read late
  — because the DUT never skipped pre-render dot 339 on odd rendering frames.
  The skip lands gated on a two-dot-delayed rendering enable (the oracle's
  `mask_skip_pipe1` pair), and **`ppu-phase-gate`** — the inverse of
  `cpu-gate`'s skip list, comparing *only* `scanline`/`dot` over twelve frames —
  is the gate that pins it: 98,562 records, with five skip mutations CAUGHT.
- **Three sprite-0 edge ROMs** (`ppuspr019/020/021`): transparent background,
  X=248 boundary, and X=255 — each setting sprite 0 once before rendering, after
  a one-ROM draft that rewrote OAM mid-frame produced 3,454 divergences of pure
  reasoning burden.
- **`PPU_SUBDOT`, a master-clock-resolution instrument** — four `ppu_clk` pulses
  per dot with `ce` on one — built to test whether the half-dot between
  `read_split` and `write_split` is observable. The half-dot is confirmed
  unobservable, **measured rather than argued**; what the instrument found
  instead is the `cpu_ce` defect below.
- `status` in the sprite-eval probe's printout (`sprite_eval_probe.rs`) — the
  `$2002` half of a divergence, next to the `$2004` half it already printed.

### Fixed

- **The DUT's CPU register file was gated by nothing** — every `$2000`–`$2007`
  write, `$2002`'s destructive read and `$2007`'s buffer advance latched on
  *every* clock edge, correct only because the harness pulsed the decode for
  exactly one pulse per CPU cycle. Latent until v2.6.5, where `nes_top` holds a
  decoded address for the full 12-master-clock cycle and every write would latch
  twelve times. Fixed with `cpu_ce`, a one-clock commit strobe separate from the
  decode; all four sub-dot phases are byte-identical after it, and the pre-fix
  behaviour is CAUGHT by two independent gates.

### Changed

- **Reads and writes can be placed on different dots in the testbench, and the
  measurement says they coincide.** `ppufetch` pins the write to dot 2,
  `ppuscroll` independently rules out dot 0, and `ppuspr020`'s then-residual was
  unchanged across all eight placements — a hypothesis removed, not a knob
  tuned. The oracle's two-master-clock write offset is half a dot: below dot
  resolution, and now demonstrated so.

### Verified

- **All ten of the rung's mutation catalog are CAUGHT** — re-run in full at the
  corrected phase, because a phase change is more invasive than a stimulus
  change. The tenth (sprite-0 flag read from the register only) is caught by
  `ppuspr020` at **exactly one divergence: the read landing on the hit dot**.
  The `$2004` readback-off-by-one mutant reproduced the old catalog's exact
  count, 110. The remaining NOT CAUGHT results are classified, each with
  evidence and an owner — three deferred coverage gaps and two inert mutants: the vblank
  register-only read (stimulus gap on the v2.5.8 race dot), the skip-check delay
  (blargg `10-even_odd_timing`, needs v2.5.8's VBlank sync), `chr_wr` (measured
  **0** assertions across all eight CHR-ROM test ROMs; reachable at v2.6.3), and
  two provably inert mutants.
- **Zero emulation-core changes** — the oracle-side diff is one diagnostic
  printout in the test harness — so **AccuracyCoin holds 141/141 (100.00%, RAM
  decoder)** and nestest stays 0-diff by construction.

## [2.5.6] - 2026-08-23 - "Vestige" (sprite evaluation closes, and a byte index that outlives the walk that set it)

### Added

- **Rung 3's sprite evaluation, gate green.** `rtl/ppu2c02.sv` in the sibling
  repository gains the evaluation FSM, secondary OAM, the eight-sprite limit, the
  documented overflow-search bug and the wiki's step 4 — compared as what a CPU
  read of `$2004` returns while rendering. **All 59,993 overlapping cycles
  match**, with **seven of eight mutations CAUGHT** and the baseline verified
  passing first. This was the step the plan named as the hardest single item in
  the programme.
- **`oam_bus_copybuffer` in `PpuStateRecord`**, at schema **2** (`RECORD_SIZE`
  113 → 114). Filled from `Ppu::oam_data_bus_observed()` — what a CPU read of
  `$2004` would return at that dot, guard included, minus that path's side
  effects. Diagnostic, never a gate. **The trace is outside the save state; the
  PPU field it reads is not** — `Ppu::oam_bus_copybuffer` is emulation state and
  has been serialized since v2.0. An earlier draft of this entry said "outside
  the save state" without distinguishing the two, which reads as a claim about
  the PPU field and would be wrong.
- `EV_SL` and `EV_ALL` on the sprite probe, and `EV_SL` / `EV_CYC` dumps in the
  sibling's testbench. `EV_CYC` is what turns a divergence reported *by cycle*
  into a scanline and a dot.

### Changed

- The overflow search now **ends** where the hardware ends it — the in-range hit
  plus the three entries step 3a owes — instead of walking to `n = 63`.
- `eval_ovf_cnt` joins the per-line arm at the end of the secondary-OAM clear —
  **defensive, not a fix.** It is unreachable: zero at every window end,
  byte-identical with it removed, and bounded structurally at 5 decide-steps of
  margin. Kept because the oracle clears its counterpart at cycle 65 and the
  margin is thin — and **the bound is now a standing check** rather than a
  measurement in a document: the sibling's testbench verifies it on every gate
  run, exits non-zero if it ever breaks, and reports its denominator
  (`0 violations in 528 window ends`) so a zero cannot read as a silence.
  Demonstrated firing on a mutant that removes the count's decrement.

### Fixed

- **Phase 4 keeps the byte index it inherited.** The wiki says `OAM[n][0]`, and
  pinning the index to 0 is right on one scanline and wrong on the next: phase 4
  advances only the high half of the address, so the low half keeps whatever
  ended the walk. Three of the four paths that finish evaluation clear it; the
  sprite-eval bug path does not. Line 55 ends via the overflow count and walks at
  `m = 0`; line 58 ends via the bug wrap and walks at `m = 3`.
- `eval_hold` — a latched byte two earlier findings had been built on — is
  **removed**, along with its reset and all four assignments. It became dead the
  moment the write dot presented `sec_oam[sec_idx]`, and Verilator's
  `UNUSEDSIGNAL` said so before the gate ran.

### Verified

- **AccuracyCoin 141/141 (100.00%, RAM decoder)** and **nestest 0-diff** —
  `rustynes-ppu` changed, so both were re-run rather than asserted. Workspace
  tests: 0 failures. `rustynes-core` still builds for `thumbv7em-none-eabihf`.
- **Regression across every earlier rung:** `ppu-render-gate` all 61,440 pixels
  match, `ppu-fetch-gate` all 6,247 compared fetches match, `nestest-gate` all
  59,554 overlapping cycles match, `check_rtl_subset.py` clean.
- **A field name that matches is not a behaviour that matches.** Two changes had
  been made faithful to the trace's `sprite_eval_*` fields and both made things
  worse (41 → 112, 39 → 68). Those fields belong to the oracle's real FSM;
  `$2004` comes from `tick_oam_bus`, a second, side-effect-free model. **The gate
  observed the model the trace did not expose**, which is why the schema-2 field
  above had to exist before any further RTL edit was worth making.
- **A change rejected against a broken baseline is not a rejected change.** The
  overflow halt was recorded as a regression at 39 → 68. Re-measured once phase 4
  was itself correct, it is right, and worth 28 of the 39.
- **Nine of nine behavioural mutants CAUGHT, and two proved INERT** — by byte
  comparison against the baseline, not by reading the code. A first pass reported
  one of them as an uncaught defect the stimulus could not reach; that was
  **wrong and is retracted**. The `eval_ovf_cnt` reset is *unreachable*, not
  under-exercised: a probe fires zero times at 528 window ends while its inverted
  predicate fires 528, removing the reset yields a byte-identical trace, and the
  bound is structural (88 decide steps to the latest possible hit, consumed by
  91, in a 96-step window). The second inert mutant — the hit not setting
  `sprite_overflow` — is *out of scope*: that flag reaches the CPU only through
  `$2002`, and the ROM contains zero `$2002` reads against sixteen `$2004` reads
  per iteration. **NOT CAUGHT has meant four different things in this project,
  and only a byte comparison separates "the mutant did nothing" from "the gate is
  blind."**

## [2.5.5] - 2026-08-23 - "Raster" (the first full frame, and three blind spots in the stimulus that fed it)

### Added

- **Rung 3's background rendering, gate green.** `rtl/ppu2c02.sv` in the sibling
  repository gains the pattern and attribute shift registers, the fine-X
  multiplexer, the palette lookup and a per-pixel index — and produces the first
  full frame this core has drawn. **All 61,440 pixels match** the oracle's index
  framebuffer, with **fifteen mutations all CAUGHT** and the baseline verified
  passing first.
- `make -C tb ppu-render-gate` and `tb/fb_diff.py`, which report the **shape** of
  any difference — population count, first pixel in raster order, inclusive
  bounding box, worst rows — because a bare count is unactionable.
- `docs/golden-fetching.md` in the sibling repository, specifying the standing
  "until golden fetching from a pinned oracle commit exists" note that had sat
  across six releases without anyone recording what it required.

### Changed

- **The oracle needed no change at all.** `index_fb.bin` has been exported since
  v2.4.1, so this is the third rung step in a row costing the oracle side
  nothing. That is what choosing the compare surface *before* the rung buys.
- The render gate **reads** its cycle window from the oracle's manifest beside
  the golden rather than keeping a copy; there is no `CYCLES_ppurender`.

### Fixed

- **A one-pixel horizontal shift, and the incomplete fix is what identified it.**
  The shift registers and the dot counter advance on the same edge, so the
  documented "shift on dots 2–257" applied each shift one dot after the pixel
  that should show it. Moving only the shift window took the first wrong pixel
  from x=9 to x=17 — one tile further in — which is what proved the reload was
  out of phase too. The resolution **removes** a register rather than adding a
  knob: the reload is the pattern-high fetch's own dot, so it takes `chr_din`
  directly and `bg_hi_latch` no longer exists.

### Verified

- **AccuracyCoin 141/141 (100.00%, RAM decoder)** and **nestest 0-diff**.
- **Five NOT CAUGHT mutations indicted the STIMULUS, not the gate** — and one of
  them three times over, for three different reasons: horizontal arrangement
  aliasing the two nametables, a zero coarse-X scroll whose only wrap `copy_x`
  undid before it reached a pixel, and a fill whose 256-period ramp made both
  nametables byte-identical. Each fix looked like it had closed the hole; only
  re-running the mutation showed it had not. **Re-run mutations after a stimulus
  change, not only after a code change.**
- `fb_diff.py`'s anti-trivial guard is **demonstrated firing** in all four paths.
  Two identical backdrop frames agree perfectly, which is a sharper trap than an
  empty window — an empty window at least looks empty. The instructive case: a
  frame with 9 distinct values and 100 non-modal pixels clears the distinct-count
  threshold and is refused only by the second, so the two are not redundant.

## [2.5.4] - 2026-08-23 - "Escapement" (the background fetch pipeline, and an access two dots early that five gates could not see)

### Added

- **Rung 3's background fetch pipeline, gate green.** `rtl/ppu2c02.sv` issues
  NT / AT / pattern-low / pattern-high fetches on the documented 8-dot cadence,
  compared against the oracle as an **address-bus** trace: **6,247 background
  fetches, 0 divergences** on scanline, dot and address across two rendering
  windows, with **eight mutations all CAUGHT** and the baseline verified passing
  first.
- **`ppu-fetch-trace`**, a default-off feature on `rustynes-ppu` /
  `rustynes-core` recording the address of every PPU VRAM read with its frame,
  scanline and dot. Hooked at the single choke point `Ppu::read_vram`, not at
  each call site, so a fetch path added later cannot escape it silently.
  Output-only and deliberately outside the save state.
- `make -C tb ppu-fetch-gate` and `tb/fetch_diff.py` in the DUT repository, plus
  a `fetch` gate in the mutation harness.

### Fixed

- **The testbench presented each CPU access two dots early.** A 6502 commits a
  write and samples a read at **phi2**, the last of the cycle's three PPU dots;
  `tb/cpu_main.cpp` presented it on the second, alongside the boot record. So
  enabling rendering through `$2001` took effect two dots early, and so did
  disabling it — the DUT issued one extra nametable fetch at each window's
  leading edge and dropped one at its trailing edge. One quantity, wrong by one
  constant, at both edges.
- **Five gates stayed green across the move, in both directions**, and that is
  not evidence it was harmless: rung 1's registers on nine ROMs, rung 2's
  per-cycle bus, the interrupt sweep, and the v2.5.2 register and v2.5.3 scroll
  gates all read state **once per CPU cycle**, so a uniform two-dot shift inside
  a cycle moves nothing any of them compare. This is the rung's first gate keyed
  to the dot counter and the first that could see it.
- **Six clippy findings in `ppu-state-trace` that no CI invocation had ever
  reached.** No workflow named any of the four trace features, and
  `--workspace --all-targets` covers default features only, so the
  `rustynes-cosim` clippy step compiled those modules as *dependencies*, where
  warnings are not denied. CI gains one explicit step per feature.
- The `nes_golden_export` usage text, which documented neither `--fetch-trace`
  nor any of the four injection flags.

### Changed

- **nestest's verified window more than doubled, 27,388 -> 59,554 cycles.** It
  was bounded by a missing peripheral rather than a CPU defect — nestest reads
  `$2002` at cycle 27,396 and the testbench had no PPU to answer. The bound is
  now the two-frame golden's length, an artifact budget rather than a wall.
- `tb/roms/ppufetch.nes` gains a second rendering window at `$2800` with
  background patterns at `$1000`. Two mutations had come back NOT CAUGHT against
  a gate that was working correctly, because the ROM held `v[11]` and `ctrl[4]`
  at zero throughout: the gate was not blind, the stimulus was. Fetch count
  3,099 -> 6,247, and both mutations to CAUGHT.
- The fetch comparison is **narrowed to the background window** (dots 1-256 and
  321-336), excluding sprite fetches (v2.5.7) and the two dummy nametable reads
  (v2.5.8). Every run prints how many records each side dropped, because a
  narrowing that is not announced reads as full coverage.

### Fixed (continued)

- **A one-record coverage hole in five of eleven rung-1 gate invocations.**
  `cpu_boot_trace_diff` required the DUT trace to *equal* the reference in
  length; it now requires it to **cover** the reference — a short actual fails
  because part of the reference window was never compared, and a longer one
  passes with the surplus announced. Under that rule `opgroup3`, `opgroup5`,
  `opgroup6`, `ppuregs` and `ppuscroll` were genuinely short by one record each.
  Five extra cycles on each window closes it, the same five for every one.
  Initially diagnosed as a benign boundary artifact and deferred; that diagnosis
  was wrong and is corrected in place rather than quietly replaced.
- **Three more never-linted clippy findings**, in `cpu_boot_trace_diff`,
  `ppu_trace_diff` and `trace_dma_4015` — binaries the MiSTer rung gates run on
  every comparison. The four trace steps added above lint `rustynes-core`; these
  live behind the same feature names on `rustynes-test-harness`, which no
  invocation reached. A fifth CI step now covers them.

### Security

- **`--fetch-trace` no longer accepts an unbounded capacity.** `FetchTrace`
  clamped only its initial allocation, so a large argument would have grown the
  buffer without bound — `--fetch-trace 1000000000` reallocating its way to
  twelve gigabytes. The stored capacity is now clamped to `MAX_CAPACITY`
  (1,048,576 records, 12 MiB) and the CLI **refuses** an argument above it rather
  than clamping silently, because a clamp the caller never learns about produces
  a golden covering less than the run it claims to. Raised in review of #450.

### Documentation

A release-wide sweep of the documents this release touches, and what it found.

- **`.gitignore` did not cover three of the golden exporter's nine artifacts.**
  `*.ram.bin` does not match `<stem>.ram_init.bin` — the suffixes differ — so the
  seeded work RAM every co-simulation gate now requires was never ignored;
  `irq.csv` had been uncovered since v2.4.2, and `fetch.bin` arrived with this
  release. Found by testing each suffix with `git check-ignore` rather than by
  reading the list, and each pattern verified to match nothing tracked before
  being added.
- **Both changelog headers advertised a coverage neither has.**
  `CHANGELOG-FULL.md` stops at `[2.0.4]` while `CHANGELOG.md` described it as
  holding "the full per-version detail" for every release. Both now state the
  boundary and name where the depth actually lives from v2.0.5 onward. It is
  deliberately not backfilled: reconstructing that detail from summaries would
  produce confident prose nobody measured.
- **README corrections.** `nes_golden_export` emits nine artifacts, not the five
  claimed since v2.4.1; the trace-lint gap is described as closed rather than
  open; and the "Fabric" line is marked delivered, with the current v2.5.1 →
  v2.7.0 line named.
- **`docs/testing-strategy.md` gained the co-simulation layer** it had never
  described — the gate/diagnostic partition, the mutation requirement, and the
  fact that these gates do **not** run in CI.
- **The MiSTer to-dos were three sprints stale.** `TASKS.md` still had v2.5.1's
  injection API unchecked; `IMPLEMENTATION_PLAN.md` said the PPU was "Not
  started" and quoted the superseded 27,388-cycle nestest bound; `SPRINT_PLAN.md`
  gained a status column. Its standing rule "a rung may not start until the one
  below is green in CI" is corrected — that is **not currently achievable**, and
  a rule nobody can satisfy is a rule that quietly stops being applied.

### Verified

- **AccuracyCoin 141/141 (100.00%, RAM decoder)** and **nestest 0-diff**.
  `rustynes-ppu` changed, so both were run rather than asserted.
- `fetch_trace` gains **five unit tests** — it had none. The byte layout is
  asserted against hand-written expected bytes rather than against `to_bytes`'s
  own output, because a test that builds its expectation with the function under
  test agrees with itself forever. Three mutations (byte order, the capacity
  clamp, a ring-buffer overwrite) are all CAUGHT.

## [2.5.3] - 2026-08-23 - "Hysteresis" (toggling rendering takes effect three dots after the write)

### Added

- **The scroll address logic** (`RustyNES_MiSTer@dbae44d`): `inc_x`, `inc_y`,
  `copy_x` and `copy_y` at their documented dots, and the `$2007`-during-rendering
  dual increment. The 29-versus-31 coarse-Y asymmetry is transcribed from the
  wiki's pseudocode, not remembered.
- **`tb/phase_delta.py`**, reporting the PPU phase relationship at *every*
  instruction boundary and printing each transition — one line means a constant
  offset, many mean drift. A spot reading cannot tell those apart, and this
  question was answered three times from spot readings with a different answer
  each time.
- **`scroll_window_probe.rs`**, the `ppu-state-trace` diagnostic that located the
  fault. Diagnostic only, `#[ignore]`d and feature-gated.

### Fixed

- **Toggling rendering now takes effect three dots after the write.** This core
  applied a `$2001` write immediately; the rendering window was a full CPU cycle
  too wide **at both ends**, costing exactly one coarse-X increment and invisible
  to everything except a read-back of `v`. Documented behaviour —
  *"approximately 3-4 dots after the write. This delay is required by Battletoads
  to avoid a crash."*
- **The PPU never reset in the testbench.** Verilator initialises `rst_n` to 0, so
  setting it to 0 produces no negedge, and `ppu_clk` was idle during reset
  assertion — the reset block never executed. The tell was that changing the
  power-on position did not move the measurement **at all**.
- **The boot record was sampled two dots early**, before the cycle's dots rather
  than after two of them.
- **`mkrom.py` now refuses an `ORG` onto a byte the program has already written.**
  The next instruction overwrites it, and if that byte is a branch *offset* the
  result assembles, runs, and tests nothing. The guard found **two more
  instances** immediately, one in the already-shipped v2.5.2 ROM.

### Changed

- The `ppuscroll` fill is **1 KiB with a page-distinguishing value**. 64 bytes
  was too small for `v` after rendering, and `value = offset` cannot see a
  nametable toggle, which moves `v` by ±1024 without changing the low byte.

### Verified

`rustynes-core` is **untouched**, so AccuracyCoin is **inherited, not re-run**.
DUT side: `ppuscroll` **19,813 records / 0 divergences**; rung 2's bus comparison
on the same ROM **49,993 of 49,993 cycles matching**; phase identical; `ppuregs`
12,840; nine opcode-group ROMs 2115; interrupt sweep 24/24. Mutations: no-delay
CAUGHT, 2 dots CAUGHT, **4 dots NOT CAUGHT** — the wiki says "3-4", so this ROM
pins the delay only to the documentation's own tolerance.

## [2.5.2] - 2026-08-23 - "Dormant" (the register file, and a gate that passed while testing nothing)

### Added

- **The 2C02's CPU-visible register file** (`RustyNES_MiSTer@d98ea3f`), opening
  rung 3. `$2000-$2007` and their `$2008-$3FFF` mirroring, the VRAM/palette bus
  with its **read buffer**, palette mirroring (`$3F10`/`$3F14`/`$3F18`/`$3F1C`),
  nametable mirroring, OAM, and the PPU data-bus latch — written from
  `nesdev_wiki` pages only, per the source map landed in v2.5.1.
- **The post-reset masking window.** Writes to PPUCTRL, PPUMASK, PPUSCROLL and
  PPUADDR are ignored for **~29,658 CPU clocks** after reset, and the
  PPUSCROLL/PPUADDR latch does not toggle either; PPUSTATUS, OAMADDR, OAMDATA
  and PPUDATA work immediately.
- **`docs/rung3-ppu.md`**, written **before** the rung started, fixing which
  fields may *fail* this rung and which may only *explain* a failure.

### Fixed

- **Four defects in the test ROM, every one found by mutation.** The ROM passed
  on its first run and **every mutation came back NOT CAUGHT**: the gate invoked
  `cargo` from the wrong tree so no comparison ran; absolute operands were
  written as one entry, assembling to `STA $xx00` with the following opcode as
  its high byte (**1 PPU access in 800 cycles**, against 71 after the fix); an
  unrelated open-bus probe wrote `$5A` to PPUMASK and **enabled rendering**; and
  a branch offset landed on the operand byte of `LDX #$00`, i.e. `BRK`.
- **Two further coverage gaps, also found by mutation**: the `$2007` increment
  *amount* was never read back, and the masking window was implemented and never
  verified.

### Changed

- Mirroring is labelled by **which address line drives CIRAM A10**, not by
  "horizontal"/"vertical" — the nesdev iNES page prints both words for the same
  bit.

### Verified

`rustynes-core` is **untouched**; the emulator does not change. DUT side: lint 0
findings, RTL subset clean, **12,840 records / 0 divergences** on the register
ROM with **8 mutations all caught**, nine opcode-group ROMs at 2115 records, and
the interrupt sweep 24/24.

## [2.5.1] - 2026-08-23 - "Retrace" (a return address, and a gate that reported a pass it could not have earned)

### Added

- **The interrupt sweep, and rung 2 closes** (`RustyNES_MiSTer@9425d73`).
  `tb/interrupt_sweep.py` asserts /NMI, /IRQ, or **both together** before
  instruction K and holds it, for every K across a hazard program, driving
  identical stimulus into both sides: **60 injection points, 0 divergences** on
  all seven CPU fields (20 instructions x three pin configurations).
- **The core-side injection API** ([ADR 0038](docs/adr/0038-cosim-interrupt-injection-api.md)),
  behind a default-off `cosim-interrupt-inject` feature that only the excluded
  `rustynes-cosim` crate enables. `Nes::inject_nmi` / `inject_irq` drive the
  **level functions** the CPU actually samples, and `Oracle::run_with_injection`
  steps instructions while toggling the pins.
- **`mister_source_map_audit.rs`**, pinning every citation in the new hardware
  **source map** to a file that exists. Under the firewall those pages are the
  *only* permitted sources, so a dangling citation is a behaviour with no source.
- **The v2.5.1 -> v2.7.0 programme** ([`to-dos/plans/v2.7.0-mister-core-plan.md`](to-dos/plans/v2.7.0-mister-core-plan.md)),
  four dated `ref-docs/` research files, and `to-dos/mister/`.

### Fixed

- **A hardware interrupt pushed the wrong return address.** `RTI` returned one
  byte too high. The cause was a **shared block with two writers**: the generic
  operand-fetch step advances PC at `tcyc == 1` for every addressing mode except
  three, and `AM_BRK` was not among them. `BRK` and a hardware interrupt *share*
  that mode and disagree about it -- `BRK` advances over its second byte, an
  interrupt does not -- so for `BRK` both writers assigned the same value and the
  fault was invisible. **`BRK` passing 186/186 is what kept it hidden**: the only
  opcode exercising the mode was the one on which the defect did not show.
- **The injection was wired to a dead path.** It first targeted `Bus::poll_nmi` /
  `poll_irq`, which look like the right functions and are not the ones the
  production CPU uses -- it samples `nmi_level()` / `irq_level()` every cycle and
  edge-detects itself. The oracle never took an injected NMI while the DUT always
  did. Moving it took **IRQ 0/8 to 4/8 and NMI 0/8 to 5/8**.
- **Second-to-last-cycle interrupt recognition** in the DUT, per the documented
  rule. It did not move the sweep's numbers and is in because the rule says so;
  said plainly rather than credited with a fix it did not make.
- **`mutate.sh` announced a baseline it had not captured.** Sourced from a
  non-bash shell, `BASH_SOURCE` was unset and `ROOT` resolved to `/`; the `cp`
  failed and the next line still printed "captured baseline". It now fails there,
  and the echo is joined to the copy with `&&`.

### Changed

- **ADR 0038's own gate 2a reported a false pass.** As written it piped
  `cargo expand` -- a separate binary, not installed here -- through
  `2>/dev/null | grep -c inject_`, so it counted an empty stream and printed the
  **0 it was looking for** while measuring nothing. Caught by the control, not by
  reading: the same command with the feature *enabled* also returned 0. Replaced
  with the toolchain's own expander, and the ADR now requires reading the control
  first. **Measured: off = 0, on = 17.**
- **The sweep gained its both-pins case because a mutation found the gap.**
  Inverting NMI/IRQ priority came back NOT CAUGHT: sweeping one pin at a time,
  an inverted priority is indistinguishable from a correct one. It is caught now.
- **A published finding is retracted.** The previous commit reported this core's
  interrupt sequence as **five cycles where hardware is seven**. It was seven
  throughout; two cycle numbers were differenced without checking which
  instruction each belonged to. The real fault was in the harness -- `cur_instr`
  was `0` before the first opcode fetch, so `--nmi-at-instr 0` asserted the pin
  throughout the eight-cycle reset. Retracted in place rather than deleted,
  because it was published as a defect against the RTL.
- **nestest 0-diff and the 5 M-cycle window are reclassified, not carried.** Both
  stop at a `$2002` read where *both sides address it* and only the data differs
  -- the DUT has no PPU. They are rung-3 acceptance criteria.

### Verified

`rustynes-core` changes (the feature gate, its fields and setters), so the
accuracy numbers are **verified, not asserted**: **AccuracyCoin 141/141 (100.00%,
RAM decoder)**, **nestest 0-diff**, workspace **2233 passed / 128 suites / 0
failed**. DUT side: lint 0 findings, nine opcode-group ROMs **2115 records / 0
divergences** (`opgroup8` unchanged at 186), sweep **60/60**. Seven mutations
against the sweep: five CAUGHT, two NOT CAUGHT and both explained.

## [2.5.0] - 2026-08-23 - "Rungwork" (the 6502 rung, and the two gates it cannot reach)

### Added

- **Interrupts in the DUT** (`RustyNES_MiSTer@27171cd`): `nmi_n` and `irq_n`, the
  /NMI **edge latch** and level-sampled /IRQ, `BRK`, the three vectors, `RTI`, the
  **NMI/`BRK` hijack** decided at the push rather than at entry, and **delayed-`I`**
  falling out of where the poll sits rather than needing a special case.
- **The indirect addressing modes** — `(zp,X)` and `(zp),Y` across seven operation
  groups. The last documented addressing gap.
- **`pc` compared, and agreeing on 100% of cycles** — 3551/3551. The wrapper now
  derives an instruction-scoped PC matching the oracle's definition.
- **nestest as a bounded gate** — **27,388 cycles**, 8571 instructions, no
  unimplemented opcode, matching on `pc`, `bus_addr`, `bus_data`, `bus_access`.
- **[ADR 0038](docs/adr/0038-cosim-interrupt-injection-api.md)** — a test-only,
  default-off interrupt-injection API, with six constraints and a written
  fallback if two of them fail.

### Fixed

- **`RTS` read the incremented address** on its final cycle; hardware reads the
  pulled address and increments at that cycle's end.
- **`AM_IZX`'s `default` arm caught cycle 1**, driving an effective address before
  the pointer byte had been read.
- **`AM_IZY` tested the wrong page-cross signal** (`idx_page_cross` rather than
  `izy_cross`), taking five cycles where hardware takes six.
- **`AM_IZY` wrote at the unfixed address** when the index did not carry.
- **`build()` stamped over every ROM's interrupt vectors**, so `opgroup8`'s
  handlers were unreachable — and *both sides would have agreed on the same wrong
  ROM*.
- **`cpu-gate` and `cpu-bus-gate` ran the DUT with different memory.** `RAM_INIT`
  is now required by both.

### Notes

- **Two of this release's own stated gates are structurally blocked, and neither
  is a defect.** nestest 0-diff and the 5 M-cycle bus window both need a PPU —
  nestest's first `$2002` read is where it stops, with *both sides addressing
  `$2002`* and only the data differing. That is rung 3 by design.
- **The interrupt-injection sweep has no oracle.** `rustynes-core` exposes no
  injection API; its /IRQ comes from the APU or a mapper and its /NMI from the
  PPU. So the pins, hijack and delayed-`I` are **implemented and not
  oracle-verified**; `BRK` *is* verified, since a software interrupt needs no pin.
  ADR 0038 records the decision and its conditions.
- **No upstream libretro sync.** The cadence rule is amended: the next sync waits
  for the MiSTer core to be **complete**, not for the next `vX.Y.0`.
- No crate under `rustynes-{cpu,ppu,apu,mappers,core}` changes, so **AccuracyCoin
  141/141 (100.00%, RAM decoder) and nestest 0-diff hold by construction.**

## [2.4.9] - 2026-08-23 - "Plumbline II" (the bus half of rung 2, and what it found the day it existed)

### Added

- **Rung 2's per-cycle bus comparison** (`RustyNES_MiSTer@715952b`). `make -C tb
  cpu-bus-gate` compares `bus_addr`, `bus_data` and `bus_access` against the
  oracle's `.obs.bin`. **Both mutations v2.4.8 recorded as NOT CAUGHT are caught
  by it** — the release named for the read-modify-write double write can finally
  verify one.
- **`<stem>.ram_init.bin`**, the power-on work RAM captured before a cycle runs.
  The oracle fills its 2 KiB from a seeded PRNG, so a zeroed testbench disagrees
  on every read of a location the program has not written. Exported as a golden
  rather than reimplemented in C++, where a second copy of a PRNG would drift.
- **The logical group** — `AND`, `ORA`, `EOR`, `BIT` across six addressing modes.
  Documented opcodes that were simply missing, and a hard prerequisite for the
  undocumented combinations.
- **The undocumented opcodes** — `LAX`, `SAX`, `SLO`/`RLA`/`SRE`/`RRA`/`DCP`/`ISC`,
  and the multi-byte `NOP`s. Rung 1 now stands at **seven ROMs, 1663 records**.

### Fixed

- **Indexed read-modify-write skipped its dummy read.** The RMW branch drove the
  bus only from the access cycle onward, leaving earlier cycles at `addr = pc`.
- **`STA $xxxx,X` without a page cross wrote TWICE.** The comment above the line
  said *"`we` stays low even for a store"*; the code read
  `we = d_ir.writes && !idx_page_cross`. Identical memory, cycles and registers —
  and on hardware a mapper register written twice is not one written once.

### Notes

- **`pc` is populated but deliberately not compared.** Enabling
  `cpu-instr-cycle-trace` fills it — verified observation-only first, all 900
  records differing in *those two bytes and nothing else*. But the two sides mean
  different things by it and agree on only **45%** of cycles, so it labels
  divergences instead of gating them.
- **Three tests agreed with their own mutations**, each a wrong answer coinciding
  with a right one: `SLO`'s flags, `DCP` writing `A` (twice), and one mutant that
  did not compile — reported as its own outcome, never as a catch.
- **Interrupts remain v2.5.0.** `cpu6502` has no `nmi_n`/`irq_n` pins, so
  `put_cycle`, `nmi_line` and both IRQ samples are skipped — stated on every
  successful run rather than left to documentation.
- No crate under `rustynes-{cpu,ppu,apu,mappers,core}` changes, so **AccuracyCoin
  141/141 (100.00%, RAM decoder) and nestest 0-diff hold by construction.**

## [2.4.8] - 2026-08-23 - "Palimpsest" (read-modify-write, and a gate that cannot see its own subject)

### Added

- **The 6502's read-modify-write group**, in SystemVerilog in the sibling
  repository (`RustyNES_MiSTer@0cb628f`): `ASL`, `LSR`, `ROL`, `ROR`, `INC`,
  `DEC`, across the accumulator form and four memory modes -- 28 opcodes. Rung 1
  now stands at **five ROMs, 1110 records**, matching the oracle on all seven CPU
  fields. `opgroup5` contributes 358 and every construct in it is new; the four
  earlier ROMs re-run unchanged at 147 / 140 / 286 / 179.
- **`tb/mutate.sh`**, a mutation harness that cannot promote a mutant to
  baseline: the baseline is captured once into a file it refuses to overwrite, it
  must **pass** the gate before any mutation runs, it reports three outcomes
  rather than two, and a trap restores on exit.

### Fixed

- **The mutation harness had been measuring against its own mutants.** It
  captured the pristine copy at *source* time and restored at the **start** of
  each run, so the last mutant of a batch survived and re-sourcing captured it as
  the new baseline. Two mutations were silently live in the tree while later
  results were measured against them, all reported CAUGHT for free -- and the
  gate's own first PASS had been against a stale binary.
- **A ROM gap the mutation pass found**: "zero-page indexed RMW uses a 16-bit
  add" came back NOT CAUGHT, because every zero-page-indexed access stayed below
  `$FF`, where an 8-bit add and a 16-bit one agree on every byte. `$20 + $F5` was
  added, with a sentinel at `$0115` that a straying access would disturb.

### Notes

- **The release is named for something rung 1 cannot verify.** Hardware writes
  the *unmodified* byte back before the modified one. Skipping that write, or
  emitting the modified value instead, changes no register, flag, final memory
  content or cycle count -- and `CpuBootTrace` carries exactly those. Both
  mutations come back **NOT CAUGHT**, and that is recorded rather than glossed.
  It matters on hardware: the middle write reaches mapper registers and I/O, and
  writing twice to `$2007` is not writing once.
- **Rung 2's bus half is scoped for v2.4.9**, beside the undocumented opcodes,
  which is where those two become verifiable. The compare surface already exists
  on both sides (`Observable`, byte-identical encoding, selftested), so the work
  is a trace writer rather than a new format.
- **RMW is a flag on the existing addressing modes, not four new ones** -- four
  would have taken `am_e` from 14 values to 18 and overflowed its four bits, the
  exact defect v2.4.7 hit on `op_e`.
- Written from public documentation only. No reference NES core was opened; none
  is present in the tree.
- No crate under `rustynes-{cpu,ppu,apu,mappers,core}` changes, so **AccuracyCoin
  141/141 (100.00%, RAM decoder) and nestest 0-diff hold by construction.**

## [2.4.7] - 2026-08-23 - "Keystone" (the stack closes, and a dead line proves itself dead)

### Added

- **The 6502's stack group, `JSR`/`RTS`/`RTI`, and `JMP` in both forms**, in
  SystemVerilog in the sibling repository (`RustyNES_MiSTer@3560c98`). Four ROMs,
  **752 records**, matching the oracle on all seven CPU fields, with seven
  mutations demonstrated to break the gate. `opgroup4` contributes 179 records and
  every construct in it is new to this release; the three earlier ROMs are re-run
  unchanged at 147 / 140 / 286.
- **`JMP ($xxFF)` reproduces the hardware page-boundary bug.** The high byte comes
  from `$C100`, not `$C200`. The ROM places a vector at `$C1FF` and a *distinct*
  sentinel at `$C200`, so an implementation that "corrects" the bug with a 16-bit
  increment lands somewhere visibly wrong rather than somewhere plausible.
- **A third rule in `release_state_prose_audit`** — a superseded release called
  "the current tag" in narrative prose. This is the fourth location the project's
  release-state drift has occupied, each move following the previous one being
  gated.

### Fixed

- **`JMP` indirect read the wrong pointer**, found by the oracle before any
  mutation was tried: `JMP ($C090)` reached `$77A0`. Cycle 3 latched the fetched
  vector low byte into `adl`, destroying the pointer low byte, so cycle 4 computed
  `{adh, vector_low + 1}`. A third latch fixes it. The divergence named the
  instruction and the wrong value said which byte had been read — the rung working
  as designed.
- **`cpu-gate` hardcoded `--cycles 301` for every ROM**, so the documented accuracy
  command compared only the first 136 of `opgroup3`'s 286 records. It failed closed,
  so no false pass was possible, but the two most recent ROMs could not be gated by
  the published invocation. Each window now lives in one variable both targets read.
- **The rung-1 record table did not add up** — 573 claimed above rows summing to
  455, because `opgroup1` was carrying the count from the 0..64 window v2.4.4 closed
  over beside two counts measured under the current windows. All four are now
  **measured**, not carried forward.
- **A superseded release was called "the current tag"** in `to-dos/ROADMAP.md`, six
  releases stale.

### Changed

- **`op_e` widened to six bits.** Verilator rejected the 33rd value in a five-bit
  enum. The rejection is the good outcome: a silently-wrapped enum decodes one
  opcode as another with no diagnostic anywhere.
- **A `JSR` arm was dead code, and a mutation is what proved it.** The mutation
  targeted `store_val`, which the `AM_JSR` bus block never reads — it drives `dout`
  directly. A mutation that cannot fail is evidence about the code it mutated, not
  about the test. Removed; re-run against the real site, it diverges.

### Notes

- **The oracle/DUT window pairing is off by one and now documented.** The oracle's
  `--boot-trace 0..N` yields N records where the harness's `--cycles N` yields N−1.
  Get it wrong and the diff reports a one-record length mismatch indistinguishable,
  at a glance, from the CPU halting a cycle early.
- Written from public documentation only — the NESdev wiki's 6502 cycle-time,
  instruction and addressing-mode pages, and this project's own `docs/cpu-6502.md`.
  No reference NES core was opened; none is present in the tree.
- No crate under `rustynes-{cpu,ppu,apu,mappers,core}` changes, so **AccuracyCoin
  141/141 (100.00%, RAM decoder) and nestest 0-diff hold by construction.**

## [2.4.6] - 2026-08-22 - "Abacus" (the core learns arithmetic, and what overflow actually means)

### Added

- **Indexed addressing, `ADC`/`SBC`, and the compare group.** (v2.4.6, continuing the "Fabric" line. RTL pinned at `RustyNES_MiSTer@26d0fd9`.) Three indexed modes — `zp,X`/`zp,Y`, `abs,X`, `abs,Y` — with their page-cross penalty; `ADC` and `SBC` across six addressing modes each; and `CMP`/`CPX`/`CPY`. **The emulation core is untouched.**

  Three ROMs, **573 records**, matching the oracle on all seven CPU fields, with seven independent mutations demonstrated to break it:

  | ROM | records | covers |
  |---|---|---|
  | `opgroup1` | 147/147 | v2.4.4's reset and implied group, unchanged |
  | `opgroup2` | 140/140 | v2.4.5's addressing modes, loads, stores, branches, unchanged |
  | `opgroup3` | **286/286** | every construct new in this release |

- **Zero-page indexing wraps inside page zero.** The add is 8-bit, so `$FE + $05` is `$0003` and not `$0103`. An implementation that indexes with a 16-bit add is wrong *only* for programs that index past `$FF` — exactly the kind of defect that survives casual testing — so the ROM does it deliberately and reads the result back through a *different* addressing mode.

- **Absolute indexing pays for its page cross, and a write pays always.** A read whose low-byte add did not carry takes four cycles; one that carried takes five. A **write takes five regardless**, because the CPU has already driven the unfixed address and must not write there. An implementation that lets a store take the read fast path agrees on every register and differs only on `cycle` — which is why the trace compares it.

- **`ADC` and `SBC` share one adder.** `SBC` feeds it `~M`, which is why the 6502 has no separate subtractor and why `SBC`'s carry means *no borrow*. Overflow is **signed** overflow — set when the operands share a sign and the result does not, `(~(a ^ m) & (a ^ r))[7]` — and the ROM covers `$7F + $01` (overflow, no carry), `$80 + $FF` (both), and `$10 + $10` (neither). One case cannot separate "V computed from carry" from "V stuck high"; three can.

- **A compare is a subtract whose result is discarded.** The register is not written and **V is not touched**: a compare has no signed-overflow meaning, and setting it there is a common quiet error. The ROM stores `A` after the compare sequence and reads it back to prove it survived.

### Notes

- **Every case in the new ROM is built so the wrong answer differs from the right one**, and that is now the default rather than a correction. v2.4.4 and v2.4.5 each shipped a test whose wrong answer coincided with the right one — `TXS` after `TSX` computing the flags already present, and a store/load pair in the same addressing mode that round-trips through *any* wrong address — and both came back NOT CAUGHT under mutation.

- **The instruction-set asymmetry is recorded because it reads like a typo.** `LDX`/`STX` index by Y; `LDY`/`STY` index by X. An index register cannot index itself, so there is no `LDX zp,X`. The decode says so at the site.

- Mutations, all caught: indexed **write** taking the read fast path; zero-page index not wrapping in page zero; V computed from carry rather than signed overflow; `SBC` not inverting its operand; `ADC` ignoring carry-in; compare setting carry inverted; and `zp,X` omitting its un-indexed dummy read.

## [2.4.5] - 2026-08-22 - "Compass" (the core reaches memory, and chooses)

### Added

- **Three addressing modes, the load/store group, and all eight branches.**
  (v2.4.5, continuing the "Fabric" line. RTL pinned at
  `RustyNES_MiSTer@b01a656`.) Immediate, zero page and absolute;
  `LDA`/`LDX`/`LDY` and `STA`/`STX`/`STY` across them; `BPL`/`BMI`/`BVC`/`BVS`/
  `BCC`/`BCS`/`BNE`/`BEQ`. **The emulation core is untouched.**

  Two ROMs, **287 records**, matching the oracle on all seven CPU fields:

  | ROM | records | what it covers |
  |---|---|---|
  | `opgroup1` | 147/147 | v2.4.4's reset and implied group — unchanged by the rewrite |
  | `opgroup2` | 140/140 | every construct new in this release |

  The ordering is not arbitrary. **Loads are what finally let a test program put
  an arbitrary value in a register** — until now `A`, `X` and `Y` could only hold
  what power-on and increments reached from zero. **Stores are the first writes
  this core has ever performed.** And **branches are the first opcodes to read a
  flag**, which retires the dated lint waiver `p` carried since v2.4.4 — the
  outcome a dated waiver exists for.

- **A page-crossing branch, placed deliberately rather than stumbled into.** An
  `ORG` directive exists in the ROM assembler for exactly one test: at `$C0FC`
  the next instruction is `$C0FE`, and `+$10` lands at `$C10E` — a different
  page, so the taken branch costs **four** cycles instead of three. An
  implementation that fixes the high byte without spending the cycle agrees on
  every register and disagrees only on `cycle`, which is why the trace compares
  it.

### Fixed

- **Write intent was sampled after the clock edge.** `we` and `dout` are
  combinational functions of `state`/`tcyc`, and those change **on** the posedge
  — so reading them afterwards reads the *next* cycle's plan. Every store
  silently did nothing, and the symptom surfaced at a later `LDA`, several
  instructions after the cycle that was actually wrong.

### Notes

- **Never read RAM a test program has not written.** The oracle powers on with
  deterministic **seeded** work RAM; a flat-memory testbench starts at zero. A
  read of an untouched address therefore diverges for a reason that has nothing
  to do with the CPU — observed as the oracle returning `$36` from `$0003` where
  the harness returned `$00`, which read at first as "the absolute store is
  broken". Test ROMs now write a sentinel before reading.

- **A read-back in the same addressing mode tests round-tripping, not
  addressing.** `STA $10` followed by `LDA $10` is self-consistent under any
  address mutation: send both to `$0110` and it still passes. Two mutants — zero
  page to the wrong page, and absolute with its address bytes swapped — came
  back **NOT CAUGHT**. Each mode is now cross-checked by a *different* mode, and
  the ROM says so at the site.

  That is the third time in two releases a mutation exposed a test that read
  correctly and verified nothing.

- **A mutant that does not compile is not a catch.** Forcing `br_fixup = 1'b0`
  left `br_sum[8]` and `adl[7]` unused, so Verilator refused to build it and the
  run reported a build failure where a naive harness would have counted a catch.
  Inverting the condition keeps both signals live and tests the same property.

- **One decoder, called twice.** v2.4.4 had two duplicated `case` blocks — over
  `din` at fetch and over `ir` during execution — which had to agree by hand
  across seventeen arms. They did, and would not have kept doing so: a mode added
  to one and not the other is an instruction that decodes differently depending
  on when you look. A function returning a packed struct removes the possibility
  rather than documenting it.

- The branch condition is expressed as the 6502's own encoding — opcode bits
  `7:6` select the flag, bit `5` the sense — rather than eight arms, because
  eight arms is eight chances to invert one.

## [2.4.4] - 2026-08-22 - "Ignition" (the first RTL, and the reset sequence that corrected our own spec)

### Added

- **The 6502's reset sequence and the single-byte implied group, in
  SystemVerilog.** (v2.4.4, continuing the "Fabric" line. RTL lives in the
  sibling repository, pinned at `RustyNES_MiSTer@7f092bd`.) The first real RTL
  of the programme, unblocked because v2.4.3 settled both risks the plan
  required answered before any existed: the subset is fitted and the licence is
  GPL-3.0-or-later. **The emulation core is untouched.**

  Reset plus seventeen implied opcodes match the oracle on all seven CPU fields:

  ```text
  All 29 aligned records match under the chosen comparator
    (skip-fields: ["scanline", "dot", "frame", "flags"])
  ```

  **The DUT is the third writer of the oracle's `CpuBootTrace` format**, after
  the oracle itself and `scripts/mesen2_cpu_boot_trace.lua`, and
  `cpu_boot_trace_diff` reads it with **no modification**. `--skip-fields`
  already existed, so this rung needed no oracle-side change at all — which is
  what replay-rather-than-lockstep was chosen for.

- **The oracle settled a question our own documentation could not.** Reset is
  **eight cycles, not seven**: the oracle's first record carries cycle 8, so
  reset occupies 0..7. `docs/cpu-6502.md` says **both** — *"Reset is a special
  7-cycle sequence"* in its Reset section and *"runs its 8 cycles"* in the
  v2.0.0 note. The first draft implemented seven, from the prose.

  Prose that contradicts itself cannot be the spec; an executable oracle can.
  This is the failure mode the whole programme is built around, arriving on the
  very first opcode group.

### Fixed

- **`docs/cpu-6502.md`'s Reset section**, which said seven where the shipped
  model does eight. Corrected rather than left as a second, wrong statement
  beside the right one.

### Notes

- **A mutation the test ROM was built to catch came back NOT CAUGHT, and the
  fault was the ROM.** `TSX` leaves `X = $FD`, so `N=1, Z=0`. A `TXS` that
  wrongly computes flags from `X` computes `N=1, Z=0` — *the values already
  there*. The wrong answer coincided with the right one and the test was
  silently inert. Two instructions now land `Y` at `$00` with `Z=1, N=0` before
  `TXS`; `mkrom.py` says so at the site, because the pair looks like filler and
  deleting it disarms the test with no symptom.

- **A harness bug made every mutation report a catch, including the baseline.**
  `cargo run` was invoked from the wrong repository and failed identically every
  time. Only the baseline control made it visible — without it, four failed
  invocations would have read as four successes.

- **ADR 0037 nearly eroded without a symptom.** The first draft put observation
  ports on the *synthesisable* module. Unconnected outputs optimise away
  identically, so nothing visibly breaks — which is precisely how that rule
  erodes. Observation now lives in `tb/cpu6502_cosim.sv`, reached by
  hierarchical reference, and the synthesisable module is exactly what Quartus
  will see.

- Unimplemented opcodes **halt** rather than behaving as `NOP`: a silent
  two-cycle NOP would desynchronise the trace and report the divergence far from
  its cause.

## [2.4.3] - 2026-08-22 - "Touchstone" (what the synthesiser accepts, and what the licence requires)

### Added

- **The RTL subset is FITTED rather than documented — Fabric risk 4 is closed.**
  (v2.4.3, continuing the "Fabric" line.) Quartus Prime Lite **17.0.2 Build 602**
  is installed and `rtl/kitchen_sink.sv` in the sibling `RustyNES_MiSTer`
  repository has been through `quartus_map` + `quartus_fit` to a
  placed-and-routed netlist on a **5CSEBA6U23I7** — the DE10-Nano and
  SuperStation One part. The emulation core is untouched.

  The plan called this the risk whose failure is **retroactive**: Quartus 17.0.2's
  SystemVerilog subset is materially narrower than Verilator's, and a construct
  rejected after ten thousand lines exist invalidates not one file but the style
  every file was written in. The acceptance criterion was never "it compiles" —
  it was a fitted netlist **and its resource report**, because only the second
  catches a 2 KiB memory that became 16,384 flip-flops.

  ```text
  Analysis & Synthesis was successful. 0 errors, 0 warnings
  Fitter was successful.               0 errors, 4 warnings

  Total block memory bits  ; 16,384 / 5,662,720 ( < 1 % )
  M10K blocks              ; 2 / 553
  Total registers          ; 29
  Logic utilization (ALMs) ; 17 / 41,910 ( < 1 % )
  ```

  **29 registers, not 16,413.** The 2 KiB `logic [7:0] mem [0:2047]` inferred as
  two M10K blocks from the *source style* alone — no `ramstyle` attribute, no
  synthesis pragma. That failure mode is silent: Quartus emits a log warning and
  the build still succeeds, so an accidentally-registered memory fits on its own
  and only makes the design unfittable in aggregate.

  Two findings the fit was not looking for. The **MIF column is populated**
  (`altsyncram ... Simple Dual Port ; 2048 ; 8 ; 16384 ; db/kitchen_sink.ram0_...`),
  so the `initial` block became a real memory-initialisation file — which is what
  makes a boot ROM land *inside* the block rather than being written at runtime.
  And the `enum` was recognised as a state machine and **one-hot encoded**, not
  flattened into anonymous logic. Zero synthesis warnings is the stronger half of
  that first line: not merely "no errors", but nothing to say about any construct
  present.

  **Nine constructs are promoted to `fitted`; three are deliberately not.**
  `kitchen_sink.sv` does not exercise plain `case`, `priority case` or `$bits`,
  so those keep the *documented* status while their neighbours advance. Plain
  `case` is near-certainly fine and `$bits` is an elaboration-time function — and
  "near-certainly" is the word that document exists to refuse. No permitted
  construct failed, and nothing moved to the forbidden table.

- **The `sys/` licence audit inverts the plan's own hedge — Fabric risk 1 is
  closed.** The plan required this **before any RTL is written**, because
  relicensing after ten thousand lines exist is precisely the failure
  `docs/originality-and-provenance.md` documents. Every file under
  `Template_MiSTer`'s `sys/` was read and classified by its own grant, with
  comment markers stripped and whitespace collapsed before matching.

  | Classification | Files |
  |---|---|
  | GPL-2.0-or-later | 9 |
  | **GPL-3.0-or-later** | **4** |
  | GPL by reference, no version stated | 4 |
  | Copyright, no grant | 4 |
  | No header | 36 |
  | **GPL-2.0-only** | **0** |
  | | **57 total** |

  The plan hedged that a GPL-2.0-**only** file would force the RTL *down* to
  GPL-2.0-or-later. **There is no such file, and the constraint runs the other
  way.** Four files are GPL-3.0-or-later — `ddr_svc.sv`, `hps_io.sv`,
  `scandoubler.v`, `sd_card.sv` — and `hps_io.sv` is **not optional**: it is how
  a MiSTer core receives a ROM from the HPS and how the OSD reaches it. No core
  functions without it. GPL-2.0-or-later may be combined into a GPL-3 work
  because "or later" permits the upgrade; GPL-3.0-or-later cannot be reduced to
  GPL-2. So the combined bitstream **must** be GPL-3.0-or-later — which is
  already RustyNES's licence. No relicensing is needed, and the hedge is
  inverted by evidence rather than by argument.

### Fixed

- **A stale success message in the RTL-subset policy checker.** `check_rtl_subset.py`
  printed "the subset itself is unproven until `quartus/synth.sh` produces a
  fitted netlist" on every green run. That was true when written and is now
  false, and a success message that describes a state the project has left is
  how a check quietly stops meaning anything. It now names the fit date and says
  that a construct added *since* then is what remains unproven.

### Notes

- **The Quartus install is recorded because it is reusable, and three of its
  findings would each have cost an afternoon.** The
  `Quartus-lite-17.0.2.602-linux.tar` bundle is **two-stage** — base 17.0.0.595
  installers plus a separate `QuartusSetup-17.0.2.602-linux.run` update — and the
  bundle's own `setup.sh` runs **only the base**, its last line being an `exec` on
  the 17.0.0 installer. A naive install therefore lands on **17.0.0**, which
  `synth.sh`'s `*17.0*` pattern would have waved through; the install script
  asserts `Version 17.0.2` exactly. 17.0 has **no `--accept_eula` flag** (it
  arrived in 17.1, and an unknown flag is a hard error to a BitRock installer, not
  a warning). And `--disable-components` accepts **device families by name**, so
  the install is scoped to Cyclone V and skips ModelSim, the GUI help and five
  unused families — **11 GB rather than roughly 30**.

- **17.0.2 specifically, and the reason is not device support.** Quartus Prime
  Lite **25.1 does still support Cyclone V** — Lite is the only edition that does.
  Three other things rule it out: `Template_MiSTer` pins v17.0.x in writing;
  `sys/` is mandated verbatim and carries Platform Designer IP generated under
  17.0, which a newer Quartus compiles only after a **one-way IP upgrade**; and a
  newer Quartus is the *more permissive* tool, so a pass under 25.1 would report
  success for a property it never tested.

## [2.4.2] - 2026-08-22 - "Cairn" (checkpoints, and what a device can actually observe)

### Added

- **Rolling per-cycle hash checkpoints — the rung-0 comparison surface.**
  (v2.4.2, continuing the "Fabric" line.) `crates/rustynes-cosim/src/checkpoint.rs`,
  a `checkpoint_diff` CLI, an `rn_write_checkpoints` C ABI entry point, and a
  `<stem>.ckpt.bin` golden. The emulation core is untouched.

  The constraint nobody budgets for in co-simulation is trace *volume*, not
  simulation time, and the figure is now **measured rather than projected**:
  3 frames of AccuracyCoin is 89,343 CPU cycles, which is **5,372,427 bytes** of
  `irq.csv` against **352 bytes** of `ckpt.bin` — a factor of **15,263**. So both
  sides chain a hash over the per-cycle tuple and compare checkpoints; the first
  mismatch names a 4096-cycle window, and only that window is re-run with full
  capture.

  **What is hashed is a decision about hardware, not about convenience.**
  `CycleRecord` carries 29 fields and most of them are `RustyNES`'s *model* —
  `dmc_abort_delay_post`, `apu_phase_post`, `dma_cycles_owed`. Gating on them
  would force an independent implementation to transliterate a Rust data
  structure, which is bad hardware and, on a programme built on never reading a
  reference implementation, an odd form of self-derivation. `Observable` is the
  subset a device-under-test can genuinely produce, `from_cycle_record` is the
  single place the partition is applied, and a test perturbs **every** dropped
  field at once and asserts the hash does not move — with its converse, so it
  cannot pass by dropping everything.

  Two subsets needed their caveats stated rather than buried. **The IRQ line is
  one wire**: `CycleRecord` attributes each sample to the mapper or the APU,
  hardware has a single wire-OR'd /IRQ pin that cannot, so the pairs are OR'd
  before hashing — hashing them apart would fail a correct DUT for disagreeing
  about something it cannot observe. And **`pc` is DUT-observable, not
  pin-observable**; it is in because the testbench wrapper can expose the
  register, but a `pc`-only mismatch means something weaker than a bus mismatch.
  `a12_events` is excluded for **scope**, not observability — A12 transitions
  genuinely are visible on the cartridge connector — and becomes a gate when the
  PPU rung opens.

  **The hash is FNV-1a 64 for exactly one reason: a C++ testbench can
  reimplement it without a library.** The top risk at this rung is a
  format-packing mismatch masquerading as an RTL bug, so `encode` fixes a
  16-byte little-endian layout with an explicit zero pad byte — the C++ side
  cannot hash uninitialised struct padding — and both layout and hash are pinned
  to a hardcoded vector. A reordered field fails that test rather than producing
  a phantom RTL defect.

  **Three answers, and the third is the point.** `checkpoint_diff` exits `0`
  identical, `1` diverged with the window printed, and **`3` inconclusive**. A
  truncated run, a DUT that stopped early, and two runs at different intervals
  all produce "no divergence was found"; reporting that as agreement is this
  project's recurring failure. Cycle **alignment is checked before the hash**,
  because two streams checkpointing at different cycles cover different spans, so
  calling their difference a divergence would send a re-run at a window where
  nothing is wrong. All three answers are demonstrated on real exported output,
  not only in unit tests.

  Two hazards found while building it, both pinned rather than worked around.
  `IrqTrace::push` **silently drops** records at capacity behind an `overflow`
  counter nobody has to read, so a hash over an overflowed trace covers fewer
  cycles than it claims and would be blamed on the DUT — `take_checkpoints` now
  refuses with the capacity to retry with, and the exporter aborts rather than
  writing a short stream. And `Bus::take_irq_trace` **moves** the trace out, so
  asking for the CSV and then the checkpoints returns `None` for whichever came
  second, which is indistinguishable from "never armed";
  `Oracle::take_irq_artifacts` derives both from one take.

- **The v2.4.2 acceptance gate, executable: the hash checkpoints agree with
  full capture.** Checkpoints are an *approximation* of "where do these two runs
  first differ", traded for four orders of magnitude of disk. The whole scheme
  is worthless if the approximation can disagree with the answer, so
  `first_full_capture_difference` computes the answer directly and
  `localisation_is_consistent` states the contract the approximation must honour
  — as a function rather than as prose in a plan.

  The contract is narrow on purpose, because a looser reading is satisfiable by
  a broken implementation. Identical streams must report `Identical` (a **false
  positive** gets a gate switched off). A real difference must never report
  `Identical` (a **false negative** passes a wrong DUT). And when it reports a
  divergence, **the named window must contain the difference** — a report naming
  the wrong window sends a full-capture re-run somewhere nothing is wrong,
  spends the debugging budget, and returns "no problem here", which reads as
  evidence the DUT is fine. `Inconclusive` is acceptable for a real difference
  and never for identical streams.

  A sweep drives **331 cases**: every run length around the interval boundary
  (1, 2, 4095, 4096, 4097, 8192, 8193, 10 000, 12 288), a corruption at every
  position for short runs and a randomised sweep for long ones, and a different
  observable field perturbed each time so it is not silently exercising one
  field. Both the gate predicate and the sweep are demonstrated to fail — a
  one-character mutation to `Divergence::contains` reddens two tests.

- **A divergence at cycle zero was reported in a window that did not contain
  it**, found by that sweep at `len = 1` — the degenerate case a hand-written
  test set omits. `Divergence::after_cycle` was a `u64` in which `0` meant both
  "no prior checkpoint" and "cycle zero", so the first window read as `(0, 0]`,
  which is empty. A full-capture re-run of it would have found nothing, and
  "nothing found" reads as evidence the DUT is fine.

  `after_cycle` is now `Option<u64>`, which removes the sentinel collision
  rather than special-casing it, and `Divergence::contains` is offered so call
  sites do not reimplement a boundary that is half-open at one end and
  open-ended at the other. `window_len` returns `Option<u64>`: for the first
  window the span begins wherever the run began, and a checkpoint stream carries
  no evidence that it began at cycle 0 — so the honest answer is "unknown", not
  an assumed `through_cycle + 1`. `checkpoint_diff` prints that window
  open-ended rather than as `(0, N]`.

- **`<stem>.obs.bin`, the full-capture observable golden — because the CSV
  cannot re-derive the checkpoints.** Found by trying to build the rung-0
  self-diff on the CSV: `irq.csv` carries **23 columns** and neither `pc` nor
  `put_cycle_post` is among them, so two of the nine observable fields are
  simply absent from it. An external testbench reading the CSV therefore cannot
  reproduce the checkpoint hashes, and "feed `RustyNES`'s golden back in as if
  it were the DUT and get zero divergences" — the rung-0 gate — was not
  implementable as designed.

  The new golden is repeated 16-byte records in the **same wire encoding the
  hash folds**, headerless. It is the only artifact the checkpoints can be
  independently re-derived from, and it is also the input a re-run of a located
  window consumes, so it would have been needed regardless. Additive: the CSV is
  untouched, which matters because `scripts/irq_trace_cross_diff.py` and the
  committed `golden/irq_trace/*.csv` both depend on its shape.

  `Observable::decode` is the inverse and **refuses what it does not
  understand** — a non-zero reserved pad byte, an undefined flag bit, an unknown
  bus-access code, a short record, a stream length that is not a multiple of 16.
  Reading a record from a newer producer as though nothing had changed is how a
  *format* divergence gets reported as a *DUT* divergence. The stream is emitted
  even when the checkpoints are refused for overflow: a hash over a truncated
  trace claims a coverage it does not have, while the records themselves are
  just records.

  Measured across the repository boundary, not only in unit tests: **89,335
  records** of AccuracyCoin, re-derived in C++ from `.obs.bin` alone, hashing to
  byte-identical checkpoints — and a one-bit corruption at the halfway record
  located to the 4096-cycle window containing it, in the same invocation,
  because a positive control alone is satisfiable by a comparison that always
  agrees.

- **The `CpuBootTrace` wire layout is pinned to a literal, on both sides of the
  co-simulation boundary.** The testbench writes this format so
  `cpu_boot_trace_diff` — already written, already tested, and already used
  against a third-party reference emulator — reads a device-under-test's output
  with **no modification at all**. Demonstrated rather than asserted: a
  C++-written 256-record trace loads and reports "All 256 aligned records match",
  and a corrupted register is located at `cyc=307 PC=$C064 A=$64` vs `A=$9B`.

  Both sides are anchored to the **same hardcoded bytes** rather than to each
  other, so a drift on either fails its own test instead of the two quietly
  agreeing on something wrong — or disagreeing at co-simulation time, where a
  format difference is indistinguishable from a DUT defect.

  The pinned record uses **`scanline = -1`** deliberately: the pre-render line is
  negative, and a writer that clamped or saturated rather than writing two's
  complement would pass every test that only ever used a positive scanline.

- **Ordinary status prose is now gated too — `release_state_prose_audit.rs`.**
  `release_anchor_audit` pins 15 fixed markers and fails closed when one goes
  missing, which is the right shape for *the* canonical current-release
  statement in each document and is why a doubled bold marker on this cut failed
  loudly rather than silently checking 14 of 15. It cannot cover prose that has
  no marker, and review found that **the drift did not stop when the anchors
  were gated; it moved into the prose beside them**: `to-dos/ROADMAP.md`
  announced *"Next up — v2.4.0 Concordance"* for a release that shipped inside <!-- release-state: not-a-claim -->
  v2.4.1 and is deliberately never tagged — in the very change documenting why
  it has no tag — still read *"In development — the v2.0.0 tag itself"* for a <!-- release-state: not-a-claim -->
  tag pushed 2026-07-03, and `to-dos/README.md` read *"In development — v1.8.9"*, <!-- release-state: not-a-claim -->
  roughly fifteen releases stale, which nothing had flagged at all.

  So this is a **pattern** check rather than a marker list: a status label
  (`Next up`, `In development`, `Planned for`, `Upcoming`) naming a version at or
  below the workspace version is a contradiction that needs no judgement to
  detect. Discovery is `git ls-files` intersected with `.markdownlintignore` —
  tracked files *are* the definition of this project's own content (the vendored
  `nesdev_wiki/` beside the checkout is 2,655 untracked markdown files), and the
  frozen-tree list is read from the file that already encodes it rather than
  duplicated into a second copy that could drift.

  **A companion rule was drafted and rejected on measurement.** "A `latest
  release` or `the current line` claim must name the workspace version" fires on
  `docs/ios.md`, which correctly says *"The current line is v1.9.9 'Workshop'"* —
  scoped to the **iOS train**, not the project release. The phrase legitimately
  scopes to a platform line, so the rule cannot separate a stale claim from a
  correct one without judgement, and a gate with false positives gets switched
  off. Recorded as a measured rejection rather than shipped and tuned.

  **Mutation testing changed the design.** The escape hatch was first any line
  containing the word "historical" — and the very first line written against it
  said "this paragraph is a historical snapshot" for unrelated reasons, so a
  mutation reintroducing the exact defect the gate exists for came back **NOT
  CAUGHT**, silently exempted by the prose beside it. The marker is now
  `<!-- release-state: not-a-claim -->`, which cannot be claimed by accident. The rename came from the gate failing on its own documentation: explaining the defect requires quoting it, and a quotation is not a historical snapshot — so the marker says the line is not making a claim, which is true of both.
  Five mutations: the bare defect and the incidentally-worded one both fail, the
  explicit marker exempts, a future version does not trip it, and inverting the
  predicate renders the gate inert — proving the comparison is load-bearing.

### Fixed

- **The excluded crate's lockfile was silently gitignored, so CI re-resolved it
  on every run.** `.gitignore` carries a bare `Cargo.lock` — which matches at any
  depth — paired with a `!/Cargo.lock` re-include naming only the workspace root.
  That was written when there was exactly one lockfile. Excluding
  `rustynes-cosim` from the workspace in v2.4.1 gave it its own resolve and its
  own lockfile, which the bare rule then ignored.

  It matters more here than for an ordinary crate: this crate emits the goldens
  an external NES implementation is verified against, and its manifest records
  the emulator version rather than the dependency resolve — so a dependency
  moving underneath it would be invisible in exactly the artifact whose job is to
  establish provenance. The lockfile is now committed, and
  `cosim_manifest_audit.rs` asserts it is **tracked** rather than merely present
  (demonstrated to fail by un-tracking it and re-running).

## [2.4.1] - 2026-08-20 - "Fabric" (RustyNES as the oracle a new implementation is verified against)

This release also carries **v2.4.0 "Concordance"**, which merged to `main` and was never
tagged; entries below marked *(v2.4.0 item)* belong to it. Two further entries — the
standing release-anchor audit and the deferred-backlog sweep — belong to neither, having
landed between the two, and are called out where they appear.

### Added

- **`rustynes-cosim` — RustyNES as a co-simulation oracle for an FPGA
  device-under-test.**
  **The crate is excluded from the workspace, and that is the load-bearing
  detail.** It enables `cpu-boot-trace` and `irq-timing-trace` on
  `rustynes-core`, and cargo unifies features across a workspace build — so as a
  member it made `cargo build --workspace` compile the core **once** with the
  union. Measured through `--message-format=json`:
  `['cpu-boot-trace', 'debug-hooks', 'default', 'hd-pack', 'irq-timing-trace', 'std']`.

  That is not a performance footnote. `irq-timing-trace` selects a **different**
  `for sub_dot in 0..3` loop in `Bus::tick_one_cpu_cycle`, so CI's
  `cargo test --workspace --release --features test-roms` — the accuracy battery —
  was validating a scheduler no user runs. The same shape as the v2.3.4 defect
  where the coverage harness tested a load path no user runs. The measured cost is
  **+1.2% to +1.9%** on `full_frame`, *below* this project's own 3% bar, which is
  worth stating precisely because it shows the percentage was never the reason.

  Exclusion has a price: an excluded package cannot use `field.workspace = true`,
  so its version, edition, rust-version, license and lint tables are duplicated,
  and `--workspace` no longer formats, lints or tests it. Both halves are
  mechanically closed rather than trusted — `cosim_manifest_audit.rs` asserts every
  duplicated value still equals the workspace's **and** that the crate is still in
  `exclude` (four mutations, all caught), and CI gains explicit `fmt`, `clippy` and
  `test` steps for it. The clippy step earned its place on its first run, reporting
  a `must_use_candidate` that `--workspace` had never surfaced.
 (v2.4.1, opening the "Fabric" line; ADR 0037.) A new
  additive crate, absent from the default build, that exposes the emulator through
  a narrow C ABI a Verilator testbench can link, plus a `nes_golden_export` CLI
  that emits the five golden formats an external NES implementation is compared
  against. Spec: `docs/mister.md`. The emulation core is untouched.

  **RustyNES is not being ported to FPGA and cannot be.** A MiSTer core is
  SystemVerilog compiled into a Cyclone V bitstream. What the line builds is a new
  NES implementation written from public hardware documentation, in a sibling
  repository, with RustyNES as its *verification oracle* — the one role it is
  uniquely equipped for. The reference firewall extends to HDL accordingly:
  `NES_MiSTer` and `fpganes` `rtl/` are strict black boxes.

  Building it immediately found two things the crate was not looking for.

  **No CI invocation had ever enabled `cpu-boot-trace` or `irq-timing-trace` for
  clippy**, so those two `rustynes-core` modules had never passed the lint gate.
  Turning them on surfaced six pre-existing findings. This crate now enables both
  *unconditionally* rather than re-exposing them as its own optional features — a
  build without them would compile, link, run, and export **empty** goldens, an
  absence of signal that reads exactly like agreement.

  And **the first `run_frame()` after power-on advances nothing at all.** The PPU
  is constructed at dot 340 of the pre-render line, so the 7-cycle reset sequence
  ticks past the frame wrap and leaves `frame_complete` latched; the first call
  consumes the latch and returns having stepped zero cycles. Measured, not
  inferred. Every other caller in the workspace runs thousands of frames, so one
  lost frame is invisible to them — but a bare `for _ in 0..n` loop would have
  emitted an (n−1)-frame golden under a manifest claiming n, which is a provenance
  record wrong in the one direction that matters. `Oracle::advance_frames` gates on
  the **frame counter** instead, and the quirk is pinned by a test that names it,
  so a future core change that removes it fails loudly rather than silently
  altering every golden's length.

- **One atomic, durable file write for every path that persists user data.**
  (v2.4.0 item C.) The seven-property write sequence v2.3.9 built for
  `Config::save_to` is extracted into `crate::atomic_write` and adopted everywhere.
  The plan named three call sites; there were **four**, and the fourth is the
  instructive one.

  `save_state.rs` matters most and was named last: **a truncated save state is a
  user's game progress**, a worse loss than a truncated config, and it was still
  using the bare `fs::write` the config path had already been fixed for. It is also
  the path most likely to be written under load — rewind capture, run-ahead and
  netplay rollback all produce save states.

  `per_game.rs` was not in the plan at all, because it *looks* correct: it writes a
  sibling temp file and renames, so a sweep for `fs::write`-onto-a-target clears
  it. It held two of seven. No `fsync`, so the rename could commit a directory
  entry pointing at bytes that never reached the medium; and a **fixed** scratch
  name shared across every process and concurrent call — the exact failure the
  mechanism exists to prevent, reintroduced by the mechanism. A partially-correct
  implementation is harder to spot than an absent one.

  The config path **gains** something it never had: a bounded retry past a
  transient Windows sharing violation. `MoveFileEx` fails if another process has
  the target open, and an antivirus scanner or search indexer reading `config.toml`
  is enough. POSIX has no such constraint, which is why it went unnoticed — and why
  it would have surfaced as a Windows user reporting a save that failed for no
  visible reason. When the attempts are exhausted the error **propagates**.

  **Review then found three more places the module reported success it had not
  earned**, and the shape was the same each time: an error discarded at a call
  site, under a comment explaining the *rest* of the operation. `set_permissions`
  was swallowed — and the mode being applied is the mode the target **already
  had**, so a failure replaces a 0600 file with one at the umask default, *wider
  than what it replaced*, and says nothing. The parent-directory `sync_all` was
  swallowed along with the `File::open` that fed it, so the entire durability
  barrier could be a no-op while the module's own table claimed "yes" for Unix;
  `EIO` — the exact condition the sync exists to detect — was reported as success.
  Both now propagate, the sync excepting only the two errnos that mean *this
  filesystem does not offer the barrier* (`EINVAL`, and `EBADF` on some network
  mounts), since failing a save outright on those mounts is a worse answer than
  proceeding. And the occupied-scratch retry was **one** attempt, on the reasoning
  that the counter cannot repeat a name within a process — true, and beside the
  point, because the collision comes from a *previous* process: a run that crashed
  mid-session orphans one scratch file per save it made, and pid reuse restarts the
  counter at zero, so two orphans defeat one retry.

  A second review round then found a **fourth**, in the fix for the third: on
  exhaustion the last name tried is one that **already existed** — an orphan, or a
  scratch file a colliding instance is actively writing — and the cleanup deleted
  it. A failed save took another process's in-progress data with it. The defect
  predated the loop, which widened it from one chance to eight; the scratch path
  is now `Option`al and assigned only on a successful create.

  **The first test written for that fix did not test it.** It forced a failure by
  writing to a directory, which fails at the *rename* — a branch where the scratch
  file genuinely is ours — so it passed against the defect and the fix alike. Two
  mutations reported NOT CAUGHT, which is the only reason it was noticed; the
  scratch-name source is now injectable so the exhaustion branch is reachable
  without predicting global state.

  A sixth round made the `cheats.rs` swallow blocking after three rounds of my
  deferring it, and the deferral was wrong on its facts: I had claimed the fix
  needed UI plumbing with nowhere to put the error, and the panel already had
  three error fields rendered in exactly the idiom needed. `cheats::save` now
  returns `io::Result<()>`, `persist_cheats` stores the failure in a `save_error`
  field cleared on a ROM change like every other one, and the panel reports it
  **above the lists**, because the message is not about any single edit — it says
  the whole list on screen is not on disk.

  A fifth round found the one defect none of the local gates could see: the
  transient-rename predicate was a `const fn` calling `io::Error::kind`, which is
  not `const` — `E0015`, and **only on Windows**, because the call sat behind
  `#[cfg(windows)]`. PR runs here are Linux-only and the full matrix runs on
  `main`, so it would have turned `main` red after merge rather than failing the
  PR that caused it. Verified against a minimal crate before being believed, since
  a plausible reviewer claim had already proved false once this release.

  The fix is not just dropping `const`. The Windows logic moved into an
  always-compiled function reached through `cfg!(windows) && …` instead of
  `#[cfg(windows)]`, so it is parsed, type-checked and borrow-checked on Linux
  while short-circuiting away at runtime exactly as before. Restoring the `const`
  now fails **on Linux** with the same `E0015` — the defect class moved from
  "invisible until another platform builds it" to "fails the PR". Also from that
  round: `SYMLINK_DEPTH` matches Linux's own `MAXSYMLINKS` of 40 rather than a
  smaller number, because where the kernel resolves a chain and this gives up, the
  two disagree about where the file *is*.

  A third round found no blocking issues and two worthwhile refinements: `ENOTSUP`
  / `EOPNOTSUPP` joins the excused set, since the list is *ways a filesystem says
  there is no barrier here* and leaving one out fails a save on that mount; and
  the parent-directory resolution loses an allocation. Mutating the second turned
  up a **pre-existing untested property** — the fallback that maps a bare relative
  filename's `Some("")` parent to `.`, without which `File::open("")` returns
  `ENOENT`. Documented as load-bearing since it was written, never tested, and it
  matters *more* now: while the sync was best-effort, losing it meant a durability
  step quietly skipped, but now that it propagates, losing it makes `write_atomic`
  **fail outright** for any relative target.

  Getting the third one under test surfaced something else. Reaching the exhaustion
  branch through `write_atomic` means predicting the process-global `SCRATCH_SEQ`
  and planting a decoy at every name the call will pick — and **that prediction
  races**, because every parallel test calling `write_atomic` consumes sequence
  values. Measured rather than assumed: a serialising mutex over the three tests
  that *peek* at the counter still failed 2 runs in 5, because the tests doing the
  consuming are precisely the ones that never look at it. **The pre-existing
  single-decoy test had been latently flaky since it was written and had simply
  never lost the race.** All three decisions are now named functions driven
  directly, which is the fourth time this release that "extract it so a test can
  reach it" was the actual fix.

  Two mutations forced design changes rather than confirming the design. The retry
  loop's predicate had to become a **parameter**: hard-wired, the exhaustion branch
  is unreachable on Unix, and a mutation making it return `Ok(())` — silently
  reporting a save that never happened — went **uncaught**. And the mode test was
  asserting less than its name claimed, since `opts.mode(0o600)` at creation
  already yields 0600 under any ordinary umask. Two properties are **not**
  observable in-process and the module says so: `fsync` (needs a power loss) and
  creation-mode (a race-window narrowing, where a test can only see the end state).
  Neither should be deleted on the evidence that no test fails.

- **A timeline generation counter, and the telemetry that reads it.** (v2.4.0 item
  B.) v2.3.9 cleared stale debug telemetry on a ROM change and recorded that it
  could not clear it on a save-state load: of the four ways the emulator jumps
  timeline, only one is reachable from a patchable frontend call site — wasm
  load-state restores inside a `spawn_local` task, and rewind happens entirely
  inside the core.

  `Nes` now carries a session-local `timeline_generation`, and `restore_inner`'s
  existing `clear_rewind` parameter already draws exactly the needed distinction,
  so it is reused rather than duplicated. **This departs from the plan's
  enumeration deliberately:** the plan listed netplay rollback as a bump site and
  also stated the mechanism that forbids it — a same-timeline restore must *not*
  bump. Netplay rollback and run-ahead both go through `restore_quiet` precisely
  because they are same-timeline; bumping there would clear a user's telemetry
  sixty times a second, which is worse than the defect being fixed. Both directions
  are pinned by tests.

  **The counter is not serialized**, and that is load-bearing rather than a
  preference: serializing it would put an *old* value back on restore, so loading a
  state saved earlier in the same session could hand a consumer a generation it has
  already seen. The plan asked for an entry in `snapshot_schema_audit.rs`; that file
  audits the four chips, not `Nes`, so the property is pinned by an **executable**
  assertion instead — snapshot at generation N, advance past N, restore, and assert
  it did not come back to N. Simulating serialization makes it fail with exactly
  that diagnostic.

- **A standing release-anchor audit — the drift v2.3.9 corrected by hand cannot
  recur silently.** (Landed between v2.4.0 and v2.4.1; part of neither.) `crates/rustynes-test-harness/tests/release_anchor_audit.rs`
  pins **15 anchors across 10 documents** against `[workspace.package] version`:
  the README badge and Current Release section, `docs/STATUS.md`, both `AGENTS.md`
  anchors plus its "never claim a later version" guard, `VERSION-PLAN.md` (header
  *and* the `(current)` row of its release table), `to-dos/ROADMAP.md`,
  `SUPPORT.md`, `SECURITY.md`, the root `ROADMAP.md`, `OVERVIEW.md` and
  `ARCHITECTURE.md`.

  Modelled deliberately on `libretro_info_audit.rs`, which exists because the
  libretro `.info` `display_version` drifted from the workspace and advertised the
  wrong licence for eleven days. Same failure, same shape of fix: the manifest is
  the single source of truth and every other statement of the fact is *compared*
  against it rather than maintained beside it. At v2.3.9 those anchors held **six
  different values**, the oldest four releases stale.

  Three assertions beyond the version itself. The CHANGELOG must carry a section
  for the workspace version with a parseable `- <date> - "<Codename>"` tail,
  because `release-auto.yml` reads that exact line twice — for the body fallback
  and for the release title — and it has broken a release before. Any anchor that
  quotes a codename must quote the CHANGELOG's, since a right version beside the
  previous release's codename is the more confusing error: the number looks
  correct, so the sentence around it gets trusted. And `VERSION-PLAN.md`'s table
  must mark exactly one row `(current)` — at v2.3.9 it marked v2.3.5, three
  releases behind its own header.

  **It fails closed.** A marker that matches nothing is a failure, never a pass;
  an audit that finds zero anchors and reports success is indistinguishable from
  one that found them all correct, which is the defect class v2.3.9 was about.
  Proven by mutation rather than asserted: five independent mutations — a drifted
  badge version, a stale codename, an anchor reworded out of existence, a moved
  `(current)` marker, and a renamed CHANGELOG section — each fail the test they
  should and only that test.

### Fixed

- **Two shipped features stop writing empty tables into an untouched config.**
  (v2.4.0 item D.) `graphics.hd_packs` (v1.5.0) and `graphics.shader_presets`
  (v1.2.0) both documented a pre-feature config as "byte-identical". Both were
  byte-identical only until the first save: `#[serde(default)]` is a **load**
  guarantee, and the TOML serializer emits an empty table for an empty collection.
  v2.3.9 corrected the prose and deliberately left the behaviour, because changing
  what a shipped feature writes is a separate decision; this is that decision.

  Both directions are tested, because a one-directional test passes just as happily
  against a field that never persists anything — and the over-eager direction is
  the dangerous one: an `is_empty` returning `true` unconditionally would silently
  discard a user's saved presets on every save, a data-loss bug wearing the shape of
  a tidiness fix. A third test asserts the property once for **the field that does
  not exist yet**, since the defect being fixed is precisely "a field was added and
  the save-side property was not considered".

### Changed

- **The owed upstream libretro sync is filed, and it was smaller than expected.**
  (v2.4.0 item A.) `libretro-super#2074` bumps `display_version` v2.3.5 → v2.3.9 —
  **one line**. Everything else was already correct upstream, including
  `license = "GPLv3+"`. Verified before pushing: the branch file is now
  **byte-identical** to this repository's copy, which is the property
  `libretro_info_audit.rs` exists to make possible.

  `libretro/docs#1180` needed nothing — it is a **pull request** open since
  2026-08-16, `MERGEABLE/CLEAN`, unreviewed, not an issue. The misreading that
  nearly produced a duplicate is recorded because it is reusable: `gh api
  repos/OWNER/REPO/issues/N` **returns pull requests**, since GitHub's issues
  endpoint serves both.

  `AGENTS.md` now carries the cadence rule: **upstream PRs are opened only on
  MINOR/MAJOR releases** (`vX.Y.0` where X or Y changed). Patch releases do not
  sync; the next is **v2.5.0**. A **licence change overrides and syncs
  immediately** — the rule that incident produced in the first place.

- **`to-dos/DEFERRED-AND-CARRYOVER-FEATURES.md` swept entry by entry** (landed
  between v2.4.0 and v2.4.1; part of neither), against
  `main` @ `fdfb2c04`. Eleven entries struck, each carrying its evidence inline —
  a file that exists, a workflow line number, a test that says so — rather than a
  bare tick, so a closure can be disagreed with.

  Most of what the sweep found was stale by far more than the five releases it was
  scoped to. **The whole of §6a — the four items (A1-A4) defining the timebase
  rewrite — shipped in v2.0.0 "Timebase" on 2026-07-03**, six weeks and roughly
  twenty releases earlier, and §6's preamble still described AccuracyCoin as
  "100% / 139/139" when it has been an exact **141/141** since v2.0.3. A backlog
  listing the project's designated MAJOR release as pending is not untidy; it is
  misleading about what the emulator is.

  Three entries were closed by something **other than what they proposed**, and
  say so rather than being quietly ticked: the feature-combo clippy gap is closed
  by *enumerating* the combos in CI (eight invocations, including the wasm32 ones)
  rather than by adopting `cargo-hack`, which leaves a real residual — a new
  feature is uncovered until someone adds a line; `merge_group` stays open but its
  companion clause shipped, so the entry is **narrowed** to the merge queue alone;
  and R3 turned out to be a **harness artifact** rather than an emulation
  residual, so A4 is not what fixed it, and conflating the two would inflate what
  the refactor is credited with.

  §7's mapper entries below the v2.3.4 line are **explicitly not swept** — they
  need a ROM corpus to adjudicate, and asserting them from source alone would be
  the over-claim this catalogue exists to avoid.

## [2.3.9] - 2026-08-20 - "Crucible" (what the gates actually cover)

A crucible is where something is tested to destruction rather than inspected, and
that is what this release does to the project's own gates: what they cover, what
they only appear to cover, and where a regression could still reach `main`
unchallenged.

Deliberately not a feature release. The v2.3.x line added five tools in four
releases, and the recurring finding across all of them was never that the
emulation was wrong — it was that **a check reported a pass it had not earned.**

**`rustynes-apu` and `rustynes-core` both change**, so AccuracyCoin is verified
rather than asserted, and re-run after the second round of deletions rather than
only the first: **141/141 (100.00%)** on the authoritative RAM decoder, nestest
0-diff.

### Added

- **A Latency Oracle measurement is remembered per game.** (#410.) Reopening a
  game shows what was measured last time instead of an empty panel, keyed on the
  ROM SHA-256 in `[input]` — the same key shape `graphics.hd_packs` already uses.
  `#[serde(default)]` so an older config loads unchanged, plus
  `skip_serializing_if` so the key stays out of the file until there is something
  to store: a user who never opens the panel carries nothing and their config is
  not rewritten.

  **Remembering is not applying.** Nothing here touches `run_ahead`, and
  restoring never queues a pending apply, so a depth measured in an earlier
  session is still one explicit click from being applied. An **inconclusive**
  result is not remembered at all: a stored "I could not tell" is
  indistinguishable from a stored answer once it has lost the context that
  produced it.

- **The two-acquisition lock race is measured rather than reasoned about.**
  (#409.) The `needs_nes` render arm — taken exactly when a debugger or tool
  panel is open — acquires the emulator lock twice per redraw, and drops it in
  between so composite work does not hold the emulator. If the emulation thread
  takes the lock in that gap, the screen shows frame N while a panel describes
  N+1.

  `Nes::cycle()` is read at both acquisitions and compared: it is cumulative and
  monotonic, and `produce_one_frame` holds the lock across a **whole** frame, so
  any difference at all means at least one complete frame landed in the gap.
  Both a hit count and a denominator are kept — "the race did not fire" and
  "nothing was observed" both read as zero hits, and only the denominator
  separates them.

- **A RAM Atlas address can leave the panel.** (#413.) `Send to RAM Watch`
  exports a classified address into Memory Compare's watch list, carrying the
  verdict **and the lens that produced it** — liveness is relative to the
  observable, and the watch list is exactly where an unqualified "LIVE" would
  outlive the panel that qualified it. `Untested` is spelled out and cites no
  lens, because nothing was observed through one. Every address is exportable,
  including `Inert`: the rule is that a claim carries its evidence, not that
  unverified addresses are unusable.

- **A real produce-to-visible latency series, and the end-to-end figure it makes
  possible.** (#412.) The interim figure was valid only because its lag term is a
  *constant*; the vblank wait and lock contention could not simply be added on,
  because `p95(A) + p95(B)` is not `p95(A + B)`. `PresentBuffer` now stamps each
  frame at publish and the redraw records `stamp.elapsed()` after the present, so
  one sample spans the whole pipeline. The panel reports both figures, gated
  independently, and says which one it is showing.

- **The SAFETY-comment rule is a gate rather than a request.** (#423.)
  `clippy::undocumented_unsafe_blocks` is enabled workspace-wide. All 91 unsafe
  sites already carried a justification; two had it where a human reads correctly
  and a checker cannot — one of them reachable only under the `browser-cheevos`
  wasm feature, so it surfaced only when the gate ran across every gated
  combination. The gate is demonstrated to fail, not assumed to.

### Changed

- **The accuracy battery runs at review time, scoped by path.** (#408.) `setup`
  computed one `full` flag and `test-roms` ran only when it was true, so a
  regular feature PR never ran the battery and an accuracy regression could not
  be caught on the PR that caused it — #403 is the worked example. A second
  `paths-filter` output covers the chip crates, core, `rustynes-gamedb`, the test
  harness and `tests/`, and `test-roms` now runs when it *or* the existing full
  flag is true. `rustynes-gamedb` is included for a non-obvious reason: it
  rewrites the iNES header on load, so it changes what the emulator *is* before a
  cycle runs.

- **Provisioning steps are bounded, not just the jobs.** (#408, #409.) The
  cross-compile gate's `apt-get update && apt-get install` were network fetches
  with no timeout of their own, so a stalled mirror hung until the job timeout
  fired and the run was reported as cancelled rather than as what it was — four
  times during the v2.3.7 cut. Now one bounded, thrice-retried helper, with
  elevation outside `timeout` so a killed fetch cannot orphan `apt-get` holding
  the dpkg lock.

- **The docs-only CI skip is repaired — it had never worked.** (#418.)
  `dorny/paths-filter`'s `predicate-quantifier` defaults to `some`, so `'**'`
  matched everything and all seven `!` exclusions were dead from the day they were
  written. A markdown-only PR logged `Filter code = true`. Fixed with two filter
  steps, because the quantifier is step-level and `accuracy` is a list of
  alternatives that `every` would make unsatisfiable — silently disabling the
  accuracy battery while fixing a different gate.

- **The ARM provisioning installs headers rather than a toolchain.** (#417.) The
  step asked for a whole cross compiler to obtain a header package the comment
  above it had already named, and whose linker it stated was unused. Its timeouts
  were calibrated against an unmeasured claim; the log gave the real number,
  `Fetched 4201 kB in 4min 45s (14.7 kB/s)`.

- **257 lines of dead code removed**, plus 25 `#[allow(dead_code)]` attributes
  that were suppressing nothing. (#422.) Two `mc-r1-*` islands whose cargo
  features exist nowhere in the workspace, and `drain_dma` — called on every CPU
  read, every CPU write and every bus cycle, with an empty body and comments
  claiming the legacy service below it "stays active for the default build".

- **Two `cargo deny` advisory ignores retired.** (#424.) Their own entry said to
  remove them once the resolve moved past quick-xml 0.40, and it had.
  `advisory-not-detected` is a warning, so nothing surfaced it.

### Fixed

- **A freeze from one cartridge kept writing into the next.** (#419.) Both memory
  panels' freezes feed the per-frame raw-cheat overlay, and neither was registered
  with the ROM-transition hook — so a byte frozen in one game went on being written
  into the next at an address that means something else there. The sweep that found
  it now covers every panel under one rule: derived output is discarded,
  user-authored input is kept, and only input that actively **writes** is
  neutralised.

- **The call stack and access counters survived a ROM change.** (#415.) Both are
  reconstructed telemetry that does not rebuild itself, and `uninit_read` is a
  *claim* rather than a tally — carried across a ROM change it reported
  uninitialized-RAM findings about a cartridge no longer loaded.

- **The config file is written atomically and durably.** (#420.) `fs::write`
  truncates then writes, so an interruption left a truncated `config.toml` — every
  keybinding, palette, shader preset and per-game setting. Now a sibling scratch
  file, `fsync`, exclusive creation, mode carried across, symlink resolved
  (including a **broken** link, the freshly-created dotfiles case), and the parent
  directory synced.

- **A failed latency-config save is reported instead of swallowed.** (#411.)

- **Movies record two ports, and now say so.** (#421.) `FrameInput` models P1 and
  P2, so a recording made with the Four Score adapter captures half of what drove
  the run — while the Replay panel printed "Four Score (P1..P4)" at the moment a
  user decides to press Record. Widening the format is a `.rnm` epoch change, so
  this is disclosed at three levels, with the caveat printed directly under the
  claim it qualifies.

### Documentation

- **The release anchors are re-synchronised, and two of them were wrong about
  more than the version number.** Every "current release" claim outside the
  CHANGELOG had drifted, each by a different amount, which is what happens when
  the same fact is written down in eight places: `README.md` and `docs/STATUS.md`
  said v2.3.7, `VERSION-PLAN.md` said v2.3.6 (its release table stopped at
  v2.3.5, still marked `(current)`), `to-dos/ROADMAP.md` said v2.3.3 in one place
  and v2.2.5 in another, `SUPPORT.md` said v2.3.0, and the root `ROADMAP.md` said
  v2.0.4. All now read **v2.3.9**, with v2.3.8 and v2.3.7 demoted into the
  historical trail rather than dropped.

  Two carried a claim worth more than the version:

  - **`SECURITY.md` still offered support for `1.0.x` and marked `< 1.0`
    unsupported** — a policy table describing a project two major versions and
    eleven months behind the tree, on the one document a reporter reads before
    deciding whether a finding is worth sending. Rewritten for the rolling
    patch cadence RustyNES actually ships on, and it now names the two
    boundaries that change what a report *means* rather than merely how old it
    is: v2.0.0 "Timebase" (a pre-v2.0.0 `.rns`/`.rnm` is refused with a clear
    error rather than reinterpreted, so such a parsing report is not
    reproducible against a current build **by design**, ADR 0028) and v2.2.9
    (the GPL-3.0-or-later relicence is a licensing correction, not a SemVer
    break, ADR 0036).
  - **The root `ARCHITECTURE.md` presented the retired dot-lockstep scheduler as
    the current design** — `tick_one_dot`, the `% 3` phase test, and the claim
    that the bus need not re-sync the PPU "because they were already advanced in
    lockstep above". `docs/architecture.md` and `docs/scheduler.md` each carry a
    v2.0.0 banner correcting exactly this and label their historical
    subsections; the root companion had neither, so the one architecture
    document a newcomer opens first was the one describing a scheduler that has
    not existed since 2026-07-03. It now carries the same banner and the same
    labels, and states the part that did **not** change: lockstep was chosen so
    that a mid-instruction PPU event is visible to the rest of the instruction
    without a per-quirk patch, and that consequence survives the mechanism
    moving. This project has been bitten before by prose asserting an intent the
    code does not implement; a spec describing the previous implementation is
    the same failure with a longer fuse.

- **The user guide covers the tools that shipped since it was last touched.**
  `docs/user-guide/analysis-tools.md` documented three tools and the menu had
  five: **Divergence Lens** (v2.3.8) and **Audio Provenance** (v2.3.7) were
  reachable in the UI and absent from the guide, and the menu reference listed
  neither. Both are added, along with v2.3.9's **RAM Atlas → RAM Watch** export
  — including why the lens travels with the address (liveness is relative to
  what was observed, so an unqualified "LIVE" in a watch list is a claim nobody
  can check) and why the cheat, Lua and RetroAchievements exports are
  deliberately still absent (a cheat is a **write**, so it needs a
  locked-session predicate the watch export correctly does without).

## [2.3.8] - 2026-08-20 - "Parallax" (which pixels differ, not just which frame)

Cut from its own boundary commit (#407's merge) rather than from `main`, so its
artifacts contain exactly the Divergence Lens. See the `v2.3.8` tag.

### Added

- **The Divergence Lens — which pixels differ, not just which frame, and why.**
  (#407, the whole of v2.3.8 "Parallax".) Surfaced as a panel under **Tools →
  Analysis**, over a headless `rustynes_probe::divergence` core that is tested
  independently of it. `Probe` could already say whether two
  configurations of the same ROM diverge and at which frame, because a trial
  reduces each frame to one `u64`. That reduction is the right shape for
  *detecting* a difference and the wrong shape for *explaining* one: a hash says
  frame 412 differs and cannot say which pixel, so it has nothing to hand to
  Pixel Provenance, which is where an answer actually lives.

  `divergence::localise` re-runs both configurations to the detected frame,
  keeps the full output instead of its hash, and reports the *shape* of the
  difference — population count, first pixel in raster order, and the inclusive
  bounding box. Count and box separate kinds of bug from each other: one pixel is
  a sprite or a palette entry, 256 in a row is a scanline, tens of thousands is a
  scroll or a mode change.

  It localises on the **index** framebuffer — 256x240 `u16`s of
  `(emphasis << 6) | colour`, the PPU's own per-pixel output before the palette
  lookup — which is half the bytes and at least as sensitive, since the RGBA
  buffer is a pure function of it given the same palette.

  Three answers, and the third is the point: `Identical`, `Differs`, and
  **`Inconclusive`** for an exhausted budget or two trials that cannot be
  compared. The Latency Oracle's precedent applies directly: "I stopped looking"
  must not arrive wearing the same shape as "they agree". The budget is checked
  up front for all four trials, so spending two on detection and then finding the
  localisation pair unaffordable cannot consume the budget that would have
  answered the question.

  Beyond locating a difference, the Lens **explains** it. Trial-scoped
  provenance capture lets a located pixel be handed to the machinery that already
  answers "what wrote this, and from which instruction", so the answer is a cause
  rather than a coordinate — and it closes v2.3.8 item B without bisection. An
  **audio** lens resolves a divergence to the CPU cycle, the cadence at which the
  mix is genuinely computed.

  One defect was found and fixed inside the same work: the Lens left the emulator
  **thirty frames ahead** of where it started. A trial restores the anchor on the
  way in and not on the way out, which is deliberate — it is what lets the Lens
  read the trial's final frame off `nes` directly — but the outermost caller has
  to put the timeline back, and did not.

## [2.3.7] - 2026-08-19 - "Overtone" (the instruction behind every mixed cycle)

An *overtone* is the structure inside a sound that a single pitch reading throws
away, and that is what this release adds: the Audio Scope already showed the
waveform and the Audio Mixer already set the gains, but nothing linked a sample
back to the instruction that caused it. **Audio Provenance** closes that — a
per-register write attribution answering *what wrote this, and from which
instruction*, and a per-CPU-cycle mix trace answering *what were the channels
actually doing*, deliberately shaped as the APU counterpart of Pixel Provenance.

The release's real subject, though, is the trap the feature inherited. Pixel
Provenance shipped **non-functional for four releases** because run-ahead's
rollback cleared its store before any UI could read it, while a comment two lines
above the clear asserted the opposite. Audio Provenance rides the identical
rollback, so the carry landed in the **same change as the feature** rather than
after a bug report. Then the same defect turned up in **three more places** —
every restore in `rustynes-probe` — which meant running the Latency Oracle or the
RAM Atlas silently emptied both provenance panels. The v2.3.6 fix had enumerated
one caller rather than the mechanism, and the test named for the contract could
not see the breach because provenance is deliberately not in the save state.

Two defects were caught by measurement rather than by reading. `apu_throughput`,
built for this release, reshaped the plumbing **three times** on regressions
invisible in the diff; and a fuzz sweep of the save-state parse boundary found
**four** panics in `VRC7`'s OPLL where hand-tracing had found one — the
maximally-hostile fixed payload concealed one of them.

Also fixed: `$4014` and `$4016` were documented as attributed and were not, since
the bus handles them without routing through `Apu::write_register`; the browser
demo applied **no** per-game header corrections; *Rad Racer*'s roadside artifact,
where the PPU spliced a hybrid address from a stale `v`; VRC7 save states dropped
the live FM synthesizer, so rewind garbled the music; and no CI job carried a
timeout, so one hung job silently skipped a release for five hours.

`rustynes-apu` and `rustynes-core` both change, so **AccuracyCoin 141/141
(100.00%, RAM decoder) and nestest 0-diff are VERIFIED, not asserted.**

### Added

- **Audio provenance — point at a moment in the frame and read why it sounds
  like that.** The APU counterpart of pixel provenance, and deliberately the
  same shape: a per-register write attribution answering *what wrote this, and
  from which instruction*, and a per-CPU-cycle mix trace answering *what were
  the channels actually doing*. Surfaced at **Tools → Audio → Audio
  Provenance**. Output-only, runtime-default-off, and not serialized, so the
  deterministic audio contract is unaffected whether it is armed or not.

  Every ingredient but one already shipped — the Audio Scope plots the
  waveforms, the Audio Mixer sets the gains, `Apu::pulse1_out()` and its
  siblings expose live channel outputs, and the Event Viewer already classifies
  `$4000-$4017` writes as `EventKind::ApuWrite`. What existed nowhere is the
  link from a sample back to the instruction that caused it: `EventRec` carries
  `kind / scanline / dot / addr / value` — no PC, no CPU cycle — and is
  scanline-oriented rather than sample-oriented. So the event log is the
  interception *point* this reuses; it is not the record.

  The trace is per **CPU cycle**, the cadence at which the mix is genuinely
  computed, rather than per output sample. `blip` decimates to 44.1 kHz — about
  one sample per 40.6 CPU cycles — and an output sample is a weighted sum of
  transitions across the filter kernel, not a copy of one instant. Recording at
  output rate would mean picking which of those ~40 mixes "is" the sample, which
  the signal chain cannot answer; the panel reports the cycle window and says so
  instead. `MIX_CAP` is sized from **Dendy** (35,464 cycles/frame), not the NTSC
  figure that comes to mind first, and reports `truncated()` rather than
  returning a short buffer that looks complete.

  Register rows carry their **side-band effects**, because naming the right
  instruction and then describing the wrong effect is its own failure: a write
  to `$4003` does not merely set the period, it also loads the length counter,
  resets the duty sequencer and restarts the envelope. Those annotations were
  confirmed against this emulator's own implementation, not from memory.

  **The trap this feature inherited was closed in the same change as the
  feature, not after a bug report.** Pixel provenance shipped non-functional for
  four releases because run-ahead's per-frame rollback cleared the store before
  the frontend released the emulator lock, so the UI could never observe a
  populated record — and a comment asserted the opposite, which is what stopped
  anyone checking. Audio provenance rides the identical rollback, so
  `take_audio_provenance` / `put_audio_provenance` carry the state around
  `restore_quiet` in `RunAhead::finish` from the outset. Save-state loads and
  netplay rollback still clear, unchanged: those are genuine timeline changes,
  and run-ahead's is not. The regression test drives the real produce path at
  `run_ahead = 1` — the default — and is mutation-checked. Both it and its
  control are floored at 20,000 records rather than "non-empty", because the
  APU's reset sequence alone produces eight, so a non-emptiness check would pass
  on a run that emulated nothing.

  Spec: `docs/audio-provenance.md`. `rustynes-apu` gains a `debug-hooks`
  feature, forwarded from the core's.

### Changed

- **Audio provenance costs the shipped default nothing when it is not armed.**
  The feature is compiled into every build (the frontend enables the core's
  `debug-hooks` unconditionally), so "default-off" describes the runtime arm
  rather than the code. Two separate mechanisms were found charging the APU
  hot path while disarmed — the mix record was being built before the arm was
  tested, and the recording body was being inlined into the hot mix path — and
  both are fixed; the disarmed path measures at baseline. The full measurement
  chronology, including a diagnosis that was made, measured and rejected, is in
  `docs/performance.md` §v2.3.7 C2 rather than here.

### Fixed

- **The Latency Oracle and the RAM Atlas no longer empty the provenance
  panels.** Both drive the emulator and then put the timeline back, and
  `Nes::restore_inner` clears the pixel- and audio-provenance stores — correctly
  for a genuine timeline change, wrongly for a restore of the state the user is
  still looking at. `rustynes-probe` had three such restores and none of them
  used the `take_provenance` / `put_provenance` stash that v2.3.6 added for
  exactly this: `Probe::run_uncounted` (once per trial, and a latency
  measurement runs up to 21 of them), `latency::measure_in_place` (the final
  restore, which sits outside every per-trial guard), and the RAM Atlas panel's
  `TimelineGuard`. Both stores are **cumulative** — "which instruction last
  wrote this" can point thousands of frames back, to a palette byte from level
  load or a `$4008` reload from init — so the records were not rebuilt by the
  next frame; they were gone for the session.

  This is the defect class v2.3.6 was written about, found in three more places.
  The v2.3.6 fix was correct at the call site the bug report named and stopped
  there, and `docs/pixel-provenance.md` then described run-ahead as "the one
  caller that needs the exception" — a correct rule with an incomplete
  enumeration under it. Closed by moving the stash into
  `rustynes_probe::TrialGuard`, the guard that already carried rewind capture
  across a trial for the same underlying reason: state that lives outside the
  save state is not carried by a snapshot round trip.

  Pinned by the **full 2x2 matrix** — each of the two stores against each of the
  two probe restores — under four independent mutations, so a fix that put back
  only one store, or guarded only one of the two restores, fails. The fourth cell
  (`measure_in_place` against the *pixel* store) was **missing until review
  caught it**, and it was not a rounding error: the `measure_in_place` mutation
  fails only the audio test, so a final restore that put back the audio stash and
  dropped the pixel one would have passed everything. The claim of "four tests"
  was written before the fourth existed. The existing
  `measure_in_place_restores_the_live_timeline` could not have caught it: it
  compares `nes.snapshot()` before and after, and provenance is deliberately not
  in the snapshot, so it asserted something strictly weaker than the contract it
  is named for. `rustynes-probe` gains a `debug-hooks` passthrough feature,
  without which the guard would have compiled out in precisely the builds that
  need it.

- **VRC7 save states now carry the FM synthesizer, so rewind no longer garbles
  the music.** `Vrc7::save_state` wrote the *shadow* OPLL register bytes and
  never the live synthesizer — not `opll`, not `opll_clock_counter`, not
  `last_opll_sample` — and `load_state` never replayed them either. After a
  rewind, a netplay rollback, or a TAS/save-state restore the FM voice therefore
  resumed from whatever envelope and phase state it happened to be holding.
  Banking, IRQ, mirroring and PRG-RAM had always round-tripped correctly; this
  was audio-only, and only on mapper 85. Recorded as an open frontier in
  `docs/accuracy-ledger.md` since v2.2.3, closed now.

  `rustynes_apu::Opll` gains a `snapshot` / `restore` pair carrying the register
  shadow, the EG and LFO counters, the per-channel patch selection, all 18
  operator slots (phase accumulators, envelope state machines, feedback history)
  and the per-channel outputs. The lookup tables and the chip's patch ROM are
  deliberately not carried — they are constants of construction, and restoring
  them would be restoring a copy of the binary into itself. The chip type rides
  along only as a tag, so a YM2413 blob restored into a VRC7 is rejected rather
  than silently reinterpreting every slot patch against the wrong instrument set.

  The VRC7 mapper section is now **v2**, appending that blob after the VRAM.
  It is additive: a v1 blob still loads and leaves the synthesizer exactly where
  the old build left it, so an old save is no worse than it always was rather
  than newly silent. A build without `mapper-audio` has no synthesizer to
  describe, so it still writes v1 and validates-then-ignores a v2 tail — which
  preserves the cross-feature save portability ADR 0004 promises, and is why the
  version byte is build-dependent rather than unconditionally 2.

  The repair a reader will think of first was rejected on the merits: replaying
  the register shadow through `Opll::write_reg` on load needs no new format, but
  restarts every keyed-on channel's envelope at attack, so every rewind frame
  would produce an audible transient.

  The regression net keys a note, advances 20,000 CPU cycles, saves, and then
  compares 4,000 mixed samples from the source against 4,000 from a **fresh**
  mapper restored from the blob — equal sample for sample. It is
  mutation-checked: making the tail carry a *reset* synthesizer reproduces the
  pre-fix failure exactly. `Opll` is also now registered in
  `snapshot_schema_audit.rs`, the standing field-vs-schema audit, which had
  never been able to see this surface — a save-state surface no audit can see is
  precisely how a gap this size survives for four releases.

  Emulation output is unchanged (nothing on the synthesis path moved), and the
  accuracy contract was verified rather than assumed: AccuracyCoin **141/141**
  via the authoritative RAM decoder, nestest 0-diff.

  Review caught a defect in the fix itself, worth recording because of *why* no
  test could have. The accept check read `version != 1 && version !=
  VRC7_SECTION_VERSION`, and that constant is **1** on a `mapper-audio`-off
  build — so the condition collapsed to "v1 only" there and a no-audio build
  **rejected** every v2 blob, the exact opposite of the portability the constant's
  own doc comment claimed. What a build can *write* and what it must *accept* are
  different sets, and only the first varies by feature; deriving one from the
  other reads as tidy and silently couples them. The check now compares against
  literals.

  The default build takes the other branch and was correct throughout, which is
  why every gate stayed green: CI **linted** the `--no-default-features` shape
  and never **ran** it. `cargo test -p rustynes-mappers --no-default-features` is
  now a CI step, and the new regression test is mutation-checked in both
  configurations — red on no-audio with the old condition, green on the default
  build either way.

  A second review pass found two more, both in the new code and both of a kind
  the tests as written could not see. **A hand-edited save state could crash the
  emulator**: `commit_slot_update` indexes the TLL table as
  `[block_fnum][tl][kl]` with dimensions `[128][64][4]`, and `restore` was
  handing it raw bytes — a `tl` of 255 computes an index of 524,539 into a
  32,768-entry table. Every register field is now masked to its hardware width
  at the parse boundary, which is what those fields physically are. The test that
  proves it is careful about one thing: an all-`0xFF` blob is rejected by the
  envelope-state tag check before any numeric field is read, so the naive hostile
  input passes **by accident** and reports the emulator safe. The interesting
  input is the one that satisfies every explicit check and is still nonsense. A
  later review pass pushed back that the masking did not in fact cover every
  field, and was right: replacing the single fixed payload with a deterministic
  pseudo-random sweep found **three more panics** the fixed one could not,
  including one it actively hid — with every byte `0xFF`, `update_requests` is
  also all-ones, so the slot state was recomputed before the restored values
  could be used. A blob that is maximally hostile in one dimension can be
  harmless in another. The three: `eg_shift` used as a shift amount (`1u32 <<`
  panics at 32), the operator feedback pair summed as two arbitrary `i32`s, and
  `eg_rate_l` indexing a 4-entry table — the last being a field I had explicitly
  traced as safe, using a broken grep whose empty output I read as proof.

  And **`load_state` was not atomic**. The v2 tail introduced a failure that can
  occur *after* the core fields are assigned, which the v1 layout could not, so a
  truncated tail returned `Err` with the banking, IRQ state and 2 KiB of VRAM
  already overwritten — a mapper left in neither its old state nor its new one
  while the caller reported failure and kept running. `Opll::restore` was already
  atomic internally, which is exactly what made it easy to miss: the guarantee
  existed one level down and was silently discarded one level up. It now parses
  into a staged value before the first write. The truncation test asserted only
  on the return value, which is why review found this and the test did not; it
  now asserts the target is byte-identical afterwards.

- **Corrected a stale comment in `security.yml`.** It justified installing
  `cargo-audit` / `cargo-deny` as prebuilt binaries with "the repo pins rustc
  1.96 **but** cargo-audit needs >= 1.88 to compile" — which argues against
  itself, since 1.96 satisfies that. True when written at a 1.86 pin; it
  survived the v1.3.0 bump. Comments only; no behaviour change.

- **The browser demo applied no per-game header corrections.** Every mapper,
  submapper and region fix the vendored game database ships was silently absent
  on the web build — Seicross, which needs submapper 4 to clear its protection
  loop, hung there exactly as it hung on the CLI before v2.3.4.

  The mechanism is the interesting part, because this is the **third** time the
  same correction has been skipped by a load path that does not go through the
  File-menu chokepoint: the CLI (fixed v2.3.4), the mapper-coverage harness
  (fixed v2.3.4), and now the browser. `apply_load_time_header_overrides` has two
  stages — the compiled-in game database, then the per-game `<rom>.json` overlay.
  Only the *second* needs a filesystem, but the whole function was `cfg`-gated
  off wasm on its account, so the first went down with it. **A `cfg` gate
  inherited from the strictest of several stages is a gate on the whole feature,
  and nothing tells you which stages did not need it.**

  The database stage is now its own function, ungated, called from both browser
  ROM entry points (the `wasm-winit` demo's `AppEvent::RomLoaded` and the
  `wasm-canvas` embed's file picker). The overlay stage stays native-only, which
  is correct rather than a remaining gap: a browser has no `<rom>.json` to find.

  Found by the audit v2.3.6 opened after Pixel Provenance — *look for shipped
  features whose core logic is tested and whose frontend wiring is not.* And, as
  with Pixel Provenance, a comment asserted the opposite of the code: the wasm
  path was documented as one that "preprocesses separately" when it preprocessed
  nothing at all. That sentence is corrected in place, quoted, rather than
  quietly deleted.

  Pinned three ways: the browser stage must produce byte-identical output to the
  full native helper when no overlay exists; a premise test proves the correction
  is observable at all, so the agreement test cannot pass vacuously (which is
  precisely the state the browser was in); and a source-text assertion requires
  every wasm ROM entry point to call it — mutation-checked — because those call
  sites live in `cfg`-gated code a native test binary cannot link, so an absent
  call is the one thing behaviour can never catch. That third test was itself
  **vacuous on first writing**, and review caught it: it lives in `app.rs`, so
  `include_str!("app.rs")` pulled in the test's own source — which contains the
  literal it searches for, making the assertion permanently true. It survived a
  mutation check only because the check deleted the *other* file's call. A file
  that reads itself has to exclude the part doing the reading; it now truncates
  at the test module, asserts that the truncation worked, and is
  mutation-checked on both halves.

- **Rad Racer's roadside artifact — the PPU spliced a hybrid address from a
  stale address bus.** A band of stray pixels flickered in the sand to the right
  of the road, tracking the horizon, on 1639 of the 1841 frames of the movie the
  maintainer recorded. It was reported as unfixed after v2.3.0 examined the same
  area and concluded the model was correct.

  Pixel Provenance answered it in one query: the stray pixels reported
  `layer = Backdrop`, `pattern_addr = PATTERN_ADDR_NONE`, `palette = $3F00`.
  They were not sprites or mis-fetched tiles — they were **holes**, pixels for
  which no background tile had been fetched at all.

  When `$2006` is written mid-render the PPU drives a hybrid address: the low
  bits come from the octal latch (a real 74LS373 that holds the previous ALE
  half), the high six from the address bus. `ale_splice` took those high six from
  `self.address_bus` — the value latched at ALE time — rather than recomputing
  them from the **live** `v` at the read dot. The NESdev wiki is explicit that
  the bus is driven every PPU cycle and that the nametable fetch's upper bits
  follow `v`; a game that times a split early therefore reads its tile from an
  address the hardware never presents. Rad Racer times exactly that way, and the
  wiki names the symptom: "a visible glitch at the end of the line".

  The one-line fix splices from the recomputed intended address instead. Note
  what it is **not**: `COPY_V_DELAY` is unchanged at 4. Removing the deferral
  entirely was tried and **fails** AccuracyCoin's `Hybrid Addresses` test —
  `$2006` is applied at the start of a CPU cycle, before its three PPU ticks, so
  an immediate copy lets the next ALE re-drive coherently and hides the bug the
  test looks for. A delay sweep across 1..6 confirms the artifact count is flat
  (1078 / 1072 / 1077 / 1072 / 1080 / 1065), so the residual is the detector's
  floor — real roadside objects — and not remaining signal.

  This touches the core, so the contract is **verified, not asserted**:
  AccuracyCoin **141/141** via the authoritative RAM decoder, nestest 0-diff.
  The bundled AccuracyCoin ROM is also synced to upstream `7dc08e5`, whose own
  source comment was rewritten from "since we are updating `v` this cycle, we
  update the address bus" to "the address bus is updated **every ppu cycle** …
  the upper 6 bits for the nametable fetch are based on the `v` register" —
  independent confirmation of the same reading, from the author of both the test
  ROM and the emulator whose timing this was once calibrated against.

  **One committed visual-regression vector moves with it.**
  `scanline_frame_180` hashes `scanline.nes`, described in its own test as a
  mid-frame scanline-effect demo exercising mid-scanline scroll timing — the
  exact `$2006`-during-render path this changes. Its hash goes
  `39e8052eedc7f4d5` → `7c1cedf0cb725375`, deterministic across independent
  runs, and it is the **only one of nine** vectors to move. The other eight are
  byte-identical, named exactly as the tests are so the entry can be correlated
  with the artifacts: `flowing_palette_frame_60` / `_180` / `_300`,
  `full_palette_frame_60` / `_180`, `ppu_vbl_nmi_basics_frame_60`,
  `instr_test_basics_frame_60` and `nmi_sync_demo_ntsc_frame_180`. That
  distribution is what distinguishes a targeted fix from a broad rendering
  shift.

  Updating a canonical vector is permitted only on an intentional, reviewed
  behaviour change, and this is one — pinned by AccuracyCoin's `Hybrid Addresses`
  test, which covers this precise mechanism, at 141/141. `scanline.nes` has no
  pass/fail protocol, so its hash is a *sentinel* rather than an oracle; it did
  its job by flagging that output changed.

- **CI jobs are bounded, so a hung job can no longer block a release.** No job
  in `ci.yml` carried a `timeout-minutes`, which means every one inherited
  GitHub's **six-hour** default. On the night of the v2.3.6 cut the `lint` job —
  normally four minutes — hung on `main` (2026-08-17 21:11 UTC). Because `main`
  runs deliberately do not cancel each other, the v2.3.6 release commit queued
  behind it and never started; GitHub
  keeps only one pending run per concurrency group, so the commit between them
  was cancelled outright; `Auto Release` fired on *that* cancellation, saw a
  non-success conclusion, and correctly skipped.

  Every PR was green. The release simply never happened, and nothing reported an
  error anywhere — the failure presented as a workflow that had quietly decided
  not to run. Every job now declares an explicit budget with its observed
  duration recorded beside it: a 20-minute floor for jobs that finish in seconds
  (a fixed ratio would put those under a minute, where startup and a cold cache
  trip them for nothing) and roughly 2-3x for the jobs long enough for a ratio to
  mean anything. It was the second hung job that night; the first cost two hours
  on a PR.

  **That fix covered `ci.yml` only, and the gap was found the way the first one
  was — by being blocked.** During this release's own cut, `Clippy Security
  Lints` hung for over two hours in a setup step, on a job whose observed runtime
  is two to three minutes, holding the release PR. `security.yml` had no
  `timeout-minutes` on any of its three jobs, and a sweep found five more
  unbounded workflows: `android.yml`, `ios.yml`, `web.yml`,
  `antigravity-review.yml`, and `release-auto.yml` — the release workflow itself.
  All are now bounded, so the sweep across `.github/workflows/` comes back empty.

  Two details worth keeping. `release-auto.yml`'s `build` job **cannot** carry a
  timeout, because `timeout-minutes` is not valid on a job that uses `uses:`; its
  budget lives on the jobs inside `release.yml`, which already had them. (Review
  challenged this, claiming the restriction was lifted in 2022. It was not —
  checked against the schema with `actionlint`, which reports the key as
  unavailable and lists the seven that are allowed. Adding one is a syntax error,
  not an ignored key.) And
  `antigravity-review.yml` is bounded *harder* than the hosted jobs rather than
  softer, because it runs on the maintainer's own hardware, where a hung run
  holds a real machine instead of a disposable VM.

## [2.3.6] - 2026-08-17 - "Sounding" (measuring, and what a measurement may claim)

A *sounding* is a depth measured with its uncertainty attached, and that is what
every workstream here has in common. Two shipped features are found not to work at
all; two new tools are added that decline to answer rather than guess; and an
optimization campaign is closed on the strength of three measured rejections.

### The release in one line each

- **Pixel Provenance never worked, in any release since v2.3.2** — and two comments
  plus four doc claims asserted the opposite of their own code, which is why nobody
  checked.
- **Duck Hunt could never score.** The Zapper light probe was exactly inverted
  against the protocol the game uses.
- **The Latency Oracle** measures the game's own input lag instead of leaving it to
  a manual frame-advance ritual — and recommends a run-ahead depth without ever
  applying one.
- **The RAM Atlas** classifies all 2 KiB of work RAM, then verifies a candidate by
  perturbing it — the step that separates causation from coincidence.
- **The Tools and Debug menus** are regrouped by task; Tools had reached twenty flat
  entries.
- **APU Workstream D is closed**, on three measured rejections and the mechanism
  that explains them.

### Added

- **Latency Oracle** (`Tools → Analysis`, spec `docs/latency-oracle.md`). Replays
  the current moment twice — once with a probe button held, once with nothing
  pressed — and reports the first frame that differs. That index *is* the game's
  internal lag, because on a deterministic core two replays of identical state can
  differ for exactly one reason.

  It is built to decline rather than guess. `frames` is an `Option`, and `None` and
  `Some(0)` are different answers that are never collapsed: `Some(0)` means the game
  reacted on the next frame, `None` means the probe could not tell. It probes six
  buttons across three observables (framebuffer, then audio, then work RAM) and
  requires agreement; `START` is deliberately excluded, because it pauses many games
  — a reaction to a menu, not to gameplay, and counting it would over-report.

  **It recommends; it never applies.** Run-ahead is linear in the core's frame cost,
  so silently raising it can push a marginal host into dropped frames for a change
  the user never asked for. The depth appears with an explicit Apply button, and a
  test fails if storing a report ever queues a config write on its own.

- **RAM Atlas** (`Tools → Analysis`, spec `docs/ram-atlas.md`). Answers what each
  byte of work RAM is *for*, in two stages with deliberately different confidence.
  Observation classifies every address (untouched / frame tick / rising / falling /
  sparse / volatile) and is **correlation only** — `classify` returns all 2048 labels
  as `Untested`, so observation is structurally incapable of claiming liveness.
  Verification pokes the byte, re-simulates from the same anchor, and compares.

  Liveness is relative to its lens, and every verdict names the one it used: the same
  byte is routinely `Live` through work RAM and `Inert` through the framebuffer.
  `Untested` is a third state, distinct from `Inert`, because "we did not look" and
  "we looked and saw nothing" are different claims. `Inert` is documented as *not*
  meaning unused — a byte the game rewrites from a master copy each frame reads inert
  because the poke is overwritten.

- **`rustynes-probe`**, the deterministic re-simulation engine both tools consume:
  anchor, replay under controlled variation, locate the first divergence. Trials are
  budgeted, and the budget is binding rather than advisory.

- **`rustynes verify <movie.rnm> --rom <rom>`** attestation tests, closing the last
  item of #360.

- **Docs**: `docs/ram-atlas.md`, `docs/latency-oracle.md`, and
  `docs/user-guide/analysis-tools.md`. `docs/pixel-provenance.md` was also added to
  the docs-site nav, having been built but unreachable since v2.3.2.

### Fixed

- **Pixel Provenance now works.** The v2.3.2 "Lucid" marquee returned an empty
  report for effectively every user, from release until now, because of two
  independent defects.

  **Run-ahead erased the record before the UI could read it.** Run-ahead defaults
  to 1, and its per-frame rollback (`RunAhead::finish` → `Nes::restore_quiet`)
  unconditionally cleared both provenance stores. That clear is right for a
  save-state load and for netplay rollback, and wrong here for a reason that has
  nothing to do with the restore: run-ahead's rollback is the *last* thing before
  the frontend releases the emulator lock, so the panel's first opportunity to
  look was always after the wipe. It did not discard a stale timeline; it
  discarded the record for the frame on screen. `finish` now carries both stores
  **around** the restore (`Nes::take_provenance` / `put_provenance` — a move of
  two boxed stores, skipped when neither is armed), keeping exactly the visible
  frame's records. Every other caller still clears, unchanged.

  **Clicking a pixel was never implemented.** The panel offered two coordinate
  spinboxes and no click hit-test, while the docs and release notes said "point
  at"/"pin" a pixel. Clicking the game view now pins that pixel. The NES image is
  a raw wgpu blit rather than an egui widget, so the click is captured in the
  winit handler and converted by a new `gfx::window_to_nes_pixel`, which inverts
  the blit's own letterbox/crop transform — correct at any window size, pixel
  aspect and overscan crop, and `None` on a letterbox bar.

  Also fixed while here: the panel mirrored the core's armed flags in frontend
  state, which desynced permanently the moment a ROM load installed a fresh
  `Nes` (checkbox ticked, core unarmed, no way back but unticking and re-ticking)
  — the core is now the single source of truth; and the panel rendered a cleared
  record as fact, because every field of one reads as a confident "scanline 0,
  dot 0, backdrop, palette `$0000`". It now distinguishes not-armed from
  nothing-recorded-yet from off-screen.

  **Why it went unnoticed:** the core data structures were well unit-tested and
  the frontend wiring was tested by nothing — the same shape as issue #360 in the
  same release train. `runahead.rs` even carried tests pinning the determinism of
  the very code path that destroyed this telemetry. The new regression net drives
  the run-ahead cycle with provenance armed and asserts a record survives, with a
  plain-run control so a failure cannot be misread as a bad assertion, plus three
  tests for the coordinate converter — one round-tripping it against the shader's
  own uniform rather than a third re-derivation of the letterbox.

  Two comments and four documentation claims asserted the opposite of the code
  and are corrected in the same change, including one in `CHANGELOG-FULL.md`'s
  spec (`docs/pixel-provenance.md`) that contradicted itself two sections apart.

  Emulation is untouched: the new core methods are additive and output-only, so
  **AccuracyCoin holds at exactly 141/141** (RAM decoder) with nestest 0-diff —
  verified, not asserted.

- **Duck Hunt is playable: a Zapper shot can finally score.** The gun fired and
  nothing could ever be hit — at any aim point, in any part of a duck.

  Duck Hunt's protocol is "the gun must see **nothing** for one frame, then a
  bright spot in the next". `Bus::sample_zapper_light()` runs at the *end* of
  `run_frame`, so the light bit a read returns during frame N was sampled from
  frame N−1. The game therefore received its probe **exactly inverted**: on the
  blanked frame it read the previous, bright frame; on the target frame it read
  the blanked one. The shot was discarded before hit-testing, which is why aiming
  made no difference.

  The **beam-relative light model is now the default** (`zapper_temporal_light`,
  opt-in since v2.2.3). It derives the light bit from where the CRT beam is at
  the moment of the read — dark before the beam paints the aim row, lit for the
  ~19-26-scanline photodiode hold, dark once drained — which is what the hardware
  does and what the frame model structurally cannot express.

  A second defect had to go with it: the beam-relative sampler read aperture rows
  the beam had **not finished painting**, which still hold the previous frame, so
  it asserted light on an all-black screen. Measured directly — at scanline 96
  the beam was 5 dots into row 96 and the sampler saw the previous frame's sky at
  luma 152 on a frame whose mean luma was 0. Rows at or after the current
  scanline are now excluded (`aperture_is_bright_painted`).

  **The reason it shipped that way was a wrong claim, not a missing oracle.**
  v2.2.3 kept the model off because "no pass/fail light-gun test ROM exists… the
  supported titles are satisfied by either model". The second half was false, and
  the first was beside the point: the game is the oracle. Measured A/B on the same
  ROM, aim and inputs — frame model: score 000000, duck still flying;
  beam-relative: score 000500, duck marked hit. Pinned by
  `duck_hunt_zapper_shot_can_score`, which asserts Duck Hunt's own scoreboard and
  was mutation-checked (with the model forced off it fails on identical score
  pixels). New `zapper_light_probe` diagnostic reproduces the whole sequence from
  the game's `$4017` traffic.

  This changes emulation behaviour when a Zapper is attached, so the gates were
  re-run rather than assumed: **AccuracyCoin 141/141** (RAM decoder), nestest
  0-diff, 2,038 workspace tests green. Pass `set_zapper_temporal_light(false)` to
  restore the pre-v2.3.6 model.

- **The Zapper's aim was off by the letterbox.** Its cursor mapping stretched the
  256×240 image across the whole window, so the aim was wrong by the bar size
  whenever the window did not match the NES aspect, and a click on a black bar
  registered as a hit on a real pixel — while the comment directly above it
  claimed "letterbox bars read as off-screen — the correct Zapper 'no light'
  behavior", which a full-window stretch cannot produce. It now shares
  `gfx::window_to_nes_pixel` with the provenance picker, so bars are genuinely
  dark and the aim tracks the pixel actually under the cursor at any window size,
  pixel-aspect setting or overscan crop. The Input Display's on-screen indicator
  uses the same converter, so the HUD and the core agree. The Vaus paddle keeps
  its full-window sweep deliberately: a knob has no off-screen state, and how far
  the hand travels per turn is a feel decision no oracle adjudicates.

- **The libretro `.info` description is corrected.** It now advertises native
  `RETRO_ENVIRONMENT_SET_MEMORY_MAPS` support and native Game Genie cheats —
  both long-standing capabilities that the description omitted — plus the two
  v2.3.5 additions, region-correct NTSC/PAL/Dendy timing and NES Zapper support.
  "Written entirely in safe Rust" becomes "written in pure Rust, with an
  unsafe-free `#![no_std]` emulation core": the chip stack is `unsafe`-free, but
  the libretro wrapper is an FFI boundary and is not, so the scoped claim is both
  accurate and more informative. This is the file RetroArch's core-information
  screen displays.

- **A probe trial no longer clears or pollutes the caller's rewind ring.** Two
  defects at the one site every trial shares. The first is a bug an earlier fix in
  this same release reported as closed and did not close: trials restored their
  anchor with the loud `Nes::restore`, which clears the rewind ring, and
  `latency::measure_in_place` runs up to 21 trials against the live emulator — so
  asking how much input lag a game has destroyed the user's rewind history,
  twenty-one times over. The earlier fix had changed only the final restore, not the
  per-trial one. With the wipe fixed a second defect became visible: the ring then
  *grows*, because trial frames are captured like any others, and those frames are
  re-simulated and never happened on the user's timeline. Both fixed; the test
  asserts the ring returns exactly as it was, since a weaker "not cleared"
  assertion is what let the incomplete fix pass review.

  Separately pinned: a trial's samples do not change when the caller has rewind
  armed, so **no measurement taken before this fix needed re-running**. The engine's
  premise is that a replay from one anchor is bit-identical, so anything that
  silently perturbed state would have invalidated the primitive rather than one
  measurement.

- **The audio observable was structurally dead.** `Observable::AudioEnergy` never
  saw any audio: the probe's trial loop emptied its buffer and never filled it, so
  the energy reduction summed an empty slice and every frame of every trial
  reported zero. Nothing failed, because a lens that returns a constant never
  disagrees with itself — so the Latency Oracle's audio fallback stage, the one
  that exists for a game whose reaction is audible before it is visible, silently
  degraded to work RAM, and the RAM Atlas's audio lens would have reported **every
  address inert**. The comment above the missing call said "Drain EVERY frame,
  whatever the observable" and explained at length why. Found in review on #392.

- **The RAM Atlas is unavailable during locked sessions.** Both of its actions
  advance the live emulator and Verify pokes work RAM, so it is now gated on the
  same `writes_locked || hardcore_blocked` predicate `emu.write` uses — netplay, a
  TAS record or replay, and RetroAchievements hardcore. Under netplay or a movie it
  would diverge a timeline other peers are lockstepped to; under hardcore it is the
  memory write that mode exists to forbid. The disabled state names which reason
  applies.

- **An un-perturbable address is reported `Untested`, not `Inert`.** `verify_liveness`
  skipped the poke for an address outside work RAM and then let the two identical
  trials agree, producing a confident verdict for a byte it never touched — the
  exact failure mode that module documents itself as never producing. It is public
  and takes a full `u16`, so a CPU-space mirror such as `$0810` is a plausible
  caller input.

- **The Latency Oracle's felt-latency figure is derived, not transcribed.** It
  multiplied by a hardcoded NTSC 16.639 ms, understating PAL and Dendy by 20.2% —
  the identical figure and mechanism as the v2.3.5 libretro defect, where a
  hardcoded 60.0988 fps had lost all connection to the constant it was copied from.
  It now uses the console's own `frame_duration`, captured at measurement time.

### Changed

- **Upstream libretro `.info` syncs are batched to MINOR releases.** RetroArch
  reads its copy from `libretro/libretro-super`, which nothing syncs
  automatically; the next sync is v2.4.0, so that copy reads `v2.3.5` through this
  line by decision rather than oversight. A stale `display_version` misreports a
  number — the v2.2.9 incident was a stale **licence**, misreporting the terms of
  distribution — so licence, supported extensions, and declared-capability changes
  still sync immediately. Recorded in `docs/libretro/UPSTREAM_SYNC.md`.

- **The Tools and Debug menus are regrouped by task.** Tools had reached twenty flat
  entries spanning cheats, TAS authoring, media capture, multiplayer, ROM inspection
  and provenance analysis; Debug listed "CPU" and "Lua Script" as peers in a
  fifteen-item column. Tools becomes Cheats at the top level, then Movies &
  Recording, Audio, Input, Game Data, Analysis and HD Pack, with Netplay and
  RetroAchievements below a separator — they change what the *session* is rather
  than being tools pointed at the game. Debug splits into Chip State, Memory and
  Execution, plus Symbols. Emulation's two FDS entries become one Famicom Disk
  System submenu.

  No entry is removed, none changes what it dispatches, and nothing moves between
  top-level menus — only the depth at which it sits. The movie transport's gating
  changes shape but not effect: the ROM/netplay condition moves from deciding
  whether the submenu can *open* to per-item enabling, so the reachable set is
  identical but the user can see which entries are unavailable instead of facing one
  opaque disabled label. `docs/user-guide/menus.md` is corrected, and was already
  stale before this release touched it — it listed five Tools entries against an
  actual twenty, and still documented a "Show Debugger" toggle removed in v1.7.1.

- **`MAX_RUN_AHEAD_DEPTH` is shared rather than redeclared.** The Latency Oracle's
  clamp was a third independent `3`; that constant exists precisely because two
  earlier caps drifted apart (PR #358).

### Documented

- **APU Workstream D is closed.** The 18.7%-of-frame-time figure stands — it is a
  correct v2.3.1 subsystem attribution, visible only because that pass attributed by
  source file, since fat LTO inlines the APU into `cpu_clock`. What is settled is
  narrower: the figure is **not recoverable by gating per-cycle bookkeeping**, the
  only strategy the workstream ever tried. One adoption (C1, shipped in v2.3.5 at
  −3.3% to −4.2%), three measured rejections (D1, D3, D6), one declined on
  inspection (D5), and two left unmeasured deliberately (D2, D4).

  The three rejections share one mechanical cause, and it generalises to the two
  remaining levers: under `lto = "fat"` with `codegen-units = 1` the guarded code is
  already inlined, its repeated loads already merged by common-subexpression
  elimination, and the elided branches always-not-taken and so perfectly predicted.
  Swapping predictable not-taken branches for an equivalent count of loads plus a
  predicate is arithmetically a wash. Stated as the rule worth keeping: *"this work
  is inert on almost every cycle" predicts a win only if the work is actually
  executed* — and under fat LTO with perfect prediction it largely is not.

  Also recorded: D1's run 1 looked like a textbook win at −3.81% (p = 0.00) on a
  shipped default workload and was **entirely an artifact** — the order-bias control,
  benching the reference against *itself*, drifted −3.73% on that same workload with
  no code change at all, because `ab_check.sh` benchmarks the reference immediately
  after a 44.9-second fat-LTO compile across all cores. Three conditions that would
  justify reopening the workstream are written down, none a variation on per-cycle
  gating. Full numbers in `docs/performance.md`.

## [2.3.5] - 2026-08-16 - "Manifest" (what the core declares about itself — and the APU, measured at last)

### Fixed

- **The libretro core metadata advertised the pre-relicense MIT/Apache-2.0
  license.** Corrected to `GPLv3+` — libretro metadata uses short tokens and
  marks "or later" with a trailing `+`, so the bare `GPLv3` carried since v2.3.0
  understated RustyNES as GPL-3.0-only. The description's mapper count is
  corrected 172 → 174.

  **This is the repo-side half only.** RetroArch reads a separate copy in
  `libretro/libretro-super` that this project does not control, so nothing in
  this release changes what an end user currently sees; the user-visible fix
  completes when that sync and the `libretro/docs` page merge upstream. Since
  RustyNES's license is itself the outcome of a corrected provenance failure,
  a frontend misreporting it is a compliance matter rather than a cosmetic one.

  A standing audit (`libretro_info_audit.rs`) now pins the `.info`'s `license`,
  `display_version` and `supported_extensions` — the last derived from the core's
  own `retro_get_system_info` declaration rather than a repeated literal — so the
  local file cannot drift and the upstream sync is a copy rather than a
  re-derivation. `docs/libretro/UPSTREAM_SYNC.md` records the full investigation,
  the token mapping and its evidence, and the surfaces that must move together.

- **PAL and Dendy games ran ~20% too fast in RetroArch.** The libretro core
  reported a hardcoded 60.0988 fps — the NTSC rate — for every cartridge, and
  never implemented `retro_get_region` at all, so RetroArch was told every game
  was NTSC on both axes at once. The emulation was never wrong: `Nes::region()`
  and `FRAME_DURATION_PAL` (19.9972 ms, 50.0070 Hz) have always been correct.
  Only what the wrapper advertised was. Both now follow the loaded cartridge, and
  the NTSC figure is derived from the core's own constant rather than
  transcribed — it reproduces the old value exactly, so nothing changes for the
  NTSC majority.

- **RetroArch's Reset did nothing.** `retro_reset` was never implemented, so it
  fell through to the library's default, which is literally a no-op. The menu
  entry and the hotkey both appeared to work and had no effect for the core's
  entire existence. Now soft-resets the console — `Nes::reset`, the RESET line,
  which preserves RAM and the CPU/PPU phase alignment; Game Genie codes survive,
  as they do on hardware through a pass-through cartridge. A Vs. cabinet resets
  both of its cross-wired consoles together.

- **Per-game state leaked across unloads.** `retro_unload_game` was also a
  default no-op. The cartridge handles were replaced on the next load, but the
  Game Genie map — keyed by the frontend's cheat *index* — survived, so indices
  from a previous game stayed live and a later removal could act on a code
  belonging to a cartridge no longer inserted.

- **The libretro controller tables were dangling stack pointers.** Found in
  review, and verified against RetroArch's handler rather than the header's
  prose, because `libretro.h` does not specify the lifetime either way:
  `SET_CONTROLLER_INFO` shallow-`memcpy`s the outer `retro_controller_info` array
  and **retains** each entry's `types` pointer, dereferencing it later when the
  Controls menu is built. Built as locals, those pointed into a stack frame that
  died when the environment call returned — a use-after-free read at menu-open
  time, on code that compiled cleanly. The tables are now `static`; the outer
  array stays a local deliberately, because it *is* copied, and that asymmetry is
  documented at the call site. The neighbouring `set_input_descriptors` call was
  checked rather than assumed, and is safe: RetroArch walks that array during the
  call and retains only `'static` string pointers.

- **The advertised display aspect assumed square pixels.** The core sent
  `aspect_ratio = 0.0`, which tells the frontend to derive the ratio from the
  pixel dimensions — 256/240 ≈ 1.067. A NES does not produce square pixels, and
  RustyNES's own desktop frontend applies 8:7, so RetroArch and the native app
  disagreed about the shape of the same frame. Now ≈1.219, and doubled for a Vs.
  cabinet's 512-wide side-by-side present, which a single fixed ratio could not
  have served.

### Added

- **An APU throughput bench** (`crates/rustynes-apu/benches/apu_throughput.rs`),
  alongside the CPU's and the PPU's. The APU is **18.7% of frame time** by
  v2.3.1's per-source-file attribution, and it is the largest core cost that had
  never been examined — because fat LTO inlines it wholesale into `cpu_clock`, so
  it does not appear in a symbol profile at all, which is why the ten-candidate
  v2.3.1 hot-path sweep never reached it. Three workloads (silent floor, all five
  channels running, and the expansion-audio path), each one NTSC frame of 29,780
  CPU cycles so the numbers compare directly against the `full_frame` bench the
  >3% adoption bar is adjudicated on. **81% of the active per-cycle cost is paid
  with every channel disabled** — the overhead is very largely unconditional.

- **NES Zapper support in the libretro core.** The emulation has existed for a
  long time — `Nes::set_zapper` resolves the photodiode against the CRT beam —
  but the wrapper polled joypads only and never implemented
  `retro_set_controller_port_device`, so light-gun games were unplayable through
  RetroArch despite being fully emulated. Ports 1 and 2 now offer "NES Zapper"
  in the Controls menu via `RETRO_ENVIRONMENT_SET_CONTROLLER_INFO`; ports 3 and 4
  — a Vs. cabinet's SUB console — are advertised pad-only, since a cabinet has no
  light gun. Off-screen
  and "reload" reports are forwarded as a trigger pull at a guaranteed-dark
  position rather than dropped, which is how a real Zapper behaves when pointed
  away from the television — the mechanism the shoot-off-screen behaviour in
  those games depends on.

- **The libretro crate's first unit tests** (it had none). Eight, pinning the
  region/timing derivation, that NTSC is unchanged by it, that Dendy still shares
  PAL's frame duration — the assumption the region fold depends on — the declared
  sample rate against the rate the APU is actually built with, the display
  aspect, and the port-device defaults.

### Documented

- **Why RustyNES does not appear in RetroArch on iOS / iPadOS / tvOS.** Not a
  build failure: the buildbot carries a valid, current core for every Apple
  target. iOS cannot download cores, so the App Store build bundles a hardcoded
  list in `libretro/RetroArch`'s `pkg/apple/update-cores.sh`, and RustyNES is
  absent from it while every NES competitor is present. The remedy is a one-line
  upstream addition covering all three Apple platforms. Details and the
  submission requirements are in `docs/libretro/UPSTREAM_SYNC.md`; the PR is
  tracked separately, as it too lands in a repository this project does not
  control.

### Changed

- **The APU's default-configuration mix takes a specialized path** (−3.3% to
  −4.2% on `nes_run_frame_nestest`, across two full replicates). Every CPU cycle
  at 1.789 MHz, `tick_with_external` evaluated a per-channel `gate` closure
  branching on `channel_mask`, a per-channel `scale` closure branching on
  `channel_gain`, a 6-wide `f32` array copy, and a sixth mask test — all of them
  the identity at the shipped default, which the determinism contract guarantees
  the oracle never leaves. The specialization hoists that question out of the
  per-cycle body, the same shape as the PPU fast dot path. `mix()` receives
  exactly the same five arguments, so output is **byte-identical by
  construction**; a 2,048-point sweep pins it anyway, and AccuracyCoin holds
  **141/141** with nestest 0-diff.

  Recorded in `docs/performance.md` with a caveat rather than a clean story: the
  absolute saving on `nes_run_frame_nestest` (~124 µs) is about **three times**
  what the standalone APU bench attributes to the change, and ~8× the saving on
  `flowing_palette` — from a component doing identical work in both. The likely
  mechanism is an LTO/register-allocation knock-on in `cpu_clock`, which is the
  same inlining that hid the APU from the profile to begin with. Adopted on the
  measurement, not on the explanation.

## [2.3.4] - 2026-08-15 - "Ledger" (mapper coverage + the load path the harness could not see)

### Added

- **Mapper 154 (NAMCOT-3453) and mapper 243 (Sachen SA-020A).** Both surfaced from
  the coverage sweep once the per-game database began reaching the harness, which
  is what routes *Devil Man* from its mapper-88 header to 154 and 美女拳 *Honey
  Peach* from its mapper-150 header to 243. Each is the only game on its board,
  and both now boot: *Devil Man* to its intro cutscene, *Honey Peach* to gameplay.
  Mapper breadth **172 → 174 families**, both `BestEffort` (their dumps are staged
  but not redistributable, so neither can be honestly oracle-gated).

  Neither needed a new type, because neither is a new chip. **154 is mapper 88**
  plus a one-screen nametable bit — decoded across the whole `$8000-$FFFF` range,
  not just the bank-select window — so it is a third `Namco118Board` variant.
  **243 is the same ASIC as mapper 150** wired to a different PCB, which NESdev
  records under its Errata; only the bank-bit significance differs (R2 is the CHR
  LSB on the SA-020A and the MSB on the SA-150), so it is a `Sa020aBoard` variant.
  Ten unit tests. `Namco118`'s save-state moves to v2: 154 makes mirroring mutable
  on a family where it had been constant, and a v1 blob would restore the wrong
  CIRAM page.
- **Mapper 176 submapper 2 (WAIXING-FS005/FS006).** The 8025 ASIC's incompatible
  variants split by NES 2.0 submapper, and only the FK23C half was implemented.
  FS005 adds the `$A001` RAM Configuration Register — 32 KiB of banked WRAM, the
  `$5000-$5FFF` register-window disable that the Waixing copy-protection sequence
  is built on, and a mapper-195-like mixed CHR-ROM/CHR-RAM mode — plus two-bit
  `$A000` mirroring with single-screen pages gated on that register, PRG A21-A25
  from `$5xx0.3/7` and `$5xx2.5/6/7`, and the `$46`/`$47` bank-select swap (which
  applies only with the PRG-invert bit set; `$06`/`$07` are unswapped).
  Implemented from the NESdev wiki page; unlike the FK23C banking transforms in
  the same file, no reference-emulator source was consulted for any of it.

### Fixed

- **The per-game database destroyed correct mapper numbers on every Sachen
  cartridge.** The vendored table uses `0` in its Mapper column as the
  unfilled-row default, with no separate empty marker, so
  `80D63472, PAL, 0, 0, …` — `Sidewinder`, a Sachen SA-72007 and genuinely
  **mapper 145** — was read as "force NROM". The header was overwritten and the
  ROM then failed NROM's size check and would not load at all. **12 staged ROMs:
  every Sachen board in the corpus** (133, 143, 145, 146, 147, 148, 149, 150).
  A `0` in that column is now treated as "unspecified"; the 11 legitimate
  non-zero overrides are unaffected. Present since v1.2.0 and reaching users, not
  only CI, because the frontend applies these on every ROM load.

  Same failure mode as the mirroring column freezing *Wizards & Warriors*
  (ADR 0031), and fixed the same way: refuse to apply an override that cannot be
  distinguished from "no data".
- **Three Waixing dumps were being emulated as a board that cannot exist.**
  `Chu Liu Xiang`, `Mo Shen Fa Shi` and `Shui Hu Zhuan` carry iNES headers
  declaring **mapper 30 with 256 KiB of CHR-ROM**, and UNROM-512 is a CHR-RAM-only
  board — so the header refutes itself. They are FS005 cartridges, and the loader
  now routes any mapper-30 image that declares CHR-ROM to mapper 176 submapper 2.
  Genuine mapper-30 images declare zero CHR-ROM and are untouched. Two of the
  three now boot to their title screens and menus; `Chu Liu Xiang` still renders
  no tiles and is recorded as an open residual rather than claimed as fixed.
- **Bandai FCG (mapper 16): a debug-build panic on real ROMs.** The I2C EEPROM
  byte-address counter is a `u8` advanced as `(addr + 1) & addr_mask()`, and the
  mask is `0xFF` on every chip but the X24C01 — so the mask was written to express
  a counter that rolls over, but the add traps on the wrap first. Now
  `wrapping_add(1)`, which is what the hardware ring does.
- **The coverage harness could not observe game-database fixes.** It loaded ROMs
  with `Nes::from_rom` while the frontend rewrites the header first, so a ROM
  fixed through the per-game database still reported blank in the regression net —
  the fix was real and the net simply could not see it. The database is now its
  own `rustynes-gamedb` crate that both consume (`rustynes-frontend` re-exports it,
  so no call site moved), and its user-overlay directory became injectable rather
  than hard-wired, so the harness cannot pick up a developer's local overrides.
  **Seicross** now renders under the harness, closing the loop on PR #127, as does
  `AV Pachi-Slot`. The risk was measured rather than assumed: all 25 staged ROMs
  whose mapper or submapper the database changes were run, and none went blank.

## [2.3.3] - 2026-08-14 - "Cadence" (display pacing + the run-ahead throttle)

### Added

- **Frontend: the run-ahead budget throttle engages lower and degrades gracefully
  (v2.3.3 F21).** The gate was 85% of the frame budget; F18 measured
  `run_ahead = 2` at 77.4% with **10.7%** of frames held for the wrong number of
  refreshes and the gate unfired. It is now **75%** — between the two measured
  points, 52.1% healthy and 77.4% harmful — and the throttle removes **one depth
  at a time** (`runahead_throttle_steps`) instead of dropping to zero,
  re-measuring each median window so an unaffordable host still converges to 0
  without the cliff. Release predicts the cost of giving back one step using
  F18's per-frame-linear model. Measured at `run_ahead = 2`, three arms in a
  Latin square: unthrottled **6.43%** of frames wrong → all-or-nothing **0.87%**
  but at depth **0** → step-down **0.84%** at depth **1** — the same cadence as
  disabling run-ahead while keeping a frame of latency reduction. A budget guard
  firing earlier and more gently, **not** a shudder fix: `display_produce_due`
  was measured first and delivers 98.3% correct holds at the shipped
  `run_ahead = 1`.
- **Frontend: the display refresh can now come from the Wayland compositor.**
  A new `wayland_presentation` module reads the refresh period straight off
  `wp_presentation`'s `presented` event — the compositor's own figure, which
  does not depend on the `wl_output` global whose absence leaves winit's
  `current_monitor()` answering `None` for an entire session. It binds against
  winit's existing connection and surface, polls without blocking, and settles
  on one value per session, so the pacing regime cannot oscillate. A declared
  refresh still always wins; this only fills the gap where there is none.
  Wayland-only and best-effort — every failure path returns `None` and leaves
  X11, Windows and macOS on exactly the code they ran before. The perf-log
  header gains `refresh_source` (`declared` | `presentation` | `none`), because
  two captures with the same refresh from different sources are not the same
  experiment. Measured over 16 asserted 45 s captures on four ROMs: display-sync
  now engages and **holds every row of every run**, where Bad Dudes (MMC3) and
  Bandit Kings (MMC5) previously never left the wall-clock pacer. Dropped frames
  on Bad Dudes go from **114-186 per 45 s to 1-6**, audio underruns are 0
  throughout, and console-rate error stays within 0.16%. One measure did not
  improve and is recorded as open in `docs/performance.md` v2.3.3 F4: the
  `produced` interval p95 sits at 27-33 ms, and the control run intended to
  attribute it was confounded by capture order, so no cause is claimed.

- **Frontend: instrumentation for the unresolved display-sync shudder** — two
  named suspects were tested and **both refuted**, which is the result. New:
  display-tick arm counters (`tick_ok` / `tick_timeout` / `tick_dropped`), a
  winit-thread emulator-mutex blocking series (`rlock_*`, the mirror of the
  producer's `wait_*` — only the producer side had ever been measured), and an
  **env-gated, default-off per-frame trace** (`RUSTYNES_FRAME_TRACE=1`) writing
  one row per produce and per present, with `scripts/perf/trace_shape.py` to
  classify its temporal shape. `rwork` is now `rtot - rwait - rlock`.

- **Core: a slim restore for run-ahead was measured and REJECTED (v2.3.3 F19).**
  Run-ahead, netplay rollback and TAS seek all re-simulate immediately after
  restoring, so the 245,760-byte framebuffer they restore is overwritten before
  it is seen — and `PPU_SNAPSHOT_SLIM_FLAG` already omits it. Projected ~110 µs
  per restore; **measured 6.9 µs** (122.8 → 115.9 µs), which is **0.25%** of the
  2.802 ms run-ahead increment and an order of magnitude under the >3% bar. The
  estimate came from "the framebuffer is 94% of the snapshot **bytes**" carried
  silently into a claim about **time** — 245 KiB is ~12-25 µs of memcpy, so it
  was never 94% of a 122 µs restore. Rejected before implementation, which also
  avoided a save-state correctness hazard and two contract renegotiations
  (netplay hashes the framebuffer as a desync classifier; TAS seek has a test
  asserting it equals linear replay). A `nes_restore_quiet_slim_*` probe bench
  keeps the evidence. What it did establish: restore costs ~114 µs with **no
  framebuffer at all**, so its 8.3× asymmetry against `snapshot_core_into` is
  per-section deserialization, not the payload.
- **Run-ahead's cost is the frames, and depth 3 throttles itself (v2.3.3 F18).**
  Sixteen captures, four per depth, in a **Latin square** (each depth in each
  round-position exactly once — F13's correction: alternation balances drift
  direction but does not buy exchangeability), every one validity-gated by F16
  and **16/16 passing**. Emulation cost is **linear in depth** at the core's own
  ~4.3 ms per frame — increments +4.491 and +4.213 ms, equal within 6% — so with
  snapshot + restore at ~136 µs (F19), run-ahead's cost is the emulated frames
  and essentially nothing else: **the price of the feature, not overhead around
  it**. Budget shares: 25.1% / 52.1% / **77.4%** at depths 0/1/2. Depth 3
  measures like depth 0 because it **throttles itself** — `run_ahead_throttled`
  is `true` in every one of its captures, the guard engaging at 85% of the frame
  budget against depth 3's ~17.2 ms; correct behaviour that reads as a defect
  without that column. It also retires an earlier cross-session artefact: the
  +3.09/+4.20 ms increments that suggested a non-linear cost structure were
  session noise, not structure.
  **`tick_lat` / `tick_iv` — the trigger is late, not the emulator (v2.3.3
  F15).** Two new independently-ranked series decomposing the produce interval,
  whose variance tracks missed presents at r = 0.937. The display tick's channel
  payload became a `CLOCK_MONOTONIC` timestamp instead of `()`, which is what
  makes the cross-thread hop measurable. First verified capture (SMB,
  `run_ahead = 1`, display-sync /2, window confirmed on screen by the F16 gate):
  the winit→emu hop is **0.033-0.050 ms** — negligible — while `produced` p95
  (24.635 ms) matches `tick_iv` p95 (24.578 ms) to within 0.06 ms. With `rlock`
  at 0.000 and `tick_timeout` at 0 of 1494, **the produce tail is inherited
  wholesale from the trigger interval**: the emulator is not late, it is asked
  late. First positive location the campaign has produced rather than an
  elimination. Also `crates/rustynes-frontend/src/clock.rs`, holding the
  frontend's only `clock_gettime` call site.
  **The unexplained between-session cadence spread is emulation budget margin
  (v2.3.3 F17).** F14 left one quantity open and called it the largest
  unexplained question in `docs/performance.md`: display cadence error varied
  **0.69-3.08%** in one session and **8.6-18.9%** in others, *on the same binary
  pair*. Across **27 captures**, plotting it against what fraction of the NTSC
  frame budget the emulator consumes at p95 (`cost_p95` / 16.639 ms) separates
  them **completely — no overlap**: every capture under 60% utilisation has
  ≤ 8.59% error, every one at or above has ≥ 9.02% (r = +0.836). Two independent
  routes get there — the run-ahead **baseline** (34% of budget at depth 0, 52% at
  1, **78% at 2**) and a host-contention **tail** — and the error follows the
  total, not the cause. The `run_ahead = 2` captures are the decisive evidence
  that this is causal rather than a shared symptom of host load: their tail ratio
  is 1.04-1.05 (evidence against a large contention *spread*, not proof of zero
  contention), their cost is elevated **structurally by design**, and they still
  show 14.77-18.93% error — showing emulation cost is *sufficient* to produce the
  effect, not that it is the only contributor. **The spread was never a property of the
  pacing code**, which is also why the F13 A/B could not resolve a ~0.4-point
  configuration effect sitting inside ten points of variance. Limits stated in
  full in F17: `ra = 2` is n = 3 from one session, 60% is an observed separator
  rather than a derived threshold, and this does not identify the reported
  shudder — only the spread between these captures.

  **The display-cadence metric was measuring the producer — F12 and F13
  corrected (v2.3.3 F14).** Both quantified "frames shown for the wrong
  duration" by counting refreshes between consecutive **produce** timestamps.
  Both ends of that interval are producer-side, so a produce firing 3 ms early
  followed by one 3 ms late scores as a mistimed pair **even when the panel
  showed both frames for exactly two refreshes**. The display-side series was
  already in the trace and unused: `since_present`, recorded on the present,
  whose gaps between frame-carrying presents are all exactly the divisor
  under a healthy cadence. Pooled over seventeen captures the two read **32.96%**
  and **5.41%** wrong — a factor of six. Every "N% of frames shown for the wrong
  duration" figure in F12 and F13 is retracted; the display was ~94.6% correct
  while the document said 65-74%. Re-running the F13 A/B on the correct metric
  **removes its display-side conclusion entirely**: two of four pairs reverse and
  the exact paired test gives **p = 0.25**, so the fix's effect on what the panel
  shows is not distinguishable from noise. (The first version of this correction
  was itself wrong — it divided by presents rather than displayed frames, and
  tested run-lengths against 1, which holds only at divisor 2; both were caught
  in review on PR #362 and the divisor is now inferred as the modal gap.) The `rlock` 8.707 -> 0.000 ms collapse and the double-replay
  removal are direct measurements and are unaffected. Three mechanisms were
  measured and refuted along the way (presentation-path flipping — `flags` is a
  constant 7; compositor sequence numbers — `seq` is 0, this compositor reports
  none; produce margin — no phase dependence). `trace_shape.py` now leads with
  the display-side metric, labels the old one as producer jitter, declines to
  rate fewer than 100 runs, and **verifies the clock join by span overlap**
  instead of assuming it — `clock_id` was `unknown` in every trace ever written
  because it is read before the Wayland registry answers, and is now emitted as
  a comment row once known.

  **`present_discarded` — an unpresented surface now reports itself (v2.3.3
  F16).** New `PerfView` field and perf-log column carrying the compositor's
  cumulative count of frames it composited but never scanned out. Sustained
  discards stop the *measured* refresh from settling, which costs display-sync
  only where no *declared* refresh is available either; where both are absent the
  session silently holds the wall-clock fallback. `PresentationClock::discarded()`
  had existed since scanout tracing landed and **was read by nobody**, so the
  condition was invisible. `perf_log_check.py` now fails closed on a non-zero
  count, since an occluded capture yields plausible but meaningless pacing
  numbers. Measurements, mechanism and limits in `docs/performance.md` v2.3.3
  F16.

  Later joined by **`rcpu`** — the winit thread's own `CLOCK_THREAD_CPUTIME_ID`
  differenced across the `rwork` span, so `rwork - rcpu` is time the thread spent
  off-CPU rather than computing. It was built to test whether the 9-32 ms `rwork`
  tail was descheduling, and it showed wall and CPU **identical** on an idle host
  and 8 us apart under 20 spinning threads — while **the tail did not reproduce
  at all**. Every tail-bearing capture had been taken while a `cargo build` was
  running: the tail was the measurement environment, not the frontend. `rcpu`
  stays in the tree as the check that tells the two apart. Also
  `scripts/perf/trace_shape.py --warmup-s N`, making the 8 s startup-transient
  discard (a host-tuned heuristic) overridable instead of hard-coded.

  **Superseded in part by F14 (below): the display-side claims in this entry and
  the next were computed with a producer-side metric and are retracted.**

  Measured on six 45 s SMB captures: the 25 ms tick watchdog **never fires** at
  the shipped `run_ahead = 2` (0 of ~1855 ticks) and drops no ticks, so its
  numeric coincidence with the 25-36 ms `produced` p95 was exactly that; and the
  winit thread does **not** block on the emulator mutex there (`rlock` p99 =
  0.000 ms), so the 13 ms `rwork` p99 is neither lock, nor egui, nor GPU, and
  stays unattributed. A **ragged refreshes-per-frame cadence** was then measured
  and, on further investigation, **disqualified as mis-measured**: present
  intervals are bimodal (31.8% under 1 ms, 38.9% at 12-16.7 ms, only 1.9% near
  one refresh), which is the known triple-buffered-Fifo signature — so
  `record_presented` timestamps *queue submission*, not scanout, and any cadence
  derived from it counts queue slots rather than refreshes on screen. The
  shudder remains **unexplained**; this round removed two candidates and
  disqualified a third line of evidence. Next instrument identified:
  `wp_presentation`'s `presented` event already delivers real scanout
  timestamps, which the handler currently discards. See `docs/performance.md`
  v2.3.3 F10.

  **Root cause then found (F11/F12).** Recording those scanout timestamps showed
  the display misses **4.6% of refreshes**; joining them to the produce series
  through a `CLOCK_MONOTONIC` anchor showed only **65.3% of produced frames get
  the intended 2 scanouts** — 18.3% get one, 10.6% get three, and **3.2% are
  never displayed at all**. Completing the `rlock` series (it had missed four
  acquisition sites, which is why F10 read it as zero) moved the whole
  unattributed 13 ms `rwork` tail into it: `rlock` p95 is **8.707 ms** against
  an 8.334 ms refresh period, and `rwork` p99 drops to 0.109 ms. The winit
  thread blocks on the emulator mutex for more than a refresh, so redraws land
  late and frames miss their slot. `pump_watchpoints` takes that lock on every
  redraw unconditionally.

  **That acquisition is now removed (F13).** A conservative, emulator-free
  predicate (`DebuggerOverlay::wants_emu_pump`) gates the work, and the call
  moved off the redraw path into the lock `post_produce_housekeeping` already
  holds — which also fixes a second defect: at divisor 2 there are two redraws
  per produced frame, so the old placement replayed each frame's debug logs
  **twice**. `rlock` p95/p99 go **8.707/9.008 ms → 0.000/0.000 ms**. It also
  improves **producer-interval regularity**, confirmed by an A/B with both
  binaries rebuilt from the two adjacent commits that differ only by this
  change, run **alternately** rather than in blocks, four captures each:
  produce intervals spanning exactly two refreshes go **67.75% → 73.77%**, ranges
  non-overlapping. Alternation is **not** randomisation, so the correct test is
  paired, giving **p = 0.0625** — suggestive, not established.

  **What this does NOT show (see F14 below).** That 67.75 → 73.77% is a
  **producer-side** statistic, not a display one: it counts refreshes between
  consecutive *produce* timestamps. Re-run on the display-side metric the same
  eight captures give **p = 0.25 with two of four pairs reversed**, so the
  change's effect on what the panel shows is **not established in either
  direction**. Every display-side figure originally published for this change is
  retracted. The `rlock` collapse above is a direct measurement and stands.

### Changed

- **Frontend: refresh measurement from redraw intervals is removed.** Shipped
  earlier in this cycle as the fallback for a silent windowing API, it worked
  on light ROMs and failed on the ones that needed it: a redraw interval
  measures the *application*, not the display, so on a ~14 ms/frame commercial
  ROM it reported **20.032 Hz on a 119.991 Hz panel** and display-sync
  correctly refused to engage. No retry schedule fixes a signal measuring the
  wrong quantity — three were tried — so the sampling half is deleted rather
  than left disabled, and `wp_presentation` above replaces it. The median +
  stability-quorum estimator was never the flawed half and is unchanged, now
  shared by both callers along with its tests; `best_divisor`, the
  phase/rate split and the console-rate fallback are untouched.

- **Frontend perf gates: `cost_p95` and `produced_dropped` are reported, not
  enforced; a console-rate gate replaces them.** Both thresholds were derived
  from the contaminated cost metric described under *Fixed* below. `cost_p95` legitimately scales with
  `run_ahead`, which multiplies work by design, and `produced_dropped` is a
  property of the display (the same build and host drops 1–9 frames per 45 s
  under display-sync and 35–131 under the wall-clock fallback), so gating either
  reports the user's hardware as a regression. The new gate checks
  `produced_mean` against the capture's `target_ms` within 0.5% — the one thing
  that is the emulator's own responsibility and is independent of both the
  display and `run_ahead`.

- **Frontend: the framebuffer is no longer re-uploaded when it has not changed.**
  In `Mailbox` present mode the frontend presents faster than the emulator
  produces — measured across the captures in `perf-logs/`, four of six runs show
  137–156 duplicate presents per second against ~60 produced frames, so roughly
  70% of presents were re-sending pixels the GPU texture already held (~35 MB/s
  of redundant staging and copy traffic). The upload is now gated on an FNV-1a
  hash of the frame. No visual change; no frame-time change is claimed.
- **Frontend: the status bar's mapper label is cached at ROM load.** It was
  rebuilt every displayed frame from `Nes::mapper_info()` — which constructs a
  whole debug structure (MMC3's runs ~25 `format!` calls and four `Vec`
  allocations) — inside the emulator lock, keeping only the name. Measured at
  1,367 ns/frame, i.e. 0.008% of a frame: this is allocation hygiene, not an
  optimization.

### Fixed

- **Frontend: a `run_ahead = 3` host reaches a sustainable depth in ~3 s instead
  of ~12 s (v2.3.3 F28).** F27's median window is correct and it cost
  convergence speed: every throttle step waited a full 10 s for the ring to turn
  over, so a configuration starting well over budget stayed there while it
  stepped down. The engage arm now computes rather than waits — within a single
  evaluation it steps while the *predicted* cost at the reduced depth is still
  over the band, using the per-frame-linear model (F18) that the release arm
  already relied on. Releasing is unchanged and still demands a full window and a
  real measurement, because releasing on a stale median is what produced the
  oscillation F27 fixed. The naive reading of "engage faster" is a bug, not a
  fix: at depth 3 the cost exceeds the band at every depth, so engaging on less
  evidence would cascade to depth 0 and discard the feature — the cascade stops
  where the prediction fits, which a test pins directly. Measured at
  `run_ahead = 3` over five paired, Latin-square rounds: convergence **12.12 s →
  2.80 s**, frames held for the wrong duration **4.82% → 2.24%**, 5/5 pairs on
  both, exact one-sided sign p = 0.0312. An alternative arm that cleared the
  produce-cost ring on each depth change converged in 4.0 s and matched on
  cadence but produced an audio underrun in **every** capture, and was rejected.
  Additive-only (65 insertions, 0 deletions) inside the engage branch, which the
  shipped `run_ahead = 1` default never enters.

- **Frontend: the run-ahead throttle no longer oscillates — it was pacing itself
  against a fifth of a median window (v2.3.3 F27).** The throttle is gated to one
  depth change per median window, but the gate used **120** frames — the number
  of samples the produce-cost ring needs before it will report at all — where the
  ring's actual capacity is **600**. A p50 sits at index 300 of 600, so 120
  frames of turnover cannot move it, and a second transition was permitted while
  80% of the median still described the depth the first had just left. Depth then
  walked `2 → 1 → 0` on one measurement, and each change displaces the displayed
  frame by the run-ahead depth — the picture jumping forward and back. Per-window
  logging caught transitions arriving in pairs sharing a median to three decimals
  (`12.958` engaging twice, `4.994` releasing twice); in the release pair the
  unchanged cost is divided by a depth that already changed, so the predicted
  cost halves without any frame getting cheaper. The gate is now expressed in
  terms of the ring it reads, so the two cannot drift apart again. Measured at
  `run_ahead = 2` over three captures: **6-7 transitions per 24 s → 1**, with
  zero spurious releases, and **1.31%** of frames held for the wrong duration
  against a measured 10.7% at depth 2. The release predicate was never wrong —
  its input was: at depth 1 it now reads an honest 8.670 ms (matching the
  independently measured 8.671 ms) instead of a stale 4.994 ms. Frontend-only and
  output-identical; AccuracyCoin and nestest are untouched by construction.

- **Frontend: display-sync consumed a frame slot per redraw without producing a
  frame, whenever the emulation thread was not driving.** `display_produce_due`
  is a stateful mutator — it advances the wall-clock schedule and resets the
  refresh counter — and both the post-present path and `display_sync_produce`
  called it for the same redraw. At divisor 2 the discarded second call could
  advance the schedule by a full period with no frame behind it, so the wall
  clock (the rate authority) outran frame production and the console ran
  **slow**, on exactly the high-refresh panel the divisor exists to serve.
  Ownership now follows `emu_thread_drives()`, the same predicate
  `display_sync_produce` stands down on.
- **Frontend: the F8 work/wait instrument measured the wrong interval, and its
  headline figure was arithmetically invalid.** The wait clock started before
  the render branch, so it spanned the framebuffer copy, the HD composite and
  the egui build in addition to the blocking present; and "render work" was
  derived as `rtot p95 − rwait p95`, a difference of percentiles rather than a
  percentile of the difference. The published table had `work p95` *below*
  `work p50`, which is impossible. The clock now restarts immediately before
  each GPU call, and work is recorded per sample as its own series (`rwork_*`).
  Re-measured, this **reverses the conclusion drawn from it**: render work is
  0.013 ms at p50 but reaches **13–22 ms at p99**, a real tail the broken
  instrument hid and on the strength of which the render loop had been
  eliminated as a suspect. Recorded as an open lead, not a diagnosis — see
  `docs/performance.md` v2.3.3 F8.
- **Frontend: `RenderPerf::clear()` left the wait samples behind**, mixing the
  previous ROM's or regime's blocking-present figures into the next
  experiment's while every other render series started fresh.
- **Frontend: a `DisplaySync::slack` field documented as "half a display
  refresh" was cached but never read**, while the guard it purported to
  describe uses half a *console frame period*. The field is deleted and the
  docs corrected rather than left to contradict the code.
- **Frontend: `close_rom` left a stale cached mapper name** behind the ROM it
  described; it now goes through `EmuCore::clear_rom`.
- **Frontend: `refresh_probe::effective_period` could panic the render loop.**
  `Duration::from_secs_f64` rejects non-finite and negative values, and the
  refresh reaching it is compositor- or API-supplied rather than checked. It
  now returns `Option<Duration>`, matching its sibling `best_divisor`.
- **Frontend: the `wp_presentation` sample set could wedge permanently.** At the
  cap the newest report was dropped rather than the oldest, so a set that
  straddled an output change could never re-form a quorum while still creating
  one feedback object per present for the rest of the session. It is now a
  sliding window.
- **Frontend: `PresentationClock` dropped winit's window before the Wayland
  objects backed by its pointers.** Rust drops fields in declaration order and
  `_window` was declared first; it is now declared last, so the foreign-backed
  `conn`/`queue`/`surface` are gone before the `Arc<Window>` is released.
- **Build: `wayland-client`'s `system` feature is now declared explicitly.**
  `Backend::from_foreign_display` and `ObjectId::from_ptr` require it, and it
  resolved only by unification through winit/sctk/rfd — a change in any of them
  would have turned into a confusing missing-API error.
- **`scripts/perf/perf_capture.sh`: the documented exit-3 "unverifiable
  capture" path was unreachable.** `grep` exiting 1 on an absent header
  propagated out of the command substitution under `set -euo pipefail` and
  killed the script first, with status 1 and no message — in precisely the case
  the message exists for. The metadata reader also imported `tomllib` outside
  its `try`, aborting the whole capture on Python < 3.11 instead of degrading.
- **CI: the PGO workflow's BOLT probe could report a runtime it had not
  linked.** Failed `sudo mkdir`/`ln` left `bolt_dir` populated (the step carries
  no `set -e`), publishing `have_bolt=true` without `libbolt_rt_instr.a`. It now
  verifies the resulting file and prefers the selected toolchain's own prefix
  over a system-wide search.
- **Security: `webbrowser` bumped 1.2.1 → 1.2.4** for RUSTSEC-2026-0257 (Unix
  `BROWSER` argument injection), reached transitively via `egui-winit`'s `links`
  feature.
- **Frontend: display-sync's occlusion watchdog never ran.** `about_to_wait`
  early-returned with `ControlFlow::Wait` whenever the emulation thread drives
  (the default build), and that return sat above the display-sync branch — so
  the watchdog was unreachable in every shipped build. Display-sync is
  self-driving from the present success path, so a compositor that stops
  delivering frame callbacks (minimised or fully occluded window) left nothing
  to re-arm the redraw and nothing scheduled to wake the loop, stopping
  emulation and audio. The stall path is additionally guarded so it does not
  produce frames on the winit thread while the emulation thread is also
  producing.
- **Frontend: the run-ahead throttle could not release without re-engaging.**
  It engaged on produce cost measured *with* run-ahead (>85% of the frame
  budget) but released on cost measured *without* it (<40%) — two different
  quantities, with the hysteresis band sitting between the two states rather
  than spanning them. At depth 2 any ROM whose base cost lands between ~28%
  and 40% of budget oscillated on a ~2 s period, and each toggle shifts the
  displayed frame by the run-ahead depth. Measured at three toggles per 45 s
  on Bad Dudes. Release now predicts the re-enabled cost.
- **Frontend: the display-sync produce phase was a marginal wall-clock test.**
  It re-decided `now + slack >= next` on every refresh with `slack` at half a
  *refresh*, putting the decision boundary 4.167 ms from an 8.334 ms grid, so
  ordinary redraw jitter flipped it between adjacent refreshes. The phase now
  comes from a refresh count, with the wall clock retained as rate guards in
  both directions. Worth ~10% of the produced-interval p95; see
  `docs/performance.md` v2.3.3 F6 for why that is reported as partial.
- **Frontend: dropped frames and stutter traced to display pacing, and fixed.**
  On a 120 Hz host the emulator produced NTSC frames perfectly (`produced_mean`
  measured 16.64 ms in every capture) while the display-synchronised pacer never
  engaged, leaving a free-running wall-clock producer beating against the
  compositor's frame callbacks — 135–254 dropped frames per 45 s. Three causes,
  all fixed: display-sync only ever supported **one emulated frame per refresh**,
  so every 120/144 Hz panel was rejected by construction; refresh detection went
  solely through winit's `current_monitor()`, which reports nothing on a
  compositor that advertises no `wl_output` (and was consulted once at startup
  before the monitor was known, then never revisited); and the regime tied
  console *rate* to present rate, so render-loop hiccups slowed the console.
  Display-sync now selects an integer divisor (120 Hz → one frame per two
  refreshes), can measure the refresh cadence itself when the windowing API is
  silent, and takes its rate from the wall-clock schedule while taking only its
  *phase* from the display. Measured: dropped frames **135–254 → 1–9** per 45 s,
  audio underruns **0–19 → 0**, console-rate error within 0.12%. Frontend-only —
  the deterministic core, save-state and movie formats, and every golden vector
  are untouched.
- **Frontend: display-sync no longer downgrades itself while it is winning.**
  Its sustained-miss fallback tripped on presented-interval p95, a proxy that
  reports the host's compositor rather than whether the regime is working. On
  Super Mario Bros — a materially heavier ROM than the synthetic one the regime
  was tuned against, 13.5 ms of work at `run_ahead = 2` versus 9.2 ms — that
  p95 sat at 25.3–27.2 ms against a 24.96 ms limit, so whether a session kept
  the good regime came down to run-to-run variance, and the fallback is sticky.
  Measured, the regime it fell back to was far worse: 1–15 dropped frames per
  45 s under display-sync against 35–147 under wall-clock. Display-sync now
  falls back on **console-rate error** instead (2% band, a structural safety
  net — the wall-clock rate authority makes a breach a genuine defect), and
  holds 4/4 runs on that ROM at both run-ahead levels with 1–8 drops. `vrr`
  keeps the present-based test, because its failure is the opposite shape: the
  emulator produces correctly at 16.64 ms while the display shows ~20 fps.
- **Frontend: `pacing_mode = "vrr"` no longer collapses on a non-VRR display.**
  It had no sustained-miss fallback, so on a fixed-refresh panel it degraded to
  ~20 fps (49.74 ms presented, 1170 dropped frames in 40 s) and stayed there. It
  now shares display-sync's health check and sticky fallback to wall-clock.
- **Frontend: the producer's mutex wait is no longer billed as emulation cost.**
  The three produce paths started their timer before acquiring the emulator
  mutex, so time blocked on the winit thread was recorded as work — making a
  contention stall indistinguishable from an expensive frame, and pinning the
  reported tail to almost exactly one display refresh. Work and wait are now
  measured separately (new `wait_*` columns in the perf-log CSV). The corrected
  figures show emulation at ~4.1 ms of the 16.639 ms budget with run-ahead off,
  and no mutex contention at any percentile.
- **`scripts/perf/perf_log_check.py` crashed on valid captures.** A run ended
  mid-write (how every timed capture ends) yields a short final row, which
  `csv.DictReader` fills with `None`; `float(None)` raises `TypeError`, which the
  bare `except ValueError` did not catch.
- **The perf-log gate ignored the metric that matters.** It tracked only
  `produced_max_ms` against a 150 ms threshold — nine times the NTSC frame
  budget, and a single sample — so a capture peaking at 128.9 ms with 62
  catch-up bursts passed every threshold it tracked. The gate now trips on
  `catchup_bursts` (200 → **16**) and `snap_forwards` (40 → **8**), both derived
  from the eight captures on file rather than chosen: healthy runs sit at 0
  bursts, the borderline one at 12, the degraded ones at 32 and 62.
  - An absolute-millisecond p99 gate was tried first and **rejected on
    measurement**: p99 tracks the host display's beat against the console rate,
    not frontend health, so it is now reported rather than gated (opt-in flags
    remain for single-machine comparisons). Full figures and reasoning:
    `docs/performance.md`.
- **The BOLT stage reported a speedup it had not measured.** Its bench fallback
  timed a plain, non-BOLT build and labelled the result "BOLT speedup vs plain
  release", and the stage benched the core while BOLT optimizes the frontend
  binary — so it could not have measured its subject in any case. Both gate
  steps are now disabled with the reasoning inline, the runtime probe fails
  closed, and the artifact is named for what it contains
  (`rustynes-bolt-optimized`). **BOLT is deferred and explicitly unmeasured**;
  PGO (measured 6.43% faster and byte-identical) remains the shipping
  optimization. Investigation trail: `docs/performance.md`.

### Performance

- **Run-ahead measurement, corrected.** An earlier entry in this section
  claimed run-ahead "blows the frame budget at the shipped default", citing
  `cost_p95` rising 4.51 ms → 24.15 ms and dropped frames 10 → 303. **Both
  figures were artefacts of the mutex-wait timing bug fixed above**, which billed
  time blocked on the winit thread to the emulator. Corrected, run-ahead costs
  ~6 ms of the 16.639 ms budget at `run_ahead = 1` and ~9.7 ms at the shipped
  `run_ahead = 2`; the dropped frames were display pacing, not run-ahead, and are
  themselves fixed above. Snapshot slimming was then measured directly and is
  **not** the lever it was assumed to be: `snapshot_core_into` is 14.8 µs and
  `restore_quiet` 122 µs, together ~2.2% of a run-ahead frame, so removing the
  245,760-byte framebuffer would buy ~0.66% of the frame budget — below the
  project's standing >3% adoption bar. It retains a real justification on
  rewind-ring *memory*, where the framebuffer is ~94% of every per-frame
  snapshot, and should be argued there rather than on frame time. See
  `docs/performance.md` (v2.3.3 F1).

- **The frontend optimization items were measured before being built, and all
  three are under 0.1% of a frame** — the framebuffer copy chain 13.2 µs
  (0.079%), `perf.view()` for a closed panel 16.2 µs (0.098%), and the
  `mapper_info()` storm 1.4 µs (0.008%). Every claim in the plan was factually
  true and verified in source; they are simply small. The core needs ~3.78 ms of
  the 16.639 ms budget, so the frontend runs with ~12.8 ms of slack and mean
  frame time was never the constraint. See `docs/performance.md` (v2.3.3 F1).

## [2.3.2] - 2026-08-11 - "Lucid" (pixel provenance + replay attestation)

### Added

- **Pixel provenance, phase 1 — per-byte write attribution** (`debug-hooks`,
  default off). Every byte of CIRAM, OAM, and palette RAM now remembers the
  **program counter and CPU cycle of the instruction that last wrote it** — the
  edge that lets the forthcoming provenance panel walk from a pixel on screen
  back to the code that produced it. Nothing in RustyNES recorded this before:
  the Trace Logger has the PC but no effect, the Event Viewer has the write and
  its PPU position but not the PC or the resolved destination, and the memory
  access counter has a cycle stamp but no PC.
  - Recording is **split across the bus/PPU boundary**, because neither side
    knows enough alone: the bus has the program counter and the PPU has the
    effective destination (a `STA $2007` lands in a nametable or in palette RAM
    depending on the PPU's internal `v`). `Nes::run_frame` pushes the executing
    instruction's context down once per instruction, inside the block that
    already performs the breakpoint check — so no new `CpuBus` hook was needed
    and `rustynes-cpu` is untouched.
  - **An OAM DMA burst is attributed to its trigger, not its victim.**
    `STA $4014` only arms the transfer; its 513/514 cycles are stolen from the
    instructions that follow, so the live context would name whichever
    instruction was being halted — true about the timing, wrong about the cause.
    The bus latches the triggering instruction, and all 256 bytes name it.
  - Attribution is **invalidated on power-cycle and on both save-state restore
    paths**: a restored state's bytes were not written by anything this session
    ran, so the honest answer is "no record" rather than a PC from a timeline
    that no longer exists.
  - CHR writes are deliberately **not** attributed (mapper-owned, so a byte
    offset is not a stable identity across a bank switch), nor is the
    `$2004`-during-rendering write the hardware discards.
  - Output-only and lazily allocated: unarmed it costs one `Option` test per PPU
    memory write, armed it costs ~37 KiB. Framebuffer, audio, and cycle counts
    are bit-identical either way, and the default build is unchanged —
    **AccuracyCoin holds at exactly 141/141** with nestest 0-diff.
  - Spec: `docs/pixel-provenance.md`.
- **Pixel provenance, phase 2 — the per-pixel causal record** (`debug-hooks`,
  default off). Every emitted pixel now records the layer that won the priority
  decision, the exact `$3Fxx` palette address behind its color, and the
  nametable / attribute / pattern addresses of the tile **actually on screen**.
  Composes with phase 1: the palette index and nametable address are the keys
  into the write-attribution store, so pixel → byte → writing instruction is one
  chain.
  - **`v` cannot answer the "which tile" question.** By the time a tile's pixels
    reach the screen, `v` has advanced two tiles past it, so an address derived
    from `v` at emit time is wrong for every pixel — and wrong in a way that
    looks plausible. The addresses ride the same `latch` → `next` → `cur`
    cascade that moves the pattern bytes through the shift registers.
  - **A tile is defined when its PATTERN is fetched, not when its nametable byte
    is read.** The PPU performs two dummy nametable fetches at dots 337-340,
    which clobbered the pending tile and made pixels x=8..15 report the tile
    belonging to x=16..23. Found by the test failing, not by review.
  - The attribute address is carried rather than derived, because an MMC5
    vertical split supplies one the standard `$23C0 | ...` arithmetic cannot
    produce.
  - The plan had been to widen the existing `hd-pack` `HdTileSource` gate; that
    was the wrong shape (it carries Mesen HD-pack tile *keys*, not addresses, and
    widening it would have pulled eight fetch-telemetry fields into every
    `debug-hooks` build). A separate lazily-allocated record leaves `hd-pack`
    byte-identical by construction rather than by review.
  - Unarmed cost in `emit_pixel` is one predicted `bool` branch — same shape as
    the bus's existing `event_logging` flag. **AccuracyCoin holds at exactly
    141/141**, nestest 0-diff.
- **Pixel provenance, phase 3 — the inspector panel.** New **Tools → Pixel
  Provenance**: pin a screen pixel and read its whole causal chain — the dot and
  scanline that emitted it, which layer won, the palette entry and the
  instruction that wrote it, the background tile's nametable / attribute /
  pattern addresses with their CIRAM offsets and writing instructions, and the
  winning sprite's slot, priority, and OAM attribution.
  - Detachable into its own OS window through the shared v2.3.0
    `detachable_window` helper. Read-only over the emulator; its only side effect
    is the two arming checkboxes, both default off and both determinism-neutral.
  - PC → source-line resolution reuses the existing `.dbg` source map when one is
    loaded. `Nes::ciram_offset_for_nametable_addr` resolves a nametable address
    to the offset attribution is keyed on, sharing its mirroring resolution with
    the PPU's own fetch path so a per-game mirroring override cannot make the two
    disagree.
  - Frontend-only, so the deterministic core is untouched: **AccuracyCoin holds
    at exactly 141/141**, nestest 0-diff.
- **Deterministic replay attestation** — a `.rnm` movie can now carry a rolling
  hash of its run that anyone else can independently re-derive:
  `rustynes verify <movie.rnm> --rom <game.nes>`. Because the core re-derives
  every pixel from the same ROM and inputs, a third party can replay the movie
  and confirm it reproduces bit-for-bit. The digest is **tamper-evident, not
  forgery-resistant** (64-bit FNV-1a): it catches accidental divergence and
  casual edits, not a motivated forger who recomputes it.
  - **No format-version bump.** `.rnm` already had a precedent for additive
    trailing fields (`rerecord_count`), so the attestation is appended the same
    way behind a marker. `MOVIE_FORMAT_VERSION` stays at 2, every existing movie
    round-trips unchanged, and a pre-v2.3.2 reader parses an attested movie as a
    plain one.
  - **The hash covers the input applied AND the framebuffer it produced**, so
    the record states *these inputs, applied to this ROM, produce this video*.
    Hashing video alone would not pin the input stream at all for a ROM that
    ignores the controller.
  - A checkpoint every 64 frames localizes a mismatch to a 64-frame window rather
    than reporting only a verdict. Exit codes are distinct: 0 verified, 1
    mismatch, 3 not attested — a movie that makes no claim has not failed.
  - Hashing the core snapshot would detect more, and was rejected: the snapshot
    schema is versioned and bumps between releases, which would silently
    invalidate every previously-recorded attestation. Audio is not covered, and
    the docs say so rather than implying it.
  - Recording arms automatically **unless run-ahead is on** — run-ahead presents
    a frame ahead of the persistent timeline, so an attestation recorded under it
    could never verify. If it is toggled on mid-recording the frame counts
    diverge and the tail is dropped at load: the failure mode is "no
    attestation", never "a wrong one".

## [2.3.1] - 2026-08-06 - "Plumb Line" (measurement apparatus + ten measured rejections)

### Performance

- **No emulation-core changes. Ten hot-path optimization candidates were
  measured and all ten rejected**, through six distinct mechanisms: LLVM already
  performed the transformation; the item's premise was factually false; the work
  was real but absorbed off the critical path; the elision was real but bought
  nothing; the target was too small to matter; the ownership model forbids it.
  Full numbers, controls and reasoning are in `docs/performance.md`
  (entries G1–G10). **AccuracyCoin remains at exactly 141/141 and nestest
  0-diff**, verified after every experimental probe was reverted.
- New measurement tooling, all of which found something the previous apparatus
  could not:
  - `crates/rustynes-test-harness/src/bin/frame_probe.rs` — harness-free
    steady-state frame cost, with no criterion in the process image (criterion's
    own rayon/`exp`/sort work had been ~17% of every profile).
  - `scripts/perf/frame_breakdown.sh` — per-subsystem attribution by **source
    file**, which recovers work the symbol profile hides. It shows the **APU at
    18.7% of frame time**, invisible under `perf report` because fat LTO inlines
    it wholesale into `cpu_clock` (`perf report --inline` does not recover it).
  - `scripts/perf/ab_check.sh` — adoption A/B with an **A/B/A order-bias
    control**: the reference is benched a third time, last, against its own first
    run, so drift from position-in-the-run is reported rather than mistaken for a
    result.
- `scripts/bench_relative_check.sh` now declines to emit a verdict when the host
  was too noisy to resolve the effect it tests for, keyed on a robust
  MAD-based coefficient of variation.

### Fixed

- **The PGO workflow's BOLT probe reported success without BOLT present.** It ran
  `apt-get install bolt` and trusted the exit status — but on Ubuntu that package
  is the *Thunderbolt 3 device manager*, an unrelated project that owns the name.
  The stage then failed on the tool it had just "confirmed", instead of skipping
  as its best-effort contract intends. The probe now locates the actual
  `llvm-bolt` binary and reports honestly when it is absent.

### Documentation

- `docs/performance.md` records every rejected experiment with its numbers, its
  order-bias control, and the mechanism behind the null result — including two
  near-misses that a single measurement would have adopted.

## [2.3.0] - 2026-08-05 - "Datum II" (PPU-accuracy capstone + true multi-viewport tool windows)

Closes the **v2.2.6 → v2.3.0 NESdev-remediation line**. Both remaining
forum-reported accuracy concerns were investigated and found *already correct*;
the release's substance turned out to be elsewhere — real OS-window tool panels,
and a frame-pacing defect that had been degrading every session with a debugger
panel open.

### Added

- **True multi-viewport tool-window detach.** Every tool panel can now pop out
  into a *real OS window* (native), finally resolving the Windows-10
  "trapped window" report. v2.2.9's affordance only *embedded* the panel, because
  `show_viewport_immediate` needs a multi-viewport integration to produce a real
  window. The new `rustynes-frontend/src/detached.rs` gives each detached panel
  its own winit window + egui context/state/renderer + wgpu surface, sharing the
  main device — **with no `unsafe`**, unlike eframe's immediate-viewport path
  (which erases an `ActiveEventLoop` lifetime into a `'static` thread-local).
  The nine panels that predated the shared helper (CPU, Cartridge Info, Lua
  Script, BasicBot, Input bindings, TAStudio, Settings, Netplay,
  RetroAchievements) were converted, so *all* tool windows are detachable.
  Detached windows inherit the main window's theme, zoom, and locale, and open at
  the size their docked window actually had. wasm keeps panels docked.

### Fixed

- **Frame stutter / high produced-interval p99 whenever a debugger or tool panel
  was open.** The overlay-visible render path held the emulator mutex "until
  after the present call" — and the *blocking* `Surface::get_current_texture`
  runs before the egui pass — so the winit thread owned the lock across a
  swapchain wait, the whole egui build, the encode and the present, while the
  emulation thread sat parked on `emu.lock()` unable to produce a frame.
  `render_shell` is split into `run_shell_ui` (locked, needs `&mut Nes`) and
  `paint_shell` (unlocked GPU work); the guard now covers only the UI build.
- **`pace_frames` took the emulator mutex on every `about_to_wait` iteration** —
  a tight spin in the wall-clock regime — which could block the UI thread for a
  full produce (~4 ms) each time. It now reads the lock-free `EmuControl::has_rom`
  atomic, falling back to the locked read only when no emulation thread exists.

### Changed

- **PPU per-dot helpers optimized: −5.13% / −3.51% frame cost** (nestest /
  flowing-palette, both clearing the project's >3% adoption bar, p = 0.00).
  `perf annotate` showed `tick_sprite_eval_per_dot`'s own `push`/`ret` were the
  two hottest instructions in its body — pure call overhead across 89,342
  calls/frame — and that `tick_oam_bus` derived values it discarded before its
  dot-0 early-out. Byte-identical: AccuracyCoin **141/141**, nestest 0-diff, PPU
  units 91/91. Documented as `v2.3.0 P1` in `docs/performance.md`.
- **Detached panels repaint on per-panel tiers** — Live (60 Hz) for
  continuously-changing state, Throttled (~10 Hz) for status, and
  interaction-only for static panels (Cheats, ROM Info, Settings) — so a wall of
  open tool windows costs almost nothing while idle.
- **AccuracyCoin gate pinned to an exact 141/141.** The gate asserted only a
  coarse 60% floor — too loose to catch a single-test regression (an A/B probe
  disabling the delayed-`CopyV` dropped exactly the Hybrid Addresses test to
  140/141 yet still cleared 60%). It now also asserts zero failing tests.
- **The `≤ 2 ms` frame-cost figure is now labeled a design-phase aspiration, not
  a gate.** It was written before the cycle-accurate core existed; the core
  measures ~3.8 ms (~23% of the NTSC budget) and that is knowingly accepted. The
  remaining bulk is accuracy-required work, and the obvious levers were already
  measured and *rejected* (`emit_pixel` elision and the SIMD blitter were both
  **slower**). Recorded so no contributor optimizes toward it by trading accuracy.
- **libretro core license declared as `GPLv3`**, matching the notation mesen /
  melonDS / bsnes use (SPDX `GPL-3.0-or-later` stays in the Cargo metadata).

### Verified (no change required)

- **SMB left-edge and the hybrid-address (Rad Racer) render.** Both were
  investigated under reproduce-before-fixing discipline and found *already
  correct* in the shipped build — resolved by the v2.0.0 "Timebase" rewrite and
  the v2.0.3 2-cycle-ALE promotion, predating the report. SMB's leftmost
  background column renders real content; the hybrid-address model passes the
  authoritative AccuracyCoin test and renders Rad Racer cleanly. See ADR 0030's
  v2.3.0 update.

### Documentation

- **Hybrid-address provenance finalized** — `NOTICE`, `docs/originality-and-provenance.md`
  §4 and ADR 0030 move from "TriCNES-calibrated, being reworked" to "verified
  correct, documentation/oracle-derived".
- **TriCNES is no longer described as "transistor-level"** — it is a
  cycle-accurate C# emulator with a sub-cycle state machine. The term properly
  denotes die-derived simulations (`Visual2C02` / `phantom2c02`), which the repo
  cites correctly. Corrected in source, `NOTICE`, README, ADR 0030, and the
  published v2.0.2 / v2.2.5 release notes.
- **GeraNES reference comments corrected** — dangling source-file paths and a
  quoted C++ line were reworded to state honestly that its source was consulted
  as a cross-reference for nesdev-documented behavior, with no code copied
  (reviewed two-sided against the upstream source and the nesdev register maps).
  Recorded as an assessment for expert review, not a self-certification.

## [2.2.9] - 2026-08-04 - "Studio II" (relicense to GPLv3 + TAS/movie wiring + detachable tool windows)

The fourth step of the **v2.2.6 → v2.3.0** NESdev-remediation line. Its headline is
a **licensing and provenance correction** — RustyNES is **relicensed to
GPL-3.0-or-later** as the derivative work of GPL emulators it is — alongside three
forum-reported fixes (TAStudio edits, `.bk2` playback, tool-window detachment). The
code changes are frontend-only, so the deterministic chip stack, save-states, and
every golden vector are byte-identical: **AccuracyCoin 141/141, nestest 0-diff**.

> **Windowing — honest scope.** The detach affordance is **native-only and currently
> *embeds***: the frontend is a single-viewport `egui_winit` integration, so
> `show_viewport_immediate` renders a detached panel *inside* the main window, not as
> a separate OS window — so this does **not** yet fully resolve the Windows-10
> "trapped window" report. True OS-window detach needs multi-viewport render-loop
> wiring, tracked as a v2.3.0 follow-up.

### Changed — License: MIT/Apache-2.0 → GPL-3.0-or-later

- **RustyNES is relicensed to GPL-3.0-or-later** (ADR 0036). A NESdev community review
  established that it **incorporates code derived from GPL emulators** — principally
  **Mesen2** (GPL-3.0-or-later: CPU unstable stores, PPU sprite-eval/OAM, ~15 mapper
  boards, Bisqwit NTSC tables, EEPROM/UNIF/debug-symbol/PGO code) and, for several
  mappers and the FDS drive model, **puNES / FCEUX / Nestopia** (GPL-2.0-or-later).
  The pre-v2.2.5 comments said as much ("Faithful port of Mesen2's …"); the v2.2.5
  "no GPL source incorporated" / MIT-Apache position was wrong and is withdrawn.
- **Credit is given, per subsystem.** `docs/originality-and-provenance.md` leads with
  the file-by-file derivation table; `NOTICE` attributes every GPL upstream; each
  derived file carries an accurate `SPDX-License-Identifier` + provenance header (the
  old imprecise "port of" comments are not restored — the headers are their accurate
  replacement). Incorporated permissive components (emu2413/MIT, TriCNES/MIT,
  rcheevos/MIT, blip_buf/LGPL-2.1-or-later, fonts) keep their notices. Zero
  emulation-core behavior change.

### Fixed

- **TAStudio piano-roll edits now drive the emulator.** `handle_tas_requests` applied
  `SetInput` to the editor's `input_log` only and never re-seeked the `Nes`; it now
  re-derives through `TasEditor::seek` after the batch, matching the scripting bridge.
- **`.bk2` playback honors the movie's `LogKey` column order.** The importer mapped
  columns by a fixed order and ignored the `LogKey:` header, driving the wrong buttons
  on movies ordered differently; it now parses the real order (falling back to the
  standard order when absent) and surfaces parse errors on the status bar.

### Added — Provenance & license firewall (+ import hardening)

- **Guardrails ruleset + post-mortem.** `docs/ai-emulator-provenance-guardrails.md` (a
  preventive, console-agnostic reference-firewall / attribution / license ruleset,
  shared as community best-guidance) and `docs/provenance-failure-postmortem.md` (the
  forensic root-cause of how GPL code was reproduced despite a black-box instruction,
  then laundered, and how it was corrected). Themed PDFs of both in `ref-docs/`.
  Ingested into `AGENTS.md` as the top development rule.
- **Reference firewall — the reference-emulator clone removed.** The local clone is
  deleted from disk and stays gitignored (+ excluded from dockerignore /
  markdownlintignore / pre-commit / CodeRabbit) so the *copyleft* references' source
  is out of reach; in-source citations were normalized to upstream form (comments-only,
  byte-identical), and the §1 derivation table audited for completeness. MIT TriCNES
  is the deliberate exception, vendored in-repo with attribution.
- **`.bk2` import hardened against a `LogKey` allocation-amplification DoS** —
  `parse_log_key` reads only the console/P1/P2 groups from the `split('#')` iterator
  instead of collecting every `#`-group; behavior is identical for valid movies (new
  `log_key_bounded_against_pathological_group_padding` regression test).
- **Detachable / floating tool windows (native).** A shared `detachable_window` helper
  gives 18 debugger/tool panels a "⧉ Detach" button via `show_viewport_immediate`,
  each preserving its geometry (wasm stays docked). Currently embeds rather than
  opening a separate OS window — see the honest-scope note above.

## [2.2.8] - 2026-08-04 - "Aperture II" (gamma-aware scanlines + sharper CRT)

A **presentation-fidelity** release addressing the NESdev-forum feedback on
gamma-aware resampling and bilinear-soft scanlines. **Presentation-only — nothing
here touches the emulation core**, so the pre-shader framebuffer, save-states, and
every golden vector are byte-identical (AccuracyCoin 141/141, nestest 0-diff), and
the **shipped native default is byte-identical** to v2.2.7 (the native sRGB
surface passes `aux = 0`, which selects the exact pre-v2.2.8 scanline profile;
the new linear-light + sharper-scanline path activates only for a non-zero
`aux`, set on the WebGL2 non-sRGB path and when the scanline knob is raised).
The base BLEP audio and the advanced CRT stacks
(royale/guest/megatron, already gamma-correct) are untouched.

> **Visual verification pending.** These are shader/appearance changes; naga
> validates that the WGSL compiles and the native/wasm builds are clean, but the
> on-screen result must be confirmed on a real display + a browser (WebGL2).

### Changed

- **Gamma-correct scanlines + aperture mask (base CRT pass, `CRT_WGSL`).** The
  scanline/mask *darkening* now happens in **linear light**. On the native path
  (sRGB texture + surface) the sampler/surface already convert, so the shader
  leaves it linear (`aux.y = 0`, output byte-identical). On a plain UNORM path
  (**WebGL2**, which does neither) the shader now sRGB-decodes on read and
  re-encodes before output (`aux.y = 1`) — fixing a real browser-only gamma bug so
  a scanline valley is 50% of the *linear* luminance, not the encoded value. The
  round-trip uses the **exact IEC 61966-2-1 piecewise sRGB transfer** (the
  `0.04045` / `0.0031308` breakpoints + a 2.4 exponent), not a `pow(2.2)`
  approximation, so the WebGL2 result matches the hardware sRGB surface the native
  path uses bit-for-bit.
- **Sharper scanlines (`aux.x`, default 0.5).** The scanline profile blends from
  the original soft parabola (0) to a narrow Gaussian beam (1) for crisp vertical
  boundaries instead of the linear-sampler blur — the sharper scanlines the
  feedback asked for. Only visible when scanlines are enabled; `aux.x = 0`
  reproduces the pre-v2.2.8 profile exactly. Wired on the desktop
  (`rustynes-frontend`), Android (`rustynes-android`), and iOS Metal
  (`rustynes-ios`) hosts via the shared 16-float CRT uniform
  (`rect + crop + params + aux`) — all three set `aux` identically for the
  scanline/CRT filters, so the corrected profile is consistent across platforms.

## [2.2.7] - 2026-08-04 - "Timbre II" (expansion-audio fidelity: VRC6 + Sunsoft 5B)

An **expansion-audio accuracy** release addressing NESdev-forum feedback. Driven by a
measure-first cross-reference of VRC6 and Sunsoft 5B against **11 reference emulators**
(Mesen2/MesenCE, ares, higan, nestopia, fceux, tetanes, rustico, GeraNES, puNES, BizHawk)
plus the NESdev wiki — because a Mesen2-only comparison hides where Mesen2 itself is the
outlier. **The base 2A03 output is byte-identical** (these are expansion-only changes:
`mix_audio()==0` for non-expansion mappers), so **AccuracyCoin holds 141/141 (100.00%)**,
nestest is 0-diff, and blargg/kevtris are unchanged. The base BLEP decimator was
independently verified excellent (SFDR **81.6 dB**, `rustynes-apu` spectral test).

### Changed

- **VRC6 level recalibrated to the field/hardware consensus** — a full-volume VRC6 pulse
  is now **≈1.0×** a 2A03 pulse (was ~1.506×). `VRC6_MIX_SCALE` 979 → 650. The prior
  1.506× mirrored **Mesen2's specifically louder mixer convention** (Mesen2 weights VRC6
  `×5`); a reviewer flagged VRC6 as too loud, and the cross-reference confirmed Mesen2 is
  the loud outlier: the NESdev wiki says the VRC6 pulses are "roughly equivalent to the
  pulse channels of the 2A03", and rustico / tetanes / BizHawk encode a VRC6 pulse == a
  2A03 pulse *exactly* (ares/higan/nestopia reach the same via `sum/61`). The `db_vrc6a/b`
  oracle target moved 1.506 → 1.000 and the two snapshots were re-blessed (audio-only —
  framebuffer + cycle count byte-identical). VRC6's per-channel balance (linear
  `pulse+pulse+saw`, saw 0–31 vs pulse 0–15) was already correct and is unchanged.
- **Sunsoft 5B envelope now uses the exact 5-bit 1.5 dB/step DAC** — the envelope-mode
  amplitude path indexes a new 32-level `SUNSOFT5B_LOG_VOL32` table (×1.1885/step = +1.5 dB,
  matching nestopia/rustico) at full 5-bit resolution, instead of truncating the envelope
  to 4-bit (the wiki-named 3 dB approximation). Fixed 4-bit volume tones (already correct
  3 dB/step) and the 5B absolute level (1.265×) are unchanged; the odd entries of the
  32-level table equal the 4-bit table exactly (guarded by a new unit test). Extant 5B
  test-ROM snapshots stay byte-identical; envelope-modulated 5B music now gets the exact
  curve.

## [2.2.6] - 2026-08-04 - "Almanac" (de-monetization + provenance accuracy)

A **de-monetization and provenance** release. RustyNES is now permanently
open-source and **income/profit-free forever** (ADR 0035): all planned
monetization is removed and the native Android/iOS apps are kept as **free FOSS
apps** — no ads, no tracking, no paid unlock, every feature available. **Zero
emulation-core behavior changes** — the `#![no_std]` chip stack, save-state / TAS
/ netplay formats, and every golden vector are byte-identical, so **AccuracyCoin
holds 141/141 (100.00%)** and nestest is 0-diff by construction.

### Removed

- The `rustynes-monetization` crate and `docs/monetization/` are deleted and the
  workspace member removed (no emulation crate ever depended on it). The Android
  paid layer (Play Billing `LicenseManager`, the AppLovin MAX / RevenueCat
  `MonetizationGate` + ad gates, the demo/paywall UI + strings, AdMob/AppLovin
  manifest entries, and the billing/ad Gradle deps + BuildConfig keys + the
  monetization cargo/uniffi tasks) is removed; `MainActivity` no longer gates any
  feature behind an unlock/demo. The iOS paid layer (the StoreKit `StoreManager`,
  the `appStore` monetization build channel, billing entitlements) is removed.

### Changed

- The `foss` / `play` Android flavor split is retained but now only distinguishes
  the pure-AOSP build from the build carrying the *free* Google-Play services
  (Play Games achievements, Cast, Integrity, in-app update, cloud save) — no ads,
  no billing. Nightly Rust is now used only by `cargo fuzz`.
- ROADMAP / `docs/STATUS.md` / version plans reframed to the OSS/income-free
  position; the freed v2.3.0 slot is repurposed for accuracy/fidelity work.
- **Provenance accuracy (ADR 0035 + ADR 0030):** `NOTICE` and
  `docs/originality-and-provenance.md` now disclose honestly that the PPU
  octal-latch / hybrid-address *timing* was calibrated to TriCNES's per-dot
  behavior (beyond black-box oracle use), which reproduced a TriCNES-specific
  artifact that mis-renders mid-render `$2006` writes (e.g. Rad Racer). This is
  scheduled to be reworked to be documentation-derived in v2.3.0.

### Added

- **ADR 0035** "RustyNES is permanently non-commercial (no monetization)";
  **ADR 0025** marked Superseded and **ADR 0027** amended (its App-Store §4.7
  ROM-compliance rules stay — valid for a free app; the ad/ATT/StoreKit-unlock
  clauses are removed).

## [2.2.5] - 2026-08-03 - "Colophon" (provenance, licensing, and documentation integrity)

A **provenance, licensing, and documentation-integrity** release, prompted by
community review of the project's licensing and AI-assisted origins. **Zero
emulation-core behavior changes**, so **AccuracyCoin holds 141/141 (100.00%)**, nestest is
0-diff, and the `#![no_std]` chip stack, save-state / TAS / netplay formats, and
every golden vector are byte-identical to v2.2.4 by construction.

### Changed

- **In-source "port" comments corrected.** A full-tree audit found comments that
  described implementations of publicly-documented hardware behavior (the CPU
  unstable-store opcodes, the PPU sprite-evaluation / OAM models, and numerous
  mapper register decoders) as "ports of" copyleft emulators (Mesen2 — GPLv3;
  puNES — GPLv2). Those behaviors are implemented from the NESdev wiki, published
  datasheets, and the documented 6502 behavior, and were cross-checked against
  reference emulators as *oracles*; the comments were reworded to say so. No
  GPL-licensed emulator source is incorporated.
- **CRT shaders & NTSC filters reworded.** `crt_royale` / `crt_guest` / `megatron`
  and the Bisqwit / EMMIR NTSC filters were reviewed at source level and reframed
  from "port / condensation of X" to independent single-pass reimplementations of
  the *look and technique* (copyright protects code expression, not a visual look);
  no upstream shader source is incorporated. The comment claiming tables were
  "ported verbatim from Bisqwit's C" was corrected — those tables encode the
  NESdev-documented NES composite signal.
- **`blip.rs`** no longer mislabels `blip_buf` as BSD/MIT (it is LGPL-2.1+); the
  file is an independent BLEP implementation and now says so.
- **README** toned down and corrected: added an AI-assistance disclosure, removed
  a comparison graphic with inaccurate details, fixed a mislabeled
  ("sub-cycle accuracy") screenshot caption, and synced Acknowledgments with
  `NOTICE`.

### Added

- **`NOTICE` rewritten** to disclose the behavioral-oracle use of GPL emulators
  (Mesen2/MesenCE, higan, **GeraNES**, ares, FCEUX, Nestopia UE, puNES — no code
  incorporated), attribute the incorporated permissive components (emu2413,
  TriCNES, rcheevos — all MIT, with the MIT text), the bundled fonts (Font Awesome;
  Press Start 2P / OFL) and test ROMs, and credit the CRT-shader / NTSC-filter
  visual influences as independent reimplementations. GeraNES (GPL-3.0-only), cited
  across ~58 files, was previously undisclosed.
- **New `docs/originality-and-provenance.md`** — an honest account of where
  RustyNES advances, diverges from, or independently re-derives NES emulation
  technique, its development timeline, and its full license posture (including that
  the project is heavily AI-assisted).
- **Press Start 2P OFL text** added to the Android app assets (it shipped without
  the required OFL text; desktop and iOS already carried it).

### Fixed

- **`tests/roms/LICENSES.md`** — a false exclusion claim (four Holy Mapperel mapper
  ROMs stated as excluded were in fact committed), a stale crate path, and the
  AccuracyCoin sub-test count; and added blanket coverage for the committed
  directories not individually tabulated (328 committed `.nes` total, none
  commercial).

## [2.2.4] - 2026-07-24 - "Cartridge" (libretro core builds/installs for RetroArch)

A **libretro / RetroArch distribution** cut. Its purpose is that the RustyNES
core builds and installs cleanly through the Libretro buildbot
(<https://git.libretro.com/libretro/RustyNES>) so RetroArch users can pull it
from the in-app core downloader. **Zero emulation-core changes**, so
AccuracyCoin holds **141/141 (100.00%)**, nestest is 0-diff, and the `#![no_std]`
chip stack, save-state / TAS / netplay formats, and every golden vector are
byte-identical to v2.2.3 by construction.

### Libretro / distribution

- **The libretro core is confirmed complete and up-to-date with every recent
  change, and builds for the buildbot ABIs.** `crates/rustynes-libretro` wraps
  `rustynes-core`, so it inherits the v2.2.3 work automatically and required no
  code change to carry it: the fast PPU dot path (now the core default) is
  active; the `PPU_SNAPSHOT_VERSION` 8 + APU v4 save-state schema is transparent
  because `get_serialize_size` / `on_serialize` size and emit the *current*
  snapshot via `Nes::snapshot_core_into` rather than a hardcoded layout; the
  `Mapper::mix_audio` i32 widening, the Zapper model, and the `mNNN_` mapper
  rename are all below the crate's public dependency surface. Both buildbot
  cross-ABIs the GitHub early-warning gate models — `x86_64-pc-windows-gnu` and
  `aarch64-linux-android` — `cargo check --release -p rustynes-libretro`
  clean.
- **`rustynes_libretro.info` metadata corrected** (the file RetroArch's core
  downloader reads to learn the core's capabilities):
  - **`disk_control` `false` → `true`** — the real fix. The FDS multi-side Disk
    Control interface (`enable_disk_control_interface()` + the
    `on_set_eject_state` / `on_get_image_index` / … callback trampolines) has
    been wired since the buildbot recipe landed, but the `.info` advertised it
    as absent, so RetroArch's Quick Menu → Disk Control never surfaced multi-disk
    FDS swapping.
  - `display_version` `v1.0.0` → `v2.2.4` (stale since the v1.0.0 era).
  - Description mapper count `168` → `172`, and a note that FDS multi-disk
    swapping runs through the Disk Control interface.
- Documented follow-up: libretro **core options** (region / overscan / palette /
  accuracy toggles) remain unexposed. `core_options = "false"` is accurate, not
  stale — a deliberate future enhancement, not a v2.2.4 gap.

### Tooling

- **The Antigravity PR reviewer is standardized onto the shared template**
  (`scripts/agy-review.sh` + `.github/workflows/antigravity-review.yml`), the
  same canonical version now installed across RustyNES / RustySNES / RustyN64.
  It carries the large-diff handling (a diff too big to inline goes to `agy` as
  a file), the 20,000-line `gh pr diff` API-limit local-`git diff` fallback, the
  `isCrossRepository` fork gate, fail-closed metadata, default-branch checkout
  with `persist-credentials: false`, and the `synchronize` auto-re-review
  trigger.
- **Reviewer security hardening (found by the reviewer itself).** The
  Antigravity reviewer, run against this PR, flagged five security regressions
  the standardized template had relative to RustyNES's prior version — all
  fixed: `printf '%q '`-escaped `script(1)` fallback (was a raw `${flags[*]}` in
  `sh -c` — command injection), an author-scoped comment-deletion filter (was
  marker-only — arbitrary comment deletion), removal of the unscoped SQLite
  conversation-store fallback (a shared-runner data-leak vector), stripping
  `GH_TOKEN` / `GITHUB_TOKEN` from `agy`'s environment (`env -u`), and the
  `issue_comment` author-association re-check restored in the script.
- **Large-diff handoff made readable (found by the reviewer, fixed on the
  runner).** The reviewer files a diff too large to inline into a gitignored
  working-tree scratch dir (`.agy-review-work/`, relocated from `.git/` after the
  reviewer flagged the hidden-dir read risk) and tells `agy` to read it — but
  that on-disk handoff never actually worked: `agy`'s sandboxed file tool
  resolves relative paths against its own workspace root, not the shell CWD, so
  the file came back "does not exist" and the review was empty. Latent until a
  diff first crossed the ~90 KB inline budget (below it the diff is inlined and
  the file path is never exercised). Root cause and fix proven on the live `agy`
  runner with a three-way probe under the exact review flags: the prompt now
  hands `agy` an **absolute** path, and `--add-dir "$PWD"` adds the checkout to
  `agy`'s sandbox workspace — but only in file-handoff mode, so an inline review
  keeps zero filesystem access. All changes ride the canonical template so the
  three consuming repos stay in sync.

## [2.2.3] - 2026-07-23 - "Datum" (fast dot path promoted + PGO shipped + the last two mapper residuals closed)

A performance and accuracy-closure patch. No *regression* on the deterministic core —
**AccuracyCoin 141/141, nestest 0-diff**, `visual_regression` and the APU oracles unmoved.
(This release does change shipped-default behavior by design: the fast PPU dot path became
the default, two Holy Mapperel mapper residuals were closed, the Sunsoft 5B level was
calibrated, and the save-state schema gained `PPU_SNAPSHOT_VERSION` 8 + an APU v4 tail —
each an intentional, oracle-gated change, detailed below.)

### Performance

- **The specialized PPU fast dot path is now the default** (~−11% frame time on
  rendering-heavy content; `nes_run_frame_nestest` 4.43 ms → 3.93 ms). Differential-tested
  byte-identical every frame since v2.1.8 (`fast_dotloop_diff.rs`); it had shipped off
  with no reachable caller. A `[emulation] fast_dotloop` escape hatch defaults on.
- **Release builds ship the PGO binary on `x86_64-unknown-linux-gnu`.** `release.yml`
  now consumes the profile-guided build behind the existing >3%-faster-and-byte-identical
  gate; a gate miss silently keeps the plain asset (macOS/Windows unchanged).
- **A same-runner relative frame-time regression gate** (`bench_relative_check.sh`) now
  fails a >10% back-to-back slowdown, closing the hole where the loose absolute ceiling
  let a 2.5× slowdown pass.

### Fixed

- **The last two Holy Mapperel residuals are closed — all 17 ROMs report `detail=0000`**:
  MMC1's two software WRAM write-protect layers (`$E000` bit 4 + SNROM's `chr_is_ram`
  CHR-register layer) and FME-7's open bus on the RAM-selected-but-disabled window, both
  via the trait's `cpu_read_unmapped` contract. Validated 60/60 commercial (incl. seven
  battery-backed MMC1 saves) + 138/138 extended.
- **Sunsoft 5B expansion audio calibrated (~23 dB louder).** The DAC shape was already
  exact; the level was blocked by `Mapper::mix_audio` returning `i16` (full-scale 5B =
  34,761). Widened to **`i32`** and calibrated against Mesen2 (`db_5b` 0.069× → 1.265×);
  `nsf_expansion::mix` likewise widened + unclamped. Every other board is byte-identical.
- **Run-ahead cost three AccuracyCoin tests** (138/141 in-app vs 141 headless): the PPU
  snapshot omitted the sprite-evaluation FSM + OAM-data-bus state. A new
  `PPU_SNAPSHOT_VERSION` **v8** tail restores **141/141 with run-ahead on**; an APU **v4**
  tail closes a matching warm-reset `$4017` gap. Netplay/TAS take the same round-trip.
- Seven stale commercial-oracle audio rows re-blessed (level constants changed in
  v2.1.6 / this line; a new `expansion_level_tripwire` CI test pins them), and the
  expansion-audio snapshot window widened so it actually observes the expansion chip.

### Changed

- **Mapper modules renamed for the board they emulate** (`sprintN.rs` → `mNNN_<board>.rs`,
  27,631 lines, ~110 boards) — proven content-preserving by a byte-for-byte item
  comparison (930 items, 0 altered) and an identical 172-ID dispatch table.
- `PPU_SNAPSHOT_VERSION` 7 → 8 **breaks pre-v8 `.rns` save states** (clear
  `VersionMismatch`, per ADR 0028); movies/netplay re-derive from power-on and are
  unaffected.

### Added

- **Two optimizations measured and REJECTED, documented with their numbers**
  (`docs/performance.md`): `ppu-idle-line-fast` (made the shipped default slower — off)
  and the P4 `cpu_clock` levers (already implemented; remaining lever ≤1.9%).
- A **save-state schema audit** standing test (`snapshot_schema_audit.rs`) that fails if
  a chip field is added without its serializer — the mechanical net that found the v8/v4
  gaps above.
- An opt-in **Zapper beam-relative light model** (default off; no pass/fail light-gun ROM
  exists to adjudicate it) and the Antigravity self-hosted PR reviewer (CI-only).

Full detail: the GitHub Release and `.github/release-notes/v2.2.3.md`.

## [2.2.2] - 2026-07-21 - "Conduit" (libretro buildbot 10/10 + CI supply-chain hardening + single-source toolchain)

A **build, distribution, and CI-integrity patch**. **Zero emulation-core changes** — no
file under `crates/rustynes-{cpu,ppu,apu,mappers,core}` is touched, so **AccuracyCoin
holds 141/141 (100.00%)** by construction, nestest 0-diff, and `pal_apu_tests` 10/10 /
`visual_regression` / the 60-ROM oracle are unchanged from v2.2.1. The one behavioral
improvement in a shipped artifact: the libretro **tvOS** core now builds with
`panic = "abort"` like every other platform.

### Fixed

- **Libretro buildbot: 1 of 10 jobs green → all ten building** (the last step before
  RustyNES lands in RetroArch's built-in core downloader). Three independent, our-side
  defects, all invisible until the buildbot ran: 8 jobs missing cross-compile targets
  (each now `rustup target add ${RUST_TARGET}` into the pinned toolchain); the upstream
  `rust-libretro 0.3.2` MinGW keycode-signedness bug (worked around by pointing bindgen's
  clang at the MSVC triple for the `-gnu` targets); and the tvOS `+nightly -Zbuild-std`
  override, now dropped since `aarch64-apple-tvos` ships a complete prebuilt std
  (`panic_abort` included).

### Security

- **`persist-credentials: false` on all 19 CI checkouts** (closes #318) —
  `actions/checkout` otherwise writes `GITHUB_TOKEN` into `.git/config`, readable by the
  unreviewed PR code nearly every job builds/runs. Audited: no job needs the credential;
  the highest-exposure site was `web.yml`'s `build` (`pages: write` + `id-token: write`).
- **The release tag-existence check is now fail-closed** (`release-auto.yml`): a
  `gh api git/matching-refs` call that can never confuse "lookup failed" with "tag
  absent" (which used to risk re-releasing a shipped version), with an explicit
  non-array guard.
- **`dtolnay/rust-toolchain` SHA-pinned** off the moving `@master` branch (it installs
  the compiler and feeds 12 of 19 checkouts), keeping the Dependabot-readable `# v1`
  marker.

### Changed

- **One toolchain everywhere: `rust-toolchain.toml`'s `channel` is the single CI source
  of truth.** `rust-setup` parses it and fails closed; no toolchain version literal
  remains under `.github/`, and **no `nightly` on any build path** (nightly survives
  only for `cargo fuzz`).
- **New `libretro-cross` CI job** cross-checks `rustynes-libretro` against the buildbot
  ABI families a Linux runner can model (MinGW-Windows, Android/NDK) — the early-warning
  gate that was previously absent.
- **Dependabot #313–#315 consolidated** into one reviewed change plus a `cargo update`
  sweep, no source changes (lz4_flex 0.13 → 0.14 with an explicit `alloc` feature,
  tokio 1.52.3 → 1.53.1, and the production-dependencies group).

### Added

- The libretro buildbot recipe (`.gitlab-ci.yml`, issue #311) covering all ten platform
  jobs, plus libretro core feature completion (native memory-maps for `rcheevos`, an FDS
  load-path fix + multi-side disk-control, native Game Genie cheats, fast-forward
  audio-skip).

Full detail: the GitHub Release and `.github/release-notes/v2.2.2.md`.

## [2.2.1] - 2026-07-15 - Housekeeping patch (dev-tooling archival + dependency consolidation + FDS test corpus)

Zero accuracy, feature, or core changes — the deterministic `#![no_std]` chip
stack, save-state / TAS / netplay-replay formats, and every golden vector are
untouched. AccuracyCoin holds **141/141 (100.00%)**, unchanged from v2.2.0.

### Added

- **Game Genie re-key research tooling archived (PR #304).** Preserved the six
  intermediate research / verification scripts behind the header-robust Game Genie
  code re-key (which shipped in #262) beside the generator in `scripts/gg/`:
  `crc_combine.py` (a pure-Python `zlib.crc32_combine` implementation,
  self-tested against 2000 random synthetic trials, underpinning the
  `rom_crc32 == crc32_combine(prgCRC, chrCRC, chrLen)` identity),
  `alias_resolve.py` (long-tail title-alias CRC resolution), `coverage.py` /
  `coverage2.py` (name-join coverage accounting), `inspect.py`, and `verify.py`
  (which now proves the combine identity over every standard nes20db cart dump).
  Dev / research tooling only — paths resolve repo-relative, and it touches no
  crate and does not affect the build or the deterministic core.

- **`TakuikaNinja` FDS hardware-verification probes wired in (gated,
  gitignored).** Added `crates/rustynes-test-harness/tests/fds_takuikaninja.rs`
  with four `RUSTYNES_FDS_BIOS`-gated smoke tests against
  `FDS-Mirroring-Test`, `FDS-4023-Test`, `FDS-Audio-Registers`, and
  `FDS-4030D1-Addr` — real hardware-verified probes of `$4023`/mirroring/audio
  register behavior and the FDS DRAM-refresh-watchdog IRQ. None of the four
  carries an explicit permissive license, so they're staged gitignored under
  `tests/roms/external/fds-takuikaninja/` (fetched from the author's GitHub
  releases) rather than committed, mirroring the existing commercial-ROM
  convention; every test skips cleanly when the BIOS or a probe disk is
  absent, keeping CI clean by default. The underlying `$4023` and mirroring
  behaviors these probes exercise are already implemented and unit-tested
  independently in `crates/rustynes-mappers/src/fds.rs` — this is regression
  insurance against a second, hardware-verified oracle, not a fix for a gap.
  The `$4030.D1` DRAM-watchdog probe tracks a known, honest residual (not yet
  modeled by RustyNES or, per upstream, by most current FDS emulators) —
  see `docs/accuracy-ledger.md`.

### Changed

- **Dependency consolidation (PR #305 — closes Dependabot #298–#303).** Rolled
  all six open Dependabot bumps into one reviewed change, each verified against
  the code and gates; all landed with **no source changes** and the deterministic
  `#![no_std]` core untouched (AccuracyCoin stays **141/141**): **pollster
  0.4 → 1.0** (frontend / android / iOS `block_on` for wgpu/Metal init), **wide
  0.7 → 1.5** (the desktop `u32x8` SIMD blitter — the SIMD-vs-scalar byte-identity
  gate still passes), **tungstenite + tokio-tungstenite 0.29 → 0.30** (netplay +
  mobile signaling; `default-features = false` + `handshake` +
  `rustls-tls-webpki-roots` retained for clean pure-rustls cross-compiles),
  **bytemuck 1.25.0 → 1.25.1** and **cc 1.2.66 → 1.2.67** (patch), and CI
  **actions/setup-python v5 → v6** (MkDocs step). Verified with `cargo check` /
  `clippy -D warnings` / `cargo deny` / the `wide` byte-identity test / the full
  netplay tungstenite-0.30 suite.

## [2.2.0] - 2026-07-12 - "Capstone" (milestone cut — netplay matchmaking/lobby + FDS medium model + peripherals + quality/security pass)

### Added

- **Netplay lobby + matchmaking (v2.2.0 "Capstone", B5).** The pure signaling protocol (`crates/rustynes-netplay/src/signaling.rs`) grows a **browse-and-join** lobby directory and a matchmaking path atop the existing room-code / TURN stack. New `SignalMessage` variants — `ListRooms { rom_hash }` → `RoomList { rooms: Vec<RoomInfo> }` (the open, joinable, optionally game-filtered rooms; each `RoomInfo` carries the code / player count / capacity / `rom_hash` and *no* SDP/ICE/identity), and `QuickMatch { rom_hash, max_players }` → `Matched { room, slot, max_players }` (server-side "quick play": join any open room for the ROM via the shared `add_to_room` primitive, or create a fresh one with a deterministic `QM-NNNNNN` code). The `room-list` JSON array is parsed by a brace-depth walk bounded at `MAX_ROOM_LIST` (256) so an oversized frame cannot force an unbounded allocation. Determinism/rollback contract untouched — this is signaling only.
- **Delayed-stream spectators (v2.2.0 "Capstone", B5).** `SpectatorConfig.delay_frames` (clamped to `MAX_DELAY_FRAMES` = 512 ≈ 8.5 s) layers an intentional broadcast / anti-spoiler / jitter-smoothing hold atop the natural spectator lag: frame `f` is revealed only once frame `f + delay_frames` is confirmed (`reveal_horizon()`). Purely a *presentation* delay — frames are still produced byte-identically and in order, and the spectator still sends nothing — so it cannot perturb the match. Wired to a configurable `NetplayUi::spectator_delay_frames` (default 0).
- **Hardened desync surface (v2.2.0 "Capstone", B5).** `DesyncDiagnostics` gains a single graded `DesyncStatus` { `InSync` / `Suspect` / `Desynced` } verdict with a hysteresis threshold (`DEFAULT_DESYNC_THRESHOLD` = 3 consecutive mismatches ≈ 1.5 s at the 30-frame checksum interval) so a lone reordered / late peer checksum no longer flashes a false desync banner, plus a sticky peak-run rule so a confirmed (unrecoverable) desync never silently downgrades. Still pure telemetry over the `NetMessage::Checksum` digests the session already exchanges.
- **Peer-liveness RTT timeouts (v2.2.0 "Capstone", B5).** A graded `PeerLink` { `Live` / `Interrupted` / `TimedOut` } for an already-synced `NetplayConnection`, driven by `last_recv` against `peer_interrupt_timeout` (2 s) / `peer_disconnect_timeout` (5 s), plus a terminal `DisconnectReason::PeerTimeout`. Deliberately far above Mesen's trigger-happy ~150 ms (documented on `PeerLink`): a single lost 1 Hz `Quality` ping or a routine Wi-Fi/LTE retransmit spike never trips it, matching the multi-second grace windows GGPO/Parsec use. Both thresholds are builder-configurable (`with_peer_timeouts`).
- **Fuzz-target expansion (v2.2.0 "Capstone", quality).** `fuzz/` grows from 3 to 8 cargo-fuzz targets covering the remaining untrusted-input boundaries: `ppu_reg_io` (`Ppu::cpu_{read,write}_register` over a minimal `PpuBus`), `apu_reg_io` (`Apu::write_register` / `read_status`), `netplay_message` (the highest-value target — `NetMessage::from_bytes` binary UDP + `SignalMessage::parse` JSON signaling/lobby, both ingesting bytes straight off the wire), `save_state` (`parse_header` + `Nes::extract_thumbnail` + `restore_quiet`), and `movie` (`Movie::deserialize`). Each builds under nightly cargo-fuzz and runs clean for tens of thousands of iterations. `fuzz/README.md` documents the targets + the LeakSanitizer-under-sandbox note.
- **Read-only ROM Info browser (v2.2.0 "Capstone").** A new **Tools → ROM Info** panel (`crates/rustynes-frontend/src/debugger/rom_info_panel.rs`) surfaces, for the loaded ROM, the two dump-identity CRC32 keys (the header-excluded game-DB key + the full-file **No-Intro** key), the SHA-256, the effective per-game database entry (title / mapper / region / mirroring / submapper), and the decoded cartridge header read straight off the running `Nes` (mapper id, region, PRG-ROM / CHR-ROM sizes). Read-only (`&Nes`) — never mutates the emulator or the DB overlay; the deterministic core never consults it. Honest about surfacing only the vendored per-game DB + the header (no bootgod / nescartdb table is vendored).
- **MkDocs handbook deepening (v2.2.0 "Capstone", quality).** Four new Material-for-MkDocs handbook pages — `docs/expansion-audio.md`, `docs/pal-region.md`, `docs/crt-composite.md`, `docs/creator-tools.md` — curated entry points for the newer subsystems, cross-linked to the authoritative `apu-2a03.md` / `ppu-2c02.md` / `frontend.md` specs, with matching `mkdocs.yml` nav entries.
- **FDS medium model completion — CRC-16 / gap / continuous head-seek (v2.2.0
  "Capstone", F4.3).** The Famicom Disk System RAM adapter
  (`crates/rustynes-mappers/src/fds.rs`) completes the disk **medium** model. The
  disk is a synthesized byte-stream wire image — lead-in / inter-block gaps, a
  `$80` start mark, the block bytes, and a **CRC-16/KERMIT** per block — and each
  BIOS-written block now **re-emits a fresh per-block CRC-16** over its updated
  payload (`resynth_block_crc`), modelling the RP2C33 controller's continuous CRC
  generator so the medium stays self-consistent after a write. A new **continuous
  analog head-seek / velocity model** (opt-in, default-OFF —
  `Fds::set_analog_head_seek`) replaces the flat fixed `HEAD_RESEEK_CYCLES`
  motor-restart not-ready window with a belt-driven, distance-proportional seek
  time (`HEAD_SEEK_BYTES_PER_CYCLE` velocity + `HEAD_SEEK_SETTLE_CYCLES` settle,
  clamped to a cold spin-up), sized from the head-travel distance captured at
  motor-off. A **BIOS-free synthetic write-verify oracle**
  (`Fds::medium_write_verify`) walks the wire image and asserts every block's
  CRC-16 and gap/mark framing round-trips — the CI-verifiable half of the medium
  model; the real-BIOS write-CRC path needs a copyright `disksys.rom` and is
  exercised only from a gitignored local dump (`docs/accuracy-ledger.md` records
  the CI-verifiable-vs-local-only split). **Additive and deterministic**: with
  the head-seek model off (the default) a non-writing `.fds` run is
  **byte-identical** to prior releases; the new state round-trips an additive
  **v4** FDS save-state tail (v1/v2/v3 blobs load with the model disabled).
  AccuracyCoin has no FDS ROM, so **141/141 (100%)** is unaffected.
- **Famicom microphone + Zapper light-timing hardening (v2.2.0 "Capstone"
  peripherals).** The Famicom built-in controller-2 **microphone** is modelled on
  **`$4016` bit 2** (`Nes::set_microphone` / `Bus::set_microphone`), wired through
  the frontend input path (hold-to-talk `M` key → `FrameInputs.microphone` →
  latch), for games such as *The Legend of Zelda* (Pols Voice) and *Kid Icarus*.
  It is a `$4016`-only signal (never touches `$4017`). The **Zapper** photodiode
  now integrates a **3×3 aperture** (field-of-view) around the aim point,
  asserting light only when ≥2 pixels cross the luma threshold
  (`ZAPPER_APERTURE_*`) — hardening detection against sub-pixel aim error and PPU
  edge noise vs the prior single-pixel sample, while staying a deterministic pure
  function of the presented framebuffer (no save-state change). Both are additive
  and **default-off**: the mic released leaves the `$4016` read byte-identical,
  and the standard controller / Four Score path is unchanged. (The full Family
  BASIC `9×8` keyboard matrix was already modelled; its frontend mapping is
  unchanged.)

### Changed

- **Movie (`.rnm`) deserializer hardening (v2.2.0 "Capstone", quality).** The new `movie` fuzz target surfaced two OOM DoS paths in `Movie::deserialize` (`crates/rustynes-core/src/movie.rs`), both now fixed **byte-identically for valid input**: (1) the untrusted 4-byte `frame_count` was passed straight to `Vec::with_capacity`, so a 49-byte header could claim a multi-gigabyte reservation — now capped at `remaining_bytes / width` (== `frame_count` for any real file); (2) a `bytes_per_frame` of 0 made each `r.take(0)` consume no input, so the frame loop pushed `frame_count` empty records out of a finite file — now rejected up front (a real movie always writes the fixed `BYTES_PER_FRAME` ≥ 1). Regression test `deserialize_hostile_frame_count_does_not_oom` added; the existing 44 movie tests (incl. the determinism round-trip) stay green.

## [2.1.10] - 2026-07-12 - "Fathom" (creator tools and web parity — TAStudio greenzone + Lua API breadth + browser-RA auth-proxy deploy stack + Vs. DualSystem libretro presentation — "Loom")

### Added

- **Vs. `DualSystem` presentation in the libretro core (v2.1.10 "Web Parity").**
  The libretro core (`crates/rustynes-libretro`) now presents Vs. `DualSystem`
  arcade cabinets (Balloon Fight / Wrecking Crew / Tennis / Baseball), reaching
  parity with the desktop frontend. It detects them with the same `Emu::from_rom`
  (NES 2.0 header Vs. type OR the SHA-keyed `vs_db`), steps **both** cross-wired
  consoles each `retro_run`, and composes their two 256×240 framebuffers into a
  single **512×240** XRGB8888 side-by-side image (MAIN left, SUB right) — presented
  within a 512-wide `max_width` geometry so RetroArch draws the variable width with
  no geometry renegotiation. Libretro ports 0/1 → MAIN P1/P2, 2/3 → SUB P1/P2; only
  MAIN audio plays; save states use `VsDualSystem::snapshot`/`restore`; the RA/cheat
  memory maps expose the MAIN console. Previously a `DualSystem` dump booted a single
  console that hangs on its absent partner. The deterministic `no_std` core is
  untouched and byte-identical — this is a parallel present/serialize branch in the
  FFI wrapper. **Code-complete + builds** (`cargo build -p rustynes-libretro`);
  a live RetroArch run with a real cabinet dump is the maintainer's manual check.
  Docs: `docs/libretro/advanced_features.md`, `docs/frontend.md`.
- **Browser RetroAchievements auth-proxy deploy stack (v2.1.10 "Web Parity", ADR
  0015).** The browser-RA marshalling (`ra_glue.js` + `wasm_cheevos.rs`) has shipped
  since v1.7.0; this lands the remaining ADR 0015 carryover's **deployable** half —
  the casual-only auth proxy that injects RA's identity `User-Agent` server-side
  (browsers forbid scripts from setting it). `deploy/` gains a first-class
  `ra-proxy` compose service (`deploy/Dockerfile.raproxy`, running the stdlib-only
  reference stub) behind the shared Caddy TLS proxy at `https://<DOMAIN>/ra/*`,
  configured **purely from env** (`RA_USER_AGENT` / `RA_ALLOWED_ORIGINS` /
  `RA_UPSTREAM` / `RA_ENFORCE_CASUAL`) — the proxy holds no RA secret. The stub
  grew env-var configuration so one script serves both local dev and the container.
  Marshalling-contract tests added to `wasm_cheevos.rs` (ACHIEVEMENT_TRIGGERED
  filtering + malformed-payload tolerance + the not-configured caveat).
  **Code-complete + compose/config validated**; standing the stack on a live host,
  the RA-team `User-Agent` coordination, and a real browser RA login + casual unlock
  are the un-CI-able acceptance gate (runbook: `deploy/README.md`,
  `docs/cheevos-browser.md`).
- **Creator tools: TAStudio depth + Lua API breadth (v2.1.10 "Creator Tools &
  Web Parity", B8 + B9).**
  - **Force-greenzone (B8).** A new "Force GZ" toggle in the TAStudio piano-roll
    header guarantees a cached save-state at *every* frame in a bounded range
    (up to `MAX_FORCED_GREENZONE_FRAMES` = 10,800 ≈ 3 min at 60 fps), so
    scrubbing / rewinding anywhere inside it is instant — versus the normal
    density-tiered keyframe skeleton. Forced frames are pinned as non-evictable
    anchors and captured as the editor seeks / records across them; shrinking or
    clearing the range releases only the anchors force-greenzone itself added
    (marker / branch-point anchors are untouched). Documented memory budget; a
    pure caching optimisation, so a seek into the forced range stays
    bit-identical to a linear replay (the determinism / TAS contract is
    unchanged). *(Named markers and branch save-slots already shipped in v1.6.0;
    this deepens the greenzone half.)*
  - **Lua HUD: `emu.drawLine` (B9).** The fourth overlay primitive alongside
    `drawText` / `drawRect` / `drawPixel` — a straight segment for graphs, watch
    plots, and hitbox visualisers. Pure overlay (never write-gated); full mlua +
    piccolo parity.
  - **Lua memory: palette + CHR domains (B9).** `memory:read_palette(idx)`
    (`$3F00-$3F1F`, 6-bit index) and `memory:read_chr(addr)` (`$0000-$1FFF`,
    mapper-banked), both via the side-effect-free debug-peek path — the
    `*Debug` (no open-bus / no read-buffer-advance / no mapper side-effect)
    variant by construction on this observational engine.
  - **Lua lifecycle events (B9).** `reset`, `spriteZeroHit`, and `codeBreak`
    join the `emu.addEventCallback` surface (host-fired: `reset` on
    soft-reset / power-cycle, `spriteZeroHit` once per frame the PPU sprite-0
    hit flag was set — sampled non-destructively via `peek($2002)` — and
    `codeBreak` on a debugger breakpoint). Observational (no live `Nes`).
  - **piccolo (wasm) parity uplift (B9).** The experimental pure-Rust backend
    gains `emu.drawLine`, the read-parity `memory` table (CPU / PPU / palette /
    CHR / OAM reads served from an extended per-frame snapshot; `poke` keeps the
    gated + deferred contract), and an `addEventCallback` no-op so portable
    scripts don't error on wasm — closing most of the read + HUD gap (ADR 0012
    carve-out now only the per-access / per-interrupt replay callbacks and the
    host-fired events).
  - **Example script library.** Three well-commented additions —
    `hud_graph.lua` (drawLine value graph), `palette_viewer.lua`
    (`read_palette` / `read_chr` inspector), and `lifecycle_events.lua` (every
    `addEventCallback` event) — all compile-time embedded and exercised by the
    `bundled_example_scripts_load_and_run` test.
  - Determinism preserved throughout: reads are debug-peeks, writes stay
    gated / deferred exactly as before, and the deterministic `#![no_std]` core
    is untouched — **AccuracyCoin holds 141/141 (100%)** and save-state / TAS
    replay stays byte-identical.

### Deferred (documented)

- **Vs. `DualSystem` on the wasm desktop-style present (v2.1.10 "Web Parity").**
  The CPU compositor (`Gfx::compose_dual_into`) and the core (`Emu::Dual`) are
  already cross-platform, but the wasm ROM-load detection + un-gating the GPU
  present branch (`Gfx::render_dual`, currently `cfg(not(wasm))`) remain deferred.
  The libretro dual present (see Added) ships now; mobile stays deferred. See
  `docs/frontend.md`.

## [2.1.9] - 2026-07-12 - "Fathom" (presentation and signal — marquee CRT shader stack (CRT-Royale / guest-advanced / Sony Megatron) + raw NTSC composite signal-decode path + GIF/WAV capture + generated-palette editor — "Aperture")

### Added

- **Marquee CRT shader stack + raw NTSC composite signal (v2.1.9 "Presentation
  & Signal").** A presentation/display cut, all opt-in and **default
  byte-identical** (the shipped presentation is untouched, so `visual_regression`
  stays byte-identical and AccuracyCoin holds **141/141**).
  - **Raw composite core (P4).** A new `rustynes-ppu::raw_signal` module that
    keeps the 2C02 composite waveform *un-decoded*: for every `(index, emphasis)`
    pair it emits the twelve per-subcarrier-phase voltages the chip actually
    generates, so a decoder can reproduce signal-domain artifacts a per-colour
    palette cannot — composite colour bleed, dot crawl, and the waterfall/dither
    transparency tricks. Follows the canonical Bisqwit `nes_ntsc` / Mesen2 "raw
    palette" model; `generate_raw_signal_lut()` yields the full 512×12 table a
    host uploads as a signal texture. No transcendental in the path, so it is
    `f32` byte-identical across x86 / aarch64 / wasm / `thumbv7em` (a `no_std`
    `GOLDEN_SIGNAL` cross-target lock guards it). Additive + default-OFF.
  - **CRT shader stack (B6).** Three single-pass WGSL ports of the reference
    libretro *slang* CRT presets, added as **new WGSL files** in
    `rustynes-gfx-shaders` behind a `CrtStackShader` registry: **CRT-Royale**
    (luminance-scaled Gaussian beam, selectable aperture/slot/shadow mask,
    gamma-correct scanlines, curvature), **CRT Guest Advanced / guest-dr-venom**
    (power-shaped beam, halation glow, mask, curvature), and **Sony Megatron**
    (per-subpixel phosphor lighting with an HDR headroom + SDR Reinhard
    fallback). All four new shaders — the three CRT plus the P4 signal-decode
    pass — are gate-validated as real, compilable WGSL by the same **naga**
    front-end + validator wgpu runs at pipeline creation.
  - **Composable-stack UI wiring + per-game presets.** The CRT trio and the raw
    signal-decode pass are selectable from **Settings → Shaders** as first-class
    `ShaderStack` passes with their `#pragma parameter` sliders (mask type,
    scanline weight, curvature, beam, glow, HDR headroom), plus per-game shader
    presets (auto-applied on ROM load, off by default).
  - **Raw NTSC signal-decode pass (P4 shader).** `signal_decode.wgsl`
    reconstructs the 2C02's actual two-level chroma square wave from the
    palette-index framebuffer (matching `raw_signal.rs` byte-for-byte) and
    demodulates it with a windowed quadrature filter — decoding the true signal
    rather than re-encoding already-decoded RGB.
  - **Capture: GIF / animated capture + WAV audio export.** The `av-record`
    feature gains GIF export (palette-quantized, frame-decimated) and standalone
    WAV audio export alongside the existing PNG-sequence / raw A/V capture —
    driven from a read-only framebuffer/audio tap so capture never perturbs the
    deterministic emulation timeline.
  - **Palette editor — live generated-palette preview.** The existing
    generated-NTSC palette editor (Settings → Video → Generated NTSC palette:
    saturation / hue / contrast / brightness / gamma sliders feeding
    `palette_gen.rs`, plus the 64-swatch editor) gains a live 16×4 swatch preview
    of the *generated* base that regenerates from the current params as you drag
    the sliders — so the look can be dialled in before enabling it. Purely
    visual; the shipped palette is unchanged until explicitly enabled.

## [2.1.8] - 2026-07-12 - "Fathom" (performance — default-off specialized fast PPU dot-loop (differential-tested byte-identical, +12% rendering-heavy) + SIMD software blitter + wasm size/startup pass; "Tempo")

### Performance

- **Specialized visible-scanline fast dot path (v2.1.8 "Performance" A1;
  default-OFF, opt-in).** Profiling a representative mixed workload
  (`perf`, the PGO training corpus) shows `Ppu::tick` is the emulator's single
  hottest function — **~46% of frame self-time** — and the overwhelming majority
  of its 89,342 per-frame invocations are visible-scanline background-render dots
  whose surrounding event/bookkeeping branches are all statically dead. A new
  **runtime knob** (`Nes::set_fast_dotloop`, default **false**) dispatches those
  "clean" dots — a visible scanline, dots `1..=256`, rendering stably enabled,
  and no sub-dot disturbance in flight (no `$2006` copy-V or PPUMASK write-delay
  pending, no PPUDATA state machine running, no armed/pending OAM-corruption,
  warm scanline-classification cache) — to `Ppu::tick_visible_render_fast`, a
  straight-line handler that runs the **identical** helper sequence with the
  dead branches pruned. Any disturbance drops instantly back to the exact
  per-dot path.
  - **Byte-identical (proven, not assumed).** The default (`false`) is
    byte-identical to a build without the field. With the knob ON, a new
    differential test (`fast_dotloop_diff`) runs a corpus (`nestest`,
    `flowing_palette`, `oam_stress`, `AccuracyCoin`, the Holy Mapperel MMC1/MMC3
    boards, and a mid-frame raster demo) through BOTH paths and asserts
    bit-for-bit identical framebuffer + palette-index framebuffer + audio + CPU
    cycles + full core snapshot, every frame — including under the opt-in
    `Rp2c02G` die revision (v2.1.7 #280), whose `$2003`-write-during-render
    OAM-corruption is one of the disturbances that forces the exact path.
    AccuracyCoin holds **141/141**, nestest 0-diff, the `visual_regression`
    golden set, and the APU oracle all stay byte-identical.
  - **Measured (interleaved per-frame A/B, drift-robust):** rendering-enabled
    content (`nestest`, a rendered menu) is **~+12.3% faster per frame**
    (4.54 → 3.98 ms, stable across rounds), well above the project's
    >3%-Criterion adoption bar; rendering-**disabled** content (`flowing_palette`,
    which shows all 64 colours via the rendering-off backdrop-override trick, so
    the fast path never applies) is **neutral** (~+0.3%, the reordered
    short-circuit guard costs ~nothing). See `docs/performance.md`.
  - **Architectural note:** a *whole-scanline batch* (the Mesen2/tetanes-style
    straight-line renderer) is **precluded** by the v2.0.0 "Timebase" lockstep
    every-cycle-bus-access scheduler — `run_ppu_to` advances the PPU ≤3 dots per
    CPU cycle and the CPU observes A12/NMI/sprite-0/`$2002` at 3-dot granularity,
    so the PPU is never invited to run a scanline uninterrupted. This is a
    per-dot specialization, not a dot-batch. **Shipped default-OFF** (the
    shipped build is unchanged/byte-identical); recommended for promotion to
    default after maintainer review + a clean-host Criterion confirmation.
- **Vectorized software palette-index -> RGBA blitter + wasm size/startup pass
  (v2.1.8 "Performance", A2 + A4).** A new frontend-only `gfx_blit` module
  (`crates/rustynes-frontend/src/gfx_blit.rs`) that converts the PPU's
  palette-index framebuffer (`&[u16]`, `(emphasis << 6) | colour`) to RGBA8
  through the exact 512-entry LUT the core emits with, so its output is
  **byte-identical** to `Ppu::framebuffer` by construction. Three interchangeable
  paths — a scalar reference, a tight scalar-`u32`, and portable SIMD (`wide::u32x8`
  on desktop / `core::arch::wasm32` `v128` under `+simd128` on wasm, with a scalar
  fallback for non-SIMD wasm) — all validated byte-for-byte equal by the
  `simd_equals_scalar_byte_identical` unit test over a full-frame corpus that sweeps the whole
  `0..512` domain, and profiled by a Criterion bench (`benches/gfx_blit.rs`). The
  conversion is a memory-bound LUT gather, so per the measured bench the SIMD path
  is within noise of scalar (documented honestly in `docs/performance.md`); the
  module is a reusable, oracle-checked utility (the shipped on-screen frame path
  stays GPU-resident and does not route through it). Determinism-neutral: the core
  and its golden vectors are untouched — AccuracyCoin **141/141**, `visual_regression`
  byte-identical. Web build (A4): the release wasm artifact now runs `wasm-opt -O4`
  (SIMD + bulk-memory features preserved) via `data-wasm-opt` in `web/index.html`,
  with streaming instantiation documented; the real `trunk build --release`
  bundle measures **3.99 MiB gzip** — 1.01 MiB of headroom under the 5 MiB budget
  (`scripts/wasm_size_budget.sh`). `wide` is a native-only dependency, so it
  never enters the wasm bundle and the `#![no_std]` chip stack stays
  dependency-light.

## [2.1.7] - 2026-07-12 - "Fathom" (hardware revisions & DMA frontier — opt-in PPU/2A03 die-revision + power-on RAM/palette model + honest DMA "unexpected read" residual ADR 0033; "Stepping")

### Added

- **PPU die-revision + power-on hardware model (v2.1.7 "Hardware Revisions &
  DMA Frontier", P5 — PPU side).** A selectable 2C02 die revision and power-on
  state model, every knob **opt-in and default-off** so the deterministic core
  stays **byte-identical** at the default (AccuracyCoin **141/141 (100%)**,
  nestest 0-diff, `visual_regression` / `pal_apu_tests` 10/10 and save-state
  round-trip all unchanged). Four additive pieces:
  - **`PpuRevision` enum** (`rustynes_core::PpuRevision`; default `Rp2c02H`,
    opt-in `Rp2c02G`) gating the one revision-dependent quirk RustyNES models.
    Config re-applied on load like `region` — not serialized.
  - **OAMADDR (`$2003`) write-during-render OAM corruption**, modeled only on
    the opt-in `Rp2c02G` die: a `$2003` write while rendering is active copies
    OAM row 0 over the row `(value>>3)&0x1F` (reusing the existing `CorruptOAM`
    row-copy, committed on the next rendered dot). The *Huge Insect* glitch. The
    default revision never arms it. The corruption state it can arm
    (`oam_corruption_pending`/`_index`) already round-trips via the v6 PPU
    snapshot tail, so **no snapshot-format change** is needed. Documented as an
    honest opt-in approximation (the exact per-revision/per-title byte output is
    not independently oracle-verified) in `docs/accuracy-ledger.md`.
  - **Power-up palette-RAM model** (`rustynes_core::PaletteInit`; default
    `Zeroed`, opt-in `Blargg`) loading the canonical blargg power-up dump (6-bit
    masked) for software that samples uninitialized palette RAM. Writes only
    `palette_ram` (already serialized), so no snapshot change. Default keeps the
    established all-zero power-up palette.
  - **Power-on work-RAM model** (`rustynes_core::PowerOnConfig` / `PowerOnRam`:
    `Zeroed` default / `Seeded(u64)` / `Filled(u8)`) via the new
    `Nes::from_rom_with_power_on_config`, for titles that read uninitialized RAM
    (*Final Fantasy* RNG seed, *River City Ransom*, *Cybernoid*). Every fill is
    **deterministic** (no wall-clock / OS RNG), stored on the bus so
    `power_cycle == fresh boot`. `from_rom_with_power_on_seed` now routes through
    `PowerOnRam::Seeded`; the default all-zero path is unchanged.
  - Exposed through additive `Nes` setters (`set_ppu_revision`,
    `set_power_up_palette`, `set_power_on_ram`) mirroring the v2.1.4 OAM-decay
    knob shape, and default-off `[emulation]` config keys
    (`ppu_oamaddr_corruption`, `blargg_power_up_palette`,
    `randomize_power_on_ram`, `power_on_ram_seed`) pushed into the core on ROM
    load / power-cycle / startup. `docs/ppu-2c02.md` documents each; the
    `#![no_std]` chip stack stays clean.
- **2A03 die-revision config + the DMA "unexpected read" frontier (v2.1.7
  "Hardware Revisions & DMA Frontier").** New additive
  `Cpu2A03Revision { Rp2A03G (default), Rp2A03H }` config
  (`Nes::set_cpu_2a03_revision`) gating the DMA unit's "unexpected DMA" extra
  parked-address re-read on a DMC-halt-overlaps-OAM-halt cycle. **Modeled +
  verified:** the existing DMC↔OAM collision (get/put), OAM alignment, aborted
  DMC-DMA, and the `$2007`/`$4015`/`$4016`/`$4017` DMC-glitch register-readout
  corruption all stay green on the default — the five `dmc_dma_during_read4`
  ROMs, both `sprdma_and_dmc_dma` variants, and `dma_timing_pin` all `Pass`.
  **Documented residual (honesty gate, ADR 0033):** the RP2A03G-vs-RP2A03H die
  revision is modeled by **no** public reference emulator (Mesen2 / ares /
  BizHawk / TriCNES / fceux / nestopia / GeraNES / higan) and verified by **no**
  test ROM; on this engine the revision gate fires but is a **documented no-op
  on every committed oracle** (the parked address during a DMC+OAM overlap is
  always the post-`$4014` instruction fetch, never a side-effect register), so
  `Rp2A03H` is byte-identical to `Rp2A03G` today — the difference is a
  mechanism-level model, not an observable divergence, and its direction is an
  unverified hypothesis recorded not faked. The revision is a config re-applied
  on load, **not** part of the save-state; the default (`Rp2A03G`) stays
  byte-identical (AccuracyCoin **141/141**, nestest 0-diff, save-state
  round-trip byte-identical). No `dmc_dma_during_read4` sub-test is made to fail
  or newly `#[ignore]`'d. See ADR 0033 + `docs/scheduler.md` §"Unexpected DMA".

## [2.1.6] - 2026-07-11 - "Fathom" (expansion audio — decibel oracle + hardware/Mesen2 channel-level calibration + Namco 163 12 dB fix + mix UI/scopes; "Timbre")

An expansion-audio fidelity cut. **Base 2A03 NTSC output stays byte-identical** —
expansion audio is a separate additive `mix_audio` term — so AccuracyCoin holds
**141/141**, `blargg_apu_2005`, nestest, and `visual_regression` are unchanged; only the
three `db_vrc6`/`db_mmc5` expansion snapshots were re-blessed (audio hash only).

### Added

- **Expansion-audio decibel oracle** (`audio_expansion.rs` `level_db_*`) — each
  bbbradsmith `db_*` ROM now asserts the measured expansion/reference peak ratio against
  the Mesen2 / hardware target (triangle ÷ square ≈0.524, VRC6 ≈1.506, MMC5 ≈1.000, N163
  1-ch ≈6.02), upgrading the prior byte-exact `insta` snapshots into a real level oracle.
- **Audio Mixer panel** (Tools → Audio Mixer) — per-source gain sliders + mutes for the
  five 2A03 channels and the detected on-cart expansion chip (VRC6/VRC7/MMC5/N163/5B/FDS),
  presets (Authentic / Balanced / Expansion boost), and per-channel oscilloscope + VU
  meters. A frontend re-weight of the determinism-safe `channel_gain`/`channel_mask`
  overlay (byte-identical at unity, never serialized).
- **VRC7 patch-set verification** — all 15 melodic (+3 rhythm) patches pinned
  byte-identical to the canonical Nuke.YKT dump; plus Sunsoft 5B log-DAC and Namco 163
  long-period wavetable unit tests.

### Changed

- **Expansion-audio channel levels calibrated to the hardware / Mesen2 `db_*` levels:**
  VRC6 `256 → 979` (≈0.39× → ≈1.51× the 2A03 pulse), MMC5 `256/16 → 650/40` (≈1.0×), and
  **Namco 163 `64 → 261`** (≈1.48× → ≈6.02× 1-channel — it was ~12 dB too quiet; no
  reference emulator attenuates N163). The N163 fix is shared with the NSF path. *(VRC6
  was later re-corrected to ~1.0× in v2.2.7.)*

### Deferred (documented)

- Sunsoft 5B absolute level and VRC7 FM level remain honest documented gaps
  (`docs/accuracy-ledger.md`) — the 5B needed a wider-than-`i16` mix path (closed in
  v2.2.3); the VRC7 FM level has no clean square-vs-square oracle.

## [2.1.5] - 2026-07-11 - "Fathom" (regression net & residual — Holy Mapperel mapper regression net + PAL APU frame-counter 10/10 + real TURN NAT-retransmit production fix + fat-LTO A/B validation + MMC3 F5.0 A12-phase study; "Vernier")

A regression-net and residual cut. Additive/observational and NTSC-byte-identical —
AccuracyCoin holds **141/141**, nestest 0-diff, the commercial byte-identity oracle
unchanged.

### Added

- **Mapper bank-reachability + IRQ regression net** — the tepples **Holy Mapperel**
  cartridge-assembly ROMs wired into CI (`holy_mapperel.rs`, 17 zlib ROMs,
  framebuffer-hash pinned with *settled* + *non-blank* guards): each detects its mapper
  from bank/mirroring response, proves every PRG/CHR bank reachable, and exercises WRAM +
  MMC3/FME-7 IRQ. 15/17 report `detail=0000`; the two MMC1 + two FME-7 ROMs surface a
  documented WRAM-protection residual (closed later in v2.2.3), recorded in
  `docs/accuracy-ledger.md`.
- **First PAL-region APU oracle** — blargg's `pal_apu_tests` (10 sub-ROMs) wired into CI
  via a new on-screen-verdict runner, which also **corrects a false oracle** (the prior
  `$6000`-status check passed vacuously on these PRG-RAM-less NROMs). Modeled the 2A07 PAL
  frame-counter step positions (region-gated, NTSC tables untouched) and fixed the length
  halt/reload write-ordering — **10/10 pass** (honestly 3/10 pre-model). NTSC byte-identity
  preserved: `blargg_apu_2005` 11/11, AccuracyCoin 141/141.
- **MMC3 R1/R2 residual A12-phase study** (ADR 0002 F5.0) — a default-off observational
  probe (`mmc3-a12-phase-probe`) that refines the F5.0 finding with fresh instrumentation:
  the two `scanline_timing` residuals have zero post-access IRQ-clocking rises, but the two
  "reload/set-IRQ-every-clock" residuals have 4 each, so "no post-access rise" is
  ROM-specific, not structural. No default/scheduler change; all four residuals stay
  `#[ignore]`'d; the ares-style M2-edge low-time filter remains the one untested axis-B
  lever.

### Changed

- **fat-LTO release profile measured, documented, and validated** — the existing
  `lto = "fat"` + `codegen-units = 1` default is now backed by an in-repo same-host A/B
  (**+8.4%** to **+20.8%** on cross-crate paths, within noise on the single-crate control),
  verified byte-identical (AccuracyCoin 141/141, nestest 0-diff). No default change;
  corrects `docs/performance.md`'s stale "thin" text and the stale `139/139` PGO comments.

### Fixed

- **Netplay: the native TURN client now retransmits (RFC 5389 §7.2.1)** — a real production
  bug where symmetric-NAT relay fallback aborted on a single dropped UDP datagram
  (`Allocate`/`CreatePermission` were sent once). It now retransmits every 250 ms until
  timeout, recovering transparently (STUN/TURN requests are idempotent). This also fixed
  the intermittent `nat_connect_loopback_relay` flake on `windows-latest` that had been
  blocking `release-auto`. The determinism contract (session-digest agreement) is unchanged.

## [2.1.4] - 2026-07-11 - "Fathom" (accuracy hardening — opt-in OAM decay + BestEffort boot-smoke sweep + MMC3-clone A12/IRQ timing oracle; "Caliper")

### Added

- **Optional OAM decay (accuracy, default-OFF).** The 2C02's Object Attribute
  Memory is dynamic RAM: sprite evaluation implicitly refreshes it every rendered
  scanline, but with rendering disabled long enough the un-refreshed rows lose
  charge and decay to a fixed garbage pattern. RustyNES now models this exactly
  like Mesen2 (`ReadSpriteRam`/`WriteSpriteRam`, 3000-CPU-cycle refresh window per
  8-byte row): every OAM read (`$2004` **and** the sprite-evaluation reads) and
  write refreshes the row's timestamp, and a row un-touched past the window decays
  on the next read to `((sprAddr & 3) == 2) ? (sprAddr & 0xE3) : sprAddr`. It is
  **off by default** — with the default the framebuffer/audio/replay output and
  the AccuracyCoin / commercial / visual regression suites are **byte-identical**
  to a decay-free build. NTSC/Dendy only (PAL's refresh cadence masks decay).
  Deterministic when on (driven off the PPU's monotonic dot counter, never
  wall-clock/OS-RNG). Enable via **Settings → Emulation → "OAM decay (accuracy)"**,
  the `[emulation] oam_decay` config bool, or `Nes::set_oam_decay(true)`. The
  per-row decay state round-trips the save-state via an additive
  `PPU_SNAPSHOT_VERSION` v7 tail (stored as a relative age so a run-ahead / netplay
  `snapshot`→`restore` stays byte-identical); pre-v7 `.rns` blobs still load.

- **CI boot-smoke sweep of every `BestEffort` mapper family (Fathom F3.1).** A
  new test-harness suite
  (`crates/rustynes-test-harness/tests/v21_best_effort_sweep.rs`, `--features
  test-roms`) exercises the full parse → construct → dispatch → run-loop
  integration for **all 26** `BestEffort` (Tier-2) mapper families — the
  reference-ported long-tail boards that lack a cleanly-booting redistributable
  ROM dump and so can never be honestly oracle-gated. The target set is derived
  live from the `rustynes-mappers::mapper_tier` classifier (the single source of
  truth), so any future family promoted into or out of `BestEffort` is swept —
  or dropped — automatically with no edit to the test. Each family is built into
  a synthetic minimal iNES / NES 2.0 image (256 KiB PRG spin loop + CHR-RAM;
  NES 2.0 headers with the byte-8 mapper-MSB for the 17 high-id boards `> 255`)
  and run for ~60 headless, deterministic frames, asserting no panic, an exact
  mapper-id header round-trip, and a well-formed 256×240 RGBA framebuffer. Any
  panic in a `BestEffort` register decode, bank wiring, or per-tick hook is now
  caught in CI instead of only when a user loads a real cart. This is a **pure
  safety net**: it promotes nothing, adds no accuracy/oracle claim (accuracy
  stays defined by the Core/Curated gate), and leaves runtime behaviour and the
  deterministic `#![no_std]` core byte-identical. The two NTDEC boards 81 / 174
  correctly reject a CHR-RAM header with a typed `RomError` (not a panic) and are
  handed CHR-ROM geometry; no real panics were found in the sweep. See
  `docs/mappers.md` ("Mapper accuracy tiering") and `docs/adr/0011-mapper-tiering.md`.
- **Shared MMC3-clone A12/IRQ timing oracle (Fathom F3.3).** A new chip-level
  test suite (`crates/rustynes-test-harness/tests/mmc3_clone_a12.rs`,
  deterministic, headless, no ROM files — runs in the default `cargo test`)
  proves the reusable `Mmc3Clone` core reproduces MMC3's A12-clocked
  scanline-counter IRQ timing for all **eleven** `Mmc3CloneMapper` boards
  (mappers 44, 49, 52, 115, 134, 189, 205, 238, 245, 348, 366). Because every
  board routes its `$8000`-`$FFFF` register space — including the IRQ ports
  `$C000`/`$C001`/`$E000`/`$E001` — into the same shared counter, the scanline
  IRQ is board-independent by construction; the oracle exercises each board's
  own register decode to confirm the ports reach that counter. The centerpiece
  drives every clone board and a reference plain `Mmc3` (Sharp / rev A) through
  the identical canonical rendering-scanline A12 edge sequence and asserts the
  clone reproduces the reference's per-scanline IRQ-assert bitmap
  **bit-for-bit**: the IRQ first asserts on rising edge `latch + 1` (the initial
  `$C001` reload consumes edge 0, then `latch` decrements reach zero) and
  re-asserts every `latch + 1` scanlines once acknowledged. The suite also pins
  the `$E001`/`$E000` enable/acknowledge gate, the `$C001` reload periodicity,
  and the A12 rising-**edge filter** (holding A12 high across consecutive reads
  clocks the counter exactly once — no double-clock). The reference `Mmc3` *is*
  the oracle, so any clone whose shared core drifted from MMC3's scanline timing
  would fail. This is **additive test evidence** deepening the cluster's
  existing `Curated` classification — it promotes nothing, moves no tier, and
  leaves the deterministic `#![no_std]` core byte-identical (no mapper source
  changed: the clone core already matches MMC3 timing). See `docs/mappers.md`
  ("MMC3-clone A12/IRQ timing oracle").

## [2.1.3] - 2026-07-11 - "Fathom" (quality-of-life — APU filter-model audio fix + Game Genie code nomination/database + universal header-robust matching + MkDocs docs handbook; "Codex")

### Added

- **Game Genie matching is now header-insensitive for all ~520 games.** The bulk
  catalog is keyed by the full-file No-Intro CRC, which only matches a dump whose
  16-byte iNES header is byte-identical to No-Intro's — so a **re-headered** dump
  (common) missed. A new third catalog (`genie_database_headerless.tsv`, ~16.5k
  rows / 521 games) carries the same libretro codes **re-keyed to the
  header-excluded `rom_crc32`** (via the NES 2.0 database's content CRCs, joined
  by game name with a manual alias table for the long-tail titles), so a game now
  resolves from PRG + CHR content regardless of its header. Previously only 6
  curated classics had a header-excluded key. The re-key is regenerated by
  `scripts/gg/gen_headerless_genie_db.py` (the NES 2.0 DB is a build-time input,
  never committed). All three catalogs ship on every target including wasm
  (together ~370 KiB gzip, inside the 5 MiB budget). Frontend-only; the
  deterministic core is untouched.
- **APU audio filter-model selector** (fixes the "thin / missing bass channel"
  sound). RustyNES applies the authentic **NES front-loader** analog filter — a
  90 Hz + an aggressive **440 Hz high-pass** + a 14 kHz low-pass — which is
  byte-correct (identical to ares/tetanes; verified by the APU golden vectors)
  but rolls off the bass/triangle register hard, reading as a missing channel.
  Mesen2 / FCEUX / Nestopia omit that high-pass, which is why they sound fuller.
  You can now pick the model in **Settings → Audio → Filter model**
  (`[audio] filter_model`): **`nes`** (default, authentic — byte-identical to
  earlier builds), **`famicom`** (a single ~37 Hz high-pass — the nesdev Famicom
  spec, fuller low end), or **`clean`** (a ~10 Hz DC-block only — fullest, the
  Mesen2-like character). Core: `Apu::set_filter_model` / `Nes::set_apu_filter_model`.
  Tonal only — channel content, determinism, save-states, and the audio oracle are
  unchanged on the default. The DRC resampler + band-limited BLEP synthesis were
  audited and found correct (they match Mesen2's approach); no change needed there.
- **Game Genie per-game code nomination + a bulk code database**. The Cheats
  panel now suggests the known Game Genie codes for the loaded game — a
  category-grouped "Known codes" pick-list, each row feeding the same validated
  `GenieCode::new` + persistence path as a hand-typed code — instead of only
  decoding codes you enter (previously it showed "No Game Genie cheats. Enter a
  6- or 8-character code above." for essentially every commercial ROM). A new
  bulk catalog (`genie_database_full.tsv`, **~10,800 codes across ~520 USA/World
  games**) is ingested from the openly-licensed libretro-database Game Genie
  files and keyed to every known dump's CRC32 via the No-Intro NES DAT. To match
  whatever dump "flavor" a user has, a ROM is now recognized on **two** CRC32
  keys: the header-excluded `rom_crc32` (the curated starter catalog) and the
  full-file No-Intro `rom_crc32_full` (the bulk catalog), unioned + de-duplicated.
  Frontend-only (the deterministic core is untouched; codes re-validate at load).
  The bulk catalog ships on every target including the wasm browser demo — at
  ~777 KB raw it gzips to ~128 KiB, well inside the wasm bundle's 5 MiB budget —
  so the browser build carries the full game coverage too.
- **Material for MkDocs documentation site** at `/docs/` on GitHub Pages
  (<https://doublegate.github.io/RustyNES/docs/>). The existing Pages deployment
  now serves three sections from one artifact: the playable wasm demo at the
  site root (`/`), the workspace rustdoc at `/api/`, and this new
  Material-themed handbook at `/docs/`. The handbook renders the existing `docs/`
  subsystem specs and user guide directly (no duplicated content — `docs_dir`
  points at the source-of-truth tree) with a curated, grouped navigation
  (Overview, Emulation Core, Frontend & Features, Testing & Accuracy, Platforms,
  User Guide), a light/dark palette toggle, instant navigation, search, and
  copy-to-clipboard code blocks. Per-page **social preview cards** (the `social`
  plugin) render an Open Graph / Twitter image for each page so shared `/docs/`
  links unfurl richly, and the `privacy` plugin self-hosts the theme's web-fonts
  into the build for a network-free, GDPR-clean served site.
  `.github/workflows/web.yml` gains a Python + `mkdocs-material[imaging]` build
  step (with the Cairo/Pango system libraries the card renderer needs) that emits
  the handbook into `_site/docs/` alongside the demo and rustdoc copies, and now
  also triggers on `docs/**` / `mkdocs.yml` changes.

## [2.1.2] - 2026-07-11 - "Fathom" (display-fidelity — generated NTSC palette + composite-shader ladder + Vs. `DualSystem` second screen + NSF non-60 Hz/NSFe; "Prism")

### Added

- **Vs. `DualSystem` second-screen presentation** (Fathom F2.1, desktop). A loaded
  Vs. `DualSystem` cabinet (Balloon Fight, Wrecking Crew, Tennis, Baseball) now
  runs **both** cross-wired consoles and presents them together — side-by-side
  (512x240, default) or stacked (256x480), selectable via `[graphics]
  dual_screen_layout`. P1/P2 drive the main console, P3/P4 the sub; coin-insert
  (F10) and the main console's audio are wired. The core dual engine already
  existed (`VsDualSystem` / `Emu::Dual`); this adds the frontend path — an
  additive `EmuCore::dual` field, a `produce_dual_frame` step, a composed
  two-screen blit (`Gfx::render_dual`), and Vs.-DB DIP/RGB-palette applied to both
  consoles — so the single-console path stays byte-identical. The advanced
  single-`Nes` features (run-ahead, rewind, netplay, TAS, dual save-state) are
  **scoped out in dual mode** (ADR 0032); the debugger/HD are unavailable there.
  Real-cabinet boot remains fixture-limited (the circulating dumps are the MAME
  maincpu half only). Desktop only for now; wasm/mobile deferred.
- **NTSC composite-shader ladder completed** (Fathom F2.2). The three-rung
  display-only ladder — simplified blur (`Ntsc`) → LMP88959 composite
  (`Lmp88959`) → Bisqwit per-dot (`CompositeRt`) — is verified end-to-end, and
  **live emulator-synced dot-crawl is now wired to LMP88959** as well as Bisqwit:
  the NES 3-frame colour phase (`ntsc_phase()`) advances the LMP base subcarrier
  phase (`video_phase / 3` turn) on top of the user's static offset. The live
  phase is decoupled from the (heavier) palette-index snapshot, so an LMP-only
  stack gets crawl without the index upload. All passes stay display-only —
  `visual_regression` is byte-identical with any filter active. Documented the
  legacy-vs-stack precedence and the palette↔pass split (the generated/custom
  palette feeds the RGBA passes but not the index-based Bisqwit pass); no
  separable-kernel rung is added (LMP covers that tier). See `docs/frontend.md`.
- **Generated NTSC palette** (Fathom F1.4). A new in-core synthesizer
  (`rustynes_ppu::generate_base_palette`) produces the 64-entry base palette from
  a model of the 2C02's composite-video output (the Bisqwit / ares YIQ
  integration: two-level chroma square wave over 12 subcarrier phases →
  demodulate → FCC YIQ→RGB with gamma), tunable via saturation / hue / contrast /
  brightness / gamma. Every transcendental routes through `libm`, so the output is
  **byte-identical across all targets** (x86 / aarch64 / wasm / `thumbv7em`) and
  locked by a committed golden. It feeds the existing `set_custom_palette` /
  emphasis-LUT path (no new emphasis model) and is **off by default** — the
  shipped build keeps the hand-authored palette and is byte-identical; enable and
  tune it under Settings → Palette → "Generated NTSC". Presentation-only; the
  deterministic core and AccuracyCoin (141/141) are unaffected.
- **NSF non-60 Hz playback + NSFe support** (Fathom F4.1/F4.2). The NSF player now
  parses the header **play-speed divider** (`$6E-$6F` NTSC / `$78-$79` PAL, µs per
  `play`) and drives non-standard rates correctly: a PAL 50 Hz tune — or any custom
  divider — on the NTSC console runs `play` from a mapper **cycle-timer IRQ** (the
  driver disables the APU frame-counter IRQ once in `init`, then arms a
  level-triggered, `$5FF1`-acked timer that fires every `period` CPU cycles). The
  standard 60 Hz path is unchanged and **byte-identical** (vblank-NMI). The extended
  chunked **`NSFE`** container is now parsed as well (INFO / DATA / BANK / auth
  chunks; rate derived from the region flag), routed through the same
  `Nes::from_nsf` path and frontend file detection. Covered by new `nsf` unit tests
  plus a core integration test asserting the timer IRQ drives `play` at a sub-60 Hz
  rate. Determinism / AccuracyCoin unaffected (NSF is not on the oracle path).

## [2.1.1] - 2026-07-10 - "Fathom" (patch — Wizards & Warriors freeze fixed at the root: game-DB mirroring override + a run-ahead PPU-snapshot gap)

### Fixed

- **Wizards & Warriors (and ~1900 other games) no longer freeze at level load —
  the actual root cause.** The per-game database (`game_database.txt`, vendored
  from TetaNES) force-applied its `mirroring` column to *every* matched ROM,
  including mappers that control their own nametable mirroring at runtime.
  Wizards & Warriors is AxROM (mapper 7), which flips single-screen A↔B mid-frame
  to draw its status bar; the DB's spurious `Horizontal` pinned the mirroring,
  blanked the bottom half of the screen, killed the sprite-0 split, and hung the
  game (on desktop **and** WASM; a headless core, which never consults the DB, was
  always unaffected). The game-database mirroring override is now honored **only**
  for hardwired-mirroring boards (NROM/UxROM/CNROM/GxROM) via the new
  `Mapper::has_hardwired_mirroring()` capability (default `false` — the safe
  direction, so a mapper that controls its own mirroring can never be corrupted),
  gated in `App::apply_game_db` and the per-game overlay through
  `Nes::mapper_has_hardwired_mirroring()`. This protects **1914** mapper-controlled
  database rows from the same class of corruption. Regression-tested
  (`hardwired_mirroring_gate_matches_board_type`) and verified **byte-identical**
  to a clean headless replay through the real game-DB path. See ADR 0031.
- **Run-ahead PPU save-state gap hardened** (`PPU_SNAPSHOT_VERSION` 5 → 6). Run-ahead's
  per-frame `snapshot`/`restore` round-trip did not serialize some PPU render
  state — the per-sprite shifter-halt state (`spr_halted`), the 1-dot-delayed
  rendering gate (`prev_rendering_enabled` / `rendering_enabled_delayed`), and the
  OAM-row-corruption arming state — so a snapshot/restore could drift them. This is
  a genuine save-state-completeness fix that also hardens netplay rollback and
  manual save/load. **Note:** this was originally believed to be the Wizards &
  Warriors freeze cause; deeper full-core-state diffing later proved run-ahead was
  byte-identical and the freeze was the game-DB mirroring override above — this
  change remains a valid correctness improvement on its own. The additive v6 tail
  keeps pre-v6 `.rns` states loadable (upconverting to power-on defaults) — not an
  ADR-0028 epoch break.
- Regression tests: `hardwired_mirroring_gate_matches_board_type` (mirroring gate)
  and the GitHub-safe `ww_runahead_matches_plain_across_a_mid_frame_split` (skips
  cleanly when the commercial dump is absent). The core / accuracy path is
  unchanged — AccuracyCoin stays **141/141**, no oracle moves, determinism holds.
- Version: workspace `2.1.0 → 2.1.1`.

## [2.1.0] - 2026-07-09 - "Fathom" (accuracy remediation — PPU display quirks, mapper completion, MMC3 residual closed)

- The **accuracy-remediation** release — a core/desktop cut that lands **ahead of**
  the joint mobile store launch (which moved from v2.1.0 to **v2.2.0**, so the
  Android + iOS apps ship on this improved core). AccuracyCoin stays **141/141**,
  nestest 0-diff, the `#![no_std]` chip stack untouched; the deterministic core is
  unchanged except the display-only PPU fix below. No save-state/format bump.
- **PPU palette backdrop-override (F1.1).** When rendering is disabled and the VRAM
  address `v` points into palette space (`$3F00-$3FFF`), the PPU now outputs the
  color at `v & 0x1F` instead of the universal backdrop — the documented 2C02
  display behavior, **byte-exact with TriCNES** (`Emulator.cs`). This makes the
  `full_palette` / `flowing_palette` demos render correctly (all 64 colors) and is
  a display-only change (palette RAM is never mutated). Nine snapshots re-blessed —
  the 2 palette demos + 7 commercial games (Micro Machines-style palette tricks) —
  all converging RustyNES **with** its TriCNES oracle; `external_real_games` 60/60
  stays byte-identical.
- **PPU OAM + open-bus audits (F1.2 / F1.3).** The OAMADDR-forced-to-0 (dots
  257-320), `$2004` `$E3` attribute mask, and open-bus refresh map were audited
  against the Blargg `ppu_open_bus` table + AccuracyCoin and found already correct;
  each is now locked by a fast unit regression test. The `OAMADDR & 0xF8`
  render-start copy stays unmodeled by design — Mesen2, ares, and TriCNES all omit
  this revision-dependent corner.
- **Mapper completion (F3): 86 families promoted BestEffort → Curated** with a
  commercial-ROM boot-snapshot oracle (57 already-staged + 29 sourced from GoodNES
  v3.23b). The tier split is now **51 Core + 95 Curated + 26 BestEffort = 172**,
  taking oracle-gated coverage from **60 → 146** of 172 families. The 26 still
  BestEffort have no cleanly-booting dump (16 NES 2.0 high-id boards + 8 with no
  matching cart + 2 whose only dump jams at boot) and stay register-decode +
  save-state unit-tested only.
- **MMC3 R1/R2 scanline-IRQ residual CLOSED (ADR 0002 F5.0).** The instrumentation-
  first review confirmed the residual is a differential 1-dot deficit that is
  structurally unreachable on the one-clock batched-catch-up model (21+ falsified
  levers; zero production-ROM impact), so it is now closed by-design-permanent, not
  deferred. All **20** `#[ignore]`'d tests are catalogued with dispositions in the
  new `docs/accuracy-ledger.md` — none is an accuracy gap.
- **Doc reconciliation (F0).** `docs/mappers.md` + `docs/compatibility.md` corrected
  (MMC5 vertical split-screen + audio and the Vs. `DualSystem` core are implemented,
  not deferred); new `docs/accuracy-ledger.md` maps every approximation to its
  disposition (remediated / no-stricter-oracle / deferred / out-of-scope).
- Version bump: workspace `2.0.8 → 2.1.0`. Mobile `MARKETING_VERSION`s are unchanged
  (the apps re-release at v2.2.0).

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

## [2.0.4] - 2026-07-08 - "Harbor" (Android release candidate — "Slipway")

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

## [2.0.3] - 2026-07-08 - "Harbor" (2-cycle-ALE promoted to default — shipped AccuracyCoin 141/141 — "Keel")

- The 2-cycle-ALE octal-latch PPU fetch model is promoted to the shipped default
  (ADR 0030) — **shipped AccuracyCoin is now 141/141 (100%)**; both the "ALE +
  Read" and "Hybrid Addresses" PPU tests now pass on the default build.
- Two commercial titles render more TriCNES-faithfully at a mid-render `$2006`
  scroll write — Super Mario Bros. 3 and Uchuu Keibitai SDF.
- The Android `play` flavor gains its full (still-dormant) monetization surface
  (AppLovin MAX + RevenueCat); the `foss` flavor keeps a no-op twin.
- Netplay rollback-determinism fix (new PPU snapshot v5 tail); headless frame
  cost rises ~10% (still ~4x realtime), accepted for the accuracy gain.

## [2.0.2] - 2026-07-08 - "Harbor" (octal-latch PPU model — AccuracyCoin 141/141 flag-on — "Soundings")

- A new octal-latch multiplexed-bus PPU model (ADR 0030) ships **default-off**:
  flag-on it reaches AccuracyCoin 141/141, while the shipped default stays
  byte-identical to v2.0.1 at its honest 139/141.
- The model faithfully reproduces the NES PPU's pin-multiplexed VRAM bus
  (74LS373-class octal latch), modeling the two corruption events behind the
  "ALE + Read" and "Hybrid Addresses" tests.
- The correct oracle was identified as TriCNES (the AccuracyCoin author's own
  emulator), not Mesen2; promotion to the default is the deliberate v2.0.3 step.

## [2.0.1] - 2026-07-08 - "Harbor" (first Android re-port onto Timebase + AccuracyCoin re-sync + housekeeping — "Mooring")

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

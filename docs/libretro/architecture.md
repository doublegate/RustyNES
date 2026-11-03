# RustyNES Libretro Core Integration Architecture

## Executive Overview

The **Libretro API** is a C ABI that separates an emulation engine from its
host (windows, audio devices, controllers). This document describes how the
**RustyNES** core, a pure-Rust, cycle-accurate NES emulator on a single
master clock, is packaged as a libretro core. It is the spec for
`crates/rustynes-libretro`; when the two disagree, the code and its tests win
and this file is corrected.

## Target Workspace and Compile Targets

`rustynes-core` is `#![no_std]` + `alloc`; it knows nothing of the host. The
wrapper crate `rustynes-libretro` builds with
`crate-type = ["cdylib", "staticlib"]`:

1. **`cdylib`** — the shared library RetroArch `dlopen`s / `LoadLibrary`s
   (`rustynes_libretro.so` / `.dll` / `.dylib` after the crate `Makefile`'s
   copy step). Its exports are the 50 `retro_*` entry points plus
   `__retro_init_core`, the link-time hook `rust-libretro`'s `retro_core!`
   generates (`docs/audits/libretro-disposition.md` L-3.3).
2. **`staticlib`** — for the platforms whose RetroArch links cores statically
   (iOS, tvOS).
3. **`panic = "unwind"`** — the libretro core is built to unwind, on every
   supported path (the crate `Makefile`, the libretro buildbot, the
   `libretro-cross` CI gate), so a panic can be contained at the core's
   boundary instead of aborting RetroArch. The workspace release profile
   aborts; a direct `cargo build --release` of this crate warns
   (`build.rs`) and logs it at load. The desktop and web builds keep
   aborting.
4. **`no_std` preserved** — `rustynes-core` is a path dependency with
   `default-features = false`; the wrapper uses `std` for the host side only.
5. **`platform=libnx` is dropped (v2.9.8, maintainer decision):** the crate
   `Makefile` now stops with an explicit `$(error ...)` rather than fall
   through to a host build. The reasoning, from the v2.9.0 re-audit (L-3.2
   side note): it mapped to `aarch64-nintendo-switch-freestanding`, a tier-3
   target: `rustc --print target-list` knows it, `rustup target list` offers
   no `rust-std` for it, and a `*-freestanding` target has no `std`, while
   this wrapper needs `std` (`std::fs` for the FDS BIOS and disk saves,
   `CString`, `std::panic::catch_unwind`). Not attempted: a nightly
   `-Zbuild-std` build, which is the only way to try it. The libretro
   buildbot never reads the Makefile's platform table and has no Rust
   template for the Switch (`.gitlab-ci.yml` header), so no shipped build is
   affected, and the buildbot matrix is unchanged. A fix would need a nightly
   `-Zbuild-std` build and a `no_std` wrapper without `catch_unwind`, which
   the panic-containment design depends on. **Re-checked, with a build, on
   2026-10-23: see "Console targets" below.**

## Console targets: Switch, PS Vita, 3DS (investigated 2026-10-23, v3.6.0, D28)

`T-LIBRETRO-CONSOLES`, ticket LR-04. Decision recorded per target: **no-go
for all three**, for the reasons below; nothing was added to `.gitlab-ci.yml`.
Each reason is the state on this date, with what would change it.

**What was checked, and how.**

* The Rust targets, against the nightly platform-support pages
  (`doc.rust-lang.org/nightly/rustc/platform-support.html` and each
  target's page) and `rustc +1.99.1 --print target-list`, which lists all
  three.
* The libretro buildbot templates, by listing
  `libretro-infrastructure/ci-templates` on `git.libretro.com` (69 files).
  The Rust templates are `rust-android-jni`, `rust-apple`, `rust-ios`,
  `rust-linux-{x64,i686,aarch64}`, `rust-osx`, `rust-webos` and
  `rust-windows-{x64,i686}`: none for a console. The console templates are
  C ones (`libnx-static.yml`, `vita-static.yml`, `ctr-static.yml`): each runs
  `make platform=<x>` in a devkitPro or VitaSDK image, takes the core's
  `<core>_libretro_<x>.a`, and links it into RetroArch's own static build
  from that RetroArch job's artefacts. The images
  (`libretro-build-devkitpro`, `libretro-build-libnx-devkitpro`,
  `libretro-build-vita` Dockerfiles, read the same day) install no Rust;
  `libretro-build-rust` installs no devkitPro or VitaSDK.
* A build of this crate's staticlib for each target, in a scratch target
  directory, with the installed nightly (`rustc 1.101.0-nightly
  7189d39d9 2026-10-22`) and `-Zbuild-std`; the project's pinned toolchain
  was not changed. devkitARM, devkitA64 and VitaSDK are not installed here,
  so nothing was linked or run.

| Target | Rust tier, `std` | Build here | Buildbot | Decision |
| --- | --- | --- | --- | --- |
| Switch, `aarch64-nintendo-switch-freestanding` | 3, `*` (no `std`) | **fails**: with a bindgen override, `std` itself does not compile for the target (`cfg_select!` finds no allocator or I/O-error module; `no_threads` on a target with threads) | no Rust template | **no-go** |
| PS Vita, `armv7-sony-vita-newlibeabihf` | 3, `std` | **compiles** (`std,panic_unwind`, 5 min 48 s), 50 `retro_*` exports | no Rust template; the Vita image has no Rust | **no-go** |
| 3DS, `armv6k-nintendo-3ds` | 3, `std` "?" (partial, via devkitARM + `pthread-3ds` / `shim-3ds`) | **compiles** (`std,panic_unwind`, 5 min 33 s), 50 `retro_*` exports | no Rust template; the devkitPro image has no Rust | **no-go** |

**Switch: v2.9.8's reason holds, now checked by building rather than
reading.** The only Rust target for the console is freestanding, without
`std`, and this wrapper, `rust-libretro` and `rust-libretro-sys` need it
(`std::fs` for the FDS BIOS and disk saves, `CString`, `catch_unwind`). The
build first fails in bindgen (`version 'freestanding' in target triple ...
is invalid`); with `BINDGEN_EXTRA_CLANG_ARGS_aarch64_nintendo_switch_freestanding=--target=aarch64-none-elf`
it gets past that and fails in `std`. Would unblock it: a Rust target for
the Switch that builds `std` against libnx (none exists upstream), or a
`no_std` wrapper with no `catch_unwind`, which gives up the core's panic
containment; plus a Rust job on a libnx image.

**Vita and 3DS: the code compiles; everything after that is missing.**

* *Toolchain.* Both need nightly `-Zbuild-std` (no `rust-std` ships for a
  tier-3 target). The buildbot pins this project's stable toolchain and the
  `libretro-cross` gate fails when `.gitlab-ci.yml`'s `RUSTUP_TOOLCHAIN`
  differs from `rust-toolchain.toml`, so a console job would be the one
  exception, on a moving nightly.
* *Image.* No libretro image has both Rust and devkitARM or VitaSDK. A job
  could install rustup at run time in the console image, or libretro could
  build an image with both; either is libretro infrastructure's change to
  make or accept, and the buildbot runs only on a push, which this
  investigation did not make.
* *Link.* Not attempted (no devkitARM or VitaSDK). The staticlibs leave
  undefined what the console's libc and RetroArch's link must supply: libc
  and file calls, `pthread_*`, the ARM EHABI unwinder (`_Unwind_*`,
  `__aeabi_unwind_cpp_pr*`), and on the 3DS `getrandom` and
  `__aeabi_read_tp`, which the Rust 3DS page says come from `shim-3ds`,
  `pthread-3ds` or equivalents that RetroArch's 3DS build does not link
  today.
* *bindgen.* The Vita triple also needs
  `BINDGEN_EXTRA_CLANG_ARGS_armv7_sony_vita_newlibeabihf=--target=armv7a-none-eabihf`
  (clang rejects `newlibeabihf` as a triple version), as MinGW needs its
  override (`.cargo/config.toml`). The 3DS triple needed none.
* *Speed, the deciding reason, and NOT measured.* The core costs about 4 ms
  per NTSC frame on the development host, an Intel i9-10850K
  (`docs/performance.md`), about 4 times faster than real time. Full speed on
  a console needs a CPU no more than about 4 times slower than that host on
  this single-threaded workload. The Vita's Cortex-A9 runs at up to 444 MHz
  and the 3DS's ARM11 at 268 MHz (804 MHz on the New 3DS); by clock alone
  that is 10 to 20 times slower, before the difference in instructions per
  clock. No figure was measured on either console or an emulator of one, so
  this is a reasoned expectation, not a result: it would take a Vita or 3DS
  build that links, run on hardware or in Vita3K or Citra, to settle.

**What would reopen this:** a measured frame time on the device within the
16.6 ms budget (or a decision that a slower-than-real-time core is worth
shipping), a libretro image with Rust and the console SDK, and a link of
the staticlib into RetroArch's static build for the console. Until then
the buildbot matrix is unchanged and the core's `.info` claims no console.

## The Abstraction Layer (`rust-libretro`)

* **`rust-libretro-sys`** — the raw `bindgen` C types. **Vendored and patched**
  in `vendor/rust-libretro-sys` (v2.8.0): the crates.io 0.3.2 binding reduced
  `struct retro_game_info` to one opaque byte, so the standard load path could
  not work; the vendored copy writes the struct out by hand
  (`vendor/rust-libretro-sys/VENDORED.md`, credited in `NOTICE`).
* **`rust-libretro`** — the `Core` trait and the exported `retro_*`
  trampolines, from crates.io. It keeps one process-global core instance,
  created on the first `retro_get_system_info` and never freed.

## System Topology & State Management

### 1. The Libretro Frontend (RetroArch)

Draws the picture, opens the audio device, reads controllers, and calls
`retro_run` once per video frame (~60.0988 Hz NTSC, ~50.007 Hz PAL; the core
reports the loaded cartridge's region in `retro_get_system_av_info`).

### 2. The FFI Bridge (`rustynes-libretro`)

`RustyNesLibretro` implements `rust_libretro::core::Core`.

* **State:** `nes: Option<Nes>` for an ordinary cartridge, or
  `dual: Option<Box<VsDualSystem>>` for one of the four Vs. `DualSystem`
  cabinet boards (two cross-wired consoles, presented side by side at
  512x240). Exactly one is `Some` while a game is loaded (`Emu::from_rom`
  decides).
* **Buffers:** reusable video, audio and serialize buffers, sized once, so a
  `retro_run` does not allocate in steady state. Three exceptions, all
  measured or bounded: a Vs. `DualSystem` cabinet's serialize reuses the same
  buffer through `VsDualSystem::snapshot_into` since v2.9.1 (NL-09; it built
  a fresh ~645 KB state with thumbnails on every call before, about 8x
  slower, `docs/performance.md`), and a dual RESTORE still allocates one
  console's backup per call, measured as no cost worth pooling; and an FDS
  game that has written to its disk has the image built once, a second after
  the write, to save it (`persist_fds_disk`). The serialize buffer is filled
  at load by the snapshot that sizes `retro_serialize_size` -- for a single
  console with the expansion-device headroom reserved; a Vs. cabinet attaches
  no device, so its size is the snapshot's exact length -- so no
  `retro_serialize` grows it: until
  v2.9.2 the first one did, during the first run-ahead or rollback frame
  (audit AUD-17). A cabinet's 512x240 image is composed over the previous
  frame's, every byte rewritten, with no per-frame zero-fill (AUD-19).
* **Containment:** every callback that runs emulation goes through
  `contained`, which stops a panic there, logs it through the frontend, and
  marks the core poisoned. The console is kept, not dropped, because the
  frontend holds pointers into its memory.
* **Frontend memory:** WRAM, the cartridge's PRG-RAM and nametable RAM are
  handed to RetroArch through `SET_MEMORY_MAPS` (cheats, RetroAchievements)
  and are withdrawn with an empty map before the console is dropped on
  unload, or at `retro_deinit` for a frontend that skips `retro_unload_game`
  (v2.9.2, audit AUD-16; after an unload deinit sends nothing). PRG-RAM is
  flagged, and exposed as `RETRO_MEMORY_SAVE_RAM` for the
  `.srm`, only when the header declares a battery (v2.9.0;
  `advanced_features.md`). On a self-flashable board the `.srm` is the flash
  image, `Nes::save_data`, and the memory map still describes only `$6000` RAM
  (v2.9.6).

### 3. The Emulation Engine (`rustynes-core`)

The deterministic core. The bridge calls `run_frame()`; the engine runs one
frame on its single master clock, with every CPU cycle clocked in two halves
and the PPU caught up to each (ADR 0002 / ADR 0029; the pre-v2.0.0
dot-lockstep loop is retired). It returns an RGBA8 framebuffer and makes
bipolar, DC-blocked `f32` audio available through `drain_audio_into`. It knows
nothing about RetroArch.

## Testing the boundary

`crates/rustynes-libretro/src/abi_tests.rs` is a C-ABI harness: it stands in
for a frontend, calling the exported `retro_*` functions with fake callbacks,
and asserts on what the core hands back — loads with and without
`GET_GAME_INFO_EXT`, save-state sizing, memory-map withdrawal, panic
containment, logging, input polling, descriptors, the Four Score option.
Because `rust-libretro` has one global instance, those tests share one core
and serialize on a lock.

## Thread Safety and Mutability

libretro calls the core from one thread. `rust-libretro` stores the core in a
`static mut` and relies on that; `RustyNesLibretro` mutates its state only
inside the callbacks, so no state is shared across threads.

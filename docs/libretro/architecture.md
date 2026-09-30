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
5. **`platform=libnx` in the crate `Makefile` is OPEN, and probably cannot
   build** (v2.9.0 re-audit, L-3.2 side note; checked by reading, not by
   building). It maps to `aarch64-nintendo-switch-freestanding`, a tier-3
   target: `rustc --print target-list` knows it, `rustup target list` offers
   no `rust-std` for it, and a `*-freestanding` target has no `std`, while
   this wrapper needs `std` (`std::fs` for the FDS BIOS and disk saves,
   `CString`, `std::panic::catch_unwind`). Not attempted: a nightly
   `-Zbuild-std` build, which is the only way to try it. The libretro
   buildbot never reads the Makefile's platform table and has no Rust
   template for the Switch (`.gitlab-ci.yml` header), so no shipped build is
   affected; the mapping is left as it is and the buildbot matrix unchanged.

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

# RustyNES Libretro Core Integration Architecture

## Executive Overview

The emulation landscape relies heavily on decoupled architectural patterns where central processing and synthesis (the emulation engine) are distinct from host OS dependencies (rendering contexts, audio streams, gamepad hooks). The **Libretro API** is the foremost C Application Binary Interface (ABI) facilitating this separation. This document outlines the exhaustive architecture required to embed the **RustyNES** core—a pure Rust, cycle-accurate, master-clock precision Nintendo Entertainment System emulator—into a dynamically linked Libretro module (`cdylib`).

## Target Workspace and Compile Targets

The `rustynes-core` crate is strictly `#![no_std]` capable. It relies exclusively on `alloc` for heap allocations, remaining entirely agnostic to the host system.
To bridge this, we establish a new crate, `rustynes-libretro`, defined explicitly with the `crate-type = ["cdylib"]` compilation target.

1. **The `cdylib` Target:** This forces the `rustc` compiler to build a C-compatible shared object (`.so` on Linux, `.dll` on Windows, `.dylib` on macOS). It strips out Rust-specific ABI metadata and embeds the required parts of the Rust standard library statically. This allows the RetroArch frontend to invoke the module dynamically via `dlopen` / `LoadLibrary` at runtime.
2. **`no_std` Preservation:** The `rustynes-libretro` crate depends on `rustynes-core` via a local path dependency with `default-features = false`. The FFI wrapper may utilize `std` to interop with the host, but the core engine remains purely deterministic and isolated.

## The Abstraction Layer (`rust-libretro`)

Direct interaction with the `libretro.h` C-header via raw `unsafe` Rust is highly error-prone, fraught with segmentation faults due to mismatched pointers and undefined behavior.
We employ the **`rust-libretro`** wrapper crate as our architectural foundation.

* **`rust-libretro-sys`**: Provides the raw, `bindgen`-generated C types.
* **`rust-libretro`**: Provides the safe Rust trait `Core`.

The `CoreWrapper` struct in `rust-libretro` securely encapsulates frontend callbacks and maintains the static state required by the C ABI.

## System Topology & State Management

The architecture operates in a strictly synchronous top-down 3-layer topology:

### 1. The Libretro Frontend (RetroArch)

The execution host. Responsible for drawing windows via OpenGL/Vulkan, opening audio devices (WASAPI/ALSA), capturing USB gamepads, and invoking the Libretro loop exactly once per video frame (~60.0988 Hz).

### 2. The FFI Bridge (`rustynes-libretro`)

The integration layer. It maintains a struct (e.g., `RustyNesLibretro`) implementing `rust_libretro::core::Core`.

* **State Maintenance:** It holds `Option<rustynes_core::Nes>`. The `Nes` is instantiated only *after* `retro_load_game` provides valid ROM bytes.
* **Buffer Management:** It maintains a persistent, reusable `Vec<i16>` allocated on the heap to act as a zero-allocation audio format converter (scaling `f32` to `i16`).
* **Translation:** Translates RetroPad boolean input queries into `rustynes_core::Buttons` bitmasks.

### 3. The Emulation Engine (`rustynes-core`)

The immutable heart. Once initialized, the FFI bridge invokes `nes.run_frame()`. The engine operates in lockstep PPU-dot resolution. It is entirely unaware that it is running inside RetroArch. It returns a contiguous RGBA8 `&[u8]` framebuffer and makes `f32` normalized audio available via `nes.drain_audio()`.

## Thread Safety and Mutability

While `rustynes-core` executes synchronously, frontends often invoke video rendering callbacks on asynchronous hardware threads (e.g., Vulkan contexts). To prevent mutex contention, `rustynes-libretro` must ensure that localized mutable state (`Nes`) is exclusively manipulated during the primary `on_run` thread slice, avoiding globally shared `static mut` state outside of the guarantees provided by `rust-libretro`'s `CoreWrapper`.

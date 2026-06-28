# RustyNES Libretro Core Exhaustive Implementation & Integration Sprint Plan

This document is the absolute source of truth for the Antigravity IDE (and any other developers or autonomous agents) when implementing the `rustynes-libretro` core integration for RetroArch. It expands upon the original architectural blueprint, defining strict code-level constraints, integration mechanisms, mathematics, and file structures.

This must be followed sequentially.

---

## Phase 1: Build System Bootstrapping & CDYLIB Workspace Configuration

**Goal:** Initialize the integration crate and configure Cargo for C-ABI exports while preserving the `no_std` nature of the core.

### 1.1: Crate Instantiation

* Create a new folder at `crates/rustynes-libretro/`.
* Initialize a standard Cargo library: `cargo init --lib crates/rustynes-libretro`.

### 1.2: `Cargo.toml` Configuration

* In `crates/rustynes-libretro/Cargo.toml`, set the crate type to `cdylib` to instruct the compiler to generate a `.so`, `.dll`, or `.dylib`.

  ```toml
  [lib]
  crate-type = ["cdylib"]
  ```

### 1.3: Dependencies

* Add `rustynes-core` as a path dependency. **CRITICAL:** You must declare `default-features = false` to prevent injecting `std` and breaking the core's purity.

  ```toml
  [dependencies.rustynes-core]
  path = "../rustynes-core"
  default-features = false
  ```

* Add the abstraction libraries `rust-libretro` and `rust-libretro-sys`.

  ```toml
  [dependencies]
  rust-libretro = "0.1" # (or the latest compatible semantic version)
  ```

* Add `Cargo.toml` of `rustynes-libretro` to the top-level workspace `Cargo.toml` if it's not automatically included.

---

## Phase 2: Core Lifecycle & Dummy State Negotiation

**Goal:** Prove FFI boundary stability and RetroArch frontend communication without executing emulation logic.

### 2.1: The `CoreWrapper` Setup

* In `crates/rustynes-libretro/src/lib.rs`, import `rust_libretro::core::Core`.
* Create a struct `RustyNesLibretro` that will hold the emulator state:

  ```rust
  struct RustyNesLibretro {
      nes: Option<rustynes_core::Nes>,
      // Buffers for conversion
      audio_buffer: Vec<i16>,
  }
  ```

* Implement the `rust_libretro::core::Core` trait for `RustyNesLibretro`.

### 2.2: `retro_get_system_info`

* Implement `get_system_info()`.
* **library_name:** `"RustyNES"`
* **library_version:** Use `rustynes_core::version()` (e.g., `env!("CARGO_PKG_VERSION")`).
* **valid_extensions:** `"nes|fds"`
* **need_fullpath:** `false` (forces RetroArch to provide the ROM buffer in RAM, avoiding file I/O).
* **block_extract:** `false`

### 2.3: Environment Handshake (`on_set_environment`)

* Override the `on_set_environment` method.
* **Pixel Format:** Call the environment's `set_pixel_format(rust_libretro::core::PixelFormat::XRGB8888)`. If this fails, log an error (or fallback to `RGB1555` with a conversion loop, but `XRGB8888` should be universally supported by modern frontends).
* **Input Descriptors:** Construct an array of `retro_input_descriptor` to map RetroPad constants to NES strings:
  * `RETRO_DEVICE_ID_JOYPAD_A` -> "NES A Button"
  * `RETRO_DEVICE_ID_JOYPAD_B` -> "NES B Button"
  * `RETRO_DEVICE_ID_JOYPAD_UP` -> "D-Pad Up"
  * etc.
  Pass this to `set_input_descriptors()`.

### 2.4: Compilation & Smoke Test

* Run `cargo build -p rustynes-libretro --release`.
* Verify that a `.so` (Linux), `.dll` (Windows), or `.dylib` (Mac) is produced in `target/release/`.
* Test via CLI: `retroarch -L target/release/librustynes_libretro.so`. RetroArch should open without crashing and identify the core.

---

## Phase 3: ROM Loading, Geometry, and Synchronous Execution

**Goal:** Instantiate the `rustynes_core::Nes`, map inputs, render the PPU contiguous framebuffer, and convert audio formats correctly.

### 3.1: ROM Loading (`on_load_game`)

* RetroArch passes `game: Option<&rust_libretro::core::GameInfo>`.
* Extract the data: `let data = game.unwrap().data.unwrap();`
* Check if it's an FDS file (by extension or header). For standard `.nes`:
  * Initialize the core: `let mut nes = rustynes_core::Nes::from_rom(data).unwrap();` (handle errors gracefully by returning `false` from `on_load_game`).
* Assign `self.nes = Some(nes)`.

### 3.2: AV Geometry (`get_system_av_info`)

* **Resolution:** Return base width `256`, height `240`.
* **Max Resolution:** Return width `256`, height `240`.
* **FPS:** Return `60.0988` (NTSC).
* **Sample Rate:** RustyNES defaults to `44100.0` or `48000.0`. You must match the sample rate you request during initialization (use `rustynes_core::bus::DEFAULT_SAMPLE_RATE` or instantiate `Nes::from_rom_with_sample_rate(data, 48000)` and return `48000.0`).

### 3.3: Input Polling (`on_run`)

* At the top of `on_run(ctx: &mut rust_libretro::core::RunContext)`, poll the input.
* Query the Joypad state for Port 1. Map `ctx.get_joypad_bit(0, rust_libretro::core::JoypadButton::A)` to `rustynes_core::Buttons::A`, etc.
* Set the input on the `Nes` (likely through `nes.set_controller_state(0, buttons)` -- verify exact `Controller` API in `nes.rs`).

### 3.4: Frame Execution & Video Refresh (`on_run`)

* Execute exactly one frame: `let framebuffer = nes.run_frame();`.
* `run_frame()` returns an `&[u8]` RGBA8 buffer of size `256 * 240 * 4 = 245760` bytes.
* Verify the pitch: `width (256) * 4 bytes per pixel = 1024`.
* Pass the video buffer to Libretro:
  `ctx.draw_frame(framebuffer, 256, 240, 1024);`
* **Frame Duplication Optimization:** If skipping frames (check frontend fast-forward flags), pass empty buffers appropriately.

### 3.5: Audio Batch Rendering (`on_run`)

* RustyNES outputs `f32` audio samples normalized to `[0.0, 1.0]` (or `[-1.0, 1.0]`).
* Call `nes.drain_audio()` or `nes.drain_audio_into(&mut buf)`.
* Libretro mandates interleaved stereo `i16`.
* **Math Conversion Loop:**

  ```rust
  self.audio_buffer.clear();
  let f32_samples = nes.drain_audio();
  for &sample in &f32_samples {
      // Map f32 [0.0, 1.0] to i16 [-32768, 32767] (adjust if RustyNES outputs [-1, 1]).
      // Given bus.rs line 1793: "normalized [0, ~1]"
      let scaled = (sample * 65535.0 - 32768.0).clamp(-32768.0, 32767.0) as i16;
      // Interleaved stereo: Push Left, Push Right
      self.audio_buffer.push(scaled);
      self.audio_buffer.push(scaled);
  }
  ctx.output_audio_slice(&self.audio_buffer);
  ```

---

## Phase 4: Advanced Subsystems (RetroAchievements, GGPO, VFS)

**Goal:** Deliver preservation-grade integration by exposing memory pointers and deterministic serialization logic.

### 4.1: Direct Memory Mapping (RetroAchievements)

* Implement `RETRO_ENVIRONMENT_SET_MEMORY_MAPS`.
* `rustynes-core` has getters for WRAM and SRAM (e.g., `nes.wram_ptr()` or similar).
* Expose these directly using `retro_memory_descriptor`.
* Mappings:
  * WRAM: `$0000 - $07FF`, Flag: `RETRO_MEMDESC_SYSTEM_RAM`
  * SRAM: `$6000 - $7FFF`, Flag: `RETRO_MEMDESC_SAVE_RAM`
  * VRAM: `$2000 - $2FFF`, Flag: `RETRO_MEMDESC_VIDEO_RAM`
* This guarantees zero-overhead `rcheevos` network tracking.

### 4.2: Save RAM File Offloading (SRAM/Battery)

* Implement `on_get_memory_data` and `on_get_memory_size`.
* When the frontend requests `RETRO_MEMORY_SAVE_RAM`, return a mutable pointer to the SRAM.
* RetroArch will automatically read/write `.srm` files to the disk on shutdown/startup, saving you from dealing with `std::fs` operations.

### 4.3: Deterministic Serialization (GGPO & TAS)

* Expose RustyNES's `save_state` system via Libretro's serialization hooks:
  * `on_serialize_size()`: Return a strict, unchanging `usize` buffer size.
  * `on_serialize(buffer: &mut [u8])`: Delegate to `rustynes_core::save_state::BinWriter` to serialize the emulator state.
  * `on_unserialize(buffer: &[u8])`: Delegate to `rustynes_core::save_state::BinReader` to restore.
* **CRITICAL:** `serialize_size` must never change once `on_load_game` is finished, or GGPO rollback instances will fault.

### 4.4: Famicom Disk System (VFS / BIOS Handling)

* Implement Libretro subsystem hooks for `.fds` handling.
* If a game is loaded via the `FDS` subsystem, use Libretro's VFS (Virtual File System) callbacks to securely locate and load the `disksys.rom` BIOS without using `std::fs`.
* Delegate to `Nes::from_disk(disk_bytes, bios_bytes)` inside the modified loader.

---

## Conclusion & Code Standards

* **NO `std` in CORE:** Under no circumstances should `rustynes-libretro` modifications leak standard library usage into `rustynes-core`.
* **Zero Allocations in `on_run`:** Do not use `vec!` or `String` allocations inside the `on_run` cycle to maintain optimal performance and prevent micro-stutters during execution. Reuse `self.audio_buffer`.
* **Error Handling:** Gracefully fail `on_load_game` using `false` if `RomError` is returned; never `panic!()`.

# RustyNES Libretro Core Implementation Plan

This document outlines the exact execution sequence to build out the `rustynes-libretro` core integration, strictly adhering to the architectural constraints established in `to-dos/libretro/SPRINT_PLAN.md` and `docs/libretro/*`.



## Proposed Changes

---

### Phase 1: Build System & Dependency Wiring

#### [NEW] [rustynes-libretro/Cargo.toml](file:///home/parobek/Code/OSS_Public-Projects/RustyNES/crates/rustynes-libretro/Cargo.toml)
Initialize the workspace member crate as a dynamic library.
- Set `crate-type = ["cdylib"]`.
- Link `rustynes-core` via path with `default-features = false`.
- Add `rust-libretro` as the FFI wrapper.

#### [MODIFY] [Cargo.toml](file:///home/parobek/Code/OSS_Public-Projects/RustyNES/Cargo.toml) (Root)
- Add `"crates/rustynes-libretro"` to the `[workspace.members]` array.

---

### Phase 2: Core Lifecycle & Dummy Initialization

#### [NEW] [rustynes-libretro/src/lib.rs](file:///home/parobek/Code/OSS_Public-Projects/RustyNES/crates/rustynes-libretro/src/lib.rs)
Establish the C ABI FFI boundary.
- Import `rust_libretro::core::{Core, CoreEnvironment, GameInfo}`.
- Define `struct RustyNesLibretro { nes: Option<rustynes_core::Nes>, audio_buffer: Vec<i16> }`.
- Implement `Core` for `RustyNesLibretro`.
- Define initialization via `rust_libretro::core_macros::libretro_core!(RustyNesLibretro)`.

---

### Phase 3: ROM Loading, Input, Video, and Audio Math

#### [MODIFY] [rustynes-libretro/src/lib.rs](file:///home/parobek/Code/OSS_Public-Projects/RustyNES/crates/rustynes-libretro/src/lib.rs)
- **`on_load_game`:** Retrieve ROM bytes from RetroArch's RAM buffer and instantiate `Nes::from_rom()`.
- **`on_run`:**
  - **Input:** Translate RetroArch joypad polling into `rustynes_core::Buttons` bitmasks and push to `nes`.
  - **Video:** Call `nes.run_frame()` and forward the 256x240 RGBA8 framebuffer via `ctx.draw_frame(buffer, 256, 240, 1024)`.
  - **Audio Batching:** Call `nes.drain_audio()`, loop over the `f32` array, execute the scaling calculation `(sample * 65535.0 - 32768.0) as i16`, and push left/right interleaving to the `audio_buffer`. Emit via `ctx.output_audio_slice()`.

---

### Phase 4: Advanced Subsystems (Memory Maps & SRAM)

#### [MODIFY] [rustynes-libretro/src/lib.rs](file:///home/parobek/Code/OSS_Public-Projects/RustyNES/crates/rustynes-libretro/src/lib.rs)
- **Direct Memory Maps:** Expose WRAM (`$0000-$07FF`), SRAM (`$6000-$7FFF`), and VRAM (`$2000-$2FFF`) using RetroArch's environment hooks for RetroAchievements zero-cost scanning.
- **Save State Hooks:** Connect `save_state::BinWriter` to `on_serialize` to enable GGPO rollback netplay.

## Verification Plan

### Automated Compilation
- I will run `cargo build -p rustynes-libretro --release` to ensure the `cdylib` compiles successfully.
- I will run `cargo test --workspace` to ensure none of the dependencies injected broken `std` references back into the `no_std` core.

### Manual Verification
- I will ask you (the user) to load the compiled `.so`/`.dll` core in RetroArch along with a test ROM to verify visual output, input polling, and audio synthesis.

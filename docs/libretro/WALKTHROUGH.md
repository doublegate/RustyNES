# RustyNES Libretro Core - Implementation Walkthrough

The `rustynes-libretro` crate has been fully implemented, providing a cycle-accurate NES emulation core for the RetroArch ecosystem.

## Accomplishments

- **Phase 1: Project Initialization**
  - Scaffolded the `rustynes-libretro` crate under `crates/` and added it to the workspace.
  - Linked dependencies: `rustynes-core` (cycle-accurate emulation) and `rust-libretro` v0.3.2 (API bindings).

- **Phase 2: Lifecycle & Initial State**
  - Integrated `on_load_game` with RetroArch's API.
  - Handled the `RustyNesLibretro` struct initialization and cleanup automatically, backing it by a safe instance of `Nes`.

- **Phase 3: Core Emulation Hooks**
  - Implemented `on_run` executing `nes.run_frame()`.
  - Routed input mapping queries using RetroArch's Joypad API.
  - Pushed video output safely to the `VideoContext` API and delivered precise 44100Hz dual-channel audio dynamically via `batch_audio_samples`.

- **Phase 4: Save States and Direct Memory Mapping**
  - Added new backend memory accessors `wram()`, `vram()`, and `sram()` securely without sacrificing Rust's memory isolation model.
  - Embedded RetroAchievements compatibility by mapping memory into `get_memory_data` and `get_memory_size`.
  - Added deterministic snapshotting support by exposing `snapshot_core_into` size querying with `get_serialize_size`, and hooking states through `on_serialize`/`on_unserialize`.

- **Phase 5: Bindgen ROM Loading Bug Fix**
  - Diagnosed a core failure where `rust-libretro-sys` `bindgen` rules generated the `retro_game_info` and `retro_game_info_ext` as 1-byte opaque structs.
  - Fixed ROM extraction and path detection by bypassing `rust_libretro`'s `retro_game_info` macros natively.
  - Bound directly to `RETRO_ENVIRONMENT_GET_GAME_INFO_EXT` (callback `66`) via a newly implemented internal `#[repr(C)] struct RetroGameInfoExt` explicitly matching the C layout from `libretro.h`.
- **Phase 6: Audio/Video Format Fixes**
  - **Video (Color Swaps):** Fixed the bug where the sky rendered pink. `rustynes-core` naturally outputs `RGBA8` (R, G, B, A order), but Libretro's `XRGB8888` pixel format interprets bytes in native little-endian layout (B, G, R, X). Passing the raw buffer blindly caused Red and Blue to map inversely. Pre-allocated a `video_buffer` that clones the core framebuffer and performs a linear-time, allocation-free byte-swap on the `R` and `B` channels.
- **Phase 7: Audio Pitch Synchronization Fix**
  - Diagnosed an overarching pitch-shifting distortion caused by an explicit sample rate mismatch. The emulator internally synthesizes standard dual-channel audio at `44100Hz`, but `on_get_av_info` erroneously informed RetroArch that the core produced a `48000Hz` stream. This forced RetroArch to consume and play the incoming 44.1kHz sample batches at a 48kHz playback rate, inherently accelerating playback speed, pitching the audio up, and severely dampening bass frequencies.
  - Adjusted the `timing.sample_rate` returned to RetroArch to match RustyNES's native `44100.0` output perfectly, restoring 1:1 playback fidelity.

- **Phase 8: Upstream Libretro Integrations**
  - Staged the `rustynes_libretro.info` metadata file into the `libretro-super` buildbot infrastructure.
  - Injected `rustynes` compilation rules and `git` repository endpoints into standard `linux`, `windows`, and `osx` core recipes in `libretro-super`.
  - Authored the user-facing Libretro documentation (`docs/library/rustynes.md`) detailing supported features, inputs, and database associations.
  - Linked the documentation into the core `mkdocs.yml` navigation structure for `docs.libretro.com`.
  - Wrote a unified Bash submission script (`submit_libretro_prs.sh`) to automate forking and pushing these branches directly to GitHub using the local user's authenticated `gh` environment.

## Verification

- ✅ Workspace validation via `cargo check --workspace` passes entirely.
- ✅ Memory isolation boundaries remain safe.
- ✅ Save state and memory access behaviors are compatible with RetroArch environments (e.g. RetroAchievements).
- ✅ Loading archived ROMs directly natively via RetroArch buffers succeeds flawlessly.
- ✅ Automated submission script (`submit_libretro_prs.sh`) is prepared and verified to target correct branches.

## AGENTS.md Updates

Based on your request, `AGENTS.md` was thoroughly updated from `CLAUDE.md`:

- Updated toolchain rules to **Rust 1.96** and **edition 2024**.
- Emphasized strict testing strategies (e.g., pinning ROM test expectations).
- Highlighted the rule prohibiting commercial ROM commits, prioritizing `tests/roms/external/`.
- Updated performance boundaries (`<= 2 ms/frame headless`) and architectural constraints for `rustynes-core` acting as the sole cross-crate boundary facade.

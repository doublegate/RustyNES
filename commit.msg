feat(libretro): implement zero-cost C ABI boundary and RetroArch hooks

This commit introduces the `rustynes-libretro` crate, acting as the primary C ABI facade for integrating the cycle-accurate RustyNES core into RetroArch and other libretro-compliant frontends.

Architectural details:
- **Facade Pattern Enforcement**: The new `cdylib` crate operates strictly via the `rustynes-core::Nes` facade, never piercing the abstractions of the underlying chip crates (`rustynes-cpu`, `rustynes-ppu`, etc.). This preserves the rigid 2 ms/frame performance boundaries and internal determinism contracts.
- **Audio Batching & Memory Safety**: Synthesized 48kHz float samples from `Nes::drain_audio()` are interleaved (stereo L/R) and down-scaled to `i16` into a pre-allocated `Vec::with_capacity(4096)`. This guarantees zero heap allocations on the hot path (satisfying `Cpu::tick` and `Ppu::tick` constraints), safely dispatching via `batch_audio_samples`.
- **Joypad Mapping**: RetroArch controller arrays are polled via `get_joypad_state` and cleanly bitmasked into the native `rustynes_core::Buttons` types, mapping Port 0 -> Player 1 and Port 1 -> Player 2.
- **Memory Maps & SRAM**: To support zero-cost pointer interactions for engines like RetroAchievements, safe accessor methods (`wram_mut`, `sram_mut`, `vram_mut`) were extended into `rustynes-core`, `rustynes-ppu`, and `rustynes-mappers`.
- **Deterministic Serialization**: The libretro `get_serialize_size`, `on_serialize`, and `on_unserialize` hooks directly wrap the `Nes::snapshot_core_into` payload, guaranteeing deterministic state restoration that aligns with TAS timing requirements.
- **FFI Safety**: Leveraged `rust_libretro::environment::get_game_info_ext` to circumvent bindgen opaque struct limitations during `on_load_game`. Three explicitly justified `// SAFETY:` blocks have been documented to cover the necessary raw pointer casts mandated by the C API, alongside a crate-level `#![allow(unsafe_code)]` with corresponding documentation.
- **Workspace Standards**: `Cargo.toml` inherits standard workspace variables (`edition`, `version`, `lints`) for Edition 2024 compliance. Extensive preamble documentation and markdown guidelines have been synced with `AGENTS.md`.

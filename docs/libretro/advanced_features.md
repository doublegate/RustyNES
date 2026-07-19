# RustyNES Libretro Core Advanced Subsystems & Features

To fulfill RustyNES's preservation-grade feature set (GGPO rollback, RetroAchievements, FDS support), the FFI wrapper must deeply integrate with advanced `libretro.h` subsystems, exposing the strict deterministic capabilities of the `rustynes-core` engine.

## Direct Memory Mapping & RetroAchievements (implemented)

Traditionally, achievement networks like `rcheevos` utilize `READ_CORE_RAM` function pointers, which incur massive function-call overhead by querying memory one byte at a time.
`RustyNesLibretro::register_memory_maps` (`crates/rustynes-libretro/src/lib.rs`) bypasses this via `RETRO_ENVIRONMENT_SET_MEMORY_MAPS`, called at the end of `on_load_game` on the `LoadGameContext` (this hook is only available there and on `InitContext` — **not** on `SetEnvironmentContext`, since the memory pointers aren't known until a ROM is loaded). It exposes an array of `retro_memory_descriptor` structures directly mapping the emulator's 6502 CPU virtual address space to physical Rust heap pointers:

* **Work RAM (WRAM):** Maps address range `$0000 - $07FF`. Flagged as `RETRO_MEMDESC_SYSTEM_RAM`.
* **Save RAM (SRAM):** Maps address range `$6000 - $7FFF`, when non-empty (battery-backed carts only). Flagged as `RETRO_MEMDESC_SAVE_RAM`.
* **Video RAM (VRAM):** Maps address range `$2000 - $2FFF`. Flagged as `RETRO_MEMDESC_VIDEO_RAM`.

The legacy `get_memory_data`/`get_memory_size` (`RETRO_MEMORY_*`) pointer path is kept alongside this, unchanged — RetroArch's own `.srm` persistence goes through it regardless, so the descriptor registration is additive, not a replacement. Both paths expose the MAIN console's memory in Vs. `DualSystem` mode (see `RustyNesLibretro::active_nes_mut`/`active_nes`).

## SRAM and Virtual File System (VFS) Offloading

As `rustynes-core` is `no_std`, it possesses no ability to interact with the host OS filesystem (`std::fs`), making native `.srm` (battery save) file writing impossible.
**Solution:** The FFI wrapper exposes the active cartridge's SRAM pointer via `retro_get_memory_data(RETRO_MEMORY_SAVE_RAM)`.

* RetroArch automatically manages the lifecycle. Upon game load, the frontend injects data from the host's `.srm` file directly into this pointer.
* Upon shutdown (`retro_deinit`), RetroArch reads the pointer and flushes the data to the disk.

This architectural inversion ensures compatibility with RetroArch Cloud Sync, mobile sandboxes (iOS/Android), and cross-platform save transfers without touching native filesystem APIs.

## Deterministic Serialization for GGPO & TAS

Rollback netplay (GGPO) masks network latency by simulating speculative remote player inputs. When actual inputs arrive later, RetroArch performs a "rollback" by instantly restoring a past emulator state (`retro_unserialize`), applying the true inputs, and fast-forwarding (`retro_run`) to catch up silently.
To support this, `rustynes-libretro` relies on the deterministic serialization engine found in `rustynes_core::save_state`.

1. **`retro_serialize_size` Permanency:** The FFI wrapper must return a static, unchanging byte-size integer post-ROM-load. Dynamic save-state resizing will immediately fault RetroArch, as the frontend pre-allocates contiguous memory pools for rollback frames based on this initial size query.
2. **Implementation:**
   * `on_serialize(buffer: &mut [u8])`: Instantiate `rustynes_core::save_state::BinWriter` targeting the `buffer`. The engine pushes exact byte-for-byte state.
   * `on_unserialize(buffer: &[u8])`: Pass the `buffer` to `BinReader`. Determinism guarantees the state is restored perfectly without desync.
3. **Fast-Forward Optimization (`get_fastforwarding`, implemented):** `RustyNesLibretro::is_fastforwarding` queries this each frame in `run_single`/`run_dual` and skips the `push_audio` call (the `f32`→`i16` interleave plus the `batch_audio_samples` FFI push) while fast-forwarding. This is a modest, honest win, not a large one: `rustynes-core` has no mixer-bypass API, so the dominant cost — APU synthesis inside `run_frame()` — is not skipped; only the presentation-side audio conversion/push is.

## Vs. `DualSystem` Two-Screen Presentation (v2.1.10 "Web Parity")

Four Vs. arcade titles (Balloon Fight, Wrecking Crew, Tennis, Baseball) ship on
**Vs. `DualSystem`** boards: two cross-wired NES consoles in one cabinet, each with
its own screen. The core already models them (`rustynes_core::Emu::Dual` /
`VsDualSystem`); the libretro wrapper wires the present path so RetroArch shows
both screens, matching the desktop frontend.

* **Detection:** `on_load_game` calls `Emu::from_rom`, which OR's the NES 2.0
  header Vs.-hardware type with the SHA-keyed `vs_db` — the identical detection the
  desktop frontend uses. The core then holds either `nes: Option<Nes>` **or**
  `dual: Option<Box<VsDualSystem>>` (mutually exclusive). Without this, a
  `DualSystem` dump would boot a single console that hangs waiting on its absent
  cross-wired partner.
* **Composed present:** `on_run` steps **both** consoles each frame, then composes
  their two 256×240 RGBA framebuffers into a single **512×240** XRGB8888 image —
  MAIN on the left half, SUB on the right — presented via `draw_frame`. The
  advertised AV-info `max_width` is raised to **512** so RetroArch honours the
  per-frame width without a geometry renegotiation: a single-console 256×240 frame
  and a dual 512×240 frame both draw correctly against the same AV info.
* **Input:** libretro ports **0/1 → MAIN P1/P2**, ports **2/3 → SUB P1/P2**
  (matching `VsDualSystem::set_buttons`).
* **Audio:** only the **MAIN** console's audio is played (one stream, as on
  desktop); the SUB console's APU ring is drained-and-discarded to keep it bounded.
* **Save states + memory maps:** dual state serializes through
  `VsDualSystem::snapshot`/`restore` (a self-describing blob of both consoles, with
  the same static-size permanency the single path guarantees); the RA / cheat
  memory maps expose the **MAIN** console.

The deterministic `no_std` core is untouched — this is purely a parallel
present/serialize branch in the FFI wrapper, exactly mirroring the desktop
frontend's `emu.dual` branch.

## Famicom Disk System (FDS) Loading & Disk Control (implemented)

Standard NES ROMs (`.nes`) bundle all data in a single file. The Famicom Disk System requires two distinct components: the `.fds` disk image and the `disksys.rom` BIOS.

**Load path.** `on_load_game` (`crates/rustynes-libretro/src/lib.rs`) inspects the extension libretro's `GET_GAME_INFO_EXT` reports (`ext_info.ext`, valid even in in-memory/`need_fullpath = false` mode). For `.fds` content, it looks up `disksys.rom` via `RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY` (`GenericContext::get_system_directory`), reads it with `std::fs::read` (this crate links full `std`, unlike the `no_std` `rustynes-core`), and constructs via `rustynes_core::Nes::from_disk(disk_bytes, bios_bytes)`. Missing BIOS surfaces as a clear `on_load_game` error naming the expected path. **This is a simpler alternative to `RETRO_ENVIRONMENT_SET_SUBSYSTEM_INFO`/`retro_load_game_special`** (an earlier design considered but not built) — routing on the single game-load path avoids the added subsystem-registration/multi-buffer-negotiation surface for no loss of functionality, since RetroArch always resolves `disksys.rom` from the system directory the same way regardless.

**Multi-side disk swap.** Once loaded, the disk-control trait overrides (`on_set_eject_state`, `on_get_eject_state`, `on_get_image_index`, `on_set_image_index`, `on_get_num_images`, `on_get_image_path`/`on_get_image_label`) are backed by `Nes::disk_side_count`/`inserted_disk_side`/`set_disk_side` — the same API the desktop frontend's F9 disk-swap keybind uses. The callback trampolines are registered once via `GenericContext::enable_disk_control_interface()` in `on_set_environment`, surfacing swap/eject in RetroArch's Quick Menu → Disk Control. `on_get_image_path`/`on_get_image_label` synthesize "Side A"/"Side B" labels since no real per-side file paths exist for a single multi-side `.fds` container; `on_replace_image_index`/`on_add_image_index` are left at their default no-ops for the same reason.

## Native Cheats (Game Genie, implemented)

`on_cheat_set`/`on_cheat_reset` are backed by `Nes::add_genie_code`/`remove_genie_code`/`clear_genie_codes`, which are deliberately excluded from serialized state — cheats never affect save-state / netplay / TAS determinism. `RustyNesLibretro::genie_cheats` (an `index -> code` map) remembers which code was applied at each frontend-assigned cheat slot, since `on_cheat_set` only reports the code being toggled, not what was previously there. Only Game Genie code syntax is decoded — a generic RetroArch "raw address:value" poke cheat is not, so `RetroArch Cheats` (the frontend's own address-poke cheat manager) stays unsupported while `Native Cheats` (Game Genie) is supported.

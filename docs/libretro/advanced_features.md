# RustyNES Libretro Core Advanced Subsystems & Features

To fulfill RustyNES's preservation-grade feature set (GGPO rollback, RetroAchievements, FDS support), the FFI wrapper must deeply integrate with advanced `libretro.h` subsystems, exposing the strict deterministic capabilities of the `rustynes-core` engine.

## Direct Memory Mapping & RetroAchievements

Traditionally, achievement networks like `rcheevos` utilize `READ_CORE_RAM` function pointers, which incur massive function-call overhead by querying memory one byte at a time.
RustyNES must bypass this by utilizing the `RETRO_ENVIRONMENT_SET_MEMORY_MAPS` hook. This exposes an array of `retro_memory_descriptor` structures directly mapping the emulator's 6502 CPU virtual address space to physical Rust heap pointers.

* **Work RAM (WRAM):** Maps address range `$0000 - $07FF`. Flagged as `RETRO_MEMDESC_SYSTEM_RAM`.
* **Save RAM (SRAM):** Maps address range `$6000 - $7FFF`. Flagged as `RETRO_MEMDESC_SAVE_RAM`.
* **Video RAM (VRAM):** Maps address range `$2000 - $2FFF`. Flagged as `RETRO_MEMDESC_VIDEO_RAM`.

By exposing these pointers right after `retro_load_game`, RetroAchievements clients hash and observe memory natively, guaranteeing cycle-accurate achievement tracking without stalling the `on_run` loop.

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
3. **Fast-Forward Optimization (`get_fastforwarding`):** If the frontend is fast-forwarding to catch up during a rollback, the FFI wrapper should skip copying audio into the batch buffer and optionally skip rendering logic to vastly increase throughput.

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

## Famicom Disk System (FDS) Subsystem Negotiation

Standard NES ROMs (`.nes`) bundle all data in a single file. The Famicom Disk System requires two distinct components: the `.fds` disk image and the `disksys.rom` BIOS.
Using `RETRO_ENVIRONMENT_SET_SUBSYSTEM_INFO`, the core declares the "FDS" operating mode.
When selected, RetroArch triggers a specialized `retro_load_game_special` hook, passing multiple memory buffers simultaneously. The core extracts both the BIOS buffer and the Disk buffer, forwarding them securely to `rustynes_core::Nes::from_disk(disk_bytes, bios_bytes)`. This provides a pristine, CLI-free user experience entirely mediated by RetroArch's UI.

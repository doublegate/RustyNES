# RustyNES Libretro Core Advanced Subsystems & Features

To fulfill RustyNES's preservation-grade feature set (GGPO rollback, RetroAchievements, FDS support), the FFI wrapper must deeply integrate with advanced `libretro.h` subsystems, exposing the strict deterministic capabilities of the `rustynes-core` engine.

## Direct Memory Mapping & RetroAchievements (implemented)

Traditionally, achievement networks like `rcheevos` utilize `READ_CORE_RAM` function pointers, which incur massive function-call overhead by querying memory one byte at a time.
`RustyNesLibretro::register_memory_maps` (`crates/rustynes-libretro/src/lib.rs`) bypasses this via `RETRO_ENVIRONMENT_SET_MEMORY_MAPS`, called at the end of `on_load_game` on the `LoadGameContext` (this hook is only available there and on `InitContext` — **not** on `SetEnvironmentContext`, since the memory pointers aren't known until a ROM is loaded). It exposes an array of `retro_memory_descriptor` structures:

* **Work RAM (WRAM):** Maps address range `$0000 - $07FF` in the blank/default (6502 CPU) address space. Flagged as `RETRO_MEMDESC_SYSTEM_RAM`.
* **Cartridge PRG-RAM:** whenever the cartridge has any (`Nes::sram` is non-empty), in two views (v2.9.0, libretro re-audit NL-02 / NL-06; `memory_descriptors` in `lib.rs`):
  * the CPU's window at `$6000 - $7FFF` in the same CPU address space (the FDS RAM adapter: `$6000 - $DFFF`), holding the buffer's first 8 KiB (32 KiB for the FDS), or all of it when it is smaller. A descriptor is registered once at load and cannot follow a bank switch, so a board that banks its RAM shows bank 0 here;
  * when the buffer is larger than that window, the whole buffer in a named `"SRAM"` address space, byte `n` at address `n`, so a cheat search or an achievement can reach every bank.

  Both views are cut into power-of-two pieces that start on a multiple of their own length, as libretro.h requires of a descriptor with `select == 0`. Until v2.9.0 there was one descriptor `{start: $6000, select: 0, len: <buffer size>}`: a 64 KiB buffer claimed `$6000 - $15FFF`, past the 16-bit bus; a 32 KiB one claimed PRG-ROM at `$8000 - $DFFF`; and a 73,728-byte one was not a power of two at all. The pieces are flagged `RETRO_MEMDESC_SAVE_RAM` **only when the header declares a battery** (`Nes::has_battery`); volatile work RAM carries no flag. This line used to say "battery-backed carts only" while every non-empty buffer was flagged.
* **Video RAM (VRAM):** Maps address range `$2000 - $2FFF`, flagged as `RETRO_MEMDESC_VIDEO_RAM` — but in a **named `"PPU"` address space**, not the blank/default one, since nametable RAM (CIRAM) lives on the PPU's own internal bus. On the CPU's real bus, `$2000-$2007` are the PPU MMIO registers (PPUCTRL/PPUMASK/etc.), not video RAM; registering the VRAM pointer under the default CPU space at that range would misrepresent it. This mirrors the convention `libretro.h` documents for other genuinely separate buses (e.g. the SNES SPC700 audio coprocessor's `"S"` address space).

The legacy `get_memory_data`/`get_memory_size` (`RETRO_MEMORY_*`) pointer path is kept alongside this, unchanged — RetroArch's own `.srm` persistence goes through it regardless, so the descriptor registration is additive, not a replacement. Both paths expose the MAIN console's memory in Vs. `DualSystem` mode (see `RustyNesLibretro::active_nes_mut`/`active_nes`).

## SRAM and Virtual File System (VFS) Offloading

As `rustynes-core` is `no_std`, it possesses no ability to interact with the host OS filesystem (`std::fs`), making native `.srm` (battery save) file writing impossible.
**Solution:** The FFI wrapper exposes the active cartridge's SRAM pointer via `retro_get_memory_data(RETRO_MEMORY_SAVE_RAM)`, **when the cartridge header declares a battery** (`Nes::has_battery`, the same gate the desktop frontend's `.sav` uses). Otherwise `RETRO_MEMORY_SAVE_RAM` reports size 0 and a null pointer (v2.9.0, libretro re-audit NL-02). Several boards expose RAM through `Nes::sram` whatever the header says (NROM always has 8 KiB; MMC1 and MMC3 allocate by default), so before v2.9.0, 581 images of the local test corpus without a battery handed RetroArch a `.srm`, and their work RAM came back on the next boot where the console would have powered on without it. An FDS image has no battery either; its in-game saves live on the disk image.

**Self-flashable boards (v2.9.6).** GTROM (111) and a flashable UNROM 512 (30)
save by rewriting their PRG flash and have no RAM at `$6000`. For them
`RETRO_MEMORY_SAVE_RAM` is `Nes::save_data`, the flash image (512 KiB on a full
board), while the memory map keeps describing `Nes::sram`, which is empty. That
keeps a 512 KiB ROM out of the `$6000` view RetroAchievements reads. The core
raises the battery flag for these boards, since GTROM headers do not set it.
RetroArch keys `.srm` files by content name, not by the ROM's hash as the
desktop does, so a renamed or updated ROM can load an older flash image, and
that image includes program code as well as the save.

* RetroArch automatically manages the lifecycle. Upon game load, the frontend injects data from the host's `.srm` file directly into this pointer.
* The frontend reads the pointer and writes the data back to the `.srm` when it saves it. `libretro.h` does not define when that is; RetroArch's timing (on unload, on exit, and on its own autosave interval if one is set) is inferred, not traced in its source, so do not rely on a write at any particular hook.
* A `.srm` a pre-v2.9.0 core wrote for a cartridge without a battery is no longer used: with a size of 0 there is nothing to load it into and nothing to write, so the file stays on disk untouched (inferred from the `libretro.h` contract, not traced in RetroArch's source). A game whose header wrongly omits the battery bit loses its save the same way on the desktop; the fix is the header.

This architectural inversion ensures compatibility with RetroArch Cloud Sync, mobile sandboxes (iOS/Android), and cross-platform save transfers without touching native filesystem APIs.

## Deterministic Serialization for GGPO & TAS

Rollback netplay (GGPO) masks network latency by simulating speculative remote player inputs. When actual inputs arrive later, RetroArch performs a "rollback" by instantly restoring a past emulator state (`retro_unserialize`), applying the true inputs, and fast-forwarding (`retro_run`) to catch up silently.
To support this, `rustynes-libretro` relies on the deterministic serialization engine found in `rustynes_core::save_state`.

1. **`retro_serialize_size` Permanency:** The FFI wrapper must return a static, unchanging byte-size integer post-ROM-load. Dynamic save-state resizing will immediately fault RetroArch, as the frontend pre-allocates contiguous memory pools for rollback frames based on this initial size query.
2. **Implementation:**
   * `on_serialize(buffer: &mut [u8])`: `Nes::snapshot_core_into` writes the state into the core's reusable `serialize_buffer`, which is copied to the front of the frontend's buffer; the rest is zero-filled. The reported size is the state at load plus room for the largest expansion device on both ports (`SAVE_STATE_DEVICE_HEADROOM`, v2.8.0), because a Zapper plugged in later grows the state and RetroArch never asks again. A buffer smaller than that reported size is refused (`false`), as libretro.h asks, even when the state itself would fit; until v2.9.0 it succeeded whenever the state fitted (libretro re-audit NL-10).
   * `on_unserialize(buffer: &[u8])`: `Nes::restore_quiet` reads the sections and stops at the all-zero tail, which the save-state reader treats as padding. A tag position holding a zero byte is rejected unless everything after it is padding, so a crafted state cannot make the scan quadratic.
3. **Fast-Forward Optimization (`get_fastforwarding`, implemented):** `RustyNesLibretro::is_fastforwarding` queries this each frame in `run_single`/`run_dual` and skips the `push_audio` call (the `f32`→`i16` interleave plus the `batch_audio_samples` FFI push) while fast-forwarding. This is a modest, honest win, not a large one: `rustynes-core` has no mixer-bypass API, so the dominant cost — APU synthesis inside `run_frame()` — is not skipped; only the presentation-side audio conversion/push is.

## Vs. `DualSystem` Two-Screen Presentation (v2.1.10 "Web Parity")

Four Vs. arcade titles (Balloon Fight, Wrecking Crew, Tennis, Baseball) ship on
**Vs. `DualSystem`** boards: two cross-wired NES consoles in one cabinet, each with
its own screen. The core already models them (`rustynes_core::Emu::Dual` /
`VsDualSystem`); the libretro wrapper wires the present path so RetroArch shows
both screens, matching the desktop frontend.

* **Detection:** `on_load_game` calls `Emu::from_rom`, which OR's the NES 2.0
  header Vs.-hardware type with the `vs_db` lookup (matched on the header-excluded
  ROM identity first, then the whole-image hash, since v2.9.8) — the identical
  detection the desktop frontend uses. The core then holds either `nes: Option<Nes>` **or**
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
  `VsDualSystem::snapshot_into`/`restore` (a self-describing blob of both consoles, without
  the desktop's slot thumbnails since v2.9.1, and with
  the same static-size permanency the single path guarantees); the RA / cheat
  memory maps expose the **MAIN** console.

The present path is a parallel branch in the FFI wrapper, mirroring the
desktop frontend's `emu.dual` branch. Serialization is not only the wrapper's:
since v2.9.1 it calls `VsDualSystem::snapshot_into`, which lives in the
`no_std` core with its pooled scratch buffer (`rustynes-core`,
`vs_dualsystem.rs`), because writing both consoles into one reused buffer is a
core operation, not an FFI one.

## Vs. System palette, DIP switches, coins and service (v2.9.0, libretro re-audit NL-08)

Every Vs. System cartridge, single or `DualSystem`:

* **Palette and DIP switches.** After loading, the core looks the image up in `rustynes_core::vs_db` (by its header-independent identity first, then its whole-file SHA-256; v2.9.8) and applies the entry's PPU type (the colour table) and factory DIP-switch setting, to both consoles of a cabinet — what the desktop's `apply_vs_db` does. An iNES 1.0 Vs. dump names no PPU, so the parser defaults it to the 2C03; until v2.9.0 the libretro core used the database only to recognise a cabinet, and every listed dump rendered in the 2C03's colours (7 of 7 local dumps measured by the re-audit). The desktop lets a config file override the DIP switches; the libretro core has no such option (see below).
* **Coins and service on the RetroPad**, named in the input descriptors only while a Vs. cartridge is loaded (the standard table is sent back at unload):

  | RetroPad | Single cartridge | `DualSystem` cabinet |
  | --- | --- | --- |
  | Port 1 L | coin, acceptor 1 | main console, acceptor 1 |
  | Port 2 L | coin, acceptor 2 | main console, acceptor 2 |
  | Port 1 R | service (held) | main console's service |
  | Port 3 L | — | sub console, acceptor 1 |
  | Port 4 L | — | sub console, acceptor 2 |
  | Port 3 R | — | sub console's service |

  A coin is a pulse: pressing L latches it for three frames (the desktop's `VS_COIN_HOLD_FRAMES`, 50 ms; the core documents the real switch as 40-70 ms) however long L is held. L and R are free on a NES pad, and each player's coin is on their own controller. Before v2.9.0 no libretro input reached the coin acceptors or the service button at all. Since v2.9.9 the three frames are EMULATED frames, timed against the console's own frame counter (which a save state carries), so run-ahead, preemptive frames, rewind and netplay rollback no longer shorten the pulse or lose it from a replayed timeline; before, it was counted in `retro_run` calls and lasted one frame (17 ms) at run-ahead 2 (re-audit NL-13). A restore to the frame a coin was pressed on, or to any frame before it, drops the coin, so the replay inserts one only if it presses L again (#583 review: a restore to the press's own frame used to keep it).
* **Not done: user-set DIP switches.** The database default is applied; a core option to change the switches is not implemented. Eight switches do not fit the one-list-per-option `SET_VARIABLES` form without either eight options or a 256-value list, and the core-options v2 form this would want is not used by this core yet.

## Famicom Disk System (FDS) Loading & Disk Control (implemented)

Standard NES ROMs (`.nes`) bundle all data in a single file. The Famicom Disk System requires two distinct components: the `.fds` disk image and the `disksys.rom` BIOS.

**Load path.** `on_load_game` (`crates/rustynes-libretro/src/lib.rs`) inspects the extension libretro's `GET_GAME_INFO_EXT` reports (`ext_info.ext`, valid even in in-memory/`need_fullpath = false` mode). For `.fds` content, it looks up `disksys.rom` via `RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY` (`GenericContext::get_system_directory`), reads it with `std::fs::read` (this crate links full `std`, unlike the `no_std` `rustynes-core`), and constructs via `rustynes_core::Nes::from_disk(disk_bytes, bios_bytes)`. Missing BIOS surfaces as a clear `on_load_game` error naming the expected path. **This is a simpler alternative to `RETRO_ENVIRONMENT_SET_SUBSYSTEM_INFO`/`retro_load_game_special`** (an earlier design considered but not built) — routing on the single game-load path avoids the added subsystem-registration/multi-buffer-negotiation surface for no loss of functionality, since RetroArch always resolves `disksys.rom` from the system directory the same way regardless.

**The standard load path (v2.8.0).** A frontend that refuses `GET_GAME_INFO_EXT` now loads through the standard `retro_game_info` that `retro_load_game` receives: its `data` and `size` when the frontend loaded the file, or the file at `path` when it did not. FDS routing follows a `.fds` extension **or** the image's own signature (`"FDS\x1A"` or a raw side's `\x01*NINTENDO-HVC*`), whether or not a path is present: a frontend may pass an extracted temporary file whose name has no extension (review on #556). An iNES image opens with `"NES\x1A"` and can match neither. This path could not exist before: the crates.io `rust-libretro-sys` 0.3.2 binding reduced `retro_game_info` to an opaque one-byte struct, so the core received nothing from it (the reason `GET_GAME_INFO_EXT` was required, recorded in `WALKTHROUGH.md`). The workspace now uses a patched, vendored copy, `vendor/rust-libretro-sys` (see its `VENDORED.md`).

**In-game saves (v2.9.0, libretro re-audit NL-03).** An FDS game saves by writing to the disk, not to battery RAM, and until v2.9.0 nothing in the libretro core read the written disk back out: every in-game save was lost when the game closed. The core now keeps the written disk in the frontend's save directory (`RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY`) as `RustyNES/<SHA-256 of the original image>.fds.sav`, the headerless image `Nes::disk_image_bytes` produces — the same name and contents as the desktop frontend's `fds-saves/<hex>.fds.sav`, so a save can be copied between them.

* **When it is written:** a second (60 frames) after the game first writes to the disk, so a crash loses at most that second; when the game is unloaded; at `retro_deinit`; and before another game loads. Written through a temporary file and a rename, so a failed write leaves the previous save; the disk's dirty flag is cleared only after the rename succeeds.
* **When it is read:** at `retro_load_game`, before the console is built. A save that no longer parses is reported in the log and the original disk boots.
* **Why not `RETRO_MEMORY_SAVE_RAM`:** libretro.h points a core whose save data is "too complex for a single memory buffer" at the save directory, and the disk is: it is not a live buffer (`disk_image_bytes` builds it on demand from the drive's state), and RetroArch fills a `SAVE_RAM` buffer only after `retro_load_game` returns, which would force the console to be rebuilt after the memory maps had been handed out. Reading the save first avoids both. The RAM adapter's 32 KiB is not a save and is no longer exposed as one (NL-02).
* **Without a save directory** (a frontend that returns none) the core logs that the disk's saves will not be kept, and runs as before.

**Multi-side disk swap.** Once loaded, the disk-control trait overrides (`on_set_eject_state`, `on_get_eject_state`, `on_get_image_index`, `on_set_image_index`, `on_get_num_images`) are backed by `Nes::disk_side_count`/`inserted_disk_side`/`set_disk_side` — the same API the desktop frontend's F9 disk-swap keybind uses. `register_disk_control`, called from `on_set_environment`, surfaces swap/eject in RetroArch's Quick Menu → Disk Control:

* When the frontend reports disk-control interface version 1 or later (`GET_DISK_CONTROL_INTERFACE_VERSION`), the core registers the **extended** interface (`SET_DISK_CONTROL_EXT_INTERFACE`), whose `get_image_label` names each side "Side A", "Side B", and so on. Otherwise it registers the v0 interface, which has no labels. Until v2.9.0 only v0 was registered, so the labels this paragraph described never reached a frontend (libretro re-audit NL-07).
* The label callback is the core's own `disk_image_label`, not `rust-libretro`'s trampoline: that one copies the label with no terminating NUL and no null check on the buffer (libretro audit L-1.5). `disk_image_label` refuses a null or zero-length buffer and an index past the last side, truncates to fit, and always terminates.
* `set_initial_image` and `get_image_path` are NULL, which libretro.h allows. With both present the frontend remembers the last side and asks for it at the next load; an FDS game boots from side A, and a single `.fds` container has no per-side file path to check such a request against.
* `on_replace_image_index`/`on_add_image_index` stay at their default no-ops: the sides come from one container and cannot be replaced one at a time.

## Cheats: RetroArch-handled (implemented since the legacy memory API) and Native (Game Genie, implemented)

Per `docs/guides/cheat-codes.md` in `libretro/docs`, RetroArch has two independent cheat mechanisms: "RetroArch Handled" cheats, where RetroArch itself directly pokes the core's exposed memory (address/value/compare, via the Cheats UI or built-in memory search) through the *same* `get_memory_data`/`get_memory_size` pointer API used for RetroAchievements — this has worked since that API was first implemented, never touches `on_cheat_set` at all — and "Emulator Handled" (native) cheats, sent to the core via `on_cheat_set` for the core to decode in its own native format.

`on_cheat_set`/`on_cheat_reset` are backed by `Nes::add_genie_code`/`remove_genie_code`/`clear_genie_codes`, which are deliberately excluded from serialized state — cheats never affect save-state / netplay / TAS determinism. `RustyNesLibretro::genie_cheats` (an `index -> code` map) remembers which code was applied at each frontend-assigned cheat slot, since `on_cheat_set` only reports the code being toggled, not what was previously there. Only Game Genie code syntax is decoded through this path — a generic RetroArch "raw address:value" poke cheat never reaches `on_cheat_set` in the first place, so it's unaffected either way.

## Controllers: four ports, the Four Score, and the Zapper (v2.8.1)

* **Four ports described.** All eight buttons are named on ports 1-4, so
  RetroArch's remap menu labels player 2 and players 3-4. The descriptor list
  is a `'static` table ending in a **null** description: `rust-libretro`'s
  `input_descriptors!` macro ends with `""`, which RetroArch's
  `for (; desc->description; desc++)` reads past.
* **Four Score.** The `rustynes_four_score` core option plugs the adapter in;
  ports 3-4 then carry players 3-4. Off by default, as on the console. The
  options are declared (`SET_VARIABLES`) from `on_set_environment`, once per
  init cycle: `rust-libretro`'s own hook runs only on the first
  `retro_set_environment` the process sees, so until v2.9.0 a frontend that
  kept the library loaded across `retro_deinit` + `retro_init` was never
  told about the option again (libretro re-audit NL-05).
* **Zapper.** Selecting "NES Zapper" on port 1 or 2 attaches the light gun on
  the next frame; selecting "NES Controller" again unplugs it. (Until v2.8.1
  the gun stayed on the bus and the controller on that port did nothing.)
* **One read per pad.** Input is polled once per frame (by `rust-libretro`,
  before `on_run`) and each pad is read as one bitmask.

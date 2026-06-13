# Vs. DualSystem — Feasibility & Design (2026-06-11)

**Status:** Design landed; full feature **deferred** (multi-session effort).
**v2.7.1 groundwork shipped:** a `vs_db` `dual_system` flag (Tennis / Mahjong /
Wrecking Crew / Balloon Fight) + a clear frontend note instead of a black screen.
This document preserves the architecture so the wrapper can be built later
without re-deriving the protocol.

## What Vs. DualSystem is

Vs. DualSystem arcade boards hold **two complete NES systems** — two CPUs, two
PPUs, two work RAMs — in one cabinet, sharing a small inter-CPU communication
latch and the coin/DIP inputs, each driving its own screen. The DualSystem set:
**Vs. Tennis, Vs. Mahjong, Vs. Wrecking Crew, Vs. Balloon Fight** (plus Baseball
/ Ice Climber Dual in the wider catalogue). RustyNES supports single-system Vs.
UniSystem (mapper 99, RGB 2C03/04/05 PPUs); the DualSystem titles black-screen
because the single-system core cannot satisfy the two-CPU handshake.

## Current architecture (what a second system needs)

- `Nes` (`crates/nes-core/src/nes.rs`) is a flat facade owning one `Cpu`, one
  `LockstepBus`, and a `rom_sha256`. `Nes` *is* the system; there is no "system"
  object above it.
- `run_frame` is a tight `while !bus.take_frame_complete() { cpu.step(&mut bus) }`.
  The PPU-dot lockstep scheduler is **internal** to `LockstepBus::tick_one_cpu_cycle`
  — there is no external scheduler object to coordinate two cores.
- The framebuffer is one 256x240 RGBA8 slice owned by the PPU inside the bus.
- Vs. panel state already lives inline on `LockstepBus`: `vs_dip`, `vs_coin`,
  `vs_service`, overlaid in `vs_overlay_4016` / `vs_overlay_4017` gated by
  `is_vs_system()`. Crucially, **bit 7 of `$4016` is hard-coded to 0** with the
  comment "we model a single CPU" — that is exactly the main/sub handshake bit a
  DualSystem ROM polls.
- The frontend (`app.rs` / `gfx.rs`) holds one `Nes`, calls `run_frame` once per
  frame, and uploads one 256x240 texture (hard-coded `W=256`/`H=240`).

## Hardware protocol (authoritative: Mesen2)

> The path `mame/.../vsnes.cpp` is **not staged** in `ref-proj/` (only the older
> `vsuni.cpp`, which has no DualSystem code; fceux skips DualSystem carts). The
> authoritative DualSystem source present in the tree is **Mesen2**
> `Core/NES/Mappers/VsSystem/VsControlManager.cpp` + `Core/NES/NesConsole.cpp`.

- **The handshake is a single `$4016` bit, not a dual-port RAM.**
  `VsControlManager::WriteRam`: on a `$4016` write, **bit 1** (`value & 0x02`) is
  the main/sub bit. `UpdateMainSubBit`: when it goes **low it asserts the *other*
  CPU's external `/IRQ`**; when high it clears it. That is the entire inter-CPU
  signal — each side pokes the other's IRQ line via a `$4016` bit.
- **Read side.** `$4016` bit 7 = `IsVsMainConsole() ? 0 : 0x80` (the bit RustyNES
  pins to 0): the sub-CPU reads 1, so the ROM knows which half it is. DIP: a
  32-bit value; the **sub console reads `dipSwitches >> 8`** — main and sub have
  independent DIP banks (DSW0 / DSW1). Coins: coins 1/2 + service drive main,
  coins 3/4 + service-2 drive sub, all latched on the main panel.
- **Shared WRAM.** `UpdateMemoryAccess` / `SwapMemoryAccess` swaps which mapper
  currently owns a shared bank, gated by the same `$4016` bit.
- **Stepping.** `NesConsole::RunFrame` steps main; after each main step it calls
  `RunVsSubConsole`, which runs the sub CPU until it is within a **5-CPU-cycle
  gap** of the main (`cycleGap > 5 || mainFrame > subFrame`). A *soft* lockstep,
  not cycle-exact interleave — this 5-cycle tolerance is the determinism-critical
  knob.
- **Construction.** On load, if `VsType == VsDualSystem`, main constructs a second
  console, back-links it, and loads the same ROM (both PRG halves are in the file).
  Two PPUs → two framebuffers, rendered side by side. Wrecking Crew needs the
  reset-time main/sub bit seeded.

## Detection

The DualSystem carts ship as **iNES-1.0**, so the NES 2.0 byte-13 **high nibble**
(Vs. hardware type 5/6 = DualSystem) is usually absent — the **SHA-256-keyed
`vs_db`** is the primary detection path (shipped in v2.7.1 as the `dual_system`
flag). A future full implementation should *also* parse byte-13 high nibble for
NES 2.0 dumps that set it (the header parser deliberately reads only the low
nibble today; a test asserts the high nibble does not change the PPU type).

## Recommended integration (when built)

A `VsDualSystem` wrapper in **`nes-core`** (which already depends on
cpu/ppu/apu/mappers — no new cross-chip dep):

```rust
pub struct VsDualSystem { main: Nes, sub: Nes, latch: VsCommsLatch }
```

- The **wrapper owns the latch** and the cross-IRQ wiring; the two buses only
  *report* a `$4016`-bit-1 edge and *accept* an external-IRQ assert (no bus holds
  a reference to the other — no aliasing / `Rc<RefCell>`).
- `LockstepBus` gains an `Option` comms hook (the `$4016` write path records the
  bit-1 edge; bit 7 of the `$4016` read overlay becomes `is_sub ? 0x80 : 0`).
  Because the hook is absent by default, the **single-system overlay stays
  byte-identical**.
- `run_frame` mirrors `RunFrame` + `RunVsSubConsole`: step main one instruction,
  then drain sub until within the 5-cycle gap; expose `framebuffer_main()` /
  `framebuffer_sub()`.
- Shared WRAM = a mapper-99 `swap_shared_window` driven by the latch bit.
- Save-state: a `VsDual` container nesting two existing `Nes` snapshots + the
  latch (single-system snapshots untouched).
- Frontend: `enum Emu { Single(Nes), Dual(VsDualSystem) }`; a second wgpu texture
  (or one 512x240); P1/P2 → main, P3/P4 → sub; coins 1/2 → main, 3/4 → sub.

**Out of scope for the dual path:** netplay (the rollback model assumes one state
blob) and RetroAchievements (one memory map). State this explicitly.

## Effort & risk verdict

**~4-7 focused days (multi-session). Defer; do not attempt in a single release.**
The non-negotiable constraint (single-system byte-identical) is satisfiable
because every change is additive + `Option`/flag-gated and the chip cores need
zero changes. Riskiest unknowns:

1. **The 5-cycle soft-lockstep** — getting a deterministic *and* accurate
   interleave that boots all titles, with **no DualSystem test-ROM oracle** in the
   suite (validation is "does it boot + play", weaker than the AccuracyCoin /
   byte-identical gate).
2. **Shared-WRAM `SwapMemoryAccess`** — the exact window + whether mapper-99 can
   express the swap cleanly is where a residual black-screen would most likely
   persist.
3. **No committable DualSystem ROMs** — all validation is manual/local.

## v2.7.1 groundwork (shipped)

- `crates/nes-core/src/vs_db.rs`: `VsDbEntry.dual_system` + `entry_dual()`; the 4
  DualSystem carts flagged; a unit test asserts exactly those 4.
- `crates/nes-frontend/src/app.rs`: `apply_vs_db` emits a clear DualSystem note
  (stderr native / `console.warn` wasm) when such a cart loads.
- This document.

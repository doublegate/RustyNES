# PPU — Ricoh 2C02

**References:** `ref-docs/research-report.md` §Technical deep-dive → PPU;
`ref-docs/nesdev-wiki-technical-report.md` §PPU; Nesdev
[PPU](https://www.nesdev.org/wiki/PPU),
[PPU power up state](https://www.nesdev.org/wiki/PPU_power_up_state),
[PPU registers](https://www.nesdev.org/wiki/PPU_registers),
[PPU rendering](https://www.nesdev.org/wiki/PPU_rendering), and
[PPU sprite evaluation](https://www.nesdev.org/wiki/PPU_sprite_evaluation).

## Purpose

Implement the 2C02 PPU as a state machine that advances exactly one dot per `tick()`. The PPU maintains the framebuffer, generates NMI at the start of vertical blank, manages OAM and sprite evaluation, and exposes the eight CPU-facing registers `$2000-$2007` plus `$4014` (OAMDMA, handled by the bus but originating an OAM-DMA request).

## Interfaces

```rust
pub trait PpuBus {
    fn ppu_read(&mut self, addr: u16) -> u8;        // $0000-$3FFF, mapper-mediated CHR + nametable
    fn ppu_write(&mut self, addr: u16, value: u8);
    fn notify_a12(&mut self, level: bool);           // for MMC3/MMC5 IRQ counters
}

pub struct Ppu {
    pub dot: u16,           // 0..=340
    pub scanline: i16,      // -1 (pre-render) ..= 260 (or 311 PAL)
    pub frame: u64,
    /* opaque internal state */
}

impl Ppu {
    pub fn new(region: Region) -> Self;
    pub fn reset(&mut self);
    pub fn tick<B: PpuBus>(&mut self, bus: &mut B);

    pub fn cpu_read_register(&mut self, reg: u8) -> u8;       // reg 0..=7
    pub fn cpu_write_register(&mut self, reg: u8, value: u8);

    pub fn nmi_pending(&self) -> bool;
    pub fn frame_complete(&self) -> bool;
    pub fn framebuffer(&self) -> &[u8; 256 * 240 * 4];        // RGBA8 sRGB
    pub fn index_framebuffer(&self) -> &[u16; 256 * 240];     // (emph<<6)|colour, 0..=511
    pub fn ntsc_phase(&self) -> u8;                           // videoPhase: 0..=2 NTSC, 0..=1 PAL/Dendy
}
```

The PPU owns its 2 KB internal VRAM but routes all `$0000-$3FFF` accesses through the `PpuBus` so the mapper can override CHR (banking) and nametable mirroring. Palette RAM (`$3F00-$3FFF`) is internal to the PPU.

## State

- **Internal VRAM** — 2 KB (the on-board nametable RAM the mapper mirrors).
- **OAM** — 256 B (64 sprites × 4 bytes).
- **Secondary OAM** — 32 B (8 sprites × 4 bytes; cleared each scanline).
- **Palette RAM** — 32 B with mirrors (`$3F10`/`$14`/`$18`/`$1C` mirror `$3F00`/`$04`/`$08`/`$0C`).
- **Internal registers** — `v` (15-bit current address), `t` (15-bit temp), `x` (3-bit fine X), `w` (1-bit write toggle).
- **Shift registers** — 16-bit BG pattern low + high, 8-bit BG attribute low + high, 8-bit per-sprite pattern low + high (×8), 8-bit per-sprite X-counter (×8), per-sprite attribute byte (×8).
- **Status latches** — VBL flag, sprite-0 hit, sprite overflow.
- **Open-bus latch** — 8-bit dynamic latch + per-bit decay timer (for PPUSTATUS bits 4-0 read).
- **NMI line + edge tracker** — observed by the CPU.
- **OAMADDR**, **read buffer** for PPUDATA.

## Behavior

### Frame structure

Per `ref-docs/research-report.md` §Frame and scanline structure:

| Scanline | Dots | Purpose |
|----------|------|---------|
| 0..=239 | 0..=340 | Visible (1..=256 emit pixels) |
| 240 | 0..=340 | Post-render idle |
| 241..=260 (NTSC) / 241..=310 (PAL) | 0..=340 | Vertical blank (VBL flag set at scanline 241 dot 1; /NMI asserted at dot 3 — see below) |
| 261 (NTSC) / 311 (PAL) | 0..=340 | Pre-render |

**VBL flag vs /NMI assertion timing.** The VBL bit (`PPUSTATUS` bit 7) is set at scanline 241 dot 1 per nesdev. Real hardware pulls /NMI low one PPU clock later (dot 2). In our scheduler, where `LockstepBus::cpu_read`/`cpu_write` performs the bus access *before* the cycle's 3 PPU dots tick (vs. real hardware's mid-cycle bus access), we delay /NMI assertion by **2 additional PPU dots** (assert at scanline 241 dot 3) so that blargg's `ppu_vbl_nmi/05-nmi_timing` and `08-nmi_off_timing` sample the rising edge on the same CPU cycle a real 6502 would. The bus's edge detector samples the /NMI line *between every PPU dot* of `tick_one_cpu_cycle` so a glitched edge that goes low→high then back to low within a single CPU cycle (e.g. PPUCTRL.7 set during pre-render dot 0, then VBL cleared at dot 1 within the same CPU cycle) is still latched — required for `ppu_vbl_nmi/07-nmi_on_timing`.

Odd-frame skip on NTSC: the pre-render scanline of odd frames omits the final dot, jumping `(339, 261) → (0, 0)`. The decision is taken on the transition out of dot 339 and gated on the rendering-enabled flag. The flag read by the dot-skip detector lags `mask` by **two PPU clocks** via a two-stage shift pipeline (`mask_for_skip_check` / `mask_skip_pipe1`), so a `$2001` write whose CPU cycle straddles dot 339 doesn't move the threshold by a full CPU cycle. This compensates for `LockstepBus`'s atomic write-before-tick ordering (CPU PPUMASK write lands ~3 PPU dots earlier than real hardware's φ2 latch); two dots of delay align the visible threshold with blargg's `ppu_vbl_nmi/10-even_odd_timing` expectations. The pipeline is fed only from `mask` and does not affect background, sprite, or palette-greyscale paths, all of which keep using `mask` directly.

### Extra-scanlines overclock (v1.7.0 F3)

The PPU can insert **N extra blank scanlines** into the vblank period each frame
(Mesen2 `UpdateTimings`), at the existing dot resolution. Set via
`Nes::set_extra_scanlines(n)` (forwarded to `Ppu::set_extra_scanlines`); read
back via `Nes::extra_scanlines()`. **Off by default (`0`)**, and a frontend
config knob — it is **not** part of the save-state (the frontend re-applies it on
restore, like the region / palette).

How it works: when `extra_scanlines != 0`, `Ppu::advance_dot` holds the PPU on
the idle vblank line immediately *before* the pre-render line (NTSC line 260) and
re-runs it `n` times before advancing to pre-render. That line is not visible
(scanline > 239), not the VBL-set line (241), and not pre-render — so the extra
lines emit no pixels, set/clear no `PPUSTATUS` flag, and fire no VBL / NMI / A12
event. They are pure additional CPU run-time (the scheduler still clocks the CPU
every third dot), giving timing-sensitive games more compute headroom without
altering the visible image.

**Byte-identical at zero.** The entire insertion branch is guarded by
`extra_scanlines != 0`, so at the default `0` not a single code path differs and
the frame is byte-for-byte identical to stock NES timing — `AccuracyCoin`, the
commercial oracle, and nestest (which never set it) are unaffected. This is
proved by `crates/rustynes-test-harness/tests/extra_scanlines.rs`
(`extra_scanlines_zero_is_byte_identical_to_stock`, plus an image-invariance
proof on the first frame and a CPU-cycle-growth check).

This is **distinct from the CPU-multiplier overclock**, which needs the
fractional-master-clock timebase rewrite and is a v2.0 item (ADR 0002); only the
dot-resolution scanline *insertion* is in scope here.

### Power-up and reset state

The PPU has a reset mask distinct from the CPU reset sequence:

| State | Power | Reset |
|---|---|---|
| PPUCTRL / PPUMASK | `$00` | `$00` |
| PPUSTATUS | Partly unspecified | VBL unchanged; other bits partly unspecified |
| OAMADDR | `$00` on documented cold power | Unchanged, but rendering normally disturbs it |
| PPUSCROLL / latch | `$0000`, write toggle clear | `$0000`, write toggle clear |
| PPUADDR | `$0000` | Unchanged |
| PPUDATA read buffer | `$00` | `$00` |
| OAM, palette, nametable RAM, CHR RAM | Unspecified | Unchanged or unspecified depending on storage |

Writes to `$2000`, `$2001`, `$2005`, and `$2006` are ignored for roughly
29,658 NTSC CPU clocks after reset; PAL is roughly 33,132 CPU clocks. The
write-pair latch also does not toggle during this mask. `$2002`, `$2003`,
`$2004`, `$2007`, and `$4014` work immediately.

Cold power shortly after a previous power-off can look reset-like rather than
fresh. Tests and user-facing deterministic mode should therefore avoid assuming
stable OAM, palette, nametable, CHR RAM, or PPUSTATUS low-bit state. The
unspecified power-up palette RAM and CPU work RAM are modeled by the opt-in,
default-off knobs described below (see [Power-up palette RAM](#power-up-palette-ram-optional-opt-in-default-off-v217-p5)
and [Power-on work-RAM model](#power-on-work-ram-model-optional-opt-in-default-off-v217-p5));
the default keeps both deterministic (all-zero).

### Per-dot fetch sequencing (visible + pre-render scanlines)

- **Dot 0** — idle.
- **Dots 1..=256** — background tile fetch (8-dot windows of NT, AT, PT-low, PT-high, 2 dots each). Shift registers reload on dot 9, 17, 25, ..., 257. Pixels emit dots 1..=256.
- **Dots 257..=320** — sprite tile fetch for next scanline. OAMADDR is forced to 0. 8 sprites × 4 fetches (garbage NT, garbage NT, PT-low, PT-high). Sprite X positions and attributes load during the second garbage fetch. Horizontal `v` bits reload from `t` at dot 257. The pattern-table fetches happen for **all 8 slots regardless of how many real sprites are in range** — empty slots use the cleared secondary-OAM tile `$FF`, producing a dummy fetch that still drives A12 to the sprite pattern table. This is what produces the per-scanline A12 rising edge that MMC3's IRQ counter clocks on; without dummy fetches, scanlines with zero visible sprites would emit no rise. Sprite tile fetch also runs on the pre-render scanline (for scanline 0's sprites), so an NTSC frame yields exactly 240 visible + 1 pre-render = **241 A12 rises** with standard pattern-table layout (BG=$0000, sprites=$1000) — the count `mmc3_test_2/2-details` validates.
- **Dots 280..=304 (pre-render only)** — vertical `v` bits reload from `t` if rendering enabled.
- **Dots 321..=336** — first two BG tiles of next scanline.
- **Dots 337..=340** — two extra NT fetches (purpose debated; preserve them so MMC5's frame-end detection works).

### Fast dot path vs exact dot path (v2.1.8 A1, opt-in, default-OFF)

`Ppu::tick` is the emulator's single hottest function (~46% of frame self-time;
`docs/performance.md`). Behind a **default-OFF runtime knob**
(`Nes::set_fast_dotloop`) the per-dot FSM splits into two paths:

- **Exact path** (the shipped default, and the always-correct fallback): the
  fully-general per-dot body described throughout this document — every event
  and quirk checked on every dot.
- **Fast path** (`Ppu::tick_visible_render_fast`): a specialized straight-line
  handler for the *common clean* dot — a **visible scanline, dots `1..=256`,
  rendering stably enabled, and no sub-dot disturbance** (no `$2006` copy-V or
  PPUMASK write-delay pending, no PPUDATA state machine in flight, no
  armed/pending OAM-corruption, warm scanline-classification cache). It runs the
  **identical** helper sequence (OAM-corruption pointer bookkeeping →
  sprite-eval FSM → OAM data-bus → BG reload/ALE/fetch/`inc_hori_v` →
  `inc_vert_v` at dot 256 → `emit_pixel` → `shift_bg`) with the branches the
  guard proves un-taken elided, so it is byte-identical to the exact path.

The dispatch guard is conservative: **any** disturbance — a mid-scanline
`$2000/$2001/$2005/$2006/$2007` write, a `$2007` read during render (the PPUDATA
state machine), a rendering-enable toggle, an armed OAM-corruption edge, the
sprite-tile-fetch / OAMADDR-reset window (dots 257..=320), the pre-render line,
scanline 241, dot 0 — fails the guard and takes the exact path. Correctness
dominates: when in doubt, the exact path runs. A whole-scanline *batch* is
**not** possible here — the lockstep every-cycle-bus-access scheduler advances
the PPU ≤3 dots per CPU cycle (`docs/scheduler.md`, `docs/performance.md` §A1),
so this is a per-dot specialization only. Byte-identity across the full corpus
(framebuffer + index buffer + audio + cycles + snapshot) is pinned by
`crates/rustynes-test-harness/tests/fast_dotloop_diff.rs`.

The "warm scanline-classification cache" in that guard
(`cached_visible` / `cached_pre_render` / `cached_render_line`, keyed by
`flags_cached_scanline`) is a pure function of `scanline` + `region`, so it is
**recomputed rather than serialized**: `Ppu::restore` resets the key to the
`Ppu::new` sentinel at every schema version, forcing the next tick to refill it
from the restored scanline. Carrying a warm key across a restore would let a
cache filled under one timeline satisfy the guard against a value computed under
another.

### Sprite evaluation (cycles 1..=256)

Per `ref-docs/research-report.md` §Sprite evaluation:

- **Cycles 1..=64** — clear secondary OAM to `$FF` (forced reads).
- **Cycles 65..=256** — alternate odd (read primary OAM) / even (write secondary OAM).
  - Read Y from `OAM[n][0]`. If in range for next scanline, copy bytes 1..=3.
  - Increment `n`. When `n` overflows to 0, evaluation completes.
  - When 8 sprites found, disable secondary OAM writes. **Then** the buggy overflow check: increments **both `n` and `m`** (without carry). This mis-reads tile/attr/X bytes as Y bytes, producing the documented hardware overflow misbehavior. **Reproduce exactly.**

#### Implementation state (T-23-002 / T-23-003 / B8 follow-up)

Sprite evaluation runs through `tick_sprite_eval_per_dot` in
`crates/rustynes-ppu/src/ppu.rs` — a per-PPU-dot FSM matching real-hardware
behavior across dots 0..=256 of every visible / pre-render scanline:

- **Dot 0**: reset FSM working state.
- **Dots 1..=64**: secondary-OAM clear (1 byte every 2 dots; reads of
  `$2004` during this window would return `$FF` on real hardware).
- **Dots 65..=256**: alternating odd/even read/write of primary OAM.
  Odd dots latch a byte; even dots commit the latch into secondary
  OAM (when copying is enabled). Eight in-range sprites fill secondary
  OAM and then transition into overflow-search mode, which walks the
  remaining 56 entries with the documented buggy `n+m` increment —
  the diagonal-read pattern that `sprite_overflow_tests/4-Obscure` and
  `sprite_overflow_tests/5-Emulator` exercise.
- **Dot 256**: commit `spr_count` and pre-clear unused slot
  rendering-side arrays.

The MMC3 A12-rising-edge timing is unchanged from the original
single-shot collapse: sprite-tile fetches still dispatch at dots 260,
268, ..., 316 via `fetch_sprite_tile`, producing the canonical "241 A12
rises per NTSC frame" count that `mmc3_test_2/2-details` and
`mmc3_test_2/3-A12_clocking` validate.

The FSM regression corpus lives in
`crates/rustynes-ppu/src/ppu.rs::tests::sprite_fsm_equivalence_*`. It runs
13 deterministic edge cases + 1000 randomized OAM / scanline /
sprite-size combinations through the FSM and asserts bit-identical
`secondary_oam`, `spr_count`, `spr_zero_in_line`, and
`SPRITE_OVERFLOW` flag at end of dot 256 against a straight-line
reference implementation (`reference_eval`). Originally introduced as
the parallel-implementation firewall gating the B8 swap from
single-shot to per-dot FSM; after B8c removed the single-shot impl
the corpus is the regression net pinning the FSM output.

**Save-state coverage (`PPU_SNAPSHOT_VERSION` v8).** The FSM's working
registers are part of the save-state, not derived: `sprite_eval_read_latch`,
`sprite_eval_n` / `_m` / `_found` / `_sec_idx`, and the
`_copying` / `_done` / `_overflow_search` / `_zero_found` / `_first_iter`
phase flags, alongside the parallel OAM-data-bus model (`oam_bus_*`) and the
dots-1..=64 clear-window write pointer `oam2_addr`. `secondary_oam` — the
buffer they fill — was serialized from v1, but the pointers and phase driving
it were not until v8, so a checkpoint taken mid-eval restored a full buffer
next to a power-on-default walker.

This is invisible to a straight `run_frame` loop and shows up only where a
save-state round trip lands mid-frame: the frontend's **run-ahead**
(`[input] run_ahead`, default 1) does exactly that every visible frame, as do
netplay rollback and TAS seeking. Before the v8 tail the AccuracyCoin battery
measured 141/141 headless but 138/141 through the desktop frontend, failing
`Sprite Evaluation :: Arbitrary Sprite zero` (error 2),
`Sprite Evaluation :: Misaligned OAM behavior` (error 1), and
`PPU Behavior :: Rendering Flag Behavior` (error 2). The regression net is
`crates/rustynes-test-harness/tests/accuracycoin_runahead.rs`, which reruns the
whole battery through the run-ahead cycle at depths 1 and 2 and asserts no test
is lost. Mesen2 serializes the equivalent set (`NesPpu<T>::Serialize`:
`_spriteIndex`, `_sprite0Added`, `_sprite0Visible`, `_oamCopybuffer`,
`_secondaryOamAddr`, `_spriteInRange`, `_oamCopyDone`, `_overflowBugCounter`).

Future work (post-flip):

- Reading `$2004` during cycles 1-64 should return `$FF` (idle clear
  phase). The per-dot FSM has the state for this but the `$2004` read
  path does not yet observe the FSM phase. Add when a test ROM
  distinguishes the behavior.
- OAMADDR-during-rendering `OAMADDR=0` forcing across dots 257-320 is
  modeled (the sprite-tile-load wash). The *write*-triggered OAMADDR
  (`$2003`) corruption glitch is now available as an opt-in
  revision-gated model — see [OAMADDR (`$2003`) write corruption](#oamaddr-2003-write-corruption-optional-opt-in-default-off)
  below.

### Sprite-0 hit

Set when a non-transparent pixel of sprite 0 overlaps a non-transparent pixel of background. Constraints:

- Cannot set on dot 255.
- Cannot set if either left-column-show flag is off and X is 0..=7.
- Cannot set on the pre-render scanline.
- Cleared at scanline 261 (or 311 PAL) dot 1.

Determined during sprite *rendering*, not evaluation.

### OAM decay (optional, opt-in, default-OFF) (Unreleased, F2.3)

Real 2C02 OAM is dynamic RAM. Sprite evaluation reads every sprite's Y byte each
rendered scanline, which implicitly refreshes the cell charge; but with rendering
disabled long enough the un-refreshed rows lose charge and decay to a fixed garbage
pattern. RustyNES models this exactly like Mesen2 (`NesPpu::ReadSpriteRam` /
`WriteSpriteRam`, `OamDecayCycleCount = 3000`), gated behind a default-OFF toggle.

- **Granularity** — one CPU-cycle timestamp per **8-byte row** (32 rows over the
  256-byte OAM), `oam_decay_cycles[addr >> 3]`. The CPU cycle is derived from the
  PPU's monotonic dot counter (`dot_counter / 3`) — deterministic, no wall-clock.
- **On every OAM read** (the `$2004` read **and** the primary-OAM reads done during
  sprite evaluation): if `cpu_cycle − oam_decay_cycles[addr>>3] ≤ 3000`, refresh the
  timestamp; otherwise the row has decayed — rewrite all 8 of its bytes to
  `((sprAddr & 3) == 2) ? (sprAddr & 0xE3) : sprAddr` (attribute bytes keep only
  their implemented bits; the rest read back their own low address). The
  refresh-on-sprite-eval is what keeps rows alive during normal rendering.
- **On every OAM write** (`$2004` write and OAM DMA): write the byte, then refresh
  the row's timestamp.
- **Region** — NTSC/Dendy only. On PAL the far more frequent refresh cadence masks
  decay entirely, so the model never acts there (matching Mesen2).
- **Default-OFF byte-identity** — with the toggle off (the default) no OAM access
  consults the decay state, so the framebuffer/audio/replay output and the
  visual/`external_real_games`/AccuracyCoin suites are byte-identical to a build
  without the field. Enable via `Nes::set_oam_decay(true)` /
  `[emulation] oam_decay` / **Settings → Emulation → OAM decay (accuracy)**.
- **Save-state** — the per-row timestamps are serialized in the `PPU_SNAPSHOT_VERSION`
  v7 tail as a *relative age* (`now − timestamp`), reconstructed against the live
  counter on load, so a run-ahead / netplay `snapshot`→`restore` stays byte-identical
  even though the free-running `dot_counter` itself is not serialized. The enable
  flag is a frontend/config knob re-applied on load (not serialized), like `region`.

### PPU die revision (optional, opt-in, default-OFF) (v2.1.7, P5)

Real RP2C02 dies shipped across several letter revisions. RustyNES exposes a
selectable `PpuRevision` (`rustynes_core::PpuRevision`) that gates the one
revision-dependent behavior it models, the OAMADDR `$2003` write corruption:

- `Rp2c02H` (**default**) — the later "H"-class die RustyNES has always modeled.
  No `$2003` write corruption. **Byte-identical** to a build without the field —
  AccuracyCoin, the commercial oracle, and the visual/audio suites are
  unaffected at the default.
- `Rp2c02G` — the earlier die ("rev E+" in the nesdev notes). Additionally
  models the `$2003` write corruption (below). Opt-in.

The selection is **config**, re-applied on load like `region` / the active
palette — it is **not** serialized in the snapshot. (The corruption *state* it
can arm — `oam_corruption_pending` / `oam_corruption_index` — is already in the
`PPU_SNAPSHOT_VERSION` v6 tail, so an armed corruption still round-trips.) Set
via `Nes::set_ppu_revision` / `[emulation] ppu_oamaddr_corruption`.

### OAMADDR (`$2003`) write corruption (optional, opt-in, default-OFF)

On the earlier `Rp2c02G` revision, writing OAMADDR (`$2003`) while rendering is
enabled on a visible or pre-render scanline corrupts one 8-byte OAM "row". A few
titles — notably *Huge Insect* — trip it. RustyNES models it by reusing the same
`CorruptOAM` row-copy the rendering-disable corruption uses: on such a write it
arms `oam_corruption_pending` with `oam_corruption_index = (value >> 3) & 0x1F`
(the row the write's high bits select), and the existing per-dot commit path
applies it on the next rendered dot — copying OAM row 0 over the targeted row
(plus the matching secondary-OAM byte). The `!oam_corruption_pending` guard
defers to an already-armed corruption so the two sources never race.

- **Render-gated** — only a `$2003` write with rendering enabled on a
  visible / pre-render scanline arms it; writes outside rendering (or in vblank)
  never corrupt, on any revision.
- **Default-OFF byte-identity** — the default `Rp2c02H` revision never arms it,
  so the corruption path is inert and the default build is byte-identical.
- **Honesty** — the precise 2C02 letter-revision taxonomy of this glitch, and
  its exact per-title byte output, are not independently oracle-verified in this
  cut; RustyNES offers the model as an opt-in approximation keyed to a single
  "earlier revision" selection rather than claiming exact silicon fidelity. See
  `docs/accuracy-ledger.md`.

### Power-up palette RAM (optional, opt-in, default-OFF) (v2.1.7, P5)

The 2C02's palette RAM is not cleared at power-on; different consoles come up
with different garbage, and a few titles sample it before writing. RustyNES
exposes a `PaletteInit` (`rustynes_core::PaletteInit`):

- `Zeroed` (**default**) — all 32 palette-RAM bytes power up to `$00`, the
  established deterministic state. **Byte-identical**.
- `Blargg` — the canonical "blargg" 32-byte power-up dump (mirrors TriCNES's
  `BlarggPalette`), each cell 6-bit masked like a `$2007` write. Opt-in.

It writes only `palette_ram`, which the snapshot already serializes, so **no
snapshot-format change is required**. Best applied at power-on (palette RAM is
preserved across a warm reset, like real hardware); the selection is stored so a
power-cycle re-applies it. Set via `Nes::set_power_up_palette` /
`[emulation] blargg_power_up_palette`.

### Power-on work-RAM model (optional, opt-in, default-OFF) (v2.1.7, P5)

Real hardware powers up with unreliable 2 KiB CPU work RAM (nesdev "CPU power up
state"); a few titles read it before writing (*Final Fantasy*'s RNG seed, *River
City Ransom*, *Cybernoid*). `rustynes_core::PowerOnConfig` selects the fill via
`PowerOnRam`:

- `Zeroed` (**default**) — all-zero work RAM + open bus (**byte-identical**;
  what CI, the oracle, and save-state tests use).
- `Seeded(u64)` — deterministic `xorshift64` randomization keyed on the seed
  (the existing developer mode; same seed ⇒ identical RAM).
- `Filled(u8)` — every work-RAM byte set to a uniform documented pattern.

All fills are **deterministic** (no wall-clock / OS RNG), so the `same config +
ROM + input ⇒ bit-identical` contract holds. The fill is stored on the bus so a
power-cycle re-applies it (`power_cycle == fresh boot`). Build via
`Nes::from_rom_with_power_on_config` / set via `Nes::set_power_on_ram`, or the
`[emulation] randomize_power_on_ram` + `power_on_ram_seed` config keys. (This
strictly generalizes `Nes::from_rom_with_power_on_seed`, which now routes through
`PowerOnRam::Seeded`.)

### Loopy `v / t / x / w`

Per `ref-docs/research-report.md` §Internal scroll registers:

- **PPUSCROLL write 1** → `t` bits 4-0 = X[7:3]; `x` = X[2:0]; clear `w`.
- **PPUSCROLL write 2** → `t` bits 14-12 = Y[2:0] (fine Y); `t` bits 9-5 = Y[7:3] (coarse Y); set `w`.
- **PPUADDR write 1** → `t` bits 13-8 = value & 0x3F; `t` bit 14 = 0; clear `w`.
- **PPUADDR write 2** → `t` bits 7-0 = value; copy `t` to `v`; set `w`.
- **PPUSTATUS read** → clear `w`.
- **During rendering** at dot 256 of every visible scanline, `v` Y increments (with the 29→0 wrap-and-flip-nametable-Y quirk). At dot 257, horizontal bits of `v` reload from `t`. At dots 280..=304 of pre-render, vertical bits reload.
- **Coarse X increment** at every 8th dot of fetch windows (dots 8, 16, ..., 256, 328, 336).

### Register quirks (must reproduce)

- **PPUSTATUS read** clears VBL flag *and* `w`.
- **PPUDATA read buffering**: returns previous buffered value, fills buffer with current `v`'s data; palette reads (`$3F00-$3FFF`) bypass buffer but still update it with the underlying nametable mirror.
- **PPUDATA (`$2007`) read during active rendering**: a `$2007` read while rendering is enabled does NOT do a clean buffered VRAM fetch. Instead it returns the value the rendering fetch cadence most recently drove on the VRAM data bus (the "render buffer"), and the `PPUDATA` state machine reloads `data_buffer` a few PPU dots *after* the read ends rather than immediately — modelled as a short PPU-dot countdown (`ppudata_sm_countdown`) with the `v`-increment glitch deferred to the same `TStep` dot (`ppudata_v_inc_pending`). This is the `AccuracyCoin` `$2007 Stress` per-dot-read bracket; verified complete (no v1.6.0 Workstream D change needed). The diag knobs `RUSTYNES_2007_DELAY` / `RUSTYNES_2007_VINC` are read-only investigation tools, not shipped behavior.
- **PPUDATA increment**: 1 or 32 per PPUCTRL bit 2.
- **OAMADDR write during rendering**: glitches OAMADDR's high 6 bits; OAM data is not modified during rendering writes.
- **OAMADDR ≥ 8 at rendering start**: row at `OAMADDR & 0xF8` is copied to OAM[0..=7]. (2C02G bug.)
- **`$2004` attribute-byte read mask**: every 4th OAM byte (at offset 2 within each 4-byte sprite group) is the attribute byte and has bits 2-4 unimplemented; reads return 0 in those bits regardless of what was stored.  Writes still store the full byte.  Required by `ppu_open_bus.nes`.
- **Open-bus**: PPUSTATUS bits 4-0 reflect last-written-or-read PPU bus value with per-bit-group decay (3-30 ms hardware; we use a coarser 600 ms approximation for emulation).  Three independent decay groups: bits 0-4, bit 5, bits 6-7.  Writes refresh all three groups; reads of $2002 refresh only bits 5-7 (the lower 5 bits' value AND timer carry over); palette $2007 reads refresh only bits 0-5 (the high 2 bits' value AND timer carry over).  Required by `cpu_dummy_writes_ppumem` and `ppu_open_bus` tests 7 and 9.
- **PPUCTRL NMI bit 0→1 while VBL set**: triggers NMI immediately.
- **Post-reset window**: writes to `$2000`/`$2001`/`$2005`/`$2006` ignored for ~29,658 NTSC CPU cycles after reset (33,132 PAL).

### Picture and border output

The stock picture region is 256x240 pixels. The PPU also generates a border
region around it on the analog signal. RustyNES exposes only the 256x240
picture framebuffer to the frontend today; any future overscan/border mode
should be modeled as an output option, not as additional framebuffer pixels that
change core rendering tests.

### Greyscale + emphasis

PPUMASK bit 0 (greyscale): output color ANDed with `$30`. Bits 7-5 (BGR emphasis): each modulates one color channel down. Both apply per-pixel during emission.

### Index framebuffer + NTSC phase (composite-filter outputs)

Alongside the RGBA framebuffer, the emit path writes a parallel **palette-index
framebuffer** (`index_framebuffer()`): one `u16` per pixel holding the same 9-bit
`(emphasis << 6) | colour_index` value (0..=511) used to look up the RGBA in the
512-entry `rgba_lut`. It is therefore an exact index-space mirror of the displayed
picture — the invariant `rgba_lut[index[p]] == framebuffer[p]` holds for every emitted
pixel (unit test `index_framebuffer_mirrors_rgba_output`). The true composite NES_NTSC
filter (T-110-A1) uploads this as an `R16Uint` texture and reconstructs the analog
signal in a shader.

`ntsc_phase()` exposes the per-frame **`videoPhase`** (0..=2 on NTSC; frame parity on
PAL/Dendy), snapshotted at each frame boundary from a free-running master-cycle
counter. This is the source of the NTSC dot-crawl; the filter derives the per-scanline
(`videoPhase*4 + y*341*8`) and per-pixel (`x*8`) phase from it.

Both are **output-only / cosmetic**: they carry no logical state and feed no emulation
path, so the determinism and AccuracyCoin contracts are unaffected and the `no_std` chip
stack is untouched. Unlike the RGBA `framebuffer` (which IS serialized in the PPU
snapshot), the index framebuffer + phase are NOT saved — they regenerate on the next
emitted frame, so a state loaded while paused shows correct NTSC from the first frame
after resume.

### Raw composite-signal model (`raw_signal`, v2.1.9 P4)

`rustynes_ppu::raw_signal` is a **new, parallel, presentation-only** output that keeps
the 2C02 composite waveform *un-decoded*. Where the generated palette (`palette_gen`,
F1.4) pre-integrates each of the 64 base colours to a single RGB triple (an ideal TV
over one pixel), `raw_signal` emits, for every `(index 0..=63, emphasis 0..=7)` pair, the
twelve per-subcarrier-phase composite voltages the chip actually generates within a
pixel. A shader (or any host NTSC decoder — see the frontend `signal_decode.wgsl` pass)
demodulates the *real* signal across neighbouring pixels, reproducing signal-domain
artifacts a per-colour RGB palette structurally cannot: composite colour bleed, dot
crawl, and the waterfall/dither transparency tricks (Kirby's Adventure waterfalls, the
240p test suite colour-bleed screens) that depend on adjacent-pixel chroma mixing.

The model is the canonical **Bisqwit `nes_ntsc` / Mesen2 "raw palette"** generator
(nesdev "NTSC video"): a two-level chroma square wave over 12 subcarrier phases, with
`InColorPhase((color + phase) % 12 < 6)` positioning the hue, the luma nibble selecting
the two `LEVELS` voltages, and each of the three emphasis bits attenuating by `0.746`
during the phases overlapping its primary's hue region. `generate_raw_signal_lut()`
yields the full 512-row (index-major) × 12-phase normalized table a host uploads as a
signal texture (heap-boxed; built once at shader-setup time, never on a hot path).

**Determinism:** the waveform is level-lookup + one multiply + one affine normalize with
**no transcendental**, so the `f32` output is bit-identical across x86 / aarch64 / wasm /
`thumbv7em` under IEEE-754 without `libm`. An in-crate `GOLDEN_SIGNAL` snapshot (row
`$00`, all 8 emphasis) locks that cross-target contract via a `const`-eval sibling of the
runtime path. It is **additive + default-OFF**: nothing here feeds the deterministic core
or the default presentation, so the default framebuffer golden vectors and AccuracyCoin
stay byte-identical (141/141). It is consumed only when the frontend explicitly selects
the signal-decode presentation shader.

### HD-pack tile-source export (`hd-pack` feature; v1.2.0 C3)

Behind the default-OFF `hd-pack` cargo feature, the emit path writes a third parallel
buffer — `hd_tile_source()`: one `HdTileSource` per visible pixel, in lockstep with the
index framebuffer. Each record names the **CHR tile that produced the pixel**: the
16-byte pattern-table tile base address (`$0000..=$1FF0`, fine-Y / in-tile-row masked
off), the final 2-bit palette group, the sprite flip flags, and whether the source was a
sprite or the background (`HD_TILE_NONE` marks a transparent / universal-background
pixel). The BG tile address rides a small two-stage queue (`hd_bg_addr_cur` /
`hd_bg_addr_next`) latched at `fetch_bg_lo` and promoted in `reload_bg_shift_regs` /
`prefetch_shift_bg_regs`, so it tracks the BG pattern shift registers tile-for-tile;
sprite tile bases are stashed per slot in `fetch_sprite_tile`.

It exists purely so the frontend's Mesen-style HD-pack loader can group pixels by 8×8
cell, hash the referenced CHR bytes (Mesen CRC32), and substitute hi-res replacement
tiles at blit time. Like the index framebuffer it is **output-only**: it reads no new
VRAM, issues no new A12 / mapper events, mutates no emulation state, and is not part of
the save-state — so the framebuffer is **byte-identical with the feature on or off**
(proven against the full ROM corpus: AccuracyCoin 139/141 — the two newest upstream
PPU tests are known gaps — `nestest` 0-diff, blargg /
kevtris green, identically with `hd-pack` on and off). The whole export is
`#[cfg(feature = "hd-pack")]`-gated, so the default and `no_std` builds carry no memory
or codegen cost.

## Edge cases and gotchas

1. **Mid-scanline scroll write.** Writing PPUSCROLL or PPUADDR mid-scanline shifts the BG tile fetch immediately. Common technique for status bars on top + scrolling playfield below; *Battletoads*, *Megaman III*, *Felix the Cat* all exercise this.
2. **PPUMASK rendering toggle mid-screen (OAM corruption).** Disabling rendering mid-screen during sprite evaluation corrupts 1 OAM row. The model is a faithful port of TriCNES's `PPU_Render_SpriteEvaluation` (NOT Mesen2's `_corruptOamRow`, which Mesen ships off-by-default and documents as unfinished): the corrupted row index is the **live secondary-OAM write pointer (`OAM2Address`)** captured at the instant rendering is disabled — NOT the raw dot (`dot >> 1`). The disable edge is armed by the `$2001` write (when rendering was on and the new mask turns both BG + sprites off on a render line), the index is captured during the dots 1-64 secondary-OAM-clear window (the pre-render line is excluded — it is read-only for corruption), and the corruption is DEFERRED until rendering re-enables (or the next pre-render line). On commit: `OAM[index*8 + i] = OAM[i]` for `i` in `0..8` (index `0x20` wraps to `0`), and `secondary_oam[index] = secondary_oam[0]`. Using the raw dot was the SMB3 (MMC3) bug — NMI/DMA jitter shifted the HUD-split disable dot so the flagged row intermittently landed on Mario's OAM row, wiping his sprite (80/240 idle frames dropped him). The eval-pointer index makes it deterministic. (Separately: turning rendering off mid-screen with `v` in `$3C00-$3FFF` corrupts palette RAM as mapper hardware races with the address bus — not yet modeled.)
3. **Sprite zero at X=0 with left-column hidden.** Common false-fire trap.
4. **MMC3 IRQ depends on PPU A12 toggling.** PPU calls `bus.notify_a12(level)` on every transition. Don't filter; the mapper does. Only emit A12 from actual address-bus drivers (BG/sprite pattern fetches, `$2007` reads/writes, `$2006` low-byte writes) — internal loopy increments (`inc_hori_v`, `inc_vert_v`) must NOT emit, because they don't drive the address bus and emitting from them spuriously toggles A12 against `v`'s fine-Y bit 0. Standard layout (BG=$0000, sprites=$1000) must produce exactly one A12 rise per rendered scanline at PPU dot 260.
5. **Scanline-241 dot-0 race.** Reading PPUSTATUS at scanline 241 dot 0 returns 0 *and* suppresses NMI for that frame. Test ROM coverage required.
6. **Palette read top 2 bits = open bus.** Palette is only 6-bit; the high 2 bits come from the open-bus latch.
7. **8×16 sprites use bit 0 of tile index for pattern table.** PPUCTRL bit 3 is *ignored* for 8×16 mode.
8. **Palette backdrop-override (rendering disabled, `v` in `$3F00-$3FFF`).** When rendering is disabled and the VRAM address `v` points into palette space (`$3F00-$3FFF`), palette RAM is addressed by `v`, so the PPU outputs the color at `v & 0x1F` — with the `$10/$14/$18/$1C → $00` universal-backdrop mirroring — **instead of** the universal backdrop (`$3F00`). This is a **display artifact only**: palette RAM is not mutated, and it cannot occur while rendering is enabled (the fetch pipeline owns `v`). Implemented in `Ppu::emit_pixel` as `!rendering_enabled() && (v & 0x3F00) == 0x3F00 → read_palette(0x3F00 | (v & 0x1F))`, **byte-exact with TriCNES** (`Emulator.cs`, the rendering-disabled `PaletteRAMAddress = PPU_v & 0x1F` path). This is what makes the `full_palette` / `flowing_palette` demos display all 64 colors at once (physically impossible through normal rendering). Distinct from the palette-RAM *corruption* in item 2 above (that mutates RAM and is not yet modeled; this is a read-through display effect).

## Test plan

- **`ppu_vbl_nmi`** (10 sub-ROMs): VBL flag timing to one PPU clock.
- **`ppu_open_bus`**: open-bus reads.
- **`sprite_overflow_tests`** (5 sub-ROMs): the buggy `n+m` increment.
- **`sprite_hit_tests_2005.10.05`** (10 sub-ROMs): sprite-0 hit edge cases.
- **`oam_read`**, **`oam_stress`**: OAM access behavior.
- **`ppu_sprite_hit`**: timing of sprite-0 hit set.
- **Visual diff against Mesen2 reference framebuffers** for a curated set of demo ROMs (no copyrighted content).

## Open questions

- **Open-bus decay model.** Per-bit-group with ~600 ms decay (Mesen approach), three timers — bits 0-4 / bit 5 / bits 6-7 — independently refreshed by writes and the subset of reads that drive each bit group.  See "Open-bus" entry above and `Ppu::refresh_open_bus`.
- **2C02 vs 2C07 differences.** PAL-only behaviors (no odd-frame skip; different post-reset masking window). Implement region as a parameter; don't fork the PPU code.
- **Vs. System PPU variants (2C03, 2C04, 2C05).** Out of v1.0 scope; design `Ppu` so a future `Ppu2C03` could share most code.

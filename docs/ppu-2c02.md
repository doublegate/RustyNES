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
stable OAM, palette, nametable, CHR RAM, or PPUSTATUS low-bit state.

### Per-dot fetch sequencing (visible + pre-render scanlines)

- **Dot 0** — idle.
- **Dots 1..=256** — background tile fetch (8-dot windows of NT, AT, PT-low, PT-high, 2 dots each). Shift registers reload on dot 9, 17, 25, ..., 257. Pixels emit dots 1..=256.
- **Dots 257..=320** — sprite tile fetch for next scanline. OAMADDR is forced to 0. 8 sprites × 4 fetches (garbage NT, garbage NT, PT-low, PT-high). Sprite X positions and attributes load during the second garbage fetch. Horizontal `v` bits reload from `t` at dot 257. The pattern-table fetches happen for **all 8 slots regardless of how many real sprites are in range** — empty slots use the cleared secondary-OAM tile `$FF`, producing a dummy fetch that still drives A12 to the sprite pattern table. This is what produces the per-scanline A12 rising edge that MMC3's IRQ counter clocks on; without dummy fetches, scanlines with zero visible sprites would emit no rise. Sprite tile fetch also runs on the pre-render scanline (for scanline 0's sprites), so an NTSC frame yields exactly 240 visible + 1 pre-render = **241 A12 rises** with standard pattern-table layout (BG=$0000, sprites=$1000) — the count `mmc3_test_2/2-details` validates.
- **Dots 280..=304 (pre-render only)** — vertical `v` bits reload from `t` if rendering enabled.
- **Dots 321..=336** — first two BG tiles of next scanline.
- **Dots 337..=340** — two extra NT fetches (purpose debated; preserve them so MMC5's frame-end detection works).

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

Future work (post-flip):

- Reading `$2004` during cycles 1-64 should return `$FF` (idle clear
  phase). The per-dot FSM has the state for this but the `$2004` read
  path does not yet observe the FSM phase. Add when a test ROM
  distinguishes the behavior.
- OAMADDR-during-rendering corruption: real hardware drives
  `OAMADDR=0` across dots 257-320. Not yet modeled in either path.
  Add when a test ROM distinguishes the behavior.

### Sprite-0 hit

Set when a non-transparent pixel of sprite 0 overlaps a non-transparent pixel of background. Constraints:

- Cannot set on dot 255.
- Cannot set if either left-column-show flag is off and X is 0..=7.
- Cannot set on the pre-render scanline.
- Cleared at scanline 261 (or 311 PAL) dot 1.

Determined during sprite *rendering*, not evaluation.

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
(proven against the full ROM corpus: AccuracyCoin 139/139, `nestest` 0-diff, blargg /
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

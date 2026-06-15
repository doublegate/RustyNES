# Compatibility scope

**References:** `ref-docs/research-report.md` §Scope and goals → In-scope /
Out-of-scope; `ref-docs/nesdev-wiki-technical-report.md`;
`docs/nesdev-hardware-emulation-checklist.md`.

## Region targets (v1.0)

| Region | Status | Notes |
|--------|--------|-------|
| NTSC (Famicom JP, NES NA, NES AU) | First-class | Primary development target |
| PAL (NES EU) | First-class | Different timing tables, 312 scanlines, no odd-frame skip |
| Dendy (PAL famiclones) | First-class | 50 Hz refresh, NTSC-style timing semantics |

Region detection per NES 2.0 byte 12; iNES 1.0 fallback uses heuristics (file extension hints, mapper-region pairs).

## Hardware variant scope (v1.0)

| Variant | Status | Notes |
|---------|--------|-------|
| Famicom (HVC-001) | Yes | Reference NTSC |
| NES (NES-001) | Yes | Functionally identical CPU+PPU |
| NES (NES-101 / "top loader") | Yes | Same internals |
| AV Famicom (HVC-101) | Yes | RGB output ignored; we emit composite-style anyway |
| Famicom Disk System (FDS) | **Yes** | RAM adaptor (mapper 20) + user-supplied `disksys.rom` BIOS; see the FDS bullet below |
| Vs. System (arcade) | **Game-verified** | RGB PPU (2C03/2C04/2C05) + 2C05 quirks + DIP/coin inputs; **mappers 99 + 151 game-verify in-game RGB** — Vs. Excitebike / Vs. Clu Clu Land + Vs. Castlevania / Vs. Pinball (clean-byte detection) boot through the 2C03 palette. See the Vs./PC10 bullet below |
| PlayChoice-10 (arcade) | **Game-verified** | NES-game part renders through the 2C03 RGB palette — Power Blade / Rad Racer 2 / Captain Skyhawk / Mario Open Golf etc. (clean-byte-7 `0x02` detection); second-screen menu + Z80 out of scope |

### PPU variant scoping (Phase 7 / T-73-007)

The RGB PPUs — **2C03 / 2C04 / 2C05** (Vs. System, PlayChoice-10) — replace the
2C02 with a hardware RGB palette (not the composite palette this emulator
generates for stock NES). The emulator implements:

- **The RGB palette tables.** All five distinct hardware palettes are baked in
  (`crates/rustynes-ppu/src/palette.rs`): the 2C03 (= 2C05) master palette and the
  four 2C04-0001..-0004 copy-protection permutations, transcribed from nesdev
  "PPU palettes" (`C = 255 * DAC / 7`). The PPU routes through
  `palette_color_to_rgba(active_palette, ..)`; the default `Composite2C02` path
  is **byte-for-byte identical** to the legacy 2C02 output. RGB-PPU colour
  emphasis follows the hardware model (each emphasis bit forces *its* channel to
  full brightness, the opposite of the 2C02's darkening).
- **2C05 register quirks.** `$2000`/`$2001` are swapped and `$2002` returns the
  2C05 sub-variant identifier (`$3D`/`$1C`/`$1B` for 2C05-02/03/04) in its low
  bits. Gated on an `is_2c05` flag (false on every other PPU).
- **NES 2.0 byte 13** parsing (`VsPpuType`) resolves the Vs. PPU type → palette +
  2C05 flag, wired into the PPU at construction.
- **Vs. System inputs.** 8-bit DIP switches + coin/service inputs overlay the
  upper bits of `$4016`/`$4017` per the Vs. protocol (`Nes::set_vs_dip` /
  `insert_coin` / `set_vs_service`). Gated on the cart's console type.

**In-game RGB is game-verified for BOTH arcade platforms.** Two
detection signals route a cart through the 2C03 RGB PPU at parse time:

1. **Mapper-driven** (the most robust): mappers **99 (Nintendo Vs. System)** and
   **151 (Konami VRC1 on a Vs. board)** are Vs.-only — no licensed home game uses
   either — so a cart bearing one is forced to `ConsoleType::VsSystem` + the 2C03
   RGB PPU. Verified by **Vs. Excitebike / Vs. Clu Clu Land** (m99) and **Vs.
   Gradius / Vs. The Goonies** (m151) booting through the 2C03 palette.
2. **Clean-byte-7 arcade flag** (real No-Intro arcade dumps): a genuine Vs./PC10
   dump is clean iNES **1.0** with byte 7 **exactly** `0x01` (Vs.) or `0x02`
   (PC10). Both route to the 2C03 RGB PPU. This is the path that game-verifies
   **PlayChoice-10**: the NES-game half of **Power Blade / Rad Racer 2 / Captain
   Skyhawk / Mario Open Golf** (all byte-7 `0x02`) renders through the 2C03
   palette, as do the clean-byte Vs. dumps **Vs. Castlevania / Vs. Pinball**
   (byte-7 `0x01`).

   The critical guard: the notorious corruption is byte 7 == `0x0A` (console
   field 2 = PlayChoice-10 **plus** the NES-2.0 marker bits 2-3 = `10`), carried
   by many home dumps (e.g. the committed `Excitebike.nes`). Because `0x0A` is
   NES 2.0 AND is neither `0x01` nor `0x02`, it is **ignored** — the cart stays
   composite (`VsPpuType::None` → `Composite2C02`, byte-for-byte the legacy home
   path). A survey confirmed **no oracle/home ROM carries a clean `0x01`/`0x02`**,
   and the 60-ROM + 49-title oracles remain byte-identical after this change.

`crates/rustynes-test-harness/tests/vs_system_rgb.rs` asserts every rendered colour is
a 2C03-palette colour and at least one is impossible under the composite 2C02
(proving genuine RGB routing). The Vs./PC10 dumps are iNES **1.0** (no byte-13),
so the parser defaults the palette to 2C03; the **per-game database**
supplies the correct 2C04-000x / RC2C03 permutation for games that used one (see
below). Vs. games sit on an insert-coin attract loop until
a coin is latched (`Nes::insert_coin`); PC10 games render directly.

### Vs. System per-game database

`crates/rustynes-core/src/vs_db.rs` is an embedded, SHA-256-keyed, binary-searched
table (`no_std`-safe const data; `rustynes_core::vs_db::lookup`) that closes two gaps
for Vs. carts:

1. **Correct PPU palette.** iNES-1.0 dumps carry no NES 2.0 byte-13, so the
   parser defaults every Vs. cart to the 2C03 — wrong for the many games that
   used a 2C04-000x (different copy-protection LUT) or an RC2C03. The DB maps the
   ROM hash to its real `VsPpuType`, applied via `Nes::set_vs_ppu_type` (which
   re-runs the palette via the bus's private `reapply_vs_palette`, shared by the
   constructor + power-cycle). The DB is **authoritative for the palette**.
2. **DIP-switch presets.** Each entry carries the game's documented factory
   `DSW0` default (sourced from MAME `src/mame/nintendo/vsnes.cpp`), in this
   emulator's encoding (switch 1 = bit 0 .. switch 8 = bit 7, exactly the byte
   `Nes::set_vs_dip` consumes).

The frontend's `apply_vs_db` (`crates/rustynes-frontend/src/app.rs`) runs on every ROM
load and applies the DIP with the precedence **explicit `[vs] dip` config >
per-game DB default > 0**. To pin an explicit DIP that overrides the DB, set
both `[vs] dip = <value>` **and** `[vs] dip_set = true` in `config.toml`
(`dip_set` is serde-default `false`, so existing configs and not-in-DB games are
unaffected). 16 entries (Excitebike, Clu Clu Land, Castlevania, Pinball,
Balloon Fight, Tennis, Mahjong, Stroke & Match Golf, Wrecking Crew, Gradius,
Goonies, Ice Climber, Duck Hunt, T.K.O. Boxing, Super Mario Bros.) cover the
staged Vs. set. **PPU types** are taken from MAME `src/mame/nintendo/vsnes.cpp` —
each game's `ROM_START` block names its hardware palette ROM via the
`PALETTE_2C04_000x` / `PALETTE_STANDARD` macro (for DualSystem carts the `ppu1`
master-CPU palette), cross-checked against the fceux `src/vsuni.cpp`
"Games/PPU list. Information copied from MAME" table; both agree for every staged
game. The 2C04-0001..-0004 scramble tables in `crates/rustynes-ppu/src/palette.rs`
were verified byte-for-byte against the nesdev "PPU palettes" DAC tables
(`C = 255*DAC/7`). Re-audited 2026-06-11 against MAME `master`: all 15 staged
assignments were already correct, no change required. Per-game assignments
(MAME driver -> PPU): vsgradus/vspinbal -> 2C04-0001; cstlevna/smgolf/wrecking
-> 2C04-0002; excitebk(o)/goonies/tkoboxng/balonfgt -> 2C04-0003;
suprmrio/iceclimb/cluclu -> 2C04-0004; duckhunt/vstennis/vsmahjng ->
RC2C03B (standard). Note the Japanese sets differ (excitebkj -> 2C04-0004,
vspinbalj -> standard) but those ROMs are not staged.

**Known limitation — Vs. DualSystem games** (Balloon Fight / Tennis / Mahjong /
Wrecking Crew): these run two CPUs/PPUs. This single-CPU model does not boot them
past the attract screen regardless of the DIP (an empirical DIP sweep confirmed
no DIP value unblocks them — the black screen is the missing sub-CPU, not the
DIP). Their entries are kept for palette correctness and forward use once
DualSystem is modelled. The non-DualSystem games (Excitebike, Clu Clu Land,
Castlevania, Pinball, Gradius, Goonies, Ice Climber, Golf, Super Mario Bros.)
boot and render with their correct 2C04 palette.
**PlayChoice-10's second-screen instruction menu and its Z80 coprocessor are out
of scope** — only the NES-game half runs (with the 2C03 palette). All of the above
is gated on `ConsoleType::VsSystem`/`Playchoice10`; a stock `Nes` cart is byte-for-
byte unchanged (AccuracyCoin 100.00% (139/139) + both ROM oracles byte-identical).
Region timing (PAL/Dendy)
is validated by automated gates (`ppu_region_constants_match_hardware` in
`rustynes-ppu`; `region_timing.rs` in `rustynes-test-harness`). The R1
master-clock scheduler runs the **region-exact CPU:PPU clock ratio** —
3:1 NTSC/Dendy and the hardware-true **3.2:1 PAL** (the `region_timing` PAL gate
asserts 33,247 CPU cyc/frame, not the legacy 3:1 approximation's 35,464).

## Mapper scope (v1.0)

See `docs/mappers.md` §Mapper coverage matrix. **51 mapper families**
(developed across the engine's v2.0–v2.6 lineage and all shipping in RustyNES
v1.0.0: the top-25 by ROM count first; then 16/159, 18, 64, 65, 67, 68,
70, 73, 78, 88/206, 118, 210; then 119 TQROM — Pin\*Bot, High Speed; then 33
TC0190, 93 Sunsoft-3R, 99 Vs. System, 152 Bandai-74161, then a second batch of
32 Irem G-101, 48 Taito TC0690 (A12 IRQ), 87 Jaleco CNROM-style, 89 Sunsoft-2,
184 Sunsoft-1, then a third batch of 80 Taito X1-005, 82 Taito X1-017, 151 Konami
VS / VRC1-on-Vs.), covering the bulk of the licensed library. These are
spec-implemented from the nesdev
wiki + unit-tested + boot-smoked (commercial-ROM visual survey via the
`coverage_smoke` bin). Pirate, multicart, and ultra-niche mappers remain gated by
the long-tail policy below.

**Known long-tail render gap — mapper 89 (Sunsoft-2).** *Tenka no
Goikenban: Mito Koumon* (the only iNES mapper-89 dump on hand) boots and executes
(CPU/RAM live; sprites briefly render) but its background nametable stays empty
and the picture degrades to the backdrop colour after ~400 frames. The banking is
spec-correct (vectors resolve in the fixed last bank; CHR sprite fetches work), so
the root cause is a rendering-enable / PPU-setup dependency this title needs that
is not yet modelled — not a banking error. The 9 other long-tail batch-2 games
(both m32, both m87, both m184, Don Doko Don 2 on m48) render correctly and 7 are
locked into the `external_extended` oracle.

## Audio expansion scope

| Mapper audio | Status | Notes |
|--------------|--------|-------|
| MMC5 (2 pulse + raw PCM) | **Landed** (`mapper-audio`, Track C2 / Phase 2.3) | Castlevania III JP, Just Breed, Laser Invasion |
| VRC6 (3 channels) | Phase 4 | Akumajou Densetsu, Madara, Esper Dream 2 |
| VRC7 (FM, 6 channels) | **Landed** — clean-room `emu2413` port (`crates/rustynes-apu/src/opll.rs`, MIT); ADR 0006 supersedes ADR 0004 | Lagrange Point (JP) plays with in-game audio. |
| Sunsoft 5B (3 channels) | Phase 4 | Gimmick! |
| Namco 163 (1-8 channels) | Phase 4 | Several Japanese RPGs |
| FDS (wavetable + envelope) | **Landed** | 2C33 — 64-entry wavetable + 32-step modulation + envelopes + master volume; behind `mapper-audio` |

## Game compatibility goals (v1.0)

- The 100 best-selling NES titles boot, render correctly, audio plays correctly.
- All blargg + kevtris test ROMs targeted in `docs/testing-strategy.md` pass.
- Holy Mapperel detects and exercises every implemented mapper.
- AccuracyCoin pass rate ≥ 90%.
- Manual smoke test of Mesen2's "compatibility-difficulty" set: *Battletoads*, *Megaman III*, *Punch-Out!!*, *Castlevania III*, *Cobra Triangle*, *Mig 29 Soviet Fighter*.

## Out-of-scope (v1.0) — HISTORICAL

> Several items below have since shipped (WebAssembly, TAS, VRC7 FM). See
> "Platform scope" immediately below for the authoritative current list; it
> supersedes this one where they disagree. All listed capabilities ship in
> RustyNES v1.0.0.

- Famicom Disk System (FDS).
- Vs. System and PlayChoice-10 arcade variants.
- Network play. **(Shipped — GGPO-style rollback netcode over UDP, native; see `docs/release-notes/v2.3.0.md`.)**
- WebAssembly target.
- Mobile (iOS/Android).
- TAS (tool-assisted speedrun) movie recording / playback. Architecture supports it (deterministic core); UI is post-v1.0.
- Cheats / Game Genie codes.
- Non-standard input devices beyond standard pads unless explicitly listed in
  release notes. NES 2.0 default-device metadata is parsed but not yet a full
  device-selection system.

## Platform scope (Phase 7)

This section supersedes the early "Out-of-scope" list above where they disagree
(several items ship in v1.0.0). Decisions from Phase 7 Sprint 4:

- **Shipped in v1.0.0:** WebAssembly target, TAS movie recording /
  playback, VRC7 FM audio. These are no longer out of scope.
- **FDS (Famicom Disk System) — SUPPORTED.** `.fds` images play. The
  `.fds`/fwNES parser (`rustynes_mappers::parse_fds`); the FDS RAM-adapter device
  (`rustynes_mappers::Fds`, iNES mapper 20, a `Box<dyn Mapper>`) owning 32 KiB PRG-RAM
  (`$6000-$DFFF`), 8 KiB CHR-RAM, and the user-supplied 8 KiB BIOS
  (`$E000-$FFFF`); the register map (`$4020-$4026`/`$4030-$4033`) + 16-bit
  per-CPU-cycle timer IRQ; the disk **read + write** drive (`$4025`/`$4030`/
  `$4031`/`$4024`, ~149-cycle cadence, byte-transfer IRQ) with **multi-side
  eject/insert** + writable-disk persistence; and the **2C33 wavetable audio**
  (`$4040-$4092`: 64-entry wavetable + 32-step modulation + envelopes + master
  volume, behind `mapper-audio`, via `mix_audio`→`tick_with_external`). Construct
  via `Nes::from_disk(disk_bytes, bios_bytes)`. Frontend: `.fds` open/drag-drop,
  a one-time BIOS prompt (new `[fds]` config), an `F9` side-swap key + a disk
  indicator, `.fds.sav` persistence under `<data_dir>/fds-saves/`; wasm-winit has
  an in-browser BIOS upload (session-only). **The BIOS is user-supplied (Nintendo
  copyright — NEVER committed); real-BIOS boot is unverified in CI by design** —
  the device + audio are unit-tested (56 FDS unit tests) + an env-gated
  (`RUSTYNES_FDS_BIOS`) `fdsirqtests.fds` path. Simplified: no CRC/gaps-on-write,
  no analog seek timing. nesdev `FDS*` is the primary source; Mesen2
  `Core/NES/Mappers/FDS/` is a structural-only (GPL-3.0) reference.
- **Input devices — standard pad + Four Score + Zapper + Vaus + Power Pad.** The
  **Four Score** 4-player adapter is supported. Also supported: the
  **Arkanoid Vaus paddle** (`Nes::set_paddle`, game-ROM-verified against
  `vaus-test`), the **Zapper** light gun (`Nes::set_zapper`, framebuffer-luma
  light-detect, unit-verified only), and the **Power Pad / Family Fun Fitness mat**
  (`Nes::set_power_pad`, the 12-button dual-shift-register serial protocol,
  unit-verified against the `NESdev` / Mesen bit layout) as opt-in per-port
  `InputDevice` overlays — when no device is attached the standard-controller +
  Four Score read paths are byte-identical. (v1.1.0 beta.1: the Power Pad is playable —
  selectable as the player-2 device with a 12-key default mapping; rebindable mat keys
  are a follow-up.) Famicom expansion-port devices (the Family BASIC keyboard), the
  microphone, and DMC-DMA controller-bit corruption remain deferred.
- **Vs. System / PlayChoice-10 (2C03/04/05 RGB PPUs) — game-verified.**
  RGB palette tables + 2C05 register quirks + NES 2.0 byte-13 parsing + Vs.
  DIP/coin inputs; **mappers 99 + 151 game-verify in-game RGB** (Vs.
  Excitebike / Vs. Clu Clu Land / Vs. Gradius / Vs. The Goonies), and the
  **clean-byte-7 arcade flag** (exact `0x01`/`0x02` on a non-NES-2.0 header)
  game-verifies **PlayChoice-10** (Power Blade / Rad Racer 2 / Captain Skyhawk /
  Mario Open Golf) plus the clean-byte Vs. dumps (Vs. Castlevania / Vs. Pinball).
  The corrupted `0x0A` home-dump trap is explicitly ignored (it is NES 2.0 and is
  neither `0x01` nor `0x02`), so the oracles stay byte-identical. The dumps are
  iNES 1.0, so 2C04-000x games would need byte-13 / a game-DB. PC10 second screen
  - Z80 remain out of scope (see "PPU variant scoping").
- **Long-tail mapper policy.** A pirate / multicart / homebrew-only mapper is
  accepted only when: (a) there is concrete user demand or a notable title that
  needs it, **and** (b) a redistributable test fixture or a well-specified
  nesdev page exists, **and** (c) it carries NES 2.0 metadata for unambiguous
  detection — weighed against maintenance cost. MMC5's >8 KiB multi-chip PRG-RAM
  configs fall under this policy (no corpus fixture; out of scope until one
  appears).

## Open questions

- **CRC32 vs. SHA-256 for ROM identification.** We use SHA-256 for save state directory naming (lower collision risk). For ROM compatibility databases, CRC32 is the community standard; we may add it as a secondary key.
- **Region override.** Some users want to play PAL versions of NTSC games at NTSC speed. Plan: expose a region override in the settings UI; warn that this may break timing-sensitive ROMs.

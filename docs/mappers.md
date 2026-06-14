# Mappers and cartridge

**References:** `ref-docs/research-report.md` §Cartridge format and mappers;
`ref-docs/nesdev-wiki-technical-report.md` §Common Mapper Families; Nesdev
[Mapper](https://www.nesdev.org/wiki/Mapper),
[Bus conflict](https://www.nesdev.org/wiki/Bus_conflict),
[MMC1](https://www.nesdev.org/wiki/MMC1),
[MMC3](https://www.nesdev.org/wiki/MMC3), and
[MMC5](https://www.nesdev.org/wiki/MMC5).

## Purpose

Implement the cartridge subsystem in `crates/rustynes-mappers`: a `Mapper` trait, a `Cartridge` struct that owns the ROM/RAM banks and a boxed `dyn Mapper`, and concrete implementations of the top ~25 mappers (covering >95% of the licensed library).

## Interfaces

```rust
pub trait Mapper: Send {
    fn cpu_read(&mut self, addr: u16) -> u8;          // $4020-$FFFF
    fn cpu_write(&mut self, addr: u16, value: u8);
    fn ppu_read(&mut self, addr: u16) -> u8;          // $0000-$3FFF (CHR + nametable)
    fn ppu_write(&mut self, addr: u16, value: u8);

    fn notify_a12(&mut self, level: bool) {}          // for MMC3/MMC5
    fn notify_cpu_cycle(&mut self) {}                 // for CPU-cycle IRQ counters (VRC, FME-7)
    fn irq_pending(&self) -> bool { false }
    fn irq_acknowledge(&mut self) {}

    fn mix_audio(&mut self) -> i16 { 0 }              // VRC6/7, MMC5, Sunsoft 5B, Namco 163, FDS

    fn save_state(&self) -> Vec<u8>;
    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError>;
}

pub struct Cartridge {
    pub prg_rom: Box<[u8]>,
    pub chr_rom: Box<[u8]>,          // empty if cart uses CHR-RAM
    pub mapper_id: u16,
    pub submapper: u8,
    pub mirroring: Mirroring,
    pub region: Region,
    pub console_type: ConsoleType,
    pub prg_ram_size: u32,
    pub chr_ram_size: u32,
    pub has_battery: bool,
    pub has_trainer: bool,
    pub is_nes2: bool,
}

pub enum Mirroring { Horizontal, Vertical, SingleScreenA, SingleScreenB, FourScreen, MapperControlled }
```

`rustynes_mappers::parse(&[u8]) -> Result<(Cartridge, Box<dyn Mapper>), RomError>`
parses an iNES or NES 2.0 file (see `cartridge-format.md`), constructs the
appropriate concrete mapper, and returns the metadata header together with
the boxed mapper as a **tuple**. The tuple shape is the pragmatic one: the
`Cartridge` is cheap-to-clone metadata (region, mirroring, mapper id, ROM
slices) that the bus, save-state path, and debugger all want to inspect
without disturbing the mapper, while the `Box<dyn Mapper>` is the live
mutable state. Keeping them as separate ownership roots lets the bus own
the mapper exclusively while metadata can be passed by `&` or cheaply
cloned for diagnostics.

## State

The `Cartridge` owns immutable PRG-ROM and CHR-ROM banks plus mutable PRG-RAM and CHR-RAM. The mapper holds:
- Bank-select registers (one per banking dimension).
- Mirroring control state (if mapper-controlled).
- IRQ counter state (latch, counter, enable, pending).
- For audio mappers: extra channel timers, DAC values, frame counter sub-state.

The mapper does *not* directly own the ROM bytes — it receives a reference to the cart-owned arrays via constructor. This avoids duplication and keeps the `dyn Mapper` boxed type small.

## Behavior

### Banking pattern

Every mapper resolves a CPU/PPU address into a `(bank_index, offset)` pair on every access, then indexes into the cart-owned ROM/RAM. This is hot code — inline aggressively.

### IRQ counter mechanisms (the four families)

1. **None** — NROM, UxROM, AxROM, MMC1, CNROM, MMC2/4, BNROM, GxROM, Color Dreams, CPROM. No IRQs.
2. **PPU A12 edge counter** — MMC3 (and clones, e.g., Namco 108 derivatives). Notify on every A12 transition; filter is *internal to the mapper*. Per `ref-docs/research-report.md` §MMC3, the filter requires A12 to remain low for 3 falling edges of M2 before the next rising edge counts.
3. **PPU scanline count** — MMC5. Detects scanline by observing PPU attribute-table fetches; IRQ at PPU cycle 4 of the target scanline.
4. **CPU cycle counter** — VRC2/4/6/7, Sunsoft FME-7, Namco 163. Tick on every CPU cycle (`notify_cpu_cycle`).

### Mirroring

Most mappers expose a register that selects horizontal, vertical, single-screen A, or single-screen B. The PPU's nametable fetch consults `cart.ppu_read(addr)` for `$2000-$3FFF`; the mapper applies mirroring. Four-screen mode requires extra cart-side VRAM (typically 2 KB) on top of the console's 2 KB.

**Per-game mirroring override (v1.1.0 beta.1, T-110-B4).** The bus carries an optional `nt_mirroring_override: Option<Mirroring>` (set via `Nes::set_mirroring_override`). When `Some`, the bus's `$2000-$3EFF` nametable translation uses it instead of the mapper's `nametable_address` — a load-time correction for ROMs with a wrong iNES mirroring flag, supplied by the frontend's CRC32-keyed game database (`rustynes-frontend::game_db`). It does **not** affect mapper-supplied VRAM (`nametable_fetch`, e.g. four-screen), is `None` by default (byte-identical; the core test suites never set it), and is persisted in the save-state. See `docs/compatibility.md` §Input devices for the device side and the CHANGELOG for the rollout.

### Bus conflicts

Some early mappers (CNROM, AxROM, GxROM, Color Dreams) do not buffer writes to the bank-select range, so the value written collides with the value being *read* from PRG at the same address. Implementations should AND the written value with the PRG byte at that address (per NESdev wiki bus-conflict rules). Affects rare but real test cases.

Bus conflict behavior is board-specific. ASIC mappers usually disable PRG ROM
outputs during writes; many discrete mappers do not. NES 2.0 submappers can
distinguish some conflict-free homebrew or modified boards from original
conflict-prone boards. The mapper implementation should therefore decide
conflicts from mapper/submapper/board metadata, not only from mapper number.

## Mapper coverage matrix (Phase 4 status)

Sorted by number of commercial titles using each mapper.

| iNES | Submapper | Name | Phase | Audio | IRQ | Status | Notes |
|------|-----------|------|-------|-------|-----|--------|-------|
| 0 | — | NROM | 1 | — | — | landed (Phase 1) | 247 titles. Trivial; no banking. |
| 1 | 1-5 | MMC1 (SUROM, SXROM, etc.) | 2 | — | — | landed (Phase 2) | Serial 5-write protocol; consecutive-write bug. |
| 2 | 0-2 | UxROM | 2 | — | — | landed (Phase 2) | UNROM, UOROM, etc. CHR-RAM only. |
| 3 | 0-2 | CNROM | 2 | — | — | landed (Phase 2) | Bus conflict required. |
| 4 | 0-3 | MMC3 (and MMC6, sub 1) | 4 | — | A12 | landed (Phase 4 / S1) | Sharp vs NEC IRQ revision; default Sharp. mmc3_test_2/5-MMC3 passes; sub-tests 1-4 partial. |
| 5 | — | MMC5 | 4 | yes (deferred) | scanline | v0+v1 landed (Phase 4 / S4) | Banking + scanline IRQ + ExRAM modes 10/11 + multiplier (v0). Fill mode (`$5106`/`$5107`), dual sprite/BG CHR registers used for sprite tile fetches, ExGrafix per-tile attribute + CHR override (mode 01) (v1). Deferred: vertical split-screen (`$5200-$5202`), audio extension (`mmc5-audio`). |
| 7 | 0-2 | AxROM | 2 | — | — | landed (Phase 2) | Single-screen mirroring control. |
| 9 | — | MMC2 | 4 | — | — | landed (Phase 4 / S2) | Punch-Out; latched CHR per fetch ($FD/$FE). |
| 10 | — | MMC4 | 4 | — | — | landed (Phase 4 / S2) | Like MMC2 with full PRG banking. |
| 11 | — | Color Dreams | 4 | — | — | landed (Phase 4 / S2) | Unlicensed; bus conflict. |
| 13 | — | CPROM | 4 | — | — | landed (Phase 4 / S2) | Videomation. |
| 19 | — | Namco 163 | 4 | yes (landed) | CPU | banking+IRQ+audio landed (Phase 4 / S3 + Track C2 / Phase 2.2) | Mappy-Land, King of Kings, Final Lap, Rolling Thunder, Megami Tensei II.  1-8 wavetable channels playing 4-bit wavetables from 128 B mapper-internal sound RAM.  Address-port at `$F800-$FFFF` (bit 7 = auto-increment, bits 6-0 = 7-bit RAM address) + data-port at `$4800-$4FFF`; per-channel registers at the top of internal RAM (channel 8 at `$78-$7F`, channel 1 at `$40-$47`); 18-bit frequency + 24-bit phase + 6-bit wave-length + nibble-addressed wave start address + 4-bit volume per channel; `$E000` bit 6 = audio-disable.  Gated behind the `mapper-audio` cargo feature. |
| 21 | 1, 2 | VRC4a / VRC4c | 4 | — | CPU | landed (Phase 4 / S3) | Konami; Wai Wai World. |
| 22 | — | VRC2a | 4 | — | — | landed (Phase 4 / S3) | Konami. |
| 23 | 1-3 | VRC4e / VRC4f / VRC2b | 4 | — | CPU | landed (Phase 4 / S3) | Konami. |
| 24 | — | VRC6a | 4 | yes (landed) | CPU | banking+IRQ+audio landed (Phase 4 / S3 + Track C2) | Akumajou Densetsu.  3 extra audio channels (2 pulse + 1 sawtooth) gated behind the `mapper-audio` cargo feature. |
| 25 | 1-3 | VRC4b / VRC4d / VRC2c | 4 | — | CPU | landed (Phase 4 / S3) | Konami. |
| 26 | — | VRC6b | 4 | yes (landed) | CPU | banking+IRQ+audio landed (Phase 4 / S3 + Track C2) | Madara, Esper Dream 2.  Same channels as VRC6a; A0/A1 swap. |
| 34 | 0-2 | BNROM / NINA-001 | 4 | — | — | landed (Phase 4 / S2) | Submapper 1 selects NINA-001. |
| 66 | — | GxROM | 2 | — | — | landed (Phase 2) | Bus conflict. |
| 69 | — | Sunsoft FME-7 | 4 | yes (5B, landed) | CPU | banking+IRQ+audio landed (Phase 4 / S3 + Track C2 / Phase 2.1) | Gimmick!  Sunsoft 5B = YM2149F clone: 3 squares + 32-step envelope generator + 17-bit LFSR noise.  Two-write protocol via `$C000-$DFFF` (address latch) and `$E000-$FFFF` (data); audio gated behind the `mapper-audio` cargo feature. |
| 71 | — | Camerica BF9093 | 4 | — | — | landed (Phase 4 / S2) | Codemasters titles. |
| 75 | — | VRC1 | 4 | — | — | landed (Phase 4 / S2) | Konami. |
| 85 | — | VRC7 | 5 | yes (FM, landed) | CPU | banking+IRQ+**OPLL FM audio landed** | Lagrange Point (JP). Banking + CPU-cycle IRQ identical to VRC6's. OPLL FM via the clean-room `emu2413` port (`crates/rustynes-apu/src/opll.rs`, MIT; ADR-0006 supersedes ADR-0004). |

### First long-tail batch (14 families, 25 → 39)

Spec-implemented from the nesdev wiki with register/IRQ/nametable unit tests +
boot-smoke (no redistributable behavioral fixtures exist for these boards).

| iNES | Submapper | Name | Audio | IRQ | Notes |
|------|-----------|------|-------|-----|-------|
| 16 / 159 | 0,4,5 | Bandai FCG | — | CPU | DBZ, Famicom Jump II, Datach. +minimal I2C EEPROM (24C02/24C01). |
| 18 | — | Jaleco SS88006 | — (ADPCM decoded-not-emulated) | CPU | Goemon Gaiden, Doropie. Nibble-paired banking; selectable-width IRQ. |
| 64 | — | Tengen RAMBO-1 | — | A12 + CPU | Klax, Skull & Crossbones. Dual-mode IRQ (reuses MMC3 A12 filter). |
| 65 | — | Irem H3001 | — | CPU | Daiku no Gen-san, Spartan X 2. 16-bit reload-latch down-counter. |
| 67 | — | Sunsoft-3 | — | CPU | Fantasy Zone 2. 16-bit write-twice-latch IRQ. |
| 68 | — | Sunsoft-4 | — | — | After Burner, Maharaja. CHR-ROM-as-nametable. |
| 70 | — | Bandai discrete | — | — | Kamen Rider Club, Family Trainer. UxROM-like. |
| 73 | — | Konami VRC3 | — | CPU | Salamander. Simplest VRC; 8K CHR-RAM. |
| 78 | 1,3 | Holy Diver / Cosmo Carrier | — | — | Submapper-selected mirroring. |
| 88 / 206 | — | Namco 118 / DxROM | — | — | Dragon Spirit, Quinty, Family Circuit. MMC3 banking subset. |
| 118 | — | TxSROM / TLSROM | — | A12 | Armadillo, NES Play Action Football. MMC3 + per-slot NT mirroring. |
| 119 | — | TQROM | — | A12 | Pin\*Bot, High Speed. MMC3 + mixed CHR (64K CHR-ROM + 8K CHR-RAM; bank bit 6 = RAM select). |
| 210 | 1,2 | Namco 175 / 340 | — | — | Famista variants. Submapper-split banking/mirroring; no IRQ. |

### Second long-tail batch (4 families, 39 → 43)

Spec-implemented from the nesdev wiki with register/bank unit tests. Mapper 99
is the headline: it is the robust, mapper-driven Vs. System signal (no licensed
home game uses it), so detecting it forces `ConsoleType::VsSystem` + the 2C03
RGB PPU at parse time — finally enabling in-game RGB-PPU verification of the
RGB device.

| iNES | Submapper | Name | Audio | IRQ | Notes |
|------|-----------|------|-------|-----|-------|
| 33 | — | Taito TC0190 / TC0350 | — | — | Don Doko Don, Power Blazer. 2x8K PRG + 2x2K + 4x1K CHR; software mirroring. (mapper 48 = TC0690, the +A12-IRQ variant.) |
| 93 | — | Sunsoft-3R | — | — | Shanghai, Fantasy Zone. UxROM-like: PRG bits 4-6 + CHR-RAM-enable bit 0; 8K CHR-RAM. |
| 99 | — | **Nintendo Vs. System** | — | — | **Vs. Excitebike, Vs. Clu Clu Land.** Fixed PRG (8/16/32K) + 8K CHR bank from `$4016` bit 2. Forces Vs. System + 2C03 RGB PPU (mapper-driven, immune to the byte-7 trap). |
| 152 | — | Bandai 74161/161 (1-screen) | — | — | Arkanoid II, Pocket Zaurus. UxROM-like (PRG bits 4-6, CHR bits 0-3) + bit-7 software 1-screen select. |

### Third long-tail batch (5 families, 43 → 48)

Five more spec-implemented licensed boards (nesdev wiki), each with register/bank
unit tests and a commercial-ROM visual survey via `coverage_smoke`. Mapper 48 is
the headline of the batch: an MMC3-style A12 scanline IRQ grafted onto the
TC0190 banking shape.

| iNES | Submapper | Name | Audio | IRQ | Notes |
|------|-----------|------|-------|-----|-------|
| 32 | 1 = Major League | Irem G-101 | — | — | Image Fight, Major League, Kaiketsu Yancha Maru 2, Magical Pop's. 2x8K PRG with a `$9000` swap-mode bit + 8x1K CHR + software H/V mirroring. Submapper 1 (Major League) hard-wires single-screen A and ignores the `$9000` mirroring bit. |
| 48 | — | Taito TC0690 | — | **A12 scanline** | Don Doko Don 2, Flintstones 2, Jetsons, Bakushou!! Jinsei Gekijou 3. TC0190 banking + an MMC3-style A12 IRQ (`$C000` latch = `value ^ 0xFF`, `$C001` reload, `$C002`/`$C003` enable/disable) + `$E000` bit-6 mirroring. The TC0690 has a 1-CPU-cycle IRQ-assert delay vs MMC3 that is **not** modelled exactly (the MMC3 A12 counter is used as-is — close enough for every licensed game). |
| 87 | — | Jaleco/Konami CNROM-style | — | — | Argus, Choplifter, The Goonies, City Connection. Fixed PRG + one bit-swapped 8K CHR-bank register in the `$6000-$7FFF` window: `bank = ((v >> 1) & 1) \| ((v << 1) & 2)`. |
| 89 | — | Sunsoft-2 (Sunsoft-3 board) | — | — | Tenka no Goikenban: Mito Koumon. One `$8000-$FFFF` register: PRG bits 4-6, CHR `((v>>3)&1)<<3 \| (v&7)` (bit 3 = A16), bit 7 = one-screen A/B select; last 16K PRG fixed. Note: **Mito Koumon (the only iNES mapper-89 dump on hand) renders blank** — see `docs/compatibility.md`. |
| 184 | — | Sunsoft-1 | — | — | Atlantis no Nazo, The Wing of Madoola, Kid Niki. Fixed PRG + two 4K CHR banks from one `$6000-$7FFF` register: bits 0-2 @ `$0000`, bits 4-6 @ `$1000`. |

### Fourth long-tail batch — Taito X1 + arcade RGB (3 families, 48 → 51)

Three more spec-implemented licensed boards (nesdev wiki), each with register/bank
unit tests and a commercial-ROM visual survey via `coverage_smoke`. This batch
pairs with the **clean-byte arcade detection** in `rustynes-mappers::parse` (see
`docs/compatibility.md` §PlayChoice-10): a clean iNES-1.0 dump whose byte 7 is
**exactly** `0x01` (Vs.) or `0x02` (PC10) is routed through the 2C03 RGB PPU, and
mapper 151 joins mapper 99 as a mapper-driven Vs. signal.

| iNES | Submapper | Name | Audio | IRQ | Notes |
|------|-----------|------|-------|-----|-------|
| 80 | — | Taito X1-005 | — | — | Kyonshiizu 2, Kyoto Ryuu no Tera Satsujin Jiken. A `$7EF0-$7EFF` register window: two 2K CHR banks (`value & 0xFE`, each driving a pair of adjacent 1K slots) + four 1K CHR banks, two switchable 8K PRG banks (last two fixed), `$7EF6` bit-0 H/V mirroring, plus an on-cart 128-byte battery RAM at `$7F00-$7FFF` enabled only after writing `$A3` to **both** `$7EF8` and `$7EF9`. No IRQ. Note: Kyonshiizu 2 boots blank on this dump; Kyoto Ryuu renders its full title (visually verified). |
| 82 | — | Taito X1-017 | — | (decoded, unused) | Kyuukyoku Harikiri Koushien / Stadium III. Like the X1-005 plus a **CHR A12-inversion mode bit** (`$7EF6` bit 1 swaps the 2K/1K CHR halves between `$0000-$0FFF` and `$1000-$1FFF` — the non-linear X1-017 quirk), **value-shifted** registers (2K CHR banks `value >> 1`; PRG banks `$7EFA-$7EFC` `value >> 2`, ≤128K addressable), and three independently-protected 8K PRG-RAM sub-regions (`$7EF7`=`$CA`, `$7EF8`=`$69`, `$7EF9`=`$84`). The IRQ surface (`$7EFD-$7EFF`) is decoded but never clocked (the licensed games do not use it). `$7EF6` bit 0: 0 = Horizontal, 1 = Vertical. |
| 151 | — | **Konami VS (VRC1 on Vs.)** | — | — | **Vs. Gradius, GVS VS. TKO Boxing.** Konami VRC1 silicon (banking byte-identical to mapper 75: three 8K PRG banks `$8000`/`$A000`/`$C000` + fixed last; two 4K CHR windows with `$9000`-driven MSB bits; `$9000` bit 0 = H/V) on a Vs. board. Like mapper 99 it forces `ConsoleType::VsSystem` + the 2C03 RGB PPU (mapper-driven, immune to the byte-7 trap). Verified in-game via Vs. Gradius / Vs. The Goonies (both mapper 151). |

Mapper coverage was staged across Phases 1-4 (the matrix above) and extended
across the engine lineage in the long-tail batches above (all shipping in
RustyNES v1.0.0); see `to-dos/ROADMAP.md`. FDS audio shipped as the last
expansion-audio integration.

## Edge cases and gotchas

1. **MMC1 consecutive-write bug.** Writes on adjacent CPU cycles after the first are ignored. *Bill & Ted's Excellent Adventure* depends on this; a clean implementation breaks the game.
2. **MMC3 IRQ pattern-table revision differences.** MMC3A (Sharp) generates IRQ even with latch = $00; MMC3B (NEC) does not. *Star Trek: 25th Anniversary* requires MMC3A behavior. Default to MMC3A unless NES 2.0 submapper specifies MMC3B (subm. 1) or MMC3C (subm. 2).
3. **MMC2/MMC4 latch on tile fetch.** PPU calls a "tile fetched" notification with the tile address; mapper switches CHR bank if tile == `$FD` or `$FE`. Used for Punch-Out's character animations.
4. **MMC5 8x16 sprite CHR.** Two separate CHR banks for sprites (`$5120-$5127`) and BG (`$5128-$512B`). PPU must tell the mapper which fetch type is in progress. **Status (Phase 4 / S4 v1):** the PPU's sprite tile fetch path now calls `PpuBus::ppu_read_sprite`, which `LockstepBus` forwards to `Mapper::ppu_read_sprite`. The default impl forwards to `ppu_read`; MMC5 overrides it to consult the eight 1 KiB sprite-CHR bank registers. The BG fetch path is untouched. The 8x16-vs-8x8 decision is taken by the PPU; the mapper always uses sprite registers for sprite fetches, which matches the documented MMC5 behavior in 8x16 mode. (In 8x8 mode real MMC5 unifies the two bank sets via internal write mirroring; games that flip in and out of 8x16 typically rewrite the sprite registers anyway.)
5. **VRC2/4 mapping confusion.** Different VRC2/4 variants share iNES mapper IDs but route registers differently. NES 2.0 submappers disambiguate.
6. **Namco 163 N163 audio enable bit.** Disabled by default; ROM must set it. Some ROMs forget; default-on causes glitches in those.
7. **Bus conflict timing.** The bus-conflict-AND happens at the time of the write; emulators that compute the conflict after the bank-switch read get it wrong.
8. **PRG-RAM enable bit.** Some mappers (MMC1, MMC3) have a PRG-RAM enable that, when clear, causes reads to return open bus and writes to be ignored. Required for *Low G Man* music to play correctly under MMC3.
9. **NES 2.0 submapper routing.** VRC2/VRC4, MMC3 revision, BNROM/NINA,
   bus-conflict variants, and some multicarts require submapper-specific
   dispatch. Treat an iNES fallback as a compatibility guess, not proof of
   board identity.
10. **Expansion audio mix levels.** VRC6, Sunsoft 5B, Namco 163, MMC5, VRC7,
    and FDS audio use different cartridge output paths and board-dependent
    levels. Keep mapper audio behind explicit state and tests so future PAL,
    Famicom adapter, or front-loader mix options can be added without changing
    mapper banking behavior.

## Test plan

- **`holy_diver_battery_test`** / **`holy_mapperel`** (tepples): detects mappers and verifies bank reachability for each PRG/CHR bank.
- **`mmc3_test_2`** (5 sub-ROMs): MMC3 IRQ behavior including the Sharp/NEC distinction and edge cases.
- **`mmc3_irq_tests`** (blargg): MMC3 IRQ timing.
- **`vrc24test`** (AWJ): all VRC2/4 variants.
- **`AccuracyCoin`** (100thCoin): single-cartridge accuracy battery covering many mappers.
- **Per-mapper boot test**: golden-master framebuffer for the first 60 frames of attract mode of one freely-distributable ROM per mapper.
- **NES 2.0 header corpus**: mapper/submapper fixtures for revision-sensitive
  boards, including MMC3A/B/C, VRC2/VRC4 address wiring, BNROM/NINA, and
  bus-conflict-free board variants.

## Open questions

- **MMC3 default revision** when iNES (no submapper) is detected. Plan: default Sharp (MMC3A) since Star Trek requires it; expose a config override.
- **Mapper #5 (MMC5) audio scope.** Implementing the 2 extra pulse + raw PCM channels is non-trivial; defer behind a `mmc5-audio` cargo feature.
- **VRC7 FM audio.** YM2413-derived; only Lagrange Point uses it commercially. Banking + IRQ landed in Track C2 / Phase 2.4 (mapper 85; same `mapper-audio` feature flag as VRC6 / Sunsoft 5B / Namco 163 / MMC5). **The FM synthesizer landed** via a clean-room pure-Rust port of `emu2413 v1.5.9` (MIT) at `crates/rustynes-apu/src/opll.rs`; ADR 0006 (`docs/adr/0006-vrc7-audio-landed.md`) supersedes the ADR 0004 deferral. *Lagrange Point* plays with in-game audio (mixed via the `mapper-audio` slot).
- **Pirate / multicart mappers.** 60+ exist; none in initial scope. Architecture supports adding them but no commitment.

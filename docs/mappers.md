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
4. **CPU cycle counter** — VRC2/4/6/7, Sunsoft FME-7, Namco 163, plus the BestEffort M2-counter pirate boards (NTDEC 2722 / mapper 40, Nitra / mapper 250). Tick on every CPU cycle (`notify_cpu_cycle`).

### Mirroring

Most mappers expose a register that selects horizontal, vertical, single-screen A, or single-screen B. The PPU's nametable fetch consults `cart.ppu_read(addr)` for `$2000-$3FFF`; the mapper applies mirroring. Four-screen mode requires extra cart-side VRAM (typically 2 KB) on top of the console's 2 KB.

**Per-game mirroring override (v1.1.0 beta.1, T-110-B4).** The bus carries an optional `nt_mirroring_override: Option<Mirroring>` (set via `Nes::set_mirroring_override`). When `Some`, the bus's `$2000-$3EFF` nametable translation uses it instead of the mapper's `nametable_address` — a load-time correction for ROMs with a wrong iNES mirroring flag, supplied by the frontend's CRC32-keyed game database (the `rustynes_frontend::game_db` module). It does **not** affect mapper-supplied VRAM (`nametable_fetch`, e.g. four-screen), is `None` by default (byte-identical; the core test suites never set it), and is persisted in the save-state. See `docs/compatibility.md` §Input devices for the device side and the CHANGELOG for the rollout.

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
| 80 | — | Taito X1-005 | — | — | Kyonshiizu 2, Kyoto Ryuu no Tera Satsujin Jiken. A `$7EF0-$7EFF` register window: two 2K CHR banks (`value & 0xFE`, each driving a pair of adjacent 1K slots) + four 1K CHR banks, **three** switchable 8K PRG banks (`$7EFA`→`$8000`, `$7EFC`→`$A000`, `$7EFE`→`$C000`; only `$E000` is fixed to the last bank), `$7EF6` bit-0 mirroring (0 = Horizontal, 1 = Vertical), plus an on-cart 128-byte battery RAM at `$7F00-$7FFF` enabled only after writing `$A3` to **both** `$7EF8` and `$7EF9`. No IRQ. Kyonshiizu 2 renders its title screen (visually verified) — the earlier blank boot was a missing `$7EFE` `$C000` PRG register that stranded the reset bank (also: the `$7EF6` polarity was inverted). |
| 82 | — | Taito X1-017 | — | (decoded, unused) | Kyuukyoku Harikiri Koushien / Stadium III. Like the X1-005 plus a **CHR A12-inversion mode bit** (`$7EF6` bit 1 swaps the 2K/1K CHR halves between `$0000-$0FFF` and `$1000-$1FFF` — the non-linear X1-017 quirk), **value-shifted** registers (2K CHR banks `value >> 1`; PRG banks `$7EFA-$7EFC` `value >> 2`, ≤128K addressable), and three independently-protected 8K PRG-RAM sub-regions (`$7EF7`=`$CA`, `$7EF8`=`$69`, `$7EF9`=`$84`). The IRQ surface (`$7EFD-$7EFF`) is decoded but never clocked (the licensed games do not use it). `$7EF6` bit 0: 0 = Horizontal, 1 = Vertical. |
| 151 | — | **Konami VS (VRC1 on Vs.)** | — | — | **Vs. Gradius, GVS VS. TKO Boxing.** Konami VRC1 silicon (banking byte-identical to mapper 75: three 8K PRG banks `$8000`/`$A000`/`$C000` + fixed last; two 4K CHR windows with `$9000`-driven MSB bits; `$9000` bit 0 = H/V) on a Vs. board. Like mapper 99 it forces `ConsoleType::VsSystem` + the 2C03 RGB PPU (mapper-driven, immune to the byte-7 trap). Verified in-game via Vs. Gradius / Vs. The Goonies (both mapper 151). |

Mapper coverage was staged across Phases 1-4 (the matrix above) and extended
across the engine lineage in the long-tail batches above (all shipping in
RustyNES v1.0.0); see `to-dos/ROADMAP.md`. FDS audio shipped as the last
expansion-audio integration.

### Fifth long-tail batch — v1.2.0 curated (9 families, 51 → 60)

Discrete-logic boards added in `sprint5.rs`, each with register-decode unit
tests. All are **Tier-1 Curated** (see "Mapper accuracy tiering" below).

| iNES | Name | Audio | IRQ | Notes |
|------|------|-------|-----|-------|
| 38 | Bit Corp UNL-PCI556 | — | — | Crime Busters. PRG/CHR latch at `$7000-$7FFF`. |
| 41 | Caltron 6-in-1 | — | — | Outer register `$6000-$67FF` (PRG/mirroring/CHR-hi); inner CHR-low at `$8000-$FFFF` with a bus conflict, gated by the outer enable bit. |
| 79 | AVE NINA-03/06 | — | — | PRG+CHR bank via `$4100-$5FFF` (the `$4100`/`$5000` address mask). |
| 86 | Jaleco JF-13 | — | — | PRG/CHR latch in the `$6000-$7FFF` window. |
| 113 | NINA-006 / MB-91 | — | — | Like 79 plus a register-controlled mirroring bit (no header mirroring). |
| 140 | Jaleco JF-11/14 | — | — | PRG/CHR latch in the `$6000-$7FFF` window. |
| 232 | Camerica Quattro (BF9096) | — | — | Two-level (outer block + inner) 16 KiB PRG banking. |
| 240 | C&E multicart | — | — | PRG/CHR via `$4020-$5FFF` (write-only). |
| 241 | BxROM-like (pirate) | — | — | 32 KiB PRG bank via `$8000-$FFFF`; CHR-RAM. |

### Sixth long-tail batch — v1.2.0 best-effort sweep (27 families, 60 → 87)

The aggressive Tier-2 sweep, ported from the GeraNES / Mesen2 references into
`sprint6.rs` (14 boards) and `sprint7.rs` (13 boards). Mostly multicart / Sachen
/ discrete boards with no redistributable test fixture; **register-decode
unit-tested only and not accuracy-gated** (see the tiering note below).

| `sprint6.rs` | `sprint7.rs` |
|---|---|
| 15 (K-1029 multicart), 36 (TXC 01-22000), 39 (Subor BNROM-like), 61, 62 (multicart), 72 / 92 (Jaleco JF-17/19), 77 (Irem, 4-screen CHR-RAM), 96 (Bandai Oeka Kids, PPU-bus CHR latch), 97 (Irem TAM-S1), 132 (TXC 22211), 133 / 145 / 146 (Sachen) | 147 (Sachen 3018), 148 / 149 (Sachen), 150 (Sachen SA-015, readable protection + custom mirroring), 180 (Nichibutsu UNROM-inverted), 185 (CNROM CHR-disable protection), 200 / 201 / 202 / 203 / 212 / 213 / 214 (multicart) |

### Seventh long-tail batch — v1.3.0 "Bedrock" best-effort sweep (14 families, 87 → 101)

The v1.3.0 Workstream D1 Tier-2 sweep, ported from the GeraNES reference into
`sprint8.rs`. Simple discrete / homebrew / multicart boards with no IRQ, no
on-cart audio, and no per-cycle / A12 hook (`MapperCaps::NONE`); **register-decode
unit-tested only and not accuracy-gated** (see the tiering note below).

| iNES | Submapper | Name | Audio | IRQ | Status | Notes |
|------|-----------|------|-------|-----|--------|-------|
| 29 | — | Sealie RET-CUFROM | — | — | landed (v1.3.0 / S8) | Homebrew. 16K PRG (data bits 4-2) + 8K CHR-RAM bank (data bits 1-0); fixed last PRG bank at `$C000`. |
| 31 | — | INL NSF-style (2A03 Puritans) | — | — | landed (v1.3.0 / S8) | Eight 4K PRG slots latched at `$5FF8-$5FFF`; CHR-RAM; power-on fixes the `$F000` slot to the last bank. |
| 58 | — | Multicart | — | — | landed (v1.3.0 / S8) | Address-decoded PRG (16/32K mode) + CHR + mirroring bit; data byte ignored. |
| 60 | — | Reset-based 4-in-1 multicart | — | — | landed (v1.3.0 / S8) | Power-on bank only modelled (reset-latch game selection is host-driven, not exercised in the no_std core). |
| 94 | — | UN1ROM (Senjou no Ookami) | — | — | landed (v1.3.0 / S8) | 16K PRG bank (data bits 4-2, bus conflict) + fixed last bank at `$C000`; CHR-RAM. |
| 101 | — | Jaleco JF-10 CHR latch | — | — | landed (v1.3.0 / S8) | Fixed 32K PRG; 8K CHR bank latched via a write to the `$6000-$7FFF` window. |
| 107 | — | Magic Dragon | — | — | landed (v1.3.0 / S8) | One `$8000-$FFFF` latch: 32K PRG = data>>1, 8K CHR = data. |
| 111 | — | GTROM / Cheapocabra | — | — | landed (v1.3.0 / S8) | Homebrew. 32K PRG + 16K CHR-RAM (two 8K banks) + 4-screen nametable RAM with a bank-select bit; LED bit ignored. |
| 143 | — | Sachen TCA01 | — | — | landed (v1.3.0 / S8) | NROM-128 (mirrored) + a simple protection read at `$4020-$5FFF` returning `(~addr & 0x3F) \| 0x40`. |
| 177 | — | Hengedianzi | — | — | landed (v1.3.0 / S8) | 32K PRG + mirroring bit (bit 5) from one `$8000-$FFFF` latch; CHR-RAM. |
| 179 | — | Hengedianzi variant | — | — | landed (v1.3.0 / S8) | 32K PRG via `$5000-$5FFF` (data>>1) + mirroring bit (bit 0) via `$8000-$FFFF`; CHR-RAM. |
| 218 | — | Magic Floor | — | — | landed (v1.3.0 / S8) | No PRG/CHR-ROM banking; the pattern table is served from the console CIRAM under a fixed custom mirroring mode. |
| 231 | — | 20-in-1 multicart | — | — | landed (v1.3.0 / S8) | Address-decoded dual 16K PRG banks + a mirroring bit; CHR-RAM. |
| 234 | — | Maxi 15 / BNROM-like multicart | — | — | landed (v1.3.0 / S8) | Two latch regs (`$FF80-$FF9F` / `$FFE8-$FFF8`) selecting 32K PRG + 8K CHR in NINA-style or CNROM-style sub-mode. |

### Eighth long-tail batch — v1.4.0 "Fidelity" best-effort sweep (12 families, 101 → 113)

The v1.4.0 Workstream G Tier-2 sweep, ported into `sprint9.rs` from the
concretely-documented nesdev decode tables (and the `Mesen2` / `GeraNES`
reference implementations). Simple discrete / homebrew / multicart boards with
no IRQ, no on-cart audio, and no per-cycle / A12 hook (`MapperCaps::NONE`);
**register-decode unit-tested only and not accuracy-gated** (see the tiering
note below).

| iNES | Submapper | Name | Audio | IRQ | Status | Notes |
|------|-----------|------|-------|-----|--------|-------|
| 28 | — | Action 53 homebrew multicart | — | — | landed (v1.4.0 / S9) | Outer `$5xxx` register-select + inner `$8000-$FFFF` bank latch; 2-bit PRG-mode field (NROM-128/256/UNROM) + 2-bit mirroring field; CHR-RAM. |
| 30 | — | UNROM-512 | — | — | landed (v1.4.0 / S9) | Homebrew. Latch `[N CC P PPPP]`: 16K PRG (bits 0-4) + 8K CHR-RAM/ROM (bits 5-6) + nametable bit (bit 7); fixed last bank at `$C000`. Bus-conflict / flash wiring keyed off submapper + battery (sub 0 w/o battery or sub 2 = bus conflicts on `$8000-$FFFF`; sub 0 w/ battery or sub 1/3/4 = no conflicts, latch only on `$C000-$FFFF`, `$8000-$BFFF` = flash window). |
| 63 | — | NTDEC 0324 (Powerful 250-in-1) | — | — | landed (v1.4.0 / S9) | Address-decoded multicart: 16/32K PRG bank + mirroring bit; CHR-RAM. |
| 76 | — | NAMCOT-3446 (Namco 109) | — | — | landed (v1.4.0 / S9) | MMC3-style `$8000`/`$8001` register pairs select two 8K PRG banks (fixed last two) + four 2K CHR banks; header-fixed mirroring. |
| 174 | — | NTDEC 5-in-1 | — | — | landed (v1.4.0 / S9) | Address-decoded 16/32K PRG bank + 8K CHR bank + mirroring bit. |
| 225 | — | ColorDreams 72-in-1 | — | — | landed (v1.4.0 / S9) | Address-decoded `A~[.HMO PPPP PPCC CCCC]`: CHR A0-A5, PRG A6-A11, mode A12 (16/32K), mirror A13, high bit A14; plus a `$5800-$5FFF` 4-nibble scratch-RAM block. |
| 226 | — | 76-in-1 BMC | — | — | landed (v1.4.0 / S9) | Two `$8000-$FFFF` regs (even/odd): reg0 `[PMOP PPPP]` (bit6 mode 0=32K/1=16K, bit7 mirror 0=H/1=V), reg1 bit0 = high PRG bit; CHR-RAM. |
| 227 | — | 1200-in-1 BMC | — | — | landed (v1.4.0 / S9) | Address-decoded 16/32K PRG + fixed-high-bank mode + mirroring bit; CHR-RAM. |
| 229 | — | 31-in-1 BMC | — | — | landed (v1.4.0 / S9) | Address-decoded: low bits zero = fixed NROM-32 menu bank, else a 16K bank pair + 8K CHR + mirroring bit. |
| 233 | — | 42-in-1 reset-based BMC | — | — | landed (v1.4.0 / S9) | DATA-driven `[MMOP PPPP]` (4-bit page, bit5 mode 0=16K/1=32K, bits6-7 mirroring); the reset-selected outer block is host-driven (fixed power-on `0`); CHR-RAM. |
| 242 | — | Waixing 43-in-1 (Wai Xing Zhan Shi) | — | — | landed (v1.4.0 / S9) | `$8000-$FFFF` address-decoded 32K PRG (inner = A2-A4, outer = A5-A6) + mirror bit (A1); 8K work-RAM at `$6000-$7FFF`; CHR-RAM. |
| 246 | — | Fong Shen Bang / G0151-1 | — | — | landed (v1.4.0 / S9) | Four `$6000-$6003` PRG (8K) + four `$6004-$6007` CHR (2K) banking regs; 2K PRG-RAM at `$6800-$6FFF`; `$6003` powers on to `$FF` and `$FFE4-$FFFF`-family reads force PRG A17 high; CHR-ROM, header-fixed mirroring. |

These were boot-smoked against real unlicensed / pirate / multicart dumps (10 of
the 12 families have a library dump; 28 + 174 do not and are register-decode +
save-state tested only). The boot-smoke caught a shared `cpu_read_unmapped`
inversion (it had also been latent in the pre-existing m132 + m143) that
open-bused the whole PRG window so the board never booted, plus several decode
errors (m225/m226/m233/m242/m246) — all corrected. 7 of the 10 staged dumps now
render a real screen headless; the other 3 (30/63/233) boot + run real menu code
but are input-/reset-gated. See `screenshots/besteffort/README.md` for the full
matrix and the per-mapper fix log.

### Ninth long-tail batch — v1.5.0 "Lens" best-effort sweep (10 families, 113 → 123)

The v1.5.0 Workstream F Tier-2 sweep, ported into `sprint10.rs` from the
concretely-documented nesdev decode tables (and the `Mesen2` / `GeraNES` /
`puNES` reference implementations). Small pirate / unlicensed / multicart
boards; eight are hook-free (`MapperCaps::NONE`) and two carry a simple
CPU-cycle (M2) IRQ (`MapperCaps::CYCLE_IRQ`, m40 + m250 — no A12 hook).
**Register-decode + save-state unit-tested only and not accuracy-gated** (see
the tiering note below).

| iNES | Submapper | Name | Audio | IRQ | Status | Notes |
|------|-----------|------|-------|-----|--------|-------|
| 40 | — | NTDEC 2722 (*SMB2J* pirate) | — | M2 cycle | landed (v1.5.0 / S10) | Fixed PRG layout (`$8000`/`$A000`/`$E000` = banks 4/5/7) with one switchable 8K window at `$C000`, selected by an `$E000` write (bits 0-2); 12-bit M2 IRQ that arms on `$A000`, asserts at 4096, and disables/acks on `$8000`; CHR-RAM. |
| 81 | — | NTDEC Super Gun | — | — | landed (v1.5.0 / S10) | CNROM-like single `$8000-$FFFF` register: 16K PRG (bits 2-3, `$C000` half fixed last) + 8K CHR (bits 0-1); header mirroring. |
| 95 | — | NAMCOT-3425 (*Dragon Buster*) | — | — | landed (v1.5.0 / S10) | MMC3-subset `$8000`/`$8001` register port (no A12 IRQ); CHR reg-0 bit 5 drives one-screen mirroring select; CHR-ROM. |
| 112 | — | NTDEC ASDER / Huang-1 | — | — | landed (v1.5.0 / S10) | Indexed `$8000`(idx)/`$A000`(data) port (no A12 IRQ) for two 8K PRG + 2K/1K CHR slots; `$E000` bit 0 = mirroring; `$C000`/`$E000` PRG fixed last two. |
| 137 | — | Sachen 8259D | — | — | landed (v1.5.0 / S10) | `$4100`(cmd)/`$4101`(data) protection board: 32K fixed PRG select (cmd 5) + four 2K CHR banks (cmds 0-3) + CHR outer (cmd 4) + mirroring (cmd 7). |
| 156 | — | DIS23C01 DAOU (Open Corp) | — | — | landed (v1.5.0 / S10; decode corrected in the coverage pass) | CHR-nibble registers `$C000-$C00F` decode the 1K slot as `(addr&0x03)+(addr>=0xC008?4:0)` with bit 2 selecting the high/low nibble array; `$C010` = 16K PRG; `$C014` = H/V mirroring from a single-screen-A power-on (Mesen2 `DaouInfosys`). |
| 162 | — | Waixing FS304 (*San Guo Zhi II*) | — | — | landed (v1.5.0 / S10; decode corrected in the coverage pass) | PRG bank composed from individual A15-A20 bits across `$5000`/`$5100`/`$5200` with a `$5300` mode selector (NESdev table; reset boots 32K bank #2); 8K battery PRG-RAM at `$6000-$7FFF`; 8K CHR-RAM; header mirroring. |
| 178 | — | Waixing educational series (FS305) | — | — | landed (v1.5.0 / S10; decode corrected in the coverage pass) | `$4800` bit 0 = mirroring, bits 1-2 = PRG mode (NROM-256/BNROM, UNROM, NROM-128, UNROM-variant); 16K bank = `(reg2<<3)\|(reg1&0x07)`; 8K work-RAM at `$6000`; CHR-RAM. |
| 244 | — | Decathlon (Mega Soft) | — | — | landed (v1.5.0 / S10; decode corrected in the coverage pass) | Data-decoded multicart: the written DATA byte selects through two scramble LUTs with bit 3 choosing CHR (`LUT_CHR[(v>>4)&7][v&7]`) vs PRG (`LUT_PRG[(v>>4)&3][v&3]`); CHR-ROM, header mirroring (Mesen2/puNES). |
| 250 | — | Nitra (*Time Diver Avenger*) | — | M2 cycle | landed (v1.5.0 / S10; decode corrected in the coverage pass) | MMC3-register-compatible, but the register data is carried in address bits A0-A7 and the even/odd line in **A10** (`addr & 0x0400`, Mesen2 `MMC3_250`); MMC3 banking subset + an M2-clocked 8-bit reload IRQ counter; CHR-ROM. |

These are register-decode + save-state unit-tested only (no redistributable
fixture is committed), and structurally excluded from the AccuracyCoin / oracle
gate by the BestEffort tier classifier.

### Per-mapper screenshot-coverage decode pass

A boot-coverage pass (the auto-discovering `external_coverage` harness +
per-mapper commercial dumps) surfaced a cluster of BestEffort boards that booted
to a blank/few-colour frame. Cross-checking each against puNES / Mesen2 / the
NESdev wiki corrected real decode bugs in **m143, m147, m150, m156, m162, m177,
m178, m185, m227, m233, m244, m250** (details in `CHANGELOG.md`); each now renders a
real screen for the staged dumps. All are BestEffort (off the AccuracyCoin
oracle), so AccuracyCoin holds 100% (139/139) and the `mapper_tier_honesty`
gate stays green. A handful of titles remain blank and are documented
follow-ups: 4 of the 5 m162 FS304 RPGs need the `$5000.7` CHR auto-switch (the
core decode is proven by *The Mummy* rendering); m036 TXC needs a proper TXC-
chip port (`$4000-$4FFF & 0x200` register window) rather than the flat decode;
m040 / m063 / m111 / m202; and 2 m227 pirate hacks need the m227-hack `$6000`
WRAM. Vs. System DualSystem games (Balloon Fight / Mahjong / Tennis / Wrecking
Crew) stay blank by design on this single-system core.

A later boot-coverage pass cleared three more blanks. **m030 UNROM-512** (Wampus,
PROTO DERE .NES) booted blank because the board unconditionally applied bus
conflicts; the self-flashing carts set the iNES battery bit, which on submapper
0 means *no* bus conflicts (and the banking latch responds only to
`$C000-$FFFF`, with `$8000-$BFFF` the flash window) — both now render gameplay.
**m080 Taito X1-005** (Kyonshiizu 2) was missing the `$7EFE` `$C000` PRG
register (only two of three switchable PRG banks were modelled), stranding the
reset bank; with all three banks it renders its title screen. **m185 Seicross**
(CRC `0F05FF0A`) is a CHR-disable copy-protection title that loops forever unless
CHR reads back as *disabled* for its protection latch (`$21`); its GoodNES dump
is iNES-1.0 mapper 185 submapper 0, but it is really submapper 4 (enabled iff
the latch low bits are `0`). The fix is a frontend per-game DB submapper
correction (`game_database.txt`, applied by `apply_header_overrides`, which now
promotes an iNES-1.0 header to NES 2.0 when a non-zero submapper override is
set) — the mapper's existing submapper-4 rule already matches FCEUX `Sync181` /
BizHawk's Seicross special-case, so the core is untouched. The `external_coverage`
boot-smoke feeds raw bytes to `Nes::from_rom` and bypasses the frontend DB, so
Seicross still captures blank there (a harness limitation, not a decode bug); the
three converted Waixing `.WXN` dumps under `mapper-030-` are actually Waixing
FS005 (iNES mapper 176 submapper 2) misdetected as mapper 30 by GoodNES, so they
need mapper-176 support + re-staging rather than any mapper-30 change.

### Mapper accuracy tiering (v1.2.0)

Every supported family is classified `Core` / `Curated` / `BestEffort` by
`rustynes-mappers::mapper_tier(id, submapper)` — an **honesty marker** (runtime
behaviour is identical) that keeps the accuracy claim precise as long-tail
coverage grows. `Core` (the original 51) and `Curated` are gated by the
AccuracyCoin / commercial-ROM oracle suites; `BestEffort` (reference-ported
boards with no redistributable fixture, register-decode unit-tested only) is
**excluded** from that gate. The invariant — no `BestEffort` mapper backs an
oracle ROM — is enforced at the classifier level (`BestEffort` is structurally
never accuracy-gated; the three tier id-sets are disjoint) and by the curated
construction of the byte-oracle corpus. See `docs/adr/0011-mapper-tiering.md`.
Current split: **168 families** — 51 Core + 9 Curated (60 accuracy-gated) + 108
BestEffort (27 from v1.2.0 `sprint6`/`sprint7` + 14 from v1.3.0 `sprint8` + 12
from v1.4.0 `sprint9` + 10 from v1.5.0 `sprint10` + the v1.6.0 "Studio"
J.Y. Company ASIC `jy_asic` family 35/90/209/211 + 23 from v1.6.0 "Studio"
Workstream E `sprint11` + 18 from v1.7.0 "Forge" Workstream G1 `sprint12`). The
v1.6.0 `sprint11` batch ports MMC3-clone variants
(44/49/52/115/134/189/205/238/245/348/366, on a shared MMC3-style core with an
A12 falling-edge IRQ + per-board outer-bank transform), the Sachen 8259 A/B/C
2 KiB-CHR variants (141/138/139 — siblings of the existing 8259D mapper 137),
and discrete unlicensed / FDS-conversion / multicart boards
(42/50 with CPU-cycle IRQs, 46/51/57/104/120/290/301 hook-free). Mapper 35 is
the J.Y. Company single-game "extended" board folded into `jy_asic.rs` (same
silicon as 209). The v1.7.0 `sprint12` batch ports the next reusable-ASIC
BMC/pirate cores: the Waixing **FK23C** 8/16 Mbit BMC (176, `$5000` config +
MMC3 surface + A12 IRQ), **COOLBOY / MINDKIDS** (268, MMC3 + four `$6000` outer
registers), Sachen **9602** (513, MMC3 + PRG-A19/A20 outer) and **3011** (136,
the TXC protection accumulator driving an 8 KiB CHR select), Waixing **164**
(split `$5000`/`$5100` PRG), **253** (*Dragon Ball Z* VRC4-clone, per-1 KiB CHR
regs + CHR-RAM escape + scaled CPU-cycle IRQ) and **286** (BS-5 DIP-gated
multicart), the **Kaiser** FDS-conversion family (56/142 KS202/KS7032 with an
up-counting M2 IRQ, 303 KS7017 with a down-counting M2 IRQ + read-ack, and the
window boards 305/306/312), and the BMC multicarts **261**/**289**/**320**/
**336**/**349**. All are BestEffort: register-decode + save-state round-trip
unit-tested, outside the AccuracyCoin / oracle gate.

### NSF player (synthetic mapper, v1.1.0)

NSF chiptune files are not cartridges — they have no iNES header and no PPU
program. They are played through a synthetic `NsfMapper` (`nsf.rs`), built by the
dedicated `Nes::from_nsf` path (not `parse`). The mapper serves the program image
(`$8000-$FFFF`, with `$5FF8-$5FFF` 4 KiB bank-switching), 8 KiB WRAM at `$6000`,
and a tiny hand-assembled 6502 **driver** at `$5000`; the reset/NMI/IRQ vectors
(`$FFFA-$FFFF`) are overridden to point at the driver. Reset runs `init` for the
selected song and enables vblank NMI; the ordinary 60 Hz NMI then calls `play`
each frame. Because this reuses the normal lockstep `run_frame`, the APU produces
audio identically to a cartridge and the determinism contract is untouched. The
`Mapper` trait carries three default-no-op `nsf_*` hooks (song count / current /
set) so the bus + `Nes` can drive track selection without downcasting. Scope: base
2A03, NTSC 60 Hz; expansion-chip audio + non-60 Hz rates + NSFe are deferred.

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

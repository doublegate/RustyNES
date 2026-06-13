# Compatibility

What runs, what doesn't, and where the rough edges are.

## ROM file formats

| Format | Status | Notes |
|--------|--------|-------|
| iNES 1.0 (`.nes`) | Supported | Region defaults to NTSC (iNES 1.0 has no reliable region byte) |
| NES 2.0 (`.nes`) | Supported | Region byte respected; submapper byte respected for MMC1 / MMC3 / VRC2 / VRC4 / Mapper 34 / Mapper 71 |
| UNIF (`.unf`) | Not supported | Effectively obsolete in modern dumps |
| FDS (`.fds`) | Supported | Famicom Disk System — real-BIOS boot (user-supplied `disksys.rom`), read/write drive, multi-side, 2C33 audio |

A file is identified as NES 2.0 when header byte 7 bits 2-3 equal `0b10`.
Otherwise it's parsed as iNES 1.0, and a few fields fall back to defaults
documented in `docs/cartridge-format.md`.

## Supported mappers

iNES mapper numbers handled (51 mapper families; the table below lists the
most common; the most popular set covers well over 90% of the licensed
library):

| iNES # | Name | Notable games |
|--------|------|---------------|
| 0 | NROM | Super Mario Bros., Donkey Kong, Excitebike, Galaga, Ice Climber |
| 1 | MMC1 | The Legend of Zelda, Metroid, Mega Man 2, Final Fantasy, Tetris |
| 2 | UxROM | Castlevania, Contra, DuckTales, Mega Man, Metal Gear |
| 3 | CNROM | Adventure Island, Arkanoid, Friday the 13th, Gradius |
| 4 | MMC3 | Mega Man 3-6, Super Mario Bros. 2 & 3, Kirby's Adventure, Battletoads |
| 5 | MMC5 | Castlevania III (US/PAL), Just Breed, Laser Invasion, Uchuu Keibitai SDF |
| 7 | AxROM | Battletoads (alt board), Marble Madness, R.C. Pro-Am, Wizards & Warriors |
| 9 | MMC2 | Punch-Out!! |
| 10 | MMC4 | Famicom Wars, Fire Emblem |
| 11 | Color Dreams | Crystal Mines, Menace Beach |
| 13 | CPROM | Videomation |
| 19 | Namco 163 | Several Japanese RPGs (Final Lap, Erika to Satoru no Yume Bouken) |
| 21 / 23 / 25 | VRC4 (and VRC2 b/c variants on shared IDs) | Wai Wai World 2, Akumajou Special, Ganbare Goemon series |
| 22 | VRC2a | Ganbare Goemon Gaiden |
| 24 / 26 | VRC6 | Akumajou Densetsu (JP Castlevania III), Madara, Esper Dream 2 |
| 34 | BNROM / NINA-001 | Deadly Towers (BNROM), Impossible Mission II (NINA-001) — limited automated test coverage; report issues if you hit them |
| 66 | GxROM | Doraemon, Dragon Power, Super Mario Bros. + Duck Hunt |
| 69 | Sunsoft FME-7 | Batman: Return of the Joker, Gimmick!, Hebereke |
| 71 | Camerica BF9093 | Bee 52, Big Nose Freaks Out, Linus Spacehead, MiG 29 Soviet Fighter |
| 75 | VRC1 | Ganbare Goemon!, Tetsuwan Atom |

If your ROM uses a mapper not in this table, the emulator exits at load
time with an `UnsupportedMapper(N)` error.

### Mapper-specific notes

- **MMC3** defaults to the **Sharp (MMC3A)** revision when the iNES
  header doesn't specify a submapper. This is required for Star Trek:
  25th Anniversary to work correctly. NES 2.0 submapper 1 selects
  NEC (MMC3B), submapper 2 stays Sharp, submapper 3 (MC-ACC clone) is
  treated as Sharp.
- **MMC5** supports banking, ExRAM modes 1 / 2 / 3, scanline IRQ,
  dual sprite/BG CHR for 8x16 sprites, 4-byte fill mode, ExGrafix
  extended attributes, and vertical split-screen. The MMC5 expansion
  audio (`$5000-$5015`) is supported (`mapper-audio` feature, default on).
- **VRC2 / VRC4** are dispatched through a shared implementation
  modelling the VRC4 superset. The submapper byte selects the
  pin-decoder variant. VRC2's missing IRQ counter is simply left idle.

## Mapper audio expansions

| Mapper audio | Status | Notes |
|--------------|--------|-------|
| VRC6 (3 channels: 2 pulse + sawtooth) | Supported | Akumajou Densetsu (Castlevania III JP) and friends |
| Sunsoft 5B (3 channels) | Supported | Gimmick! |
| Namco 163 (1-8 channels) | Supported | Several Japanese RPGs |
| MMC5 (2 pulse + raw PCM) | Supported (`mapper-audio`, default on) | Castlevania III JP, Just Breed, Laser Invasion |
| VRC7 (FM, 6 channels) | Supported | Lagrange Point — clean-room emu2413 OPLL port |
| FDS (wavetable + envelope) | Not supported (with FDS) | |

## Regions

| Region | Status | Frame rate |
|--------|--------|------------|
| NTSC | First-class | 60.0988 Hz |
| PAL | First-class | 50.0070 Hz |
| Dendy | First-class | 50.0070 Hz |

The CPU, PPU, and APU have separate timing tables per region; you should
get correct gameplay speed, correct audio pitch, and the right scanline
count for any of the three. See [Display and audio](./display-and-audio.md)
for how the region is determined.

## Accuracy

RustyNES clears the headline accuracy bar in full: the kevtris
**AccuracyCoin** suite at **100% (139/139)**, **nestest** with zero
golden-log diff over 8,991 instructions, and the entire blargg
`instr_test_v5`, `instr_misc`, `instr_timing`, `cpu_timing_test6`,
`cpu_interrupts_v2`, `ppu_open_bus`, `ppu_vbl_nmi`, `apu_test`,
`apu_mixer`, and `dmc_dma_during_read4` corpora, plus `mmc3_irq_tests`
and the kevtris `mmc3_test_2` sub-ROMs. A 60-ROM commercial-ROM oracle
and a 52-entry extended oracle are tracked byte-identically as
regression gates (the ROMs themselves are user-supplied, never shipped).

This accuracy was developed across the upstream emulation engine's
v2.0–v2.8 lineage — the cycle-accurate master-clock scheduler, the
unified DMA engine, and the cpu_interrupts_v2 / MMC3-IRQ closures — and
ships here at v1.0.0.

### Remaining edge cases

One kevtris sub-test, `mmc3_test_2/4-scanline_timing` sub-test #3
("Scanline 0 IRQ should occur sooner when $2000=$08"), is a known
1-PPU-clock bracket on the MMC3 A12-to-IRQ discriminator. It is not
known to affect any commercial game, and the AccuracyCoin battery (which
exercises the same surface) passes. The full diagnosis lives in the
project's developer documentation.

If you find a game that misbehaves, please file an issue with the
exact ROM (sha256), the symptom, and ideally a save state at the
problem point — see the [GitHub issue
tracker](https://github.com/doublegate/RustyNES/issues).

## Other features and limitations

- **USB gamepads supported** — `gilrs` auto-binds an Xbox-style layout
  (South=A, West=B, Start, Back=Select, D-Pad); a second/third/fourth pad
  auto-binds to Players 2/3/4. Fully rebindable in the in-app input UI.
- **Recent-files menu** — **File → Open Recent** keeps an MRU list (up to
  10, persisted); you can also open a ROM via the file dialog (`F12`),
  drag-and-drop, or the command line.
- **Fullscreen** — `F11` (or **View → Fullscreen**) toggles borderless
  fullscreen; `Esc` leaves it. **View → Window Size** scales the game to
  1x–4x.
- **Up to four players** — Players 1 & 2 are standard; Players 3 & 4 are
  supported via the **Four Score** adapter (toggle it in the input modal;
  off by default). All four are keyboard- and gamepad-rebindable.
- **Vs. System + PlayChoice-10** — supported with RGB-PPU palettes;
  insert a Vs. coin with `F10`.
- **Rollback netplay** — 2–4 player GGPO-style rollback over UDP (native)
  and WebRTC (browser).
- **Movie recording (TAS)** — `.rnm` record/playback with save-state
  branching (`F6`/`F7`/`F8`).
- **RetroAchievements** — opt-in and native-only (built with the
  `retroachievements` feature).
- **Cheat support** — Game Genie codes and GameShark-style raw RAM cheats,
  both in the **Tools → Cheats…** panel, persisted per-ROM.
- **Mid-game ROM swap** — open a different ROM at any time via
  **File → Open ROM…** (`F12`), Open Recent, or drag-and-drop.

## See also

- [Troubleshooting](./troubleshooting.md) — what to do when a specific game misbehaves
- [Display and audio](./display-and-audio.md) — region detection details
- [Save states and rewind](./save-states-and-rewind.md) — slot file format compatibility

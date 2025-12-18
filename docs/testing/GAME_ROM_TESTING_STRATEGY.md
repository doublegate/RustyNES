# NES Game ROM Testing Strategy

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** Practical game ROM validation strategy for RustyNES development milestones

---

## Table of Contents

- [Introduction](#introduction)
- [Testing Philosophy](#testing-philosophy)
- [Testing Tiers by Development Milestone](#testing-tiers-by-development-milestone)
  - [Tier 1: MVP / Basic (NROM)](#tier-1-mvp--basic-nrom)
  - [Tier 2: Core Mappers](#tier-2-core-mappers)
  - [Tier 3: Advanced Mappers](#tier-3-advanced-mappers)
  - [Tier 4: Edge Cases & Quirks](#tier-4-edge-cases--quirks)
  - [Tier 5: Expansion Audio](#tier-5-expansion-audio)
- [Testing Checkpoints](#testing-checkpoints)
- [Recommended Test Suite](#recommended-test-suite)
- [Per-Game Testing Matrix](#per-game-testing-matrix)
- [Testing Workflow](#testing-workflow)
- [References](#references)

---

## Introduction

### Purpose of Game ROM Testing

While **test ROMs** (nestest, blargg suite, etc.) validate low-level accuracy, **commercial game ROMs** provide:

1. **User-facing validation** - Real games represent actual user experience
2. **Integration testing** - Complex CPU+PPU+APU+Mapper interactions
3. **Edge case discovery** - Games exploit hardware quirks test ROMs may miss
4. **Compatibility milestones** - Measurable progress (e.g., "80% of games playable")
5. **Regression detection** - Known-good games catch regressions early

### Relationship to Test ROMs

**Test ROMs** provide:
- Precise diagnostic information
- Isolated component testing
- Pass/fail automation
- Development-focused validation

**Game ROMs** provide:
- End-to-end validation
- User experience verification
- Compatibility metrics
- Real-world workload testing

**Best Practice**: Use both in tandem
1. **Test ROMs first** - Fix fundamental accuracy issues
2. **Game ROMs next** - Validate real-world behavior
3. **Iterate** - Game failures inform which test ROMs to revisit

See [TEST_ROM_GUIDE.md](TEST_ROM_GUIDE.md) for comprehensive test ROM documentation.

---

## Testing Philosophy

### Validation Levels

**What "Passing" Means:**

| Level | Description | Criteria |
|-------|-------------|----------|
| **L0: Loads** | ROM file parses correctly | No crash on load, valid header |
| **L1: Boots** | Reaches title screen | Title screen renders, music plays |
| **L2: Playable** | First level/area functional | Player can move, interact, reach level end |
| **L3: Completable** | Full playthrough possible | Can beat game without game-breaking bugs |
| **L4: Accurate** | Pixel/cycle-perfect match | No glitches, timing issues, or audio artifacts |

### Progressive Validation

Start with **L1** (boots) for all games in a tier, then progressively target **L2-L4** as accuracy improves.

**Example Progression:**
```
Week 1:  10 Tier 1 games boot (L1)
Week 2:  10 Tier 1 games playable (L2)
Week 4:  5 Tier 1 games completable (L3)
Week 8:  All Tier 1 games accurate (L4)
```

### Audio Accuracy Assessment

Audio is harder to objectively test than video. Use comparative listening:

1. **Reference recording** - Hardware or highly accurate emulator (Mesen2)
2. **Side-by-side comparison** - Play same section in reference vs RustyNES
3. **Check for** - Pitch accuracy, channel mixing, timing, no pops/clicks
4. **Tools** - Spectral analysis (Audacity) for precise comparison

---

## Testing Tiers by Development Milestone

### Tier 1: MVP / Basic (NROM)

**Development Phase:** Phase 1 (Months 1-6)
**Target:** Mapper 0 (NROM) games
**Coverage:** ~9.5% of NES library
**Validation Level:** L2 (Playable) minimum, L3 (Completable) goal

#### Why Start Here

- **No banking logic** - Simplest mapper, baseline functionality
- **Fixed memory layout** - 32KB PRG, 8KB CHR
- **Well-documented** - Easiest to debug
- **Classic games** - Iconic titles for demonstrating progress

#### Recommended Games (15 games)

| Game | Year | Publisher | What It Tests | Notes |
|------|------|-----------|---------------|-------|
| **Super Mario Bros.** | 1985 | Nintendo | Sprite 0 hit (status bar), scrolling, sound | Industry standard test |
| **Donkey Kong** | 1983 | Nintendo | Basic rendering, ladders, gravity | Simple baseline |
| **Ice Climber** | 1985 | Nintendo | Vertical scrolling, 2-player | Tests dual controller input |
| **Excitebike** | 1985 | Nintendo | Horizontal scrolling, DMC audio | Track editor tests RAM |
| **Balloon Fight** | 1986 | Nintendo | Physics, sprite collision | Water level tests palette |
| **Clu Clu Land** | 1985 | Nintendo | 4-directional scrolling | Unusual movement mechanics |
| **Wrecking Crew** | 1985 | Nintendo | Level editor, object interaction | Tests complex logic |
| **Pinball** | 1985 | Nintendo | Physics simulation | Heavy sprite usage |
| **Duck Hunt** | 1985 | Nintendo | Zapper input, timing | Light gun support (optional) |
| **Hogan's Alley** | 1985 | Nintendo | Zapper input | Alternative light gun test |
| **Popeye** | 1983 | Nintendo | Basic platforming | Simple sprite mechanics |
| **Tennis** | 1984 | Nintendo | 2-player sports | Tests multiplayer |
| **Golf** | 1985 | Nintendo | Power meter timing | Precise input timing |
| **Baseball** | 1985 | Nintendo | Complex game state | Multiple game modes |
| **Kung Fu** | 1985 | Nintendo | Side-scrolling action | Enemy AI patterns |

#### PPU Features Exercised

- **Scrolling:** Horizontal (SMB), vertical (Ice Climber), minimal (Donkey Kong)
- **Sprite 0 hit:** Super Mario Bros. status bar split
- **Sprite limits:** Balloon Fight (many sprites), Pinball (sprite overflow)
- **Palettes:** Standard usage, water effects (Balloon Fight)

#### APU Features Exercised

- **Pulse channels:** Melody lines (all games)
- **Triangle:** Bass lines (SMB, Excitebike)
- **Noise:** Percussion, sound effects
- **DMC:** Excitebike (voice samples)

#### Known Challenges

- **Super Mario Bros.**: Sprite 0 hit must be cycle-accurate or status bar shakes
- **Excitebike**: DMC samples require proper timing
- **Ice Climber**: Vertical scrolling edge cases

#### Success Criteria

- [ ] All 15 games boot to title screen (L1)
- [ ] 12+ games playable through first level (L2)
- [ ] 8+ games completable (L3)
- [ ] Super Mario Bros. World 1-1 pixel-perfect (L4)

---

### Tier 2: Core Mappers

**Development Phase:** Phase 1 (Months 4-6)
**Target:** Mappers 1, 2, 3, 4
**Coverage:** +70% (cumulative ~80%)
**Validation Level:** L2 minimum

#### Mapper 1 (MMC1 / SxROM) - 27.9% of games

**Technical Characteristics:**
- Serial write interface (5 writes to register)
- PRG banking: 16KB switchable + 16KB fixed, or 32KB switchable
- CHR banking: 4KB or 8KB
- Mirroring control: H/V/One-screen
- Battery-backed SRAM common

**Recommended Games (10 games):**

| Game | Year | Publisher | What It Tests | Difficulty |
|------|------|-----------|---------------|------------|
| **The Legend of Zelda** | 1987 | Nintendo | Save RAM, complex scrolling, multi-directional | Medium |
| **Metroid** | 1986 | Nintendo | Vertical scrolling, password system, large world | Medium |
| **Mega Man 2** | 1989 | Capcom | Tight timing, weapon switching, precise controls | Hard |
| **Castlevania II** | 1988 | Konami | Day/night cycle, RPG elements, multiple areas | Medium |
| **Kid Icarus** | 1987 | Nintendo | Vertical scrolling, shops, save system | Medium |
| **Zelda II: Adventure of Link** | 1988 | Nintendo | Side-view + overworld, experience system | Hard |
| **Blaster Master** | 1988 | Sunsoft | Vehicle + on-foot, complex maps | Medium |
| **Final Fantasy** | 1990 | Square | Battery save, complex menus, turn-based | Medium |
| **Dragon Warrior** | 1989 | Enix | Battery save, RPG mechanics | Easy |
| **Dr. Mario** | 1990 | Nintendo | Falling blocks, music timing | Easy |

**PPU Features:**
- **Scrolling:** Zelda (omnidirectional), Metroid (vertical focus), Mega Man 2 (horizontal)
- **Sprite 0 hit:** Mega Man 2 (weapon energy bars), Zelda (HUD)
- **CHR banking:** Frequent pattern table updates for animations

**APU Features:**
- **Complex music:** Final Fantasy, Dr. Mario
- **Sound effects:** Mega Man 2 weapon sounds, Zelda secrets

**Known Challenges:**
- **Serial write timing:** 5 consecutive writes to $8000-$FFFF, bit 7 resets
- **Mega Man 2**: Infamously timing-sensitive, crashes on inaccurate emulators
- **Zelda**: Save RAM must persist, complex scrolling edge cases

#### Mapper 2 (UxROM) - 10.6% of games

**Technical Characteristics:**
- Simple PRG banking: 16KB switchable at $8000, 16KB fixed at $C000
- No CHR banking (8KB CHR-RAM standard)
- Bus conflicts on some boards (see [BUS_CONFLICTS.md](../bus/BUS_CONFLICTS.md))

**Recommended Games (8 games):**

| Game | Year | Publisher | What It Tests | Difficulty |
|------|------|-----------|---------------|------------|
| **Mega Man** | 1987 | Capcom | Precise platforming, weapon system | Medium |
| **Castlevania** | 1987 | Konami | Subweapon system, stairs, whip mechanics | Medium |
| **Contra** | 1988 | Konami | Fast action, spread gun, 2-player | Medium |
| **Duck Tales** | 1989 | Capcom | Pogo stick mechanic, treasure hunting | Easy |
| **Ninja Gaiden II** | 1990 | Tecmo | Cutscenes, wall climbing | Hard |
| **Batman** | 1990 | Sunsoft | Wall jump, tight controls | Medium |
| **Ghosts 'n Goblins** | 1986 | Capcom | Notoriously difficult, armor system | Very Hard |
| **Teenage Mutant Ninja Turtles** | 1989 | Konami | Turtle switching, underwater level | Medium |

**Known Challenges:**
- **Bus conflicts**: Writes to $8000-$FFFF must AND with ROM data on some boards
- **Mega Man**: Tight timing (classic test)
- **Ghosts 'n Goblins**: Infamously difficult game, good stress test

#### Mapper 3 (CNROM) - 6.3% of games

**Technical Characteristics:**
- Simple CHR banking: 8KB switchable
- No PRG banking (32KB fixed)
- Single write to $8000-$FFFF selects CHR bank

**Recommended Games (5 games):**

| Game | Year | Publisher | What It Tests | Difficulty |
|------|------|-----------|---------------|------------|
| **Super Mario Bros. (alt)** | 1985 | Nintendo | (Some releases use CNROM) | Easy |
| **Gradius** | 1986 | Konami | Horizontal scrolling shooter, power-ups | Medium |
| **Arkanoid** | 1987 | Taito | Paddle controller, brick physics | Easy |
| **Paperboy** | 1988 | Mindscape | Isometric view, obstacle avoidance | Medium |
| **Solomon's Key** | 1987 | Tecmo | Puzzle platformer, block creation | Medium |

**Known Challenges:**
- **Very simple mapper** - Usually works if NROM works
- **CHR banking only** - Good test for graphics switching

#### Mapper 4 (MMC3 / TxROM) - 23.4% of games

**Technical Characteristics:**
- Complex PRG banking: 8KB switchable banks, configurable layout
- Complex CHR banking: 2KB + 1KB banks
- **Scanline IRQ counter** - Critical feature for status bars, raster effects
- Mirroring control

**Recommended Games (12 games):**

| Game | Year | Publisher | What It Tests | Difficulty |
|------|------|-----------|---------------|------------|
| **Super Mario Bros. 3** | 1990 | Nintendo | Scanline IRQ, complex levels, inventory | Hard |
| **Mega Man 3** | 1990 | Capcom | Slide mechanic, Rush abilities | Medium |
| **Mega Man 4** | 1991 | Capcom | Charge shot, Wire adapter | Medium |
| **Kirby's Adventure** | 1993 | HAL | Large ROM, copy abilities, smooth scrolling | Very Hard |
| **Batman: Return of the Joker** | 1991 | Sunsoft | Advanced graphics, parallax scrolling | Hard |
| **Teenage Mutant Ninja Turtles II** | 1990 | Konami | 4-player support, beat-em-up | Medium |
| **Super Mario Bros. 2** | 1988 | Nintendo | Character switching, vegetable throwing | Medium |
| **Crystalis** | 1990 | SNK | Action RPG, sword leveling | Medium |
| **StarTropics** | 1990 | Nintendo | Island exploration, yo-yo weapon | Medium |
| **Chip 'n Dale: Rescue Rangers** | 1990 | Capcom | Object throwing, 2-player co-op | Easy |
| **Gauntlet** | 1987 | Tengen | 4-player, health drain, dungeon crawling | Medium |
| **TMNT III: The Manhattan Project** | 1991 | Konami | Beat-em-up, special moves | Medium |

**PPU Features:**
- **Scanline IRQ:** SMB3 status bar, Kirby's Adventure effects
- **Split-screen:** Status bars on top/bottom
- **Advanced scrolling:** Smooth omnidirectional (Kirby)

**APU Features:**
- **Complex soundtracks:** SMB3, Kirby's Adventure (advanced music)

**Known Challenges:**
- **Scanline IRQ timing**: Must trigger on PPU A12 rising edge (background fetch)
- **Kirby's Adventure**: Largest licensed NES game (6 Mbit), heavy CHR banking
- **SMB3**: Status bar requires precise IRQ timing or it shakes/glitches
- **MMC3 revisions**: Rev A vs Rev B have different IRQ behavior (use submapper)

#### Tier 2 Success Criteria

- [ ] 10+ MMC1 games boot and play (L2)
- [ ] 6+ UxROM games boot and play (L2)
- [ ] 4+ CNROM games boot and play (L2)
- [ ] 8+ MMC3 games boot (L1)
- [ ] SMB3 status bar stable (L3)
- [ ] Kirby's Adventure playable (L2)

---

### Tier 3: Advanced Mappers

**Development Phase:** Phase 2-3 (Months 7-18)
**Target:** Mappers 5, 7, 9, 10, 11, 19, 23-26, 69
**Coverage:** +15% (cumulative ~95%)
**Validation Level:** L2 minimum

#### Mapper 5 (MMC5 / ExROM) - Very Complex

**Technical Characteristics:**
- Advanced PRG banking (multiple modes)
- Advanced CHR banking
- ExRAM ($5C00-$5FFF) - Extended attribute table
- IRQ support (scanline counter)
- Expansion audio (2 pulse channels + PCM)
- Multiply/divide registers

**Recommended Games (4 games):**

| Game | Year | Publisher | What It Tests | Difficulty |
|------|------|-----------|---------------|------------|
| **Castlevania III** (US) | 1990 | Konami | ExRAM, expansion audio (MMC5), partner system | Very Hard |
| **Just Breed** | 1992 | Enix | Large PRG ROM, ExRAM, tactical RPG | Hard |
| **Metal Slader Glory** | 1991 | HAL | Large ROM, visual novel | Hard |
| **Uncharted Waters** | 1991 | Koei | Complex menus, world map | Medium |

**Known Challenges:**
- **Most complex mapper** - Save for late implementation
- **ExRAM modes** - Can be used as nametable, attribute, or regular RAM
- **Expansion audio** - 2 additional pulse channels + PCM playback
- **IRQ timing** - Multiple IRQ modes

**Note**: Castlevania III US version uses MMC5. Japanese version (Akumajou Densetsu) uses VRC6 (see Tier 5).

#### Mapper 7 (AxROM) - 3.1% of games

**Technical Characteristics:**
- 32KB PRG banking (full $8000-$FFFF switchable)
- One-screen mirroring control
- No CHR banking (8KB CHR-RAM)

**Recommended Games (3 games):**

| Game | Year | Publisher | What It Tests | Difficulty |
|------|------|-----------|---------------|------------|
| **Battletoads** | 1991 | Tradewest | Precise timing, sprite 0 hit, fast scrolling | Very Hard |
| **Marble Madness** | 1989 | Milton Bradley | Isometric physics, momentum | Medium |
| **Wizards & Warriors** | 1987 | Acclaim | Platforming, item collection | Medium |

**Known Challenges:**
- **Battletoads**: [Infamously difficult to emulate](https://www.nesdev.org/wiki/Tricky-to-emulate_games) - requires precise CPU/PPU timing
  - Streams animation frames into CHR-RAM during rendering-disabled scanlines
  - Sprite 0 hit timing must be exact or game hangs entering first stage
  - Uses 1-screen mirroring dynamically
  - Classic "emulator killer" - if Battletoads works, your timing is excellent

#### Mapper 9 (MMC2 / PxROM) - Punch-Out style

**Technical Characteristics:**
- PRG banking: 8KB switchable at $8000, 24KB fixed
- CHR banking: Latch-based (switches on PPU reads of $FD/$FE tiles)
- 2 independent latches for $0000 and $1000 regions

**Recommended Games (2 games):**

| Game | Year | Publisher | What It Tests | Difficulty |
|------|------|-----------|---------------|------------|
| **Punch-Out!!** | 1990 | Nintendo | CHR latching, large sprites, timing | Hard |
| **Mike Tyson's Punch-Out!!** | 1987 | Nintendo | (Same as above, different opponent) | Hard |

**Known Challenges:**
- **CHR latch mechanism** - Banks switch based on which tiles PPU fetches
- **Large sprites** - Uses advanced PPU techniques for big characters

#### Mapper 10 (MMC4) - Similar to MMC2

**Recommended Games (1 game):**

| Game | Year | Publisher | What It Tests | Difficulty |
|------|------|-----------|---------------|------------|
| **Fire Emblem: Gaiden** | 1992 | Nintendo | CHR latching, tactical RPG | Medium |

#### Mapper 11 (Color Dreams) - Unlicensed

**Recommended Games (2 games):**

| Game | Year | Publisher | What It Tests | Difficulty |
|------|------|-----------|---------------|------------|
| **Crystal Mines** | 1989 | Color Dreams | Simple unlicensed mapper | Easy |
| **Bible Adventures** | 1990 | Wisdom Tree | (Unlicensed religious game) | Easy |

#### Tier 3 Success Criteria

- [ ] Battletoads boots and reaches first stage (L2)
- [ ] Punch-Out!! playable (L2)
- [ ] MMC5 games boot (L1)
- [ ] 50+ total mappers implemented (Phase 3 goal)

---

### Tier 4: Edge Cases & Quirks

**Development Phase:** Phase 3-4 (Months 13-24)
**Target:** Games with known emulation challenges
**Coverage:** Quality over quantity
**Validation Level:** L3-L4 (accuracy focus)

#### Timing-Sensitive Games

| Game | Mapper | Issue | Fix Required |
|------|--------|-------|--------------|
| **Battletoads** | 7 | Sprite 0 hit timing, CHR streaming | Cycle-accurate PPU |
| **Mega Man 2** | 1 | Crashes on inaccurate timing | Proper CPU/PPU sync |
| **Teenage Mutant Ninja Turtles** | 2 | Status bar sprite 0 hit | Accurate sprite 0 timing |
| **Addams Family** | 4 | Shaky status bar | Scanline IRQ precision |
| **Ghostbusters 2** | 4 | Status bar issues | Scanline IRQ precision |
| **Gradius** | 3 | Slowdown emulation | Proper sprite overflow |

#### Sprite 0 Hit Edge Cases

**Games with problematic sprite 0 splits:**

| Game | Issue | Reference |
|------|-------|-----------|
| **TMNT** | Bottom status bar | [nesdoug.com](https://nesdoug.com/2018/09/05/18-sprite-zero/) |
| **Addams Family** | Shaky status bar | nesdev forums |
| **Ghostbusters 2** | Split screen artifacts | nesdev wiki |
| **Legend of Prince Valiant** | Status bar timing | nesdev forums |
| **Ninja Gaiden** | 8x16 sprites, complex split | [nesdev forums](https://forums.nesdev.org/viewtopic.php?t=8832) |
| **Sword Master** | Multiple splits (HUD, foreground) | nesdev forums |

**See [Tricky-to-emulate games](https://www.nesdev.org/wiki/Tricky-to-emulate_games) for comprehensive list.**

#### Sprite Overflow Games

**Games requiring sprite overflow emulation:**

| Game | Mapper | Requirement | Notes |
|------|--------|-------------|-------|
| **Bee 52** | Unknown | Splits at scanline 165 (overflow), then 207 (sprite 0) | Crashes without sprite overflow |
| **Many action games** | Various | Intentional slowdown effect | Authentic slowdown requires overflow |

#### PPU Edge Cases

| Game | Mapper | Edge Case | Fix |
|------|--------|-----------|-----|
| **Solar Jetman** | 4 | Scrolling seam artifacts | Precise scroll register timing |
| **Klax** | 4 | Falling block rendering | Accurate CHR banking |
| **Elite** | 7 | Split-screen mode | Complex $2005/$2006 writes |

#### APU Edge Cases

| Game | Mapper | Issue | Fix |
|------|--------|-------|-----|
| **Skate or Die** | 1 | DMC conflicts with rendering | DMA cycle stealing |
| **Bart vs. the Space Mutants** | 1 | DMC audio glitches | Proper DMC timing |

#### Success Criteria

- [ ] Battletoads completes first stage (L3)
- [ ] TMNT status bar stable (L4)
- [ ] Mega Man 2 completable (L3)
- [ ] Ninja Gaiden split screen correct (L4)

---

### Tier 5: Expansion Audio

**Development Phase:** Phase 3 (Months 13-15)
**Target:** Mappers with expansion audio chips
**Coverage:** <1% of library, but critical for accuracy enthusiasts
**Validation Level:** Audio accuracy focus

#### VRC6 (Mappers 24/26) - Konami

**Technical Characteristics:**
- 2 additional pulse wave channels
- 1 sawtooth wave channel
- Used in Japanese Famicom games (not NES compatible)

**Recommended Games (3 games):**

| Game | Year | Publisher | Audio Characteristics | Notes |
|------|------|-----------|----------------------|-------|
| **Akumajou Densetsu** (Castlevania III JP) | 1989 | Konami | Iconic VRC6 soundtrack, "crunchy" sawtooth bass | [Most famous VRC6 example](https://classicalgaming.wordpress.com/2011/02/28/fidelity-concerns-akumajou-dentetsu-vs-castlevania-iii-draculas-curse/) |
| **Esper Dream 2** | 1992 | Konami | RPG with VRC6 music | Japanese exclusive |
| **Madara** | 1990 | Konami | Action RPG, VRC6 soundtrack | Japanese exclusive |

**Audio Features:**
- **Sawtooth channel** - Rich bass tones, "thudding" quality
- **Extra pulse waves** - 6 total melodic channels (4 from 2A03 + 2 VRC6)
- **Comparison**: VRC6 vs MMC5 - Sawtooth is key difference

**Implementation Reference:**
- [Famicom Expansion Audio](https://jsgroth.dev/blog/posts/famicom-expansion-audio/)
- [Castlevania III with VRC6](https://callanbrown.com/index.php/castlevania-iii-with-full-famicom-audio/)

#### VRC7 (Mapper 85) - Konami FM Synthesis

**Technical Characteristics:**
- Yamaha OPLL FM synthesis chip (customized YM2413)
- 6 FM channels
- Only 1 game uses VRC7 audio

**Recommended Games (1 game):**

| Game | Year | Publisher | Audio Characteristics | Notes |
|------|------|-----------|----------------------|-------|
| **Lagrange Point** | 1991 | Konami | Only VRC7 game, FM synth soundtrack | Japanese exclusive, rare |

**Audio Features:**
- **FM synthesis** - Rich, complex timbres unlike PSG
- **Implementation challenge** - Requires YM2413 emulation core

#### MMC5 (Mapper 5) - Nintendo

**Technical Characteristics:**
- 2 additional pulse channels (identical to 2A03)
- 1 PCM channel
- Only expansion usable on international NES

**Recommended Games (3 games):**

| Game | Year | Publisher | Audio Characteristics | Notes |
|------|------|-----------|----------------------|-------|
| **Castlevania III** (US) | 1990 | Konami | Downgraded from VRC6, less rich bass | [Comparison with JP version](https://classicalgaming.wordpress.com/2011/02/27/fidelity-concerns-the-lost-sound-expansion-chips-of-the-nes/) |
| **Just Breed** | 1992 | Enix | MMC5 audio, tactical RPG | Japanese exclusive |
| **Shin 4-Nin Uchi Mahjong** | 1984 | Nintendo | MMC5 pulse waves | Obscure |

**Audio Features:**
- **2 pulse + PCM** - Less capable than VRC6 (no sawtooth)
- **Comparison**: US Castlevania III uses MMC5 vs JP uses VRC6

#### Namco 163 (Mapper 19)

**Technical Characteristics:**
- 1-8 wavetable channels (configurable)
- Shared among channels (more channels = less fidelity per channel)

**Recommended Games (2 games):**

| Game | Year | Publisher | Audio Characteristics | Notes |
|------|------|-----------|----------------------|-------|
| **Final Lap** | 1988 | Namco | N163 wavetable audio | Racing game |
| **Splatterhouse: Wanpaku Graffiti** | 1989 | Namco | N163 audio, horror theme | Japanese exclusive |

#### Sunsoft 5B (Mapper 69)

**Technical Characteristics:**
- AY-3-8910 PSG (Programmable Sound Generator)
- 3 square wave channels
- 1 noise channel
- Only 1 game uses 5B audio

**Recommended Games (1 game):**

| Game | Year | Publisher | Audio Characteristics | Notes |
|------|------|-----------|----------------------|-------|
| **Gimmick!** (Japanese) | 1992 | Sunsoft | Only 5B audio game, excellent soundtrack | Japanese/Scandinavian exclusive |

**Note**: Other Sunsoft 5B games (Batman: Return of the Joker) don't use expansion audio.

#### Famicom Disk System (FDS)

**Technical Characteristics:**
- Wavetable channel
- Modulation unit
- Proprietary disk format (.fds files)

**Recommended Games (3 games):**

| Game | Year | Publisher | Audio Characteristics | Notes |
|------|------|-----------|----------------------|-------|
| **Castlevania** (FDS) | 1986 | Konami | FDS wavetable audio | Different from NES Castlevania |
| **Metroid** (FDS) | 1986 | Nintendo | FDS audio version | Japan exclusive |
| **Zelda no Densetsu** (FDS) | 1986 | Nintendo | FDS audio version | Japan exclusive |

#### Expansion Audio Success Criteria

- [ ] VRC6 games play with correct audio (Akumajou Densetsu)
- [ ] MMC5 games play with expansion audio (Castlevania III US)
- [ ] Lagrange Point FM synthesis works (VRC7)
- [ ] Gimmick! plays with 5B audio (Sunsoft 5B)
- [ ] Audio matches hardware recordings (spectral analysis)

#### Why Expansion Audio Matters

While <1% of games, expansion audio is:
- **Showcase feature** - Demonstrates emulator sophistication
- **Accuracy benchmark** - Requires precise audio mixing
- **Community favorite** - Castlevania III (VRC6 vs MMC5) is iconic comparison
- **Technical challenge** - FM synthesis (VRC7) is complex

**Implementation Priority:** Phase 3 (after core mappers stable)

---

## Testing Checkpoints

### Checkpoint Definitions

**Formal testing stages to validate progress:**

| Checkpoint | Description | Success Criteria | Phase |
|------------|-------------|------------------|-------|
| **C1: Boot** | ROM loads, title screen appears | Video output renders, no crash | Phase 1 |
| **C2: Interact** | Player can control character | Input registers work, movement functional | Phase 1 |
| **C3: First Level** | Complete first level/area | Collision, enemies, level transition works | Phase 1 |
| **C4: Audio** | Music and sound effects play | All APU channels functional, no artifacts | Phase 1 |
| **C5: Save/Load** | Battery-backed saves persist | SRAM writes to disk, loads correctly | Phase 1 |
| **C6: Complete** | Full game playthrough possible | All levels, no game-breaking bugs | Phase 2 |
| **C7: Accurate** | Pixel/cycle-perfect match | Passes accuracy test ROMs, no glitches | Phase 4 |

### Per-Tier Checkpoints

**Tier 1 (NROM):**
- [ ] 15/15 games: C1 (Boot)
- [ ] 12/15 games: C3 (First Level)
- [ ] 8/15 games: C6 (Complete)
- [ ] 5/15 games: C7 (Accurate)

**Tier 2 (MMC1/UxROM/CNROM/MMC3):**
- [ ] 30/35 games: C1 (Boot)
- [ ] 25/35 games: C3 (First Level)
- [ ] 15/35 games: C6 (Complete)
- [ ] SMB3, Kirby: C7 (Accurate)

**Tier 3 (Advanced):**
- [ ] 10/15 games: C1 (Boot)
- [ ] 7/15 games: C3 (First Level)
- [ ] Battletoads: C3 (First Level)

**Tier 4 (Edge Cases):**
- [ ] All edge case games: C3 (First Level)
- [ ] 50% edge cases: C7 (Accurate)

**Tier 5 (Expansion Audio):**
- [ ] All expansion games: C4 (Audio accurate)

---

## Recommended Test Suite

### Curated 50-Game Test Suite

**This suite covers all essential mappers, PPU/APU features, and edge cases.**

#### NROM (Mapper 0) - 8 games
1. Super Mario Bros.
2. Donkey Kong
3. Ice Climber
4. Excitebike
5. Balloon Fight
6. Kung Fu
7. Wrecking Crew
8. Pinball

#### MMC1 (Mapper 1) - 8 games
9. The Legend of Zelda
10. Metroid
11. Mega Man 2
12. Castlevania II
13. Kid Icarus
14. Blaster Master
15. Final Fantasy
16. Dr. Mario

#### UxROM (Mapper 2) - 6 games
17. Mega Man
18. Castlevania
19. Contra
20. Duck Tales
21. Ninja Gaiden II
22. Batman

#### CNROM (Mapper 3) - 3 games
23. Gradius
24. Arkanoid
25. Solomon's Key

#### MMC3 (Mapper 4) - 10 games
26. Super Mario Bros. 3
27. Mega Man 3
28. Mega Man 4
29. Kirby's Adventure
30. Batman: Return of the Joker
31. TMNT II: The Arcade Game
32. Super Mario Bros. 2
33. Crystalis
34. StarTropics
35. Chip 'n Dale: Rescue Rangers

#### AxROM (Mapper 7) - 3 games
36. Battletoads
37. Marble Madness
38. Wizards & Warriors

#### MMC2 (Mapper 9) - 1 game
39. Punch-Out!!

#### MMC5 (Mapper 5) - 2 games
40. Castlevania III (US)
41. Just Breed

#### Color Dreams (Mapper 11) - 1 game
42. Crystal Mines

#### Edge Cases - 5 games
43. TMNT (sprite 0 edge case)
44. Addams Family (status bar)
45. Ninja Gaiden (complex sprite 0)
46. Bee 52 (sprite overflow)
47. Ghosts 'n Goblins (difficulty stress test)

#### Expansion Audio - 3 games
48. Akumajou Densetsu (VRC6)
49. Lagrange Point (VRC7)
50. Gimmick! (Sunsoft 5B)

### Testing Frequency

**Regression Testing Schedule:**

| Frequency | Scope | Purpose |
|-----------|-------|---------|
| **Every commit** | 10 core games (automated) | Catch regressions immediately |
| **Daily** | 25 game subset | Development validation |
| **Weekly** | Full 50-game suite | Comprehensive coverage |
| **Pre-release** | 100+ games + all test ROMs | Final validation |

---

## Per-Game Testing Matrix

### Template for Documenting Game Testing

```markdown
### [Game Title]

**Mapper:** [Number] ([Name])
**Year:** [Year]
**Publisher:** [Publisher]

**PPU Features Exercised:**
- Scrolling: [Type]
- Sprite 0 hit: [Yes/No]
- Sprite count: [Low/Medium/High]
- Special: [Any unique PPU usage]

**APU Features Exercised:**
- Channels used: [Pulse1/Pulse2/Triangle/Noise/DMC]
- Expansion audio: [Yes/No]
- Complexity: [Simple/Medium/Complex]

**Mapper Features:**
- PRG banking: [Description]
- CHR banking: [Description]
- IRQ: [Yes/No]
- Save RAM: [Yes/No]

**Known Emulation Challenges:**
- [List any known issues or quirks]

**Testing Checkpoints:**
- [ ] C1: Boots to title screen
- [ ] C2: Controls responsive
- [ ] C3: First level playable
- [ ] C4: Audio correct
- [ ] C5: Saves work (if applicable)
- [ ] C6: Full playthrough
- [ ] C7: Pixel-perfect accuracy

**Test Notes:**
[Specific things to watch for]
```

### Example: Super Mario Bros.

```markdown
### Super Mario Bros.

**Mapper:** 0 (NROM)
**Year:** 1985
**Publisher:** Nintendo

**PPU Features Exercised:**
- Scrolling: Horizontal (left-to-right)
- Sprite 0 hit: Yes (status bar split at top)
- Sprite count: Medium (Mario, enemies, projectiles)
- Special: Classic status bar split

**APU Features Exercised:**
- Channels used: All (Pulse1, Pulse2, Triangle, Noise)
- Expansion audio: No
- Complexity: Medium (iconic soundtrack)

**Mapper Features:**
- PRG banking: None (32KB fixed)
- CHR banking: None (8KB fixed)
- IRQ: No
- Save RAM: No

**Known Emulation Challenges:**
- Sprite 0 hit timing must be cycle-accurate or status bar shakes
- Scrolling seam can appear if PPU timing off
- Flag pole timing sensitive

**Testing Checkpoints:**
- [x] C1: Boots to title screen
- [x] C2: Controls responsive
- [x] C3: World 1-1 completable
- [x] C4: Overworld theme plays correctly
- [x] C6: Full 8 worlds playable
- [ ] C7: Pixel-perfect (status bar stable, no seam)

**Test Notes:**
- Watch status bar during gameplay (should not shake or flicker)
- Check for vertical scrolling seam on right edge
- Verify coin counter increments correctly
- Test warp zones (4-2, 8-4)
```

---

## Testing Workflow

### Development Cycle Integration

**Daily Development:**
1. **Morning**: Run 10-game automated test
2. **During development**: Test specific game for feature being implemented
3. **Before commit**: Run 25-game subset
4. **CI/CD**: Automated 10-game test on push

**Weekly Validation:**
1. Run full 50-game suite
2. Document new failures/fixes
3. Update testing matrix
4. Check for regressions

**Phase Milestones:**
1. Run comprehensive test (100+ games)
2. Run all test ROMs (nestest, blargg, TASVideos)
3. Compare with accuracy targets (ROADMAP.md)
4. Document compatibility percentage

### Automation Strategy

```rust
// Example automated game test
#[test]
fn test_super_mario_bros_boots() {
    let rom = load_rom("test_roms/games/Super Mario Bros.nes");
    let mut nes = Nes::new(rom).unwrap();

    // Run 5 seconds (300 frames NTSC)
    for _ in 0..300 {
        nes.step_frame();
    }

    // Check that we're past title screen (gameplay started)
    // This is game-specific - would check RAM for game state
    let game_state = nes.cpu.bus.read(0x000E); // Game mode flag
    assert!(game_state >= 0x01); // 0x01 = in-game
}

#[test]
fn test_super_mario_bros_world_1_1() {
    let rom = load_rom("test_roms/games/Super Mario Bros.nes");
    let mut nes = Nes::new(rom).unwrap();

    // Load TAS input file (pre-recorded World 1-1 completion)
    let inputs = load_fm2("test_roms/tas/smb_world1-1.fm2");

    // Play back inputs
    for (frame, input) in inputs.iter().enumerate() {
        nes.set_controller_state(*input);
        nes.step_frame();
    }

    // Check that level was completed
    let world = nes.cpu.bus.read(0x075F); // World number
    let level = nes.cpu.bus.read(0x0760); // Level number
    assert_eq!((world, level), (0, 1)); // Should be World 1-2 after completing 1-1
}
```

### Manual Testing Checklist

**Per-Game Manual Test:**
- [ ] ROM loads without error
- [ ] Title screen renders correctly
- [ ] Music plays (no pops/clicks)
- [ ] Controls respond to input
- [ ] First level/area playable
- [ ] No visual glitches (sprites, background)
- [ ] Audio matches reference emulator
- [ ] Save/load works (if applicable)
- [ ] Game completable (spot-check later levels)

**Visual Comparison:**
- Run RustyNES and Mesen2 side-by-side
- Take screenshots at key points
- Compare frame-by-frame if issues found

**Audio Comparison:**
- Record 30-second audio clip from RustyNES
- Record same clip from Mesen2
- Load both in Audacity, compare waveforms
- Check spectral analysis for frequency accuracy

### Issue Documentation

When a game fails, document:

```markdown
### [Game Title] - [Issue Type]

**Date:** [YYYY-MM-DD]
**Mapper:** [Number]
**Phase:** [Current dev phase]

**Issue Description:**
[What went wrong]

**Expected Behavior:**
[What should happen]

**Actual Behavior:**
[What actually happens]

**Steps to Reproduce:**
1. [Step 1]
2. [Step 2]
...

**Related Test ROMs:**
[Any test ROMs that cover this behavior]

**Component:**
[CPU/PPU/APU/Mapper/Bus/Input]

**Priority:**
[P0-P3]

**Fix Status:**
- [ ] Root cause identified
- [ ] Fix implemented
- [ ] Test passes
- [ ] Regression test added
```

---

## References

### External Resources

**NESdev Wiki:**
- [Tricky-to-emulate games](https://www.nesdev.org/wiki/Tricky-to-emulate_games)
- [Mapper List](https://www.nesdev.org/wiki/Mapper)
- [NROM](https://www.nesdev.org/wiki/NROM)
- [MMC1](https://www.nesdev.org/wiki/MMC1)
- [MMC3](https://www.nesdev.org/wiki/MMC3)
- [PPU Scrolling](https://www.nesdev.org/wiki/PPU_scrolling)
- [Sprite Overflow Games](https://www.nesdev.org/wiki/Sprite_overflow_games)
- [Expansion Audio Games](https://wiki.nesdev.com/w/index.php/List_of_games_with_expansion_audio)

**Articles & Guides:**
- [Advanced Nerdy Nights: Sprite 0 Hit](https://archive.nes.science/nintendoage-forums/nintendoage.com/forum/messageview6158-2.html?catid=22&threadid=36969)
- [nesdoug: Sprite Zero](https://nesdoug.com/2018/09/05/18-sprite-zero/)
- [Writing NES Emulator: PPU Scrolling](https://bugzmanov.github.io/nes_ebook/chapter_8.html)
- [NES Graphics Part 3](https://www.dustmop.io/blog/2015/12/18/nes-graphics-part-3/)
- [Elite Split-Screen Mode](https://elite.bbcelite.com/deep_dives/the_split-screen_mode_nes.html)

**Expansion Audio:**
- [Famicom Expansion Audio Blog](https://jsgroth.dev/blog/posts/famicom-expansion-audio/)
- [Castlevania III Audio Comparison](https://callanbrown.com/index.php/castlevania-iii-with-full-famicom-audio/)
- [VRC6 vs MMC5 Fidelity](https://classicalgaming.wordpress.com/2011/02/28/fidelity-concerns-akumajou-dentetsu-vs-castlevania-iii-draculas-curse/)
- [NES Sound Expansion Chips](https://classicalgaming.wordpress.com/2011/02/27/fidelity-concerns-the-lost-sound-expansion-chips-of-the-nes/)

**Emulator Development:**
- [emudev: MMC1 and MMC3](https://emudev.de/nes-emulator/about-mappers-mmc1-and-mmc3/)
- [nesdoug: MMC1](https://nesdoug.com/2019/10/02/22-advanced-mapper-mmc1/)
- [nesdoug: MMC3](https://nesdoug.com/2019/11/11/23-advanced-mapper-mmc3/)
- [TASVideos: NES Accuracy Tests](https://tasvideos.org/EmulatorResources/NESAccuracyTests)

**Homebrew Development:**
- [HonkeyPong](https://github.com/HonkeyKong/HonkeyPong) - NROM tutorial game
- [Holy Mapperel](https://github.com/pinobatch/holy-mapperel) - Cartridge testing

### Related RustyNES Documentation

**Core Documentation:**
- [TEST_ROM_GUIDE.md](TEST_ROM_GUIDE.md) - Test ROM validation strategy
- [NESTEST_GOLDEN_LOG.md](NESTEST_GOLDEN_LOG.md) - nestest golden log reference
- [MAPPER_OVERVIEW.md](../mappers/MAPPER_OVERVIEW.md) - Mapper architecture
- [PPU_SCROLLING_INTERNALS.md](../ppu/PPU_SCROLLING_INTERNALS.md) - Scrolling implementation
- [PPU_SPRITE_EVALUATION.md](../ppu/PPU_SPRITE_EVALUATION.md) - Sprite rendering
- [APU_OVERVIEW.md](../apu/APU_OVERVIEW.md) - Audio system

**Mapper Documentation:**
- [MAPPER_NROM.md](../mappers/MAPPER_NROM.md)
- [MAPPER_MMC1.md](../mappers/MAPPER_MMC1.md)
- [MAPPER_UXROM.md](../mappers/MAPPER_UXROM.md)
- [MAPPER_CNROM.md](../mappers/MAPPER_CNROM.md)
- [MAPPER_MMC3.md](../mappers/MAPPER_MMC3.md)

**Project Planning:**
- [ROADMAP.md](../../ROADMAP.md) - Development phases and milestones
- [OVERVIEW.md](../../OVERVIEW.md) - Project vision
- [CONTRIBUTING.md](../dev/CONTRIBUTING.md) - How to contribute

---

## Appendix: Game Statistics

### Mapper Coverage

| Mapper | Games | % Library | Cumulative % |
|--------|-------|-----------|--------------|
| 1 (MMC1) | 681 | 27.9% | 27.9% |
| 4 (MMC3) | 600 | 23.4% | 51.3% |
| 2 (UxROM) | 270 | 10.6% | 61.9% |
| 0 (NROM) | 248 | 9.5% | 71.4% |
| 3 (CNROM) | 155 | 6.3% | 77.7% |
| 7 (AxROM) | 76 | 3.1% | 80.8% |
| **Top 6** | **2,030** | **80.8%** | - |
| **Top 15** | **2,380** | **95%** | - |
| **All mappers** | **~2,500** | **100%** | - |

**Source:** Mapper usage statistics from NESdev wiki and emulator compatibility databases.

### Platform Coverage

| Platform | Count | Notes |
|----------|-------|-------|
| Licensed NES | ~800 | Official Nintendo releases |
| Licensed Famicom | ~1,200 | Japan exclusive |
| Unlicensed | ~300 | Color Dreams, Wisdom Tree, etc. |
| Homebrew | ~200+ | Modern development |
| Total | ~2,500+ | Approximate catalog size |

---

**Document Status:** Comprehensive game ROM testing strategy complete, covering all development phases and mapper tiers with practical validation checkpoints.

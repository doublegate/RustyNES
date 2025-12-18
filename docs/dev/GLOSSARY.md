# NES Emulation Glossary

**Table of Contents**
- [Hardware Terms](#hardware-terms)
- [CPU Terms](#cpu-terms)
- [PPU Terms](#ppu-terms)
- [APU Terms](#apu-terms)
- [Mapper Terms](#mapper-terms)
- [Emulation Terms](#emulation-terms)

---

## Hardware Terms

**APU** (Audio Processing Unit)
- The NES sound chip (integrated into CPU chip on 2A03/2A07)
- Generates pulse, triangle, noise, and DMC audio channels

**Bus**
- The communication pathway connecting components (CPU, PPU, APU, cartridge)
- CPU bus (16-bit address, 8-bit data)
- PPU bus (14-bit address, 8-bit data)

**Cartridge**
- Game ROM module inserted into NES
- Contains PRG-ROM (program), CHR-ROM/RAM (graphics), and mapper hardware

**CHR** (Character ROM/RAM)
- Graphics tile data (8×8 pixel tiles)
- ROM = read-only, RAM = writable

**CPU** (Central Processing Unit)
- Modified 6502 processor (Ricoh 2A03 NTSC, 2A07 PAL)
- Runs game code at 1.789773 MHz (NTSC)

**Famicom**
- Japanese version of NES
- Different controller ports, expansion port, audio hardware

**NES** (Nintendo Entertainment System)
- North American/European version
- Released 1985 (US), 1986 (Europe)

**PPU** (Picture Processing Unit)
- Graphics chip (Ricoh 2C02 NTSC, 2C07 PAL)
- Renders 256×240 pixel display at 60 Hz (NTSC)

**PRG-ROM** (Program ROM)
- Executable game code and data
- Mapped to CPU address space ($8000-$FFFF)

**VRAM** (Video RAM)
- 2KB internal RAM for nametables
- Additional CHR-RAM/ROM for tile graphics

---

## CPU Terms

**6502**
- 8-bit microprocessor used in NES (modified version)
- Also used in Apple II, Commodore 64, Atari 2600

**Accumulator (A)**
- Primary 8-bit register for arithmetic/logic operations

**Addressing Mode**
- How an instruction accesses memory (immediate, zero page, absolute, indexed, etc.)

**Cycle**
- One CPU clock tick (1/1.789773 MHz ≈ 559 nanoseconds)
- Instructions take 2-7 cycles

**IRQ** (Interrupt Request)
- Maskable interrupt (can be disabled)
- Used by mappers (MMC3 scanline counter)

**NMI** (Non-Maskable Interrupt)
- Triggered at start of VBlank (PPU scanline 241)
- Cannot be disabled

**Program Counter (PC)**
- 16-bit register pointing to next instruction

**Stack Pointer (SP)**
- 8-bit register ($0100-$01FF address range)
- Grows downward

**Status Register (P)**
- 8-bit flags: Carry, Zero, Interrupt Disable, Decimal (unused on NES), Break, Overflow, Negative

**Unofficial Opcodes**
- Undocumented 6502 instructions
- Used by some games for optimization

**Zero Page**
- First 256 bytes of RAM ($0000-$00FF)
- Faster access than absolute addressing

---

## PPU Terms

**Attribute Table**
- 64-byte area defining palette selection for 2×2 tile groups
- Located at end of each nametable ($23C0, $27C0, etc.)

**Background**
- Scrollable 256×240 pixel playfield
- Composed of 8×8 pixel tiles from pattern tables

**Dot**
- One PPU clock tick (3 dots per CPU cycle)
- PPU runs at 5.37 MHz (NTSC)

**Mirroring**
- How 4 nametables map to 2KB VRAM
- Horizontal, Vertical, Single-screen, Four-screen

**Nametable**
- 1024-byte area defining background tile layout
- 30 rows × 32 columns = 960 bytes + 64-byte attribute table

**OAM** (Object Attribute Memory)
- 256 bytes storing sprite data (64 sprites × 4 bytes each)
- Attributes: Y, tile, flags, X

**Palette**
- 32-byte RAM storing color indices ($3F00-$3F1F)
- 4 background palettes + 4 sprite palettes
- Each palette has 4 colors (one shared backdrop)

**Pattern Table**
- 8KB area ($0000-$1FFF) containing tile graphics
- Two 4KB tables (256 tiles each)

**Rendering**
- Process of generating 256×240 pixel output
- Occurs scanlines 0-239, dots 0-256

**Scanline**
- One horizontal line of pixels (341 PPU dots)
- 262 scanlines per frame (NTSC), lines 0-239 visible

**Sprite**
- Movable 8×8 (or 8×16) pixel object
- Max 64 sprites, 8 per scanline

**Sprite Zero Hit**
- Flag set when sprite 0 overlaps non-transparent background pixel
- Used for split-screen effects

**VBlank** (Vertical Blank)
- Period when PPU is not rendering (scanlines 241-260)
- Safe time for PPU memory updates
- Triggers NMI

**VRAM Address**
- 15-bit address in PPU address space
- Loopy register: holds scroll position and VRAM address

---

## APU Terms

**DMC** (Delta Modulation Channel)
- Sample playback channel (1-bit DPCM)
- Can play 4-bit PCM samples from ROM

**Duty Cycle**
- Ratio of high to low in pulse wave (12.5%, 25%, 50%, 75%)

**Envelope**
- Volume decay over time (ADSR style)

**Frame Counter**
- Controls APU timing (4-step or 5-step mode)
- Clocks envelopes, length counters, sweep units

**Length Counter**
- Automatically disables channels after set duration

**Mixer**
- Combines 5 APU channels into final audio output
- Non-linear mixing (lookup tables)

**Noise Channel**
- Generates pseudo-random noise (percussion)
- 16-step or 93-step mode

**Pulse Channel**
- Square wave with duty cycle, volume, pitch sweep
- Two pulse channels available

**Sweep Unit**
- Automatically adjusts pulse channel frequency
- Creates pitch bends, glissandos

**Triangle Channel**
- Low-frequency bass/melody channel
- No volume control

---

## Mapper Terms

**Bank**
- Fixed-size chunk of ROM (typically 8KB or 16KB)
- Swapped into address space dynamically

**Bank Switching**
- Changing which ROM bank is visible in address space
- Enables games larger than 32KB PRG / 8KB CHR

**Bus Conflict**
- When CPU and ROM both drive data bus during write
- Occurs on discrete logic mappers (UxROM, CNROM)

**iNES**
- ROM file format (.nes files)
- 16-byte header + PRG-ROM + CHR-ROM

**Mapper**
- Hardware in cartridge providing bank switching, IRQ, extra features
- Over 300 different mappers documented

**MMC** (Memory Management Controller)
- Nintendo's custom mapper ASICs (MMC1-MMC6)

**NES 2.0**
- Extended iNES format with submapper field
- Supports larger ROMs, more precise hardware description

**Submapper**
- 4-bit variant identifier in NES 2.0 header
- Disambiguates different revisions of same mapper

**UNIF**
- Alternative ROM format using board names instead of mapper numbers
- Largely superseded by NES 2.0

---

## Emulation Terms

**Accuracy**
- How closely emulator behavior matches real hardware
- Cycle-accurate, scanline-accurate, frame-accurate

**Cycle-Accurate**
- Emulates hardware at CPU/PPU cycle granularity
- Required for timing-sensitive games

**Frame**
- One complete screen update (262 scanlines)
- 60 FPS (NTSC), 50 FPS (PAL)

**Headless**
- Emulation without GUI (for testing, TAS automation)

**Netplay**
- Online multiplayer via internet
- GGPO = rollback netcode for low latency

**Open Bus**
- Reading unmapped address returns last value on data bus
- Decays over time (milliseconds)

**Retroachievements**
- Achievement system for retro games
- Integration via rcheevos library

**Save State**
- Snapshot of entire emulator state
- Allows instant save/load

**TAS** (Tool-Assisted Speedrun)
- Frame-perfect gameplay using save states, slow-motion
- FM2 format for NES movies

**Test ROM**
- Special ROM designed to validate emulator accuracy
- Examples: nestest, blargg's suite

**Timing**
- Synchronization between CPU, PPU, APU
- Critical for accuracy (3 PPU dots = 1 CPU cycle)

---

**Related Documents**:
- [CPU_6502.md](../cpu/CPU_6502.md) - CPU architecture
- [PPU_OVERVIEW.md](../ppu/PPU_OVERVIEW.md) - PPU architecture
- [APU_OVERVIEW.md](../apu/APU_OVERVIEW.md) - APU architecture
- [MAPPER_OVERVIEW.md](../mappers/MAPPER_OVERVIEW.md) - Mappers

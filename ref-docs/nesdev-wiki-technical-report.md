# Nesdev Wiki NES Technical Report

**Generated:** 2026-05-20
**Primary source:** [Nesdev Wiki](https://www.nesdev.org/wiki/Nesdev_Wiki)
**Source snapshot:** MediaWiki reports 763 English articles and 597 uploaded files as
of this run.
**Scope:** Nintendo Entertainment System / Family Computer hardware,
programming, emulation, cartridge formats, mappers, test ROMs, and accuracy
risks synthesized from the Nesdev Wiki root pages and major nested reference
pages.

## Scope And Attribution

This document is an engineering synthesis, not a mirror of the Nesdev Wiki.
The wiki has hundreds of pages, historical files, forum links, diagrams, and
downloadable test material. Reproducing it wholesale would be impractical and
would create attribution and licensing ambiguity. Instead, this report organizes
the emulator-relevant knowledge into a single local Markdown reference and links
back to the primary pages for exact diagrams, tables, examples, and revision
history.

Primary entry points used for this pass:

- [Nesdev Wiki main page](https://www.nesdev.org/wiki/Nesdev_Wiki)
- [NES reference guide](https://www.nesdev.org/wiki/NES_reference_guide)
- [Programming guide](https://www.nesdev.org/wiki/Programming_guide)
- [CPU](https://www.nesdev.org/wiki/CPU)
- [PPU](https://www.nesdev.org/wiki/PPU)
- [APU](https://www.nesdev.org/wiki/APU)
- [DMA](https://www.nesdev.org/wiki/DMA)
- [iNES](https://www.nesdev.org/wiki/INES)
- [NES 2.0](https://www.nesdev.org/wiki/NES_2.0)
- [Mapper](https://www.nesdev.org/wiki/Mapper)
- [Emulator tests](https://www.nesdev.org/wiki/Emulator_tests)
- [Tricky-to-emulate games](https://www.nesdev.org/wiki/Tricky-to-emulate_games)
- [Errata](https://www.nesdev.org/wiki/Errata)

## Executive Summary

The NES is a tightly timed 8-bit system built around a Ricoh 2A03 CPU/APU
package in NTSC consoles, a Ricoh 2A07 CPU/APU package in PAL consoles, a Ricoh
2C02/2C07-family PPU, 2 KiB of internal CPU RAM, 2 KiB of internal PPU
nametable RAM, dynamic sprite OAM, and a cartridge bus that supplies PRG ROM,
CHR ROM or CHR RAM, optional PRG RAM, optional expansion audio, mirroring
control, and mapper-specific banking or IRQ hardware.

For emulator work, the important point is that there is no clean abstraction
boundary between chips. CPU reads and writes can affect PPU scroll latches,
DMC DMA can corrupt joypad and PPUDATA reads, PPU address bit A12 drives MMC3
scanline counters, mapper IRQs race with CPU interrupt polling, and register
open bus behavior leaks prior bus values. A correct emulator therefore needs a
coordinated scheduler, precise bus-side effects, cartridge-aware memory
mapping, and a strong test ROM suite.

For NES programming, the important point is that the machine is simple but
time-constrained. Most VRAM and OAM updates must be done during vblank or
carefully timed forced blanking. Game logic normally runs in the main thread,
while NMI performs small buffered updates. Mappers expand PRG/CHR capacity but
also introduce reset-vector, fixed-bank, mirroring, bus-conflict, and IRQ
constraints that software must obey.

## System Architecture

### Major Components

| Component | Role | Emulator-facing concerns |
|---|---|---|
| 2A03 / 2A07 CPU | 6502-derived CPU, APU, controller I/O, DMA | No decimal mode, unofficial opcodes, interrupt polling, read/write cycle type, DMA halts |
| 2C02 / 2C07 PPU | Video generator, PPU bus master, OAM, palette RAM | Dot timing, scrolling latches, sprite evaluation, open bus, PPUDATA buffer, NMI |
| Cartridge | PRG/CHR storage, mapper, mirroring, IRQ, RAM, audio | Bank switching, bus conflicts, mapper IRQs, save RAM, NES 2.0 metadata |
| Controllers / expansion devices | Serial input through $4016/$4017 | Strobe protocol, DMC read conflict, device variants |
| Frontend analog path | Composite/RF video and audio output | NTSC artifacts, color emphasis, nonlinear mixer, filters |

### Clock Domains

The CPU page gives the core timing families:

| Region | Master clock | CPU divisor | CPU rate | PPU divisor | PPU rate | CPU:PPU |
|---|---:|---:|---:|---:|---:|---:|
| NTSC NES/Famicom | 21.477272 MHz | 12 | 1.789773 MHz | 4 | 5.369318 MHz | 1:3 |
| PAL NES | 26.601712 MHz | 16 | 1.662607 MHz | 5 | 5.320342 MHz | 1:3.2 |
| Dendy-style | 26.601712 MHz | 15 | 1.773448 MHz | 5 | 5.320342 MHz | 1:3 |

NTSC has exactly three PPU dots per CPU cycle. PAL has 3.2 PPU dots per CPU
cycle, so a PAL emulator either needs fractional scheduling, a master-clock
scheduler, or a region-specific timing core.

### Address Spaces

The CPU and PPU use different buses. The cartridge participates in both.

CPU-visible address space:

| Range | Function |
|---|---|
| $0000-$07FF | 2 KiB internal RAM |
| $0800-$1FFF | Mirrors of internal RAM every $0800 |
| $2000-$2007 | PPU registers |
| $2008-$3FFF | PPU register mirrors every 8 bytes |
| $4000-$4013 | APU channel registers |
| $4014 | OAM DMA trigger |
| $4015 | APU status and channel enable |
| $4016-$4017 | Controller and APU frame counter registers |
| $4018-$401F | CPU test-mode / normally disabled APU and I/O |
| $4020-$5FFF | Cartridge expansion area, mapper registers, expansion RAM |
| $6000-$7FFF | Usually PRG RAM or mapper-specific area |
| $8000-$FFFF | Usually PRG ROM and mapper registers |

PPU-visible address space:

| Range | Function |
|---|---|
| $0000-$1FFF | Pattern tables, normally CHR ROM/RAM on cartridge |
| $2000-$2FFF | Nametables, usually internal CIRAM with cartridge-controlled mirroring |
| $3000-$3EFF | Mirror of $2000-$2EFF |
| $3F00-$3F1F | Palette RAM |
| $3F20-$3FFF | Palette mirrors |

## CPU: Ricoh 2A03 / 2A07

### Core Identity

The CPU core is a 6502 derivative. On NTSC systems the CPU and APU live in the
RP2A03. On PAL systems they live in the RP2A07. The visible software model is
close to a MOS 6502, with these major differences:

- Decimal mode is absent. The D flag can be set and cleared, but ADC and SBC do
  not perform BCD arithmetic.
- All standard and unofficial NMOS 6502 opcodes behave like a 6502, aside from
  decimal behavior.
- Every CPU cycle is externally a read or a write cycle. This matters for DMA,
  dummy accesses, mapper register writes, and bus conflicts.
- The CPU package includes APU, DMA, and controller port logic, making CPU
  behavior inseparable from system I/O timing.

### Programmer Registers

| Register | Width | Purpose |
|---|---:|---|
| A | 8 | Accumulator |
| X | 8 | Index register |
| Y | 8 | Index register |
| S | 8 | Stack pointer, stack page is $0100-$01FF |
| PC | 16 | Program counter |
| P | 8 | Status flags: N V unused B D I Z C |

The B flag is not a physical latch in the CPU status register. It exists in the
copy of P pushed by PHP and BRK. IRQ and NMI push P with B clear; PHP and BRK
push P with B set.

### Reset And Power-Up

Reset uses an interrupt-like seven-cycle sequence. Stack writes are suppressed,
but the stack pointer is decremented as if three pushes occurred. This is why
S is normally observed as $FD after reset. The I flag is set. PC is loaded from
$FFFC/$FFFD. RAM content is not guaranteed by the hardware and can vary by
console, temperature, power-off duration, and previous state.

For deterministic emulation, expose a configurable cold-boot RAM fill pattern
and a separate randomization mode. For tests, deterministic initial state is
usually preferable; for compatibility analysis, randomized power-up can reveal
software assumptions.

### Instructions And Addressing

The official 6502 instruction set contains loads, stores, arithmetic, logic,
shifts, rotates, branches, jumps, stack operations, flag operations, and
interrupt/control instructions. NES software also uses unofficial opcodes,
especially for compactness and timing. Implementing only official opcodes is not
enough for broad commercial compatibility.

Unofficial opcode families that matter:

- Combined ALU/RMW operations: SLO, RLA, SRE, RRA, DCP, ISC.
- Load/store combinations: LAX, SAX.
- Immediate ALU variants: ANC, ALR, ARR, XAA, AXS.
- Multi-byte NOPs with addressing-mode side effects.
- Stack/address-high-byte unstable opcodes: AHX, SHX, SHY, TAS, LAS.
- Halt opcodes, often called KIL/JAM/STP, which lock the CPU.

Emulation notes:

- Preserve dummy reads and dummy writes. They can trigger mapper side effects.
- Preserve read-modify-write double writes.
- Preserve the indirect JMP bug: JMP ($xxFF) reads the high byte from $xx00.
- Page-cross penalties apply to indexed reads. Writes perform their dummy read
  unconditionally and have fixed cycle counts.
- Branches have special interrupt polling behavior and optional page-fixup
  cycles.

### Interrupts

The CPU supports NMI, IRQ, BRK, and Reset. Reset is not maskable and has a
special write-suppressed sequence. NMI and IRQ are active-low external signals.

Key behavior from the CPU interrupts page:

- NMI is edge-sensitive. A high-to-low transition is latched internally.
- IRQ is level-sensitive. It remains pending only while the line is asserted
  and the I flag allows recognition at the sample point.
- The interrupt lines are effectively sampled so that, for most instructions,
  the line state at the end of the second-to-last CPU cycle determines whether
  an interrupt sequence starts after the instruction.
- If NMI and IRQ are both pending, NMI wins.
- Interrupt sequences do not poll for additional interrupts, so at least one
  handler instruction executes before another interrupt can be taken.
- CLI, SEI, and PLP can delay IRQ response because of when they change I
  relative to the poll. RTI restores I early enough for an immediate IRQ after
  RTI if an IRQ is pending and I is restored clear.
- Branch instructions poll differently than ordinary instructions. Correct
  behavior is tested by branch and cpu_interrupts_v2 test ROMs.

Interrupt vector addresses:

| Event | Vector |
|---|---|
| NMI | $FFFA/$FFFB |
| Reset | $FFFC/$FFFD |
| IRQ/BRK | $FFFE/$FFFF |

Interrupt-hijacking behavior is required for accuracy. If NMI arrives during
the early part of BRK or IRQ service, the CPU can push state as one event while
using the vector of another. This is visible to software that uses BRK or very
precise interrupt timing.

### DMA Interaction

DMA uses the CPU core's RDY halt behavior. The key rule is that DMA can halt
the CPU only on a CPU read cycle. If the CPU is writing, the DMA request waits
and tries again on the next CPU cycle. Read-modify-write instructions and
interrupt stack pushes can therefore delay DMA by multiple cycles.

When RDY halts the CPU on the 2A03, repeated reads of the halted address are
externally visible on no-operation DMA cycles. This creates hardware bugs when
the halted read is from a register with side effects, such as $2007, $4015,
$4016, or $4017.

## DMA

The 2A03 contains two DMA units:

- OAM DMA copies 256 bytes from one CPU page to PPU OAM through $2004.
- DMC DMA copies one byte from CPU memory into the DMC sample buffer.

DMA alternates between "get" cycles, where reads can happen, and "put" cycles,
where writes can happen. These are aligned to APU clock phases. The initial
get/put phase is not guaranteed across power cycles.

### OAM DMA

OAM DMA is triggered by writing a page number to $4014. It attempts to halt the
CPU on the first CPU cycle after the write. Once halted, it performs:

1. A halt cycle.
2. An optional alignment cycle if the first DMA access would not be on a get
   cycle.
3. 256 get/put pairs: read $xx00-$xxFF, write to $2004.

Total cost is normally 513 or 514 CPU cycles. If a DMC DMA overlaps, the total
can vary because DMC gets have priority over OAM gets.

### DMC DMA

DMC DMA occurs when sample playback is enabled, bytes remain, and the DMC sample
buffer needs a byte. It halts the CPU, performs a dummy cycle, optionally aligns
to a get cycle, and performs one memory read. Common duration is 3 or 4 CPU
cycles, but timing depends on load versus reload DMA, current get/put phase, and
whether the halt was delayed by writes.

Load DMA follows enabling DMC playback through $4015. Reload DMA follows
sample-buffer emptying during playback. Their scheduling differs, so an emulator
that treats all DMC reads as identical will fail timing-sensitive tests.

### DMC Register-Read Bugs

DMC DMA can corrupt reads from side-effect registers because the CPU repeats the
halted read during DMA no-operation cycles. Consequences include:

- Joypad bit deletion or duplicated shifts while reading $4016/$4017.
- PPUDATA read-buffer side effects while reading $2007.
- Extra reads of APU status/control addresses.

NES programs can work around joypad corruption with repeated reads and compare
logic, by scheduling reads away from DMC DMA, or by using OAM DMA for partial
synchronization. Emulators should reproduce the bug because commercial software
and test ROMs can observe it.

## PPU: Ricoh 2C02 / 2C07

### Core Identity

The PPU generates the composite video signal, owns palette RAM and OAM, and
masters the PPU memory bus. In normal NES cartridges, pattern data is on the
cartridge and nametable RAM is the console's 2 KiB CIRAM, addressed through
mapper-controlled mirroring. Some cartridges add VRAM, CHR RAM, CHR ROM banking,
or four-screen RAM.

### Frame Timing

NTSC timing:

- 262 scanlines per frame.
- 341 PPU dots per scanline.
- Visible scanlines: 0-239.
- Post-render scanline: 240.
- Vblank scanlines: 241-260.
- Pre-render scanline: 261.
- Vblank flag sets at scanline 241, dot 1.
- Odd frames with rendering enabled skip one dot near the end of the pre-render
  scanline, producing the familiar alternating 89342/89341-dot frame length.

PAL timing uses 312 scanlines and a different CPU:PPU ratio. Dendy-like systems
use PAL video timing characteristics with an NTSC-like CPU:PPU ratio and a
different vblank/post-render distribution.

### CPU-Facing Registers

The PPU exposes eight registers at $2000-$2007, mirrored through $3FFF, plus
OAMDMA at CPU address $4014.

| Register | Address | Access | Summary |
|---|---:|---|---|
| PPUCTRL | $2000 | W | NMI enable, sprite size, pattern table selects, VRAM increment, base nametable bits |
| PPUMASK | $2001 | W | Rendering enable, left-column masks, greyscale, color emphasis |
| PPUSTATUS | $2002 | R | Vblank, sprite 0 hit, sprite overflow; read clears vblank and write toggle |
| OAMADDR | $2003 | W | OAM address |
| OAMDATA | $2004 | R/W | OAM data |
| PPUSCROLL | $2005 | W x2 | X and Y scroll write pair |
| PPUADDR | $2006 | W x2 | VRAM address write pair |
| PPUDATA | $2007 | R/W | VRAM data through internal read buffer |
| OAMDMA | $4014 | W | Start 256-byte sprite DMA |

Important register facts:

- PPU writes to $2000, $2001, $2005, and $2006 are ignored for roughly the first
  frame after reset.
- Reading PPUSTATUS clears the vblank flag and the PPUSCROLL/PPUADDR write
  toggle.
- PPUSTATUS lower bits are open bus, not stable status bits.
- Enabling NMI while vblank is already set can cause an immediate NMI.
- PPUDATA reads are buffered except for palette reads. A read from pattern or
  nametable space returns the previous buffered byte and then reloads the
  buffer.
- Palette reads bypass the normal buffer but also affect it through the
  underlying nametable mirror behavior.
- Writes to OAMADDR during rendering can corrupt OAM on some PPU revisions.
- The PPU has a dynamic internal I/O latch; values decay over milliseconds and
  can be temperature-sensitive.

### Internal Scroll Model

The standard emulator model uses Loopy-style internal registers:

| Register | Width | Meaning |
|---|---:|---|
| v | 15 | Current VRAM address / rendering scroll address |
| t | 15 | Temporary VRAM address |
| x | 3 | Fine X scroll |
| w | 1 | First/second write toggle |

PPUCTRL writes update nametable bits in t. PPUSCROLL first write sets coarse X
and fine X. PPUSCROLL second write sets coarse Y and fine Y. PPUADDR first write
sets high address bits in t; second write sets low address bits and copies t to
v.

During rendering:

- Coarse X increments every 8 dots in fetch regions.
- Y increments around dot 256.
- Horizontal bits reload from t at dot 257.
- Vertical bits reload from t during dots 280-304 of the pre-render scanline.

Incorrect scroll timing breaks split screens, status bars, raster effects, and
games that write scroll registers outside vblank.

### Background Fetch Pipeline

Visible and pre-render scanlines perform repeating 8-dot fetch groups:

1. Nametable byte.
2. Attribute byte.
3. Pattern low byte.
4. Pattern high byte.

Each fetch takes two PPU dots. The fetched data is loaded into shift registers,
and pixels are generated from pattern and attribute shift-register outputs plus
fine X.

Dots 321-336 fetch the first two background tiles for the next scanline. Dots
337-340 perform additional nametable fetches. These "extra" fetches matter for
mapper timing and hardware observations even if they do not directly produce
visible pixels.

### Sprite Evaluation And Rendering

Primary OAM is 256 bytes: 64 sprites times 4 bytes. Secondary OAM is 32 bytes:
up to 8 sprites selected for the next scanline.

Per visible scanline:

- Dots 1-64 clear secondary OAM to $FF.
- Dots 65-256 evaluate primary OAM. The PPU alternates reads and writes while
  searching for sprites whose Y range overlaps the next scanline.
- After 8 sprites are found, secondary OAM is full. The hardware still scans
  primary OAM, but the overflow-check algorithm is buggy: it increments both
  sprite index and byte index in a diagonal pattern. This causes false positive
  and false negative sprite-overflow behavior.
- Dots 257-320 fetch sprite pattern data for the next scanline.

Sprite rendering uses eight sprite slots, each with a pattern shifter,
attributes, X counter, and priority. Sprite pixels can appear in front of or
behind background pixels depending on the priority bit, except that color 0 is
transparent for both background and sprite patterns.

Sprite 0 hit occurs when a non-transparent background pixel and a non-transparent
sprite 0 pixel overlap, subject to clipping and dot restrictions. It is a common
software timing source for status bars.

### Palette And Color

Palette RAM is 32 bytes, mirrored through $3FFF. Universal background entries
and sprite/background palette mirrors have special address aliases, including
$3F10 mirroring $3F00 and analogous entries. PPUMASK greyscale masks color
outputs, and color emphasis bits affect output differently across PPU variants
and video encoders.

Emulators usually choose one of these levels:

- Simple RGB lookup palette.
- Palette plus greyscale and emphasis adjustments.
- NTSC composite filter with dot crawl, artifacts, and phase behavior.
- PPU-model-specific palettes for 2C02, 2C03/2C04 RGB PPUs, PAL PPUs, and Vs.
  System variants.

### Open Bus And Decay

The PPU's internal CPU I/O data bus behaves like a dynamic latch. Writes to PPU
ports and reads from readable ports update it. Reading write-only registers or
unused PPUSTATUS bits returns the latched value. Bits can decay after a few to
tens of milliseconds. This is visible in open-bus tests and in a few software
edge cases.

## APU: Ricoh 2A03 / 2A07 Audio

### Channel Overview

The APU has five native sound channels:

| Channel | Registers | Main units |
|---|---|---|
| Pulse 1 | $4000-$4003 | Timer, duty sequencer, envelope, length counter, sweep |
| Pulse 2 | $4004-$4007 | Timer, duty sequencer, envelope, length counter, sweep |
| Triangle | $4008-$400B | Timer, 32-step sequencer, length counter, linear counter |
| Noise | $400C-$400F | Timer, LFSR, envelope, length counter |
| DMC | $4010-$4013 | Timer, DMA memory reader, sample buffer, shifter, 7-bit output |

$4015 controls channel enables and status reads. $4017 controls the frame
counter.

### Length Counter Table

The length counter load table indexed by the high five bits of channel length
register writes is:

| Index | Value | Index | Value | Index | Value | Index | Value |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 10 | 1 | 254 | 2 | 20 | 3 | 2 |
| 4 | 40 | 5 | 4 | 6 | 80 | 7 | 6 |
| 8 | 160 | 9 | 8 | 10 | 60 | 11 | 10 |
| 12 | 14 | 13 | 12 | 14 | 26 | 15 | 14 |
| 16 | 12 | 17 | 16 | 18 | 24 | 19 | 18 |
| 20 | 48 | 21 | 20 | 22 | 96 | 23 | 22 |
| 24 | 192 | 25 | 24 | 26 | 72 | 27 | 26 |
| 28 | 16 | 29 | 28 | 30 | 32 | 31 | 30 |

### Frame Counter

The frame counter clocks envelopes, triangle linear counter, length counters,
and pulse sweep units. It has 4-step and 5-step modes.

4-step mode:

- Quarter-frame clocks: envelope and triangle linear counter.
- Half-frame clocks: length counters and sweep units.
- Can generate a frame IRQ on the final step if IRQ inhibit is clear.

5-step mode:

- Adds a fifth step.
- Does not generate frame IRQ.
- Can immediately clock units after writing $4017 with mode bit set, after the
  documented delay/alignment behavior.

The frame counter is not inherently synchronized with PPU NMI. Some games
manually write $4017 once per frame to maintain audio timing relative to video.

### Pulse Channels

Pulse channels have four duty patterns, an 11-bit timer, envelope/constant
volume, length counter, and sweep. Frequency is:

```text
frequency = cpu_clock / (16 * (timer + 1))
```

Pulse channels are silenced when the timer period is too small, when the length
counter is zero, or when sweep overflow rules silence them. Pulse 1 and Pulse 2
have slightly different negate behavior in the sweep unit.

Writing $4003 or $4007 loads length, updates timer high bits, restarts the
envelope, and resets the duty sequencer phase. Audio engines usually avoid
writing high timer registers for vibrato unless a phase reset click is desired
or acceptable.

### Triangle Channel

The triangle channel has a 32-step waveform and no volume control. It is gated
by both the length counter and linear counter. Its timer is clocked every CPU
cycle, and an equivalent timer value produces a pitch one octave below a pulse
channel. When silenced by counters, it holds its last output rather than forcing
zero.

### Noise Channel

The noise channel uses an LFSR with two feedback modes. Mode 0 produces the
longer pseudo-random sequence; mode 1 taps a shorter sequence and sounds more
periodic or metallic.

NTSC noise period table in CPU cycles:

```text
4, 8, 16, 32, 64, 96, 128, 160,
202, 254, 380, 508, 762, 1016, 2034, 4068
```

PAL differs, so region selection must affect APU timing.

### DMC Channel

DMC plays 1-bit delta-coded samples from CPU memory. Sample address and length:

```text
address = $C000 + ($4012 * 64)
length  = ($4013 * 16) + 1
```

Each bit increments or decrements a 7-bit output counter by 2, clamped to
0..127. The DMC output value is always present in the mixer; the enable bit
controls automatic sample fetching rather than muting the output counter.

NTSC DMC period table in CPU cycles:

```text
428, 380, 340, 320, 286, 254, 226, 214,
190, 160, 142, 128, 106, 85, 72, 54
```

DMC can loop or generate IRQ at sample end. DMC DMA is one of the most important
CPU/APU/bus accuracy points.

### Mixer

The APU output is nonlinear. A commonly used approximation is:

```text
pulse = 0 if p1 + p2 == 0
      = 95.88 / ((8128 / (p1 + p2)) + 100)

tnd = 0 if t / 8227 + n / 12241 + d / 22638 == 0
    = 159.79 / (1 / (t / 8227 + n / 12241 + d / 22638) + 100)
```

The final console path applies additional high-pass and low-pass filtering.
Expansion audio on Famicom cartridges enters through the cartridge audio return
path and must be mixed at mapper-defined relative levels.

### Expansion Audio

Important expansion audio sources:

| Hardware | Audio capability |
|---|---|
| Famicom Disk System | Wavetable plus modulation unit |
| MMC5 | Two extra pulse channels and PCM-like output |
| VRC6 | Two pulse channels plus sawtooth |
| VRC7 | Six-channel FM based on Yamaha OPLL-like design |
| Namco 163 | Multiple wavetable channels, time-multiplexed |
| Sunsoft 5B / FME-7 | AY-3-8910-style square/noise/envelope |

NES front-loaders do not normally route cartridge expansion audio without
hardware modification. Famicom and some adapters do. Emulators generally expose
it when the mapper has it, with optional regional output differences.

## Controllers And Input Devices

Standard controllers are read through $4016 and $4017. The CPU writes a strobe
bit, then reads one bit at a time from each controller shift register.

Standard controller order:

```text
A, B, Select, Start, Up, Down, Left, Right
```

Implementation notes:

- A write with strobe bit 1 continuously reloads controller state.
- Transitioning strobe to 0 latches state for serial reads.
- Reads after eight bits typically return 1 on standard controllers, but device
  and open-bus behavior can vary.
- DMC DMA can corrupt reads by causing repeated side-effect reads, so robust
  NES software often reads controllers twice and compares results.

The wiki also documents many non-standard input devices: Zapper, Power Pad,
Four Score, Famicom expansion controllers, keyboards, paddles, mice, barcode
readers, mahjong controllers, and regional or game-specific peripherals. NES
2.0 byte 15 can identify a default expansion device for some software.

## Cartridge Hardware

### Cartridge Bus Responsibilities

Cartridges provide:

- PRG ROM mapped into CPU $8000-$FFFF, often banked.
- Optional PRG RAM or non-volatile RAM, often at $6000-$7FFF.
- CHR ROM or CHR RAM mapped into PPU $0000-$1FFF.
- Nametable mirroring control through CIRAM A10 or on-cartridge VRAM.
- Mapper registers decoded from CPU and sometimes PPU addresses.
- IRQ generation, usually based on CPU cycles, PPU A12 transitions, or scanline
  observation.
- Optional expansion audio or special peripherals.

### Nametable Mirroring

The console has 2 KiB of internal nametable RAM, enough for two physical
nametables. The PPU address range exposes four logical nametables. Cartridges
select how logical nametables map to physical RAM:

| Arrangement | Common name confusion | CIRAM A10 source |
|---|---|---|
| Vertical arrangement | Often called horizontal mirroring | PPU A10 |
| Horizontal arrangement | Often called vertical mirroring | PPU A11 |
| One-screen lower/upper | Single physical nametable | Fixed 0 or 1 |
| Four-screen | Cartridge VRAM | Cartridge handles all four |

Names are historically confusing because some sources describe arrangement and
others describe mirroring. In code and tests, prefer explicit CIRAM A10 behavior
or "vertical arrangement" / "horizontal arrangement."

### Bus Conflicts

Discrete mappers may not disable PRG ROM output during writes to ROM space.
When CPU and ROM both drive the data bus, the effective written value can be the
bitwise AND of the CPU value and ROM byte. Many games deliberately write values
to ROM addresses containing the same byte to avoid conflicts.

Emulators should support mapper-specific bus conflicts where documented and use
NES 2.0 submappers when available to disambiguate boards.

## ROM And Music File Formats

### iNES

iNES is the de facto `.nes` file format. It consists of:

1. 16-byte header.
2. Optional 512-byte trainer.
3. PRG ROM in 16 KiB units.
4. CHR ROM in 8 KiB units, or CHR RAM when size is zero.
5. Optional PlayChoice data.
6. Sometimes a non-standard trailing title.

Header essentials:

| Byte | Meaning |
|---:|---|
| 0-3 | "NES" followed by $1A |
| 4 | PRG ROM size in 16 KiB units |
| 5 | CHR ROM size in 8 KiB units |
| 6 | Mapper low nibble, mirroring, battery, trainer, alternate nametable bit |
| 7 | Mapper high nibble, Vs./PlayChoice, NES 2.0 signature bits |
| 8 | PRG RAM size extension |
| 9 | TV system extension |
| 10 | Unofficial TV/PRG RAM/bus-conflict hints |
| 11-15 | Padding in iNES; often polluted by old tools |

If bytes 7-15 contain old tool signatures such as "DiskDude!" and the header is
not NES 2.0, mapper high bits can be corrupted. A robust loader should validate
the header and file size instead of blindly trusting every field.

### NES 2.0

NES 2.0 extends iNES while keeping the same file extension and magic bytes. It
adds:

- 12-bit mapper number.
- 4-bit submapper.
- Larger PRG/CHR sizes, including exponent-multiplier encoding.
- Separate volatile and non-volatile PRG RAM sizes.
- Separate volatile and non-volatile CHR RAM sizes.
- Region and CPU/PPU timing flags.
- Vs. System and extended console type metadata.
- Miscellaneous ROM count.
- Default expansion device.

NES 2.0 should be preferred for new dumps, homebrew, and emulator test ROMs
because it can describe boards that iNES cannot unambiguously encode.

### UNIF, NSF, FDS, QD

- UNIF was an alternate board-name-oriented format. NES 2.0 supersedes it for
  most use cases.
- NSF stores NES music driver code and data for playback. NSF players emulate
  enough CPU/APU/mapper behavior to run the music routine.
- FDS images represent Famicom Disk System disk data and must be paired with
  BIOS behavior, disk timing, IRQs, motor state, and FDS audio.
- QD relates to Quick Disk-derived media and FDS ecosystem material.

## Common Mapper Families

The Nesdev mapper pages are extensive. This table focuses on emulator behavior
that affects compatibility.

| Mapper | iNES | Key behavior |
|---|---:|---|
| NROM | 0 | No banking; 16 or 32 KiB PRG ROM; 8 KiB CHR; fixed mirroring; no IRQ |
| MMC1 / SxROM | 1, 155, 105 | Serial 5-write register interface; PRG/CHR banking; one-screen/H/V mirroring; PRG RAM enable; consecutive-cycle write ignore behavior |
| UxROM | 2, 94, 180 | 16 KiB switchable PRG at $8000, fixed last bank at $C000; CHR RAM; bus conflicts on original boards |
| CNROM | 3, 185 | Fixed PRG; 8 KiB CHR ROM banking; AND-type bus conflicts; mapper 185 CHR-enable copy protection |
| MMC3 / TxROM | 4, 118, 119 | 8 KiB/16 KiB PRG banking; 1 KiB/2 KiB CHR banking; PPU A12-filtered scanline IRQ; mirroring and PRG RAM protect |
| MMC5 / ExROM | 5 | Advanced PRG/CHR banking; ExRAM; extended attributes; split mode; multiplier; scanline IRQ; expansion audio |
| AxROM | 7 | 32 KiB PRG banking; one-screen mirroring select; usually CHR RAM |
| MMC2 / PxROM | 9 | Punch-Out!! latch-based CHR bank switching on PPU tile fetches |
| MMC4 / FxROM | 10 | MMC2-like CHR latches plus PRG banking, used by Famicom Wars / Fire Emblem |
| Color Dreams | 11 | Discrete PRG/CHR banking; often bus conflicts |
| CPROM | 13 | Fixed PRG; CHR RAM bank switching for upper pattern table |
| Namco 163 | 19 | PRG/CHR banking; IRQ counter; optional multi-channel wavetable audio |
| VRC2/VRC4 | 21, 22, 23, 25 | Konami PRG/CHR banking; VRC4 IRQ; address-line variants via submappers |
| VRC6 | 24, 26 | Konami banking, IRQ, and expansion audio |
| VRC7 | 85 | Konami banking, IRQ, and FM expansion audio |
| BNROM / NINA variants | 34 | Board-dependent PRG banking and sometimes CHR banking; NES 2.0 needed |
| GxROM | 66 | 32 KiB PRG and 8 KiB CHR bank selection |
| Sunsoft FME-7 / 5B | 69 | Command/data mapper interface; IRQ counter; optional AY-style audio |
| FDS | 20 | Disk BIOS, disk data, timer IRQ, transfer/status registers, FDS wavetable audio |

### MMC1 Details

MMC1 writes use a serial load register mapped across $8000-$FFFF. Writes with
bit 7 set reset the shift register and force PRG mode toward fixed-last-bank
behavior. Otherwise, five writes shift bit 0, least significant bit first. On
the fifth write, address bits select the target internal register:

| CPU range | Register |
|---|---|
| $8000-$9FFF | Control |
| $A000-$BFFF | CHR bank 0 |
| $C000-$DFFF | CHR bank 1 |
| $E000-$FFFF | PRG bank |

The consecutive-cycle write ignore rule matters. Read-modify-write instructions
write twice; MMC1 ignores the second consecutive-cycle data write except for
reset behavior. Some games rely on this.

### MMC3 Details

MMC3 uses bank select/data registers for PRG and CHR banking. Its IRQ counter is
clocked by filtered rising edges on PPU A12, normally once per visible scanline
when background and sprite pattern tables are arranged in the common split. The
A12 low time filter and the reload/counter/enable semantics are common emulator
failure points.

Accurate MMC3 requires:

- PPU fetch timing that produces real A12 transitions, including dummy sprite
  fetches.
- Correct handling of background/sprite pattern table selection.
- Correct IRQ reload behavior and acknowledgement timing.
- Mapper revision differences where relevant.

### MMC5 Details

MMC5 is the most complex Nintendo mapper. Major features include:

- Multiple PRG banking modes.
- Multiple CHR banking modes.
- ExRAM used as extra nametable RAM, attribute data, or general RAM depending
  on mode.
- Extended attribute mode, allowing finer color attribute selection.
- Split-screen support.
- Scanline IRQ based on observing PPU rendering activity.
- 8x8 multiplier registers.
- Extra pulse audio and PCM-like output.

An emulator should implement MMC5 in layers and test each behavior separately.
Many games use only subsets of the chip, but partial implementations can still
break high-profile titles.

### Konami VRC Family

VRC mappers have address-line wiring variants. The same conceptual register can
appear at different CPU addresses depending on the board. NES 2.0 submappers
help identify variants. VRC2/VRC4 provide PRG/CHR banking and, for VRC4, IRQs.
VRC6 adds expansion audio. VRC7 adds FM audio and its own audio-register
semantics.

### Namco 163

Namco 163 supports PRG/CHR banking, IRQs, and optional wavetable audio. Audio is
time-multiplexed across channels; enabling more channels reduces per-channel
update rate and changes audible aliasing/noise. Some emulators allow filtering
or level adjustment because real hardware output varies by board and mixing
path.

### Sunsoft FME-7 / 5B

FME-7 uses command and parameter registers to control PRG banks, CHR banks,
mirroring, RAM enable, and IRQ. Sunsoft 5B variants add AY-style expansion
audio. IRQ behavior is CPU-cycle based and must be integrated with CPU interrupt
polling.

## Famicom Disk System

FDS adds a disk drive, BIOS ROM, RAM adapter, timer IRQ, disk transfer
registers, motor/control status, and expansion audio. Accurate FDS emulation is
more than loading bytes from an image:

- BIOS behavior and license screen matter.
- Disk side changes and insertion/ejection state matter.
- Transfer timing and IRQs can be software-visible.
- The wavetable audio unit has modulation behavior distinct from the base APU.
- Save/write behavior should preserve modified disk data safely.

## Vs. System, PlayChoice, And PPU Variants

Arcade-derived NES hardware can use different PPUs, palettes, input devices,
coin counters, DIP switches, and memory maps. Vs. System games may require
specific PPU palette variants and security behavior. PlayChoice-10 data can be
present in iNES files but is often ignored by ordinary NES emulators.

NES 2.0 contains fields for Vs. System type, PPU variant, and extended console
type. Use them when available instead of relying on ROM names or checksums.

## NES Programming Model

### Initialization

Typical startup code:

1. Disable IRQs with SEI and clear decimal mode with CLD.
2. Initialize stack pointer.
3. Disable APU frame IRQ and DMC IRQ.
4. Disable PPU rendering and NMI.
5. Wait for PPU power-up, commonly by waiting for vblank twice.
6. Clear RAM and initialize zero page/state.
7. Upload palettes, nametables, CHR RAM data if needed, and initial OAM.
8. Configure mapper banks and mirroring.
9. Enable NMI and rendering at a controlled time.

PPU power-up waiting is required because early writes to several PPU registers
are ignored.

### Main Thread And NMI Thread

A robust game loop usually treats NMI as a short video-update thread:

- Main thread computes game logic and prepares buffers.
- NMI copies OAM with DMA, applies palette updates, applies VRAM update lists,
  sets scroll, and acknowledges frame completion.
- Main thread waits on a frame flag rather than doing large work inside NMI.

Vblank time is limited. On NTSC, only about 2270 CPU cycles are available before
rendering resumes, and OAM DMA consumes 513 or 514 cycles. Large nametable
updates require buffering, compression, forced blanking, or spreading work over
multiple frames.

### VRAM Updates

With rendering enabled, safe VRAM updates normally happen only during vblank.
During forced blanking, rendering can be disabled to allow larger updates, but
this creates visible blank time if used mid-frame. PPUDATA increments by 1 or 32
depending on PPUCTRL, so update list formats often group horizontal and vertical
runs separately.

### Scrolling And Raster Effects

Scrolling requires coordinated writes to PPUCTRL, PPUSCROLL, and PPUADDR after
resetting the write toggle by reading PPUSTATUS. Split-screen effects can use:

- Sprite 0 hit polling.
- Mapper IRQs, especially MMC3.
- Timed code loops.
- Mid-frame writes to scroll and control registers.

Mid-frame effects are fragile because CPU/PPU alignment, odd-frame skip, NMI
timing, and mapper IRQ behavior all matter.

### Sprite Management

Hardware draws at most 8 sprites per scanline. Games reduce flicker by rotating
sprite order in OAM, but software should avoid hardcoding OAM addresses because
OAM cycling and metasprite allocation can change. 8x16 sprites use pattern table
selection differently than 8x8 sprites, with tile index bit 0 selecting pattern
table.

### Controller Reading

Basic controller reading:

1. Write 1 to $4016 strobe.
2. Write 0 to $4016 strobe.
3. Read $4016 eight times for controller 1.
4. Read $4017 eight times for controller 2.

Because DMC DMA can corrupt reads, games using DPCM often use repeated reads
until two match, avoid reading during DMC activity, or use known-safe timing.

### Audio Programming

Audio engines generally maintain shadow registers in RAM and write APU
registers at a stable cadence. Avoid unnecessary high-byte timer writes to pulse
channels to prevent phase-reset artifacts. If the game uses DMC, account for DMA
cycle stealing and controller-read conflicts. If using expansion audio, detect
or assume the target cartridge and mix levels accordingly.

### Mapper Programming

Common mapper programming rules:

- Keep reset, NMI, IRQ, and bank-switch trampolines in a fixed bank.
- Initialize mapper state before enabling interrupts.
- For bus-conflict mappers, write a value to an address containing the same
  value.
- For MMC1, avoid unintended consecutive-cycle writes and reset the serial port
  deliberately.
- For MMC3, acknowledge and rearm IRQs in a stable order.
- Preserve save RAM only when the board actually has non-volatile memory.

## Emulator Architecture Guidance

### Scheduler

Recommended models:

- Master-clock scheduler: most general for NTSC, PAL, Dendy, and sub-cycle
  alignment, but more complex.
- Dot-lockstep scheduler: simple and accurate for NTSC if every CPU cycle ticks
  exactly three PPU dots.
- Catch-up scheduler: faster and simpler but risky for mid-instruction PPU
  effects, DMA, and mapper IRQs.

For this Rust workspace, the safest architecture is a deterministic scheduler
that advances CPU one cycle at a time, advances the PPU by the correct number of
dots for the region, advances APU and mapper timing in the same bus cycle, and
performs all bus side effects in access order.

### CPU Bus

The CPU bus should own or coordinate:

- Internal RAM mirrors.
- PPU register accesses and mirrors.
- APU register accesses.
- Controller ports and strobe state.
- DMA scheduling and halting.
- Cartridge CPU reads/writes.
- Open bus value where observable.
- IRQ line aggregation from APU, mapper, FDS, and other devices.
- NMI edge line from PPU.

Avoid giving the CPU direct access to subsystem internals. The CPU should emit
reads and writes; the bus should decide side effects.

### PPU Bus

The PPU bus should route:

- $0000-$1FFF pattern accesses through mapper CHR banking.
- $2000-$2FFF nametable accesses through mirroring or cartridge VRAM.
- $3F00-$3FFF palette accesses internally to the PPU.
- PPU A12 transitions to mapper IRQ logic.

PPU fetches that appear useless to the final pixel output can still matter to
mappers. Do not skip dummy fetches if mapper accuracy is a goal.

### APU And Audio Output

The APU should be deterministic at CPU-cycle granularity. For host audio:

- Generate band-limited or high-rate internal samples.
- Apply nonlinear mixer and output filtering.
- Resample to the host sample rate with stable buffering.
- Keep audio generation independent from wall-clock frame pacing.

DMC DMA should be serviced by the system bus, not hidden inside the APU, because
it steals CPU cycles and affects CPU-visible reads.

### Mapper Interface

A mapper trait or enum should cover:

- CPU read/write.
- PPU read/write.
- Nametable mirroring decision.
- IRQ line state.
- Per-CPU-cycle hooks where needed.
- PPU A12 or PPU-address notification where needed.
- Save-state serialization.
- Battery-backed memory exposure.
- Optional expansion audio sample/output.

Mapper implementations should be tested with both ROM fixtures and focused unit
tests for register state, bank selection, IRQ counters, and mirroring.

### Save States

Save states must include every stateful latch that can affect future execution:

- CPU registers, internal instruction state if mid-instruction save is allowed,
  pending interrupts, cycle counters.
- PPU dot, scanline, frame parity, v/t/x/w, read buffer, OAM, secondary OAM,
  palette RAM, open-bus latch and decay state, shift registers if mid-frame
  save is allowed.
- APU channel counters, timers, sequencer positions, frame counter, DMC DMA
  state, filter/sample accumulators if audio continuity matters.
- Mapper registers, IRQ counters, RAM, latches, audio state.
- DMA unit state and get/put phase.
- Controller shift state.

Version save states explicitly. Mapper and core changes should either migrate
old states or reject them clearly.

## Test Strategy

The Nesdev emulator-test page is the core source index for validation ROMs.
For an accuracy-oriented emulator, use layered tests:

| Area | Useful ROM families |
|---|---|
| CPU instructions | nestest, instr_test_v5, instr_misc, cpu_timing_test6 |
| CPU interrupts | cpu_interrupts_v2, branch_timing_tests |
| CPU dummy access | cpu_dummy_reads, cpu_dummy_writes |
| PPU vblank/NMI | ppu_vbl_nmi, vbl_nmi_timing, nmi_sync |
| PPU read buffer/open bus | ppu_read_buffer, ppu_open_bus |
| Sprites | sprite_hit_tests, sprite_overflow_tests, oam_read, oam_stress |
| DMA | sprdma_and_dmc_dma, dmc_dma_during_read4 |
| APU | apu_test, blargg_apu, apu_mixer, test_apu_env, test_apu_sweep |
| Mappers | mmc3_test, mmc3_test_2, Holy Mapperel, mapper-specific homebrew tests |
| Integration | Curated commercial-game attract-mode and gameplay snapshots |

Testing recommendations:

- Compare CPU logs against known-good traces before integrating PPU/APU.
- Use deterministic power-up mode for CI.
- Add visual snapshot tests after PPU basics pass.
- Add audio spectral or envelope tests for APU changes.
- Test mapper IRQ timing with trace logs, not just final screenshots.
- Keep real commercial ROMs outside the repository unless licensing permits.

## Tricky Compatibility Themes

The wiki's tricky-game and errata pages are best treated as a compatibility
risk catalog. Common failure themes:

- CPU interrupt polling edge cases.
- PPUSTATUS vblank suppression and immediate NMI on enabling NMI.
- Sprite 0 hit timing.
- Sprite overflow bug behavior.
- OAMADDR/OAMDATA behavior during rendering.
- PPUDATA read-buffer and palette-read behavior.
- PPU open bus and decay.
- MMC3 A12 IRQ filtering.
- MMC5 scanline and ExRAM behavior.
- DMC DMA interaction with controller and PPUDATA reads.
- Bus conflicts on discrete mappers.
- Region timing assumptions in games designed for NTSC, PAL, or Dendy.
- Expansion audio levels and channel timing.
- Mapper variants that require NES 2.0 submappers or board-specific metadata.

## Hardware Errata And Quirks Checklist

Use this as an implementation checklist:

- CPU lacks decimal mode but preserves D flag storage.
- Unofficial opcodes execute.
- Every CPU cycle is a read or write.
- Dummy reads/writes happen at documented addresses.
- IRQ/NMI sample timing and hijacking behavior are modeled.
- Reset suppresses writes but decrements stack pointer.
- DMA halts only on read cycles.
- OAM DMA costs 513/514 cycles without DMC overlap.
- DMC DMA has load/reload timing and register-read side effects.
- PPU ignores early writes after reset.
- PPUSTATUS read clears vblank and w toggle.
- PPU vblank flag/NMI race behavior is modeled.
- PPUADDR/PPUSCROLL write pair behavior is modeled through v/t/x/w.
- PPUDATA read buffer and palette bypass behavior are modeled.
- Palette mirrors are modeled.
- Open bus exists for PPU unused bits and write-only reads.
- NTSC odd-frame dot skip occurs only when rendering is enabled.
- Sprite evaluation overflow bug is modeled.
- Sprite 0 hit restrictions are modeled.
- Mapper bus conflicts are modeled where appropriate.
- Mapper IRQ line state reaches CPU with correct polling timing.
- Save RAM is persisted only for boards that actually have it.

## Source Map For Continued Expansion

The report should be updated from these page clusters when deeper coverage is
needed:

### Hardware Reference Cluster

- [2A03](https://www.nesdev.org/wiki/2A03)
- [CPU](https://www.nesdev.org/wiki/CPU)
- [CPU memory map](https://www.nesdev.org/wiki/CPU_memory_map)
- [CPU registers](https://www.nesdev.org/wiki/CPU_registers)
- [Status flags](https://www.nesdev.org/wiki/Status_flags)
- [CPU interrupts](https://www.nesdev.org/wiki/CPU_interrupts)
- [Instruction reference](https://www.nesdev.org/wiki/Instruction_reference)
- [Unofficial opcodes](https://www.nesdev.org/wiki/Unofficial_opcodes)
- [PPU](https://www.nesdev.org/wiki/PPU)
- [PPU registers](https://www.nesdev.org/wiki/PPU_registers)
- [PPU memory map](https://www.nesdev.org/wiki/PPU_memory_map)
- [PPU rendering](https://www.nesdev.org/wiki/PPU_rendering)
- [PPU scrolling](https://www.nesdev.org/wiki/PPU_scrolling)
- [PPU sprite evaluation](https://www.nesdev.org/wiki/PPU_sprite_evaluation)
- [PPU power up state](https://www.nesdev.org/wiki/PPU_power_up_state)
- [APU](https://www.nesdev.org/wiki/APU)
- [APU Registers](https://www.nesdev.org/wiki/APU_registers)
- [APU Frame Counter](https://www.nesdev.org/wiki/APU_Frame_Counter)
- [APU Pulse](https://www.nesdev.org/wiki/APU_Pulse)
- [APU Triangle](https://www.nesdev.org/wiki/APU_Triangle)
- [APU Noise](https://www.nesdev.org/wiki/APU_Noise)
- [APU DMC](https://www.nesdev.org/wiki/APU_DMC)
- [APU Mixer](https://www.nesdev.org/wiki/APU_Mixer)
- [DMA](https://www.nesdev.org/wiki/DMA)
- [Input devices](https://www.nesdev.org/wiki/Input_devices)

### Cartridge And File Format Cluster

- [Mapper](https://www.nesdev.org/wiki/Mapper)
- [iNES](https://www.nesdev.org/wiki/INES)
- [NES 2.0](https://www.nesdev.org/wiki/NES_2.0)
- [UNIF](https://www.nesdev.org/wiki/UNIF)
- [NSF](https://www.nesdev.org/wiki/NSF)
- [FDS file format](https://www.nesdev.org/wiki/FDS_file_format)
- [FDS disk format](https://www.nesdev.org/wiki/FDS_disk_format)
- [Cartridge board reference](https://www.nesdev.org/wiki/Cartridge_board_reference)
- [Nametable mirroring](https://www.nesdev.org/wiki/Mirroring)
- [Bus conflicts](https://www.nesdev.org/wiki/Bus_conflict)

### Mapper Cluster

- [NROM](https://www.nesdev.org/wiki/NROM)
- [MMC1](https://www.nesdev.org/wiki/MMC1)
- [UxROM](https://www.nesdev.org/wiki/UxROM)
- [CNROM](https://www.nesdev.org/wiki/CNROM)
- [MMC3](https://www.nesdev.org/wiki/MMC3)
- [MMC5](https://www.nesdev.org/wiki/MMC5)
- [AxROM](https://www.nesdev.org/wiki/AxROM)
- [MMC2](https://www.nesdev.org/wiki/MMC2)
- [MMC4](https://www.nesdev.org/wiki/MMC4)
- [VRC2 and VRC4](https://www.nesdev.org/wiki/VRC2_and_VRC4)
- [VRC6](https://www.nesdev.org/wiki/VRC6)
- [VRC7](https://www.nesdev.org/wiki/VRC7)
- [Namco 163](https://www.nesdev.org/wiki/Namco_163)
- [Sunsoft FME-7](https://www.nesdev.org/wiki/Sunsoft_FME-7)
- [Family Computer Disk System](https://www.nesdev.org/wiki/Family_Computer_Disk_System)

### Programming Cluster

- [Programming guide](https://www.nesdev.org/wiki/Programming_guide)
- [Init code](https://www.nesdev.org/wiki/Init_code)
- [The frame and NMIs](https://www.nesdev.org/wiki/The_frame_and_NMIs)
- [Controller reading](https://www.nesdev.org/wiki/Controller_reading)
- [PPU scrolling](https://www.nesdev.org/wiki/PPU_scrolling)
- [Programming mappers](https://www.nesdev.org/wiki/Programming_mappers)
- [Programming NROM](https://www.nesdev.org/wiki/Programming_NROM)
- [Programming UNROM](https://www.nesdev.org/wiki/Programming_UNROM)
- [Programming MMC1](https://www.nesdev.org/wiki/Programming_MMC1)
- [Programming MMC3](https://www.nesdev.org/wiki/Programming_MMC3)
- [APU basics](https://www.nesdev.org/wiki/APU_basics)
- [Audio drivers](https://www.nesdev.org/wiki/Audio_drivers)
- [Cycle counting](https://www.nesdev.org/wiki/Cycle_counting)
- [6502 assembly optimisations](https://www.nesdev.org/wiki/6502_assembly_optimisations)
- [Compression](https://www.nesdev.org/wiki/Compression)
- [Limitations](https://www.nesdev.org/wiki/Limitations)

### Emulation Cluster

- [Emulators](https://www.nesdev.org/wiki/Emulators)
- [Emulator tests](https://www.nesdev.org/wiki/Emulator_tests)
- [Tricky-to-emulate games](https://www.nesdev.org/wiki/Tricky-to-emulate_games)
- [Game bugs](https://www.nesdev.org/wiki/Game_bugs)
- [Sprite overflow games](https://www.nesdev.org/wiki/Sprite_overflow_games)
- [Colour-emphasis games](https://www.nesdev.org/wiki/Colour-emphasis_games)
- [Colour $0D games](https://www.nesdev.org/wiki/Colour_$0D_games)
- [Expansion audio games](https://www.nesdev.org/wiki/Expansion_audio_games)
- [Visual 2C02](https://www.nesdev.org/wiki/Visual_2C02)

## Maintenance Protocol

To keep this report useful:

1. When a RustyNES subsystem changes, update the matching section and link the
   exact Nesdev page used to justify behavior.
2. When a mapper is implemented, expand the mapper row into a subsection with
   register map, power-up state, IRQ behavior, PRG/CHR banking, mirroring, bus
   conflicts, and tests.
3. When a test ROM fails, add the failure theme to the compatibility checklist
   and link the relevant Nesdev test or forum thread.
4. Do not paste entire wiki pages. Summarize behavior, cite the page, and link
   to primary diagrams or tables.
5. Keep region-specific behavior explicit: NTSC, PAL, Dendy, Vs. System, and
   clone consoles are not interchangeable.

## Iteration Log

- 2026-05-20: Initial comprehensive synthesis from the Nesdev main page, NES
  reference guide, programming guide, CPU, PPU, APU, DMA, file format, mapper,
  emulator-test, tricky-game, and errata clusters.

# RustyNES v2 — Deep Research Report

**Generated:** 2026-05-10
**Mode:** Autonomous (topic-driven, no starter document)
**Source count:** 19 fetched primary sources + 30+ surfaced via search
**Topic:** Cycle-accurate emulation of the Nintendo Entertainment System (NES / Famicom) on modern x86_64 / aarch64 architectures, implemented in Rust.

---

## Executive summary

The Nintendo Entertainment System (Famicom in Japan, 1983; NES worldwide, 1985) is built around a 1.79 MHz Ricoh 2A03 CPU (a 6502 derivative with the integrated audio processing unit) and a Ricoh 2C02 picture processing unit running at three times the CPU clock. The console's library spans roughly 1,000 commercial titles using over 250 distinct cartridge mapper ICs, ranging from trivial NROM (no banking) to MMC5 (the most elaborate official mapper, with extra audio channels, extended attributes, and a precise scanline IRQ). State-of-the-art emulation today is dominated by the closed-source-but-well-documented Mesen2 (C++/C#) and the higan/ares NES core (C++), both of which schedule components in tight lockstep at PPU-dot resolution. The NESdev community wiki is the canonical reference for hardware behavior, often more accurate than the original Nintendo documentation due to two decades of die-shot-level reverse engineering (notably the Visual 2C02 / Visual 6502 transistor-accurate models).

The principal engineering challenges for a *new* cycle-accurate emulator are (a) the CPU's interrupt-hijacking and edge-detection semantics, (b) the PPU's sub-cycle behavior during sprite evaluation, mid-scanline register writes, and the odd-frame skip, (c) DMA cycle-stealing with read-cycle-only halt rules and the DMC/OAM interaction, (d) the mapper IRQ counters (especially MMC3's PPU A12 filter and MMC5's cycle-4 detection), and (e) the APU's nonlinear analog mixer with band-limited synthesis. A Rust implementation gains memory safety and ownership-driven module isolation but must contend with the borrow checker when modeling the shared bus — common patterns are `Rc<RefCell<dyn Mapper>>` (TetaNES, ergonomic, slower) versus monomorphized enums (faster, more code).

For a project targeting cycle-accurate lockstep with a winit + wgpu + cpal frontend in Rust, the critical workstreams are: (1) a workspace split into independently fuzzable crates per chip; (2) a deterministic master-clock scheduler that ticks PPU at 3× and APU at 0.5× CPU pace; (3) a comprehensive test ROM harness gated in CI (nestest, blargg suites, holy mapperel, mmc3_test_2, AccuracyCoin); (4) save state and rewind built on a serializable trait implemented by every chip; and (5) an optional NTSC composite filter (Blargg-style or Mesen NTSC) for visual authenticity.

## Scope and goals

### In-scope

- Cycle-accurate emulation of the NTSC Famicom/NES, with PAL and Dendy as supported alternate timings.
- All official Ricoh 2A03 instructions, including the unofficial/illegal opcodes that commercial games rely on (NOP variants, LAX, SAX, etc.); BCD mode is *not* present on the 2A03.
- 2C02 PPU rendering (background, sprite, scrolling, sprite-zero hit, sprite overflow with the documented hardware bug, mid-scanline register effects, odd-frame skip).
- 2A03 APU (two pulse, triangle, noise, DMC) with the nonlinear mixer and a band-limited sample emitter (blip_buf-style).
- Cartridge file formats: iNES 1.0 and NES 2.0 (12-bit mapper number, exponent-multiplier ROM sizing, submapper field, default-input-device hint).
- Mapper coverage targeting the top ~25 mappers (covers >95% of the licensed library): NROM (0), MMC1 (1), UxROM (2), CNROM (3), MMC3 (4), MMC5 (5), AxROM (7), MMC2 (9), Color Dreams (11), MMC4 (10), CPROM (13), VRC2/4/6/7 (21/22/23/24/25/26/85), Sunsoft FME-7 (69), Namco 163 (19), VRC6 audio variants, FDS (audio + disk swap), GxROM (66), BNROM/NINA-001 (34), and a few common pirate mappers.
- Frontend: cross-platform winit (windowing) + wgpu (rendering) + cpal (audio), with native binaries for Linux/macOS/Windows.
- Save states (deterministic, versioned), rewind (ring-buffered states), snapshot recording for regression tests.
- Debugger overlays (CPU disassembly, PPU nametable / pattern / OAM viewers, APU channel scopes).

### Out-of-scope (initial release)

- Famicom Disk System emulation beyond loading FDS images (audio + disk swap timing belongs to a follow-up).
- Vs. System and PlayChoice-10 arcade variants (different PPU ICs: 2C03, 2C04, 2C05).
- Network play (architecture should *not* foreclose it; deferred to a later phase).
- WebAssembly / mobile builds (the wgpu/winit stack supports them; defer until native is solid).
- Ultra-niche mappers (anything outside the top ~25 by ROM count).
- Hardware-accurate analog video synthesis at composite-signal timing accuracy beyond the Blargg-style filter.

### Success criteria

- Passes the entire blargg `instr_test_v5` suite, `cpu_timing_test6`, `cpu_interrupts_v2`, all `ppu_vbl_nmi` and `ppu_open_bus` sub-tests, all `apu_test` and `apu_mixer` sub-tests, `oam_read`, `oam_stress`, `dmc_dma_during_read4`, and `mmc3_test_2`.
- Boots and runs to attract-mode demo (or first stage gameplay where applicable) for a curated golden-master set covering each implemented mapper.
- Holds locked 60 fps NTSC / 50 fps PAL on a modest contemporary CPU (i.e., a 2018-era laptop chip) with rendering, audio, and rewind ring buffer all enabled.
- All public APIs documented; `cargo doc` produces a usable reference.
- CI runs CPU/PPU/APU regression tests on every push; releases ship signed binaries for Linux/macOS/Windows.

## Background and context

### The console

The Famicom shipped in Japan in July 1983; the NES launched in North America in October 1985 and Europe/Australia from 1986. Internally the system is a tightly integrated three-chip design: the 2A03 CPU (NTSC) or 2A07 (PAL), the 2C02 PPU (NTSC) or 2C07 (PAL), and the cartridge — which carries program ROM (PRG-ROM), pattern table ROM (CHR-ROM, sometimes CHR-RAM), optional save RAM, and a *mapper* IC that arbitrates banking, mirroring, and (in many cases) interrupt generation. Per [NESdev CPU_ALL](https://www.nesdev.org/wiki/CPU_ALL) the master clock is 21.477272 MHz (NTSC); the CPU receives a 12÷ divider for 1.789773 MHz; the PPU receives a 4÷ divider for 5.369318 MHz; PAL is 26.601712 MHz with 16÷ CPU and 5÷ PPU dividers (1.662607 MHz CPU, 5.320342 MHz PPU); Dendy uses the PAL crystal but a 15÷ CPU divider (1.773448 MHz, closer to NTSC) — see [NESdev forums on PAL/Dendy timings](https://forums.nesdev.org/viewtopic.php?t=20931).

### Why cycle accuracy matters

Many NES titles depend on sub-instruction timing: split-screen status bars (sprite-zero hit), per-scanline scroll and palette changes (mid-screen register pokes), MMC3 IRQ-driven interrupts that fire at a precise PPU dot (260 with the standard pattern-table layout), and DPCM samples whose pitch changes if DMA cycle-stealing is mis-modeled. The NESdev forum thread [Importance of cycle accuracy?](https://forums.nesdev.org/viewtopic.php?t=25618) catalogues failure modes of common shortcuts. Catch-up scheduling (run the CPU instruction, then catch the PPU and APU up) is faster and works for ≈98% of titles, but games like *Battletoads*, *Megaman III*, *Punch-Out!!*, and the more aggressive demos require lockstep at PPU-dot resolution.

### The state of the art

The reference implementations a serious project should study (in roughly decreasing order of accuracy / reverse-engineering depth):

- **Mesen2** ([SourMesen/Mesen2](https://github.com/SourMesen/Mesen2)) — multi-system C++/C# rewrite of Mesen, generally accepted as the highest-accuracy NES emulator currently active. Cycle-accurate, comprehensive debugger.
- **higan / ares NES core** ([ares-emulator/ares](https://github.com/ares-emulator/ares)) — byuu-derived, lockstep-scheduled, accuracy first.
- **Nintendulator** — long-running C++ reference, source of the canonical `nestest.log` golden master. See [nes-test-roms/other/nestest.log](https://github.com/christopherpow/nes-test-roms/blob/master/other/nestest.log).
- **FCEUX**, **Nestopia UE**, **PuNES** — older lineages, useful for compatibility cross-checks but with known accuracy gaps.
- **TetaNES** ([lukexor/tetanes](https://github.com/lukexor/tetanes)) — Rust + wgpu, the most prominent Rust NES emulator; the author's [Designs and Frustrations](https://lukeworks.tech/tetanes-part-2) postmortem is essential reading.
- Other Rust efforts: [bugzmanov/nes_ebook](https://bugzmanov.github.io/nes_ebook/), [starrhorne/nes-rust](https://github.com/starrhorne/nes-rust), [Determinant/runes](https://github.com/Determinant/runes) (no_std), [zeta0134/rustico](https://github.com/zeta0134/rustico).

The reverse-engineering substrate underneath all of these is the [NESdev wiki](https://www.nesdev.org/wiki/Nesdev_Wiki) and the [Visual 6502 project](http://www.visual6502.org/) (transistor-level simulation derived from die photographs of decapped chips, presented at [27C3 in 2010](https://media.ccc.de/v/27c3-4159-en-reverse_engineering_mos_6502)). For the PPU, the [Visual 2C02](https://www.nesdev.org/wiki/Visual_2C02) project provides the analogous die-shot-derived simulation.

## Technical deep-dive

### CPU — Ricoh 2A03 (6502 derivative)

The 2A03 packages a MOS 6502 core, the APU, two DMA controllers (sprite OAM and DMC), the controller-port interface, and the audio output stage on a single die. Per [NESdev CPU_ALL](https://www.nesdev.org/wiki/CPU_ALL), the CPU exposes a flat 16-bit address space:

| Range | Size | Purpose |
|-------|------|---------|
| `$0000-$07FF` | 2 KB | Internal RAM (zero page `$00-$FF`, stack `$0100-$01FF`, general `$0200-$07FF`) |
| `$0800-$1FFF` | 6 KB | Mirrors of `$0000-$07FF` |
| `$2000-$2007` | 8 B | PPU registers |
| `$2008-$3FFF` | ~8 KB | PPU register mirrors (every 8 bytes) |
| `$4000-$4017` | 24 B | APU + I/O registers |
| `$4018-$401F` | 8 B | Disabled (APU/IO test mode) |
| `$4020-$FFFF` | ~49 KB | Cartridge address space (PRG-ROM, mapper registers, optional PRG-RAM) |

Interrupt vectors live at `$FFFA/B` (NMI), `$FFFC/D` (Reset), `$FFFE/F` (IRQ/BRK).

#### Differences from a stock 6502

The 2A03 omits BCD mode: the D flag is settable but `ADC`/`SBC` ignore it. All 151 documented opcodes plus all 105 unofficial opcodes execute identically to a stock 6502 (excluding decimal mode). The undocumented opcode set includes legitimately useful instructions: `LAX` (load A and X), `SAX` (store A AND X), `DCP` (DEC then CMP), `ISC` (INC then SBC), `RLA`, `SLO`, `SRE`, `RRA`, plus several NOPs of varying length used by commercial games for timing padding.

#### Cycle behavior and clock phases

Per [NESdev CPU_ALL](https://www.nesdev.org/wiki/CPU_ALL), every CPU cycle is either a read or a write — there are no idle cycles. The internal clock is divided into φ1 and φ2 phases. Address output and reads complete during one phase; writes during the other. The exact duty cycle on the NTSC 2A03E/G/H is 15/24 (5/8) — reads take 1⅞ PPU cycles, writes take 1⅛. Most emulators don't model phase asymmetry, but it matters for sub-cycle DMA alignment and for the DPCM "DMA during read of $4015" bug.

#### Interrupts

Per [NESdev CPU_interrupts](https://www.nesdev.org/wiki/CPU_interrupts):

- **NMI** (`$FFFA/B`) is **edge-sensitive**: the CPU's edge detector polls the NMI line during φ2 of every cycle and raises an internal latch on a high-to-low transition. The latch goes high during φ1 of the *following* cycle and stays asserted until handled.
- **IRQ** (`$FFFE/F`, shared with BRK) is **level-sensitive**: a low level on IRQ during φ2 raises the internal IRQ signal during the next φ1, which stays active only as long as IRQ remains low.
- Polling occurs during the *final cycle of an instruction, before the opcode fetch for the next one* — but more precisely, "the status of the interrupt lines at the end of the second-to-last cycle" determines whether an interrupt fires.
- Both IRQ and NMI use the same 7-cycle sequence: opcode fetch (discarded), throwaway read, PCH push, PCL push, P push (B clear for IRQ/NMI, set for BRK), vector low fetch, vector high fetch.
- **Hijacking**: if NMI asserts during ticks 1-4 of a BRK instruction, the BRK proceeds normally through stack pushes but the vector fetch goes to `$FFFA/B` instead of `$FFFE/F`. NMI can hijack IRQ; IRQ can hijack BRK. The hardware has explicit anti-hijacking logic for the T5/T6 vector-fetch cycles.
- Branches poll for interrupts in a non-obvious order: before cycle 2, before the PCH-fixup cycle on a page-crossing taken branch, but **not** before cycle 3 of a taken branch.

#### Power-up state

A, X, Y are 0; PC reads from the reset vector; S is `$FD` (initial $00, then three implicit pushes during reset suppress writes but decrement S); P has Interrupt Disable set; all other flags clear.

### PPU — Ricoh 2C02

The PPU is a fixed-function rendering engine with its own memory bus. It exposes 8 memory-mapped registers to the CPU (mirrored across `$2000-$3FFF`), plus the OAM-DMA register at `$4014`. PPU memory occupies 14 bits (`$0000-$3FFF`):

| Range | Size | Purpose |
|-------|------|---------|
| `$0000-$1FFF` | 8 KB | Pattern tables (typically cartridge CHR-ROM/RAM) |
| `$2000-$2FFF` | 4 KB | Nametables (mostly internal 2 KB VRAM with cartridge-controlled mirroring) |
| `$3000-$3EFF` | ~4 KB | Mirror of `$2000-$2EFF` |
| `$3F00-$3F1F` | 32 B | Palette RAM (with mirrors through `$3FFF`) |

Source: [NESdev PPU_memory_map](https://www.nesdev.org/wiki/PPU_memory_map).

#### Frame and scanline structure

Per [NESdev PPU_rendering](https://www.nesdev.org/wiki/PPU_rendering): NTSC frame = 262 scanlines × 341 PPU cycles ("dots"). Each dot produces one pixel during visible scanlines (0-239). The post-render scanline (240) is idle. Vertical blanking spans scanlines 241-260; the VBL flag in PPUSTATUS sets at scanline 241, dot 1, and that is also when the NMI fires (if PPUCTRL bit 7 is set). Scanline 261 is the pre-render line, used to refill shift registers and reload vertical scroll bits during dots 280-304. The pre-render scanline of *odd* frames skips the final dot (jumps from `(339, 261)` directly to `(0, 0)`); even frames do not. PAL is 312 scanlines and does not skip; Dendy uses NTSC's 262-line height but with PAL's clock divider, producing 51 post-render scanlines instead of 1.

#### The 8-dot tile cycle

For visible scanlines 0-239 and the pre-render scanline 261, dots 1-256 fetch background tiles in 8-dot windows. Each window performs four 2-dot memory accesses: nametable byte → attribute byte → pattern table low → pattern table high. The fetched data is loaded into shift registers on every 8th dot, simultaneously with the coarse-X increment of the internal `v` register.

Dots 257-320 fetch sprite tiles for the *next* scanline: 8 sprites × (garbage NT, garbage NT, pattern low, pattern high). During these dots, OAMADDR is forced to 0; sprite X-position and attribute latches load during the second garbage fetch. Dots 321-336 prefetch the first two background tiles of the next scanline. Dots 337-340 perform two more nametable fetches whose purpose is debated but whose timing is preserved (MMC5 in particular can detect them).

#### Internal scroll registers (loopy v / t / x / w)

The de-facto-standard model for PPU scroll, due to Loopy's reverse-engineering work, uses four internal registers:

- **v** — current VRAM address (15 bits effective, though only 14 are externally addressable). During rendering, it doubles as the scroll position cursor.
- **t** — temporary VRAM address; the "next scanline's start" buffer.
- **x** — fine X scroll, 3 bits, the per-pixel offset into a tile.
- **w** — write toggle (1 bit), tracks first vs. second write to PPUSCROLL/PPUADDR. Cleared by reading PPUSTATUS.

Per [NESdev PPU_registers](https://www.nesdev.org/wiki/PPU_registers): writing PPUSCROLL first updates `t` bits 4-0 (coarse X) on the first write, bits 14-12 (fine Y) plus bits 11-5 (coarse Y) on the second. PPUADDR's first write updates `t` bits 13-8; the second updates bits 7-0 and copies all of `t` into `v`. During rendering, `v` increments along bit-twiddly paths (coarse X carry into nametable X, coarse Y wrap from 29→0 with nametable Y flip, etc.). The horizontal bits of `v` are reloaded from `t` at dot 257 of every visible/pre-render scanline; the vertical bits are reloaded only during dots 280-304 of the pre-render scanline (if rendering is enabled).

#### PPU registers — the gotchas

- **PPUSTATUS ($2002) read** clears both the VBL flag *and* the `w` toggle. Polling VBL by reading `$2002` is unreliable: a read at scanline 241 dot 0 returns 0 *and* suppresses the NMI for that frame.
- **PPUDATA ($2007) read** is buffered: the first read after setting PPUADDR returns stale data and updates the buffer with the new address's data. Palette reads (`$3F00-$3FFF`) bypass the buffer but simultaneously update it with the underlying nametable mirror's content.
- **Open bus**: PPUSTATUS bits 4-0 are open-bus (they reflect whatever was last on the PPU's internal data bus). The bus decays after 3-30 ms (faster when warm). Per the [NESdev forum on open bus decay](https://forums.nesdev.org/viewtopic.php?t=12549), Mesen-style accuracy emulators decay each bit group separately.
- **PPUCTRL ($2000) write** with NMI bit transitioning 0→1 *while* the VBL flag is already set will fire an NMI immediately. The early ~29,658-cycle (NTSC) post-reset window also ignores writes to $2000/$2001/$2005/$2006.
- **OAMADDR ($2003)** has a notorious 2C02G bug: writes corrupt OAM by copying sprites 8-9 over the target row. 2C03/2C04/2C05/2C07 do not have this.
- **PPUMASK ($2001) bit 0 (greyscale)** ANDs all output colors with `$30`, providing a quick way to flash the screen.

#### Sprite evaluation

Per [NESdev PPU_sprite_evaluation](https://www.nesdev.org/wiki/PPU_sprite_evaluation):

- **Cycles 1-64** of every visible scanline clear secondary OAM to `$FF` (reads from primary OAM are forced to `$FF` by the sprite-evaluation FSM).
- **Cycles 65-256** evaluate sprites: odd cycles read from primary OAM, even cycles write to secondary OAM. The hardware iterates `n` (sprite index 0-63) and copies in-range sprites' four bytes to secondary OAM.
- After 8 sprites are found, the algorithm continues to scan but the buggy overflow check kicks in: instead of incrementing only `n`, it increments **both `n` and `m` (the byte index within a sprite) without carry**. This causes the algorithm to misinterpret tile numbers, attributes, and X-coordinates as Y-coordinates, producing both false positives and false negatives in the sprite-overflow flag. This is a famous documented hardware bug that emulators must reproduce.

#### Sprite-zero hit

The sprite-zero hit flag (PPUSTATUS bit 6) is set when a non-transparent pixel of sprite 0 overlaps a non-transparent pixel of the background. Crucial constraints:

- Cannot be set on dot 255 (right edge).
- Cannot be set if either layer's leftmost-8-pixel show flag is off and the X position is 0-7.
- Cannot be set during scanline 261 (pre-render). The flag is cleared at scanline 261, dot 1.
- Sprite-0 hit is determined during sprite *rendering*, not sprite evaluation.

This is the canonical mechanism for split-screen status bars.

### APU — Ricoh 2A03 audio unit

Per [NESdev APU](https://www.nesdev.org/wiki/APU), the APU registers occupy `$4000-$4017`:

| Range | Channel |
|-------|---------|
| `$4000-$4003` | Pulse 1 (timer, length, envelope, sweep) |
| `$4004-$4007` | Pulse 2 |
| `$4008-$400B` | Triangle (timer, length, linear counter) |
| `$400C-$400F` | Noise (timer, length, envelope, LFSR mode) |
| `$4010-$4013` | DMC (timer, output level, sample address, sample length) |
| `$4015` | Status (channel enable, length-counter status) |
| `$4017` | Frame counter mode + IRQ inhibit |

Channels:

- **Pulse 1 / Pulse 2** — 11-bit timer, length counter, 4-step envelope (15→0 or constant volume), and a frequency sweep. Frequency `f = f_CPU / (16 × (t + 1))`.
- **Triangle** — 32-step quantized triangle wave, no volume. Has a linear counter and a length counter. Period one octave below an equivalent pulse setting.
- **Noise** — pseudo-random LFSR (15-bit, mode 0; or 6-bit period for "metallic" mode 1). Lookup-table-driven period.
- **DMC** — 7-bit PCM played from delta-encoded samples in CPU memory (`$C000-$FFFF`). Samples are 1-bit deltas that add or subtract 2 from a 7-bit counter.

#### Frame counter

Per [NESdev APU_Frame_Counter](https://www.nesdev.org/wiki/APU_Frame_Counter), the frame counter divides the CPU clock to roughly 240 Hz and clocks channel sub-units (envelope, linear counter, length counter, sweep) on a 4-step or 5-step cycle:

```
Mode 0 (4-step): -  -  -  f      f = frame IRQ if inhibit clear
                 -  l  -  l      l = length counter + sweep
                 e  e  e  e      e = envelope + triangle linear counter

Mode 1 (5-step): -  -  -  -  -
                 -  l  -  -  l
                 e  e  e  -  e
```

The 4-step mode is what generates the frame IRQ used by some games for timing; mode 1 (set by writing `$80` to `$4017`) suppresses the IRQ entirely and runs at a slower effective update rate. Writing `$4017` resets the counter with a 3- or 4-CPU-cycle delay (depending on alignment) and, if mode 1 is selected, immediately clocks the quarter and half frame events.

#### DMC and DMA interactions

Per [NESdev DMA](https://www.nesdev.org/wiki/DMA):

- **OAM DMA** ($4014 write) takes **513 or 514 CPU cycles**: 1 halt cycle, optional 1 alignment cycle if the next phase isn't a "get" cycle, then 256 read/write pairs.
- **DMC DMA** takes **3 or 4 CPU cycles** per sample byte: halt + dummy + optional alignment + single read. Two scheduling variants exist (load DMA after `$4015` D4 write vs. reload DMA when the buffer empties), differing in get-vs-put scheduling.
- Halts succeed only on **read** cycles. On a write cycle the halt re-tries next cycle, stretching DMA delay during read-modify-write instructions.
- DMC DMA takes precedence over OAM DMA when they collide (1 DMC + 1 OAM realignment = +2 cycles typical).
- The 2A03 has a notorious bug where, while halted, it re-reads the previously-addressed location during each no-op DMA cycle. If that address is `$2007` (PPUDATA), 2-3 extra reads occur (skipping VRAM bytes); if `$4015-$4017`, 1-4 extra reads can cause spurious controller reads (joypad crosstalk) or APU register effects. The 2A07 (PAL) fixes this. Late RP2A03G/H chips also exhibit an "unexpected DMA" reload from the same address when implicit stops align with reload scheduling.

#### Mixer

Per [NESdev APU_Mixer](https://www.nesdev.org/wiki/APU_Mixer), the analog mixer is non-linear. Two emulator models:

**Linear approximation** (good first cut):
```
pulse_out = 0.00752 * (pulse1 + pulse2)
tnd_out   = 0.00851 * triangle + 0.00494 * noise + 0.00335 * dmc
output    = pulse_out + tnd_out
```

**Lookup-table approximation** (matches hardware to within ~4%):
```
pulse_table[n] = 95.52 / (8128.0/n + 100)
tnd_table[n]   = 163.67 / (24329.0/n + 100)
output = pulse_table[pulse1 + pulse2]
       + tnd_table[3*triangle + 2*noise + dmc]
```

The console then applies a 90 Hz first-order high-pass, a 440 Hz first-order high-pass, and a 14 kHz first-order low-pass before output. Quality emulators (Mesen, Nestopia, ares) apply these filters.

#### Band-limited synthesis

Naive sample emission causes aliasing — the channels are 1-bit step generators clocking at MHz rates being downsampled to 44.1/48 kHz. The standard solution is Blargg's [blip_buf](https://github.com/cc65/blip_buf) library: per-step responses convolved with a windowed sinc, summed into an output buffer. This is an O(steps × kernel_width) approach and produces effectively ideal antialiasing. Most modern Rust crates use either a port or an analog (e.g., `blip_buf-rs`).

### Cartridge format and mappers

#### iNES and NES 2.0

Per [NESdev iNES](https://www.nesdev.org/wiki/INES) and [NESdev NES_2.0](https://www.nesdev.org/wiki/NES_2.0): a 16-byte header precedes optional trainer (512 bytes), PRG-ROM (16 KiB units), and CHR-ROM (8 KiB units). Detection rule: NES 2.0 if `(header[7] & 0x0C) == 0x08`. Header byte layout:

| Off | iNES 1.0 | NES 2.0 additions |
|-----|----------|-------------------|
| 0-3 | `"NES\x1A"` | same |
| 4 | PRG-ROM size (16 KiB) | LSB; MSB nibble in byte 9 |
| 5 | CHR-ROM size (8 KiB) | LSB; MSB nibble in byte 9 |
| 6 | Mirroring, battery, trainer, mapper D3-D0 | same |
| 7 | VS, PC10, mapper D7-D4 | NES 2.0 detect bits, console type |
| 8 | PRG-RAM (rare, ignored) | mapper D11-D8 + submapper |
| 9 | Reserved | PRG-ROM MSB + CHR-ROM MSB |
| 10 | Reserved | PRG-RAM (4-bit shift) + PRG-NVRAM (4-bit shift) |
| 11 | Reserved | CHR-RAM + CHR-NVRAM (same encoding) |
| 12 | Reserved | CPU/PPU timing (NTSC/PAL/multi/Dendy) |
| 13 | Reserved | Vs. PPU type or extended console type |
| 14 | Reserved | Misc ROM count |
| 15 | Reserved | Default expansion device code |

NES 2.0's exponent-multiplier ROM sizing (when MSB nibble is `$F`): size = `2^E * (MM*2+1)` bytes, where E is 6 bits and MM is 2 bits. RAM sizes use a shift encoding: shift count of 0 = no RAM; count > 0 = `64 << count` bytes. The 12-bit mapper number plus 4-bit submapper supports 4096 mappers × 16 hardware variants.

#### Mapper landscape (top mappers by ROM count)

| iNES # | Name | PRG bank | CHR bank | IRQ | Notes |
|--------|------|----------|----------|-----|-------|
| 0 | NROM | none | none | no | 16/32K PRG, 8K CHR. 247 commercial titles. |
| 1 | MMC1 | 16K (or 32K) | 4K (or 8K) | no | Serial 5-write protocol, slow bank switch |
| 2 | UxROM | 16K | none (CHR-RAM) | no | Bank `$8000-$BFFF`, fixed `$C000-$FFFF` |
| 3 | CNROM | none | 8K | no | Bus-conflict-prone CHR bank switch |
| 4 | MMC3 | 8K | 1K/2K | scanline | PPU-A12 IRQ counter; *the* defining mid-life mapper |
| 5 | MMC5 | 8K-32K | 1K-8K | scanline | Most complex official mapper; ExRAM, extra audio |
| 7 | AxROM | 32K | none (CHR-RAM) | no | Single-screen mirroring control |
| 9 | MMC2 | 8K | 4K | no | "Punch-Out" latch: tile-fetch-driven CHR switch |
| 10 | MMC4 | 16K | 4K | no | Like MMC2 with full PRG banking |
| 11 | Color Dreams | 32K | 8K | no | Unlicensed; bus-conflict |
| 13 | CPROM | none | 4K (CHR-RAM) | no | Videomation only |
| 19 | Namco 163 | 8K | 1K | scanline | Extra audio (8 channels) |
| 21,22,23,25 | VRC2/4 | 8K | 1K | CPU cycle | Konami |
| 24,26 | VRC6 | 8K/16K | 1K | CPU cycle | Konami; 3 extra audio channels (Akumajou Densetsu / Castlevania III) |
| 34 | BNROM/NINA-001 | 32K (16K NINA) | 4K (NINA) | no | |
| 66 | GxROM | 32K | 8K | no | |
| 69 | Sunsoft FME-7 | 8K | 1K | CPU cycle | 5B variant has 3 audio channels (Gimmick!) |
| 85 | VRC7 | 8K | 1K | CPU cycle | YM2413-derived FM audio (Lagrange Point) |

#### MMC3 — the defining mapper

Per [NESdev MMC3](https://www.nesdev.org/wiki/MMC3): registers `$8000-$E001` (paired even/odd: bank-select + bank-data, mirroring + RAM-protect, IRQ-latch + IRQ-reload, IRQ-disable + IRQ-enable). PRG bank modes alternate which 8 KB window is fixed (`$C000` or `$8000` sees the second-to-last bank); the last bank is always at `$E000`. CHR modes alternate which pattern table gets the two 2 KB banks vs. four 1 KB banks (controlled by bit 7 of the bank-select register).

The IRQ counter is the famously tricky part. It's clocked off **PPU A12** edge transitions, filtered such that A12 must remain low for 3 falling edges of M2 before the next rising edge counts. In standard pattern-table layout (BG pattern at `$0000`, sprites at `$1000`), this filter produces exactly one clock per scanline at PPU dot 260. On each filtered rising edge: if the counter is 0 or the reload flag is set, reload from the latch register; otherwise decrement; if the post-action counter is 0 and IRQs are enabled, assert IRQ. The "alternate revision" (Sharp MMC3A) checks the 1→0 transition and generates IRQs even when the latch is `$00`; the NEC revision (MMC3B) does not. Star Trek: 25th Anniversary depends on Sharp behavior. Reversed pattern-table layout (BG at `$1000`, sprites at `$0000`) puts the decrement at PPU cycle 324 of the *previous* scanline, producing the classic Wario's Woods flicker if not modeled. 8×16 sprites complicate A12 tracking further.

#### MMC5 — the most complex official mapper

Per [NESdev MMC5](https://www.nesdev.org/wiki/MMC5): four PRG banking modes (single 32 KB up to four independent 8 KB), four CHR banking modes (8 KB down to 1 KB), separate sprite and background CHR banks for 8×16 sprites (registers `$5120-$5127` for sprites, `$5128-$512B` for BG), 1 KB on-board ExRAM with multiple modes (extended attributes that grant per-tile palette selection from 16 384 unique tiles, fill-mode nametable, etc.), a scanline IRQ that triggers at PPU cycle 4 (not 260), and three extra audio channels (two pulse, one 8-bit raw PCM). Used by Castlevania III (Japan), Just Breed, Laser Invasion, and a handful of others.

### Controllers and inputs

Per [NESdev Standard_controller](https://www.nesdev.org/wiki/Standard_controller): controllers are CD4021-style 8-bit parallel-to-serial shift registers. Writing the strobe bit (bit 0 of `$4016`) high then low latches button state into the register; subsequent reads of `$4016` (port 1) or `$4017` (port 2) shift out one bit per read, in order: A, B, Select, Start, Up, Down, Left, Right. Bits 2-1 of `$4016` write are also internal latches but route to the expansion port. The Famicom expansion port supports light gun (Zapper at `$4017`), Power Pad, Family BASIC keyboard, and serial cables; the NES has only the two front controller ports plus expansion lines on the bottom edge.

### Region differences

Per [NESdev Cycle_reference_chart](https://www.nesdev.org/wiki/Cycle_reference_chart) and the [Differences thread](https://forums.nesdev.org/viewtopic.php?t=20931):

| Region | Master | CPU | PPU | Lines/frame | VBL lines | CPU:PPU |
|--------|--------|-----|-----|-------------|-----------|---------|
| NTSC | 21.477 MHz | ÷12 = 1.7898 MHz | ÷4 = 5.3693 MHz | 262 | 20 | 1:3 |
| PAL | 26.602 MHz | ÷16 = 1.6626 MHz | ÷5 = 5.3203 MHz | 312 | 70 | 1:3.2 |
| Dendy | 26.602 MHz | ÷15 = 1.7734 MHz | ÷5 = 5.3203 MHz | 312 | 20 | 1:3 |

Dendy (Russian PAL famiclone) inherits PAL's wall-clock 50 Hz refresh but keeps NTSC's timing semantics (CPU:PPU 1:3, 20-line VBL), making most NTSC games run cleanly at 50 fps. Per-region length counters, sweep mute thresholds, and DPCM rate tables differ — see the cycle reference chart.

### NTSC composite filter and CRT shaders

NES output is composite NTSC at ~240p; the original signal exhibits dot crawl, color bleed, and luminance-color crosstalk that affects how artists composed sprites (the Pac-Man arcade and Sonic's waterfalls being more famous examples). Two main approaches:

- **Blargg's NTSC filter** — CPU-side, takes 3 NES pixels at a time and produces 7 output pixels reflecting two colorburst cycles. Has presets (composite, S-video, RGB, monochrome). Has been ported widely. Reference: [emulation gametechwiki NTSC filters](https://emulation.gametechwiki.com/index.php/NTSC_filters).
- **Themaister's NTSC composite shader** — GPU side, GLSL/slang, real-time. Available in libretro's slang-shaders and Mesen.
- **CRT shaders** (cgwg, crt-royale, crt-easymode) layer scanlines, mask geometry, and phosphor decay over the NTSC step.

### Frontend (Rust ecosystem)

For a pure-Rust frontend stack:

- **Windowing**: [winit](https://github.com/rust-windowing/winit) (cross-platform: X11, Wayland, AppKit, Win32, Web).
- **Rendering**: [wgpu](https://github.com/gfx-rs/wgpu) (WebGPU API targeting Vulkan/Metal/D3D12/OpenGL/WebGL2).
- **Audio**: [cpal](https://github.com/RustAudio/cpal) (cross-platform PCM stream). Often paired with [rubato](https://github.com/HEnquist/rubato) for sample-rate conversion or with `blip_buf-rs` for direct band-limited emission.
- **GUI overlays**: [egui](https://github.com/emilk/egui) integrates cleanly with wgpu for debugger panels.
- **Input**: [gilrs](https://gitlab.com/gilrs-project/gilrs) for gamepads.

TetaNES uses this exact stack and works on Linux/macOS/Windows + WebAssembly.

## State of the art / prior art

### What the leaders do well

- **Mesen2** ([SourMesen/Mesen2](https://github.com/SourMesen/Mesen2)): cycle-accurate scheduler, comprehensive debugger (memory viewer, PPU viewer, APU scope, breakpoints, conditional breakpoints, trace logs, Lua scripting), netplay, rewind, save states, NES + SNES + Game Boy + GBA + PCE + SMS + WonderSwan in one codebase. C++/C# split.
- **higan / ares NES core** ([ares-emulator/ares](https://github.com/ares-emulator/ares)): byuu's scheduler (all components advance to a global timestamp), accuracy-first design philosophy, exemplary code clarity.
- **Nintendulator** (Quietust): older but the *de facto* nestest reference; produces the canonical `nestest.log`.
- **FCEUX**: large feature set (Lua, TAS tooling), accuracy is decent but trails Mesen.
- **PuNES**: strong NTSC filter, broad mapper support.

### Rust-specific lineage

- **TetaNES** ([lukexor/tetanes](https://github.com/lukexor/tetanes)) — most mature Rust NES emulator. Uses wgpu, supports save state + rewind, runs on web. Architecture lessons in [Designs and Frustrations](https://lukeworks.tech/tetanes-part-2): started catch-up, migrated to lockstep with per-memory-access sync; uses `Rc<RefCell<dyn Mapper>>` for runtime polymorphism; Bus struct centralizes shared mutable state; relies on `Clocked` trait for uniform tick interface; turned on `opt-level = 2` in dev to keep overflow checks while preserving acceptable speed.
- **bugzmanov/nes_ebook** — the canonical Rust NES tutorial; not accuracy-focused but pedagogically clear.
- **starrhorne/nes-rust** — small reference impl, MIT.
- **Determinant/runes** — no_std, embeddable; informs the workspace split.
- **zeta0134/rustico** — runs in browser and native; strong audio.

### Common architectural choices in accuracy-first emulators

1. **Lockstep scheduling** with a single master tick (often the PPU dot, since PPU is the highest-frequency consumer). CPU advances 1/3 as often (NTSC).
2. **Per-memory-access PPU/APU/mapper sync** so a mid-instruction CPU read sees an up-to-date PPU register.
3. **Component traits** for serialization, reset, and tick.
4. **Centralized bus** that the CPU borrows mutably to perform reads/writes; PPU gets its own VRAM bus through the mapper.
5. **Immutable-by-default ROM banks** indexed at runtime; mappers map address ranges to bank indices.

## Principal engineering challenges

1. **CPU interrupt edge/level semantics + hijacking.** Easy to model the wrong way: if NMI is checked at the wrong cycle within an instruction, or BRK isn't re-routed when NMI hijacks, ROMs that rely on tight NMI-IRQ sequencing (e.g., MMC3 split-screen + NMI music driver) glitch. Mitigation: model NMI as an internal latch set during φ2 polling; make IRQ a continuously sampled level; route both through the per-instruction "interrupt poll" point, with the documented branch-instruction quirks.

2. **Sub-instruction PPU state during sprite eval and mid-scanline writes.** Writing PPUSCROLL or PPUADDR mid-scanline manipulates the loopy `t/v` registers and therefore the effective scroll. Sprite evaluation runs in lockstep; a CPU-driven OAMADDR write at the wrong dot causes the documented OAMADDR corruption. Mitigation: PPU is the master clock, CPU calls into a `tick()` that also services PPU/APU/mapper at every memory access.

3. **DMA cycle stealing with read-only halt rule.** OAM DMA can take 513 or 514 cycles depending on alignment; DMC DMA takes 3 or 4 and can collide with OAM DMA. The 2A03 register-readout bug must be modeled to pass `dmc_dma_during_read4`. Mitigation: a DMA controller chip-let inside the bus that owns "halt CPU on next read cycle," coupled with a single `cycles_pending` counter the CPU drains.

4. **MMC3 IRQ counter via PPU A12.** The 3-falling-edge filter on M2 plus the 1→0 vs. 0→0 reload distinction between Sharp and NEC revisions must be reproducible, including the 8×16 sprite case where A12 toggles multiple times per scanline. Mitigation: track the A12 history in the PPU and call `Mapper::ppu_a12_clock(level: bool)` on every change; mappers that don't care implement no-op.

5. **APU mixing accuracy + band-limited synthesis.** Linear approximation passes most ROM ear tests but fails `apu_mixer`. Lookup-table mixer + proper high/low-pass filters + blip_buf-style band-limited emission are required for cycle-accurate audio. Mitigation: implement both in parallel — a `LinearMixer` for first-cut work and a `LookupMixer` with `BlipBuf` for the accuracy run.

6. **Save-state determinism across Rust versions.** Without care, derived `Serialize` can produce different bytes when struct field order changes. Mitigation: explicit hand-rolled save format with a version byte, tagged sections per chip, and round-trip property tests that load every state in a corpus and confirm subsequent execution diverges by 0 cycles.

7. **Performance under release builds with overflow checks.** Per the TetaNES postmortem, leaving `overflow-checks` on is correct but costs performance; turning it off may hide bugs. Mitigation: keep overflow checks on for debug + tests, off for release; add a CI matrix entry that runs the test ROM corpus with checks on.

## Architecture options

### Option A — Tight lockstep, master-clock PPU dot

PPU drives the master tick. The scheduler advances the PPU one dot at a time; every third dot, it advances the CPU one cycle (NTSC). Every other CPU cycle clocks the APU. Mapper hooks fire from the PPU on A12 changes and from the CPU on memory accesses. Pros: highest accuracy, simple mental model, naturally matches Mesen/ares. Cons: tightest coupling; the PPU "dot" becomes a hot loop and benefits from inlining + monomorphization.

### Option B — Catch-up, CPU instruction granularity

Run a full CPU instruction; record the cycle count; advance PPU and APU by 3× and 0.5× that count respectively, calling sub-cycle hooks only at known boundaries (e.g., NMI generation, sprite-zero hit). Pros: fastest; simplest CPU code. Cons: requires careful patching for mid-instruction hardware events; sub-cycle accuracy is degraded; many edge cases need explicit special-casing.

### Option C — Hybrid (lockstep core + catch-up fast path, runtime toggleable)

Default to lockstep. Provide a runtime flag that switches to catch-up for known-compatible games. Pros: usable on low-power devices; user choice. Cons: largest code footprint; two scheduling paths to keep in sync.

### Recommendation (per Phase 1 user decision)

**Option A — Tight lockstep.** This is the user's stated goal and matches the project name's intent ("cycle-accurate"). Architecture decisions in `docs/` will be written assuming Option A.

## External dependencies and integrations

### Required (frontend binary)

| Crate | Use | License |
|-------|-----|---------|
| `winit` | Cross-platform windowing | Apache-2.0 |
| `wgpu` | GPU rendering | MIT or Apache-2.0 |
| `cpal` | Audio output | Apache-2.0 |
| `egui` + `egui-wgpu` | Debugger UI | MIT |
| `gilrs` | Gamepad input | Apache-2.0 |
| `rfd` | Native file dialogs | MIT |
| `directories` | XDG/Win/macOS config paths | MIT or Apache-2.0 |

### Required (core, no_std-friendly subset)

| Crate | Use | License |
|-------|-----|---------|
| `bitflags` | CPU status flags, PPU control bits | MIT or Apache-2.0 |
| `serde` (optional, default off) | Save state serialization | MIT or Apache-2.0 |
| `bincode` (optional) | Compact save state encoding | MIT |
| `thiserror` | Error types | MIT or Apache-2.0 |

### Dev dependencies

| Crate | Use |
|-------|-----|
| `criterion` | Benchmarking |
| `proptest` | Property tests for CPU instructions |
| `insta` | Snapshot tests for nestest log comparison |
| `pretty_assertions` | Better diff output |

### Test ROM corpus (not a Rust crate; vendored in `tests/roms/`)

- [christopherpow/nes-test-roms](https://github.com/christopherpow/nes-test-roms) — blargg, kevtris, pubby et al. (CC0 / public-domain individual ROMs)
- [100thCoin/AccuracyCoin](https://github.com/100thCoin/AccuracyCoin) — single-cartridge accuracy battery
- Holy Mapperel by tepples (pineight.com)

These are freely redistributable. **Commercial NES game ROMs are NOT bundled and never will be.**

## Standards and compliance

- **iNES 1.0** — de facto file format from ~1996; documented at [NESdev iNES](https://www.nesdev.org/wiki/INES) and at [Marat Fayzullin's iNES doc](http://fms.komkon.org/iNES/iNES.html).
- **NES 2.0** — extension dating from 2010s; documented at [NESdev NES_2.0](https://www.nesdev.org/wiki/NES_2.0). The community-blessed superset of iNES.
- **No formal IEEE/ISO standard exists** for the NES platform itself (it is a 1980s consumer-electronics product). The NESdev wiki + Visual 2C02 / Visual 6502 die-shot reverse engineering serve the community equivalent role.
- **Test ROM conformance suites** play the role of a standards body's reference implementation: blargg's suites are de facto required to claim cycle accuracy.

## Open questions

These are surfaced for `/init` to plan around. None block initial development:

- **CRT shader sourcing.** Adopt cgwg/crt-easymode/crt-royale via slang-shaders port? Write our own pure-wgsl variants? Pursue Themaister-style real-time NTSC composite shader for GPU side? *Suggested resolution: ship Blargg-style CPU NTSC filter in v0.1; add slang-shader-style WGSL CRT shaders in a later phase.*
- **MMC5 audio scope.** Is the extra-pulse + raw-PCM channel must-have for v1.0, or deferable behind a feature flag? Castlevania III (J), Just Breed, Laser Invasion are the only commercial cases. *Suggested: feature flag, off by default, in Phase 4.*
- **VRC7 FM audio (YM2413-derived).** Lagrange Point is the only commercial title. Implementation requires either a YM2413 emulation layer or a borrowed implementation. *Suggested: defer beyond v1.0.*
- **FDS support depth.** Disk-image emulation, expansion audio (wavetable + envelope), audio mixing with the 2A03 — significant scope. *Suggested: defer to a v0.x stretch goal.*
- **Save state format stability.** Backwards-compatible loads across releases vs. "best effort" with version-skew? *Suggested: tagged-section format with `bincode` per chip; release-note-documented breaking version bumps; no auto-migration.*
- **Netplay.** Out-of-scope for v1.0 but the deterministic core enables it. *Suggested: don't design against it (don't add hidden state); revisit post-v1.0.*

## Sensitivity considerations

None. The NES is a 1980s consumer product; its hardware is exhaustively documented through reverse engineering, and the community wiki is open. The project ships zero copyrighted Nintendo game ROMs. Test ROMs are individually redistributable per their authors' CC0/public-domain dedications. The implementation copies no Nintendo code.

## Source manifest

### Tier 1 — primary, authoritative (NESdev wiki + Visual chip projects)

1. [NESdev wiki — CPU_ALL](https://www.nesdev.org/wiki/CPU_ALL) — CPU clock frequencies, memory map, registers, vectors.
2. [NESdev wiki — CPU interrupts](https://www.nesdev.org/wiki/CPU_interrupts) — NMI/IRQ semantics, polling, hijacking.
3. [NESdev wiki — Visual6502wiki/6502 Interrupt Hijacking](https://www.nesdev.org/wiki/Visual6502wiki/6502_Interrupt_Hijacking) — die-shot-derived interrupt analysis.
4. [NESdev wiki — Visual6502wiki/6502 Interrupt Recognition Stages and Tolerances](https://www.nesdev.org/wiki/Visual6502wiki/6502_Interrupt_Recognition_Stages_and_Tolerances) — sub-cycle detail on when interrupts are sampled.
5. [NESdev wiki — PPU](https://www.nesdev.org/wiki/PPU) — overview index.
6. [NESdev wiki — PPU rendering](https://www.nesdev.org/wiki/PPU_rendering) — per-dot timing, scanline phases, odd-frame skip.
7. [NESdev wiki — PPU registers](https://www.nesdev.org/wiki/PPU_registers) — `$2000-$2007` + `$4014` register bit layouts and quirks.
8. [NESdev wiki — PPU memory map](https://www.nesdev.org/wiki/PPU_memory_map) — `$0000-$3FFF` PPU bus layout.
9. [NESdev wiki — PPU sprite evaluation](https://www.nesdev.org/wiki/PPU_sprite_evaluation) — cycle-by-cycle eval algorithm + the `n+m` overflow bug.
10. [NESdev wiki — PPU OAM](https://www.nesdev.org/wiki/PPU_OAM) — OAM layout and access semantics.
11. [NESdev wiki — PPU programmer reference](https://www.nesdev.org/wiki/PPU_programmer_reference) — programming-side reference.
12. [NESdev wiki — PPU frame timing](https://www.nesdev.org/wiki/PPU_frame_timing) — frame-level timing.
13. [NESdev wiki — Visual 2C02](https://www.nesdev.org/wiki/Visual_2C02) — die-shot-derived PPU simulation.
14. [NESdev wiki — APU](https://www.nesdev.org/wiki/APU) — register map + channel descriptions.
15. [NESdev wiki — APU Frame Counter](https://www.nesdev.org/wiki/APU_Frame_Counter) — 4/5-step counter and IRQ.
16. [NESdev wiki — APU Mixer](https://www.nesdev.org/wiki/APU_Mixer) — linear and lookup-table mixers + analog filters.
17. [NESdev wiki — APU DMC](https://www.nesdev.org/wiki/APU_DMC) — delta modulation channel.
18. [NESdev wiki — DMA](https://www.nesdev.org/wiki/DMA) — OAM DMA + DMC DMA cycle stealing + 2A03 register-readout bug.
19. [NESdev wiki — INES](https://www.nesdev.org/wiki/INES) — iNES 1.0 file format.
20. [NESdev wiki — NES 2.0](https://www.nesdev.org/wiki/NES_2.0) — NES 2.0 file format.
21. [NESdev wiki — MMC1](https://www.nesdev.org/wiki/MMC1) — serial register protocol.
22. [NESdev wiki — MMC3](https://www.nesdev.org/wiki/MMC3) — banking + PPU A12 IRQ.
23. [NESdev wiki — MMC5](https://www.nesdev.org/wiki/MMC5) — most complex official mapper.
24. [NESdev wiki — Standard controller](https://www.nesdev.org/wiki/Standard_controller) — controller serial protocol.
25. [NESdev wiki — Controller reading](https://www.nesdev.org/wiki/Controller_reading) — reading code patterns.
26. [NESdev wiki — Cycle reference chart](https://www.nesdev.org/wiki/Cycle_reference_chart) — NTSC/PAL/Dendy cycle reference.
27. [NESdev wiki — Detect TV system](https://www.nesdev.org/wiki/Detect_TV_system) — region detection.
28. [NESdev wiki — Emulator tests](https://www.nesdev.org/wiki/Emulator_tests) — test ROM index.
29. [NESdev wiki — Errata](https://www.nesdev.org/wiki/Errata) — known hardware bugs catalog.
30. [NESdev wiki — PPU glitches](https://www.nesdev.org/wiki/PPU_glitches) — assorted PPU glitches.
31. [NESdev wiki — PPU power up state](https://www.nesdev.org/wiki/PPU_power_up_state) — power-up state.
32. [Visual6502.org](http://www.visual6502.org/) — transistor-accurate 6502 simulator.

### Tier 1 — primary author publications

33. [27C3 talk: Reverse Engineering the MOS 6502 CPU](https://media.ccc.de/v/27c3-4159-en-reverse_engineering_mos_6502) — Visual 6502 team's CCC talk.
34. [NESdev forum: PAL famiclone / Dendy timings F.A.Q.](https://forums.nesdev.org/viewtopic.php?t=20931) — primary author summary of region differences.
35. [NESdev forum: Importance of cycle accuracy?](https://forums.nesdev.org/viewtopic.php?t=25618) — cycle-accuracy decision factors.
36. [NESdev forum: DMC/DMA timing and quirks](https://forums.nesdev.org/viewtopic.php?t=25574) — DMA edge cases.
37. [NESdev forum: 6502 interrupt behaviour](https://forums.nesdev.org/viewtopic.php?t=2282) — long-running interrupt-detail thread.
38. [NESdev forum: Riding the open bus](https://forums.nesdev.org/viewtopic.php?t=12549) — open-bus modelling.

### Tier 2 — reliable secondary

39. [christopherpow/nes-test-roms](https://github.com/christopherpow/nes-test-roms) — archive of blargg, kevtris, et al. test ROMs.
40. [SourMesen/Mesen2](https://github.com/SourMesen/Mesen2) — gold-standard NES emulator (C++/C#).
41. [ares-emulator/ares](https://github.com/ares-emulator/ares) — higan-derived multi-system emulator.
42. [lukexor/tetanes](https://github.com/lukexor/tetanes) — most prominent Rust NES emulator.
43. [TetaNES Part 2 — Designs and Frustrations](https://lukeworks.tech/tetanes-part-2) — the Rust NES architecture postmortem.
44. [bugzmanov/nes_ebook](https://bugzmanov.github.io/nes_ebook/) — Rust NES tutorial.
45. [starrhorne/nes-rust](https://github.com/starrhorne/nes-rust) — Rust reference impl.
46. [Determinant/runes](https://github.com/Determinant/runes) — no_std Rust NES core.
47. [zeta0134/rustico](https://github.com/zeta0134/rustico) — Rust NES + browser.
48. [100thCoin/AccuracyCoin](https://github.com/100thCoin/AccuracyCoin) — single-ROM accuracy battery.
49. [TASvideos — NES Accuracy Tests](https://tasvideos.org/EmulatorResources/NESAccuracyTests) — community-curated accuracy test list.
50. [Emulation General Wiki — NTSC filters](https://emulation.gametechwiki.com/index.php/NTSC_filters) — survey of NTSC filters (Blargg, Themaister).
51. [Filthy Pants — Multipass Shaders: NTSC, motion blur and more](http://filthypants.blogspot.com/2013/04/multipass-shaders-ntsc-motion-blur-and.html) — CRT+NTSC shader composition.

### Tier 2 — reference implementations and tools

52. [nes-test-roms/other/nestest.txt](https://github.com/christopherpow/nes-test-roms/blob/master/other/nestest.txt) — nestest documentation.
53. [nes-test-roms/other/nestest.log](https://github.com/christopherpow/nes-test-roms/blob/master/other/nestest.log) — Nintendulator golden master log.

### Cross-references searched but not deeply fetched

54. [Wikipedia — Ricoh 2A03](https://en.wikipedia.org/wiki/Ricoh_2A03) — overview.
55. [Wikipedia — Memory management controller (Nintendo)](https://en.wikipedia.org/wiki/Memory_management_controller_(Nintendo)) — mapper overview.
56. [Marat Fayzullin's iNES doc](http://fms.komkon.org/iNES/iNES.html) — alternate iNES reference.
57. [FCEUX PPU help](https://fceux.com/web/help/PPU.html) — FCEUX-flavored PPU notes.
58. [Famicom Party — chapter 9 (PPU)](https://book.famicom.party/chapters/09-theppu.html) — programming guide.
59. [emudev.de mapper articles](https://emudev.de/nes-emulator/about-mappers-mmc1-and-mmc3/) — implementer-focused mapper writeups.
60. [Slack.net APU sound hardware reference](https://www.slack.net/~ant/nes-emu/apu_ref.txt) — Blargg's APU reference.

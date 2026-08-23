# The MiSTer framework: `emu`, `sys/`, `hps_io`, video, audio and memory

**Dated supplemental reference, 2026-08-23.** `ref-docs/` is immutable.

**Primary sources**, fetched 2026-08-23:
`https://mister-devel.github.io/MkDocs_MiSTer/developer/emu/` ·
`.../developer/hps_io/` · `.../developer/conf_str/` · `.../developer/porting/` ·
`https://github.com/MiSTer-devel/Template_MiSTer`.

This is rung-6 material. It is written down now so the PPU and APU rungs can be
designed with the integration constraints known, rather than discovering at
v2.6.5 that a video path has to be rebuilt.

---

## 1. The core is not the top level

A MiSTer core implements a module named **`emu`**, which `sys_top.v` instantiates.
The framework — not the core — owns HDMI scaling, the OSD, audio output and input
handling. Everything under `sys/` is identical across cores and **must not be
modified**.

The practical consequence for this project: **`nes_top.sv` is not the deliverable
top level.** It becomes an inner module of `emu`, and the co-simulation testbench
keeps driving `nes_top` directly — which is exactly the separation ADR 0037 wants,
since `tb/` is never in `files.qip`.

## 2. Clocking

- **`CLK_50M`** is the board reference, fed to a PLL that the framework expects to
  be named `pll` with instance name `pll`.
- **`CLK_AUDIO`** is a fixed 24.576 MHz reference.

The Fabric plan already fixed the core's internal clocking: a single
**21.477272 MHz `clk_sys`** with a **mod-12 master phase counter** (÷4 → PPU dot,
÷12 → CPU cycle, low/high halves giving M2 phase). That is derived from the PLL
here; nothing about this framework requirement changes it.

## 3. Video — mandatory

| Signal | Meaning |
|---|---|
| `CLK_VIDEO` | Base pixel clock, typically `clk_sys` |
| `CE_PIXEL` | Clock enable derived from `CLK_VIDEO` — this is how variable resolutions work |
| `VGA_R/G/B` | 8-bit per channel |
| `VGA_HS/VS` | Sync |
| `VGA_DE` | Display enable, `~(HBlank \| VBlank)` |
| `VGA_F1` | Interlace field |
| `VGA_SL[1:0]` | Scanline control |
| `VIDEO_ARX/ARY[12:0]` | Aspect ratio; bit 12 set means bits [11:0] are scaled dimensions |

**Note for this core:** the NES is 256×240 at 8:7 pixel aspect. RustyNES's own
desktop frontend applies 8:7, and the libretro wrapper shipped `aspect_ratio = 0.0`
(square pixels) as a defect until v2.3.5 — so this is a known trap in this project
specifically, and `VIDEO_ARX/ARY` must be set deliberately rather than defaulted.

The optional framebuffer path (`MISTER_FB`) is **not** wanted: it renders through
DDRAM and is for cores that produce frames rather than scanlines. A cycle-accurate
PPU produces pixels at `CE_PIXEL`, which is the direct path.

## 4. Audio — mandatory

`AUDIO_L/R[15:0]`, `AUDIO_S` (1 = signed), `AUDIO_MIX[1:0]` (0/25/50/100% mono
blend).

**Design consequence, and it matters for rung 4.** The framework takes 16-bit
integer samples. RustyNES's non-linear mixer and BLEP resampler are *software*
artifacts with no hardware counterpart — which is why rung 4 gates on the
**integer channel levels** in `MixRecord` (pulse/triangle/noise 0–15, DMC 0–127)
and never on the mixed `f32`. The RTL produces its own mix into these 16 bits; the
oracle comparison stops at the channel level.

## 5. `hps_io` — mandatory

`HPS_BUS[45:0]` is passed straight into `hps_io`, which abstracts ARM
communication: ROM download, status bits from the OSD, buttons, joysticks, RTC,
and the `CONF_STR` menu. It is how a core receives a cartridge at all.

`CONF_STR` is the OSD menu definition — a string of options the framework parses.
For this core it will carry at minimum: region (NTSC/PAL/Dendy), aspect ratio,
scanline/blend video options, and reset.

## 6. Memory — SDRAM versus DDR3

| | SDRAM | DDR3 (DDRAM) |
|---|---|---|
| Latency | Low, deterministic | ~20+ cycles |
| Interface | Direct address/data bus | Request/response with `DDRAM_BUSY`, burst up to 128 words |
| Suits | Cartridge ROM read on demand | Bulk/streaming, framebuffers |

**A NES core needs SDRAM.** The CPU and PPU read cartridge ROM directly, on
demand, with no tolerance for a 20-cycle response — which is why the DE10-Nano
**SDRAM add-on board is mandatory** for any NES core and why the SuperStation One
(128 MB integrated) avoids the prerequisite entirely.

Signals: `SDRAM_CLK/CKE/A[12:0]/BA[1:0]/DQ[15:0]/DQML/DQMH/nCS/nCAS/nRAS/nWE`.

**Scope note:** NROM is 327 Kb and fits entirely in on-chip M10K. Rungs 3–6 need
**no external memory at all**. The SDRAM controller is a rung-7 item, forced by
MMC3's 6 Mb — which is why the plan puts hardware bring-up (rung 6) *before* it.

## 7. Other interfaces, all optional here

`UART_*`, `SD_*`, `ADC_BUS`, `USER_IN/OUT` — none needed for an NES core.
`LED_USER`/`LED_POWER`/`LED_DISK` and `BUTTONS` are optional but cheap.

## 8. Optional helper modules

`video_mixer`, `video_freak`, `arcade_video` provide gamma, scaling and
scandoubling. Optional for basic VGA output; `video_mixer` is the conventional
choice for a console core and is worth using rather than reimplementing.

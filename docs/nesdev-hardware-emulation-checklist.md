# Nesdev Hardware And Emulation Checklist

**Derived from:** `ref-docs/nesdev-wiki-technical-report.md`
**Primary upstream:** [Nesdev Wiki](https://www.nesdev.org/wiki/Nesdev_Wiki)
**Purpose:** convert the Nesdev-derived reference report into an actionable
accuracy checklist for RustyNES v2 subsystem docs, tests, and v1.x TODOs.

This document is intentionally not a wiki mirror. It is the project-facing
control list: if a hardware behavior is relevant to this emulator, it should
either be implemented, explicitly deferred, or covered by a TODO.

> **Status refresh (v2.1.0, 2026-06-10).** The "Project status" cells below were
> updated to reflect that the **R1 master-clock accuracy program is complete**
> (AccuracyCoin **100.00% / 139**): the C1 IRQ trio, `$2007`/sprite-eval sub-cycle,
> SH\*, and region (3.2:1 PAL) clusters are all closed; MMC5/VRC7 expansion audio,
> Four Score + Vaus + Zapper input, and **FDS (v2.2.0: drive + IRQs + writable
> disks + 2C33 audio)** have landed. **What remains on this checklist:** the
> DMC-controller-conflict model, the two `apu_reset` residuals, `mmc3_test_2/4` #3
> (deferred), and the Vs./PlayChoice-10 RGB PPUs (a separate platform initiative);
> the next major feature is netplay. The authoritative forward roadmap is the
> post-v2.1.0 gap analysis (`~/.claude/plans/toasty-noodling-scroll.md`).

## Source Priority

Use this hierarchy when references disagree:

1. Passing hardware test ROM behavior in this repository.
2. Nesdev hardware pages and linked forum investigations.
3. Mesen2 or another accuracy-first emulator trace used as an oracle.
4. Older standalone references, only when they do not contradict the above.

Primary source clusters:

- CPU: [CPU](https://www.nesdev.org/wiki/CPU),
  [CPU power up state](https://www.nesdev.org/wiki/CPU_power_up_state),
  [Status flags](https://www.nesdev.org/wiki/Status_flags),
  [CPU interrupts](https://www.nesdev.org/wiki/CPU_interrupts),
  [Instruction reference](https://www.nesdev.org/wiki/Instruction_reference).
- PPU: [PPU](https://www.nesdev.org/wiki/PPU),
  [PPU power up state](https://www.nesdev.org/wiki/PPU_power_up_state),
  [PPU registers](https://www.nesdev.org/wiki/PPU_registers),
  [PPU rendering](https://www.nesdev.org/wiki/PPU_rendering),
  [PPU scrolling](https://www.nesdev.org/wiki/PPU_scrolling),
  [PPU sprite evaluation](https://www.nesdev.org/wiki/PPU_sprite_evaluation).
- APU/DMA/input: [APU](https://www.nesdev.org/wiki/APU),
  [APU Frame Counter](https://www.nesdev.org/wiki/APU_Frame_Counter),
  [DMA](https://www.nesdev.org/wiki/DMA),
  [Controller reading](https://www.nesdev.org/wiki/Controller_reading),
  [Input devices](https://www.nesdev.org/wiki/Input_devices).
- Cartridge: [iNES](https://www.nesdev.org/wiki/INES),
  [NES 2.0](https://www.nesdev.org/wiki/NES_2.0),
  [Mapper](https://www.nesdev.org/wiki/Mapper),
  [Bus conflict](https://www.nesdev.org/wiki/Bus_conflict),
  [Cartridge board reference](https://www.nesdev.org/wiki/Cartridge_board_reference).
- Validation: [Emulator tests](https://www.nesdev.org/wiki/Emulator_tests),
  [Tricky-to-emulate games](https://www.nesdev.org/wiki/Tricky-to-emulate_games),
  [Game bugs](https://www.nesdev.org/wiki/Game_bugs).

## CPU Checklist

| Behavior | Required emulator treatment | Project status |
|---|---|---|
| No decimal arithmetic | Keep D flag observable, but ADC/SBC ignore decimal mode | Implemented; documented in `docs/cpu-6502.md` |
| Status flags | Treat bits 5 and 4 as stack-push artifacts, not internal CPU latches | Implemented; add tests for PHP/BRK/IRQ/NMI stack values when touching interrupt code |
| Reset sequence | Suppress reset stack writes but decrement S by 3 and fetch $FFFC/$FFFD | Implemented; cold-boot SP regression tests exist |
| Power-up register state | A/X/Y normally 0 on tested hardware; RAM is unreliable; reset preserves most state | Deterministic test mode (default) + seeded randomized developer mode (`Nes::from_rom_with_power_on_seed`) both landed v1.5.0 (T-72-002) |
| Unofficial opcodes | Implement all NMOS 6502 unofficial opcodes including unstable store family | Implemented; **SH\* pass 6/6 in AccuracyCoin under R1** — the deeper internal/external data-bus split is coarse but no test demands more |
| Interrupt polling | NMI edge-sensitive, IRQ level-sensitive, BRK/IRQ/NMI vector hijacking | **Closed under the R1 master clock (v2.0.0):** `cpu_interrupts_v2` 5/5 strict; the only residual is `mmc3_test_2/4` #3 (ADR-0002 axis, deferred) |
| Dummy reads/writes | Preserve all documented dummy bus accesses and RMW double writes | Implemented for major suites; internal-vs-external bus modeling remains a v1.x TODO |
| DMA halt eligibility | DMA can halt only on CPU read cycles | Implemented; continue to guard with DMC/OAM DMA tests |

## PPU Checklist

| Behavior | Required emulator treatment | Project status |
|---|---|---|
| Post-reset write mask | Ignore early writes to $2000/$2001/$2005/$2006 until first pre-render/vblank window | Implemented for NTSC/PAL timing; documented in `docs/ppu-2c02.md` |
| Register latch/open bus | Model `_io_db` dynamic latch, write-only reads, and unused PPUSTATUS bits | Implemented with coarse decay; refine only if tests demand |
| PPUDATA buffering | Non-palette reads return old buffer, palette reads bypass while updating buffer side effects | Implemented; the rendering-time `$2007` residual is **closed under R1** (`$2007` Stress 170/170 stable dots) |
| Loopy v/t/x/w | Keep scroll latch behavior and render-time horizontal/vertical reload timing | Implemented; guarded by PPU tests and visual baselines |
| Background fetch pipeline | Preserve all visible, prefetch, and extra nametable fetches | Implemented; Mesen2 trace tooling documents exact pipeline |
| Sprite evaluation | Per-dot secondary-OAM clear, copy, overflow bug, and OAMADDR walk | Implemented; the sub-cycle flag-timing residuals **closed under R1** (AccuracyCoin sprite-eval 9/9) |
| Sprite 0 hit | Set during rendering with left-edge, dot, and pre-render restrictions | Implemented; residual stale-shifter cases tracked |
| NTSC odd-frame skip | Skip dot on odd frames only when rendering is enabled | Implemented and tested by `ppu_vbl_nmi` |
| Region variants | PAL and Dendy timing must not be inferred from NTSC constants | **Region-exact under the R1 master clock (v2.0.0):** 3:1 NTSC/Dendy, hardware-true **3.2:1 PAL**; `region_timing` 4/4 |
| PPU variants | 2C03/2C04/2C05/Vs. palettes and behavior are separate from stock 2C02 | Out of scope — a separate platform initiative (Vs. System / PlayChoice-10); load-time diagnostics only |

## APU, DMA, And Input Checklist

| Behavior | Required emulator treatment | Project status |
|---|---|---|
| Frame counter write delay | $4017 effects occur after 3 or 4 CPU clocks depending on APU-cycle phase | Implemented for test ROMs; keep in APU docs |
| 4-step IRQ | Frame IRQ is set only in 4-step mode when IRQ inhibit is clear; $4015 read clears old flag | Implemented with residual AccuracyCoin frame-IRQ cases |
| 5-step mode | No frame IRQ; mode write can clock quarter/half frame units | Implemented |
| Nonlinear mixer | Use nonlinear pulse and TND paths before console filtering | Implemented |
| DMC load vs reload DMA | Model different scheduling phase, dummy cycle, and alignment cycle | Implemented; residual $4015/$4016 bracket cases remain |
| DMA register-read bugs | Repeated halted reads must affect $2007/$4015/$4016/$4017 side-effect registers | Implemented enough for blargg; AccuracyCoin residuals remain |
| Controller strobe/read | $4016 low 3 bits latch OUT lines; $4016/$4017 reads clock device and return D0-D4 plus open bus | Standard pads + **Four Score (v1.7.0)** + **Arkanoid Vaus paddle + Zapper (v2.1.0)** via the opt-in per-port `InputDevice` overlay; microphone / other expansion devices remain deferred |
| DMC controller conflict | Reads can lose or duplicate joypad bits during DMC DMA | Known; **still untested/undiagnosed** (`read_joy3/count_errors`, `sprdma_and_dmc_dma`) — an ongoing accuracy-polish item |

## Cartridge And Mapper Checklist

| Behavior | Required emulator treatment | Project status |
|---|---|---|
| iNES variants | Detect NES 2.0 before trusting upper header bytes; tolerate old dirty iNES padding carefully | Implemented; doc now calls out dirty-header risk |
| NES 2.0 metadata | Preserve mapper, submapper, region, RAM/NVRAM, console type, and default device fields | Parser supports core fields; default-device/input integration is v1.x |
| Nametable layout naming | Prefer explicit CIRAM A10 behavior over ambiguous "horizontal/vertical mirroring" prose | Documented in `docs/mappers.md` and `docs/cartridge-format.md` |
| Bus conflicts | For discrete boards, mapper sees CPU value AND PRG ROM byte at the written address | Implemented for supported conflict mappers |
| MMC1 | Serial 5-write protocol, reset write, and consecutive-cycle write ignore behavior | Implemented |
| MMC3 | Filtered PPU A12 rising edge after sufficient low time; revision-sensitive IRQ behavior | Implemented with one scanline-timing residual |
| MMC5 | PRG/CHR modes, ExRAM, fill, split, multiplier, scanline IRQ, audio | Feature-complete incl. audio (v1.x); only the >8 KiB multi-chip PRG-RAM configs remain (long-tail policy, no fixture) |
| Expansion audio | VRC6, Sunsoft 5B, Namco 163, MMC5, VRC7, FDS require mapper/APU integration | **All landed** via `mix_audio`→`tick_with_external`: VRC6 / 5B / N163 / MMC5 / VRC7 OPLL FM (v1.1.0) / **FDS 2C33 wavetable (v2.2.0)** |
| FDS | BIOS, disk timing, IRQs, writable media, and FDS audio are a separate platform surface | **Supported (v2.2.0):** parser + RAM adaptor (mapper 20) + user-supplied `disksys.rom` BIOS + read/write drive + timer/transfer IRQs + writable `.fds.sav` + 2C33 wavetable audio. Real-BIOS boot unverified in CI (BIOS non-distributable); device + audio unit-tested |

## Validation Coverage Checklist

| Area | Required coverage | Current action |
|---|---|---|
| CPU reset/power | `cpu_reset`, power-on unit tests, randomized RAM developer mode | Add missing external ROM fixture if license permits |
| CPU instruction edges | `instr_test_v5`, `instr_misc`, `cpu_timing_test6`, dummy reads/writes | `instr_misc` (5) + `instr_timing` (2) + `cpu_reset` vendored + wired v1.5.0 (T-71-002/003) |
| CPU interrupts | `cpu_interrupts_v2`, IRQ trace fixture, mapper IRQ tests | C1 residual carried forward |
| PPU vblank/open bus | `ppu_vbl_nmi`, `ppu_open_bus`, PPU state traces | Passing; keep traces for future changes |
| Sprites/OAM | sprite hit/overflow, `oam_read`, `oam_stress`, AccuracyCoin | Residuals tracked under Cascade A |
| APU/DMA | `apu_test`, `apu_mixer`, `dmc_dma_during_read4` | Passing; AccuracyCoin residuals remain |
| Mapper timing | `mmc3_test_2`, `mmc3_irq_tests`, Holy Mapperel, mapper-specific ROMs | MMC3 sub-test #3 residual; VRC24 test source unresolved |
| Input | Standard pads, DMC conflict, Four Score/Zapper/expansion devices | Standard pads only; expanded devices are v1.x |
| Commercial canaries | User-supplied external snapshots, never committed ROM bytes | 60-ROM oracle exists |

## Maintenance Rules

1. Any subsystem doc that cites Nesdev should link both the local report and
   the exact upstream page used for the behavior.
2. Any deferred behavior in this checklist needs a TODO ticket under `to-dos/`.
3. If a test ROM contradicts prose in this document, keep the failing test as
   the authoritative evidence and update this checklist after diagnosis.
4. Do not paste full Nesdev tables unless the table is small and needed locally;
   cite the source page for exhaustive tables.
5. Keep commercial-ROM references limited to hashes, behavior, mapper, and user
   reproduction instructions. Do not commit commercial ROM data.

## Phase 7 audit status (v1.5.0, 2026-05-24)

**T-71-001 source-map audit — DONE.** Each subsystem doc was verified to cite
both the local report and an exact upstream Nesdev page: `docs/cpu-6502.md` (5),
`docs/ppu-2c02.md` (6), `docs/apu-2a03.md` (6), `docs/mappers.md` (6),
`docs/cartridge-format.md` (3) carry `nesdev.org/wiki` references and link
`nesdev-wiki-technical-report.md` / this checklist. No stale or missing links
were found that required correction. See
`docs/audit/phase-7-assessment-2026-05-24.md` for the full Phase 7 disposition
(implemented / explicitly-out-of-scope / test-guarded per checklist row).

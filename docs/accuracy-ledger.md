# Accuracy Ledger

The single at-a-glance record of where RustyNES is **cycle-exact**, where it uses
a **documented approximation**, and where a behavior is a **by-design non-goal**
or **out of scope**. This exists so an audit can tell those apart — "not modeled"
is very different from "no oracle exists to model against" or "deliberately out of
scope." `docs/STATUS.md` remains the authoritative per-suite pass-count record;
this ledger is the approximation map. The remediation line is **v2.1.0
"Fathom"** (accuracy work, shipping ahead of the v2.2.0 mobile store launch).

**Headline:** every suite on the oracle path is green — AccuracyCoin **141/141
(100.00%)**, nestest 0-diff, `cpu_interrupts_v2` 5/5, `ppu_sprites` 19/19,
mmc3 A12-IRQ 18/18, `ppu_vbl_nmi` 10/10, the 60-ROM commercial byte-identity
oracle 60/60. The items below are the residual approximations, each with its
disposition under the v2.1.0 "Fathom" accuracy-remediation line
(`to-dos/plans/v2.1.0-fathom-accuracy-remediation-plan.md`).

## Legend

- **Remediated** — closed (or being closed) in the Fathom line.
- **No stricter oracle** — the reference test only defines a tolerance band /
  visual bar; there is nothing to tighten against. Not a defect.
- **Deferred** — genuine engineering, scheduled but not yet done.
- **Out of scope** — deliberately not modeled.

| Area | Nature | Oracle | Disposition |
|---|---|---|---|
| Palette backdrop-override ($3F00-$3FFF, render disabled) | Was not modeled; PPU outputs `palette[v & 0x1F]` not the backdrop | `full_palette` / `flowing_palette` visual corpus | **Remediated** (F1.1) |
| OAMADDR forced to 0 across dots 257-320 + `$2004` `$E3` attribute-byte mask | Already modeled; both correct | AccuracyCoin `$2004` / `Sprite0Hit` (141/141) | **Verified** — F1.2 added a fast unit regression guard |
| `OAMADDR & 0xF8` render-start OAM copy | Not modeled (revision-dependent, unreliable) | none | **Out of scope** — Mesen2, ares, and TriCNES all deliberately omit it |
| PPU open-bus decay | Refresh map matches the Blargg `ppu_open_bus` table exactly; ~600 ms is the spec decay value (per-group model) | `ppu_open_bus` (verifies the map, not a stopwatch) | **Verified** — F1.3 audit + regression test; per-bit timing has no oracle and isn't pursued |
| Optional OAM decay (F2.3) | Dynamic-RAM sprite decay when rendering stays off: per-8-byte-row 3000-CPU-cycle refresh model (Mesen2 `ReadSpriteRam`/`WriteSpriteRam`), rows refreshed by sprite-eval + `$2004`/DMA, decayed rows read `((a&3)==2)?a&0xE3:a` | No decay test ROM (Mesen2 code-level parity); NTSC/Dendy-only | **Landed on `main`** (Unreleased, next tag; F2.3) — **default-off** so the golden vectors / AccuracyCoin / commercial oracle (all decay-off) stay byte-identical; a real core feature (affects the framebuffer when ON), deterministic off `dot_counter`, save-state v7 tail (relative-age) |
| NTSC generated palette (F1.4) | Optional composite-model base palette synthesized in-core (`rustynes_ppu::generate_base_palette`) in place of the hand table | `full_palette` visual corpus + Mesen2/ares baselines (visual only) | **Shipped** (v2.1.2) — **default-off**, golden-locked cross-target; select via Settings → Palette → "Generated NTSC". Default build byte-identical |
| NTSC composite shader ladder (F2.2) | Display-only GPU post-passes: simplified blur (`Ntsc`) → LMP88959 composite → Bisqwit per-dot (`CompositeRt`) | No pass/fail ROM (visual only); `visual_regression` stays byte-identical with any filter active | **Shipped** (v2.1.2) — three-rung ladder verified end-to-end, live emulator-synced dot-crawl now on both composite passes (`Lmp88959` phase wired), palette↔pass split documented; no separable-kernel rung (LMP covers that tier). See `docs/frontend.md` |
| Vs. `DualSystem` second screen | Core modeled + `sub_framebuffer()` exposed; desktop frontend now presents both screens (side-by-side / stacked), routes P1-P4 + coin, plays the main console's audio | Synth harness + boot of the 4 DualSystem titles (real-cabinet boot stays fixture-limited — see below) | **Shipped** (v2.1.2 F2.1, desktop) — advanced features (run-ahead / rewind / netplay / TAS / dual save-state) scoped out in dual mode per ADR 0032; wasm/mobile deferred |
| NSF non-60 Hz playback + NSFe | **Done** (F4.1/F4.2): the play-speed divider (`$6E-$6F`/`$78-$79`) is parsed and a non-standard rate (PAL 50 Hz / custom µs) drives `play` via a mapper cycle-timer IRQ (frame-IRQ-disabled); standard 60 Hz keeps the byte-identical vblank-NMI path. `NSFE` chunked container parsed (INFO/DATA/BANK/auth). | `nsf` unit + core integration tests | **Done** (F4.1/F4.2) |
| FDS medium model | No CRC/gaps-on-write, fixed-cycle seek (no analog seek) | Real-BIOS write-verify (copyright-gated, out of CI) | **Deferred** stretch (F4.3) |
| BestEffort mapper tier (26 families, was 112) | Register-decode + save-state round-trip only; off the oracle gate | `mapper_tier_honesty.rs` invariant | **Mostly remediated** (F3): 86 promoted to Curated with commercial-ROM oracle; the 26 left have no cleanly-booting dump (16 NES 2.0 high-id + 8 no-cart + 2 jam-at-boot) |
| MMC3 R1/R2 scanline-IRQ (ADR 0002) | ≤1-CPU-cycle differential on 4 `#[ignore]`'d sub-tests; zero game impact | `mmc3_test_2/4` #3 + siblings; `mmc3_r1r2_phase_probe` A12-phase golden probe (v2.1.5, `--features mmc3-a12-phase-probe`) | **CLOSED for the shipping default; axis-B candidate deferred to maintainer** (F5.0, ADR 0002). v2.1.5 direct instrumentation refined the closure: "no post-access qualifying rise" is ROM-specific (holds for the two `scanline_timing` #3 residuals, `irq_post=0`; **false** for `mmc3_test_v1/5`+`/6` #2, `irq_post=4` — post-access IRQ-clocking rises Session B never measured). Every *tested* lever stays non-curative (incl. the `mmc3-m2-phase-irq` deferral, byte-identical status on `/5`+`/6`); the four pins stay `#[ignore]`'d. One untested lever — an ares-style M2-edge-precise falling-edge low-time filter — is deferred to a maintainer decision (needs a sacred-gate-risking substrate change to prototype) |
| APU non-linear mixer | Lookup-table matches within the `apu_mixer` band | `apu_mixer` (analog-cancellation, tolerance) | **No stricter oracle** — the LUT already passes; ±4% is honest |
| PAL APU frame-counter step positions | Not modeled: `frame_counter.rs` clocks the sequencer at the NTSC step positions (7457/14913/22371/29828-30, 37281-82) unconditionally; the PAL positions (8313/16627/24939/33252-53, …) are absent. PAL *scheduler* timing (3.2:1, 50 Hz, PAL DMC/noise tables) **is** modeled | `pal_apu_tests` (10 sub-ROMs, forced PAL, on-screen verdict via `run_nes_screen`) | **Deferred** (v2.1.5, honestly pinned) — 3/10 pass (region-independent: `01.len_ctr`, `02.len_table`, `03.irq_flag`); 7/10 fail (all PAL step-timing: clock jitter, mode-0/1 length timing, IRQ-flag/IRQ timing, length halt/reload). Fails identically under NTSC region → the timing *model*, not region select. **Not an NTSC defect** (NTSC frame counter oracle-exact: AccuracyCoin APU 141/141, `apu_test` 8/8). Residuals pinned fail-loud in `tests/pal_apu_tests.rs`; a fix region-parameterizes the step constants behind the existing `Region`, keeping NTSC byte-identical |
| APU analog HPF/LPF chain | Fixed-coefficient 3-pole | No pass/fail ROM | **No stricter oracle** (optional measured-RC future work) |
| PlayChoice-10 Z80 second-screen menu | Not modeled | — | **Out of scope** |
| MMC1 software WRAM write-protect | `mmc1.rs` reads/writes `$6000-$7FFF` `prg_ram` **unconditionally** and does not model the software power-off write-protect (MMC1 `$E000` bit 4; SNROM's second `$A000` bit-4 layer) | Holy Mapperel `M1_*` "detailed result" WRAM nibble (`1000` SJROM = `$E000` layer; `5000` SNROM = both layers) | **Deferred** — a widely-shared simplification (Holy Mapperel's README notes FCEUX / PowerPak omit it too; modelling MMC1 RAM-disable is a known game-compat hazard). *Not* a bank-reachability defect: every bank is reachable. Pinned honestly (not blind-passed) by the **v2.1.5 holy_mapperel bank-reachability regression net** |
| FME-7 open bus on RAM-selected-but-disabled `$6000-$7FFF` | FME-7 **does** model the command-`$8` RAM-enable (bit 7) / RAM-select (bit 6) bits — `sprint3.rs` maps PRG-RAM only when both are set and a PRG-ROM bank when RAM is deselected. The narrow gap: the "RAM selected but disabled" state (bit 6 = 1, bit 7 = 0) should read back as **open bus** but falls through to the PRG-ROM bank (`value & $3F` = last 8 KiB bank) | Holy Mapperel `M69_*` "detailed result" WRAM nibble `1000` — the driver's third "read open bus" sub-check requires a byte `>= 3` (`$7F`) but reads the last-bank tag `1`, so it sets `MAPTEST_WRAMEN` | **Deferred** — a single-register open-bus edge; no known FME-7 game reads `$6000-$7FFF` in the RAM-selected-yet-disabled state, so a fix is not provably byte-identical against the commercial oracle. *Not* a bank-reachability or RAM-enable-modelling defect, and the FME-7 IRQ nibble is `0` (IRQ works). Pinned honestly by the **v2.1.5 holy_mapperel bank-reachability regression net** |

## Oracles / regression nets

- **Holy Mapperel bank-reachability + IRQ net** (v2.1.5, `crates/rustynes-test-harness/tests/holy_mapperel.rs`, `--features test-roms`): the 17 committed zlib-licensed ROMs (`tests/roms/holy_mapperel/`) each run to their settled result screen, pinned by an `insta` framebuffer-hash snapshot with settled + non-blank structural guards. Catches silent mapper-detection / bank-layout / RAM-sizing / IRQ regressions the `AccuracyCoin` / blargg suites don't cover; 15/17 detect + reach all banks with detailed code `0000`; the two MMC1 rows carry the documented MMC1 WRAM write-protect residual and the two FME-7 rows the documented FME-7 open-bus-on-disabled-RAM residual above.

## Notes

- **Determinism boundary.** Display-only work (the NTSC composite filter/shader
  ladder) stays in the frontend/shader and never feeds the framebuffer/audio
  golden-vector or save-state hash. Two accuracy features DO change deterministic
  core output when enabled — the generated NTSC palette (F1.4) and optional OAM
  decay (F2.3) — so both ship **default-off**: with the default the golden vectors
  / AccuracyCoin / commercial oracle / save-state hashes are byte-identical, and
  turning either on is a deliberate, deterministic opt-in (OAM decay is driven off
  the monotonic `dot_counter`, never wall-clock, and its per-row state round-trips
  the save-state via the v7 relative-age tail).

## Ignored-test dispositions (all 20)

Every `#[ignore]`'d test in the workspace, with its disposition. **None is an
open accuracy gap** — each is either a superseded historical pin, a by-design
revision/fixture/network limitation, or the MMC3 residual (closed for the
shipping default; one axis-B lever deferred to a maintainer decision).

| Group | Count | Tests | Disposition |
|---|---|---|---|
| Permanent historical pins | 7 | APU `$4015`-load / reload-arm / `put_cycle` (`apu.rs`), CPU interrupt-dispatch ×3 (`opcodes.rs`), PPU BG-shifter (`ppu.rs`) | Pin **superseded pre-master-clock** unit assertions on mock buses; the master-clock core is the only scheduler, so these legitimately cannot be un-ignored. Real coverage supersedes them: AccuracyCoin 100%, `cpu_interrupts_v2` 5/5 strict, `visual_regression` 7/7 |
| MMC3 R1/R2 scanline-IRQ | 4 | `mmc3_test_2/4` #3, `mmc3_test_v1/4` #3, `/5` #2, `/6` #2 | **CLOSED for the shipping default** (ADR 0002 F5.0, 2026-07-09; refined 2026-07-11) — a ≤1-CPU-cycle differential; **zero production-ROM impact**; 21+ falsified levers, all *tested* levers non-curative. The v2.1.5 A12-phase probe (`mmc3_r1r2_phase_probe`) showed post-access IRQ-clocking rises DO exist on `/5`+`/6` (`irq_post=4`) — one untested axis-B lever (M2-edge low-time filter) is deferred to a maintainer decision. Fail-loud `*_currently_fails` companions stay |
| MMC3 NEC-rev-B | 1 | `mmc3_test_2/6-MMC3_alt` | By-design: only one of the two *opposite* silicon revisions can pass; the project defaults to Sharp rev A (sub-ROM 5 passes strictly) |
| Vs. DualSystem GVS boots | 5 | `vs_dualsystem` boot ×4 + 1 combined-dump diagnostic | Fixture-limited: the staged GVS dumps are the MAME maincpu half only (sub-CPU PRG absent), so boot cannot complete; needs a combined 64 KiB dual dump (see `docs/audit/vs-dualsystem-combined-dumps-2026-07-02.md`) |
| Live-network probes | 2 | `stun_probe`, `turn_probe` | Hit live public STUN / TURN servers; `#[ignore]`'d so CI/offline runs stay hermetic — run manually with `--ignored` |
| HD-pack local | 1 | `hdpack` | Needs a copyrighted local HD pack via `RUSTYNES_HDPACK_LOCAL` |

The 16 in the last five rows are documented in `docs/testing-strategy.md`; the 4
MMC3 R1/R2 pins are dispositioned in ADR 0002's F5.0 decision-update (closed for
the shipping default; the v2.1.5 A12-phase instrumentation study
(`mmc3_r1r2_phase_probe`) refined the rationale and deferred one untested axis-B
lever to a maintainer decision).

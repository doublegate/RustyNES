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
| NTSC generated palette (F1.4) | Optional composite-model base palette synthesized in-core (`rustynes_ppu::generate_base_palette`) in place of the hand table | `full_palette` visual corpus + Mesen2/ares baselines (visual only) | **Shipped** (v2.1.2) — **default-off**, golden-locked cross-target; select via Settings → Palette → "Generated NTSC". Default build byte-identical |
| NTSC composite shader (F2.2) | Digital framebuffer only; the analog look is a display-only shader | No pass/fail ROM (visual only) | **Deferred** (F2.2 — the `Ntsc`/`Lmp88959`/`CompositeRt` passes exist; verification + palette-fed wiring pending) |
| Vs. `DualSystem` second screen | Core modeled + `sub_framebuffer()` exposed; no frontend presents it | Boot of the 4 DualSystem titles | **Deferred** (F2.1 frontend) |
| NSF non-60 Hz playback + NSFe | **Done** (F4.1/F4.2): the play-speed divider (`$6E-$6F`/`$78-$79`) is parsed and a non-standard rate (PAL 50 Hz / custom µs) drives `play` via a mapper cycle-timer IRQ (frame-IRQ-disabled); standard 60 Hz keeps the byte-identical vblank-NMI path. `NSFE` chunked container parsed (INFO/DATA/BANK/auth). | `nsf` unit + core integration tests | **Done** (F4.1/F4.2) |
| FDS medium model | No CRC/gaps-on-write, fixed-cycle seek (no analog seek) | Real-BIOS write-verify (copyright-gated, out of CI) | **Deferred** stretch (F4.3) |
| BestEffort mapper tier (26 families, was 112) | Register-decode + save-state round-trip only; off the oracle gate | `mapper_tier_honesty.rs` invariant | **Mostly remediated** (F3): 86 promoted to Curated with commercial-ROM oracle; the 26 left have no cleanly-booting dump (16 NES 2.0 high-id + 8 no-cart + 2 jam-at-boot) |
| MMC3 R1/R2 scanline-IRQ (ADR 0002) | ≤1-CPU-cycle differential on 4 `#[ignore]`'d sub-tests; zero game impact | `mmc3_test_2/4` #3 + siblings | **CLOSED by-design-permanent** (F5.0, ADR 0002) — differential 1-dot deficit, structurally unreachable; 21+ falsified levers |
| APU non-linear mixer | Lookup-table matches within the `apu_mixer` band | `apu_mixer` (analog-cancellation, tolerance) | **No stricter oracle** — the LUT already passes; ±4% is honest |
| APU analog HPF/LPF chain | Fixed-coefficient 3-pole | No pass/fail ROM | **No stricter oracle** (optional measured-RC future work) |
| PlayChoice-10 Z80 second-screen menu | Not modeled | — | **Out of scope** |

## Notes

- **Determinism boundary.** Anything display-only (NTSC filter, optional OAM
  decay) stays in the frontend/shader and never feeds the framebuffer/audio
  golden-vector or save-state hash. The generated NTSC palette (F1.4) is the one
  exception that changes framebuffer output, hence it ships default-off with a
  deliberate visual re-bless before any promotion.

## Ignored-test dispositions (all 20)

Every `#[ignore]`'d test in the workspace, with its disposition. **None is an
open accuracy gap** — each is either a superseded historical pin, a by-design
revision/fixture/network limitation, or the now-closed MMC3 residual.

| Group | Count | Tests | Disposition |
|---|---|---|---|
| Permanent historical pins | 7 | APU `$4015`-load / reload-arm / `put_cycle` (`apu.rs`), CPU interrupt-dispatch ×3 (`opcodes.rs`), PPU BG-shifter (`ppu.rs`) | Pin **superseded pre-master-clock** unit assertions on mock buses; the master-clock core is the only scheduler, so these legitimately cannot be un-ignored. Real coverage supersedes them: AccuracyCoin 100%, `cpu_interrupts_v2` 5/5 strict, `visual_regression` 7/7 |
| MMC3 R1/R2 scanline-IRQ | 4 | `mmc3_test_2/4` #3, `mmc3_test_v1/4` #3, `/5` #2, `/6` #2 | **CLOSED by-design-permanent** (ADR 0002 F5.0, 2026-07-09) — a differential 1-dot deficit that is structurally unreachable on the one-clock batched-catch-up model; 21+ falsified levers; **zero production-ROM impact**. Fail-loud `*_currently_fails` companions stay |
| MMC3 NEC-rev-B | 1 | `mmc3_test_2/6-MMC3_alt` | By-design: only one of the two *opposite* silicon revisions can pass; the project defaults to Sharp rev A (sub-ROM 5 passes strictly) |
| Vs. DualSystem GVS boots | 5 | `vs_dualsystem` boot ×4 + 1 combined-dump diagnostic | Fixture-limited: the staged GVS dumps are the MAME maincpu half only (sub-CPU PRG absent), so boot cannot complete; needs a combined 64 KiB dual dump (see `docs/audit/vs-dualsystem-combined-dumps-2026-07-02.md`) |
| Live-network probes | 2 | `stun_probe`, `turn_probe` | Hit live public STUN / TURN servers; `#[ignore]`'d so CI/offline runs stay hermetic — run manually with `--ignored` |
| HD-pack local | 1 | `hdpack` | Needs a copyrighted local HD pack via `RUSTYNES_HDPACK_LOCAL` |

The 16 in the last five rows are documented in `docs/testing-strategy.md`; the 4
MMC3 R1/R2 pins are closed in ADR 0002's F5.0 decision-update.

# Archived to-do folders

These directories are kept for historical traceability only. They are **not**
live work plans — do not pick tickets from them.

### Engine-lineage phase plans (the v1.0.0 production core)

These are the phase-and-sprint plans whose increments produced the v1.0.0
cycle-accurate core. They are delivered; the core ships at AccuracyCoin
**100% (139/139)**. Archived 2026-06-15.

| Folder | What it was |
|---|---|
| `phase-1-foundation/` | Workspace + CI + cartridge parser + CPU core (nestest 0-diff). Delivered. |
| `phase-2-graphics-timing/` | PPU, the dot-resolution lockstep scheduler, and the simple mappers. Delivered. |
| `phase-3-audio-polish/` | APU channels, DMC + DMC DMA, the non-linear mixer + band-limited synthesis. Delivered. |
| `phase-4-mapper-coverage/` | MMC3 + the misc/VRC/MMC5 mapper families. Delivered. |
| `phase-5-frontend-tooling/` | The winit+wgpu+cpal frontend, save/rewind, the egui debugger overlay, the release pipeline. Delivered. |
| `phase-6-v1.0.0-final/` + `phase-6-v1-closeout/` | The v1.0.0 closeout gate plan. **SUPERSEDED** — the accuracy gates it chased were closed (or documented-deferred) by the engine-lineage master-clock work. |
| `phase-7-nesdev-accuracy-hardening/` | Nesdev accuracy-hardening plan. The work it targets is **already accomplished** by the v1.0.0 core (AccuracyCoin 100%). |
| `phase-8-v1.2.0-accuracy-residuals/` | Plan for "v1.2.0 accuracy residuals" + the "v2.0 master-clock refactor". The master clock is **already the only scheduler** in the v1.0.0 core and the accuracy program is complete, so these tracks are retired. |

### Shipped release plans (the v1.1.0 feature release)

These staged the first feature release on the v1.0.0 core. Both have **shipped
in v1.1.0** and are retained for history. Archived 2026-06-15.

| Folder | What shipped |
|---|---|
| `v1.0.1-compat-hygiene/` | The v1.0.1 patch — the game-specific compatibility fixes + the doc/roadmap/test hygiene pass. Shipped. |
| `v1.1.0-features/` | The v1.1.0 feature release: full NES_NTSC composite + CRT/scanline shaders + `.pal` palette filters; NES Power Pad + turbo/autofire + an input-display overlay + a per-game nametable-mirroring override database; debugger breakpoints + a cycle trace logger + an event viewer; an NSF/NSFe player + a 5-band graphic EQ; and the flagship **Lua scripting engine**. This is the authoritative shipped-feature detail for v1.1.0. (Family BASIC keyboard and a Game Genie code database were planned here but **did not ship**.) |

### Legacy tree

| Folder | Why archived |
|---|---|
| `legacy-v0.8-todos/` | The original RustyNES (v0.1.0–v0.8.6) to-do tree, superseded when the emulation core was replaced for v1.0.0. |

### Historical session reports

Dated, point-in-time reports superseded by the synthesis. Archived 2026-06-15.

| File | What it is |
|---|---|
| `TEST-ROM-ACQUISITION-REPORT.md` | Point-in-time report on test-ROM acquisition. |
| `TEST-ROM-WORKFLOW-SUMMARY.md` | Point-in-time summary of the test-ROM workflow. |
| `TODO_AUDIT_SUMMARY_REPORT.md` | Point-in-time audit of the to-do tree. |
| `TODO-GENERATION-STATUS.md` | Point-in-time status of the to-do generation pass. |

The remaining genuinely-open accuracy items (a handful of by-design `#[ignore]`
test probes) are documented in-place at their test sites as **permanent-by-design**
— per project policy they are documented, not ground on. See `docs/STATUS.md` and
`docs/compatibility.md` for the current, authoritative state.

For the live forward roadmap (post-v1.1.0), see [`../ROADMAP.md`](../ROADMAP.md)
and [`../README.md`](../README.md).

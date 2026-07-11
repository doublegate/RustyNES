# ADR 0032 — Vs. `DualSystem` desktop presentation (additive `dual` path, scoped-down advanced features)

- **Status:** Accepted
- **Date:** 2026-07-10
- **Deciders:** DoubleGate
- **Supersedes / relates to:** ADR 0002 (Vs. `DualSystem` core), the v2.1.2 fidelity plan (`to-dos/plans/v2.1.2-fidelity-ntsc-dualsystem-plan.md`, F2.1)

## Context

The Vs. `DualSystem` cabinet emulation is **complete in the core**: `crates/rustynes-core/src/vs_dualsystem.rs` provides `VsDualSystem` (two cross-wired `Nes` consoles) and `Emu::{Single, Dual}` with auto-detection (`Emu::from_rom`), `main_framebuffer()` / `sub_framebuffer()`, `run_frame()`, `set_buttons` (P1/P2 → main, P3/P4 → sub), coin/service routing, and `snapshot()` / `restore()`.

The **desktop frontend**, however, is built entirely around a single `EmuCore.nes: Option<Nes>`: the per-frame produce loop, the input latch, the audio drain, and the present path all assume one 256×240 console. The `nes` handle is read from ~74 scattered sites (debugger panels, save-state, cheats, palette, run-ahead, netplay, TAS). So the cabinet ran in the core and test harness but no user could see the second screen.

We want to present both screens on desktop **without** regressing the single-console path (99.99% of use) and **without** a high-risk rewrite that threads `Emu` through all ~74 sites.

## Decision

1. **Additive `dual` field, not an `Emu` refactor.** `EmuCore` gains `dual: Option<Box<VsDualSystem>>`, mutually exclusive with `nes` (exactly one is `Some` while a ROM is loaded). The single-console produce / latch / present / audio paths are untouched; the dual path is a **parallel branch at each of the few chokepoints** (install, produce, latch, audio, present), so the single path stays byte-identical and the ~74 scattered `nes` read sites simply see `None` in dual mode (they no-op).

2. **Frontend-only, plus one trivial additive core constructor.** The only core addition is `VsDualSystem::from_rom_with_sample_rate` / `Emu::from_rom_with_sample_rate` (the sample rate is baked at construction — there is no runtime setter — so the desktop path needs it for the main console's audio to resample). No behavior change to existing core paths; determinism and AccuracyCoin are untouched.

3. **Compose in the frontend; a dedicated dynamic blit.** `produce_dual_frame` harvests both framebuffers; the present path composes them into a 512×240 (side-by-side) or 256×480 (stacked) image (`[graphics] dual_screen_layout`) and blits it through a new always-on `Gfx::render_dual` / `DynBlit` (the HD-pack blit generalized and un-gated), with an aspect-correct letterbox.

4. **Scope the advanced single-`Nes` features OUT of dual mode.** Run-ahead, rewind, netplay, TAS, and dual save-state all snapshot a single `Nes`; in dual mode they are disabled (they no-op because `nes` is `None`, and the lock-free present fast-path + `needs_nes` debugger/HD branch are gated off). The debugger and HD-pack are unavailable in dual mode. This ships the high-value presentation now instead of a half-working rollback.

5. **Real-cabinet boot stays fixture-limited.** The circulating `DualSystem` dumps are the MAME `maincpu` half only, so a real boot cannot complete (ADR 0002, the 5 `#[ignore]`'d `vs_dualsystem` tests). Verification is via the synth harness (`vs_dualsystem_synth.rs`) + the composition unit tests; those `#[ignore]`'d boots are unchanged.

## Consequences

### Positive

- Both screens, P1–P4 + coin input, and main-console audio work on desktop now.
- The single-console path is provably byte-identical (`visual_regression` 9/9 unchanged; the dual path never touches the deterministic core golden vectors).
- Low blast radius: the change is a set of parallel branches at ~6 chokepoints plus one dynamic-blit present method, not a threading of `Emu` through the whole frontend.

### Negative / deferred

- Run-ahead / rewind / netplay / TAS / dual save-state are unavailable in dual mode; the debugger + HD-pack too. Follow-up tickets: `T-PS-dual-savestate`, `T-PS-dual-runahead`, `T-PS-dual-netplay`.
- Dual mode forgoes the lock-free present fast-path (it needs both framebuffers from the emu lock) — acceptable, the cabinet is rare.
- Desktop only; wasm and the mobile hosts are deferred.
- The `dual` / `nes` mutual-exclusion invariant is a runtime convention, not type-enforced; the load/close paths and the chokepoint branches maintain it.

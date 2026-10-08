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

## Amendment (2026-09-30, v2.9.7 "Tandem"): save states in dual mode

`T-PS-dual-savestate` is done. The core already had the container: the
`VsDualSystem` "RVSD" snapshot, which holds both consoles and the latch that
wires them together, and which restores atomically. What was missing was the
frontend path. `EmuCore::save_state_blob`, `restore_state_blob` and
`loaded_rom_sha256` now dispatch on the single console or the cabinet, and the
desktop's F1/F4 and the Save States grid go through them.

A cabinet's states share the ROM's slot files. The same image always loads the
same way (the `vs_db` flag decides), and the two containers carry different
magic, so offering one kind to the other fails with an error rather than being
misread. The slot grid shows a cabinet slot without a thumbnail, because
`Nes::extract_thumbnail` reads single-console blobs only.

Run-ahead, rewind, netplay and TAS stay out of dual mode (`T-PS-dual-runahead`,
`T-PS-dual-netplay`), and so do the debugger and HD packs. The overclock is not
applied to a cabinet either (v2.9.7).

## Amendment (2026-10-07): rewind and run-ahead in dual mode

The maintainer decided to lift two of Decision 4's exclusions: **rewind and
run-ahead** (decision D27 in
[`v3.1-to-v4.0-line-plan.md`](../../to-dos/plans/v3.1-to-v4.0-line-plan.md),
scheduled for v3.1.0 as `T-PS-dual-runahead`).

The 2026-09-30 amendment supplies the means. The "RVSD" container already
snapshots both consoles and the latch that wires them, and restores them
atomically. Rewind and run-ahead are built on that container rather than on a
single `Nes`.

The gate:

- a rewind across a cabinet frame restores both framebuffers byte-identically;
- run-ahead gives the same output as a run without it.

Netplay and TAS (`T-PS-dual-netplay`), the debugger and HD packs stay out of
dual mode.

**Outcome (v3.1.0).** Implemented as decided. The cabinet owns a rewind ring
of whole RVSD containers, framebuffers included, so a step back restores both
screens without the re-render a single console's slim ring needs.
`VsDualSystem::restore_quiet` is the restore that keeps the ring, for
run-ahead's rollback and a rewind step; `restore` (a loaded state) and
`power_cycle` empty it. Both gate clauses are tests that mutation shows can
fail: `vs_dualsystem_rewind.rs` (a slim sub block in the capture, and a loud
restore in the step back, each caught) and `runahead.rs`
`cabinet_runahead_matches_a_plain_run_on_both_screens` at depths 1 and 2 (no
rollback, and capture left on across hidden frames, each caught). The stimulus
is a cart built for it: the protocol cart renders nothing, so a framebuffer
comparison against it is blind.

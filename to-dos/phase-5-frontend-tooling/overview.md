# Phase 5 — Frontend + Tooling

> **Status (v1.0.0): delivered (and extended by the ported UX shell).** The
> v1.0.0 `rustynes` binary (winit 0.30 + wgpu + cpal + egui 0.29) ships save
> state + rewind + run-ahead, the egui debugger (CPU/PPU/APU/memory/OAM/mapper),
> the NTSC filter, the multi-OS release pipeline, AND the parent emulator's
> ported desktop-UX shell — an always-on menu bar + status bar, a first-run
> Welcome modal, a tabbed Settings window, light/dark/system themes, 8:7
> pixel-aspect correction, fullscreen + 1x-4x window scaling, recent-ROMs MRU,
> save-state slots, and surfaced Cheats/Movies/Netplay/RA/Performance panels.
> This overview is retained as development history — see
> [`ROADMAP.md`](../ROADMAP.md) for current status.

## Goal

Build the user-facing `rustynes` binary: winit + wgpu + cpal + egui. Implement save state, rewind, input bindings, debugger overlays, NTSC filter, and the release pipeline. By the end of this phase the project ships signed binaries on tag.

## Exit criteria

- [ ] Binary builds and runs on Linux, macOS, Windows.
- [ ] Manual smoke test of the compatibility-difficulty corpus (Battletoads, Megaman III, Punch-Out, Castlevania III, Cobra Triangle, Mig 29) passes.
- [ ] Save state save/load works; rewind works.
- [ ] Debugger overlays functional (CPU disasm, PPU viewer, OAM viewer, APU scope).
- [ ] NTSC filter selectable in settings.
- [ ] Release workflow publishes signed binaries on `v*` tags.

## Scope

In-scope:
- Frontend stack (winit, wgpu, cpal, egui, gilrs, rfd, directories).
- Save state format with versioning.
- Rewind ring buffer.
- Input rebinding UI.
- Debugger UI (read-only at v1.0; breakpoints stretch).
- Blargg-style NTSC filter as a wgpu post-pass.
- Release workflow.

Out-of-scope:
- WebAssembly target (defer).
- Mobile.
- Network play.
- TAS movie recording.
- Slang shaders / cgwg CRT.

## Sprints

- [Sprint 1 — Frontend MVP (winit + wgpu + cpal)](sprint-1-frontend-mvp.md)
- [Sprint 2 — Save state + rewind + input bindings](sprint-2-save-rewind.md)
- [Sprint 3 — Debugger overlays + NTSC filter + release pipeline](sprint-3-debugger-release.md)

## Dependencies

Phase 4 complete (full mapper coverage so the binary works for the smoke-test set).

## Risks

- **Risk: cpal sample-rate variance across platforms causes audio underruns.** Detection: smoke test on each OS. Mitigation: configure APU at startup with the device's actual rate; size the ring buffer for ~50 ms of headroom.
- **Risk: wgpu surface configuration mismatches across vendors.** Detection: smoke test on Linux (Vulkan), macOS (Metal), Windows (D3D12). Mitigation: prefer `Fifo` present mode; use sRGB surface format consistently.
- **Risk: save state format breaks in patch releases.** Detection: load-state regression suite. Mitigation: version byte; refuse to load unknown versions with a typed error.

## Reference docs

- [docs/frontend.md](../../docs/frontend.md)
- [docs/architecture.md](../../docs/architecture.md) — public API surface
- [docs/build-and-tooling.md](../../docs/build-and-tooling.md) — release workflow

# Sprint 5-3 — Debugger overlays + NTSC filter + release pipeline

**Phase:** Phase 5 — Frontend + Tooling
**Sprint goal:** egui-based debugger panels (CPU, PPU, OAM, APU, memory, mapper); Blargg-style NTSC filter as a wgpu post-pass; release workflow publishing signed binaries on tag.
**Estimated duration:** 2 weeks

**Status:** debugger + NTSC + rebind modal landed; release workflow + README badges landed; T-53-011 (v0.9.0 tag — release candidate for v1.0.0) pending final smoke test.

## Planned tickets

- [x] T-53-001 — egui-wgpu integration; toggle overlay with `~`.
- [x] T-53-002 — CPU panel: registers, current instruction, scrollable disasm.
- [x] T-53-003 — PPU panel: nametable viewer, pattern table viewer, palette viewer, scroll-cursor overlay.
- [x] T-53-004 — OAM panel: sprite list + visual.
- [x] T-53-005 — APU panel: per-channel waveform scope.
- [x] T-53-006 — Memory hex viewer (CPU bus + PPU bus, go-to-address).
- [x] T-53-007 — Mapper panel: bank registers, IRQ counter state.
- [x] T-53-008 — Blargg-style NTSC filter as a wgsl post-pass (simplified; not bit-exact NES_NTSC).
- [x] T-53-009 — Release workflow (`.github/workflows/release.yml`): builds for Linux/macOS/Windows; uploads to GitHub Releases.
- [x] T-53-010 — README badges for build status, latest release, license.
- [ ] T-53-011 — Tag `v0.9.0` after final smoke test. (Release candidate for v1.0.0; v1.0.0 itself is gated on the coordinated CPU/Bus/PPU IRQ-sample-timing rework — see CHANGELOG `[Unreleased]` → "Investigated and rolled back".) *(pending user approval)*

## Reference docs

- [docs/frontend.md](../../docs/frontend.md) §Debugger panels
- [docs/build-and-tooling.md](../../docs/build-and-tooling.md)

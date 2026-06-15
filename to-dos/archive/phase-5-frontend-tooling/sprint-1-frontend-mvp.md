# Sprint 5-1 — Frontend MVP

**Phase:** Phase 5 — Frontend + Tooling
**Sprint goal:** A `rustynes` binary that opens a window, loads a ROM via drag-and-drop or file dialog, runs the emulator at 60 fps, displays the framebuffer scaled correctly, and outputs audio.
**Estimated duration:** 2 weeks

**Status:** v0 MVP delivered (window + render + audio + keyboard). Drag-and-drop, gamepad (`gilrs`), and file dialog (`rfd`) landed in Track A (v0.9.0 release-candidate prep). Multi-OS smoke test (T-51-009) still pending pre-tag.

## Planned tickets

- [x] T-51-001 — Cargo deps for frontend (winit, wgpu, cpal, egui-wgpu, gilrs, rfd, directories). All in `[workspace.dependencies]`; frontend-only `pollster` and `bytemuck` added (already transitive via wgpu).
- [x] T-51-002 — Window + event loop scaffolding (`crates/rustynes-frontend/src/app.rs`).
- [x] T-51-003 — wgpu surface, framebuffer texture upload, scaled fullscreen-triangle pass (`crates/rustynes-frontend/src/gfx.rs`, inlined WGSL shader).
- [x] T-51-004 — cpal output stream with `Mutex<VecDeque<f32>>` queue between emulator + audio thread (`crates/rustynes-frontend/src/audio.rs`). Lock-free SPSC ring is a Sprint 5-3 optimization per the audio thread note in the sprint brief.
- [x] T-51-005 — Default keyboard input mapping (arrows + Z/X/Enter/RShift). End-to-end controller plumbing (`Controller` struct + `Nes::set_buttons`) implemented in `rustynes-core`. Gamepad via `gilrs` landed in Track A (A5c): hardcoded DPad + South=A + West=B + Start + Select scheme, blended with keyboard via logical OR.
- [x] T-51-006 — File dialog (rfd) + drag-and-drop ROM loading landed in Track A (A5a/A5b). `O` opens an `rfd::FileDialog` with `.nes` filter; `winit::event::WindowEvent::DroppedFile` validates extension and reloads via `Cartridge::parse`.
- [x] T-51-007 — Aspect-ratio-correct viewport via uniform-buffer letterbox transform. PAR (8:7) option deferred.
- [x] T-51-008 — Frame pacing + vsync via wgpu `PresentMode::Fifo` and `request_redraw` continuous loop.
- [ ] T-51-009 — Smoke test on all 3 OSes. Linux build verified; macOS/Windows runtime smoke deferred.

## Reference docs

- [docs/frontend.md](../../docs/frontend.md)

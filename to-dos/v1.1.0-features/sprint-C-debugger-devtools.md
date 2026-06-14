# v1.1.0 · Sprint C — Debugger & devtools  → beta.2

Extends the existing debugger (which already has CPU/PPU/OAM/APU/memory/mapper
viewers). New hooks live behind a `debug-hooks` cargo feature so the default and
wasm/perf builds keep a byte-identical hot path (preserve determinism + perf).

Extension points: `crates/rustynes-core/src/nes.rs` (run loop), the bus/PPU for
event taps, `crates/rustynes-frontend/src/debugger/` (panel registry in `mod.rs`).

## T-110-C1 — Breakpoints / watchpoints

- exec / read / write / PC breakpoints with simple conditions; lightweight
  break-check in the run loop under `debug-hooks`. UI in `debugger/cpu_panel.rs`.
- **Ref:** `ref-proj/Mesen2/.../Debugger/BreakpointManager.h`.
- **Done when:** hitting a breakpoint pauses + surfaces state; zero overhead when
  the feature is off; determinism unaffected (read-only inspection).

## T-110-C2 — Cycle trace logger

- Ring buffer of CPU state + disassembly; export to file. UI panel + format options.
- **Ref:** `ref-proj/Mesen2/.../NesTraceLogger.h`, `ref-proj/fceux`.
- **Done when:** trace can be captured + exported; bounded memory; off by default.

## T-110-C3 — Event viewer

- Timeline of IRQ / NMI / mapper-write / PPU+APU register-write events on a
  scanline×dot grid. Minimal event taps behind `debug-hooks`.
- **Ref:** `ref-proj/Mesen2/.../NesEventManager.h`.
- New panels: `debugger/trace_panel.rs`, `debugger/event_panel.rs` (follow the
  existing panel-registration pattern in `debugger/mod.rs`).

## Verification
- Default build (feature off) is byte-identical → AccuracyCoin/oracle unaffected.
- Unit tests for the break-check + event taps; manual exercise via the overlay.

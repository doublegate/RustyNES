# v1.1.0 · Sprint C — Debugger & devtools  → beta.2

Extends the existing debugger (which already has CPU/PPU/OAM/APU/memory/mapper
viewers). New hooks live behind a `debug-hooks` cargo feature so the default and
wasm/perf builds keep a byte-identical hot path (preserve determinism + perf).

Extension points: `crates/rustynes-core/src/nes.rs` (run loop), the bus/PPU for
event taps, `crates/rustynes-frontend/src/debugger/` (panel registry in `mod.rs`).

## T-110-C1 — Breakpoints / watchpoints  (exec/PC breakpoints DONE)

- exec / read / write / PC breakpoints with simple conditions; lightweight
  break-check in the run loop under `debug-hooks`. UI in `debugger/cpu_panel.rs`.
- **Ref:** `ref-proj/Mesen2/.../Debugger/BreakpointManager.h`.
- **Done when:** hitting a breakpoint pauses + surfaces state; zero overhead when
  the feature is off; determinism unaffected (read-only inspection).
- **DONE (2026-06-14):** the new `debug-hooks` core cargo feature (off by default →
  headless test/bench builds keep a byte-identical hot path) gates exec/PC breakpoints:
  `Nes::{add_breakpoint, remove_breakpoint, clear_breakpoints, breakpoints,
  set_breakpoints_enabled, take_break_hit}` + a break-check at the top of `run_frame`'s
  loop (skips the first iteration so "continue" steps past the current PC). Output-only
  (stops the partial frame + records the PC; no state mutated) so determinism /
  AccuracyCoin hold even with the feature on. Frontend: enables `debug-hooks`, a
  Breakpoints section in `cpu_panel` (armed toggle, hex-add, per-row remove, clear), and
  `produce_one_frame` surfaces the hit via `ProduceFx.breakpoint_hit` → `apply_produce_fx`
  pauses + opens the CPU panel + a status toast. Core test
  `breakpoint_stops_run_frame_at_pc`. Native + both wasm clippy clean; no_std core
  (no feature) unchanged. **Remaining:** read/write **watchpoints** (need bus access
  taps) + conditional breakpoints — a follow-up.

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

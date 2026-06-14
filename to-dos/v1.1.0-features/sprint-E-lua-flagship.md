# v1.1.0 · Sprint E — Lua scripting (FLAGSHIP)  → beta.3 / rc

The headline v1.1.0 feature: a full Mesen2/FCEUX-style Lua scripting API.

## T-110-E1 — `rustynes-script` crate + sandbox

- New crate `crates/rustynes-script` (native-first; feature-gated `scripting`)
  embedding **`mlua`** (Lua 5.4), sandboxed — no `io`/`os`/`require`/`package` by
  default; CPU/instruction budget guard against runaway scripts.

## T-110-E2 — API surface (model on Mesen2 `LuaApi.cpp` + fceux `lua-engine.cpp`)

- **memory:** `read`/`write`/`readRange` over the CPU bus (writes flagged
  non-deterministic).
- **state:** CPU/PPU/APU registers; frame + cycle counters.
- **callbacks:** `onFrame`, `onExec(addr)`, `onRead/onWrite(addr)`, `onNmi/onIrq`.
- **control:** savestate save/load, input override, pause/step.
- **overlay:** drawing API (text/rect/pixel) rendered through the egui pass; `log`.

## T-110-E3 — Core hook points

- Per-frame + optional per-instruction / per-access callbacks in
  `crates/rustynes-core/src/nes.rs`, all behind the `scripting` (and `debug-hooks`)
  feature so the default / wasm / perf builds are byte-identical when off.

## T-110-E4 — Determinism + safety gating

- Script **writes**, `onExec` RAM-poke, and input override are disabled during
  netplay / TAS replay / RA-hardcore — reuse the cheat-gating mechanism.
- Sandbox-escape tests required (no fs/process/network access from a script).

## T-110-E5 — Frontend + docs

- Script console / loader panel in the debugger overlay; `examples/scripts/`
  directory; `docs/scripting.md` API reference; an ADR for the Lua API design.

## Out of scope
- Browser/wasm Lua (follow-up release).

## Verification
- Lua API unit tests + sandbox-escape tests; example scripts run headless.
- Confirm the default build (`scripting` + `debug-hooks` OFF) is byte-identical →
  AccuracyCoin 100% + oracles unaffected.

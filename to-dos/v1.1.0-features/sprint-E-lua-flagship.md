# v1.1.0 · Sprint E — Lua scripting (FLAGSHIP)  → beta.3 / rc

The headline v1.1.0 feature: a full Mesen2/FCEUX-style Lua scripting API.

## T-110-E1 — `rustynes-script` crate + sandbox  ✅ DONE (2026-06-14)

- New crate `crates/rustynes-script` (native-first; feature-gated `scripting`)
  embedding **`mlua`** (Lua 5.4), sandboxed — no `io`/`os`/`require`/`package` by
  default; CPU/instruction budget guard against runaway scripts.
- **DONE:** `crates/rustynes-script` embeds `mlua` (lua54 + vendored). `ScriptEngine::new`
  loads only `table`/`string`/`math`/`coroutine` and nils out `load`/`loadfile`/`dofile`/
  `loadstring`/`collectgarbage`/`require`/`package`/`io`/`os`/`debug`; a per-frame
  VM-instruction hook (`DEFAULT_INSTRUCTION_BUDGET` = 5M) aborts runaways. Pulled in only
  behind the frontend's optional `scripting` feature → default/wasm/no_std builds never
  compile it (byte-identical). Sandbox-escape + budget tests included. See **ADR 0010**.

## T-110-E2 — API surface (model on Mesen2 `LuaApi.cpp` + fceux `lua-engine.cpp`)

- **memory:** `read`/`write`/`readRange` over the CPU bus (writes flagged
  non-deterministic).
- **state:** CPU/PPU/APU registers; frame + cycle counters.
- **callbacks:** `onFrame`, `onExec(addr)`, `onRead/onWrite(addr)`, `onNmi/onIrq`.
- **control:** savestate save/load, input override, pause/step.
- **overlay:** drawing API (text/rect/pixel) rendered through the egui pass; `log`.
- **DONE (E-PR1 + E-PR2):** `emu.read`/`readRange`/`write` (system-RAM writes), `emu.cpu()`
  (A/X/Y/S/P/PC), `emu.frame`/`cycle`, `emu.log`, `emu.onFrame` (E-PR1); **E-PR2:**
  `emu.onExec(addr,fn)` (trace replay), `emu.onRead`/`onWrite(addr,fn)` (a new gated
  `debug-hooks` bus-access log — reads + writes + values, replayed each frame), control
  `emu.pause`/`saveState`/`loadState`/`setInput` and overlay `emu.drawText`/`drawRect`/
  `drawPixel` (collected into host-drained `ControlCmd`/`DrawCmd` queues). **Remaining:** PPU/APU
  state tables (minor) and `onNmi`/`onIrq` (blocked on the non-`const` interrupt tap, see
  T-110-C3). The frontend wiring of control/draw + write-gating is **T-110-E5** (next PR).

## T-110-E3 — Core hook points

- Per-frame + optional per-instruction / per-access callbacks in
  `crates/rustynes-core/src/nes.rs`, all behind the `scripting` (and `debug-hooks`)
  feature so the default / wasm / perf builds are byte-identical when off.

## T-110-E4 — Determinism + safety gating

- Script **writes**, `onExec` RAM-poke, and input override are disabled during
  netplay / TAS replay / RA-hardcore — reuse the cheat-gating mechanism.
- Sandbox-escape tests required (no fs/process/network access from a script).

## T-110-E5 — Frontend + docs  ✅ DONE (2026-06-14)

- Script console / loader panel in the debugger overlay; `examples/scripts/`
  directory; `docs/scripting.md` API reference; an ADR for the Lua API design.
- **DONE:** the `scripting` frontend feature (default-OFF, native-only optional
  `rustynes-script` dep). `debugger/script_panel.rs` — a **Lua Script** console
  (Debug → Lua Script): Load/Reload/Stop a `.lua`, scrolling log, error display,
  callback count. `App::pump_scripts` runs the engine once per redraw under the
  emu lock with the live `Nes`; `App::paint_script_overlay` renders the draw
  commands through the egui pass; control commands apply via the existing
  pause / save-state path. **Write-gating** (T-110-E4) wired: writes off during
  netplay / TAS replay+record / RA-hardcore. Ships `examples/scripts/{hud,
  ram_watch}.lua` + `docs/scripting.md` + ADR 0010 (from E-PR1). Default build
  byte-identical (feature off → no script code compiled). **Follow-ups:**
  `setInput` application through the emu-thread late-latch path, pixel-perfect
  letterbox overlay mapping, `onNmi`/`onIrq`, a wasm Lua build.

## Status

**Workstream E (Lua scripting flagship) is COMPLETE** across E-PR1 (engine +
sandbox, #46), E-PR2 (callbacks + control + overlay API + access log, #47), and
E-PR3 (frontend console + pump + overlay render + gating + examples + docs).

## Out of scope
- Browser/wasm Lua (follow-up release).

## Verification
- Lua API unit tests + sandbox-escape tests; example scripts run headless.
- Confirm the default build (`scripting` + `debug-hooks` OFF) is byte-identical →
  AccuracyCoin 100% + oracles unaffected.

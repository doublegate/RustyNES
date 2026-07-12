# Lua scripting

RustyNES has an optional, sandboxed **Lua 5.4** scripting engine (the v1.1.0
flagship, Workstream E). Scripts can read and write emulator memory, inspect
CPU state, react to per-frame / per-access events, draw an overlay, and drive a
few control actions.

Scripting is **off by default**. The reference engine is **native-only** (it
embeds vendored Lua 5.4 via `mlua`, which needs a C toolchain). Build it in
with:

```bash
cargo run --release -p rustynes-frontend --features scripting -- path/to/rom.nes
```

A build without the feature is byte-identical to a plain build (no Lua, no `cc`
dependency), and the default wasm builds never include it.

## Experimental wasm backend (piccolo)

There is also an **experimental, off-by-default wasm Lua backend** built on
[piccolo](https://crates.io/crates/piccolo) — a pure-Rust Lua VM with no C
dependency, so it compiles to `wasm32-unknown-unknown` where the native
`mlua`/`cc` path cannot. Enable it on the browser build with the `script-wasm`
feature:

```bash
cargo build -p rustynes-frontend --target wasm32-unknown-unknown --features script-wasm
```

Load a script from the browser via the JS bridge:
`window.wasm_bindgen.rustynes_load_script("emu.onFrame(function() ... end)")`
(and `rustynes_stop_script()` to unload). Output goes to the browser console.

The piccolo backend is **explicitly NOT byte-parity** with the native mlua
engine — it is a different VM with a different (incomplete) Lua 5.4
implementation and its own GC + fuel accounting. This is acceptable because
scripts are observational / overlay + *gated* writes and are **never** part of
the framebuffer/audio determinism oracle (AccuracyCoin / nestest / TAS /
netplay). See [ADR 0012](adr/0012-wasm-lua-piccolo-backend.md).

What the piccolo backend supports vs. not:

| Capability | piccolo (wasm) | mlua (native) |
|---|---|---|
| `emu.onFrame` | yes | yes |
| `emu.read` / `peek` / `readRange` | yes (per-frame **snapshot**) | yes (live) |
| `emu.cpu` / `frame` / `cycle` | yes (snapshot) | yes (live) |
| `emu.log` / `print` | yes | yes |
| `emu.drawText` / `drawRect` / `drawPixel` / `drawLine` | yes | yes |
| `memory:peek` / `peek_ppu` / `read_chr` / `read_palette` / `read_oam` / `read_range` | yes (snapshot) *(v2.1.10)* | yes (live) |
| `memory:poke` | yes — gated + **deferred** *(v2.1.10)* | yes — gated, live |
| `emu.write` | yes — gated + **deferred** (applied after the frame's callbacks; the snapshot is updated so a same-frame read sees it) | yes — gated, live |
| `emu.pause` / `saveState` / `loadState` | queued | applied |
| `emu.setInput` | queued + gated (not yet applied on wasm) | applied |
| `emu.onExec` / `onRead` / `onWrite` | **no-op** (native-only) | yes |
| `emu.onNmi` / `onIrq` | **no-op** (native-only) | yes |
| `emu.addEventCallback` (`reset` / `spriteZeroHit` / `codeBreak` / `startFrame` / …) | **no-op** (registers cleanly, never fires; native-only) | yes |

As of **v2.1.10 "Creator Tools"** the piccolo backend gained the read-parity
`memory` table (CPU / PPU / palette / CHR / OAM reads served from an extended
per-frame snapshot; `poke` keeps the same gated + deferred contract as
`emu.write`) and the `emu.drawLine` primitive, closing most of the read + HUD
gap. The remaining native-only carve-out is the per-access / per-interrupt
replay callbacks and the host-fired lifecycle events (`addEventCallback`), which
need the native event dispatch — they register as no-ops on wasm so a portable
script does not error (ADR 0012).

The runaway-loop guard is shared in spirit: piccolo's `Fuel` is fed the same
per-frame instruction budget (`DEFAULT_INSTRUCTION_BUDGET`, 1,000,000), and
exhaustion surfaces as a `ScriptError::Budget`.

## Loading a script

Open the console: **Debug → Lua Script** (or the toolbar "Lua" checkbox in the
debugger). Click **Load .lua…**, pick a file, and it runs immediately. The
console shows `print` / `emu.log` output, the loaded path, the `onFrame`
callback count, and any load / runtime error. **Reload** re-reads the file;
**Stop** unloads it.

Five examples live in [`examples/scripts/`](../examples/scripts): `hud.lua`
(an on-screen frame/PC HUD), `ram_watch.lua` (a write tracer + RAM dump), and
the v1.5.0 dev/TAS set — `memory_scanner.lua` (a Cheat-Engine-style RAM
next-scan), `tas_frame_analysis.lua` (rolling in-memory checkpoints +
`pause_at_frame` + per-frame deltas), and `game_state_tracker.lua` (a
symbol-aware HUD that resolves watched fields by name).

## The `emu` API

### Memory

| Call | Description |
|---|---|
| `emu.read(addr)` | Read one byte from the CPU bus (`$0000-$FFFF`), side-effect-free. |
| `emu.readRange(addr, len)` | Read `len` bytes starting at `addr`; returns a 1-based array. |
| `emu.write(addr, value)` | Write a byte into **system RAM** (`$0000-$1FFF`). See *Determinism* below. |

### State

| Call | Description |
|---|---|
| `emu.cpu()` | Table `{ a, x, y, s, p, pc }` — the current CPU register file. |
| `emu.frame` | The current frame number (a value, refreshed each pump). |
| `emu.cycle` | The cumulative CPU cycle counter. |

### Callbacks

| Call | When it fires |
|---|---|
| `emu.onFrame(fn)` | Once per emulated frame. |
| `emu.onExec(addr, fn)` | After a frame, for each time the CPU executed an instruction at `addr` (`fn(addr)`). |
| `emu.onRead(addr, fn)` | After a frame, for each CPU read of `addr` (`fn(addr, value)`). |
| `emu.onWrite(addr, fn)` | After a frame, for each CPU write to `addr` (`fn(addr, value)`). |
| `emu.onNmi(fn)` | After a frame, once per NMI the CPU serviced that frame (`fn(vector)`, `vector == 0xFFFA`). |
| `emu.onIrq(fn)` | After a frame, once per IRQ / BRK the CPU serviced that frame (`fn(vector)`, `vector == 0xFFFE`). |

`onExec` / `onRead` / `onWrite` / `onNmi` / `onIrq` are **observational** and are
dispatched by replaying the frame's trace / bus-access / interrupt-service logs
*after* the frame completes — they report what happened, they do not intercept
execution mid-instruction (a deliberate limitation that keeps the cycle-accurate
core `#![no_std]` and the determinism contract intact; see ADR 0010).

`onNmi` / `onIrq` tap the CPU's **committed** interrupt-service commit point
(`Bus::notify_irq_service`, the same point the IRQ trace records) — the cycle the
CPU fetches its service vector — *not* the speculative `poll_nmi` / `poll_irq`
sampler that ADR 0010 flagged as unreliable. So a callback sees exactly the
interrupts the CPU actually serviced, in service order, classified by the vector
that was fetched (`0xFFFA` ⇒ NMI, `0xFFFE` ⇒ IRQ/BRK — robust even when an NMI
hijacks an in-progress IRQ/BRK sequence).

### Control

| Call | Effect |
|---|---|
| `emu.pause()` | Pause emulation. |
| `emu.saveState(slot)` | Save to numbered slot. |
| `emu.loadState(slot)` | Load from numbered slot (ignored under RA-hardcore). |
| `emu.setInput(port, buttons)` | Override port `port`'s (0 = P1, 1 = P2) controller buttons for the next frame (`buttons` is the standard NES bitmask: bit 0 = A, 1 = B, 2 = Select, 3 = Start, 4-7 = Up/Down/Left/Right). Merged at the deterministic late-latch; **gated identically to `emu.write`** (no-op under netplay / TAS replay / RA-hardcore). |

### Overlay + logging

| Call | Effect |
|---|---|
| `emu.drawText(x, y, text [, color])` | Draw text (NES px coords; `color` is `0xRRGGBBAA`, default white). |
| `emu.drawRect(x, y, w, h [, color])` | Draw a filled rectangle. |
| `emu.drawPixel(x, y [, color])` | Draw a single pixel. |
| `emu.drawLine(x1, y1, x2, y2 [, color])` *(v2.1.10)* | Draw a straight line segment — the fourth HUD primitive, ideal for graphs / plots / hitbox overlays. Full mlua + piccolo parity. |
| `emu.log(...)` | Append to the console. `print(...)` is redirected here too. |

All four draw primitives are pure overlay: they decorate the presented frame and
are **never** write-gated (drawing cannot perturb deterministic state).

Overlay coordinates are NES-framebuffer space (256×240), mapped onto the actual
letterboxed game rect — honouring 8:7 pixel-aspect correction and the overscan
crop — so HUD coordinates line up with game pixels.

### Driving the emulator (v1.6.0 Workstream B2)

The callbacks above are *reactive* — the host drives the emulator and calls your
`onFrame` once per frame. The **driving** primitives let a single script drive
the emulator a frame at a time instead, the FCEUX / BizHawk model that bots and
TAS scripts want: linear "do this, advance one frame, then do that" logic.

| Call | Effect |
|---|---|
| `emu.run(fn)` | Register `fn` as the **driving coroutine**. It runs until it next calls `emu.frameadvance()`. Only one driver is active at a time (a later `emu.run` replaces it). |
| `emu.frameadvance()` | Yield control back to the emulator. The host advances **exactly one frame**, then resumes the coroutine where it left off, so each `emu.frameadvance()` corresponds to one emulated frame. |

```lua
emu.run(function()
  while true do
    emu.setInput(0, 0x01)  -- hold A
    emu.frameadvance()     -- one frame elapses, then we resume here
  end
end)
```

`emu.frameadvance()` is a thin alias of Lua's `coroutine.yield()`, so it only
works inside the coroutine `emu.run` creates; calling it elsewhere raises an
error (surfaced to the script console, never a host crash). A driver that
returns simply stops being resumed.

Driving is **determinism-safe**: a driver issues the same `emu.write` /
`emu.setInput` / `load_state` effects as any callback, and those are gated
identically to `emu.write` (a silent no-op under netplay / TAS replay /
RA-hardcore). Driving is **native-only** (the mlua backend), the same carve-out
as the dev/TAS API below. See `examples/scripts/driving_loop.lua`.

## The dev / TAS API (v1.5.0 Workstream B)

A deeper automation surface for memory inspection, cart introspection,
in-script checkpointing, and symbol-aware debugging. These tables and methods
are **native-only** (the mlua backend) — the same documented carve-out as
`onExec` / `onNmi` (the experimental piccolo/wasm backend keeps the v1.2.0
subset; see [ADR 0012](adr/0012-wasm-lua-piccolo-backend.md)). All
state-mutating calls are **gated identically to `emu.write`** (a silent no-op
under netplay / TAS replay / RA-hardcore), so they cannot perturb a
deterministic / locked session.

> These new tables use **colon-call** syntax (`memory:peek(addr)`,
> `cart:mapper_id()`, `emu:save_state(1)`, `sym:addr("main")`). The original
> `emu.read` / `emu.write` / `emu.saveState` (dot form) are unchanged.

### `memory` — explicit CPU + PPU memory access (B1)

| Call | Description |
|---|---|
| `memory:peek(addr)` | One CPU-bus byte (`$0000-$FFFF`), side-effect-free (`$2002` does not clear VBL; `$2007` does not advance the read buffer). |
| `memory:read_range(addr, len)` | `len` CPU bytes from `addr` (wrapping), 1-based array. `len` ≤ 65536. |
| `memory:peek_ppu(addr)` | One PPU-bus byte (`$0000-$3FFF`: CHR, nametables, palette), side-effect-free. |
| `memory:read_range_ppu(addr, len)` | `len` PPU bytes from `addr` (wrapping the 14-bit PPU space). `len` ≤ 16384. |
| `memory:read_u16_le(addr)` *(v1.6.0)* | A 16-bit **little-endian** word — two CPU `peek`s (`addr`, `addr+1`), side-effect-free. The common need for positions / timers / pointers. |
| `memory:read_u16_be(addr)` *(v1.6.0)* | A 16-bit **big-endian** word from `addr`. |
| `memory:read_oam(index)` *(v1.6.0)* | One byte of sprite RAM (**OAM**) — the third read domain alongside CPU and PPU; `index` wraps to `0-255`. |
| `memory:read_palette(index)` *(v2.1.10)* | One palette-RAM entry (`$3F00-$3F1F`, `index` masked to `0-31`), returning the raw 6-bit NES colour index (`0-63`). Side-effect-free. |
| `memory:read_chr(addr)` *(v2.1.10)* | One CHR / pattern-table byte (`$0000-$1FFF`, `addr` masked to 13 bits), resolved through the mapper's current CHR banking exactly as the PPU fetches it. Side-effect-free. |
| `memory:poke(addr, value)` | Write a byte into **system RAM** (`$0000-$1FFF`). Gated like `emu.write`. |
| `memory:write_range(addr, bytes)` | Write a 1-based byte array starting at `addr` into system RAM. Gated like `emu.write`. |

> **Side-effect-free reads (the `*Debug` contract).** Every `memory:*` read
> above uses the emulator's debug-peek path, so observing memory never trips
> open-bus, advances the `$2007` read buffer, clears the `$2002` VBL latch, or
> fires a mapper side-effect. On this observational engine the standard reads
> ARE the side-effect-free / `*Debug` variant — there is no separate
> latch-consuming read to guard against.

### `joypad` — controller input (B3)

| Call | Description |
|---|---|
| `joypad:get(port)` *(v1.6.0)* | The latched standard-controller bitmask for `port` (`0` = P1, `1` = P2, `2`/`3` = Four Score), in `Buttons` bit order (`A`=bit 0 .. `Right`=bit 7). Read-only and side-effect-free (reads the latch, not the shift register). |
| `joypad:set(port, buttons)` *(v1.6.0)* | Override a controller's button bitmask for the frame — identical to `emu.setInput(port, buttons)`, so like that call it applies to the **standard ports `0` (P1) / `1` (P2)** the host latches; ports `≥ 2` are accepted but not applied (the frontend feeds only P1/P2). Gated like `emu.write`: a silent no-op under a locked / replayed session (netplay / TAS replay / RA-hardcore). |

### `cart` — read-only cart / system queries (B2)

| Call | Description |
|---|---|
| `cart:mapper_id()` | The loaded iNES / NES 2.0 mapper id. |
| `cart:prg_size()` | PRG-ROM size in bytes. |
| `cart:chr_size()` | CHR-ROM size in bytes (0 for CHR-RAM boards). |
| `cart:sha256()` | Lowercase-hex SHA-256 of the ROM bytes (64 chars). |
| `cart:region()` | `"NTSC"`, `"PAL"`, or `"Dendy"`. |
| `cart.frame` | The current frame number (mirrors `emu.frame`). |

### In-memory save-state slots (B3)

| Call | Effect |
|---|---|
| `emu:save_state(slot)` | Snapshot the full emulator state into in-memory script slot `slot` (`0-255`). Read-only — always allowed. |
| `emu:load_state(slot)` | Restore from script slot `slot`. Returns `true` on success, `false` for an empty slot or a locked session. **Gated like `emu.write`.** |

These slots are **distinct** from the host's on-disk numbered slots
(`emu.saveState` / `emu.loadState`): they live in the script engine for the
session and are never persisted, so a TAS / analysis script can checkpoint and
roll back without touching the user's save files.

### Debug hooks for scripts (B4)

| Call | Effect |
|---|---|
| `emu:on_breakpoint(addr, fn)` | Register `fn(pc)` to fire each time the CPU executed an instruction at `addr` that frame. Observational — replayed from the per-frame exec-PC log (like `onExec`), never a mid-instruction intercept; arms the exec log. |
| `emu:pause_at_frame(n)` | Queue a one-shot pause that fires when the emulated frame count reaches `n`. |
| `sym:addr(name)` | The CPU address for label `name`, or `nil`. |
| `sym:name(addr)` | The label at `addr`, or `nil`. |

The `sym` table resolves against the debugger's loaded symbol-file labels
(`.sym` / Mesen `.mlb` / FCEUX `.nl`, the v1.4.0 Workstream D loader). The host
pushes the current symbol map into the engine when a script loads and on every
symbol load / clear, so `sym:` tracks whatever is loaded. With no symbol file
loaded, both queries return `nil`.

## Scriptable TAStudio + full Lua parity (v1.7.0 "Forge" Workstream B)

v1.6.0 built the `TAStudio` piano-roll *editor*; v1.7.0 makes it
**programmable** (bots, generated TASes, analysis canvases) and rounds out the
Mesen2 parity surface. All native-only (the mlua backend), behind `scripting`;
the experimental piccolo wasm backend hosts none of it (the same carve-out as
the dev/TAS surface above).

### `tastudio` — control the piano-roll editor (B1)

Colon-call form (`tastudio:engaged()`). **Queries** read a snapshot of the live
editor the host pushes each frame; **mutators** queue an action the host applies
to the editor (and are **gated identically to `emu.write`** — silent no-ops
under netplay / TAS replay / RA-hardcore). When no `TAStudio` session is open,
`engaged()` is `false` and every query returns its empty / `nil` form.

| Call | Effect |
|---|---|
| `tastudio:engaged()` | `true` while the editor is open. |
| `tastudio:getrecording()` | The editor's recording mode. |
| `tastudio:getseekframe()` | The current cursor / seek frame. |
| `tastudio:getselection()` | The selected `(first, last)` frame range, or `(nil, nil)`. |
| `tastudio:islag(frame)` | `true`/`false` lag verdict, or `nil` if `frame` is not yet emulated. |
| `tastudio:hasstate(frame)` | `true` if a greenzone save-state exists at `frame`. |
| `tastudio:getmarker(frame)` | The marker label at `frame`, or `nil`. |
| `tastudio:getbranches()` | An array of `{ frame=, text= }` per saved branch. |
| `tastudio:getbranchtext(index)` | A branch's annotation text (1-based), or `nil`. |
| `tastudio:getbranchinput(index, frame)` | The branch's `(p1, p2)` button bitmasks at `frame`, or `(nil, nil)`. |
| `tastudio:setrecording(bool)` / `:togglerecording()` | Set / toggle recording mode. **Gated.** |
| `tastudio:setplayback(frame \| markerName)` | Seek the cursor to a frame or a named marker. **Gated.** |
| `tastudio:setlag(frame, bool)` | Override a frame's lag verdict. **Gated.** |
| `tastudio:setmarker(frame, text)` / `:removemarker(frame)` | Set/rename or clear a marker. **Gated.** |
| `tastudio:submitinputchange(frame, port, buttons)` | **Stage** one input edit (does not apply yet). **Gated.** |
| `tastudio:applyinputchanges()` | Flush the staged edits as one atomic batch (the host re-seeks at most once). **Gated.** |
| `tastudio:loadbranch(index)` | Restore a saved branch. **Gated.** |
| `tastudio:setbranchtext(index, text)` | Set a branch's annotation. **Gated.** |

`submitinputchange` + `applyinputchanges` are the BizHawk atomic-edit pattern:
stage any number of per-frame edits, then apply them all in one shot so the
editor re-derives state once. (`setrecording` / `setlag` / `setbranchtext` are
accepted but the v1.6.0 editor model does not yet have a target for them, so
they are documented host stubs.)

### `tastudio` analysis-canvas callbacks (B2)

Annotate the piano-roll grid programmatically. The cell-query callbacks are
**pure overlay** — they return a colour / text / icon the host paints, and can
never mutate state. The event callbacks are observational.

| Call | Effect |
|---|---|
| `tastudio:onqueryitembg(fn)` | `fn(frame, column)` returns a `0xRRGGBBAA` cell background, or `nil`. |
| `tastudio:onqueryitemtext(fn)` | `fn(frame, column)` returns replacement cell text, or `nil`. |
| `tastudio:onqueryitemicon(fn)` | `fn(frame, column)` returns an icon key, or `nil`. |
| `tastudio:clearIconCache()` | Ask the host to drop its cached cell icons. |
| `tastudio:ongreenzoneinvalidated(fn)` | `fn(firstFrame)` fires when an edit invalidates the greenzone. |
| `tastudio:onbranchload(fn)` | `fn(index)` fires when a branch loads. |

### Full Lua parity (B3, Mesen2)

| Call | Effect |
|---|---|
| `emu.getScreenBuffer()` | The 256×240 frame as a flat array (1-based) of `0xRRGGBBAA` pixels. Read-only. |
| `emu.getPixel(x, y)` | One `0xRRGGBBAA` pixel, or `nil` if out of the 256×240 frame. |
| `emu:setScreenBuffer(t)` | Paint the **display** framebuffer from such an array (output only — never a register/latch; a later real frame fully repaints). **Gated** like `emu.write`. |
| `emu:getState()` | A structured map: CPU `a`/`x`/`y`/`s`/`p`/`pc` + `frameCount` / `cycle` / `region`. Read-only. |
| `emu:setState(t)` | Write back the CPU register file from such a map (a partial table leaves the rest untouched). **Gated** like `emu.write`. |
| `emu.addEventCallback(fn, type)` | Register `fn` for an event. Engine-fired: `startFrame`, `endFrame`, `inputPolled`, `nmi`, `irq`, `stateLoaded`, `stateSaved`. Host-fired *(v2.1.10)*: `reset` (soft-reset / power-cycle), `spriteZeroHit` (`fn(frame)`, once per frame the PPU sprite-0 hit flag was set — sampled non-destructively), `codeBreak` (`fn(pc)`, on a debugger breakpoint). All observational (no live `Nes`). An unknown type errors at load. |
| `emu.addMemoryCallback(fn, "write", start[, end])` | A **value-modifying** write watch over `[start, end]`: `fn(addr, value)` may RETURN a replacement byte, which is poked back through the gated `poke_ram` path (a scriptable cheat / watchpoint). **Gated** like `emu.write`. |
| `emu.takeScreenshot()` | Write the current frame to a PNG (the host owns the encoder + screenshot dir). A read-only side effect — *not* gated. |
| `emu.getScriptDataFolder()` | A per-script sandboxed data directory (the clean persist-without-arbitrary-FS path), or `nil`. |

The value-modifying memory callback rides the same post-frame access-log replay
as the observational `onWrite`, so it never intercepts mid-instruction; the poke
of the replacement byte is the mutation, gated exactly like `emu.write` (dropped
under a locked / replayed session). `startFrame` / `endFrame` / `inputPolled`
fire from the per-frame pump; `stateLoaded` / `stateSaved` fire from the
in-memory `emu:load_state` / `save_state` slots.

## Host IPC / automation (v1.7.0 "Forge" Workstream E)

The power-user tier (modelled on BizHawk's `comm` / `client` / `userdata`
libraries) that turns RustyNES into a platform for external bots / RL agents /
randomizers / stream tools. The defining property: **a script never gets a raw
socket or any OS handle** — the host owns every connection and marshals plain
values across the boundary, so the sandbox guarantee below is preserved.

### `comm` — host-mediated IPC (E1, `script-ipc` only)

Enabled by the off-by-default `script-ipc` feature
(`cargo build -p rustynes-frontend --features scripting,script-ipc`). The
**host** (`rustynes-frontend::script_host::ScriptHost`) owns the TCP / HTTP /
WebSocket / memory-mapped-file connection and does the I/O off the emulator lock
on a dedicated worker thread; the script only queues a request and polls the
result. See ADR 0016.

| Call | Effect |
|---|---|
| `comm.socketServerSend(data)` | Send `data` over the host's configured outbound TCP socket (`RUSTYNES_COMM_TCP` endpoint). Fire-and-forget. |
| `comm.httpGet(url)` → `id` | Issue an HTTP GET; returns a correlation `id`. |
| `comm.httpPost(url, body)` → `id` | Issue an HTTP POST. |
| `comm.ws_open(url)` → `id` | Open a WebSocket (host-owned). |
| `comm.ws_send(text)` | Send a text frame. |
| `comm.ws_close()` | Close the WebSocket. |
| `comm.mmfWrite(name, data)` | Write `data` to the host's named memory-mapped-file buffer. |
| `comm.mmfRead(name, len)` → `id` | Read up to `len` bytes from a named MMF. |
| `comm.receive()` → table or `nil` | Pop the oldest host-fulfilled result. `{kind="http", id, status, body}`, `{kind="ws", id, open, message}`, or `{kind="mmf", id, data}`. |

`comm.*` is a **new non-deterministic source**, so every verb is gated EXACTLY
like `emu.write`: under netplay / TAS replay or record / RA-hardcore the verb is
dropped at the source (the async ones return `id = 0`), no `CommCmd` is queued,
and the host opens no connection. The core synthesis never sees a `CommCmd`.

### `client` — host automation (E2)

Ships with the base `scripting` surface (no feature gate). Collected and applied
by the host after the frame.

| Call | Effect |
|---|---|
| `client.opentool(name)` | Open a debugger panel (`cpu`/`ppu`/`oam`/`apu`/`memory`/`mapper`/`trace`/`watch`/`events`/`script`). |
| `client.screenshot()` | Capture the framebuffer to a file. |
| `client.screenshottoclipboard()` | Capture to the system clipboard. |
| `client.setwindowsize(scale)` | Set the integer window scale. |
| `client.speedmode(pct)` | Set emulation speed (`100` = realtime). Presentation-only. |
| `client.frameskip(n)` | Request a render frame-skip (recorded; no skip pipeline today). |
| `client.reboot_core()` | Power-cycle the running ROM. **Gated like `emu.write`.** |
| `client.pause_av()` / `client.unpause_av()` | A/V-recorder pause intent (recorder is start/stop only today). |
| `client.addcheat(code)` / `client.removecheat(code)` | Add/remove a Game Genie code. **Gated like `emu.write`.** |

The observational verbs (screenshot, window size, speed, …) are
presentation-only and never perturb the deterministic core; the state-changing
verbs (`reboot_core`, cheats) are dropped under a locked session.

### `userdata` — persisted KV store (E3)

A per-script string→string store the host persists across runs (and may carry
into save-states). Script-local host memory, never emulator state, so it is not
write-gated.

| Call | Effect |
|---|---|
| `userdata.set(key, value)` | Store a string value. |
| `userdata.get(key)` → string or `nil` | Read a value. |
| `userdata.containskey(key)` → bool | Membership test. |
| `userdata.remove(key)` → bool | Remove a key (returns whether it existed). |
| `userdata.keys()` → table | All keys, sorted (deterministic order). |

## Determinism + safety

- **Sandbox.** Only the `table` / `string` / `math` / `coroutine` standard
  libraries are available. `io`, `os`, `package`, `require`, `debug`, and the
  unsafe base loaders (`load`, `loadfile`, `dofile`, `loadstring`,
  `collectgarbage`) are removed — a script cannot touch the filesystem, the
  process, or the network.
- **Budget.** A runaway script (e.g. an infinite loop in a callback) is aborted
  by a per-frame VM-instruction budget.
- **Write gating.** `emu.write` *and* `emu.setInput` mutate state / input, so
  both are **disabled** during netplay, TAS-movie replay/record, and
  RetroAchievements hardcore mode — the same policy as the Game Genie / raw-RAM
  cheat path. The gate is enforced twice: the engine drops the command at the
  source (it never queues), and the host re-checks the identical condition
  (`netplay_locked || movie_locked`, which folds in RA-hardcore) at the
  late-latch — so a locked / replayed session is provably unperturbed. Reads and
  the overlay are always allowed. The v1.5.0 dev/TAS mutators ride the **same**
  `set_writes_locked` gate: `memory:poke`, `memory:write_range`, and
  `emu:load_state` are silent no-ops under a locked session (an in-script
  `emu:save_state` is a read-only snapshot and is always allowed; `emu:load_state`
  returns `false` rather than mutating). The `memory:peek*` / `read_range*`,
  `cart:*`, and `sym:*` queries are pure reads, so they always run. The v1.7.0
  Workstream-B mutators ride the **same** gate: every `tastudio:*` editor mutator,
  `emu:setScreenBuffer`, `emu:setState`, and the value-modify poke of an
  `emu.addMemoryCallback` write watch are all dropped at the source under a locked
  session (the `tastudio:*` queries, `emu.getScreenBuffer`/`getPixel`/`getState`,
  and `emu.takeScreenshot` are reads / read-only side effects, so they always run).
  The v1.7.0 host-IPC / automation surface rides the **same** gate: every `comm.*`
  verb (E1, `script-ipc`) and the state-changing `client.*` verbs (`reboot_core` /
  `addcheat` / `removecheat`, E2) are dropped at the source under a locked
  session, so a script can neither open a connection nor perturb the run while
  netplay / replay / hardcore is active. The `userdata.*` KV store (E3) is
  script-local host memory and is never gated.
- **Host-mediated IPC (no raw sockets).** With `script-ipc` on, the `comm`
  table still does **not** expose a socket / file handle / OS object — it only
  queues marshalled requests, and the host (`script_host`) owns the connection
  and does the I/O off the emulator lock. The sandbox stdlib set is unchanged,
  so a script with IPC enabled still cannot reach `io` / `os` / `package` / the
  raw network. See ADR 0016.
- **`emu.setInput` late-latch.** When unlocked, a `setInput(port, buttons)` is
  applied at the *same* deterministic point a real keypress enters — the
  per-frame controller latch, just before the frame runs — so a session that
  records or replays this exact input stream stays bit-identical. The override is
  one-shot per call (it does not stick across frames); a script that wants a
  button held re-issues it from `onFrame`.
- **Pacing.** The engine runs on the UI thread (Lua is not thread-safe to share
  with the emulation thread), so callbacks fire at display rate; the
  exec/read/write logs reflect the most recent emulated frame. Callbacks execute
  while the host holds the emulator lock (they need live state), so a heavy
  script costs frame time — the per-frame instruction budget (default 1M, ~10 ms)
  bounds a runaway. Keep per-frame work light.
- **Registry safety.** Registered callbacks are stored **Rust-side** (as Lua
  registry keys), not in a script-visible global. A script cannot inspect,
  clobber, or inject junk into the callback registry, so it can never corrupt
  the host pump — the protection is structural, not best-effort.
- **Overlay coordinates** are mapped onto the actual letterboxed game rect
  (honouring 8:7 pixel-aspect correction + overscan crop), so HUD coordinates
  line up with game pixels.

## Example script library

Well-commented example scripts live in `examples/scripts/`. Load one from
Debug → Lua Script → Load .lua…. Every bundled example is compile-time embedded
and exercised by a `rustynes-script` test (`bundled_example_scripts_load_and_run`),
so a doc-referenced example never bit-rots against the API.

| Script | Demonstrates |
|---|---|
| `ram_watch.lua` | Watch + log RAM values each frame. |
| `hud.lua` | A minimal on-screen HUD (`drawText` / `drawRect`). |
| `hud_graph.lua` *(v2.1.10)* | A scrolling value graph drawn with `emu.drawLine`. |
| `palette_viewer.lua` *(v2.1.10)* | An on-screen palette + CHR inspector (`memory:read_palette` / `read_chr`). |
| `lifecycle_events.lua` *(v2.1.10)* | Every `emu.addEventCallback` lifecycle event (`reset` / `spriteZeroHit` / `codeBreak` / …). |
| `memory_scanner.lua` | A simple changed-value memory scanner. |
| `game_state_tracker.lua` | Track structured game state across frames. |
| `tas_frame_analysis.lua` | Per-frame TAS analysis via the `tastudio` query API. |
| `driving_loop.lua` | Drive the emulator a frame at a time (`emu.run` / `frameadvance`). |

## See also

- `docs/adr/0010-lua-scripting-engine.md` — the architecture decision.
- `docs/adr/0016-host-mediated-script-ipc.md` — the host-mediated IPC security
  posture (the host owns the socket; the sandbox never does).
- `crates/rustynes-script/` — the engine crate.
- `crates/rustynes-frontend/src/script_host.rs` — the host-mediated IPC bridge.

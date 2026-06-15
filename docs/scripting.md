# Lua scripting

RustyNES has an optional, sandboxed **Lua 5.4** scripting engine (the v1.1.0
flagship, Workstream E). Scripts can read and write emulator memory, inspect
CPU state, react to per-frame / per-access events, draw an overlay, and drive a
few control actions.

Scripting is **off by default** and **native-only**. Build it in with:

```bash
cargo run --release -p rustynes-frontend --features scripting -- path/to/rom.nes
```

A build without the feature is byte-identical to a plain build (no Lua, no `cc`
dependency), and the wasm builds never include it.

## Loading a script

Open the console: **Debug → Lua Script** (or the toolbar "Lua" checkbox in the
debugger). Click **Load .lua…**, pick a file, and it runs immediately. The
console shows `print` / `emu.log` output, the loaded path, the `onFrame`
callback count, and any load / runtime error. **Reload** re-reads the file;
**Stop** unloads it.

Two examples live in [`examples/scripts/`](../examples/scripts): `hud.lua`
(an on-screen frame/PC HUD) and `ram_watch.lua` (a write tracer + RAM dump).

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

`onExec` / `onRead` / `onWrite` are **observational** and are dispatched by
replaying the frame's trace / bus-access logs *after* the frame completes — they
report what happened, they do not intercept execution mid-instruction (a
deliberate limitation that keeps the cycle-accurate core `#![no_std]` and the
determinism contract intact; see ADR 0010). `onNmi` / `onIrq` are a follow-up
(blocked on the non-`const` interrupt sampler).

### Control

| Call | Effect |
|---|---|
| `emu.pause()` | Pause emulation. |
| `emu.saveState(slot)` | Save to numbered slot. |
| `emu.loadState(slot)` | Load from numbered slot (ignored under RA-hardcore). |
| `emu.setInput(port, buttons)` | *Accepted but not yet applied* — input override through the emu-thread late-latch path is a follow-up. |

### Overlay + logging

| Call | Effect |
|---|---|
| `emu.drawText(x, y, text [, color])` | Draw text (NES px coords; `color` is `0xRRGGBBAA`, default white). |
| `emu.drawRect(x, y, w, h [, color])` | Draw a filled rectangle. |
| `emu.drawPixel(x, y [, color])` | Draw a single pixel. |
| `emu.log(...)` | Append to the console. `print(...)` is redirected here too. |

Overlay coordinates are NES-framebuffer space (256×240), mapped onto the actual
letterboxed game rect — honouring 8:7 pixel-aspect correction and the overscan
crop — so HUD coordinates line up with game pixels.

## Determinism + safety

- **Sandbox.** Only the `table` / `string` / `math` / `coroutine` standard
  libraries are available. `io`, `os`, `package`, `require`, `debug`, and the
  unsafe base loaders (`load`, `loadfile`, `dofile`, `loadstring`,
  `collectgarbage`) are removed — a script cannot touch the filesystem, the
  process, or the network.
- **Budget.** A runaway script (e.g. an infinite loop in a callback) is aborted
  by a per-frame VM-instruction budget.
- **Write gating.** `emu.write` mutates state, so it is **disabled** during
  netplay, TAS-movie replay/record, and RetroAchievements hardcore mode — the
  same policy as the Game Genie / raw-RAM cheat path. Reads and the overlay are
  always allowed.
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
- **`emu.setInput`** is accepted but **not yet applied** (input override through
  the emulation thread's late-latch path is a follow-up); calling it logs a
  one-time notice to the console.

## See also

- `docs/adr/0010-lua-scripting-engine.md` — the architecture decision.
- `crates/rustynes-script/` — the engine crate.

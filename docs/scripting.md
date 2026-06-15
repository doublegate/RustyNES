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
| `emu.drawText` / `drawRect` / `drawPixel` | yes | yes |
| `emu.write` | yes — gated + **deferred** (applied after the frame's callbacks; the snapshot is updated so a same-frame read sees it) | yes — gated, live |
| `emu.pause` / `saveState` / `loadState` | queued | applied |
| `emu.setInput` | queued + gated (not yet applied on wasm) | applied |
| `emu.onExec` / `onRead` / `onWrite` | **no-op** (native-only) | yes |
| `emu.onNmi` / `onIrq` | **no-op** (native-only) | yes |

The runaway-loop guard is shared in spirit: piccolo's `Fuel` is fed the same
per-frame instruction budget (`DEFAULT_INSTRUCTION_BUDGET`, 1,000,000), and
exhaustion surfaces as a `ScriptError::Budget`.

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
- **Write gating.** `emu.write` *and* `emu.setInput` mutate state / input, so
  both are **disabled** during netplay, TAS-movie replay/record, and
  RetroAchievements hardcore mode — the same policy as the Game Genie / raw-RAM
  cheat path. The gate is enforced twice: the engine drops the command at the
  source (it never queues), and the host re-checks the identical condition
  (`netplay_locked || movie_locked`, which folds in RA-hardcore) at the
  late-latch — so a locked / replayed session is provably unperturbed. Reads and
  the overlay are always allowed.
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

## See also

- `docs/adr/0010-lua-scripting-engine.md` — the architecture decision.
- `crates/rustynes-script/` — the engine crate.

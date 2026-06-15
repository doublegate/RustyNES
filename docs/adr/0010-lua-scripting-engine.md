# ADR 0010 — Lua scripting engine architecture

**Status:** Accepted.
**Date:** 2026-06-14
**Author:** RustyNES maintainers
**Relates to:** `to-dos/v1.1.0-features/sprint-E-lua-flagship.md` (the v1.1.0
Lua flagship). Implemented by the `rustynes-script` crate (T-110-E1..E4).

## Context

v1.1.0 ships a Mesen2 / FCEUX-style Lua scripting API: scripts can read/write
emulator memory, inspect CPU/PPU/APU state, register per-frame and per-event
callbacks, drive control (save-states, input override), and draw overlays.

Two hard constraints shape the design:

1. **The chip stack is `#![no_std]`.** `rustynes-core` (and the chip crates)
   compile against `core` + `alloc` and cross-compile to bare-metal targets.
   A Lua VM (`mlua`, vendored C Lua 5.4) is `std`-only and needs a C compiler.
   It therefore **cannot** live inside the core or be called from inside the
   `no_std` run loop.
2. **The determinism contract is absolute.** Same seed + ROM + input ⇒
   bit-identical framebuffer + audio; AccuracyCoin 100% + the commercial
   oracles must stay byte-identical. Anything that mutates emulator state must
   be gated off in netplay / TAS replay / RA-hardcore, like the existing cheat
   path, and the *default* build must be byte-identical to a build with no
   scripting at all.

## Options considered

1. **VM inside the core, synchronous mid-instruction callbacks** (the FCEUX
   model). Rejected: impossible without dragging `std` + `mlua` into the
   `no_std` core, and reentrant Lua-calls-into-a-mid-tick-CPU create a
   borrow/ reentrancy hazard. It would also put a C dependency on the bare-metal
   target.
2. **Separate `std` engine crate, host-driven, callbacks at frame boundaries
   with live `Nes` access via `mlua::Lua::scope`.** Chosen.
3. **Separate engine, but only a copied RAM snapshot exposed to scripts.**
   Rejected: too limiting (scripts routinely read `$6000-$7FFF` SRAM and
   `$2000+`), and a copy each frame is wasteful versus a scoped live borrow.

## Decision

A new **`rustynes-script`** crate (a `std` crate, **not** a core dependency)
embeds `mlua` (Lua 5.4, `vendored`). It is **host-driven**: the frontend owns
the `Nes` and the run loop, and calls `ScriptEngine::on_frame(&mut nes)` once
per emulated frame.

Live emulator access from Lua uses **`mlua::Lua::scope`**: each frame the engine
opens a scope, registers `emu.read` / `readRange` / `write` / `cpu` functions
that borrow `&mut Nes` (through a `RefCell`) for the scope's duration, invokes
every registered callback, then tears the scope down. This gives scripts real
live memory/state access with no `'static` lifetime hacks and no snapshot copy.

The engine is pulled in only behind the frontend's optional **`scripting`**
feature. The default desktop build, the wasm build, and the `no_std`
cross-compile do **not** compile `rustynes-script` (they build specific crates
with `-p`), so the shipped emulator is byte-identical and carries no Lua / `cc`
dependency unless scripting is explicitly enabled.

**Sandbox:** only `table` / `string` / `math` / `coroutine` standard libraries
load; `io` / `os` / `package` / `require` / `debug` and the unsafe base globals
(`load`, `loadfile`, `dofile`, `loadstring`, `collectgarbage`) are removed;
`print` is redirected to a captured log. A VM-instruction-count hook aborts a
runaway callback (default 5M instructions/frame).

**Determinism + gating:** `emu.write` pokes only system RAM, *after* the
deterministic `run_frame` (the same mechanism as the raw-RAM cheat path), and
becomes a silent no-op when the host sets `set_writes_locked(true)` (netplay /
TAS replay / RA-hardcore). Memory reads (`emu.read`) are side-effect-free
(`Nes::peek` → `Bus::peek_cpu`), so they never perturb the emulator.

**Callback model.** `onFrame` fires synchronously inside the per-frame scope.
The per-event callbacks (`onExec` / `onRead` / `onWrite`) are **observational**:
they are dispatched by replaying the `debug-hooks` trace / event logs after the
frame, rather than synchronously mid-instruction. This is a deliberate
limitation versus FCEUX's mid-execution interception — it keeps the `no_std`
core and the determinism contract intact — and is documented in
`docs/scripting.md`. `onNmi` / `onIrq` markers wait on the non-`const` interrupt
tap noted in T-110-C3.

## Consequences

- Scripts get genuine live memory/state access and per-frame logic with a clean
  sandbox, while the core stays `no_std` and the default build stays
  byte-identical.
- `onExec`/`onRead`/`onWrite` cannot *alter* mid-instruction behaviour (only
  observe); a future synchronous-hook design would require a `dyn` callback
  trait threaded through the core run loop.
- The scripting CI job needs a C toolchain (vendored Lua); the default / wasm /
  no_std jobs are unaffected because they never build the crate.
- **Callback registry is Rust-side** (`Vec`/`HashMap` of `mlua::RegistryKey`),
  not a script-visible Lua global. A script can register callbacks but cannot
  inspect, clobber, or inject junk into the registry, so no malformed registry
  value can ever error the host pump — the protection is structural. (An earlier
  iteration kept the registry as a `__rustynes` Lua global and hardened each
  traversal point against junk; the Rust-side store removes the attack surface
  entirely and also makes the per-address callback gate a free `HashMap` lookup.)

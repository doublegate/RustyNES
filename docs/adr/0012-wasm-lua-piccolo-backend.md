# ADR 0012 — Experimental wasm Lua via a piccolo backend

**Status:** Accepted (experimental, off by default).
**Date:** 2026-06-15
**Author:** RustyNES maintainers
**Relates to:** [ADR 0010 — Lua scripting engine](0010-lua-scripting-engine.md)
(the v1.1.0 mlua engine this generalises); the v1.2.0 "Curator" release plan
(Workstream F4).

## Context

The v1.1.0 flagship scripting engine (`rustynes-script`) embeds **mlua**
(vendored Lua 5.4), which compiles Lua's C sources via `cc`. That C dependency
is exactly why scripting is **native-only**: `wasm32-unknown-unknown` has no C
toolchain in our `trunk` build, so the browser frontend ships without it.

The v1.2.0 plan wants scripting *available* in the browser demo, accepting that
a wasm Lua VM need not be a perfect match for the native one. The natural
candidate is **piccolo** (kyren): a pure-Rust, stackless Lua 5.4-ish VM built on
`gc-arena`, with no C dependency, that compiles cleanly to wasm32. Its **fuel**
mechanism maps directly onto our existing per-frame instruction budget.

The hard constraint is that the native path must stay **byte-identical**: all
of v1.1.0's accuracy and determinism guarantees (AccuracyCoin 100%, nestest
0-diff, TAS replay, netplay rollback) depend on the emulation core, and the
scripting engine must never perturb them. Scripts are observational / overlay
plus *gated* writes; they are **not** part of the framebuffer/audio determinism
oracle.

piccolo is, however, a *different* VM: a different (incomplete) Lua 5.4
implementation, a different garbage collector, and its own fuel accounting. Its
`Callback::from_fn` host functions are `'static + Fn`, so a callback **cannot**
hold a `&mut Nes` (the borrow is not `'static`) the way mlua's `lua.scope`
accessors can. That single fact forces a different host-binding shape on wasm.

## Decision

1. **Refactor `rustynes-script` behind a `VmBackend` trait.** The host-facing
   surface — `ScriptEngine`, `ControlCmd`, `DrawCmd`, `ScriptError`, the
   log/control/draw queues, the callback registration, and the write-gate — is
   the trait `VmBackend` (`backend.rs`) plus shared types (`types.rs`).
   `ScriptEngine` (`lib.rs`) is a thin facade over the compile-time-selected
   implementor. It is a **compile-time** switch, not a `dyn` object: piccolo's
   `'gc` lifetime makes a trait object impractical, and exactly one backend is
   ever compiled for a target.

2. **Two backends, selected by Cargo feature.**
   - `mlua-backend` (the crate default; the frontend's native `scripting`
     feature pulls it in) — the v1.1.0 engine, **logic verbatim**. Byte-identical.
   - `script-wasm` (off by default, a separate feature) — the piccolo backend.
     On the frontend it is wired through an aliased `rustynes-script-wasm`
     dependency built `--no-default-features --features script-wasm`, so the
     wasm build never compiles mlua's C and the native/shipped builds never
     compile piccolo.

3. **Accept documented divergence on the piccolo backend** (it is NOT
   byte-parity with mlua), because scripts are observational/overlay + gated
   writes and are never part of the determinism oracle. Specifically:
   - **Reads come from a per-frame snapshot.** At frame start the backend
     snapshots the 64 KiB CPU address space (via `peek`) plus the CPU registers
     and frame/cycle counters into `Rc<RefCell<…>>` cells that the `'static`
     callbacks read. `emu.read` / `peek` / `readRange` / `cpu` / `frame` /
     `cycle` serve from that snapshot, so a callback's view is internally
     consistent without a live `&mut Nes` borrow — and the whole backend stays
     `unsafe`-free.
   - **Writes are deferred.** `emu.write` is gated by `set_writes_locked`
     exactly like mlua; an accepted write is buffered AND reflected in the
     snapshot (so a same-frame read-after-write is consistent), then applied to
     the live `Nes` by the host *after* the frame's callbacks run, where the
     `&mut Nes` is held.
   - **Per-access / per-interrupt callbacks are native-only.**
     `emu.onExec` / `onRead` / `onWrite` / `onNmi` / `onIrq` are registered as
     **no-ops** on piccolo (a portable script that calls them does not error;
     `needs_exec_log` / `needs_access_log` / `needs_interrupt_log` all return
     `false`). They need the core's exec/access/interrupt logs and a hot
     per-event Lua re-entry that this first experimental cut does not wire up.
   - **`emu.setInput` queues a `ControlCmd` but is not applied on wasm** in this
     first cut (the native late-latch override path is native-only); it stays
     gated by `set_writes_locked` so it is harmless.

4. **Fuel ↔ budget mapping.** `DEFAULT_INSTRUCTION_BUDGET` (1,000,000) maps to
   `Fuel::with(budget.min(i32::MAX) as i32)`. Each `onFrame` callback runs in a
   fresh `Executor` stepped under that fuel cap; piccolo signals exhaustion by
   `Executor::step` returning `false` while `Fuel::should_continue()` is false,
   which the backend turns into `ScriptError::Budget` (a distinct variant from
   the mlua backend's `ScriptError::Lua`).

5. **Sandbox via `Lua::core()`** — base + `string`/`table`/`math`/`coroutine`,
   no `io`/`os`/`require`. `load`/`loadstring`/`dofile`/`loadfile`/
   `collectgarbage` are additionally niled out (defence-in-depth).

## Options considered

- **Compile mlua's Lua 5.4 to wasm via emscripten.** Rejected: it needs a C
  toolchain in the `trunk`/Pages build, balloons the wasm bundle by multiple MB,
  and complicates CI for an experimental feature.
- **A `dyn VmBackend` trait object chosen at runtime.** Rejected: piccolo values
  are `'gc`-bound, so the trait cannot be made object-safe without erasing the
  arena; and only one backend is ever wanted per target anyway.
- **Make the native mlua backend also snapshot-based for symmetry.** Rejected:
  it would change the native engine's observable behaviour (live mid-frame
  reads), breaking the byte-identical guarantee for zero benefit.
- **Faithfully implement onExec/onRead/onWrite/onNmi/onIrq on piccolo.**
  Deferred, not rejected: the per-event Lua re-entry is feasible but adds hot-
  path complexity disproportionate to an experimental first cut. Documented as a
  follow-up.

## Consequences

- **Native is unchanged.** The mlua backend is the v1.1.0 engine verbatim; its
  13 tests pass unchanged, and the shipped / wasm-without-`script-wasm` /
  `no_std` builds are byte-identical to v1.0.0/v1.1.0 (piccolo is never pulled).
- **The browser demo can run a useful subset of Lua** — HUD/overlay scripts,
  RAM watches via per-frame reads, gated pokes — behind an explicit,
  off-by-default, clearly-experimental feature.
- **Determinism is preserved** because the piccolo backend is, by construction,
  outside the oracle: its reads are a snapshot, its writes are gated + deferred,
  and it has no hook into the core's synthesis. A `script-wasm` build's
  framebuffer/audio is identical to a non-`script-wasm` build until a script
  issues a (gated, allowed) poke — the same contract as the Game Genie / raw-RAM
  cheat path.
- **The divergence is a maintenance signal.** Because piccolo is pre-1.0 and its
  Lua coverage is incomplete, the backend is feature-gated and labelled
  experimental in `docs/scripting.md`; scripts that must behave identically on
  desktop and web should stay within the documented supported subset.

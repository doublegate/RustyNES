# 16. Host-mediated `comm.*` script IPC — the host owns the socket

Date: 2026-06-18

## Status

Accepted (v1.7.0 "Forge" Workstream E1). Implemented behind the off-by-default
`script-ipc` feature; the `client.*` (E2) and `userdata.*` (E3) surfaces ship
unconditionally with `scripting`.

## Context

v1.7.0 "Forge" Workstream E turns RustyNES into a platform for external bots /
RL agents / randomizers / stream tools — RustyNES's determinism is a selling
point for reproducible RL episodes. The headline capability (E1, modelled on
BizHawk `CommLuaLibrary.cs`) is **IPC from a Lua script**: a TCP socket, HTTP
GET/POST, a WebSocket, and a memory-mapped-file bridge.

This collides head-on with the scripting sandbox. Since v1.1.0 (ADR 0010) the
Lua engine has guaranteed that a script **cannot reach the OS**: `Lua::new_with`
loads only `TABLE | STRING | MATH | COROUTINE`, and `io` / `os` / `package` /
`require` / `debug` / `load` / `loadfile` / `dofile` / `loadstring` are stripped
from the globals. The whole value of the sandbox is that an untrusted script
(downloaded from TASVideos, a Discord, a randomizer pack) cannot open a file,
spawn a process, or open a socket. A naive "give the script a socket" IPC API
would hand exactly that capability back.

IPC is also a **new non-deterministic input/output source**. The determinism
contract (same seed + ROM + input ⇒ bit-identical framebuffer + audio) is the
foundation of save-states, regression tests, TAS replay, and netplay rollback.
Bytes arriving from a socket are not part of that closed system; if they reached
the core synthesis they would break it.

## Decision

**The host owns every connection; the script only ever sees marshalled plain
values.** Concretely:

1. **`comm.*` is host-mediated, never raw.** The `rustynes-script` `comm` table
   does not expose a socket, a file handle, or any OS object. Each entry only
   *queues* a marshalled `CommCmd` (`SocketSend` / `HttpGet` / `HttpPost` /
   `WsOpen` / `WsSend` / `WsClose` / `MmfWrite` / `MmfRead`). A new frontend
   component — `rustynes-frontend::script_host::ScriptHost` — owns the actual
   TCP / HTTP / WebSocket / MMF connections, does the blocking I/O **on a
   dedicated worker thread off the emulator lock**, and feeds results back as
   plain `CommResult` values via `ScriptEngine::push_comm_result`. The script
   polls them with `comm.receive()`, which yields a small Lua table of strings /
   numbers. The VM never touches a connection, so the no-`io`/`os`/`package`/net
   sandbox guarantee is **preserved even with IPC on** — the sandbox stdlib set
   is unchanged.

2. **Off by default, behind `script-ipc`.** The `comm` table is only installed
   when the `rustynes-script/script-ipc` (and frontend `script-ipc`) feature is
   enabled. The shipped / native-default / `no_std` / wasm builds carry no IPC
   surface and are **byte-identical** without it. The feature pulls no new
   network dependency into `rustynes-script` (the host does the I/O); the
   frontend reuses its already-present native-only optional `ureq` for HTTP and
   `std` for TCP + the in-process MMF bridge.

3. **Gated like `emu.write`, disabled under a locked session.** Every `comm.*`
   verb reads the same `set_writes_locked` flag as `emu.write` / `emu.setInput`.
   Under netplay / TAS replay or record / RA-hardcore the verb is **dropped at
   the source** — no `CommCmd` is queued and the host opens no connection — so a
   deterministic / replayed session is provably unperturbed. The state-changing
   `client.*` verbs (`reboot_core`, `addcheat`, `removecheat`) gate the same way
   (with a defence-in-depth re-check in the host dispatch).

4. **The core never sees it.** `CommCmd` / `CommResult` live entirely in the
   frontend ⇄ script-host boundary. The `rustynes-core` `Nes` stack is `#![no_std]`
   and untouched. AccuracyCoin (139/139), nestest 0-diff, and the commercial
   oracle are unaffected.

`client.*` (E2 host-automation verbs: open tool, screenshot, window size, speed,
frameskip, A/V pause, cheats) and `userdata.*` (E3 per-script string→string KV
store, persisted across runs) ship **unconditionally** with `scripting`: they
introduce no socket and no new non-determinism (`userdata` is script-local host
memory; the `client` mutators reuse the existing gated host paths).

## Consequences

- An untrusted script gains useful IPC reach (bot endpoints, randomizer servers,
  stream overlays) **without** gaining a raw OS escape — the threat surface is
  the finite, host-audited `CommCmd` set, not "arbitrary sockets."
- A reproducible RL episode is possible: with `script-ipc` enabled and writes
  unlocked, an agent drives input via `emu.setInput` and exchanges observations
  over `comm.*`, while the deterministic core keeps the run replayable.
- The WebSocket transport currently reports a clean closed/error state (the
  host-owned contract + marshalling is in place); a full WS client (a `ws`
  crate) and an OS shared-memory MMF backing are maintainer follow-ups. The
  in-process named-buffer MMF works today for same-host single-process bridges.
- The off-by-default + gated + host-mediated triad means the feature can never
  perturb the determinism oracle: with `script-ipc` off the builds are
  byte-identical, and with it on a locked session queues nothing.

See `docs/scripting.md` (the `comm.*` / `client.*` / `userdata.*` reference) and
`crates/rustynes-frontend/src/script_host.rs` (the host bridge).

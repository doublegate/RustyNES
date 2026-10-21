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
  (Both done in v3.5.0: see the 2026-10-20 amendment.)
- The off-by-default + gated + host-mediated triad means the feature can never
  perturb the determinism oracle: with `script-ipc` off the builds are
  byte-identical, and with it on a locked session queues nothing.

See `docs/scripting.md` (the `comm.*` / `client.*` / `userdata.*` reference) and
`crates/rustynes-frontend/src/script_host.rs` (the host bridge).

## Amendment (2026-09-23, v2.7.3): HTTP destinations are gated

The decision above prevented a script from getting a raw socket. It did not
limit **where** the host would connect on a script's behalf: `comm.httpGet` and
`comm.httpPost` fetched any URL. A script downloaded from the Internet could
therefore reach the machine's own services, such as an unauthenticated
container API on `127.0.0.1` or a cloud metadata endpoint (frontend audit
SEC-04). The TCP transport never had this problem, because the user names its
endpoint (`RUSTYNES_COMM_TCP`).

The maintainer chose (2026-09-23): **public destinations by default; loopback,
private, link-local and similar addresses only when the user allowlists the
host** in `RUSTYNES_COMM_HTTP_ALLOW`. Blocking inward addresses outright was
rejected, because this ADR's own use case (local bot and RL endpoints) usually
means `localhost`. Requiring an allowlist for every destination was rejected as
needlessly strict for public endpoints.

How it holds:

- The check is in the HTTP client's resolver, on the addresses the connection
  uses, not on a separate lookup, so a name that answers differently to a
  second lookup gains nothing.
- Redirects are not followed. A public URL cannot hop inward past the check.
- The response body is limited to 10 MiB; a larger one is a transport
  failure (`status = 0`, empty body).
- No proxy is used, including one named in `HTTP_PROXY` / `ALL_PROXY`: through a
  CONNECT proxy the resolver would check the proxy's address, not the target's.

This uses ureq's `unversioned` resolver interface, which does not follow
semver. A breaking change there fails to compile; it cannot silently remove the
check.

## Amendment (2026-10-20, v3.5.0): a real WebSocket and real shared memory

The two follow-ups the Consequences above left open are done (CR-01, CR-02).
The decision itself is unchanged: the host owns every connection and region,
the script sees only marshalled values, the verbs are gated like `emu.write`,
and the Lua side (the `comm` table, the per-request and per-frame bounds, the
instruction budget) did not change at all.

**WebSocket.** `comm.ws_open` connects with `tungstenite`, the crate and
feature set the netplay signaling client already uses, so no crate entered the
lock file. Choices made:

- The host dials the TCP connection itself, to an address it has checked under
  the SEC-04 rule, and hands the connected stream to `tungstenite`. Calling
  `tungstenite::connect` would resolve the name a second time, out of the
  check's sight, which is the DNS-rebinding hole the HTTP resolver hook was
  written to close. The HTTP allowlist (`RUSTYNES_COMM_HTTP_ALLOW`) governs
  WebSockets too, rather than a second variable a user would have to learn.
- One socket at a time, because `ws_send` and `ws_close` name none; a second
  `ws_open` closes the first. Each socket has its own thread, so a 10-second
  handshake never stalls the HTTP queue.
- Inbound text has a budget (4 MiB undrained, 1 MiB per message). Over it the
  socket is simply not read, which pushes back through TCP instead of buffering.

**Shared memory.** `comm.mmfWrite` / `mmfRead` use POSIX `shm_open` on Linux
and macOS and named file mappings on Windows, through the platform APIs
directly (`libc`, `windows-sys`, both already in the lock file) behind
`// SAFETY:` notes in `crates/rustynes-frontend/src/shm.rs`. The maintained
crates were considered and not taken: `shared_memory` is not in the tree and
would bring its own dependency set (`nix` among them) for what is six calls per
platform, and `memmap2` maps files, which a Windows named mapping is not. Choices
made:

- Every script name is prefixed `rnes-mmf-` before it reaches the OS, so a
  script can open only RustyNES's regions, never another program's (shared
  memory is a per-user namespace that browsers, sound servers and Python all
  use). Names are limited to the characters and length every platform accepts
  (macOS allows 30 characters after the `/`).
- Each region carries a 32-byte header (magic, version, capacity, length, a
  sequence counter) in front of the payload, so a reader in another process
  knows how much is valid and can detect a copy that raced a write. A raw
  buffer would have matched BizHawk's Windows-only API more closely, but would
  have left every reader guessing at the length and unable to tell a torn read.
- The budget (16 MiB, 256 names) now counts the mapped bytes, since that is
  what the OS holds.

The memory-export interface of `T-SPEEDRUN-MEMIF` is built on the same module
(ADR 0047).

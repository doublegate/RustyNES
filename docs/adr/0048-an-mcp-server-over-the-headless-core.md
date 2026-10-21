# 48. An MCP server over the headless core

Date: 2026-10-20

## Status

Accepted. `T-MCP-SERVER` (D33), plan row in `to-dos/plans/v3.5.0-plan.md`
("Power-user rows"). The v4.6.0 game-playing runner `T-AI-PLAY` (D34) and the
v4.5.0 `T-RL-ENV` build on it.

## Context

The D33 research synthesis asked for a way for an agent (a language model, a
test script, a search driver) to drive RustyNES: read and write RAM, step,
set breakpoints, see the screen, and run experiments of the form
`fork(state) -> run(inputs) -> diff`, every experiment reproducible by
replay. `T-AI-PLAY` adds what it needs to play a game: pause, and layered
observations (the frame, the nametable as tile text, OAM).

The Model Context Protocol is the interface agent hosts already speak. Four
choices had to be made: where the server lives, which protocol implementation
it uses, what "reproducible" means on a machine an agent can also poke, and
what an untrusted agent may and may not do.

## Decision

1. **A new crate and binary, `rustynes-mcp`**, depending on `rustynes-core`
   (with `debug-hooks`, for exec breakpoints) and `rustynes-script` only. Not
   a frontend feature: the server needs no window, GPU or audio device, so it
   builds and runs where the frontend cannot (CI images, a headless host), and
   it collides with no other v3.5.0 work. `rustynes-script` already enables
   `debug-hooks` on the core, so workspace feature unification is unchanged.
2. **The official Rust MCP SDK, `rmcp` 3.5.1** (modelcontextprotocol/rust-sdk,
   Apache-2.0, allowed by `deny.toml`), with default features off and only
   `server` and `transport-io` on: no HTTP client or server, no auth stack, no
   child-process transport is compiled in. 3.5.1 rather than 3.6.0 because
   3.6.0 was published the day this was written; 3.5.1 had been out fifteen
   days. The tool surface is a hand-written `ServerHandler` (`list_tools`,
   `call_tool`) over a synchronous, transport-free `Session`, so every tool
   is testable without a client and the SDK is confined to one file.
3. **Stdio only.** No network listener ships. If one is added it binds
   loopback and requires a bearer token (the agents-and-scale line's rule);
   that is a later ADR amendment, not this one.
4. **Reproducible means a movie.** A fork is a second `Nes` built from the
   same ROM image and restored from a snapshot of its origin; it accepts
   controller input and nothing else, and records a `.rnm` whose start point
   is that snapshot. `movie_verify` replays the movie's serialised bytes on a
   third machine that sees only the ROM and the movie, and compares save
   state, work RAM, save RAM, framebuffer and frame counter byte for byte.
   The main machine is interactive, so it keeps a **journal**: a movie
   segment that restarts from a snapshot whenever something other than input
   changes the machine (a RAM write, an instruction step, a stop at a
   breakpoint, a reset, an adopted fork). The segment in force always replays
   exactly. No movie-format change was needed: the current `Movie` /
   `MovieRecorder` API with `StartPoint::SaveState` carries all of it.
5. **A write outside the declared tools is refused**, at four layers:
   - the catalogue is a fixed list; any other tool name is a JSON-RPC
     `-32602` error, and core debug writes that exist (`debug_poke_ppu`,
     `poke_oam_byte`, `debug_set_cpu_state`, `restore`) are deliberately not
     exposed;
   - every argument type is `deny_unknown_fields`, and the published schema
     says `additionalProperties: false`;
   - `write_ram`, the only memory write, accepts `$0000-$07FF` and nothing
     else (no mirrors, no registers, no cartridge space);
   - agent Lua runs in `rustynes-script`'s sandbox (no `io`, `os`,
     `package`/`require`, `debug`, `load`/`dofile`; the instruction budget;
     the host-memory limit) with `set_writes_locked(true)`, which drops
     `emu.write`, `memory:poke`/`write_range`, `emu.setInput` and
     `emu:load_state`. The server then compares the save state with the one
     before the script and, on any difference, restores it and refuses the
     call (defence in depth; it has never fired).
6. **No file access the operator did not grant.** The ROM path is a
   command-line argument; no tool opens a file. `movie_export` writes only
   into `--out-dir`, under a name of `[A-Za-z0-9_-]{1,64}` plus `.rnm`, and
   never over an existing file. Without `--out-dir` nothing is written.
7. **Pause by default.** The machine starts paused; `resume` runs it in real
   time on a runner thread at the region's frame rate, journalled like any
   other frames, until `pause`, a breakpoint, a CPU jam or `max_frames`
   (at most 36,000). Stepping, `reset` and `fork_adopt` are refused while it
   runs; observation is not.
8. **Limits are refused, not truncated**: frames per step and per fork run,
   instructions per step, forks alive, frames per fork movie, bytes per read,
   Lua source size and log size, breakpoints. A refused call changes nothing.

## Consequences

- Agents get a stable, self-describing tool surface whose every experiment is
  a `.rnm` anyone can replay with the ordinary movie player.
- `T-AI-PLAY` adds its adapters on top without touching the core: the
  observation layers, pause and the movie record are already here. A
  per-game RAM schema (shared with `T-GHOST-RACE`) is not part of this ADR.
- The journal is a segment, not a history: after a RAM write the frames
  before it are no longer in the replayable record. An agent that needs a
  whole experiment reproducible runs it on a fork.
- The movie records controller input only. When `T-MOVIE-INPUT-STREAM`
  (v3.5.0 row 1) adds Zapper, microphone, Vs. coin and FDS events to
  `.rnm`, a fork's input segments can grow the same fields; until then the
  server exposes no non-controller input, so nothing it does is unrecorded.
- Forks and the main machine are built from iNES / NES 2.0 images only; FDS
  and NSF sessions are not offered.
- New dependencies, all permissive and already allowed: `rmcp`, `schemars`
  (+ derive), `tokio-util`, `futures`, `chrono`, `iana-time-zone`,
  `dyn-clone`, `ref-cast`, `pastey`, `serde_derive_internals`,
  `tracing-attributes` (MIT or MIT/Apache-2.0; `rmcp` Apache-2.0). They are
  reached only from `rustynes-mcp`; no shipped binary links them.

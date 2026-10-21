<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
# MCP server (`rustynes-mcp`)

`rustynes-mcp` serves one ROM to a [Model Context Protocol](https://modelcontextprotocol.io)
client over stdio, so an agent (a language model, a test script, a search
driver) can observe the machine, step it, and run reproducible experiments.
Added in v3.5.0 (`T-MCP-SERVER`, D33). Decisions: [ADR 0047](adr/0047-an-mcp-server-over-the-headless-core.md).
Code: `crates/rustynes-mcp/`.

## Running it

```bash
cargo build --release -p rustynes-mcp
target/release/rustynes-mcp --rom path/to/game.nes [--out-dir path/to/exports]
```

An MCP client launches it as a command. For example, a client's server list:

```json
{ "rustynes": { "command": "/path/to/rustynes-mcp", "args": ["--rom", "/path/to/game.nes"] } }
```

- `--rom` (required) is an iNES / NES 2.0 image. FDS and NSF are not offered.
- `--out-dir` (optional) is an existing directory `movie_export` may write
  `<name>.rnm` files into. Without it the server writes no files.
- Stdout carries the protocol; diagnostics go to stderr.
- Transport: stdio only. There is no network listener.

The machine **starts paused**. Time moves only when a tool moves it
(`step_frames`, `step_instructions`, `fork_run`) or while `resume` runs it in
real time.

## Tools

Every tool's argument object is closed: an unknown field is refused, and the
schema `tools/list` publishes says `additionalProperties: false`. Pads are
lists of button names, case-insensitive: `A`, `B`, `Select`, `Start`, `Up`,
`Down`, `Left`, `Right`. An unknown name is refused, never dropped.

| Tool | Arguments | What it does |
| --- | --- | --- |
| `info` | — | ROM SHA-256, region, mapper, frame, run state, held input, breakpoints, journal segment, forks, limits |
| `pause` | — | Stop real-time running; reports frames run and why it stopped |
| `resume` | `max_frames?` (1-36000) | Run the main machine in real time with the held input |
| `set_input` | `p1..p4` | Set the held buttons |
| `step_frames` | `frames` (1-3600), `input?` | Run whole frames; stops early at an exec breakpoint |
| `step_instructions` | `count` (1-1000000) | Run CPU instructions |
| `breakpoint_add` / `breakpoint_remove` | `pc` | Exec breakpoints (at most 64) |
| `breakpoint_list` | — | |
| `read_memory` | `space` (`cpu`/`ppu`/`oam`), `addr`, `len` (1-4096) | Side-effect-free read: a `$2002` read does not clear vblank |
| `write_ram` | `addr`, `bytes` | Write console work RAM, `$0000-$07FF` only |
| `cpu_state` | — | Registers, cycles, frame |
| `screenshot` | — | The frame as a 256x240 PNG |
| `nametable` | `index` (0-3) | 30x32 tile indices in hex, the attribute table, the PPU scroll registers |
| `oam` | — | The visible sprites (position, tile, palette, priority, flips); off-screen ones counted |
| `observe` | `layers?` (`frame`, `nametable`, `oam`, `cpu`) | Several layers in one call (default: all) |
| `reset` | `kind` (`soft`/`power`) | Reset or power-cycle the main machine |
| `lua_eval` | `source` | Sandboxed, read-only Lua 5.4; `emu.onFrame` callbacks run once |
| `fork_create` | `from?` (`main` or a fork id) | Branch a machine from a state; returns `f1`, `f2`, ... (at most 32) |
| `fork_run` | `fork`, `inputs: [{frames, p1..p4}]` | Run input segments on a fork (at most 36000 frames a call, 216000 a fork) |
| `fork_diff` | `a`, `b` (`main` or fork ids) | Work-RAM bytes (first 256 listed, all counted), save-RAM bytes, framebuffer pixels, CPU registers, and whether the save states are identical |
| `fork_adopt` | `fork` | Main continues from the fork's state |
| `fork_delete` | `fork` | Discard a fork |
| `movie_export` | `source`, `name?` | The movie as base64 `.rnm` (+ SHA-256); with `name`, also `<out-dir>/<name>.rnm` |
| `movie_verify` | `source` | Replay the movie on a fresh machine and compare state byte for byte |

Results are a JSON object (sent as the first text block and as
`structuredContent`), then any text layers, then any image. A refused or
failed call is a tool result with `isError: true`; an undeclared tool name is
a JSON-RPC error (`-32602`).

### Lua

`lua_eval` runs the script in `rustynes-script`'s sandbox (see
[scripting.md](scripting.md)): no `io`, `os`, `package` / `require`,
`debug`, `load` / `dofile`; the per-call instruction budget; the host-memory
limit. Top-level code runs at load with no machine bound; register
`emu.onFrame(function() ... end)` to read the machine, and it is called once
against the current state. Writes are locked: `emu.write`, `memory:poke`,
`memory:write_range`, `emu.setInput` and `emu:load_state` are dropped, and
control requests and overlay draws are counted and
ignored. After the script the server checks the save state is byte-identical
to what it was and, if not, restores it and refuses the call. `emu.log` /
`print` output comes back as text, capped at 64 KiB.

## Reproducibility

The core is deterministic (same state + same input = the same bytes), and the
server keeps a movie of everything that can be replayed:

- **A fork** is a second machine built from the same ROM image and restored
  from a snapshot of its origin. It accepts controller input and nothing
  else, and records every frame into a `.rnm` whose start point is that
  snapshot. The movie is the whole experiment.
- **The main machine** is interactive, so it keeps a **journal**: a movie
  segment that restarts from a snapshot whenever something other than input
  changes the machine (`write_ram`, `step_instructions`, a stop at a
  breakpoint, `reset`, `fork_adopt`, or a segment reaching 216000 frames).
  The segment in force always replays. `info` reports its number, length and
  why it began. Frames run in real time are journalled like stepped ones.

`movie_verify` serialises the movie, replays those bytes on a third machine
built from the ROM alone, and compares save state, work RAM, save RAM,
framebuffer and frame counter. An exported `.rnm` is an ordinary movie: the core's own player
(`Movie::seek_to_start`, `MoviePlayer`) replays it, which is all
`movie_verify` uses.

What a movie does not hold: the agent's reasoning, Lua output, and anything
before the journal segment began. Movies record the four controller ports
only, which is also all the input this server exposes.

## Security posture

| Route | Answer |
| --- | --- |
| A tool not in the catalogue (including real core debug writes: `debug_poke_ppu`, `poke_oam_byte`, `debug_set_cpu_state`, `restore`) | JSON-RPC `-32602`; nothing runs |
| An argument a tool does not declare | Refused (`deny_unknown_fields`) |
| `write_ram` outside `$0000-$07FF` (mirrors, PPU/APU registers, cartridge space) | Refused; nothing written |
| Lua writes, input, state loads | Dropped by the write lock; the machine is checked unchanged |
| Lua `io` / `os` / `require` / `load` / `dofile` / `debug` | Absent from the sandbox |
| A runaway Lua loop | Stopped by the instruction budget |
| Reading a file | No tool opens one; the ROM is the operator's `--rom` |
| Writing a file | Only `movie_export` into `--out-dir`, name `[A-Za-z0-9_-]{1,64}`, never over an existing file |
| Unbounded work | Every count is capped, and a call over a cap is refused, not truncated |
| Network | None: stdio only. A listener, if one is ever added, binds loopback and requires a token (ADR 0047) |

Text an agent reads from the game (nametable tiles, RAM) is data; the server
never interprets it.

## Gate

The plan's gate, as tests in `crates/rustynes-mcp/tests/`:

- `gate.rs` `scripted_session_forks_replay_byte_identically`: a scripted
  agent session (observe, step, four forks with different inputs, an appended
  run, a fork of a fork, diffs) whose every fork replays byte-identically,
  through `movie_verify` and through an independent replay of the exported
  bytes outside the server. The fixture ROM (assembled in
  `tests/common/mod.rs`, no ROM file) reads the pad every frame into work RAM
  and paints the backdrop from it, so different inputs diverge in RAM and on
  screen.
- `gate.rs` `the_main_journal_replays_across_every_non_input_change`.
- `refusals.rs`: undeclared tools, undeclared arguments, `write_ram` bounds,
  Lua writes and sandbox doors, file writes, limits — each with the machine's
  save state compared before and after.
- `stdio.rs`: the binary driven over stdio by a JSON-RPC client (handshake,
  `tools/list`, a call with an image, a refused call, `-32602`).

### Mutations

Each mutant changes a value (all sixteen compile) and was run against the
test file named; every one fails at least one test (2026-10-20). Two were
NOT CAUGHT on the first run and were remediated in the same change:
M2 was stimulus-blind (stepping instructions with the input unchanged
replays either way, because `run_frame` finishes the frame; the test now
steps through the controller read and then changes the input), and M6
was out of surface (the second Lua layer is unreachable while the first
holds; a unit test now opens the first layer through a private field no
tool reaches).

| # | Mutation | Caught by |
| --- | --- | --- |
| M1 | A fork records a different input from the one it ran | `scripted_session_forks_replay_byte_identically` |
| M2 | An instruction step keeps the journal segment | `the_main_journal_replays_across_every_non_input_change` |
| M3 | A breakpoint stop keeps the journal segment | `the_main_journal_replays_across_every_non_input_change` |
| M4 | `write_ram` keeps the journal segment | `the_main_journal_replays_across_every_non_input_change` |
| M5 | Agent Lua runs with writes unlocked | `lua_writes_are_dropped_and_the_machine_is_unchanged` |
| M6 | The post-Lua state check is disabled | `session::tests::a_lua_write_past_the_lock_is_undone_and_refused` |
| M7 | `write_ram`'s bound widened to `$2000` | `write_ram_writes_work_ram_and_nothing_else` |
| M8 | `write_ram` accepts unknown fields | `undeclared_arguments_are_refused_not_ignored` |
| M9 | An undeclared tool name falls through to `info` | `undeclared_tools_are_refused_and_change_nothing` |
| M10 | The export name allows `/` and `.` | `files_are_written_only_where_and_as_the_operator_allows` |
| M11 | Export overwrites an existing file | `files_are_written_only_where_and_as_the_operator_allows` |
| M12 | The replayer drops the last frame | three gate tests |
| M13 | A fork starts at power-on, not its origin | `scripted_session_forks_replay_byte_identically` |
| M14 | `fork_diff` blind to work RAM | `scripted_session_forks_replay_byte_identically` |
| M15 | The main machine journals a different input from the one it ran | `real_time_running_...`, `the_main_journal_...` |
| M16 | The real-time runner ignores `max_frames` | `real_time_running_pauses_and_stops_at_breakpoints` |

What the gate cannot verify: that a fork's continuation equals what the main
machine would have done from the same state (that is save-state fidelity,
gated by the core's own save-state tests); a real MCP host's behaviour beyond
the JSON-RPC exchange `stdio.rs` performs; and anything about a commercial
ROM (the fixture is the only ROM the suite runs).

## Not done

- No HTTP transport (stdio covers every MCP host that launches a command).
- No per-game RAM schemas (`T-AI-PLAY`, `T-GHOST-RACE`).
- No non-controller input (Zapper, microphone, Vs. coins, FDS disk events)
  until `.rnm` carries it (`T-MOVIE-INPUT-STREAM`).
- No audio observation.
- FDS and NSF images are not loaded.

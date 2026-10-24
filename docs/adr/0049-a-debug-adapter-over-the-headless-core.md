# 49. A debug adapter and a GDB stub over the headless core

Date: 2026-10-23

## Status

Accepted. `T-DEBUG-DAP` (D33), plan row in `to-dos/plans/v3.6.0-plan.md`
("Power-user rows"). Spec: `docs/dap-debugger.md`.

## Context

The D33 research synthesis asked for source-level debugging of homebrew from
an editor: breakpoints on source lines, stepping by line, and variables, for
the two C toolchains NES homebrew uses: **cc65** (debug information in a
separate `ld65 --dbgfile` text file) and **llvm-mos** (DWARF 5 in the ELF it
links beside the `.nes`). RustyNES already had exec breakpoints in the core
(`Nes::add_breakpoint`, the `debug-hooks` feature, used by the frontend
debugger, Lua and `rustynes-mcp`) and a cc65 line-map importer in the frontend
(`debugger/source_map.rs`, which carries a Mesen2 provenance header).

Five choices had to be made: where the adapter lives, which protocols it
speaks and how, how it reads the two debug formats, how it finds a local's
address on a CPU with no frame pointer, and what a client may do.

## Decision

1. **A new crate and binary, `rustynes-dap`**, depending on `rustynes-core`
   (with `debug-hooks`) and two read-only parsers. Like `rustynes-mcp`
   (ADR 0048) it is not a frontend feature: an editor launches a debug
   adapter as a child process, and it needs no window, GPU or audio device.
   The frontend's `source_map.rs` is not reused: it is a display aid with a
   derived-code header, it reads only line records, and the adapter needs
   scopes, symbols and C symbols too, so the adapter has its own reader.
2. **Breakpoints are the core's.** The adapter arms the core's exec
   breakpoint list with the union of user breakpoints, per-function **frame
   hooks** and one temporary breakpoint for step-over and step-out, and keeps
   the reason for each address itself. The core stops before the PC and skips
   it once on the next `run_frame`, so a hook is invisible to the machine:
   `tests/engine_fixtures.rs` compares a debugged machine with the bare core
   byte for byte.
3. **Two protocols, two transports.**
   - **DAP over stdio**, implemented from the Debug Adapter Protocol
     specification. Stdio is how editors launch adapters, and it opens no
     socket. There is no DAP TCP mode.
   - **A GDB Remote Serial Protocol stub over TCP**, implemented from the GDB
     manual's protocol and target-description appendices, **bound to loopback
     only**: a non-loopback address is refused at startup, and there is no
     option to change that. One client per run.
4. **Read-only.** No DAP `setVariable` / `setExpression` / `writeMemory` /
   `restart`; the GDB stub answers `G`, `P`, `M` and `X` with `E01`. A session
   observes the program and cannot change it, which keeps it on the
   determinism contract (same ROM, same requests, same stops and values).
5. **DWARF through `gimli` and `object`, location expressions by hand.**
   `gimli` 0.34 (MIT OR Apache-2.0) reads the line program and DIEs; `object`
   0.40 (Apache-2.0 OR MIT) reads the ELF, with only `read_core`, `elf` and
   `std`. Both were published more than two weeks before this was written.
   llvm-mos's register numbers do not fit gimli's 16-bit `Register`, so the
   handful of location operations a 6502 compiler emits (`DW_OP_addr`,
   `addrx`, `fbreg`, `reg0..31`, `regx`) are decoded in the crate; anything
   else is reported as unavailable rather than guessed.
6. **Register numbers are llvm-mos's published ones.** The *DWARF
   Specification for MOS-Compatible Processors* (llvm-mos.org): `A`=0 ..
   `PC`=5, `RCn`=`0x0002_0000+n`, `RSn`=`0x0003_0000+n` (the pair
   `RC(2n)`/`RC(2n+1)`); the zero-page address of `RCn` is the ELF symbol
   `__rcN`. The GDB target description numbers the physical registers 0-5 in
   that order and adds `rcN` / `rsN` with LLDB's `dwarf_regnum` attribute when
   an ELF is given.
7. **A shadow call stack from frame hooks.** Neither toolchain emits call
   frame information for the 6502, so the engine records `(function, S)` at
   each function's entry and samples its frame base at its frame hook: for
   llvm-mos the line program's `prologue_end`; for cc65 the instruction after
   a prologue `JSR` to a runtime push routine (`pusha`, `pushax`, ...),
   because cc65's `csym` offsets are relative to the soft-stack pointer
   (`c_sp`, `sp` in cc65 2.19) after the register-passed parameter is pushed.
   That rule was established on the fixture, black-box: the values read back
   are the C program's (`v == 5`, `r == 8`). Frames whose entry `S` is below
   the current `S` have returned and are dropped; caller positions are read
   from the return addresses the `JSR`s pushed.
8. **Fixtures are our own program, built by both toolchains and committed.**
   `tests/roms/dap/dapdemo.c` (CC0-1.0) and `build.py`; the built ROMs and
   debug files are committed so `cargo test --workspace` runs the end-to-end
   gate everywhere, and the CI job `dap-fixtures` rebuilds them with pinned
   toolchains (cc65 at commit `034f73a9e1`, the llvm-mos SDK v23.3.0, its
   archive's SHA-256 checked) and runs the same tests against its own build.
   The ROMs link the toolchains' runtime libraries (cc65: zlib; llvm-mos SDK:
   Apache-2.0 WITH LLVM-exception), recorded in `tests/roms/LICENSES.md`.

## Consequences

- An editor with a generic DAP client (VS Code's, or any other) debugs cc65
  and llvm-mos homebrew in C: line, function and instruction breakpoints;
  next, step in, step out, instruction step; a call stack; parameters,
  locals, globals, registers; hover evaluation and the memory view.
- **No released GDB or LLDB has a MOS 6502 architecture**, so the GDB stub's
  only client today is a script or a front end that symbolicates for itself;
  its tests are a protocol client written in Rust. The stub is ready for an
  LLDB or GDB that gains MOS support, and that has not been tried.
- cc65 records no C types: cc65 variables show as one byte with a type that
  says so. llvm-mos variables show with their DWARF types; location lists,
  composite locations and computed locations show as unavailable.
- Before a function's frame hook has run in the current call (stopped on its
  opening brace), its frame-relative variables show "frame base not yet set"
  instead of reading through a stale pointer.
- A function entered other than through its entry address (a tail jump into
  its middle) is not recorded; its frame shows without variables.
- New dependencies, permissive and already allowed by `deny.toml`: `gimli`,
  `object` (and `base64`, already in the lock file). They are reached only
  from `rustynes-dap`; no shipped binary links them.

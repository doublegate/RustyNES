<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
# Source-level debugger (`rustynes-dap`)

`rustynes-dap` debugs a homebrew NES program in its C source from an editor,
over the [Debug Adapter Protocol](https://microsoft.github.io/debug-adapter-protocol/),
and offers a GDB Remote Serial Protocol stub beside it. It reads the debug
information of the two C toolchains NES homebrew uses: **cc65** (`ld65
--dbgfile`) and **llvm-mos** (DWARF in the ELF). Added in v3.6.0
(`T-DEBUG-DAP`, D33). Decisions: [ADR 0049](adr/0049-a-debug-adapter-over-the-headless-core.md).
Code: `crates/rustynes-dap/`.

## Building a program to debug

```bash
# cc65: -g for C and assembler debug info, --dbgfile for the linker's file.
cl65 -t nes -g -Wl --dbgfile,game.dbg -o game.nes game.c

# llvm-mos: -g; the ELF is written beside the image as game.nes.elf.
mos-nes-nrom-clang -O0 -g -o game.nes game.c
```

Optimisation makes stepping jump around and removes variables, as on any
target; `-O0` is the debugging build.

## Running it

```bash
cargo build --release -p rustynes-dap
```

An editor launches `target/release/rustynes-dap` as its debug adapter (stdio;
no port, no listener) and sends a `launch` request with these arguments:

| argument | meaning |
| --- | --- |
| `program` | the iNES / NES 2.0 image (required unless `--rom` was given) |
| `debugInfo` | the cc65 `.dbg` or the llvm-mos ELF; default: `<program>.elf`, then `<program stem>.dbg`, if present |
| `sourceRoot` | directory that relative source names resolve against; default: the debug file's directory |
| `stopOnEntry` | stop at the reset vector before running (default `false`) |

Editors that take an adapter command directly (Neovim's `nvim-dap`, Emacs
`dape`, Helix, Zed) point it at the binary. VS Code needs a debugger
contribution; a `package.json`-only extension is enough:

```json
{
  "name": "rustynes-debug", "version": "0.0.1", "engines": { "vscode": "^1.80.0" },
  "contributes": { "debuggers": [{
    "type": "rustynes", "label": "RustyNES", "program": "/path/to/rustynes-dap",
    "languages": ["c"]
  }] }
}
```

and a `launch.json` entry:

```json
{ "type": "rustynes", "request": "launch", "name": "Debug game",
  "program": "${workspaceFolder}/game.nes", "debugInfo": "${workspaceFolder}/game.dbg" }
```

The extension snippet above has not been exercised in a VS Code instance by
the project; the protocol it relies on is what the tests drive.

## What it does

| | |
| --- | --- |
| Breakpoints | source lines (a line with no code moves to the next one that has some), function names, raw addresses (instruction breakpoints) |
| Stepping | `next` (over calls), `stepIn` (into calls that have source; it finishes the callee's prologue so the stop is on its first body line), `stepOut`, and single instructions with `granularity: "instruction"`; `pause` |
| Stack | one frame per recorded call, innermost first, with the source line of each call site |
| Variables | Locals (parameters first) per frame, Globals, Registers (`A` `X` `Y` `P` with flags, `S`, `PC`) |
| Evaluate | a variable name, a register name, or `$XXXX` / `0xXXXX` for one byte |
| Memory | `readMemory` over the CPU bus, without register side effects (reading `$2002` does not clear VBlank) |

### What the debug information gives, per toolchain

| | cc65 | llvm-mos |
| --- | --- | --- |
| Lines | C lines (`type=1`), so steps move by C statement; a pure-assembler program uses its assembler lines | the DWARF line program; `is_stmt` rows are statement starts |
| Locals | `csym` `sc=auto`: offset from the soft-stack pointer (`c_sp`, or `sp` in cc65 2.19) as it stands after the prologue's `JSR pusha` | `DW_OP_fbreg` from the frame base `RS0` (`__rc0`/`__rc1`) |
| Globals | `csym` `ext`/`static`, else the `_name` data labels the C compiler emits | `DW_TAG_variable` at file scope |
| Types | none recorded: every variable shows as one byte, with a type saying so | DWARF base types, pointers, typedefs, qualifiers, enums, arrays |

The frame base is sampled at each function's **frame hook** on every call
(for llvm-mos the line program's `prologue_end`; for cc65 the instruction
after a prologue `JSR` to a push routine) and kept in a shadow call stack, so
caller frames show their own locals. Stopped on a function's opening brace,
before its frame hook has run, its frame-relative variables say "frame base
not yet set".

## GDB stub

```bash
rustynes-dap --gdb 127.0.0.1:2345 --rom game.nes [--debug-info game.nes.elf]
```

One client, loopback only (a non-loopback address is refused). Packets:
`qSupported`, `QStartNoAckMode`, `qXfer:features:read:target.xml`, `?`, `g`,
`p`, `m`, `Z0`/`Z1`, `z0`/`z1`, `c`, `s`, `vCont`, the `0x03` interrupt,
`k`, `D`. `G`, `P`, `M` and `X` answer `E01`: the stub is read-only.

The target description numbers `a x y p s pc` as registers 0-5, which are
llvm-mos's DWARF numbers for them. With an llvm-mos ELF it adds the imaginary
registers `rcN` (from the ELF's `__rcN` symbols) and `rsN` (adjacent pairs),
each with its DWARF number in a `dwarf_regnum` attribute (LLDB's extension;
GDB ignores it).

**No released GDB or LLDB has a MOS 6502 architecture**, so the stub's
clients today are scripts and front ends that symbolicate themselves; it has
not been tried with a GDB or LLDB build that has MOS support.

## Security posture

- The DAP server speaks only on its own stdin/stdout; it opens no socket.
- The GDB stub binds loopback only and serves one client.
- Nothing either protocol offers writes emulated memory or registers.
- Files are read only where the launching user names them (`program`,
  `debugInfo`, or the command line), size-bounded: 16 MiB for a ROM, 64 MiB
  for debug information, 1 MiB for a DAP message, 4 KiB for a GDB packet.

## Gate

`cargo test -p rustynes-dap` runs, against `tests/roms/dap/` (our own
`dapdemo.c`, CC0-1.0, built by `build.py` with both toolchains and committed):

- `tests/dap_e2e.rs`: the real binary over stdio with a scripted DAP client,
  per toolchain: stop at a source breakpoint, check the stack (`add3` called
  from `main`'s call line), read the parameter `v == 5` and `main`'s local,
  `next` to the return line and read `r == 8` (as a variable, through
  `readMemory` and through `evaluate`), continue to the second call
  (`v == 8`, global `counter == 8`), pause while running, disconnect.
- `tests/engine_fixtures.rs`: the same through the engine API, plus step in
  and step out, a breakpoint that moves forward, and the **no-perturbation**
  check (a debugged machine equals the bare core byte for byte at the same
  stop).
- `tests/gdb_e2e.rs`: the stub over TCP: target description, `Z0`, `c` stops
  at the line's address, `v` read through `rs0` plus its `fbreg` offset,
  `s`, write refusals, interrupt, and the non-loopback refusal.

CI's `dap-fixtures` job rebuilds the fixtures with pinned toolchains (cc65
`034f73a9e1`, llvm-mos SDK v23.3.0) and runs these tests against that build
(`RUSTYNES_DAP_FIXTURES`), so the gate covers what the toolchains produce
today, not only what was committed.

### Mutations

Each mutant was applied to the crate and `cargo test -p rustynes-dap` run
(driver and logs in the gitignored `salvaged/evidence/v3.6.0-dap/`). The
first sweep left three NOT CAUGHT; each was a gap in the tests, closed by
the test named in the last column, and the re-run caught all three.

| mutant | first sweep | caught by |
| --- | --- | --- |
| cc65 frame hook left on the prologue `JSR` | caught | cc65 variable reads |
| continue does not step off the breakpoint it is on | caught | second-call reads (`v == 8`) |
| a returned frame kept on the shadow stack (`<=` for `<`) | caught | stack and step checks |
| a line step stops on its own line | caught | `next` line checks |
| `next` and `stepIn` swapped | caught | step-over and step-in checks |
| caller position taken as the pushed word, not the `JSR` | NOT CAUGHT (same source line) | the caller's PC must hold a `JSR` opcode |
| frame offset subtracted | caught | `r` reads |
| debugger reads through the side-effecting bus probe | NOT CAUGHT (no register was read) | the no-perturbation check now reads all 64 KiB, `$2002` included |
| DAP reports a breakpoint stop as a step | caught | DAP stop-reason checks |
| GDB stop reply PC byte order | caught | GDB stop-address checks |
| GDB `M` no longer refused | caught | GDB write-refusal check |
| cc65 overlapping C lines: the wider wins | NOT CAUGHT (no overlap in the fixtures) | cc65 unit test with an enclosing C line |
| DWARF frame hook left at `low_pc` | caught | llvm-mos variable reads |
| DWARF `fbreg` offset off by one | caught | decoder unit test and llvm-mos reads |

## Not done

- No VS Code extension is shipped (the `package.json` above is a sketch), and
  no editor other than the scripted client has been run against the adapter.
- No GDB or LLDB has driven the stub (none has a MOS target).
- No writes, by design (ADR 0049).
- cc65 types (cc65 records none); DWARF location lists and composite
  locations; lexical-block scoping (block variables are shown for the whole
  function); disassembly (`disassemble`); data breakpoints and watchpoints;
  conditional and hit-count breakpoints.
- FDS and NSF images (`launch` takes iNES / NES 2.0 only).

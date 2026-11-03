// SPDX-License-Identifier: GPL-3.0-or-later
//! `rustynes-dap` — source-level debugging of homebrew NES programs over the
//! Debug Adapter Protocol and a GDB Remote Serial Protocol stub (v3.6.0,
//! `T-DEBUG-DAP`, D33).
//!
//! A homebrew author builds a program with cc65 or llvm-mos, opens it in an
//! editor that speaks DAP (VS Code, or any other DAP client), and debugs it in
//! the C source: breakpoints on source lines and functions, stepping by line
//! (over, into, out of calls) or by instruction, a call stack, parameters and
//! locals, globals, registers and memory.
//!
//! The design is recorded in `docs/adr/0049-a-debug-adapter-over-the-headless-core.md`
//! and specified in `docs/dap-debugger.md`. The pieces:
//!
//! - [`debuginfo`] lowers cc65's `ld65 --dbgfile` output and llvm-mos's DWARF
//!   into one model: line table, statement starts, functions, variables and
//!   their frame bases.
//! - [`engine`] runs one [`rustynes_core::Nes`] with the core's own exec
//!   breakpoints, line stepping, and a shadow call stack recorded from frame
//!   hooks, without perturbing the machine.
//! - [`dap`] is the Debug Adapter Protocol server on stdio.
//! - [`gdb`] is the GDB remote stub on a loopback socket, read-only.
//!
//! # A debugging session observes; it never writes
//!
//! Neither front end exposes a write to emulated memory or registers. A stop
//! adds no cycle and changes no state (`tests/engine_fixtures.rs` compares a
//! debugged machine with an undebugged one byte for byte), so a session is on
//! the determinism contract: the same ROM and the same requests give the same
//! stops and the same values.

pub mod dap;
pub mod debuginfo;
pub mod engine;
pub mod gdb;

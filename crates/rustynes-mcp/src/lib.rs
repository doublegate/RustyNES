//! `rustynes-mcp` — a Model Context Protocol server over the headless
//! `RustyNES` core (v3.5.0, `T-MCP-SERVER`, D33).
//!
//! An agent (a language model, a test script, a search driver) connects over
//! stdio and drives one emulator through a fixed set of **declared tools**:
//! observe the machine (the frame as a PNG, the nametables as tile text, OAM,
//! the CPU registers, memory), step it (frames or instructions, with exec
//! breakpoints), write work RAM, run sandboxed read-only Lua, and run
//! **experiments**: `fork(state) -> run(inputs) -> diff`.
//!
//! The design is recorded in `docs/adr/0048-an-mcp-server-over-the-headless-core.md`
//! and specified in `docs/mcp-server.md`. The load-bearing decisions:
//!
//! # Every experiment replays from its recorded inputs
//!
//! A fork is a second `Nes` built from the same ROM and restored from a
//! snapshot of its origin. Running it records a `.rnm` movie whose start point
//! is that snapshot ([`rustynes_core::StartPoint::SaveState`]) and whose frames
//! are exactly the inputs the agent supplied. A fork run accepts **only**
//! controller input — no RAM write, no instruction step, no breakpoint — so the
//! movie is the complete description of the experiment, and
//! `movie_verify` re-derives it on a fresh machine and compares the result
//! byte for byte (save state, work RAM, framebuffer). The core's determinism
//! contract is what makes that comparison exact rather than approximate.
//!
//! The main timeline is interactive and so cannot promise the same of its
//! whole history: a RAM write or a mid-frame stop is not an input. It keeps a
//! **journal** instead: a movie segment that restarts, from a snapshot of the
//! moment, whenever something other than controller input changes the machine.
//! The current segment always replays exactly; `info` reports how many
//! segments the session has broken into and why the last one began.
//!
//! # A write outside the declared tools is refused
//!
//! The tool catalogue ([`tools::TOOL_NAMES`]) is the whole surface. A call to
//! any other name is a protocol error; an argument object with an unknown
//! field is refused (`deny_unknown_fields`), so a misspelt or invented option
//! never silently becomes a default; `write_ram` accepts the 2 KiB of console
//! work RAM and nothing else. Agent-supplied Lua runs in `rustynes-script`'s
//! sandbox (no `io` / `os` / `require` / `load`, an instruction budget, a
//! host-memory limit) **with writes locked**, so `emu.write`, `memory:poke`,
//! `emu.setInput` and `emu:load_state` are dropped; the server then checks
//! the machine's save state is byte-identical to what it was before the script
//! ran and, if it is not, restores it and reports the breach. No tool reads or
//! writes a file the operator did not name on the command line: the ROM path is
//! the operator's, and movies are exported only into the operator's
//! `--out-dir`, under a name restricted to `[A-Za-z0-9_-]`.
//!
//! # Transport
//!
//! Stdio only ([`host::serve_stdio`], the official `rmcp` SDK). There is no
//! network listener; ADR 0048 records that one, if added, binds loopback and
//! requires a token.

pub mod host;
pub mod input;
pub mod observe;
pub mod session;
pub mod tools;

pub use host::Host;
pub use session::{Session, SessionConfig, ToolError, ToolOutput};

/// Hard limits on what one call may ask of the server.
///
/// Every one bounds memory or wall-clock time an agent could otherwise spend
/// without limit. They are deliberately generous for real use (an hour of
/// fork footage, a minute of frames per step) and small next to the host.
pub mod limits {
    /// Frames one `step_frames` call may run (one minute at 60 Hz).
    pub const MAX_STEP_FRAMES: u32 = 3_600;
    /// Instructions one `step_instructions` call may run.
    pub const MAX_STEP_INSTRUCTIONS: u32 = 1_000_000;
    /// Frames one `fork_run` call may run across all of its segments
    /// (ten minutes at 60 Hz).
    pub const MAX_FORK_RUN_FRAMES: u32 = 36_000;
    /// Frames a single fork's movie may hold (an hour at 60 Hz). A further
    /// `fork_run` is refused rather than silently truncating the record.
    pub const MAX_FORK_FRAMES: usize = 216_000;
    /// Frames the main journal keeps before it rolls to a new segment.
    pub const MAX_JOURNAL_FRAMES: usize = 216_000;
    /// Live forks at once. Each holds a whole machine.
    pub const MAX_FORKS: usize = 32;
    /// Bytes one `read_memory` call may return.
    pub const MAX_READ_LEN: u32 = 0x1000;
    /// Bytes of Lua source one `lua_eval` call may submit.
    pub const MAX_LUA_SOURCE: usize = 64 * 1024;
    /// Bytes of Lua log text one `lua_eval` call returns (further lines are
    /// counted and dropped).
    pub const MAX_LUA_LOG: usize = 64 * 1024;
    /// Exec breakpoints at once.
    pub const MAX_BREAKPOINTS: usize = 64;
    /// Work-RAM differences `fork_diff` lists individually (all are counted).
    pub const MAX_DIFF_LIST: usize = 256;
    /// Frames one `resume` may run in real time before pausing itself
    /// (ten minutes at 60 Hz) — an agent that forgets to pause does not leave
    /// the machine running for ever.
    pub const MAX_REALTIME_FRAMES: u32 = 36_000;
    /// Bytes of ROM image the server will load.
    pub const MAX_ROM_BYTES: u64 = 16 * 1024 * 1024;
}

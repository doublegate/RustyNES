//! The declared tool catalogue: every tool's name, description, argument
//! type and JSON Schema, in one table.
//!
//! The schema an agent sees and the parser that reads its arguments are the
//! same Rust type (`Deserialize + JsonSchema`), so they cannot drift. Every
//! argument type is `deny_unknown_fields`: an invented option (`"space":
//! "prg"` on `write_ram`, say) is refused, not ignored.
//!
//! [`TOOL_NAMES`] is the authority for "declared". The dispatcher in
//! [`crate::session`] and [`crate::host`] handles exactly these names; a test
//! (`every_declared_tool_dispatches`) holds the two sets equal, so a tool cannot
//! be reachable without being declared, or declared without being reachable.

use std::sync::Arc;

use rmcp::model::{Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::input::PadsArg;

/// Every declared tool, in catalogue order.
pub const TOOL_NAMES: [&str; 25] = [
    "info",
    "pause",
    "resume",
    "set_input",
    "step_frames",
    "step_instructions",
    "breakpoint_add",
    "breakpoint_remove",
    "breakpoint_list",
    "read_memory",
    "write_ram",
    "cpu_state",
    "screenshot",
    "nametable",
    "oam",
    "observe",
    "reset",
    "lua_eval",
    "fork_create",
    "fork_run",
    "fork_diff",
    "fork_adopt",
    "fork_delete",
    "movie_export",
    "movie_verify",
];

/// No arguments. Still `deny_unknown_fields`, so `{"x": 1}` is refused.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

/// `resume`: run the main machine in real time.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResumeArgs {
    /// Pause again after this many frames (default and maximum 36000).
    pub max_frames: Option<u32>,
}

/// `step_frames`: run whole frames on the main machine.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepFramesArgs {
    /// Frames to run (1-3600).
    pub frames: u32,
    /// Buttons to hold for these frames. Omitted: keep the held input.
    pub input: Option<PadsArg>,
}

/// `step_instructions`: run single CPU instructions on the main machine.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepInstructionsArgs {
    /// Instructions to run (1-1000000).
    pub count: u32,
}

/// `breakpoint_add` / `breakpoint_remove`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BreakpointArgs {
    /// CPU address of the instruction (0-65535).
    pub pc: u16,
}

/// Which address space `read_memory` reads.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Space {
    /// The CPU bus, `$0000-$FFFF`, read without side effects.
    #[default]
    Cpu,
    /// The PPU bus, `$0000-$3FFF`, read without side effects.
    Ppu,
    /// Object attribute memory, 256 bytes.
    Oam,
}

/// `read_memory`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadMemoryArgs {
    /// Address space (default `cpu`).
    #[serde(default)]
    pub space: Space,
    /// First address.
    pub addr: u16,
    /// Bytes to read (1-4096).
    pub len: u32,
}

/// `write_ram`: write console work RAM. The only memory write there is.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WriteRamArgs {
    /// First address, in `$0000-$07FF`.
    pub addr: u16,
    /// Bytes to write; `addr + len` must not pass `$0800`.
    pub bytes: Vec<u8>,
}

/// `nametable`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NametableArgs {
    /// Nametable 0-3 (`$2000`, `$2400`, `$2800`, `$2C00`). Default 0.
    #[serde(default)]
    pub index: u8,
}

/// One observation layer.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    /// The frame as a PNG image.
    Frame,
    /// Nametable 0 as tile text.
    Nametable,
    /// OAM as a sprite table.
    Oam,
    /// The CPU registers.
    Cpu,
}

/// `observe`: several layers in one call.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObserveArgs {
    /// Layers to return (default: all four).
    pub layers: Option<Vec<Layer>>,
}

/// Reset kind.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResetKind {
    /// The console's reset button.
    #[default]
    Soft,
    /// Power off and on (battery RAM kept, as on a console).
    Power,
}

/// `reset`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResetArgs {
    /// `soft` (default) or `power`.
    #[serde(default)]
    pub kind: ResetKind,
}

/// `lua_eval`: run read-only Lua against the main machine's current state.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LuaEvalArgs {
    /// Lua 5.4 source. Top-level code runs at load; register
    /// `emu.onFrame(function() ... end)` to read the machine (it is called
    /// once). Writes, `setInput` and `load_state` are dropped.
    pub source: String,
}

/// `fork_create`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForkCreateArgs {
    /// `"main"` (default) or a fork id to branch from its current state.
    pub from: Option<String>,
}

/// One run of frames inside `fork_run`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InputSegment {
    /// Frames to hold this input for (at least 1).
    pub frames: u32,
    /// Player 1 buttons.
    #[serde(default)]
    pub p1: Vec<String>,
    /// Player 2 buttons.
    #[serde(default)]
    pub p2: Vec<String>,
    /// Player 3 buttons.
    #[serde(default)]
    pub p3: Vec<String>,
    /// Player 4 buttons.
    #[serde(default)]
    pub p4: Vec<String>,
}

/// `fork_run`: run controller input on a fork, recording it.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForkRunArgs {
    /// Fork id.
    pub fork: String,
    /// Input segments, played in order (at most 36000 frames per call).
    pub inputs: Vec<InputSegment>,
}

/// `fork_diff`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForkDiffArgs {
    /// `"main"` or a fork id.
    pub a: String,
    /// `"main"` or a fork id.
    pub b: String,
}

/// `fork_adopt` / `fork_delete`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForkArgs {
    /// Fork id.
    pub fork: String,
}

/// `movie_export` / `movie_verify`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MovieArgs {
    /// `"main"` (the journal's current segment) or a fork id.
    pub source: String,
    /// `movie_export` only: also write `<out-dir>/<name>.rnm`. Requires the
    /// server to have been started with `--out-dir`; `[A-Za-z0-9_-]`, 1-64
    /// characters.
    pub name: Option<String>,
}

fn schema<T: JsonSchema>() -> Arc<rmcp::model::JsonObject> {
    let value = serde_json::to_value(schemars::schema_for!(T)).unwrap_or_default();
    match value {
        serde_json::Value::Object(map) => Arc::new(map),
        _ => Arc::new(serde_json::Map::new()),
    }
}

fn tool<T: JsonSchema>(name: &'static str, description: &'static str, read_only: bool) -> Tool {
    let mut t = Tool::new(name, description, schema::<T>());
    t.annotations = Some(
        ToolAnnotations::new()
            .read_only(read_only)
            .destructive(false)
            .open_world(false),
    );
    t
}

/// The catalogue `tools/list` returns. Same order as [`TOOL_NAMES`].
#[must_use]
// One entry per tool; splitting the table would only scatter it.
#[allow(clippy::too_many_lines)]
pub fn catalogue() -> Vec<Tool> {
    vec![
        tool::<NoArgs>(
            "info",
            "ROM identity, region, mapper, frame counter, run state, held input, forks, journal and limits.",
            true,
        ),
        tool::<NoArgs>(
            "pause",
            "Stop real-time running. The machine is paused at start-up; stepping tools need it paused.",
            false,
        ),
        tool::<ResumeArgs>(
            "resume",
            "Run the main machine in real time with the held input until pause, a breakpoint, or max_frames.",
            false,
        ),
        tool::<PadsArg>(
            "set_input",
            "Set the buttons held on the main machine (used by step_frames without input and by real-time running).",
            false,
        ),
        tool::<StepFramesArgs>(
            "step_frames",
            "Run whole frames on the paused main machine; stops early at an exec breakpoint.",
            false,
        ),
        tool::<StepInstructionsArgs>(
            "step_instructions",
            "Run CPU instructions on the paused main machine.",
            false,
        ),
        tool::<BreakpointArgs>(
            "breakpoint_add",
            "Add an exec breakpoint at a CPU address (main machine).",
            false,
        ),
        tool::<BreakpointArgs>("breakpoint_remove", "Remove an exec breakpoint.", false),
        tool::<NoArgs>("breakpoint_list", "List exec breakpoints.", true),
        tool::<ReadMemoryArgs>(
            "read_memory",
            "Read cpu, ppu or oam memory without side effects (a $2002 read does not clear vblank).",
            true,
        ),
        tool::<WriteRamArgs>(
            "write_ram",
            "Write console work RAM ($0000-$07FF only). Starts a new journal segment.",
            false,
        ),
        tool::<NoArgs>(
            "cpu_state",
            "CPU registers, cycle and frame counters.",
            true,
        ),
        tool::<NoArgs>("screenshot", "The current frame as a 256x240 PNG.", true),
        tool::<NametableArgs>(
            "nametable",
            "A nametable as hex tile text, with its attribute table and the PPU scroll registers.",
            true,
        ),
        tool::<NoArgs>(
            "oam",
            "The 64 sprites: position, tile, palette, priority, flips.",
            true,
        ),
        tool::<ObserveArgs>(
            "observe",
            "Several observation layers at once: frame, nametable, oam, cpu.",
            true,
        ),
        tool::<ResetArgs>(
            "reset",
            "Soft reset or power cycle the main machine. Starts a new journal segment.",
            false,
        ),
        tool::<LuaEvalArgs>(
            "lua_eval",
            "Run sandboxed, read-only Lua 5.4 against the main machine (emu.onFrame callbacks run once). \
             Writes are dropped and the machine is checked unchanged afterwards.",
            true,
        ),
        tool::<ForkCreateArgs>(
            "fork_create",
            "Branch a new machine from main or a fork's current state. Returns a fork id.",
            false,
        ),
        tool::<ForkRunArgs>(
            "fork_run",
            "Run controller input on a fork, recording every frame into the fork's .rnm movie.",
            false,
        ),
        tool::<ForkDiffArgs>(
            "fork_diff",
            "Compare two machines: work RAM bytes, save RAM, framebuffer pixels, CPU registers, save state.",
            true,
        ),
        tool::<ForkArgs>(
            "fork_adopt",
            "Make the main machine continue from a fork's state. Starts a new journal segment.",
            false,
        ),
        tool::<ForkArgs>("fork_delete", "Discard a fork.", false),
        tool::<MovieArgs>(
            "movie_export",
            "A fork's movie (or main's current journal segment) as base64 .rnm; optionally written to --out-dir.",
            false,
        ),
        tool::<MovieArgs>(
            "movie_verify",
            "Replay a movie on a fresh machine from its recorded inputs and compare state byte for byte.",
            true,
        ),
    ]
}

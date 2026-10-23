//! The emulator session behind the tools: the main machine, its journal, the
//! forks, and the dispatcher that maps a tool call onto them.
//!
//! Everything here is synchronous and transport-free, so the whole tool
//! surface is testable without an MCP client: [`Session::call`] takes a tool
//! name and a JSON argument object and returns a [`ToolOutput`] or a
//! [`ToolError`]. [`crate::host`] wraps it in a lock, adds real-time running,
//! and puts it on the wire.
//!
//! # Two kinds of machine
//!
//! - **Main** is interactive. Any tool may change it, but every change that is
//!   not controller input (a RAM write, an instruction step, a stop at a
//!   breakpoint, a reset, adopting a fork) ends the current journal segment
//!   and starts a new one from a snapshot of that moment. The current segment
//!   is therefore always a movie that replays exactly.
//! - **A fork** accepts controller input and nothing else, so its movie is the
//!   whole experiment. It is built from the same ROM image and restored from a
//!   snapshot of its origin; replaying its movie on a third machine built the
//!   same way must reach the same bytes, and `movie_verify` checks exactly that.
//!
//! # Audio
//!
//! The server never plays sound, but the core buffers samples as it runs.
//! Each frame's samples are drained and dropped (`drop_audio`) so a long
//! session's buffer stays empty; audio is not part of any comparison here.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine as _;
use rustynes_core::{FrameInput, Movie, MoviePlayer, MovieRecorder, Nes, StartPoint};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::input::button_names;
use crate::limits;
use crate::observe;
use crate::tools::{
    BreakpointArgs, ForkArgs, ForkCreateArgs, ForkDiffArgs, ForkRunArgs, Layer, LuaEvalArgs,
    MovieArgs, NametableArgs, NoArgs, ObserveArgs, ReadMemoryArgs, ResetArgs, ResetKind, Space,
    StepFramesArgs, StepInstructionsArgs, WriteRamArgs,
};

/// What the operator gives the server at start-up.
#[derive(Clone, Debug, Default)]
pub struct SessionConfig {
    /// The iNES / NES 2.0 image (header included).
    pub rom: Vec<u8>,
    /// Where `movie_export` may write `<name>.rnm`. `None` refuses every
    /// file write.
    pub out_dir: Option<PathBuf>,
}

/// A tool's result: a JSON value (always), extra text blocks, and an image.
#[derive(Clone, Debug, Default)]
pub struct ToolOutput {
    /// The structured result, also rendered as the first text block.
    pub json: Value,
    /// Further text blocks (a nametable dump, a sprite table ...).
    pub texts: Vec<String>,
    /// A PNG image block.
    pub png: Option<Vec<u8>>,
}

impl ToolOutput {
    fn json(json: Value) -> Self {
        Self {
            json,
            ..Self::default()
        }
    }
}

/// Why a call did not run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolError {
    /// The name is not a declared tool. A protocol error on the wire.
    UnknownTool(String),
    /// The arguments did not parse (an unknown field, a wrong type, a value
    /// out of range).
    InvalidArgs(String),
    /// The call parsed but is not allowed (a write outside work RAM, a step
    /// while running, a file write with no `--out-dir`, a limit).
    Refused(String),
    /// The call was allowed and failed.
    Failed(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTool(n) => write!(f, "unknown tool {n:?}: not in the declared catalogue"),
            Self::InvalidArgs(m) => write!(f, "invalid arguments: {m}"),
            Self::Refused(m) => write!(f, "refused: {m}"),
            Self::Failed(m) => write!(f, "failed: {m}"),
        }
    }
}

impl std::error::Error for ToolError {}

type ToolResult = Result<ToolOutput, ToolError>;

/// The main machine's movie, restarted whenever something other than input
/// changes the machine.
struct Journal {
    rec: MovieRecorder,
    /// 1-based segment number.
    segment: u32,
    /// Why the current segment began.
    reason: String,
}

impl Journal {
    fn new(nes: &Nes) -> Self {
        Self {
            rec: MovieRecorder::from_current_state(nes),
            segment: 1,
            reason: "session start (power-on)".to_owned(),
        }
    }

    fn restart(&mut self, nes: &Nes, reason: impl Into<String>) {
        self.rec = MovieRecorder::from_current_state(nes);
        self.segment += 1;
        self.reason = reason.into();
    }
}

/// One experiment branch.
struct Fork {
    nes: Nes,
    rec: MovieRecorder,
    origin: String,
}

/// Drop the samples the core buffered for the last frame (see the module
/// docs). `drain_audio_into` KEEPS whatever does not fit for the next drain,
/// so this loops until a drain comes back short; the fixed sink keeps it
/// allocation-free. Until v3.5.0's review this drained into a one-sample
/// buffer on the belief that the remainder was discarded, which removed one
/// sample per frame and let a session's buffer grow by about 800 samples a
/// frame (`Copilot` on #624, against the speedrun runner's same omission).
fn drop_audio(nes: &mut Nes) {
    let mut sink = [0.0f32; 1024];
    while nes.drain_audio_into(&mut sink) == sink.len() {}
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn parse<T: DeserializeOwned>(
    args: Option<&serde_json::Map<String, Value>>,
) -> Result<T, ToolError> {
    let v = Value::Object(args.cloned().unwrap_or_default());
    serde_json::from_value(v).map_err(|e| ToolError::InvalidArgs(e.to_string()))
}

/// Everything a comparison needs from one machine.
struct MachineDigest {
    snapshot: Vec<u8>,
    wram: Vec<u8>,
    sram: Vec<u8>,
    framebuffer: Vec<u8>,
    cpu: [u16; 6],
    frame: u64,
}

impl MachineDigest {
    fn of(nes: &Nes) -> Self {
        let c = nes.cpu_snapshot();
        Self {
            snapshot: nes.snapshot(),
            wram: nes.wram().to_vec(),
            sram: nes.sram().to_vec(),
            framebuffer: nes.framebuffer().to_vec(),
            cpu: [
                c.pc,
                u16::from(c.a),
                u16::from(c.x),
                u16::from(c.y),
                u16::from(c.s),
                u16::from(c.p),
            ],
            frame: nes.frame(),
        }
    }

    fn summary(&self) -> Value {
        json!({
            "frame": self.frame,
            "snapshot_sha256": sha256_hex(&self.snapshot),
            "wram_sha256": sha256_hex(&self.wram),
            "framebuffer_sha256": sha256_hex(&self.framebuffer),
        })
    }
}

/// The emulator session. See the module docs.
pub struct Session {
    rom: Arc<[u8]>,
    nes: Nes,
    held: FrameInput,
    journal: Journal,
    forks: BTreeMap<String, Fork>,
    next_fork: u32,
    out_dir: Option<PathBuf>,
    running: bool,
    /// Whether agent Lua runs with its writes locked. Always `true` outside
    /// this module's unit tests, which clear it to prove the post-script
    /// state check (the second layer) catches a write the first layer let
    /// through. No tool or argument reaches it.
    lua_writes_locked: bool,
}

impl Session {
    /// Load the ROM and power on.
    ///
    /// # Errors
    ///
    /// A ROM the core refuses.
    pub fn new(config: SessionConfig) -> Result<Self, String> {
        let rom: Arc<[u8]> = Arc::from(config.rom);
        let nes = Nes::from_rom(&rom).map_err(|e| format!("ROM not loaded: {e}"))?;
        let journal = Journal::new(&nes);
        Ok(Self {
            rom,
            nes,
            held: FrameInput::default(),
            journal,
            forks: BTreeMap::new(),
            next_fork: 1,
            out_dir: config.out_dir,
            running: false,
            lua_writes_locked: true,
        })
    }

    /// The main machine (read-only; tests and the host's status line).
    #[must_use]
    pub const fn nes(&self) -> &Nes {
        &self.nes
    }

    /// Set by [`crate::host::Host`] while real-time running.
    pub(crate) const fn set_running(&mut self, running: bool) {
        self.running = running;
    }

    /// Whether the main machine is running in real time.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.running
    }

    /// One frame of the main machine with the held input, recorded in the
    /// journal. Returns the PC of a breakpoint that stopped it.
    pub(crate) fn run_main_frame(&mut self) -> Option<u16> {
        let input = self.held;
        input.apply_to(&mut self.nes);
        self.journal.rec.capture_input(input);
        self.nes.run_frame();
        drop_audio(&mut self.nes);
        if let Some(pc) = self.nes.take_break_hit() {
            // The frame just captured did not finish, and a replay with no
            // breakpoint would finish it: the record restarts here.
            self.journal
                .restart(&self.nes, format!("stopped at breakpoint ${pc:04X}"));
            return Some(pc);
        }
        if self.journal.rec.len() >= limits::MAX_JOURNAL_FRAMES {
            self.journal
                .restart(&self.nes, "journal segment reached its frame limit");
        }
        None
    }

    /// Dispatch one tool call. `pause` and `resume` belong to the host (they
    /// need its runner thread) and are answered here only as a refusal.
    ///
    /// # Errors
    ///
    /// See [`ToolError`].
    // A flat dispatch table: one arm per declared tool reads more plainly
    // than the same arms split across helpers.
    #[allow(clippy::too_many_lines)]
    pub fn call(
        &mut self,
        name: &str,
        args: Option<&serde_json::Map<String, Value>>,
    ) -> ToolResult {
        match name {
            "info" => {
                parse::<NoArgs>(args)?;
                Ok(self.info())
            }
            "pause" | "resume" => Err(ToolError::Refused(format!(
                "{name} needs the server host; this session has no runner"
            ))),
            "set_input" => {
                let pads: crate::input::PadsArg = parse(args)?;
                self.held = pads.to_frame_input().map_err(ToolError::InvalidArgs)?;
                Ok(ToolOutput::json(json!({ "held": held_json(self.held) })))
            }
            "step_frames" => self.step_frames(&parse(args)?),
            "step_instructions" => self.step_instructions(&parse(args)?),
            "breakpoint_add" => {
                let a: BreakpointArgs = parse(args)?;
                if !self.nes.breakpoints().contains(&a.pc)
                    && self.nes.breakpoints().len() >= limits::MAX_BREAKPOINTS
                {
                    return Err(ToolError::Refused(format!(
                        "at most {} breakpoints",
                        limits::MAX_BREAKPOINTS
                    )));
                }
                self.nes.add_breakpoint(a.pc);
                Ok(self.breakpoint_list())
            }
            "breakpoint_remove" => {
                let a: BreakpointArgs = parse(args)?;
                self.nes.remove_breakpoint(a.pc);
                Ok(self.breakpoint_list())
            }
            "breakpoint_list" => {
                parse::<NoArgs>(args)?;
                Ok(self.breakpoint_list())
            }
            "read_memory" => self.read_memory(&parse(args)?),
            "write_ram" => self.write_ram(&parse(args)?),
            "cpu_state" => {
                parse::<NoArgs>(args)?;
                Ok(self.cpu_state())
            }
            "screenshot" => {
                parse::<NoArgs>(args)?;
                let png = observe::png_rgba(self.nes.framebuffer()).map_err(ToolError::Failed)?;
                Ok(ToolOutput {
                    json: json!({ "frame": self.nes.frame(), "width": 256, "height": 240 }),
                    texts: Vec::new(),
                    png: Some(png),
                })
            }
            "nametable" => {
                let a: NametableArgs = parse(args)?;
                if a.index > 3 {
                    return Err(ToolError::InvalidArgs("index is 0-3".into()));
                }
                Ok(ToolOutput {
                    json: json!({ "index": a.index, "frame": self.nes.frame() }),
                    texts: vec![observe::nametable_text(&mut self.nes, a.index)],
                    png: None,
                })
            }
            "oam" => {
                parse::<NoArgs>(args)?;
                Ok(ToolOutput {
                    json: json!({ "frame": self.nes.frame() }),
                    texts: vec![observe::oam_text(&self.nes)],
                    png: None,
                })
            }
            "observe" => self.observe(&parse(args)?),
            "reset" => {
                let a: ResetArgs = parse(args)?;
                self.refuse_while_running("reset")?;
                match a.kind {
                    ResetKind::Soft => self.nes.reset(),
                    ResetKind::Power => self.nes.power_cycle(),
                }
                let why = format!("{:?} reset", a.kind).to_lowercase();
                self.journal.restart(&self.nes, why);
                Ok(self.cpu_state())
            }
            "lua_eval" => self.lua_eval(&parse(args)?),
            "fork_create" => self.fork_create(&parse(args)?),
            "fork_run" => self.fork_run(&parse(args)?),
            "fork_diff" => self.fork_diff(&parse(args)?),
            "fork_adopt" => {
                let a: ForkArgs = parse(args)?;
                self.refuse_while_running("fork_adopt")?;
                let snap = self.fork(&a.fork)?.nes.snapshot();
                self.nes
                    .restore(&snap)
                    .map_err(|e| ToolError::Failed(e.to_string()))?;
                self.journal
                    .restart(&self.nes, format!("adopted fork {}", a.fork));
                Ok(ToolOutput::json(json!({
                    "adopted": a.fork,
                    "main": MachineDigest::of(&self.nes).summary(),
                })))
            }
            "fork_delete" => {
                let a: ForkArgs = parse(args)?;
                self.forks
                    .remove(&a.fork)
                    .map(|_| ToolOutput::json(json!({ "deleted": a.fork })))
                    .ok_or_else(|| ToolError::InvalidArgs(format!("no fork {:?}", a.fork)))
            }
            "movie_export" => self.movie_export(&parse(args)?),
            "movie_verify" => self.movie_verify(&parse(args)?),
            other => Err(ToolError::UnknownTool(other.to_owned())),
        }
    }

    fn refuse_while_running(&self, what: &str) -> Result<(), ToolError> {
        if self.running {
            Err(ToolError::Refused(format!(
                "{what} needs the machine paused; call pause first"
            )))
        } else {
            Ok(())
        }
    }

    fn info(&self) -> ToolOutput {
        let forks: Vec<Value> = self
            .forks
            .iter()
            .map(|(id, f)| {
                json!({ "id": id, "origin": f.origin, "frames": f.rec.len(), "frame": f.nes.frame() })
            })
            .collect();
        ToolOutput::json(json!({
            "server": concat!("rustynes-mcp ", env!("CARGO_PKG_VERSION")),
            "emulation_epoch": rustynes_core::EMULATION_EPOCH,
            "rom_sha256": hex::encode(self.nes.rom_sha256()),
            "region": format!("{:?}", self.nes.region()),
            "mapper": self.nes.mapper_id(),
            "submapper": self.nes.submapper(),
            "frame": self.nes.frame(),
            "running": self.running,
            "held": held_json(self.held),
            "breakpoints": self.nes.breakpoints(),
            "journal": {
                "segment": self.journal.segment,
                "frames": self.journal.rec.len(),
                "began": self.journal.reason,
            },
            "forks": forks,
            "out_dir": self.out_dir.is_some(),
            "limits": {
                "step_frames": limits::MAX_STEP_FRAMES,
                "step_instructions": limits::MAX_STEP_INSTRUCTIONS,
                "fork_run_frames": limits::MAX_FORK_RUN_FRAMES,
                "fork_frames": limits::MAX_FORK_FRAMES,
                "forks": limits::MAX_FORKS,
                "read_len": limits::MAX_READ_LEN,
                "lua_source": limits::MAX_LUA_SOURCE,
                "realtime_frames": limits::MAX_REALTIME_FRAMES,
            },
        }))
    }

    fn cpu_state(&self) -> ToolOutput {
        let c = self.nes.cpu_snapshot();
        ToolOutput::json(json!({
            "pc": c.pc, "a": c.a, "x": c.x, "y": c.y, "s": c.s, "p": c.p,
            "cycles": c.cycles, "jammed": c.jammed, "frame": self.nes.frame(),
            "text": observe::cpu_text(&self.nes),
        }))
    }

    fn breakpoint_list(&self) -> ToolOutput {
        let list: Vec<String> = self
            .nes
            .breakpoints()
            .iter()
            .map(|pc| format!("${pc:04X}"))
            .collect();
        ToolOutput::json(json!({ "breakpoints": list }))
    }

    fn step_frames(&mut self, a: &StepFramesArgs) -> ToolResult {
        self.refuse_while_running("step_frames")?;
        if a.frames == 0 || a.frames > limits::MAX_STEP_FRAMES {
            return Err(ToolError::InvalidArgs(format!(
                "frames is 1-{}",
                limits::MAX_STEP_FRAMES
            )));
        }
        if let Some(pads) = &a.input {
            self.held = pads.to_frame_input().map_err(ToolError::InvalidArgs)?;
        }
        let mut ran = 0u32;
        let mut break_hit = None;
        for _ in 0..a.frames {
            ran += 1;
            if let Some(pc) = self.run_main_frame() {
                break_hit = Some(pc);
                break;
            }
            if self.nes.cpu().is_jammed() {
                break;
            }
        }
        Ok(ToolOutput::json(json!({
            "frames_run": ran,
            "break_hit": break_hit.map(|pc| format!("${pc:04X}")),
            "jammed": self.nes.cpu().is_jammed(),
            "main": MachineDigest::of(&self.nes).summary(),
        })))
    }

    fn step_instructions(&mut self, a: &StepInstructionsArgs) -> ToolResult {
        self.refuse_while_running("step_instructions")?;
        if a.count == 0 || a.count > limits::MAX_STEP_INSTRUCTIONS {
            return Err(ToolError::InvalidArgs(format!(
                "count is 1-{}",
                limits::MAX_STEP_INSTRUCTIONS
            )));
        }
        let mut cycles = 0u64;
        let mut ran = 0u32;
        for _ in 0..a.count {
            if self.nes.cpu().is_jammed() {
                break;
            }
            cycles += u64::from(self.nes.step_instruction());
            ran += 1;
        }
        drop_audio(&mut self.nes);
        // A replay runs whole frames, so a stop mid-frame begins a new record.
        self.journal.restart(&self.nes, "instruction step");
        let mut out = self.cpu_state();
        out.json["instructions_run"] = json!(ran);
        out.json["cpu_cycles"] = json!(cycles);
        Ok(out)
    }

    fn read_memory(&mut self, a: &ReadMemoryArgs) -> ToolResult {
        if a.len == 0 || a.len > limits::MAX_READ_LEN {
            return Err(ToolError::InvalidArgs(format!(
                "len is 1-{}",
                limits::MAX_READ_LEN
            )));
        }
        let oam = self.nes.oam();
        let bytes: Vec<u8> = (0..a.len)
            .map(|i| {
                #[allow(clippy::cast_possible_truncation)] // len <= 4096
                let addr = a.addr.wrapping_add(i as u16);
                match a.space {
                    Space::Cpu => self.nes.cpu_bus_peek(addr),
                    Space::Ppu => self.nes.ppu_bus_peek(addr & 0x3FFF),
                    Space::Oam => oam[usize::from(addr & 0xFF)],
                }
            })
            .collect();
        let mut dump = String::with_capacity(bytes.len() * 3 + bytes.len() / 16 * 8);
        for (row, chunk) in bytes.chunks(16).enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let addr = a.addr.wrapping_add((row * 16) as u16);
            let _ = write!(dump, "{addr:04X}:");
            for b in chunk {
                let _ = write!(dump, " {b:02X}");
            }
            dump.push('\n');
        }
        Ok(ToolOutput {
            json: json!({
                "space": format!("{:?}", a.space).to_lowercase(),
                "addr": a.addr,
                "hex": hex::encode(&bytes),
            }),
            texts: vec![dump],
            png: None,
        })
    }

    fn write_ram(&mut self, a: &WriteRamArgs) -> ToolResult {
        // A write restarts the journal, so during a real-time run it would cut
        // the run's recording mid-way; like every other timeline change it
        // needs the machine paused (Antigravity on #624, round 3).
        self.refuse_while_running("write_ram")?;
        let end = usize::from(a.addr) + a.bytes.len();
        if a.bytes.is_empty() || end > 0x800 {
            return Err(ToolError::Refused(format!(
                "write_ram writes console work RAM $0000-$07FF only; ${:04X}+{} is outside it",
                a.addr,
                a.bytes.len()
            )));
        }
        for (i, b) in a.bytes.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)] // end <= 0x800
            self.nes.poke_ram(a.addr + i as u16, *b);
        }
        self.journal.restart(
            &self.nes,
            format!("write_ram ${:04X}+{}", a.addr, a.bytes.len()),
        );
        Ok(ToolOutput::json(json!({
            "written": a.bytes.len(),
            "journal_segment": self.journal.segment,
        })))
    }

    fn observe(&mut self, a: &ObserveArgs) -> ToolResult {
        let layers = a
            .layers
            .clone()
            .unwrap_or_else(|| vec![Layer::Frame, Layer::Nametable, Layer::Oam, Layer::Cpu]);
        let mut out = ToolOutput::json(json!({
            "frame": self.nes.frame(),
            "layers": layers.iter().map(|l| format!("{l:?}").to_lowercase()).collect::<Vec<_>>(),
        }));
        for layer in layers {
            match layer {
                Layer::Frame => {
                    out.png =
                        Some(observe::png_rgba(self.nes.framebuffer()).map_err(ToolError::Failed)?);
                }
                Layer::Nametable => out.texts.push(observe::nametable_text(&mut self.nes, 0)),
                Layer::Oam => out.texts.push(observe::oam_text(&self.nes)),
                Layer::Cpu => out.texts.push(observe::cpu_text(&self.nes)),
            }
        }
        Ok(out)
    }

    fn lua_eval(&mut self, a: &LuaEvalArgs) -> ToolResult {
        if a.source.len() > limits::MAX_LUA_SOURCE {
            return Err(ToolError::Refused(format!(
                "Lua source is limited to {} bytes",
                limits::MAX_LUA_SOURCE
            )));
        }
        let before = self.nes.snapshot();
        let mut engine = rustynes_script::ScriptEngine::new()
            .map_err(|e| ToolError::Failed(format!("Lua sandbox: {e}")))?;
        // Agent Lua is untrusted: every write path the engine has is closed.
        engine.set_writes_locked(self.lua_writes_locked);
        engine.set_instruction_budget(rustynes_script::DEFAULT_INSTRUCTION_BUDGET);
        let run = engine
            .load(&a.source)
            .and_then(|()| engine.on_frame(&mut self.nes));
        let mut log = String::new();
        let mut dropped_lines = 0usize;
        for line in engine.drain_log() {
            if log.len() + line.len() + 1 > limits::MAX_LUA_LOG {
                dropped_lines += 1;
            } else {
                log.push_str(&line);
                log.push('\n');
            }
        }
        let controls = engine.drain_controls().len();
        let draws = engine.drain_draws().len();
        drop(engine);
        // Defence in depth: the sandbox is meant to make this impossible. If
        // the machine changed anyway, put it back and say so rather than leave
        // an unrecorded write on the timeline.
        let breach = self.nes.snapshot() != before;
        if breach {
            self.nes
                .restore(&before)
                .map_err(|e| ToolError::Failed(format!("restoring after a Lua breach: {e}")))?;
        }
        let json = json!({
            "ok": run.is_ok() && !breach,
            "error": run.err().map(|e| e.to_string()),
            "state_unchanged": !breach,
            "refused_control_requests": controls,
            "ignored_draw_commands": draws,
            "dropped_log_lines": dropped_lines,
        });
        if breach {
            return Err(ToolError::Refused(format!(
                "the script changed the machine despite the write lock; it was restored. {json}"
            )));
        }
        Ok(ToolOutput {
            json,
            texts: if log.is_empty() {
                Vec::new()
            } else {
                vec![log]
            },
            png: None,
        })
    }

    fn fork(&self, id: &str) -> Result<&Fork, ToolError> {
        self.forks
            .get(id)
            .ok_or_else(|| ToolError::InvalidArgs(format!("no fork {id:?}")))
    }

    /// The machine a source id names (`"main"` or a fork id).
    fn machine(&self, id: &str) -> Result<&Nes, ToolError> {
        if id == "main" {
            Ok(&self.nes)
        } else {
            self.fork(id).map(|f| &f.nes)
        }
    }

    fn fork_create(&mut self, a: &ForkCreateArgs) -> ToolResult {
        if self.forks.len() >= limits::MAX_FORKS {
            return Err(ToolError::Refused(format!(
                "at most {} forks; delete one first",
                limits::MAX_FORKS
            )));
        }
        let origin = a.from.clone().unwrap_or_else(|| "main".to_owned());
        let snap = self.machine(&origin)?.snapshot();
        let mut nes =
            Nes::from_rom(&self.rom).map_err(|e| ToolError::Failed(format!("fork: {e}")))?;
        nes.restore(&snap)
            .map_err(|e| ToolError::Failed(format!("fork restore: {e}")))?;
        let rec = MovieRecorder::from_current_state(&nes);
        let id = format!("f{}", self.next_fork);
        self.next_fork += 1;
        let summary = MachineDigest::of(&nes).summary();
        self.forks.insert(
            id.clone(),
            Fork {
                nes,
                rec,
                origin: origin.clone(),
            },
        );
        Ok(ToolOutput::json(
            json!({ "fork": id, "from": origin, "state": summary }),
        ))
    }

    fn fork_run(&mut self, a: &ForkRunArgs) -> ToolResult {
        let total: u64 = a.inputs.iter().map(|s| u64::from(s.frames)).sum();
        if a.inputs.is_empty() || a.inputs.iter().any(|s| s.frames == 0) {
            return Err(ToolError::InvalidArgs(
                "inputs needs at least one segment, each of at least one frame".into(),
            ));
        }
        if total > u64::from(limits::MAX_FORK_RUN_FRAMES) {
            return Err(ToolError::Refused(format!(
                "{total} frames in one call; the limit is {}",
                limits::MAX_FORK_RUN_FRAMES
            )));
        }
        let plan: Vec<(u32, FrameInput)> = a
            .inputs
            .iter()
            .map(|s| {
                let pads = crate::input::PadsArg {
                    p1: s.p1.clone(),
                    p2: s.p2.clone(),
                    p3: s.p3.clone(),
                    p4: s.p4.clone(),
                };
                pads.to_frame_input().map(|fi| (s.frames, fi))
            })
            .collect::<Result<_, _>>()
            .map_err(ToolError::InvalidArgs)?;
        let fork = self
            .forks
            .get_mut(&a.fork)
            .ok_or_else(|| ToolError::InvalidArgs(format!("no fork {:?}", a.fork)))?;
        #[allow(clippy::cast_possible_truncation)] // total <= 36,000
        if fork.rec.len() + total as usize > limits::MAX_FORK_FRAMES {
            return Err(ToolError::Refused(format!(
                "the fork's movie would pass {} frames; branch a new fork from it",
                limits::MAX_FORK_FRAMES
            )));
        }
        for (frames, input) in plan {
            for _ in 0..frames {
                input.apply_to(&mut fork.nes);
                fork.rec.capture_input(input);
                fork.nes.run_frame();
                drop_audio(&mut fork.nes);
            }
        }
        Ok(ToolOutput::json(json!({
            "fork": a.fork,
            "frames_run": total,
            "movie_frames": fork.rec.len(),
            "jammed": fork.nes.cpu().is_jammed(),
            "state": MachineDigest::of(&fork.nes).summary(),
        })))
    }

    fn fork_diff(&self, a: &ForkDiffArgs) -> ToolResult {
        let da = MachineDigest::of(self.machine(&a.a)?);
        let db = MachineDigest::of(self.machine(&a.b)?);
        let mut wram = Vec::new();
        let mut wram_count = 0usize;
        for (i, (x, y)) in da.wram.iter().zip(&db.wram).enumerate() {
            if x != y {
                wram_count += 1;
                if wram.len() < limits::MAX_DIFF_LIST {
                    wram.push(json!({ "addr": format!("${i:04X}"), "a": x, "b": y }));
                }
            }
        }
        let sram_count = da.sram.iter().zip(&db.sram).filter(|(x, y)| x != y).count()
            + da.sram.len().abs_diff(db.sram.len());
        let pixels = da
            .framebuffer
            .as_chunks::<4>()
            .0
            .iter()
            .zip(db.framebuffer.as_chunks::<4>().0)
            .filter(|(x, y)| x != y)
            .count();
        let names = ["pc", "a", "x", "y", "s", "p"];
        let cpu: Vec<Value> = names
            .iter()
            .zip(da.cpu.iter().zip(&db.cpu))
            .filter(|(_, (x, y))| x != y)
            .map(|(n, (x, y))| json!({ "reg": n, "a": x, "b": y }))
            .collect();
        Ok(ToolOutput::json(json!({
            "a": a.a,
            "b": a.b,
            "identical_state": da.snapshot == db.snapshot,
            "frames": { "a": da.frame, "b": db.frame },
            "wram_bytes_differing": wram_count,
            "wram_differences": wram,
            "wram_differences_listed": wram.len(),
            "sram_bytes_differing": sram_count,
            "framebuffer_pixels_differing": pixels,
            "cpu_differences": cpu,
        })))
    }

    /// The movie a source id names: a fork's whole record, or main's current
    /// journal segment.
    fn movie_of(&self, source: &str) -> Result<Movie, ToolError> {
        if source == "main" {
            Ok(self.journal.rec.clone().finish())
        } else {
            Ok(self.fork(source)?.rec.clone().finish())
        }
    }

    fn movie_export(&self, a: &MovieArgs) -> ToolResult {
        let movie = self.movie_of(&a.source)?;
        let bytes = movie.serialize();
        let written = if let Some(name) = &a.name {
            let valid = !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
            if !valid {
                return Err(ToolError::Refused(format!(
                    "export name {name:?}: use 1-64 characters of [A-Za-z0-9_-]"
                )));
            }
            let Some(dir) = &self.out_dir else {
                return Err(ToolError::Refused(
                    "no --out-dir was given at start-up, so the server writes no files".into(),
                ));
            };
            let path = dir.join(format!("{name}.rnm"));
            // `create_new`: an export never overwrites an existing file.
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| ToolError::Refused(format!("{}: {e}", path.display())))?;
            std::io::Write::write_all(&mut f, &bytes)
                .map_err(|e| ToolError::Failed(format!("{}: {e}", path.display())))?;
            Some(path.display().to_string())
        } else {
            None
        };
        let start = match &movie.start {
            StartPoint::PowerOn => "power-on",
            StartPoint::SaveState(_) => "save state",
        };
        Ok(ToolOutput::json(json!({
            "source": a.source,
            "frames": movie.len(),
            "start": start,
            "format_version": rustynes_core::MOVIE_FORMAT_VERSION,
            "bytes": bytes.len(),
            "sha256": sha256_hex(&bytes),
            "written": written,
            "rnm_base64": base64::engine::general_purpose::STANDARD.encode(&bytes),
        })))
    }

    fn movie_verify(&self, a: &MovieArgs) -> ToolResult {
        if a.name.is_some() {
            return Err(ToolError::InvalidArgs("movie_verify takes no name".into()));
        }
        let live = MachineDigest::of(self.machine(&a.source)?);
        let bytes = self.movie_of(&a.source)?.serialize();
        let replayed = replay_rnm(&self.rom, &bytes).map_err(ToolError::Failed)?;
        let replay = MachineDigest::of(&replayed);
        let checks = [
            ("save_state", live.snapshot == replay.snapshot),
            ("work_ram", live.wram == replay.wram),
            ("save_ram", live.sram == replay.sram),
            ("framebuffer", live.framebuffer == replay.framebuffer),
            ("frame_counter", live.frame == replay.frame),
        ];
        let mismatches: Vec<&str> = checks
            .iter()
            .filter(|(_, ok)| !ok)
            .map(|(n, _)| *n)
            .collect();
        Ok(ToolOutput::json(json!({
            "source": a.source,
            "identical": mismatches.is_empty(),
            "mismatches": mismatches,
            "movie_sha256": sha256_hex(&bytes),
            "live": live.summary(),
            "replayed": replay.summary(),
        })))
    }
}

/// Replay a serialised `.rnm` on a fresh machine built from `rom`, and return
/// that machine. This is the reproducibility check's other half: it sees only
/// the ROM and the movie's bytes, never the live machine.
///
/// # Errors
///
/// A movie or ROM the core refuses.
pub fn replay_rnm(rom: &[u8], rnm: &[u8]) -> Result<Nes, String> {
    let movie = Movie::deserialize(rnm).map_err(|e| format!("movie: {e}"))?;
    let mut nes = Nes::from_rom(rom).map_err(|e| format!("ROM: {e}"))?;
    movie
        .seek_to_start(&mut nes)
        .map_err(|e| format!("seek: {e}"))?;
    let mut player = MoviePlayer::new(&movie);
    while player.apply_next(&mut nes) {
        nes.run_frame();
        drop_audio(&mut nes);
    }
    Ok(nes)
}

fn held_json(f: FrameInput) -> Value {
    json!({
        "p1": button_names(f.p1),
        "p2": button_names(f.p2),
        "p3": button_names(f.p3),
        "p4": button_names(f.p4),
    })
}

// The integration tests' hand-assembled fixture ROM, shared by path.
#[cfg(test)]
#[path = "../tests/common/rom.rs"]
mod fixture_rom;

#[cfg(test)]
mod tests {
    use super::*;

    use super::fixture_rom as rom;

    fn session() -> Session {
        Session::new(SessionConfig {
            rom: rom::input_rom(),
            out_dir: None,
        })
        .expect("fixture loads")
    }

    fn args(v: Value) -> serde_json::Map<String, Value> {
        match v {
            Value::Object(m) => m,
            _ => serde_json::Map::new(),
        }
    }

    /// A session's audio buffer stays empty however long it runs: every
    /// stepped frame's samples are dropped in full. Mutation-checked by
    /// draining into a one-sample sink once (the pre-fix `drop_audio`),
    /// which leaves a whole frame's samples behind.
    #[test]
    fn stepping_leaves_no_audio_buffered() {
        let mut s = session();
        s.call("step_frames", Some(&args(json!({ "frames": 120 }))))
            .expect("steps");
        let left = s.nes.drain_audio().len();
        assert_eq!(left, 0, "{left} samples stayed buffered after 120 frames");
    }

    /// The second layer: if the sandbox's write lock ever failed, the
    /// post-script comparison restores the machine and refuses the call.
    /// Only reachable here, with the first layer deliberately opened.
    #[test]
    fn a_lua_write_past_the_lock_is_undone_and_refused() {
        let mut s = session();
        s.call("step_frames", Some(&args(json!({ "frames": 3 }))))
            .expect("steps");
        let before = s.nes.snapshot();
        s.lua_writes_locked = false;
        let script = "emu.onFrame(function() memory:poke(0x12, 0x99) end)";
        let r = s.call("lua_eval", Some(&args(json!({ "source": script }))));
        assert!(
            matches!(&r, Err(ToolError::Refused(m)) if m.contains("despite the write lock")),
            "{r:?}"
        );
        assert_eq!(s.nes.snapshot(), before, "the breach was not undone");
    }
}

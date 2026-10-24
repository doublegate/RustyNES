// SPDX-License-Identifier: GPL-3.0-or-later
//! The Debug Adapter Protocol server.
//!
//! Written from the Debug Adapter Protocol specification
//! (microsoft.github.io/debug-adapter-protocol): base protocol framing
//! (`Content-Length` header, blank line, JSON body), the request / response /
//! event message shapes, and the requests below. An editor launches the
//! adapter as a child process and speaks to it on stdin / stdout, so there is
//! no listener to secure; stderr carries diagnostics.
//!
//! | request | what it does here |
//! | --- | --- |
//! | `initialize` | capabilities (configuration done, function / instruction breakpoints, read memory, hover evaluation, instruction stepping) |
//! | `launch` | `program` (an iNES / NES 2.0 image), `debugInfo` (a cc65 `.dbg` or an llvm-mos ELF; default: `<program>.elf`, then `<stem>.dbg`), `sourceRoot`, `stopOnEntry`; then the `initialized` event |
//! | `setBreakpoints` / `setFunctionBreakpoints` / `setInstructionBreakpoints` | arm the core's exec breakpoints at a line's statement starts / a function's entry / an address |
//! | `configurationDone` | start running (or stop at entry) |
//! | `threads`, `stackTrace`, `scopes`, `variables` | one thread (the 6502); the shadow call stack; Locals, Globals, Registers |
//! | `continue`, `next`, `stepIn`, `stepOut`, `pause` | run, line steps (instruction steps with `granularity: "instruction"`), pause |
//! | `evaluate` | a variable name, a register name, or `$XXXX` / `0xXXXX` (one byte) |
//! | `readMemory` | side-effect-free CPU-bus bytes, base64 |
//! | `disconnect` / `terminate` | end the session |
//!
//! While the machine runs, the server keeps reading: the engine's run loop
//! polls the request channel once per emulated frame (and per hook), so
//! `pause`, `setBreakpoints` and `disconnect` take effect within a frame.
//! A request that needs a stopped machine (`stackTrace`, `variables`, ...)
//! is answered with an error while it runs, as the specification allows.
//!
//! Nothing a client sends can write emulated memory or registers: there is
//! no `setVariable`, `setExpression`, `writeMemory` or `restart`. That keeps
//! a debugging session a pure observer of the program, on the determinism
//! contract (ADR 0049).

use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use base64::Engine as _;
use rustynes_core::Nes;
use serde_json::{Map, Value, json};

use crate::debuginfo::{DebugInfo, MAX_DEBUG_FILE_BYTES};
use crate::engine::{Engine, FrameView, StepKind, Stop};

/// The largest DAP message body accepted.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// The largest ROM accepted.
pub const MAX_ROM_BYTES: u64 = 16 * 1024 * 1024;
/// The one thread id.
const THREAD: i64 = 1;
/// `variablesReference` of the Globals scope.
const REF_GLOBALS: i64 = 1;
/// `variablesReference` of the Registers scope.
const REF_REGISTERS: i64 = 2;
/// `variablesReference` of frame `n`'s Locals is `REF_LOCALS + n`.
const REF_LOCALS: i64 = 1000;

/// Read one DAP message: headers up to a blank line, then exactly
/// `Content-Length` bytes of JSON. `Ok(None)` at a clean end of input.
///
/// # Errors
///
/// An I/O error, a missing or oversized `Content-Length`, or a body that is
/// not JSON.
pub fn read_message(r: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut len: Option<usize> = None;
    let mut saw_header = false;
    loop {
        let mut line = String::new();
        let mut limited = r.by_ref().take(1024);
        if limited.read_line(&mut line)? == 0 {
            return if saw_header {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "end of input inside a header",
                ))
            } else {
                Ok(None)
            };
        }
        saw_header = true;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':')
            && k.trim().eq_ignore_ascii_case("content-length")
        {
            len =
                Some(v.trim().parse().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "bad Content-Length")
                })?);
        }
    }
    let len = len.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no Content-Length"))?;
    if len > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("message of {len} bytes exceeds {MAX_MESSAGE_BYTES}"),
        ));
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Write one DAP message.
///
/// # Errors
///
/// An I/O error.
pub fn write_message(w: &mut impl Write, msg: &Value) -> io::Result<()> {
    let body = msg.to_string();
    write!(w, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    w.flush()
}

/// Defaults the command line supplies for `launch` (a client's own
/// `launch` arguments override them).
#[derive(Clone, Debug, Default)]
pub struct Defaults {
    /// `--rom`.
    pub program: Option<PathBuf>,
    /// `--debug-info`.
    pub debug_info: Option<PathBuf>,
}

enum Incoming {
    Message(Value),
    Bad(String),
    Eof,
}

/// What the machine is doing between requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Stopped,
    Continue,
    Line(StepKind),
    Instruction,
}

/// Serve one debug session on `input` / `output` until the client
/// disconnects or the input ends.
///
/// # Errors
///
/// An I/O error writing to `output`.
pub fn serve<R: Read + Send + 'static>(
    input: R,
    output: &mut impl Write,
    defaults: Defaults,
) -> io::Result<()> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("dap-reader".into())
        .spawn(move || {
            let mut r = BufReader::new(input);
            loop {
                match read_message(&mut r) {
                    Ok(Some(v)) => {
                        if tx.send(Incoming::Message(v)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => {
                        let _ = tx.send(Incoming::Eof);
                        return;
                    }
                    Err(e) => {
                        let fatal = e.kind() != io::ErrorKind::InvalidData;
                        let _ = tx.send(if fatal {
                            Incoming::Eof
                        } else {
                            Incoming::Bad(e.to_string())
                        });
                        if fatal {
                            return;
                        }
                    }
                }
            }
        })?;
    let mut s = Server {
        out: output,
        seq: 0,
        engine: None,
        mode: Mode::Stopped,
        stop_on_entry: false,
        defaults,
        bp_id: 0,
        stack: Vec::new(),
    };
    s.run(&rx)
}

struct Server<'w, W: Write> {
    out: &'w mut W,
    seq: i64,
    engine: Option<Engine>,
    mode: Mode,
    stop_on_entry: bool,
    defaults: Defaults,
    bp_id: i64,
    /// The stack as of the last stop (frame ids index it).
    stack: Vec<FrameView>,
}

/// The outcome of handling one request.
enum Flow {
    Go,
    Quit,
}

fn arg<'a>(args: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a Value> {
    args.and_then(|a| a.get(key))
}

fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 > max {
        return Err(format!("{}: larger than {max} bytes", path.display()));
    }
    Ok(bytes)
}

/// A `Breakpoint` body for an address-armed breakpoint.
fn address_breakpoint(id: i64, addr: Option<u16>, why: &str) -> Value {
    addr.map_or_else(
        || json!({"id": id, "verified": false, "message": why}),
        |a| json!({"id": id, "verified": true, "instructionReference": format!("0x{a:04X}")}),
    )
}

fn parse_addr(s: &str) -> Option<u16> {
    let s = s.trim();
    let hex = s
        .strip_prefix('$')
        .or_else(|| s.strip_prefix("0x"))
        .or_else(|| s.strip_prefix("0X"))?;
    u16::from_str_radix(hex, 16).ok()
}

impl<W: Write> Server<'_, W> {
    fn send(&mut self, mut msg: Value) -> io::Result<()> {
        self.seq += 1;
        msg["seq"] = json!(self.seq);
        write_message(self.out, &msg)
    }

    // `body` is moved into the message; clippy cannot see through `json!`.
    #[allow(clippy::needless_pass_by_value)]
    fn event(&mut self, event: &str, body: Value) -> io::Result<()> {
        self.send(json!({"type": "event", "event": event, "body": body}))
    }

    fn respond(&mut self, req: &Value, result: Result<Value, String>) -> io::Result<()> {
        let mut msg = json!({
            "type": "response",
            "request_seq": req.get("seq").cloned().unwrap_or_else(|| json!(0)),
            "command": req.get("command").cloned().unwrap_or_else(|| json!("")),
        });
        match result {
            Ok(body) => {
                msg["success"] = json!(true);
                msg["body"] = body;
            }
            Err(message) => {
                msg["success"] = json!(false);
                msg["message"] = json!(message);
            }
        }
        self.send(msg)
    }

    fn output(&mut self, text: &str) -> io::Result<()> {
        self.event(
            "output",
            json!({"category": "console", "output": format!("{text}\n")}),
        )
    }

    fn run(&mut self, rx: &Receiver<Incoming>) -> io::Result<()> {
        loop {
            let incoming = if self.mode == Mode::Stopped || self.engine.is_none() {
                match rx.recv() {
                    Ok(m) => m,
                    Err(_) => return Ok(()),
                }
            } else {
                // Running: execute until a stop or until a request arrives.
                let mut pending: Option<Incoming> = None;
                let mut poll = || {
                    if pending.is_none() {
                        match rx.try_recv() {
                            Ok(m) => pending = Some(m),
                            Err(TryRecvError::Empty) => {}
                            Err(TryRecvError::Disconnected) => pending = Some(Incoming::Eof),
                        }
                    }
                    pending.is_some()
                };
                let engine = self.engine.as_mut().unwrap_or_else(|| unreachable!());
                let stop = match self.mode {
                    Mode::Continue => engine.resume(&mut poll),
                    Mode::Line(kind) => engine.step_line(kind, &mut poll),
                    Mode::Instruction => engine.step_instruction(),
                    Mode::Stopped => unreachable!(),
                };
                if stop == Stop::Pause && pending.is_some() {
                    let m = pending.take().unwrap_or_else(|| unreachable!());
                    if let Incoming::Message(req) = &m
                        && req.get("command").and_then(Value::as_str) == Some("pause")
                    {
                        self.respond(req, Ok(json!({})))?;
                        self.stopped(Stop::Pause)?;
                        continue;
                    }
                    // Any other request: it is handled below, and the machine
                    // keeps running afterwards unless the request stops it. A
                    // step interrupted this way becomes a plain stop.
                    if self.mode != Mode::Continue {
                        self.stopped(Stop::Pause)?;
                    }
                    m
                } else {
                    self.stopped(stop)?;
                    if let Some(m) = pending {
                        m
                    } else {
                        continue;
                    }
                }
            };
            match incoming {
                Incoming::Eof => return Ok(()),
                Incoming::Bad(e) => {
                    eprintln!("rustynes-dap: ignoring a malformed message: {e}");
                }
                Incoming::Message(req) => {
                    if req.get("type").and_then(Value::as_str) != Some("request") {
                        continue;
                    }
                    if matches!(self.request(&req)?, Flow::Quit) {
                        return Ok(());
                    }
                }
            }
        }
    }

    fn stopped(&mut self, stop: Stop) -> io::Result<()> {
        self.mode = Mode::Stopped;
        let (reason, description) = match stop {
            Stop::Breakpoint => ("breakpoint", None),
            Stop::Step => ("step", None),
            Stop::Pause => ("pause", None),
            Stop::Entry => ("entry", None),
            Stop::Jam => (
                "exception",
                Some("the CPU executed a JAM opcode and halted"),
            ),
            Stop::StepLimit => (
                "step",
                Some("the line did not finish within the step limit; stopped where it was"),
            ),
        };
        if let Some(e) = self.engine.as_mut() {
            self.stack = e.stack();
        }
        let mut body = json!({"reason": reason, "threadId": THREAD, "allThreadsStopped": true});
        if let Some(d) = description {
            body["description"] = json!(d);
            body["text"] = json!(d);
        }
        self.event("stopped", body)
    }

    // One arm per DAP command reads more plainly than the arms spread across
    // helpers.
    #[allow(clippy::too_many_lines)]
    fn request(&mut self, req: &Value) -> io::Result<Flow> {
        let command = req.get("command").and_then(Value::as_str).unwrap_or("");
        let args = req.get("arguments").and_then(Value::as_object);
        let running = self.mode != Mode::Stopped;
        let needs_stop = matches!(
            command,
            "stackTrace"
                | "scopes"
                | "variables"
                | "evaluate"
                | "readMemory"
                | "next"
                | "stepIn"
                | "stepOut"
                | "continue"
        );
        if needs_stop && running {
            self.respond(req, Err("the program is running; pause it first".into()))?;
            return Ok(Flow::Go);
        }
        if needs_stop && self.engine.is_none() {
            self.respond(req, Err("no program launched".into()))?;
            return Ok(Flow::Go);
        }
        match command {
            "initialize" => {
                self.respond(
                    req,
                    Ok(json!({
                        "supportsConfigurationDoneRequest": true,
                        "supportsFunctionBreakpoints": true,
                        "supportsInstructionBreakpoints": true,
                        "supportsReadMemoryRequest": true,
                        "supportsEvaluateForHovers": true,
                        "supportsSteppingGranularity": true,
                        "supportsTerminateRequest": true,
                        "supportsSetVariable": false,
                        "supportsRestartRequest": false,
                    })),
                )?;
            }
            "launch" => {
                let result = self.launch(args);
                let ok = result.is_ok();
                self.respond(req, result.map(|()| json!({})))?;
                if ok {
                    let summary = self.summary();
                    self.output(&summary)?;
                    self.event("initialized", json!({}))?;
                }
            }
            "attach" => {
                self.respond(req, Err("attach is not supported: use launch".into()))?;
            }
            "setBreakpoints" => {
                let r = self.set_breakpoints(args);
                self.respond(req, r)?;
            }
            "setFunctionBreakpoints" => {
                let r = self.set_function_breakpoints(args);
                self.respond(req, r)?;
            }
            "setInstructionBreakpoints" => {
                let r = self.set_instruction_breakpoints(args);
                self.respond(req, r)?;
            }
            "setExceptionBreakpoints" => {
                self.respond(req, Ok(json!({"breakpoints": []})))?;
            }
            "configurationDone" => {
                self.respond(req, Ok(json!({})))?;
                if self.engine.is_some() {
                    if self.stop_on_entry {
                        self.stopped(Stop::Entry)?;
                    } else {
                        self.mode = Mode::Continue;
                    }
                }
            }
            "threads" => {
                self.respond(
                    req,
                    Ok(json!({"threads": [{"id": THREAD, "name": "6502"}]})),
                )?;
            }
            "stackTrace" => {
                let r = Ok(self.stack_trace(args));
                self.respond(req, r)?;
            }
            "scopes" => {
                let frame = arg(args, "frameId").and_then(Value::as_i64).unwrap_or(0);
                let r = if usize::try_from(frame).is_ok_and(|f| f < self.stack.len()) {
                    Ok(json!({"scopes": [
                        {"name": "Locals", "presentationHint": "locals", "variablesReference": REF_LOCALS + frame, "expensive": false},
                        {"name": "Globals", "variablesReference": REF_GLOBALS, "expensive": false},
                        {"name": "Registers", "presentationHint": "registers", "variablesReference": REF_REGISTERS, "expensive": false},
                    ]}))
                } else {
                    Err(format!("no frame {frame}"))
                };
                self.respond(req, r)?;
            }
            "variables" => {
                let r = self.variables(args);
                self.respond(req, r)?;
            }
            "evaluate" => {
                let r = self.evaluate(args);
                self.respond(req, r)?;
            }
            "readMemory" => {
                let r = self.read_memory(args);
                self.respond(req, r)?;
            }
            "continue" => {
                self.respond(req, Ok(json!({"allThreadsContinued": true})))?;
                self.mode = Mode::Continue;
            }
            "next" | "stepIn" | "stepOut" => {
                let instruction =
                    arg(args, "granularity").and_then(Value::as_str) == Some("instruction");
                self.respond(req, Ok(json!({})))?;
                self.mode = match (command, instruction) {
                    ("stepOut", _) => Mode::Line(StepKind::Out),
                    (_, true) => Mode::Instruction,
                    ("next", false) => Mode::Line(StepKind::Over),
                    _ => Mode::Line(StepKind::In),
                };
            }
            "pause" => {
                self.respond(req, Ok(json!({})))?;
                if self.engine.is_some() && running {
                    self.stopped(Stop::Pause)?;
                }
            }
            "disconnect" | "terminate" => {
                self.respond(req, Ok(json!({})))?;
                self.event("terminated", json!({}))?;
                return Ok(Flow::Quit);
            }
            other => {
                self.respond(
                    req,
                    Err(format!("`{other}` is not supported by rustynes-dap")),
                )?;
            }
        }
        Ok(Flow::Go)
    }

    fn summary(&self) -> String {
        self.engine.as_ref().and_then(Engine::info).map_or_else(
            || "rustynes-dap: no debug information; instruction-level debugging only".into(),
            |i| {
                format!(
                    "rustynes-dap: {:?} debug information: {} source files, {} functions, {} globals, {} statement starts",
                    i.format,
                    i.files.len(),
                    i.functions.len(),
                    i.globals.len(),
                    i.stmts.len()
                )
            },
        )
    }

    fn launch(&mut self, args: Option<&Map<String, Value>>) -> Result<(), String> {
        let program = arg(args, "program")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .or_else(|| self.defaults.program.clone())
            .ok_or("launch needs `program` (an .nes file)")?;
        let debug = arg(args, "debugInfo")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .or_else(|| self.defaults.debug_info.clone())
            .or_else(|| {
                let elf = PathBuf::from(format!("{}.elf", program.display()));
                let dbg = program.with_extension("dbg");
                [elf, dbg].into_iter().find(|p| p.is_file())
            });
        let rom = read_bounded(&program, MAX_ROM_BYTES)?;
        let nes = Nes::from_rom(&rom).map_err(|e| format!("{}: {e}", program.display()))?;
        let info = match &debug {
            Some(path) => {
                let bytes = read_bounded(path, MAX_DEBUG_FILE_BYTES)?;
                let root = arg(args, "sourceRoot")
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
                    .or_else(|| path.parent().map(Path::to_path_buf))
                    .unwrap_or_default();
                Some(
                    DebugInfo::load(&bytes, &root)
                        .map_err(|e| format!("{}: {e}", path.display()))?,
                )
            }
            None => None,
        };
        self.stop_on_entry = arg(args, "stopOnEntry")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        self.engine = Some(Engine::new(nes, info));
        self.mode = Mode::Stopped;
        Ok(())
    }

    const fn next_bp_id(&mut self) -> i64 {
        self.bp_id += 1;
        self.bp_id
    }

    fn set_breakpoints(&mut self, args: Option<&Map<String, Value>>) -> Result<Value, String> {
        let source = arg(args, "source").cloned().unwrap_or_else(|| json!({}));
        let path = source.get("path").and_then(Value::as_str).unwrap_or("");
        // `breakpoints[].line` is the current form; `lines` the deprecated one.
        let numbers: Vec<u64> = arg(args, "breakpoints")
            .and_then(Value::as_array)
            .map_or_else(
                || {
                    arg(args, "lines")
                        .and_then(Value::as_array)
                        .map(|ls| ls.iter().filter_map(Value::as_u64).collect())
                        .unwrap_or_default()
                },
                |bps| {
                    bps.iter()
                        .filter_map(|b| b.get("line").and_then(Value::as_u64))
                        .collect()
                },
            );
        let lines: Vec<u32> = numbers
            .into_iter()
            .filter_map(|l| u32::try_from(l).ok())
            .collect();
        let engine = self.engine.as_mut().ok_or("no program launched")?;
        let file = engine.info().and_then(|i| i.file_for_path(path));
        let resolved = match file {
            Some(f) => engine.set_source_breakpoints(f, &lines)?,
            None => vec![None; lines.len()],
        };
        let mut out = Vec::new();
        for (req, got) in lines.iter().zip(resolved) {
            let id = self.next_bp_id();
            let why = if file.is_some() {
                "no code at or after this line"
            } else {
                "this file is not in the program's debug information"
            };
            out.push(got.map_or_else(
                || json!({"id": id, "verified": false, "line": req, "source": source, "message": why}),
                |line| json!({"id": id, "verified": true, "line": line, "source": source}),
            ));
        }
        Ok(json!({"breakpoints": out}))
    }

    fn set_function_breakpoints(
        &mut self,
        args: Option<&Map<String, Value>>,
    ) -> Result<Value, String> {
        let names: Vec<String> = arg(args, "breakpoints")
            .and_then(Value::as_array)
            .map(|b| {
                b.iter()
                    .filter_map(|x| x.get("name").and_then(Value::as_str).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let engine = self.engine.as_mut().ok_or("no program launched")?;
        let found = engine.set_function_breakpoints(&refs)?;
        let mut out = Vec::new();
        for f in found {
            let id = self.next_bp_id();
            out.push(address_breakpoint(id, f, "no such function"));
        }
        Ok(json!({"breakpoints": out}))
    }

    fn set_instruction_breakpoints(
        &mut self,
        args: Option<&Map<String, Value>>,
    ) -> Result<Value, String> {
        let mut addrs = Vec::new();
        let mut verified = Vec::new();
        for b in arg(args, "breakpoints")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let base = b
                .get("instructionReference")
                .and_then(Value::as_str)
                .and_then(parse_addr);
            let off = b.get("offset").and_then(Value::as_i64).unwrap_or(0);
            let addr = base.and_then(|a| u16::try_from(i64::from(a) + off).ok());
            if let Some(a) = addr {
                addrs.push(a);
            }
            verified.push(addr);
        }
        let engine = self.engine.as_mut().ok_or("no program launched")?;
        engine.set_address_breakpoints(&addrs)?;
        let mut out = Vec::new();
        for v in verified {
            let id = self.next_bp_id();
            out.push(address_breakpoint(id, v, "not an address in $0000-$FFFF"));
        }
        Ok(json!({"breakpoints": out}))
    }

    fn stack_trace(&self, args: Option<&Map<String, Value>>) -> Value {
        let start = arg(args, "startFrame").and_then(Value::as_u64).unwrap_or(0);
        let levels = arg(args, "levels")
            .and_then(Value::as_u64)
            .filter(|&l| l > 0)
            .unwrap_or(u64::MAX);
        let info = self.engine.as_ref().and_then(Engine::info);
        let frames: Vec<Value> = self
            .stack
            .iter()
            .enumerate()
            .skip(usize::try_from(start).unwrap_or(usize::MAX))
            .take(usize::try_from(levels).unwrap_or(usize::MAX))
            .map(|(i, f)| {
                let mut v = json!({
                    "id": i,
                    "name": f.name,
                    "line": 0,
                    "column": 0,
                    "instructionPointerReference": format!("0x{:04X}", f.pc),
                });
                if let (Some(loc), Some(info)) = (f.loc, info) {
                    let file = &info.files[loc.file];
                    v["line"] = json!(loc.line);
                    v["column"] = json!(1);
                    v["source"] = json!({
                        "name": Path::new(&file.recorded).file_name().map_or_else(|| file.recorded.clone(), |n| n.to_string_lossy().into_owned()),
                        "path": file.path.to_string_lossy(),
                    });
                } else {
                    v["presentationHint"] = json!("subtle");
                }
                v
            })
            .collect();
        json!({"stackFrames": frames, "totalFrames": self.stack.len()})
    }

    // `a` / `x` / `y` / `s` / `p` are the 6502's register names.
    #[allow(clippy::many_single_char_names)]
    fn variables(&mut self, args: Option<&Map<String, Value>>) -> Result<Value, String> {
        let r = arg(args, "variablesReference")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let engine = self.engine.as_mut().ok_or("no program launched")?;
        let mut out = Vec::new();
        let var_json = |name: &str, value: &crate::engine::Value, ty: &str| {
            let mut v =
                json!({"name": name, "value": value.text, "type": ty, "variablesReference": 0});
            if let Some(a) = value.addr {
                v["memoryReference"] = json!(format!("0x{a:04X}"));
            }
            v
        };
        match r {
            REF_GLOBALS => {
                let globals = engine.info().map(|i| i.globals.clone()).unwrap_or_default();
                for g in &globals {
                    let value = engine.value(g, None);
                    out.push(var_json(&g.name, &value, &g.ty.name));
                }
            }
            REF_REGISTERS => {
                let cpu = engine.nes().cpu();
                let (a, x, y, s, pc) = (cpu.a, cpu.x, cpu.y, cpu.s, cpu.pc);
                let p = cpu.p.bits();
                let flags: String = "NV-BDIZC"
                    .chars()
                    .enumerate()
                    .map(|(i, c)| {
                        if p & (0x80 >> i) != 0 {
                            c
                        } else {
                            c.to_ascii_lowercase()
                        }
                    })
                    .collect();
                for (name, value) in [
                    ("A", format!("${a:02X}")),
                    ("X", format!("${x:02X}")),
                    ("Y", format!("${y:02X}")),
                    ("P", format!("${p:02X} {flags}")),
                    ("S", format!("${s:02X}")),
                    ("PC", format!("${pc:04X}")),
                ] {
                    out.push(json!({"name": name, "value": value, "variablesReference": 0}));
                }
            }
            r if r >= REF_LOCALS => {
                let i = usize::try_from(r - REF_LOCALS).map_err(|_| "bad reference")?;
                let frame = self
                    .stack
                    .get(i)
                    .cloned()
                    .ok_or_else(|| format!("no frame {i}"))?;
                let mut vars = engine.frame_vars(&frame);
                vars.sort_by_key(|v| !v.is_param);
                for v in &vars {
                    let value = engine.value(v, frame.base);
                    out.push(var_json(&v.name, &value, &v.ty.name));
                }
            }
            _ => return Err(format!("no variables reference {r}")),
        }
        Ok(json!({"variables": out}))
    }

    fn evaluate(&mut self, args: Option<&Map<String, Value>>) -> Result<Value, String> {
        let expr = arg(args, "expression")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned();
        let frame = arg(args, "frameId")
            .and_then(Value::as_u64)
            .and_then(|f| usize::try_from(f).ok())
            .unwrap_or(0);
        let engine = self.engine.as_mut().ok_or("no program launched")?;
        if let Some(addr) = parse_addr(&expr) {
            let b = engine.peek(addr);
            return Ok(
                json!({"result": format!("{b} (0x{b:02X})"), "variablesReference": 0, "memoryReference": format!("0x{addr:04X}")}),
            );
        }
        let cpu = engine.nes().cpu();
        let reg = match expr.to_ascii_uppercase().as_str() {
            "A" => Some(format!("${:02X}", cpu.a)),
            "X" => Some(format!("${:02X}", cpu.x)),
            "Y" => Some(format!("${:02X}", cpu.y)),
            "P" => Some(format!("${:02X}", cpu.p.bits())),
            "S" | "SP" => Some(format!("${:02X}", cpu.s)),
            "PC" => Some(format!("${:04X}", cpu.pc)),
            _ => None,
        };
        if let Some(r) = reg {
            return Ok(json!({"result": r, "variablesReference": 0}));
        }
        if let Some(f) = self.stack.get(frame).cloned()
            && let Some(v) = engine.frame_vars(&f).into_iter().find(|v| v.name == expr)
        {
            let value = engine.value(&v, f.base);
            return Ok(json!({"result": value.text, "type": v.ty.name, "variablesReference": 0}));
        }
        if let Some(g) = engine.info().and_then(|i| i.global_named(&expr)).cloned() {
            let value = engine.value(&g, None);
            return Ok(json!({"result": value.text, "type": g.ty.name, "variablesReference": 0}));
        }
        Err(format!(
            "`{expr}` is not a variable, a register or a $XXXX address"
        ))
    }

    fn read_memory(&mut self, args: Option<&Map<String, Value>>) -> Result<Value, String> {
        let base = arg(args, "memoryReference")
            .and_then(Value::as_str)
            .and_then(parse_addr)
            .ok_or("memoryReference must be 0xXXXX")?;
        let off = arg(args, "offset").and_then(Value::as_i64).unwrap_or(0);
        let count = arg(args, "count")
            .and_then(Value::as_u64)
            .and_then(|c| usize::try_from(c).ok())
            .unwrap_or(0);
        let start = i64::from(base) + off;
        let Ok(start) = u16::try_from(start) else {
            return Ok(
                json!({"address": format!("0x{:X}", start.max(0)), "unreadableBytes": count}),
            );
        };
        let readable = count.min(0x1_0000 - usize::from(start));
        let engine = self.engine.as_mut().ok_or("no program launched")?;
        let bytes = engine.read(start, readable);
        Ok(json!({
            "address": format!("0x{start:04X}"),
            "data": base64::engine::general_purpose::STANDARD.encode(&bytes),
            "unreadableBytes": count - bytes.len(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_round_trips_and_bounds_the_body() {
        let mut buf = Vec::new();
        write_message(&mut buf, &json!({"a": 1})).expect("write");
        assert_eq!(buf, b"Content-Length: 7\r\n\r\n{\"a\":1}");
        let mut r = io::Cursor::new(buf);
        assert_eq!(read_message(&mut r).expect("read"), Some(json!({"a": 1})));
        assert_eq!(read_message(&mut r).expect("eof"), None);

        let big = format!("Content-Length: {}\r\n\r\n", MAX_MESSAGE_BYTES + 1);
        let e = read_message(&mut io::Cursor::new(big)).expect_err("too big");
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        let e = read_message(&mut io::Cursor::new("X-Other: 1\r\n\r\n{}")).expect_err("no length");
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        let e = read_message(&mut io::Cursor::new("Content-Length: 2\r\n\r\nno"))
            .expect_err("not json");
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn addresses_parse() {
        assert_eq!(parse_addr("$6000"), Some(0x6000));
        assert_eq!(parse_addr("0x00ff"), Some(0xFF));
        assert_eq!(parse_addr("6000"), None);
        assert_eq!(parse_addr("$10000"), None);
    }
}

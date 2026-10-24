// SPDX-License-Identifier: GPL-3.0-or-later
//! The `T-DEBUG-DAP` gate: a scripted Debug Adapter Protocol client drives
//! the real `rustynes-dap` binary over stdio, exactly as an editor would,
//! against the cc65 and the llvm-mos fixture. Each session stops at a source
//! breakpoint, steps a line and reads a variable; the values are the C
//! program's own (`add3(5)` sees `v == 5` and computes `r == 8`), so they
//! check the debugger against the program rather than against itself.

mod common;

use std::io::{BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use base64::Engine as _;
use common::{CC65, Fixture, LLVM_MOS};
use rustynes_dap::dap::{read_message, write_message};
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(60);

struct Client {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    seq: i64,
    /// Events seen while waiting for responses.
    events: Vec<Value>,
}

impl Client {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rustynes-dap"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn rustynes-dap");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            while let Ok(Some(m)) = read_message(&mut r) {
                if tx.send(m).is_err() {
                    return;
                }
            }
        });
        Self {
            child,
            stdin,
            rx,
            seq: 0,
            events: Vec::new(),
        }
    }

    fn send(&mut self, command: &str, arguments: &Value) -> i64 {
        self.seq += 1;
        let msg = json!({"seq": self.seq, "type": "request", "command": command, "arguments": *arguments});
        write_message(&mut self.stdin, &msg).expect("write request");
        self.seq
    }

    fn next(&self) -> Value {
        self.rx
            .recv_timeout(TIMEOUT)
            .expect("the adapter answered within the timeout")
    }

    /// Send a request and return its response (events are queued).
    // Callers pass `json!` literals; taking them by value reads better.
    #[allow(clippy::needless_pass_by_value)]
    fn call(&mut self, command: &str, arguments: Value) -> Value {
        let seq = self.send(command, &arguments);
        loop {
            let m = self.next();
            if m["type"] == "response" && m["request_seq"] == seq {
                assert_eq!(m["command"], command);
                return m;
            }
            self.events.push(m);
        }
    }

    /// A successful response's body.
    // Callers pass `json!` literals; taking them by value reads better.
    #[allow(clippy::needless_pass_by_value)]
    fn ok(&mut self, command: &str, arguments: Value) -> Value {
        let r = self.call(command, arguments);
        assert_eq!(r["success"], true, "{command} failed: {r}");
        r["body"].clone()
    }

    /// Wait for the event `name` (taking any already queued first).
    fn event(&mut self, name: &str) -> Value {
        if let Some(i) = self.events.iter().position(|e| e["event"] == name) {
            return self.events.remove(i);
        }
        loop {
            let m = self.next();
            if m["type"] == "event" && m["event"] == name {
                return m;
            }
            self.events.push(m);
        }
    }

    fn stopped(&mut self) -> String {
        let e = self.event("stopped");
        e["body"]["reason"].as_str().expect("reason").to_owned()
    }

    fn top(&mut self) -> Value {
        let st = self.ok("stackTrace", json!({"threadId": 1}));
        st["stackFrames"].clone()
    }

    fn locals(&mut self, frame: i64) -> Vec<Value> {
        let scopes = self.ok("scopes", json!({"frameId": frame}));
        let r = scopes["scopes"][0]["variablesReference"].clone();
        assert_eq!(scopes["scopes"][0]["name"], "Locals");
        let vars = self.ok("variables", json!({"variablesReference": r}));
        vars["variables"].as_array().expect("variables").clone()
    }
}

fn number(v: &Value) -> u64 {
    v.as_str()
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("not a number: {v}"))
}

fn local(vars: &[Value], name: &str) -> Value {
    vars.iter()
        .find(|v| v["name"] == name)
        .cloned()
        .unwrap_or_else(|| panic!("no local {name} in {vars:?}"))
}

// One editor session, in the order an editor sends its requests.
#[allow(clippy::too_many_lines)]
fn session(fx: Fixture) {
    let mut c = Client::start();
    let caps = c.ok(
        "initialize",
        json!({"adapterID": "rustynes", "linesStartAt1": true, "columnsStartAt1": true}),
    );
    assert_eq!(caps["supportsConfigurationDoneRequest"], true);
    assert_eq!(caps["supportsReadMemoryRequest"], true);
    c.ok(
        "launch",
        json!({
            "program": fx.rom_path(),
            "debugInfo": fx.debug_path(),
            "sourceRoot": common::source().parent().expect("dir"),
        }),
    );
    c.event("initialized");

    let src = common::source().canonicalize().expect("source path");
    let body = common::line("add3-body");
    let ret = common::line("add3-return");
    let bps = c.ok(
        "setBreakpoints",
        json!({"source": {"path": src}, "breakpoints": [{"line": body}, {"line": 2}]}),
    );
    assert_eq!(
        bps["breakpoints"][0]["verified"], true,
        "{}: {bps}",
        fx.name
    );
    assert_eq!(bps["breakpoints"][0]["line"], body);
    // Line 2 is in the comment header; the next line with code is add3's
    // opening brace (its prologue), so the breakpoint moves there.
    assert_eq!(bps["breakpoints"][1]["verified"], true);
    c.ok("setExceptionBreakpoints", json!({"filters": []}));
    c.ok(
        "setBreakpoints",
        json!({"source": {"path": src}, "breakpoints": [{"line": body}]}),
    );
    c.ok("configurationDone", json!({}));

    // Stop at the source breakpoint.
    assert_eq!(c.stopped(), "breakpoint", "{}", fx.name);
    let threads = c.ok("threads", json!({}));
    assert_eq!(threads["threads"][0]["id"], 1);
    let frames = c.top();
    assert_eq!(frames[0]["name"], "add3", "{}: {frames}", fx.name);
    assert_eq!(frames[0]["line"], body, "{}", fx.name);
    assert!(
        frames[0]["source"]["path"]
            .as_str()
            .is_some_and(|p| p.ends_with("dapdemo.c"))
    );
    assert_eq!(frames[1]["name"], "main", "{}", fx.name);
    assert_eq!(
        frames[1]["line"],
        common::line("main-call"),
        "{}: the caller's line",
        fx.name
    );
    let vars = c.locals(0);
    assert_eq!(number(&local(&vars, "v")["value"]), 5, "{}: v", fx.name);
    let main_vars = c.locals(1);
    assert_eq!(
        number(&local(&main_vars, "local")["value"]),
        5,
        "{}: main's local",
        fx.name
    );

    // Step a line and read the variable it assigned.
    c.ok("next", json!({"threadId": 1}));
    assert_eq!(c.stopped(), "step", "{}", fx.name);
    assert_eq!(
        c.top()[0]["line"],
        ret,
        "{}: next reaches the return line",
        fx.name
    );
    let r = local(&c.locals(0), "r");
    assert_eq!(number(&r["value"]), 8, "{}: r = v + 3", fx.name);

    // The same value through readMemory and evaluate.
    let mem_ref = r["memoryReference"]
        .as_str()
        .expect("memoryReference")
        .to_owned();
    let mem = c.ok(
        "readMemory",
        json!({"memoryReference": mem_ref, "count": 1}),
    );
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(mem["data"].as_str().expect("data"))
        .expect("base64");
    assert_eq!(bytes, vec![8], "{}", fx.name);
    let ev = c.ok(
        "evaluate",
        json!({"expression": "r", "frameId": 0, "context": "hover"}),
    );
    assert_eq!(number(&ev["result"]), 8, "{}", fx.name);

    // Second call: continue to the breakpoint again.
    c.ok("continue", json!({"threadId": 1}));
    assert_eq!(c.stopped(), "breakpoint", "{}", fx.name);
    assert_eq!(
        number(&local(&c.locals(0), "v")["value"]),
        8,
        "{}: second call",
        fx.name
    );
    let g = c.ok(
        "evaluate",
        json!({"expression": "counter", "frameId": 0, "context": "watch"}),
    );
    assert_eq!(number(&g["result"]), 8, "{}: the global", fx.name);
    let regs = c.ok("variables", json!({"variablesReference": 2}));
    assert_eq!(regs["variables"][5]["name"], "PC");

    // Pause while running, and refuse a stopped-only request meanwhile.
    c.ok(
        "setBreakpoints",
        json!({"source": {"path": src}, "breakpoints": []}),
    );
    c.ok("continue", json!({"threadId": 1}));
    let refused = c.call("stackTrace", json!({"threadId": 1}));
    assert_eq!(
        refused["success"], false,
        "stackTrace is refused while running"
    );
    c.ok("pause", json!({"threadId": 1}));
    assert_eq!(c.stopped(), "pause", "{}", fx.name);
    assert_ne!(
        c.top().as_array().map(Vec::len),
        Some(0),
        "a stack after the pause"
    );

    c.ok("disconnect", json!({}));
    c.event("terminated");
    drop(c.stdin);
    let status = c.child.wait().expect("adapter exits");
    assert!(status.success(), "{}: exit {status}", fx.name);
}

#[test]
fn cc65_editor_session() {
    session(CC65);
}

#[test]
fn llvm_mos_editor_session() {
    session(LLVM_MOS);
}

#[test]
fn launch_refuses_what_it_cannot_load() {
    let mut c = Client::start();
    c.ok("initialize", json!({"adapterID": "rustynes"}));
    let r = c.call(
        "launch",
        json!({"program": common::dir().join("no-such.nes")}),
    );
    assert_eq!(r["success"], false);
    let r = c.call("launch", json!({"program": common::source()}));
    assert_eq!(r["success"], false, "a C file is not an iNES image");
    let r = c.call(
        "launch",
        json!({"program": CC65.rom_path(), "debugInfo": common::source()}),
    );
    assert_eq!(r["success"], false, "a C file is not debug information");
    let r = c.call(
        "writeMemory",
        json!({"memoryReference": "0x0000", "data": "AA=="}),
    );
    assert_eq!(r["success"], false, "there is no write path");
    let r = c.call("stackTrace", json!({"threadId": 1}));
    assert_eq!(r["success"], false, "nothing launched");
    c.ok("disconnect", json!({}));
    c.stdin.flush().expect("flush");
    assert!(c.child.wait().expect("exit").success());
}

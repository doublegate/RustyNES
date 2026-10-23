//! End to end over the real transport: spawn the `rustynes-mcp` binary, speak
//! newline-delimited JSON-RPC to it on stdin/stdout as an MCP client does,
//! and check the handshake, the catalogue, a tool call with an image, a tool
//! failure, and the protocol error for an undeclared tool.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Client {
    fn send(&mut self, msg: &Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Read lines until the response with `id` arrives.
    fn response(&mut self, id: u64) -> Value {
        let mut line = String::new();
        loop {
            line.clear();
            let n = self.stdout.read_line(&mut line).unwrap();
            assert!(n > 0, "server closed stdout before answering {id}");
            let v: Value = serde_json::from_str(&line).expect("one JSON message per line");
            if v["id"] == id {
                return v;
            }
        }
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        let mut msg = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        msg["params"] = params;
        self.send(&msg);
        self.response(id)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn an_mcp_client_drives_the_binary_over_stdio() {
    let rom_path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("mcp-stdio-{}.nes", std::process::id()));
    std::fs::write(&rom_path, common::input_rom()).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_rustynes-mcp"))
        .arg("--rom")
        .arg(&rom_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the binary starts");
    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let mut c = Client {
        child,
        stdin,
        stdout,
    };

    let init = c.request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "rustynes-mcp-test", "version": "0" },
        }),
    );
    assert_eq!(
        init["result"]["serverInfo"]["name"], "rustynes-mcp",
        "{init}"
    );
    assert!(
        init["result"]["capabilities"]["tools"].is_object(),
        "{init}"
    );
    c.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));

    let list = c.request(2, "tools/list", json!({}));
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .expect("a tool list")
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, rustynes_mcp::tools::TOOL_NAMES, "{list}");
    let write_ram = &list["result"]["tools"][10];
    assert_eq!(write_ram["name"], "write_ram");
    assert_eq!(
        write_ram["inputSchema"]["additionalProperties"], false,
        "the schema an agent sees says unknown fields are refused: {write_ram}"
    );

    let step = c.request(
        3,
        "tools/call",
        json!({ "name": "step_frames", "arguments": { "frames": 5, "input": { "p1": ["A"] } } }),
    );
    assert_eq!(step["result"]["isError"], false, "{step}");
    assert_eq!(
        step["result"]["structuredContent"]["frames_run"], 5,
        "{step}"
    );

    let shot = c.request(
        4,
        "tools/call",
        json!({ "name": "screenshot", "arguments": {} }),
    );
    let image = shot["result"]["content"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["type"] == "image")
        .expect("an image block");
    assert_eq!(image["mimeType"], "image/png");

    let refused = c.request(
        5,
        "tools/call",
        json!({ "name": "write_ram", "arguments": { "addr": 8192, "bytes": [1] } }),
    );
    assert_eq!(refused["result"]["isError"], true, "{refused}");

    let unknown = c.request(
        6,
        "tools/call",
        json!({ "name": "debug_poke_ppu", "arguments": { "addr": 0, "value": 1 } }),
    );
    assert_eq!(unknown["error"]["code"], -32602, "{unknown}");

    drop(c);
    let _ = std::fs::remove_file(&rom_path);
}

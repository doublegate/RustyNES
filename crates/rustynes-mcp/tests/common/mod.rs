//! Shared fixtures for the `rustynes-mcp` tests.
//!
//! The ROM is assembled here, by hand, so the suite needs no ROM file and no
//! licence: a 16 KiB NROM whose main loop waits for vblank, reads controller 1
//! into `$10`, adds it into a running sum at `$12`, counts frames in `$11`,
//! and paints the backdrop colour from the sum. Different inputs therefore
//! diverge in work RAM AND in the framebuffer within one frame, which is what
//! a fork comparison needs to be able to see.

#![allow(dead_code)] // each test binary uses a different subset

use rustynes_mcp::{Host, Session, SessionConfig, ToolError, ToolOutput};
use serde_json::{Value, json};

mod rom;
#[allow(unused_imports)] // each test binary uses a different subset
pub use rom::{AFTER_READ_PC, LOOP_END_PC, input_rom};

/// A host over the fixture with no output directory.
#[must_use]
pub fn host() -> Host {
    host_with(None)
}

/// A host over the fixture, optionally allowed to write into `out_dir`.
#[must_use]
pub fn host_with(out_dir: Option<std::path::PathBuf>) -> Host {
    Host::new(
        Session::new(SessionConfig {
            rom: input_rom(),
            out_dir,
        })
        .expect("fixture loads"),
    )
}

/// Call a tool with a JSON object of arguments.
pub fn call(host: &Host, name: &str, args: Value) -> Result<ToolOutput, ToolError> {
    let map = match args {
        Value::Object(m) => m,
        Value::Null => serde_json::Map::new(),
        other => panic!("arguments must be an object, got {other}"),
    };
    host.call(name, Some(&map))
}

/// Call a tool that must succeed; return its JSON.
pub fn ok(host: &Host, name: &str, args: Value) -> Value {
    let shown = args.to_string();
    match call(host, name, args) {
        Ok(out) => out.json,
        Err(e) => panic!("{name}({shown}) failed: {e}"),
    }
}

/// The main machine's save state, for "nothing changed" assertions.
#[must_use]
pub fn main_snapshot(host: &Host) -> Vec<u8> {
    host.with_session(|s| s.nes().snapshot())
}

/// `{"p1": [...]}` shorthand.
#[must_use]
pub fn p1(buttons: &[&str]) -> Value {
    json!({ "p1": buttons })
}

//! `rustynes-mcp` — serve one ROM to an MCP client over stdio.
//!
//! ```text
//! rustynes-mcp --rom <file.nes> [--out-dir <dir>]
//! ```
//!
//! The operator, not the agent, chooses the ROM and the only directory the
//! server may write. Stdout carries the protocol, so diagnostics go to stderr.
//! An MCP client launches it as a command, e.g. in a client's server list:
//! `{"command": "rustynes-mcp", "args": ["--rom", "/path/game.nes"]}`.

use std::path::PathBuf;
use std::process::ExitCode;

use rustynes_mcp::{Host, Session, SessionConfig, limits};

const USAGE: &str = "usage: rustynes-mcp --rom <file.nes> [--out-dir <dir>]\n\
\n\
Serves the Model Context Protocol on stdin/stdout. The machine starts paused.\n\
--rom      iNES / NES 2.0 image to load (required)\n\
--out-dir  existing directory movie_export may write <name>.rnm into\n\
           (without it the server writes no files)";

fn parse_args() -> Result<SessionConfig, String> {
    let mut rom = None;
    let mut out_dir = None;
    let mut it = std::env::args_os().skip(1);
    while let Some(arg) = it.next() {
        match arg.to_str() {
            Some("--rom") => rom = Some(PathBuf::from(it.next().ok_or("--rom needs a path")?)),
            Some("--out-dir") => {
                out_dir = Some(PathBuf::from(it.next().ok_or("--out-dir needs a path")?));
            }
            Some("-h" | "--help") => return Err(String::new()),
            _ => return Err(format!("unknown argument {}", arg.display())),
        }
    }
    let rom = rom.ok_or("--rom is required")?;
    let len = std::fs::metadata(&rom)
        .map_err(|e| format!("{}: {e}", rom.display()))?
        .len();
    if len > limits::MAX_ROM_BYTES {
        return Err(format!("{}: {len} bytes is too large", rom.display()));
    }
    let bytes = std::fs::read(&rom).map_err(|e| format!("{}: {e}", rom.display()))?;
    if let Some(dir) = &out_dir
        && !dir.is_dir()
    {
        return Err(format!("--out-dir {}: not a directory", dir.display()));
    }
    Ok(SessionConfig {
        rom: bytes,
        out_dir,
    })
}

fn main() -> ExitCode {
    let config = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("rustynes-mcp: {e}");
            }
            eprintln!("{USAGE}");
            return if e.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            };
        }
    };
    let session = match Session::new(config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rustynes-mcp: {e}");
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("rustynes-mcp: tokio runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(rustynes_mcp::host::serve_stdio(Host::new(session))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rustynes-mcp: {e}");
            ExitCode::FAILURE
        }
    }
}

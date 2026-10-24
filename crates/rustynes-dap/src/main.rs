// SPDX-License-Identifier: GPL-3.0-or-later
//! `rustynes-dap` — debug a homebrew NES program at the source level.
//!
//! ```text
//! rustynes-dap [--rom <file.nes>] [--debug-info <file.dbg|file.elf>]
//! rustynes-dap --gdb 127.0.0.1:<port> --rom <file.nes> [--debug-info <file.elf>]
//! ```
//!
//! With no `--gdb`, the Debug Adapter Protocol is served on stdin / stdout:
//! an editor launches this binary as its debug adapter, and its `launch`
//! request names the ROM and debug file (`--rom` / `--debug-info` are
//! defaults for it). With `--gdb`, a GDB Remote Serial Protocol stub listens
//! on that loopback address for one client. Diagnostics go to stderr.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use rustynes_core::Nes;
use rustynes_dap::{dap, debuginfo, engine::Engine, gdb};

const USAGE: &str = "usage: rustynes-dap [--rom <file.nes>] [--debug-info <file>]\n\
       rustynes-dap --gdb 127.0.0.1:<port> --rom <file.nes> [--debug-info <file.elf>]\n\
\n\
Without --gdb: serves the Debug Adapter Protocol on stdin/stdout (an editor\n\
launches it). --rom and --debug-info are defaults for the launch request.\n\
--gdb ADDR       serve a GDB remote stub on ADDR (loopback only), one client\n\
--rom FILE       iNES / NES 2.0 image\n\
--debug-info F   cc65 ld65 --dbgfile output, or an llvm-mos ELF";

struct Args {
    rom: Option<PathBuf>,
    debug: Option<PathBuf>,
    gdb: Option<SocketAddr>,
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        rom: None,
        debug: None,
        gdb: None,
    };
    let mut it = std::env::args_os().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.to_str() {
            Some("--rom") => a.rom = Some(PathBuf::from(value("--rom")?)),
            Some("--debug-info") => a.debug = Some(PathBuf::from(value("--debug-info")?)),
            Some("--gdb") => {
                let v = value("--gdb")?;
                let v = v.to_str().ok_or("--gdb: not UTF-8")?;
                a.gdb = Some(v.parse().map_err(|e| format!("--gdb {v}: {e}"))?);
            }
            Some("-h" | "--help") => return Err(String::new()),
            _ => return Err(format!("unknown argument {}", arg.display())),
        }
    }
    Ok(a)
}

fn read(path: &PathBuf, max: u64) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(max + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 > max {
        return Err(format!("{}: too large", path.display()));
    }
    Ok(bytes)
}

fn gdb_main(addr: SocketAddr, args: &Args) -> Result<(), String> {
    let rom_path = args.rom.as_ref().ok_or("--gdb needs --rom")?;
    let rom = read(rom_path, dap::MAX_ROM_BYTES)?;
    let nes = Nes::from_rom(&rom).map_err(|e| format!("{}: {e}", rom_path.display()))?;
    let mut regs = gdb::Registers::physical();
    if let Some(p) = &args.debug {
        let bytes = read(p, debuginfo::MAX_DEBUG_FILE_BYTES)?;
        if bytes.starts_with(b"\x7fELF") {
            let rc = debuginfo::dwarf::imaginary_registers(&bytes)
                .map_err(|e| format!("{}: {e}", p.display()))?;
            regs = gdb::Registers::with_imaginary(&rc);
        }
    }
    // The stub's client does its own symbolication; the engine runs with no
    // debug information, so no frame hooks are armed.
    gdb::listen(addr, Engine::new(nes, None), &regs).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("rustynes-dap: {e}");
            }
            eprintln!("{USAGE}");
            return if e.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            };
        }
    };
    let result = if let Some(addr) = args.gdb {
        gdb_main(addr, &args)
    } else {
        let defaults = dap::Defaults {
            program: args.rom.clone(),
            debug_info: args.debug,
        };
        let mut out = std::io::stdout().lock();
        dap::serve(std::io::stdin(), &mut out, defaults).map_err(|e| e.to_string())
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rustynes-dap: {e}");
            ExitCode::FAILURE
        }
    }
}

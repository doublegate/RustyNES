//! `RustyNES` v2 frontend binary (native).
//!
//! v1.3.0 Sprint 1.2 — converted into a thin shim over `lib.rs`,
//! which now owns the module tree. The wasm32 entry point lives at
//! `lib.rs::wasm::start` (gated `#[cfg(target_arch = "wasm32")]`).
//!
//! See `docs/frontend.md` for the architecture and
//! `to-dos/phase-5-frontend-tooling/sprint-1-frontend-mvp.md` for the
//! original sprint ticket list.

// v1.3.0 Sprint 1.4 — this `[[bin]]` is the NATIVE desktop binary.
// The wasm32 frontend is the `cdylib` (`lib.rs` + `wasm.rs`), so when
// cargo builds this bin for the wasm32 target (it tries to build all
// targets) we compile an empty `main` instead — the real entry point
// is `wasm::start`.
#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::process::ExitCode;

#[cfg(not(target_arch = "wasm32"))]
use rustynes_frontend::app;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> ExitCode {
    // Tiny argv parsing — no need for clap in v0.
    let mut args = std::env::args().skip(1);
    let rom_arg = match args.next() {
        Some(arg) if !arg.starts_with('-') => arg,
        Some(arg) if arg == "-h" || arg == "--help" => {
            print_usage();
            return ExitCode::SUCCESS;
        }
        Some(_) | None => {
            print_usage();
            return ExitCode::from(2);
        }
    };
    let rom_path = PathBuf::from(rom_arg);
    if !rom_path.exists() {
        eprintln!("rustynes: ROM file not found: {}", rom_path.display());
        return ExitCode::from(1);
    }

    match app::run(&rom_path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rustynes: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn print_usage() {
    eprintln!(
        "RustyNES v2 — version {}\n\nUsage:\n    rustynes <ROM.nes>\n\nKeyboard:\n    Arrow keys -> D-pad\n    Z          -> A\n    X          -> B\n    Enter      -> Start\n    Right Shift -> Select\n    Esc        -> Quit",
        rustynes_core::version()
    );
}

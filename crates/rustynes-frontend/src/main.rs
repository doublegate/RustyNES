//! `RustyNES` frontend binary (native).
//!
//! v1.3.0 Sprint 1.2 — a thin shim over `lib.rs`, which owns the module tree.
//! The wasm32 entry point lives at `lib.rs::wasm::start` (gated
//! `#[cfg(target_arch = "wasm32")]`).
//!
//! v1.4.0 Workstream H — the hand-rolled argv parser was replaced with a clap 4
//! CLI (`cli.rs`), a `help [<topic>]` subcommand backed by a structured topic
//! registry, shell completions, and an interactive ratatui help browser
//! (`help_tui.rs`, default-on `help-tui` feature). All native-only; the clap /
//! ratatui / crossterm deps are gated out of the wasm target in `Cargo.toml`.
//!
//! See `docs/frontend.md` for the architecture.

// v1.3.0 Sprint 1.4 — this `[[bin]]` is the NATIVE desktop binary.
// The wasm32 frontend is the `cdylib` (`lib.rs` + `wasm.rs`), so when
// cargo builds this bin for the wasm32 target (it tries to build all
// targets) we compile an empty `main` instead — the real entry point
// is `wasm::start`.
#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
#[cfg(not(target_arch = "wasm32"))]
use std::process::ExitCode;

#[cfg(not(target_arch = "wasm32"))]
use clap::{ColorChoice, CommandFactory, Parser};

#[cfg(not(target_arch = "wasm32"))]
use rustynes_frontend::app;
#[cfg(not(target_arch = "wasm32"))]
use rustynes_frontend::cli::{self, Cli, CliCommand};

#[cfg(not(target_arch = "wasm32"))]
fn main() -> ExitCode {
    // clap parses argv, auto-handles `-h`/`--help`/`-V`/`--version` (exit 0),
    // and exits with code 2 on a usage error — preserving the historical
    // "bad arg -> exit 2" contract.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            // clap prints help/version to stdout (exit 0) and errors to stderr
            // (exit 2); `print()` + `exit_code()` reproduce that.
            let _ = e.print();
            return ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(2));
        }
    };

    match cli.command {
        Some(CliCommand::Help { topic, interactive }) => {
            run_help(topic.as_deref(), interactive, cli.color.into())
        }
        Some(CliCommand::Completions { shell }) => {
            print_completions(shell);
            ExitCode::SUCCESS
        }
        None => run_emulator(cli.rom),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn run_emulator(rom: Option<std::path::PathBuf>) -> ExitCode {
    // No ROM: the native binary has no bare-launch path (`app::run` always
    // loads a cartridge), so — preserving the historical behavior — print the
    // long help and exit with the usage code. Use the menu / F12 inside a
    // running session, or pass a ROM here. (A browser tab opens bare via the
    // separate wasm entry point.)
    let Some(rom_path) = rom else {
        let _ = Cli::command().print_long_help();
        println!();
        return ExitCode::from(2);
    };

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

/// `rustynes help [<topic>]` — print a styled topic page, or launch the
/// interactive browser when on a TTY (or `--interactive`).
#[cfg(not(target_arch = "wasm32"))]
fn run_help(topic: Option<&str>, interactive: bool, _color: ColorChoice) -> ExitCode {
    // A named topic that doesn't exist is a usage error (exit 2).
    if let Some(id) = topic
        && cli::topic(id).is_none()
    {
        eprintln!(
            "rustynes: unknown help topic '{id}'.\n  Valid topics: {}",
            cli::topic_ids()
        );
        return ExitCode::from(2);
    }

    // Launch the interactive TUI only when it's built in AND we're on a real
    // terminal (or the user forced it). Otherwise (piped output, CI, no topic
    // or a topic) fall through to the static page so `… | less` never blocks.
    #[cfg(feature = "help-tui")]
    {
        let on_tty = std::io::stdout().is_terminal() && std::io::stdin().is_terminal();
        if (topic.is_none() && on_tty) || interactive {
            return match rustynes_frontend::help_tui::run(topic) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("rustynes: help browser error: {e}");
                    ExitCode::from(1)
                }
            };
        }
    }
    #[cfg(not(feature = "help-tui"))]
    let _ = interactive;

    print_static_help(topic);
    ExitCode::SUCCESS
}

/// Print the static styled help page: one topic, or the index of all topics.
#[cfg(not(target_arch = "wasm32"))]
fn print_static_help(topic: Option<&str>) {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    if let Some(id) = topic {
        if let Some(t) = cli::topic(id) {
            let _ = writeln!(out, "{}\n", t.body);
        }
        return;
    }

    // No topic + non-interactive: list every topic, then their bodies.
    let _ = writeln!(out, "RustyNES help topics (use `rustynes help <topic>`):\n");
    for t in cli::HELP_TOPICS {
        let _ = writeln!(out, "  {:<10} {}", t.id, t.title);
    }
    let _ = writeln!(out);
    for t in cli::HELP_TOPICS {
        let _ = writeln!(out, "{}\n", t.body);
    }
}

/// `rustynes completions <shell>` — emit a completion script to stdout.
#[cfg(not(target_arch = "wasm32"))]
fn print_completions(shell: clap_complete::Shell) {
    let mut cmd = Cli::command();
    let bin = cmd.get_name().to_string();
    clap_complete::generate(shell, &mut cmd, bin, &mut std::io::stdout());
}

// `is_terminal` lives on `std::io::IsTerminal` (stable since 1.70).
#[cfg(all(not(target_arch = "wasm32"), feature = "help-tui"))]
use std::io::IsTerminal;

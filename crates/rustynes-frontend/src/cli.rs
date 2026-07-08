//! v1.4.0 "Fidelity" Workstream H — native CLI / terminal help & argument UX.
//!
//! Replaces the hand-rolled argv parser (`main.rs`, "no clap in v0") with a
//! clap 4 derive `Command` that preserves the historical contract:
//!
//! - `rustynes <ROM>` loads + runs the ROM (positional).
//! - `rustynes` with no ROM prints help and exits 2 (the native binary has no
//!   bare-launch path — `app::run` always loads a cartridge; this matches the
//!   pre-clap behavior). Load further ROMs from the menu / F12 inside a session.
//! - a bad argument exits with code 2 (clap's default usage-error code).
//!
//! It adds `rustynes help [<topic>]` (and `--interactive`), shell completions
//! (`rustynes completions <shell>`), `-V`/`--version`, and a styled `--help`.
//!
//! **Native-only.** The wasm entry point is an empty shim (a browser tab has no
//! terminal); the clap / ratatui / crossterm dep cluster is gated out of the
//! wasm target in `Cargo.toml`. This whole module is therefore declared under
//! `#[cfg(not(target_arch = "wasm32"))]` in `lib.rs`. Zero determinism surface:
//! everything here runs before any emulation.

use std::path::PathBuf;

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{ColorChoice, CommandFactory, Parser, Subcommand, ValueEnum};

/// Build the clap colour palette for the help / usage output.
///
/// Separated out so the clap `Command` carries it and so a test can
/// assert the styled help renders. clap + anstream honour `NO_COLOR` and the
/// `--color` choice automatically.
#[must_use]
pub fn cli_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Green.on_default() | Effects::BOLD)
        .usage(AnsiColor::Green.on_default() | Effects::BOLD)
        .literal(AnsiColor::Cyan.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::Cyan.on_default())
        .valid(AnsiColor::Green.on_default())
        .invalid(AnsiColor::Yellow.on_default())
        .error(AnsiColor::Red.on_default() | Effects::BOLD)
}

/// The colored "Examples" + "Keyboard" footer shown under `--help`.
///
/// `color-print`'s `cstr!` expands the HTML-like tags into ANSI at compile time;
/// clap renders the `&'static str` as a `StyledStr`
/// and strips the colour when output is not a TTY / `NO_COLOR` is set.
const AFTER_HELP: &str = color_print::cstr!(
    "<bold><underline>Examples:</underline></bold>
  <cyan>rustynes</cyan> <cyan!>game.nes</cyan!>            Load and run a ROM
  <cyan>rustynes</cyan> <cyan!>help mappers</cyan!>        Show the supported-mapper reference
  <cyan>rustynes</cyan> <cyan!>help</cyan!>                Browse all help topics (interactive on a TTY)
  <cyan>rustynes</cyan> <cyan!>completions fish</cyan!>    Print a shell-completion script

<bold><underline>Keyboard (P1):</underline></bold>
  <cyan!>Arrows</cyan!> D-pad   <cyan!>Z</cyan!> A   <cyan!>X</cyan!> B   <cyan!>Enter</cyan!> Start   <cyan!>RShift</cyan!> Select
  <cyan!>F1</cyan!> save state   <cyan!>F4</cyan!> load state   <cyan!>F12</cyan!> open ROM   <cyan!>Esc</cyan!> quit
  Run <cyan>rustynes help hotkeys</cyan> for the full table.

See <cyan>rustynes help &lt;topic&gt;</cyan> for: controls, hotkeys, gamepad, features, mappers, config, scripting, netplay, about."
);

/// `RustyNES` — a cycle-accurate Nintendo Entertainment System emulator.
#[derive(Debug, Parser)]
#[command(
    name = "rustynes",
    bin_name = "rustynes",
    version,
    author,
    about = "RustyNES — a cycle-accurate NES / Famicom emulator (winit + wgpu + cpal + egui).",
    long_about = "RustyNES — a cycle-accurate Nintendo Entertainment System emulator written in \
                  pure Rust.\n\nPass a ROM path to load and run it. Once a session is open you \
                  can load further ROMs from the File menu, with F12, or by drag-and-drop.",
    after_help = AFTER_HELP,
    styles = cli_styles(),
    disable_help_subcommand = true,
)]
pub struct Cli {
    /// Path to the `.nes` / `.fds` / `.zip` / `.nsf` ROM to load and run.
    ///
    /// Load further ROMs from the File menu, with F12, or by drag-and-drop
    /// once a session is open.
    #[arg(value_name = "ROM", value_hint = clap::ValueHint::FilePath)]
    pub rom: Option<PathBuf>,

    /// Control when colored output is used (also honours `NO_COLOR`).
    #[arg(long, value_name = "WHEN", value_enum, default_value_t = ColorWhen::Auto, global = true)]
    pub color: ColorWhen,

    /// Subcommands: `help [<topic>]`, `completions <shell>`.
    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

/// `--color` choices, mapped onto clap's `ColorChoice`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum ColorWhen {
    /// Colour when stdout is a terminal (the default).
    #[default]
    Auto,
    /// Always emit ANSI colour.
    Always,
    /// Never emit colour.
    Never,
}

impl From<ColorWhen> for ColorChoice {
    fn from(w: ColorWhen) -> Self {
        match w {
            ColorWhen::Auto => Self::Auto,
            ColorWhen::Always => Self::Always,
            ColorWhen::Never => Self::Never,
        }
    }
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Show help for a topic, or browse all topics interactively on a TTY.
    Help {
        /// Topic to print (controls, hotkeys, gamepad, features, mappers,
        /// config, scripting, netplay, about). Omit to browse everything.
        #[arg(value_name = "TOPIC")]
        topic: Option<String>,

        /// Force the interactive TUI browser even on a non-default stream.
        #[arg(long)]
        interactive: bool,
    },

    /// Print a shell-completion script to stdout.
    Completions {
        /// Target shell.
        #[arg(value_name = "SHELL", value_enum)]
        shell: clap_complete::Shell,
    },
}

/// Parse `std::env::args`, returning the typed [`Cli`].
///
/// Mirrors clap's `Parser::parse` but lets callers (and tests) inject argv.
#[must_use]
pub fn parse_from<I, T>(args: I) -> Cli
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    Cli::parse_from(args)
}

/// The clap `Command`, for `debug_assert`, completions, and help.
#[must_use]
pub fn command() -> clap::Command {
    Cli::command()
}

// ===========================================================================
// Help-topic registry — the SINGLE content source shared by the static
// `rustynes help <topic>` page (H2) and the interactive ratatui browser (H3).
// Kept in sync with `docs/frontend.md`, the README, and the in-app
// "Keyboard Shortcuts" window (`ui_shell::shortcuts_grid`).
// ===========================================================================

/// One help topic: a stable id (the CLI arg + completion candidate), a short
/// title for the TUI tab/list, and the rendered body.
#[derive(Debug, Clone, Copy)]
pub struct HelpTopic {
    /// CLI argument / lookup key (e.g. `"mappers"`). Lowercase, stable.
    pub id: &'static str,
    /// Human title for the TUI tab row / topic list.
    pub title: &'static str,
    /// The topic body (plain text; the TUI colours headings heuristically).
    pub body: &'static str,
}

/// All help topics, in display order. The first entry is the landing topic.
pub const HELP_TOPICS: &[HelpTopic] = &[
    HelpTopic {
        id: "controls",
        title: "Controls",
        body: CONTROLS_BODY,
    },
    HelpTopic {
        id: "hotkeys",
        title: "Hotkeys",
        body: HOTKEYS_BODY,
    },
    HelpTopic {
        id: "gamepad",
        title: "Gamepad",
        body: GAMEPAD_BODY,
    },
    HelpTopic {
        id: "features",
        title: "Features",
        body: FEATURES_BODY,
    },
    HelpTopic {
        id: "mappers",
        title: "Mappers",
        body: MAPPERS_BODY,
    },
    HelpTopic {
        id: "config",
        title: "Config",
        body: CONFIG_BODY,
    },
    HelpTopic {
        id: "scripting",
        title: "Scripting",
        body: SCRIPTING_BODY,
    },
    HelpTopic {
        id: "netplay",
        title: "Netplay",
        body: NETPLAY_BODY,
    },
    HelpTopic {
        id: "about",
        title: "About",
        body: ABOUT_BODY,
    },
];

/// Look up a topic by its `id` (case-insensitive).
#[must_use]
pub fn topic(id: &str) -> Option<&'static HelpTopic> {
    let id = id.trim().to_ascii_lowercase();
    HELP_TOPICS.iter().find(|t| t.id == id)
}

/// Comma-separated list of valid topic ids (for error messages).
#[must_use]
pub fn topic_ids() -> String {
    HELP_TOPICS
        .iter()
        .map(|t| t.id)
        .collect::<Vec<_>>()
        .join(", ")
}

const CONTROLS_BODY: &str = "\
Default keyboard controls
=========================

Player 1
  D-pad ............. Arrow keys
  A ................. Z
  B ................. X
  Start ............. Enter
  Select ............ Right Shift

Player 2
  D-pad ............. W A S D
  A ................. Q
  B ................. E
  Start ............. P
  Select ............ L

All keys are remappable in Settings -> Input (persisted to config.toml).
Run `rustynes help hotkeys` for the system / emulation hotkeys, or
`rustynes help gamepad` for controller bindings.";

const HOTKEYS_BODY: &str = "\
System & emulation hotkeys
==========================

  F1 ......... Save state (current slot)
  F4 ......... Load state (current slot)
  F5 (hold) .. Rewind
  F2 ......... Reset (warm)
  F3 ......... Power cycle (cold boot)
  F6 ......... TAS movie: record
  F7 ......... TAS movie: play
  F8 ......... TAS movie: branch
  F9 ......... Swap disk side (FDS)
  F10 ........ Insert coin (Vs. System)
  F11 ........ Toggle fullscreen
  F12 ........ Open ROM
  M .......... Toggle the menu bar
  ` .......... Toggle the debugger overlay
  Esc ........ Quit / exit fullscreen

You can also drag-and-drop a .nes / .fds / .zip onto the window to load it.
These accelerators are remappable under [input.system] in config.toml.";

const GAMEPAD_BODY: &str = "\
Gamepad support
===============

USB / Bluetooth gamepads auto-bind to Player 1 (Xbox-style layout):

  South (A) ...... NES A
  West (X) ....... NES B
  Start .......... Start
  Back / Select .. Select
  D-pad .......... D-pad

Multiple pads bind to P1..P4 in connection order. Rebind any button in
Settings -> Input. Turbo / autofire is configurable per button.";

const FEATURES_BODY: &str = "\
Feature highlights
==================

Accuracy
  Cycle-accurate one-clock, every-cycle-bus-access scheduler (v2.0.0 Timebase);
  AccuracyCoin 98.58% (139/141), nestest 0-diff, blargg / kevtris suites green.

Cartridges & platforms
  101+ mapper families, Famicom Disk System, Vs. System / PlayChoice-10,
  region timing (NTSC / PAL / Dendy) as data.

Modern features
  Rollback netplay (native UDP + browser WebRTC), RetroAchievements
  (opt-in, native), TAS movies (.rnm), save-states, rewind, run-ahead,
  Game Genie + raw-RAM cheats, Four Score.

Video / audio
  NES_NTSC + composable CRT / scanline shader stack, .pal palette loading,
  5-band EQ, per-channel mixing, NSF / NSFe player.

Tooling
  egui debugger overlay (CPU / PPU / APU / memory / OAM / mapper panels),
  Lua 5.4 scripting engine, an in-app ROM-database editor.

Build the optional features with cargo: --features retroachievements,
scripting, hd-pack (see `rustynes help about`).";

const MAPPERS_BODY: &str = "\
Mapper support
==============

RustyNES implements 101+ mapper families across three accuracy tiers
(ADR 0011, CI honesty-gated):

  Core ........ Fully cycle-accurate, oracle-verified (NROM, MMC1, UxROM,
                CNROM, MMC3, MMC5, AxROM, ...).
  Curated ..... Verified against commercial dumps + test ROMs.
  BestEffort .. Register-decode + save-state tested; boots, may have edge
                cases.

Includes expansion-audio mappers (VRC6, VRC7/OPLL, MMC5, Namco 163,
Sunsoft 5B), the Famicom Disk System, and the Vs. System / PlayChoice-10
RGB PPU variants. See docs/mappers.md for the full per-mapper table and the
IRQ-family matrix.";

const CONFIG_BODY: &str = "\
Configuration
=============

Settings live in a TOML file under the platform config directory:

  Linux ...... ~/.config/rustynes/config.toml
  macOS ...... ~/Library/Application Support/rustynes/config.toml
  Windows .... %APPDATA%\\rustynes\\config.toml

Save-state slots, the ROM database, and netplay-deploy assets live under the
matching data directory. Most settings are editable in-app (Settings window,
auto-saved per change); the TOML is the source of truth on next launch. Unknown
keys are preserved and the prior file backed up to config.toml.bak on upgrade.

Shell completions: `rustynes completions <bash|zsh|fish|powershell>`.";

const SCRIPTING_BODY: &str = "\
Lua scripting
=============

RustyNES embeds a sandboxed Lua 5.4 engine (native, behind the off-by-default
`scripting` cargo feature). Scripts can read CPU / PPU / memory state, draw
overlays, and register callbacks: onFrame, onNmi, onIrq, setInput (the last two
and emu.write are gated). The browser build has an experimental pure-Rust
piccolo backend behind the `script-wasm` feature (NOT byte-parity with native;
ADR 0012).

See docs/scripting.md for the full API. Enable it with:
  cargo run -p rustynes-frontend --features scripting -- game.nes";

const NETPLAY_BODY: &str = "\
Netplay
=======

GGPO-style rollback netcode for 2-4 players:

  Native ..... UDP transport (host / join from the Netplay menu).
  Browser .... WebRTC data channel (the hosted demo + a signaling server).

Dynamic rate control and run-ahead live in the frontend, so the deterministic
core stays bit-identical across peers. A turn-key host/TURN bundle ships under
deploy/. See docs/netplay-webrtc.md for the browser connectivity matrix.";

const ABOUT_BODY: &str = "\
About RustyNES
==============

A cycle-accurate NES / Famicom emulator written in pure Rust. The frontend is
winit + wgpu + cpal + egui; the chip stack (CPU / PPU / APU / mappers) is
no_std + alloc and fuzzable in isolation.

  License .... MIT OR Apache-2.0
  Author ..... DoubleGate <parobek@gmail.com>
  Repo ....... https://github.com/doublegate/RustyNES
  Web demo ... https://doublegate.github.io/RustyNES/

Optional cargo features: retroachievements (opt-in cheevos), scripting (Lua),
hd-pack (Mesen-style HD packs), help-tui (this interactive help browser; on by
default). Run `rustynes --version` for the build version.";

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn cli_command_is_valid() {
        // clap's own structural self-check (panics on a malformed Command).
        command().debug_assert();
    }

    #[test]
    fn positional_rom_parses() {
        let cli = Cli::try_parse_from(["rustynes", "game.nes"]).unwrap();
        assert_eq!(cli.rom.as_deref(), Some(std::path::Path::new("game.nes")));
        assert!(cli.command.is_none());
    }

    #[test]
    fn no_args_means_no_rom_no_command() {
        let cli = Cli::try_parse_from(["rustynes"]).unwrap();
        assert!(cli.rom.is_none());
        assert!(cli.command.is_none());
    }

    #[test]
    fn color_flag_parses() {
        let cli = Cli::try_parse_from(["rustynes", "--color", "never", "game.nes"]).unwrap();
        assert_eq!(cli.color, ColorWhen::Never);
        assert_eq!(ColorChoice::from(cli.color), ColorChoice::Never);
    }

    #[test]
    fn help_subcommand_with_topic_parses() {
        let cli = Cli::try_parse_from(["rustynes", "help", "mappers"]).unwrap();
        match cli.command {
            Some(CliCommand::Help {
                topic: Some(t),
                interactive,
            }) => {
                assert_eq!(t, "mappers");
                assert!(!interactive);
            }
            other => panic!("expected help mappers, got {other:?}"),
        }
    }

    #[test]
    fn help_subcommand_bare_parses() {
        let cli = Cli::try_parse_from(["rustynes", "help"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(CliCommand::Help { topic: None, .. })
        ));
    }

    #[test]
    fn completions_subcommand_parses() {
        let cli = Cli::try_parse_from(["rustynes", "completions", "bash"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(CliCommand::Completions {
                shell: clap_complete::Shell::Bash
            })
        ));
    }

    #[test]
    fn bad_argument_is_usage_error() {
        // An unknown flag must be a usage error → exit code 2 in main().
        let err = Cli::try_parse_from(["rustynes", "--definitely-not-a-flag"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::UnknownArgument);
        // clap maps usage errors to process exit code 2.
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn bad_completions_shell_is_usage_error() {
        let err = Cli::try_parse_from(["rustynes", "completions", "tcsh"]).unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn help_topic_registry_is_complete_and_unique() {
        // Every advertised topic resolves, and ids are unique + lowercase.
        let advertised = [
            "controls",
            "hotkeys",
            "gamepad",
            "features",
            "mappers",
            "config",
            "scripting",
            "netplay",
            "about",
        ];
        for id in advertised {
            assert!(topic(id).is_some(), "missing help topic: {id}");
        }
        assert_eq!(
            HELP_TOPICS.len(),
            advertised.len(),
            "HELP_TOPICS drifted from the advertised set"
        );
        let mut seen = std::collections::HashSet::new();
        for t in HELP_TOPICS {
            assert_eq!(
                t.id,
                t.id.to_ascii_lowercase(),
                "topic id must be lowercase"
            );
            assert!(seen.insert(t.id), "duplicate topic id: {}", t.id);
            assert!(!t.title.is_empty());
            assert!(!t.body.is_empty(), "empty body for topic {}", t.id);
        }
    }

    #[test]
    fn topic_lookup_is_case_insensitive() {
        assert!(topic("MAPPERS").is_some());
        assert!(topic("  About ").is_some());
        assert!(topic("nope").is_none());
    }

    #[test]
    fn topic_ids_lists_every_topic() {
        let ids = topic_ids();
        for t in HELP_TOPICS {
            assert!(ids.contains(t.id), "{} missing from topic_ids()", t.id);
        }
    }
}

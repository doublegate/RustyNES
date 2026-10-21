//! v3.5.0 (`T-SPEEDRUN-MEMIF`) — `rustynes speedrun`: time a movie (or a
//! fixed number of input-free frames) headlessly against a splits file.
//!
//! Like `rustynes verify` it builds a bare console with no window, audio or
//! host input, so the answer depends only on the ROM, the movie and the
//! deterministic core: the same movie gives the same split times on every
//! machine. With `--memif` (a `memif` build) it also publishes the
//! memory-export block every frame, and `--memif-hold-ms` keeps the block
//! alive after the run so a reader in another process can take the final
//! state; that is how `scripts/memif/two_build_check.sh` drives the reference
//! splitter.

use std::path::PathBuf;

use rustynes_core::{Movie, MoviePlayer, Nes};

use crate::speedrun_host::{SpeedrunHost, SpeedrunOptions, describe};

/// The command's arguments (parsed by `cli.rs`).
#[derive(Clone, Debug)]
pub struct SpeedrunArgs {
    /// The ROM.
    pub rom: PathBuf,
    /// The splits definition.
    pub splits: PathBuf,
    /// The movie to replay; without one, `frames` input-free frames run.
    pub movie: Option<PathBuf>,
    /// Frames to run without a movie (default 216,000, one NTSC hour).
    pub frames: Option<u64>,
    /// Write the result document here as well as to stdout.
    pub json: Option<PathBuf>,
    /// Publish the memory-export block under this name.
    pub memif: Option<String>,
    /// Keep the block published this long after the run ends.
    pub memif_hold_ms: u64,
}

/// Exit status: the run finished.
pub const EXIT_FINISHED: u8 = 0;
/// Exit status: an error (a file, the ROM, the movie, the splits file).
pub const EXIT_ERROR: u8 = 1;
/// Exit status: the input ended before the last split fired.
pub const EXIT_UNFINISHED: u8 = 2;

/// Run the command; returns the exit status.
#[must_use]
pub fn run(args: &SpeedrunArgs) -> u8 {
    match run_inner(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("rustynes speedrun: {e}");
            EXIT_ERROR
        }
    }
}

fn run_inner(args: &SpeedrunArgs) -> Result<u8, String> {
    let rom =
        std::fs::read(&args.rom).map_err(|e| format!("cannot read {}: {e}", args.rom.display()))?;
    let mut nes =
        Nes::from_rom(&rom).map_err(|e| format!("cannot load {}: {e}", args.rom.display()))?;
    let mut host = SpeedrunHost::new(&SpeedrunOptions {
        splits: Some(args.splits.clone()),
        splits_json: args.json.clone(),
        memif: args.memif.clone(),
    })?;
    if let Some(name) = host.memif_name() {
        eprintln!("publishing the memory-export block {name:?}");
    }

    let mut finished = false;
    if let Some(path) = &args.movie {
        let bytes =
            std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let movie = Movie::deserialize(&bytes)
            .map_err(|e| format!("{} is not a valid movie: {e}", path.display()))?;
        movie
            .seek_to_start(&mut nes)
            .map_err(|e| format!("{} cannot start on this ROM: {e}", path.display()))?;
        let mut player = MoviePlayer::new(&movie);
        while player.apply_next(&mut nes) {
            if step(&mut nes, &mut host) {
                finished = true;
                break;
            }
        }
    } else {
        for _ in 0..args.frames.unwrap_or(216_000) {
            if step(&mut nes, &mut host) {
                finished = true;
                break;
            }
        }
    }

    if let Some(t) = host.timer() {
        print!("{}", t.to_json());
        if !finished && let Some(path) = &args.json {
            // A finish writes it from the host; an unfinished run still gets
            // its partial result written, so a script always finds the file.
            crate::atomic_write::write_atomic(path, t.to_json().as_bytes())
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        }
    }
    if args.memif.is_some() && args.memif_hold_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(args.memif_hold_ms));
    }
    Ok(if finished {
        EXIT_FINISHED
    } else {
        EXIT_UNFINISHED
    })
}

/// Run one frame, time it, print its event; `true` once the run finished.
fn step(nes: &mut Nes, host: &mut SpeedrunHost) -> bool {
    nes.run_frame();
    if let Some(event) = host.after_frame(nes)
        && let Some(t) = host.timer()
    {
        println!("{}", describe(event, t, nes));
        return event == rustynes_core::speedrun::TimerEvent::Finished;
    }
    false
}

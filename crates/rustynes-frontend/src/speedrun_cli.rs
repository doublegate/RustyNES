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
///
/// Not 2, which is clap's usage error, nor 3 (`verify`'s "no attestation") or 4
/// (`dump-check`'s "a finding"), so a script can tell every outcome apart; it
/// was 2 until v3.5.0's review (`CodeRabbit` on #624).
pub const EXIT_UNFINISHED: u8 = 5;

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
    let rom = crate::bounded_read::read(&args.rom)
        .map_err(|e| format!("cannot read {}: {e}", args.rom.display()))?;
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
        let bytes = crate::bounded_read::read(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
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
        if let Some(path) = &args.json {
            // Written here for BOTH outcomes, so a failed write is an error
            // exit. The host also writes it when the run finishes, but only
            // logs a failure there (a live session must not stop over it);
            // until v3.5.0's review this skipped finished runs, so an
            // unwritable path exited 0 with no file (Copilot on #624). An
            // unfinished run gets its partial result, so a script always
            // finds the file.
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
    drop_audio(nes);
    if let Some(event) = host.after_frame(nes)
        && let Some(t) = host.timer()
    {
        println!("{}", describe(event, t, nes));
        return event == rustynes_core::speedrun::TimerEvent::Finished;
    }
    false
}

/// Discard the frame's audio: nothing here plays it, and the core keeps every
/// sample until drained (about 800 a frame, so an hour's run would hold some
/// 600 MiB; `Copilot` on #624). `drain_audio_into` keeps what does not fit, so
/// this loops until a drain comes back short.
fn drop_audio(nes: &mut Nes) {
    let mut sink = [0.0f32; 1024];
    while nes.drain_audio_into(&mut sink) == sink.len() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal NROM image (16 KiB PRG + 8 KiB CHR, all zero).
    fn synth_nrom() -> Vec<u8> {
        let mut rom = vec![0x4E, 0x45, 0x53, 0x1A, 1, 1, 0, 0];
        rom.extend(std::iter::repeat_n(0u8, 8));
        rom.extend(std::iter::repeat_n(0u8, 16 * 1024));
        rom.extend(std::iter::repeat_n(0u8, 8 * 1024));
        rom
    }

    /// The timer reads the same addresses an achievement does: its mapping
    /// and the achievements client's agree over the whole RA space and past
    /// it. The speedrun module's docs promised this pin before it existed.
    #[cfg(feature = "retroachievements")]
    #[test]
    fn the_timer_maps_addresses_as_the_achievements_client_does() {
        for addr in 0..=0x1_0010u32 {
            assert_eq!(
                rustynes_core::speedrun::ra_flat_to_cpu(addr),
                rustynes_cheevos::ra_addr_to_nes(addr),
                "address {addr:#x}"
            );
        }
    }

    /// Each step leaves nothing buffered. Mutation-checked by deleting the
    /// `drop_audio` call in `step`.
    #[test]
    fn a_step_leaves_no_audio_buffered() {
        let mut nes = Nes::from_rom(&synth_nrom()).expect("loads");
        let mut host = SpeedrunHost::new(&SpeedrunOptions::default()).expect("host");
        for _ in 0..30 {
            let _ = step(&mut nes, &mut host);
        }
        assert_eq!(nes.drain_audio().len(), 0);
    }

    /// A FINISHED run whose `--json` path cannot be written is an error, not
    /// exit 0 with no file. Mutation-checked by restoring the `!finished`
    /// guard on the CLI's write.
    #[test]
    fn a_finished_run_reports_an_unwritable_result_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rom = dir.path().join("t.nes");
        std::fs::write(&rom, synth_nrom()).expect("rom");
        let splits = dir.path().join("t.splits");
        // $0000 stays zero on this ROM, so the run starts and finishes at once.
        std::fs::write(
            &splits,
            "game = T\ncategory = Any%\nstart = 0xH0000=0\nsplit = End | 0xH0000=0\n",
        )
        .expect("splits");
        let base = SpeedrunArgs {
            rom,
            splits,
            movie: None,
            frames: Some(30),
            json: None,
            memif: None,
            memif_hold_ms: 0,
        };
        assert_eq!(
            run_inner(&base),
            Ok(EXIT_FINISHED),
            "the fixture must finish"
        );
        // A missing directory would be created (`write_atomic` makes parents),
        // so the parent here is a regular file, which no write can pass.
        let blocker = dir.path().join("a-file");
        std::fs::write(&blocker, b"").expect("blocker");
        let unwritable = SpeedrunArgs {
            json: Some(blocker.join("out.json")),
            ..base
        };
        let err = run_inner(&unwritable).expect_err("an unwritable path is an error");
        assert!(err.starts_with("cannot write"), "{err}");
    }
}

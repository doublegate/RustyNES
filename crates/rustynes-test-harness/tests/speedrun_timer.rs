//! v3.5.0 (`T-SPEEDRUN-MEMIF`) — the first half of the plan gate: "a run
//! timed from a movie matches its cycle count on every replay".
//!
//! A synthetic NROM program (assembled below, ours, no third-party code)
//! waits for the PPU to warm up, enables NMI, and in its NMI handler counts
//! frames into `$11` and reads controller 1 into `$10` (A in bit 7 ... Right
//! in bit 0, the order the shift register delivers them). A movie presses
//! Start, then B, then A; the splits file starts on the Start edge, splits on
//! B, finishes on A. The run is timed while it is recorded, then the movie is
//! serialized, and three replays from those bytes must each give the same
//! start, split and finish clocks. The total must also equal an independent
//! count: the CPU cycles between the start and finish frames times the NTSC
//! divider, 12 (the affine relation `one_clock_invariants.rs` pins between
//! the master clock and the cycle counter).
//!
//! `write_fixture` (ignored; run by `scripts/memif/two_build_check.sh`) writes
//! the same ROM, splits file and movie to a directory for the second half of
//! the gate.

use rustynes_core::speedrun::{SpeedrunTimer, SplitsDefinition, TimerEvent, TimerState};
use rustynes_core::{Buttons, FrameInput, Movie, MoviePlayer, MovieRecorder, Nes, Region};

/// The program: reset at `$C000`, NMI at `$C01D`.
fn synth_rom() -> Vec<u8> {
    #[rustfmt::skip]
    let code: &[u8] = &[
        0x78,             // C000 SEI
        0xD8,             // C001 CLD
        0xA2, 0xFF,       // C002 LDX #$FF
        0x9A,             // C004 TXS
        0xA9, 0x00,       // C005 LDA #$00
        0x85, 0x10,       // C007 STA $10
        0x85, 0x11,       // C009 STA $11
        0x2C, 0x02, 0x20, // C00B BIT $2002   ; wait for vblank (twice)
        0x10, 0xFB,       // C00E BPL $C00B
        0x2C, 0x02, 0x20, // C010 BIT $2002
        0x10, 0xFB,       // C013 BPL $C010
        0xA9, 0x80,       // C015 LDA #$80
        0x8D, 0x00, 0x20, // C017 STA $2000   ; NMI on
        0x4C, 0x1A, 0xC0, // C01A JMP $C01A
        // NMI:
        0xE6, 0x11,       // C01D INC $11     ; frame counter
        0xA9, 0x01,       // C01F LDA #$01
        0x8D, 0x16, 0x40, // C021 STA $4016   ; strobe
        0xA9, 0x00,       // C024 LDA #$00
        0x8D, 0x16, 0x40, // C026 STA $4016
        0xA2, 0x08,       // C029 LDX #$08
        0xAD, 0x16, 0x40, // C02B LDA $4016
        0x4A,             // C02E LSR A
        0x26, 0x10,       // C02F ROL $10
        0xCA,             // C031 DEX
        0xD0, 0xF7,       // C032 BNE $C02B
        0x40,             // C034 RTI
    ];
    let mut rom = vec![b'N', b'E', b'S', 0x1A, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut prg = vec![0xEAu8; 16 * 1024];
    prg[..code.len()].copy_from_slice(code);
    let n = prg.len();
    // NMI $C01D, RESET $C000, IRQ $C000.
    prg[n - 6..].copy_from_slice(&[0x1D, 0xC0, 0x00, 0xC0, 0x00, 0xC0]);
    rom.extend_from_slice(&prg);
    rom.extend_from_slice(&[0u8; 8 * 1024]);
    rom
}

/// Start on the Start edge (bit 4), split on B (bit 6, `0xS` — which the
/// tokenizer must not read as a group separator), finish on A (bit 7).
const SPLITS: &str = "\
game = Synthetic
category = Buttons%
start = 0xQ0010=1_d0xQ0010=0
split = B pressed | 0xS0010=1
split = A pressed | 0xT0010=1
";

/// Frame `i`'s player-1 buttons.
const fn input(i: usize) -> Buttons {
    match i {
        30..=31 => Buttons::START,
        80 => Buttons::B,
        150..=151 => Buttons::A,
        _ => Buttons::empty(),
    }
}

const FRAMES: usize = 200;

/// The timeline of one run: each event, with the master clock and the CPU
/// cycle counter at the end of its frame.
#[derive(Debug, PartialEq, Eq)]
struct Run {
    events: Vec<(TimerEvent, u64, u64)>,
    elapsed: u64,
    splits: Vec<u64>,
}

fn observe(timer: &mut SpeedrunTimer, nes: &mut Nes, run: &mut Run) {
    if let Some(e) = timer.observe_nes(nes) {
        run.events.push((e, nes.cpu().master_clock(), nes.cycle()));
    }
}

fn record() -> (Run, Vec<u8>) {
    let rom = synth_rom();
    let mut nes = Nes::from_rom(&rom).expect("synthetic ROM loads");
    nes.power_cycle();
    let mut rec = MovieRecorder::power_on(&nes);
    let def = SplitsDefinition::parse(SPLITS).expect("splits parse");
    let mut timer = SpeedrunTimer::new(def, Region::Ntsc);
    let mut run = Run {
        events: Vec::new(),
        elapsed: 0,
        splits: Vec::new(),
    };
    for i in 0..FRAMES {
        FrameInput::new(input(i), Buttons::empty()).apply_to(&mut nes);
        rec.capture(&nes);
        nes.run_frame();
        observe(&mut timer, &mut nes, &mut run);
    }
    run.elapsed = timer.elapsed();
    run.splits = timer.splits().iter().map(|s| s.master_clock).collect();
    assert_eq!(timer.state(), TimerState::Finished, "{:?}", run.events);
    (run, rec.finish().serialize())
}

fn replay(bytes: &[u8]) -> Run {
    let movie = Movie::deserialize(bytes).expect("movie round-trips");
    let mut nes = Nes::from_rom(&synth_rom()).unwrap();
    movie.seek_to_start(&mut nes).expect("start point");
    let mut timer = SpeedrunTimer::new(SplitsDefinition::parse(SPLITS).unwrap(), Region::Ntsc);
    let mut run = Run {
        events: Vec::new(),
        elapsed: 0,
        splits: Vec::new(),
    };
    let mut player = MoviePlayer::new(&movie);
    while player.apply_next(&mut nes) {
        nes.run_frame();
        observe(&mut timer, &mut nes, &mut run);
    }
    run.elapsed = timer.elapsed();
    run.splits = timer.splits().iter().map(|s| s.master_clock).collect();
    run
}

#[test]
fn a_run_timed_from_a_movie_matches_its_cycle_count_on_every_replay() {
    let (recorded, bytes) = record();
    let kinds: Vec<TimerEvent> = recorded.events.iter().map(|e| e.0).collect();
    assert_eq!(
        kinds,
        [
            TimerEvent::Started,
            TimerEvent::Split(0),
            TimerEvent::Finished
        ],
        "the program and the movie drive exactly one run"
    );
    // The independent count: CPU cycles between the start and finish
    // frames, times the NTSC divider.
    let (_, start_clock, start_cycles) = recorded.events[0];
    let (_, end_clock, end_cycles) = recorded.events[2];
    assert_eq!(recorded.elapsed, end_clock - start_clock);
    assert_eq!(
        recorded.elapsed,
        (end_cycles - start_cycles) * 12,
        "the time is the CPU cycles run, in master clocks"
    );
    // 120 frames from the Start edge (frame 30) to the A press (frame 150):
    // about two NTSC seconds, nowhere near a frame count times a constant.
    let secs = rustynes_core::speedrun::cycles_to_seconds(recorded.elapsed, Region::Ntsc);
    assert!((1.95..2.05).contains(&secs), "{secs} s");
    for replay_no in 1..=3 {
        let replayed = replay(&bytes);
        assert_eq!(replayed, recorded, "replay {replay_no} differs");
    }
}

/// The same run, observed with the timer attached and detached, produces the
/// same console: the timer reads and never writes (the determinism contract).
#[test]
fn timing_a_run_does_not_change_it() {
    let rom = synth_rom();
    let run = |timed: bool| {
        let mut nes = Nes::from_rom(&rom).unwrap();
        nes.power_cycle();
        let mut timer = SpeedrunTimer::new(SplitsDefinition::parse(SPLITS).unwrap(), Region::Ntsc);
        let mut fb_hash = 0xCBF2_9CE4_8422_2325u64;
        for i in 0..FRAMES {
            FrameInput::new(input(i), Buttons::empty()).apply_to(&mut nes);
            for &b in nes.run_frame() {
                fb_hash = (fb_hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3);
            }
            if timed {
                let _ = timer.observe_nes(&mut nes);
            }
        }
        (fb_hash, nes.cpu().master_clock(), nes.wram().to_vec())
    };
    assert_eq!(run(true), run(false));
}

/// Not a test on its own: writes the fixture `scripts/memif/two_build_check.sh`
/// feeds both builds (the ROM, the splits file, the movie, and the expected
/// result) into `$RUSTYNES_SPEEDRUN_FIXTURE_DIR`.
#[test]
#[ignore = "fixture writer for scripts/memif/two_build_check.sh"]
fn write_fixture() {
    let Ok(dir) = std::env::var("RUSTYNES_SPEEDRUN_FIXTURE_DIR") else {
        return;
    };
    let dir = std::path::Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    let (run, movie) = record();
    std::fs::write(dir.join("synthetic.nes"), synth_rom()).unwrap();
    std::fs::write(dir.join("buttons.rnsplits"), SPLITS).unwrap();
    std::fs::write(dir.join("buttons.rnm"), movie).unwrap();
    let start = run.events[0].1;
    let splits: Vec<String> = run.splits.iter().map(|c| (c - start).to_string()).collect();
    std::fs::write(
        dir.join("expected.txt"),
        format!("elapsed={}\nsplits={}\n", run.elapsed, splits.join(",")),
    )
    .unwrap();
}

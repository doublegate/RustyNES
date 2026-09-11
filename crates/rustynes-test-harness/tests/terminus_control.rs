//! v2.6.18 "Terminus" — the FIRST-DIFFERENCE control, on the SHIPPED path.
//!
//! Deliverable A of `to-dos/plans/v2.6.18-terminus-plan.md`. The version moves
//! when a CPU access is applied relative to the PPU dot grid, which shifts
//! every PPU register write in every game. The acceptance question is not "did
//! the battery go up" but "is the first thing that changed the thing I aimed
//! at, and nothing earlier" — so a baseline has to exist BEFORE the move.
//!
//! # Why this is not the co-simulation checkpoint chain
//!
//! The obvious instrument is `nes_golden_export`'s rolling per-cycle hash, and
//! it was built, committed, and then DELETED, because a mutation against it
//! came back NOT CAUGHT: moving the write commit two master clocks produced a
//! byte-identical chain. The reason is structural. Those chains require
//! `--irq-trace`, `irq-timing-trace` selects a different `for sub_dot in 0..3`
//! loop in `Bus::tick_one_cpu_cycle` (the v2.4.1 finding), and that loop does
//! not go through `write_split` at all. The instrument is blind to the exact
//! change this version exists to make — agreement about an unasked question.
//!
//! So the control runs the DEFAULT feature set, which is the scheduler that
//! ships, and hashes what a user can observe: the pre-palette framebuffer and
//! CPU work RAM, per frame, folded into a rolling FNV-1a. The per-frame fold
//! is what makes it a FIRST-difference control rather than a pass/fail one:
//! `TERMINUS_DUMP=<dir>` writes the per-frame chain so the earliest differing
//! frame can be read off directly.
//!
//! # What it is measured to catch, and what it is measured NOT to
//!
//! Demonstrated by mutation, moving the write commit to phi2 -- the exact
//! change v2.6.18 exists to make:
//!
//! * `nestest` MOVES. The control is not blind.
//! * `ppu_vbl_nmi/01-vbl_basics` and `sprite_hit_tests/01.basics` do NOT move,
//!   at 60 frames or at 400.
//!
//! The second half is a finding, not a gap to paper over: those two workloads
//! are measurably INSENSITIVE to a two-master-clock shift in the write commit,
//! so the corpus that has to be re-baselined when the dot grid moves is
//! narrower than "everything with a PPU write in it". Recording which
//! workloads do not move is as useful as recording which do — it is what keeps
//! a re-baseline from being blessed wholesale.
//!
//! Coverage is therefore stated rather than implied: this control proves the
//! shipped scheduler is unchanged on THESE THREE workloads. It is not a
//! substitute for the battery, the commercial oracle, or the visual corpus.
use std::fmt::Write as _;

use rustynes_core::Nes;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fold(h: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *h ^= u64::from(*b);
        *h = h.wrapping_mul(FNV_PRIME);
    }
}

/// `(rom, frames)` — chosen for what they exercise, not for coverage breadth.
///
/// The first three write `$2000`/`$2001` constantly mid-frame, which is the
/// traffic the dot-grid move perturbs. `nestest` is the deliberate CONTROL:
/// it is CPU-bound with almost no mid-frame PPU register traffic, so it should
/// be the LEAST disturbed. A move that shifts nestest as much as the others is
/// not the move that was intended.
const WORKLOADS: &[(&str, u32)] = &[
    ("tests/roms/blargg/ppu_vbl_nmi/01-vbl_basics.nes", 400),
    ("tests/roms/blargg/sprite_hit_tests/01.basics.nes", 400),
    ("tests/roms/nestest/nestest.nes", 400),
];

/// 400 frames, not 60, and that number is a MEASUREMENT rather than a guess.
///
/// At 60 frames the phi2 mutation moved only `nestest`: the blargg ROMs were
/// still in their init/wait phase and had not begun the work the dot grid
/// perturbs, so three of four workloads contributed nothing. A control whose
/// stimulus never reaches the subject is the "stimulus blind" outcome in this
/// project's mutation taxonomy, and the remediation is a longer run rather
/// than a weaker assertion.
///
/// `AccuracyCoin` was dropped rather than lengthened: it needs a START press
/// and several thousand frames to leave its menu, which is what the battery
/// gate in `accuracycoin.rs` already does. Carrying a copy here would be a
/// slow duplicate of a check that exists.
const _STIMULUS_NOTE: () = ();

/// Per-frame rolling hashes captured on the shipped scheduler.
///
/// Re-bless ONLY together with a written reason for every workload that moved,
/// and only after reading the first differing frame.
const EXPECTED: &[(&str, u64)] = &[
    (
        "tests/roms/blargg/ppu_vbl_nmi/01-vbl_basics.nes",
        0x1484_9546_BA06_AD6E,
    ),
    (
        "tests/roms/blargg/sprite_hit_tests/01.basics.nes",
        0x02D0_97B1_A425_C8F1,
    ),
    ("tests/roms/nestest/nestest.nes", 0x42B9_B06F_0A51_772E),
];

/// Apply the v2.6.18 write-commit offset from the environment, so the control
/// can be run either side of the move without editing shipped code.
#[cfg(feature = "phi2-write-sweep")]
fn apply_offset() {
    // PANIC rather than fall back on an unparseable value. A silently ignored
    // knob makes a diagnostic run report SHIPPED-path hashes while the operator
    // believes a mutation was under test -- which is v2.6.13's `USE_SDRAM`
    // finding in a different harness, where one binary ran under both
    // configurations' names and four consecutive passes looked identical to a
    // knob that never reached the compiler. An absent variable is a legitimate
    // "run the control"; a present-but-malformed one is an operator error and
    // must be loud.
    let Ok(v) = std::env::var("TERMINUS_WRITE_OFFSET") else {
        return;
    };
    let n: u8 = v.parse().unwrap_or_else(|e| {
        panic!(
            "TERMINUS_WRITE_OFFSET is set to {v:?}, which is not a u8 ({e}). \
             Refusing to run: the control would silently measure the shipped \
             path under a name claiming otherwise. Unset it to run the control."
        )
    });
    rustynes_core::rustynes_cpu::WRITE_PHI_OFFSET.store(n, core::sync::atomic::Ordering::Relaxed);
}
#[cfg(not(feature = "phi2-write-sweep"))]
const fn apply_offset() {}

fn run(rom: &str, frames: u32) -> (u64, Vec<u64>) {
    apply_offset();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let data = std::fs::read(root.join(rom)).unwrap_or_else(|e| panic!("{rom}: {e}"));
    let mut nes = Nes::from_rom(&data).unwrap_or_else(|e| panic!("{rom}: {e:?}"));
    let mut rolling = FNV_OFFSET;
    let mut per_frame = Vec::with_capacity(frames as usize);
    // `Nes::frame()`, never the call count: the first `run_frame` after
    // power-on advances ZERO cycles (reset leaves `frame_complete` latched).
    while nes.frame() < u64::from(frames) {
        nes.run_frame();
        let fb: Vec<u8> = nes
            .index_framebuffer()
            .iter()
            .flat_map(|p| p.to_le_bytes())
            .collect();
        fold(&mut rolling, &fb);
        fold(&mut rolling, nes.wram());
        per_frame.push(rolling);
    }
    (rolling, per_frame)
}

#[test]
fn first_difference_control() {
    let mut mismatches = Vec::new();
    for (rom, frames) in WORKLOADS {
        let (got, per_frame) = run(rom, *frames);
        let Some(&(_, want)) = EXPECTED.iter().find(|(r, _)| r == rom) else {
            panic!("{rom} missing from EXPECTED")
        };
        if want == 0 {
            println!("CAPTURE {rom} = 0x{got:016X}");
            continue;
        }
        if let Ok(dir) = std::env::var("TERMINUS_DUMP") {
            // Every failure here is LOUD. This dump is an instrument: the
            // operator sets `TERMINUS_DUMP`, runs the control either side of a
            // change, and diffs the two files to find the first differing
            // frame. A swallowed `create_dir_all` or `write` leaves them
            // diffing files that do not exist -- or, worse, STALE files from an
            // earlier run -- while the test reports green.
            //
            // That is the same defect as a silently-ignored sweep offset one
            // function above: an instrument that reports a success it has not
            // earned is worse than one that is absent, because absence is
            // visible.
            let stem = rom.rsplit('/').next().unwrap_or(rom);
            let mut body = String::new();
            for (i, h) in per_frame.iter().enumerate() {
                writeln!(body, "{i}\t{h:016X}").expect("writing to a String cannot fail");
            }
            std::fs::create_dir_all(&dir)
                .unwrap_or_else(|e| panic!("TERMINUS_DUMP directory {dir:?} is not usable: {e}"));
            let path = format!("{dir}/{stem}.frames.tsv");
            std::fs::write(&path, body)
                .unwrap_or_else(|e| panic!("could not write the frame chain to {path:?}: {e}"));
        }
        if got != want {
            // Locating the FIRST differing frame is the whole point: a
            // difference earlier than the access being moved means the
            // experiment changed rather than the subject. Set
            // TERMINUS_DUMP=<dir> before and after a change and diff the two
            // dumps; the first differing line is the frame.
            mismatches.push(format!(
                "{rom}: rolling hash 0x{got:016X} != 0x{want:016X} \
                 ({} frames; set TERMINUS_DUMP=<dir> to localise the frame)",
                per_frame.len()
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "Terminus control moved:\n  {}\n\nRead the FIRST differing frame before \
         re-baselining. If nestest moved as much as the PPU-heavy workloads, the \
         change is not the one that was intended.",
        mismatches.join("\n  ")
    );
}

//! The control for the `AccuracyCoin` `$6000` readback mirror.
//!
//! `scripts/accuracycoin-build/build_mirror_rom.py` patches `AccuracyCoin` to
//! copy its result vector (`$0300-$04FF`) into the cartridge's battery-backed
//! PRG-RAM window once the battery has finished, so that a hardware run's
//! `<rom>.sav` carries the vector as **bytes** rather than as a photograph a
//! human transcribes. `RustyNES_MiSTer/docs/bringup.md` states the limit this
//! removes: *"a photograph is not 149 status bytes, and reading one is a human
//! transcribing a picture."*
//!
//! ## Why this file exists, and what the builder's byte budget does NOT prove
//!
//! The builder asserts that its patch moved nothing — exactly one header byte,
//! five bytes at the call site, and a run inside bank 2's end-of-bank fill. That
//! is a claim about *layout*. It is not a claim about *answers*.
//!
//! The v2.7.0 plan sets the bar this file enforces: **"the patched ROM must
//! produce a byte-identical vector to the unpatched one in simulation before a
//! single hardware reading is taken from it"**, and the consequence if it does
//! not — *"drop the patch and keep the photograph; a readback channel that
//! alters the thing it reads is worse than no channel."*
//!
//! So both ROMs are run through the oracle, through **the same driver**
//! (`accuracy_coin::run_battery_rom`, which the shipped gate also delegates to),
//! and four things are checked:
//!
//! 1. the raw `$0300-$04FF` window is byte-identical between the two ROMs;
//! 2. the decoded 149-entry status vector is identical entry for entry;
//! 3. the mirror at `$6000-$61FF` reproduces the patched run's own live window;
//! 4. the mirror is not vacuous.
//!
//! Check 4 is the one that is easy to leave out and is the reason the others
//! could pass while the channel is useless. A mirror that never ran leaves 512
//! zero bytes in `.sav`, and on the wire that is indistinguishable from a
//! console which failed every test — the same shape as this project's standing
//! rule that an empty result is not a pass. It is checked against the decoded
//! vector rather than against a hardcoded count, so it cannot go stale when the
//! catalog grows.
//!
//! ## The battery bit is invisible to simulation, and only the budget sees it
//!
//! Found by the mutation pass rather than by reading. A mutant that clears iNES
//! flags6 bit 1 — the change that on hardware means the cartridge has no
//! `$6000` window at all and the save controller never arms — **passes
//! [`mirror_rom_reproduces_the_unpatched_vector_and_mirrors_it`] cleanly**,
//! because the oracle's NROM hands out PRG-RAM unconditionally regardless of
//! the header. That is a known oracle-vs-board divergence, recorded in
//! `docs/accuracy-ledger.md` since v2.6.3 ("NROM provides PRG-RAM at
//! `$6000-$7FFF` where the board has none"), and it means no amount of running
//! this battery can test the bit.
//!
//! So the bit is checked **structurally**, in
//! [`mirror_rom_differs_from_upstream_only_by_the_stated_budget`], or not at
//! all. Do not "simplify" that assertion away on the grounds that the runtime
//! control covers it: the runtime control is provably blind to it, in the one
//! direction that would ship a ROM which mirrors perfectly in simulation and
//! writes into open bus on the board.
//!
//! ## What this does not establish
//!
//! Nothing here has run on hardware. This is a simulation control on the ROM,
//! not a measurement of the readback path: it says the patched ROM answers the
//! same questions the unpatched one does and puts those answers where the save
//! controller will find them. Whether the `MiSTer` core's save path actually
//! writes them to the card is a hardware measurement, and it is v2.7.0's.

#![cfg(feature = "test-roms")]

use std::path::PathBuf;

use rustynes_test_harness::accuracy_coin::{self, BatteryRun};
use rustynes_test_harness::accuracy_coin_catalog;

/// Frame budget. The same figure the shipped battery gate uses; the battery
/// typically completes around frame 4200 and the driver stops on a stable
/// not-run count well before this ceiling.
const MAX_FRAMES: u64 = 72_000;

/// The mirrored window: the `AccuracyCoin` result vector.
const VECTOR_BASE: usize = 0x0300;
const VECTOR_LEN: usize = 0x0200;

/// Where the patched ROM copies it. The cartridge PRG-RAM window is
/// `$6000-$7FFF`, so `sram[0]` is `$6000`.
const MIRROR_OFFSET: usize = 0;

/// The builder's byte budget, restated here so a committed ROM that was NOT
/// produced by the current builder is caught by the gate rather than by
/// whoever next reads the hardware.
///
/// One iNES header byte (flags6, the battery bit), five at the call site, and
/// 29 for the routine.
const EXPECTED_DIFFERING_BYTES: usize = 1 + 5 + 29;

fn mirror_rom_path() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root (CARGO_MANIFEST_DIR/../..)")
        .join("tests")
        .join("roms")
        .join("AccuracyCoin")
        .join("mirror")
        .join("AccuracyCoin-mirror.nes")
}

fn window(ram: &[u8]) -> &[u8] {
    &ram[VECTOR_BASE..VECTOR_BASE + VECTOR_LEN]
}

/// The two committed ROMs differ only by the builder's stated budget.
///
/// Cheap, and it is a check on the ARTIFACT rather than on the builder: the
/// builder proves this about a ROM it has just produced, and this proves it
/// about the ROM that is actually committed. Those come apart the moment
/// somebody rebuilds one and not the other.
#[test]
fn mirror_rom_differs_from_upstream_only_by_the_stated_budget() {
    let base = std::fs::read(accuracy_coin::rom_path()).expect("read vendored AccuracyCoin.nes");
    let mirror = std::fs::read(mirror_rom_path()).expect("read AccuracyCoin-mirror.nes");

    assert_eq!(
        base.len(),
        mirror.len(),
        "the mirror ROM is a different SIZE from upstream ({} vs {}). The patch \
         is size-neutral by construction, so a length change means the \
         committed artifact did not come from the current builder.",
        base.len(),
        mirror.len(),
    );

    let differing: Vec<usize> = base
        .iter()
        .zip(&mirror)
        .enumerate()
        .filter_map(|(i, (a, b))| (a != b).then_some(i))
        .collect();

    assert_eq!(
        differing.len(),
        EXPECTED_DIFFERING_BYTES,
        "expected exactly {EXPECTED_DIFFERING_BYTES} differing bytes (1 header \
         + 5 call site + 29 routine), found {}. Offsets: {:?}",
        differing.len(),
        differing.iter().take(16).collect::<Vec<_>>(),
    );

    // The battery bit specifically. On this core `emu.sv:609` takes
    // `has_prg_ram` from flags6 bit 1 and `emu.sv:353` feeds the same signal to
    // the save controller as `cart_has_battery`, so this ONE bit both creates
    // the $6000 window and arms the write-back. Without it the mirror writes
    // into open bus and the save path never mounts.
    assert_eq!(base[6] & 0x02, 0, "upstream should not set the battery bit");
    assert_eq!(
        mirror[6] & 0x02,
        0x02,
        "the mirror ROM must set iNES flags6 bit 1 (battery). Without it the \
         cartridge has no PRG-RAM window at all and the mirror writes nowhere."
    );
}

/// (5) END TO END, through the shipped comparator rather than through this
/// file's own reimplementation of what it does.
///
/// Checks 1-4 establish two things separately: the mirror equals the window,
/// and `accuracycoin_status`'s lift maps a save back onto the window (its own
/// unit tests). Their CONJUNCTION -- that a real save file, handed to the real
/// CLI against a real work-RAM dump, reports agreement -- is a third claim, and
/// this project has a standing rule about assembling one from two verified
/// halves without checking it. So it is checked.
fn end_to_end_through_the_comparator(oracle_ram: &[u8], hardware_shaped_sav: &[u8]) {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let sav = dir.join("accuracycoin-mirror-e2e.sav");
    let ram_dump = dir.join("accuracycoin-upstream-e2e.ram.bin");
    std::fs::write(&sav, hardware_shaped_sav).expect("write the save");
    std::fs::write(&ram_dump, oracle_ram).expect("write the work-RAM dump");

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_accuracycoin_status"))
        .arg(&ram_dump)
        .arg(format!("sav:{}", sav.display()))
        .output()
        .expect("run accuracycoin_status");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "accuracycoin_status refused the save/dump pair (exit {:?}).\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code(),
    );
    assert!(
        stdout.contains("IDENTICAL entry for entry"),
        "the comparator did not report agreement between the oracle work RAM \
         and the mirrored save.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // And the coverage is full, read off the comparator's own sentence rather
    // than inferred from its exit code. 144 scored rows, all executed on both
    // sides -- the catalog's other five share upstream's omit-sentinel
    // `result_DrawTest` and are excluded by the catalog, not waived here.
    assert!(
        stdout.contains("144 of 144 scored entries executed on both sides"),
        "the comparator's coverage sentence is not the expected 144 of 144.\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("(0 on neither"),
        "some scored entry ran on neither side, so the agreement is partial.\nstdout:\n{stdout}"
    );
    println!("[mirror-control] end-to-end via accuracycoin_status: agreement on a real .sav");
}

/// The control the v2.7.0 plan requires: same ROM, same answers.
#[test]
fn mirror_rom_reproduces_the_unpatched_vector_and_mirrors_it() {
    let base: BatteryRun = accuracy_coin::run_battery_rom(&accuracy_coin::rom_path(), MAX_FRAMES);
    let patched: BatteryRun = accuracy_coin::run_battery_rom(&mirror_rom_path(), MAX_FRAMES);

    println!(
        "[mirror-control] upstream : frames={} sram={} bytes",
        base.result.frames,
        base.sram.len()
    );
    println!(
        "[mirror-control] patched  : frames={} sram={} bytes",
        patched.result.frames,
        patched.sram.len()
    );

    // (1) The raw window, before any decoding. Decoding reads only the bytes the
    // catalog assigns, so a difference at an UNASSIGNED address inside the
    // window would survive a vector comparison and still reach `.sav`.
    let base_window = window(&base.ram);
    let patched_window = window(&patched.ram);
    if base_window != patched_window {
        let diffs: Vec<String> = base_window
            .iter()
            .zip(patched_window)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, (a, b))| format!("${:04X}: ${a:02X} -> ${b:02X}", VECTOR_BASE + i))
            .collect();
        panic!(
            "the patched ROM's result window differs from upstream's in {} of \
             {VECTOR_LEN} bytes. The patch was supposed to be inert with respect \
             to every answer. Per the v2.7.0 plan, the response to this is to \
             DROP the patch and keep the photograph, not to explain it away: a \
             readback channel that alters the thing it reads is worse than no \
             channel.\n  First 20: {:#?}",
            diffs.len(),
            &diffs[..diffs.len().min(20)],
        );
    }

    // (2) The decoded vector, entry for entry — the comparison rung 5 makes.
    let base_vec =
        accuracy_coin_catalog::decode_results(&base.ram).expect("decode upstream vector");
    let patched_vec =
        accuracy_coin_catalog::decode_results(&patched.ram).expect("decode patched vector");
    assert_eq!(
        base_vec.len(),
        patched_vec.len(),
        "catalog length differs between runs, which cannot happen for one catalog"
    );
    let mismatches: Vec<String> = base_vec
        .iter()
        .zip(&patched_vec)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, (a, b))| {
            let name = accuracy_coin_catalog::entry(i).map_or("?", |e| e.name.as_str());
            format!("{name}: {a:?} -> {b:?}")
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "{} catalog entries disagree between the upstream and patched ROMs: {:#?}",
        mismatches.len(),
        mismatches,
    );

    // (3) The mirror reproduces the patched run's OWN live window. Compared
    // against `patched.ram`, not against `base.ram`: this asks whether the copy
    // is faithful, and (1) has already established the two windows are equal, so
    // routing it through `base` would let a broken copy pass whenever the two
    // happened to agree for some other reason.
    assert!(
        patched.sram.len() >= VECTOR_LEN,
        "the patched cartridge exposes only {} bytes of PRG-RAM; the mirror \
         needs {VECTOR_LEN}. The battery bit is what creates this window.",
        patched.sram.len(),
    );
    let mirrored = &patched.sram[MIRROR_OFFSET..MIRROR_OFFSET + VECTOR_LEN];
    if mirrored != patched_window {
        let n = mirrored
            .iter()
            .zip(patched_window)
            .filter(|(a, b)| a != b)
            .count();
        panic!(
            "the mirror at $6000-$61FF does not match the live window at \
             $0300-$04FF: {n} of {VECTOR_LEN} bytes differ. The copy ran at \
             `; All tests are complete!`; anything writing to the window AFTER \
             that point would produce exactly this."
        );
    }

    // (4) The vacuous-pass guard. A mirror that never ran is 512 zero bytes,
    // which on the wire is indistinguishable from a console that failed every
    // test. Anchored to the decoded vector rather than to a literal so it
    // cannot go stale as the catalog grows.
    let non_zero = mirrored.iter().filter(|&&b| b != 0).count();
    let decoded_non_not_run = accuracy_coin_catalog::scored(&patched_vec)
        .filter(|(_, s)| !matches!(s, accuracy_coin_catalog::TestStatus::NotRun))
        .count();
    assert!(
        decoded_non_not_run > 0,
        "the patched run decoded to an all-NotRun vector, so this comparison \
         proves nothing — the battery never executed."
    );
    assert!(
        non_zero >= decoded_non_not_run,
        "the mirror holds {non_zero} non-zero bytes but the battery recorded \
         {decoded_non_not_run} entries that are not NotRun. The copy did not \
         run, or ran before the results were written."
    );

    println!(
        "[mirror-control] window $0300-$04FF identical; {} catalog entries identical; \
         mirror reproduces it ({non_zero} non-zero bytes, {decoded_non_not_run} entries run)",
        base_vec.len()
    );

    end_to_end_through_the_comparator(&base.ram, &patched.sram);
}

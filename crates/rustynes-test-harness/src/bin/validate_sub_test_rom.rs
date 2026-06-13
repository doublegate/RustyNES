//! Standalone validator for custom `AccuracyCoin` sub-test ROMs.
//!
//! Boots a `.nes` file under `rustynes-core`, runs frames until either the
//! target RAM byte becomes non-zero and stable, or the budget expires.
//! Prints the final byte + a human interpretation per
//! `accuracy_coin_catalog::TestStatus::from_byte`.
//!
//! USAGE:
//!
//! ```text
//! cargo run -p rustynes-test-harness --release \
//!   --bin validate_sub_test_rom -- \
//!   <rom.nes> <result-addr-hex> <max-frames>
//! ```
//!
//! EXAMPLE:
//!
//! ```text
//! cargo run -p rustynes-test-harness --release \
//!   --bin validate_sub_test_rom -- \
//!   tests/roms/AccuracyCoin/sub-tests/controller-strobing.nes \
//!   045F 600
//! ```
//!
//! This is part of Phase 2 of the v1.0.0-final brief — a turn-key
//! probe that confirms a custom sub-test ROM boots into its target
//! test (rather than getting stuck on title-screen or menu loops).
//! Mesen2's `testRunner` can then trace the SAME custom ROM under the
//! `accuracycoin` protocol path in `scripts/mesen2_irq_trace.lua` and
//! produce per-cycle oracles for Phase 3 (Controller Strobing) and
//! Phase 4 (Implied Dummy + DMC coordinated) without the wall-time
//! blocker documented in
//! `docs/audit/session-22-sprint1-iter2-phase-b-2026-05-22.md`.

use std::env;
use std::fs;
use std::process::ExitCode;

use rustynes_core::Nes;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!(
            "usage: {} <rom.nes> <result-addr-hex> <max-frames>",
            args[0]
        );
        return ExitCode::from(2);
    }
    let rom_path = &args[1];
    let addr =
        u16::from_str_radix(args[2].trim_start_matches('$'), 16).expect("parse hex result address");
    let max_frames: u64 = args[3].parse().expect("parse max-frames");

    let bytes = fs::read(rom_path).expect("read ROM");
    let mut nes = Nes::from_rom(&bytes).expect("parse ROM (NROM)");

    let mut first_set_frame: Option<u64> = None;
    let mut stable_frames: u64 = 0;
    let mut last_val: u8 = 0;
    for f in 0..max_frames {
        nes.run_frame();
        let v = nes.bus().ram_bytes()[addr as usize];
        if first_set_frame.is_none() && v != 0 {
            first_set_frame = Some(f);
            last_val = v;
        }
        if first_set_frame.is_some() {
            if v == last_val {
                stable_frames += 1;
                if stable_frames >= 60 {
                    break;
                }
            } else {
                stable_frames = 0;
                last_val = v;
            }
        }
    }
    let final_val = nes.bus().ram_bytes()[addr as usize];
    let interpretation = match final_val {
        0x00 => "NotRun",
        v if v & 0x03 == 0x01 => "Pass",
        v if v & 0x03 == 0x02 => "Fail",
        v if v & 0x03 == 0x03 => "PassWithCode/Other",
        _ => "Skipped/Unknown",
    };
    println!(
        "rom={rom_path} addr=${addr:04X} final=0x{final_val:02X} \
         first_set_frame={first_set_frame:?} interpretation={interpretation}"
    );
    if first_set_frame.is_none() || final_val == 0 {
        eprintln!("FAIL: target RAM byte never set in {max_frames} frames");
        ExitCode::from(1)
    } else {
        ExitCode::from(0)
    }
}

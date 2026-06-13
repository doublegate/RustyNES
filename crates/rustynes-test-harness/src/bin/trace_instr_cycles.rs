#![allow(
    clippy::items_after_statements,
    clippy::doc_markdown,
    clippy::cast_possible_truncation,
    clippy::format_push_string
)]
//! v2.0 R1c-1 diagnostic: per-CPU-instruction `(PC, cumulative cpu_cycle)`
//! trace, dumped from the `cpu-instr-cycle-trace` ring after running the
//! AccuracyCoin battery to a target result. Built BOTH ways and diffed
//! (`scripts`-side python) to pin the odd-cycle cumulative divergence between
//! R1 and default that makes `CheckDMATiming` Y = 3 vs 4:
//!
//!   default: cargo run -p rustynes-test-harness --release \
//!     --features cpu-instr-cycle-trace,test-roms --bin trace_instr_cycles -- \
//!     tests/roms/accuracycoin/AccuracyCoin.nes 0477 2000 /tmp/RustyNES_v2/ic_def.csv
//!   R1:      ... --features cpu-instr-cycle-trace,test-roms,mc-r1-substrate,mc-r1-dmc-idle-halt ...
//!
//! Columns: `idx,pc,cpu_cycle`. The ring keeps the LAST `CAP` instructions, so
//! running to the `$0477` (DMC+OAM) result captures the CheckDMATiming region.

use std::env;
use std::fs;
use std::process::ExitCode;

use rustynes_core::{Buttons, Nes};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 5 {
        eprintln!(
            "usage: {} <rom.nes> <result-addr-hex> <max-frames> <out.csv>",
            args[0]
        );
        return ExitCode::from(2);
    }
    let rom = fs::read(&args[1]).expect("read ROM");
    // Sentinel "BOOT": capture the FIRST `CAP` instructions of cold boot (the
    // window where the R1-vs-default cumulative offset C goes 0 -> ~92), instead
    // of the last `CAP` before a result-addr is set. Used by the P2 verify step
    // to pin the divergence-injection PCs (instr_cost_localize.py).
    let boot_mode = args[2].eq_ignore_ascii_case("BOOT");
    let addr = if boot_mode {
        0u16
    } else {
        u16::from_str_radix(args[2].trim_start_matches('$'), 16).expect("hex addr")
    };
    let max_frames: u64 = args[3].parse().expect("max-frames");
    let out = &args[4];

    use core::sync::atomic::Ordering::Relaxed;
    use rustynes_core::instr_trace::{CAP, CYC, IDX, PC};

    let mut nes = Nes::from_rom(&rom).expect("parse ROM");
    let mut set_frame = None;
    if boot_mode {
        // Run `max_frames` frames of cold boot (must stay under CAP instructions
        // so the ring does NOT wrap), then dump [0, total). ~8k instr/frame, so
        // keep max_frames <= ~28. Captures the cold-boot vblank-wait phase where
        // the R1-vs-default offset C is injected.
        for _ in 0..max_frames {
            nes.run_frame();
        }
        assert!(
            IDX.load(Relaxed) <= CAP as u64,
            "BOOT ring wrapped ({} > {CAP}); lower max_frames",
            IDX.load(Relaxed)
        );
    } else {
        for _ in 0..300 {
            nes.run_frame();
        }
        nes.set_buttons(0, Buttons::START);
        for _ in 0..6 {
            nes.run_frame();
        }
        nes.set_buttons(0, Buttons::empty());
        for f in 0..max_frames {
            nes.run_frame();
            if nes.bus().ram_bytes()[addr as usize] != 0 {
                set_frame = Some(f);
                break;
            }
        }
    }

    let total = IDX.load(Relaxed);
    // BOOT mode: dump cold-boot [0, total) (no wrap, asserted above); else last CAP.
    let start = if boot_mode {
        0
    } else {
        total.saturating_sub(CAP as u64)
    };
    let end = total;
    let mut s = String::from("idx,pc,cpu_cycle\n");
    for i in start..end {
        let slot = (i as usize) % CAP;
        let pc = PC[slot].load(Relaxed);
        let cyc = CYC[slot].load(Relaxed);
        s.push_str(&format!("{i},{pc:04X},{cyc}\n"));
    }
    fs::write(out, &s).expect("write csv");
    println!(
        "rom={} addr=${addr:04X} set_frame={set_frame:?} total_instr={total} dumped={} out={out}",
        args[1],
        end - start
    );
    ExitCode::from(0)
}

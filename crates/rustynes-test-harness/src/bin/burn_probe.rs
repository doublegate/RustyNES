//! v2.0.0 beta.2 (A2 scoping) — burn-loop histogram probe.
//!
//! Runs a ROM and prints `Cpu::burn_histogram`: the per-opcode count of
//! cycles the trailing burn-loop had to fill (`cycles - cycles_emitted`).
//! Every nonzero row is a dispatch arm that still declares more cycles than
//! it emits through the per-cycle helpers — the exact remaining
//! busless-cycle surface the every-cycle-bus-access conversion (Workstream
//! A2 of `to-dos/plans/v2.0.0-master-clock-plan.md`) must turn into dummy
//! reads of the held address.
//!
//! Usage:
//!   `cargo run -p rustynes-test-harness --release \
//!      --features test-roms,cpu-instr-cycle-trace --bin burn_probe \
//!      -- <rom.nes> <frames> [--start]`
//!
//! `--start` presses START after 300 boot frames (the `AccuracyCoin` battery
//! launch protocol).

use std::env;
use std::fs;
use std::process::ExitCode;

use rustynes_core::{Buttons, Nes};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: {} <rom.nes> <frames> [--start]", args[0]);
        return ExitCode::FAILURE;
    }
    let rom_path = &args[1];
    let frames: u64 = args[2].parse().expect("parse frames");
    let press_start = args.iter().any(|a| a == "--start");

    let bytes = fs::read(rom_path).expect("read ROM");
    let mut nes = Nes::from_rom(&bytes).expect("parse ROM");

    if press_start {
        for _ in 0..300 {
            nes.run_frame();
        }
        nes.set_buttons(0, Buttons::START);
        for _ in 0..6 {
            nes.run_frame();
        }
        nes.set_buttons(0, Buttons::empty());
    }

    for _ in 0..frames {
        nes.run_frame();
    }

    let hist = &nes.cpu().burn_histogram;
    let total: u64 = hist.iter().sum();
    let mut rows: Vec<(usize, u64)> = hist
        .iter()
        .enumerate()
        .filter(|&(_, &c)| c > 0)
        .map(|(op, &c)| (op, c))
        .collect();
    rows.sort_by_key(|&(_, c)| core::cmp::Reverse(c));

    println!("burn-loop total cycles filled: {total}");
    println!("nonzero opcodes: {}", rows.len());
    println!("opcode  burned-cycles");
    for (op, c) in rows {
        println!("  ${op:02X}   {c}");
    }
    ExitCode::SUCCESS
}

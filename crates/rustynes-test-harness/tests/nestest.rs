//! Layer 2: nestest golden-log compare.
//!
//! Run nestest in PC=$C000 automation mode for the documented number of
//! instructions and assert each instruction's `(PC, A, X, Y, P, SP, PPU
//! scanline, PPU dot, CYC)` matches the bundled `nestest.log`.

#![cfg(feature = "test-roms")]

use std::fs;
use std::path::PathBuf;

use rustynes_test_harness::{
    NestestBus, NestestRunner, cpu_for_nestest, format_log_line, parse_log_line,
};

fn rom_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("tests")
        .join("roms")
        .join("nestest")
}

#[test]
fn nestest_pc_c000_matches_golden_log() {
    let dir = rom_dir();
    let rom = fs::read(dir.join("nestest.nes")).expect("read nestest.nes");
    let log = fs::read_to_string(dir.join("nestest.log")).expect("read nestest.log");

    let mut bus = NestestBus::new(&rom);
    let mut cpu = cpu_for_nestest();
    let mut runner = NestestRunner::new(&mut bus, &mut cpu);

    let golden_lines: Vec<&str> = log.lines().collect();
    let mut compared = 0usize;

    for (i, golden_raw) in golden_lines.iter().enumerate() {
        // Ignore preamble lines that don't match the format.
        let Some(expected) = parse_log_line(golden_raw) else {
            continue;
        };
        let actual = runner.step();
        compared += 1;

        if actual != expected {
            let actual_fmt = format_log_line(&actual);
            panic!(
                "nestest divergence at instruction {} (golden line {}):\n  expected: {}\n  actual:   {}\n  golden:   {}",
                compared,
                i + 1,
                format_log_line(&expected),
                actual_fmt,
                golden_raw
            );
        }
    }

    // Sanity: nestest's golden log is documented as ~8991 lines.
    assert!(
        compared >= 8000,
        "expected to compare at least 8000 instructions, got {compared}"
    );
}

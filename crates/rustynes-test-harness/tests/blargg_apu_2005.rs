//! blargg `blargg_apu_2005.07.30/*.nes` corpus (11 sub-ROMs).
//!
//! The full 2005-era APU regression suite: length-counter behaviour + table,
//! the frame-IRQ flag, clock jitter, length timing in both frame-counter
//! modes, IRQ-flag and IRQ timing, reset timing, and length halt/reload
//! timing. All NROM (mapper 0), driven through the full lockstep `Nes` via
//! the `$6000` status protocol.
//!
//! Per `docs/testing-strategy.md` §Layer 3.
//!
//! Observed status (v2.1.0 coverage wiring, R1 master-clock default build):
//! all eleven PASS.

#![cfg(feature = "test-roms")]

use std::fs;
use std::path::PathBuf;

use rustynes_test_harness::run_nes_blargg;

fn rom_path(rel: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("tests")
        .join("roms")
        .join(rel)
}

fn run(name: &str, max_frames: u64) -> (u8, String, u64) {
    let path = rom_path(&format!("nes-test-roms/blargg_apu_2005.07.30/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg(&bytes, max_frames).expect("rom must parse + run");
    (r.status, r.message, r.frames)
}

macro_rules! blargg_apu_2005_test {
    ($name:ident, $rom:literal) => {
        #[test]
        fn $name() {
            let (s, m, f) = run($rom, 2000);
            eprintln!("{}: status={s:#x} frames={f} msg={m:?}", $rom);
            assert_eq!(s, 0, "{} failed: {m}", $rom);
        }
    };
}

blargg_apu_2005_test!(blargg_apu_2005_01_len_ctr, "01.len_ctr.nes");
blargg_apu_2005_test!(blargg_apu_2005_02_len_table, "02.len_table.nes");
blargg_apu_2005_test!(blargg_apu_2005_03_irq_flag, "03.irq_flag.nes");
blargg_apu_2005_test!(blargg_apu_2005_04_clock_jitter, "04.clock_jitter.nes");
blargg_apu_2005_test!(
    blargg_apu_2005_05_len_timing_mode0,
    "05.len_timing_mode0.nes"
);
blargg_apu_2005_test!(
    blargg_apu_2005_06_len_timing_mode1,
    "06.len_timing_mode1.nes"
);
blargg_apu_2005_test!(blargg_apu_2005_07_irq_flag_timing, "07.irq_flag_timing.nes");
blargg_apu_2005_test!(blargg_apu_2005_08_irq_timing, "08.irq_timing.nes");
blargg_apu_2005_test!(blargg_apu_2005_09_reset_timing, "09.reset_timing.nes");
blargg_apu_2005_test!(blargg_apu_2005_10_len_halt_timing, "10.len_halt_timing.nes");
blargg_apu_2005_test!(
    blargg_apu_2005_11_len_reload_timing,
    "11.len_reload_timing.nes"
);

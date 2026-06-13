//! blargg `pal_apu_tests/*.nes` corpus (10 sub-ROMs) — PAL region.
//!
//! The PAL counterpart of `blargg_apu_2005`: the same APU length-counter /
//! frame-IRQ / timing checks, but with the test expectations calibrated for
//! PAL frame-counter timing. These ROMs ship as plain iNES 1.0 with no
//! NES-2.0 region byte, so they would default to NTSC and fail; the
//! [`run_nes_blargg_pal`] helper rewrites the header (NES-2.0 marker + PAL
//! region nibble) in a throwaway copy so the core selects PAL dividers.
//!
//! Per `docs/testing-strategy.md` §Layer 3 and
//! `docs/audit/pal-dendy-validation-inventory-2026-05-24.md`.
//!
//! Observed status (v2.1.0 coverage wiring, R1 master-clock default build):
//! all ten PASS under forced PAL timing. (The suite has no `09.reset_timing`
//! ROM — that variant lives only in the NTSC `blargg_apu_2005` set.)

#![cfg(feature = "test-roms")]

use std::fs;
use std::path::PathBuf;

use rustynes_test_harness::run_nes_blargg_pal;

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
    let path = rom_path(&format!("nes-test-roms/pal_apu_tests/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg_pal(&bytes, max_frames).expect("rom must parse + run");
    (r.status, r.message, r.frames)
}

macro_rules! pal_apu_test {
    ($name:ident, $rom:literal) => {
        #[test]
        fn $name() {
            let (s, m, f) = run($rom, 2000);
            eprintln!("PAL {}: status={s:#x} frames={f} msg={m:?}", $rom);
            assert_eq!(s, 0, "PAL {} failed: {m}", $rom);
        }
    };
}

pal_apu_test!(pal_apu_01_len_ctr, "01.len_ctr.nes");
pal_apu_test!(pal_apu_02_len_table, "02.len_table.nes");
pal_apu_test!(pal_apu_03_irq_flag, "03.irq_flag.nes");
pal_apu_test!(pal_apu_04_clock_jitter, "04.clock_jitter.nes");
pal_apu_test!(pal_apu_05_len_timing_mode0, "05.len_timing_mode0.nes");
pal_apu_test!(pal_apu_06_len_timing_mode1, "06.len_timing_mode1.nes");
pal_apu_test!(pal_apu_07_irq_flag_timing, "07.irq_flag_timing.nes");
pal_apu_test!(pal_apu_08_irq_timing, "08.irq_timing.nes");
pal_apu_test!(pal_apu_10_len_halt_timing, "10.len_halt_timing.nes");
pal_apu_test!(pal_apu_11_len_reload_timing, "11.len_reload_timing.nes");

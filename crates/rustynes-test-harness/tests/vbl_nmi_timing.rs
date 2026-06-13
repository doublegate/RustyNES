//! blargg `vbl_nmi_timing/*.nes` corpus (7 sub-ROMs).
//!
//! Companion suite to `ppu_vbl_nmi` covering the precise PPU-clock timing of
//! the frame structure, VBL set/clear, NMI suppression and disable, the
//! even/odd-frame dot skip, and NMI assertion timing. All NROM (mapper 0),
//! driven through the full lockstep `Nes` via the `$6000` status protocol.
//!
//! Per `docs/testing-strategy.md` §Layer 3.
//!
//! Observed status (v2.1.0 coverage wiring, R1 master-clock default build):
//! all seven PASS.

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
    let path = rom_path(&format!("nes-test-roms/vbl_nmi_timing/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg(&bytes, max_frames).expect("rom must parse + run");
    (r.status, r.message, r.frames)
}

macro_rules! vbl_nmi_timing_test {
    ($name:ident, $rom:literal) => {
        #[test]
        fn $name() {
            let (s, m, f) = run($rom, 1000);
            eprintln!("{}: status={s:#x} frames={f} msg={m:?}", $rom);
            assert_eq!(s, 0, "{} failed: {m}", $rom);
        }
    };
}

vbl_nmi_timing_test!(vbl_nmi_timing_1_frame_basics, "1.frame_basics.nes");
vbl_nmi_timing_test!(vbl_nmi_timing_2_vbl_timing, "2.vbl_timing.nes");
vbl_nmi_timing_test!(vbl_nmi_timing_3_even_odd_frames, "3.even_odd_frames.nes");
vbl_nmi_timing_test!(vbl_nmi_timing_4_vbl_clear_timing, "4.vbl_clear_timing.nes");
vbl_nmi_timing_test!(vbl_nmi_timing_5_nmi_suppression, "5.nmi_suppression.nes");
vbl_nmi_timing_test!(vbl_nmi_timing_6_nmi_disable, "6.nmi_disable.nes");
vbl_nmi_timing_test!(vbl_nmi_timing_7_nmi_timing, "7.nmi_timing.nes");

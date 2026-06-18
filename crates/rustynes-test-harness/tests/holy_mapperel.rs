//! Tepples / Damian Yerrick `holy_mapperel` cartridge-PCB-assembly test ROM.
//!
//! Source: <https://github.com/pinobatch/holy-mapperel> v0.02 release
//! `holy-mapperel-bin-0.02.7z`. The README's "Legal" section places the
//! ROMs under the **zlib license** (permissive redistribution).
//!
//! Holy Mapperel detects which mapper it's running on by mirroring tests,
//! then sizes PRG/CHR ROM/RAM and exercises bank reachability. Results are
//! reported **visually** (on-screen text + Morse-coded audio beeps), NOT
//! via the blargg `$6000` status-byte protocol. Therefore each ROM is
//! smoke-tested: parse + boot + advance the frame counter to the budget
//! without panic, crash, or open-bus assertion.
//!
//! Per `docs/testing-strategy.md` §Layer 3 (test ROM corpus) and
//! `docs/STATUS.md` (Track B1 of the gap-analysis remediation plan).
//!
//! The ROMs are named `M<mapper>_P<PRG>_<CHR>[_<mirroring or WRAM>].nes`
//! per the upstream convention. We vendor only the subset whose mapper
//! ID is in the project's supported set (per `docs/STATUS.md` §"Mapper
//! coverage"):
//!
//! | Mapper | ROMs vendored | Notes |
//! |--------|---------------|-------|
//! | 0 (NROM)   | `M0_P32K_CR8K_V`, `M0_P32K_CR32K_V` | Minimum + max CHR-RAM |
//! | 1 (MMC1)   | `M1_P128K_CR8K`, `M1_P128K_C32K` | CHR-RAM and CHR-ROM forms |
//! | 2 (`UxROM`)  | `M2_P128K_CR8K_V` | |
//! | 3 (`CNROM`)  | `M3_P32K_C32K_H` | |
//! | 4 (MMC3)   | `M4_P128K_CR8K`, `M4_P128K_CR32K`, `M4_P256K_C256K` | CHR-RAM 8K / 32K, CHR-ROM 256K |
//! | 7 (`AxROM`)  | `M7_P128K_CR8K` | |
//! | 9 (MMC2)   | `M9_P128K_C64K` | |
//! | 10 (MMC4)  | `M10_P128K_C64K_W8K`, `M10_P128K_C64K_S8K` | WRAM vs SRAM variants |
//! | 34 (`BNROM`) | `M34_P128K_CR8K_H` | |
//! | 66 (`GxROM`) | `M66_P64K_C16K_V` | |
//! | 69 (FME-7) | `M69_P128K_C64K_W8K`, `M69_P128K_C64K_S8K` | |
//!
//! v1.7.0 "Forge" F1 note: mappers **28** (Action 53), **118** (`TxSROM`), and
//! **180** (Crazy Climber / `UNROM`-180) are now ALL in the supported set
//! (`crates/rustynes-mappers/src/lib.rs`), so the earlier "excluded — not
//! supported" note for them is stale. The blocker for wiring their
//! holy-mapperel ROMs here is asset availability: the committed holy-mapperel
//! v0.02 release does NOT ship `M28_*` / `M118_*` / `M180_*` binaries (they have
//! to be built from the holy-mapperel source with the right config), and the
//! project policy is to never fabricate or commit ROMs that aren't already in
//! the corpus. So these three remain a documented **F1 carryover**: when an
//! `M28_*` / `M118_*` / `M180_*` ROM is added to `tests/roms/holy_mapperel/`,
//! add a `smoke_mapperel(...)` line below (the harness already supports their
//! mappers). Mapper 78.3 stays out of scope. The 17 ROMs below are the present,
//! committed subset.

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
        .join("holy_mapperel")
        .join(rel)
}

fn smoke_mapperel(rel: &str) {
    let path = rom_path(rel);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg(&bytes, 600).expect("rom must parse + run");
    assert!(
        r.frames > 0,
        "{rel} produced 0 frames — emulator did not advance"
    );
}

// ---- NROM (mapper 0) ----
#[test]
fn holy_mapperel_m0_p32k_cr8k_v_smoke() {
    smoke_mapperel("M0_P32K_CR8K_V.nes");
}

#[test]
fn holy_mapperel_m0_p32k_cr32k_v_smoke() {
    smoke_mapperel("M0_P32K_CR32K_V.nes");
}

// ---- MMC1 (mapper 1) ----
#[test]
fn holy_mapperel_m1_p128k_cr8k_smoke() {
    smoke_mapperel("M1_P128K_CR8K.nes");
}

#[test]
fn holy_mapperel_m1_p128k_c32k_smoke() {
    smoke_mapperel("M1_P128K_C32K.nes");
}

// ---- UxROM (mapper 2) ----
#[test]
fn holy_mapperel_m2_p128k_cr8k_v_smoke() {
    smoke_mapperel("M2_P128K_CR8K_V.nes");
}

// ---- CNROM (mapper 3) ----
#[test]
fn holy_mapperel_m3_p32k_c32k_h_smoke() {
    smoke_mapperel("M3_P32K_C32K_H.nes");
}

// ---- MMC3 (mapper 4) ----
#[test]
fn holy_mapperel_m4_p128k_cr8k_smoke() {
    smoke_mapperel("M4_P128K_CR8K.nes");
}

#[test]
fn holy_mapperel_m4_p128k_cr32k_smoke() {
    smoke_mapperel("M4_P128K_CR32K.nes");
}

#[test]
fn holy_mapperel_m4_p256k_c256k_smoke() {
    smoke_mapperel("M4_P256K_C256K.nes");
}

// ---- AxROM (mapper 7) ----
#[test]
fn holy_mapperel_m7_p128k_cr8k_smoke() {
    smoke_mapperel("M7_P128K_CR8K.nes");
}

// ---- MMC2 (mapper 9) ----
#[test]
fn holy_mapperel_m9_p128k_c64k_smoke() {
    smoke_mapperel("M9_P128K_C64K.nes");
}

// ---- MMC4 (mapper 10) ----
#[test]
fn holy_mapperel_m10_p128k_c64k_w8k_smoke() {
    smoke_mapperel("M10_P128K_C64K_W8K.nes");
}

#[test]
fn holy_mapperel_m10_p128k_c64k_s8k_smoke() {
    smoke_mapperel("M10_P128K_C64K_S8K.nes");
}

// ---- BNROM / NINA-001 variants (mapper 34) ----
#[test]
fn holy_mapperel_m34_p128k_cr8k_h_smoke() {
    smoke_mapperel("M34_P128K_CR8K_H.nes");
}

// ---- GxROM (mapper 66) ----
#[test]
fn holy_mapperel_m66_p64k_c16k_v_smoke() {
    smoke_mapperel("M66_P64K_C16K_V.nes");
}

// ---- Sunsoft FME-7 (mapper 69) ----
#[test]
fn holy_mapperel_m69_p128k_c64k_w8k_smoke() {
    smoke_mapperel("M69_P128K_C64K_W8K.nes");
}

#[test]
fn holy_mapperel_m69_p128k_c64k_s8k_smoke() {
    smoke_mapperel("M69_P128K_C64K_S8K.nes");
}

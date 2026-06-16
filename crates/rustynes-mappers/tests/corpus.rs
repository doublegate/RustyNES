//! Sprint 2 corpus test: walk every NROM ROM under `tests/roms/sprint-2/`
//! and assert the parser + NROM construction succeed.
//!
//! Gated behind the `test-roms` feature so default `cargo test --workspace`
//! does not need the corpus on disk.

#![cfg(feature = "test-roms")]

use std::fs;
use std::path::PathBuf;

use rustynes_mappers::{RomError, parse};

fn corpus_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root has two parents above the crate manifest")
        .join("tests")
        .join("roms")
        .join("sprint-2")
}

#[test]
fn every_sprint2_rom_parses_as_nrom() {
    let dir = corpus_dir();
    let entries: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("failed to read {dir:?}: {e}"))
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("nes"))
        .collect();

    assert!(
        entries.len() >= 10,
        "sprint-2 corpus should have >= 10 NROM ROMs; found {}",
        entries.len()
    );

    let mut failures = Vec::new();
    for entry in &entries {
        let path = entry.path();
        let bytes = fs::read(&path).expect("read rom file");
        match parse(&bytes) {
            Ok((cart, _mapper)) => {
                assert_eq!(
                    cart.mapper_id, 0,
                    "sprint-2 ROM {path:?} should be NROM but reported mapper {}",
                    cart.mapper_id
                );
                assert!(!cart.prg_rom.is_empty(), "{path:?} has empty PRG-ROM");
                // CHR-ROM may be empty for CHR-RAM variants.
            }
            Err(RomError::UnsupportedMapper(m)) => {
                failures.push(format!("{path:?}: UnsupportedMapper({m})"));
            }
            Err(other) => {
                failures.push(format!("{path:?}: {other}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} ROM(s) failed to parse:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn nestest_parses_as_nrom() {
    let path = corpus_dir().join("nestest.nes");
    let bytes = fs::read(&path).expect("read nestest.nes");
    let (cart, _mapper) = parse(&bytes).expect("nestest must parse");
    assert_eq!(cart.mapper_id, 0);
    assert_eq!(cart.prg_rom.len(), 16 * 1024); // nestest is a 16K PRG NROM.
    assert_eq!(cart.chr_rom.len(), 8 * 1024);
}

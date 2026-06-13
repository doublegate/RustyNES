//! v2.8.0 Phase 4 — the PGO training workload (see `scripts/pgo/run.sh`).
//!
//! Adapted from Mesen2's `PGOHelper`: sweep a ROM corpus at maximum speed
//! (no limiter, no audio device, no display) with scripted input that
//! pushes games past their title screens (Start held on a 4-of-7-frame
//! cycle — Mesen2's exact trick — plus a rotating d-pad/A mix so movement
//! and collision code paths get profiled too).
//!
//! Corpus: a committed CC0/MIT/zlib spread covering the hot configurations
//! (NROM static + render-heavy, MMC1, MMC3, APU/DMC, sprite-eval stress,
//! the `AccuracyCoin` gauntlet), plus every `.nes` the user drops into
//! `tests/roms/external/PGOGames/` (never committed) for a profile that
//! matches real games.
//!
//! Usage: `pgo_trainer [frames-per-rom]` (default 3600 ≈ 60 s of NTSC
//! gameplay each). Run via `cargo pgo build` so the profile data lands in
//! the instrumented build's output directory.

use std::path::PathBuf;

use rustynes_core::{Buttons, Nes};

/// Committed training corpus, workspace-relative.
const COMMITTED: &[&str] = &[
    "tests/roms/nestest/nestest.nes",
    "tests/roms/sprint-2/flowing_palette.nes",
    "tests/roms/sprint-2/oam_stress.nes",
    "tests/roms/audio-tests/db_apu.nes",
    "tests/roms/accuracycoin/AccuracyCoin.nes",
    "tests/roms/holy_mapperel/M1_P128K_CR8K.nes",
    "tests/roms/holy_mapperel/M4_P128K_CR8K.nes",
];

/// Scripted input for training frame `f`: Start on a 4-of-7 cycle (gets
/// past title screens; Mesen2's `PGOHelper` pattern) + a rotating
/// d-pad/A/B mix so gameplay paths run.
fn buttons_for(f: u32) -> Buttons {
    let mut b = Buttons::empty();
    if f % 7 <= 3 {
        b |= Buttons::START;
    }
    match (f / 60) % 4 {
        0 => b |= Buttons::RIGHT | Buttons::A,
        1 => b |= Buttons::LEFT,
        2 => b |= Buttons::A | Buttons::B,
        _ => b |= Buttons::DOWN,
    }
    b
}

fn main() {
    let frames: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root is two levels above the crate manifest")
        .to_path_buf();

    let mut roms: Vec<PathBuf> = COMMITTED.iter().map(|r| root.join(r)).collect();
    let user_dir = root.join("tests/roms/external/PGOGames");
    if let Ok(entries) = std::fs::read_dir(&user_dir) {
        let mut user: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nes")))
            .collect();
        user.sort();
        println!("+ {} user ROM(s) from {}", user.len(), user_dir.display());
        roms.append(&mut user);
    }

    let mut total_frames: u64 = 0;
    let mut audio = vec![0.0f32; 8192];
    for rom in &roms {
        let bytes = match std::fs::read(rom) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("skip {}: {e}", rom.display());
                continue;
            }
        };
        let mut nes = match Nes::from_rom(&bytes) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("skip {}: {e:?}", rom.display());
                continue;
            }
        };
        for f in 0..frames {
            nes.set_buttons(0, buttons_for(f));
            nes.run_frame();
            let _ = nes.drain_audio_into(&mut audio);
        }
        total_frames += u64::from(frames);
        println!(
            "trained {:>7} frames on {}",
            frames,
            rom.file_name().and_then(|n| n.to_str()).unwrap_or("?")
        );
    }
    println!(
        "pgo_trainer: {total_frames} frames across {} ROM(s)",
        roms.len()
    );
}

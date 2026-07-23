//! End-to-end TAS movie record / playback determinism tests.
//!
//! v1.4.0 Sprint 4.1 (core movie infrastructure). These exercise the
//! `rustynes_core::movie` API against a committed (CC0 / public-domain) test ROM
//! with visible framebuffer motion AND audio output, proving that a recorded
//! movie replays bit-identically (framebuffer FNV-1a + audio FNV-1a +
//! cumulative cycle count) from its start point.
//!
//! Gated on `test-roms` because the driver ROM (`assorted/flowing_palette.nes`
//! — an animated palette demo) lives under `tests/roms/`. The pure-unit
//! determinism tests (synthetic NROM) live in `crates/rustynes-core/src/movie.rs`
//! and run in the default build.
//!
//! ```bash
//! cargo test -p rustynes-test-harness --features test-roms --test movie
//! ```
#![cfg(feature = "test-roms")]

use rustynes_core::{Movie, MoviePlayer, MovieRecorder, Nes, StartPoint};

/// An animated CC0 demo: the palette flows, so successive framebuffers
/// differ — a strong signal that the movie reconstructs *visible* motion,
/// not just a static screen.
const DRIVER_ROM: &[u8] = include_bytes!("../../../tests/roms/assorted/flowing_palette.nes");

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

fn audio_fnv(samples: &[f32]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for s in samples {
        for &b in &s.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    h
}

/// Deterministic, varied synthetic input — no RNG, reproducible. Even though
/// this ROM ignores controller input, recording + replaying it proves the
/// movie input stream round-trips and the replay is byte-identical.
fn synthetic_inputs(n: usize) -> Vec<(u8, u8)> {
    (0..n)
        .map(|i| {
            let i = u8::try_from(i % 256).unwrap();
            let p1 = i.wrapping_mul(37);
            let p2 = i.wrapping_mul(101).rotate_left(3);
            (p1, p2)
        })
        .collect()
}

/// Per-frame digest accumulated across a run: a rolling hash that folds in
/// every frame's framebuffer + the cycle count, so the assertion catches a
/// divergence on *any* frame, not just the last one.
#[derive(Default, PartialEq, Eq, Debug)]
struct RunDigest {
    fb_rolling: u64,
    audio: u64,
    cycles: u64,
    frames: u64,
}

#[test]
fn movie_round_trip_is_byte_identical_on_real_rom() {
    use rustynes_core::Buttons;
    let inputs = synthetic_inputs(90); // 1.5 s of NTSC frames

    // ----- Original run: drive + record. -----
    let mut nes = Nes::from_rom(DRIVER_ROM).expect("boot driver rom");
    nes.power_cycle(); // pin the start point a replay will reconstruct
    let mut rec = MovieRecorder::power_on(&nes);
    let mut orig = RunDigest::default();
    let mut audio = Vec::new();
    for &(p1, p2) in &inputs {
        nes.set_buttons(0, Buttons::from_bits_truncate(p1));
        nes.set_buttons(1, Buttons::from_bits_truncate(p2));
        rec.capture(&nes);
        orig.fb_rolling ^= fnv(nes.run_frame()).wrapping_add(orig.frames);
        audio.extend(nes.drain_audio());
        orig.frames += 1;
    }
    orig.audio = audio_fnv(&audio);
    orig.cycles = nes.cycle();
    let movie = rec.finish();
    assert_eq!(movie.len(), inputs.len());
    assert!(matches!(movie.start, StartPoint::PowerOn));

    // ----- Replay from the movie's start point. -----
    let bytes = movie.serialize();
    let movie = Movie::deserialize(&bytes).expect("movie round-trips through bytes");
    let mut replay = Nes::from_rom(DRIVER_ROM).expect("boot driver rom (replay)");
    movie.seek_to_start(&mut replay).expect("seek to start");
    let mut player = MoviePlayer::new(&movie);
    let mut rep = RunDigest::default();
    let mut audio = Vec::new();
    while player.apply_next(&mut replay) {
        rep.fb_rolling ^= fnv(replay.run_frame()).wrapping_add(rep.frames);
        audio.extend(replay.drain_audio());
        rep.frames += 1;
    }
    rep.audio = audio_fnv(&audio);
    rep.cycles = replay.cycle();

    assert_eq!(
        orig, rep,
        "movie replay must be byte-identical to the original run"
    );
}

#[test]
fn movie_serialize_deserialize_is_structurally_equal() {
    use rustynes_core::Buttons;
    let mut nes = Nes::from_rom(DRIVER_ROM).expect("boot");
    nes.power_cycle();
    let mut rec = MovieRecorder::power_on(&nes);
    for i in 0..40u8 {
        nes.set_buttons(0, Buttons::from_bits_truncate(i));
        rec.capture(&nes);
        nes.run_frame();
    }
    let movie = rec.finish();
    let bytes = movie.serialize();
    let back = Movie::deserialize(&bytes).expect("round-trip");
    assert_eq!(movie, back);
    // Re-serialising the parsed movie reproduces identical bytes.
    assert_eq!(bytes, back.serialize());
}

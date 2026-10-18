//! v3.4.0 `T-NETPLAY-ROLLBACK-WINDOW` (Round 2 NET-R2-01, -02, -05, -09): the
//! gates on the rollback window.
//!
//! Until v3.4.0 the session kept one canonical checkpoint (a full core
//! snapshot, framebuffer included) and could only move it forward by
//! *replaying*: every tick on which confirmation advanced restored the
//! checkpoint and re-ran every frame up to the present, even when every
//! prediction had been right. And every per-frame vector grew for the whole
//! session. The redesign keeps a bounded window of per-frame entries over
//! the unconfirmed frames; a confirmation that held its predictions promotes
//! entries the session already holds, and a misprediction replays from the
//! mispredicted frame, not from the checkpoint.
//!
//! The three gates the plan names:
//!
//! 1. **No replay when predictions held.** Counted, not inferred:
//!    [`held_predictions_confirm_without_a_replay`] sums every tick's
//!    `resimulated_frames` and asserts zero over a latent session whose remote
//!    inputs always match their prediction, while confirmation advances
//!    hundreds of frames.
//! 2. **Memory bounded by the window.** [`the_window_stays_bounded_over_a_long_session`]
//!    samples [`RollbackSession::retained`] through a long session and asserts
//!    a bound that does not depend on the session's length.
//! 3. **Rollback determinism unchanged.** The existing `determinism.rs` suite,
//!    plus [`a_misprediction_forces_a_replay_and_every_frame_matches`], which
//!    checks that a mismatch does replay, and that every confirmed frame's
//!    digest (not one sampled frame) equals the no-rollback reference on both
//!    peers.

use std::path::PathBuf;

use rustynes_core::{Buttons, Nes};
use rustynes_netplay::{
    LinkConditions, MemoryTransport, RollbackSession, SessionConfig, SessionIdentity, SplitMix64,
    gameplay_digest,
};

fn nestest_rom() -> Vec<u8> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root is two levels above the crate manifest");
    let rom = root.join("tests/roms/nestest/nestest.nes");
    std::fs::read(&rom).unwrap_or_else(|e| panic!("read nestest rom {}: {e}", rom.display()))
}

/// Two sessions over one in-memory link, P1 and P2.
fn pair(
    rom: &[u8],
    conditions: LinkConditions,
    seed: u64,
    cfg: SessionConfig,
) -> Vec<(RollbackSession<MemoryTransport>, Nes)> {
    let (a, b) = MemoryTransport::pair(conditions, seed);
    let n0 = Nes::from_rom(rom).expect("rom parses");
    let n1 = Nes::from_rom(rom).expect("rom parses");
    let id = SessionIdentity::of(&n0);
    let s0 = RollbackSession::new(
        SessionConfig {
            local_player: 0,
            ..cfg
        },
        a,
        id,
    );
    let s1 = RollbackSession::new(
        SessionConfig {
            local_player: 1,
            ..cfg
        },
        b,
        id,
    );
    vec![(s0, n0), (s1, n1)]
}

/// Each peer's confirmed entering digest of every frame, collected as the
/// frames confirm (the session keeps only a recent window of them, so a test
/// that wants all of them reads each tick).
#[derive(Default)]
struct Recorder {
    digests: Vec<u64>,
}

impl Recorder {
    fn collect(&mut self, s: &RollbackSession<MemoryTransport>) {
        while let Some(d) =
            s.confirmed_entering_digest(u32::try_from(self.digests.len()).unwrap_or(u32::MAX))
        {
            self.digests.push(d);
        }
    }
}

/// The no-rollback reference: the entering digest of every frame `0..frames`
/// for one machine fed each player's effective (input-delayed) input.
fn reference_digests(rom: &[u8], streams: [&[Buttons]; 2], frames: u32, delay: u32) -> Vec<u64> {
    let mut nes = Nes::from_rom(rom).expect("rom parses");
    let mut out = Vec::with_capacity(frames as usize);
    for f in 0..frames {
        out.push(gameplay_digest(&nes));
        for (port, s) in streams.iter().enumerate() {
            let b = if f < delay {
                Buttons::empty()
            } else {
                s.get((f - delay) as usize).copied().unwrap_or_default()
            };
            nes.set_buttons(port, b);
        }
        let _ = nes.run_frame();
    }
    out
}

/// GATE 1 (NET-R2-01). Both players hold no buttons, so every prediction ("the
/// last known input", or none before the first) is right, but every remote
/// input arrives five polls late (three more than the input delay hides), so
/// confirmation trails the present by several frames and advances on nearly every tick. Before v3.4.0 each of
/// those advances restored the checkpoint and replayed to the present; now
/// not one frame may be re-simulated.
#[test]
fn held_predictions_confirm_without_a_replay() {
    let rom = nestest_rom();
    let mut peers = pair(
        &rom,
        LinkConditions::fixed_latency(5),
        0x0E1D,
        SessionConfig::default(),
    );
    let mut resimulated = [0u64; 2];
    let mut rollbacks = [0u32; 2];
    for _ in 0..300 {
        for (i, (s, nes)) in peers.iter_mut().enumerate() {
            s.add_local_input(Buttons::empty());
            let out = s.advance(nes).expect("no desync");
            resimulated[i] += u64::from(out.resimulated_frames);
            rollbacks[i] += u32::from(out.rolled_back);
        }
    }
    for (i, (s, _)) in peers.iter().enumerate() {
        let confirmed = s.last_confirmed_frame().expect("frames confirmed");
        assert!(
            confirmed >= 250,
            "peer {i}: confirmation must advance for the gate to mean anything (confirmed {confirmed})"
        );
        assert!(
            s.current_frame() - confirmed >= 3,
            "peer {i}: confirmation must trail the present (latency), else nothing is speculative"
        );
    }
    assert_eq!(
        resimulated,
        [0, 0],
        "a confirmation that held its predictions must not replay"
    );
    assert_eq!(rollbacks, [0, 0]);
}

/// GATE 1, the other half: input that changes now and then. Each change is
/// mispredicted once (the prediction repeats the old value), so it is replayed
/// once, from the mispredicted frame; the frames between changes are not. The
/// bound is what the changes alone can cost, far below the per-tick replay of
/// the old design (hundreds of frames over this session).
#[test]
fn only_mispredictions_replay() {
    let rom = nestest_rom();
    let cfg = SessionConfig::default();
    let mut peers = pair(&rom, LinkConditions::fixed_latency(3), 0x0E1E, cfg);
    let frames = 360u32;
    // Every player changes its buttons every 60 frames.
    let stream = |p: u32, f: u32| {
        Buttons::from_bits_truncate(u8::try_from(((f / 60) * 37 + p * 11) % 251).unwrap_or(0))
    };
    let mut resimulated = 0u64;
    let mut rollbacks = 0u32;
    let mut authored = [0u32; 2];
    for _ in 0..frames + 60 {
        for ((i, (s, nes)), p) in peers.iter_mut().enumerate().zip(0u32..) {
            while authored[i] <= s.current_frame() && authored[i] < frames {
                s.add_local_input(stream(p, authored[i]));
                authored[i] += 1;
            }
            let out = s.advance(nes).expect("no desync");
            resimulated += u64::from(out.resimulated_frames);
            rollbacks += u32::from(out.rolled_back);
        }
    }
    let changes = u64::from(frames / 60) * 2;
    let per_change = u64::from(cfg.max_rollback_frames) + 1;
    assert!(rollbacks > 0, "the changes must be mispredicted");
    assert!(
        resimulated <= changes * per_change,
        "only mispredictions may replay: {resimulated} frames re-simulated for {changes} changes"
    );
}

/// GATE 3. Random input over a latent, jittery, lossy link: a misprediction
/// must replay, and every confirmed frame on both peers must equal the
/// no-rollback reference, frame by frame.
#[test]
fn a_misprediction_forces_a_replay_and_every_frame_matches() {
    let rom = nestest_rom();
    let cfg = SessionConfig::default();
    let frames = 600u32;
    let mut rng = [SplitMix64::new(0xA1), SplitMix64::new(0xB2)];
    let streams: [Vec<Buttons>; 2] = [0, 1].map(|p| {
        (0..frames)
            .map(|_| Buttons::from_bits_truncate(rng[p].next_u8()))
            .collect()
    });
    let conditions = LinkConditions {
        latency_polls: 2,
        jitter_polls: 2,
        drop_prob: 0.05,
    };
    let mut peers = pair(&rom, conditions, 0xF00D, cfg);
    let mut rec = [Recorder::default(), Recorder::default()];
    let mut authored = [0u32; 2];
    let mut rollbacks = 0u32;
    let mut ticks = 0;
    while rec.iter().any(|r| r.digests.len() < (frames - 30) as usize) && ticks < frames * 40 {
        ticks += 1;
        for (i, (s, nes)) in peers.iter_mut().enumerate() {
            while authored[i] <= s.current_frame() && authored[i] < frames {
                s.add_local_input(streams[i][authored[i] as usize]);
                authored[i] += 1;
            }
            rollbacks += u32::from(s.advance(nes).expect("no desync").rolled_back);
            rec[i].collect(s);
        }
    }
    let compared = frames - 30;
    let n = compared as usize;
    assert!(
        rec.iter().all(|r| r.digests.len() >= n),
        "both peers confirmed {n} frames"
    );
    assert!(
        rollbacks > 0,
        "random input over a latent link must mispredict"
    );
    let reference = reference_digests(&rom, [&streams[0], &streams[1]], compared, cfg.input_delay);
    for (f, want) in reference.iter().enumerate() {
        assert_eq!(rec[0].digests[f], *want, "peer 0, frame {f}");
        assert_eq!(rec[1].digests[f], *want, "peer 1, frame {f}");
    }
}

/// GATE 2 (NET-R2-02, -09). The per-frame state a session keeps must be
/// bounded by the window, not by the session's length: sampled through a long
/// session, no count may exceed a bound computed from the configuration
/// alone, and the snapshot bytes must stay those of one core snapshot per
/// entry.
#[test]
fn the_window_stays_bounded_over_a_long_session() {
    let rom = nestest_rom();
    let cfg = SessionConfig::default();
    let conditions = LinkConditions {
        latency_polls: 2,
        jitter_polls: 1,
        drop_prob: 0.02,
    };
    let mut peers = pair(&rom, conditions, 0x5E55, cfg);
    let mut rng = SplitMix64::new(0x77);
    let frames = 3_000u32;
    let mut peak = rustynes_netplay::RetainedWindow::default();
    let mut ticks = 0;
    while peers[0].0.current_frame() < frames && ticks < frames * 4 {
        ticks += 1;
        for (s, nes) in &mut peers {
            s.add_local_input(Buttons::from_bits_truncate(rng.next_u8()));
            s.advance(nes).expect("no desync");
            let r = s.retained();
            peak.history_frames = peak.history_frames.max(r.history_frames);
            peak.window_entries = peak.window_entries.max(r.window_entries);
            peak.window_bytes = peak.window_bytes.max(r.window_bytes);
            peak.confirmed_digests = peak.confirmed_digests.max(r.confirmed_digests);
            peak.checksums = peak.checksums.max(r.checksums);
        }
    }
    assert!(
        peers[0].0.current_frame() >= frames,
        "the session ran {frames} frames"
    );
    let bound = rustynes_netplay::RetainedWindow::bound(&cfg);
    assert!(
        peak.history_frames <= bound.history_frames
            && peak.window_entries <= bound.window_entries
            && peak.confirmed_digests <= bound.confirmed_digests
            && peak.checksums <= bound.checksums,
        "retained state {peak:?} exceeds the window bound {bound:?}"
    );
    // Each entry is one full core snapshot (about 256 KiB for nestest; see
    // `WindowEntry` for why not a slim one), so the bytes are bounded by the
    // entry count, never by the session length.
    let per_entry = peak.window_bytes / peak.window_entries.max(1);
    assert!(
        per_entry < 320 * 1024,
        "a window entry must be one core snapshot: {per_entry} bytes per entry"
    );
}

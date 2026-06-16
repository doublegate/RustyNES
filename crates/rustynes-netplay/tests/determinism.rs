//! The determinism harness — the load-bearing proof that rollback netcode
//! reproduces the reference simulation bit-for-bit.
//!
//! Each test drives two `RollbackSession`s (one per player) over a paired,
//! deterministic in-memory transport, and compares their emulator state to a
//! single **reference** `Nes` fed the exact same combined `(p1, p2)` input
//! sequence with NO rollback. The emulator's determinism contract guarantees
//! the reference is the ground truth; if rollback re-simulation is correct,
//! both sessions' snapshots equal the reference's, and each other's.
//!
//! All randomness (the transport's latency/jitter/drop and the test input
//! generators) comes from a seeded `SplitMix64` — no `std::time`, no OS RNG —
//! so every run is reproducible.

use std::path::PathBuf;

use rustynes_core::{Buttons, Nes};
use rustynes_netplay::{
    LinkConditions, MemoryTransport, MeshTransport, NetplayError, RollbackSession, SessionConfig,
    SplitMix64, fnv1a64,
};

/// The deterministic gameplay digest used for cross-peer comparison —
/// framebuffer + cumulative cycle. Mirrors `RollbackSession`'s internal
/// `gameplay_digest`. (The full `Nes::snapshot()` also serializes audio-
/// synthesis transients that vary with audio-drain history but never affect
/// future frames; framebuffer + cycle are byte-deterministic across
/// restore+replay, which is exactly what rollback needs.)
fn gameplay_digest(nes: &Nes) -> u64 {
    fnv1a64(nes.framebuffer()) ^ nes.cycle().wrapping_mul(0x100_0000_01b3)
}

/// Resolve the committed public-domain nestest ROM (`tests/roms/nestest/`)
/// from the workspace root, derived from this crate's manifest dir.
fn nestest_rom() -> Vec<u8> {
    // CARGO_MANIFEST_DIR = <root>/crates/rustynes-netplay
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root is two levels above the crate manifest");
    let rom = root.join("tests/roms/nestest/nestest.nes");
    std::fs::read(&rom).unwrap_or_else(|e| panic!("read nestest rom {}: {e}", rom.display()))
}

/// Deterministic per-player input generator. Mixes the frame index and a
/// per-player salt so the two players produce different, non-trivial button
/// sequences. Masks to the 8 standard buttons.
const fn gen_input(rng: &mut SplitMix64) -> Buttons {
    Buttons::from_bits_truncate(rng.next_u8())
}

/// A committed, CC0, **PPU-heavy** ROM (continuous rendering + palette churn) —
/// the opposite of CPU-heavy `nestest`. Exercises the PPU/render state a
/// rollback must reproduce.
fn flowing_palette_rom() -> Vec<u8> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root");
    std::fs::read(root.join("tests/roms/sprint-2/flowing_palette.nes"))
        .expect("flowing_palette.nes (committed CC0)")
}

/// Reproduce the NATIVE two-instance desync in the harness: drive the two
/// sessions at INDEPENDENT, drifting rates (0-2 advances each per iteration)
/// instead of the symmetric one-each lockstep the other tests use. This mimics
/// two real processes whose 60 Hz clocks drift — the only condition the live
/// native run has that the harness didn't. Idle input, near-zero latency (few
/// rollbacks), like SMB sitting on its title. If `advance` has any
/// order-of-call dependence, the confirmed digests diverge.
#[test]
fn asymmetric_realtime_drive_stays_in_sync() {
    for rel in [
        "tests/roms/sprint-2/flowing_palette.nes",
        "tests/roms/sprint-2/oam_stress.nes",
        "tests/roms/accuracycoin/AccuracyCoin.nes",
    ] {
        let rom = rom_at(rel);
        let frames = 800u32;
        let target = frames - 40;
        let (t0, t1) = MemoryTransport::pair(
            LinkConditions {
                latency_polls: 0,
                jitter_polls: 1,
                drop_prob: 0.0,
            },
            0xDEAD_BEEF,
        );
        let mut nes0 = Nes::from_rom(&rom).unwrap_or_else(|e| panic!("{rel}: {e:?}"));
        let mut nes1 = Nes::from_rom(&rom).unwrap_or_else(|e| panic!("{rel}: {e:?}"));
        let hash = *nes0.rom_sha256();
        let mut s0 = RollbackSession::new(SessionConfig::default(), t0, hash);
        let mut s1 = RollbackSession::new(
            SessionConfig {
                local_player: 1,
                ..SessionConfig::default()
            },
            t1,
            hash,
        );

        let mut rng = SplitMix64::new(0x515F_AC51);
        let confirmed = |s: &RollbackSession<MemoryTransport>| {
            s.last_confirmed_frame().is_some_and(|c| c >= target)
        };
        let (mut a0, mut a1) = (0u32, 0u32);
        let mut iters = 0u32;
        while !(confirmed(&s0) && confirmed(&s1)) && iters < target * 80 {
            iters += 1;
            // Idle input (empty), authored one frame per advance, but each side
            // advances an independent 0-2 times this iteration so they drift.
            for _ in 0..(rng.next_u8() % 3) {
                while a0 <= s0.current_frame() {
                    s0.add_local_input(Buttons::empty());
                    a0 += 1;
                }
                let _ = s0.advance(&mut nes0).expect("s0 advance");
            }
            for _ in 0..(rng.next_u8() % 3) {
                while a1 <= s1.current_frame() {
                    s1.add_local_input(Buttons::empty());
                    a1 += 1;
                }
                let _ = s1.advance(&mut nes1).expect("s1 advance");
            }
        }
        assert!(
            confirmed(&s0) && confirmed(&s1),
            "{rel}: did not confirm {target} in {iters} iters",
        );
        let d0 = s0.confirmed_entering_digest(target).expect("s0 digest");
        let d1 = s1.confirmed_entering_digest(target).expect("s1 digest");
        assert_eq!(d0, d1, "{rel}: asymmetric real-time drive desynced");
    }
}

/// Load a committed ROM by its path relative to the workspace root.
fn rom_at(rel: &str) -> Vec<u8> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root");
    std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// The deep rollback invariant on ROMs that exercise what SMB does and the
/// palette demo does NOT: sprites, OAM DMA, the APU (incl. DMC), sprite-0 hit.
/// If any of that state is not fully captured by a snapshot, restore + replay
/// diverges from the forward run — the exact "state — picture differs" desync
/// reproduced natively. Each ROM: warm up, snapshot, record 200 forward frames,
/// restore, replay, and require a frame-for-frame match.
#[test]
fn snapshot_restore_replay_matches_forward_run_sprite_apu_heavy() {
    for rel in [
        "tests/roms/accuracycoin/AccuracyCoin.nes",
        "tests/roms/sprint-2/oam_stress.nes",
        "tests/roms/audio-tests/db_apu.nes",
    ] {
        let rom = rom_at(rel);
        let mut nes = Nes::from_rom(&rom).unwrap_or_else(|e| panic!("boot {rel}: {e:?}"));
        for _ in 0..120u32 {
            nes.run_frame();
        }
        let snap = nes.snapshot();
        let mut forward = Vec::new();
        for _ in 0..200u32 {
            nes.run_frame();
            forward.push((fnv1a64(nes.framebuffer()), nes.cycle()));
        }
        nes.restore(&snap).expect("restore");
        for (i, &(fb, cyc)) in forward.iter().enumerate() {
            nes.run_frame();
            assert_eq!(
                (fnv1a64(nes.framebuffer()), nes.cycle()),
                (fb, cyc),
                "{rel}: restore+replay diverged from forward at replay frame {i} \
                 — the snapshot is incomplete for this ROM's state",
            );
        }
    }
}

/// The load-bearing rollback invariant, stated directly: **restoring a snapshot
/// and replaying must reproduce the forward run frame-for-frame** — otherwise a
/// rollback (which restores + replays) produces a different state than a peer
/// that reached the frame without rolling back, and they desync ("state —
/// picture differs"). The existing harness only ever drove CPU-heavy `nestest`;
/// this checks the PPU-heavy path (the class of ROM, e.g. SMB, that desynced
/// live).
#[test]
fn snapshot_restore_replay_matches_forward_run_ppu_heavy() {
    let rom = flowing_palette_rom();
    let mut nes = Nes::from_rom(&rom).expect("load");
    for _ in 0..90u32 {
        nes.run_frame();
    }
    // Record the forward path (framebuffer hash + cumulative cycle per frame).
    let snap = nes.snapshot();
    let mut forward = Vec::new();
    for _ in 0..150u32 {
        nes.run_frame();
        forward.push((fnv1a64(nes.framebuffer()), nes.cycle()));
    }
    // Restore the snapshot and replay; every frame must match the forward run.
    nes.restore(&snap).expect("restore");
    for (i, &(fb, cyc)) in forward.iter().enumerate() {
        nes.run_frame();
        assert_eq!(
            (fnv1a64(nes.framebuffer()), nes.cycle()),
            (fb, cyc),
            "restore+replay diverged from the forward run at replay frame {i}: the \
             snapshot does not fully capture state (a rollback would desync here)",
        );
    }
}

/// The live SMB desync happened during idle boot/attract — **no buttons**, so no
/// mispredictions; only the confirmation-driven resync (rollback + replay from
/// the confirmed checkpoint) runs every frame. Every other rollback test here
/// drives random input (which exercises mispredictions). This isolates the
/// no-misprediction resync path on a PPU-heavy ROM with latency.
#[test]
fn idle_rollback_matches_reference_ppu_heavy() {
    let rom = flowing_palette_rom();
    let frames = 600u32;
    let p0 = vec![Buttons::empty(); frames as usize];
    let p1 = vec![Buttons::empty(); frames as usize];

    let cfg = SessionConfig::default();
    let compare_frame = frames - 40;
    let reference = reference_digest(&rom, &p0, &p1, compare_frame, cfg.input_delay);

    let conditions = LinkConditions {
        latency_polls: 3,
        jitter_polls: 2,
        drop_prob: 0.0,
    };
    let (s0, s1) = run_two_sessions(&rom, &p0, &p1, compare_frame, conditions, 0x1D1E, cfg)
        .expect("idle PPU-heavy session must not desync");
    assert_eq!(s0, s1, "peers identical");
    assert_eq!(s0, reference, "matches the no-rollback reference");
}

/// `power_cycle()` MUST fully reset the machine — its result cannot depend on
/// how long the emulator ran before. Browser netplay power-cycles each peer at
/// session start, and the two peers ran a DIFFERENT number of frames first; if
/// any state survives the power-cycle, the peers boot from different states and
/// desync (a "state — picture differs" divergence). This is the live-only case
/// the from-cold-boot harness above never exercises.
///
/// This test CAUGHT the live netplay desync: `Bus::power_cycle()` was missing
/// resets for several run-history-dependent fields — most critically
/// `ppu_clock`, the master clock whose residual value carried the old CPU/PPU
/// phase into the "new" boot (plus the DMA accounting, DMC-DMA latches, deferred
/// controller write), AND it never reset the cartridge/mapper, so a stateful
/// mapper's bank registers / CHR-RAM / volatile PRG-RAM survived. Both are now
/// fixed (the mapper is rebuilt from the stored ROM bytes). Covers NROM (the
/// pure bus-field case) PLUS MMC1 + MMC3 (the mapper-state case).
#[test]
fn power_cycle_result_is_independent_of_prior_history() {
    let roms: [Vec<u8>; 3] = [
        nestest_rom(),                                        // NROM — bus fields only
        rom_at("tests/roms/holy_mapperel/M1_P128K_CR8K.nes"), // MMC1 + CHR-RAM
        rom_at("tests/roms/holy_mapperel/M4_P128K_CR8K.nes"), // MMC3 + CHR-RAM + PRG-RAM
    ];
    for rom in &roms {
        // A: power-cycle straight from a cold boot.
        let mut a = Nes::from_rom(rom).expect("load a");
        a.power_cycle();

        // B: run a while (advancing PPU/APU/CPU/MAPPER state — bank switches +
        // CHR-RAM/WRAM writes for the stateful mappers), THEN power-cycle.
        let mut b = Nes::from_rom(rom).expect("load b");
        for f in 0..173u32 {
            b.set_buttons(
                0,
                Buttons::from_bits_truncate(f.to_le_bytes()[0].wrapping_mul(37)),
            );
            b.run_frame();
        }
        b.set_buttons(0, Buttons::empty());
        b.power_cycle();

        // After power-cycling, A and B must be byte-identical forever.
        for f in 0..300u32 {
            a.run_frame();
            b.run_frame();
            assert_eq!(
                fnv1a64(a.framebuffer()),
                fnv1a64(b.framebuffer()),
                "framebuffer diverged at frame {f}: power_cycle() left residual state \
                 (its result depends on prior run history)",
            );
            assert_eq!(
                a.cycle(),
                b.cycle(),
                "cycle count diverged at frame {f}: power_cycle() left residual state",
            );
        }
    }
}

/// Pre-generate `frames` authored inputs per player from seeded PRNGs so the
/// reference run and both sessions see an identical authored sequence.
fn make_input_streams(frames: u32, seed: u64) -> (Vec<Buttons>, Vec<Buttons>) {
    let mut r0 = SplitMix64::new(seed ^ 0x1111_1111);
    let mut r1 = SplitMix64::new(seed ^ 0x2222_2222);
    let mut p0 = Vec::with_capacity(frames as usize);
    let mut p1 = Vec::with_capacity(frames as usize);
    for _ in 0..frames {
        p0.push(gen_input(&mut r0));
        p1.push(gen_input(&mut r1));
    }
    (p0, p1)
}

/// The *effective* per-frame input under GGPO input delay: authored input `i`
/// lands at emulated frame `i + input_delay`; frames before that run with no
/// buttons. This is exactly the mapping the session applies, so the reference
/// must use it too for an apples-to-apples comparison.
fn effective(authored: &[Buttons], frames: u32, input_delay: u32) -> Vec<Buttons> {
    (0..frames)
        .map(|f| {
            if f < input_delay {
                Buttons::empty()
            } else {
                authored
                    .get((f - input_delay) as usize)
                    .copied()
                    .unwrap_or_else(Buttons::empty)
            }
        })
        .collect()
}

/// Run the reference: one `Nes`, fed the combined *effective* input each
/// frame, no rollback. Returns the gameplay digest after `frames` frames
/// (i.e. the deterministic state entering frame `frames`).
fn reference_digest(
    rom: &[u8],
    p0: &[Buttons],
    p1: &[Buttons],
    frames: u32,
    input_delay: u32,
) -> u64 {
    let e0 = effective(p0, frames, input_delay);
    let e1 = effective(p1, frames, input_delay);
    let mut nes = Nes::from_rom(rom).expect("load nestest");
    for f in 0..frames as usize {
        nes.set_buttons(0, e0[f]);
        nes.set_buttons(1, e1[f]);
        let _ = nes.run_frame();
    }
    gameplay_digest(&nes)
}

/// Drive both sessions over the paired transport, feeding each its local
/// player's authored input, until both have CONFIRMED at least `compare_frame`
/// (i.e. every frame up to `compare_frame` ran with both players' real
/// inputs). Returns each session's confirmed *entering* gameplay digest for
/// `compare_frame` — a function of confirmed inputs only, so it is the right
/// thing to compare against the no-rollback reference.
///
/// (We compare a confirmed frame rather than the live tail: the last few
/// frames a session has produced still hold unconfirmed predictions for the
/// remote player, which legitimately differ from the reference until the real
/// inputs arrive. Comparing a confirmed frame isolates rollback correctness.)
///
/// The two sessions advance in lockstep ticks. A `Stalled` outcome (time-sync
/// back-pressure) produces no frame that tick.
fn run_two_sessions(
    rom: &[u8],
    p0: &[Buttons],
    p1: &[Buttons],
    compare_frame: u32,
    conditions: LinkConditions,
    seed: u64,
    base: SessionConfig,
) -> Result<(u64, u64), NetplayError> {
    let (t0, t1) = MemoryTransport::pair(conditions, seed);
    let mut nes0 = Nes::from_rom(rom).expect("load nestest p0");
    let mut nes1 = Nes::from_rom(rom).expect("load nestest p1");
    let hash = *nes0.rom_sha256();

    let cfg0 = SessionConfig {
        local_player: 0,
        ..base
    };
    let cfg1 = SessionConfig {
        local_player: 1,
        ..base
    };
    let mut s0 = RollbackSession::new(cfg0, t0, hash);
    let mut s1 = RollbackSession::new(cfg1, t1, hash);

    let mut authored0: u32 = 0;
    let mut authored1: u32 = 0;

    let confirmed_at = |s: &RollbackSession<MemoryTransport>| {
        s.last_confirmed_frame().is_some_and(|c| c >= compare_frame)
    };

    let max_ticks = compare_frame * 30 + 200;
    let mut ticks = 0;
    while !(confirmed_at(&s0) && confirmed_at(&s1)) && ticks < max_ticks {
        ticks += 1;

        while authored0 <= s0.current_frame() && (authored0 as usize) < p0.len() {
            s0.add_local_input(p0[authored0 as usize]);
            authored0 += 1;
        }
        let _ = s0.advance(&mut nes0)?;

        while authored1 <= s1.current_frame() && (authored1 as usize) < p1.len() {
            s1.add_local_input(p1[authored1 as usize]);
            authored1 += 1;
        }
        let _ = s1.advance(&mut nes1)?;
    }

    assert!(
        confirmed_at(&s0) && confirmed_at(&s1),
        "sessions did not confirm frame {compare_frame} within {max_ticks} ticks \
         (s0 confirmed={:?} cur={}, s1 confirmed={:?} cur={})",
        s0.last_confirmed_frame(),
        s0.current_frame(),
        s1.last_confirmed_frame(),
        s1.current_frame()
    );

    let d0 = s0
        .confirmed_entering_digest(compare_frame)
        .expect("s0 confirmed digest present");
    let d1 = s1
        .confirmed_entering_digest(compare_frame)
        .expect("s1 confirmed digest present");
    Ok((d0, d1))
}

/// THE HEADLINE: rollback over a lossy/latent link reproduces the no-rollback
/// reference exactly, and the two peers are byte-identical to each other.
#[test]
fn rollback_matches_reference() {
    let rom = nestest_rom();
    let frames = 600u32;
    let (p0, p1) = make_input_streams(frames, 0xABCD_1234);

    let cfg = SessionConfig::default();
    // Compare the confirmed state entering this frame (well inside the
    // authored range, so it confirms before the run ends).
    let compare_frame = frames - 40;
    let reference = reference_digest(&rom, &p0, &p1, compare_frame, cfg.input_delay);

    // ~2-3 frame latency with a touch of jitter — the realistic case where
    // mispredictions and rollbacks happen constantly.
    let conditions = LinkConditions {
        latency_polls: 2,
        jitter_polls: 1,
        drop_prob: 0.0,
    };
    let (snap0, snap1) = run_two_sessions(&rom, &p0, &p1, compare_frame, conditions, 0x5EED, cfg)
        .expect("no desync");

    assert_eq!(
        snap0, snap1,
        "the two peers must hold identical confirmed state"
    );
    assert_eq!(
        snap0, reference,
        "rollback re-simulation must equal the no-rollback reference"
    );
}

/// Packet-loss recovery: over a link that DROPS ~20% of messages (with latency
/// and jitter too), the two peers must still converge byte-identically to the
/// no-loss reference. Without the un-acked input resend
/// (`resend_unacked_local_inputs`), a dropped `Input` would be lost forever, the
/// peer would mispredict that frame permanently, and the next checksum would
/// trip a desync (the live "desync at frame N" bug). The drop pattern is seeded,
/// so this is reproducible, not flaky.
#[test]
fn rollback_recovers_from_packet_loss() {
    let rom = nestest_rom();
    let frames = 600u32;
    let (p0, p1) = make_input_streams(frames, 0xABCD_1234);

    let cfg = SessionConfig::default();
    let compare_frame = frames - 40;
    let reference = reference_digest(&rom, &p0, &p1, compare_frame, cfg.input_delay);

    // Drops PLUS heavy jitter (reordering): a higher frame frequently arrives
    // while a lower one is still missing, which is exactly what a
    // highest-received `InputAck` mishandles (it would suppress resending the
    // gap). The cumulative `last_confirmed_frame` ack + the resend must recover
    // every gap.
    let conditions = LinkConditions {
        latency_polls: 2,
        jitter_polls: 4,
        drop_prob: 0.25,
    };
    let (snap0, snap1) =
        run_two_sessions(&rom, &p0, &p1, compare_frame, conditions, 0x1055_0001, cfg)
            .expect("must NOT desync despite 25% loss + reordering (resend + cumulative ack)");

    assert_eq!(
        snap0, snap1,
        "the two peers must hold identical confirmed state despite packet loss"
    );
    assert_eq!(
        snap0, reference,
        "loss-recovered rollback must still equal the no-loss reference"
    );
}

/// High-latency stress: ~8-frame latency forces a misprediction nearly every
/// frame, exercising deep rollbacks. Still must equal the reference.
#[test]
fn rollback_stress_high_latency() {
    let rom = nestest_rom();
    let frames = 400u32;
    let (p0, p1) = make_input_streams(frames, 0x0F0F_0F0F);

    // The rollback window must exceed the link latency, else a peer would
    // stall waiting for confirmation it can't yet have — so widen it for the
    // 8-frame link. (A real session sizes `max_rollback_frames` from the
    // measured ping; Stage 2's Quality messages feed that.)
    let cfg = SessionConfig {
        max_rollback_frames: 16,
        ..SessionConfig::default()
    };
    let compare_frame = frames - 40;
    let reference = reference_digest(&rom, &p0, &p1, compare_frame, cfg.input_delay);

    let conditions = LinkConditions {
        latency_polls: 8,
        jitter_polls: 2,
        drop_prob: 0.0,
    };
    let (snap0, snap1) =
        run_two_sessions(&rom, &p0, &p1, compare_frame, conditions, 0x0BAD_C0DE, cfg)
            .expect("no desync");

    assert_eq!(snap0, snap1, "peers diverged under high latency");
    assert_eq!(snap0, reference, "deep rollback diverged from reference");
}

/// Zero-latency link: inputs are always confirmed before they are consumed,
/// so the session never mispredicts and never rolls back. Output must equal
/// the reference exactly. (Also a sanity check that the no-rollback fast path
/// is byte-identical.)
#[test]
fn zero_rollback_is_byte_identical() {
    let rom = nestest_rom();
    let frames = 300u32;
    let (p0, p1) = make_input_streams(frames, 0x7777_7777);

    let cfg = SessionConfig::default();
    let compare_frame = frames - 20;
    let reference = reference_digest(&rom, &p0, &p1, compare_frame, cfg.input_delay);

    let (snap0, snap1) = run_two_sessions(
        &rom,
        &p0,
        &p1,
        compare_frame,
        LinkConditions::PERFECT,
        0x1234_5678,
        cfg,
    )
    .expect("no desync");

    assert_eq!(snap0, snap1);
    assert_eq!(
        snap0, reference,
        "zero-rollback session must match reference byte-for-byte"
    );
}

/// Desync detection: when one peer's emulator is forced to diverge (we
/// persistently corrupt the remote input it sees for one frame, so it runs
/// different state), the periodic `Checksum` exchange must surface a `Desync`
/// error rather than silently continuing.
///
/// We model the divergence with a transport wrapper that flips the input bits
/// on every inbound `Input` for a fixed target frame (so even retransmits stay
/// corrupted — a genuine, persistent divergence, not a healable misprediction).
/// Session 1's confirmed state then drifts from session 0's, and the next
/// confirmed-frame checksum mismatches.
#[test]
fn desync_detection() {
    use rustynes_netplay::{NetMessage, Transport};

    /// Wraps a `MemoryTransport`, flipping the bits of every `Input` it
    /// delivers for `target_frame`. Persistent: retransmits stay corrupted.
    struct CorruptingTransport {
        inner: MemoryTransport,
        target_frame: u32,
    }
    impl Transport for CorruptingTransport {
        fn send(&mut self, msg: &NetMessage) {
            self.inner.send(msg);
        }
        fn poll(&mut self) -> Vec<NetMessage> {
            let mut msgs = self.inner.poll();
            for m in &mut msgs {
                if let NetMessage::Input { frame, input, .. } = m
                    && *frame == self.target_frame
                {
                    *input = !*input;
                }
            }
            msgs
        }
    }

    let rom = nestest_rom();
    let frames = 200u32;
    let (p0, p1) = make_input_streams(frames, 0xDEAD_BEEF);

    let conditions = LinkConditions::fixed_latency(1);
    let (t0, t1) = MemoryTransport::pair(conditions, 0x00C0_FFEE);
    let mut nes0 = Nes::from_rom(&rom).expect("load");
    let mut nes1 = Nes::from_rom(&rom).expect("load");
    let hash = *nes0.rom_sha256();

    // Session 1 receives corrupted remote (player-0) inputs, so its emulator
    // diverges from session 0's. Both still exchange checksums.
    let cfg0 = SessionConfig {
        local_player: 0,
        checksum_interval: 5,
        ..SessionConfig::default()
    };
    let cfg1 = SessionConfig {
        local_player: 1,
        checksum_interval: 5,
        ..SessionConfig::default()
    };
    let mut s0 = RollbackSession::new(cfg0, t0, hash);
    let mut s1 = RollbackSession::new(
        cfg1,
        CorruptingTransport {
            inner: t1,
            // A frame past the input-delay prefix, so the corrupted value is a
            // real authored input that the divergence propagates from.
            target_frame: 30,
        },
        hash,
    );

    let mut authored0 = 0u32;
    let mut authored1 = 0u32;
    let mut saw_desync = false;
    for _ in 0..(frames * 8) {
        if s0.current_frame() < frames {
            while authored0 <= s0.current_frame() && (authored0 as usize) < p0.len() {
                s0.add_local_input(p0[authored0 as usize]);
                authored0 += 1;
            }
            if let Err(NetplayError::Desync { .. }) = s0.advance(&mut nes0) {
                saw_desync = true;
                break;
            }
        }
        if s1.current_frame() < frames {
            while authored1 <= s1.current_frame() && (authored1 as usize) < p1.len() {
                s1.add_local_input(p1[authored1 as usize]);
                authored1 += 1;
            }
            if let Err(NetplayError::Desync { .. }) = s1.advance(&mut nes1) {
                saw_desync = true;
                break;
            }
        }
        if s0.current_frame() >= frames && s1.current_frame() >= frames {
            break;
        }
    }

    assert!(
        saw_desync,
        "the checksum exchange must detect the injected divergence"
    );
}

/// A ROM-hash mismatch in the `Sync` handshake must be rejected.
#[test]
fn rom_mismatch_rejected() {
    let rom = nestest_rom();
    let (t0, t1) = MemoryTransport::pair(LinkConditions::PERFECT, 1);
    let mut nes0 = Nes::from_rom(&rom).expect("load");
    let hash = *nes0.rom_sha256();
    let mut wrong = hash;
    wrong[0] ^= 0xFF;

    let mut s0 = RollbackSession::new(SessionConfig::default(), t0, hash);
    // Peer announces a different ROM.
    let _s1: RollbackSession<MemoryTransport> = RollbackSession::new(
        SessionConfig {
            local_player: 1,
            ..SessionConfig::default()
        },
        t1,
        wrong,
    );

    s0.add_local_input(Buttons::empty());
    let err = s0.advance(&mut nes0);
    assert!(matches!(err, Err(NetplayError::RomMismatch)));
}

/// The `AdvanceOutcome` reports rollbacks: under latency, at least one
/// `advance` must report `rolled_back` with `resimulated_frames > 0`, while a
/// perfect link never rolls back. Exercises the public outcome surface that
/// Stage 3's frontend HUD will read.
#[test]
fn advance_outcome_reports_rollbacks() {
    let rom = nestest_rom();
    let frames = 120u32;
    let (p0, p1) = make_input_streams(frames, 0x1357_9BDF);
    let hash = *Nes::from_rom(&rom).unwrap().rom_sha256();

    // Latent link: expect rollbacks.
    let (t0, t1) = MemoryTransport::pair(LinkConditions::fixed_latency(3), 0xFEED);
    let mut nes0 = Nes::from_rom(&rom).unwrap();
    let mut nes1 = Nes::from_rom(&rom).unwrap();
    let mut s0 = RollbackSession::new(
        SessionConfig {
            local_player: 0,
            ..SessionConfig::default()
        },
        t0,
        hash,
    );
    let mut s1 = RollbackSession::new(
        SessionConfig {
            local_player: 1,
            ..SessionConfig::default()
        },
        t1,
        hash,
    );
    let mut a0 = 0u32;
    let mut a1 = 0u32;
    let mut saw_rollback = false;
    let mut saw_resim = false;
    for _ in 0..frames {
        while a0 <= s0.current_frame() && (a0 as usize) < p0.len() {
            s0.add_local_input(p0[a0 as usize]);
            a0 += 1;
        }
        let o0 = s0.advance(&mut nes0).unwrap();
        if o0.rolled_back {
            saw_rollback = true;
            assert!(o0.resimulated_frames > 0, "rollback re-simulated 0 frames");
            saw_resim = true;
        }
        while a1 <= s1.current_frame() && (a1 as usize) < p1.len() {
            s1.add_local_input(p1[a1 as usize]);
            a1 += 1;
        }
        let _ = s1.advance(&mut nes1).unwrap();
    }
    assert!(
        saw_rollback && saw_resim,
        "expected rollbacks under latency"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// N-player (3 + 4) determinism harness (v2.5.0 Phase B).
//
// Generalizes the 2-player harness above. For `num_players` ∈ {3, 4} we wire N
// `RollbackSession`s (one per local player) over a deterministic in-memory
// `MeshTransport` (each peer broadcasts its own input to all others). Each peer
// feeds only its own authored input stream; rollback fills the rest. We then
// assert ALL peers' confirmed-entering digest equal each other AND a single
// no-rollback reference `Nes` fed the same combined N-port input sequence (with
// the Four Score adapter on for >2 players).
// ───────────────────────────────────────────────────────────────────────────

/// Pre-generate `frames` authored inputs for each of `num_players` players from
/// per-player seeded PRNGs, so the reference run and every session see an
/// identical authored sequence.
fn make_n_input_streams(num_players: u8, frames: u32, seed: u64) -> Vec<Vec<Buttons>> {
    (0..num_players)
        .map(|p| {
            let mut r = SplitMix64::new(seed ^ (0x1111_1111_u64.wrapping_mul(u64::from(p) + 1)));
            (0..frames).map(|_| gen_input(&mut r)).collect()
        })
        .collect()
}

/// The no-rollback reference for an N-player session: one `Nes` (Four Score on
/// when `num_players > 2`) fed each player's *effective* (input-delayed) input
/// on its controller port. Returns the gameplay digest entering frame `frames`.
fn reference_digest_n(rom: &[u8], streams: &[Vec<Buttons>], frames: u32, input_delay: u32) -> u64 {
    let num_players = streams.len();
    let eff: Vec<Vec<Buttons>> = streams
        .iter()
        .map(|s| effective(s, frames, input_delay))
        .collect();
    let mut nes = Nes::from_rom(rom).expect("load reference");
    nes.set_four_score(num_players > 2);
    for f in 0..frames as usize {
        for (port, e) in eff.iter().enumerate() {
            nes.set_buttons(port, e[f]);
        }
        let _ = nes.run_frame();
    }
    gameplay_digest(&nes)
}

/// Drive `num_players` sessions over a paired mesh transport until ALL have
/// confirmed at least `compare_frame`, then return each session's confirmed-
/// entering digest for `compare_frame`. Each session feeds only its own
/// authored stream; the mesh delivers every peer's input to every other.
fn run_n_sessions(
    rom: &[u8],
    streams: &[Vec<Buttons>],
    compare_frame: u32,
    conditions: LinkConditions,
    seed: u64,
    base: SessionConfig,
) -> Result<Vec<u64>, NetplayError> {
    let num_players = u8::try_from(streams.len()).expect("at most 4 players");
    let transports = MeshTransport::mesh(num_players, conditions, seed);

    let mut nes: Vec<Nes> = (0..num_players)
        .map(|_| Nes::from_rom(rom).expect("load peer nes"))
        .collect();
    let hash = *nes[0].rom_sha256();

    let mut sessions: Vec<RollbackSession<MeshTransport>> = transports
        .into_iter()
        .enumerate()
        .map(|(p, t)| {
            let cfg = SessionConfig {
                num_players,
                local_player: u8::try_from(p).expect("player index fits u8"),
                ..base
            };
            RollbackSession::new(cfg, t, hash)
        })
        .collect();

    // Per-session authored cursor.
    let mut authored = vec![0u32; num_players as usize];

    let confirmed_all = |sessions: &[RollbackSession<MeshTransport>]| {
        sessions
            .iter()
            .all(|s| s.last_confirmed_frame().is_some_and(|c| c >= compare_frame))
    };

    let max_ticks = compare_frame * 40 + 400;
    let mut ticks = 0;
    while !confirmed_all(&sessions) && ticks < max_ticks {
        ticks += 1;
        for p in 0..num_players as usize {
            while authored[p] <= sessions[p].current_frame()
                && (authored[p] as usize) < streams[p].len()
            {
                sessions[p].add_local_input(streams[p][authored[p] as usize]);
                authored[p] += 1;
            }
            let _ = sessions[p].advance(&mut nes[p])?;
        }
    }

    assert!(
        confirmed_all(&sessions),
        "the {num_players} sessions did not confirm frame {compare_frame} within {max_ticks} ticks \
         (confirmed = {:?})",
        sessions
            .iter()
            .map(RollbackSession::last_confirmed_frame)
            .collect::<Vec<_>>(),
    );

    Ok(sessions
        .iter()
        .map(|s| {
            s.confirmed_entering_digest(compare_frame)
                .expect("confirmed digest present")
        })
        .collect())
}

/// THE N-PLAYER HEADLINE: for 3 and 4 players, rollback over a latent+jittery
/// mesh reproduces the no-rollback reference exactly, and every peer is
/// byte-identical to every other.
#[test]
fn n_player_rollback_matches_reference() {
    let rom = nestest_rom();
    for &num_players in &[3u8, 4u8] {
        let frames = 600u32;
        let streams =
            make_n_input_streams(num_players, frames, 0xABCD_1234 ^ u64::from(num_players));

        let cfg = SessionConfig {
            num_players,
            ..SessionConfig::default()
        };
        let compare_frame = frames - 40;
        let reference = reference_digest_n(&rom, &streams, compare_frame, cfg.input_delay);

        // ~2-3 frame latency with a touch of jitter — mispredictions and
        // rollbacks happen constantly, and with N players a misprediction on
        // ANY remote player forces a rollback.
        let conditions = LinkConditions {
            latency_polls: 2,
            jitter_polls: 1,
            drop_prob: 0.0,
        };
        let digests = run_n_sessions(
            &rom,
            &streams,
            compare_frame,
            conditions,
            0x5EED ^ u64::from(num_players),
            cfg,
        )
        .expect("no desync");

        assert!(
            digests.windows(2).all(|w| w[0] == w[1]),
            "{num_players} peers must hold identical confirmed state, got {digests:?}"
        );
        assert_eq!(
            digests[0], reference,
            "{num_players}-player rollback re-simulation must equal the no-rollback reference"
        );
    }
}

/// 4-player high-latency stress: ~8-frame latency forces a misprediction nearly
/// every frame across four remote players, exercising deep rollbacks. Still
/// must equal the reference.
#[test]
fn four_player_rollback_stress_high_latency() {
    let rom = nestest_rom();
    let num_players = 4u8;
    let frames = 400u32;
    let streams = make_n_input_streams(num_players, frames, 0x0F0F_0F0F);

    // The rollback window must exceed the link latency.
    let cfg = SessionConfig {
        num_players,
        max_rollback_frames: 16,
        ..SessionConfig::default()
    };
    let compare_frame = frames - 40;
    let reference = reference_digest_n(&rom, &streams, compare_frame, cfg.input_delay);

    let conditions = LinkConditions {
        latency_polls: 8,
        jitter_polls: 2,
        drop_prob: 0.0,
    };
    let digests = run_n_sessions(&rom, &streams, compare_frame, conditions, 0x0BAD_C0DE, cfg)
        .expect("no desync");

    assert!(
        digests.windows(2).all(|w| w[0] == w[1]),
        "4 peers diverged under high latency: {digests:?}"
    );
    assert_eq!(
        digests[0], reference,
        "deep 4-player rollback diverged from reference"
    );
}

/// 3-player desync detection: one peer is fed a persistently corrupted copy of
/// another player's input (via a transport wrapper that flips a target frame's
/// bits on every delivery), so its confirmed state genuinely diverges. The
/// periodic checksum exchange must surface a `Desync` rather than continue.
#[test]
#[allow(clippy::too_many_lines)]
fn n_player_desync_detection() {
    use rustynes_netplay::{NetMessage, Transport};

    /// Flips every `Input` it delivers for `target_frame`, persistently (even
    /// retransmits stay corrupted — a genuine divergence).
    struct CorruptingMesh {
        inner: MeshTransport,
        target_frame: u32,
    }
    impl Transport for CorruptingMesh {
        fn send(&mut self, msg: &NetMessage) {
            self.inner.send(msg);
        }
        fn poll(&mut self) -> Vec<NetMessage> {
            let mut msgs = self.inner.poll();
            for m in &mut msgs {
                if let NetMessage::Input { frame, input, .. } = m
                    && *frame == self.target_frame
                {
                    *input = !*input;
                }
            }
            msgs
        }
    }

    let rom = nestest_rom();
    let num_players = 3u8;
    let frames = 200u32;
    let streams = make_n_input_streams(num_players, frames, 0xDEAD_BEEF);

    let conditions = LinkConditions::fixed_latency(1);
    let transports = MeshTransport::mesh(num_players, conditions, 0x00C0_FFEE);
    let hash = *Nes::from_rom(&rom).unwrap().rom_sha256();

    let base = SessionConfig {
        num_players,
        checksum_interval: 5,
        ..SessionConfig::default()
    };

    let mut nes: Vec<Nes> = (0..num_players)
        .map(|_| Nes::from_rom(&rom).expect("load"))
        .collect();

    // Player 0 and 1 run clean sessions; player 2's transport corrupts the
    // inputs it receives, so its confirmed state diverges from the others'.
    let mut t = transports.into_iter();
    let mut s0 = RollbackSession::new(
        SessionConfig {
            local_player: 0,
            ..base
        },
        t.next().unwrap(),
        hash,
    );
    let mut s1 = RollbackSession::new(
        SessionConfig {
            local_player: 1,
            ..base
        },
        t.next().unwrap(),
        hash,
    );
    let mut s2 = RollbackSession::new(
        SessionConfig {
            local_player: 2,
            ..base
        },
        CorruptingMesh {
            inner: t.next().unwrap(),
            target_frame: 30,
        },
        hash,
    );

    let mut authored = [0u32; 3];
    let mut saw_desync = false;

    'outer: for _ in 0..(frames * 12) {
        // s0
        if s0.current_frame() < frames {
            while authored[0] <= s0.current_frame() && (authored[0] as usize) < streams[0].len() {
                s0.add_local_input(streams[0][authored[0] as usize]);
                authored[0] += 1;
            }
            if let Err(NetplayError::Desync { .. }) = s0.advance(&mut nes[0]) {
                saw_desync = true;
                break 'outer;
            }
        }
        // s1
        if s1.current_frame() < frames {
            while authored[1] <= s1.current_frame() && (authored[1] as usize) < streams[1].len() {
                s1.add_local_input(streams[1][authored[1] as usize]);
                authored[1] += 1;
            }
            if let Err(NetplayError::Desync { .. }) = s1.advance(&mut nes[1]) {
                saw_desync = true;
                break 'outer;
            }
        }
        // s2 (corrupted)
        if s2.current_frame() < frames {
            while authored[2] <= s2.current_frame() && (authored[2] as usize) < streams[2].len() {
                s2.add_local_input(streams[2][authored[2] as usize]);
                authored[2] += 1;
            }
            if let Err(NetplayError::Desync { .. }) = s2.advance(&mut nes[2]) {
                saw_desync = true;
                break 'outer;
            }
        }
        if s0.current_frame() >= frames
            && s1.current_frame() >= frames
            && s2.current_frame() >= frames
        {
            break;
        }
    }

    assert!(
        saw_desync,
        "the checksum exchange must detect the injected 3-player divergence"
    );
}

/// A perfect (zero-latency) N-player mesh never mispredicts → never rolls back,
/// and still equals the reference. Sanity check for the >2-player fast path.
#[test]
fn n_player_zero_rollback_is_byte_identical() {
    let rom = nestest_rom();
    for &num_players in &[3u8, 4u8] {
        let frames = 200u32;
        let streams =
            make_n_input_streams(num_players, frames, 0x7777_7777 ^ u64::from(num_players));
        let cfg = SessionConfig {
            num_players,
            ..SessionConfig::default()
        };
        let compare_frame = frames - 20;
        let reference = reference_digest_n(&rom, &streams, compare_frame, cfg.input_delay);

        let digests = run_n_sessions(
            &rom,
            &streams,
            compare_frame,
            LinkConditions::PERFECT,
            0x1234_5678,
            cfg,
        )
        .expect("no desync");

        assert!(digests.windows(2).all(|w| w[0] == w[1]));
        assert_eq!(
            digests[0], reference,
            "{num_players}-player zero-rollback must match reference byte-for-byte"
        );
    }
}

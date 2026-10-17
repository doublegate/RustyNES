#![allow(missing_docs)]
//! Criterion bench — the netplay session's per-frame costs (v3.4.0,
//! `T-NETPLAY-ROLLBACK-WINDOW`, Round 2 NET-R2-01 / -02 / -05).
//!
//! Two numbers, each measured before and after the rollback-window redesign
//! (the figures are recorded in `docs/netplay-session.md` and
//! `docs/performance.md`):
//!
//! - `gameplay_digest` — hashing one 245,760-byte framebuffer plus the cycle
//!   term, the digest every confirmed frame needs (NET-R2-05). The bench reads
//!   a real rendered frame (nestest's menu after 60 frames), not zeros, so the
//!   hash sees the data it hashes in play.
//! - `session_steady_state` — one `advance` of each of two peers over an
//!   in-memory link with a three-poll latency and *perfectly predictable*
//!   input (both players hold no buttons), so every remote input arrives late
//!   but matches its prediction. That is the steady state NET-R2-01 is about:
//!   before v3.4.0 each confirmation restored the checkpoint and replayed to
//!   the present even though nothing was mispredicted; after it, confirmation
//!   only promotes the per-frame entries it already holds.
//!
//! Run: `cargo bench -p rustynes-netplay --bench rollback_window`.

use std::hint::black_box;
use std::path::PathBuf;

use criterion::{Criterion, criterion_group, criterion_main};
use rustynes_core::{Buttons, Nes};
use rustynes_netplay::{
    LinkConditions, MemoryTransport, RollbackSession, SessionConfig, SessionIdentity,
};

fn nestest() -> Vec<u8> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let rom = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("tests/roms/nestest/nestest.nes");
    std::fs::read(&rom).unwrap_or_else(|e| panic!("read {}: {e}", rom.display()))
}

/// The pre-v3.4.0 digest, kept here as the A side of the A/B: FNV-1a 64 over
/// every framebuffer byte, one multiply per byte, XOR the cycle term.
fn digest_fnv1a_bytewise(nes: &Nes) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for &b in nes.framebuffer() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(PRIME);
    }
    hash ^ nes.cycle().wrapping_mul(0x100_0000_01b3)
}

fn bench_digest(c: &mut Criterion) {
    let rom = nestest();
    let mut nes = Nes::from_rom(&rom).expect("nestest parses");
    for _ in 0..60 {
        nes.run_frame();
    }
    c.bench_function("digest_fnv1a_bytewise", |b| {
        b.iter(|| black_box(digest_fnv1a_bytewise(black_box(&nes))));
    });
    c.bench_function("gameplay_digest", |b| {
        b.iter(|| black_box(rustynes_netplay::gameplay_digest(black_box(&nes))));
    });
}

/// Two sessions, already 120 frames into a latent session, so the measured
/// `advance` pair is the steady state rather than the start-up.
fn warmed_pair(rom: &[u8]) -> Vec<(RollbackSession<MemoryTransport>, Nes)> {
    let (a, b) = MemoryTransport::pair(LinkConditions::fixed_latency(3), 0xBE7C);
    let n0 = Nes::from_rom(rom).expect("nestest parses");
    let n1 = Nes::from_rom(rom).expect("nestest parses");
    let id = SessionIdentity::of(&n0);
    let cfg = SessionConfig::default();
    let s0 = RollbackSession::new(cfg, a, id);
    let s1 = RollbackSession::new(
        SessionConfig {
            local_player: 1,
            ..cfg
        },
        b,
        id,
    );
    let mut pair = vec![(s0, n0), (s1, n1)];
    for _ in 0..120 {
        step(&mut pair);
    }
    pair
}

fn step(pair: &mut [(RollbackSession<MemoryTransport>, Nes)]) {
    for (s, nes) in pair.iter_mut() {
        s.add_local_input(Buttons::empty());
        let _ = s.advance(nes).expect("no desync");
    }
}

fn bench_session(c: &mut Criterion) {
    let rom = nestest();
    // One pair, kept across iterations: the steady state persists, and a fresh
    // pair per iteration would put 240 warm-up frames in every sample's setup.
    let mut pair = warmed_pair(&rom);
    c.bench_function("session_steady_state", |b| {
        b.iter(|| step(black_box(&mut pair)));
    });
}

criterion_group!(benches, bench_digest, bench_session);
criterion_main!(benches);

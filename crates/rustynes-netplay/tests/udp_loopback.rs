//! Stage 2 loopback proof: the rollback session composes over a REAL UDP
//! socket.
//!
//! This mirrors the determinism harness's `rollback_matches_reference`, but
//! swaps the deterministic `MemoryTransport` for two real [`UdpTransport`]s
//! bound to `127.0.0.1:0` (ephemeral ports), each pointed at the other. Two
//! `RollbackSession`s run the same seeded nestest scenario over those sockets,
//! and we assert both peers' confirmed gameplay digest equal each other AND a
//! single no-rollback reference run. That proves the wire format
//! (`to_bytes`/`from_bytes`), the UDP routing, and the session all compose over
//! real sockets.
//!
//! # Robustness / determinism notes
//!
//! - **Ephemeral ports** (`:0`) → no fixed-port collisions on CI.
//!
//! Native-only (`std::net` UDP sockets); the whole file compiles to nothing on
//! wasm32 so the wasm `--all-targets` build stays green.
//! - **Single-threaded scheduling**: the two sessions are advanced by the SAME
//!   thread, alternating `advance` calls, so there is no inter-thread timing
//!   flake. Localhost UDP is reliable and in-order for the small per-frame
//!   traffic here, but is not strictly synchronous, so between ticks we let the
//!   loopback settle with a short, BOUNDED sleep and a bounded outer tick cap —
//!   no unbounded spin, no hang.
//! - The *seeded* input streams and the emulator's determinism contract are
//!   unchanged; only the transport is real. `std::time` is used solely for the
//!   bounded settle (host-side test scaffolding), never inside the session.
#![cfg(not(target_arch = "wasm32"))]

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::time::Duration;

use rustynes_core::{Buttons, Nes};
use rustynes_netplay::{
    NetplayError, RollbackSession, SessionConfig, SplitMix64, UdpTransport, fnv1a64,
};

fn gameplay_digest(nes: &Nes) -> u64 {
    fnv1a64(nes.framebuffer()) ^ nes.cycle().wrapping_mul(0x100_0000_01b3)
}

fn nestest_rom() -> Vec<u8> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root is two levels above the crate manifest");
    let rom = root.join("tests/roms/nestest/nestest.nes");
    std::fs::read(&rom).unwrap_or_else(|e| panic!("read nestest rom {}: {e}", rom.display()))
}

fn make_input_streams(frames: u32, seed: u64) -> (Vec<Buttons>, Vec<Buttons>) {
    let mut r0 = SplitMix64::new(seed ^ 0x1111_1111);
    let mut r1 = SplitMix64::new(seed ^ 0x2222_2222);
    let mut p0 = Vec::with_capacity(frames as usize);
    let mut p1 = Vec::with_capacity(frames as usize);
    for _ in 0..frames {
        p0.push(Buttons::from_bits_truncate(r0.next_u8()));
        p1.push(Buttons::from_bits_truncate(r1.next_u8()));
    }
    (p0, p1)
}

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

/// Load a committed ROM by its path relative to the workspace root.
fn rom_at(rel: &str) -> Vec<u8> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().and_then(|p| p.parent()).expect("root");
    std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// The closest single-threaded reproduction of the native two-instance desync:
/// real `UdpTransport`s (not the in-memory model) driven ASYMMETRICALLY (each
/// session advances an independent 0-2 times per tick, so they drift like two
/// real processes), idle input, on sprite/APU-heavy ROMs. asymmetric+memory and
/// symmetric+UDP both already pass — this is the untested combination.
#[test]
fn udp_asymmetric_idle_drive_stays_in_sync_sprite_heavy() {
    for rel in [
        "tests/roms/sprint-2/oam_stress.nes",
        "tests/roms/accuracycoin/AccuracyCoin.nes",
    ] {
        let rom = rom_at(rel);
        let target = 500u32;
        let (t0, t1) = udp_pair();
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
        let mut rng = SplitMix64::new(0xA53C_1234);
        let confirmed = |s: &RollbackSession<UdpTransport>| {
            s.last_confirmed_frame().is_some_and(|c| c >= target)
        };
        let (mut a0, mut a1) = (0u32, 0u32);
        let mut ticks = 0u32;
        let max_ticks = target * 90 + 5000;
        while !(confirmed(&s0) && confirmed(&s1)) && ticks < max_ticks {
            ticks += 1;
            for _ in 0..(rng.next_u8() % 3) {
                while a0 <= s0.current_frame() {
                    s0.add_local_input(Buttons::empty());
                    a0 += 1;
                }
                s0.advance(&mut nes0).expect("s0 advance");
            }
            for _ in 0..(rng.next_u8() % 3) {
                while a1 <= s1.current_frame() {
                    s1.add_local_input(Buttons::empty());
                    a1 += 1;
                }
                s1.advance(&mut nes1).expect("s1 advance");
            }
            std::thread::sleep(Duration::from_micros(100));
        }
        assert!(confirmed(&s0) && confirmed(&s1), "{rel}: did not confirm");
        let d0 = s0.confirmed_entering_digest(target).expect("s0 digest");
        let d1 = s1.confirmed_entering_digest(target).expect("s1 digest");
        assert_eq!(d0, d1, "{rel}: real-UDP asymmetric idle drive desynced");
    }
}

/// Build a connected pair of loopback `UdpTransport`s on ephemeral ports.
fn udp_pair() -> (UdpTransport, UdpTransport) {
    let local = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let sa = UdpSocket::bind(local).expect("bind a");
    let sb = UdpSocket::bind(local).expect("bind b");
    let addr_a = sa.local_addr().unwrap();
    let addr_b = sb.local_addr().unwrap();
    let a = UdpTransport::from_socket(sa, addr_b).expect("transport a");
    let b = UdpTransport::from_socket(sb, addr_a).expect("transport b");
    (a, b)
}

/// Drive both sessions over the real UDP pair until both confirm at least
/// `compare_frame`, then return each peer's confirmed-entering digest.
fn run_two_sessions_over_udp(
    rom: &[u8],
    p0: &[Buttons],
    p1: &[Buttons],
    compare_frame: u32,
    base: SessionConfig,
) -> Result<(u64, u64), NetplayError> {
    let (t0, t1) = udp_pair();
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

    let confirmed_at = |s: &RollbackSession<UdpTransport>| {
        s.last_confirmed_frame().is_some_and(|c| c >= compare_frame)
    };

    // Generous bound: localhost is reliable but a tick may occasionally produce
    // no confirmation while waiting on an in-flight datagram, so we allow many
    // ticks per target frame.
    let max_ticks = compare_frame * 60 + 2000;
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

        // Let the loopback deliver this tick's datagrams before the next
        // advance polls. Bounded and tiny; keeps the single-threaded schedule
        // flake-free without busy-spinning.
        std::thread::sleep(Duration::from_micros(200));
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

/// THE STAGE-2 HEADLINE: the rollback session reproduces the no-rollback
/// reference exactly when run over real UDP sockets on localhost, and the two
/// peers are byte-identical to each other.
#[test]
fn rollback_over_udp_matches_reference() {
    let rom = nestest_rom();
    let frames = 300u32;
    let (p0, p1) = make_input_streams(frames, 0xABCD_1234);

    let cfg = SessionConfig::default();
    let compare_frame = frames - 40;
    let reference = reference_digest(&rom, &p0, &p1, compare_frame, cfg.input_delay);

    let (snap0, snap1) =
        run_two_sessions_over_udp(&rom, &p0, &p1, compare_frame, cfg).expect("no desync");

    assert_eq!(
        snap0, snap1,
        "the two UDP peers must hold identical confirmed state"
    );
    assert_eq!(
        snap0, reference,
        "rollback over real UDP must equal the no-rollback reference"
    );
}

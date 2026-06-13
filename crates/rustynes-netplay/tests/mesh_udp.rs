//! v2.6.0 N-peer UDP proof: a full 3-/4-player rollback session composes over
//! REAL UDP sockets, after the multi-joiner **roster handshake**.
//!
//! This is the N-peer analogue of `udp_loopback.rs` (which proved the 2-player
//! session over real UDP) and of the in-memory `n_player_rollback_matches_reference`
//! determinism test (which proved the N-player session over an in-memory mesh).
//! Here we put the two together: a [`MeshHost`] + 2-3 [`MeshJoiner`]s complete
//! the roster handshake on `127.0.0.1` ephemeral ports, then each peer runs a
//! [`RollbackSession`] over its real [`UdpMeshTransport`], and we assert every
//! peer's confirmed gameplay digest equals each other AND a single no-rollback
//! reference run fed the same combined N-port input sequence (Four Score on for
//! >2 players).
//!
//! # Robustness / determinism notes
//!
//! - **Ephemeral ports** (`:0`) → no fixed-port collisions on CI.
//! - **Single-threaded scheduling**: all peers are advanced by the SAME thread,
//!   in round-robin, so there is no inter-thread timing flake. Localhost UDP is
//!   reliable + in-order for the small per-frame traffic here but not strictly
//!   synchronous, so between ticks we let the loopback settle with a short,
//!   BOUNDED sleep and a bounded outer tick cap — no unbounded spin, no hang.
//! - The *seeded* input streams and the emulator's determinism contract are
//!   unchanged; only the transport is real. `std::time` is used solely for the
//!   bounded settle (host-side test scaffolding), never inside the session.
//!
//! Native-only (`std::net` UDP sockets); the whole file compiles to nothing on
//! wasm32 so the wasm `--all-targets` build stays green.
#![cfg(not(target_arch = "wasm32"))]

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::time::Duration;

use rustynes_core::{Buttons, Nes};
use rustynes_netplay::{
    fnv1a64, MeshHost, MeshJoiner, NetplayError, RollbackSession, SessionConfig, SplitMix64,
    UdpMeshTransport,
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

fn make_n_input_streams(num_players: u8, frames: u32, seed: u64) -> Vec<Vec<Buttons>> {
    (0..num_players)
        .map(|p| {
            let mut r = SplitMix64::new(seed ^ (0x1111_1111_u64.wrapping_mul(u64::from(p) + 1)));
            (0..frames)
                .map(|_| Buttons::from_bits_truncate(r.next_u8()))
                .collect()
        })
        .collect()
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

/// One `Nes` (Four Score on for >2 players) fed each player's *effective* input
/// on its controller port, no rollback. The ground-truth digest entering
/// `frames`.
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

fn loopback() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
}

/// Run the multi-joiner roster handshake on loopback, returning the per-player
/// mesh transports in player order (index 0 = host). Bounded; never hangs.
fn run_roster_handshake(num_players: u8, rom_hash: [u8; 32]) -> Vec<UdpMeshTransport> {
    // Probe a free port so the host's listening addr == its gameplay addr (true
    // on loopback; an internet deployment substitutes the STUN public addr).
    let probe = UdpSocket::bind(loopback()).unwrap();
    let port = probe.local_addr().unwrap();
    drop(probe);

    let mut host = MeshHost::bind(port, port, num_players, rom_hash).expect("bind host");
    let host_listen = host.local_addr().unwrap();

    let mut joiners: Vec<MeshJoiner> = (1..num_players)
        .map(|p| MeshJoiner::connect(loopback(), host_listen, p, rom_hash).expect("connect joiner"))
        .collect();

    let mut host_out: Option<UdpMeshTransport> = None;
    let mut joiner_out: Vec<Option<UdpMeshTransport>> = (0..joiners.len()).map(|_| None).collect();

    for _ in 0..4000 {
        if host_out.is_none() {
            host_out = host.pump().expect("host handshake");
        }
        for (i, j) in joiners.iter_mut().enumerate() {
            if joiner_out[i].is_none() {
                joiner_out[i] = j.pump().expect("joiner handshake");
            }
        }
        if host_out.is_some() && joiner_out.iter().all(Option::is_some) {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    let host_t = host_out.expect("host produced a mesh transport");
    let mut all = Vec::with_capacity(num_players as usize);
    all.push(host_t);
    for j in joiner_out {
        all.push(j.expect("joiner produced a mesh transport"));
    }
    all
}

/// Drive `num_players` sessions over the real UDP mesh until ALL confirm at
/// least `compare_frame`, then return each session's confirmed-entering digest.
fn run_n_sessions_over_udp(
    rom: &[u8],
    streams: &[Vec<Buttons>],
    compare_frame: u32,
    base: SessionConfig,
) -> Result<Vec<u64>, NetplayError> {
    let num_players = u8::try_from(streams.len()).expect("at most 4 players");
    let mut nes: Vec<Nes> = (0..num_players)
        .map(|_| Nes::from_rom(rom).expect("load peer nes"))
        .collect();
    let hash = *nes[0].rom_sha256();

    let transports = run_roster_handshake(num_players, hash);

    let mut sessions: Vec<RollbackSession<UdpMeshTransport>> = transports
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

    let mut authored = vec![0u32; num_players as usize];

    let confirmed_all = |sessions: &[RollbackSession<UdpMeshTransport>]| {
        sessions
            .iter()
            .all(|s| s.last_confirmed_frame().is_some_and(|c| c >= compare_frame))
    };

    let max_ticks = compare_frame * 60 + 4000;
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
        // Let the loopback deliver this round's datagrams before the next poll.
        std::thread::sleep(Duration::from_micros(200));
    }

    assert!(
        confirmed_all(&sessions),
        "the {num_players} UDP sessions did not confirm frame {compare_frame} within {max_ticks} ticks \
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

/// THE N-PEER UDP HEADLINE: for 3 and 4 players, the roster handshake stands up
/// the full mesh over real UDP sockets, and the rollback re-simulation
/// reproduces the no-rollback reference exactly — every peer byte-identical.
#[test]
fn n_player_rollback_over_udp_matches_reference() {
    let rom = nestest_rom();
    for &num_players in &[3u8, 4u8] {
        // 120 frames is enough to exercise many confirmations + the periodic
        // checksum exchange over real sockets while keeping the loopback test
        // brisk.
        let frames = 120u32;
        let streams =
            make_n_input_streams(num_players, frames, 0xABCD_1234 ^ u64::from(num_players));

        let cfg = SessionConfig {
            num_players,
            ..SessionConfig::default()
        };
        let compare_frame = frames - 30;
        let reference = reference_digest_n(&rom, &streams, compare_frame, cfg.input_delay);

        let digests = run_n_sessions_over_udp(&rom, &streams, compare_frame, cfg)
            .expect("no desync over UDP");

        assert!(
            digests.windows(2).all(|w| w[0] == w[1]),
            "{num_players} UDP peers must hold identical confirmed state, got {digests:?}"
        );
        assert_eq!(
            digests[0], reference,
            "{num_players}-player rollback over real UDP must equal the no-rollback reference"
        );
    }
}

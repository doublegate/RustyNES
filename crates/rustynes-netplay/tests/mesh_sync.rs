//! v3.4.0 `T-NETPLAY-MESH-NATIVE` (plan row 5): three and four players stay
//! in sync, frame by frame.
//!
//! Earlier proofs (`determinism.rs`'s `n_player_rollback_matches_reference`,
//! `mesh_udp.rs`) compared ONE confirmed frame against the reference. These
//! gates compare EVERY confirmed frame: each peer's confirmed entering digest
//! of frame `f`, for every `f` below `N` (600), must equal every other peer's
//! and the no-rollback reference's, under random input and a link that delays,
//! reorders (jitter) and drops. The ROM is [`four_score_probe_rom`], whose
//! picture depends on all four controllers; nestest reads only the first.
//!
//! - [`four_peers_stay_in_sync_every_frame_in_memory`] and
//!   [`three_peers_stay_in_sync_every_frame_in_memory`]: the in-memory mesh,
//!   whose latency, jitter and loss come from a seeded PRNG, so a failure
//!   reproduces exactly.
//! - [`four_peers_stay_in_sync_every_frame_over_udp`]: the real thing, four
//!   peers on `127.0.0.1` through the sealed roster handshake (`MeshHost`,
//!   `MeshJoiner`, protocol 9), with loss, delay and reordering injected on
//!   top of the sockets by [`Lossy`].
#![cfg(not(target_arch = "wasm32"))]

use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::time::Duration;

use rustynes_core::{Buttons, Nes};
use rustynes_netplay::{
    LinkConditions, MeshHost, MeshJoiner, MeshTransport, NetMessage, RollbackSession,
    SessionConfig, SessionIdentity, SplitMix64, Transport, UdpMeshTransport, gameplay_digest,
};

/// Frames compared on every peer.
const FRAMES: u32 = 600;

fn nestest_rom() -> Vec<u8> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let rom = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("tests/roms/nestest/nestest.nes");
    std::fs::read(&rom).unwrap_or_else(|e| panic!("read {}: {e}", rom.display()))
}

/// v3.4.0 — a ROM whose picture depends on ALL FOUR controllers, built here
/// (public-domain by construction: 95 bytes of 6502 written for this test).
///
/// nestest, the other fixture, reads only the first controller, so a session
/// that dropped players 3 and 4 entirely (the Four Score left off, say) still
/// matched its reference; mutation testing showed exactly that. This program
/// waits for vblank each frame, strobes the controllers, reads 16 bits from
/// `$4016` (player 1 then player 3, through the Four Score) and 16 from `$4017`
/// (players 2 and 4), mixes the four bytes with shifts and XORs, and writes
/// the result to the backdrop colour `$3F00` with rendering off, so the whole
/// frame is that colour. Every port's buttons reach the framebuffer, and so
/// the gameplay digest.
fn four_score_probe_rom() -> Vec<u8> {
    const PROGRAM: [u8; 95] = [
        0x78, 0xA2, 0xFF, 0x9A, 0xA9, 0x00, 0x8D, 0x00, 0x20, 0x8D, 0x01, 0x20, 0x2C, 0x02, 0x20,
        0x10, 0xFB, 0xA9, 0x01, 0x8D, 0x16, 0x40, 0xA9, 0x00, 0x8D, 0x16, 0x40, 0xA2, 0x00, 0xAD,
        0x16, 0x40, 0x4A, 0x26, 0x00, 0x26, 0x01, 0xE8, 0xE0, 0x10, 0xD0, 0xF3, 0xA2, 0x00, 0xAD,
        0x17, 0x40, 0x4A, 0x26, 0x02, 0x26, 0x03, 0xE8, 0xE0, 0x10, 0xD0, 0xF3, 0xA5, 0x00, 0x0A,
        0x45, 0x01, 0x0A, 0x45, 0x02, 0x0A, 0x45, 0x03, 0x29, 0x3F, 0xAA, 0xA9, 0x3F, 0x8D, 0x06,
        0x20, 0xA9, 0x00, 0x8D, 0x06, 0x20, 0x8E, 0x07, 0x20, 0xA9, 0x00, 0x8D, 0x06, 0x20, 0x8D,
        0x06, 0x20, 0x4C, 0x0C, 0xC0,
    ];
    let mut rom = b"NES\x1A\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
    let mut prg = vec![0xEAu8; 16 * 1024];
    prg[..PROGRAM.len()].copy_from_slice(&PROGRAM);
    // NMI, RESET and IRQ all point at $C000 (the 16 KiB bank sits there).
    for v in [0x3FFA, 0x3FFC, 0x3FFE] {
        prg[v] = 0x00;
        prg[v + 1] = 0xC0;
    }
    rom.extend_from_slice(&prg);
    rom.extend_from_slice(&[0u8; 8 * 1024]);
    rom
}

/// The fixture does what it says: a button on any one of the four ports,
/// with the Four Score on, changes the picture.
#[test]
fn the_probe_rom_sees_every_port() {
    let rom = four_score_probe_rom();
    let run_with = |four_score: bool, port: Option<usize>| {
        let mut nes = Nes::from_rom(&rom).expect("probe parses");
        nes.set_four_score(four_score);
        for _ in 0..4 {
            if let Some(p) = port {
                nes.set_buttons(p, Buttons::from_bits_truncate(0x81));
            }
            let _ = nes.run_frame();
        }
        gameplay_digest(&nes)
    };
    let idle = run_with(true, None);
    for p in 0..4 {
        assert_ne!(
            run_with(true, Some(p)),
            idle,
            "port {p} must reach the picture"
        );
    }
    // Without the Four Score, players 3 and 4 are not read at all: a session
    // that lost the adapter would show it.
    assert_eq!(run_with(false, Some(2)), run_with(false, None));
}

fn streams(players: u8, frames: u32, seed: u64) -> Vec<Vec<Buttons>> {
    (0..players)
        .map(|p| {
            let mut r = SplitMix64::new(seed ^ (u64::from(p) + 1).wrapping_mul(0x9E37_79B9));
            (0..frames)
                .map(|_| Buttons::from_bits_truncate(r.next_u8()))
                .collect()
        })
        .collect()
}

/// The entering digest of every frame `0..frames` of one machine fed every
/// player's input-delayed stream on its port (Four Score on for > 2 players).
fn reference(rom: &[u8], streams: &[Vec<Buttons>], frames: u32, delay: u32) -> Vec<u64> {
    let mut nes = Nes::from_rom(rom).expect("rom parses");
    nes.set_four_score(streams.len() > 2);
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

/// Drive one session per peer until every peer has confirmed `FRAMES`
/// frames, reading each peer's confirmed digests as they confirm; then assert
/// every frame equal on every peer and to the reference. `settle` runs between
/// rounds (the UDP test lets the loopback deliver).
fn run_and_compare<T: Transport>(
    rom: &[u8],
    transports: Vec<T>,
    seed: u64,
    cfg: SessionConfig,
    mut settle: impl FnMut(),
) {
    let players = u8::try_from(transports.len()).expect("at most four");
    let input = streams(players, FRAMES + 60, seed);
    let mut nes: Vec<Nes> = (0..players)
        .map(|_| Nes::from_rom(rom).expect("rom parses"))
        .collect();
    let id = SessionIdentity::of(&nes[0]);
    let mut sessions: Vec<RollbackSession<T>> = transports
        .into_iter()
        .zip(0u8..)
        .map(|(t, p)| {
            RollbackSession::new(
                SessionConfig {
                    num_players: players,
                    local_player: p,
                    ..cfg
                },
                t,
                id,
            )
        })
        .collect();
    let mut digests: Vec<Vec<u64>> = vec![Vec::new(); players as usize];
    let mut authored = vec![0u32; players as usize];
    let mut rollbacks = 0u32;
    let mut ticks = 0u32;
    let want = FRAMES as usize;
    while digests.iter().any(|d| d.len() < want) && ticks < FRAMES * 60 {
        ticks += 1;
        for (p, s) in sessions.iter_mut().enumerate() {
            while authored[p] <= s.current_frame() && (authored[p] as usize) < input[p].len() {
                s.add_local_input(input[p][authored[p] as usize]);
                authored[p] += 1;
            }
            rollbacks += u32::from(s.advance(&mut nes[p]).expect("no desync").rolled_back);
            let d = &mut digests[p];
            while let Some(x) =
                s.confirmed_entering_digest(u32::try_from(d.len()).unwrap_or(u32::MAX))
            {
                d.push(x);
            }
        }
        settle();
    }
    assert!(
        digests.iter().all(|d| d.len() >= want),
        "every peer confirmed {FRAMES} frames within {ticks} ticks: {:?}",
        digests.iter().map(Vec::len).collect::<Vec<_>>()
    );
    assert!(
        rollbacks > 0,
        "the link must force rollbacks for the gate to bite"
    );
    let truth = reference(rom, &input, FRAMES, cfg.input_delay);
    for (f, want) in truth.iter().enumerate() {
        for (p, d) in digests.iter().enumerate() {
            assert_eq!(
                d[f], *want,
                "{players} peers: peer {p} diverged at frame {f} (reference {want:#018x})"
            );
        }
    }
}

/// Latency, reordering (jitter larger than the latency step) and loss.
const HOSTILE: LinkConditions = LinkConditions {
    latency_polls: 2,
    jitter_polls: 3,
    drop_prob: 0.05,
};

#[test]
fn four_peers_stay_in_sync_every_frame_in_memory() {
    let rom = four_score_probe_rom();
    let cfg = SessionConfig {
        max_rollback_frames: 12,
        ..SessionConfig::default()
    };
    run_and_compare(
        &rom,
        MeshTransport::mesh(4, HOSTILE, 0x4444),
        0xF0F0,
        cfg,
        || {},
    );
}

#[test]
fn three_peers_stay_in_sync_every_frame_in_memory() {
    let rom = four_score_probe_rom();
    let cfg = SessionConfig {
        max_rollback_frames: 12,
        ..SessionConfig::default()
    };
    run_and_compare(
        &rom,
        MeshTransport::mesh(3, HOSTILE, 0x3333),
        0x0F0F,
        cfg,
        || {},
    );
}

/// A transport wrapper that loses, delays and reorders what the wrapped
/// transport receives, from a seeded PRNG: the injected faults are
/// reproducible even though the sockets under them are real.
struct Lossy<T> {
    inner: T,
    rng: SplitMix64,
    /// Messages held back, each with the polls left before delivery.
    held: VecDeque<(u32, NetMessage)>,
}

impl<T: Transport> Lossy<T> {
    const fn new(inner: T, seed: u64) -> Self {
        Self {
            inner,
            rng: SplitMix64::new(seed),
            held: VecDeque::new(),
        }
    }
}

impl<T: Transport> Transport for Lossy<T> {
    fn send(&mut self, msg: &NetMessage) {
        self.inner.send(msg);
    }

    fn poll(&mut self) -> Vec<NetMessage> {
        for msg in self.inner.poll() {
            // 5% lost; the rest held 0 to 4 polls, so later ones overtake.
            if self.rng.next_unit() < 0.05 {
                continue;
            }
            let delay = self.rng.next_below(5);
            self.held.push_back((delay, msg));
        }
        let mut out = Vec::new();
        let mut keep = VecDeque::with_capacity(self.held.len());
        while let Some((d, m)) = self.held.pop_front() {
            if d == 0 {
                out.push(m);
            } else {
                keep.push_back((d - 1, m));
            }
        }
        self.held = keep;
        out
    }
}

/// The four-player roster handshake on loopback; transports in player order.
fn udp_mesh(players: u8, id: SessionIdentity) -> Vec<UdpMeshTransport> {
    let any = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let probe = UdpSocket::bind(any).unwrap();
    let port = probe.local_addr().unwrap();
    drop(probe);
    let mut host = MeshHost::bind(port, port, players, id).expect("bind host");
    let listen = host.local_addr().unwrap();
    // Every joiner is told it is player 1: the roster's key shares, not the
    // hint, must give each its own index.
    let mut joiners: Vec<MeshJoiner> = (1..players)
        .map(|_| MeshJoiner::connect(any, listen, 1, id).expect("connect"))
        .collect();
    let mut out: Vec<Option<UdpMeshTransport>> = (0..players).map(|_| None).collect();
    for _ in 0..4000 {
        if out[0].is_none() {
            out[0] = host.pump().expect("host handshake");
        }
        for (i, j) in joiners.iter_mut().enumerate() {
            if out[i + 1].is_none() {
                out[i + 1] = j.pump().expect("joiner handshake");
            }
        }
        if out.iter().all(Option::is_some) {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    // Put each joiner's transport at the index its roster entry gave it.
    let mut by_player: Vec<Option<UdpMeshTransport>> = (0..players).map(|_| None).collect();
    by_player[0] = out[0].take();
    for (i, j) in joiners.iter().enumerate() {
        let p = usize::from(j.my_player());
        assert!(by_player[p].is_none(), "two joiners were given player {p}");
        assert_eq!(j.num_players(), players);
        by_player[p] = out[i + 1].take();
    }
    by_player
        .into_iter()
        .map(|t| t.expect("every player has a transport"))
        .collect()
}

#[test]
fn four_peers_stay_in_sync_every_frame_over_udp() {
    let rom = four_score_probe_rom();
    let id = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
    let transports: Vec<Lossy<UdpMeshTransport>> = udp_mesh(4, id)
        .into_iter()
        .zip(0u64..)
        .map(|(t, i)| {
            assert!(t.is_sealed(), "protocol 9 seals the mesh");
            Lossy::new(t, 0xD00D ^ i)
        })
        .collect();
    let cfg = SessionConfig {
        max_rollback_frames: 12,
        ..SessionConfig::default()
    };
    run_and_compare(&rom, transports, 0x1234, cfg, || {
        std::thread::sleep(Duration::from_micros(200));
    });
}

/// v3.4.0 — an input one peer lost must be resent even when ANOTHER peer has
/// acknowledged it.
///
/// Found by the desktop 3-player UI test: the mesh host hands its socket to
/// the session some 200 ms after the joiners start theirs (it finishes its
/// roster resends first), and everything the joiners sent meanwhile is
/// drained by the handshake. Until v3.4.0 `InputAck` named no sender and the
/// session kept the highest ack from any peer, so joiner 2's ack stopped
/// joiner 1 resending what the host never received, and the host stalled at
/// frame 9 with nothing confirmed, forever. Here the host's session starts
/// 40 ticks late and everything delivered to it before then is discarded.
#[test]
fn a_peer_that_lost_early_inputs_still_gets_them() {
    let rom = nestest_rom();
    let id = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
    let mut transports = MeshTransport::mesh(3, LinkConditions::PERFECT, 0x1A7E);
    let host_t = transports.remove(0);
    let mut nes: Vec<Nes> = (0..3).map(|_| Nes::from_rom(&rom).unwrap()).collect();
    let mut joiners: Vec<RollbackSession<MeshTransport>> = transports
        .into_iter()
        .zip(1u8..)
        .map(|(t, p)| {
            RollbackSession::new(
                SessionConfig {
                    num_players: 3,
                    local_player: p,
                    ..SessionConfig::default()
                },
                t,
                id,
            )
        })
        .collect();
    let mut host_t = Some(host_t);
    let mut host: Option<RollbackSession<MeshTransport>> = None;
    for tick in 0..400 {
        // The handshake drains the host's socket, up to and including the
        // tick its session starts: nothing the joiners sent reaches it.
        if tick <= 40 {
            let _ = host_t.as_mut().map(Transport::poll);
        }
        if tick == 40 {
            host = host_t.take().map(|t| {
                RollbackSession::new(
                    SessionConfig {
                        num_players: 3,
                        local_player: 0,
                        ..SessionConfig::default()
                    },
                    t,
                    id,
                )
            });
        }
        if let Some(h) = host.as_mut() {
            h.add_local_input(Buttons::empty());
            h.advance(&mut nes[0]).expect("no desync");
        }
        for (j, n) in joiners.iter_mut().zip(nes.iter_mut().skip(1)) {
            j.add_local_input(Buttons::empty());
            j.advance(n).expect("no desync");
        }
    }
    let confirmed = host
        .as_ref()
        .and_then(RollbackSession::last_confirmed_frame);
    assert!(
        confirmed.is_some_and(|c| c > 200),
        "the late host must receive the joiners' early inputs (confirmed {confirmed:?})"
    );
}

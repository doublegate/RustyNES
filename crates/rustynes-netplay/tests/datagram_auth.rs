//! v3.4.0 `T-NETPLAY-DATAGRAM-AUTH` (protocol 9, ADR 0046): the gates on the
//! per-session datagram key, over real loopback UDP.
//!
//! Each test plays one side of the handshake by hand from a raw socket, so it
//! holds the same key the transport derived and can seal, replay, forge and
//! move address at will:
//!
//! - [`a_forged_datagram_from_a_third_address_is_refused`]: a third socket that
//!   can reach the port, sending messages that would end a v3.3.0 session (a
//!   wrong `Checksum`, a `Sync` for another game), unsealed or sealed under a
//!   guessed key, gets nothing through; the genuine peer still does.
//! - [`a_replayed_datagram_is_refused`]: the same sealed datagram twice, from
//!   the peer or from anyone else, is accepted once.
//! - [`a_rebinding_peer_is_kept`]: the peer's datagrams start arriving from a
//!   new address (a NAT rebinding): they are accepted, and the transport's
//!   own datagrams follow the peer to it.
//! - [`a_forged_checksum_cannot_end_a_session`]: the same at session level, two
//!   `RollbackSession`s over sealed transports, a forger sending a desync and a
//!   ROM mismatch every tick; the session runs on.
//! - [`a_protocol_8_peer_is_refused_with_a_reason`]: the protocol bump.
//! - [`a_mesh_peer_refuses_a_forgery`] and [`a_mesh_peer_follows_a_rebinding_peer`]:
//!   the 3-peer mesh, sealed the same way.
#![cfg(not(target_arch = "wasm32"))]

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

use rustynes_core::{Buttons, Nes};
use rustynes_netplay::{
    ConnectionState, DatagramAuth, DisconnectReason, KEY_SHARE_LEN, MeshHost, MeshJoiner,
    NetMessage, NetplayConnection, RollbackSession, SessionConfig, SessionIdentity, Transport,
    UdpMeshTransport, UdpTransport, fresh_key_share,
};

fn loopback() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
}

const fn identity() -> SessionIdentity {
    SessionIdentity::new([0x5A; 32], [0xA5; 32])
}

/// A peer played by hand: a raw socket and the key the handshake derived.
struct HandPeer {
    socket: UdpSocket,
    auth: DatagramAuth,
}

impl HandPeer {
    fn send(&mut self, msg: &NetMessage, to: SocketAddr) -> Vec<u8> {
        let sealed = self.auth.seal(&msg.to_bytes());
        self.socket.send_to(&sealed, to).unwrap();
        sealed
    }
}

/// A listening host `NetplayConnection` and a hand-played joiner, through the
/// protocol 9 handshake. Returns the host's sealed transport, the joiner, and
/// the host's address.
fn handshake() -> (UdpTransport, HandPeer, SocketAddr) {
    let id = identity();
    let mut host = NetplayConnection::host(loopback(), id).unwrap();
    let host_addr = host.transport().local_addr().unwrap();
    let socket = UdpSocket::bind(loopback()).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_millis(5)))
        .unwrap();
    let ours = fresh_key_share();
    let sync = NetMessage::Sync {
        magic: NetMessage::SYNC_MAGIC,
        identity: id,
        key_share: ours,
    }
    .to_bytes();
    let mut theirs = None;
    let mut buf = [0u8; 1500];
    for _ in 0..400 {
        if theirs.is_none() {
            socket.send_to(&sync, host_addr).unwrap();
        }
        host.pump(0);
        if let Ok((n, _)) = socket.recv_from(&mut buf)
            && let Some(NetMessage::Sync { key_share, .. }) = NetMessage::from_bytes(&buf[..n])
            && key_share != [0; KEY_SHARE_LEN]
        {
            theirs = Some(key_share);
        }
        if theirs.is_some() && host.state() == ConnectionState::Synced {
            break;
        }
    }
    assert_eq!(host.state(), ConnectionState::Synced, "the host synced");
    let theirs = theirs.expect("the host answered with its share");
    let auth = DatagramAuth::pair(&id, ours, theirs).expect("a pair key");
    let transport = host.into_transport();
    assert!(
        transport.is_sealed(),
        "a synced connection's transport is sealed"
    );
    (transport, HandPeer { socket, auth }, host_addr)
}

/// Poll `t` a few times, collecting everything it surfaces.
fn drain<T: Transport>(t: &mut T) -> Vec<NetMessage> {
    let mut got = Vec::new();
    for _ in 0..40 {
        got.extend(t.poll());
        std::thread::sleep(Duration::from_millis(1));
    }
    got
}

const fn checksum(frame: u32) -> NetMessage {
    NetMessage::Checksum {
        frame,
        hash: 0,
        fb_hash: 0,
    }
}

#[test]
fn a_forged_datagram_from_a_third_address_is_refused() {
    let (mut host, mut peer, host_addr) = handshake();
    let forger = UdpSocket::bind(loopback()).unwrap();
    // Unsealed: what any third party could send to a v3.3.0 session.
    forger.send_to(&checksum(30).to_bytes(), host_addr).unwrap();
    let other_game = NetMessage::Sync {
        magic: NetMessage::SYNC_MAGIC,
        identity: SessionIdentity::new([1; 32], [2; 32]),
        key_share: [9; KEY_SHARE_LEN],
    };
    forger.send_to(&other_game.to_bytes(), host_addr).unwrap();
    // Sealed, but under a key the forger guessed (its own shares).
    let mut guess = DatagramAuth::pair(&identity(), fresh_key_share(), fresh_key_share()).unwrap();
    forger
        .send_to(&guess.seal(&checksum(60).to_bytes()), host_addr)
        .unwrap();
    // The genuine peer's datagram, last.
    peer.send(
        &NetMessage::InputAck {
            player: 1,
            frame: 7,
        },
        host_addr,
    );

    let got = drain(&mut host);
    assert_eq!(
        got,
        vec![NetMessage::InputAck {
            player: 1,
            frame: 7
        }],
        "only the genuine peer's datagram may reach the session"
    );
    assert!(
        host.refused_datagrams() >= 3,
        "the three forgeries were refused (refused {})",
        host.refused_datagrams()
    );
}

#[test]
fn a_replayed_datagram_is_refused() {
    let (mut host, mut peer, host_addr) = handshake();
    let sealed = peer.send(
        &NetMessage::InputAck {
            player: 1,
            frame: 11,
        },
        host_addr,
    );
    // The peer's own retransmission of the same bytes, and a third party
    // that recorded them, replaying from its own address.
    peer.socket.send_to(&sealed, host_addr).unwrap();
    let recorder = UdpSocket::bind(loopback()).unwrap();
    recorder.send_to(&sealed, host_addr).unwrap();
    let got = drain(&mut host);
    assert_eq!(
        got,
        vec![NetMessage::InputAck {
            player: 1,
            frame: 11
        }],
        "accepted once"
    );
    assert_eq!(host.refused_datagrams(), 2, "both replays refused");
    // A NEW datagram from the peer still opens: the refusals changed nothing.
    peer.send(
        &NetMessage::InputAck {
            player: 1,
            frame: 12,
        },
        host_addr,
    );
    assert_eq!(
        drain(&mut host),
        vec![NetMessage::InputAck {
            player: 1,
            frame: 12
        }]
    );
}

#[test]
fn a_rebinding_peer_is_kept() {
    let (mut host, mut peer, host_addr) = handshake();
    peer.send(
        &NetMessage::InputAck {
            player: 1,
            frame: 1,
        },
        host_addr,
    );
    assert_eq!(
        drain(&mut host),
        vec![NetMessage::InputAck {
            player: 1,
            frame: 1
        }]
    );
    let old_addr = peer.socket.local_addr().unwrap();
    assert_eq!(host.remote_addr(), Some(old_addr));

    // The NAT gives the peer a new public port: same key, new source.
    let rebound = UdpSocket::bind(loopback()).unwrap();
    rebound
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    let new_addr = rebound.local_addr().unwrap();
    let sealed = peer.auth.seal(
        &NetMessage::InputAck {
            player: 1,
            frame: 2,
        }
        .to_bytes(),
    );
    rebound.send_to(&sealed, host_addr).unwrap();
    assert_eq!(
        drain(&mut host),
        vec![NetMessage::InputAck {
            player: 1,
            frame: 2
        }],
        "the rebinding peer's datagram is accepted"
    );
    assert_eq!(
        host.remote_addr(),
        Some(new_addr),
        "the transport follows it"
    );
    assert_eq!(host.roamed(), 1);

    // And the host's own datagrams now reach the peer at the new address.
    host.send(&NetMessage::InputAck {
        player: 1,
        frame: 3,
    });
    let mut buf = [0u8; 1500];
    let (n, from) = rebound
        .recv_from(&mut buf)
        .expect("the host follows the peer");
    assert_eq!(from, host_addr);
    let (_, payload) = peer.auth.open(&buf[..n]).expect("sealed by the host");
    assert_eq!(
        NetMessage::from_bytes(payload),
        Some(NetMessage::InputAck {
            player: 1,
            frame: 3
        })
    );
}

/// The session-level form: two real sessions, sealed, and a forger sending
/// the two datagrams that ended a v3.3.0 session (a mismatched checksum for
/// every checksum frame, a `Sync` for another game) every tick.
#[test]
fn a_forged_checksum_cannot_end_a_session() {
    let rom = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/roms/nestest/nestest.nes"
    ))
    .unwrap();
    let mut n0 = Nes::from_rom(&rom).unwrap();
    let mut n1 = Nes::from_rom(&rom).unwrap();
    let id = SessionIdentity::of(&n0);
    let mut host = NetplayConnection::host(loopback(), id).unwrap();
    let host_addr = host.transport().local_addr().unwrap();
    let mut join = NetplayConnection::connect(loopback(), host_addr, id).unwrap();
    for _ in 0..400 {
        host.pump(0);
        join.pump(0);
        if host.is_synced() && join.is_synced() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(host.is_synced() && join.is_synced());
    let join_addr = join.transport().local_addr().unwrap();
    let cfg = SessionConfig::default();
    let mut s0 = RollbackSession::new(cfg, host.into_transport(), id);
    let mut s1 = RollbackSession::new(
        SessionConfig {
            local_player: 1,
            ..cfg
        },
        join.into_transport(),
        id,
    );
    let forger = UdpSocket::bind(loopback()).unwrap();
    let other_game = NetMessage::Sync {
        magic: NetMessage::SYNC_MAGIC,
        identity: SessionIdentity::new([1; 32], [2; 32]),
        key_share: [9; KEY_SHARE_LEN],
    }
    .to_bytes();
    for tick in 0..240u32 {
        for to in [host_addr, join_addr] {
            forger
                .send_to(&checksum(tick / 30 * 30).to_bytes(), to)
                .unwrap();
            forger.send_to(&other_game, to).unwrap();
        }
        s0.add_local_input(Buttons::empty());
        s0.advance(&mut n0)
            .expect("a forgery must not end the session");
        s1.add_local_input(Buttons::empty());
        s1.advance(&mut n1)
            .expect("a forgery must not end the session");
        std::thread::sleep(Duration::from_micros(300));
    }
    let confirmed = s0.last_confirmed_frame().expect("frames confirmed");
    assert!(
        confirmed > 120,
        "the session ran on (confirmed {confirmed})"
    );
    assert!(s0.transport().refused_datagrams() >= 400);
    assert!(
        s0.diagnostics().total() > 0,
        "the genuine checksums were still compared"
    );
}

#[test]
fn a_protocol_8_peer_is_refused_with_a_reason() {
    let id = identity();
    let mut host = NetplayConnection::host(loopback(), id).unwrap();
    let host_addr = host.transport().local_addr().unwrap();
    let old = UdpSocket::bind(loopback()).unwrap();
    // A v3.3.x `Sync`: 72 bytes under "RNE8", no key share.
    let mut sync = NetMessage::Sync {
        magic: NetMessage::PROTOCOL_8_SYNC_MAGIC,
        identity: id,
        key_share: [0; KEY_SHARE_LEN],
    }
    .to_bytes();
    sync.truncate(1 + 72);
    for _ in 0..100 {
        old.send_to(&sync, host_addr).unwrap();
        if host.pump(0) == ConnectionState::Disconnected {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        host.disconnect_reason(),
        Some(DisconnectReason::EmulatorMismatch {
            ours: id.epoch,
            theirs: Some(id.epoch),
        }),
        "an RNE8 peer is refused as another version, naming its epoch"
    );
    let text = rustynes_netplay::emulator_mismatch_text(id.epoch, Some(id.epoch));
    assert!(text.contains("emulates differently"), "{text}");
}

/// The mesh roster handshake for three players on loopback.
fn mesh3() -> Vec<UdpMeshTransport> {
    let id = identity();
    let probe = UdpSocket::bind(loopback()).unwrap();
    let port = probe.local_addr().unwrap();
    drop(probe);
    let mut host = MeshHost::bind(port, port, 3, id).unwrap();
    let listen = host.local_addr().unwrap();
    let mut joiners: Vec<MeshJoiner> = (1..3u8)
        .map(|p| MeshJoiner::connect(loopback(), listen, p, id).unwrap())
        .collect();
    let mut out: Vec<Option<UdpMeshTransport>> = vec![None, None, None];
    for _ in 0..4000 {
        if out[0].is_none() {
            out[0] = host.pump().unwrap();
        }
        for (i, j) in joiners.iter_mut().enumerate() {
            if out[i + 1].is_none() {
                out[i + 1] = j.pump().unwrap();
            }
        }
        if out.iter().all(Option::is_some) {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    out.into_iter().map(|t| t.expect("transport")).collect()
}

#[test]
fn a_mesh_peer_refuses_a_forgery() {
    let mut mesh = mesh3();
    assert!(mesh.iter().all(UdpMeshTransport::is_sealed));
    let host_addr = mesh[0].local_addr().unwrap();
    // A forgery straight at the host, unsealed.
    let forger = UdpSocket::bind(loopback()).unwrap();
    forger.send_to(&checksum(30).to_bytes(), host_addr).unwrap();
    // A genuine broadcast from player 2.
    mesh[2].send(&NetMessage::InputAck {
        player: 1,
        frame: 5,
    });
    let got = drain(&mut mesh[0]);
    assert_eq!(
        got,
        vec![NetMessage::InputAck {
            player: 1,
            frame: 5
        }]
    );
    assert_eq!(mesh[0].refused_datagrams(), 1);
    // The peers that hear player 1 hear it once each.
    mesh[1].send(&NetMessage::InputAck {
        player: 1,
        frame: 6,
    });
    assert_eq!(
        drain(&mut mesh[2]),
        vec![NetMessage::InputAck {
            player: 1,
            frame: 6
        }]
    );
    assert_eq!(
        drain(&mut mesh[0]),
        vec![NetMessage::InputAck {
            player: 1,
            frame: 6
        }]
    );
}

/// The mesh transport follows a peer to a new address too: built directly
/// with known shares, so the test can seal as player 1 from a socket the
/// transport has never seen.
#[test]
fn a_mesh_peer_follows_a_rebinding_peer() {
    let id = identity();
    let shares = [(0u8, [1u8; 16]), (1, [2; 16]), (2, [3; 16])];
    let s0 = UdpSocket::bind(loopback()).unwrap();
    let a0 = s0.local_addr().unwrap();
    let old1 = UdpSocket::bind(loopback()).unwrap();
    let old2 = UdpSocket::bind(loopback()).unwrap();
    let mut t0 = UdpMeshTransport::new_sealed(
        s0,
        &[
            (1, old1.local_addr().unwrap()),
            (2, old2.local_addr().unwrap()),
        ],
        DatagramAuth::mesh(&id, &shares, 0).unwrap(),
        None,
    )
    .unwrap();
    let mut p1 = DatagramAuth::mesh(&id, &shares, 1).unwrap();
    let rebound = UdpSocket::bind(loopback()).unwrap();
    rebound
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    rebound
        .send_to(
            &p1.seal(
                &NetMessage::InputAck {
                    player: 1,
                    frame: 9,
                }
                .to_bytes(),
            ),
            a0,
        )
        .unwrap();
    assert_eq!(
        drain(&mut t0),
        vec![NetMessage::InputAck {
            player: 1,
            frame: 9
        }]
    );
    assert_eq!(t0.roamed(), 1);
    assert!(t0.peers().contains(&rebound.local_addr().unwrap()));
    t0.send(&NetMessage::InputAck {
        player: 1,
        frame: 10,
    });
    let mut buf = [0u8; 1500];
    let (n, _) = rebound
        .recv_from(&mut buf)
        .expect("player 0 follows player 1");
    let (sender, payload) = p1.open(&buf[..n]).unwrap();
    assert_eq!(sender, 0);
    assert_eq!(
        NetMessage::from_bytes(payload),
        Some(NetMessage::InputAck {
            player: 1,
            frame: 10
        })
    );
}

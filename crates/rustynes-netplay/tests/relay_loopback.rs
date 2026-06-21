//! v1.8.7 TURN relay proof: the full register → discover → exchange → punch →
//! **relay** flow drives two [`NatConnect`] orchestrators through a symmetric-NAT
//! fallback to a confirmed [`RollbackSession`] **over a TURN relay**, all on
//! `127.0.0.1`.
//!
//! This is the relay analogue of `nat_loopback.rs` (which proves the punched /
//! cone-NAT path). It adds two more in-process mocks on top of that test's mock
//! signaling relay:
//!
//! - a **mock STUN responder** that reports a *bogus, unreachable* reflexive
//!   address so the hole punch can never succeed — forcing the orchestrator to
//!   fall through to the TURN relay (the symmetric-NAT behaviour we cannot stage
//!   on loopback otherwise);
//! - a **mock TURN relay** — a tiny UDP server speaking enough of RFC 8656 to be
//!   driven by the production [`rustynes_netplay::relay::TurnClient`]: the
//!   long-term-credential `Allocate` two-step (401 challenge → authenticated
//!   success with an `XOR-RELAYED-ADDRESS`), `CreatePermission`, and — the heart
//!   of it — forwarding `Send-Indication`s between the two allocations as
//!   `Data-Indication`s, so the two relayed transports actually reach each other.
//!
//! With both in place the two orchestrators register, "discover" their bogus
//! addresses, exchange them, fail to punch, allocate relays, exchange the relayed
//! addresses, reach [`NatPhase::Synced`] on the **relay** path, hand off
//! [`NetplayConnection`]s whose `is_relayed()` is `true`, finish the `Sync`
//! handshake over the relay, run an N-frame [`RollbackSession`], and assert the
//! confirmed digests agree — the same proof shape as `nat_loopback.rs`, but every
//! gameplay byte rides the mock TURN relay.
//!
//! Native-only and `netplay-client`-gated; compiles to nothing on wasm32.
#![cfg(all(not(target_arch = "wasm32"), feature = "netplay-client"))]
// Integration-test scaffolding — relax the pedantic/nursery lints that fire on
// the mock relay + the long end-to-end flow (not worth fracturing for a test).
#![allow(
    clippy::items_after_statements,
    clippy::collection_is_never_read,
    clippy::manual_let_else,
    clippy::collapsible_if,
    clippy::too_many_lines,
    clippy::missing_const_for_fn
)]

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rustynes_core::{Buttons, Nes};
use rustynes_netplay::signaling::{Action, ClientId, Relay, SignalMessage};
use rustynes_netplay::{
    ConnectionState, NatConfig, NatConnect, NatPhase, NetplayConnection, RollbackSession,
    SessionConfig, SplitMix64, TurnConfig, fnv1a64,
};

const MAGIC_COOKIE: u32 = 0x2112_A442;
const HEADER_LEN: usize = 20;

// ── STUN/TURN message types (mirroring relay.rs, server side) ────────────────
const MSG_ALLOCATE_REQUEST: u16 = 0x0003;
const MSG_ALLOCATE_SUCCESS: u16 = 0x0103;
const MSG_ALLOCATE_ERROR: u16 = 0x0113;
const MSG_CREATE_PERMISSION_REQUEST: u16 = 0x0008;
const MSG_CREATE_PERMISSION_SUCCESS: u16 = 0x0108;
const MSG_SEND_INDICATION: u16 = 0x0016;
const MSG_DATA_INDICATION: u16 = 0x0017;

const ATTR_XOR_PEER_ADDRESS: u16 = 0x0012;
const ATTR_DATA: u16 = 0x0013;
const ATTR_XOR_RELAYED_ADDRESS: u16 = 0x0016;
const ATTR_ERROR_CODE: u16 = 0x0009;
const ATTR_REALM: u16 = 0x0014;
const ATTR_NONCE: u16 = 0x0015;

fn nestest_rom() -> Vec<u8> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root is two levels above the crate manifest");
    let rom = root.join("tests/roms/nestest/nestest.nes");
    std::fs::read(&rom).unwrap_or_else(|e| panic!("read nestest rom {}: {e}", rom.display()))
}

fn gameplay_digest(nes: &Nes) -> u64 {
    fnv1a64(nes.framebuffer()) ^ nes.cycle().wrapping_mul(0x100_0000_01b3)
}

// ── tiny STUN/TURN encoder helpers (server side) ─────────────────────────────

/// Append a STUN attribute `(type, value)` with 4-byte padding.
fn push_attr(out: &mut Vec<u8>, attr_type: u16, value: &[u8]) {
    out.extend_from_slice(&attr_type.to_be_bytes());
    out.extend_from_slice(&u16::try_from(value.len()).unwrap().to_be_bytes());
    out.extend_from_slice(value);
    let pad = (4 - value.len() % 4) % 4;
    out.extend(std::iter::repeat_n(0u8, pad));
}

/// Patch the STUN header length to cover the attribute section.
fn patch_length(out: &mut [u8]) {
    let attr_len = u16::try_from(out.len() - HEADER_LEN).unwrap();
    out[2..4].copy_from_slice(&attr_len.to_be_bytes());
}

/// Encode a v4 (XOR-)address attribute value (loopback is v4 only).
fn encode_xor_v4(addr: SocketAddr) -> Vec<u8> {
    let SocketAddr::V4(v4) = addr else {
        panic!("mock turn is loopback v4 only");
    };
    let cookie_be = MAGIC_COOKIE.to_be_bytes();
    let cookie_hi16 = u16::try_from(MAGIC_COOKIE >> 16).unwrap();
    let x_port = v4.port() ^ cookie_hi16;
    let mut octets = v4.ip().octets();
    for (b, k) in octets.iter_mut().zip(cookie_be.iter()) {
        *b ^= *k;
    }
    let mut out = vec![0u8, 0x01]; // reserved + family v4
    out.extend_from_slice(&x_port.to_be_bytes());
    out.extend_from_slice(&octets);
    out
}

/// Decode a v4 (XOR-)address attribute value.
fn decode_xor_v4(value: &[u8]) -> Option<SocketAddr> {
    if value.len() < 8 || value[1] != 0x01 {
        return None;
    }
    let cookie_be = MAGIC_COOKIE.to_be_bytes();
    let cookie_hi16 = u16::try_from(MAGIC_COOKIE >> 16).unwrap();
    let port = u16::from_be_bytes([value[2], value[3]]) ^ cookie_hi16;
    let mut octets: [u8; 4] = value[4..8].try_into().ok()?;
    for (b, k) in octets.iter_mut().zip(cookie_be.iter()) {
        *b ^= *k;
    }
    Some(SocketAddr::V4(SocketAddrV4::new(octets.into(), port)))
}

/// Iterate `(type, value)` over a STUN attribute section, honoring padding.
fn iter_attrs(attrs: &[u8]) -> Vec<(u16, Vec<u8>)> {
    let mut out = Vec::new();
    let mut rest = attrs;
    while rest.len() >= 4 {
        let ty = u16::from_be_bytes([rest[0], rest[1]]);
        let len = u16::from_be_bytes([rest[2], rest[3]]) as usize;
        let Some(value) = rest.get(4..4 + len) else {
            break;
        };
        out.push((ty, value.to_vec()));
        let padded = 4 + len.div_ceil(4) * 4;
        rest = rest.get(padded..).unwrap_or(&[]);
    }
    out
}

/// Build a STUN/TURN response with the given message type echoing `tx`.
fn build_msg(msg_type: u16, tx: &[u8; 12], attrs: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&msg_type.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // length placeholder
    out.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
    out.extend_from_slice(tx);
    for (ty, val) in attrs {
        push_attr(&mut out, *ty, val);
    }
    patch_length(&mut out);
    out
}

/// A mock TURN relay: a single UDP socket on `127.0.0.1` that allocates a
/// relayed address per client (keyed by source `SocketAddr`), answers the
/// long-term-credential `Allocate` two-step + `CreatePermission`, and forwards
/// `Send-Indication`s between the two allocations as `Data-Indication`s. Returns
/// its address + a stop flag + the join handle.
fn spawn_mock_turn() -> (SocketAddr, Arc<AtomicBool>, thread::JoinHandle<()>) {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind mock turn");
    socket
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    let addr = socket.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = Arc::clone(&stop);

    let handle = thread::spawn(move || {
        // client source addr → its assigned relayed transport address.
        let mut relayed_of: HashMap<SocketAddr, SocketAddr> = HashMap::new();
        // relayed transport address → the owning client's source addr.
        let mut owner_of: HashMap<SocketAddr, SocketAddr> = HashMap::new();
        // A counter handing out distinct synthetic relayed addresses.
        let mut next_relayed_port: u16 = 49_000;
        let mut buf = [0u8; 1500];

        while !stop_t.load(Ordering::Relaxed) {
            let (len, from) = match socket.recv_from(&mut buf) {
                Ok(v) => v,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(_) => break,
            };
            if len < HEADER_LEN {
                continue;
            }
            let msg_type = u16::from_be_bytes([buf[0], buf[1]]);
            let cookie = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
            if cookie != MAGIC_COOKIE {
                continue;
            }
            let tx: [u8; 12] = buf[8..20].try_into().unwrap();
            let msg_len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
            let attrs_bytes = buf
                .get(HEADER_LEN..HEADER_LEN + msg_len)
                .unwrap_or(&[])
                .to_vec();
            let attrs = iter_attrs(&attrs_bytes);
            let has = |ty: u16| attrs.iter().any(|(t, _)| *t == ty);

            match msg_type {
                MSG_ALLOCATE_REQUEST => {
                    // First (unauthenticated) Allocate has no MESSAGE-INTEGRITY:
                    // answer 401 with REALM + NONCE. The authenticated retry
                    // carries the integrity attr (0x0008) — answer success.
                    const ATTR_MESSAGE_INTEGRITY: u16 = 0x0008;
                    if has(ATTR_MESSAGE_INTEGRITY) {
                        // Assign (or reuse) a relayed address for this client.
                        let relayed = *relayed_of.entry(from).or_insert_with(|| {
                            let a = SocketAddr::V4(SocketAddrV4::new(
                                Ipv4Addr::new(203, 0, 113, 1),
                                next_relayed_port,
                            ));
                            next_relayed_port += 1;
                            a
                        });
                        owner_of.insert(relayed, from);
                        let resp = build_msg(
                            MSG_ALLOCATE_SUCCESS,
                            &tx,
                            &[(ATTR_XOR_RELAYED_ADDRESS, encode_xor_v4(relayed))],
                        );
                        let _ = socket.send_to(&resp, from);
                    } else {
                        // 401 challenge: ERROR-CODE 401 + REALM + NONCE.
                        let mut error_code = vec![0u8, 0u8, 4u8, 1u8]; // class 4, number 01
                        error_code.extend_from_slice(b"stale"); // reason phrase
                        let resp = build_msg(
                            MSG_ALLOCATE_ERROR,
                            &tx,
                            &[
                                (ATTR_ERROR_CODE, error_code),
                                (ATTR_REALM, b"rustynes.test".to_vec()),
                                (ATTR_NONCE, b"nonce-0123456789".to_vec()),
                            ],
                        );
                        let _ = socket.send_to(&resp, from);
                    }
                }
                MSG_CREATE_PERMISSION_REQUEST => {
                    let resp = build_msg(MSG_CREATE_PERMISSION_SUCCESS, &tx, &[]);
                    let _ = socket.send_to(&resp, from);
                }
                MSG_SEND_INDICATION => {
                    // Forward the DATA to the peer addressed by XOR-PEER-ADDRESS
                    // (which is a relayed transport address) as a Data Indication
                    // to that allocation's owning client, tagged with the SENDER's
                    // relayed address.
                    let mut peer_relayed = None;
                    let mut data = None;
                    for (ty, val) in &attrs {
                        match *ty {
                            ATTR_XOR_PEER_ADDRESS => peer_relayed = decode_xor_v4(val),
                            ATTR_DATA => data = Some(val.clone()),
                            _ => {}
                        }
                    }
                    let (Some(peer_relayed), Some(data)) = (peer_relayed, data) else {
                        continue;
                    };
                    let Some(&dest_client) = owner_of.get(&peer_relayed) else {
                        continue; // no such allocation yet
                    };
                    let sender_relayed = relayed_of.get(&from).copied();
                    let Some(sender_relayed) = sender_relayed else {
                        continue;
                    };
                    // Data Indication carries XOR-PEER-ADDRESS = sender's relayed
                    // addr (so the receiver sees who sent it) + DATA. A fresh tx
                    // id is fine for an indication.
                    let ind = build_msg(
                        MSG_DATA_INDICATION,
                        &[0x99u8; 12],
                        &[
                            (ATTR_XOR_PEER_ADDRESS, encode_xor_v4(sender_relayed)),
                            (ATTR_DATA, data),
                        ],
                    );
                    let _ = socket.send_to(&ind, dest_client);
                }
                _ => {}
            }
        }
    });
    (addr, stop, handle)
}

/// A mock STUN responder that reports a *bogus, unreachable* reflexive address
/// (so the hole punch never lands and the orchestrator falls through to TURN).
fn spawn_unreachable_stun() -> (SocketAddr, Arc<AtomicBool>, thread::JoinHandle<()>) {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind mock stun");
    socket
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    let addr = socket.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = Arc::clone(&stop);
    // A TEST-NET-2 address (RFC 5737) that nothing on loopback will ever answer,
    // with a per-peer-distinct port so the two peers' "public" addrs differ.
    let bogus_port = Arc::new(std::sync::atomic::AtomicU16::new(40_000));
    let handle = thread::spawn(move || {
        let mut buf = [0u8; 512];
        while !stop_t.load(Ordering::Relaxed) {
            match socket.recv_from(&mut buf) {
                Ok((len, peer)) if len >= 20 => {
                    let tx: [u8; 12] = buf[8..20].try_into().unwrap();
                    let port = bogus_port.fetch_add(1, Ordering::Relaxed);
                    let bogus =
                        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 7), port));
                    // Binding Success with XOR-MAPPED-ADDRESS = the bogus addr.
                    let resp = build_msg(0x0101, &tx, &[(0x0020, encode_xor_v4(bogus))]);
                    let _ = socket.send_to(&resp, peer);
                }
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => break,
            }
        }
    });
    (addr, stop, handle)
}

/// A mock signaling relay: a real WebSocket server on `127.0.0.1` driving the
/// production [`Relay`] routing logic (identical to `nat_loopback.rs`).
fn spawn_mock_relay() -> (String, Arc<AtomicBool>) {
    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind mock relay");
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{addr}");
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = Arc::clone(&stop);

    let relay = Arc::new(Mutex::new(Relay::new()));
    type Outbox = Arc<Mutex<HashMap<ClientId, std::sync::mpsc::Sender<SignalMessage>>>>;
    let outbox: Outbox = Arc::new(Mutex::new(HashMap::new()));
    let next_id = Arc::new(std::sync::atomic::AtomicU64::new(1));

    thread::spawn(move || {
        let mut workers = Vec::new();
        while !stop_t.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _peer)) => {
                    stream.set_nonblocking(false).ok();
                    let relay = Arc::clone(&relay);
                    let outbox = Arc::clone(&outbox);
                    let id = next_id.fetch_add(1, Ordering::Relaxed);
                    let stop_w = Arc::clone(&stop_t);
                    workers.push(thread::spawn(move || {
                        relay_client_worker(stream, id, &relay, &outbox, &stop_w);
                    }));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(2));
                }
                Err(_) => break,
            }
        }
    });

    (url, stop)
}

#[allow(clippy::type_complexity)]
fn relay_client_worker(
    stream: std::net::TcpStream,
    id: ClientId,
    relay: &std::sync::Mutex<Relay>,
    outbox: &Arc<std::sync::Mutex<HashMap<ClientId, std::sync::mpsc::Sender<SignalMessage>>>>,
    stop: &AtomicBool,
) {
    use tungstenite::Message;

    let mut ws = match tungstenite::accept(stream) {
        Ok(ws) => ws,
        Err(_) => return,
    };
    let _ = ws
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(10)));
    let (out_tx, out_rx) = std::sync::mpsc::channel::<SignalMessage>();
    outbox.lock().unwrap().insert(id, out_tx);

    while !stop.load(Ordering::Relaxed) {
        while let Ok(msg) = out_rx.try_recv() {
            if ws.send(Message::Text(msg.to_json().into())).is_err() {
                return;
            }
        }
        match ws.read() {
            Ok(Message::Text(txt)) => {
                if let Some(msg) = SignalMessage::parse(&txt) {
                    let actions = relay.lock().unwrap().handle(id, msg);
                    dispatch(&actions, outbox);
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
    }
    let _ = relay.lock().unwrap().disconnect(id);
}

fn dispatch(
    actions: &[Action],
    outbox: &std::sync::Mutex<HashMap<ClientId, std::sync::mpsc::Sender<SignalMessage>>>,
) {
    let map = outbox.lock().unwrap();
    for action in actions {
        if let Action::Send { to, msg } = action {
            if let Some(tx) = map.get(to) {
                let _ = tx.send(msg.clone());
            }
        }
    }
}

/// Drive a [`NetplayConnection`] to [`ConnectionState::Synced`] (bounded).
fn pump_connection_to_synced(conn: &mut NetplayConnection, rounds: usize) {
    for _ in 0..rounds {
        conn.pump(0);
        if conn.is_synced() {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn input_stream(frames: u32, seed: u64) -> Vec<Buttons> {
    let mut r = SplitMix64::new(seed);
    (0..frames)
        .map(|_| Buttons::from_bits_truncate(r.next_u8()))
        .collect()
}

#[test]
fn nat_connect_loopback_relay_then_session_digests_agree() {
    let rom = nestest_rom();
    let mut host_nes = Nes::from_rom(&rom).expect("host nes");
    let mut join_nes = Nes::from_rom(&rom).expect("join nes");
    let rom_hash = *host_nes.rom_sha256();

    let (stun_addr, stun_stop, stun_handle) = spawn_unreachable_stun();
    let (turn_addr, turn_stop, turn_handle) = spawn_mock_turn();
    let (relay_url, relay_stop) = spawn_mock_relay();
    thread::sleep(Duration::from_millis(20));

    let turn_cfg = TurnConfig {
        server: turn_addr,
        username: "user".into(),
        credential: "pass".into(),
    };
    let cfg = |url: &str| NatConfig {
        stun_servers: vec![stun_addr.to_string()],
        turn: Some(turn_cfg.clone()),
        signaling_url: url.to_string(),
    };

    let (mut host, room) =
        NatConnect::host(2, rom_hash, cfg(&relay_url), 0x57_0000_0011).expect("host");
    let mut join = NatConnect::join(&room, rom_hash, cfg(&relay_url), 22).expect("join");

    // Drive both until both reach Synced (the punch will fail on the bogus STUN
    // addrs and they fall through to the relay). Allow ample time: the punch
    // timeout (5s) must elapse first, then the relay handshake runs.
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut host_done = false;
    let mut join_done = false;
    while Instant::now() < deadline && !(host_done && join_done) {
        let hp = host.pump();
        let jp = join.pump();
        host_done = matches!(hp, NatPhase::Synced);
        join_done = matches!(jp, NatPhase::Synced);
        if let NatPhase::Failed(reason) = hp {
            panic!("host orchestration failed: {reason}");
        }
        if let NatPhase::Failed(reason) = jp {
            panic!("join orchestration failed: {reason}");
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(
        host_done && join_done,
        "both orchestrators must reach Synced over the relay (host={:?}, join={:?})",
        host.phase(),
        join.phase()
    );
    assert!(host.is_relayed(), "host must have fallen back to the relay");
    assert!(join.is_relayed(), "join must have fallen back to the relay");

    // Hand off the relay transports + assert is_relayed propagates.
    let mut host_conn = host.into_connection();
    let mut join_conn = join.into_connection();
    assert!(host_conn.is_relayed(), "host connection rides the relay");
    assert!(join_conn.is_relayed(), "join connection rides the relay");

    // Finish the Sync handshake over the relay.
    let rounds = 600;
    for _ in 0..rounds {
        host_conn.pump(0);
        join_conn.pump(0);
        if host_conn.is_synced() && join_conn.is_synced() {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    pump_connection_to_synced(&mut host_conn, rounds);
    pump_connection_to_synced(&mut join_conn, rounds);
    assert_eq!(host_conn.state(), ConnectionState::Synced, "host handshake");
    assert_eq!(join_conn.state(), ConnectionState::Synced, "join handshake");

    // Run a short rollback session over the two relay transports.
    let frames = 90u32;
    let compare_frame = frames - 30;
    let host_stream = input_stream(frames, 0x1111);
    let join_stream = input_stream(frames, 0x2222);

    let host_t = host_conn.into_transport();
    let join_t = join_conn.into_transport();
    assert!(host_t.is_relayed(), "host transport rides the relay");
    assert!(join_t.is_relayed(), "join transport rides the relay");
    let mut host_sess = RollbackSession::new(
        SessionConfig {
            num_players: 2,
            local_player: 0,
            ..SessionConfig::default()
        },
        host_t,
        rom_hash,
    );
    let mut join_sess = RollbackSession::new(
        SessionConfig {
            num_players: 2,
            local_player: 1,
            ..SessionConfig::default()
        },
        join_t,
        rom_hash,
    );

    let mut ha = 0u32;
    let mut ja = 0u32;
    let max_ticks = compare_frame * 60 + 8000;
    let mut ticks = 0;
    while ticks < max_ticks
        && !(host_sess
            .last_confirmed_frame()
            .is_some_and(|c| c >= compare_frame)
            && join_sess
                .last_confirmed_frame()
                .is_some_and(|c| c >= compare_frame))
    {
        ticks += 1;
        while ha <= host_sess.current_frame() && (ha as usize) < host_stream.len() {
            host_sess.add_local_input(host_stream[ha as usize]);
            ha += 1;
        }
        while ja <= join_sess.current_frame() && (ja as usize) < join_stream.len() {
            join_sess.add_local_input(join_stream[ja as usize]);
            ja += 1;
        }
        host_sess.advance(&mut host_nes).expect("host advance");
        join_sess.advance(&mut join_nes).expect("join advance");
        thread::sleep(Duration::from_micros(200));
    }

    let host_digest = host_sess
        .confirmed_entering_digest(compare_frame)
        .expect("host confirmed digest");
    let join_digest = join_sess
        .confirmed_entering_digest(compare_frame)
        .expect("join confirmed digest");
    let _ = gameplay_digest(&host_nes);
    assert_eq!(
        host_digest, join_digest,
        "the two peers must hold identical confirmed state after a TURN-relay handoff"
    );

    stun_stop.store(true, Ordering::Relaxed);
    turn_stop.store(true, Ordering::Relaxed);
    relay_stop.store(true, Ordering::Relaxed);
    let _ = stun_handle.join();
    let _ = turn_handle.join();
}

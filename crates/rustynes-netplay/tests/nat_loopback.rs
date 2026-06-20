//! v1.8.7 NAT-traversal proof: the full register → discover → exchange → punch
//! flow drives two [`NatConnect`] orchestrators to a confirmed
//! [`RollbackSession`] over **real loopback UDP sockets**.
//!
//! This is the orchestration analogue of `udp_loopback.rs` (which proved the
//! 2-player session over real UDP given an already-known peer) and of
//! `stun.rs`'s isolated unit tests (which proved the STUN + hole-punch pieces
//! independently). Here we put the whole pipeline together with **in-process
//! mocks** standing in for the network services that cannot run in CI:
//!
//! - a **mock signaling relay** — a real WebSocket server (sync `tungstenite`)
//!   on `127.0.0.1` that drives the production
//!   [`rustynes_netplay::signaling::Relay`] routing logic, so the two clients
//!   exchange `Join` / `Joined` / `PeerJoined` / `PublicAddr` exactly as they
//!   would against the deployed `signaling_server`;
//! - a **mock STUN responder** — a UDP socket on `127.0.0.1` that answers each
//!   Binding Request with the source address it observed (i.e. the client's own
//!   loopback `IP:port`), so discovery yields a reflexive address the peer can
//!   actually reach on loopback.
//!
//! With both mocks in place the two orchestrators register, discover their
//! (loopback) public addresses, exchange them over the relay, punch — and since
//! loopback has no NAT, the punch packets reach each other directly — reach
//! [`NatPhase::Synced`], hand off [`NetplayConnection`]s, and run an N-frame
//! [`RollbackSession`] whose confirmed digests agree (the same proof shape as
//! `mesh_udp.rs` / `udp_loopback.rs`).
//!
//! Native-only and `netplay-client`-gated (the orchestrator + signaling client
//! live behind that feature); compiles to nothing on wasm32.
#![cfg(all(not(target_arch = "wasm32"), feature = "netplay-client"))]
// Integration-test scaffolding — relax the pedantic/nursery lints that fire on the
// mock relay + the long end-to-end flow (not worth fracturing for a test).
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
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use rustynes_core::{Buttons, Nes};
use rustynes_netplay::signaling::{Action, ClientId, Relay, SignalMessage};
use rustynes_netplay::{
    ConnectionState, NatConfig, NatConnect, NatPhase, NetplayConnection, RollbackSession,
    SessionConfig, SplitMix64, fnv1a64,
};

const MAGIC_COOKIE: u32 = 0x2112_A442;

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

/// A mock STUN server: bind a UDP socket and, in a background thread, answer
/// every Binding Request with a Binding Success Response carrying the source
/// address it observed in an XOR-MAPPED-ADDRESS attribute. Returns its address +
/// a stop flag.
fn spawn_mock_stun() -> (SocketAddr, Arc<AtomicBool>, thread::JoinHandle<()>) {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind mock stun");
    socket
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    let addr = socket.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = Arc::clone(&stop);
    let handle = thread::spawn(move || {
        let mut buf = [0u8; 512];
        while !stop_t.load(Ordering::Relaxed) {
            match socket.recv_from(&mut buf) {
                Ok((len, from)) if len >= 20 => {
                    // Echo the transaction id (bytes 8..20) into a success
                    // response with an XOR-MAPPED-ADDRESS for `from` (v4 only on
                    // loopback).
                    let tx: [u8; 12] = buf[8..20].try_into().unwrap();
                    let resp = build_stun_success(from, &tx);
                    let _ = socket.send_to(&resp, from);
                }
                Ok(_) => {}
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => break,
            }
        }
    });
    (addr, stop, handle)
}

/// Build a STUN Binding Success Response with an XOR-MAPPED-ADDRESS for a v4
/// `addr`.
fn build_stun_success(addr: SocketAddr, tx: &[u8; 12]) -> Vec<u8> {
    let SocketAddr::V4(v4) = addr else {
        panic!("mock stun is loopback v4 only");
    };
    let cookie_be = MAGIC_COOKIE.to_be_bytes();
    let cookie_hi16 = u16::try_from(MAGIC_COOKIE >> 16).unwrap();
    let x_port = v4.port() ^ cookie_hi16;
    let mut x_addr = v4.ip().octets();
    for (b, k) in x_addr.iter_mut().zip(cookie_be.iter()) {
        *b ^= *k;
    }
    let mut value = vec![0u8, 0x01]; // reserved + family v4
    value.extend_from_slice(&x_port.to_be_bytes());
    value.extend_from_slice(&x_addr);

    let mut attr = Vec::new();
    attr.extend_from_slice(&0x0020u16.to_be_bytes()); // XOR-MAPPED-ADDRESS
    attr.extend_from_slice(&u16::try_from(value.len()).unwrap().to_be_bytes());
    attr.extend_from_slice(&value);

    let mut msg = Vec::new();
    msg.extend_from_slice(&0x0101u16.to_be_bytes()); // Binding Success
    msg.extend_from_slice(&u16::try_from(attr.len()).unwrap().to_be_bytes());
    msg.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
    msg.extend_from_slice(tx);
    msg.extend_from_slice(&attr);
    msg
}

/// A mock signaling relay: a real WebSocket server on `127.0.0.1` driving the
/// production [`Relay`] routing logic. Each accepted connection gets a worker
/// thread; a shared [`Relay`] (behind a mutex) routes messages, and an outbound
/// map fans [`Action::Send`]s to the right client's WS sink.
fn spawn_mock_relay() -> (String, Arc<AtomicBool>) {
    use std::sync::Mutex;

    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind mock relay");
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{addr}");
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = Arc::clone(&stop);

    // The relay + a per-client outbound queue, shared across connection workers.
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
    // Give the socket a short read timeout so we can interleave outbound sends.
    let _ = ws
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(10)));
    let (out_tx, out_rx) = std::sync::mpsc::channel::<SignalMessage>();
    outbox.lock().unwrap().insert(id, out_tx);

    while !stop.load(Ordering::Relaxed) {
        // Flush any messages routed TO this client.
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

#[test]
fn nat_connect_loopback_punch_then_session_digests_agree() {
    let rom = nestest_rom();
    let mut host_nes = Nes::from_rom(&rom).expect("host nes");
    let mut join_nes = Nes::from_rom(&rom).expect("join nes");
    let rom_hash = *host_nes.rom_sha256();

    let (stun_addr, stun_stop, stun_handle) = spawn_mock_stun();
    let (relay_url, relay_stop) = spawn_mock_relay();
    // Give the relay listener a moment to be ready.
    thread::sleep(Duration::from_millis(20));

    let cfg = |url: &str| NatConfig {
        stun_servers: vec![stun_addr.to_string()],
        turn: None,
        signaling_url: url.to_string(),
    };

    let (mut host, room) =
        NatConnect::host(2, rom_hash, cfg(&relay_url), 0x57_0000_0001).expect("host");
    let mut join = NatConnect::join(&room, rom_hash, cfg(&relay_url), 2).expect("join");

    // Drive both orchestrators until both reach Synced (or fail / time out).
    let deadline = Instant::now() + Duration::from_secs(15);
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
        "both orchestrators must reach Synced (host={:?}, join={:?})",
        host.phase(),
        join.phase()
    );

    // Hand off the punched transports to NetplayConnections + finish the
    // handshake over the now-open loopback mapping.
    let mut host_conn = host.into_connection();
    let mut join_conn = join.into_connection();
    let rounds = 400;
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

    // Run a short rollback session over the two real loopback transports and
    // assert the confirmed digests agree.
    let frames = 90u32;
    let compare_frame = frames - 30;
    let host_stream = input_stream(frames, 0x1111);
    let join_stream = input_stream(frames, 0x2222);

    let host_t = host_conn.into_transport();
    let join_t = join_conn.into_transport();
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
    let max_ticks = compare_frame * 60 + 4000;
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
        "the two peers must hold identical confirmed state after NAT-traversal handoff"
    );

    // Tear down the mocks.
    stun_stop.store(true, Ordering::Relaxed);
    relay_stop.store(true, Ordering::Relaxed);
    let _ = stun_handle.join();
}

fn input_stream(frames: u32, seed: u64) -> Vec<Buttons> {
    let mut r = SplitMix64::new(seed);
    (0..frames)
        .map(|_| Buttons::from_bits_truncate(r.next_u8()))
        .collect()
}

// Silence an unused import on some feature permutations.
#[allow(dead_code)]
fn _addr_helper() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
}

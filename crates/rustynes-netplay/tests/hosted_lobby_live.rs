//! v3.4.0 (`T-HOSTED-NETPLAY`): the native [`SignalingClient`] against a
//! RUNNING hosted lobby — `wrangler dev` locally, or the deployed Worker.
//!
//! The conformance vectors (`hosted_signaling_vectors.rs` plus the Worker's
//! replay of them) prove the lobby answers like the reference relay. This test
//! closes the remaining gap the other way round: the production Rust client,
//! over its real `tungstenite` WebSocket, completes a room handshake against
//! the TypeScript service. Two clients join one room, the host learns of the
//! joiner, and a `public-addr` (the native rendezvous message) travels in each
//! direction.
//!
//! `#[ignore]`d because it needs a lobby process: CI runs neither Node nor the
//! Worker in the Rust jobs. Run it by hand:
//!
//! ```text
//! cd services/hosted && npx wrangler dev --port 8799 &
//! RUSTYNES_HOSTED_LOBBY_URL=ws://127.0.0.1:8799/signal \
//!   cargo test -p rustynes-netplay --test hosted_lobby_live -- --ignored
//! ```
//!
//! After a deploy, point `RUSTYNES_HOSTED_LOBBY_URL` at
//! `wss://<hosted base>/signal` to check the live service the same way.
#![cfg(all(not(target_arch = "wasm32"), feature = "netplay-client"))]

use std::time::{Duration, Instant};

use rustynes_netplay::signaling::SignalMessage;
use rustynes_netplay::{SignalEvent, SignalingClient};

/// Poll `client` until `pick` returns a value, or fail after five seconds.
fn wait_for<T>(
    client: &SignalingClient,
    what: &str,
    mut pick: impl FnMut(&SignalEvent) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        for ev in client.poll() {
            if let SignalEvent::Closed(why) = &ev {
                panic!("connection closed while waiting for {what}: {why}");
            }
            if let Some(v) = pick(&ev) {
                return v;
            }
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "needs a running hosted lobby: set RUSTYNES_HOSTED_LOBBY_URL"]
fn the_native_client_completes_a_room_handshake_on_the_hosted_lobby() {
    let url = std::env::var("RUSTYNES_HOSTED_LOBBY_URL").expect("set RUSTYNES_HOSTED_LOBBY_URL");
    // A room code per run, so a re-run never meets a stale room.
    let room = format!(
        "live{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_micros() % 1_000_000_000)
    );
    let join = || SignalMessage::Join {
        room: room.clone(),
        rom_hash: "ab".repeat(32),
        max_players: 2,
    };

    let host = SignalingClient::connect(&url);
    wait_for(&host, "host connect", |e| {
        (*e == SignalEvent::Connected).then_some(())
    });
    host.send(join());
    let host_slot = wait_for(&host, "host joined", |e| match e {
        SignalEvent::Message(SignalMessage::Joined {
            slot,
            max_players: 2,
        }) => Some(*slot),
        _ => None,
    });
    assert_eq!(host_slot, 0);

    let guest = SignalingClient::connect(&url);
    wait_for(&guest, "guest connect", |e| {
        (*e == SignalEvent::Connected).then_some(())
    });
    guest.send(join());
    let guest_slot = wait_for(&guest, "guest joined", |e| match e {
        SignalEvent::Message(SignalMessage::Joined {
            slot,
            max_players: 2,
        }) => Some(*slot),
        _ => None,
    });
    assert_eq!(guest_slot, 1);
    wait_for(&host, "peer-joined", |e| {
        (*e == SignalEvent::Message(SignalMessage::PeerJoined { slot: 1 })).then_some(())
    });

    let to_guest = SignalMessage::PublicAddr {
        from: 0,
        to: 1,
        addr: "198.51.100.9:40000".into(),
    };
    host.send(to_guest.clone());
    wait_for(&guest, "host's public-addr", |e| {
        (*e == SignalEvent::Message(to_guest.clone())).then_some(())
    });

    let to_host = SignalMessage::PublicAddr {
        from: 1,
        to: 0,
        addr: "203.0.113.5:55555".into(),
    };
    guest.send(to_host.clone());
    wait_for(&host, "guest's public-addr", |e| {
        (*e == SignalEvent::Message(to_host.clone())).then_some(())
    });

    drop(guest);
    wait_for(&host, "peer-left", |e| {
        (*e == SignalEvent::Message(SignalMessage::PeerLeft { slot: 1 })).then_some(())
    });
}

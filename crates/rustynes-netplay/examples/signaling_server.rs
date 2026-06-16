//! v2.6.0: the reference **WebRTC signaling server** — a standalone WebSocket
//! relay broker for browser netplay.
//!
//! It pairs two browser peers by **room id** and relays their SDP offer/answer +
//! ICE candidates so a WebRTC peer connection can form; it carries **no
//! gameplay traffic** (that flows peer-to-peer over the WebRTC data channel once
//! connected). The routing logic is the pure, async-free
//! [`rustynes_netplay::signaling::Relay`] — this binary is just the async WebSocket
//! plumbing around it (accept connections -> parse text frames -> `Relay::handle`
//! -> fan the resulting actions back out).
//!
//! # Run
//!
//! ```text
//! cargo run -p rustynes-netplay --features signaling-server --example signaling_server
//! # or bind a specific address:
//! cargo run -p rustynes-netplay --features signaling-server --example signaling_server -- 0.0.0.0:9000
//! ```
//!
//! It listens on `127.0.0.1:9000` by default (override with one CLI arg). Point
//! the wasm frontend's signaling-client URL at `ws://<host>:9000/`.
//!
//! # Deploy
//!
//! - Run behind a TLS-terminating reverse proxy (nginx / Caddy) so browsers can
//!   reach it as `wss://signal.example.com/` (a `https://` page cannot open a
//!   plain `ws://` socket). The server speaks plain WS; the proxy adds TLS.
//! - It is stateless apart from in-memory rooms, so it scales horizontally only
//!   if both peers of a match land on the same instance — front it with a
//!   sticky/room-affinity load balancer, or run a single instance (a signaling
//!   server is tiny; one box handles thousands of brief handshakes).
//! - Pair it with a STUN/TURN server (e.g. `coturn`) for the actual NAT
//!   traversal; the signaling server only brokers the handshake.
//!
//! This example is built **only** with `--features signaling-server` (see the
//! crate's `Cargo.toml` `[[example]]` `required-features`), so it never enters
//! the default / wasm / `cargo build --workspace` build.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use rustynes_netplay::signaling::{Action, ClientId, Relay, SignalMessage};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::tungstenite::Message;

/// Shared server state: the pure relay + a per-client outbound channel sender so
/// any task can route an [`Action::Send`] to the right socket.
struct Server {
    relay: Mutex<Relay>,
    /// `client id -> outbound sender`. The per-connection write task drains its
    /// receiver.
    outbound: Mutex<HashMap<ClientId, mpsc::UnboundedSender<SignalMessage>>>,
}

#[tokio::main]
async fn main() {
    let addr: SocketAddr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:9000".to_string())
        .parse()
        .expect("valid listen address (e.g. 0.0.0.0:9000)");

    let listener = TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    println!("signaling server listening on ws://{addr}/");

    let server = Arc::new(Server {
        relay: Mutex::new(Relay::new()),
        outbound: Mutex::new(HashMap::new()),
    });

    // Monotonic client-id allocator.
    let mut next_id: ClientId = 0;
    while let Ok((stream, peer)) = listener.accept().await {
        let id = next_id;
        next_id += 1;
        let server = Arc::clone(&server);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(server, stream, id, peer).await {
                eprintln!("client {id} ({peer}) ended: {e}");
            }
        });
    }
}

/// Drive one WebSocket connection: upgrade, register an outbound channel, then
/// loop reading text frames -> `Relay::handle` -> dispatch actions.
async fn handle_connection(
    server: Arc<Server>,
    stream: TcpStream,
    id: ClientId,
    peer: SocketAddr,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws = tokio_tungstenite::accept_async(stream).await?;
    let (mut write, mut read) = ws.split();
    println!("client {id} connected from {peer}");

    // Per-connection outbound queue: any task routes messages here; this task's
    // forwarder drains them onto the socket.
    let (tx, mut rx) = mpsc::unbounded_channel::<SignalMessage>();
    server.outbound.lock().await.insert(id, tx);

    // Forward queued outbound messages to the socket until the channel closes.
    let forward = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            // tungstenite 0.29: `Message::Text` wraps `Utf8Bytes`, not `String`.
            if write
                .send(Message::Text(msg.to_json().into()))
                .await
                .is_err()
            {
                break;
            }
        }
        let _ = write.close().await;
    });

    // Read inbound frames.
    while let Some(frame) = read.next().await {
        let Ok(frame) = frame else { break };
        match frame {
            Message::Text(txt) => {
                if let Some(msg) = SignalMessage::parse(txt.as_str()) {
                    let actions = server.relay.lock().await.handle(id, msg);
                    dispatch(&server, actions).await;
                }
                // Unparseable frames are dropped (never panic).
            }
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => {}
        }
    }

    // Disconnect: notify the peer, clean up the room, drop the outbound sender.
    let actions = server.relay.lock().await.disconnect(id);
    dispatch(&server, actions).await;
    server.outbound.lock().await.remove(&id);
    forward.abort();
    println!("client {id} disconnected");
    Ok(())
}

/// Perform the relay's [`Action`]s: route each `Send` to the target client's
/// outbound channel; a `Close` drops the channel (its forwarder then closes the
/// socket).
async fn dispatch(server: &Arc<Server>, actions: Vec<Action>) {
    for action in actions {
        match action {
            Action::Send { to, msg } => {
                let outbound = server.outbound.lock().await;
                if let Some(tx) = outbound.get(&to) {
                    let _ = tx.send(msg);
                }
            }
            Action::Close { who } => {
                // Dropping the sender closes the receiver in the forwarder task,
                // which closes the socket.
                server.outbound.lock().await.remove(&who);
            }
        }
    }
}

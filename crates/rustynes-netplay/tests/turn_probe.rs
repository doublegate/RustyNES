//! v1.8.7 TURN integration probe.
//!
//! The TURN framing (`wrap` / `unwrap` of Send/Data indications), the
//! MESSAGE-INTEGRITY HMAC-SHA1, and the long-term-key MD5 are unit-tested
//! headlessly in `src/relay.rs`. This file holds the ONE test that needs a real
//! TURN server (`coturn`) reachable on the network — it is `#[ignore]`d so CI /
//! offline runs stay green, and is run manually to confirm a live allocation
//! against a deployed relay (the `deploy/` bundle's coturn, or any RFC 8656
//! long-term-credential server).
//!
//! Manual run (needs a reachable TURN server + credentials via env):
//!
//! ```text
//! RUSTYNES_TURN_SERVER=turn.example.com:3478 \
//! RUSTYNES_TURN_USER=user RUSTYNES_TURN_PASS=secret \
//!   cargo test -p rustynes-netplay --features netplay-client \
//!   --test turn_probe -- --ignored --nocapture
//! ```
//!
//! It binds an ephemeral UDP socket, runs the long-term-credential `Allocate`
//! transaction, and prints the server-assigned relayed transport address. A
//! failure here is almost always a blocked / offline network or a bad
//! credential, not a code bug — which is why it is not part of the default
//! suite.
//!
//! Native-only + `netplay-client`-gated (it uses [`TurnClient`]); compiles to
//! nothing on wasm32.
#![cfg(all(not(target_arch = "wasm32"), feature = "netplay-client"))]

use std::net::{ToSocketAddrs, UdpSocket};
use std::time::Duration;

use rustynes_netplay::{TurnClient, TurnConfig};

#[test]
#[ignore = "hits a live TURN server; run manually with --ignored + RUSTYNES_TURN_* env"]
fn allocates_a_relayed_address_via_live_turn() {
    let server_str =
        std::env::var("RUSTYNES_TURN_SERVER").expect("set RUSTYNES_TURN_SERVER (host:port)");
    let username = std::env::var("RUSTYNES_TURN_USER").expect("set RUSTYNES_TURN_USER");
    let credential = std::env::var("RUSTYNES_TURN_PASS").expect("set RUSTYNES_TURN_PASS");

    let server = server_str
        .to_socket_addrs()
        .expect("resolve turn server")
        .next()
        .expect("at least one address");

    let socket = UdpSocket::bind("0.0.0.0:0").expect("bind ephemeral udp");
    let cfg = TurnConfig {
        server,
        username,
        credential,
    };

    let turn = TurnClient::allocate(&socket, &cfg, Duration::from_secs(5), 0xC0FF_EE00_1234_5678)
        .expect("TURN allocate (needs network + valid credentials)");

    let relayed = turn
        .relayed_addr()
        .expect("a successful allocation carries a relayed transport address");
    println!("allocated relayed transport address: {relayed}");
    assert_ne!(relayed.port(), 0, "a relayed allocation has a real port");
}

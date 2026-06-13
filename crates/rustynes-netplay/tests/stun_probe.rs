//! v2.5.0 Phase C — STUN integration probe.
//!
//! The STUN encode/decode and the hole-punch state machine are unit-tested
//! headlessly in `src/stun.rs`. This file holds the ONE test that needs a real
//! STUN server and outbound UDP — it is `#[ignore]`d so CI / offline runs stay
//! green, and is run manually to confirm live NAT discovery against a public
//! server.
//!
//! Manual run (needs network access):
//!
//! ```text
//! cargo test -p rustynes-netplay --test stun_probe -- --ignored --nocapture
//! ```
//!
//! It binds an ephemeral UDP socket, sends a Binding Request to Google's public
//! STUN server, and prints the discovered public `SocketAddr`. A failure here is
//! almost always a blocked / offline network, not a code bug — which is exactly
//! why it is not part of the default suite.
//!
//! Native-only (`std::net` UDP + `StunClient`); compiles to nothing on wasm32.
#![cfg(not(target_arch = "wasm32"))]

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

use rustynes_netplay::StunClient;

/// A public STUN server. Resolved at run time (not hardcoded to an IP).
const STUN_SERVER: &str = "stun.l.google.com:19302";

#[test]
#[ignore = "hits a live public STUN server; run manually with --ignored"]
fn discovers_public_address_via_google_stun() {
    let server: SocketAddr = STUN_SERVER
        .to_socket_addrs()
        .expect("resolve stun server")
        .next()
        .expect("at least one address");

    let socket = UdpSocket::bind("0.0.0.0:0").expect("bind ephemeral udp");
    let mut client = StunClient::new(socket, 0xC0FF_EE00_1234_5678);

    let public = client
        .discover(server, Duration::from_secs(3))
        .expect("STUN discovery (needs network)");

    println!("discovered public address: {public}");
    // We can't assert the exact address (it depends on the runner's NAT), but a
    // valid response must carry a non-zero port.
    assert_ne!(public.port(), 0, "a discovered mapping has a real port");
}

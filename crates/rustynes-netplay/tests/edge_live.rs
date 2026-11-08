//! v3.7.0 (`T-NETPLAY-EDGE-SPECTATE`): the live gate. The native publisher
//! and viewer against a RUNNING spectate relay (`wrangler dev` locally, or the
//! deployed Worker): two players run a session in this process, the host
//! publishes its confirmed inputs through the relay, and a viewer that joins
//! mid-session (after 300 confirmed frames) reaches the players' confirmed
//! digest and stays matched for 300 more frames.
//!
//! `#[ignore]`d because it needs the relay: no Rust CI job runs the Worker.
//!
//! ```text
//! cd services/hosted && npx wrangler dev --port 8799 &
//! RUSTYNES_SPECTATE_BASE=http://127.0.0.1:8799 \
//!   cargo test -p rustynes-netplay --test edge_live -- --ignored
//! ```
//!
//! After a deploy, set `RUSTYNES_SPECTATE_BASE` to the hosted base
//! (`https://rustynes-hosted.<sub>.workers.dev`), as
//! `services/hosted/README.md` "Spectate relay: live gates" says.
#![cfg(all(not(target_arch = "wasm32"), feature = "netplay-client"))]

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rustynes_core::{Buttons, Nes};
use rustynes_netplay::{
    EdgeConfig, EdgeEvent, EdgePublisher, EdgeRelayClient, EdgeSpectator, LinkConditions,
    MemoryTransport, RollbackSession, SessionConfig, SessionIdentity, SplitMix64, gameplay_digest,
    spectate_url,
};

fn nestest() -> Vec<u8> {
    let rom = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("tests/roms/nestest/nestest.nes");
    std::fs::read(&rom).unwrap_or_else(|e| panic!("read {}: {e}", rom.display()))
}

fn wait_connected(client: &EdgeRelayClient, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        for ev in client.poll() {
            match ev {
                EdgeEvent::Connected => return,
                EdgeEvent::Closed(why) => panic!("{what}: {why}"),
                EdgeEvent::Message(_) => {}
            }
        }
        assert!(Instant::now() < deadline, "{what}: no connection in 10 s");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "needs a running spectate relay: set RUSTYNES_SPECTATE_BASE"]
fn a_viewer_joining_mid_session_through_the_relay_matches_the_players() {
    let base = std::env::var("RUSTYNES_SPECTATE_BASE").expect("set RUSTYNES_SPECTATE_BASE");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_micros());
    let match_id = format!("live{}", stamp % 1_000_000_000_000);
    let key = format!("{:032x}", stamp ^ 0x5eed_5eed_5eed_5eed_5eed_5eed_5eed_5eed);

    let rom = nestest();
    let mut nes: Vec<Nes> = (0..2).map(|_| Nes::from_rom(&rom).expect("rom")).collect();
    for n in &mut nes {
        rustynes_core::power_on_for_movie(n);
    }
    let id = SessionIdentity::of(&nes[0]);
    let (a, b) = MemoryTransport::pair(LinkConditions::fixed_latency(2), 3);
    let mut sessions = [
        RollbackSession::new(SessionConfig::default(), a, id),
        RollbackSession::new(
            SessionConfig {
                local_player: 1,
                ..SessionConfig::default()
            },
            b,
            id,
        ),
    ];
    let mut rng = [SplitMix64::new(1), SplitMix64::new(2)];
    let mut publisher = EdgePublisher::new(id);
    let relay_pub = EdgeRelayClient::connect(
        &spectate_url(&base, &match_id, Some(&key)).expect("a spectate URL"),
    );
    wait_connected(&relay_pub, "publisher");

    let mut digests: HashMap<u32, u64> = HashMap::new();
    let mut tick = |sessions: &mut [RollbackSession<MemoryTransport>; 2],
                    nes: &mut Vec<Nes>,
                    digests: &mut HashMap<u32, u64>| {
        for p in 0..2 {
            sessions[p].add_local_input(Buttons::from_bits_truncate(rng[p].next_u8() & 0xF0));
            sessions[p].advance(&mut nes[p]).expect("in sync");
        }
        if let Some(c) = sessions[0].last_confirmed_frame() {
            for f in c.saturating_sub(40)..=c {
                if let Some(d) = sessions[0].confirmed_entering_digest(f) {
                    digests.insert(f, d);
                }
            }
        }
        for msg in publisher.poll(&sessions[0]) {
            relay_pub.send(&msg);
        }
        // Real time for the relay to keep up: about 60 frames a second.
        std::thread::sleep(Duration::from_millis(16));
    };
    while sessions[0].last_confirmed_frame().unwrap_or(0) < 300 {
        tick(&mut sessions, &mut nes, &mut digests);
    }

    let viewer_conn = EdgeRelayClient::connect(&spectate_url(&base, &match_id, None).expect("url"));
    let mut viewer = EdgeSpectator::new(id, EdgeConfig::default());
    let mut vnes = Nes::from_rom(&rom).expect("rom");
    let mut matched = 0;
    let deadline = Instant::now() + Duration::from_secs(120);
    while matched < 300 {
        tick(&mut sessions, &mut nes, &mut digests);
        for ev in viewer_conn.poll() {
            match ev {
                EdgeEvent::Message(m) => viewer.on_message(&m),
                EdgeEvent::Closed(why) => panic!("viewer closed: {why}"),
                EdgeEvent::Connected => {}
            }
        }
        assert_eq!(viewer.mismatch(), None);
        for _ in 0..16 {
            if viewer.catch_up(&mut vnes, 1) == 0 {
                break;
            }
            if let Some(&d) = digests.get(&viewer.current_frame()) {
                assert_eq!(
                    gameplay_digest(&vnes),
                    d,
                    "the relayed viewer matches at frame {}",
                    viewer.current_frame()
                );
                matched += 1;
            }
        }
        assert!(
            Instant::now() < deadline,
            "matched only {matched} frames in 120 s"
        );
    }
}

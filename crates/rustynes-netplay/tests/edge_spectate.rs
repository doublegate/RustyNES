//! v3.7.0 `T-NETPLAY-EDGE-SPECTATE` — the plan's Rust gate: a spectator
//! joining mid-session reaches the players' confirmed digest and stays matched
//! for N frames.
//!
//! Two players run a rollback session over the in-memory transport with
//! latency and jitter (so they mispredict and roll back; only the confirmed
//! stream is canonical). The host publishes its confirmed inputs through
//! [`EdgePublisher`] into a byte log standing in for the relay: every frame is
//! encoded and decoded through the wire format, as the relay forwards bytes.
//! The players' confirmed entering digests are recorded as they confirm.
//!
//! - [`a_viewer_joining_mid_session_matches_the_players`]: a viewer joins after
//!   300 confirmed frames, receives the log from frame 0 (the relay's
//!   backlog), catches up, and then matches the players' digest at every frame
//!   it shows for 300 more.
//! - [`a_viewer_joining_from_a_checkpoint_matches_the_players`]: a second
//!   viewer joins from a state checkpoint the first one serves, and matches.
//! - [`seek_and_rewind_land_on_the_players_frames`]: rewound 250 frames and
//!   sought forward again, the viewer's machine is the players' at each.
#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;
use std::path::PathBuf;

use rustynes_core::{Buttons, Nes};
use rustynes_netplay::{
    EdgeConfig, EdgePublisher, EdgeSpectator, LinkConditions, MemoryTransport, NetMessage,
    RollbackSession, SessionConfig, SessionIdentity, SplitMix64, View, gameplay_digest,
};

/// Frames a viewer must stay matched for after it catches up.
const N: u32 = 300;

fn nestest() -> Vec<u8> {
    let rom = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("tests/roms/nestest/nestest.nes");
    std::fs::read(&rom).unwrap_or_else(|e| panic!("read {}: {e}", rom.display()))
}

/// The two players, the host's publisher, the relay's byte log and the
/// players' confirmed digests.
struct Match {
    nes: Vec<Nes>,
    sessions: Vec<RollbackSession<MemoryTransport>>,
    input: Vec<SplitMix64>,
    publisher: EdgePublisher,
    /// Every frame the host published, as the relay holds it: bytes.
    relay: Vec<Vec<u8>>,
    /// Confirmed entering digests, frame -> digest, checked equal across both
    /// players as they arrive.
    digests: HashMap<u32, u64>,
    rollbacks: u32,
}

impl Match {
    fn new(rom: &[u8]) -> Self {
        let mut nes: Vec<Nes> = (0..2).map(|_| Nes::from_rom(rom).expect("rom")).collect();
        for n in &mut nes {
            rustynes_core::power_on_for_movie(n);
        }
        let id = SessionIdentity::of(&nes[0]);
        let link = LinkConditions {
            latency_polls: 2,
            jitter_polls: 2,
            drop_prob: 0.0,
        };
        let (a, b) = MemoryTransport::pair(link, 99);
        let sessions = vec![
            RollbackSession::new(
                SessionConfig {
                    local_player: 0,
                    ..SessionConfig::default()
                },
                a,
                id,
            ),
            RollbackSession::new(
                SessionConfig {
                    local_player: 1,
                    ..SessionConfig::default()
                },
                b,
                id,
            ),
        ];
        Self {
            nes,
            sessions,
            input: vec![SplitMix64::new(5), SplitMix64::new(6)],
            publisher: EdgePublisher::new(id),
            relay: Vec::new(),
            digests: HashMap::new(),
            rollbacks: 0,
        }
    }

    fn identity(&self) -> SessionIdentity {
        SessionIdentity::of(&self.nes[0])
    }

    /// One tick of both players; returns the frames the relay received.
    fn tick(&mut self) -> Vec<Vec<u8>> {
        for p in 0..2 {
            // Held for a few frames at a time, so predictions are often right
            // and sometimes wrong.
            let b = Buttons::from_bits_truncate(self.input[p].next_u8() & 0xF0);
            self.sessions[p].add_local_input(b);
            let out = self.sessions[p].advance(&mut self.nes[p]).expect("in sync");
            self.rollbacks += u32::from(out.rolled_back);
            let s = &self.sessions[p];
            if let Some(c) = s.last_confirmed_frame() {
                for f in c.saturating_sub(40)..=c + 1 {
                    if let Some(d) = s.confirmed_entering_digest(f) {
                        let prev = *self.digests.entry(f).or_insert(d);
                        assert_eq!(prev, d, "the players agree at frame {f}");
                    }
                }
            }
        }
        let fresh: Vec<Vec<u8>> = self
            .publisher
            .poll(&self.sessions[0])
            .iter()
            .map(NetMessage::to_bytes)
            .collect();
        assert!(!self.publisher.lost_frames());
        self.relay.extend(fresh.iter().cloned());
        fresh
    }

    fn confirmed(&self) -> u32 {
        self.sessions[0].last_confirmed_frame().unwrap_or(0)
    }
}

fn deliver(viewer: &mut EdgeSpectator, frames: &[Vec<u8>]) {
    for bytes in frames {
        let msg = NetMessage::from_bytes(bytes).expect("the relay forwards valid frames");
        viewer.on_message(&msg);
    }
}

/// Run `viewer` for one tick: catch up a bounded amount, then check every
/// frame it is at against the players' digest. Returns the frames matched.
fn step_and_check(viewer: &mut EdgeSpectator, nes: &mut Nes, m: &Match) -> u32 {
    let mut matched = 0;
    for _ in 0..8 {
        if viewer.catch_up(nes, 1) == 0 {
            break;
        }
        let f = viewer.current_frame();
        if let Some(&d) = m.digests.get(&f) {
            assert_eq!(
                gameplay_digest(nes),
                d,
                "the viewer matches the players entering frame {f}"
            );
            matched += 1;
        }
    }
    matched
}

/// A count from these small tests as a frame number.
fn n32(x: usize) -> u32 {
    u32::try_from(x).expect("a small count")
}

/// v3.7.0 review (#646, Codex P1): the publisher sent a message for every
/// tick that confirmed a frame, about 60 a second against the relay's 30, and
/// the relay stalled two seconds in. It now sends one per
/// `BATCH_FRAMES` frames, contiguous from frame 0, and `flush` sends the tail.
#[test]
fn the_publisher_sends_one_message_per_batch_and_flushes_the_tail() {
    let rom = nestest();
    let mut m = Match::new(&rom);
    while m.confirmed() < 300 {
        m.tick();
    }
    let batch = n32(EdgePublisher::BATCH_FRAMES);
    let mut next = 0u32;
    let mut inputs_msgs = 0u32;
    for bytes in m.relay.iter().skip(1) {
        let Some(NetMessage::ConfirmedInputs {
            start_frame,
            inputs,
            ..
        }) = NetMessage::from_bytes(bytes)
        else {
            panic!("the relay log is the Sync, then confirmed inputs");
        };
        assert_eq!(start_frame, next, "batches are contiguous from frame 0");
        assert!(
            n32(inputs.len()) >= batch,
            "no batch shorter than BATCH_FRAMES"
        );
        next += n32(inputs.len());
        inputs_msgs += 1;
    }
    let confirmed = m.confirmed() + 1;
    assert!(
        next + batch > confirmed && next <= confirmed,
        "at most a batch waits"
    );
    assert!(
        inputs_msgs <= confirmed / batch,
        "{inputs_msgs} messages for {confirmed} frames: more than one per batch"
    );
    let tail = m.publisher.flush(2).expect("frames were waiting");
    let NetMessage::ConfirmedInputs {
        start_frame,
        inputs,
        ..
    } = tail
    else {
        panic!("flush sends confirmed inputs");
    };
    assert_eq!(
        (start_frame, start_frame + n32(inputs.len())),
        (next, confirmed)
    );
    assert!(
        m.publisher.flush(2).is_none(),
        "nothing is left after a flush"
    );
}

#[test]
fn a_viewer_joining_mid_session_matches_the_players() {
    let rom = nestest();
    let mut m = Match::new(&rom);
    while m.confirmed() < 300 {
        m.tick();
    }
    let mut viewer = EdgeSpectator::new(m.identity(), EdgeConfig::default());
    let mut vnes = Nes::from_rom(&rom).expect("rom");
    // The relay's backlog: everything from frame 0.
    deliver(&mut viewer, &m.relay);
    assert!(viewer.is_synced());
    // Up to `BATCH_FRAMES - 1` confirmed frames wait for their batch.
    assert!(
        viewer.reachable_through() + n32(EdgePublisher::BATCH_FRAMES) > 300,
        "the backlog covers frame 0 on"
    );
    let joined_at = viewer.reachable_through();
    let mut matched = 0;
    let mut ticks = 0;
    while matched < N || viewer.current_frame() < joined_at + N {
        let fresh = m.tick();
        deliver(&mut viewer, &fresh);
        matched += step_and_check(&mut viewer, &mut vnes, &m);
        ticks += 1;
        assert!(ticks < 4_000, "matched {matched} within 4,000 ticks");
    }
    assert!(m.rollbacks > 0, "the players did roll back");
    assert!(viewer.current_frame() >= joined_at + N);
    assert_eq!(viewer.mismatch(), None);
}

#[test]
fn a_viewer_joining_from_a_checkpoint_matches_the_players() {
    let rom = nestest();
    let mut m = Match::new(&rom);
    let mut first = EdgeSpectator::new(m.identity(), EdgeConfig::default());
    let mut nes1 = Nes::from_rom(&rom).expect("rom");
    let mut ticks = 0;
    while first.current_frame() < 400 {
        ticks += 1;
        assert!(ticks < 4_000, "the first viewer reached frame 400");
        let fresh = m.tick();
        deliver(&mut first, &fresh);
        step_and_check(&mut first, &mut nes1, &m);
    }
    // The second viewer gets a checkpoint from the first, then the relay's
    // whole log (inputs before the checkpoint are not needed and are
    // ignored), then the live feed.
    let mut second = EdgeSpectator::from_checkpoint(m.identity(), EdgeConfig::default());
    let mut nes2 = Nes::from_rom(&rom).expect("rom");
    let at = first.current_frame();
    let sync = m.relay[0].clone();
    deliver(&mut second, &[sync]);
    for msg in first.checkpoint(&nes1) {
        second.on_message(&msg);
    }
    deliver(&mut second, &m.relay);
    assert_eq!(second.catch_up(&mut nes2, 0), 0);
    assert_eq!(second.current_frame(), at, "joined at the checkpoint");
    // The players' digest for that frame may confirm a tick later.
    for _ in 0..100 {
        if m.digests.contains_key(&at) {
            break;
        }
        let fresh = m.tick();
        deliver(&mut second, &fresh);
    }
    assert_eq!(gameplay_digest(&nes2), m.digests[&at]);
    let mut matched = 0;
    let mut ticks = 0;
    while matched < 200 {
        let fresh = m.tick();
        deliver(&mut second, &fresh);
        matched += step_and_check(&mut second, &mut nes2, &m);
        ticks += 1;
        assert!(ticks < 4_000, "matched {matched}");
    }
}

#[test]
fn seek_and_rewind_land_on_the_players_frames() {
    let rom = nestest();
    let mut m = Match::new(&rom);
    let config = EdgeConfig {
        keyframe_interval: 60,
        max_keyframes: 64,
    };
    let mut viewer = EdgeSpectator::new(m.identity(), config);
    let mut vnes = Nes::from_rom(&rom).expect("rom");
    let mut ticks = 0;
    while viewer.current_frame() < 600 {
        ticks += 1;
        assert!(ticks < 4_000, "the viewer reached frame 600");
        let fresh = m.tick();
        deliver(&mut viewer, &fresh);
        step_and_check(&mut viewer, &mut vnes, &m);
    }
    let here = viewer.current_frame();
    viewer.rewind(&mut vnes, 250).expect("rewinds");
    assert_eq!(viewer.view(), View::Paused);
    assert_eq!(viewer.current_frame(), here - 250);
    assert_eq!(gameplay_digest(&vnes), m.digests[&(here - 250)]);
    // Paused: advance runs nothing.
    assert_eq!(viewer.advance(&mut vnes), None);
    viewer.seek(&mut vnes, here - 7).expect("seeks");
    assert_eq!(gameplay_digest(&vnes), m.digests[&(here - 7)]);
    assert!(
        viewer
            .seek(&mut vnes, viewer.reachable_through() + 1)
            .is_err()
    );
    // Live again: it resumes from where it is and stays matched.
    viewer.set_view(View::Live);
    let mut checked = 0;
    for _ in 0..20 {
        let f = viewer.current_frame();
        if viewer.advance(&mut vnes).is_some()
            && let Some(&d) = m.digests.get(&(f + 1))
        {
            assert_eq!(gameplay_digest(&vnes), d, "live again at frame {}", f + 1);
            checked += 1;
        }
    }
    assert!(checked >= 5, "{checked} frames checked after resuming");
}

/// The byte layouts the hosted relay (`services/hosted/src/spectate.ts`)
/// classifies, generated from this crate's encoder and pinned in
/// `services/hosted/test/vectors/spectate-vectors.json`, which the Worker's
/// `spectate.test.ts` replays. Fails if the encoder and the committed file
/// disagree; re-record with `RUSTYNES_SPECTATE_VECTORS_BLESS=1`.
#[test]
#[allow(clippy::too_many_lines)] // a table of seven vectors, kept in one place
fn hosted_spectate_vectors_match_the_encoder() {
    use std::fmt::Write as _;
    let id = SessionIdentity::new([7; 32], [9; 32]);
    let hex = |b: &[u8]| {
        b.iter().fold(String::new(), |mut s, x| {
            let _ = write!(s, "{x:02x}");
            s
        })
    };
    let frames: Vec<(&str, Vec<u8>, String)> = vec![
        (
            "sync",
            NetMessage::Sync {
                magic: NetMessage::SYNC_MAGIC,
                identity: id,
                key_share: [0; rustynes_netplay::KEY_SHARE_LEN],
            }
            .to_bytes(),
            r#"{"kind":"sync"}"#.to_string(),
        ),
        (
            "protocol 9 sync",
            NetMessage::Sync {
                magic: NetMessage::PROTOCOL_9_SYNC_MAGIC,
                identity: id,
                key_share: [0; rustynes_netplay::KEY_SHARE_LEN],
            }
            .to_bytes(),
            r#"{"kind":"drop","why":"not a protocol 10 Sync"}"#.to_string(),
        ),
        (
            "two players, three frames",
            NetMessage::ConfirmedInputs {
                num_players: 2,
                start_frame: 0,
                inputs: vec![[1, 2, 0, 0], [3, 4, 0, 0], [5, 6, 0, 0]],
            }
            .to_bytes(),
            r#"{"kind":"inputs","start":0,"count":3,"numPlayers":2}"#.to_string(),
        ),
        (
            "four players, a full batch",
            NetMessage::ConfirmedInputs {
                num_players: 4,
                start_frame: 0,
                inputs: vec![[0x80, 0x40, 0x20, 0x10]; NetMessage::MAX_CONFIRMED_BATCH],
            }
            .to_bytes(),
            r#"{"kind":"inputs","start":0,"count":64,"numPlayers":4}"#.to_string(),
        ),
        (
            "a gap",
            NetMessage::ConfirmedInputs {
                num_players: 2,
                start_frame: 5,
                inputs: vec![[1, 1, 0, 0]],
            }
            .to_bytes(),
            r#"{"kind":"drop","why":"gap"}"#.to_string(),
        ),
        (
            "a state checkpoint",
            NetMessage::StateCheckpoint {
                frame: 0,
                total: 4,
                offset: 0,
                data: vec![1, 2, 3, 4],
            }
            .to_bytes(),
            r#"{"kind":"drop","why":"tag 11 is not relayed"}"#.to_string(),
        ),
        (
            "a checksum",
            NetMessage::Checksum {
                frame: 30,
                hash: 1,
                fb_hash: 2,
            }
            .to_bytes(),
            r#"{"kind":"drop","why":"tag 3 is not relayed"}"#.to_string(),
        ),
    ];
    let mut json = String::from("{\n  \"frames\": [\n");
    for (i, (name, bytes, verdict)) in frames.iter().enumerate() {
        let comma = if i + 1 == frames.len() { "" } else { "," };
        let _ = writeln!(
            json,
            "    {{ \"name\": \"{name}\", \"hex\": \"{}\", \"verdict\": {verdict} }}{comma}",
            hex(bytes)
        );
    }
    json.push_str("  ]\n}\n");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("services/hosted/test/vectors/spectate-vectors.json");
    if std::env::var_os("RUSTYNES_SPECTATE_VECTORS_BLESS").is_some() {
        std::fs::write(&path, &json).expect("write vectors");
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        committed, json,
        "re-record with RUSTYNES_SPECTATE_VECTORS_BLESS=1"
    );
}

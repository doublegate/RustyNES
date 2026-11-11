//! v3.7.0 `T-NETPLAY-DESYNC-FORENSICS` — the plan's gate: a planted one-byte
//! divergence in a loopback session is reported at the right subsystem,
//! address and frame.
//!
//! Two peers run a session over the in-memory transport on a perfect link
//! (so nothing mispredicts and no rollback can replay the planted byte away).
//! After frame `PLANT_AFTER` runs, one byte of player 2's CPU RAM is changed
//! (`Nes::poke_ram`, the raw-RAM-cheat path). The fixture ROM never touches
//! that address, so the picture never shows it and the gameplay checksum never
//! fires: only forensics can find it. The window entry for the frame being
//! entered was recorded before the write, so the first frame whose ENTERING
//! state differs is the one after it, which is what the report must name.
//!
//! - [`a_planted_ram_byte_is_found_at_its_frame_and_address`]: both peers keep
//!   advancing and both reports name `BUS `, CPU RAM `$0300`, the frame, and
//!   the two byte values (each from its own side).
//! - [`the_exchange_finishes_after_the_session_stops_advancing`]: the peers
//!   stop advancing as soon as the divergence is found (as after a fatal
//!   desync) and finish through `pump_forensics` alone.
//! - [`a_peer_that_did_not_opt_in_sends_no_state`]: forensics is opt-in.
#![cfg(not(target_arch = "wasm32"))]

use rustynes_core::{Buttons, Nes};
use rustynes_netplay::{
    ForensicsConfig, ForensicsStatus, LinkConditions, MemoryTransport, RollbackSession,
    SessionConfig, SessionIdentity, SplitMix64, StateLocation,
};

/// The planted byte's address. The fixture uses only `$00-$03` and the stack.
const ADDR: u16 = 0x0300;
/// The planted value, combined by exclusive or with what the byte held.
const FLIP: u8 = 0xA5;
/// Frames produced (`current_frame` after the call) when the byte is planted.
const PLANT_AFTER: u32 = 90;

/// A ROM that reads both controllers into `$00-$03` and paints the backdrop
/// from them every frame (public-domain by construction; the same program as
/// `mesh_sync.rs`'s `four_score_probe_rom`).
fn probe_rom() -> Vec<u8> {
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
    for v in [0x3FFA, 0x3FFC, 0x3FFE] {
        prg[v] = 0x00;
        prg[v + 1] = 0xC0;
    }
    rom.extend_from_slice(&prg);
    rom.extend_from_slice(&[0u8; 8 * 1024]);
    rom
}

struct Peers {
    nes: Vec<Nes>,
    sessions: [RollbackSession<MemoryTransport>; 2],
    input: [SplitMix64; 2],
    /// The planted byte's value before and after, for the assertions.
    planted: Option<(u8, u8)>,
}

impl Peers {
    fn new(opt_in: [bool; 2]) -> Self {
        let rom = probe_rom();
        let nes = vec![
            Nes::from_rom(&rom).expect("rom"),
            Nes::from_rom(&rom).expect("rom"),
        ];
        let id = SessionIdentity::of(&nes[0]);
        let (ta, tb) = MemoryTransport::pair(LinkConditions::PERFECT, 7);
        let cfg = SessionConfig::default();
        let mut sessions = [
            RollbackSession::new(
                SessionConfig {
                    local_player: 0,
                    ..cfg
                },
                ta,
                id,
            ),
            RollbackSession::new(
                SessionConfig {
                    local_player: 1,
                    ..cfg
                },
                tb,
                id,
            ),
        ];
        for (s, on) in sessions.iter_mut().zip(opt_in) {
            if on {
                s.enable_forensics(ForensicsConfig::default());
            }
        }
        Self {
            nes,
            sessions,
            input: [SplitMix64::new(11), SplitMix64::new(23)],
            planted: None,
        }
    }

    /// One tick of each peer; plants the byte once player 2 has produced
    /// `PLANT_AFTER` frames.
    fn tick(&mut self) {
        for p in 0..2 {
            let b = Buttons::from_bits_truncate(self.input[p].next_u8());
            self.sessions[p].add_local_input(b);
            let out = self.sessions[p]
                .advance(&mut self.nes[p])
                .expect("the planted byte never reaches the gameplay checksum");
            assert!(!out.rolled_back, "a perfect link never rolls back");
        }
        if self.planted.is_none() && self.sessions[1].current_frame() == PLANT_AFTER {
            let before = self.nes[1].peek(ADDR);
            let after = before ^ FLIP;
            self.nes[1].poke_ram(ADDR, after);
            self.planted = Some((before, after));
        }
    }

    fn reported(&self) -> bool {
        self.sessions.iter().all(|s| s.forensics_report().is_some())
    }

    fn assert_reports(&self) {
        let (before, after) = self.planted.expect("planted");
        for (p, s) in self.sessions.iter().enumerate() {
            assert_eq!(s.forensics_status(), ForensicsStatus::Reported, "peer {p}");
            let r = s.forensics_report().expect("report");
            assert_eq!(
                r.frame,
                PLANT_AFTER + 1,
                "peer {p}: the first frame entered from the planted state"
            );
            assert_eq!(
                r.peer,
                1 - u8::try_from(p).expect("two"),
                "against the other"
            );
            assert_eq!(
                r.sections.len(),
                1,
                "peer {p}: only one section differs: {r}"
            );
            let d = r.primary().expect("a section");
            assert_eq!(&d.tag, b"BUS ", "peer {p}: {r}");
            assert_eq!(d.location, StateLocation::CpuRam { addr: ADDR }, "{r}");
            assert_eq!(d.differing_bytes, 1, "{r}");
            assert_eq!(d.digest_only, None);
            let (ours, theirs) = if p == 1 {
                (after, before)
            } else {
                (before, after)
            };
            assert_eq!((d.local, d.remote), (Some(ours), Some(theirs)), "{r}");
            let text = r.to_string();
            assert!(text.contains("CPU RAM $0300"), "{text}");
            assert!(
                text.contains(&format!("frame {}", PLANT_AFTER + 1)),
                "{text}"
            );
        }
    }
}

#[test]
fn a_planted_ram_byte_is_found_at_its_frame_and_address() {
    let mut peers = Peers::new([true, true]);
    let mut ticks = 0;
    while !peers.reported() {
        peers.tick();
        ticks += 1;
        assert!(ticks < 600, "reported within 600 ticks");
    }
    peers.assert_reports();
    // The report is final: more frames change nothing.
    let before = peers.sessions[0].forensics_report().cloned();
    for _ in 0..30 {
        peers.tick();
    }
    assert_eq!(peers.sessions[0].forensics_report().cloned(), before);
}

#[test]
fn the_exchange_finishes_after_the_session_stops_advancing() {
    let mut peers = Peers::new([true, true]);
    let mut ticks = 0;
    // Advance only until either side has found the divergence.
    while peers
        .sessions
        .iter()
        .all(|s| matches!(s.forensics_status(), ForensicsStatus::Watching { .. }))
    {
        peers.tick();
        ticks += 1;
        assert!(ticks < 600, "found within 600 ticks");
    }
    let mut pumps = 0;
    while !peers.reported() {
        for s in &mut peers.sessions {
            s.pump_forensics();
        }
        pumps += 1;
        assert!(pumps < 200, "finished within 200 pumps");
    }
    peers.assert_reports();
}

#[test]
fn a_peer_that_did_not_opt_in_sends_no_state() {
    let mut peers = Peers::new([true, false]);
    for _ in 0..300 {
        peers.tick();
    }
    assert!(peers.planted.is_some());
    assert_eq!(
        peers.sessions[0].forensics_status(),
        ForensicsStatus::Watching {
            compared_through: None
        },
        "player 2 sent no digest, so nothing was compared"
    );
    assert_eq!(peers.sessions[1].forensics_status(), ForensicsStatus::Off);
    assert!(peers.sessions[0].forensics_report().is_none());
}

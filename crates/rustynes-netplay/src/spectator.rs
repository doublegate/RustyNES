//! Read-only **spectator** session (v1.7.0 "Forge" Workstream H8).
//!
//! A spectator is a determinism-safe, *receive-only* extension of the rollback
//! stack: it joins a running netplay match purely to watch. It **never authors
//! or sends input** — it ingests the players' confirmed input stream off the
//! transport and replays it into a local [`Nes`], one frame at a time, the
//! moment every player's real input for that frame is known.
//!
//! # Why this is determinism-safe
//!
//! The cross-peer determinism contract (`same ROM + seed + input ⇒
//! byte-identical state`, see `rustynes-core`) is exactly what a spectator
//! relies on. The active players, when they reach a *confirmed* frame, have all
//! played it from real inputs only — so the spectator, replaying those same
//! confirmed inputs from the same deterministic cold-boot, reproduces every
//! frame byte-for-byte. Crucially:
//!
//! - The spectator **predicts nothing** and therefore **never rolls back**. It
//!   only ever advances a frame once that frame is fully confirmed, so there is
//!   no speculative state to mispredict. This is strictly simpler than (and a
//!   subset of) the player-side [`RollbackSession`](crate::RollbackSession)
//!   algorithm.
//! - It draws no randomness, reads no wall clock, and feeds nothing back into
//!   the players' session. The transport is used **poll-only** (the spectator
//!   `send`s nothing), so it cannot perturb the match it is watching.
//!
//! The net effect on the existing 2-4 player rollback path is *zero*: a
//! spectator is invisible to the players (it sends no datagrams), and the
//! players' [`RollbackSession`](crate::RollbackSession) already drops any
//! unexpected / foreign packet.
//!
//! # Lag-behind, never ahead
//!
//! A spectator runs `input_delay + network-latency` frames behind the live
//! match (it can only show a frame once it has *received* every player's input
//! for it). [`SpectatorSession::pending_frames`] reports how many fully-confirmed
//! frames are buffered but not yet shown, so the frontend can fast-forward to
//! catch up when it falls behind.

use rustynes_core::{Buttons, Nes};

use crate::message::NetMessage;
use crate::session::MAX_PLAYERS;
use crate::transport::Transport;

/// How far ahead of the current confirmed/horizon frame a peer-supplied
/// `Input.frame` may legitimately be before we reject it.
///
/// A spectator only ever shows fully-confirmed frames, lagging the live match
/// by `input_delay + network-latency` frames; it never predicts. So the
/// newest in-flight `Input.frame` it can plausibly receive sits a small,
/// bounded distance ahead of the frame it is currently confirming. We allow a
/// generous window — comfortably larger than any player's `max_rollback_frames`
/// (default 8) plus jitter/reorder slack — but cap it so a malicious or
/// corrupt peer cannot drive [`SpectatorSession::ensure_frame`] into an
/// unbounded `Vec` resize (an OOM `DoS`). A frame beyond this horizon is simply
/// dropped (mirrors the beta.4 movie-parser bounds hardening).
const MAX_SPECTATOR_FRAME_LOOKAHEAD: u32 = 1024;

/// Configuration for a [`SpectatorSession`].
#[derive(Clone, Copy, Debug)]
pub struct SpectatorConfig {
    /// How many players are in the match being watched (2..=4). Used to know
    /// when a frame is fully confirmed (all `num_players` inputs present) and
    /// whether to enable the Four Score adapter. Defaults to `2`.
    pub num_players: u8,
    /// **Delayed-stream buffer depth**, in frames. A spectator already lags the
    /// live match by `input_delay + network-latency` frames (it can only show a
    /// frame it has fully received); `delay_frames` adds a *further* intentional
    /// hold so the spectator only reveals frame `f` once frame `f + delay_frames`
    /// is also confirmed. Defaults to `0` (show as soon as confirmed).
    ///
    /// # Why an extra delay
    ///
    /// - **Anti-spoiler / broadcast delay.** A tournament stream commonly runs a
    ///   spectator several seconds behind so a caster (or a co-spectator on the
    ///   same feed) cannot leak an imminent input to a player.
    /// - **Jitter smoothing.** Holding a small backlog of confirmed frames lets
    ///   the frontend present at a steady cadence even when confirmations arrive
    ///   bursty over a lossy relay, instead of stalling then fast-forwarding.
    ///
    /// This is purely a *presentation* delay: the emulated frames are still
    /// produced byte-identically and in order — only the moment each is revealed
    /// moves later. It never sends anything, so it cannot perturb the match. The
    /// value is clamped to [`SpectatorConfig::MAX_DELAY_FRAMES`] on use so it can
    /// never push the reveal point past the bounded lookahead window.
    pub delay_frames: u32,
}

impl SpectatorConfig {
    /// Upper bound on [`delay_frames`](Self::delay_frames). Kept comfortably
    /// below `MAX_SPECTATOR_FRAME_LOOKAHEAD` so the buffered-but-unshown
    /// backlog always fits inside the frames the session will accept, and a
    /// misconfigured huge delay cannot wedge the spectator permanently behind
    /// the horizon. 512 frames ≈ 8.5 s at 60 Hz — ample for a broadcast delay.
    pub const MAX_DELAY_FRAMES: u32 = 512;
}

impl Default for SpectatorConfig {
    fn default() -> Self {
        Self {
            num_players: 2,
            delay_frames: 0,
        }
    }
}

/// One frame's confirmed per-player input, plus which players have arrived.
#[derive(Clone, Copy, Debug, Default)]
struct FrameInputs {
    /// One cell per player index (only `0..num_players` are meaningful).
    inputs: [u8; MAX_PLAYERS],
    /// Bit `p` set once player `p`'s real input for this frame has arrived.
    arrived: u8,
}

/// What a single [`SpectatorSession::advance`] did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SpectatorOutcome {
    /// `true` if a frame was produced this tick. `false` means the spectator is
    /// waiting for the next fully-confirmed frame's inputs to arrive (the
    /// caller should skip rendering this tick — it would re-present the same
    /// picture).
    pub produced_frame: bool,
    /// The frame index just produced (only meaningful when `produced_frame`).
    pub frame: u32,
}

/// A read-only spectator that replays a match's confirmed input stream into a
/// local [`Nes`].
///
/// Construct with [`Self::new`], then call [`Self::advance`] once per visual
/// frame. The session polls the transport for [`NetMessage::Input`] (every
/// player's stream) + [`NetMessage::Sync`] (handshake) + [`NetMessage::Roster`]
/// (the player count, when relayed) and **never sends anything**.
pub struct SpectatorSession<T: Transport> {
    config: SpectatorConfig,
    transport: T,
    rom_hash: [u8; 32],

    /// The next frame to be produced (== number of frames shown so far).
    current_frame: u32,
    /// Newest frame for which ALL players' real inputs have arrived. `None`
    /// before the first complete frame.
    last_confirmed_frame: Option<u32>,
    /// `true` once a peer's `Sync` handshake (matching ROM) has been seen. A
    /// spectator validates but does not answer it.
    synced: bool,

    /// Per-frame confirmed input history, indexed by frame. Append-only.
    history: Vec<FrameInputs>,
}

impl<T: Transport> SpectatorSession<T> {
    /// Create a read-only spectator for `rom_hash` (from [`Nes::rom_sha256`]).
    ///
    /// Unlike [`RollbackSession::new`](crate::RollbackSession::new) this sends
    /// **no** opening handshake — a spectator is invisible to the match. The
    /// caller is responsible for joining a stream that carries every player's
    /// `Input` (e.g. a relay/broadcast endpoint, or a `MeshTransport` leg that
    /// receives all peers).
    ///
    /// # Panics
    ///
    /// Panics in debug builds if `config.num_players` is not in `2..=4`.
    #[must_use]
    pub fn new(config: SpectatorConfig, transport: T, rom_hash: [u8; 32]) -> Self {
        debug_assert!(
            (2..=4).contains(&config.num_players),
            "num_players must be 2..=4"
        );
        Self {
            config,
            transport,
            rom_hash,
            current_frame: 0,
            last_confirmed_frame: None,
            synced: false,
            history: Vec::new(),
        }
    }

    /// The frame the spectator will show next.
    #[must_use]
    pub const fn current_frame(&self) -> u32 {
        self.current_frame
    }

    /// Newest fully-confirmed frame (all players' inputs arrived), if any.
    #[must_use]
    pub const fn last_confirmed_frame(&self) -> Option<u32> {
        self.last_confirmed_frame
    }

    /// Number of players in the watched match (2..=4).
    #[must_use]
    pub const fn num_players(&self) -> u8 {
        self.config.num_players
    }

    /// The configured delayed-stream buffer depth, clamped to
    /// [`SpectatorConfig::MAX_DELAY_FRAMES`]. See
    /// [`SpectatorConfig::delay_frames`].
    #[must_use]
    pub const fn delay_frames(&self) -> u32 {
        let d = self.config.delay_frames;
        if d > SpectatorConfig::MAX_DELAY_FRAMES {
            SpectatorConfig::MAX_DELAY_FRAMES
        } else {
            d
        }
    }

    /// The newest frame the spectator is currently permitted to *reveal*: the
    /// confirmed horizon pulled back by [`delay_frames`](Self::delay_frames).
    /// `None` until enough frames past the delay have been confirmed.
    #[must_use]
    fn reveal_horizon(&self) -> Option<u32> {
        self.last_confirmed_frame
            .and_then(|c| c.checked_sub(self.delay_frames()))
    }

    /// `true` once a `Sync` with the matching ROM has been observed.
    #[must_use]
    pub const fn is_synced(&self) -> bool {
        self.synced
    }

    /// How many fully-confirmed frames are buffered but not yet shown — i.e.
    /// how far the spectator is *behind* the live match. The frontend can
    /// fast-forward (call [`Self::advance`] repeatedly) to catch up.
    #[must_use]
    pub fn pending_frames(&self) -> u32 {
        match self.reveal_horizon() {
            Some(h) if h >= self.current_frame => h - self.current_frame + 1,
            _ => 0,
        }
    }

    /// Borrow the transport (e.g. to inspect link stats). Mainly for tests.
    #[must_use]
    pub const fn transport(&self) -> &T {
        &self.transport
    }

    /// Advance one visual frame, if the next frame is fully confirmed.
    ///
    /// Polls the transport (folding in every player's `Input`, validating
    /// `Sync`, adopting a `Roster`'s player count), then — only if every
    /// player's real input for [`current_frame`](Self::current_frame) has
    /// arrived — applies those inputs and runs exactly one emulator frame.
    /// Otherwise it produces nothing and waits.
    ///
    /// A spectator never sends, predicts, or rolls back, so this returns no
    /// error: a malformed / foreign packet is simply ignored by the transport
    /// layer, and a ROM mismatch on the observed `Sync` only leaves `synced`
    /// false (the caller can surface that via [`is_synced`](Self::is_synced)).
    pub fn advance(&mut self, nes: &mut Nes) -> SpectatorOutcome {
        self.ingest();
        self.recompute_confirmed();

        // Show the next frame only once every player's real input is known AND
        // it sits at or behind the (optionally delayed) reveal horizon. With
        // `delay_frames == 0` this is exactly "as soon as confirmed"; with a
        // positive delay the frame is held until `frame + delay_frames` has also
        // been confirmed (the delayed-stream / broadcast-delay buffer).
        let frame = self.current_frame;
        let ready = self.reveal_horizon().is_some_and(|h| frame <= h);
        if !ready {
            return SpectatorOutcome::default();
        }

        self.apply_and_run(nes, frame);
        self.current_frame += 1;
        SpectatorOutcome {
            produced_frame: true,
            frame,
        }
    }

    /// Poll the transport and fold in every relevant message. Receive-only: no
    /// ack, checksum, or quality reply is ever sent.
    fn ingest(&mut self) {
        let messages = self.transport.poll();
        for msg in messages {
            match msg {
                NetMessage::Sync { magic, rom_hash } => {
                    if magic == NetMessage::SYNC_MAGIC && rom_hash == self.rom_hash {
                        self.synced = true;
                    }
                }
                NetMessage::Input {
                    player,
                    frame,
                    input,
                } => {
                    // Drop an out-of-range player index. `num_players` is fixed
                    // by construction (and only ever set by a `Roster` BEFORE
                    // the first confirmed frame — see below), so a `player`
                    // beyond it can never become valid: dropping it is correct,
                    // not merely a best-effort guard, and it keeps a malformed /
                    // foreign packet from indexing out of bounds or corrupting a
                    // real player's stream.
                    if player >= self.config.num_players {
                        continue;
                    }
                    // Reject a `frame` that sits implausibly far ahead of the
                    // frame we are currently confirming. A spectator never shows
                    // anything beyond the confirmed horizon, so any legitimate
                    // in-flight `Input.frame` is only a small bounded distance
                    // ahead. Without this cap a peer-supplied `frame` near
                    // `u32::MAX` would make `ensure_frame` resize `history`
                    // unboundedly (an OOM DoS). Dropping it is safe: a real,
                    // in-window frame is retransmitted by the players' session.
                    let horizon = self.last_confirmed_frame.map_or(0, |c| c + 1);
                    if frame > horizon.saturating_add(MAX_SPECTATOR_FRAME_LOOKAHEAD) {
                        continue;
                    }
                    self.ensure_frame(frame);
                    let slot = &mut self.history[frame as usize];
                    slot.inputs[player as usize] = input;
                    slot.arrived |= 1 << player;
                }
                NetMessage::Roster { peers } => {
                    // The host's roster tells the spectator how many players to
                    // expect; clamp into the valid range. (A relay that fans the
                    // match to spectators forwards this.) `peers.len()` is bounded
                    // by `MAX_ROSTER` (4) on the wire, so the cast cannot truncate.
                    //
                    // Only honor a roster BEFORE any frame has been produced or
                    // confirmed. Changing `num_players` after frames are
                    // confirmed would retroactively re-define what "fully
                    // confirmed" means without re-walking the existing prefix,
                    // which could silently un-confirm already-shown frames.
                    // Once the match's player count is locked in, a later roster
                    // is stale relay chatter and is ignored.
                    if self.current_frame == 0 && self.last_confirmed_frame.is_none() {
                        let n = u8::try_from(peers.len()).unwrap_or(4);
                        self.config.num_players = n.clamp(2, 4);
                    }
                }
                // A spectator ignores acks (it sends no input to ack), peer
                // checksums (it does not participate in desync detection), and
                // quality hints (it never stalls the players).
                NetMessage::InputAck { .. }
                | NetMessage::Checksum { .. }
                | NetMessage::Quality { .. } => {}
            }
        }
    }

    /// Grow the history so index `frame` is addressable.
    fn ensure_frame(&mut self, frame: u32) {
        let need = frame as usize + 1;
        if self.history.len() < need {
            self.history.resize(need, FrameInputs::default());
        }
    }

    /// Recompute `last_confirmed_frame` = the newest frame, contiguously from
    /// the current confirmed prefix, for which every player's input arrived.
    fn recompute_confirmed(&mut self) {
        let n = self.config.num_players;
        let all = if n >= 8 { u8::MAX } else { (1u8 << n) - 1 };
        let start = self.last_confirmed_frame.map_or(0, |c| c + 1);
        // Frame indices are addressed as `u32` on the wire (`NetMessage::Input`'s
        // `frame`), so a history longer than `u32::MAX` is impossible; saturate
        // for the bound rather than cast-truncate.
        let len = u32::try_from(self.history.len()).unwrap_or(u32::MAX);
        let mut confirmed = self.last_confirmed_frame;
        for f in start..len {
            if self.history[f as usize].arrived & all == all {
                confirmed = Some(f);
            } else {
                break;
            }
        }
        self.last_confirmed_frame = confirmed;
    }

    /// Apply every player's confirmed input for `frame` and run one emulator
    /// frame. Mirrors `RollbackSession::apply_and_run` so the spectator's
    /// per-port routing + Four Score gating are byte-identical to the players'.
    fn apply_and_run(&self, nes: &mut Nes, frame: u32) {
        let slot = self.history[frame as usize];
        let n = self.config.num_players as usize;
        nes.set_four_score(n > 2);
        for (port, &input) in slot.inputs.iter().enumerate().take(n) {
            nes.set_buttons(port, Buttons::from_bits_truncate(input));
        }
        let _ = nes.run_frame();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{LinkConditions, MemoryTransport};

    // A minimal NROM (infinite loop) so a session can advance frames without a
    // real game. Mirrors the session/movie_ui test fixtures.
    fn synth_nrom() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"NES\x1A");
        bytes.push(1);
        bytes.push(1);
        bytes.push(0);
        bytes.push(0);
        bytes.extend_from_slice(&[0u8; 8]);
        let mut prg = vec![0u8; 16 * 1024];
        prg[0] = 0x4C;
        prg[1] = 0x00;
        prg[2] = 0xC0;
        let len = prg.len();
        prg[len - 4] = 0x00;
        prg[len - 3] = 0xC0;
        prg[len - 6] = 0x00;
        prg[len - 5] = 0xC0;
        prg[len - 2] = 0x00;
        prg[len - 1] = 0xC0;
        bytes.extend_from_slice(&prg);
        bytes.extend_from_slice(&vec![0u8; 8 * 1024]);
        bytes
    }

    #[test]
    fn spectator_waits_until_confirmed() {
        let rom = synth_nrom();
        let hash = *Nes::from_rom(&rom).unwrap().rom_sha256();
        let (a, _b) = MemoryTransport::pair(LinkConditions::PERFECT, 1);
        let mut spec = SpectatorSession::new(SpectatorConfig::default(), a, hash);
        let mut nes = Nes::from_rom(&rom).unwrap();
        // No inputs received yet: nothing to show.
        let out = spec.advance(&mut nes);
        assert!(!out.produced_frame);
        assert_eq!(spec.current_frame(), 0);
        assert_eq!(spec.pending_frames(), 0);
    }

    /// A delayed-stream spectator holds each confirmed frame until `delay_frames`
    /// *further* frames are also confirmed, then reveals frames in order and
    /// byte-identically to a no-delay spectator (the delay is presentation-only).
    #[test]
    fn spectator_delay_buffer_holds_then_reveals() {
        const DELAY: u32 = 3;
        let rom = synth_nrom();
        let hash = *Nes::from_rom(&rom).unwrap().rom_sha256();
        let (spec_link, mut feeder) = MemoryTransport::pair(LinkConditions::PERFECT, 7);
        let mut spec = SpectatorSession::new(
            SpectatorConfig {
                num_players: 2,
                delay_frames: DELAY,
            },
            spec_link,
            hash,
        );
        let mut nes = Nes::from_rom(&rom).unwrap();

        // Confirm frames 0..=2 (fewer than DELAY past frame 0): nothing reveals.
        for f in 0..DELAY {
            for player in 0..2 {
                feeder.send(&NetMessage::Input {
                    player,
                    frame: f,
                    input: 0,
                });
            }
        }
        for _ in 0..DELAY {
            assert!(
                !spec.advance(&mut nes).produced_frame,
                "nothing reveals until delay_frames past frame 0 are confirmed"
            );
        }
        assert_eq!(spec.pending_frames(), 0, "reveal horizon not reached yet");

        // Confirm frame 3 (== frame 0 + DELAY): frame 0 may now be revealed.
        for player in 0..2 {
            feeder.send(&NetMessage::Input {
                player,
                frame: DELAY,
                input: 0,
            });
        }
        let out = spec.advance(&mut nes);
        assert!(
            out.produced_frame,
            "frame 0 reveals once frame DELAY confirmed"
        );
        assert_eq!(out.frame, 0);
        assert_eq!(spec.delay_frames(), DELAY);
    }

    /// An absurd `delay_frames` is clamped to `MAX_DELAY_FRAMES`, so it cannot
    /// push the reveal point past the accept window.
    #[test]
    fn spectator_delay_is_clamped() {
        let rom = synth_nrom();
        let hash = *Nes::from_rom(&rom).unwrap().rom_sha256();
        let (a, _b) = MemoryTransport::pair(LinkConditions::PERFECT, 1);
        let spec = SpectatorSession::new(
            SpectatorConfig {
                num_players: 2,
                delay_frames: u32::MAX,
            },
            a,
            hash,
        );
        assert_eq!(spec.delay_frames(), SpectatorConfig::MAX_DELAY_FRAMES);
    }

    /// The load-bearing determinism-safety property: a spectator fed the SAME
    /// confirmed per-player input stream reaches a **byte-identical
    /// framebuffer** to a reference `Nes` run directly over those inputs. This
    /// is exactly the cross-peer determinism the players' rollback session
    /// relies on, exercised through the receive-only spectator path.
    /// A peer-supplied `Input.frame` far beyond the confirmed horizon must be
    /// dropped WITHOUT growing `history` (otherwise a `frame` near `u32::MAX`
    /// would resize the `Vec` unboundedly — an OOM `DoS`). The in-window frame
    /// that follows is still accepted.
    #[test]
    fn spectator_rejects_out_of_window_frame_without_allocating() {
        let rom = synth_nrom();
        let hash = *Nes::from_rom(&rom).unwrap().rom_sha256();
        let (spec_link, mut feeder) = MemoryTransport::pair(LinkConditions::PERFECT, 7);
        let mut spec = SpectatorSession::new(
            SpectatorConfig {
                num_players: 2,
                delay_frames: 0,
            },
            spec_link,
            hash,
        );
        let mut nes = Nes::from_rom(&rom).unwrap();

        // An absurd frame index (near u32::MAX) for a valid player. The horizon
        // starts at 0, so this is far past MAX_SPECTATOR_FRAME_LOOKAHEAD.
        feeder.send(&NetMessage::Input {
            player: 0,
            frame: u32::MAX - 5,
            input: 0xFF,
        });
        feeder.send(&NetMessage::Input {
            player: 1,
            frame: u32::MAX,
            input: 0x0F,
        });
        let out = spec.advance(&mut nes);
        assert!(!out.produced_frame, "out-of-window frames produce nothing");
        assert!(
            spec.history.len() <= MAX_SPECTATOR_FRAME_LOOKAHEAD as usize + 1,
            "history must not be resized to an attacker-chosen frame index (len = {})",
            spec.history.len()
        );

        // A legitimate in-window frame is still accepted (both players),
        // confirming frame 0 and letting the spectator show it.
        feeder.send(&NetMessage::Input {
            player: 0,
            frame: 0,
            input: 0,
        });
        feeder.send(&NetMessage::Input {
            player: 1,
            frame: 0,
            input: 0,
        });
        let out = spec.advance(&mut nes);
        assert!(out.produced_frame, "in-window frame 0 is shown");
        assert_eq!(out.frame, 0);
    }

    /// A `Roster` that arrives AFTER a frame has been confirmed/produced must be
    /// ignored — applying it could retroactively un-confirm already-shown frames
    /// (the confirmed prefix is not re-walked). A `Roster` before any frame is
    /// honored.
    #[test]
    fn spectator_ignores_late_roster() {
        use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
        let addr = |p: u8| -> (u8, SocketAddr) {
            (
                p,
                SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 5000 + u16::from(p))),
            )
        };
        let rom = synth_nrom();
        let hash = *Nes::from_rom(&rom).unwrap().rom_sha256();
        let (spec_link, mut feeder) = MemoryTransport::pair(LinkConditions::PERFECT, 7);
        let mut spec = SpectatorSession::new(
            SpectatorConfig {
                num_players: 2,
                delay_frames: 0,
            },
            spec_link,
            hash,
        );
        let mut nes = Nes::from_rom(&rom).unwrap();

        // Confirm + show frame 0 (a 2-player match).
        feeder.send(&NetMessage::Input {
            player: 0,
            frame: 0,
            input: 0,
        });
        feeder.send(&NetMessage::Input {
            player: 1,
            frame: 0,
            input: 0,
        });
        assert!(spec.advance(&mut nes).produced_frame);
        assert_eq!(spec.num_players(), 2);

        // A late roster claiming 4 players must be ignored.
        feeder.send(&NetMessage::Roster {
            peers: vec![addr(0), addr(1), addr(2), addr(3)],
        });
        let _ = spec.advance(&mut nes);
        assert_eq!(
            spec.num_players(),
            2,
            "a roster after a confirmed frame is ignored"
        );
    }

    #[test]
    fn spectator_matches_reference_framebuffer() {
        const FRAMES: usize = 24;
        let rom = synth_nrom();
        let hash = *Nes::from_rom(&rom).unwrap().rom_sha256();

        // A deterministic per-frame input script for both players (a NROM
        // infinite loop ignores it, but the routing path is still exercised
        // byte-for-byte, which is what matters for the contract).
        let p0_script: [u8; FRAMES] =
            core::array::from_fn(|i| u8::try_from(i % 256).unwrap().wrapping_mul(3));
        let p1_script: [u8; FRAMES] =
            core::array::from_fn(|i| u8::try_from(i % 256).unwrap().wrapping_mul(5) ^ 0x11);

        // Reference: a single Nes fed the inputs with NO networking.
        let mut reference = Nes::from_rom(&rom).unwrap();
        reference.power_cycle();
        for f in 0..FRAMES {
            reference.set_four_score(false);
            reference.set_buttons(0, Buttons::from_bits_truncate(p0_script[f]));
            reference.set_buttons(1, Buttons::from_bits_truncate(p1_script[f]));
            let _ = reference.run_frame();
        }
        let ref_fb = reference.framebuffer().to_vec();

        // Spectator: feed the same stream over the transport. `feeder.send`
        // pushes onto the spectator's inbound wire.
        let (spec_link, mut feeder) = MemoryTransport::pair(LinkConditions::PERFECT, 7);
        let mut spec = SpectatorSession::new(
            SpectatorConfig {
                num_players: 2,
                delay_frames: 0,
            },
            spec_link,
            hash,
        );
        let mut spec_nes = Nes::from_rom(&rom).unwrap();
        spec_nes.power_cycle();

        feeder.send(&NetMessage::Sync {
            magic: NetMessage::SYNC_MAGIC,
            rom_hash: hash,
        });
        for (f, (&p0, &p1)) in p0_script.iter().zip(p1_script.iter()).enumerate() {
            let frame = u32::try_from(f).unwrap();
            feeder.send(&NetMessage::Input {
                player: 0,
                frame,
                input: p0,
            });
            feeder.send(&NetMessage::Input {
                player: 1,
                frame,
                input: p1,
            });
        }

        // Drive the spectator until it has shown all FRAMES (or a bounded cap).
        let mut shown = 0usize;
        for _ in 0..(FRAMES * 4) {
            if spec.advance(&mut spec_nes).produced_frame {
                shown += 1;
            }
        }
        assert!(spec.is_synced(), "spectator validated the Sync");
        assert_eq!(shown, FRAMES, "spectator showed every confirmed frame");
        assert_eq!(
            spec_nes.framebuffer(),
            ref_fb.as_slice(),
            "spectator framebuffer is byte-identical to the reference"
        );
    }
}

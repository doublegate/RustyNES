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

use std::collections::{BTreeMap, VecDeque};

use rustynes_core::{Buttons, Nes};

use crate::message::{IdentityMismatch, NetMessage, SessionIdentity, SyncVerdict};
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

/// v3.0.0 (T-SPECTATOR-HISTORY) — the most frames of input the spectator
/// holds past the frame it shows next. The lookahead above bounds one
/// packet's jump from the confirmed horizon, not how far that horizon walks:
/// contiguous inputs keep confirming frames whether or not any are shown.
/// 65,536 frames is about 18 minutes of play at about 5 bytes a frame, about
/// 320 KiB, so a spectator that falls that far behind (a paused window) still
/// catches up, while a peer streaming faster than real time can no longer
/// grow the buffer without limit. A frame past it is dropped, as an
/// out-of-window frame is.
const MAX_SPECTATOR_BUFFER_FRAMES: u32 = 65_536;

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
    identity: SessionIdentity,

    /// The next frame to be produced (== number of frames shown so far).
    current_frame: u32,
    /// Newest frame for which ALL players' real inputs have arrived. `None`
    /// before the first complete frame.
    last_confirmed_frame: Option<u32>,
    /// `true` once a peer's `Sync` handshake (matching ROM) has been seen. A
    /// spectator validates but does not answer it.
    synced: bool,
    /// v2.9.9 (NF-15) — set by a `Sync` whose identity differs from ours;
    /// see [`Self::mismatch`].
    mismatch: Option<IdentityMismatch>,

    /// Per-frame input history for the frames not yet shown: the front is
    /// [`Self::current_frame`], so frame `f` lives at `f - current_frame`.
    /// Shown frames are released (v3.0.0, T-SPECTATOR-HISTORY; until then
    /// this was an append-only `Vec` indexed by absolute frame, which grew
    /// for the whole session), and its length never passes
    /// [`MAX_SPECTATOR_BUFFER_FRAMES`].
    history: VecDeque<FrameInputs>,
    /// The buffer's capacity, in frames past `current_frame`:
    /// [`MAX_SPECTATOR_BUFFER_FRAMES`], lowered only by this module's tests.
    buffer_cap: u32,
    /// v3.0.0 — the frames whose input was dropped because they lay beyond
    /// the buffer, each with the mask of players whose input was dropped and
    /// has not arrived since. The players acknowledge each other, not the
    /// spectator, so such input is normally never resent, and playback cannot
    /// pass it; see [`Self::stream_lost`]. If a dropped input does arrive
    /// again inside the window, its bit is cleared and the frame plays
    /// normally. Only a wait ON a frame still listed here is a loss, so an
    /// ordinary wait elsewhere is never mistaken for one.
    ///
    /// Precise per frame on purpose. A `(first, last)` range of drops also
    /// covered frames between two drops that were never dropped, which made
    /// an ordinary wait on them fatal (the commit security review of
    /// `6475dd3f`). Bounded: a frame is listed only when it lies at least
    /// `buffer_cap` past the shown frame and within
    /// `MAX_SPECTATOR_FRAME_LOOKAHEAD` of the confirmed horizon, which is
    /// itself inside the buffer. A frame is shown only once every player's
    /// input has arrived, and each arrival clears that player's bit, so no
    /// shown frame is ever still listed (a prune on show was tried and could
    /// never remove anything). At most `buffer_cap +
    /// MAX_SPECTATOR_FRAME_LOOKAHEAD + 1` frames are listed.
    dropped: BTreeMap<u32, u8>,
    /// Set once playback has reached a dropped frame with its input still
    /// missing. Terminal, like [`Self::mismatch`].
    lost: Option<u32>,
}

impl<T: Transport> SpectatorSession<T> {
    /// Create a read-only spectator for `identity` ([`SessionIdentity::of`] the
    /// local machine). It syncs only to a stream announcing the same ROM and,
    /// since v2.9.8, the same machine configuration.
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
    pub fn new(config: SpectatorConfig, transport: T, identity: SessionIdentity) -> Self {
        debug_assert!(
            (2..=4).contains(&config.num_players),
            "num_players must be 2..=4"
        );
        Self {
            config,
            transport,
            identity,
            current_frame: 0,
            last_confirmed_frame: None,
            synced: false,
            mismatch: None,
            history: VecDeque::new(),
            buffer_cap: MAX_SPECTATOR_BUFFER_FRAMES,
            dropped: BTreeMap::new(),
            lost: None,
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

    /// v2.9.9 (NF-15) — why the watched stream was refused: the `Sync` it
    /// announced named another game ([`IdentityMismatch::Rom`]) or another
    /// machine configuration ([`IdentityMismatch::Config`]). Terminal: once
    /// set, [`Self::advance`] produces nothing for the rest of the session,
    /// and the frontend reports the reason as the player handshake does.
    #[must_use]
    pub const fn mismatch(&self) -> Option<IdentityMismatch> {
        self.mismatch
    }

    /// v3.0.0 — the frame playback stopped at because its input was dropped:
    /// the match ran more than [`MAX_SPECTATOR_BUFFER_FRAMES`] (65,536, about
    /// 18 minutes) ahead of what this spectator had shown. Every frame kept
    /// before it plays first; then this is set and [`Self::advance`] produces
    /// nothing more. Terminal: the dropped input is never resent, so the
    /// frontend reports it and the viewer spectates again. Before v3.0.0 the
    /// spectator waited at the gap forever, with no reason given.
    #[must_use]
    pub const fn stream_lost(&self) -> Option<u32> {
        self.lost
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
    /// layer.
    ///
    /// v2.9.9 (NF-15): nothing is shown until a `Sync` whose identity matches
    /// ours has been seen ([`is_synced`](Self::is_synced)); inputs that arrive
    /// first are buffered and play once it does. A `Sync` that does NOT match
    /// ends the session ([`mismatch`](Self::mismatch)). Before v2.9.9 a
    /// mismatch only left `synced` false, which nothing read, so a spectator
    /// with another ROM or configuration showed a different game than the one
    /// being played. A relay that fans the match out to spectators must
    /// therefore forward a player's `Sync`, as it forwards `Roster`.
    pub fn advance(&mut self, nes: &mut Nes) -> SpectatorOutcome {
        self.ingest();
        self.recompute_confirmed();
        if !self.synced || self.mismatch.is_some() || self.lost.is_some() {
            return SpectatorOutcome::default();
        }

        // Show the next frame only once every player's real input is known AND
        // it sits at or behind the (optionally delayed) reveal horizon. With
        // `delay_frames == 0` this is exactly "as soon as confirmed"; with a
        // positive delay the frame is held until `frame + delay_frames` has also
        // been confirmed (the delayed-stream / broadcast-delay buffer).
        let frame = self.current_frame;
        let ready = self.reveal_horizon().is_some_and(|h| frame <= h);
        if !ready {
            // Waiting is right unless this frame's input was dropped at the
            // cap: that input never comes, so say so instead of waiting.
            if self.dropped.contains_key(&frame) {
                self.lost = Some(frame);
            }
            return SpectatorOutcome::default();
        }

        self.apply_and_run(nes, frame);
        // The frame is shown: release it (the front of `history` is always
        // `current_frame`).
        self.history.pop_front();
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
                NetMessage::Sync { magic, identity } => {
                    // A foreign magic is a different protocol, ignored like any
                    // stray datagram; our magic with another identity, or an
                    // older RustyNES's magic (v3.0.0), is the players' stream
                    // for another machine or emulator, and terminal.
                    if self.mismatch.is_none() {
                        match self.identity.check_sync(magic, &identity) {
                            SyncVerdict::Ignore => {}
                            SyncVerdict::Accept => self.synced = true,
                            SyncVerdict::Refuse(why) => self.mismatch = Some(why),
                        }
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
                    // v3.0.0 (T-SPECTATOR-HISTORY): and within the buffer.
                    // The check above bounds one packet's jump from the
                    // confirmed horizon, but the horizon itself walks forward
                    // with every contiguous frame of input, shown or not.
                    // Without this cap, a peer streaming faster than real
                    // time, or a stream that never sends a matching `Sync`,
                    // grew the history without limit. A frame already shown
                    // is past, and dropped too.
                    if frame < self.current_frame {
                        continue;
                    }
                    if frame - self.current_frame >= self.buffer_cap {
                        // Dropped, and not resent: remember where playback
                        // will have to stop (v3.0.0, `stream_lost`).
                        *self.dropped.entry(frame).or_default() |= 1 << player;
                        continue;
                    }
                    // A dropped input that arrives again in the window is
                    // no longer missing.
                    if let Some(mask) = self.dropped.get_mut(&frame) {
                        *mask &= !(1 << player);
                        if *mask == 0 {
                            self.dropped.remove(&frame);
                        }
                    }
                    let slot = self.slot_mut(frame);
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

    /// The history slot for `frame`, growing the buffer to reach it. The
    /// caller has checked `current_frame <= frame` and that the distance is
    /// under [`MAX_SPECTATOR_BUFFER_FRAMES`], so the growth is bounded.
    fn slot_mut(&mut self, frame: u32) -> &mut FrameInputs {
        let idx = (frame - self.current_frame) as usize;
        if self.history.len() <= idx {
            self.history.resize(idx + 1, FrameInputs::default());
        }
        &mut self.history[idx]
    }

    /// Recompute `last_confirmed_frame` = the newest frame, contiguously from
    /// the current confirmed prefix, for which every player's input arrived.
    /// Frames before `current_frame` were shown, so they were confirmed and
    /// have been released; the walk starts at whichever is later.
    fn recompute_confirmed(&mut self) {
        let n = self.config.num_players;
        let all = if n >= 8 { u8::MAX } else { (1u8 << n) - 1 };
        let start = self
            .last_confirmed_frame
            .map_or(0, |c| c + 1)
            .max(self.current_frame);
        // The buffer is bounded by `MAX_SPECTATOR_BUFFER_FRAMES` (65,536), so
        // its length always fits `u32`; saturate rather than cast-truncate.
        let end = self
            .current_frame
            .saturating_add(u32::try_from(self.history.len()).unwrap_or(u32::MAX));
        let mut confirmed = self.last_confirmed_frame;
        for f in start..end {
            if self.history[(f - self.current_frame) as usize].arrived & all == all {
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
    /// `frame` is always `current_frame`, the front of the buffer.
    fn apply_and_run(&self, nes: &mut Nes, frame: u32) {
        let slot = self.history[(frame - self.current_frame) as usize];
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
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
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
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
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
        // v2.9.9 (NF-15): nothing is shown before a matching `Sync`.
        feeder.send(&NetMessage::Sync {
            magic: NetMessage::SYNC_MAGIC,
            identity: hash,
        });

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
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
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
    /// T-SPECTATOR-HISTORY (v3.0.0): the input history is bounded however
    /// long the stream runs. `MAX_SPECTATOR_FRAME_LOOKAHEAD` stopped one
    /// packet jumping far ahead, but not the horizon walking: every
    /// contiguous frame of input advanced the confirmed frame, and the window
    /// with it, whether or not anything was shown, so a peer streaming
    /// faster than real time (or a stream that never sends a matching
    /// `Sync`) grew `history` without limit. Here 70,000 frames arrive with
    /// no `Sync`, and `advance` runs every 512 frames, well inside the
    /// lookahead, so every frame passes the jump guard and only the buffer
    /// cap can stop it. (A first draft advanced every 4,096 frames: the jump
    /// guard then dropped most inputs between calls, so the history never
    /// neared the cap, and removing the cap went NOT CAUGHT.) Then, once
    /// synced, every shown frame must be released, so a long session does
    /// not accumulate.
    #[test]
    fn the_input_history_is_bounded_and_releases_shown_frames() {
        let rom = synth_nrom();
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
        let (spec_link, mut feeder) = MemoryTransport::pair(LinkConditions::PERFECT, 7);
        let mut spec = SpectatorSession::new(SpectatorConfig::default(), spec_link, hash);
        let mut nes = Nes::from_rom(&rom).unwrap();
        for frame in 0..70_000u32 {
            for player in 0..2 {
                feeder.send(&NetMessage::Input {
                    player,
                    frame,
                    input: 0,
                });
            }
            if frame % 512 == 0 {
                let _ = spec.advance(&mut nes);
            }
        }
        let _ = spec.advance(&mut nes);
        assert_eq!(
            spec.history.len(),
            MAX_SPECTATOR_BUFFER_FRAMES as usize,
            "the buffer fills to the cap and stops there"
        );
        assert!(
            spec.history.len() <= MAX_SPECTATOR_BUFFER_FRAMES as usize,
            "unsynced history grew to {} frames",
            spec.history.len()
        );
        // Synced now: the buffered frames play, and each shown frame leaves
        // the buffer.
        feeder.send(&NetMessage::Sync {
            magic: NetMessage::SYNC_MAGIC,
            identity: hash,
        });
        let before = spec.history.len();
        let shown = (0..64)
            .filter(|_| spec.advance(&mut nes).produced_frame)
            .count();
        assert_eq!(shown, 64, "the buffered stream plays once synced");
        assert_eq!(spec.history.len(), before - 64, "shown frames are released");
    }

    /// v3.0.0 — a frame dropped at the buffer cap is never resent (the
    /// players acknowledge each other, not the spectator), so a spectator
    /// that plays its retained frames reached the gap and waited forever,
    /// with no reason given (a review finding on #588; the 64-frame window of the
    /// test above stopped short of it). It now plays every frame it kept and
    /// then reports the stream lost, at the first dropped frame. The cap is
    /// lowered to 32 here so the test reaches it in 32 emulated frames.
    #[test]
    fn a_spectator_that_drops_input_at_the_cap_reports_it() {
        let rom = synth_nrom();
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
        let (spec_link, mut feeder) = MemoryTransport::pair(LinkConditions::PERFECT, 7);
        let mut spec = SpectatorSession::new(SpectatorConfig::default(), spec_link, hash);
        spec.buffer_cap = 32;
        let mut nes = Nes::from_rom(&rom).unwrap();
        for frame in 0..40u32 {
            for player in 0..2 {
                feeder.send(&NetMessage::Input {
                    player,
                    frame,
                    input: 0,
                });
            }
        }
        feeder.send(&NetMessage::Sync {
            magic: NetMessage::SYNC_MAGIC,
            identity: hash,
        });
        let shown = (0..32)
            .filter(|_| spec.advance(&mut nes).produced_frame)
            .count();
        assert_eq!(shown, 32, "every retained frame plays");
        assert_eq!(spec.stream_lost(), None, "not lost while frames remain");
        assert!(
            !spec.advance(&mut nes).produced_frame,
            "frame 32 was dropped"
        );
        assert_eq!(
            spec.stream_lost(),
            Some(32),
            "the gap is reported, not waited on"
        );
    }

    /// v3.0.0 — the record of dropped frames must not outlive them. If a
    /// dropped frame's input does arrive later inside the window (a relay
    /// that replays), it is shown normally. After that, an ordinary wait for
    /// a later frame must NOT be reported as a lost stream. The first version
    /// kept `dropped_from` forever, so any wait past it was terminal (found
    /// by the commit's security review).
    #[test]
    fn a_dropped_frame_that_arrives_later_clears_the_record() {
        let rom = synth_nrom();
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
        let (spec_link, mut feeder) = MemoryTransport::pair(LinkConditions::PERFECT, 7);
        let mut spec = SpectatorSession::new(SpectatorConfig::default(), spec_link, hash);
        spec.buffer_cap = 8;
        let mut nes = Nes::from_rom(&rom).unwrap();
        let send = |feeder: &mut MemoryTransport, frames: std::ops::Range<u32>| {
            for frame in frames {
                for player in 0..2 {
                    feeder.send(&NetMessage::Input {
                        player,
                        frame,
                        input: 0,
                    });
                }
            }
        };
        send(&mut feeder, 0..10); // frames 8 and 9 fall beyond the cap
        feeder.send(&NetMessage::Sync {
            magic: NetMessage::SYNC_MAGIC,
            identity: hash,
        });
        let shown = (0..8)
            .filter(|_| spec.advance(&mut nes).produced_frame)
            .count();
        assert_eq!(shown, 8);
        // The dropped frames arrive again, now inside the window, and play.
        send(&mut feeder, 8..10);
        let shown = (0..2)
            .filter(|_| spec.advance(&mut nes).produced_frame)
            .count();
        assert_eq!(shown, 2, "the recovered frames play");
        // Frame 10 has simply not arrived yet: an ordinary wait.
        assert!(!spec.advance(&mut nes).produced_frame);
        assert_eq!(
            spec.stream_lost(),
            None,
            "a wait past a recovered gap is not a loss"
        );
        send(&mut feeder, 10..11);
        assert!(spec.advance(&mut nes).produced_frame, "the session goes on");

        // A later drop must not condemn the frames between it and the earlier
        // one. Frame 30 drops (past the window [11, 19)); frames 11-12 are
        // complete and 13 has only player 0 so far. Waiting on 13 is an
        // ordinary wait: 13 was never dropped. A `(first, last)` range of
        // drops, 8..=30, covered it and made the wait fatal (the security
        // review of 6475dd3f).
        for player in 0..2 {
            feeder.send(&NetMessage::Input {
                player,
                frame: 30,
                input: 0,
            });
        }
        send(&mut feeder, 11..13);
        feeder.send(&NetMessage::Input {
            player: 0,
            frame: 13,
            input: 0,
        });
        let shown = (0..2)
            .filter(|_| spec.advance(&mut nes).produced_frame)
            .count();
        assert_eq!(shown, 2);
        assert!(
            !spec.advance(&mut nes).produced_frame,
            "frame 13 is incomplete"
        );
        assert_eq!(
            spec.stream_lost(),
            None,
            "a frame never dropped is not lost"
        );
    }

    /// v3.0.0 — a frame is lost only while a DROPPED input is missing. Here
    /// frame 8 drops for player 0 only; player 0's input arrives again inside
    /// the window, and player 1's (never dropped) is simply late. The wait on
    /// frame 8 is then ordinary, not a loss.
    #[test]
    fn a_frame_whose_dropped_input_returned_is_an_ordinary_wait() {
        let rom = synth_nrom();
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
        let (spec_link, mut feeder) = MemoryTransport::pair(LinkConditions::PERFECT, 7);
        let mut spec = SpectatorSession::new(SpectatorConfig::default(), spec_link, hash);
        spec.buffer_cap = 8;
        let mut nes = Nes::from_rom(&rom).unwrap();
        let input = |player: u8, frame: u32| NetMessage::Input {
            player,
            frame,
            input: 0,
        };
        for frame in 0..8u32 {
            feeder.send(&input(0, frame));
            feeder.send(&input(1, frame));
        }
        feeder.send(&input(0, 8)); // beyond the cap: dropped for player 0
        feeder.send(&NetMessage::Sync {
            magic: NetMessage::SYNC_MAGIC,
            identity: hash,
        });
        let shown = (0..8)
            .filter(|_| spec.advance(&mut nes).produced_frame)
            .count();
        assert_eq!(shown, 8);
        feeder.send(&input(0, 8)); // player 0's input returns, in the window
        assert!(!spec.advance(&mut nes).produced_frame, "player 1 is late");
        assert_eq!(spec.stream_lost(), None, "no dropped input is missing");
        feeder.send(&input(1, 8));
        assert!(spec.advance(&mut nes).produced_frame, "frame 8 plays");
    }

    /// A peer-supplied `Input.frame` far beyond the confirmed horizon must be
    /// dropped WITHOUT growing `history` (otherwise a `frame` near `u32::MAX`
    /// would resize the `Vec` unboundedly — an OOM `DoS`). The in-window frame
    /// that follows is still accepted.
    #[test]
    fn spectator_rejects_out_of_window_frame_without_allocating() {
        let rom = synth_nrom();
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
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
        // v2.9.9 (NF-15): nothing is shown before a matching `Sync`.
        feeder.send(&NetMessage::Sync {
            magic: NetMessage::SYNC_MAGIC,
            identity: hash,
        });

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
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
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
        // v2.9.9 (NF-15): nothing is shown before a matching `Sync`.
        feeder.send(&NetMessage::Sync {
            magic: NetMessage::SYNC_MAGIC,
            identity: hash,
        });

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
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());

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
            identity: hash,
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

    /// v2.9.9 (NF-15) — a spectator shows nothing until a matching `Sync`,
    /// and a mismatching one ends the session with its reason.
    ///
    /// `advance` never consulted `synced`, and nothing in either frontend read
    /// `is_synced`, so a spectator with another ROM or another machine
    /// configuration ran the players' stream regardless and showed a
    /// different game than the one being played, silently. v2.9.8's handshake
    /// refuses a mismatched PLAYER with a message; this is the same rule for a
    /// spectator.
    #[test]
    fn a_spectator_runs_only_a_stream_whose_sync_matches() {
        let rom = synth_nrom();
        let ours = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
        let feed = |feeder: &mut MemoryTransport, frames: u32| {
            for frame in 0..frames {
                for player in 0..2 {
                    feeder.send(&NetMessage::Input {
                        player,
                        frame,
                        input: 0,
                    });
                }
            }
        };
        let mut other_config = ours;
        other_config.config_hash[0] ^= 1;
        let mut other_rom = ours;
        other_rom.rom_hash[0] ^= 1;

        // No Sync yet: inputs are buffered, nothing is shown.
        let (link, mut feeder) = MemoryTransport::pair(LinkConditions::PERFECT, 7);
        let mut spec = SpectatorSession::new(SpectatorConfig::default(), link, ours);
        let mut nes = Nes::from_rom(&rom).unwrap();
        feed(&mut feeder, 10);
        for _ in 0..20 {
            assert!(
                !spec.advance(&mut nes).produced_frame,
                "shown before a Sync"
            );
        }
        assert_eq!(spec.mismatch(), None);
        // The matching Sync arrives: the buffered stream plays.
        feeder.send(&NetMessage::Sync {
            magic: NetMessage::SYNC_MAGIC,
            identity: ours,
        });
        let shown = (0..20)
            .filter(|_| spec.advance(&mut nes).produced_frame)
            .count();
        assert_eq!(shown, 10, "the stream plays once synced");

        for (theirs, want) in [
            (other_config, IdentityMismatch::Config),
            (other_rom, IdentityMismatch::Rom),
        ] {
            let (link, mut feeder) = MemoryTransport::pair(LinkConditions::PERFECT, 7);
            let mut spec = SpectatorSession::new(SpectatorConfig::default(), link, ours);
            let mut nes = Nes::from_rom(&rom).unwrap();
            feeder.send(&NetMessage::Sync {
                magic: NetMessage::SYNC_MAGIC,
                identity: theirs,
            });
            feed(&mut feeder, 10);
            for _ in 0..20 {
                assert!(
                    !spec.advance(&mut nes).produced_frame,
                    "{want:?}: a mismatched stream was shown"
                );
            }
            assert_eq!(spec.mismatch(), Some(want));
        }
    }
}

//! The GGPO-style rollback session — the heart of the netcode.
//!
//! # The rollback algorithm
//!
//! `N` peers (2..=4) run the same deterministic emulator. Each owns one
//! player's input; every other player's input arrives over the network with
//! latency. Rather than wait for it, a peer **predicts** the remote inputs and
//! advances immediately, so input feels lag-free. When a real remote input
//! arrives and differs from what was predicted, the peer **rolls back**: it
//! restores a save-state from before the mispredicted frame and
//! **re-simulates** forward to the present, now applying the correct inputs.
//! The emulator's determinism contract (same ROM + seed + input ⇒
//! byte-identical state) guarantees the re-simulation reproduces every frame
//! exactly — so all peers converge on identical state once inputs are
//! confirmed.
//!
//! Each [`RollbackSession::advance`] call performs, in order:
//!
//! 1. **Ingest** — poll the transport, fold remote `Input`s (each tagged with
//!    its `player` index) into history, answer with `InputAck`, process
//!    `Sync` / `Checksum` / `Quality`.
//! 2. **Rollback (on misprediction)** — if any freshly-ingested remote input
//!    disagrees with the prediction we previously used for that frame,
//!    restore the snapshot at the earliest affected frame and re-run forward
//!    to `current_frame`, applying every player's inputs from history each
//!    step.
//! 3. **Predict** — fill each not-yet-known remote player's input for
//!    `current_frame` by repeating that player's last known input (the
//!    standard heuristic).
//! 4. **Advance** — snapshot the present, apply all players' inputs, run one
//!    frame.
//! 5. **Send** — transmit the local input to every peer.
//! 6. **Confirm** — recompute `last_confirmed_frame` = the newest frame for
//!    which ALL players' real inputs are known.
//! 7. **Checksum** — periodically hash state at a confirmed frame and
//!    exchange it; a mismatch is a fatal [`NetplayError::Desync`].
//!
//! # Topology
//!
//! The session is transport-agnostic: it only ever
//! [`send`](crate::transport::Transport::send)s its local input and
//! [`poll`](crate::transport::Transport::poll)s for *every* other player's
//! inputs. For >2 players the natural realization is a **mesh** — each peer
//! sends its own input (tagged with its player index) to all others and polls
//! all of them — which keeps `advance` clean (one `send`, one `poll`, no
//! relay bookkeeping) and avoids a single relay point of failure. The
//! determinism harness wires N peers with an in-memory mesh transport; the
//! 2-player path is exactly the prior pairwise link (`player` is always the
//! one remote index).

use rustynes_core::{Buttons, Nes};

use crate::message::{NetMessage, fnv1a64};
use crate::transport::Transport;

/// The maximum number of players.
///
/// The NES has two ports; the Four Score adapter multiplexes up to four.
/// Per-frame input is stored in a fixed `[_; MAX_PLAYERS]` array to keep the
/// rollback hot path allocation-free.
pub const MAX_PLAYERS: usize = 4;

/// Max number of recent un-acknowledged local inputs resent per tick over the
/// unreliable transport (packet-loss recovery; see
/// `resend_unacked_local_inputs`). Caps the burst during a long outage; under
/// normal conditions, with acks flowing, far fewer (the in-flight latency
/// window) are resent.
const INPUT_RESEND_WINDOW: u32 = 64;

/// Errors that abort a netplay session.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NetplayError {
    /// A confirmed-frame checksum from the peer disagreed with ours: the two
    /// emulators have diverged. Unrecoverable.
    #[error(
        "desync detected at frame {frame} ({}): local checksum {local:#018x} != remote {remote:#018x}",
        if *same_framebuffer { "timing/cycle divergence — same picture, different cycle count" } else { "state divergence — the rendered state itself differs" }
    )]
    Desync {
        /// The confirmed frame whose checksums disagreed.
        frame: u32,
        /// Our combined checksum.
        local: u64,
        /// The peer's combined checksum.
        remote: u64,
        /// `true` if both peers' framebuffer hashes for the frame were equal
        /// (so only the cumulative cycle term diverged — a timing bug); `false`
        /// if the rendered state itself diverged.
        same_framebuffer: bool,
    },

    /// Restoring a save-state during rollback failed (should be impossible
    /// with a well-formed ring; surfaced rather than panicking).
    #[error("rollback restore failed: {0}")]
    Restore(#[from] rustynes_core::SnapshotError),

    /// A `Sync` handshake from the peer carried a different ROM hash, so the
    /// two peers are not running the same game.
    #[error("rom mismatch: peer is running a different ROM")]
    RomMismatch,
}

/// Configuration for a [`RollbackSession`].
#[derive(Clone, Copy, Debug)]
pub struct SessionConfig {
    /// How many players are in the session (2..=4). Two players map to the two
    /// NES controller ports directly; 3-4 players use the Four Score adapter
    /// (controller ports `0..num_players`). Defaults to `2`.
    pub num_players: u8,
    /// Which player this peer controls: `0` = P1 (`$4016`), `1` = P2
    /// (`$4017`), `2`/`3` = Four Score players 3/4. Must be `< num_players`.
    pub local_player: u8,
    /// Frames of buffer between recording a local input and the frame it
    /// applies to. Trades input latency for fewer rollbacks; GGPO calls this
    /// the "input delay". `0` is valid.
    pub input_delay: u32,
    /// Maximum number of frames the session will speculatively run ahead of
    /// the last confirmed frame (the size of the save-state / history
    /// window). A peer that would exceed this stalls instead.
    pub max_rollback_frames: u32,
    /// Exchange a checksum every `checksum_interval` frames (`0` disables
    /// checksums / desync detection). Checksums cover only confirmed frames.
    pub checksum_interval: u32,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            num_players: 2,
            local_player: 0,
            input_delay: 2,
            max_rollback_frames: 8,
            checksum_interval: 30,
        }
    }
}

/// What a single [`RollbackSession::advance`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdvanceOutcome {
    /// `true` if a frame was produced this tick. `false` means the session
    /// stalled — the caller should skip rendering this tick (time-sync
    /// back-pressure).
    pub produced_frame: bool,
    /// `true` if a rollback + re-simulation happened this tick.
    pub rolled_back: bool,
    /// How many frames were re-simulated during the rollback (0 if none).
    pub resimulated_frames: u32,
    /// The frame index just produced (only meaningful when `produced_frame`).
    pub frame: u32,
}

impl AdvanceOutcome {
    /// The outcome for a tick that stalled (produced nothing) for time-sync.
    const STALLED: Self = Self {
        produced_frame: false,
        rolled_back: false,
        resimulated_frames: 0,
        frame: 0,
    };
}

/// One player's input for a single frame.
///
/// For the local player `confirmed` is set the moment it is authored (we own
/// it). For a remote player it is `false` (holding a prediction) until the
/// real value arrives over the transport. The flag lets a rollback detect a
/// misprediction by comparing a newly-arrived value against the (predicted)
/// value we previously ran with.
#[derive(Clone, Copy, Debug, Default)]
struct PlayerInput {
    input: u8,
    confirmed: bool,
}

/// Per-frame input record for all players, plus a `simulated` flag.
#[derive(Clone, Copy, Debug, Default)]
struct InputSlot {
    /// One cell per player index (only `0..num_players` are meaningful).
    players: [PlayerInput; MAX_PLAYERS],
    /// Set once we have run this frame (consumed its inputs into the
    /// emulator). Used to detect that a late remote input mispredicts an
    /// already-simulated frame and therefore requires a rollback.
    simulated: bool,
}

/// An `N`-player GGPO-style rollback session driving one [`Nes`] over a
/// [`Transport`].
///
/// Construct with [`Self::new`], feed the local input each tick with
/// [`Self::add_local_input`], then call [`Self::advance`] once per visual
/// frame. See the module docs for the algorithm. `N` (2..=4) is
/// [`SessionConfig::num_players`]; with `N == 2` this is exactly the
/// pairwise 2-player session.
pub struct RollbackSession<T: Transport> {
    config: SessionConfig,
    transport: T,
    rom_hash: [u8; 32],

    /// The next frame to be produced (== number of frames produced so far).
    current_frame: u32,
    /// Newest frame for which ALL players' real inputs are known.
    last_confirmed_frame: Option<u32>,
    /// Highest frame at which the peer has confirmed receiving our input.
    remote_ack_frame: Option<u32>,
    /// `true` once a peer's `Sync` handshake has been validated.
    synced: bool,

    /// Per-frame input history, indexed by frame. Grows as frames are
    /// produced; old entries below the rollback window are never read again
    /// but are retained (Stage 1 keeps the full history for simplicity — a
    /// production build would prune; see the Stage 2/3 handoff).
    history: Vec<InputSlot>,

    /// Save-state ring: `snapshots[f]` is the emulator state *before* frame
    /// `f` was run (so restoring it and running forward reproduces frame f).
    /// Indexed by frame; only the trailing `max_rollback_frames + 1` entries
    /// are guaranteed present (older are `None` after pruning).
    snapshots: Vec<Option<Vec<u8>>>,

    /// Checksums we have *sent* at confirmed frames, kept so a later remote
    /// `Checksum` for the same frame can be compared and so we don't re-send.
    local_checksums: Vec<Option<u64>>,

    /// The canonical hash of the deterministic *entering* state of each
    /// checksummed frame, recorded by `resync` the moment the frame becomes
    /// confirmed (its `snapshots[f]` is then a function of real inputs only).
    /// Because all peers reach byte-identical entering state for a confirmed
    /// frame, these hashes match across peers — the property that makes the
    /// checksum exchange a sound desync detector.
    /// Each entry is `(combined_digest, framebuffer_hash)`: the combined value
    /// is what desync detection compares; the framebuffer hash is carried so a
    /// detected desync can report whether the *picture* diverged (a state bug)
    /// or only the cycle term (a timing bug).
    confirmed_hashes: Vec<Option<(u64, u64)>>,

    /// Remote checksums that arrived before our matching `confirmed_hashes`
    /// entry was ready, kept so the comparison happens once we can compute
    /// ours. `(combined_digest, framebuffer_hash)`, as above.
    remote_checksums: Vec<Option<(u64, u64)>>,

    /// The canonical confirmed checkpoint: `(frame, entering-state snapshot)`,
    /// where `frame == last_confirmed_frame + 1` (or 0 before any frame). The
    /// snapshot is the emulator state entering `frame` derived from real
    /// (confirmed) inputs ONLY, so it is byte-identical across peers. Every
    /// rollback restores it as the base, and confirmed-frame checksums hash
    /// it — that is what makes both peer-sync and desync-detection sound.
    /// `None` until the first frame is produced.
    checkpoint: Option<(u32, Vec<u8>)>,

    /// The canonical *entering* gameplay digest of each confirmed frame,
    /// written only from a fully-confirmed replay (so it is cross-peer-
    /// identical). Stage 1 retains the full history for the determinism
    /// harness to inspect; a production build would keep only a sliding window
    /// (see the Stage 2/3 handoff). `confirmed_entering[f]`.
    confirmed_entering: Vec<Option<u64>>,
}

impl<T: Transport> RollbackSession<T> {
    /// Create a session for `rom_hash` (from [`Nes::rom_sha256`]). Sends the
    /// opening `Sync` handshake immediately.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if `config.num_players` is not in `2..=4` or
    /// `config.local_player >= config.num_players`.
    pub fn new(config: SessionConfig, mut transport: T, rom_hash: [u8; 32]) -> Self {
        debug_assert!(
            (2..=4).contains(&config.num_players),
            "num_players must be 2..=4"
        );
        debug_assert!(
            config.local_player < config.num_players,
            "local_player must be < num_players"
        );
        transport.send(&NetMessage::Sync {
            magic: NetMessage::SYNC_MAGIC,
            rom_hash,
        });
        let mut session = Self {
            config,
            transport,
            rom_hash,
            current_frame: 0,
            last_confirmed_frame: None,
            remote_ack_frame: None,
            synced: false,
            history: Vec::new(),
            snapshots: Vec::new(),
            local_checksums: Vec::new(),
            confirmed_hashes: Vec::new(),
            remote_checksums: Vec::new(),
            checkpoint: None,
            confirmed_entering: Vec::new(),
        };
        // GGPO input-delay convention: the first `input_delay` frames have no
        // buffered local input, so they run with "no buttons" and are
        // immediately confirmable. Seed (and announce) those empty inputs for
        // this peer's local player so confirmation can begin from frame 0 once
        // the other peers' matching empty inputs arrive.
        let lp = config.local_player;
        for f in 0..config.input_delay {
            session.ensure_frame(f);
            session.history[f as usize].players[lp as usize] = PlayerInput {
                input: 0,
                confirmed: true,
            };
            session.transport.send(&NetMessage::Input {
                player: lp,
                frame: f,
                input: 0,
            });
        }
        session
    }

    /// The frame the session will produce next.
    #[must_use]
    pub const fn current_frame(&self) -> u32 {
        self.current_frame
    }

    /// Number of players in this session (2..=4).
    #[must_use]
    pub const fn num_players(&self) -> u8 {
        self.config.num_players
    }

    /// Newest frame for which all players' real inputs are known, if any.
    #[must_use]
    pub const fn last_confirmed_frame(&self) -> Option<u32> {
        self.last_confirmed_frame
    }

    /// The canonical gameplay digest of the deterministic state *entering*
    /// `frame`, available once `frame` is confirmed (every frame before it ran
    /// with real inputs). It is a function of confirmed inputs alone, so it is
    /// identical across peers — the basis for cross-peer state verification.
    /// Returns `None` if `frame` is not yet confirmed. Primarily for tests /
    /// debugging.
    #[must_use]
    pub fn confirmed_entering_digest(&self, frame: u32) -> Option<u64> {
        self.confirmed_entering
            .get(frame as usize)
            .copied()
            .flatten()
    }

    /// The confirmed per-port input applied at `frame`, if every player is
    /// confirmed. The returned array is indexed by canonical controller port
    /// (port 0 = P1, … port 3 = Four Score P4), independent of which player is
    /// local. Ports `>= num_players` are `0`. For tests / debugging.
    #[must_use]
    pub fn confirmed_input(&self, frame: u32) -> Option<[u8; MAX_PLAYERS]> {
        let slot = self.history.get(frame as usize).copied()?;
        let n = self.config.num_players as usize;
        if (0..n).all(|p| slot.players[p].confirmed) {
            let mut out = [0u8; MAX_PLAYERS];
            for (p, cell) in slot.players.iter().enumerate().take(n) {
                out[p] = cell.input;
            }
            Some(out)
        } else {
            None
        }
    }

    /// `true` once a peer's `Sync` handshake has been validated.
    #[must_use]
    pub const fn is_synced(&self) -> bool {
        self.synced
    }

    /// Borrow the transport (e.g. to inspect link stats). Mainly for tests.
    pub const fn transport(&self) -> &T {
        &self.transport
    }

    /// The local player's index (`0..num_players`).
    const fn local_player(&self) -> u8 {
        self.config.local_player
    }

    /// Record the local player's input for the frame it will apply to
    /// (`current_frame + input_delay`). Call once per tick before
    /// [`Self::advance`].
    pub fn add_local_input(&mut self, input: Buttons) {
        let target = self.current_frame + self.config.input_delay;
        let lp = self.local_player();
        self.ensure_frame(target);
        self.history[target as usize].players[lp as usize] = PlayerInput {
            input: input.bits(),
            confirmed: true,
        };
        // Immediately tell every peer (redundant resend of recent inputs is
        // the UDP transport's job in Stage 2; here we send once per add). The
        // transport fans this out to all other players (mesh topology).
        self.transport.send(&NetMessage::Input {
            player: lp,
            frame: target,
            input: input.bits(),
        });
    }

    /// Grow the per-frame vectors so index `frame` is addressable.
    fn ensure_frame(&mut self, frame: u32) {
        let need = frame as usize + 1;
        if self.history.len() < need {
            self.history.resize(need, InputSlot::default());
        }
        if self.snapshots.len() < need {
            self.snapshots.resize(need, None);
        }
        if self.local_checksums.len() < need {
            self.local_checksums.resize(need, None);
        }
        if self.confirmed_hashes.len() < need {
            self.confirmed_hashes.resize(need, None);
        }
        if self.remote_checksums.len() < need {
            self.remote_checksums.resize(need, None);
        }
        if self.confirmed_entering.len() < need {
            self.confirmed_entering.resize(need, None);
        }
    }

    /// Advance one visual frame. See the module docs for the full algorithm.
    ///
    /// # Errors
    ///
    /// Returns [`NetplayError::Desync`] if a confirmed-frame checksum from
    /// the peer disagrees with ours, [`NetplayError::RomMismatch`] if a peer
    /// is running a different ROM, or [`NetplayError::Restore`] if a
    /// rollback's save-state restore fails.
    pub fn advance(&mut self, nes: &mut Nes) -> Result<AdvanceOutcome, NetplayError> {
        // 1. Ingest everything the peers sent us, detecting the earliest frame
        //    whose prediction was just contradicted by a real remote input.
        let earliest_mispredict = self.ingest(nes)?;

        // 2. Recompute confirmation: newly-arrived inputs may extend the
        //    confirmed prefix, advancing the canonical checkpoint.
        let confirmed_before = self.last_confirmed_frame;
        self.recompute_confirmed();
        let confirmation_advanced = self.last_confirmed_frame != confirmed_before;

        // 2b. Acknowledge inputs with a CUMULATIVE (contiguous) frame, not the
        //     highest one received. `last_confirmed_frame` is the highest frame
        //     for which every player's input is present with NO gaps — and since
        //     our own inputs are always confirmed, it is exactly the highest
        //     contiguous frame for which we have received all of each peer's
        //     inputs. Acking that lets a sender keep resending a dropped LOW
        //     frame even after HIGHER frames arrived out of order on the
        //     unordered transport (a highest-received ack would wrongly suppress
        //     the resend of the gap and desync). Sent every tick so a dropped
        //     ack self-heals; harmless before the first confirmed frame.
        if let Some(frame) = self.last_confirmed_frame {
            self.transport.send(&NetMessage::InputAck { frame });
        }

        // 3. Roll back + re-simulate when a misprediction contradicted an
        //    already-run frame, OR when confirmation advanced (the checkpoint
        //    must move forward over now-confirmed frames). The re-sim restores
        //    the canonical confirmed checkpoint and replays forward, advancing
        //    the checkpoint as it crosses confirmed frames. Because the
        //    checkpoint is derived from confirmed inputs only, every peer
        //    replays from a byte-identical base — the property the cross-peer
        //    determinism proof relies on.
        let mispredicted = earliest_mispredict.is_some_and(|m| m < self.current_frame);
        let mut rolled_back = false;
        let mut resimulated = 0u32;
        if (mispredicted || confirmation_advanced) && self.checkpoint.is_some() {
            resimulated = self.resync(nes)?;
            rolled_back = mispredicted;
        }

        // 3b. Resolve any remote checksums now that the checkpoint advanced.
        self.compare_pending_checksums()?;

        // 3c. Redundantly resend the local inputs the remote has NOT yet
        //     acknowledged. The transport is unreliable (UDP / an
        //     unreliable+unordered WebRTC data channel): a single dropped Input
        //     would otherwise be lost forever, so the peer mispredicts that
        //     frame permanently and desyncs at the next checksum. Resending the
        //     un-acked window every tick (idempotent on the receiver) recovers
        //     any drop within a few frames. Runs even when we stall below.
        self.resend_unacked_local_inputs();

        // Time-sync: if we are running too far ahead of the confirmed frame,
        // stall so the peers can catch up and we stay inside the window.
        if self.should_stall() {
            self.send_quality();
            return Ok(AdvanceOutcome::STALLED);
        }

        // 4. Predict the remote inputs for the frame we're about to run.
        let frame = self.current_frame;
        self.ensure_frame(frame);
        self.predict_remotes(frame);

        // 5. Run one frame. Seed the checkpoint on the very first frame (frame
        //    0's entering state is the deterministic power-on, identical
        //    across peers).
        if self.checkpoint.is_none() {
            // v2.8.0 Phase 3 — core snapshot (no thumbnail section); the
            // checkpoint is machine-consumed only.
            let mut seed = Vec::new();
            nes.snapshot_core_into(&mut seed);
            self.checkpoint = Some((frame, seed));
        }
        self.apply_and_run(nes, frame);
        self.history[frame as usize].simulated = true;

        // 6. Send our (already-confirmed-local) input for this frame to the
        //    peers. (add_local_input also sent it for the input-delay target;
        //    this resend covers the immediate frame for the zero-delay case.)
        let lp = self.local_player() as usize;
        if self.history[frame as usize].players[lp].confirmed {
            self.transport.send(&NetMessage::Input {
                player: self.local_player(),
                frame,
                input: self.history[frame as usize].players[lp].input,
            });
        }

        // 7. Advance the clock.
        self.current_frame += 1;

        // 8. Periodically checksum a confirmed frame and exchange it.
        self.maybe_send_checksum();

        Ok(AdvanceOutcome {
            produced_frame: true,
            rolled_back,
            resimulated_frames: resimulated,
            frame,
        })
    }

    /// Poll the transport and fold in every message. Returns the earliest
    /// frame (if any) whose previously-used prediction was contradicted by a
    /// newly-arrived real remote input.
    fn ingest(&mut self, nes: &Nes) -> Result<Option<u32>, NetplayError> {
        let mut earliest_mispredict: Option<u32> = None;
        let messages = self.transport.poll();
        let lp = self.local_player();

        for msg in messages {
            match msg {
                NetMessage::Sync { magic, rom_hash } => {
                    if magic != NetMessage::SYNC_MAGIC {
                        continue;
                    }
                    if rom_hash != self.rom_hash {
                        return Err(NetplayError::RomMismatch);
                    }
                    self.synced = true;
                }
                NetMessage::Input {
                    player,
                    frame,
                    input,
                } => {
                    // Ignore an out-of-range or self-addressed player index
                    // (a malformed / foreign packet must never corrupt our own
                    // authored input or index out of bounds).
                    if player >= self.config.num_players || player == lp {
                        continue;
                    }
                    self.ensure_frame(frame);
                    let slot = &mut self.history[frame as usize];
                    let cell = &mut slot.players[player as usize];
                    let prev = cell.input;
                    let already_simulated = slot.simulated;
                    cell.input = input;
                    cell.confirmed = true;
                    // Misprediction: a frame we already ran used a value that
                    // the just-arrived real input contradicts. Re-running it
                    // (and everything after) is required. A second copy of an
                    // identical input (idempotent resend, whether previously a
                    // prediction or already confirmed) leaves `prev == input`,
                    // so it is correctly NOT treated as a misprediction.
                    if already_simulated && prev != input {
                        earliest_mispredict =
                            Some(earliest_mispredict.map_or(frame, |e| e.min(frame)));
                    }
                }
                NetMessage::InputAck { frame } => {
                    self.remote_ack_frame =
                        Some(self.remote_ack_frame.map_or(frame, |a| a.max(frame)));
                }
                NetMessage::Checksum {
                    frame,
                    hash,
                    fb_hash,
                } => {
                    self.ensure_frame(frame);
                    // Record the remote value, then compare if our canonical
                    // hash for that confirmed frame is ready. If not, the
                    // stored value is re-checked once we confirm the frame
                    // (see `advance` after `recompute_confirmed`).
                    self.remote_checksums[frame as usize] = Some((hash, fb_hash));
                    if let Some((local, local_fb)) = self.confirmed_hashes[frame as usize] {
                        if local != hash {
                            return Err(NetplayError::Desync {
                                frame,
                                local,
                                remote: hash,
                                same_framebuffer: local_fb == fb_hash,
                            });
                        }
                    }
                    let _ = nes;
                }
                // `Quality`: Stage 1 time-sync is driven by our own
                // confirmed-frame lag (see `should_stall`), so remote Quality
                // hints are accepted but not yet acted on (Stage 2 folds ping +
                // remote frame_advantage into the stall decision).
                // `Roster`: consumed by the N-peer handshake
                // (`mesh_net::MeshHost`/`MeshJoiner`) BEFORE the session exists;
                // a stray one reaching the running session is ignored (it never
                // affects deterministic state).
                NetMessage::Quality { .. } | NetMessage::Roster { .. } => {}
            }
        }

        Ok(earliest_mispredict)
    }

    /// Restore the canonical confirmed checkpoint and replay forward to
    /// `current_frame`, applying each frame's best-known inputs from history.
    ///
    /// As the replay crosses frames that are now confirmed (`f` below the
    /// confirmed boundary), it advances the checkpoint to `f + 1`'s entering
    /// state — a canonical, cross-peer-identical snapshot — and records `f`'s
    /// confirmed state hash for the checksum exchange. Frames at or past the
    /// confirmed boundary replay with predictions and update only the
    /// per-frame snapshot ring (used purely so a subsequent same-tick rollback
    /// could re-derive them).
    ///
    /// Returns the number of frames replayed.
    fn resync(&mut self, nes: &mut Nes) -> Result<u32, NetplayError> {
        // v2.8.0 Phase 3 — take + reinstate instead of cloning the ~250 KiB
        // checkpoint blob on every rollback; `restore_quiet` keeps the
        // user's rewind ring intact (this is a machine-driven rollback on
        // the same timeline, not a user load).
        let (cp_frame, cp_snap) = self
            .checkpoint
            .take()
            .expect("resync called only when a checkpoint exists");
        let restore_result = nes.restore_quiet(&cp_snap);
        self.checkpoint = Some((cp_frame, cp_snap));
        restore_result?;

        // First unconfirmed frame: frames strictly below it are confirmed.
        let confirmed_boundary = self.last_confirmed_frame.map_or(0, |c| c + 1);

        let mut count = 0;
        for f in cp_frame..self.current_frame {
            // v2.8.0 Phase 3 — encode straight into the ring slot's reused
            // buffer (the old path built a full thumbnail-carrying snapshot
            // AND cloned it: two ~320 KiB allocations per replayed frame).
            let mut slot = self.snapshots[f as usize].take().unwrap_or_default();
            nes.snapshot_core_into(&mut slot);
            self.snapshots[f as usize] = Some(slot);
            // A confirmed replay enters `f` (f < boundary) on a canonical
            // base, so its entering *gameplay* state is canonical and cross-
            // peer-identical. Record a digest for verification. (We digest the
            // deterministic gameplay state — framebuffer + cumulative cycle —
            // NOT the full snapshot: the snapshot also carries audio-synthesis
            // transients that legitimately differ with drain history yet never
            // affect future frames, so hashing them would false-desync. The
            // framebuffer + cycle are proven byte-deterministic across
            // restore+replay.)
            if f < confirmed_boundary {
                self.confirmed_entering[f as usize] = Some(Self::gameplay_digest(nes));
            }

            self.apply_and_run(nes, f);
            self.history[f as usize].simulated = true;
            count += 1;

            if f < confirmed_boundary {
                // The resulting state is the canonical entering state of
                // `f + 1`. Advance the checkpoint (we restore the full
                // snapshot, whose audio transients are harmless for replay)
                // and record the confirmed gameplay digest on a checksum
                // boundary.
                if self.config.checksum_interval != 0
                    && f % self.config.checksum_interval == 0
                    && self.confirmed_hashes[f as usize].is_none()
                {
                    self.confirmed_hashes[f as usize] = Some(Self::gameplay_digest_parts(nes));
                }
                // v2.8.0 Phase 3 — advance the checkpoint into its reused
                // buffer (no per-confirmed-frame allocation).
                let mut cp_buf = self.checkpoint.take().map_or_else(Vec::new, |(_, v)| v);
                nes.snapshot_core_into(&mut cp_buf);
                self.checkpoint = Some((f + 1, cp_buf));
            }
        }
        Ok(count)
    }

    /// A deterministic digest of the emulator's *gameplay* state: the
    /// framebuffer plus the cumulative CPU cycle count. Both are byte-
    /// reproducible across save-state restore + replay (unlike the full
    /// snapshot, which also serializes audio-synthesis transients that vary
    /// with audio-drain history but never affect future frames). This is the
    /// right cross-peer desync / sync digest.
    fn gameplay_digest(nes: &Nes) -> u64 {
        Self::gameplay_digest_parts(nes).0
    }

    /// Like [`gameplay_digest`](Self::gameplay_digest) but returns
    /// `(combined, framebuffer_hash)`. The framebuffer hash is kept separately
    /// so a desync can be classified: equal framebuffer hashes with an unequal
    /// combined digest mean the *picture* matched but the cumulative cycle count
    /// diverged (a timing bug); unequal framebuffer hashes mean the rendered
    /// state itself diverged.
    fn gameplay_digest_parts(nes: &Nes) -> (u64, u64) {
        let fb = fnv1a64(nes.framebuffer());
        // Fold in the cycle count so two distinct cycle positions with the
        // same framebuffer still differ.
        let combined = fb ^ nes.cycle().wrapping_mul(0x100_0000_01b3);
        (combined, fb)
    }

    /// Apply every player's input for `frame` from history and run one
    /// emulator frame. Each player index maps directly to its controller port;
    /// the Four Score adapter is enabled when there are more than two players.
    fn apply_and_run(&self, nes: &mut Nes, frame: u32) {
        let slot = self.history[frame as usize];
        let n = self.config.num_players as usize;
        // The session owns whether Four Score is on (it is when >2 players);
        // setting it every frame is idempotent and keeps a rollback's restored
        // state consistent regardless of the live toggle.
        nes.set_four_score(n > 2);
        for (port, cell) in slot.players.iter().enumerate().take(n) {
            nes.set_buttons(port, Buttons::from_bits_truncate(cell.input));
        }
        let _ = nes.run_frame();
    }

    /// Fill each not-yet-confirmed remote player's input for `frame` with a
    /// prediction: repeat that player's most recent confirmed input (the
    /// standard GGPO heuristic). If nothing is known yet, predict "no
    /// buttons". The local player's cell is left untouched (we authored it).
    fn predict_remotes(&mut self, frame: u32) {
        let n = self.config.num_players;
        let lp = self.local_player();
        for player in 0..n {
            if player == lp {
                continue;
            }
            let p = player as usize;
            if self.history[frame as usize].players[p].confirmed {
                continue;
            }
            // Walk back to this player's most recent confirmed input.
            let mut predicted = 0u8;
            let mut f = frame;
            while f > 0 {
                f -= 1;
                if self.history[f as usize].players[p].confirmed {
                    predicted = self.history[f as usize].players[p].input;
                    break;
                }
            }
            self.history[frame as usize].players[p].input = predicted;
        }
    }

    /// Recompute `last_confirmed_frame` = the newest frame `f < current_frame`
    /// for which ALL players' inputs are confirmed, contiguously from 0.
    fn recompute_confirmed(&mut self) {
        let n = self.config.num_players as usize;
        let mut confirmed = self.last_confirmed_frame;
        let start = confirmed.map_or(0, |c| c + 1);
        for f in start..self.current_frame {
            let slot = self.history[f as usize];
            if (0..n).all(|p| slot.players[p].confirmed) {
                confirmed = Some(f);
            } else {
                break;
            }
        }
        self.last_confirmed_frame = confirmed;
    }

    /// How far ahead of the confirmed frame we are running.
    fn frame_advantage(&self) -> i32 {
        let confirmed = self.last_confirmed_frame.map_or(-1i64, i64::from);
        let ahead = i64::from(self.current_frame) - 1 - confirmed;
        i32::try_from(ahead).unwrap_or(i32::MAX)
    }

    /// `true` if producing another frame would exceed the rollback window.
    fn should_stall(&self) -> bool {
        self.frame_advantage() > i32::try_from(self.config.max_rollback_frames).unwrap_or(i32::MAX)
    }

    /// Compare any remote checksums that arrived before our matching
    /// canonical hash was computed, now that more frames may be confirmed.
    fn compare_pending_checksums(&mut self) -> Result<(), NetplayError> {
        let upto = self.last_confirmed_frame.map_or(0, |c| c + 1);
        for f in 0..upto {
            if let (Some(local), Some(remote)) = (
                self.confirmed_hashes[f as usize],
                self.remote_checksums[f as usize],
            ) {
                // Take the remote so we only compare once.
                self.remote_checksums[f as usize] = None;
                let (local_combined, local_fb) = local;
                let (remote_combined, remote_fb) = remote;
                if local_combined != remote_combined {
                    return Err(NetplayError::Desync {
                        frame: f,
                        local: local_combined,
                        remote: remote_combined,
                        same_framebuffer: local_fb == remote_fb,
                    });
                }
            }
        }
        Ok(())
    }

    /// Send a time-sync hint to the peers.
    fn send_quality(&mut self) {
        self.transport.send(&NetMessage::Quality {
            ping_ms: 0,
            frame_advantage: self.frame_advantage(),
        });
    }

    /// Redundantly resend this peer's recent **local** inputs that the remote
    /// has not yet acknowledged (`remote_ack_frame`), so a packet dropped by the
    /// unreliable transport is recovered within a few frames instead of causing
    /// a permanent misprediction + desync. The resend is idempotent on the
    /// receiver (folding in an input it already has is a no-op). The window is
    /// capped by [`INPUT_RESEND_WINDOW`] so a long outage can't burst the whole
    /// input history in one tick; normally — with acks flowing — only the few
    /// in-flight (latency-sized) frames are resent.
    fn resend_unacked_local_inputs(&mut self) {
        let lp = self.local_player();
        let lp_idx = lp as usize;
        // The newest local input we hold (set `input_delay` frames ahead by
        // `add_local_input`).
        let newest = self.current_frame + self.config.input_delay;
        // First un-acked frame (the remote has everything <= remote_ack_frame).
        let first_unacked = self.remote_ack_frame.map_or(0, |a| a.saturating_add(1));
        let start = first_unacked.max(newest.saturating_sub(INPUT_RESEND_WINDOW));
        for frame in start..=newest {
            let idx = frame as usize;
            if idx >= self.history.len() {
                break;
            }
            let slot = self.history[idx].players[lp_idx];
            // Only resend frames we actually authored (confirmed-local).
            if slot.confirmed {
                self.transport.send(&NetMessage::Input {
                    player: lp,
                    frame,
                    input: slot.input,
                });
            }
        }
    }

    /// Send a `Checksum` for every confirmed checksum-boundary frame whose
    /// canonical gameplay digest is ready and hasn't been sent yet.
    ///
    /// We iterate rather than only handling `last_confirmed_frame` because
    /// confirmation can jump past a boundary frame in a single tick (a burst
    /// of inputs arriving), and we must still exchange that boundary's
    /// checksum. The hash is the canonical confirmed digest recorded by
    /// `resync` — a function of confirmed inputs only, so all peers compute
    /// the same value and a mismatch is a true desync.
    fn maybe_send_checksum(&mut self) {
        let interval = self.config.checksum_interval;
        if interval == 0 {
            return;
        }
        let Some(confirmed) = self.last_confirmed_frame else {
            return;
        };
        let mut f = interval; // skip frame 0
        while f <= confirmed {
            if self.local_checksums[f as usize].is_none() {
                if let Some((hash, fb_hash)) = self.confirmed_hashes[f as usize] {
                    self.local_checksums[f as usize] = Some(hash);
                    self.transport.send(&NetMessage::Checksum {
                        frame: f,
                        hash,
                        fb_hash,
                    });
                }
            }
            f += interval;
        }
    }
}

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

use std::collections::{BTreeMap, VecDeque};

use rustynes_core::{Buttons, Nes};

use crate::diagnostics::DesyncDiagnostics;
use crate::message::{IdentityMismatch, NetMessage, SessionIdentity, SyncVerdict};
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
// The whole retransmit window (`INPUT_RESEND_WINDOW + 1` frames) fits one
// protocol-8 `InputBatch`, whose count byte caps it.
const _: () = assert!((INPUT_RESEND_WINDOW as usize) < NetMessage::MAX_INPUT_BATCH);

/// How far past this peer's [`current_frame`](RollbackSession::current_frame)
/// a remote `Input.frame` / `Checksum.frame` may sit before the message is
/// dropped as implausible (AUD-06, v2.9.2 full audit).
///
/// Every per-frame vector in the session is indexed by frame number and grown
/// on demand, so an unchecked remote frame is an allocation size chosen by the
/// peer: one datagram carrying `frame = u32::MAX` asked for ~4 G entries in
/// each of six vectors (an OOM abort on 64-bit, a `usize` overflow on wasm32).
///
/// # Why this value
///
/// A well-behaved peer cannot get far ahead of us. Its input for frame `f` is
/// authored `input_delay` frames early, and it stalls once it runs more than
/// `max_rollback_frames` past its last confirmed frame, which in turn cannot
/// pass the newest input WE have sent (`our current_frame + input_delay`). So
/// the furthest frame it can legitimately name is our current frame plus our
/// `input_delay`, plus its `max_rollback_frames` and its `input_delay`, plus
/// two: 14 frames with the default config. Checksums are only sent for
/// confirmed frames, which sit below that. The session config is not
/// negotiated on the wire, so the peer's values may exceed ours; 1024 frames
/// (about 17 s at 60 Hz) leaves room for a peer configured roughly 70x past
/// the defaults, and [`RollbackSession::max_accepted_frame`] adds this peer's
/// own `input_delay` and `max_rollback_frames` on top so a local config larger
/// than the constant is never undercut. Each frame costs about 110 bytes across
/// the six vectors, so the worst a hostile peer can force is about 115 KiB.
///
/// Dropping an over-window message is safe: inputs are resent every tick
/// until acknowledged, so a genuine one arrives again once it is in range.
/// Mirrors the spectator's `MAX_SPECTATOR_FRAME_LOOKAHEAD` (also 1024).
pub const MAX_SESSION_FRAME_LOOKAHEAD: u32 = 1024;

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

    /// v2.9.8 — the peer runs the same ROM on a differently configured
    /// machine (emulation options, region or cartridge header), so the two
    /// timelines would diverge.
    #[error(
        "configuration mismatch: the peer runs this ROM with different emulation \
         settings (console model, power-on RAM, die revisions, overclock, Four Score, \
         Vs. settings, Game Genie codes, region or header); match them and reconnect"
    )]
    ConfigMismatch,
    /// v3.0.0 (ADR 0045) — the peer runs a version of `RustyNES` that emulates
    /// differently, so the two timelines would diverge whatever the settings.
    #[error("{}", crate::message::emulator_mismatch_text(*ours, *theirs))]
    EmulatorMismatch {
        /// This build's emulation epoch.
        ours: u32,
        /// The peer's epoch; `None` when it predates the epoch (v2.9.9 or earlier).
        theirs: Option<u32>,
    },
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

/// How many confirmed frames' entering digests the session keeps for
/// [`RollbackSession::confirmed_entering_digest`] (v3.4.0, NET-R2-09).
///
/// Until v3.4.0 the digest of every confirmed frame was kept for the whole
/// session, eight bytes plus an `Option` tag per frame, forever. Nothing in the
/// session reads them back: they exist for the caller (the determinism
/// harness, the mobile bridge's debug surface) to compare peers. A caller that
/// wants every frame reads them as they confirm; 256 frames (about four
/// seconds at 60 Hz) is far more than the gap between two reads of a caller
/// that polls once per tick.
pub const CONFIRMED_DIGEST_RETAIN: usize = 256;

/// How far behind the confirmed boundary a checkpoint digest (ours or a
/// peer's) is kept for the desync comparison (v3.4.0, NET-R2-09).
///
/// A `Checksum` is sent once and never resent, and peers confirm a given frame
/// at different moments, so a peer's checksum can arrive a little before or a
/// little after ours for the same frame. Both sides stay within the rollback
/// window of each other (each stalls past it), so any honest pairing happens
/// within a few frames; the same 1024-frame allowance as
/// [`MAX_SESSION_FRAME_LOOKAHEAD`] covers a peer configured far past the
/// defaults. Older entries are dropped: a comparison that never happened is a
/// detection missed, not a desync claimed.
pub const CHECKSUM_RETAIN_FRAMES: u32 = MAX_SESSION_FRAME_LOOKAHEAD;

/// v3.4.0 — how much per-frame state a session holds right now.
///
/// Returned by [`RollbackSession::retained`]. Diagnostic: the long-session test bounds
/// every field by [`RetainedWindow::bound`], which depends on the
/// configuration and never on how long the session has run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetainedWindow {
    /// Frames of input history held (each one `InputSlot`, a few bytes).
    pub history_frames: usize,
    /// Entries in the rollback window: one core snapshot plus a digest per
    /// frame from the first unconfirmed frame to the present.
    pub window_entries: usize,
    /// Bytes of snapshot data in those entries.
    pub window_bytes: usize,
    /// Confirmed entering digests held for
    /// [`RollbackSession::confirmed_entering_digest`].
    pub confirmed_digests: usize,
    /// Checkpoint digests held for the desync comparison, ours and the
    /// peers' pending ones together.
    pub checksums: usize,
}

impl RetainedWindow {
    /// The most of each kind an honest session with `cfg` may hold, whatever
    /// its length. A hostile peer can push `history_frames` further, up to
    /// [`MAX_SESSION_FRAME_LOOKAHEAD`] past the present (see that constant);
    /// this bound is the honest one the long-session test asserts.
    ///
    /// - history: the retransmit window ([`INPUT_RESEND_WINDOW`]) below the
    ///   newest local input, which sits `input_delay` ahead of the present,
    ///   plus a peer's inputs, which can run up to `input_delay +
    ///   max_rollback_frames + 2` ahead of it.
    /// - window: one entry per unconfirmed frame and one for the present; the
    ///   stall keeps at most `max_rollback_frames + 1` frames unconfirmed.
    /// - checksums: one per checksum interval over
    ///   [`CHECKSUM_RETAIN_FRAMES`], for ours and for the peers'.
    #[must_use]
    pub const fn bound(cfg: &SessionConfig) -> Self {
        let delay = cfg.input_delay as usize;
        let rollback = cfg.max_rollback_frames as usize;
        let interval = if cfg.checksum_interval == 0 {
            usize::MAX
        } else {
            cfg.checksum_interval as usize
        };
        Self {
            history_frames: INPUT_RESEND_WINDOW as usize + 2 * delay + 2 * rollback + 4,
            window_entries: rollback + 3,
            window_bytes: usize::MAX,
            confirmed_digests: CONFIRMED_DIGEST_RETAIN,
            checksums: 2 * (CHECKSUM_RETAIN_FRAMES as usize / interval + 2),
        }
    }
}

/// v3.4.0 — the digest of one framebuffer, the picture term of the gameplay
/// digest ([`gameplay_digest`]) and the `fb_hash` a [`NetMessage::Checksum`]
/// carries.
///
/// Until v3.4.0 this was FNV-1a 64 over every byte, one multiply per byte:
/// measured at 209.7 us for one 245,760-byte frame (`benches/rollback_window.rs`,
/// `digest_fnv1a_bytewise`, release build), paid at least once per confirmed
/// frame (NET-R2-05). This one reads the frame as little-endian 64-bit words in
/// four independent lanes, so the four multiply chains run in parallel, and
/// each round rotates as well as multiplies, so a high bit of one word reaches
/// the low bits of the lane before the next word arrives (a bare
/// XOR-and-multiply only carries bits upward, so two equal high-bit flips in
/// two words cancel). The round and the final mix are the familiar
/// multiply-rotate-multiply and xor-shift-multiply shapes of the xxHash family
/// with its published odd constants; this is not XXH64 and is not claimed to
/// be, only a deterministic non-cryptographic checksum. Measured at the
/// figure `docs/netplay-session.md` records.
///
/// Deterministic everywhere: the words are read little-endian whatever the
/// host, so a wasm32 peer and an x86-64 peer agree. Changing this function
/// changes every `Checksum` peers exchange, which is why it moved with
/// protocol 9 and must not move without a protocol bump.
///
/// Not used for any security purpose.
#[must_use]
pub fn framebuffer_digest(data: &[u8]) -> u64 {
    const P1: u64 = 0x9E37_79B1_85EB_CA87;
    const P2: u64 = 0xC2B2_AE3D_27D4_EB4F;
    const P3: u64 = 0x1656_67B1_9E37_79F9;
    #[inline]
    const fn round(acc: u64, word: u64) -> u64 {
        acc.wrapping_add(word.wrapping_mul(P2))
            .rotate_left(31)
            .wrapping_mul(P1)
    }
    let mut lanes = [P1, P2, P3, P1 ^ P2];
    let (blocks, tail) = data.as_chunks::<32>();
    for block in blocks {
        let (words, _) = block.as_chunks::<8>();
        for (lane, word) in lanes.iter_mut().zip(words) {
            *lane = round(*lane, u64::from_le_bytes(*word));
        }
    }
    // A tail shorter than one 32-byte block (none for a NES frame, whose
    // 245,760 bytes are 7,680 blocks) goes into the first lane bytewise.
    for &b in tail {
        lanes[0] = round(lanes[0], u64::from(b));
    }
    let mut h = (data.len() as u64).wrapping_mul(P3);
    for lane in lanes {
        h = round(h, lane);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(P2);
    h ^= h >> 29;
    h = h.wrapping_mul(P3);
    h ^ (h >> 32)
}

/// The deterministic gameplay digest of the machine right now.
///
/// The framebuffer digest XOR a cycle term, so two machines with the same picture
/// at different cycle counts still differ. This is the value
/// [`RollbackSession::confirmed_entering_digest`] reports and a
/// [`NetMessage::Checksum`] compares.
///
/// Framebuffer and cycle, not the full snapshot: the snapshot also carries
/// audio-synthesis transients that differ with how the audio was drained yet
/// never affect a later frame, so hashing them would report a desync that is
/// not one. Both terms are byte-reproducible across a save-state restore and
/// replay.
#[must_use]
pub fn gameplay_digest(nes: &Nes) -> u64 {
    gameplay_digest_parts(nes).0
}

/// [`gameplay_digest`] and the framebuffer digest it is built on, as
/// `(combined, framebuffer)`. Equal framebuffer digests with unequal combined
/// ones mean the same picture at a different cycle count (a timing bug);
/// unequal framebuffer digests mean the rendered state itself diverged.
fn gameplay_digest_parts(nes: &Nes) -> (u64, u64) {
    let fb = framebuffer_digest(nes.framebuffer());
    let combined = fb ^ nes.cycle().wrapping_mul(0x100_0000_01b3);
    (combined, fb)
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

/// v3.4.0 — one entry of the rollback window: the machine as it was entering
/// one frame.
///
/// `snapshot` is a FULL core snapshot, framebuffer included. Round 2 NET-R2-02
/// proposed a slim one (no framebuffer), on the claim that a replay repaints
/// every pixel before anything reads the picture. Measured, that is not true:
/// a `run_frame` boundary can fall a few dots into scanline 0, so the first
/// pixels of a frame's top row are painted by the call before it. On nestest,
/// a slim restore of the state entering frames 1, 2 and 3 followed by one
/// frame left 51, 15 and 6 framebuffer bytes (row 0, x = 1 to 17) holding the
/// later picture, and the per-frame gate
/// (`a_misprediction_forces_a_replay_and_every_frame_matches`) failed at
/// frame 4 as a false desync. A full snapshot restores those pixels too.
///
/// `digest` is [`gameplay_digest_parts`] of the same moment. Keeping it beside
/// the snapshot means a confirmation never has to restore anything to learn
/// a confirmed frame's digest.
struct WindowEntry {
    snapshot: Vec<u8>,
    digest: (u64, u64),
}

/// An `N`-player GGPO-style rollback session driving one [`Nes`] over a
/// [`Transport`].
///
/// Construct with [`Self::new`], feed the local input each tick with
/// [`Self::add_local_input`], then call [`Self::advance`] once per visual
/// frame. See the module docs for the algorithm. `N` (2..=4) is
/// [`SessionConfig::num_players`]; with `N == 2` this is exactly the
/// pairwise 2-player session.
///
/// # The rollback window (v3.4.0)
///
/// The session holds one [`WindowEntry`] per frame from the first unconfirmed
/// frame `B` (= `last_confirmed_frame + 1`) to the present, each recorded the
/// moment the machine entered that frame, during the forward run or a replay.
/// Every frame below `B` ran with every player's real input, so the entry for
/// `B` is canonical: byte-identical on every peer. It is the checkpoint.
///
/// - **A misprediction at frame `m`** restores entry `m` and replays `m` to the
///   present, re-predicting the frames still unconfirmed. Before v3.4.0 a
///   replay always started at the checkpoint.
/// - **A confirmation** moves `B` forward. The entries between the old and the
///   new `B` were recorded on inputs that have now all turned out to be right
///   (otherwise a misprediction would have replayed them), so their digests
///   are the confirmed digests, and the entries below the new `B` are dropped.
///   No frame is re-run. Before v3.4.0 every confirmation restored the
///   checkpoint and replayed to the present, so a session with perfect
///   predictions still re-ran several frames per tick (NET-R2-01).
///
/// Every frame-indexed store is a window too (NET-R2-09): input history from
/// just below the checkpoint (the retransmit window and the prediction walk
/// need no more), the last [`CONFIRMED_DIGEST_RETAIN`] confirmed digests, and
/// checkpoint digests for [`CHECKSUM_RETAIN_FRAMES`]. [`Self::retained`]
/// reports the sizes.
pub struct RollbackSession<T: Transport> {
    config: SessionConfig,
    transport: T,
    identity: SessionIdentity,

    /// The next frame to be produced (== number of frames produced so far).
    current_frame: u32,
    /// Newest frame for which ALL players' real inputs are known.
    last_confirmed_frame: Option<u32>,
    /// Highest frame at which the peer has confirmed receiving our input.
    remote_ack_frame: Option<u32>,
    /// `true` once a peer's `Sync` handshake has been validated.
    synced: bool,

    /// Per-frame input history, `history[i]` being frame `history_base + i`.
    /// Frames below `history_base` were pruned (see `prune_history`); a late
    /// datagram naming one is ignored, as every frame there is confirmed.
    history: VecDeque<InputSlot>,
    /// The frame `history[0]` holds.
    history_base: u32,

    /// The rollback window: `window[i]` is the machine entering frame
    /// `window_base + i`, for every frame from the checkpoint (the first
    /// unconfirmed frame) to the present. Empty until the first frame runs.
    window: VecDeque<WindowEntry>,
    /// The frame `window[0]` holds: the checkpoint frame.
    window_base: u32,
    /// Snapshot buffers of dropped window entries, reused for new ones so the
    /// steady state allocates nothing.
    spare_snapshots: Vec<Vec<u8>>,

    /// The canonical entering digest of the most recent confirmed frames,
    /// `confirmed_entering[i]` being frame `entering_base + i`. A function of
    /// confirmed inputs alone, so identical across peers.
    confirmed_entering: VecDeque<u64>,
    /// The frame `confirmed_entering[0]` holds.
    entering_base: u32,

    /// Our canonical digest at each confirmed checksum-boundary frame `f`: the
    /// state AFTER running `f` (entering `f + 1`), as `(combined,
    /// framebuffer)`. Compared with the peers' `Checksum` for `f`; the
    /// framebuffer half lets a desync say whether the picture diverged (a
    /// state bug) or only the cycle term (a timing bug).
    confirmed_hashes: BTreeMap<u32, (u64, u64)>,

    /// Peers' checksums that arrived before our matching `confirmed_hashes`
    /// entry existed, compared once it does.
    remote_checksums: BTreeMap<u32, (u64, u64)>,

    /// The next checksum-boundary frame whose `Checksum` we have not sent.
    next_checksum_frame: u32,

    /// Observational desync telemetry (v1.3.0 Workstream G1). Records every
    /// confirmed-frame checksum comparison — match or mismatch — for the
    /// frontend's `DesyncMonitor`-style panel. It only reads the digests the
    /// session already computes and exchanges, never feeding back into the
    /// rollback algorithm, so it cannot perturb determinism or correctness.
    diagnostics: DesyncDiagnostics,
}

impl<T: Transport> RollbackSession<T> {
    /// Create a session for `identity` ([`SessionIdentity::of`] the local
    /// machine). Sends the opening `Sync` handshake immediately.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if `config.num_players` is not in `2..=4` or
    /// `config.local_player >= config.num_players`.
    pub fn new(config: SessionConfig, mut transport: T, identity: SessionIdentity) -> Self {
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
            identity,
        });
        let mut session = Self {
            config,
            transport,
            identity,
            current_frame: 0,
            last_confirmed_frame: None,
            remote_ack_frame: None,
            synced: false,
            history: VecDeque::new(),
            history_base: 0,
            window: VecDeque::new(),
            window_base: 0,
            spare_snapshots: Vec::new(),
            confirmed_entering: VecDeque::new(),
            entering_base: 0,
            confirmed_hashes: BTreeMap::new(),
            remote_checksums: BTreeMap::new(),
            next_checksum_frame: config.checksum_interval,
            diagnostics: DesyncDiagnostics::new(),
        };
        // GGPO input-delay convention: the first `input_delay` frames have no
        // buffered local input, so they run with "no buttons" and are
        // immediately confirmable. Seed (and announce) those empty inputs for
        // this peer's local player so confirmation can begin from frame 0 once
        // the other peers' matching empty inputs arrive.
        let lp = config.local_player;
        for f in 0..config.input_delay {
            if let Some(slot) = session.slot_mut(f) {
                slot.players[lp as usize] = PlayerInput {
                    input: 0,
                    confirmed: true,
                };
            }
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

    /// The canonical gameplay digest ([`gameplay_digest`]) of the
    /// deterministic state *entering* `frame`, available once every frame
    /// before it is confirmed. It is a function of confirmed inputs alone, so
    /// it is identical across peers — the basis for cross-peer state
    /// verification.
    ///
    /// Since v3.4.0 only the most recent [`CONFIRMED_DIGEST_RETAIN`] confirmed
    /// frames are kept, so `None` means "not confirmed yet" or "confirmed
    /// long enough ago to have been dropped"; a caller that wants every frame
    /// reads each one as it confirms. Primarily for tests / debugging.
    #[must_use]
    pub fn confirmed_entering_digest(&self, frame: u32) -> Option<u64> {
        let i = frame.checked_sub(self.entering_base)? as usize;
        self.confirmed_entering.get(i).copied()
    }

    /// The confirmed per-port input applied at `frame`, if every player is
    /// confirmed and the frame is still in the history window. The returned
    /// array is indexed by canonical controller port (port 0 = P1, … port 3 =
    /// Four Score P4), independent of which player is local. Ports `>=
    /// num_players` are `0`. For tests / debugging.
    #[must_use]
    pub fn confirmed_input(&self, frame: u32) -> Option<[u8; MAX_PLAYERS]> {
        let slot = self.slot(frame).copied()?;
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

    /// v3.4.0 — how much per-frame state the session holds now. See
    /// [`RetainedWindow`].
    #[must_use]
    pub fn retained(&self) -> RetainedWindow {
        RetainedWindow {
            history_frames: self.history.len(),
            window_entries: self.window.len(),
            window_bytes: self.window.iter().map(|e| e.snapshot.len()).sum(),
            confirmed_digests: self.confirmed_entering.len(),
            checksums: self.confirmed_hashes.len() + self.remote_checksums.len(),
        }
    }

    /// `true` once a peer's `Sync` handshake has been validated.
    #[must_use]
    pub const fn is_synced(&self) -> bool {
        self.synced
    }

    /// This peer's player index (`0..num_players`): which controller port this
    /// peer drives (0 = P1 / `$4016`, 1 = P2 / `$4017`, 2/3 = Four Score). For
    /// the netplay panel's input-topology display.
    #[must_use]
    pub const fn local_player(&self) -> u8 {
        self.config.local_player
    }

    /// Borrow the read-only desync diagnostics (CRC-match history, first-desync
    /// frame, consecutive-mismatch count, last local-vs-remote CRC). Surfaced by
    /// the frontend netplay panel (v1.3.0 Workstream G1).
    #[must_use]
    pub const fn diagnostics(&self) -> &DesyncDiagnostics {
        &self.diagnostics
    }

    /// Borrow the transport (e.g. to inspect link stats). Mainly for tests.
    pub const fn transport(&self) -> &T {
        &self.transport
    }

    /// Record the local player's input for the frame it will apply to
    /// (`current_frame + input_delay`). Call once per tick before
    /// [`Self::advance`].
    pub fn add_local_input(&mut self, input: Buttons) {
        let target = self.current_frame + self.config.input_delay;
        let lp = self.local_player();
        if let Some(slot) = self.slot_mut(target) {
            slot.players[lp as usize] = PlayerInput {
                input: input.bits(),
                confirmed: true,
            };
        }
        // Immediately tell every peer; the retransmit window
        // (`resend_unacked_local_inputs`) covers a loss. The transport fans
        // this out to all other players (mesh topology).
        self.transport.send(&NetMessage::Input {
            player: lp,
            frame: target,
            input: input.bits(),
        });
    }

    /// The newest frame a remote `Input` or `Checksum` may name and still be
    /// folded in: [`current_frame`](Self::current_frame) plus this peer's
    /// `input_delay + max_rollback_frames` plus [`MAX_SESSION_FRAME_LOOKAHEAD`]
    /// (saturating). See the constant for the derivation. Anything later is
    /// dropped before it can size an allocation.
    ///
    /// Capped one short of `u32::MAX`: the private `slot_mut` sizes the
    /// history as `frame - history_base + 1`, and saturation alone would let
    /// the window reach exactly `u32::MAX`. Unreachable in play (the clock
    /// would have to run for over two years at 60 fps), but this is the guard,
    /// so it holds at its own endpoint.
    #[must_use]
    pub const fn max_accepted_frame(&self) -> u32 {
        let window = self
            .current_frame
            .saturating_add(self.config.input_delay)
            .saturating_add(self.config.max_rollback_frames)
            .saturating_add(MAX_SESSION_FRAME_LOOKAHEAD);
        if window == u32::MAX {
            u32::MAX - 1
        } else {
            window
        }
    }

    /// The history slot for `frame`, if it is inside the history window.
    fn slot(&self, frame: u32) -> Option<&InputSlot> {
        let i = frame.checked_sub(self.history_base)? as usize;
        self.history.get(i)
    }

    /// The history slot for `frame`, growing the window to reach it. `None`
    /// for a frame below the window (pruned, so already confirmed).
    ///
    /// Callers pass either a locally-derived frame or a remote one that has
    /// already been checked against [`max_accepted_frame`](Self::max_accepted_frame),
    /// so the growth is bounded.
    fn slot_mut(&mut self, frame: u32) -> Option<&mut InputSlot> {
        let i = frame.checked_sub(self.history_base)? as usize;
        if self.history.len() <= i {
            self.history.resize(i + 1, InputSlot::default());
        }
        self.history.get_mut(i)
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
        //    confirmed prefix.
        self.recompute_confirmed();

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

        // 3. Roll back + re-simulate when a real input contradicted the
        //    prediction an already-run frame used. The replay starts at that
        //    frame's window entry, not at the checkpoint: the frames before it
        //    ran on inputs nothing has contradicted. A confirmation alone
        //    replays nothing (v3.4.0, NET-R2-01); step 3b promotes what the
        //    window already holds.
        let mispredicted = earliest_mispredict.filter(|&m| m < self.current_frame);
        let mut rolled_back = false;
        let mut resimulated = 0u32;
        if let Some(m) = mispredicted
            && !self.window.is_empty()
        {
            // A mispredicted frame is never below the checkpoint: every frame
            // there is confirmed, and a confirmed cell is never rewritten
            // (`ingest_single_input`). The clamp is defence in depth.
            resimulated = self.replay_from(nes, m.max(self.window_base))?;
            rolled_back = true;
        }

        // 3b. Promote the frames that just became confirmed: record their
        //     canonical digests and drop their window entries.
        self.promote_confirmed();

        // 3c. Resolve any remote checksums now that more frames are confirmed.
        self.compare_pending_checksums()?;

        // 3d. Redundantly resend the local inputs the remote has NOT yet
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
        self.predict_remotes(frame);

        // 5. Run one frame, recording the window entry for the frame it enters.
        //    The very first frame also records its own entering entry: frame
        //    0's entering state is the deterministic power-on, identical
        //    across peers, and it is the first checkpoint.
        if self.window.is_empty() {
            self.window_base = frame;
            self.record_entry(nes, frame);
        }
        self.apply_and_run(nes, frame);
        if let Some(slot) = self.slot_mut(frame) {
            slot.simulated = true;
        }
        self.record_entry(nes, frame + 1);

        // 6. Send our (already-confirmed-local) input for this frame to the
        //    peers. (add_local_input also sent it for the input-delay target;
        //    this resend covers the immediate frame for the zero-delay case.)
        let lp = self.local_player() as usize;
        if let Some(cell) = self.slot(frame).map(|s| s.players[lp])
            && cell.confirmed
        {
            self.transport.send(&NetMessage::Input {
                player: self.local_player(),
                frame,
                input: cell.input,
            });
        }

        // 7. Advance the clock.
        self.current_frame += 1;

        // 8. Periodically checksum a confirmed frame and exchange it.
        self.maybe_send_checksum();

        // 9. Drop the history the session no longer reads.
        self.prune_history();

        Ok(AdvanceOutcome {
            produced_frame: true,
            rolled_back,
            resimulated_frames: resimulated,
            frame,
        })
    }

    /// Fold a single remote input into the history, returning the frame it
    /// mispredicted (if any).
    ///
    /// The first real value for a cell is final (v3.4.0): a different value
    /// for a cell already confirmed is ignored. An honest peer never sends
    /// one, and honouring it would rewrite a frame that may already sit below
    /// the checkpoint, where no window entry exists to replay from; the
    /// pre-v3.4.0 session took it, replayed from the checkpoint above it, and
    /// left the peers to find the difference at the next checksum.
    fn ingest_single_input(&mut self, player: u8, frame: u32, input: u8) -> Option<u32> {
        let lp = self.local_player();
        // Ignore an out-of-range or self-addressed player index.
        if player >= self.config.num_players || player == lp {
            return None;
        }
        // Drop a frame past the lookahead window before it can size an allocation.
        if frame > self.max_accepted_frame() {
            return None;
        }
        // A frame below the history window is confirmed and was pruned.
        let slot = self.slot_mut(frame)?;
        let already_simulated = slot.simulated;
        let cell = &mut slot.players[player as usize];
        if cell.confirmed {
            return None;
        }
        let prev = cell.input;
        cell.input = input;
        cell.confirmed = true;
        // Misprediction: a frame we already ran used a value that the
        // just-arrived real input contradicts. Re-running it (and everything
        // after) is required. A real input equal to the prediction leaves
        // `prev == input`, so it is correctly NOT a misprediction.
        if already_simulated && prev != input {
            Some(frame)
        } else {
            None
        }
    }

    /// Poll the transport and fold in every message. Returns the earliest
    /// frame (if any) whose previously-used prediction was contradicted by a
    /// newly-arrived real remote input.
    fn ingest(&mut self, nes: &Nes) -> Result<Option<u32>, NetplayError> {
        let mut earliest_mispredict: Option<u32> = None;
        let messages = self.transport.poll();

        for msg in messages {
            match msg {
                NetMessage::Sync { magic, identity } => {
                    match self.identity.check_sync(magic, &identity) {
                        SyncVerdict::Ignore => continue,
                        SyncVerdict::Accept => {}
                        SyncVerdict::Refuse(IdentityMismatch::Rom) => {
                            return Err(NetplayError::RomMismatch);
                        }
                        SyncVerdict::Refuse(IdentityMismatch::Config) => {
                            return Err(NetplayError::ConfigMismatch);
                        }
                        SyncVerdict::Refuse(IdentityMismatch::Emulator { ours, theirs }) => {
                            return Err(NetplayError::EmulatorMismatch { ours, theirs });
                        }
                    }
                    self.synced = true;
                }
                NetMessage::Input {
                    player,
                    frame,
                    input,
                } => {
                    if let Some(mispredict) = self.ingest_single_input(player, frame, input) {
                        earliest_mispredict =
                            Some(earliest_mispredict.map_or(mispredict, |e| e.min(mispredict)));
                    }
                }
                NetMessage::InputBatch {
                    player,
                    start_frame,
                    inputs,
                } => {
                    for (offset, input) in inputs.into_iter().enumerate() {
                        #[allow(clippy::cast_possible_truncation)]
                        let frame = start_frame.saturating_add(offset as u32);
                        if let Some(mispredict) = self.ingest_single_input(player, frame, input) {
                            earliest_mispredict =
                                Some(earliest_mispredict.map_or(mispredict, |e| e.min(mispredict)));
                        }
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
                    self.ingest_checksum(frame, hash, fb_hash)?;
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

    /// Record a peer's checksum for `frame`, comparing it now if our digest
    /// for the frame exists.
    fn ingest_checksum(&mut self, frame: u32, hash: u64, fb_hash: u64) -> Result<(), NetplayError> {
        let interval = self.config.checksum_interval;
        // Same window as `Input` (AUD-06). A peer only checksums confirmed
        // frames, which are never past our inputs, so a legitimate checksum is
        // always well inside it. A frame that is not one of our boundaries
        // (or any frame, with checksums off) has no digest of ours to meet,
        // and one older than the retention window is past comparing: neither
        // is kept, which is what bounds `remote_checksums`.
        let boundary = self.last_confirmed_frame.map_or(0, |c| c + 1);
        if frame > self.max_accepted_frame()
            || interval == 0
            || !frame.is_multiple_of(interval)
            || frame.saturating_add(CHECKSUM_RETAIN_FRAMES) < boundary
        {
            return Ok(());
        }
        let Some(&(local, local_fb)) = self.confirmed_hashes.get(&frame) else {
            // Ours is not ready: compared once we confirm the frame
            // (`compare_pending_checksums`).
            self.remote_checksums.insert(frame, (hash, fb_hash));
            return Ok(());
        };
        // Observational: record the compare (match or mismatch) before any
        // fatal exit.
        self.diagnostics
            .record(frame, local, hash, local_fb, fb_hash);
        if local != hash {
            return Err(NetplayError::Desync {
                frame,
                local,
                remote: hash,
                same_framebuffer: local_fb == fb_hash,
            });
        }
        Ok(())
    }

    /// Restore the window entry for frame `from` and replay forward to the
    /// present, re-predicting the frames still unconfirmed and re-recording the
    /// window entry for every frame the replay enters. Returns the number of
    /// frames replayed.
    ///
    /// Re-predicting matters as much as the replay: the frames after `from`
    /// were first predicted before `from`'s real input was known, so they
    /// still repeat the stale value; without a new prediction the next
    /// arrival would contradict each of them in turn and replay again.
    fn replay_from(&mut self, nes: &mut Nes, from: u32) -> Result<u32, NetplayError> {
        let idx = (from - self.window_base) as usize;
        // `restore_quiet` keeps the user's rewind ring intact: this is a
        // machine-driven rollback on the same timeline, not a user load.
        nes.restore_quiet(&self.window[idx].snapshot)?;
        let mut count = 0;
        for f in from..self.current_frame {
            self.predict_remotes(f);
            self.apply_and_run(nes, f);
            if let Some(slot) = self.slot_mut(f) {
                slot.simulated = true;
            }
            self.record_entry(nes, f + 1);
            count += 1;
        }
        Ok(count)
    }

    /// Record the window entry for `frame`: the machine as it is now, which
    /// must be the state entering `frame`. Overwrites the entry if the frame
    /// already has one (a replay), else appends it (the frame after the last
    /// one recorded), reusing a spare buffer.
    fn record_entry(&mut self, nes: &Nes, frame: u32) {
        let idx = (frame - self.window_base) as usize;
        let digest = gameplay_digest_parts(nes);
        if let Some(entry) = self.window.get_mut(idx) {
            // A full core snapshot, framebuffer included (see `WindowEntry`).
            nes.snapshot_core_into(&mut entry.snapshot);
            entry.digest = digest;
        } else {
            debug_assert_eq!(idx, self.window.len(), "window entries are contiguous");
            let mut snapshot = self.spare_snapshots.pop().unwrap_or_default();
            nes.snapshot_core_into(&mut snapshot);
            self.window.push_back(WindowEntry { snapshot, digest });
        }
    }

    /// Move the checkpoint up to the first unconfirmed frame: record the
    /// canonical digests of the frames that became confirmed since the last
    /// call, from the window entries recorded while they ran, then drop those
    /// entries.
    ///
    /// Sound because a window entry for frame `g` was recorded on the inputs
    /// history held for every frame before `g` at the time, and any of those
    /// that later turned out wrong was a misprediction, which replayed from
    /// it and re-recorded every entry after it. So once every frame before `g`
    /// is confirmed, entry `g` was recorded on exactly the confirmed inputs:
    /// the same machine every peer reaches.
    fn promote_confirmed(&mut self) {
        let new_base = self.last_confirmed_frame.map_or(0, |c| c + 1);
        if self.window.is_empty() || new_base <= self.window_base {
            return;
        }
        // Confirmation never passes the present, and the window always holds
        // an entry for the present, so every index below is in range.
        debug_assert!(((new_base - self.window_base) as usize) < self.window.len());
        let interval = self.config.checksum_interval;
        for g in self.window_base..new_base {
            let i = (g - self.window_base) as usize;
            let entering = self.window[i].digest.0;
            if self.confirmed_entering.is_empty() {
                self.entering_base = g;
            }
            debug_assert_eq!(
                self.entering_base as usize + self.confirmed_entering.len(),
                g as usize,
                "confirmed digests are recorded once per frame, in order"
            );
            self.confirmed_entering.push_back(entering);
            if self.confirmed_entering.len() > CONFIRMED_DIGEST_RETAIN {
                self.confirmed_entering.pop_front();
                self.entering_base += 1;
            }
            // The checkpoint digest for a boundary frame is the state AFTER it
            // ran: the entry for the frame after it.
            if interval != 0 && g.is_multiple_of(interval) {
                let after = self.window[i + 1].digest;
                self.confirmed_hashes.entry(g).or_insert(after);
            }
        }
        while self.window_base < new_base {
            if let Some(entry) = self.window.pop_front() {
                self.spare_snapshots.push(entry.snapshot);
            }
            self.window_base += 1;
        }
        // Keep no more spare buffers than the window can use at once.
        self.spare_snapshots
            .truncate(self.config.max_rollback_frames as usize + 3);
        // Checkpoint digests older than the retention window cannot meet a
        // peer's any more.
        let floor = new_base.saturating_sub(CHECKSUM_RETAIN_FRAMES);
        self.confirmed_hashes = self.confirmed_hashes.split_off(&floor);
        self.remote_checksums = self.remote_checksums.split_off(&floor);
    }

    /// Drop input history the session will not read again. Two readers reach
    /// back into it: the prediction walk, which stops at the newest confirmed
    /// input of each player (never below the last confirmed frame), and the
    /// retransmit window, [`INPUT_RESEND_WINDOW`] frames below the newest
    /// local input. Everything below both goes.
    fn prune_history(&mut self) {
        let last_confirmed = self.last_confirmed_frame.unwrap_or(0);
        let resend_floor =
            (self.current_frame + self.config.input_delay).saturating_sub(INPUT_RESEND_WINDOW);
        let floor = last_confirmed.min(resend_floor);
        while self.history_base < floor && self.history.pop_front().is_some() {
            self.history_base += 1;
        }
    }

    /// Apply every player's input for `frame` from history and run one
    /// emulator frame. Each player index maps directly to its controller port;
    /// the Four Score adapter is enabled when there are more than two players.
    fn apply_and_run(&self, nes: &mut Nes, frame: u32) {
        let slot = self.slot(frame).copied().unwrap_or_default();
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
        if self.slot_mut(frame).is_none() {
            return;
        }
        for player in 0..n {
            if player == lp {
                continue;
            }
            let p = player as usize;
            if self.slot(frame).is_some_and(|s| s.players[p].confirmed) {
                continue;
            }
            // Walk back to this player's most recent confirmed input. The
            // walk never needs history below the last confirmed frame, where
            // every input is confirmed, so the pruned prefix is never missed.
            let mut predicted = 0u8;
            let mut f = frame;
            while f > self.history_base {
                f -= 1;
                if let Some(cell) = self.slot(f).map(|s| s.players[p])
                    && cell.confirmed
                {
                    predicted = cell.input;
                    break;
                }
            }
            if let Some(slot) = self.slot_mut(frame) {
                slot.players[p].input = predicted;
            }
        }
    }

    /// Recompute `last_confirmed_frame` = the newest frame `f < current_frame`
    /// for which ALL players' inputs are confirmed, contiguously from 0.
    fn recompute_confirmed(&mut self) {
        let n = self.config.num_players as usize;
        let mut confirmed = self.last_confirmed_frame;
        let start = confirmed.map_or(0, |c| c + 1);
        for f in start..self.current_frame {
            match self.slot(f) {
                Some(slot) if (0..n).all(|p| slot.players[p].confirmed) => confirmed = Some(f),
                _ => break,
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

    /// Compare the remote checksums that arrived before our matching digest
    /// existed, now that more frames may be confirmed. A remote checksum for a
    /// confirmed frame with no digest of ours (none was ever recorded, or it
    /// aged out) can never be compared and is dropped.
    fn compare_pending_checksums(&mut self) -> Result<(), NetplayError> {
        let upto = self.last_confirmed_frame.map_or(0, |c| c + 1);
        while let Some((&f, &(remote_combined, remote_fb))) =
            self.remote_checksums.first_key_value()
        {
            if f >= upto {
                break;
            }
            self.remote_checksums.remove(&f);
            let Some(&(local_combined, local_fb)) = self.confirmed_hashes.get(&f) else {
                continue;
            };
            // Observational: record the compare (match or mismatch) before
            // any fatal exit.
            self.diagnostics
                .record(f, local_combined, remote_combined, local_fb, remote_fb);
            if local_combined != remote_combined {
                return Err(NetplayError::Desync {
                    frame: f,
                    local: local_combined,
                    remote: remote_combined,
                    same_framebuffer: local_fb == remote_fb,
                });
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
        let start = first_unacked
            .max(newest.saturating_sub(INPUT_RESEND_WINDOW))
            .max(self.history_base);
        if start > newest {
            return;
        }
        // v3.2.0 (protocol 8): one `InputBatch` per run of consecutive
        // confirmed (locally authored) frames, instead of one `Input` per
        // frame. A frame we did not author ends a run and is skipped, as the
        // per-frame loop skipped it; the next confirmed frame starts a new
        // run, so every authored frame in the window is still resent. (The
        // first form of this batching stopped at the first gap and dropped
        // the rest of the window.) The window fits one batch (const assert
        // beside INPUT_RESEND_WINDOW).
        let mut run_start = start;
        let mut run: Vec<u8> = Vec::new();
        for frame in start..=newest {
            let Some(slot) = self.slot(frame).map(|s| s.players[lp_idx]) else {
                break;
            };
            if slot.confirmed {
                if run.is_empty() {
                    run_start = frame;
                }
                run.push(slot.input);
            } else if !run.is_empty() {
                self.transport.send(&NetMessage::InputBatch {
                    player: lp,
                    start_frame: run_start,
                    inputs: core::mem::take(&mut run),
                });
            }
        }
        if !run.is_empty() {
            self.transport.send(&NetMessage::InputBatch {
                player: lp,
                start_frame: run_start,
                inputs: run,
            });
        }
    }

    /// Send a `Checksum` for every confirmed checksum-boundary frame not sent
    /// yet.
    ///
    /// A cursor walks the boundaries rather than only handling
    /// `last_confirmed_frame`, because confirmation can jump past a boundary
    /// frame in a single tick (a burst of inputs arriving), and that
    /// boundary's checksum must still go out. The hash is the canonical
    /// confirmed digest `promote_confirmed` recorded — a function of
    /// confirmed inputs only, so all peers compute the same value and a
    /// mismatch is a true desync. (Before v3.4.0 this loop started at frame
    /// zero on every tick, which grew with the session.)
    fn maybe_send_checksum(&mut self) {
        let interval = self.config.checksum_interval;
        if interval == 0 {
            return;
        }
        let Some(confirmed) = self.last_confirmed_frame else {
            return;
        };
        while self.next_checksum_frame <= confirmed {
            let f = self.next_checksum_frame;
            if let Some(&(hash, fb_hash)) = self.confirmed_hashes.get(&f) {
                self.transport.send(&NetMessage::Checksum {
                    frame: f,
                    hash,
                    fb_hash,
                });
            }
            let Some(next) = f.checked_add(interval) else {
                // The frame space is exhausted; nothing is left to send.
                self.next_checksum_frame = u32::MAX;
                break;
            };
            self.next_checksum_frame = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{LinkConditions, MemoryTransport};

    // A minimal NROM (infinite loop) so a session can advance frames without a
    // real game. Same fixture as the spectator tests.
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

    /// Every frame-indexed store a remote frame number could size: the input
    /// history and the two checksum maps (v3.4.0: the history is a window and
    /// the checksums are maps, so a far frame must not grow either).
    fn per_frame_lens<T: Transport>(s: &RollbackSession<T>) -> [usize; 3] {
        [
            s.history.len(),
            s.confirmed_hashes.len(),
            s.remote_checksums.len(),
        ]
    }

    /// AUD-06 (v2.9.2 full audit): a remote peer's `Input.frame` /
    /// `Checksum.frame` sized six `Vec`s with no bound, so one datagram with
    /// `frame = u32::MAX` asked for ~4 G entries per vector (an OOM abort on
    /// 64-bit; a `usize` overflow on wasm32). A frame past the lookahead window
    /// must be dropped WITHOUT growing any per-frame vector.
    #[test]
    fn remote_frame_beyond_lookahead_is_dropped_without_allocating() {
        let rom = synth_nrom();
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
        let (link, mut peer) = MemoryTransport::pair(LinkConditions::PERFECT, 11);
        let mut session = RollbackSession::new(SessionConfig::default(), link, hash);
        let mut nes = Nes::from_rom(&rom).unwrap();

        // A moderately far frame first: large enough to be implausible, small
        // enough (~20 MiB if accepted) that an unguarded build fails the
        // assertion below instead of aborting the test process.
        let far = 200_000;
        peer.send(&NetMessage::Input {
            player: 1,
            frame: far,
            input: 0xFF,
        });
        peer.send(&NetMessage::Checksum {
            frame: far,
            hash: 1,
            fb_hash: 2,
        });
        session.advance(&mut nes).expect("advance");
        let bound = session.max_accepted_frame() as usize + 1;
        for len in per_frame_lens(&session) {
            assert!(
                len <= bound,
                "a remote frame index must not size the per-frame vectors (len = {len}, bound = {bound})"
            );
        }

        // The worst case: the largest representable frame, for both message
        // kinds that carry one.
        peer.send(&NetMessage::Input {
            player: 1,
            frame: u32::MAX,
            input: 0xFF,
        });
        peer.send(&NetMessage::Checksum {
            frame: u32::MAX,
            hash: 1,
            fb_hash: 2,
        });
        session.advance(&mut nes).expect("advance");
        let bound = session.max_accepted_frame() as usize + 1;
        for len in per_frame_lens(&session) {
            assert!(len <= bound, "len = {len}, bound = {bound}");
        }
    }

    /// The window edge, pinned both ways: the last accepted frame is folded
    /// into history (so legitimate far-ahead traffic survives), the next one is
    /// dropped. Mutating the comparison in either direction fails one half.
    #[test]
    fn lookahead_window_never_reaches_u32_max() {
        // `slot_mut` computes `frame - base + 1`, which overflows on a
        // 32-bit `usize` for `frame == u32::MAX`. The window saturates, so a
        // session near the end of the frame space must still cap it one short
        // (Copilot on #563). Unreachable in play -- the clock would have to
        // run for over two years at 60 fps -- but the bound is the guard, so
        // it must hold at its own endpoint.
        let rom = synth_nrom();
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
        let (link, _peer) = MemoryTransport::pair(LinkConditions::PERFECT, 12);
        let mut session = RollbackSession::new(SessionConfig::default(), link, hash);
        session.current_frame = u32::MAX - 3;
        assert_eq!(session.max_accepted_frame(), u32::MAX - 1);
    }

    #[test]
    fn lookahead_window_edge_is_inclusive() {
        let rom = synth_nrom();
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
        let (link, mut peer) = MemoryTransport::pair(LinkConditions::PERFECT, 12);
        let mut session = RollbackSession::new(SessionConfig::default(), link, hash);
        let mut nes = Nes::from_rom(&rom).unwrap();

        let edge = session.max_accepted_frame();
        peer.send(&NetMessage::Input {
            player: 1,
            frame: edge,
            input: 0x42,
        });
        peer.send(&NetMessage::Input {
            player: 1,
            frame: edge + 1,
            input: 0x24,
        });
        // `advance` ingests before it moves the clock, so both frames are
        // judged against the window computed above.
        session.advance(&mut nes).expect("advance");
        let accepted = session.slot(edge).expect("edge slot").players[1];
        assert!(
            accepted.confirmed,
            "the window edge itself must be accepted"
        );
        assert_eq!(accepted.input, 0x42);
        assert_eq!(
            session.history.len(),
            edge as usize + 1,
            "one past the edge must not grow history"
        );
    }

    /// The bound must clear everything a well-behaved peer can send: its input
    /// runs at most `input_delay + max_rollback_frames + 2` frames past ours
    /// (see `max_accepted_frame`), and it may run a larger config than ours.
    #[test]
    fn window_covers_the_protocol_maximum_with_slack() {
        let rom = synth_nrom();
        let hash = SessionIdentity::of(&Nes::from_rom(&rom).unwrap());
        let (link, _peer) = MemoryTransport::pair(LinkConditions::PERFECT, 13);
        let cfg = SessionConfig::default();
        let session = RollbackSession::new(cfg, link, hash);
        let protocol_max = 2 * cfg.input_delay + cfg.max_rollback_frames + 2;
        assert!(session.max_accepted_frame() - session.current_frame() >= protocol_max);
        assert!(
            session.max_accepted_frame() - session.current_frame() >= MAX_SESSION_FRAME_LOOKAHEAD
        );
    }
}

//! Edge-relayed spectating (v3.7.0, `T-NETPLAY-EDGE-SPECTATE`).
//!
//! The direct spectator ([`crate::SpectatorSession`]) needs a leg of the
//! players' own transport, so the players carry every viewer's traffic. Here
//! the players publish their **confirmed** inputs once, to a relay at the
//! network edge (a Cloudflare Durable Object, `services/hosted/src/spectate.ts`),
//! which fans them out to any number of viewers. Each viewer runs the core
//! locally from that stream, so it can seek, rewind and pause on its own
//! timeline without touching the match.
//!
//! # What travels
//!
//! Inputs only. The relay carries the publisher's [`NetMessage::Sync`] (the
//! match's [`SessionIdentity`], so a viewer refuses another game, another
//! configuration or another emulator epoch) and
//! [`NetMessage::ConfirmedInputs`], every player's confirmed input for
//! consecutive frames. Never a frame, a sample or a state: the relay refuses
//! anything else. A viewer joining mid-session receives the whole input log
//! from frame 0 and catches up by running it; that costs CPU time on the
//! viewer, never bandwidth on the relay beyond about 2 to 4 bytes a frame.
//!
//! The other way to join mid-session, over a direct link, is from a
//! **checkpoint**: a core snapshot of the machine entering some frame
//! ([`NetMessage::StateCheckpoint`]) followed by the inputs from that frame.
//! Any viewer that has caught up holds such a machine, since every frame it
//! shows is the canonical one, so [`EdgeSpectator::checkpoint`] serves one.
//!
//! # Why a viewer matches the players
//!
//! A confirmed frame ran with every player's real input on every peer, and the
//! core's determinism contract makes the same machine from the same inputs, so
//! a viewer that starts from the same deterministic power-on (or from a
//! checkpoint of a confirmed frame) and applies the same confirmed inputs
//! reaches the players' confirmed state byte for byte, every frame. The viewer
//! predicts nothing and never rolls back.
//!
//! # Seek, rewind and views
//!
//! The viewer keeps the whole input log it has received and a keyframe (a
//! core snapshot) every [`EdgeConfig::keyframe_interval`] frames. A seek
//! restores the nearest keyframe at or before the target and replays the log
//! to it; rewind is a seek backwards. [`View::Live`] follows the newest
//! confirmed frame; [`View::Paused`] holds the shown frame where a seek or a
//! pause left it.

use std::collections::BTreeMap;

use rustynes_core::{Buttons, Nes};

use crate::message::KEY_SHARE_LEN;
use crate::message::{IdentityMismatch, NetMessage, SessionIdentity, SyncVerdict};
use crate::session::MAX_PLAYERS;
use crate::session::RollbackSession;
use crate::transport::Transport;

/// The largest checkpoint a viewer accepts: a full core snapshot is about
/// 256 KiB, and a mapper with large RAM stays well under a megabyte.
pub const MAX_CHECKPOINT_LEN: usize = 4 * 1024 * 1024;

/// The most frames a viewer keeps in its input log: one hour at 60 Hz. A
/// stream longer than that is refused past it ([`EdgeSpectator::overflowed`]).
pub const MAX_LOG_FRAMES: usize = 60 * 60 * 60;

/// Configuration for an [`EdgeSpectator`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeConfig {
    /// A keyframe every this many frames (at least 1): the cost of a seek is
    /// at most this many replayed frames.
    pub keyframe_interval: u32,
    /// The most keyframes kept (at least 1); the oldest go first, except the
    /// join point, which is always kept so any frame stays reachable.
    pub max_keyframes: usize,
}

impl Default for EdgeConfig {
    fn default() -> Self {
        Self {
            // Five seconds: a seek replays at most 300 frames.
            keyframe_interval: 300,
            // One hour of five-second keyframes, about 180 MiB at most; the
            // frontend lowers it.
            max_keyframes: 720,
        }
    }
}

/// What the viewer shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// Follow the newest confirmed frame.
    Live,
    /// Hold the shown frame.
    Paused,
}

/// Why a seek could not happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SeekError {
    /// The frame is past the newest confirmed one, or before the join point.
    OutOfRange {
        /// The first frame reachable.
        first: u32,
        /// The newest frame reachable (the machine entering it).
        last: u32,
    },
    /// The viewer has not joined yet (no checkpoint, or no stream).
    NotJoined,
    /// A keyframe failed to restore.
    Restore(String),
}

/// A checkpoint being received.
#[derive(Debug, Default)]
struct Incoming {
    frame: u32,
    total: u32,
    buf: Vec<u8>,
}

/// A viewer of an edge-relayed (or directly linked) match.
///
/// Feed it every message received with [`Self::on_message`], then call
/// [`Self::advance`] once per displayed frame (or [`Self::catch_up`] to run
/// several). It sends nothing.
// Four independent lifecycle flags (synced, joined, waiting for a checkpoint,
// log overflowed), each set at its own event; an enum would pair states that
// can coexist.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct EdgeSpectator {
    identity: SessionIdentity,
    config: EdgeConfig,
    synced: bool,
    mismatch: Option<IdentityMismatch>,
    num_players: Option<u8>,
    /// The confirmed input log, `log[i]` being frame `base + i`.
    log: Vec<[u8; MAX_PLAYERS]>,
    /// The join point: frame 0, or the checkpoint's frame.
    base: u32,
    /// `true` once the machine has been started at `base`.
    joined: bool,
    /// Waiting for a checkpoint before joining.
    wants_checkpoint: bool,
    checkpoint: Option<Incoming>,
    /// The checkpoint, once complete, until the machine is started from it.
    ready_checkpoint: Option<(u32, Vec<u8>)>,
    /// The frame the machine will run next (it holds the state entering it).
    shown: u32,
    view: View,
    /// Snapshots of the machine entering their frame.
    keyframes: BTreeMap<u32, Vec<u8>>,
    overflowed: bool,
}

impl EdgeSpectator {
    /// A viewer that joins from frame 0: it runs the input log from the
    /// deterministic power-on.
    #[must_use]
    pub const fn new(identity: SessionIdentity, config: EdgeConfig) -> Self {
        Self {
            identity,
            config,
            synced: false,
            mismatch: None,
            num_players: None,
            log: Vec::new(),
            base: 0,
            joined: false,
            wants_checkpoint: false,
            checkpoint: None,
            ready_checkpoint: None,
            shown: 0,
            view: View::Live,
            keyframes: BTreeMap::new(),
            overflowed: false,
        }
    }

    /// A viewer that joins from a checkpoint ([`NetMessage::StateCheckpoint`])
    /// and the inputs after it. Inputs for earlier frames are ignored.
    #[must_use]
    pub const fn from_checkpoint(identity: SessionIdentity, config: EdgeConfig) -> Self {
        let mut s = Self::new(identity, config);
        s.wants_checkpoint = true;
        s
    }

    /// `true` once the stream's identity has been seen and matches ours.
    #[must_use]
    pub const fn is_synced(&self) -> bool {
        self.synced
    }

    /// Set (terminally) when the stream announces another game, machine
    /// configuration or emulator.
    #[must_use]
    pub const fn mismatch(&self) -> Option<IdentityMismatch> {
        self.mismatch
    }

    /// `true` if inputs past [`MAX_LOG_FRAMES`] were dropped.
    #[must_use]
    pub const fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// The frame the machine will run next: the state it holds is the one
    /// entering this frame.
    #[must_use]
    pub const fn current_frame(&self) -> u32 {
        self.shown
    }

    /// The newest frame whose entering state the viewer can reach (the frame
    /// after the newest confirmed input), once joined.
    #[must_use]
    pub fn reachable_through(&self) -> u32 {
        self.base + u32::try_from(self.log.len()).unwrap_or(u32::MAX)
    }

    /// How many confirmed frames are received but not shown yet.
    #[must_use]
    pub fn behind(&self) -> u32 {
        self.reachable_through().saturating_sub(self.shown)
    }

    /// The view.
    #[must_use]
    pub const fn view(&self) -> View {
        self.view
    }

    /// Switch the view: [`View::Live`] resumes following the stream from the
    /// shown frame; [`View::Paused`] holds it.
    pub const fn set_view(&mut self, view: View) {
        self.view = view;
    }

    /// Fold in one received message. Anything that is not spectator traffic
    /// is ignored.
    pub fn on_message(&mut self, msg: &NetMessage) {
        if self.mismatch.is_some() {
            return;
        }
        match *msg {
            NetMessage::Sync {
                magic,
                ref identity,
                ..
            } => match self.identity.check_sync(magic, identity) {
                SyncVerdict::Ignore => {}
                SyncVerdict::Accept => self.synced = true,
                SyncVerdict::Refuse(why) => self.mismatch = Some(why),
            },
            NetMessage::ConfirmedInputs {
                num_players,
                start_frame,
                ref inputs,
            } => {
                if !self.synced || *self.num_players.get_or_insert(num_players) != num_players {
                    return;
                }
                for (i, frame_inputs) in inputs.iter().enumerate() {
                    let Some(frame) = u32::try_from(i)
                        .ok()
                        .and_then(|i| start_frame.checked_add(i))
                    else {
                        break;
                    };
                    self.push_input(frame, *frame_inputs);
                }
            }
            NetMessage::StateCheckpoint {
                frame,
                total,
                offset,
                ref data,
            } => {
                if !self.wants_checkpoint || self.ready_checkpoint.is_some() || self.joined {
                    return;
                }
                let inc = self.checkpoint.get_or_insert_with(|| Incoming {
                    frame,
                    total,
                    buf: Vec::new(),
                });
                if inc.frame != frame || inc.total != total {
                    return;
                }
                let have = inc.buf.len();
                let start = offset as usize;
                let end = start + data.len();
                if start <= have && end > have {
                    inc.buf.extend_from_slice(&data[have - start..]);
                }
                if inc.buf.len() == total as usize
                    && let Some(done) = self.checkpoint.take()
                {
                    self.base = done.frame;
                    // Nothing is in the log yet: `push_input` drops every
                    // input until the checkpoint is in, and the relay's
                    // backlog or the sender's resend supplies them. Cleared
                    // anyway, so the log always starts at `base`.
                    self.log.clear();
                    self.ready_checkpoint = Some((done.frame, done.buf));
                }
            }
            _ => {}
        }
    }

    /// Append frame `frame`'s inputs if it is the next one the log needs. A
    /// duplicate (a relay's backlog overlapping its live feed) is ignored, and
    /// so is a gap, which the relay's contiguity check never sends.
    fn push_input(&mut self, frame: u32, inputs: [u8; MAX_PLAYERS]) {
        if self.wants_checkpoint && self.ready_checkpoint.is_none() && !self.joined {
            // The join point is unknown until the checkpoint is in; inputs
            // that arrive first are dropped and the relay's backlog (or the
            // sender's resend) supplies them.
            return;
        }
        if frame != self.reachable_through() {
            return;
        }
        if self.log.len() >= MAX_LOG_FRAMES {
            self.overflowed = true;
            return;
        }
        self.log.push(inputs);
    }

    /// Start the machine at the join point if it has not been yet.
    fn join(&mut self, nes: &mut Nes) -> bool {
        if self.joined {
            return true;
        }
        if !self.synced {
            return false;
        }
        if self.wants_checkpoint {
            let Some((frame, state)) = self.ready_checkpoint.take() else {
                return false;
            };
            if nes.restore_quiet(&state).is_err() {
                // Not a snapshot this build can load (another epoch's, or
                // corrupt): wait for another.
                return false;
            }
            self.shown = frame;
            self.keyframes.insert(frame, state);
        } else {
            rustynes_core::power_on_for_movie(nes);
            self.shown = 0;
            let mut snap = Vec::new();
            nes.snapshot_core_into(&mut snap);
            self.keyframes.insert(0, snap);
        }
        self.joined = true;
        true
    }

    /// Run the frame the machine is entering, from the log.
    fn run_one(&mut self, nes: &mut Nes) -> bool {
        let Some(&inputs) = (self.shown - self.base)
            .try_into()
            .ok()
            .and_then(|i: usize| self.log.get(i))
        else {
            return false;
        };
        let n = usize::from(self.num_players.unwrap_or(2));
        nes.set_four_score(n > 2);
        for (port, &b) in inputs.iter().enumerate().take(n) {
            nes.set_buttons(port, Buttons::from_bits_truncate(b));
        }
        let _ = nes.run_frame();
        self.shown += 1;
        if self
            .shown
            .is_multiple_of(self.config.keyframe_interval.max(1))
            && !self.keyframes.contains_key(&self.shown)
        {
            let mut snap = Vec::new();
            nes.snapshot_core_into(&mut snap);
            self.keyframes.insert(self.shown, snap);
            while self.keyframes.len() > self.config.max_keyframes.max(1) {
                // Keep the join point; drop the oldest other keyframe.
                let Some(&oldest) = self.keyframes.keys().find(|&&k| k != self.base) else {
                    break;
                };
                self.keyframes.remove(&oldest);
            }
        }
        true
    }

    /// In [`View::Live`], run the next confirmed frame if it has arrived.
    /// Returns the frame run, or `None` (paused, not joined, or waiting).
    pub fn advance(&mut self, nes: &mut Nes) -> Option<u32> {
        if self.view == View::Paused || !self.join(nes) {
            return None;
        }
        let frame = self.shown;
        self.run_one(nes).then_some(frame)
    }

    /// Run up to `max_frames` received frames at once (a viewer catching up
    /// after joining mid-session), in either view. Returns how many ran.
    pub fn catch_up(&mut self, nes: &mut Nes, max_frames: u32) -> u32 {
        if !self.join(nes) {
            return 0;
        }
        let mut ran = 0;
        while ran < max_frames && self.run_one(nes) {
            ran += 1;
        }
        ran
    }

    /// Put the machine at the state entering `frame`, from the nearest
    /// keyframe at or before it, and pause there.
    ///
    /// # Errors
    ///
    /// [`SeekError`].
    pub fn seek(&mut self, nes: &mut Nes, frame: u32) -> Result<(), SeekError> {
        if !self.join(nes) {
            return Err(SeekError::NotJoined);
        }
        let last = self.reachable_through();
        if frame < self.base || frame > last {
            return Err(SeekError::OutOfRange {
                first: self.base,
                last,
            });
        }
        let (&at, state) = self
            .keyframes
            .range(..=frame)
            .next_back()
            .ok_or(SeekError::NotJoined)?;
        nes.restore_quiet(state)
            .map_err(|e| SeekError::Restore(e.to_string()))?;
        self.shown = at;
        while self.shown < frame && self.run_one(nes) {}
        self.view = View::Paused;
        Ok(())
    }

    /// Seek `frames` back from the shown frame (clamped to the join point).
    ///
    /// # Errors
    ///
    /// [`SeekError`].
    pub fn rewind(&mut self, nes: &mut Nes, frames: u32) -> Result<(), SeekError> {
        self.seek(nes, self.shown.saturating_sub(frames).max(self.base))
    }

    /// A checkpoint for another viewer: the machine's state now, entering
    /// [`Self::current_frame`], as [`NetMessage::StateCheckpoint`] pieces.
    /// The machine is canonical at every frame this viewer shows.
    #[must_use]
    pub fn checkpoint(&self, nes: &Nes) -> Vec<NetMessage> {
        let mut snap = Vec::new();
        nes.snapshot_core_into(&mut snap);
        checkpoint_messages(self.shown, &snap)
    }
}

/// `snapshot` (the machine entering `frame`) as [`NetMessage::StateCheckpoint`]
/// pieces.
#[must_use]
pub fn checkpoint_messages(frame: u32, snapshot: &[u8]) -> Vec<NetMessage> {
    let total = u32::try_from(snapshot.len()).unwrap_or(u32::MAX);
    snapshot
        .chunks(NetMessage::MAX_FORENSIC_CHUNK)
        .zip((0u32..).step_by(NetMessage::MAX_FORENSIC_CHUNK))
        .map(|(data, offset)| NetMessage::StateCheckpoint {
            frame,
            total,
            offset,
            data: data.to_vec(),
        })
        .collect()
}

/// The publisher's side: turns a running [`RollbackSession`]'s newly
/// confirmed frames into [`NetMessage::ConfirmedInputs`] for the relay.
///
/// Call [`Self::poll`] once per tick, after the session's `advance`, and send
/// what it returns. The first call also returns the identity `Sync`, which
/// the relay keeps and gives every viewer first. Frames go out in batches of
/// [`Self::BATCH_FRAMES`]; call [`Self::flush`] when the session ends to send
/// the last, shorter one.
#[derive(Debug)]
pub struct EdgePublisher {
    identity: SessionIdentity,
    announced: bool,
    /// The next frame to read from the session.
    next: u32,
    /// Confirmed frames read but not yet sent: copied out as soon as they
    /// are confirmed, so holding them for a batch never depends on the
    /// session's history window.
    pending: Vec<[u8; MAX_PLAYERS]>,
    /// Set when a confirmed frame left the session's history before it was
    /// published (poll was not called for too long); publishing stops.
    gap: bool,
}

impl EdgePublisher {
    /// Frames per relay message in a normally paced match: 7.5 messages a
    /// second at 60 Hz. The relay admits 30 a second with a burst of 60
    /// (`services/hosted/src/spectate.ts`), and drops, then closes, a
    /// publisher above that. Sending every newly confirmed frame at once
    /// (v3.7.0 before the #646 review) was about 60 a second and stalled the
    /// relay two seconds in. The cost is up to 7 frames (117 ms) of extra
    /// delay for viewers, who run behind the players anyway.
    pub const BATCH_FRAMES: usize = 8;

    /// A publisher for the match `identity` describes.
    #[must_use]
    pub const fn new(identity: SessionIdentity) -> Self {
        Self {
            identity,
            announced: false,
            next: 0,
            pending: Vec::new(),
            gap: false,
        }
    }

    /// `true` once a frame could not be published because the session no
    /// longer held its input.
    #[must_use]
    pub const fn lost_frames(&self) -> bool {
        self.gap
    }

    /// What to send to the relay for the frames `session` has newly
    /// confirmed: the identity first, then a batch once
    /// [`Self::BATCH_FRAMES`] frames are waiting (several, of at most
    /// `MAX_CONFIRMED_BATCH`, after a catch-up).
    pub fn poll<T: Transport>(&mut self, session: &RollbackSession<T>) -> Vec<NetMessage> {
        let mut out = Vec::new();
        if !self.announced {
            out.push(NetMessage::Sync {
                magic: NetMessage::SYNC_MAGIC,
                identity: self.identity,
                key_share: [0; KEY_SHARE_LEN],
            });
            self.announced = true;
        }
        let Some(confirmed) = session.last_confirmed_frame() else {
            return out;
        };
        while !self.gap && self.next <= confirmed {
            let Some(inputs) = session.confirmed_input(self.next) else {
                self.gap = true;
                break;
            };
            self.pending.push(inputs);
            self.next += 1;
        }
        let n = session.num_players();
        while self.pending.len() >= NetMessage::MAX_CONFIRMED_BATCH {
            let rest = self.pending.split_off(NetMessage::MAX_CONFIRMED_BATCH);
            let batch = core::mem::replace(&mut self.pending, rest);
            out.push(self.batch(batch, n));
        }
        if self.pending.len() >= Self::BATCH_FRAMES {
            let batch = core::mem::take(&mut self.pending);
            out.push(self.batch(batch, n));
        }
        out
    }

    /// The frames still waiting for a full batch, as one last message: call
    /// it when the session ends, so viewers get the match's final frames.
    pub fn flush(&mut self, num_players: u8) -> Option<NetMessage> {
        if self.pending.is_empty() {
            return None;
        }
        let batch = core::mem::take(&mut self.pending);
        Some(self.batch(batch, num_players))
    }

    /// A message for `inputs`, the oldest frames waiting: they start where
    /// the rest of `pending` (still to be sent) and `inputs` together end.
    fn batch(&self, inputs: Vec<[u8; MAX_PLAYERS]>, num_players: u8) -> NetMessage {
        let waiting = u32::try_from(inputs.len() + self.pending.len()).unwrap_or(0);
        NetMessage::ConfirmedInputs {
            num_players,
            start_frame: self.next - waiting,
            inputs,
        }
    }
}

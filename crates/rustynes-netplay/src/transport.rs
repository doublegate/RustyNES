//! The network-agnostic [`Transport`] trait plus an in-memory implementation
//! for tests.
//!
//! The [`RollbackSession`](crate::session::RollbackSession) talks to its peer
//! exclusively through [`Transport`]. Stage 2 will add a `UdpTransport` that
//! implements this same trait (serializing via [`NetMessage::to_bytes`]); the
//! session does not change. Stage 1 ships [`MemoryTransport`], a fully
//! deterministic in-memory link whose latency / jitter / drop behaviour is
//! driven by a seeded PRNG so the determinism harness can reproduce a run
//! exactly.
//!
//! [`NetMessage::to_bytes`]: crate::message::NetMessage::to_bytes

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::message::NetMessage;
use crate::rng::SplitMix64;

/// A bidirectional message channel to the remote peer.
///
/// Network-agnostic: the session only ever `send`s and `poll`s. An
/// implementation is free to drop, delay, duplicate, or reorder messages —
/// the rollback protocol is designed to tolerate all of those. `poll`
/// returns every message that has *arrived* since the last `poll`.
pub trait Transport {
    /// Queue a message for delivery to the remote peer. May be dropped or
    /// delayed by the underlying medium.
    fn send(&mut self, msg: &NetMessage);

    /// Return all messages that have arrived from the remote peer since the
    /// previous call. Never blocks.
    fn poll(&mut self) -> Vec<NetMessage>;
}

/// One message in flight across the [`MemoryTransport`] link: the payload
/// plus how many more `poll`s must elapse before it is delivered.
#[derive(Clone, Debug)]
struct InFlight {
    msg: NetMessage,
    /// Polls-until-delivery countdown. 0 means "deliverable on the next
    /// poll".
    deliver_in: u32,
}

/// The shared, ordered queue feeding ONE direction of the link. Endpoint A's
/// `send` pushes here; endpoint B's `poll` drains the ready entries.
type Wire = Rc<RefCell<VecDeque<InFlight>>>;

/// Latency / jitter / drop configuration for [`MemoryTransport`].
///
/// All of it is applied deterministically via the endpoint's seeded PRNG.
#[derive(Clone, Copy, Debug)]
pub struct LinkConditions {
    /// Base one-way latency, measured in `poll` ticks (one tick == one call
    /// to [`Transport::poll`], which the session calls once per visual
    /// frame). Latency `n` means a message sent during frame `f` becomes
    /// visible to the peer's `poll` on frame `f + n`.
    pub latency_polls: u32,
    /// Maximum extra jitter in polls, added uniformly in `0..=jitter_polls`
    /// per message. `0` disables jitter (deterministic constant latency).
    pub jitter_polls: u32,
    /// Drop probability in `[0.0, 1.0)`. `0.0` = a perfectly reliable link.
    /// Dropped messages are silently discarded — the protocol's input
    /// redundancy / retransmit (Stage 2) tolerates this.
    pub drop_prob: f64,
}

impl LinkConditions {
    /// A perfect link: zero latency, no jitter, no loss. With this, every
    /// input is confirmed before it is consumed, so the session never
    /// rolls back.
    pub const PERFECT: Self = Self {
        latency_polls: 0,
        jitter_polls: 0,
        drop_prob: 0.0,
    };

    /// A fixed-latency, lossless link of `polls` one-way delay.
    #[must_use]
    pub const fn fixed_latency(polls: u32) -> Self {
        Self {
            latency_polls: polls,
            jitter_polls: 0,
            drop_prob: 0.0,
        }
    }
}

/// One end of a deterministic in-memory link. Construct a connected pair via
/// [`MemoryTransport::pair`].
///
/// `send` pushes onto the outbound wire with a PRNG-derived delivery delay;
/// `poll` drains the ready entries from the inbound wire. Because the wires
/// are shared `Rc<RefCell<…>>` queues and the PRNG is seeded, a full run is
/// perfectly reproducible — the precondition for the determinism harness.
pub struct MemoryTransport {
    /// Messages this endpoint has sent, awaiting delivery to the peer.
    outbound: Wire,
    /// Messages destined for this endpoint, sent by the peer.
    inbound: Wire,
    conditions: LinkConditions,
    rng: SplitMix64,
}

impl MemoryTransport {
    /// Build a connected pair of endpoints sharing two directed wires.
    ///
    /// Each endpoint gets its own seeded PRNG (derived from `seed`) so the
    /// two directions' jitter/drop draws are independent yet reproducible.
    /// Both ends share the same [`LinkConditions`]; pass asymmetric
    /// conditions via [`MemoryTransport::pair_with`].
    #[must_use]
    pub fn pair(conditions: LinkConditions, seed: u64) -> (Self, Self) {
        Self::pair_with(conditions, conditions, seed)
    }

    /// Like [`Self::pair`] but with independent conditions per direction
    /// (`a_to_b` governs messages A sends to B, `b_to_a` the reverse).
    #[must_use]
    pub fn pair_with(a_to_b: LinkConditions, b_to_a: LinkConditions, seed: u64) -> (Self, Self) {
        let wire_a_to_b: Wire = Rc::new(RefCell::new(VecDeque::new()));
        let wire_b_to_a: Wire = Rc::new(RefCell::new(VecDeque::new()));
        let endpoint_a = Self {
            outbound: Rc::clone(&wire_a_to_b),
            inbound: Rc::clone(&wire_b_to_a),
            conditions: a_to_b,
            // Distinct seeds per direction; XOR with a constant keeps them
            // distinct even when `seed` is small.
            rng: SplitMix64::new(seed ^ 0xA5A5_A5A5_A5A5_A5A5),
        };
        let endpoint_b = Self {
            outbound: wire_b_to_a,
            inbound: wire_a_to_b,
            conditions: b_to_a,
            rng: SplitMix64::new(seed ^ 0x5A5A_5A5A_5A5A_5A5A),
        };
        (endpoint_a, endpoint_b)
    }
}

impl Transport for MemoryTransport {
    fn send(&mut self, msg: &NetMessage) {
        // Deterministic drop check first (so a dropped message still consumes
        // one PRNG draw, keeping the stream aligned regardless of outcome).
        let dropped =
            self.conditions.drop_prob > 0.0 && self.rng.next_unit() < self.conditions.drop_prob;
        let jitter = if self.conditions.jitter_polls > 0 {
            self.rng.next_below(self.conditions.jitter_polls + 1)
        } else {
            0
        };
        if dropped {
            return;
        }
        let deliver_in = self.conditions.latency_polls + jitter;
        self.outbound.borrow_mut().push_back(InFlight {
            msg: msg.clone(),
            deliver_in,
        });
    }

    fn poll(&mut self) -> Vec<NetMessage> {
        let mut ready = Vec::new();
        let mut inbound = self.inbound.borrow_mut();
        // Decrement every in-flight entry; deliver those whose countdown has
        // reached zero. We keep relative order among delivered messages
        // (FIFO per wire) — jitter can still reorder relative to send order
        // because a later send with smaller `deliver_in` would surface
        // earlier, which the protocol tolerates.
        let mut remaining = VecDeque::with_capacity(inbound.len());
        while let Some(mut entry) = inbound.pop_front() {
            if entry.deliver_in == 0 {
                ready.push(entry.msg);
            } else {
                entry.deliver_in -= 1;
                remaining.push_back(entry);
            }
        }
        *inbound = remaining;
        ready
    }
}

/// A deterministic in-memory **mesh** transport for an `N`-player session.
///
/// Each peer's [`send`](Transport::send) fans the message out to every other
/// peer; [`poll`](Transport::poll) collects the inbound messages from all of
/// them. This is the multi-peer analogue of [`MemoryTransport`] — the topology
/// a real >2-player session uses (each peer broadcasts its own input, tagged
/// with its player index, to all others) — and is what the N-player
/// determinism harness wires up.
///
/// Like [`MemoryTransport`], every latency/jitter/drop draw comes from a
/// seeded PRNG, so a full run is perfectly reproducible.
pub struct MeshTransport {
    /// One directed wire to every *other* peer (this peer's `send` pushes onto
    /// all of them).
    outbound: Vec<Wire>,
    /// One directed wire from every *other* peer (this peer's `poll` drains
    /// all of them).
    inbound: Vec<Wire>,
    conditions: LinkConditions,
    rng: SplitMix64,
}

impl MeshTransport {
    /// Build a fully-connected mesh of `num_players` endpoints (2..=4),
    /// returning one [`MeshTransport`] per player index (`0..num_players`). Every
    /// directed link shares the same [`LinkConditions`]; each endpoint has its
    /// own seeded PRNG derived from `seed` and its index so the directions'
    /// jitter/drop draws are independent yet reproducible.
    ///
    /// # Panics
    ///
    /// Panics if `num_players` is not in `1..=4`.
    #[must_use]
    pub fn mesh(num_players: u8, conditions: LinkConditions, seed: u64) -> Vec<Self> {
        let n = num_players as usize;
        assert!((1..=4).contains(&n), "mesh needs 1..=4 players");
        // wires[from][to] is the directed queue carrying `from`'s sends to
        // `to`. The self-diagonal is unused (a peer never sends to itself).
        let wires: Vec<Vec<Wire>> = (0..n)
            .map(|_| {
                (0..n)
                    .map(|_| Rc::new(RefCell::new(VecDeque::new())))
                    .collect()
            })
            .collect();

        let mut endpoints = Vec::with_capacity(n);
        for (me, my_row) in wires.iter().enumerate() {
            // `outbound` is this peer's row `wires[me][*]`; `inbound` is the
            // column `wires[*][me]` (each other peer's wire TO me).
            let outbound: Vec<Wire> = my_row
                .iter()
                .enumerate()
                .filter(|&(other, _)| other != me)
                .map(|(_, w)| Rc::clone(w))
                .collect();
            let inbound: Vec<Wire> = wires
                .iter()
                .enumerate()
                .filter(|&(other, _)| other != me)
                .map(|(_, row)| Rc::clone(&row[me]))
                .collect();
            // Per-endpoint seed: mix the index in so each peer's draws differ.
            let rng = SplitMix64::new(seed ^ (me as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            endpoints.push(Self {
                outbound,
                inbound,
                conditions,
                rng,
            });
        }
        endpoints
    }

    /// Push `msg` onto one directed wire with a PRNG-derived delay (or drop
    /// it). Mirrors `MemoryTransport::send`'s per-message model so the two
    /// transports behave identically per link.
    fn push_one(rng: &mut SplitMix64, conditions: LinkConditions, wire: &Wire, msg: &NetMessage) {
        let dropped = conditions.drop_prob > 0.0 && rng.next_unit() < conditions.drop_prob;
        let jitter = if conditions.jitter_polls > 0 {
            rng.next_below(conditions.jitter_polls + 1)
        } else {
            0
        };
        if dropped {
            return;
        }
        let deliver_in = conditions.latency_polls + jitter;
        wire.borrow_mut().push_back(InFlight {
            msg: msg.clone(),
            deliver_in,
        });
    }

    /// Drain the ready entries of one inbound wire into `ready`.
    fn drain_one(wire: &Wire, ready: &mut Vec<NetMessage>) {
        let mut inbound = wire.borrow_mut();
        let mut remaining = VecDeque::with_capacity(inbound.len());
        while let Some(mut entry) = inbound.pop_front() {
            if entry.deliver_in == 0 {
                ready.push(entry.msg);
            } else {
                entry.deliver_in -= 1;
                remaining.push_back(entry);
            }
        }
        *inbound = remaining;
    }
}

impl Transport for MeshTransport {
    fn send(&mut self, msg: &NetMessage) {
        // Fan out to every other peer, each link drawing its own delay/drop so
        // the broadcast is not artificially correlated across recipients.
        let conditions = self.conditions;
        // Clone the wire handles up front so `self.rng` and `self.outbound`
        // are not borrowed simultaneously in the loop.
        let wires: Vec<Wire> = self.outbound.iter().map(Rc::clone).collect();
        for wire in &wires {
            Self::push_one(&mut self.rng, conditions, wire, msg);
        }
    }

    fn poll(&mut self) -> Vec<NetMessage> {
        let mut ready = Vec::new();
        for wire in &self.inbound {
            Self::drain_one(wire, &mut ready);
        }
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_link_delivers_next_poll() {
        let (mut a, mut b) = MemoryTransport::pair(LinkConditions::PERFECT, 1);
        a.send(&NetMessage::InputAck { frame: 5 });
        // With zero latency, the message is deliverable on b's next poll.
        let got = b.poll();
        assert_eq!(got, vec![NetMessage::InputAck { frame: 5 }]);
        assert!(b.poll().is_empty());
    }

    #[test]
    fn fixed_latency_delays_n_polls() {
        let (mut a, mut b) = MemoryTransport::pair(LinkConditions::fixed_latency(3), 7);
        a.send(&NetMessage::InputAck { frame: 1 });
        // Three polls of nothing, then delivery.
        assert!(b.poll().is_empty());
        assert!(b.poll().is_empty());
        assert!(b.poll().is_empty());
        assert_eq!(b.poll(), vec![NetMessage::InputAck { frame: 1 }]);
    }

    #[test]
    fn drop_is_deterministic() {
        // Same seed ⇒ identical drop pattern across two independent runs.
        fn run() -> Vec<u32> {
            let cond = LinkConditions {
                latency_polls: 0,
                jitter_polls: 0,
                drop_prob: 0.5,
            };
            let (mut a, mut b) = MemoryTransport::pair(cond, 0x00C0_FFEE);
            let mut delivered = Vec::new();
            for f in 0..50u32 {
                a.send(&NetMessage::InputAck { frame: f });
                for m in b.poll() {
                    if let NetMessage::InputAck { frame } = m {
                        delivered.push(frame);
                    }
                }
            }
            delivered
        }
        assert_eq!(run(), run());
        // And some — but not all — were dropped.
        let d = run();
        assert!(!d.is_empty() && d.len() < 50);
    }

    #[test]
    fn directions_are_independent() {
        let (mut a, mut b) = MemoryTransport::pair(LinkConditions::PERFECT, 9);
        a.send(&NetMessage::InputAck { frame: 1 });
        b.send(&NetMessage::InputAck { frame: 2 });
        assert_eq!(b.poll(), vec![NetMessage::InputAck { frame: 1 }]);
        assert_eq!(a.poll(), vec![NetMessage::InputAck { frame: 2 }]);
    }

    #[test]
    fn mesh_broadcasts_to_all_other_peers() {
        // Player 0 sends; players 1, 2, 3 each receive exactly that message,
        // and player 0 receives nothing of its own.
        let mut peers = MeshTransport::mesh(4, LinkConditions::PERFECT, 0x1234);
        let msg = NetMessage::Input {
            player: 0,
            frame: 7,
            input: 0x55,
        };
        peers[0].send(&msg);
        assert!(peers[0].poll().is_empty(), "sender hears no echo of itself");
        for (p, peer) in peers.iter_mut().enumerate().skip(1) {
            assert_eq!(peer.poll(), vec![msg.clone()], "peer {p} got the input");
        }
    }

    #[test]
    fn mesh_is_deterministic_for_same_seed() {
        fn run() -> Vec<Vec<NetMessage>> {
            let cond = LinkConditions {
                latency_polls: 1,
                jitter_polls: 2,
                drop_prob: 0.25,
            };
            let mut peers = MeshTransport::mesh(3, cond, 0x00C0_FFEE);
            let mut out = vec![Vec::new(); 3];
            for f in 0..40u32 {
                for (p, peer) in peers.iter_mut().enumerate() {
                    peer.send(&NetMessage::Input {
                        player: u8::try_from(p).unwrap(),
                        frame: f,
                        input: u8::try_from(f & 0xFF).unwrap(),
                    });
                }
                for (p, peer) in peers.iter_mut().enumerate() {
                    out[p].extend(peer.poll());
                }
            }
            out
        }
        assert_eq!(run(), run(), "mesh delivery must be reproducible");
    }
}

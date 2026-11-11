# Netplay: the rollback session, its window, and datagram authentication

This is the spec for `rustynes-netplay`'s session layer: how
`RollbackSession` keeps and uses its rollback window (v3.4.0,
`T-NETPLAY-ROLLBACK-WINDOW`, section 1), and how every native datagram is
authenticated (v3.4.0, `T-NETPLAY-DATAGRAM-AUTH`, protocol 9, section 2,
[ADR 0046](adr/0046-a-per-session-secret-authenticates-every-netplay-datagram.md)),
and the two protocol-10 additions of v3.7.0 (Sync magic `"RN10"`): desync
forensics (section 4) and edge-relayed spectating (section 5). Protocol 10
changes nothing in sections 1 to 3; it adds messages, so a protocol-9 peer is
refused at the handshake. Transport, NAT traversal, WebRTC and the lobby are in
[`netplay-webrtc.md`](netplay-webrtc.md).

Netplay is on the frontend side of the determinism contract: nothing here
changes a frame, a sample or a bus cycle. The emulator only ever runs frames on
inputs; the session decides which inputs and when to re-run.

## 1. The rollback window (v3.4.0)

### 1.1 What the session holds

| Store | Holds | Bound |
|---|---|---|
| input history | one slot per frame, from just below the checkpoint to the newest input received | `INPUT_RESEND_WINDOW + 2 * input_delay + 2 * max_rollback_frames + 4` frames for an honest peer (88 with the defaults); a hostile peer can push it to `MAX_SESSION_FRAME_LOOKAHEAD` past the present |
| rollback window | one entry per frame from the checkpoint to the present: a full core snapshot (about 256 KiB) and the gameplay digest of the same moment | `max_rollback_frames + 3` entries |
| confirmed digests | the entering digest of the most recent confirmed frames | `CONFIRMED_DIGEST_RETAIN` = 256 |
| checkpoint digests | ours, one per checksum interval, and the peers' pending ones: every distinct value received for the frame, at most two | `CHECKSUM_RETAIN_FRAMES` = 1024 frames behind the checkpoint |

`RollbackSession::retained` reports the sizes and `RetainedWindow::bound`
computes the bound from the configuration alone.

The **checkpoint** is the window entry for the first unconfirmed frame `B`:
every frame below it ran on every player's real input, so it is the machine
every peer reaches.

### 1.2 What a tick does

- **Forward run.** Before frame 0 the session records the entry for frame 0;
  after running any frame `f` it records the entry for `f + 1`.
- **Misprediction.** A real input that contradicts the prediction an
  already-run frame `m` used restores entry `m` and replays `m` to the present.
  During the replay every frame still unconfirmed is **re-predicted** from what
  is now known, and its entry re-recorded. Until v3.4.0 a replay always started
  at the checkpoint, and did not re-predict, so the next late input for the
  following frame mispredicted again.
- **Confirmation.** Moving `B` forward re-runs nothing. The entries between
  the old and the new `B` were recorded on inputs that have all turned out
  right (a wrong one would have been a misprediction, which replays and
  re-records every entry after it), so their digests become the confirmed
  digests and the entries are dropped. Until v3.4.0 every confirmation restored
  the checkpoint and replayed to the present, even when every prediction had
  held (Round 2 NET-R2-01).
- **A confirmed input is final.** A different value for a cell already
  confirmed is ignored. An honest peer never sends one, and a frame that may
  sit below the checkpoint has no entry to replay from.
- **So is the local one (v3.6.0).** `add_local_input` records the input for
  `current_frame + input_delay`, and every frontend calls it once per tick
  with the buttons held on that tick. A tick on which the session stalled
  (frame advantage past `max_rollback_frames`) leaves `current_frame` where it
  was, so the next call names the same frame. The first value recorded is
  kept and resent; the newer buttons apply from the next frame produced.
  Until v3.6.0 the call overwrote the cell and sent the new value, while every
  peer had already kept the first, so the two sides ran different input for
  that frame and desynced (`T-NETPLAY-UI-SYNC-FLAKE`).

### 1.3 Why the entries are full snapshots

Round 2 NET-R2-02 proposed slim entries (no framebuffer), on the claim that a
replay repaints every pixel before the picture is read. Measured, it does not:
a `run_frame` boundary can fall a few dots into scanline 0, so the first
pixels of a frame's top row are painted by the call before it. On nestest a
slim restore of the state entering frames 1, 2 and 3 plus one frame left 51,
15 and 6 framebuffer bytes (row 0, x = 1 to 17) holding a later picture, and
the per-frame gate failed at frame 4 as a false desync. The entries are full
core snapshots; the window holds at most `max_rollback_frames + 3` of them
(about 2.8 MiB with the defaults), where v3.3.0 held one.

Each entry keeps its digest beside the snapshot, so a confirmation never
restores anything to learn a confirmed frame's digest.

Nothing in netplay reads the framebuffer as input. A Zapper would (it senses
light from the picture), but netplay carries only standard controllers and
the Four Score.

### 1.4 The digest (since protocol 9)

`gameplay_digest` is `framebuffer_digest(framebuffer) ^ cycle * 0x100_0000_01b3`.
Until v3.4.0 the framebuffer term was FNV-1a 64 over every byte. Since
protocol 9 it is a four-lane multiply-rotate checksum over little-endian
64-bit words (`framebuffer_digest`, NET-R2-05); it is not cryptographic and
not XXH64. A `Checksum` carries both terms, so changing either is a protocol
change.

### 1.5 Measured

Release build, `cargo bench -p rustynes-netplay --bench rollback_window`, on
the shared 20-core development host:

| Bench | Before (v3.3.0 code) | After, run 1 | After, run 2 |
|---|---|---|---|
| one nestest frame's digest: `digest_fnv1a_bytewise` before, `gameplay_digest` after | 209.7 us | 13.32 us | 13.27 us |
| `session_steady_state`: one tick of two peers, latency 3, predictable input | 20.78 ms | 8.13 ms (-60.9%) | 8.90 ms (-57.2%) |

The byte-wise digest re-measured in the after run at 205.5 us (-2.0%), which
is the host drift between the runs; the digest change is about 15x. The
session figure is a whole tick, emulation included: most of what remains is
the two frames the peers actually run.

### 1.6 Gates

`tests/rollback_window.rs`:

- `held_predictions_confirm_without_a_replay` sums `resimulated_frames` over a
  latent session whose predictions always hold and asserts zero (the v3.3.0
  code re-simulated 1,181 and 887 frames on the two peers).
- `only_mispredictions_replay` bounds the replays by what the input changes
  alone can cost (v3.3.0: 1,085 frames for 12 changes).
- `a_misprediction_forces_a_replay_and_every_frame_matches` checks every
  confirmed frame on both peers against the no-rollback reference over a
  latent, jittery, lossy link.
- `the_window_stays_bounded_over_a_long_session` samples `retained()` through
  3,000 frames against `RetainedWindow::bound` (v3.3.0: 3,002 history frames,
  3,002 digests, 6,004 checksum slots).

`tests/mesh_sync.rs`, `a_stalled_tick_does_not_re_author_the_local_input`
(v3.6.0): two and three peers called the way the frontends call them, one
input per tick with new buttons on every tick, over a 14-poll link that makes
every peer stall; every confirmed frame must agree. Without the local-input
rule above, the session raises `Desync` at the frame-60 checksum.

The `determinism.rs` suite is unchanged and still passes.

## 2. Datagram authentication (protocol 9)

The decision and its limits are in
[ADR 0046](adr/0046-a-per-session-secret-authenticates-every-netplay-datagram.md);
this section is the wire and the behaviour.

### 2.1 The handshake

| Path | Who sends a share | Key |
|---|---|---|
| 2 players, `NetplayConnection` (desktop host, mobile, room code, TURN) | both, in every handshake `Sync` | `DatagramAuth::pair`; sender 0 is the lower share |
| 2 players, desktop joiner (`MeshJoiner` meeting a 2-player host) | as above | the same pair key; the joiner runs as player 1 |
| 3-4 players, `MeshHost` + `MeshJoiner` | each joiner in its `Sync`; the host in the `Roster`, which lists every peer's share | `DatagramAuth::mesh`; sender = player index |

A share is 16 random bytes (`fresh_key_share`, never all zeros). A `Sync` with
an all-zero share (the one a running session sends, a NAT punch packet, a
spectator's announcement) is not a handshake `Sync`: it adopts no peer and
completes no handshake. A host takes the share only from the adopted peer's
address, and only the first one. On a mesh host a `Sync` from an adopted
address that carries a different share is not proof of life for that seat and
replaces nothing, since an off-path attacker can forge one
(`a_forged_share_from_a_live_joiner_s_address_replaces_nothing`). A joiner
that restarted on the same address with a fresh share therefore recovers by
expiry: its old seat is dropped after 3 s without the old share, and its next
`Sync` is adopted with the share it holds
(`a_restarted_joiner_is_re_adopted_with_its_new_key_share`). `DatagramAuth::mesh` refuses a roster
in which two seats carry the same share, as `pair` refuses `ours == theirs`
(`mesh_refuses_duplicate_key_shares`).

A joiner finds its own roster entry by its own share, which works through a
NAT where its address does not. A roster that does not carry its share is
ignored as if none had arrived, so its resend and deadline still run
(`a_roster_without_our_key_share_is_ignored`,
`a_stream_of_ignored_rosters_does_not_stop_the_deadline`); it used to fall back to
its bound address and then to the index given at connect, keying the mesh on
another seat's share. The roster lists each joiner's address in canonical form
(an IPv4-mapped address as plain IPv4), so an IPv4-bound joiner can dial a
peer that a dual-stack host heard in mapped form
(`the_roster_lists_mapped_joiners_in_their_ipv4_form`).

**Gathering (3-4 players).** A mesh host answers each adopted joiner's
`Sync` with an accepted `Sync` carrying an all-zero share ("adopted, still
gathering") until the roster is full; then the roster is the answer. A joiner
that has heard this keeps waiting while the host keeps answering: it fails if
the host is silent for its handshake timeout (10 s in the desktop UI), and in
any case at its gather timeout (`MeshJoiner::with_gather_timeout`, the host's
120 s in the UI). Before any answer the handshake timeout alone applies, so a
dead address still fails in 10 s. Until this, a mesh host sent nothing before
the roster was full, so the first joiner gave up at 10 s while the host,
gathering for 120 s, still counted it. A joiner re-sends its `Sync` every
50 ms while it waits; the host drops one it has not heard for 3 s
(`MeshHost::JOINER_SILENCE`) before the roster goes out, so a session never
starts with a joiner that left, and gives its seat to the next joiner (seats
are the lowest free index, so the full roster is always `0..n`). Pinned by
`mesh_net::tests::a_joiner_outlives_its_handshake_timeout_while_the_host_gathers`,
`a_joiner_of_a_dead_address_still_times_out_quickly` and
`the_host_drops_a_joiner_that_went_silent_before_the_roster_filled`.

The roster is the one message a joiner acts on before any key exists, so it is
untrusted: a joiner keeps a roster only from the host it dialed, and only when
it has 2 to 4 entries whose player indices are exactly `0..n`, each once
(`roster_is_well_formed`). A gap, such as `{0, 1, 3}`, would make a 3-player
session with a peer 3 it does not have, which a release build indexes out of
range. The frontend checks again before it builds the session
(`local_player < num_players`), because `RollbackSession::new` only
`debug_assert!`s it. Pinned by `mesh_net::tests::joiner_ignores_a_malformed_or_foreign_roster`.

### 2.2 The sealed datagram

```text
0xA9 | sender u8 | sequence u64 LE | NetMessage bytes | HMAC-SHA-256 tag, first 16 bytes
```

Receive order: length, marker, sender (known, not ourselves), replay window
(1024 per sender), tag. Only then is the sequence marked and the message
parsed. A refusal changes no state and is counted (`refused_datagrams`).
The send sequence saturates at `u64::MAX` rather than wrapping
(`the_send_counter_saturates_at_the_end_of_its_space`); 2^64 datagrams is out
of reach, and a wrap would only re-issue numbers every receiver has passed.

**A message speaks only for its sender's seat (mesh).** On a
`UdpMeshTransport` the sender index is the player index, and an `Input`,
`InputBatch` or `InputAck` whose `player` is not the sender's seat is dropped
after it opens and counted (`misattributed_datagrams`). The session takes the
first confirmed input of a cell as final and an ack as the end of a
retransmission, so without this a peer holding its own key could seal another
seat's input or ack. On the joiner's 2-player fallback the pair key numbers
senders by share order, so the host's seat is recorded as player 0 apart from
its sender index. A 2-player `UdpTransport` has one remote seat, and the
session already ignores an input or ack naming its own, so it needs no check.

### 2.3 Address handling

- A datagram that opens is accepted from any address. On the direct path the
  transport then sends to that address (`roamed`): a NAT rebinding keeps the
  session. A relayed peer's address is pinned by its TURN permission and not
  moved.
- An unsealed `Sync` from the peer's address, after the handshake, is
  answered with our handshake `Sync` (a mesh host: the roster), at most every
  100 ms: the peer lost our reply. It must decode as a whole `Sync` (any
  protocol's); a datagram that only starts with the `Sync` tag, such as a
  truncated `[0x02]`, gets nothing (`a_truncated_sync_gets_no_handshake_reply`,
  `a_truncated_sync_gets_no_roster_resend`). Nothing in it reaches the session.
- Everything else unsealed is refused.

### 2.4 What is not covered

- An attacker who observes the handshake can derive the key (the shares are
  in the clear). X25519 plus an authenticated fingerprint is the follow-up.
- The handshake itself is first-come, as before.
- Spectators carry no key. A direct-link spectator is unauthenticated, and so
  is a viewer of the edge relay (section 5), which receives only the
  identity and confirmed inputs; a relay publisher proves ownership of its
  match with its publish key, not the session secret. The relay match id is
  therefore a viewer's whole credential. The desktop mints a random 128-bit
  id when a host publishes with none, and warns when a typed one is shorter
  than 22 characters (`NetplayConfig::edge_match_or_mint`,
  `edge_match_is_guessable`).
- The browser (WebRTC) path does not seal: its data channel is DTLS.

### 2.5 Gates

`tests/datagram_auth.rs`, over loopback UDP, one side played by hand so the
test holds the key:

| Test | Asserts | Mutation that it catches |
|---|---|---|
| `a_forged_datagram_from_a_third_address_is_refused` | an unsealed checksum, an unsealed foreign `Sync` and a datagram sealed under a guessed key from a third socket reach nothing; the peer's does | accepting unsealed datagrams once sealed |
| `a_forged_checksum_cannot_end_a_session` | two sealed sessions run 240 ticks with a forger sending a desync and a ROM mismatch every tick | the same |
| `a_replayed_datagram_is_refused` | the same sealed bytes from the peer and from a third address open once | the replay check disabled |
| `a_rebinding_peer_is_kept` | the peer's datagrams from a new address open and the transport's next datagram goes there | the roam disabled |
| `a_mesh_peer_follows_a_rebinding_peer`, `a_mesh_peer_refuses_a_forgery` | the same on a sealed mesh | the mesh roam disabled |
| `a_mesh_peer_cannot_speak_for_another_seat` | player 2's sealed `Input`, `InputBatch` and `InputAck` naming player 1 reach nothing and are counted; its own input and a `Checksum` do | the seat comparison weakened to `>` |
| `a_protocol_8_peer_is_refused_with_a_reason` | an `"RNE8"` `Sync` ends the handshake with `EmulatorMismatch` naming its epoch | `"RNE8"` dropped from `OLDER_SYNC_MAGICS` |

Exact refusal counts are measured from a baseline taken after the
handshake (`settled_refusals`): the counter is cumulative, and a handshake can
leave an unsealed `Sync` in flight that the sealed side refuses.
`mesh_net::tests::the_pair_fallback_checks_the_hosts_seat_not_its_sender_index`
pins the fallback's seat mapping.

`auth.rs` unit tests cover the primitives: both ends of a pair agree, every
single-bit flip is refused, another key or identity does not open, the window
accepts reordering once and refuses the old.

## 3. Three and four players on the desktop (v3.4.0)

`T-NETPLAY-MESH-NATIVE`. The desktop netplay panel's player count reaches the
mesh (`docs/netplay-webrtc.md` section 2.4): `NetplayUi::start_host` with 3 or
4 players binds a `MeshHost`; `start_join` always dials through a
`MeshJoiner`, which completes as player 1 of 2 against a 2-player host and as
its roster index against a mesh host. Every session runs over a sealed
transport boxed as `Box<dyn Transport>` (`impl Transport for Box<T>`).

**Acks are per player (since protocol 9).** `InputAck { player, frame }`; the
session keeps one ack per remote player and resends from the least of them.
Before, the highest ack from any peer stood for all of them: found when the
3-player UI test's host stalled at frame 9 with nothing confirmed, because it
starts its session after its roster resends and a joiner's ack had already
stopped the other joiner resending what the host never received.

**Pending checksums keep every distinct value.** A `Checksum` names no
sender, and in a 3-4 player session several peers' checksums for one frame
can arrive before ours exists. The pending store kept one value per frame
until v3.4.0's review, so a matching peer's checksum overwrote a divergent
one and the desync went unseen; it now keeps up to two distinct values per
frame (honest peers send one), and compares each once ours exists. Pinned by
`session::tests::a_divergent_pending_checksum_is_not_overwritten_by_a_matching_one`,
in both arrival orders.

**Gates.** `tests/mesh_sync.rs` compares every confirmed frame of every peer
with the no-rollback reference, frame by frame, for 600 frames: 4 and 3 peers
in memory (latency 2, jitter 3, 5% loss), 4 peers over sealed loopback UDP with
loss, delay and reordering injected on the sockets. The ROM is a 95-byte
probe written for the test whose picture depends on all four controllers
(nestest reads only the first, and with it a session that left the Four Score
off passed). `a_peer_that_lost_early_inputs_still_gets_them` pins the ack fix.
`netplay_ui` tests drive 2, 3 and 4 `NetplayUi`s through host/join to
`InGame`, check the player count and distinct indices, and compare 120
confirmed frames. Mutations caught: the Four Score enabled only above three
players; player 4's input never applied; the least ack replaced by the
greatest; the joiner finding the wrong roster entry; a joiner's player index
capped at 1.

**Not done.** Live play across machines (the maintainer's run); NAT traversal
or TURN for more than two players (direct IP only); mobile (`MOB-09`).

## 4. Desync forensics (v3.7.0, protocol 10)

`T-NETPLAY-DESYNC-FORENSICS`, `crates/rustynes-netplay/src/forensics.rs`.

The checksum (section 1.4) says THAT two peers diverged, at a checkpoint, and
whether the picture or only the cycle term differs. It cannot say where: it
covers the framebuffer and the cycle counter only, so a wrong RAM byte stays
invisible until it reaches the picture, often many frames after its cause.
Forensics names the first frame whose state differs, the `.rns` section, and
the byte.

### 4.1 Opt-in

Forensics sends parts of the machine state to the peer, so it is off unless
each peer turns it on: `RollbackSession::enable_forensics` (the desktop's
`[netplay] desync_forensics` setting and the netplay panel's checkbox, the
mobile bridge's `np_set_desync_forensics`). A peer that has not opted in
ignores every forensics message and sends no state; the other side reports
"no digest from the peer yet".

### 4.2 What travels

| Message (tag) | Carries | When |
|---|---|---|
| `StateDigest` (7) | up to 8 frames of per-section digests: one 64-bit digest of each of the `CPU`, `BUS`, `MAP`, `PPU`, `APU` section bodies of the machine entering the frame | every tick: every frame not sent yet, and the newest 4 again, so a burst of confirmations skips nothing and a lost datagram is covered |
| `ForensicChunk` (8) | up to 1,024 bytes of one differing section body, at an offset, with the body's length | while the bodies are exchanged: up to 32 a tick, from where the peer's acknowledgement stands (go-back-N) |
| `ForensicAck` (9) | how much of one body has arrived contiguously | every tick of the exchange |

The digests are `framebuffer_digest` of each section body, taken from the
rollback window entry of each frame as it confirms: the same canonical
snapshot every peer reaches byte for byte (section 1.1). Only the sections
whose digests differ travel: one wrong RAM byte costs one `BUS` body, about
2.4 KiB. A peer keeps the snapshots of its last 64 confirmed frames (about
16 MiB) so the divergence frame's bytes are still at hand when the peer's
digests reveal it; a divergence found later than that reports the sections
from the digests alone.

### 4.3 The report

`RollbackSession::forensics_report` returns a `ForensicReport`:

- **`frame`**: the first frame whose ENTERING state differs. Whatever
  diverged did so while the frame before it ran (or, for a write from outside
  the core such as a RAM cheat, between the two).
- **`sections`**: every differing section, in the order CPU, BUS, MAP, PPU,
  APU (from the state a game's code reads most directly to the one it reads
  least), each with its first differing byte, both peers' values there, and
  how many bytes differ. `primary()` is the first.
- **Locations**: a `BUS` byte at offsets 8 to `0x807` is CPU RAM `$0000` to
  `$07FF` (the section's 2 KiB image follows its 8-byte cycle counter);
  offsets 0 to 7 are that counter. Any other byte is reported as its offset
  in the section body.

A report is about bytes, not causes: two sections can differ at one frame
because one wrote the other. The `APU` section also holds the host audio
resampler's state and output rate, so peers at different output sample rates
differ there with no gameplay difference; a report whose only differing
section is `APU` says so.

### 4.4 After a fatal desync

`NetplayError::Desync` ends the session, but the exchange may still be
running. `RollbackSession::pump_forensics` polls, folds in only the forensics
messages and sends this peer's; the desktop keeps an ended session in an
`Investigating` state, pumping it for up to 10 seconds, and then shows the
report (or that the peer sent nothing) as the error message. The mobile
bridge keeps it until `np_forensics` has returned a report or the app leaves.

### 4.5 Limits

- One divergence per session, against the first peer it is found with; a 3-4
  player mesh reports the first pair.
- Forensics never writes the emulator, the inputs or the checksum: a session
  with it on produces every frame, sample and checksum byte-identically to
  one with it off. Its cost, on an opted-in peer only, is one section-digest
  pass and one snapshot copy per confirmed frame.

### 4.6 Gates

`tests/desync_forensics.rs`: two peers on a perfect in-memory link (nothing
mispredicts, so no rollback can replay the planted byte away); after frame
90, one byte of player 2's CPU RAM at `$0300`, which the fixture ROM never
reads, is XORed with `$A5`. The gameplay checksum never fires.

- `a_planted_ram_byte_is_found_at_its_frame_and_address`: both peers' reports
  name frame 91, `BUS`, CPU RAM `$0300`, one differing byte, and each side's
  own and the other's value.
- `the_exchange_finishes_after_the_session_stops_advancing`: the same,
  finished by `pump_forensics` alone once the divergence is found.
- `a_peer_that_did_not_opt_in_sends_no_state`.

Mutations caught: the RAM image's offset in `BUS` moved to 0 (address
wrong); the report frame one early; the `BUS` digest comparison skipped (no
report); forensics on by default (the opt-in test).

## 5. Edge-relayed spectating (v3.7.0, protocol 10)

`T-NETPLAY-EDGE-SPECTATE`: `crates/rustynes-netplay/src/edge.rs` (the viewer
and the publisher), `edge_client.rs` (the native WebSocket client), and the
relay, `services/hosted/src/spectate.ts` (its README section "Spectate relay"
has the limits, the cost, the deploy and the live gates).

### 5.1 The stream

The host's `EdgePublisher` copies each newly confirmed frame out of the
session and sends them as `ConfirmedInputs` (tag 10) in batches of
`BATCH_FRAMES` (8): the player count, the first frame, and every port's input
for up to 64 consecutive frames. Its first message is the match's `Sync`
(identity: ROM, configuration, emulation epoch), and `flush` sends the last,
shorter batch when the session ends. At 60 Hz that is 7.5 messages a second,
a quarter of what the relay admits; one message per tick (v3.7.0 before
the PR #646 review) was twice it, and the relay stalled two seconds in. The relay
appends a batch only if it continues the log (storing only its new frames),
closes a publisher whose batch it must drop for the rate or a gap, keeps the
log, and gives a new viewer the identity and the whole log from frame 0
before the live feed. Each publish connection is a session: a new one from
the owner restarts the match's log. The relay carries nothing else: never a
frame, a sample or a state.

### 5.2 The viewer

`EdgeSpectator` refuses another identity as `SpectatorSession` does, keeps
the input log, and runs the core from the deterministic power-on
(`power_on_for_movie`, the start every session uses). A confirmed frame ran on
every player's real input, so the viewer reaches the players' confirmed state
byte for byte at every frame; it predicts nothing and never rolls back.

- **Seek and rewind:** a keyframe (core snapshot) every `keyframe_interval`
  frames (300 by default; at most `max_keyframes`, the join point always
  kept). A seek restores the nearest keyframe at or before the target and
  replays the log to it, then pauses.
- **Views:** `View::Live` follows the newest confirmed frame;
  `View::Paused` holds where a seek or pause left it.
- **Joining from a checkpoint:** over a direct link, a viewer can join from
  `StateCheckpoint` (tag 11): a core snapshot of the machine entering a frame,
  in pieces of up to 1,024 bytes, then the inputs from that frame. Any viewer
  that has caught up holds such a machine (`EdgeSpectator::checkpoint`). The
  relay refuses tag 11: through the relay a viewer always joins from frame 0.

### 5.3 Gates

`tests/edge_spectate.rs` (two players on an in-memory link with latency and
jitter, so they roll back; the host's published frames go through the wire
encoding):

- `a_viewer_joining_mid_session_matches_the_players`: a viewer that joins
  after 300 confirmed frames gets the log from frame 0 and matches the
  players' confirmed entering digest at every frame it shows for 300 more.
- `a_viewer_joining_from_a_checkpoint_matches_the_players`: a second viewer
  joins from a checkpoint the first serves, and matches for 200 frames.
- `seek_and_rewind_land_on_the_players_frames`.
- `hosted_spectate_vectors_match_the_encoder`: pins the byte layouts the
  Worker classifies (`services/hosted/test/vectors/spectate-vectors.json`,
  which `spectate.test.ts` replays).

`services/hosted/test/spectate.test.ts` (vitest in workerd): the classifier;
a mid-session viewer gets the backlog then the live feed; inputs only (a
checkpoint, a checksum, a per-player input, a text frame and a second identity
are dropped, five in all, and only the next valid batch is forwarded); the log
survives eviction; only the claiming key may publish, one at a time; the
per-IP viewer and match limits; the kill switch at the Worker and in the
object. `tests/edge_live.rs` is the same mid-session join against a running
relay (`#[ignore]`d; the deploy's live gate).

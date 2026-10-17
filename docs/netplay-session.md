# Netplay: the rollback session, its window, and datagram authentication

This is the spec for `rustynes-netplay`'s session layer: how
`RollbackSession` keeps and uses its rollback window (v3.4.0,
`T-NETPLAY-ROLLBACK-WINDOW`, section 1), and how every native datagram is
authenticated (v3.4.0, `T-NETPLAY-DATAGRAM-AUTH`, protocol 9, section 2,
[ADR 0046](adr/0046-a-per-session-secret-authenticates-every-netplay-datagram.md)).
Transport, NAT traversal, WebRTC and the lobby are in
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
| checkpoint digests | ours and the peers' pending ones, one per checksum interval | `CHECKSUM_RETAIN_FRAMES` = 1024 frames behind the checkpoint |

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

### 1.4 The digest (protocol 9)

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
address, and only the first one.

A joiner finds its own roster entry by its own share, which works through a
NAT where its address does not.

### 2.2 The sealed datagram

```text
0xA9 | sender u8 | sequence u64 LE | NetMessage bytes | HMAC-SHA-256 tag, first 16 bytes
```

Receive order: length, marker, sender (known, not ourselves), replay window
(1024 per sender), tag. Only then is the sequence marked and the message
parsed. A refusal changes no state and is counted (`refused_datagrams`).

### 2.3 Address handling

- A datagram that opens is accepted from any address. On the direct path the
  transport then sends to that address (`roamed`): a NAT rebinding keeps the
  session. A relayed peer's address is pinned by its TURN permission and not
  moved.
- An unsealed `Sync` from the peer's address, after the handshake, is
  answered with our handshake `Sync` (a mesh host: the roster), at most every
  100 ms, without being parsed: the peer lost our reply.
- Everything else unsealed is refused.

### 2.4 What is not covered

- An attacker who observes the handshake can derive the key (the shares are
  in the clear). X25519 plus an authenticated fingerprint is the follow-up.
- The handshake itself is first-come, as before.
- Spectators carry no key; no host-side spectator broadcast exists yet.
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
| `a_protocol_8_peer_is_refused_with_a_reason` | an `"RNE8"` `Sync` ends the handshake with `EmulatorMismatch` naming its epoch | `"RNE8"` dropped from `OLDER_SYNC_MAGICS` |

`auth.rs` unit tests cover the primitives: both ends of a pair agree, every
single-bit flip is refused, another key or identity does not open, the window
accepts reordering once and refuses the old.

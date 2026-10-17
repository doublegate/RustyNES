# Netplay: the rollback session and its window

This is the spec for `rustynes-netplay`'s session layer: how
`RollbackSession` keeps and uses its rollback window (v3.4.0,
`T-NETPLAY-ROLLBACK-WINDOW`). Transport, NAT traversal, WebRTC and the lobby are in
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

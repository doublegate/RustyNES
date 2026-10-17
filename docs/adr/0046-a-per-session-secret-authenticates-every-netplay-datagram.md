# 46. A per-session secret authenticates every netplay datagram

Date: 2026-10-16

## Status

Accepted. Plan row 11 of `to-dos/plans/v3.4.0-plan.md`
(`T-NETPLAY-DATAGRAM-AUTH`, Round 2 NET-R2-07). Protocol 9 (`"RNE9"`).

## Context

Until v3.4.0 a native netplay session took any datagram that decoded. A host
that could reach a peer's UDP port, without being on the path, could end the
match with one forged `Checksum` (a desync) or one `Sync` for another game (a
ROM mismatch), or corrupt a prediction with an `Input`. The v3.2.0 audit
import added source-address filters; they were reverted (`c752446f`) because
they ended a session whenever the peer's NAT rebound its public port, or the
peer reappeared as its IPv4-mapped IPv6 address, and they authenticated
nothing: a UDP source is trivially spoofed.

The browser path is not affected: a WebRTC data channel runs over DTLS.

## Decision

1. **Every native datagram after the handshake is sealed**: a marker byte
   (`0xA9`), the sender index, a 64-bit sequence number, the encoded message,
   and a 128-bit truncated HMAC-SHA-256 tag over all of it
   (`rustynes_netplay::auth`). The receiver checks length, marker, sender,
   replay window and tag, in that order, before the message is parsed. A
   refused datagram changes no state.
2. **The key is per session.** Each peer draws a 16-byte key share from the
   OS CSPRNG (`getrandom`) and sends it in its handshake `Sync`; the mesh host
   distributes every peer's share in the `Roster`. Both sides derive, HKDF
   style with HMAC as the PRF, one extract over the shares (in player order)
   and the `SessionIdentity` (epoch, ROM hash, configuration hash), then one
   key per sender. In a pair the sender indices come from sorting the two
   shares, so neither side needs to know which one hosted; in a mesh they are
   the player indices.
3. **The address is not trusted, in either direction.** A datagram that opens
   is accepted from any source, and the transport then sends to that source
   (on the direct path; a TURN relay's address is pinned by its permission).
   That keeps a session alive across a NAT rebinding, which is what the
   reverted filters broke.
4. **A replay window per sender** (1024 sequences, the RFC 4303 sliding
   window kept as a ring) refuses a datagram seen before or older than the
   window, from any address.
5. **Unsealed traffic after the handshake** is refused, except that a `Sync`
   from the peer's own address, recognised by its first byte and never
   parsed, is answered with our handshake `Sync` (or, on a mesh host, the
   roster) at most every 100 ms: that peer lost our reply.
6. **Vetted crates only**: RustCrypto `hmac` 0.13 over `sha2` 0.11 (the SHA-256
   the core already uses), and `getrandom` 0.4 (already in the graph). All
   are MIT OR Apache-2.0, allowed by `deny.toml`. No primitive is implemented
   in this repository.
7. **The protocol moves to 9.** The `Sync` grows to 88 bytes (the share) under
   `"RNE9"`; a `Roster` entry ends with the peer's share; the gameplay digest
   changed in the same release (`framebuffer_digest`). `"RNE8"` joins
   `OLDER_SYNC_MAGICS` and is refused as another emulator version, naming its
   epoch. The signalling room protocol is unchanged.

## Consequences

- An off-path third party can no longer end or steer a session: the gates in
  `crates/rustynes-netplay/tests/datagram_auth.rs` send a forged checksum and
  a forged `Sync` every tick into a running pair of sessions, and it runs on;
  a forgery, a replay and a sealed datagram under a guessed key are each
  refused; a peer whose source address changes is kept and followed.
- **It does not stop an attacker who observes the handshake.** The shares
  travel in the clear, so anyone on the path (or on the same broadcast LAN
  segment) can derive the key. Defending against that needs a key exchange
  (X25519) and a way to authenticate it, for example a fingerprint carried
  over the signalling server's TLS or shown to both players; both are left
  for a later release. Nothing is encrypted; the payload is only made
  unforgeable.
- The handshake itself (`Sync`, `Roster`) is still unauthenticated, as before:
  a party that can race the real joiner's first `Sync` to the host can take
  its seat. That is the pre-existing first-come adoption, unchanged.
- Spectators are not keyed: a spectator only listens, and no host-side
  spectator broadcast exists yet. When one is built it must seal its stream
  with a key the spectator can derive.
- 26 bytes per datagram, one HMAC per datagram sent (once per broadcast on a
  mesh) and per datagram received.
- A v3.4.0 peer and a v3.3.x peer cannot play together: the v3.4.0 side
  refuses with the reason, the v3.3.x side times out (it does not know
  `"RNE9"`), as at every earlier protocol bump.

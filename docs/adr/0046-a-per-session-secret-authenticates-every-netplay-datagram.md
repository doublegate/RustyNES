# 46. A per-session secret authenticates every netplay datagram

Date: 2026-10-16

## Status

Accepted. Plan row 11 of `to-dos/plans/v3.4.0-plan.md`
(`T-NETPLAY-DATAGRAM-AUTH`, Round 2 NET-R2-07). Protocol 9 (`"RNE9"`).
Amended 2026-11-10 (v3.7.0, protocol 10, `"RN10"`), approved by the
maintainer 2026-11-11: see the amendment at the end.

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
6. **A sealed message speaks only for its sender's seat** (mesh; added in
   the #613 review). The seal says who sent a datagram, and in a mesh the
   sender index is the player index, so an `Input`, `InputBatch` or
   `InputAck` naming another player is dropped after it opens and counted.
   Without it the key authenticated the peer but not the seat it claimed:
   any peer of a 3-4 player session could seal another seat's input or ack.
7. **Vetted crates only**: RustCrypto `hmac` 0.13 over `sha2` 0.11 (the SHA-256
   the core already uses), and `getrandom` 0.4 (already in the graph). All
   are MIT OR Apache-2.0, allowed by `deny.toml`. No primitive is implemented
   in this repository.
8. **The protocol moves to 9.** The `Sync` grows to 88 bytes (the share) under
   `"RNE9"`; a `Roster` entry ends with the peer's share; the gameplay digest
   changed in the same release (`framebuffer_digest`), and an `InputAck`
   names the acknowledging player (the mesh retransmission fix, same release).
   A gathering mesh host answers each adopted joiner's `Sync` with an
   accepted zero-share `Sync` until its roster is full, so the first joiner
   of a 3-4 player host knows the host is alive while the others arrive
   (`docs/netplay-session.md` section 2.1). `"RNE8"` joins
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

## Amendment — 2026-11-10 (v3.7.0): protocol 10, and a spectator stream not sealed with the session key

**What changed.** v3.7.0 adds two features that need messages a protocol-9
peer cannot read, so the Sync magic becomes `"RN10"` (protocol 10). The Sync
layout (88 bytes) and the per-datagram seal decided above are unchanged. A
protocol-9 peer (v3.4.x to v3.6.x) is refused at the handshake, naming its
epoch, as at every earlier bump.

- **Desync forensics** (`T-NETPLAY-DESYNC-FORENSICS`, `docs/netplay-session.md`
  section 4): opted-in peers exchange per-frame section digests, then the
  differing section bodies. These are ordinary session datagrams, sealed like
  every other.
- **Edge-relayed spectating** (`T-NETPLAY-EDGE-SPECTATE`, section 5): a host
  publishes the match identity and its confirmed inputs to a Cloudflare
  Durable Object, which fans them out to viewers.

**The departure from this ADR's Consequences.** They said that a host-side
spectator broadcast, "when one is built", "must seal its stream with a key
the spectator can derive". The relay does not:

- The relay leg is a WebSocket over TLS (`wss://`) to the hosted Worker, so it
  is encrypted and the Worker is authenticated by its certificate. The
  session secret has nothing to add on that leg.
- What travels is the match identity (the publisher's `Sync`, with its key
  share zeroed: `EdgePublisher`, `crates/rustynes-netplay/src/edge.rs`) and
  the players' confirmed inputs. That is the record a spectator exists to
  receive, never frames, states or the session secret. A viewer presents no
  key, so the match id is a viewer's whole credential: anyone who has it can
  watch. The maintainer chose (2026-11-11) to keep it that way and make the
  id hard to guess instead of adding a viewer key: a host that publishes with
  no match id gets 128 random bits as 32 hex digits
  (`NetplayConfig::edge_match_or_mint`), shown in the status bar to pass on,
  and the netplay panel warns when a typed id is shorter than 22 characters
  (`edge_match_is_guessable`). A typed short id still works; the match is
  then as private as its name.
- Writing is authenticated: the first publisher's key (a 32-hex secret,
  stored hashed) owns the match id until it has been idle for
  `SPECTATE_IDLE_SECONDS`. Only that key may publish, and only one connection
  at a time. The claim is atomic: a race found by the v3.7.0 security review
  is fixed in `6a8f88f9`. The desktop keeps its key in
  `[netplay] edge_publish_key`, so a host can re-host its own id
  (`e19245ec`).
- Integrity against the relay operator is not provided: whoever runs the
  Worker could alter inputs in flight. A viewer's copy is then wrong, but no
  player is affected, because the relay never feeds back into the session.

A direct-link spectator (v1.7.0 H8) is unchanged and still unkeyed.

**Who decided.** The maintainer chose to build, test and deploy the relay
(2026-11-08), chose a random match id by default over a viewer key
(2026-11-11), and approved this amendment, including the unsealed relay
stream, on 2026-11-11.

**Addendum, 2026-11-11 (v3.7.1).** v3.7.0 stored the minted id and re-hosted
it every session, so anyone once given it could watch every later match. The
maintainer chose a fresh id per match: `NetplayConfig::edge_match_for_session`
(replacing `edge_match_or_mint`) mints one for each hosted session and stores
none, and the netplay panel shows it. A typed id is still the host's to reuse.

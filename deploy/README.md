# RustyNES — netplay deployment bundle

This directory deploys the pieces an internet **netplay** session needs: a
**signaling server** (brokers the rendezvous), a **TLS reverse proxy** (so an
`https` page / a `wss://` client can reach the relay), and a **STUN/TURN server**
(NAT traversal).

**One stack, two clients.** The exact same signaling + Caddy-TLS + coturn stack
serves **both**:

- the **browser (WebRTC)** path — the relay brokers the SDP offer/answer + ICE
  candidates; ICE (the browser's own STUN/TURN agent) does the traversal; and
- the **mobile / native UDP** path (v1.8.7 room-code netplay) — the *same* relay
  routes the `PublicAddr` raw-`IP:port` rendezvous, STUN gives each peer its
  reflexive address, and the peers UDP-hole-punch directly. This is the path two
  phones behind carrier-grade NAT (CGNAT) use.

LAN / direct-IP native netplay needs none of this — it is only the *internet*
paths (browser + mobile room-code) that need a hosted relay.

See `docs/netplay-webrtc.md` for the protocol — §2.5 (mobile native-UDP
rendezvous) and §3 (browser WebRTC) — and how each frontend plugs in.

## Status

**Deployment-ready; live verification pending.** Every piece below builds and is
unit/loopback-tested, and this bundle is turn-key (`docker compose up` on a host
with a domain brings up signaling + STUN/TURN). A real end-to-end internet
session — browser WebRTC ICE, or the mobile room-code STUN/punch, plus a live
signaling round-trip — **cannot be exercised headlessly** and has **not** been
run here. It is the maintainer's manual step: the copy-pasteable checklists are
the [Mobile room-code checklist](#mobile-room-code-checklist-maintainer-ops) and
the [Manual verification checklist](#manual-verification-checklist) below.

## What's here

| File | Role |
|---|---|
| `Dockerfile` | Builds + runs the `rustynes-netplay` `signaling_server` example (`--features signaling-server`). |
| `docker-compose.yml` | Wires `signaling` + `caddy` (TLS proxy, `wss://`) + `coturn` (STUN/TURN). |
| `Caddyfile` | Caddy config: terminate TLS, proxy WebSocket upgrades to the relay. |
| `turnserver.conf` | Minimal coturn STUN + TURN config (credential/realm injected from env). |
| `.env.example` | Template for the per-deploy values (`DOMAIN`, `TURN_*`); copy to `.env`. |
| `.dockerignore` lives at the **workspace root** | Keeps `target/`, ROMs, docs out of the build context. |

The signaling relay carries **no gameplay traffic** — for the browser path it
only relays the SDP offer/answer + ICE candidates, and for the mobile path it
only relays the `PublicAddr` reflexive addresses; in both cases the game state
flows directly between peers (over the WebRTC data channel, or over the punched
UDP socket). The lone exception is a **TURN-relayed** symmetric-NAT pair, where
coturn carries the media (browser path only today — see the mobile caveat below).

> **No COOP/COEP / SharedArrayBuffer required.** Browser netplay uses a WebRTC
> `RtcDataChannel` (and the audio path is an `AudioWorklet`); neither needs
> cross-origin isolation. So the page hosting the wasm build does **not** need
> `Cross-Origin-Opener-Policy` / `Cross-Origin-Embedder-Policy` headers, and the
> existing GitHub Pages deploy works unchanged.

## Run it (local two-tab test)

```bash
cd deploy
DOMAIN=localhost docker compose up --build
```

- Caddy serves `wss://localhost/` with its **internal self-signed CA**.
- **Accept the self-signed cert once:** open **`https://localhost/`** in your
  browser and accept the security warning. You should then see a small
  *"RustyNES signaling server is up…"* health page (HTTP 200). A 502 here means
  Caddy can't reach the `signaling` container (check `docker compose ps`).
- The signaling relay is reachable only through Caddy on the internal network.
- coturn provides STUN/TURN on `:3478` (TURN credential defaults
  `rustynes:changeme` — fine for a local test).

Then build the wasm frontend pointing at it (below) and open it in two tabs.

## Run it (public deploy)

1. Point a DNS `A`/`AAAA` record at the host (e.g. `signaling.example.com`).
2. Copy the env template and fill in real values:

   ```bash
   cd deploy
   cp .env.example .env
   # then edit .env:
   #   DOMAIN=signaling.example.com      # your real hostname
   #   TURN_USER=rustynes                # any username
   #   TURN_SECRET=<a-strong-secret>     # REPLACE `changeme`
   #   TURN_REALM=signaling.example.com  # your domain
   ```

   `docker compose` auto-loads `.env` from this directory; `.env` is gitignored.
   No file in this bundle hard-codes a real domain or secret — they all come
   from `.env`, so you point the stack at your host **without a rebuild**.
3. For a **real domain**, drop `tls internal` from the `Caddyfile` so Caddy
   provisions a Let's Encrypt cert automatically, then bring it up:

   ```bash
   docker compose up --build -d
   ```

4. If coturn sits behind 1:1 NAT and can't self-detect its public address, add
   an `--external-ip=YOUR.PUBLIC.IP` line to the `coturn` `command:` in
   `docker-compose.yml`.
5. Open `443` (and `3478/udp` + `3478/tcp` plus coturn's relay port range) on
   the firewall. On Docker Desktop (no host networking), map coturn's ports
   explicitly in `docker-compose.yml` instead of `network_mode: host`.

## Point the wasm build at it

The browser lobby (the "Netplay (browser)" panel in the `~` debugger overlay)
takes the signaling URL + room code at runtime. To bake defaults in, set the
frontend `[netplay]` config:

```toml
[netplay]
signaling_url = "wss://signaling.example.com"
stun_servers = [
  "stun:signaling.example.com:3478",
  "turn:signaling.example.com:3478?transport=udp",
]
```

(For a local test, `signaling_url = "wss://localhost"` and the public
`stun_servers` default is fine — STUN alone traverses most home NATs.) These are
`#[serde(default)]` fields: an existing config with no `[netplay]` section, or no
`signaling_url`, loads byte-identically and leaves browser netplay off until the
user types a URL in the lobby.

Build + serve the wasm frontend (see the project `CLAUDE.md` / `docs/`):

```bash
cd ../crates/rustynes-frontend/web
trunk build --release
# serve dist/ from your https host, or `trunk serve` for local dev
```

## Point the mobile (Android) build at it

The Android **room-code** netplay (v1.8.7) uses the *same* deployed stack. The
app passes an `NpNetConfig` to `np_host_room` / `np_join_room`, with the
endpoints overridable in **Settings**. Map the `.env` values you set above onto
the config exactly like this:

| `NpNetConfig` field | Value (from your `.env`) |
|---|---|
| `signaling_url` | `wss://<DOMAIN>` — Caddy serves `wss://` on `DOMAIN`, proxying WebSocket upgrades to the relay at the **root** path (e.g. `wss://signaling.example.com`). The relay speaks WS at `/`, so no path suffix. |
| `stun_servers` | leave empty to use the built-in default (`stun.l.google.com:19302` + `stun1`), **or** point at your own: `["stun:<DOMAIN>:3478"]`. |
| `turn_url` | `turn:<DOMAIN>:3478` (optional — enables the symmetric-NAT fallback). |
| `turn_user` | `<TURN_USER>` (the username in your `.env`). |
| `turn_secret` | `<TURN_SECRET>` (the long-term credential you replaced `changeme` with). |

`turn_url` + `turn_user` + `turn_secret` are only wired up **when all three are
present**; otherwise the session is punch-or-fail (cone-NAT only). An empty
`stun_servers` falls back to `DEFAULT_STUN_SERVERS`.

> **Placeholder default — replace it.** The shipped app defaults
> `signaling_url` to a **placeholder** `wss://relay.rustynes.example/ws`, which
> does not resolve. Until you host this `deploy/` stack and substitute your real
> `wss://<DOMAIN>`, mobile room-code netplay cannot connect. (Direct-IP / LAN
> netplay is unaffected — it needs no relay.)
>
> **Symmetric-NAT relay is not yet wired for mobile.** The TURN *client* is
> implemented and a `turn_url`/creds trio configures it, but routing live
> gameplay over the relay is a tracked carryover (`docs/netplay-webrtc.md` §2.5):
> symmetric-NAT mobile pairs do not yet relay. Cone NAT (the common home/CGNAT
> case) hole-punches end-to-end without TURN.

### Mobile room-code checklist (maintainer ops)

Standing the mobile path up is the same host as the browser path plus pointing
the app at it:

- [ ] Host this `deploy/` stack (the **Ops / hosting** steps below) — host,
      domain, TLS, coturn — once; it serves both the browser and mobile paths.
- [ ] In the app's Settings, set `signaling_url` to `wss://<DOMAIN>` (replace
      the `wss://relay.rustynes.example/ws` placeholder).
- [ ] (Optional) set the TURN trio (`turn_url`/`turn_user`/`turn_secret`) to
      match `.env` for the symmetric-NAT fallback; leave STUN empty for the
      default servers.
- [ ] Host a room on one device → share the 6-char room code → join from the
      other device. Both should pass `Negotiating` (Discovering → Exchanging →
      Punching) into the in-game session.
- [ ] For a true CGNAT test, run the two devices on **two different cellular
      networks** (not the same Wi-Fi) — that exercises the live STUN + punch
      across real carrier NAT.

## Manual verification checklist

This is the maintainer's hands-on run — **not done in CI / by the build**. Tick
the ops steps, then walk the connectivity matrix from cheapest (one machine, two
tabs) to fullest (four players across machines).

### Ops / hosting (do once)

- [ ] DNS `A`/`AAAA` record points at the host (`signaling.example.com`).
- [ ] `cp deploy/.env.example deploy/.env` and set `DOMAIN`, `TURN_USER`,
      a strong `TURN_SECRET`, `TURN_REALM`.
- [ ] Real domain: removed `tls internal` from `Caddyfile` (Let's Encrypt).
- [ ] `docker compose up --build -d`; `docker compose ps` shows all three
      services healthy.
- [ ] Firewall: `443/tcp` open; `3478/udp` + `3478/tcp` open; coturn relay port
      range reachable.
- [ ] coturn behind 1:1 NAT → `--external-ip=` flag added.
- [ ] TLS check: `https://signaling.example.com/` returns the 200 health page;
      cert is valid (no warning on a real domain).
- [ ] Frontend `[netplay] signaling_url` + `stun_servers` point at the deploy
      (or entered live in the lobby).

### Connectivity matrix (escalating)

- [ ] **2 tabs, one machine** — open the wasm build in two browser tabs; Host in
      one, Join the same room code in the other; confirm both reach "in-game"
      and play stays in sync (same on-screen state, no desync). Validates the
      signaling round-trip + data channel locally.
- [ ] **2 machines, same LAN** — repeat across two devices on one network;
      validates real ICE candidate exchange + local-network traversal.
- [ ] **2 machines, different networks (real NAT)** — one peer off-LAN (e.g.
      mobile hotspot). Validates STUN hole-punching across real NATs.
- [ ] **2 mobile devices, two cellular networks (room code, real CGNAT)** — host
      a room on one Android device, share the code, join from another device on a
      *different* carrier network. Validates the v1.8.7 native-UDP rendezvous
      (`PublicAddr` over the same relay) + live STUN/punch through carrier-grade
      NAT. (Symmetric-NAT mobile relay is a known carryover — see §2.5.)
- [ ] **Symmetric-NAT fallback (TURN relay)** — force a relay path (a symmetric
      NAT, or temporarily STUN-disabled). Confirms coturn relays the data
      channel. **Watch TURN bandwidth:** relayed traffic flows through your box
      and costs egress — every relayed player-pair's input/rollback traffic
      transits coturn, so a busy relay can run up bandwidth. Monitor and rate-
      limit/quota coturn for a public deploy.
- [ ] **3-player mesh** — three peers, room size 3; confirm the N-peer mesh
      forms (each peer connects to both others) and gameplay stays in sync.
- [ ] **4-player mesh (Four Score)** — four peers, room size 4; confirm full
      mesh + sync. This is the fullest path.
- [ ] **Disconnect handling** — drop one peer mid-session; confirm the lobby
      surfaces it (`peer-left`) and the remaining peers behave sanely.

### TURN-bandwidth ops caveat

STUN is cheap (the relay only learns each peer's public mapping; gameplay still
flows peer-to-peer). **TURN is not**: when a peer is behind a symmetric NAT, its
data channel is *relayed through coturn*, so that traffic counts against your
host's bandwidth/egress for the whole session. Budget for it, monitor coturn's
relay usage, and apply coturn quotas / `total-quota` / `bps-capacity` limits on a
public deploy.

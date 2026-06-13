# RustyNES v2 — browser-netplay deployment bundle

This directory deploys the pieces a **browser (WebRTC) netplay** session needs:
a **signaling server** (brokers the WebRTC handshake), a **TLS reverse proxy**
(so an `https` page can reach the relay as `wss://`), and a **STUN/TURN server**
(NAT traversal). Native UDP netplay needs none of this — it is for the wasm
build only.

See `docs/netplay-webrtc.md` for the protocol + how the wasm frontend plugs in.

## What's here

| File | Role |
|---|---|
| `Dockerfile` | Builds + runs the `nes-netplay` `signaling_server` example (`--features signaling-server`). |
| `docker-compose.yml` | Wires `signaling` + `caddy` (TLS proxy, `wss://`) + `coturn` (STUN/TURN). |
| `Caddyfile` | Caddy config: terminate TLS, proxy WebSocket upgrades to the relay. |
| `turnserver.conf` | Minimal coturn STUN + TURN config. |

The signaling relay carries **no gameplay traffic** — it only relays the SDP
offer/answer + ICE candidates so the two browsers form a peer-to-peer WebRTC
data channel; the game state flows directly between peers over that channel.

## Run it (local two-tab test)

```bash
cd deploy
DOMAIN=localhost docker compose up --build
```

- Caddy serves `wss://localhost/` with its **internal self-signed CA**.
- **Accept the self-signed cert once:** open **`https://localhost/`** in your
  browser, accept the security warning. You should then see a small
  *"RustyNES v2 signaling server is up…"* health page (HTTP 200). Until v2.7.1
  this returned a **502 Bad Gateway** — that was harmless (the signaling server
  is WebSocket-only and rejected the plain page GET; the cert still got
  accepted), but the Caddyfile now serves a friendly health response for plain
  visits and proxies only real `wss://` upgrades. A 502 here now means Caddy
  truly can't reach the `signaling` container (check `docker compose ps`).
- The signaling relay is reachable only through Caddy on the internal network.
- coturn provides STUN/TURN on `:3478`.

Then build the wasm frontend pointing at it (below) and open it in two tabs.

## Run it (public deploy)

1. Point a DNS `A`/`AAAA` record at the host (e.g. `signal.example.com`).
2. Bring it up with that domain — Caddy auto-provisions a Let's Encrypt cert:

   ```bash
   cd deploy
   DOMAIN=signal.example.com docker compose up --build -d
   ```

3. Edit `turnserver.conf`: set a strong `user=` credential, set `realm=` to your
   domain, and uncomment `external-ip=` with the box's public IP. For a real
   domain also drop `tls internal` from the `Caddyfile`.
4. Open `443` (and `3478/udp`+`3478/tcp` plus coturn's relay range) on the
   firewall. On Docker Desktop (no host networking), map coturn's ports
   explicitly in `docker-compose.yml` instead of `network_mode: host`.

## Point the wasm build at it

The browser lobby (the "Netplay (browser)" panel in the `~` debugger overlay)
takes the signaling URL + room code at runtime. To bake defaults in, set the
frontend `[netplay]` config:

```toml
[netplay]
signaling_url = "wss://signal.example.com"
stun_servers = [
  "stun:turn.example.com:3478",
  "turn:turn.example.com:3478?transport=udp",
]
```

(For a local test, `signaling_url = "wss://localhost"` and the public
`stun_servers` default is fine — STUN alone traverses most home NATs.)

Build + serve the wasm frontend (see the project `CLAUDE.md` / `docs/`):

```bash
cd ../crates/nes-frontend/web
trunk build --release
# serve dist/ from your https host, or `trunk serve` for local dev
```

## Verification status

- **Shipped + buildable:** this bundle (Docker images build; the signaling
  server is a tested relay; the wasm lobby is wired + compiles for both wasm
  flavours).
- **Needs a live deploy + two browsers:** an actual end-to-end browser netplay
  session. WebRTC ICE + a real signaling round-trip cannot be exercised
  headlessly, so the live session is a documented manual step (see
  `docs/netplay-webrtc.md` §4).
- **Follow-up:** 3-4 player browser netplay needs the N-peer mesh signaling
  (the 2-player WebRTC path is wired today).

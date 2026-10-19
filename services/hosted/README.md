# RustyNES hosted services

One Cloudflare Worker (v3.4.0, `T-HOSTED-NETPLAY` and `T-RA-PROXY`) that
gives the RustyNES clients three things they cannot get from a static page:

| Route | What it is | Source |
| --- | --- | --- |
| `GET /signal` (or `/`) | The netplay lobby: the reference signalling room protocol over a WebSocket, in one Durable Object with the hibernation API | `src/lobby.ts`, `src/protocol.ts` |
| `POST /turn-credentials` | Short-lived Cloudflare Realtime TURN credentials, behind the spend guards (GET is refused: see "Spend guards") | `src/turn.ts`, `src/budget.ts` |
| `POST /ra/dorequest.php` | The browser RetroAchievements proxy (ADR 0015): an allowlisted forward with RustyNES's User-Agent, behind its own abuse guards | `src/ra.ts`, `src/budget.ts` |
| `GET /healthz` | Liveness | `src/index.ts` |

**The service is public.** It runs at a `*.workers.dev` address that anyone on
the Internet can reach, not only RustyNES. Every limit below exists because of
that, and `PRIVACY.md` at the repository root says so to users.

## Contents

- [Design notes](#design-notes)
- [Spend guards](#spend-guards)
- [Local development and tests](#local-development-and-tests)
- [Deploy runbook](#deploy-runbook)
- [Verifying the live gates](#verifying-the-live-gates)
- [Rollback and the kill switches](#rollback-and-the-kill-switches)

## Design notes

**The room protocol is ported, byte for byte.** `src/protocol.ts` is a port of
`crates/rustynes-netplay/src/signaling.rs` (RustyNES's own code), including the
reference parser's leniencies, so a client that works against
`examples/signaling_server.rs` works unchanged here. That is checked, not
asserted: `crates/rustynes-netplay/tests/hosted_signaling_vectors.rs` records
the reference's exact outbound frames for 12 scenarios (72 steps) into
`test/vectors/signaling-vectors.json` and fails if the Rust code stops
producing them; `test/conformance.test.ts` replays the same file against the
TypeScript relay, then end to end over real WebSockets into the Durable
Object, then again with the object evicted (its sockets hibernated) before
every sending step. Re-record after an intentional protocol change with
`RUSTYNES_SIGNALING_VECTORS_BLESS=1 cargo test -p rustynes-netplay --test hosted_signaling_vectors`.

**One lobby object, not one per room.** A client opens one socket to a fixed
URL and names a room only afterwards, and `list-rooms` / `quick-match` span
every room, so routing a socket to a per-room object would need a different
protocol. Signalling is a few small frames per session, far below one object's
throughput, and hibernated sockets cost no duration. `src/lobby.ts` explains
the alternative if that ever changes.

**Boundary rules the reference server does not have** (a public service needs
them; a well-formed client never meets them): frames over 16 KiB refused; room
codes `[A-Za-z0-9_-]{1,32}`; ROM hashes empty or up to 64 hex digits; a message
token bucket per socket; per-IP socket cap, connection-attempt rate and
room-creation rate; a global socket cap; browsers from any origin other than
the Pages site (and localhost) refused. A refusal is the reference's own shape:
an `error` frame, then the socket is closed.

**Privacy by construction.** The Worker never stores an IP address: the Durable
Objects keep a salted SHA-256 key (`src/limits.ts`) and only for as long as a
limit needs it. Nothing logs a request body, and Workers Logs are off in
`wrangler.jsonc`. RetroAchievements passwords and tokens pass through
`src/ra.ts` and are never stored or logged (a test spies on every console
method to prove it).

## Spend guards

Cloudflare bills Realtime TURN egress past the free monthly allowance to the
account that owns the TURN key, i.e. the maintainer's. Each guard is a var in
`wrangler.jsonc` and is pinned by a test in `test/turn.test.ts` (the
RetroAchievements rows in `test/ra.test.ts`).

| Guard | Var (deployed value) | Behaviour | Test |
| --- | --- | --- | --- |
| Per-IP rate limit | `TURN_PER_IP_PER_HOUR` (6) | 429 `rate-limited` with `Retry-After`; the window is in Durable Object storage, so it survives eviction | `limits one IP with 429 and Retry-After`, `keeps per-IP windows across an eviction` |
| Daily global cap | `TURN_DAILY_CAP` (500) | 503 `daily-cap` with `Retry-After` until 00:00 UTC; failed upstream mints are refunded | `caps credentials per UTC day across all callers, and resets the next day`, the six `... is a 502 and does not spend the budget` tests |
| Short TTL, enforced | `TURN_TTL_SECONDS` (10800, clamped 600-86400) | Credentials are asked to expire after 3 h, and the budget object revokes each one when its TTL ends, because Cloudflare does not refuse expired credentials (v3.4.1) | `returns short-lived ICE servers and passes the TTL upstream`, `clamps the configured TTL`, the `revocation` tests |
| Kill switch | `TURN_ENABLED` (`"true"`) | Anything else: 503 `turn-disabled`, no upstream call | `the kill switch refuses without calling upstream` |
| POST only | (none) | A GET is 405 before the budget is consulted, so another page cannot spend the cap through its visitors' browsers | `a GET with no Origin, as an <img> on another page sends it, is refused and spends nothing` |
| RA per-IP rate limit | `RA_PER_IP_PER_HOUR` (240) | 429 with `Retry-After`; in Durable Object storage (the `ra` instance) | `limits one address per hour with 429 and Retry-After` |
| RA login rate limit | `RA_LOGINS_PER_IP_PER_HOUR` (10) | `login2` calls only; 429 with `Retry-After` | `limits logins per address more tightly than other calls` |
| RA daily global cap | `RA_DAILY_CAP` (50000) | 503 with `Retry-After` until 00:00 UTC | `caps forwarded calls per UTC day across all addresses` |
| RA kill switch | `RA_ENABLED` (`"true"`) | Anything else: 503, no upstream call | `the kill switch refuses` |
| Room-creation rate (lobby) | `LOBBY_ROOMS_PER_IP_PER_MINUTE` (10) | `error` `rate limited`, then close | `test/lobby.test.ts`: `limits room creation per IP` |

**Why 3 hours.** A TURN allocation is kept alive by Refresh requests that
authenticate with the same credential; once it expires the next Refresh is
refused and the allocation lapses (RFC 8656 section 7, 10-minute default
lifetime). Neither RustyNES client re-fetches credentials mid-session, so the
TTL must outlive the session, as Cloudflare's own guidance says. Three hours
covers a long sitting with one game and is the shortest value that does; a
longer session drops its relay and reconnects through the lobby, which mints
fresh credentials. Shorter is better because the TTL is how long a leaked
credential stays usable. `src/turn.ts` has the full argument.

**The TTL is enforced by revocation (v3.4.1).** Cloudflare does not refuse a
credential once the TTL it was minted with has passed: on 2026-10-19 a 3-hour
credential still obtained a relay 2 h 38 min after its expiry, and a 600 s one
6.6 minutes after, although Cloudflare's own page says an expired credential is
disconnected. So the Worker revokes each credential itself when its TTL ends,
through Cloudflare's revoke endpoint, from the budget object's alarm; a failed
revocation is retried every 5 minutes, six attempts in all. `src/budget.ts`,
"Revoking credentials when their TTL ends", has the details.

**Why POST only.** A request without `Origin` is served, because that is how
the native apps look. But a browser also omits `Origin` on a GET it sends
without CORS, such as an `<img src>` on any page, so a minting GET would let a
third-party page spend one credential of the shared daily cap per visitor. A
browser always sends `Origin` on a POST (the Fetch standard), so with POST only
every browser request either carries the RustyNES site's origin or is refused
with a 403. `src/turn.ts` has the detail; both RustyNES clients send POST.

**Why the RetroAchievements proxy has guards too.** Its Origin check keeps
other web pages out, but a program can send any `Origin`, and every call it
pushes through goes to retroachievements.org under RustyNES's shared
User-Agent. Unmetered, that is a path for credential stuffing (`login2`) and a
way to get that identity banned for every browser user. The guards count only
requests that would be forwarded (a refused call type costs nothing). The
defaults leave room for the casual runtime, which makes about one call a
minute while a game runs; `src/budget.ts` has the reasoning.

**What the guards do not bound.** They bound how many credentials exist, not
how many bytes one carries: a credential can relay whatever its holder sends
until it is revoked at the end of its TTL. That is why the budget alert below is a deploy prerequisite,
not an option.

**What clients do when refused.** Every refusal is JSON
`{"error": code, "message": text}`. The Rust parser
(`crates/rustynes-netplay/src/hosted.rs`) turns it into
`HostedIceError::Refused`; the browser and the mobile apps then continue with
STUN only (a direct, hole-punched connection), and the browser shows the
message.

## Local development and tests

```bash
cd services/hosted
npm ci              # the exact tree in package-lock.json (all versions pinned)
npm run typecheck   # tsc --noEmit
npm test            # vitest in workerd, no network
npm run dev         # wrangler dev on http://localhost:8787 (local only)
```

`npm run dev` needs no Cloudflare login. Without secrets `/turn-credentials`
answers 503 `turn-not-configured`, which is the honest local result; to try
the mint locally, put the two secrets in `services/hosted/.dev.vars` (listed in
the repository `.gitignore`; never commit it). Point the web build at
the local service by setting `hosted_base_url = "http://localhost:8787"` in the
`[netplay]` config table, or `RUSTYNES_HOSTED_BASE=http://localhost:8787` when
building.

With `npm run dev -- --port 8799` running, the production Rust signalling
client can be checked against the Worker directly (a full join, peer-joined,
public-addr in both directions, peer-left):

```bash
RUSTYNES_HOSTED_LOBBY_URL=ws://127.0.0.1:8799/signal \
  cargo test -p rustynes-netplay --test hosted_lobby_live -- --ignored
```

After a deploy the same test takes `wss://<hosted base>/signal`.

The tests use Cloudflare's Vitest plugin against `wrangler.jsonc`, so the
bindings, Durable Objects and storage are the real runtime's. Only outbound
`fetch` (Cloudflare's TURN API, retroachievements.org) is mocked. CI runs the
same commands (`.github/workflows/hosted.yml`).

## Deploy runbook

Run by the maintainer, on the maintainer's Cloudflare account, from
`services/hosted/`. Nothing in this repository deploys automatically.

### 0. Before the first deploy: a budget alert

Do this first, so that a surprise is an email and not an invoice.

1. **What is billed.** Read on 2026-10-16:
   [Realtime pricing](https://developers.cloudflare.com/realtime/sfu/platform/pricing/)
   (page last updated 2026-09-22) and the
   [TURN FAQ](https://developers.cloudflare.com/realtime/turn/faq/). Realtime
   TURN costs **$0.05 per GB of egress** (data sent from Cloudflare to the TURN
   client, TURN overhead included). The **first 1,000 GB each month are free,
   shared with the Realtime SFU** (one allowance, not one each). Ingress is free,
   and Cloudflare's STUN is free and unlimited. The Worker and Durable Objects
   have their own pricing: on the Workers Free plan, exceeding a free daily
   limit makes further requests fail rather than bill; on Workers Paid, usage
   past the included amounts is billed
   ([Durable Objects pricing](https://developers.cloudflare.com/durable-objects/platform/pricing/)).
   The plan's inference (`to-dos/plans/v3.4.0-plan.md` row 1) is about 22 MB
   per player-hour, so 1,000 GB is roughly 45,000 relayed player-hours a month;
   only sessions that cannot connect directly use TURN at all.
2. **Set the alert.** In the dashboard: **Manage Account > Billing > Billable
   Usage > Set Budget Alert**, enter a dollar threshold (for example $1, so the
   first cent past the free tier is visible), **Create**. The same alert is
   under **Notifications > Add > Budget Alert**
   ([Budget alerts](https://developers.cloudflare.com/billing/manage/budget-alerts/),
   [changelog 2026-04-13](https://developers.cloudflare.com/changelog/post/2026-04-13-billable-usage-dashboard-and-budget-alerts/)).
   Know its limits: it is informational (it caps nothing), and usage is
   processed once a day, so it fires the day after the threshold is crossed.
   Pay-as-you-go accounts got a default $10 alert in mid-2026
   ([changelog 2026-06-15](https://developers.cloudflare.com/changelog/post/2026-06-15-budget-alerts-default-on/));
   check it exists and lower it.
3. **Know the backstop.** If spend runs away, the kill switch
   (`TURN_ENABLED`, below) stops new credentials within one deploy of a var;
   credentials already issued stay valid until the Worker revokes them at the
   end of their TTL (at most 3 h), and can be revoked earlier, one at a time,
   through the TURN API's revoke endpoint.

### 1. Log in and install

```bash
cd services/hosted
npm ci
npx wrangler login          # browser OAuth to the maintainer's account
npx wrangler whoami         # confirm the account before anything else
```

### 2. Create the TURN key

Dashboard: open the Realtime section
(<https://dash.cloudflare.com/?to=/:account/calls>, the link Cloudflare's
[Generate Credentials](https://developers.cloudflare.com/realtime/turn/generate-credentials/)
page gives), go to TURN, and create a key named `rustynes-hosted`. Cloudflare
shows the key's **Turn Token ID** and **API Token**; copy both straight into
the next step. (API alternative: the
[create TURN key](https://developers.cloudflare.com/api/resources/calls/subresources/turn/methods/create/)
endpoint.) A TURN key is a long-term secret: it stays in Worker secrets and is
never given to a client; clients only ever see the 3-hour credentials minted
from it.

### 3. Set the secrets

```bash
npx wrangler secret put TURN_KEY_ID          # paste the Turn Token ID
npx wrangler secret put TURN_KEY_API_TOKEN   # paste the API Token
```

On the first run the Worker does not exist yet; `wrangler secret put` offers to
create it, which is expected.

### 4. Review the vars, then deploy

Check `wrangler.jsonc` `vars` (origins, the spend guards, `RA_USER_AGENT`,
which must name the release being shipped), then:

```bash
npx wrangler deploy
```

The service is then at **`https://rustynes-hosted.<account-subdomain>.workers.dev`**,
where `<account-subdomain>` is the account's workers.dev subdomain (Workers &
Pages > Overview shows it). That URL is the **hosted base**.

### 5. Point the clients at it

- **Web build:** add the RA proxy base to `crates/rustynes-frontend/web/index.html`
  as `<meta name="rustynes-ra-proxy-base" content="https://rustynes-hosted.<sub>.workers.dev/ra">`
  (read by `web/cheevos/ra_glue.js`; only when the `browser-cheevos` build is
  shipped), and build the Pages site with
  `RUSTYNES_HOSTED_BASE=https://rustynes-hosted.<sub>.workers.dev` in the
  `trunk build` environment (`web.yml`), which makes the browser lobby default
  to the hosted lobby and fetch TURN credentials.
- **Android / iOS:** build the bridge with the same `RUSTYNES_HOSTED_BASE`.
  An empty signaling URL in Settings then means the hosted lobby, and a missing
  TURN trio means "ask the hosted service". A user's own URLs always win.
- The F-Droid build is made by F-Droid from source without that variable, so
  it never contacts the hosted lobby (`docs/android.md`, F-Droid
  anti-features).

## Verifying the live gates

The plan's gates for rows 1-3, in the order to run them. None can run in CI:
each needs the deployed service, real browsers or devices, or a real account.

1. **Smoke:** `curl https://rustynes-hosted.<sub>.workers.dev/healthz` prints
   `ok`; the `hosted_lobby_live` test above passes against
   `wss://rustynes-hosted.<sub>.workers.dev/signal`; `curl -i -X POST .../turn-credentials` returns 200 with `iceServers`, `ttl`
   10800 and an `expiresAt` three hours ahead; repeat it seven times from one
   address and the seventh is a 429; `curl -i .../turn-credentials` (a GET)
   is a 405.
2. **Two browsers through the hosted lobby (row 1):** open the Pages build in
   two browsers (two machines, or two profiles), load the same ROM, join the
   same room code. Both reach `InGame`; the frames stay in sync.
3. **A relayed session across two NATs (row 1):** two devices on networks that
   cannot hole-punch (two mobile carriers behind CGNAT usually qualify; or
   block UDP between them with a firewall). The session connects, and the
   relay is in use: `chrome://webrtc-internals` shows the selected candidate
   pair of type `relay`, and the native apps' netplay status reports
   `relayed`. Cloudflare's TURN analytics show the egress.
4. **Credentials expire (row 1):** take the `username`/`credential` from step 1
   and, more than 3 h later (plus a minute for the revocation alarm), try them
   in an ICE test page (for example the WebRTC samples' "Trickle ICE" page): no
   `relay` candidate is gathered. Within the TTL the same test gathers one. To
   check it faster, deploy with `TURN_TTL_SECONDS` 600, mint one credential,
   redeploy the normal value, and test 11 minutes later. Before v3.4.1 this
   step failed: Cloudflare kept honouring expired credentials.
5. **Spend guards live:** set `TURN_ENABLED` to `false` in the dashboard (it
   deploys a new version of the same code); `/turn-credentials` answers 503
   `turn-disabled` and a browser session still connects directly. Set it back.
6. **RetroAchievements (row 2, RA-02):** the maintainer logs in from the
   Pages build with their own account and earns a casual unlock; the unlock
   appears on retroachievements.org. A hardcore request is refused by the proxy
   by design.
7. **The first month's traffic (row 1):** after 30 days, read the TURN egress
   (dashboard Realtime > TURN analytics, or the GraphQL Analytics API's
   `callsTurnUsageAdaptiveGroups` dataset, `sum { egressBytes }`, documented in
   [Replacing existing TURN](https://developers.cloudflare.com/realtime/turn/replacing-existing/))
   and the Billable Usage page, and record the measured bytes per relayed
   player-hour against the plan's 22 MB inference, in the v3.4.x release notes
   or `docs/netplay-webrtc.md` section 3.5. No usage forecast exists, so this
   is the first real number.

## Rollback and the kill switches

- **Stop TURN spend now:** dashboard **Workers & Pages > rustynes-hosted >
  Settings > Variables and Secrets**, set `TURN_ENABLED` to `false`, save
  (or edit `wrangler.jsonc` and `npx wrangler deploy`). Clients fall back to
  STUN only. To also kill credentials already issued, delete the TURN key in
  Realtime > TURN Server (every credential minted from it stops working).
- **Stop the RA proxy:** the same, with `RA_ENABLED` set to `false`.
- **Roll back a bad deploy:** `npx wrangler deployments list`, then
  `npx wrangler rollback <version-id>` (or the dashboard's Deployments tab).
  The Durable Objects keep their storage across versions; the only migration is
  `v1` (creating the two SQLite-backed classes), so a rollback never needs a
  storage migration.
- **Take the whole service down:** `npx wrangler delete`. Clients built with a
  hosted base then fail to reach the lobby with a clear error, and users can
  enter their own signalling URL; nothing else in RustyNES depends on it.

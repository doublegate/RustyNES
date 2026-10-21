# RustyNES privacy policy

Effective from v3.5.0 (the TURN relay switched back on; the previous version
was effective 2026-10-19, v3.4.1). Published at
<https://doublegate.github.io/RustyNES/privacy/>; the source is `PRIVACY.md` in
the [RustyNES repository](https://github.com/doublegate/RustyNES), and its
history there is the record of every change.

## The short version

- RustyNES is an emulator that runs on your device. Playing a game sends
  nothing anywhere. There are no accounts, no analytics, no advertising and no
  tracking.
- Three **optional** online features use services the maintainer runs on
  Cloudflare: online netplay (the lobby and a relay), and RetroAchievements in
  the **web** version. They are used only when you start one of them.
- Those services keep no personal data beyond what they need for at most two
  hours to stop abuse (described below), and they never store or log your
  RetroAchievements password or token.
- The services are **public**: they run at a `*.workers.dev` address anyone on
  the Internet can reach. Nothing you send them is private from the other
  players in your room, and the limits described below exist because the
  endpoints are open.

## Who runs the services

The hosted services are run by the RustyNES maintainer (DoubleGate), the
controller for the purposes of this policy. They run on Cloudflare Workers,
Durable Objects and Cloudflare Realtime TURN. Cloudflare, Inc. processes the
traffic on the maintainer's behalf as a processor: it terminates the encrypted
connection, protects the service from attack, and runs the code. Cloudflare's
own handling of that traffic is described in
[Cloudflare's privacy policy](https://www.cloudflare.com/privacypolicy/).

RustyNES is free software (GPL-3.0-or-later); the services' code is in
[`services/hosted/`](https://github.com/doublegate/RustyNES/tree/main/services/hosted),
so every statement below can be checked against what actually runs.

## What each service receives and keeps

### Netplay lobby (signalling)

Used when you host or join an online room (the web version's lobby, or the
Android and iOS room codes, in builds that point at the hosted lobby).

- **It receives:** your IP address (every Internet connection reveals it); the
  room code; a fingerprint (SHA-256 hash) of the game file, so that players of
  different games are not paired; the number of players; and the WebRTC
  connection offers and network candidates your device generates. Those
  candidates contain your device's local and public IP addresses and ports.
- **It passes on:** the offers and candidates go to the other players in your
  room, because that is how your devices find each other. The room code, player
  count and game fingerprint of an open room are listed to anyone who asks the
  lobby for its directory of rooms.
- **It keeps:** nothing after you disconnect. While you are connected, the lobby
  holds your room membership and a pseudonymous key derived from your IP
  address (a salted SHA-256 hash, never the address itself), used to limit how
  many connections and rooms one address can open. The counters behind those
  limits are kept only in memory, never written to storage. Each counts for
  one minute; while the lobby is in use (anyone connecting or sending it a
  message), an expired counter is deleted within two minutes of being
  created, and when the lobby is idle, the counters are discarded as the
  service is unloaded from memory.

### TURN relay credentials and the relay itself

Used when your device asks the hosted service for relay credentials, which
happens at the start of an online session in builds that point at the hosted
service. If a direct connection between the players cannot be made (some home
and mobile networks prevent it), the game traffic then flows through
Cloudflare's TURN relay.

- **The credential endpoint receives:** your IP address. It keeps a salted hash
  of it, with a count, to limit how many credentials one address can obtain.
  The count covers one hour; an hourly cleanup deletes it once that hour is
  over, so it is kept for at most two hours. It also keeps a single number per
  day, the count of credentials issued, which is not linked to anyone and is
  deleted within an hour after that day ends (UTC). The credentials it returns
  are meant to expire after their lifetime (three hours as deployed; the
  service allows up to 24). The relay provider does not enforce that by
  itself, so the service keeps each credential's username (a random identifier
  the provider issues, with no link to you or your address) until the
  lifetime is up, then asks the provider to revoke the credential and deletes
  the username once the provider confirms. If the provider does not confirm,
  the request is retried and the username kept for at most 48 hours after the
  credential's lifetime ends, then deleted. The secret half of the credential
  is not stored. The relay was switched off in v3.4.1 and v3.4.2, when none of
  this was kept; it is on again from v3.5.0, so all of it is kept as
  described here while it is on.
- **The relay receives:** the game traffic of a relayed session (controller
  input and synchronisation messages, roughly 22 MB per player per hour) and
  the IP addresses of both ends. It does not receive the game itself, your
  saves or anything else on your device. Cloudflare measures the relayed
  traffic for billing, and the maintainer looks at Cloudflare's usage totals;
  the service does not tag credentials with any user or device identifier.

### RetroAchievements proxy (web version only)

Browsers do not let a web page identify itself to RetroAchievements the way the
desktop and mobile apps do, so the web version sends its RetroAchievements
requests through this proxy, which adds RustyNES's identification and forwards
them to [retroachievements.org](https://retroachievements.org).

- **It receives and forwards:** your RetroAchievements user name and password
  (when you log in) or your login token (afterwards), the game you are playing,
  and the achievements you unlock. Only requests the web version needs are
  accepted; anything else, including any hardcore-mode request, is refused.
- **It keeps:** no request content. Your password and token pass through to
  RetroAchievements and the answer passes back; the proxy does not store them,
  does not write them to any log, and does not see them again. To limit abuse
  of the proxy (for example, many login attempts from one address), it keeps a
  salted hash of your IP address with two counts, how many requests and how
  many logins it forwarded for that address, for at most two hours, exactly
  as the relay credential endpoint does; and a single number per day, the
  count of requests forwarded, not linked to anyone and deleted within an hour
  after that day ends (UTC).
- **RetroAchievements** receives what you would send it from any client, and its
  own [terms and policies](https://retroachievements.org/terms) apply to that.

The desktop and mobile apps do not use this proxy; when you enable
RetroAchievements there, they talk to retroachievements.org directly.

### Logs

Workers request logging is switched off for these services, so there is no log
of requests, addresses or headers kept by the maintainer. It may be switched on
for a short period to diagnose a fault, after which it is switched off again;
any such logs are kept by Cloudflare for its standard short retention period
and are not used for anything else. Cloudflare's own security systems may
process connection metadata as described in its privacy policy.

## Other services the apps can contact

Only when you use the feature in question:

- **STUN servers** (by default Google's public STUN servers, or the ones you
  configure, or Cloudflare's when the hosted service supplies them) learn your
  public IP address when an online session starts, so your device can discover
  it.
- **retroachievements.org** (desktop and mobile, when you enable
  RetroAchievements) and **media.retroachievements.org** (achievement badge
  images).
- **GitHub** hosts the web version (GitHub Pages) and the downloads, and sees
  the requests your browser makes to it.
- On Android, the "Find box art" action (only when you use it) fetches cover
  art from the libretro-thumbnails collection on GitHub, or from TheGamesDB if
  you entered your own API key in Settings, sending the game's name.

## On your device

Settings, save states, battery saves and, if you log in, your RetroAchievements
token are stored on your device so that you stay logged in. They are not sent
anywhere except, for the token, to RetroAchievements (through the proxy in the
web version).

## Children

RustyNES is not directed at children and the hosted services do not knowingly
collect any information about anyone, of any age, beyond what is described
above.

## Your choices and rights

You can use RustyNES entirely offline. Online netplay and RetroAchievements
are off until you use them, and you can point netplay at a lobby and relay you
run yourself (the reference lobby is in the repository). Because the hosted
services keep no data that identifies you after the periods stated above,
there is normally nothing to access, correct or delete; if you believe
otherwise, or have any question about this policy, contact the maintainer as
below.

## Contact

Open an issue at <https://github.com/doublegate/RustyNES/issues>, or, for
anything you would rather not post publicly, use the contact address in
[`SECURITY.md`](https://github.com/doublegate/RustyNES/blob/main/SECURITY.md).

## Changes

Changes to this policy are made in the repository and take effect when
published at the address above. A change that would collect or keep more data
will be stated in the release notes of the version that makes it.

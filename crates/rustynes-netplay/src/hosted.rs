//! v3.4.0 (`T-HOSTED-NETPLAY`): the client side of the **hosted services**.
//!
//! The hosted services are the Cloudflare Worker in `services/hosted/`: it runs
//! the signalling lobby, mints short-lived TURN credentials, and proxies the
//! browser's `RetroAchievements` calls.
//!
//! # What is here
//!
//! - **The base URL.** A deployment is one origin, e.g.
//!   `https://rustynes-hosted.<account>.workers.dev`. [`resolve_base`] picks it:
//!   a configured value first, else the build-time constant
//!   [`BUILD_HOSTED_BASE`] (the `RUSTYNES_HOSTED_BASE` environment variable at
//!   compile time), else nothing. **Both are unset by default**, so a stock
//!   build behaves exactly as before: no hosted lobby, no TURN fetch, no
//!   network call it did not already make. From the base, [`signaling_url`],
//!   [`turn_credentials_url`] and [`ra_proxy_base`] derive the three endpoints.
//! - **The `/turn-credentials` answer.** [`parse_turn_credentials`] reads the
//!   Worker's JSON (`{"iceServers":[…],"ttl":…,"expiresAt":…}`) or its refusal
//!   (`{"error":…,"message":…}`) into a [`HostedIce`] or a
//!   [`HostedIceError::Refused`]. A refusal is not a failure of netplay: the
//!   clients carry on with STUN only (a direct, hole-punched connection), which
//!   is what the Worker's kill switch, its daily cap and its per-IP limit are
//!   designed to fall back to.
//! - **Expiry.** The credentials are valid for `ttl` seconds (3 h on the
//!   deployed Worker; `services/hosted/src/turn.ts` explains the value).
//!   [`HostedIce::is_expired`] says when a cached set must not be used for a
//!   new connection.
//! - **The native TURN endpoint.** The native relay client
//!   ([`crate::relay::TurnClient`]) speaks TURN over UDP only, so
//!   [`HostedIce::turn_udp`] picks the first `turn:` URL whose transport is UDP
//!   and splits it into host, port and credentials.
//! - **The native fetch** ([`fetch_ice`], native + `netplay-client` only): one
//!   blocking HTTPS `POST` (empty body) with a timeout and a response-size
//!   cap. `POST`, not `GET`: the Worker refuses a `GET`, because a browser
//!   sends one without `Origin` from any page's `<img>`, and a missing
//!   `Origin` is what a native client looks like (`services/hosted/src/
//!   turn.ts`, "Why POST only").
//!
//! # Why a hand-written JSON reader
//!
//! The crate's protocol code is dependency-free by design (the signalling
//! module's reasoning applies), and the response is nested (an array of
//! objects holding an array of strings), which the flat-object helpers in
//! [`crate::signaling`] cannot read. [`Json`] is a small, strict, bounded
//! recursive-descent parser: it refuses input over [`MAX_RESPONSE_BYTES`] or
//! nested deeper than [`MAX_DEPTH`], so a hostile or broken server cannot make
//! a client allocate without bound or recurse off its stack. Everything it
//! returns is validated again by [`parse_turn_credentials`] (scheme, lengths,
//! counts) before any of it reaches an `RTCPeerConnection` or a socket.

use std::fmt;

/// The hosted base URL baked in at build time, from the `RUSTYNES_HOSTED_BASE`
/// environment variable. `None` (the default) means a stock build knows no
/// hosted service.
pub const BUILD_HOSTED_BASE: Option<&str> = option_env!("RUSTYNES_HOSTED_BASE");

/// Largest `/turn-credentials` response accepted, in bytes. The real answer is
/// well under 1 KiB.
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024;
/// Deepest JSON nesting accepted. The real answer nests three levels.
pub const MAX_DEPTH: usize = 16;
/// At most this many ICE servers, and this many URLs per server, are kept.
pub const MAX_ICE_SERVERS: usize = 8;
/// See [`MAX_ICE_SERVERS`].
pub const MAX_URLS_PER_SERVER: usize = 8;
/// A cached set counts as expired this many seconds before `expiresAt`, so a
/// connection is never started on credentials about to lapse.
pub const EXPIRY_MARGIN_SECS: u64 = 300;

/// Normalize a hosted base URL; `None` when it is not a usable base.
///
/// A usable base is trimmed, has no trailing `/`, is `https://` (or `http://`
/// to `localhost` / `127.0.0.1` for local development), and has no query,
/// fragment, userinfo (`@`) or whitespace.
///
/// Userinfo is refused because it moves the host: in
/// `http://localhost:80@evil.example`, `localhost:80` is a user name and
/// password and the host is `evil.example` (RFC 3986 section 3.2), so the
/// localhost-only check on the text before the first `/` or `:` would pass a
/// plain-HTTP base to another machine. A hosted base never needs userinfo, so
/// any `@` refuses the whole value, which also keeps this check in step with
/// the browser glue's anchored pattern in `web/cheevos/ra_glue.js`.
#[must_use]
pub fn normalize_base(raw: &str) -> Option<String> {
    let s = raw.trim().trim_end_matches('/');
    if s.is_empty()
        || s.contains(char::is_whitespace)
        || s.contains('?')
        || s.contains('#')
        || s.contains('@')
    {
        return None;
    }
    let rest = match (s.strip_prefix("https://"), s.strip_prefix("http://")) {
        (Some(r), _) => r,
        // Plain HTTP only to this machine (`wrangler dev`).
        (None, Some(r)) => {
            let host = r.split(['/', ':']).next().unwrap_or("");
            if host != "localhost" && host != "127.0.0.1" {
                return None;
            }
            r
        }
        (None, None) => return None,
    };
    if rest.is_empty() || rest.starts_with('/') {
        return None;
    }
    Some(s.to_string())
}

/// The hosted base to use: `configured` when it is set and valid, otherwise
/// [`BUILD_HOSTED_BASE`], otherwise `None`.
#[must_use]
pub fn resolve_base(configured: &str) -> Option<String> {
    resolve_base_with(configured, BUILD_HOSTED_BASE)
}

/// [`resolve_base`] with the build-time value passed in (for tests).
#[must_use]
pub fn resolve_base_with(configured: &str, build: Option<&str>) -> Option<String> {
    normalize_base(configured).or_else(|| build.and_then(normalize_base))
}

/// The lobby's WebSocket URL: `https://h` becomes `wss://h/signal`.
#[must_use]
pub fn signaling_url(base: &str) -> String {
    let ws = base.strip_prefix("https://").map_or_else(
        || format!("ws://{}", base.trim_start_matches("http://")),
        |r| format!("wss://{r}"),
    );
    format!("{ws}/signal")
}

/// The TURN credential endpoint.
#[must_use]
pub fn turn_credentials_url(base: &str) -> String {
    format!("{base}/turn-credentials")
}

/// The base the browser's `RetroAchievements` glue posts to (`ra_glue.js`
/// appends `/dorequest.php`).
#[must_use]
pub fn ra_proxy_base(base: &str) -> String {
    format!("{base}/ra")
}

/// One ICE server: what `RTCIceServer` holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IceServer {
    /// `stun:` / `turn:` / `turns:` URLs.
    pub urls: Vec<String>,
    /// The TURN username (absent for STUN).
    pub username: Option<String>,
    /// The TURN credential (absent for STUN).
    pub credential: Option<String>,
}

impl IceServer {
    /// A credential-less server from one URL (the configured STUN list).
    #[must_use]
    pub fn from_url(url: &str) -> Self {
        Self {
            urls: vec![url.to_string()],
            username: None,
            credential: None,
        }
    }
}

/// A TURN-over-UDP endpoint for the native relay client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnEndpoint {
    /// Host name or IP literal (IPv6 without brackets).
    pub host: String,
    /// UDP port.
    pub port: u16,
    /// Long-term-credential username.
    pub username: String,
    /// Long-term-credential password.
    pub credential: String,
}

/// A parsed `/turn-credentials` answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedIce {
    /// The ICE servers, STUN and TURN.
    pub ice_servers: Vec<IceServer>,
    /// The lifetime the Worker requested, seconds.
    pub ttl: u32,
    /// When the credentials expire, Unix seconds.
    pub expires_at: u64,
}

impl HostedIce {
    /// `true` once `now_unix` is within [`EXPIRY_MARGIN_SECS`] of expiry.
    #[must_use]
    pub const fn is_expired(&self, now_unix: u64) -> bool {
        now_unix.saturating_add(EXPIRY_MARGIN_SECS) >= self.expires_at
    }

    /// The first TURN-over-UDP endpoint, for [`crate::relay::TurnClient`].
    #[must_use]
    pub fn turn_udp(&self) -> Option<TurnEndpoint> {
        self.ice_servers.iter().find_map(|s| {
            let (user, cred) = (s.username.as_ref()?, s.credential.as_ref()?);
            s.urls.iter().find_map(|u| {
                let (host, port) = parse_turn_udp_url(u)?;
                Some(TurnEndpoint {
                    host,
                    port,
                    username: user.clone(),
                    credential: cred.clone(),
                })
            })
        })
    }
}

/// `turn:host[:port][?transport=udp]` (RFC 7065; UDP is the default
/// transport) to `(host, port)`. `turns:` and `transport=tcp` are not UDP.
fn parse_turn_udp_url(url: &str) -> Option<(String, u16)> {
    let rest = url.strip_prefix("turn:")?;
    let (hostport, query) = rest.split_once('?').unwrap_or((rest, ""));
    if !query.is_empty() && query.split('&').all(|kv| kv != "transport=udp") {
        return None;
    }
    let (host, port) = if let Some(v6) = hostport.strip_prefix('[') {
        let (h, tail) = v6.split_once(']')?;
        let port = match tail.strip_prefix(':') {
            Some(p) => p.parse().ok()?,
            None if tail.is_empty() => 3478,
            None => return None,
        };
        (h.to_string(), port)
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().ok()?),
            None => (hostport.to_string(), 3478),
        }
    };
    if host.is_empty() || port == 0 {
        return None;
    }
    Some((host, port))
}

/// Why no [`HostedIce`] was obtained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostedIceError {
    /// The service answered and declined (`turn-disabled`, `rate-limited`,
    /// `daily-cap`, `turn-not-configured`, `turn-upstream`, …). `message` is
    /// the human-readable text the Worker supplied.
    Refused {
        /// The machine-readable code.
        code: String,
        /// The Worker's explanation.
        message: String,
    },
    /// The answer was not a valid credentials document.
    Malformed(&'static str),
    /// The request did not complete (DNS, TLS, timeout, …).
    Transport(String),
}

impl fmt::Display for HostedIceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused { code, message } => write!(f, "TURN unavailable ({code}): {message}"),
            Self::Malformed(why) => write!(f, "TURN credentials malformed: {why}"),
            Self::Transport(why) => write!(f, "TURN credential request failed: {why}"),
        }
    }
}

impl std::error::Error for HostedIceError {}

/// Parse a `/turn-credentials` body (whatever its HTTP status: refusals carry
/// a JSON body too).
///
/// # Errors
///
/// [`HostedIceError::Refused`] for the Worker's `{"error":…}` answer,
/// [`HostedIceError::Malformed`] for anything that is not a usable document.
pub fn parse_turn_credentials(body: &str) -> Result<HostedIce, HostedIceError> {
    let root = Json::parse(body).ok_or(HostedIceError::Malformed("not JSON"))?;
    let Json::Obj(fields) = &root else {
        return Err(HostedIceError::Malformed("not an object"));
    };
    if let Some(Json::Str(code)) = get(fields, "error") {
        let message = match get(fields, "message") {
            Some(Json::Str(m)) => m.chars().take(512).collect(),
            _ => String::new(),
        };
        return Err(HostedIceError::Refused {
            code: code.chars().take(64).collect(),
            message,
        });
    }
    let Some(Json::Arr(list)) = get(fields, "iceServers") else {
        return Err(HostedIceError::Malformed("no iceServers"));
    };
    let mut ice_servers = Vec::new();
    for s in list.iter().take(MAX_ICE_SERVERS) {
        let Json::Obj(sf) = s else { continue };
        let urls: Vec<String> = match get(sf, "urls") {
            Some(Json::Str(u)) => vec![u.clone()],
            Some(Json::Arr(us)) => us
                .iter()
                .filter_map(|u| match u {
                    Json::Str(u) => Some(u.clone()),
                    _ => None,
                })
                .collect(),
            _ => continue,
        };
        let urls: Vec<String> = urls
            .into_iter()
            .filter(|u| {
                u.len() <= 256
                    && (u.starts_with("stun:") || u.starts_with("turn:") || u.starts_with("turns:"))
            })
            .take(MAX_URLS_PER_SERVER)
            .collect();
        if urls.is_empty() {
            continue;
        }
        let text = |k: &str| match get(sf, k) {
            Some(Json::Str(v)) if v.len() <= 1024 => Some(v.clone()),
            _ => None,
        };
        let (username, credential) = match (text("username"), text("credential")) {
            (Some(u), Some(c)) => (Some(u), Some(c)),
            _ if urls.iter().any(|u| u.starts_with("turn")) => continue,
            _ => (None, None),
        };
        ice_servers.push(IceServer {
            urls,
            username,
            credential,
        });
    }
    if ice_servers.is_empty() {
        return Err(HostedIceError::Malformed("no usable ICE server"));
    }
    let int = |k: &str| match get(fields, k) {
        Some(Json::Num(n)) if *n >= 0.0 && n.fract() == 0.0 && *n < 1e15 => Some(*n),
        _ => None,
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ttl = int("ttl")
        .filter(|t| *t <= f64::from(u32::MAX))
        .ok_or(HostedIceError::Malformed("no ttl"))? as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let expires_at = int("expiresAt").ok_or(HostedIceError::Malformed("no expiresAt"))? as u64;
    Ok(HostedIce {
        ice_servers,
        ttl,
        expires_at,
    })
}

fn get<'a>(fields: &'a [(String, Json)], key: &str) -> Option<&'a Json> {
    fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// A JSON value, from the bounded parser.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    /// `null`
    Null,
    /// `true` / `false`
    Bool(bool),
    /// A number.
    Num(f64),
    /// A string.
    Str(String),
    /// An array.
    Arr(Vec<Self>),
    /// An object, in source order.
    Obj(Vec<(String, Self)>),
}

impl Json {
    /// Parse a complete JSON document; `None` if it is malformed, larger than
    /// [`MAX_RESPONSE_BYTES`] or nested deeper than [`MAX_DEPTH`].
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() > MAX_RESPONSE_BYTES {
            return None;
        }
        let mut p = Parser {
            s: text.as_bytes(),
            i: 0,
        };
        let v = p.value(0)?;
        p.ws();
        (p.i == p.s.len()).then_some(v)
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while matches!(self.s.get(self.i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, lit: &[u8]) -> Option<()> {
        self.s
            .get(self.i..self.i + lit.len())
            .filter(|w| *w == lit)?;
        self.i += lit.len();
        Some(())
    }

    fn value(&mut self, depth: usize) -> Option<Json> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.ws();
        match *self.s.get(self.i)? {
            b'n' => self.eat(b"null").map(|()| Json::Null),
            b't' => self.eat(b"true").map(|()| Json::Bool(true)),
            b'f' => self.eat(b"false").map(|()| Json::Bool(false)),
            b'"' => self.string().map(Json::Str),
            b'[' => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Some(Json::Arr(out));
                }
                loop {
                    out.push(self.value(depth + 1)?);
                    self.ws();
                    match *self.s.get(self.i)? {
                        b',' => self.i += 1,
                        b']' => {
                            self.i += 1;
                            return Some(Json::Arr(out));
                        }
                        _ => return None,
                    }
                }
            }
            b'{' => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Some(Json::Obj(out));
                }
                loop {
                    self.ws();
                    if self.s.get(self.i) != Some(&b'"') {
                        return None;
                    }
                    let k = self.string()?;
                    self.ws();
                    self.eat(b":")?;
                    let v = self.value(depth + 1)?;
                    out.push((k, v));
                    self.ws();
                    match *self.s.get(self.i)? {
                        b',' => self.i += 1,
                        b'}' => {
                            self.i += 1;
                            return Some(Json::Obj(out));
                        }
                        _ => return None,
                    }
                }
            }
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn number(&mut self) -> Option<Json> {
        let start = self.i;
        if self.s.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        while matches!(
            self.s.get(self.i),
            Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
        ) {
            self.i += 1;
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).ok()?;
        text.parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .map(Json::Num)
    }

    fn hex4(&mut self) -> Option<u32> {
        let h = std::str::from_utf8(self.s.get(self.i..self.i + 4)?).ok()?;
        self.i += 4;
        u32::from_str_radix(h, 16).ok()
    }

    fn string(&mut self) -> Option<String> {
        self.i += 1; // opening quote
        let mut out = String::new();
        loop {
            let b = *self.s.get(self.i)?;
            match b {
                b'"' => {
                    self.i += 1;
                    return Some(out);
                }
                b'\\' => {
                    self.i += 1;
                    let e = *self.s.get(self.i)?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let cp = if (0xD800..0xDC00).contains(&hi) {
                                self.eat(b"\\u")?;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return None;
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            out.push(char::from_u32(cp)?);
                        }
                        _ => return None,
                    }
                }
                0x00..=0x1F => return None,
                0x20..=0x7F => {
                    out.push(char::from(b));
                    self.i += 1;
                }
                _ => {
                    // Copy one whole UTF-8 sequence. The input is a &str, so
                    // it is valid UTF-8 and `b` is a leading byte; its high
                    // bits give the sequence length (RFC 3629 section 3). Only
                    // those bytes are decoded: validating the whole rest of
                    // the input for every non-ASCII character, as this did
                    // until v3.4.0's review, is quadratic in the body size.
                    let width = match b {
                        0xC0..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        _ => 4,
                    };
                    let seq = self.s.get(self.i..self.i + width)?;
                    let c = std::str::from_utf8(seq).ok()?.chars().next()?;
                    out.push(c);
                    self.i += width;
                }
            }
        }
    }
}

/// Fetch credentials from a hosted base (native, blocking, one request).
///
/// The request is a `POST` with an empty body: the Worker refuses `GET`.
///
/// # Errors
///
/// [`HostedIceError::Transport`] if the request does not complete within
/// `timeout`, otherwise whatever [`parse_turn_credentials`] reports.
#[cfg(all(not(target_arch = "wasm32"), feature = "netplay-client"))]
pub fn fetch_ice(base: &str, timeout: std::time::Duration) -> Result<HostedIce, HostedIceError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        // Refusals (429, 503) carry the JSON reason the caller reports.
        .http_status_as_error(false)
        .user_agent(concat!("RustyNES/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut resp = agent
        .post(&turn_credentials_url(base))
        .send_empty()
        .map_err(|e| HostedIceError::Transport(e.to_string()))?;
    let body = resp
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES as u64)
        .read_to_string()
        .map_err(|e| HostedIceError::Transport(e.to_string()))?;
    parse_turn_credentials(&body)
}

/// Resolve [`HostedIce::turn_udp`] into the native relay client's config.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn turn_config(ice: &HostedIce) -> Option<crate::relay::TurnConfig> {
    use std::net::ToSocketAddrs;
    let ep = ice.turn_udp()?;
    let server = (ep.host.as_str(), ep.port).to_socket_addrs().ok()?.next()?;
    Some(crate::relay::TurnConfig {
        server,
        username: ep.username,
        credential: ep.credential,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Worker's success answer, as `services/hosted/test/turn.test.ts`
    /// pins it (port-53 URLs already dropped).
    const OK: &str = r#"{"iceServers":[{"urls":["stun:stun.cloudflare.com:3478"]},{"urls":["turn:turn.cloudflare.com:3478?transport=udp","turn:turn.cloudflare.com:3478?transport=tcp","turns:turn.cloudflare.com:5349?transport=tcp"],"username":"u-short-lived","credential":"c-short-lived"}],"ttl":10800,"expiresAt":1792200000}"#;

    #[test]
    fn parses_the_worker_answer() {
        let ice = parse_turn_credentials(OK).expect("valid answer");
        assert_eq!(ice.ttl, 10800);
        assert_eq!(ice.expires_at, 1_792_200_000);
        assert_eq!(ice.ice_servers.len(), 2);
        assert_eq!(
            ice.ice_servers[0],
            IceServer::from_url("stun:stun.cloudflare.com:3478")
        );
        assert_eq!(ice.ice_servers[1].urls.len(), 3);
        assert_eq!(
            ice.ice_servers[1].username.as_deref(),
            Some("u-short-lived")
        );
        assert_eq!(
            ice.ice_servers[1].credential.as_deref(),
            Some("c-short-lived")
        );
    }

    #[test]
    fn refusals_carry_their_code_and_message() {
        for code in [
            "turn-disabled",
            "rate-limited",
            "daily-cap",
            "turn-not-configured",
            "turn-upstream",
        ] {
            let body = format!(r#"{{"error":"{code}","message":"connect directly (STUN only)."}}"#);
            assert_eq!(
                parse_turn_credentials(&body),
                Err(HostedIceError::Refused {
                    code: code.into(),
                    message: "connect directly (STUN only).".into()
                })
            );
        }
    }

    #[test]
    fn malformed_answers_are_rejected() {
        for body in [
            "",
            "not json",
            "[]",
            r#"{"iceServers":[]}"#,
            r#"{"iceServers":[{"urls":["http://x"]}],"ttl":1,"expiresAt":2}"#,
            // A TURN URL without credentials is unusable, so nothing is left.
            r#"{"iceServers":[{"urls":["turn:x:3478"]}],"ttl":1,"expiresAt":2}"#,
            r#"{"iceServers":[{"urls":["stun:x"]}],"expiresAt":2}"#,
            r#"{"iceServers":[{"urls":["stun:x"]}],"ttl":-1,"expiresAt":2}"#,
            r#"{"iceServers":[{"urls":["stun:x"]}],"ttl":1.5,"expiresAt":2}"#,
            r#"{"iceServers":[{"urls":["stun:x"]}],"ttl":1,"expiresAt":2} trailing"#,
        ] {
            assert!(
                matches!(
                    parse_turn_credentials(body),
                    Err(HostedIceError::Malformed(_))
                ),
                "accepted {body:?}"
            );
        }
    }

    #[test]
    fn the_parser_is_bounded() {
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 2), "]".repeat(MAX_DEPTH + 2));
        assert_eq!(Json::parse(&deep), None);
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(Json::parse(&ok).is_some());
        let big = format!(r#"{{"x":"{}"}}"#, "a".repeat(MAX_RESPONSE_BYTES));
        assert_eq!(Json::parse(&big), None);
    }

    #[test]
    fn the_parser_handles_escapes_and_unicode() {
        assert_eq!(
            Json::parse(r#""a\"b\\c\/d\né𝄞 é""#),
            Some(Json::Str("a\"b\\c/d\né\u{1d11e} é".into()))
        );
        assert_eq!(Json::parse(r#""\ud834""#), None, "a lone surrogate");
        assert_eq!(Json::parse("\"\u{1}\""), None, "a raw control character");
    }

    #[test]
    fn the_parser_is_linear_on_multibyte_strings() {
        // A maximal body of non-ASCII text: each two-byte character used to
        // re-validate the whole rest of the input (n^2 / 2 byte checks, about
        // a billion here): measured at 521 ms in a debug test build on this
        // project's 20-core host, std's validator being optimised code even
        // there. Decoding one sequence at a time makes it a single pass.
        let n = (MAX_RESPONSE_BYTES - 4) / 2;
        let body = format!("\"{}\"", "\u{e9}".repeat(n));
        assert!(body.len() <= MAX_RESPONSE_BYTES);
        let start = std::time::Instant::now();
        let parsed = Json::parse(&body);
        let took = start.elapsed();
        assert_eq!(parsed, Some(Json::Str("\u{e9}".repeat(n))));
        // The single pass measured 0.24 ms in the same build: 50 ms leaves
        // a 200x margin for a loaded machine and is still a tenth of what
        // the quadratic version took.
        assert!(took < std::time::Duration::from_millis(50), "took {took:?}");
        // Three- and four-byte sequences decode too, mixed with ASCII.
        assert_eq!(
            Json::parse("\"a\u{20ac}b\u{1d11e}c\""),
            Some(Json::Str("a\u{20ac}b\u{1d11e}c".into()))
        );
    }

    #[test]
    fn expiry_includes_the_safety_margin() {
        let ice = parse_turn_credentials(OK).expect("valid answer");
        // Literal times, not expressions of the constant, so a changed margin
        // fails here: five minutes before `expiresAt` already counts as expired.
        assert_eq!(EXPIRY_MARGIN_SECS, 300);
        assert!(!ice.is_expired(1_792_200_000 - 301));
        assert!(ice.is_expired(1_792_200_000 - 300));
        assert!(ice.is_expired(1_792_200_000));
        assert!(ice.is_expired(u64::MAX));
        // Freshly minted (3 h left) is usable; with 4 minutes left it is not.
        assert!(!ice.is_expired(1_792_200_000 - 10_800));
        assert!(ice.is_expired(1_792_200_000 - 240));
    }

    #[test]
    fn picks_the_udp_turn_endpoint() {
        let ice = parse_turn_credentials(OK).expect("valid answer");
        assert_eq!(
            ice.turn_udp(),
            Some(TurnEndpoint {
                host: "turn.cloudflare.com".into(),
                port: 3478,
                username: "u-short-lived".into(),
                credential: "c-short-lived".into(),
            })
        );
        assert_eq!(parse_turn_udp_url("turn:h"), Some(("h".into(), 3478)));
        assert_eq!(
            parse_turn_udp_url("turn:[2001:db8::1]:3479"),
            Some(("2001:db8::1".into(), 3479))
        );
        assert_eq!(parse_turn_udp_url("turn:h:3478?transport=tcp"), None);
        assert_eq!(parse_turn_udp_url("turns:h:5349"), None);
        assert_eq!(parse_turn_udp_url("turn:h:0"), None);
        assert_eq!(parse_turn_udp_url("turn::3478"), None);
    }

    #[test]
    fn base_urls_resolve_and_derive() {
        assert_eq!(resolve_base_with("", None), None, "unset by default");
        assert_eq!(
            resolve_base_with("  https://x.example.workers.dev/ ", None).as_deref(),
            Some("https://x.example.workers.dev")
        );
        assert_eq!(
            resolve_base_with("", Some("https://b.dev")).as_deref(),
            Some("https://b.dev")
        );
        assert_eq!(
            resolve_base_with("https://c.dev", Some("https://b.dev")).as_deref(),
            Some("https://c.dev")
        );
        assert_eq!(
            resolve_base_with("garbage", Some("https://b.dev")).as_deref(),
            Some("https://b.dev")
        );
        for bad in [
            "http://example.com",
            "ftp://x",
            "https://",
            "https:///x",
            "https://a b",
            "https://x?y",
            // Userinfo: the host is what follows the `@`, so these would reach
            // evil.example (the first over plain HTTP) while looking like
            // localhost / x.dev to the checks above.
            "http://localhost:80@evil.example",
            "http://localhost@evil.example",
            "http://127.0.0.1:8787@evil.example/x",
            "https://x.dev@evil.example",
            "https://user:pass@x.dev",
        ] {
            assert_eq!(normalize_base(bad), None, "{bad}");
        }
        assert_eq!(
            normalize_base("http://localhost:8787").as_deref(),
            Some("http://localhost:8787")
        );
        assert_eq!(signaling_url("https://h.dev"), "wss://h.dev/signal");
        assert_eq!(
            signaling_url("http://127.0.0.1:8787"),
            "ws://127.0.0.1:8787/signal"
        );
        assert_eq!(
            turn_credentials_url("https://h.dev"),
            "https://h.dev/turn-credentials"
        );
        assert_eq!(ra_proxy_base("https://h.dev"), "https://h.dev/ra");
    }

    /// The native client asks with `POST`: the Worker refuses a `GET`
    /// (`services/hosted/src/turn.ts`, "Why POST only"), so a GET here would
    /// make every native session fall back to STUN only. A one-shot HTTP
    /// server on loopback records the request line and answers with the
    /// Worker's success body.
    #[cfg(feature = "netplay-client")]
    #[test]
    fn fetch_ice_posts_to_the_credential_endpoint() {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut request_line = String::new();
            reader.read_line(&mut request_line).expect("request line");
            // Drain the headers (the body is empty).
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let mut stream = stream;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{OK}",
                OK.len()
            )
            .expect("respond");
            request_line
        });
        let ice = fetch_ice(
            &format!("http://127.0.0.1:{port}"),
            std::time::Duration::from_secs(5),
        )
        .expect("fetched");
        assert_eq!(ice.ttl, 10800);
        let request_line = server.join().expect("server thread");
        assert!(
            request_line.starts_with("POST /turn-credentials HTTP/1.1"),
            "{request_line:?}"
        );
    }

    #[test]
    fn a_stock_build_has_no_hosted_base() {
        // The CI build sets no RUSTYNES_HOSTED_BASE, so default behaviour is
        // unchanged: nothing hosted is contacted unless configured.
        if std::env::var_os("RUSTYNES_HOSTED_BASE").is_none() {
            assert_eq!(BUILD_HOSTED_BASE, None);
            assert_eq!(resolve_base(""), None);
        }
    }
}

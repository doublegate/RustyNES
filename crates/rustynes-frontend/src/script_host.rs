//! v1.7.0 "Forge" Workstream E1 — the host-mediated IPC bridge for the Lua
//! `comm.*` table (native-only, behind the frontend's `script-ipc` feature).
//!
//! # Security posture (ADR 0016)
//!
//! The defining contract: **the Lua sandbox never gets a raw socket.** The
//! script engine (`rustynes-script`) exposes a `comm.*` table whose entries only
//! *queue* marshalled [`CommCmd`] values; **this host component owns every
//! actual connection** (TCP / HTTP / WebSocket / memory-mapped-file), performs
//! the I/O **off the emulator lock** on a dedicated worker thread, and feeds the
//! results back as plain [`CommResult`] values via
//! [`rustynes_script::ScriptEngine::push_comm_result`]. Because the VM only ever
//! sees Lua strings / numbers / tables, the sandbox's no-`io` / no-`os` /
//! no-`package` / no-net guarantee is preserved even with IPC enabled.
//!
//! IPC is a NEW non-deterministic input/output source, so it is:
//! - behind the off-by-default `script-ipc` cargo feature (the shipped / native
//!   default / `no_std` / wasm builds are byte-identical without it);
//! - **disabled under a locked session** (netplay / TAS replay or record /
//!   RA-hardcore) — the `comm.*` verbs drop at the source via the SAME
//!   `set_writes_locked` gate as `emu.write` (see `rustynes-script`), so no
//!   `CommCmd` is ever queued and this host opens no connection; and
//! - never visible to the core synthesis — the [`crate::emu::EmuCore`] / `Nes`
//!   stack is untouched by anything in this module.
//!
//! Mirrors the `debugger::badge_cache` worker-thread + channel pattern: a job
//! sender, a result receiver, and a worker that exits when the host drops.

use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rustynes_script::{CommCmd, CommResult};

/// Per-connect timeout for the outbound `socketServerSend` TCP socket, so a dead
/// / unreachable / firewalled endpoint can never block the worker thread
/// indefinitely on `connect` (it would otherwise hang until the OS default
/// timeout, freezing every later IPC command behind it).
const TCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// Minimum spacing between reconnect attempts after a failed/dropped socket, so
/// a script spamming `socketServerSend` at an unreachable target can't make the
/// worker re-attempt (and re-pay the connect timeout) on every single command.
const TCP_RECONNECT_BACKOFF: Duration = Duration::from_secs(5);

/// v2.7.3 (frontend audit SEC-04) — the most a `comm.httpGet` / `httpPost`
/// response body may be. ureq 3's `read_to_string` already stops at 10 MB; the
/// limit is stated here so a ureq default change cannot lift it silently.
/// Because it equals that default, removing it changes no test result today:
/// a mutation of this line is EXPECTED to come back not caught.
#[cfg(feature = "script-ipc")]
const HTTP_BODY_LIMIT: u64 = 10 * 1024 * 1024;

/// v2.7.3 (SEC-04) — the environment variable naming hosts a script may reach
/// even though they resolve to a loopback, private or link-local address:
/// comma-separated, each `host` or `host:port` (`localhost:8080,127.0.0.1`).
/// The same pattern as `RUSTYNES_COMM_TCP`: the USER names inward endpoints, a
/// script never can.
#[cfg(feature = "script-ipc")]
const HTTP_ALLOW_ENV: &str = "RUSTYNES_COMM_HTTP_ALLOW";

/// Whether a script may reach `ip` without the user allowlisting its host.
///
/// Refused: loopback, unspecified, the RFC 1918 private ranges, link-local
/// (which includes the `169.254.169.254` cloud metadata endpoint), CGNAT shared
/// space (`100.64.0.0/10`), `0.0.0.0/8`, broadcast, multicast and documentation
/// ranges, and in IPv6 the loopback, unique-local (`fc00::/7`) and link-local
/// (`fe80::/10`) and documentation (`2001:db8::/32`) ranges. An IPv4-mapped or
/// IPv4-compatible IPv6 address is judged as its IPv4 form, so both
/// `::ffff:127.0.0.1` and `::127.0.0.1` are loopback.
#[cfg(feature = "script-ipc")]
fn is_public(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || a == 0
                || (a == 100 && (b & 0xC0) == 64))
        }
        IpAddr::V6(v6) => {
            // `to_ipv4`, not `to_ipv4_mapped`: it also unwraps the deprecated
            // IPv4-compatible form (`::127.0.0.1`), which some stacks still
            // route to the embedded IPv4 address. `::1` and `::` become
            // `0.0.0.1` / `0.0.0.0`, which the IPv4 arm refuses.
            if let Some(v4) = v6.to_ipv4() {
                return is_public(IpAddr::V4(v4));
            }
            let documentation = v6.segments()[..2] == [0x2001, 0x0db8];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || documentation)
        }
    }
}

/// Parse [`HTTP_ALLOW_ENV`]'s value: trimmed, lower-cased, empty entries dropped.
#[cfg(feature = "script-ipc")]
fn parse_allowlist(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .collect()
}

/// Whether `uri`'s host (or `host:port`) is on the user's allowlist.
#[cfg(feature = "script-ipc")]
fn host_allowlisted(uri: &ureq::http::Uri, allow: &[String]) -> bool {
    let Some(host) = uri.host() else {
        return false;
    };
    // `[::1]` in a URI; the allowlist may name it with or without brackets.
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    let port = uri.port_u16().or_else(|| match uri.scheme_str() {
        Some("https") => Some(443),
        Some("http") => Some(80),
        _ => None,
    });
    allow.iter().any(|entry| {
        let entry = entry.trim_start_matches('[').replace("]:", ":");
        let entry = entry.trim_end_matches(']');
        entry == host || port.is_some_and(|p| entry == format!("{host}:{p}"))
    })
}

/// The first resolved address a script may not reach, if any.
#[cfg(feature = "script-ipc")]
fn first_blocked(
    uri: &ureq::http::Uri,
    addrs: &[std::net::SocketAddr],
    allow: &[String],
) -> Option<std::net::SocketAddr> {
    if host_allowlisted(uri, allow) {
        return None;
    }
    addrs.iter().copied().find(|a| !is_public(a.ip()))
}

/// v2.7.3 (SEC-04) — ureq's default resolver, with every resolved address
/// checked before ureq connects to it.
///
/// Checking HERE, rather than resolving the URL up front and then handing it to
/// ureq, is what makes the check hold: a pre-check resolves the name once and
/// ureq resolves it again, so a name can answer a public address to the check
/// and `127.0.0.1` to the connection (DNS rebinding). The resolver's answer is
/// the one ureq connects to. Redirects are disabled on the agent for the same
/// reason: a public URL could otherwise redirect inward past this check.
///
/// `ureq::unversioned` does not follow semver. A breaking change there fails
/// to compile rather than silently weakening this, which is the direction we
/// want.
#[cfg(feature = "script-ipc")]
#[derive(Debug)]
struct GuardedResolver {
    inner: ureq::unversioned::resolver::DefaultResolver,
    allow: Vec<String>,
}

#[cfg(feature = "script-ipc")]
impl ureq::unversioned::resolver::Resolver for GuardedResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        let addrs = self.inner.resolve(uri, config, timeout)?;
        if let Some(blocked) = first_blocked(uri, &addrs, &self.allow) {
            return Err(ureq::Error::Other(
                format!(
                    "blocked: {} resolves to {}, a non-public address; list the host in {HTTP_ALLOW_ENV} to allow it",
                    uri.host().unwrap_or("?"),
                    blocked.ip()
                )
                .into(),
            ));
        }
        Ok(addrs)
    }
}

/// The script-IPC HTTP agent: a global timeout, no automatic redirects, the
/// [`GuardedResolver`], and statuses returned rather than raised.
#[cfg(feature = "script-ipc")]
fn http_agent(allow: Vec<String>) -> ureq::Agent {
    let config = http_config(ureq::Agent::config_builder());
    ureq::Agent::with_parts(
        config,
        ureq::unversioned::transport::DefaultConnector::new(),
        GuardedResolver {
            inner: ureq::unversioned::resolver::DefaultResolver::default(),
            allow,
        },
    )
}

/// The script-IPC agent settings, applied over `builder`. Split from
/// [`http_agent`] so a test can start from a builder that already carries a
/// proxy, as `Agent::config_builder()` does when `HTTP_PROXY` & co. are set.
#[cfg(feature = "script-ipc")]
fn http_config(
    builder: ureq::config::ConfigBuilder<ureq::typestate::AgentScope>,
) -> ureq::config::Config {
    builder
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        // Report the real status + body for non-2xx instead of an `Err` (ureq 3's
        // `StatusCode` error drops the body the script wants).
        .http_status_as_error(false)
        // v2.7.3 (SEC-04): a 3xx is returned to the script, never followed, so
        // the next hop goes through the resolver check like any other request.
        .max_redirects(0)
        // Review on #551: no proxy, including one from the environment. Through
        // a CONNECT proxy the resolver above checks the PROXY's address and the
        // proxy resolves the script's target, so the check would never see it.
        // A user behind a mandatory proxy loses script HTTP; that is the price.
        .proxy(None)
        .build()
}

/// The host side of the `comm.*` bridge: owns the worker thread + the result
/// inbox the host pumps back into the engine each frame.
pub struct ScriptHost {
    /// Outbound jobs to the worker (`None` is never sent; the worker exits when
    /// this sender drops).
    job_tx: Sender<CommCmd>,
    /// Results the worker produced (drained each frame and pushed to the engine).
    result_rx: Receiver<CommResult>,
    /// The worker thread handle (joined on drop).
    worker: Option<JoinHandle<()>>,
}

impl ScriptHost {
    /// Spawn the IPC worker thread.
    #[must_use]
    pub fn new() -> Self {
        let (job_tx, job_rx) = channel::<CommCmd>();
        let (result_tx, result_rx) = channel::<CommResult>();
        let worker = std::thread::Builder::new()
            .name("script-ipc".to_string())
            .spawn(move || worker_loop(&job_rx, &result_tx))
            .ok();
        Self {
            job_tx,
            result_rx,
            worker,
        }
    }

    /// Hand a marshalled, host-owned IPC request to the worker. Fire-and-forget;
    /// any reply arrives later via [`Self::drain_results`]. The caller (the
    /// frontend pump) only ever forwards `CommCmd`s the engine produced AFTER
    /// the `set_writes_locked` gate, so a locked session never reaches here.
    pub fn submit(&self, cmd: CommCmd) {
        // A send error means the worker died; drop the command (the next frame's
        // results drain simply yields nothing).
        let _ = self.job_tx.send(cmd);
    }

    /// Drain every result the worker produced since the last call (non-blocking).
    /// The host pushes each back into the engine via `push_comm_result`.
    #[must_use]
    pub fn drain_results(&self) -> Vec<CommResult> {
        let mut out = Vec::new();
        while let Ok(r) = self.result_rx.try_recv() {
            out.push(r);
        }
        out
    }
}

impl Default for ScriptHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ScriptHost {
    fn drop(&mut self) {
        // Dropping `job_tx` ends the worker's `recv` loop; join so the thread is
        // cleaned up deterministically.
        if let Some(h) = self.worker.take() {
            // The sender is a field, so it is still alive here; replace it with a
            // detached channel to drop the original and unblock the worker.
            let (dead_tx, _dead_rx) = channel::<CommCmd>();
            let _ = std::mem::replace(&mut self.job_tx, dead_tx);
            let _ = h.join();
        }
    }
}

/// The worker loop: own the connections, do the blocking I/O, ship results back.
/// Exits when the job sender is dropped (host `ScriptHost::drop`).
fn worker_loop(job_rx: &Receiver<CommCmd>, result_tx: &Sender<CommResult>) {
    // Host-owned connection state — the script can NEVER name any of these
    // handles; it only ever sees the marshalled `CommResult` values below.
    #[cfg(feature = "script-ipc")]
    let agent = http_agent(parse_allowlist(
        &std::env::var(HTTP_ALLOW_ENV).unwrap_or_default(),
    ));
    // A single outbound TCP socket (`socketServerSend`) — lazily connected to the
    // host's configured endpoint via the env override (off-by-default; an
    // unconfigured host simply drops the byte stream). Kept host-side.
    let mut tcp: Option<TcpStream> = None;
    // When the last connect was attempted, so a failed connect backs off rather
    // than re-paying `TCP_CONNECT_TIMEOUT` on every queued send (see
    // `try_connect_tcp`). `None` = no attempt yet.
    let mut last_connect_attempt: Option<Instant> = None;
    // The in-process memory-mapped-file bridge: a host-owned named byte buffer
    // map. A real OS shared-memory backing is a maintainer follow-up; this gives
    // a deterministic, dependency-free host-owned MMF surface today.
    let mmf: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::new(Mutex::new(HashMap::new()));

    while let Ok(cmd) = job_rx.recv() {
        match cmd {
            CommCmd::SocketSend(data) => {
                if tcp.is_none() {
                    tcp = try_connect_tcp(&mut last_connect_attempt);
                }
                if let Some(s) = tcp.as_mut()
                    && s.write_all(&data).is_err()
                {
                    tcp = None; // drop a dead socket; reconnect (backed-off) next send.
                }
            }
            #[cfg(feature = "script-ipc")]
            CommCmd::HttpGet { id, url } => {
                let (status, body) = http_call(agent.get(&url).call());
                let _ = result_tx.send(CommResult::Http { id, status, body });
            }
            #[cfg(feature = "script-ipc")]
            CommCmd::HttpPost { id, url, body } => {
                // Pass the owned `body` by value (ureq reuses the allocation) and keep
                // ureq 2 `send_string`'s implicit `text/plain; charset=utf-8` content type.
                let (status, resp) = http_call(
                    agent
                        .post(&url)
                        .content_type("text/plain; charset=utf-8")
                        .send(body),
                );
                let _ = result_tx.send(CommResult::Http {
                    id,
                    status,
                    body: resp,
                });
            }
            CommCmd::WsOpen { id, .. } => {
                // A full WebSocket client is a maintainer follow-up (it needs a
                // ws crate); the host-owned contract is in place. Report a clean
                // closed/error state so a portable script does not hang.
                let _ = result_tx.send(CommResult::WsState {
                    id,
                    open: false,
                    message: None,
                });
            }
            CommCmd::WsSend(_) | CommCmd::WsClose => {
                // No open WS connection (see WsOpen) — drop.
            }
            CommCmd::MmfWrite { name, data } => {
                if let Ok(mut m) = mmf.lock() {
                    m.insert(name, data);
                }
            }
            CommCmd::MmfRead { id, name, len } => {
                let data = mmf
                    .lock()
                    .ok()
                    .and_then(|m| m.get(&name).cloned())
                    .map_or_else(Vec::new, |mut v| {
                        v.truncate(len as usize);
                        v
                    });
                let _ = result_tx.send(CommResult::Mmf { id, data });
            }
            // The remaining variants only exist under `script-ipc` (the only
            // config that compiles this module), so this arm is unreachable; the
            // catch-all keeps the match total if a variant is later cfg-gated.
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }
}

/// Attempt to (re)connect the outbound TCP socket to the `RUSTYNES_COMM_TCP`
/// endpoint, using a bounded [`TcpStream::connect_timeout`] so an unreachable
/// target never hangs the worker, and throttling retries to
/// [`TCP_RECONNECT_BACKOFF`] so a script spamming sends at a dead endpoint
/// re-attempts at most once per backoff window. Returns `None` (without
/// attempting) when unconfigured, inside the backoff window, or on any failure.
fn try_connect_tcp(last_attempt: &mut Option<Instant>) -> Option<TcpStream> {
    // Honour the reconnect backoff first (cheap; no env / DNS work in the
    // window).
    if let Some(t) = last_attempt
        && t.elapsed() < TCP_RECONNECT_BACKOFF
    {
        return None;
    }
    let addr = std::env::var("RUSTYNES_COMM_TCP").ok()?;
    *last_attempt = Some(Instant::now());
    // Resolve to a concrete `SocketAddr` (connect_timeout needs one). Try each
    // resolved address with the bounded timeout; the first success wins.
    let resolved = addr.to_socket_addrs().ok()?;
    for sa in resolved {
        if let Ok(s) = TcpStream::connect_timeout(&sa, TCP_CONNECT_TIMEOUT) {
            return Some(s);
        }
    }
    None
}

/// Marshal a `ureq` HTTP result into `(status, body)` plain values. Any
/// transport error becomes `status = 0` with an empty body so the script gets a
/// deterministic, non-panicking signal.
#[cfg(feature = "script-ipc")]
fn http_call(result: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> (u16, String) {
    result.map_or_else(
        // Only a transport error lands here -> status 0 + an empty body.
        |_| (0, String::new()),
        // With `http_status_as_error(false)`, non-2xx responses arrive as `Ok` too,
        // so the script gets the real status code + body.
        // A body that cannot be read -- over `HTTP_BODY_LIMIT`, or cut off --
        // is a transport failure too. Returning the real 2xx with an empty
        // body made it indistinguishable from an empty response (agy round 3
        // on #551).
        |mut resp| {
            let status = resp.status().as_u16();
            resp.body_mut()
                .with_config()
                .limit(HTTP_BODY_LIMIT)
                .lossy_utf8(true)
                .read_to_string()
                .map_or_else(|_| (0, String::new()), |body| (status, body))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The host-owned memory-mapped-file bridge round-trips through the worker:
    /// a `MmfWrite` then a `MmfRead` returns the stored bytes (truncated to the
    /// requested length) as a `CommResult` — no socket, no OS surface.
    #[test]
    fn mmf_write_then_read_round_trips_via_the_host() {
        let host = ScriptHost::new();
        host.submit(CommCmd::MmfWrite {
            name: "frame".to_string(),
            data: vec![1, 2, 3, 4, 5],
        });
        host.submit(CommCmd::MmfRead {
            id: 7,
            name: "frame".to_string(),
            len: 3,
        });
        // Poll briefly for the worker to produce the result.
        let mut got = None;
        for _ in 0..200 {
            let results = host.drain_results();
            if let Some(r) = results.into_iter().next() {
                got = Some(r);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(
            got,
            Some(CommResult::Mmf {
                id: 7,
                data: vec![1, 2, 3],
            }),
            "the host MMF bridge must round-trip the truncated bytes"
        );
    }

    /// A read of an unknown MMF name yields an empty buffer (never a panic / OS
    /// error leaking to the script).
    #[test]
    fn mmf_read_unknown_name_is_empty() {
        let host = ScriptHost::new();
        host.submit(CommCmd::MmfRead {
            id: 1,
            name: "nope".to_string(),
            len: 16,
        });
        let mut got = None;
        for _ in 0..200 {
            if let Some(r) = host.drain_results().into_iter().next() {
                got = Some(r);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(
            got,
            Some(CommResult::Mmf {
                id: 1,
                data: vec![]
            })
        );
    }

    /// A one-shot local HTTP server. Answers every connection with `response`
    /// and counts connections, so a test can assert one was never made.
    #[cfg(feature = "script-ipc")]
    fn local_server(response: String) -> (u16, Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::Read;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                counter.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (port, hits)
    }

    /// Review on #551: `Agent::config_builder()` picks up `HTTP_PROXY` /
    /// `HTTPS_PROXY` / `ALL_PROXY`. Through a CONNECT proxy the resolver sees
    /// the PROXY's address, and the proxy resolves the script's target itself,
    /// so the SEC-04 check never saw the target. The agent must not use one.
    #[cfg(feature = "script-ipc")]
    #[test]
    fn an_environment_proxy_is_not_used() {
        let proxied = ureq::Agent::config_builder()
            .proxy(Some(ureq::Proxy::new("http://proxy.example:3128").unwrap()));
        assert!(proxied.build().proxy().is_some(), "the builder carries it");
        let proxied = ureq::Agent::config_builder()
            .proxy(Some(ureq::Proxy::new("http://proxy.example:3128").unwrap()));
        assert!(http_config(proxied).proxy().is_none());
        assert_eq!(
            http_config(ureq::Agent::config_builder()).max_redirects(),
            0
        );
    }

    #[cfg(feature = "script-ipc")]
    #[test]
    fn inward_addresses_are_not_public() {
        use std::net::IpAddr;
        for blocked in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "0.1.2.3",
            "255.255.255.255",
            "::1",
            "::",
            "fc00::1",
            "fd12::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            // Review on #551: the IPv6 documentation range, and the deprecated
            // IPv4-compatible form, which `to_ipv4_mapped` does not unwrap.
            "2001:db8::1",
            "2001:db8:ffff::1",
            "::127.0.0.1",
            "::10.0.0.1",
            "::169.254.169.254",
        ] {
            let ip: IpAddr = blocked.parse().unwrap();
            assert!(!is_public(ip), "{blocked} must be refused");
        }
        for allowed in [
            "93.184.216.34",
            "1.1.1.1",
            "100.128.0.1",
            "2606:4700::1111",
            "2001:db9::1",
            "::1.1.1.1",
        ] {
            let ip: IpAddr = allowed.parse().unwrap();
            assert!(is_public(ip), "{allowed} is public");
        }
    }

    #[cfg(feature = "script-ipc")]
    #[test]
    fn the_allowlist_matches_host_or_host_and_port() {
        let uri = |u: &str| u.parse::<ureq::http::Uri>().unwrap();
        let allow = parse_allowlist(" LocalHost:8080 , 127.0.0.1,[::1],");
        assert_eq!(allow, vec!["localhost:8080", "127.0.0.1", "[::1]"]);
        assert!(host_allowlisted(&uri("http://localhost:8080/x"), &allow));
        assert!(
            !host_allowlisted(&uri("http://localhost:9090/x"), &allow),
            "wrong port"
        );
        assert!(
            host_allowlisted(&uri("http://127.0.0.1:5000/"), &allow),
            "any port"
        );
        assert!(host_allowlisted(&uri("http://[::1]:7000/"), &allow));
        assert!(!host_allowlisted(&uri("http://192.168.1.1/"), &allow));
    }

    /// SEC-04: a script reached any address, including loopback services. With
    /// no allowlist the request must fail at resolution, before a connection.
    #[cfg(feature = "script-ipc")]
    #[test]
    fn a_loopback_request_is_refused_before_connecting() {
        let (port, hits) = local_server("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".into());
        let agent = http_agent(Vec::new());
        let err = agent
            .get(&format!("http://127.0.0.1:{port}/"))
            .call()
            .expect_err("loopback is blocked");
        assert!(err.to_string().contains("blocked"), "{err}");
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "no connection"
        );
    }

    #[cfg(feature = "script-ipc")]
    #[test]
    fn an_allowlisted_host_is_reached() {
        let (port, hits) = local_server("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".into());
        let agent = http_agent(parse_allowlist("127.0.0.1"));
        let (status, body) = http_call(agent.get(&format!("http://127.0.0.1:{port}/")).call());
        assert_eq!((status, body.as_str()), (200, "ok"));
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// agy round 3 on #551: a body over the limit came back as the real 2xx
    /// status with an empty body, indistinguishable from an empty response.
    /// A body that cannot be read is a transport failure: `status = 0`.
    #[cfg(feature = "script-ipc")]
    #[test]
    fn a_body_over_the_limit_is_a_transport_failure() {
        #[allow(clippy::cast_possible_truncation)] // 10 MiB + 1 fits a usize.
        let len = HTTP_BODY_LIMIT as usize + 1;
        let (port, _) = local_server(format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {len}\r\n\r\n{}",
            "x".repeat(len)
        ));
        let agent = http_agent(parse_allowlist("127.0.0.1"));
        let (status, body) = http_call(agent.get(&format!("http://127.0.0.1:{port}/")).call());
        assert_eq!((status, body.len()), (0, 0));
    }

    /// A redirect is returned to the script, never followed: otherwise a public
    /// URL could hop inward past the resolver check.
    #[cfg(feature = "script-ipc")]
    #[test]
    fn a_redirect_is_returned_not_followed() {
        let (inner, inner_hits) =
            local_server("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nno".into());
        let (outer, _) = local_server(format!(
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{inner}/\r\nContent-Length: 0\r\n\r\n"
        ));
        let agent = http_agent(parse_allowlist("127.0.0.1"));
        let (status, _) = http_call(agent.get(&format!("http://127.0.0.1:{outer}/")).call());
        assert_eq!(status, 302);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(inner_hits.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}

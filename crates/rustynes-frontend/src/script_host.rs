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
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{
    Receiver, Sender, SyncSender, TryRecvError, TrySendError, channel, sync_channel,
};
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
    let port = uri.port_u16().or_else(|| match uri.scheme_str() {
        Some("https") => Some(443),
        Some("http") => Some(80),
        _ => None,
    });
    host_port_allowlisted(host, port, allow)
}

/// Whether `host` (or `host:port`) is on the user's allowlist. Split from
/// [`host_allowlisted`] in v3.5.0 so the WebSocket client, whose URI type is
/// `tungstenite`'s rather than `ureq`'s, applies the SAME rule.
#[cfg(feature = "script-ipc")]
fn host_port_allowlisted(host: &str, port: Option<u16>, allow: &[String]) -> bool {
    // `[::1]` in a URI; the allowlist may name it with or without brackets.
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
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
    /// Outbound jobs to the worker, BOUNDED at [`COMM_QUEUE_DEPTH`] (v2.9.0
    /// re-audit NF-08). The worker exits when this sender drops.
    job_tx: SyncSender<CommCmd>,
    /// Results the worker produced (drained each frame and pushed to the engine).
    result_rx: Receiver<CommResult>,
    /// Set by `Drop`: the worker abandons its backlog at the next job boundary.
    cancel: Arc<AtomicBool>,
    /// The worker thread handle (joined, or detached after a grace, on drop).
    worker: Option<JoinHandle<()>>,
    /// v3.5.0 (CR-01) — bytes of inbound WebSocket text sitting in
    /// `result_rx`, not yet drained. The socket thread stops reading while it
    /// is over [`WS_INBOUND_BUDGET`], so a server that floods the socket
    /// pushes back on its own TCP window instead of growing this process.
    ws_pending: Arc<AtomicUsize>,
}

/// v2.9.0 re-audit NF-08 — how many `comm.*` requests may wait for the worker.
///
/// The queue was an unbounded `mpsc::channel`, and each HTTP request can take
/// up to the agent's 20 s timeout, so a script could queue a backlog that
/// outlived it by hours. 256 is far more than a script polling a service
/// needs in flight, and bounds the backlog's memory to 256 requests of at most
/// 1 MiB each (the engine's per-request cap, NF-03). A full queue REFUSES the
/// request rather than blocking: `submit` runs on the frame thread, which must
/// never wait on the network.
pub const COMM_QUEUE_DEPTH: usize = 256;

/// v2.9.0 re-audit NF-08 — how long `Drop` waits for the worker before
/// detaching it. An idle worker, or one between jobs, sees the closed channel
/// or the cancel flag at once; only a worker inside a network call outlives
/// it, and that call is bounded by its own timeout (the MOB-04 precedent,
/// `rustynes-netplay`'s `join_or_detach`).
const DROP_GRACE: Duration = Duration::from_millis(100);

impl ScriptHost {
    /// Spawn the IPC worker thread.
    #[must_use]
    pub fn new() -> Self {
        Self::spawn(None, MMF_PREFIX.to_string())
    }

    /// Spawn with an explicit HTTP allowlist instead of reading
    /// `RUSTYNES_COMM_HTTP_ALLOW`, so a test can reach a loopback listener
    /// without mutating the process environment (which other tests share).
    /// The MMF prefix is unique to the test as well: shared-memory names are
    /// global to the machine, and parallel tests (or two `cargo test` runs)
    /// writing "frame" under the production prefix would share one region.
    #[cfg(all(test, feature = "script-ipc"))]
    fn with_allowlist(allow: Vec<String>) -> Self {
        Self::spawn(Some(allow), test_prefix())
    }

    /// Spawn with no allowlist and a test-unique MMF prefix (see
    /// [`Self::with_allowlist`]).
    #[cfg(test)]
    fn for_test() -> Self {
        Self::spawn(Some(Vec::new()), test_prefix())
    }

    fn spawn(allow: Option<Vec<String>>, mmf_prefix: String) -> Self {
        let (job_tx, job_rx) = sync_channel::<CommCmd>(COMM_QUEUE_DEPTH);
        let (result_tx, result_rx) = channel::<CommResult>();
        let cancel = Arc::new(AtomicBool::new(false));
        let ws_pending = Arc::new(AtomicUsize::new(0));
        let worker_cancel = Arc::clone(&cancel);
        let worker_pending = Arc::clone(&ws_pending);
        let worker = std::thread::Builder::new()
            .name("script-ipc".to_string())
            .spawn(move || {
                worker_loop(
                    &job_rx,
                    &result_tx,
                    &worker_cancel,
                    allow,
                    mmf_prefix,
                    &worker_pending,
                );
            })
            .ok();
        Self {
            job_tx,
            result_rx,
            cancel,
            worker,
            ws_pending,
        }
    }

    /// Hand a marshalled, host-owned IPC request to the worker. Any reply
    /// arrives later via [`Self::drain_results`]. The caller (the frontend
    /// pump) only ever forwards `CommCmd`s the engine produced AFTER the
    /// `set_writes_locked` gate, so a locked session never reaches here.
    ///
    /// # Errors
    ///
    /// Never blocks. When the queue already holds [`COMM_QUEUE_DEPTH`]
    /// requests, or the worker has died, the request is handed back; the
    /// caller tells the script with [`Self::refusal`].
    pub fn submit(&self, cmd: CommCmd) -> Result<(), CommCmd> {
        self.job_tx.try_send(cmd).map_err(|e| match e {
            TrySendError::Full(c) | TrySendError::Disconnected(c) => c,
        })
    }

    /// Submit a frame's `comm.*` requests (v2.9.0 re-audit NF-08). A request
    /// the full queue refuses is answered at once with [`Self::refusal`],
    /// pushed straight into `engine`, so the script sees a failed request
    /// rather than one that never completes. Returns a console note when any
    /// were refused (one line per frame, not one per request).
    #[must_use]
    pub fn forward(
        &self,
        cmds: Vec<CommCmd>,
        engine: &rustynes_script::ScriptEngine,
    ) -> Option<String> {
        let mut refused = 0usize;
        for cmd in cmds {
            if let Err(cmd) = self.submit(cmd) {
                refused += 1;
                if let Some(r) = Self::refusal(&cmd) {
                    engine.push_comm_result(r);
                }
            }
        }
        (refused > 0).then(|| {
            format!("[comm: {refused} request(s) refused: {COMM_QUEUE_DEPTH} already queued]")
        })
    }

    /// The failure a script sees for a request [`Self::submit`] refused: the
    /// same values a transport failure produces (`status = 0` / an empty
    /// buffer / a closed socket), so a script handles both one way. `None`
    /// for the fire-and-forget verbs, which have no reply to carry it.
    #[must_use]
    pub const fn refusal(cmd: &CommCmd) -> Option<CommResult> {
        match *cmd {
            CommCmd::HttpGet { id, .. } | CommCmd::HttpPost { id, .. } => Some(CommResult::Http {
                id,
                status: 0,
                body: String::new(),
            }),
            CommCmd::MmfRead { id, .. } => Some(CommResult::Mmf {
                id,
                data: Vec::new(),
            }),
            CommCmd::WsOpen { id, .. } => Some(CommResult::WsState {
                id,
                open: false,
                message: None,
            }),
            _ => None,
        }
    }

    /// Drain every result the worker produced since the last call (non-blocking).
    /// The host pushes each back into the engine via `push_comm_result`.
    #[must_use]
    pub fn drain_results(&self) -> Vec<CommResult> {
        let mut out = Vec::new();
        while let Ok(r) = self.result_rx.try_recv() {
            // CR-01: hand the drained message's bytes back to the socket
            // thread's inbound budget.
            if let CommResult::WsState {
                message: Some(m), ..
            } = &r
            {
                self.ws_pending.fetch_sub(m.len(), Ordering::AcqRel);
            }
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
    /// v2.9.0 re-audit NF-08. Before, this dropped the sender and joined. But
    /// `mpsc` delivers every buffered job before it reports disconnection, so
    /// the worker ran the whole backlog first: a stopped script's requests
    /// kept going out, and the join blocked the dropping thread (the UI
    /// thread, on Reload) for up to 20 s per queued request. Now the cancel
    /// flag makes the worker abandon the backlog at the next job boundary,
    /// and the join is bounded: a worker still inside a network call after
    /// `DROP_GRACE` is detached. That is safe because it owns everything it
    /// touches (its agent, socket and MMF map, its end of both channels, and
    /// its clone of the flag) and calls back into nothing; it exits when the
    /// call returns, bounded by the agent's timeout.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(h) = self.worker.take() {
            // The sender is a field, so it is still alive here; replace it with a
            // detached channel to drop the original and unblock an idle worker.
            let (dead_tx, _dead_rx) = sync_channel::<CommCmd>(0);
            let _ = std::mem::replace(&mut self.job_tx, dead_tx);
            join_or_detach(h, DROP_GRACE);
        }
    }
}

/// Join `worker` if it finishes within `grace`; otherwise let it go (dropping
/// the handle detaches the thread). Returns whether it joined.
fn join_or_detach(worker: JoinHandle<()>, grace: Duration) -> bool {
    let deadline = Instant::now() + grace;
    while !worker.is_finished() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let _ = worker.join();
    true
}

/// The worker loop: own the connections, do the blocking I/O, ship results back.
/// Exits when the job sender is dropped (host `ScriptHost::drop`).
fn worker_loop(
    job_rx: &Receiver<CommCmd>,
    result_tx: &Sender<CommResult>,
    cancel: &Arc<AtomicBool>,
    allow: Option<Vec<String>>,
    mmf_prefix: String,
    ws_pending: &Arc<AtomicUsize>,
) {
    // Host-owned connection state — the script can NEVER name any of these
    // handles; it only ever sees the marshalled `CommResult` values below.
    // `allow` is `Some` only from the test constructors; production reads the
    // user's environment.
    let allow = allow
        .unwrap_or_else(|| parse_allowlist(&std::env::var(HTTP_ALLOW_ENV).unwrap_or_default()));
    let agent = http_agent(allow.clone());
    // A single outbound TCP socket (`socketServerSend`) — lazily connected to the
    // host's configured endpoint via the env override (off-by-default; an
    // unconfigured host simply drops the byte stream). Kept host-side.
    let mut tcp: Option<TcpStream> = None;
    // When the last connect was attempted, so a failed connect backs off rather
    // than re-paying `TCP_CONNECT_TIMEOUT` on every queued send (see
    // `try_connect_tcp`). `None` = no attempt yet.
    let mut last_connect_attempt: Option<Instant> = None;
    // v3.5.0 (CR-02): the memory-mapped files are real OS shared memory,
    // named `<prefix><script name>`, so another process can open them.
    let mut mmf = MmfStore::new(mmf_prefix);
    // Whether a refused MMF write has been reported (once per worker, so a
    // script retrying every frame does not flood stderr; agy on #561).
    let mut mmf_refusal_reported = false;
    // v3.5.0 (CR-01): the one open WebSocket (`ws_send` names no socket, so
    // there is at most one), run on its own thread.
    let mut ws: Option<WsLink> = None;

    while let Ok(cmd) = job_rx.recv() {
        // NF-08: once the host is dropped, the backlog is abandoned, not run.
        if cancel.load(Ordering::Relaxed) {
            break;
        }
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
            CommCmd::HttpGet { id, url } => {
                let (status, body) = http_call(agent.get(&url).call());
                let _ = result_tx.send(CommResult::Http { id, status, body });
            }
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
            CommCmd::WsOpen { id, url } => {
                // A second open replaces the first: the old socket is told to
                // close (and reports `open = false` under ITS id) while the new
                // one connects on a thread of its own, so the worker never
                // waits on either handshake.
                if let Some(old) = ws.take() {
                    old.close();
                }
                ws = WsLink::spawn(
                    id,
                    url,
                    allow.clone(),
                    result_tx.clone(),
                    Arc::clone(cancel),
                    Arc::clone(ws_pending),
                );
                if ws.is_none() {
                    // The OS would not start a thread: report the failure the
                    // way every other transport failure is reported.
                    let _ = result_tx.send(CommResult::WsState {
                        id,
                        open: false,
                        message: None,
                    });
                }
            }
            CommCmd::WsSend(text) => {
                // No open socket: dropped, as before (fire-and-forget).
                if let Some(link) = ws.as_ref() {
                    link.send(text);
                }
            }
            CommCmd::WsClose => {
                if let Some(link) = ws.take() {
                    link.close();
                }
            }
            CommCmd::MmfWrite { name, data } => {
                // NF-03: a write that would take the store past its budget is
                // dropped, like any other fire-and-forget request, and
                // reported once so a script author can see why.
                if let Err(why) = mmf.write(&name, &data)
                    && !mmf_refusal_reported
                {
                    mmf_refusal_reported = true;
                    eprintln!("rustynes: script memory-mapped file write refused: {why}");
                }
            }
            CommCmd::MmfRead { id, name, len } => {
                let data = mmf.read(&name, len as usize);
                let _ = result_tx.send(CommResult::Mmf { id, data });
            }
        }
    }
    // Leaving: the WebSocket thread sees its command channel close and shuts
    // the socket; the MMF store unlinks the regions it created (on drop).
    if let Some(link) = ws.take() {
        link.close();
    }
}

// ---------------------------------------------------------------------------
// v3.5.0 (CR-02) — memory-mapped files backed by real OS shared memory
// ---------------------------------------------------------------------------

/// The prefix every script MMF name gets before it reaches the OS.
///
/// A script can therefore only ever open regions in the emulator's own corner
/// of the shared-memory namespace, never another program's (Python's, a
/// browser's, a sound server's).
///
/// A tool that wants the region a script named `frame` opens
/// `rnmf-frame` (on POSIX the OS object is `/rnmf-frame.<uid hex>`, on
/// Windows `Local\rnmf-frame`; see `crate::shm`).
pub const MMF_PREFIX: &str = "rnmf-";

/// The longest script MMF name: the portable shared-memory name limit less the
/// prefix (16 characters).
pub const MAX_MMF_NAME_LEN: usize = crate::shm::MAX_NAME_LEN - MMF_PREFIX.len();

/// Size of the header in front of every MMF payload, in bytes.
pub const MMF_HEADER_LEN: usize = 32;

/// The header's first four bytes.
pub const MMF_MAGIC: [u8; 4] = *b"RNMF";

/// The header layout version this host writes and accepts.
pub const MMF_LAYOUT_VERSION: u32 = 1;

// Header field offsets (all little-endian `u32`, see `docs/scripting.md`).
const MMF_OFF_VERSION: usize = 4;
const MMF_OFF_CAPACITY: usize = 8;
const MMF_OFF_LENGTH: usize = 12;
const MMF_OFF_SEQ: usize = 16;

/// How many times a reader retries a copy that raced a writer before giving up
/// (and returning an empty buffer, the transport-failure value).
const MMF_READ_ATTEMPTS: u32 = 64;

/// v2.9.0 re-audit NF-03 — the most bytes (names plus mapped regions) the
/// script's memory-mapped files may hold. The regions live as long as the
/// worker, so unlike a per-frame queue they never empty by themselves: before
/// this cap a script writing distinct names grew the store without bound. Each
/// write is also capped by the engine at `MAX_COMM_PAYLOAD_BYTES` (1 MiB).
/// Since v3.5.0 the bytes counted are the mapped size (header and page
/// rounding included), which is what the OS actually holds.
const MAX_MMF_BYTES: usize = 16 * 1024 * 1024;

/// v2.9.0 re-audit NF-03 — the most names the script may hold.
const MAX_MMF_NAMES: usize = 256;

/// The OS page granularity the regions are rounded to (4 KiB on every shipped
/// target; Windows allocates mapping views at 64 KiB granularity, but
/// commits only the pages the size covers).
const MMF_PAGE: usize = 4096;

/// The mapped size for a payload of `len` bytes: header plus payload, rounded
/// up to whole pages.
const fn mmf_region_len(len: usize) -> usize {
    (MMF_HEADER_LEN + len).div_ceil(MMF_PAGE) * MMF_PAGE
}

/// The script's memory-mapped files: one shared-memory region per name this
/// host has written. Dropping the store unmaps them, and on POSIX unlinks the
/// ones it created.
struct MmfStore {
    prefix: String,
    regions: HashMap<String, crate::shm::SharedRegion>,
}

impl MmfStore {
    fn new(prefix: String) -> Self {
        Self {
            prefix,
            regions: HashMap::new(),
        }
    }

    /// The OS name for a script name, or `None` if the script name is not
    /// one the portable rule (and [`MAX_MMF_NAME_LEN`]) accepts.
    fn os_name(&self, name: &str) -> Option<String> {
        let full = format!("{}{name}", self.prefix);
        (!name.is_empty() && crate::shm::validate_name(&full).is_ok()).then_some(full)
    }

    /// Bytes (names plus mapped regions) the store holds.
    fn held_bytes(&self) -> usize {
        self.regions.iter().map(|(k, r)| k.len() + r.len()).sum()
    }

    /// Write `data` under `name`, creating (or growing) its region. Refused,
    /// with the reason, when the name is invalid, the store would pass
    /// [`MAX_MMF_BYTES`] or [`MAX_MMF_NAMES`], or the OS refuses.
    fn write(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
        let Some(full) = self.os_name(name) else {
            return Err(format!(
                "the name {name:?} must be 1-{MAX_MMF_NAME_LEN} characters of A-Z a-z 0-9 _ . -"
            ));
        };
        let fits = self
            .regions
            .get(name)
            .is_some_and(|r| r.len() - MMF_HEADER_LEN >= data.len());
        if !fits {
            let want = mmf_region_len(data.len());
            let old = self.regions.get(name).map(|r| name.len() + r.len());
            let next = self.held_bytes() - old.unwrap_or(0) + name.len() + want;
            if next > MAX_MMF_BYTES || (old.is_none() && self.regions.len() >= MAX_MMF_NAMES) {
                return Err(format!(
                    "the store holds at most {MAX_MMF_NAMES} names and {} MiB",
                    MAX_MMF_BYTES >> 20
                ));
            }
            // Too small (or absent): drop the old mapping first, so on POSIX
            // its name is unlinked and the create below makes a fresh object
            // of the new size rather than reopening the small one. A reader
            // still mapping the old object sees its magic cleared, which is
            // its signal to reopen the name.
            if let Some(old) = self.regions.remove(name) {
                mmf_publish(&old, |r| r.write_bytes(0, &[0; 4]));
            }
            let region = match crate::shm::SharedRegion::create(&full, want) {
                Ok(region) => region,
                Err(e) => {
                    // Windows keeps a mapping alive while ANY process holds a
                    // handle or view, so with a reader attached the name could
                    // not be released and the create met the old, smaller
                    // object (Antigravity on #624). Take that object back, so
                    // the name keeps working at its old capacity, re-validate
                    // its header (the clear above told readers to reopen), and
                    // refuse this write with the reason. On POSIX the name was
                    // unlinked, so the open finds nothing and only the error
                    // is reported.
                    if let Ok(back) = crate::shm::SharedRegion::open(&full)
                        && back.len() > MMF_HEADER_LEN
                    {
                        let capacity =
                            u32::try_from(back.len() - MMF_HEADER_LEN).unwrap_or(u32::MAX);
                        mmf_publish(&back, |r| {
                            r.write_bytes(0, &MMF_MAGIC);
                            r.write_bytes(MMF_OFF_VERSION, &MMF_LAYOUT_VERSION.to_le_bytes());
                            r.write_bytes(MMF_OFF_CAPACITY, &capacity.to_le_bytes());
                        });
                        self.regions.insert(name.to_owned(), back);
                        return Err(format!(
                            "cannot grow {full} to {} bytes while another program has it \
                             open (it keeps {capacity} bytes): {e}",
                            data.len()
                        ));
                    }
                    return Err(format!("cannot create {full}: {e}"));
                }
            };
            if region.len() < want {
                return Err(format!("{full} exists and is too small"));
            }
            // A fresh (zero-filled) or reopened region: (re)initialise the
            // header under the sequence counter, like any write.
            let capacity = u32::try_from(region.len() - MMF_HEADER_LEN).unwrap_or(u32::MAX);
            mmf_publish(&region, |r| {
                r.write_bytes(0, &MMF_MAGIC);
                r.write_bytes(MMF_OFF_VERSION, &MMF_LAYOUT_VERSION.to_le_bytes());
                r.write_bytes(MMF_OFF_CAPACITY, &capacity.to_le_bytes());
            });
            self.regions.insert(name.to_owned(), region);
        }
        let region = &self.regions[name];
        let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
        mmf_publish(region, |r| {
            r.write_bytes(MMF_OFF_LENGTH, &len.to_le_bytes());
            r.write_bytes(MMF_HEADER_LEN, data);
        });
        Ok(())
    }

    /// Read up to `max` bytes of the named region's payload. A region this
    /// host wrote is read from its own mapping; any other is opened by name
    /// for this one read (so a region another process re-creates is always
    /// seen fresh). Empty when the name is invalid or absent, the header is
    /// not a valid v1 header, or a writer kept racing the copy.
    fn read(&self, name: &str, max: usize) -> Vec<u8> {
        let Some(full) = self.os_name(name) else {
            return Vec::new();
        };
        if let Some(r) = self.regions.get(name) {
            return mmf_read_region(r, max).unwrap_or_default();
        }
        crate::shm::SharedRegion::open(&full)
            .ok()
            .and_then(|r| mmf_read_region(&r, max))
            .unwrap_or_default()
    }
}

/// Run `write` as one seqlock-published update of `region`: the counter goes
/// odd before the first byte changes and even after the last, so a reader can
/// tell a torn copy from a whole one.
fn mmf_publish(region: &crate::shm::SharedRegion, write: impl FnOnce(&crate::shm::SharedRegion)) {
    let seq = region.atomic_u32(MMF_OFF_SEQ);
    let s = seq.load(Ordering::Relaxed);
    seq.store(s.wrapping_add(1) | 1, Ordering::Relaxed);
    std::sync::atomic::fence(Ordering::Release);
    write(region);
    seq.store((s.wrapping_add(1) | 1).wrapping_add(1), Ordering::Release);
}

/// One consistent copy of a region's payload (at most `max` bytes), or `None`
/// for a region whose header is not a valid v1 header, or a copy that raced a
/// writer [`MMF_READ_ATTEMPTS`] times.
fn mmf_read_region(region: &crate::shm::SharedRegion, max: usize) -> Option<Vec<u8>> {
    if region.len() < MMF_HEADER_LEN {
        return None;
    }
    let u32_at = |off: usize| {
        let mut b = [0u8; 4];
        region.read_bytes(off, &mut b);
        u32::from_le_bytes(b)
    };
    let seq = region.atomic_u32(MMF_OFF_SEQ);
    for _ in 0..MMF_READ_ATTEMPTS {
        let s1 = seq.load(Ordering::Acquire);
        if s1 & 1 == 1 {
            std::thread::yield_now();
            continue;
        }
        let mut magic = [0u8; 4];
        region.read_bytes(0, &mut magic);
        let capacity = u32_at(MMF_OFF_CAPACITY) as usize;
        let length = u32_at(MMF_OFF_LENGTH) as usize;
        let valid = magic == MMF_MAGIC
            && u32_at(MMF_OFF_VERSION) == MMF_LAYOUT_VERSION
            && capacity <= region.len() - MMF_HEADER_LEN
            && length <= capacity;
        let data = valid.then(|| {
            let mut v = vec![0u8; length.min(max)];
            region.read_bytes(MMF_HEADER_LEN, &mut v);
            v
        });
        std::sync::atomic::fence(Ordering::Acquire);
        if seq.load(Ordering::Relaxed) == s1 {
            return data;
        }
    }
    None
}

// ---------------------------------------------------------------------------
// v3.5.0 (CR-01) — the WebSocket client
// ---------------------------------------------------------------------------

/// The TCP connect and the WebSocket (and TLS) handshake are each bounded by
/// this, so an unreachable or stalled server ends in `open = false` rather
/// than a socket thread that never reports.
const WS_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The socket thread's read timeout once connected: how often it looks at its
/// outbound queue and the cancel flag between inbound frames.
const WS_POLL: Duration = Duration::from_millis(20);

/// The largest inbound WebSocket message (and frame) accepted, the same 1 MiB
/// the engine allows one outbound `comm.*` request. A larger one ends the
/// connection (`open = false`): a script cannot hold it anyway.
const WS_MAX_MESSAGE: usize = 1024 * 1024;

/// Inbound text not yet drained by the frame thread above which the socket
/// thread stops reading (see [`ScriptHost::ws_pending`]). The engine's own
/// `comm.receive()` inbox is capped at `MAX_QUEUED_CMDS` results (drop
/// oldest); this bounds the step before it, the channel to the frame thread.
const WS_INBOUND_BUDGET: usize = 4 * 1024 * 1024;

/// A command for the socket thread.
enum WsOut {
    Text(String),
    Close,
}

/// The host's handle on the one open WebSocket. The socket itself lives on
/// its thread; dropping the handle (or [`Self::close`]) closes it.
struct WsLink {
    out_tx: Sender<WsOut>,
}

impl WsLink {
    /// Start a socket thread for `url`. It connects, reports `open = true`
    /// (under `id`), then reports each inbound text message, and finally
    /// `open = false` once, however it ends. `None` if the OS would not start
    /// the thread.
    fn spawn(
        id: u64,
        url: String,
        allow: Vec<String>,
        result_tx: Sender<CommResult>,
        cancel: Arc<AtomicBool>,
        pending: Arc<AtomicUsize>,
    ) -> Option<Self> {
        let (out_tx, out_rx) = channel::<WsOut>();
        std::thread::Builder::new()
            .name("script-ws".to_string())
            .spawn(move || {
                ws_thread(id, &url, &allow, &out_rx, &result_tx, &cancel, &pending);
            })
            .ok()
            .map(|_detached| Self { out_tx })
    }

    fn send(&self, text: String) {
        let _ = self.out_tx.send(WsOut::Text(text));
    }

    /// Ask the socket to close. The thread finishes the closing handshake (or
    /// gives up after [`WS_CONNECT_TIMEOUT`]) and reports `open = false`.
    fn close(self) {
        let _ = self.out_tx.send(WsOut::Close);
    }
}

type WsSocket = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>;

/// Dial `url` under the SEC-04 destination rule and complete the WebSocket
/// handshake (TLS first for `wss://`).
///
/// The address check is the HTTP one, made on the addresses this function
/// then connects to: the name is resolved once, every resolved address must
/// be public unless the user allowlisted the host in
/// `RUSTYNES_COMM_HTTP_ALLOW`, and the TCP connection is made here, to one of
/// those addresses, before `tungstenite` sees the stream. Letting
/// `tungstenite::connect` dial instead would resolve the name a second time,
/// out of the check's sight (the DNS-rebinding hole the HTTP resolver closes).
/// `tungstenite` follows no redirects, and no proxy is consulted.
fn ws_connect(url: &str, allow: &[String]) -> Result<WsSocket, String> {
    use tungstenite::client::{IntoClientRequest, uri_mode};
    use tungstenite::stream::Mode;

    let request = url.into_client_request().map_err(|e| e.to_string())?;
    let uri = request.uri().clone();
    // `ws` / `wss` only: anything else is refused here.
    let mode = uri_mode(&uri).map_err(|e| e.to_string())?;
    let host = uri.host().ok_or("no host in the WebSocket URL")?;
    let bare = host
        .strip_prefix('[')
        .map_or(host, |h| h.strip_suffix(']').unwrap_or(h));
    let port = uri.port_u16().unwrap_or(match mode {
        Mode::Plain => 80,
        Mode::Tls => 443,
    });
    let addrs: Vec<std::net::SocketAddr> = (bare, port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve {bare}:{port}: {e}"))?
        .collect();
    if !host_port_allowlisted(host, Some(port), allow)
        && let Some(blocked) = addrs.iter().find(|a| !is_public(a.ip()))
    {
        return Err(format!(
            "blocked: {bare} resolves to {}, a non-public address; list the host in {HTTP_ALLOW_ENV} to allow it",
            blocked.ip()
        ));
    }
    let mut last = format!("no addresses for {bare}:{port}");
    let mut stream = None;
    for addr in &addrs {
        match TcpStream::connect_timeout(addr, WS_CONNECT_TIMEOUT) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last = format!("connect {addr}: {e}"),
        }
    }
    let stream = stream.ok_or(last)?;
    let _ = stream.set_nodelay(true);
    // The handshake runs under the generous timeout; the loop then switches
    // to `WS_POLL` (the netplay signaling client's lesson: a 20 ms timeout
    // during a slow handshake aborts it with an interrupted-handshake error).
    stream
        .set_read_timeout(Some(WS_CONNECT_TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(WS_CONNECT_TIMEOUT))
        .map_err(|e| e.to_string())?;
    let config = tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(WS_MAX_MESSAGE))
        .max_frame_size(Some(WS_MAX_MESSAGE));
    let (socket, _resp) = tungstenite::client_tls_with_config(request, stream, Some(config), None)
        .map_err(|e| e.to_string())?;
    Ok(socket)
}

/// The raw TCP stream under a plain or TLS socket, to change its timeout.
fn ws_tcp(socket: &WsSocket) -> Option<&TcpStream> {
    match socket.get_ref() {
        tungstenite::stream::MaybeTlsStream::Plain(s) => Some(s),
        tungstenite::stream::MaybeTlsStream::Rustls(s) => Some(&s.sock),
        _ => None,
    }
}

/// The socket thread: connect, then interleave the outbound queue with
/// inbound frames until either side closes, the host is dropped, or the
/// worker hands the slot to a new socket.
fn ws_thread(
    id: u64,
    url: &str,
    allow: &[String],
    out_rx: &Receiver<WsOut>,
    result_tx: &Sender<CommResult>,
    cancel: &AtomicBool,
    pending: &AtomicUsize,
) {
    let closed = || {
        let _ = result_tx.send(CommResult::WsState {
            id,
            open: false,
            message: None,
        });
    };
    let mut socket = match ws_connect(url, allow) {
        Ok(s) => s,
        Err(why) => {
            eprintln!("rustynes: script WebSocket {url}: {why}");
            closed();
            return;
        }
    };
    if let Some(tcp) = ws_tcp(&socket) {
        let _ = tcp.set_read_timeout(Some(WS_POLL));
    }
    let _ = result_tx.send(CommResult::WsState {
        id,
        open: true,
        message: None,
    });
    // Set once `Close` is requested: the loop keeps reading until the server
    // answers the close frame, or this passes.
    let mut closing_since: Option<Instant> = None;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = socket.close(None);
            let _ = socket.flush();
            return; // the host is gone; nobody reads a final state.
        }
        if closing_since.is_some_and(|t| t.elapsed() > WS_CONNECT_TIMEOUT) {
            break;
        }
        // 1. Outbound.
        if closing_since.is_none() {
            loop {
                match out_rx.try_recv() {
                    Ok(WsOut::Text(t)) => {
                        if socket.send(tungstenite::Message::Text(t.into())).is_err() {
                            closed();
                            return;
                        }
                    }
                    Ok(WsOut::Close) | Err(TryRecvError::Disconnected) => {
                        let _ = socket.close(None);
                        let _ = socket.flush();
                        closing_since = Some(Instant::now());
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                }
            }
        }
        // 2. Back-pressure: while the frame thread has not drained enough,
        // leave inbound bytes in the kernel (the server's TCP window fills).
        if pending.load(Ordering::Acquire) > WS_INBOUND_BUDGET {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        // 3. Inbound, bounded by `WS_POLL`.
        let text = match socket.read() {
            Ok(tungstenite::Message::Text(t)) => t.as_str().to_owned(),
            // A Lua string is bytes, but the result carries text: a binary
            // message is delivered with invalid UTF-8 replaced.
            Ok(tungstenite::Message::Binary(b)) => String::from_utf8_lossy(&b).into_owned(),
            Ok(tungstenite::Message::Close(_)) => break,
            Ok(_) => {
                // Ping / pong / raw frame: tungstenite queues the pong; send it.
                let _ = socket.flush();
                continue;
            }
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(_) => break,
        };
        // Counted before it is sent, so the drain's subtraction never runs
        // ahead of this addition.
        pending.fetch_add(text.len(), Ordering::AcqRel);
        if result_tx
            .send(CommResult::WsState {
                id,
                open: true,
                message: Some(text),
            })
            .is_err()
        {
            return; // the host is gone.
        }
    }
    closed();
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

/// A machine-unique MMF prefix for one test host (see
/// [`ScriptHost::with_allowlist`]); short enough that prefix plus a test's
/// name stays inside the portable 30-character limit.
#[cfg(test)]
fn test_prefix() -> String {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    // Exactly as long as `MMF_PREFIX` (9), so the name-length rule tests the
    // production limit.
    format!("t{:05}{:02}-", std::process::id() % 100_000, n % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wait up to two seconds for the worker's first result.
    fn first_result(host: &ScriptHost) -> Option<CommResult> {
        for _ in 0..2000 {
            if let Some(r) = host.drain_results().into_iter().next() {
                return Some(r);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        None
    }

    /// The host-owned memory-mapped-file bridge round-trips through the worker:
    /// a `MmfWrite` then a `MmfRead` returns the stored bytes (truncated to the
    /// requested length) as a `CommResult`.
    #[test]
    fn mmf_write_then_read_round_trips_via_the_host() {
        let host = ScriptHost::for_test();
        host.submit(CommCmd::MmfWrite {
            name: "frame".to_string(),
            data: vec![1, 2, 3, 4, 5],
        })
        .expect("queued");
        host.submit(CommCmd::MmfRead {
            id: 7,
            name: "frame".to_string(),
            len: 3,
        })
        .expect("queued");
        assert_eq!(
            first_result(&host),
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
        let host = ScriptHost::for_test();
        host.submit(CommCmd::MmfRead {
            id: 1,
            name: "nope".to_string(),
            len: 16,
        })
        .expect("queued");
        assert_eq!(
            first_result(&host),
            Some(CommResult::Mmf {
                id: 1,
                data: vec![]
            })
        );
    }

    /// A reader written from the DOCUMENTED layout only (`docs/scripting.md`,
    /// "Memory-mapped files"): magic, version, capacity, length and sequence
    /// at fixed offsets, the payload at 32. It shares no code with
    /// `mmf_read_region`, so a layout change on one side shows up here.
    fn read_documented(region: &crate::shm::SharedRegion) -> Vec<u8> {
        let u32_at = |off: usize| {
            let mut b = [0u8; 4];
            region.read_bytes(off, &mut b);
            u32::from_le_bytes(b)
        };
        // Bounded, so a writer that never finishes fails the test instead of
        // hanging it.
        for _ in 0..1_000_000 {
            let s1 = region.atomic_u32(16).load(Ordering::Acquire);
            if s1 % 2 == 1 {
                std::thread::yield_now();
                continue;
            }
            let mut magic = [0u8; 4];
            region.read_bytes(0, &mut magic);
            assert_eq!(&magic, b"RNMF");
            assert_eq!(u32_at(4), 1, "layout version");
            let capacity = u32_at(8) as usize;
            let length = u32_at(12) as usize;
            assert!(length <= capacity && 32 + capacity <= region.len());
            let mut data = vec![0u8; length];
            region.read_bytes(32, &mut data);
            if region.atomic_u32(16).load(Ordering::Acquire) == s1 {
                return data;
            }
        }
        panic!("the sequence counter never settled: a write never completed");
    }

    /// A reader must never return a copy taken while a write is in
    /// progress. A writer stalled mid-write (the counter held odd, as when
    /// the writing process is descheduled) leaves a payload that may be half
    /// old and half new; the reader must refuse it (empty, the transport-
    /// failure value) rather than return it. A test with two live threads
    /// almost never lands a copy entirely inside one write, so the stall is
    /// set up directly.
    #[test]
    fn a_read_never_returns_a_copy_taken_mid_write() {
        let mut store = MmfStore::new(test_prefix());
        store.write("tear", b"whole").unwrap();
        let region = &store.regions["tear"];
        let seq = region.atomic_u32(MMF_OFF_SEQ);
        let even = seq.load(Ordering::Acquire);
        assert_eq!(even % 2, 0, "a finished write leaves the counter even");
        seq.store(even + 1, Ordering::Release); // a writer is "inside" now
        region.write_bytes(MMF_HEADER_LEN, b"ha");
        assert_eq!(mmf_read_region(region, 64), None, "a copy during a write");
        assert_eq!(store.read("tear", 64), Vec::<u8>::new());
        seq.store(even + 2, Ordering::Release); // the write completes
        assert_eq!(store.read("tear", 64), b"haole");
    }

    /// CR-02's host test: what a script writes is real OS shared memory. A
    /// second, independent mapping opened BY NAME (`<prefix><script name>`) on
    /// another thread reads the script's bytes through the documented layout.
    /// (`shm::tests::another_process_reads_the_region_by_name` proves the same
    /// object is visible from a second process.)
    #[test]
    fn a_script_mmf_write_is_readable_by_name_from_outside() {
        let prefix = test_prefix();
        let host = ScriptHost::spawn(Some(Vec::new()), prefix.clone());
        host.submit(CommCmd::MmfWrite {
            name: "state".into(),
            data: b"lives=3;world=1-2".to_vec(),
        })
        .unwrap();
        // A read through the host is the barrier that the write was done.
        host.submit(CommCmd::MmfRead {
            id: 1,
            name: "state".into(),
            len: 64,
        })
        .unwrap();
        assert!(first_result(&host).is_some());
        let name = format!("{prefix}state");
        let seen = std::thread::spawn(move || {
            let r = crate::shm::SharedRegion::open(&name).expect("open the script's region");
            read_documented(&r)
        })
        .join()
        .unwrap();
        assert_eq!(seen, b"lives=3;world=1-2");
        // Dropping the host removes the regions it made.
        drop(host);
        assert!(crate::shm::SharedRegion::open(&format!("{prefix}state")).is_err());
    }

    /// The other direction: a region another program made, in the documented
    /// layout, is what `comm.mmfRead` returns.
    #[test]
    fn a_region_made_outside_is_read_by_the_script() {
        let prefix = test_prefix();
        let host = ScriptHost::spawn(Some(Vec::new()), prefix.clone());
        let outside = crate::shm::SharedRegion::create(&format!("{prefix}cmd"), 4096).unwrap();
        outside.write_bytes(0, b"RNMF");
        outside.write_bytes(4, &1u32.to_le_bytes());
        outside.write_bytes(8, &(4096u32 - 32).to_le_bytes());
        outside.write_bytes(12, &5u32.to_le_bytes());
        outside.write_bytes(32, b"RIGHT");
        host.submit(CommCmd::MmfRead {
            id: 9,
            name: "cmd".into(),
            len: 100,
        })
        .unwrap();
        assert_eq!(
            first_result(&host),
            Some(CommResult::Mmf {
                id: 9,
                data: b"RIGHT".to_vec()
            })
        );
        // A region without the header is not read as data.
        let raw = crate::shm::SharedRegion::create(&format!("{prefix}raw"), 4096).unwrap();
        raw.write_bytes(32, b"junk");
        host.submit(CommCmd::MmfRead {
            id: 10,
            name: "raw".into(),
            len: 100,
        })
        .unwrap();
        assert_eq!(
            first_result(&host),
            Some(CommResult::Mmf {
                id: 10,
                data: Vec::new()
            })
        );
    }

    /// A write larger than the region grows it, and the old mapping's magic
    /// is cleared so a reader holding it knows to reopen the name.
    #[test]
    #[cfg(not(windows))]
    fn a_larger_write_replaces_the_region() {
        let mut store = MmfStore::new(test_prefix());
        store.write("grow", &[1u8; 10]).unwrap();
        let first = &store.regions["grow"];
        assert_eq!(first.len(), 4096);
        let reader = crate::shm::SharedRegion::open(first.name()).unwrap();
        store.write("grow", &[2u8; 10_000]).unwrap();
        assert_eq!(store.regions["grow"].len(), 12_288);
        let mut magic = [0u8; 4];
        reader.read_bytes(0, &mut magic);
        assert_eq!(magic, [0; 4], "the stale mapping is marked invalid");
        assert_eq!(store.read("grow", usize::MAX), vec![2u8; 10_000]);
    }

    /// Windows cannot release a mapping another process still has open, so a
    /// grow with a reader attached is refused with the reason, and the region
    /// keeps working at its old capacity: same name, header valid, old data
    /// readable, and a write that fits still lands. Without the recovery the
    /// name dropped out of the store and every later write failed too.
    #[test]
    #[cfg(windows)]
    fn a_larger_write_with_a_reader_attached_is_refused_and_the_region_kept() {
        let mut store = MmfStore::new(test_prefix());
        store.write("grow", &[1u8; 10]).unwrap();
        let reader = crate::shm::SharedRegion::open(store.regions["grow"].name()).unwrap();
        let err = store.write("grow", &[2u8; 10_000]).unwrap_err();
        assert!(err.contains("another program has it open"), "{err}");
        let mut magic = [0u8; 4];
        reader.read_bytes(0, &mut magic);
        assert_eq!(magic, MMF_MAGIC, "the kept region is valid again");
        assert_eq!(store.read("grow", usize::MAX), vec![1u8; 10]);
        store.write("grow", &[3u8; 20]).unwrap();
        assert_eq!(store.read("grow", usize::MAX), vec![3u8; 20]);
        drop(reader);
    }

    /// Script names outside the portable rule are refused (and never reach
    /// the OS as a path): no separators, no over-long names.
    #[test]
    fn mmf_names_are_checked() {
        // A prefix exactly as long as the production one, so the length
        // limit tested is the production limit.
        let prefix = format!("u{:03}-", std::process::id() % 1000);
        assert_eq!(prefix.len(), MMF_PREFIX.len());
        let mut store = MmfStore::new(prefix);
        for bad in ["", "a/b", "..", "x\\y", &"n".repeat(MAX_MMF_NAME_LEN + 1)] {
            if bad == ".." {
                continue; // dots are legal characters; ".." is just a name.
            }
            assert!(store.write(bad, b"x").is_err(), "{bad:?}");
            assert_eq!(store.read(bad, 8), Vec::<u8>::new());
        }
        assert!(store.write(&"n".repeat(MAX_MMF_NAME_LEN), b"x").is_ok());
        assert_eq!(
            MMF_PREFIX.len() + MAX_MMF_NAME_LEN,
            crate::shm::MAX_NAME_LEN
        );
    }

    /// v2.9.0 re-audit NF-03: the store outlives every frame, so it has its
    /// own budget. 32 distinct 1 MiB writes must not all be kept, and the
    /// name count is capped.
    #[test]
    fn the_mmf_store_is_bounded() {
        let mut store = MmfStore::new(test_prefix());
        let mut kept = 0;
        for i in 0..32 {
            if store
                .write(&format!("m{i}"), &vec![0u8; 1024 * 1024])
                .is_ok()
            {
                kept += 1;
            }
        }
        assert!(
            store.held_bytes() <= MAX_MMF_BYTES,
            "the store holds {} bytes",
            store.held_bytes()
        );
        assert!(kept < 32, "a full store refuses");
        assert!(kept >= 14, "only {kept} kept: the budget is 16 MiB");
        // Overwriting a kept name reuses its region.
        assert!(store.write("m0", &vec![1u8; 1024 * 1024]).is_ok());
        drop(store);
        let mut names = MmfStore::new(test_prefix());
        for i in 0..=MAX_MMF_NAMES {
            let _ = names.write(&format!("n{i}"), &[]);
        }
        assert_eq!(
            names.regions.len(),
            MAX_MMF_NAMES,
            "the name count is capped"
        );
    }

    // --- CR-01: the WebSocket client ---------------------------------------

    /// A local WebSocket echo server: answers each text message with
    /// `echo:<text>`, and a `close-me` message by closing. Returns its port.
    fn echo_server() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || {
                    let Ok(mut ws) = tungstenite::accept(stream) else {
                        return;
                    };
                    while let Ok(msg) = ws.read() {
                        match msg {
                            tungstenite::Message::Text(t) if t.as_str() == "close-me" => {
                                let _ = ws.close(None);
                            }
                            tungstenite::Message::Text(t) => {
                                let reply = format!("echo:{}", t.as_str());
                                if ws.send(tungstenite::Message::Text(reply.into())).is_err() {
                                    return;
                                }
                            }
                            _ => {}
                        }
                    }
                });
            }
        });
        port
    }

    /// Collect results until `pred` holds for one, or five seconds pass.
    fn wait_for(
        host: &ScriptHost,
        seen: &mut Vec<CommResult>,
        pred: impl Fn(&CommResult) -> bool,
    ) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            for r in host.drain_results() {
                let hit = pred(&r);
                seen.push(r);
                if hit {
                    return true;
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        false
    }

    /// CR-01's host test: open, send, receive the echo, close, against a local
    /// echo server. Until v3.5.0 `ws_open` reported `open = false` at once.
    #[test]
    fn a_websocket_opens_sends_receives_and_closes() {
        let port = echo_server();
        let host = ScriptHost::with_allowlist(vec!["127.0.0.1".into()]);
        host.submit(CommCmd::WsOpen {
            id: 42,
            url: format!("ws://127.0.0.1:{port}/"),
        })
        .unwrap();
        let mut seen = Vec::new();
        assert!(
            wait_for(&host, &mut seen, |r| matches!(
                r,
                CommResult::WsState {
                    id: 42,
                    open: true,
                    message: None
                }
            )),
            "no open event: {seen:?}"
        );
        host.submit(CommCmd::WsSend("hello".into())).unwrap();
        host.submit(CommCmd::WsSend("world".into())).unwrap();
        assert!(
            wait_for(&host, &mut seen, |r| matches!(
                r,
                CommResult::WsState { message: Some(m), .. } if m == "echo:world"
            )),
            "no echo: {seen:?}"
        );
        let messages: Vec<&str> = seen
            .iter()
            .filter_map(|r| match r {
                CommResult::WsState {
                    id: 42,
                    open: true,
                    message: Some(m),
                } => Some(m.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            messages,
            ["echo:hello", "echo:world"],
            "in order, under the open id"
        );
        host.submit(CommCmd::WsClose).unwrap();
        assert!(
            wait_for(&host, &mut seen, |r| matches!(
                r,
                CommResult::WsState {
                    id: 42,
                    open: false,
                    ..
                }
            )),
            "no close event: {seen:?}"
        );
        // Everything drained: the inbound budget is back to zero.
        assert_eq!(host.ws_pending.load(Ordering::Acquire), 0);
    }

    /// A server that closes the socket is reported as `open = false`.
    #[test]
    fn a_server_close_is_reported() {
        let port = echo_server();
        let host = ScriptHost::with_allowlist(vec!["127.0.0.1".into()]);
        host.submit(CommCmd::WsOpen {
            id: 5,
            url: format!("ws://127.0.0.1:{port}/"),
        })
        .unwrap();
        let mut seen = Vec::new();
        assert!(wait_for(&host, &mut seen, |r| matches!(
            r,
            CommResult::WsState { open: true, .. }
        )));
        host.submit(CommCmd::WsSend("close-me".into())).unwrap();
        assert!(
            wait_for(&host, &mut seen, |r| matches!(
                r,
                CommResult::WsState {
                    id: 5,
                    open: false,
                    ..
                }
            )),
            "{seen:?}"
        );
    }

    /// SEC-04 holds for a WebSocket: without the user's allowlist a loopback
    /// URL is refused before any connection is made.
    #[test]
    fn a_loopback_websocket_is_refused_without_the_allowlist() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let host = ScriptHost::for_test();
        host.submit(CommCmd::WsOpen {
            id: 3,
            url: format!("ws://127.0.0.1:{port}/"),
        })
        .unwrap();
        let mut seen = Vec::new();
        assert!(
            wait_for(&host, &mut seen, |r| matches!(
                r,
                CommResult::WsState {
                    id: 3,
                    open: false,
                    ..
                }
            )),
            "{seen:?}"
        );
        assert!(
            !seen
                .iter()
                .any(|r| matches!(r, CommResult::WsState { open: true, .. })),
            "it must never report open"
        );
        assert!(listener.accept().is_err(), "no connection was made");
        let blocked = ws_connect(&format!("ws://127.0.0.1:{port}/"), &[]).unwrap_err();
        assert!(blocked.contains("blocked"), "{blocked}");
        // And a non-WebSocket scheme is refused outright.
        assert!(ws_connect("http://example.com/", &[]).is_err());
    }

    /// Back-pressure: a server that floods the socket is read only while the
    /// undrained inbound text is under `WS_INBOUND_BUDGET`, so an idle frame
    /// thread cannot be made to hold an unbounded backlog.
    #[test]
    fn a_flooding_server_is_held_to_the_inbound_budget() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let chunk = "x".repeat(64 * 1024);
            // 64 MiB offered; the client must stop near 4 MiB.
            for _ in 0..1024 {
                if ws
                    .send(tungstenite::Message::Text(chunk.clone().into()))
                    .is_err()
                {
                    return;
                }
            }
        });
        let host = ScriptHost::with_allowlist(vec!["127.0.0.1".into()]);
        host.submit(CommCmd::WsOpen {
            id: 1,
            url: format!("ws://127.0.0.1:{port}/"),
        })
        .unwrap();
        // Do not drain for a while: let the socket thread run into the budget.
        std::thread::sleep(Duration::from_millis(1500));
        let held = host.ws_pending.load(Ordering::Acquire);
        assert!(
            held <= WS_INBOUND_BUDGET + 64 * 1024,
            "{held} bytes undrained: the budget is {WS_INBOUND_BUDGET}"
        );
        assert!(held > 0, "the socket was read at all");
        // Draining releases it and reading resumes.
        let first: usize = host
            .drain_results()
            .iter()
            .filter_map(|r| match r {
                CommResult::WsState {
                    message: Some(m), ..
                } => Some(m.len()),
                _ => None,
            })
            .sum();
        assert_eq!(first, held);
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            host.ws_pending.load(Ordering::Acquire) > 0,
            "reading resumed"
        );
    }

    /// A local listener that accepts every connection and never answers,
    /// holding each socket open, so an HTTP request to it blocks until the
    /// agent's 20 s timeout.
    #[cfg(feature = "script-ipc")]
    fn silent_server() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            // Kept only so the sockets stay open (a dropped stream would
            // close the connection and let the request fail fast).
            #[allow(clippy::collection_is_never_read)]
            let mut held = Vec::new();
            for stream in listener.incoming().flatten() {
                held.push(stream);
            }
        });
        port
    }

    /// v2.9.0 re-audit NF-08: `Drop` joined the worker, and `mpsc` delivers
    /// every buffered job before reporting disconnection, so dropping a host
    /// with a backlog of slow requests blocked the dropping thread (the UI
    /// thread, on Reload) for up to 20 s per request, while a STOPPED
    /// script's requests kept going out. Dropping must now return promptly.
    /// Timed on its own thread: a regression fails at 10 s, it never stalls.
    #[cfg(feature = "script-ipc")]
    #[test]
    fn dropping_a_host_with_a_slow_backlog_returns_promptly() {
        let port = silent_server();
        let host = ScriptHost::with_allowlist(vec!["127.0.0.1".into()]);
        for id in 1..=10 {
            let _ = host.submit(CommCmd::HttpGet {
                id,
                url: format!("http://127.0.0.1:{port}/"),
            });
        }
        // Let the worker enter the first (blocking) request.
        std::thread::sleep(Duration::from_millis(200));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let t = Instant::now();
            drop(host);
            let _ = tx.send(t.elapsed());
        });
        let took = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("drop did not return within 10 s: it waits on the backlog");
        assert!(took < Duration::from_secs(1), "drop took {took:?}");
    }

    /// NF-08, the other half: a stopped script's backlog must stop going out.
    /// A listener that answers each request after 300 ms counts connections;
    /// ten requests are queued and the host dropped mid-way through the
    /// first. Without the cancel flag the detached worker would keep working
    /// through all ten (one every 300 ms).
    #[cfg(feature = "script-ipc")]
    #[test]
    fn dropping_a_host_abandons_its_backlog() {
        use std::io::Read;
        use std::sync::atomic::AtomicUsize;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                counter.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                std::thread::sleep(Duration::from_millis(300));
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
            }
        });
        let host = ScriptHost::with_allowlist(vec!["127.0.0.1".into()]);
        for id in 1..=10 {
            host.submit(CommCmd::HttpGet {
                id,
                url: format!("http://127.0.0.1:{port}/"),
            })
            .expect("queued");
        }
        std::thread::sleep(Duration::from_millis(100));
        drop(host);
        std::thread::sleep(Duration::from_millis(1500));
        let n = hits.load(Ordering::SeqCst);
        assert!(n <= 2, "{n} requests went out after the host was dropped");
    }

    /// NF-08: the queue is bounded, and a full one refuses at once instead of
    /// blocking the frame thread. The refusal reads as a transport failure.
    #[cfg(feature = "script-ipc")]
    #[test]
    fn a_full_queue_refuses_without_blocking() {
        let port = silent_server();
        let host = ScriptHost::with_allowlist(vec!["127.0.0.1".into()]);
        let t = Instant::now();
        let mut refused = Vec::new();
        for id in 1..=(COMM_QUEUE_DEPTH as u64 + 10) {
            if let Err(cmd) = host.submit(CommCmd::HttpGet {
                id,
                url: format!("http://127.0.0.1:{port}/"),
            }) {
                refused.push(cmd);
            }
        }
        assert!(t.elapsed() < Duration::from_secs(1), "submit blocked");
        assert!(!refused.is_empty(), "a full queue refuses");
        let first = &refused[0];
        let CommCmd::HttpGet { id, .. } = *first else {
            panic!("refused an HttpGet")
        };
        assert_eq!(
            ScriptHost::refusal(first),
            Some(CommResult::Http {
                id,
                status: 0,
                body: String::new()
            })
        );
        assert_eq!(ScriptHost::refusal(&CommCmd::WsClose), None);
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

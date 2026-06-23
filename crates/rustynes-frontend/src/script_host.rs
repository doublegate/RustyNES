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
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        // Report the real status + body for non-2xx instead of an `Err` (ureq 3's
        // `StatusCode` error drops the body the script wants).
        .http_status_as_error(false)
        .build()
        .into();
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
        |mut resp| {
            let status = resp.status().as_u16();
            let body = resp.body_mut().read_to_string().unwrap_or_default();
            (status, body)
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
}

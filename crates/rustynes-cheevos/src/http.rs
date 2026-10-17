//! Off-thread HTTP transport for rcheevos server calls.
//!
//! rcheevos issues server calls through a `rc_client_server_call_t` callback.
//! It is asynchronous: rcheevos hands us a request plus a completion callback
//! (`rc_client_server_callback_t`) + opaque `callback_data`, and expects us to
//! invoke that completion later with the server response.
//!
//! ## Threading model
//!
//! A single worker thread owns a [`ureq::Agent`] and performs the blocking
//! HTTP. The `server_call` trampoline merely enqueues a [`HttpJob`] (it never
//! blocks the emulator thread and never touches the `rc_client`). The worker
//! sends each [`HttpCompletion`] back over a channel.
//!
//! The `rc_client` completion callback is **never** invoked on the worker — that
//! would re-enter rcheevos from the wrong thread. Instead
//! [`HttpTransport::poll_completions`] drains the completion channel on the
//! main thread and invokes each `rc_client_server_callback_t` there, building a
//! stack [`rc_api_server_response_t`] that borrows the response bytes for the
//! duration of the call.
//!
//! The `rc_client` callback pointer + `callback_data` are carried as `usize` (raw
//! pointer bits) so the job is `Send`; they are only ever dereferenced back on
//! the main thread in `poll_completions`.
//!
//! ## Offline unlocks (v3.4.0, `T-RA-HARDCORE`)
//!
//! RA's compliance page requires that "unlocks created while offline must be
//! securely cached and sync to `RetroAchievements` when connectivity returns"
//! (<https://docs.retroachievements.org/general/hardcore-compliance-requirements.html>,
//! section A). `rc_client` already does the caching: an award or leaderboard
//! submission whose server call fails RETRYABLY is kept in its scheduled
//! queue and re-sent from `rc_client_do_frame` / `rc_client_idle` (first retry
//! at once, then 1 s, 2 s, ... capped at 120 s), raising
//! `RC_CLIENT_EVENT_DISCONNECTED` while anything is pending and
//! `RC_CLIENT_EVENT_RECONNECTED` once it all went through
//! (<https://github.com/RetroAchievements/rcheevos/wiki/rc_client-integration>,
//! "Server Errors"). This transport does not duplicate that queue. Its one job
//! is to report a transport failure (no network, DNS, TLS, refused, timeout)
//! as `RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR` with the error text as
//! the body. Until v3.4.0 it reported `RC_API_SERVER_RESPONSE_CLIENT_ERROR` (-1)
//! with an empty body, which rcheevos treats as a final error from the
//! server: an unlock earned while offline raised a server-error toast and was
//! never sent again. The queue is in memory, so an unlock still pending when
//! the program exits is lost; see `docs/frontend.md` for why that is left to
//! `rc_client`.
//!
//! The blocking exchange itself is a [`Fetch`] the worker owns: `ureq` in the
//! shipped build, a scripted fake server in the tests (the HTTP seam the
//! end-to-end retry test drives).

use std::os::raw::c_void;
use std::sync::mpsc::{Receiver, Sender};
use std::thread::JoinHandle;

use crate::ffi;

/// A queued HTTP request handed off to the worker thread.
struct HttpJob {
    url: String,
    /// `Some` => POST with this body, `None` => GET.
    post: Option<Vec<u8>>,
    content_type: String,
    /// `rc_client_server_callback_t` as raw bits (invoked on the main thread).
    callback: usize,
    /// rcheevos `callback_data` as raw bits.
    callback_data: usize,
}

// SAFETY: the `callback`/`callback_data` pointer bits are inert on the worker;
// they are only turned back into pointers and invoked on the main thread.
unsafe impl Send for HttpJob {}

/// One request as the [`Fetch`] sees it.
pub struct Request<'a> {
    /// The full request URL.
    pub url: &'a str,
    /// `Some` => POST with this body, `None` => GET.
    pub post: Option<&'a [u8]>,
    /// The POST body's content type (empty for a GET).
    pub content_type: &'a str,
}

/// The blocking exchange the worker performs: `Ok((body, http_status))` for
/// any HTTP response (a 4xx / 5xx included, with its body), `Err(text)` for a
/// transport failure that produced no HTTP response at all.
pub type Fetch = Box<dyn FnMut(&Request<'_>) -> Result<(Vec<u8>, i32), String> + Send>;

/// A completed HTTP exchange ready to be delivered to rcheevos.
struct HttpCompletion {
    body: Vec<u8>,
    http_status_code: i32,
    callback: usize,
    callback_data: usize,
}

// SAFETY: as above, the pointer bits are only used on the main thread.
unsafe impl Send for HttpCompletion {}

/// Owns the worker thread and the channels bridging it to the main thread.
pub struct HttpTransport {
    job_tx: Option<Sender<HttpJob>>,
    completion_rx: Receiver<HttpCompletion>,
    worker: Option<JoinHandle<()>>,
}

impl HttpTransport {
    /// Spawn the worker thread with a fresh `ureq::Agent`.
    pub(crate) fn new() -> Self {
        Self::with_fetch(ureq_fetch())
    }

    /// Spawn the worker thread around `fetch` (the shipped build passes
    /// [`ureq_fetch`]; the tests pass a scripted fake server).
    pub(crate) fn with_fetch(fetch: Fetch) -> Self {
        let (job_tx, job_rx) = std::sync::mpsc::channel::<HttpJob>();
        let (completion_tx, completion_rx) = std::sync::mpsc::channel::<HttpCompletion>();

        let worker = std::thread::Builder::new()
            .name("ra-http".into())
            .spawn(move || worker_loop(&job_rx, &completion_tx, fetch))
            .expect("spawn ra-http worker thread");

        Self {
            job_tx: Some(job_tx),
            completion_rx,
            worker: Some(worker),
        }
    }

    /// Enqueue a job (called from the `server_call` trampoline).
    fn enqueue(&self, job: HttpJob) {
        if let Some(tx) = &self.job_tx {
            // If the worker is gone, drop the job: rcheevos will time the
            // request out on its own (we simply never call the completion).
            let _ = tx.send(job);
        }
    }

    /// Drain completed exchanges and invoke their rcheevos callbacks on the
    /// current (main) thread.
    pub(crate) fn poll_completions(&self) {
        while let Ok(done) = self.completion_rx.try_recv() {
            // Rebuild the C callback pointer + data from the carried bits.
            let cb: ffi::rc_client_server_callback_t = {
                // rcheevos may pass a NULL completion callback; the trampoline
                // carried it as 0 (v3.2.0, the callback type is an `Option`),
                // and it is skipped below instead of called.
                if done.callback == 0 {
                    None
                } else {
                    // SAFETY: a non-zero `done.callback` is the bits of the exact
                    // non-null `extern "C" fn` rcheevos handed the trampoline
                    // (`server_call_trampoline`), so transmuting them back yields
                    // that pointer; it is invoked here, on the main thread.
                    unsafe {
                        std::mem::transmute::<usize, ffi::rc_client_server_callback_t>(
                            done.callback,
                        )
                    }
                }
            };
            let callback_data = done.callback_data as *mut c_void;

            let response = ffi::rc_api_server_response_t {
                body: done.body.as_ptr().cast::<std::os::raw::c_char>(),
                body_length: done.body.len(),
                http_status_code: done.http_status_code,
            };
            // SAFETY: `cb` is a valid rcheevos completion callback; `response`
            // borrows `done.body` which outlives this call.
            if let Some(cb_fn) = cb {
                cb_fn(&raw const response, callback_data);
            }
        }
    }
}

impl Drop for HttpTransport {
    fn drop(&mut self) {
        // Close the job channel so the worker loop exits, then join it -- but
        // only briefly (v2.7.4, audit MOB-04).
        self.job_tx = None;
        if let Some(worker) = self.worker.take() {
            join_or_detach(worker, DROP_GRACE);
        }
    }
}

/// How long `Drop` waits for the worker before detaching it (v2.7.4, MOB-04).
/// An idle worker is blocked in `recv` and exits as soon as the job channel
/// closes, so this covers the normal case; only a request in flight outlives it.
const DROP_GRACE: std::time::Duration = std::time::Duration::from_millis(100);

/// Join `worker` if it finishes within `grace`; otherwise let it go.
///
/// Before v2.7.4 `Drop` joined unconditionally, and a request in flight to an
/// unreachable `RetroAchievements` server held the dropping thread (a logout, a
/// ROM change; on mobile, the UI thread) for up to the 30 s request timeout.
/// Detaching is safe: the worker owns its agent and the request; when the
/// request returns, sending the completion fails because the receiver is gone,
/// and it exits. It never calls the rcheevos callback -- only
/// [`HttpTransport::poll_completions`] does, on the caller's thread, and that
/// receiver is dropped with `self`. Returns whether it joined.
fn join_or_detach(worker: JoinHandle<()>, grace: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + grace;
    while !worker.is_finished() {
        if std::time::Instant::now() >= deadline {
            return false; // dropping the handle detaches the thread
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let _ = worker.join();
    true
}

/// The operating-system name for the User-Agent's system-information clause,
/// chosen at compile time. RA's format is
/// `EmulatorName/v1.0.0 (OSName 10.0) core_name/v0.5.0`, where the
/// parenthesised system information is optional; the OS VERSION is not known
/// at compile time and has no portable runtime query, so the clause carries
/// the OS and the CPU architecture, which RA's own examples show is accepted
/// free-form ("PCSX/v2.5.103 (Microsoft Windows 10+)").
#[cfg(target_os = "linux")]
macro_rules! ra_os {
    () => {
        "Linux"
    };
}
#[cfg(target_os = "android")]
macro_rules! ra_os {
    () => {
        "Android"
    };
}
#[cfg(target_os = "macos")]
macro_rules! ra_os {
    () => {
        "macOS"
    };
}
#[cfg(target_os = "ios")]
macro_rules! ra_os {
    () => {
        "iOS"
    };
}
#[cfg(target_os = "windows")]
macro_rules! ra_os {
    () => {
        "Windows"
    };
}
#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "windows"
)))]
macro_rules! ra_os {
    () => {
        "Unknown"
    };
}

/// The CPU architecture for the same clause (`x86_64`, `aarch64`, ...).
#[cfg(target_arch = "x86_64")]
macro_rules! ra_arch {
    () => {
        "x86_64"
    };
}
#[cfg(target_arch = "aarch64")]
macro_rules! ra_arch {
    () => {
        "aarch64"
    };
}
#[cfg(target_arch = "x86")]
macro_rules! ra_arch {
    () => {
        "x86"
    };
}
#[cfg(target_arch = "arm")]
macro_rules! ra_arch {
    () => {
        "arm"
    };
}
#[cfg(not(any(
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "x86",
    target_arch = "arm"
)))]
macro_rules! ra_arch {
    () => {
        "other"
    };
}

/// The `RetroAchievements` client identification string (HTTP `User-Agent`).
///
/// RA's documented format (compliance requirements, section C,
/// <https://docs.retroachievements.org/general/hardcore-compliance-requirements.html>;
/// the `rc_client` guide's "User-Agent Header" section gives the same shape as
/// `<product>/<product-version> (<system-information>) <extensions>`):
///
/// ```text
/// EmulatorName/v1.0.0 (OSName 10.0) core_name/v0.5.0
/// ```
///
/// - **product**: `RustyNES/<crate version>` — required, unique, no spaces,
///   numeric and incrementing. RA recognizes the client by this token; an
///   unrecognized one has its hardcore unlocks turned into casual ones until
///   RA validates it ("contact `RAdmin`", per the guide).
/// - **system information**: `(<OS> <arch>)` from the build target (v3.4.0;
///   before it the UA had no system clause). Optional in RA's format.
/// - **extension**: `rcheevos/<version>`, the clause the guide recommends
///   (`rc_client_get_user_agent_clause`), from the vendored library via
///   `RCHEEVOS_VERSION` (emitted by `build.rs` from `rc_version.h`), so it
///   stays correct across a re-vendor. One core, so no core clause.
///
/// Result on a Linux x86-64 build: `RustyNES/3.4.0 (Linux x86_64) rcheevos/12.5.0`.
pub const RA_USER_AGENT: &str = concat!(
    "RustyNES/",
    env!("CARGO_PKG_VERSION"),
    " (",
    ra_os!(),
    " ",
    ra_arch!(),
    ") rcheevos/",
    env!("RCHEEVOS_VERSION")
);

/// The shipped [`Fetch`]: one `ureq::Agent` with RA's User-Agent and a 30 s
/// request timeout.
fn ureq_fetch() -> Fetch {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .user_agent(RA_USER_AGENT)
        // We hand rcheevos the real status code + body for 4xx/5xx, so do NOT turn
        // those into transport errors (ureq 3's `StatusCode` error drops the body).
        .http_status_as_error(false)
        .build()
        .into();
    Box::new(move |req: &Request<'_>| {
        let result = req.post.map_or_else(
            || agent.get(req.url).call(),
            |post| {
                agent
                    .post(req.url)
                    .header("Content-Type", req.content_type)
                    .send(post)
            },
        );
        // With `http_status_as_error(false)` a non-2xx response is still
        // `Ok(resp)`, so `read_response` reports the real status + body (e.g. a
        // 401/403/429 JSON body) exactly as RA wants. Only a transport error
        // (DNS, TLS, refused, timeout, ...) lands in the `Err` arm.
        result.map(read_response).map_err(|e| e.to_string())
    })
}

fn worker_loop(
    job_rx: &Receiver<HttpJob>,
    completion_tx: &Sender<HttpCompletion>,
    mut fetch: Fetch,
) {
    // Exits when the job sender is dropped (transport Drop).
    while let Ok(job) = job_rx.recv() {
        let (body, status) = perform(&mut fetch, &job);
        let _ = completion_tx.send(HttpCompletion {
            body,
            http_status_code: status,
            callback: job.callback,
            callback_data: job.callback_data,
        });
    }
}

/// Perform one exchange, returning `(body, http_status_code)` for rcheevos.
///
/// An HTTP response passes through unchanged. A transport failure becomes
/// [`crate::ffi::RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR`] with the error
/// text as the body: rcheevos then keeps an unlock or leaderboard entry queued
/// and re-sends it when the network returns (see the module docs), and shows
/// the text if a client-initiated call (login, game load) fails outright.
fn perform(fetch: &mut Fetch, job: &HttpJob) -> (Vec<u8>, i32) {
    let req = Request {
        url: &job.url,
        post: job.post.as_deref(),
        content_type: &job.content_type,
    };
    transport_response(fetch(&req))
}

/// Map one fetch outcome onto the `(body, status)` pair rcheevos receives.
fn transport_response(outcome: Result<(Vec<u8>, i32), String>) -> (Vec<u8>, i32) {
    outcome.unwrap_or_else(|e| {
        let text = if e.is_empty() {
            "network request failed".to_owned()
        } else {
            e
        };
        (
            text.into_bytes(),
            ffi::RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR,
        )
    })
}

/// Consume a ureq 3 `Response<Body>`, returning `(body_bytes, http_status_code)`.
fn read_response(mut resp: ureq::http::Response<ureq::Body>) -> (Vec<u8>, i32) {
    let status = i32::from(resp.status().as_u16());
    // RA API bodies are small JSON; ureq's 10 MB default read cap is plenty. A read
    // error hands rcheevos the status with an empty body.
    let body = resp.body_mut().read_to_vec().unwrap_or_default();
    (body, status)
}

/// The `extern "C"` server-call trampoline installed on the `rc_client`. It
/// enqueues the request onto the worker thread and returns immediately.
///
/// # Safety
/// `request` is valid for the call. `client` carries our [`crate::client::Inner`]
/// pointer via `rc_client_get_userdata`, installed in
/// [`crate::client::RaClient::new`].
pub extern "C" fn server_call_trampoline(
    request: *const ffi::rc_api_request_t,
    callback: ffi::rc_client_server_callback_t,
    callback_data: *mut c_void,
    _client: *mut ffi::rc_client_t,
) {
    let _ = std::panic::catch_unwind(|| {
        if request.is_null() {
            return;
        }
        // SAFETY: valid for this call.
        let req = unsafe { &*request };

        let url = crate::util::cstr_to_string(req.url);
        let content_type = crate::util::cstr_to_string(req.content_type);
        let post = if req.post_data.is_null() {
            None
        } else {
            // SAFETY: NUL-terminated string valid for this call.
            let cstr = unsafe { std::ffi::CStr::from_ptr(req.post_data) };
            Some(cstr.to_bytes().to_vec())
        };

        let cb_bits = callback.map_or(0, |f| f as usize);
        let data_bits = callback_data as usize;

        crate::client::with_transport(|t| {
            t.enqueue(HttpJob {
                url,
                post,
                content_type,
                callback: cb_bits,
                callback_data: data_bits,
            });
        });
    });
}

#[cfg(test)]
mod tests {

    /// v2.7.4 (MOB-04): a worker still busy is detached after the grace period
    /// instead of joined; one that finishes in time is joined.
    #[test]
    fn a_busy_worker_is_detached_not_joined() {
        let busy = std::thread::spawn(|| std::thread::sleep(std::time::Duration::from_secs(5)));
        let start = std::time::Instant::now();
        assert!(!super::join_or_detach(busy, super::DROP_GRACE));
        assert!(start.elapsed() < std::time::Duration::from_secs(1));

        let quick = std::thread::spawn(|| {});
        assert!(super::join_or_detach(quick, super::DROP_GRACE));
    }
    use super::RA_USER_AGENT;

    /// The `RetroAchievements` User-Agent must identify the client as `RustyNES`
    /// (the token RA allowlists by) with a version, and carry a non-empty
    /// canonical `rcheevos/<version>` clause. Guards against a name/version
    /// regression (e.g. a stray "`RustyNES` v2" or a dropped rcheevos version).
    #[test]
    fn ra_user_agent_identifies_rustynes_with_versions() {
        // Leading client token: `RustyNES/<version>`, version present (not bare).
        let client = RA_USER_AGENT
            .split(' ')
            .next()
            .expect("user-agent has a leading token");
        let (name, version) = client
            .split_once('/')
            .expect("client token is name/version");
        assert_eq!(name, "RustyNES", "RA identifies the client by this name");
        assert!(!version.is_empty(), "client version must be present");
        assert!(
            !RA_USER_AGENT.contains("RustyNES v2") && !RA_USER_AGENT.contains("RustyNES_v2"),
            "no stale v2 identifier"
        );

        // Canonical rcheevos clause with a non-empty version.
        let clause = RA_USER_AGENT
            .split(' ')
            .find_map(|t| t.strip_prefix("rcheevos/"))
            .expect("user-agent carries an rcheevos/<version> clause");
        assert!(!clause.is_empty(), "rcheevos version must be present");
        assert!(
            clause
                .split('.')
                .all(|p| p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty()),
            "rcheevos version is dotted digits, got {clause:?}"
        );
    }

    /// v3.4.0 — the User-Agent has RA's documented three-segment shape,
    /// `Name/version (system information) extension`, with the product token
    /// first, a numeric incrementing version, a non-empty system clause naming
    /// this build's OS, and the extension last.
    #[test]
    fn ra_user_agent_has_the_documented_three_segment_shape() {
        let (product, rest) = RA_USER_AGENT
            .split_once(' ')
            .expect("product token, then the rest");
        let version = product.strip_prefix("RustyNES/").expect("RustyNES/ first");
        assert!(
            version
                .trim_start_matches('v')
                .split('.')
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())),
            "the product version is numeric, got {version:?}"
        );
        assert_eq!(version, env!("CARGO_PKG_VERSION"), "the crate version");
        let system = rest
            .strip_prefix('(')
            .and_then(|r| r.split_once(") "))
            .map(|(sys, _)| sys)
            .expect("a parenthesised system clause after the product");
        let os = system.split(' ').next().unwrap_or_default();
        assert!(!os.is_empty(), "an OS name");
        if cfg!(target_os = "linux") {
            assert_eq!(os, "Linux");
        }
        assert!(
            RA_USER_AGENT.ends_with(concat!(" rcheevos/", env!("RCHEEVOS_VERSION"))),
            "the rcheevos extension is last: {RA_USER_AGENT}"
        );
        assert!(
            !RA_USER_AGENT.contains("  "),
            "no empty segment: {RA_USER_AGENT}"
        );
    }

    /// v3.4.0 — a transport failure (no HTTP response at all) is reported to
    /// rcheevos as RETRYABLE, with the error text as the body, so an unlock
    /// earned offline stays queued (`rc_client_should_retry` returns 1 only
    /// for this status among the client-provided ones). An HTTP response,
    /// even a 503, passes through unchanged for rcheevos to judge.
    #[test]
    fn a_transport_failure_is_reported_as_retryable_with_its_text() {
        let (body, status) = super::transport_response(Err("connection refused".into()));
        assert_eq!(
            status,
            crate::ffi::RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR
        );
        assert_eq!(body, b"connection refused");
        let (body, _) = super::transport_response(Err(String::new()));
        assert!(!body.is_empty(), "an empty error still carries text");
        let (body, status) = super::transport_response(Ok((b"busy".to_vec(), 503)));
        assert_eq!((body.as_slice(), status), (&b"busy"[..], 503));
    }
}

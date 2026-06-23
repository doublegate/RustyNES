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
//! blocks the emulator thread and never touches the rc_client). The worker
//! sends each [`HttpCompletion`] back over a channel.
//!
//! The rc_client completion callback is **never** invoked on the worker — that
//! would re-enter rcheevos from the wrong thread. Instead
//! [`HttpTransport::poll_completions`] drains the completion channel on the
//! main thread and invokes each `rc_client_server_callback_t` there, building a
//! stack [`rc_api_server_response_t`] that borrows the response bytes for the
//! duration of the call.
//!
//! The rc_client callback pointer + `callback_data` are carried as `usize` (raw
//! pointer bits) so the job is `Send`; they are only ever dereferenced back on
//! the main thread in `poll_completions`.

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
pub(crate) struct HttpTransport {
    job_tx: Option<Sender<HttpJob>>,
    completion_rx: Receiver<HttpCompletion>,
    worker: Option<JoinHandle<()>>,
}

impl HttpTransport {
    /// Spawn the worker thread with a fresh `ureq::Agent`.
    pub(crate) fn new() -> Self {
        let (job_tx, job_rx) = std::sync::mpsc::channel::<HttpJob>();
        let (completion_tx, completion_rx) = std::sync::mpsc::channel::<HttpCompletion>();

        let worker = std::thread::Builder::new()
            .name("ra-http".into())
            .spawn(move || worker_loop(&job_rx, &completion_tx))
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
                // SAFETY: `done.callback` is the exact pointer rcheevos handed
                // to the trampoline; transmuting the bits back is sound and it
                // is invoked here on the main thread.
                unsafe {
                    std::mem::transmute::<usize, ffi::rc_client_server_callback_t>(done.callback)
                }
            };
            let callback_data = done.callback_data as *mut c_void;

            let response = ffi::rc_api_server_response_t {
                body: done.body.as_ptr() as *const std::os::raw::c_char,
                body_length: done.body.len(),
                http_status_code: done.http_status_code,
            };
            // SAFETY: `cb` is a valid rcheevos completion callback; `response`
            // borrows `done.body` which outlives this call.
            cb(&response, callback_data);
        }
    }
}

impl Drop for HttpTransport {
    fn drop(&mut self) {
        // Close the job channel so the worker loop exits, then join it.
        self.job_tx = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The RetroAchievements client identification string (HTTP `User-Agent`).
///
/// RA recognizes an emulator by the leading `<Client>/<Version>` token —
/// here `RustyNES/<crate version>`; an unrecognized client gets the "unknown
/// emulator" warning and cannot earn hardcore unlocks. Setting this is the
/// prerequisite for RA to allowlist RustyNES server-side (see docs / the
/// integration request). The canonical `rcheevos/<version>` clause is appended
/// per RA convention (RA logs it); the rcheevos version comes from the vendored
/// library via `RCHEEVOS_VERSION` (emitted by `build.rs` from `rc_version.h`),
/// so it stays correct across a re-vendor. Result, e.g.:
/// `RustyNES/1.0.0 rcheevos/12.3.0`.
pub(crate) const RA_USER_AGENT: &str = concat!(
    "RustyNES/",
    env!("CARGO_PKG_VERSION"),
    " rcheevos/",
    env!("RCHEEVOS_VERSION")
);

fn worker_loop(job_rx: &Receiver<HttpJob>, completion_tx: &Sender<HttpCompletion>) {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .user_agent(RA_USER_AGENT)
        // We hand rcheevos the real status code + body for 4xx/5xx, so do NOT turn
        // those into transport errors (ureq 3's `StatusCode` error drops the body).
        .http_status_as_error(false)
        .build()
        .into();

    // Exits when the job sender is dropped (transport Drop).
    while let Ok(job) = job_rx.recv() {
        let (body, status) = perform(&agent, &job);
        let _ = completion_tx.send(HttpCompletion {
            body,
            http_status_code: status,
            callback: job.callback,
            callback_data: job.callback_data,
        });
    }
}

/// Perform one HTTP exchange, returning `(body, http_status_code)`.
///
/// On a transport-level failure we report status
/// `RC_API_SERVER_RESPONSE_CLIENT_ERROR` (-1) with an empty body, which
/// rcheevos treats as a non-retryable client error.
fn perform(agent: &ureq::Agent, job: &HttpJob) -> (Vec<u8>, i32) {
    let result = if let Some(post) = &job.post {
        agent
            .post(&job.url)
            .header("Content-Type", &job.content_type)
            .send(post.as_slice())
    } else {
        agent.get(&job.url).call()
    };

    // With `http_status_as_error(false)` a non-2xx response is still `Ok(resp)`, so
    // `read_response` reports the real status + body (e.g. a 401/403/429 JSON body)
    // exactly as RA wants. Only a transport error (DNS, TLS, refused, timeout, ...)
    // lands in the `Err` arm.
    match result {
        Ok(resp) => read_response(resp),
        Err(_) => (Vec::new(), -1),
    }
}

/// Consume a ureq 3 `Response<Body>`, returning `(body_bytes, http_status_code)`.
fn read_response(mut resp: ureq::http::Response<ureq::Body>) -> (Vec<u8>, i32) {
    let status = i32::from(resp.status().as_u16());
    // RA API bodies are small JSON; ureq's 10 MB default read cap is plenty. A read
    // error hands rcheevos the status with an empty body.
    let body = resp.body_mut().read_to_vec().unwrap_or_default();
    (body, status)
}

/// The `extern "C"` server-call trampoline installed on the rc_client. It
/// enqueues the request onto the worker thread and returns immediately.
///
/// # Safety
/// `request` is valid for the call. `client` carries our [`crate::client::Inner`]
/// pointer via `rc_client_get_userdata`, installed in
/// [`crate::client::RaClient::new`].
pub(crate) extern "C" fn server_call_trampoline(
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

        let cb_bits = callback as usize;
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
    use super::RA_USER_AGENT;

    /// The RetroAchievements User-Agent must identify the client as `RustyNES`
    /// (the token RA allowlists by) with a version, and carry a non-empty
    /// canonical `rcheevos/<version>` clause. Guards against a name/version
    /// regression (e.g. a stray "RustyNES v2" or a dropped rcheevos version).
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
}

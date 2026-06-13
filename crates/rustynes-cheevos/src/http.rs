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

fn worker_loop(job_rx: &Receiver<HttpJob>, completion_tx: &Sender<HttpCompletion>) {
    // Identify this client to the RetroAchievements server. RA recognizes an
    // emulator by the leading `<Client>/<Version>` token of the User-Agent; an
    // unrecognized client gets the "unknown emulator" warning and cannot earn
    // hardcore unlocks. Setting this is the prerequisite for RA to allowlist
    // RustyNES server-side (see docs / the integration request). The rcheevos
    // version is appended for completeness (RA logs it).
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("RustyNES/", env!("CARGO_PKG_VERSION"), " rcheevos"))
        .build();

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
            .set("Content-Type", &job.content_type)
            .send_bytes(post)
    } else {
        agent.get(&job.url).call()
    };

    match result {
        Ok(resp) => read_response(resp),
        // ureq surfaces non-2xx as Error::Status(code, response). RA wants the
        // real HTTP status code + body for those (e.g. a 401/403/429 JSON body).
        Err(ureq::Error::Status(code, resp)) => {
            let (body, _) = read_response(resp);
            (body, code as i32)
        }
        // Transport error (DNS, TLS, connection refused, timeout, ...).
        Err(_) => (Vec::new(), -1),
    }
}

/// Consume a `ureq::Response`, returning `(body_bytes, http_status_code)`.
fn read_response(resp: ureq::Response) -> (Vec<u8>, i32) {
    let status = resp.status() as i32;
    let mut body = Vec::new();
    // Reading the body should never fail the whole exchange; on a read error we
    // hand rcheevos the status with an empty body.
    if std::io::copy(&mut resp.into_reader(), &mut body).is_err() {
        body.clear();
    }
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

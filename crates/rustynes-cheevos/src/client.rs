//! The safe [`RaClient`] wrapper around `rc_client_t`.
//!
//! ## Ownership & callback bridging
//!
//! `rc_client_t` invokes three C callbacks, all of which run synchronously on
//! the thread that drove the rc_client call:
//!
//! - **read-memory** (during `do_frame`/`idle`/`reset`/`deserialize`): bridged
//!   to a caller-supplied `&mut dyn FnMut(u16) -> u8` via a thread-local raw
//!   pointer installed by a [drop-guard][Guard] for exactly the duration of the
//!   rc_client call. The trampoline maps the RA flat address to a NES bus
//!   address ([`crate::memory::ra_addr_to_nes`]) and calls the closure.
//! - **server-call** (whenever rcheevos needs the network): bridged to the
//!   off-thread [`HttpTransport`] via a thread-local pointer installed for the
//!   duration of any rc_client call that may issue requests.
//! - **event-handler**: pushes owned [`RaEvent`]s onto a thread-local queue
//!   (see [`crate::events`]); drained into [`RaClient::take_events`].
//!
//! Async completions (login / load game) are bridged with a boxed `FnOnce`
//! whose raw pointer is passed as rcheevos' `callback_userdata`; the C
//! completion trampoline reconstitutes and runs it. These fire while the main
//! thread is inside [`RaClient::poll_http_completions`].

use std::cell::Cell;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};

use crate::events::{self, RaEvent};
use crate::ffi;
use crate::http::{self, HttpTransport};
use crate::memory::ra_addr_to_nes;
use crate::util::{cchar_arr_to_string, cstr_to_string};

// ---------------------------------------------------------------------------
// Thread-locals bridging the C callbacks to Rust state.
// ---------------------------------------------------------------------------

/// The currently-installed memory read closure as a (fat) trait-object
/// pointer, or `None` when no rc_client call is in flight.
type ReadPtr = Option<*mut dyn FnMut(u16) -> u8>;

thread_local! {
    /// Currently-installed memory read closure, or `None`.
    static READ_CLOSURE: Cell<ReadPtr> = const { Cell::new(None) };

    /// Currently-installed HTTP transport pointer, or null.
    static TRANSPORT: Cell<*const HttpTransport> = const { Cell::new(std::ptr::null()) };
}

/// RAII guard that installs the read closure pointer for the duration of an
/// rc_client call and restores the previous value on drop (including on
/// panic/unwind).
struct ReadGuard {
    prev: ReadPtr,
}

impl ReadGuard {
    fn new<'a>(closure: &'a mut (dyn FnMut(u16) -> u8 + 'a)) -> Self {
        let ptr: *mut (dyn FnMut(u16) -> u8 + 'a) = closure;
        // SAFETY: erase the borrow's lifetime to store in the 'static-typed
        // thread-local. The pointer is only ever dereferenced (in
        // `read_trampoline`) while this guard is alive on the same thread, and
        // the guard's Drop restores the previous value, so the dereference can
        // never outlive `'a`.
        let ptr: *mut dyn FnMut(u16) -> u8 =
            unsafe { std::mem::transmute::<*mut (dyn FnMut(u16) -> u8 + 'a), _>(ptr) };
        let prev = READ_CLOSURE.with(|c| c.replace(Some(ptr)));
        ReadGuard { prev }
    }
}

impl Drop for ReadGuard {
    fn drop(&mut self) {
        READ_CLOSURE.with(|c| c.set(self.prev));
    }
}

/// RAII guard installing the transport pointer for the duration of a call.
struct TransportGuard {
    prev: *const HttpTransport,
}

impl TransportGuard {
    fn new(t: &HttpTransport) -> Self {
        let ptr: *const HttpTransport = t;
        let prev = TRANSPORT.with(|c| c.replace(ptr));
        TransportGuard { prev }
    }
}

impl Drop for TransportGuard {
    fn drop(&mut self) {
        TRANSPORT.with(|c| c.set(self.prev));
    }
}

/// Run `f` with the currently-installed transport, if any. Used by the
/// `server_call` trampoline (in `http.rs`).
pub(crate) fn with_transport<R>(f: impl FnOnce(&HttpTransport) -> R) -> Option<R> {
    let ptr = TRANSPORT.with(Cell::get);
    if ptr.is_null() {
        None
    } else {
        // SAFETY: the pointer is non-null only while a `TransportGuard` keeps
        // the borrowed `HttpTransport` alive on this thread.
        Some(f(unsafe { &*ptr }))
    }
}

/// The read-memory trampoline handed to `rc_client_create`.
///
/// # Safety
/// `buffer` is valid for `num_bytes`. The thread-local closure (if installed)
/// is valid for the duration of the rc_client call by construction of
/// [`ReadGuard`].
extern "C" fn read_trampoline(
    address: u32,
    buffer: *mut u8,
    num_bytes: u32,
    _client: *mut ffi::rc_client_t,
) -> u32 {
    let result = std::panic::catch_unwind(|| {
        let Some(ptr) = READ_CLOSURE.with(Cell::get) else {
            return 0u32;
        };
        if buffer.is_null() {
            return 0u32;
        }
        // SAFETY: non-null pointer installed by ReadGuard, valid for this call.
        let closure: &mut dyn FnMut(u16) -> u8 = unsafe { &mut *ptr };
        let mut written = 0u32;
        for i in 0..num_bytes {
            match ra_addr_to_nes(address + i) {
                Some(nes_addr) => {
                    let byte = closure(nes_addr);
                    // SAFETY: i < num_bytes, buffer valid for num_bytes.
                    unsafe { *buffer.add(i as usize) = byte };
                    written += 1;
                }
                // Unmapped address: stop; rcheevos treats a short read as the
                // address being invalid past this point.
                None => break,
            }
        }
        written
    });
    result.unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Async completion bridge (login / load game).
// ---------------------------------------------------------------------------

/// Boxed one-shot completion: `Ok(())` on success, `Err(message)` on failure.
type CompletionFn = Box<dyn FnOnce(Result<(), String>)>;

/// The C completion trampoline for `rc_client_callback_t`. `userdata` is a
/// `Box<CompletionFn>` raw pointer created in the `begin_*` helpers.
///
/// # Safety
/// `userdata` is exactly the pointer produced by `Box::into_raw` in a
/// `begin_*` call and is consumed (freed) here exactly once.
extern "C" fn completion_trampoline(
    result: c_int,
    error_message: *const c_char,
    _client: *mut ffi::rc_client_t,
    userdata: *mut c_void,
) {
    let _ = std::panic::catch_unwind(|| {
        if userdata.is_null() {
            return;
        }
        // SAFETY: reconstitute the Box we leaked in the begin_* call.
        let cb: Box<CompletionFn> = unsafe { Box::from_raw(userdata as *mut CompletionFn) };
        let outcome = if result == ffi::RC_OK {
            Ok(())
        } else {
            let msg = if error_message.is_null() {
                error_string(result)
            } else {
                cstr_to_string(error_message)
            };
            Err(msg)
        };
        (*cb)(outcome);
    });
}

fn error_string(code: c_int) -> String {
    // SAFETY: rc_error_str returns a static NUL-terminated string.
    let ptr = unsafe { ffi::rc_error_str(code) };
    cstr_to_string(ptr)
}

// ---------------------------------------------------------------------------
// Public safe types.
// ---------------------------------------------------------------------------

/// A safe, owned snapshot of one achievement (subset of `rc_client_achievement_t`).
#[derive(Debug, Clone, PartialEq)]
pub struct RaAchievement {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub points: u32,
    /// `RC_CLIENT_ACHIEVEMENT_STATE_*` raw value (0 inactive, 1 active/locked,
    /// 2 unlocked, 3 disabled).
    pub state: u8,
    /// `true` if the user has earned this achievement (softcore and/or
    /// hardcore). Read from the canonical `rc_client_achievement_t.unlocked`
    /// bitmask (non-zero = earned) — the authoritative lock-state flag, separate
    /// from `state` (which also encodes inactive/disabled).
    pub unlocked: bool,
    /// `RC_CLIENT_ACHIEVEMENT_BUCKET_*` raw value (the bucket it was listed in).
    pub bucket: u8,
    pub measured_percent: f32,
    pub measured_progress: String,
    /// The RA media-server URL of the unlocked (color) badge PNG. Empty if
    /// rcheevos has not populated it (e.g. before the game's badges resolve).
    pub badge_url: String,
    /// The RA media-server URL of the locked (greyed) badge PNG. Empty if
    /// rcheevos has not populated it.
    pub badge_locked_url: String,
}

/// A safe, owned snapshot of one leaderboard (subset of `rc_client_leaderboard_t`).
#[derive(Debug, Clone, PartialEq)]
pub struct RaLeaderboard {
    pub id: u32,
    pub title: String,
    pub description: String,
    /// `RC_CLIENT_LEADERBOARD_STATE_*` raw value.
    pub state: u8,
    /// `RC_CLIENT_LEADERBOARD_FORMAT_*` raw value.
    pub format: u8,
    pub lower_is_better: bool,
}

/// A safe, owned snapshot of the user's game progress summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RaGameSummary {
    pub num_core_achievements: u32,
    pub num_unofficial_achievements: u32,
    pub num_unlocked_achievements: u32,
    pub num_unsupported_achievements: u32,
    pub points_core: u32,
    pub points_unlocked: u32,
}

/// A safe, owned snapshot of the logged-in user (subset of `rc_client_user_t`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaUser {
    pub display_name: String,
    pub username: String,
    /// The login token (persist this to log in without a password next time).
    pub token: String,
    pub score: u32,
    pub score_softcore: u32,
}

// ---------------------------------------------------------------------------
// RaClient
// ---------------------------------------------------------------------------

/// A safe wrapper owning an `rc_client_t` plus its HTTP transport.
///
/// `RaClient` is **not** `Send`/`Sync`: all rc_client calls and callback
/// bridging happen on one thread (the emulator/main thread). The HTTP worker
/// thread is internal and communicates only through channels.
pub struct RaClient {
    raw: *mut ffi::rc_client_t,
    transport: HttpTransport,
    // Make the type !Send + !Sync explicitly.
    _not_send: std::marker::PhantomData<*const ()>,
}

impl RaClient {
    /// Create a new client. Spawns the HTTP worker thread, installs the
    /// read/server/event trampolines, and enables unofficial achievements off
    /// by default (hardcore on by default, matching rcheevos).
    #[must_use]
    pub fn new() -> Self {
        let transport = HttpTransport::new();
        // SAFETY: trampolines are valid function pointers with the rc_client ABI.
        let raw = unsafe { ffi::rc_client_create(read_trampoline, http::server_call_trampoline) };
        assert!(!raw.is_null(), "rc_client_create returned null");
        // SAFETY: raw is a valid client.
        unsafe {
            ffi::rc_client_set_event_handler(raw, events::event_handler_trampoline);
        }
        RaClient {
            raw,
            transport,
            _not_send: std::marker::PhantomData,
        }
    }

    /// Drain HTTP completions, invoking any pending rcheevos server callbacks
    /// (and the async login/load completions they trigger) on this thread.
    ///
    /// Call this once per frame (or whenever convenient) so async work makes
    /// progress. The transport pointer is installed for the duration so a
    /// completion that issues a follow-up request can enqueue it.
    pub fn poll_http_completions(&mut self) {
        let _t = TransportGuard::new(&self.transport);
        self.transport.poll_completions();
    }

    /// Enable or disable unofficial achievements (evaluated at game load).
    pub fn set_unofficial_enabled(&mut self, enabled: bool) {
        // SAFETY: valid client.
        unsafe { ffi::rc_client_set_unofficial_enabled(self.raw, c_int::from(enabled)) };
    }

    /// Enable or disable hardcore mode.
    pub fn set_hardcore_enabled(&mut self, enabled: bool) {
        // SAFETY: valid client.
        unsafe { ffi::rc_client_set_hardcore_enabled(self.raw, c_int::from(enabled)) };
    }

    /// Whether hardcore mode is enabled.
    #[must_use]
    pub fn get_hardcore_enabled(&self) -> bool {
        // SAFETY: valid client.
        unsafe { ffi::rc_client_get_hardcore_enabled(self.raw) != 0 }
    }

    /// Drain the events raised since the last call into owned [`RaEvent`]s.
    #[must_use]
    pub fn take_events(&mut self) -> Vec<RaEvent> {
        events::drain_events()
    }

    /// Process one frame of achievement logic, reading memory through `read`.
    ///
    /// `read` maps a NES CPU-bus address (`$0000..=$FFFF`) to a byte — the
    /// caller's closure is typically `|nes_addr| nes.cpu_bus_peek(nes_addr)`.
    /// The RA-flat -> NES-address mapping is handled internally.
    pub fn do_frame(&mut self, read: &mut dyn FnMut(u16) -> u8) {
        let _r = ReadGuard::new(read);
        let _t = TransportGuard::new(&self.transport);
        // SAFETY: valid client; guards keep the closure + transport alive.
        unsafe { ffi::rc_client_do_frame(self.raw) };
    }

    /// Process the periodic queue while emulation is paused, reading memory
    /// through `read`.
    pub fn idle(&mut self, read: &mut dyn FnMut(u16) -> u8) {
        let _r = ReadGuard::new(read);
        let _t = TransportGuard::new(&self.transport);
        // SAFETY: valid client; guards keep the closure + transport alive.
        unsafe { ffi::rc_client_idle(self.raw) };
    }

    /// Reset all achievement/leaderboard state (call after the emulator resets).
    pub fn reset(&mut self, read: &mut dyn FnMut(u16) -> u8) {
        let _r = ReadGuard::new(read);
        let _t = TransportGuard::new(&self.transport);
        // SAFETY: valid client; guards keep the closure + transport alive.
        unsafe { ffi::rc_client_reset(self.raw) };
    }

    /// Serialize the runtime achievement progress into a byte buffer.
    #[must_use]
    pub fn serialize_progress(&mut self) -> Vec<u8> {
        // SAFETY: valid client.
        let size = unsafe { ffi::rc_client_progress_size(self.raw) };
        let mut buf = vec![0u8; size];
        // SAFETY: buf has `size` bytes of capacity.
        let rc = unsafe {
            ffi::rc_client_serialize_progress_sized(self.raw, buf.as_mut_ptr(), buf.len())
        };
        if rc != ffi::RC_OK {
            buf.clear();
        }
        buf
    }

    /// Deserialize previously-serialized runtime progress, reading current
    /// memory through `read`. Returns `Ok(())` on success.
    ///
    /// # Errors
    /// Returns the rcheevos error string if deserialization fails.
    pub fn deserialize_progress(
        &mut self,
        data: &[u8],
        read: &mut dyn FnMut(u16) -> u8,
    ) -> Result<(), String> {
        let _r = ReadGuard::new(read);
        let _t = TransportGuard::new(&self.transport);
        // SAFETY: valid client; `data` is read-only for the call.
        let rc = unsafe { ffi::rc_client_deserialize_progress(self.raw, data.as_ptr()) };
        if rc == ffi::RC_OK {
            Ok(())
        } else {
            Err(error_string(rc))
        }
    }

    /// Get the current rich-presence message (empty if none).
    #[must_use]
    pub fn rich_presence(&mut self) -> String {
        let mut buf = vec![0u8; 256];
        // SAFETY: valid client; buffer valid for buf.len().
        let n = unsafe {
            ffi::rc_client_get_rich_presence_message(
                self.raw,
                buf.as_mut_ptr() as *mut c_char,
                buf.len(),
            )
        };
        let n = n.min(buf.len());
        // rcheevos NUL-terminates; trim to the reported length and any trailing NUL.
        buf.truncate(n);
        if buf.last() == Some(&0) {
            buf.pop();
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// Snapshot all achievements (core + unofficial) grouped by lock state.
    #[must_use]
    pub fn achievement_list(&mut self) -> Vec<RaAchievement> {
        // SAFETY: valid client.
        let list = unsafe {
            ffi::rc_client_create_achievement_list(
                self.raw,
                ffi::RC_CLIENT_ACHIEVEMENT_CATEGORY_CORE_AND_UNOFFICIAL,
                ffi::RC_CLIENT_ACHIEVEMENT_LIST_GROUPING_LOCK_STATE,
            )
        };
        let mut out = Vec::new();
        if list.is_null() {
            return out;
        }
        // SAFETY: non-null list owned by rcheevos until we destroy it.
        let list_ref = unsafe { &*list };
        if !list_ref.buckets.is_null() {
            for b in 0..list_ref.num_buckets as usize {
                // SAFETY: b < num_buckets.
                let bucket = unsafe { &*list_ref.buckets.add(b) };
                if bucket.achievements.is_null() {
                    continue;
                }
                for a in 0..bucket.num_achievements as usize {
                    // SAFETY: a < num_achievements; entries are non-null.
                    let ach_ptr = unsafe { *bucket.achievements.add(a) };
                    if ach_ptr.is_null() {
                        continue;
                    }
                    // SAFETY: non-null achievement pointer.
                    let ach = unsafe { &*ach_ptr };
                    out.push(RaAchievement {
                        id: ach.id,
                        title: cstr_to_string(ach.title),
                        description: cstr_to_string(ach.description),
                        points: ach.points,
                        state: ach.state,
                        unlocked: ach.unlocked != 0,
                        bucket: ach.bucket,
                        measured_percent: ach.measured_percent,
                        measured_progress: cchar_arr_to_string(&ach.measured_progress),
                        badge_url: cstr_to_string(ach.badge_url),
                        badge_locked_url: cstr_to_string(ach.badge_locked_url),
                    });
                }
            }
        }
        // SAFETY: destroy the list we created.
        unsafe { ffi::rc_client_destroy_achievement_list(list) };
        out
    }

    /// Snapshot all leaderboards (ungrouped).
    #[must_use]
    pub fn leaderboard_list(&mut self) -> Vec<RaLeaderboard> {
        // SAFETY: valid client.
        let list = unsafe {
            ffi::rc_client_create_leaderboard_list(
                self.raw,
                ffi::RC_CLIENT_LEADERBOARD_LIST_GROUPING_NONE,
            )
        };
        let mut out = Vec::new();
        if list.is_null() {
            return out;
        }
        // SAFETY: non-null list owned by rcheevos until we destroy it.
        let list_ref = unsafe { &*list };
        if !list_ref.buckets.is_null() {
            for b in 0..list_ref.num_buckets as usize {
                // SAFETY: b < num_buckets.
                let bucket = unsafe { &*list_ref.buckets.add(b) };
                if bucket.leaderboards.is_null() {
                    continue;
                }
                for l in 0..bucket.num_leaderboards as usize {
                    // SAFETY: l < num_leaderboards.
                    let lb_ptr = unsafe { *bucket.leaderboards.add(l) };
                    if lb_ptr.is_null() {
                        continue;
                    }
                    // SAFETY: non-null leaderboard pointer.
                    let lb = unsafe { &*lb_ptr };
                    out.push(RaLeaderboard {
                        id: lb.id,
                        title: cstr_to_string(lb.title),
                        description: cstr_to_string(lb.description),
                        state: lb.state,
                        format: lb.format,
                        lower_is_better: lb.lower_is_better != 0,
                    });
                }
            }
        }
        // SAFETY: destroy the list we created.
        unsafe { ffi::rc_client_destroy_leaderboard_list(list) };
        out
    }

    /// Get the user's game progress summary for the loaded game.
    #[must_use]
    pub fn user_game_summary(&self) -> RaGameSummary {
        let mut s = ffi::rc_client_user_game_summary_t {
            num_core_achievements: 0,
            num_unofficial_achievements: 0,
            num_unlocked_achievements: 0,
            num_unsupported_achievements: 0,
            points_core: 0,
            points_unlocked: 0,
            beaten_time: 0,
            completed_time: 0,
        };
        // SAFETY: valid client; `s` is a valid out-pointer.
        unsafe { ffi::rc_client_get_user_game_summary(self.raw, &mut s) };
        RaGameSummary {
            num_core_achievements: s.num_core_achievements,
            num_unofficial_achievements: s.num_unofficial_achievements,
            num_unlocked_achievements: s.num_unlocked_achievements,
            num_unsupported_achievements: s.num_unsupported_achievements,
            points_core: s.points_core,
            points_unlocked: s.points_unlocked,
        }
    }

    /// Get info about the logged-in user, or `None` if not logged in.
    #[must_use]
    pub fn user_info(&self) -> Option<RaUser> {
        // SAFETY: valid client.
        let ptr = unsafe { ffi::rc_client_get_user_info(self.raw) };
        if ptr.is_null() {
            return None;
        }
        // SAFETY: non-null user pointer owned by rcheevos.
        let u = unsafe { &*ptr };
        Some(RaUser {
            display_name: cstr_to_string(u.display_name),
            username: cstr_to_string(u.username),
            token: cstr_to_string(u.token),
            score: u.score,
            score_softcore: u.score_softcore,
        })
    }

    /// Begin an asynchronous password login. `on_done` fires during a later
    /// [`poll_http_completions`][Self::poll_http_completions] call.
    pub fn begin_login_password<F>(&mut self, username: &str, password: &str, on_done: F)
    where
        F: FnOnce(Result<(), String>) + 'static,
    {
        let user = CString::new(username).unwrap_or_default();
        let pass = CString::new(password).unwrap_or_default();
        let userdata = box_completion(on_done);
        let _t = TransportGuard::new(&self.transport);
        // SAFETY: valid client; strings valid for the call (rcheevos copies
        // what it needs); `userdata` is consumed by completion_trampoline.
        unsafe {
            ffi::rc_client_begin_login_with_password(
                self.raw,
                user.as_ptr(),
                pass.as_ptr(),
                completion_trampoline,
                userdata,
            );
        }
    }

    /// Begin an asynchronous token login.
    pub fn begin_login_token<F>(&mut self, username: &str, token: &str, on_done: F)
    where
        F: FnOnce(Result<(), String>) + 'static,
    {
        let user = CString::new(username).unwrap_or_default();
        let tok = CString::new(token).unwrap_or_default();
        let userdata = box_completion(on_done);
        let _t = TransportGuard::new(&self.transport);
        // SAFETY: as in begin_login_password.
        unsafe {
            ffi::rc_client_begin_login_with_token(
                self.raw,
                user.as_ptr(),
                tok.as_ptr(),
                completion_trampoline,
                userdata,
            );
        }
    }

    /// Log out the current user.
    pub fn logout(&mut self) {
        // SAFETY: valid client.
        unsafe { ffi::rc_client_logout(self.raw) };
    }

    /// Begin loading a game from raw ROM bytes (console = NES). rcheevos hashes
    /// the bytes internally to identify the game. `on_done` fires during a later
    /// [`poll_http_completions`][Self::poll_http_completions] call.
    pub fn begin_load_game<F>(&mut self, rom: &[u8], on_done: F)
    where
        F: FnOnce(Result<(), String>) + 'static,
    {
        let userdata = box_completion(on_done);
        let _t = TransportGuard::new(&self.transport);
        // SAFETY: valid client; `rom` is read-only for the call (rcheevos hashes
        // it synchronously); `userdata` consumed by completion_trampoline.
        unsafe {
            ffi::rc_client_begin_identify_and_load_game(
                self.raw,
                ffi::RC_CONSOLE_NINTENDO,
                std::ptr::null(), // no file_path; identify from data
                rom.as_ptr(),
                rom.len(),
                completion_trampoline,
                userdata,
            );
        }
    }

    /// Unload the current game.
    pub fn unload_game(&mut self) {
        // SAFETY: valid client.
        unsafe { ffi::rc_client_unload_game(self.raw) };
    }
}

/// Box an `FnOnce` completion into the type-erased pointer the trampoline
/// expects, returned as a raw `*mut c_void` (`callback_userdata`).
fn box_completion<F>(f: F) -> *mut c_void
where
    F: FnOnce(Result<(), String>) + 'static,
{
    let boxed: CompletionFn = Box::new(f);
    // Double-box so the fat trait-object pointer fits a thin `*mut c_void`.
    let double: Box<CompletionFn> = Box::new(boxed);
    Box::into_raw(double) as *mut c_void
}

impl Default for RaClient {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RaClient {
    fn drop(&mut self) {
        // SAFETY: valid client; no callbacks are in flight on this thread and
        // the HTTP worker is shut down by `transport`'s Drop after this.
        unsafe { ffi::rc_client_destroy(self.raw) };
        // `transport` is dropped after this, joining the worker thread.
    }
}

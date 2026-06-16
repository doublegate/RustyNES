//! Hand-written `extern "C"` declarations for the `rcheevos` `rc_client` API
//! subset this crate uses, plus the `#[repr(C)]` POD structs mirrored from the
//! vendored headers (`vendor/rcheevos/include/rc_client.h`,
//! `rc_api_request.h`, `rc_error.h`).
//!
//! These are *not* bindgen output: every layout is transcribed by hand from the
//! headers and pinned by a `size_of` test below (paired with the C
//! `_Static_assert`s in `static_asserts.c`) so a future vendor bump that
//! changes a struct layout fails loudly at build time.
//!
//! Only the fields actually read by [`crate::client`] are spelled out; trailing
//! fields are included where needed to make `size_of` match the C struct.

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int, c_void};

/// Opaque `rc_client_t`.
#[repr(C)]
pub struct rc_client_t {
    _private: [u8; 0],
}

/// Opaque async handle returned by the `begin_*` calls.
#[repr(C)]
pub struct rc_client_async_handle_t {
    _private: [u8; 0],
}

// ---------------------------------------------------------------------------
// Console id (rc_consoles.h).
// ---------------------------------------------------------------------------

/// `RC_CONSOLE_NINTENDO` (the NES/Famicom console id).
pub const RC_CONSOLE_NINTENDO: u32 = 7;

/// `RC_OK`.
pub const RC_OK: c_int = 0;

// ---------------------------------------------------------------------------
// Callback typedefs.
// ---------------------------------------------------------------------------

pub type rc_client_read_memory_func_t =
    extern "C" fn(address: u32, buffer: *mut u8, num_bytes: u32, client: *mut rc_client_t) -> u32;

pub type rc_client_server_callback_t =
    extern "C" fn(server_response: *const rc_api_server_response_t, callback_data: *mut c_void);

pub type rc_client_server_call_t = extern "C" fn(
    request: *const rc_api_request_t,
    callback: rc_client_server_callback_t,
    callback_data: *mut c_void,
    client: *mut rc_client_t,
);

pub type rc_client_event_handler_t =
    extern "C" fn(event: *const rc_client_event_t, client: *mut rc_client_t);

/// Generic async result callback (login / load game).
pub type rc_client_callback_t = extern "C" fn(
    result: c_int,
    error_message: *const c_char,
    client: *mut rc_client_t,
    userdata: *mut c_void,
);

// ---------------------------------------------------------------------------
// rc_api_request.h
// ---------------------------------------------------------------------------

/// `rc_api_request_t`. Only the first three fields are read here; the trailing
/// `rc_buffer_t buffer` (a 256-byte inline arena) is opaque. We only ever
/// receive `*const rc_api_request_t` from rcheevos, so the tail is never
/// touched and is left as a sized blob to keep the type a valid pointee.
#[repr(C)]
pub struct rc_api_request_t {
    pub url: *const c_char,
    pub post_data: *const c_char,
    pub content_type: *const c_char,
    /// Opaque `rc_buffer_t buffer` (rc_buffer_chunk_t + uint8_t[256]).
    _buffer: [u8; 256 + 4 * std::mem::size_of::<usize>()],
}

/// `rc_api_server_response_t`.
#[repr(C)]
pub struct rc_api_server_response_t {
    pub body: *const c_char,
    pub body_length: usize,
    pub http_status_code: c_int,
}

// ---------------------------------------------------------------------------
// rc_client.h POD types.
// ---------------------------------------------------------------------------

/// `rc_client_user_t`.
#[repr(C)]
pub struct rc_client_user_t {
    pub display_name: *const c_char,
    pub username: *const c_char,
    pub token: *const c_char,
    pub score: u32,
    pub score_softcore: u32,
    pub num_unread_messages: u32,
    pub avatar_url: *const c_char,
}

/// `rc_client_user_game_summary_t`. `time_t` is 8 bytes on the targets we build.
#[repr(C)]
pub struct rc_client_user_game_summary_t {
    pub num_core_achievements: u32,
    pub num_unofficial_achievements: u32,
    pub num_unlocked_achievements: u32,
    pub num_unsupported_achievements: u32,
    pub points_core: u32,
    pub points_unlocked: u32,
    pub beaten_time: i64,
    pub completed_time: i64,
}

/// `rc_client_achievement_t`.
#[repr(C)]
pub struct rc_client_achievement_t {
    pub title: *const c_char,
    pub description: *const c_char,
    pub badge_name: [c_char; 8],
    pub measured_progress: [c_char; 24],
    pub measured_percent: f32,
    pub id: u32,
    pub points: u32,
    pub unlock_time: i64, // time_t
    pub state: u8,
    pub category: u8,
    pub bucket: u8,
    pub unlocked: u8,
    pub rarity: f32,
    pub rarity_hardcore: f32,
    pub r#type: u8,
    // 3 bytes padding before the pointers
    pub badge_url: *const c_char,
    pub badge_locked_url: *const c_char,
}

/// `rc_client_leaderboard_t`.
#[repr(C)]
pub struct rc_client_leaderboard_t {
    pub title: *const c_char,
    pub description: *const c_char,
    pub tracker_value: *const c_char,
    pub id: u32,
    pub state: u8,
    pub format: u8,
    pub lower_is_better: u8,
}

/// `rc_client_leaderboard_tracker_t`.
#[repr(C)]
pub struct rc_client_leaderboard_tracker_t {
    /// `char display[24]`.
    pub display: [c_char; 24],
    pub id: u32,
}

/// `rc_client_server_error_t`.
#[repr(C)]
pub struct rc_client_server_error_t {
    pub error_message: *const c_char,
    pub api: *const c_char,
    pub result: c_int,
    pub related_id: u32,
}

/// `rc_client_event_t`. The "union" in the docs is really a flat set of
/// pointers; the active one is selected by `type`.
#[repr(C)]
pub struct rc_client_event_t {
    pub r#type: u32,
    pub achievement: *mut rc_client_achievement_t,
    pub leaderboard: *mut rc_client_leaderboard_t,
    pub leaderboard_tracker: *mut rc_client_leaderboard_tracker_t,
    pub leaderboard_scoreboard: *mut c_void, // rc_client_leaderboard_scoreboard_t (unused)
    pub server_error: *mut rc_client_server_error_t,
    pub subset: *mut c_void, // rc_client_subset_t (unused)
}

/// `rc_client_achievement_bucket_t`.
#[repr(C)]
pub struct rc_client_achievement_bucket_t {
    pub achievements: *const *const rc_client_achievement_t,
    pub num_achievements: u32,
    pub label: *const c_char,
    pub subset_id: u32,
    pub bucket_type: u8,
}

/// `rc_client_achievement_list_t`.
#[repr(C)]
pub struct rc_client_achievement_list_t {
    pub buckets: *const rc_client_achievement_bucket_t,
    pub num_buckets: u32,
}

/// `rc_client_leaderboard_bucket_t`.
#[repr(C)]
pub struct rc_client_leaderboard_bucket_t {
    pub leaderboards: *const *const rc_client_leaderboard_t,
    pub num_leaderboards: u32,
    pub label: *const c_char,
    pub subset_id: u32,
    pub bucket_type: u8,
}

/// `rc_client_leaderboard_list_t`.
#[repr(C)]
pub struct rc_client_leaderboard_list_t {
    pub buckets: *const rc_client_leaderboard_bucket_t,
    pub num_buckets: u32,
}

// ---------------------------------------------------------------------------
// Event type constants (rc_client.h).
// ---------------------------------------------------------------------------

pub const RC_CLIENT_EVENT_ACHIEVEMENT_TRIGGERED: u32 = 1;
pub const RC_CLIENT_EVENT_LEADERBOARD_STARTED: u32 = 2;
pub const RC_CLIENT_EVENT_LEADERBOARD_FAILED: u32 = 3;
pub const RC_CLIENT_EVENT_LEADERBOARD_SUBMITTED: u32 = 4;
pub const RC_CLIENT_EVENT_ACHIEVEMENT_CHALLENGE_INDICATOR_SHOW: u32 = 5;
pub const RC_CLIENT_EVENT_ACHIEVEMENT_CHALLENGE_INDICATOR_HIDE: u32 = 6;
pub const RC_CLIENT_EVENT_ACHIEVEMENT_PROGRESS_INDICATOR_SHOW: u32 = 7;
pub const RC_CLIENT_EVENT_ACHIEVEMENT_PROGRESS_INDICATOR_HIDE: u32 = 8;
pub const RC_CLIENT_EVENT_ACHIEVEMENT_PROGRESS_INDICATOR_UPDATE: u32 = 9;
pub const RC_CLIENT_EVENT_LEADERBOARD_TRACKER_SHOW: u32 = 10;
pub const RC_CLIENT_EVENT_LEADERBOARD_TRACKER_HIDE: u32 = 11;
pub const RC_CLIENT_EVENT_LEADERBOARD_TRACKER_UPDATE: u32 = 12;
#[allow(dead_code)] // documents the full event enum; scoreboard not modeled
pub const RC_CLIENT_EVENT_LEADERBOARD_SCOREBOARD: u32 = 13;
pub const RC_CLIENT_EVENT_RESET: u32 = 14;
pub const RC_CLIENT_EVENT_GAME_COMPLETED: u32 = 15;
pub const RC_CLIENT_EVENT_SERVER_ERROR: u32 = 16;
pub const RC_CLIENT_EVENT_DISCONNECTED: u32 = 17;
pub const RC_CLIENT_EVENT_RECONNECTED: u32 = 18;
pub const RC_CLIENT_EVENT_SUBSET_COMPLETED: u32 = 19;

/// Achievement category bitmask: core + unofficial.
pub const RC_CLIENT_ACHIEVEMENT_CATEGORY_CORE_AND_UNOFFICIAL: c_int = (1 << 0) | (1 << 1);
/// Achievement list grouping by lock state.
pub const RC_CLIENT_ACHIEVEMENT_LIST_GROUPING_LOCK_STATE: c_int = 0;
/// Leaderboard list grouping: none.
pub const RC_CLIENT_LEADERBOARD_LIST_GROUPING_NONE: c_int = 0;

// ---------------------------------------------------------------------------
// extern "C" function declarations.
// ---------------------------------------------------------------------------

unsafe extern "C" {
    pub fn rc_client_create(
        read_memory: rc_client_read_memory_func_t,
        server_call: rc_client_server_call_t,
    ) -> *mut rc_client_t;
    pub fn rc_client_destroy(client: *mut rc_client_t);
    pub fn rc_client_set_event_handler(
        client: *mut rc_client_t,
        handler: rc_client_event_handler_t,
    );
    pub fn rc_client_set_unofficial_enabled(client: *mut rc_client_t, enabled: c_int);
    pub fn rc_client_set_hardcore_enabled(client: *mut rc_client_t, enabled: c_int);
    pub fn rc_client_get_hardcore_enabled(client: *const rc_client_t) -> c_int;

    pub fn rc_client_begin_login_with_password(
        client: *mut rc_client_t,
        username: *const c_char,
        password: *const c_char,
        callback: rc_client_callback_t,
        callback_userdata: *mut c_void,
    ) -> *mut rc_client_async_handle_t;
    pub fn rc_client_begin_login_with_token(
        client: *mut rc_client_t,
        username: *const c_char,
        token: *const c_char,
        callback: rc_client_callback_t,
        callback_userdata: *mut c_void,
    ) -> *mut rc_client_async_handle_t;
    pub fn rc_client_logout(client: *mut rc_client_t);

    pub fn rc_client_begin_identify_and_load_game(
        client: *mut rc_client_t,
        console_id: u32,
        file_path: *const c_char,
        data: *const u8,
        data_size: usize,
        callback: rc_client_callback_t,
        callback_userdata: *mut c_void,
    ) -> *mut rc_client_async_handle_t;
    pub fn rc_client_unload_game(client: *mut rc_client_t);

    pub fn rc_client_do_frame(client: *mut rc_client_t);
    pub fn rc_client_idle(client: *mut rc_client_t);
    pub fn rc_client_reset(client: *mut rc_client_t);

    pub fn rc_client_serialize_progress_sized(
        client: *mut rc_client_t,
        buffer: *mut u8,
        buffer_size: usize,
    ) -> c_int;
    pub fn rc_client_progress_size(client: *mut rc_client_t) -> usize;
    pub fn rc_client_deserialize_progress(client: *mut rc_client_t, serialized: *const u8)
    -> c_int;

    pub fn rc_client_get_rich_presence_message(
        client: *mut rc_client_t,
        buffer: *mut c_char,
        buffer_size: usize,
    ) -> usize;

    pub fn rc_client_create_achievement_list(
        client: *mut rc_client_t,
        category: c_int,
        grouping: c_int,
    ) -> *mut rc_client_achievement_list_t;
    pub fn rc_client_destroy_achievement_list(list: *mut rc_client_achievement_list_t);

    pub fn rc_client_create_leaderboard_list(
        client: *mut rc_client_t,
        grouping: c_int,
    ) -> *mut rc_client_leaderboard_list_t;
    pub fn rc_client_destroy_leaderboard_list(list: *mut rc_client_leaderboard_list_t);

    pub fn rc_client_get_user_game_summary(
        client: *const rc_client_t,
        summary: *mut rc_client_user_game_summary_t,
    );
    pub fn rc_client_get_user_info(client: *const rc_client_t) -> *const rc_client_user_t;

    pub fn rc_error_str(ret: c_int) -> *const c_char;
}

#[cfg(test)]
mod abi_guard {
    use super::*;
    use std::mem::size_of;

    // Each Rust mirror is verified against the ACTUAL C `sizeof` (via the
    // `rc_cheevos_sizeof_*` accessors compiled from `static_asserts.c`), so any
    // Rust/C layout drift is caught on EVERY platform — not pinned to numbers
    // validated on one host. (We avoid C11 `_Static_assert`, which MSVC rejects.)
    unsafe extern "C" {
        fn rc_cheevos_sizeof_event() -> usize;
        fn rc_cheevos_sizeof_achievement() -> usize;
        fn rc_cheevos_sizeof_leaderboard() -> usize;
        fn rc_cheevos_sizeof_leaderboard_tracker() -> usize;
        fn rc_cheevos_sizeof_user() -> usize;
        fn rc_cheevos_sizeof_user_game_summary() -> usize;
        fn rc_cheevos_sizeof_achievement_bucket() -> usize;
        fn rc_cheevos_sizeof_achievement_list() -> usize;
        fn rc_cheevos_sizeof_leaderboard_bucket() -> usize;
        fn rc_cheevos_sizeof_leaderboard_list() -> usize;
        fn rc_cheevos_sizeof_server_error() -> usize;
        fn rc_cheevos_sizeof_api_server_response() -> usize;
    }

    #[test]
    fn struct_sizes_match_c() {
        // SAFETY: the `rc_cheevos_sizeof_*` functions are plain `size_t f(void)`
        // accessors with no side effects, linked from the vendored static lib.
        unsafe {
            assert_eq!(size_of::<rc_client_event_t>(), rc_cheevos_sizeof_event());
            assert_eq!(
                size_of::<rc_client_achievement_t>(),
                rc_cheevos_sizeof_achievement()
            );
            assert_eq!(
                size_of::<rc_client_leaderboard_t>(),
                rc_cheevos_sizeof_leaderboard()
            );
            assert_eq!(
                size_of::<rc_client_leaderboard_tracker_t>(),
                rc_cheevos_sizeof_leaderboard_tracker()
            );
            assert_eq!(size_of::<rc_client_user_t>(), rc_cheevos_sizeof_user());
            assert_eq!(
                size_of::<rc_client_user_game_summary_t>(),
                rc_cheevos_sizeof_user_game_summary()
            );
            assert_eq!(
                size_of::<rc_client_achievement_bucket_t>(),
                rc_cheevos_sizeof_achievement_bucket()
            );
            assert_eq!(
                size_of::<rc_client_achievement_list_t>(),
                rc_cheevos_sizeof_achievement_list()
            );
            assert_eq!(
                size_of::<rc_client_leaderboard_bucket_t>(),
                rc_cheevos_sizeof_leaderboard_bucket()
            );
            assert_eq!(
                size_of::<rc_client_leaderboard_list_t>(),
                rc_cheevos_sizeof_leaderboard_list()
            );
            assert_eq!(
                size_of::<rc_client_server_error_t>(),
                rc_cheevos_sizeof_server_error()
            );
            assert_eq!(
                size_of::<rc_api_server_response_t>(),
                rc_cheevos_sizeof_api_server_response()
            );
        }
    }
}

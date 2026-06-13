/* ABI guard: portable size accessors for the rcheevos POD structs mirrored in
 * `src/ffi.rs`. Each function returns `sizeof` the corresponding C struct; the
 * `abi_guard` Rust tests in src/ffi.rs compare `size_of::<RustMirror>()` to
 * these, so any layout drift (a vendor bump, or a Rust/C mismatch on a given
 * platform) is caught at test time on EVERY target.
 *
 * NOTE: this deliberately does NOT use C11 `_Static_assert` — MSVC's `cl.exe`
 * (the Windows CI toolchain) does not accept it in the C mode cc-rs drives,
 * whereas `sizeof` in a plain function is portable to every C compiler. */
#include <stddef.h>
#include "rc_client.h"

size_t rc_cheevos_sizeof_event(void) { return sizeof(rc_client_event_t); }
size_t rc_cheevos_sizeof_achievement(void) { return sizeof(rc_client_achievement_t); }
size_t rc_cheevos_sizeof_leaderboard(void) { return sizeof(rc_client_leaderboard_t); }
size_t rc_cheevos_sizeof_leaderboard_tracker(void) { return sizeof(rc_client_leaderboard_tracker_t); }
size_t rc_cheevos_sizeof_user(void) { return sizeof(rc_client_user_t); }
size_t rc_cheevos_sizeof_user_game_summary(void) { return sizeof(rc_client_user_game_summary_t); }
size_t rc_cheevos_sizeof_achievement_bucket(void) { return sizeof(rc_client_achievement_bucket_t); }
size_t rc_cheevos_sizeof_achievement_list(void) { return sizeof(rc_client_achievement_list_t); }
size_t rc_cheevos_sizeof_leaderboard_bucket(void) { return sizeof(rc_client_leaderboard_bucket_t); }
size_t rc_cheevos_sizeof_leaderboard_list(void) { return sizeof(rc_client_leaderboard_list_t); }
size_t rc_cheevos_sizeof_server_error(void) { return sizeof(rc_client_server_error_t); }
size_t rc_cheevos_sizeof_api_server_response(void) { return sizeof(rc_api_server_response_t); }

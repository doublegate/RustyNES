//! Safe Rust mirror of `rc_client_event_t`, plus the thread-local queue the
//! C event-handler trampoline pushes into.
//!
//! rcheevos raises events synchronously from inside `rc_client_do_frame`,
//! `rc_client_idle`, `rc_client_reset`, and the HTTP-completion callbacks (all
//! of which run on the main thread). The trampoline copies the data it needs
//! out of the (borrowed-only-for-the-callback) C structs into owned Rust
//! values and pushes them onto a thread-local [`VecDeque`]. [`RaClient`] drains
//! the queue into a `Vec<RaEvent>` after each call.
//!
//! [`RaClient`]: crate::client::RaClient

use std::cell::RefCell;
use std::collections::VecDeque;

use crate::ffi;
use crate::util::cstr_to_string;

/// One row of a leaderboard scoreboard (a top entry shown after a submission).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaScoreboardEntry {
    /// The username for this rank.
    pub username: String,
    /// The 1-based rank of this entry.
    pub rank: u32,
    /// The formatted score string (rcheevos pre-formats it for display).
    pub score: String,
}

/// A safe, owned RetroAchievements event.
#[derive(Debug, Clone, PartialEq)]
pub enum RaEvent {
    /// An achievement was earned by the player.
    AchievementTriggered {
        id: u32,
        title: String,
        points: u32,
        /// The RA media-server URL of the unlocked (color) badge PNG (empty if
        /// rcheevos has not populated it). Used by the frontend to show the
        /// badge image in the unlock toast.
        badge_url: String,
    },
    /// A leaderboard attempt has started.
    LeaderboardStarted { id: u32, title: String },
    /// A leaderboard attempt failed.
    LeaderboardFailed { id: u32, title: String },
    /// A leaderboard attempt was submitted.
    LeaderboardSubmitted { id: u32, title: String },
    /// A challenge indicator should be shown (`true`) or hidden (`false`).
    ChallengeIndicator {
        id: u32,
        show: bool,
        badge_name: String,
    },
    /// A progress indicator should be shown/updated/hidden.
    ProgressIndicator {
        /// `Some(true)` = show, `Some(false)` = hide, `None` = update.
        show: Option<bool>,
        measured_progress: String,
    },
    /// A leaderboard tracker should be shown/updated/hidden.
    LeaderboardTracker {
        id: u32,
        /// `Some(true)` = show, `Some(false)` = hide, `None` = update.
        show: Option<bool>,
        display: String,
    },
    /// A new leaderboard ranking was received after a submission — the data
    /// for the scoreboard popup (your new rank "#N of M" + the top entries).
    LeaderboardScoreboard {
        leaderboard_id: u32,
        /// The score the player just submitted (formatted).
        submitted_score: String,
        /// The player's best submitted score (formatted).
        best_score: String,
        /// The player's new rank in the leaderboard (1-based).
        new_rank: u32,
        /// The total number of entries in the leaderboard.
        num_entries: u32,
        /// The top entries the server returned for the scoreboard.
        top_entries: Vec<RaScoreboardEntry>,
    },
    /// All achievements for the game have been earned.
    GameCompleted,
    /// All achievements for a subset have been earned.
    SubsetCompleted,
    /// The emulated system should be reset (hardcore was enabled).
    Reset,
    /// The server connection was lost; unlocks are pending.
    Disconnected,
    /// The server connection was restored; pending unlocks completed.
    Reconnected,
    /// An API response returned a non-retryable server error.
    ServerError { msg: String, api: String },
    /// An event type this wrapper does not model in detail.
    Other { event_type: u32 },
}

thread_local! {
    static EVENT_QUEUE: RefCell<VecDeque<RaEvent>> = const { RefCell::new(VecDeque::new()) };
}

/// Push a translated event onto the thread-local queue.
fn push_event(ev: RaEvent) {
    EVENT_QUEUE.with(|q| q.borrow_mut().push_back(ev));
}

/// Drain all pending events for the current thread.
pub(crate) fn drain_events() -> Vec<RaEvent> {
    EVENT_QUEUE.with(|q| q.borrow_mut().drain(..).collect())
}

/// The `extern "C"` event handler installed on the rc_client. It translates the
/// borrowed C event into an owned [`RaEvent`] and enqueues it.
///
/// # Safety
/// `event` is a valid `*const rc_client_event_t` for the duration of the call,
/// supplied by rcheevos. The union pointers are only dereferenced for the event
/// types that define them.
pub(crate) extern "C" fn event_handler_trampoline(
    event: *const ffi::rc_client_event_t,
    _client: *mut ffi::rc_client_t,
) {
    // Never unwind across the FFI boundary.
    let _ = std::panic::catch_unwind(|| {
        if event.is_null() {
            return;
        }
        // SAFETY: rcheevos guarantees `event` is valid for this call.
        let ev = unsafe { &*event };
        let translated = translate_event(ev);
        push_event(translated);
    });
}

/// # Safety
/// Caller guarantees `ev` is a valid reference and that any union pointer it
/// reads is valid for the active event type (rcheevos' contract).
fn translate_event(ev: &ffi::rc_client_event_t) -> RaEvent {
    // Helper closures to safely read the union pointers.
    let ach = || -> Option<&ffi::rc_client_achievement_t> {
        if ev.achievement.is_null() {
            None
        } else {
            // SAFETY: non-null for the achievement-bearing event types.
            Some(unsafe { &*ev.achievement })
        }
    };
    let lb = || -> Option<&ffi::rc_client_leaderboard_t> {
        if ev.leaderboard.is_null() {
            None
        } else {
            // SAFETY: non-null for the leaderboard-bearing event types.
            Some(unsafe { &*ev.leaderboard })
        }
    };
    let tracker = || -> Option<&ffi::rc_client_leaderboard_tracker_t> {
        if ev.leaderboard_tracker.is_null() {
            None
        } else {
            // SAFETY: non-null for the tracker-bearing event types.
            Some(unsafe { &*ev.leaderboard_tracker })
        }
    };
    let scoreboard = || -> Option<&ffi::rc_client_leaderboard_scoreboard_t> {
        if ev.leaderboard_scoreboard.is_null() {
            None
        } else {
            // SAFETY: non-null for the scoreboard event type.
            Some(unsafe { &*ev.leaderboard_scoreboard })
        }
    };

    match ev.r#type {
        ffi::RC_CLIENT_EVENT_ACHIEVEMENT_TRIGGERED => {
            let a = ach();
            RaEvent::AchievementTriggered {
                id: a.map_or(0, |a| a.id),
                title: a.map_or_else(String::new, |a| cstr_to_string(a.title)),
                points: a.map_or(0, |a| a.points),
                badge_url: a.map_or_else(String::new, |a| cstr_to_string(a.badge_url)),
            }
        }
        ffi::RC_CLIENT_EVENT_LEADERBOARD_STARTED => RaEvent::LeaderboardStarted {
            id: lb().map_or(0, |l| l.id),
            title: lb().map_or_else(String::new, |l| cstr_to_string(l.title)),
        },
        ffi::RC_CLIENT_EVENT_LEADERBOARD_FAILED => RaEvent::LeaderboardFailed {
            id: lb().map_or(0, |l| l.id),
            title: lb().map_or_else(String::new, |l| cstr_to_string(l.title)),
        },
        ffi::RC_CLIENT_EVENT_LEADERBOARD_SUBMITTED => RaEvent::LeaderboardSubmitted {
            id: lb().map_or(0, |l| l.id),
            title: lb().map_or_else(String::new, |l| cstr_to_string(l.title)),
        },
        ffi::RC_CLIENT_EVENT_ACHIEVEMENT_CHALLENGE_INDICATOR_SHOW
        | ffi::RC_CLIENT_EVENT_ACHIEVEMENT_CHALLENGE_INDICATOR_HIDE => {
            let show = ev.r#type == ffi::RC_CLIENT_EVENT_ACHIEVEMENT_CHALLENGE_INDICATOR_SHOW;
            let a = ach();
            RaEvent::ChallengeIndicator {
                id: a.map_or(0, |a| a.id),
                show,
                badge_name: a.map_or_else(String::new, |a| {
                    crate::util::cchar_arr_to_string(&a.badge_name)
                }),
            }
        }
        ffi::RC_CLIENT_EVENT_ACHIEVEMENT_PROGRESS_INDICATOR_SHOW
        | ffi::RC_CLIENT_EVENT_ACHIEVEMENT_PROGRESS_INDICATOR_HIDE
        | ffi::RC_CLIENT_EVENT_ACHIEVEMENT_PROGRESS_INDICATOR_UPDATE => {
            let show = match ev.r#type {
                ffi::RC_CLIENT_EVENT_ACHIEVEMENT_PROGRESS_INDICATOR_SHOW => Some(true),
                ffi::RC_CLIENT_EVENT_ACHIEVEMENT_PROGRESS_INDICATOR_HIDE => Some(false),
                _ => None,
            };
            RaEvent::ProgressIndicator {
                show,
                measured_progress: ach().map_or_else(String::new, |a| {
                    crate::util::cchar_arr_to_string(&a.measured_progress)
                }),
            }
        }
        ffi::RC_CLIENT_EVENT_LEADERBOARD_TRACKER_SHOW
        | ffi::RC_CLIENT_EVENT_LEADERBOARD_TRACKER_HIDE
        | ffi::RC_CLIENT_EVENT_LEADERBOARD_TRACKER_UPDATE => {
            let show = match ev.r#type {
                ffi::RC_CLIENT_EVENT_LEADERBOARD_TRACKER_SHOW => Some(true),
                ffi::RC_CLIENT_EVENT_LEADERBOARD_TRACKER_HIDE => Some(false),
                _ => None,
            };
            let t = tracker();
            RaEvent::LeaderboardTracker {
                id: t.map_or(0, |t| t.id),
                show,
                display: t.map_or_else(String::new, |t| {
                    crate::util::cchar_arr_to_string(&t.display)
                }),
            }
        }
        ffi::RC_CLIENT_EVENT_LEADERBOARD_SCOREBOARD => {
            let sb = scoreboard();
            let top_entries = sb.map_or_else(Vec::new, |s| {
                if s.top_entries.is_null() {
                    return Vec::new();
                }
                let n = s.num_top_entries as usize;
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    // SAFETY: i < num_top_entries; rcheevos owns the array for
                    // the duration of this callback. We copy each field out.
                    let e = unsafe { &*s.top_entries.add(i) };
                    out.push(RaScoreboardEntry {
                        username: cstr_to_string(e.username),
                        rank: e.rank,
                        score: crate::util::cchar_arr_to_string(&e.score),
                    });
                }
                out
            });
            RaEvent::LeaderboardScoreboard {
                leaderboard_id: sb.map_or(0, |s| s.leaderboard_id),
                submitted_score: sb.map_or_else(String::new, |s| {
                    crate::util::cchar_arr_to_string(&s.submitted_score)
                }),
                best_score: sb.map_or_else(String::new, |s| {
                    crate::util::cchar_arr_to_string(&s.best_score)
                }),
                new_rank: sb.map_or(0, |s| s.new_rank),
                num_entries: sb.map_or(0, |s| s.num_entries),
                top_entries,
            }
        }
        ffi::RC_CLIENT_EVENT_GAME_COMPLETED => RaEvent::GameCompleted,
        ffi::RC_CLIENT_EVENT_SUBSET_COMPLETED => RaEvent::SubsetCompleted,
        ffi::RC_CLIENT_EVENT_RESET => RaEvent::Reset,
        ffi::RC_CLIENT_EVENT_DISCONNECTED => RaEvent::Disconnected,
        ffi::RC_CLIENT_EVENT_RECONNECTED => RaEvent::Reconnected,
        ffi::RC_CLIENT_EVENT_SERVER_ERROR => {
            let se = if ev.server_error.is_null() {
                None
            } else {
                // SAFETY: non-null for the server-error event type.
                Some(unsafe { &*ev.server_error })
            };
            RaEvent::ServerError {
                msg: se.map_or_else(String::new, |e| cstr_to_string(e.error_message)),
                api: se.map_or_else(String::new, |e| cstr_to_string(e.api)),
            }
        }
        other => RaEvent::Other { event_type: other },
    }
}

//! `RetroAchievements` session state (v2.7.0, native-only, feature-gated).
//!
//! [`RaSession`] bundles the safe [`rustynes_cheevos::RaClient`] together with all
//! the frontend-side state the achievement integration needs:
//!
//! - the **login state machine** (logged-out / logging-in / logged-in / error)
//!   plus the login-dialog text buffers (username + password + token);
//! - the **hardcore** flag (mirrored from the client so the UI + gating
//!   predicate can read it without a `&mut`);
//! - the **unlock-toast queue** (transient HUD notifications) and the active
//!   **leaderboard trackers** + rich-presence string the HUD renders;
//! - the cached **achievement list** + **game summary** (refreshed when the
//!   game loads / an achievement unlocks);
//! - the **per-game progress** persistence bookkeeping (the loaded game's ROM
//!   bytes hash → `<data_dir>/ra-progress/<sha>.rap`).
//!
//! The whole module is compiled only with the `retroachievements` feature on a
//! native target (see `lib.rs`); the browser builds never see it.
//!
//! # Threading / borrow model
//!
//! `RaClient` is `!Send`/`!Sync` and all of its calls (and the C callback
//! bridging) run on the emulator/main thread — the same thread that owns the
//! `App`. The per-frame hook borrows `self.ra` and `self.nes` as *disjoint*
//! `App` fields (Rust's field-borrow splitting permits `&mut self.ra` +
//! `&mut self.nes` simultaneously), so the `do_frame(&mut |a| nes.cpu_bus_peek(a))`
//! read-closure can call into `nes` while the client is mutably borrowed.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use rustynes_cheevos::{RaAchievement, RaClient, RaEvent, RaGameSummary, RaLeaderboard, RaUser};

/// How long a transient unlock / event toast stays on the HUD.
const TOAST_TTL: Duration = Duration::from_secs(5);

/// Maximum number of toasts shown at once (oldest dropped beyond this).
const MAX_TOASTS: usize = 6;

/// The coarse login state of an [`RaSession`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LoginState {
    /// Not logged in (the login dialog accepts a username + password/token).
    #[default]
    LoggedOut,
    /// A login request is in flight (waiting on `poll_http_completions`).
    LoggingIn,
    /// Logged in successfully.
    LoggedIn,
    /// The last login attempt failed with this message.
    Error(String),
}

/// One transient HUD toast (achievement unlock, game-completed, server error).
#[derive(Clone, Debug)]
pub struct Toast {
    /// The toast headline (e.g. the achievement title).
    pub title: String,
    /// Optional secondary line (points, error detail).
    pub detail: String,
    /// Wall-clock instant the toast was created; it expires after `TOAST_TTL`.
    pub created: Instant,
    /// `true` for an error/warning toast (rendered in a warmer color).
    pub is_error: bool,
    /// v2.7.1 — for an achievement-unlock toast, the RA media-server URL of the
    /// unlocked badge PNG (empty for non-achievement toasts). The HUD draws the
    /// badge image next to the toast text when this is set.
    pub badge_url: String,
}

/// An active leaderboard tracker shown on the HUD (keyed by leaderboard id).
#[derive(Clone, Debug, Default)]
pub struct Trackers {
    /// id → current display string (e.g. a running timer / score).
    pub active: HashMap<u32, String>,
}

/// The `RetroAchievements` session bundled into the `App` as `Option<RaSession>`.
pub struct RaSession {
    /// The safe rcheevos client.
    client: RaClient,
    /// Mirror of `client.get_hardcore_enabled()` so the UI + the gating
    /// predicate can read it cheaply (kept in sync on every toggle).
    hardcore: bool,
    /// Login state machine.
    pub login: LoginState,
    /// Login-dialog username buffer.
    pub username_input: String,
    /// Login-dialog password buffer (never persisted; only the returned token
    /// is written to config).
    pub password_input: String,
    /// Transient HUD toasts (expire after `TOAST_TTL`).
    pub toasts: Vec<Toast>,
    /// Active leaderboard trackers (HUD).
    pub trackers: Trackers,
    /// The current rich-presence string (HUD), refreshed each frame.
    pub rich_presence: String,
    /// Cached achievement list (refreshed on load / unlock; the panel renders
    /// from this rather than rebuilding the list every UI frame).
    pub achievements: Vec<RaAchievement>,
    /// Cached leaderboard list.
    pub leaderboards: Vec<RaLeaderboard>,
    /// Cached game summary (points / unlock counts).
    pub summary: RaGameSummary,
    /// `true` once a game has been (successfully) loaded into the client.
    pub game_loaded: bool,
    /// SHA-256 of the loaded game's ROM bytes — the progress-sidecar key.
    game_sha256: Option<[u8; 32]>,
    /// Sidecar progress bytes loaded at `begin_load_game` time, pending until
    /// the async game-load completes (then applied in `reconcile_game_loaded`).
    pending_progress: Option<Vec<u8>>,
    /// Set `true` for one poll when a login just succeeded, so the app can
    /// (re-)identify the currently-loaded ROM (a ROM opened *before* logging in
    /// could not be identified yet — `rc_client` must be logged in first).
    /// Consumed via [`Self::take_just_logged_in`].
    just_logged_in: bool,
}

impl RaSession {
    /// Construct a session from the persisted config. Applies the configured
    /// hardcore flag and unofficial-off default; does NOT begin a login (the
    /// caller drives [`Self::auto_login`] once, to keep `new` side-effect-free
    /// w.r.t. the network).
    #[must_use]
    pub fn new(config: &crate::config::RetroAchievementsConfig) -> Self {
        let mut client = RaClient::new();
        client.set_hardcore_enabled(config.hardcore);
        client.set_unofficial_enabled(false);
        let hardcore = client.get_hardcore_enabled();
        Self {
            client,
            hardcore,
            login: LoginState::LoggedOut,
            username_input: config.username.clone(),
            password_input: String::new(),
            toasts: Vec::new(),
            trackers: Trackers::default(),
            rich_presence: String::new(),
            achievements: Vec::new(),
            leaderboards: Vec::new(),
            summary: RaGameSummary::default(),
            game_loaded: false,
            game_sha256: None,
            pending_progress: None,
            just_logged_in: false,
        }
    }

    /// Whether hardcore mode is enabled (cheap mirror; no `&mut` needed).
    #[must_use]
    pub const fn hardcore(&self) -> bool {
        self.hardcore
    }

    /// The predicate the app's gating sites consult: when this session is
    /// hardcore, the "soft" affordances (save-state load, rewind, cheats,
    /// frame-advance, RAM-watch / memory editing) are refused.
    ///
    /// Save-state SAVE and Reset/PowerCycle are NOT gated by this (they stay
    /// allowed); only the loosely-cheating affordances are.
    #[must_use]
    pub const fn hardcore_blocks(&self) -> bool {
        self.hardcore
    }

    /// Toggle hardcore mode. rcheevos resets the active achievement session on
    /// a hardcore change, so this also clears the cached lists; the caller is
    /// responsible for any emulator reset the RA `Reset` event then requests.
    pub fn set_hardcore(&mut self, enabled: bool) {
        self.client.set_hardcore_enabled(enabled);
        self.hardcore = self.client.get_hardcore_enabled();
    }

    /// Begin a username + password login (the dialog "Login" button). On
    /// success the returned token is surfaced via `Self::user_token` for the
    /// caller to persist.
    pub fn begin_login_password(&mut self, username: &str, password: &str) {
        self.login = LoginState::LoggingIn;
        // The async completion only carries Ok/Err; the resolved login state +
        // token are read back from `user_info()` during `poll` after success.
        self.client
            .begin_login_password(username, password, move |_res| {
                // The outcome is reconciled in `poll` (which checks
                // `user_info`); nothing thread-unsafe to do here.
            });
    }

    /// Begin a token login (used by [`Self::auto_login`]).
    pub fn begin_login_token(&mut self, username: &str, token: &str) {
        self.login = LoginState::LoggingIn;
        self.client
            .begin_login_token(username, token, move |_res| {});
    }

    /// Auto-login from config at startup if enabled and a token is saved.
    /// Returns `true` if a login was started.
    pub fn auto_login(&mut self, config: &crate::config::RetroAchievementsConfig) -> bool {
        if config.enabled && !config.username.is_empty() && !config.token.is_empty() {
            self.begin_login_token(&config.username, &config.token);
            true
        } else {
            false
        }
    }

    /// Log out and clear the cached per-game state.
    pub fn logout(&mut self) {
        self.client.logout();
        self.login = LoginState::LoggedOut;
        self.password_input.clear();
        self.clear_game_caches();
        self.game_loaded = false;
        self.game_sha256 = None;
    }

    /// The logged-in user info, if any.
    #[must_use]
    pub fn user_info(&self) -> Option<RaUser> {
        self.client.user_info()
    }

    /// Consume the "a login just succeeded" edge (true for exactly one poll
    /// after login completes). The app uses it to (re-)identify a ROM that was
    /// loaded before the user logged in.
    pub fn take_just_logged_in(&mut self) -> bool {
        core::mem::take(&mut self.just_logged_in)
    }

    /// The persisted login token from the current user (write this to config
    /// after a successful login). `None` if not logged in.
    #[must_use]
    pub fn user_token(&self) -> Option<String> {
        self.client.user_info().map(|u| u.token)
    }

    /// Begin loading a game from its ROM bytes. `sha256` keys the progress
    /// sidecar so a later [`Self::serialize_progress`] / load can find it.
    /// `sidecar` (if any) is the previously-saved progress blob, applied once
    /// the async game-load completes (see `reconcile_game_loaded`).
    pub fn begin_load_game(&mut self, rom: &[u8], sha256: [u8; 32], sidecar: Option<Vec<u8>>) {
        self.game_sha256 = Some(sha256);
        self.game_loaded = false;
        self.pending_progress = sidecar;
        self.clear_game_caches();
        self.client.begin_load_game(rom, move |_res| {});
    }

    /// Unload the current game (on ROM close).
    pub fn unload_game(&mut self) {
        self.client.unload_game();
        self.game_loaded = false;
        self.game_sha256 = None;
        self.pending_progress = None;
        self.clear_game_caches();
    }

    /// The loaded game's ROM-bytes SHA-256 (the progress-sidecar key).
    #[must_use]
    pub const fn game_sha256(&self) -> Option<[u8; 32]> {
        self.game_sha256
    }

    /// Serialize the runtime achievement progress (for the sidecar file).
    #[must_use]
    pub fn serialize_progress(&mut self) -> Vec<u8> {
        self.client.serialize_progress()
    }

    /// Deserialize previously-saved progress, reading current memory through
    /// `read`.
    pub fn deserialize_progress(&mut self, data: &[u8], read: &mut dyn FnMut(u16) -> u8) {
        if let Err(e) = self.client.deserialize_progress(data, read) {
            self.push_toast("RA progress load failed", &e, true);
        }
    }

    /// Drive one emulated frame of achievement logic (memory read via `read`),
    /// then drain + apply the resulting events. Returns `true` if a `Reset`
    /// event fired (the caller should `nes.reset()`).
    #[must_use]
    pub fn do_frame(&mut self, read: &mut dyn FnMut(u16) -> u8) -> bool {
        self.client.poll_http_completions();
        self.reconcile_login();
        self.reconcile_game_loaded(read);
        self.client.do_frame(read);
        self.drain_events()
    }

    /// Drive the periodic (paused / menu) queue, then drain events. Returns
    /// `true` on a `Reset` request.
    #[must_use]
    pub fn idle(&mut self, read: &mut dyn FnMut(u16) -> u8) -> bool {
        self.client.poll_http_completions();
        self.reconcile_login();
        self.reconcile_game_loaded(read);
        self.client.idle(read);
        self.drain_events()
    }

    /// Reset achievement/leaderboard state (after the emulator resets).
    pub fn reset(&mut self, read: &mut dyn FnMut(u16) -> u8) {
        self.client.reset(read);
    }

    /// Refresh the rich-presence string + cached lists/summary from the client
    /// (cheap; call once per frame for the HUD/panel).
    pub fn refresh_views(&mut self) {
        self.rich_presence = self.client.rich_presence();
    }

    /// Rebuild the cached achievement + leaderboard lists + summary. Called on
    /// game load (after the async completion) and after an unlock so the panel
    /// reflects the new lock state without rebuilding every UI frame.
    pub fn refresh_lists(&mut self) {
        self.achievements = self.client.achievement_list();
        self.leaderboards = self.client.leaderboard_list();
        self.summary = self.client.user_game_summary();
    }

    /// Expire toasts older than `TOAST_TTL` (call once per frame).
    pub fn expire_toasts(&mut self) {
        let now = Instant::now();
        self.toasts
            .retain(|t| now.duration_since(t.created) < TOAST_TTL);
    }

    // --- internal ---------------------------------------------------------

    /// Reconcile the login state machine against the client's actual user info
    /// (the async completion only carries Ok/Err, so we read the resolved
    /// state here after `poll_http_completions`).
    fn reconcile_login(&mut self) {
        if self.login == LoginState::LoggingIn
            && let Some(u) = self.client.user_info()
        {
            self.login = LoginState::LoggedIn;
            self.just_logged_in = true;
            self.password_input.clear();
            self.push_toast(
                "RetroAchievements",
                &format!("Logged in as {}", u.display_name),
                false,
            );
        }
        // If still no user_info, stay LoggingIn; a hard failure surfaces as
        // a ServerError event handled in `drain_events`.
    }

    /// Once the async game-load completes, mark the game loaded, refresh the
    /// cached lists, and apply the pending progress sidecar (if any). Detected
    /// by the client's game summary reporting achievements (a game with zero
    /// achievements is never interesting for RA so stays "unloaded" here).
    fn reconcile_game_loaded(&mut self, read: &mut dyn FnMut(u16) -> u8) {
        if self.game_loaded || self.game_sha256.is_none() {
            return;
        }
        let summary = self.client.user_game_summary();
        if summary.num_core_achievements == 0 && summary.num_unofficial_achievements == 0 {
            return; // not loaded yet (or nothing to track).
        }
        self.game_loaded = true;
        self.refresh_lists();
        if let Some(blob) = self.pending_progress.take()
            && !blob.is_empty()
        {
            if let Err(e) = self.client.deserialize_progress(&blob, read) {
                self.push_toast("RA progress load failed", &e, true);
            } else {
                // Re-snapshot the lists so the restored unlocks show.
                self.refresh_lists();
            }
        }
    }

    /// Drain rcheevos events into toasts / trackers / list-refresh. Returns
    /// `true` if a `Reset` event fired.
    fn drain_events(&mut self) -> bool {
        let mut reset_requested = false;
        let mut refresh = false;
        for ev in self.client.take_events() {
            match ev {
                RaEvent::AchievementTriggered {
                    title,
                    points,
                    badge_url,
                    ..
                } => {
                    self.push_toast_with_badge(
                        "Achievement Unlocked",
                        &format!("{title} ({points})"),
                        false,
                        badge_url,
                    );
                    refresh = true;
                }
                RaEvent::LeaderboardStarted { title, .. } => {
                    self.push_toast("Leaderboard started", &title, false);
                }
                RaEvent::LeaderboardSubmitted { title, .. } => {
                    self.push_toast("Leaderboard submitted", &title, false);
                }
                RaEvent::LeaderboardFailed { title, .. } => {
                    self.push_toast("Leaderboard failed", &title, true);
                }
                RaEvent::LeaderboardTracker { id, show, display } => match show {
                    Some(false) => {
                        self.trackers.active.remove(&id);
                    }
                    _ => {
                        self.trackers.active.insert(id, display);
                    }
                },
                RaEvent::GameCompleted => {
                    self.push_toast("Game Completed!", "All achievements unlocked", false);
                    refresh = true;
                }
                RaEvent::SubsetCompleted => {
                    self.push_toast("Subset Completed!", "", false);
                    refresh = true;
                }
                RaEvent::Reset => {
                    reset_requested = true;
                }
                RaEvent::Disconnected => {
                    self.push_toast("RetroAchievements", "Server disconnected", true);
                }
                RaEvent::Reconnected => {
                    self.push_toast("RetroAchievements", "Server reconnected", false);
                }
                RaEvent::ServerError { msg, .. } => {
                    if self.login == LoginState::LoggingIn {
                        self.login = LoginState::Error(msg.clone());
                    }
                    self.push_toast("RA server error", &msg, true);
                }
                // ChallengeIndicator / ProgressIndicator / Other: no HUD surface
                // needed for the MVP (the achievement list shows progress).
                RaEvent::ChallengeIndicator { .. }
                | RaEvent::ProgressIndicator { .. }
                | RaEvent::Other { .. } => {}
            }
        }
        if refresh {
            self.refresh_lists();
        }
        reset_requested
    }

    /// Push a transient toast, capping the queue at `MAX_TOASTS`.
    fn push_toast(&mut self, title: &str, detail: &str, is_error: bool) {
        self.push_toast_with_badge(title, detail, is_error, String::new());
    }

    /// Push a transient toast carrying an optional badge URL (v2.7.1; used for
    /// achievement-unlock toasts so the HUD can show the badge image).
    fn push_toast_with_badge(
        &mut self,
        title: &str,
        detail: &str,
        is_error: bool,
        badge_url: String,
    ) {
        self.toasts.push(Toast {
            title: title.to_string(),
            detail: detail.to_string(),
            created: Instant::now(),
            is_error,
            badge_url,
        });
        while self.toasts.len() > MAX_TOASTS {
            self.toasts.remove(0);
        }
    }

    fn clear_game_caches(&mut self) {
        self.achievements.clear();
        self.leaderboards.clear();
        self.trackers.active.clear();
        self.summary = RaGameSummary::default();
        self.rich_presence.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(
        enabled: bool,
        hardcore: bool,
        user: &str,
        token: &str,
    ) -> crate::config::RetroAchievementsConfig {
        crate::config::RetroAchievementsConfig {
            enabled,
            username: user.to_string(),
            token: token.to_string(),
            hardcore,
            host: "https://retroachievements.org".to_string(),
        }
    }

    #[test]
    fn hardcore_blocks_mirrors_flag() {
        let mut s = RaSession::new(&cfg(false, true, "", ""));
        assert!(s.hardcore());
        assert!(s.hardcore_blocks());
        s.set_hardcore(false);
        assert!(!s.hardcore());
        assert!(!s.hardcore_blocks());
        s.set_hardcore(true);
        assert!(s.hardcore_blocks());
    }

    #[test]
    fn auto_login_requires_enabled_user_and_token() {
        // Disabled → no login.
        let mut s = RaSession::new(&cfg(false, true, "bob", "tok"));
        assert!(!s.auto_login(&cfg(false, true, "bob", "tok")));
        assert_eq!(s.login, LoginState::LoggedOut);

        // Enabled but missing token → no login.
        let mut s = RaSession::new(&cfg(true, true, "bob", ""));
        assert!(!s.auto_login(&cfg(true, true, "bob", "")));

        // Enabled + user + token → login started.
        let mut s = RaSession::new(&cfg(true, true, "bob", "tok"));
        assert!(s.auto_login(&cfg(true, true, "bob", "tok")));
        assert_eq!(s.login, LoginState::LoggingIn);
    }

    #[test]
    fn toasts_expire_and_cap() {
        let mut s = RaSession::new(&cfg(false, false, "", ""));
        for i in 0..(MAX_TOASTS + 4) {
            s.push_toast("t", &format!("{i}"), false);
        }
        assert_eq!(s.toasts.len(), MAX_TOASTS);
        // Force expiry by back-dating (checked subtraction to satisfy clippy).
        let past = Instant::now()
            .checked_sub(TOAST_TTL + Duration::from_secs(1))
            .expect("instant in range");
        for t in &mut s.toasts {
            t.created = past;
        }
        s.expire_toasts();
        assert!(s.toasts.is_empty());
    }

    #[test]
    fn game_sha_round_trips() {
        let mut s = RaSession::new(&cfg(false, false, "", ""));
        assert!(s.game_sha256().is_none());
        s.begin_load_game(&[0u8; 16], [7u8; 32], None);
        assert_eq!(s.game_sha256(), Some([7u8; 32]));
        s.unload_game();
        assert!(s.game_sha256().is_none());
    }
}

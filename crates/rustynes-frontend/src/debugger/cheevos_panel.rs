#![allow(
    clippy::missing_const_for_fn,
    clippy::items_after_statements,
    clippy::too_many_lines
)]
//! `RetroAchievements` login + achievement/leaderboard panel (v2.7.0).
//!
//! Like [`netplay_panel`](super::netplay_panel), this is a **command + view
//! surface** only: it never owns the `RaSession`
//! (that lives in `App`, driven by the per-frame produce hook). Edits here emit
//! a `CheevosRequest` the app drains each pacer iteration (mirroring the
//! settings / netplay panels), and the app pushes a read-only status snapshot
//! via `CheevosPanelState::set_status` which the panel + HUD render.
//!
//! `RetroAchievements` is native-only and behind the default-OFF
//! `retroachievements` cargo feature. When the feature is OFF (or on wasm32),
//! the panel renders a single "not built" note and emits no requests — so the
//! debugger module compiles in every configuration.

/// A read-only snapshot of the RA state the app pushes into the debugger.
///
/// Pushed each pacer iteration (mirrors `NetplayStatusView` / `MovieStatus`).
/// It is a plain inert struct so the debugger module compiles with the feature
/// off / on wasm.
#[derive(Clone, Debug, Default)]
pub struct CheevosStatusView {
    /// `true` once a session exists (the feature is on + a client is live).
    pub enabled: bool,
    /// `true` while logged in.
    pub logged_in: bool,
    /// `true` while a login request is in flight.
    pub logging_in: bool,
    /// Login error text (empty if none).
    pub error: String,
    /// Display name (when logged in).
    pub display_name: String,
    /// Hardcore-mode flag.
    pub hardcore: bool,
    /// Total user score (points).
    pub score: u32,
    /// Unlocked / total achievement counts for the loaded game.
    pub unlocked: u32,
    /// Total core achievements for the loaded game.
    pub total: u32,
    /// Points unlocked / total for the loaded game.
    pub points_unlocked: u32,
    /// Points available for the loaded game.
    pub points_total: u32,
    /// Rich-presence string (HUD).
    pub rich_presence: String,
    /// Active leaderboard tracker display strings (HUD).
    pub trackers: Vec<String>,
    /// Transient toast headlines (HUD); `(title, detail, is_error, badge_url)`.
    /// `badge_url` is non-empty only for achievement-unlock toasts (v2.7.1).
    pub toasts: Vec<(String, String, bool, String)>,
    /// Cached achievement list rows: `(title, description, points, unlocked,
    /// measured_progress)`.
    pub achievements: Vec<AchievementRow>,
    /// Cached leaderboard rows: `(title, description)`.
    pub leaderboards: Vec<(String, String)>,
}

/// One achievement row for the panel list.
#[derive(Clone, Debug, Default)]
pub struct AchievementRow {
    /// Achievement title.
    pub title: String,
    /// One-line description.
    pub description: String,
    /// Point value.
    pub points: u32,
    /// `true` if the player has unlocked it.
    pub unlocked: bool,
    /// Measured-progress string (e.g. "12/50"), empty when not applicable.
    pub measured_progress: String,
    /// v2.7.1 — RA media-server URL of the unlocked (color) badge PNG. Empty
    /// until rcheevos resolves the game's badges. Used by the panel to draw the
    /// badge icon (via the debugger's badge cache); falls back to the text badge.
    pub badge_url: String,
    /// v2.7.1 — RA media-server URL of the locked (greyed) badge PNG.
    pub badge_locked_url: String,
}

impl AchievementRow {
    /// v2.7.1 — the badge URL appropriate for this row's lock state (the color
    /// badge when unlocked, the greyed badge otherwise). Empty if not resolved.
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    #[must_use]
    pub fn badge_url_for_state(&self) -> &str {
        if self.unlocked {
            &self.badge_url
        } else {
            &self.badge_locked_url
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
impl CheevosStatusView {
    /// Build a status snapshot from a live `RaSession`.
    #[must_use]
    pub fn from_session(s: &crate::ra_session::RaSession) -> Self {
        use crate::ra_session::LoginState;
        let user = s.user_info();
        let achievements = s
            .achievements
            .iter()
            .map(|a| AchievementRow {
                title: a.title.clone(),
                description: a.description.clone(),
                points: a.points,
                unlocked: a.unlocked,
                measured_progress: a.measured_progress.clone(),
                badge_url: a.badge_url.clone(),
                badge_locked_url: a.badge_locked_url.clone(),
            })
            .collect();
        let leaderboards = s
            .leaderboards
            .iter()
            .map(|l| (l.title.clone(), l.description.clone()))
            .collect();
        Self {
            enabled: true,
            logged_in: matches!(s.login, LoginState::LoggedIn),
            logging_in: matches!(s.login, LoginState::LoggingIn),
            error: match &s.login {
                LoginState::Error(e) => e.clone(),
                _ => String::new(),
            },
            display_name: user
                .as_ref()
                .map(|u| u.display_name.clone())
                .unwrap_or_default(),
            hardcore: s.hardcore(),
            score: user.as_ref().map_or(0, |u| u.score),
            unlocked: s.summary.num_unlocked_achievements,
            total: s.summary.num_core_achievements,
            points_unlocked: s.summary.points_unlocked,
            points_total: s.summary.points_core,
            rich_presence: s.rich_presence.clone(),
            trackers: s.trackers.active.values().cloned().collect(),
            toasts: s
                .toasts
                .iter()
                .map(|t| {
                    (
                        t.title.clone(),
                        t.detail.clone(),
                        t.is_error,
                        t.badge_url.clone(),
                    )
                })
                .collect(),
            achievements,
            leaderboards,
        }
    }
}

/// A request the cheevos panel emits for the app to act on. Drained by the app
/// via `CheevosPanelState::take_request` each pacer iteration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheevosRequest {
    /// Log in with a username + password (the dialog "Login" button). The app
    /// persists only the returned token.
    LoginPassword {
        /// `RetroAchievements` username.
        username: String,
        /// Password (used once for the login; never persisted).
        password: String,
    },
    /// Log out.
    Logout,
    /// Toggle hardcore mode (resets the active session).
    SetHardcore(bool),
}

/// Persistent panel state (text fields + pending request + pushed status).
// The text buffers + `seed` are only consulted by the feature-on `body`; on the
// feature-off / wasm build the panel is an informational note, so they are
// unused there (the struct is shared so the app's `set_status` / `take_request`
// surface stays target-agnostic).
#[cfg_attr(
    not(all(not(target_arch = "wasm32"), feature = "retroachievements")),
    allow(dead_code)
)]
#[derive(Debug, Default)]
pub struct CheevosPanelState {
    /// Username text buffer.
    username: String,
    /// Password text buffer (never persisted; only the login token is).
    password: String,
    /// `true` once the username has been seeded from config.
    seeded: bool,
    /// Pending request, drained by the app.
    request: Option<CheevosRequest>,
    /// Latest status pushed by the app.
    status: CheevosStatusView,
}

impl CheevosPanelState {
    /// Drain the pending request, if any.
    pub fn take_request(&mut self) -> Option<CheevosRequest> {
        self.request.take()
    }

    /// Push the latest RA status for the panel + HUD to render.
    pub fn set_status(&mut self, status: CheevosStatusView) {
        self.status = status;
    }

    /// The current status snapshot (used by the app for the toolbar HUD).
    #[must_use]
    pub fn status(&self) -> &CheevosStatusView {
        &self.status
    }

    /// Seed the username field from config once.
    #[cfg_attr(
        not(all(not(target_arch = "wasm32"), feature = "retroachievements")),
        allow(dead_code)
    )]
    fn seed(&mut self, username: &str) {
        if self.seeded {
            return;
        }
        if self.username.is_empty() {
            self.username = username.to_string();
        }
        self.seeded = true;
    }
}

/// Render the cheevos panel.
///
/// `badges` is the debugger's badge-image cache (polled by the caller before
/// this call); the panel `request`s any newly-seen badge URLs through it and
/// draws ready textures next to each achievement title.
#[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    state: &mut CheevosPanelState,
    config: &crate::config::Config,
    badges: &mut super::badge_cache::BadgeCache,
) {
    state.seed(&config.retroachievements.username);
    egui::Window::new("RetroAchievements")
        .open(open)
        .default_pos([560.0, 96.0])
        .default_size([420.0, 460.0])
        .resizable(true)
        .show(ctx, |ui| {
            body(ui, state, badges);
        });
}

/// Variant compiled when the feature is OFF (or on wasm32): an informational
/// note only, so the debugger module always has a `cheevos_panel::show`.
#[cfg(not(all(not(target_arch = "wasm32"), feature = "retroachievements")))]
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    _state: &mut CheevosPanelState,
    _config: &crate::config::Config,
) {
    egui::Window::new("RetroAchievements")
        .open(open)
        .default_pos([560.0, 96.0])
        .default_size([340.0, 130.0])
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(egui::RichText::new("RetroAchievements not built").strong());
            ui.label(
                "This build was compiled without the `retroachievements` \
                 feature (it is off by default and native-only). Rebuild with \
                 `--features retroachievements` to enable achievement tracking.",
            );
        });
}

/// v2.7.1 — draw one achievement's badge icon, falling back to the text badge
/// (`[x]` / `[ ]`) until (or unless) the image is fetched + decoded. Requests
/// the badge URL through the cache so it is fetched at most once.
#[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
fn draw_badge(ui: &mut egui::Ui, badges: &mut super::badge_cache::BadgeCache, a: &AchievementRow) {
    use super::badge_cache::BADGE_SIZE;
    let url = a.badge_url_for_state();
    if !url.is_empty() {
        badges.request(url);
        if let Some(tex) = badges.texture(url) {
            ui.add(
                egui::Image::new((tex.id(), egui::vec2(BADGE_SIZE, BADGE_SIZE)))
                    .maintain_aspect_ratio(true),
            );
            return;
        }
    }
    // No URL yet, or texture not ready: keep the colored text badge.
    let badge = if a.unlocked { "[x]" } else { "[ ]" };
    let color = if a.unlocked {
        egui::Color32::from_rgb(0x40, 0xC0, 0x40)
    } else {
        egui::Color32::GRAY
    };
    ui.colored_label(color, badge);
}

#[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
fn body(
    ui: &mut egui::Ui,
    state: &mut CheevosPanelState,
    badges: &mut super::badge_cache::BadgeCache,
) {
    let st = &state.status;

    // --- Login / header ---
    if st.logged_in {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("Logged in as {}", st.display_name)).strong());
            ui.label(format!("({} pts)", st.score));
        });
        if ui.button("Log out").clicked() {
            state.request = Some(CheevosRequest::Logout);
        }
    } else {
        ui.label(egui::RichText::new("Log in").strong());
        ui.horizontal(|ui| {
            ui.label("username:");
            ui.add(egui::TextEdit::singleline(&mut state.username).desired_width(180.0));
        });
        ui.horizontal(|ui| {
            ui.label("password:");
            ui.add(
                egui::TextEdit::singleline(&mut state.password)
                    .password(true)
                    .desired_width(180.0),
            );
        });
        if st.logging_in {
            ui.colored_label(egui::Color32::from_rgb(0xF0, 0xC0, 0x40), "Logging in...");
        } else if ui
            .add_enabled(
                !state.username.trim().is_empty() && !state.password.is_empty(),
                egui::Button::new("Login"),
            )
            .clicked()
        {
            state.request = Some(CheevosRequest::LoginPassword {
                username: state.username.trim().to_string(),
                password: std::mem::take(&mut state.password),
            });
        }
        if !st.error.is_empty() {
            ui.colored_label(
                egui::Color32::from_rgb(0xE0, 0x40, 0x40),
                format!("Login failed: {}", st.error),
            );
        }
        ui.label(
            egui::RichText::new("Only the returned login token is stored; your password is not.")
                .weak(),
        );
    }

    ui.separator();

    // --- Hardcore toggle ---
    let mut hardcore = st.hardcore;
    if ui
        .checkbox(&mut hardcore, "Hardcore mode")
        .on_hover_text(
            "Disables save-state load, rewind, cheats, frame-advance, and the \
             memory editor. Toggling resets the current achievement session.",
        )
        .changed()
    {
        state.request = Some(CheevosRequest::SetHardcore(hardcore));
    }

    ui.separator();

    // --- Game summary ---
    if st.total > 0 {
        ui.label(format!(
            "Achievements: {}/{}   Points: {}/{}",
            st.unlocked, st.total, st.points_unlocked, st.points_total
        ));
        if !st.rich_presence.is_empty() {
            ui.label(egui::RichText::new(&st.rich_presence).italics());
        }
    } else if st.logged_in {
        ui.label(
            egui::RichText::new("No achievements for the loaded game (or none loaded).").weak(),
        );
    }

    ui.separator();

    // --- Achievement list ---
    egui::CollapsingHeader::new("Achievements")
        .default_open(true)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .show(ui, |ui| {
                    if st.achievements.is_empty() {
                        ui.label(egui::RichText::new("(none)").weak());
                    }
                    for a in &st.achievements {
                        ui.horizontal(|ui| {
                            draw_badge(ui, badges, a);
                            ui.label(egui::RichText::new(&a.title).strong());
                            ui.label(format!("({})", a.points));
                        });
                        ui.label(egui::RichText::new(&a.description).weak());
                        if !a.unlocked && !a.measured_progress.is_empty() {
                            ui.label(
                                egui::RichText::new(format!("progress: {}", a.measured_progress))
                                    .weak(),
                            );
                        }
                        ui.separator();
                    }
                });
        });

    // --- Leaderboards ---
    if !st.leaderboards.is_empty() {
        egui::CollapsingHeader::new("Leaderboards")
            .default_open(false)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(120.0)
                    .show(ui, |ui| {
                        for (title, desc) in &st.leaderboards {
                            ui.label(egui::RichText::new(title).strong());
                            ui.label(egui::RichText::new(desc).weak());
                        }
                    });
            });
    }

    // v2.7.1 — achievement badge PNGs are fetched off-thread by the debugger's
    // `BadgeCache` and drawn by `draw_badge` above; rows fall back to the text
    // badge until (or unless) the image is loaded.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_request_drains() {
        let mut s = CheevosPanelState::default();
        assert!(s.take_request().is_none());
        s.request = Some(CheevosRequest::Logout);
        assert_eq!(s.take_request(), Some(CheevosRequest::Logout));
        assert!(s.take_request().is_none());
    }

    #[test]
    fn status_round_trips() {
        let mut s = CheevosPanelState::default();
        s.set_status(CheevosStatusView {
            logged_in: true,
            display_name: "bob".to_string(),
            score: 1234,
            ..CheevosStatusView::default()
        });
        assert!(s.status().logged_in);
        assert_eq!(s.status().display_name, "bob");
        assert_eq!(s.status().score, 1234);
    }

    #[test]
    fn seed_is_idempotent() {
        let mut s = CheevosPanelState::default();
        s.seed("alice");
        s.seed("other");
        assert_eq!(s.username, "alice");
    }
}

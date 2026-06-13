# Milestone 7: RetroAchievements

**Phase:** 2 (Advanced Features)
**Duration:** Months 7-8
**Status:** Planned
**Target:** August 2026
**Prerequisites:** Phase 1.5 Complete (eframe 0.33 + egui 0.33 frontend)
**Last Updated:** 2025-12-28

---

## Overview

Implement RetroAchievements integration using rcheevos FFI bindings. This milestone adds achievement detection, login system, leaderboard support, rich presence functionality, and native egui UI for achievement notifications.

## Goals

- [ ] rcheevos FFI integration
- [ ] Achievement detection logic
- [ ] UI notifications (egui toast popups)
- [ ] Login system (egui modal dialog)
- [ ] Leaderboard support
- [ ] Rich presence
- [ ] Achievement list panel (egui::Window)

## UI Integration (egui 0.33)

### Achievement Toast Notifications

Achievement unlock notifications use anchored egui::Window positioned at screen corner:

```rust
use egui::{Context, Window, Align2, Color32, Vec2};

pub struct AchievementToast {
    title: String,
    description: String,
    icon_texture: Option<egui::TextureHandle>,
    show_until: std::time::Instant,
}

impl AchievementToast {
    pub fn show(&self, ctx: &Context) {
        let remaining = self.show_until.duration_since(std::time::Instant::now());
        if remaining.as_secs() == 0 {
            return; // Toast expired
        }

        Window::new("Achievement")
            .anchor(Align2::RIGHT_TOP, Vec2::new(-20.0, 60.0))
            .collapsible(false)
            .title_bar(false)
            .resizable(false)
            .fixed_size(Vec2::new(300.0, 80.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Achievement icon
                    if let Some(ref texture) = self.icon_texture {
                        ui.image(texture);
                    }

                    ui.vertical(|ui| {
                        ui.colored_label(Color32::GOLD, "Achievement Unlocked!");
                        ui.strong(&self.title);
                        ui.label(&self.description);
                    });
                });
            });
    }
}
```

### Login Dialog

Login uses egui::Modal (new in egui 0.33):

```rust
use egui::{Context, Modal, TextEdit};

pub struct LoginState {
    username: String,
    password: String,
    remember_me: bool,
    error_message: Option<String>,
}

impl LoginState {
    pub fn show_login_modal(&mut self, ctx: &Context) -> Option<LoginAction> {
        let mut action = None;

        Modal::new("ra_login".into()).show(ctx, |ui| {
            ui.heading("RetroAchievements Login");
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Username:");
                TextEdit::singleline(&mut self.username)
                    .desired_width(200.0)
                    .show(ui);
            });

            ui.horizontal(|ui| {
                ui.label("Password:");
                TextEdit::singleline(&mut self.password)
                    .password(true)
                    .desired_width(200.0)
                    .show(ui);
            });

            ui.checkbox(&mut self.remember_me, "Remember me");

            if let Some(ref error) = self.error_message {
                ui.colored_label(egui::Color32::RED, error);
            }

            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("Login").clicked() {
                    action = Some(LoginAction::Submit);
                }
                if ui.button("Cancel").clicked() {
                    action = Some(LoginAction::Cancel);
                }
            });
        });

        action
    }
}
```

### Achievement List Panel

Achievement browser using egui::Window with ScrollArea:

```rust
use egui::{Context, Window, ScrollArea, Color32};

pub struct AchievementList {
    achievements: Vec<Achievement>,
    filter: AchievementFilter,
}

impl AchievementList {
    pub fn show(&mut self, ctx: &Context, open: &mut bool) {
        Window::new("Achievements")
            .open(open)
            .default_size([400.0, 500.0])
            .show(ctx, |ui| {
                // Filter buttons
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.filter, AchievementFilter::All, "All");
                    ui.selectable_value(&mut self.filter, AchievementFilter::Locked, "Locked");
                    ui.selectable_value(&mut self.filter, AchievementFilter::Unlocked, "Unlocked");
                });

                ui.separator();

                // Progress bar
                let unlocked = self.achievements.iter().filter(|a| a.unlocked).count();
                let total = self.achievements.len();
                ui.add(egui::ProgressBar::new(unlocked as f32 / total as f32)
                    .text(format!("{}/{} ({:.0}%)", unlocked, total,
                                  unlocked as f32 / total as f32 * 100.0)));

                ui.separator();

                // Achievement list
                ScrollArea::vertical().show(ui, |ui| {
                    for achievement in &self.achievements {
                        if self.filter.matches(achievement) {
                            self.render_achievement(ui, achievement);
                            ui.separator();
                        }
                    }
                });
            });
    }

    fn render_achievement(&self, ui: &mut egui::Ui, achievement: &Achievement) {
        ui.horizontal(|ui| {
            // Icon
            if let Some(ref texture) = achievement.icon_texture {
                ui.image(texture);
            }

            ui.vertical(|ui| {
                let color = if achievement.unlocked {
                    Color32::GOLD
                } else {
                    Color32::GRAY
                };
                ui.colored_label(color, &achievement.title);
                ui.label(&achievement.description);
                ui.small(format!("{} points", achievement.points));
            });
        });
    }
}
```

## Acceptance Criteria

- [ ] Achievements unlock correctly in 10 test games
- [ ] No false positives/negatives
- [ ] Leaderboard submissions work
- [ ] <1% performance impact
- [ ] Toast notifications display properly
- [ ] Login modal works with keyboard navigation
- [ ] Achievement list scrolls smoothly with 100+ achievements

## Dependencies

- Phase 1.5 Complete (eframe 0.33 + egui 0.33 desktop frontend)
- MVP release complete
- rcheevos-sys FFI bindings

### Crate Dependencies

```toml
# crates/rustynes-achievements/Cargo.toml (new crate)

[dependencies]
rcheevos-sys = "0.1"  # FFI bindings (version TBD)
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"  # RA API responses
reqwest = { version = "0.12", features = ["json"] }  # HTTP client
tokio = { version = "1", features = ["rt-multi-thread"] }  # Async runtime
image = "0.25"  # Achievement icon loading
```

---

## Future Planning

*Detailed tasks to be created when milestone begins.*

---

**Last Updated:** 2025-12-28
**Milestone Status:** PLANNED
**Prerequisites:** Phase 1.5 Complete (eframe 0.33 + egui 0.33 frontend)
**Next Milestone:** M8 (GGPO Netplay)

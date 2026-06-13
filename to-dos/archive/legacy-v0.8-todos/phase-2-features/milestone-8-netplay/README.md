# Milestone 8: Netplay (GGPO)

**Phase:** 2 (Advanced Features)
**Duration:** Months 7-9
**Status:** Planned
**Target:** September 2026
**Prerequisites:** Phase 1.5 Complete (eframe 0.33 + egui 0.33 frontend)
**Last Updated:** 2025-12-28

---

## Overview

Implement rollback netcode using backroll-rs (Rust GGPO port). This milestone adds multiplayer capabilities with minimal input lag and robust synchronization, along with native egui UI for lobby system, connection status, and spectator mode.

## Goals

- [ ] backroll-rs integration (Rust GGPO port)
- [ ] Save state serialization for rollback
- [ ] Input prediction/rollback
- [ ] Lobby system (egui UI)
- [ ] Spectator mode
- [ ] NAT traversal (STUN/TURN)
- [ ] Connection status panel (egui)

## UI Integration (egui 0.33)

### Lobby Dialog

Main lobby interface using egui::Window:

```rust
use egui::{Context, Window, ScrollArea, Color32, RichText};

pub struct LobbyState {
    players: Vec<PlayerInfo>,
    chat_messages: Vec<ChatMessage>,
    chat_input: String,
    room_code: String,
    connection_status: ConnectionStatus,
}

impl LobbyState {
    pub fn show(&mut self, ctx: &Context, open: &mut bool) -> Option<LobbyAction> {
        let mut action = None;

        Window::new("Netplay Lobby")
            .open(open)
            .default_size([600.0, 500.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Left panel: Players
                    ui.vertical(|ui| {
                        ui.heading("Players");
                        ui.separator();

                        for player in &self.players {
                            self.render_player(ui, player);
                        }

                        ui.separator();

                        // Room code
                        ui.horizontal(|ui| {
                            ui.label("Room Code:");
                            ui.code(&self.room_code);
                            if ui.button("Copy").clicked() {
                                ui.output_mut(|o| o.copied_text = self.room_code.clone());
                            }
                        });
                    });

                    ui.separator();

                    // Right panel: Chat
                    ui.vertical(|ui| {
                        ui.heading("Chat");
                        ui.separator();

                        ScrollArea::vertical()
                            .id_salt("chat_scroll")
                            .max_height(300.0)
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                for msg in &self.chat_messages {
                                    ui.horizontal(|ui| {
                                        ui.colored_label(msg.player_color, &msg.player_name);
                                        ui.label(": ");
                                        ui.label(&msg.text);
                                    });
                                }
                            });

                        ui.separator();

                        // Chat input
                        ui.horizontal(|ui| {
                            let response = ui.text_edit_singleline(&mut self.chat_input);
                            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                if !self.chat_input.is_empty() {
                                    action = Some(LobbyAction::SendChat(self.chat_input.clone()));
                                    self.chat_input.clear();
                                }
                            }
                            if ui.button("Send").clicked() && !self.chat_input.is_empty() {
                                action = Some(LobbyAction::SendChat(self.chat_input.clone()));
                                self.chat_input.clear();
                            }
                        });
                    });
                });

                ui.separator();

                // Bottom: Connection status and controls
                ui.horizontal(|ui| {
                    self.render_connection_status(ui);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Leave").clicked() {
                            action = Some(LobbyAction::Leave);
                        }
                        if self.is_host() {
                            if ui.button("Start Game").clicked() {
                                action = Some(LobbyAction::StartGame);
                            }
                        }
                    });
                });
            });

        action
    }

    fn render_player(&self, ui: &mut egui::Ui, player: &PlayerInfo) {
        ui.horizontal(|ui| {
            // Ready indicator
            let ready_color = if player.ready { Color32::GREEN } else { Color32::GRAY };
            ui.colored_label(ready_color, if player.ready { "●" } else { "○" });

            // Player name with host badge
            if player.is_host {
                ui.label(RichText::new(&player.name).color(Color32::GOLD));
                ui.small("(Host)");
            } else {
                ui.label(&player.name);
            }

            // Ping
            ui.small(format!("{}ms", player.ping));
        });
    }

    fn render_connection_status(&self, ui: &mut egui::Ui) {
        let (color, text) = match self.connection_status {
            ConnectionStatus::Connected => (Color32::GREEN, "Connected"),
            ConnectionStatus::Connecting => (Color32::YELLOW, "Connecting..."),
            ConnectionStatus::Disconnected => (Color32::RED, "Disconnected"),
            ConnectionStatus::Syncing => (Color32::LIGHT_BLUE, "Syncing..."),
        };
        ui.colored_label(color, format!("● {}", text));
    }
}
```

### Host/Join Modal Dialog

Using egui::Modal (egui 0.33):

```rust
use egui::{Context, Modal, TextEdit};

pub enum NetplayAction {
    Host { port: u16 },
    Join { address: String },
    Cancel,
}

pub struct NetplayModal {
    mode: NetplayMode,
    host_port: String,
    join_address: String,
}

impl NetplayModal {
    pub fn show(&mut self, ctx: &Context) -> Option<NetplayAction> {
        let mut action = None;

        Modal::new("netplay_modal".into()).show(ctx, |ui| {
            ui.heading("Netplay");
            ui.separator();

            // Mode selection
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.mode, NetplayMode::Host, "Host Game");
                ui.selectable_value(&mut self.mode, NetplayMode::Join, "Join Game");
            });

            ui.separator();

            match self.mode {
                NetplayMode::Host => {
                    ui.horizontal(|ui| {
                        ui.label("Port:");
                        TextEdit::singleline(&mut self.host_port)
                            .desired_width(80.0)
                            .show(ui);
                    });

                    if ui.button("Host").clicked() {
                        if let Ok(port) = self.host_port.parse() {
                            action = Some(NetplayAction::Host { port });
                        }
                    }
                }

                NetplayMode::Join => {
                    ui.horizontal(|ui| {
                        ui.label("Address:");
                        TextEdit::singleline(&mut self.join_address)
                            .hint_text("host:port or room code")
                            .desired_width(200.0)
                            .show(ui);
                    });

                    if ui.button("Join").clicked() {
                        action = Some(NetplayAction::Join {
                            address: self.join_address.clone(),
                        });
                    }
                }
            }

            ui.separator();

            if ui.button("Cancel").clicked() {
                action = Some(NetplayAction::Cancel);
            }
        });

        action
    }
}
```

### Connection Status Overlay

Real-time connection quality indicator:

```rust
use egui::{Context, Align2, Vec2, Color32};

pub struct ConnectionOverlay {
    ping: u32,
    rollback_frames: u8,
    local_frame: u32,
    remote_frame: u32,
}

impl ConnectionOverlay {
    pub fn show(&self, ctx: &Context) {
        egui::Window::new("Connection")
            .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-10.0, -10.0))
            .title_bar(false)
            .resizable(false)
            .show(ctx, |ui| {
                // Ping with color coding
                let ping_color = match self.ping {
                    0..=50 => Color32::GREEN,
                    51..=100 => Color32::YELLOW,
                    _ => Color32::RED,
                };
                ui.horizontal(|ui| {
                    ui.label("Ping:");
                    ui.colored_label(ping_color, format!("{}ms", self.ping));
                });

                // Rollback indicator
                if self.rollback_frames > 0 {
                    ui.colored_label(Color32::YELLOW,
                        format!("Rollback: {} frames", self.rollback_frames));
                }

                // Frame sync indicator
                let frame_diff = self.local_frame.abs_diff(self.remote_frame);
                if frame_diff > 2 {
                    ui.colored_label(Color32::RED, format!("Desync: {} frames", frame_diff));
                }
            });
    }
}
```

## Acceptance Criteria

- [ ] 1-2 frame input lag over LAN
- [ ] <5 frame rollback on 100ms ping
- [ ] No desyncs in 30-minute sessions
- [ ] Works behind typical NAT setups
- [ ] Lobby UI responsive and intuitive
- [ ] Chat works with keyboard shortcuts
- [ ] Connection status clearly visible during play

## Dependencies

- Save states functional (fast serialization)
- Deterministic emulation verified
- Phase 1.5 Complete (eframe 0.33 + egui 0.33 frontend)

### Crate Dependencies

```toml
# crates/rustynes-netplay/Cargo.toml (new crate)

[dependencies]
backroll = "0.5"  # Rust GGPO port
bincode = "2.0"   # State serialization
tokio = { version = "1", features = ["rt-multi-thread", "net"] }
webrtc = "0.8"    # NAT traversal
stun-client = "0.1"  # STUN protocol
serde = { version = "1.0", features = ["derive"] }
```

---

## Future Planning

*Detailed tasks to be created when milestone begins.*

---

**Last Updated:** 2025-12-28
**Milestone Status:** PLANNED
**Prerequisites:** Phase 1.5 Complete (eframe 0.33 + egui 0.33 frontend)
**Next Milestone:** M9 (Lua Scripting)

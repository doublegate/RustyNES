# RetroAchievements Integration Guide

Complete reference for implementing RetroAchievements support in RustyNES using the rcheevos library.

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [rcheevos Integration](#rcheevos-integration)
4. [Achievement Processing](#achievement-processing)
5. [Leaderboards](#leaderboards)
6. [Rich Presence](#rich-presence)
7. [Network API](#network-api)
8. [UI Integration](#ui-integration)
9. [Hardcore Mode](#hardcore-mode)
10. [Security Considerations](#security-considerations)
11. [Testing](#testing)
12. [References](#references)

---

## Overview

RetroAchievements (RA) is a community-driven system for adding achievements to retro games. RustyNES integrates with RA through the official rcheevos library, providing achievement tracking, leaderboards, and rich presence.

### Key Features

1. **Achievement Tracking**: Trigger achievements based on game memory conditions
2. **Leaderboards**: Compete for high scores and speedruns
3. **Rich Presence**: Display current game status
4. **Hardcore Mode**: Disable cheats and save states for verified runs
5. **Offline Support**: Queue achievements when disconnected

### Design Goals

- Official rcheevos library integration
- Minimal performance impact
- Secure credential handling
- Seamless user experience

---

## Architecture

### Component Overview

```
┌─────────────────────────────────────────────────────────────┐
│                  RetroAchievements System                    │
│                                                              │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │    RA       │    │ Achievement │    │   Leaderboard   │  │
│  │   Client    │◄──►│  Processor  │◄──►│    Manager      │  │
│  └──────┬──────┘    └──────┬──────┘    └────────┬────────┘  │
│         │                  │                    │           │
│         ▼                  ▼                    ▼           │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │   Network   │    │   Memory    │    │  Rich Presence  │  │
│  │     API     │    │   Reader    │    │    Generator    │  │
│  └─────────────┘    └─────────────┘    └─────────────────┘  │
└─────────────────────────────────────────────────────────────┘
          │                  │                    │
          ▼                  ▼                    ▼
┌─────────────────────────────────────────────────────────────┐
│                    Emulator Core                             │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │   Memory    │    │    State    │    │     Frame       │  │
│  │    Bus      │    │   Manager   │    │     Timing      │  │
│  └─────────────┘    └─────────────┘    └─────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

### Core Types

```rust
use std::collections::HashMap;

/// RetroAchievements client
pub struct RaClient {
    /// User authentication
    auth: Option<RaAuth>,

    /// Current game info
    game: Option<RaGameInfo>,

    /// Active achievements
    achievements: Vec<Achievement>,

    /// Active leaderboards
    leaderboards: Vec<Leaderboard>,

    /// Rich presence definition
    rich_presence: Option<RichPresence>,

    /// rcheevos runtime
    runtime: RcheevosRuntime,

    /// Network client
    network: RaNetworkClient,

    /// Hardcore mode enabled
    hardcore_mode: bool,

    /// Achievement queue (for offline)
    pending_unlocks: Vec<PendingUnlock>,
}

/// User authentication data
#[derive(Clone)]
pub struct RaAuth {
    pub username: String,
    pub token: String,
    pub points: u32,
    pub rank: u32,
}

/// Game information from RA
#[derive(Clone)]
pub struct RaGameInfo {
    pub id: u32,
    pub title: String,
    pub console_id: u32,
    pub image_icon: String,
    pub image_title: String,
    pub image_ingame: String,
    pub image_boxart: String,
    pub publisher: String,
    pub developer: String,
    pub genre: String,
    pub release_date: String,
    pub hash: String,
}

/// Achievement definition
#[derive(Clone)]
pub struct Achievement {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub points: u32,
    pub badge_name: String,
    pub badge_url: String,
    pub mem_addr: String,  // Achievement logic
    pub flags: AchievementFlags,
    pub state: AchievementState,
    pub unlock_time: Option<u64>,
    pub type_: AchievementType,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AchievementState {
    /// Not yet triggered
    Active,

    /// Triggered, awaiting submission
    Triggered,

    /// Submitted and confirmed
    Unlocked,

    /// Disabled (e.g., invalid)
    Disabled,
}

#[derive(Clone, Copy, Debug)]
pub enum AchievementType {
    /// Standard achievement
    Core,

    /// Unofficial/community achievement
    Unofficial,

    /// Progression achievement
    Progression,

    /// Win condition
    WinCondition,

    /// Missable achievement
    Missable,
}

bitflags::bitflags! {
    pub struct AchievementFlags: u32 {
        const ACTIVE = 3;
        const UNOFFICIAL = 5;
        const PAUSED = 0x100;
    }
}

/// Leaderboard definition
#[derive(Clone)]
pub struct Leaderboard {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub start: String,   // Start condition
    pub cancel: String,  // Cancel condition
    pub submit: String,  // Submit condition
    pub value: String,   // Value definition
    pub format: LeaderboardFormat,
    pub lower_is_better: bool,
    pub state: LeaderboardState,
    pub current_value: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LeaderboardState {
    Inactive,
    Active,
    Started,
}

#[derive(Clone, Copy, Debug)]
pub enum LeaderboardFormat {
    Score,
    Time,
    Value,
    TimeSecs,
    TimeMillis,
    TimeFrames,
}

/// Pending achievement unlock (for offline queueing)
#[derive(Clone)]
pub struct PendingUnlock {
    pub achievement_id: u32,
    pub timestamp: u64,
    pub hardcore: bool,
}
```

---

## rcheevos Integration

### FFI Bindings

```rust
// rcheevos-sys bindings
mod ffi {
    use std::os::raw::{c_char, c_int, c_uint, c_void};

    pub type rc_runtime_t = c_void;
    pub type rc_trigger_t = c_void;
    pub type rc_lboard_t = c_void;
    pub type rc_richpresence_t = c_void;

    #[repr(C)]
    pub struct rc_runtime_event_t {
        pub id: c_uint,
        pub value: c_int,
        pub event_type: c_int,
    }

    extern "C" {
        // Runtime management
        pub fn rc_runtime_alloc() -> *mut rc_runtime_t;
        pub fn rc_runtime_destroy(runtime: *mut rc_runtime_t);
        pub fn rc_runtime_reset(runtime: *mut rc_runtime_t);

        // Achievement processing
        pub fn rc_runtime_activate_achievement(
            runtime: *mut rc_runtime_t,
            id: c_uint,
            memaddr: *const c_char,
            lua: *mut c_void,
            lua_index: c_int,
        ) -> c_int;

        pub fn rc_runtime_deactivate_achievement(
            runtime: *mut rc_runtime_t,
            id: c_uint,
        ) -> c_int;

        // Leaderboard processing
        pub fn rc_runtime_activate_lboard(
            runtime: *mut rc_runtime_t,
            id: c_uint,
            memaddr: *const c_char,
            lua: *mut c_void,
            lua_index: c_int,
        ) -> c_int;

        // Rich presence
        pub fn rc_runtime_activate_richpresence(
            runtime: *mut rc_runtime_t,
            script: *const c_char,
            lua: *mut c_void,
            lua_index: c_int,
        ) -> c_int;

        pub fn rc_runtime_get_richpresence(
            runtime: *const rc_runtime_t,
            buffer: *mut c_char,
            buffer_size: usize,
        ) -> usize;

        // Frame processing
        pub fn rc_runtime_do_frame(
            runtime: *mut rc_runtime_t,
            event_handler: Option<extern "C" fn(*const rc_runtime_event_t, *mut c_void)>,
            peek: Option<extern "C" fn(c_uint, c_uint, *mut c_void) -> c_uint>,
            peek_userdata: *mut c_void,
            lua: *mut c_void,
        );
    }
}

/// Safe wrapper around rcheevos runtime
pub struct RcheevosRuntime {
    runtime: *mut ffi::rc_runtime_t,
    memory_reader: Box<dyn Fn(u32, u32) -> u32>,
}

impl RcheevosRuntime {
    pub fn new<F>(memory_reader: F) -> Self
    where
        F: Fn(u32, u32) -> u32 + 'static,
    {
        unsafe {
            let runtime = ffi::rc_runtime_alloc();
            Self {
                runtime,
                memory_reader: Box::new(memory_reader),
            }
        }
    }

    /// Activate an achievement
    pub fn activate_achievement(&mut self, id: u32, memaddr: &str) -> Result<(), RaError> {
        let c_memaddr = std::ffi::CString::new(memaddr)
            .map_err(|_| RaError::InvalidMemAddr)?;

        unsafe {
            let result = ffi::rc_runtime_activate_achievement(
                self.runtime,
                id,
                c_memaddr.as_ptr(),
                std::ptr::null_mut(),
                0,
            );

            if result == 0 {
                Ok(())
            } else {
                Err(RaError::ActivationFailed(result))
            }
        }
    }

    /// Process one frame
    pub fn do_frame(&mut self) -> Vec<RaEvent> {
        let events = std::cell::RefCell::new(Vec::new());

        extern "C" fn event_handler(event: *const ffi::rc_runtime_event_t, userdata: *mut std::ffi::c_void) {
            unsafe {
                let events = &*(userdata as *const std::cell::RefCell<Vec<RaEvent>>);
                let event = &*event;

                events.borrow_mut().push(RaEvent {
                    id: event.id,
                    value: event.value,
                    event_type: event.event_type.into(),
                });
            }
        }

        extern "C" fn peek(address: u32, num_bytes: u32, userdata: *mut std::ffi::c_void) -> u32 {
            unsafe {
                let reader = &*(userdata as *const Box<dyn Fn(u32, u32) -> u32>);
                reader(address, num_bytes)
            }
        }

        unsafe {
            let events_ptr = &events as *const _ as *mut std::ffi::c_void;
            let reader_ptr = &self.memory_reader as *const _ as *mut std::ffi::c_void;

            ffi::rc_runtime_do_frame(
                self.runtime,
                Some(event_handler),
                Some(peek),
                reader_ptr,
                std::ptr::null_mut(),
            );
        }

        events.into_inner()
    }

    /// Get rich presence string
    pub fn get_rich_presence(&self) -> String {
        let mut buffer = vec![0u8; 256];

        unsafe {
            let len = ffi::rc_runtime_get_richpresence(
                self.runtime,
                buffer.as_mut_ptr() as *mut i8,
                buffer.len(),
            );

            buffer.truncate(len);
            String::from_utf8_lossy(&buffer).to_string()
        }
    }

    /// Reset runtime state
    pub fn reset(&mut self) {
        unsafe {
            ffi::rc_runtime_reset(self.runtime);
        }
    }
}

impl Drop for RcheevosRuntime {
    fn drop(&mut self) {
        unsafe {
            ffi::rc_runtime_destroy(self.runtime);
        }
    }
}

/// Event from rcheevos
#[derive(Clone, Debug)]
pub struct RaEvent {
    pub id: u32,
    pub value: i32,
    pub event_type: RaEventType,
}

#[derive(Clone, Copy, Debug)]
pub enum RaEventType {
    AchievementTriggered,
    AchievementReset,
    AchievementPrimed,
    LeaderboardStarted,
    LeaderboardCanceled,
    LeaderboardSubmitted,
    LeaderboardUpdate,
    Unknown(i32),
}

impl From<i32> for RaEventType {
    fn from(value: i32) -> Self {
        match value {
            0 => RaEventType::AchievementTriggered,
            1 => RaEventType::LeaderboardStarted,
            2 => RaEventType::LeaderboardCanceled,
            3 => RaEventType::LeaderboardUpdate,
            4 => RaEventType::LeaderboardSubmitted,
            5 => RaEventType::AchievementReset,
            6 => RaEventType::AchievementPrimed,
            _ => RaEventType::Unknown(value),
        }
    }
}
```

### Memory Reader Integration

```rust
/// Memory reader for rcheevos
pub struct RaMemoryReader<'a> {
    bus: &'a dyn Bus,
}

impl<'a> RaMemoryReader<'a> {
    pub fn new(bus: &'a dyn Bus) -> Self {
        Self { bus }
    }

    /// Read memory for rcheevos
    pub fn read(&self, address: u32, num_bytes: u32) -> u32 {
        // rcheevos uses NES memory map:
        // $0000-$07FF: RAM
        // $8000-$FFFF: PRG-ROM (mapped)

        let addr = address as u16;

        match num_bytes {
            1 => self.bus.read(addr) as u32,
            2 => {
                let lo = self.bus.read(addr) as u32;
                let hi = self.bus.read(addr.wrapping_add(1)) as u32;
                lo | (hi << 8)
            }
            4 => {
                let b0 = self.bus.read(addr) as u32;
                let b1 = self.bus.read(addr.wrapping_add(1)) as u32;
                let b2 = self.bus.read(addr.wrapping_add(2)) as u32;
                let b3 = self.bus.read(addr.wrapping_add(3)) as u32;
                b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
            }
            _ => 0,
        }
    }
}
```

---

## Achievement Processing

### Processing Loop

```rust
impl RaClient {
    /// Process achievements for current frame
    pub fn process_frame(&mut self, bus: &dyn Bus) -> Vec<AchievementEvent> {
        if self.game.is_none() {
            return Vec::new();
        }

        let mut events = Vec::new();

        // Create memory reader
        let reader = RaMemoryReader::new(bus);

        // Process rcheevos frame
        let ra_events = self.runtime.do_frame();

        // Handle events
        for event in ra_events {
            match event.event_type {
                RaEventType::AchievementTriggered => {
                    if let Some(achievement) = self.find_achievement_mut(event.id) {
                        achievement.state = AchievementState::Triggered;

                        events.push(AchievementEvent::Unlocked {
                            id: event.id,
                            title: achievement.title.clone(),
                            description: achievement.description.clone(),
                            points: achievement.points,
                            badge_url: achievement.badge_url.clone(),
                        });

                        // Queue for submission
                        self.queue_unlock(event.id);
                    }
                }

                RaEventType::AchievementPrimed => {
                    events.push(AchievementEvent::Primed {
                        id: event.id,
                    });
                }

                RaEventType::AchievementReset => {
                    events.push(AchievementEvent::Reset {
                        id: event.id,
                    });
                }

                RaEventType::LeaderboardStarted => {
                    if let Some(lb) = self.find_leaderboard_mut(event.id) {
                        lb.state = LeaderboardState::Started;
                        events.push(AchievementEvent::LeaderboardStarted {
                            id: event.id,
                            title: lb.title.clone(),
                        });
                    }
                }

                RaEventType::LeaderboardUpdate => {
                    if let Some(lb) = self.find_leaderboard_mut(event.id) {
                        lb.current_value = event.value;
                        events.push(AchievementEvent::LeaderboardUpdate {
                            id: event.id,
                            value: event.value,
                        });
                    }
                }

                RaEventType::LeaderboardCanceled => {
                    if let Some(lb) = self.find_leaderboard_mut(event.id) {
                        lb.state = LeaderboardState::Inactive;
                        events.push(AchievementEvent::LeaderboardCanceled {
                            id: event.id,
                        });
                    }
                }

                RaEventType::LeaderboardSubmitted => {
                    if let Some(lb) = self.find_leaderboard_mut(event.id) {
                        lb.state = LeaderboardState::Inactive;
                        events.push(AchievementEvent::LeaderboardSubmitted {
                            id: event.id,
                            value: event.value,
                        });

                        // Submit to server
                        self.submit_leaderboard(event.id, event.value);
                    }
                }

                _ => {}
            }
        }

        events
    }

    fn queue_unlock(&mut self, achievement_id: u32) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        self.pending_unlocks.push(PendingUnlock {
            achievement_id,
            timestamp,
            hardcore: self.hardcore_mode,
        });

        // Try to submit immediately
        self.flush_pending_unlocks();
    }

    /// Submit pending unlocks
    pub fn flush_pending_unlocks(&mut self) {
        let unlocks = std::mem::take(&mut self.pending_unlocks);

        for unlock in unlocks {
            match self.network.submit_achievement(unlock.achievement_id, unlock.hardcore) {
                Ok(_) => {
                    // Mark as submitted
                    if let Some(achievement) = self.find_achievement_mut(unlock.achievement_id) {
                        achievement.state = AchievementState::Unlocked;
                        achievement.unlock_time = Some(unlock.timestamp);
                    }
                }
                Err(_) => {
                    // Re-queue for later
                    self.pending_unlocks.push(unlock);
                }
            }
        }
    }
}

/// Achievement events for UI
#[derive(Clone, Debug)]
pub enum AchievementEvent {
    Unlocked {
        id: u32,
        title: String,
        description: String,
        points: u32,
        badge_url: String,
    },
    Primed {
        id: u32,
    },
    Reset {
        id: u32,
    },
    LeaderboardStarted {
        id: u32,
        title: String,
    },
    LeaderboardUpdate {
        id: u32,
        value: i32,
    },
    LeaderboardCanceled {
        id: u32,
    },
    LeaderboardSubmitted {
        id: u32,
        value: i32,
    },
}
```

---

## Leaderboards

### Leaderboard Management

```rust
impl RaClient {
    /// Activate leaderboard
    pub fn activate_leaderboard(&mut self, leaderboard: &Leaderboard) -> Result<(), RaError> {
        // Construct memaddr from leaderboard definition
        let memaddr = format!(
            "STA:{}::CAN:{}::SUB:{}::VAL:{}",
            leaderboard.start,
            leaderboard.cancel,
            leaderboard.submit,
            leaderboard.value
        );

        self.runtime.activate_leaderboard(leaderboard.id, &memaddr)?;
        Ok(())
    }

    /// Submit leaderboard score
    fn submit_leaderboard(&mut self, id: u32, value: i32) {
        let leaderboard = match self.find_leaderboard(id) {
            Some(lb) => lb.clone(),
            None => return,
        };

        // Submit to network
        tokio::spawn({
            let network = self.network.clone();
            let hardcore = self.hardcore_mode;
            async move {
                let _ = network.submit_leaderboard(id, value, hardcore).await;
            }
        });
    }

    /// Get leaderboard rankings
    pub async fn get_leaderboard_rankings(&self, id: u32) -> Result<Vec<LeaderboardEntry>, RaError> {
        self.network.get_leaderboard_entries(id).await
    }
}

/// Leaderboard entry
#[derive(Clone, Debug)]
pub struct LeaderboardEntry {
    pub rank: u32,
    pub user: String,
    pub score: i32,
    pub formatted_score: String,
    pub date_submitted: String,
}

/// Format leaderboard value for display
pub fn format_leaderboard_value(value: i32, format: LeaderboardFormat) -> String {
    match format {
        LeaderboardFormat::Score => format!("{}", value),
        LeaderboardFormat::Value => format!("{}", value),
        LeaderboardFormat::Time => {
            // Frames to MM:SS.ms
            let frames = value as f64;
            let seconds = frames / 60.0;
            let minutes = (seconds / 60.0) as i32;
            let secs = seconds % 60.0;
            format!("{:02}:{:05.2}", minutes, secs)
        }
        LeaderboardFormat::TimeSecs => {
            let seconds = value;
            let minutes = seconds / 60;
            let secs = seconds % 60;
            format!("{:02}:{:02}", minutes, secs)
        }
        LeaderboardFormat::TimeMillis => {
            let millis = value;
            let seconds = millis / 1000;
            let ms = millis % 1000;
            let minutes = seconds / 60;
            let secs = seconds % 60;
            format!("{:02}:{:02}.{:03}", minutes, secs, ms)
        }
        LeaderboardFormat::TimeFrames => {
            let frames = value;
            let seconds = frames / 60;
            let f = frames % 60;
            let minutes = seconds / 60;
            let secs = seconds % 60;
            format!("{:02}:{:02}:{:02}", minutes, secs, f)
        }
    }
}
```

---

## Rich Presence

### Rich Presence Processing

```rust
/// Rich presence definition
#[derive(Clone)]
pub struct RichPresence {
    pub script: String,
}

impl RaClient {
    /// Activate rich presence script
    pub fn activate_rich_presence(&mut self, script: &str) -> Result<(), RaError> {
        self.runtime.activate_rich_presence(script)?;
        self.rich_presence = Some(RichPresence {
            script: script.to_string(),
        });
        Ok(())
    }

    /// Get current rich presence string
    pub fn get_rich_presence(&self) -> String {
        self.runtime.get_rich_presence()
    }

    /// Update rich presence on server
    pub async fn update_rich_presence(&self) -> Result<(), RaError> {
        if let Some(ref auth) = self.auth {
            let presence = self.get_rich_presence();
            self.network.ping(&auth.username, &auth.token, &presence).await
        } else {
            Err(RaError::NotAuthenticated)
        }
    }
}
```

### Example Rich Presence Script

```
Format:Lives
FormatType=VALUE

Format:Stage
FormatType=VALUE

Lookup:Mode
0=Playing
1=Demo
2=Game Over

Display:
@Mode(0xE0)==Game Over
@Mode(0xE0) Stage @Stage(0xF0) - @Lives(0x76) Lives
```

---

## Network API

### API Client

```rust
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct RaNetworkClient {
    client: Client,
    base_url: String,
}

impl RaNetworkClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            base_url: "https://retroachievements.org/dorequest.php".to_string(),
        }
    }

    /// Authenticate user
    pub async fn login(&self, username: &str, password: &str) -> Result<RaAuth, RaError> {
        #[derive(Deserialize)]
        struct LoginResponse {
            #[serde(rename = "Success")]
            success: bool,
            #[serde(rename = "Token")]
            token: Option<String>,
            #[serde(rename = "Score")]
            score: Option<u32>,
            #[serde(rename = "Messages")]
            messages: Option<u32>,
            #[serde(rename = "Error")]
            error: Option<String>,
        }

        let response: LoginResponse = self.client
            .get(&self.base_url)
            .query(&[
                ("r", "login"),
                ("u", username),
                ("p", password),
            ])
            .send()
            .await?
            .json()
            .await?;

        if response.success {
            Ok(RaAuth {
                username: username.to_string(),
                token: response.token.ok_or(RaError::AuthFailed)?,
                points: response.score.unwrap_or(0),
                rank: 0,
            })
        } else {
            Err(RaError::AuthFailed)
        }
    }

    /// Get game info and achievements
    pub async fn get_game_info(&self, game_id: u32, auth: &RaAuth) -> Result<RaGameData, RaError> {
        #[derive(Deserialize)]
        struct GameResponse {
            #[serde(rename = "ID")]
            id: u32,
            #[serde(rename = "Title")]
            title: String,
            #[serde(rename = "Achievements")]
            achievements: HashMap<String, AchievementData>,
            #[serde(rename = "Leaderboards")]
            leaderboards: Option<Vec<LeaderboardData>>,
            #[serde(rename = "RichPresencePatch")]
            rich_presence: Option<String>,
        }

        let response: GameResponse = self.client
            .get(&self.base_url)
            .query(&[
                ("r", "patch"),
                ("u", &auth.username),
                ("t", &auth.token),
                ("g", &game_id.to_string()),
            ])
            .send()
            .await?
            .json()
            .await?;

        // Convert to internal types
        let achievements = response.achievements
            .into_iter()
            .map(|(_, data)| data.into())
            .collect();

        let leaderboards = response.leaderboards
            .unwrap_or_default()
            .into_iter()
            .map(|data| data.into())
            .collect();

        Ok(RaGameData {
            info: RaGameInfo {
                id: response.id,
                title: response.title,
                ..Default::default()
            },
            achievements,
            leaderboards,
            rich_presence: response.rich_presence,
        })
    }

    /// Submit achievement unlock
    pub async fn submit_achievement(&self, achievement_id: u32, hardcore: bool, auth: &RaAuth) -> Result<(), RaError> {
        let response = self.client
            .get(&self.base_url)
            .query(&[
                ("r", "awardachievement"),
                ("u", &auth.username),
                ("t", &auth.token),
                ("a", &achievement_id.to_string()),
                ("h", if hardcore { "1" } else { "0" }),
            ])
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(RaError::SubmissionFailed)
        }
    }

    /// Submit leaderboard score
    pub async fn submit_leaderboard(&self, id: u32, score: i32, hardcore: bool, auth: &RaAuth) -> Result<(), RaError> {
        let response = self.client
            .get(&self.base_url)
            .query(&[
                ("r", "submitlbentry"),
                ("u", &auth.username),
                ("t", &auth.token),
                ("i", &id.to_string()),
                ("s", &score.to_string()),
            ])
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(RaError::SubmissionFailed)
        }
    }

    /// Send ping with rich presence
    pub async fn ping(&self, username: &str, token: &str, rich_presence: &str) -> Result<(), RaError> {
        self.client
            .get(&self.base_url)
            .query(&[
                ("r", "ping"),
                ("u", username),
                ("t", token),
                ("m", rich_presence),
            ])
            .send()
            .await?;

        Ok(())
    }

    /// Identify game by hash
    pub async fn identify_game(&self, hash: &str) -> Result<u32, RaError> {
        #[derive(Deserialize)]
        struct IdentifyResponse {
            #[serde(rename = "GameID")]
            game_id: u32,
        }

        let response: IdentifyResponse = self.client
            .get(&self.base_url)
            .query(&[
                ("r", "gameid"),
                ("m", hash),
            ])
            .send()
            .await?
            .json()
            .await?;

        Ok(response.game_id)
    }
}

/// Combined game data
pub struct RaGameData {
    pub info: RaGameInfo,
    pub achievements: Vec<Achievement>,
    pub leaderboards: Vec<Leaderboard>,
    pub rich_presence: Option<String>,
}
```

---

## UI Integration

### Achievement Notification

```rust
/// Achievement notification for display
pub struct AchievementNotification {
    pub achievement: Achievement,
    pub display_time: f32,
    pub elapsed: f32,
    pub badge_texture: Option<TextureHandle>,
}

impl AchievementNotification {
    pub fn new(achievement: Achievement) -> Self {
        Self {
            achievement,
            display_time: 5.0, // 5 seconds
            elapsed: 0.0,
            badge_texture: None,
        }
    }

    pub fn update(&mut self, dt: f32) -> bool {
        self.elapsed += dt;
        self.elapsed < self.display_time
    }

    pub fn progress(&self) -> f32 {
        (self.elapsed / self.display_time).min(1.0)
    }
}

/// Achievement overlay renderer
pub struct AchievementOverlay {
    notifications: Vec<AchievementNotification>,
    badge_cache: HashMap<String, TextureHandle>,
}

impl AchievementOverlay {
    pub fn push(&mut self, notification: AchievementNotification) {
        self.notifications.push(notification);
    }

    pub fn update(&mut self, dt: f32) {
        self.notifications.retain_mut(|n| n.update(dt));
    }

    pub fn render(&self, ctx: &egui::Context) {
        for (i, notification) in self.notifications.iter().enumerate() {
            let y_offset = i as f32 * 80.0;

            egui::Window::new(format!("achievement_{}", notification.achievement.id))
                .fixed_pos(egui::pos2(10.0, 10.0 + y_offset))
                .fixed_size(egui::vec2(300.0, 70.0))
                .title_bar(false)
                .frame(egui::Frame::popup(ctx.style().as_ref()))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        // Badge image
                        if let Some(ref texture) = notification.badge_texture {
                            ui.image(texture, egui::vec2(64.0, 64.0));
                        }

                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new("Achievement Unlocked!")
                                .color(egui::Color32::GOLD)
                                .strong());
                            ui.label(&notification.achievement.title);
                            ui.label(format!("{} points", notification.achievement.points));
                        });
                    });
                });
        }
    }
}
```

### Achievement List UI

```rust
/// Achievement list panel
pub fn render_achievement_list(ui: &mut egui::Ui, client: &RaClient) {
    ui.heading("Achievements");

    let (unlocked, total): (u32, u32) = client.achievements.iter().fold((0, 0), |(u, t), a| {
        if a.state == AchievementState::Unlocked {
            (u + 1, t + 1)
        } else {
            (u, t + 1)
        }
    });

    ui.label(format!("Progress: {} / {} ({:.0}%)",
        unlocked, total,
        (unlocked as f32 / total as f32) * 100.0
    ));

    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        for achievement in &client.achievements {
            ui.horizontal(|ui| {
                // Status indicator
                let color = match achievement.state {
                    AchievementState::Unlocked => egui::Color32::GREEN,
                    AchievementState::Triggered => egui::Color32::YELLOW,
                    _ => egui::Color32::GRAY,
                };
                ui.colored_label(color, "●");

                // Achievement info
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(&achievement.title).strong());
                    ui.label(&achievement.description);
                    ui.label(format!("{} points", achievement.points));
                });
            });

            ui.separator();
        }
    });
}
```

---

## Hardcore Mode

### Hardcore Mode Management

```rust
impl RaClient {
    /// Enable hardcore mode
    pub fn enable_hardcore(&mut self) {
        self.hardcore_mode = true;

        // Reset all achievements to revalidate
        self.runtime.reset();

        // Reactivate all achievements
        for achievement in &self.achievements {
            let _ = self.runtime.activate_achievement(
                achievement.id,
                &achievement.mem_addr,
            );
        }
    }

    /// Disable hardcore mode
    pub fn disable_hardcore(&mut self) {
        self.hardcore_mode = false;
    }

    /// Check if an action would disable hardcore mode
    pub fn would_disable_hardcore(&self, action: &EmulatorAction) -> bool {
        if !self.hardcore_mode {
            return false;
        }

        matches!(action,
            EmulatorAction::LoadState |
            EmulatorAction::CheatEnable |
            EmulatorAction::Rewind |
            EmulatorAction::FrameAdvance |
            EmulatorAction::SlowMotion
        )
    }

    /// Confirm hardcore mode violation
    pub fn confirm_hardcore_violation(&mut self, action: EmulatorAction) -> HardcoreConfirmation {
        if self.would_disable_hardcore(&action) {
            HardcoreConfirmation::RequiresConfirmation {
                action,
                message: format!(
                    "This action will disable Hardcore Mode. \
                    You will not earn achievements until you reset the game. \
                    Continue?"
                ),
            }
        } else {
            HardcoreConfirmation::Allowed
        }
    }
}

pub enum HardcoreConfirmation {
    Allowed,
    RequiresConfirmation {
        action: EmulatorAction,
        message: String,
    },
}

#[derive(Clone, Debug)]
pub enum EmulatorAction {
    LoadState,
    CheatEnable,
    Rewind,
    FrameAdvance,
    SlowMotion,
}
```

---

## Security Considerations

### Credential Storage

```rust
use keyring::Entry;

pub struct CredentialStore {
    service_name: &'static str,
}

impl CredentialStore {
    pub fn new() -> Self {
        Self {
            service_name: "RustyNES-RetroAchievements",
        }
    }

    /// Store credentials securely
    pub fn store(&self, username: &str, token: &str) -> Result<(), CredentialError> {
        let entry = Entry::new(self.service_name, username)?;
        entry.set_password(token)?;
        Ok(())
    }

    /// Retrieve stored credentials
    pub fn retrieve(&self, username: &str) -> Result<String, CredentialError> {
        let entry = Entry::new(self.service_name, username)?;
        entry.get_password().map_err(Into::into)
    }

    /// Delete stored credentials
    pub fn delete(&self, username: &str) -> Result<(), CredentialError> {
        let entry = Entry::new(self.service_name, username)?;
        entry.delete_credential()?;
        Ok(())
    }
}
```

### Anti-Cheat Measures

```rust
impl RaClient {
    /// Validate memory read integrity
    fn validate_memory_read(&self, address: u32, value: u32) -> bool {
        // Ensure reads are within valid NES memory space
        if address > 0xFFFF {
            return false;
        }

        // Additional validation can be added here
        true
    }

    /// Detect potential tampering
    fn detect_tampering(&self) -> bool {
        // Check for debugger
        // Check for memory modification tools
        // Validate rcheevos library integrity
        false
    }
}
```

---

## Testing

### Test Utilities

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Mock memory for testing
    struct MockBus {
        ram: [u8; 2048],
    }

    impl MockBus {
        fn new() -> Self {
            Self { ram: [0; 2048] }
        }

        fn set(&mut self, addr: u16, value: u8) {
            if addr < 0x800 {
                self.ram[addr as usize] = value;
            }
        }
    }

    impl Bus for MockBus {
        fn read(&self, addr: u16) -> u8 {
            if addr < 0x800 {
                self.ram[addr as usize]
            } else {
                0
            }
        }

        fn write(&mut self, addr: u16, data: u8) {
            self.set(addr, data);
        }
    }

    #[test]
    fn test_achievement_trigger() {
        let mut bus = MockBus::new();
        let mut client = RaClient::new_test();

        // Add test achievement: trigger when address 0x76 equals 0
        client.add_test_achievement(Achievement {
            id: 1,
            title: "Test".to_string(),
            mem_addr: "0xH0076=0".to_string(),
            ..Default::default()
        });

        // Set initial value
        bus.set(0x76, 5);

        // Process frame - should not trigger
        let events = client.process_frame(&bus);
        assert!(events.is_empty());

        // Set trigger value
        bus.set(0x76, 0);

        // Process frame - should trigger
        let events = client.process_frame(&bus);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], AchievementEvent::Unlocked { id: 1, .. }));
    }
}
```

---

## References

### Related Documentation

- [Memory Addressing](../bus/MEMORY_MAP.md)
- [Save State Format](../api/SAVESTATE_FORMAT.md)
- [Configuration](../api/CONFIGURATION.md)

### External Resources

- [RetroAchievements](https://retroachievements.org/)
- [rcheevos GitHub](https://github.com/RetroAchievements/rcheevos)
- [RA Developer Documentation](https://docs.retroachievements.org/)

### Source Files

```
crates/rustynes-achievements/
├── src/
│   ├── lib.rs           # Module exports
│   ├── client.rs        # RaClient implementation
│   ├── network.rs       # API client
│   ├── runtime.rs       # rcheevos FFI wrapper
│   ├── achievement.rs   # Achievement processing
│   ├── leaderboard.rs   # Leaderboard support
│   ├── rich_presence.rs # Rich presence
│   ├── hardcore.rs      # Hardcore mode
│   └── ui.rs            # UI integration
├── rcheevos-sys/        # FFI bindings crate
│   ├── build.rs
│   └── src/lib.rs
└── tests/
    └── integration.rs
```

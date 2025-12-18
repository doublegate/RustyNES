# RustyNES Netplay Protocol Reference

Complete technical specification for the `rustynes-netplay` crate, implementing GGPO-style rollback netcode for low-latency online multiplayer using the backroll-rs library.

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Quick Start](#quick-start)
4. [Session Management](#session-management)
5. [Input Handling](#input-handling)
6. [State Synchronization](#state-synchronization)
7. [Network Protocol](#network-protocol)
8. [Rollback System](#rollback-system)
9. [Spectator Mode](#spectator-mode)
10. [Matchmaking](#matchmaking)
11. [Error Handling](#error-handling)
12. [Performance Tuning](#performance-tuning)
13. [Security Considerations](#security-considerations)
14. [Debugging](#debugging)
15. [Examples](#examples)
16. [References](#references)

---

## Overview

RustyNES implements rollback-based netcode for online multiplayer, providing a responsive gaming experience even with significant network latency. The system is built on backroll-rs, a Rust implementation of the GGPO (Good Game Peace Out) algorithm.

### Key Features

- **Rollback Netcode**: Predict opponent inputs, rollback and resimulate on misprediction
- **Input Delay Hiding**: Local inputs feel instant regardless of network conditions
- **Spectator Support**: Watch games with minimal delay
- **P2P and Relay**: Direct peer-to-peer or relay server connections
- **State Synchronization**: Automatic desync detection and recovery
- **Matchmaking Integration**: Lobby system and match finding

### Design Goals

1. **Low Latency**: Sub-frame input responsiveness
2. **Consistency**: Guaranteed synchronized game state
3. **Resilience**: Handle packet loss and jitter gracefully
4. **Simplicity**: Clean API for frontend integration

### Dependencies

```toml
[dependencies]
backroll = "0.6"
tokio = { version = "1", features = ["net", "rt-multi-thread", "sync"] }
serde = { version = "1", features = ["derive"] }
bincode = "1.3"
```

---

## Architecture

### Component Overview

```
┌─────────────────────────────────────────────────────────┐
│                    Application Layer                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
│  │   Desktop   │  │     Web     │  │   Matchmaking   │  │
│  │  Frontend   │  │  Frontend   │  │     Client      │  │
│  └──────┬──────┘  └──────┬──────┘  └────────┬────────┘  │
└─────────┼────────────────┼──────────────────┼───────────┘
          │                │                  │
┌─────────┴────────────────┴──────────────────┴───────────┐
│                    Netplay Session                       │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
│  │   Session   │  │   Input     │  │     State       │  │
│  │   Manager   │  │   Handler   │  │  Synchronizer   │  │
│  └──────┬──────┘  └──────┬──────┘  └────────┬────────┘  │
└─────────┼────────────────┼──────────────────┼───────────┘
          │                │                  │
┌─────────┴────────────────┴──────────────────┴───────────┐
│                    Backroll (GGPO)                       │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
│  │  Rollback   │  │ Prediction  │  │     Network     │  │
│  │   Engine    │  │   System    │  │    Transport    │  │
│  └─────────────┘  └─────────────┘  └─────────────────┘  │
└─────────────────────────────────────────────────────────┘
          │                │                  │
┌─────────┴────────────────┴──────────────────┴───────────┐
│                    Network Layer                         │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
│  │    UDP      │  │   WebRTC    │  │   Relay Server  │  │
│  │  Transport  │  │  Transport  │  │    Transport    │  │
│  └─────────────┘  └─────────────┘  └─────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

### Data Flow

1. **Local Input**: Player input captured and timestamped
2. **Prediction**: Opponent inputs predicted (repeat last known)
3. **Simulation**: Frame advanced with predicted inputs
4. **Transmission**: Local input sent to remote peer
5. **Verification**: Received inputs compared to predictions
6. **Rollback**: If mispredicted, restore state and resimulate

---

## Quick Start

### Basic Two-Player Session

```rust
use rustynes_netplay::{NetplaySession, SessionConfig, PlayerType};
use rustynes_core::Emulator;
use std::net::SocketAddr;

// Create emulator instance
let mut emulator = Emulator::new();
emulator.load_rom("game.nes")?;

// Configure netplay session
let config = SessionConfig {
    local_port: 7000,
    remote_addr: "192.168.1.100:7000".parse()?,
    player_type: PlayerType::Local(1), // Player 1
    input_delay: 2,                    // 2 frame input delay
    max_prediction_frames: 8,          // Max rollback depth
    ..Default::default()
};

// Create and start session
let mut session = NetplaySession::new(config, &mut emulator)?;
session.start()?;

// Main loop
loop {
    // Get local input
    let local_input = get_controller_input();

    // Advance frame with netplay
    let result = session.advance_frame(local_input)?;

    // Handle netplay events
    for event in result.events {
        match event {
            NetplayEvent::Connected => println!("Connected to peer"),
            NetplayEvent::Disconnected => println!("Peer disconnected"),
            NetplayEvent::Desync { frame } => println!("Desync at frame {}", frame),
            _ => {}
        }
    }

    // Render frame
    render_frame(emulator.get_framebuffer());
}
```

---

## Session Management

### SessionConfig

Configuration for netplay sessions.

```rust
pub struct SessionConfig {
    /// Local UDP port to bind
    pub local_port: u16,

    /// Remote peer address
    pub remote_addr: SocketAddr,

    /// Player assignment
    pub player_type: PlayerType,

    /// Input delay frames (reduces rollbacks)
    pub input_delay: u8,

    /// Maximum prediction/rollback frames
    pub max_prediction_frames: u8,

    /// Sync test interval (frames between checksums)
    pub sync_test_interval: u32,

    /// Disconnect timeout (milliseconds)
    pub disconnect_timeout: u32,

    /// Network quality of service settings
    pub qos: QosSettings,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            local_port: 7000,
            remote_addr: "127.0.0.1:7001".parse().unwrap(),
            player_type: PlayerType::Local(1),
            input_delay: 2,
            max_prediction_frames: 8,
            sync_test_interval: 60,
            disconnect_timeout: 5000,
            qos: QosSettings::default(),
        }
    }
}
```

### PlayerType

```rust
pub enum PlayerType {
    /// Local player (controller port 1 or 2)
    Local(u8),

    /// Remote player (peer connection)
    Remote(u8),

    /// Spectator (receive-only)
    Spectator,
}
```

### NetplaySession

Main session management struct.

```rust
impl NetplaySession {
    /// Create new session with configuration
    pub fn new(config: SessionConfig, emulator: &mut Emulator) -> Result<Self>;

    /// Start the session (begin networking)
    pub fn start(&mut self) -> Result<()>;

    /// Stop the session gracefully
    pub fn stop(&mut self) -> Result<()>;

    /// Advance one frame with local input
    pub fn advance_frame(&mut self, input: ControllerInput) -> Result<FrameResult>;

    /// Get current session state
    pub fn state(&self) -> SessionState;

    /// Get network statistics
    pub fn network_stats(&self) -> NetworkStats;

    /// Check if session is synchronized
    pub fn is_synchronized(&self) -> bool;

    /// Get current frame number
    pub fn current_frame(&self) -> u32;
}
```

### SessionState

```rust
pub enum SessionState {
    /// Initializing connection
    Connecting,

    /// Synchronizing initial state
    Synchronizing,

    /// Normal gameplay
    Running,

    /// Temporarily interrupted (packet loss)
    Interrupted,

    /// Connection lost
    Disconnected,
}
```

---

## Input Handling

### Input Format

Controller input is encoded as a single byte for efficient transmission.

```rust
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ControllerInput(u8);

impl ControllerInput {
    pub const A:      u8 = 0b0000_0001;
    pub const B:      u8 = 0b0000_0010;
    pub const SELECT: u8 = 0b0000_0100;
    pub const START:  u8 = 0b0000_1000;
    pub const UP:     u8 = 0b0001_0000;
    pub const DOWN:   u8 = 0b0010_0000;
    pub const LEFT:   u8 = 0b0100_0000;
    pub const RIGHT:  u8 = 0b1000_0000;

    pub fn new() -> Self {
        Self(0)
    }

    pub fn set(&mut self, button: u8, pressed: bool) {
        if pressed {
            self.0 |= button;
        } else {
            self.0 &= !button;
        }
    }

    pub fn is_pressed(&self, button: u8) -> bool {
        (self.0 & button) != 0
    }

    pub fn as_byte(&self) -> u8 {
        self.0
    }
}
```

### Input Timing

```
Timeline: Local Player Perspective

Frame:     0    1    2    3    4    5    6    7    8
          ─────┼────┼────┼────┼────┼────┼────┼────┼────
Local:     I₀   I₁   I₂   I₃   I₄   I₅   I₆   I₇   I₈
           │    │    │    │    │    │    │    │    │
Sent:      └────┴────┴────┘    (input delay = 2)
           │    │    │
Received:  ─────────────────>  Remote receives ~frame 5

Remote:    ?    ?    P₂   P₃   I₄   I₅   ...
           │    │    │    │    │    │
           └────┴────┴────┴────┘
           Predicted (P) until actual input arrives

Legend: I = Input sent, P = Predicted input, ? = Unknown
```

### Input Delay Configuration

Input delay trades responsiveness for prediction accuracy:

| Delay | Pros | Cons | Recommended For |
|-------|------|------|-----------------|
| 0 | Instant response | Maximum rollbacks | LAN only |
| 1-2 | Near-instant | Occasional rollbacks | Fast internet |
| 3-4 | Smooth | Noticeable delay | Average internet |
| 5+ | Very smooth | Input lag | Poor connections |

```rust
// Auto-adjust input delay based on measured RTT
let rtt_ms = session.network_stats().round_trip_time_ms;
let recommended_delay = match rtt_ms {
    0..=20 => 1,
    21..=50 => 2,
    51..=100 => 3,
    101..=150 => 4,
    _ => 5,
};
config.input_delay = recommended_delay;
```

---

## State Synchronization

### State Serialization

The emulator state must be serializable for rollback.

```rust
use serde::{Serialize, Deserialize};

/// Complete emulator state for netplay
#[derive(Clone, Serialize, Deserialize)]
pub struct NetplayState {
    pub cpu: CpuState,
    pub ppu: PpuState,
    pub apu: ApuState,
    pub ram: [u8; 2048],
    pub mapper_state: MapperState,
    pub frame_number: u32,
}

/// Trait for netplay-compatible emulator
pub trait NetplayEmulator {
    /// Serialize current state
    fn save_state(&self) -> NetplayState;

    /// Restore from serialized state
    fn load_state(&mut self, state: &NetplayState);

    /// Advance one frame with given inputs
    fn advance_frame(&mut self, p1_input: ControllerInput, p2_input: ControllerInput);

    /// Get state checksum for sync verification
    fn checksum(&self) -> u64;
}
```

### Checksum Calculation

Periodic checksums detect desynchronization.

```rust
impl Emulator {
    /// Calculate checksum of critical state
    pub fn checksum(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;

        let mut hasher = DefaultHasher::new();

        // CPU state
        self.cpu.registers.hash(&mut hasher);

        // RAM (most important)
        self.ram.hash(&mut hasher);

        // PPU state (visible effects)
        self.ppu.registers.hash(&mut hasher);
        self.ppu.oam.hash(&mut hasher);

        // Frame counter
        self.frame_number.hash(&mut hasher);

        hasher.finish()
    }
}
```

### Sync Verification

```rust
struct SyncVerifier {
    interval: u32,
    last_check: u32,
    pending_checksums: HashMap<u32, u64>,
}

impl SyncVerifier {
    fn should_verify(&self, frame: u32) -> bool {
        frame - self.last_check >= self.interval
    }

    fn verify(&mut self, local: u64, remote: u64, frame: u32) -> Result<(), DesyncError> {
        if local != remote {
            Err(DesyncError {
                frame,
                local_checksum: local,
                remote_checksum: remote,
            })
        } else {
            Ok(())
        }
    }
}
```

---

## Network Protocol

### Packet Format

```
┌──────────────────────────────────────────────────────────┐
│                    Netplay Packet Header                  │
├──────────┬──────────┬──────────┬─────────────────────────┤
│  Magic   │  Type    │  Seq #   │       Payload           │
│  2 bytes │  1 byte  │  2 bytes │      Variable           │
└──────────┴──────────┴──────────┴─────────────────────────┘
```

### Packet Types

```rust
#[repr(u8)]
pub enum PacketType {
    /// Synchronization handshake
    SyncRequest = 0x01,
    SyncReply = 0x02,

    /// Input data
    Input = 0x10,
    InputAck = 0x11,

    /// State verification
    ChecksumRequest = 0x20,
    ChecksumReply = 0x21,

    /// Quality of service
    QosPing = 0x30,
    QosPong = 0x31,

    /// Session control
    KeepAlive = 0x40,
    Disconnect = 0x41,

    /// Spectator
    SpectatorJoin = 0x50,
    SpectatorData = 0x51,
}
```

### Input Packet

```rust
#[derive(Serialize, Deserialize)]
pub struct InputPacket {
    /// Starting frame for this input batch
    pub start_frame: u32,

    /// Input data (up to 8 frames)
    pub inputs: [ControllerInput; 8],

    /// Number of valid inputs
    pub count: u8,

    /// Acknowledgment: last received remote frame
    pub ack_frame: u32,
}
```

### Connection Handshake

```
Player A                                    Player B
    │                                           │
    │──── SyncRequest(game_hash, state) ───────>│
    │                                           │
    │<─── SyncReply(game_hash, state, ok) ──────│
    │                                           │
    │──── SyncRequest(ready=true) ─────────────>│
    │                                           │
    │<─── SyncReply(ready=true) ────────────────│
    │                                           │
    │         ═══ Synchronized ═══              │
    │                                           │
    │<──────── Input(frame=0, data) ───────────>│
    │<──────── Input(frame=1, data) ───────────>│
    │              ... gameplay ...             │
```

### UDP Transport

```rust
pub struct UdpTransport {
    socket: UdpSocket,
    remote_addr: SocketAddr,
    sequence: u16,
    pending_acks: VecDeque<(u16, Instant)>,
}

impl UdpTransport {
    pub async fn send(&mut self, packet: &NetplayPacket) -> Result<()> {
        let data = bincode::serialize(packet)?;
        self.socket.send_to(&data, self.remote_addr).await?;
        self.sequence = self.sequence.wrapping_add(1);
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<NetplayPacket> {
        let mut buf = [0u8; 1024];
        let (len, addr) = self.socket.recv_from(&mut buf).await?;

        if addr != self.remote_addr {
            return Err(Error::UnknownSender);
        }

        bincode::deserialize(&buf[..len]).map_err(Into::into)
    }
}
```

---

## Rollback System

### Rollback Algorithm

```rust
impl RollbackEngine {
    pub fn process_frame(
        &mut self,
        local_input: ControllerInput,
        emulator: &mut impl NetplayEmulator,
    ) -> FrameResult {
        let current_frame = self.current_frame;

        // 1. Save state before advancing
        let state = emulator.save_state();
        self.state_buffer.push(current_frame, state);

        // 2. Get inputs (local + predicted remote)
        let local = local_input;
        let remote = self.get_remote_input(current_frame);

        // 3. Advance frame
        emulator.advance_frame(local, remote);
        self.current_frame += 1;

        // 4. Check for rollback
        let rollback_depth = self.check_rollback(current_frame);

        if rollback_depth > 0 {
            self.perform_rollback(rollback_depth, emulator);
        }

        FrameResult {
            frame: self.current_frame,
            rollback_frames: rollback_depth,
            events: self.drain_events(),
        }
    }

    fn check_rollback(&self, frame: u32) -> u32 {
        // Find earliest frame where prediction was wrong
        let mut rollback_to = None;

        for f in self.confirmed_frame + 1..frame {
            if let Some(actual) = self.received_inputs.get(&f) {
                if let Some(predicted) = self.predicted_inputs.get(&f) {
                    if actual != predicted {
                        rollback_to = Some(f);
                        break;
                    }
                }
            }
        }

        match rollback_to {
            Some(f) => frame - f,
            None => 0,
        }
    }

    fn perform_rollback(
        &mut self,
        depth: u32,
        emulator: &mut impl NetplayEmulator,
    ) {
        let target_frame = self.current_frame - depth;

        // 1. Restore old state
        let state = self.state_buffer.get(target_frame)
            .expect("State not found for rollback");
        emulator.load_state(&state);

        // 2. Resimulate with correct inputs
        for f in target_frame..self.current_frame {
            let local = self.confirmed_inputs.get(&f).copied()
                .unwrap_or_default();
            let remote = self.received_inputs.get(&f).copied()
                .unwrap_or_else(|| self.predict_input(f));

            emulator.advance_frame(local, remote);
        }
    }
}
```

### State Buffer

```rust
pub struct StateBuffer {
    states: VecDeque<(u32, NetplayState)>,
    max_size: usize,
}

impl StateBuffer {
    pub fn new(max_frames: usize) -> Self {
        Self {
            states: VecDeque::with_capacity(max_frames),
            max_size: max_frames,
        }
    }

    pub fn push(&mut self, frame: u32, state: NetplayState) {
        if self.states.len() >= self.max_size {
            self.states.pop_front();
        }
        self.states.push_back((frame, state));
    }

    pub fn get(&self, frame: u32) -> Option<&NetplayState> {
        self.states.iter()
            .find(|(f, _)| *f == frame)
            .map(|(_, s)| s)
    }
}
```

### Input Prediction

```rust
impl InputPredictor {
    /// Predict input for a frame (repeat last known)
    pub fn predict(&self, frame: u32) -> ControllerInput {
        // Find most recent confirmed input
        for f in (0..frame).rev() {
            if let Some(input) = self.confirmed.get(&f) {
                return *input;
            }
        }
        ControllerInput::new() // Default: no buttons pressed
    }
}
```

---

## Spectator Mode

### Spectator Session

```rust
pub struct SpectatorSession {
    transport: UdpTransport,
    input_buffer: BTreeMap<u32, (ControllerInput, ControllerInput)>,
    playback_frame: u32,
    buffer_delay: u32,
}

impl SpectatorSession {
    /// Connect to an ongoing match
    pub async fn connect(match_addr: SocketAddr) -> Result<Self> {
        let transport = UdpTransport::connect(match_addr).await?;

        // Request spectator join
        transport.send(&NetplayPacket::SpectatorJoin).await?;

        // Receive initial state and input history
        let response = transport.recv().await?;
        let (state, inputs) = parse_spectator_init(response)?;

        Ok(Self {
            transport,
            input_buffer: inputs,
            playback_frame: state.frame_number,
            buffer_delay: 30, // 0.5 second buffer
        })
    }

    /// Advance spectator view by one frame
    pub fn advance_frame(&mut self, emulator: &mut impl NetplayEmulator) -> Result<()> {
        // Wait for sufficient buffer
        if self.input_buffer.len() < self.buffer_delay as usize {
            return Ok(()); // Buffer underrun, wait
        }

        // Get inputs for this frame
        let (p1, p2) = self.input_buffer.remove(&self.playback_frame)
            .ok_or(Error::MissingInput)?;

        emulator.advance_frame(p1, p2);
        self.playback_frame += 1;

        Ok(())
    }
}
```

### Spectator Data Broadcast

```rust
impl NetplaySession {
    /// Broadcast inputs to spectators
    fn broadcast_to_spectators(&mut self, frame: u32, p1: ControllerInput, p2: ControllerInput) {
        let packet = SpectatorData {
            frame,
            inputs: (p1, p2),
            checksum: self.emulator.checksum(),
        };

        for spectator in &self.spectators {
            self.transport.send_to(&packet, spectator.addr);
        }
    }
}
```

---

## Matchmaking

### Lobby System

```rust
pub struct MatchmakingClient {
    server_url: String,
    session_id: Option<String>,
}

impl MatchmakingClient {
    /// Connect to matchmaking server
    pub async fn connect(server_url: &str) -> Result<Self> {
        Ok(Self {
            server_url: server_url.to_string(),
            session_id: None,
        })
    }

    /// Create a lobby
    pub async fn create_lobby(&mut self, config: LobbyConfig) -> Result<Lobby> {
        let response = self.post("/api/lobby/create", &config).await?;
        let lobby: Lobby = response.json().await?;
        self.session_id = Some(lobby.id.clone());
        Ok(lobby)
    }

    /// Join an existing lobby
    pub async fn join_lobby(&mut self, lobby_id: &str) -> Result<Lobby> {
        let response = self.post(&format!("/api/lobby/{}/join", lobby_id), &()).await?;
        let lobby: Lobby = response.json().await?;
        self.session_id = Some(lobby_id.to_string());
        Ok(lobby)
    }

    /// Find available lobbies
    pub async fn find_lobbies(&self, filter: LobbyFilter) -> Result<Vec<LobbyInfo>> {
        let response = self.get("/api/lobby/list", &filter).await?;
        response.json().await.map_err(Into::into)
    }

    /// Start match (host only)
    pub async fn start_match(&self) -> Result<MatchInfo> {
        let session_id = self.session_id.as_ref().ok_or(Error::NotInLobby)?;
        let response = self.post(&format!("/api/lobby/{}/start", session_id), &()).await?;
        response.json().await.map_err(Into::into)
    }
}

#[derive(Serialize, Deserialize)]
pub struct LobbyConfig {
    pub game_name: String,
    pub game_hash: String,
    pub max_players: u8,
    pub password: Option<String>,
    pub region: Region,
}

#[derive(Serialize, Deserialize)]
pub struct MatchInfo {
    pub peer_addr: SocketAddr,
    pub player_assignment: u8,
    pub start_frame: u32,
}
```

### NAT Traversal

```rust
pub struct NatTraversal {
    stun_servers: Vec<String>,
    local_addr: SocketAddr,
    public_addr: Option<SocketAddr>,
}

impl NatTraversal {
    /// Determine public address using STUN
    pub async fn discover_public_address(&mut self) -> Result<SocketAddr> {
        for server in &self.stun_servers {
            match stun_request(server, self.local_addr).await {
                Ok(addr) => {
                    self.public_addr = Some(addr);
                    return Ok(addr);
                }
                Err(_) => continue,
            }
        }
        Err(Error::StunFailed)
    }

    /// Attempt UDP hole punching
    pub async fn hole_punch(&self, remote: SocketAddr) -> Result<UdpSocket> {
        let socket = UdpSocket::bind(self.local_addr).await?;

        // Send punch packets
        for _ in 0..10 {
            socket.send_to(b"PUNCH", remote).await?;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Wait for response
        let mut buf = [0u8; 64];
        match tokio::time::timeout(
            Duration::from_secs(5),
            socket.recv_from(&mut buf)
        ).await {
            Ok(Ok((_, addr))) if addr == remote => Ok(socket),
            _ => Err(Error::HolePunchFailed),
        }
    }
}
```

---

## Error Handling

### Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum NetplayError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Peer disconnected")]
    Disconnected,

    #[error("Synchronization failed at frame {frame}")]
    SyncFailed { frame: u32 },

    #[error("Desync detected at frame {frame}: local={local:016x}, remote={remote:016x}")]
    Desync {
        frame: u32,
        local: u64,
        remote: u64,
    },

    #[error("Input timeout for frame {frame}")]
    InputTimeout { frame: u32 },

    #[error("ROM mismatch: expected {expected}, got {actual}")]
    RomMismatch { expected: String, actual: String },

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Network error: {0}")]
    Network(#[from] std::io::Error),
}
```

### Error Recovery

```rust
impl NetplaySession {
    /// Handle desync with recovery attempt
    fn handle_desync(&mut self, error: NetplayError) -> Result<RecoveryAction> {
        match error {
            NetplayError::Desync { frame, .. } => {
                // Attempt resync
                self.request_state_from_peer(frame)?;
                Ok(RecoveryAction::Resync)
            }

            NetplayError::InputTimeout { frame } => {
                // Extend prediction
                if self.prediction_depth < self.max_prediction_frames {
                    self.prediction_depth += 1;
                    Ok(RecoveryAction::ExtendPrediction)
                } else {
                    // Too many predictions, pause
                    Ok(RecoveryAction::Pause)
                }
            }

            NetplayError::Disconnected => {
                Ok(RecoveryAction::Disconnect)
            }

            _ => Err(error),
        }
    }
}

pub enum RecoveryAction {
    Resync,
    ExtendPrediction,
    Pause,
    Disconnect,
}
```

---

## Performance Tuning

### Quality of Service Settings

```rust
#[derive(Clone)]
pub struct QosSettings {
    /// Target rollback frames (aim for this average)
    pub target_rollback: u8,

    /// Maximum acceptable ping (ms)
    pub max_ping: u32,

    /// Jitter buffer size (frames)
    pub jitter_buffer: u8,

    /// Bandwidth limit (bytes/sec, 0 = unlimited)
    pub bandwidth_limit: u32,
}

impl Default for QosSettings {
    fn default() -> Self {
        Self {
            target_rollback: 2,
            max_ping: 200,
            jitter_buffer: 2,
            bandwidth_limit: 0,
        }
    }
}
```

### Network Statistics

```rust
#[derive(Clone, Debug)]
pub struct NetworkStats {
    /// Round-trip time in milliseconds
    pub round_trip_time_ms: u32,

    /// Packet loss percentage (0-100)
    pub packet_loss_percent: f32,

    /// Jitter (RTT variance) in milliseconds
    pub jitter_ms: u32,

    /// Current rollback depth
    pub rollback_frames: u8,

    /// Frames ahead of remote
    pub local_frame_advantage: i8,

    /// Bandwidth usage (bytes/sec)
    pub bandwidth_bytes_sec: u32,
}

impl NetplaySession {
    pub fn network_stats(&self) -> NetworkStats {
        NetworkStats {
            round_trip_time_ms: self.rtt_tracker.average(),
            packet_loss_percent: self.loss_tracker.percentage(),
            jitter_ms: self.rtt_tracker.jitter(),
            rollback_frames: self.rollback_engine.current_depth(),
            local_frame_advantage: self.calculate_frame_advantage(),
            bandwidth_bytes_sec: self.bandwidth_tracker.current(),
        }
    }
}
```

### Adaptive Quality

```rust
impl NetplaySession {
    /// Automatically adjust settings based on network conditions
    pub fn adapt_quality(&mut self) {
        let stats = self.network_stats();

        // Adjust input delay
        if stats.rollback_frames > self.config.qos.target_rollback + 2 {
            // Too many rollbacks, increase delay
            self.config.input_delay = (self.config.input_delay + 1).min(8);
        } else if stats.rollback_frames < self.config.qos.target_rollback
            && self.config.input_delay > 1
        {
            // Smooth connection, reduce delay
            self.config.input_delay -= 1;
        }

        // Adjust sync check frequency
        if stats.packet_loss_percent > 5.0 {
            // High packet loss, check more often
            self.sync_interval = 30;
        } else {
            self.sync_interval = 60;
        }
    }
}
```

---

## Security Considerations

### Input Validation

```rust
impl NetplaySession {
    fn validate_packet(&self, packet: &NetplayPacket) -> Result<()> {
        // Check sequence number (reject old packets)
        if packet.sequence < self.last_received_sequence.saturating_sub(100) {
            return Err(Error::StalePacket);
        }

        // Check frame bounds
        if let Some(frame) = packet.frame() {
            if frame > self.current_frame + 1000 {
                return Err(Error::InvalidFrame);
            }
        }

        // Validate input data
        if let Some(input) = packet.input() {
            // Input is a single byte, all values are valid
            // Just ensure count doesn't exceed buffer
            if packet.input_count() > 8 {
                return Err(Error::InvalidInputCount);
            }
        }

        Ok(())
    }
}
```

### ROM Verification

```rust
impl NetplaySession {
    /// Verify both players have identical ROM
    fn verify_rom_match(&self, remote_hash: &str) -> Result<()> {
        let local_hash = self.emulator.rom_hash();

        if local_hash != remote_hash {
            return Err(NetplayError::RomMismatch {
                expected: local_hash.to_string(),
                actual: remote_hash.to_string(),
            });
        }

        Ok(())
    }
}
```

### Rate Limiting

```rust
impl PacketFilter {
    /// Limit incoming packet rate to prevent DoS
    pub fn should_accept(&mut self, addr: SocketAddr) -> bool {
        let now = Instant::now();
        let entry = self.rate_limits.entry(addr).or_insert((now, 0));

        if now.duration_since(entry.0) > Duration::from_secs(1) {
            // Reset counter
            *entry = (now, 1);
            true
        } else {
            entry.1 += 1;
            // Max 120 packets/sec (2 per frame at 60fps, with headroom)
            entry.1 <= 120
        }
    }
}
```

---

## Debugging

### Debug Overlay

```rust
pub struct NetplayDebugOverlay {
    enabled: bool,
    position: (u32, u32),
}

impl NetplayDebugOverlay {
    pub fn render(&self, session: &NetplaySession, framebuffer: &mut [u8]) {
        if !self.enabled {
            return;
        }

        let stats = session.network_stats();

        let lines = [
            format!("Ping: {}ms", stats.round_trip_time_ms),
            format!("Loss: {:.1}%", stats.packet_loss_percent),
            format!("Rollback: {} frames", stats.rollback_frames),
            format!("Delay: {} frames", session.input_delay()),
            format!("Frame: {}", session.current_frame()),
        ];

        // Render debug text to framebuffer
        for (i, line) in lines.iter().enumerate() {
            draw_text(
                framebuffer,
                self.position.0,
                self.position.1 + i as u32 * 10,
                line,
            );
        }
    }
}
```

### Logging

```rust
use tracing::{debug, info, warn, error};

impl NetplaySession {
    fn log_frame_stats(&self) {
        debug!(
            frame = self.current_frame,
            rollback = self.rollback_depth,
            rtt = self.rtt_tracker.average(),
            "Frame processed"
        );
    }

    fn log_desync(&self, error: &NetplayError) {
        error!(
            ?error,
            frame = self.current_frame,
            "Desynchronization detected"
        );
    }
}
```

### Replay Recording

```rust
pub struct NetplayReplay {
    rom_hash: String,
    start_state: NetplayState,
    inputs: Vec<(u32, ControllerInput, ControllerInput)>,
    checksums: Vec<(u32, u64)>,
}

impl NetplayReplay {
    pub fn start_recording(emulator: &impl NetplayEmulator) -> Self {
        Self {
            rom_hash: emulator.rom_hash().to_string(),
            start_state: emulator.save_state(),
            inputs: Vec::new(),
            checksums: Vec::new(),
        }
    }

    pub fn record_frame(&mut self, frame: u32, p1: ControllerInput, p2: ControllerInput, checksum: u64) {
        self.inputs.push((frame, p1, p2));
        if frame % 60 == 0 {
            self.checksums.push((frame, checksum));
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let data = bincode::serialize(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }
}
```

---

## Examples

### Complete Client Implementation

```rust
use rustynes_netplay::*;
use rustynes_core::Emulator;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize emulator
    let mut emulator = Emulator::new();
    emulator.load_rom("game.nes")?;

    // Parse command line for connection info
    let args: Vec<String> = std::env::args().collect();
    let is_host = args.get(1).map(|s| s == "--host").unwrap_or(false);

    let config = if is_host {
        SessionConfig {
            local_port: 7000,
            remote_addr: "0.0.0.0:0".parse()?, // Will be set when peer connects
            player_type: PlayerType::Local(1),
            ..Default::default()
        }
    } else {
        SessionConfig {
            local_port: 7001,
            remote_addr: args[2].parse()?,
            player_type: PlayerType::Local(2),
            ..Default::default()
        }
    };

    let mut session = NetplaySession::new(config, &mut emulator)?;
    session.start()?;

    println!("Waiting for connection...");

    // Wait for synchronization
    while !session.is_synchronized() {
        session.poll()?;
        tokio::time::sleep(Duration::from_millis(16)).await;
    }

    println!("Connected! Starting game.");

    // Main game loop
    loop {
        // Get local input
        let input = poll_controller();

        // Advance netplay frame
        match session.advance_frame(input) {
            Ok(result) => {
                // Handle events
                for event in result.events {
                    match event {
                        NetplayEvent::Rollback { frames } => {
                            println!("Rolled back {} frames", frames);
                        }
                        NetplayEvent::Disconnected => {
                            println!("Peer disconnected");
                            return Ok(());
                        }
                        _ => {}
                    }
                }

                // Render
                render_frame(emulator.get_framebuffer());
            }
            Err(e) => {
                eprintln!("Netplay error: {}", e);
                break;
            }
        }

        // Maintain 60fps
        tokio::time::sleep(Duration::from_micros(16667)).await;
    }

    session.stop()?;
    Ok(())
}
```

### Lobby-Based Matchmaking

```rust
async fn matchmaking_flow() -> Result<NetplaySession> {
    let mut client = MatchmakingClient::connect("wss://match.rustynes.io").await?;

    // Either create or join lobby
    let lobby = if should_host() {
        client.create_lobby(LobbyConfig {
            game_name: "Super Mario Bros.".to_string(),
            game_hash: "abc123...".to_string(),
            max_players: 2,
            password: None,
            region: Region::NorthAmerica,
        }).await?
    } else {
        // Find and join existing lobby
        let lobbies = client.find_lobbies(LobbyFilter::default()).await?;
        let chosen = select_lobby(&lobbies);
        client.join_lobby(&chosen.id).await?
    };

    println!("In lobby: {} ({}/{})",
        lobby.game_name, lobby.current_players, lobby.max_players);

    // Wait for lobby to fill
    while !lobby.is_full() {
        client.poll().await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Host starts the match
    let match_info = if lobby.is_host {
        client.start_match().await?
    } else {
        client.wait_for_start().await?
    };

    // Create netplay session with match info
    let config = SessionConfig {
        local_port: 0, // OS assigns port
        remote_addr: match_info.peer_addr,
        player_type: PlayerType::Local(match_info.player_assignment),
        ..Default::default()
    };

    let mut emulator = Emulator::new();
    emulator.load_rom(&lobby.game_path)?;

    NetplaySession::new(config, &mut emulator)
}
```

---

## References

### Related Documentation

- [Core API Reference](CORE_API.md)
- [Save State Format](SAVESTATE_FORMAT.md)
- [TAS Movie Format](../formats/FM2_FORMAT.md)

### External Resources

- [GGPO Whitepaper](http://ggpo.net/developer/GGPO.pdf)
- [backroll-rs Documentation](https://docs.rs/backroll)
- [Rollback Netcode Explained](https://words.infil.net/w02-netcode.html)

### Source Files

```
crates/rustynes-netplay/
├── src/
│   ├── lib.rs           # Module exports
│   ├── session.rs       # NetplaySession implementation
│   ├── rollback.rs      # Rollback engine
│   ├── transport.rs     # UDP/WebRTC transport
│   ├── protocol.rs      # Packet definitions
│   ├── sync.rs          # State synchronization
│   ├── spectator.rs     # Spectator mode
│   ├── matchmaking.rs   # Lobby client
│   └── nat.rs           # NAT traversal
└── tests/
    ├── integration.rs   # Full session tests
    └── rollback.rs      # Rollback algorithm tests
```

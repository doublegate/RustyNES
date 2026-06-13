# FM2 (FCEUX Movie) Format Specification

## Overview

FM2 is the movie file format used by FCEUX for recording and playing back NES gameplay. It stores controller inputs frame-by-frame, enabling frame-perfect reproduction of gameplay for Tool-Assisted Speedruns (TAS).

**File Extension:** `.fm2`
**Format Type:** Text-based header + binary/text input log
**Origin:** FCEUX emulator

---

## File Structure

```
+------------------+
|   Text Header    |  Key-value pairs
+------------------+
|  "subtitle ..."  |  Optional subtitle lines
+------------------+
|    Input Log     |  Frame data (| delimited)
+------------------+
```

---

## Header Format

The header consists of key-value pairs, one per line:

```
version 3
emuVersion 20000
rerecordCount 1234
palFlag 0
romFilename Super Mario Bros. (World)
romChecksum base64:abc123...
guid 12345678-1234-1234-1234-123456789ABC
fourscore 0
microphone 0
port0 1
port1 1
port2 0
FDS 0
NewPPU 0
RAMInitOption 0
RAMInitSeed 0
savestate ...base64 or empty...
```

### Required Fields

| Field | Type | Description |
|-------|------|-------------|
| `version` | int | FM2 format version (currently 3) |
| `emuVersion` | int | FCEUX version × 10000 |
| `palFlag` | 0/1 | 0 = NTSC, 1 = PAL |
| `romFilename` | string | Original ROM filename |
| `romChecksum` | base64 | MD5 hash of ROM |
| `port0` | int | Controller type for port 0 |
| `port1` | int | Controller type for port 1 |

### Optional Fields

| Field | Type | Description |
|-------|------|-------------|
| `rerecordCount` | int | Number of re-records |
| `guid` | UUID | Unique movie identifier |
| `fourscore` | 0/1 | Four-player adapter |
| `microphone` | 0/1 | Famicom microphone |
| `port2` | int | Expansion port controller |
| `FDS` | 0/1 | Famicom Disk System |
| `NewPPU` | 0/1 | New PPU emulation mode |
| `RAMInitOption` | int | RAM initialization type |
| `RAMInitSeed` | string | RAM seed (hex) |
| `savestate` | base64 | Starting savestate |
| `comment` | string | Author comment |
| `subtitle` | string | Subtitle lines |

### Controller Types

| Value | Type | Description |
|-------|------|-------------|
| 0 | None | No controller |
| 1 | Gamepad | Standard controller |
| 2 | Zapper | Light gun |
| 3 | Power Pad A | Power Pad Side A |
| 4 | Power Pad B | Power Pad Side B |
| 5 | Arkanoid | Arkanoid paddle |
| 6 | Mouse | SNES mouse |

---

## Input Log Format

Each line represents one frame of input:

```
|RLDUTSBA|RLDUTSBA|
```

### Input Format

```
|commands|port0|port1|port2|
```

- **Commands:** System commands (soft reset, hard reset)
- **Port 0:** Player 1 input
- **Port 1:** Player 2 input
- **Port 2:** Expansion port (optional)

### Button Encoding

```
R = Right
L = Left
D = Down
U = Up
T = Start
S = Select
B = B button
A = A button
```

Each position is either the letter (pressed) or `.` (not pressed):

```
|........|  = No buttons pressed
|R.......|  = Right only
|....T..A|  = Start + A
|RLDUTSBA|  = All buttons pressed
```

### Command Characters

Position 0 (before first `|`):

- `.` = No command
- `r` = Soft reset
- `R` = Hard reset
- `F` = FDS disk insert/eject

---

## Rust Implementation

### Data Structures

```rust
use thiserror::Error;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ControllerInput {
    pub right: bool,
    pub left: bool,
    pub down: bool,
    pub up: bool,
    pub start: bool,
    pub select: bool,
    pub b: bool,
    pub a: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameCommand {
    None,
    SoftReset,
    HardReset,
    FdsDisk,
}

#[derive(Debug, Clone)]
pub struct FrameInput {
    pub command: FrameCommand,
    pub port0: ControllerInput,
    pub port1: ControllerInput,
    pub port2: Option<ControllerInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerType {
    None,
    Gamepad,
    Zapper,
    PowerPadA,
    PowerPadB,
    Arkanoid,
    Mouse,
}

#[derive(Debug, Clone)]
pub struct Fm2Header {
    pub version: u32,
    pub emu_version: u32,
    pub rerecord_count: u32,
    pub pal_flag: bool,
    pub rom_filename: String,
    pub rom_checksum: String,
    pub guid: Option<String>,
    pub fourscore: bool,
    pub microphone: bool,
    pub port0: ControllerType,
    pub port1: ControllerType,
    pub port2: ControllerType,
    pub fds: bool,
    pub new_ppu: bool,
    pub ram_init_option: u32,
    pub ram_init_seed: Option<String>,
    pub savestate: Option<Vec<u8>>,
    pub comments: Vec<String>,
    pub subtitles: Vec<(u32, String)>,  // (frame, text)
}

#[derive(Debug, Clone)]
pub struct Fm2Movie {
    pub header: Fm2Header,
    pub frames: Vec<FrameInput>,
}

#[derive(Debug, Error)]
pub enum Fm2Error {
    #[error("Invalid FM2 format: {0}")]
    InvalidFormat(String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid input line at frame {0}: {1}")]
    InvalidInput(usize, String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}
```

### Input Parsing

```rust
impl ControllerInput {
    pub fn from_fm2_string(s: &str) -> Result<Self, Fm2Error> {
        if s.len() != 8 {
            return Err(Fm2Error::InvalidFormat(
                format!("Input string must be 8 characters, got {}", s.len())
            ));
        }

        let chars: Vec<char> = s.chars().collect();

        Ok(ControllerInput {
            right:  chars[0] == 'R',
            left:   chars[1] == 'L',
            down:   chars[2] == 'D',
            up:     chars[3] == 'U',
            start:  chars[4] == 'T',
            select: chars[5] == 'S',
            b:      chars[6] == 'B',
            a:      chars[7] == 'A',
        })
    }

    pub fn to_fm2_string(&self) -> String {
        format!("{}{}{}{}{}{}{}{}",
            if self.right  { 'R' } else { '.' },
            if self.left   { 'L' } else { '.' },
            if self.down   { 'D' } else { '.' },
            if self.up     { 'U' } else { '.' },
            if self.start  { 'T' } else { '.' },
            if self.select { 'S' } else { '.' },
            if self.b      { 'B' } else { '.' },
            if self.a      { 'A' } else { '.' },
        )
    }

    pub fn to_byte(&self) -> u8 {
        (if self.a      { 0x01 } else { 0 }) |
        (if self.b      { 0x02 } else { 0 }) |
        (if self.select { 0x04 } else { 0 }) |
        (if self.start  { 0x08 } else { 0 }) |
        (if self.up     { 0x10 } else { 0 }) |
        (if self.down   { 0x20 } else { 0 }) |
        (if self.left   { 0x40 } else { 0 }) |
        (if self.right  { 0x80 } else { 0 })
    }

    pub fn from_byte(byte: u8) -> Self {
        ControllerInput {
            a:      (byte & 0x01) != 0,
            b:      (byte & 0x02) != 0,
            select: (byte & 0x04) != 0,
            start:  (byte & 0x08) != 0,
            up:     (byte & 0x10) != 0,
            down:   (byte & 0x20) != 0,
            left:   (byte & 0x40) != 0,
            right:  (byte & 0x80) != 0,
        }
    }
}

impl FrameInput {
    pub fn from_fm2_line(line: &str) -> Result<Self, Fm2Error> {
        let parts: Vec<&str> = line.split('|').collect();

        if parts.len() < 3 {
            return Err(Fm2Error::InvalidFormat(
                "Input line must have at least 3 pipe-separated sections".to_string()
            ));
        }

        // Parse command
        let command = if parts[0].is_empty() {
            FrameCommand::None
        } else {
            match parts[0].chars().next() {
                Some('r') => FrameCommand::SoftReset,
                Some('R') => FrameCommand::HardReset,
                Some('F') => FrameCommand::FdsDisk,
                Some('.') | None => FrameCommand::None,
                Some(c) => return Err(Fm2Error::InvalidFormat(
                    format!("Unknown command character: {}", c)
                )),
            }
        };

        // Parse ports
        let port0 = if parts.len() > 1 && !parts[1].is_empty() {
            ControllerInput::from_fm2_string(parts[1])?
        } else {
            ControllerInput::default()
        };

        let port1 = if parts.len() > 2 && !parts[2].is_empty() {
            ControllerInput::from_fm2_string(parts[2])?
        } else {
            ControllerInput::default()
        };

        let port2 = if parts.len() > 3 && !parts[3].is_empty() {
            Some(ControllerInput::from_fm2_string(parts[3])?)
        } else {
            None
        };

        Ok(FrameInput {
            command,
            port0,
            port1,
            port2,
        })
    }

    pub fn to_fm2_line(&self) -> String {
        let cmd = match self.command {
            FrameCommand::None => "",
            FrameCommand::SoftReset => "r",
            FrameCommand::HardReset => "R",
            FrameCommand::FdsDisk => "F",
        };

        match &self.port2 {
            Some(p2) => format!("|{}|{}|{}|",
                cmd,
                self.port0.to_fm2_string(),
                self.port1.to_fm2_string(),
                p2.to_fm2_string()
            ),
            None => format!("|{}|{}|{}|",
                cmd,
                self.port0.to_fm2_string(),
                self.port1.to_fm2_string()
            ),
        }
    }
}
```

### Movie Parser

```rust
impl Fm2Movie {
    pub fn parse(content: &str) -> Result<Self, Fm2Error> {
        let mut header = Fm2Header::default();
        let mut frames = Vec::new();
        let mut in_input_section = false;
        let mut frame_number = 0;

        for line in content.lines() {
            let line = line.trim();

            if line.is_empty() {
                continue;
            }

            if line.starts_with('|') {
                // Input line
                in_input_section = true;
                let input = FrameInput::from_fm2_line(line)
                    .map_err(|e| Fm2Error::InvalidInput(frame_number, e.to_string()))?;
                frames.push(input);
                frame_number += 1;
            } else if !in_input_section {
                // Header line
                Self::parse_header_line(line, &mut header)?;
            }
        }

        // Validate required fields
        if header.rom_filename.is_empty() {
            return Err(Fm2Error::MissingField("romFilename".to_string()));
        }

        Ok(Fm2Movie { header, frames })
    }

    fn parse_header_line(line: &str, header: &mut Fm2Header) -> Result<(), Fm2Error> {
        let (key, value) = line.split_once(' ')
            .unwrap_or((line, ""));

        match key {
            "version" => header.version = value.parse().unwrap_or(3),
            "emuVersion" => header.emu_version = value.parse().unwrap_or(0),
            "rerecordCount" => header.rerecord_count = value.parse().unwrap_or(0),
            "palFlag" => header.pal_flag = value == "1",
            "romFilename" => header.rom_filename = value.to_string(),
            "romChecksum" => header.rom_checksum = value.to_string(),
            "guid" => header.guid = Some(value.to_string()),
            "fourscore" => header.fourscore = value == "1",
            "microphone" => header.microphone = value == "1",
            "port0" => header.port0 = Self::parse_controller_type(value),
            "port1" => header.port1 = Self::parse_controller_type(value),
            "port2" => header.port2 = Self::parse_controller_type(value),
            "FDS" => header.fds = value == "1",
            "NewPPU" => header.new_ppu = value == "1",
            "RAMInitOption" => header.ram_init_option = value.parse().unwrap_or(0),
            "RAMInitSeed" => header.ram_init_seed = Some(value.to_string()),
            "comment" => header.comments.push(value.to_string()),
            "subtitle" => {
                if let Some((frame_str, text)) = value.split_once(' ') {
                    if let Ok(frame) = frame_str.parse() {
                        header.subtitles.push((frame, text.to_string()));
                    }
                }
            }
            "savestate" => {
                if !value.is_empty() {
                    if let Ok(data) = base64::decode(value) {
                        header.savestate = Some(data);
                    }
                }
            }
            _ => {}  // Ignore unknown fields
        }

        Ok(())
    }

    fn parse_controller_type(value: &str) -> ControllerType {
        match value.parse::<u32>() {
            Ok(0) => ControllerType::None,
            Ok(1) => ControllerType::Gamepad,
            Ok(2) => ControllerType::Zapper,
            Ok(3) => ControllerType::PowerPadA,
            Ok(4) => ControllerType::PowerPadB,
            Ok(5) => ControllerType::Arkanoid,
            Ok(6) => ControllerType::Mouse,
            _ => ControllerType::None,
        }
    }

    pub fn to_string(&self) -> String {
        let mut output = String::new();

        // Write header
        output.push_str(&format!("version {}\n", self.header.version));
        output.push_str(&format!("emuVersion {}\n", self.header.emu_version));
        output.push_str(&format!("rerecordCount {}\n", self.header.rerecord_count));
        output.push_str(&format!("palFlag {}\n", if self.header.pal_flag { 1 } else { 0 }));
        output.push_str(&format!("romFilename {}\n", self.header.rom_filename));
        output.push_str(&format!("romChecksum {}\n", self.header.rom_checksum));

        if let Some(ref guid) = self.header.guid {
            output.push_str(&format!("guid {}\n", guid));
        }

        output.push_str(&format!("fourscore {}\n", if self.header.fourscore { 1 } else { 0 }));
        output.push_str(&format!("port0 {}\n", self.header.port0 as u32));
        output.push_str(&format!("port1 {}\n", self.header.port1 as u32));

        for comment in &self.header.comments {
            output.push_str(&format!("comment {}\n", comment));
        }

        for (frame, text) in &self.header.subtitles {
            output.push_str(&format!("subtitle {} {}\n", frame, text));
        }

        // Write input log
        for frame in &self.frames {
            output.push_str(&frame.to_fm2_line());
            output.push('\n');
        }

        output
    }

    /// Get frame count
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Get movie duration in seconds
    pub fn duration_seconds(&self) -> f64 {
        let fps = if self.header.pal_flag { 50.0 } else { 60.0988 };
        self.frames.len() as f64 / fps
    }

    /// Get input for a specific frame
    pub fn get_frame(&self, frame: usize) -> Option<&FrameInput> {
        self.frames.get(frame)
    }
}

impl Default for Fm2Header {
    fn default() -> Self {
        Fm2Header {
            version: 3,
            emu_version: 20000,
            rerecord_count: 0,
            pal_flag: false,
            rom_filename: String::new(),
            rom_checksum: String::new(),
            guid: None,
            fourscore: false,
            microphone: false,
            port0: ControllerType::Gamepad,
            port1: ControllerType::Gamepad,
            port2: ControllerType::None,
            fds: false,
            new_ppu: false,
            ram_init_option: 0,
            ram_init_seed: None,
            savestate: None,
            comments: Vec::new(),
            subtitles: Vec::new(),
        }
    }
}
```

---

## Movie Playback

### Integration with Emulator

```rust
pub struct MoviePlayer {
    movie: Fm2Movie,
    current_frame: usize,
    is_playing: bool,
}

impl MoviePlayer {
    pub fn new(movie: Fm2Movie) -> Self {
        MoviePlayer {
            movie,
            current_frame: 0,
            is_playing: true,
        }
    }

    /// Get input for current frame and advance
    pub fn advance(&mut self) -> Option<FrameInput> {
        if !self.is_playing || self.current_frame >= self.movie.frames.len() {
            return None;
        }

        let input = self.movie.frames[self.current_frame].clone();
        self.current_frame += 1;
        Some(input)
    }

    /// Check if movie has finished
    pub fn is_finished(&self) -> bool {
        self.current_frame >= self.movie.frames.len()
    }

    /// Seek to specific frame
    pub fn seek(&mut self, frame: usize) {
        self.current_frame = frame.min(self.movie.frames.len());
    }

    /// Get current frame number
    pub fn frame(&self) -> usize {
        self.current_frame
    }

    /// Get total frame count
    pub fn total_frames(&self) -> usize {
        self.movie.frames.len()
    }
}
```

### Recording

```rust
pub struct MovieRecorder {
    header: Fm2Header,
    frames: Vec<FrameInput>,
    rerecord_count: u32,
}

impl MovieRecorder {
    pub fn new(rom_filename: String, rom_checksum: String, pal: bool) -> Self {
        let mut header = Fm2Header::default();
        header.rom_filename = rom_filename;
        header.rom_checksum = rom_checksum;
        header.pal_flag = pal;
        header.guid = Some(uuid::Uuid::new_v4().to_string());

        MovieRecorder {
            header,
            frames: Vec::new(),
            rerecord_count: 0,
        }
    }

    /// Record input for current frame
    pub fn record_frame(&mut self, input: FrameInput) {
        self.frames.push(input);
    }

    /// Truncate movie to current position (for re-recording)
    pub fn truncate(&mut self, frame: usize) {
        if frame < self.frames.len() {
            self.frames.truncate(frame);
            self.rerecord_count += 1;
        }
    }

    /// Build final movie
    pub fn finish(mut self) -> Fm2Movie {
        self.header.rerecord_count = self.rerecord_count;
        Fm2Movie {
            header: self.header,
            frames: self.frames,
        }
    }

    /// Save to file
    pub fn save<P: AsRef<std::path::Path>>(&self, path: P) -> Result<(), Fm2Error> {
        let movie = Fm2Movie {
            header: self.header.clone(),
            frames: self.frames.clone(),
        };
        let content = movie.to_string();
        std::fs::write(path, content)?;
        Ok(())
    }
}
```

---

## Determinism Requirements

For TAS movie sync, the emulator must be deterministic:

### Critical Requirements

1. **Consistent Power-On State**
   - RAM must be initialized identically
   - CPU registers at known values
   - PPU state initialized correctly

2. **Timing Accuracy**
   - Frame length must be exact
   - CPU cycle counts must match reference
   - PPU dot timing must be accurate

3. **Random Number Generation**
   - No external randomness sources
   - All "random" behavior from game code

4. **Save State Consistency**
   - Starting from savestate must produce identical results
   - All internal state must be captured

### RAM Initialization

```rust
pub enum RamInitOption {
    AllZeros,      // All bytes = 0x00
    AllOnes,       // All bytes = 0xFF
    Pattern,       // Alternating 0x00/0xFF
    Random(u64),   // Seeded PRNG
}

impl RamInitOption {
    pub fn initialize_ram(&self, ram: &mut [u8]) {
        match self {
            RamInitOption::AllZeros => ram.fill(0x00),
            RamInitOption::AllOnes => ram.fill(0xFF),
            RamInitOption::Pattern => {
                for (i, byte) in ram.iter_mut().enumerate() {
                    *byte = if (i / 4) % 2 == 0 { 0x00 } else { 0xFF };
                }
            }
            RamInitOption::Random(seed) => {
                use rand::{SeedableRng, Rng};
                let mut rng = rand::rngs::StdRng::seed_from_u64(*seed);
                rng.fill(ram);
            }
        }
    }
}
```

---

## Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_input_line() {
        let input = FrameInput::from_fm2_line("|........|R......A|").unwrap();

        assert_eq!(input.command, FrameCommand::None);
        assert!(!input.port0.right);
        assert!(input.port1.right);
        assert!(input.port1.a);
    }

    #[test]
    fn test_controller_to_string() {
        let input = ControllerInput {
            right: true,
            left: false,
            down: false,
            up: true,
            start: false,
            select: false,
            b: true,
            a: true,
        };

        assert_eq!(input.to_fm2_string(), "R..U..BA");
    }

    #[test]
    fn test_controller_from_byte() {
        let input = ControllerInput::from_byte(0x81);  // A + Right
        assert!(input.a);
        assert!(input.right);
        assert!(!input.b);
    }

    #[test]
    fn test_movie_parse() {
        let content = r#"version 3
emuVersion 20000
rerecordCount 5
palFlag 0
romFilename Test ROM
romChecksum base64:test
port0 1
port1 1
|........|........|
|R.......|........|
|R......A|........|
"#;

        let movie = Fm2Movie::parse(content).unwrap();
        assert_eq!(movie.header.version, 3);
        assert_eq!(movie.header.rerecord_count, 5);
        assert_eq!(movie.frame_count(), 3);
        assert!(movie.frames[1].port0.right);
        assert!(movie.frames[2].port0.a);
    }

    #[test]
    fn test_movie_roundtrip() {
        let original = r#"version 3
emuVersion 20000
rerecordCount 0
palFlag 0
romFilename Test
romChecksum md5:test
fourscore 0
port0 1
port1 1
|........|........|
|R......A|L......B|
"#;

        let movie = Fm2Movie::parse(original).unwrap();
        let output = movie.to_string();
        let reparsed = Fm2Movie::parse(&output).unwrap();

        assert_eq!(movie.frame_count(), reparsed.frame_count());
        assert_eq!(
            movie.frames[1].port0.to_fm2_string(),
            reparsed.frames[1].port0.to_fm2_string()
        );
    }

    #[test]
    fn test_reset_command() {
        let input = FrameInput::from_fm2_line("|r|........|........|").unwrap();
        assert_eq!(input.command, FrameCommand::SoftReset);

        let input = FrameInput::from_fm2_line("|R|........|........|").unwrap();
        assert_eq!(input.command, FrameCommand::HardReset);
    }

    #[test]
    fn test_duration() {
        let content = r#"version 3
romFilename Test
port0 1
port1 1
palFlag 0
|........|........|
|........|........|
|........|........|
"#;
        // 3 frames at ~60 fps
        let movie = Fm2Movie::parse(content).unwrap();
        let duration = movie.duration_seconds();
        assert!((duration - 0.05).abs() < 0.01);  // ~0.05 seconds
    }
}
```

---

## Binary FM2 Extension

Some tools use a binary variant for efficiency:

```rust
pub struct BinaryFm2 {
    pub header: Fm2Header,
    pub frames: Vec<(u8, u8)>,  // (port0, port1) as bytes
}

impl BinaryFm2 {
    pub fn from_movie(movie: &Fm2Movie) -> Self {
        let frames = movie.frames.iter()
            .map(|f| (f.port0.to_byte(), f.port1.to_byte()))
            .collect();

        BinaryFm2 {
            header: movie.header.clone(),
            frames,
        }
    }

    pub fn write_binary<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        // Write frame count
        writer.write_all(&(self.frames.len() as u32).to_le_bytes())?;

        // Write packed input data
        for (p0, p1) in &self.frames {
            writer.write_all(&[*p0, *p1])?;
        }

        Ok(())
    }
}
```

---

## References

- [FCEUX Movie Format Spec](http://fceux.com/web/FM2.html)
- [TASVideos FM2 Documentation](http://tasvideos.org/FM2.html)
- [FCEUX Source Code](https://github.com/TASEmulators/fceux)

---

## See Also

- [TAS_RECORDING.md](../features/TAS_RECORDING.md) - TAS recording implementation
- [DETERMINISM.md](../dev/DETERMINISM.md) - Emulator determinism
- [SAVESTATE_FORMAT.md](../api/SAVESTATE_FORMAT.md) - Save state format

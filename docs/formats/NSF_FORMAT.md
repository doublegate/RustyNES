# NSF (NES Sound Format) Specification

## Overview

NSF (NES Sound Format) is a file format for storing and playing NES music outside of its original game context. It contains the game's music engine code and data, allowing playback of individual tracks.

**File Extension:** `.nsf`
**Magic Number:** `$4E $45 $53 $4D $1A` ("NESM" + EOF)
**Header Size:** 128 bytes

---

## File Structure

```
+------------------+
|  Header (128B)   |  NSF header with metadata
+------------------+
|    Music Data    |  6502 code and music data
+------------------+
```

---

## Header Format

```
Offset  Size  Description
------  ----  -----------
$000    5     Magic: "NESM\x1A"
$005    1     Version number (currently $01)
$006    1     Total number of songs (1-256)
$007    1     Starting song (1-indexed)
$008    2     Load address (little-endian)
$00A    2     Init address (little-endian)
$00C    2     Play address (little-endian)
$00E    32    Song name (null-terminated ASCII)
$02E    32    Artist name (null-terminated ASCII)
$04E    32    Copyright holder (null-terminated ASCII)
$06E    2     NTSC play speed (in 1/1000000th sec units)
$070    8     Bankswitch init values (0 = no bankswitching)
$078    2     PAL play speed (in 1/1000000th sec units)
$07A    1     PAL/NTSC flags
$07B    1     Extra sound chip support
$07C    4     Reserved (must be $00)
```

---

## Header Fields

### Bytes $000-$004: Magic Number

```rust
const NSF_MAGIC: [u8; 5] = [0x4E, 0x45, 0x53, 0x4D, 0x1A]; // "NESM\x1A"
```

### Byte $005: Version

Currently always `$01`. Future versions may extend the format.

### Byte $006: Total Songs

Number of songs in the NSF (1-255, or 256 if $00).

### Byte $007: Starting Song

Default song to play when loaded (1-indexed).

### Bytes $008-$009: Load Address

16-bit little-endian address where music data is loaded:

- Range: `$8000-$FFFF` (typical)
- For bankswitched NSFs: typically `$8000`

### Bytes $00A-$00B: Init Address

Address of the INIT routine. Called once when a song starts:

- **Input:** A = song number (0-indexed)
- **Output:** None (sets up internal state)

### Bytes $00C-$00D: Play Address

Address of the PLAY routine. Called at the play rate:

- **Input:** None
- **Output:** None (updates APU registers)

### Bytes $00E-$02D: Song Name

32-byte null-terminated ASCII string. Pad with zeros if shorter.

### Bytes $02E-$04D: Artist Name

32-byte null-terminated ASCII string.

### Bytes $04E-$06D: Copyright

32-byte null-terminated ASCII string.

### Bytes $06E-$06F: NTSC Play Speed

Play speed in microseconds (μs). Common values:

- `$411A` (16666) = ~60.002 Hz (standard NTSC)
- `$4E20` (20000) = 50 Hz
- `$0000` = Use default (16666 for NTSC)

### Bytes $070-$077: Bankswitch Init Values

8 bytes for initial bank configuration:

```
Index  Bank      Address Range
-----  ----      -------------
0      Bank 0    $8000-$8FFF
1      Bank 1    $9000-$9FFF
2      Bank 2    $A000-$AFFF
3      Bank 3    $B000-$BFFF
4      Bank 4    $C000-$CFFF
5      Bank 5    $D000-$DFFF
6      Bank 6    $E000-$EFFF
7      Bank 7    $F000-$FFFF
```

If all bytes are `$00`, no bankswitching is used.

### Bytes $078-$079: PAL Play Speed

Play speed for PAL systems in microseconds:

- `$4E20` (20000) = 50 Hz (standard PAL)
- `$0000` = Use default (20000 for PAL)

### Byte $07A: PAL/NTSC Flags

```
7  bit  0
---------
xxxx xxBN

N: 0 = NTSC, 1 = PAL
B: 0 = Single mode, 1 = Dual PAL/NTSC
```

| Value | Meaning |
|-------|---------|
| $00   | NTSC only |
| $01   | PAL only |
| $02   | Dual NTSC/PAL |
| $03   | Dual NTSC/PAL (PAL preferred) |

### Byte $07B: Expansion Audio

```
7  bit  0
---------
xSFN 5V0E

E: VRC6 (Konami)
0: VRC7 (Konami, FM synthesis)
V: FDS (Famicom Disk System)
5: MMC5 (Nintendo)
N: Namco 163
F: Sunsoft 5B (FME-07)
S: Reserved (should be 0)
x: Reserved
```

| Bit | Chip | Games |
|-----|------|-------|
| 0   | VRC6 | Akumajou Densetsu, Madara |
| 1   | VRC7 | Lagrange Point |
| 2   | FDS  | Most FDS games |
| 3   | MMC5 | Castlevania III, Just Breed |
| 4   | N163 | King of Kings, Sangokushi II |
| 5   | 5B   | Gimmick! |

### Bytes $07C-$07F: Reserved

Must be `$00 00 00 00`.

---

## Rust Implementation

### Header Structure

```rust
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NsfRegion {
    Ntsc,
    Pal,
    DualNtsc,  // Dual mode, prefer NTSC
    DualPal,   // Dual mode, prefer PAL
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExpansionAudio {
    pub vrc6: bool,
    pub vrc7: bool,
    pub fds: bool,
    pub mmc5: bool,
    pub namco163: bool,
    pub sunsoft5b: bool,
}

#[derive(Debug, Clone)]
pub struct NsfHeader {
    pub version: u8,
    pub total_songs: u8,
    pub starting_song: u8,
    pub load_address: u16,
    pub init_address: u16,
    pub play_address: u16,
    pub song_name: String,
    pub artist: String,
    pub copyright: String,
    pub ntsc_speed_us: u16,
    pub pal_speed_us: u16,
    pub bankswitch_init: [u8; 8],
    pub region: NsfRegion,
    pub expansion: ExpansionAudio,
}

#[derive(Debug, Error)]
pub enum NsfError {
    #[error("Invalid magic number: expected 'NESM\\x1A'")]
    InvalidMagic,

    #[error("File too small: expected at least {expected} bytes, got {actual}")]
    FileTooSmall { expected: usize, actual: usize },

    #[error("Invalid load address: ${0:04X} (must be >= $8000)")]
    InvalidLoadAddress(u16),

    #[error("Unsupported version: {0}")]
    UnsupportedVersion(u8),
}
```

### Header Parser

```rust
impl NsfHeader {
    pub fn parse(data: &[u8]) -> Result<Self, NsfError> {
        if data.len() < 128 {
            return Err(NsfError::FileTooSmall {
                expected: 128,
                actual: data.len(),
            });
        }

        // Validate magic
        if &data[0..5] != b"NESM\x1A" {
            return Err(NsfError::InvalidMagic);
        }

        let version = data[5];
        if version != 1 {
            return Err(NsfError::UnsupportedVersion(version));
        }

        let load_address = u16::from_le_bytes([data[8], data[9]]);
        if load_address < 0x8000 {
            return Err(NsfError::InvalidLoadAddress(load_address));
        }

        // Parse strings (null-terminated, 32 bytes max)
        let song_name = Self::parse_string(&data[0x0E..0x2E]);
        let artist = Self::parse_string(&data[0x2E..0x4E]);
        let copyright = Self::parse_string(&data[0x4E..0x6E]);

        // Parse bankswitch init
        let mut bankswitch_init = [0u8; 8];
        bankswitch_init.copy_from_slice(&data[0x70..0x78]);

        // Parse region flags
        let region_byte = data[0x7A];
        let region = match region_byte & 0x03 {
            0x00 => NsfRegion::Ntsc,
            0x01 => NsfRegion::Pal,
            0x02 => NsfRegion::DualNtsc,
            0x03 => NsfRegion::DualPal,
            _ => unreachable!(),
        };

        // Parse expansion audio
        let exp_byte = data[0x7B];
        let expansion = ExpansionAudio {
            vrc6: (exp_byte & 0x01) != 0,
            vrc7: (exp_byte & 0x02) != 0,
            fds: (exp_byte & 0x04) != 0,
            mmc5: (exp_byte & 0x08) != 0,
            namco163: (exp_byte & 0x10) != 0,
            sunsoft5b: (exp_byte & 0x20) != 0,
        };

        Ok(NsfHeader {
            version,
            total_songs: data[6],
            starting_song: data[7],
            load_address,
            init_address: u16::from_le_bytes([data[0x0A], data[0x0B]]),
            play_address: u16::from_le_bytes([data[0x0C], data[0x0D]]),
            song_name,
            artist,
            copyright,
            ntsc_speed_us: u16::from_le_bytes([data[0x6E], data[0x6F]]),
            pal_speed_us: u16::from_le_bytes([data[0x78], data[0x79]]),
            bankswitch_init,
            region,
            expansion,
        })
    }

    fn parse_string(data: &[u8]) -> String {
        let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
        String::from_utf8_lossy(&data[..end]).to_string()
    }

    /// Check if NSF uses bankswitching
    pub fn uses_bankswitching(&self) -> bool {
        self.bankswitch_init.iter().any(|&b| b != 0)
    }

    /// Get effective play speed in Hz
    pub fn play_rate_hz(&self, use_pal: bool) -> f64 {
        let speed_us = if use_pal {
            if self.pal_speed_us > 0 {
                self.pal_speed_us
            } else {
                20000  // Default PAL
            }
        } else {
            if self.ntsc_speed_us > 0 {
                self.ntsc_speed_us
            } else {
                16666  // Default NTSC
            }
        };

        1_000_000.0 / speed_us as f64
    }
}
```

### NSF Player

```rust
pub struct NsfPlayer {
    pub header: NsfHeader,
    pub data: Vec<u8>,
    cpu: Cpu,
    apu: Apu,
    memory: [u8; 0x10000],
    banks: [u8; 8],
    current_song: u8,
    play_counter: u32,
    cycles_per_play: u32,
}

impl NsfPlayer {
    pub fn load(data: &[u8]) -> Result<Self, NsfError> {
        let header = NsfHeader::parse(data)?;
        let music_data = data[128..].to_vec();

        let mut player = NsfPlayer {
            header,
            data: music_data,
            cpu: Cpu::new(),
            apu: Apu::new(),
            memory: [0; 0x10000],
            banks: [0; 8],
            current_song: 0,
            play_counter: 0,
            cycles_per_play: 0,
        };

        player.init_memory();
        Ok(player)
    }

    fn init_memory(&mut self) {
        // Clear RAM
        self.memory[0x0000..0x0800].fill(0);

        // Initialize APU registers
        for addr in 0x4000..=0x4013 {
            self.memory[addr] = 0;
        }
        self.memory[0x4015] = 0x00;  // Silence all channels
        self.memory[0x4017] = 0x40;  // Frame counter

        // Copy initial bank values
        self.banks.copy_from_slice(&self.header.bankswitch_init);

        // Load music data
        self.load_banks();
    }

    fn load_banks(&mut self) {
        if self.header.uses_bankswitching() {
            // Bankswitched: load 4KB chunks
            for (bank_idx, &bank_num) in self.banks.iter().enumerate() {
                let src_offset = (bank_num as usize) * 0x1000;
                let dst_addr = 0x8000 + (bank_idx * 0x1000);

                if src_offset < self.data.len() {
                    let src_end = (src_offset + 0x1000).min(self.data.len());
                    let len = src_end - src_offset;
                    self.memory[dst_addr..dst_addr + len]
                        .copy_from_slice(&self.data[src_offset..src_end]);
                }
            }
        } else {
            // No bankswitching: load at load address
            let load_addr = self.header.load_address as usize;
            let copy_len = self.data.len().min(0x10000 - load_addr);
            self.memory[load_addr..load_addr + copy_len]
                .copy_from_slice(&self.data[..copy_len]);
        }
    }

    /// Initialize a song for playback
    pub fn init_song(&mut self, song_number: u8) {
        // Validate song number
        let song = song_number.min(self.header.total_songs.saturating_sub(1));
        self.current_song = song;

        // Reset CPU
        self.cpu.reset();

        // Set up for INIT call
        self.cpu.a = song;
        self.cpu.x = 0;  // NTSC
        self.cpu.pc = self.header.init_address;

        // Push return address for RTS
        let return_addr = 0xFFFF - 1;  // Will halt when RTS returns here
        self.cpu.push_word(&mut self.memory, return_addr);

        // Execute INIT until RTS
        self.run_until_return();

        // Reset APU after init
        self.apu.reset();
    }

    /// Call PLAY routine once
    pub fn play(&mut self) {
        // Set up for PLAY call
        self.cpu.pc = self.header.play_address;

        // Push return address
        let return_addr = 0xFFFF - 1;
        self.cpu.push_word(&mut self.memory, return_addr);

        // Execute PLAY
        self.run_until_return();
    }

    fn run_until_return(&mut self) {
        let max_cycles = 1_000_000;  // Timeout
        let mut cycles = 0;

        while cycles < max_cycles {
            let pc = self.cpu.pc;

            // Check for return (PC at $FFFF)
            if pc >= 0xFFFE {
                break;
            }

            // Execute one instruction
            let cpu_cycles = self.cpu.step(&mut NsfBus {
                memory: &mut self.memory,
                apu: &mut self.apu,
                banks: &mut self.banks,
                data: &self.data,
                uses_bankswitching: self.header.uses_bankswitching(),
            });

            cycles += cpu_cycles as u32;
        }
    }

    /// Generate audio samples for the given duration
    pub fn generate_samples(&mut self, sample_rate: u32, duration_ms: u32) -> Vec<f32> {
        let samples_needed = (sample_rate * duration_ms / 1000) as usize;
        let mut samples = Vec::with_capacity(samples_needed);

        let play_rate = self.header.play_rate_hz(false);
        let samples_per_play = sample_rate as f64 / play_rate;
        let mut sample_counter = 0.0;

        for _ in 0..samples_needed {
            sample_counter += 1.0;
            if sample_counter >= samples_per_play {
                sample_counter -= samples_per_play;
                self.play();
            }

            // Get sample from APU
            let sample = self.apu.output();
            samples.push(sample);
        }

        samples
    }
}

/// Memory bus for NSF execution
struct NsfBus<'a> {
    memory: &'a mut [u8; 0x10000],
    apu: &'a mut Apu,
    banks: &'a mut [u8; 8],
    data: &'a [u8],
    uses_bankswitching: bool,
}

impl<'a> Bus for NsfBus<'a> {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x4015 => self.apu.read_status(),
            _ => self.memory[addr as usize],
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // APU registers
            0x4000..=0x4013 | 0x4015 | 0x4017 => {
                self.apu.write(addr, value);
            }

            // Bankswitching
            0x5FF8..=0x5FFF if self.uses_bankswitching => {
                let bank_idx = (addr - 0x5FF8) as usize;
                self.banks[bank_idx] = value;

                // Reload bank
                let src_offset = (value as usize) * 0x1000;
                let dst_addr = 0x8000 + (bank_idx * 0x1000);

                if src_offset < self.data.len() {
                    let src_end = (src_offset + 0x1000).min(self.data.len());
                    let len = src_end - src_offset;
                    self.memory[dst_addr..dst_addr + len]
                        .copy_from_slice(&self.data[src_offset..src_end]);
                }
            }

            // Regular memory
            _ => {
                self.memory[addr as usize] = value;
            }
        }
    }
}
```

---

## Bankswitching Details

NSF uses a simple 4KB bankswitching scheme:

### Memory Layout (Bankswitched)

```
Address Range   Bank Register   Size
-------------   -------------   ----
$8000-$8FFF     $5FF8           4KB
$9000-$9FFF     $5FF9           4KB
$A000-$AFFF     $5FFA           4KB
$B000-$BFFF     $5FFB           4KB
$C000-$CFFF     $5FFC           4KB
$D000-$DFFF     $5FFD           4KB
$E000-$EFFF     $5FFE           4KB
$F000-$FFFF     $5FFF           4KB
```

### Bank Calculation

The music data is divided into 4KB pages:

- Page 0: bytes $0000-$0FFF of music data
- Page 1: bytes $1000-$1FFF of music data
- etc.

Writing to $5FFx selects which 4KB page appears in that address range.

### Non-Bankswitched NSFs

If all bankswitch init bytes are $00:

- Music data is loaded directly at Load Address
- No bankswitching registers are active
- Maximum size: 32KB ($8000-$FFFF)

---

## Expansion Audio

### VRC6 (Konami)

Two square channels + one sawtooth:

```
$9000-$9002: Pulse 1
$A000-$A002: Pulse 2
$B000-$B002: Sawtooth
```

### VRC7 (Konami)

6-channel FM synthesis (YM2413 subset):

```
$9010: Register select
$9030: Register data
```

### FDS (Famicom Disk System)

Wavetable synthesis:

```
$4040-$407F: Wavetable RAM
$4080-$408A: FDS audio registers
```

### MMC5

Two pulse channels + PCM:

```
$5000-$5003: Pulse 1
$5004-$5007: Pulse 2
$5010-$5011: PCM
$5015: Channel enable
```

### Namco 163

Up to 8 wavetable channels:

```
$4800: Data port
$F800: Address port
```

### Sunsoft 5B (FME-07)

3 square channels (AY-3-8910 compatible):

```
$C000: Address port
$E000: Data port
```

---

## Play Speed Calculation

The play speed defines how often the PLAY routine is called:

### Standard Speeds

| Region | Default μs | Hz |
|--------|-----------|-----|
| NTSC | 16666 | ~60.00 |
| PAL | 20000 | 50.00 |

### Custom Speed Calculation

```rust
fn calculate_cycles_between_plays(speed_us: u16, cpu_clock: f64) -> u32 {
    let play_rate = 1_000_000.0 / speed_us as f64;
    (cpu_clock / play_rate) as u32
}

// Example: NTSC, 60Hz play rate
// CPU clock: 1,789,773 Hz
// Cycles per play: 1,789,773 / 60 ≈ 29,829
```

---

## Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn create_minimal_nsf() -> Vec<u8> {
        let mut data = vec![0u8; 128 + 16];

        // Magic
        data[0..5].copy_from_slice(b"NESM\x1A");
        // Version
        data[5] = 1;
        // Songs
        data[6] = 1;
        data[7] = 1;
        // Addresses
        data[8..10].copy_from_slice(&0x8000u16.to_le_bytes());
        data[0x0A..0x0C].copy_from_slice(&0x8000u16.to_le_bytes());
        data[0x0C..0x0E].copy_from_slice(&0x8003u16.to_le_bytes());

        // Song name
        data[0x0E..0x1E].copy_from_slice(b"Test Song\0\0\0\0\0\0\0");

        // Music data: simple RTS
        data[128] = 0x60;  // RTS (init)
        data[131] = 0x60;  // RTS (play)

        data
    }

    #[test]
    fn test_parse_valid_header() {
        let data = create_minimal_nsf();
        let header = NsfHeader::parse(&data).unwrap();

        assert_eq!(header.version, 1);
        assert_eq!(header.total_songs, 1);
        assert_eq!(header.starting_song, 1);
        assert_eq!(header.load_address, 0x8000);
        assert_eq!(header.init_address, 0x8000);
        assert_eq!(header.play_address, 0x8003);
        assert_eq!(header.song_name, "Test Song");
    }

    #[test]
    fn test_invalid_magic() {
        let mut data = create_minimal_nsf();
        data[0] = 0x00;

        assert!(matches!(
            NsfHeader::parse(&data),
            Err(NsfError::InvalidMagic)
        ));
    }

    #[test]
    fn test_expansion_audio_parsing() {
        let mut data = create_minimal_nsf();
        data[0x7B] = 0x3F;  // All expansion chips

        let header = NsfHeader::parse(&data).unwrap();
        assert!(header.expansion.vrc6);
        assert!(header.expansion.vrc7);
        assert!(header.expansion.fds);
        assert!(header.expansion.mmc5);
        assert!(header.expansion.namco163);
        assert!(header.expansion.sunsoft5b);
    }

    #[test]
    fn test_region_parsing() {
        let mut data = create_minimal_nsf();

        data[0x7A] = 0x00;
        assert_eq!(NsfHeader::parse(&data).unwrap().region, NsfRegion::Ntsc);

        data[0x7A] = 0x01;
        assert_eq!(NsfHeader::parse(&data).unwrap().region, NsfRegion::Pal);

        data[0x7A] = 0x02;
        assert_eq!(NsfHeader::parse(&data).unwrap().region, NsfRegion::DualNtsc);
    }

    #[test]
    fn test_bankswitching_detection() {
        let mut data = create_minimal_nsf();

        // No bankswitching (all zeros)
        assert!(!NsfHeader::parse(&data).unwrap().uses_bankswitching());

        // With bankswitching
        data[0x70] = 0x01;
        assert!(NsfHeader::parse(&data).unwrap().uses_bankswitching());
    }

    #[test]
    fn test_play_rate() {
        let data = create_minimal_nsf();
        let header = NsfHeader::parse(&data).unwrap();

        // Default NTSC
        let rate = header.play_rate_hz(false);
        assert!((rate - 60.0).abs() < 0.1);
    }
}
```

---

## NSFe Extension

NSFe is an extended format with better metadata support:

```
+------------------+
| Chunk: "NSFE"    |  Magic identifier
+------------------+
| Chunk: "INFO"    |  Required: basic info (9 bytes min)
+------------------+
| Chunk: "DATA"    |  Required: music data
+------------------+
| Chunk: "NEND"    |  Required: end marker
+------------------+
| Optional chunks  |  "auth", "plst", "time", "fade", "tlbl", etc.
+------------------+
```

### NSFe Chunks

| FourCC | Required | Description |
|--------|----------|-------------|
| INFO | Yes | Basic information |
| DATA | Yes | Music data |
| NEND | Yes | End marker |
| auth | No | Author info strings |
| plst | No | Playlist order |
| time | No | Track times (ms) |
| fade | No | Fade times (ms) |
| tlbl | No | Track labels |
| text | No | Extended text info |
| mixe | No | Expansion mixing levels |
| regn | No | Region override per track |

---

## References

- [NESdev Wiki: NSF](https://www.nesdev.org/wiki/NSF)
- [NESdev Wiki: NSFe](https://www.nesdev.org/wiki/NSFe)
- [NSF Spec (original)](http://kevtris.org/nes/nsfspec.txt)

---

## See Also

- [APU_OVERVIEW.md](../apu/APU_OVERVIEW.md) - APU specification
- [APU_CHANNEL_*.md](../apu/) - Individual channel specs
- [EXPANSION_AUDIO.md](../apu/EXPANSION_AUDIO.md) - Expansion chip audio

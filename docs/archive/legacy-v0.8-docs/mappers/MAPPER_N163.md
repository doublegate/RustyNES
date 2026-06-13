# Mapper 19: Namco 163 (N163)

Namco 163 mapper with wavetable synthesis expansion audio.

## Overview

| Property | Value |
|----------|-------|
| Mapper Number | 19 |
| PRG ROM | 512 KB max |
| PRG RAM | 8 KB (battery optional) |
| CHR ROM/RAM | 256 KB max |
| Mirroring | Mapper controlled |
| Expansion Audio | Yes (up to 8 wavetable channels) |
| IRQ | CPU cycle counter |

## Memory Map

### CPU Memory

| Address | Size | Description |
|---------|------|-------------|
| $4800-$4FFF | 2 KB | Sound RAM / Expansion Audio |
| $5000-$57FF | 2 KB | IRQ Counter (Low) |
| $5800-$5FFF | 2 KB | IRQ Counter (High) / Enable |
| $6000-$7FFF | 8 KB | PRG RAM (battery-backed) |
| $8000-$9FFF | 8 KB | Switchable PRG bank 0 |
| $A000-$BFFF | 8 KB | Switchable PRG bank 1 |
| $C000-$DFFF | 8 KB | Switchable PRG bank 2 |
| $E000-$FFFF | 8 KB | Fixed to last bank |

### PPU Memory

| Address | Size | Description |
|---------|------|-------------|
| $0000-$03FF | 1 KB | CHR bank 0 |
| $0400-$07FF | 1 KB | CHR bank 1 |
| $0800-$0BFF | 1 KB | CHR bank 2 |
| $0C00-$0FFF | 1 KB | CHR bank 3 |
| $1000-$13FF | 1 KB | CHR bank 4 |
| $1400-$17FF | 1 KB | CHR bank 5 |
| $1800-$1BFF | 1 KB | CHR bank 6 |
| $1C00-$1FFF | 1 KB | CHR bank 7 |
| $2000-$27FF | 2 KB | Nametable selection |
| $2800-$2FFF | 2 KB | Nametable selection |

## Registers

### Sound RAM Address ($F800)

```
7  bit  0
---- ----
EAAA AAAA
|||| ||||
|+++-++++- Sound RAM address
+--------- Auto-increment enable
```

### Sound RAM Data ($4800)

Read/write data at the address selected by $F800. If auto-increment is enabled, the address increments after each access.

### IRQ Counter Low ($5000)

```
7  bit  0
---- ----
CCCC CCCC
|||| ||||
++++-++++- IRQ counter bits 0-7
```

### IRQ Counter High ($5800)

```
7  bit  0
---- ----
ECCC CCCC
|||| ||||
|+++-++++- IRQ counter bits 8-14
+--------- IRQ enable
```

### CHR Bank Registers

| Address | CHR Range | Description |
|---------|-----------|-------------|
| $8000 | $0000-$03FF | CHR bank 0 |
| $8800 | $0400-$07FF | CHR bank 1 |
| $9000 | $0800-$0BFF | CHR bank 2 |
| $9800 | $0C00-$0FFF | CHR bank 3 |
| $A000 | $1000-$13FF | CHR bank 4 |
| $A800 | $1400-$17FF | CHR bank 5 |
| $B000 | $1800-$1BFF | CHR bank 6 |
| $B800 | $1C00-$1FFF | CHR bank 7 |

Values $E0-$FF select internal CIRAM instead of CHR ROM.

### Nametable Registers

| Address | Nametable | Description |
|---------|-----------|-------------|
| $C000 | $2000 | NT 0 source |
| $C800 | $2400 | NT 1 source |
| $D000 | $2800 | NT 2 source |
| $D800 | $2C00 | NT 3 source |

Values $E0-$FF select CIRAM pages.

### PRG Bank Registers

| Address | CPU Range | Description |
|---------|-----------|-------------|
| $E000 | $8000-$9FFF | PRG bank 0 (+ sound enable) |
| $E800 | $A000-$BFFF | PRG bank 1 (+ CHR RAM protect) |
| $F000 | $C000-$DFFF | PRG bank 2 |

#### PRG Bank 0 ($E000)

```
7  bit  0
---- ----
CSPP PPPP
|||| ||||
||++-++++- PRG bank select
|+-------- Expansion sound disable (0=enabled)
+--------- ? (unused)
```

#### PRG Bank 1 ($E800)

```
7  bit  0
---- ----
HLPP PPPP
|||| ||||
||++-++++- PRG bank select
|+-------- CHR RAM write protect for $0000-$0FFF
+--------- CHR RAM write protect for $1000-$1FFF
```

## Expansion Audio

### Sound RAM Layout (128 bytes)

The N163 has 128 bytes of internal RAM used for wavetable data and channel registers.

```
$00-$3F: Waveform data (shared by all channels)
$40-$47: Channel 8 registers (if enabled)
$48-$4F: Channel 7 registers
$50-$57: Channel 6 registers
$58-$5F: Channel 5 registers
$60-$67: Channel 4 registers
$68-$6F: Channel 3 registers
$70-$77: Channel 2 registers
$78-$7F: Channel 1 registers (always active)
```

### Channel Registers (8 bytes per channel)

| Offset | Description |
|--------|-------------|
| +0 | Frequency Low |
| +1 | Phase Low |
| +2 | Frequency Mid |
| +3 | Phase Mid |
| +4 | Frequency High + Wave Length |
| +5 | Phase High |
| +6 | Wave Address |
| +7 | Volume + Channel Count (channel 1 only) |

#### Frequency Registers

```
+0: Frequency bits 0-7
+2: Frequency bits 8-15
+4: ---- --FF  Frequency bits 16-17
    LLLL LL--  Wave length (64 - L*4 samples)
```

18-bit frequency value determines playback rate.

#### Phase Registers

```
+1: Phase bits 0-7
+3: Phase bits 8-15
+5: Phase bits 16-23
```

24-bit phase accumulator (top bits index waveform).

#### Wave Address ($+6)

```
7  bit  0
---- ----
AAAA AAAA
|||| ||||
++++-++++- Wave address (in sound RAM nibbles)
```

#### Volume / Channel Count ($+7, Channel 1 only)

```
7  bit  0
---- ----
NCCC VVVV
|||| ||||
|||| ++++- Channel volume (0-15)
|+++------ Number of active channels - 1 (0-7)
+--------- ? (unused)
```

### Wavetable Format

Waveforms are stored as 4-bit samples (nibbles), two per byte:

- Low nibble: First sample
- High nibble: Second sample

Values 0-15 represent amplitude levels.

## Implementation

```rust
pub struct N163 {
    // ROM
    prg_rom: Vec<u8>,
    prg_ram: [u8; 8192],
    chr_mem: Vec<u8>,

    // Banking
    prg_banks: [usize; 3],
    chr_banks: [usize; 8],
    nt_banks: [usize; 4],

    // Control
    sound_enabled: bool,
    chr_ram_protect: [bool; 2],

    // IRQ
    irq_counter: u16,
    irq_enable: bool,
    irq_pending: bool,

    // Sound RAM
    sound_ram: [u8; 128],
    sound_ram_addr: u8,
    sound_auto_increment: bool,

    // Audio state
    active_channels: u8,
    channel_counter: u8,
    cycle_counter: u32,
}

impl N163 {
    pub fn new(prg_rom: &[u8], chr_rom: &[u8]) -> Self {
        let prg_banks = prg_rom.len() / 8192;
        Self {
            prg_rom: prg_rom.to_vec(),
            prg_ram: [0; 8192],
            chr_mem: if chr_rom.is_empty() {
                vec![0; 8192]
            } else {
                chr_rom.to_vec()
            },
            prg_banks: [0, 0, 0],
            chr_banks: [0; 8],
            nt_banks: [0; 4],
            sound_enabled: true,
            chr_ram_protect: [false; 2],
            irq_counter: 0,
            irq_enable: false,
            irq_pending: false,
            sound_ram: [0; 128],
            sound_ram_addr: 0,
            sound_auto_increment: false,
            active_channels: 1,
            channel_counter: 0,
            cycle_counter: 0,
        }
    }

    fn read_sound_ram(&mut self) -> u8 {
        let value = self.sound_ram[self.sound_ram_addr as usize & 0x7F];
        if self.sound_auto_increment {
            self.sound_ram_addr = (self.sound_ram_addr + 1) & 0x7F;
        }
        value
    }

    fn write_sound_ram(&mut self, value: u8) {
        self.sound_ram[self.sound_ram_addr as usize & 0x7F] = value;
        if self.sound_auto_increment {
            self.sound_ram_addr = (self.sound_ram_addr + 1) & 0x7F;
        }
    }

    fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            // Sound RAM data
            0x4800..=0x4FFF => self.write_sound_ram(value),

            // IRQ counter low
            0x5000..=0x57FF => {
                self.irq_counter = (self.irq_counter & 0xFF00) | (value as u16);
                self.irq_pending = false;
            }

            // IRQ counter high + enable
            0x5800..=0x5FFF => {
                self.irq_counter = (self.irq_counter & 0x00FF) | ((value as u16 & 0x7F) << 8);
                self.irq_enable = value & 0x80 != 0;
                self.irq_pending = false;
            }

            // CHR banks
            0x8000..=0x8FFF => self.chr_banks[0] = value as usize,
            0x8800..=0x8FFF => self.chr_banks[1] = value as usize,
            0x9000..=0x9FFF => self.chr_banks[2] = value as usize,
            0x9800..=0x9FFF => self.chr_banks[3] = value as usize,
            0xA000..=0xAFFF => self.chr_banks[4] = value as usize,
            0xA800..=0xAFFF => self.chr_banks[5] = value as usize,
            0xB000..=0xBFFF => self.chr_banks[6] = value as usize,
            0xB800..=0xBFFF => self.chr_banks[7] = value as usize,

            // Nametable banks
            0xC000..=0xCFFF => self.nt_banks[0] = value as usize,
            0xC800..=0xCFFF => self.nt_banks[1] = value as usize,
            0xD000..=0xDFFF => self.nt_banks[2] = value as usize,
            0xD800..=0xDFFF => self.nt_banks[3] = value as usize,

            // PRG banks
            0xE000..=0xEFFF => {
                self.prg_banks[0] = (value & 0x3F) as usize;
                self.sound_enabled = value & 0x40 == 0;
            }
            0xE800..=0xEFFF => {
                self.prg_banks[1] = (value & 0x3F) as usize;
                self.chr_ram_protect[0] = value & 0x40 != 0;
                self.chr_ram_protect[1] = value & 0x80 != 0;
            }
            0xF000..=0xFFFF => {
                self.prg_banks[2] = (value & 0x3F) as usize;
            }

            // Sound RAM address
            0xF800..=0xFFFF => {
                self.sound_ram_addr = value & 0x7F;
                self.sound_auto_increment = value & 0x80 != 0;
            }

            _ => {}
        }
    }

    /// Clock IRQ (called every CPU cycle)
    pub fn clock_irq(&mut self) {
        if self.irq_enable {
            if self.irq_counter == 0x7FFF {
                self.irq_pending = true;
            } else {
                self.irq_counter += 1;
            }
        }
    }

    /// Clock audio (called at CPU rate)
    pub fn clock_audio(&mut self) {
        if !self.sound_enabled {
            return;
        }

        // N163 clocks at CPU/15 rate
        self.cycle_counter += 1;
        if self.cycle_counter < 15 {
            return;
        }
        self.cycle_counter = 0;

        // Cycle through active channels
        self.clock_channel(self.channel_counter);

        // Advance to next channel
        if self.channel_counter == 0 {
            self.channel_counter = self.active_channels;
        } else {
            self.channel_counter -= 1;
        }
    }

    fn clock_channel(&mut self, channel: u8) {
        // Channel registers start at $78 and go down
        let base = 0x78 - (channel as usize * 8);

        // Read channel parameters
        let freq_low = self.sound_ram[base] as u32;
        let freq_mid = self.sound_ram[base + 2] as u32;
        let freq_high = self.sound_ram[base + 4] as u32;
        let frequency = freq_low | (freq_mid << 8) | ((freq_high & 0x03) << 16);

        let wave_length = 256 - ((freq_high as usize >> 2) * 4);
        let wave_addr = self.sound_ram[base + 6] as usize;

        // Read current phase
        let phase_low = self.sound_ram[base + 1] as u32;
        let phase_mid = self.sound_ram[base + 3] as u32;
        let phase_high = self.sound_ram[base + 5] as u32;
        let mut phase = phase_low | (phase_mid << 8) | (phase_high << 16);

        // Advance phase
        phase = phase.wrapping_add(frequency);

        // Write back phase
        self.sound_ram[base + 1] = phase as u8;
        self.sound_ram[base + 3] = (phase >> 8) as u8;
        self.sound_ram[base + 5] = (phase >> 16) as u8;
    }

    /// Get audio output (-1.0 to 1.0)
    pub fn audio_output(&self) -> f32 {
        if !self.sound_enabled {
            return 0.0;
        }

        let mut total = 0i32;

        for ch in 0..=self.active_channels {
            let base = 0x78 - (ch as usize * 8);

            // Get phase and wave parameters
            let phase_high = self.sound_ram[base + 5] as usize;
            let freq_high = self.sound_ram[base + 4] as usize;
            let wave_length = 256 - ((freq_high >> 2) * 4);
            let wave_addr = self.sound_ram[base + 6] as usize;

            // Calculate sample position
            let sample_pos = (phase_high >> (8 - wave_length.trailing_zeros())) % wave_length;
            let ram_addr = (wave_addr + sample_pos) & 0x7F;

            // Read 4-bit sample (nibble)
            let byte = self.sound_ram[ram_addr / 2];
            let sample = if ram_addr & 1 == 0 {
                byte & 0x0F
            } else {
                byte >> 4
            };

            // Get volume
            let volume = self.sound_ram[base + 7] & 0x0F;

            // Accumulate
            total += (sample as i32 - 8) * volume as i32;
        }

        // Normalize output
        let channel_count = (self.active_channels + 1) as f32;
        (total as f32) / (channel_count * 15.0 * 8.0)
    }
}

impl Mapper for N163 {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x4800..=0x4FFF => {
                // Sound RAM read (needs mutable, return 0 here)
                0
            }
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0x9FFF => {
                let offset = self.prg_banks[0] * 0x2000 + (addr - 0x8000) as usize;
                self.prg_rom[offset % self.prg_rom.len()]
            }
            0xA000..=0xBFFF => {
                let offset = self.prg_banks[1] * 0x2000 + (addr - 0xA000) as usize;
                self.prg_rom[offset % self.prg_rom.len()]
            }
            0xC000..=0xDFFF => {
                let offset = self.prg_banks[2] * 0x2000 + (addr - 0xC000) as usize;
                self.prg_rom[offset % self.prg_rom.len()]
            }
            0xE000..=0xFFFF => {
                let last_bank = self.prg_rom.len() / 0x2000 - 1;
                let offset = last_bank * 0x2000 + (addr - 0xE000) as usize;
                self.prg_rom[offset]
            }
            _ => 0,
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }
}
```

## Audio Mixing

```rust
fn mix_audio(apu_output: f32, n163_output: f32) -> f32 {
    // N163 is approximately 30% of total volume
    let n163_scaled = n163_output * 0.30;
    apu_output * 0.70 + n163_scaled
}
```

## Channel Count Considerations

The N163 can enable 1-8 channels, but more channels mean:

- Lower per-channel update rate (channels are time-multiplexed)
- Potentially audible aliasing artifacts
- Most games use 4-5 channels maximum

## Notable Games

- Megami Tensei II
- Rolling Thunder
- Sangokushi
- Final Lap
- Erika to Satoru no Yume Bouken
- King of Kings
- Namco Classic

## Implementation Notes

1. **Sound RAM Access**: Reading $4800 can conflict with audio clocking.

2. **Waveform Updates**: Changing wave data while playing causes clicks.

3. **Channel Timing**: Channels are clocked sequentially, not simultaneously.

4. **Battery RAM**: Some games save to both $6000-$7FFF and sound RAM.

## References

- [NESdev Wiki: Namco 163](https://www.nesdev.org/wiki/Namco_163)
- [NESdev Wiki: Namco 163 audio](https://www.nesdev.org/wiki/Namco_163_audio)

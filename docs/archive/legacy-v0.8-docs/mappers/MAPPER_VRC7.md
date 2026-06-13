# Mapper 85: VRC7

Konami VRC7 mapper with YM2413-derived FM synthesis expansion audio.

## Overview

| Property | Value |
|----------|-------|
| Mapper Number | 85 |
| PRG ROM | 512 KB max |
| PRG RAM | 8 KB (battery optional) |
| CHR ROM/RAM | 128 KB max |
| Mirroring | Mapper controlled |
| Expansion Audio | Yes (6 FM channels) |
| IRQ | Scanline counter |

## Memory Map

### CPU Memory

| Address | Size | Description |
|---------|------|-------------|
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

## Registers

### Address Selection

The VRC7 uses an address line selection scheme. Addresses are decoded as:

```
$8000: A4=0, A3=0 (PRG Bank 0)
$8008: A4=0, A3=1 (PRG Bank 0 mirror)
$8010: A4=1, A3=0 (PRG Bank 1)
$9000: A4=0, A3=0 (Audio Register Select)
$9010: A4=1, A3=0 (Audio Data Write)
$9030: A4=1, A3=1 (Audio Data Write alternate)
```

### PRG Bank Registers

| Address | Register | Description |
|---------|----------|-------------|
| $8000 | PRG Bank 0 | 8 KB bank at $8000 |
| $8010 | PRG Bank 1 | 8 KB bank at $A000 |
| $9000 | PRG Bank 2 | 8 KB bank at $C000 |

### CHR Bank Registers

| Address | Register | Description |
|---------|----------|-------------|
| $A000 | CHR Bank 0 | 1 KB at $0000 |
| $A010 | CHR Bank 1 | 1 KB at $0400 |
| $B000 | CHR Bank 2 | 1 KB at $0800 |
| $B010 | CHR Bank 3 | 1 KB at $0C00 |
| $C000 | CHR Bank 4 | 1 KB at $1000 |
| $C010 | CHR Bank 5 | 1 KB at $1400 |
| $D000 | CHR Bank 6 | 1 KB at $1800 |
| $D010 | CHR Bank 7 | 1 KB at $1C00 |

### Mirroring ($E000)

```
7  bit  0
---- ----
RS.. ..MM
||     ||
||     ++- Mirroring mode
||         0: Vertical
||         1: Horizontal
||         2: One-screen, lower
||         3: One-screen, upper
|+-------- Silence audio
+--------- WRAM enable
```

### IRQ Registers

| Address | Register | Description |
|---------|----------|-------------|
| $E010 | IRQ Latch | Counter reload value |
| $F000 | IRQ Control | Enable and mode |
| $F010 | IRQ Acknowledge | Clear IRQ and reload |

## FM Synthesis Audio

### Overview

The VRC7's audio is based on the Yamaha YM2413 (OPLL), a low-cost FM synthesis chip. It provides:

- 6 melodic channels
- 15 preset instrument patches + 1 custom patch
- 2-operator FM synthesis per channel

### Audio Registers

Write to $9010 (or $9030) after selecting register with $9000.

#### Register Map

| Register | Description |
|----------|-------------|
| $00-$07 | Custom patch definition |
| $10-$15 | Channel frequency low |
| $20-$25 | Channel control (key, octave, freq high) |
| $30-$35 | Channel instrument and volume |

### Custom Patch Registers ($00-$07)

```
$00: Modulator tremolo/vibrato/sustain/KSR/multiplication
$01: Carrier tremolo/vibrato/sustain/KSR/multiplication
$02: Modulator key scaling level / total level
$03: Carrier key scaling level / waveform / feedback
$04: Modulator attack / decay rate
$05: Carrier attack / decay rate
$06: Modulator sustain / release rate
$07: Carrier sustain / release rate
```

### Frequency Low Register ($10-$15)

```
7  bit  0
---- ----
FFFF FFFF
|||| ||||
++++-++++- F-Number bits 0-7
```

### Channel Control ($20-$25)

```
7  bit  0
---- ----
..ST OOOO
  || ||||
  || ++++- F-Number bit 8 / Octave
  |+------ Trigger (key on)
  +------- Sustain
```

### Instrument/Volume ($30-$35)

```
7  bit  0
---- ----
IIII VVVV
|||| ||||
|||| ++++- Volume (0=loudest, 15=silent)
++++------ Instrument (0=custom, 1-15=preset)
```

### Preset Instruments

| Number | Name |
|--------|------|
| 0 | Custom (user-defined) |
| 1 | Bell |
| 2 | Guitar |
| 3 | Piano |
| 4 | Flute |
| 5 | Clarinet |
| 6 | Rattling Bell |
| 7 | Trumpet |
| 8 | Reed Organ |
| 9 | Soft Bell |
| 10 | Xylophone |
| 11 | Vibraphone |
| 12 | Brass |
| 13 | Bass Guitar |
| 14 | Synthesizer |
| 15 | Chorus |

## Implementation

```rust
pub struct Vrc7 {
    // Mapper state
    prg_rom: Vec<u8>,
    prg_ram: [u8; 8192],
    chr_mem: Vec<u8>,

    // Banking
    prg_banks: [usize; 3],
    chr_banks: [usize; 8],

    // Control
    mirroring: Mirroring,
    prg_ram_enabled: bool,
    audio_silenced: bool,

    // IRQ
    irq_latch: u8,
    irq_counter: u8,
    irq_enable: bool,
    irq_enable_after_ack: bool,
    irq_mode: bool,
    irq_pending: bool,
    irq_prescaler: u16,

    // FM audio
    fm: Vrc7Fm,
    audio_register: u8,
}

impl Vrc7 {
    pub fn new(prg_rom: &[u8], chr: &[u8]) -> Self {
        let prg_banks = prg_rom.len() / 8192;
        Self {
            prg_rom: prg_rom.to_vec(),
            prg_ram: [0; 8192],
            chr_mem: if chr.is_empty() {
                vec![0; 8192]
            } else {
                chr.to_vec()
            },
            prg_banks: [0, 0, 0],
            chr_banks: [0; 8],
            mirroring: Mirroring::Vertical,
            prg_ram_enabled: false,
            audio_silenced: false,
            irq_latch: 0,
            irq_counter: 0,
            irq_enable: false,
            irq_enable_after_ack: false,
            irq_mode: false,
            irq_pending: false,
            irq_prescaler: 0,
            fm: Vrc7Fm::new(),
            audio_register: 0,
        }
    }

    fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            // PRG banking
            0x8000 => self.prg_banks[0] = (value & 0x3F) as usize,
            0x8010 | 0x8008 => self.prg_banks[1] = (value & 0x3F) as usize,
            0x9000 => {
                // Audio register select
                self.audio_register = value;
            }
            0x9010 | 0x9030 => {
                // Audio data write
                if !self.audio_silenced {
                    self.fm.write(self.audio_register, value);
                }
            }

            // More PRG (depends on board variant)
            0x9000 if false => self.prg_banks[2] = (value & 0x3F) as usize,

            // CHR banking
            0xA000 => self.chr_banks[0] = value as usize,
            0xA010 | 0xA008 => self.chr_banks[1] = value as usize,
            0xB000 => self.chr_banks[2] = value as usize,
            0xB010 | 0xB008 => self.chr_banks[3] = value as usize,
            0xC000 => self.chr_banks[4] = value as usize,
            0xC010 | 0xC008 => self.chr_banks[5] = value as usize,
            0xD000 => self.chr_banks[6] = value as usize,
            0xD010 | 0xD008 => self.chr_banks[7] = value as usize,

            // Mirroring and control
            0xE000 => {
                self.mirroring = match value & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SingleScreenLower,
                    3 => Mirroring::SingleScreenUpper,
                    _ => unreachable!(),
                };
                self.audio_silenced = value & 0x40 != 0;
                self.prg_ram_enabled = value & 0x80 != 0;
            }

            // IRQ
            0xE010 | 0xE008 => self.irq_latch = value,
            0xF000 => {
                self.irq_enable_after_ack = value & 0x01 != 0;
                self.irq_enable = value & 0x02 != 0;
                self.irq_mode = value & 0x04 != 0;
                if self.irq_enable {
                    self.irq_counter = self.irq_latch;
                    self.irq_prescaler = 341;
                }
                self.irq_pending = false;
            }
            0xF010 | 0xF008 => {
                self.irq_pending = false;
                self.irq_enable = self.irq_enable_after_ack;
            }

            _ => {}
        }
    }

    pub fn audio_output(&mut self) -> f32 {
        if self.audio_silenced {
            0.0
        } else {
            self.fm.output()
        }
    }
}

/// Simplified FM synthesis engine
pub struct Vrc7Fm {
    channels: [FmChannel; 6],
    custom_patch: [u8; 8],
}

impl Vrc7Fm {
    pub fn new() -> Self {
        Self {
            channels: [FmChannel::new(); 6],
            custom_patch: [0; 8],
        }
    }

    pub fn write(&mut self, reg: u8, value: u8) {
        match reg {
            // Custom patch
            0x00..=0x07 => {
                self.custom_patch[(reg - 0x00) as usize] = value;
            }
            // Frequency low
            0x10..=0x15 => {
                let ch = (reg - 0x10) as usize;
                self.channels[ch].freq_low = value;
            }
            // Channel control
            0x20..=0x25 => {
                let ch = (reg - 0x20) as usize;
                self.channels[ch].octave = value & 0x0F;
                self.channels[ch].trigger = value & 0x10 != 0;
                self.channels[ch].sustain = value & 0x20 != 0;
            }
            // Instrument/volume
            0x30..=0x35 => {
                let ch = (reg - 0x30) as usize;
                self.channels[ch].volume = value & 0x0F;
                self.channels[ch].instrument = (value >> 4) & 0x0F;
            }
            _ => {}
        }
    }

    pub fn clock(&mut self) {
        for channel in &mut self.channels {
            channel.clock(&self.custom_patch);
        }
    }

    pub fn output(&self) -> f32 {
        let mut total = 0i32;
        for channel in &self.channels {
            total += channel.output() as i32;
        }
        // Normalize output
        (total as f32) / (6.0 * 256.0)
    }
}

#[derive(Clone, Copy)]
pub struct FmChannel {
    freq_low: u8,
    octave: u8,
    trigger: bool,
    sustain: bool,
    volume: u8,
    instrument: u8,

    // Internal state
    phase: u32,
    envelope: u8,
    key_on: bool,
}

impl FmChannel {
    pub fn new() -> Self {
        Self {
            freq_low: 0,
            octave: 0,
            trigger: false,
            sustain: false,
            volume: 15, // Silent
            instrument: 0,
            phase: 0,
            envelope: 0,
            key_on: false,
        }
    }

    pub fn clock(&mut self, custom_patch: &[u8; 8]) {
        // Handle key on/off
        if self.trigger && !self.key_on {
            self.key_on = true;
            self.phase = 0;
            self.envelope = 0;
        } else if !self.trigger && self.key_on {
            self.key_on = false;
        }

        // Calculate frequency
        let f_number = self.freq_low as u32 | ((self.octave as u32 & 0x01) << 8);
        let octave = (self.octave >> 1) & 0x07;

        // Phase increment
        let phase_inc = (f_number << octave) >> 2;
        self.phase = self.phase.wrapping_add(phase_inc);

        // Simple envelope
        if self.key_on {
            if self.envelope < 255 {
                self.envelope = self.envelope.saturating_add(4);
            }
        } else {
            self.envelope = self.envelope.saturating_sub(2);
        }
    }

    pub fn output(&self) -> i16 {
        if self.volume == 15 {
            return 0; // Muted
        }

        // Simple sine approximation
        let phase_index = (self.phase >> 18) & 0x3FF;
        let sine = SINE_TABLE[phase_index as usize];

        // Apply volume and envelope
        let vol = 15 - self.volume;
        let env = self.envelope as i32;
        let output = (sine as i32 * vol as i32 * env) >> 12;

        output as i16
    }
}

/// Precomputed sine table (1024 entries, -127 to 127)
static SINE_TABLE: [i8; 1024] = {
    let mut table = [0i8; 1024];
    let mut i = 0;
    while i < 1024 {
        let angle = (i as f64) * std::f64::consts::PI * 2.0 / 1024.0;
        table[i] = (angle.sin() * 127.0) as i8;
        i += 1;
    }
    table
};
```

## Audio Mixing

```rust
fn mix_audio(apu_output: f32, vrc7_output: f32) -> f32 {
    // VRC7 FM audio is approximately 35% of total mix
    let vrc7_scaled = vrc7_output * 0.35;
    apu_output * 0.65 + vrc7_scaled
}
```

## Notable Games

- Lagrange Point (only VRC7 game)
- Tiny Toon Adventures 2 (Japan, uses VRC7 mapper but no audio)

## Implementation Notes

1. **FM Synthesis Complexity**: Full YM2413 emulation requires accurate operator handling, envelope generators, and feedback. Consider using existing FM synthesis libraries.

2. **Register Timing**: Audio register writes have specific timing requirements.

3. **Preset Instruments**: The 15 preset patches are hardcoded in the chip and different from YM2413 presets.

## References

- [NESdev Wiki: VRC7](https://www.nesdev.org/wiki/VRC7)
- [NESdev Wiki: VRC7 audio](https://www.nesdev.org/wiki/VRC7_audio)
- [YM2413 datasheet](https://www.smspower.org/uploads/Development/YM2413ApplicationManual.pdf)

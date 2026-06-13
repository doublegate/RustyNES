# Mapper 24/26: VRC6

Konami VRC6 mapper with expansion audio (two pulse channels + sawtooth).

## Overview

| Property | Value |
|----------|-------|
| Mapper Number | 24 (VRC6a), 26 (VRC6b) |
| PRG ROM | 256 KB max |
| PRG RAM | 8 KB (battery optional) |
| CHR ROM/RAM | 256 KB max |
| Mirroring | Mapper controlled |
| Expansion Audio | Yes (3 channels) |
| IRQ | Scanline counter |

## Variants

| Mapper | Pin Configuration | Address Lines |
|--------|-------------------|---------------|
| 24 (VRC6a) | Standard | A0, A1 |
| 26 (VRC6b) | Swapped | A1, A0 |

The difference is A0 and A1 are swapped on VRC6b.

## Memory Map

### CPU Memory

| Address | Size | Description |
|---------|------|-------------|
| $6000-$7FFF | 8 KB | PRG RAM (battery-backed) |
| $8000-$BFFF | 16 KB | Switchable PRG bank |
| $C000-$DFFF | 8 KB | Switchable PRG bank |
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

### PRG Bank Select

| Address | Register | Description |
|---------|----------|-------------|
| $8000-$8003 | PRG Bank 0 | 16 KB bank at $8000-$BFFF |
| $C000-$C003 | PRG Bank 1 | 8 KB bank at $C000-$DFFF |

### CHR Bank Select

| Address | Register | Description |
|---------|----------|-------------|
| $D000-$D003 | CHR Bank 0-3 | 1 KB banks at $0000-$0FFF |
| $E000-$E003 | CHR Bank 4-7 | 1 KB banks at $1000-$1FFF |

### Mirroring Control ($B003)

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
|+-------- CHR RAM/ROM select
+--------- PRG RAM enable
```

### IRQ Registers

| Address | Register | Description |
|---------|----------|-------------|
| $F000 | IRQ Latch | IRQ counter reload value |
| $F001 | IRQ Control | IRQ enable and mode |
| $F002 | IRQ Acknowledge | Clear IRQ flag |

#### IRQ Control ($F001)

```
7  bit  0
---- ----
A... .MES
|     |||
|     ||+- IRQ enable after acknowledgement
|     |+-- IRQ enable
|     +--- IRQ mode (0: scanline, 1: CPU cycle)
+--------- Acknowledge IRQ
```

## Expansion Audio

### Audio Registers

| Address | Register | Description |
|---------|----------|-------------|
| $9000 | Pulse 1 Control | Volume, duty, enable |
| $9001 | Pulse 1 Period Low | Period bits 0-7 |
| $9002 | Pulse 1 Period High | Period bits 8-11, enable |
| $A000 | Pulse 2 Control | Volume, duty, enable |
| $A001 | Pulse 2 Period Low | Period bits 0-7 |
| $A002 | Pulse 2 Period High | Period bits 8-11, enable |
| $B000 | Sawtooth Accumulator | Accumulator rate |
| $B001 | Sawtooth Period Low | Period bits 0-7 |
| $B002 | Sawtooth Period High | Period bits 8-11, enable |

### Pulse Channel Control ($9000, $A000)

```
7  bit  0
---- ----
MDDD VVVV
|||| ||||
|||| ++++- Volume (0-15)
|+++------ Duty cycle (0-7, 1/16 to 8/16)
+--------- Mode (0: normal, 1: ignore duty, output = volume)
```

### Pulse Period High ($9002, $A002)

```
7  bit  0
---- ----
E... PPPP
|    ||||
|    ++++- Period bits 8-11
+--------- Channel enable
```

### Sawtooth Accumulator Rate ($B000)

```
7  bit  0
---- ----
..AA AAAA
  || ||||
  ++-++++- Accumulator rate (added every clock)
```

### Sawtooth Period High ($B002)

```
7  bit  0
---- ----
E... PPPP
|    ||||
|    ++++- Period bits 8-11
+--------- Channel enable
```

## Implementation

```rust
pub struct Vrc6 {
    // Mapper state
    prg_rom: Vec<u8>,
    prg_ram: [u8; 8192],
    chr_mem: Vec<u8>,
    chr_is_ram: bool,

    // Bank registers
    prg_bank_16k: usize,
    prg_bank_8k: usize,
    chr_banks: [usize; 8],

    // Control
    mirroring: Mirroring,
    prg_ram_enabled: bool,

    // IRQ
    irq_latch: u8,
    irq_counter: u8,
    irq_enable: bool,
    irq_enable_after_ack: bool,
    irq_mode: bool, // false = scanline, true = cycle
    irq_pending: bool,
    irq_prescaler: u16,

    // Expansion audio
    pulse1: Vrc6Pulse,
    pulse2: Vrc6Pulse,
    sawtooth: Vrc6Sawtooth,

    // A0/A1 swap for VRC6b
    addr_swap: bool,
}

impl Vrc6 {
    pub fn new(prg_rom: &[u8], chr: &[u8], is_vrc6b: bool) -> Self {
        Self {
            prg_rom: prg_rom.to_vec(),
            prg_ram: [0; 8192],
            chr_mem: chr.to_vec(),
            chr_is_ram: chr.is_empty(),
            prg_bank_16k: 0,
            prg_bank_8k: 0,
            chr_banks: [0; 8],
            mirroring: Mirroring::Vertical,
            prg_ram_enabled: false,
            irq_latch: 0,
            irq_counter: 0,
            irq_enable: false,
            irq_enable_after_ack: false,
            irq_mode: false,
            irq_pending: false,
            irq_prescaler: 0,
            pulse1: Vrc6Pulse::new(),
            pulse2: Vrc6Pulse::new(),
            sawtooth: Vrc6Sawtooth::new(),
            addr_swap: is_vrc6b,
        }
    }

    fn translate_addr(&self, addr: u16) -> u16 {
        if self.addr_swap {
            // VRC6b: swap A0 and A1
            let a0 = addr & 0x0001;
            let a1 = addr & 0x0002;
            (addr & 0xFFFC) | (a0 << 1) | (a1 >> 1)
        } else {
            addr
        }
    }

    fn write_register(&mut self, addr: u16, value: u8) {
        let addr = self.translate_addr(addr);

        match addr & 0xF003 {
            // PRG banking
            0x8000..=0x8003 => {
                self.prg_bank_16k = (value & 0x0F) as usize;
            }
            0xC000..=0xC003 => {
                self.prg_bank_8k = (value & 0x1F) as usize;
            }

            // Pulse 1
            0x9000 => self.pulse1.write_control(value),
            0x9001 => self.pulse1.write_period_low(value),
            0x9002 => self.pulse1.write_period_high(value),

            // Pulse 2
            0xA000 => self.pulse2.write_control(value),
            0xA001 => self.pulse2.write_period_low(value),
            0xA002 => self.pulse2.write_period_high(value),

            // Sawtooth
            0xB000 => self.sawtooth.write_accumulator_rate(value),
            0xB001 => self.sawtooth.write_period_low(value),
            0xB002 => self.sawtooth.write_period_high(value),

            // Mirroring
            0xB003 => {
                self.mirroring = match value & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SingleScreenLower,
                    3 => Mirroring::SingleScreenUpper,
                    _ => unreachable!(),
                };
                self.prg_ram_enabled = value & 0x80 != 0;
            }

            // CHR banking
            0xD000 => self.chr_banks[0] = value as usize,
            0xD001 => self.chr_banks[1] = value as usize,
            0xD002 => self.chr_banks[2] = value as usize,
            0xD003 => self.chr_banks[3] = value as usize,
            0xE000 => self.chr_banks[4] = value as usize,
            0xE001 => self.chr_banks[5] = value as usize,
            0xE002 => self.chr_banks[6] = value as usize,
            0xE003 => self.chr_banks[7] = value as usize,

            // IRQ
            0xF000 => self.irq_latch = value,
            0xF001 => {
                self.irq_enable_after_ack = value & 0x01 != 0;
                self.irq_enable = value & 0x02 != 0;
                self.irq_mode = value & 0x04 != 0;
                if value & 0x02 != 0 {
                    self.irq_counter = self.irq_latch;
                    self.irq_prescaler = 341;
                }
                self.irq_pending = false;
            }
            0xF002 => {
                self.irq_pending = false;
                self.irq_enable = self.irq_enable_after_ack;
            }

            _ => {}
        }
    }

    /// Clock the IRQ counter (called every CPU cycle)
    pub fn clock_irq(&mut self) {
        if !self.irq_enable {
            return;
        }

        if self.irq_mode {
            // CPU cycle mode
            self.clock_irq_counter();
        } else {
            // Scanline mode (prescaler divides by 114)
            self.irq_prescaler = self.irq_prescaler.saturating_sub(3);
            if self.irq_prescaler == 0 {
                self.irq_prescaler = 341;
                self.clock_irq_counter();
            }
        }
    }

    fn clock_irq_counter(&mut self) {
        if self.irq_counter == 0xFF {
            self.irq_counter = self.irq_latch;
            self.irq_pending = true;
        } else {
            self.irq_counter += 1;
        }
    }

    /// Get audio output (-1.0 to 1.0)
    pub fn audio_output(&mut self) -> f32 {
        let pulse1 = self.pulse1.output() as f32;
        let pulse2 = self.pulse2.output() as f32;
        let saw = self.sawtooth.output() as f32;

        // Mix channels (approximate mixing ratio)
        let total = pulse1 + pulse2 + saw;
        total / 45.0 // Normalize to roughly -1.0 to 1.0
    }
}

/// VRC6 Pulse channel
pub struct Vrc6Pulse {
    volume: u8,
    duty: u8,
    mode: bool,
    enabled: bool,
    period: u16,
    timer: u16,
    phase: u8,
}

impl Vrc6Pulse {
    pub fn new() -> Self {
        Self {
            volume: 0,
            duty: 0,
            mode: false,
            enabled: false,
            period: 0,
            timer: 0,
            phase: 0,
        }
    }

    pub fn write_control(&mut self, value: u8) {
        self.volume = value & 0x0F;
        self.duty = (value >> 4) & 0x07;
        self.mode = value & 0x80 != 0;
    }

    pub fn write_period_low(&mut self, value: u8) {
        self.period = (self.period & 0x0F00) | (value as u16);
    }

    pub fn write_period_high(&mut self, value: u8) {
        self.period = (self.period & 0x00FF) | ((value as u16 & 0x0F) << 8);
        self.enabled = value & 0x80 != 0;
    }

    pub fn clock(&mut self) {
        if !self.enabled {
            return;
        }

        if self.timer == 0 {
            self.timer = self.period;
            self.phase = (self.phase + 1) & 0x0F;
        } else {
            self.timer -= 1;
        }
    }

    pub fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }

        if self.mode {
            // Ignore duty, always output volume
            self.volume
        } else {
            // Output volume if phase <= duty
            if self.phase <= self.duty {
                self.volume
            } else {
                0
            }
        }
    }
}

/// VRC6 Sawtooth channel
pub struct Vrc6Sawtooth {
    accumulator_rate: u8,
    enabled: bool,
    period: u16,
    timer: u16,
    accumulator: u8,
    step: u8,
}

impl Vrc6Sawtooth {
    pub fn new() -> Self {
        Self {
            accumulator_rate: 0,
            enabled: false,
            period: 0,
            timer: 0,
            accumulator: 0,
            step: 0,
        }
    }

    pub fn write_accumulator_rate(&mut self, value: u8) {
        self.accumulator_rate = value & 0x3F;
    }

    pub fn write_period_low(&mut self, value: u8) {
        self.period = (self.period & 0x0F00) | (value as u16);
    }

    pub fn write_period_high(&mut self, value: u8) {
        self.period = (self.period & 0x00FF) | ((value as u16 & 0x0F) << 8);
        self.enabled = value & 0x80 != 0;
    }

    pub fn clock(&mut self) {
        if !self.enabled {
            return;
        }

        if self.timer == 0 {
            self.timer = self.period;
            self.step = (self.step + 1) % 14;

            if self.step == 0 {
                self.accumulator = 0;
            } else if self.step % 2 == 0 {
                self.accumulator += self.accumulator_rate;
            }
        } else {
            self.timer -= 1;
        }
    }

    pub fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        // Output top 5 bits of accumulator
        self.accumulator >> 3
    }
}
```

## Audio Mixing

VRC6 expansion audio should be mixed with the standard APU output:

```rust
fn mix_audio(apu_output: f32, vrc6_output: f32) -> f32 {
    // VRC6 is approximately 25% of total volume
    let vrc6_scaled = vrc6_output * 0.25;
    apu_output * 0.75 + vrc6_scaled
}
```

## Notable Games

- Akumajou Densetsu (Castlevania III Japan)
- Madara
- Esper Dream 2

## References

- [NESdev Wiki: VRC6](https://www.nesdev.org/wiki/VRC6)
- [NESdev Wiki: VRC6 audio](https://www.nesdev.org/wiki/VRC6_audio)

# APU DMC Channel Specification (2A03)

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** Complete technical reference for NES APU Delta Modulation Channel

---

## Table of Contents

- [Overview](#overview)
- [Channel Architecture](#channel-architecture)
- [Register Interface](#register-interface)
- [Delta Modulation Playback](#delta-modulation-playback)
- [Memory Reader and DMA](#memory-reader-and-dma)
- [Sample Address and Length](#sample-address-and-length)
- [Output Unit](#output-unit)
- [IRQ Generation](#irq-generation)
- [DMA Conflicts](#dma-conflicts)
- [Implementation Guide](#implementation-guide)
- [Common Pitfalls](#common-pitfalls)
- [Testing and Validation](#testing-and-validation)

---

## Overview

The NES APU contains **one Delta Modulation Channel (DMC)** that plays 1-bit delta-encoded samples from CPU memory. Unlike other channels, the DMC reads sample data via **Direct Memory Access (DMA)**, which stalls the CPU.

**Key Characteristics:**
- 1-bit delta modulation (increment/decrement output level)
- 7-bit output counter (0-127)
- Reads samples from CPU memory ($C000-$FFFF typically)
- 16 selectable sample rates (4.2 kHz - 33.1 kHz)
- DMA steals CPU cycles (4 cycles per sample byte)
- IRQ capability on sample completion
- Loop support for continuous playback
- Typical use: drum samples, voice clips, sampled instruments

**Why Delta Modulation?**

True PCM sample playback requires dedicated memory and DAC hardware. Delta modulation **stores only changes** (+1 or -1), requiring minimal memory. The output level integrates these deltas, reconstructing the original waveform.

---

## Channel Architecture

The DMC channel consists of **six interconnected units**:

```
┌──────────────────────────────────────────────────────────────┐
│                      DMC Channel                             │
│                                                              │
│  ┌─────────────┐    ┌──────────┐    ┌─────────┐             │
│  │ Memory      │───>│  Sample  │───>│ Output  │             │
│  │ Reader      │    │  Buffer  │    │ Shifter │             │
│  │ (DMA)       │    │  8-bit   │    │ 8-bit   │             │
│  └─────────────┘    └──────────┘    └─────────┘             │
│         │                                  │                 │
│         ▼                                  ▼                 │
│  ┌─────────────┐                    ┌──────────┐             │
│  │  Address    │                    │  Output  │             │
│  │  Counter    │                    │  Level   │             │
│  │  15-bit     │                    │  7-bit   │             │
│  └─────────────┘                    └──────────┘             │
│         │                                  │                 │
│         ▼                                  ▼                 │
│  ┌─────────────┐                    7-bit Output (0-127)    │
│  │   Bytes     │                                             │
│  │ Remaining   │                                             │
│  │   12-bit    │                                             │
│  └─────────────┘                                             │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

**Signal Flow:**
1. **Timer** counts down using selected rate from lookup table
2. **Memory Reader** fetches sample bytes via DMA when buffer empty
3. **Sample Buffer** holds current byte being processed
4. **Output Shifter** processes buffer one bit at a time
5. **Output Level** increments or decrements based on bit value

---

## Register Interface

### Complete Register Map

| Address | Bits | Description |
|---------|------|-------------|
| **$4010** | IL-- RRRR | IRQ enable, Loop, Rate index |
| **$4011** | -DDD DDDD | Direct load (7-bit output level) |
| **$4012** | AAAA AAAA | Sample address = $C000 + (A × $40) |
| **$4013** | LLLL LLLL | Sample length = (L × $10) + 1 bytes |

### Register $4010 - Flags and Rate

```
IL-- RRRR
||   ||||
||   ++++- Rate index (R): 0-15, selects sample rate
|+-------- Loop flag (L): 1 = restart on completion
+--------- IRQ enable (I): 1 = generate IRQ on completion
```

**Bit Definitions:**
- **I (IRQ Enable)**: If 1, DMC IRQ flag is set when sample completes
- **L (Loop)**: If 1, sample automatically restarts when bytes remaining reaches 0
- **RRRR (Rate)**: Index into rate table (see [Rate Table](#rate-table))

**Important:** Clearing the IRQ enable flag (I=0) clears the DMC interrupt flag.

### Register $4011 - Direct Load

```
-DDD DDDD
 ||| ||||
 +++-++++- Direct load value (D): Sets output level directly
```

**Purpose:** Allows games to manually control the output level without sample playback.

**Side Effects:**
- Output level counter is set to D (0-127)
- Sample playback continues normally if active

**Hardware Quirk:** If the timer outputs a clock simultaneously with a $4011 write, the output level may not change. This is a rare edge case.

**Typical Uses:**
- Setting initial output level before sample playback
- Creating simple waveforms via rapid writes
- Clearing residual DC offset after sample completion

### Register $4012 - Sample Address

```
AAAA AAAA
|||| ||||
++++-++++- Sample address high byte (A)
```

**Address Calculation:**
```
Sample Address = $C000 + (A × $40)

Examples:
  $00 → $C000
  $01 → $C040
  $40 → $D000
  $FF → $FFC0
```

**Address Range:** $C000-$FFC0 (samples typically in PRG-ROM)

**Important:** This register only sets the **starting address**. The actual current address is tracked internally and increments during playback.

### Register $4013 - Sample Length

```
LLLL LLLL
|||| ||||
++++-++++- Sample length (L)
```

**Length Calculation:**
```
Sample Length = (L × $10) + 1 bytes

Examples:
  $00 → 1 byte
  $01 → 17 bytes
  $10 → 257 bytes
  $FF → 4081 bytes (maximum)
```

**Note:** Maximum sample length is 4081 bytes (~123ms at highest rate).

---

## Delta Modulation Playback

### What is Delta Modulation?

**Traditional PCM:** Stores absolute sample values (8-bit: 0-255)
```
Samples: [64, 65, 67, 70, 68, 65, ...]
```

**Delta Modulation:** Stores only changes (+1 or -1)
```
Bits:    [1, 1, 1, 0, 0, ...]  (1=+1, 0=-1)
```

**Output Reconstruction:**
```
Start: 64
Bit 1 (1): 64 + 1 = 65
Bit 2 (1): 65 + 1 = 66
Bit 3 (1): 66 + 1 = 67
Bit 4 (0): 67 - 1 = 66
...
```

### Output Unit Operation

The DMC maintains a **7-bit output level** (0-127) that is incremented or decremented based on sample bits:

```rust
if sample_bit == 1 {
    if output_level <= 125 {
        output_level += 2;
    }
} else {
    if output_level >= 2 {
        output_level -= 2;
    }
}
```

**Key Behaviors:**
- Output changes by ±2 (not ±1)
- Clamping prevents overflow (0-127 range)
- No change if already at boundary

**Why ±2?** The APU mixer expects even values for proper mixing with other channels.

### Output Shifter

The output shifter processes sample bytes **one bit at a time**:

```
8-bit Sample Buffer: [b7 b6 b5 b4 b3 b2 b1 b0]

Timer clocks output shifter:
  Clock 1: Process b0, shift right
  Clock 2: Process b1, shift right
  ...
  Clock 8: Process b7, shift right → buffer empty
```

**Bit Order:** Least significant bit (b0) is processed first.

### Silence Bit

If the sample buffer is empty and bits remaining > 0, the output shifter uses a **silence bit**:

```rust
if sample_buffer_empty {
    // Silence bit (always 0) → decrement output level
    if output_level >= 2 {
        output_level -= 2;
    }
}
```

This prevents the channel from "hanging" if DMA can't keep up.

---

## Memory Reader and DMA

### Sample Fetch Mechanism

When the sample buffer empties:

1. **Check bytes remaining**: If > 0, initiate fetch
2. **DMA read**: Stall CPU for 1-4 cycles (see [DMA Timing](#dma-timing))
3. **Load buffer**: Store fetched byte in sample buffer
4. **Update address**: Increment address, wrapping $FFFF → $8000
5. **Update length**: Decrement bytes remaining
6. **Check completion**: If bytes = 0, handle loop/IRQ

### Address Counter Behavior

```rust
// Fetch sample byte
let sample = cpu_memory.read(current_address);

// Increment address (wraps to $8000, not $0000!)
if current_address == 0xFFFF {
    current_address = 0x8000;
} else {
    current_address += 1;
}

// Decrement bytes remaining
bytes_remaining -= 1;
```

**Critical Detail:** Address wraps from $FFFF to $8000, **not $0000**. This prevents DMC from accidentally reading from RAM/PPU/APU registers.

### Sample Completion

When bytes remaining reaches 0:

**If Loop Flag = 1:**
```rust
current_address = sample_start_address;
bytes_remaining = sample_length;
// Continue playback
```

**If Loop Flag = 0:**
```rust
// Stop playback
sample_buffer_empty = true;
bits_remaining = 0;

// Set IRQ flag if enabled
if irq_enabled {
    dmc_irq_flag = true;
}
```

### DMA Timing

Each sample fetch steals **1-4 CPU cycles**:

```
Best case:  1 CPU cycle  (aligned fetch)
Typical:    3 CPU cycles
Worst case: 4 CPU cycles (fetch during opcode read)
```

**Factors Affecting Duration:**
- CPU instruction phase (fetch/decode/execute)
- OAM DMA active (additional +1-2 cycles)
- Alignment with CPU bus access

**Implementation Note:** Accurate cycle counting requires tracking CPU instruction state.

---

## Sample Address and Length

### Address Calculation Examples

| Register $4012 | Hex Address | Use Case |
|----------------|-------------|----------|
| $00 | $C000 | Start of CHR-ROM window (if mapping allows) |
| $20 | $C800 | Common sample location |
| $40 | $D000 | Mid PRG-ROM |
| $80 | $E000 | High PRG-ROM |
| $C0 | $F000 | Top PRG-ROM |
| $FF | $FFC0 | Near interrupt vectors (avoid!) |

### Length Calculation Examples

| Register $4013 | Byte Length | Duration @ 33 kHz | Duration @ 4.2 kHz |
|----------------|-------------|-------------------|-------------------|
| $01 | 17 | 0.51 ms | 4.0 ms |
| $10 | 257 | 7.8 ms | 61 ms |
| $40 | 1025 | 31 ms | 244 ms |
| $80 | 2049 | 62 ms | 488 ms |
| $FF | 4081 | 123 ms | 972 ms |

### Sample Storage Strategies

**Strategy 1: Fixed Location (Simple)**
```
PRG-ROM $E000-$EFFF: 4KB sample bank
$4012 = $80 (address = $E000)
$4013 = $FF (length = 4081 bytes)
```

**Strategy 2: Mapper Banking (Flexible)**
```
Use mapper (MMC3/MMC5) to bank samples into $C000-$DFFF
$4012 varies based on current bank
```

**Strategy 3: Multiple Small Samples**
```
$C000-$C7FF: 8 samples × 256 bytes
$4012 = $00, $04, $08, $0C, $10, $14, $18, $1C
$4013 = $0F (256 bytes each)
```

---

## Output Unit

### Output Level Range

The DMC output is a **7-bit value** (0-127), providing 128 discrete levels.

**Comparison with Other Channels:**
- Pulse/Noise: 4-bit (0-15)
- Triangle: 4-bit (0-15)
- DMC: 7-bit (0-127)

**Mixer Weighting:** DMC has significantly more dynamic range, but the mixer applies non-linear weighting.

### Direct Output Control

Games can write $4011 to manually control output level:

**Example: Generate Triangle Wave**
```rust
// 32-step triangle wave
const TRIANGLE: [u8; 32] = [
    64, 68, 72, 76, 80, 84, 88, 92,
    96, 100, 104, 108, 112, 116, 120, 124,
    127, 124, 120, 116, 112, 108, 104, 100,
    96, 92, 88, 84, 80, 76, 72, 68,
];

for &level in &TRIANGLE {
    apu.write(0x4011, level);
    wait_some_cycles();
}
```

**Example: Bass Drum**
```rust
// Sharp attack, exponential decay
apu.write(0x4011, 127);  // Maximum
wait(100);
apu.write(0x4011, 80);
wait(200);
apu.write(0x4011, 50);
wait(300);
apu.write(0x4011, 30);
wait(400);
apu.write(0x4011, 64);   // Return to center
```

---

## IRQ Generation

### IRQ Enable and Flag

**IRQ Enable Bit (I in $4010):**
- If 1: DMC IRQ flag is set when sample completes
- If 0: No IRQ generated (and flag is cleared immediately)

**IRQ Flag:**
- Set when: Sample completes (bytes remaining = 0) with loop disabled
- Cleared by: Reading $4015 or writing $4010 with I=0

### IRQ Timing

```
Byte 1 read → 8 bits processed → buffer empty
Byte 2 read → 8 bits processed → buffer empty
...
Last byte read → 8 bits processed → buffer empty
                                  ↓
                       bytes_remaining = 0
                       If IRQ enabled: set IRQ flag
```

**Important:** IRQ is set **after** the last bit is processed, not when the last byte is read.

### Handling DMC IRQ

```rust
fn check_apu_irq(&mut self) -> bool {
    let status = self.apu.read(0x4015);

    // Bit 7: DMC IRQ flag
    if (status & 0x80) != 0 {
        // DMC sample completed
        return true;
    }

    false
}
```

**Usage Example: Streamed Audio**
```
1. Start sample playback (1KB chunk)
2. Wait for DMC IRQ
3. Bank in next 1KB chunk
4. Restart playback ($4015 write)
5. Repeat
```

---

## DMA Conflicts

### Controller Read Corruption (NTSC Only)

**Problem:** DMC sample fetch can corrupt controller reads on NTSC systems.

**Mechanism:**
1. CPU reads $4016/$4017 (controller ports)
2. DMC DMA pulls RDY low, halting CPU
3. DMC drives address bus for sample fetch
4. Controller sees extra clock edge
5. Controller shifts an extra bit

**Result:** Controller reads return garbage (button presses missed or phantom inputs).

**Workaround Strategies:**

**1. Read Multiple Times**
```rust
let read1 = controller.read();
let read2 = controller.read();
let read3 = controller.read();

// Use majority vote or check consistency
if read1 == read2 || read1 == read3 {
    return read1;
} else {
    return read2;
}
```

**2. Disable DMC During Input Read**
```rust
// Save DMC state
let dmc_state = apu.read(0x4015);

// Disable DMC
apu.write(0x4015, dmc_state & 0b11101111);

// Read controllers
let input = controller.read();

// Re-enable DMC
apu.write(0x4015, dmc_state);
```

**3. Use Slower Sample Rates**
Lower DMC frequencies reduce conflict probability.

### PPU Read Corruption

**Problem:** DMC DMA during $2007 (PPU data) read causes address increment glitches.

**Mechanism:**
1. CPU reads $2007
2. DMC DMA interrupts
3. PPU address increments multiple times
4. Wrong data returned

**Workaround:** Avoid reading $2007 during DMC playback, or account for extra increments.

### OAM DMA Conflict

**Problem:** DMC sample fetch during OAM DMA ($4014) extends the stall time.

**Timing:**
- Normal OAM DMA: 513-514 cycles
- With DMC fetch: 515-516 cycles (additional +1-2 cycles)

**Workaround:** None needed for most games (slight delay is acceptable).

### PAL vs NTSC Differences

**NTSC (2A03):** DMC conflicts affect controllers and PPU
**PAL (2A07):** DMC conflicts are **significantly reduced** or absent

Games targeting both regions should test on NTSC hardware.

---

## Implementation Guide

### Complete DMC Channel Structure

```rust
pub struct DmcChannel {
    // Configuration
    irq_enabled: bool,
    loop_enabled: bool,
    rate_index: u8,

    // Sample playback
    sample_address: u16,      // $4012 value
    sample_length: u16,       // $4013 value
    current_address: u16,     // Current read address
    bytes_remaining: u16,     // Bytes left to read

    // Output units
    sample_buffer: u8,        // 8-bit buffer
    sample_buffer_empty: bool,
    bits_remaining: u8,       // 0-8
    output_level: u8,         // 0-127 (7-bit)

    // Timer
    timer: u16,
    rate_table: [u16; 16],

    // IRQ
    irq_flag: bool,

    // Enable flag from $4015
    enabled: bool,
}

impl DmcChannel {
    pub fn new(system: System) -> Self {
        let rate_table = match system {
            System::NTSC => DMC_RATE_NTSC,
            System::PAL => DMC_RATE_PAL,
        };

        Self {
            irq_enabled: false,
            loop_enabled: false,
            rate_index: 0,
            sample_address: 0xC000,
            sample_length: 1,
            current_address: 0xC000,
            bytes_remaining: 0,
            sample_buffer: 0,
            sample_buffer_empty: true,
            bits_remaining: 0,
            output_level: 64,  // Start at center
            timer: rate_table[0],
            rate_table,
            irq_flag: false,
            enabled: false,
        }
    }

    /// Clock the timer (every APU cycle)
    pub fn clock_timer(&mut self, memory: &mut dyn Memory) -> u8 {
        let mut dma_cycles = 0;

        // Clock timer
        if self.timer == 0 {
            self.timer = self.rate_table[self.rate_index as usize];

            // Clock output shifter
            if self.bits_remaining > 0 {
                self.clock_output_shifter();
            }

            // Refill buffer if empty
            if self.sample_buffer_empty && self.bytes_remaining > 0 {
                dma_cycles = self.fetch_sample(memory);
            }
        } else {
            self.timer -= 1;
        }

        dma_cycles
    }

    /// Clock output shifter
    fn clock_output_shifter(&mut self) {
        if self.sample_buffer_empty {
            // Use silence bit (0) → decrement
            if self.output_level >= 2 {
                self.output_level -= 2;
            }
        } else {
            // Process LSB
            if (self.sample_buffer & 1) == 1 {
                if self.output_level <= 125 {
                    self.output_level += 2;
                }
            } else {
                if self.output_level >= 2 {
                    self.output_level -= 2;
                }
            }

            // Shift buffer
            self.sample_buffer >>= 1;
        }

        self.bits_remaining -= 1;

        // Check if buffer is now empty
        if self.bits_remaining == 0 {
            self.sample_buffer_empty = true;
        }
    }

    /// Fetch sample byte via DMA
    fn fetch_sample(&mut self, memory: &mut dyn Memory) -> u8 {
        // Read byte from memory (DMA)
        self.sample_buffer = memory.read(self.current_address);
        self.sample_buffer_empty = false;
        self.bits_remaining = 8;

        // Increment address (wrap $FFFF → $8000)
        if self.current_address == 0xFFFF {
            self.current_address = 0x8000;
        } else {
            self.current_address += 1;
        }

        // Decrement bytes remaining
        self.bytes_remaining -= 1;

        // Handle completion
        if self.bytes_remaining == 0 {
            if self.loop_enabled {
                self.restart_sample();
            } else if self.irq_enabled {
                self.irq_flag = true;
            }
        }

        // Return DMA stall cycles (3 typical, can be 1-4)
        3
    }

    /// Restart sample playback
    fn restart_sample(&mut self) {
        self.current_address = 0xC000 | ((self.sample_address as u16) << 6);
        self.bytes_remaining = ((self.sample_length as u16) << 4) | 1;
    }

    /// Get output level (0-127)
    pub fn output(&self) -> u8 {
        if self.enabled {
            self.output_level
        } else {
            0
        }
    }

    /// Write to $4010
    pub fn write_flags_rate(&mut self, value: u8) {
        self.irq_enabled = (value & 0x80) != 0;
        self.loop_enabled = (value & 0x40) != 0;
        self.rate_index = value & 0x0F;

        // Clearing IRQ enable clears flag
        if !self.irq_enabled {
            self.irq_flag = false;
        }
    }

    /// Write to $4011
    pub fn write_output_level(&mut self, value: u8) {
        self.output_level = value & 0x7F;
    }

    /// Write to $4012
    pub fn write_sample_address(&mut self, value: u8) {
        self.sample_address = value as u16;
    }

    /// Write to $4013
    pub fn write_sample_length(&mut self, value: u8) {
        self.sample_length = value as u16;
    }
}
```

### Rate Table

```rust
const DMC_RATE_NTSC: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214,
    190, 160, 142, 128, 106, 84, 72, 54,
];

const DMC_RATE_PAL: [u16; 16] = [
    398, 354, 316, 298, 276, 236, 210, 198,
    176, 148, 132, 118, 98, 78, 66, 50,
];
```

**Frequency Calculation:**
```
f_DMC = f_CPU / (rate × 8)

NTSC examples:
  Rate $0F: 1,789,773 / (54 × 8) ≈ 4.1 kHz
  Rate $00: 1,789,773 / (428 × 8) ≈ 520 Hz
```

---

## Common Pitfalls

### 1. Address Wrap to $0000

**Problem:** Wrapping address from $FFFF to $0000 instead of $8000.

**Solution:** Always wrap to $8000.

```rust
// WRONG
current_address = (current_address + 1) & 0xFFFF;

// CORRECT
if current_address == 0xFFFF {
    current_address = 0x8000;
} else {
    current_address += 1;
}
```

### 2. Ignoring DMA Cycles

**Problem:** Not accounting for CPU stall during sample fetch.

**Solution:** Return DMA cycle count and add to CPU timing.

```rust
let dma_cycles = dmc.clock_timer(memory);
cpu_cycles += dma_cycles as u64;
```

### 3. ±1 Instead of ±2

**Problem:** Changing output level by ±1 instead of ±2.

**Solution:** Always use ±2.

```rust
// WRONG
if bit == 1 { output_level += 1; }

// CORRECT
if bit == 1 && output_level <= 125 { output_level += 2; }
```

### 4. IRQ Flag Not Clearing

**Problem:** Not clearing IRQ flag when reading $4015 or writing $4010.

**Solution:** Clear on both operations.

```rust
// On $4015 read
let status = (dmc_irq_flag as u8) << 7 | ...;
dmc_irq_flag = false;  // Reading clears

// On $4010 write with IRQ disabled
if (value & 0x80) == 0 {
    dmc_irq_flag = false;
}
```

---

## Testing and Validation

### Test ROMs

| ROM | Tests | Pass Criteria |
|-----|-------|---------------|
| **apu_test** | Basic DMC functionality | All tests pass |
| **blargg_apu_2005.nes** | Comprehensive DMC behavior | Text output "Passed" |
| **dmc_basics.nes** | DMC fundamentals | Correct playback |
| **dmc_dma_during_read4.nes** | DMA conflicts | Proper handling |
| **dmc_rates.nes** | Sample rate accuracy | Correct frequencies |

### Manual Testing

**Sample Playback Test:**
```
Configure: Rate=$0F, Address=$C000, Length=$10 (257 bytes)
Enable DMC via $4015
Verify: Sample plays correctly, IRQ fires on completion
```

**Direct Output Test:**
```
For level in 0..128:
    Write $4011 = level
    Verify: Output equals level (audible as click/pop)
```

**DMA Stall Test:**
```
Run CPU-intensive code with DMC active
Measure: Execution time increases proportionally to DMC rate
```

---

## Related Documentation

- [APU_OVERVIEW.md](APU_OVERVIEW.md) - General APU architecture
- [APU_TIMING.md](APU_TIMING.md) - Frame counter and timing details
- [BUS_ARCHITECTURE.md](../bus/BUS_ARCHITECTURE.md) - DMA and memory access
- [MEMORY_MAP.md](../bus/MEMORY_MAP.md) - CPU memory layout

---

## References

- [NESdev Wiki: APU DMC](https://www.nesdev.org/wiki/APU_DMC)
- [NESdev Wiki: DMC DMA](https://www.nesdev.org/wiki/APU_DMC#DMA)
- [NESdev Wiki: APU DMC Conflict](https://www.nesdev.org/wiki/APU_DMC#Conflict)
- Blargg APU Test ROMs - DMC validation
- Visual 2A03 - Hardware simulation

---

**Document Status:** Complete specification for DMC channel implementation with DMA cycle accounting.

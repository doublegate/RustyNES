# [Milestone 3] Sprint 3.4: DMC Channel

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** ~1-2 weeks
**Assignee:** Claude Code / Developer
**Dependencies:** Sprint 3.1 (APU Core) must be complete

---

## Overview

Implement the Delta Modulation Channel (DMC) for sample playback. This is the most complex APU channel, reading 1-bit delta-encoded samples from CPU memory via DMA, which stalls the CPU. Essential for drum samples, voice clips, and sound effects in many games.

---

## Acceptance Criteria

- [ ] DMC sample playback from CPU memory
- [ ] 16 sample rates (4.2 kHz to 33.1 kHz)
- [ ] DMA with CPU cycle stealing
- [ ] 7-bit output level with delta counter
- [ ] Sample address and length calculation
- [ ] Loop flag support
- [ ] IRQ on sample completion
- [ ] Direct load register ($4011)
- [ ] Zero unsafe code
- [ ] Comprehensive unit tests

---

## Tasks

### 3.4.1 DMC Channel Structure

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Create the DmcChannel struct with sample buffer, output unit, and DMA interface.

**Files:**

- `crates/rustynes-apu/src/dmc.rs` - DMC channel implementation

**Subtasks:**

- [ ] Define DmcChannel struct
- [ ] Sample buffer (8-bit shift register)
- [ ] Output unit (7-bit counter)
- [ ] Timer with rate lookup table
- [ ] Sample address and bytes remaining
- [ ] IRQ flag and loop flag
- [ ] Silence flag

**Implementation:**

```rust
pub struct DmcChannel {
    // Timer
    timer_period: u16,         // From rate lookup table
    timer_counter: u16,        // Current timer value

    // Output unit
    output_level: u8,          // 7-bit counter (0-127)
    silence_flag: bool,

    // Sample buffer
    sample_buffer: u8,         // 8-bit shift register
    bits_remaining: u8,        // 0-8 bits in buffer

    // Memory reader
    sample_address: u16,       // Current address ($C000-$FFFF)
    sample_length: u16,        // Bytes remaining
    current_address: u16,      // Current read address
    bytes_remaining: u16,      // Current bytes left

    // Control
    irq_enabled: bool,
    loop_flag: bool,
    irq_flag: bool,

    // State
    enabled: bool,
}

impl DmcChannel {
    pub fn new() -> Self {
        Self {
            timer_period: 0,
            timer_counter: 0,
            output_level: 0,
            silence_flag: true,
            sample_buffer: 0,
            bits_remaining: 0,
            sample_address: 0,
            sample_length: 0,
            current_address: 0,
            bytes_remaining: 0,
            irq_enabled: false,
            loop_flag: false,
            irq_flag: false,
            enabled: false,
        }
    }
}

// DMC rate table (NTSC)
const DMC_RATE_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214,
    190, 160, 142, 128, 106,  84,  72,  54,
];
```

---

### 3.4.2 DMC Register Interface

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement the 4-register interface for the DMC channel ($4010-$4013).

**Files:**

- `crates/rustynes-apu/src/dmc.rs` - Register handlers

**Subtasks:**

- [ ] Register $4010: IRQ enable, loop, rate
- [ ] Register $4011: Direct load (7-bit output level)
- [ ] Register $4012: Sample address ($C000 + value × $40)
- [ ] Register $4013: Sample length (value × $10 + 1)

**Implementation:**

```rust
impl DmcChannel {
    pub fn write_register(&mut self, addr: u8, value: u8) {
        match addr {
            0 => {
                // $4010: IL-- RRRR
                self.irq_enabled = (value & 0x80) != 0;
                self.loop_flag = (value & 0x40) != 0;

                let rate_index = value & 0x0F;
                self.timer_period = DMC_RATE_TABLE[rate_index as usize];

                // Clear IRQ flag if disabled
                if !self.irq_enabled {
                    self.irq_flag = false;
                }
            }
            1 => {
                // $4011: -DDD DDDD (direct load)
                self.output_level = value & 0x7F;
            }
            2 => {
                // $4012: AAAA AAAA
                // Sample address = $C000 + (value * $40)
                self.sample_address = 0xC000 + ((value as u16) * 0x40);
            }
            3 => {
                // $4013: LLLL LLLL
                // Sample length = (value * $10) + 1
                self.sample_length = ((value as u16) * 0x10) + 1;
            }
            _ => {}
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;

        if !enabled {
            self.bytes_remaining = 0;
        } else if self.bytes_remaining == 0 {
            // Restart sample
            self.restart_sample();
        }
    }

    fn restart_sample(&mut self) {
        self.current_address = self.sample_address;
        self.bytes_remaining = self.sample_length;
    }
}
```

---

### 3.4.3 Memory Reader and DMA

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 3 hours

**Description:**
Implement the memory reader that fetches samples via DMA, stalling the CPU.

**Files:**

- `crates/rustynes-apu/src/dmc.rs` - Memory reader
- `crates/rustynes-apu/src/apu.rs` - DMA interface

**Subtasks:**

- [ ] DMA request callback to CPU
- [ ] Read byte from current address
- [ ] Increment address with wraparound ($C000-$FFFF)
- [ ] Decrement bytes remaining
- [ ] Handle sample completion (loop or IRQ)
- [ ] Fill sample buffer

**Implementation:**

```rust
impl DmcChannel {
    pub fn clock_memory_reader<F>(&mut self, mut read_memory: F)
    where
        F: FnMut(u16) -> u8,
    {
        // Only read if buffer is empty and bytes remaining
        if self.bits_remaining == 0 && self.bytes_remaining > 0 {
            // Request DMA (stalls CPU for 4 cycles)
            self.sample_buffer = read_memory(self.current_address);
            self.bits_remaining = 8;

            // Increment address (with wraparound)
            self.current_address = if self.current_address == 0xFFFF {
                0x8000 // Wraparound to ROM
            } else {
                self.current_address.wrapping_add(1)
            };

            // Decrement bytes remaining
            self.bytes_remaining -= 1;

            // Handle sample completion
            if self.bytes_remaining == 0 {
                if self.loop_flag {
                    self.restart_sample();
                } else if self.irq_enabled {
                    self.irq_flag = true;
                }
            }

            // Clear silence flag
            self.silence_flag = false;
        }
    }

    pub fn needs_dma_read(&self) -> bool {
        self.bits_remaining == 0 && self.bytes_remaining > 0
    }
}

// In apu.rs
impl Apu {
    pub fn clock_dmc<F>(&mut self, read_memory: F)
    where
        F: FnMut(u16) -> u8,
    {
        self.dmc.clock_memory_reader(read_memory);
    }

    pub fn dmc_dma_pending(&self) -> bool {
        self.dmc.needs_dma_read()
    }
}
```

---

### 3.4.4 Output Unit

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement the output unit that processes delta-encoded samples.

**Files:**

- `crates/rustynes-apu/src/dmc.rs` - Output unit logic

**Subtasks:**

- [ ] Timer countdown
- [ ] Shift sample buffer on timer expiration
- [ ] Increment/decrement output level based on bit
- [ ] Clamp output level (0-127)
- [ ] Handle silence flag

**Implementation:**

```rust
impl DmcChannel {
    pub fn clock_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;
            self.clock_output_unit();
        } else {
            self.timer_counter -= 1;
        }
    }

    fn clock_output_unit(&mut self) {
        if !self.silence_flag {
            // Shift out one bit
            let bit = self.sample_buffer & 1;
            self.sample_buffer >>= 1;
            self.bits_remaining -= 1;

            // Update output level based on bit
            if bit == 1 {
                if self.output_level <= 125 {
                    self.output_level += 2;
                }
            } else {
                if self.output_level >= 2 {
                    self.output_level -= 2;
                }
            }

            // Check if buffer is empty
            if self.bits_remaining == 0 {
                self.silence_flag = true;
            }
        }
    }

    pub fn output(&self) -> u8 {
        self.output_level
    }

    pub fn is_active(&self) -> bool {
        self.bytes_remaining > 0
    }

    pub fn irq_pending(&self) -> bool {
        self.irq_flag
    }

    pub fn clear_irq(&mut self) {
        self.irq_flag = false;
    }
}
```

---

### 3.4.5 CPU Cycle Stealing

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement CPU stall logic for DMC DMA reads.

**Files:**

- `crates/rustynes-apu/src/apu.rs` - DMA stall interface
- `crates/rustynes-cpu/src/cpu.rs` - Stall handling

**Subtasks:**

- [ ] DMC DMA steals 4 CPU cycles
- [ ] Additional cycles if during OAM DMA (2-4 cycles)
- [ ] Stall counter in CPU
- [ ] DMA request from APU

**Implementation:**

```rust
// In apu.rs
impl Apu {
    pub fn step<F>(&mut self, mut read_memory: F) -> u8
    where
        F: FnMut(u16) -> u8,
    {
        // Clock DMC timer
        self.dmc.clock_timer();

        // Check if DMC needs DMA read
        let mut stall_cycles = 0;
        if self.dmc.needs_dma_read() {
            self.dmc.clock_memory_reader(&mut read_memory);
            stall_cycles = 4; // DMC DMA steals 4 CPU cycles
        }

        stall_cycles
    }
}

// In cpu.rs
impl Cpu {
    pub fn step(&mut self, bus: &mut impl Bus) -> u8 {
        // Check for DMC DMA stall
        if self.dmc_stall > 0 {
            self.dmc_stall -= 1;
            return 1; // Stalled for 1 cycle
        }

        // Normal instruction execution
        // ...
    }

    pub fn add_dmc_stall(&mut self, cycles: u8) {
        self.dmc_stall += cycles;
    }
}
```

---

### 3.4.6 Sample Format and Decoding

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 1 hour

**Description:**
Document DMC sample format and provide decoding utilities for testing.

**Files:**

- `crates/rustynes-apu/src/dmc.rs` - Documentation
- `crates/rustynes-apu/examples/dmc_decode.rs` - Example decoder

**Subtasks:**

- [ ] Document 1-bit delta encoding format
- [ ] Example: Convert PCM to DMC format
- [ ] Example: Decode DMC to PCM for testing

**Implementation:**

```rust
/// DMC Sample Format:
///
/// DMC samples are 1-bit delta-encoded:
/// - Bit 1: Increment output level by 2
/// - Bit 0: Decrement output level by 2
///
/// Output level is clamped to 0-127 (7-bit).
///
/// Example encoding:
/// PCM samples: [64, 66, 68, 66, 64]
/// Delta:       [ 0, +2, +2, -2, -2]
/// DMC bits:    [ -, 1,  1,  0,  0]

#[cfg(test)]
fn encode_pcm_to_dmc(pcm: &[u8]) -> Vec<u8> {
    let mut dmc = Vec::new();
    let mut prev = 64u8; // Start at center

    for &sample in pcm {
        let bit = if sample > prev { 1 } else { 0 };
        dmc.push(bit);
        prev = sample;
    }

    // Pack bits into bytes (LSB first)
    let mut bytes = Vec::new();
    for chunk in dmc.chunks(8) {
        let mut byte = 0u8;
        for (i, &bit) in chunk.iter().enumerate() {
            byte |= bit << i;
        }
        bytes.push(byte);
    }

    bytes
}
```

---

### 3.4.7 Unit Tests

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 3 hours

**Description:**
Create comprehensive unit tests for DMC channel functionality.

**Files:**

- `crates/rustynes-apu/src/dmc.rs` - Test module

**Subtasks:**

- [ ] Test sample address calculation
- [ ] Test sample length calculation
- [ ] Test memory reader
- [ ] Test output level updates
- [ ] Test loop behavior
- [ ] Test IRQ generation
- [ ] Test direct load
- [ ] Test silence flag

**Tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_address_calculation() {
        let mut dmc = DmcChannel::new();

        // $4012 = $00 → $C000
        dmc.write_register(2, 0x00);
        assert_eq!(dmc.sample_address, 0xC000);

        // $4012 = $FF → $FFC0
        dmc.write_register(2, 0xFF);
        assert_eq!(dmc.sample_address, 0xFFC0);
    }

    #[test]
    fn test_sample_length_calculation() {
        let mut dmc = DmcChannel::new();

        // $4013 = $00 → 1 byte
        dmc.write_register(3, 0x00);
        assert_eq!(dmc.sample_length, 1);

        // $4013 = $FF → 4081 bytes
        dmc.write_register(3, 0xFF);
        assert_eq!(dmc.sample_length, 4081);
    }

    #[test]
    fn test_memory_reader() {
        let mut dmc = DmcChannel::new();
        dmc.sample_address = 0xC000;
        dmc.sample_length = 3;
        dmc.set_enabled(true);

        let memory = vec![0xAA, 0xBB, 0xCC];
        let mut addr_index = 0;

        // Read first byte
        dmc.clock_memory_reader(|addr| {
            assert_eq!(addr, 0xC000 + addr_index);
            addr_index += 1;
            memory[(addr - 0xC000) as usize]
        });

        assert_eq!(dmc.sample_buffer, 0xAA);
        assert_eq!(dmc.bits_remaining, 8);
        assert_eq!(dmc.bytes_remaining, 2);
    }

    #[test]
    fn test_output_unit() {
        let mut dmc = DmcChannel::new();
        dmc.sample_buffer = 0b10101010; // Alternating bits
        dmc.bits_remaining = 8;
        dmc.silence_flag = false;
        dmc.output_level = 64;

        // Clock output unit
        dmc.clock_output_unit();

        // First bit is 0, should decrement
        assert_eq!(dmc.output_level, 62);
        assert_eq!(dmc.bits_remaining, 7);
    }

    #[test]
    fn test_loop_behavior() {
        let mut dmc = DmcChannel::new();
        dmc.sample_address = 0xC000;
        dmc.sample_length = 1;
        dmc.loop_flag = true;
        dmc.set_enabled(true);

        // Read until sample completes
        dmc.clock_memory_reader(|_| 0xFF);

        // Consume all bits
        for _ in 0..8 {
            dmc.clock_output_unit();
        }

        // Check if sample restarted
        assert_eq!(dmc.bytes_remaining, 1);
        assert_eq!(dmc.current_address, 0xC000);
    }

    #[test]
    fn test_irq_generation() {
        let mut dmc = DmcChannel::new();
        dmc.sample_address = 0xC000;
        dmc.sample_length = 1;
        dmc.irq_enabled = true;
        dmc.loop_flag = false;
        dmc.set_enabled(true);

        // Read and consume sample
        dmc.clock_memory_reader(|_| 0xFF);
        for _ in 0..8 {
            dmc.clock_output_unit();
        }

        // IRQ should be set
        assert!(dmc.irq_flag);
    }

    #[test]
    fn test_direct_load() {
        let mut dmc = DmcChannel::new();

        // Write to $4011
        dmc.write_register(1, 0x7F);
        assert_eq!(dmc.output_level, 127);

        dmc.write_register(1, 0x00);
        assert_eq!(dmc.output_level, 0);
    }

    #[test]
    fn test_dmc_rate_table() {
        assert_eq!(DMC_RATE_TABLE[0], 428);  // ~4.2 kHz
        assert_eq!(DMC_RATE_TABLE[15], 54);  // ~33.1 kHz
    }
}
```

---

## Dependencies

**Required:**

- Sprint 3.1 complete (frame counter, APU core)
- rustynes-cpu (for DMA cycle stealing)

**Blocks:**

- Sprint 3.5: Audio output and mixing (needs DMC output)

---

## Related Documentation

- [APU DMC Channel](../../docs/apu/APU_CHANNEL_DMC.md)
- [APU 2A03 Specification](../../docs/apu/APU_2A03_SPECIFICATION.md)
- [APU Overview](../../docs/apu/APU_OVERVIEW.md)
- [NESdev Wiki - APU DMC](https://www.nesdev.org/wiki/APU_DMC)

---

## Technical Notes

### DMC DMA Timing

DMC DMA reads steal CPU cycles:
- **Base cost**: 4 CPU cycles per sample byte
- **During OAM DMA**: Additional 2-4 cycles depending on alignment
- **Rarest case**: Up to 8 cycles total during aligned OAM DMA

### Sample Address Range

DMC can only read from $C000-$FFFF (ROM and upper cartridge space). Address wraps from $FFFF to $8000 (not $C000).

### Output Level Clamping

The 7-bit output level (0-127) is updated in steps of ±2 and clamped:
- Bit 1: Add 2 (max 127)
- Bit 0: Subtract 2 (min 0)

### IRQ vs Loop

If both IRQ enable and loop are set, loop takes priority and IRQ is never triggered.

### Direct Load ($4011)

Writing to $4011 immediately changes the output level without affecting the sample buffer. Used for PCM playback in some games.

---

## Test Requirements

- [ ] Unit tests for sample address/length calculation
- [ ] Unit tests for memory reader
- [ ] Unit tests for output level updates
- [ ] Unit tests for loop behavior
- [ ] Unit tests for IRQ generation
- [ ] Unit tests for direct load
- [ ] Integration test: DMC DMA with CPU cycle stealing
- [ ] Integration test: DMC playback with sample data

---

## Performance Targets

- Timer clock: <10 ns per cycle
- Memory read: <100 ns (includes DMA overhead)
- Output calculation: <15 ns
- Memory: <200 bytes per channel

---

## Success Criteria

- [ ] DMC reads samples from memory via DMA
- [ ] CPU cycles are stolen during DMA reads
- [ ] Output level updates correctly based on delta encoding
- [ ] Loop flag restarts samples
- [ ] IRQ triggers on sample completion
- [ ] Direct load works independently
- [ ] Sample address and length calculations correct
- [ ] All unit tests pass
- [ ] Zero unsafe code
- [ ] Documentation complete

---

**Previous Sprint:** [Sprint 3.3: Triangle & Noise Channels](M3-S3-TRIANGLE-NOISE.md)
**Next Sprint:** [Sprint 3.5: Integration & Testing](M3-S5-INTEGRATION.md)

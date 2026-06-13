# [Milestone 3] Sprint 3.2: Pulse Channels

**Status:** ✅ COMPLETED
**Started:** December 2025
**Completed:** December 2025
**Duration:** ~1 week (actual)
**Assignee:** Claude Code / Developer
**Dependencies:** Sprint 3.1 (APU Core) ✅ Complete

---

## Overview

Implement both pulse wave channels (Pulse 1 and Pulse 2) with duty cycle control, envelope, sweep, length counter, and timer. These channels produce the iconic square wave sounds used for melody and harmony in NES games.

---

## Acceptance Criteria

- [ ] Two independent pulse channels implemented
- [ ] Four duty cycles (12.5%, 25%, 50%, 75%)
- [ ] Hardware envelope generator integrated
- [ ] Frequency sweep with one's/two's complement
- [ ] Length counter integration
- [ ] Timer with 11-bit period
- [ ] Channel silencing logic
- [ ] Zero unsafe code
- [ ] Unit tests for all pulse channel features

---

## Tasks

### 3.2.1 Pulse Channel Structure

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Create the PulseChannel struct integrating all components (envelope, sweep, length counter, timer).

**Files:**

- `crates/rustynes-apu/src/pulse.rs` - Pulse channel implementation

**Subtasks:**

- [ ] Define PulseChannel struct
- [ ] Integrate Envelope component
- [ ] Integrate LengthCounter component
- [ ] Integrate Sweep component
- [ ] Add duty cycle sequencer
- [ ] Add timer (11-bit period)
- [ ] Add channel number (0 or 1) for sweep

**Implementation:**

```rust
use crate::envelope::Envelope;
use crate::length_counter::LengthCounter;
use crate::sweep::Sweep;

pub struct PulseChannel {
    // Components
    envelope: Envelope,
    length_counter: LengthCounter,
    sweep: Sweep,

    // Duty cycle
    duty: u8,              // 0-3 (12.5%, 25%, 50%, 75%)
    duty_position: u8,     // 0-7 position in duty cycle

    // Timer
    timer: u16,            // 11-bit period
    timer_counter: u16,    // Current timer value

    // Output
    enabled: bool,
    channel: u8,           // 0 = Pulse 1, 1 = Pulse 2
}

impl PulseChannel {
    pub fn new(channel: u8) -> Self {
        Self {
            envelope: Envelope::new(),
            length_counter: LengthCounter::new(),
            sweep: Sweep::new(channel),
            duty: 0,
            duty_position: 0,
            timer: 0,
            timer_counter: 0,
            enabled: false,
            channel,
        }
    }
}
```

---

### 3.2.2 Duty Cycle Implementation

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement the duty cycle sequencer that produces the square wave pattern.

**Files:**

- `crates/rustynes-apu/src/pulse.rs` - Duty cycle logic

**Subtasks:**

- [ ] Define duty cycle waveforms (4 patterns)
- [ ] 8-step sequencer
- [ ] Duty cycle position tracking
- [ ] Clock advancement on timer overflow

**Implementation:**

```rust
// Duty cycle waveforms (0 = low, 1 = high)
const DUTY_SEQUENCES: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0], // 12.5% duty cycle
    [0, 1, 1, 0, 0, 0, 0, 0], // 25% duty cycle
    [0, 1, 1, 1, 1, 0, 0, 0], // 50% duty cycle
    [1, 0, 0, 1, 1, 1, 1, 1], // 75% duty cycle (inverted 25%)
];

impl PulseChannel {
    fn duty_output(&self) -> u8 {
        DUTY_SEQUENCES[self.duty as usize][self.duty_position as usize]
    }

    fn clock_duty(&mut self) {
        self.duty_position = (self.duty_position + 1) & 0x07;
    }
}
```

---

### 3.2.3 Register Interface

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement the 4-register interface for each pulse channel ($4000-$4003, $4004-$4007).

**Files:**

- `crates/rustynes-apu/src/pulse.rs` - Register handlers

**Subtasks:**

- [ ] Register 0: Duty, envelope, length counter halt, volume
- [ ] Register 1: Sweep enable, period, negate, shift
- [ ] Register 2: Timer low 8 bits
- [ ] Register 3: Length counter load, timer high 3 bits

**Implementation:**

```rust
impl PulseChannel {
    pub fn write_register(&mut self, addr: u8, value: u8) {
        match addr {
            0 => {
                // $4000/$4004: DDLC VVVV
                self.duty = (value >> 6) & 0x03;
                let halt = (value & 0x20) != 0;
                self.length_counter.set_halt(halt);
                self.envelope.write_register(value);
            }
            1 => {
                // $4001/$4005: EPPP NSSS
                self.sweep.write_register(value);
            }
            2 => {
                // $4002/$4006: TTTT TTTT
                self.timer = (self.timer & 0xFF00) | (value as u16);
            }
            3 => {
                // $4003/$4007: LLLL LTTT
                self.timer = (self.timer & 0x00FF) | (((value & 0x07) as u16) << 8);

                // Load length counter
                let length_index = (value >> 3) & 0x1F;
                if self.enabled {
                    self.length_counter.load(length_index);
                }

                // Restart envelope and reset duty position
                self.envelope.start();
                self.duty_position = 0;
            }
            _ => {}
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.length_counter.set_enabled(false);
        }
    }
}
```

---

### 3.2.4 Timer Logic

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement the 11-bit timer that controls waveform frequency.

**Files:**

- `crates/rustynes-apu/src/pulse.rs` - Timer implementation

**Subtasks:**

- [ ] Timer countdown (clocked every APU cycle)
- [ ] Timer reload on reaching 0
- [ ] Duty cycle advance on timer overflow
- [ ] Frequency calculation

**Implementation:**

```rust
impl PulseChannel {
    pub fn clock_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer;
            self.clock_duty();
        } else {
            self.timer_counter -= 1;
        }
    }

    fn frequency_hz(&self) -> f32 {
        // CPU frequency / (16 * (timer + 1))
        1_789_773.0 / (16.0 * (self.timer as f32 + 1.0))
    }
}
```

---

### 3.2.5 Output Logic

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement the output calculation considering all silencing conditions.

**Files:**

- `crates/rustynes-apu/src/pulse.rs` - Output method

**Subtasks:**

- [ ] Check if channel is enabled
- [ ] Check if length counter is active
- [ ] Check if sweep unit is muting
- [ ] Multiply duty output by envelope volume
- [ ] Return 0 if any condition silences channel

**Implementation:**

```rust
impl PulseChannel {
    pub fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }

        if !self.length_counter.is_active() {
            return 0;
        }

        if self.sweep.is_muted(self.timer) {
            return 0;
        }

        if self.duty_output() == 0 {
            return 0;
        }

        self.envelope.output()
    }

    pub fn is_silenced(&self) -> bool {
        !self.enabled
            || !self.length_counter.is_active()
            || self.sweep.is_muted(self.timer)
    }
}
```

---

### 3.2.6 Frame Clock Integration

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 1 hour

**Description:**
Integrate pulse channels with frame counter for envelope, length counter, and sweep updates.

**Files:**

- `crates/rustynes-apu/src/apu.rs` - Frame action handlers

**Subtasks:**

- [ ] Clock envelopes on quarter frame
- [ ] Clock length counters on half frame
- [ ] Clock sweep units on half frame

**Implementation:**

```rust
impl Apu {
    pub fn step(&mut self) {
        // Clock timers every APU cycle
        self.pulse1.clock_timer();
        self.pulse2.clock_timer();

        // Clock frame counter
        let action = self.frame_counter.clock();

        match action {
            FrameAction::QuarterFrame => {
                self.pulse1.clock_envelope();
                self.pulse2.clock_envelope();
            }
            FrameAction::HalfFrame => {
                self.pulse1.clock_envelope();
                self.pulse2.clock_envelope();
                self.pulse1.clock_length_counter();
                self.pulse2.clock_length_counter();
                self.pulse1.clock_sweep();
                self.pulse2.clock_sweep();
            }
            FrameAction::None => {}
        }
    }
}

impl PulseChannel {
    pub fn clock_envelope(&mut self) {
        self.envelope.clock();
    }

    pub fn clock_length_counter(&mut self) {
        self.length_counter.clock();
    }

    pub fn clock_sweep(&mut self) {
        self.sweep.clock(&mut self.timer);
    }
}
```

---

### 3.2.7 Unit Tests

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 3 hours

**Description:**
Create comprehensive unit tests for pulse channel functionality.

**Files:**

- `crates/rustynes-apu/src/pulse.rs` - Test module

**Subtasks:**

- [ ] Test duty cycle waveforms
- [ ] Test register writes
- [ ] Test timer countdown
- [ ] Test sweep modulation
- [ ] Test silencing conditions
- [ ] Test envelope integration
- [ ] Test length counter integration

**Tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duty_cycles() {
        let mut pulse = PulseChannel::new(0);

        // Test 12.5% duty cycle
        pulse.duty = 0;
        for i in 0..8 {
            pulse.duty_position = i;
            let expected = DUTY_SEQUENCES[0][i as usize];
            assert_eq!(pulse.duty_output(), expected);
        }
    }

    #[test]
    fn test_register_writes() {
        let mut pulse = PulseChannel::new(0);
        pulse.set_enabled(true);

        // Write duty and envelope
        pulse.write_register(0, 0xBF); // 50% duty, constant volume 15
        assert_eq!(pulse.duty, 2);
        assert_eq!(pulse.envelope.output(), 15);

        // Write timer low
        pulse.write_register(2, 0x54);
        assert_eq!(pulse.timer & 0xFF, 0x54);

        // Write length counter and timer high
        pulse.write_register(3, 0xF8);
        assert_eq!(pulse.timer, 0x054);
    }

    #[test]
    fn test_timer_countdown() {
        let mut pulse = PulseChannel::new(0);
        pulse.timer = 10;
        pulse.timer_counter = 10;

        let initial_pos = pulse.duty_position;

        // Clock timer to 0
        for _ in 0..11 {
            pulse.clock_timer();
        }

        // Duty position should advance
        assert_eq!(pulse.duty_position, (initial_pos + 1) & 0x07);

        // Timer should reload
        assert_eq!(pulse.timer_counter, 10);
    }

    #[test]
    fn test_sweep_muting() {
        let mut pulse = PulseChannel::new(0);
        pulse.set_enabled(true);
        pulse.length_counter.load(0); // Load non-zero
        pulse.duty_position = 1; // Non-zero duty output

        // Timer < 8 should silence
        pulse.timer = 7;
        assert_eq!(pulse.output(), 0);

        // Valid timer should produce output
        pulse.timer = 100;
        assert!(pulse.output() > 0);
    }

    #[test]
    fn test_length_counter_silencing() {
        let mut pulse = PulseChannel::new(0);
        pulse.set_enabled(true);
        pulse.timer = 100;
        pulse.duty_position = 1;
        pulse.envelope.write_register(0x1F); // Constant volume 15

        // With active length counter
        pulse.length_counter.load(0);
        assert_eq!(pulse.output(), 15);

        // Clock length counter to 0
        for _ in 0..10 {
            pulse.clock_length_counter();
        }
        assert_eq!(pulse.output(), 0);
    }

    #[test]
    fn test_sweep_target_period() {
        let mut pulse1 = PulseChannel::new(0);
        let mut pulse2 = PulseChannel::new(1);

        pulse1.timer = 200;
        pulse2.timer = 200;

        // Configure sweep: enabled, period=0, negate, shift=1
        pulse1.sweep.write_register(0x89);
        pulse2.sweep.write_register(0x89);

        // Clock sweep
        pulse1.clock_sweep();
        pulse2.clock_sweep();

        // Pulse 1 uses one's complement: 200 - 100 - 1 = 99
        // Pulse 2 uses two's complement: 200 - 100 = 100
        assert_eq!(pulse1.timer, 99);
        assert_eq!(pulse2.timer, 100);
    }
}
```

---

## Dependencies

**Required:**

- Sprint 3.1 complete (envelope, length counter, sweep, frame counter)

**Blocks:**

- Sprint 3.5: Audio output and mixing (needs pulse channel output)

---

## Related Documentation

- [APU Pulse Channel](../../../docs/apu/APU_CHANNEL_PULSE.md)
- [APU 2A03 Specification](../../../docs/apu/APU_2A03_SPECIFICATION.md)
- [NESdev Wiki - APU Pulse](https://www.nesdev.org/wiki/APU_Pulse)
- [NESdev Wiki - APU Sweep](https://www.nesdev.org/wiki/APU_Sweep)

---

## Technical Notes

### Sweep Unit Differences

Pulse 1 and Pulse 2 differ in their sweep negate behavior:
- **Pulse 1**: Uses one's complement (subtract change + 1)
- **Pulse 2**: Uses two's complement (subtract change only)

This subtle difference is audible in games and must be implemented correctly.

### Timer Frequency

The pulse timer is clocked every other CPU cycle (APU runs at CPU/2). The output frequency is:

```
frequency = CPU_CLOCK / (16 * (timer + 1))
```

For NTSC: `frequency = 1789773 / (16 * (timer + 1))`

### Duty Cycle Usage

- **12.5%**: Short, sharp sounds (sound effects)
- **25%**: Thin, hollow sounds (lead melody)
- **50%**: Square, full sounds (harmony)
- **75%**: Same as 25% but phase-inverted (rarely used)

---

## Test Requirements

- [ ] Unit tests for all four duty cycles
- [ ] Unit tests for register writes
- [ ] Unit tests for timer countdown
- [ ] Unit tests for sweep modulation (both channels)
- [ ] Unit tests for envelope integration
- [ ] Unit tests for length counter silencing
- [ ] Integration test: Full pulse channel with all components

---

## Performance Targets

- Timer clock: <10 ns per cycle
- Output calculation: <20 ns
- Memory: <100 bytes per channel

---

## Success Criteria

- [ ] Both pulse channels produce correct waveforms
- [ ] All four duty cycles work correctly
- [ ] Sweep units modulate frequency accurately
- [ ] Length counters silence channels appropriately
- [ ] Envelopes control volume correctly
- [ ] One's vs two's complement sweep works for both channels
- [ ] All unit tests pass
- [ ] Zero unsafe code
- [ ] Documentation complete

---

**Previous Sprint:** [Sprint 3.1: APU Core](M3-S1-APU-CORE.md)
**Next Sprint:** [Sprint 3.3: Triangle & Noise Channels](M3-S3-TRIANGLE-NOISE.md)

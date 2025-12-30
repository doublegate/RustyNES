//! APU DMC (Delta Modulation Channel).
//!
//! The DMC channel plays back 1-bit delta-encoded samples from memory.
//! It's the only channel that requires memory access to function.
//!
//! The DMC can trigger an IRQ when a sample completes, and can optionally
//! loop the sample.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// DMC rate lookup table (NTSC).
/// Index is the 4-bit rate value from the register.
const DMC_RATE_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];

/// DMC channel.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[allow(clippy::struct_excessive_bools)] // Hardware state requires multiple flags
pub struct Dmc {
    /// Timer counter.
    timer: u16,
    /// Timer period (from lookup table).
    period: u16,
    /// Output level (0-127).
    output_level: u8,
    /// Sample buffer (empty when None).
    sample_buffer: Option<u8>,
    /// Shift register (8 bits).
    shift_register: u8,
    /// Bits remaining in shift register.
    bits_remaining: u8,
    /// Sample address (start address for next sample).
    sample_address: u16,
    /// Current address being read.
    current_address: u16,
    /// Sample length (in bytes).
    sample_length: u16,
    /// Bytes remaining to read.
    bytes_remaining: u16,
    /// Loop flag.
    loop_flag: bool,
    /// IRQ enabled flag.
    irq_enabled: bool,
    /// IRQ pending flag.
    irq_pending: bool,
    /// Silence flag (output unit silenced).
    silence: bool,
}

impl Dmc {
    /// Create a new DMC channel.
    #[must_use]
    pub fn new() -> Self {
        Self {
            timer: DMC_RATE_TABLE[0],
            period: DMC_RATE_TABLE[0],
            output_level: 0,
            sample_buffer: None,
            shift_register: 0,
            bits_remaining: 0,
            sample_address: 0xC000,
            current_address: 0xC000,
            sample_length: 0,
            bytes_remaining: 0,
            loop_flag: false,
            irq_enabled: false,
            irq_pending: false,
            silence: true,
        }
    }

    /// Write to register $4010 (flags, rate).
    pub fn write_ctrl(&mut self, value: u8) {
        self.irq_enabled = value & 0x80 != 0;
        self.loop_flag = value & 0x40 != 0;
        self.period = DMC_RATE_TABLE[(value & 0x0F) as usize];

        // Clear IRQ if disabled
        if !self.irq_enabled {
            self.irq_pending = false;
        }
    }

    /// Write to register $4011 (direct load).
    pub fn write_direct_load(&mut self, value: u8) {
        self.output_level = value & 0x7F;
    }

    /// Write to register $4012 (sample address).
    pub fn write_sample_address(&mut self, value: u8) {
        // Address = $C000 + (A * 64)
        self.sample_address = 0xC000 + (u16::from(value) << 6);
    }

    /// Write to register $4013 (sample length).
    pub fn write_sample_length(&mut self, value: u8) {
        // Length = (L * 16) + 1
        self.sample_length = (u16::from(value) << 4) + 1;
    }

    /// Set the enabled state.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.irq_pending = false;

        if enabled {
            if self.bytes_remaining == 0 {
                self.restart();
            }
        } else {
            self.bytes_remaining = 0;
        }
    }

    /// Restart sample playback.
    fn restart(&mut self) {
        self.current_address = self.sample_address;
        self.bytes_remaining = self.sample_length;
    }

    /// Check if the channel is active (bytes remaining > 0).
    #[must_use]
    pub fn active(&self) -> bool {
        self.bytes_remaining > 0
    }

    /// Check if an IRQ is pending.
    #[must_use]
    pub fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    /// Clear the IRQ pending flag.
    pub fn clear_irq(&mut self) {
        self.irq_pending = false;
    }

    /// Check if the sample buffer needs to be filled.
    #[must_use]
    pub fn needs_sample(&self) -> bool {
        self.sample_buffer.is_none() && self.bytes_remaining > 0
    }

    /// Get the next sample address to read.
    #[must_use]
    pub fn sample_addr(&self) -> u16 {
        self.current_address
    }

    /// Fill the sample buffer with a byte from memory.
    pub fn fill_sample_buffer(&mut self, sample: u8) {
        self.sample_buffer = Some(sample);

        // Advance address (wraps at $FFFF to $8000)
        self.current_address = if self.current_address == 0xFFFF {
            0x8000
        } else {
            self.current_address + 1
        };

        self.bytes_remaining -= 1;

        // Check for end of sample
        if self.bytes_remaining == 0 {
            if self.loop_flag {
                self.restart();
            } else if self.irq_enabled {
                self.irq_pending = true;
            }
        }
    }

    /// Clock the timer. Should be called every APU cycle (CPU/2).
    pub fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.period;
            self.clock_output_unit();
        } else {
            self.timer -= 1;
        }
    }

    /// Clock the output unit.
    fn clock_output_unit(&mut self) {
        if self.bits_remaining == 0 {
            // Load new sample if available
            if let Some(sample) = self.sample_buffer.take() {
                self.shift_register = sample;
                self.silence = false;
            } else {
                self.silence = true;
            }
            self.bits_remaining = 8;
        }

        if !self.silence {
            // Update output level based on bit 0
            if self.shift_register & 1 != 0 {
                if self.output_level <= 125 {
                    self.output_level += 2;
                }
            } else if self.output_level >= 2 {
                self.output_level -= 2;
            }
        }

        self.shift_register >>= 1;
        self.bits_remaining -= 1;
    }

    /// Get the current output value (0-127).
    #[must_use]
    pub fn output(&self) -> u8 {
        self.output_level
    }

    /// Get the bytes remaining count.
    #[must_use]
    pub fn bytes_remaining(&self) -> u16 {
        self.bytes_remaining
    }
}

impl Default for Dmc {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dmc_rate_table() {
        // Verify first and last entries
        assert_eq!(DMC_RATE_TABLE[0], 428);
        assert_eq!(DMC_RATE_TABLE[15], 54);
    }

    #[test]
    fn test_dmc_direct_load() {
        let mut dmc = Dmc::new();
        dmc.write_direct_load(0x40);
        assert_eq!(dmc.output(), 0x40);

        // Only 7 bits used
        dmc.write_direct_load(0xFF);
        assert_eq!(dmc.output(), 0x7F);
    }

    #[test]
    fn test_dmc_sample_address() {
        let mut dmc = Dmc::new();
        dmc.write_sample_address(0x00);
        assert_eq!(dmc.sample_address, 0xC000);

        dmc.write_sample_address(0xFF);
        assert_eq!(dmc.sample_address, 0xC000 + (0xFF << 6));
    }

    #[test]
    fn test_dmc_sample_length() {
        let mut dmc = Dmc::new();
        dmc.write_sample_length(0x00);
        assert_eq!(dmc.sample_length, 1);

        dmc.write_sample_length(0xFF);
        assert_eq!(dmc.sample_length, (0xFF << 4) + 1);
    }

    #[test]
    fn test_dmc_enable_restart() {
        let mut dmc = Dmc::new();
        dmc.write_sample_address(0x10);
        dmc.write_sample_length(0x10);

        dmc.set_enabled(true);
        assert!(dmc.active());
        assert_eq!(dmc.current_address, dmc.sample_address);
    }

    #[test]
    fn test_dmc_disable() {
        let mut dmc = Dmc::new();
        dmc.write_sample_length(0x10);
        dmc.set_enabled(true);
        assert!(dmc.active());

        dmc.set_enabled(false);
        assert!(!dmc.active());
    }

    #[test]
    fn test_dmc_irq() {
        let mut dmc = Dmc::new();
        dmc.write_ctrl(0x80); // IRQ enabled
        dmc.write_sample_length(0x00); // Length = 1
        dmc.set_enabled(true);

        // Fill sample buffer and consume it
        dmc.fill_sample_buffer(0x00);

        // Should trigger IRQ when sample ends
        assert!(dmc.irq_pending());
    }

    #[test]
    fn test_dmc_loop() {
        let mut dmc = Dmc::new();
        dmc.write_ctrl(0x40); // Loop enabled
        dmc.write_sample_length(0x00); // Length = 1
        dmc.set_enabled(true);

        let initial_bytes = dmc.bytes_remaining;
        dmc.fill_sample_buffer(0x00);

        // Should restart when sample ends with loop
        assert_eq!(dmc.bytes_remaining, initial_bytes);
        assert!(!dmc.irq_pending());
    }
}

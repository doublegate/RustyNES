// DMC (Delta Modulation Channel) - 1-bit delta-encoded sample playback
//
// The DMC channel plays 1-bit delta-encoded samples from CPU memory via DMA.
// Unlike other channels, it reads sample data directly from memory, which
// stalls the CPU for 1-4 cycles per byte fetched.

/// DMC rate table for NTSC (CPU cycles per timer tick)
const DMC_RATE_NTSC: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];

/// DMC rate table for PAL (CPU cycles per timer tick)
const DMC_RATE_PAL: [u16; 16] = [
    398, 354, 316, 298, 276, 236, 210, 198, 176, 148, 132, 118, 98, 78, 66, 50,
];

/// System type for rate table selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum System {
    /// NTSC (North America, Japan)
    NTSC,
    /// PAL (Europe, Australia)
    PAL,
}

/// DMC channel implementation
///
/// The DMC (Delta Modulation Channel) plays 1-bit delta-encoded samples from CPU memory.
/// It uses DMA to read sample bytes, which stalls the CPU for 1-4 cycles per byte.
///
/// # Registers
///
/// - `$4010`: IRQ enable, Loop flag, Rate index
/// - `$4011`: Direct load (7-bit output level)
/// - `$4012`: Sample address = $C000 + (A × $40)
/// - `$4013`: Sample length = (L × $10) + 1 bytes
///
/// # Delta Modulation
///
/// The DMC stores only changes (+2 or -2 to output level) instead of absolute
/// sample values. Each bit in a sample byte represents increment (1) or decrement (0).
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct DmcChannel {
    // Configuration
    irq_enabled: bool,
    loop_enabled: bool,
    rate_index: u8,

    // Sample playback state
    sample_address: u8,   // Register value ($4012)
    sample_length: u8,    // Register value ($4013)
    current_address: u16, // Current read address (internal)
    bytes_remaining: u16, // Bytes left to read (internal)

    // Output units
    sample_buffer: u8,         // 8-bit sample buffer
    sample_buffer_empty: bool, // Buffer empty flag
    bits_remaining: u8,        // Bits remaining in buffer (0-8)
    output_level: u8,          // 7-bit output level (0-127)

    // Timer
    timer: u16,
    timer_counter: u16,
    rate_table: [u16; 16],

    // IRQ flag
    irq_flag: bool,

    // Enable flag (from $4015)
    enabled: bool,
}

impl DmcChannel {
    /// Create a new DMC channel
    ///
    /// # Arguments
    ///
    /// * `system` - System type (NTSC or PAL) for rate table selection
    #[must_use]
    pub fn new(system: System) -> Self {
        let rate_table = match system {
            System::NTSC => DMC_RATE_NTSC,
            System::PAL => DMC_RATE_PAL,
        };

        let initial_timer = rate_table[0];

        Self {
            irq_enabled: false,
            loop_enabled: false,
            rate_index: 0,
            sample_address: 0,
            sample_length: 0,
            current_address: 0xC000,
            bytes_remaining: 0,
            sample_buffer: 0,
            sample_buffer_empty: true,
            bits_remaining: 0,
            output_level: 0,
            timer: initial_timer,
            timer_counter: initial_timer,
            rate_table,
            irq_flag: false,
            enabled: false,
        }
    }

    /// Write to DMC register
    ///
    /// # Arguments
    ///
    /// * `addr` - Register offset (0-3 for $4010-$4013)
    /// * `value` - Value to write
    pub fn write_register(&mut self, addr: u8, value: u8) {
        match addr {
            0 => {
                // $4010: IL-- RRRR
                // I = IRQ enable
                // L = Loop enable
                // R = Rate index
                self.irq_enabled = (value & 0x80) != 0;
                self.loop_enabled = (value & 0x40) != 0;
                self.rate_index = value & 0x0F;

                // Update timer period
                self.timer = self.rate_table[self.rate_index as usize];

                // Clearing IRQ enable clears the IRQ flag
                if !self.irq_enabled {
                    self.irq_flag = false;
                }
            }
            1 => {
                // $4011: -DDD DDDD
                // D = Direct load value (output level)
                self.output_level = value & 0x7F;
            }
            2 => {
                // $4012: AAAA AAAA
                // A = Sample address ($C000 + A × $40)
                self.sample_address = value;
            }
            3 => {
                // $4013: LLLL LLLL
                // L = Sample length ((L × $10) + 1 bytes)
                self.sample_length = value;
            }
            _ => {}
        }
    }

    /// Set channel enable state (called from $4015 write)
    ///
    /// When enabled, starts sample playback if bytes remaining is 0.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;

        if enabled {
            // If bytes remaining is 0, restart sample
            if self.bytes_remaining == 0 {
                self.restart_sample();
            }
        } else {
            // Disable clears bytes remaining
            self.bytes_remaining = 0;
        }
    }

    /// Clock the timer (called every CPU cycle)
    ///
    /// Returns the number of CPU cycles stolen by DMA (0 if no DMA occurred).
    /// Typical DMA stall is 3 cycles, but can be 1-4 depending on CPU state.
    pub fn clock_timer<F>(&mut self, mut read_memory: F) -> u8
    where
        F: FnMut(u16) -> u8,
    {
        let mut dma_cycles = 0;

        if self.timer_counter == 0 {
            self.timer_counter = self.timer;

            // Clock output shifter if bits remain
            if self.bits_remaining > 0 {
                self.clock_output_shifter();
            }

            // Refill buffer if empty and bytes remaining
            if self.sample_buffer_empty && self.bytes_remaining > 0 {
                dma_cycles = self.fetch_sample(&mut read_memory);
            }
        } else {
            self.timer_counter -= 1;
        }

        dma_cycles
    }

    /// Clock the output shifter
    ///
    /// Processes one bit from the sample buffer:
    /// - If bit is 1: increment output level by 2 (clamped to 127)
    /// - If bit is 0: decrement output level by 2 (clamped to 0)
    /// - If buffer empty: use silence bit (0) and decrement
    fn clock_output_shifter(&mut self) {
        if self.sample_buffer_empty {
            // Silence bit (always 0) → decrement
            if self.output_level >= 2 {
                self.output_level -= 2;
            }
        } else {
            // Process LSB of sample buffer
            if (self.sample_buffer & 1) == 1 {
                // Bit is 1 → increment by 2
                if self.output_level <= 125 {
                    self.output_level += 2;
                }
            } else {
                // Bit is 0 → decrement by 2
                if self.output_level >= 2 {
                    self.output_level -= 2;
                }
            }

            // Shift buffer right
            self.sample_buffer >>= 1;
        }

        // Decrement bits remaining
        self.bits_remaining -= 1;

        // Mark buffer empty if all bits processed
        if self.bits_remaining == 0 {
            self.sample_buffer_empty = true;
        }
    }

    /// Fetch sample byte via DMA
    ///
    /// Reads a byte from memory, stalling the CPU for 1-4 cycles.
    /// Updates address, bytes remaining, and handles sample completion.
    ///
    /// Returns the number of CPU cycles stolen (typically 3).
    fn fetch_sample<F>(&mut self, read_memory: &mut F) -> u8
    where
        F: FnMut(u16) -> u8,
    {
        // Read byte from memory (DMA)
        self.sample_buffer = read_memory(self.current_address);
        self.sample_buffer_empty = false;
        self.bits_remaining = 8;

        // Increment address with wrap ($FFFF → $8000, not $0000!)
        if self.current_address == 0xFFFF {
            self.current_address = 0x8000;
        } else {
            self.current_address += 1;
        }

        // Decrement bytes remaining
        self.bytes_remaining -= 1;

        // Handle sample completion
        if self.bytes_remaining == 0 {
            if self.loop_enabled {
                // Restart sample
                self.restart_sample();
            } else if self.irq_enabled {
                // Set IRQ flag
                self.irq_flag = true;
            }
        }

        // Return DMA stall cycles (3 typical, can be 1-4)
        3
    }

    /// Restart sample playback
    ///
    /// Reloads address and length from register values.
    fn restart_sample(&mut self) {
        // Sample address = $C000 + (sample_address × $40)
        self.current_address = 0xC000 | (u16::from(self.sample_address) << 6);

        // Sample length = (sample_length × $10) + 1
        self.bytes_remaining = (u16::from(self.sample_length) << 4) | 1;
    }

    /// Get current output value (0-127)
    ///
    /// Returns 0 if channel is disabled.
    #[must_use]
    pub fn output(&self) -> u8 {
        if self.enabled {
            self.output_level
        } else {
            0
        }
    }

    /// Check if DMC IRQ flag is set
    #[must_use]
    pub fn irq_pending(&self) -> bool {
        self.irq_flag
    }

    /// Clear DMC IRQ flag (called when $4015 is read)
    pub fn clear_irq(&mut self) {
        self.irq_flag = false;
    }

    /// Check if bytes remaining > 0 (for $4015 status read)
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.bytes_remaining > 0
    }
}

impl Default for DmcChannel {
    fn default() -> Self {
        Self::new(System::NTSC)
    }
}

#[cfg(test)]
#[allow(clippy::large_stack_arrays)]
mod tests {
    use super::*;

    #[test]
    fn test_dmc_new() {
        let dmc = DmcChannel::new(System::NTSC);
        assert_eq!(dmc.output_level, 0);
        assert!(!dmc.enabled);
        assert!(!dmc.irq_enabled);
        assert!(!dmc.loop_enabled);
        assert_eq!(dmc.rate_index, 0);
        assert_eq!(dmc.bytes_remaining, 0);
        assert!(dmc.sample_buffer_empty);
    }

    #[test]
    fn test_rate_tables() {
        let ntsc = DmcChannel::new(System::NTSC);
        let pal = DmcChannel::new(System::PAL);

        // Verify different rate tables
        assert_eq!(ntsc.rate_table[0], 428);
        assert_eq!(pal.rate_table[0], 398);
    }

    #[test]
    fn test_direct_load() {
        let mut dmc = DmcChannel::new(System::NTSC);
        dmc.set_enabled(true);

        // Write to $4011 (direct load)
        dmc.write_register(1, 0x7F); // Max value (127)
        assert_eq!(dmc.output_level, 127);
        assert_eq!(dmc.output(), 127);

        // Only 7 bits are used
        dmc.write_register(1, 0xFF);
        assert_eq!(dmc.output_level, 127);
    }

    #[test]
    fn test_rate_index() {
        let mut dmc = DmcChannel::new(System::NTSC);

        // Set rate index via $4010
        dmc.write_register(0, 0x0F); // Rate 15 (fastest)
        assert_eq!(dmc.rate_index, 15);
        assert_eq!(dmc.timer, DMC_RATE_NTSC[15]);

        dmc.write_register(0, 0x00); // Rate 0 (slowest)
        assert_eq!(dmc.rate_index, 0);
        assert_eq!(dmc.timer, DMC_RATE_NTSC[0]);
    }

    #[test]
    fn test_irq_enable_clears_flag() {
        let mut dmc = DmcChannel::new(System::NTSC);

        // Set IRQ flag manually (simulating sample completion)
        dmc.irq_flag = true;
        assert!(dmc.irq_pending());

        // Disable IRQ via $4010 with I=0
        dmc.write_register(0, 0x00); // IRQ disabled
        assert!(!dmc.irq_pending());
    }

    #[test]
    fn test_loop_flag() {
        let mut dmc = DmcChannel::new(System::NTSC);

        // Enable loop via $4010
        dmc.write_register(0, 0x40); // Loop enabled
        assert!(dmc.loop_enabled);
        assert!(!dmc.irq_enabled);

        // Disable loop
        dmc.write_register(0, 0x00);
        assert!(!dmc.loop_enabled);
    }

    #[test]
    fn test_sample_address_calculation() {
        let mut dmc = DmcChannel::new(System::NTSC);

        dmc.write_register(2, 0x00); // Address = $C000
        dmc.restart_sample();
        assert_eq!(dmc.current_address, 0xC000);

        dmc.write_register(2, 0x01); // Address = $C040
        dmc.restart_sample();
        assert_eq!(dmc.current_address, 0xC040);

        dmc.write_register(2, 0xFF); // Address = $FFC0
        dmc.restart_sample();
        assert_eq!(dmc.current_address, 0xFFC0);
    }

    #[test]
    fn test_sample_length_calculation() {
        let mut dmc = DmcChannel::new(System::NTSC);

        dmc.write_register(3, 0x00); // Length = 1
        dmc.restart_sample();
        assert_eq!(dmc.bytes_remaining, 1);

        dmc.write_register(3, 0x01); // Length = 17
        dmc.restart_sample();
        assert_eq!(dmc.bytes_remaining, 17);

        dmc.write_register(3, 0xFF); // Length = 4081
        dmc.restart_sample();
        assert_eq!(dmc.bytes_remaining, 4081);
    }

    #[test]
    fn test_output_shifter_increment() {
        let mut dmc = DmcChannel::new(System::NTSC);
        dmc.output_level = 64;
        dmc.sample_buffer = 0xFF; // All 1s
        dmc.sample_buffer_empty = false;
        dmc.bits_remaining = 8;

        // Process bit (LSB is 1)
        dmc.clock_output_shifter();
        assert_eq!(dmc.output_level, 66); // Incremented by 2
        assert_eq!(dmc.bits_remaining, 7);
    }

    #[test]
    fn test_output_shifter_decrement() {
        let mut dmc = DmcChannel::new(System::NTSC);
        dmc.output_level = 64;
        dmc.sample_buffer = 0x00; // All 0s
        dmc.sample_buffer_empty = false;
        dmc.bits_remaining = 8;

        // Process bit (LSB is 0)
        dmc.clock_output_shifter();
        assert_eq!(dmc.output_level, 62); // Decremented by 2
        assert_eq!(dmc.bits_remaining, 7);
    }

    #[test]
    fn test_output_clamping_high() {
        let mut dmc = DmcChannel::new(System::NTSC);
        dmc.output_level = 127; // Max
        dmc.sample_buffer = 0xFF;
        dmc.sample_buffer_empty = false;
        dmc.bits_remaining = 8;

        // Try to increment (should clamp)
        dmc.clock_output_shifter();
        assert_eq!(dmc.output_level, 127); // Clamped at max
    }

    #[test]
    fn test_output_clamping_low() {
        let mut dmc = DmcChannel::new(System::NTSC);
        dmc.output_level = 0; // Min
        dmc.sample_buffer = 0x00;
        dmc.sample_buffer_empty = false;
        dmc.bits_remaining = 8;

        // Try to decrement (should clamp)
        dmc.clock_output_shifter();
        assert_eq!(dmc.output_level, 0); // Clamped at min
    }

    #[test]
    fn test_silence_bit() {
        let mut dmc = DmcChannel::new(System::NTSC);
        dmc.output_level = 64;
        dmc.sample_buffer_empty = true;
        dmc.bits_remaining = 1;

        // Silence bit always decrements
        dmc.clock_output_shifter();
        assert_eq!(dmc.output_level, 62);
    }

    #[test]
    fn test_address_wrap() {
        let mut dmc = DmcChannel::new(System::NTSC);
        dmc.current_address = 0xFFFF;
        dmc.bytes_remaining = 2;
        dmc.enabled = true;

        let mut memory = [0u8; 0x10000];
        memory[0xFFFF] = 0xAA;

        // Fetch sample (should wrap to $8000)
        dmc.fetch_sample(&mut |addr| memory[addr as usize]);
        assert_eq!(dmc.current_address, 0x8000); // Wrapped
    }

    #[test]
    fn test_sample_completion_with_irq() {
        let mut dmc = DmcChannel::new(System::NTSC);
        dmc.write_register(0, 0x80); // IRQ enabled, no loop
        dmc.bytes_remaining = 1;
        dmc.enabled = true;

        let memory = [0u8; 0x10000];

        // Fetch last byte (should set IRQ)
        dmc.fetch_sample(&mut |addr| memory[addr as usize]);
        assert_eq!(dmc.bytes_remaining, 0);
        assert!(dmc.irq_pending());
    }

    #[test]
    fn test_sample_completion_with_loop() {
        let mut dmc = DmcChannel::new(System::NTSC);
        dmc.write_register(0, 0x40); // Loop enabled, no IRQ
        dmc.write_register(2, 0x01); // Address = $C040
        dmc.write_register(3, 0x01); // Length = 17 bytes
        dmc.bytes_remaining = 1;
        dmc.current_address = 0xD000;
        dmc.enabled = true;

        let memory = [0u8; 0x10000];

        // Fetch last byte (should restart)
        dmc.fetch_sample(&mut |addr| memory[addr as usize]);
        assert_eq!(dmc.bytes_remaining, 17); // Reloaded
        assert_eq!(dmc.current_address, 0xC040); // Reset
        assert!(!dmc.irq_pending()); // No IRQ
    }

    #[test]
    fn test_enable_starts_sample() {
        let mut dmc = DmcChannel::new(System::NTSC);
        dmc.write_register(2, 0x00); // Address
        dmc.write_register(3, 0x10); // Length = 257
        assert_eq!(dmc.bytes_remaining, 0);

        // Enable should start sample
        dmc.set_enabled(true);
        assert_eq!(dmc.bytes_remaining, 257);
        assert_eq!(dmc.current_address, 0xC000);
    }

    #[test]
    fn test_disable_clears_bytes_remaining() {
        let mut dmc = DmcChannel::new(System::NTSC);
        dmc.set_enabled(true);
        dmc.bytes_remaining = 100;

        // Disable should clear
        dmc.set_enabled(false);
        assert_eq!(dmc.bytes_remaining, 0);
    }

    #[test]
    fn test_timer_clocking() {
        let mut dmc = DmcChannel::new(System::NTSC);
        dmc.write_register(0, 0x0F); // Rate 15 (fastest, 54 cycles)
        dmc.timer_counter = 2;
        dmc.bits_remaining = 0;

        let memory = [0u8; 0x10000];

        // Clock twice (no DMA yet)
        assert_eq!(dmc.clock_timer(|addr| memory[addr as usize]), 0);
        assert_eq!(dmc.timer_counter, 1);

        assert_eq!(dmc.clock_timer(|addr| memory[addr as usize]), 0);
        assert_eq!(dmc.timer_counter, 0);

        // Next clock reloads timer
        assert_eq!(dmc.clock_timer(|addr| memory[addr as usize]), 0);
        assert_eq!(dmc.timer_counter, 54);
    }

    #[test]
    fn test_output_disabled() {
        let mut dmc = DmcChannel::new(System::NTSC);
        dmc.output_level = 64;

        // Disabled channel outputs 0
        assert_eq!(dmc.output(), 0);

        dmc.set_enabled(true);
        assert_eq!(dmc.output(), 64);
    }

    #[test]
    fn test_clear_irq() {
        let mut dmc = DmcChannel::new(System::NTSC);
        dmc.irq_flag = true;

        dmc.clear_irq();
        assert!(!dmc.irq_pending());
    }

    #[test]
    fn test_is_active() {
        let mut dmc = DmcChannel::new(System::NTSC);

        assert!(!dmc.is_active()); // No bytes remaining

        dmc.bytes_remaining = 10;
        assert!(dmc.is_active());

        dmc.bytes_remaining = 0;
        assert!(!dmc.is_active());
    }
}

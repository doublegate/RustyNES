//! APU Main Module
//!
//! This module contains the main APU struct that integrates all audio channels
//! and provides the register interface.

use crate::dmc::{DmcChannel, System};
use crate::frame_counter::{FrameAction, FrameCounter};
use crate::mixer::Mixer;
use crate::noise::NoiseChannel;
use crate::pulse::PulseChannel;
use crate::resampler::Resampler;
use crate::triangle::TriangleChannel;

/// Main APU (Audio Processing Unit) structure
///
/// Integrates all 5 audio channels (2 pulse, triangle, noise, DMC) and the frame counter.
/// Provides memory-mapped register interface at CPU addresses $4000-$4017.
#[allow(clippy::struct_excessive_bools)]
pub struct Apu {
    /// Frame counter for timing envelope/length/sweep updates
    frame_counter: FrameCounter,

    /// Pulse wave channels
    pulse1: PulseChannel,
    pulse2: PulseChannel,

    /// Triangle wave channel
    triangle: TriangleChannel,

    /// Noise channel
    noise: NoiseChannel,

    /// DMC channel
    dmc: DmcChannel,

    /// Non-linear mixer for combining channel outputs
    mixer: Mixer,

    /// Resampler for converting APU rate to target sample rate
    resampler: Resampler,

    /// CPU cycle counter
    cycles: u64,

    /// Memory read callback for DMC DMA
    /// This will be set by the emulator to allow DMC to read from CPU memory
    memory_read_fn: Option<Box<dyn FnMut(u16) -> u8>>,
}

impl Apu {
    /// Creates a new APU instance with default settings (48 kHz output, NTSC)
    #[must_use]
    pub fn new() -> Self {
        Self {
            frame_counter: FrameCounter::new(),
            pulse1: PulseChannel::new(0),
            pulse2: PulseChannel::new(1),
            triangle: TriangleChannel::new(),
            noise: NoiseChannel::new(),
            dmc: DmcChannel::new(System::NTSC),
            mixer: Mixer::new(),
            resampler: Resampler::new(48000),
            cycles: 0,
            memory_read_fn: None,
        }
    }

    /// Creates a new APU instance with custom sample rate
    ///
    /// # Arguments
    ///
    /// * `sample_rate` - Target audio sample rate (typically 44100 or 48000 Hz)
    #[must_use]
    pub fn with_sample_rate(sample_rate: u32) -> Self {
        Self {
            frame_counter: FrameCounter::new(),
            pulse1: PulseChannel::new(0),
            pulse2: PulseChannel::new(1),
            triangle: TriangleChannel::new(),
            noise: NoiseChannel::new(),
            dmc: DmcChannel::new(System::NTSC),
            mixer: Mixer::new(),
            resampler: Resampler::new(sample_rate),
            cycles: 0,
            memory_read_fn: None,
        }
    }

    /// Set the memory read callback for DMC DMA
    ///
    /// The DMC channel needs to read sample bytes from CPU memory.
    /// This callback will be invoked when the DMC needs to fetch a sample.
    ///
    /// # Arguments
    ///
    /// * `callback` - Function that reads a byte from the given address
    pub fn set_memory_read_callback<F>(&mut self, callback: F)
    where
        F: FnMut(u16) -> u8 + 'static,
    {
        self.memory_read_fn = Some(Box::new(callback));
    }

    /// Reads from an APU register
    ///
    /// # Arguments
    ///
    /// * `addr` - Register address ($4000-$4017)
    ///
    /// # Returns
    ///
    /// Register value, or 0 for write-only registers
    pub fn read_register(&mut self, addr: u16) -> u8 {
        match addr {
            0x4015 => self.read_status(),
            _ => 0, // All other APU registers are write-only
        }
    }

    /// Writes to an APU register
    ///
    /// # Arguments
    ///
    /// * `addr` - Register address ($4000-$4017)
    /// * `value` - Value to write
    #[allow(clippy::match_same_arms)] // TODO stubs will be implemented in future sprints
    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            // Pulse 1
            0x4000 => self.pulse1.write_register(0, value),
            0x4001 => self.pulse1.write_register(1, value),
            0x4002 => self.pulse1.write_register(2, value),
            0x4003 => self.pulse1.write_register(3, value),

            // Pulse 2
            0x4004 => self.pulse2.write_register(0, value),
            0x4005 => self.pulse2.write_register(1, value),
            0x4006 => self.pulse2.write_register(2, value),
            0x4007 => self.pulse2.write_register(3, value),

            // Triangle
            0x4008 => self.triangle.write_register(0, value),
            0x400A => self.triangle.write_register(2, value),
            0x400B => self.triangle.write_register(3, value),

            // Noise
            0x400C => self.noise.write_register(0, value),
            0x400E => self.noise.write_register(2, value),
            0x400F => self.noise.write_register(3, value),

            // DMC
            0x4010 => self.dmc.write_register(0, value),
            0x4011 => self.dmc.write_register(1, value),
            0x4012 => self.dmc.write_register(2, value),
            0x4013 => self.dmc.write_register(3, value),

            // Status
            0x4015 => self.write_status(value),

            // Frame counter
            0x4017 => {
                let action = self.frame_counter.write_control(value);
                self.process_frame_action(action);
            }

            _ => {}
        }
    }

    /// Process a frame counter action
    fn process_frame_action(&mut self, action: FrameAction) {
        match action {
            FrameAction::QuarterFrame => {
                // Clock envelopes and linear counter
                self.pulse1.clock_envelope();
                self.pulse2.clock_envelope();
                self.triangle.clock_linear_counter();
                self.noise.clock_envelope();
            }
            FrameAction::HalfFrame => {
                // Clock envelopes, linear counter, length counters, and sweep units
                self.pulse1.clock_envelope();
                self.pulse2.clock_envelope();
                self.triangle.clock_linear_counter();
                self.noise.clock_envelope();

                self.pulse1.clock_length_counter();
                self.pulse2.clock_length_counter();
                self.triangle.clock_length_counter();
                self.noise.clock_length_counter();

                self.pulse1.clock_sweep();
                self.pulse2.clock_sweep();
            }
            FrameAction::None => {}
        }
    }

    /// Reads the status register ($4015)
    ///
    /// # Status Register Format (Read)
    ///
    /// ```text
    /// 7  bit  0
    /// ---- ----
    /// IF-D NT21
    /// || | ||||
    /// || | |||+- Pulse 1 length counter > 0
    /// || | ||+-- Pulse 2 length counter > 0
    /// || | |+--- Triangle length counter > 0
    /// || | +---- Noise length counter > 0
    /// || +------ DMC bytes remaining > 0
    /// |+-------- Frame interrupt flag
    /// +--------- DMC interrupt flag
    /// ```
    ///
    /// Reading $4015 clears the frame IRQ flag.
    fn read_status(&mut self) -> u8 {
        let mut status = 0;

        // Bits 0-3: Channel length counter status
        if self.pulse1.length_counter_active() {
            status |= 0x01;
        }
        if self.pulse2.length_counter_active() {
            status |= 0x02;
        }
        if self.triangle.length_counter_active() {
            status |= 0x04;
        }
        if self.noise.length_counter_active() {
            status |= 0x08;
        }

        // Bit 4: DMC active (bytes remaining > 0)
        if self.dmc.is_active() {
            status |= 0x10;
        }

        // Bit 6: Frame IRQ
        if self.frame_counter.irq_flag {
            status |= 0x40;
        }

        // Bit 7: DMC IRQ
        if self.dmc.irq_pending() {
            status |= 0x80;
        }

        // Reading $4015 clears the frame IRQ flag
        // Note: It does NOT clear the DMC IRQ flag
        self.frame_counter.clear_irq();

        status
    }

    /// Writes to the status register ($4015)
    ///
    /// # Status Register Format (Write)
    ///
    /// ```text
    /// 7  bit  0
    /// ---- ----
    /// ---D NT21
    ///    | ||||
    ///    | |||+- Enable Pulse 1
    ///    | ||+-- Enable Pulse 2
    ///    | |+--- Enable Triangle
    ///    | +---- Enable Noise
    ///    +------ Enable DMC
    /// ```
    ///
    /// Writing to $4015:
    /// - Enables/disables channels
    /// - If a channel is disabled, its length counter is set to 0
    /// - If DMC is enabled with 0 bytes remaining, restarts the sample
    /// - Clears the DMC IRQ flag
    fn write_status(&mut self, value: u8) {
        self.pulse1.set_enabled((value & 0x01) != 0);
        self.pulse2.set_enabled((value & 0x02) != 0);
        self.triangle.set_enabled((value & 0x04) != 0);
        self.noise.set_enabled((value & 0x08) != 0);
        self.dmc.set_enabled((value & 0x10) != 0);

        // Writing to $4015 clears the DMC IRQ flag
        self.dmc.clear_irq();
    }

    /// Steps the APU by one CPU cycle
    ///
    /// This should be called once per CPU cycle to keep the APU synchronized.
    /// Generates audio samples internally which can be retrieved via [`samples()`](Self::samples).
    ///
    /// # Returns
    ///
    /// Frame action to be processed (if any)
    ///
    /// # Panics
    ///
    /// Panics if `memory_read_fn` is not set when DMC attempts DMA
    #[inline]
    pub fn step(&mut self) -> FrameAction {
        self.cycles += 1;

        // Clock channel timers
        // Pulse and Noise timers are clocked every other CPU cycle
        if self.cycles % 2 == 0 {
            self.pulse1.clock_timer();
            self.pulse2.clock_timer();
            self.noise.clock_timer();
        }

        // Triangle and DMC timers are clocked every CPU cycle
        self.triangle.clock_timer();

        // Clock DMC timer (may perform DMA)
        // For now, provide a dummy memory reader if none is set
        if let Some(ref mut memory_fn) = self.memory_read_fn {
            // Clock with actual memory reader
            let dma_cycles = self.dmc.clock_timer(memory_fn);
            // DMA cycles would need to be communicated back to the CPU
            // For now, we just track that DMA occurred
            let _ = dma_cycles;
        } else {
            // No memory callback set, clock with dummy reader
            self.dmc.clock_timer(|_| 0);
        }

        // Clock frame counter and handle frame actions
        let action = self.frame_counter.clock();
        self.process_frame_action(action);

        // Mix channel outputs and resample
        let mixed = self.mixer.mix(
            self.pulse1.output(),
            self.pulse2.output(),
            self.triangle.output(),
            self.noise.output(),
            self.dmc.output(),
        );

        // Add to resampler
        self.resampler.add_sample(mixed);

        action
    }

    /// Get the current mixed audio output from all channels
    ///
    /// This returns the instantaneous mixed output without resampling.
    /// For resampled output suitable for audio playback, use [`samples()`](Self::samples).
    ///
    /// # Returns
    ///
    /// Mixed audio sample (approximately 0.0-2.0 range)
    #[inline]
    #[must_use]
    pub fn output(&self) -> f32 {
        self.mixer.mix(
            self.pulse1.output(),
            self.pulse2.output(),
            self.triangle.output(),
            self.noise.output(),
            self.dmc.output(),
        )
    }

    /// Get resampled audio samples ready for playback
    ///
    /// Returns audio samples at the target sample rate (e.g., 48 kHz).
    /// Call [`clear_samples()`](Self::clear_samples) after consuming.
    ///
    /// # Returns
    ///
    /// Slice of audio samples at target sample rate
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        self.resampler.samples()
    }

    /// Clear the audio sample buffer
    ///
    /// Should be called after consuming samples via [`samples()`](Self::samples).
    pub fn clear_samples(&mut self) {
        self.resampler.clear();
    }

    /// Check if at least `min_samples` are available
    ///
    /// Useful for determining when to pull samples for audio output.
    ///
    /// # Arguments
    ///
    /// * `min_samples` - Minimum number of samples required
    #[must_use]
    pub fn samples_ready(&self, min_samples: usize) -> bool {
        self.resampler.is_ready(min_samples)
    }

    /// Returns whether a frame IRQ is pending
    #[must_use]
    pub fn irq_pending(&self) -> bool {
        self.frame_counter.irq_pending() || self.dmc.irq_pending()
    }

    /// Get DMC output (0-127)
    #[must_use]
    pub fn dmc_output(&self) -> u8 {
        self.dmc.output()
    }

    /// Returns the current CPU cycle count
    #[must_use]
    pub const fn cycles(&self) -> u64 {
        self.cycles
    }

    /// Resets the APU to power-on state
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apu_creation() {
        let apu = Apu::new();
        assert_eq!(apu.cycles, 0);
        assert!(!apu.pulse1.length_counter_active());
        assert!(!apu.irq_pending());
    }

    #[test]
    fn test_status_register_read() {
        let mut apu = Apu::new();

        // Enable all channels first
        apu.write_register(0x4015, 0x1F);

        // Load length counters for all channels
        apu.write_register(0x4003, 0x08); // Pulse 1
        apu.write_register(0x4007, 0x08); // Pulse 2
        apu.write_register(0x400B, 0x08); // Triangle
        apu.write_register(0x400F, 0x08); // Noise

        let status = apu.read_register(0x4015);
        // Bits 0-3 should be set (all channels have length > 0)
        assert_eq!(status & 0x0F, 0x0F); // All channels active
    }

    #[test]
    fn test_status_register_write() {
        let mut apu = Apu::new();

        // Enable all channels first
        apu.write_register(0x4015, 0x1F);

        // Load length counters
        apu.write_register(0x4003, 0x08); // Pulse 1
        apu.write_register(0x4007, 0x08); // Pulse 2
        apu.write_register(0x400B, 0x08); // Triangle
        apu.write_register(0x400F, 0x08); // Noise

        // Disable pulse 2 and noise, keep pulse 1 and triangle enabled
        apu.write_register(0x4015, 0x05);

        // Check that pulse 1 and triangle are enabled, pulse 2 and noise are disabled
        assert!(apu.pulse1.length_counter_active());
        assert!(!apu.pulse2.length_counter_active()); // Should be 0 after disabling
        assert!(apu.triangle.length_counter_active());
        assert!(!apu.noise.length_counter_active()); // Should be 0 after disabling
        assert!(!apu.dmc.is_active()); // DMC not active (no bytes remaining)
    }

    #[test]
    fn test_frame_counter_control() {
        let mut apu = Apu::new();

        // Write to frame counter (5-step mode)
        apu.write_register(0x4017, 0x80);

        // Should not generate IRQ in 5-step mode
        for _ in 0..40000 {
            apu.step();
        }

        assert!(!apu.irq_pending());
    }

    #[test]
    fn test_4step_mode_irq_generation() {
        let mut apu = Apu::new();

        // Write $00 to $4017: 4-step mode, IRQ enabled
        apu.write_register(0x4017, 0x00);

        // Initially no IRQ pending
        assert!(!apu.irq_pending());

        // Clock for 29829 cycles - no IRQ yet (delayed by 1 cycle in fix)
        for _ in 0..29829 {
            apu.step();
        }
        assert!(!apu.irq_pending());

        // Clock one more time to cycle 29830 - IRQ should fire
        apu.step();
        assert!(apu.irq_pending(), "IRQ should be pending at cycle 29830");

        // Verify $4015 shows bit 6 set
        let status = apu.read_register(0x4015);
        assert_eq!(status & 0x40, 0x40, "$4015 bit 6 should be set");

        // Reading $4015 clears the IRQ
        assert!(
            !apu.irq_pending(),
            "IRQ should be cleared after reading $4015"
        );
    }

    #[test]
    fn test_read_clears_frame_irq() {
        let mut apu = Apu::new();

        // Set frame IRQ
        apu.frame_counter.irq_flag = true;
        assert!(apu.frame_counter.irq_flag);

        // Read status should clear it
        let status = apu.read_register(0x4015);
        assert_eq!(status & 0x40, 0x40); // IRQ flag was set

        // Now IRQ flag should be cleared
        assert!(!apu.frame_counter.irq_flag);

        // Read again should show cleared
        let status = apu.read_register(0x4015);
        assert_eq!(status & 0x40, 0x00);
    }

    #[test]
    fn test_step_increments_cycles() {
        let mut apu = Apu::new();

        assert_eq!(apu.cycles(), 0);

        apu.step();
        assert_eq!(apu.cycles(), 1);

        for _ in 0..99 {
            apu.step();
        }

        assert_eq!(apu.cycles(), 100);
    }

    #[test]
    fn test_reset() {
        let mut apu = Apu::new();

        // Enable channels first, then load length counters
        apu.write_register(0x4015, 0x1F);
        apu.write_register(0x4003, 0x08);

        // Modify state
        for _ in 0..1000 {
            apu.step();
        }

        assert_ne!(apu.cycles(), 0);
        assert!(apu.pulse1.length_counter_active());

        // Reset
        apu.reset();

        assert_eq!(apu.cycles(), 0);
        assert!(!apu.pulse1.length_counter_active());
    }
}

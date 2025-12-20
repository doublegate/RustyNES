//! NES console emulation coordinator.
//!
//! This module orchestrates the CPU, PPU, APU, and mapper with cycle-accurate timing.
//!
//! # Timing Model
//!
//! - Master clock: 21.477272 MHz (NTSC)
//! - CPU: 1.789773 MHz (master ÷ 12) = 1.79 MHz
//! - PPU: 5.369318 MHz (master ÷ 4) = 3× CPU clock
//! - APU: Clocked every CPU cycle
//! - Frame: 29,780 CPU cycles, 89,341 PPU dots (odd frames skip 1 dot)
//!
//! # Example
//!
//! ```no_run
//! use rustynes_core::Console;
//! use std::fs;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let rom_data = fs::read("game.nes")?;
//! let mut console = Console::from_rom_bytes(&rom_data)?;
//!
//! // Main loop
//! loop {
//!     console.step_frame();
//!     let framebuffer = console.framebuffer();
//!     // ... render framebuffer ...
//! }
//! # }
//! ```

use crate::bus::Bus as SystemBus;
use crate::input::Button;
use rustynes_cpu::{Bus as CpuBus, Cpu};
use rustynes_mappers::{create_mapper, Mapper, Rom, RomError};
use thiserror::Error;

/// Console creation error
#[derive(Debug, Error)]
pub enum ConsoleError {
    /// ROM loading error
    #[error("ROM error: {0}")]
    Rom(#[from] RomError),

    /// Mapper creation error
    #[error("Mapper error: {0}")]
    Mapper(#[from] rustynes_mappers::MapperError),
}

/// NES console emulator
///
/// Coordinates CPU, PPU, APU, and cartridge mapper with cycle-accurate timing.
pub struct Console {
    /// 6502 CPU
    cpu: Cpu,

    /// Memory bus (RAM, PPU, APU, mappers, controllers)
    bus: SystemBus,

    /// Total cycles executed
    master_cycles: u64,

    /// Current frame count
    frame_count: u64,
}

impl Console {
    /// Create a new console from ROM data
    ///
    /// # Arguments
    ///
    /// * `rom_data` - Raw ROM file bytes (iNES or NES 2.0 format)
    ///
    /// # Returns
    ///
    /// New Console instance or error if ROM is invalid
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - ROM format is invalid
    /// - Mapper is not supported
    /// - ROM configuration is invalid
    pub fn from_rom_bytes(rom_data: &[u8]) -> Result<Self, ConsoleError> {
        let rom = Rom::load(rom_data)?;
        let mapper = create_mapper(&rom)?;
        Ok(Self::new(mapper))
    }

    /// Create a new console with a custom mapper
    ///
    /// # Arguments
    ///
    /// * `mapper` - Cartridge mapper implementation
    ///
    /// # Returns
    ///
    /// New Console instance
    #[must_use]
    pub fn new(mapper: Box<dyn Mapper>) -> Self {
        let mut cpu = Cpu::new();
        let mut bus = SystemBus::new(mapper);

        // Reset CPU (loads PC from $FFFC-$FFFD)
        cpu.reset(&mut bus);

        Self {
            cpu,
            bus,
            master_cycles: 0,
            frame_count: 0,
        }
    }

    /// Reset the console (simulates pressing the RESET button)
    pub fn reset(&mut self) {
        self.cpu.reset(&mut self.bus);
        self.bus.reset();
        self.master_cycles = 0;
        // Note: Don't reset frame_count (preserve for debugging)
    }

    /// Execute one CPU instruction (coarse-grained execution)
    ///
    /// This method runs one complete CPU instruction, then catches up
    /// the PPU and APU. Use `tick()` for cycle-accurate execution.
    ///
    /// # Returns
    ///
    /// Number of cycles consumed
    pub fn step(&mut self) -> u8 {
        // Step CPU (this will handle DMA internally via stall cycles)
        let cpu_cycles = self.cpu.step(&mut self.bus);

        // Track CPU cycles for DMA timing (odd/even cycle detection)
        self.bus.add_cpu_cycles(cpu_cycles);

        // Step PPU (3 dots per CPU cycle)
        // Use step_ppu which provides CHR ROM access from mapper for tile fetching
        for _ in 0..(cpu_cycles * 3) {
            let (frame_complete, nmi) = self.bus.step_ppu();

            // Trigger NMI if PPU requests it
            if nmi {
                self.cpu.trigger_nmi();
            }

            // Check for frame completion
            if frame_complete {
                self.frame_count += 1;
            }
        }

        // Step APU (1 step per CPU cycle)
        for _ in 0..cpu_cycles {
            self.bus.apu.step();
        }

        // Clock mapper (for cycle-based timers)
        self.bus.clock_mapper(cpu_cycles);

        // Check for mapper IRQ
        if self.bus.mapper_irq_pending() {
            self.cpu.set_irq(true);
        }

        // Handle DMA if active
        while self.bus.dma_active() {
            if self.bus.tick_dma() {
                break;
            }
        }

        // Update master cycle count
        self.master_cycles += u64::from(cpu_cycles);

        cpu_cycles
    }

    /// Execute exactly one CPU cycle with perfect PPU/APU synchronization.
    ///
    /// This is the cycle-accurate execution mode. Each call:
    /// - Advances the CPU by exactly one cycle
    /// - Advances the PPU by exactly 3 dots (3:1 PPU:CPU ratio)
    /// - Advances the APU by one step
    ///
    /// # Returns
    ///
    /// `(instruction_complete, frame_complete)`:
    /// - `instruction_complete`: true when a CPU instruction boundary is reached
    /// - `frame_complete`: true when a video frame has just been completed
    ///
    /// # Example
    ///
    /// ```no_run
    /// use rustynes_core::Console;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let rom_data = vec![0; 16 + 16384 + 8192];
    /// let mut console = Console::from_rom_bytes(&rom_data)?;
    ///
    /// // Cycle-accurate main loop
    /// loop {
    ///     let (instr_done, frame_done) = console.tick();
    ///
    ///     if frame_done {
    ///         let framebuffer = console.framebuffer();
    ///         // ... render to screen ...
    ///     }
    /// }
    /// # }
    /// ```
    pub fn tick(&mut self) -> (bool, bool) {
        let mut frame_complete = false;

        // Handle DMA if active (DMA steals CPU cycles)
        if self.bus.dma_active() {
            let dma_done = self.bus.tick_dma();
            if !dma_done {
                // DMA cycle - still advance PPU and APU
                for _ in 0..3 {
                    let (fc, nmi) = self.bus.step_ppu();
                    if nmi {
                        self.cpu.trigger_nmi();
                    }
                    if fc {
                        frame_complete = true;
                        self.frame_count += 1;
                    }
                }
                self.bus.apu.step();
                self.master_cycles += 1;
                return (false, frame_complete);
            }
        }

        // Execute one CPU cycle
        let instruction_complete = self.cpu.tick(&mut self.bus);

        // Track CPU cycles for DMA timing
        self.bus.add_cpu_cycles(1);

        // Step PPU (3 dots per CPU cycle)
        for _ in 0..3 {
            let (fc, nmi) = self.bus.step_ppu();
            if nmi {
                self.cpu.trigger_nmi();
            }
            if fc {
                frame_complete = true;
                self.frame_count += 1;
            }
        }

        // Step APU (1 step per CPU cycle)
        self.bus.apu.step();

        // Clock mapper (once per CPU cycle for cycle-based timing)
        self.bus.clock_mapper(1);

        // Check for mapper IRQ
        if self.bus.mapper_irq_pending() {
            self.cpu.set_irq(true);
        }

        // Update master cycle count
        self.master_cycles += 1;

        (instruction_complete, frame_complete)
    }

    /// Execute instructions until a complete frame is rendered using cycle-accurate mode.
    ///
    /// This method uses the cycle-accurate `tick()` method for precise timing.
    /// Use this when you need perfect emulation accuracy.
    pub fn step_frame_accurate(&mut self) {
        let target_frame = self.frame_count + 1;

        // Run until frame is complete
        while self.frame_count < target_frame {
            self.tick();

            // Safety check: prevent infinite loop
            // A frame is ~29,780 CPU cycles, give it some margin
            if self.master_cycles > (target_frame * 35000) {
                log::error!("Frame didn't complete in expected time (accurate mode)");
                break;
            }
        }
    }

    /// Execute instructions until a complete frame is rendered
    ///
    /// A frame consists of 29,780 CPU cycles (89,341 PPU dots).
    pub fn step_frame(&mut self) {
        let target_frame = self.frame_count + 1;

        // Run until frame is complete
        while self.frame_count < target_frame {
            self.step();

            // Safety check: prevent infinite loop if PPU doesn't generate frames
            if self.master_cycles > (target_frame * 30000) {
                log::error!("Frame didn't complete in expected time");
                break;
            }
        }
    }

    /// Get reference to framebuffer (256×240 palette indices)
    ///
    /// # Returns
    ///
    /// Slice of palette indices (0-63), row-major order
    ///
    /// To convert to RGB, use the NES palette lookup table.
    #[must_use]
    pub fn framebuffer(&self) -> &[u8] {
        self.bus.ppu.frame_buffer()
    }

    /// Get total CPU cycles executed
    #[must_use]
    pub fn cycles(&self) -> u64 {
        self.master_cycles
    }

    /// Get current frame number
    #[must_use]
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    // === Input Methods ===

    /// Set controller 1 button state
    ///
    /// # Arguments
    ///
    /// * `button` - Button to set
    /// * `pressed` - true if pressed, false if released
    pub fn set_button_1(&mut self, button: Button, pressed: bool) {
        self.bus.controller1.set_button(button, pressed);
    }

    /// Set controller 2 button state
    ///
    /// # Arguments
    ///
    /// * `button` - Button to set
    /// * `pressed` - true if pressed, false if released
    pub fn set_button_2(&mut self, button: Button, pressed: bool) {
        self.bus.controller2.set_button(button, pressed);
    }

    /// Set all controller 1 buttons at once
    ///
    /// # Arguments
    ///
    /// * `buttons` - 8-bit field where each bit represents a button:
    ///   - Bit 0: A
    ///   - Bit 1: B
    ///   - Bit 2: Select
    ///   - Bit 3: Start
    ///   - Bit 4: Up
    ///   - Bit 5: Down
    ///   - Bit 6: Left
    ///   - Bit 7: Right
    pub fn set_controller_1(&mut self, buttons: u8) {
        self.bus.controller1.set_buttons(buttons);
    }

    /// Set all controller 2 buttons at once
    ///
    /// # Arguments
    ///
    /// * `buttons` - 8-bit field (see `set_controller_1` for bit layout)
    pub fn set_controller_2(&mut self, buttons: u8) {
        self.bus.controller2.set_buttons(buttons);
    }

    /// Get controller 1 button state
    ///
    /// # Arguments
    ///
    /// * `button` - Button to check
    ///
    /// # Returns
    ///
    /// true if pressed, false if released
    #[must_use]
    pub fn get_button_1(&self, button: Button) -> bool {
        self.bus.controller1.get_button(button)
    }

    /// Get controller 2 button state
    ///
    /// # Arguments
    ///
    /// * `button` - Button to check
    ///
    /// # Returns
    ///
    /// true if pressed, false if released
    #[must_use]
    pub fn get_button_2(&self, button: Button) -> bool {
        self.bus.controller2.get_button(button)
    }

    /// Get all controller 1 buttons
    ///
    /// # Returns
    ///
    /// 8-bit field where each bit represents a button (see `set_controller_1`)
    #[must_use]
    pub fn controller_1_buttons(&self) -> u8 {
        self.bus.controller1.buttons()
    }

    /// Get all controller 2 buttons
    ///
    /// # Returns
    ///
    /// 8-bit field where each bit represents a button (see `set_controller_1`)
    #[must_use]
    pub fn controller_2_buttons(&self) -> u8 {
        self.bus.controller2.buttons()
    }

    // === Audio Methods ===

    /// Get audio samples ready for playback
    ///
    /// Returns audio samples at the target sample rate (e.g., 48 kHz).
    /// Call [`clear_audio_samples()`](Self::clear_audio_samples) after consuming.
    ///
    /// # Returns
    ///
    /// Slice of audio samples at target sample rate
    #[must_use]
    pub fn audio_samples(&self) -> &[f32] {
        self.bus.apu.samples()
    }

    /// Clear the audio sample buffer
    ///
    /// Should be called after consuming samples via [`audio_samples()`](Self::audio_samples).
    pub fn clear_audio_samples(&mut self) {
        self.bus.apu.clear_samples();
    }

    /// Check if at least `min_samples` audio samples are available
    ///
    /// Useful for determining when to pull samples for audio output.
    ///
    /// # Arguments
    ///
    /// * `min_samples` - Minimum number of samples required
    #[must_use]
    pub fn audio_samples_ready(&self, min_samples: usize) -> bool {
        self.bus.apu.samples_ready(min_samples)
    }

    // === Testing/Debugging Methods ===

    /// Read memory at address without side effects (for testing)
    ///
    /// This uses the bus peek method which doesn't trigger any hardware side effects.
    /// Useful for test ROMs that write result codes to specific addresses.
    ///
    /// # Arguments
    ///
    /// * `addr` - Memory address to read
    ///
    /// # Returns
    ///
    /// Value at the specified address
    #[must_use]
    pub fn peek_memory(&self, addr: u16) -> u8 {
        self.bus.peek(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_ppu::{FRAME_HEIGHT, FRAME_WIDTH};

    fn create_test_rom() -> Vec<u8> {
        let mut rom_data = vec![0; 16 + 16384 + 8192]; // Header + 16KB PRG + 8KB CHR

        // iNES header
        rom_data[0..4].copy_from_slice(b"NES\x1A");
        rom_data[4] = 1; // 1 PRG bank (16KB)
        rom_data[5] = 1; // 1 CHR bank (8KB)
        rom_data[6] = 0; // Horizontal mirroring, NROM

        // Set RESET vector to $8000 (at offset 0x3FFC for 16KB ROM)
        rom_data[16 + 0x3FFC] = 0x00;
        rom_data[16 + 0x3FFD] = 0x80;

        // Write a simple infinite loop at $8000: JMP $8000
        rom_data[16] = 0x4C; // JMP absolute
        rom_data[17] = 0x00;
        rom_data[18] = 0x80;

        rom_data
    }

    #[test]
    fn test_console_creation() {
        let rom_data = create_test_rom();
        let console = Console::from_rom_bytes(&rom_data).unwrap();

        assert_eq!(console.frame_count(), 0);
        assert_eq!(console.framebuffer().len(), FRAME_WIDTH * FRAME_HEIGHT);
    }

    #[test]
    fn test_console_reset() {
        let rom_data = create_test_rom();
        let mut console = Console::from_rom_bytes(&rom_data).unwrap();

        // Execute some cycles
        for _ in 0..100 {
            console.step();
        }

        let cycles_before = console.cycles();
        assert!(cycles_before > 0);

        // Reset
        console.reset();

        // Cycles should be cleared
        assert_eq!(console.cycles(), 0);
    }

    #[test]
    fn test_step_execution() {
        let rom_data = create_test_rom();
        let mut console = Console::from_rom_bytes(&rom_data).unwrap();

        // Step once
        let cycles = console.step();

        assert!(cycles > 0);
        assert!(console.cycles() > 0);
    }

    #[test]
    fn test_frame_execution() {
        let rom_data = create_test_rom();
        let mut console = Console::from_rom_bytes(&rom_data).unwrap();

        // Execute one frame
        console.step_frame();

        assert_eq!(console.frame_count(), 1);
    }

    #[test]
    fn test_controller_input() {
        let rom_data = create_test_rom();
        let mut console = Console::from_rom_bytes(&rom_data).unwrap();

        // Test button setting
        console.set_button_1(Button::A, true);
        assert!(console.get_button_1(Button::A));

        console.set_button_1(Button::A, false);
        assert!(!console.get_button_1(Button::A));

        // Test setting all buttons
        console.set_controller_1(0b1111_1111);
        assert_eq!(console.controller_1_buttons(), 0b1111_1111);

        console.set_controller_1(0);
        assert_eq!(console.controller_1_buttons(), 0);
    }

    #[test]
    fn test_two_controllers() {
        let rom_data = create_test_rom();
        let mut console = Console::from_rom_bytes(&rom_data).unwrap();

        // Set different buttons on each controller
        console.set_button_1(Button::A, true);
        console.set_button_2(Button::B, true);

        assert!(console.get_button_1(Button::A));
        assert!(!console.get_button_1(Button::B));

        assert!(!console.get_button_2(Button::A));
        assert!(console.get_button_2(Button::B));
    }
}

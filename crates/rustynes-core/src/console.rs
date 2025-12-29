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
use rustynes_cpu::{Cpu, CpuBus};
use rustynes_mappers::{Mapper, Rom, RomError, create_mapper};
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

    /// Create a new console from ROM data with custom sample rate
    ///
    /// # Arguments
    ///
    /// * `rom_data` - Raw ROM file bytes (iNES or NES 2.0 format)
    /// * `sample_rate` - Audio output sample rate (e.g., 44100 or 48000 Hz)
    ///
    /// # Returns
    ///
    /// New Console instance with APU configured for the specified sample rate
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - ROM format is invalid
    /// - Mapper is not supported
    /// - ROM configuration is invalid
    pub fn from_rom_bytes_with_sample_rate(
        rom_data: &[u8],
        sample_rate: u32,
    ) -> Result<Self, ConsoleError> {
        let rom = Rom::load(rom_data)?;
        let mapper = create_mapper(&rom)?;
        Ok(Self::with_sample_rate(mapper, sample_rate))
    }

    /// Create a new console with a custom mapper
    ///
    /// # Arguments
    ///
    /// * `mapper` - Cartridge mapper implementation
    ///
    /// # Returns
    ///
    /// New Console instance (APU defaults to 48000 Hz sample rate)
    #[must_use]
    pub fn new(mapper: Box<dyn Mapper>) -> Self {
        Self::with_sample_rate(mapper, 48000)
    }

    /// Create a new console with a custom mapper and audio sample rate
    ///
    /// # Arguments
    ///
    /// * `mapper` - Cartridge mapper implementation
    /// * `sample_rate` - Audio output sample rate (e.g., 44100 or 48000 Hz)
    ///
    /// # Returns
    ///
    /// New Console instance with APU configured for the specified sample rate
    #[must_use]
    pub fn with_sample_rate(mapper: Box<dyn Mapper>, sample_rate: u32) -> Self {
        let mut cpu = Cpu::new();
        let mut bus = SystemBus::with_sample_rate(mapper, sample_rate);

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
    #[inline]
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

        // Update IRQ line (level-triggered: high when any source is active)
        let irq = self.bus.mapper_irq_pending() || self.bus.apu.irq_pending();
        self.cpu.set_irq(irq);

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
    /// - PPU and APU are stepped via `on_cpu_cycle()` BEFORE each CPU memory access
    /// - Handles DMC DMA stalls (CPU halted while PPU/APU continue)
    ///
    /// # Timing Model
    ///
    /// PPU is stepped BEFORE each CPU memory access (inside `cpu.tick()`) to ensure
    /// accurate `VBlank` flag detection. When CPU reads $2002 (PPUSTATUS), PPU state
    /// has already been updated. This is critical for passing ppu_02-vbl_set_time
    /// and ppu_03-vbl_clear_time tests which require ±2 cycle accuracy.
    ///
    /// # DMC DMA Stalls
    ///
    /// When DMC performs DMA to fetch sample bytes, it steals CPU cycles (typically 3-4).
    /// During these stalls:
    /// - CPU is halted (no instruction progress)
    /// - PPU continues (3 dots per stall cycle)
    /// - APU continues (1 cycle per stall cycle)
    /// - Mapper is clocked (1 cycle per stall cycle)
    ///
    /// # Architecture Note
    ///
    /// Unlike the old architecture where Console manually stepped PPU before CPU,
    /// the new cycle-accurate architecture uses the `CpuBus::on_cpu_cycle()` callback.
    /// When CPU calls `read_cycle()` or `write_cycle()`, it first calls `on_cpu_cycle()`
    /// which steps PPU 3 dots and APU 1 cycle. This ensures PPU state is current when
    /// CPU observes memory.
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
    #[inline]
    pub fn tick(&mut self) -> (bool, bool) {
        // Handle OAM DMA if active (DMA steals CPU cycles)
        // During DMA, CPU doesn't execute so we must manually step PPU/APU
        if self.bus.dma_active() {
            // Manually step PPU (3 dots per CPU cycle) during DMA
            for _ in 0..3 {
                let (fc, nmi) = self.bus.step_ppu();
                if nmi {
                    self.cpu.trigger_nmi();
                }
                if fc {
                    self.frame_count += 1;
                    // Note: frame_complete captured below from bus
                }
            }

            // Manually step APU during DMA (ignore DMC stalls during OAM DMA)
            let _ = self.bus.apu.step();

            // Clock mapper during DMA
            self.bus.clock_mapper(1);

            let dma_done = self.bus.tick_dma();
            if !dma_done {
                self.master_cycles += 1;
                return (false, self.bus.take_frame_complete());
            }
        }

        // Execute one CPU cycle
        // This calls on_cpu_cycle() internally BEFORE each memory access,
        // which steps PPU (3 dots), APU (1 cycle), mapper (1 cycle), and
        // captures NMI/frame_complete/dmc_stalls
        let instruction_complete = self.cpu.tick(&mut self.bus);

        // Handle NMI from PPU (set during on_cpu_cycle via step_ppu)
        if self.bus.take_nmi() {
            self.cpu.trigger_nmi();
        }

        // Check for frame completion
        let mut frame_complete = self.bus.take_frame_complete();
        if frame_complete {
            self.frame_count += 1;
        }

        // Handle DMC DMA stall cycles
        // During DMC stalls, CPU is halted but PPU/APU/mapper continue
        let dmc_stalls = self.bus.take_dmc_stall_cycles();
        for _ in 0..dmc_stalls {
            // Step PPU 3 dots per stall cycle
            for _ in 0..3 {
                let (fc, nmi) = self.bus.step_ppu();
                if nmi {
                    self.cpu.trigger_nmi();
                }
                if fc {
                    self.frame_count += 1;
                    frame_complete = true;
                }
            }

            // Step APU once per stall cycle (ignore nested DMC stalls)
            let _ = self.bus.apu.step();

            // Clock mapper once per stall cycle
            self.bus.clock_mapper(1);

            // Count the stall cycle
            self.master_cycles += 1;
        }

        // Note: Mapper is already clocked in on_cpu_cycle() which is called
        // during cpu.tick() for each memory access. No need to clock again here.

        // Update IRQ line (level-triggered: high when any source is active)
        let irq = self.bus.mapper_irq_pending() || self.bus.apu.irq_pending();
        self.cpu.set_irq(irq);

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

    // === Accessor Methods (for debugging/testing) ===

    /// Get reference to the memory bus
    #[must_use]
    pub fn bus(&self) -> &SystemBus {
        &self.bus
    }

    /// Get reference to the CPU
    #[must_use]
    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    /// Get mutable reference to the CPU (internal use/testing)
    pub fn cpu_mut(&mut self) -> &mut Cpu {
        &mut self.cpu
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
        let mut rom_data = create_test_rom();
        // At 0x8000 (start of ROM): LDA #$42; STA $0000
        // LDA #$42: A9 42
        // STA $0000: 8D 00 00
        // Offset 16 (Header)
        rom_data[16] = 0xA9;
        rom_data[17] = 0x42;
        rom_data[18] = 0x8D;
        rom_data[19] = 0x00;
        rom_data[20] = 0x00;

        let mut console = Console::from_rom_bytes(&rom_data).unwrap();

        // Step for a while
        // 1 cycle = ~559ns. Frame = ~29780 cycles.
        // Step for 100 cycles
        for _ in 0..100 {
            console.step();
        }

        assert!(console.cpu().get_cycles() >= 100);
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

    #[test]
    fn test_console_nmi() {
        let mut rom_data = create_test_rom();

        // Setup NMI vector at $FFFA to point to handler at $8010
        // $FFFA is at offset $7FFA in ROM data
        rom_data[16 + 0x3FFA] = 0x10;
        rom_data[16 + 0x3FFB] = 0x80;

        // Main code at $8000: Loop forever
        // 8000: 4C 00 80 (JMP $8000)
        rom_data[16] = 0x4C;
        rom_data[17] = 0x00;
        rom_data[18] = 0x80;

        // NMI Handler at $8010: RTI
        // 8010: 40 (RTI)
        // Offset 0x0010
        rom_data[16 + 0x0010] = 0xA9; // LDA immediate
        rom_data[16 + 0x0011] = 0x01; // #$01
        rom_data[16 + 0x0012] = 0x40; // RTI

        let mut console = Console::from_rom_bytes(&rom_data).unwrap();

        // Run main loop
        for _ in 0..100 {
            console.step();
        }

        // Trigger NMI
        console.cpu_mut().trigger_nmi();

        // Run until inside handler
        let mut in_handler = false;
        for _ in 0..100 {
            console.step();
            let pc = console.cpu().pc;
            if (0x8010..=0x8012).contains(&pc) {
                in_handler = true;
                break;
            }
        }

        assert!(in_handler, "CPU did not enter NMI handler");
    }

    #[test]
    fn test_console_irq() {
        let mut rom_data = create_test_rom();

        // Setup IRQ vector at $FFFE to point to handler at $8020
        rom_data[16 + 0x3FFE] = 0x20;
        rom_data[16 + 0x3FFF] = 0x80;

        // Main code: CLI (enable interrupts), then Loop
        // 8000: 58 (CLI)
        // 8001: 4C 01 80 (JMP $8001)
        rom_data[16] = 0x58;
        rom_data[17] = 0x4C;
        rom_data[18] = 0x01;
        rom_data[19] = 0x80;

        // IRQ Handler at $8020: RTI
        // 8020: A9 02 (LDA #$02)
        // 8022: 40 (RTI)
        // Offset 0x0020
        rom_data[16 + 0x0020] = 0xA9; // LDA immediate
        rom_data[16 + 0x0021] = 0x02; // #$02
        rom_data[16 + 0x0022] = 0x40; // RTI

        let mut console = Console::from_rom_bytes(&rom_data).unwrap();

        // Run to enable interrupts
        for _ in 0..100 {
            console.step();
        }

        // Assert IRQ line
        console.cpu_mut().set_irq(true);

        // Run until inside handler
        let mut in_handler = false;
        // Run enough cycles to trigger IRQ logic
        for _cycle in 0..100_000 {
            console.step();
            let pc = console.cpu().pc;
            if (0x8020..=0x8024).contains(&pc) {
                in_handler = true;
                break;
            }
        }

        assert!(in_handler, "CPU did not enter IRQ handler");
    }
}

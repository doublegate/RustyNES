//! NES Console Implementation.
//!
//! The Console struct provides the high-level emulation API, managing the
//! CPU, PPU, APU, and mapper integration with proper timing.

use crate::bus::{ControllerState, NesBus};
use rustynes_cpu::{Cpu, Status};
use rustynes_mappers::{Mapper, Rom, RomError, create_mapper};

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, vec::Vec};

/// NES emulation timing constants.
pub mod timing {
    /// Master clock frequency (NTSC).
    pub const MASTER_CLOCK_NTSC: u32 = 21_477_272;
    /// CPU clock frequency (NTSC).
    pub const CPU_CLOCK_NTSC: u32 = MASTER_CLOCK_NTSC / 12;
    /// PPU clock frequency (NTSC).
    pub const PPU_CLOCK_NTSC: u32 = MASTER_CLOCK_NTSC / 4;
    /// CPU cycles per frame (NTSC).
    pub const CPU_CYCLES_PER_FRAME: u32 = 29_780;
    /// PPU dots per scanline.
    pub const PPU_DOTS_PER_SCANLINE: u16 = 341;
    /// Total scanlines (including vblank).
    pub const PPU_SCANLINES: u16 = 262;
    /// Target frame rate (NTSC).
    pub const FRAME_RATE_NTSC: f64 = 60.0988;
}

/// Console error type.
#[derive(Debug, Clone)]
pub enum ConsoleError {
    /// ROM loading error.
    RomError(RomError),
    /// Invalid state.
    InvalidState(String),
}

impl From<RomError> for ConsoleError {
    fn from(err: RomError) -> Self {
        Self::RomError(err)
    }
}

impl core::fmt::Display for ConsoleError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RomError(e) => write!(f, "ROM error: {e}"),
            Self::InvalidState(msg) => write!(f, "Invalid state: {msg}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ConsoleError {}

/// NES console emulator.
pub struct Console {
    /// 6502 CPU.
    cpu: Cpu,
    /// System bus (PPU, APU, mapper, RAM).
    bus: NesBus,
    /// Frame buffer (256x240 RGBA).
    framebuffer: Vec<u8>,
    /// Audio sample buffer.
    audio_buffer: Vec<f32>,
    /// Total CPU cycles executed.
    total_cycles: u64,
    /// Frame counter.
    frame_count: u64,
    /// Is emulation running?
    running: bool,
}

impl Console {
    /// Create a new console with the given ROM.
    ///
    /// # Errors
    ///
    /// Returns an error if the ROM cannot be loaded or uses an unsupported mapper.
    pub fn new(rom_data: &[u8]) -> Result<Self, ConsoleError> {
        let rom = Rom::load(rom_data)?;
        let mapper = create_mapper(&rom)?;
        Self::with_mapper(mapper)
    }

    /// Create a new console from ROM bytes (alias for `new`).
    ///
    /// # Errors
    ///
    /// Returns an error if the ROM cannot be loaded or uses an unsupported mapper.
    pub fn from_rom_bytes(rom_data: &[u8]) -> Result<Self, ConsoleError> {
        Self::new(rom_data)
    }

    /// Create a new console from ROM bytes with sample rate configuration.
    ///
    /// Note: The sample rate is currently unused as the APU handles resampling internally.
    ///
    /// # Errors
    ///
    /// Returns an error if the ROM cannot be loaded or uses an unsupported mapper.
    pub fn from_rom_bytes_with_sample_rate(
        rom_data: &[u8],
        _sample_rate: u32,
    ) -> Result<Self, ConsoleError> {
        Self::new(rom_data)
    }

    /// Create a console with a pre-created mapper.
    ///
    /// # Errors
    ///
    /// Returns an error if the console cannot be initialized.
    pub fn with_mapper(mapper: Box<dyn Mapper>) -> Result<Self, ConsoleError> {
        let bus = NesBus::new(mapper);
        let cpu = Cpu::new();

        Ok(Self {
            cpu,
            bus,
            framebuffer: vec![0; 256 * 240 * 4],
            audio_buffer: Vec::with_capacity(2048),
            total_cycles: 0,
            frame_count: 0,
            running: true,
        })
    }

    /// Reset the console to initial state.
    pub fn reset(&mut self) {
        self.cpu.reset(&mut self.bus);
        self.bus.reset();
        self.total_cycles = 0;
        self.running = true;
    }

    /// Power on the console (cold boot).
    pub fn power_on(&mut self) {
        self.reset();
    }

    /// Run emulation for one CPU instruction.
    ///
    /// Returns the number of CPU cycles executed.
    pub fn step(&mut self) -> u8 {
        if !self.running {
            return 0;
        }

        // Handle DMC DMA stall
        if self.bus.dmc_stall_active() {
            self.bus.decrement_dmc_stall();
            self.step_components(1);
            return 1;
        }

        // Handle OAM DMA
        if self.bus.oam_dma_pending() {
            let dma_cycles = self.bus.execute_oam_dma();
            self.step_components(dma_cycles);
            return dma_cycles as u8;
        }

        // Handle interrupts
        if self.bus.nmi_pending() {
            self.bus.acknowledge_nmi();
            self.cpu.trigger_nmi();
        } else if self.bus.irq_pending() && !self.cpu.status().contains(Status::I) {
            self.cpu.set_irq(true);
        }

        // Execute one CPU instruction
        let cycles = self.cpu.step(&mut self.bus);
        self.step_components(u16::from(cycles));
        self.bus.add_cpu_cycles(cycles);
        self.total_cycles += u64::from(cycles);

        cycles
    }

    /// Step PPU and APU for the given number of CPU cycles.
    fn step_components(&mut self, cpu_cycles: u16) {
        for _ in 0..cpu_cycles {
            // Step PPU (3 dots per CPU cycle)
            self.bus.step_ppu();

            // Step APU (1:1 with CPU)
            if let Some(sample) = self.bus.step_apu() {
                self.audio_buffer.push(sample);
            }
        }
    }

    /// Run emulation for one frame (approximately 29,780 CPU cycles).
    ///
    /// Returns the actual number of CPU cycles executed.
    pub fn step_frame(&mut self) -> u64 {
        let start_cycles = self.total_cycles;
        let target_cycles = self.total_cycles + u64::from(timing::CPU_CYCLES_PER_FRAME);

        while self.total_cycles < target_cycles && self.running {
            self.step();
        }

        // Copy PPU framebuffer
        self.update_framebuffer();
        self.frame_count += 1;

        self.total_cycles - start_cycles
    }

    /// Run emulation for one frame with cycle-accurate timing.
    ///
    /// This is an alias for `step_frame()` for API compatibility.
    /// Returns the actual number of CPU cycles executed.
    pub fn step_frame_accurate(&mut self) -> u64 {
        self.step_frame()
    }

    /// Update the framebuffer from PPU output.
    fn update_framebuffer(&mut self) {
        let ppu_buffer = self.bus.ppu.frame_buffer();

        // Convert PPU palette indices to RGBA
        for (i, &palette_idx) in ppu_buffer.iter().enumerate() {
            let rgb = crate::palette::NES_PALETTE[palette_idx as usize & 0x3F];
            let offset = i * 4;
            self.framebuffer[offset] = rgb.0; // R
            self.framebuffer[offset + 1] = rgb.1; // G
            self.framebuffer[offset + 2] = rgb.2; // B
            self.framebuffer[offset + 3] = 255; // A
        }
    }

    /// Get the current framebuffer (256x240 RGBA).
    #[must_use]
    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    /// Take the audio buffer (drains accumulated samples).
    pub fn take_audio(&mut self) -> Vec<f32> {
        core::mem::take(&mut self.audio_buffer)
    }

    /// Get the audio buffer without draining.
    #[must_use]
    pub fn audio_buffer(&self) -> &[f32] {
        &self.audio_buffer
    }

    /// Get audio samples (alias for `audio_buffer`).
    #[must_use]
    pub fn audio_samples(&self) -> &[f32] {
        &self.audio_buffer
    }

    /// Clear the audio sample buffer.
    pub fn clear_audio_samples(&mut self) {
        self.audio_buffer.clear();
    }

    /// Set controller 1 state from button byte.
    pub fn set_controller_1(&mut self, buttons: u8) {
        self.bus.controller1 = ControllerState { buttons };
    }

    /// Set controller 2 state from button byte.
    pub fn set_controller_2(&mut self, buttons: u8) {
        self.bus.controller2 = ControllerState { buttons };
    }

    /// Set controller 1 state.
    pub fn set_controller1(&mut self, state: ControllerState) {
        self.bus.controller1 = state;
    }

    /// Set controller 2 state.
    pub fn set_controller2(&mut self, state: ControllerState) {
        self.bus.controller2 = state;
    }

    /// Get controller 1 state.
    #[must_use]
    pub fn controller1(&self) -> ControllerState {
        self.bus.controller1
    }

    /// Get controller 2 state.
    #[must_use]
    pub fn controller2(&self) -> ControllerState {
        self.bus.controller2
    }

    /// Get the total CPU cycles executed.
    #[must_use]
    pub fn total_cycles(&self) -> u64 {
        self.total_cycles
    }

    /// Get the frame count.
    #[must_use]
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Check if emulation is running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Pause emulation.
    pub fn pause(&mut self) {
        self.running = false;
    }

    /// Resume emulation.
    pub fn resume(&mut self) {
        self.running = true;
    }

    /// Get a reference to the CPU for debugging.
    #[must_use]
    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    /// Get a reference to the PPU for debugging.
    #[must_use]
    pub fn ppu(&self) -> &rustynes_ppu::Ppu {
        &self.bus.ppu
    }

    /// Get a reference to the APU for debugging.
    #[must_use]
    pub fn apu(&self) -> &rustynes_apu::Apu {
        &self.bus.apu
    }

    /// Get the total CPU cycles (alias for `total_cycles`).
    #[must_use]
    pub fn cycles(&self) -> u64 {
        self.total_cycles
    }

    /// Peek at memory without side effects.
    ///
    /// This is useful for debugging/display purposes where we don't want
    /// to trigger PPU register side effects or mapper state changes.
    #[must_use]
    pub fn peek_memory(&self, addr: u16) -> u8 {
        self.bus.peek(addr)
    }

    /// Get a reference to the bus for debugging.
    #[must_use]
    pub fn bus(&self) -> &NesBus {
        &self.bus
    }

    /// Get a mutable reference to the bus.
    pub fn bus_mut(&mut self) -> &mut NesBus {
        &mut self.bus
    }

    /// Get the mapper number.
    #[must_use]
    pub fn mapper_number(&self) -> u16 {
        self.bus.mapper.mapper_number()
    }

    /// Get the mapper name.
    #[must_use]
    pub fn mapper_name(&self) -> &'static str {
        self.bus.mapper.mapper_name()
    }

    /// Check if the ROM has battery-backed RAM.
    #[must_use]
    pub fn has_battery(&self) -> bool {
        self.bus.mapper.has_battery()
    }

    /// Get battery-backed RAM for saving.
    #[must_use]
    pub fn battery_ram(&self) -> Option<&[u8]> {
        self.bus.mapper.battery_ram()
    }

    /// Load battery-backed RAM.
    pub fn load_battery_ram(&mut self, data: &[u8]) {
        self.bus.mapper.set_battery_ram(data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_mappers::{Mirroring, Nrom, RomFormat, RomHeader};

    #[cfg(not(feature = "std"))]
    use alloc::{boxed::Box, vec, vec::Vec};

    fn create_test_console() -> Console {
        let rom = Rom {
            header: RomHeader {
                format: RomFormat::INes,
                mapper: 0,
                prg_rom_size: 2,
                chr_rom_size: 1,
                prg_ram_size: 0,
                chr_ram_size: 0,
                mirroring: Mirroring::Vertical,
                has_battery: false,
                has_trainer: false,
                tv_system: 0,
            },
            // Simple program: NOP loop at $8000
            prg_rom: {
                let mut prg = vec![0xEA; 32768]; // Fill with NOPs
                // Reset vector at $FFFC points to $8000
                prg[0x7FFC] = 0x00;
                prg[0x7FFD] = 0x80;
                prg
            },
            chr_rom: vec![0; 8192],
            trainer: None,
        };
        Console::with_mapper(Box::new(Nrom::new(&rom))).unwrap()
    }

    #[test]
    fn test_console_creation() {
        let console = create_test_console();
        assert_eq!(console.mapper_number(), 0);
        assert_eq!(console.mapper_name(), "NROM");
    }

    #[test]
    fn test_console_step() {
        let mut console = create_test_console();
        console.reset();

        let cycles = console.step();
        assert!(cycles > 0);
        assert!(console.total_cycles() > 0);
    }

    #[test]
    fn test_console_framebuffer() {
        let console = create_test_console();
        let fb = console.framebuffer();
        assert_eq!(fb.len(), 256 * 240 * 4);
    }

    #[test]
    fn test_console_pause_resume() {
        let mut console = create_test_console();
        assert!(console.is_running());

        console.pause();
        assert!(!console.is_running());

        console.resume();
        assert!(console.is_running());
    }

    #[test]
    fn test_controller_state() {
        let mut console = create_test_console();

        let state = ControllerState {
            buttons: ControllerState::A | ControllerState::START,
        };
        console.set_controller1(state);

        assert_eq!(console.controller1().buttons, 0x09);
    }

    #[test]
    fn test_console_reset() {
        let mut console = create_test_console();

        // Run some cycles
        for _ in 0..100 {
            console.step();
        }

        let cycles_before = console.total_cycles();
        assert!(cycles_before > 0);

        console.reset();
        assert_eq!(console.total_cycles(), 0);
    }
}

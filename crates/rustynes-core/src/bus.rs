//! NES memory bus implementation.
//!
//! This module implements the NES bus which routes CPU memory access to various
//! components: internal RAM, PPU registers, APU registers, controllers, and cartridge.
//!
//! # Memory Map
//!
//! ```text
//! $0000-$07FF: 2KB internal RAM
//! $0800-$1FFF: Mirrors of $0000-$07FF (3×)
//! $2000-$2007: PPU registers
//! $2008-$3FFF: Mirrors of $2000-$2007
//! $4000-$4013: APU and I/O registers
//! $4014:       OAM DMA register
//! $4015:       APU status register
//! $4016:       Controller 1 data / strobe
//! $4017:       Controller 2 data / APU frame counter
//! $4018-$401F: APU/I/O test mode registers (normally disabled)
//! $4020-$FFFF: Cartridge space (mapper-controlled)
//! ```

use crate::input::Controller;
use rustynes_apu::Apu;
use rustynes_cpu::CpuBus;
use rustynes_mappers::Mapper;
use rustynes_ppu::{Mirroring as PpuMirroring, Ppu};

/// NES memory bus connecting CPU to all components
#[allow(clippy::struct_excessive_bools)]
pub struct Bus {
    /// 2KB internal RAM
    ram: [u8; 0x800],

    /// 8KB PRG-RAM / battery-backed SRAM ($6000-$7FFF)
    /// Used by many test ROMs to report results
    prg_ram: [u8; 0x2000],

    /// Picture Processing Unit
    pub ppu: Ppu,

    /// Audio Processing Unit
    pub apu: Apu,

    /// Cartridge mapper
    pub mapper: Box<dyn Mapper>,

    /// Controller 1
    pub controller1: Controller,

    /// Controller 2
    pub controller2: Controller,

    /// OAM DMA state
    dma_page: u8,
    dma_addr: u8,
    dma_data: u8,
    dma_dummy_cycles: u8, // Number of dummy cycles remaining (1 or 2 depending on alignment)
    dma_transfer: bool,
    dma_write: bool,

    /// CPU cycle counter for odd/even tracking (for DMA timing)
    cpu_cycles: u64,

    /// NMI pending from PPU (set during `on_cpu_cycle`, cleared by Console)
    nmi_pending: bool,

    /// Frame complete flag (set during `on_cpu_cycle`, cleared by Console)
    frame_complete: bool,

    /// DMC DMA stall cycles pending (set during `on_cpu_cycle`, cleared by Console)
    /// When DMC performs DMA, it steals CPU cycles (typically 3-4 cycles)
    dmc_stall_cycles: u8,

    /// Last value on data bus (for open bus behavior)
    /// Unmapped addresses return this value; controllers mix it with their data
    last_bus_value: u8,
}

impl Bus {
    /// Create a new bus with the given mapper
    ///
    /// # Arguments
    ///
    /// * `mapper` - Cartridge mapper implementation
    ///
    /// # Returns
    ///
    /// New Bus instance with all components initialized (APU defaults to 48000 Hz)
    #[must_use]
    pub fn new(mapper: Box<dyn Mapper>) -> Self {
        Self::with_sample_rate(mapper, 48000)
    }

    /// Create a new bus with the given mapper and custom audio sample rate
    ///
    /// # Arguments
    ///
    /// * `mapper` - Cartridge mapper implementation
    /// * `sample_rate` - Audio output sample rate (e.g., 44100 or 48000 Hz)
    ///
    /// # Returns
    ///
    /// New Bus instance with all components initialized
    #[must_use]
    pub fn with_sample_rate(mapper: Box<dyn Mapper>, sample_rate: u32) -> Self {
        let mirroring = mapper.mirroring();

        // Convert mapper Mirroring to PPU Mirroring
        let ppu_mirroring = match mirroring {
            rustynes_mappers::Mirroring::Horizontal => PpuMirroring::Horizontal,
            rustynes_mappers::Mirroring::Vertical => PpuMirroring::Vertical,
            rustynes_mappers::Mirroring::SingleScreenLower => PpuMirroring::SingleScreenLower,
            rustynes_mappers::Mirroring::SingleScreenUpper => PpuMirroring::SingleScreenUpper,
            rustynes_mappers::Mirroring::FourScreen => PpuMirroring::FourScreen,
        };

        Self {
            ram: [0; 0x800],
            prg_ram: [0; 0x2000], // Initialize PRG-RAM to 0 (matches internal RAM)
            ppu: Ppu::new(ppu_mirroring),
            apu: Apu::with_sample_rate(sample_rate),
            mapper,
            controller1: Controller::new(),
            controller2: Controller::new(),
            dma_page: 0,
            dma_addr: 0,
            dma_data: 0,
            dma_dummy_cycles: 0,
            dma_transfer: false,
            dma_write: false,
            cpu_cycles: 0,
            nmi_pending: false,
            frame_complete: false,
            dmc_stall_cycles: 0,
            last_bus_value: 0,
        }
    }

    /// Check if DMA transfer is active
    ///
    /// # Returns
    ///
    /// true if OAM DMA is in progress, false otherwise
    #[must_use]
    pub fn dma_active(&self) -> bool {
        self.dma_transfer
    }

    /// Execute one cycle of DMA transfer
    ///
    /// OAM DMA takes 513 or 514 CPU cycles:
    /// - 513 cycles: Started on even CPU cycle (1 dummy + 512 transfer cycles)
    /// - 514 cycles: Started on odd CPU cycle (2 dummy cycles for alignment + 512 transfer cycles)
    /// - Transfer: 256 alternating read/write cycles (512 total)
    ///
    /// # Returns
    ///
    /// true if DMA is complete, false if still in progress
    pub fn tick_dma(&mut self) -> bool {
        if !self.dma_transfer {
            return true;
        }

        // Dummy cycle(s) for alignment
        // If started on odd cycle, we need 2 dummy cycles
        // If started on even cycle, we need 1 dummy cycle
        if self.dma_dummy_cycles > 0 {
            self.dma_dummy_cycles -= 1;
            self.cpu_cycles += 1;
            return false;
        }

        if self.dma_write {
            // Write cycle
            self.ppu.write_register(0x2004, self.dma_data, |_, _| {});
            self.dma_write = false;
            self.cpu_cycles += 1;

            // Advance to next byte
            self.dma_addr = self.dma_addr.wrapping_add(1);

            // Check if transfer is complete (256 bytes transferred)
            if self.dma_addr == 0 {
                self.dma_transfer = false;
                true
            } else {
                false
            }
        } else {
            // Read cycle
            let addr = u16::from(self.dma_page) << 8 | u16::from(self.dma_addr);
            self.dma_data = self.read_for_dma(addr);
            self.dma_write = true;
            self.cpu_cycles += 1;
            false
        }
    }

    /// Read memory for DMA (doesn't trigger side effects)
    ///
    /// This is separate from regular `read()` to avoid triggering PPU/APU side effects
    /// during DMA operations.
    fn read_for_dma(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x4020..=0xFFFF => self.mapper.read_prg(addr),
            _ => 0, // DMA from PPU/APU registers returns open bus
        }
    }

    /// Initiate OAM DMA transfer
    ///
    /// # Arguments
    ///
    /// * `page` - High byte of source address (e.g., $02 for $0200-$02FF)
    ///
    /// # Timing
    ///
    /// OAM DMA takes 513 or 514 CPU cycles:
    /// - 513 cycles if started on an even CPU cycle (1 dummy + 512 transfer)
    /// - 514 cycles if started on an odd CPU cycle (2 dummy + 512 transfer)
    ///
    /// The extra cycle on odd alignment ensures reads happen on even cycles.
    fn start_oam_dma(&mut self, page: u8) {
        self.dma_page = page;
        self.dma_addr = 0;
        self.dma_transfer = true;
        self.dma_write = false;

        // Determine number of dummy cycles based on CPU cycle parity
        // Odd cycle: need 2 dummy cycles for alignment (514 total)
        // Even cycle: need 1 dummy cycle (513 total)
        self.dma_dummy_cycles = if (self.cpu_cycles % 2) == 1 { 2 } else { 1 };
    }

    /// Increment CPU cycle counter (called from console after each CPU instruction)
    ///
    /// This tracks odd/even cycles for DMA timing precision.
    ///
    /// # Arguments
    ///
    /// * `cycles` - Number of CPU cycles to add
    pub fn add_cpu_cycles(&mut self, cycles: u8) {
        self.cpu_cycles += u64::from(cycles);
    }

    /// Reset the bus and all components
    pub fn reset(&mut self) {
        self.ram = [0; 0x800];
        self.prg_ram.fill(0); // Initialize PRG-RAM to 0
        self.ppu.reset();
        self.apu.reset();
        self.controller1.reset();
        self.controller2.reset();
        self.dma_transfer = false;
        self.cpu_cycles = 0;
        self.nmi_pending = false;
        self.frame_complete = false;
        self.dmc_stall_cycles = 0;
        self.last_bus_value = 0;
    }

    /// Clock the mapper (for IRQ timing)
    ///
    /// # Arguments
    ///
    /// * `cycles` - Number of CPU cycles elapsed
    pub fn clock_mapper(&mut self, cycles: u8) {
        self.mapper.clock(cycles);
    }

    /// Notify mapper of PPU A12 rising edge (for scanline IRQ)
    pub fn ppu_a12_edge(&mut self) {
        self.mapper.ppu_a12_edge();
    }

    /// Check if mapper IRQ is pending
    ///
    /// # Returns
    ///
    /// true if mapper is asserting IRQ line
    #[must_use]
    pub fn mapper_irq_pending(&self) -> bool {
        self.mapper.irq_pending()
    }

    /// Clear mapper IRQ
    pub fn clear_mapper_irq(&mut self) {
        self.mapper.clear_irq();
    }

    /// Step PPU by one dot with CHR ROM access from mapper
    ///
    /// This method bridges the PPU and mapper, allowing the PPU to fetch
    /// pattern table data (CHR ROM) during background and sprite rendering.
    ///
    /// # Returns
    ///
    /// Tuple of (`frame_complete`, `nmi`):
    /// - `frame_complete`: true if a complete frame was just rendered
    /// - `nmi`: true if NMI should be triggered (`VBlank` start with NMI enabled)
    #[inline]
    pub fn step_ppu(&mut self) -> (bool, bool) {
        // Borrow mapper immutably for CHR reads while PPU is borrowed mutably
        // This works because they are separate fields of the struct
        let mapper = &*self.mapper;
        self.ppu.step_with_chr(|addr| mapper.read_chr(addr))
    }

    /// Take and clear the pending NMI flag.
    ///
    /// This is set by `on_cpu_cycle()` when PPU asserts NMI.
    /// Console should call this after `cpu.tick()` to check if NMI needs handling.
    ///
    /// # Returns
    ///
    /// true if NMI was pending, false otherwise
    #[inline]
    pub fn take_nmi(&mut self) -> bool {
        let pending = self.nmi_pending;
        self.nmi_pending = false;
        pending
    }

    /// Take and clear the frame complete flag.
    ///
    /// This is set by `on_cpu_cycle()` when PPU completes a frame.
    /// Console should call this to detect when a new frame is ready.
    ///
    /// # Returns
    ///
    /// true if a frame was completed, false otherwise
    #[inline]
    pub fn take_frame_complete(&mut self) -> bool {
        let complete = self.frame_complete;
        self.frame_complete = false;
        complete
    }

    /// Take and clear pending DMC DMA stall cycles.
    ///
    /// This is set by `on_cpu_cycle()` when DMC performs DMA.
    /// Console should call this after `cpu.tick()` to check if CPU needs to stall.
    ///
    /// During DMC DMA stalls:
    /// - CPU is halted (no instruction execution)
    /// - PPU continues running (3 dots per stall cycle)
    /// - APU continues running (1 cycle per stall cycle)
    /// - Mapper is clocked (1 cycle per stall cycle)
    ///
    /// # Returns
    ///
    /// Number of stall cycles (0-4, typically 3 when DMA occurs)
    #[inline]
    pub fn take_dmc_stall_cycles(&mut self) -> u8 {
        let stalls = self.dmc_stall_cycles;
        self.dmc_stall_cycles = 0;
        stalls
    }
}

impl CpuBus for Bus {
    fn read(&mut self, addr: u16) -> u8 {
        let value = match addr {
            // 2KB internal RAM, mirrored 4 times
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],

            // PPU registers, mirrored every 8 bytes
            0x2000..=0x3FFF => {
                let ppu_addr = 0x2000 + (addr & 0x0007);
                // Pass closure to read CHR from mapper
                let mapper = &*self.mapper;
                self.ppu
                    .read_register(ppu_addr, |chr_addr| mapper.read_chr(chr_addr))
            }

            // APU and I/O registers
            0x4000..=0x4015 => self.apu.read_register(addr),

            // Controller 1 - bits 0-4 from controller, bits 5-7 from open bus
            0x4016 => self.controller1.read() | (self.last_bus_value & 0xE0),

            // Controller 2 (note: $4017 write goes to APU, read goes to controller)
            // bits 0-4 from controller, bits 5-7 from open bus
            0x4017 => self.controller2.read() | (self.last_bus_value & 0xE0),

            // Unmapped APU/IO test registers ($4018-$401F) and expansion ROM area ($4020-$5FFF)
            // Both return open bus value
            0x4018..=0x5FFF => self.last_bus_value,

            // PRG-RAM / battery-backed SRAM ($6000-$7FFF)
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],

            // Cartridge PRG-ROM (mapper-controlled)
            0x8000..=0xFFFF => self.mapper.read_prg(addr),
        };

        // Track last bus value for open bus behavior
        self.last_bus_value = value;
        value
    }

    fn write(&mut self, addr: u16, value: u8) {
        // Track last bus value for open bus behavior
        // Writes also put a value on the data bus
        self.last_bus_value = value;

        match addr {
            // 2KB internal RAM, mirrored 4 times
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize] = value,

            // PPU registers, mirrored every 8 bytes
            0x2000..=0x3FFF => {
                let ppu_addr = 0x2000 + (addr & 0x0007);
                // Pass closure to write CHR to mapper
                let mapper = &mut *self.mapper;
                self.ppu.write_register(ppu_addr, value, |chr_addr, val| {
                    mapper.write_chr(chr_addr, val);
                });
            }

            // APU registers
            0x4000..=0x4013 | 0x4015 | 0x4017 => self.apu.write_register(addr, value),

            // OAM DMA
            0x4014 => self.start_oam_dma(value),

            // Controller strobe (affects BOTH controllers)
            0x4016 => {
                self.controller1.write_strobe(value);
                self.controller2.write_strobe(value);
            }

            // PRG-RAM / battery-backed SRAM ($6000-$7FFF)
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize] = value,

            // Cartridge PRG-ROM / mapper registers ($8000-$FFFF)
            0x8000..=0xFFFF => self.mapper.write_prg(addr, value),

            // Unmapped regions ($4018-$401F, $4020-$5FFF)
            _ => {}
        }
    }

    /// Step PPU, APU, and mapper before each CPU memory access.
    ///
    /// This is the core of cycle-accurate emulation. For NTSC:
    /// - PPU runs at 3x CPU clock (3 PPU dots per CPU cycle)
    /// - APU runs at CPU clock
    /// - Mapper is clocked once per CPU cycle (for IRQ timing)
    ///
    /// Called BEFORE each CPU read/write to ensure PPU, APU, and mapper are in
    /// the correct state when the CPU observes memory. This is critical
    /// for accurate `VBlank` flag ($2002) timing and mapper IRQ precision.
    ///
    /// NMI, `frame_complete`, and DMC stall signals are captured and can be
    /// retrieved via `take_nmi()`, `take_frame_complete()`, and `take_dmc_stall_cycles()`.
    #[inline]
    fn on_cpu_cycle(&mut self) {
        // Step PPU 3 times (3 PPU dots per CPU cycle for NTSC)
        for _ in 0..3 {
            let (frame_complete, nmi) = self.step_ppu();
            if nmi {
                self.nmi_pending = true;
            }
            if frame_complete {
                self.frame_complete = true;
            }
        }

        // Step APU once (1 APU cycle per CPU cycle)
        // APU internally divides this further for its frame sequencer
        // APU returns DMC DMA stall cycles if DMC performed a DMA fetch
        let dmc_stalls = self.apu.step();
        if dmc_stalls > 0 {
            // Accumulate stall cycles (can happen multiple times if CPU stalls span instructions)
            self.dmc_stall_cycles = self.dmc_stall_cycles.saturating_add(dmc_stalls);
        }

        // Clock mapper once per CPU cycle (for cycle-based IRQ timing)
        // This is critical for VRC mappers and other cycle-counting mappers
        self.mapper.clock(1);

        // Track CPU cycles for DMA alignment
        self.cpu_cycles += 1;
    }

    fn peek(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xFFFF => self.mapper.read_prg(addr),
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_mappers::{Nrom, Rom};

    fn create_test_bus() -> Bus {
        // Create a minimal ROM for testing
        let mut rom_data = vec![0; 16 + 16384]; // Header + 16KB PRG
        // iNES header
        rom_data[0..4].copy_from_slice(b"NES\x1A");
        rom_data[4] = 1; // 1 PRG bank
        rom_data[5] = 0; // 0 CHR banks (use CHR-RAM)
        rom_data[6] = 0; // Horizontal mirroring, NROM

        let rom = Rom::load(&rom_data).unwrap();
        let mapper = Box::new(Nrom::new(&rom));

        Bus::new(mapper)
    }

    #[test]
    fn test_ram_read_write() {
        let mut bus = create_test_bus();

        bus.write(0x0000, 0x42);
        assert_eq!(bus.read(0x0000), 0x42);

        // Test mirroring
        assert_eq!(bus.read(0x0800), 0x42);
        assert_eq!(bus.read(0x1000), 0x42);
        assert_eq!(bus.read(0x1800), 0x42);
    }

    #[test]
    fn test_ppu_register_mirroring() {
        let mut bus = create_test_bus();

        // Write to PPUCTRL
        bus.write(0x2000, 0x80);

        // Should be mirrored every 8 bytes
        bus.write(0x2008, 0x00);
        bus.write(0x3000, 0x00);
        bus.write(0x3FF8, 0x00);
    }

    #[test]
    fn test_controller_strobe() {
        let mut bus = create_test_bus();

        // Set button on controller 1
        bus.controller1.set_button(crate::input::Button::A, true);

        // Strobe
        bus.write(0x4016, 0x01);
        bus.write(0x4016, 0x00);

        // Read should return A button state
        let value = bus.read(0x4016);
        assert_eq!(value & 0x01, 1);
    }

    #[test]
    #[allow(clippy::cast_possible_truncation)]
    fn test_oam_dma() {
        let mut bus = create_test_bus();

        // Write test data to RAM
        for i in 0..256_u16 {
            bus.write(0x0200 + i, i as u8);
        }

        // Initiate DMA
        bus.write(0x4014, 0x02);

        assert!(bus.dma_active());

        // Run DMA to completion
        let mut cycles = 0;
        while !bus.tick_dma() {
            cycles += 1;
            assert!(cycles <= 600, "DMA didn't complete");
        }

        assert!(!bus.dma_active());
        assert!((512..=514).contains(&cycles));
    }

    #[test]
    fn test_reset() {
        let mut bus = create_test_bus();

        // Write some data
        bus.write(0x0000, 0x42);
        bus.controller1.set_button(crate::input::Button::A, true);

        // Reset
        bus.reset();

        // RAM should be cleared
        assert_eq!(bus.read(0x0000), 0);

        // Controllers should be reset
        assert!(!bus.controller1.get_button(crate::input::Button::A));
    }

    #[test]
    fn test_open_bus_behavior() {
        let mut bus = create_test_bus();

        // Initially, open bus should be 0
        assert_eq!(bus.read(0x4018), 0);
        assert_eq!(bus.read(0x5000), 0);

        // After reading RAM, open bus should reflect that value
        bus.write(0x0000, 0x42);
        let _ = bus.read(0x0000);
        assert_eq!(bus.read(0x4018), 0x42); // Open bus now has 0x42
        assert_eq!(bus.read(0x5000), 0x42); // Still 0x42

        // After writing, open bus should reflect the written value
        bus.write(0x0001, 0xAB);
        assert_eq!(bus.read(0x4018), 0xAB); // Open bus updated by write
    }

    #[test]
    fn test_controller_open_bus_bits() {
        let mut bus = create_test_bus();

        // Strobe controllers first
        bus.write(0x4016, 0x01);
        bus.write(0x4016, 0x00);

        // Now set up open bus with value 0xE0 (bits 5-7 set)
        // Note: writes update open bus too, so we must read to set it after strobe
        bus.write(0x0000, 0xE0);
        let _ = bus.read(0x0000); // Open bus now 0xE0

        // Controller read should mix open bus bits 5-7 with controller data bits 0-4
        // With no buttons pressed, controller returns 0x00 for bit 0
        // Result bits 5-7 should come from open bus (0xE0)
        let value = bus.read(0x4016);
        assert_eq!(value & 0xE0, 0xE0); // Upper 3 bits from open bus

        // Set open bus again before reading controller 2
        // (the previous controller read updated open bus to the read value)
        let _ = bus.read(0x0000); // Open bus now 0xE0 again
        let value2 = bus.read(0x4017);
        assert_eq!(value2 & 0xE0, 0xE0);
    }
}

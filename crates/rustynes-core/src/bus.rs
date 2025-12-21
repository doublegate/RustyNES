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
use rustynes_cpu::Bus as CpuBus;
use rustynes_mappers::Mapper;
use rustynes_ppu::{Mirroring as PpuMirroring, Ppu};

/// NES memory bus connecting CPU to all components
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
}

impl Bus {
    /// Create a new bus with the given mapper
    ///
    /// # Arguments
    ///
    /// * `mapper` - Cartridge mapper implementation
    /// * `mirroring` - Initial nametable mirroring mode
    ///
    /// # Returns
    ///
    /// New Bus instance with all components initialized
    #[must_use]
    pub fn new(mapper: Box<dyn Mapper>) -> Self {
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
            prg_ram: [0xFF; 0x2000], // Initialize PRG-RAM to 0xFF (test ROMs expect this)
            ppu: Ppu::new(ppu_mirroring),
            apu: Apu::new(),
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
            self.ppu.write_register(0x2004, self.dma_data);
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
        self.prg_ram.fill(0xFF); // Initialize PRG-RAM to 0xFF (many test ROMs expect this)
        self.ppu.reset();
        self.apu.reset();
        self.controller1.reset();
        self.controller2.reset();
        self.dma_transfer = false;
        self.cpu_cycles = 0;
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
    pub fn step_ppu(&mut self) -> (bool, bool) {
        // Borrow mapper immutably for CHR reads while PPU is borrowed mutably
        // This works because they are separate fields of the struct
        let mapper = &*self.mapper;
        self.ppu.step_with_chr(|addr| mapper.read_chr(addr))
    }
}

impl CpuBus for Bus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // 2KB internal RAM, mirrored 4 times
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],

            // PPU registers, mirrored every 8 bytes
            0x2000..=0x3FFF => {
                let ppu_addr = 0x2000 + (addr & 0x0007);
                self.ppu.read_register(ppu_addr)
            }

            // APU and I/O registers
            0x4000..=0x4015 => self.apu.read_register(addr),

            // Controller 1
            0x4016 => self.controller1.read(),

            // Controller 2 (note: $4017 write goes to APU, read goes to controller)
            0x4017 => self.controller2.read(),

            // PRG-RAM / battery-backed SRAM ($6000-$7FFF)
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],

            // Cartridge PRG-ROM (mapper-controlled)
            0x8000..=0xFFFF => self.mapper.read_prg(addr),

            // Unmapped regions ($4018-$401F, $4020-$5FFF)
            _ => 0, // Open bus
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // 2KB internal RAM, mirrored 4 times
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize] = value,

            // PPU registers, mirrored every 8 bytes
            0x2000..=0x3FFF => {
                let ppu_addr = 0x2000 + (addr & 0x0007);
                self.ppu.write_register(ppu_addr, value);
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
}

//! NES System Bus Implementation.
//!
//! The bus connects the CPU to all other components:
//! - PPU registers ($2000-$2007, mirrored every 8 bytes to $3FFF)
//! - APU/IO registers ($4000-$4017)
//! - Cartridge space ($4020-$FFFF)
//! - Internal RAM ($0000-$07FF, mirrored to $1FFF)

use rustynes_apu::Apu;
use rustynes_cpu::Bus;
use rustynes_mappers::{Mapper, Mirroring};
use rustynes_ppu::Ppu;

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;

/// Controller input state.
#[derive(Debug, Clone, Copy, Default)]
pub struct ControllerState {
    /// Button states: A, B, Select, Start, Up, Down, Left, Right
    pub buttons: u8,
}

impl ControllerState {
    /// A button mask.
    pub const A: u8 = 0x01;
    /// B button mask.
    pub const B: u8 = 0x02;
    /// Select button mask.
    pub const SELECT: u8 = 0x04;
    /// Start button mask.
    pub const START: u8 = 0x08;
    /// Up button mask.
    pub const UP: u8 = 0x10;
    /// Down button mask.
    pub const DOWN: u8 = 0x20;
    /// Left button mask.
    pub const LEFT: u8 = 0x40;
    /// Right button mask.
    pub const RIGHT: u8 = 0x80;
}

/// PPU memory bus adapter for CHR and CIRAM access.
///
/// This wrapper allows the PPU to access CHR memory through the mapper
/// and nametable memory (CIRAM) with proper mirroring.
///
/// NES PPU memory map:
/// - $0000-$1FFF: Pattern tables (CHR ROM/RAM, handled by mapper)
/// - $2000-$3EFF: Nametables (2KB CIRAM with mirroring)
/// - $3F00-$3FFF: Palette RAM (handled internally by PPU)
pub struct PpuMemory<'a> {
    mapper: &'a mut dyn Mapper,
    ciram: &'a mut [u8; 2048],
    mirroring: Mirroring,
}

impl PpuMemory<'_> {
    /// Calculate the CIRAM address with nametable mirroring applied.
    ///
    /// The NES has 2KB of internal VRAM (CIRAM) for nametables, but the
    /// nametable address space is 4KB ($2000-$2FFF). The mirroring mode
    /// determines how the 4 logical nametables map to the 2 physical ones.
    fn ciram_addr(&self, addr: u16) -> usize {
        // Mask to get offset within nametable region ($0000-$0FFF)
        let addr = addr & 0x0FFF;

        match self.mirroring {
            Mirroring::Horizontal => {
                // Horizontal mirroring: $2000/$2400 share, $2800/$2C00 share
                // Use bit 11 to select nametable (0 or 1)
                let nametable = (addr >> 11) & 1;
                let offset = addr & 0x03FF;
                (nametable * 0x400 + offset) as usize
            }
            Mirroring::Vertical => {
                // Vertical mirroring: $2000/$2800 share, $2400/$2C00 share
                // Use bit 10 to select nametable (0 or 1)
                let nametable = (addr >> 10) & 1;
                let offset = addr & 0x03FF;
                (nametable * 0x400 + offset) as usize
            }
            Mirroring::SingleScreenLower => {
                // All nametables map to first 1KB
                (addr & 0x03FF) as usize
            }
            Mirroring::SingleScreenUpper => {
                // All nametables map to second 1KB
                ((addr & 0x03FF) + 0x400) as usize
            }
            Mirroring::FourScreen => {
                // Four-screen uses mapper-provided extra VRAM
                // For now, treat as vertical mirroring (TODO: proper 4-screen support)
                let nametable = (addr >> 10) & 1;
                let offset = addr & 0x03FF;
                (nametable * 0x400 + offset) as usize
            }
        }
    }
}

impl rustynes_ppu::PpuBus for PpuMemory<'_> {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // Pattern tables: CHR ROM/RAM handled by mapper
            0x0000..=0x1FFF => self.mapper.read_chr(addr),
            // Nametables: internal CIRAM with mirroring
            0x2000..=0x3EFF => {
                let ciram_addr = self.ciram_addr(addr);
                self.ciram[ciram_addr]
            }
            // Palette RAM is handled internally by PPU, but we may get
            // reads here for the VRAM buffer behavior at $3F00-$3FFF
            // Return underlying nametable data (mirrors $2F00-$2FFF)
            0x3F00..=0x3FFF => {
                let ciram_addr = self.ciram_addr(addr - 0x1000);
                self.ciram[ciram_addr]
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // Pattern tables: CHR RAM writes (if mapper supports it)
            0x0000..=0x1FFF => self.mapper.write_chr(addr, value),
            // Nametables: internal CIRAM with mirroring
            0x2000..=0x3EFF => {
                let ciram_addr = self.ciram_addr(addr);
                self.ciram[ciram_addr] = value;
            }
            // Palette writes go to PPU's internal palette RAM, not CIRAM
            0x3F00..=0x3FFF => {
                // This shouldn't normally happen as PPU handles palette writes internally
            }
            _ => {}
        }
    }
}

/// NES system bus connecting all components.
pub struct NesBus {
    /// Internal RAM (2KB, mirrored 4 times).
    pub ram: [u8; 2048],
    /// PPU internal VRAM (CIRAM, 2KB) for nametables.
    pub ciram: [u8; 2048],
    /// PPU (Picture Processing Unit).
    pub ppu: Ppu,
    /// APU (Audio Processing Unit).
    pub apu: Apu,
    /// Cartridge mapper.
    pub mapper: Box<dyn Mapper>,
    /// Controller 1 state.
    pub controller1: ControllerState,
    /// Controller 2 state.
    pub controller2: ControllerState,
    /// Controller 1 shift register.
    controller1_shift: u8,
    /// Controller 2 shift register.
    controller2_shift: u8,
    /// Controller strobe latch.
    controller_strobe: bool,
    /// OAM DMA page.
    oam_dma_page: Option<u8>,
    /// CPU cycle counter for DMA timing.
    cpu_cycles: u64,
    /// DMC DMA stall cycles.
    dmc_stall_cycles: u8,
    /// Last value on the data bus (for open bus behavior).
    last_bus_value: u8,
    /// NMI pending from PPU.
    nmi_pending: bool,
    /// IRQ pending from mapper/APU.
    irq_pending: bool,
    /// Sample accumulator for downsampling.
    sample_count: u32,
    /// Sample sum for averaging.
    sample_sum: f32,
}

impl NesBus {
    /// CPU cycles per audio sample (at 44100 Hz).
    const CYCLES_PER_SAMPLE: u32 = 40; // ~1789773 / 44100

    /// Create a new NES bus with the given mapper.
    pub fn new(mapper: Box<dyn Mapper>) -> Self {
        Self {
            ram: [0; 2048],
            ciram: [0; 2048],
            ppu: Ppu::new(),
            apu: Apu::new(),
            mapper,
            controller1: ControllerState::default(),
            controller2: ControllerState::default(),
            controller1_shift: 0,
            controller2_shift: 0,
            controller_strobe: false,
            oam_dma_page: None,
            cpu_cycles: 0,
            dmc_stall_cycles: 0,
            last_bus_value: 0,
            nmi_pending: false,
            irq_pending: false,
            sample_count: 0,
            sample_sum: 0.0,
        }
    }

    /// Reset the bus and all components.
    pub fn reset(&mut self) {
        self.ram.fill(0);
        self.ciram.fill(0);
        self.ppu.reset();
        self.apu.reset();
        self.mapper.reset();
        self.controller1_shift = 0;
        self.controller2_shift = 0;
        self.controller_strobe = false;
        self.oam_dma_page = None;
        self.cpu_cycles = 0;
        self.dmc_stall_cycles = 0;
        self.last_bus_value = 0;
        self.nmi_pending = false;
        self.irq_pending = false;
        self.sample_count = 0;
        self.sample_sum = 0.0;
    }

    /// Check if OAM DMA is pending.
    #[must_use]
    pub fn oam_dma_pending(&self) -> bool {
        self.oam_dma_page.is_some()
    }

    /// Execute OAM DMA transfer.
    ///
    /// Returns the number of CPU cycles consumed.
    pub fn execute_oam_dma(&mut self) -> u16 {
        if let Some(page) = self.oam_dma_page.take() {
            let base = u16::from(page) << 8;

            // Copy 256 bytes to OAM
            for i in 0..256u16 {
                let addr = base.wrapping_add(i);
                let data = self.cpu_read(addr);
                self.ppu.write_oam(data);
            }

            // DMA takes 513 or 514 cycles depending on CPU cycle parity
            let cycles = if self.cpu_cycles % 2 == 1 { 514 } else { 513 };
            self.cpu_cycles += u64::from(cycles);
            cycles
        } else {
            0
        }
    }

    /// Internal CPU read without updating bus state (for DMA).
    fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x8000..=0xFFFF => self.mapper.read_prg(addr),
            _ => 0,
        }
    }

    /// Step the PPU by 3 dots (one CPU cycle worth).
    ///
    /// Returns true if NMI should be triggered.
    pub fn step_ppu(&mut self) -> bool {
        let mut nmi = false;

        for _ in 0..3 {
            // Create a temporary PPU memory bus for CHR and CIRAM access
            let mirroring = self.mapper.mirroring();
            let mut ppu_mem = PpuMemory {
                mapper: &mut *self.mapper,
                ciram: &mut self.ciram,
                mirroring,
            };
            if self.ppu.step(&mut ppu_mem) {
                nmi = true;
            }
        }

        // Clock the mapper for each CPU cycle
        self.mapper.clock(1);

        if nmi {
            self.nmi_pending = true;
        }

        nmi
    }

    /// Step the APU by one CPU cycle.
    ///
    /// Returns audio sample if available.
    pub fn step_apu(&mut self) -> Option<f32> {
        self.apu.clock();

        // Handle DMC sample fetch
        if self.apu.dmc_needs_sample() {
            let addr = self.apu.dmc_sample_addr();
            let sample = self.mapper.read_prg(addr);
            self.apu.dmc_fill_sample(sample);
            // DMC DMA stalls CPU for 4 cycles
            self.dmc_stall_cycles = 4;
        }

        // Accumulate samples for downsampling
        self.sample_sum += self.apu.output();
        self.sample_count += 1;

        if self.sample_count >= Self::CYCLES_PER_SAMPLE {
            #[allow(clippy::cast_precision_loss)]
            let sample = self.sample_sum / self.sample_count as f32;
            self.sample_count = 0;
            self.sample_sum = 0.0;
            Some(sample)
        } else {
            None
        }
    }

    /// Check if NMI is pending.
    #[must_use]
    pub fn nmi_pending(&self) -> bool {
        self.nmi_pending
    }

    /// Acknowledge NMI.
    pub fn acknowledge_nmi(&mut self) {
        self.nmi_pending = false;
    }

    /// Check if IRQ is pending.
    #[must_use]
    pub fn irq_pending(&self) -> bool {
        self.irq_pending || self.mapper.irq_pending() || self.apu.irq_pending()
    }

    /// Acknowledge mapper IRQ.
    pub fn acknowledge_mapper_irq(&mut self) {
        self.mapper.irq_acknowledge();
    }

    /// Get the current CPU cycle count.
    #[must_use]
    pub fn cpu_cycles(&self) -> u64 {
        self.cpu_cycles
    }

    /// Increment CPU cycle count.
    pub fn add_cpu_cycles(&mut self, cycles: u8) {
        self.cpu_cycles += u64::from(cycles);
    }

    /// Read controller register.
    fn read_controller(&mut self, port: u8) -> u8 {
        let shift = if port == 0 {
            &mut self.controller1_shift
        } else {
            &mut self.controller2_shift
        };

        // Open bus behavior: bits 5-7 come from last bus value
        let open_bus = self.last_bus_value & 0xE0;

        // Read bit 0 from shift register
        let data = (*shift & 1) | open_bus;
        *shift >>= 1;
        *shift |= 0x80; // Shift in 1s after all buttons read

        data
    }

    /// Write controller strobe.
    fn write_controller_strobe(&mut self, val: u8) {
        let new_strobe = val & 1 != 0;

        // On falling edge (strobe 1->0), latch controller state
        if self.controller_strobe && !new_strobe {
            self.controller1_shift = self.controller1.buttons;
            self.controller2_shift = self.controller2.buttons;
        }

        self.controller_strobe = new_strobe;

        // While strobe is high, continuously reload
        if self.controller_strobe {
            self.controller1_shift = self.controller1.buttons;
            self.controller2_shift = self.controller2.buttons;
        }
    }

    /// Check if DMC stall is active.
    #[must_use]
    pub fn dmc_stall_active(&self) -> bool {
        self.dmc_stall_cycles > 0
    }

    /// Decrement DMC stall counter.
    pub fn decrement_dmc_stall(&mut self) {
        if self.dmc_stall_cycles > 0 {
            self.dmc_stall_cycles -= 1;
        }
    }

    /// Peek at memory without side effects.
    ///
    /// This is useful for debugging/display purposes where we don't want
    /// to trigger PPU register side effects or mapper state changes.
    #[must_use]
    pub fn peek(&self, addr: u16) -> u8 {
        match addr {
            // Internal RAM (mirrored every 2KB)
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],

            // PPU registers - return last bus value to avoid side effects
            0x2000..=0x3FFF => self.last_bus_value,

            // APU and I/O registers
            0x4000..=0x4017 => match addr {
                0x4015 => self.apu.peek_status(),
                0x4016 | 0x4017 => self.last_bus_value,
                _ => self.last_bus_value,
            },

            // APU test mode
            0x4018..=0x401F => self.last_bus_value,

            // Cartridge space
            0x4020..=0xFFFF => self.mapper.read_prg(addr),
        }
    }
}

/// CPU bus implementation.
impl Bus for NesBus {
    fn read(&mut self, addr: u16) -> u8 {
        let value = match addr {
            // Internal RAM (mirrored every 2KB)
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],

            // PPU registers (mirrored every 8 bytes)
            0x2000..=0x3FFF => {
                let mirroring = self.mapper.mirroring();
                let mut ppu_mem = PpuMemory {
                    mapper: &mut *self.mapper,
                    ciram: &mut self.ciram,
                    mirroring,
                };
                self.ppu.read_register(addr, &mut ppu_mem)
            }

            // APU and I/O registers
            0x4000..=0x4017 => match addr {
                0x4015 => self.apu.read_status(),
                0x4016 => self.read_controller(0),
                0x4017 => self.read_controller(1),
                _ => self.last_bus_value, // Write-only registers
            },

            // APU test mode (normally disabled)
            0x4018..=0x401F => self.last_bus_value,

            // Cartridge space
            0x4020..=0xFFFF => self.mapper.read_prg(addr),
        };

        self.last_bus_value = value;
        value
    }

    fn write(&mut self, addr: u16, val: u8) {
        self.last_bus_value = val;

        match addr {
            // Internal RAM (mirrored every 2KB)
            0x0000..=0x1FFF => {
                self.ram[(addr & 0x07FF) as usize] = val;
            }

            // PPU registers (mirrored every 8 bytes)
            0x2000..=0x3FFF => {
                let mirroring = self.mapper.mirroring();
                let mut ppu_mem = PpuMemory {
                    mapper: &mut *self.mapper,
                    ciram: &mut self.ciram,
                    mirroring,
                };
                self.ppu.write_register(addr, val, &mut ppu_mem);
            }

            // APU and I/O registers
            0x4000..=0x4017 => match addr {
                0x4000..=0x4013 | 0x4015 | 0x4017 => {
                    self.apu.write(addr, val);
                }
                0x4014 => {
                    // OAM DMA
                    self.oam_dma_page = Some(val);
                }
                0x4016 => {
                    self.write_controller_strobe(val);
                }
                _ => {}
            },

            // APU test mode (normally disabled)
            0x4018..=0x401F => {}

            // Cartridge space
            0x4020..=0xFFFF => {
                self.mapper.write_prg(addr, val);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_mappers::{Mirroring, Nrom, Rom, RomFormat, RomHeader};

    #[cfg(not(feature = "std"))]
    use alloc::{boxed::Box, vec, vec::Vec};

    fn create_test_bus() -> NesBus {
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
            prg_rom: vec![0; 32768],
            chr_rom: vec![0; 8192],
            trainer: None,
        };
        NesBus::new(Box::new(Nrom::new(&rom)))
    }

    #[test]
    fn test_ram_mirroring() {
        let mut bus = create_test_bus();

        // Write to $0000
        Bus::write(&mut bus, 0x0000, 0x42);
        assert_eq!(Bus::read(&mut bus, 0x0000), 0x42);

        // Should mirror to $0800, $1000, $1800
        assert_eq!(Bus::read(&mut bus, 0x0800), 0x42);
        assert_eq!(Bus::read(&mut bus, 0x1000), 0x42);
        assert_eq!(Bus::read(&mut bus, 0x1800), 0x42);

        // Write to mirrored address
        Bus::write(&mut bus, 0x1234, 0xAB);
        assert_eq!(Bus::read(&mut bus, 0x0234), 0xAB); // $1234 & $07FF = $0234
    }

    #[test]
    fn test_controller_strobe() {
        let mut bus = create_test_bus();

        // Set controller 1 buttons
        bus.controller1.buttons = 0b1010_0101; // A, Select, Up, Right

        // Strobe high then low to latch
        Bus::write(&mut bus, 0x4016, 1);
        Bus::write(&mut bus, 0x4016, 0);

        // Read buttons one at a time (bit 0 of each read)
        assert_eq!(Bus::read(&mut bus, 0x4016) & 1, 1); // A
        assert_eq!(Bus::read(&mut bus, 0x4016) & 1, 0); // B
        assert_eq!(Bus::read(&mut bus, 0x4016) & 1, 1); // Select
        assert_eq!(Bus::read(&mut bus, 0x4016) & 1, 0); // Start
        assert_eq!(Bus::read(&mut bus, 0x4016) & 1, 0); // Up (bit 4)
        assert_eq!(Bus::read(&mut bus, 0x4016) & 1, 1); // Down
        assert_eq!(Bus::read(&mut bus, 0x4016) & 1, 0); // Left
        assert_eq!(Bus::read(&mut bus, 0x4016) & 1, 1); // Right
    }

    #[test]
    fn test_oam_dma() {
        let mut bus = create_test_bus();

        // Fill RAM page 2 ($0200-$02FF) with test data
        for i in 0..256 {
            Bus::write(&mut bus, 0x0200 + i, i as u8);
        }

        // Trigger OAM DMA from page 2
        Bus::write(&mut bus, 0x4014, 0x02);
        assert!(bus.oam_dma_pending());

        // Execute DMA
        let cycles = bus.execute_oam_dma();
        assert!(!bus.oam_dma_pending());
        assert!(cycles == 513 || cycles == 514);
    }

    #[test]
    fn test_open_bus_behavior() {
        let mut bus = create_test_bus();

        // Read from a location to set bus value
        Bus::write(&mut bus, 0x0000, 0xAB);
        let _ = Bus::read(&mut bus, 0x0000);

        // Last bus value should be updated
        assert_eq!(bus.last_bus_value, 0xAB);
    }

    #[test]
    fn test_peek_memory() {
        let mut bus = create_test_bus();

        // Write to RAM
        Bus::write(&mut bus, 0x0100, 0x42);

        // Peek should return the value without side effects
        assert_eq!(bus.peek(0x0100), 0x42);

        // Peek at mirrored address
        assert_eq!(bus.peek(0x0900), 0x42);
    }

    #[test]
    fn test_reset() {
        let mut bus = create_test_bus();
        bus.nmi_pending = true;

        bus.reset();

        assert_eq!(Bus::read(&mut bus, 0x0000), 0);
        assert_eq!(bus.cpu_cycles, 0);
        assert!(!bus.nmi_pending);
    }
}

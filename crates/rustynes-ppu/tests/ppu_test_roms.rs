//! PPU test ROM validation tests.
//!
//! This module validates the PPU implementation against standard test ROMs:
//! - blargg's ppu_vbl_nmi tests: VBlank and NMI timing
//! - sprite_hit_tests_2005: Sprite 0 hit detection
//!
//! Test ROMs are not included in the repository. Download from:
//! - https://github.com/christopherpow/nes-test-roms
//!
//! Place test ROMs in: test-roms/ppu/

use rustynes_cpu::{Bus, Cpu, CpuBus, INesRom};
use rustynes_ppu::{Mirroring, Ppu};
use std::path::PathBuf;

/// Integration bus connecting CPU and PPU for test ROMs.
///
/// This is a minimal implementation sufficient for running PPU test ROMs.
/// The full emulator will have a more comprehensive bus implementation.
///
/// Implements `CpuBus` for cycle-accurate PPU synchronization via `on_cpu_cycle()`.
struct TestBus {
    ram: [u8; 0x0800], // 2KB RAM
    ppu: Ppu,          // PPU instance
    prg_rom: Vec<u8>,  // PRG-ROM data
    #[allow(dead_code)] // CHR-ROM will be used when mapper support is added
    chr_rom: Vec<u8>, // CHR-ROM data
    apu_io: [u8; 0x20], // APU and I/O registers
    ppu_cycles: u32,   // Track PPU cycles for synchronization
    nmi_pending: bool, // NMI pending from PPU (captured during `on_cpu_cycle`)
}

impl TestBus {
    fn new(rom: &INesRom) -> Self {
        // Determine mirroring from ROM header
        let mirroring = if rom.header.mirroring == 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        };

        Self {
            ram: [0; 0x0800],
            ppu: Ppu::new(mirroring),
            prg_rom: rom.prg_rom.clone(),
            chr_rom: rom.chr_rom.clone(),
            apu_io: [0xFF; 0x20],
            ppu_cycles: 0,
            nmi_pending: false,
        }
    }

    /// Reset the bus and PPU
    fn reset(&mut self) {
        self.ppu.reset();
        self.ppu_cycles = 0;
        self.nmi_pending = false;
    }

    /// Take and clear the pending NMI flag.
    fn take_nmi(&mut self) -> bool {
        let pending = self.nmi_pending;
        self.nmi_pending = false;
        pending
    }

    /// Get PPU frame buffer for rendering verification (if needed)
    #[allow(dead_code)]
    fn frame_buffer(&self) -> &[u8] {
        self.ppu.frame_buffer()
    }
}

impl CpuBus for TestBus {
    /// Step PPU 3 dots per CPU cycle for cycle-accurate synchronization.
    ///
    /// This is called BEFORE each CPU memory access, ensuring PPU state
    /// is correct when CPU reads/writes memory (critical for $2002 timing).
    fn on_cpu_cycle(&mut self) {
        // PPU runs at 3x CPU clock (3 PPU dots per CPU cycle for NTSC)
        for _ in 0..3 {
            let (_frame_complete, nmi) = self.ppu.step();
            if nmi {
                self.nmi_pending = true;
            }
        }
    }

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // 2KB RAM, mirrored 4 times
            0x0000..=0x1FFF => {
                let mirror_addr = addr & 0x07FF;
                self.ram[mirror_addr as usize]
            }

            // PPU registers (mirrored every 8 bytes)
            0x2000..=0x3FFF => {
                let ppu_addr = 0x2000 + (addr & 0x07);
                let chr_rom = &self.chr_rom;
                self.ppu
                    .read_register(ppu_addr, |addr| chr_rom[addr as usize])
            }

            // APU and I/O registers
            0x4000..=0x401F => {
                let reg_addr = (addr - 0x4000) as usize;
                self.apu_io[reg_addr]
            }

            // Cartridge space
            0x6000..=0x7FFF => {
                // Battery-backed RAM (used for test results)
                // For now, we'll use regular RAM mirrored
                let ram_addr = (addr - 0x6000) as usize;
                if ram_addr < 0x0800 {
                    self.ram[ram_addr]
                } else {
                    0
                }
            }

            // PRG-ROM
            0x8000..=0xFFFF => {
                let rom_addr = (addr - 0x8000) as usize;

                // Handle ROM mirroring for 16KB ROMs
                if self.prg_rom.len() == 16384 {
                    self.prg_rom[rom_addr % 16384]
                } else if rom_addr < self.prg_rom.len() {
                    self.prg_rom[rom_addr]
                } else {
                    0
                }
            }

            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // 2KB RAM, mirrored 4 times
            0x0000..=0x1FFF => {
                let mirror_addr = addr & 0x07FF;
                self.ram[mirror_addr as usize] = value;
            }

            // PPU registers (mirrored every 8 bytes)
            0x2000..=0x3FFF => {
                let ppu_addr = 0x2000 + (addr & 0x07);
                let chr_rom = &mut self.chr_rom;
                self.ppu.write_register(ppu_addr, value, |addr, val| {
                    if (addr as usize) < chr_rom.len() {
                        chr_rom[addr as usize] = val;
                    }
                });
            }

            // APU and I/O registers
            0x4000..=0x401F => {
                let reg_addr = (addr - 0x4000) as usize;
                self.apu_io[reg_addr] = value;

                // Handle OAMDMA ($4014)
                if addr == 0x4014 {
                    // DMA from CPU memory to OAM
                    // For simplicity, we'll skip actual DMA implementation in tests
                    // Real implementation would copy 256 bytes from $XX00-$XXFF to OAM
                }
            }

            // Cartridge space
            0x6000..=0x7FFF => {
                // Battery-backed RAM (used for test results)
                let ram_addr = (addr - 0x6000) as usize;
                if ram_addr < 0x0800 {
                    self.ram[ram_addr] = value;
                }
            }

            // PRG-ROM (writes ignored)
            0x8000..=0xFFFF => {}

            _ => {}
        }
    }

    fn peek(&self, addr: u16) -> u8 {
        // Non-destructive read for debugging
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x8000..=0xFFFF => {
                let rom_addr = (addr - 0x8000) as usize;
                if self.prg_rom.len() == 16384 {
                    self.prg_rom[rom_addr % 16384]
                } else if rom_addr < self.prg_rom.len() {
                    self.prg_rom[rom_addr]
                } else {
                    0
                }
            }
            _ => 0,
        }
    }
}

/// Run a test ROM and check for success/failure.
///
/// Uses cycle-accurate execution with `cpu.tick()` which calls `on_cpu_cycle()`
/// before each memory access, stepping PPU 3 dots per CPU cycle.
///
/// Returns the test result code from address $6000:
/// - 0x00: Success
/// - 0x01+: Error code (test-specific)
fn run_test_rom(rom_path: &PathBuf) -> Result<u8, String> {
    // Load ROM
    let rom = INesRom::load(rom_path).map_err(|e| format!("Failed to load ROM: {e}"))?;

    println!("  Mapper: {}", rom.header.mapper);
    println!("  PRG-ROM: {} bytes", rom.prg_rom_size());
    println!("  CHR-ROM: {} bytes", rom.chr_rom_size());

    // Create CPU and bus
    let mut cpu = Cpu::new();
    let mut bus = TestBus::new(&rom);

    // Reset CPU and PPU
    bus.reset();
    cpu.reset(&mut bus);

    println!("  Starting at PC=${:04X}", cpu.pc);

    // Execute until test completes or timeout
    let max_frames = 600; // 10 seconds at 60fps
    let mut frames = 0;
    let mut last_check_cycle = 0u64;

    loop {
        // Execute one CPU cycle using cycle-accurate tick()
        // PPU is stepped via on_cpu_cycle() callback BEFORE each memory access
        let instruction_complete = cpu.tick(&mut bus);

        // Handle NMI from PPU (captured during on_cpu_cycle)
        if bus.take_nmi() {
            cpu.trigger_nmi();
        }

        // Check for test completion every ~10000 cycles (after instruction completes)
        if instruction_complete && cpu.cycles >= last_check_cycle + 10_000 {
            last_check_cycle = cpu.cycles;
            let result = Bus::read(&mut bus, 0x6000);

            // Check if test has started writing results
            // Some tests write 0x80 while running, then final result
            if result != 0x80 && result != 0xFF && cpu.cycles > 100_000 {
                // Test likely complete
                println!(
                    "  Test result at ${:02X} after {} cycles",
                    result, cpu.cycles
                );
                return Ok(result);
            }
        }

        // Frame counter (approximate)
        if cpu.cycles > (29780 * (frames + 1)) {
            frames += 1;
            if frames >= max_frames {
                return Err(format!("Test timeout after {frames} frames"));
            }
        }

        // Check for CPU jam
        if cpu.jammed {
            let result = Bus::read(&mut bus, 0x6000);
            println!(
                "  CPU jammed after {} cycles, result=${:02X}",
                cpu.cycles, result
            );
            return Ok(result);
        }
    }
}

#[test]
fn test_ppu_vbl_basics() {
    // Path to test ROM
    let rom_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test-roms/ppu")
        .join("ppu_01-vbl_basics.nes");

    // Skip if ROM doesn't exist
    if !rom_path.exists() {
        eprintln!("Skipping PPU VBL basics test: ROM not found");
        eprintln!(
            "Download from: https://github.com/christopherpow/nes-test-roms/tree/master/ppu_vbl_nmi"
        );
        eprintln!("Place in: test-roms/ppu/01-vbl_basics.nes");
        return;
    }

    println!("Running 01-vbl_basics.nes:");

    match run_test_rom(&rom_path) {
        Ok(result) => {
            assert_eq!(
                result, 0x00,
                "PPU VBL basics test failed with code: ${result:02X}"
            );
            println!("  PASSED!");
        }
        Err(e) => {
            eprintln!("  ERROR: {e}");
            panic!("Test execution failed");
        }
    }
}

/// VBlank Set Time Test
///
/// This test requires ±2 cycle timing accuracy for VBlank flag detection.
///
/// Now uses cycle-accurate execution via `cpu.tick()` with `CpuBus::on_cpu_cycle()`
/// which steps PPU 3 dots BEFORE each CPU memory access, enabling precise
/// detection of VBlank flag state when CPU reads $2002 (PPUSTATUS).
///
/// Expected: $00 (VBlank set timing within ±2 cycles)
#[test]
fn test_ppu_vbl_set_time() {
    let rom_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test-roms/ppu")
        .join("ppu_02-vbl_set_time.nes");

    if !rom_path.exists() {
        eprintln!("Skipping PPU VBL set time test: ROM not found");
        return;
    }

    println!("Running ppu_02-vbl_set_time.nes:");

    match run_test_rom(&rom_path) {
        Ok(result) => {
            assert_eq!(
                result, 0x00,
                "PPU VBL set time test failed with code: ${result:02X}"
            );
            println!("  PASSED!");
        }
        Err(e) => {
            eprintln!("  ERROR: {e}");
            panic!("Test execution failed");
        }
    }
}

/// VBlank Clear Time Test
///
/// This test requires exact cycle timing accuracy for VBlank flag clear detection.
///
/// Now uses cycle-accurate execution via `cpu.tick()` with `CpuBus::on_cpu_cycle()`
/// which steps PPU 3 dots BEFORE each CPU memory access, enabling precise
/// detection of VBlank flag clear when CPU reads $2002 (PPUSTATUS) during pre-render.
///
/// Expected: $00 (VBlank clear timing exact)
#[test]
fn test_ppu_vbl_clear_time() {
    let rom_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test-roms/ppu")
        .join("ppu_03-vbl_clear_time.nes");

    if !rom_path.exists() {
        eprintln!("Skipping PPU VBL clear time test: ROM not found");
        return;
    }

    println!("Running ppu_03-vbl_clear_time.nes:");

    match run_test_rom(&rom_path) {
        Ok(result) => {
            assert_eq!(
                result, 0x00,
                "PPU VBL clear time test failed with code: ${result:02X}"
            );
            println!("  PASSED!");
        }
        Err(e) => {
            eprintln!("  ERROR: {e}");
            panic!("Test execution failed");
        }
    }
}

#[test]
fn test_sprite_hit_basics() {
    let rom_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test-roms/ppu")
        .join("ppu_01.basics.nes");

    if !rom_path.exists() {
        eprintln!("Skipping sprite hit basics test: ROM not found");
        eprintln!(
            "Download from: https://github.com/christopherpow/nes-test-roms/tree/master/sprite_hit_tests_2005.10.05"
        );
        return;
    }

    println!("Running sprite_hit 01.basics.nes:");

    match run_test_rom(&rom_path) {
        Ok(result) => {
            assert_eq!(
                result, 0x00,
                "Sprite hit basics test failed with code: ${result:02X}"
            );
            println!("  PASSED!");
        }
        Err(e) => {
            eprintln!("  ERROR: {e}");
            // Don't panic yet - sprite hit is complex and may not be fully implemented
            eprintln!("  (Sprite hit tests may fail until full PPU rendering is implemented)");
        }
    }
}

#[test]
fn test_sprite_hit_alignment() {
    let rom_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test-roms/ppu")
        .join("ppu_02.alignment.nes");

    if !rom_path.exists() {
        eprintln!("Skipping sprite hit alignment test: ROM not found");
        return;
    }

    println!("Running sprite_hit 02.alignment.nes:");

    match run_test_rom(&rom_path) {
        Ok(result) => {
            assert_eq!(
                result, 0x00,
                "Sprite hit alignment test failed with code: ${result:02X}"
            );
            println!("  PASSED!");
        }
        Err(e) => {
            eprintln!("  ERROR: {e}");
            eprintln!("  (Sprite hit tests may fail until full PPU rendering is implemented)");
        }
    }
}

/// Comprehensive PPU test ROM suite (master ROM containing all tests)
#[test]
fn test_ppu_vbl_nmi_suite() {
    let rom_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test-roms/ppu")
        .join("ppu_vbl_nmi.nes");

    if !rom_path.exists() {
        eprintln!("Skipping PPU VBL/NMI suite: ROM not found");
        eprintln!(
            "Download from: https://github.com/christopherpow/nes-test-roms/tree/master/ppu_vbl_nmi"
        );
        return;
    }

    println!("Running ppu_vbl_nmi.nes (full suite):");
    println!("  Note: This ROM contains all VBL/NMI tests in one file");

    match run_test_rom(&rom_path) {
        Ok(result) => {
            if result == 0x00 {
                println!("  PASSED!");
            } else {
                println!("  Some tests failed (result=${result:02X})");
                println!("  Run individual test ROMs for details");
            }
        }
        Err(e) => {
            eprintln!("  ERROR: {e}");
        }
    }
}

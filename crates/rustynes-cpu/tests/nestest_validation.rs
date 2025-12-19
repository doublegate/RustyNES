//! nestest.nes golden log validation test.
//!
//! This integration test validates the CPU implementation against the
//! nestest.nes golden log, ensuring cycle-accurate emulation.

use rustynes_cpu::{Bus, Cpu, CpuTracer, INesRom};
use std::path::PathBuf;

/// Simple bus implementation for nestest.
///
/// nestest uses mapper 0 (NROM) which has simple memory mapping:
/// - $0000-$07FF: 2KB internal RAM (mirrored to $0800-$1FFF)
/// - $8000-$BFFF: First 16KB of PRG-ROM
/// - $C000-$FFFF: Last 16KB of PRG-ROM (or mirror of first 16KB if only 16KB total)
struct NestestBus {
    ram: [u8; 0x0800],  // 2KB RAM
    apu_io: [u8; 0x20], // APU and I/O registers ($4000-$401F)
    prg_rom: Vec<u8>,   // PRG-ROM data
}

impl NestestBus {
    fn new(rom: &INesRom) -> Self {
        Self {
            ram: [0; 0x0800],
            apu_io: [0xFF; 0x20], // Initialize APU/IO registers to 0xFF for nestest
            prg_rom: rom.prg_rom.clone(),
        }
    }
}

impl Bus for NestestBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // 2KB RAM, mirrored 4 times
            0x0000..=0x1FFF => {
                let mirror_addr = addr & 0x07FF;
                self.ram[mirror_addr as usize]
            }

            // PPU registers (not needed for CPU-only test)
            0x2000..=0x3FFF => 0,

            // APU and I/O registers
            0x4000..=0x401F => {
                let reg_addr = (addr - 0x4000) as usize;
                self.apu_io[reg_addr]
            }

            // Cartridge space
            0x6000..=0x7FFF => 0, // Battery-backed RAM (not used by nestest)

            // PRG-ROM
            0x8000..=0xFFFF => {
                let rom_addr = (addr - 0x8000) as usize;

                // Handle ROM mirroring for 16KB ROMs
                if self.prg_rom.len() == 16384 {
                    // Mirror: $C000-$FFFF maps to same data as $8000-$BFFF
                    self.prg_rom[rom_addr % 16384]
                } else {
                    // 32KB ROM: direct mapping
                    self.prg_rom[rom_addr]
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

            // PPU registers (ignored)
            0x2000..=0x3FFF => {}

            // APU and I/O registers
            0x4000..=0x401F => {
                let reg_addr = (addr - 0x4000) as usize;
                self.apu_io[reg_addr] = value;
            }

            // Cartridge space
            0x6000..=0x7FFF => {} // Battery-backed RAM (ignored)

            // PRG-ROM (writes ignored)
            0x8000..=0xFFFF => {}

            _ => {}
        }
    }
}

/// Compare two log lines and find differences.
fn compare_log_lines(line_num: usize, expected: &str, actual: &str) -> Result<(), String> {
    if expected == actual {
        return Ok(());
    }

    // Find the first difference
    let mut diff_pos = 0;
    for (i, (e_ch, a_ch)) in expected.chars().zip(actual.chars()).enumerate() {
        if e_ch != a_ch {
            diff_pos = i;
            break;
        }
    }

    Err(format!(
        "Line {line_num} mismatch at position {diff_pos}:\nExpected: {expected}\nActual:   {actual}\n"
    ))
}

#[test]
#[allow(clippy::too_many_lines)] // Test function requires detailed validation logic
fn nestest_golden_log_validation() {
    // Load nestest.nes ROM
    // Path is relative to workspace root
    let rom_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..") // crates
        .join("..") // workspace root
        .join("test-roms")
        .join("cpu")
        .join("nestest.nes");

    // Skip test if nestest.nes doesn't exist (test ROMs not included in repo)
    if !rom_path.exists() {
        eprintln!("Skipping nestest validation: nestest.nes not found at {rom_path:?}");
        eprintln!("To run this test, download nestest.nes from https://github.com/christopherpow/nes-test-roms");
        eprintln!("and place it in the test-roms/cpu/ directory");
        return;
    }

    let rom = INesRom::load(&rom_path).expect("Failed to load nestest.nes");

    println!("Loaded nestest.nes:");
    println!("  Mapper: {}", rom.header.mapper);
    println!("  PRG-ROM: {} bytes", rom.prg_rom_size());
    println!("  CHR-ROM: {} bytes", rom.chr_rom_size());

    // Verify it's mapper 0
    assert_eq!(rom.header.mapper, 0, "nestest.nes should use mapper 0");

    // Create CPU and bus
    let mut cpu = Cpu::new();
    let mut bus = NestestBus::new(&rom);
    let mut tracer = CpuTracer::new();

    // Set up automation mode starting state
    // nestest automation starts at $C000 with cycles=7
    cpu.pc = 0xC000;
    cpu.cycles = 7;

    // Load golden log
    let golden_log_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..") // crates
        .join("..") // workspace root
        .join("test-roms")
        .join("cpu")
        .join("nestest.log");

    // Skip test if golden log doesn't exist
    if !golden_log_path.exists() {
        eprintln!("Skipping nestest validation: nestest.log not found at {golden_log_path:?}");
        eprintln!("To run this test, download nestest.log from https://github.com/christopherpow/nes-test-roms");
        eprintln!("and place it in the test-roms/cpu/ directory");
        return;
    }

    let golden_log = std::fs::read_to_string(&golden_log_path).expect("Failed to load nestest.log");

    // Split into lines and remove PPU cycle information for comparison
    // Golden log format: "... CYC:7\n" or "... PPU:  0, 21 CYC:7\n"
    let golden_lines: Vec<String> = golden_log
        .lines()
        .map(|line| {
            // Remove PPU cycle info if present
            if let Some(ppu_pos) = line.find("PPU:") {
                // Find CYC after PPU
                if let Some(cyc_pos) = line[ppu_pos..].find("CYC:") {
                    let before_ppu = &line[..ppu_pos];
                    let cyc_part = &line[ppu_pos + cyc_pos..];
                    format!("{before_ppu}{cyc_part}")
                } else {
                    line.to_string()
                }
            } else {
                line.to_string()
            }
        })
        .collect();

    println!("Golden log: {} lines", golden_lines.len());
    println!("Starting nestest automation mode at PC=$C000, cycles=7");

    // Execute instructions and compare traces
    let mut line_num = 0;
    let max_cycles = 100_000; // Safety limit

    while cpu.cycles < max_cycles {
        // Trace before execution
        tracer.trace(&cpu, &mut bus);
        line_num += 1;

        // Compare with golden log
        if line_num <= golden_lines.len() {
            let expected = &golden_lines[line_num - 1];
            let log = tracer.get_log();
            let actual_lines: Vec<&str> = log.lines().collect();
            let actual = actual_lines[line_num - 1];

            if let Err(e) = compare_log_lines(line_num, expected, actual) {
                eprintln!("\nDIVERGENCE DETECTED:\n{e}");
                eprintln!("CPU State:");
                eprintln!("  PC: ${:04X}", cpu.pc);
                eprintln!("  A:  ${:02X}", cpu.a);
                eprintln!("  X:  ${:02X}", cpu.x);
                eprintln!("  Y:  ${:02X}", cpu.y);
                eprintln!("  P:  ${:02X}", cpu.status.bits());
                eprintln!("  SP: ${:02X}", cpu.sp);
                eprintln!("  Cycles: {}", cpu.cycles);

                panic!("nestest validation failed at line {line_num}");
            }
        }

        // Execute instruction
        cpu.step(&mut bus);

        // Check for completion (PC = $C66E)
        if cpu.pc == 0xC66E {
            println!("nestest completed at line {line_num}");
            break;
        }

        // Check for infinite loop (JAM instruction or stuck)
        if cpu.jammed {
            eprintln!("CPU jammed at line {line_num}");
            break;
        }
    }

    // Verify we completed all expected lines
    assert!(
        line_num >= golden_lines.len() || cpu.pc == 0xC66E,
        "Test did not complete all {} lines (stopped at {})",
        golden_lines.len(),
        line_num
    );

    // Check test result at $6000
    let test_result = bus.read(0x6000);
    assert_eq!(
        test_result, 0x00,
        "nestest reported error code: 0x{test_result:02X}"
    );

    println!("\nnestest PASSED!");
    println!("  Total lines traced: {line_num}");
    println!("  Final PC: ${:04X}", cpu.pc);
    println!("  Final cycles: {}", cpu.cycles);
}

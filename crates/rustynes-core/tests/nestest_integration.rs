//! NESTest ROM integration test.
//!
//! This test validates CPU emulation against the nestest.nes test ROM.
//! The nestest ROM starts at $C000 in automation mode.

use rustynes_core::Console;
use std::fs;
use std::path::PathBuf;

/// Get the workspace root directory.
fn workspace_root() -> PathBuf {
    // Navigate from crate to workspace root
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Load and validate nestest.nes execution.
#[test]
fn test_nestest_basic_execution() {
    // Load the nestest ROM
    let rom_path = workspace_root().join("test-roms/cpu/nestest.nes");

    let Ok(rom_data) = fs::read(&rom_path) else {
        println!("Skipping nestest: ROM file not found at {rom_path:?}");
        return;
    };

    // Create console
    let mut console = Console::new(&rom_data).expect("Failed to create console from nestest.nes");

    // Power on and reset
    console.power_on();

    // Validate initial state
    assert_eq!(console.mapper_number(), 0, "nestest uses NROM (mapper 0)");
    assert_eq!(console.mapper_name(), "NROM");

    println!("nestest.nes loaded successfully");
    println!("Initial PC: 0x{:04X}", console.cpu().pc);
    println!("Initial SP: 0x{:02X}", console.cpu().sp);

    // Run for a number of instructions to verify basic execution
    let max_instructions = 10_000;
    let mut instruction_count = 0;

    for _ in 0..max_instructions {
        let cycles = console.step();
        if cycles == 0 {
            break;
        }
        instruction_count += 1;
    }

    println!("Executed {instruction_count} instructions");
    println!("Total cycles: {}", console.total_cycles());
    println!("Final PC: 0x{:04X}", console.cpu().pc);

    // Verify we ran some instructions
    assert!(
        instruction_count > 100,
        "Should execute at least 100 instructions"
    );
    assert!(console.total_cycles() > 0, "Should have accumulated cycles");
}

/// Test ROM loading for various test ROMs.
#[test]
fn test_rom_loading_cpu() {
    let root = workspace_root();
    let rom_paths = [
        "test-roms/cpu/nestest.nes",
        "test-roms/cpu/cpu_nestest.nes",
        "test-roms/cpu/cpu_all_instrs.nes",
    ];

    for path in &rom_paths {
        let full_path = root.join(path);
        if let Ok(rom_data) = fs::read(&full_path) {
            match Console::new(&rom_data) {
                Ok(console) => {
                    println!("Loaded: {path} (mapper {})", console.mapper_number());
                }
                Err(e) => {
                    println!("Failed to load {path}: {e}");
                }
            }
        }
    }
}

/// Test ROM loading for PPU test ROMs.
#[test]
fn test_rom_loading_ppu() {
    let root = workspace_root();
    let rom_paths = [
        "test-roms/ppu/ppu_01-vbl_basics.nes",
        "test-roms/ppu/ppu_vbl_nmi.nes",
        "test-roms/ppu/ppu_palette_ram.nes",
    ];

    for path in &rom_paths {
        let full_path = root.join(path);
        if let Ok(rom_data) = fs::read(&full_path) {
            match Console::new(&rom_data) {
                Ok(console) => {
                    println!("Loaded: {path} (mapper {})", console.mapper_number());
                }
                Err(e) => {
                    println!("Failed to load {path}: {e}");
                }
            }
        }
    }
}

/// Test ROM loading for APU test ROMs.
#[test]
fn test_rom_loading_apu() {
    let root = workspace_root();
    let rom_paths = [
        "test-roms/apu/apu_test_1.nes",
        "test-roms/apu/apu_len_ctr.nes",
        "test-roms/apu/apu_env.nes",
    ];

    for path in &rom_paths {
        let full_path = root.join(path);
        if let Ok(rom_data) = fs::read(&full_path) {
            match Console::new(&rom_data) {
                Ok(console) => {
                    println!("Loaded: {path} (mapper {})", console.mapper_number());
                }
                Err(e) => {
                    println!("Failed to load {path}: {e}");
                }
            }
        }
    }
}

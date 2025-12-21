//! NROM Debugging Test
//!
//! Investigates why NROM test ROMs (ppu_palette_ram, apu_len_ctr) fail with 0xFF.

use rustynes_core::Console;
use std::path::PathBuf;

#[test]
fn debug_apu_len_ctr_boot() {
    let rom_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..") // crates/
        .join("..") // workspace root
        .join("test-roms")
        .join("apu")
        .join("apu_len_ctr.nes");

    if !rom_path.exists() {
        eprintln!("Skipping debug test: ROM not found");
        return;
    }

    let rom_data = std::fs::read(&rom_path).unwrap();
    let mut console = Console::from_rom_bytes(&rom_data).unwrap();

    println!("Loaded apu_len_ctr.nes");
    println!(
        "Reset Vector: {:04X}",
        console.peek_memory(0xFFFC) as u16 | ((console.peek_memory(0xFFFD) as u16) << 8)
    );
    println!("Initial PC: {:04X}", console.cpu().pc);

    // Run for many instructions and track writes to $6000
    let mut writes_to_6000 = Vec::new();
    for i in 0..5000 {
        let pc = console.cpu().pc;
        let opcode = console.peek_memory(pc);

        let status_before = console.peek_memory(0x6000);
        console.step();
        let status_after = console.peek_memory(0x6000);

        if status_before != status_after {
            println!(
                "Instr {i}: PC=${pc:04X} Opcode=${opcode:02X} wrote ${status_after:02X} to $6000"
            );
            writes_to_6000.push((i, pc, status_after));
        }

        if i < 50 {
            println!("Instr {i}: PC=${pc:04X} Opcode=${opcode:02X}");
        }
    }

    if writes_to_6000.is_empty() {
        println!("No writes to $6000 detected.");
    }
}

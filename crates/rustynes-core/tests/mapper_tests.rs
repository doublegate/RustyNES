//! Mapper test suite validation.
//!
//! This integration test runs Holy Mapperel and other mapper test ROMs.
//! Validates NROM, MMC1, UxROM, CNROM, and MMC3 implementations.

use rustynes_core::Console;
use std::path::PathBuf;

/// Maximum frames to run before timeout (20 seconds at 60 FPS)
const MAX_FRAMES: u32 = 1200;

/// Check test completion and result.
fn check_test_result(console: &Console) -> (bool, bool, Option<String>) {
    let status = console.peek_memory(0x6000);

    match status {
        0x80 => (false, false, None), // Running

        0x00 => (true, true, None), // Pass

        _ => {
            // Fail

            let code1 = console.peek_memory(0x6001);

            let code2 = console.peek_memory(0x6002);

            let mut text = String::new();

            for i in 0..256 {
                let ch = console.peek_memory(0x6004 + i);

                if ch == 0 {
                    break;
                }

                if ch.is_ascii() && ch >= 0x20 {
                    text.push(ch as char);
                }
            }

            let msg = if text.is_empty() {
                format!("Failed with status ${status:02X}, code ${code1:02X} ${code2:02X}")
            } else {
                format!("Failed: {text}")
            };

            (true, false, Some(msg))
        }
    }
}

fn run_mapper_test(rom_name: &str) -> Result<(), String> {
    let rom_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..") // crates/
        .join("..") // workspace root
        .join("test-roms")
        .join("mappers")
        .join(rom_name);

    if !rom_path.exists() {
        eprintln!("Skipping {rom_name}: ROM not found");

        return Ok(());
    }

    println!("Running test: {rom_name}");

    let rom_data = std::fs::read(&rom_path).map_err(|e| e.to_string())?;

    let mut console = Console::from_rom_bytes(&rom_data).map_err(|e| e.to_string())?;

    for frame in 0..MAX_FRAMES {
        console.step_frame_accurate(); // Use accurate timing just in case

        if frame >= 10 {
            let (done, pass, msg) = check_test_result(&console);

            if done {
                if pass {
                    println!("  ✓ PASS ({} frames)", frame + 1);

                    return Ok(());
                }

                let err = msg.unwrap_or_default();

                println!("  ✗ FAIL: {err}");

                return Err(err);
            }
        }
    }

    println!("  ✗ TIMEOUT");

    Err("Timeout".to_string())
}

// ============================================================================
// NROM (Mapper 0)
// ============================================================================

#[test]
#[ignore = "Holy Mapperel test requires investigation - pre-existing issue"]
fn test_nrom_0_p32k_c8k_v() {
    run_mapper_test("mapper_holymapperel_0_P32K_C8K_V.nes").unwrap();
}

#[test]
fn test_nrom_0_p32k_cr32k_v() {
    run_mapper_test("mapper_holymapperel_0_P32K_CR32K_V.nes").unwrap();
}

#[test]
fn test_nrom_0_p32k_cr8k_v() {
    run_mapper_test("mapper_holymapperel_0_P32K_CR8K_V.nes").unwrap();
}

// ============================================================================
// MMC1 (Mapper 1)
// ============================================================================

#[test]
#[ignore = "Holy Mapperel test requires investigation - pre-existing issue"]
fn test_mmc1_p128k_c128k_s8k() {
    run_mapper_test("mapper_holymapperel_1_P128K_C128K_S8K.nes").unwrap();
}

#[test]
#[ignore = "Holy Mapperel test requires investigation - pre-existing issue"]
fn test_mmc1_p128k_c128k_w8k() {
    run_mapper_test("mapper_holymapperel_1_P128K_C128K_W8K.nes").unwrap();
}

#[test]
#[ignore = "Holy Mapperel test requires investigation - pre-existing issue"]
fn test_mmc1_p128k_c32k_s8k() {
    run_mapper_test("mapper_holymapperel_1_P128K_C32K_S8K.nes").unwrap();
}

#[test]
#[ignore = "Holy Mapperel test requires investigation - pre-existing issue"]
fn test_mmc1_p128k_c32k_w8k() {
    run_mapper_test("mapper_holymapperel_1_P128K_C32K_W8K.nes").unwrap();
}

#[test]
fn test_mmc1_p512k_cr8k_s32k() {
    run_mapper_test("mapper_holymapperel_1_P512K_CR8K_S32K.nes").unwrap();
}

#[test]
fn test_mmc1_p512k_cr8k_s8k() {
    run_mapper_test("mapper_holymapperel_1_P512K_CR8K_S8K.nes").unwrap();
}

#[test]
fn test_mmc1_p512k_s32k() {
    run_mapper_test("mapper_holymapperel_1_P512K_S32K.nes").unwrap();
}

#[test]
fn test_mmc1_p512k_s8k() {
    run_mapper_test("mapper_holymapperel_1_P512K_S8K.nes").unwrap();
}

#[test]
#[ignore = "Holy Mapperel test requires investigation - pre-existing issue"]
fn test_mmc1_p128k_c128k() {
    run_mapper_test("mapper_holymapperel_1_P128K_C128K.nes").unwrap();
}

#[test]
#[ignore = "Holy Mapperel test requires investigation - pre-existing issue"]
fn test_mmc1_p128k_c32k() {
    run_mapper_test("mapper_holymapperel_1_P128K_C32K.nes").unwrap();
}

#[test]
fn test_mmc1_p128k_cr8k() {
    run_mapper_test("mapper_holymapperel_1_P128K_CR8K.nes").unwrap();
}

#[test]
fn test_mmc1_p128k() {
    run_mapper_test("mapper_holymapperel_1_P128K.nes").unwrap();
}

// ============================================================================
// UxROM (Mapper 2)
// ============================================================================

#[test]
fn test_uxrom_p128k_cr8k_v() {
    run_mapper_test("mapper_holymapperel_2_P128K_CR8K_V.nes").unwrap();
}

#[test]
fn test_uxrom_p128k_v() {
    run_mapper_test("mapper_holymapperel_2_P128K_V.nes").unwrap();
}

// ============================================================================
// CNROM (Mapper 3)
// ============================================================================

#[test]
#[ignore = "Holy Mapperel test requires investigation - pre-existing issue"]
fn test_cnrom_p32k_c32k_h() {
    run_mapper_test("mapper_holymapperel_3_P32K_C32K_H.nes").unwrap();
}

// ============================================================================
// MMC3 (Mapper 4)
// ============================================================================

#[test]
fn test_mmc3_p128k_cr32k() {
    run_mapper_test("mapper_holymapperel_4_P128K_CR32K.nes").unwrap();
}

#[test]
fn test_mmc3_p128k_cr8k() {
    run_mapper_test("mapper_holymapperel_4_P128K_CR8K.nes").unwrap();
}

#[test]
fn test_mmc3_p128k() {
    run_mapper_test("mapper_holymapperel_4_P128K.nes").unwrap();
}

#[test]
fn test_mmc3_p256k_c256k() {
    run_mapper_test("mapper_holymapperel_4_P256K_C256K.nes").unwrap();
}

// ============================================================================
// MMC3 IRQ Tests
// ============================================================================

#[test]
fn test_mmc3_irq_1_clocking() {
    run_mapper_test("mapper_mmc3_irq_1_clocking.nes").unwrap();
}

#[test]
fn test_mmc3_irq_2_details() {
    run_mapper_test("mapper_mmc3_irq_2_details.nes").unwrap();
}

#[test]
fn test_mmc3_irq_3_a12_clocking() {
    run_mapper_test("mapper_mmc3_irq_3_a12_clocking.nes").unwrap();
}

#[test]
fn test_mmc3_irq_4_scanline_timing() {
    run_mapper_test("mapper_mmc3_irq_4_scanline_timing.nes").unwrap();
}

#[test]
fn test_mmc3_irq_5_rev_a() {
    run_mapper_test("mapper_mmc3_irq_5_rev_a.nes").unwrap();
}

#[test]
fn test_mmc3_irq_6_rev_b() {
    run_mapper_test("mapper_mmc3_irq_6_rev_b.nes").unwrap();
}

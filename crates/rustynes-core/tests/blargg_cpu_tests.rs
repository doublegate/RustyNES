//! Blargg CPU instruction test suite validation.
//!
//! This integration test runs all Blargg CPU test ROMs to validate
//! instruction timing, addressing modes, and edge case handling.
//!
//! Test ROM format (Blargg standard):
//! - $6000: Status (0x80 = running, 0x81 = reset needed, 0x00 = pass, other = fail)
//! - $6001-$6003: Error signature
//! - Text output at $6004+ (null-terminated string)

use rustynes_core::Console;
use std::path::PathBuf;

/// Maximum frames to run before timeout (5 seconds at 60 FPS)
const MAX_FRAMES: u32 = 300;

/// Check test completion and result.
///
/// Returns (is_complete, is_pass, error_message)
fn check_blargg_result(console: &Console) -> (bool, bool, Option<String>) {
    let status = console.peek_memory(0x6000);

    match status {
        0x80 => {
            // Still running
            (false, false, None)
        }
        0x81 => {
            // Reset needed (error)
            (true, false, Some("Test requested reset".to_string()))
        }
        0x00 => {
            // Pass
            (true, true, None)
        }
        _ => {
            // Fail with error code
            let error_code1 = console.peek_memory(0x6001);
            let error_code2 = console.peek_memory(0x6002);
            let error_code3 = console.peek_memory(0x6003);

            // Try to read error text from $6004
            let mut error_text = String::new();
            for i in 0..256 {
                let ch = console.peek_memory(0x6004 + i);
                if ch == 0 {
                    break;
                }
                if ch.is_ascii() && ch >= 0x20 {
                    error_text.push(ch as char);
                }
            }

            let msg = if error_text.is_empty() {
                format!(
                    "Test failed with status 0x{status:02X}, error signature: {error_code1:02X} {error_code2:02X} {error_code3:02X}"
                )
            } else {
                format!("Test failed: {error_text}")
            };

            (true, false, Some(msg))
        }
    }
}

/// Run a single Blargg test ROM and check result.
fn run_blargg_test(rom_name: &str) -> Result<(), String> {
    run_blargg_test_with_timeout(rom_name, MAX_FRAMES)
}

/// Run a single Blargg test ROM with custom timeout.
fn run_blargg_test_with_timeout(rom_name: &str, max_frames: u32) -> Result<(), String> {
    // Construct path to test ROM
    let rom_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..") // crates/
        .join("..") // workspace root
        .join("test-roms")
        .join("cpu")
        .join(rom_name);

    // Skip if ROM doesn't exist
    if !rom_path.exists() {
        eprintln!(
            "Skipping {rom_name}: ROM not found at {}",
            rom_path.display()
        );
        return Ok(()); // Don't fail if ROM is missing
    }

    println!("Running test: {rom_name}");

    // Load ROM
    let rom_data = std::fs::read(&rom_path).map_err(|e| format!("Failed to load ROM: {e}"))?;

    // Create console
    let mut console =
        Console::from_rom_bytes(&rom_data).map_err(|e| format!("Failed to create console: {e}"))?;

    // Run test
    for frame in 0..max_frames {
        console.step_frame();

        // Check result (but give ROM a few frames to initialize)
        if frame >= 10 {
            let (is_complete, is_pass, error_msg) = check_blargg_result(&console);

            if is_complete {
                if is_pass {
                    println!("  ✓ PASS (completed in {} frames)", frame + 1);
                    return Ok(());
                }
                let msg = error_msg.unwrap_or_else(|| "Unknown error".to_string());
                eprintln!("  ✗ FAIL (frame {}): {msg}", frame + 1);
                return Err(msg);
            }
        }
    }

    // Timeout - check final status
    let (_, is_pass, error_msg) = check_blargg_result(&console);
    if is_pass {
        println!("  ✓ PASS (completed at timeout)");
        Ok(())
    } else {
        let msg = error_msg.unwrap_or_else(|| "Test timed out without completion".to_string());
        eprintln!("  ✗ TIMEOUT: {msg}");
        Err(msg)
    }
}

// ============================================================================
// Blargg CPU Instruction Tests (11 tests)
// ============================================================================

#[test]
fn cpu_instr_01_implied() {
    run_blargg_test("cpu_instr_01_implied.nes").unwrap();
}

#[test]
fn cpu_instr_02_immediate() {
    run_blargg_test("cpu_instr_02_immediate.nes").unwrap();
}

#[test]
fn cpu_instr_03_zero_page() {
    run_blargg_test("cpu_instr_03_zero_page.nes").unwrap();
}

#[test]
fn cpu_instr_04_zp_xy() {
    run_blargg_test("cpu_instr_04_zp_xy.nes").unwrap();
}

#[test]
fn cpu_instr_05_absolute() {
    run_blargg_test("cpu_instr_05_absolute.nes").unwrap();
}

#[test]
fn cpu_instr_06_abs_xy() {
    run_blargg_test("cpu_instr_06_abs_xy.nes").unwrap();
}

#[test]
fn cpu_instr_07_ind_x() {
    run_blargg_test("cpu_instr_07_ind_x.nes").unwrap();
}

#[test]
fn cpu_instr_08_ind_y() {
    run_blargg_test("cpu_instr_08_ind_y.nes").unwrap();
}

#[test]
fn cpu_instr_09_branches() {
    run_blargg_test("cpu_instr_09_branches.nes").unwrap();
}

#[test]
fn cpu_instr_10_stack() {
    run_blargg_test("cpu_instr_10_stack.nes").unwrap();
}

#[test]
fn cpu_instr_11_special() {
    run_blargg_test("cpu_instr_11_special.nes").unwrap();
}

// ============================================================================
// Blargg Comprehensive Tests
// ============================================================================

#[test]
fn cpu_all_instrs() {
    run_blargg_test("cpu_all_instrs.nes").unwrap();
}

#[test]
fn cpu_official_only() {
    run_blargg_test("cpu_official_only.nes").unwrap();
}

// ============================================================================
// Timing Tests
// ============================================================================

#[test]
fn cpu_instr_timing() {
    run_blargg_test("cpu_instr_timing.nes").unwrap();
}

#[test]
fn cpu_instr_timing_1() {
    run_blargg_test("cpu_instr_timing_1.nes").unwrap();
}

#[test]
fn cpu_branch_timing_2() {
    run_blargg_test("cpu_branch_timing_2.nes").unwrap();
}

// ============================================================================
// Dummy Read/Write Tests
// ============================================================================

#[test]
fn cpu_dummy_reads() {
    run_blargg_test("cpu_dummy_reads.nes").unwrap();
}

#[test]
fn cpu_dummy_writes_ppumem() {
    run_blargg_test("cpu_dummy_writes_ppumem.nes").unwrap();
}

#[test]
fn cpu_dummy_writes_oam() {
    run_blargg_test("cpu_dummy_writes_oam.nes").unwrap();
}

// ============================================================================
// Interrupt Tests
// ============================================================================

#[test]
fn cpu_interrupts() {
    run_blargg_test("cpu_interrupts.nes").unwrap();
}

// ============================================================================
// Summary Test (runs all and generates report)
// ============================================================================

#[test]
#[allow(clippy::cast_precision_loss)]
fn blargg_cpu_test_suite_summary() {
    let tests = vec![
        // Instruction tests (11)
        "cpu_instr_01_implied.nes",
        "cpu_instr_02_immediate.nes",
        "cpu_instr_03_zero_page.nes",
        "cpu_instr_04_zp_xy.nes",
        "cpu_instr_05_absolute.nes",
        "cpu_instr_06_abs_xy.nes",
        "cpu_instr_07_ind_x.nes",
        "cpu_instr_08_ind_y.nes",
        "cpu_instr_09_branches.nes",
        "cpu_instr_10_stack.nes",
        "cpu_instr_11_special.nes",
        // Comprehensive tests (2)
        "cpu_all_instrs.nes",
        "cpu_official_only.nes",
        // Timing tests (3)
        "cpu_instr_timing.nes",
        "cpu_instr_timing_1.nes",
        "cpu_branch_timing_2.nes",
        // Dummy read/write tests (3)
        "cpu_dummy_reads.nes",
        "cpu_dummy_writes_ppumem.nes",
        "cpu_dummy_writes_oam.nes",
        // Interrupt tests (1)
        "cpu_interrupts.nes",
    ];

    println!("\n=== Blargg CPU Test Suite Summary ===\n");

    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let mut failed_tests = Vec::new();

    for test_name in &tests {
        match run_blargg_test(test_name) {
            Ok(()) => {
                // Check if it was actually run or skipped
                let rom_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("..") // crates/
                    .join("..") // workspace root
                    .join("test-roms")
                    .join("cpu")
                    .join(test_name);

                if rom_path.exists() {
                    passed += 1;
                } else {
                    skipped += 1;
                }
            }
            Err(e) => {
                failed += 1;
                failed_tests.push((test_name, e));
            }
        }
    }

    let total = tests.len();
    let pass_rate = if total - skipped > 0 {
        (passed as f64 / (total - skipped) as f64) * 100.0
    } else {
        0.0
    };

    println!("\n=== Results ===");
    println!("Total Tests: {total}");
    println!("Passed: {passed} ({pass_rate:.1}%)");
    println!("Failed: {failed}");
    println!("Skipped: {skipped} (ROM not found)");

    if !failed_tests.is_empty() {
        println!("\n=== Failed Tests ===");
        for (name, error) in &failed_tests {
            println!("  ✗ {name}: {error}");
        }
        panic!("{failed} test(s) failed");
    }

    println!("\n✓ All available Blargg CPU tests passed!");
}

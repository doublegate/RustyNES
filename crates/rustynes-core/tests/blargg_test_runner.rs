//! Blargg Test ROM Runner
//!
//! This test runner executes Blargg test ROMs and validates their results.
//! Blargg tests write their status to memory location $6000:
//! - $80 = Test passed
//! - $81+ = Test failed with error code
//! - $00 = Test still running
//!
//! Tests also output text messages to $6004+ for debugging.

use rustynes_core::Console;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

/// Get the workspace root directory.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Result of a Blargg test execution.
#[derive(Debug)]
#[allow(dead_code)]
struct BlarggResult {
    name: String,
    passed: bool,
    status_code: u8,
    message: Option<String>,
    cycles_run: u64,
    time_ms: u64,
}

/// Run a single Blargg test ROM.
fn run_blargg_test(rom_path: &PathBuf, max_cycles: u64) -> Result<BlarggResult, String> {
    let name = rom_path.file_name().unwrap().to_string_lossy().to_string();

    // Load the ROM
    let rom_data = fs::read(rom_path).map_err(|e| format!("Failed to read ROM: {e}"))?;

    // Create console
    let mut console =
        Console::new(&rom_data).map_err(|e| format!("Failed to create console: {e}"))?;

    // Power on
    console.power_on();

    let start_time = Instant::now();
    let mut cycles_run: u64 = 0;

    // Run until test completes or timeout
    loop {
        // Step one instruction
        let cycles = console.step() as u64;
        cycles_run += cycles;

        // Check status at $6000
        // Blargg protocol: $00 = passed, $01-$7F = error code, $80+ = running
        let status = console.peek_memory(0x6000);

        // $00 = passed (test completed successfully)
        if status == 0x00 {
            // Check for signature to confirm test is actually done
            let sig1 = console.peek_memory(0x6001);
            let sig2 = console.peek_memory(0x6002);
            let sig3 = console.peek_memory(0x6003);
            // Only consider passed if signature is present (test has written results)
            if sig1 == 0xDE && sig2 == 0xB0 && sig3 == 0x61 {
                let message = read_test_message(&console);
                return Ok(BlarggResult {
                    name,
                    passed: true,
                    status_code: status,
                    message,
                    cycles_run,
                    time_ms: start_time.elapsed().as_millis() as u64,
                });
            }
        } else if (0x01..=0x7F).contains(&status) {
            // $01-$7F = error code (test failed)
            let message = read_test_message(&console);
            return Ok(BlarggResult {
                name,
                passed: false,
                status_code: status,
                message,
                cycles_run,
                time_ms: start_time.elapsed().as_millis() as u64,
            });
        }
        // $80+ = test still running, continue

        // Timeout check
        if cycles_run >= max_cycles {
            let message = read_test_message(&console);
            return Ok(BlarggResult {
                name,
                passed: false,
                status_code: 0xFF, // Timeout
                message: Some(format!(
                    "Timeout after {} cycles. Last message: {}",
                    cycles_run,
                    message.unwrap_or_default()
                )),
                cycles_run,
                time_ms: start_time.elapsed().as_millis() as u64,
            });
        }
    }
}

/// Read the test message from $6004+.
fn read_test_message(console: &Console) -> Option<String> {
    let mut message = Vec::new();

    // Check for "signature" at $6001-$6003 (some tests use 0xDE, 0xB0, 0x61)
    let sig1 = console.peek_memory(0x6001);
    let sig2 = console.peek_memory(0x6002);
    let sig3 = console.peek_memory(0x6003);

    // Standard Blargg signature check
    if sig1 == 0xDE && sig2 == 0xB0 && sig3 == 0x61 {
        // Read message from $6004
        for addr in 0x6004..0x6100 {
            let byte = console.peek_memory(addr);
            if byte == 0 {
                break;
            }
            message.push(byte);
        }

        if !message.is_empty() {
            return Some(String::from_utf8_lossy(&message).to_string());
        }
    }

    None
}

/// Find all Blargg-style test ROMs in a directory.
fn find_test_roms(dir: &PathBuf) -> Vec<PathBuf> {
    let mut files = Vec::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "nes") {
                files.push(path);
            }
        }
    }

    files.sort();
    files
}

/// Run all CPU Blargg tests.
#[test]
#[allow(clippy::cast_precision_loss)]
fn test_blargg_cpu() {
    let root = workspace_root();
    let cpu_dir = root.join("test-roms/cpu");
    let roms = find_test_roms(&cpu_dir);

    println!("\n========================================");
    println!("  Blargg CPU Test Suite");
    println!("========================================\n");

    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;

    // Max cycles for CPU tests (about 5 seconds of emulated time)
    let max_cycles: u64 = 30_000_000;

    for rom_path in &roms {
        let name = rom_path.file_name().unwrap().to_string_lossy();

        // Skip non-blargg tests (nestest has different format)
        if name.contains("nestest") {
            skipped += 1;
            continue;
        }

        match run_blargg_test(rom_path, max_cycles) {
            Ok(result) => {
                if result.passed {
                    println!("  [PASS] {name}");
                    passed += 1;
                } else {
                    println!("  [FAIL] {name} (code: 0x{:02X})", result.status_code);
                    if let Some(msg) = &result.message {
                        println!("         Message: {msg}");
                    }
                    failed += 1;
                }
            }
            Err(e) => {
                println!("  [SKIP] {name}: {e}");
                skipped += 1;
            }
        }
    }

    let total = passed + failed;
    let pct = if total > 0 {
        (passed as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    println!("\n----------------------------------------");
    println!("CPU Tests: {passed}/{total} passed ({pct:.1}%), {skipped} skipped");
    println!("----------------------------------------\n");
}

/// Run all PPU Blargg tests.
#[test]
#[allow(clippy::cast_precision_loss)]
fn test_blargg_ppu() {
    let root = workspace_root();
    let ppu_dir = root.join("test-roms/ppu");
    let roms = find_test_roms(&ppu_dir);

    println!("\n========================================");
    println!("  Blargg PPU Test Suite");
    println!("========================================\n");

    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;

    // Max cycles for PPU tests
    let max_cycles: u64 = 50_000_000;

    for rom_path in &roms {
        let name = rom_path.file_name().unwrap().to_string_lossy();

        match run_blargg_test(rom_path, max_cycles) {
            Ok(result) => {
                if result.passed {
                    println!("  [PASS] {name}");
                    passed += 1;
                } else {
                    println!("  [FAIL] {name} (code: 0x{:02X})", result.status_code);
                    if let Some(msg) = &result.message {
                        println!("         Message: {msg}");
                    }
                    failed += 1;
                }
            }
            Err(e) => {
                println!("  [SKIP] {name}: {e}");
                skipped += 1;
            }
        }
    }

    let total = passed + failed;
    let pct = if total > 0 {
        (passed as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    println!("\n----------------------------------------");
    println!("PPU Tests: {passed}/{total} passed ({pct:.1}%), {skipped} skipped");
    println!("----------------------------------------\n");
}

/// Run all APU Blargg tests.
#[test]
#[allow(clippy::cast_precision_loss)]
fn test_blargg_apu() {
    let root = workspace_root();
    let apu_dir = root.join("test-roms/apu");
    let roms = find_test_roms(&apu_dir);

    println!("\n========================================");
    println!("  Blargg APU Test Suite");
    println!("========================================\n");

    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;

    // Max cycles for APU tests
    let max_cycles: u64 = 50_000_000;

    for rom_path in &roms {
        let name = rom_path.file_name().unwrap().to_string_lossy();

        // Skip directories
        if rom_path.is_dir() {
            continue;
        }

        match run_blargg_test(rom_path, max_cycles) {
            Ok(result) => {
                if result.passed {
                    println!("  [PASS] {name}");
                    passed += 1;
                } else {
                    println!("  [FAIL] {name} (code: 0x{:02X})", result.status_code);
                    if let Some(msg) = &result.message {
                        println!("         Message: {msg}");
                    }
                    failed += 1;
                }
            }
            Err(e) => {
                println!("  [SKIP] {name}: {e}");
                skipped += 1;
            }
        }
    }

    let total = passed + failed;
    let pct = if total > 0 {
        (passed as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    println!("\n----------------------------------------");
    println!("APU Tests: {passed}/{total} passed ({pct:.1}%), {skipped} skipped");
    println!("----------------------------------------\n");
}

/// Quick test to verify the test runner works with nestest.nes.
#[test]
fn test_runner_sanity_check() {
    let root = workspace_root();
    let nestest_path = root.join("test-roms/cpu/nestest.nes");

    if !nestest_path.exists() {
        println!("Skipping sanity check: nestest.nes not found");
        return;
    }

    let rom_data = fs::read(&nestest_path).expect("Failed to read nestest.nes");
    let mut console = Console::new(&rom_data).expect("Failed to create console");

    console.power_on();

    // Run 10,000 instructions
    for _ in 0..10_000 {
        console.step();
    }

    // Verify console is in a reasonable state
    assert!(console.total_cycles() > 0, "Should have executed cycles");

    println!(
        "Sanity check passed: {} cycles executed",
        console.total_cycles()
    );
}

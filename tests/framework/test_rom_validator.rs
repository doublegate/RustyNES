//! Standalone test ROM validator for RustyNES
//!
//! This program loads and attempts to run all test ROMs, generating a comprehensive report.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone)]
struct TestResult {
    rom_name: String,
    category: String,
    status: TestStatus,
    execution_time_ms: u64,
    rom_size: usize,
    error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum TestStatus {
    CanLoad,
    LoadError,
}

impl TestStatus {
    fn as_str(&self) -> &str {
        match self {
            TestStatus::CanLoad => "CAN_LOAD",
            TestStatus::LoadError => "LOAD_ERROR",
        }
    }

    fn emoji(&self) -> &str {
        match self {
            TestStatus::CanLoad => "✓",
            TestStatus::LoadError => "✗",
        }
    }
}

fn test_rom_loading(rom_path: &Path, category: &str) -> TestResult {
    let rom_name = rom_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let start_time = Instant::now();

    // Try to load ROM file
    let result = fs::read(rom_path);

    let (status, rom_size, error_message) = match result {
        Ok(data) => {
            // Successfully read file, check iNES header
            if data.len() < 16 {
                (
                    TestStatus::LoadError,
                    data.len(),
                    Some("File too small to be valid iNES ROM".to_string()),
                )
            } else if &data[0..4] != b"NES\x1A" {
                (
                    TestStatus::LoadError,
                    data.len(),
                    Some("Invalid iNES header magic".to_string()),
                )
            } else {
                (TestStatus::CanLoad, data.len(), None)
            }
        }
        Err(e) => (
            TestStatus::LoadError,
            0,
            Some(format!("Failed to read file: {}", e)),
        ),
    };

    let execution_time_ms = start_time.elapsed().as_millis() as u64;

    TestResult {
        rom_name,
        category: category.to_string(),
        status,
        execution_time_ms,
        rom_size,
        error_message,
    }
}

fn find_test_roms(base_dir: &Path, category: &str) -> Vec<PathBuf> {
    let category_dir = base_dir.join(category);
    let mut roms = Vec::new();

    fn scan_directory(dir: &Path, roms: &mut Vec<PathBuf>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();

                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("nes") {
                    roms.push(path);
                } else if path.is_dir() {
                    // Recursively search subdirectories
                    scan_directory(&path, roms);
                }
            }
        }
    }

    scan_directory(&category_dir, &mut roms);
    roms.sort();
    roms
}

fn generate_report(results_by_category: &HashMap<String, Vec<TestResult>>) -> String {
    let mut report = String::new();

    // Header
    report.push_str("# RustyNES Comprehensive Test ROM Validation Results\n\n");
    report.push_str(&format!(
        "**Generated**: {}\n",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    ));
    report.push_str("**RustyNES Version**: v0.4.0 (Milestone 5 Complete)\n\n");

    report.push_str("---\n\n");

    // Calculate totals
    let mut total_roms = 0;
    let mut total_can_load = 0;
    let mut total_load_error = 0;

    for results in results_by_category.values() {
        total_roms += results.len();
        total_can_load += results
            .iter()
            .filter(|r| r.status == TestStatus::CanLoad)
            .count();
        total_load_error += results
            .iter()
            .filter(|r| r.status == TestStatus::LoadError)
            .count();
    }

    // Executive Summary
    report.push_str("## Executive Summary\n\n");
    report.push_str("| Metric | Count | Percentage |\n");
    report.push_str("|--------|-------|------------|\n");
    report.push_str(&format!("| **Total Test ROMs** | {} | 100.0% |\n", total_roms));
    report.push_str(&format!(
        "| Valid ROMs (Can Load) | {} | {:.1}% |\n",
        total_can_load,
        (total_can_load as f64 / total_roms as f64) * 100.0
    ));
    report.push_str(&format!(
        "| Invalid ROMs | {} | {:.1}% |\n\n",
        total_load_error,
        (total_load_error as f64 / total_roms as f64) * 100.0
    ));

    report.push_str("**Note**: This validation performs ROM file loading checks only. ");
    report.push_str("Actual emulation testing requires integration with RustyNES core.\n\n");

    report.push_str("---\n\n");

    // Category breakdown
    report.push_str("## Detailed Results by Category\n\n");

    for category in &["cpu", "ppu", "apu", "mappers"] {
        if let Some(results) = results_by_category.get(*category) {
            if results.is_empty() {
                continue;
            }

            let cat_total = results.len();
            let cat_can_load = results
                .iter()
                .filter(|r| r.status == TestStatus::CanLoad)
                .count();
            let cat_load_error = results
                .iter()
                .filter(|r| r.status == TestStatus::LoadError)
                .count();

            let load_rate = if cat_total > 0 {
                (cat_can_load as f64 / cat_total as f64) * 100.0
            } else {
                0.0
            };

            report.push_str(&format!("### {} Tests\n\n", category.to_uppercase()));
            report.push_str(&format!("**Total**: {} ROMs\n", cat_total));
            report.push_str(&format!("**Load Success Rate**: {:.1}%\n\n", load_rate));

            report.push_str("| Status | Count |\n");
            report.push_str("|--------|-------|\n");
            report.push_str(&format!("| Can Load | {} |\n", cat_can_load));
            report.push_str(&format!("| Load Error | {} |\n\n", cat_load_error));

            // Detailed test list
            report.push_str(&format!(
                "#### Detailed {} Test Results\n\n",
                category.to_uppercase()
            ));
            report.push_str("| Test ROM | Status | Size (KB) | Time (ms) | Notes |\n");
            report.push_str("|----------|--------|-----------|-----------|-------|\n");

            for result in results {
                let size_kb = (result.rom_size as f64 / 1024.0).ceil() as usize;

                let notes = result
                    .error_message
                    .as_ref()
                    .map(|m| {
                        // Truncate long error messages
                        if m.len() > 60 {
                            format!("{}...", &m[..57])
                        } else {
                            m.clone()
                        }
                    })
                    .unwrap_or_default();

                report.push_str(&format!(
                    "| {} | {} {} | {} | {} | {} |\n",
                    result.rom_name,
                    result.status.emoji(),
                    result.status.as_str(),
                    size_kb,
                    result.execution_time_ms,
                    notes
                ));
            }

            report.push_str("\n");
        }
    }

    report.push_str("---\n\n");

    // Failure Analysis
    report.push_str("## Load Failure Analysis\n\n");

    let mut all_failures = Vec::new();
    for results in results_by_category.values() {
        all_failures.extend(
            results
                .iter()
                .filter(|r| r.status == TestStatus::LoadError)
                .cloned(),
        );
    }

    if all_failures.is_empty() {
        report.push_str("✓ All test ROM files have valid iNES headers and can be loaded.\n\n");
    } else {
        report.push_str(&format!("**Total Load Failures**: {}\n\n", all_failures.len()));

        for failure in &all_failures {
            report.push_str(&format!(
                "- **{}** ({}): {}\n",
                failure.rom_name,
                failure.category,
                failure
                    .error_message
                    .as_ref()
                    .unwrap_or(&"Unknown error".to_string())
            ));
        }

        report.push_str("\n");
    }

    report.push_str("---\n\n");

    // Test ROM Statistics
    report.push_str("## Test ROM Statistics\n\n");

    for category in &["cpu", "ppu", "apu", "mappers"] {
        if let Some(results) = results_by_category.get(*category) {
            if results.is_empty() {
                continue;
            }

            let total_size: usize = results.iter().map(|r| r.rom_size).sum();
            let total_size_mb = (total_size as f64 / 1024.0 / 1024.0);
            let avg_size_kb = (total_size as f64 / results.len() as f64 / 1024.0);

            report.push_str(&format!(
                "- **{}**: {} ROMs, {:.2} MB total, {:.1} KB average\n",
                category.to_uppercase(),
                results.len(),
                total_size_mb,
                avg_size_kb
            ));
        }
    }

    report.push_str("\n");

    report.push_str("---\n\n");

    // Next Steps
    report.push_str("## Next Steps for Full Validation\n\n");
    report.push_str("This report validates ROM file integrity only. ");
    report.push_str("Complete test ROM validation requires:\n\n");
    report.push_str("1. **Integration Test Framework**: Create test harness in RustyNES core\n");
    report.push_str("2. **ROM Execution**: Load ROMs through Console::from_rom_bytes()\n");
    report.push_str("3. **Result Checking**: Read $6000 memory location for pass/fail status\n");
    report.push_str(
        "4. **Timeout Handling**: Set appropriate frame limits (typically 600 frames = 10 seconds)\n",
    );
    report.push_str("5. **Categorized Testing**: Test by priority (P0 critical, P1 important, P2 edge cases)\n");
    report.push_str("6. **CI/CD Integration**: Add passing tests to GitHub Actions workflow\n\n");

    report.push_str("### Recommended Test Priority Order\n\n");
    report.push_str("1. **CPU Tests (Priority: Critical)**\n");
    report.push_str("   - Start with: cpu_nestest.nes (already passing)\n");
    report.push_str("   - Add: cpu_instr_*.nes (instruction validation)\n");
    report.push_str("   - Then: cpu_timing_*.nes (timing validation)\n\n");

    report.push_str("2. **PPU Tests (Priority: High)**\n");
    report.push_str("   - Start with: ppu_vbl_nmi.nes (already integrated)\n");
    report.push_str("   - Add: ppu_01.basics.nes through ppu_08.double_height.nes\n");
    report.push_str("   - Then: sprite hit and overflow tests\n\n");

    report.push_str("3. **APU Tests (Priority: Medium)**\n");
    report.push_str("   - Start with: apu_test_1.nes through apu_test_10.nes\n");
    report.push_str("   - Add: channel-specific tests\n");
    report.push_str("   - Then: DMC and timing tests\n\n");

    report.push_str("4. **Mapper Tests (Priority: Medium)**\n");
    report.push_str("   - Start with: mapper_nrom_*.nes (Mapper 0)\n");
    report.push_str("   - Add: mapper_mmc1_*.nes (Mapper 1)\n");
    report.push_str("   - Then: mapper_mmc3_*.nes (Mapper 4)\n\n");

    report.push_str("---\n\n");

    // Appendix
    report.push_str("## Appendix: Test ROM Sources\n\n");
    report.push_str("Test ROMs sourced from:\n\n");
    report.push_str("- **NESdev Wiki**: https://www.nesdev.org/wiki/Emulator_tests\n");
    report.push_str("- **christopherpow/nes-test-roms**: https://github.com/christopherpow/nes-test-roms\n");
    report.push_str("- **Blargg's Test Suite**: http://blargg.8bitalley.com/parodius/nes-tests/\n");
    report.push_str("- **TASVideos Accuracy Tests**: https://tasvideos.org/EmulatorResources/NESAccuracyTests\n\n");

    report.push_str("---\n\n");
    report.push_str("**End of Report**\n");

    report
}

fn main() {
    println!("\n=== RustyNES Test ROM Validator ===\n");

    let test_rom_dir = PathBuf::from("/home/parobek/Code/RustyNES/test-roms");

    if !test_rom_dir.exists() {
        eprintln!("Error: Test ROM directory not found: {:?}", test_rom_dir);
        eprintln!("Please ensure test ROMs are placed in the test-roms/ directory.");
        std::process::exit(1);
    }

    println!("Test ROM Directory: {:?}\n", test_rom_dir);

    let categories = vec!["cpu", "ppu", "apu", "mappers"];
    let mut results_by_category: HashMap<String, Vec<TestResult>> = HashMap::new();

    for category in &categories {
        println!("=== Scanning {} ROMs ===", category.to_uppercase());

        let roms = find_test_roms(&test_rom_dir, category);
        println!("Found {} ROMs in {} category\n", roms.len(), category);

        let mut category_results = Vec::new();

        for rom_path in &roms {
            let result = test_rom_loading(rom_path, category);
            println!(
                "  {} {} - {} ({} KB, {} ms)",
                result.status.emoji(),
                result.rom_name,
                result.status.as_str(),
                (result.rom_size as f64 / 1024.0).ceil() as usize,
                result.execution_time_ms
            );

            if let Some(ref err) = result.error_message {
                println!("      Error: {}", err);
            }

            category_results.push(result);
        }

        println!();
        results_by_category.insert(category.to_string(), category_results);
    }

    // Generate report
    println!("=== Generating Report ===\n");

    let report = generate_report(&results_by_category);

    // Write report to file
    let report_path = PathBuf::from("/tmp/RustyNES/TEST_ROM_RESULTS.md");

    if let Some(parent) = report_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    match fs::write(&report_path, &report) {
        Ok(_) => {
            println!("✓ Report written to: {:?}", report_path);
        }
        Err(e) => {
            eprintln!("✗ Failed to write report: {}", e);
            println!("\n{}", report);
        }
    }

    println!("\n=== Validation Complete ===\n");
}

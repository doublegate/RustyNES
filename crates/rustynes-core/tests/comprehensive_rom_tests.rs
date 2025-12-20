//! Comprehensive test ROM validation for all categories
//!
//! This integration test runs ALL test ROMs in test-roms/ directory:
//! - CPU tests (36 ROMs)
//! - PPU tests (49 ROMs)
//! - APU tests (70 ROMs)
//! - Mapper tests (57 ROMs)
//!
//! Results are written to /tmp/RustyNES/TEST_ROM_RESULTS.md

#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::format_push_string)]

use rustynes_core::Console;
use std::collections::HashMap;
use std::fs;
use std::panic;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone)]
struct TestResult {
    rom_name: String,
    category: String,
    status: TestStatus,
    execution_time_ms: u64,
    error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum TestStatus {
    Pass,
    Fail,
    Timeout,
    LoadError,
    NotImplemented, // For mappers/features not yet implemented
}

impl TestStatus {
    fn as_str(&self) -> &str {
        match self {
            TestStatus::Pass => "PASS",
            TestStatus::Fail => "FAIL",
            TestStatus::Timeout => "TIMEOUT",
            TestStatus::LoadError => "LOAD_ERROR",
            TestStatus::NotImplemented => "NOT_IMPLEMENTED",
        }
    }

    fn emoji(&self) -> &str {
        match self {
            TestStatus::Pass => "✓",
            TestStatus::Fail => "✗",
            TestStatus::Timeout => "⏱",
            TestStatus::LoadError => "⚠",
            TestStatus::NotImplemented => "○",
        }
    }
}

fn run_single_test_rom(rom_path: &Path, category: &str) -> TestResult {
    let rom_name = rom_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let start_time = Instant::now();

    // Try to load ROM
    let rom_data = match fs::read(rom_path) {
        Ok(data) => data,
        Err(e) => {
            return TestResult {
                rom_name,
                category: category.to_string(),
                status: TestStatus::LoadError,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
                error_message: Some(format!("Failed to read ROM file: {e}")),
            };
        }
    };

    // Try to create console (with panic recovery)
    let mut console = match panic::catch_unwind(|| Console::from_rom_bytes(&rom_data)) {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            let error_str = e.to_string();
            let status = if error_str.contains("Mapper") || error_str.contains("mapper") {
                TestStatus::NotImplemented
            } else {
                TestStatus::LoadError
            };

            return TestResult {
                rom_name,
                category: category.to_string(),
                status,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
                error_message: Some(error_str),
            };
        }
        Err(panic_info) => {
            // Panic occurred during ROM loading
            let panic_msg = if let Some(s) = panic_info.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                (*s).to_string()
            } else {
                "Unknown panic during ROM loading".to_string()
            };

            return TestResult {
                rom_name,
                category: category.to_string(),
                status: TestStatus::LoadError,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
                error_message: Some(format!("Panic: {panic_msg}")),
            };
        }
    };

    // Run the ROM and check for completion (with panic recovery)
    // Most test ROMs write result code to $6000 (0x00 = pass, non-zero = fail)
    let execution_result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        let max_frames = 300; // Run for max 10 seconds at 30 FPS
        let mut status = TestStatus::Timeout;
        let mut error_message = None;

        for frame in 0..max_frames {
            // Run one frame
            console.step_frame();

            // Check result code at $6000
            let result_code = console.peek_memory(0x6000);

            // Check if test completed (non-0xFF value usually means complete)
            if result_code != 0xFF && frame > 10 {
                // Give ROM 10 frames to initialize
                if result_code == 0x00 {
                    status = TestStatus::Pass;
                } else {
                    status = TestStatus::Fail;
                    error_message =
                        Some(format!("Test failed with error code: 0x{result_code:02X}"));
                }
                break;
            }

            // Safety check: if we've been running for too long, it's a timeout
            if frame == max_frames - 1 {
                // Check final state
                let final_code = console.peek_memory(0x6000);
                if final_code == 0x00 {
                    status = TestStatus::Pass;
                } else if final_code != 0xFF {
                    status = TestStatus::Fail;
                    error_message = Some(format!(
                        "Test did not complete, final code: 0x{final_code:02X}"
                    ));
                } else {
                    status = TestStatus::Timeout;
                    error_message = Some("Test did not complete within timeout period".to_string());
                }
            }
        }

        (status, error_message)
    }));

    let execution_time_ms = start_time.elapsed().as_millis() as u64;

    let (status, error_message) = match execution_result {
        Ok((s, e)) => (s, e),
        Err(panic_info) => {
            // Panic occurred during execution
            let panic_msg = if let Some(s) = panic_info.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                (*s).to_string()
            } else {
                "Unknown panic during execution".to_string()
            };

            (
                TestStatus::Fail,
                Some(format!("Runtime panic: {panic_msg}")),
            )
        }
    };

    TestResult {
        rom_name,
        category: category.to_string(),
        status,
        execution_time_ms,
        error_message,
    }
}

fn find_test_roms(base_dir: &Path, category: &str) -> Vec<PathBuf> {
    let category_dir = base_dir.join(category);
    let mut roms = Vec::new();

    if let Ok(entries) = fs::read_dir(&category_dir) {
        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("nes") {
                roms.push(path);
            } else if path.is_dir() {
                // Recursively search subdirectories (for apu_test/rom_singles/, etc.)
                if let Ok(sub_entries) = fs::read_dir(&path) {
                    for sub_entry in sub_entries.flatten() {
                        let sub_path = sub_entry.path();
                        if sub_path.is_file()
                            && sub_path.extension().and_then(|s| s.to_str()) == Some("nes")
                        {
                            roms.push(sub_path);
                        }
                    }
                }
            }
        }
    }

    roms.sort();
    roms
}

fn generate_report(results_by_category: &HashMap<String, Vec<TestResult>>) -> String {
    let mut report = String::new();

    // Header
    report.push_str("# RustyNES Comprehensive Test ROM Validation Results\n\n");
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    report.push_str(&format!("**Generated**: {timestamp}\n"));
    report.push_str("**RustyNES Version**: v0.4.0 (Milestone 5 Complete)\n\n");

    // Calculate totals
    let mut total_roms = 0;
    let mut total_pass = 0;
    let mut total_fail = 0;
    let mut total_timeout = 0;
    let mut total_load_error = 0;
    let mut total_not_implemented = 0;

    for results in results_by_category.values() {
        total_roms += results.len();
        total_pass += results
            .iter()
            .filter(|r| r.status == TestStatus::Pass)
            .count();
        total_fail += results
            .iter()
            .filter(|r| r.status == TestStatus::Fail)
            .count();
        total_timeout += results
            .iter()
            .filter(|r| r.status == TestStatus::Timeout)
            .count();
        total_load_error += results
            .iter()
            .filter(|r| r.status == TestStatus::LoadError)
            .count();
        total_not_implemented += results
            .iter()
            .filter(|r| r.status == TestStatus::NotImplemented)
            .count();
    }

    // Executive Summary
    report.push_str("## Executive Summary\n\n");
    report.push_str("| Metric | Count | Percentage |\n");
    report.push_str("|--------|-------|------------|\n");
    report.push_str(&format!(
        "| **Total Test ROMs** | {total_roms} | 100.0% |\n"
    ));

    let pass_pct = (total_pass as f64 / total_roms as f64) * 100.0;
    let fail_pct = (total_fail as f64 / total_roms as f64) * 100.0;
    let timeout_pct = (total_timeout as f64 / total_roms as f64) * 100.0;
    let load_err_pct = (total_load_error as f64 / total_roms as f64) * 100.0;
    let not_impl_pct = (total_not_implemented as f64 / total_roms as f64) * 100.0;

    report.push_str(&format!("| Passed | {total_pass} | {pass_pct:.1}% |\n"));
    report.push_str(&format!("| Failed | {total_fail} | {fail_pct:.1}% |\n"));
    report.push_str(&format!(
        "| Timeout | {total_timeout} | {timeout_pct:.1}% |\n"
    ));
    report.push_str(&format!(
        "| Load Error | {total_load_error} | {load_err_pct:.1}% |\n"
    ));
    report.push_str(&format!(
        "| Not Implemented | {total_not_implemented} | {not_impl_pct:.1}% |\n\n"
    ));

    // Category breakdown
    report.push_str("## Detailed Results by Category\n\n");

    for category in &["cpu", "ppu", "apu", "mappers"] {
        if let Some(results) = results_by_category.get(*category) {
            let cat_total = results.len();
            let cat_pass = results
                .iter()
                .filter(|r| r.status == TestStatus::Pass)
                .count();
            let cat_fail = results
                .iter()
                .filter(|r| r.status == TestStatus::Fail)
                .count();
            let cat_timeout = results
                .iter()
                .filter(|r| r.status == TestStatus::Timeout)
                .count();
            let cat_load_error = results
                .iter()
                .filter(|r| r.status == TestStatus::LoadError)
                .count();
            let cat_not_impl = results
                .iter()
                .filter(|r| r.status == TestStatus::NotImplemented)
                .count();

            let pass_rate = if cat_total > 0 {
                (cat_pass as f64 / cat_total as f64) * 100.0
            } else {
                0.0
            };

            let cat_upper = category.to_uppercase();
            report.push_str(&format!("### {cat_upper} Tests\n\n"));
            report.push_str(&format!("**Total**: {cat_total} ROMs\n"));
            report.push_str(&format!("**Pass Rate**: {pass_rate:.1}%\n\n"));

            report.push_str("| Status | Count |\n");
            report.push_str("|--------|-------|\n");
            report.push_str(&format!("| Pass | {cat_pass} |\n"));
            report.push_str(&format!("| Fail | {cat_fail} |\n"));
            report.push_str(&format!("| Timeout | {cat_timeout} |\n"));
            report.push_str(&format!("| Load Error | {cat_load_error} |\n"));
            report.push_str(&format!("| Not Implemented | {cat_not_impl} |\n\n"));

            // Detailed test list
            report.push_str(&format!("#### Detailed {cat_upper} Test Results\n\n"));
            report.push_str("| Test ROM | Status | Time (ms) | Notes |\n");
            report.push_str("|----------|--------|-----------|-------|\n");

            for result in results {
                let notes = result
                    .error_message
                    .as_ref()
                    .map(|m| {
                        // Truncate long error messages
                        if m.len() > 80 {
                            format!("{}...", &m[..77])
                        } else {
                            m.clone()
                        }
                    })
                    .unwrap_or_default();

                let emoji = result.status.emoji();
                let status_str = result.status.as_str();
                let rom = &result.rom_name;
                let time = result.execution_time_ms;
                report.push_str(&format!(
                    "| {rom} | {emoji} {status_str} | {time} | {notes} |\n"
                ));
            }

            report.push('\n');
        }
    }

    // Failure Analysis
    report.push_str("## Failure Analysis\n\n");

    let mut all_failures = Vec::new();
    for results in results_by_category.values() {
        all_failures.extend(
            results
                .iter()
                .filter(|r| r.status == TestStatus::Fail || r.status == TestStatus::LoadError)
                .cloned(),
        );
    }

    if all_failures.is_empty() {
        report.push_str("No critical failures detected (only timeouts and not-implemented).\n\n");
    } else {
        let failure_count = all_failures.len();
        report.push_str(&format!("**Total Failures**: {failure_count}\n\n"));

        // Group by error type
        let mut error_groups: HashMap<String, Vec<TestResult>> = HashMap::new();

        for failure in &all_failures {
            if let Some(ref msg) = failure.error_message {
                // Extract error type
                let error_type = if msg.contains("Mapper") {
                    "Unsupported Mapper"
                } else if msg.contains("ROM") {
                    "ROM Format Error"
                } else {
                    "Other Error"
                };

                error_groups
                    .entry(error_type.to_string())
                    .or_default()
                    .push(failure.clone());
            }
        }

        for (error_type, failures) in error_groups {
            let count = failures.len();
            report.push_str(&format!("### {error_type}\n\n"));
            report.push_str(&format!("**Count**: {count}\n\n"));

            for failure in failures {
                let rom = &failure.rom_name;
                let cat = &failure.category;
                let err = failure
                    .error_message
                    .as_ref()
                    .map_or("Unknown error", |s| s.as_str());
                report.push_str(&format!("- **{rom}** ({cat}): {err}\n"));
            }

            report.push('\n');
        }
    }

    // Recommendations
    report.push_str("## Recommendations\n\n");

    let implemented_total = total_roms - total_not_implemented;
    let implemented_pass = total_pass;

    if implemented_total > 0 {
        let impl_pass_rate = (implemented_pass as f64 / implemented_total as f64) * 100.0;
        report.push_str(&format!(
            "- **Implemented ROMs Pass Rate**: {impl_pass_rate:.1}% ({implemented_pass}/{implemented_total})\n"
        ));
    }

    if total_timeout > 0 {
        report.push_str(&format!(
            "- **Action Required**: {total_timeout} test ROMs timed out - need to implement result checking mechanism\n"
        ));
    }

    if total_not_implemented > 0 {
        report.push_str(&format!(
            "- **Future Work**: {total_not_implemented} test ROMs require unimplemented mappers (Phase 3 feature)\n"
        ));
    }

    report.push_str("\n## Next Steps\n\n");
    report.push_str("1. Implement Console::read_memory() method to check $6000 result code\n");
    report.push_str("2. Add actual ROM execution and result validation\n");
    report.push_str("3. Integrate passing test ROMs into CI/CD pipeline\n");
    report.push_str("4. Prioritize fixing timeout cases\n");
    report.push_str("5. Plan Phase 3 mapper implementations based on test requirements\n");

    report
}

#[test]
fn comprehensive_test_rom_validation() {
    // Path to test-roms is at workspace root, not crate root
    let test_rom_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..") // crates/
        .join("..") // workspace root
        .join("test-roms");

    println!("\n=== RustyNES Comprehensive Test ROM Validation ===\n");
    println!("Test ROM Directory: {test_rom_dir:?}\n");

    // Skip if test-roms directory doesn't exist
    if !test_rom_dir.exists() {
        eprintln!("Test ROM directory not found: {test_rom_dir:?}");
        eprintln!("Skipping comprehensive ROM validation.");
        return;
    }

    let categories = vec!["cpu", "ppu", "apu", "mappers"];
    let mut results_by_category: HashMap<String, Vec<TestResult>> = HashMap::new();

    for category in &categories {
        let cat_upper = category.to_uppercase();
        println!("=== Testing {cat_upper} ROMs ===");

        let roms = find_test_roms(&test_rom_dir, category);
        let rom_count = roms.len();
        println!("Found {rom_count} ROMs in {category} category\n");

        let mut category_results = Vec::new();

        for rom_path in &roms {
            let result = run_single_test_rom(rom_path, category);
            let emoji = result.status.emoji();
            let name = &result.rom_name;
            let status = result.status.as_str();
            let time = result.execution_time_ms;
            println!("  {emoji} {name} - {status} ({time} ms)");

            if let Some(ref err) = result.error_message {
                println!("      Error: {err}");
            }

            category_results.push(result);
        }

        println!();
        results_by_category.insert((*category).to_string(), category_results);
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
        Ok(()) => {
            println!("Report written to: {report_path:?}");
        }
        Err(e) => {
            eprintln!("Failed to write report: {e}");
            println!("Report output:\n{report}");
        }
    }

    println!("\n=== Validation Complete ===\n");
}

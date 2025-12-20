//! Enhanced test ROM validator with actual Console loading
//!
//! This validator:
//! 1. Validates iNES header format
//! 2. Attempts to load ROM through Console::from_rom_bytes()
//! 3. Categorizes failures (LoadError, UnsupportedMapper, etc.)
//! 4. Generates comprehensive report

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

// We need to add rustynes-core to dependencies
// For standalone compilation, we'll use dynamic library approach
#[path = "/home/parobek/Code/RustyNES/crates/rustynes-core/src/lib.rs"]
mod rustynes_core;

use rustynes_core::Console;

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
    UnsupportedMapper,
}

impl TestStatus {
    fn as_str(&self) -> &str {
        match self {
            TestStatus::CanLoad => "CAN_LOAD",
            TestStatus::LoadError => "LOAD_ERROR",
            TestStatus::UnsupportedMapper => "UNSUPPORTED_MAPPER",
        }
    }

    fn emoji(&self) -> &str {
        match self {
            TestStatus::CanLoad => "✓",
            TestStatus::LoadError => "✗",
            TestStatus::UnsupportedMapper => "○",
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
    let rom_data = match fs::read(rom_path) {
        Ok(data) => data,
        Err(e) => {
            return TestResult {
                rom_name,
                category: category.to_string(),
                status: TestStatus::LoadError,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
                rom_size: 0,
                error_message: Some(format!("Failed to read file: {}", e)),
            };
        }
    };

    let rom_size = rom_data.len();

    // Validate iNES header
    if rom_data.len() < 16 {
        return TestResult {
            rom_name,
            category: category.to_string(),
            status: TestStatus::LoadError,
            execution_time_ms: start_time.elapsed().as_millis() as u64,
            rom_size,
            error_message: Some("File too small to be valid iNES ROM".to_string()),
        };
    }

    if &rom_data[0..4] != b"NES\x1A" {
        return TestResult {
            rom_name,
            category: category.to_string(),
            status: TestStatus::LoadError,
            execution_time_ms: start_time.elapsed().as_millis() as u64,
            rom_size,
            error_message: Some("Invalid iNES header magic".to_string()),
        };
    }

    // Try to load through Console
    match Console::from_rom_bytes(&rom_data) {
        Ok(_console) => {
            let execution_time_ms = start_time.elapsed().as_millis() as u64;
            TestResult {
                rom_name,
                category: category.to_string(),
                status: TestStatus::CanLoad,
                execution_time_ms,
                rom_size,
                error_message: None,
            }
        }
        Err(e) => {
            let execution_time_ms = start_time.elapsed().as_millis() as u64;
            let error_str = e.to_string();

            // Categorize error
            let status = if error_str.contains("Mapper") || error_str.contains("mapper") {
                TestStatus::UnsupportedMapper
            } else {
                TestStatus::LoadError
            };

            TestResult {
                rom_name,
                category: category.to_string(),
                status,
                execution_time_ms,
                rom_size,
                error_message: Some(error_str),
            }
        }
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
    report.push_str("# RustyNES Enhanced Test ROM Validation Results\n\n");
    report.push_str(&format!(
        "**Generated**: {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    ));
    report.push_str("**RustyNES Version**: v0.4.0 (Milestone 5 Complete)\n");
    report.push_str("**Validation Method**: iNES header check + Console::from_rom_bytes()\n\n");

    report.push_str("---\n\n");

    // Calculate totals
    let mut total_roms = 0;
    let mut total_can_load = 0;
    let mut total_load_error = 0;
    let mut total_unsupported_mapper = 0;

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
        total_unsupported_mapper += results
            .iter()
            .filter(|r| r.status == TestStatus::UnsupportedMapper)
            .count();
    }

    // Executive Summary
    report.push_str("## Executive Summary\n\n");
    report.push_str("| Metric | Count | Percentage |\n");
    report.push_str("|--------|-------|------------|\n");
    report.push_str(&format!("| **Total Test ROMs** | {} | 100.0% |\n", total_roms));
    report.push_str(&format!(
        "| ROMs That Load Successfully | {} | {:.1}% |\n",
        total_can_load,
        (total_can_load as f64 / total_roms as f64) * 100.0
    ));
    report.push_str(&format!(
        "| Unsupported Mappers | {} | {:.1}% |\n",
        total_unsupported_mapper,
        (total_unsupported_mapper as f64 / total_roms as f64) * 100.0
    ));
    report.push_str(&format!(
        "| Load Errors (Other) | {} | {:.1}% |\n\n",
        total_load_error,
        (total_load_error as f64 / total_roms as f64) * 100.0
    ));

    report.push_str("**Validation Level**: This report validates ROM loading through RustyNES Console. ");
    report.push_str("ROMs that load successfully can be executed, but pass/fail status requires ");
    report.push_str("running the ROM and checking result code at $6000.\n\n");

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
            let cat_unsupported = results
                .iter()
                .filter(|r| r.status == TestStatus::UnsupportedMapper)
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
            report.push_str(&format!("| Unsupported Mapper | {} |\n", cat_unsupported));
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

    // Group failures by type
    let mut mapper_failures = Vec::new();
    let mut other_failures = Vec::new();

    for results in results_by_category.values() {
        for result in results {
            match result.status {
                TestStatus::UnsupportedMapper => mapper_failures.push(result.clone()),
                TestStatus::LoadError => other_failures.push(result.clone()),
                _ => {}
            }
        }
    }

    if mapper_failures.is_empty() && other_failures.is_empty() {
        report.push_str("✓ All test ROM files loaded successfully through Console::from_rom_bytes().\n\n");
    } else {
        // Unsupported mappers
        if !mapper_failures.is_empty() {
            report.push_str(&format!(
                "### Unsupported Mappers ({} ROMs)\n\n",
                mapper_failures.len()
            ));

            // Extract mapper numbers
            let mut mapper_map: HashMap<String, Vec<String>> = HashMap::new();
            for failure in &mapper_failures {
                if let Some(ref msg) = failure.error_message {
                    let mapper_info = if msg.contains("Mapper") {
                        msg.clone()
                    } else {
                        "Unknown mapper".to_string()
                    };
                    mapper_map
                        .entry(mapper_info)
                        .or_default()
                        .push(failure.rom_name.clone());
                }
            }

            for (mapper, roms) in mapper_map {
                report.push_str(&format!("**{}**:\n", mapper));
                for rom in roms {
                    report.push_str(&format!("- {}\n", rom));
                }
                report.push_str("\n");
            }
        }

        // Other load errors
        if !other_failures.is_empty() {
            report.push_str(&format!(
                "### Other Load Errors ({} ROMs)\n\n",
                other_failures.len()
            ));

            for failure in &other_failures {
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
    }

    report.push_str("---\n\n");

    // Implementation status
    report.push_str("## RustyNES Implementation Status\n\n");
    report.push_str("**Implemented Mappers**: 0 (NROM), 1 (MMC1), 2 (UxROM), 3 (CNROM), 4 (MMC3)\n\n");

    let loadable_roms = total_can_load;
    let total_with_impl_mappers = total_roms - total_unsupported_mapper;

    report.push_str(&format!(
        "**ROMs Compatible with Implemented Mappers**: {} / {} ({:.1}%)\n",
        total_with_impl_mappers,
        total_roms,
        (total_with_impl_mappers as f64 / total_roms as f64) * 100.0
    ));
    report.push_str(&format!(
        "**ROMs That Successfully Load**: {} / {} ({:.1}%)\n\n",
        loadable_roms,
        total_with_impl_mappers,
        if total_with_impl_mappers > 0 {
            (loadable_roms as f64 / total_with_impl_mappers as f64) * 100.0
        } else {
            0.0
        }
    ));

    report.push_str("---\n\n");

    // Next Steps
    report.push_str("## Next Steps for Full Validation\n\n");
    report.push_str("This report validates ROM loading capability. ");
    report.push_str("Complete test ROM validation requires:\n\n");
    report.push_str("1. **Execution Framework**: Run loaded ROMs through emulation\n");
    report.push_str("2. **Result Checking**: Read $6000 memory location for pass/fail status\n");
    report.push_str("   - 0x00 = All tests passed\n");
    report.push_str("   - 0x01+ = Specific test failed (number indicates which test)\n");
    report.push_str("3. **Timeout Handling**: Set frame limits (typically 600 frames = 10 seconds)\n");
    report.push_str("4. **Console API Enhancement**: Add Console::read_memory(addr: u16) -> u8\n");
    report.push_str("5. **CI/CD Integration**: Add passing tests to GitHub Actions workflow\n\n");

    report.push_str("### Recommended Priority Order\n\n");
    report.push_str("1. **Fix Load Errors**: Address any non-mapper load failures\n");
    report.push_str("2. **CPU Tests**: Start with nestest.nes (already passing)\n");
    report.push_str("3. **PPU Tests**: Focus on loadable PPU test ROMs\n");
    report.push_str("4. **APU Tests**: Test loadable APU ROMs\n");
    report.push_str("5. **Mapper Tests**: Validate mapper implementations\n");
    report.push_str("6. **Phase 3**: Implement additional mappers as needed\n\n");

    report.push_str("---\n\n");
    report.push_str("**End of Report**\n");

    report
}

fn main() {
    println!("\n=== RustyNES Enhanced Test ROM Validator ===\n");
    println!("Validation method: iNES header + Console::from_rom_bytes()\n");

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
                // Truncate long error messages for console output
                if err.len() > 80 {
                    println!("      Error: {}...", &err[..77]);
                } else {
                    println!("      Error: {}", err);
                }
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

//! ROM file system scanner.
//!
//! Discovers NES ROM files (.nes extension) in directories and extracts
//! basic metadata.

use std::fs;
use std::path::{Path, PathBuf};
use tracing::{error, warn};

/// ROM file entry with metadata
#[derive(Debug, Clone)]
pub struct RomEntry {
    /// Full path to ROM file
    pub path: PathBuf,
    /// Display title (derived from filename)
    pub title: String,
    /// File size in bytes
    pub size: u64,
    /// Optional mapper number (extracted from iNES header)
    pub mapper: Option<u16>,
}

impl RomEntry {
    /// Create ROM entry from file path
    ///
    /// Reads file metadata and attempts to extract mapper information
    /// from the iNES header.
    #[must_use]
    pub fn from_path(path: PathBuf) -> Self {
        // Extract title from filename
        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string();

        // Get file size
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

        // Try to extract mapper number from iNES header
        let mapper = Self::extract_mapper_number(&path);

        Self {
            path,
            title,
            size,
            mapper,
        }
    }

    /// Extract mapper number from iNES header
    ///
    /// Reads the first 16 bytes of the ROM file and parses the iNES header
    /// to extract the mapper number.
    fn extract_mapper_number(path: &Path) -> Option<u16> {
        // Read first 16 bytes (iNES header)
        let header = fs::read(path).ok()?;
        if header.len() < 16 {
            return None;
        }

        // Check for iNES magic number "NES\x1A"
        if &header[0..4] != b"NES\x1A" {
            return None;
        }

        // Extract mapper number (bits 4-7 of byte 6, bits 4-7 of byte 7)
        let mapper_low = (header[6] & 0xF0) >> 4;
        let mapper_high = header[7] & 0xF0;
        let mapper = u16::from(mapper_high | mapper_low);

        Some(mapper)
    }

    /// Format file size as human-readable string
    #[must_use]
    pub fn size_display(&self) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;

        if self.size >= MB {
            format!("{:.2} MB", self.size as f64 / MB as f64)
        } else if self.size >= KB {
            format!("{:.2} KB", self.size as f64 / KB as f64)
        } else {
            format!("{} B", self.size)
        }
    }

    /// Get mapper display string
    #[must_use]
    pub fn mapper_display(&self) -> String {
        match self.mapper {
            Some(mapper) => format!("Mapper {mapper}"),
            None => "Unknown".to_string(),
        }
    }
}

/// ROM scanner for discovering .nes files
pub struct RomScanner;

impl RomScanner {
    /// Scan directory for .nes files
    ///
    /// Recursively scans the directory (up to 1 level deep) and returns
    /// all discovered ROM files sorted alphabetically.
    ///
    /// # Arguments
    ///
    /// * `dir` - Directory to scan
    ///
    /// # Returns
    ///
    /// Vector of ROM entries, sorted by title
    #[must_use]
    pub fn scan_directory(dir: &Path) -> Vec<RomEntry> {
        let mut roms = Vec::new();

        if !dir.exists() {
            warn!("ROM directory does not exist: {}", dir.display());
            return roms;
        }

        if !dir.is_dir() {
            warn!("ROM path is not a directory: {}", dir.display());
            return roms;
        }

        Self::scan_recursive(dir, &mut roms, 0, 1);

        // Sort alphabetically by title
        roms.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));

        roms
    }

    /// Recursively scan directory
    ///
    /// # Arguments
    ///
    /// * `dir` - Directory to scan
    /// * `roms` - Vector to accumulate ROM entries
    /// * `depth` - Current recursion depth
    /// * `max_depth` - Maximum recursion depth
    fn scan_recursive(dir: &Path, roms: &mut Vec<RomEntry>, depth: usize, max_depth: usize) {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                error!("Failed to read directory {}: {}", dir.display(), e);
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_file() {
                // Check for .nes extension
                if let Some(ext) = path.extension() {
                    if ext.eq_ignore_ascii_case("nes") {
                        roms.push(RomEntry::from_path(path));
                    }
                }
            } else if path.is_dir() && depth < max_depth {
                // Recurse into subdirectory
                Self::scan_recursive(&path, roms, depth + 1, max_depth);
            }
        }
    }

    /// Quick scan for ROM count (doesn't extract metadata)
    ///
    /// Useful for showing progress indicators during large scans.
    #[must_use]
    #[allow(dead_code)] // Future: progress bar during library scanning
    pub fn count_roms(dir: &Path) -> usize {
        if !dir.is_dir() {
            return 0;
        }

        let mut count = 0;
        Self::count_recursive(dir, &mut count, 0, 1);
        count
    }

    /// Recursively count ROMs
    #[allow(dead_code)] // Future: used by count_roms for progress indicators
    fn count_recursive(dir: &Path, count: &mut usize, depth: usize, max_depth: usize) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();

                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext.eq_ignore_ascii_case("nes") {
                            *count += 1;
                        }
                    }
                } else if path.is_dir() && depth < max_depth {
                    Self::count_recursive(&path, count, depth + 1, max_depth);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_ines_rom(path: &Path, mapper: u8) {
        let mut header = vec![
            b'N',
            b'E',
            b'S',
            0x1A,                 // Magic
            0x02,                 // PRG ROM size (32KB)
            0x01,                 // CHR ROM size (8KB)
            (mapper << 4) | 0x01, // Mapper low nibble + mirroring
            mapper & 0xF0,        // Mapper high nibble
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
        ];

        // Add dummy PRG/CHR data
        header.extend_from_slice(&vec![0u8; 32 * 1024 + 8 * 1024]);

        fs::write(path, header).unwrap();
    }

    #[test]
    fn test_rom_entry_from_path() {
        let temp_dir = TempDir::new().unwrap();
        let rom_path = temp_dir.path().join("test_game.nes");

        // Create test ROM with mapper 1 (MMC1)
        create_test_ines_rom(&rom_path, 1);

        let entry = RomEntry::from_path(rom_path.clone());

        assert_eq!(entry.title, "test_game");
        assert!(entry.size > 0);
        assert_eq!(entry.mapper, Some(1));
    }

    #[test]
    fn test_rom_scanner_basic() {
        let temp_dir = TempDir::new().unwrap();

        // Create test ROMs
        create_test_ines_rom(&temp_dir.path().join("zelda.nes"), 1);
        create_test_ines_rom(&temp_dir.path().join("mario.nes"), 0);
        create_test_ines_rom(&temp_dir.path().join("metroid.nes"), 1);

        // Create non-ROM file
        fs::write(temp_dir.path().join("readme.txt"), b"test").unwrap();

        let roms = RomScanner::scan_directory(temp_dir.path());

        assert_eq!(roms.len(), 3);

        // Should be sorted alphabetically
        assert_eq!(roms[0].title, "mario");
        assert_eq!(roms[1].title, "metroid");
        assert_eq!(roms[2].title, "zelda");
    }

    #[test]
    fn test_rom_scanner_recursive() {
        let temp_dir = TempDir::new().unwrap();

        // Create ROMs in root
        create_test_ines_rom(&temp_dir.path().join("root.nes"), 0);

        // Create subdirectory with ROMs
        let sub_dir = temp_dir.path().join("subdir");
        fs::create_dir(&sub_dir).unwrap();
        create_test_ines_rom(&sub_dir.join("sub.nes"), 0);

        let roms = RomScanner::scan_directory(temp_dir.path());

        assert_eq!(roms.len(), 2);
    }

    #[test]
    fn test_size_display() {
        let mut entry = RomEntry {
            path: PathBuf::from("test.nes"),
            title: "Test".to_string(),
            size: 512,
            mapper: None,
        };

        assert_eq!(entry.size_display(), "512 B");

        entry.size = 2048;
        assert_eq!(entry.size_display(), "2.00 KB");

        entry.size = 1_048_576;
        assert_eq!(entry.size_display(), "1.00 MB");
    }

    #[test]
    fn test_mapper_display() {
        let entry = RomEntry {
            path: PathBuf::from("test.nes"),
            title: "Test".to_string(),
            size: 0,
            mapper: Some(4),
        };

        assert_eq!(entry.mapper_display(), "Mapper 4");

        let entry_no_mapper = RomEntry {
            path: PathBuf::from("test.nes"),
            title: "Test".to_string(),
            size: 0,
            mapper: None,
        };

        assert_eq!(entry_no_mapper.mapper_display(), "Unknown");
    }

    #[test]
    fn test_count_roms() {
        let temp_dir = TempDir::new().unwrap();

        create_test_ines_rom(&temp_dir.path().join("game1.nes"), 0);
        create_test_ines_rom(&temp_dir.path().join("game2.nes"), 0);

        let count = RomScanner::count_roms(temp_dir.path());
        assert_eq!(count, 2);
    }
}

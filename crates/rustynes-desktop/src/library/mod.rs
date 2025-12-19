//! ROM library management.
//!
//! Provides ROM file discovery, metadata extraction, and organization
//! for the game library browser.

pub mod scanner;

use scanner::{RomEntry, RomScanner};
use std::path::{Path, PathBuf};

/// ROM library state
#[derive(Debug, Clone)]
pub struct LibraryState {
    /// All discovered ROM entries
    pub roms: Vec<RomEntry>,
    /// Current ROM directory being scanned
    pub rom_directory: Option<PathBuf>,
    /// Current view mode
    pub view_mode: ViewMode,
    /// Search query filter
    pub search_query: String,
    /// Sort order
    pub sort_order: SortOrder,
}

/// Library view mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Grid view with cover art placeholders
    Grid,
    /// List view with metadata columns
    List,
}

/// ROM sort order
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Future: all sort orders will be available in sort dropdown
pub enum SortOrder {
    /// Sort by title (A-Z)
    TitleAsc,
    /// Sort by title (Z-A)
    TitleDesc,
    /// Sort by file size (smallest first)
    SizeAsc,
    /// Sort by file size (largest first)
    SizeDesc,
    /// Sort by recently added (newest first)
    RecentlyAdded,
}

impl LibraryState {
    /// Create new empty library state
    #[must_use]
    pub fn new() -> Self {
        Self {
            roms: Vec::new(),
            rom_directory: None,
            view_mode: ViewMode::Grid,
            search_query: String::new(),
            sort_order: SortOrder::TitleAsc,
        }
    }

    /// Scan directory for ROM files
    ///
    /// This replaces the current ROM list with newly discovered ROMs.
    pub fn scan_directory(&mut self, dir: &Path) {
        self.rom_directory = Some(dir.to_path_buf());
        self.roms = RomScanner::scan_directory(dir);
        self.apply_sort();

        tracing::info!("Scanned {} ROMs from {}", self.roms.len(), dir.display());
    }

    /// Rescan current directory
    #[allow(dead_code)] // Future: refresh button in library view
    pub fn rescan(&mut self) {
        if let Some(dir) = self.rom_directory.clone() {
            self.scan_directory(&dir);
        }
    }

    /// Get filtered ROMs based on search query
    #[must_use]
    pub fn filtered_roms(&self) -> Vec<&RomEntry> {
        if self.search_query.is_empty() {
            self.roms.iter().collect()
        } else {
            let query_lower = self.search_query.to_lowercase();
            self.roms
                .iter()
                .filter(|rom| rom.title.to_lowercase().contains(&query_lower))
                .collect()
        }
    }

    /// Set search query
    pub fn set_search_query(&mut self, query: String) {
        self.search_query = query;
    }

    /// Clear search query
    #[allow(dead_code)] // Future: clear button in search bar
    pub fn clear_search(&mut self) {
        self.search_query.clear();
    }

    /// Toggle between grid and list view
    pub fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Grid => ViewMode::List,
            ViewMode::List => ViewMode::Grid,
        };
    }

    /// Set sort order and re-sort
    #[allow(dead_code)] // Future: sort dropdown in library view
    pub fn set_sort_order(&mut self, order: SortOrder) {
        self.sort_order = order;
        self.apply_sort();
    }

    /// Apply current sort order to ROM list
    fn apply_sort(&mut self) {
        match self.sort_order {
            SortOrder::TitleAsc => {
                self.roms.sort_by(|a, b| a.title.cmp(&b.title));
            }
            SortOrder::TitleDesc => {
                self.roms.sort_by(|a, b| b.title.cmp(&a.title));
            }
            SortOrder::SizeAsc => {
                self.roms.sort_by_key(|rom| rom.size);
            }
            SortOrder::SizeDesc => {
                self.roms.sort_by(|a, b| b.size.cmp(&a.size));
            }
            SortOrder::RecentlyAdded => {
                // Already in discovery order (most recent first if filesystem supports it)
                // Could be enhanced with timestamp tracking
            }
        }
    }

    /// Get total number of ROMs
    #[must_use]
    pub fn rom_count(&self) -> usize {
        self.roms.len()
    }

    /// Get total size of all ROMs in bytes
    #[must_use]
    #[allow(dead_code)] // Future: display in library footer
    pub fn total_size(&self) -> u64 {
        self.roms.iter().map(|rom| rom.size).sum()
    }
}

impl Default for LibraryState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_library_state_new() {
        let state = LibraryState::new();
        assert_eq!(state.rom_count(), 0);
        assert_eq!(state.view_mode, ViewMode::Grid);
        assert!(state.search_query.is_empty());
    }

    #[test]
    fn test_toggle_view_mode() {
        let mut state = LibraryState::new();

        assert_eq!(state.view_mode, ViewMode::Grid);

        state.toggle_view_mode();
        assert_eq!(state.view_mode, ViewMode::List);

        state.toggle_view_mode();
        assert_eq!(state.view_mode, ViewMode::Grid);
    }

    #[test]
    fn test_search_query() {
        let mut state = LibraryState::new();

        state.set_search_query("mario".to_string());
        assert_eq!(state.search_query, "mario");

        state.clear_search();
        assert!(state.search_query.is_empty());
    }
}

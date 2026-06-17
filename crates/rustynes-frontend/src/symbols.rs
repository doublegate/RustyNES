//! v1.4.0 Workstream D (D1) — debugger symbol / label file loading.
//!
//! Parses the three label-file formats `RustyNES`'s reference emulators emit and
//! folds them into a single `address -> label` map the CPU disassembler, the
//! breakpoint list, and the trace view consult to annotate raw `$XXXX`
//! addresses with human-readable names.
//!
//! Supported formats (all simple, line-based — no external dependency):
//!
//! - **`.sym`** — the ca65 / WLA-DX style `ADDR LABEL` table (also the form
//!   `ld65 --dbgfile`-adjacent tools and many homebrew toolchains export). Lines
//!   are `<hex-addr> <label>`, the address optionally bank-prefixed
//!   (`00:8000 reset`) — we keep the low 16 bits. A leading `[sections]` /
//!   `[labels]` INI-style header (WLA `.sym`) is tolerated: only lines that
//!   parse as `addr label` are kept; anything else is skipped.
//! - **Mesen `.mlb`** — `MemoryType:Address[-EndAddress]:Label[:Comment]`, e.g.
//!   `P:8000:Reset` (PRG-ROM) or `G:0000:zp_player_x` (CPU RAM). We map the
//!   memory types that live in the CPU address space to their CPU address:
//!   `G` (system RAM) -> the address as-is (`$0000-$1FFF`), `R` (WRAM/SRAM) ->
//!   `$6000 + addr`, `P` (PRG ROM) -> `$8000 + (addr & 0x7FFF)` (the usual
//!   `$8000` mapping window). A range labels only its start address.
//! - **FCEUX `.nl`** — name-list lines `$ADDR#Name#Comment`, e.g.
//!   `$8000#Reset#`. The optional bank banner line (`$8000`) and blank/comment
//!   lines are skipped.
//!
//! The map is purely a frontend display aid; it never touches the deterministic
//! core. Loading is native-only (it reads a file the user picks); the type is
//! present on every target so the call sites don't need `cfg` walls.

use std::collections::HashMap;

/// The format a symbol file was parsed as (for the status line).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolFormat {
    /// ca65 / WLA-DX `ADDR LABEL` table.
    Sym,
    /// Mesen `MemoryType:Address:Label` label file.
    Mlb,
    /// FCEUX `$ADDR#Name#Comment` name list.
    Nl,
}

impl SymbolFormat {
    /// Pick a parser from a file extension (case-insensitive). `None` for an
    /// unrecognized extension.
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "sym" => Some(Self::Sym),
            "mlb" => Some(Self::Mlb),
            "nl" => Some(Self::Nl),
            _ => None,
        }
    }
}

/// An `address -> label` map for annotating the disassembler / breakpoint /
/// trace views.
///
/// Built from one or more parsed label files (later loads merge in, overwriting
/// an existing label at the same address).
#[derive(Debug, Default, Clone)]
pub struct SymbolMap {
    labels: HashMap<u16, String>,
}

impl SymbolMap {
    /// The label at `addr`, if any.
    #[must_use]
    pub fn label(&self, addr: u16) -> Option<&str> {
        self.labels.get(&addr).map(String::as_str)
    }

    /// Number of labels.
    #[must_use]
    pub fn len(&self) -> usize {
        self.labels.len()
    }

    /// Whether the map holds no labels.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    /// Drop every label.
    pub fn clear(&mut self) {
        self.labels.clear();
    }

    /// v1.5.0 Workstream B (B4) — every `(address, label)` pair (unordered), for
    /// pushing into the Lua scripting engine's `sym:` query tables.
    #[must_use]
    pub fn pairs(&self) -> Vec<(u16, String)> {
        self.labels
            .iter()
            .map(|(addr, label)| (*addr, label.clone()))
            .collect()
    }

    /// Insert a single label (later inserts overwrite). Empty labels are
    /// ignored so a malformed line can't shadow an address with "".
    fn insert(&mut self, addr: u16, label: &str) {
        let label = label.trim();
        if !label.is_empty() {
            self.labels.insert(addr, label.to_owned());
        }
    }

    /// Parse `text` in `format` and merge the results into this map. Returns the
    /// number of labels added (lines that didn't parse are silently skipped, so
    /// a header / comment / blank line never aborts the load).
    pub fn merge_str(&mut self, text: &str, format: SymbolFormat) -> usize {
        let before = self.labels.len();
        match format {
            SymbolFormat::Sym => parse_sym(self, text),
            SymbolFormat::Mlb => parse_mlb(self, text),
            SymbolFormat::Nl => parse_nl(self, text),
        }
        // `insert` may overwrite, so report distinct-address growth rather than
        // a raw line count.
        self.labels.len().saturating_sub(before)
    }
}

/// Strip an optional `$` / `0x` prefix and parse a hex `u16` (keeping the low 16
/// bits of a wider value, so a bank-prefixed `00:8000` low part still parses).
fn parse_hex(s: &str) -> Option<u16> {
    let s = s.trim();
    let s = s.strip_prefix('$').unwrap_or(s);
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u32::from_str_radix(s, 16).ok().map(|v| (v & 0xFFFF) as u16)
}

/// `ADDR LABEL` (whitespace-separated), address optionally `bank:addr`.
fn parse_sym(map: &mut SymbolMap, text: &str) {
    for line in text.lines() {
        let line = line.trim();
        // Skip blanks, comments (`;`), and INI section headers (`[labels]`).
        if line.is_empty() || line.starts_with(';') || line.starts_with('[') {
            continue;
        }
        let mut it = line.split_whitespace();
        let Some(addr_tok) = it.next() else { continue };
        let Some(label) = it.next() else { continue };
        // A bank-prefixed `00:8000` keeps the part after the last colon.
        let addr_tok = addr_tok.rsplit(':').next().unwrap_or(addr_tok);
        if let Some(addr) = parse_hex(addr_tok) {
            map.insert(addr, label);
        }
    }
}

/// Mesen `MemoryType:Address[-End]:Label[:Comment]`.
fn parse_mlb(map: &mut SymbolMap, text: &str) {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let mut parts = line.splitn(4, ':');
        let Some(mem) = parts.next() else { continue };
        let Some(addr_field) = parts.next() else {
            continue;
        };
        let Some(label) = parts.next() else { continue };
        // A range `8000-8010` labels only its start address.
        let start = addr_field.split('-').next().unwrap_or(addr_field);
        let Some(raw) = parse_hex(start) else {
            continue;
        };
        // Map the CPU-visible memory types into the CPU address space; skip the
        // PPU / OAM / palette types (not part of the CPU disassembly view).
        let cpu_addr = match mem.trim() {
            "G" => Some(raw & 0x1FFF), // system RAM ($0000-$1FFF)
            "R" => Some(0x6000u16.wrapping_add(raw & 0x1FFF)), // WRAM/SRAM ($6000+)
            "P" => Some(0x8000u16.wrapping_add(raw & 0x7FFF)), // PRG ROM ($8000+)
            _ => None,
        };
        if let Some(addr) = cpu_addr {
            map.insert(addr, label);
        }
    }
}

/// FCEUX `$ADDR#Name#Comment`.
fn parse_nl(map: &mut SymbolMap, text: &str) {
    for line in text.lines() {
        let line = line.trim();
        // A bank banner is a lone `$XXXX` with no `#`; skip it + blanks.
        if line.is_empty() || !line.contains('#') {
            continue;
        }
        let mut parts = line.splitn(3, '#');
        let Some(addr_tok) = parts.next() else {
            continue;
        };
        let Some(name) = parts.next() else { continue };
        if let Some(addr) = parse_hex(addr_tok) {
            map.insert(addr, name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sym_basic_and_bank_prefixed() {
        let mut m = SymbolMap::default();
        let added = m.merge_str(
            "; a comment\n[labels]\n8000 Reset\n00:C000 main_loop\n$0010 player_x\n",
            SymbolFormat::Sym,
        );
        assert_eq!(added, 3);
        assert_eq!(m.label(0x8000), Some("Reset"));
        assert_eq!(m.label(0xC000), Some("main_loop"));
        assert_eq!(m.label(0x0010), Some("player_x"));
        assert_eq!(m.label(0x1234), None);
    }

    #[test]
    fn mlb_memory_types_map_into_cpu_space() {
        let mut m = SymbolMap::default();
        let added = m.merge_str(
            "// header\nP:0000:Reset\nG:0010:zp_player\nR:0000:save_slot\nP:0100-0110:table\n\
             X:0000:ignored_ppu\n",
            SymbolFormat::Mlb,
        );
        // P:0000 -> $8000, G:0010 -> $0010, R:0000 -> $6000, P:0100 -> $8100.
        // X: (a non-CPU type) is skipped.
        assert_eq!(added, 4);
        assert_eq!(m.label(0x8000), Some("Reset"));
        assert_eq!(m.label(0x0010), Some("zp_player"));
        assert_eq!(m.label(0x6000), Some("save_slot"));
        assert_eq!(m.label(0x8100), Some("table"));
    }

    #[test]
    fn nl_name_list_skips_banner_and_comments() {
        let mut m = SymbolMap::default();
        let added = m.merge_str(
            "$8000\n$8000#Reset#the entry point\n$8003#loop#\n\n$0042#flag#zp\n",
            SymbolFormat::Nl,
        );
        assert_eq!(added, 3);
        assert_eq!(m.label(0x8000), Some("Reset"));
        assert_eq!(m.label(0x8003), Some("loop"));
        assert_eq!(m.label(0x0042), Some("flag"));
    }

    #[test]
    fn later_loads_overwrite_and_clear_empties() {
        let mut m = SymbolMap::default();
        m.merge_str("8000 first\n", SymbolFormat::Sym);
        // Re-loading the same address overwrites; distinct-address growth is 0.
        let added = m.merge_str("8000 second\n", SymbolFormat::Sym);
        assert_eq!(added, 0);
        assert_eq!(m.label(0x8000), Some("second"));
        assert!(!m.is_empty());
        m.clear();
        assert!(m.is_empty());
    }

    #[test]
    fn malformed_lines_are_skipped_not_fatal() {
        let mut m = SymbolMap::default();
        // Garbage, an address with no label, an empty label — all skipped.
        let added = m.merge_str(
            "not hex here\nZZZZ label\n8000\n9000 \n8100 ok\n",
            SymbolFormat::Sym,
        );
        assert_eq!(added, 1);
        assert_eq!(m.label(0x8100), Some("ok"));
    }

    #[test]
    fn extension_dispatch() {
        assert_eq!(SymbolFormat::from_extension("SYM"), Some(SymbolFormat::Sym));
        assert_eq!(SymbolFormat::from_extension("mlb"), Some(SymbolFormat::Mlb));
        assert_eq!(SymbolFormat::from_extension("nl"), Some(SymbolFormat::Nl));
        assert_eq!(SymbolFormat::from_extension("txt"), None);
    }
}

//! v1.7.0 "Forge" Workstream C (C3) — ca65 / cc65 `.dbg` source-line mapping.
//!
//! The existing [`crate::symbols::SymbolMap`] (v1.4.0) carries symbol *names*
//! only. This module adds the #1 remaining devtools gap versus Mesen2: parsing
//! the **ld65 `--dbgfile` `.dbg`** output into an `address → (source file, line)`
//! map, so the disassembly + memory views can be annotated with the original
//! ca65/cc65 source line.
//!
//! ## The `.dbg` schema (the records we consume)
//!
//! ld65 emits a flat, line-based file; each line is `<type> key=value,key=value`.
//! Source-line mapping needs three record types:
//!
//! - `seg id=N,name="CODE",start=0x8000,size=...,...` — a segment, with its
//!   **CPU base address**.
//! - `span id=N,seg=M,start=O,size=Z,...` — a span: `Z` bytes at byte offset `O`
//!   within segment `M` (so its CPU address is `seg[M].start + O`).
//! - `file id=N,name="src/main.s",...` — the source-file table.
//! - `line id=N,file=F,line=L[,span=S+S+...][,...]` — a source line `L` in file
//!   `F`, covering the listed spans.
//!
//! For every `line` record we resolve each referenced span to its CPU address
//! range and record `address → (file, line)` for every byte in range. Lines
//! with no spans (e.g. macro / comment lines) carry no address and are skipped.
//! This mirrors Mesen2's `DbgImporter`/`NesDbgImporter`.
//!
//! ## Output-only
//!
//! The map is a pure frontend display aid built by *parsing a file on disk*. It
//! never touches the deterministic core, so `AccuracyCoin` / the determinism
//! contract are unaffected.

use std::collections::HashMap;

/// A resolved source location: which file (by index into [`SourceMap::files`])
/// and which 1-based line number.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceLoc {
    /// Index into [`SourceMap::files`].
    pub file: usize,
    /// 1-based source line number.
    pub line: u32,
}

/// An `address → source line` map parsed from a ca65/cc65 `.dbg` file.
#[derive(Debug, Default, Clone)]
pub struct SourceMap {
    /// Source-file paths, indexed by [`SourceLoc::file`].
    files: Vec<String>,
    /// CPU address → the source line that produced it.
    locs: HashMap<u16, SourceLoc>,
}

/// A parsed `seg` record (id → CPU base address).
#[derive(Clone, Copy, Default)]
struct Seg {
    start: u32,
}

/// A parsed `span` record (id → segment id + offset + size).
#[derive(Clone, Copy)]
struct Span {
    seg: u32,
    start: u32,
    size: u32,
}

impl SourceMap {
    /// The number of distinct addresses with a mapped source line.
    #[must_use]
    pub fn len(&self) -> usize {
        self.locs.len()
    }

    /// Whether no source lines are mapped.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.locs.is_empty()
    }

    /// Drop the whole map.
    pub fn clear(&mut self) {
        self.files.clear();
        self.locs.clear();
    }

    /// The source location at `addr`, if any.
    #[must_use]
    pub fn loc(&self, addr: u16) -> Option<SourceLoc> {
        self.locs.get(&addr).copied()
    }

    /// The source-file path for a [`SourceLoc`].
    #[must_use]
    pub fn file_name(&self, file: usize) -> Option<&str> {
        self.files.get(file).map(String::as_str)
    }

    /// A short `file:line` annotation for `addr`, if mapped. The file is
    /// reduced to its basename so the disassembly stays compact.
    #[must_use]
    pub fn annotation(&self, addr: u16) -> Option<String> {
        let loc = self.loc(addr)?;
        let name = self.file_name(loc.file)?;
        let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
        Some(format!("{base}:{}", loc.line))
    }

    /// Parse a `.dbg` file's `text` and replace this map's contents with the
    /// result. Returns the number of distinct addresses mapped (so the caller
    /// can surface a status line). Malformed / unrecognised lines are skipped,
    /// so a partial / future-extended `.dbg` never aborts the load.
    pub fn load_dbg(&mut self, text: &str) -> usize {
        self.clear();

        let mut segs: HashMap<u32, Seg> = HashMap::new();
        let mut spans: HashMap<u32, Span> = HashMap::new();
        // file id → our `files` index (in declaration order).
        let mut file_index: HashMap<u32, usize> = HashMap::new();

        // Pass 1: gather the segment, span, and file tables. (ld65 declares
        // these before the `line` records, but we don't rely on ordering.)
        for line in text.lines() {
            let line = line.trim();
            let Some((kind, rest)) = split_record(line) else {
                continue;
            };
            match kind {
                "seg" => {
                    if let (Some(id), Some(start)) =
                        (field_u32(rest, "id"), field_u32(rest, "start"))
                    {
                        segs.insert(id, Seg { start });
                    }
                }
                "span" => {
                    if let (Some(id), Some(seg), Some(start)) = (
                        field_u32(rest, "id"),
                        field_u32(rest, "seg"),
                        field_u32(rest, "start"),
                    ) {
                        let size = field_u32(rest, "size").unwrap_or(1);
                        spans.insert(id, Span { seg, start, size });
                    }
                }
                "file" => {
                    if let (Some(id), Some(name)) = (field_u32(rest, "id"), field_str(rest, "name"))
                    {
                        let idx = self.files.len();
                        self.files.push(name);
                        file_index.insert(id, idx);
                    }
                }
                _ => {}
            }
        }

        // Pass 2: resolve every `line` record's spans to CPU addresses.
        for line in text.lines() {
            let line = line.trim();
            let Some((kind, rest)) = split_record(line) else {
                continue;
            };
            if kind != "line" {
                continue;
            }
            let (Some(file_id), Some(line_no)) = (field_u32(rest, "file"), field_u32(rest, "line"))
            else {
                continue;
            };
            let Some(&file) = file_index.get(&file_id) else {
                continue;
            };
            // `span` is a `+`-joined list of span ids; absent for code-less
            // lines (which carry no address).
            let Some(span_field) = field_raw(rest, "span") else {
                continue;
            };
            let loc = SourceLoc {
                file,
                line: line_no,
            };
            for span_id in span_field.split('+') {
                let Ok(span_id) = span_id.trim().parse::<u32>() else {
                    continue;
                };
                let Some(span) = spans.get(&span_id) else {
                    continue;
                };
                let Some(seg) = segs.get(&span.seg) else {
                    continue;
                };
                let base = seg.start + span.start;
                for off in 0..span.size {
                    let addr = base + off;
                    if let Ok(addr) = u16::try_from(addr) {
                        // First writer wins for a given byte so the *narrowest*
                        // owning line (declared first by ld65) is kept.
                        self.locs.entry(addr).or_insert(loc);
                    }
                }
            }
        }

        self.locs.len()
    }
}

/// Split a `.dbg` line into its record type + the `key=value,...` remainder.
/// Returns `None` for blank lines or a line with no space-separated body.
fn split_record(line: &str) -> Option<(&str, &str)> {
    if line.is_empty() {
        return None;
    }
    let (kind, rest) = line.split_once('\t').or_else(|| line.split_once(' '))?;
    Some((kind.trim(), rest.trim()))
}

/// Extract a `key=...` field's raw value (stops at the next comma not inside a
/// quoted string). ld65 quotes string values and leaves numbers bare.
fn field_raw<'a>(rest: &'a str, key: &str) -> Option<&'a str> {
    // Scan comma-separated `key=value` pairs, honouring quotes so a quoted
    // value containing a comma isn't split.
    let mut i = 0;
    let bytes = rest.as_bytes();
    while i < bytes.len() {
        // Find the end of this pair (a comma outside quotes).
        let start = i;
        let mut in_quotes = false;
        while i < bytes.len() {
            match bytes[i] {
                b'"' => in_quotes = !in_quotes,
                b',' if !in_quotes => break,
                _ => {}
            }
            i += 1;
        }
        let pair = rest[start..i].trim();
        i += 1; // skip the comma
        if let Some((k, v)) = pair.split_once('=')
            && k.trim() == key
        {
            return Some(v.trim());
        }
    }
    None
}

/// A `key=N` field parsed as a `u32` (accepts `0x`-prefixed hex, bare decimal,
/// or bare hex).
fn field_u32(rest: &str, key: &str) -> Option<u32> {
    let v = field_raw(rest, key)?.trim_matches('"');
    v.strip_prefix("0x")
        .or_else(|| v.strip_prefix("0X"))
        .map_or_else(
            || {
                v.parse::<u32>()
                    .ok()
                    .or_else(|| u32::from_str_radix(v, 16).ok())
            },
            |hex| u32::from_str_radix(hex, 16).ok(),
        )
}

/// A quoted `key="..."` field's unquoted string value.
fn field_str(rest: &str, key: &str) -> Option<String> {
    let v = field_raw(rest, key)?;
    Some(v.trim_matches('"').to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal but representative ld65 `.dbg` excerpt: one CODE segment based
    /// at $8000, two spans, two source files, and three line records (one
    /// without a span → no address).
    const SAMPLE: &str = "\
version\tmajor=2,minor=0
file\tid=0,name=\"src/main.s\",size=128,mtime=0x600
file\tid=1,name=\"inc/macros.inc\",size=64,mtime=0x600
seg\tid=0,name=\"CODE\",start=0x8000,size=0x100,addrsize=absolute,type=ro
span\tid=0,seg=0,start=0,size=3
span\tid=1,seg=0,start=3,size=2
line\tid=0,file=0,line=10,span=0,type=1
line\tid=1,file=0,line=11,span=1
line\tid=2,file=1,line=5
sym\tid=0,name=\"reset\",addrsize=absolute,scope=0,def=0
";

    #[test]
    fn parses_address_to_source_lines() {
        let mut m = SourceMap::default();
        let mapped = m.load_dbg(SAMPLE);
        // span 0 = $8000..$8003 (3 bytes), span 1 = $8003..$8005 (2 bytes).
        assert_eq!(mapped, 5);
        assert_eq!(
            m.loc(0x8000),
            Some(SourceLoc { file: 0, line: 10 }),
            "$8000 is line 10 of main.s"
        );
        assert_eq!(m.loc(0x8002), Some(SourceLoc { file: 0, line: 10 }));
        assert_eq!(
            m.loc(0x8003),
            Some(SourceLoc { file: 0, line: 11 }),
            "$8003 is line 11 of main.s"
        );
        assert_eq!(m.loc(0x8004), Some(SourceLoc { file: 0, line: 11 }));
        assert_eq!(m.loc(0x8005), None, "past the mapped spans");
    }

    #[test]
    fn line_without_span_maps_no_address() {
        let mut m = SourceMap::default();
        m.load_dbg(SAMPLE);
        // line id=2 (macros.inc:5) has no span → contributes no address.
        let macros_addrs = (0u16..=0xFFFF)
            .filter(|&a| m.loc(a).is_some_and(|l| l.file == 1))
            .count();
        assert_eq!(macros_addrs, 0);
    }

    #[test]
    fn annotation_uses_basename() {
        let mut m = SourceMap::default();
        m.load_dbg(SAMPLE);
        assert_eq!(m.annotation(0x8000).as_deref(), Some("main.s:10"));
        assert_eq!(m.annotation(0x8003).as_deref(), Some("main.s:11"));
        assert_eq!(m.annotation(0x9000), None);
    }

    #[test]
    fn file_names_resolve() {
        let mut m = SourceMap::default();
        m.load_dbg(SAMPLE);
        assert_eq!(m.file_name(0), Some("src/main.s"));
        assert_eq!(m.file_name(1), Some("inc/macros.inc"));
        assert_eq!(m.file_name(2), None);
    }

    #[test]
    fn clear_empties_the_map() {
        let mut m = SourceMap::default();
        m.load_dbg(SAMPLE);
        assert!(!m.is_empty());
        m.clear();
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
        assert_eq!(m.loc(0x8000), None);
    }

    #[test]
    fn load_replaces_previous_contents() {
        let mut m = SourceMap::default();
        m.load_dbg(SAMPLE);
        let n = m.load_dbg(SAMPLE);
        assert_eq!(n, 5, "reloading is idempotent, not additive");
        assert_eq!(m.len(), 5);
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let mut m = SourceMap::default();
        let mapped = m.load_dbg("garbage\nline\tfile=99,line=1,span=99\nseg\tid=0\n");
        assert_eq!(mapped, 0, "no resolvable spans/segs/files");
        assert!(m.is_empty());
    }

    #[test]
    fn field_parsers_handle_hex_and_quotes() {
        assert_eq!(field_u32("id=0,start=0x8000", "start"), Some(0x8000));
        assert_eq!(field_u32("id=42,line=10", "line"), Some(10));
        assert_eq!(
            field_str("id=0,name=\"src/main.s\",size=10", "name").as_deref(),
            Some("src/main.s")
        );
        // A comma inside a quoted value must not split the field.
        assert_eq!(
            field_str("name=\"a,b.s\",size=1", "name").as_deref(),
            Some("a,b.s")
        );
    }
}

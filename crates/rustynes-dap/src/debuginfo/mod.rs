// SPDX-License-Identifier: GPL-3.0-or-later
//! The toolchain-neutral debug-information model the debugger runs on.
//!
//! Two toolchains build C for the NES and describe the result differently:
//!
//! - **cc65** writes a separate text file through `ld65 --dbgfile`
//!   ([`cc65`]): segments, spans, source lines, assembler symbols, scopes and
//!   C symbols (`csym`), but **no C types**.
//! - **llvm-mos** writes DWARF 5 into the ELF it links beside the `.nes`
//!   ([`dwarf`]): a line program, subprograms, variables with location
//!   expressions, and real types.
//!
//! Both are lowered into one [`DebugInfo`], so the engine, the DAP server and
//! the tests never ask which toolchain produced a program. What the model
//! keeps is exactly what a source-level debugger needs and no more:
//!
//! - an address -> source-line map ([`DebugInfo::line_at`]) for stack traces;
//! - the **statement starts** ([`DebugInfo::stmt_at`]): the addresses where a
//!   source line begins, which are where a line breakpoint is placed and where
//!   a line step stops;
//! - functions with their address range, their entry, the address at which
//!   their frame base becomes valid ([`Function::frame_hook`]) and their
//!   variables;
//! - globals.
//!
//! # Frame bases
//!
//! A local's address is `frame base + offset`, and the two toolchains keep the
//! frame base in different places ([`FrameBase`]):
//!
//! - cc65 compiles C locals onto a software stack whose pointer is the
//!   zero-page word `c_sp` (`sp` before cc65 renamed it). A `csym`'s `offs` is
//!   relative to the value that pointer has once the function's prologue has
//!   pushed the register-passed (`__fastcall__`) parameter: measured on the
//!   fixture, parameter `v` is at `offs=0` and local `r` at `offs=-1`, and
//!   both read back correctly only against the pointer sampled *after* the
//!   prologue's `JSR pusha`. The pointer moves again as the body pushes
//!   arguments for calls, so it is sampled once, at that point, per call.
//! - llvm-mos keeps its soft-stack pointer in the imaginary 16-bit register
//!   `RS0` (the zero-page pair `__rc0` / `__rc1`) and names it as the frame
//!   base with `DW_OP_regx`; locals are `DW_OP_fbreg` offsets from it. The
//!   register numbers follow llvm-mos's published *DWARF Specification for
//!   MOS-Compatible Processors*: `RCn` is `0x0002_0000 + n`, `RSn` is
//!   `0x0003_0000 + n` (the pair `RC(2n)`, `RC(2n+1)`), and the zero-page
//!   address of `RCn` is the ELF symbol `__rcN`.
//!
//! The engine samples the frame base at [`Function::frame_hook`] on every
//! call and keeps it in a shadow frame, so caller frames show their locals
//! too, not only the innermost one.

pub mod cc65;
pub mod dwarf;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Which toolchain the information came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// cc65 / ld65 `--dbgfile`.
    Cc65,
    /// DWARF in an llvm-mos ELF.
    Dwarf,
}

/// One source file the program was built from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFile {
    /// The name exactly as the debug information records it.
    pub recorded: String,
    /// The name resolved against the source root (the debug file's
    /// directory unless the client names another). Not checked for
    /// existence: a client may map paths itself.
    pub path: PathBuf,
}

/// A source position: an index into [`DebugInfo::files`] and a 1-based line.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct Loc {
    /// Index into [`DebugInfo::files`].
    pub file: usize,
    /// 1-based line number.
    pub line: u32,
}

/// What a variable's bytes mean, as far as the debug information says.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Encoding {
    /// Unsigned integer (also `unsigned char`).
    Unsigned,
    /// Signed integer (also `signed char`).
    Signed,
    /// `_Bool`.
    Bool,
    /// A pointer: shown as a hexadecimal address.
    Pointer,
    /// The toolchain recorded no type (cc65): shown as unsigned bytes.
    Unknown,
    /// An aggregate or anything else: shown as raw bytes.
    Bytes,
}

/// A variable's type, reduced to what is needed to show its value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeInfo {
    /// The type's name as the source spells it (or a description).
    pub name: String,
    /// Size in bytes (at least 1).
    pub size: u16,
    /// How to render the bytes.
    pub encoding: Encoding,
}

impl TypeInfo {
    /// The stand-in type for a cc65 symbol: cc65 records no C types, so the
    /// debugger shows one byte and says why.
    #[must_use]
    pub fn cc65_untyped() -> Self {
        Self {
            name: "byte (cc65 records no C type)".into(),
            size: 1,
            encoding: Encoding::Unknown,
        }
    }
}

/// Where a variable lives.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VarLoc {
    /// A fixed CPU address.
    Absolute(u16),
    /// An offset from the function's frame base.
    Frame(i32),
    /// The debug information describes a location this debugger does not
    /// evaluate; the string says which.
    Unavailable(String),
}

/// A named variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Variable {
    /// Source name.
    pub name: String,
    /// Its type.
    pub ty: TypeInfo,
    /// Its location.
    pub loc: VarLoc,
    /// Whether it is a parameter (shown first, as editors expect).
    pub is_param: bool,
}

/// How to read a function's frame base at its [`Function::frame_hook`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameBase {
    /// The function has no frame-relative variables.
    None,
    /// A little-endian 16-bit pointer in zero page (cc65's `c_sp`, or an
    /// llvm-mos `RSn` pair).
    ZpPointer(u8),
}

/// A function with debug information.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Function {
    /// Source name.
    pub name: String,
    /// First address (inclusive).
    pub low: u16,
    /// End address (exclusive, may be `0x1_0000`).
    pub high: u32,
    /// Where its frame base is valid and is sampled. For cc65 this is moved
    /// past a prologue `JSR` to a push routine by the engine, which can read
    /// the code; for llvm-mos it is the line program's `prologue_end`.
    pub frame_hook: u16,
    /// How to read the frame base.
    pub frame_base: FrameBase,
    /// Parameters and locals.
    pub vars: Vec<Variable>,
    /// Declaration position, when recorded.
    pub decl: Option<Loc>,
}

impl Function {
    /// Whether `pc` lies inside this function.
    #[must_use]
    pub fn contains(&self, pc: u16) -> bool {
        pc >= self.low && u32::from(pc) < self.high
    }
}

/// A half-open address range mapped to one source line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineRange {
    /// First address.
    pub start: u16,
    /// End (exclusive).
    pub end: u32,
    /// The line.
    pub loc: Loc,
}

/// Debug information, lowered from either toolchain.
#[derive(Clone, Debug)]
pub struct DebugInfo {
    /// Which toolchain.
    pub format: Format,
    /// Source files.
    pub files: Vec<SourceFile>,
    /// Address ranges, sorted by start, non-overlapping.
    pub ranges: Vec<LineRange>,
    /// Statement starts.
    pub stmts: BTreeMap<u16, Loc>,
    /// Functions, sorted by `low`.
    pub functions: Vec<Function>,
    /// Program-wide variables.
    pub globals: Vec<Variable>,
    /// cc65 only: the entry addresses of the runtime's argument-push
    /// routines (`pusha`, `pusha0`, `pushax`, `pusheax`), so the engine can
    /// recognise a prologue `JSR` to one.
    pub push_routines: Vec<u16>,
}

/// A refusal to load debug information.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadError(pub String);

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LoadError {}

/// The largest debug file accepted (cc65's file for a small program is
/// about 430 KiB because it lists the whole runtime library).
pub const MAX_DEBUG_FILE_BYTES: u64 = 64 * 1024 * 1024;

impl DebugInfo {
    /// Load from bytes, choosing the reader by content: an ELF header means
    /// DWARF, `version\tmajor=` means a cc65 file. `source_root` resolves
    /// recorded relative file names.
    ///
    /// # Errors
    ///
    /// Neither format recognised, or the reader refused the content.
    pub fn load(bytes: &[u8], source_root: &Path) -> Result<Self, LoadError> {
        if bytes.starts_with(b"\x7fELF") {
            return dwarf::load(bytes, source_root);
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| LoadError("debug file is neither ELF nor UTF-8 text".into()))?;
        if text.trim_start().starts_with("version") {
            return Ok(cc65::load(text, source_root));
        }
        Err(LoadError(
            "debug file is neither an ELF (llvm-mos) nor an ld65 --dbgfile (cc65)".into(),
        ))
    }

    /// The source line whose code contains `pc`.
    #[must_use]
    pub fn line_at(&self, pc: u16) -> Option<Loc> {
        let i = self.ranges.partition_point(|r| r.start <= pc);
        let r = self.ranges.get(i.checked_sub(1)?)?;
        (u32::from(pc) < r.end).then_some(r.loc)
    }

    /// The line that begins at `pc`, if `pc` is a statement start.
    #[must_use]
    pub fn stmt_at(&self, pc: u16) -> Option<Loc> {
        self.stmts.get(&pc).copied()
    }

    /// The function containing `pc`.
    #[must_use]
    pub fn function_at(&self, pc: u16) -> Option<(usize, &Function)> {
        self.functions
            .iter()
            .enumerate()
            .find(|(_, f)| f.contains(pc))
    }

    /// The function named `name`.
    #[must_use]
    pub fn function_named(&self, name: &str) -> Option<&Function> {
        self.functions.iter().find(|f| f.name == name)
    }

    /// The statement-start addresses of `file:line`, moving forward to the
    /// next line that has code when `line` has none (what editors expect of
    /// a breakpoint on a blank or declaration line). Returns the line
    /// actually used and its addresses.
    #[must_use]
    pub fn resolve_line(&self, file: usize, line: u32) -> Option<(u32, Vec<u16>)> {
        let mut best: Option<u32> = None;
        for loc in self.stmts.values() {
            if loc.file == file && loc.line >= line && best.is_none_or(|b| loc.line < b) {
                best = Some(loc.line);
            }
        }
        let line = best?;
        let addrs = self
            .stmts
            .iter()
            .filter(|(_, l)| l.file == file && l.line == line)
            .map(|(&a, _)| a)
            .collect();
        Some((line, addrs))
    }

    /// The file a client means by `path`: an exact match on the resolved or
    /// recorded name first, then the longest match on trailing path
    /// components (so `/home/me/game/src/main.c` finds `src/main.c`).
    #[must_use]
    pub fn file_for_path(&self, path: &str) -> Option<usize> {
        let want = normalise(path);
        if let Some(i) = self.files.iter().position(|f| {
            normalise(&f.path.to_string_lossy()) == want || normalise(&f.recorded) == want
        }) {
            return Some(i);
        }
        let want_parts: Vec<&str> = want.rsplit('/').collect();
        let mut best = None;
        let mut best_len = 0;
        for (i, f) in self.files.iter().enumerate() {
            let rec = normalise(&f.recorded);
            let parts: Vec<&str> = rec.rsplit('/').filter(|p| *p != ".").collect();
            let common = parts
                .iter()
                .zip(&want_parts)
                .take_while(|(a, b)| a == b)
                .count();
            if common == parts.len() && common > best_len {
                best = Some(i);
                best_len = common;
            }
        }
        best
    }

    /// The global named `name`.
    #[must_use]
    pub fn global_named(&self, name: &str) -> Option<&Variable> {
        self.globals.iter().find(|v| v.name == name)
    }
}

/// Forward slashes, no `./` prefix, no duplicate separators.
fn normalise(path: &str) -> String {
    let s = path.replace('\\', "/");
    let mut out: Vec<&str> = Vec::new();
    for part in s.split('/') {
        if part == "." || (part.is_empty() && !out.is_empty()) {
            continue;
        }
        out.push(part);
    }
    out.join("/")
}

/// Resolve a recorded file name against `root` (absolute names stay).
fn resolve(root: &Path, recorded: &str) -> PathBuf {
    let p = Path::new(recorded);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}

/// Build sorted, non-overlapping [`LineRange`]s from per-byte claims. When
/// two lines claim one byte the narrower claim wins (an enclosing line, such
/// as a whole-function span, never hides the statement inside it).
fn ranges_from_claims(claims: &BTreeMap<u16, (Loc, u32)>) -> Vec<LineRange> {
    let mut out: Vec<LineRange> = Vec::new();
    for (&addr, &(loc, _)) in claims {
        if let Some(last) = out.last_mut()
            && last.loc == loc
            && last.end == u32::from(addr)
        {
            last.end += 1;
            continue;
        }
        out.push(LineRange {
            start: addr,
            end: u32::from(addr) + 1,
            loc,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> DebugInfo {
        let mut stmts = BTreeMap::new();
        stmts.insert(0x8000, Loc { file: 0, line: 10 });
        stmts.insert(0x8004, Loc { file: 0, line: 12 });
        stmts.insert(0x8010, Loc { file: 0, line: 12 });
        DebugInfo {
            format: Format::Cc65,
            files: vec![SourceFile {
                recorded: "src/main.c".into(),
                path: PathBuf::from("/proj/src/main.c"),
            }],
            ranges: vec![
                LineRange {
                    start: 0x8000,
                    end: 0x8004,
                    loc: Loc { file: 0, line: 10 },
                },
                LineRange {
                    start: 0x8004,
                    end: 0x8008,
                    loc: Loc { file: 0, line: 12 },
                },
            ],
            stmts,
            functions: Vec::new(),
            globals: Vec::new(),
            push_routines: Vec::new(),
        }
    }

    #[test]
    fn line_at_is_half_open() {
        let d = info();
        assert_eq!(d.line_at(0x7FFF), None);
        assert_eq!(d.line_at(0x8000).map(|l| l.line), Some(10));
        assert_eq!(d.line_at(0x8003).map(|l| l.line), Some(10));
        assert_eq!(d.line_at(0x8004).map(|l| l.line), Some(12));
        assert_eq!(d.line_at(0x8008), None);
    }

    #[test]
    fn resolve_line_moves_forward_and_collects_every_start() {
        let d = info();
        assert_eq!(d.resolve_line(0, 11), Some((12, vec![0x8004, 0x8010])));
        assert_eq!(d.resolve_line(0, 10), Some((10, vec![0x8000])));
        assert_eq!(d.resolve_line(0, 13), None);
        assert_eq!(d.resolve_line(1, 10), None);
    }

    #[test]
    fn file_for_path_matches_suffixes() {
        let d = info();
        assert_eq!(d.file_for_path("/proj/src/main.c"), Some(0));
        assert_eq!(d.file_for_path("/elsewhere/src/main.c"), Some(0));
        assert_eq!(d.file_for_path("C:\\x\\src\\main.c"), Some(0));
        assert_eq!(d.file_for_path("/elsewhere/lib/main.c"), None);
        assert_eq!(d.file_for_path("main.h"), None);
    }

    #[test]
    fn adjacent_claims_of_one_line_merge() {
        let mut claims = BTreeMap::new();
        claims.insert(0x10, (Loc { file: 0, line: 1 }, 4));
        claims.insert(0x11, (Loc { file: 0, line: 1 }, 4));
        claims.insert(0x12, (Loc { file: 0, line: 2 }, 1));
        claims.insert(0x13, (Loc { file: 0, line: 1 }, 4));
        let r = ranges_from_claims(&claims);
        assert_eq!(r.len(), 3);
        assert_eq!((r[0].start, r[0].end), (0x10, 0x12));
        assert_eq!(r[1].loc.line, 2);
        assert_eq!((r[2].start, r[2].end), (0x13, 0x14));
    }
}

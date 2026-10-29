// SPDX-License-Identifier: GPL-3.0-or-later
//
// Origin (a disclosure, not a derivation record): written for T-DEBUG-DAP
// from the ld65 `--dbgfile` output itself (the fixture's
// `tests/roms/dap/dapdemo_cc65.dbg`, read as data) and the cc65
// documentation, which names the option but not the record layout. No
// emulator's importer was consulted. The author HAD read
// `crates/rustynes-frontend/src/debugger/source_map.rs` earlier in the same
// session; that file is Mesen2-derived (its own Provenance header). This
// reader shares its record vocabulary (`seg` / `span` / `file` / `line`,
// which is ld65's), not its code, and also reads `mod`, `scope`, `sym` and
// `csym`, which that one does not. Flagged for maintainer review rather
// than self-certified; if judged derived, it takes a Provenance header and
// a section 1 row.
//! The cc65 reader: `ld65 --dbgfile` output.
//!
//! The file is line-oriented: `<record>\t<key>=<value>,<key>=<value>...`.
//! Strings are double-quoted; numbers are decimal or `0x` hexadecimal; a
//! list is `+`-joined ids. The records this reader uses, as ld65 2.19
//! writes them (see the fixture `.dbg`):
//!
//! | record | fields used | meaning |
//! | --- | --- | --- |
//! | `file` | `id`, `name`, `mod` | a source file; `mod` lists the modules it went into |
//! | `mod` | `id`, `lib` | an object module; `lib` present means it came from a library |
//! | `seg` | `id`, `start` | a segment and its CPU base address |
//! | `span` | `id`, `seg`, `start`, `size` | `size` bytes at offset `start` in a segment |
//! | `line` | `file`, `line`, `type`, `span` | a source line and the spans it produced; `type=1` is a C line, absent (0) an assembler line, 2 a macro expansion |
//! | `scope` | `id`, `name`, `mod`, `type`, `parent`, `sym`, `span` | a lexical scope; `type=scope` with a `sym` is a C function |
//! | `sym` | `id`, `name`, `val`, `seg`, `scope`, `type` | an assembler symbol |
//! | `csym` | `name`, `scope`, `sc`, `offs`, `sym` | a C symbol: `sc=auto` is a stack local at `offs` from the frame base, `sc=static`/`ext` names an assembler symbol |
//!
//! Lines: when the program has C lines (`type=1`), only C lines form the
//! line table, so a step moves by C statement and never stops in the
//! compiler's generated assembler; a pure-assembler program falls back to
//! assembler lines from modules that are not library members (the runtime
//! library's own sources are not the program). Each span start is a
//! statement start; where spans of two lines overlap, the narrower wins.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use super::{
    DebugInfo, Format, FrameBase, Function, Loc, SourceFile, TypeInfo, VarLoc, Variable,
    ranges_from_claims, resolve,
};

/// One record: its kind and the raw `key=value` body.
struct Record<'a> {
    kind: &'a str,
    body: &'a str,
}

impl<'a> Record<'a> {
    fn parse(line: &'a str) -> Option<Self> {
        let line = line.trim_end_matches(['\r', '\n']);
        let (kind, body) = line.split_once('\t')?;
        Some(Self { kind, body })
    }

    /// The raw text of `key`, honouring quotes (a quoted value may contain a
    /// comma).
    fn raw(&self, key: &str) -> Option<&'a str> {
        let mut rest = self.body;
        while !rest.is_empty() {
            let mut quoted = false;
            let mut end = rest.len();
            for (i, b) in rest.bytes().enumerate() {
                match b {
                    b'"' => quoted = !quoted,
                    b',' if !quoted => {
                        end = i;
                        break;
                    }
                    _ => {}
                }
            }
            let pair = &rest[..end];
            if let Some((k, v)) = pair.split_once('=')
                && k == key
            {
                return Some(v);
            }
            rest = rest.get(end + 1..).unwrap_or("");
        }
        None
    }

    fn num(&self, key: &str) -> Option<i64> {
        let v = self.raw(key)?;
        let (neg, v) = v.strip_prefix('-').map_or((false, v), |s| (true, s));
        let n = v
            .strip_prefix("0x")
            .or_else(|| v.strip_prefix("0X"))
            .map_or_else(
                || v.parse::<i64>().ok(),
                |h| i64::from_str_radix(h, 16).ok(),
            )?;
        Some(if neg { -n } else { n })
    }

    fn id(&self, key: &str) -> Option<u32> {
        self.num(key).and_then(|n| u32::try_from(n).ok())
    }

    fn string(&self, key: &str) -> Option<&'a str> {
        let v = self.raw(key)?;
        v.strip_prefix('"')?.strip_suffix('"')
    }

    fn ids(&self, key: &str) -> Vec<u32> {
        self.raw(key)
            .map(|v| v.split('+').filter_map(|s| s.parse().ok()).collect())
            .unwrap_or_default()
    }
}

struct Span {
    seg: u32,
    start: u32,
    size: u32,
}

struct Scope<'a> {
    name: &'a str,
    module: Option<u32>,
    is_function: bool,
    sym: Option<u32>,
    spans: Vec<u32>,
}

struct Sym<'a> {
    name: &'a str,
    val: Option<i64>,
    scope: Option<u32>,
    is_label: bool,
    seg: Option<u32>,
}

struct CSym<'a> {
    name: &'a str,
    scope: u32,
    sc: &'a str,
    offs: i64,
    sym: Option<u32>,
}

struct LineRec {
    file: u32,
    line: u32,
    kind: i64,
    spans: Vec<u32>,
}

/// The runtime's argument-push routines a prologue may call.
const PUSH_ROUTINES: [&str; 4] = ["pusha", "pusha0", "pushax", "pusheax"];

/// Parse an ld65 debug file. Malformed or unrecognised records are skipped,
/// so a newer ld65 that adds records or fields still loads.
#[must_use]
// One pass per record kind, then the joins; splitting it would only move the
// tables into a struct for no reader's benefit.
#[allow(clippy::too_many_lines)]
pub fn load(text: &str, source_root: &Path) -> DebugInfo {
    let mut files: Vec<SourceFile> = Vec::new();
    let mut file_ix: HashMap<u32, usize> = HashMap::new();
    let mut file_mods: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut lib_mods: Vec<u32> = Vec::new();
    let mut segs: HashMap<u32, u32> = HashMap::new();
    let mut rw_segs: Vec<u32> = Vec::new();
    let mut spans: HashMap<u32, Span> = HashMap::new();
    let mut lines: Vec<LineRec> = Vec::new();
    let mut scopes: HashMap<u32, Scope<'_>> = HashMap::new();
    let mut syms: HashMap<u32, Sym<'_>> = HashMap::new();
    let mut csyms: Vec<CSym<'_>> = Vec::new();

    for raw in text.lines() {
        let Some(r) = Record::parse(raw) else {
            continue;
        };
        match r.kind {
            "file" => {
                if let (Some(id), Some(name)) = (r.id("id"), r.string("name")) {
                    file_ix.insert(id, files.len());
                    files.push(SourceFile {
                        recorded: name.to_owned(),
                        path: resolve(source_root, name),
                    });
                    file_mods.insert(id, r.ids("mod"));
                }
            }
            "mod" => {
                if let Some(id) = r.id("id")
                    && r.raw("lib").is_some()
                {
                    lib_mods.push(id);
                }
            }
            "seg" => {
                if let (Some(id), Some(start)) = (r.id("id"), r.id("start")) {
                    segs.insert(id, start);
                    if r.raw("type") == Some("rw") {
                        rw_segs.push(id);
                    }
                }
            }
            "span" => {
                if let (Some(id), Some(seg), Some(start)) = (r.id("id"), r.id("seg"), r.id("start"))
                {
                    let size = r.id("size").unwrap_or(0);
                    spans.insert(id, Span { seg, start, size });
                }
            }
            "line" => {
                if let (Some(file), Some(line)) = (r.id("file"), r.id("line")) {
                    lines.push(LineRec {
                        file,
                        line,
                        kind: r.num("type").unwrap_or(0),
                        spans: r.ids("span"),
                    });
                }
            }
            "scope" => {
                if let Some(id) = r.id("id") {
                    scopes.insert(
                        id,
                        Scope {
                            name: r.string("name").unwrap_or(""),
                            module: r.id("mod"),
                            is_function: r.raw("type") == Some("scope"),
                            sym: r.id("sym"),
                            spans: r.ids("span"),
                        },
                    );
                }
            }
            "sym" => {
                if let (Some(id), Some(name)) = (r.id("id"), r.string("name")) {
                    syms.insert(
                        id,
                        Sym {
                            name,
                            val: r.num("val"),
                            scope: r.id("scope"),
                            is_label: r.raw("type") == Some("lab"),
                            seg: r.id("seg"),
                        },
                    );
                }
            }
            "csym" => {
                if let (Some(name), Some(scope), Some(sc)) =
                    (r.string("name"), r.id("scope"), r.raw("sc"))
                {
                    csyms.push(CSym {
                        name,
                        scope,
                        sc,
                        offs: r.num("offs").unwrap_or(0),
                        sym: r.id("sym"),
                    });
                }
            }
            _ => {}
        }
    }

    let span_range = |id: u32| -> Option<(u32, u32)> {
        let s = spans.get(&id)?;
        let base = segs.get(&s.seg)?;
        let start = base + s.start;
        (s.size > 0 && start <= 0xFFFF).then_some((start, (start + s.size).min(0x1_0000)))
    };
    let is_lib_file = |file: u32| {
        file_mods
            .get(&file)
            .is_some_and(|m| !m.is_empty() && m.iter().all(|x| lib_mods.contains(x)))
    };

    // The line table: C lines when there are any, else non-library assembler.
    let has_c = lines.iter().any(|l| l.kind == 1);
    let mut claims: BTreeMap<u16, (Loc, u32)> = BTreeMap::new();
    let mut starts: BTreeMap<u16, (Loc, u32)> = BTreeMap::new();
    for l in &lines {
        let wanted = if has_c {
            l.kind == 1
        } else {
            l.kind == 0 && !is_lib_file(l.file)
        };
        let Some(&file) = file_ix.get(&l.file) else {
            continue;
        };
        if !wanted || l.line == 0 {
            continue;
        }
        let loc = Loc { file, line: l.line };
        for &sid in &l.spans {
            let Some((start, end)) = span_range(sid) else {
                continue;
            };
            let width = end - start;
            for a in start..end {
                // `start..end` is bounded by 0x1_0000 above.
                #[allow(clippy::cast_possible_truncation)]
                let a = a as u16;
                let e = claims.entry(a).or_insert((loc, width));
                if width < e.1 {
                    *e = (loc, width);
                }
            }
            #[allow(clippy::cast_possible_truncation)]
            let s = start as u16;
            let e = starts.entry(s).or_insert((loc, width));
            if width < e.1 {
                *e = (loc, width);
            }
        }
    }
    let ranges = ranges_from_claims(&claims);
    let stmts = starts.into_iter().map(|(a, (l, _))| (a, l)).collect();

    // The frame-base pointer: `c_sp` (cc65 after 2.19) or `sp` (2.19 and
    // earlier).
    let zp_sym = |name: &str| -> Option<u8> {
        syms.values()
            .filter(|s| s.name == name && s.is_label)
            .find_map(|s| s.val.and_then(|v| u8::try_from(v).ok()))
    };
    // (`spc` existed only between two cc65 commits on 2025-06-04.)
    let frame_ptr = zp_sym("c_sp")
        .or_else(|| zp_sym("sp"))
        .or_else(|| zp_sym("spc"));

    let push_routines = syms
        .values()
        .filter(|s| s.is_label && PUSH_ROUTINES.contains(&s.name))
        .filter_map(|s| s.val.and_then(|v| u16::try_from(v).ok()))
        .collect();

    // Functions: `type=scope` scopes that own a symbol.
    let mut functions = Vec::new();
    for (&sid, sc) in &scopes {
        // C functions and the program's own `.proc`s; the runtime
        // library's routines are not the program and would only add frame
        // hooks.
        if !sc.is_function || sc.module.is_none_or(|m| lib_mods.contains(&m)) {
            continue;
        }
        let Some(sym_id) = sc.sym else { continue };
        let Some(entry) = syms
            .get(&sym_id)
            .and_then(|s| s.val)
            .and_then(|v| u16::try_from(v).ok())
        else {
            continue;
        };
        let mut low = u32::from(entry);
        let mut high = low + 1;
        for &sp in &sc.spans {
            if let Some((s, e)) = span_range(sp) {
                low = low.min(s);
                high = high.max(e);
            }
        }
        let c_name = csyms.iter().find(|c| c.sym == Some(sym_id)).map_or_else(
            || sc.name.trim_start_matches('_').to_owned(),
            |c| c.name.to_owned(),
        );
        let mut vars = Vec::new();
        for c in csyms
            .iter()
            .filter(|c| c.scope == sid && c.sym != Some(sym_id))
        {
            let loc = match c.sc {
                "auto" => VarLoc::Frame(i32::try_from(c.offs).unwrap_or(i32::MAX)),
                "static" | "ext" => c
                    .sym
                    .and_then(|s| syms.get(&s))
                    .and_then(|s| s.val)
                    .and_then(|v| u16::try_from(v).ok())
                    .map_or_else(
                        || VarLoc::Unavailable("static with no resolved symbol".into()),
                        VarLoc::Absolute,
                    ),
                other => VarLoc::Unavailable(format!("cc65 storage class `{other}`")),
            };
            vars.push(Variable {
                name: c.name.to_owned(),
                ty: TypeInfo::cc65_untyped(),
                // cc65 gives parameters non-negative offsets and locals
                // negative ones.
                is_param: c.sc == "auto" && c.offs >= 0,
                loc,
            });
        }
        let frame_base = match frame_ptr {
            Some(zp) if vars.iter().any(|v| matches!(v.loc, VarLoc::Frame(_))) => {
                FrameBase::ZpPointer(zp)
            }
            _ => FrameBase::None,
        };
        // `low` started from a u16 and spans are clamped to 0x1_0000.
        #[allow(clippy::cast_possible_truncation)]
        functions.push(Function {
            name: c_name,
            low: low.min(0xFFFF) as u16,
            high,
            frame_hook: entry,
            frame_base,
            vars,
            decl: None,
        });
    }
    functions.sort_by_key(|f| f.low);

    // Globals: C-level `ext`/`static` csyms at file scope, then the
    // underscore-prefixed data labels cc65 emits for C globals it gives no
    // csym (it emits none for `counter` in the fixture), from modules that
    // are not library members.
    let mut globals: Vec<Variable> = Vec::new();
    for c in &csyms {
        let Some(scope) = scopes.get(&c.scope) else {
            continue;
        };
        if scope.is_function || !matches!(c.sc, "ext" | "static") {
            continue;
        }
        if let Some(addr) = c
            .sym
            .and_then(|s| syms.get(&s))
            .filter(|s| s.seg.is_some_and(|g| rw_segs.contains(&g)))
            .and_then(|s| s.val)
            .and_then(|v| u16::try_from(v).ok())
        {
            globals.push(Variable {
                name: c.name.to_owned(),
                ty: TypeInfo::cc65_untyped(),
                loc: VarLoc::Absolute(addr),
                is_param: false,
            });
        }
    }
    let mut data_syms: Vec<&Sym<'_>> = syms
        .values()
        .filter(|s| {
            s.is_label
                && s.name.len() > 1
                && s.name.starts_with('_')
                && s.seg.is_some_and(|g| rw_segs.contains(&g))
                && s.scope
                    .and_then(|sc| scopes.get(&sc))
                    .and_then(|sc| sc.module)
                    .is_some_and(|m| !lib_mods.contains(&m))
        })
        .collect();
    data_syms.sort_by_key(|s| s.name);
    for s in data_syms {
        let name = &s.name[1..];
        if globals.iter().any(|g| g.name == name) {
            continue;
        }
        if let Some(addr) = s.val.and_then(|v| u16::try_from(v).ok()) {
            globals.push(Variable {
                name: name.to_owned(),
                ty: TypeInfo::cc65_untyped(),
                loc: VarLoc::Absolute(addr),
                is_param: false,
            });
        }
    }

    DebugInfo {
        format: Format::Cc65,
        files,
        ranges,
        stmts,
        functions,
        globals,
        push_routines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-written excerpt in ld65's layout: one C file with a function
    /// whose prologue line and body line overlap an enclosing span.
    const SAMPLE: &str = "\
version\tmajor=2,minor=0
file\tid=0,name=\"main.c\",size=10,mtime=0x1,mod=0
file\tid=1,name=\"runtime/pusha.s\",size=10,mtime=0x1,mod=1
lib\tid=0,name=\"nes.lib\"
mod\tid=0,name=\"main.o\",file=0
mod\tid=1,name=\"pusha.o\",file=1,lib=0
seg\tid=0,name=\"CODE\",start=0x008000,size=0x0100,addrsize=absolute,type=ro
seg\tid=1,name=\"BSS\",start=0x006000,size=0x0010,addrsize=absolute,type=rw
seg\tid=2,name=\"ZEROPAGE\",start=0x000002,size=0x0002,addrsize=zeropage,type=rw
span\tid=0,seg=0,start=0,size=20
span\tid=1,seg=0,start=0,size=3
span\tid=2,seg=0,start=3,size=5
span\tid=3,seg=0,start=8,size=2
line\tid=0,file=0,line=4,type=1,span=1
line\tid=1,file=0,line=6,type=1,span=2
line\tid=2,file=0,line=7,type=1,span=3
line\tid=3,file=1,line=30,span=0
scope\tid=0,name=\"\",mod=0,size=20,span=0
scope\tid=1,name=\"_f\",mod=0,type=scope,size=20,parent=0,sym=1,span=0
scope\tid=2,name=\"\",mod=1,size=3
sym\tid=1,name=\"_f\",addrsize=absolute,size=20,scope=0,def=1,val=0x8000,seg=0,type=lab
sym\tid=2,name=\"_g\",addrsize=absolute,scope=0,def=2,val=0x6001,seg=1,type=lab
sym\tid=3,name=\"c_sp\",addrsize=zeropage,size=2,scope=2,def=3,val=0x2,seg=2,type=lab
sym\tid=4,name=\"pusha\",addrsize=absolute,scope=2,def=4,val=0x80F0,seg=0,type=lab
csym\tid=0,name=\"f\",scope=1,type=0,sc=static,sym=1
csym\tid=1,name=\"p\",scope=1,type=0,sc=auto
csym\tid=2,name=\"q\",scope=1,type=0,sc=auto,offs=-2
";

    #[test]
    fn c_lines_form_the_table_and_narrower_spans_win() {
        let d = load(SAMPLE, Path::new("/src"));
        assert_eq!(d.format, Format::Cc65);
        assert_eq!(d.line_at(0x8000).map(|l| l.line), Some(4));
        assert_eq!(d.line_at(0x8003).map(|l| l.line), Some(6));
        assert_eq!(d.line_at(0x8009).map(|l| l.line), Some(7));
        assert_eq!(
            d.line_at(0x800A),
            None,
            "the library's assembler line is not used"
        );
        let starts: Vec<(u16, u32)> = d.stmts.iter().map(|(&a, l)| (a, l.line)).collect();
        assert_eq!(starts, vec![(0x8000, 4), (0x8003, 6), (0x8008, 7)]);
        assert_eq!(d.files[0].path, Path::new("/src/main.c"));
    }

    #[test]
    fn an_enclosing_c_line_never_hides_the_statements_inside_it() {
        // A C line whose span covers the whole function (as a declaration
        // with an initialiser can) must not take over the bytes of the
        // narrower lines inside it, whichever order ld65 lists them in.
        for extra in ["line\tid=9,file=0,line=3,type=1,span=0\n", ""] {
            let text = SAMPLE.replacen("line\tid=0,", &format!("{extra}line\tid=0,"), 1);
            let d = load(&text, Path::new("/src"));
            assert_eq!(d.line_at(0x8003).map(|l| l.line), Some(6));
            assert_eq!(d.line_at(0x8009).map(|l| l.line), Some(7));
            assert_eq!(d.stmt_at(0x8000).map(|l| l.line), Some(4));
            if !extra.is_empty() {
                assert_eq!(
                    d.line_at(0x8012).map(|l| l.line),
                    Some(3),
                    "only line 3 covers $8012"
                );
            }
        }
    }

    #[test]
    fn functions_carry_frame_locals_and_the_soft_stack_pointer() {
        let d = load(SAMPLE, Path::new("/src"));
        assert_eq!(d.functions.len(), 1);
        let f = &d.functions[0];
        assert_eq!(f.name, "f");
        assert_eq!((f.low, f.high), (0x8000, 0x8014));
        assert_eq!(f.frame_base, FrameBase::ZpPointer(0x02));
        let names: Vec<(&str, &VarLoc, bool)> = f
            .vars
            .iter()
            .map(|v| (v.name.as_str(), &v.loc, v.is_param))
            .collect();
        assert_eq!(
            names,
            vec![
                ("p", &VarLoc::Frame(0), true),
                ("q", &VarLoc::Frame(-2), false)
            ]
        );
        assert_eq!(d.push_routines, vec![0x80F0]);
    }

    #[test]
    fn underscore_data_labels_become_globals() {
        let d = load(SAMPLE, Path::new("/src"));
        let g = d.global_named("g").expect("global g");
        assert_eq!(g.loc, VarLoc::Absolute(0x6001));
        assert!(
            d.global_named("f").is_none(),
            "a code label is not a global"
        );
    }

    #[test]
    fn the_older_sp_name_is_accepted() {
        let text = SAMPLE.replace("name=\"c_sp\"", "name=\"sp\"");
        let d = load(&text, Path::new("/src"));
        assert_eq!(d.functions[0].frame_base, FrameBase::ZpPointer(0x02));
    }

    #[test]
    fn quoted_commas_and_negative_numbers_parse() {
        let r = Record::parse("file\tid=3,name=\"a,b.c\",offs=-0x2").expect("record");
        assert_eq!(r.string("name"), Some("a,b.c"));
        assert_eq!(r.num("offs"), Some(-2));
        assert_eq!(r.id("id"), Some(3));
        assert_eq!(r.raw("missing"), None);
    }

    #[test]
    fn garbage_loads_as_empty() {
        let d = load(
            "version\tmajor=2\nnonsense\nline\tfile=9,line=1,span=9\n",
            Path::new("."),
        );
        assert!(d.ranges.is_empty() && d.functions.is_empty() && d.globals.is_empty());
    }
}

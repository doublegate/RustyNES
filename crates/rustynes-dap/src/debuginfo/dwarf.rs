// SPDX-License-Identifier: GPL-3.0-or-later
//! The llvm-mos reader: DWARF 5 in an ELF32 `EM_MOS` executable.
//!
//! llvm-mos's NES targets (`mos-nes-*-clang`) link an ELF and write the
//! `.nes` image beside it (`game.nes` + `game.nes.elf`); the ELF carries the
//! DWARF. Parsing is done by `gimli` (line programs, DIEs) over sections
//! `object` finds; what is specific to the 6502 is done here:
//!
//! - **Register numbers.** llvm-mos publishes them in its *DWARF
//!   Specification for MOS-Compatible Processors* (llvm-mos.org): `A`=0,
//!   `X`=1, `Y`=2, `P`=3, `S`=4, `PC`=5, the 8-bit imaginary registers
//!   `RCn` at `0x0002_0000 + n` and the 16-bit `RSn` at `0x0003_0000 + n`
//!   (the pair `RC(2n)`, `RC(2n+1)`). Those numbers do not fit gimli's
//!   16-bit `Register`, so location expressions are decoded here, by a
//!   small reader of the DWARF 5 operations a 6502 compiler emits for
//!   variables (`decode`, private to this module).
//! - **Where `RCn` lives.** The ELF symbol `__rcN` is its zero-page address
//!   (the fixture links `__rc0` at `$00`), so the register file is read
//!   from the symbol table, not assumed.
//!
//! Line table: every row of a sequence covers `[row, next row)`. Rows with
//! line 0 (compiler-generated code) attribute to nothing; `is_stmt` rows
//! with a real line are statement starts. A function's frame hook is its
//! first `prologue_end` row, or its low PC when there is none.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use gimli::{AttributeValue, EndianSlice, LittleEndian, SectionId, UnitOffset};
use object::{Object as _, ObjectSection as _, ObjectSymbol as _};

use super::{
    DebugInfo, Encoding, Format, FrameBase, Function, LineRange, LoadError, Loc, SourceFile,
    TypeInfo, VarLoc, Variable, resolve,
};

type R<'a> = EndianSlice<'a, LittleEndian>;
type Unit<'a> = gimli::Unit<R<'a>>;

/// `EM_MOS`, the ELF machine number llvm-mos uses.
const EM_MOS: u16 = 6502;
/// First DWARF number of the `RCn` (8-bit imaginary register) block.
const RC_BASE: u64 = 0x0002_0000;
/// First DWARF number of the `RSn` (16-bit imaginary register) block.
const RS_BASE: u64 = 0x0003_0000;
/// Deepest type chain followed (typedef of const of pointer ...).
const MAX_TYPE_DEPTH: usize = 16;

fn err(e: impl std::fmt::Display) -> LoadError {
    LoadError(format!("DWARF: {e}"))
}

/// The zero-page addresses of the imaginary registers, from `__rcN`.
struct Regs(HashMap<u64, u8>);

impl Regs {
    /// The zero-page address a DWARF register number names, and the size of
    /// the register.
    fn zp(&self, regno: u64) -> Option<(u8, u16)> {
        if (RC_BASE..RC_BASE + 256).contains(&regno) {
            self.0.get(&(regno - RC_BASE)).map(|&a| (a, 1))
        } else if (RS_BASE..RS_BASE + 128).contains(&regno) {
            self.0.get(&((regno - RS_BASE) * 2)).map(|&a| (a, 2))
        } else {
            None
        }
    }
}

/// The imaginary registers an llvm-mos ELF defines.
///
/// One `(n, zero-page address)` per `__rcN` symbol, sorted by `n`. The GDB stub exposes
/// them as registers so a client that knows llvm-mos's register numbers can
/// evaluate `DW_OP_regx` / `DW_OP_fbreg` itself.
///
/// # Errors
///
/// Not an ELF32 for `EM_MOS`.
pub fn imaginary_registers(bytes: &[u8]) -> Result<Vec<(u32, u8)>, LoadError> {
    let elf = object::read::elf::ElfFile32::<object::Endianness>::parse(bytes).map_err(err)?;
    let machine = elf.elf_header().e_machine.get(elf.endian()).0;
    if machine != EM_MOS {
        return Err(LoadError(format!(
            "ELF machine {machine} is not EM_MOS ({EM_MOS})"
        )));
    }
    let mut out: Vec<(u32, u8)> = elf
        .symbols()
        .filter_map(|sym| {
            let n = sym.name().ok()?.strip_prefix("__rc")?.parse::<u32>().ok()?;
            Some((n, u8::try_from(sym.address()).ok()?))
        })
        .collect();
    out.sort_unstable();
    out.dedup_by_key(|r| r.0);
    Ok(out)
}

/// Load an llvm-mos ELF.
///
/// # Errors
///
/// Not an ELF32 for `EM_MOS`, no `.debug_info`, or gimli refused a section.
pub fn load(bytes: &[u8], source_root: &Path) -> Result<DebugInfo, LoadError> {
    let elf = object::read::elf::ElfFile32::<object::Endianness>::parse(bytes).map_err(err)?;
    let machine = elf.elf_header().e_machine.get(elf.endian()).0;
    if machine != EM_MOS {
        return Err(LoadError(format!(
            "ELF machine {machine} is not EM_MOS ({EM_MOS}): not an llvm-mos program"
        )));
    }
    if elf.section_by_name(".debug_info").is_none() {
        return Err(LoadError(
            "the ELF has no .debug_info (built without -g?)".into(),
        ));
    }
    let mut rc = HashMap::new();
    for sym in elf.symbols() {
        if let Ok(name) = sym.name()
            && let Some(n) = name.strip_prefix("__rc")
            && let Ok(n) = n.parse::<u64>()
            && let Ok(addr) = u8::try_from(sym.address())
        {
            rc.insert(n, addr);
        }
    }
    let regs = Regs(rc);

    let section = |id: SectionId| -> Result<R<'_>, LoadError> {
        let data = elf
            .section_by_name(id.name())
            .map(|s| s.data().map_err(err))
            .transpose()?
            .unwrap_or(&[]);
        Ok(EndianSlice::new(data, LittleEndian))
    };
    let dwarf = gimli::Dwarf::load(section)?;

    let mut b = Builder {
        files: Vec::new(),
        file_ix: HashMap::new(),
        claims: BTreeMap::new(),
        stmts: BTreeMap::new(),
        prologue_ends: Vec::new(),
        functions: Vec::new(),
        globals: Vec::new(),
        source_root,
    };
    let mut units = dwarf.units();
    while let Some(header) = units.next().map_err(err)? {
        let unit = dwarf.unit(header).map_err(err)?;
        b.lines(&dwarf, &unit)?;
        b.dies(&dwarf, &unit, &regs)?;
    }
    Ok(b.finish())
}

struct Builder<'p> {
    files: Vec<SourceFile>,
    file_ix: HashMap<String, usize>,
    claims: BTreeMap<u16, (Loc, u32)>,
    stmts: BTreeMap<u16, Loc>,
    prologue_ends: Vec<u16>,
    functions: Vec<Function>,
    globals: Vec<Variable>,
    source_root: &'p Path,
}

impl Builder<'_> {
    fn file(&mut self, recorded: String) -> usize {
        if let Some(&i) = self.file_ix.get(&recorded) {
            return i;
        }
        let i = self.files.len();
        self.files.push(SourceFile {
            path: resolve(self.source_root, &recorded),
            recorded: recorded.clone(),
        });
        self.file_ix.insert(recorded, i);
        i
    }

    fn lines(&mut self, dwarf: &gimli::Dwarf<R<'_>>, unit: &Unit<'_>) -> Result<(), LoadError> {
        let Some(program) = unit.line_program.clone() else {
            return Ok(());
        };
        let mut rows = program.rows();
        // The previous row of the current sequence: (address, loc or None).
        let mut prev: Option<(u64, Option<Loc>)> = None;
        while let Some((header, row)) = rows.next_row().map_err(err)? {
            let addr = row.address();
            if let Some((start, Some(loc))) = prev {
                for a in start..addr.min(0x1_0000) {
                    // Bounded by 0x1_0000 just above.
                    #[allow(clippy::cast_possible_truncation)]
                    self.claims.entry(a as u16).or_insert((loc, 0));
                }
            }
            if row.end_sequence() {
                prev = None;
                continue;
            }
            let loc = match (row.line(), row.file(header)) {
                (Some(line), Some(entry)) => {
                    let mut name = dwarf
                        .attr_string(unit, entry.path_name())
                        .map_err(err)?
                        .to_string_lossy()
                        .into_owned();
                    if !Path::new(&name).is_absolute()
                        && let Some(dir) = entry.directory(header)
                    {
                        let dir = dwarf
                            .attr_string(unit, dir)
                            .map_err(err)?
                            .to_string_lossy()
                            .into_owned();
                        if !dir.is_empty() && dir != "." {
                            name = format!("{dir}/{name}");
                        }
                    }
                    let file = self.file(name);
                    u32::try_from(line.get())
                        .ok()
                        .map(|line| Loc { file, line })
                }
                _ => None,
            };
            if let (Some(loc), Ok(a)) = (loc, u16::try_from(addr)) {
                if row.is_stmt() {
                    self.stmts.entry(a).or_insert(loc);
                }
                if row.prologue_end() {
                    self.prologue_ends.push(a);
                }
            }
            prev = Some((addr, loc));
        }
        Ok(())
    }

    fn dies(
        &mut self,
        dwarf: &gimli::Dwarf<R<'_>>,
        unit: &Unit<'_>,
        regs: &Regs,
    ) -> Result<(), LoadError> {
        let mut tree = unit.entries_tree(None).map_err(err)?;
        let root = tree.root().map_err(err)?;
        let mut children = root.children();
        while let Some(child) = children.next().map_err(err)? {
            let entry = child.entry();
            match entry.tag() {
                gimli::DW_TAG_subprogram => {
                    if let Some(f) = subprogram(dwarf, unit, child, regs)? {
                        self.functions.push(f);
                    }
                }
                gimli::DW_TAG_variable => {
                    if let Some(v) = variable(dwarf, unit, entry, regs)? {
                        self.globals.push(v);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn finish(mut self) -> DebugInfo {
        self.functions.sort_by_key(|f| f.low);
        for f in &mut self.functions {
            if let Some(&pe) = self.prologue_ends.iter().filter(|&&a| f.contains(a)).min() {
                f.frame_hook = pe;
            }
        }
        let mut ranges: Vec<LineRange> = Vec::new();
        for (&a, &(loc, _)) in &self.claims {
            if let Some(last) = ranges.last_mut()
                && last.loc == loc
                && last.end == u32::from(a)
            {
                last.end += 1;
                continue;
            }
            ranges.push(LineRange {
                start: a,
                end: u32::from(a) + 1,
                loc,
            });
        }
        DebugInfo {
            format: Format::Dwarf,
            files: self.files,
            ranges,
            stmts: self.stmts,
            functions: self.functions,
            globals: self.globals,
            push_routines: Vec::new(),
        }
    }
}

fn name_of(
    dwarf: &gimli::Dwarf<R<'_>>,
    unit: &Unit<'_>,
    entry: &gimli::DebuggingInformationEntry<R<'_>>,
) -> Result<Option<String>, LoadError> {
    let Some(v) = entry.attr_value(gimli::DW_AT_name) else {
        return Ok(None);
    };
    let s = dwarf.attr_string(unit, v).map_err(err)?;
    Ok(Some(s.to_string_lossy().into_owned()))
}

fn u_attr(entry: &gimli::DebuggingInformationEntry<R<'_>>, at: gimli::DwAt) -> Option<u64> {
    entry.attr_value(at).and_then(|v| v.udata_value())
}

fn subprogram(
    dwarf: &gimli::Dwarf<R<'_>>,
    unit: &Unit<'_>,
    node: gimli::EntriesTreeNode<'_, '_, R<'_>>,
    regs: &Regs,
) -> Result<Option<Function>, LoadError> {
    let entry = node.entry();
    let Some(low) = entry
        .attr_value(gimli::DW_AT_low_pc)
        .map(|v| dwarf.attr_address(unit, v))
        .transpose()
        .map_err(err)?
        .flatten()
    else {
        return Ok(None); // a declaration, or inlined only
    };
    let high = match entry.attr_value(gimli::DW_AT_high_pc) {
        Some(AttributeValue::Addr(a)) => a,
        Some(v) => low + v.udata_value().unwrap_or(1),
        None => low + 1,
    };
    let Ok(low16) = u16::try_from(low) else {
        return Ok(None);
    };
    let name = name_of(dwarf, unit, entry)?.unwrap_or_else(|| format!("${low16:04X}"));
    let frame_base = match entry.attr_value(gimli::DW_AT_frame_base) {
        Some(AttributeValue::Exprloc(e)) => match decode(e.0.slice()) {
            Some(Op::Reg(r)) => regs
                .zp(r)
                .filter(|&(_, size)| size == 2)
                .map_or(FrameBase::None, |(zp, _)| FrameBase::ZpPointer(zp)),
            _ => FrameBase::None,
        },
        _ => FrameBase::None,
    };
    let mut vars = Vec::new();
    collect_vars(dwarf, unit, node, regs, &mut vars, 0)?;
    Ok(Some(Function {
        name,
        low: low16,
        high: u32::try_from(high.min(0x1_0000)).unwrap_or(0x1_0000),
        frame_hook: low16,
        frame_base,
        vars,
        decl: None,
    }))
}

/// Variables and parameters of a subprogram, including those in nested
/// lexical blocks (shown together: a 6502 function is small, and a block's
/// range check would hide a variable a user is stepping towards).
fn collect_vars(
    dwarf: &gimli::Dwarf<R<'_>>,
    unit: &Unit<'_>,
    node: gimli::EntriesTreeNode<'_, '_, R<'_>>,
    regs: &Regs,
    out: &mut Vec<Variable>,
    depth: usize,
) -> Result<(), LoadError> {
    let mut children = node.children();
    while let Some(child) = children.next().map_err(err)? {
        let tag = child.entry().tag();
        if tag == gimli::DW_TAG_variable || tag == gimli::DW_TAG_formal_parameter {
            if let Some(mut v) = variable(dwarf, unit, child.entry(), regs)? {
                v.is_param = tag == gimli::DW_TAG_formal_parameter;
                out.push(v);
            }
        } else if tag == gimli::DW_TAG_lexical_block && depth < 8 {
            collect_vars(dwarf, unit, child, regs, out, depth + 1)?;
        }
    }
    Ok(())
}

fn variable(
    dwarf: &gimli::Dwarf<R<'_>>,
    unit: &Unit<'_>,
    entry: &gimli::DebuggingInformationEntry<R<'_>>,
    regs: &Regs,
) -> Result<Option<Variable>, LoadError> {
    let Some(name) = name_of(dwarf, unit, entry)? else {
        return Ok(None);
    };
    let ty = match entry.attr_value(gimli::DW_AT_type) {
        Some(AttributeValue::UnitRef(off)) => type_info(dwarf, unit, off, 0)?,
        _ => TypeInfo {
            name: "void".into(),
            size: 1,
            encoding: Encoding::Bytes,
        },
    };
    let loc = match entry.attr_value(gimli::DW_AT_location) {
        None => VarLoc::Unavailable("optimized out".into()),
        Some(AttributeValue::Exprloc(e)) => match decode(e.0.slice()) {
            Some(Op::Addr(a)) => u16::try_from(a).map_or_else(
                |_| VarLoc::Unavailable("address above $FFFF".into()),
                VarLoc::Absolute,
            ),
            Some(Op::AddrIndex(i)) => {
                let a = dwarf
                    .address(
                        unit,
                        gimli::DebugAddrIndex(usize::try_from(i).unwrap_or(usize::MAX)),
                    )
                    .map_err(err)?;
                u16::try_from(a).map_or_else(
                    |_| VarLoc::Unavailable("address above $FFFF".into()),
                    VarLoc::Absolute,
                )
            }
            Some(Op::FrameOffset(o)) => VarLoc::Frame(i32::try_from(o).unwrap_or(i32::MAX)),
            Some(Op::Reg(r)) => regs.zp(r).map_or_else(
                || VarLoc::Unavailable(format!("in register {r:#x}")),
                |(zp, _)| VarLoc::Absolute(u16::from(zp)),
            ),
            None => VarLoc::Unavailable("location expression not supported".into()),
        },
        Some(_) => VarLoc::Unavailable("location list not supported".into()),
    };
    Ok(Some(Variable {
        name,
        ty,
        loc,
        is_param: false,
    }))
}

fn type_info(
    dwarf: &gimli::Dwarf<R<'_>>,
    unit: &Unit<'_>,
    off: UnitOffset,
    depth: usize,
) -> Result<TypeInfo, LoadError> {
    let opaque = |name: &str| TypeInfo {
        name: name.into(),
        size: 1,
        encoding: Encoding::Bytes,
    };
    if depth > MAX_TYPE_DEPTH {
        return Ok(opaque("<type chain too deep>"));
    }
    let entry = unit.entry(off).map_err(err)?;
    let name = name_of(dwarf, unit, &entry)?;
    let size = u_attr(&entry, gimli::DW_AT_byte_size)
        .and_then(|s| u16::try_from(s).ok())
        .filter(|&s| s > 0);
    let inner = |depth: usize| -> Result<Option<TypeInfo>, LoadError> {
        match entry.attr_value(gimli::DW_AT_type) {
            Some(AttributeValue::UnitRef(o)) => type_info(dwarf, unit, o, depth + 1).map(Some),
            _ => Ok(None),
        }
    };
    Ok(match entry.tag() {
        gimli::DW_TAG_base_type => {
            let enc = match entry.attr_value(gimli::DW_AT_encoding) {
                Some(AttributeValue::Encoding(e)) => e,
                _ => gimli::DW_ATE_unsigned,
            };
            let encoding = match enc {
                gimli::DW_ATE_signed | gimli::DW_ATE_signed_char => Encoding::Signed,
                gimli::DW_ATE_boolean => Encoding::Bool,
                gimli::DW_ATE_unsigned | gimli::DW_ATE_unsigned_char => Encoding::Unsigned,
                _ => Encoding::Bytes,
            };
            TypeInfo {
                name: name.unwrap_or_else(|| "base".into()),
                size: size.unwrap_or(1),
                encoding,
            }
        }
        gimli::DW_TAG_pointer_type => TypeInfo {
            name: format!(
                "{} *",
                inner(depth)?.map_or_else(|| "void".into(), |t| t.name)
            ),
            size: size.unwrap_or(2),
            encoding: Encoding::Pointer,
        },
        gimli::DW_TAG_typedef => {
            let t = inner(depth)?.unwrap_or_else(|| opaque("void"));
            TypeInfo {
                name: name.unwrap_or(t.name),
                ..t
            }
        }
        gimli::DW_TAG_const_type | gimli::DW_TAG_volatile_type => {
            let q = if entry.tag() == gimli::DW_TAG_const_type {
                "const"
            } else {
                "volatile"
            };
            let t = inner(depth)?.unwrap_or_else(|| opaque("void"));
            TypeInfo {
                name: format!("{q} {}", t.name),
                ..t
            }
        }
        gimli::DW_TAG_enumeration_type => TypeInfo {
            name: format!("enum {}", name.unwrap_or_default()),
            size: size.unwrap_or(1),
            encoding: Encoding::Unsigned,
        },
        gimli::DW_TAG_array_type => {
            let elem = inner(depth)?.unwrap_or_else(|| opaque("?"));
            let count = array_count(unit, off)?;
            let total = count.map_or(elem.size, |c| {
                u16::try_from(u64::from(elem.size) * c).unwrap_or(u16::MAX)
            });
            TypeInfo {
                name: format!(
                    "{}[{}]",
                    elem.name,
                    count.map_or_else(String::new, |c| c.to_string())
                ),
                size: total.max(1),
                encoding: Encoding::Bytes,
            }
        }
        _ => TypeInfo {
            name: name.unwrap_or_else(|| entry.tag().static_string().unwrap_or("?").into()),
            size: size.unwrap_or(1),
            encoding: Encoding::Bytes,
        },
    })
}

fn array_count(unit: &Unit<'_>, off: UnitOffset) -> Result<Option<u64>, LoadError> {
    let mut tree = unit.entries_tree(Some(off)).map_err(err)?;
    let root = tree.root().map_err(err)?;
    let mut children = root.children();
    while let Some(child) = children.next().map_err(err)? {
        let e = child.entry();
        if e.tag() == gimli::DW_TAG_subrange_type {
            if let Some(c) = u_attr(e, gimli::DW_AT_count) {
                return Ok(Some(c));
            }
            if let Some(u) = u_attr(e, gimli::DW_AT_upper_bound) {
                return Ok(Some(u + 1));
            }
        }
    }
    Ok(None)
}

/// One decoded single-operation location expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Op {
    /// `DW_OP_addr`.
    Addr(u64),
    /// `DW_OP_addrx`: an index into `.debug_addr`.
    AddrIndex(u64),
    /// `DW_OP_fbreg`.
    FrameOffset(i64),
    /// `DW_OP_reg0..31` / `DW_OP_regx`: the value is in a register.
    Reg(u64),
}

/// Decode a location expression that is exactly one of the operations a
/// 6502 compiler uses for a variable's whole lifetime (DWARF 5 section 2.5
/// and 2.6 opcode values). Anything else, including a composite
/// (`DW_OP_piece`) or a computed location, is `None` and the variable is
/// reported unavailable rather than guessed at.
fn decode(bytes: &[u8]) -> Option<Op> {
    let (&op, rest) = bytes.split_first()?;
    let (value, rest) = match op {
        0x03 => {
            // DW_OP_addr: an ELF32 target address, 4 bytes little-endian.
            let (a, rest) = rest.split_at_checked(4)?;
            (
                Op::Addr(u64::from(u32::from_le_bytes(a.try_into().ok()?))),
                rest,
            )
        }
        0xa1 => {
            let (i, rest) = uleb(rest)?;
            (Op::AddrIndex(i), rest)
        }
        0x91 => {
            let (o, rest) = sleb(rest)?;
            (Op::FrameOffset(o), rest)
        }
        0x50..=0x6f => (Op::Reg(u64::from(op - 0x50)), rest),
        0x90 => {
            let (r, rest) = uleb(rest)?;
            (Op::Reg(r), rest)
        }
        _ => return None,
    };
    rest.is_empty().then_some(value)
}

fn uleb(bytes: &[u8]) -> Option<(u64, &[u8])> {
    let mut v: u64 = 0;
    for (i, &b) in bytes.iter().enumerate().take(10) {
        v |= u64::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            return Some((v, &bytes[i + 1..]));
        }
    }
    None
}

fn sleb(bytes: &[u8]) -> Option<(i64, &[u8])> {
    let mut v: i64 = 0;
    let mut shift = 0;
    for (i, &b) in bytes.iter().enumerate().take(10) {
        v |= i64::from(b & 0x7f) << shift;
        shift += 7;
        if b & 0x80 == 0 {
            if shift < 64 && b & 0x40 != 0 {
                v |= -1_i64 << shift;
            }
            return Some((v, &bytes[i + 1..]));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_operations_llvm_mos_emits() {
        // DW_OP_regx RS0 (0x30000 = ULEB 80 80 0C), the fixture's frame base.
        assert_eq!(decode(&[0x90, 0x80, 0x80, 0x0c]), Some(Op::Reg(0x3_0000)));
        assert_eq!(decode(&[0x91, 0x00]), Some(Op::FrameOffset(0)));
        assert_eq!(decode(&[0x91, 0x01]), Some(Op::FrameOffset(1)));
        assert_eq!(decode(&[0x91, 0x7f]), Some(Op::FrameOffset(-1)));
        assert_eq!(decode(&[0xa1, 0x02]), Some(Op::AddrIndex(2)));
        assert_eq!(
            decode(&[0x03, 0x20, 0x00, 0x00, 0x00]),
            Some(Op::Addr(0x20))
        );
        assert_eq!(decode(&[0x51]), Some(Op::Reg(1)));
    }

    #[test]
    fn refuses_composites_and_trailing_operations() {
        // DW_OP_fbreg 0; DW_OP_piece 1.
        assert_eq!(decode(&[0x91, 0x00, 0x93, 0x01]), None);
        assert_eq!(decode(&[0x03, 0x20]), None, "truncated address");
        assert_eq!(decode(&[]), None);
        assert_eq!(decode(&[0x9f]), None, "DW_OP_stack_value");
    }

    #[test]
    fn imaginary_registers_map_through_the_rc_symbols() {
        let regs = Regs(
            [(0, 0x00), (1, 0x01), (2, 0x02), (3, 0x03)]
                .into_iter()
                .collect(),
        );
        assert_eq!(regs.zp(RC_BASE + 2), Some((0x02, 1)));
        assert_eq!(regs.zp(RS_BASE), Some((0x00, 2)));
        assert_eq!(regs.zp(RS_BASE + 1), Some((0x02, 2)));
        assert_eq!(regs.zp(RS_BASE + 2), None, "no __rc4 symbol");
        assert_eq!(regs.zp(4), None, "S is not an imaginary register");
    }

    #[test]
    fn rejects_a_non_mos_elf() {
        // A minimal ELF32 header for EM_386 (3).
        let mut h = vec![0u8; 52];
        h[..4].copy_from_slice(b"\x7fELF");
        h[4] = 1; // ELFCLASS32
        h[5] = 1; // little-endian
        h[6] = 1;
        h[18] = 3;
        h[20] = 1;
        h[40] = 52;
        let e = load(&h, Path::new(".")).expect_err("not MOS");
        assert!(e.0.contains("EM_MOS") || e.0.contains("DWARF"), "{e}");
    }
}

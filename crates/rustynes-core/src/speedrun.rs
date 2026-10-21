//! v3.5.0 (`T-SPEEDRUN-MEMIF`) — speedrun auto-timing on RAM predicates,
//! counted in master-clock cycles.
//!
//! A speedrun timer has to answer two questions every frame: has the run
//! started (or reached its next split, or been reset), and how long has it
//! taken. `RustyNES` answers the first with **RAM predicates written in the
//! `RetroAchievements` condition grammar** (the `0xH0770=1_d0xH0770=0` strings
//! achievement sets are written in, so a condition copied from an achievement
//! set or a splitter's notes works unchanged), and the second with **the
//! console's own master clock**: the time of a run is the number of master-
//! clock cycles between the frame its start predicate fired and the frame its
//! last split fired.
//!
//! # Why the master clock, not frames or the wall clock
//!
//! - **Wall-clock time** depends on the host (a stutter, fast-forward, a
//!   paused window). A run timed from a movie must give the same number on
//!   every machine and every replay, and only emulated time can.
//! - **Frames** are not all the same length. An NTSC frame is 89,342 or
//!   89,341 PPU dots depending on the odd-frame skip, the extra-scanline
//!   option lengthens them, and a lag frame is still a frame. The master clock
//!   counts what the console actually ran: [`crate::Nes`]'s CPU master clock,
//!   which advances by the region's divider (12 NTSC, 16 PAL, 15 Dendy) on
//!   every CPU cycle. A difference of two readings is exact, and seconds follow
//!   from the region's master-clock frequency ([`master_clock_hz`]).
//!
//! The predicates are evaluated once per frame, after the frame (the same
//! cadence `RetroAchievements` evaluates achievements at), so a split's time
//! is resolved to the end of the frame its condition first held in. That is
//! the resolution every RAM-driven splitter has; what this adds is that the
//! frame boundary itself is exact.
//!
//! # The condition grammar (the subset implemented)
//!
//! Written from the `RetroAchievements` developer documentation of the
//! condition syntax (public prose; no implementation was consulted). A
//! condition set is one or more **groups** separated by `S`; the set is true
//! when the first (core) group is true and, if there are more groups, at least
//! one of them is. A group is **conditions** separated by `_`, all of which
//! must hold. A condition is
//!
//! ```text
//! operand  comparison  operand  [ "." hits "." ]
//! ```
//!
//! | Piece | Accepted |
//! | --- | --- |
//! | memory operand | `0x` + size + hex address. Sizes: `M`..`T` bit 0..7, `L` low nibble, `U` high nibble, `H` 8-bit, none (or a space) 16-bit, `W` 24-bit, `X` 32-bit (little-endian), `I` / `J` / `G` 16 / 24 / 32-bit big-endian, `K` bit count of a byte |
//! | operand prefix | `d` delta (the value at the previous evaluation), `p` prior (the value before the last change), `b` BCD, `~` bitwise invert (within the operand's size) |
//! | constant | decimal (`12`) or `h` + hex (`h1F`) |
//! | comparison | `=` `!=` `<` `<=` `>` `>=` |
//! | hit target | `.N.`: the condition must have been true on `N` evaluations (counted while its trigger is armed) |
//!
//! Addresses are the `RetroAchievements` NES flat space, the same one
//! `rustynes-cheevos` maps (`ra_addr_to_nes`): `0x0000..=0x07FF` is the 2 KiB
//! system RAM, `0x0800..=0x27FF` is cartridge RAM at `$6000..=$7FFF`, and
//! anything else reads 0. [`ra_flat_to_cpu`] is the mapping; a test in the
//! frontend pins it to the cheevos crate's.
//!
//! **Not implemented**, and refused with a message rather than misread:
//! condition flags (`R:` reset-if, `P:` pause-if, `A:` add-source, `B:`,
//! `C:`, `N:`, `M:`, `Q:`, `I:`, `T:`, `Z:`, `O:`, `D:`, `K:`), float and
//! negative constants, arithmetic operators (`*`, `/`, `&`, `^`, `%`, `+`,
//! `-`), and the `f` float sizes. A splits file has its own `reset` predicate,
//! which covers the common use of `R:`.
//!
//! # The splits definition
//!
//! A small line-based text format ([`SplitsDefinition::parse`]):
//!
//! ```text
//! # comments and blank lines are ignored
//! game = Super Mario Bros.
//! category = Any%
//! start = 0xH0770=1_d0xH0770=0
//! split = World 1 | 0xH075f=1_d0xH075f=0
//! split = Bowser | 0xH0770=2
//! reset = 0xH0770=0
//! ```
//!
//! `start` and at least one `split` are required; the last split ends the
//! run. `reset` is optional. Keys are split from values at the FIRST `=`, so a
//! condition keeps its own `=` signs; a split's name and condition are split
//! at the first `|`.
//!
//! # Determinism
//!
//! The timer only reads: a `FnMut(u32) -> u8` peek closure (side-effect-free
//! on the NES: [`crate::Nes::peek`]) and two counters. It never writes the
//! console, so attaching it cannot change a frame, a sample or a bus cycle,
//! and the same movie replayed twice produces the same split times
//! (`crates/rustynes-test-harness/tests/speedrun_timer.rs` checks both).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write as _;

use crate::Region;

/// The master-clock frequency of `region` as an exact fraction
/// `(numerator, denominator)` in hertz.
///
/// NTSC: 236.25 MHz / 11 = 21.477272... MHz (six times the 315/88 MHz colour
/// subcarrier). PAL and Dendy: 26.6017125 MHz (six times the 4.43361875 MHz
/// subcarrier). The rounded figures are in `ref-docs/nesdev-wiki-technical-
/// report.md`'s clock table (21.477272 / 26.601712 MHz, dividers 12 / 16 / 15).
#[must_use]
pub const fn master_clock_hz_ratio(region: Region) -> (u64, u64) {
    match region {
        Region::Ntsc => (236_250_000, 11),
        Region::Pal | Region::Dendy => (53_203_425, 2),
    }
}

/// The master-clock frequency of `region` in hertz.
#[must_use]
#[allow(clippy::cast_precision_loss)] // both terms are far below 2^53.
pub fn master_clock_hz(region: Region) -> f64 {
    let (n, d) = master_clock_hz_ratio(region);
    n as f64 / d as f64
}

/// `cycles` master-clock cycles of `region`, in seconds.
#[must_use]
#[allow(clippy::cast_precision_loss)] // a run of 2^53 cycles is 13 years.
pub fn cycles_to_seconds(cycles: u64, region: Region) -> f64 {
    let (n, d) = master_clock_hz_ratio(region);
    cycles as f64 * d as f64 / n as f64
}

/// A `RetroAchievements` NES flat address as a CPU-bus address (`None` for an
/// address outside the two mapped windows). See the module docs.
#[must_use]
#[allow(clippy::cast_possible_truncation)] // both casts are range-checked.
pub const fn ra_flat_to_cpu(addr: u32) -> Option<u16> {
    if addr < 0x0800 {
        Some(addr as u16)
    } else if addr < 0x0800 + 0x2000 {
        Some(0x6000 + (addr - 0x0800) as u16)
    } else {
        None
    }
}

/// Why a condition or a splits file was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// What was wrong, in words, naming the offending text.
    pub message: String,
    /// The 1-based line of a splits file, or 0 for a bare condition.
    pub line: usize,
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.line == 0 {
            f.write_str(&self.message)
        } else {
            write!(f, "line {}: {}", self.line, self.message)
        }
    }
}

fn err(message: impl Into<String>) -> ParseError {
    ParseError {
        message: message.into(),
        line: 0,
    }
}

/// The width of a memory operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Size {
    Bit(u8),
    Low4,
    High4,
    Byte,
    BitCount,
    Le(u8),
    Be(u8),
}

impl Size {
    /// The largest value this size can hold (for `~`).
    const fn mask(self) -> u32 {
        match self {
            Self::Bit(_) => 1,
            Self::Low4 | Self::High4 => 0xF,
            Self::Byte | Self::BitCount => 0xFF,
            Self::Le(n) | Self::Be(n) => {
                if n >= 4 {
                    u32::MAX
                } else {
                    (1u32 << (8 * n as u32)) - 1
                }
            }
        }
    }
}

/// How a memory operand's value is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Modifier {
    Value,
    Delta,
    Prior,
    Bcd,
    Invert,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Operand {
    Const(u32),
    Mem {
        addr: u32,
        size: Size,
        modifier: Modifier,
        /// The value at the previous evaluation (`None` before the first).
        delta: Option<u32>,
        /// The value before the last change.
        prior: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Condition {
    left: Operand,
    cmp: Cmp,
    right: Operand,
    /// Hit target (`0` = none: true whenever the comparison is).
    target: u32,
    hits: u32,
}

/// A parsed condition set: the core group and any alternative groups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trigger {
    groups: Vec<Vec<Condition>>,
    source: String,
}

fn read_mem(addr: u32, size: Size, read: &mut impl FnMut(u32) -> u8) -> u32 {
    let byte = |read: &mut dyn FnMut(u32) -> u8, a: u32| u32::from(read(a));
    match size {
        Size::Bit(b) => (byte(read, addr) >> b) & 1,
        Size::Low4 => byte(read, addr) & 0xF,
        Size::High4 => byte(read, addr) >> 4,
        Size::Byte => byte(read, addr),
        Size::BitCount => byte(read, addr).count_ones(),
        Size::Le(n) => {
            (0..u32::from(n)).fold(0, |v, i| v | (byte(read, addr.wrapping_add(i)) << (8 * i)))
        }
        Size::Be(n) => {
            (0..u32::from(n)).fold(0, |v, i| (v << 8) | byte(read, addr.wrapping_add(i)))
        }
    }
}

/// Hex digits of `v` read as decimal (`0x1234` -> 1234); a nibble above 9
/// reads as its value (`0xA` -> 10), the usual BCD-decoder leniency.
const fn bcd(v: u32) -> u32 {
    let mut out = 0u32;
    let mut mul = 1u32;
    let mut v = v;
    while v != 0 {
        out = out.wrapping_add((v & 0xF).wrapping_mul(mul));
        mul = mul.wrapping_mul(10);
        v >>= 4;
    }
    out
}

impl Operand {
    /// This evaluation's value. `read` peeks a RA flat address.
    fn value(&self, read: &mut impl FnMut(u32) -> u8) -> u32 {
        match *self {
            Self::Const(c) => c,
            Self::Mem {
                addr,
                size,
                modifier,
                delta,
                prior,
            } => {
                let now = read_mem(addr, size, read);
                match modifier {
                    Modifier::Value => now,
                    // Before the first evaluation there is no previous value;
                    // the current one stands in, so `d` never reports a change
                    // that did not happen (no edge on the first frame).
                    Modifier::Delta => delta.unwrap_or(now),
                    Modifier::Prior => prior,
                    Modifier::Bcd => bcd(now),
                    Modifier::Invert => !now & size.mask(),
                }
            }
        }
    }

    /// Record this evaluation's raw value for the next one's `d` / `p`.
    fn advance(&mut self, read: &mut impl FnMut(u32) -> u8) {
        if let Self::Mem {
            addr,
            size,
            delta,
            prior,
            ..
        } = self
        {
            let now = read_mem(*addr, *size, read);
            match *delta {
                None => *prior = now,
                Some(last) if last != now => *prior = last,
                Some(_) => {}
            }
            *delta = Some(now);
        }
    }

    const fn reset_history(&mut self) {
        if let Self::Mem { delta, .. } = self {
            *delta = None;
        }
    }
}

impl Cmp {
    const fn test(self, a: u32, b: u32) -> bool {
        match self {
            Self::Eq => a == b,
            Self::Ne => a != b,
            Self::Lt => a < b,
            Self::Le => a <= b,
            Self::Gt => a > b,
            Self::Ge => a >= b,
        }
    }
}

/// A byte cursor over a condition string.
struct Cursor<'a> {
    s: &'a [u8],
    i: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, b: u8) -> bool {
        if self.peek() == Some(b) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn rest(&self) -> &str {
        core::str::from_utf8(&self.s[self.i..]).unwrap_or("?")
    }
    fn hex(&mut self) -> Option<u32> {
        let start = self.i;
        while self.peek().is_some_and(|b| b.is_ascii_hexdigit()) {
            self.i += 1;
        }
        let digits = core::str::from_utf8(&self.s[start..self.i]).ok()?;
        if digits.is_empty() {
            None
        } else {
            u32::from_str_radix(digits, 16).ok()
        }
    }
    fn dec(&mut self) -> Option<u32> {
        let start = self.i;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.i += 1;
        }
        let digits = core::str::from_utf8(&self.s[start..self.i]).ok()?;
        if digits.is_empty() {
            None
        } else {
            digits.parse().ok()
        }
    }
}

fn parse_operand(c: &mut Cursor<'_>) -> Result<Operand, ParseError> {
    let modifier = match c.peek() {
        Some(b'd' | b'D') => Modifier::Delta,
        Some(b'p' | b'P') => Modifier::Prior,
        Some(b'b' | b'B') => Modifier::Bcd,
        Some(b'~') => Modifier::Invert,
        _ => Modifier::Value,
    };
    if modifier != Modifier::Value {
        c.i += 1;
    }
    if c.s[c.i..].starts_with(b"0x") || c.s[c.i..].starts_with(b"0X") {
        c.i += 2;
        let size = match c.peek().map(|b| b.to_ascii_uppercase()) {
            Some(b @ b'M'..=b'T') => {
                c.i += 1;
                Size::Bit(b - b'M')
            }
            Some(b'L') => {
                c.i += 1;
                Size::Low4
            }
            Some(b'U') => {
                c.i += 1;
                Size::High4
            }
            Some(b'H') => {
                c.i += 1;
                Size::Byte
            }
            Some(b'K') => {
                c.i += 1;
                Size::BitCount
            }
            Some(b'W') => {
                c.i += 1;
                Size::Le(3)
            }
            Some(b'X') => {
                c.i += 1;
                Size::Le(4)
            }
            Some(b'I') => {
                c.i += 1;
                Size::Be(2)
            }
            Some(b'J') => {
                c.i += 1;
                Size::Be(3)
            }
            Some(b'G') => {
                c.i += 1;
                Size::Be(4)
            }
            Some(b' ') => {
                c.i += 1;
                Size::Le(2)
            }
            Some(b) if b.is_ascii_hexdigit() => Size::Le(2),
            _ => {
                return Err(err(format!(
                    "unsupported memory size at {:?} (supported: M-T, L, U, H, K, W, X, I, J, G, or none for 16-bit)",
                    c.rest()
                )));
            }
        };
        let addr = c
            .hex()
            .ok_or_else(|| err(format!("expected a hex address at {:?}", c.rest())))?;
        return Ok(Operand::Mem {
            addr,
            size,
            modifier,
            delta: None,
            prior: 0,
        });
    }
    if modifier != Modifier::Value && modifier != Modifier::Bcd {
        return Err(err(format!(
            "a prefix needs a memory operand at {:?}",
            c.rest()
        )));
    }
    if modifier == Modifier::Bcd {
        return Err(err("`b` (BCD) applies to a memory operand"));
    }
    if c.eat(b'h') || c.eat(b'H') {
        return c
            .hex()
            .map(Operand::Const)
            .ok_or_else(|| err(format!("expected hex digits after `h` at {:?}", c.rest())));
    }
    if matches!(c.peek(), Some(b'f' | b'F' | b'-' | b'+')) {
        return Err(err(format!(
            "float and signed constants are not supported ({:?})",
            c.rest()
        )));
    }
    c.dec()
        .map(Operand::Const)
        .ok_or_else(|| err(format!("expected an operand at {:?}", c.rest())))
}

fn parse_condition(c: &mut Cursor<'_>) -> Result<Condition, ParseError> {
    // A flag is one letter and a colon.
    if c.s.get(c.i + 1) == Some(&b':') && c.peek().is_some_and(|b| b.is_ascii_alphabetic()) {
        return Err(err(format!(
            "condition flags are not supported by the speedrun timer ({:?}); use the splits file's `reset` for a reset",
            c.rest()
        )));
    }
    let left = parse_operand(c)?;
    let cmp = if c.eat(b'=') {
        Cmp::Eq
    } else if c.eat(b'!') {
        if !c.eat(b'=') {
            return Err(err("`!` must be `!=`"));
        }
        Cmp::Ne
    } else if c.eat(b'<') {
        if c.eat(b'=') { Cmp::Le } else { Cmp::Lt }
    } else if c.eat(b'>') {
        if c.eat(b'=') { Cmp::Ge } else { Cmp::Gt }
    } else {
        return Err(err(format!(
            "expected a comparison (= != < <= > >=) at {:?}",
            c.rest()
        )));
    };
    let right = parse_operand(c)?;
    let mut target = 0;
    if c.eat(b'.') {
        target = c
            .dec()
            .ok_or_else(|| err("a hit target is `.N.` with N a decimal number"))?;
        if !c.eat(b'.') {
            return Err(err("a hit target is `.N.`: missing the closing `.`"));
        }
    }
    Ok(Condition {
        left,
        cmp,
        right,
        target,
        hits: 0,
    })
}

impl Trigger {
    /// Parse a condition set (see the module docs for the grammar).
    ///
    /// # Errors
    ///
    /// [`ParseError`] naming the first piece that is malformed or unsupported.
    pub fn parse(source: &str) -> Result<Self, ParseError> {
        let trimmed = source.trim();
        if trimmed.is_empty() {
            return Err(err("an empty condition"));
        }
        let mut c = Cursor {
            s: trimmed.as_bytes(),
            i: 0,
        };
        let mut groups = alloc::vec![Vec::new()];
        loop {
            let cond = parse_condition(&mut c)?;
            groups.last_mut().expect("never empty").push(cond);
            match c.peek() {
                None => break,
                Some(b'_') => c.i += 1,
                Some(b'S') => {
                    c.i += 1;
                    groups.push(Vec::new());
                }
                Some(_) => {
                    return Err(err(format!(
                        "expected `_`, `S` or the end at {:?}",
                        c.rest()
                    )));
                }
            }
        }
        Ok(Self {
            groups,
            source: trimmed.to_string(),
        })
    }

    /// The condition text this trigger was parsed from.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Evaluate the set against memory. When `armed`, hit counts advance and
    /// the result is returned; when not, the conditions are only sampled (so
    /// their `d` / `p` history stays one frame deep) and the result is
    /// `false`. Either way the operand history then advances.
    pub fn evaluate(&mut self, armed: bool, read: &mut impl FnMut(u32) -> u8) -> bool {
        let mut group_true = Vec::with_capacity(self.groups.len());
        for group in &mut self.groups {
            let mut all = true;
            for cond in group.iter_mut() {
                let ok = cond.cmp.test(cond.left.value(read), cond.right.value(read));
                let held = if cond.target == 0 {
                    ok
                } else {
                    if armed && ok && cond.hits < cond.target {
                        cond.hits += 1;
                    }
                    cond.hits >= cond.target
                };
                all &= held;
            }
            group_true.push(all);
        }
        for group in &mut self.groups {
            for cond in group.iter_mut() {
                cond.left.advance(read);
                cond.right.advance(read);
            }
        }
        let core = group_true[0];
        let alt = group_true.len() == 1 || group_true[1..].iter().any(|&g| g);
        armed && core && alt
    }

    /// Clear the hit counts (the trigger is re-armed from zero).
    pub fn reset_hits(&mut self) {
        for cond in self.groups.iter_mut().flatten() {
            cond.hits = 0;
        }
    }

    /// Forget the `d` / `p` history, as after a discontinuity in time.
    fn reset_history(&mut self) {
        for cond in self.groups.iter_mut().flatten() {
            cond.left.reset_history();
            cond.right.reset_history();
        }
    }
}

/// A named split: the predicate that ends its segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Split {
    /// Its name, as the splits file gives it.
    pub name: String,
    /// When it fires.
    pub trigger: Trigger,
}

/// A parsed splits file (see the module docs for the format).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitsDefinition {
    /// The `game` line, or empty.
    pub game: String,
    /// The `category` line, or empty.
    pub category: String,
    /// When the run starts.
    pub start: Trigger,
    /// The splits, in order; the last ends the run. Never empty.
    pub splits: Vec<Split>,
    /// When a run in progress (or finished) is abandoned, if ever.
    pub reset: Option<Trigger>,
}

/// The most splits a definition may have, which is also the size of the
/// memory-export interface's split table.
pub const MAX_SPLITS: usize = 64;

impl SplitsDefinition {
    /// Parse a splits file.
    ///
    /// # Errors
    ///
    /// [`ParseError`] with the line number, for an unknown key, a malformed
    /// condition, a missing `start` or split, a second `start` or `reset`, or
    /// more than [`MAX_SPLITS`] splits.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let mut game = String::new();
        let mut category = String::new();
        let mut start = None;
        let mut reset = None;
        let mut splits = Vec::new();
        for (n, raw) in text.lines().enumerate() {
            let line_no = n + 1;
            let at = |e: ParseError| ParseError {
                message: e.message,
                line: line_no,
            };
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(at(err(format!("expected `key = value`, got {line:?}"))));
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "game" => game = value.to_string(),
                "category" => category = value.to_string(),
                "start" | "reset" => {
                    let slot = if key == "start" {
                        &mut start
                    } else {
                        &mut reset
                    };
                    if slot.is_some() {
                        return Err(at(err(format!("a second `{key}`"))));
                    }
                    *slot = Some(Trigger::parse(value).map_err(at)?);
                }
                "split" => {
                    let Some((name, cond)) = value.split_once('|') else {
                        return Err(at(err("a split is `split = <name> | <condition>`")));
                    };
                    if splits.len() == MAX_SPLITS {
                        return Err(at(err(format!("more than {MAX_SPLITS} splits"))));
                    }
                    splits.push(Split {
                        name: name.trim().to_string(),
                        trigger: Trigger::parse(cond).map_err(at)?,
                    });
                }
                other => {
                    return Err(at(err(format!(
                        "unknown key `{other}` (expected game, category, start, split, reset)"
                    ))));
                }
            }
        }
        let start = start.ok_or_else(|| err("no `start` line"))?;
        if splits.is_empty() {
            return Err(err("no `split` line: the last split ends the run"));
        }
        Ok(Self {
            game,
            category,
            start,
            splits,
            reset,
        })
    }
}

/// Where a timer is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerState {
    /// Waiting for the start predicate.
    Idle,
    /// Started; the next split has not fired.
    Running,
    /// The last split fired. Stays until the reset predicate (or a
    /// discontinuity) returns it to [`Self::Idle`].
    Finished,
}

impl TimerState {
    /// The memory-export interface's code for this state (0 / 1 / 2).
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Running => 1,
            Self::Finished => 2,
        }
    }
}

/// What one observation changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerEvent {
    /// The start predicate fired.
    Started,
    /// Split `n` (0-based) fired and the run continues.
    Split(usize),
    /// The last split fired.
    Finished,
    /// The reset predicate fired.
    Reset,
    /// Time ran backward or leapt (a state load, rewind, power cycle): the run
    /// is abandoned, because its time no longer measures one run.
    Discontinuity,
}

/// One split's time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitTime {
    /// The master clock at the end of the frame the split fired in.
    pub master_clock: u64,
    /// Cycles from the start to this split.
    pub elapsed: u64,
    /// The frame counter at that point.
    pub frame: u64,
}

/// The longest gap between two observations that still counts as one
/// continuous run.
///
/// Sixteen of the longest region's frames (a PAL frame is
/// 312 lines of 341 dots at 5 master clocks per dot, 531,960 master clocks),
/// which leaves room for the extra-scanline option and an observer that skips
/// a few frames. A larger leap, or any step backward, is a
/// [`TimerEvent::Discontinuity`].
pub const MAX_OBSERVATION_GAP: u64 = 16 * 531_960;

/// The timer. Feed it [`Self::observe`] once per frame, after the frame.
#[derive(Clone, Debug)]
pub struct SpeedrunTimer {
    def: SplitsDefinition,
    region: Region,
    state: TimerState,
    start_clock: u64,
    start_frame: u64,
    splits: Vec<SplitTime>,
    last_clock: Option<u64>,
    now: u64,
}

impl SpeedrunTimer {
    /// A timer for `def`, timing in `region`'s master clock.
    #[must_use]
    pub const fn new(def: SplitsDefinition, region: Region) -> Self {
        Self {
            def,
            region,
            state: TimerState::Idle,
            start_clock: 0,
            start_frame: 0,
            splits: Vec::new(),
            last_clock: None,
            now: 0,
        }
    }

    /// The definition being timed.
    #[must_use]
    pub const fn definition(&self) -> &SplitsDefinition {
        &self.def
    }

    /// The region whose master clock the times are in.
    #[must_use]
    pub const fn region(&self) -> Region {
        self.region
    }

    /// The current state.
    #[must_use]
    pub const fn state(&self) -> TimerState {
        self.state
    }

    /// The splits fired so far in this run.
    #[must_use]
    pub fn splits(&self) -> &[SplitTime] {
        &self.splits
    }

    /// The master clock the run started at (meaningful once started).
    #[must_use]
    pub const fn start_clock(&self) -> u64 {
        self.start_clock
    }

    /// Cycles since the start: live while running, the final time once
    /// finished, 0 when idle.
    #[must_use]
    pub fn elapsed(&self) -> u64 {
        match self.state {
            TimerState::Idle => 0,
            TimerState::Running => self.now.saturating_sub(self.start_clock),
            TimerState::Finished => self.splits.last().map_or(0, |s| s.elapsed),
        }
    }

    /// Return to idle and clear every hit count, as if just created (the
    /// operand history is kept: time did not jump).
    pub fn reset(&mut self) {
        self.state = TimerState::Idle;
        self.splits.clear();
        self.def.start.reset_hits();
        for s in &mut self.def.splits {
            s.trigger.reset_hits();
        }
        if let Some(r) = self.def.reset.as_mut() {
            r.reset_hits();
        }
    }

    /// Observe the end of a frame: `master_clock` and `frame` are the
    /// console's counters now (on [`crate::Nes`], `cpu().master_clock()` and
    /// `frame()`), and `read` peeks a `RetroAchievements` flat address
    /// (unmapped addresses should read 0; [`Self::observe_nes`] does this).
    pub fn observe(
        &mut self,
        master_clock: u64,
        frame: u64,
        mut read: impl FnMut(u32) -> u8,
    ) -> Option<TimerEvent> {
        let mut event = None;
        if let Some(last) = self.last_clock
            && (master_clock < last || master_clock - last > MAX_OBSERVATION_GAP)
        {
            let was_active = self.state != TimerState::Idle;
            self.reset();
            self.def.start.reset_history();
            for s in &mut self.def.splits {
                s.trigger.reset_history();
            }
            if let Some(r) = self.def.reset.as_mut() {
                r.reset_history();
            }
            if was_active {
                event = Some(TimerEvent::Discontinuity);
            }
        }
        self.last_clock = Some(master_clock);
        self.now = master_clock;

        // Every trigger is evaluated every frame so its `d` / `p` history is
        // one frame deep whenever it becomes the one that matters; only the
        // armed ones count hits or fire.
        let state = self.state;
        let next = self.splits.len();
        let start_fired = self
            .def
            .start
            .evaluate(state == TimerState::Idle, &mut read);
        let reset_fired = self
            .def
            .reset
            .as_mut()
            .is_some_and(|r| r.evaluate(state != TimerState::Idle, &mut read));
        let mut split_fired = false;
        for (i, s) in self.def.splits.iter_mut().enumerate() {
            let fired = s
                .trigger
                .evaluate(state == TimerState::Running && i == next, &mut read);
            split_fired |= fired;
        }
        if event.is_some() {
            return event;
        }
        match state {
            TimerState::Idle if start_fired => {
                self.state = TimerState::Running;
                self.start_clock = master_clock;
                self.start_frame = frame;
                self.splits.clear();
                Some(TimerEvent::Started)
            }
            TimerState::Running | TimerState::Finished if reset_fired => {
                self.reset();
                Some(TimerEvent::Reset)
            }
            TimerState::Running if split_fired => {
                self.splits.push(SplitTime {
                    master_clock,
                    elapsed: master_clock - self.start_clock,
                    frame,
                });
                if self.splits.len() == self.def.splits.len() {
                    self.state = TimerState::Finished;
                    Some(TimerEvent::Finished)
                } else {
                    Some(TimerEvent::Split(self.splits.len() - 1))
                }
            }
            _ => None,
        }
    }

    /// [`Self::observe`] against a live console, after its frame.
    pub fn observe_nes(&mut self, nes: &mut crate::Nes) -> Option<TimerEvent> {
        let clock = nes.cpu().master_clock();
        let frame = nes.frame();
        self.observe(clock, frame, |a| {
            ra_flat_to_cpu(a).map_or(0, |cpu| nes.peek(cpu))
        })
    }

    /// The run as a JSON document (format `rustynes-splits-result`, version
    /// 1, documented in `docs/memory-export-interface.md`). Times are given
    /// both as exact master-clock cycle counts and as seconds.
    #[must_use]
    pub fn to_json(&self) -> String {
        let region = match self.region {
            Region::Ntsc => "NTSC",
            Region::Pal => "PAL",
            Region::Dendy => "Dendy",
        };
        let (num, den) = master_clock_hz_ratio(self.region);
        let mut out = String::new();
        // `fmt::Write` for a `String` cannot fail; the results are discarded.
        let _ = writeln!(out, "{{");
        let _ = writeln!(out, "  \"format\": \"rustynes-splits-result\",");
        let _ = writeln!(out, "  \"version\": 1,");
        let _ = writeln!(out, "  \"game\": {},", json_str(&self.def.game));
        let _ = writeln!(out, "  \"category\": {},", json_str(&self.def.category));
        let _ = writeln!(out, "  \"region\": \"{region}\",");
        let _ = writeln!(
            out,
            "  \"master_clock_hz\": {{\"numerator\": {num}, \"denominator\": {den}}},"
        );
        let state = match self.state {
            TimerState::Idle => "idle",
            TimerState::Running => "running",
            TimerState::Finished => "finished",
        };
        let _ = writeln!(out, "  \"state\": \"{state}\",");
        let _ = writeln!(out, "  \"start_master_clock\": {},", self.start_clock);
        let _ = writeln!(out, "  \"start_frame\": {},", self.start_frame);
        out.push_str("  \"splits\": [");
        let mut prev = 0u64;
        for (i, (t, s)) in self.splits.iter().zip(&self.def.splits).enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "\n    {{\"name\": {}, \"master_clock\": {}, \"frame\": {}, \"elapsed_cycles\": {}, \"segment_cycles\": {}, \"elapsed_seconds\": {:.9}}}",
                json_str(&s.name),
                t.master_clock,
                t.frame,
                t.elapsed,
                t.elapsed - prev,
                cycles_to_seconds(t.elapsed, self.region)
            );
            prev = t.elapsed;
        }
        out.push_str(if self.splits.is_empty() {
            "],\n"
        } else {
            "\n  ],\n"
        });
        let total = self.elapsed();
        let _ = writeln!(out, "  \"elapsed_cycles\": {total},");
        let _ = writeln!(
            out,
            "  \"elapsed_seconds\": {:.9}",
            cycles_to_seconds(total, self.region)
        );
        let _ = writeln!(out, "}}");
        out
    }
}

/// `s` as a JSON string literal.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// A 64 KiB flat memory for the evaluator tests (RA addresses index it
    /// directly).
    struct Mem(Vec<u8>);
    impl Mem {
        fn new() -> Self {
            Self(vec![0; 0x1_0000])
        }
        fn read(&self) -> impl FnMut(u32) -> u8 + '_ {
            |a| self.0.get(a as usize).copied().unwrap_or(0)
        }
    }

    fn fire(t: &mut Trigger, m: &Mem) -> bool {
        t.evaluate(true, &mut m.read())
    }

    #[test]
    fn sizes_and_comparisons() {
        let mut m = Mem::new();
        m.0[0x10] = 0b1010_0101;
        m.0[0x11] = 0x34;
        m.0[0x12] = 0x12;
        m.0[0x13] = 0x56;
        m.0[0x14] = 0x78;
        for (cond, want) in [
            ("0xH0010=165", true),
            ("0xH0010=h a5", false), // no spaces inside a constant
            ("0xH0010=hA5", true),
            ("0xM0010=1", true),
            ("0xN0010=0", true),
            ("0xO0010=1", true),
            ("0xT0010=1", true),
            ("0xS0010=0", true),
            ("0xL0010=5", true),
            ("0xU0010=10", true),
            ("0xK0010=4", true),
            ("0x0011=h1234", true),
            ("0x 0011=h1234", true),
            ("0xW0011=h561234", true),
            ("0xX0011=h78561234", true),
            ("0xI0011=h3412", true),
            ("0xJ0011=h341256", true),
            ("0xG0011=h34125678", true),
            ("b0xH0011=34", true),
            ("~0xH0010=h5a", true),
            ("0xH0011>0xH0012", true),
            ("0xH0011<=h34", true),
            ("0xH0011<h34", false),
            ("0xH0011>=h35", false),
            ("0xH0011!=h34", false),
        ] {
            if cond.contains("h a5") {
                assert!(Trigger::parse(cond).is_err(), "{cond}");
                continue;
            }
            let mut t = Trigger::parse(cond).unwrap_or_else(|e| panic!("{cond}: {e}"));
            assert_eq!(fire(&mut t, &m), want, "{cond}");
        }
    }

    /// `S` separates groups, but `0xS` is bit 6: the tokenizer must not split
    /// inside a memory operand.
    #[test]
    fn groups_and_the_bit6_size() {
        let mut m = Mem::new();
        m.0[0x20] = 0x40;
        let mut t = Trigger::parse("0xS0020=1").unwrap();
        assert_eq!(t.groups.len(), 1);
        assert!(fire(&mut t, &m));
        // core AND (alt1 OR alt2)
        let mut t = Trigger::parse("0xS0020=1S0xH0021=1S0xH0022=1").unwrap();
        assert_eq!(t.groups.len(), 3);
        assert!(!fire(&mut t, &m), "neither alternative holds");
        m.0[0x22] = 1;
        assert!(fire(&mut t, &m));
        m.0[0x20] = 0;
        assert!(!fire(&mut t, &m), "the core group fails");
    }

    #[test]
    fn delta_prior_and_hits() {
        let mut m = Mem::new();
        let mut edge = Trigger::parse("0xH0000=1_d0xH0000=0").unwrap();
        m.0[0] = 1;
        assert!(!fire(&mut edge, &m), "no edge on the first evaluation");
        m.0[0] = 0;
        assert!(!fire(&mut edge, &m));
        m.0[0] = 1;
        assert!(fire(&mut edge, &m), "0 -> 1 is an edge");
        assert!(!fire(&mut edge, &m), "held is not an edge");

        let mut prior = Trigger::parse("p0xH0001=7").unwrap();
        m.0[1] = 7;
        fire(&mut prior, &m);
        m.0[1] = 9;
        fire(&mut prior, &m);
        assert!(fire(&mut prior, &m), "the value before the last change");

        let mut held = Trigger::parse("0xH0002=1.3.").unwrap();
        m.0[2] = 1;
        assert!(!fire(&mut held, &m));
        assert!(!fire(&mut held, &m));
        assert!(fire(&mut held, &m), "third hit");
        m.0[2] = 0;
        assert!(fire(&mut held, &m), "hits are kept once reached");
        held.reset_hits();
        assert!(!fire(&mut held, &m));
    }

    #[test]
    fn unsupported_syntax_is_refused_not_misread() {
        for bad in [
            "",
            "R:0xH0000=1",
            "A:0xH0000=1",
            "0xH0000",
            "0xH0000=f1.5",
            "0xH0000=-1",
            "0xH0000*2=4",
            "0xZ0000=1",
            "0xH=1",
            "0xH0000==1",
            "0xH0000=1.3",
            "0xH0000=1|",
            "d5=5",
        ] {
            assert!(Trigger::parse(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn the_flat_map_matches_the_ra_layout() {
        assert_eq!(ra_flat_to_cpu(0), Some(0));
        assert_eq!(ra_flat_to_cpu(0x07FF), Some(0x07FF));
        assert_eq!(ra_flat_to_cpu(0x0800), Some(0x6000));
        assert_eq!(ra_flat_to_cpu(0x27FF), Some(0x7FFF));
        assert_eq!(ra_flat_to_cpu(0x2800), None);
    }

    #[test]
    fn master_clock_rates() {
        // 21.477272... MHz and 26.6017125 MHz.
        assert!((master_clock_hz(Region::Ntsc) - 21_477_272.727_272_7).abs() < 1e-3);
        assert!((master_clock_hz(Region::Pal) - 26_601_712.5).abs() < 1e-6);
        assert_eq!(master_clock_hz(Region::Dendy), master_clock_hz(Region::Pal));
        // One NTSC second of CPU cycles (1,789,772.7 at divider 12).
        let one_second = 236_250_000 / 11;
        assert!((cycles_to_seconds(one_second, Region::Ntsc) - 1.0).abs() < 1e-7);
    }

    const DEF: &str = "\
# a test definition
game = Test \"Game\"
category = Any%
start = 0xH0000=1_d0xH0000=0
split = Half way | 0xH0001=1
split = The end | 0xH0002=1
reset = 0xH0003=1
";

    #[test]
    fn the_splits_file_parses() {
        let d = SplitsDefinition::parse(DEF).unwrap();
        assert_eq!(d.game, "Test \"Game\"");
        assert_eq!(d.category, "Any%");
        assert_eq!(d.splits.len(), 2);
        assert_eq!(d.splits[1].name, "The end");
        assert_eq!(d.start.source(), "0xH0000=1_d0xH0000=0");
        assert!(d.reset.is_some());
        for (bad, line) in [
            ("start = 0xH0000=1\n", 0),
            ("split = a | 0xH0000=1\n", 0),
            ("start = 0xH0000=1\nsplit = a | 0xH0000=1\nfoo = 1\n", 3),
            (
                "start = 0xH0000=1\nstart = 0xH0000=1\nsplit = a | 0xH0000=1\n",
                2,
            ),
            ("start = 0xH0000\nsplit = a | 0xH0000=1\n", 1),
            ("start = 0xH0000=1\nsplit = no-bar 0xH0000=1\n", 2),
        ] {
            let e = SplitsDefinition::parse(bad).unwrap_err();
            assert_eq!(e.line, line, "{bad:?}: {e}");
        }
    }

    /// Drive a timer through a scripted memory history: start on an edge,
    /// two splits, a reset, a discontinuity.
    #[test]
    fn the_timer_runs_splits_finishes_and_resets() {
        let def = SplitsDefinition::parse(DEF).unwrap();
        let mut t = SpeedrunTimer::new(def, Region::Ntsc);
        let mut m = Mem::new();
        let mut clock = 1_000u64;
        let mut frame = 0u64;
        let mut step = |t: &mut SpeedrunTimer, m: &Mem| {
            clock += 357_368; // one NTSC frame: 341 x 262 dots x 4
            frame += 1;
            t.observe(clock, frame, m.read())
        };
        assert_eq!(step(&mut t, &m), None);
        m.0[0] = 1;
        assert_eq!(step(&mut t, &m), Some(TimerEvent::Started));
        let started = t.start_clock();
        assert_eq!(step(&mut t, &m), None, "the start is an edge");
        m.0[2] = 1;
        assert_eq!(
            step(&mut t, &m),
            None,
            "split 2 is not armed before split 1"
        );
        m.0[1] = 1;
        assert_eq!(step(&mut t, &m), Some(TimerEvent::Split(0)));
        assert_eq!(t.splits()[0].elapsed, 3 * 357_368);
        assert_eq!(step(&mut t, &m), Some(TimerEvent::Finished));
        assert_eq!(t.state(), TimerState::Finished);
        assert_eq!(t.elapsed(), 4 * 357_368);
        assert_eq!(t.splits()[1].master_clock - started, 4 * 357_368);
        let json = t.to_json();
        assert!(json.contains("\"elapsed_cycles\": 1429472"), "{json}");
        assert!(json.contains("\"segment_cycles\": 357368"), "{json}");
        assert!(json.contains("\"game\": \"Test \\\"Game\\\"\""), "{json}");
        assert_eq!(step(&mut t, &m), None, "finished holds");
        assert_eq!(t.elapsed(), 4 * 357_368, "the final time does not move");
        m.0[3] = 1;
        assert_eq!(step(&mut t, &m), Some(TimerEvent::Reset));
        assert_eq!(t.state(), TimerState::Idle);
        // Restart, then go back in time.
        m.0[3] = 0;
        m.0[0] = 0;
        step(&mut t, &m);
        m.0[0] = 1;
        assert_eq!(step(&mut t, &m), Some(TimerEvent::Started));
        assert_eq!(t.observe(10, 99, m.read()), Some(TimerEvent::Discontinuity));
        assert_eq!(t.state(), TimerState::Idle);
        // And a leap forward.
        assert_eq!(t.observe(20, 100, m.read()), None);
        m.0[0] = 0;
        t.observe(30, 101, m.read());
        m.0[0] = 1;
        assert_eq!(t.observe(40, 102, m.read()), Some(TimerEvent::Started));
        assert_eq!(
            t.observe(40 + MAX_OBSERVATION_GAP + 1, 103, m.read()),
            Some(TimerEvent::Discontinuity)
        );
        // A one-frame step back (a rewind of a single frame) is a
        // discontinuity too, however small.
        let base = 40 + MAX_OBSERVATION_GAP + 1;
        m.0[0] = 0;
        t.observe(base + 357_368, 104, m.read());
        m.0[0] = 1;
        assert_eq!(
            t.observe(base + 2 * 357_368, 105, m.read()),
            Some(TimerEvent::Started)
        );
        assert_eq!(
            t.observe(base + 357_368, 104, m.read()),
            Some(TimerEvent::Discontinuity)
        );
    }
}

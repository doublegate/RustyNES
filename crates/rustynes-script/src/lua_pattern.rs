// SPDX-License-Identifier: GPL-3.0-or-later
//
// Provenance: the pattern matcher and the find / match / gmatch / gsub drivers are derived from Lua 5.4.9 `lstrlib.c` (Copyright (C) 1994-2026 Lua.org, PUC-Rio), MIT-licensed. See docs/originality-and-provenance.md (Section 1)
// and NOTICE for the complete, audited derivation record.
//! A step-metered Lua 5.4 pattern matcher for the script sandbox.
//!
//! # Why this exists (v2.9.0 re-audit NF-02)
//!
//! The sandbox bounds a script's CPU time with a VM count hook
//! (`mlua_backend.rs`, SEC-03): every 10,000 VM instructions the hook adds to a
//! per-frame counter and aborts the script once the budget is spent. A hook can
//! only fire *between* VM instructions, and Lua's `string.find`, `string.match`,
//! `string.gmatch` and `string.gsub` are single C calls. Lua patterns
//! backtrack (`.-`, `*`, `+`), so one call can do polynomial work with zero VM
//! instructions: `string.rep('a', 3000):find('.-.-.-.-b')` is roughly n^4 C
//! steps, and ran for longer than any test was willing to wait while the host
//! held its emulator lock. The 64 MiB heap limit (SEC-02) does not help, because
//! matching allocates nothing.
//!
//! This module replaces those four functions with a Rust implementation of
//! the same algorithm that **counts its steps** and charges them to the very
//! counter the hook uses. When the budget is spent it trips the same flag and
//! raises the same error as the hook, so the uncatchable-abort wrappers
//! (`pcall` / `xpcall` / `coroutine.resume`, SEC-03) treat a runaway pattern
//! exactly like a runaway loop.
//!
//! # Derivation, and why it is declared
//!
//! Lua's pattern semantics are defined by `lstrlib.c`, and the only way to be
//! *exactly* compatible — captures, position captures, `%b`, `%f`, anchors,
//! the `[]]` / `[^]]` quirks, which class letters mean what, the order in which
//! malformed-pattern errors are detected and their exact messages, the
//! 200-deep recursion limit, gsub's "an empty match right after the previous
//! match does not count" rule — is to follow its structure. This file does,
//! function for function (`classend`, `match_class`, `matchbracketclass`,
//! `singlematch`, `matchbalance`, `max_expand`, `min_expand`, the capture
//! helpers, `match`, `str_find_aux`, `gmatch`, `add_s` / `add_value`,
//! `str_gsub`), from the copy mlua vendors (`lua-src` 551.0.2, Lua 5.4.9). It
//! is therefore a derivative of Lua, which is MIT-licensed and compatible with
//! this project's GPL-3.0-or-later. Lua is a language runtime, not a reference
//! emulator, so reading it is outside the project's reference firewall; the
//! derivation is declared here, in `docs/originality-and-provenance.md` §1 and
//! in `NOTICE` (which reproduces Lua's MIT notice) all the same, because the
//! rule is to declare every derivation, whatever its source.
//!
//! What is new rather than derived: the step metering ([`Meter`]), the host
//! memory cap on gsub's output buffer, the plain-find scan, and the
//! status-returning calling convention the Lua wrappers below rely on.
//!
//! # Calling convention
//!
//! Each Rust function returns `true, results...` or `false, error_value`, and a
//! small Lua wrapper (`WRAPPERS`) turns the second form into `error(value, 0)`.
//! The indirection exists for two reasons. An error raised by an mlua callback
//! reaches Lua as a userdata, whereas Lua's own string library raises a plain
//! string, and scripts match on those messages. And an error raised by a gsub
//! replacement *function* must reach the script as the value that function
//! raised (a string with its position, or a table), which only works if it is
//! caught and re-raised rather than converted to an mlua error. The wrapper
//! raises a pattern or argument error at level 2 (its caller), which is where
//! C's `luaL_error` places the position, and a re-raised value, the budget
//! abort or an out-of-memory error at level 0 (no position), as C does.
//!
//! # Known divergence
//!
//! The parity test (`matches_the_c_library_case_for_case`) compares values,
//! types and error messages byte for byte over 145 cases. Two differences
//! remain, both in error *text*, never in matching:
//!
//! - An argument error raised with no Lua call site to name the function
//!   (`pcall(string.find, nil, 'x')`) says `'find'` where C, falling back to
//!   a search of the loaded libraries, says `'string.find'`.
//! - A pattern error from a call in tail position (`return s:find('[')`)
//!   carries the position of the frame one further down (none, if that is a
//!   C function such as `pcall`), where C reports the Lua frame the tail call
//!   replaced: a call into a Lua-level wrapper replaces that frame, and C's
//!   call into a C function does not.
//!
//! # Metering
//!
//! One step is charged per entry to the matcher's core loop (each pattern item
//! tried at each subject position), per repetition counted by a greedy
//! quantifier, and per subject byte scanned by `%b`. Byte-linear work (the
//! plain-find scan, back-reference compares, gsub output) is charged one step
//! per [`BYTES_PER_STEP`] bytes, roughly what one VM instruction costs. Steps
//! are accumulated locally and committed every [`FLUSH_STEPS`], so the budget
//! check costs almost nothing per step.

use std::sync::Mutex;

use mlua::{Function, Lua, LuaString, MultiValue, Table, Value};

use crate::{SharedCounter, SharedFlag};

/// Most captures a pattern may open (`LUA_MAXCAPTURES`).
const MAXCAPTURES: usize = 32;
/// Recursion limit of the matcher (`MAXCCALLS`): deeper patterns fail with
/// "pattern too complex", as in C, which also keeps the Rust stack bounded.
const MAXCCALLS: u32 = 200;
/// Capture length markers (`CAP_UNFINISHED` / `CAP_POSITION`).
const CAP_UNFINISHED: isize = -1;
const CAP_POSITION: isize = -2;
/// The escape character.
const L_ESC: u8 = b'%';
/// Characters that make a `find` pattern non-plain.
const SPECIALS: &[u8] = b"^$*+?.([%-";

/// Bytes of linear work (scan, compare, copy) charged as one step.
const BYTES_PER_STEP: u64 = 64;
/// Steps accumulated locally before being committed to the shared counter.
const FLUSH_STEPS: u64 = 1024;

/// The shared instruction-budget state, the same three cells the VM hook
/// uses. Cloning shares them.
#[derive(Clone)]
pub struct Meter {
    /// Instructions (and now pattern steps) spent this frame.
    pub count: SharedCounter,
    /// The per-frame budget.
    pub budget: SharedCounter,
    /// Set when the budget trips; read by the `pcall` wrappers (SEC-03).
    pub tripped: SharedFlag,
}

/// Why a pattern operation stopped.
#[derive(Debug)]
enum PatError {
    /// A Lua error with this message (malformed pattern, bad argument).
    Msg(String),
    /// The instruction budget is spent; the trip flag is already set.
    Budget,
}

impl PatError {
    fn msg(s: impl Into<String>) -> Self {
        Self::Msg(s.into())
    }
}

type PResult<T> = Result<T, PatError>;

/// A per-call view of the [`Meter`] with a local step accumulator.
struct Charge<'m> {
    meter: &'m Meter,
    pending: u64,
}

impl<'m> Charge<'m> {
    const fn new(meter: &'m Meter) -> Self {
        Self { meter, pending: 0 }
    }

    #[inline]
    fn step(&mut self) -> PResult<()> {
        self.add(1)
    }

    #[inline]
    fn add(&mut self, n: u64) -> PResult<()> {
        self.pending += n;
        if self.pending >= FLUSH_STEPS {
            self.flush()
        } else {
            Ok(())
        }
    }

    /// Charge `bytes` of linear work at [`BYTES_PER_STEP`] per step.
    fn bytes(&mut self, bytes: usize) -> PResult<()> {
        self.add((bytes as u64).div_ceil(BYTES_PER_STEP))
    }

    /// Commit the pending steps and check the budget, exactly as the hook
    /// does: over budget sets the trip flag and aborts.
    fn flush(&mut self) -> PResult<()> {
        let n = self.meter.count.get().saturating_add(self.pending);
        self.pending = 0;
        self.meter.count.set(n);
        if n > self.meter.budget.get() {
            self.meter.tripped.set(true);
            Err(PatError::Budget)
        } else {
            Ok(())
        }
    }
}

/// A capture's value: a byte range of the subject, or a position (1-based).
enum Cap {
    Str(usize, usize),
    Pos(usize),
}

/// `MatchState`: the subject, the pattern, and the capture stack. Positions
/// are byte indices; reading one past either end yields `0`, which is what C
/// reads there (Lua strings are NUL-terminated), so every boundary case
/// behaves the same.
struct MatchState<'a, 'm> {
    src: &'a [u8],
    pat: &'a [u8],
    level: usize,
    capture: [(usize, isize); MAXCAPTURES],
    matchdepth: u32,
    charge: &'a mut Charge<'m>,
}

/// `match_class`: does byte `c` belong to class letter `cl` (`%a`, `%D`, ...)?
/// The C library's classifiers run in the "C" locale (the sandbox never calls
/// `setlocale`), which is ASCII; `isspace` there includes `\v`, which Rust's
/// `is_ascii_whitespace` does not, so it is spelled out.
fn match_class(c: u8, cl: u8) -> bool {
    let res = match cl.to_ascii_lowercase() {
        b'a' => c.is_ascii_alphabetic(),
        b'c' => c.is_ascii_control(),
        b'd' => c.is_ascii_digit(),
        b'g' => c.is_ascii_graphic(),
        b'l' => c.is_ascii_lowercase(),
        b'p' => c.is_ascii_punctuation(),
        b's' => c == b' ' || (0x09..=0x0D).contains(&c),
        b'u' => c.is_ascii_uppercase(),
        b'w' => c.is_ascii_alphanumeric(),
        b'x' => c.is_ascii_hexdigit(),
        b'z' => c == 0,
        _ => return cl == c,
    };
    if cl.is_ascii_lowercase() { res } else { !res }
}

impl<'a, 'm> MatchState<'a, 'm> {
    const fn new(src: &'a [u8], pat: &'a [u8], charge: &'a mut Charge<'m>) -> Self {
        Self {
            src,
            pat,
            level: 0,
            capture: [(0, 0); MAXCAPTURES],
            matchdepth: MAXCCALLS,
            charge,
        }
    }

    /// `reprepstate`: reset before each match attempt.
    const fn reprep(&mut self) {
        self.matchdepth = MAXCCALLS;
        self.level = 0;
    }

    #[inline]
    fn p(&self, i: usize) -> u8 {
        self.pat.get(i).copied().unwrap_or(0)
    }

    #[inline]
    fn s(&self, i: usize) -> u8 {
        self.src.get(i).copied().unwrap_or(0)
    }

    fn check_capture(&self, l: u8) -> PResult<usize> {
        let l = i32::from(l) - i32::from(b'1');
        match usize::try_from(l) {
            Ok(i) if i < self.level && self.capture[i].1 != CAP_UNFINISHED => Ok(i),
            _ => Err(PatError::msg(format!("invalid capture index %{}", l + 1))),
        }
    }

    fn capture_to_close(&self) -> PResult<usize> {
        (0..self.level)
            .rev()
            .find(|&l| self.capture[l].1 == CAP_UNFINISHED)
            .ok_or_else(|| PatError::msg("invalid pattern capture"))
    }

    /// `classend`: the index just past the single-character class at `p`.
    fn classend(&self, p: usize) -> PResult<usize> {
        let len = self.pat.len();
        let c = self.pat[p];
        let mut p = p + 1;
        match c {
            L_ESC => {
                if p == len {
                    return Err(PatError::msg("malformed pattern (ends with '%')"));
                }
                Ok(p + 1)
            }
            b'[' => {
                if self.p(p) == b'^' {
                    p += 1;
                }
                // A `]` straight after `[` / `[^` is a member, not the end:
                // the body runs before the first test.
                loop {
                    if p == len {
                        return Err(PatError::msg("malformed pattern (missing ']')"));
                    }
                    let cc = self.pat[p];
                    p += 1;
                    if cc == L_ESC && p < len {
                        p += 1; // skip escapes (e.g. '%]')
                    }
                    if self.p(p) == b']' {
                        break;
                    }
                }
                Ok(p + 1)
            }
            _ => Ok(p),
        }
    }

    /// `matchbracketclass`: `p` indexes the `[`, `ec` the closing `]`.
    fn matchbracketclass(&self, c: u8, p: usize, ec: usize) -> bool {
        let mut sig = true;
        let mut p = p;
        if self.p(p + 1) == b'^' {
            sig = false;
            p += 1;
        }
        loop {
            p += 1;
            if p >= ec {
                break;
            }
            if self.pat[p] == L_ESC {
                p += 1;
                if match_class(c, self.p(p)) {
                    return sig;
                }
            } else if self.p(p + 1) == b'-' && p + 2 < ec {
                p += 2;
                if self.pat[p - 2] <= c && c <= self.pat[p] {
                    return sig;
                }
            } else if self.pat[p] == c {
                return sig;
            }
        }
        !sig
    }

    fn singlematch(&self, s: usize, p: usize, ep: usize) -> bool {
        if s >= self.src.len() {
            return false;
        }
        let c = self.src[s];
        match self.pat[p] {
            b'.' => true,
            L_ESC => match_class(c, self.p(p + 1)),
            b'[' => self.matchbracketclass(c, p, ep - 1),
            pc => pc == c,
        }
    }

    fn matchbalance(&mut self, s: usize, p: usize) -> PResult<Option<usize>> {
        if p + 1 >= self.pat.len() {
            return Err(PatError::msg(
                "malformed pattern (missing arguments to '%b')",
            ));
        }
        if self.s(s) != self.pat[p] {
            return Ok(None);
        }
        let (b, e) = (self.pat[p], self.pat[p + 1]);
        let mut cont = 1u32;
        let mut s = s;
        loop {
            s += 1;
            if s >= self.src.len() {
                return Ok(None); // the string ends out of balance
            }
            self.charge.step()?;
            if self.src[s] == e {
                cont -= 1;
                if cont == 0 {
                    return Ok(Some(s + 1));
                }
            } else if self.src[s] == b {
                cont += 1;
            }
        }
    }

    fn max_expand(&mut self, s: usize, p: usize, ep: usize) -> PResult<Option<usize>> {
        let mut i = 0usize;
        while self.singlematch(s + i, p, ep) {
            self.charge.step()?;
            i += 1;
        }
        // Try the rest of the pattern with the most repetitions first.
        loop {
            if let Some(r) = self.do_match(s + i, ep + 1)? {
                return Ok(Some(r));
            }
            if i == 0 {
                return Ok(None);
            }
            i -= 1;
        }
    }

    fn min_expand(&mut self, mut s: usize, p: usize, ep: usize) -> PResult<Option<usize>> {
        loop {
            if let Some(r) = self.do_match(s, ep + 1)? {
                return Ok(Some(r));
            } else if self.singlematch(s, p, ep) {
                s += 1; // try with one more repetition
            } else {
                return Ok(None);
            }
        }
    }

    fn start_capture(&mut self, s: usize, p: usize, what: isize) -> PResult<Option<usize>> {
        if self.level >= MAXCAPTURES {
            return Err(PatError::msg("too many captures"));
        }
        self.capture[self.level] = (s, what);
        self.level += 1;
        let r = self.do_match(s, p)?;
        if r.is_none() {
            self.level -= 1; // undo capture
        }
        Ok(r)
    }

    fn end_capture(&mut self, s: usize, p: usize) -> PResult<Option<usize>> {
        let l = self.capture_to_close()?;
        #[allow(clippy::cast_possible_wrap)] // a subject length fits an isize.
        let len = (s - self.capture[l].0) as isize;
        self.capture[l].1 = len;
        let r = self.do_match(s, p)?;
        if r.is_none() {
            self.capture[l].1 = CAP_UNFINISHED; // undo capture
        }
        Ok(r)
    }

    fn match_capture(&mut self, s: usize, l: u8) -> PResult<Option<usize>> {
        let l = self.check_capture(l)?;
        let (init, len) = self.capture[l];
        // A position capture has a negative length, which C compares as a
        // huge `size_t` and so never matches.
        let Ok(len) = usize::try_from(len) else {
            return Ok(None);
        };
        self.charge.bytes(len)?;
        if self.src.len() - s >= len && self.src[init..init + len] == self.src[s..s + len] {
            Ok(Some(s + len))
        } else {
            Ok(None)
        }
    }

    /// `match`, with its depth accounting. The C function's `goto init` tail
    /// calls are the loop in [`Self::match_body`].
    fn do_match(&mut self, s: usize, p: usize) -> PResult<Option<usize>> {
        if self.matchdepth == 0 {
            return Err(PatError::msg("pattern too complex"));
        }
        self.matchdepth -= 1;
        let r = self.match_body(s, p);
        self.matchdepth += 1;
        r
    }

    fn match_body(&mut self, mut s: usize, mut p: usize) -> PResult<Option<usize>> {
        loop {
            self.charge.step()?;
            if p == self.pat.len() {
                return Ok(Some(s)); // end of pattern
            }
            match self.pat[p] {
                b'(' => {
                    return if self.p(p + 1) == b')' {
                        self.start_capture(s, p + 2, CAP_POSITION)
                    } else {
                        self.start_capture(s, p + 1, CAP_UNFINISHED)
                    };
                }
                b')' => return self.end_capture(s, p + 1),
                // `$` anchors only as the last pattern character; elsewhere
                // it falls through to the default (a literal `$`).
                b'$' if p + 1 == self.pat.len() => {
                    return Ok((s == self.src.len()).then_some(s));
                }
                L_ESC => match self.p(p + 1) {
                    b'b' => match self.matchbalance(s, p + 2)? {
                        Some(ns) => {
                            s = ns;
                            p += 4;
                            continue;
                        }
                        None => return Ok(None),
                    },
                    b'f' => {
                        p += 2;
                        if self.p(p) != b'[' {
                            return Err(PatError::msg("missing '[' after '%f' in pattern"));
                        }
                        let ep = self.classend(p)?;
                        let previous = if s == 0 { 0 } else { self.src[s - 1] };
                        if !self.matchbracketclass(previous, p, ep - 1)
                            && self.matchbracketclass(self.s(s), p, ep - 1)
                        {
                            p = ep;
                            continue;
                        }
                        return Ok(None);
                    }
                    d @ b'0'..=b'9' => match self.match_capture(s, d)? {
                        Some(ns) => {
                            s = ns;
                            p += 2;
                            continue;
                        }
                        None => return Ok(None),
                    },
                    _ => {} // an escaped class: the default below
                },
                _ => {}
            }
            // Default: a single-character class plus an optional suffix.
            let ep = self.classend(p)?;
            let suffix = self.p(ep);
            if !self.singlematch(s, p, ep) {
                if matches!(suffix, b'*' | b'?' | b'-') {
                    p = ep + 1; // accept empty
                    continue;
                }
                return Ok(None); // '+' or no suffix
            }
            match suffix {
                b'?' => {
                    if let Some(r) = self.do_match(s + 1, ep + 1)? {
                        return Ok(Some(r));
                    }
                    p = ep + 1;
                }
                b'+' => return self.max_expand(s + 1, p, ep),
                b'*' => return self.max_expand(s, p, ep),
                b'-' => return self.min_expand(s, p, ep),
                _ => {
                    s += 1;
                    p = ep;
                }
            }
        }
    }

    /// `get_onecapture`: capture `i`, or the whole match `s..e` when the
    /// pattern has no captures and `i == 0`.
    fn get_onecapture(&self, i: usize, whole: Option<(usize, usize)>) -> PResult<Cap> {
        if i >= self.level {
            return match whole {
                Some((s, e)) if i == 0 => Ok(Cap::Str(s, e)),
                _ => Err(PatError::msg(format!("invalid capture index %{}", i + 1))),
            };
        }
        let (init, len) = self.capture[i];
        match len {
            CAP_UNFINISHED => Err(PatError::msg("unfinished capture")),
            CAP_POSITION => Ok(Cap::Pos(init + 1)),
            #[allow(clippy::cast_sign_loss)] // every other marker is negative.
            n => Ok(Cap::Str(init, init + n as usize)),
        }
    }

    /// `push_captures`: every capture (or the whole match, when there are
    /// none and `whole` is given), as Lua values.
    fn captures(&self, lua: &Lua, whole: Option<(usize, usize)>) -> Result<Vec<Value>, CallError> {
        let n = if self.level == 0 && whole.is_some() {
            1
        } else {
            self.level
        };
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            out.push(self.cap_value(lua, i, whole)?);
        }
        Ok(out)
    }

    fn cap_value(
        &self,
        lua: &Lua,
        i: usize,
        whole: Option<(usize, usize)>,
    ) -> Result<Value, CallError> {
        match self.get_onecapture(i, whole)? {
            Cap::Str(a, b) => Ok(Value::String(lua.create_string(&self.src[a..b])?)),
            #[allow(clippy::cast_possible_wrap)] // a subject position fits an i64.
            Cap::Pos(n) => Ok(Value::Integer(n as i64)),
        }
    }
}

/// An error from a driver.
enum CallError {
    /// A pattern-level error, or the budget abort.
    Pat(PatError),
    /// An error value raised by a gsub replacement function (or a table's
    /// `__index`), re-raised unchanged.
    Raised(Value),
    /// An mlua failure: out of memory creating a result, or a stack error.
    Lua(mlua::Error),
}

impl From<mlua::Error> for CallError {
    fn from(e: mlua::Error) -> Self {
        Self::Lua(e)
    }
}

impl From<PatError> for CallError {
    fn from(e: PatError) -> Self {
        Self::Pat(e)
    }
}

/// Convert a driver result to the status convention: `true, values...` or
/// `false, error_value, level`, where `level` is what the wrapper passes to
/// `error`. A pattern or argument error uses level 2, the wrapper's caller,
/// which is where C's `luaL_error` puts its position (level 1 from inside a
/// C function is its caller). A re-raised value, the budget abort and an
/// out-of-memory error use level 0: no position is added, as in C.
fn finish(lua: &Lua, r: Result<Vec<Value>, CallError>) -> mlua::Result<MultiValue> {
    let (err, level) = match r {
        Ok(vals) => {
            let mut out = Vec::with_capacity(vals.len() + 1);
            out.push(Value::Boolean(true));
            out.extend(vals);
            return Ok(MultiValue::from_vec(out));
        }
        Err(CallError::Pat(PatError::Msg(m))) => (Value::String(lua.create_string(m)?), 2),
        Err(CallError::Pat(PatError::Budget)) => (
            Value::String(lua.create_string(crate::mlua_backend::BUDGET_EXCEEDED)?),
            0,
        ),
        Err(CallError::Raised(v)) => (v, 0),
        // An allocation past the heap limit: raise what Lua's own library
        // would, a plain "not enough memory".
        Err(CallError::Lua(mlua::Error::MemoryError(_))) => {
            (Value::String(lua.create_string("not enough memory")?), 0)
        }
        Err(CallError::Lua(e)) => return Err(e),
    };
    Ok(MultiValue::from_vec(vec![
        Value::Boolean(false),
        err,
        Value::Integer(level),
    ]))
}

/// `luaL_typename`, for argument errors.
fn typename(v: Option<&Value>) -> &'static str {
    match v {
        None => "no value",
        Some(Value::Integer(_) | Value::Number(_)) => "number",
        Some(Value::LightUserData(_) | Value::UserData(_)) => "userdata",
        Some(v) => v.type_name(),
    }
}

fn arg_error(n: usize, fname: &str, what: &str) -> CallError {
    CallError::Pat(PatError::msg(format!(
        "bad argument #{} to '{fname}' ({what})",
        n + 1
    )))
}

/// `luaL_checklstring`: a string, or a number converted to one.
fn check_string(lua: &Lua, args: &[Value], n: usize, fname: &str) -> Result<LuaString, CallError> {
    match args.get(n) {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(v @ (Value::Integer(_) | Value::Number(_))) => lua
            .coerce_string(v.clone())?
            .ok_or_else(|| arg_error(n, fname, "string expected, got number")),
        v => Err(arg_error(
            n,
            fname,
            &format!("string expected, got {}", typename(v)),
        )),
    }
}

/// `luaL_optinteger`: absent or nil gives `def`; a float or numeric string
/// must have an exact integer value.
fn opt_integer(
    lua: &Lua,
    args: &[Value],
    n: usize,
    def: i64,
    fname: &str,
) -> Result<i64, CallError> {
    match args.get(n) {
        None | Some(Value::Nil) => Ok(def),
        Some(Value::Integer(i)) => Ok(*i),
        Some(v) => {
            if let Some(i) = lua.coerce_integer(v.clone())? {
                Ok(i)
            } else if lua.coerce_number(v.clone())?.is_some() {
                Err(arg_error(n, fname, "number has no integer representation"))
            } else {
                Err(arg_error(
                    n,
                    fname,
                    &format!("number expected, got {}", typename(Some(v))),
                ))
            }
        }
    }
}

/// `posrelatI`: a relative initial position (negative counts back from the
/// end), clipped to `[1, inf)`. The casts wrap exactly as C's `size_t` casts
/// do.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
const fn posrelat_i(pos: i64, len: usize) -> usize {
    if pos > 0 {
        pos as usize
    } else if pos == 0 || pos < -(len as i64) {
        1
    } else {
        (len as i64 + pos + 1) as usize
    }
}

const fn truthy(v: Option<&Value>) -> bool {
    !matches!(v, None | Some(Value::Nil | Value::Boolean(false)))
}

/// `lmemfind`, metered: the first occurrence of `needle` in `hay`. The scan
/// is linear in the common case (a first-byte search, then one compare per
/// candidate); the worst case, many near-miss candidates, is quadratic, which
/// is why every byte examined is charged.
fn plain_find(hay: &[u8], needle: &[u8], charge: &mut Charge<'_>) -> PResult<Option<usize>> {
    let Some((&first, rest)) = needle.split_first() else {
        return Ok(Some(0)); // empty strings are everywhere
    };
    if needle.len() > hay.len() {
        return Ok(None);
    }
    let last = hay.len() - needle.len();
    let mut i = 0usize;
    while i <= last {
        let Some(k) = hay[i..=last].iter().position(|&b| b == first) else {
            charge.bytes(last + 1 - i)?;
            return Ok(None);
        };
        charge.bytes(k + 1 + rest.len())?;
        i += k;
        if &hay[i + 1..i + needle.len()] == rest {
            return Ok(Some(i));
        }
        i += 1;
    }
    Ok(None)
}

/// `str_find_aux`: `string.find` (`find == true`) and `string.match`.
fn str_find_aux(
    lua: &Lua,
    args: &[Value],
    find: bool,
    meter: &Meter,
) -> Result<Vec<Value>, CallError> {
    let fname = if find { "find" } else { "match" };
    let s = check_string(lua, args, 0, fname)?;
    let p = check_string(lua, args, 1, fname)?;
    let s = s.as_bytes();
    let p = p.as_bytes();
    let (s, p): (&[u8], &[u8]) = (&s, &p);
    let init = posrelat_i(opt_integer(lua, args, 2, 1, fname)?, s.len()) - 1;
    if init > s.len() {
        return Ok(vec![Value::Nil]); // start after the string's end
    }
    let mut charge = Charge::new(meter);
    let r = find_in(lua, s, p, init, find, truthy(args.get(3)), &mut charge);
    charge.flush()?;
    r
}

fn find_in(
    lua: &Lua,
    s: &[u8],
    p: &[u8],
    init: usize,
    find: bool,
    plain: bool,
    charge: &mut Charge<'_>,
) -> Result<Vec<Value>, CallError> {
    #[allow(clippy::cast_possible_wrap)] // subject positions fit an i64.
    let pos = |n: usize| Value::Integer(n as i64);
    if find && (plain || !p.iter().any(|c| SPECIALS.contains(c))) {
        // A plain search.
        if let Some(at) = plain_find(&s[init..], p, charge)? {
            let at = init + at;
            return Ok(vec![pos(at + 1), pos(at + p.len())]);
        }
        return Ok(vec![Value::Nil]);
    }
    let anchor = p.first() == Some(&b'^');
    let p = if anchor { &p[1..] } else { p };
    let mut ms = MatchState::new(s, p, charge);
    let mut s1 = init;
    loop {
        ms.reprep();
        if let Some(res) = ms.do_match(s1, 0)? {
            return if find {
                let mut out = vec![pos(s1 + 1), pos(res)];
                out.extend(ms.captures(lua, None)?);
                Ok(out)
            } else {
                ms.captures(lua, Some((s1, res)))
            };
        }
        if s1 < s.len() && !anchor {
            s1 += 1;
        } else {
            return Ok(vec![Value::Nil]);
        }
    }
}

/// The per-iterator state of a `gmatch` closure.
struct GmState {
    /// Current position in the subject.
    src: usize,
    /// End of the last match (an empty match there does not count again).
    lastmatch: Option<usize>,
}

/// `gmatch`: returns the iterator (a Rust closure, wrapped in Lua).
fn gmatch(lua: &Lua, args: &[Value], meter: &Meter) -> Result<Vec<Value>, CallError> {
    let s = check_string(lua, args, 0, "gmatch")?;
    let p = check_string(lua, args, 1, "gmatch")?;
    let ls = s.as_bytes().len();
    let mut init = posrelat_i(opt_integer(lua, args, 2, 1, "gmatch")?, ls) - 1;
    if init > ls {
        init = ls + 1; // start after the end: never matches
    }
    let state = Mutex::new(GmState {
        src: init,
        lastmatch: None,
    });
    let meter = meter.clone();
    // The closure keeps the subject and pattern alive as Lua strings (no host
    // copy), exactly as the C iterator keeps them as upvalues.
    let iter = lua.create_function(move |lua, _: MultiValue| {
        let r = gmatch_step(lua, &s, &p, &state, &meter);
        finish(lua, r)
    })?;
    Ok(vec![Value::Function(iter)])
}

fn gmatch_step(
    lua: &Lua,
    s: &LuaString,
    p: &LuaString,
    state: &Mutex<GmState>,
    meter: &Meter,
) -> Result<Vec<Value>, CallError> {
    let (s, p) = (s.as_bytes(), p.as_bytes());
    let (s, p): (&[u8], &[u8]) = (&s, &p);
    // The lock is taken only to read and to update the position: nothing
    // re-enters Lua while it is held.
    let (mut src, lastmatch) = {
        let st = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (st.src, st.lastmatch)
    };
    let mut charge = Charge::new(meter);
    let mut ms = MatchState::new(s, p, &mut charge);
    let mut found = None;
    while src <= s.len() {
        ms.reprep();
        if let Some(e) = ms.do_match(src, 0)?
            && Some(e) != lastmatch
        {
            found = Some((src, e));
            break;
        }
        src += 1;
    }
    // Not found: the iterator ends, and (as in C) the position is not moved.
    let r = found.map_or_else(
        || Ok(Vec::new()),
        |(start, e)| {
            let mut st = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            st.src = e;
            st.lastmatch = Some(e);
            drop(st);
            ms.captures(lua, Some((start, e)))
        },
    );
    charge.flush()?;
    r
}

/// Handles `gsub` needs to call back into Lua without losing an error value.
struct GsubCtx<'a> {
    /// The original C `pcall`, captured before the sandbox wraps it. A
    /// replacement function (or a table's `__index`) is called through it,
    /// so an error it raises comes back as its own value and is re-raised
    /// unchanged; the budget flag is checked separately.
    raw_pcall: &'a Function,
    /// `function(t, k) return t[k] end`, so table indexing honours `__index`
    /// exactly as `lua_gettable` does.
    index: &'a Function,
    /// Host bytes the output buffer may grow to before the call fails the
    /// way Lua's own buffer would: "not enough memory".
    max_out: usize,
}

/// The replacement argument of `gsub`, classified once.
enum Repl {
    Str(LuaString),
    Table(Table),
    Func(Function),
}

/// `str_gsub`.
#[allow(clippy::too_many_lines)] // one driver, as in C.
fn gsub(
    lua: &Lua,
    args: &[Value],
    meter: &Meter,
    ctx: &GsubCtx<'_>,
) -> Result<Vec<Value>, CallError> {
    let src_s = check_string(lua, args, 0, "gsub")?;
    let pat_s = check_string(lua, args, 1, "gsub")?;
    let srcb = src_s.as_bytes();
    let patb = pat_s.as_bytes();
    let (src, p): (&[u8], &[u8]) = (&srcb, &patb);
    #[allow(clippy::cast_possible_wrap)]
    let max_s = opt_integer(lua, args, 3, src.len() as i64 + 1, "gsub")?;
    let repl = match args.get(2) {
        Some(Value::String(s)) => Repl::Str(s.clone()),
        Some(v @ (Value::Integer(_) | Value::Number(_))) => {
            Repl::Str(lua.coerce_string(v.clone())?.ok_or_else(|| {
                arg_error(2, "gsub", "string/function/table expected, got number")
            })?)
        }
        Some(Value::Table(t)) => Repl::Table(t.clone()),
        Some(Value::Function(f)) => Repl::Func(f.clone()),
        v => {
            return Err(arg_error(
                2,
                "gsub",
                &format!("string/function/table expected, got {}", typename(v)),
            ));
        }
    };
    let anchor = p.first() == Some(&b'^');
    let p = if anchor { &p[1..] } else { p };

    let mut charge = Charge::new(meter);
    let mut out: Vec<u8> = Vec::new();
    let mut changed = false;
    let mut n: i64 = 0;
    let mut pos = 0usize;
    let mut lastmatch: Option<usize> = None;
    {
        let mut ms = MatchState::new(src, p, &mut charge);
        while n < max_s {
            ms.reprep();
            let e = ms.do_match(pos, 0)?;
            if let Some(e) = e
                && Some(e) != lastmatch
            {
                n += 1;
                changed |= add_value(lua, &mut ms, &mut out, pos, e, &repl, ctx)?;
                pos = e;
                lastmatch = Some(e);
            } else if pos < src.len() {
                out.push(src[pos]); // otherwise skip one character
                pos += 1;
            } else {
                break; // end of subject
            }
            if out.len() > ctx.max_out {
                return Err(CallError::Lua(mlua::Error::MemoryError(String::new())));
            }
            if anchor {
                break;
            }
        }
    }
    charge.flush()?;
    let result = if changed {
        out.extend_from_slice(&src[pos..]);
        if out.len() > ctx.max_out {
            return Err(CallError::Lua(mlua::Error::MemoryError(String::new())));
        }
        Value::String(lua.create_string(&out)?)
    } else {
        Value::String(src_s) // no changes: the original string
    };
    Ok(vec![result, Value::Integer(n)])
}

/// `add_value`: append the replacement for the match `s..e`. Returns whether
/// the subject changed (a function or table giving nil / false keeps the
/// original text and does not count as a change).
fn add_value(
    lua: &Lua,
    ms: &mut MatchState<'_, '_>,
    out: &mut Vec<u8>,
    s: usize,
    e: usize,
    repl: &Repl,
    ctx: &GsubCtx<'_>,
) -> Result<bool, CallError> {
    let value = match repl {
        Repl::Str(news) => {
            add_s(ms, out, s, e, &news.as_bytes())?;
            return Ok(true);
        }
        Repl::Func(f) => {
            let caps = ms.captures(lua, Some((s, e)))?;
            let mut call = Vec::with_capacity(caps.len() + 1);
            call.push(Value::Function(f.clone()));
            call.extend(caps);
            protected(ctx, ms, MultiValue::from_vec(call))?
        }
        Repl::Table(t) => {
            let key = ms.cap_value(lua, 0, Some((s, e)))?;
            let call = vec![
                Value::Function(ctx.index.clone()),
                Value::Table(t.clone()),
                key,
            ];
            protected(ctx, ms, MultiValue::from_vec(call))?
        }
    };
    match value {
        Value::Nil | Value::Boolean(false) => {
            out.extend_from_slice(&ms.src[s..e]); // keep the original text
            ms.charge.bytes(e - s)?;
            Ok(false)
        }
        v @ (Value::String(_) | Value::Integer(_) | Value::Number(_)) => {
            let text = lua
                .coerce_string(v)?
                .ok_or_else(|| PatError::msg("invalid replacement value (a number)"))?;
            let text = text.as_bytes();
            ms.charge.bytes(text.len())?;
            out.extend_from_slice(&text);
            Ok(true)
        }
        other => Err(CallError::Pat(PatError::msg(format!(
            "invalid replacement value (a {})",
            typename(Some(&other))
        )))),
    }
}

/// Call `args[0](args[1..])` through the raw `pcall`, returning its first
/// result. A budget abort inside the call is re-raised as the budget error;
/// any other error is re-raised as the value the callee raised.
fn protected(
    ctx: &GsubCtx<'_>,
    ms: &mut MatchState<'_, '_>,
    call: MultiValue,
) -> Result<Value, CallError> {
    // Commit the pending steps first, so the callee's own hook charges land
    // on an up-to-date count.
    ms.charge.flush()?;
    let mut r = ctx.raw_pcall.call::<MultiValue>(call)?.into_iter();
    let ok = matches!(r.next(), Some(Value::Boolean(true)));
    if ms.charge.meter.tripped.get() {
        return Err(CallError::Pat(PatError::Budget));
    }
    let v = r.next().unwrap_or(Value::Nil);
    if ok { Ok(v) } else { Err(CallError::Raised(v)) }
}

/// `add_s`: expand a replacement string (`%0`-`%9`, `%%`).
fn add_s(
    ms: &mut MatchState<'_, '_>,
    out: &mut Vec<u8>,
    start: usize,
    end: usize,
    news: &[u8],
) -> Result<(), CallError> {
    ms.charge.bytes(news.len())?;
    let mut from = 0usize;
    while let Some(off) = news[from..].iter().position(|&b| b == L_ESC) {
        out.extend_from_slice(&news[from..from + off]);
        let at = from + off + 1; // the character after the escape
        let next = news.get(at).copied().unwrap_or(0);
        if next == L_ESC {
            out.push(L_ESC);
        } else if next == b'0' {
            out.extend_from_slice(&ms.src[start..end]);
            ms.charge.bytes(end - start)?;
        } else if next.is_ascii_digit() {
            match ms.get_onecapture(usize::from(next - b'1'), Some((start, end)))? {
                Cap::Str(lo, hi) => {
                    out.extend_from_slice(&ms.src[lo..hi]);
                    ms.charge.bytes(hi - lo)?;
                }
                Cap::Pos(pos) => out.extend_from_slice(pos.to_string().as_bytes()),
            }
        } else {
            return Err(CallError::Pat(PatError::msg(
                "invalid use of '%' in replacement string",
            )));
        }
        from = at + 1;
    }
    out.extend_from_slice(&news[from.min(news.len())..]);
    Ok(())
}

/// The Lua side of the calling convention (see the module docs). Runs once
/// at VM creation with the `string` table and the four Rust functions.
/// Replacing the entries *in* the `string` table also replaces the method
/// form (`s:find(...)`), because the string metatable's `__index` is that
/// same table.
const WRAPPERS: &str = r"
local string, rs_find, rs_match, rs_gmatch, rs_gsub = ...
local error = error
local function check(ok, ...)
    if ok then
        return ...
    end
    local e, level = ...
    error(e, level)
end
string.find = function(...) return check(rs_find(...)) end
string.match = function(...) return check(rs_match(...)) end
string.gsub = function(...) return check(rs_gsub(...)) end
string.gmatch = function(...)
    -- Not `check(...)`: that call is not a tail call here, so its level 2
    -- would be this wrapper. `error` straight from the wrapper reaches the
    -- same caller.
    local ok, it, level = rs_gmatch(...)
    if not ok then
        error(it, level)
    end
    return function() return check(it()) end
end
";

/// Replace `string.find` / `match` / `gmatch` / `gsub` in `lua` with the
/// metered implementation, charging `meter`. `max_out` bounds gsub's output
/// buffer (host memory). Must run before the sandbox wraps `pcall`, whose
/// original this captures.
pub fn install(lua: &Lua, meter: &Meter, max_out: usize) -> mlua::Result<()> {
    let string: Table = lua.globals().get("string")?;
    let raw_pcall: Function = lua.globals().get("pcall")?;
    let index: Function = lua.load("return function(t, k) return t[k] end").eval()?;

    let m = meter.clone();
    let find = lua.create_function(move |lua, args: MultiValue| {
        let args = args.into_vec();
        finish(lua, str_find_aux(lua, &args, true, &m))
    })?;
    let m = meter.clone();
    let smatch = lua.create_function(move |lua, args: MultiValue| {
        let args = args.into_vec();
        finish(lua, str_find_aux(lua, &args, false, &m))
    })?;
    let m = meter.clone();
    let gm = lua.create_function(move |lua, args: MultiValue| {
        let args = args.into_vec();
        finish(lua, gmatch(lua, &args, &m))
    })?;
    let m = meter.clone();
    let gs = lua.create_function(move |lua, args: MultiValue| {
        let args = args.into_vec();
        let ctx = GsubCtx {
            raw_pcall: &raw_pcall,
            index: &index,
            max_out: max_out.saturating_sub(lua.used_memory()),
        };
        finish(lua, gsub(lua, &args, &m, &ctx))
    })?;
    lua.load(WRAPPERS)
        .set_name("=string-patterns")
        .call::<()>((string, find, smatch, gm, gs))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A meter with a budget no parity case can reach.
    fn open_meter() -> Meter {
        Meter {
            count: SharedCounter::new(0),
            budget: SharedCounter::new(u64::MAX),
            tripped: SharedFlag::new(false),
        }
    }

    /// Serialize every value an expression returns (or the error it raises),
    /// with types, so two VMs can be compared exactly.
    const HARNESS: &str = r"
        local function ser(...)
            local t = {}
            for i = 1, select('#', ...) do
                local v = select(i, ...)
                t[#t + 1] = type(v) .. ':' .. tostring(v)
            end
            return table.concat(t, '|')
        end
        return function(f) return ser(pcall(f)) end
    ";

    fn run(lua: &Lua, expr: &str) -> String {
        let harness: Function = lua.load(HARNESS).eval().expect("harness");
        // Not `return {expr}`: in a tail call, C reports the error position
        // of the Lua frame the call replaced, which a Lua wrapper cannot see
        // (see `WRAPPERS`). Inside `table.pack` the call is an ordinary one.
        let f: Function = lua
            .load(format!(
                "return function() local t = table.pack({expr}) return table.unpack(t, 1, t.n) end"
            ))
            .eval()
            .unwrap_or_else(|e| panic!("{expr}: {e}"));
        // Results may be arbitrary bytes (`\200`), so compare them lossily
        // decoded; both sides decode the same bytes the same way.
        harness
            .call::<LuaString>(f)
            .expect("harness call")
            .to_string_lossy()
    }

    /// Two VMs: the stock one (Lua's C `lstrlib.c`) and one with this
    /// module's replacement installed.
    fn vms() -> (Lua, Lua) {
        let reference = Lua::new();
        let metered = Lua::new();
        install(&metered, &open_meter(), usize::MAX).expect("install");
        (reference, metered)
    }

    /// PARITY: every case gives the same values (types included), and the
    /// same error message, through the C library and through this module.
    #[test]
    #[allow(clippy::too_many_lines)] // one table of cases.
    fn matches_the_c_library_case_for_case() {
        let cases = [
            // find: plain, specials, init, anchors, captures.
            r#"string.find("hello world", "o w")"#,
            r#"string.find("hello world", "o", 6)"#,
            r#"string.find("hello world", "o", -3)"#,
            r#"string.find("hello world", "o", -100)"#,
            r#"string.find("hello world", "o", 100)"#,
            r#"string.find("hello world", "", 12)"#,
            r#"string.find("hello world", "", 13)"#,
            r#"string.find("hello", "")"#,
            r#"string.find("", "")"#,
            r#"string.find("a.b", ".", 1, true)"#,
            r#"string.find("a.b", "%.")"#,
            r#"string.find("a+b", "+", 1, true)"#,
            r#"string.find("aab", "ab", 1, true)"#,
            r#"string.find("hello", "l+")"#,
            r#"string.find("hello", "(h)(e)")"#,
            r#"string.find("hello", "()ll()")"#,
            r#"string.find("hello", "^h")"#,
            r#"string.find("hello", "^e")"#,
            r#"string.find("hello", "o$")"#,
            r#"string.find("hel$lo", "l$l")"#,
            r#"string.find("hello", "^")"#,
            r#"string.find("x^y", "^^")"#,
            r"string.find(12345, 34)",
            r#"string.find("abc", "b", 2.0)"#,
            r#"string.find("abc", "b", "2")"#,
            r#"string.find("a\0b", "\0")"#,
            r#"string.find("a\0b", "%z")"#,
            r#"string.find("a\0b", "[\0]")"#,
            // match: classes and sets.
            r#"string.match("  key = value  ", "^%s*(%w+)%s*=%s*(%w+)%s*$")"#,
            r#"string.match("2024-01-02", "(%d+)-(%d+)-(%d+)")"#,
            r#"string.match("Hello World", "%u%l+")"#,
            r#"string.match("tab\there", "%c")"#,
            r#"string.match("v\vx", "%s")"#,
            r#"string.match("a1_b", "[%w_]+")"#,
            r#"string.match("x]y", "[]]")"#,
            r#"string.match("x]y^", "[^]]+")"#,
            r#"string.match("a-z", "[a%-z]+")"#,
            r#"string.match("m", "[a-z]")"#,
            r#"string.match("-", "[a-]")"#,
            r#"string.match("abc", "[^a]+")"#,
            r#"string.match("0x1F!", "%x+")"#,
            r#"string.match("punct!?", "%p+")"#,
            r#"string.match("AbC", "%U")"#,
            r#"string.match("abc", "%A")"#,
            r#"string.match("é", ".")"#,
            r#"string.match("\200\201", "[\200-\210]+")"#,
            r#"string.match("aaa", "a-")"#,
            r#"string.match("aaa", "a-$")"#,
            r#"string.match("aaab", "a*b")"#,
            r#"string.match("b", "a*b")"#,
            r#"string.match("b", "a+b")"#,
            r#"string.match("ab", "a?b")"#,
            r#"string.match("b", "a?b")"#,
            r#"string.match("xyz", "(x)(y)(z)")"#,
            r#"string.match("xyz", "((x)(y))")"#,
            r#"string.match("xyz", "()")"#,
            r#"string.match("hello", ".-(l+)(.*)")"#,
            r#"string.match("THE (quick) fox", "%((%a+)%)")"#,
            r#"string.match("f(a(b)c)d", "%b()")"#,
            r#"string.match("f(a(b)c", "%b()")"#,
            r#"string.match("[[x]]", "%b[]")"#,
            r#"string.match("THE (quick) fox", "%f[%a]%a+")"#,
            r#"string.match("hello world", "%f[%w]%w+$")"#,
            r#"string.match("hello", "%f[%z]")"#,
            r#"string.match("abcabc", "(abc)%1")"#,
            r#"string.match("abcabd", "(abc)%1")"#,
            r#"string.match("aXa", "(%a)X%1")"#,
            r#"string.match("hello", "(h)(e)(l)(l)(o)")"#,
            r#"string.match("abc", "b", -1)"#,
            r#"string.match("abc", "b", 10)"#,
            // gmatch: iteration, empty matches, init, anchors.
            r#"(function() local t = {} for w in string.gmatch("one two  three", "%a+") do t[#t+1] = w end return table.concat(t, ",") end)()"#,
            r#"(function() local t = {} for k, v in string.gmatch("a=1, b=2", "(%w+)=(%w+)") do t[#t+1] = k .. v end return table.concat(t, ",") end)()"#,
            r#"(function() local t = {} for w in string.gmatch("abc", "") do t[#t+1] = tostring(w) end return #t end)()"#,
            r#"(function() local t = {} for p in string.gmatch("abc", "()") do t[#t+1] = p end return table.concat(t, ",") end)()"#,
            r#"(function() local t = {} for w in string.gmatch("xaxbx", "x*") do t[#t+1] = "[" .. w .. "]" end return table.concat(t) end)()"#,
            r#"(function() local t = {} for w in string.gmatch("hello world", "%a+", 3) do t[#t+1] = w end return table.concat(t, ",") end)()"#,
            r#"(function() local t = {} for w in string.gmatch("hello", "%a+", 100) do t[#t+1] = w end return #t end)()"#,
            r#"(function() local t = {} for w in string.gmatch("^a^b", "^%a") do t[#t+1] = w end return table.concat(t, ",") end)()"#,
            r#"(function() local it = string.gmatch("ab", "%a") it() it() return it(), it() end)()"#,
            // gsub: string / table / function replacements, %n, max n.
            r#"string.gsub("hello world", "o", "0")"#,
            r#"string.gsub("hello world", "(o)", "[%1]")"#,
            r#"string.gsub("hello world", "o", "%0%0")"#,
            r#"string.gsub("hello", "", "-")"#,
            r#"string.gsub("abc", "%w", "%%")"#,
            r#"string.gsub("hello world", "%w+", "%0 %0", 1)"#,
            r#"string.gsub("hello world", "o", "0", 0)"#,
            r#"string.gsub("hello world", "o", "0", -1)"#,
            r#"string.gsub("hello world", "^h", "H")"#,
            r#"string.gsub("hello world", "^o", "O")"#,
            r#"string.gsub("abc", "()", "%1")"#,
            r#"string.gsub("abc", "b", 7)"#,
            r#"string.gsub("hello world", "%w+", {hello = "HI", world = false})"#,
            r#"string.gsub("a b", "%w", setmetatable({}, {__index = function(_, k) return k:upper() end}))"#,
            r#"string.gsub("$name is $age", "%$(%w+)", {name = "Ann", age = 7})"#,
            r#"string.gsub("hello world", "(%w+) (%w+)", function(a, b) return b .. " " .. a end)"#,
            r#"string.gsub("abc", "%w", function(c) if c == "b" then return nil end return c:upper() end)"#,
            r#"string.gsub("abc", "%w", function() return 1.5 end)"#,
            r#"string.gsub("abc", "x*", "-")"#,
            r#"string.gsub("abc", ".", {a = 1})"#,
            r#"string.gsub("hello", "l", "L", 1.0)"#,
            r#"string.gsub("abc", "", "")"#,
            r#"select(2, string.gsub("aaa", "a", "b"))"#,
            // Method form resolves through the string metatable.
            r#"("hello"):find("l+")"#,
            r#"("hello"):gsub("l", "L")"#,
            r#"("k=v"):match("(%w)=(%w)")"#,
            // Malformed patterns and bad replacements: the exact message.
            r#"string.find("abc", "%")"#,
            r#"string.find("abc", "[a")"#,
            r#"string.find("abc", "[^")"#,
            r#"string.find("abc", "[]")"#,
            r#"string.find("abc", "[%")"#,
            r#"string.find("abc", "%b")"#,
            r#"string.find("abc", "%ba")"#,
            r#"string.find("abc", "%f")"#,
            r#"string.find("abc", "%fa")"#,
            r#"string.find("abc", "(")"#,
            r#"string.find("abc", ")")"#,
            r#"string.find("abc", "%1")"#,
            r#"string.find("abc", "(a)%2")"#,
            r#"string.find("abc", "(a%1)")"#,
            r#"string.find("abc", "%0")"#,
            r#"string.match("abc", "()%1")"#,
            r#"string.find(string.rep("a", 300), string.rep("(", 40))"#,
            r#"string.find(string.rep("a", 300), string.rep("a?", 300))"#,
            r#"string.gsub("abc", "(%w)", "%2")"#,
            r#"string.gsub("abc", "%w", "%x")"#,
            r#"string.gsub("abc", "%w", "%")"#,
            r#"string.gsub("abc", "%w", function() return {} end)"#,
            r#"string.gsub("abc", "%w", function() return true end)"#,
            r#"string.gsub("abc", "%w", function() error("boom") end)"#,
            r#"(function() local ok, e = pcall(string.gsub, "abc", "%w", function() error({code = 1}) end) return ok, type(e), e.code end)()"#,
            r#"string.gsub("abc", "%w", function() error("x", 0) end)"#,
            // Called straight from `pcall` (a C function): no position.
            r#"pcall(string.find, "abc", "%")"#,
            r#"pcall(string.gsub, "abc", "%w", "%9")"#,
            r#"pcall(string.gmatch("abc", "(%w"))"#,
            // Deliberately absent: `pcall(string.find, nil, "x")`. With no
            // Lua call site to name the function, C looks it up in the loaded
            // libraries and says 'string.find'; the drivers say 'find'. The
            // one known divergence (module docs, "Known divergence").
            // Bad arguments, called from Lua: C names the function from the
            // call site ('find'), which is also the name the drivers use.
            r#"string.find(nil, "x")"#,
            r#"string.find("x")"#,
            r#"string.find("x", {})"#,
            r#"string.find("x", "x", "y")"#,
            r#"string.find("x", "x", 1.5)"#,
            r"string.match()",
            r#"string.gmatch("x")"#,
            r#"string.gsub("x", "x")"#,
            r#"string.gsub("x", "x", true)"#,
            r#"string.gsub("x", "x", "y", "z")"#,
            r#"string.gsub("x", "x", true, "z")"#,
        ];
        let (reference, metered) = vms();
        let mut failures = Vec::new();
        for expr in cases {
            let want = run(&reference, expr);
            let got = run(&metered, expr);
            if want != got {
                failures.push(format!("{expr}\n    C:    {want}\n    Rust: {got}"));
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} cases differ:\n{}",
            failures.len(),
            cases.len(),
            failures.join("\n")
        );
    }

    /// The pathological backtracking case is charged: with a small budget it
    /// stops with the budget error and trips the flag, as the hook does,
    /// instead of running roughly n^4 C steps.
    ///
    /// Run on its own thread with a 10 s watchdog: on a regressed tree this
    /// case never ends, and it must fail rather than stall the test run.
    #[test]
    fn a_backtracking_pattern_is_charged_to_the_budget() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let lua = Lua::new();
            let meter = Meter {
                count: SharedCounter::new(0),
                budget: SharedCounter::new(100_000),
                tripped: SharedFlag::new(false),
            };
            install(&lua, &meter, usize::MAX).expect("install");
            let r = lua
                .load("local s = string.rep('a', 3000) return s:find('.-.-.-.-b')")
                .exec()
                .map_err(|e| e.to_string());
            let _ = tx.send((r, meter.tripped.get()));
        });
        let (r, tripped) = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("did not return within 10 s: a hang");
        let err = r.expect_err("the budget stops it");
        assert!(err.contains("budget"), "{err}");
        assert!(tripped, "the trip flag is set");
    }

    /// Plain find is a linear scan charged per 64 bytes, so an ordinary
    /// search of a large subject costs little.
    #[test]
    fn plain_find_is_cheap() {
        let lua = Lua::new();
        let meter = open_meter();
        install(&lua, &meter, usize::MAX).expect("install");
        let at: i64 = lua
            .load("local s = string.rep('a', 1000000) .. 'b' return (s:find('b', 1, true))")
            .eval()
            .expect("found");
        assert_eq!(at, 1_000_001);
        let spent = meter.count.get();
        assert!(spent < 20_000, "one scan of 1 MB charged {spent} steps");
    }

    /// gsub's output buffer is host memory; it is capped like the Lua heap,
    /// and the failure reads as Lua's own out-of-memory error.
    #[test]
    fn gsub_output_is_bounded() {
        let lua = Lua::new();
        install(&lua, &open_meter(), 1 << 20).expect("install");
        let err = lua
            .load("return string.gsub(string.rep('a', 1024), 'a', string.rep('b', 4096))")
            .exec()
            .expect_err("4 MiB of output exceeds a 1 MiB cap");
        assert!(err.to_string().contains("not enough memory"), "{err}");
    }
}

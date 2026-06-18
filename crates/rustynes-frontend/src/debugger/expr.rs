// The tokenizer's scan cursor (`c`) and the test fixtures (`a`/`b`/`x`/`y`,
// matching the 6502 register names this module evaluates) are intentionally
// single-character; the field-reassign pattern is the clearest way to build the
// register-varied test contexts off a `Default` base.
#![allow(clippy::many_single_char_names, clippy::field_reassign_with_default)]
//! Debugger expression evaluator (v1.6.0 "Studio" Workstream C, C1 keystone).
//!
//! A small Mesen-`ExpressionEvaluator`-style language used to drive conditional
//! breakpoints, read/write/exec watchpoints, the watch window, and conditional
//! trace logging. It is the keystone of Workstream C: parse once, evaluate every
//! frame against the just-finished frame's observational logs.
//!
//! # Grammar (precedence low → high)
//!
//! ```text
//! expr     := ternary
//! ternary  := logic_or ( '?' expr ':' ternary )?
//! logic_or := logic_and ( '||' logic_and )*
//! logic_and:= bit_or    ( '&&' bit_or )*
//! bit_or   := bit_xor   ( '|' bit_xor )*
//! bit_xor  := bit_and   ( '^' bit_and )*
//! bit_and  := equality  ( '&' equality )*
//! equality := relation  ( ('==' | '!=') relation )*
//! relation := shift      ( ('<' | '>' | '<=' | '>=') shift )*
//! shift    := add        ( ('<<' | '>>') add )*
//! add      := mul        ( ('+' | '-') mul )*
//! mul      := unary      ( ('*' | '/' | '%') unary )*
//! unary    := ('-' | '!' | '~')? primary
//! primary  := number | ident | '[' expr ']' | '{' expr '}' | '(' expr ')'
//! ```
//!
//! - Numbers: decimal (`42`), hex (`$1234` or `0x1234`), binary (`%1010`).
//! - `[addr]` reads one byte from the CPU bus; `{addr}` reads a little-endian
//!   16-bit word (`[addr] | [addr+1] << 8`). Both go through
//!   [`EvalContext::peek`], i.e. a non-mutating bus peek.
//! - Identifiers: CPU regs `a x y s p pc`; PPU `scanline cycle frame`; and the
//!   per-access context tokens `value address isread iswrite isexec` (case-
//!   insensitive; `isRead`/`isWrite`/`isExec` are accepted spellings). The
//!   context tokens carry the access being tested when a watchpoint replays the
//!   read/write/exec log; in a context-free evaluation (e.g. the watch window)
//!   they resolve to `0`.
//!
//! All arithmetic is on `i64`; comparisons / logical ops yield `1` or `0`. The
//! evaluator is **pure** — it only *reads* through [`EvalContext`], so it never
//! perturbs the deterministic core (it mirrors the observational `onExec` /
//! `onRead` / `onWrite` Lua replay; see ADR 0010).

use core::fmt;

/// The access context a watchpoint/breakpoint expression is evaluated against.
///
/// For a context-free evaluation (the watch window, or a breakpoint with no
/// access in flight) the access tokens read as zero — `kind` is [`None`].
#[derive(Clone, Copy, Debug, Default)]
pub struct AccessContext {
    /// The byte read or written (`value`), or `0` when no access is in flight.
    pub value: u8,
    /// The accessed address (`address`), or `0` when none.
    pub address: u16,
    /// The kind of access in flight, if any (drives `isread`/`iswrite`/`isexec`).
    pub kind: Option<AccessKind>,
}

/// The kind of memory access an expression's context tokens describe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessKind {
    /// A CPU read (`isRead` → 1).
    Read,
    /// A CPU write (`isWrite` → 1).
    Write,
    /// An instruction fetch / exec (`isExec` → 1).
    Exec,
}

/// What an evaluator reads from the live machine. Implemented by the frontend
/// over a `&Nes` snapshot; the unit tests use a lightweight fake.
///
/// Every method is a *read* — an implementation MUST NOT mutate
/// emulator-visible state (the contract that keeps determinism intact).
pub trait EvalContext {
    /// The accumulator.
    fn a(&self) -> u8;
    /// The X index.
    fn x(&self) -> u8;
    /// The Y index.
    fn y(&self) -> u8;
    /// The stack pointer.
    fn s(&self) -> u8;
    /// The processor-status byte.
    fn p(&self) -> u8;
    /// The program counter.
    fn pc(&self) -> u16;
    /// The current PPU scanline (`-1` pre-render .. `260`).
    fn scanline(&self) -> i16;
    /// The current PPU dot / "cycle" (`0..=340`).
    fn dot(&self) -> u16;
    /// The PPU frame counter.
    fn frame(&self) -> u64;
    /// A non-mutating one-byte CPU-bus peek.
    fn peek(&self, addr: u16) -> u8;
    /// The access context (value/address/isread/...) being tested.
    fn access(&self) -> AccessContext;
}

/// A parse-time error (tokenizer or parser), with a human-readable message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(i64),
    Ident(String),
    // Operators / punctuation.
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    AmpAmp,
    Pipe,
    PipePipe,
    Caret,
    Tilde,
    Bang,
    EqEq,
    BangEq,
    Lt,
    Gt,
    Le,
    Ge,
    Shl,
    Shr,
    Question,
    Colon,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
}

fn tokenize(src: &str) -> Result<Vec<Tok>, ParseError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '+' => {
                out.push(Tok::Plus);
                i += 1;
            }
            '-' => {
                out.push(Tok::Minus);
                i += 1;
            }
            '*' => {
                out.push(Tok::Star);
                i += 1;
            }
            '/' => {
                out.push(Tok::Slash);
                i += 1;
            }
            '%' => {
                // `%1010` is a binary literal; `%` between values is modulo.
                if i + 1 < chars.len() && (chars[i + 1] == '0' || chars[i + 1] == '1') {
                    let start = i + 1;
                    let mut j = start;
                    while j < chars.len() && (chars[j] == '0' || chars[j] == '1') {
                        j += 1;
                    }
                    let s: String = chars[start..j].iter().collect();
                    let v = i64::from_str_radix(&s, 2)
                        .map_err(|_| ParseError(format!("bad binary literal %{s}")))?;
                    out.push(Tok::Num(v));
                    i = j;
                } else {
                    out.push(Tok::Percent);
                    i += 1;
                }
            }
            '&' => {
                if i + 1 < chars.len() && chars[i + 1] == '&' {
                    out.push(Tok::AmpAmp);
                    i += 2;
                } else {
                    out.push(Tok::Amp);
                    i += 1;
                }
            }
            '|' => {
                if i + 1 < chars.len() && chars[i + 1] == '|' {
                    out.push(Tok::PipePipe);
                    i += 2;
                } else {
                    out.push(Tok::Pipe);
                    i += 1;
                }
            }
            '^' => {
                out.push(Tok::Caret);
                i += 1;
            }
            '~' => {
                out.push(Tok::Tilde);
                i += 1;
            }
            '!' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    out.push(Tok::BangEq);
                    i += 2;
                } else {
                    out.push(Tok::Bang);
                    i += 1;
                }
            }
            '=' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    out.push(Tok::EqEq);
                    i += 2;
                } else {
                    return Err(ParseError("'=' (use '==' for equality)".into()));
                }
            }
            '<' => {
                if i + 1 < chars.len() && chars[i + 1] == '<' {
                    out.push(Tok::Shl);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '=' {
                    out.push(Tok::Le);
                    i += 2;
                } else {
                    out.push(Tok::Lt);
                    i += 1;
                }
            }
            '>' => {
                if i + 1 < chars.len() && chars[i + 1] == '>' {
                    out.push(Tok::Shr);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '=' {
                    out.push(Tok::Ge);
                    i += 2;
                } else {
                    out.push(Tok::Gt);
                    i += 1;
                }
            }
            '?' => {
                out.push(Tok::Question);
                i += 1;
            }
            ':' => {
                out.push(Tok::Colon);
                i += 1;
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            '[' => {
                out.push(Tok::LBracket);
                i += 1;
            }
            ']' => {
                out.push(Tok::RBracket);
                i += 1;
            }
            '{' => {
                out.push(Tok::LBrace);
                i += 1;
            }
            '}' => {
                out.push(Tok::RBrace);
                i += 1;
            }
            '$' => {
                // Hex literal `$XXXX`.
                let start = i + 1;
                let mut j = start;
                while j < chars.len() && chars[j].is_ascii_hexdigit() {
                    j += 1;
                }
                if j == start {
                    return Err(ParseError("'$' with no hex digits".into()));
                }
                let s: String = chars[start..j].iter().collect();
                let v = i64::from_str_radix(&s, 16)
                    .map_err(|_| ParseError(format!("bad hex literal ${s}")))?;
                out.push(Tok::Num(v));
                i = j;
            }
            '0'..='9' => {
                // Decimal, or `0x`-prefixed hex.
                if c == '0' && i + 1 < chars.len() && (chars[i + 1] == 'x' || chars[i + 1] == 'X') {
                    let start = i + 2;
                    let mut j = start;
                    while j < chars.len() && chars[j].is_ascii_hexdigit() {
                        j += 1;
                    }
                    if j == start {
                        return Err(ParseError("'0x' with no hex digits".into()));
                    }
                    let s: String = chars[start..j].iter().collect();
                    let v = i64::from_str_radix(&s, 16)
                        .map_err(|_| ParseError(format!("bad hex literal 0x{s}")))?;
                    out.push(Tok::Num(v));
                    i = j;
                } else {
                    let start = i;
                    let mut j = start;
                    while j < chars.len() && chars[j].is_ascii_digit() {
                        j += 1;
                    }
                    let s: String = chars[start..j].iter().collect();
                    let v = s
                        .parse::<i64>()
                        .map_err(|_| ParseError(format!("bad number {s}")))?;
                    out.push(Tok::Num(v));
                    i = j;
                }
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                let mut j = start;
                while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                let s: String = chars[start..j].iter().collect();
                out.push(Tok::Ident(s));
                i = j;
            }
            other => return Err(ParseError(format!("unexpected character {other:?}"))),
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Num(i64),
    Reg(RegId),
    /// `[expr]` — one-byte peek.
    PeekByte(Box<Self>),
    /// `{expr}` — two-byte little-endian word peek.
    PeekWord(Box<Self>),
    Unary(UnOp, Box<Self>),
    Binary(BinOp, Box<Self>, Box<Self>),
    Ternary(Box<Self>, Box<Self>, Box<Self>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RegId {
    A,
    X,
    Y,
    S,
    P,
    Pc,
    Scanline,
    Dot,
    Frame,
    Value,
    Address,
    IsRead,
    IsWrite,
    IsExec,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UnOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    LogAnd,
    LogOr,
}

fn ident_to_reg(s: &str) -> Result<RegId, ParseError> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "a" => RegId::A,
        "x" => RegId::X,
        "y" => RegId::Y,
        "s" | "sp" => RegId::S,
        "p" => RegId::P,
        "pc" => RegId::Pc,
        "scanline" => RegId::Scanline,
        "cycle" | "dot" => RegId::Dot,
        "frame" => RegId::Frame,
        "value" => RegId::Value,
        "address" | "addr" => RegId::Address,
        "isread" => RegId::IsRead,
        "iswrite" => RegId::IsWrite,
        "isexec" => RegId::IsExec,
        other => return Err(ParseError(format!("unknown identifier '{other}'"))),
    })
}

// ---------------------------------------------------------------------------
// Parser (recursive descent)
// ---------------------------------------------------------------------------

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn parse(&mut self) -> Result<Node, ParseError> {
        let n = self.ternary()?;
        if self.pos != self.toks.len() {
            return Err(ParseError(format!(
                "unexpected trailing tokens at position {}",
                self.pos
            )));
        }
        Ok(n)
    }

    fn ternary(&mut self) -> Result<Node, ParseError> {
        let cond = self.logic_or()?;
        if self.eat(&Tok::Question) {
            let then = self.ternary()?;
            if !self.eat(&Tok::Colon) {
                return Err(ParseError("expected ':' in ternary".into()));
            }
            let els = self.ternary()?;
            Ok(Node::Ternary(Box::new(cond), Box::new(then), Box::new(els)))
        } else {
            Ok(cond)
        }
    }

    fn logic_or(&mut self) -> Result<Node, ParseError> {
        let mut lhs = self.logic_and()?;
        while self.eat(&Tok::PipePipe) {
            let rhs = self.logic_and()?;
            lhs = Node::Binary(BinOp::LogOr, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn logic_and(&mut self) -> Result<Node, ParseError> {
        let mut lhs = self.bit_or()?;
        while self.eat(&Tok::AmpAmp) {
            let rhs = self.bit_or()?;
            lhs = Node::Binary(BinOp::LogAnd, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn bit_or(&mut self) -> Result<Node, ParseError> {
        let mut lhs = self.bit_xor()?;
        while self.eat(&Tok::Pipe) {
            let rhs = self.bit_xor()?;
            lhs = Node::Binary(BinOp::Or, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn bit_xor(&mut self) -> Result<Node, ParseError> {
        let mut lhs = self.bit_and()?;
        while self.eat(&Tok::Caret) {
            let rhs = self.bit_and()?;
            lhs = Node::Binary(BinOp::Xor, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn bit_and(&mut self) -> Result<Node, ParseError> {
        let mut lhs = self.equality()?;
        while self.eat(&Tok::Amp) {
            let rhs = self.equality()?;
            lhs = Node::Binary(BinOp::And, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn equality(&mut self) -> Result<Node, ParseError> {
        let mut lhs = self.relation()?;
        loop {
            let op = if self.eat(&Tok::EqEq) {
                BinOp::Eq
            } else if self.eat(&Tok::BangEq) {
                BinOp::Ne
            } else {
                break;
            };
            let rhs = self.relation()?;
            lhs = Node::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn relation(&mut self) -> Result<Node, ParseError> {
        let mut lhs = self.shift()?;
        loop {
            let op = if self.eat(&Tok::Le) {
                BinOp::Le
            } else if self.eat(&Tok::Ge) {
                BinOp::Ge
            } else if self.eat(&Tok::Lt) {
                BinOp::Lt
            } else if self.eat(&Tok::Gt) {
                BinOp::Gt
            } else {
                break;
            };
            let rhs = self.shift()?;
            lhs = Node::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn shift(&mut self) -> Result<Node, ParseError> {
        let mut lhs = self.add()?;
        loop {
            let op = if self.eat(&Tok::Shl) {
                BinOp::Shl
            } else if self.eat(&Tok::Shr) {
                BinOp::Shr
            } else {
                break;
            };
            let rhs = self.add()?;
            lhs = Node::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn add(&mut self) -> Result<Node, ParseError> {
        let mut lhs = self.mul()?;
        loop {
            let op = if self.eat(&Tok::Plus) {
                BinOp::Add
            } else if self.eat(&Tok::Minus) {
                BinOp::Sub
            } else {
                break;
            };
            let rhs = self.mul()?;
            lhs = Node::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn mul(&mut self) -> Result<Node, ParseError> {
        let mut lhs = self.unary()?;
        loop {
            let op = if self.eat(&Tok::Star) {
                BinOp::Mul
            } else if self.eat(&Tok::Slash) {
                BinOp::Div
            } else if self.eat(&Tok::Percent) {
                BinOp::Mod
            } else {
                break;
            };
            let rhs = self.unary()?;
            lhs = Node::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Node, ParseError> {
        if self.eat(&Tok::Minus) {
            Ok(Node::Unary(UnOp::Neg, Box::new(self.unary()?)))
        } else if self.eat(&Tok::Bang) {
            Ok(Node::Unary(UnOp::Not, Box::new(self.unary()?)))
        } else if self.eat(&Tok::Tilde) {
            Ok(Node::Unary(UnOp::BitNot, Box::new(self.unary()?)))
        } else {
            self.primary()
        }
    }

    fn primary(&mut self) -> Result<Node, ParseError> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Node::Num(n)),
            Some(Tok::Ident(s)) => Ok(Node::Reg(ident_to_reg(&s)?)),
            Some(Tok::LParen) => {
                let n = self.ternary()?;
                if !self.eat(&Tok::RParen) {
                    return Err(ParseError("expected ')'".into()));
                }
                Ok(n)
            }
            Some(Tok::LBracket) => {
                let n = self.ternary()?;
                if !self.eat(&Tok::RBracket) {
                    return Err(ParseError("expected ']'".into()));
                }
                Ok(Node::PeekByte(Box::new(n)))
            }
            Some(Tok::LBrace) => {
                let n = self.ternary()?;
                if !self.eat(&Tok::RBrace) {
                    return Err(ParseError("expected '}'".into()));
                }
                Ok(Node::PeekWord(Box::new(n)))
            }
            Some(t) => Err(ParseError(format!("unexpected token {t:?}"))),
            None => Err(ParseError("unexpected end of expression".into())),
        }
    }
}

// ---------------------------------------------------------------------------
// Public compiled expression
// ---------------------------------------------------------------------------

/// A parsed, reusable expression. Compile once (`Expr::parse`), evaluate every
/// frame against a fresh [`EvalContext`].
#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    root: Node,
}

impl Expr {
    /// Parse `src` into a compiled expression, or return a [`ParseError`].
    ///
    /// # Errors
    /// Returns a [`ParseError`] if `src` is empty, has a lexing error, or fails
    /// to parse as a complete expression.
    pub fn parse(src: &str) -> Result<Self, ParseError> {
        let toks = tokenize(src)?;
        if toks.is_empty() {
            return Err(ParseError("empty expression".into()));
        }
        let mut p = Parser { toks, pos: 0 };
        let root = p.parse()?;
        Ok(Self { root })
    }

    /// Evaluate against `ctx`, returning the `i64` result. Comparisons / logical
    /// operators yield `1` (true) or `0` (false).
    #[must_use]
    pub fn eval(&self, ctx: &dyn EvalContext) -> i64 {
        eval_node(&self.root, ctx)
    }

    /// Convenience: evaluate as a boolean (`eval(..) != 0`).
    #[must_use]
    pub fn eval_bool(&self, ctx: &dyn EvalContext) -> bool {
        self.eval(ctx) != 0
    }
}

fn eval_node(node: &Node, ctx: &dyn EvalContext) -> i64 {
    match node {
        Node::Num(n) => *n,
        Node::Reg(r) => eval_reg(*r, ctx),
        Node::PeekByte(inner) => {
            let addr = eval_node(inner, ctx) as u16;
            i64::from(ctx.peek(addr))
        }
        Node::PeekWord(inner) => {
            let addr = eval_node(inner, ctx) as u16;
            let lo = i64::from(ctx.peek(addr));
            let hi = i64::from(ctx.peek(addr.wrapping_add(1)));
            lo | (hi << 8)
        }
        Node::Unary(op, inner) => {
            let v = eval_node(inner, ctx);
            match op {
                UnOp::Neg => v.wrapping_neg(),
                UnOp::Not => i64::from(v == 0),
                UnOp::BitNot => !v,
            }
        }
        Node::Ternary(c, t, e) => {
            if eval_node(c, ctx) != 0 {
                eval_node(t, ctx)
            } else {
                eval_node(e, ctx)
            }
        }
        Node::Binary(op, l, r) => eval_binary(*op, l, r, ctx),
    }
}

fn eval_binary(op: BinOp, l: &Node, r: &Node, ctx: &dyn EvalContext) -> i64 {
    // Short-circuit the logical operators.
    match op {
        BinOp::LogAnd => {
            return i64::from(eval_node(l, ctx) != 0 && eval_node(r, ctx) != 0);
        }
        BinOp::LogOr => {
            return i64::from(eval_node(l, ctx) != 0 || eval_node(r, ctx) != 0);
        }
        _ => {}
    }
    let a = eval_node(l, ctx);
    let b = eval_node(r, ctx);
    match op {
        BinOp::Add => a.wrapping_add(b),
        BinOp::Sub => a.wrapping_sub(b),
        BinOp::Mul => a.wrapping_mul(b),
        // Division / modulo by zero yields 0 (a friendlier debugger default than
        // a panic; matches Mesen's tolerant evaluator).
        BinOp::Div => {
            if b == 0 {
                0
            } else {
                a.wrapping_div(b)
            }
        }
        BinOp::Mod => {
            if b == 0 {
                0
            } else {
                a.wrapping_rem(b)
            }
        }
        BinOp::And => a & b,
        BinOp::Or => a | b,
        BinOp::Xor => a ^ b,
        // Mask the shift amount to a sane range so a huge RHS can't panic.
        BinOp::Shl => a.wrapping_shl((b & 63) as u32),
        BinOp::Shr => a.wrapping_shr((b & 63) as u32),
        BinOp::Eq => i64::from(a == b),
        BinOp::Ne => i64::from(a != b),
        BinOp::Lt => i64::from(a < b),
        BinOp::Gt => i64::from(a > b),
        BinOp::Le => i64::from(a <= b),
        BinOp::Ge => i64::from(a >= b),
        BinOp::LogAnd | BinOp::LogOr => unreachable!("handled above"),
    }
}

fn eval_reg(r: RegId, ctx: &dyn EvalContext) -> i64 {
    let acc = ctx.access();
    match r {
        RegId::A => i64::from(ctx.a()),
        RegId::X => i64::from(ctx.x()),
        RegId::Y => i64::from(ctx.y()),
        RegId::S => i64::from(ctx.s()),
        RegId::P => i64::from(ctx.p()),
        RegId::Pc => i64::from(ctx.pc()),
        RegId::Scanline => i64::from(ctx.scanline()),
        RegId::Dot => i64::from(ctx.dot()),
        RegId::Frame => ctx.frame() as i64,
        RegId::Value => i64::from(acc.value),
        RegId::Address => i64::from(acc.address),
        RegId::IsRead => i64::from(acc.kind == Some(AccessKind::Read)),
        RegId::IsWrite => i64::from(acc.kind == Some(AccessKind::Write)),
        RegId::IsExec => i64::from(acc.kind == Some(AccessKind::Exec)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test context with overridable regs + a 64 KiB flat memory.
    struct Ctx {
        a: u8,
        x: u8,
        y: u8,
        s: u8,
        p: u8,
        pc: u16,
        scanline: i16,
        dot: u16,
        frame: u64,
        mem: Vec<u8>,
        access: AccessContext,
    }

    impl Default for Ctx {
        fn default() -> Self {
            Self {
                a: 0,
                x: 0,
                y: 0,
                s: 0xFD,
                p: 0x24,
                pc: 0x8000,
                scanline: 0,
                dot: 0,
                frame: 0,
                mem: vec![0; 0x1_0000],
                access: AccessContext::default(),
            }
        }
    }

    impl EvalContext for Ctx {
        fn a(&self) -> u8 {
            self.a
        }
        fn x(&self) -> u8 {
            self.x
        }
        fn y(&self) -> u8 {
            self.y
        }
        fn s(&self) -> u8 {
            self.s
        }
        fn p(&self) -> u8 {
            self.p
        }
        fn pc(&self) -> u16 {
            self.pc
        }
        fn scanline(&self) -> i16 {
            self.scanline
        }
        fn dot(&self) -> u16 {
            self.dot
        }
        fn frame(&self) -> u64 {
            self.frame
        }
        fn peek(&self, addr: u16) -> u8 {
            self.mem[addr as usize]
        }
        fn access(&self) -> AccessContext {
            self.access
        }
    }

    fn ev(src: &str, ctx: &Ctx) -> i64 {
        Expr::parse(src).expect("parse").eval(ctx)
    }

    #[test]
    fn number_literals_decimal_hex_binary() {
        let c = Ctx::default();
        assert_eq!(ev("42", &c), 42);
        assert_eq!(ev("$10", &c), 16);
        assert_eq!(ev("0x1F", &c), 31);
        assert_eq!(ev("%1010", &c), 10);
        assert_eq!(ev("$FFFF", &c), 0xFFFF);
    }

    #[test]
    fn arithmetic_and_precedence() {
        let c = Ctx::default();
        assert_eq!(ev("2 + 3 * 4", &c), 14);
        assert_eq!(ev("(2 + 3) * 4", &c), 20);
        assert_eq!(ev("10 - 2 - 3", &c), 5); // left assoc
        assert_eq!(ev("20 / 4 / 5", &c), 1);
        assert_eq!(ev("17 % 5", &c), 2);
        assert_eq!(ev("-5 + 8", &c), 3);
    }

    #[test]
    fn division_by_zero_is_zero() {
        let c = Ctx::default();
        assert_eq!(ev("5 / 0", &c), 0);
        assert_eq!(ev("5 % 0", &c), 0);
    }

    #[test]
    fn bitwise_operators() {
        let c = Ctx::default();
        assert_eq!(ev("$F0 & $0F", &c), 0);
        assert_eq!(ev("$F0 | $0F", &c), 0xFF);
        assert_eq!(ev("$FF ^ $0F", &c), 0xF0);
        assert_eq!(ev("~0 & $FF", &c), 0xFF);
        assert_eq!(ev("1 << 4", &c), 16);
        assert_eq!(ev("256 >> 2", &c), 64);
    }

    #[test]
    fn comparison_and_logical_operators() {
        let c = Ctx::default();
        assert_eq!(ev("3 == 3", &c), 1);
        assert_eq!(ev("3 != 3", &c), 0);
        assert_eq!(ev("2 < 3", &c), 1);
        assert_eq!(ev("3 <= 3", &c), 1);
        assert_eq!(ev("4 > 5", &c), 0);
        assert_eq!(ev("5 >= 5", &c), 1);
        assert_eq!(ev("1 && 0", &c), 0);
        assert_eq!(ev("1 || 0", &c), 1);
        assert_eq!(ev("!0", &c), 1);
        assert_eq!(ev("!5", &c), 0);
    }

    #[test]
    fn ternary() {
        let c = Ctx::default();
        assert_eq!(ev("1 ? 10 : 20", &c), 10);
        assert_eq!(ev("0 ? 10 : 20", &c), 20);
        assert_eq!(ev("(2 > 1) ? (3 + 4) : 0", &c), 7);
    }

    #[test]
    fn cpu_registers() {
        let mut c = Ctx::default();
        c.a = 0x42;
        c.x = 0x10;
        c.y = 0x20;
        c.s = 0xFE;
        c.p = 0x30;
        c.pc = 0xC123;
        assert_eq!(ev("a", &c), 0x42);
        assert_eq!(ev("x + y", &c), 0x30);
        assert_eq!(ev("s", &c), 0xFE);
        assert_eq!(ev("p", &c), 0x30);
        assert_eq!(ev("pc", &c), 0xC123);
        assert_eq!(ev("PC == $C123", &c), 1); // case-insensitive
    }

    #[test]
    fn ppu_registers() {
        let mut c = Ctx::default();
        c.scanline = 30;
        c.dot = 256;
        c.frame = 1234;
        assert_eq!(ev("scanline", &c), 30);
        assert_eq!(ev("cycle", &c), 256);
        assert_eq!(ev("dot", &c), 256);
        assert_eq!(ev("frame", &c), 1234);
        assert_eq!(ev("scanline == 30 && cycle >= 256", &c), 1);
    }

    #[test]
    fn negative_scanline_prerender() {
        let mut c = Ctx::default();
        c.scanline = -1;
        assert_eq!(ev("scanline", &c), -1);
        assert_eq!(ev("scanline < 0", &c), 1);
    }

    #[test]
    fn peek_byte_and_word() {
        let mut c = Ctx::default();
        c.mem[0x0010] = 0x34;
        c.mem[0x0011] = 0x12;
        assert_eq!(ev("[$10]", &c), 0x34);
        assert_eq!(ev("{$10}", &c), 0x1234); // little-endian word
        assert_eq!(ev("[$10] + [$11]", &c), 0x46);
    }

    #[test]
    fn peek_with_computed_address() {
        let mut c = Ctx::default();
        c.x = 0x05;
        c.mem[0x0205] = 0x99;
        assert_eq!(ev("[$200 + x]", &c), 0x99);
    }

    #[test]
    fn peek_word_wraps_at_top_of_memory() {
        let mut c = Ctx::default();
        c.mem[0xFFFF] = 0xAB;
        c.mem[0x0000] = 0xCD;
        // High byte reads from $0000 (wrap), not out of bounds.
        assert_eq!(ev("{$FFFF}", &c), 0xCD_AB);
    }

    #[test]
    fn access_context_tokens() {
        let mut c = Ctx::default();
        c.access = AccessContext {
            value: 0x7F,
            address: 0x0300,
            kind: Some(AccessKind::Write),
        };
        assert_eq!(ev("value", &c), 0x7F);
        assert_eq!(ev("address", &c), 0x0300);
        assert_eq!(ev("isWrite", &c), 1);
        assert_eq!(ev("isRead", &c), 0);
        assert_eq!(ev("isExec", &c), 0);
        assert_eq!(ev("isWrite && value == $7F", &c), 1);
    }

    #[test]
    fn access_context_defaults_to_zero() {
        let c = Ctx::default();
        assert_eq!(ev("value", &c), 0);
        assert_eq!(ev("address", &c), 0);
        assert_eq!(ev("isRead", &c), 0);
        assert_eq!(ev("isWrite", &c), 0);
        assert_eq!(ev("isExec", &c), 0);
    }

    #[test]
    fn realistic_watchpoint_expression() {
        // "writing a non-zero value to $0300 while on a visible scanline"
        let mut c = Ctx::default();
        c.scanline = 100;
        c.access = AccessContext {
            value: 0x01,
            address: 0x0300,
            kind: Some(AccessKind::Write),
        };
        assert_eq!(
            ev(
                "isWrite && address == $300 && value != 0 && scanline >= 0",
                &c
            ),
            1
        );
    }

    #[test]
    fn eval_bool_helper() {
        let c = Ctx::default();
        assert!(Expr::parse("1 + 1").unwrap().eval_bool(&c));
        assert!(!Expr::parse("0").unwrap().eval_bool(&c));
    }

    #[test]
    fn error_cases() {
        assert!(Expr::parse("").is_err()); // empty
        assert!(Expr::parse("   ").is_err()); // whitespace only
        assert!(Expr::parse("1 +").is_err()); // dangling operator
        assert!(Expr::parse("(1 + 2").is_err()); // unbalanced paren
        assert!(Expr::parse("[1 + 2").is_err()); // unbalanced bracket
        assert!(Expr::parse("{1").is_err()); // unbalanced brace
        assert!(Expr::parse("1 2").is_err()); // trailing token
        assert!(Expr::parse("bogus").is_err()); // unknown identifier
        assert!(Expr::parse("1 = 2").is_err()); // assignment not allowed
        assert!(Expr::parse("$").is_err()); // bare hex sigil
        assert!(Expr::parse("@").is_err()); // bad character
        assert!(Expr::parse("1 ? 2").is_err()); // ternary missing ':'
    }

    #[test]
    fn whitespace_insensitive() {
        let c = Ctx::default();
        assert_eq!(ev("  2+3 ", &c), 5);
        assert_eq!(ev("2  +  3", &c), 5);
    }
}

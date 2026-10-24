// SPDX-License-Identifier: GPL-3.0-or-later
//! A GDB Remote Serial Protocol stub for the 6502.
//!
//! Written from the GDB manual's *Remote Protocol* appendix: packet framing
//! (`$data#checksum`, `+` / `-` acknowledgements, `QStartNoAckMode`), the
//! `0x03` interrupt byte, stop replies (`T` with `thread:` and `swbreak:`),
//! and the *Target Descriptions* appendix for `qXfer:features:read`.
//!
//! The stub serves exactly one client on a **loopback** TCP socket; a
//! non-loopback address is refused at startup (ADR 0049). It is **read-only**
//! by design: `G`, `P`, `M` and `X` answer `E01`, so a client can stop, step,
//! break and read but never change the machine.
//!
//! # Registers
//!
//! The target description lists the 6502's registers in llvm-mos's DWARF
//! order, so the register number a client uses *is* the DWARF number for the
//! physical registers: `a`=0, `x`=1, `y`=2, `p`=3, `s`=4, `pc`=5 (16-bit,
//! little-endian in `g` / `p` replies, as the protocol sends target-order
//! bytes). With an llvm-mos ELF the imaginary registers follow: `rc0..rcN`
//! (8-bit, from the ELF's `__rcN` symbols) and then `rs0..rs(N/2)` (16-bit
//! pairs), each carrying a `dwarf_regnum` attribute with its number from
//! llvm-mos's *DWARF Specification for MOS-Compatible Processors*.
//! `dwarf_regnum` and `generic` are LLDB's target-description extensions; GDB
//! ignores attributes it does not know.
//!
//! **No released GDB or LLDB has a MOS 6502 architecture**, so today this
//! stub's client is a script, a test, or a debugger front end that speaks the
//! protocol and does its own symbolication (the DAP server in this crate does
//! the latter in-process). The description omits `<architecture>` for that
//! reason. Its tests drive it with a protocol client written in Rust.

use std::io::{self, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::engine::{Engine, Stop};

/// The largest packet body accepted from a client.
pub const MAX_PACKET: usize = 4096;
/// The largest `m` read, in bytes (two hex digits each fit `MAX_PACKET`).
pub const MAX_READ: usize = MAX_PACKET / 2 - 16;

/// An imaginary register exposed through the target description.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Imaginary {
    name_index: u32,
    zp: u8,
    size: u8,
    dwarf: u32,
}

/// The stub's register file.
#[derive(Clone, Debug, Default)]
pub struct Registers {
    imaginary: Vec<Imaginary>,
}

impl Registers {
    /// The physical registers only.
    #[must_use]
    pub const fn physical() -> Self {
        Self {
            imaginary: Vec::new(),
        }
    }

    /// The physical registers plus llvm-mos's imaginary ones, from the
    /// ELF's `__rcN` symbols (`(n, zero-page address)`). `RSn` is exposed
    /// when both `RC(2n)` and `RC(2n+1)` exist and are adjacent.
    #[must_use]
    pub fn with_imaginary(rc: &[(u32, u8)]) -> Self {
        let mut imaginary: Vec<Imaginary> = rc
            .iter()
            .map(|&(n, zp)| Imaginary {
                name_index: n,
                zp,
                size: 1,
                dwarf: 0x0002_0000 + n,
            })
            .collect();
        for &(n, zp) in rc {
            if n % 2 == 0
                && rc
                    .iter()
                    .any(|&(m, zp2)| m == n + 1 && u16::from(zp2) == u16::from(zp) + 1)
            {
                imaginary.push(Imaginary {
                    name_index: n / 2,
                    zp,
                    size: 2,
                    dwarf: 0x0003_0000 + n / 2,
                });
            }
        }
        Self { imaginary }
    }

    /// The target description XML.
    #[must_use]
    pub fn target_xml(&self) -> String {
        use std::fmt::Write as _;
        let mut x = String::from(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE target SYSTEM \"gdb-target.dtd\">\n<target version=\"1.0\">\n<feature name=\"org.rustynes.mos6502.core\">\n",
        );
        for (i, (name, bits, ty)) in [
            ("a", 8, "uint8"),
            ("x", 8, "uint8"),
            ("y", 8, "uint8"),
            ("p", 8, "uint8"),
            ("s", 8, "uint8"),
            ("pc", 16, "code_ptr"),
        ]
        .iter()
        .enumerate()
        {
            let generic = match *name {
                "pc" => " generic=\"pc\"",
                "s" => " generic=\"sp\"",
                "p" => " generic=\"flags\"",
                _ => "",
            };
            let _ = writeln!(
                x,
                "<reg name=\"{name}\" bitsize=\"{bits}\" regnum=\"{i}\" type=\"{ty}\" group=\"general\" dwarf_regnum=\"{i}\"{generic}/>"
            );
        }
        x.push_str("</feature>\n");
        if !self.imaginary.is_empty() {
            x.push_str("<feature name=\"org.rustynes.mos6502.llvm-mos-imaginary\">\n");
            for (i, r) in self.imaginary.iter().enumerate() {
                let (prefix, ty) = if r.size == 1 {
                    ("rc", "uint8")
                } else {
                    ("rs", "uint16")
                };
                let _ = writeln!(
                    x,
                    "<reg name=\"{prefix}{}\" bitsize=\"{}\" regnum=\"{}\" type=\"{ty}\" group=\"imaginary\" dwarf_regnum=\"{}\"/>",
                    r.name_index,
                    u32::from(r.size) * 8,
                    i + 6,
                    r.dwarf
                );
            }
            x.push_str("</feature>\n");
        }
        x.push_str("</target>\n");
        x
    }

    /// The bytes of register `n` (target order).
    // `a` / `x` / `y` / `p` / `s` are the 6502's register names.
    #[allow(clippy::many_single_char_names)]
    fn read(&self, engine: &mut Engine, n: usize) -> Option<Vec<u8>> {
        let cpu = engine.nes().cpu();
        let (a, x, y, p, s, pc) = (cpu.a, cpu.x, cpu.y, cpu.p.bits(), cpu.s, cpu.pc);
        match n {
            0 => Some(vec![a]),
            1 => Some(vec![x]),
            2 => Some(vec![y]),
            3 => Some(vec![p]),
            4 => Some(vec![s]),
            5 => Some(pc.to_le_bytes().to_vec()),
            _ => {
                let r = self.imaginary.get(n - 6)?;
                Some(engine.read(u16::from(r.zp), usize::from(r.size)))
            }
        }
    }

    const fn count(&self) -> usize {
        6 + self.imaginary.len()
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn checksum(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |a, &b| a.wrapping_add(b))
}

enum Incoming {
    Packet(Vec<u8>),
    Interrupt,
    Eof,
}

/// Bind `addr` (loopback only) and serve one client.
///
/// # Errors
///
/// A non-loopback address, or an I/O error.
pub fn listen(addr: SocketAddr, engine: Engine, regs: &Registers) -> io::Result<()> {
    if !addr.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{addr} is not a loopback address; the GDB stub listens on loopback only"),
        ));
    }
    let listener = TcpListener::bind(addr)?;
    eprintln!(
        "rustynes-dap: GDB stub listening on {}",
        listener.local_addr()?
    );
    let (stream, peer) = listener.accept()?;
    eprintln!("rustynes-dap: GDB client {peer}");
    serve(stream, engine, regs)
}

/// Serve one client on an accepted stream.
///
/// # Errors
///
/// An I/O error.
pub fn serve(stream: TcpStream, mut engine: Engine, regs: &Registers) -> io::Result<()> {
    stream.set_nodelay(true)?;
    let reader = stream.try_clone()?;
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("gdb-reader".into())
        .spawn(move || read_packets(reader, &tx))?;
    let mut s = Stub {
        out: stream,
        ack: true,
        regs,
    };
    s.run(&mut engine, &rx)
}

/// Split the byte stream into packets, acknowledging each when asked to.
/// Acknowledgement bytes from the client are dropped (this stub does not
/// retransmit: it speaks over TCP, which already does).
fn read_packets(stream: TcpStream, tx: &mpsc::Sender<Incoming>) {
    let mut r = BufReader::new(stream);
    let mut byte = [0u8; 1];
    let mut ack_writer = r.get_ref().try_clone().ok();
    loop {
        if r.read_exact(&mut byte).is_err() {
            let _ = tx.send(Incoming::Eof);
            return;
        }
        match byte[0] {
            0x03 => {
                if tx.send(Incoming::Interrupt).is_err() {
                    return;
                }
            }
            b'$' => {
                let mut body = Vec::new();
                loop {
                    if r.read_exact(&mut byte).is_err() {
                        let _ = tx.send(Incoming::Eof);
                        return;
                    }
                    if byte[0] == b'#' {
                        break;
                    }
                    if body.len() >= MAX_PACKET {
                        let _ = tx.send(Incoming::Eof);
                        return;
                    }
                    body.push(byte[0]);
                }
                let mut cs = [0u8; 2];
                if r.read_exact(&mut cs).is_err() {
                    let _ = tx.send(Incoming::Eof);
                    return;
                }
                let good = std::str::from_utf8(&cs)
                    .ok()
                    .and_then(|c| u8::from_str_radix(c, 16).ok())
                    == Some(checksum(&body));
                if let Some(w) = ack_writer.as_mut() {
                    let _ = w.write_all(if good { b"+" } else { b"-" });
                }
                if good && tx.send(Incoming::Packet(body)).is_err() {
                    return;
                }
            }
            _ => {} // '+', '-', noise between packets
        }
    }
}

struct Stub<'r> {
    out: TcpStream,
    ack: bool,
    regs: &'r Registers,
}

impl Stub<'_> {
    fn send(&mut self, data: &str) -> io::Result<()> {
        let pkt = format!("${data}#{:02x}", checksum(data.as_bytes()));
        self.out.write_all(pkt.as_bytes())?;
        self.out.flush()
    }

    fn stop_reply(engine: &Engine, stop: Stop, interrupted: bool) -> String {
        let pc = engine.nes().cpu().pc.to_le_bytes();
        let signal = match stop {
            Stop::Jam => "04",
            _ if interrupted => "02",
            _ => "05",
        };
        let swbreak = if stop == Stop::Breakpoint {
            "swbreak:;"
        } else {
            ""
        };
        format!("T{signal}05:{};thread:1;{swbreak}", hex(&pc))
    }

    // One arm per packet, as the protocol appendix lists them.
    #[allow(clippy::too_many_lines)]
    fn run(&mut self, engine: &mut Engine, rx: &Receiver<Incoming>) -> io::Result<()> {
        let mut last_stop = Stop::Entry;
        loop {
            let Ok(msg) = rx.recv() else { return Ok(()) };
            let pkt = match msg {
                Incoming::Eof => return Ok(()),
                Incoming::Interrupt => {
                    // Already stopped: report where.
                    let r = Self::stop_reply(engine, Stop::Pause, true);
                    self.send(&r)?;
                    continue;
                }
                Incoming::Packet(p) => p,
            };
            let pkt = String::from_utf8_lossy(&pkt).into_owned();
            let reply = match pkt.as_str() {
                p if p.starts_with("qSupported") => {
                    "PacketSize=1000;qXfer:features:read+;swbreak+;QStartNoAckMode+;vContSupported+"
                        .to_owned()
                }
                "QStartNoAckMode" => {
                    self.send("OK")?;
                    self.ack = false;
                    continue;
                }
                "?" => Self::stop_reply(engine, last_stop, false),
                "qAttached" => "1".into(),
                "qC" => "QC1".into(),
                "qfThreadInfo" => "m1".into(),
                "qsThreadInfo" => "l".into(),
                "vCont?" => "vCont;c;s".into(),
                p if p.starts_with('H') => "OK".into(),
                p if p.starts_with("qXfer:features:read:target.xml:") => {
                    self.xfer(&p["qXfer:features:read:target.xml:".len()..])
                }
                "g" => {
                    let mut all = Vec::new();
                    for n in 0..self.regs.count() {
                        all.extend(self.regs.read(engine, n).unwrap_or_default());
                    }
                    hex(&all)
                }
                p if p.starts_with('p') => usize::from_str_radix(&p[1..], 16)
                    .ok()
                    .and_then(|n| self.regs.read(engine, n))
                    .map_or_else(|| "E00".into(), |b| hex(&b)),
                p if p.starts_with('m') => Self::read_mem(engine, &p[1..]),
                p if p.starts_with('G')
                    || p.starts_with('P')
                    || p.starts_with('M')
                    || p.starts_with('X') =>
                {
                    // Read-only by design (ADR 0049).
                    "E01".into()
                }
                p if p.starts_with("Z0,") || p.starts_with("Z1,") => parse_bp(&p[3..]).map_or_else(
                    || "E00".into(),
                    |a| {
                        engine
                            .add_address_breakpoint(a)
                            .map_or_else(|_| "E02".into(), |()| "OK".into())
                    },
                ),
                p if p.starts_with("z0,") || p.starts_with("z1,") => parse_bp(&p[3..]).map_or_else(
                    || "E00".into(),
                    |a| {
                        engine.remove_address_breakpoint(a);
                        "OK".into()
                    },
                ),
                "c" | "vCont;c" | "vCont;c:1" => {
                    let mut interrupted = false;
                    let mut eof = false;
                    let stop = engine.resume(&mut || match rx.try_recv() {
                        Ok(Incoming::Interrupt) => {
                            interrupted = true;
                            true
                        }
                        Ok(Incoming::Eof) | Err(TryRecvError::Disconnected) => {
                            eof = true;
                            true
                        }
                        // A packet while running is not legal in all-stop
                        // mode; it is dropped.
                        Ok(Incoming::Packet(_)) | Err(TryRecvError::Empty) => false,
                    });
                    if eof {
                        return Ok(());
                    }
                    last_stop = stop;
                    Self::stop_reply(engine, stop, interrupted)
                }
                "s" | "vCont;s" | "vCont;s:1" => {
                    let stop = engine.step_instruction();
                    last_stop = stop;
                    Self::stop_reply(engine, stop, false)
                }
                "k" => return Ok(()),
                "D" | "D;1" => {
                    self.send("OK")?;
                    return Ok(());
                }
                _ => String::new(),
            };
            self.send(&reply)?;
        }
    }

    fn xfer(&self, args: &str) -> String {
        let xml = self.regs.target_xml();
        let Some((off, len)) = args.split_once(',') else {
            return "E00".into();
        };
        let (Ok(off), Ok(len)) = (
            usize::from_str_radix(off, 16),
            usize::from_str_radix(len, 16),
        ) else {
            return "E00".into();
        };
        let bytes = xml.as_bytes();
        if off >= bytes.len() {
            return "l".into();
        }
        let end = (off + len.min(MAX_READ)).min(bytes.len());
        let chunk = String::from_utf8_lossy(&bytes[off..end]);
        let marker = if end == bytes.len() { 'l' } else { 'm' };
        // `$`, `#`, `}` and `*` must be escaped in a binary reply; the XML
        // contains none of them.
        format!("{marker}{chunk}")
    }

    fn read_mem(engine: &mut Engine, args: &str) -> String {
        let Some((a, l)) = args.split_once(',') else {
            return "E00".into();
        };
        let (Ok(a), Ok(l)) = (u32::from_str_radix(a, 16), usize::from_str_radix(l, 16)) else {
            return "E00".into();
        };
        let Ok(a) = u16::try_from(a) else {
            return "E01".into();
        };
        let l = l.min(MAX_READ).min(0x1_0000 - usize::from(a));
        hex(&engine.read(a, l))
    }
}

fn parse_bp(args: &str) -> Option<u16> {
    let addr = args.split(',').next()?;
    u16::from_str_radix(addr, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_are_mod_256_sums() {
        assert_eq!(checksum(b"OK"), 0x9a);
        assert_eq!(checksum(b""), 0);
    }

    #[test]
    fn the_target_description_numbers_registers_as_dwarf_does() {
        let x = Registers::physical().target_xml();
        assert!(x.contains("name=\"a\" bitsize=\"8\" regnum=\"0\""));
        assert!(x.contains("name=\"pc\" bitsize=\"16\" regnum=\"5\" type=\"code_ptr\" group=\"general\" dwarf_regnum=\"5\" generic=\"pc\""));
        assert!(!x.contains("imaginary"));
        let r = Registers::with_imaginary(&[(0, 0x00), (1, 0x01), (2, 0x02)]);
        let x = r.target_xml();
        assert!(x.contains("name=\"rc2\" bitsize=\"8\" regnum=\"8\""));
        assert!(x.contains(&format!("name=\"rs0\" bitsize=\"16\" regnum=\"9\" type=\"uint16\" group=\"imaginary\" dwarf_regnum=\"{}\"", 0x30000)));
        assert!(!x.contains("rs1"), "rc3 is missing, so rs1 is not exposed");
        assert_eq!(r.count(), 10);
    }

    #[test]
    fn breakpoint_arguments_parse() {
        assert_eq!(parse_bp("806c,1"), Some(0x806C));
        assert_eq!(parse_bp("10000,1"), None);
        assert_eq!(parse_bp("zz"), None);
    }
}

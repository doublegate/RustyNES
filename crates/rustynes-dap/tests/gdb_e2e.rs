// SPDX-License-Identifier: GPL-3.0-or-later
//! The GDB Remote Serial Protocol stub, driven over TCP by a protocol client
//! written here (no released GDB or LLDB has a MOS 6502 target to drive it
//! with). The client does what a debugger with llvm-mos support would do
//! with the ELF: it looks up the statement address of a source line, sets a
//! software breakpoint there, continues, and evaluates a local's
//! `DW_OP_fbreg` location by reading the frame-base register `rs0` and then
//! memory.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use common::{CC65, Fixture, LLVM_MOS};
use rustynes_dap::debuginfo::{DebugInfo, FrameBase, VarLoc};

struct Gdb {
    child: Child,
    stream: TcpStream,
    ack: bool,
}

impl Gdb {
    fn start(fx: Fixture, with_debug: bool) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustynes-dap"));
        cmd.arg("--gdb")
            .arg("127.0.0.1:0")
            .arg("--rom")
            .arg(fx.rom_path());
        if with_debug {
            cmd.arg("--debug-info").arg(fx.debug_path());
        }
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn");
        let mut err = BufReader::new(child.stderr.take().expect("stderr"));
        let mut line = String::new();
        err.read_line(&mut line).expect("listening line");
        let addr = line.trim().rsplit(' ').next().expect("address").to_owned();
        assert!(line.contains("listening on 127.0.0.1:"), "{line}");
        // Keep draining stderr: a closed pipe would fail the stub's next
        // diagnostic line.
        std::thread::spawn(move || {
            let mut sink = String::new();
            while err.read_line(&mut sink).is_ok_and(|n| n > 0) {
                sink.clear();
            }
        });
        let stream = TcpStream::connect(&addr).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(60)))
            .expect("timeout");
        Self {
            child,
            stream,
            ack: true,
        }
    }

    fn send_raw(&mut self, data: &str) {
        let cs = data.bytes().fold(0u8, u8::wrapping_add);
        write!(self.stream, "${data}#{cs:02x}").expect("send");
        self.stream.flush().expect("flush");
    }

    fn recv(&mut self) -> String {
        let mut b = [0u8; 1];
        loop {
            self.stream.read_exact(&mut b).expect("reply");
            if b[0] == b'$' {
                break;
            }
            // '+' acknowledgements from the stub
        }
        let mut body = Vec::new();
        loop {
            self.stream.read_exact(&mut b).expect("reply body");
            if b[0] == b'#' {
                break;
            }
            body.push(b[0]);
        }
        let mut cs = [0u8; 2];
        self.stream.read_exact(&mut cs).expect("checksum");
        let want = body.iter().fold(0u8, |a, &x| a.wrapping_add(x));
        assert_eq!(
            u8::from_str_radix(std::str::from_utf8(&cs).expect("hex"), 16).expect("hex"),
            want,
            "reply checksum"
        );
        if self.ack {
            self.stream.write_all(b"+").expect("ack");
        }
        String::from_utf8(body).expect("utf-8")
    }

    fn call(&mut self, data: &str) -> String {
        self.send_raw(data);
        self.recv()
    }

    fn pc_of(reply: &str) -> u16 {
        // T05 05:<lo><hi>;thread:1;...
        let field = reply
            .split(';')
            .find_map(|f| {
                f.strip_prefix("T05")
                    .or_else(|| f.strip_prefix("T02"))
                    .map(str::to_owned)
            })
            .and_then(|f| f.strip_prefix("05:").map(str::to_owned))
            .expect("pc in the stop reply");
        let lo = u8::from_str_radix(&field[0..2], 16).expect("lo");
        let hi = u8::from_str_radix(&field[2..4], 16).expect("hi");
        u16::from_le_bytes([lo, hi])
    }
}

fn info(fx: Fixture) -> DebugInfo {
    let bytes = std::fs::read(fx.debug_path()).expect("debug file");
    DebugInfo::load(&bytes, &common::dir()).expect("loads")
}

fn line_addr(d: &DebugInfo, marker: &str) -> u16 {
    let file = d
        .file_for_path(&common::source().to_string_lossy())
        .expect("file");
    let (_, addrs) = d
        .resolve_line(file, common::line(marker))
        .expect("line has code");
    addrs[0]
}

#[test]
fn llvm_mos_breakpoint_step_and_frame_relative_read() {
    let d = info(LLVM_MOS);
    let add3 = d.function_named("add3").expect("add3");
    let FrameBase::ZpPointer(fb_zp) = add3.frame_base else {
        panic!("add3 has an RS frame base")
    };
    let Some(v_off) = add3
        .vars
        .iter()
        .find(|v| v.name == "v")
        .and_then(|v| match v.loc {
            VarLoc::Frame(o) => Some(o),
            _ => None,
        })
    else {
        panic!("v is fbreg-relative")
    };
    let bp = line_addr(&d, "add3-body");

    let mut g = Gdb::start(LLVM_MOS, true);
    let sup = g.call("qSupported:swbreak+;xmlRegisters=i386");
    assert!(
        sup.contains("qXfer:features:read+") && sup.contains("swbreak+"),
        "{sup}"
    );
    assert_eq!(g.call("QStartNoAckMode"), "OK");
    g.ack = false;
    let xml = g.call("qXfer:features:read:target.xml:0,3fb");
    assert!(xml.starts_with('m') || xml.starts_with('l'), "{xml}");
    let mut whole = xml[1..].to_owned();
    let mut done = xml.starts_with('l');
    while !done {
        let more = g.call(&format!(
            "qXfer:features:read:target.xml:{:x},3fb",
            whole.len()
        ));
        done = more.starts_with('l');
        whole.push_str(&more[1..]);
    }
    assert!(
        whole.contains("name=\"pc\" bitsize=\"16\" regnum=\"5\""),
        "{whole}"
    );
    // rs0's register number in the description.
    let rs0 = whole
        .lines()
        .find(|l| l.contains("name=\"rs0\""))
        .and_then(|l| l.split("regnum=\"").nth(1))
        .and_then(|r| r.split('"').next())
        .and_then(|n| n.parse::<usize>().ok())
        .expect("rs0 is described");
    assert!(
        whole.contains(&format!("dwarf_regnum=\"{}\"", 0x30000)),
        "rs0 carries its DWARF number"
    );

    assert_eq!(g.call("?").get(..3), Some("T05"));
    assert_eq!(g.call(&format!("Z0,{bp:x},1")), "OK");
    let stop = g.call("c");
    assert!(stop.contains("swbreak:"), "{stop}");
    assert_eq!(
        Gdb::pc_of(&stop),
        bp,
        "stopped at the source line's address"
    );

    // Evaluate `v` the way a DWARF consumer would: rs0 + fbreg offset.
    let rs = g.call(&format!("p{rs0:x}"));
    let fb = u16::from_le_bytes([
        u8::from_str_radix(&rs[0..2], 16).expect("lo"),
        u8::from_str_radix(&rs[2..4], 16).expect("hi"),
    ]);
    let zp = g.call(&format!("m{fb_zp:x},2"));
    assert_eq!(zp, rs, "rs0 is the zero-page pair the ELF names");
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let v_addr = fb.wrapping_add(v_off as u16);
    assert_eq!(
        g.call(&format!("m{v_addr:x},1")),
        "05",
        "v == 5 at the first call"
    );

    // One instruction step moves the PC; writes are refused.
    let step = g.call("s");
    assert_ne!(Gdb::pc_of(&step), bp);
    assert_eq!(g.call("M0,1:ff"), "E01");
    assert_eq!(g.call("P0=ff"), "E01");

    // Remove the breakpoint, run, interrupt.
    assert_eq!(g.call(&format!("z0,{bp:x},1")), "OK");
    g.send_raw("c");
    std::thread::sleep(Duration::from_millis(50));
    g.stream.write_all(&[0x03]).expect("interrupt");
    let stop = g.recv();
    assert!(
        stop.starts_with("T02"),
        "an interrupt stops with SIGINT: {stop}"
    );
    let regs = g.call("g");
    assert!(regs.len() >= 14, "a, x, y, p, s and a 16-bit pc: {regs}");
    g.send_raw("k");
    let status = g.child.wait().expect("exit");
    assert!(status.success());
}

#[test]
fn cc65_breakpoint_and_global_read() {
    let d = info(CC65);
    let bp = line_addr(&d, "main-store");
    let counter = match d.global_named("counter").expect("counter").loc {
        VarLoc::Absolute(a) => a,
        ref other => panic!("{other:?}"),
    };
    let mut g = Gdb::start(CC65, false);
    let xml = g.call("qXfer:features:read:target.xml:0,fff");
    assert!(
        xml.starts_with('l') && !xml.contains("rc0"),
        "no imaginary registers without an ELF"
    );
    assert_eq!(g.call(&format!("Z0,{bp:x},1")), "OK");
    assert_eq!(Gdb::pc_of(&g.call("c")), bp);
    assert_eq!(
        g.call(&format!("m{counter:x},1")),
        "00",
        "before the first store"
    );
    assert_eq!(Gdb::pc_of(&g.call("c")), bp);
    assert_eq!(
        g.call(&format!("m{counter:x},1")),
        "08",
        "after add3(5) was stored"
    );
    assert_eq!(g.call("D"), "OK");
    assert!(g.child.wait().expect("exit").success());
}

#[test]
fn a_non_loopback_address_is_refused() {
    let out = Command::new(env!("CARGO_BIN_EXE_rustynes-dap"))
        .args(["--gdb", "0.0.0.0:0", "--rom"])
        .arg(CC65.rom_path())
        .output()
        .expect("run");
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not a loopback address"), "{err}");
}

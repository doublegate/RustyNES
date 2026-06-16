//! Nestest golden-log harness.
//!
//! Runs the kevtris `nestest.nes` ROM in "automation" mode (PC forced to
//! `$C000`), capturing pre-execute CPU state for each instruction and
//! comparing the captured `(PC, A, X, Y, P, SP, PPU scanline, PPU dot, CYC)`
//! tuple against the matching field set parsed out of the bundled
//! `nestest.log` (Nintendulator-generated).
//!
//! Per `docs/testing-strategy.md` §Layer 2.

use rustynes_core::rustynes_cpu::{Bus, Cpu};
use rustynes_core::rustynes_mappers::{Mapper, parse};

/// Pre-execute snapshot captured before each instruction step.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct LogLine {
    /// Program counter at instruction fetch.
    pub pc: u16,
    /// Accumulator.
    pub a: u8,
    /// X register.
    pub x: u8,
    /// Y register.
    pub y: u8,
    /// Status register (with bits 4 and 5 forced per Nintendulator convention:
    /// bit 5 = 1 (always), bit 4 = 0 (B is observation-only).
    pub p: u8,
    /// Stack pointer.
    pub sp: u8,
    /// PPU scanline (`cycles * 3 / 341 % 262` in our model).
    pub ppu_scanline: u16,
    /// PPU dot within the scanline.
    pub ppu_dot: u16,
    /// Total CPU cycle count.
    pub cyc: u64,
}

/// Format a [`LogLine`] in the field-only Nintendulator style.
///
/// Output: `"<PC> A:?? X:?? Y:?? P:?? SP:?? PPU:nnn,nnn CYC:n"`.
///
/// Used by the harness's failure reporter; does NOT include the per-line
/// disassembly column because we do not (yet) reproduce Nintendulator's
/// exact disassembler.
#[must_use]
pub fn format_log_line(l: &LogLine) -> String {
    format!(
        "{:04X} A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X} PPU:{:>3},{:>3} CYC:{}",
        l.pc, l.a, l.x, l.y, l.p, l.sp, l.ppu_scanline, l.ppu_dot, l.cyc
    )
}

fn extract<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let idx = line.find(key)?;
    Some(&line[idx + key.len()..])
}

fn parse_byte(after: &str) -> Option<u8> {
    u8::from_str_radix(after.get(0..2)?, 16).ok()
}

/// Parse one line of `nestest.log` (Nintendulator format) into the same
/// [`LogLine`] structure.
#[must_use]
pub fn parse_log_line(line: &str) -> Option<LogLine> {
    // Lines look like:
    // "C000  4C F5 C5  JMP $C5F5                       A:00 X:00 Y:00 P:24 SP:FD PPU:  0, 21 CYC:7"
    let pc = u16::from_str_radix(line.get(0..4)?, 16).ok()?;

    let a = parse_byte(extract(line, "A:")?)?;
    let x = parse_byte(extract(line, "X:")?)?;
    let y = parse_byte(extract(line, "Y:")?)?;
    let p = parse_byte(extract(line, "P:")?)?;
    let sp = parse_byte(extract(line, "SP:")?)?;

    // PPU is 3-digit, comma, 3-digit, possibly with leading spaces inside the
    // 3-digit fields.
    let ppu_after = extract(line, "PPU:")?;
    let comma = ppu_after.find(',')?;
    let scanline = ppu_after.get(..comma)?.trim().parse::<u16>().ok()?;
    let after_comma = &ppu_after[comma + 1..];
    // Dot field ends before " CYC:".
    let cyc_idx = after_comma.find("CYC:")?;
    let dot = after_comma.get(..cyc_idx)?.trim().parse::<u16>().ok()?;
    let cyc = after_comma[cyc_idx + 4..].trim().parse::<u64>().ok()?;

    Some(LogLine {
        pc,
        a,
        x,
        y,
        p,
        sp,
        ppu_scanline: scanline,
        ppu_dot: dot,
        cyc,
    })
}

/// Bus implementation backing the nestest harness.
///
/// Holds 2 KiB CPU RAM mirrored at `$0000-$1FFF`, mapper-mediated
/// `$4020-$FFFF`, and stub responses for the PPU/APU register windows nestest
/// itself does not exercise.
pub struct NestestBus {
    /// CPU RAM ($0000-$07FF, mirrored to $1FFF).
    pub ram: Box<[u8; 0x0800]>,
    /// Mapper instance from the parsed cartridge.
    pub mapper: Box<dyn Mapper>,
    /// Per-cycle PPU dot accumulator (cycles * 3, NTSC).
    pub ppu_total: u64,
}

impl NestestBus {
    /// Construct from raw nestest ROM bytes.
    ///
    /// # Panics
    ///
    /// Panics if the bytes do not parse as a valid NROM cartridge — nestest
    /// is bundled with the harness and is canonical, so a parse failure here
    /// is a bug.
    #[must_use]
    pub fn new(rom_bytes: &[u8]) -> Self {
        let (_cart, mapper) = parse(rom_bytes).expect("nestest.nes must parse as NROM");
        Self {
            ram: Box::new([0u8; 0x0800]),
            mapper,
            ppu_total: 21, // Nintendulator's first log line shows PPU dot 21 (cycles=7 * 3).
        }
    }

    /// Current `(scanline, dot)` per `cycles * 3` model.
    #[must_use]
    pub const fn ppu_position(&self) -> (u16, u16) {
        let total = self.ppu_total % (341 * 262);
        let scanline = (total / 341) as u16;
        let dot = (total % 341) as u16;
        (scanline, dot)
    }
}

impl Bus for NestestBus {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            // PPU + APU/IO register windows: nestest's automation path
            // doesn't actually exercise these reads, so a 0 return is safe.
            0x2000..=0x401F => 0,
            0x4020..=0xFFFF => self.mapper.cpu_read(addr),
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize] = value,
            0x4020..=0xFFFF => self.mapper.cpu_write(addr, value),
            _ => {}
        }
    }

    fn on_cpu_cycle(&mut self) {
        self.ppu_total = self.ppu_total.wrapping_add(3);
    }
}

/// Driver: capture pre-execute log lines, run N instructions.
pub struct NestestRunner<'a> {
    bus: &'a mut NestestBus,
    cpu: &'a mut Cpu,
}

impl<'a> NestestRunner<'a> {
    /// New runner.
    pub const fn new(bus: &'a mut NestestBus, cpu: &'a mut Cpu) -> Self {
        Self { bus, cpu }
    }

    /// Capture state, run one instruction, return the captured log line.
    pub fn step(&mut self) -> LogLine {
        // Snapshot BEFORE stepping — this matches Nintendulator's column.
        let (scanline, dot) = self.bus.ppu_position();
        let p_pushed = (self.cpu.p.bits() & 0xEF) | 0x20; // B clear, U set
        let line = LogLine {
            pc: self.cpu.pc,
            a: self.cpu.a,
            x: self.cpu.x,
            y: self.cpu.y,
            p: p_pushed,
            sp: self.cpu.s,
            ppu_scanline: scanline,
            ppu_dot: dot,
            cyc: self.cpu.cycles,
        };
        self.cpu.step(self.bus);
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_log_line_handles_first_line() {
        let line = "C000  4C F5 C5  JMP $C5F5                       A:00 X:00 Y:00 P:24 SP:FD PPU:  0, 21 CYC:7";
        let parsed = parse_log_line(line).expect("must parse");
        assert_eq!(parsed.pc, 0xC000);
        assert_eq!(parsed.a, 0x00);
        assert_eq!(parsed.x, 0x00);
        assert_eq!(parsed.y, 0x00);
        assert_eq!(parsed.p, 0x24);
        assert_eq!(parsed.sp, 0xFD);
        assert_eq!(parsed.ppu_scanline, 0);
        assert_eq!(parsed.ppu_dot, 21);
        assert_eq!(parsed.cyc, 7);
    }

    #[test]
    fn format_round_trips_through_parse() {
        let l = LogLine {
            pc: 0xC5F5,
            a: 0,
            x: 0,
            y: 0,
            p: 0x24,
            sp: 0xFD,
            ppu_scanline: 0,
            ppu_dot: 30,
            cyc: 10,
        };
        let s = format_log_line(&l);
        // Build a "nintendulator-shaped" line by concatenating dummy mid-cols.
        let synthetic = format!(
            "C5F5  A2 00     LDX #$00                        {}",
            &s[5..]
        );
        let parsed = parse_log_line(&synthetic).unwrap();
        assert_eq!(parsed.pc, l.pc);
        assert_eq!(parsed.a, l.a);
        assert_eq!(parsed.cyc, l.cyc);
    }
}

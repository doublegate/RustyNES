//! blargg / kevtris ROM runner.
//!
//! These ROMs use a uniform protocol: write status to `$6000` (0x80 = running,
//! 0x81 = needs reset, 0x00..=0x7F = complete with code) plus an optional
//! result string at `$6004..` once the magic `'D' 'E' 'B' '\0'` is written
//! to `$6001..=$6003`.
//!
//! Per `docs/testing-strategy.md` §Layer 3.

use rustynes_core::rustynes_cpu::{Bus, Cpu};
use rustynes_core::rustynes_mappers::{Mapper, RomError, parse};

/// Outcome of running a blargg-style ROM to completion.
#[derive(Debug)]
pub struct BlarggResult {
    /// Final status byte at `$6000` (0 == pass).
    pub status: u8,
    /// Result string read out of `$6004..` (terminated by 0).
    pub message: String,
    /// CPU cycles spent.
    pub cycles: u64,
}

/// Blargg-style bus: 8 KiB WRAM at `$6000-$7FFF` plus the cart, with the
/// 2 KiB CPU RAM at `$0000-$1FFF` and stub PPU/APU windows.
pub struct BlarggBus {
    pub(crate) ram: Box<[u8; 0x0800]>,
    pub(crate) wram: Box<[u8; 0x2000]>,
    pub(crate) mapper: Box<dyn Mapper>,
    pub(crate) cycles: u64,
    /// `$4015` reads return 0 unless the cart's mapper supplies APU. We hold
    /// a stub here so reads/writes don't panic.
    apu_io: u8,
}

impl BlarggBus {
    /// Construct from the parsed cartridge bytes.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`RomError`] if the bytes don't parse.
    pub fn new(rom_bytes: &[u8]) -> Result<Self, RomError> {
        let (_cart, mapper) = parse(rom_bytes)?;
        Ok(Self {
            ram: Box::new([0u8; 0x0800]),
            wram: Box::new([0u8; 0x2000]),
            mapper,
            cycles: 0,
            apu_io: 0,
        })
    }

    /// Current `$6000` status byte.
    #[must_use]
    pub fn status(&self) -> u8 {
        self.wram[0]
    }
}

impl Bus for BlarggBus {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x4015 => self.apu_io,
            // PPU + APU/IO stub for everything except the $4015 latch above.
            0x2000..=0x3FFF | 0x4000..=0x4014 | 0x4016..=0x401F => 0,
            0x6000..=0x7FFF => self.wram[(addr - 0x6000) as usize],
            0x4020..=0xFFFF => self.mapper.cpu_read(addr),
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize] = value,
            0x4015 => self.apu_io = value,
            0x6000..=0x7FFF => self.wram[(addr - 0x6000) as usize] = value,
            0x4020..=0xFFFF => self.mapper.cpu_write(addr, value),
            _ => {}
        }
    }

    fn on_cpu_cycle(&mut self) {
        self.cycles = self.cycles.wrapping_add(1);
        // MMC1's consecutive-write bug + VRC/FME-7's CPU-cycle IRQ counters
        // both need a per-cycle tick. Drive it here so any mapper attached
        // via `parse()` gets accurate cycle bookkeeping during blargg-style
        // CPU validation runs.
        self.mapper.notify_cpu_cycle();
    }
}

/// Run a blargg-style ROM until either `$6000` reports a completion code
/// or `max_cycles` is reached.
///
/// The status protocol: read `$6000`. If `$80` keep going. If `$81` perform a
/// reset. Anything else is the terminal status code.
///
/// # Errors
///
/// Returns the underlying [`RomError`] if the bytes don't parse.
pub fn run_blargg_until_complete(
    rom_bytes: &[u8],
    max_cycles: u64,
) -> Result<BlarggResult, RomError> {
    let mut bus = BlarggBus::new(rom_bytes)?;
    let mut cpu = Cpu::new();
    cpu.reset(&mut bus);

    let magic = [b'D', b'E', b'B', 0];
    let mut started = false;

    while bus.cycles < max_cycles {
        // Has the magic been written so the status byte is meaningful?
        if !started {
            let m = [bus.wram[1], bus.wram[2], bus.wram[3]];
            if m == magic[..3] {
                started = true;
            }
        }
        let status = bus.wram[0];
        if started {
            match status {
                0x80 => {
                    // Running. Continue.
                }
                0x81 => {
                    // Reset requested. Real ROMs need ~100 ms before the reset
                    // line is asserted; we approximate by stepping a small
                    // fixed window of cycles before resetting.
                    for _ in 0..100_000 {
                        if bus.cycles >= max_cycles {
                            break;
                        }
                        cpu.step(&mut bus);
                    }
                    cpu.reset(&mut bus);
                }
                code => {
                    return Ok(BlarggResult {
                        status: code,
                        message: read_blargg_message(&bus),
                        cycles: bus.cycles,
                    });
                }
            }
        }
        cpu.step(&mut bus);
        if cpu.is_jammed() {
            return Ok(BlarggResult {
                status: bus.wram[0],
                message: read_blargg_message(&bus),
                cycles: bus.cycles,
            });
        }
    }

    Ok(BlarggResult {
        status: bus.wram[0],
        message: read_blargg_message(&bus),
        cycles: bus.cycles,
    })
}

fn read_blargg_message(bus: &BlarggBus) -> String {
    let mut out = String::new();
    let mut i = 4usize;
    while i < bus.wram.len() {
        let b = bus.wram[i];
        if b == 0 {
            break;
        }
        if b.is_ascii() && (b == b'\n' || !b.is_ascii_control()) {
            out.push(b as char);
        } else {
            out.push('.');
        }
        i += 1;
    }
    out
}

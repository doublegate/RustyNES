//! v3.5.0 (`T-SPEEDRUN-MEMIF`) — the memory-export interface.
//!
//! A fixed-layout, named shared-memory block that publishes the console's
//! RAM, its frame and master-clock counters and the speedrun timer's state
//! after every frame.
//!
//! # Why a published block
//!
//! A `LiveSplit` auto-splitter reads an emulator's memory by attaching to the
//! process and following pointers from base addresses someone found for ONE
//! build of ONE emulator; the next release moves them and every splitter
//! breaks. This block is the alternative: `RustyNES` copies what a splitter needs
//! into shared memory at offsets that are part of a documented, versioned
//! contract (`docs/memory-export-interface.md`, ADR 0047). A tool opens the
//! block by name and reads byte `0x0100 + a` for CPU RAM address `a`, on every
//! build, on every platform, with no process attachment and no elevated
//! permissions.
//!
//! # Layout version 1 (little-endian throughout)
//!
//! | Offset | Size | Field |
//! | --- | --- | --- |
//! | `0x0000` | 8 | magic `RNESMEMF` |
//! | `0x0008` | 4 | layout version, `1` |
//! | `0x000C` | 4 | offset of the CPU RAM block, `0x0100` |
//! | `0x0010` | 4 | layout size, `0x2B00` |
//! | `0x0014` | 4 | sequence counter: odd while a publish is in progress |
//! | `0x0018` | 8 | publish count (one per frame published) |
//! | `0x0020` | 8 | frame counter (PPU frames since power-on) |
//! | `0x0028` | 8 | CPU master clock |
//! | `0x0030` | 4 | master-clock frequency numerator (Hz) |
//! | `0x0034` | 4 | master-clock frequency denominator |
//! | `0x0038` | 1 | region: 0 NTSC, 1 PAL, 2 Dendy |
//! | `0x0039` | 1 | flags: bit 0 a game is loaded, bit 1 a speedrun timer is defined |
//! | `0x003A` | 1 | timer state: 0 idle, 1 running, 2 finished |
//! | `0x003B` | 1 | reserved |
//! | `0x003C` | 4 | splits completed |
//! | `0x0040` | 4 | splits defined |
//! | `0x0044` | 4 | the core's emulation epoch |
//! | `0x0048` | 8 | timer start, as a master-clock value |
//! | `0x0050` | 8 | timer elapsed, in master-clock cycles (live while running, final once finished) |
//! | `0x0058` | 8 | reserved |
//! | `0x0060` | 32 | SHA-256 of the ROM (header excluded, `Nes::rom_sha256`) |
//! | `0x0080` | 128 | reserved, zero |
//! | `0x0100` | 2048 | CPU RAM `$0000-$07FF` |
//! | `0x0900` | 8192 | cartridge RAM window `$6000-$7FFF` (as the CPU would read it, side-effect-free) |
//! | `0x2900` | 512 | split table: 64 x `u64` elapsed cycles at each split (0 = not reached) |
//!
//! The RAM blocks are the `RetroAchievements` NES flat space laid out
//! contiguously: RA address `x` is at `0x0100 + x` for both windows, so a
//! condition's addresses index the block directly.
//!
//! # Consistency
//!
//! Each publish is one seqlock update ([`crate::shm`] explains the counter):
//! a reader that sees the counter odd, or changed across its copy, retries, so
//! RAM, counters and timer always come from the same frame.
//!
//! # What it is not
//!
//! Output only. Nothing a reader writes into the block reaches the emulator:
//! the block is rewritten whole each frame and never read back, so attaching a
//! tool cannot change a frame, a sample or a bus cycle (the determinism
//! contract), and the interface is allowed in every session, hardcore
//! included, because it reveals only what the screen already shows a player.

use std::io;

use rustynes_core::Nes;
use rustynes_core::speedrun::{MAX_SPLITS, SpeedrunTimer, master_clock_hz_ratio};

use crate::shm::SharedRegion;

/// The default name of the block (`/dev/shm/rustynes-memif` on Linux).
pub const DEFAULT_NAME: &str = "rustynes-memif";

/// The block's first eight bytes.
pub const MAGIC: [u8; 8] = *b"RNESMEMF";

/// The layout version this build writes.
pub const LAYOUT_VERSION: u32 = 1;

/// Field offsets (see the module docs; `docs/memory-export-interface.md` is
/// the normative copy and `layout_matches_the_document` pins one to the other).
pub mod offset {
    /// `u32` layout version.
    pub const VERSION: usize = 0x0008;
    /// `u32` offset of the CPU RAM block.
    pub const RAM_OFFSET: usize = 0x000C;
    /// `u32` layout size.
    pub const LAYOUT_SIZE: usize = 0x0010;
    /// `u32` sequence counter.
    pub const SEQ: usize = 0x0014;
    /// `u64` publish count.
    pub const PUBLISHES: usize = 0x0018;
    /// `u64` frame counter.
    pub const FRAME: usize = 0x0020;
    /// `u64` CPU master clock.
    pub const MASTER_CLOCK: usize = 0x0028;
    /// `u32` master-clock frequency numerator.
    pub const HZ_NUM: usize = 0x0030;
    /// `u32` master-clock frequency denominator.
    pub const HZ_DEN: usize = 0x0034;
    /// `u8` region.
    pub const REGION: usize = 0x0038;
    /// `u8` flags.
    pub const FLAGS: usize = 0x0039;
    /// `u8` timer state.
    pub const TIMER_STATE: usize = 0x003A;
    /// `u32` splits completed.
    pub const SPLITS_DONE: usize = 0x003C;
    /// `u32` splits defined.
    pub const SPLITS_DEFINED: usize = 0x0040;
    /// `u32` emulation epoch.
    pub const EPOCH: usize = 0x0044;
    /// `u64` timer start (master clock).
    pub const TIMER_START: usize = 0x0048;
    /// `u64` timer elapsed (cycles).
    pub const TIMER_ELAPSED: usize = 0x0050;
    /// 32-byte ROM SHA-256.
    pub const ROM_SHA256: usize = 0x0060;
    /// 2 KiB CPU RAM.
    pub const CPU_RAM: usize = 0x0100;
    /// 8 KiB cartridge RAM window.
    pub const CART_RAM: usize = 0x0900;
    /// 64 x `u64` split table.
    pub const SPLITS: usize = 0x2900;
    /// End of the layout.
    pub const END: usize = 0x2B00;
}

/// Flag bit: a game is loaded.
pub const FLAG_GAME: u8 = 1;
/// Flag bit: a speedrun timer is defined.
pub const FLAG_TIMER: u8 = 2;

/// The block, owned by the emulator.
#[derive(Debug)]
pub struct MemoryExport {
    region: SharedRegion,
    publishes: u64,
    /// One frame's image, assembled here and copied into the region in one
    /// seqlock update (so the critical section is one copy long).
    image: Vec<u8>,
}

impl MemoryExport {
    /// Create (or take over) the block `name` and publish an empty image
    /// (no game loaded).
    ///
    /// # Errors
    ///
    /// An invalid name, or the OS refusing the region.
    pub fn create(name: &str) -> io::Result<Self> {
        let region = SharedRegion::create(name, offset::END)?;
        let mut me = Self {
            region,
            publishes: 0,
            image: vec![0; offset::END],
        };
        me.publish_empty();
        Ok(me)
    }

    /// The block's OS name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.region.name()
    }

    /// Fill the fixed header fields of `self.image`.
    fn header(&mut self) {
        self.publishes += 1;
        let img = &mut self.image;
        img[..8].copy_from_slice(&MAGIC);
        put_u32(img, offset::VERSION, LAYOUT_VERSION);
        put_u32(img, offset::RAM_OFFSET, u32_of(offset::CPU_RAM));
        put_u32(img, offset::LAYOUT_SIZE, u32_of(offset::END));
        put_u64(img, offset::PUBLISHES, self.publishes);
        put_u32(img, offset::EPOCH, rustynes_core::EMULATION_EPOCH);
    }

    /// Publish "no game loaded": the header with everything else zero.
    pub fn publish_empty(&mut self) {
        self.image.fill(0);
        self.header();
        self.commit();
    }

    /// Publish the console's state after a frame, and the timer's if one is
    /// defined. Reads only (side-effect-free peeks).
    pub fn publish(&mut self, nes: &mut Nes, timer: Option<&SpeedrunTimer>) {
        self.image.fill(0);
        self.header();
        let img = &mut self.image;
        put_u64(img, offset::FRAME, nes.frame());
        put_u64(img, offset::MASTER_CLOCK, nes.cpu().master_clock());
        let region = nes.region();
        let (num, den) = master_clock_hz_ratio(region);
        put_u32(img, offset::HZ_NUM, u32::try_from(num).unwrap_or(u32::MAX));
        put_u32(img, offset::HZ_DEN, u32::try_from(den).unwrap_or(u32::MAX));
        img[offset::REGION] = match region {
            rustynes_core::Region::Ntsc => 0,
            rustynes_core::Region::Pal => 1,
            rustynes_core::Region::Dendy => 2,
        };
        img[offset::ROM_SHA256..offset::ROM_SHA256 + 32].copy_from_slice(nes.rom_sha256());
        let ram = nes.wram();
        let n = ram.len().min(0x800);
        img[offset::CPU_RAM..offset::CPU_RAM + n].copy_from_slice(&ram[..n]);
        for i in 0..0x2000u16 {
            img[offset::CART_RAM + usize::from(i)] = nes.peek(0x6000 + i);
        }
        let mut flags = FLAG_GAME;
        if let Some(t) = timer {
            flags |= FLAG_TIMER;
            img[offset::TIMER_STATE] = t.state().code();
            let splits = t.splits();
            put_u32(img, offset::SPLITS_DONE, u32_of(splits.len()));
            put_u32(
                img,
                offset::SPLITS_DEFINED,
                u32_of(t.definition().splits.len()),
            );
            put_u64(img, offset::TIMER_START, t.start_clock());
            put_u64(img, offset::TIMER_ELAPSED, t.elapsed());
            for (i, s) in splits.iter().take(MAX_SPLITS).enumerate() {
                put_u64(img, offset::SPLITS + 8 * i, s.elapsed);
            }
        }
        img[offset::FLAGS] = flags;
        self.commit();
    }

    /// Copy the image into the region as one seqlock update.
    fn commit(&self) {
        let seq = self.region.atomic_u32(offset::SEQ);
        let s = seq.load(std::sync::atomic::Ordering::Relaxed);
        let odd = s.wrapping_add(1) | 1;
        seq.store(odd, std::sync::atomic::Ordering::Relaxed);
        std::sync::atomic::fence(std::sync::atomic::Ordering::Release);
        // Everything but the counter itself.
        self.region.write_bytes(0, &self.image[..offset::SEQ]);
        self.region
            .write_bytes(offset::SEQ + 4, &self.image[offset::SEQ + 4..]);
        seq.store(odd.wrapping_add(1), std::sync::atomic::Ordering::Release);
    }
}

/// A small count or offset as a `u32` (every caller's value is under 2^16).
fn u32_of(v: usize) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

fn put_u32(img: &mut [u8], at: usize, v: u32) {
    img[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_u64(img: &mut [u8], at: usize, v: u64) {
    img[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// A reader written from `docs/memory-export-interface.md` alone: it
    /// takes every offset from the document's table, as a third-party tool
    /// would, and shares nothing with the writer above but the name.
    struct DocReader(SharedRegion);
    impl DocReader {
        fn snapshot(&self) -> Vec<u8> {
            // Bounded: a publish that never completes fails, not hangs.
            for _ in 0..1_000_000 {
                let s1 = self.0.atomic_u32(0x14).load(Ordering::Acquire);
                if s1 % 2 == 1 {
                    std::thread::yield_now();
                    continue;
                }
                let mut b = vec![0u8; 0x2B00];
                self.0.read_bytes(0, &mut b);
                if self.0.atomic_u32(0x14).load(Ordering::Acquire) == s1 {
                    return b;
                }
            }
            panic!("the sequence counter never settled");
        }
    }
    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }
    fn u64_at(b: &[u8], at: usize) -> u64 {
        u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
    }

    fn unique() -> String {
        use std::sync::atomic::AtomicUsize;
        static N: AtomicUsize = AtomicUsize::new(0);
        format!(
            "rm{}m{}",
            std::process::id() % 100_000,
            N.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// The offsets in code equal the document's table (a field moved in one
    /// place and not the other fails here, and again in the splitter gate).
    #[test]
    fn layout_matches_the_document() {
        let doc = include_str!("../../../docs/memory-export-interface.md");
        for (name, at) in [
            ("magic", 0x0000),
            ("layout version", offset::VERSION),
            ("offset of the CPU RAM block", offset::RAM_OFFSET),
            ("layout size", offset::LAYOUT_SIZE),
            ("sequence counter", offset::SEQ),
            ("publish count", offset::PUBLISHES),
            ("frame counter", offset::FRAME),
            ("CPU master clock", offset::MASTER_CLOCK),
            ("frequency numerator", offset::HZ_NUM),
            ("frequency denominator", offset::HZ_DEN),
            ("region", offset::REGION),
            ("flags", offset::FLAGS),
            ("timer state", offset::TIMER_STATE),
            ("splits completed", offset::SPLITS_DONE),
            ("splits defined", offset::SPLITS_DEFINED),
            ("emulation epoch", offset::EPOCH),
            ("timer start", offset::TIMER_START),
            ("timer elapsed", offset::TIMER_ELAPSED),
            ("SHA-256 of the ROM", offset::ROM_SHA256),
            ("CPU RAM", offset::CPU_RAM),
            ("cartridge RAM window", offset::CART_RAM),
            ("split table", offset::SPLITS),
        ] {
            let row = doc
                .lines()
                .find(|l| l.starts_with(&format!("| `0x{at:04X}` |")))
                .unwrap_or_else(|| panic!("no table row at 0x{at:04X} ({name})"));
            assert!(
                row.contains(name),
                "row 0x{at:04X} is {row:?}, expected {name}"
            );
        }
        assert!(doc.contains(&format!("`{}`", std::str::from_utf8(&MAGIC).unwrap())));
        assert!(doc.contains("`0x2B00`"), "the layout size is documented");
    }

    /// A published frame, read through the document's offsets, equals the
    /// console's own state: counters, region fraction, hash, both RAM
    /// windows (the RA flat address `x` at `0x0100 + x`), and the timer.
    #[test]
    fn a_published_frame_matches_the_console() {
        use rustynes_core::speedrun::{SpeedrunTimer, SplitsDefinition};
        // NROM with 8 KiB PRG-RAM: the program stores a counter into $0005
        // and $6003 every loop.
        #[rustfmt::skip]
        let code: &[u8] = &[
            0xE6, 0x05,       // C000 INC $05
            0xA5, 0x05,       // C002 LDA $05
            0x8D, 0x03, 0x60, // C004 STA $6003
            0x4C, 0x00, 0xC0, // C007 JMP $C000
        ];
        let mut rom = vec![
            b'N', b'E', b'S', 0x1A, 1, 1, 0x02, 0, 1, 0, 0, 0, 0, 0, 0, 0,
        ];
        let mut prg = vec![0xEAu8; 0x4000];
        prg[..code.len()].copy_from_slice(code);
        prg[0x3FFA..].copy_from_slice(&[0x00, 0xC0, 0x00, 0xC0, 0x00, 0xC0]);
        rom.extend_from_slice(&prg);
        rom.extend_from_slice(&[0u8; 0x2000]);
        let mut nes = Nes::from_rom(&rom).unwrap();
        let def = SplitsDefinition::parse("start = 0xH0005>0\nsplit = s | 0xH0005=0\n").unwrap();
        let mut timer = SpeedrunTimer::new(def, nes.region());
        let name = unique();
        let mut m = MemoryExport::create(&name).unwrap();
        for _ in 0..3 {
            nes.run_frame();
            let _ = timer.observe_nes(&mut nes);
            m.publish(&mut nes, Some(&timer));
        }
        let b = DocReader(SharedRegion::open(&name).unwrap()).snapshot();
        assert_eq!(u64_at(&b, 0x18), 4, "create + three publishes");
        assert_eq!(u64_at(&b, 0x20), nes.frame());
        assert_eq!(u64_at(&b, 0x28), nes.cpu().master_clock());
        assert_eq!((u32_at(&b, 0x30), u32_at(&b, 0x34)), (236_250_000, 11));
        assert_eq!(b[0x38], 0, "NTSC");
        assert_eq!(b[0x39], 3, "game loaded, timer defined");
        assert_eq!(b[0x3A], 1, "running");
        assert_eq!(u32_at(&b, 0x40), 1, "one split defined");
        assert_eq!(u32_at(&b, 0x44), rustynes_core::EMULATION_EPOCH);
        assert_eq!(u64_at(&b, 0x48), timer.start_clock());
        assert_eq!(u64_at(&b, 0x50), timer.elapsed());
        assert!(timer.elapsed() > 0);
        assert_eq!(&b[0x60..0x80], nes.rom_sha256());
        assert_eq!(&b[0x100..0x900], nes.wram());
        assert_ne!(b[0x100 + 5], 0, "the counter ran");
        assert_eq!(
            b[0x100 + 0x0800 + 3],
            nes.peek(0x6003),
            "RA 0x0803 is $6003"
        );
        assert_ne!(b[0x100 + 0x0800 + 3], 0, "the cartridge RAM window is live");
    }

    /// A block with no game: magic, version, sizes, and flags 0.
    #[test]
    fn an_empty_block_is_well_formed() {
        let name = unique();
        let _m = MemoryExport::create(&name).unwrap();
        let r = DocReader(SharedRegion::open(&name).unwrap());
        let b = r.snapshot();
        assert_eq!(&b[..8], b"RNESMEMF");
        assert_eq!(u32_at(&b, 8), 1);
        assert_eq!(u32_at(&b, 0x0C), 0x100);
        assert_eq!(u32_at(&b, 0x10), 0x2B00);
        assert_eq!(u64_at(&b, 0x18), 1, "one publish");
        assert_eq!(b[0x39], 0, "no game");
    }
}

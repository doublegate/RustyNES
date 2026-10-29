//! v3.6.0 (`T-AUDIO-STEMS`) — trace-assisted NSF ripping: find a game's music
//! driver from what the CPU does, propose its `init` and `play` addresses, and
//! write an NSF that drives it.
//!
//! An NSF file (nesdev "NSF") is a music engine lifted out of a game plus a
//! header that says how to drive it: load the program image at `load`, call
//! `init` once with the song number in A, then call `play` once per frame. A
//! ripper's job is to find those two routines. This one finds them by
//! watching the game run.
//!
//! # How the routines are found
//!
//! [`Tracer`] steps the console one instruction at a time and keeps a
//! **shadow call stack**: a `JSR` pushes its target; an interrupt (a step
//! that pushed three bytes and landed on the NMI or IRQ/BRK vector's target)
//! pushes the vector; and an entry is popped when the stack pointer climbs back to
//! where it was before the call, which is what `RTS` and `RTI` do (and also
//! what a routine that discards its own return address does). The CPU bus
//! access log (`Nes::set_access_logging`) names every write an instruction
//! made; each write to an APU register (`$4000-$4013`, `$4015`, `$4017`: not
//! `$4014`, which is OAM DMA, nor `$4016`, the controller strobe) is recorded
//! with the shadow stack it happened under.
//!
//! [`propose`] then reads the evidence:
//!
//! - **play** is the routine that encloses every APU write of a frame. For
//!   each frame, the common prefix of its writes' call chains is taken; a
//!   routine that is in that prefix in the most frames is the driver, and of
//!   several such (the NMI handler's body calling the driver, say) the
//!   innermost is chosen, because an outer one also does the game's other
//!   per-frame work. The evidence reported is how many frames it enclosed
//!   every write in, how many times it was called per frame, and how many of
//!   the run's APU writes it enclosed.
//! - **init** is the routine that encloses every APU write made outside a
//!   `play` call, if there is one: that is the code that silences the
//!   channels and sets the engine up before the first frame plays. When the
//!   game makes no such writes from a subroutine (its reset code writes the
//!   APU inline, or not at all), no init is proposed and the NSF gets a no-op
//!   one, said so in the report: the NSF player zeroes RAM before init, which
//!   is often all the engine needed.
//!
//! These are heuristics over one run, and the report says which: the proposal
//! is what the evidence supports, with the counts to judge it by. The gate is
//! that the NSF plays the tune — `rustynes-core`'s tests and
//! `docs/audio-stems.md` describe it: the NSF's `play` calls make the same
//! APU writes, call for call, as the game's.
//!
//! # What can be ripped
//!
//! The program image is the cartridge's PRG-ROM, loaded at `$8000`. That is
//! only the code the console ran when the PRG is not bank-switched: mapper 0
//! (NROM) and mapper 3 (CNROM, whose banking is CHR only), up to 32 KiB, a
//! 16 KiB image mirrored to fill `$8000-$FFFF` as the board does. A
//! bank-switched board would need the NSF's own 4 KiB bank registers set to
//! the layout the driver runs under, which this does not infer;
//! [`build_nsf`] refuses it with that reason rather than write a file that
//! does not play. It refuses a driver that runs from RAM for the same
//! reason: the code is not in the image (the game copied it there), and the
//! NSF player clears RAM before `init`.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::{format, vec};

use crate::{Nes, Region};

/// The CPU registers a write in this range is an APU register write for the
/// purposes of ripping: `$4000-$4013`, `$4015`, `$4017`.
#[must_use]
pub const fn is_apu_register(addr: u16) -> bool {
    matches!(addr, 0x4000..=0x4013 | 0x4015 | 0x4017)
}

/// How a routine on the shadow call stack was entered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallKind {
    /// A `JSR` to the target.
    Jsr,
    /// The NMI, which entered its vector's target.
    Nmi,
    /// An IRQ or `BRK`, which entered its vector's target.
    Irq,
}

/// One entry of the shadow call stack, kept for the whole run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Call {
    /// How it was entered.
    pub kind: CallKind,
    /// The routine's address.
    pub target: u16,
    /// The frame it was entered in (`Nes::frame`).
    pub frame: u64,
}

/// One APU register write and the calls it happened under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApuWrite {
    /// The frame it happened in.
    pub frame: u64,
    /// The register.
    pub addr: u16,
    /// The byte.
    pub value: u8,
    /// The instruction that wrote it.
    pub pc: u16,
    /// The call chain, outermost first, as indices into [`Tracer::calls`].
    pub chain: Vec<u32>,
}

/// A running trace over one [`Nes`].
pub struct Tracer {
    /// The live shadow stack: (index into `calls`, stack pointer before entry).
    stack: Vec<(u32, u8)>,
    calls: Vec<Call>,
    writes: Vec<ApuWrite>,
    frames: u64,
}

/// The most instructions [`Tracer::run_frame`] steps before giving up on a
/// frame (a jammed or interrupt-less program): about five NTSC frames of
/// two-cycle instructions.
const MAX_STEPS_PER_FRAME: u32 = 80_000;

impl Tracer {
    /// Start tracing: turns the console's access log on.
    pub fn new(nes: &mut Nes) -> Self {
        nes.set_access_logging(true);
        nes.bus_mut().clear_accesses();
        Self {
            stack: Vec::new(),
            calls: Vec::new(),
            writes: Vec::new(),
            frames: 0,
        }
    }

    /// Every call entered so far.
    #[must_use]
    pub fn calls(&self) -> &[Call] {
        &self.calls
    }

    /// Every APU write so far.
    #[must_use]
    pub fn writes(&self) -> &[ApuWrite] {
        &self.writes
    }

    /// Frames traced so far.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    fn vector(nes: &mut Nes, at: u16) -> u16 {
        u16::from(nes.cpu_bus_peek(at)) | (u16::from(nes.cpu_bus_peek(at + 1)) << 8)
    }

    /// Step one instruction and record what it did.
    pub fn step(&mut self, nes: &mut Nes) {
        let pc = nes.cpu().pc;
        let s_before = nes.cpu().s;
        let opcode = nes.cpu_bus_peek(pc);
        let jsr_target = (opcode == 0x20).then(|| {
            u16::from(nes.cpu_bus_peek(pc.wrapping_add(1)))
                | (u16::from(nes.cpu_bus_peek(pc.wrapping_add(2))) << 8)
        });
        let frame = nes.frame();
        nes.step_instruction();

        // The writes the instruction made belong to the context it ran in:
        // the stack as it was before this step's own call or return.
        let chain: Vec<u32> = self.stack.iter().map(|&(i, _)| i).collect();
        for a in nes.accesses() {
            if a.write && is_apu_register(a.addr) {
                self.writes.push(ApuWrite {
                    frame,
                    addr: a.addr,
                    value: a.value,
                    pc,
                    chain: chain.clone(),
                });
            }
        }
        nes.bus_mut().clear_accesses();

        let pc_after = nes.cpu().pc;
        let s_after = nes.cpu().s;
        // The CPU services an interrupt as a step of its own (the seven-cycle
        // sequence, no instruction), so on such a step the opcode at `pc` was
        // NOT executed: it is the instruction the interrupt returns to, and a
        // `JSR` there must not be taken for a call. An interrupt step pushed
        // exactly three bytes (P and the return address) and landed on a
        // vector's target, which tells it apart from the two lookalikes: a
        // jump onto a handler (it pushes nothing, or two bytes for `JSR`) and
        // code that falls through onto the address a vector names (nothing).
        // `BRK` makes the same push onto the IRQ vector and is recorded as
        // the IRQ entry it is.
        let nmi = Self::vector(nes, 0xFFFA);
        let irq = Self::vector(nes, 0xFFFE);
        let pushed_three = s_before.wrapping_sub(s_after) == 3;
        let entered = if !pushed_three {
            None
        } else if pc_after == nmi {
            Some(CallKind::Nmi)
        } else if pc_after == irq {
            Some(CallKind::Irq)
        } else {
            None
        };
        let called = if entered.is_none() { jsr_target } else { None };

        // Pop every call the stack pointer has climbed back past.
        if entered.is_none() {
            while let Some(&(_, s_entry)) = self.stack.last() {
                if s_after >= s_entry {
                    self.stack.pop();
                } else {
                    break;
                }
            }
        }
        if let Some(target) = called {
            self.push(CallKind::Jsr, target, frame, s_before);
        } else if let Some(kind) = entered {
            self.push(kind, pc_after, frame, s_before);
        }
    }

    fn push(&mut self, kind: CallKind, target: u16, frame: u64, s_entry: u8) {
        // A program that never returns from its calls (a reset routine that
        // JSRs into a main loop, a handler that resets the stack) would grow
        // the shadow stack forever; past the 6502's own 128 return addresses
        // the oldest entries are dropped, as the hardware stack would wrap.
        if self.stack.len() >= 128 {
            self.stack.remove(0);
        }
        let idx = u32::try_from(self.calls.len()).unwrap_or(u32::MAX);
        self.calls.push(Call {
            kind,
            target,
            frame,
        });
        self.stack.push((idx, s_entry));
    }

    /// Step until the console's frame counter advances (one frame), or a cap
    /// of steps is reached. Returns `false` when the cap was hit or the CPU
    /// jammed.
    pub fn run_frame(&mut self, nes: &mut Nes) -> bool {
        let start = nes.frame();
        for _ in 0..MAX_STEPS_PER_FRAME {
            if nes.is_jammed() {
                return false;
            }
            self.step(nes);
            if nes.frame() != start {
                self.frames += 1;
                return true;
            }
        }
        false
    }

    /// The writes each call to `target` made (directly or in routines it
    /// called), in call order: one list per call, each `(register, byte)`.
    /// A call that wrote nothing is an empty list.
    #[must_use]
    pub fn writes_per_call(&self, target: u16) -> Vec<Vec<(u16, u8)>> {
        let mut out: Vec<Vec<(u16, u8)>> = Vec::new();
        let mut index: Vec<Option<usize>> = vec![None; self.calls.len()];
        for (i, c) in self.calls.iter().enumerate() {
            if c.target == target && c.kind == CallKind::Jsr {
                index[i] = Some(out.len());
                out.push(Vec::new());
            }
        }
        for w in &self.writes {
            // The innermost enclosing call to `target` owns the write.
            if let Some(slot) = w
                .chain
                .iter()
                .rev()
                .find_map(|&c| index.get(c as usize).copied().flatten())
            {
                out[slot].push((w.addr, w.value));
            }
        }
        out
    }
}

/// What the trace says about the `play` routine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayEvidence {
    /// The proposed address.
    pub addr: u16,
    /// Frames that made at least one APU write.
    pub frames_with_writes: u64,
    /// Of those, frames in which this routine enclosed every write.
    pub frames_enclosed: u64,
    /// Frames in which it was called.
    pub frames_called: u64,
    /// The most common number of calls per frame it was called in.
    pub calls_per_frame: u32,
    /// The run's APU writes it enclosed.
    pub writes_enclosed: u64,
    /// Every APU write of the run.
    pub writes_total: u64,
}

/// What the trace says about the `init` routine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InitEvidence {
    /// A routine encloses every APU write made outside `play`.
    Routine {
        /// Its address.
        addr: u16,
        /// The writes it enclosed.
        writes: u64,
        /// How many times it was called in the run.
        calls: u64,
    },
    /// No routine does (the reset code writes the APU inline, or never
    /// outside play); the NSF gets a no-op init.
    None {
        /// APU writes made outside `play`.
        writes_outside_play: u64,
    },
}

/// The proposal [`propose`] makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    /// The `play` routine.
    pub play: PlayEvidence,
    /// The `init` routine, or why there is none.
    pub init: InitEvidence,
}

/// Why no proposal could be made.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RipError {
    /// The run made no APU writes at all.
    #[error("the run made no APU register writes; nothing to rip")]
    NoWrites,
    /// The writes are not inside any subroutine (the driver runs inline in an
    /// interrupt handler or the main loop), so there is no `play` to call.
    #[error(
        "the APU writes are not inside a subroutine (the driver runs inline, \
         in an interrupt handler or the main loop), so there is no play routine to call"
    )]
    NoSubroutine,
    /// The cartridge's PRG is bank-switched, or too large, for a flat image.
    #[error("{0}")]
    Unsupported(String),
}

/// The common prefix of several call chains.
fn common_prefix<'a>(mut chains: impl Iterator<Item = &'a [u32]>) -> Vec<u32> {
    let Some(first) = chains.next() else {
        return Vec::new();
    };
    let mut prefix = first.to_vec();
    for c in chains {
        let n = prefix.iter().zip(c).take_while(|(a, b)| a == b).count();
        prefix.truncate(n);
    }
    prefix
}

/// The routine that encloses every APU write of a frame in the most frames,
/// and in how many. Of several (an NMI body that calls the driver encloses
/// the same writes as the driver does), the innermost: the deepest position
/// on the stack any of them was seen at.
fn pick_play(t: &Tracer, frames: &[u64]) -> Result<(u16, u64), RipError> {
    let mut enclosed: Vec<(u16, u64)> = Vec::new(); // (target, frames)
    let mut depth: Vec<(u16, usize)> = Vec::new(); // deepest position seen
    for &f in frames {
        let prefix = common_prefix(
            t.writes
                .iter()
                .filter(|w| w.frame == f)
                .map(|w| w.chain.as_slice()),
        );
        let mut seen: Vec<u16> = Vec::new();
        for (pos, &ci) in prefix.iter().enumerate() {
            let c = t.calls[ci as usize];
            if c.kind != CallKind::Jsr || seen.contains(&c.target) {
                continue;
            }
            seen.push(c.target);
            match enclosed.iter_mut().find(|(a, _)| *a == c.target) {
                Some((_, n)) => *n += 1,
                None => enclosed.push((c.target, 1)),
            }
            match depth.iter_mut().find(|(a, _)| *a == c.target) {
                Some((_, d)) => *d = (*d).max(pos),
                None => depth.push((c.target, pos)),
            }
        }
    }
    let best = enclosed
        .iter()
        .map(|&(_, n)| n)
        .max()
        .ok_or(RipError::NoSubroutine)?;
    let play = enclosed
        .iter()
        .filter(|&&(_, n)| n == best)
        .map(|&(a, _)| a)
        .max_by_key(|a| depth.iter().find(|(d, _)| d == a).map_or(0, |&(_, p)| p))
        .ok_or(RipError::NoSubroutine)?;
    Ok((play, best))
}

/// How many frames `play` was called in, and the most common number of
/// calls in such a frame (the smaller on a tie).
fn cadence(t: &Tracer, play: u16) -> (u64, u32) {
    let mut per_frame: Vec<(u64, u32)> = Vec::new();
    for c in t
        .calls
        .iter()
        .filter(|c| c.kind == CallKind::Jsr && c.target == play)
    {
        match per_frame.iter_mut().find(|(f, _)| *f == c.frame) {
            Some((_, n)) => *n += 1,
            None => per_frame.push((c.frame, 1)),
        }
    }
    let mut counts: Vec<(u32, u64)> = Vec::new();
    for &(_, n) in &per_frame {
        match counts.iter_mut().find(|(k, _)| *k == n) {
            Some((_, m)) => *m += 1,
            None => counts.push((n, 1)),
        }
    }
    let calls_per_frame = counts
        .iter()
        .max_by_key(|&&(k, m)| (m, core::cmp::Reverse(k)))
        .map_or(0, |&(k, _)| k);
    (per_frame.len() as u64, calls_per_frame)
}

/// The innermost subroutine enclosing every write in `outside` (the writes
/// made outside `play`), if one does.
fn pick_init(t: &Tracer, outside: &[&ApuWrite]) -> InitEvidence {
    let none = InitEvidence::None {
        writes_outside_play: outside.len() as u64,
    };
    if outside.is_empty() {
        return none;
    }
    let prefix = common_prefix(outside.iter().map(|w| w.chain.as_slice()));
    prefix
        .iter()
        .rev()
        .map(|&c| t.calls[c as usize])
        .find(|c| c.kind == CallKind::Jsr)
        .map_or(none, |c| InitEvidence::Routine {
            addr: c.target,
            writes: outside.len() as u64,
            calls: t
                .calls
                .iter()
                .filter(|k| k.kind == CallKind::Jsr && k.target == c.target)
                .count() as u64,
        })
}

/// Propose `init` and `play` from a finished trace.
///
/// # Errors
///
/// [`RipError::NoWrites`] or [`RipError::NoSubroutine`].
pub fn propose(t: &Tracer) -> Result<Proposal, RipError> {
    if t.writes.is_empty() {
        return Err(RipError::NoWrites);
    }
    // The frames with writes, in order (the writes are recorded in order).
    let mut frames: Vec<u64> = t.writes.iter().map(|w| w.frame).collect();
    frames.dedup();
    let (play, frames_enclosed) = pick_play(t, &frames)?;
    let (frames_called, calls_per_frame) = cadence(t, play);
    let inside_play = |w: &ApuWrite| {
        w.chain.iter().any(|&c| {
            t.calls[c as usize].kind == CallKind::Jsr && t.calls[c as usize].target == play
        })
    };
    let outside: Vec<&ApuWrite> = t.writes.iter().filter(|w| !inside_play(w)).collect();
    Ok(Proposal {
        play: PlayEvidence {
            addr: play,
            frames_with_writes: frames.len() as u64,
            frames_enclosed,
            frames_called,
            calls_per_frame,
            writes_enclosed: (t.writes.len() - outside.len()) as u64,
            writes_total: t.writes.len() as u64,
        },
        init: pick_init(t, &outside),
    })
}

/// NSF header length.
const HEADER: usize = 0x80;

/// The metadata strings an NSF carries (32 bytes each, NUL-padded; longer
/// text is cut at a character boundary).
#[derive(Clone, Debug, Default)]
pub struct NsfInfo {
    /// Song / game name.
    pub name: String,
    /// Artist.
    pub artist: String,
    /// Copyright.
    pub copyright: String,
}

fn put_str(out: &mut [u8], s: &str) {
    // 31 bytes of text, at least one NUL.
    let mut end = s.len().min(31);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    out[..end].copy_from_slice(&s.as_bytes()[..end]);
}

/// Write an NSF (version 1, one song) that drives `proposal` over the
/// cartridge `rom`'s PRG.
///
/// When the proposal has no init routine, the init address is a `$60` (RTS)
/// byte found in the image, so init returns at once; the returned
/// [`NoOpInit`] says where. `region` sets the file's timing byte and speed word.
///
/// # Errors
///
/// [`RipError::Unsupported`] for a board this cannot image (see the module
/// docs), or an image with no `RTS` byte to use as a no-op init.
pub fn build_nsf(
    rom: &[u8],
    proposal: &Proposal,
    region: Region,
    info: &NsfInfo,
) -> Result<(Vec<u8>, Option<NoOpInit>), RipError> {
    let header = rustynes_mappers::parse_header(rom)
        .map_err(|e| RipError::Unsupported(format!("the ROM header does not parse: {e}")))?;
    let mapper = header.mapper_id;
    if !matches!(mapper, 0 | 3) {
        return Err(RipError::Unsupported(format!(
            "mapper {mapper} bank-switches its PRG; ripping needs the NSF bank layout \
             the driver runs under, which is not inferred (mappers 0 and 3 only)"
        )));
    }
    let prg_len = header.prg_size;
    let start = 16 + if header.has_trainer { 512 } else { 0 };
    let prg = rom
        .get(start..start + prg_len)
        .ok_or_else(|| RipError::Unsupported("the PRG-ROM is truncated".into()))?;
    let image: Vec<u8> = match prg_len {
        0x4000 => prg.iter().chain(prg).copied().collect(),
        0x8000 => prg.to_vec(),
        n => {
            return Err(RipError::Unsupported(format!(
                "a {n}-byte PRG-ROM is neither 16 nor 32 KiB"
            )));
        }
    };

    // The image is the ROM at $8000. A driver the game copied into RAM
    // (some engines run from $0000-$07FF or cartridge RAM) is not in it, and
    // the NSF player clears RAM before init, so its play would call zeroes.
    let in_image = |a: u16| a >= 0x8000;
    if !in_image(proposal.play.addr) {
        return Err(RipError::Unsupported(format!(
            "play ${:04X} runs from RAM, which the NSF player clears before init; \
             the driver is not in the ROM image",
            proposal.play.addr
        )));
    }
    if let InitEvidence::Routine { addr, .. } = proposal.init
        && !in_image(addr)
    {
        return Err(RipError::Unsupported(format!(
            "init ${addr:04X} runs from RAM, which the NSF player clears before init"
        )));
    }
    let (init, noop) = match proposal.init {
        InitEvidence::Routine { addr, .. } => (addr, None),
        InitEvidence::None { .. } => {
            let at = image
                .iter()
                .position(|&b| b == 0x60)
                .ok_or_else(|| RipError::Unsupported("no RTS byte for a no-op init".into()))?;
            let addr = 0x8000 + u16::try_from(at).unwrap_or(0);
            (addr, Some(NoOpInit { addr }))
        }
    };

    let mut out = vec![0u8; HEADER];
    out[..5].copy_from_slice(b"NESM\x1A");
    out[0x05] = 1; // version
    out[0x06] = 1; // songs
    out[0x07] = 1; // starting song
    out[0x08..0x0A].copy_from_slice(&0x8000u16.to_le_bytes());
    out[0x0A..0x0C].copy_from_slice(&init.to_le_bytes());
    out[0x0C..0x0E].copy_from_slice(&proposal.play.addr.to_le_bytes());
    put_str(&mut out[0x0E..0x2E], &info.name);
    put_str(&mut out[0x2E..0x4E], &info.artist);
    put_str(&mut out[0x4E..0x6E], &info.copyright);
    // Play speed in microseconds per call: the hardware frame (nesdev's
    // 16639 NTSC, 19997 PAL), the rate a once-per-frame driver runs at. A
    // driver the trace saw called more than once a frame
    // (`PlayEvidence::calls_per_frame`) is still filed at the frame rate,
    // and the report says so: a faster divider would change the tempo of
    // a driver that counts its own ticks.
    out[0x6E..0x70].copy_from_slice(&16_639u16.to_le_bytes());
    // $70-$77 bank registers: all zero, no bank switching.
    out[0x78..0x7A].copy_from_slice(&19_997u16.to_le_bytes());
    out[0x7A] = u8::from(matches!(region, Region::Pal));
    out[0x7B] = 0; // no expansion chips on mappers 0 and 3
    out.extend_from_slice(&image);
    Ok((out, noop))
}

/// Where [`build_nsf`] pointed a no-op init.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoOpInit {
    /// The `RTS` byte's address.
    pub addr: u16,
}

/// The routines of [`example_music_rom`]'s program, for the tests and docs
/// that drive it.
pub mod example {
    /// `INIT` (reset calls it with A = 0).
    pub const INIT: u16 = 0xC045;
    /// `PLAY`, the driver.
    pub const PLAY: u16 = 0xC059;
    /// `BODY`, the NMI's per-frame work, which calls `PLAY`.
    pub const BODY: u16 = 0xC02E;
    /// The NMI handler.
    pub const NMI: u16 = 0xC020;
    /// The main loop the reset code falls into (`JSR IDLE; JMP LOOP`), which
    /// is also the IRQ vector's target.
    pub const LOOP: u16 = 0xC019;
}

/// A homebrew NROM with a tiny music driver, written for the ripper's tests
/// and docs.
///
/// No commercial code: the program is this function, assembled at `$C000`
/// of a 16 KiB image, so the board also shows it at `$8000`.
///
/// Reset calls `INIT` (A = song, ignored), which turns the channels on and
/// zeroes the driver's two RAM bytes, waits two vblanks (the PPU ignores
/// `$2000` until it has warmed up), enables the vblank NMI and spins. The NMI
/// handler saves the registers and calls `BODY`, the game's per-frame work,
/// which starts an OAM DMA (`$4014`), strobes the controller (`$4016`) and
/// calls `PLAY`; the main loop calls an empty `IDLE` routine forever, so the
/// NMI often arrives just as a `JSR` is next. Every eighth call `PLAY` steps an eight-note scale on pulse 1
/// and, through a nested `TRI` routine, on the triangle. So a trace of it has
/// a driver that is not the NMI handler, an enclosing routine that is not the
/// driver, a nested routine inside it, two non-APU writes in `$4000-$4017`,
/// and reset code that falls through onto the address the IRQ vector names.
/// The addresses are in [`example`].
pub fn example_music_rom() -> Vec<u8> {
    let code: &[u8] = &[
        // RESET: $C000
        0x78, // $C000 SEI
        0xD8, // $C001 CLD
        0xA2, 0xFF, // $C002 LDX #$FF
        0x9A, // $C004 TXS
        0xA9, 0x00, // $C005 LDA #$00
        0x20, 0x45, 0xC0, // $C007 JSR INIT
        // VW1: $C00A
        0x2C, 0x02, 0x20, // $C00A BIT $2002
        0x10, 0xFB, // $C00D BPL VW1
        // VW2: $C00F
        0x2C, 0x02, 0x20, // $C00F BIT $2002
        0x10, 0xFB, // $C012 BPL VW2
        0xA9, 0x80, // $C014 LDA #$80
        0x8D, 0x00, 0x20, // $C016 STA $2000
        // LOOP: $C019
        0x20, 0x1F, 0xC0, // $C019 JSR IDLE
        0x4C, 0x19, 0xC0, // $C01C JMP LOOP
        // IDLE: $C01F
        0x60, // $C01F RTS
        // NMI: $C020
        0x48, // $C020 PHA
        0x8A, // $C021 TXA
        0x48, // $C022 PHA
        0x98, // $C023 TYA
        0x48, // $C024 PHA
        0x20, 0x2E, 0xC0, // $C025 JSR BODY
        0x68, // $C028 PLA
        0xA8, // $C029 TAY
        0x68, // $C02A PLA
        0xAA, // $C02B TAX
        0x68, // $C02C PLA
        0x40, // $C02D RTI
        // BODY: $C02E
        0xA9, 0x02, // $C02E LDA #$02
        0x8D, 0x14, 0x40, // $C030 STA $4014
        0x20, 0x3A, 0xC0, // $C033 JSR JOY
        0x20, 0x59, 0xC0, // $C036 JSR PLAY
        0x60, // $C039 RTS
        // JOY: $C03A
        0xA9, 0x01, // $C03A LDA #$01
        0x8D, 0x16, 0x40, // $C03C STA $4016
        0xA9, 0x00, // $C03F LDA #$00
        0x8D, 0x16, 0x40, // $C041 STA $4016
        0x60, // $C044 RTS
        // INIT: $C045
        0xA9, 0x0F, // $C045 LDA #$0F
        0x8D, 0x15, 0x40, // $C047 STA $4015
        0xA9, 0x30, // $C04A LDA #$30
        0x8D, 0x04, 0x40, // $C04C STA $4004
        0x8D, 0x0C, 0x40, // $C04F STA $400C
        0xA9, 0x00, // $C052 LDA #$00
        0x85, 0x00, // $C054 STA $00
        0x85, 0x01, // $C056 STA $01
        0x60, // $C058 RTS
        // PLAY: $C059
        0xC6, 0x01, // $C059 DEC $01
        0x10, 0x20, // $C05B BPL DONE
        0xA9, 0x07, // $C05D LDA #$07
        0x85, 0x01, // $C05F STA $01
        0xA6, 0x00, // $C061 LDX $00
        0xA9, 0xBF, // $C063 LDA #$BF
        0x8D, 0x00, 0x40, // $C065 STA $4000
        0xBD, 0x8F, 0xC0, // $C068 LDA LO,X
        0x8D, 0x02, 0x40, // $C06B STA $4002
        0xBD, 0x97, 0xC0, // $C06E LDA HI,X
        0x8D, 0x03, 0x40, // $C071 STA $4003
        0x20, 0x7E, 0xC0, // $C074 JSR TRI
        0xE8, // $C077 INX
        0x8A, // $C078 TXA
        0x29, 0x07, // $C079 AND #$07
        0x85, 0x00, // $C07B STA $00
        // DONE: $C07D
        0x60, // $C07D RTS
        // TRI: $C07E
        0xA9, 0xC0, // $C07E LDA #$C0
        0x8D, 0x08, 0x40, // $C080 STA $4008
        0xBD, 0x8F, 0xC0, // $C083 LDA LO,X
        0x8D, 0x0A, 0x40, // $C086 STA $400A
        0xA9, 0x08, // $C089 LDA #$08
        0x8D, 0x0B, 0x40, // $C08B STA $400B
        0x60, // $C08E RTS
        // LO: $C08F
        0xFD, 0xE2, 0xC9, 0xBD, 0xA9, 0x96, 0x86, 0x7E, // $C08F .db
        // HI: $C097
        0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, // $C097 .db
    ];
    let mut prg = vec![0u8; 0x4000];
    prg[..code.len()].copy_from_slice(code);
    // Vectors: NMI, RESET, IRQ. The IRQ is never enabled, and its vector
    // names the spin loop on purpose: the reset code falls through onto that
    // address, which the tracer must not take for an interrupt entry.
    prg[0x3FFA..0x3FFC].copy_from_slice(&example::NMI.to_le_bytes());
    prg[0x3FFC..0x3FFE].copy_from_slice(&0xC000u16.to_le_bytes());
    prg[0x3FFE..0x4000].copy_from_slice(&example::LOOP.to_le_bytes());
    let mut rom = vec![b'N', b'E', b'S', 0x1A, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    rom.extend_from_slice(&prg);
    rom.extend(core::iter::repeat_n(0u8, 0x2000));
    rom
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::example::{BODY, INIT, NMI, PLAY};

    fn music_rom() -> Vec<u8> {
        example_music_rom()
    }

    fn trace(nes: &mut Nes, frames: u32) -> Tracer {
        let mut t = Tracer::new(nes);
        for _ in 0..frames {
            assert!(t.run_frame(nes), "a frame completes");
        }
        t
    }

    #[test]
    fn the_driver_and_its_init_are_found() {
        let mut nes = Nes::from_rom(&music_rom()).expect("loads");
        let t = trace(&mut nes, 120);
        let p = propose(&t).expect("a proposal");
        assert_eq!(p.play.addr, PLAY, "play, not the enclosing {BODY:04X}");
        assert_eq!(p.play.calls_per_frame, 1);
        assert!(p.play.frames_with_writes >= 14, "{p:?}");
        assert_eq!(
            p.play.frames_enclosed,
            p.play.frames_with_writes - 1,
            "{p:?}"
        );
        assert_eq!(
            p.init,
            InitEvidence::Routine {
                addr: INIT,
                writes: 3,
                calls: 1
            }
        );
        // The shadow stack saw only what the program did: one NMI per frame
        // and never an IRQ (the reset code falls through onto the address
        // the IRQ vector names, which is not an interrupt).
        assert!(
            t.calls().iter().all(|c| c.kind != CallKind::Irq),
            "no IRQ was taken"
        );
        let nmis = t.calls().iter().filter(|c| c.kind == CallKind::Nmi).count();
        assert!((115..=119).contains(&nmis), "{nmis} NMIs in 120 frames");
        // The OAM DMA and controller strobe were not taken for APU writes.
        assert!(
            t.writes()
                .iter()
                .all(|w| w.addr != 0x4014 && w.addr != 0x4016)
        );
    }

    #[test]
    fn the_ripped_nsf_plays_the_same_tune() {
        let rom = music_rom();
        let mut nes = Nes::from_rom(&rom).expect("loads");
        let game = trace(&mut nes, 200);
        let proposal = propose(&game).expect("a proposal");
        let (nsf, noop) =
            build_nsf(&rom, &proposal, nes.region(), &NsfInfo::default()).expect("an NSF");
        assert_eq!(noop, None);
        assert_eq!(&nsf[..5], b"NESM\x1A");
        assert_eq!(nsf.len(), 0x80 + 0x8000, "a 16 KiB PRG fills $8000-$FFFF");

        // Play it in the NSF player and trace that run the same way.
        let mut player = Nes::from_nsf(&nsf).expect("the NSF parses");
        let played = trace(&mut player, 200);
        let replay = propose(&played).expect("the NSF's own run has a driver");
        assert_eq!(replay.play.addr, PLAY, "the player calls the proposed play");

        // Call for call, the NSF's play makes the game's APU writes.
        let ours = game.writes_per_call(PLAY);
        let theirs = played.writes_per_call(PLAY);
        let n = ours.len().min(theirs.len());
        assert!(n >= 150, "{n} play calls compared");
        assert_eq!(ours[..n], theirs[..n], "the same tune");
        let notes = ours[..n].iter().filter(|c| !c.is_empty()).count();
        assert!(notes >= 18, "{notes} calls played a note");

        // And the player is audible.
        let audio = player.drain_audio();
        let peak = audio.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
        assert!(peak > 0.05, "peak {peak}");
    }

    #[test]
    fn a_driver_in_ram_is_refused() {
        // A play routine at $0300: code the game copied to RAM, which the
        // ROM image does not hold and the NSF player clears.
        let t = synthetic(
            &[(CallKind::Nmi, NMI), (CallKind::Jsr, 0x0300)],
            &[(1, &[0, 1])],
        );
        let p = propose(&t).expect("a proposal");
        assert_eq!(p.play.addr, 0x0300);
        let e = build_nsf(&music_rom(), &p, Region::Ntsc, &NsfInfo::default()).unwrap_err();
        assert!(
            matches!(e, RipError::Unsupported(ref m) if m.contains("runs from RAM")),
            "{e}"
        );
        let t = synthetic(
            &[
                (CallKind::Jsr, 0x6000),
                (CallKind::Nmi, NMI),
                (CallKind::Jsr, PLAY),
            ],
            &[(0, &[0]), (1, &[1, 2])],
        );
        let p = propose(&t).expect("a proposal");
        let e = build_nsf(&music_rom(), &p, Region::Ntsc, &NsfInfo::default()).unwrap_err();
        assert!(
            matches!(e, RipError::Unsupported(ref m) if m.contains("init $6000")),
            "{e}"
        );
    }

    #[test]
    fn a_bank_switched_board_is_refused() {
        let mut rom = music_rom();
        rom[6] = 0x20; // mapper 2 (UxROM)
        let mut nes = Nes::from_rom(&music_rom()).expect("loads");
        let p = propose(&trace(&mut nes, 30)).expect("a proposal");
        let e = build_nsf(&rom, &p, Region::Ntsc, &NsfInfo::default()).unwrap_err();
        assert!(
            matches!(e, RipError::Unsupported(ref m) if m.contains("mapper 2")),
            "{e}"
        );
    }

    fn synthetic(calls: &[(CallKind, u16)], writes: &[(u64, &[u32])]) -> Tracer {
        Tracer {
            stack: Vec::new(),
            calls: calls
                .iter()
                .map(|&(kind, target)| Call {
                    kind,
                    target,
                    frame: 0,
                })
                .collect(),
            writes: writes
                .iter()
                .map(|&(frame, chain)| ApuWrite {
                    frame,
                    addr: 0x4000,
                    value: 0,
                    pc: 0,
                    chain: chain.to_vec(),
                })
                .collect(),
            frames: 2,
        }
    }

    #[test]
    fn an_inline_driver_has_no_play_routine() {
        // Writes straight from the NMI handler: no subroutine encloses them.
        let t = synthetic(
            &[(CallKind::Nmi, NMI), (CallKind::Nmi, NMI)],
            &[(1, &[0]), (2, &[1])],
        );
        assert_eq!(propose(&t), Err(RipError::NoSubroutine));
        assert_eq!(propose(&synthetic(&[], &[])), Err(RipError::NoWrites));
    }

    #[test]
    fn reset_code_that_writes_inline_gets_a_no_op_init() {
        // One write with an empty chain (the reset code itself), then the
        // driver's writes under NMI -> JSR play.
        let t = synthetic(
            &[(CallKind::Nmi, NMI), (CallKind::Jsr, PLAY)],
            &[(0, &[]), (1, &[0, 1])],
        );
        let p = propose(&t).expect("a proposal");
        assert_eq!(p.play.addr, PLAY);
        assert_eq!(
            p.init,
            InitEvidence::None {
                writes_outside_play: 1
            }
        );
        let (nsf, noop) =
            build_nsf(&music_rom(), &p, Region::Ntsc, &NsfInfo::default()).expect("an NSF");
        let at = noop.expect("a no-op init").addr;
        assert_eq!(u16::from_le_bytes([nsf[0x0A], nsf[0x0B]]), at);
        assert_eq!(nsf[0x80 + usize::from(at - 0x8000)], 0x60, "init is an RTS");
    }

    #[test]
    fn init_is_the_innermost_routine_enclosing_the_setup_writes() {
        // Reset code calls a setup routine, which calls the engine's init:
        // both enclose the writes made outside play; the innermost is init.
        let t = synthetic(
            &[
                (CallKind::Jsr, 0xC100),
                (CallKind::Jsr, INIT),
                (CallKind::Nmi, NMI),
                (CallKind::Jsr, PLAY),
            ],
            &[(0, &[0, 1]), (0, &[0, 1]), (1, &[2, 3])],
        );
        let p = propose(&t).expect("a proposal");
        assert_eq!(p.play.addr, PLAY);
        assert_eq!(
            p.init,
            InitEvidence::Routine {
                addr: INIT,
                writes: 2,
                calls: 1
            }
        );
    }

    #[test]
    fn the_header_carries_the_metadata_cut_to_31_bytes() {
        let t = synthetic(&[(CallKind::Jsr, PLAY)], &[(1, &[0])]);
        let p = propose(&t).expect("a proposal");
        let info = NsfInfo {
            name: "x".repeat(40),
            artist: "Ä".repeat(20),
            copyright: String::from("2026 test"),
        };
        let (nsf, _) = build_nsf(&music_rom(), &p, Region::Pal, &info).expect("an NSF");
        assert_eq!(&nsf[0x0E..0x0E + 31], "x".repeat(31).as_bytes());
        assert_eq!(nsf[0x0E + 31], 0, "NUL-terminated");
        // 'Ä' is two bytes: 15 fit in 31, the 16th would split.
        assert_eq!(&nsf[0x2E..0x2E + 30], "Ä".repeat(15).as_bytes());
        assert_eq!(nsf[0x2E + 30], 0);
        assert_eq!(&nsf[0x4E..0x4E + 9], b"2026 test");
        assert_eq!(nsf[0x7A], 1, "PAL");
        assert_eq!(u16::from_le_bytes([nsf[0x6E], nsf[0x6F]]), 16_639);
        assert_eq!(u16::from_le_bytes([nsf[0x78], nsf[0x79]]), 19_997);
    }
}

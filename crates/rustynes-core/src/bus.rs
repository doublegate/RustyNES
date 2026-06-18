//! Lockstep bus for the `Nes` facade.
//!
//! Per `docs/scheduler.md` §Bus design: this bus owns CPU RAM, the PPU, the
//! APU, the cartridge mapper, and the controller stub. Each
//! `cpu_read`/`cpu_write` ticks the PPU exactly 3 times (NTSC) and dispatches
//! the access to the right device. PPU register reads have side effects;
//! OAM DMA and DMC DMA are handled by `cpu_cycles_owed`-style state machines
//! that drain stolen cycles before completing the access that triggered them.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::{boxed::Box, vec::Vec};

use crate::genie::{GenieCode, GenieError};
use rustynes_apu::{Apu, ApuSnapshotError, Region as ApuRegion};

/// v2.0 R1c-1 DIAGNOSTIC (gated `cpu-instr-cycle-trace`).
///
/// A per-CPU-instruction `(PC, cumulative cpu_cycle)` ring buffer (keeps the
/// LAST `CAP` instructions). `Cpu::step` calls `trace_instr` at each opcode
/// fetch; the harness dumps the ring (R1 + default) and diffs the
/// per-instruction cycle deltas to pin the odd-cycle cumulative divergence (the
/// Y=3-vs-4 source). Read via `rustynes_core::instr_trace`.
#[cfg(feature = "cpu-instr-cycle-trace")]
pub mod instr_trace {
    use core::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
    /// Ring capacity (last CAP instructions kept).
    pub const CAP: usize = 1 << 18; // 262144
    /// Per-entry instruction PC.
    pub static PC: [AtomicU32; CAP] = [const { AtomicU32::new(0) }; CAP];
    /// Per-entry cumulative CPU cycle.
    pub static CYC: [AtomicU64; CAP] = [const { AtomicU64::new(0) }; CAP];
    /// Monotonic write index (total instructions; ring slot = `IDX % CAP`).
    pub static IDX: AtomicU64 = AtomicU64::new(0);

    /// Record one instruction `(pc, cpu_cycle)` into the ring.
    #[allow(clippy::cast_possible_truncation)]
    pub fn record(pc: u16, cpu_cycle: u64) {
        let slot = (IDX.fetch_add(1, Relaxed) % CAP as u64) as usize;
        PC[slot].store(u32::from(pc), Relaxed);
        CYC[slot].store(cpu_cycle, Relaxed);
    }
}
use rustynes_cpu::Bus;
use rustynes_mappers::{Cartridge, Mapper, MapperError, MapperFrameEvents, RomError};
use rustynes_ppu::{
    BgSplitState as PpuBgSplitState, ExAttribute as PpuExAttribute, Ppu, PpuBus, PpuPalette,
    PpuRegion, PpuSnapshotError,
};

use crate::controller::{Buttons, Controller};
#[cfg(feature = "irq-timing-trace")]
use crate::irq_trace::{A12Event, BusAccess, CycleRecord, IrqTrace};
use crate::save_state::{self, SnapshotError};
use crate::scheduler::M2Phase;

/// CPU RAM (2 KiB).
const RAM_SIZE: usize = 0x0800;

/// OAM DMA source-page write target (`$4014`). Triggers a 256-byte DMA on
/// the next CPU read cycle.
const REG_OAM_DMA: u16 = 0x4014;

/// Default audio sample rate. The frontend may rebuild the bus with a
/// different rate when CPAL picks something else.
pub const DEFAULT_SAMPLE_RATE: u32 = 44_100;

/// Map the cartridge-layer [`rustynes_mappers::VsPpuPalette`] to the PPU's
/// [`PpuPalette`]. `rustynes-core` is the one crate that depends on both `rustynes-ppu`
/// and `rustynes-mappers`, so the bridge lives here rather than creating a
/// cross-crate dependency edge.
const fn vs_palette_to_ppu(p: rustynes_mappers::VsPpuPalette) -> PpuPalette {
    match p {
        rustynes_mappers::VsPpuPalette::Composite2C02 => PpuPalette::Composite2C02,
        rustynes_mappers::VsPpuPalette::Rgb2C03 => PpuPalette::Rgb2C03,
        rustynes_mappers::VsPpuPalette::Rgb2C04_0001 => PpuPalette::Rgb2C04_0001,
        rustynes_mappers::VsPpuPalette::Rgb2C04_0002 => PpuPalette::Rgb2C04_0002,
        rustynes_mappers::VsPpuPalette::Rgb2C04_0003 => PpuPalette::Rgb2C04_0003,
        rustynes_mappers::VsPpuPalette::Rgb2C04_0004 => PpuPalette::Rgb2C04_0004,
        rustynes_mappers::VsPpuPalette::Rgb2C05 => PpuPalette::Rgb2C05,
    }
}

/// Initial reset state for the bus.
fn fresh_ram() -> Box<[u8; RAM_SIZE]> {
    // Deterministic seeded fill — for now zero, matching most emulators'
    // "post-power-on" approximation.
    Box::new([0u8; RAM_SIZE])
}

/// v1.1.0 beta.2 (Workstream C, T-110-C3) — the class of a captured CPU write.
///
/// One per event-viewer timeline entry: PPU `$2000-$3FFF`, APU `$4000-$4017`,
/// or mapper `$4020-$FFFF`, tagged (in [`EventRec`]) with the PPU position at
/// the moment of the write.
#[cfg(feature = "debug-hooks")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventKind {
    /// A `$2000-$3FFF` PPU-register write.
    PpuWrite,
    /// A `$4000-$4017` APU / I/O-register write.
    ApuWrite,
    /// A `$4020-$FFFF` mapper-register write.
    MapperWrite,
    /// A `$2000-$3FFF` PPU-register read (v1.5.0 Workstream A2 — the graphical
    /// PPU Event Viewer draws reads as well as writes, so the read/write heatmap
    /// + the register-access table can show both directions).
    PpuRead,
}

#[cfg(feature = "debug-hooks")]
impl EventKind {
    /// Whether this event is a CPU read (vs a write). Used by the v1.5.0 PPU
    /// Event Viewer heatmap to colour reads (blue) vs writes (red).
    #[must_use]
    pub const fn is_read(self) -> bool {
        matches!(self, Self::PpuRead)
    }
}

/// One event-viewer record: kind + the PPU `(scanline, dot)` + the address +
/// (v1.5.0 A2) the byte read or written.
#[cfg(feature = "debug-hooks")]
#[derive(Clone, Copy, Debug)]
pub struct EventRec {
    /// What happened.
    pub kind: EventKind,
    /// PPU scanline at the event (`-1` = pre-render, `0..=239` visible, ...).
    pub scanline: i16,
    /// PPU dot (`0..=340`).
    pub dot: u16,
    /// The accessed address.
    pub addr: u16,
    /// The byte written, or the byte the read returned (v1.5.0 Workstream A2).
    pub value: u8,
}

/// Max events captured per frame (bounded so a write-heavy frame can't grow the
/// log without limit; a frame has at most a few thousand CPU writes).
#[cfg(feature = "debug-hooks")]
const EVENT_CAP: usize = 20_000;

/// v1.1.0 beta.3 (Workstream E, T-110-E2) — one CPU bus-access record for the
/// Lua `onRead` / `onWrite` callbacks: direction + full address + the byte.
///
/// Distinct from [`EventRec`] (which is the scanline/dot-oriented event-viewer
/// record): this captures *every* CPU read and write across the whole address
/// space, with the value, so a script can react to a specific access. Output-
/// only and gated behind `access_logging`; the host (Lua engine) enables it
/// only while `onRead`/`onWrite` callbacks are registered.
#[cfg(feature = "debug-hooks")]
#[derive(Clone, Copy, Debug)]
pub struct AccessRec {
    /// `true` for a CPU write, `false` for a CPU read.
    pub write: bool,
    /// The accessed CPU address (`$0000-$FFFF`).
    pub addr: u16,
    /// The byte written, or the byte the read returned.
    pub value: u8,
}

/// Max bus accesses captured per frame. A frame issues on the order of 30k CPU
/// cycles; this caps the worst case so a tight loop can't grow the log
/// unbounded. A frame that overflows the cap is truncated (the tail is dropped).
#[cfg(feature = "debug-hooks")]
const ACCESS_CAP: usize = 60_000;

/// v1.2.0 (Workstream E, T-110-E1) — one interrupt-service record for the Lua
/// `onNmi` / `onIrq` callbacks: the service direction + the vector the CPU
/// fetched its new PC from.
///
/// Captured at the commit point — [`Bus::notify_irq_service`], called once per
/// real interrupt entry right before the CPU reads the service vector. This is
/// the *committed* service (the same point the IRQ trace records), NOT the
/// speculative `poll_nmi` / `poll_irq` sampler that ADR 0010 flagged as
/// unreliable — so a script that watches `onNmi`/`onIrq` sees exactly the
/// interrupts the CPU actually serviced this frame, in order. Output-only and
/// gated behind `interrupt_logging`; the host (Lua engine) enables it only
/// while `onNmi`/`onIrq` callbacks are registered.
#[cfg(feature = "debug-hooks")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterruptRec {
    /// `true` for an NMI service entry (`$FFFA`), `false` for an IRQ/BRK
    /// service entry (`$FFFE`).
    pub is_nmi: bool,
    /// The service vector the CPU fetched its new PC from (`$FFFA` for an NMI,
    /// `$FFFE` for IRQ/BRK).
    pub vector: u16,
}

/// Max interrupt-service records captured per frame. A frame services at most a
/// few hundred interrupts (NMI once + mapper/APU IRQs); this caps a pathological
/// case so the log can't grow unbounded. A frame that overflows is truncated.
#[cfg(feature = "debug-hooks")]
const INTERRUPT_CAP: usize = 4_096;

/// v1.4.0 Workstream D (D2) — the class of hardware event an event-driven
/// breakpoint can trigger on.
///
/// These are tapped at the SAME observational commit points the event-viewer /
/// interrupt-service / bus-access logs already use (`Bus::cpu_read`,
/// `Bus::cpu_write`, `Bus::notify_irq_service`, the DMC-DMA GET, the `$4014`
/// write). A hit only RECORDS the event (kind + PPU position); it never mutates
/// emulator-visible state, so the determinism contract holds and the
/// feature-off build is byte-identical.
///
/// The 16 categories are packed into a `u16` arm mask (see
/// [`LockstepBus::set_event_breakpoints`]); the bit index is the discriminant.
#[cfg(feature = "debug-hooks")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum EventBpKind {
    /// An NMI service entry (`$FFFA`), observed at the interrupt-service commit.
    Nmi = 0,
    /// An IRQ / BRK service entry (`$FFFE`), observed at the same commit.
    Irq = 1,
    /// A sprite-0 hit, observed when the CPU reads `$2002` with bit 6 set (the
    /// point games actually detect the hit; purely observational).
    Sprite0Hit = 2,
    /// An OAM DMA, observed at the `$4014` write that starts it.
    OamDma = 3,
    /// A DMC DMA sample fetch (the GET cycle).
    DmcDma = 4,
    /// A PPU-register read (`$2000-$3FFF`).
    PpuRead = 5,
    /// A PPU-register write (`$2000-$3FFF`).
    PpuWrite = 6,
    /// An APU / I/O-register read (`$4000-$4017`).
    ApuRead = 7,
    /// An APU / I/O-register write (`$4000-$4017`).
    ApuWrite = 8,
    /// A mapper-register read (`$4020-$FFFF`).
    MapperRead = 9,
    /// A mapper-register write (`$4020-$FFFF`).
    MapperWrite = 10,
}

#[cfg(feature = "debug-hooks")]
impl EventBpKind {
    /// The arm-mask bit for this kind.
    #[must_use]
    pub const fn bit(self) -> u16 {
        1u16 << (self as u8)
    }

    /// A human-readable label (used by the debugger UI + tests).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Nmi => "NMI entry",
            Self::Irq => "IRQ entry",
            Self::Sprite0Hit => "Sprite-0 hit",
            Self::OamDma => "OAM DMA",
            Self::DmcDma => "DMC DMA",
            Self::PpuRead => "PPU read",
            Self::PpuWrite => "PPU write",
            Self::ApuRead => "APU read",
            Self::ApuWrite => "APU write",
            Self::MapperRead => "Mapper read",
            Self::MapperWrite => "Mapper write",
        }
    }

    /// All categories, in discriminant order (for the UI checkbox list).
    #[must_use]
    pub const fn all() -> [Self; 11] {
        [
            Self::Nmi,
            Self::Irq,
            Self::Sprite0Hit,
            Self::OamDma,
            Self::DmcDma,
            Self::PpuRead,
            Self::PpuWrite,
            Self::ApuRead,
            Self::ApuWrite,
            Self::MapperRead,
            Self::MapperWrite,
        ]
    }
}

/// v1.4.0 Workstream D (D2) — one event-driven breakpoint hit.
///
/// Carries the kind, the associated address (`0` for the interrupt entries that
/// carry none), and the full timing context (frame / CPU cycle / PPU
/// scanline+dot) at the moment of the event. Recorded by the first armed-event
/// tap of a frame; the frontend takes it via
/// [`crate::Nes::take_event_break_hit`] to pause + report.
#[cfg(feature = "debug-hooks")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventBreakHit {
    /// Which event fired.
    pub kind: EventBpKind,
    /// The associated CPU address (the read/write address, the OAM-DMA `$4014`,
    /// the DMC sample address, or the service vector for NMI/IRQ).
    pub addr: u16,
    /// PPU frame counter at the event.
    pub frame: u64,
    /// Cumulative CPU cycle at the event.
    pub cycle: u64,
    /// PPU scanline (`-1` pre-render .. `260`).
    pub scanline: i16,
    /// PPU dot (`0..=340`).
    pub dot: u16,
}

/// Lockstep bus.
///
/// Owns the entire emulator's mutable state. The CPU borrows `&mut LockstepBus`
/// during `Cpu::step`. The PPU and APU are ticked from the bus's
/// `cpu_read`/`cpu_write` implementations (3 dots per CPU cycle, NTSC; APU
/// every CPU cycle).
// The per-phase IRQ snapshots (Phase B2 of the C1 IRQ-timing rework) add
// 4 bools beyond the original 3 (last_nmi_level / nmi_edge_latch /
// in_dmc_dma), plus another `trace_last_a12` when the trace feature is
// on. They're independent state words, not a single enum-modelled
// machine — silencing the lint is the right call.
#[allow(clippy::struct_excessive_bools)]
pub struct LockstepBus {
    /// CPU RAM (2 KiB), mirrored every 0x800 bytes from `$0000-$1FFF`.
    pub(crate) ram: Box<[u8; RAM_SIZE]>,
    /// PPU instance.
    pub(crate) ppu: Ppu,
    /// APU instance.
    pub(crate) apu: Apu,
    /// Cartridge metadata (kept for save-state and debugger).
    #[allow(dead_code)]
    pub(crate) cart: Cartridge,
    /// Boxed mapper.
    pub(crate) mapper: Box<dyn Mapper>,
    /// v2.8.0 Phase 4 — the mapper's capability flags, cached at
    /// construction (and refreshed when [`Self::power_cycle`] rebuilds the
    /// mapper) so the per-CPU-cycle hot loop can skip the up-to-four
    /// virtual dispatches (`notify_cpu_cycle` / `mix_audio` /
    /// `notify_frame_event` / `irq_pending`) on boards that don't use
    /// them. Constant per mapper type; NOT part of the save-state.
    mapper_caps: rustynes_mappers::MapperCaps,
    /// The original iNES/NES-2.0 ROM bytes, kept so [`Self::power_cycle`] can
    /// rebuild the mapper to a true power-on state (fresh bank registers,
    /// cleared CHR-RAM + volatile PRG-RAM). `None` on the FDS path (which has
    /// no iNES image; FDS netplay is unsupported). NOT part of the save-state
    /// (constant; the encoder skips it).
    rom_bytes: Option<Box<[u8]>>,
    /// Standard NES controllers (player 1 on `$4016`, player 2 on `$4017`).
    pub(crate) controllers: [Controller; 2],
    /// Four Score 4-player adapter. When `true`, `$4016`/`$4017` multiplex
    /// four controllers + an adapter signature over a 24-read serial sequence
    /// (nesdev "Four score"; matches `Mesen2` / `TetaNES`). When `false` (default)
    /// the read path is byte-identical to the standard two-controller
    /// behavior, so the determinism contract and existing save-states are
    /// unaffected.
    four_score: bool,
    /// Players 3 (`$4016`) and 4 (`$4017`) — only polled when
    /// [`Self::four_score`] is set.
    controllers34: [Controller; 2],
    /// Per-port Four Score read counter (0-7 = primary pad, 8-15 = secondary
    /// pad, 16-23 = signature, then 1s). Reset on each strobe.
    four_score_idx: [u8; 2],
    /// Per-port Four Score signature shift register, reloaded on each strobe
    /// (port 0 = `0x08`, port 1 = `0x04`; shifted out LSB-first).
    four_score_sig: [u8; 2],
    /// Output-only `TAStudio` lag-log flag (v1.6.0 Workstream A3): set `true`
    /// whenever the running program reads a controller port (`$4016`/`$4017`)
    /// during the current frame; cleared at the top of each
    /// [`crate::Nes::run_frame`]. A frame still `false` at frame end is a "lag
    /// frame" (the game polled no input that frame). `debug-hooks`-gated and
    /// never read back into emulation, so the shipped build stays byte-identical
    /// and the determinism contract is unaffected.
    #[cfg(feature = "debug-hooks")]
    controller_polled: bool,
    /// Vs. System DIP switches (8 bits, switch 1 = bit 0 .. switch 8 = bit 7).
    /// Read through the upper bits of `$4016`/`$4017` per the Vs. protocol
    /// (nesdev "Vs. System"). Only consulted when the cart is
    /// [`rustynes_mappers::ConsoleType::VsSystem`]; on a standard NES cart the
    /// `$4016`/`$4017` read path is byte-identical regardless of this value.
    vs_dip: u8,
    /// Vs. System coin-acceptor state: bit 0 = acceptor #1 ($4016 bit 5),
    /// bit 1 = acceptor #2 ($4016 bit 6). A real coin pulse reads true for
    /// ~40-70 ms; the frontend latches it for a configurable number of frames
    /// via [`LockstepBus::insert_coin`] and clears it with
    /// [`LockstepBus::clear_coin`]. Vs.-System carts only.
    vs_coin: u8,
    /// Vs. System service button ($4016 bit 2). Vs.-System carts only.
    vs_service: bool,
    /// Optional non-standard input-device overlay per port (`$4016`/`$4017`).
    /// When a port has `Some(device)`, [`Self::read_port`] returns that
    /// device's byte instead of the standard controller / Four Score serial
    /// byte. `None` (the default) leaves the existing path byte-identical, so
    /// the default + Four Score reads and the determinism contract are
    /// unaffected unless a device is explicitly attached.
    expansion_device: [Option<crate::input_device::InputDevice>; 2],
    /// v1.1.0 beta.1 (T-110-B4) — optional per-game nametable mirroring
    /// override. `None` (default) defers to the mapper's `nametable_address`
    /// (byte-identical). When `Some`, the standard `$2000-$3EFF` nametable
    /// translation uses this mirroring instead — a load-time correction for
    /// ROMs with a wrong iNES mirroring flag, supplied by the frontend's game
    /// database. Does NOT affect mapper-supplied VRAM (`nametable_fetch`, e.g.
    /// 4-screen). Persisted in the save-state so rollback / restore stay
    /// consistent. The core test suites never set it, so `AccuracyCoin` / the
    /// oracle are unaffected.
    nt_mirroring_override: Option<rustynes_mappers::Mirroring>,
    /// v1.1.0 beta.2 (T-110-C3) — event-viewer log (this frame's CPU-write
    /// events). Output-only; populated only while `event_logging`, cleared per
    /// frame. Gated on `debug-hooks` so the default hot path is untouched.
    #[cfg(feature = "debug-hooks")]
    events: alloc::vec::Vec<EventRec>,
    /// Whether the event viewer is recording. Default `false`.
    #[cfg(feature = "debug-hooks")]
    event_logging: bool,
    /// v1.1.0 beta.3 (T-110-E2) — full CPU bus-access log (reads + writes +
    /// values) for the Lua `onRead`/`onWrite` callbacks. Output-only; populated
    /// only while `access_logging`, cleared per frame.
    #[cfg(feature = "debug-hooks")]
    accesses: alloc::vec::Vec<AccessRec>,
    /// Whether the bus-access log is recording. Default `false`.
    #[cfg(feature = "debug-hooks")]
    access_logging: bool,
    /// v1.2.0 (T-110-E1) — per-frame interrupt-service log (this frame's
    /// committed NMI / IRQ / BRK service entries) for the Lua `onNmi`/`onIrq`
    /// callbacks. Output-only; populated only while `interrupt_logging`, cleared
    /// per frame.
    #[cfg(feature = "debug-hooks")]
    interrupts: alloc::vec::Vec<InterruptRec>,
    /// Whether the interrupt-service log is recording. Default `false`.
    #[cfg(feature = "debug-hooks")]
    interrupt_logging: bool,
    /// v1.4.0 Workstream D (D2) — armed event-breakpoint categories, packed as a
    /// bitmask of [`EventBpKind::bit`]. `0` (default) disarms every category, so
    /// the per-access tap is a single `mask == 0` early-out — the default + the
    /// feature-off build are byte-identical and pay no per-cycle cost. Output-
    /// only: a hit records [`Self::event_break_hit`] but never mutates state.
    #[cfg(feature = "debug-hooks")]
    event_bp_mask: u16,
    /// The first event-breakpoint hit of the current frame (`None` until one
    /// fires). Recorded by the taps, taken by the frontend after `run_frame`.
    #[cfg(feature = "debug-hooks")]
    event_break_hit: Option<EventBreakHit>,
    /// Cumulative CPU cycle counter.
    pub(crate) cycle: u64,

    /// OAM DMA pending source page (set by `$4014` write; consumed on the
    /// next `cpu_read`/`cpu_write`).
    dma_pending: Option<u8>,
    /// Cycles owed to the OAM DMA before the original access can complete.
    dma_cycles_owed: u32,
    /// OAM DMA scratch byte: read on even cycles, written on odd cycles.
    dma_byte: u8,
    /// OAM DMA progress index (0..256).
    dma_idx: u16,
    /// OAM DMA active source page (latched from `dma_pending`).
    dma_page: u8,
    /// CPU read address that OAM DMA halted. While the CPU is halted,
    /// no-op DMA cycles keep this address on the 6502 core bus.
    dma_halt_addr: u16,
    /// Stage-D (`mc-r1-full-cpu`): the OAM DMA's original total cycle count
    /// (513 or 514) latched at set-up, so the CPU-driven per-cycle
    /// `oam_dma_step` can recompute `consumed = dma_total - dma_cycles_owed`
    /// and the alignment across calls. 0 when no OAM DMA is in flight.
    dma_total: u32,

    /// Edge-detector latch for the PPU NMI line, used by `poll_nmi`.
    last_nmi_level: bool,
    /// Latched NMI edge (consumed by `poll_nmi`).
    nmi_edge_latch: bool,

    /// v2.0 master-clock R1 substrate (Phase 1): PPU progress in master-clock
    /// units, consumed by `run_ppu_to(target)` (ticks a dot while
    /// `ppu_clock + ppu_divider <= target`). Only used under the R1 CPU loop.
    ppu_clock: u64,
    /// v2.0 master-clock R1 substrate: the cartridge region's `(cpu_divider,
    /// ppu_divider)` in master clocks (NTSC 12/4, PAL 16/5, Dendy 15/5),
    /// computed once at construction. The region never changes after power-on,
    /// so caching these removes the per-CPU-cycle `match self.cart.region` from
    /// the hottest R1 paths (`cpu_divider`, `run_ppu_to`). Behaviour-identical:
    /// the value equals what the prior `region_dividers()` match returned.
    cpu_div_cached: u8,
    ppu_div_cached: u8,
    /// v2.0 master-clock R1 substrate (Phase 1): master clocks consumed by
    /// bus-side DMA cycles since the CPU last drained the accumulator (folded
    /// into `Cpu::master_clock` in `end_cycle` to keep the CPU<->PPU phase
    /// coherent across a DMA span). Drained by `take_dma_mc_consumed`.
    dma_mc_consumed: u64,

    /// External CPU data bus latch: last value driven onto the bus
    /// by ANY device (CPU, DMC DMA, OAM DMA conflict reads).
    ///
    /// This is the classic "open bus" floating-latch value that NES
    /// emulation refers to.  Reads from unmapped or open-bus regions
    /// return this value; the upper 3 bits of the controller-strobe
    /// register reads (`$4016` / `$4017`) bleed through from this
    /// latch.  DMC DMA fetches update this latch (because the DMC
    /// drives the external bus during halt).
    open_bus: u8,
    /// Internal CPU data bus latch: last value driven onto the bus
    /// by a CPU-initiated read or write.
    ///
    /// The 2A03 silicon has two distinct data buses.  The
    /// **internal** bus is driven only by CPU operations (instruction
    /// fetch, operand read, ALU result, write).  DMC DMA fetches
    /// drive only the **external** bus (`open_bus` above) — the
    /// internal bus retains its prior value across a DMC halt.  This
    /// distinction is invisible while the CPU runs unimpeded (the
    /// two buses carry the same value), but it surfaces on the SH*
    /// unstable-store family when DMC DMA interleaves with the
    /// store's address-high-byte AND computation, and on the `$4015`
    /// bit-5 open-bus read after a DMC DMA fetch.
    ///
    /// Phase 1 of the v1.0.0-final `linked-puzzling-sutherland`
    /// brief (`to-dos/phase-6-v1.0.0-final/sprint-6-sh-unstable-stores.md`).
    /// Mirrored from every `cpu_read` / `cpu_write` path; explicitly
    /// NOT updated by `dmc_dma_read` (the DMC fetch path).
    internal_data_bus: u8,

    /// Most recent CPU bus access — used by the 2A03 DMC-DMA readout-bug
    /// emulation. (Address only; some bug variants need the address, the
    /// bus value is the open-bus latch above.)
    last_read_addr: u16,
    /// Side-effect register read whose absolute high-byte operand was
    /// halted by DMC DMA one CPU read before the actual register access.
    deferred_dma_replay_addr: u16,
    /// True while we're servicing a DMC DMA fetch — used to suppress
    /// recursion / re-entrancy when the DMA controller invokes `raw_cpu_read`.
    in_dmc_dma: bool,
    /// v2.0 interleaved-DMA Phase B (`mc-r1-substrate`): the `TriCNES`
    /// `DMCDMA_Halt` flag — set when the interleaved DMC DMA starts, cleared
    /// after a GET cycle. Gates whether the current get cycle is the halt
    /// re-read or the actual sample fetch. Only used by `dmc_dma_step`.
    dmc_halt: bool,
    /// Program M (M-2, `mc-r1-dmc-oam-overlap`): whether the most recent
    /// `dmc_dma_step` performed the GET (vs a halt/dummy/align). Read by the
    /// read1 overlap loop to decide whether the DMC cycle can share an OAM cycle.
    dmc_step_was_get: bool,
    /// W3-Stage-1 (`mc-r1-dma-unified`): the unified engine's OAM-DMA-active
    /// flag (`TriCNES` `DoOAMDMA` once latched). The 513/514 length is EMERGENT
    /// from `uni_oam_halt`/`uni_oam_aligned` + the per-cycle dispatch — no
    /// owed-cycle counter.
    uni_oam_active: bool,
    /// W3-Stage-1: `TriCNES` `OAMDMA_Halt` — set when the OAM DMA's FIRST
    /// serviced cycle lands on the OAM engine's read half (at floor parity:
    /// `put_cycle == true`, the floor's `self.cycle & 1 == 0` -> 514 case);
    /// cleared at the end of every OAM-read-half cycle.
    uni_oam_halt: bool,
    /// W3-Stage-1: `TriCNES` `OAMDMA_Aligned` — set by the OAM read, consumed
    /// by the OAM write; force-cleared by a DMC GET (the emergent post-GET
    /// realign: the next write half becomes an alignment dummy and the byte
    /// is re-read).
    uni_oam_aligned: bool,
    /// W3-Stage-1: `TriCNES` `DMAAddress` — the OAM byte index (0..=255;
    /// reaching 256 on a write completes the DMA). Only increments on writes,
    /// so a DMC-GET-stalled byte is re-read.
    uni_oam_addr: u16,

    /// Active Game Genie codes, keyed by the PRG address they patch
    /// (`$8000-$FFFF`). Applied on the CPU read path; empty by default, so
    /// with no codes active reads are byte-identical to a build without the
    /// feature (the determinism contract is preserved). NOT part of the
    /// save-state — codes are a user overlay persisted by the frontend, not
    /// emulation state. See [`crate::genie`].
    genie_codes: BTreeMap<u16, GenieCode>,

    /// Which half of the current CPU cycle the lockstep scheduler is in.
    /// See [`M2Phase`] for the convention; see
    /// [`LockstepBus::current_m2_phase`] for the read accessor.
    ///
    /// Maintained by `tick_one_cpu_cycle`: enters each cycle at
    /// [`M2Phase::Low`], transitions to [`M2Phase::High`] after sub-dot
    /// 1 of the 3-PPU-dot tick loop (the M2-rising boundary), then
    /// resets to [`M2Phase::Low`] at end-of-cycle.  As of Phase B2 of
    /// the C1 IRQ-timing rework this is still informational; the
    /// production [`Bus::poll_irq_at_phase`] path reads from the
    /// `irq_snapshot_*` fields below.
    m2_phase: M2Phase,

    /// Deferred controller strobe write (Session-24 / Phase 3 of the
    /// v1.0.0-final brief).  Mirrors Mesen2's `NesControlManager`
    /// `_writeAddr` / `_writeValue` / `_writePending` triplet (see
    /// `Core/NES/NesControlManager.cpp` lines 252-273): a CPU write to
    /// `$4016` (or `$4017`) does NOT directly update the controllers'
    /// strobe state.  Instead the write is buffered here.
    /// `controller_write_pending` is set to 1 (odd-cycle write) or 2
    /// (even-cycle write) at the moment of the CPU write, then
    /// decremented every CPU cycle at the START of `tick_one_cpu_cycle`
    /// (BEFORE the 3-dot PPU loop runs); when it reaches 0 the buffered
    /// value is committed to `Controller::write_strobe`.  Multiple
    /// writes within the commit window collapse — the latest value
    /// wins (the buffer is single-slot, the previous value is
    /// silently overwritten).
    ///
    /// This is the load-bearing structural change for `AccuracyCoin`
    /// `Controller Strobing` Test 4 (a 1-cycle DEC `$4016` strobe pulse
    /// whose 0→1→0 sequence must NOT fire the latch when it happens
    /// to span an L→H half-cycle pair — under deferred commit both
    /// writes target the SAME commit cycle, the second overwrites the
    /// first, no edge is observed).  See
    /// `docs/audit/session-24-phase3-controller-strobing-2026-05-23.md`.
    controller_write_pending: u8,
    /// Buffered controller-write value (latched at the moment of the
    /// CPU write; committed when `controller_write_pending` reaches 0).
    controller_write_value: u8,

    /// Mapper-side IRQ line snapshotted at the conventional M2-low
    /// boundary of the current CPU cycle (between PPU sub-dot 0 and
    /// sub-dot 1, per the [`M2Phase`] convention).  Updated by
    /// [`LockstepBus::tick_one_cpu_cycle`] every cycle; read by
    /// [`Bus::poll_irq_at_phase`] when `phase == M2Phase::Low`.
    ///
    /// Phase B2 of the C1 IRQ-timing rework: the storage is
    /// unconditional (not gated on the `irq-timing-trace` feature) so
    /// it's available on every build of the bus.  Per Phase A's
    /// empirical finding the M2-low and M2-high values are byte-
    /// identical for every baseline trace ROM, but the storage is kept
    /// separate so Phase B4's MMC3 sub_dot-aware A12 filter can change
    /// the two halves' values independently.
    irq_snapshot_mapper_at_low: bool,
    /// APU-side IRQ line snapshotted at the conventional M2-low
    /// boundary.  See [`Self::irq_snapshot_mapper_at_low`].
    irq_snapshot_apu_at_low: bool,
    /// Mapper-side IRQ line snapshotted at the conventional M2-high
    /// boundary of the current CPU cycle.  The exact intra-cycle
    /// position is "between the end of PPU sub-dot 2 and the call to
    /// `mapper.notify_cpu_cycle`" — i.e. the historical query point the
    /// pre-Phase-B2 `Bus::poll_irq` impl used when called from
    /// `Cpu::idle_tick` after `bus.on_cpu_cycle()` returned.
    ///
    /// Read by [`Bus::poll_irq`] and by
    /// [`Bus::poll_irq_at_phase`] when `phase == M2Phase::High`.
    irq_snapshot_mapper_at_high: bool,
    /// APU-side IRQ line snapshotted at the conventional M2-high
    /// boundary.  See [`Self::irq_snapshot_mapper_at_high`].
    irq_snapshot_apu_at_high: bool,

    /// Optional IRQ-timing trace buffer (Track C1 pre-work, gated on the
    /// `irq-timing-trace` cargo feature). See `crates/rustynes-core/src/irq_trace.rs`
    /// and ADR-0002 "Decision (revised, 2026-05-13)".
    #[cfg(feature = "irq-timing-trace")]
    pub(crate) irq_trace: Option<IrqTrace>,
    /// Scratch latch the `PpuBusAdapter` writes when the mapper sees an
    /// `notify_a12` call.  Polled between every PPU sub-dot tick inside
    /// `tick_one_cpu_cycle` and drained into the current cycle record's
    /// `a12_events`.  Only populated when the trace feature is on.
    #[cfg(feature = "irq-timing-trace")]
    pub(crate) trace_a12_latest: Option<bool>,
    /// Last A12 level seen across cycle boundaries; used to filter out
    /// "no transition" sub-dots so the trace records only the actual
    /// rising / falling edges.
    #[cfg(feature = "irq-timing-trace")]
    pub(crate) trace_last_a12: bool,
    /// Scratch buffer for A12 events accumulated during a single CPU
    /// cycle's 3 PPU dots.  Drained into the trace record at end-of-cycle.
    #[cfg(feature = "irq-timing-trace")]
    pub(crate) trace_a12_scratch: alloc::vec::Vec<A12Event>,
    /// Session-21 (Sprint 1 iteration 2 prereq) bus-access tracker.
    ///
    /// Set by `cpu_read` / `cpu_write` / the DMC DMA service path / the
    /// OAM DMA service path BEFORE `tick_one_cpu_cycle` records the
    /// per-cycle bus-access columns; consumed (and reset to
    /// `BusAccess::Idle` / 0) inside `tick_one_cpu_cycle` after the
    /// record is pushed.  A single CPU cycle has at most one external
    /// bus access — burn cycles (`idle_tick`) leave the tracker at
    /// `BusAccess::Idle`, which is the correct semantics for the trace
    /// (CPU internal cycles do not drive the bus).
    ///
    /// The DMA paths set this directly because the bus owns the cycle
    /// during DMA halt and the CPU's `cpu_read` / `cpu_write` is not
    /// invoked (the bus's `raw_cpu_read` is invoked instead, which
    /// does not advance time on its own — `tick_one_cpu_cycle` is
    /// called separately).
    #[cfg(feature = "irq-timing-trace")]
    pub(crate) trace_bus_access: BusAccess,
    #[cfg(feature = "irq-timing-trace")]
    pub(crate) trace_bus_addr: u16,
    #[cfg(feature = "irq-timing-trace")]
    pub(crate) trace_bus_data: u8,
    /// PC of the instruction currently executing, latched by the
    /// `trace_instr` hook (`cpu-instr-cycle-trace`). Copied into each
    /// `CycleRecord.pc` so the per-cycle trace can be diffed against
    /// `TriCNES` by ROM PC. Stays at the halted instruction's PC across
    /// DMA-insertion cycles. `0` unless `cpu-instr-cycle-trace` is on.
    #[cfg(feature = "irq-timing-trace")]
    pub(crate) trace_last_pc: u16,
    /// R1-path PPU position captured at cycle-start (`cpu_clock`) for the
    /// `trace_end_cycle` diagnostic push (the R1 loop bypasses
    /// `tick_one_cpu_cycle`'s own snapshot).
    #[cfg(feature = "irq-timing-trace")]
    pub(crate) trace_r1_scanline_start: i16,
    #[cfg(feature = "irq-timing-trace")]
    pub(crate) trace_r1_dot_start: u16,
    #[cfg(feature = "irq-timing-trace")]
    pub(crate) trace_r1_frame_start: u64,
}

impl LockstepBus {
    /// Construct from a parsed ROM with a default 44.1 kHz audio sample rate.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`RomError`] if the bytes don't parse.
    pub fn new(rom_bytes: &[u8]) -> Result<Self, RomError> {
        Self::with_sample_rate(rom_bytes, DEFAULT_SAMPLE_RATE)
    }

    /// Construct with an explicit audio sample rate.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`RomError`] if the bytes don't parse.
    // The struct-literal init grows with every feature-gated field; the W3
    // unified-engine fields pushed it past the line gate.
    #[allow(clippy::too_many_lines)]
    pub fn with_sample_rate(rom_bytes: &[u8], sample_rate: u32) -> Result<Self, RomError> {
        let (cart, mapper) = rustynes_mappers::parse(rom_bytes)?;
        let mut bus = Self::from_cart_and_mapper(cart, mapper, sample_rate);
        // Keep the iNES bytes so `power_cycle` can rebuild the mapper to a true
        // power-on state. Cheap relative to the cart it already holds, and never
        // serialized into the save-state.
        bus.rom_bytes = Some(Box::from(rom_bytes));
        Ok(bus)
    }

    /// Construct a bus directly from an already-parsed cartridge + boxed mapper.
    ///
    /// This is the shared core of [`Self::with_sample_rate`] (iNES / NES 2.0
    /// path) and [`Self::with_disk`] (Famicom Disk System path). Both produce a
    /// [`Cartridge`] metadata value plus a `Box<dyn Mapper>`; this routine wires
    /// up the PPU/APU region, the R1 master-clock dividers, and the rest of the
    /// bus state identically for both.
    // The struct-literal init grows with every feature-gated field; the W3
    // unified-engine fields pushed it past the line gate.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn from_cart_and_mapper(
        cart: Cartridge,
        mapper: Box<dyn Mapper>,
        sample_rate: u32,
    ) -> Self {
        let region = match cart.region {
            rustynes_mappers::Region::Pal => PpuRegion::Pal,
            rustynes_mappers::Region::Dendy => PpuRegion::Dendy,
            _ => PpuRegion::Ntsc,
        };
        let apu_region = match cart.region {
            rustynes_mappers::Region::Pal => ApuRegion::Pal,
            rustynes_mappers::Region::Dendy => ApuRegion::Dendy,
            _ => ApuRegion::Ntsc,
        };
        // R1 master-clock dividers, cached once (region is immutable after parse).
        // Identical to the prior `region_dividers()` match: NTSC 12/4, PAL 16/5,
        // Dendy 15/5.
        let (cpu_div_cached, ppu_div_cached): (u8, u8) = match cart.region {
            rustynes_mappers::Region::Pal => (16, 5),
            rustynes_mappers::Region::Dendy => (15, 5),
            _ => (12, 4),
        };
        // v2.8.0 Phase 4 — cache the capability flags once (constant per
        // mapper type); the per-cycle hot loop reads the copy.
        let mapper_caps = mapper.caps();
        let mut bus = Self {
            ram: fresh_ram(),
            ppu: Ppu::new(region),
            apu: Apu::new(apu_region, sample_rate),
            cart,
            mapper,
            mapper_caps,
            // Set by `with_sample_rate` (iNES path); stays `None` for FDS.
            rom_bytes: None,
            controllers: [Controller::new(); 2],
            four_score: false,
            controllers34: [Controller::new(); 2],
            four_score_idx: [0; 2],
            four_score_sig: [0; 2],
            #[cfg(feature = "debug-hooks")]
            controller_polled: false,
            vs_dip: 0,
            vs_coin: 0,
            vs_service: false,
            expansion_device: [None, None],
            nt_mirroring_override: None,
            #[cfg(feature = "debug-hooks")]
            events: alloc::vec::Vec::new(),
            #[cfg(feature = "debug-hooks")]
            event_logging: false,
            #[cfg(feature = "debug-hooks")]
            accesses: alloc::vec::Vec::new(),
            #[cfg(feature = "debug-hooks")]
            access_logging: false,
            #[cfg(feature = "debug-hooks")]
            interrupts: alloc::vec::Vec::new(),
            #[cfg(feature = "debug-hooks")]
            interrupt_logging: false,
            #[cfg(feature = "debug-hooks")]
            event_bp_mask: 0,
            #[cfg(feature = "debug-hooks")]
            event_break_hit: None,
            cycle: 0,
            dma_pending: None,
            dma_cycles_owed: 0,
            dma_byte: 0,
            dma_idx: 0,
            dma_page: 0,
            dma_halt_addr: 0,
            dma_total: 0,
            last_nmi_level: false,
            nmi_edge_latch: false,
            ppu_clock: 0,
            cpu_div_cached,
            ppu_div_cached,
            dma_mc_consumed: 0,
            open_bus: 0,
            internal_data_bus: 0,
            last_read_addr: 0,
            deferred_dma_replay_addr: 0,
            in_dmc_dma: false,
            dmc_step_was_get: false,
            uni_oam_active: false,
            uni_oam_halt: false,
            uni_oam_aligned: false,
            uni_oam_addr: 0,
            dmc_halt: false,
            genie_codes: BTreeMap::new(),
            m2_phase: M2Phase::Low,
            irq_snapshot_mapper_at_low: false,
            irq_snapshot_apu_at_low: false,
            irq_snapshot_mapper_at_high: false,
            irq_snapshot_apu_at_high: false,
            controller_write_pending: 0,
            controller_write_value: 0,
            #[cfg(feature = "irq-timing-trace")]
            irq_trace: None,
            #[cfg(feature = "irq-timing-trace")]
            trace_a12_latest: None,
            #[cfg(feature = "irq-timing-trace")]
            trace_last_a12: false,
            #[cfg(feature = "irq-timing-trace")]
            trace_a12_scratch: alloc::vec::Vec::new(),
            #[cfg(feature = "irq-timing-trace")]
            trace_bus_access: BusAccess::Idle,
            #[cfg(feature = "irq-timing-trace")]
            trace_bus_addr: 0,
            #[cfg(feature = "irq-timing-trace")]
            trace_bus_data: 0,
            #[cfg(feature = "irq-timing-trace")]
            trace_last_pc: 0,
            #[cfg(feature = "irq-timing-trace")]
            trace_r1_scanline_start: 0,
            #[cfg(feature = "irq-timing-trace")]
            trace_r1_dot_start: 0,
            #[cfg(feature = "irq-timing-trace")]
            trace_r1_frame_start: 0,
        };
        // Vs. System / PlayChoice-10: the arcade boards replace the 2C02 with a
        // 2C03 / 2C04 / 2C05 RGB PPU. For ConsoleType::Nes (the default), the
        // resolved type is VsPpuType::None -> Composite2C02, is_2c05 = false, so
        // this is byte-for-byte a no-op on normal carts.
        bus.reapply_vs_palette();
        // F-2: under R1 the DMC byte-timer is driven at end-of-cycle by
        // `cpu_clock_apu_dmc` (main's DMC fire-phase for DMASync).
        {
            bus.apu.set_dmc_driven_externally(true);
            // Interleaved-DMA Phase A: seed the get/put + DMC fire-phase from one
            // APUAlignment value. Fixed alignment 0 for now; Phase B drives it
            // from the power-on PRNG (the 2 AccuracyCoin answer-key alignments).
            bus.apu.seed_apu_alignment(0);
        }
        bus
    }

    /// Construct a Famicom Disk System bus from a `.fds` disk image and a
    /// user-supplied 8 KiB BIOS (`disksys.rom`).
    ///
    /// Parses the disk container ([`rustynes_mappers::parse_fds`]), constructs the
    /// FDS device ([`rustynes_mappers::Fds`]) as the bus's `Box<dyn Mapper>`, and
    /// wires the bus exactly like a cartridge build (shared internal
    /// `from_cart_and_mapper`). The FDS is NTSC/Famicom hardware, so the
    /// synthetic [`Cartridge`] metadata reports [`rustynes_mappers::Region::Ntsc`].
    ///
    /// # Errors
    ///
    /// Returns [`RomError`] if the disk image is unparseable, or the BIOS is not
    /// exactly 8 KiB.
    pub fn with_disk(
        disk_bytes: &[u8],
        bios_bytes: &[u8],
        sample_rate: u32,
    ) -> Result<Self, RomError> {
        let disk = rustynes_mappers::parse_fds(disk_bytes)?;
        let fds = rustynes_mappers::Fds::new(disk, bios_bytes)?;
        // Synthetic cartridge metadata: the bus only consults `cart.region`
        // (verified — see `docs/audit` FDS Stage 1). The FDS device owns all
        // PRG/CHR/BIOS storage, so the ROM byte fields are empty.
        let cart = Cartridge {
            prg_rom: Box::default(),
            chr_rom: Box::default(),
            mapper_id: 20,
            submapper: 0,
            mirroring: rustynes_mappers::Mirroring::Horizontal,
            region: rustynes_mappers::Region::Ntsc,
            console_type: rustynes_mappers::ConsoleType::Nes,
            vs_ppu_type: rustynes_mappers::VsPpuType::None,
            vs_dual_system: false,
            prg_ram_size: 0x8000,
            chr_ram_size: 0x2000,
            has_battery: false,
            has_trainer: false,
            is_nes2: false,
        };
        Ok(Self::from_cart_and_mapper(cart, Box::new(fds), sample_rate))
    }

    /// Build a bus that plays an NSF music file. Parses the `.nsf`, builds an
    /// [`rustynes_mappers::NsfMapper`] (a synthetic driver + the program image)
    /// as the bus's `Box<dyn Mapper>`, and reports synthetic NTSC cartridge
    /// metadata (the file carries no CHR / PPU program).
    ///
    /// # Errors
    ///
    /// Returns [`RomError::InvalidConfig`] when the NSF header is malformed.
    pub fn with_nsf(nsf_bytes: &[u8], sample_rate: u32) -> Result<Self, RomError> {
        let nsf = rustynes_mappers::parse_nsf(nsf_bytes)
            .map_err(|e| RomError::InvalidConfig(alloc::format!("{e}")))?;
        let mapper = rustynes_mappers::NsfMapper::new(&nsf);
        let cart = Cartridge {
            prg_rom: Box::default(),
            chr_rom: Box::default(),
            mapper_id: 31, // NSF banking is conventionally documented as mapper 31-like
            submapper: 0,
            mirroring: rustynes_mappers::Mirroring::Horizontal,
            // Playback is NTSC 60 Hz (vblank-NMI-driven) regardless of the
            // file's region preference; the PAL flag only feeds the driver's
            // init X-register. Exact non-60 Hz play rates are a documented
            // deferral (see `nsf.rs` module docs).
            region: rustynes_mappers::Region::Ntsc,
            console_type: rustynes_mappers::ConsoleType::Nes,
            vs_ppu_type: rustynes_mappers::VsPpuType::None,
            vs_dual_system: false,
            prg_ram_size: 0x2000,
            chr_ram_size: 0,
            has_battery: false,
            has_trainer: false,
            is_nes2: false,
        };
        Ok(Self::from_cart_and_mapper(
            cart,
            Box::new(mapper),
            sample_rate,
        ))
    }

    /// Reset (warm). Defers to `Ppu::reset` and clears DMA state. CPU is
    /// reset by the caller.
    pub fn reset(&mut self) {
        self.ppu.reset();
        self.apu.reset();
        {
            self.apu.set_dmc_driven_externally(true);
            self.apu.seed_apu_alignment(0);
        }
        self.dma_pending = None;
        self.dma_cycles_owed = 0;
        self.dma_idx = 0;
        self.dma_halt_addr = 0;
        self.deferred_dma_replay_addr = 0;
        self.unified_dma_clear();
    }

    /// Power-cycle. Zeroes RAM and resets all state. Caller resets the CPU.
    pub fn power_cycle(&mut self) {
        self.ram.fill(0);
        self.ppu = Ppu::new(self.ppu_region());
        // Re-apply the Vs./PC10 RGB-PPU configuration (lost when the PPU is
        // reconstructed). No-op for ConsoleType::Nes carts.
        self.reapply_vs_palette();
        self.apu = Apu::new(self.apu_region(), self.apu.sample_rate);
        {
            self.apu.set_dmc_driven_externally(true);
            self.apu.seed_apu_alignment(0);
        }
        self.controllers = [Controller::new(); 2];
        // The Four Score stays "plugged in" (it's hardware config), but its
        // transient strobe/read state resets like the controllers above.
        self.controllers34 = [Controller::new(); 2];
        self.four_score_idx = [0; 2];
        self.four_score_sig = [0; 2];
        // Vs. System coin/service inputs are transient (DIP switches are
        // hardware config and persist across a power-cycle, like the panel).
        self.vs_coin = 0;
        self.vs_service = false;
        // Non-standard input devices are unplugged on power-cycle (they are
        // re-attached explicitly by the frontend, like the controllers above).
        self.expansion_device = [None, None];
        self.cycle = 0;
        self.dma_pending = None;
        self.dma_cycles_owed = 0;
        self.dma_idx = 0;
        self.dma_halt_addr = 0;
        self.last_nmi_level = false;
        self.nmi_edge_latch = false;
        self.open_bus = 0;
        self.internal_data_bus = 0;
        self.deferred_dma_replay_addr = 0;
        self.m2_phase = M2Phase::Low;
        self.irq_snapshot_mapper_at_low = false;
        self.irq_snapshot_apu_at_low = false;
        self.irq_snapshot_mapper_at_high = false;
        self.irq_snapshot_apu_at_high = false;
        self.unified_dma_clear();
        // A cold boot must reset EVERY run-history-dependent field, or the
        // post-power-cycle machine depends on how long it ran before — breaking
        // the `power_cycle == fresh boot` equivalence (netplay power-cycles
        // both peers at session start and requires byte-identical state). A
        // residual `ppu_clock` in particular carries the old master-clock
        // CPU/PPU phase into the "new" boot, diverging timing-sensitive games
        // from frame 0. Mirrors the `with_sample_rate` initial values.
        self.ppu_clock = 0;
        self.dma_byte = 0;
        self.dma_page = 0;
        self.dma_total = 0;
        self.dma_mc_consumed = 0;
        self.last_read_addr = 0;
        self.in_dmc_dma = false;
        self.dmc_step_was_get = false;
        self.dmc_halt = false;
        self.controller_write_pending = 0;
        self.controller_write_value = 0;
        // Rebuild the mapper to its power-on state (fresh bank registers, cleared
        // CHR-RAM + volatile PRG-RAM), so a power-cycle is a true cold boot for
        // mapper-stateful games (MMC1/MMC3/…) too — without this, a stateful
        // mapper's banking + CHR-RAM survive, so two netplay peers that power-
        // cycled from different running states would desync. The existing `cart`
        // metadata (incl. any post-load `set_vs_ppu_type` override) is kept; only
        // the mapper is replaced. FDS (`rom_bytes == None`) keeps its mapper.
        // This also clears battery PRG-RAM (a battery-pull); RustyNES does not
        // persist standard battery saves to disk, so nothing on-disk is lost.
        if let Some(bytes) = self.rom_bytes.take() {
            if let Ok((_cart, mapper)) = rustynes_mappers::parse(&bytes) {
                self.mapper = mapper;
                // v2.8.0 Phase 4 — re-cache the capability flags for the
                // fresh mapper instance (same type, same flags, but keep
                // the invariant mechanical).
                self.mapper_caps = self.mapper.caps();
            }
            self.rom_bytes = Some(bytes);
        }
    }

    /// Developer-mode power-on randomization (Phase 7 / T-72-005).
    ///
    /// Fills the 2 KiB CPU work RAM and the external open-bus latch from a
    /// deterministic `xorshift64` PRNG. Real hardware powers up with
    /// unreliable RAM (see nesdev "CPU power up state"); games that depend on
    /// a particular post-power-on RAM pattern are buggy, and this option
    /// surfaces such bugs the way Mesen2's "randomize RAM on power-on" does.
    ///
    /// The fill is **seeded and deterministic** — the same `seed` always
    /// yields the same power-on state, so the
    /// `same seed + ROM + input ⇒ bit-identical` determinism contract (and
    /// therefore save-state round-trip and the regression oracle) is
    /// preserved. CI and tests use the default (zeroed) path; this is opt-in
    /// via [`crate::Nes::from_rom_with_power_on_seed`].
    ///
    /// CPU/PPU phase alignment and DMA get/put phase are intentionally **not**
    /// randomized here: the lockstep scheduler's phase is deterministic by
    /// design and randomizing it is entangled with the v2.0 master-clock
    /// scheduling refactor (see `docs/audit/phase-7-assessment-2026-05-24.md`).
    pub fn randomize_power_on_ram(&mut self, seed: u64) {
        // Avoid the xorshift64 zero fixed point.
        let mut s = if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        };
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            // Byte 3 (bits 24-31) — extracted without a truncating cast.
            s.to_le_bytes()[3]
        };
        for byte in self.ram.iter_mut() {
            *byte = next();
        }
        self.open_bus = next();
    }

    /// Borrow the framebuffer (RGBA8, 256x240).
    #[must_use]
    pub fn framebuffer(&self) -> &[u8] {
        self.ppu.framebuffer()
    }

    /// Borrow the parallel palette-index framebuffer (256x240 `u16`s) for the
    /// `NES_NTSC` composite filter. See [`rustynes_ppu::Ppu::index_framebuffer`].
    #[must_use]
    pub fn index_framebuffer(&self) -> &[u16] {
        self.ppu.index_framebuffer()
    }

    /// v1.2.0 C3 (hd-pack) — borrow the per-pixel HD-pack tile-source buffer.
    /// See [`rustynes_ppu::Ppu::hd_tile_source`]. Output-only telemetry.
    #[cfg(feature = "hd-pack")]
    #[must_use]
    pub fn hd_tile_source(&self) -> &[rustynes_ppu::HdTileSource] {
        self.ppu.hd_tile_source()
    }

    /// The per-frame NTSC composite colour phase for the `NES_NTSC` filter
    /// (`0..=2` on NTSC; frame parity `0..=1` on PAL/Dendy). See
    /// [`rustynes_ppu::Ppu::ntsc_phase`].
    #[must_use]
    pub const fn ntsc_phase(&self) -> u8 {
        self.ppu.ntsc_phase()
    }

    /// v1.1.0 beta.1 — install (`Some`) or clear (`None`) a custom 64-entry base
    /// palette from a loaded `.pal` file. A presentation override; `None` (default)
    /// is byte-identical to the built-in palette.
    pub const fn set_custom_palette(&mut self, base: Option<[[u8; 3]; 64]>) {
        self.ppu.set_custom_palette(base);
    }

    /// v1.7.0 "Forge" F3 — set the PPU extra-scanlines overclock (extra idle
    /// vblank lines per frame). `0` (default) is byte-identical to stock timing.
    pub const fn set_extra_scanlines(&mut self, lines: u16) {
        self.ppu.set_extra_scanlines(lines);
    }

    /// v1.7.0 F3 — the configured extra-scanline count (`0` = stock).
    #[must_use]
    pub const fn extra_scanlines(&self) -> u16 {
        self.ppu.extra_scanlines()
    }

    /// Cartridge region (NTSC / PAL / Dendy / Multi). Drives wall-clock
    /// frame pacing in the frontend and clock-divider selection inside the
    /// PPU + APU.
    #[must_use]
    pub const fn region(&self) -> rustynes_mappers::Region {
        self.cart.region
    }

    /// Length in bytes of the loaded cartridge's PRG-ROM (read-only metadata).
    #[must_use]
    pub const fn prg_rom_len(&self) -> usize {
        self.cart.prg_rom.len()
    }

    /// Length in bytes of the loaded cartridge's CHR-ROM (0 when the board uses
    /// CHR-RAM). Read-only metadata.
    #[must_use]
    pub const fn chr_rom_len(&self) -> usize {
        self.cart.chr_rom.len()
    }

    /// Enable the per-CPU-cycle IRQ-timing trace fixture with the given
    /// record capacity.  Records past the cap are silently dropped (see
    /// `IrqTrace::overflow`).  See ADR-0002 "Decision (revised,
    /// 2026-05-13)" → "Test fixture" and
    /// `crates/rustynes-core/src/irq_trace.rs`.
    #[cfg(feature = "irq-timing-trace")]
    pub fn enable_irq_trace(&mut self, capacity: usize) {
        self.irq_trace = Some(IrqTrace::with_capacity(capacity));
        self.trace_a12_latest = None;
        self.trace_a12_scratch.clear();
        // Snapshot whatever A12 level the PPU last drove so the first
        // recorded transition matches reality (the PPU's `last_a12` is
        // private to its module; we accept "first cycle may miss a level
        // assignment" as a cold-start artifact, matching every existing
        // diagnostic probe).
        self.trace_last_a12 = false;
        // Session-21: reset bus-access tracker so the first traced cycle
        // reflects accurate (CPU-driven) state rather than a stale
        // pre-trace driver.
        self.trace_bus_access = BusAccess::Idle;
        self.trace_bus_addr = 0;
        self.trace_bus_data = 0;
    }

    /// Take the accumulated IRQ trace, leaving the bus's trace slot empty.
    /// Returns `None` if tracing was never enabled.
    #[cfg(feature = "irq-timing-trace")]
    #[must_use]
    pub const fn take_irq_trace(&mut self) -> Option<IrqTrace> {
        self.irq_trace.take()
    }

    /// Borrow the in-flight IRQ trace for inspection without taking it.
    #[cfg(feature = "irq-timing-trace")]
    #[must_use]
    pub const fn irq_trace(&self) -> Option<&IrqTrace> {
        self.irq_trace.as_ref()
    }

    /// Direct CPU-bus probe (does **not** advance time). Intended for
    /// blargg-style status polls at `$6000-$7FFF` and the test harness's
    /// mapper-resident WRAM peek. Note that this still has side effects on
    /// PPU registers (`$2002` clears VBL and toggle, `$2007` reads advance
    /// the buffer); callers should avoid touching `$2000-$3FFF` via peek.
    pub fn peek_cpu(&mut self, addr: u16) -> u8 {
        self.raw_cpu_read(addr)
    }

    /// Add a Game Genie code (6 or 8 characters, case-insensitive). The code
    /// patches a PRG address (`$8000-$FFFF`) on the CPU read path; adding a
    /// code at an address that already has one replaces it.
    ///
    /// # Errors
    ///
    /// Returns [`GenieError`] if the code string cannot be decoded.
    pub fn add_genie_code(&mut self, code: &str) -> Result<(), GenieError> {
        let gc = GenieCode::new(code)?;
        self.genie_codes.insert(gc.addr(), gc);
        Ok(())
    }

    /// Remove the active Game Genie code whose canonical (upper-case) string
    /// matches `code`. No-op if no such code is active.
    pub fn remove_genie_code(&mut self, code: &str) {
        let want = code.to_ascii_uppercase();
        self.genie_codes.retain(|_, gc| gc.code() != want.as_str());
    }

    /// Remove all active Game Genie codes.
    pub fn clear_genie_codes(&mut self) {
        self.genie_codes.clear();
    }

    /// Iterate the active Game Genie codes (address-sorted).
    pub fn genie_codes(&self) -> impl Iterator<Item = &GenieCode> {
        self.genie_codes.values()
    }

    /// Apply any active Game Genie code at `addr` to a freshly-read byte.
    /// Fast-paths (single branch) when no codes are active.
    fn apply_genie(&self, addr: u16, original: u8) -> u8 {
        if self.genie_codes.is_empty() {
            return original;
        }
        self.genie_codes
            .get(&addr)
            .map_or(original, |gc| gc.read(original))
    }

    /// Side-effect-free CPU bus sample for the debugger hex viewer.
    ///
    /// Returns the bus's view of the byte at `addr` without the side
    /// effects `peek_cpu` / `raw_cpu_read` carry on PPU register space
    /// (no VBL clear, no PPUDATA buffer advance, no open-bus update). For
    /// PPU registers we read back the cached snapshot; for mappers we go
    /// through `cpu_read` — the overwhelming majority of mappers are
    /// idempotent on `$8000-$FFFF` reads, and the few that latch on read
    /// (MMC2 in particular) document that behavior as inherent.
    ///
    /// Takes `&mut self` because mapper `cpu_read` is `&mut` — but no
    /// emulator-visible state advances. The CPU cycle counter, PPU
    /// scheduler, and APU all stay put.
    pub fn debug_peek_cpu(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => {
                let reg = (addr & 7) as u8;
                let regs = self.ppu.debug_registers();
                match reg {
                    0 => regs[0],
                    1 => regs[1],
                    2 => regs[2],
                    3 => regs[3],
                    _ => 0,
                }
            }
            0x4015 => {
                let mut v = 0u8;
                if self.apu.pulse1_out() != 0 {
                    v |= 0x01;
                }
                if self.apu.pulse2_out() != 0 {
                    v |= 0x02;
                }
                if self.apu.triangle_out() != 0 {
                    v |= 0x04;
                }
                if self.apu.noise_out() != 0 {
                    v |= 0x08;
                }
                if self.apu.frame_irq_pending() {
                    v |= 0x40;
                }
                if self.apu.dmc_irq_pending() {
                    v |= 0x80;
                }
                v
            }
            0x4016 => 0x40 | self.peek_port(0),
            0x4017 => 0x40 | self.peek_port(1),
            0x4000..=0x4014 | 0x4018..=0x401F => self.open_bus,
            0x4020..=0xFFFF => {
                // Mirror the production read path so the debugger hex viewer
                // shows the Game-Genie-substituted byte the CPU would see.
                let raw = self.mapper.cpu_read(addr);
                self.apply_genie(addr, raw)
            }
        }
    }

    /// Side-effect-free PPU bus sample (`$0000-$3FFF`).
    ///
    /// `$0000-$1FFF` -> mapper CHR, `$2000-$3EFF` -> nametable
    /// (via mapper's mirroring), `$3F00-$3FFF` -> palette RAM.
    pub fn debug_peek_ppu(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.mapper.ppu_read(addr),
            0x2000..=0x3EFF => {
                if let Some(v) = self.mapper.nametable_fetch(addr) {
                    v
                } else {
                    let phys = match self.nt_mirroring_override {
                        Some(m) => override_nt_addr(m, addr) as usize,
                        None => self.mapper.nametable_address(addr) as usize,
                    };
                    let ciram = self.ppu.ciram();
                    ciram[phys % ciram.len()]
                }
            }
            0x3F00..=0x3FFF => {
                let idx = (addr & 0x1F) as usize;
                let palette = self.ppu.palette_ram();
                // Mirror sprite-palette zero into BG-palette zero.
                let idx = if idx & 0x13 == 0x10 { idx & 0x0F } else { idx };
                palette[idx]
            }
            _ => 0,
        }
    }

    /// v1.7.0 "Forge" Workstream A1 — debugger writeback. The structural mirror
    /// of [`Self::debug_peek_ppu`]: `$0000-$1FFF` → mapper CHR (`ppu_write`,
    /// a no-op on CHR-ROM), `$2000-$3EFF` → nametable (mapper-absorbed, else
    /// CIRAM via the active mirroring), `$3F00-$3FFF` → palette RAM.
    ///
    /// Side-effect-free w.r.t. the run loop: it is reached *only* through the
    /// gated post-frame poke path (the same caller-side, after-`run_frame` stage
    /// the raw RAM cheats use), so the deterministic core run loop is unchanged
    /// and the no-edit path is byte-identical. `debug-hooks`-gated.
    #[cfg(feature = "debug-hooks")]
    pub fn debug_poke_ppu(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.mapper.ppu_write(addr & 0x1FFF, value),
            0x2000..=0x3EFF => {
                let nt_addr = if addr >= 0x3000 { addr - 0x1000 } else { addr };
                // Give the mapper a chance to absorb the write (ExRAM
                // nametables, fill-mode drops), exactly like `write_vram`.
                if !self.mapper.nametable_write(nt_addr, value) {
                    let phys = match self.nt_mirroring_override {
                        Some(m) => override_nt_addr(m, nt_addr) as usize,
                        None => self.mapper.nametable_address(nt_addr) as usize,
                    };
                    self.ppu.debug_poke_ciram(phys, value);
                }
            }
            0x3F00..=0x3FFF => self.ppu.debug_poke_palette((addr & 0x1F) as u8, value),
            _ => {}
        }
    }

    /// v1.7.0 "Forge" Workstream A1 — debugger writeback for one OAM byte
    /// (`idx` = 0..256). `debug-hooks`-gated; reached only through the gated
    /// post-frame poke path, so the default build is byte-identical.
    #[cfg(feature = "debug-hooks")]
    pub const fn debug_poke_oam(&mut self, idx: u8, value: u8) {
        self.ppu.debug_poke_oam(idx, value);
    }

    /// Borrow the PPU (debugger / tests).
    #[must_use]
    pub const fn ppu(&self) -> &Ppu {
        &self.ppu
    }

    /// Mutably borrow the PPU (debugger / tests).
    pub const fn ppu_mut(&mut self) -> &mut Ppu {
        &mut self.ppu
    }

    /// Borrow the APU (debugger / tests).
    #[must_use]
    pub const fn apu(&self) -> &Apu {
        &self.apu
    }

    /// Mutably borrow the APU (debugger / tests).
    pub const fn apu_mut(&mut self) -> &mut Apu {
        &mut self.apu
    }

    /// Set the buttons currently held on player `port`. Ports 0/1 are the
    /// standard `$4016`/`$4017` controllers; ports 2/3 are players 3/4 on the
    /// Four Score adapter (only polled when [`Self::set_four_score`] is on).
    /// The change takes effect on the next strobe edge.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=3`.
    pub const fn set_buttons(&mut self, port: usize, buttons: Buttons) {
        assert!(
            port < 4,
            "controller port must be 0..=3 (2/3 are the Four Score)"
        );
        match port {
            0 | 1 => self.controllers[port].set_buttons(buttons),
            _ => self.controllers34[port - 2].set_buttons(buttons),
        }
    }

    /// Attach (or replace) a non-standard overlay device on `port` (0 =
    /// `$4016`, 1 = `$4017`). Pass `None` to unplug the device and return the
    /// port to the standard controller / Four Score path (byte-identical).
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub fn set_expansion_device(
        &mut self,
        port: usize,
        device: Option<crate::input_device::InputDevice>,
    ) {
        assert!(port < 2, "expansion-device port must be 0..=1");
        self.expansion_device[port] = device;
    }

    /// Borrow the overlay device attached to `port`, if any.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    #[must_use]
    pub const fn expansion_device(&self, port: usize) -> &Option<crate::input_device::InputDevice> {
        assert!(port < 2, "expansion-device port must be 0..=1");
        &self.expansion_device[port]
    }

    /// Update an attached Vaus paddle's position + fire state on `port`. No-op
    /// if the attached device is not a Vaus (or no device is attached).
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub const fn set_paddle(&mut self, port: usize, position: u8, fire: bool) {
        assert!(port < 2, "paddle port must be 0..=1");
        if let Some(crate::input_device::InputDevice::Vaus(v)) = &mut self.expansion_device[port] {
            v.set(position, fire);
        }
    }

    /// Update an attached Zapper's aim point + trigger on `port`. No-op if the
    /// attached device is not a Zapper (or no device is attached).
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub const fn set_zapper(&mut self, port: usize, x: u16, y: u16, trigger: bool) {
        assert!(port < 2, "zapper port must be 0..=1");
        if let Some(crate::input_device::InputDevice::Zapper(z)) = &mut self.expansion_device[port]
        {
            z.set(x, y, trigger);
        }
    }

    /// Update an attached Power Pad's live button mask (bit `i` = mat button
    /// `i+1`) on `port`. No-op if the attached device is not a Power Pad.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub const fn set_power_pad(&mut self, port: usize, buttons: u16) {
        assert!(port < 2, "power pad port must be 0..=1");
        if let Some(crate::input_device::InputDevice::PowerPad(p)) =
            &mut self.expansion_device[port]
        {
            p.set(buttons);
        }
    }

    /// Update an attached SNES mouse's movement + buttons + sensitivity on
    /// `port`. No-op if the attached device is not a mouse.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub const fn set_snes_mouse(
        &mut self,
        port: usize,
        dx: i16,
        dy: i16,
        left: bool,
        right: bool,
        sensitivity: u8,
    ) {
        assert!(port < 2, "mouse port must be 0..=1");
        if let Some(crate::input_device::InputDevice::SnesMouse(m)) =
            &mut self.expansion_device[port]
        {
            m.set(dx, dy, left, right, sensitivity);
        }
    }

    /// Update an attached Family BASIC keyboard's pressed-key bitmap on `port`
    /// (one byte per matrix row). No-op if the attached device is not a keyboard.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub const fn set_family_keyboard(&mut self, port: usize, keys: [u8; 9]) {
        assert!(port < 2, "keyboard port must be 0..=1");
        if let Some(crate::input_device::InputDevice::FamilyKeyboard(k)) =
            &mut self.expansion_device[port]
        {
            k.set_keys(keys);
        }
    }

    /// v1.3.0 Workstream F1 — update an attached Family Trainer mat's 12-button
    /// mask on `port`. No-op if the attached device is not a Family Trainer.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub const fn set_family_trainer(&mut self, port: usize, buttons: u16) {
        assert!(port < 2, "family trainer port must be 0..=1");
        if let Some(crate::input_device::InputDevice::FamilyTrainer(p)) =
            &mut self.expansion_device[port]
        {
            p.set(buttons);
        }
    }

    /// v1.3.0 Workstream F1 — update an attached Subor keyboard's pressed-key
    /// bitmap on `port`. No-op if the attached device is not a Subor keyboard.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub const fn set_subor_keyboard(&mut self, port: usize, keys: [u8; 9]) {
        assert!(port < 2, "subor keyboard port must be 0..=1");
        if let Some(crate::input_device::InputDevice::SuborKeyboard(k)) =
            &mut self.expansion_device[port]
        {
            k.set_keys(keys);
        }
    }

    /// v1.3.0 Workstream F1 — update an attached Konami Hyper Shot's 4-button
    /// mask on `port`. No-op if the attached device is not a Konami Hyper Shot.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub const fn set_konami_hyper_shot(&mut self, port: usize, buttons: u8) {
        assert!(port < 2, "konami hyper shot port must be 0..=1");
        if let Some(crate::input_device::InputDevice::KonamiHyperShot(h)) =
            &mut self.expansion_device[port]
        {
            h.set(buttons);
        }
    }

    /// v1.3.0 Workstream F1 — update an attached Bandai Hyper Shot's 8-sensor
    /// mask on `port`. No-op if the attached device is not a Bandai Hyper Shot.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub const fn set_bandai_hyper_shot(&mut self, port: usize, sensors: u8) {
        assert!(port < 2, "bandai hyper shot port must be 0..=1");
        if let Some(crate::input_device::InputDevice::BandaiHyperShot(b)) =
            &mut self.expansion_device[port]
        {
            b.set(sensors);
        }
    }

    /// v1.1.0 beta.1 (T-110-B4) — set (`Some`) or clear (`None`) the per-game
    /// nametable mirroring override. A frontend load-time correction; `None`
    /// (default) defers to the mapper (byte-identical).
    pub const fn set_mirroring_override(&mut self, m: Option<rustynes_mappers::Mirroring>) {
        self.nt_mirroring_override = m;
    }

    /// The current per-game mirroring override (for the save-state).
    #[must_use]
    pub const fn mirroring_override(&self) -> Option<rustynes_mappers::Mirroring> {
        self.nt_mirroring_override
    }

    /// v1.1.0 beta.2 (T-110-C3) — start/stop event-viewer recording.
    #[cfg(feature = "debug-hooks")]
    pub const fn set_event_logging(&mut self, enabled: bool) {
        self.event_logging = enabled;
    }

    /// Whether event-viewer recording is on.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub const fn event_logging(&self) -> bool {
        self.event_logging
    }

    /// The events captured so far this frame.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Vec->slice deref is not const.
    pub fn events(&self) -> &[EventRec] {
        &self.events
    }

    /// v1.1.0 beta.3 (T-110-E2) — start/stop the Lua bus-access log.
    #[cfg(feature = "debug-hooks")]
    pub const fn set_access_logging(&mut self, enabled: bool) {
        self.access_logging = enabled;
    }

    /// Whether the bus-access log is recording.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub const fn access_logging(&self) -> bool {
        self.access_logging
    }

    /// The CPU bus accesses captured so far this frame.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Vec->slice deref is not const.
    pub fn accesses(&self) -> &[AccessRec] {
        &self.accesses
    }

    /// Clear the bus-access log (called per frame by the run loop).
    #[cfg(feature = "debug-hooks")]
    pub fn clear_accesses(&mut self) {
        self.accesses.clear();
    }

    /// v1.2.0 (T-110-E1) — start/stop the Lua interrupt-service log.
    #[cfg(feature = "debug-hooks")]
    pub const fn set_interrupt_logging(&mut self, enabled: bool) {
        self.interrupt_logging = enabled;
    }

    /// Whether the interrupt-service log is recording.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub const fn interrupt_logging(&self) -> bool {
        self.interrupt_logging
    }

    /// The interrupt-service entries captured so far this frame.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Vec->slice deref is not const.
    pub fn interrupts(&self) -> &[InterruptRec] {
        &self.interrupts
    }

    /// Clear the interrupt-service log (called per frame by the run loop).
    #[cfg(feature = "debug-hooks")]
    pub fn clear_interrupts(&mut self) {
        self.interrupts.clear();
    }

    /// v1.4.0 Workstream D (D2) — set the armed event-breakpoint category mask
    /// (a bit-OR of [`EventBpKind::bit`]). `0` disarms all (the default + the
    /// per-cycle-cheap path).
    #[cfg(feature = "debug-hooks")]
    pub const fn set_event_breakpoints(&mut self, mask: u16) {
        self.event_bp_mask = mask;
    }

    /// The armed event-breakpoint category mask.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub const fn event_breakpoints(&self) -> u16 {
        self.event_bp_mask
    }

    /// Take the first event-breakpoint hit of the current frame (cleared on
    /// read). The frontend polls this after `run_frame`.
    #[cfg(feature = "debug-hooks")]
    pub const fn take_event_break_hit(&mut self) -> Option<EventBreakHit> {
        self.event_break_hit.take()
    }

    /// Clear any recorded event-breakpoint hit (called per frame by the run
    /// loop so each frame starts fresh).
    #[cfg(feature = "debug-hooks")]
    pub const fn clear_event_break_hit(&mut self) {
        self.event_break_hit = None;
    }

    /// v1.4.0 Workstream D (D2) — observational event-breakpoint tap. If `kind`
    /// is armed and no hit has been recorded yet this frame, latch the event
    /// with its full timing context. Pure observation — no emulator-visible
    /// state changes, so determinism holds. The `mask == 0` fast path keeps the
    /// default (no armed categories) cheap.
    #[cfg(feature = "debug-hooks")]
    const fn record_event_break(&mut self, kind: EventBpKind, addr: u16) {
        if self.event_bp_mask & kind.bit() == 0 || self.event_break_hit.is_some() {
            return;
        }
        self.event_break_hit = Some(EventBreakHit {
            kind,
            addr,
            frame: self.ppu.frame(),
            cycle: self.cycle,
            scanline: self.ppu.scanline(),
            dot: self.ppu.dot(),
        });
    }

    /// Clear the event log (called at each frame start while recording).
    #[cfg(feature = "debug-hooks")]
    pub fn clear_events(&mut self) {
        self.events.clear();
    }

    /// Clear the per-frame `TAStudio` lag-log "controller polled" flag (called at
    /// the top of each [`crate::Nes::run_frame`]). `debug-hooks`-gated;
    /// output-only, so the shipped build is byte-identical.
    #[cfg(feature = "debug-hooks")]
    pub(crate) const fn clear_controller_polled(&mut self) {
        self.controller_polled = false;
    }

    /// `true` if a controller port (`$4016`/`$4017`) was read since the last
    /// [`Self::clear_controller_polled`] — i.e. during the current frame.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub(crate) const fn controller_polled(&self) -> bool {
        self.controller_polled
    }

    /// Sample the framebuffer luminance at each attached Zapper's aim point.
    /// Called once per frame (only does work when a Zapper is attached, so the
    /// no-device path is byte-identical).
    pub fn sample_zapper_light(&mut self) {
        let has_zapper = self
            .expansion_device
            .iter()
            .any(|d| matches!(d, Some(crate::input_device::InputDevice::Zapper(_))));
        if !has_zapper {
            return;
        }
        // Borrow the framebuffer once; copy the per-port aim sample.
        for port in 0..2 {
            if let Some(crate::input_device::InputDevice::Zapper(_)) = &self.expansion_device[port]
            {
                // Take the device out to avoid the &mut self / &self.ppu borrow
                // conflict, sample, then put it back.
                let mut dev = self.expansion_device[port].take();
                if let Some(crate::input_device::InputDevice::Zapper(z)) = &mut dev {
                    z.sample_light(self.ppu.framebuffer());
                }
                self.expansion_device[port] = dev;
            }
        }
    }

    /// Borrow controller `port` (0/1 = `$4016`/`$4017`; 2/3 = Four Score
    /// players 3/4).
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=3`.
    #[must_use]
    pub const fn controller(&self, port: usize) -> &Controller {
        match port {
            0 | 1 => &self.controllers[port],
            _ => &self.controllers34[port - 2],
        }
    }

    /// Has the PPU completed a frame? Drains the latch.
    pub const fn take_frame_complete(&mut self) -> bool {
        self.ppu.take_frame_complete()
    }

    /// Drain finalized audio samples (host sample rate, normalized `[0, ~1]`).
    pub fn drain_audio(&mut self) -> Vec<f32> {
        self.apu.drain_audio()
    }

    /// Drain into a slice.
    pub fn drain_audio_into(&mut self, out: &mut [f32]) -> usize {
        self.apu.drain_audio_into(out)
    }

    /// Cumulative CPU cycle count.
    #[must_use]
    pub const fn cycle(&self) -> u64 {
        self.cycle
    }

    /// Returns the M2 phase the lockstep scheduler is currently in.
    ///
    /// The scheduler ticks the PPU 3 dots per CPU cycle.  Convention:
    /// [`M2Phase::Low`] is the FIRST half of the cycle (the cycle's
    /// pre-sub-dot-1 portion, corresponding to silicon's `φ1`);
    /// [`M2Phase::High`] is the SECOND half (post-sub-dot-1, silicon's
    /// `φ2`).  The boundary is the M2-rising edge between sub-dot 1 and
    /// sub-dot 2.
    ///
    /// At the start of `tick_one_cpu_cycle` the bus is in [`M2Phase::Low`].
    /// After sub-dot 1 of the 3-PPU-dot tick loop, it transitions to
    /// [`M2Phase::High`].  After the cycle's last sub-dot the bus
    /// advances the cycle counter and returns to [`M2Phase::Low`] for
    /// the next cycle.
    ///
    /// This accessor is informational.  As of Phase B2 the bus stores
    /// per-phase IRQ snapshots (read by [`Bus::poll_irq_at_phase`])
    /// independently of this accessor — `current_m2_phase()` itself is
    /// not consulted by the CPU's IRQ sample path.
    #[must_use]
    pub const fn current_m2_phase(&self) -> M2Phase {
        self.m2_phase
    }

    /// Set the Vs. System 8-bit DIP switch bank (switch 1 = bit 0 ..
    /// switch 8 = bit 7). No effect on non-Vs. carts. Default 0.
    pub const fn set_vs_dip(&mut self, dip: u8) {
        self.vs_dip = dip;
    }

    /// Current Vs. System DIP switch bank.
    #[must_use]
    pub const fn vs_dip(&self) -> u8 {
        self.vs_dip
    }

    /// Push the cartridge's current [`rustynes_mappers::VsPpuType`] into the PPU
    /// (output palette + 2C05 `$2000`/`$2001` swap + `$2002` identifier).
    ///
    /// Called from the constructor, [`Self::power_cycle`], and
    /// [`Self::set_vs_ppu_type`]. For [`rustynes_mappers::ConsoleType::Nes`] carts
    /// the resolved type is [`rustynes_mappers::VsPpuType::None`] -> `Composite2C02`,
    /// `is_2c05 = false`, so this is byte-for-byte a no-op on normal carts.
    const fn reapply_vs_palette(&mut self) {
        let vs = self.cart.vs_ppu_type;
        let palette = vs_palette_to_ppu(vs.ppu_palette());
        self.ppu
            .set_palette(palette, vs.is_2c05(), vs.ppu_2c05_id());
    }

    /// Override the Vs. System PPU type and re-apply the output palette / 2C05
    /// quirks immediately.
    ///
    /// iNES-1.0 dumps carry no NES 2.0 byte-13, so the parser defaults a Vs.
    /// cart to [`rustynes_mappers::VsPpuType::Rp2C03`]; a per-game database (keyed on
    /// the ROM SHA-256) supplies the correct 2C04-000x / 2C05 type, which the
    /// frontend applies through this setter. No effect on the running game's
    /// logic — only the colour LUT the PPU emits through. No-op shape on
    /// non-Vs. carts (the default path never calls this).
    pub const fn set_vs_ppu_type(&mut self, t: rustynes_mappers::VsPpuType) {
        self.cart.vs_ppu_type = t;
        self.reapply_vs_palette();
    }

    /// Latch a Vs. System coin insertion. `acceptor` 0 = acceptor #1 ($4016
    /// bit 5), 1 = acceptor #2 ($4016 bit 6); any other value is ignored. The
    /// frontend should clear the latch (see [`Self::clear_coin`]) after the
    /// real-hardware ~40-70 ms window (a few frames). No effect on non-Vs.
    /// carts.
    pub const fn insert_coin(&mut self, acceptor: u8) {
        match acceptor {
            0 => self.vs_coin |= 0x01,
            1 => self.vs_coin |= 0x02,
            _ => {}
        }
    }

    /// Clear all latched Vs. System coin-insert signals.
    pub const fn clear_coin(&mut self) {
        self.vs_coin = 0;
    }

    /// Set / clear the Vs. System service button ($4016 bit 2).
    pub const fn set_vs_service(&mut self, pressed: bool) {
        self.vs_service = pressed;
    }

    /// True when the running cart is Vs. System hardware (NES 2.0 console type).
    #[must_use]
    pub fn is_vs_system(&self) -> bool {
        self.cart.console_type == rustynes_mappers::ConsoleType::VsSystem
    }

    /// True when the cart's header marks a Vs. `DualSystem` board (two CPUs /
    /// two PPUs). Detection only — the dual-console emulation is a documented
    /// v2.0 deferral (`docs/audit/vs-dualsystem-design-2026-06-11.md`); this
    /// lets the frontend surface a clear note instead of a black screen.
    #[must_use]
    pub const fn is_vs_dual_system(&self) -> bool {
        self.cart.vs_dual_system
    }

    /// Overlay the Vs. System `$4016` upper bits (service, DIP 1/2, coins) onto
    /// the standard controller read. No-op on non-Vs. carts, so the standard
    /// `$4016` read is byte-identical.
    ///
    /// Layout (nesdev "Vs. System" §`$4016` read): `PCCD DS0B` — bit 0 = right
    /// stick (already in `base`), bit 2 = service, bit 3 = DIP switch 1, bit 4 =
    /// DIP switch 2, bit 5 = coin #1, bit 6 = coin #2, bit 7 = primary CPU (we
    /// model a single CPU, so leave it 0).
    fn vs_overlay_4016(&self, base: u8) -> u8 {
        if !self.is_vs_system() {
            return base;
        }
        // Keep only bit 0 (controller D0) + bit 1 (D1, always 0 here); the Vs.
        // bus drives bits 2-7 from the panel, not from open bus.
        let mut v = base & 0x01;
        if self.vs_service {
            v |= 0x04;
        }
        // DIP switch 1 -> bit 3, switch 2 -> bit 4.
        v |= (self.vs_dip & 0x01) << 3;
        v |= ((self.vs_dip >> 1) & 0x01) << 4;
        // Coin acceptors -> bits 5/6.
        v |= (self.vs_coin & 0x03) << 5;
        v
    }

    /// Overlay the Vs. System `$4017` upper bits (DIP 3-8) onto the standard
    /// controller read. No-op on non-Vs. carts.
    ///
    /// Layout (nesdev "Vs. System" §`$4017` read): `DDDD DD0B` — bit 0 = left
    /// stick (already in `base`), bits 2-7 = DIP switches 3 through 8.
    fn vs_overlay_4017(&self, base: u8) -> u8 {
        if !self.is_vs_system() {
            return base;
        }
        // DIP switches 3..=8 occupy bits 2..=7 (switch 3 = DIP bit 2 -> $4017
        // bit 2, switch 8 = DIP bit 7 -> $4017 bit 7); a 1:1 mapping.
        (base & 0x01) | (self.vs_dip & 0xFC)
    }

    /// Mapper debug info for the debugger UI: the mapper's own bank/IRQ state
    /// ENRICHED (v1.5.0 "Lens" Workstream I8) with the cartridge-level metadata
    /// the bus owns — submapper, accuracy tier, ROM/RAM sizes, battery, the IRQ
    /// mechanism, and the expansion-audio chip. Output-only; these enrichment
    /// fields are filled here rather than in each of the 100+ mappers, and they
    /// default to empty (so a mapper's own `debug_info()` is unchanged).
    #[must_use]
    pub fn mapper_debug_info(&self) -> rustynes_mappers::MapperDebugInfo {
        let mut info = self.mapper.debug_info();
        let cart = &self.cart;
        info.submapper = cart.submapper;
        info.tier = rustynes_mappers::mapper_tier(cart.mapper_id, cart.submapper)
            .map_or("", rustynes_mappers::MapperTier::name);
        info.prg_rom_size = cart.prg_rom.len();
        info.chr_rom_size = cart.chr_rom.len();
        info.prg_ram_size = cart.prg_ram_size as usize;
        info.chr_ram_size = cart.chr_ram_size as usize;
        info.has_battery = cart.has_battery;
        // IRQ mechanism: named per the documented per-mapper IRQ family table
        // (docs/mappers.md). MMC3/RAMBO use PPU A12; MMC5 uses scanline
        // detection; the VRC/FME-7/N163 families tick on the CPU-cycle hook.
        info.irq_kind = match cart.mapper_id {
            4 | 64 | 118 | 119 | 206 => "PPU A12 counter (MMC3-style)",
            5 => "PPU scanline (MMC5)",
            // CPU-cycle-clocked IRQ counters surface via the caps hook.
            _ if self.mapper_caps.cpu_cycle_hook => "CPU cycle (VRC / FME-7 / N163)",
            _ => "",
        };
        info.expansion_audio = if self.mapper_caps.audio {
            Some(match cart.mapper_id {
                5 => "MMC5",
                19 | 210 => "Namco 163",
                20 => "FDS",
                24 | 26 => "VRC6",
                69 => "Sunsoft 5B",
                85 => "VRC7 (OPLL)",
                _ => "Expansion audio",
            })
        } else {
            None
        };
        info
    }

    /// The cached per-cycle mapper capability flags (see
    /// [`rustynes_mappers::MapperCaps`]). `caps.audio` reflects whether the
    /// loaded mapper has on-cart expansion audio with the `mapper-audio` feature
    /// compiled in — used by the frontend to surface expansion-channel mixing
    /// controls only for boards that actually have them.
    #[must_use]
    pub const fn mapper_caps(&self) -> rustynes_mappers::MapperCaps {
        self.mapper_caps
    }

    /// Borrow CPU RAM (2 KiB).
    #[must_use]
    pub fn ram_bytes(&self) -> &[u8] {
        &*self.ram
    }

    /// Borrow both controllers as a slice.
    #[must_use]
    pub const fn controllers_ref(&self) -> &[Controller; 2] {
        &self.controllers
    }

    /// Borrow the Four Score players 3 & 4 (save-state).
    #[must_use]
    pub const fn controllers34_ref(&self) -> &[Controller; 2] {
        &self.controllers34
    }

    /// Enable/disable the Four Score 4-player adapter. Off by default; while
    /// off, `$4016`/`$4017` behave exactly as the standard two controllers
    /// (byte-identical reads — determinism + save-states unaffected).
    pub const fn set_four_score(&mut self, enabled: bool) {
        self.four_score = enabled;
    }

    /// Whether the Four Score adapter is currently enabled.
    #[must_use]
    pub const fn four_score(&self) -> bool {
        self.four_score
    }

    // --- Famicom Disk System disk control (delegates to the mapper) ---

    /// Number of disk sides in the inserted FDS image (0 for cartridge builds).
    #[must_use]
    pub fn disk_side_count(&self) -> usize {
        self.mapper.disk_side_count()
    }

    /// The currently inserted FDS disk side, or `None` when ejected (or for a
    /// cartridge build).
    #[must_use]
    pub fn inserted_disk_side(&self) -> Option<usize> {
        self.mapper.inserted_disk_side()
    }

    /// Insert FDS side `i` (`Some`) or eject (`None`). No-op on cartridge builds.
    pub fn set_disk_side(&mut self, side: Option<usize>) {
        self.mapper.set_disk_side(side);
    }

    /// Number of selectable NSF songs (0 for cartridge / disk builds).
    #[must_use]
    pub fn nsf_song_count(&self) -> u8 {
        self.mapper.nsf_song_count()
    }

    /// The currently-selected 0-based NSF song (0 for cartridge / disk builds).
    #[must_use]
    pub fn nsf_current_song(&self) -> u8 {
        self.mapper.nsf_current_song()
    }

    /// Select a 0-based NSF song. Returns `true` if this is an NSF build (so the
    /// caller re-runs the reset that re-enters the driver's `init`).
    pub fn nsf_set_song(&mut self, song: u8) -> bool {
        self.mapper.nsf_set_song(song)
    }

    /// Start recording the diagnostic FDS read-stream trace (off by default;
    /// observation-only). No-op on cartridge builds.
    pub fn enable_fds_trace(&mut self) {
        self.mapper.enable_fds_trace();
    }

    /// Drain the accumulated FDS read-stream trace records (empty for cartridge
    /// builds / when tracing was never enabled).
    pub fn take_fds_trace(&mut self) -> Vec<rustynes_mappers::FdsTraceRec> {
        self.mapper.take_fds_trace()
    }

    /// Re-serialize the (possibly-modified) FDS disk image to its byte layout
    /// for host persistence. Empty for cartridge builds.
    #[must_use]
    pub fn disk_image_bytes(&self) -> Vec<u8> {
        self.mapper.disk_image_bytes()
    }

    /// Whether the FDS disk image has unsaved writes.
    #[must_use]
    pub fn disk_is_dirty(&self) -> bool {
        self.mapper.disk_is_dirty()
    }

    /// Clear the FDS disk dirty flag (after the host persists the image).
    pub fn clear_disk_dirty(&mut self) {
        self.mapper.clear_disk_dirty();
    }

    /// Mark the inserted FDS disk read-only (`true`) or writable (`false`).
    pub fn set_disk_write_protected(&mut self, protected: bool) {
        self.mapper.set_disk_write_protected(protected);
    }

    /// Commit a controller-strobe write to all controllers, resetting the
    /// Four Score read sequence + reloading its signature when enabled.
    const fn commit_controller_strobe(&mut self, value: u8) {
        self.controllers[0].write_strobe(value);
        self.controllers[1].write_strobe(value);
        // Forward the strobe to any attached overlay device (only the Vaus
        // latches on it; the Zapper ignores it). Done unconditionally — the
        // standard controllers above are still strobed, so detaching a device
        // returns to byte-identical behavior.
        if let Some(d) = &mut self.expansion_device[0] {
            d.write_strobe(value);
        }
        if let Some(d) = &mut self.expansion_device[1] {
            d.write_strobe(value);
        }
        if self.four_score {
            self.controllers34[0].write_strobe(value);
            self.controllers34[1].write_strobe(value);
            // Reset the 24-read sequence + reload the adapter signature
            // (port 0 = 0x08, port 1 = 0x04, shifted out LSB-first).
            self.four_score_idx = [0, 0];
            self.four_score_sig = [0x08, 0x04];
        }
    }

    /// Read the D0 controller bit for `port` (0 = `$4016`, 1 = `$4017`),
    /// advancing the shift register. Four Score off → just
    /// `controllers[port].read()`; on → the multiplexed 24-read sequence
    /// (primary pad → secondary pad → signature → 1s).
    const fn read_port(&mut self, port: usize) -> u8 {
        // v1.6.0 Workstream A3 (`TAStudio` lag log): any read of $4016/$4017
        // counts as the game polling input this frame. Output-only; gated.
        #[cfg(feature = "debug-hooks")]
        {
            self.controller_polled = true;
        }
        // A non-standard overlay device takes over the port entirely: it
        // returns its own bit-positioned byte (Vaus = bits 3/4, Zapper =
        // bits 3/4) instead of the standard D0 shift-register bit. The
        // standard controller is still strobed (in `commit_controller_strobe`)
        // so detaching the device restores byte-identical behavior.
        if let Some(d) = &mut self.expansion_device[port] {
            return d.read();
        }
        if !self.four_score || self.controllers[port].strobe {
            return self.controllers[port].read();
        }
        let idx = self.four_score_idx[port];
        let bit = if idx < 8 {
            self.controllers[port].read()
        } else if idx < 16 {
            self.controllers34[port].read()
        } else if idx < 24 {
            let b = self.four_score_sig[port] & 1;
            self.four_score_sig[port] = (self.four_score_sig[port] >> 1) | 0x80;
            b
        } else {
            1
        };
        if idx < 24 {
            self.four_score_idx[port] += 1;
        }
        bit
    }

    /// Side-effect-free companion to [`Self::read_port`] (debugger peek).
    const fn peek_port(&self, port: usize) -> u8 {
        if let Some(d) = &self.expansion_device[port] {
            return d.peek();
        }
        if !self.four_score || self.controllers[port].strobe {
            return self.controllers[port].peek();
        }
        let idx = self.four_score_idx[port];
        if idx < 8 {
            self.controllers[port].peek()
        } else if idx < 16 {
            self.controllers34[port].peek()
        } else if idx < 24 {
            self.four_score_sig[port] & 1
        } else {
            1
        }
    }

    /// Bus-side bookkeeping snapshot used by `bus_snapshot::encode_bus`.
    #[must_use]
    pub const fn bus_misc_state(&self) -> crate::bus_snapshot::BusMiscState {
        crate::bus_snapshot::BusMiscState {
            dma_pending: self.dma_pending,
            dma_cycles_owed: self.dma_cycles_owed,
            dma_byte: self.dma_byte,
            dma_idx: self.dma_idx,
            dma_page: self.dma_page,
            dma_halt_addr: self.dma_halt_addr,
            last_nmi_level: self.last_nmi_level,
            nmi_edge_latch: self.nmi_edge_latch,
            open_bus: self.open_bus,
            last_read_addr: self.last_read_addr,
            deferred_dma_replay_addr: self.deferred_dma_replay_addr,
            in_dmc_dma: self.in_dmc_dma,
            controller_write_pending: self.controller_write_pending,
            controller_write_value: self.controller_write_value,
            four_score: self.four_score,
            four_score_idx: self.four_score_idx,
            four_score_sig: self.four_score_sig,
            // W3-Stage-4 (2026-06-10): the unified-engine OAM state + the
            // DMC halt latch. Always present in the ferry struct (zeros when
            // the engine feature is off) so the BUS section layout is
            // identical across feature builds.
            dmc_halt: self.dmc_halt,
            uni_oam_active: self.uni_oam_active,
            uni_oam_halt: self.uni_oam_halt,
            uni_oam_aligned: self.uni_oam_aligned,
            uni_oam_addr: self.uni_oam_addr,
            ppu_clock: self.ppu_clock,
            dma_mc_consumed: self.dma_mc_consumed,
        }
    }

    /// Apply a previously-snapshotted bus bookkeeping state.
    pub const fn set_bus_misc_state(&mut self, s: crate::bus_snapshot::BusMiscState) {
        self.dma_pending = s.dma_pending;
        self.dma_cycles_owed = s.dma_cycles_owed;
        self.dma_byte = s.dma_byte;
        self.dma_idx = s.dma_idx;
        self.dma_page = s.dma_page;
        self.dma_halt_addr = s.dma_halt_addr;
        self.last_nmi_level = s.last_nmi_level;
        self.nmi_edge_latch = s.nmi_edge_latch;
        self.open_bus = s.open_bus;
        self.last_read_addr = s.last_read_addr;
        self.deferred_dma_replay_addr = s.deferred_dma_replay_addr;
        self.in_dmc_dma = s.in_dmc_dma;
        self.controller_write_pending = s.controller_write_pending;
        self.controller_write_value = s.controller_write_value;
        self.four_score = s.four_score;
        self.four_score_idx = s.four_score_idx;
        self.four_score_sig = s.four_score_sig;
        // W3-Stage-4 (2026-06-10): the unified engine's OAM state + the DMC
        // halt latch are now serialized (trailing-default-zero in the BUS
        // section), replacing the Stage-1 clear-on-restore. Snapshots are
        // taken at instruction boundaries where the engine is idle, so for
        // every legitimately produced blob these decode to the same inactive
        // state the clear imposed -- but a restored blob now reproduces them
        // EXACTLY instead of by assumption.
        {
            self.dmc_halt = s.dmc_halt;
        }
        {
            self.uni_oam_active = s.uni_oam_active;
            self.uni_oam_halt = s.uni_oam_halt;
            self.uni_oam_aligned = s.uni_oam_aligned;
            self.uni_oam_addr = s.uni_oam_addr;
        }
        // The R1 substrate master-clock pair (see `BusMiscState::ppu_clock`):
        // pre-Stage-4 blobs decode these as 0, which together with the CPU
        // v1-blob `master_clock` upconvert keeps the pair coherent.
        {
            self.ppu_clock = s.ppu_clock;
            self.dma_mc_consumed = s.dma_mc_consumed;
        }
    }

    /// Set the cumulative CPU cycle counter (used by save-state restore).
    pub const fn set_cycle(&mut self, cycle: u64) {
        self.cycle = cycle;
    }

    /// Overwrite the 2 KiB CPU RAM.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::SectionInvalid`] if `bytes.len() != 2048`.
    pub fn set_ram_bytes(&mut self, bytes: &[u8]) -> Result<(), SnapshotError> {
        if bytes.len() != self.ram.len() {
            return Err(SnapshotError::SectionInvalid {
                tag: "BUS ".into(),
                reason: format!("ram length {} != {}", bytes.len(), self.ram.len()),
            });
        }
        self.ram.copy_from_slice(bytes);
        Ok(())
    }

    /// Overwrite both controllers' state.
    pub const fn set_controllers(&mut self, controllers: [Controller; 2]) {
        self.controllers = controllers;
    }

    /// Overwrite the Four Score players 3 & 4 (save-state restore).
    pub const fn set_controllers34(&mut self, controllers: [Controller; 2]) {
        self.controllers34 = controllers;
    }

    /// Write a byte directly into CPU work RAM (`$0000-$1FFF`, mirrored every
    /// `$800`). Used by the frontend's raw RAM cheats (GameShark-style),
    /// applied caller-side *after* [`crate::Nes::run_frame`] so the core run
    /// loop stays pure (the determinism contract is unperturbed for the
    /// no-cheat path). No-op for addresses outside system RAM.
    pub fn poke_ram(&mut self, addr: u16, value: u8) {
        if addr < 0x2000 {
            self.ram[(addr & 0x07FF) as usize] = value;
        }
    }

    /// Encode the entire bus + chip state into a `.rns` snapshot.
    ///
    /// Returns the bytes the caller should persist via
    /// `frontend::save_state` (or feed into the rewind ring).
    ///
    /// The output is bit-deterministic: same `(seed, ROM, input sequence)`
    /// produces identical bytes.
    #[must_use]
    pub fn snapshot(&self, rom_hash_tag: [u8; save_state::ROM_HASH_TAG_LEN]) -> Vec<u8> {
        let mut out = Vec::with_capacity(0x4_0000);
        self.snapshot_into(&mut out, rom_hash_tag);
        out
    }

    /// v2.8.0 Phase 3 — [`Self::snapshot`] into a caller-owned buffer
    /// (cleared first; capacity reused across calls). The per-call
    /// allocation of the ~250 KiB blob matters to per-frame consumers
    /// (run-ahead, the netplay save-state ring, rewind).
    pub fn snapshot_into(
        &self,
        out: &mut Vec<u8>,
        rom_hash_tag: [u8; save_state::ROM_HASH_TAG_LEN],
    ) {
        out.clear();
        save_state::write_header(out, rom_hash_tag);

        // BUS section.
        let bus_body = crate::bus_snapshot::encode_bus(self);
        save_state::write_section(
            out,
            save_state::tag::BUS,
            crate::bus_snapshot::BUS_SECTION_VERSION,
            &bus_body,
        );

        // CPU is owned by the surrounding `Nes` facade — but the bus is
        // the canonical owner of the persistable state, so the public
        // `snapshot` lives there. The CPU section is appended by
        // `Nes::snapshot` because the CPU isn't reachable from inside
        // the bus without violating the dependency graph. We stub
        // section emission here; `Nes::snapshot` will re-call this and
        // splice the CPU bytes in.

        // PPU section.
        let ppu_body = self.ppu.snapshot();
        save_state::write_section(
            out,
            save_state::tag::PPU,
            rustynes_ppu::PPU_SNAPSHOT_VERSION,
            &ppu_body,
        );

        // APU section.
        let apu_body = self.apu.snapshot();
        save_state::write_section(
            out,
            save_state::tag::APU,
            rustynes_apu::APU_SNAPSHOT_VERSION,
            &apu_body,
        );

        // MAP section (mapper-resident state).
        let map_body = self.mapper.save_state();
        save_state::write_section(out, save_state::tag::MAP, 1, &map_body);
    }

    /// Apply a previously snapshotted blob *to the bus and chips*. The CPU
    /// is restored separately by [`crate::Nes::restore`].
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError`] for unknown sections, version mismatches,
    /// or malformed bodies.
    pub fn restore(&mut self, data: &[u8]) -> Result<(), SnapshotError> {
        let (header, body_off) = save_state::parse_header(data)?;
        let _ = header; // currently informational
        let mut saw_bus = false;
        let mut saw_ppu = false;
        let mut saw_apu = false;
        let mut saw_map = false;
        for s in save_state::SectionIter::new(&data[body_off..]) {
            let s = s?;
            match s.tag {
                save_state::tag::BUS => {
                    if s.version != crate::bus_snapshot::BUS_SECTION_VERSION {
                        return Err(SnapshotError::VersionMismatch {
                            tag: save_state::tag_string(s.tag),
                            file_version: s.version,
                            chip_supports: crate::bus_snapshot::BUS_SECTION_VERSION,
                        });
                    }
                    crate::bus_snapshot::decode_bus(self, s.body)?;
                    saw_bus = true;
                }
                save_state::tag::PPU => {
                    if s.version != rustynes_ppu::PPU_SNAPSHOT_VERSION {
                        return Err(SnapshotError::VersionMismatch {
                            tag: save_state::tag_string(s.tag),
                            file_version: s.version,
                            chip_supports: rustynes_ppu::PPU_SNAPSHOT_VERSION,
                        });
                    }
                    self.ppu.restore(s.body).map_err(|e: PpuSnapshotError| {
                        SnapshotError::SectionInvalid {
                            tag: save_state::tag_string(s.tag),
                            reason: format!("{e}"),
                        }
                    })?;
                    saw_ppu = true;
                }
                save_state::tag::APU => {
                    if s.version != rustynes_apu::APU_SNAPSHOT_VERSION {
                        return Err(SnapshotError::VersionMismatch {
                            tag: save_state::tag_string(s.tag),
                            file_version: s.version,
                            chip_supports: rustynes_apu::APU_SNAPSHOT_VERSION,
                        });
                    }
                    self.apu.restore(s.body).map_err(|e: ApuSnapshotError| {
                        SnapshotError::SectionInvalid {
                            tag: save_state::tag_string(s.tag),
                            reason: format!("{e}"),
                        }
                    })?;
                    saw_apu = true;
                }
                save_state::tag::MAP => {
                    self.mapper.load_state(s.body).map_err(|e: MapperError| {
                        SnapshotError::SectionInvalid {
                            tag: save_state::tag_string(s.tag),
                            reason: format!("{e}"),
                        }
                    })?;
                    saw_map = true;
                }
                save_state::tag::CPU => {
                    // Skipped — restored by the surrounding `Nes` facade.
                }
                _other => {
                    // Unknown tags are forward-compatible: skip silently
                    // so cross-version files load when they include
                    // sections this build doesn't know about (e.g. a
                    // future "DBG " debugger section).
                }
            }
        }
        // BUS is mandatory; chip sections are mandatory too because
        // they round-trip the entire emulator state.
        if !saw_bus {
            return Err(SnapshotError::MissingSection("BUS ".into()));
        }
        if !saw_ppu {
            return Err(SnapshotError::MissingSection("PPU ".into()));
        }
        if !saw_apu {
            return Err(SnapshotError::MissingSection("APU ".into()));
        }
        if !saw_map {
            return Err(SnapshotError::MissingSection("MAP ".into()));
        }
        // RW-0 fix: under R1, `dmc_driven_externally` is NOT serialized (it is
        // build configuration, not emulated state), so after `apu.restore` it
        // reverts to the `Apu::new` default (`false`), which STOPS `put_cycle`
        // toggling and disables the interleaved DMC DMA service — a latent R1
        // save-state correctness bug. Re-apply the R1 drive here exactly as
        // `new`/`reset`/`power_cycle` do.
        //
        // W3-Stage-4 (2026-06-10, the RW-3 follow-through): the APU snapshot
        // now DOES carry the exact `put_cycle` / `parity_seed` phase in its
        // Stage-4 tail, so re-seed the boot alignment ONLY for pre-Stage-4
        // blobs that lack the tail (`snapshot_restored_parity` is false) —
        // otherwise the boot seed would overwrite the restored mid-state
        // parity that the counter-collapse end-flip reads at the next access
        // point.
        {
            self.apu.set_dmc_driven_externally(true);
            if !self.apu.snapshot_restored_parity() {
                self.apu.seed_apu_alignment(0);
            }
        }
        Ok(())
    }

    const fn ppu_region(&self) -> PpuRegion {
        match self.cart.region {
            rustynes_mappers::Region::Pal => PpuRegion::Pal,
            rustynes_mappers::Region::Dendy => PpuRegion::Dendy,
            _ => PpuRegion::Ntsc,
        }
    }

    const fn apu_region(&self) -> ApuRegion {
        match self.cart.region {
            rustynes_mappers::Region::Pal => ApuRegion::Pal,
            rustynes_mappers::Region::Dendy => ApuRegion::Dendy,
            _ => ApuRegion::Ntsc,
        }
    }

    /// Drive the PPU forward 3 dots and account for one CPU cycle of
    /// bookkeeping (mapper-cycle hook, DMA progress, NMI edge sample,
    /// APU tick).
    #[allow(clippy::too_many_lines)] // Session-21 added per-cycle DMC + bus-access snapshots; splitting the trace push into a helper would force the bus to recompute `trace_*_pre_tick` values across function boundaries.
    pub(crate) fn tick_one_cpu_cycle(&mut self) {
        // Tick PPU 3 dots in NTSC.  PAL would be 3.2 (5 dots per 16 PPU dots);
        // we approximate as 3 for now and gate region accuracy behind a
        // future Phase 2 follow-up.
        //
        // Sample the PPU /NMI line state *between every dot* so a glitched
        // edge that goes low->high then back to low within a single CPU
        // cycle (e.g. PPUCTRL.7 set during pre-render dot 0, then VBL
        // cleared at dot 1 within the same CPU cycle) is still latched.
        //
        // When the `irq-timing-trace` cargo feature is enabled, capture
        // per-cycle (cpu_cycle, ppu_scanline, ppu_dot, a12_events, IRQ
        // lines sampled at TWO points within the cycle, NMI line) into
        // the bus's trace buffer.
        //
        // Phase A of the C1 plan (`docs/adr/0002-irq-timing-coordination.md`)
        // takes TWO IRQ snapshots per CPU cycle so the M2-low → M2-high
        // asymmetry the coordinated change is designed to model is
        // observable in the trace data:
        //
        //   * M2-low snapshot:  taken AFTER PPU sub-dot 0 has ticked.
        //     This catches any A12 transition / APU IRQ assertion that
        //     happened on the cycle's first PPU dot, but before sub-dots
        //     1 and 2 have run.
        //   * M2-high snapshot: taken AFTER PPU sub-dot 2 has ticked,
        //     i.e. at the end-of-3-PPU-dots boundary, BEFORE
        //     `notify_cpu_cycle` / `tick_with_external` run.  This is
        //     the historical query point the pre-Phase-B2
        //     `Bus::poll_irq` impl used when called from
        //     `Cpu::idle_tick` after `bus.on_cpu_cycle()` returned.
        //
        // The conventional names map to silicon's φ1 / φ2 halves of the
        // 6502 cycle.  The exact sub-dot placement is conventional, not
        // canonical — what matters is that the bus records IRQ state at
        // TWO distinct points within the cycle so downstream phases can
        // diff them.
        //
        // Phase B2 of the C1 IRQ-timing rework: the M2-low and M2-high
        // snapshots are now stored on `self` unconditionally (not gated
        // on the `irq-timing-trace` feature).  The trace fixture's
        // `_at_low` / `_at_high` columns read from these snapshots
        // rather than re-querying the mapper / APU, removing the
        // duplicate `mapper.irq_pending()` call that Phase A introduced
        // inside the cycle.  The production `Bus::poll_irq` /
        // `Bus::poll_irq_at_phase` paths on `LockstepBus` also read
        // from these snapshots — see the `impl Bus for LockstepBus`
        // block below.
        // Session-24 / Phase 3 (Controller Strobing): commit any
        // pending controller-strobe write at the START of this CPU
        // cycle (M2-low boundary).  Mirrors Mesen2's
        // `NesConsole::ProcessCpuClock` → `NesControlManager::ProcessWrites`
        // call site (`Core/NES/NesConsole.cpp` line 72).  See
        // `docs/audit/session-24-phase3-controller-strobing-2026-05-23.md`.
        if self.controller_write_pending > 0 {
            self.controller_write_pending -= 1;
            if self.controller_write_pending == 0 {
                let value = self.controller_write_value;
                // The strobe line is shared between both controllers.
                self.commit_controller_strobe(value);
            }
        }
        #[cfg(feature = "irq-timing-trace")]
        let (trace_scanline_start, trace_dot_start, trace_frame_start) =
            (self.ppu.scanline(), self.ppu.dot(), self.ppu.frame());
        // Session-21 (Sprint 1 iteration 2 prereq): snapshot the DMC
        // scheduler's "pre-tick" state (mirrors `_at_low` for the IRQ
        // columns).  These read BEFORE `apu.tick_with_external` runs at
        // the bottom of this method.
        #[cfg(feature = "irq-timing-trace")]
        let trace_dmc_dma_pending_pre = self.apu.dmc_dma_pending();
        // M2-phase tracking (Phase B1 of the C1 IRQ-timing rework):
        // each CPU cycle begins in `M2Phase::Low`, transitions to
        // `M2Phase::High` after sub-dot 1 has ticked (the M2-rising
        // boundary), and resets to `Low` at end-of-cycle.
        self.m2_phase = M2Phase::Low;
        #[cfg(not(feature = "irq-timing-trace"))]
        for sub_dot in 0..3u8 {
            let mut adapter = PpuBusAdapter {
                mapper: self.mapper.as_mut(),
                nt_override: self.nt_mirroring_override,
                sub_dot,
            };
            self.ppu.tick(&mut adapter);
            self.sample_nmi_edge();
            if sub_dot == 0 {
                // M2-low IRQ snapshot.
                self.irq_snapshot_mapper_at_low = self.mapper.irq_pending();
                self.irq_snapshot_apu_at_low = self.apu.irq_line();
            }
            if sub_dot == 1 {
                self.m2_phase = M2Phase::High;
            }
        }
        #[cfg(feature = "irq-timing-trace")]
        for sub_dot in 0..3u8 {
            let mut adapter = PpuBusAdapter {
                mapper: self.mapper.as_mut(),
                nt_override: self.nt_mirroring_override,
                sub_dot,
                trace_a12_latest: if self.irq_trace.is_some() {
                    Some(&mut self.trace_a12_latest)
                } else {
                    None
                },
            };
            self.ppu.tick(&mut adapter);
            self.sample_nmi_edge();
            if self.irq_trace.is_some() {
                if let Some(level) = self.trace_a12_latest.take() {
                    if let Some(t) = self.irq_trace.as_mut() {
                        t.notify_a12_count = t.notify_a12_count.saturating_add(1);
                    }
                    // The PPU already filters to transitions only; every
                    // `notify_a12` call IS a level change.  Record it.
                    self.trace_a12_scratch.push(A12Event { sub_dot, level });
                    self.trace_last_a12 = level;
                }
            }
            if sub_dot == 0 {
                // M2-low IRQ snapshot: taken AFTER sub-dot 0 has ticked
                // so it reflects the dot's mapper-side effects (e.g. an
                // A12 rise on sub-dot 0 that just clocked the MMC3 IRQ
                // counter).  Sub-dots 1 and 2 have not yet run.
                self.irq_snapshot_mapper_at_low = self.mapper.irq_pending();
                self.irq_snapshot_apu_at_low = self.apu.irq_line();
            }
            if sub_dot == 1 {
                self.m2_phase = M2Phase::High;
            }
        }

        // Phase-A-compatible end-of-3-PPU-dots snapshot — taken BEFORE
        // `notify_cpu_cycle` / `tick_with_external` advance the mapper
        // and APU.  Only used by the trace fixture's `_at_high` column
        // so the Phase A baseline CSV files stay byte-identical across
        // Phases B2+.  The production `Bus::poll_irq{,_at_phase}` path
        // reads from `irq_snapshot_*_at_high` below, taken AFTER those
        // advance, so the CPU's IRQ sample point is unchanged.
        #[cfg(feature = "irq-timing-trace")]
        let trace_mapper_at_high_pre_tick = self.mapper.irq_pending();
        #[cfg(feature = "irq-timing-trace")]
        let trace_apu_at_high_pre_tick = self.apu.irq_line();

        // End-of-cycle: the bus advances to the next CPU cycle, which
        // (re)starts in `M2Phase::Low`.  Reset BEFORE the cycle counter
        // increment so any future read of `current_m2_phase()` from
        // inside `notify_cpu_cycle` / `tick_with_external` sees the new
        // cycle's phase rather than the previous cycle's tail.
        self.m2_phase = M2Phase::Low;
        self.cycle = self.cycle.wrapping_add(1);
        self.ppu.on_cpu_cycle();
        self.mapper.notify_cpu_cycle();
        // Sample the mapper's audio extension AFTER notify_cpu_cycle has
        // advanced its oscillators. `Mapper::mix_audio` returns i16; we
        // scale to approximately the same [-0.5, 0.5] range as the APU
        // mixer's own output. Mappers without on-cart audio return 0,
        // which scales to 0.0 -- a no-op for the standard cartridges.
        let mapper_sample = f32::from(self.mapper.mix_audio()) / 65536.0;
        self.apu.tick_with_external(mapper_sample);
        // Fan-out the APU frame-counter events to any on-cart audio
        // extension that shares the 2A03 frame-counter cadence (MMC5).
        // Default no-op for all other mappers.
        let ev = self.apu.last_frame_events();
        self.mapper.notify_frame_event(MapperFrameEvents {
            quarter: ev.quarter,
            half: ev.half,
        });

        // M2-high IRQ snapshot: at the VERY END of `tick_one_cpu_cycle`,
        // AFTER `notify_cpu_cycle` / `tick_with_external` /
        // `notify_frame_event` have run.  This matches the historical
        // `mapper.irq_pending() || apu.irq_line()` query point that
        // `Cpu::idle_tick` saw when it called `bus.poll_irq()` after
        // `bus.on_cpu_cycle()` returned — so the production
        // `Bus::poll_irq` / `poll_irq_at_phase(M2Phase::High)` paths
        // stay semantically identical to the pre-Phase-B2 direct query
        // of `mapper.irq_pending() || apu.irq_line()`.
        self.irq_snapshot_mapper_at_high = self.mapper.irq_pending();
        self.irq_snapshot_apu_at_high = self.apu.irq_line();

        #[cfg(feature = "irq-timing-trace")]
        if self.irq_trace.is_some() {
            let events = core::mem::take(&mut self.trace_a12_scratch);
            let events_len = events.len();
            // Session-21: snapshot the DMC scheduler "post-tick" state
            // and consume the per-cycle bus-access tracker.  These are
            // taken AFTER `apu.tick_with_external` has run for this
            // cycle, so they reflect the end-of-cycle scheduler shape
            // that the next CPU cycle's bus access will observe.
            let bus_access = core::mem::replace(&mut self.trace_bus_access, BusAccess::Idle);
            let bus_addr = core::mem::take(&mut self.trace_bus_addr);
            let bus_data = core::mem::take(&mut self.trace_bus_data);
            let rec = CycleRecord {
                // `cpu_cycle` here refers to the cycle we JUST ticked.
                // `self.cycle` was incremented above, so subtract 1.
                cpu_cycle: self.cycle.wrapping_sub(1),
                pc: self.trace_last_pc,
                ppu_scanline: trace_scanline_start,
                ppu_dot: trace_dot_start,
                ppu_frame: trace_frame_start,
                irq_pending_mapper_at_low: self.irq_snapshot_mapper_at_low,
                irq_pending_apu_at_low: self.irq_snapshot_apu_at_low,
                // Trace's `_at_high` columns retain the Phase A
                // pre-tick_with_external semantics so the committed
                // baseline CSVs in
                // `crates/rustynes-test-harness/golden/irq_trace/` stay
                // byte-identical.  Production `poll_irq` reads from
                // the post-tick `irq_snapshot_*_at_high` fields above
                // instead.
                irq_pending_mapper_at_high: trace_mapper_at_high_pre_tick,
                irq_pending_apu_at_high: trace_apu_at_high_pre_tick,
                nmi_line: self.ppu.nmi_line(),
                a12_events: events,
                // --- Session-21 DMC + bus-access columns ---
                dmc_dma_pending_pre: trace_dmc_dma_pending_pre,
                dmc_dma_pending_post: self.apu.dmc_dma_pending(),
                dmc_dma_short_post: self.apu.dmc_dma_short(),
                dmc_abort_pending_post: self.apu.dmc_abort_pending(),
                dmc_abort_delay_post: self.apu.dmc_abort_delay(),
                dmc_dma_cooldown_post: self.apu.dmc_dma_cooldown(),
                dmc_dma_delay_post: self.apu.dmc_dma_delay(),
                apu_phase_post: self.apu.apu_phase(),
                in_dmc_dma: self.in_dmc_dma,
                dma_cycles_owed: self.dma_cycles_owed,
                bus_access,
                bus_addr,
                bus_data,
                put_cycle_post: self.apu.put_cycle(),
                dmc_timer_post: self.apu.dmc_timer(),
                dmc_bits_remaining_post: self.apu.dmc_bits_remaining(),
                dmc_silence_post: self.apu.dmc_silence(),
                dmc_buffer_full_post: self.apu.dmc_buffer_full(),
            };
            if let Some(t) = self.irq_trace.as_mut() {
                if events_len > 0 {
                    t.records_with_a12_count = t.records_with_a12_count.saturating_add(1);
                }
                t.push(rec);
            }
        }
        // v2.0 R1 DMA-coherence (Phase 3): under `mc-r1-substrate` this fn is
        // reached ONLY from the bus-side DMA path — the normal R1 cycle runs
        // the PPU via `run_ppu_to` + does its per-cycle work in `cpu_clock`,
        // which does NOT call this. Each DMA cycle ticked the real PPU by 3
        // dots without advancing `master_clock`/`ppu_clock`. Bump `ppu_clock`
        // so the next `run_ppu_to` does not RE-tick those dots, and
        // `dma_mc_consumed` so `Cpu::end_cycle` folds the DMA span into
        // `master_clock` — keeping the CPU<->PPU phase coherent across DMA
        // (the v2.0-R1 regression this prevents). Mirrors `dma_tick_one_cycle`
        // on `refactor/v2.0-master-clock`.
        {
            let (cpu_div, ppu_div) = self.region_dividers();
            // The PPU was physically ticked 3 dots by this DMA cycle, so
            // `ppu_clock` advances by exactly `3 * ppu_divider` mc — keeping the
            // boundary check in `run_ppu_to` from re-ticking those dots.
            self.ppu_clock = self.ppu_clock.wrapping_add(u64::from(ppu_div) * 3);
            // `master_clock` (via `dma_mc_consumed`) advances by the region's
            // true CPU-cycle span (`cpu_divider`). On NTSC/Dendy this equals
            // `3 * ppu_divider` (12/15), so the path is byte-identical; on PAL
            // (16 vs 15) the 1-mc/cycle deficit accumulates and the next
            // `run_ppu_to` ticks the catch-up dot, yielding the correct 3.2:1
            // average across the DMA span.
            self.dma_mc_consumed = self.dma_mc_consumed.wrapping_add(u64::from(cpu_div));
        }
    }

    /// Capture the PPU /NMI line transition (false → true) into the edge
    /// latch consumed by [`Bus::poll_nmi`].  Idempotent within a "still
    /// asserted" window: only the rising edge latches.
    const fn sample_nmi_edge(&mut self) {
        let level = self.ppu.nmi_line();
        if level && !self.last_nmi_level {
            self.nmi_edge_latch = true;
        }
        self.last_nmi_level = level;
    }

    /// Drain any cycles owed to the DMA controllers (OAM DMA + DMC DMA)
    /// before completing a CPU access.
    ///
    /// Called from `cpu_read` and `cpu_write`. DMC DMA preempts OAM DMA per
    /// nesdev: while OAM DMA is running and a DMC DMA also fires, the DMC
    /// fetch happens "between the dummy and DMA reads" of the OAM DMA,
    /// stalling the OAM transfer for 3-4 extra cycles.  Our simpler model
    /// services DMC DMA at the start of each cycle the bus controls; if
    /// OAM DMA is in flight, the DMC DMA inserts itself between transfer
    /// pairs.
    // Under `mc-r1-full-cpu` the body reduces to the DMC-abort/idle handling;
    // OAM moved to the CPU-driven `oam_dma_step`, so `&mut self`/`read_addr` are
    // only lightly used here — silence the resulting lints under the flag.
    #[allow(
        clippy::unused_self,
        clippy::needless_pass_by_ref_mut,
        clippy::missing_const_for_fn
    )]
    fn drain_dma(&mut self, read_addr: Option<u16>) {
        // Stage-D: under `mc-r1-full-cpu` the OAM DMA is CPU-driven (read1), so
        // the legacy OAM block below (the only user of `read_addr` once the
        // abort-cancel path owns aborts) is cfg'd out — silence the param.
        let _ = read_addr;
        // Sprint 3 iter 3 — under the `dmc-get-put-scheduler`
        // feature, the abort is handled INSIDE `service_dmc_dma` /
        // `service_dmc_dma_during_oam` (matching Mesen2's unified
        // `RunDma` loop where the `processCycle` lambda checks
        // `_abortDmcDma` per iteration). The pre-service abort
        // call is preserved on the default-off path.
        // accuracycoin-100 Phase 2: under `mc-r1-dmc-abort-cancel` the R1
        // read1/write1 path OWNS the abort (get-cycle 1-halt Y=1 / put-write
        // cancel Y=0). Skip the legacy `drain_dma` service — `drain_dma(None)`
        // runs every R1 `cpu_clock` cycle and would `complete_dmc_abort` the
        // pending abort BEFORE the read1 hook can see it (the inert-as-placed
        // bug). The legacy service below stays active for the default build.
        // Stage-D (`mc-r1-full-cpu`): OAM DMA is CPU-driven in `read1`
        // (`oam_dma_step`), NOT bus-side burst — leave `dma_pending` set for the
        // read1 loop to consume; drain_dma does no OAM work under the flag.
    }

    fn clock_oam_dma_cycle(&mut self, total: u32, alignment: u32) {
        let consumed = total - self.dma_cycles_owed; // 0, 1, ...
        if consumed < alignment {
            // OAM DMA halt / alignment cycle — bus idle for the CPU,
            // but the DMA controller owns it.  Trace as DmaRead with
            // the halted CPU read address so the trace shows what the
            // open-bus latch ended up driving (Session-21).
            #[cfg(feature = "irq-timing-trace")]
            self.set_trace_dma_access(BusAccess::DmaRead, self.dma_halt_addr, self.open_bus);
            self.tick_one_cpu_cycle();
            self.dma_cycles_owed -= 1;
            return;
        }
        let xfer_idx = consumed - alignment; // 0..512
        // Even xfer index: read; odd: write.
        if xfer_idx & 1 == 0 {
            let src_addr =
                (u16::from(self.dma_page) << 8) | u16::try_from(xfer_idx >> 1).unwrap_or(0);
            self.dma_byte = self.raw_oam_dma_read(src_addr);
            #[cfg(feature = "irq-timing-trace")]
            self.set_trace_dma_access(BusAccess::DmaRead, src_addr, self.dma_byte);
        } else {
            self.oam_dma_put();
            #[cfg(feature = "irq-timing-trace")]
            self.set_trace_dma_access(BusAccess::DmaWrite, 0x2004, self.dma_byte);
        }
        self.tick_one_cpu_cycle();
        self.dma_cycles_owed -= 1;
    }

    /// OAM-DMA source fetch (Session-26 / Sprint 2 iter 4).
    ///
    /// The 2A03 has three internal address buses (6502, OAM DMA, DMC
    /// DMA), but only the 6502 bus asserts the APU/controller chip
    /// select. During OAM DMA the 6502 is halted, so its bus is parked
    /// at `self.dma_halt_addr` (last CPU read address). The OAM DMA
    /// engine drives the EXTERNAL address bus with `src_addr`, but the
    /// APU registers' `CHIP_SELECT` is gated on `6502_addr ∈ $4000-$401F`,
    /// not on the DMA's source page.
    ///
    /// Consequence: if the 6502 bus is parked outside `$4000-$401F`
    /// and the OAM DMA reads a source address inside that range, the
    /// APU/controllers are silent — the read returns the open-bus
    /// latch and triggers no register side-effects (no `apu.read_status()`,
    /// no controller shift, etc.). The DMC DMA helper already implements
    /// the equivalent gate (`dmc_dma_read` lines 1329-1356).
    ///
    /// `AccuracyCoin` `APU Register Activation` Test 4 (asm:8091-8109)
    /// exercises this: `LDA #$40; STA $4014` runs an OAM DMA from page
    /// `$40` while CPU code lives in PRG ROM. Without this gate, the
    /// DMA's `$4015` read clears the frame-counter IRQ flag, failing
    /// the subsequent `LDA $4015 / AND #$40 / BEQ FAIL` check.
    ///
    /// The Test 5/6 conflict-path semantics (where the 6502 bus IS in
    /// `$4000-$401F` because the test uses `JSR $3FFE` + the BRK trick)
    /// need additional modelling — deferred. Two components, established by
    /// the 2026-06-05 investigation (`docs/audit/`):
    /// 1. **Active-window mirror decode.** When the 6502 bus is parked in
    ///    `$4000-$401F`, an OAM DMA reading page `$40` reads the readable
    ///    registers (`$4015`/`$4016`/`$4017`) AND their `$20`-byte mirrors:
    ///    the 2A03 decodes on the low 5 address bits, so `$4020-$40FF` mirror
    ///    `$4000-$401F` (`$4035` -> `$4015`) with side-effects (`$4015` clears
    ///    the frame IRQ flag; `$4016`/`$4017` advance the controller shift).
    ///    The fix is to mask `src` to `0x4000 | (src & 0x1F)` here when active.
    /// 2. **Upstream coupling (the actual blocker).** This is NOT independently
    ///    reachable: Test 6's OAM-copy is all-zeros in this emulator because the
    ///    page-`$40` register-read OAM DMA does not fire as the test intends — it
    ///    depends on Test 5's `[DMC DMA! Overwrite data bus with $40]` trick
    ///    landing cycle-exactly so `STA $4014` reads `$40` and the 6502 bus is
    ///    parked in `$40xx` during the DMA. That is the deferred DMC-DMA-timing /
    ///    data-bus axis. So component 1 is correct hardware behavior but inert
    ///    until that axis lands — do NOT add it speculatively (it touches the
    ///    default build and cannot be verified against the test in isolation).
    fn raw_oam_dma_read(&mut self, src_addr: u16) -> u8 {
        if (self.dma_halt_addr & 0xFFE0) != 0x4000 && (src_addr & 0xFFE0) == 0x4000 {
            // APU/controllers inactive: return the floating-bus latch
            // without firing any register side-effects. The latch
            // itself is NOT updated — DMA reads of the inactive
            // register window don't drive the external data bus
            // (the chip is silent).
            return self.open_bus;
        }
        // W3-Stage-4 (`mc-r1-oam-dma-reg-window`): the ACTIVE-window arm —
        // the 6502 bus is parked in `$4000-$401F`, so the APU/controller
        // chip select is asserted for EVERY OAM-DMA source read and the
        // readable registers decode at `$4000 | (src & $1F)` (the `$20`-byte
        // mirrors AccuracyCoin `APU Register Activation` Tests 5-7 bracket).
        if (self.dma_halt_addr & 0xFFE0) == 0x4000 {
            return self.oam_dma_read_reg_active(src_addr);
        }
        self.raw_cpu_read(src_addr)
    }

    /// W3-Stage-4 (`mc-r1-oam-dma-reg-window`): one OAM-DMA source read with
    /// the 2A03 register window ACTIVE (the halted 6502 address bus is parked
    /// in `$4000-$401F`, e.g. the `AccuracyCoin` `APU Register Activation`
    /// Test 5/7 `JSR $3FFE` + BRK choreography parks it at `$4001`).
    ///
    /// Direct port of the `TriCNES` `Fetch` addressBus-window block
    /// (`Emulator.cs:9252-9311`):
    ///
    /// * The normal external decode of `src_addr` runs first (RAM / PPU /
    ///   cartridge / floating), tracking whether the region DRIVES the data
    ///   pins (`dataPinsAreNotFloating`).
    /// * `Reg == $15` (`$4015` mirror): returns the APU status on the
    ///   INTERNAL bus — the frame-IRQ flag is cleared (the side effect Test 4
    ///   brackets from the inactive side), bit 5 comes from the internal-bus
    ///   latch (Test 7's `$24` = triangle + bit 5 of the previous page-2
    ///   fetch), and the data bus / open-bus latch is NOT driven ("reading
    ///   from `$4015` can not affect the databus"). The status value still
    ///   reaches OAM because the DMA PUT half writes (and drives the bus
    ///   with) the byte — see [`Self::oam_dma_put`].
    /// * `Reg == $16`/`$17` (`$4016`/`$4017` mirrors): the controller shift
    ///   register is clocked; the value is `bit | (open_bus & $E0)` when the
    ///   source region floats (Test 5's page-`$50` chain: `$41`, `$40`, then
    ///   `$01`/`$00` after the `$4015` value decays bit 6 off the bus), but
    ///   when the source DRIVES the pins the external byte wins the bus
    ///   conflict and the controller bits are invisible (Test 7's page-`$02`
    ///   variant — "it does not appear to have read the controllers...
    ///   but they are still getting clocked").
    /// * Everything else: the external fetch value (floating sources return
    ///   the open-bus latch untouched).
    ///
    /// The end of the Test-5 chain leaves `$00` on the bus, so the resumed
    /// opcode fetch at `$4001` (open bus) executes BRK — the value path is
    /// load-bearing for the test's own control flow: any divergence here is
    /// what wedged the Stage-3 attempt (runaway execution instead of BRK).
    fn oam_dma_read_reg_active(&mut self, src_addr: u16) -> u8 {
        // Does the external decode of `src_addr` drive the data pins?
        // (TriCNES `dataPinsAreNotFloating` after the normal decode.)
        let drives = match src_addr {
            // RAM and the PPU registers always drive (write-only PPU regs
            // return the PPU-bus latch — still driven).
            0x0000..=0x3FFF => true,
            // The `$4000-$401F` window itself: the APU drives the INTERNAL
            // bus only; the external pins float. (The register overlay
            // below is the single decode — skip the external fetch so the
            // readable registers don't double-fire.)
            0x4000..=0x401F => false,
            // Cartridge space: mapper-dependent.
            _ => !self.mapper.cpu_read_unmapped(src_addr),
        };
        let external = if (src_addr & 0xFFE0) == 0x4000 {
            self.last_read_addr = src_addr;
            self.open_bus
        } else {
            // Normal external fetch (side effects included — a PPU-register
            // source behaves exactly as TriCNES's normal decode does).
            // Floating sources early-return the open-bus latch untouched.
            self.raw_cpu_read(src_addr)
        };
        match src_addr & 0x1F {
            0x15 => {
                // `$4015` mirror: internal-bus read, external bus untouched.
                // Mirrors the normal-CPU `$4015` composition in
                // `raw_cpu_read` (status bits + internal-bus bit 5).
                let status = self.apu.read_status();
                (status & 0xDF) | (self.internal_data_bus & 0x20)
            }
            reg @ (0x16 | 0x17) => {
                let port = usize::from(reg - 0x16);
                let bit = self.read_port(port);
                if drives {
                    // Bus conflict: the externally-driven byte wins; the
                    // controller was still clocked (`read_port` above).
                    external
                } else {
                    let v = (self.open_bus & 0xE0) | bit;
                    self.open_bus = v;
                    v
                }
            }
            _ => external,
        }
    }

    /// OAM-DMA PUT half: write the latched byte to OAM.
    ///
    /// W3-Stage-4 (`mc-r1-oam-dma-reg-window`): when the halted 6502 bus is
    /// parked in `$4000-$401F`, the put (a `$2004` write) DRIVES the external
    /// data bus with the byte — `TriCNES` `OAMDMA_Put` ->
    /// `Store(OAM_InternalBus, 0x2004)`, where every `Store` puts the value
    /// on `dataBus`. This is how the `$4015`-mirror value (which cannot drive
    /// the bus on its read) reaches the open-bus latch for the NEXT mirror
    /// read's `& $E0` merge, and how the Test-5 chain decays to `$00` so the
    /// resumed `$4001` fetch executes BRK. On real silicon every OAM put
    /// drives the bus; the model is deliberately scoped to the parked-window
    /// case so all other OAM DMAs stay byte-identical to the floor.
    fn oam_dma_put(&mut self) {
        self.ppu.oam_dma_write(self.dma_byte);
        if (self.dma_halt_addr & 0xFFE0) == 0x4000 {
            self.open_bus = self.dma_byte;
        }
    }

    /// Session-21: set the bus-access tracker for an upcoming DMA cycle.
    /// `tick_one_cpu_cycle` consumes this when it pushes the record.
    /// No-op (no field even exists) when the trace feature is disabled.
    #[cfg(feature = "irq-timing-trace")]
    const fn set_trace_dma_access(&mut self, access: BusAccess, addr: u16, data: u8) {
        self.trace_bus_access = access;
        self.trace_bus_addr = addr;
        self.trace_bus_data = data;
    }

    /// Service one DMC DMA transfer.
    ///
    /// Per nesdev §DMC DMA:
    /// - Halt cycle (1 CPU cycle).
    /// - Dummy cycle (1 CPU cycle).
    /// - Optional alignment cycle if the DMA get would otherwise land on
    ///   a put cycle.
    /// - One memory-read/get cycle.
    ///
    /// While CPU is halted, the previous read is logically repeated.  For
    /// `$4015` / `$4016` / `$4017` / `$2007` this has the documented
    /// register-readout side-effect bug; PAL fixes it.
    ///
    /// v1.2 Sprint 3.2 — two implementations now coexist via the
    /// `dmc-get-put-scheduler` cargo feature (ADR-0007). With the
    /// flag OFF, the v1.0/v1.1 baseline ("phase-agnostic noop loop +
    /// compensating delays") is preserved bit-identically — the four
    /// delays `dmc_dma_short`, `dmc_dma_cooldown`, `dmc_abort_delay`,
    /// `dmc_dma_delay` are still load-bearing. With the flag ON, the
    /// new path uses Mesen2's get/put cycle alternation model
    /// (`NesCpu.cpp:399-450`) — `dmc_need_halt` and
    /// `dmc_need_dummy_read` on the APU are consumed cycle-by-cycle,
    /// and the four compensating delays are NO-OPS under the new
    /// model. The new path closes the cycle-2 implied-dummy-read
    /// cascade that 6 prior single-delay tweaks could not.
    // Phase B: the DMC burst is CPU-driven-interleaved under R1, so this
    // bus-side burst is unused there (still used on the default path).
    #[allow(dead_code)]
    fn service_dmc_dma(&mut self, halted_addr: u16) {
        if !self.apu.dmc_dma_pending() || self.in_dmc_dma {
            return;
        }
        let addr = self.apu.dmc_dma_addr();
        let noop_cycles = if self.apu.dmc_dma_short() { 2 } else { 3 };
        self.in_dmc_dma = true;
        self.capture_deferred_dma_replay();

        for _ in 0..noop_cycles {
            self.replay_dma_noop_read(halted_addr);
            // Session-21: tag DMC halt/dummy/align cycles as DmaRead with
            // the halted CPU address (which is what the open-bus latch
            // sees on real silicon during those cycles).
            #[cfg(feature = "irq-timing-trace")]
            self.set_trace_dma_access(BusAccess::DmaRead, halted_addr, self.open_bus);
            self.tick_one_cpu_cycle();
        }

        // Perform the actual sample read/get and deliver back to the APU.
        let byte = self.dmc_dma_read(addr, halted_addr);
        if self.apu.dmc_dma_deliver_before_tick() {
            self.apu.complete_dmc_dma_before_get_tick(byte);
            #[cfg(feature = "irq-timing-trace")]
            self.set_trace_dma_access(BusAccess::DmaRead, addr, byte);
            self.tick_one_cpu_cycle();
        } else {
            #[cfg(feature = "irq-timing-trace")]
            self.set_trace_dma_access(BusAccess::DmaRead, addr, byte);
            self.tick_one_cpu_cycle();
            self.apu.complete_dmc_dma(byte);
        }
        self.in_dmc_dma = false;
    }

    #[allow(dead_code)]
    fn service_dmc_abort(&mut self, halted_addr: u16) {
        if !self.apu.dmc_abort_pending() || self.in_dmc_dma {
            return;
        }
        self.in_dmc_dma = true;
        self.replay_dma_noop_read(halted_addr);
        // Session-21: abort halt cycle is observable from the bus as a
        // DmaRead of the halted CPU address (open-bus driver retained).
        #[cfg(feature = "irq-timing-trace")]
        self.set_trace_dma_access(BusAccess::DmaRead, halted_addr, self.open_bus);
        self.tick_one_cpu_cycle();
        self.apu.complete_dmc_abort();
        self.in_dmc_dma = false;
    }

    #[allow(dead_code)]
    fn service_dmc_dma_during_oam(&mut self, total: u32, alignment: u32) {
        if !self.apu.dmc_dma_pending() || self.in_dmc_dma {
            return;
        }
        let addr = self.apu.dmc_dma_addr();
        let noop_cycles = if self.apu.dmc_dma_short() { 2 } else { 3 };
        let halted_addr = self.dma_halt_addr;
        self.in_dmc_dma = true;
        self.capture_deferred_dma_replay();

        // DMC halt, dummy, and alignment no-op cycles overlap with OAM DMA.
        // The 6502 core remains halted, but OAM can keep consuming its own
        // read/write slots on those cycles.
        for _ in 0..noop_cycles {
            self.replay_dma_noop_read(halted_addr);
            if self.dma_cycles_owed > 0 {
                // clock_oam_dma_cycle owns its own trace tagging.
                self.clock_oam_dma_cycle(total, alignment);
            } else {
                #[cfg(feature = "irq-timing-trace")]
                self.set_trace_dma_access(BusAccess::DmaRead, halted_addr, self.open_bus);
                self.tick_one_cpu_cycle();
            }
        }

        // The actual DMC get owns the memory read cycle. If OAM still has a
        // transfer pending, this skips one OAM slot and forces the next OAM
        // read to realign on a later get cycle.
        let byte = self.dmc_dma_read(addr, halted_addr);
        let deliver_before_tick = self.apu.dmc_dma_deliver_before_tick();
        if deliver_before_tick {
            self.apu.complete_dmc_dma_before_get_tick(byte);
        }
        #[cfg(feature = "irq-timing-trace")]
        self.set_trace_dma_access(BusAccess::DmaRead, addr, byte);
        self.tick_one_cpu_cycle();
        if self.dma_cycles_owed > 0 {
            #[cfg(feature = "irq-timing-trace")]
            self.set_trace_dma_access(BusAccess::DmaRead, halted_addr, self.open_bus);
            self.tick_one_cpu_cycle();
        }
        if !deliver_before_tick {
            self.apu.complete_dmc_dma(byte);
        }
        self.in_dmc_dma = false;
    }

    const fn capture_deferred_dma_replay(&mut self) {
        self.deferred_dma_replay_addr = match self.open_bus {
            0x02 => 0x2002,
            0x07 => 0x2007,
            0x15 => 0x4015,
            0x16 => 0x4016,
            0x17 => 0x4017,
            _ => 0,
        };
    }

    /// Re-execute the side-effect of the most recent CPU read for the
    /// 2A03 DMC-DMA readout bug. Replays side effects of reads from
    /// `$2002`, `$2007`, `$4015`, `$4016` and `$4017`. Per `AccuracyCoin`
    /// "APU Registers and DMA tests" — sub-tests check that the DMC
    /// DMA halt cycles re-trigger the cached read's side effects on
    /// real silicon.
    fn replay_dma_noop_read(&mut self, addr: u16) {
        if matches!(self.apu_region(), ApuRegion::Pal) {
            return;
        }
        match addr {
            0x2002 => {
                let mut adapter = PpuBusAdapter {
                    mapper: self.mapper.as_mut(),
                    nt_override: self.nt_mirroring_override,
                    sub_dot: 2,
                    #[cfg(feature = "irq-timing-trace")]
                    trace_a12_latest: None,
                };
                let _ = self.ppu.cpu_read_register(2, &mut adapter);
            }
            0x2007 => {
                let mut adapter = PpuBusAdapter {
                    mapper: self.mapper.as_mut(),
                    nt_override: self.nt_mirroring_override,
                    // CPU register replay (e.g. $2007 read-bug): treated as
                    // M2-high (sub_dot 2) since the 6502 drives its bus
                    // during φ2.
                    sub_dot: 2,
                    #[cfg(feature = "irq-timing-trace")]
                    trace_a12_latest: None,
                };
                let _ = self.ppu.cpu_read_register(7, &mut adapter);
            }
            0x4015 => {
                let _ = self.apu.read_status();
                self.apu.clear_frame_irq_immediate_for_dma();
            }
            0x4016 => {
                let _ = self.controllers[0].read();
            }
            0x4017 => {
                let _ = self.controllers[1].read();
            }
            _ => {}
        }
    }

    /// Read the DMC sample byte and model the 2A03 register-conflict path
    /// where 6502 core address bits 15..=5 remain from the halted CPU read
    /// while DMA supplies address bits 4..=0.
    fn dmc_dma_read(&mut self, addr: u16, halted_addr: u16) -> u8 {
        #[cfg(feature = "mc-r1-dmc-abort-probe")]
        {
            #[allow(clippy::cast_possible_truncation)]
            let c = self.cycle as u32;
            rustynes_apu::abort_probe::log_get(c, addr);
        }
        let sample = self.raw_cpu_read(addr);
        if matches!(self.apu_region(), ApuRegion::Pal) || (halted_addr & 0xFFE0) != 0x4000 {
            return sample;
        }

        let conflict_addr = 0x4000 | (addr & 0x001F);
        match conflict_addr {
            0x4015 => {
                let _ = self.apu.read_status();
                sample
            }
            0x4016 => {
                let v = (sample & 0xE0) | self.controllers[0].read();
                self.open_bus = v;
                v
            }
            0x4017 => {
                let v = (sample & 0xE0) | self.controllers[1].read();
                self.open_bus = v;
                v
            }
            _ => sample,
        }
    }

    /// v2.0 interleaved-DMA Phase B: perform ONE cycle of an interleaved DMC
    /// DMA (`TriCNES` `_6502` DMC-only path: `DMCDMA_Halted`/`Put`/`Get`). Called
    /// once per R1 cycle from `Cpu::read1` while `apu.dmc_dma_pending()`, at the
    /// access-point of the cycle (after `start_cycle`, before `end_cycle`). The
    /// CPU drives the cycle timing; this does only the DMA bus access + advances
    /// the halt/get state. The GET always lands on a get cycle (`!put_cycle`),
    /// so the 3-vs-4-cycle span is EMERGENT from the `put_cycle` parity at arm
    /// time (divergence-A self-consistency), not main's fixed `short?2:3`.
    #[allow(clippy::too_many_lines)]
    fn dmc_dma_step_impl(&mut self, halted_addr: u16) {
        if !self.in_dmc_dma {
            // First cycle of this DMA span: latch halt + the open-bus replay.
            self.in_dmc_dma = true;
            self.dmc_halt = true;
            self.capture_deferred_dma_replay();
        }
        // get = read cycle (TriCNES `!APU_PutCycle`); put = write cycle.
        let get_cycle = !self.apu.put_cycle();
        if get_cycle && !self.dmc_halt {
            // The GET: fetch the sample (with the `$4000` open-bus conflict the
            // DMA cluster brackets) + deliver to the DMC.
            let addr = self.apu.dmc_dma_addr();
            let byte = self.dmc_dma_read(addr, halted_addr);
            #[cfg(feature = "irq-timing-trace")]
            self.set_trace_dma_access(BusAccess::DmaRead, addr, byte);
            // v1.4.0 Workstream D (D2) — DMC-DMA event-breakpoint tap (the GET
            // cycle that fetches a sample). Output-only.
            #[cfg(feature = "debug-hooks")]
            self.record_event_break(EventBpKind::DmcDma, addr);
            self.apu.complete_dmc_dma(byte);
            self.in_dmc_dma = false;
            // Program M (M-2): this step performed the GET (steals an OAM slot).
            {
                self.dmc_step_was_get = true;
            }
        } else {
            // Program M (M-2): this step was a halt/dummy/align (overlaps OAM).
            {
                self.dmc_step_was_get = false;
            }
            // Halt / alignment / put cycle: re-read the halted CPU address bus
            // (TriCNES `Fetch(addressBus)`).
            self.replay_dma_noop_read(halted_addr);
            // Tag the halt re-read so the trace shows the DMC DMA's $4015
            // (etc.) re-read landing — the side-effect cycle the $4015
            // frame-IRQ-clear diagnostic correlates against.
            #[cfg(feature = "irq-timing-trace")]
            self.set_trace_dma_access(BusAccess::DmaRead, halted_addr, self.open_bus);
            if get_cycle {
                // A get cycle clears the halt ("halts clear after a get cycle").
                self.dmc_halt = false;
            }
        }
    }

    /// W3-Stage-1 (`mc-r1-dma-unified`): clear the unified engine's transient
    /// OAM-DMA state (reset / power-cycle / snapshot-restore).
    const fn unified_dma_clear(&mut self) {
        self.uni_oam_active = false;
        self.uni_oam_halt = false;
        self.uni_oam_aligned = false;
        self.uni_oam_addr = 0;
    }

    /// W3-Stage-1 (`mc-r1-dma-unified`): ONE cycle of the unified DMC/OAM DMA
    /// engine — a direct port of the `TriCNES` `_6502` per-cycle DMA dispatch
    /// table (`crates/rustynes-test-harness/golden/tricnes/tricnes-harness-src/
    /// Emulator.cs` ~4233-4357), the SINGLE driver that standalone DMC,
    /// standalone OAM, and the DMC-during-OAM overlap all ride — AT FLOOR
    /// PARITY for this stage (the structural-equivalence proof; Stage 2 flips
    /// the one engine to the breakthrough parity).
    ///
    /// Floor-parity mapping (the structural truth Stage 2 collapses): the
    /// floor's two drivers run on OPPOSITE halves of the shared cycle counter
    /// (`put_cycle == (self.cycle & 1 == 0)` at the access point):
    ///
    /// * the DMC engine's GET half is `!put_cycle` (ODD bus cycles) — the
    ///   emergent `dmc_dma_step_impl` span (halt latched on entry, cleared at
    ///   the end of the first odd cycle, GET on the next odd) is preserved
    ///   exactly: entry-on-even = span 4, entry-on-odd = span 3;
    /// * the OAM engine's READ half is `put_cycle` (EVEN bus cycles) — the
    ///   floor's `oam_dma_step` latches 514 (halt + align + 512) when its
    ///   first serviced cycle is even (`self.cycle & 1 == 0`) and its reads
    ///   always land on even cycles; the emergent `uni_oam_halt` (`TriCNES`
    ///   `OAMDMA_Halt`, set only when the first serviced cycle is the read
    ///   half) reproduces the same 514/513 split with no owed-cycle counter.
    ///
    /// Each engine's halt clears at the end of ITS OWN get half — `TriCNES`
    /// "both halt cycles get cleared after a get cycle", split across the
    /// floor's two parities (Stage 2 merges them onto one). The post-GET
    /// realign is EMERGENT: a DMC GET stalls OAM for the slot AND forces
    /// `uni_oam_aligned = false` (`TriCNES` `DMCDMA_Get` ->
    /// `OAMDMA_Aligned = false`), so the in-flight byte is re-read.
    ///
    /// ONE bus slot per cycle. When a halted DMC overlaps an advancing OAM
    /// cycle, the held CPU read's side-effect replay still fires alongside —
    /// the lockstep `service_dmc_dma_during_oam` noop-body model (the in-tree
    /// overlap spec that passes the whole abort cluster on the default build).
    #[allow(clippy::too_many_lines)] // the cfg-split floor + merged dispatches
    fn unified_dma_cycle_impl(&mut self, halted_addr: u16) {
        // Cycle-half label at the access point (post `cpu_clock`, the APU
        // counter has flipped): even bus cycle == `put_cycle` at floor parity.
        // W3-Stage-2 (`mc-r1-dma-unified-collapse`): under the put_cycle
        // END-flip (the counter-collapse breakthrough parity) the access-point
        // read is the references' in-cycle `APU_PutCycle` label DIRECTLY —
        // TriCNES also flips at end-of-cycle — so the dispatch runs the single
        // TriCNES labeling: `get = !APU_PutCycle`. The floor's split halves
        // (DMC GET = odd / OAM READ = even) merge onto this one label.
        let get = !self.apu.put_cycle();

        // The two activation-time roles, derived per parity model:
        // * `oam_halt_on_first` — TriCNES `FirstCycleOfOAMDMA`: halt when the
        //   first serviced cycle lands on the OAM READ half (floor: even; the
        //   merged labeling: the GET half). Half-swap x parity-flip = the SAME
        //   absolute cycles, so standalone OAM timing is invariant.
        // * `dmc_noop_half` — the half a LOAD may not ENTER on (the span-3
        //   load-get-entry rule: a load enters on its get half).
        let (oam_halt_on_first, dmc_noop_half) = (get, !get);

        // --- OAM activation (TriCNES `$4014` -> FirstCycleOfOAMDMA) ---
        // The first serviced cycle after the `$4014` write latches the page +
        // the parked CPU address; `uni_oam_halt` is set only when this first
        // cycle lands on the OAM read half (floor: even -> the 514 case).
        // Latching here, regardless of any in-flight DMC, natively absorbs the
        // Stage-0 `$4014`-write-to-first-OAM-cycle gap (lockstep `drain_dma`
        // latches OAM BEFORE its DMC-pending check).
        if let Some(page) = self.dma_pending.take() {
            self.dma_page = page;
            self.uni_oam_addr = 0;
            self.uni_oam_aligned = false;
            self.uni_oam_active = true;
            self.uni_oam_halt = oam_halt_on_first;
            self.dma_halt_addr = halted_addr;
        }

        // --- DMC activation (the floor `dmc_dma_step_impl` first-cycle latch)
        // A LOAD may not ENTER on the DMC noop half: the floor's
        // `dmc_dma_defer_load_entry` while-gate defers exactly the entries
        // whose access-point parity is the noop half, so a load enters on its
        // get half = span 3 (`mc-r1-dmc-load-get-entry`). The same defer is
        // re-derived here for cycles the loop runs anyway because OAM is
        // active.
        // W3-Stage-3 (`mc-r1-dmc-delayed-4015`): a pending DMC whose APPLIED
        // status is false may not ACTIVATE either (the loop can still be
        // running for an active OAM; TriCNES's stale `DoDMCDMA` similarly
        // never re-enters the halt-latch path — `DMCDMA_Halt` was latched at
        // the original activation).
        let dmc_serviceable = self.apu.dmc_dma_serviceable();
        if self.apu.dmc_dma_pending() && dmc_serviceable && !self.in_dmc_dma {
            let defer_load = self.apu.dmc_dma_is_load() && dmc_noop_half;
            if !defer_load {
                self.in_dmc_dma = true;
                self.dmc_halt = true;
                self.capture_deferred_dma_replay();
            }
        }

        // --- Dispatch: ONE bus slot per cycle (floor parity: split halves) ---

        // --- Dispatch: ONE bus slot per cycle (W3-Stage-2: the references'
        // single get/put labeling — the literal TriCNES `_6502` table) ---
        if get {
            // GET half: DMC GET (priority) > OAM READ > halted reads.
            if self.in_dmc_dma && !self.dmc_halt {
                // THE DMC GET: owns the bus slot (with the `$4000` open-bus
                // conflict the DMA cluster brackets); a sharing OAM is STALLED
                // for the slot AND loses alignment (TriCNES `DMCDMA_Get` ->
                // `OAMDMA_Aligned = false`, the emergent post-GET realign).
                let addr = self.apu.dmc_dma_addr();
                let byte = self.dmc_dma_read(addr, halted_addr);
                #[cfg(feature = "irq-timing-trace")]
                self.set_trace_dma_access(BusAccess::DmaRead, addr, byte);
                self.apu.complete_dmc_dma(byte);
                self.in_dmc_dma = false;
                if self.uni_oam_active {
                    self.uni_oam_aligned = false;
                }
            } else if self.uni_oam_active && !self.uni_oam_halt {
                // A halted DMC shares this cycle: the held CPU read's
                // side-effect replay fires first (lockstep noop-body order:
                // `replay_dma_noop_read` THEN the OAM slot).
                if self.in_dmc_dma {
                    self.replay_dma_noop_read(halted_addr);
                }
                // OAM GET: the OAM engine owns the bus slot.
                let src = (u16::from(self.dma_page) << 8) | self.uni_oam_addr;
                self.dma_byte = self.raw_oam_dma_read(src);
                self.uni_oam_aligned = true;
                #[cfg(feature = "irq-timing-trace")]
                self.set_trace_dma_access(BusAccess::DmaRead, src, self.dma_byte);
            } else if self.in_dmc_dma {
                // DMC halted get: re-read the parked CPU address (TriCNES
                // `Fetch(addressBus)`). Covers the both-halted shared cycle
                // too (ONE re-read — TriCNES `DMCDMA_Halted`).
                self.replay_dma_noop_read(halted_addr);
                #[cfg(feature = "irq-timing-trace")]
                self.set_trace_dma_access(BusAccess::DmaRead, halted_addr, self.open_bus);
            } else {
                // OAM halt cycle alone: the parked address stays on the bus
                // (the floor `oam_dma_step` halt branch — no side-effect
                // replay).
                #[cfg(feature = "irq-timing-trace")]
                self.set_trace_dma_access(BusAccess::DmaRead, self.dma_halt_addr, self.open_bus);
            }
            // TriCNES: BOTH halt cycles get cleared after a get cycle.
            self.dmc_halt = false;
            self.uni_oam_halt = false;
        } else {
            // PUT half: OAM WRITE/align; a waiting/halted DMC replays the held
            // CPU read's side-effect alongside (TriCNES `DMCDMA_Put` /
            // `DMCDMA_Halted` — both `Fetch(addressBus)`).
            if self.in_dmc_dma {
                self.replay_dma_noop_read(halted_addr);
                #[cfg(feature = "irq-timing-trace")]
                self.set_trace_dma_access(BusAccess::DmaRead, halted_addr, self.open_bus);
            }
            if self.uni_oam_active && !self.uni_oam_halt {
                if self.uni_oam_aligned {
                    // OAM PUT: write the latched byte to OAM ($2004).
                    // `uni_oam_aligned` stays set through the transfer
                    // (TriCNES: only `DMCDMA_Get` and completion clear it).
                    self.oam_dma_put();
                    #[cfg(feature = "irq-timing-trace")]
                    self.set_trace_dma_access(BusAccess::DmaWrite, 0x2004, self.dma_byte);
                    self.uni_oam_addr += 1;
                    if self.uni_oam_addr == 256 {
                        // The DMA completes on the 256th write.
                        self.uni_oam_active = false;
                        self.uni_oam_aligned = false;
                    }
                } else {
                    // OAM alignment dummy: the parked address stays on the
                    // bus (the floor `oam_dma_step` align branch — no
                    // side-effect replay).
                    #[cfg(feature = "irq-timing-trace")]
                    if !self.in_dmc_dma {
                        self.set_trace_dma_access(
                            BusAccess::DmaRead,
                            self.dma_halt_addr,
                            self.open_bus,
                        );
                    }
                }
            }
            // (An OAM halt can never land on the PUT half under the merged
            // labeling — `uni_oam_halt` is set only on a GET first cycle and
            // clears at the end of that same GET half.)
        }
    }

    /// Raw CPU read that does **not** advance time — used by the OAM DMA
    /// engine and DMC DMA fetches.  Time was already advanced by the
    /// surrounding `tick_one_cpu_cycle` (or the DMA stall).
    pub(crate) fn raw_cpu_read(&mut self, addr: u16) -> u8 {
        // $4015 special case: reading from the APU status port reads
        // 2A03 internal state but does NOT drive the data bus (per
        // nesdev "Open bus behavior" + AccuracyCoin `CPU Behavior ::
        // Open Bus` Test 7). The CPU still receives the APU status,
        // but the open-bus latch stays at its prior value, so a
        // subsequent open-bus-region read returns the *previous*
        // floating-bus value rather than the APU status.
        if addr == 0x4015 {
            // $4015 read returns the APU status (internal silicon
            // state) and does NOT drive the external data bus, so
            // `self.open_bus` stays at its prior value (per nesdev
            // "Open bus behavior" + AccuracyCoin `CPU Behavior ::
            // Open Bus` Test 7).
            //
            // Bit 5 of $4015 is documented as open-bus on silicon.
            // With the Phase 1a internal-vs-external bus split, we
            // expose this from the INTERNAL data bus (CPU-only, NOT
            // polluted by DMC DMA fetches).  This satisfies BOTH:
            //   * Open Bus Test 9 — bit 5 returns the bus latch value
            //   * Internal Data Bus Test 2 — DMC DMA does NOT change
            //     bit 5 because DMC drives only the external bus.
            //
            // The pre-2026-05-23 conflated `open_bus` model could
            // not honour both tests simultaneously: empirically (per
            // CLAUDE.md Phase D3 audit), OR-ing `open_bus & 0x20`
            // into the read flipped Test 9 PASS but tripped Test 2
            // to FAIL — net-zero swap. With the internal-bus
            // separation, the trade-off is resolved.
            let status = self.apu.read_status();
            let v = (status & 0xDF) | (self.internal_data_bus & 0x20);
            self.last_read_addr = addr;
            return v;
        }
        let v = match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => self.ppu_register_read(addr),
            0x4000..=0x4014 | 0x4018..=0x401F => self.open_bus,
            0x4015 => unreachable!("handled above"),
            // Controllers drive D0 (and D1 on Famicom expansion port,
            // unused here). Bits 5-7 are open bus — the bus latch's
            // upper 3 bits show through. Bit 4 is the secondary
            // controller D1 (also open bus on stock NES). Per nesdev
            // "Standard controller" + AccuracyCoin `CPU Behavior ::
            // Open Bus` Test 6.
            0x4016 => {
                let base = (self.open_bus & 0xE0) | self.read_port(0);
                self.vs_overlay_4016(base)
            }
            0x4017 => {
                let base = (self.open_bus & 0xE0) | self.read_port(1);
                self.vs_overlay_4017(base)
            }
            0x4020..=0xFFFF => {
                if self.mapper.cpu_read_unmapped(addr) {
                    // Unmapped read: bus stays at the floating-latch
                    // value (per nesdev "Open bus behavior"). Don't
                    // overwrite `open_bus` — return early.
                    self.last_read_addr = addr;
                    return self.open_bus;
                }
                // The Game Genie physically substitutes the byte on the
                // cartridge bus, so the (possibly substituted) value is what
                // the CPU sees AND what latches onto `open_bus` below.
                let raw = self.mapper.cpu_read(addr);
                self.apply_genie(addr, raw)
            }
        };
        self.last_read_addr = addr;
        self.open_bus = v;
        #[cfg(feature = "mc-r1-dmc-abort-probe")]
        if addr == 0x4000 && (48_000_000..=48_700_000).contains(&self.cycle) {
            #[allow(clippy::cast_possible_truncation)]
            let c = self.cycle as u32;
            rustynes_apu::abort_probe::log_4000_read(c, v);
        }
        // Mirror the read onto the internal data bus, but ONLY when
        // this is a CPU-initiated access.  DMC DMA fetches drive
        // only the EXTERNAL (`open_bus`) bus per nesdev's two-bus
        // 2A03 model and per AccuracyCoin's `CPU Behavior 2 ::
        // Internal Data Bus` Test 2 ("This DMC DMA does not update
        // the external data bus.  Only the internal one." — the
        // upstream comment treats "internal" as the OPPOSITE of
        // what we call internal here; per the test sequence the
        // INTERNAL_data_bus is what `$4015` bit-5 returns, and DMC
        // DMA must NOT pollute it).  The `in_dmc_dma` guard is set
        // by `service_dmc_dma` before invoking `dmc_dma_read` →
        // `raw_cpu_read`; we skip the internal-bus mirror in that
        // path so the internal latch retains its prior CPU-driven
        // value across DMC halts.  Phase 1 of `linked-puzzling-sutherland`.
        if !self.in_dmc_dma {
            self.internal_data_bus = v;
        }
        v
    }

    /// PPU register read with side effects.
    fn ppu_register_read(&mut self, addr: u16) -> u8 {
        let reg = (addr & 7) as u8;
        let mut adapter = PpuBusAdapter {
            mapper: self.mapper.as_mut(),
            nt_override: self.nt_mirroring_override,
            // CPU bus access happens during φ2 → sub_dot 2 (M2-high).
            sub_dot: 2,
            #[cfg(feature = "irq-timing-trace")]
            trace_a12_latest: None,
        };
        self.ppu.cpu_read_register(reg, &mut adapter)
    }

    /// PPU register write with side effects.
    fn ppu_register_write(&mut self, addr: u16, value: u8) {
        let reg = (addr & 7) as u8;
        let mut adapter = PpuBusAdapter {
            mapper: self.mapper.as_mut(),
            nt_override: self.nt_mirroring_override,
            // CPU bus access happens during φ2 → sub_dot 2 (M2-high).
            sub_dot: 2,
            #[cfg(feature = "irq-timing-trace")]
            trace_a12_latest: None,
        };
        self.ppu.cpu_write_register(reg, value, &mut adapter);
    }
}

/// v1.1.0 beta.1 (T-110-B4) — translate a `$2000-$3EFF` PPU address to a
/// CIRAM offset under an explicit mirroring (the per-game override path),
/// mirroring the `Mapper::nametable_address` default impl.
#[allow(clippy::cast_possible_truncation)] // physical_bank is always 0 or 1.
const fn override_nt_addr(m: rustynes_mappers::Mirroring, addr: u16) -> u16 {
    const NT: u16 = 0x0400;
    let table = ((addr.wrapping_sub(0x2000)) / NT) & 0x03;
    let local = addr & (NT - 1);
    (m.physical_bank(table as u8) as u16) * NT + local
}

/// Adapter that exposes the [`PpuBus`] interface over a `&mut dyn Mapper`.
struct PpuBusAdapter<'a> {
    mapper: &'a mut dyn Mapper,
    /// v1.1.0 beta.1 (T-110-B4) — the bus's per-game mirroring override, copied
    /// in at construction. When `Some`, `nametable_address` uses it instead of
    /// the mapper's mirroring.
    nt_override: Option<rustynes_mappers::Mirroring>,
    /// Current PPU sub-dot of the host CPU cycle (0, 1, or 2).  Set by
    /// the bus's tick loop before each `Ppu::tick` call so that
    /// `notify_a12_at_sub_dot` (C1 step B4-successor M2-phase plumbing)
    /// can forward the sub-dot to the mapper for cycle-precise IRQ
    /// propagation modeling.  Sub-dots 0 / 1 are M2-low (φ1) and 2 is
    /// M2-high (φ2) per our convention.
    sub_dot: u8,
    /// When the IRQ-timing trace feature is enabled, the most recent A12
    /// level passed through `notify_a12` is mirrored here so the bus's
    /// per-sub-dot trace loop can pick it up.  `None` when tracing is
    /// off (the standard hot path).
    #[cfg(feature = "irq-timing-trace")]
    trace_a12_latest: Option<&'a mut Option<bool>>,
}

impl PpuBus for PpuBusAdapter<'_> {
    fn ppu_read(&mut self, addr: u16) -> u8 {
        self.mapper.ppu_read(addr & 0x1FFF)
    }
    fn ppu_read_sprite(&mut self, addr: u16) -> u8 {
        self.mapper.ppu_read_sprite(addr & 0x1FFF)
    }
    fn ppu_write(&mut self, addr: u16, value: u8) {
        self.mapper.ppu_write(addr & 0x1FFF, value);
    }
    fn peek_nametable(&mut self, addr: u16) -> Option<u8> {
        self.mapper.nametable_fetch(addr)
    }
    fn write_nametable(&mut self, addr: u16, value: u8) -> bool {
        self.mapper.nametable_write(addr, value)
    }
    fn peek_ex_attribute(&mut self, v: u16) -> Option<PpuExAttribute> {
        self.mapper.peek_ex_attribute(v).map(|ex| PpuExAttribute {
            palette: ex.palette,
            chr_bank: ex.chr_bank,
        })
    }
    fn bg_split_state(&mut self, scanline_y: u16, coarse_x: u16) -> Option<PpuBgSplitState> {
        self.mapper
            .bg_split_state(scanline_y, coarse_x)
            .map(|s| PpuBgSplitState {
                nt_addr: s.nt_addr,
                at_addr: s.at_addr,
                fine_y: s.fine_y,
                chr_bank: s.chr_bank,
            })
    }
    fn notify_a12(&mut self, level: bool) {
        // C1 step B4 successor: forward the current sub-dot to the
        // mapper so MMC3 can apply the M2-phase-aware IRQ-output
        // propagation delay required by `mmc3_test_2/4-scanline_timing`
        // sub-test #3.  Non-MMC3 mappers' default
        // `notify_a12_at_sub_dot` impl falls back to plain `notify_a12`,
        // so this thread-through is invisible to NROM / UxROM / etc.
        self.mapper.notify_a12_at_sub_dot(level, self.sub_dot);
        #[cfg(feature = "irq-timing-trace")]
        if let Some(slot) = self.trace_a12_latest.as_deref_mut() {
            *slot = Some(level);
        }
    }
    fn notify_scanline_start(&mut self) {
        self.mapper.notify_scanline_start();
    }
    fn notify_vblank(&mut self) {
        self.mapper.notify_vblank();
    }
    fn nametable_address(&self, addr: u16) -> u16 {
        self.nt_override.map_or_else(
            || self.mapper.nametable_address(addr),
            |m| override_nt_addr(m, addr),
        )
    }
}

/// v2.0 master-clock R1 substrate helpers (Phase 1). Compiled only under
/// `mc-r1-substrate`; used by the clean `Bus` contract overrides below.
impl LockstepBus {
    /// `(cpu_divider, ppu_divider)` in master clocks for the cartridge region
    /// (NTSC 12/4, PAL 16/5, Dendy 15/5). Drives the R1 `run_ppu_to` dot loop.
    /// Reads the values cached at construction (region is immutable after
    /// parse), so the hot R1 paths avoid a per-cycle `match`.
    const fn region_dividers(&self) -> (u8, u8) {
        (self.cpu_div_cached, self.ppu_div_cached)
    }

    /// Tick the APU + frame counter once and fan frame events out to on-cart
    /// audio (the per-CPU-cycle APU advance extracted from
    /// `tick_one_cpu_cycle`, for the R1 `cpu_clock`).
    ///
    /// v2.8.0 Phase 4 — the mapper dispatches are gated on the cached
    /// capability flags: boards without on-cart audio would return 0 from
    /// the default `mix_audio` (0.0 after the f32 conversion — identical),
    /// and boards without the frame hook have the default no-op. Skipping
    /// both saves two virtual calls + an f32 divide per CPU cycle.
    fn apu_advance_one(&mut self) {
        let mapper_sample = if self.mapper_caps.audio {
            f32::from(self.mapper.mix_audio()) / 65536.0
        } else {
            0.0
        };
        self.apu.tick_with_external(mapper_sample);
        if self.mapper_caps.frame_event_hook {
            let ev = self.apu.last_frame_events();
            self.mapper.notify_frame_event(MapperFrameEvents {
                quarter: ev.quarter,
                half: ev.half,
            });
        }
    }
}

impl Bus for LockstepBus {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        // Drain any pending DMA before doing the requested access.
        self.drain_dma(Some(addr));
        if self.deferred_dma_replay_addr != 0
            && self.open_bus == (self.deferred_dma_replay_addr >> 8) as u8
        {
            if self.deferred_dma_replay_addr == addr {
                self.replay_dma_noop_read(addr);
            }
            self.deferred_dma_replay_addr = 0;
        }
        let value = self.raw_cpu_read(addr);
        // v1.1.0 beta.3 (T-110-E2) — Lua onRead access tap. Output-only, gated.
        #[cfg(feature = "debug-hooks")]
        if self.access_logging && self.accesses.len() < ACCESS_CAP {
            self.accesses.push(AccessRec {
                write: false,
                addr,
                value,
            });
        }
        // v1.5.0 Workstream A2 — event-viewer read tap: the graphical PPU Event
        // Viewer needs PPU-register READS (`$2002` status polls, `$2007` data
        // fetches) plotted alongside writes. Only the `$2000-$3FFF` PPU window is
        // captured (the dense APU/RAM/PRG read stream would swamp the timeline);
        // writes across PPU/APU/mapper are captured in `cpu_write`. Output-only,
        // gated, bounded by `EVENT_CAP` — determinism-neutral.
        #[cfg(feature = "debug-hooks")]
        if self.event_logging && matches!(addr, 0x2000..=0x3FFF) && self.events.len() < EVENT_CAP {
            self.events.push(EventRec {
                kind: EventKind::PpuRead,
                scanline: self.ppu.scanline(),
                dot: self.ppu.dot(),
                addr,
                value,
            });
        }
        // v1.4.0 Workstream D (D2) — event-breakpoint read taps. Output-only.
        // The `mask == 0` early-out in `record_event_break` keeps the default
        // path cheap; the sprite-0-hit category is observed where games detect
        // it: a `$2002` read returning bit 6 set.
        #[cfg(feature = "debug-hooks")]
        if self.event_bp_mask != 0 {
            match addr {
                0x2002 if value & 0x40 != 0 => {
                    self.record_event_break(EventBpKind::Sprite0Hit, addr);
                }
                0x2000..=0x3FFF => self.record_event_break(EventBpKind::PpuRead, addr),
                0x4000..=0x4017 => self.record_event_break(EventBpKind::ApuRead, addr),
                0x4020..=0xFFFF => self.record_event_break(EventBpKind::MapperRead, addr),
                _ => {}
            }
        }
        #[cfg(feature = "irq-timing-trace")]
        {
            // Session-21: record the CPU-initiated read at the bus-access
            // tracker.  `tick_one_cpu_cycle` was already called by the
            // CPU's `read1`/`idle_tick` path (post `bus.on_cpu_cycle()`),
            // but the order in `Cpu::read1` is `bus.cpu_read(addr)` then
            // `idle_tick(bus)` → `bus.on_cpu_cycle()` → record-push.
            // So writing the tracker here populates the record that the
            // about-to-fire `tick_one_cpu_cycle` will consume.
            self.trace_bus_access = BusAccess::Read;
            self.trace_bus_addr = addr;
            self.trace_bus_data = value;
        }
        value
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        self.drain_dma(None);
        self.open_bus = value;
        // Mirror the CPU-initiated write onto the internal data bus.
        // Symmetric with `raw_cpu_read`'s mirror — DMC DMA does not
        // perform writes, so internal-vs-external divergence only
        // arises across DMC read halts.  (No `in_dmc_dma` guard
        // here because DMC DMA never invokes `cpu_write`.)
        self.internal_data_bus = value;
        // v1.1.0 beta.3 (T-110-E2) — Lua onWrite access tap. Output-only, gated.
        #[cfg(feature = "debug-hooks")]
        if self.access_logging && self.accesses.len() < ACCESS_CAP {
            self.accesses.push(AccessRec {
                write: true,
                addr,
                value,
            });
        }
        // v1.1.0 beta.2 (T-110-C3) — event-viewer tap: classify the write +
        // record it with the current PPU position. Output-only, gated.
        #[cfg(feature = "debug-hooks")]
        if self.event_logging {
            let kind = match addr {
                0x2000..=0x3FFF => Some(EventKind::PpuWrite),
                // The whole `$4000-$4017` APU / I/O window (Copilot #43): this
                // now also captures `$4014` OAM DMA and `$4016` controller
                // strobe, which the legend's "$4000-4017" already advertises.
                0x4000..=0x4017 => Some(EventKind::ApuWrite),
                0x4020..=0xFFFF => Some(EventKind::MapperWrite),
                _ => None,
            };
            if let Some(kind) = kind
                && self.events.len() < EVENT_CAP
            {
                self.events.push(EventRec {
                    kind,
                    scanline: self.ppu.scanline(),
                    dot: self.ppu.dot(),
                    addr,
                    value,
                });
            }
        }
        // v1.4.0 Workstream D (D2) — event-breakpoint write taps. Output-only.
        // `$4014` is the OAM-DMA trigger; the rest classify by window.
        #[cfg(feature = "debug-hooks")]
        if self.event_bp_mask != 0 {
            match addr {
                0x2000..=0x3FFF => self.record_event_break(EventBpKind::PpuWrite, addr),
                REG_OAM_DMA => self.record_event_break(EventBpKind::OamDma, addr),
                0x4000..=0x4017 => self.record_event_break(EventBpKind::ApuWrite, addr),
                0x4020..=0xFFFF => self.record_event_break(EventBpKind::MapperWrite, addr),
                _ => {}
            }
        }
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize] = value,
            0x2000..=0x3FFF => self.ppu_register_write(addr, value),
            REG_OAM_DMA => self.dma_pending = Some(value),
            0x4000..=0x4013 | 0x4015 | 0x4017 => {
                #[cfg(feature = "mc-r1-dmc-abort-probe")]
                if addr == 0x4010 {
                    #[allow(clippy::cast_possible_truncation)]
                    let c = self.cycle as u32;
                    rustynes_apu::abort_probe::log_4010(c, value);
                }
                self.apu.write_register(addr, value);
            }
            0x4016 => {
                // Session-24 / Phase 3 (Controller Strobing): the
                // controllers' OUT pins are only updated at the start
                // of M2-low (PUT) cycles.  Buffer the write and
                // commit at the next M2-low boundary inside
                // `tick_one_cpu_cycle`.  Mirrors Mesen2's
                // `NesControlManager::WriteRam` (Core/NES/
                // NesControlManager.cpp lines 252-273).
                //
                // Parity convention: in `RustyNES` the bus enters each
                // CPU cycle at `M2Phase::Low` and transitions to
                // `M2Phase::High` after PPU sub-dot 1.  The cycle
                // counter advances at end-of-cycle.  So a CPU write
                // executed during cycle `self.cycle` lands at the END
                // of that cycle's M2-high half.  The NEXT cycle
                // (`self.cycle + 1`) starts at M2-low — which is the
                // commit boundary.  In Mesen2's master-clock terms,
                // odd master clocks mean "one cycle from PUT" and
                // even mean "two cycles from PUT"; the corresponding
                // `RustyNES` rule is: if `self.cycle` is odd at write
                // time, pending = 1 (commit at next cycle); if even,
                // pending = 2 (commit at cycle-after-next).  This
                // collapses the AccuracyCoin Test 4 1-cycle DEC
                // `$4016` strobe pulse (both writes target the SAME
                // commit cycle; the second overwrites the first; no
                // edge is observed → no latch).  See
                // `docs/audit/session-24-phase3-controller-strobing-2026-05-23.md`.
                self.controller_write_value = value;
                // Parity convention: in `RustyNES` the CPU `cpu_write` runs
                // INSIDE `tick_one_cpu_cycle` AFTER `self.cycle` has
                // been incremented to the post-cycle value (see
                // `tick_one_cpu_cycle` flow).  The committed commit
                // cycle MUST land on an M2-low boundary (PUT cycle).
                // In `RustyNES` every CPU cycle starts at M2-low and
                // transitions to M2-high after sub-dot 1, so every
                // cycle has an M2-low half — but only cycles where
                // the COMMITTED strobe value is observable AT the
                // beginning of the cycle qualify as the deferred-
                // write commit target.
                //
                // The empirical calibration from the Phase 3 oracle:
                // Mesen2 PUT cycles correspond to ODD `cpu.cycleCount`
                // (per `NesCpu.cpp:400` `bool getCycle = (CycleCount &
                // 0x01) == 0;` — get cycles are even, put cycles are
                // odd).  Our `self.cycle` parity at the moment of
                // `cpu_write` differs from Mesen2's by an offset
                // (Mesen2's cycle count includes the boot/reset
                // sequence differently); empirically, our EVEN cycles
                // correspond to Mesen2's PUT cycles in the
                // `controller-strobing.nes` Test 3 vs Test 4
                // discrimination.  Hence: even `self.cycle` → pending
                // = 1 (commit next cycle); odd `self.cycle` → pending
                // = 2 (commit cycle-after-next).
                self.controller_write_pending = if (self.cycle & 1) == 0 { 1 } else { 2 };
                // Vs. System (mapper 99): the CHR bank select is bit 2 of the
                // value written to $4016 (shared with the controller strobe).
                // Forward every $4016 write to the mapper; only mapper 99
                // consumes it — every other mapper's `cpu_write` ignores the
                // $4016 address (their match arms only cover $8000-$FFFF /
                // $4020-$7FFF), so this is byte-for-byte a no-op on all
                // non-Vs. carts.
                self.mapper.cpu_write(0x4016, value);
            }
            0x4018..=0x401F => {}
            0x4020..=0xFFFF => self.mapper.cpu_write(addr, value),
        }
        #[cfg(feature = "irq-timing-trace")]
        {
            // Session-21: record the CPU-initiated write at the bus-access
            // tracker for the same reason `cpu_read` does above.
            self.trace_bus_access = BusAccess::Write;
            self.trace_bus_addr = addr;
            self.trace_bus_data = value;
        }
    }

    fn poll_nmi(&mut self) -> bool {
        let edge = self.nmi_edge_latch;
        self.nmi_edge_latch = false;
        edge
    }

    fn poll_irq(&mut self) -> bool {
        // Phase B2 of the C1 IRQ-timing rework: read the M2-high
        // snapshot captured at end-of-3-PPU-dots inside
        // `tick_one_cpu_cycle`.  Semantically identical to the prior
        // `mapper.irq_pending() || apu.irq_line()` query for every
        // workspace test ROM (verified: 500 strict + 6 ignored
        // unchanged; trace baselines byte-identical).  Phase B4 will
        // make the snapshot's value depend on the M2 phase via the
        // MMC3 sub_dot-aware A12 filter — this method becomes the
        // single point where the production CPU IRQ sample crosses
        // into the bus, and from there into the mapper.
        self.irq_snapshot_mapper_at_high || self.irq_snapshot_apu_at_high
    }

    fn poll_irq_at_phase(&mut self, phase: M2Phase) -> bool {
        match phase {
            M2Phase::Low => self.irq_snapshot_mapper_at_low || self.irq_snapshot_apu_at_low,
            M2Phase::High => self.irq_snapshot_mapper_at_high || self.irq_snapshot_apu_at_high,
        }
    }

    fn on_cpu_cycle(&mut self) {
        self.tick_one_cpu_cycle();
    }

    fn internal_data_bus(&self) -> u8 {
        // Phase 1 of `linked-puzzling-sutherland` v1.0.0-final brief:
        // expose the internal CPU data bus latch separately from the
        // external `open_bus`.  Mirrored from every CPU read / write;
        // NOT updated by DMC DMA fetches.  See the field documentation
        // on [`LockstepBus::internal_data_bus`] and the trait method
        // documentation on [`Bus::internal_data_bus`].
        self.internal_data_bus
    }

    fn cycle_count(&self) -> u64 {
        // Cumulative bus-side cycle counter, including DMC DMA cycles
        // that the CPU's own `Cpu::cycles` field does not count.  Used
        // by the SH* unstable-store family to detect DMA interrupting
        // their dummy-read cycle per Mesen2's `SyaSxaAxa` algorithm.
        self.cycle
    }

    fn notify_irq_service(&mut self, vector: u16, is_nmi: bool) {
        // v1.2.0 (T-110-E1) — Lua onNmi/onIrq interrupt-service tap. This is the
        // committed-service commit point (same as the IRQ trace below), NOT the
        // speculative poll_nmi/poll_irq sampler. Output-only, gated; no-op when
        // `debug-hooks` is off (the log slot only exists feature-gated).
        //
        // The reliable NMI/IRQ discriminator here is the COMMITTED `vector`
        // ($FFFA = NMI, $FFFE = IRQ/BRK), not the `is_nmi` arg: the unified
        // dispatch always enters `service_interrupt` with the IRQ vector and
        // resolves the NMI *hijack* internally (so the `is_nmi` arg reads
        // `false` on a hijacked NMI). Classifying by the vector the CPU actually
        // fetched reports exactly the service that committed.
        #[cfg(feature = "debug-hooks")]
        if self.interrupt_logging && self.interrupts.len() < INTERRUPT_CAP {
            let _ = is_nmi;
            self.interrupts.push(InterruptRec {
                is_nmi: vector == 0xFFFA,
                vector,
            });
        }
        // v1.4.0 Workstream D (D2) — NMI/IRQ event-breakpoint tap. Classified by
        // the COMMITTED vector (same discriminator the interrupt log uses).
        #[cfg(feature = "debug-hooks")]
        if self.event_bp_mask != 0 {
            let kind = if vector == 0xFFFA {
                EventBpKind::Nmi
            } else {
                EventBpKind::Irq
            };
            self.record_event_break(kind, vector);
        }
        // Phase 1.2 of Track C1 attempt 14: emit a [`ServiceEvent`] into
        // the IRQ trace if the trace is armed.  Production builds with
        // the `irq-timing-trace` feature OFF compile this down to a
        // no-op (the trace slot only exists feature-gated).
        #[cfg(feature = "irq-timing-trace")]
        if let Some(trace) = self.irq_trace.as_mut() {
            let frame_start = self.ppu.frame();
            let scanline_start = self.ppu.scanline();
            let dot_start = self.ppu.dot();
            let kind = if is_nmi {
                crate::irq_trace::ServiceKind::Nmi
            } else {
                crate::irq_trace::ServiceKind::Irq
            };
            // `self.cycle` is the count of cycles already consumed; the
            // service-vector fetch is the cycle the CPU is ABOUT to
            // emit, so reporting `self.cycle` (== the next cycle index)
            // matches Mesen2's `cpu.cycleCount` at the moment its
            // `emu.eventType.irq` callback fires (its cycle count is
            // sampled at the start of the service cycle).
            trace.push_service(crate::irq_trace::ServiceEvent {
                cpu_cycle: self.cycle,
                ppu_scanline: scanline_start,
                ppu_dot: dot_start,
                ppu_frame: frame_start,
                kind,
                vector,
            });
        } else {
            let _ = (vector, is_nmi);
        }
        // Suppress unused-variable warnings when the feature is off.
        #[cfg(not(feature = "irq-timing-trace"))]
        {
            let _ = (vector, is_nmi);
        }
    }

    // ============================================================
    // v2.0 master-clock R1 substrate — production overrides (Phase 1).
    // Compiled only under `mc-r1-substrate`; consulted by the R1 CPU loop
    // (Phases 2+). NOT exercised on the default build, so default behaviour
    // is byte-identical. Ported from refactor/v2.0-master-clock with the
    // trace + S1/S2 (mc-apu-subcycle / r4-cpu-dma) wiring stripped.
    // ============================================================

    /// Pure address-space read under R1 (the DMA drain happens in
    /// [`Bus::cpu_clock`]; Phase 3 will split the drain out of `cpu_read`).
    /// Phase 1 delegates to the legacy path so the contract compiles.
    fn read(&mut self, addr: u16) -> u8 {
        self.cpu_read(addr)
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.cpu_write(addr, value);
    }

    /// R1 master clocks per CPU cycle for the cartridge region (NTSC 12 / PAL
    /// 16 / Dendy 15) — the `cpu_divider` half of `region_dividers`.
    /// Drives the CPU loop's `master_clock` advance + read/write split so the
    /// CPU<->PPU phase is 3:1 NTSC, 3.2:1 PAL, 3:1 Dendy.
    fn cpu_divider(&self) -> u64 {
        u64::from(self.cpu_div_cached)
    }

    /// R1 double catch-up: tick whole PPU dots while
    /// `ppu_clock + ppu_divider <= target`.
    fn run_ppu_to(&mut self, target: u64) {
        let ppu_div = u64::from(self.ppu_div_cached);
        let mut sub_dot = 0u8;
        while self.ppu_clock + ppu_div <= target {
            let mut adapter = PpuBusAdapter {
                mapper: self.mapper.as_mut(),
                nt_override: self.nt_mirroring_override,
                sub_dot,
                #[cfg(feature = "irq-timing-trace")]
                trace_a12_latest: None,
            };
            self.ppu.tick(&mut adapter);
            self.sample_nmi_edge();
            self.ppu_clock += ppu_div;
            sub_dot = sub_dot.wrapping_add(1);
        }
    }

    /// R1: one CPU cycle of bus-side work (NO PPU advance — that lives in
    /// [`Bus::run_ppu_to`]). Controller strobe + bus-side DMA drain + cycle
    /// counter + per-cycle PPU/mapper hooks + APU tick. DMA stays bus-side
    /// (the pivot's working `service_dmc_dma`); Phase 3 wires the
    /// `dma_mc_consumed` coherence accounting.
    fn cpu_clock(&mut self) {
        // Diagnostic: snapshot the APU IRQ line (frame-counter | DMC) BEFORE
        // `apu_advance_one` runs the frame counter, so `trace_end_cycle` can
        // expose the within-cycle frame-counter SET (low=0 -> high=1) vs the
        // DMA `$4015` CLEAR (low=1 -> high=0) ordering. Only meaningful under
        // the trace feature; the field is otherwise unused on the R1 path.
        #[cfg(feature = "irq-timing-trace")]
        {
            self.irq_snapshot_apu_at_low = self.apu.irq_line();
            self.trace_r1_scanline_start = self.ppu.scanline();
            self.trace_r1_dot_start = self.ppu.dot();
            self.trace_r1_frame_start = self.ppu.frame();
        }
        if self.controller_write_pending > 0 {
            self.controller_write_pending -= 1;
            if self.controller_write_pending == 0 {
                let value = self.controller_write_value;
                self.commit_controller_strobe(value);
            }
        }
        self.drain_dma(None);
        self.cycle = self.cycle.wrapping_add(1);
        self.ppu.on_cpu_cycle();
        // v2.8.0 Phase 4 — skip the virtual dispatch on boards whose
        // `notify_cpu_cycle` is the default no-op (capability-flag cache).
        if self.mapper_caps.cpu_cycle_hook {
            self.mapper.notify_cpu_cycle();
        }
        // F-2: `apu_advance_one` (start) ticks the whole APU EXCEPT the DMC
        // byte-timer (gated out by `dmc_driven_externally`); the DMC is ticked
        // at end-of-cycle by `cpu_clock_apu_dmc`.
        self.apu_advance_one();
        // (W2 $2007 Stress) The deferred $2007 render-buffer reload is now
        // PPU-dot-scheduled and consumed inside `Ppu::tick` — the prior
        // per-CPU-cycle `apply_pending_render_buffer` hook here was quantized
        // to 3-dot steps and structurally aliased mod 3 against the test's
        // 1-dot-per-iteration clockslide.
    }

    // RA-1 (mc-r1-apu-unified-clock): the DMC byte-timer is now clocked at cycle
    // START (in `Apu::tick_with_external` via `apu_advance_one` in `cpu_clock`),
    // unified with the rest of the APU and advancing through the DMC DMA span,
    // matching Mesen's `ProcessCpuClock` at `StartCpuCycle`. So the END-of-cycle
    // DMC tick is a no-op here.
    fn cpu_clock_apu_dmc(&mut self) {
        // v2.0 Program M (M-1): clock the DMC byte-timer + arm the reload HERE at
        // end-of-cycle (after the CPU's bus access), the references' within-cycle
        // order. When the flag is OFF the byte-timer stays at cycle-start (above,
        // in `tick_with_external`) and this is a no-op -> floor byte-identical.
        // Runs BEFORE `promote_dmc_pending_next` so a reload armed at end-of-cycle
        // N latches `_next` and is promoted by this SAME call -> serviced N+1
        // (the floor service cadence), the byte-timer position being the only
        // shift (vs promote-before, which adds a full +1 service cycle and
        // over-shifts every DMA).
        self.apu.dmc_tick_end();
        // Visibility-delay: promote a reload latched this cycle at END (after the
        // CPU's bus access) so the NEXT cycle's DMA loop first-services it (put).
        self.apu.promote_dmc_pending_next();
    }

    fn take_dma_mc_consumed(&mut self) -> u64 {
        core::mem::take(&mut self.dma_mc_consumed)
    }

    fn irq_level(&self) -> bool {
        // v2.8.0 Phase 4 — boards without an IRQ source have the default
        // `irq_pending() == false`; skip the per-cycle virtual call.
        (self.mapper_caps.irq_source && self.mapper.irq_pending()) || self.apu.irq_line()
    }

    fn nmi_level(&self) -> bool {
        self.ppu.nmi_line()
    }

    fn dmc_dma_pending(&self) -> bool {
        self.apu.dmc_dma_pending()
    }

    fn dmc_dma_defer_load_entry(&self) -> bool {
        {
            // The while-gate runs PRE-cycle (before `start_cycle`'s APU tick).
            // Floor: the start-flip means the pre-cycle `!put_cycle` predicts
            // an access-point parity on the DMC noop half (defer it).
            // W3-Stage-2 (`mc-r1-dma-unified-collapse`): the flip moved to
            // end-of-cycle, so the pre-cycle value IS the upcoming
            // access-point label — the noop half is now the PUT half, so the
            // defer condition INVERTS to `put_cycle` (pre-cycle reads are
            // flip-invariant in value; the predicted half changes).
            let lands_on_noop_half = self.apu.put_cycle();
            self.apu.dmc_dma_pending()
                && self.apu.dmc_dma_is_load()
                && lands_on_noop_half
                && !self.in_dmc_dma
        }
    }

    fn dmc_dma_step(&mut self, halted_addr: u16) {
        self.dmc_dma_step_impl(halted_addr);
    }

    fn dmc_dma_step_idle(&mut self) {
        // Internal-cycle DMC halt: re-read the held (last CPU read) address.
        let halted = self.last_read_addr;
        self.dmc_dma_step_impl(halted);
    }

    // Stage-D: OAM DMA is pending (a `$4014` write awaits its first read cycle)
    // or in flight. The CPU `read1` loop drives it one cycle at a time.
    fn oam_dma_pending(&self) -> bool {
        self.dma_pending.is_some() || self.dma_cycles_owed > 0
    }

    // Stage-D: one CPU-driven OAM DMA cycle. First call latches the pending
    // `$4014` page + the 513/514 alignment count; subsequent calls run one
    // halt/align/read/write cycle. Does NOT advance time — the surrounding
    // `start_cycle`/`end_cycle` (and their `cpu_clock`/`run_ppu_to`/φ2 sample)
    // do, so each OAM cycle is interrupt-sampled like a normal CPU cycle (the
    // surface the bus burst bypassed). Mirrors `clock_oam_dma_cycle` minus the
    // `tick_one_cpu_cycle`.
    fn oam_dma_step(&mut self, halted_addr: u16) {
        if let Some(page) = self.dma_pending.take() {
            self.dma_page = page;
            self.dma_idx = 0;
            self.dma_halt_addr = halted_addr;
            let extra: u32 = if self.cycle & 1 == 0 { 514 } else { 513 };
            self.dma_cycles_owed = extra;
            self.dma_total = extra;
        }
        if self.dma_cycles_owed == 0 {
            return;
        }
        let total = self.dma_total;
        let alignment = if total == 514 { 2 } else { 1 };
        let consumed = total - self.dma_cycles_owed;
        if consumed < alignment {
            // Halt / alignment cycle: the held CPU address stays on the bus.
            #[cfg(feature = "irq-timing-trace")]
            self.set_trace_dma_access(BusAccess::DmaRead, self.dma_halt_addr, self.open_bus);
        } else {
            let xfer_idx = consumed - alignment; // 0..512
            if xfer_idx & 1 == 0 {
                let src_addr =
                    (u16::from(self.dma_page) << 8) | u16::try_from(xfer_idx >> 1).unwrap_or(0);
                self.dma_byte = self.raw_oam_dma_read(src_addr);
            } else {
                self.oam_dma_put();
            }
        }
        self.dma_cycles_owed -= 1;
        if self.dma_cycles_owed == 0 {
            self.dma_total = 0;
        }
    }

    // W3-Stage-1 (`mc-r1-dma-unified`): the unified engine's pending query.
    // Folds the floor's load-get-entry defer (the standalone DMC loop's
    // pre-flip while-gate: a deferred load alone does NOT hold the CPU — the
    // real read runs and the load enters on the next cycle, its get half) with
    // the OAM pending/in-flight state. The engine re-derives the same defer at
    // the access point for cycles the loop runs anyway because OAM is active.
    fn unified_dma_pending(&self) -> bool {
        let dmc = self.apu.dmc_dma_pending() && !Bus::dmc_dma_defer_load_entry(self);
        // W3-Stage-3 (`mc-r1-dmc-delayed-4015`): the TriCNES `_6502` line-4218
        // service gate — `DoDMCDMA && (APU_Status_DMC || implicit-abort)`. A
        // pending (or halted in-flight) DMC DMA whose APPLIED status dropped
        // is NOT serviced: the loop exits and the CPU resumes mid-DMA — the
        // emergent explicit abort. The engine's transient state (`in_dmc_dma`
        // / `dmc_halt` / the APU pending flag) persists, like TriCNES's stale
        // `DoDMCDMA`/`DMCDMA_Halt`, and resumes if the status re-applies.
        let dmc = dmc && self.apu.dmc_dma_serviceable();
        dmc || self.dma_pending.is_some() || self.uni_oam_active
    }

    // W3-Stage-1: one unified-engine cycle at a CPU read (the preempted
    // instruction/operand read supplies the parked 6502 address).
    fn unified_dma_cycle(&mut self, halted_addr: u16) {
        self.unified_dma_cycle_impl(halted_addr);
    }

    // W3-Stage-1: one unified-engine cycle at a CPU internal cycle — the bus
    // supplies its held (last-read) address, like `dmc_dma_step_idle`.
    fn unified_dma_cycle_idle(&mut self) {
        let halted = self.last_read_addr;
        self.unified_dma_cycle_impl(halted);
    }

    // Program M (M-2): an OAM DMA is started + still owes cycles (in flight),
    // distinct from `oam_dma_pending` (which also covers a not-yet-started write).
    fn oam_dma_in_flight(&self) -> bool {
        self.dma_cycles_owed > 0
    }

    // W3-Stage-0 (`mc-r1-counter-collapse`): a pending DMC DMA may overlap an OAM
    // DMA that is in flight OR still pending its first cycle. The collapse flag's
    // end-of-cycle byte-timer shift can surface the DMC arm in the one-iteration
    // gap between the `$4014` write and OAM's start-latch; lockstep `drain_dma`
    // latches OAM BEFORE its DMC-pending check, so the same arm overlaps OAM's
    // halt/alignment cycles there (the traced DMC+OAM Loop1 idx[6]/idx[7] events
    // with owed_at_begin == the FULL 514/513). Routing it to the standalone
    // `dmc_dma_step` instead pays a full unshared reload span = the idx[7] `03`.
    // Without the collapse flag the arm cannot surface in that gap, so the
    // original in-flight-only condition is preserved (audit-state invariant).
    fn oam_dma_overlap_ready(&self) -> bool {
        self.dma_cycles_owed > 0 || self.dma_pending.is_some()
    }

    // Program M (M-2): whether the most recent `dmc_dma_step` did the GET.
    fn dmc_dma_last_was_get(&self) -> bool {
        self.dmc_step_was_get
    }

    // Program M (M-2): advance ONE in-flight OAM cycle shared with a DMC halt
    // cycle. Mirrors the transfer/alignment body of `oam_dma_step` MINUS the
    // pending-latch (OAM is already in flight) and MINUS the time tick (the
    // surrounding start_cycle/end_cycle owns it). This is the per-cycle analogue
    // of lockstep `service_dmc_dma_during_oam` calling `clock_oam_dma_cycle` on
    // the DMC halt/dummy/align cycles, which is what produces the test's `02/01`
    // "DMC appears to take only 2/1 cycles" sweep entries.
    fn oam_dma_overlap_cycle(&mut self) {
        if self.dma_cycles_owed == 0 {
            return;
        }
        let total = self.dma_total;
        let alignment = if total == 514 { 2 } else { 1 };
        let consumed = total - self.dma_cycles_owed;
        if consumed >= alignment {
            let xfer_idx = consumed - alignment; // 0..512
            if xfer_idx & 1 == 0 {
                let src_addr =
                    (u16::from(self.dma_page) << 8) | u16::try_from(xfer_idx >> 1).unwrap_or(0);
                self.dma_byte = self.raw_oam_dma_read(src_addr);
            } else {
                self.oam_dma_put();
            }
        }
        self.dma_cycles_owed -= 1;
        if self.dma_cycles_owed == 0 {
            self.dma_total = 0;
        }
    }

    // Program M (M-2, exact): begin ONE DMC-DMA-during-OAM event. Direct port of
    // lockstep `service_dmc_dma_during_oam`'s prologue (bus.rs ~2067): latch the
    // halt + the open-bus replay and return the UNCONDITIONAL halt/dummy/align
    // noop count (`dmc_dma_short() ? 2 : 3`) — NOT parity-gated. This replaces the
    // prior per-cycle "share-on-every-non-GET" heuristic (which OVER-GLUED the
    // looping reloads, reading 2C=44 runaway at the test's `02` positions) with
    // lockstep's exact noop/GET/realign accounting bound to ONE DMC DMA.
    fn dmc_overlap_begin(&mut self, halted_addr: u16) -> u32 {
        // W3-Stage-0 (`mc-r1-counter-collapse` boundary-start): when this event
        // STARTS a pending (not-yet-latched) `$4014` OAM DMA, the OAM halt
        // address is the CPU read this DMA pair is preempting — the same value
        // `oam_dma_step` would have latched. The owed/total latch itself is
        // deferred to the first `dmc_overlap_noop_cycle` (inside the cycle's
        // `start_cycle`, so the 514/513 `self.cycle & 1` parity matches the
        // position `oam_dma_step` would have evaluated it).
        if self.dma_pending.is_some() {
            self.dma_halt_addr = halted_addr;
        }
        self.in_dmc_dma = true;
        self.dmc_step_was_get = false;
        self.capture_deferred_dma_replay();
        if self.apu.dmc_dma_short() { 2 } else { 3 }
    }

    // Program M (M-2, exact): one DMC halt/dummy/align cycle overlapping OAM.
    // Mirrors lockstep's noop-loop body: replay the held CPU read's side-effect,
    // then (if OAM still owes) advance one OAM slot — the 6502 is RDY-halted but
    // the OAM engine keeps its bus slot. The time tick is owned by the CPU's
    // surrounding start_cycle/end_cycle.
    fn dmc_overlap_noop_cycle(&mut self) {
        // W3-Stage-0 (`mc-r1-counter-collapse` boundary-start): latch a pending
        // `$4014` OAM DMA on the first shared halt cycle, mirroring
        // `oam_dma_step`'s start block at the same within-cycle position (after
        // `start_cycle`'s `cpu_clock` increments `self.cycle`, so the 514/513
        // parity choice is identical to the no-DMC counterfactual). The latched
        // OAM then consumes its halt/alignment/transfer slots through
        // `oam_dma_overlap_cycle` below, exactly like lockstep's
        // `service_dmc_dma_during_oam` after `drain_dma` started the OAM.
        if let Some(page) = self.dma_pending.take() {
            self.dma_page = page;
            self.dma_idx = 0;
            let extra: u32 = if self.cycle & 1 == 0 { 514 } else { 513 };
            self.dma_cycles_owed = extra;
            self.dma_total = extra;
        }
        let halted_addr = self.dma_halt_addr;
        self.replay_dma_noop_read(halted_addr);
        if self.dma_cycles_owed > 0 {
            self.oam_dma_overlap_cycle();
        } else {
            #[cfg(feature = "irq-timing-trace")]
            self.set_trace_dma_access(BusAccess::DmaRead, halted_addr, self.open_bus);
        }
    }

    // Program M (M-2, exact): the DMC GET cycle. Mirrors lockstep's get block +
    // the R1 `dmc_dma_step` GET (bus.rs ~2530): fetch the sample (with the
    // `$4000` open-bus conflict the cluster brackets), deliver it, and clear the
    // DMC-DMA pending state. OAM is STALLED — it does NOT advance on the GET.
    fn dmc_overlap_get_cycle(&mut self) {
        let halted_addr = self.dma_halt_addr;
        let addr = self.apu.dmc_dma_addr();
        let byte = self.dmc_dma_read(addr, halted_addr);
        #[cfg(feature = "irq-timing-trace")]
        self.set_trace_dma_access(BusAccess::DmaRead, addr, byte);
        self.apu.complete_dmc_dma(byte);
        self.in_dmc_dma = false;
        self.dmc_step_was_get = true;
    }

    // Program M (M-2, exact): the post-GET realign stall. Mirrors lockstep's
    // `if dma_cycles_owed > 0 { tick }` after the GET — ONE extra OAM-stalled
    // cycle (OAM does NOT advance; the parked CPU address stays on the bus) so
    // the next OAM read resumes on a later get. The cycle the prior per-cycle
    // scaffold was MISSING.
    fn dmc_overlap_realign_cycle(&mut self) {
        #[cfg(feature = "irq-timing-trace")]
        {
            let halted_addr = self.dma_halt_addr;
            self.set_trace_dma_access(BusAccess::DmaRead, halted_addr, self.open_bus);
        }
    }

    fn dmc_abort_pending(&self) -> bool {
        self.apu.dmc_abort_pending()
    }

    fn dmc_abort_is_get_cycle(&self) -> bool {
        // get = read half (TriCNES `!APU_PutCycle`); the 1-cycle abort DMA can
        // only land its halt on a get cycle.
        !self.apu.put_cycle()
    }

    fn dmc_abort_halt_step(&mut self, halted_addr: u16) {
        // 1-cycle abort DMA (Y=1): one halt re-read of the held CPU address (the
        // DMASync `$4000` the spin polls — drives the open-bus conflict), then
        // cancel the reload + the abort. The surrounding `read1` start/end_cycle
        // advances the clock, so CalculateDMADuration measures exactly 1 cycle.
        self.replay_dma_noop_read(halted_addr);
        #[cfg(feature = "irq-timing-trace")]
        self.set_trace_dma_access(BusAccess::DmaRead, halted_addr, self.open_bus);
        self.apu.cancel_dmc_dma();
    }

    fn dmc_abort_cancel(&mut self) {
        // Y=0: the abort matured on a put/write cycle — no DMA occurs. Clear the
        // reload + the abort with no halt cycle consumed.
        self.apu.cancel_dmc_dma();
    }

    #[cfg(not(feature = "irq-timing-trace"))]
    fn trace_end_cycle(&mut self) {}

    /// v2.0 R1c-1 diagnostic: record this instruction's `(pc, cpu_cycle)` into
    /// the per-instruction trace ring (default + R1 both; not mc-r1-gated).
    #[cfg(feature = "cpu-instr-cycle-trace")]
    fn trace_instr(&mut self, pc: u16, cpu_cycle: u64) {
        instr_trace::record(pc, cpu_cycle);
        // Latch the PC so the per-cycle `CycleRecord` push can stamp every
        // cycle (including DMA-insertion cycles, which hold this PC) with the
        // instruction currently executing — the TriCNES cross-diff landmark.
        #[cfg(feature = "irq-timing-trace")]
        {
            self.trace_last_pc = pc;
        }
    }

    /// R1-path per-cycle trace push (mirrors the `tick_one_cpu_cycle`
    /// `CycleRecord` build for the legacy path). `irq_pending_apu_at_low` was
    /// snapshotted at cycle-start in `cpu_clock` (before `apu_advance_one`);
    /// `_at_high` is read here at end-of-cycle (after the access + DMC tick), so
    /// a record where low=0/high=1 is a frame-counter SET this cycle and
    /// low=1/high=0 is a DMA `$4015` CLEAR this cycle — the ordering signal the
    /// `DMA + $4015` diagnostic needs.
    #[cfg(feature = "irq-timing-trace")]
    fn trace_end_cycle(&mut self) {
        if self.irq_trace.is_none() {
            return;
        }
        let events = core::mem::take(&mut self.trace_a12_scratch);
        let bus_access = core::mem::replace(&mut self.trace_bus_access, BusAccess::Idle);
        let bus_addr = core::mem::take(&mut self.trace_bus_addr);
        let bus_data = core::mem::take(&mut self.trace_bus_data);
        let mapper_irq = self.mapper.irq_pending();
        let rec = CycleRecord {
            cpu_cycle: self.cycle.wrapping_sub(1),
            pc: self.trace_last_pc,
            ppu_scanline: self.trace_r1_scanline_start,
            ppu_dot: self.trace_r1_dot_start,
            ppu_frame: self.trace_r1_frame_start,
            irq_pending_mapper_at_low: mapper_irq,
            irq_pending_apu_at_low: self.irq_snapshot_apu_at_low,
            irq_pending_mapper_at_high: mapper_irq,
            irq_pending_apu_at_high: self.apu.irq_line(),
            nmi_line: self.ppu.nmi_line(),
            a12_events: events,
            dmc_dma_pending_pre: false,
            dmc_dma_pending_post: self.apu.dmc_dma_pending(),
            dmc_dma_short_post: self.apu.dmc_dma_short(),
            dmc_abort_pending_post: self.apu.dmc_abort_pending(),
            dmc_abort_delay_post: self.apu.dmc_abort_delay(),
            dmc_dma_cooldown_post: self.apu.dmc_dma_cooldown(),
            dmc_dma_delay_post: self.apu.dmc_dma_delay(),
            apu_phase_post: self.apu.apu_phase(),
            in_dmc_dma: self.in_dmc_dma,
            dma_cycles_owed: self.dma_cycles_owed,
            bus_access,
            bus_addr,
            bus_data,
            put_cycle_post: self.apu.put_cycle(),
            dmc_timer_post: self.apu.dmc_timer(),
            dmc_bits_remaining_post: self.apu.dmc_bits_remaining(),
            dmc_silence_post: self.apu.dmc_silence(),
            dmc_buffer_full_post: self.apu.dmc_buffer_full(),
        };
        if let Some(t) = self.irq_trace.as_mut() {
            t.push(rec);
        }
    }
}

#[cfg(test)]
mod four_score_tests {
    use super::*;
    use crate::controller::Buttons;

    /// Minimal NROM (16-byte iNES header + 16 KiB PRG + 8 KiB CHR). Enough to
    /// construct a `LockstepBus`; these tests never run the CPU.
    fn test_bus() -> LockstepBus {
        let mut rom = Vec::with_capacity(16 + 0x4000 + 0x2000);
        rom.extend_from_slice(b"NES\x1A");
        rom.push(1); // 16 KiB PRG
        rom.push(1); // 8 KiB CHR
        rom.extend_from_slice(&[0u8; 10]);
        rom.extend_from_slice(&[0u8; 0x4000]);
        rom.extend_from_slice(&[0u8; 0x2000]);
        LockstepBus::new(&rom).expect("synthetic NROM parses")
    }

    fn strobe(bus: &mut LockstepBus) {
        bus.commit_controller_strobe(1);
        bus.commit_controller_strobe(0);
    }

    #[test]
    fn four_score_off_reads_like_standard_controller() {
        let mut bus = test_bus();
        assert!(!bus.four_score());
        bus.set_buttons(0, Buttons::A);
        strobe(&mut bus);
        // A, then 7 zeros, then 1s — exactly the standard pad.
        assert_eq!(bus.read_port(0), 1);
        for _ in 0..7 {
            assert_eq!(bus.read_port(0), 0);
        }
        for _ in 0..3 {
            assert_eq!(bus.read_port(0), 1);
        }
    }

    #[test]
    fn four_score_multiplexes_four_pads_and_signature() {
        let mut bus = test_bus();
        bus.set_four_score(true);
        bus.set_buttons(0, Buttons::A); // pad 1
        bus.set_buttons(2, Buttons::B); // pad 3
        bus.set_buttons(1, Buttons::SELECT); // pad 2
        bus.set_buttons(3, Buttons::START); // pad 4
        strobe(&mut bus);

        // Port 0 ($4016): pad1 (A) | pad3 (B) | signature 0x08 (LSB-first) | 1.
        let p0: Vec<u8> = (0..25).map(|_| bus.read_port(0)).collect();
        assert_eq!(&p0[0..8], &[1, 0, 0, 0, 0, 0, 0, 0], "pad 1: A");
        assert_eq!(&p0[8..16], &[0, 1, 0, 0, 0, 0, 0, 0], "pad 3: B");
        assert_eq!(&p0[16..24], &[0, 0, 0, 1, 0, 0, 0, 0], "signature 0x08");
        assert_eq!(p0[24], 1, "past 24 reads -> 1");

        // Port 1 ($4017): pad2 (Select) | pad4 (Start) | signature 0x04 | 1.
        let p1: Vec<u8> = (0..25).map(|_| bus.read_port(1)).collect();
        assert_eq!(&p1[0..8], &[0, 0, 1, 0, 0, 0, 0, 0], "pad 2: Select");
        assert_eq!(&p1[8..16], &[0, 0, 0, 1, 0, 0, 0, 0], "pad 4: Start");
        assert_eq!(&p1[16..24], &[0, 0, 1, 0, 0, 0, 0, 0], "signature 0x04");
        assert_eq!(p1[24], 1);
    }

    #[test]
    fn four_score_state_round_trips_through_save_state() {
        let mut bus = test_bus();
        bus.set_four_score(true);
        bus.set_buttons(2, Buttons::B | Buttons::A); // pad 3
        bus.set_buttons(3, Buttons::START); // pad 4
        strobe(&mut bus);
        let _ = bus.read_port(0); // advance idx[0] off zero
        let blob = crate::bus_snapshot::encode_bus(&bus);

        let mut restored = test_bus();
        crate::bus_snapshot::decode_bus(&mut restored, &blob).unwrap();
        assert!(restored.four_score());
        assert_eq!(restored.controller(2).buttons(), Buttons::B | Buttons::A);
        assert_eq!(restored.controller(3).buttons(), Buttons::START);
    }

    #[test]
    fn override_nt_addr_maps_per_mirroring() {
        use rustynes_mappers::Mirroring;
        // Logical tables $2000/$2400/$2800/$2C00, offset 0.
        // Horizontal: tables 0/1 -> bank 0, 2/3 -> bank 1.
        assert_eq!(override_nt_addr(Mirroring::Horizontal, 0x2000), 0x000);
        assert_eq!(override_nt_addr(Mirroring::Horizontal, 0x2400), 0x000);
        assert_eq!(override_nt_addr(Mirroring::Horizontal, 0x2800), 0x400);
        assert_eq!(override_nt_addr(Mirroring::Horizontal, 0x2C00), 0x400);
        // Vertical: tables 0/2 -> bank 0, 1/3 -> bank 1.
        assert_eq!(override_nt_addr(Mirroring::Vertical, 0x2000), 0x000);
        assert_eq!(override_nt_addr(Mirroring::Vertical, 0x2400), 0x400);
        assert_eq!(override_nt_addr(Mirroring::Vertical, 0x2800), 0x000);
        assert_eq!(override_nt_addr(Mirroring::Vertical, 0x2C00), 0x400);
        // Local offset preserved.
        assert_eq!(override_nt_addr(Mirroring::Vertical, 0x2456), 0x456);
    }

    #[test]
    fn mirroring_override_round_trips_through_save_state() {
        use rustynes_mappers::Mirroring;
        let mut bus = test_bus();
        assert_eq!(bus.mirroring_override(), None, "default is no override");
        bus.set_mirroring_override(Some(Mirroring::Vertical));
        let blob = crate::bus_snapshot::encode_bus(&bus);
        let mut restored = test_bus();
        crate::bus_snapshot::decode_bus(&mut restored, &blob).unwrap();
        assert_eq!(restored.mirroring_override(), Some(Mirroring::Vertical));
    }

    #[test]
    fn pre_v1_7_0_save_state_decodes_with_four_score_off() {
        // A v1.7.0 blob carries 11 trailing Four Score bytes (1 flag + 6
        // controllers34 + 2 idx + 2 sig); the W3-Stage-4 tail appends 22
        // more (dmc_halt + 3 uni_oam flags + uni_oam_addr u16 + ppu_clock
        // u64 + dma_mc_consumed u64); the v2.1.0 tail appends 2 more (one
        // expansion-device tag byte per port, both `None`); the v1.1.0 beta.1
        // tail appends 1 more (the nametable mirroring-override tag, `None`).
        // Truncating all 36 simulates a pre-v1.7.0 save, which must still load
        // with the adapter off (and no expansion device / override).
        let mut bus = test_bus();
        bus.set_four_score(true);
        let blob = crate::bus_snapshot::encode_bus(&bus);
        let old = &blob[..blob.len() - 36];
        let mut restored = test_bus();
        restored.set_four_score(true); // prove decode actively turns it off
        crate::bus_snapshot::decode_bus(&mut restored, old).unwrap();
        assert!(!restored.four_score());
    }

    #[test]
    fn expansion_device_state_round_trips_through_save_state() {
        use crate::input_device::{InputDevice, VausState, ZapperState};
        let mut bus = test_bus();
        // Vaus on port 0, Zapper on port 1, with distinctive non-default state.
        bus.set_expansion_device(0, Some(InputDevice::Vaus(VausState::new())));
        bus.set_paddle(0, 0x3C, true);
        bus.set_expansion_device(1, Some(InputDevice::Zapper(ZapperState::new())));
        bus.set_zapper(1, 100, 50, true);
        let blob = crate::bus_snapshot::encode_bus(&bus);

        let mut restored = test_bus();
        crate::bus_snapshot::decode_bus(&mut restored, &blob).unwrap();
        match restored.expansion_device(0) {
            Some(InputDevice::Vaus(v)) => {
                assert_eq!(v.position_raw(), 0x3C);
                assert!(v.fire_raw());
            }
            other => panic!("port 0 should be a Vaus, got {other:?}"),
        }
        match restored.expansion_device(1) {
            Some(InputDevice::Zapper(z)) => {
                assert_eq!(z.x_raw(), 100);
                assert_eq!(z.y_raw(), 50);
                assert!(z.trigger_raw());
            }
            other => panic!("port 1 should be a Zapper, got {other:?}"),
        }
    }

    #[test]
    fn pre_v2_1_0_save_state_decodes_with_no_expansion_device() {
        // A pre-v2.1.0 blob lacks the 2 trailing device-tag bytes (one None
        // tag per port); a pre-v1.1.0 blob also lacks the mirroring-override
        // tag. With nothing attached the encoder writes `[0, 0]` + `[0]`, so
        // truncating those 3 trailing bytes reproduces an older save — which
        // must still load with both ports unplugged and no override.
        let bus = test_bus();
        let blob = crate::bus_snapshot::encode_bus(&bus);
        let old = &blob[..blob.len() - 3];
        let mut restored = test_bus();
        crate::bus_snapshot::decode_bus(&mut restored, old).unwrap();
        assert!(restored.expansion_device(0).is_none());
        assert!(restored.expansion_device(1).is_none());
        assert_eq!(restored.mirroring_override(), None);
    }

    #[test]
    fn power_pad_state_round_trips_through_save_state() {
        use crate::input_device::{InputDevice, PowerPadState};
        let mut bus = test_bus();
        bus.set_expansion_device(1, Some(InputDevice::PowerPad(PowerPadState::new())));
        bus.set_power_pad(1, 0b1010_0101_0011);
        let blob = crate::bus_snapshot::encode_bus(&bus);
        let mut restored = test_bus();
        crate::bus_snapshot::decode_bus(&mut restored, &blob).unwrap();
        match restored.expansion_device(1) {
            Some(InputDevice::PowerPad(p)) => {
                assert_eq!(p.buttons_raw(), 0b1010_0101_0011);
            }
            other => panic!("expected a Power Pad on port 1, got {other:?}"),
        }
    }

    #[test]
    fn snes_mouse_state_round_trips_through_save_state() {
        use crate::input_device::{InputDevice, SnesMouseState};
        let mut bus = test_bus();
        bus.set_expansion_device(0, Some(InputDevice::SnesMouse(SnesMouseState::new())));
        bus.set_snes_mouse(0, -7, 9, true, false, 2);
        let blob = crate::bus_snapshot::encode_bus(&bus);
        let mut restored = test_bus();
        crate::bus_snapshot::decode_bus(&mut restored, &blob).unwrap();
        match restored.expansion_device(0) {
            Some(InputDevice::SnesMouse(m)) => {
                assert_eq!(m.dx_raw(), -7);
                assert_eq!(m.dy_raw(), 9);
                assert!(m.left_raw());
                assert!(!m.right_raw());
                assert_eq!(m.sensitivity_raw(), 2);
            }
            other => panic!("expected a SNES mouse on port 0, got {other:?}"),
        }
    }

    #[test]
    fn family_keyboard_state_round_trips_through_save_state() {
        use crate::input_device::{FamilyKeyboardState, InputDevice};
        let mut bus = test_bus();
        bus.set_expansion_device(
            1,
            Some(InputDevice::FamilyKeyboard(FamilyKeyboardState::new())),
        );
        let keys = [0x01, 0x10, 0x00, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00];
        bus.set_family_keyboard(1, keys);
        let blob = crate::bus_snapshot::encode_bus(&bus);
        let mut restored = test_bus();
        crate::bus_snapshot::decode_bus(&mut restored, &blob).unwrap();
        match restored.expansion_device(1) {
            Some(InputDevice::FamilyKeyboard(k)) => {
                assert_eq!(k.keys_raw(), keys);
            }
            other => panic!("expected a Family BASIC keyboard on port 1, got {other:?}"),
        }
    }
}

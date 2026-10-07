// SPDX-License-Identifier: GPL-3.0-or-later
//
// Provenance: this bus is RustyNES's own, but it incorporates models ported from TriCNES (MIT): the OAM-DMA register-window read (`oam_dma_read_reg_active`) is a direct port of TriCNES's `Fetch` address-bus-window block, and the unified DMA engine's state (`dmc_halt`, `uni_oam_active` / `_halt` / `_aligned` / `_addr`) is modelled on TriCNES's DMA flags. See docs/originality-and-provenance.md (Section 1)
// and NOTICE for the complete, audited derivation record.
//! The system bus behind the `Nes` facade.
//!
//! Per `docs/scheduler.md` §Bus design: [`SystemBus`] owns CPU RAM, the PPU,
//! the APU, the cartridge mapper, the controller ports and the two data-bus
//! latches, and implements `rustynes_cpu::Bus`. The CPU clocks every cycle in
//! two halves (ADR 0002 / ADR 0029): `run_ppu_to` catches the PPU up to the
//! master clock, `cpu_clock` runs the cycle-start work (APU, mapper hook,
//! the deferred controller strobe), the access is dispatched to the right
//! device, and `cpu_clock_apu_dmc` ticks the DMC at the cycle's end. OAM and
//! DMC DMA run through one unified engine (`unified_dma_cycle_impl`), one
//! full CPU cycle at a time.
//!
//! The type was `LockstepBus` until v2.9.8 (ADR 0042), a name left over from
//! the pre-v2.0.0 dot-lockstep scheduler.

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
    BgSplitState as PpuBgSplitState, ExAttribute as PpuExAttribute, PaletteInit, Ppu, PpuBus,
    PpuPalette, PpuRegion, PpuRevision, PpuSnapshotError,
};

use crate::Cpu2A03Revision;
use crate::controller::{Buttons, Controller};
#[cfg(feature = "irq-timing-trace")]
use crate::irq_trace::{BusAccess, CycleRecord, IrqTrace};
use crate::save_state::{self, SnapshotError};

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
/// [`SystemBus::set_event_breakpoints`]); the bit index is the discriminant.
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

/// The system bus.
///
/// Owns the entire emulator's mutable state. The CPU borrows `&mut SystemBus`
/// during `Cpu::step`, and the bus advances the PPU (`run_ppu_to`, 3 dots per
/// CPU cycle on NTSC / Dendy, 3.2 on PAL) and the APU (`cpu_clock`, every CPU
/// cycle) from the hooks the CPU calls around each access. Named
/// `LockstepBus` until v2.9.8 (ADR 0042).
// The bus carries many independent `bool` state words (the Four Score and
// Vs. flags, `in_dmc_dma`, the unified DMA engine's OAM latches, the
// debug-hook toggles). They are not a single enum-modelled machine, so
// silencing the lint is the right call.
#[allow(clippy::struct_excessive_bools)]
pub struct SystemBus {
    /// CPU RAM (2 KiB), mirrored every 0x800 bytes from `$0000-$1FFF`.
    pub(crate) ram: Box<[u8; RAM_SIZE]>,
    /// PPU instance.
    pub(crate) ppu: Ppu,
    /// APU instance.
    pub(crate) apu: Apu,
    /// Cartridge metadata (kept for save-state and debugger).
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
    /// v2.1.7 P5 — power-on 2 KiB work-RAM fill selection. [`crate::nes::PowerOnRam::Zeroed`]
    /// (default) leaves the established all-zero power-up state; the other
    /// variants are opt-in and deterministic. Stored so [`Self::power_cycle`] can
    /// re-apply the same fill after it zeroes RAM, keeping `power_cycle == fresh
    /// boot`. At the default this is inert (the zero fill matches `fresh_ram()`).
    power_on_ram: crate::nes::PowerOnRam,
    /// v2.1.7 P5 — selected 2C02 die revision (see [`PpuRevision`]). Stored so
    /// [`Self::power_cycle`] can re-apply it after the PPU is reconstructed
    /// (the PPU field is lost on rebuild, like the Vs. palette).
    /// [`PpuRevision::default`] models no extra behavior → byte-identical.
    ppu_die_revision: PpuRevision,
    /// v2.9.8 — which console's reset wiring is modelled (see
    /// [`crate::nes::ConsoleModel`]). Consulted at power-on and warm reset.
    /// [`crate::nes::ConsoleModel::Nes`] (default) is byte-identical to every
    /// earlier release. Config, not save-state.
    console_model: crate::nes::ConsoleModel,
    /// v2.1.7 P5 — selected power-up palette pattern (see [`PaletteInit`]).
    /// Re-applied on [`Self::power_cycle`] after the PPU (and thus its palette
    /// RAM) is rebuilt. [`PaletteInit::default`] is all-zero → byte-identical.
    power_up_palette: PaletteInit,
    /// Players 3 (`$4016`) and 4 (`$4017`) — only polled when
    /// [`Self::four_score`] is set.
    controllers34: [Controller; 2],
    /// Per-port Four Score read counter (0-7 = primary pad, 8-15 = secondary
    /// pad, 16-23 = signature, then 1s). Reset on each strobe.
    four_score_idx: [u8; 2],
    /// Does the Four Score chain owe a clock edge on this port?
    ///
    /// The exact counterpart of `Controller::pending_shift`, and it exists for
    /// the same reason. v2.6.5 made a contiguous read of a port return the same
    /// bit — `CLK` stays low across the run, so the pads do not advance — but
    /// the adapter's 24-read multiplexer went on advancing `four_score_idx` and
    /// shifting `four_score_sig` on EVERY read. The chain then ran ahead of the
    /// pads feeding it: a contiguous pair at index 7 moved to the pad-3 window
    /// after only seven advances of pad 1, and a pair inside the signature
    /// window consumed two signature bits where the hardware returns one twice.
    ///
    /// The Four Score is one shift chain, so it clocks on the same edge the
    /// pads do. Cleared by a strobe, exactly as `pending_shift` is.
    four_score_pending: [bool; 2],
    /// CPU cycle of the most recent read of each controller port, or
    /// `u64::MAX` for "never".
    ///
    /// `CLK` on the controller port is LOW only while `$4016`/`$4017` is being
    /// read, and the shift register advances on its low-to-high transition —
    /// when the read ENDS. Consecutive read cycles hold it low throughout and
    /// so produce one edge between them, not two. This is what says whether a
    /// read continues such a run. Every CPU cycle is a bus access in this core
    /// (ADR 0029), so cycle adjacency IS address-bus continuity.
    port_read_cycle: [u64; 2],
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
    /// via [`SystemBus::insert_coin`] and clears it with
    /// [`SystemBus::clear_coin`]. Vs.-System carts only.
    vs_coin: u8,
    /// Vs. System service button ($4016 bit 2). Vs.-System carts only.
    vs_service: bool,
    /// v2.0.0 beta.5 (Vs. `DualSystem`): `true` when this console is the SUB
    /// half of a `DualSystem` pair — `$4016` reads then return bit 7 = `0x80`
    /// (the main/sub identity bit the ROM polls; hard-pinned `0` on a single
    /// console, byte-identically). Set only by the `VsDualSystem` wrapper.
    vs_is_sub: bool,
    /// v2.0.0 beta.5 (Vs. `DualSystem`): the external `/IRQ` line driven by the
    /// PARTNER console's `$4016` bit-1 signal (Mesen2 `IRQSource::External`
    /// via `UpdateMainSubBit`). OR'd into [`Bus::irq_level`]; always `false`
    /// on a single console, so the default IRQ path is byte-identical.
    vs_external_irq: bool,
    /// v2.0.0 beta.5 (Vs. `DualSystem`): the last `$4016`-write bit-1 value
    /// (the main/sub comms signal) + a dirty latch the wrapper polls after
    /// each step batch. The bus only RECORDS the LEVEL (deliberately not
    /// edge-filtered — see [`Self::vs_4016_bit1_dirty`]); the cross-console
    /// wiring (asserting the partner's `/IRQ`, the shared-WRAM swap) lives in
    /// the wrapper — no bus ever references the other console.
    vs_4016_bit1: bool,
    /// See [`Self::vs_4016_bit1`] — set on EVERY `$4016` write, regardless
    /// of whether bit 1 changed; cleared by [`Self::take_vs_mainsub_edge`].
    /// Deliberately level-driven, not edge-filtered: at reset both consoles
    /// write `$4016 = $00` to establish the wrapper's seeded main/sub
    /// levels, and an edge filter starting from a `false` latch would
    /// swallow that seeded-HIGH -> written-LOW transition and deadlock the
    /// boot handshake (see the `cpu_write` `$4016` arm for the full
    /// rationale). Re-applying an unchanged level is idempotent in the
    /// wrapper, so marking every write dirty (not just changed ones) is
    /// correct, if conservatively named.
    vs_4016_bit1_dirty: bool,
    /// Optional non-standard input-device overlay per port (`$4016`/`$4017`).
    /// When a port has `Some(device)`, [`Self::read_port`] returns that
    /// device's byte instead of the standard controller / Four Score serial
    /// byte. `None` (the default) leaves the existing path byte-identical, so
    /// the default + Four Score reads and the determinism contract are
    /// unaffected unless a device is explicitly attached.
    expansion_device: [Option<crate::input_device::InputDevice>; 2],
    /// A3 (v2.2.3): serve a Zapper's light bit from the beam-relative
    /// temporal model instead of the frame-granular one. Default **on** since
    /// v2.3.6 (the constructor sets `true`); off restores the frame-granular
    /// model. See [`SystemBus::set_zapper_temporal_light`].
    zapper_temporal_light: bool,
    /// Famicom built-in **microphone** signal (v2.2.0 "Capstone"). The hardwired
    /// second Famicom controller carries a push-to-talk microphone whose state is
    /// read on **`$4016` bit 2** (not `$4017`) — games such as *The Legend of
    /// Zelda* (killing Pols Voice), *Kid Icarus*, *Raid on Bungeling Bay*, and
    /// *Takeshi no Chōsenjō* poll it. Modelled as a single live bit (the analog
    /// mic is quantized to "loud enough / not" by the frontend, matching how the
    /// Famicom's comparator fed the port): `true` ORs `1` into `$4016.D2`.
    /// Default `false` leaves the `$4016` read byte-identical (bit 2 is otherwise
    /// open-bus / 0), so the standard controller path is unaffected until a
    /// frontend explicitly drives the mic via [`Self::set_microphone`].
    famicom_mic: bool,
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
    /// OAM DMA scratch byte: read on even cycles, written on odd cycles.
    dma_byte: u8,
    /// OAM DMA active source page (latched from `dma_pending`).
    dma_page: u8,
    /// CPU read address that OAM DMA halted. While the CPU is halted,
    /// no-op DMA cycles keep this address on the 6502 core bus.
    dma_halt_addr: u16,

    /// v2.5.1 (ADR 0038) — externally asserted /NMI, for co-simulation only.
    ///
    /// Active-high here (`true` = the pin is asserted, i.e. /NMI low). It is
    /// OR'd into the poll rather than replacing it, so an injected NMI and a
    /// PPU-generated one are the same event to the CPU -- which is the point:
    /// the API sets the pin the CPU samples and does nothing else. It does not
    /// bypass the poll, force a vector, or short-circuit the sequence.
    ///
    /// The field does not exist in a default build.
    #[cfg(feature = "cosim-interrupt-inject")]
    inject_nmi: bool,
    /// v2.5.1 (ADR 0038) — externally asserted /IRQ. Level-sensitive, exactly
    /// as the pin is, so it is masked by `I` through the CPU's own logic and a
    /// pulse shorter than a poll is missed. Modelling it as a latch would make
    /// injected IRQs behave unlike real ones.
    #[cfg(feature = "cosim-interrupt-inject")]
    inject_irq: bool,

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
    /// re-read or the actual sample fetch. Read and written by the unified
    /// DMA engine (`unified_dma_cycle_impl`).
    dmc_halt: bool,
    /// v3.1.0 (`AccuracyCoin` "DMA Landing on Write", test 9): a pending LOAD
    /// DMC DMA reached the get half on which it would have entered, but that
    /// cycle was a CPU WRITE, and RDY cannot halt a write. The load then
    /// enters on the very next read whichever half it is, so a load refused
    /// by one write takes four cycles (`[Put (halt)] [Get] [Put] [Get]`)
    /// instead of being deferred a second cycle to its get half and taking
    /// three. Without this latch the CPU ran one real cycle the hardware
    /// spends halted. Set in [`Bus::write`], consumed by the DMC entry in
    /// `unified_dma_cycle_impl`, and cleared by the next CPU read either way.
    ///
    /// Written from the test ROM's own description (`TEST_DMALandingOnWrite`
    /// test 9 and its cycle comments) and pinned by a black-box per-cycle
    /// comparison against `TriCNES`'s output at the test's `STA $5000`. No
    /// emulator source was consulted.
    dmc_load_write_delayed: bool,
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

    /// v2.1.7 "Hardware Revisions & DMA Frontier" — the emulated Ricoh 2A03 die
    /// revision, gating the DMA unit's "unexpected DMA" extra halt-read on the
    /// DMC-halt-overlaps-OAM-halt cycle. **Default [`Cpu2A03Revision::Rp2A03G`]**
    /// = byte-identical to the pre-v2.1.7 core; it performs the extra read *in
    /// the model*, but that read is a documented no-op on every committed oracle
    /// (the parked address during a DMC+OAM overlap is never a side-effect
    /// register — see the enum docs + ADR 0033), so it changes nothing
    /// observable. [`Cpu2A03Revision::Rp2A03H`] omits the modeled read and is
    /// consequently byte-identical to `Rp2A03G` across the entire committed DMA
    /// corpus today (opt-in, deterministic, unverified direction). A config
    /// knob, NOT part of the save-state: the only state it influences (the
    /// parked-address side-effect re-read count during a DMC+OAM overlap) is
    /// fully re-derived from the deterministic timeline, so a save/restore
    /// round-trip stays byte-identical for a fixed revision.
    cpu_2a03_revision: Cpu2A03Revision,

    /// Active Game Genie codes, keyed by the PRG address they patch
    /// (`$8000-$FFFF`). Applied on the CPU read path; empty by default, so
    /// with no codes active reads are byte-identical to a build without the
    /// feature (the determinism contract is preserved). NOT part of the
    /// save-state — codes are a user overlay persisted by the frontend, not
    /// emulation state. See [`crate::genie`].
    genie_codes: BTreeMap<u16, GenieCode>,

    /// Deferred controller strobe write (Session-24 / Phase 3 of the
    /// v1.0.0-final brief).  Mirrors Mesen2's `NesControlManager`
    /// `_writeAddr` / `_writeValue` / `_writePending` triplet (see
    /// `Core/NES/NesControlManager.cpp` lines 252-273): a CPU write to
    /// `$4016` (or `$4017`) does NOT directly update the controllers'
    /// strobe state.  Instead the write is buffered here.
    /// `controller_write_pending` is set to 1 (odd-cycle write) or 2
    /// (even-cycle write) at the moment of the CPU write, then
    /// decremented every CPU cycle at the START of `cpu_clock` (the
    /// cycle-start half of the one-clock scheduler); when it reaches 0 the buffered
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

    /// APU-side IRQ line snapshotted at the start of each CPU cycle, before
    /// `apu_advance_one` runs the frame counter (`irq-timing-trace` only).
    /// `trace_end_cycle` pairs it with the end-of-cycle level so a record
    /// shows whether the frame-counter flag was SET or a `$4015` read CLEARED
    /// it within the cycle.
    ///
    /// v2.9.8 (ADR 0042) removed the four unconditional per-phase snapshots
    /// this replaced. They were written by the dead pre-v2.0.0
    /// `tick_one_cpu_cycle` and read only by the removed `poll_irq` /
    /// `poll_irq_at_phase`.
    #[cfg(feature = "irq-timing-trace")]
    irq_snapshot_apu_at_low: bool,

    /// Optional IRQ-timing trace buffer (Track C1 pre-work, gated on the
    /// `irq-timing-trace` cargo feature). See `crates/rustynes-core/src/irq_trace.rs`
    /// and ADR-0002 "Decision (revised, 2026-05-13)".
    #[cfg(feature = "irq-timing-trace")]
    pub(crate) irq_trace: Option<IrqTrace>,
    /// Session-21 (Sprint 1 iteration 2 prereq) bus-access tracker.
    ///
    /// Set by `cpu_read` / `cpu_write` / the unified DMA engine BEFORE
    /// `trace_end_cycle` records the per-cycle bus-access columns; consumed
    /// (and reset to `BusAccess::Idle` / 0) by `trace_end_cycle` when it
    /// pushes the record.  A single CPU cycle has at most one external
    /// bus access — burn cycles (`idle_tick`) leave the tracker at
    /// `BusAccess::Idle`, which is the correct semantics for the trace
    /// (CPU internal cycles do not drive the bus).
    ///
    /// The DMA paths set this directly because the bus owns the cycle
    /// during DMA halt and the CPU's `cpu_read` / `cpu_write` is not
    /// invoked (the bus's `raw_cpu_read` is invoked instead, which
    /// does not advance time on its own — the CPU's surrounding
    /// `start_cycle` / `end_cycle` do).
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
    /// `trace_end_cycle` diagnostic push.
    #[cfg(feature = "irq-timing-trace")]
    pub(crate) trace_r1_scanline_start: i16,
    #[cfg(feature = "irq-timing-trace")]
    pub(crate) trace_r1_dot_start: u16,
    #[cfg(feature = "irq-timing-trace")]
    pub(crate) trace_r1_frame_start: u64,
}

impl SystemBus {
    /// v2.7.0 -- reject a restored CPU/PPU clock pair too far apart to be real.
    ///
    /// `run_ppu_to` ticks the PPU until `ppu_clock` catches up to the CPU's
    /// `master_clock`. The two live in different save-state sections (BUS and
    /// CPU) and the running machine keeps them within a CPU cycle of each
    /// other, but a restore took both raw. A `ppu_clock` far BEHIND made the
    /// next catch-up tick billions of dots; one far AHEAD meant the PPU never
    /// ticked again, so no frame ever completed. Either is a hang, found by the
    /// v2.7.0 `save_state` fuzz target as a libFuzzer timeout once its patch
    /// offsets could reach the sections behind the framebuffer.
    ///
    /// The allowance, [`Self::RESTORED_CLOCK_SKEW_MAX`] master clocks, is ~85
    /// CPU cycles: generous against anything the machine produces, and it
    /// bounds the first catch-up at a few hundred dots.
    pub(crate) fn check_restored_clocks(&self, master_clock: u64) -> Result<(), SnapshotError> {
        // v2.9.0 (re-audit NC-04): the skew alone is not enough; see
        // [`Self::RESTORED_CLOCK_MAX`]. Checking the larger of the two is
        // enough to bound both, since they are then also within the skew.
        let highest = master_clock.max(self.ppu_clock);
        if highest > Self::RESTORED_CLOCK_MAX {
            return Err(SnapshotError::SectionInvalid {
                tag: "BUS ".into(),
                reason: format!(
                    "master clock {highest} exceeds the {} a real machine can reach",
                    Self::RESTORED_CLOCK_MAX
                ),
            });
        }
        let skew = master_clock.abs_diff(self.ppu_clock);
        if skew > Self::RESTORED_CLOCK_SKEW_MAX {
            return Err(SnapshotError::SectionInvalid {
                tag: "BUS ".into(),
                reason: format!(
                    "PPU clock {} is {skew} master clocks from the CPU's {master_clock}",
                    self.ppu_clock
                ),
            });
        }
        Ok(())
    }

    /// Largest CPU/PPU master-clock skew [`Self::check_restored_clocks`] accepts.
    pub(crate) const RESTORED_CLOCK_SKEW_MAX: u64 = 1024;

    /// Largest absolute master clock [`Self::check_restored_clocks`] accepts,
    /// for either clock: 2^62.
    ///
    /// v2.9.0 (re-audit NC-04). The skew bound alone let a crafted state put
    /// BOTH clocks just below 2^64; the CPU's `wrapping_add` then took its
    /// clock back to a small value within a frame while the PPU's stayed high,
    /// and `run_ppu_to`'s `ppu_clock + div <= target` never held again — the
    /// frozen-PPU state F-05 exists to reject.
    ///
    /// Why 2^62. It must sit far above anything a real machine reaches and far
    /// enough below 2^64 that no run from an accepted state can wrap. The
    /// fastest master clock is NTSC/Dendy's ~21.477 MHz (PAL's is slower), so
    /// 2^62 master clocks is ~2.1e11 s, about **6,800 years** of continuous
    /// emulation — no genuine state can be refused. The remaining headroom to
    /// the wrap is 3 x 2^62, about **20,000 years** more from the worst
    /// accepted state, so the catch-up loop's addition cannot overflow either.
    /// A power of two keeps the bound legible in a hex dump of a rejected blob.
    pub(crate) const RESTORED_CLOCK_MAX: u64 = 1 << 62;

    /// Test seam: move the PPU clock so a snapshot carries a chosen skew.
    #[cfg(test)]
    pub(crate) const fn set_ppu_clock_for_test(&mut self, v: u64) {
        self.ppu_clock = v;
    }

    /// Test seam: the PPU clock, for the skew tests.
    #[cfg(test)]
    pub(crate) const fn ppu_clock_for_test(&self) -> u64 {
        self.ppu_clock
    }

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
            // v2.1.7 P5 — power-on config knobs, all at their byte-identical
            // defaults (zeroed RAM, default revision, all-zero power-up palette).
            power_on_ram: crate::nes::PowerOnRam::Zeroed,
            ppu_die_revision: PpuRevision::Rp2c02H,
            console_model: crate::nes::ConsoleModel::Nes,
            power_up_palette: PaletteInit::Zeroed,
            controllers34: [Controller::new(); 2],
            four_score_idx: [0; 2],
            four_score_pending: [false; 2],
            port_read_cycle: [u64::MAX; 2],
            four_score_sig: [0; 2],
            #[cfg(feature = "debug-hooks")]
            controller_polled: false,
            vs_dip: 0,
            vs_coin: 0,
            vs_service: false,
            vs_is_sub: false,
            vs_external_irq: false,
            vs_4016_bit1: false,
            vs_4016_bit1_dirty: false,
            expansion_device: [None, None],
            // v2.3.6: ON by default. See `set_zapper_temporal_light` — the frame
            // model made a Duck Hunt hit impossible.
            zapper_temporal_light: true,
            famicom_mic: false,
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
            dma_byte: 0,
            dma_page: 0,
            dma_halt_addr: 0,
            #[cfg(feature = "cosim-interrupt-inject")]
            inject_nmi: false,
            #[cfg(feature = "cosim-interrupt-inject")]
            inject_irq: false,
            ppu_clock: 0,
            cpu_div_cached,
            ppu_div_cached,
            open_bus: 0,
            internal_data_bus: 0,
            last_read_addr: 0,
            deferred_dma_replay_addr: 0,
            in_dmc_dma: false,
            uni_oam_active: false,
            uni_oam_halt: false,
            uni_oam_aligned: false,
            uni_oam_addr: 0,
            cpu_2a03_revision: Cpu2A03Revision::default(),
            dmc_halt: false,
            dmc_load_write_delayed: false,
            genie_codes: BTreeMap::new(),
            #[cfg(feature = "irq-timing-trace")]
            irq_snapshot_apu_at_low: false,
            controller_write_pending: 0,
            controller_write_value: 0,
            #[cfg(feature = "irq-timing-trace")]
            irq_trace: None,
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
        let cart = Cartridge::synthetic(20, 0x8000, 0x2000);
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
        // Mapper 31: NSF banking is conventionally documented as mapper
        // 31-like. `synthetic` plays NTSC 60 Hz (vblank-NMI-driven) regardless
        // of the file's region preference; the PAL flag only feeds the
        // driver's init X-register. Exact non-60 Hz play rates are a
        // documented deferral (see `nsf.rs` module docs).
        let cart = Cartridge::synthetic(31, 0x2000, 0);
        Ok(Self::from_cart_and_mapper(
            cart,
            Box::new(mapper),
            sample_rate,
        ))
    }

    /// Reset (warm). Defers to `Ppu::reset` and clears DMA state. CPU is
    /// reset by the caller.
    pub fn reset(&mut self) {
        // v2.9.8 — on a Famicom the PPU's /RESET is tied to 5 V, so the Reset
        // button reaches only the CPU (NESdev "PPU power up state", §Famicom):
        // the PPU keeps PPUCTRL/PPUMASK, its latches and its frame position,
        // and no warm-up window is re-armed. The NES (default) resets both.
        if matches!(self.console_model, crate::nes::ConsoleModel::Nes) {
            self.ppu.reset();
        }
        self.apu.reset();
        {
            self.apu.set_dmc_driven_externally(true);
            self.apu.seed_apu_alignment(0);
        }
        self.dma_pending = None;
        self.dma_halt_addr = 0;
        self.deferred_dma_replay_addr = 0;
        self.unified_dma_clear();
        // v2.9.6: the boards that see the reset line (`Mapper::reset`).
        self.mapper.reset();
    }

    /// Power-cycle. Zeroes RAM and resets all state. Caller resets the CPU.
    pub fn power_cycle(&mut self) {
        self.ram.fill(0);
        // v2.9.8 — the PPU is rebuilt to its power-on state, but the host's
        // settings stored on it (custom palette, overclock scanlines, fast dot
        // path, OAM-decay model) are configuration, not console state: carry
        // them onto the new PPU, so every host gets a correct power cycle
        // without re-pushing them. Until v2.9.8 they reverted to their
        // defaults here. See `Ppu::adopt_settings_from`.
        let fresh_ppu = Ppu::new(self.ppu_region());
        #[cfg_attr(not(feature = "debug-hooks"), allow(unused_mut))]
        let mut prev_ppu = core::mem::replace(&mut self.ppu, fresh_ppu);
        self.ppu.adopt_settings_from(&prev_ppu);
        // The provenance stores stay ARMED across the cycle (the user asked
        // for them); `Nes::power_cycle` then empties them, since a cold boot
        // ends the history they describe. Until v2.9.8 they were dropped with
        // the old PPU, which made that clear a no-op.
        #[cfg(feature = "debug-hooks")]
        self.ppu.put_provenance(prev_ppu.take_provenance());
        drop(prev_ppu);
        // Re-apply the Vs./PC10 RGB-PPU configuration (lost when the PPU is
        // reconstructed). No-op for ConsoleType::Nes carts.
        self.reapply_vs_palette();
        // v2.1.7 P5 — re-apply the PPU-revision + power-up-palette config lost
        // when the PPU was reconstructed above, so `power_cycle == fresh boot`
        // holds for these knobs too (a core-only consumer that power-cycles
        // without a frontend still gets the configured hardware). All no-ops at
        // their defaults, so a default power-cycle stays byte-identical.
        self.ppu.set_revision(self.ppu_die_revision);
        self.ppu.apply_power_up_palette(self.power_up_palette);
        // v2.9.8 — the rebuilt PPU starts a fresh warm-up window; a Famicom's
        // closes before the CPU's first instruction. No-op on the NES.
        self.apply_console_model_power_on();
        // v2.1.7 P5 — re-apply the power-on work-RAM fill after the `fill(0)`
        // above. At the default (`Zeroed`) this is the same zero fill.
        self.apply_power_on_ram();
        // v2.9.8 — as for the PPU above: the rebuilt APU keeps the host's
        // channel mask, per-channel gain and filter model (until v2.9.8 they
        // reverted to their defaults), and its audio provenance stays armed
        // for `Nes::power_cycle` to empty. See `Apu::adopt_settings_from`.
        let fresh_apu = Apu::new(self.apu_region(), self.apu.sample_rate);
        #[cfg_attr(not(feature = "debug-hooks"), allow(unused_mut))]
        let mut prev_apu = core::mem::replace(&mut self.apu, fresh_apu);
        self.apu.adopt_settings_from(&prev_apu);
        #[cfg(feature = "debug-hooks")]
        self.apu
            .put_audio_provenance(prev_apu.take_audio_provenance());
        drop(prev_apu);
        {
            self.apu.set_dmc_driven_externally(true);
            self.apu.seed_apu_alignment(0);
        }
        self.controllers = [Controller::new(); 2];
        // The Four Score stays "plugged in" (it's hardware config), but its
        // transient strobe/read state resets like the controllers above.
        self.controllers34 = [Controller::new(); 2];
        self.four_score_idx = [0; 2];
        self.four_score_pending = [false; 2];
        self.four_score_sig = [0; 2];
        // Vs. System coin/service inputs are transient (DIP switches are
        // hardware config and persist across a power-cycle, like the panel).
        self.vs_coin = 0;
        self.vs_service = false;
        // v2.0.0 beta.5 (Vs. DualSystem): the comms latch + external IRQ are
        // transient signals; the sub identity is cabinet wiring and persists
        // (re-applied by the wrapper anyway).
        self.vs_external_irq = false;
        self.vs_4016_bit1 = false;
        self.vs_4016_bit1_dirty = false;
        // Non-standard input devices are unplugged on power-cycle (they are
        // re-attached explicitly by the frontend, like the controllers above).
        self.expansion_device = [None, None];
        // The microphone is a transient live signal; a power-cycle releases it
        // (the frontend re-drives it each frame while a key is held).
        self.famicom_mic = false;
        self.cycle = 0;
        self.dma_pending = None;
        self.dma_halt_addr = 0;
        self.open_bus = 0;
        self.internal_data_bus = 0;
        self.deferred_dma_replay_addr = 0;
        #[cfg(feature = "irq-timing-trace")]
        {
            self.irq_snapshot_apu_at_low = false;
        }
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
        self.last_read_addr = 0;
        self.in_dmc_dma = false;
        self.dmc_halt = false;
        self.dmc_load_write_delayed = false;
        self.controller_write_pending = 0;
        self.controller_write_value = 0;
        // v2.9.8 — the ports' last-read stamps are bus cycles of the OLD
        // timeline; `cycle` restarts at 0 above, so a kept stamp made the
        // cycled state depend on how long the console had run (and could, in
        // principle, read as "continues a run" against the new clock). A
        // fresh bus has never read either port.
        self.port_read_cycle = [u64::MAX; 2];
        // Rebuild the mapper to its power-on state (fresh bank registers, cleared
        // CHR-RAM + volatile PRG-RAM), so a power-cycle is a true cold boot for
        // mapper-stateful games (MMC1/MMC3/…) too — without this, a stateful
        // mapper's banking + CHR-RAM survive, so two netplay peers that power-
        // cycled from different running states would desync. The existing `cart`
        // metadata (incl. any post-load `set_vs_ppu_type` override) is kept; only
        // the mapper is replaced. FDS (`rom_bytes == None`) keeps its mapper.
        //
        // v2.9.0 — battery-backed PRG-RAM SURVIVES, as it does on a console.
        // Until v2.9.0 the rebuild cleared it too ("a battery-pull"), which was
        // harmless while RustyNES persisted no battery saves; from v2.7.3 the
        // desktop writes the live RAM to a `.sav` whenever it changes, so a
        // Power Cycle wrote zeros over the player's save. Volatile PRG-RAM and
        // CHR-RAM are still cleared. A power-on MOVIE wants cleared save RAM
        // and asks for it explicitly (`movie::power_on_for_movie`).
        let battery_ram: Option<Vec<u8>> = self
            .cart
            .has_battery
            .then(|| self.mapper.save_data().to_vec());
        if let Some(bytes) = self.rom_bytes.take() {
            if let Ok((_cart, mapper)) = rustynes_mappers::parse(&bytes) {
                self.mapper = mapper;
                if let Some(saved) = battery_ram.as_deref() {
                    let fresh = self.mapper.save_data_mut();
                    // Same ROM, same board: the sizes match. Guarded anyway,
                    // since a mismatch would mean the rebuild is not the board
                    // the RAM came from, and copying into it would be wrong.
                    if fresh.len() == saved.len() {
                        fresh.copy_from_slice(saved);
                    }
                }
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
        // `&mut *self.ram` reborrows the array: iterating `&mut Box<[T; N]>`
        // directly needs Rust 1.97+. The crate's floor is 1.99, so either form
        // compiles today; the reborrow dates from v3.0.1's one-day split, when
        // the libretro buildbot briefly built this crate on 1.96.
        for byte in &mut *self.ram {
            *byte = next();
        }
        self.open_bus = next();
    }

    /// Assert or release the injected /NMI pin. See [`crate::Nes::inject_nmi`].
    #[cfg(feature = "cosim-interrupt-inject")]
    pub const fn set_inject_nmi(&mut self, asserted: bool) {
        self.inject_nmi = asserted;
    }

    /// Assert or release the injected /IRQ pin. See [`crate::Nes::inject_irq`].
    #[cfg(feature = "cosim-interrupt-inject")]
    pub const fn set_inject_irq(&mut self, asserted: bool) {
        self.inject_irq = asserted;
    }

    /// Borrow the framebuffer (RGBA8, 256x240).
    #[must_use]
    pub fn framebuffer(&self) -> &[u8] {
        self.ppu.framebuffer()
    }

    /// v1.7.0 "Forge" Workstream B (B3) — overwrite the RGBA8 output framebuffer
    /// (the Lua `emu:setScreenBuffer`). Output-only; see
    /// [`rustynes_ppu::Ppu::debug_set_framebuffer`]. `debug-hooks`-gated.
    #[cfg(feature = "debug-hooks")]
    pub fn debug_set_framebuffer(&mut self, rgba: &[u8]) {
        self.ppu.debug_set_framebuffer(rgba);
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

    /// v2.1.8 A1 — enable/disable the specialized visible-scanline fast dot
    /// path. `false` (default) is byte-identical to a build without it. See
    /// [`rustynes_ppu::Ppu::set_fast_dotloop`].
    pub const fn set_fast_dotloop(&mut self, enabled: bool) {
        self.ppu.set_fast_dotloop(enabled);
    }

    /// v2.1.8 A1 — whether the visible-scanline fast dot path is enabled.
    #[must_use]
    pub const fn fast_dotloop(&self) -> bool {
        self.ppu.fast_dotloop()
    }

    /// v2.1.4 F2.3 — enable/disable the optional OAM-decay accuracy model.
    /// `false` (default) is byte-identical to a decay-free PPU. See
    /// [`rustynes_ppu::Ppu::set_oam_decay`].
    pub const fn set_oam_decay(&mut self, enabled: bool) {
        self.ppu.set_oam_decay(enabled);
    }

    /// v2.1.7 P5 — select the emulated 2C02 die revision, storing it so a
    /// power-cycle re-applies it, and applying it to the live PPU now. The
    /// default revision is byte-identical. See [`PpuRevision`].
    pub const fn set_ppu_revision(&mut self, revision: PpuRevision) {
        self.ppu_die_revision = revision;
        self.ppu.set_revision(revision);
    }

    /// v2.1.7 P5 — the currently-selected 2C02 die revision.
    #[must_use]
    pub const fn ppu_revision(&self) -> PpuRevision {
        self.ppu_die_revision
    }

    /// v2.9.8 — select the console's reset wiring (see
    /// [`crate::nes::ConsoleModel`]), storing it for [`Self::power_cycle`] and
    /// [`Self::reset`].
    ///
    /// Selecting [`crate::nes::ConsoleModel::Famicom`] also ends any PPU warm-up
    /// in progress, since a Famicom's PPU is never held in reset while the CPU
    /// runs; that is what gives a host that applies the knob straight after
    /// construction the Famicom power-on. At the default this is a store only.
    pub const fn set_console_model(&mut self, model: crate::nes::ConsoleModel) {
        self.console_model = model;
        self.apply_console_model_power_on();
    }

    /// v2.9.8 — the power-on half of the console model: on a Famicom the PPU
    /// left reset about one frame before the CPU, which is longer than the
    /// warm-up window, so the window is already closed. No-op on the NES.
    const fn apply_console_model_power_on(&mut self) {
        if matches!(self.console_model, crate::nes::ConsoleModel::Famicom) {
            self.ppu.end_warmup();
        }
    }

    /// v2.9.8 — the currently-selected console reset wiring.
    #[must_use]
    pub const fn console_model(&self) -> crate::nes::ConsoleModel {
        self.console_model
    }

    /// v2.1.7 P5 — apply a power-up palette-RAM pattern, storing it so a
    /// power-cycle re-applies it and writing it to the live PPU's palette RAM
    /// now. The default ([`PaletteInit::Zeroed`]) is byte-identical. See
    /// [`PaletteInit`].
    pub const fn set_power_up_palette(&mut self, init: PaletteInit) {
        self.power_up_palette = init;
        self.ppu.apply_power_up_palette(init);
    }

    /// v2.1.7 P5 — the currently-selected power-up palette pattern.
    #[must_use]
    pub const fn power_up_palette(&self) -> PaletteInit {
        self.power_up_palette
    }

    /// v2.1.7 P5 — select the power-on work-RAM fill, storing it so a
    /// power-cycle re-applies it, and applying it to the current RAM now. The
    /// default ([`crate::nes::PowerOnRam::Zeroed`]) is byte-identical. See [`crate::nes::PowerOnRam`].
    pub fn set_power_on_ram(&mut self, ram: crate::nes::PowerOnRam) {
        self.power_on_ram = ram;
        self.apply_power_on_ram();
    }

    /// v2.1.7 P5 — the currently-selected power-on work-RAM fill.
    #[must_use]
    pub const fn power_on_ram(&self) -> crate::nes::PowerOnRam {
        self.power_on_ram
    }

    /// v2.1.7 P5 — apply the stored [`Self::power_on_ram`] selection to the 2 KiB
    /// work RAM (and the open-bus latch). Called by [`Self::set_power_on_ram`]
    /// and re-applied by [`Self::power_cycle`] after it zeroes RAM. RAM is not
    /// consulted during the reset sequence (only the `$FFFC/D` vector is), so
    /// applying it here is correct. Deterministic: no wall-clock / OS RNG.
    fn apply_power_on_ram(&mut self) {
        match self.power_on_ram {
            crate::nes::PowerOnRam::Zeroed => {
                self.ram.fill(0);
                self.open_bus = 0;
            }
            crate::nes::PowerOnRam::Seeded(seed) => self.randomize_power_on_ram(seed),
            crate::nes::PowerOnRam::Filled(byte) => {
                self.ram.fill(byte);
                self.open_bus = byte;
            }
        }
    }

    /// v2.1.4 F2.3 — whether the optional OAM-decay model is enabled.
    #[must_use]
    pub const fn oam_decay_enabled(&self) -> bool {
        self.ppu.oam_decay_enabled()
    }

    /// v2.1.7 — set the emulated 2A03 die revision (DMA "unexpected read" axis).
    /// [`Cpu2A03Revision::Rp2A03G`] (default) is byte-identical to the pre-v2.1.7
    /// core; [`Cpu2A03Revision::Rp2A03H`] is the opt-in later-die model. See the
    /// [`Cpu2A03Revision`] docs + ADR 0033.
    pub const fn set_cpu_2a03_revision(&mut self, revision: Cpu2A03Revision) {
        self.cpu_2a03_revision = revision;
    }

    /// v2.1.7 — the configured 2A03 die revision (default
    /// [`Cpu2A03Revision::Rp2A03G`]).
    #[must_use]
    pub const fn cpu_2a03_revision(&self) -> Cpu2A03Revision {
        self.cpu_2a03_revision
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
            0x4016 => 0x40 | self.peek_port(0) | (u8::from(self.famicom_mic) << 2),
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
                let addr = if addr >= 0x3000 && !self.mapper.nametable_unfolded() {
                    addr - 0x1000
                } else {
                    addr
                };
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
                let nt_addr = if addr >= 0x3000 && !self.mapper.nametable_unfolded() {
                    addr - 0x1000
                } else {
                    addr
                };
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

    /// Enable the **beam-relative** Zapper light model.
    ///
    /// **Default ON since v2.3.6.** The light bit is derived from where the CRT
    /// beam is at the moment of the `$4016`/`$4017` read: dark before the beam
    /// paints the aim row, lit while the photodiode holds (~19-26 scanlines,
    /// per the `NESdev` wiki's capacitor model), dark once it drains. That is what
    /// real hardware does, and the frame-granular model structurally cannot
    /// express it — it returns one answer for the whole frame, sampled at
    /// end-of-frame, so every read during frame N reports frame N-1.
    ///
    /// # Why it was promoted (v2.3.6)
    ///
    /// A3 (v2.2.3) shipped this off, on the reasoning that "there is no pass/fail
    /// light-gun test ROM… the supported titles re-poll every frame and are
    /// satisfied by either model". **The second half of that was false**, and no
    /// test ROM was needed to show it — the game itself is the oracle.
    ///
    /// *Duck Hunt* requires the gun to see **nothing for one frame** and then a
    /// bright spot in the next. Under the frame model it received exactly the
    /// inverse: on the blanked frame it read the previous (bright) frame's
    /// answer, and on the target frame it read the blanked frame's. The shot was
    /// discarded before hit-testing, so the gun fired and **nothing could ever be
    /// hit** — reported by the maintainer, then reproduced headlessly from the
    /// game's own `$4017` traffic (`zapper_light_probe`).
    ///
    /// Measured A/B on the same ROM, aim and inputs: frame model → score 000000,
    /// duck still flying; beam-relative → score 000500, duck marked hit.
    ///
    /// Turning it off restores the pre-v2.3.6 frame-granular behaviour.
    /// Deterministic either way: the answer is a pure function of framebuffer +
    /// aim + scanline and holds no state, so it adds nothing to serialize.
    pub const fn set_zapper_temporal_light(&mut self, on: bool) {
        self.zapper_temporal_light = on;
    }

    /// Whether the beam-relative Zapper light model is enabled (A3).
    #[must_use]
    pub const fn zapper_temporal_light(&self) -> bool {
        self.zapper_temporal_light
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

    /// Drive the Famicom built-in microphone signal (read on `$4016` bit 2).
    ///
    /// `pressed` = the frontend's quantized "mic is loud" verdict. Additive:
    /// leaving it `false` (the default) keeps the `$4016` read byte-identical.
    pub const fn set_microphone(&mut self, pressed: bool) {
        self.famicom_mic = pressed;
    }

    /// Whether the Famicom microphone signal is currently asserted.
    #[must_use]
    pub const fn microphone(&self) -> bool {
        self.famicom_mic
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

    /// Whether the loaded mapper hardwires its nametable mirroring (so an
    /// external mirroring correction is safe to honor). See
    /// [`rustynes_mappers::Mapper::has_hardwired_mirroring`].
    #[must_use]
    pub fn mapper_has_hardwired_mirroring(&self) -> bool {
        self.mapper.has_hardwired_mirroring()
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

    /// v2.0.0 beta.5 (Vs. `DualSystem`): mark this console as the SUB half of a
    /// `DualSystem` pair (`$4016` reads return bit 7 = `0x80`). Wrapper-only.
    pub const fn set_vs_sub(&mut self, is_sub: bool) {
        self.vs_is_sub = is_sub;
    }

    /// v2.0.0 beta.5 (Vs. `DualSystem`): drive this console's external `/IRQ`
    /// line (the partner console's `$4016` bit-1 signal, Mesen2
    /// `IRQSource::External`). Wrapper-only; OR'd into the IRQ level.
    pub const fn set_vs_external_irq(&mut self, asserted: bool) {
        self.vs_external_irq = asserted;
    }

    /// v2.0.0 beta.5 (Vs. `DualSystem`): poll-and-clear the latched `$4016`
    /// bit-1 (main/sub comms signal) LEVEL. Returns `Some(level)` whenever
    /// this console wrote `$4016` since the last poll — deliberately
    /// level-driven, not edge-filtered (see the `vs_4016_bit1_dirty` field
    /// doc); the wrapper turns the level into the partner's external-IRQ
    /// assert (LOW asserts, HIGH clears). The shared-WRAM convergence
    /// (`pump_comms`'s separate `drain_vs_dual_wram_writes` step) runs on
    /// BOTH consoles every poll, independent of this bit-1 signal.
    pub const fn take_vs_mainsub_edge(&mut self) -> Option<bool> {
        if self.vs_4016_bit1_dirty {
            self.vs_4016_bit1_dirty = false;
            Some(self.vs_4016_bit1)
        } else {
            None
        }
    }

    /// v2.0.0 beta.5 (Vs. `DualSystem`): provision the mapper-99 shared 2 KiB
    /// WRAM window (`$6000-$7FFF`). Wrapper-only; no-op on other boards.
    pub fn enable_vs_dual_wram(&mut self) {
        self.mapper.enable_vs_dual_wram();
    }

    /// v2.0.0 beta.5 (Vs. `DualSystem`): mark the mapper as the SUB
    /// console's instance (banks the second PRG half + upper CHR pages —
    /// the two CPUs run different programs). Wrapper-only cabinet wiring.
    pub fn set_vs_dual_sub(&mut self) {
        self.mapper.set_vs_dual_sub();
    }

    /// v2.0.0 beta.5 (Vs. `DualSystem`): drain this console's shared-WRAM
    /// write log for the wrapper to replay into the partner console (the
    /// fully-shared MAME model). Empty off-board. Allocates a fresh `Vec`
    /// each call — fine for diagnostics/tests, NOT used by the hot
    /// `pump_comms` path (see [`Self::drain_vs_dual_wram_writes`]).
    pub fn take_vs_dual_wram_writes(&mut self) -> alloc::vec::Vec<(u16, u8)> {
        self.mapper.take_vs_dual_wram_writes()
    }

    /// v2.0.0 beta.5 (Vs. `DualSystem`): drain this console's shared-WRAM
    /// write log into a caller-owned, reusable `dst` buffer — the
    /// hot-path counterpart of [`Self::take_vs_dual_wram_writes`], used by
    /// `VsDualSystem::pump_comms` (called after every stepped instruction)
    /// to avoid allocating a fresh `Vec` on every call.
    pub fn drain_vs_dual_wram_writes(&mut self, dst: &mut alloc::vec::Vec<(u16, u8)>) {
        self.mapper.drain_vs_dual_wram_writes(dst);
    }

    /// v2.0.0 beta.5 (Vs. `DualSystem`): replay one partner-console write
    /// into this console's shared-WRAM copy (no re-log).
    pub fn apply_vs_dual_wram_write(&mut self, offset: u16, value: u8) {
        self.mapper.apply_vs_dual_wram_write(offset, value);
    }

    /// v2.0.0 beta.5 (Vs. `DualSystem`): take the shared-WRAM copy (wrapper
    /// snapshot-restore normalization). `None` off-board.
    pub fn take_vs_dual_wram(&mut self) -> Option<alloc::boxed::Box<[u8]>> {
        self.mapper.take_vs_dual_wram()
    }

    /// v2.0.0 beta.5 (Vs. `DualSystem`): install a shared-WRAM copy (the
    /// other half of the restore normalization).
    pub fn set_vs_dual_wram(&mut self, wram: alloc::boxed::Box<[u8]>) {
        self.mapper.set_vs_dual_wram(wram);
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
    /// DIP switch 2, bit 5 = coin #1, bit 6 = coin #2, bit 7 = primary CPU.
    /// Bit 7 is `0` on a single console / the `DualSystem` MAIN half and `0x80`
    /// on the `DualSystem` SUB half (Mesen2 `IsVsMainConsole() ? 0 : 0x80` —
    /// the identity bit the `DualSystem` ROM polls; v2.0.0 beta.5).
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
        // v2.0.0 beta.5: the DualSystem main/sub identity bit.
        if self.vs_is_sub {
            v |= 0x80;
        }
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
        // v2.9.8 — the id is cartridge metadata, like the submapper below. A
        // board's `debug_info` names its own id only when it overrides the
        // default, which names mapper 0, so the debugger's mapper panel showed
        // "Mapper 0" for `UxROM`, CNROM, `AxROM` and every other board without
        // an override (and for an NSF, whose synthetic cartridge is mapper 31).
        info.mapper_id = cart.mapper_id;
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

    /// The CPU cycle of `port`'s most recent read (`u64::MAX` = never), for
    /// the save state.
    #[must_use]
    pub const fn port_read_cycle(&self, port: usize) -> u64 {
        self.port_read_cycle[port]
    }

    /// Restore the controller-port CLK run state: four `pending_shift` flags
    /// (ports 1-2 then the Four Score's 3-4) and the two per-port read cycles.
    /// The Four Score chain's owed-edge flags, for the snapshot.
    #[must_use]
    pub const fn four_score_pending(&self) -> [bool; 2] {
        self.four_score_pending
    }

    /// Restore the Four Score chain's owed-edge flags. See
    /// [`Self::set_controller_run_state`]; kept beside it because the two are
    /// one piece of state split across two devices.
    pub const fn set_four_score_pending(&mut self, pending: [bool; 2]) {
        self.four_score_pending = pending;
    }

    /// Restore the controller-port CLK run state: four `pending_shift` flags
    /// (ports 1-2 then the Four Score's 3-4) and the two per-port read cycles.
    pub const fn set_controller_run_state(&mut self, pending: [bool; 4], cycles: [u64; 2]) {
        self.controllers[0].pending_shift = pending[0];
        self.controllers[1].pending_shift = pending[1];
        self.controllers34[0].pending_shift = pending[2];
        self.controllers34[1].pending_shift = pending[3];
        self.port_read_cycle[0] = cycles[0];
        self.port_read_cycle[1] = cycles[1];
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
            // The chain owes nothing immediately after a strobe, so the FIRST
            // read serves index 0 rather than advancing past it.
            self.four_score_pending = [false, false];
        }
    }

    /// Read the D0 controller bit for `port` (0 = `$4016`, 1 = `$4017`),
    /// advancing the shift register. Four Score off → just
    /// `controllers[port].read()`; on → the multiplexed 24-read sequence
    /// (primary pad → secondary pad → signature → 1s).
    /// Does a read of `port` on this cycle continue an unbroken run of reads
    /// of the same port? Records this cycle as the port's latest read either
    /// way, so callers must invoke it exactly once per read.
    const fn port_continues_run(&mut self, port: usize) -> bool {
        let last = self.port_read_cycle[port];
        let cont = last != u64::MAX && self.cycle == last.wrapping_add(1);
        self.port_read_cycle[port] = self.cycle;
        cont
    }

    fn read_port(&mut self, port: usize) -> u8 {
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
        // A3 (v2.2.3, opt-in): serve the Zapper's light bit from the
        // beam-relative model. `read_at_scanline` takes `&self` and the PPU is a
        // different field, so these are disjoint borrows. Off by default, so the
        // shipped path below is byte-identical.
        if self.zapper_temporal_light
            && let Some(crate::input_device::InputDevice::Zapper(z)) = &self.expansion_device[port]
        {
            // `scanline()` is `i16` but is non-negative on every current region
            // (visible 0..=239, then post-render / vblank up to the pre-render
            // line — 261 NTSC / 311 PAL, NOT -1), so `try_from` always succeeds
            // and this resolves to `read_at_scanline`, which already yields
            // no-light for the pre-render line (`prerender - y >= HOLD` for every
            // visible aim). The `Err` arm is a total-conversion fallback: if a
            // future convention ever produced a negative scanline (a -1
            // pre-render), the correct answer is "no light yet" —
            // `read_before_visible` — rather than the row-0 fold a bare
            // `unwrap_or(0)` would give.
            return match u16::try_from(self.ppu.scanline()) {
                Ok(sl) => z.read_at_scanline(self.ppu.framebuffer(), sl),
                Err(_) => z.read_before_visible(),
            };
        }
        if let Some(d) = &mut self.expansion_device[port] {
            return d.read();
        }
        let cont = self.port_continues_run(port);
        if !self.four_score || self.controllers[port].strobe {
            return self.controllers[port].read(cont);
        }
        // ADVANCE FIRST, THEN SERVE — the same shape as `Controller::read`,
        // and for the same reason. The chain clocks on the rising edge that
        // ENDS the previous run, so a contiguous read serves the position it
        // already served instead of stepping past it. Advancing after the
        // serve, unconditionally, is what let the adapter run ahead of the pads
        // feeding it once contiguous reads stopped advancing them.
        if self.four_score_pending[port] && !cont && self.four_score_idx[port] < 24 {
            if self.four_score_idx[port] >= 16 {
                self.four_score_sig[port] = (self.four_score_sig[port] >> 1) | 0x80;
            }
            self.four_score_idx[port] += 1;
        }
        self.four_score_pending[port] = true;
        let idx = self.four_score_idx[port];
        if idx < 8 {
            self.controllers[port].read(cont)
        } else if idx < 16 {
            self.controllers34[port].read(cont)
        } else if idx < 24 {
            self.four_score_sig[port] & 1
        } else {
            1
        }
    }

    /// Side-effect-free companion to [`Self::read_port`] (debugger peek).
    fn peek_port(&self, port: usize) -> u8 {
        // Mirror the temporal-Zapper branch in `read_port` so a debugger peek
        // shows the same `$4016`/`$4017` byte the CPU would receive. Without
        // this, with `zapper_temporal_light` on, `peek_port` fell through to the
        // overlay's frame-granular `peek()` and could disagree with the live
        // read. `peek` is non-mutating and all of `scanline()` / `framebuffer()`
        // / `read_at_scanline` / `read_before_visible` take `&self`, so this is a
        // pure read; it costs `peek_port` its `const` (try_from/match are not
        // const here), which nothing relied on. Off by default → byte-identical.
        if self.zapper_temporal_light
            && let Some(crate::input_device::InputDevice::Zapper(z)) = &self.expansion_device[port]
        {
            return u16::try_from(self.ppu.scanline()).map_or_else(
                |_| z.read_before_visible(),
                |sl| z.read_at_scanline(self.ppu.framebuffer(), sl),
            );
        }
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
            dma_byte: self.dma_byte,
            dma_page: self.dma_page,
            dma_halt_addr: self.dma_halt_addr,
            open_bus: self.open_bus,
            internal_data_bus: self.internal_data_bus,
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
            dmc_load_write_delayed: self.dmc_load_write_delayed,
            uni_oam_active: self.uni_oam_active,
            uni_oam_halt: self.uni_oam_halt,
            uni_oam_aligned: self.uni_oam_aligned,
            uni_oam_addr: self.uni_oam_addr,
            ppu_clock: self.ppu_clock,
        }
    }

    /// Apply a previously-snapshotted bus bookkeeping state.
    pub const fn set_bus_misc_state(&mut self, s: crate::bus_snapshot::BusMiscState) {
        self.dma_pending = s.dma_pending;
        self.dma_byte = s.dma_byte;
        self.dma_page = s.dma_page;
        self.dma_halt_addr = s.dma_halt_addr;
        self.open_bus = s.open_bus;
        self.internal_data_bus = s.internal_data_bus;
        self.last_read_addr = s.last_read_addr;
        self.deferred_dma_replay_addr = s.deferred_dma_replay_addr;
        self.in_dmc_dma = s.in_dmc_dma;
        self.controller_write_pending = s.controller_write_pending;
        self.controller_write_value = s.controller_write_value;
        self.four_score = s.four_score;
        self.four_score_idx = s.four_score_idx;
        self.four_score_sig = s.four_score_sig;
        // W3-Stage-4 (2026-06-10): the unified engine's OAM state + the DMC
        // halt latch are serialized, replacing the Stage-1 clear-on-restore.
        // Snapshots are taken at instruction boundaries where the engine is
        // idle, so for every legitimately produced blob these decode to the
        // same inactive state the clear imposed -- but a restored blob
        // reproduces them EXACTLY instead of by assumption.
        self.dmc_halt = s.dmc_halt;
        self.dmc_load_write_delayed = s.dmc_load_write_delayed;
        self.uni_oam_active = s.uni_oam_active;
        self.uni_oam_halt = s.uni_oam_halt;
        self.uni_oam_aligned = s.uni_oam_aligned;
        self.uni_oam_addr = s.uni_oam_addr;
        // Half of the master-clock pair; the other half is
        // `Cpu::master_clock` in the CPU section (see `BusMiscState::ppu_clock`).
        self.ppu_clock = s.ppu_clock;
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
        self.snapshot_into_with(out, rom_hash_tag, false);
    }

    /// v2.3.3 — [`Self::snapshot_into`] with the PPU encoded slim (no
    /// framebuffer). See `rustynes_ppu::PPU_SNAPSHOT_SLIM_FLAG`.
    pub fn snapshot_into_slim(
        &self,
        out: &mut Vec<u8>,
        rom_hash_tag: [u8; save_state::ROM_HASH_TAG_LEN],
    ) {
        self.snapshot_into_with(out, rom_hash_tag, true);
    }

    fn snapshot_into_with(
        &self,
        out: &mut Vec<u8>,
        rom_hash_tag: [u8; save_state::ROM_HASH_TAG_LEN],
        slim: bool,
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
        let ppu_body = if slim {
            self.ppu.snapshot_slim()
        } else {
            self.ppu.snapshot()
        };
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
        // carries the exact `put_cycle` / `parity_seed` phase, so the boot
        // alignment is NOT re-seeded -- that would overwrite the restored
        // mid-state parity the counter-collapse end-flip reads at the next
        // access point. (Until v2.9.8 a pre-Stage-4 blob without that tail
        // was accepted and re-seeded here; ADR 0042 removed that path.)
        self.apu.set_dmc_driven_externally(true);
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
    /// `trace_end_cycle` consumes this when it pushes the record.
    /// No-op (no field even exists) when the trace feature is disabled.
    #[cfg(feature = "irq-timing-trace")]
    const fn set_trace_dma_access(&mut self, access: BusAccess, addr: u16, data: u8) {
        self.trace_bus_access = access;
        self.trace_bus_addr = addr;
        self.trace_bus_data = data;
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
                };
                let _ = self.ppu.cpu_read_register(7, &mut adapter);
            }
            0x4015 => {
                let _ = self.apu.read_status();
                self.apu.clear_frame_irq_immediate_for_dma();
            }
            0x4016 => {
                let cont = self.port_continues_run(0);
                let _ = self.controllers[0].read(cont);
            }
            0x4017 => {
                let cont = self.port_continues_run(1);
                let _ = self.controllers[1].read(cont);
            }
            _ => {}
        }
    }

    /// Read the DMC sample byte and model the 2A03 register-conflict path
    /// where 6502 core address bits 15..=5 remain from the halted CPU read
    /// while DMA supplies address bits 4..=0.
    fn dmc_dma_read(&mut self, addr: u16, halted_addr: u16) -> u8 {
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
                // Keep the DMC-conflict $4016 composition consistent with the
                // normal controller read (line ~3890): D2 carries the Famicom
                // built-in microphone. Default-off (mic released) leaves `mic`
                // = 0, so the returned byte is byte-identical to prior releases.
                let mic = u8::from(self.famicom_mic) << 2;
                let cont = self.port_continues_run(0);
                let v = (sample & 0xE0) | self.controllers[0].read(cont) | mic;
                self.open_bus = v;
                v
            }
            0x4017 => {
                let cont = self.port_continues_run(1);
                let v = (sample & 0xE0) | self.controllers[1].read(cont);
                self.open_bus = v;
                v
            }
            _ => sample,
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
    /// table (the instrumented harness's `Emulator.cs` ~4233-4357; out of the
    /// repository since v3.0.1, at `~/reference-oracles/TriCNES-rustynes-harness`), the SINGLE driver that standalone DMC,
    /// standalone OAM, and the DMC-during-OAM overlap all ride — AT FLOOR
    /// PARITY for this stage (the structural-equivalence proof; Stage 2 flips
    /// the one engine to the breakthrough parity).
    ///
    /// The "floor" functions named below (`dmc_dma_step_impl`,
    /// `oam_dma_step`) were the per-engine drivers this engine replaced. They
    /// were deprecated at v2.7.5 and removed at v2.9.8 (ADR 0042); git history
    /// holds them.
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
            // A load refused by a write enters here regardless of the half.
            let defer_load =
                self.apu.dmc_dma_is_load() && dmc_noop_half && !self.dmc_load_write_delayed;
            if !defer_load {
                self.in_dmc_dma = true;
                self.dmc_halt = true;
                self.dmc_load_write_delayed = false;
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
                // A halted DMC shares this OAM-READ cycle: on `Rp2A03G` the held
                // CPU read's side-effect replay fires first (lockstep noop-body
                // order: `replay_dma_noop_read` THEN the OAM slot). This extra
                // parked-address re-read — a *halted* DMC squeezing a side-effect
                // into an OAM-owned read cycle — is v2.1.7's "unexpected DMA"
                // extra read, and it is revision-gated: `Rp2A03G` (default)
                // performs it, `Rp2A03H` OMITS it (opt-in later-die model —
                // unverified direction; see ADR 0033). Suppression is
                // deterministic and cannot desync the transfer:
                // `replay_dma_noop_read` only re-triggers a *register's*
                // side-effect (a `$2007` buffer advance / `$4016`-`$4017` shift /
                // `$4015` IRQ-clear); it ticks no time and advances no DMA
                // counter, so the OAM/DMC data path and cycle length are
                // identical on both arms.
                //
                // HONEST RESIDUAL (ADR 0033): on this ported engine the branch
                // FIRES (measured ~75× in a synthetic DMC+OAM+`$2007`-loop probe)
                // but `replay_dma_noop_read(halted_addr)` is a no-op every time,
                // because `halted_addr` during a DMC+OAM overlap is always the
                // post-`$4014` *instruction fetch* in PRG (OAM DMA drains on the
                // next opcode read, not on a register operand read), never a
                // `$2002/$2007/$4015/$4016/$4017` address. So `Rp2A03G` and
                // `Rp2A03H` are, in practice, byte-identical on every public
                // oracle and every constructible scenario — the die-revision
                // extra read is unobservable here, not merely unverified. The
                // gate is kept at its mechanism-correct location so it becomes
                // live immediately if the parked-address model ever exposes a
                // register during the overlap; it never perturbs the default
                // (`Rp2A03G`) path.
                if self.in_dmc_dma && self.cpu_2a03_revision.has_unexpected_dma_extra_read() {
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
    /// engine and DMC DMA fetches.  Time is advanced by the CPU's
    /// surrounding `start_cycle` / `end_cycle`.
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
                let mic = u8::from(self.famicom_mic) << 2;
                let base = (self.open_bus & 0xE0) | self.read_port(0) | mic;
                self.vs_overlay_4016(base)
            }
            0x4017 => {
                let base = (self.open_bus & 0xE0) | self.read_port(1);
                self.vs_overlay_4017(base)
            }
            0x4020..=0xFFFF => {
                if self.mapper.cpu_read_unmapped(addr) {
                    // Unmapped read: nothing drives the bus, so the CPU sees
                    // the floating-latch value (nesdev "Open bus behavior":
                    // "all it sees on its data inputs is whatever was left to
                    // float"). It falls through like the undecoded
                    // `$4000-$401F` arm above: `open_bus` is rewritten with the
                    // value it already holds, and -- the point -- the CPU's
                    // internal bus latches it, so a following `$4015` read's
                    // bit 5 comes from THIS cycle ("the last cycle that did
                    // not read $4015", nesdev APU). Until v2.9.2 this arm
                    // returned early and skipped that update, which is visible
                    // only after something has moved the external bus alone --
                    // a DMC DMA fetch, or an OAM-DMA put with the 6502 bus
                    // parked in `$4000-$401F` (core audit v2.9.2 AUD-03).
                    // v2.9.6: a board whose register latches on reads (GTROM)
                    // sees the value that floated.
                    self.mapper.notify_floating_read(addr, self.open_bus);
                    self.open_bus
                } else {
                    // The Game Genie physically substitutes the byte on the
                    // cartridge bus, so the (possibly substituted) value is
                    // what the CPU sees AND what latches onto `open_bus` below.
                    let raw = self.mapper.cpu_read(addr);
                    // Register-window reads may drive only some data bits; the
                    // rest keep the floating latch (v2.7.2, core audit §4.5).
                    // Limited to `$4020-$5FFF`, so PRG fetches pay nothing.
                    let raw = if addr < 0x6000 {
                        let driven = self.mapper.cpu_read_driven_mask(addr);
                        (self.open_bus & !driven) | (raw & driven)
                    } else {
                        raw
                    };
                    self.apply_genie(addr, raw)
                }
            }
        };
        self.last_read_addr = addr;
        self.open_bus = v;
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
        };
        self.ppu.cpu_read_register(reg, &mut adapter)
    }

    /// PPU register write with side effects.
    fn ppu_register_write(&mut self, addr: u16, value: u8) {
        // MMC5 decodes `$2000` / `$2001` itself (8x16 mode, render enables);
        // it sees the undecoded address, so a mirror write is not snooped.
        self.mapper.notify_ppu_register_write(addr, value);
        let reg = (addr & 7) as u8;
        let mut adapter = PpuBusAdapter {
            mapper: self.mapper.as_mut(),
            nt_override: self.nt_mirroring_override,
            // CPU bus access happens during φ2 → sub_dot 2 (M2-high).
            sub_dot: 2,
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
}

impl PpuBus for PpuBusAdapter<'_> {
    fn ppu_read(&mut self, addr: u16) -> u8 {
        self.mapper.ppu_read(addr & 0x1FFF)
    }
    fn ppu_read_sprite(&mut self, addr: u16) -> u8 {
        self.mapper.ppu_read_sprite(addr & 0x1FFF)
    }
    fn chr_phys(&self, addr: u16) -> Option<u32> {
        self.mapper.chr_phys(addr & 0x1FFF)
    }
    fn ppu_write(&mut self, addr: u16, value: u8) {
        self.mapper.ppu_write(addr & 0x1FFF, value);
    }
    fn nametable_unfolded(&self) -> bool {
        self.mapper.nametable_unfolded()
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
    }
    fn notify_scanline_start(&mut self) {
        self.mapper.notify_scanline_start();
    }
    fn notify_vblank(&mut self) {
        self.mapper.notify_vblank();
    }
    fn nametable_address(&self, addr: u16) -> u16 {
        resolve_nt_addr(self.nt_override, &*self.mapper, addr)
    }
}

/// Resolve a nametable address to a physical CIRAM offset, honouring the
/// per-game mirroring override when one is set.
///
/// Factored out of [`PpuBusAdapter::nametable_address`] (v2.3.2 "Lucid") so
/// [`SystemBus::resolve_nametable_address`] can answer the same question
/// without constructing an adapter. One definition, so the fetch path and the
/// provenance panel cannot drift apart on a board with an override.
fn resolve_nt_addr(
    nt_override: Option<rustynes_mappers::Mirroring>,
    mapper: &dyn Mapper,
    addr: u16,
) -> u16 {
    nt_override.map_or_else(
        || mapper.nametable_address(addr),
        |m| override_nt_addr(m, addr),
    )
}

impl SystemBus {
    /// Read-only nametable-address resolution for the pixel-provenance panel.
    ///
    /// Shares [`resolve_nt_addr`] with the PPU's own fetch path, so a board with
    /// a per-game mirroring override reports the offset its fetches really use.
    #[cfg(feature = "debug-hooks")]
    pub(crate) fn resolve_nametable_address(&self, addr: u16) -> u16 {
        resolve_nt_addr(self.nt_mirroring_override, &*self.mapper, addr)
    }
}

/// v2.0 master-clock R1 substrate helpers (Phase 1). Compiled only under
/// `mc-r1-substrate`; used by the clean `Bus` contract overrides below.
impl SystemBus {
    /// Tick the APU + frame counter once and fan frame events out to on-cart
    /// audio (the per-CPU-cycle APU advance `cpu_clock` runs at cycle start).
    ///
    /// v2.8.0 Phase 4 — the mapper dispatches are gated on the cached
    /// capability flags: boards without on-cart audio would return 0 from
    /// the default `mix_audio` (0.0 after the f32 conversion — identical),
    /// and boards without the frame hook have the default no-op. Skipping
    /// both saves two virtual calls + an f32 divide per CPU cycle.
    fn apu_advance_one(&mut self) {
        // `Mapper::mix_audio` returns i32 (widened from i16 in v2.2.3 so the
        // Sunsoft 5B's ~3.6x full-volume level is representable); scale it to
        // about the APU mixer's own [-0.5, 0.5] range. `as f32` rather than
        // `f32::from`: there is no lossless From<i32> for f32, and the cast
        // is exact for every value a board produces (|sample| well under
        // 2^24, where f32 is still integer-exact).
        #[allow(clippy::cast_precision_loss)]
        let mapper_sample = if self.mapper_caps.audio {
            self.mapper.mix_audio() as f32 / 65536.0
        } else {
            0.0
        };
        // v2.0.0 beta.1 (A1 one-clock collapse): hand the APU the canonical
        // bus cycle counter (incremented earlier in this same `cpu_clock`)
        // instead of letting it keep an independent `+= 1` mirror (the
        // one-clock collapse, promoted to the only path in v2.0.0 beta.4).
        self.apu.set_canonical_cycle(self.cycle);
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

impl Bus for SystemBus {
    fn cpu_read(&mut self, addr: u16) -> u8 {
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
            // tracker. `Cpu::read1` performs the access between
            // `start_cycle` and `end_cycle`, and `end_cycle` ends with
            // `trace_end_cycle`, which consumes the tracker into this cycle's
            // record.
            self.trace_bus_access = BusAccess::Read;
            self.trace_bus_addr = addr;
            self.trace_bus_data = value;
        }
        value
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
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
            REG_OAM_DMA => {
                // v2.3.2 "Lucid" — freeze THIS instruction (the `STA $4014`) as
                // the cause of the burst before it is armed. The 513/514 DMA
                // cycles are stolen from the instructions that follow, so by the
                // time the first OAM byte lands the live attribution context has
                // moved on to whichever instruction is being halted.
                #[cfg(feature = "debug-hooks")]
                self.ppu.latch_dma_attrib_context();
                // v2.3.7 "Overtone" — `$4014` sits inside the `$4000-$4017`
                // window the audio-provenance table reserves a slot for, but the
                // arm below routes only `$4000-$4013 | $4015 | $4017` to
                // `Apu::write_register`, where attribution is recorded. Record it
                // here so the reserved slot is actually populated; nothing is
                // dispatched to the APU, so the DMA behaviour is unchanged.
                #[cfg(feature = "debug-hooks")]
                self.apu
                    .record_bus_handled_register_write(REG_OAM_DMA, value);
                self.dma_pending = Some(value);
            }
            0x4000..=0x4013 | 0x4015 | 0x4017 => self.apu.write_register(addr, value),
            0x4016 => {
                // v2.3.7 "Overtone" — same as `$4014` above: inside the
                // provenance window, never routed to `Apu::write_register`, so
                // attribute it here. The strobe itself is still buffered and
                // committed by the code below; this only records the cause.
                #[cfg(feature = "debug-hooks")]
                self.apu.record_bus_handled_register_write(0x4016, value);
                // Session-24 / Phase 3 (Controller Strobing): the
                // controllers' OUT pins are only updated at the start
                // of M2-low (PUT) cycles.  Buffer the write and
                // commit at the next M2-low boundary inside
                // `cpu_clock` (`tick_one_cpu_cycle` until v2.9.8).  Mirrors Mesen2's
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
                // AFTER `cpu_clock` has incremented `self.cycle` to the
                // post-cycle value (`Cpu::start_cycle` calls `cpu_clock`
                // before the access; `tick_one_cpu_cycle` until v2.9.8).  The committed commit
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
                // v2.0.0 beta.5 (Vs. DualSystem): report the bit-1 (main/sub
                // comms signal) LEVEL on EVERY $4016 write for the wrapper
                // to poll. Deliberately not edge-filtered: the wrapper seeds
                // the reset-time levels itself (Mesen2's
                // `UpdateMainSubBit(main ? 0x00 : 0x02)`), so a bus-side
                // edge filter starting from a `false` latch would swallow a
                // genuine seeded-HIGH → written-LOW transition (Balloon
                // Fight's reset writes `$4016 = $00` on both consoles) and
                // deadlock the boot handshake. Applying an unchanged level
                // is idempotent in the wrapper. The latch is only consulted
                // by the DualSystem wrapper; single-console behavior is
                // untouched (two dead field writes on non-Vs carts, no
                // reads).
                self.vs_4016_bit1 = (value & 0x02) != 0;
                self.vs_4016_bit1_dirty = true;
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
        // A CPU read cycle ran, so any write-refused load either entered on
        // the DMA cycles before it (clearing the latch there) or was not
        // serviceable; the latch spans exactly one write-to-read boundary.
        self.dmc_load_write_delayed = false;
        self.cpu_read(addr)
    }

    fn write(&mut self, addr: u16, value: u8) {
        // RDY cannot halt a write. A pending load that would have entered on
        // this cycle (the get half: the access-point label is `!put_cycle`,
        // as in `unified_dma_cycle_impl`) is refused, and enters on the next
        // read without the get-half deferral (`dmc_load_write_delayed`).
        if self.apu.dmc_dma_pending()
            && self.apu.dmc_dma_is_load()
            && self.apu.dmc_dma_serviceable()
            && !self.in_dmc_dma
            && !self.apu.put_cycle()
        {
            self.dmc_load_write_delayed = true;
        }
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
    ///
    /// R1c-3 (v2.0.0's `mmc3-m2-phase-irq`, removed at v2.9.9; now
    /// `mmc3-a12-phase-probe` only): when the feature is enabled, `sub_dot` is seeded from the REAL M2-phase of this catch-up
    /// call (`0` = pre-access / M2-low, called from `Cpu::start_cycle`
    /// before the bus access; `2` = post-access / M2-high, called from
    /// `Cpu::end_cycle` after it) instead of always restarting at `0`. Prior
    /// to this experiment `sub_dot` was a call-LOCAL counter that reset to
    /// zero on every invocation of this function — since `run_ppu_to` is
    /// called twice per CPU cycle (once per half) and each half typically
    /// ticks at most one PPU dot, the value threaded to
    /// `Mapper::notify_a12_at_sub_dot` was almost always `0` regardless of
    /// which half of the cycle actually produced the A12 transition. That
    /// meant the M2-phase plumbing ADR-0002 describes ("sub-dot 0/1 is
    /// M2-low, 2 is M2-high") was never actually true on the live R1
    /// (non-DMA) scheduler path — only on the legacy `tick_one_cpu_cycle`
    /// DMA-burst path (removed at v2.9.8, ADR 0042), which genuinely walked
    /// all 3 dots of a cycle in one call with a persistent counter. This experiment closes that gap so
    /// MMC3's (default-off) M2-phase-aware IRQ-visibility pipeline can be
    /// evaluated against real phase data on the promoted core. See
    /// `docs/adr/0002-irq-timing-coordination.md` and
    /// `docs/audit/r1r2-per-dot-scheduler-attempt-2026-07-02.md`.
    ///
    /// When the feature is OFF this compiles to the exact prior
    /// call-local-counter behavior (`sub_dot` always starts at `0`) —
    /// byte-identical default build, per the project's additive/off-by-
    /// default convention.
    fn run_ppu_to(&mut self, target: u64, is_post_access: bool) {
        let ppu_div = u64::from(self.ppu_div_cached);
        // Seed the real M2-phase into `sub_dot` (0 = pre-access/M2-low catch-up,
        // 2 = post-access/M2-high catch-up) for the v2.1.5 F5.0
        // `mmc3-a12-phase-probe` observational tally (v2.0.0's
        // `mmc3-m2-phase-irq` deferral also read it; removed at v2.9.9).
        // The probe only counts, so the emulated timeline stays byte-identical
        // even with its feature on. See ADR 0002.
        #[cfg(feature = "mmc3-a12-phase-probe")]
        let mut sub_dot = if is_post_access { 2u8 } else { 0u8 };
        #[cfg(not(feature = "mmc3-a12-phase-probe"))]
        let (mut sub_dot, _) = (0u8, is_post_access);
        while self.ppu_clock + ppu_div <= target {
            let mut adapter = PpuBusAdapter {
                mapper: self.mapper.as_mut(),
                nt_override: self.nt_mirroring_override,
                sub_dot,
            };
            // No per-dot /NMI sampling here: the CPU reads the live level
            // through `nmi_level` and edge-detects it itself. The bus-side
            // edge detector that used to run on every dot fed only the
            // removed `poll_nmi` (ADR 0042, v2.9.8).
            self.ppu.tick(&mut adapter);
            self.ppu_clock += ppu_div;
            sub_dot = sub_dot.wrapping_add(1);
        }
    }

    /// R1: one CPU cycle of bus-side work (NO PPU advance — that lives in
    /// [`Bus::run_ppu_to`]). Controller strobe commit + cycle counter +
    /// per-cycle PPU/mapper hooks + APU tick. DMA is not run here: the CPU
    /// drives the unified engine through `unified_dma_cycle`, one full cycle
    /// at a time.
    fn cpu_clock(&mut self) {
        // Stamp the PPU with the cycle whose dots this call is about to run.
        // See `Ppu::set_trace_cpu_cycle`.
        //
        // This is the path a running console takes. The stamp once lived only
        // in the pre-v2.0.0 `tick_one_cpu_cycle` (removed at v2.9.8), which left
        // every record stamped `0` while the field, the column and the
        // plumbing all looked correct -- caught by
        // `tests/state_trace_records_carry_their_cpu_cycle.rs`, which exists
        // because a present-but-constant field reinstates the whole problem it
        // was added to solve while appearing to fix it.
        #[cfg(feature = "ppu-state-trace")]
        self.ppu.set_trace_cpu_cycle(self.cycle);

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

    fn irq_level(&self) -> bool {
        // Bound BEFORE the expression rather than as an inline `#[cfg]` block
        // inside it. The two forms compile identically -- the default build
        // still emits nothing named `inject_`, which is ADR 0038's structural
        // gate -- but a `cfg` block in the middle of a boolean chain is hard to
        // read, and this chain is the wire-OR of every /IRQ source.
        #[cfg(feature = "cosim-interrupt-inject")]
        let injected = self.inject_irq;
        #[cfg(not(feature = "cosim-interrupt-inject"))]
        let injected = false;

        // v2.8.0 Phase 4 — boards without an IRQ source have the default
        // `irq_pending() == false`; skip the per-cycle virtual call.
        // v2.0.0 beta.5 — `vs_external_irq` is the DualSystem partner
        // console's `$4016` bit-1 signal (always `false` on a single
        // console, so the default path is unchanged).
        (self.mapper_caps.irq_source && self.mapper.irq_pending())
            || self.apu.irq_line()
            || self.vs_external_irq
            // v2.5.1 (ADR 0038). Level-sensitive and OR'd, exactly like
            // `vs_external_irq` beside it -- which is the precedent: an external
            // IRQ source already joins the wire-OR here, and this is the same
            // shape with a different driver.
            || injected
    }

    fn nmi_level(&self) -> bool {
        // v2.5.1 (ADR 0038). Injected here, on the LEVEL: the production CPU
        // samples it every cycle and edge-detects it itself (`nmi_first_tick`
        // -> `pending_nmi` -> `armed_nmi`).
        //
        // The first implementation injected at `poll_nmi` (a dead hook,
        // removed at v2.9.8 with ADR 0042), which looked like the right
        // function and was never called on this path. The rung-2 sweep found it on
        // its first real run -- the DUT took the injected NMI and the oracle did
        // not -- which is exactly the defect class a co-simulation exists to
        // catch, arriving in the harness rather than in the RTL.
        //
        // A LEVEL, not a latch: the CPU does its own edge detection, so
        // consuming it here would make an injected NMI behave unlike a PPU one.
        #[cfg(feature = "cosim-interrupt-inject")]
        if self.inject_nmi {
            return true;
        }
        self.ppu.nmi_line()
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
                // A load refused by a write may not be deferred again.
                && !self.dmc_load_write_delayed
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

    /// Per-cycle trace push, the one `CycleRecord` producer since v2.9.8
    /// removed the legacy `tick_one_cpu_cycle` build. `irq_pending_apu_at_low` was
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
            // The per-sub-dot A12 capture lived in the pre-v2.0.0
            // `tick_one_cpu_cycle`, removed at v2.9.8 (ADR 0042); the R1 path
            // never fed it, so the column has been empty on every record
            // since v2.0.0 and stays in the schema as such.
            a12_events: alloc::vec::Vec::new(),
            dmc_dma_pending_pre: false,
            dmc_dma_pending_post: self.apu.dmc_dma_pending(),
            dmc_dma_short_post: self.apu.dmc_dma_short(),
            dmc_abort_pending_post: self.apu.dmc_abort_pending(),
            dmc_abort_delay_post: self.apu.dmc_abort_delay(),
            dmc_dma_cooldown_post: self.apu.dmc_dma_cooldown(),
            dmc_dma_delay_post: self.apu.dmc_dma_delay(),
            apu_phase_post: self.apu.apu_phase(),
            in_dmc_dma: self.in_dmc_dma,
            // No owed-cycle counter exists since the unified DMA engine
            // (its 513/514 length is emergent); the column is kept so the
            // trace CSV schema is unchanged.
            dma_cycles_owed: 0,
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
    /// construct a `SystemBus`; these tests never run the CPU.
    fn test_bus() -> SystemBus {
        let mut rom = Vec::with_capacity(16 + 0x4000 + 0x2000);
        rom.extend_from_slice(b"NES\x1A");
        rom.push(1); // 16 KiB PRG
        rom.push(1); // 8 KiB CHR
        rom.extend_from_slice(&[0u8; 10]);
        rom.extend_from_slice(&[0u8; 0x4000]);
        rom.extend_from_slice(&[0u8; 0x2000]);
        SystemBus::new(&rom).expect("synthetic NROM parses")
    }

    fn strobe(bus: &mut SystemBus) {
        bus.commit_controller_strobe(1);
        bus.commit_controller_strobe(0);
    }

    #[test]
    fn famicom_microphone_drives_4016_bit2() {
        let mut bus = test_bus();
        // Default: mic released -> $4016 bit 2 clear (byte-identical stock read).
        assert!(!bus.microphone());
        assert_eq!(bus.peek_cpu(0x4016) & 0x04, 0x00, "mic off -> D2 clear");
        // Press the mic: $4016 bit 2 reads 1.
        bus.set_microphone(true);
        assert!(bus.microphone());
        assert_eq!(bus.peek_cpu(0x4016) & 0x04, 0x04, "mic on -> D2 set");
        // $4017 is unaffected (the Famicom mic is a $4016-only signal).
        assert_eq!(bus.peek_cpu(0x4017) & 0x04, 0x00, "mic never touches $4017");
        // Release restores the stock read.
        bus.set_microphone(false);
        assert_eq!(
            bus.peek_cpu(0x4016) & 0x04,
            0x00,
            "mic released -> D2 clear"
        );
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
    fn a_short_bus_section_is_refused_at_every_length() {
        // v2.9.8 (BUS section version 2, ADR 0042). Version 1 decoded every
        // missing tail as its default, so a body cut short anywhere after the
        // first ~2 KiB loaded as an "older" layout. Version 2 has no older
        // layout to fall back to: the section version check refuses a v1
        // body before it reaches the decoder, so a short v2 body can only be
        // damage. Every cut is checked, not a representative, because the
        // failure this guards is a single trailing-default read left behind.
        let mut bus = test_bus();
        // Attach the largest device so the device decoder's own reads are in
        // the range the cuts walk through.
        bus.set_expansion_device(
            0,
            Some(crate::input_device::InputDevice::FamilyKeyboard(
                crate::input_device::FamilyKeyboardState::new(),
            )),
        );
        let blob = crate::bus_snapshot::encode_bus(&bus);
        crate::bus_snapshot::decode_bus(&mut test_bus(), &blob).expect("the whole body loads");
        for len in 0..blob.len() {
            assert!(
                crate::bus_snapshot::decode_bus(&mut test_bus(), &blob[..len]).is_err(),
                "a BUS body cut to {len} of {} bytes decoded cleanly",
                blob.len()
            );
        }
    }

    #[test]
    fn trailing_bytes_after_a_bus_section_are_refused() {
        // The other half of a fixed layout: bytes past the last field are not
        // a newer tail this build can ignore, because the section version is
        // what announces a newer layout.
        let bus = test_bus();
        let mut blob = crate::bus_snapshot::encode_bus(&bus);
        blob.push(0);
        assert!(matches!(
            crate::bus_snapshot::decode_bus(&mut test_bus(), &blob),
            Err(SnapshotError::SectionInvalid { .. })
        ));
    }

    #[test]
    fn an_unknown_expansion_device_tag_is_refused() {
        // Version 1 read an unknown tag as "no device" and carried on reading
        // the bytes behind it as the next field. Tag 0 is the empty port; the
        // first unassigned tag is 10.
        let bus = test_bus();
        let mut bad = crate::bus_snapshot::encode_bus(&bus);
        // The two device tags sit 25 bytes before the end with both ports
        // empty: mirroring override (1) + controller-run tail (22) +
        // internal bus (1) follow them, and port 1's tag is the second.
        let port0_tag = bad.len() - 1 - 22 - 1 - 2;
        assert_eq!(&bad[port0_tag..port0_tag + 2], &[0, 0]);
        bad[port0_tag] = 10;
        assert!(matches!(
            crate::bus_snapshot::decode_bus(&mut test_bus(), &bad),
            Err(SnapshotError::SectionInvalid { .. })
        ));
    }

    #[test]
    fn an_out_of_range_oam_dma_index_is_rejected() {
        // Found by the v2.7.0 `save_state` fuzz target: an active OAM DMA
        // restored at index >= 256 never completes and overflows the `u16`.
        // Legal: 0..=255 while active, and 256 once the transfer has ended.
        let decode = |active: bool, addr: u16| {
            let mut bus = test_bus();
            bus.uni_oam_active = active;
            bus.uni_oam_addr = addr;
            let blob = crate::bus_snapshot::encode_bus(&bus);
            crate::bus_snapshot::decode_bus(&mut test_bus(), &blob)
        };
        assert!(decode(true, 255).is_ok(), "the last in-flight index loads");
        assert!(
            decode(false, 256).is_ok(),
            "the completed-transfer index loads"
        );
        for (active, addr) in [(true, 256), (true, 257), (false, 257), (false, u16::MAX)] {
            assert!(
                matches!(
                    decode(active, addr),
                    Err(SnapshotError::SectionInvalid { .. })
                ),
                "active={active} addr={addr} must be rejected"
            );
        }
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

    /// With the beam-relative Zapper model on, a debugger peek of `$4017` must
    /// return the SAME light contribution the CPU read produces — at the
    /// pre-render line and at a visible line — and must not advance device
    /// state.
    ///
    /// Regression pin for the `peek_port` parity fix: before it, `peek_port`
    /// fell through to the overlay's frame-granular `peek()` and could report a
    /// different light bit than `read_port` at the same instant. (This is the
    /// real defect the fix addressed; the separate `read_before_visible`
    /// conversion fallback is defensive, since `scanline()` is non-negative on
    /// every current region — pre-render is line 261 NTSC / 311 PAL, not -1.)
    #[test]
    fn temporal_zapper_debugger_peek_matches_cpu_read() {
        use crate::input_device::{InputDevice, ZapperState};

        // $4017 bit 3 is the (inverted) light bit; the open-bus upper bits differ
        // between the read and peek paths, so compare only the device bit.
        const LIGHT: u8 = 0b0000_1000;

        // The two models are constructed to DISAGREE, so the test fails if
        // `peek_port` does not mirror `read_port`'s temporal branch:
        //   * frame model (`peek()` -> `ZapperState::read()`) reads `light_seen`,
        //     which we force TRUE via `from_parts` -> reports light;
        //   * temporal model (`read_at_scanline`) reads the current scanline. A
        //     fresh bus sits on the pre-render line (261 NTSC), past the
        //     photodiode hold window -> reports NO light.
        // So a peek that (wrongly) fell through to the frame `peek()` would
        // return light while the CPU read returns none. No framebuffer or
        // scanline poke is needed — the injected `light_seen` supplies the
        // divergence, and the default dark framebuffer keeps the temporal path
        // at no-light on every line anyway.
        let mut bus = test_bus();
        // from_parts(x, y, trigger, light_seen): trigger + light_seen both true.
        let zapper = ZapperState::from_parts(128, 12, true, true);
        bus.set_expansion_device(1, Some(InputDevice::Zapper(zapper)));
        bus.set_zapper_temporal_light(true);

        assert!(
            bus.ppu.scanline() > 239,
            "fresh PPU is on the pre-render line"
        );
        let cpu = bus.read_port(1) & LIGHT;
        let peek = bus.peek_port(1) & LIGHT;
        assert_eq!(cpu, LIGHT, "temporal read at pre-render reports NO light");
        assert_eq!(
            peek, cpu,
            "debugger peek must match the CPU read, not the frame `peek()` \
             (which would report light from the injected light_seen)",
        );

        // The peek must be side-effect-free: repeating it does not change the
        // answer (guards a regression where a peek routes through mutating state).
        assert_eq!(bus.peek_port(1) & LIGHT, peek);
        assert_eq!(bus.peek_port(1) & LIGHT, peek);
    }

    #[test]
    fn a_contiguous_four_score_read_does_not_advance_the_chain() {
        // The adapter is one shift chain with the pads it multiplexes, so a
        // contiguous read -- `CLK` staying low across consecutive-cycle reads
        // of the same port -- must return the SAME bit from the SAME position,
        // exactly as a bare controller does.
        //
        // Before this guard the chain advanced on every read while the pads
        // advanced only on a rising edge, so it ran ahead of them: reaching the
        // pad-3 window after seven advances of pad 1 rather than eight, and
        // consuming two signature bits where the hardware returns one twice.
        let mut bus = test_bus();
        bus.set_four_score(true);
        bus.write(0x4016, 1);
        bus.write(0x4016, 0);

        // Walk the whole 24-read sequence. At each position, a read on the very
        // next CPU cycle must repeat it, and must leave the chain where it was.
        for step in 0..24u8 {
            let first = bus.read_port(0);
            // Where the run's OWN rising edge left the chain. The contiguous
            // read must not move it from here — comparing against the position
            // before the first read would instead assert the first read does
            // not advance, which is a different (and wrong) claim.
            let idx_in_run = bus.four_score_idx[0];
            bus.cycle = bus.cycle.wrapping_add(1);
            let contiguous = bus.read_port(0);
            assert_eq!(
                first, contiguous,
                "step {step}: a contiguous read returned a different bit"
            );
            assert_eq!(
                bus.four_score_idx[0], idx_in_run,
                "step {step}: the chain advanced during a contiguous read"
            );
            // Break the run so the next iteration starts a fresh one.
            bus.cycle = bus.cycle.wrapping_add(4);
        }
    }

    #[test]
    fn the_four_score_owed_edge_survives_a_save_state() {
        // `four_score_pending` is the adapter's half of the same state
        // `pending_shift` is for the pads. Restoring one without the other puts
        // the two halves of one shift chain on different positions.
        let mut bus = test_bus();
        bus.set_four_score(true);
        bus.write(0x4016, 1);
        bus.write(0x4016, 0);
        bus.read_port(0);
        assert_eq!(bus.four_score_pending(), [true, false]);

        let blob = crate::bus_snapshot::encode_bus(&bus);
        let mut restored = test_bus();
        restored.set_four_score(true);
        crate::bus_snapshot::decode_bus(&mut restored, &blob).unwrap();
        assert_eq!(
            restored.four_score_pending(),
            [true, false],
            "the adapter resumed without the edge it owed"
        );
    }

    /// Core audit v2.9.2 AUD-03. A CPU read of an address nothing decodes
    /// (`$5000` on NROM) returns the floating bus value, and the CPU latches
    /// that value like any other read: `nesdev_wiki/Open_bus_behavior.xhtml`
    /// ("when the CPU reads an address that no circuit decodes, all it sees on
    /// its data inputs is whatever was left to float on the data bus"). The
    /// `$4015` read's bit 5 then comes from it: `nesdev_wiki/APU.xhtml`
    /// ("Bit 5 is open bus ... the open bus value comes from the last cycle
    /// that did not read `$4015`").
    ///
    /// The two latches differ only after something drives the external bus
    /// alone: a DMC DMA fetch (`AccuracyCoin` `Internal Data Bus` Test 2), or
    /// an OAM-DMA put with the 6502 bus parked in `$4000-$401F`. So: a CPU
    /// read leaves both at `$00`, a DMC fetch floats `$20` onto the external
    /// bus, the CPU reads the undecoded `$5000` (and sees `$20`), then reads
    /// `$4015`. The last non-`$4015` cycle carried `$20`, so bit 5 is set.
    /// Before the fix the unmapped arm returned early and skipped the
    /// internal-bus update, so bit 5 still came from the `$00` read before
    /// the DMC fetch -- unlike the `$4000-$401F` undecoded arm, which always
    /// updated it.
    #[test]
    fn an_unmapped_cartridge_read_latches_the_floating_value_onto_the_internal_bus() {
        let mut rom = Vec::with_capacity(16 + 0x4000 + 0x2000);
        rom.extend_from_slice(b"NES\x1A");
        rom.push(1); // 16 KiB PRG
        rom.push(1); // 8 KiB CHR
        rom.extend_from_slice(&[0u8; 10]);
        let mut prg = [0u8; 0x4000];
        prg[0] = 0x20; // $C000 (and $8000): the DMC sample byte
        rom.extend_from_slice(&prg);
        rom.extend_from_slice(&[0u8; 0x2000]);
        let mut bus = SystemBus::new(&rom).expect("synthetic NROM parses");
        assert!(
            bus.mapper.cpu_read_unmapped(0x5000),
            "fixture: $5000 floats"
        );

        bus.ram[0] = 0x00;
        assert_eq!(bus.raw_cpu_read(0x0000), 0x00);
        // A DMC DMA sample fetch, as `dmc_dma_step_impl` performs it.
        bus.in_dmc_dma = true;
        assert_eq!(bus.dmc_dma_read(0xC000, 0x8000), 0x20);
        bus.in_dmc_dma = false;
        assert_eq!(bus.open_bus, 0x20, "the DMC fetch drove the external bus");
        assert_eq!(bus.internal_data_bus, 0x00, "but not the internal one");

        assert_eq!(bus.raw_cpu_read(0x5000), 0x20, "the undecoded read floats");
        assert_eq!(
            bus.internal_data_bus, 0x20,
            "the CPU latched the floating value it read"
        );
        assert_eq!(
            bus.raw_cpu_read(0x4015) & 0x20,
            0x20,
            "$4015 bit 5 comes from the last non-$4015 cycle: the $5000 read"
        );
    }

    #[test]
    fn internal_data_bus_round_trips_through_save_state() {
        // v2.8.0 (libretro audit §2.4). The 2A03's internal data bus is a
        // separate latch from the external open bus: a DMC DMA fetch drives
        // the external bus only, so across a DMC halt the two differ, and a
        // `$4015` read returns bit 5 from the INTERNAL one. It was not in the
        // BUS section, so a restore left whatever the running machine held --
        // a value from a discarded timeline under run-ahead and rollback.
        // Found by the widened `snapshot_schema_audit`, which now covers the
        // bus. The two latches are set to different values here so a decoder
        // that restored one from the other would fail.
        let mut bus = test_bus();
        bus.open_bus = 0x00;
        bus.internal_data_bus = 0x20;
        let blob = crate::bus_snapshot::encode_bus(&bus);
        let mut restored = test_bus();
        restored.internal_data_bus = 0xFF;
        crate::bus_snapshot::decode_bus(&mut restored, &blob).unwrap();
        assert_eq!(restored.internal_data_bus, 0x20);
        assert_eq!(restored.open_bus, 0x00);
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

#[cfg(test)]
mod partial_drive_tests {
    use super::*;

    /// Sachen SA-020A (mapper 150): the `$4101` data register drives D2-D0
    /// only (`nesdev_wiki/INES_Mapper_150.xhtml`), so D7-D3 keep whatever
    /// was floating on the bus. Core audit section 4.5 found them read as 0,
    /// which the bus then latched.
    #[test]
    fn a_sachen_register_read_keeps_the_floating_high_bits() {
        let mut rom = Vec::with_capacity(16 + 0x8000 + 0x2000);
        rom.extend_from_slice(b"NES\x1A");
        rom.push(2); // 32 KiB PRG
        rom.push(1); // 8 KiB CHR
        rom.push(0x60); // mapper 150: low nibble 6
        rom.push(0x90); // high nibble 9
        rom.extend_from_slice(&[0u8; 8]);
        rom.resize(16 + 0x8000 + 0x2000, 0);
        let mut bus = SystemBus::new(&rom).expect("mapper 150 parses");
        bus.mapper.cpu_write(0x4100, 0x05); // select register 5
        bus.mapper.cpu_write(0x4101, 0x03); // R5 = 3
        bus.open_bus = 0xA8; // the last value driven on the bus
        let v = bus.raw_cpu_read(0x4101);
        assert_eq!(v & 0x07, 0x03, "the driven bits are the register");
        assert_eq!(v & 0xF8, 0xA8, "the undriven bits are the floating latch");
        assert_eq!(bus.open_bus, v, "and the latch keeps them");
    }
}

#[cfg(test)]
mod n163_nametable_tests {
    use super::*;

    /// Namco 163 (mapper 19) with 32 KiB PRG and 8 KiB CHR-ROM whose 1 KiB
    /// page `p` is filled with `0x40 + p`. The CPU spins on `JMP $E000` with
    /// rendering off; two frames take the PPU past its post-power-on window,
    /// in which `$2006` writes are ignored.
    fn n163() -> crate::Nes {
        let mut rom = Vec::with_capacity(16 + 0x8000 + 0x2000);
        rom.extend_from_slice(b"NES\x1A");
        rom.push(2); // 32 KiB PRG
        rom.push(1); // 8 KiB CHR-ROM
        rom.push(0x31); // mapper 19 low nibble 3, vertical
        rom.push(0x10); // high nibble 1
        rom.extend_from_slice(&[0u8; 8]);
        let mut prg = alloc::vec![0u8; 0x8000];
        // The last 8 KiB is fixed at $E000: `JMP $E000`, vectors -> $E000.
        prg[0x6000..0x6003].copy_from_slice(&[0x4C, 0x00, 0xE0]);
        prg[0x7FFA..0x8000].copy_from_slice(&[0x00, 0xE0, 0x00, 0xE0, 0x00, 0xE0]);
        rom.extend_from_slice(&prg);
        for page in 0..8u8 {
            rom.extend(core::iter::repeat_n(0x40 + page, 0x400));
        }
        let mut nes = crate::Nes::from_rom(&rom).expect("mapper 19 parses");
        nes.run_frame();
        nes.run_frame();
        nes
    }

    /// A `$2007` write, through the PPU's own register path.
    fn poke(nes: &mut crate::Nes, addr: u16, value: u8) {
        let bus = nes.bus_mut();
        let [hi, lo] = addr.to_be_bytes();
        bus.cpu_write(0x2006, hi);
        bus.cpu_write(0x2006, lo);
        bus.cpu_write(0x2007, value);
    }

    // The PPU reaches nametables only through `nametable_fetch` /
    // `nametable_write` / `nametable_address`. v2.7.2's first cut implemented
    // N163's nametable select in `ppu_read`/`ppu_write`, which the PPU never
    // calls for `$2000-$3EFF`, and its unit tests called `ppu_read` directly,
    // so none of this was reachable in the emulator (PR #550 review).

    #[test]
    fn a_chr_rom_nametable_page_is_fetched_and_read_only() {
        let mut nes = n163();
        nes.bus_mut().mapper.cpu_write(0xC000, 0x03); // quadrant 0 -> CHR-ROM page 3
        assert_eq!(nes.bus_mut().debug_peek_ppu(0x2005), 0x43);
        poke(&mut nes, 0x2005, 0x99);
        assert_eq!(
            nes.bus_mut().debug_peek_ppu(0x2005),
            0x43,
            "CHR-ROM is read-only"
        );
    }

    #[test]
    fn the_nametable_registers_pick_the_ciram_page() {
        let mut nes = n163();
        nes.bus_mut().mapper.cpu_write(0xC000, 0xE1); // quadrant 0 -> CIRAM B
        nes.bus_mut().mapper.cpu_write(0xC800, 0xE1); // quadrant 1 -> CIRAM B
        poke(&mut nes, 0x2010, 0x77);
        assert_eq!(
            nes.bus_mut().debug_peek_ppu(0x2410),
            0x77,
            "both quadrants are page B"
        );
    }

    #[test]
    fn ciram_mapped_as_chr_sees_nametable_writes() {
        let mut nes = n163();
        nes.bus_mut().mapper.cpu_write(0xE800, 0x00); // CIRAM-as-CHR allowed in both halves
        nes.bus_mut().mapper.cpu_write(0x8000, 0xE0); // pattern $0000-$03FF -> CIRAM A
        nes.bus_mut().mapper.cpu_write(0xC000, 0xE0); // quadrant 0 -> CIRAM A
        poke(&mut nes, 0x2005, 0x5C);
        assert_eq!(
            nes.bus_mut().debug_peek_ppu(0x0005),
            0x5C,
            "one RAM, two windows"
        );
        poke(&mut nes, 0x0006, 0xA7);
        assert_eq!(
            nes.bus_mut().debug_peek_ppu(0x2006),
            0xA7,
            "and the other way"
        );
    }
}

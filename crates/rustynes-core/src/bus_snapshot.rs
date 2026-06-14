//! Save-state encoding for the [`crate::bus::LockstepBus`] (the "BUS"
//! tagged section) — owns CPU RAM, controllers, OAM/DMC DMA bookkeeping,
//! NMI edge latches, open-bus, and the cumulative cycle counter.
//!
//! The chip sub-states (CPU / PPU / APU / mapper) are emitted as their own
//! tagged sections by [`crate::bus::LockstepBus::snapshot`].

use crate::bus::LockstepBus;
use crate::controller::Controller;
use crate::input_device::{InputDevice, VausState, ZapperState};
use crate::save_state::{BinReader, BinWriter, SnapshotError};
use alloc::format;
use alloc::vec::Vec;

/// Schema version for the BUS section payload.
pub const BUS_SECTION_VERSION: u8 = 1;

/// Encode the bus's own state (RAM, controllers, DMA, edge latches, cycle).
pub fn encode_bus(bus: &LockstepBus) -> Vec<u8> {
    let mut w = BinWriter::with_capacity(0x900);
    // Cumulative cycle counter.
    w.u64(bus.cycle());
    // CPU RAM (2 KiB).
    w.bytes(bus.ram_bytes());
    // Controllers.
    for c in bus.controllers_ref() {
        encode_controller(&mut w, *c);
    }
    // DMA + edge latch state.
    let s = bus.bus_misc_state();
    w.u8(s.dma_pending.unwrap_or(0));
    w.u8(u8::from(s.dma_pending.is_some()));
    w.u32(s.dma_cycles_owed);
    w.u8(s.dma_byte);
    w.u16(s.dma_idx);
    w.u8(s.dma_page);
    w.u8(u8::from(s.last_nmi_level));
    w.u8(u8::from(s.nmi_edge_latch));
    w.u8(s.open_bus);
    w.u16(s.last_read_addr);
    w.u8(u8::from(s.in_dmc_dma));
    w.u16(s.dma_halt_addr);
    w.u16(s.deferred_dma_replay_addr);
    // Session-24 / Phase 3 (Controller Strobing) deferred-write buffer.
    // Appended at the tail so v1 blobs without these bytes still decode
    // via the trailing-default-zero pattern used by `dma_halt_addr` /
    // `deferred_dma_replay_addr` above.
    w.u8(s.controller_write_pending);
    w.u8(s.controller_write_value);
    // v1.7.0 Four Score state, appended at the tail (same trailing-default
    // pattern as the bytes above): pre-v1.7.0 blobs lack these and decode
    // with the adapter off, so old saves still load.
    w.u8(u8::from(s.four_score));
    for c in bus.controllers34_ref() {
        encode_controller(&mut w, *c);
    }
    w.u8(s.four_score_idx[0]);
    w.u8(s.four_score_idx[1]);
    w.u8(s.four_score_sig[0]);
    w.u8(s.four_score_sig[1]);
    // W3-Stage-4 (2026-06-10), appended at the tail (same trailing-default
    // pattern): the DMC halt latch + the unified DMA engine's OAM state
    // (`mc-r1-dma-unified`; zeros when the feature is off so the layout is
    // identical across feature builds). Pre-Stage-4 blobs lack these bytes
    // and decode to the inactive defaults — exactly the state the old
    // clear-on-restore imposed.
    w.u8(u8::from(s.dmc_halt));
    w.u8(u8::from(s.uni_oam_active));
    w.u8(u8::from(s.uni_oam_halt));
    w.u8(u8::from(s.uni_oam_aligned));
    w.u16(s.uni_oam_addr);
    // The R1 substrate's bus-side master-clock pair: `ppu_clock` (PPU
    // progress in master-clock units) + `dma_mc_consumed` (master clocks
    // consumed by bus-side DMA cycles not yet folded into the CPU). These
    // MUST travel with `Cpu::master_clock` (CPU section v2) -- restoring one
    // side without the other desynchronizes `run_ppu_to` and spins the PPU
    // until the pair re-coheres.
    w.u64(s.ppu_clock);
    w.u64(s.dma_mc_consumed);
    // v2.1.0 non-standard input devices, appended at the tail (same
    // trailing-default pattern): a tag byte per port (0 = None, 1 = Zapper,
    // 2 = Vaus) followed by that device's fields. Pre-v2.1.0 blobs lack these
    // bytes and decode with both ports unplugged (`None`) — exactly the
    // default. Devices are NOT part of the determinism-critical no-device path.
    for port in 0..2 {
        encode_expansion_device(&mut w, bus.expansion_device(port).as_ref());
    }
    // v1.1.0 beta.1 (T-110-B4) — per-game nametable mirroring override (trailing
    // field; pre-v1.1.0 blobs lack it and decode as `None` = no override).
    w.u8(encode_mirroring_override(bus.mirroring_override()));
    w.into_vec()
}

/// Encode the optional mirroring override as a tag byte (0 = none).
const fn encode_mirroring_override(m: Option<rustynes_mappers::Mirroring>) -> u8 {
    use rustynes_mappers::Mirroring;
    match m {
        None => 0,
        Some(Mirroring::Horizontal) => 1,
        Some(Mirroring::Vertical) => 2,
        Some(Mirroring::SingleScreenA) => 3,
        Some(Mirroring::SingleScreenB) => 4,
        Some(Mirroring::FourScreen) => 5,
        Some(Mirroring::MapperControlled) => 6,
    }
}

/// Decode a mirroring-override tag byte (inverse of [`encode_mirroring_override`]).
const fn decode_mirroring_override(tag: u8) -> Option<rustynes_mappers::Mirroring> {
    use rustynes_mappers::Mirroring;
    match tag {
        1 => Some(Mirroring::Horizontal),
        2 => Some(Mirroring::Vertical),
        3 => Some(Mirroring::SingleScreenA),
        4 => Some(Mirroring::SingleScreenB),
        5 => Some(Mirroring::FourScreen),
        6 => Some(Mirroring::MapperControlled),
        _ => None,
    }
}

/// Encode one port's optional overlay device (tag byte + fields).
fn encode_expansion_device(w: &mut BinWriter, device: Option<&InputDevice>) {
    match device {
        None => w.u8(0),
        Some(InputDevice::Zapper(z)) => {
            w.u8(1);
            w.u16(z.x_raw());
            w.u16(z.y_raw());
            w.bool(z.trigger_raw());
            w.bool(z.light_seen_raw());
        }
        Some(InputDevice::Vaus(v)) => {
            w.u8(2);
            w.u8(v.position_raw());
            w.bool(v.fire_raw());
            w.u8(v.shift_raw());
            w.bool(v.strobe_raw());
        }
        Some(InputDevice::PowerPad(p)) => {
            w.u8(3);
            w.u16(p.buttons_raw());
            w.u8(p.shift_l_raw());
            w.u8(p.shift_h_raw());
            w.bool(p.strobe_raw());
        }
    }
}

/// Decode one port's optional overlay device (trailing-default `None`).
fn decode_expansion_device(r: &mut BinReader<'_>) -> Result<Option<InputDevice>, SnapshotError> {
    if r.remaining() < 1 {
        return Ok(None);
    }
    Ok(match r.u8()? {
        1 => {
            let x = r.u16()?;
            let y = r.u16()?;
            let trigger = r.bool()?;
            let light_seen = r.bool()?;
            Some(InputDevice::Zapper(ZapperState::from_parts(
                x, y, trigger, light_seen,
            )))
        }
        2 => {
            let position = r.u8()?;
            let fire = r.bool()?;
            let shift = r.u8()?;
            let strobe = r.bool()?;
            Some(InputDevice::Vaus(VausState::from_parts(
                position, fire, shift, strobe,
            )))
        }
        3 => {
            let buttons = r.u16()?;
            let shift_l = r.u8()?;
            let shift_h = r.u8()?;
            let strobe = r.bool()?;
            Some(InputDevice::PowerPad(
                crate::input_device::PowerPadState::from_parts(buttons, shift_l, shift_h, strobe),
            ))
        }
        // 0 (None) or any unknown tag => no device.
        _ => None,
    })
}

/// Apply a previously [`encode_bus`]-emitted blob.
///
/// # Errors
///
/// Returns [`SnapshotError`] for malformed inputs.
// The body is one straight-line field-by-field decode mirroring `encode_bus`
// (trailing-default reads dominate the count); splitting it would obscure the
// byte-order correspondence between the two functions.
#[allow(clippy::too_many_lines)]
pub fn decode_bus(bus: &mut LockstepBus, data: &[u8]) -> Result<(), SnapshotError> {
    let mut r = BinReader::new(data);
    let cycle = r.u64()?;
    let ram = r.take(0x800)?;
    bus.set_cycle(cycle);
    bus.set_ram_bytes(ram)?;
    let mut controllers = [Controller::new(); 2];
    for c in &mut controllers {
        decode_controller(&mut r, c)?;
    }
    bus.set_controllers(controllers);

    let dma_byte_value = r.u8()?;
    let dma_present = r.u8()?;
    let dma_pending = match dma_present {
        0 => None,
        1 => Some(dma_byte_value),
        other => {
            return Err(SnapshotError::SectionInvalid {
                tag: "BUS ".into(),
                reason: format!("invalid dma-pending presence {other}"),
            });
        }
    };
    let dma_cycles_owed = r.u32()?;
    let dma_byte = r.u8()?;
    let dma_idx = r.u16()?;
    let dma_page = r.u8()?;
    let last_nmi_level = r.u8()? != 0;
    let nmi_edge_latch = r.u8()? != 0;
    let open_bus = r.u8()?;
    let last_read_addr = r.u16()?;
    let in_dmc_dma = r.u8()? != 0;
    let dma_halt_addr = if r.remaining() >= 2 { r.u16()? } else { 0 };
    let deferred_dma_replay_addr = if r.remaining() >= 2 { r.u16()? } else { 0 };
    // Session-24 / Phase 3: trailing controller-strobe deferred-write
    // bytes.  Default zero so v1 blobs without these bytes still load.
    let controller_write_pending = if r.remaining() >= 1 { r.u8()? } else { 0 };
    let controller_write_value = if r.remaining() >= 1 { r.u8()? } else { 0 };
    // v1.7.0 Four Score state (trailing-default: pre-v1.7.0 blobs decode off).
    let four_score = if r.remaining() >= 1 {
        r.u8()? != 0
    } else {
        false
    };
    let mut controllers34 = [Controller::new(); 2];
    if r.remaining() >= 6 {
        for c in &mut controllers34 {
            decode_controller(&mut r, c)?;
        }
    }
    bus.set_controllers34(controllers34);
    let four_score_idx = [
        if r.remaining() >= 1 { r.u8()? } else { 0 },
        if r.remaining() >= 1 { r.u8()? } else { 0 },
    ];
    let four_score_sig = [
        if r.remaining() >= 1 { r.u8()? } else { 0 },
        if r.remaining() >= 1 { r.u8()? } else { 0 },
    ];
    // W3-Stage-4 trailing bytes (default-zero for pre-Stage-4 blobs).
    let dmc_halt = if r.remaining() >= 1 {
        r.u8()? != 0
    } else {
        false
    };
    let uni_oam_active = if r.remaining() >= 1 {
        r.u8()? != 0
    } else {
        false
    };
    let uni_oam_halt = if r.remaining() >= 1 {
        r.u8()? != 0
    } else {
        false
    };
    let uni_oam_aligned = if r.remaining() >= 1 {
        r.u8()? != 0
    } else {
        false
    };
    let uni_oam_addr = if r.remaining() >= 2 { r.u16()? } else { 0 };
    let ppu_clock = if r.remaining() >= 8 { r.u64()? } else { 0 };
    let dma_mc_consumed = if r.remaining() >= 8 { r.u64()? } else { 0 };
    // v2.1.0 non-standard input devices (trailing-default `None`).
    let device0 = decode_expansion_device(&mut r)?;
    let device1 = decode_expansion_device(&mut r)?;
    bus.set_expansion_device(0, device0);
    bus.set_expansion_device(1, device1);
    // v1.1.0 beta.1 (T-110-B4) — per-game mirroring override (trailing-default:
    // pre-v1.1.0 blobs have no byte left and decode as `None`).
    let mirroring_override = if r.remaining() >= 1 {
        decode_mirroring_override(r.u8()?)
    } else {
        None
    };
    bus.set_mirroring_override(mirroring_override);
    bus.set_bus_misc_state(BusMiscState {
        dma_pending,
        dma_cycles_owed,
        dma_byte,
        dma_idx,
        dma_page,
        dma_halt_addr,
        deferred_dma_replay_addr,
        last_nmi_level,
        nmi_edge_latch,
        open_bus,
        last_read_addr,
        in_dmc_dma,
        controller_write_pending,
        controller_write_value,
        four_score,
        four_score_idx,
        four_score_sig,
        dmc_halt,
        uni_oam_active,
        uni_oam_halt,
        uni_oam_aligned,
        uni_oam_addr,
        ppu_clock,
        dma_mc_consumed,
    });
    Ok(())
}

fn encode_controller(w: &mut BinWriter, c: Controller) {
    w.u8(c.buttons.bits());
    w.u8(c.shift);
    w.bool(c.strobe);
}
fn decode_controller(r: &mut BinReader<'_>, c: &mut Controller) -> Result<(), SnapshotError> {
    let bits = r.u8()?;
    c.buttons = crate::controller::Buttons::from_bits_truncate(bits);
    c.shift = r.u8()?;
    c.strobe = r.bool()?;
    Ok(())
}

/// Bus-side bookkeeping fields not owned by the chips. Exists as a small
/// struct so [`encode_bus`] / [`decode_bus`] can ferry them across the
/// crate-private boundary without exposing the bus's many private fields
/// individually.
#[derive(Debug, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)] // independent state words, not a FSM
pub struct BusMiscState {
    /// Source page of a deferred OAM DMA (consumed at the next CPU access).
    pub dma_pending: Option<u8>,
    /// Cycles still owed to the OAM DMA.
    pub dma_cycles_owed: u32,
    /// Scratch byte for the OAM DMA's read/write pair.
    pub dma_byte: u8,
    /// Progress index into the 256-byte OAM DMA window.
    pub dma_idx: u16,
    /// Active OAM DMA page.
    pub dma_page: u8,
    /// CPU read address repeated while OAM DMA has the CPU halted.
    pub dma_halt_addr: u16,
    /// Deferred DMC readout side-effect target for absolute register reads.
    pub deferred_dma_replay_addr: u16,
    /// Last-observed PPU NMI level.
    pub last_nmi_level: bool,
    /// Latched NMI edge.
    pub nmi_edge_latch: bool,
    /// Open-bus latch.
    pub open_bus: u8,
    /// Most recent CPU read address (for the DMC-DMA readout-bug emulation).
    pub last_read_addr: u16,
    /// `true` while servicing a DMC DMA fetch.
    pub in_dmc_dma: bool,
    /// Session-24 / Phase 3 (Controller Strobing) deferred-write
    /// pending counter (CPU cycles until commit; 0 means no pending
    /// write).
    pub controller_write_pending: u8,
    /// Latched controller-write value waiting for the next M2-low
    /// commit cycle.
    pub controller_write_value: u8,
    /// Whether the Four Score 4-player adapter is enabled (v1.7.0).
    pub four_score: bool,
    /// Per-port Four Score read counter.
    pub four_score_idx: [u8; 2],
    /// Per-port Four Score signature shift register.
    pub four_score_sig: [u8; 2],
    /// W3-Stage-4 (2026-06-10): the DMC-DMA halt latch (a DMC DMA is
    /// pending/halted and waiting for its GET slot).
    pub dmc_halt: bool,
    /// W3-Stage-4: unified DMA engine (`mc-r1-dma-unified`) — OAM DMA active
    /// (`TriCNES` `DoOAMDMA`). Always `false` when the feature is off.
    pub uni_oam_active: bool,
    /// W3-Stage-4: unified DMA engine — `TriCNES` `OAMDMA_Halt`.
    pub uni_oam_halt: bool,
    /// W3-Stage-4: unified DMA engine — `TriCNES` `OAMDMA_Aligned`.
    pub uni_oam_aligned: bool,
    /// W3-Stage-4: unified DMA engine — `TriCNES` `DMAAddress` (the OAM
    /// byte index).
    pub uni_oam_addr: u16,
    /// W3-Stage-4: R1 substrate PPU progress in master-clock units (the
    /// `run_ppu_to` cursor). Paired with `Cpu::master_clock` (CPU section
    /// v2); always 0 when `mc-r1-substrate` is off.
    pub ppu_clock: u64,
    /// W3-Stage-4: R1 substrate master clocks consumed by bus-side DMA
    /// cycles not yet folded into `Cpu::master_clock`.
    pub dma_mc_consumed: u64,
}

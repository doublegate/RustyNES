//! Save-state encoding for the [`crate::bus::SystemBus`] (the "BUS"
//! tagged section) — owns CPU RAM, controllers, the unified DMA engine's
//! bookkeeping, the two data-bus latches, and the cumulative cycle counter.
//!
//! The chip sub-states (CPU / PPU / APU / mapper) are emitted as their own
//! tagged sections by [`crate::bus::SystemBus::snapshot`].
//!
//! # Version 2 (v2.9.8, ADR 0042)
//!
//! Version 1 grew by appending fields to the tail and decoding each
//! missing tail as its default, so a blob from any release since v0.9 still
//! loaded. Version 2 drops that: every field is required, a short body is a
//! truncation error, and trailing bytes are rejected, because a version-2
//! reader is never handed anything but a version-2 body (the section version
//! check in `SystemBus::restore` refuses version 1 first). It also drops
//! the fields that no longer carry state: the NMI edge detector's
//! `last_nmi_level` / `nmi_edge_latch` (they fed only the removed `poll_nmi`),
//! the OAM-DMA owed-cycle counter and byte index (the unified engine's
//! length is emergent), and `dma_mc_consumed` (structurally zero since
//! v2.0.0, and decoded as zero regardless since v2.7.0).

use crate::bus::SystemBus;
use crate::controller::Controller;
use crate::input_device::{
    FamilyKeyboardState, InputDevice, SnesMouseState, VausState, ZapperState,
};
use crate::save_state::{BinReader, BinWriter, SnapshotError};
use alloc::format;
use alloc::vec::Vec;

/// Schema version for the BUS section payload.
///
/// 2 since v2.9.8 (ADR 0042): see the module docs for what changed. A
/// version-1 section is refused with [`SnapshotError::VersionMismatch`].
pub const BUS_SECTION_VERSION: u8 = 2;

/// Largest encoding of one port's expansion device in the BUS section.
///
/// The tag byte plus that device's fields. The Family BASIC / Subor keyboards
/// and the SNES mouse are the largest, at 14 bytes; an unplugged port is the
/// 1-byte tag alone.
pub const EXPANSION_DEVICE_MAX_LEN: usize = 14;

/// How much a snapshot can grow after it is first measured.
///
/// Devices attach lazily: a port reads "unplugged" (1 byte) until the host
/// plugs a device in, and the largest device then takes
/// [`EXPANSION_DEVICE_MAX_LEN`]. Two ports, so twice the difference.
///
/// v2.8.0 (libretro audit §2.1): the libretro core adds this to the
/// `retro_serialize_size` it reports at load, because the frontend sizes its
/// save-state, rewind and run-ahead buffers from that one answer, and a Zapper
/// plugged in mid-game otherwise made every later save fail. The padding it
/// leaves is zeroed and read back as padding (`save_state::SectionIter`).
pub const SAVE_STATE_DEVICE_HEADROOM: usize = 2 * (EXPANSION_DEVICE_MAX_LEN - 1);

/// Encode the bus's own state (RAM, controllers, DMA, data-bus latches,
/// cycle). The order is the on-wire layout [`decode_bus`] reads back.
pub fn encode_bus(bus: &SystemBus) -> Vec<u8> {
    let mut w = BinWriter::with_capacity(0x900);
    // Cumulative cycle counter.
    w.u64(bus.cycle());
    // CPU RAM (2 KiB).
    w.bytes(bus.ram_bytes());
    // Standard controllers (players 1 and 2).
    for c in bus.controllers_ref() {
        encode_controller(&mut w, *c);
    }
    let s = bus.bus_misc_state();
    // A `$4014` write awaiting its first DMA cycle: page, then presence.
    w.u8(s.dma_pending.unwrap_or(0));
    w.u8(u8::from(s.dma_pending.is_some()));
    // The OAM DMA's scratch byte, source page and parked CPU address.
    w.u8(s.dma_byte);
    w.u8(s.dma_page);
    w.u16(s.dma_halt_addr);
    // The external data bus (open bus) and the last CPU read address.
    w.u8(s.open_bus);
    w.u16(s.last_read_addr);
    w.u8(u8::from(s.in_dmc_dma));
    w.u16(s.deferred_dma_replay_addr);
    // The deferred controller-strobe write (Session-24 / Phase 3).
    w.u8(s.controller_write_pending);
    w.u8(s.controller_write_value);
    // Four Score: the flag, players 3 and 4, and the per-port read index and
    // signature shift register.
    w.u8(u8::from(s.four_score));
    for c in bus.controllers34_ref() {
        encode_controller(&mut w, *c);
    }
    w.u8(s.four_score_idx[0]);
    w.u8(s.four_score_idx[1]);
    w.u8(s.four_score_sig[0]);
    w.u8(s.four_score_sig[1]);
    // The unified DMA engine: the DMC halt latch and the OAM engine's state.
    w.u8(u8::from(s.dmc_halt));
    w.u8(u8::from(s.uni_oam_active));
    w.u8(u8::from(s.uni_oam_halt));
    w.u8(u8::from(s.uni_oam_aligned));
    w.u16(s.uni_oam_addr);
    // `ppu_clock`, the PPU's progress in master clocks. It MUST travel with
    // `Cpu::master_clock` (CPU section): restoring one without the other
    // desynchronises `run_ppu_to`, and `check_restored_clocks` refuses a pair
    // too far apart to be real.
    w.u64(s.ppu_clock);
    // A tag byte per port (0 = none, 1 = Zapper, 2 = Vaus, 3 = Power Pad,
    // 4 = SNES mouse, 5 = Family BASIC keyboard, 6 = Family Trainer,
    // 7 = Subor keyboard, 8 = Konami Hyper Shot, 9 = Bandai Hyper Shot)
    // followed by that device's fields.
    for port in 0..2 {
        encode_expansion_device(&mut w, bus.expansion_device(port).as_ref());
    }
    // The per-game nametable mirroring override (0 = none).
    w.u8(encode_mirroring_override(bus.mirroring_override()));
    // The controller-port CLK run state (v2.6.5). Both halves outlive an
    // instruction: a `$4016` read is the last cycle of `LDA $4016`, so a
    // snapshot at that boundary has a shift owed and a run open, and
    // restoring without them makes the next read return a bit the timeline
    // already delivered.
    for c in bus.controllers_ref() {
        w.bool(c.pending_shift);
    }
    for c in bus.controllers34_ref() {
        w.bool(c.pending_shift);
    }
    for port in 0..2 {
        w.u64(bus.port_read_cycle(port));
    }
    // The Four Score chain's own owed edge, which clocks with the pads.
    for f in bus.four_score_pending() {
        w.bool(f);
    }
    // The 2A03's INTERNAL data bus (v2.8.0): a separate latch from
    // `open_bus`, because a DMC DMA fetch drives only the external bus, and a
    // `$4015` read takes bit 5 from this one.
    w.u8(s.internal_data_bus);
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
        Some(InputDevice::SnesMouse(m)) => {
            w.u8(4);
            w.i16(m.dx_raw());
            w.i16(m.dy_raw());
            w.bool(m.left_raw());
            w.bool(m.right_raw());
            w.u8(m.sensitivity_raw());
            w.u32(m.shift_raw());
            w.u8(m.read_count_raw());
            w.bool(m.strobe_raw());
        }
        Some(InputDevice::FamilyKeyboard(k)) => {
            w.u8(5);
            w.bytes(&k.keys_raw());
            w.u8(k.row_raw());
            w.bool(k.column_raw());
            w.bool(k.enabled_raw());
            w.bool(k.clock_raw());
        }
        // v1.3.0 Workstream F1 — the Family Trainer reuses PowerPadState and
        // the Subor keyboard reuses FamilyKeyboardState, but get distinct tags
        // so a restore reattaches the SAME device variant the user selected.
        Some(InputDevice::FamilyTrainer(p)) => {
            w.u8(6);
            w.u16(p.buttons_raw());
            w.u8(p.shift_l_raw());
            w.u8(p.shift_h_raw());
            w.bool(p.strobe_raw());
        }
        Some(InputDevice::SuborKeyboard(k)) => {
            w.u8(7);
            w.bytes(&k.keys_raw());
            w.u8(k.row_raw());
            w.bool(k.column_raw());
            w.bool(k.enabled_raw());
            w.bool(k.clock_raw());
        }
        Some(InputDevice::KonamiHyperShot(h)) => {
            w.u8(8);
            w.u8(h.buttons_raw());
            w.bool(h.p1_enabled_raw());
            w.bool(h.p2_enabled_raw());
        }
        Some(InputDevice::BandaiHyperShot(b)) => {
            w.u8(9);
            w.u8(b.sensors_raw());
            w.bool(b.select_raw());
        }
    }
}

/// Decode one port's optional overlay device (tag 0 is an empty port).
// A flat one-arm-per-device-tag dispatch decoder; the length is inherent to the
// device count, not a sign of tangled logic.
#[allow(clippy::too_many_lines)]
fn decode_expansion_device(r: &mut BinReader<'_>) -> Result<Option<InputDevice>, SnapshotError> {
    Ok(match r.u8()? {
        0 => None,
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
        4 => {
            let dx = r.i16()?;
            let dy = r.i16()?;
            let left = r.bool()?;
            let right = r.bool()?;
            let sensitivity = r.u8()?;
            let shift = r.u32()?;
            let read_count = r.u8()?;
            let strobe = r.bool()?;
            Some(InputDevice::SnesMouse(SnesMouseState::from_parts(
                dx,
                dy,
                left,
                right,
                sensitivity,
                shift,
                read_count,
                strobe,
            )))
        }
        5 => {
            let mut keys = [0u8; 9];
            r.read_into(&mut keys)?;
            let row = r.u8()?;
            let column = r.bool()?;
            let enabled = r.bool()?;
            let clock = r.bool()?;
            Some(InputDevice::FamilyKeyboard(
                FamilyKeyboardState::from_parts(keys, row, column, enabled, clock),
            ))
        }
        6 => {
            let buttons = r.u16()?;
            let shift_l = r.u8()?;
            let shift_h = r.u8()?;
            let strobe = r.bool()?;
            Some(InputDevice::FamilyTrainer(
                crate::input_device::PowerPadState::from_parts(buttons, shift_l, shift_h, strobe),
            ))
        }
        7 => {
            let mut keys = [0u8; 9];
            r.read_into(&mut keys)?;
            let row = r.u8()?;
            let column = r.bool()?;
            let enabled = r.bool()?;
            let clock = r.bool()?;
            Some(InputDevice::SuborKeyboard(FamilyKeyboardState::from_parts(
                keys, row, column, enabled, clock,
            )))
        }
        8 => {
            let buttons = r.u8()?;
            let p1_enabled = r.bool()?;
            let p2_enabled = r.bool()?;
            Some(InputDevice::KonamiHyperShot(
                crate::input_device::KonamiHyperShotState::from_parts(
                    buttons, p1_enabled, p2_enabled,
                ),
            ))
        }
        9 => {
            let sensors = r.u8()?;
            let select = r.bool()?;
            Some(InputDevice::BandaiHyperShot(
                crate::input_device::BandaiHyperShotState::from_parts(sensors, select),
            ))
        }
        other => {
            return Err(SnapshotError::SectionInvalid {
                tag: "BUS ".into(),
                reason: format!("unknown expansion-device tag {other}"),
            });
        }
    })
}

/// Apply a previously [`encode_bus`]-emitted blob.
///
/// Every field is required (version 2, see the module docs): a short body is
/// [`SnapshotError::Eof`], and bytes left over after the last field are
/// [`SnapshotError::SectionInvalid`].
///
/// # Errors
///
/// Returns [`SnapshotError`] for malformed inputs.
// The body is one straight-line field-by-field decode mirroring `encode_bus`;
// splitting it would obscure the byte-order correspondence between the two.
#[allow(clippy::too_many_lines)]
pub fn decode_bus(bus: &mut SystemBus, data: &[u8]) -> Result<(), SnapshotError> {
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

    let dma_page_pending = r.u8()?;
    let dma_present = r.u8()?;
    let dma_pending = match dma_present {
        0 => None,
        1 => Some(dma_page_pending),
        other => {
            return Err(SnapshotError::SectionInvalid {
                tag: "BUS ".into(),
                reason: format!("invalid dma-pending presence {other}"),
            });
        }
    };
    let dma_byte = r.u8()?;
    let dma_page = r.u8()?;
    let dma_halt_addr = r.u16()?;
    let open_bus = r.u8()?;
    let last_read_addr = r.u16()?;
    let in_dmc_dma = r.bool()?;
    let deferred_dma_replay_addr = r.u16()?;
    let controller_write_pending = r.u8()?;
    let controller_write_value = r.u8()?;
    let four_score = r.bool()?;
    let mut controllers34 = [Controller::new(); 2];
    for c in &mut controllers34 {
        decode_controller(&mut r, c)?;
    }
    bus.set_controllers34(controllers34);
    let four_score_idx = [r.u8()?, r.u8()?];
    let four_score_sig = [r.u8()?, r.u8()?];
    let dmc_halt = r.bool()?;
    let uni_oam_active = r.bool()?;
    let uni_oam_halt = r.bool()?;
    let uni_oam_aligned = r.bool()?;
    let uni_oam_addr = r.u16()?;
    // The OAM-DMA byte index runs 0..=255 while a transfer is active and is
    // left at 256 when the 256th write completes it (it is re-zeroed only when
    // the next transfer starts). Any other value is a corrupt file: an active
    // transfer at 256 or above never reaches the `== 256` completion test and
    // counts on until the `u16` overflows -- a panic in dev profiles, found by
    // the v2.7.0 `save_state` fuzz target in 287,867 runs.
    if uni_oam_addr > 256 || (uni_oam_active && uni_oam_addr > 255) {
        return Err(SnapshotError::SectionInvalid {
            tag: "BUS ".into(),
            reason: format!(
                "OAM-DMA byte index {uni_oam_addr} out of range (active: {uni_oam_active})"
            ),
        });
    }
    let ppu_clock = r.u64()?;
    let device0 = decode_expansion_device(&mut r)?;
    let device1 = decode_expansion_device(&mut r)?;
    bus.set_expansion_device(0, device0);
    bus.set_expansion_device(1, device1);
    bus.set_mirroring_override(decode_mirroring_override(r.u8()?));
    let mut pending = [false; 4];
    for p in &mut pending {
        *p = r.bool()?;
    }
    let mut cycles = [0u64; 2];
    for c in &mut cycles {
        *c = r.u64()?;
    }
    // Through a setter, not locals: the controllers were installed above, and
    // mutating copies here would decode cleanly and restore nothing.
    bus.set_controller_run_state(pending, cycles);
    let mut fs = [false; 2];
    for f in &mut fs {
        *f = r.bool()?;
    }
    bus.set_four_score_pending(fs);
    let internal_data_bus = r.u8()?;
    if r.remaining() != 0 {
        return Err(SnapshotError::SectionInvalid {
            tag: "BUS ".into(),
            reason: format!("{} unexpected trailing bytes", r.remaining()),
        });
    }
    bus.set_bus_misc_state(BusMiscState {
        dma_pending,
        dma_byte,
        dma_page,
        dma_halt_addr,
        deferred_dma_replay_addr,
        open_bus,
        internal_data_bus,
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
    /// Scratch byte for the OAM DMA's read/write pair.
    pub dma_byte: u8,
    /// Active OAM DMA page.
    pub dma_page: u8,
    /// CPU read address repeated while OAM DMA has the CPU halted.
    pub dma_halt_addr: u16,
    /// Deferred DMC readout side-effect target for absolute register reads.
    pub deferred_dma_replay_addr: u16,
    /// Open-bus latch (the EXTERNAL data bus).
    pub open_bus: u8,
    /// The 2A03's INTERNAL data bus, which a DMC DMA fetch does not drive;
    /// `$4015` bit 5 reads it (v2.8.0).
    pub internal_data_bus: u8,
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
    /// (`TriCNES` `DoOAMDMA`).
    pub uni_oam_active: bool,
    /// W3-Stage-4: unified DMA engine — `TriCNES` `OAMDMA_Halt`.
    pub uni_oam_halt: bool,
    /// W3-Stage-4: unified DMA engine — `TriCNES` `OAMDMA_Aligned`.
    pub uni_oam_aligned: bool,
    /// W3-Stage-4: unified DMA engine — `TriCNES` `DMAAddress` (the OAM
    /// byte index).
    pub uni_oam_addr: u16,
    /// PPU progress in master-clock units (the `run_ppu_to` cursor). Paired
    /// with `Cpu::master_clock` (CPU section).
    pub ppu_clock: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_device::{
        BandaiHyperShotState, KonamiHyperShotState, PowerPadState, SnesMouseState,
    };
    use alloc::vec;

    /// One of every device, named by an exhaustive `match` so that adding an
    /// `InputDevice` variant fails to compile here until its encoding is
    /// checked against [`EXPANSION_DEVICE_MAX_LEN`].
    fn every_device() -> Vec<InputDevice> {
        let all = vec![
            InputDevice::Zapper(ZapperState::from_parts(0, 0, false, false)),
            InputDevice::Vaus(VausState::from_parts(0, false, 0, false)),
            InputDevice::PowerPad(PowerPadState::from_parts(0, 0, 0, false)),
            InputDevice::SnesMouse(SnesMouseState::from_parts(
                0, 0, false, false, 0, 0, 0, false,
            )),
            InputDevice::FamilyKeyboard(FamilyKeyboardState::from_parts(
                [0; 9], 0, false, false, false,
            )),
            InputDevice::FamilyTrainer(PowerPadState::from_parts(0, 0, 0, false)),
            InputDevice::SuborKeyboard(FamilyKeyboardState::from_parts(
                [0; 9], 0, false, false, false,
            )),
            InputDevice::KonamiHyperShot(KonamiHyperShotState::from_parts(0, false, false)),
            InputDevice::BandaiHyperShot(BandaiHyperShotState::from_parts(0, false)),
        ];
        for d in &all {
            match d {
                InputDevice::Zapper(_)
                | InputDevice::Vaus(_)
                | InputDevice::PowerPad(_)
                | InputDevice::SnesMouse(_)
                | InputDevice::FamilyKeyboard(_)
                | InputDevice::FamilyTrainer(_)
                | InputDevice::SuborKeyboard(_)
                | InputDevice::KonamiHyperShot(_)
                | InputDevice::BandaiHyperShot(_) => {}
            }
        }
        all
    }

    fn encoded_len(device: Option<&InputDevice>) -> usize {
        let mut w = BinWriter::with_capacity(32);
        encode_expansion_device(&mut w, device);
        w.into_vec().len()
    }

    #[test]
    fn no_expansion_device_encodes_past_the_documented_maximum() {
        assert_eq!(
            encoded_len(None),
            1,
            "an unplugged port is the tag byte alone"
        );
        let largest = every_device()
            .iter()
            .map(|d| encoded_len(Some(d)))
            .max()
            .expect("at least one device");
        assert_eq!(
            largest, EXPANSION_DEVICE_MAX_LEN,
            "EXPANSION_DEVICE_MAX_LEN must be the largest device encoding exactly: \
             larger and the libretro core's save-state buffer is too small, smaller \
             and it is merely wasteful but the documented figure is wrong"
        );
    }
}

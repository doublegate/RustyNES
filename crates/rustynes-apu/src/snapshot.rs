//! Save-state encoding / decoding for the [`Apu`].
//!
//! Hand-rolled little-endian binary so the crate stays free of `serde` /
//! `bincode`. The container that wraps this blob into a tagged section
//! lives in `rustynes_core::save_state`.
//!
//! The blob covers the four wave channels, DMC, frame counter, mixer phase /
//! filter state, blip buffer (drained on restore), cycle bookkeeping, the
//! DMC-DMA scheduling bytes, the W3-Stage-4 master-clock DMA-engine state
//! (get/put parity, the exclusion/need latches, the delayed-`$4015`
//! DMC-status machinery) and the scheduled warm-reset `$4017` re-write.
//!
//! Since v2.9.8 (ADR 0042) [`Apu::restore`] reads the current version only,
//! and every field is required. Until then it accepted versions 1-3,
//! migrating the frame counter's IRQ fields, and read the DMC-DMA bytes and
//! the Stage-4 tail as trailing-optional, synthesising best-effort defaults
//! for blobs that ended early. Until v5 (v2.9.9) the blip's synthesis state
//! was not preserved, so a restore restarted the resampler cold and the audio
//! after a load differed from an unrestored run (libretro re-audit NL-12); v5
//! carries it. The undrained output queue is still not carried: hosts drain
//! it every frame, so it is empty wherever a snapshot is taken.

use alloc::vec::Vec;
use thiserror::Error;

use crate::Region;
use crate::apu::Apu;
use crate::blip::BlipBuf;
use crate::dmc::Dmc;
use crate::envelope::Envelope;
use crate::frame_counter::{FrameCounter, Mode as FcMode};
use crate::length::LengthCounter;
use crate::mixer::{FilterChain, OnePole};
use crate::noise::Noise;
use crate::pulse::Pulse;
use crate::triangle::Triangle;

/// Schema version for the APU snapshot blob.
///
/// - v1 (v0.9.0 .. v1.0.0-rc2): original schema with `FrameCounter`
///   carrying a `pending_irq_clear: bool` consumed at the next tick.
/// - v2 (Session-25, 2026-05-23): `FrameCounter` replaces the bool
///   with a `irq_flag_clear_cycle: u64` lazy-clear schedule mirroring
///   Mesen2's `_irqFlagClearClock`. Old v1 blobs restore by migrating
///   the bool to a synthesized schedule (a pending clear becomes
///   "schedule for `cpu_cycle + 1`", a fresh clear).
/// - v3 (Session-26 Sprint 2 iter 5, 2026-05-23 onwards):
///   `FrameCounter` adds `irq_line_active: bool` as a SEPARATE field
///   from `irq_flag`. v2 blobs migrate by setting both fields to the
///   v2 `irq_flag` value (the IRQ-line state coincided with $4015
///   bit 6 visibility under the v2 conflated model). Per ADR-0003,
///   the v2 -> v3 migration may show a 1-cycle transient where a
///   reloaded inhibited state has the CPU IRQ line deasserted as the
///   FC step re-establishes it — acceptable.
/// - v4 (2026-07-22): appends the scheduled warm-reset `$4017` re-write
///   (`reset_4017_delay` + `reset_4017_value`, 2 bytes). [`Apu::reset`] arms
///   the countdown at 2 and `tick_with_external` decrements it once per CPU
///   cycle, issuing `FrameCounter::write` when it hits zero (the v2.0.0
///   beta.3 A4 cycle-accurate reset, calibrated against blargg
///   `4017_timing`). Both fields were previously unserialized, so a snapshot
///   taken inside that 2-cycle window restored `delay = 0` and dropped the
///   re-write entirely — the restored frame counter then kept the sequencer
///   phase the re-write was supposed to reset. This is the same class as the
///   PPU's v5 / v6 / v8 tails (ADR 0030 / ADR 0034): live mid-frame state
///   absent from the schema, invisible to any straight-`run_frame` test and
///   reachable only through a snapshot/restore round trip. Surfaced by the
///   standing schema audit
///   (`crates/rustynes-test-harness/tests/snapshot_schema_audit.rs`) rather
///   than by a user-visible symptom.
///
///
/// Since v2.9.8 (ADR 0042) only v4 is read, with every field required: the
/// earlier versions' migrations and the trailing-optional tails (the v1.x
/// DMC-DMA scheduling bytes and the W3-Stage-4 block) are gone. The v4 layout
/// itself is unchanged, so the number did not move.
///
/// v5 (v2.9.9, NL-12): the blip resampler's synthesis state (ring head,
/// warm-up flag, integrator, the 32 delta slots still in flight), so a
/// save/load round trip at a frame boundary resumes the exact audio
/// stream and serializes the same bytes as a run that never restored. v4 is
/// refused (ADR 0042's current-version-only rule).
pub const APU_SNAPSHOT_VERSION: u8 = 5;

/// Errors returned by [`Apu::restore`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ApuSnapshotError {
    /// Blob is shorter than the schema declares.
    #[error("APU snapshot truncated at offset {0}")]
    Truncated(usize),
    /// Blob is longer than the schema declares: this many bytes follow the
    /// last field (v2.9.8; until then reported as [`Self::Truncated`]).
    #[error("APU snapshot has {0} trailing byte(s) after its last field")]
    TrailingBytes(usize),
    /// The blob's version byte is not understood by this build.
    #[error("APU snapshot unsupported version {0}")]
    UnsupportedVersion(u8),
    /// Region tag was not 0/1/2.
    #[error("APU snapshot has invalid region tag {0}")]
    InvalidRegion(u8),
    /// Frame-counter mode tag was not 0/1.
    #[error("APU snapshot has invalid frame-counter mode tag {0}")]
    InvalidMode(u8),
    /// Optional sample-buffer presence byte was not 0/1.
    #[error("APU snapshot has invalid optional presence byte {0}")]
    InvalidPresence(u8),
    /// A register-width field held a value its hardware register cannot.
    ///
    /// Several of these index fixed tables on the next tick (`duty` and `step`
    /// into the duty table, the triangle `step` into its 32-step sequence) or
    /// flow into one (`decay`, a constant-volume `volume_or_period` and the DMC
    /// `dac` sum into the mixer's 31- and 203-entry lookup tables), so an
    /// unchecked value restores cleanly and then panics one CPU cycle later.
    /// The rest (sweep, DMC rate / bit count) cannot panic today but are
    /// bounded to the same register width so that no field of the restored
    /// state is one the emulator itself could never have written. Core audit
    /// IMP-02.
    #[error("APU snapshot field `{field}` is {value}, above its maximum {max}")]
    FieldOutOfRange {
        /// Which field, as `channel.field`.
        field: &'static str,
        /// The value found in the blob.
        value: u8,
        /// The largest value the hardware register can hold.
        max: u8,
    },
    /// A floating-point resampler or filter field was not usable.
    ///
    /// The band-limited resampler advances `phase` by `sample_rate / cpu_rate`
    /// per CPU cycle and emits one host sample per whole unit crossed, so a
    /// zero, negative, non-finite or merely huge ratio does not panic: it hangs
    /// the emulation thread in that loop, or fills the host audio with NaN.
    /// Core audit IMP-02.
    #[error("APU snapshot resampler field `{0}` is out of range or not finite")]
    InvalidResampler(&'static str),
}

/// Read one `u8` field and reject it if it exceeds `max` (IMP-02).
fn bounded(r: &mut R<'_>, field: &'static str, max: u8) -> Result<u8, ApuSnapshotError> {
    let value = r.u8()?;
    if value > max {
        return Err(ApuSnapshotError::FieldOutOfRange { field, value, max });
    }
    Ok(value)
}

/// Reject a non-finite float (IMP-02). A NaN in any filter or resampler field
/// propagates into every later host sample; an infinity does the same after
/// one subtraction.
fn finite_f32(v: f32, field: &'static str) -> Result<f32, ApuSnapshotError> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(ApuSnapshotError::InvalidResampler(field))
    }
}

// Bounds on the resampler's restored signal state (NC-09 and NL-12, v2.9.9
// re-audits). `finite` alone was not enough: a finite value near `f32::MAX`
// overflows to infinity within a few samples, the filter state then stays
// non-finite, host audio is NaN for the rest of the session, and every
// snapshot the machine takes afterwards is refused by this same validator,
// which also breaks `Nes::restore`'s rollback (it restores the machine's own
// snapshot). `1.0e38` decays; `-2.05e38` and `f32::MAX` do not.
//
// Each bound is chosen so that a state inside it can only produce states
// inside it, which is what keeps the machine's own snapshot loadable:
//
// - `held_value` is the last input `add_sample` took, which it clamps to
//   `-4.0..=4.0`.
// - The integrator tracks the input: once every delta in flight has been
//   integrated it equals `held_value`, so `integrator + sum(window) -
//   held_value` is a constant of the motion (zero, up to float rounding,
//   for a state this emulator produced). Requiring it under
//   `RESAMPLER_DRIFT_MAX`, and every partial sum under
//   `RESAMPLER_INTEGRATOR_MAX`, keeps every future integrator value near the
//   clamped input.
// - The filters are stable one-poles fed by that integrator. A high-pass's
//   state can grow only by its input step (at most 8) times its coefficient,
//   and shrinks by `1 - coeff` of itself each sample, so any bound above
//   `8 * a / (1 - a)` (about 607 for the 90 Hz stage at 44.1 kHz) is closed
//   under the filter. `RESAMPLER_FILTER_MAX` leaves margin above that.
const RESAMPLER_HELD_MAX: f32 = 4.0;
const RESAMPLER_INTEGRATOR_MAX: f32 = 16.0;
const RESAMPLER_DRIFT_MAX: f32 = 1.0;
const RESAMPLER_FILTER_MAX: f32 = 1024.0;

fn bounded_f32(v: f32, max: f32, field: &'static str) -> Result<f32, ApuSnapshotError> {
    let v = finite_f32(v, field)?;
    if v.abs() <= max {
        Ok(v)
    } else {
        Err(ApuSnapshotError::InvalidResampler(field))
    }
}

fn region_to_u8(r: Region) -> u8 {
    match r {
        Region::Ntsc => 0,
        Region::Pal => 1,
        Region::Dendy => 2,
    }
}
fn region_from_u8(v: u8) -> Result<Region, ApuSnapshotError> {
    match v {
        0 => Ok(Region::Ntsc),
        1 => Ok(Region::Pal),
        2 => Ok(Region::Dendy),
        other => Err(ApuSnapshotError::InvalidRegion(other)),
    }
}
fn mode_to_u8(m: FcMode) -> u8 {
    match m {
        FcMode::FourStep => 0,
        FcMode::FiveStep => 1,
    }
}
fn mode_from_u8(v: u8) -> Result<FcMode, ApuSnapshotError> {
    match v {
        0 => Ok(FcMode::FourStep),
        1 => Ok(FcMode::FiveStep),
        other => Err(ApuSnapshotError::InvalidMode(other)),
    }
}

struct W {
    buf: Vec<u8>,
}
impl W {
    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn bool(&mut self, v: bool) {
        self.buf.push(u8::from(v));
    }
}

struct R<'a> {
    src: &'a [u8],
    pos: usize,
}
impl R<'_> {
    fn need(&self, n: usize) -> Result<(), ApuSnapshotError> {
        if self.src.len() - self.pos < n {
            return Err(ApuSnapshotError::Truncated(self.pos));
        }
        Ok(())
    }
    fn u8(&mut self) -> Result<u8, ApuSnapshotError> {
        self.need(1)?;
        let v = self.src[self.pos];
        self.pos += 1;
        Ok(v)
    }
    fn u16(&mut self) -> Result<u16, ApuSnapshotError> {
        self.need(2)?;
        let v = u16::from_le_bytes([self.src[self.pos], self.src[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }
    fn u32(&mut self) -> Result<u32, ApuSnapshotError> {
        self.need(4)?;
        let mut a = [0u8; 4];
        a.copy_from_slice(&self.src[self.pos..self.pos + 4]);
        self.pos += 4;
        Ok(u32::from_le_bytes(a))
    }
    fn u64(&mut self) -> Result<u64, ApuSnapshotError> {
        self.need(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(&self.src[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(u64::from_le_bytes(a))
    }
    fn f32(&mut self) -> Result<f32, ApuSnapshotError> {
        self.need(4)?;
        let mut a = [0u8; 4];
        a.copy_from_slice(&self.src[self.pos..self.pos + 4]);
        self.pos += 4;
        Ok(f32::from_le_bytes(a))
    }
    fn f64(&mut self) -> Result<f64, ApuSnapshotError> {
        self.need(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(&self.src[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(f64::from_le_bytes(a))
    }
    fn bool(&mut self) -> Result<bool, ApuSnapshotError> {
        Ok(self.u8()? != 0)
    }
}

fn write_envelope(w: &mut W, e: Envelope) {
    w.bool(e.start);
    w.bool(e.loop_flag);
    w.bool(e.constant);
    w.u8(e.volume_or_period);
    w.u8(e.divider);
    w.u8(e.decay);
}
fn read_envelope(r: &mut R<'_>) -> Result<Envelope, ApuSnapshotError> {
    Ok(Envelope {
        start: r.bool()?,
        loop_flag: r.bool()?,
        constant: r.bool()?,
        // All three are 4-bit: `$4000`/`$400C` bits 0-3, a divider reloaded
        // from that period, and a counter that runs 15 -> 0.
        volume_or_period: bounded(r, "envelope.volume_or_period", 15)?,
        divider: bounded(r, "envelope.divider", 15)?,
        decay: bounded(r, "envelope.decay", 15)?,
    })
}

fn write_length(w: &mut W, l: LengthCounter) {
    w.u8(l.count);
    w.bool(l.halt);
    w.bool(l.enabled);
}
fn read_length(r: &mut R<'_>) -> Result<LengthCounter, ApuSnapshotError> {
    let count = r.u8()?;
    let halt = r.bool()?;
    let enabled = r.bool()?;
    // The deferred-write scratch fields (`new_halt` / `reload_val` /
    // `previous_count`) are NOT serialized: they resolve within the same CPU
    // cycle as the register write that sets them (`LengthCounter::reload` runs
    // every cycle), so no live deferral survives to a save-state taken at an
    // instruction boundary. The snapshot byte layout is therefore unchanged
    // (count + halt + enabled). `new_halt` MUST be seeded to the restored
    // `halt`, otherwise the first post-restore `reload` would promote a stale
    // `false` and spuriously clear a genuinely-halted counter.
    Ok(LengthCounter {
        count,
        halt,
        new_halt: halt,
        enabled,
        reload_val: 0,
        previous_count: 0,
    })
}

fn write_pulse(w: &mut W, p: &Pulse) {
    w.u8(p.duty);
    w.u8(p.step);
    w.u16(p.timer_period);
    w.u16(p.timer);
    write_envelope(w, p.envelope);
    write_length(w, p.length);
    w.bool(p.sweep_enabled);
    w.u8(p.sweep_period);
    w.bool(p.sweep_negate);
    w.u8(p.sweep_shift);
    w.bool(p.sweep_reload);
    w.u8(p.sweep_divider);
    w.bool(p.is_pulse1);
}
fn read_pulse(r: &mut R<'_>) -> Result<Pulse, ApuSnapshotError> {
    // `duty` and `step` index `DUTY_TABLE: [[u8; 8]; 4]`; the three sweep
    // fields are the 3-bit fields of `$4001`/`$4005` (a shift of 16 or more
    // would also overflow the `u16` shift in `sweep_target`).
    let duty = bounded(r, "pulse.duty", 3)?;
    let step = bounded(r, "pulse.step", 7)?;
    let timer_period = r.u16()?;
    let timer = r.u16()?;
    let envelope = read_envelope(r)?;
    let length = read_length(r)?;
    let sweep_enabled = r.bool()?;
    let sweep_period = bounded(r, "pulse.sweep_period", 7)?;
    let sweep_negate = r.bool()?;
    let sweep_shift = bounded(r, "pulse.sweep_shift", 7)?;
    let sweep_reload = r.bool()?;
    let sweep_divider = bounded(r, "pulse.sweep_divider", 7)?;
    let is_pulse1 = r.bool()?;
    let mut p = Pulse::new(is_pulse1);
    p.duty = duty;
    p.step = step;
    p.timer_period = timer_period;
    p.timer = timer;
    p.envelope = envelope;
    p.length = length;
    p.sweep_enabled = sweep_enabled;
    p.sweep_period = sweep_period;
    p.sweep_negate = sweep_negate;
    p.sweep_shift = sweep_shift;
    p.sweep_reload = sweep_reload;
    p.sweep_divider = sweep_divider;
    Ok(p)
}

fn write_triangle(w: &mut W, t: &Triangle) {
    w.u16(t.timer_period);
    w.u16(t.timer);
    w.u8(t.step);
    write_length(w, t.length);
    w.u8(t.linear_reload_value);
    w.u8(t.linear_counter);
    w.bool(t.linear_control);
    w.bool(t.linear_reload_flag);
}
fn read_triangle(r: &mut R<'_>) -> Result<Triangle, ApuSnapshotError> {
    let mut t = Triangle::new();
    t.timer_period = r.u16()?;
    t.timer = r.u16()?;
    // Indexes the 32-step `TRIANGLE_TABLE`.
    t.step = bounded(r, "triangle.step", 31)?;
    t.length = read_length(r)?;
    t.linear_reload_value = r.u8()?;
    t.linear_counter = r.u8()?;
    t.linear_control = r.bool()?;
    t.linear_reload_flag = r.bool()?;
    Ok(t)
}

fn write_noise(w: &mut W, n: &Noise) {
    w.u16(n.lfsr);
    w.bool(n.mode);
    w.u16(n.timer_period);
    w.u16(n.timer);
    write_envelope(w, n.envelope);
    write_length(w, n.length);
    w.u8(region_to_u8(n.region));
}
fn read_noise(r: &mut R<'_>) -> Result<Noise, ApuSnapshotError> {
    let lfsr = r.u16()?;
    let mode = r.bool()?;
    let timer_period = r.u16()?;
    let timer = r.u16()?;
    let envelope = read_envelope(r)?;
    let length = read_length(r)?;
    let region = region_from_u8(r.u8()?)?;
    let mut n = Noise::new(region);
    n.lfsr = lfsr;
    n.mode = mode;
    n.timer_period = timer_period;
    n.timer = timer;
    n.envelope = envelope;
    n.length = length;
    Ok(n)
}

fn write_dmc(w: &mut W, d: &Dmc) {
    w.bool(d.irq_enable);
    w.bool(d.loop_flag);
    w.u8(d.rate_index);
    w.u16(d.sample_addr);
    w.u16(d.sample_length);
    w.u16(d.current_addr);
    w.u16(d.bytes_remaining);
    if let Some(b) = d.sample_buffer {
        w.u8(1);
        w.u8(b);
    } else {
        w.u8(0);
        w.u8(0);
    }
    w.u8(d.shift_register);
    w.u8(d.bits_remaining);
    w.u8(d.dac);
    w.bool(d.silence);
    w.u16(d.timer_period);
    w.u16(d.timer);
    w.bool(d.irq_flag);
}
fn read_dmc(r: &mut R<'_>, region: Region) -> Result<Dmc, ApuSnapshotError> {
    let irq_enable = r.bool()?;
    let loop_flag = r.bool()?;
    // `$4010` bits 0-3.
    let rate_index = bounded(r, "dmc.rate_index", 15)?;
    let sample_addr = r.u16()?;
    let sample_length = r.u16()?;
    let current_addr = r.u16()?;
    let bytes_remaining = r.u16()?;
    let presence = r.u8()?;
    let buf_byte = r.u8()?;
    let sample_buffer = match presence {
        0 => None,
        1 => Some(buf_byte),
        other => return Err(ApuSnapshotError::InvalidPresence(other)),
    };
    let shift_register = r.u8()?;
    // The output unit counts 8 -> 0; the DAC is 7-bit and its value feeds the
    // mixer's 203-entry `tnd_table`, which a DAC above 127 overruns.
    let bits_remaining = bounded(r, "dmc.bits_remaining", 8)?;
    let dac = bounded(r, "dmc.dac", 127)?;
    let silence = r.bool()?;
    let timer_period = r.u16()?;
    let timer = r.u16()?;
    let irq_flag = r.bool()?;
    let mut d = Dmc::new(region);
    d.irq_enable = irq_enable;
    d.loop_flag = loop_flag;
    d.rate_index = rate_index;
    d.sample_addr = sample_addr;
    d.sample_length = sample_length;
    d.current_addr = current_addr;
    d.bytes_remaining = bytes_remaining;
    d.sample_buffer = sample_buffer;
    d.shift_register = shift_register;
    d.bits_remaining = bits_remaining;
    d.dac = dac;
    d.silence = silence;
    d.timer_period = timer_period;
    d.timer = timer;
    d.irq_flag = irq_flag;
    Ok(d)
}

fn write_fc(w: &mut W, fc: &FrameCounter) {
    w.u8(mode_to_u8(fc.mode));
    w.bool(fc.irq_inhibit);
    w.bool(fc.irq_flag);
    w.u32(fc.cycle);
    w.u8(fc.reset_in);
    w.u8(mode_to_u8(fc.pending_mode));
    w.bool(fc.pending_inhibit);
    w.bool(fc.apu_aligned);
    // v2 (Session-25, 2026-05-23): lazy `$4015`-read clear schedule.
    // 0 = no pending clear; otherwise the CPU cycle at which the
    // clear matures. Replaces the v1 `pending_irq_clear: bool`.
    w.u64(fc.irq_flag_clear_cycle);
    // v3 (Session-26 iter 5, 2026-05-23): CPU IRQ line driver
    // (`irq_line_active`) is now a separate field from `irq_flag`.
    // Mesen2's `IRQSource::FrameCounter` registration on the CPU's
    // `_irqSource` list, distinct from `_irqFlag` ($4015 bit 6
    // visibility).
    w.bool(fc.irq_line_active);
}
fn read_fc(r: &mut R<'_>) -> Result<FrameCounter, ApuSnapshotError> {
    let mode = mode_from_u8(r.u8()?)?;
    let irq_inhibit = r.bool()?;
    let irq_flag = r.bool()?;
    let cycle = r.u32()?;
    let reset_in = r.u8()?;
    let pending_mode = mode_from_u8(r.u8()?)?;
    let pending_inhibit = r.bool()?;
    let apu_aligned = r.bool()?;
    let irq_flag_clear_cycle = r.u64()?;
    let irq_line_active = r.bool()?;
    let mut fc = FrameCounter::new();
    fc.mode = mode;
    fc.irq_inhibit = irq_inhibit;
    fc.irq_flag = irq_flag;
    fc.irq_line_active = irq_line_active;
    fc.cycle = cycle;
    fc.reset_in = reset_in;
    fc.pending_mode = pending_mode;
    fc.pending_inhibit = pending_inhibit;
    fc.apu_aligned = apu_aligned;
    fc.irq_flag_clear_cycle = irq_flag_clear_cycle;
    Ok(fc)
}

fn write_onepole(w: &mut W, o: &OnePole) {
    w.f32(o.coeff);
    w.f32(o.prev_in);
    w.f32(o.prev_out);
    w.bool(o.is_hpf);
}
fn read_onepole(r: &mut R<'_>) -> Result<OnePole, ApuSnapshotError> {
    let coeff = finite_f32(r.f32()?, "filter.coeff")?;
    // Both constructors keep the coefficient in [0, 1]: `exp(-2*pi*fc/fs)` for
    // the high-pass, `1 - exp(-2*pi*fc/fs)` for the low-pass. A finite value
    // above 1 in the high-pass feeds `prev_out` back with gain > 1, which
    // diverges to infinity and then NaN in the host audio (review finding on
    // #546, CodeRabbit), so finiteness alone is not enough.
    if !(0.0..=1.0).contains(&coeff) {
        return Err(ApuSnapshotError::InvalidResampler("filter.coeff"));
    }
    let prev_in = bounded_f32(r.f32()?, RESAMPLER_FILTER_MAX, "filter.prev_in")?;
    let prev_out = bounded_f32(r.f32()?, RESAMPLER_FILTER_MAX, "filter.prev_out")?;
    let is_hpf = r.bool()?;
    // Reconstruct by overriding fields of a default-shape filter; we use
    // either high_pass or low_pass to get the right shape, then patch the
    // mutable state.
    let mut o = if is_hpf {
        OnePole::high_pass(0.0, 1.0)
    } else {
        OnePole::low_pass(0.0, 1.0)
    };
    o.coeff = coeff;
    o.prev_in = prev_in;
    o.prev_out = prev_out;
    o.is_hpf = is_hpf;
    Ok(o)
}

fn write_filter(w: &mut W, f: &FilterChain) {
    write_onepole(w, &f.hp1);
    write_onepole(w, &f.hp2);
    write_onepole(w, &f.lp);
}
fn read_filter(r: &mut R<'_>) -> Result<FilterChain, ApuSnapshotError> {
    let hp1 = read_onepole(r)?;
    let hp2 = read_onepole(r)?;
    let lp = read_onepole(r)?;
    Ok(FilterChain { hp1, hp2, lp })
}

fn write_blip(w: &mut W, b: &BlipBuf) {
    w.u32(b.sample_rate);
    w.f64(b.cpu_rate);
    w.f64(b.phase);
    write_filter(w, &b.filter);
    w.f32(b.held_value);
    // v5 (v2.9.9, NL-12): the band-limited synthesis state, so a restore
    // resumes the exact stream. Until v5 a restore restarted the resampler
    // cold: the frame after a load lost about 17 samples to warm-up and the
    // integrator's level (a click), and the filter state then diverged from
    // an unrestored run for good. Fixed size (135 bytes), so libretro's
    // `retro_serialize_size`, read once at load, still covers every state.
    //
    // The undrained output queue is still NOT carried: it is host-rate
    // output already produced, not machine state, and its length varies.
    // Every host drains it at the end of each frame, which is where save
    // states, run-ahead and rollback snapshot, so at those points it is
    // empty and the round trip is exact.
    let (head, primed, integrator, window) = b.live_state();
    w.u16(head);
    w.bool(primed);
    w.f32(integrator);
    for v in window {
        w.f32(v);
    }
}
fn read_blip(r: &mut R<'_>) -> Result<BlipBuf, ApuSnapshotError> {
    let sample_rate = r.u32()?;
    let cpu_rate = r.f64()?;
    let phase = r.f64()?;
    let filter = read_filter(r)?;
    let held_value = bounded_f32(r.f32()?, RESAMPLER_HELD_MAX, "blip.held_value")?;
    let head = r.u16()?;
    let primed = r.bool()?;
    let integrator = bounded_f32(r.f32()?, RESAMPLER_INTEGRATOR_MAX, "blip.integrator")?;
    let mut window = [0.0f32; crate::blip_kernel::TAPS];
    let mut partial = integrator;
    for v in &mut window {
        *v = finite_f32(r.f32()?, "blip.delta_window")?;
        partial += *v;
        if !partial.is_finite() || partial.abs() > RESAMPLER_INTEGRATOR_MAX {
            return Err(ApuSnapshotError::InvalidResampler("blip.delta_window"));
        }
    }
    if (partial - held_value).abs() > RESAMPLER_DRIFT_MAX {
        return Err(ApuSnapshotError::InvalidResampler("blip.delta_window"));
    }

    if sample_rate == 0 {
        return Err(ApuSnapshotError::InvalidResampler("blip.sample_rate"));
    }
    // `is_finite` rejects NaN and both infinities first, so the plain
    // comparison that follows is total.
    if !cpu_rate.is_finite() || cpu_rate <= 0.0 {
        return Err(ApuSnapshotError::InvalidResampler("blip.cpu_rate"));
    }
    // The resampler emits one host sample per unit of phase crossed, and
    // `add_sample` runs once per CPU cycle. A ratio above one output per
    // CPU cycle is not a configuration the emulator produces (host rates are
    // tens of kHz against a ~1.7 MHz CPU), and it is the knob that turns a
    // finite corrupt value into an arbitrarily long `while phase >= 1.0` loop.
    if f64::from(sample_rate) / cpu_rate > 1.0 {
        return Err(ApuSnapshotError::InvalidResampler(
            "blip.sample_rate/cpu_rate",
        ));
    }
    if !(0.0..1.0).contains(&phase) {
        return Err(ApuSnapshotError::InvalidResampler("blip.phase"));
    }
    let mut b = BlipBuf::new(sample_rate, cpu_rate);
    b.phase = phase;
    b.filter = filter;
    b.held_value = held_value;
    b.set_live_state(head, primed, integrator, &window);
    Ok(b)
}

impl Apu {
    /// Encode the APU's mutable state into a versioned binary blob.
    #[must_use]
    pub fn snapshot(&self) -> Vec<u8> {
        let mut w = W {
            buf: Vec::with_capacity(512),
        };
        w.u8(APU_SNAPSHOT_VERSION);
        w.u8(region_to_u8(self.region));

        write_pulse(&mut w, &self.pulse1);
        write_pulse(&mut w, &self.pulse2);
        write_triangle(&mut w, &self.triangle);
        write_noise(&mut w, &self.noise);
        write_dmc(&mut w, &self.dmc);
        write_fc(&mut w, &self.frame_counter);
        write_blip(&mut w, &self.blip);

        w.bool(self.apu_phase);
        w.u64(self.cpu_cycle);
        w.bool(self.pending_dmc_dma);
        w.u16(self.dmc_dma_addr);
        w.u32(self.sample_rate);
        w.u8(self.dmc_dma_delay);
        w.bool(self.dmc_dma_is_load);
        w.bool(self.pending_dmc_abort);
        w.u8(self.dmc_abort_delay);
        w.bool(self.dmc_dma_short);
        w.bool(self.defer_dmc_reload_once);
        w.u8(self.dmc_dma_cooldown);
        w.u8(self.dmc_reload_suppress_outputs);

        // === W3-Stage-4 (2026-06-10) trailing tail ===
        // Serializes the master-clock DMA-engine state that the
        // `mc-r1-full-cpu` umbrella promotion made load-bearing across an
        // instruction boundary: the exact get/put parity, the TriCNES
        // `CannotRunDMCDMARightNow` exclusion + its companion latches, the
        // get/put-scheduler need flags, and the W3-Stage-3 delayed-`$4015`
        // DMC-status machinery (pending slot + countdown + the implicit-abort
        // trio + the `$540` consume-edge arm-suppress latch). The bytes are
        // written UNCONDITIONALLY so the blob layout is identical across
        // feature builds.
        w.bool(self.put_cycle);
        w.u64(self.parity_seed);
        w.u8(self.cannot_run_dmc_dma);
        w.bool(self.dmc_reenable_period_block);
        w.u8(self.subpos_arm_countdown);
        w.bool(self.dmc_need_halt);
        w.bool(self.dmc_need_dummy_read);
        w.bool(self.pending_dmc_dma_next);
        {
            w.u8(self.dmc_delayed_4015);
            w.bool(self.dmc_delayed_status);
            w.bool(self.dmc_status_applied);
            w.bool(self.dmc_set_implicit_abort);
            w.bool(self.dmc_implicit_abort);
            w.bool(self.dmc_edge_arm_suppress);
        }

        // === v4 (2026-07-22) scheduled warm-reset `$4017` re-write ===
        // Armed by `Apu::reset` (delay = 2, value = the frame counter's last
        // `$4017`), consumed one CPU cycle at a time in `tick_with_external`.
        // Live for only those 2 cycles, but a snapshot landing in them used to
        // restore `delay = 0` and silently cancel the re-write.
        w.u8(self.reset_4017_delay);
        w.u8(self.reset_4017_value);

        w.buf
    }

    /// Decode a previously [`Apu::snapshot`]ed blob.
    ///
    /// # Errors
    ///
    /// Returns [`ApuSnapshotError`] on a malformed blob.
    pub fn restore(&mut self, data: &[u8]) -> Result<(), ApuSnapshotError> {
        let mut r = R { src: data, pos: 0 };
        let version = r.u8()?;
        // Only the current version is read (v2.9.8, ADR 0042); see the
        // module docs for what older versions used to migrate.
        if version != APU_SNAPSHOT_VERSION {
            return Err(ApuSnapshotError::UnsupportedVersion(version));
        }
        self.region = region_from_u8(r.u8()?)?;

        self.pulse1 = read_pulse(&mut r)?;
        self.pulse2 = read_pulse(&mut r)?;
        self.triangle = read_triangle(&mut r)?;
        self.noise = read_noise(&mut r)?;
        self.dmc = read_dmc(&mut r, self.region)?;
        self.frame_counter = read_fc(&mut r)?;
        // v2.1.5: the frame counter's PAL step-position selector is derived
        // from region, not persisted (the snapshot format is unchanged). Re-
        // derive it here from the just-restored region so a restored PAL state
        // keeps the PAL sequencer positions. `read_fc` returns a counter with
        // `pal = false` (NTSC), which is correct for NTSC/Dendy.
        self.frame_counter.pal = matches!(self.region, Region::Pal);
        self.blip = read_blip(&mut r)?;

        self.apu_phase = r.bool()?;
        self.cpu_cycle = r.u64()?;
        self.pending_dmc_dma = r.bool()?;
        self.dmc_dma_addr = r.u16()?;
        self.sample_rate = r.u32()?;
        self.dmc_dma_delay = r.u8()?;
        self.dmc_dma_is_load = r.bool()?;
        self.pending_dmc_abort = r.bool()?;
        self.dmc_abort_delay = r.u8()?;
        self.dmc_dma_short = r.bool()?;
        self.defer_dmc_reload_once = r.bool()?;
        self.dmc_dma_cooldown = r.u8()?;
        self.dmc_reload_suppress_outputs = r.u8()?;

        // === W3-Stage-4 (2026-06-10) tail ===
        // See the matching block in [`Apu::snapshot`].
        self.put_cycle = r.bool()?;
        self.parity_seed = r.u64()?;
        self.cannot_run_dmc_dma = r.u8()?;
        self.dmc_reenable_period_block = r.bool()?;
        self.subpos_arm_countdown = r.u8()?;
        self.dmc_need_halt = r.bool()?;
        self.dmc_need_dummy_read = r.bool()?;
        self.pending_dmc_dma_next = r.bool()?;
        self.dmc_delayed_4015 = r.u8()?;
        self.dmc_delayed_status = r.bool()?;
        self.dmc_status_applied = r.bool()?;
        self.dmc_set_implicit_abort = r.bool()?;
        self.dmc_implicit_abort = r.bool()?;
        self.dmc_edge_arm_suppress = r.bool()?;

        // === v4 scheduled warm-reset `$4017` re-write ===
        // See the matching block in [`Apu::snapshot`].
        self.reset_4017_delay = r.u8()?;
        self.reset_4017_value = r.u8()?;

        // Every field is fixed-size, so the blob must end here.
        if r.pos != data.len() {
            return Err(ApuSnapshotError::TrailingBytes(data.len() - r.pos));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blip::CPU_HZ_NTSC;

    /// Locate a field's bytes in the blob by DIFFERENCE: snapshot two APUs that
    /// differ only in that field, and the differing bytes are the field. This
    /// keeps the IMP-02 tests independent of the schema's byte offsets, which
    /// a hard-coded index would silently go stale against.
    fn field_span(set_a: impl Fn(&mut Apu), set_b: impl Fn(&mut Apu)) -> (Vec<u8>, usize, usize) {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        let mut b = Apu::new(Region::Ntsc, 44_100);
        set_a(&mut a);
        set_b(&mut b);
        let (sa, sb) = (a.snapshot(), b.snapshot());
        assert_eq!(sa.len(), sb.len());
        let first = (0..sa.len())
            .find(|&i| sa[i] != sb[i])
            .expect("fields differ");
        let last = (0..sa.len()).rfind(|&i| sa[i] != sb[i]).unwrap();
        (sb, first, last + 1)
    }

    /// Corrupt one register-width `u8` field and assert a typed rejection
    /// naming it, while its largest legal value still restores.
    fn assert_u8_bounded(name: &'static str, max: u8, set: impl Fn(&mut Apu, u8)) {
        let lo = max.saturating_sub(1);
        let (blob, at, end) = field_span(|a| set(a, lo), |a| set(a, max));
        assert_eq!(end - at, 1, "{name} is one byte");
        Apu::new(Region::Ntsc, 44_100)
            .restore(&blob)
            .unwrap_or_else(|e| panic!("{name} = {max} is legal and must load: {e}"));
        for bad in [max + 1, 0x80u8.max(max + 1), 0xFF] {
            let mut b = blob.clone();
            b[at] = bad;
            match Apu::new(Region::Ntsc, 44_100).restore(&b) {
                Err(ApuSnapshotError::FieldOutOfRange {
                    field,
                    value,
                    max: m,
                }) => {
                    assert_eq!((field, value, m), (name, bad, max));
                }
                Err(e) => panic!("{name} = {bad}: wrong error {e}"),
                Ok(()) => panic!("{name} = {bad}: restore ACCEPTED an out-of-range value"),
            }
        }
    }

    #[test]
    fn every_register_width_field_is_bounded_on_restore() {
        // Core audit IMP-02. One call per bound; each names the field its
        // error must carry, so a check that is removed or attached to the
        // wrong field fails here.
        assert_u8_bounded("pulse.duty", 3, |a, v| a.pulse1.duty = v);
        assert_u8_bounded("pulse.step", 7, |a, v| a.pulse1.step = v);
        assert_u8_bounded("pulse.sweep_period", 7, |a, v| a.pulse2.sweep_period = v);
        assert_u8_bounded("pulse.sweep_shift", 7, |a, v| a.pulse1.sweep_shift = v);
        assert_u8_bounded("pulse.sweep_divider", 7, |a, v| a.pulse2.sweep_divider = v);
        assert_u8_bounded("envelope.volume_or_period", 15, |a, v| {
            a.pulse1.envelope.volume_or_period = v;
        });
        assert_u8_bounded("envelope.divider", 15, |a, v| a.noise.envelope.divider = v);
        assert_u8_bounded("envelope.decay", 15, |a, v| a.pulse2.envelope.decay = v);
        assert_u8_bounded("triangle.step", 31, |a, v| a.triangle.step = v);
        assert_u8_bounded("dmc.rate_index", 15, |a, v| a.dmc.rate_index = v);
        assert_u8_bounded("dmc.bits_remaining", 8, |a, v| a.dmc.bits_remaining = v);
        assert_u8_bounded("dmc.dac", 127, |a, v| a.dmc.dac = v);
    }

    /// Overwrite a multi-byte field located by difference and expect a typed
    /// resampler rejection naming `name`.
    fn assert_float_rejected(name: &'static str, span: (Vec<u8>, usize, usize), bad: &[u8]) {
        let (mut b, at, end) = span;
        assert_eq!(
            end - at,
            bad.len(),
            "{name}: located span is the value's width"
        );
        b[at..end].copy_from_slice(bad);
        match Apu::new(Region::Ntsc, 44_100).restore(&b) {
            Err(ApuSnapshotError::InvalidResampler(f)) => assert_eq!(f, name),
            Err(e) => panic!("{name}: wrong error {e}"),
            Ok(()) => panic!("{name}: restore ACCEPTED {bad:02x?}"),
        }
    }

    #[test]
    fn resampler_fields_that_would_hang_or_poison_audio_are_rejected() {
        // Core audit IMP-02: a zero/NaN/huge rate ratio or an out-of-range
        // phase cannot panic -- it hangs `add_sample`'s `while phase >= 1.0`
        // loop, or fills the output with NaN. Each field is located by
        // difference (its partner value is the bitwise complement, so every
        // byte differs and the located span is the full width), then replaced
        // with a hostile value.
        let rate = |a: &mut Apu, v: u32| a.blip.sample_rate = v;
        let sr = || field_span(|a| rate(a, 44_100), |a| rate(a, !44_100));
        assert_float_rejected("blip.sample_rate", sr(), &0u32.to_le_bytes());
        // A finite but enormous host rate: 4 billion outputs per ~1.8 M CPU
        // cycles is > 1 per cycle, the hang the ratio bound exists for.
        assert_float_rejected("blip.sample_rate/cpu_rate", sr(), &u32::MAX.to_le_bytes());

        let cpu = |a: &mut Apu, v: f64| a.blip.cpu_rate = v;
        let cr = || {
            field_span(
                |a| cpu(a, CPU_HZ_NTSC),
                |a| cpu(a, f64::from_bits(!CPU_HZ_NTSC.to_bits())),
            )
        };
        for bad in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            assert_float_rejected("blip.cpu_rate", cr(), &bad.to_le_bytes());
        }
        assert_float_rejected("blip.sample_rate/cpu_rate", cr(), &1.0e-9f64.to_le_bytes());

        let ph = |a: &mut Apu, v: f64| a.blip.phase = v;
        let pp = || field_span(|a| ph(a, 0.0), |a| ph(a, f64::from_bits(!0)));
        for bad in [1.0, 1.0e300, -0.25, f64::NAN] {
            assert_float_rejected("blip.phase", pp(), &bad.to_le_bytes());
        }

        let hv = |a: &mut Apu, v: f32| a.blip.held_value = v;
        let hp = || field_span(|a| hv(a, 0.0), |a| hv(a, f32::from_bits(!0)));
        assert_float_rejected("blip.held_value", hp(), &f32::NAN.to_le_bytes());
        // NC-09 (v2.9.9): finite but outside anything the mixer produces.
        for bad in [f32::MAX, -2.05e38, 17.0] {
            assert_float_rejected("blip.held_value", hp(), &bad.to_le_bytes());
        }
        let po = |a: &mut Apu, v: f32| a.blip.filter.hp1.prev_out = v;
        let pop = || field_span(|a| po(a, 0.0), |a| po(a, f32::from_bits(!0)));
        let pi = |a: &mut Apu, v: f32| a.blip.filter.hp1.prev_in = v;
        let pip = || field_span(|a| pi(a, 0.0), |a| pi(a, f32::from_bits(!0)));
        for bad in [f32::MAX, -2.05e38, -2048.0] {
            assert_float_rejected("filter.prev_out", pop(), &bad.to_le_bytes());
            assert_float_rejected("filter.prev_in", pip(), &bad.to_le_bytes());
        }

        let co = |a: &mut Apu, v: f32| a.blip.filter.lp.coeff = v;
        let cp = || field_span(|a| co(a, 0.5), |a| co(a, f32::from_bits(!0.5f32.to_bits())));
        assert_float_rejected("filter.coeff", cp(), &f32::INFINITY.to_le_bytes());
        // Finite but out of range: a high-pass gain above 1 diverges.
        for bad in [1.5f32, -0.25] {
            assert_float_rejected("filter.coeff", cp(), &bad.to_le_bytes());
        }
    }

    /// NL-12 (v2.9.9 libretro re-audit): a save/load round trip resumes the
    /// exact sample stream and the exact serialized state. Before v5 the
    /// restored resampler started cold: fewer samples on the next drain, a
    /// different level, and filter bytes that never re-converged.
    #[test]
    fn a_restore_resumes_the_exact_audio_stream() {
        fn program(a: &mut Apu) {
            a.write_register(0x4015, 0x0F);
            a.write_register(0x4000, 0xBF);
            a.write_register(0x4002, 0x40);
            a.write_register(0x4003, 0x01);
            a.write_register(0x4008, 0xFF);
            a.write_register(0x400A, 0x80);
            a.write_register(0x400B, 0x02);
        }
        let mut straight = Apu::new(Region::Ntsc, 44_100);
        program(&mut straight);
        for _ in 0..20_000 {
            straight.tick();
        }
        // At a frame boundary, as every host snapshots: output drained.
        let _ = straight.blip.drain_all();
        let blob = straight.snapshot();
        let mut restored = Apu::new(Region::Ntsc, 48_000);
        restored.restore(&blob).unwrap();
        for _ in 0..40_000 {
            straight.tick();
            restored.tick();
        }
        let (a, b) = (straight.blip.drain_all(), restored.blip.drain_all());
        assert_eq!(a.len(), b.len(), "same number of samples");
        assert!(
            a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()),
            "bit-identical samples"
        );
        assert_eq!(straight.snapshot(), restored.snapshot(), "same state");
    }

    /// A delta window whose sum leaves the integrator far from the held
    /// input is not a state the resampler produces, and would leave a DC
    /// offset the bounds could not contain; it is refused.
    #[test]
    fn a_window_inconsistent_with_the_held_value_is_refused() {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        a.write_register(0x4015, 0x01);
        a.write_register(0x4000, 0xBF);
        a.write_register(0x4002, 0x40);
        a.write_register(0x4003, 0x01);
        for _ in 0..5_000 {
            a.tick();
        }
        let (head, primed, integrator, mut window) = a.blip.live_state();
        window[0] += 3.0;
        a.blip.set_live_state(head, primed, integrator, &window);
        assert!(matches!(
            Apu::new(Region::Ntsc, 44_100).restore(&a.snapshot()),
            Err(ApuSnapshotError::InvalidResampler("blip.delta_window"))
        ));
    }

    /// NC-09 (v2.9.9 re-audit): the machine's own snapshot must always load
    /// back. Before the bound, a finite `prev_out` near `f32::MAX` was
    /// accepted, overflowed within a frame, and the next snapshot carried a
    /// non-finite value that `restore` then refused.
    #[test]
    fn a_huge_filter_value_is_refused_so_the_next_snapshot_still_loads() {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        a.write_register(0x4015, 0x0F);
        a.write_register(0x4000, 0xBF);
        a.write_register(0x4002, 0x40);
        a.write_register(0x4003, 0x01);
        a.blip.filter.hp1.prev_out = f32::MAX;
        let poisoned = a.snapshot();
        let mut b = Apu::new(Region::Ntsc, 44_100);
        assert!(b.restore(&poisoned).is_err(), "the huge value is refused");
        // And the bound admits everything a real run produces.
        let mut c = Apu::new(Region::Ntsc, 44_100);
        c.write_register(0x4015, 0x0F);
        c.write_register(0x4000, 0xBF);
        c.write_register(0x4002, 0x40);
        c.write_register(0x4003, 0x01);
        for _ in 0..30_000 {
            c.tick();
        }
        let mut d = Apu::new(Region::Ntsc, 44_100);
        d.restore(&c.snapshot()).expect("a real state loads");
    }

    #[test]
    fn snapshot_round_trip_on_fresh_apu() {
        let a = Apu::new(Region::Ntsc, 44_100);
        let blob = a.snapshot();
        let mut b = Apu::new(Region::Pal, 48_000);
        b.restore(&blob).unwrap();
        assert_eq!(b.region, Region::Ntsc);
        assert_eq!(b.sample_rate, 44_100);
    }

    #[test]
    fn snapshot_after_some_ticks_round_trips() {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        a.write_register(0x4000, 0xBE);
        a.write_register(0x4002, 0x42);
        a.write_register(0x4015, 0x0F);
        for _ in 0..100 {
            a.tick();
        }
        let blob = a.snapshot();
        let mut b = Apu::new(Region::Ntsc, 44_100);
        b.restore(&blob).unwrap();
        // Spot-check critical fields.
        assert_eq!(b.cpu_cycle, a.cpu_cycle);
        assert_eq!(b.pulse1.timer_period, a.pulse1.timer_period);
        assert_eq!(b.pulse1.length.count, a.pulse1.length.count);
        assert_eq!(b.frame_counter.cycle, a.frame_counter.cycle);
    }

    #[test]
    fn snapshot_rejects_bad_version() {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        let err = a.restore(&[0xFF; 4]).unwrap_err();
        assert!(matches!(err, ApuSnapshotError::UnsupportedVersion(0xFF)));
    }

    #[test]
    fn snapshot_is_deterministic() {
        let a = Apu::new(Region::Ntsc, 44_100);
        assert_eq!(a.snapshot(), a.snapshot());
    }

    #[test]
    fn stage4_tail_round_trips_parity_and_dma_state() {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        a.put_cycle = true;
        a.cannot_run_dmc_dma = 2;
        a.dmc_reenable_period_block = true;
        a.subpos_arm_countdown = 3;
        a.dmc_need_halt = true;
        a.dmc_need_dummy_read = true;
        {
            a.dmc_delayed_4015 = 4;
            a.dmc_delayed_status = true;
            a.dmc_status_applied = true;
            a.dmc_edge_arm_suppress = true;
        }
        let blob = a.snapshot();
        let mut b = Apu::new(Region::Ntsc, 44_100);
        b.restore(&blob).unwrap();
        assert!(b.put_cycle);
        assert_eq!(b.cannot_run_dmc_dma, 2);
        assert!(b.dmc_reenable_period_block);
        assert_eq!(b.subpos_arm_countdown, 3);
        assert!(b.dmc_need_halt);
        assert!(b.dmc_need_dummy_read);
        {
            assert_eq!(b.dmc_delayed_4015, 4);
            assert!(b.dmc_delayed_status);
            assert!(b.dmc_status_applied);
            assert!(b.dmc_edge_arm_suppress);
        }
    }

    /// v2.9.8 (ADR 0042): older versions and blobs that end early are
    /// refused. Until then v1-v3 blobs were migrated, and a blob that ended
    /// before the DMC-DMA bytes or the W3-Stage-4 tail loaded with defaults.
    #[test]
    fn older_versions_and_short_blobs_are_refused() {
        let a = Apu::new(Region::Ntsc, 44_100);
        let blob = a.snapshot();
        for v in 1..APU_SNAPSHOT_VERSION {
            let mut old = blob.clone();
            old[0] = v;
            assert!(matches!(
                Apu::new(Region::Ntsc, 44_100).restore(&old),
                Err(ApuSnapshotError::UnsupportedVersion(got)) if got == v
            ));
        }
        // The pre-Stage-4 shape: the current blob minus the v4 tail (2 bytes)
        // and the Stage-4 tail (21 bytes), at the current version.
        let short = &blob[..blob.len() - (2 + 21)];
        assert!(matches!(
            Apu::new(Region::Ntsc, 44_100).restore(short),
            Err(ApuSnapshotError::Truncated(_))
        ));
        // Every shorter length is refused, and so is a trailing byte.
        for len in 1..blob.len() {
            assert!(
                Apu::new(Region::Ntsc, 44_100)
                    .restore(&blob[..len])
                    .is_err(),
                "an APU blob cut to {len} bytes loaded"
            );
        }
        let mut long = blob.clone();
        long.push(0);
        // Named as what it is: "truncated" for a blob that is too LONG sent
        // the reader looking for missing bytes (CodeRabbit on #580).
        assert!(matches!(
            Apu::new(Region::Ntsc, 44_100).restore(&long),
            Err(ApuSnapshotError::TrailingBytes(n)) if n == 1
        ));
    }

    #[test]
    fn v4_round_trips_the_scheduled_reset_4017_rewrite() {
        // The countdown and its payload are live for the 2 CPU cycles between
        // `Apu::reset` arming them and `tick_with_external` firing the write.
        let mut a = Apu::new(Region::Ntsc, 44_100);
        a.reset_4017_delay = 2;
        a.reset_4017_value = 0x80;
        let blob = a.snapshot();
        assert_eq!(
            blob[0], APU_SNAPSHOT_VERSION,
            "blob carries current version"
        );

        let mut b = Apu::new(Region::Pal, 48_000);
        b.restore(&blob).unwrap();
        assert_eq!(b.reset_4017_delay, 2);
        assert_eq!(b.reset_4017_value, 0x80);
    }

    #[test]
    fn a_reset_survives_a_snapshot_restore_taken_mid_countdown() {
        // The behavioural pin, not just a field round trip: a save/restore
        // landing inside the arming window must still deliver the `$4017`
        // re-write on the same cycle a straight run would. Before the v4 tail
        // the restored APU dropped it, so the frame counter kept the sequencer
        // phase the re-write exists to reset.
        let mut plain = Apu::new(Region::Ntsc, 44_100);
        plain.write_register(0x4017, 0x80); // mode 5-step, so the re-write is observable
        plain.reset();
        assert_eq!(plain.reset_4017_delay, 2, "reset arms the countdown");

        // Round-trip through a snapshot taken with the countdown live.
        let mut restored = Apu::new(Region::Pal, 48_000);
        restored.restore(&plain.snapshot()).unwrap();

        // Advance far enough for the whole chain to play out: the countdown
        // fires at t=2, `FrameCounter::write` then schedules its own 3/4-cycle
        // maturation, and only when THAT lands does the sequencer restart. Ten
        // cycles clears it with margin. (Four does not — the write has fired
        // but its effect has not yet matured, and both sides still look alike.)
        for _ in 0..10 {
            plain.tick_with_external(0.0);
            restored.tick_with_external(0.0);
        }
        assert_eq!(
            restored.reset_4017_delay, plain.reset_4017_delay,
            "countdown diverged across the round trip"
        );
        // `frame_counter.cycle` is the discriminating observable. `mode` is not:
        // `reset_rewrite_4017` retains bit 7, so the re-write always restores the
        // mode already in effect and the field reads the same either way.
        // Without the v4 tail the restored APU never issues the write, so its
        // sequencer keeps counting instead of restarting.
        assert_eq!(
            restored.frame_counter.cycle, plain.frame_counter.cycle,
            "the scheduled $4017 re-write did not survive the round trip — the \
             restored sequencer never restarted"
        );
        assert_eq!(
            restored.frame_counter.reset_in, plain.frame_counter.reset_in,
            "frame-counter reset maturation diverged across the round trip"
        );
    }

    #[test]
    fn fresh_apu_snapshot_has_zero_irq_clear_schedule() {
        let a = Apu::new(Region::Ntsc, 44_100);
        assert_eq!(a.frame_counter.irq_flag_clear_cycle, 0);
        let blob = a.snapshot();
        let mut b = Apu::new(Region::Pal, 48_000);
        b.restore(&blob).unwrap();
        assert_eq!(b.frame_counter.irq_flag_clear_cycle, 0);
    }
}

//! Save-state encoding / decoding for the [`Ppu`].
//!
//! Hand-rolled little-endian binary so the crate stays free of `serde` and
//! `bincode`. The container format used by the bus to wrap this blob into
//! a tagged section lives in `rustynes_core::save_state`.
//!
//! Schema version 1 layout (all little-endian, top-down):
//!
//! - `version` u8
//! - region tag u8 (0=NTSC, 1=PAL, 2=Dendy)
//! - `ctrl` u8 / `mask` u8 / `mask_for_skip_check` u8 / `mask_skip_pipe1` u8 / `status` u8
//! - `oam_addr` u8 / `data_buffer` u8
//! - loopy: `v` u16 / `t` u16 / `x` u8 / `w` bool
//! - 2 KiB CIRAM (raw bytes)
//! - 256 B OAM (raw bytes)
//! - 32 B secondary OAM (raw bytes)
//! - 32 B palette RAM (raw bytes)
//! - `open_bus` u8 / 3× `open_bus_decay[i]` u32
//! - `nmi_line` / `suppress_vbl_this_frame` / `last_a12_level` u8
//! - `dot` u16 / `scanline` i16 / `frame` u64 / `frame_complete` bool
//! - `post_reset_mask_remaining` u32
//! - BG latches: `nt_latch` u8 / `at_latch` u8 / `bg_lo_latch` u8 / `bg_hi_latch` u8
//! - BG shifts (v2): `bg_shift_lo` u16 / `bg_shift_hi` u16 / `at_shift_lo` u16 /
//!   `at_shift_hi` u16. (v1 stored `at_shift_*` as u8 + two 1-bit feed bytes;
//!   v1 blobs are upconverted on read.)
//! - `ex_attr_latch` (presence u8 + `palette` u8 + `chr_bank` u16)
//! - `bg_split_latch` (presence u8 + `nt_addr` u16 + `at_addr` u16 + `fine_y` u8 + `chr_bank` u8)
//! - sprite arrays: 8× `shift_lo` / `shift_hi` / `attr` / `x` / `spr_count` u8 / `spr_zero_in_line` bool
//! - `256*240*4` framebuffer bytes

use alloc::vec::Vec;
use thiserror::Error;

use crate::bus::{BgSplitState, ExAttribute};
use crate::ppu::{FRAMEBUFFER_LEN, Ppu, PpuRegion};
use crate::registers::{PpuCtrl, PpuMask, PpuStatus};

/// Schema version for the PPU snapshot blob.
///
/// - v1: 8-bit `at_shift_lo`/`at_shift_hi` + 1-bit `at_feed_lo`/`at_feed_hi`.
/// - v2: 16-bit `at_shift_lo`/`at_shift_hi` (lockstep with the pattern
///   shifters); the feed fields are gone. v1 blobs are still read.
/// - v3 (W3-Stage-4 promotion, 2026-06-10): appends the
///   `mc-ppu-2007-render-buffer` rendering-time `$2007` PPUDATA
///   state-machine tail — `render_data_bus`, `ppudata_sm_countdown`,
///   `ppudata_v_inc_pending`, the raw (pre-h-flip) sprite pattern fetch
///   bytes, and the slot-0 garbage-NT ALE latch. Written unconditionally
///   (zeros when the feature is off) so the layout is identical across
///   feature builds; v1/v2 blobs upconvert with the tail at the inactive
///   defaults (the state the old clear-on-restore assumption imposed).
/// - v4 (v1.7.0 F3, 2026-06-18): appends the `extra_lines_remaining`
///   countdown for the in-flight PPU extra-scanlines overclock insertion.
///   At the default `extra_scanlines == 0` this is always `0`, so the
///   blob merely gains a zero `u16` and restore is behaviourally identical
///   to v3; a non-default countdown taken mid-insertion now round-trips
///   instead of restoring as `0` (which desynced). v1/v2/v3 blobs upconvert
///   with `extra_lines_remaining = 0` (no insertion in flight).
/// - v5 (v2.0.3, ADR 0030): appends the 2-cycle-ALE fetch model's in-flight
///   multiplexed-bus / octal-latch state — `octal_latch`, `address_bus`,
///   `ale_armed`, `pattern_latch_stale`, and the delayed-`CopyV` `copy_v_delay`
///   countdown. These fields carry a background fetch's ALE→read state and the
///   two modeled `$2006`/`$2007` corruption one-shots; a mid-render save/restore
///   (netplay rollback checkpoints, TAS/save-states) that landed with any of
///   them live restored them to the wrong value and desynced the re-simulated
///   framebuffer — exactly the class of bug the v3/v4 tails already fixed for
///   the `$2007` state machine and the overclock countdown. This is an ADDITIVE
///   save-state format change (a `.rns` gains the tail): v1..=4 blobs upconvert
///   with all five at their inactive defaults (`0`/`false`), i.e. the
///   "no fetch in flight" rest state, which is correct for any pre-v5 save taken
///   at rest. Not a *load-break* — a pre-v5 `.rns` still restores.
/// - v6 (v2.1.1, "Fathom"): appends the per-sprite shifter-halt flags
///   (`spr_halted[8]`), the 1-dot-delayed rendering gate
///   (`prev_rendering_enabled` / `rendering_enabled_delayed`), and the
///   OAM-corruption arming state (`oam_corruption_pending`,
///   `oam_corruption_index`, `oam_corruption_disabled`,
///   `oam_corruption_disabled_instant`). These fields were previously not
///   serialized; a frontend run-ahead `snapshot`/`restore` round-trip
///   landing with any of them live (mid-frame sprite-0 split, OAM
///   corruption arming) would restore stale constructor values and corrupt
///   rendering — the Wizards & Warriors half-blank playfield / stalled
///   audio class. v1..=5 blobs upconvert with `spr_halted = [true; 8]`
///   (halted = power-on default) and all others at `false`/`0`. Not a
///   *load-break* — a pre-v6 `.rns` still restores.
pub const PPU_SNAPSHOT_VERSION: u8 = 6;

const CIRAM_LEN: usize = 0x800;
const OAM_LEN: usize = 0x100;
const SEC_OAM_LEN: usize = 32;
const PAL_LEN: usize = 32;

/// Errors returned by [`Ppu::restore`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PpuSnapshotError {
    /// Blob is too short for the version-1 schema.
    #[error("PPU snapshot truncated at offset {0}")]
    Truncated(usize),
    /// The blob's version byte is not understood by this build.
    #[error("PPU snapshot unsupported version {0}")]
    UnsupportedVersion(u8),
    /// Region tag was not one of `0` (NTSC), `1` (PAL), `2` (Dendy).
    #[error("PPU snapshot has invalid region tag {0}")]
    InvalidRegion(u8),
    /// Optional struct presence byte was something other than 0 or 1.
    #[error("PPU snapshot has invalid optional presence byte {0}")]
    InvalidPresence(u8),
}

const fn region_to_u8(r: PpuRegion) -> u8 {
    match r {
        PpuRegion::Ntsc => 0,
        PpuRegion::Pal => 1,
        PpuRegion::Dendy => 2,
    }
}

const fn region_from_u8(v: u8) -> Result<PpuRegion, PpuSnapshotError> {
    match v {
        0 => Ok(PpuRegion::Ntsc),
        1 => Ok(PpuRegion::Pal),
        2 => Ok(PpuRegion::Dendy),
        other => Err(PpuSnapshotError::InvalidRegion(other)),
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
    fn i16(&mut self, v: i16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, v: &[u8]) {
        self.buf.extend_from_slice(v);
    }
}

struct R<'a> {
    src: &'a [u8],
    pos: usize,
}
impl R<'_> {
    const fn need(&self, n: usize) -> Result<(), PpuSnapshotError> {
        if self.src.len() - self.pos < n {
            return Err(PpuSnapshotError::Truncated(self.pos));
        }
        Ok(())
    }
    fn u8(&mut self) -> Result<u8, PpuSnapshotError> {
        self.need(1)?;
        let v = self.src[self.pos];
        self.pos += 1;
        Ok(v)
    }
    fn u16(&mut self) -> Result<u16, PpuSnapshotError> {
        self.need(2)?;
        let v = u16::from_le_bytes([self.src[self.pos], self.src[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }
    fn u32(&mut self) -> Result<u32, PpuSnapshotError> {
        self.need(4)?;
        let mut a = [0u8; 4];
        a.copy_from_slice(&self.src[self.pos..self.pos + 4]);
        self.pos += 4;
        Ok(u32::from_le_bytes(a))
    }
    fn u64(&mut self) -> Result<u64, PpuSnapshotError> {
        self.need(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(&self.src[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(u64::from_le_bytes(a))
    }
    fn i16(&mut self) -> Result<i16, PpuSnapshotError> {
        self.need(2)?;
        let v = i16::from_le_bytes([self.src[self.pos], self.src[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }
    fn bytes_into(&mut self, dst: &mut [u8]) -> Result<(), PpuSnapshotError> {
        self.need(dst.len())?;
        dst.copy_from_slice(&self.src[self.pos..self.pos + dst.len()]);
        self.pos += dst.len();
        Ok(())
    }
}

impl Ppu {
    /// Encode the PPU's mutable state into a versioned binary blob.
    #[must_use]
    pub fn snapshot(&self) -> Vec<u8> {
        // Capacity hint: ~256 KiB framebuffer dominates.
        let mut w = W {
            buf: Vec::with_capacity(FRAMEBUFFER_LEN + 4096),
        };
        w.u8(PPU_SNAPSHOT_VERSION);
        w.u8(region_to_u8(self.region));

        w.u8(self.ctrl.bits());
        w.u8(self.mask.bits());
        w.u8(self.mask_for_skip_check.bits());
        w.u8(self.mask_skip_pipe1.bits());
        w.u8(self.status.bits());

        w.u8(self.oam_addr);
        w.u8(self.data_buffer);
        w.u16(self.v);
        w.u16(self.t);
        w.u8(self.x);
        w.u8(u8::from(self.w));

        // Memory blocks at fixed sizes — no length prefix needed (versioned schema).
        w.bytes(&self.ciram);
        w.bytes(&self.oam);
        w.bytes(&self.secondary_oam);
        w.bytes(&self.palette_ram);

        w.u8(self.open_bus);
        for d in self.open_bus_decay {
            w.u32(d);
        }

        w.u8(u8::from(self.nmi_line));
        w.u8(u8::from(self.suppress_vbl_this_frame));
        w.u8(u8::from(self.last_a12_level));

        w.u16(self.dot);
        w.i16(self.scanline);
        w.u64(self.frame);
        w.u8(u8::from(self.frame_complete));

        w.u32(self.post_reset_mask_remaining);

        w.u8(self.nt_latch);
        w.u8(self.at_latch);
        w.u8(self.bg_lo_latch);
        w.u8(self.bg_hi_latch);
        w.u16(self.bg_shift_lo);
        w.u16(self.bg_shift_hi);
        // v2: attribute shift registers widened to 16-bit (lockstep with
        // the pattern shifters); the v1 1-bit `at_feed_*` fields are gone.
        w.u16(self.at_shift_lo);
        w.u16(self.at_shift_hi);

        if let Some(ex) = self.ex_attr_latch {
            w.u8(1);
            w.u8(ex.palette);
            w.u16(ex.chr_bank);
        } else {
            w.u8(0);
            w.u8(0);
            w.u16(0);
        }
        if let Some(s) = self.bg_split_latch {
            w.u8(1);
            w.u16(s.nt_addr);
            w.u16(s.at_addr);
            w.u8(s.fine_y);
            w.u8(s.chr_bank);
        } else {
            w.u8(0);
            w.u16(0);
            w.u16(0);
            w.u8(0);
            w.u8(0);
        }

        w.bytes(&self.spr_shift_lo);
        w.bytes(&self.spr_shift_hi);
        w.bytes(&self.spr_attr);
        w.bytes(&self.spr_x);
        w.u8(self.spr_count);
        w.u8(u8::from(self.spr_zero_in_line));

        w.bytes(&self.framebuffer);

        // v3 (W3-Stage-4): the `mc-ppu-2007-render-buffer` PPUDATA
        // state-machine tail. Written unconditionally (zeros when the
        // feature is off) so the blob layout is feature-independent.
        {
            w.u8(self.render_data_bus);
            w.u8(self.ppudata_sm_countdown);
            w.u8(u8::from(self.ppudata_v_inc_pending));
            w.bytes(&self.spr_fetch_lo_raw);
            w.bytes(&self.spr_fetch_hi_raw);
            w.u16(self.ppudata_spr0_nt_addr);
        }
        // v3 (W3-Stage-4): the `mc-ppu-subpos` BG-reload freeze (the
        // `$2001`-write-commit delay that injects the BG serial-in '1's).
        // `bg_reload_render` re-syncs from the live mask when settled, but an
        // in-flight `mask_write_delay` countdown can straddle an instruction
        // boundary — serialize both so a restored state resumes the freeze
        // exactly.
        {
            w.u8(u8::from(self.bg_reload_render));
            w.u8(self.mask_write_delay);
        }

        // v4 (v1.7.0 F3): the in-flight extra-scanlines overclock countdown.
        // Always `0` at the default `extra_scanlines == 0`, so this is a
        // zero `u16` in the stock build (no behavioural change). The
        // configured count itself (`extra_scanlines`) stays a frontend knob
        // re-applied on restore, like `region` / `active_palette`.
        w.u16(self.extra_lines_remaining);

        // v5 (v2.0.3, ADR 0030): the 2-cycle-ALE fetch model's in-flight
        // multiplexed-bus / octal-latch state. All at rest (`0`/`false`) at a
        // clean fetch boundary, but a mid-render checkpoint (netplay rollback)
        // can land with `copy_v_delay`/`pattern_latch_stale`/`ale_armed` live —
        // serialize them (plus the latch + bus they splice through) so the
        // re-simulated frame is byte-identical to the forward run.
        {
            w.u8(self.octal_latch);
            w.u16(self.address_bus);
            w.u8(u8::from(self.ale_armed));
            w.u8(u8::from(self.pattern_latch_stale));
            w.u8(self.copy_v_delay);
        }

        // v6 (W&W run-ahead fix): the per-sprite shifter HALT state. Set by the
        // v2.0 sprite-shifter-counter model (a loaded-but-halted slot draws
        // immediately on re-enable), it persists across the frame boundary and
        // governs whether each of the 8 loaded sprites emits — so it is genuine
        // rendering state. It was previously unserialized, so a per-frame
        // save/restore (run-ahead, netplay rollback) drifted it: for a game that
        // toggles rendering mid-frame (Wizards & Warriors' sprite-0 status-bar
        // split), the drift accumulated into dropped/blinking sprites and a
        // half-rendered playfield. `true` (halted) is the power-on default.
        for h in &self.spr_halted {
            w.u8(u8::from(*h));
        }
        // v6 (cont.) — remaining unserialized cross-frame render state.
        w.u8(u8::from(self.prev_rendering_enabled));
        w.u8(u8::from(self.rendering_enabled_delayed));
        w.u8(u8::from(self.oam_corruption_pending));
        w.u8(self.oam_corruption_index);
        w.u8(u8::from(self.oam_corruption_disabled));
        w.u8(u8::from(self.oam_corruption_disabled_instant));

        w.buf
    }

    /// Decode a previously [`Ppu::snapshot`]ed blob.
    ///
    /// # Errors
    ///
    /// Returns [`PpuSnapshotError`] on a malformed blob.
    // A flat, linear field-by-field decoder with per-version tail branches (v1
    // through v6); splitting it would only scatter the schema that is clearest
    // read top-to-bottom against the matching `snapshot` writer.
    #[allow(clippy::too_many_lines)]
    pub fn restore(&mut self, data: &[u8]) -> Result<(), PpuSnapshotError> {
        // A valid v1..=6 snapshot always contains these fixed-size blocks (the
        // framebuffer, read unconditionally below at every version, dominates);
        // the version-specific tails only add to this. This is a *conservative
        // lower bound* — it deliberately omits the ~40 scalar register/latch
        // bytes and the spr shift arrays, so it can never reject a valid blob,
        // yet it rejects a clearly-truncated one BEFORE the version byte is read
        // (so short/garbled input reports `Truncated`, not a misleading
        // `UnsupportedVersion` on whatever byte sits at offset 0). `Truncated(0)`
        // matches the offset semantics `R::need` uses elsewhere (the position at
        // which a read ran out) — here, nothing valid was read.
        const MIN_SNAPSHOT_SIZE: usize =
            1 + CIRAM_LEN + OAM_LEN + SEC_OAM_LEN + PAL_LEN + FRAMEBUFFER_LEN;
        if data.len() < MIN_SNAPSHOT_SIZE {
            return Err(PpuSnapshotError::Truncated(0));
        }
        let mut r = R { src: data, pos: 0 };
        let version = r.u8()?;
        if !matches!(version, 1..=6) {
            return Err(PpuSnapshotError::UnsupportedVersion(version));
        }
        self.region = region_from_u8(r.u8()?)?;

        self.ctrl = PpuCtrl::from_bits_truncate(r.u8()?);
        self.mask = PpuMask::from_bits_truncate(r.u8()?);
        self.mask_for_skip_check = PpuMask::from_bits_truncate(r.u8()?);
        self.mask_skip_pipe1 = PpuMask::from_bits_truncate(r.u8()?);
        self.status = PpuStatus::from_bits_truncate(r.u8()?);

        self.oam_addr = r.u8()?;
        self.data_buffer = r.u8()?;
        self.v = r.u16()?;
        self.t = r.u16()?;
        self.x = r.u8()?;
        self.w = r.u8()? != 0;

        r.bytes_into(&mut self.ciram)?;
        r.bytes_into(&mut self.oam)?;
        r.bytes_into(&mut self.secondary_oam)?;
        r.bytes_into(&mut self.palette_ram)?;

        self.open_bus = r.u8()?;
        for d in &mut self.open_bus_decay {
            *d = r.u32()?;
        }

        self.nmi_line = r.u8()? != 0;
        self.suppress_vbl_this_frame = r.u8()? != 0;
        self.last_a12_level = r.u8()? != 0;

        self.dot = r.u16()?;
        self.scanline = r.i16()?;
        self.frame = r.u64()?;
        self.frame_complete = r.u8()? != 0;

        self.post_reset_mask_remaining = r.u32()?;

        self.nt_latch = r.u8()?;
        self.at_latch = r.u8()?;
        self.bg_lo_latch = r.u8()?;
        self.bg_hi_latch = r.u8()?;
        self.bg_shift_lo = r.u16()?;
        self.bg_shift_hi = r.u16()?;
        if version >= 2 {
            self.at_shift_lo = r.u16()?;
            self.at_shift_hi = r.u16()?;
        } else {
            // v1: 8-bit attribute shift registers + a 1-bit feed each.
            // Promote into the v2 16-bit registers (low byte = the v1
            // 8-bit value; the high byte was always implicitly zero in
            // the v1 model). The transient feed bits are dropped — they
            // are regenerated from `at_latch` within one scanline of
            // resumed rendering, so this is lossless in practice.
            self.at_shift_lo = u16::from(r.u8()?);
            self.at_shift_hi = u16::from(r.u8()?);
            let _at_feed_lo = r.u8()?;
            let _at_feed_hi = r.u8()?;
        }

        let ex_present = r.u8()?;
        let palette = r.u8()?;
        let chr_bank = r.u16()?;
        self.ex_attr_latch = match ex_present {
            0 => None,
            1 => Some(ExAttribute { palette, chr_bank }),
            other => return Err(PpuSnapshotError::InvalidPresence(other)),
        };
        let split_present = r.u8()?;
        let nt_addr = r.u16()?;
        let at_addr = r.u16()?;
        let fine_y = r.u8()?;
        let chr_bank8 = r.u8()?;
        self.bg_split_latch = match split_present {
            0 => None,
            1 => Some(BgSplitState {
                nt_addr,
                at_addr,
                fine_y,
                chr_bank: chr_bank8,
            }),
            other => return Err(PpuSnapshotError::InvalidPresence(other)),
        };

        r.bytes_into(&mut self.spr_shift_lo)?;
        r.bytes_into(&mut self.spr_shift_hi)?;
        r.bytes_into(&mut self.spr_attr)?;
        r.bytes_into(&mut self.spr_x)?;
        self.spr_count = r.u8()?;
        self.spr_zero_in_line = r.u8()? != 0;

        r.bytes_into(&mut self.framebuffer)?;

        // v3 (W3-Stage-4): the gated master-clock PPU tail. v1/v2 blobs
        // lack it; upconvert at the inactive defaults (countdown 0 = no
        // reload in flight), which is exactly what the pre-v3
        // clear-on-restore assumption imposed.
        if version >= 3 {
            self.restore_stage4_tail(&mut r)?;
        }

        // v4 (v1.7.0 F3): the in-flight extra-scanlines overclock countdown.
        // v1/v2/v3 blobs lack it; upconvert to `0` (no insertion in flight),
        // which is exactly the state a pre-v4 restore left it in.
        self.extra_lines_remaining = if version >= 4 { r.u16()? } else { 0 };

        // v5 (v2.0.3, ADR 0030): the 2-cycle-ALE in-flight fetch state. v1..=4
        // blobs lack it; upconvert to the inactive rest defaults (`0`/`false`) —
        // the state a fetch boundary leaves it in, and exactly what a pre-v5
        // restore imposed. A v5 blob taken mid-render round-trips the live values
        // so the re-simulated frame stays byte-identical (netplay rollback).
        if version >= 5 {
            self.octal_latch = r.u8()?;
            self.address_bus = r.u16()?;
            self.ale_armed = r.u8()? != 0;
            self.pattern_latch_stale = r.u8()? != 0;
            self.copy_v_delay = r.u8()?;
        } else {
            self.octal_latch = 0;
            self.address_bus = 0;
            self.ale_armed = false;
            self.pattern_latch_stale = false;
            self.copy_v_delay = 0;
        }

        // v6: per-sprite shifter halt state (see the write side). Pre-v6 blobs
        // lack it; upconvert to the power-on default (`true` = halted), which is
        // what a pre-v6 restore left it at (the field kept its constructor value).
        if version >= 6 {
            for h in &mut self.spr_halted {
                *h = r.u8()? != 0;
            }
            self.prev_rendering_enabled = r.u8()? != 0;
            self.rendering_enabled_delayed = r.u8()? != 0;
            self.oam_corruption_pending = r.u8()? != 0;
            self.oam_corruption_index = r.u8()?;
            self.oam_corruption_disabled = r.u8()? != 0;
            self.oam_corruption_disabled_instant = r.u8()? != 0;
        } else {
            self.spr_halted = [true; 8];
            self.prev_rendering_enabled = false;
            self.rendering_enabled_delayed = false;
            self.oam_corruption_pending = false;
            self.oam_corruption_index = 0;
            self.oam_corruption_disabled = false;
            self.oam_corruption_disabled_instant = false;
        }

        // sanity: the schema-fixed sizes mean we should be at end of input now.
        if r.pos != data.len() {
            return Err(PpuSnapshotError::Truncated(r.pos));
        }
        Ok(())
    }
}

impl Ppu {
    /// v3 (W3-Stage-4) tail decode: the `mc-ppu-2007-render-buffer` PPUDATA
    /// state machine + the `mc-ppu-subpos` BG-reload freeze. Bytes are
    /// always present in a v3 blob; fields whose cargo feature is off are
    /// consumed and discarded.
    fn restore_stage4_tail(&mut self, r: &mut R<'_>) -> Result<(), PpuSnapshotError> {
        let render_data_bus = r.u8()?;
        let ppudata_sm_countdown = r.u8()?;
        let ppudata_v_inc_pending = r.u8()? != 0;
        let mut spr_fetch_lo_raw = [0u8; 8];
        let mut spr_fetch_hi_raw = [0u8; 8];
        r.bytes_into(&mut spr_fetch_lo_raw)?;
        r.bytes_into(&mut spr_fetch_hi_raw)?;
        let ppudata_spr0_nt_addr = r.u16()?;
        {
            self.render_data_bus = render_data_bus;
            self.ppudata_sm_countdown = ppudata_sm_countdown;
            self.ppudata_v_inc_pending = ppudata_v_inc_pending;
            self.spr_fetch_lo_raw = spr_fetch_lo_raw;
            self.spr_fetch_hi_raw = spr_fetch_hi_raw;
            self.ppudata_spr0_nt_addr = ppudata_spr0_nt_addr;
        }
        let bg_reload_render = r.u8()? != 0;
        let mask_write_delay = r.u8()?;
        {
            self.bg_reload_render = bg_reload_render;
            self.mask_write_delay = mask_write_delay;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_round_trip() {
        let mut p = Ppu::new(PpuRegion::Ntsc);
        p.ciram[10] = 0xAB;
        p.oam[20] = 0xCD;
        p.palette_ram[5] = 0x21;
        p.framebuffer[100] = 0xEF;
        p.dot = 123;
        p.scanline = -1;
        p.frame = 42;
        p.ex_attr_latch = Some(ExAttribute {
            palette: 2,
            chr_bank: 0x123,
        });
        p.bg_split_latch = Some(BgSplitState {
            nt_addr: 0x2400,
            at_addr: 0x23C0,
            fine_y: 5,
            chr_bank: 7,
        });

        let blob = p.snapshot();

        let mut q = Ppu::new(PpuRegion::Pal);
        q.restore(&blob).unwrap();
        assert_eq!(q.region, PpuRegion::Ntsc);
        assert_eq!(q.ciram[10], 0xAB);
        assert_eq!(q.oam[20], 0xCD);
        assert_eq!(q.palette_ram[5], 0x21);
        assert_eq!(q.framebuffer[100], 0xEF);
        assert_eq!(q.dot, 123);
        assert_eq!(q.scanline, -1);
        assert_eq!(q.frame, 42);
        assert_eq!(
            q.ex_attr_latch,
            Some(ExAttribute {
                palette: 2,
                chr_bank: 0x123
            })
        );
        assert_eq!(
            q.bg_split_latch,
            Some(BgSplitState {
                nt_addr: 0x2400,
                at_addr: 0x23C0,
                fine_y: 5,
                chr_bank: 7,
            })
        );
    }

    #[test]
    fn snapshot_round_trips_16bit_attribute_shifters() {
        // v2 widened `at_shift_lo`/`at_shift_hi` from u8 to u16 (lockstep
        // with the BG pattern shifters; the 086ce4d left-edge palette
        // fix). Verify the full 16-bit value survives a round trip — a
        // regression that truncated to 8 bits would re-introduce the
        // attribute/pattern drift after a save-state load.
        let mut p = Ppu::new(PpuRegion::Ntsc);
        p.at_shift_lo = 0xAB12;
        p.at_shift_hi = 0xCD34;
        p.bg_shift_lo = 0x5678;
        p.bg_shift_hi = 0x9ABC;
        let blob = p.snapshot();
        assert_eq!(
            blob[0], PPU_SNAPSHOT_VERSION,
            "blob carries current version"
        );

        let mut q = Ppu::new(PpuRegion::Ntsc);
        q.restore(&blob).unwrap();
        assert_eq!(q.at_shift_lo, 0xAB12);
        assert_eq!(q.at_shift_hi, 0xCD34);
        assert_eq!(q.bg_shift_lo, 0x5678);
        assert_eq!(q.bg_shift_hi, 0x9ABC);
    }

    #[test]
    fn snapshot_reads_v1_attribute_shifters_as_low_byte() {
        // A v1 blob stored `at_shift_lo`/`at_shift_hi` as u8 plus two
        // 1-bit `at_feed_*` bytes. The v2 reader must accept v1 blobs and
        // promote the 8-bit attribute value into the low byte of the new
        // 16-bit register (the high byte was always implicitly zero in
        // the v1 model). Synthesize a v1 blob by snapshotting v2, then
        // rewriting the version byte + the 4-byte attribute region in the
        // v1 (u8 + u8 + u8 + u8) layout.
        let mut p = Ppu::new(PpuRegion::Ntsc);
        p.at_shift_lo = 0x00CD; // v1 could only hold the low byte
        p.at_shift_hi = 0x00EF;
        let v2 = p.snapshot();

        // Locate the attribute field: it follows bg_shift_lo (u16) +
        // bg_shift_hi (u16). We rebuild the blob as v1 by re-serialising
        // up to that point and splicing a v1-shaped attribute block. The
        // simplest robust construction: decode the v2 layout offset by
        // searching for the known 16-bit AT-low bytes we set.
        // at_shift_lo = 0x00CD -> LE bytes [0xCD, 0x00]; at_shift_hi =
        // 0x00EF -> [0xEF, 0x00]. In v1 these become [0xCD][0xEF] plus two
        // feed bytes. Build the v1 blob field-by-field by copying the
        // prefix, then the v1 attribute block, then the v2 tail (which is
        // identical from `ex_attr_latch` onward).
        // Find the 4-byte AT region: bytes [.., 0xCD,0x00, 0xEF,0x00, ..].
        let mut idx = None;
        for w in 0..v2.len().saturating_sub(4) {
            if v2[w] == 0xCD && v2[w + 1] == 0x00 && v2[w + 2] == 0xEF && v2[w + 3] == 0x00 {
                idx = Some(w);
                break;
            }
        }
        let at = idx.expect("locate v2 16-bit AT region");
        let mut v1 = Vec::new();
        v1.extend_from_slice(&v2[..at]); // prefix (incl. bg shifters)
        v1.push(0xCD); // v1 at_shift_lo (u8)
        v1.push(0xEF); // v1 at_shift_hi (u8)
        v1.push(0x01); // v1 at_feed_lo (u8)
        v1.push(0x00); // v1 at_feed_hi (u8)
        // Tail from ex_attr_latch onward, MINUS the v3 W3-Stage-4 tail
        // (23 bytes: u8*3 + [u8;8]*2 + u16 PPUDATA state machine, then
        // u8*2 BG-reload freeze), the v4 extra-scanlines countdown (2 bytes:
        // u16 `extra_lines_remaining`), the v5 2-cycle-ALE fetch-state tail
        // (6 bytes: u8 `octal_latch` + u16 `address_bus` + u8 `ale_armed` + u8
        // `pattern_latch_stale` + u8 `copy_v_delay`), AND the v6 render-state
        // tail (14 bytes: [u8;8] `spr_halted` + u8 `prev_rendering_enabled` + u8
        // `rendering_enabled_delayed` + u8*4 `oam_corruption_*`) — 45 bytes
        // total, none of which a v1 blob carried.
        v1.extend_from_slice(&v2[at + 4..v2.len() - 45]);
        v1[0] = 1; // version byte -> v1

        let mut q = Ppu::new(PpuRegion::Ntsc);
        q.restore(&v1).expect("v1 blob must upconvert");
        assert_eq!(q.at_shift_lo, 0x00CD, "v1 low byte promoted to 16-bit");
        assert_eq!(q.at_shift_hi, 0x00EF);
    }

    #[test]
    fn snapshot_rejects_short_blob() {
        let mut p = Ppu::new(PpuRegion::Ntsc);
        assert!(matches!(
            p.restore(&[]).unwrap_err(),
            PpuSnapshotError::Truncated(_)
        ));
        // The regression this size guard prevents: a SHORT blob whose first byte
        // is an unknown version must be classified `Truncated` (the guard runs
        // before the version check), NOT `UnsupportedVersion(0xFF)`.
        assert!(matches!(
            p.restore(&[0xFF; 4]).unwrap_err(),
            PpuSnapshotError::Truncated(_)
        ));
    }

    #[test]
    fn snapshot_rejects_bad_version() {
        let mut p = Ppu::new(PpuRegion::Ntsc);
        // A full-size blob (past the truncation guard) whose version byte is
        // unknown must be rejected at the version check — not mistaken for a
        // truncated blob. (A short bad-version blob is a Truncated case,
        // covered by `snapshot_rejects_short_blob`.)
        let mut blob = p.snapshot();
        blob[0] = 0xFF;
        let err = p.restore(&blob).unwrap_err();
        assert!(matches!(err, PpuSnapshotError::UnsupportedVersion(0xFF)));
    }

    #[test]
    fn snapshot_is_deterministic() {
        let p = Ppu::new(PpuRegion::Ntsc);
        assert_eq!(p.snapshot(), p.snapshot());
    }

    #[test]
    fn snapshot_round_trips_extra_lines_remaining() {
        // v1.7.0 F3: a save-state taken mid-insertion (extra_lines_remaining
        // > 0) must restore the in-flight countdown, not reset it to 0.
        let mut p = Ppu::new(PpuRegion::Ntsc);
        p.set_extra_scanlines(8);
        p.extra_lines_remaining = 5;
        let blob = p.snapshot();
        assert_eq!(
            blob[0], PPU_SNAPSHOT_VERSION,
            "blob carries current version"
        );

        let mut q = Ppu::new(PpuRegion::Ntsc);
        q.restore(&blob).unwrap();
        assert_eq!(q.extra_lines_remaining, 5);
    }

    #[test]
    fn snapshot_round_trips_2cycle_ale_fetch_state() {
        // v2.0.3 (ADR 0030): a checkpoint taken mid-render (netplay rollback)
        // can land with the 2-cycle-ALE fetch model's in-flight state live —
        // the octal latch / multiplexed bus, the ALE arm, and the two corruption
        // one-shots (`pattern_latch_stale`, the delayed-`CopyV` `copy_v_delay`).
        // They must round-trip so the re-simulated frame is byte-identical to the
        // forward run (else the peers desync). Regression pin for the promotion.
        let mut p = Ppu::new(PpuRegion::Ntsc);
        p.octal_latch = 0x19;
        p.address_bus = 0x2F19;
        p.ale_armed = true;
        p.pattern_latch_stale = true;
        p.copy_v_delay = 3;
        let blob = p.snapshot();

        let mut q = Ppu::new(PpuRegion::Ntsc);
        q.restore(&blob).unwrap();
        assert_eq!(q.octal_latch, 0x19);
        assert_eq!(q.address_bus, 0x2F19);
        assert!(q.ale_armed);
        assert!(q.pattern_latch_stale);
        assert_eq!(q.copy_v_delay, 3);
    }

    #[test]
    fn snapshot_default_extra_lines_remaining_is_zero() {
        // At the default extra_scanlines == 0 the countdown is always 0, so
        // the v4 field is a zero u16 and restore is behaviourally identical.
        let p = Ppu::new(PpuRegion::Ntsc);
        assert_eq!(p.extra_lines_remaining, 0);
        let blob = p.snapshot();
        let mut q = Ppu::new(PpuRegion::Ntsc);
        q.restore(&blob).unwrap();
        assert_eq!(q.extra_lines_remaining, 0);
    }

    #[test]
    fn set_extra_scanlines_resets_in_flight_countdown() {
        // Changing the configured count cancels any in-flight insertion so
        // the per-frame countdown cannot remain stale/out-of-bounds.
        let mut p = Ppu::new(PpuRegion::Ntsc);
        p.set_extra_scanlines(8);
        p.extra_lines_remaining = 6;
        p.set_extra_scanlines(2);
        assert_eq!(p.extra_lines_remaining, 0);
        p.extra_lines_remaining = 1;
        p.set_extra_scanlines(0); // disable
        assert_eq!(p.extra_lines_remaining, 0);
    }
}

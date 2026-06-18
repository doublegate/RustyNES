//! `Nes` facade — the public entry point that owns the entire emulator.
//!
//! Per `docs/architecture.md` §Public API surface. Mirrors the surface
//! that `rustynes-frontend` and `rustynes-test-harness` will consume.

use alloc::vec::Vec;
use alloc::{format, vec};
use rustynes_cpu::Cpu;
use rustynes_mappers::RomError;
use sha2::{Digest, Sha256};

// `core::time::Duration` is identical to `std::time::Duration` (same Duration
// type, re-exported through std for convenience). Using the `core` path keeps
// the public API surface portable to `#![no_std]` consumers without changing
// any caller. See `docs/architecture.md` §149 (no_std + alloc migration).
use core::time::Duration;

use crate::Region;
use crate::bus::LockstepBus;
use crate::controller::Buttons;
use crate::debug::{ApuDebugView, CpuDebugView, MapperDebugView, PpuDebugView};
use crate::genie::{GenieCode, GenieError};
use crate::input_device::InputDevice;
use crate::rewind::{REWIND_DEFAULT_KEYFRAME_PERIOD, REWIND_DEFAULT_MAX_BYTES, RewindRing};
use crate::save_state::{self, ROM_HASH_TAG_LEN, SnapshotError};

/// Nominal NTSC frame duration: `1 / 60.0988 Hz ≈ 16.6393 ms`.
///
/// Real hardware alternates 29780-cycle and 29781-cycle frames (the half
/// cycle averages to 60.0988 Hz); for wall-clock pacing we treat the
/// average as a single fixed-point interval and let small slips snap.
pub const FRAME_DURATION_NTSC: Duration = Duration::from_nanos(16_639_267);

/// Nominal PAL frame duration: `1 / 50.0070 Hz ≈ 19.9972 ms`.
pub const FRAME_DURATION_PAL: Duration = Duration::from_nanos(19_997_200);

/// Nominal Dendy frame duration: 50 Hz Russian famiclone, same as PAL.
pub const FRAME_DURATION_DENDY: Duration = Duration::from_nanos(19_997_200);

/// v1.1.0 beta.2 (Workstream C, T-110-C2) — one cycle-trace record.
///
/// The CPU register file + cycle count captured just before an instruction
/// executes. Recorded by [`Nes::run_frame`] while tracing is enabled (the
/// `debug-hooks` feature). The frontend disassembles the instruction at `pc`.
#[cfg(feature = "debug-hooks")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TraceRec {
    /// Program counter at instruction fetch.
    pub pc: u16,
    /// Accumulator.
    pub a: u8,
    /// X index.
    pub x: u8,
    /// Y index.
    pub y: u8,
    /// Stack pointer.
    pub s: u8,
    /// Processor status bits.
    pub p: u8,
    /// CPU cycle count at fetch.
    pub cycle: u64,
}

/// Top-level NES emulator handle.
///
/// Owns the CPU, PPU, mapper, RAM, and controller stub. Construct via
/// [`Nes::from_rom`]; drive forward via [`Nes::run_frame`] or
/// [`Nes::step_instruction`]. The framebuffer can be sampled at any time via
/// [`Nes::framebuffer`].
// Several independent debug-hooks toggles (breakpoints / trace / exec log) push
// the bool count over clippy's threshold; they are genuinely independent flags.
#[allow(clippy::struct_excessive_bools)]
pub struct Nes {
    cpu: Cpu,
    bus: LockstepBus,
    /// SHA-256 of the original ROM bytes the emulator was constructed from.
    rom_sha256: [u8; 32],
    /// Optional rewind ring buffer. Disabled by default — frontend opts in
    /// via [`Nes::enable_rewind`].
    rewind: Option<RewindRing>,
    /// v2.8.0 Phase 3 — when `false`, [`Nes::run_frame`] skips the rewind
    /// capture even with the ring armed. Run-ahead sets this for its
    /// hidden + visible frames so only the persistent timeline's frames
    /// land in the ring. Default `true` (byte-identical legacy behavior).
    rewind_capture_enabled: bool,
    /// v2.8.0 Phase 3 — reused scratch for the per-frame rewind capture
    /// (kills the ~320 KiB snapshot allocation per frame).
    rewind_snap_buf: Vec<u8>,
    /// Optional per-CPU-instruction boot trace (Session-12 observability).
    /// Gated on the `cpu-boot-trace` cargo feature so the default build
    /// pays no memory or codegen cost. See
    /// `crates/rustynes-core/src/cpu_boot_trace.rs`.
    #[cfg(feature = "cpu-boot-trace")]
    cpu_boot_trace: Option<crate::cpu_boot_trace::CpuBootTrace>,
    /// v1.1.0 beta.2 (Workstream C) — exec/PC breakpoints checked in
    /// [`Nes::run_frame`]. Gated on `debug-hooks` so the default build's hot
    /// path is untouched. Output-only: a hit stops the frame early and records
    /// the PC; it never mutates emulation, so determinism holds.
    #[cfg(feature = "debug-hooks")]
    breakpoints: Vec<u16>,
    /// Whether breakpoints are armed (lets the UI keep its list but pause
    /// checking). Default `true`.
    #[cfg(feature = "debug-hooks")]
    breakpoints_enabled: bool,
    /// The PC that last hit a breakpoint, taken by the frontend to pause.
    #[cfg(feature = "debug-hooks")]
    break_hit: Option<u16>,
    /// The PC to skip the breakpoint check on for the next step, so a
    /// "continue" resumes *past* the instruction it stopped on instead of
    /// re-breaking. Unlike a blind "skip the first iteration", this only skips
    /// the exact resumed PC — so a breakpoint sitting at the frame's start PC
    /// (after a reset / save-state load / manual PC change) still fires.
    #[cfg(feature = "debug-hooks")]
    skip_breakpoint_at: Option<u16>,
    /// v1.1.0 beta.2 (T-110-C2) — cycle-trace ring buffer (most-recent
    /// [`Self::TRACE_CAP`] instructions). Recorded in `run_frame` while
    /// `trace_enabled`. Output-only.
    #[cfg(feature = "debug-hooks")]
    trace: alloc::collections::VecDeque<TraceRec>,
    /// Whether the cycle-trace logger is recording. Default `false`.
    #[cfg(feature = "debug-hooks")]
    trace_enabled: bool,
    /// v1.1.0 beta.3 (T-110-E2) — per-frame executed-PC log for the Lua
    /// `onExec` callback. Distinct from [`Self::trace`] (a 50k rolling buffer
    /// shared with the Trace Logger panel): this is **cleared every frame**, so
    /// `onExec` replays only this frame's PCs — no stale / duplicate dispatch.
    /// Output-only; recorded only while `exec_logging`.
    #[cfg(feature = "debug-hooks")]
    exec_log: Vec<u16>,
    /// Whether the per-frame exec-PC log is recording. Default `false`.
    #[cfg(feature = "debug-hooks")]
    exec_logging: bool,
}

impl Nes {
    /// Build a new emulator from raw ROM bytes (iNES 1.0 or NES 2.0).
    ///
    /// # Errors
    ///
    /// Returns the underlying [`RomError`] if the bytes don't parse.
    pub fn from_rom(bytes: &[u8]) -> Result<Self, RomError> {
        let mut bus = LockstepBus::new(bytes)?;
        // Cold-boot path: `Cpu::power_on()` seeds `S=$00`; the subsequent
        // `reset()`'s `S -= 3` (wrapping) lands at `$FD`, matching Mesen2's
        // power-up state. See `docs/audit/session-13-cpu-boot-fix-2026-05-21.md`.
        let mut cpu = Cpu::power_on();
        cpu.reset(&mut bus);
        Ok(Self {
            cpu,
            bus,
            rom_sha256: sha256_of(bytes),
            rewind: None,
            rewind_capture_enabled: true,
            rewind_snap_buf: Vec::new(),
            #[cfg(feature = "cpu-boot-trace")]
            cpu_boot_trace: None,
            #[cfg(feature = "debug-hooks")]
            breakpoints: Vec::new(),
            #[cfg(feature = "debug-hooks")]
            breakpoints_enabled: true,
            #[cfg(feature = "debug-hooks")]
            break_hit: None,
            #[cfg(feature = "debug-hooks")]
            skip_breakpoint_at: None,
            #[cfg(feature = "debug-hooks")]
            trace: alloc::collections::VecDeque::new(),
            #[cfg(feature = "debug-hooks")]
            trace_enabled: false,
            #[cfg(feature = "debug-hooks")]
            exec_log: Vec::new(),
            #[cfg(feature = "debug-hooks")]
            exec_logging: false,
        })
    }

    /// Build an emulator with an explicit audio sample rate (the rate the
    /// CPAL stream is opened at).
    ///
    /// # Errors
    ///
    /// Returns the underlying [`RomError`] if the bytes don't parse.
    pub fn from_rom_with_sample_rate(bytes: &[u8], sample_rate: u32) -> Result<Self, RomError> {
        let mut bus = LockstepBus::with_sample_rate(bytes, sample_rate)?;
        // Cold-boot path: see comment in `from_rom`.
        let mut cpu = Cpu::power_on();
        cpu.reset(&mut bus);
        Ok(Self {
            cpu,
            bus,
            rom_sha256: sha256_of(bytes),
            rewind: None,
            rewind_capture_enabled: true,
            rewind_snap_buf: Vec::new(),
            #[cfg(feature = "cpu-boot-trace")]
            cpu_boot_trace: None,
            #[cfg(feature = "debug-hooks")]
            breakpoints: Vec::new(),
            #[cfg(feature = "debug-hooks")]
            breakpoints_enabled: true,
            #[cfg(feature = "debug-hooks")]
            break_hit: None,
            #[cfg(feature = "debug-hooks")]
            skip_breakpoint_at: None,
            #[cfg(feature = "debug-hooks")]
            trace: alloc::collections::VecDeque::new(),
            #[cfg(feature = "debug-hooks")]
            trace_enabled: false,
            #[cfg(feature = "debug-hooks")]
            exec_log: Vec::new(),
            #[cfg(feature = "debug-hooks")]
            exec_logging: false,
        })
    }

    /// Build an emulator from a Famicom Disk System `.fds` disk image and a
    /// user-supplied 8 KiB BIOS (`disksys.rom`).
    ///
    /// The BIOS is never committed to this repo (it is Nintendo IP); the caller
    /// supplies it (a frontend BIOS prompt is Stage 2). Construction parses the
    /// disk container, builds the FDS device as the bus's mapper, and runs the
    /// standard cold-boot reset (the BIOS reset vector at `$FFFC` drives the
    /// disk-load sequence).
    ///
    /// Uses the default 44.1 kHz audio sample rate; use
    /// [`Nes::from_disk_with_sample_rate`] to pick the rate.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`RomError`] if the disk image is unparseable or
    /// the BIOS is not exactly 8 KiB.
    pub fn from_disk(disk_bytes: &[u8], bios_bytes: &[u8]) -> Result<Self, RomError> {
        Self::from_disk_with_sample_rate(disk_bytes, bios_bytes, crate::bus::DEFAULT_SAMPLE_RATE)
    }

    /// Build an FDS emulator with an explicit audio sample rate. See
    /// [`Nes::from_disk`].
    ///
    /// The reported `rom_sha256` hashes the disk-image bytes (not the BIOS), so
    /// save-states / movies key off the disk the way cartridge builds key off
    /// the ROM.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`RomError`] if the disk image is unparseable or
    /// the BIOS is not exactly 8 KiB.
    pub fn from_disk_with_sample_rate(
        disk_bytes: &[u8],
        bios_bytes: &[u8],
        sample_rate: u32,
    ) -> Result<Self, RomError> {
        let mut bus = LockstepBus::with_disk(disk_bytes, bios_bytes, sample_rate)?;
        // Cold-boot path: see comment in `from_rom`.
        let mut cpu = Cpu::power_on();
        cpu.reset(&mut bus);
        Ok(Self {
            cpu,
            bus,
            rom_sha256: sha256_of(disk_bytes),
            rewind: None,
            rewind_capture_enabled: true,
            rewind_snap_buf: Vec::new(),
            #[cfg(feature = "cpu-boot-trace")]
            cpu_boot_trace: None,
            #[cfg(feature = "debug-hooks")]
            breakpoints: Vec::new(),
            #[cfg(feature = "debug-hooks")]
            breakpoints_enabled: true,
            #[cfg(feature = "debug-hooks")]
            break_hit: None,
            #[cfg(feature = "debug-hooks")]
            skip_breakpoint_at: None,
            #[cfg(feature = "debug-hooks")]
            trace: alloc::collections::VecDeque::new(),
            #[cfg(feature = "debug-hooks")]
            trace_enabled: false,
            #[cfg(feature = "debug-hooks")]
            exec_log: Vec::new(),
            #[cfg(feature = "debug-hooks")]
            exec_logging: false,
        })
    }

    /// Build an emulator that plays a classic NSF (`NESM`) music file.
    ///
    /// Only the classic `NESM\x1a` container is supported; `NSFe` and
    /// expansion-chip audio are documented deferrals.
    ///
    /// NSF files carry a ripped NES sound engine plus an `init`/`play` address
    /// pair, not a PPU program. Construction parses the file, installs a
    /// [`rustynes_mappers::NsfMapper`] (a synthetic 6502 driver + the program
    /// image) as the bus's mapper, and runs the standard cold-boot reset — the
    /// driver's reset vector calls `init` for the starting song, enables vblank
    /// NMI, and the ordinary 60 Hz NMI then calls `play` once per frame. Audio
    /// is produced through the unchanged lockstep loop; there is no video.
    ///
    /// Uses the default 44.1 kHz sample rate; see
    /// [`Nes::from_nsf_with_sample_rate`].
    ///
    /// # Errors
    ///
    /// Returns the underlying [`RomError`] when the NSF header is malformed.
    pub fn from_nsf(nsf_bytes: &[u8]) -> Result<Self, RomError> {
        Self::from_nsf_with_sample_rate(nsf_bytes, crate::bus::DEFAULT_SAMPLE_RATE)
    }

    /// Build an NSF player with an explicit audio sample rate. See
    /// [`Nes::from_nsf`].
    ///
    /// # Errors
    ///
    /// Returns the underlying [`RomError`] when the NSF header is malformed.
    pub fn from_nsf_with_sample_rate(nsf_bytes: &[u8], sample_rate: u32) -> Result<Self, RomError> {
        let mut bus = LockstepBus::with_nsf(nsf_bytes, sample_rate)?;
        let mut cpu = Cpu::power_on();
        cpu.reset(&mut bus);
        Ok(Self {
            cpu,
            bus,
            rom_sha256: sha256_of(nsf_bytes),
            rewind: None,
            rewind_capture_enabled: true,
            rewind_snap_buf: Vec::new(),
            #[cfg(feature = "cpu-boot-trace")]
            cpu_boot_trace: None,
            #[cfg(feature = "debug-hooks")]
            breakpoints: Vec::new(),
            #[cfg(feature = "debug-hooks")]
            breakpoints_enabled: true,
            #[cfg(feature = "debug-hooks")]
            break_hit: None,
            #[cfg(feature = "debug-hooks")]
            skip_breakpoint_at: None,
            #[cfg(feature = "debug-hooks")]
            trace: alloc::collections::VecDeque::new(),
            #[cfg(feature = "debug-hooks")]
            trace_enabled: false,
            #[cfg(feature = "debug-hooks")]
            exec_log: Vec::new(),
            #[cfg(feature = "debug-hooks")]
            exec_logging: false,
        })
    }

    /// Number of selectable songs in the loaded NSF (0 for a cartridge / disk).
    #[must_use]
    pub fn nsf_song_count(&self) -> u8 {
        self.bus.nsf_song_count()
    }

    /// The currently-selected 0-based NSF song (0 for a cartridge / disk).
    #[must_use]
    pub fn nsf_current_song(&self) -> u8 {
        self.bus.nsf_current_song()
    }

    /// Select a 0-based NSF song and restart playback on it (re-runs `init` via
    /// a warm reset). No-op for a cartridge / disk.
    pub fn nsf_set_song(&mut self, song: u8) {
        if self.bus.nsf_set_song(song) {
            // Re-vector through the driver's reset entry so `init` runs for the
            // new track. Warm reset preserves the freshly-patched driver state.
            self.reset();
        }
    }

    /// Build an emulator with a **randomized power-on RAM** state (developer
    /// mode; Phase 7 / T-72-005).
    ///
    /// Identical to [`Nes::from_rom`] except the 2 KiB CPU work RAM and the
    /// open-bus latch are filled from a deterministic `xorshift64` PRNG keyed
    /// on `seed`, modelling the unreliable power-on RAM of real hardware
    /// (nesdev "CPU power up state"). Use this to shake out game/test code
    /// that depends on a particular post-power-on RAM pattern.
    ///
    /// The randomization is **seeded and deterministic** — the same `seed`
    /// yields the same state, so the `same seed + ROM + input ⇒ bit-identical`
    /// contract still holds. The default [`Nes::from_rom`] (zeroed RAM) is
    /// what CI, the regression oracle, and save-state tests use.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`RomError`] if the bytes don't parse.
    pub fn from_rom_with_power_on_seed(bytes: &[u8], seed: u64) -> Result<Self, RomError> {
        let mut nes = Self::from_rom(bytes)?;
        // RAM is not consulted during the reset sequence (only the $FFFC/D
        // vector is), so randomizing after construction is correct.
        nes.bus.randomize_power_on_ram(seed);
        Ok(nes)
    }

    /// Reset (warm boot). Preserves WRAM; reloads PC from `$FFFC/D`.
    pub fn reset(&mut self) {
        self.bus.reset();
        self.cpu.reset(&mut self.bus);
    }

    /// Power-cycle (cold boot). Zeroes WRAM, re-rolls phase, reloads vectors.
    pub fn power_cycle(&mut self) {
        self.bus.power_cycle();
        // Cold-boot path: see comment in `from_rom`.
        self.cpu = Cpu::power_on();
        self.cpu.reset(&mut self.bus);
    }

    /// Run until the PPU finishes a frame. Returns the framebuffer slice.
    ///
    /// # Panics
    ///
    /// Panics if the CPU JAMs without producing a frame. Real software
    /// shouldn't JAM; if it does, the caller's run-loop should catch it
    /// before the next frame.
    pub fn run_frame(&mut self) -> &[u8] {
        // Hard cap: at NTSC the frame budget is 29,780.5 CPU cycles. Run
        // up to 5x that before bailing — gives breathing room for late
        // VBL detection or DMA-stall heavy frames before declaring "stuck".
        const MAX_CYCLES_PER_FRAME: u64 = 150_000;
        let start = self.bus.cycle();
        // T-110-C3 — the event viewer shows one frame; reset the log per frame.
        #[cfg(feature = "debug-hooks")]
        if self.bus.event_logging() {
            self.bus.clear_events();
        }
        // T-110-E2 — the Lua onRead/onWrite access log is per-frame too.
        #[cfg(feature = "debug-hooks")]
        if self.bus.access_logging() {
            self.bus.clear_accesses();
        }
        // T-110-E1 — the Lua onNmi/onIrq interrupt-service log is per-frame too
        // (cleared here so a replay only ever sees this frame's services, never
        // a stale carry-over — mirrors the exec_log clear below).
        #[cfg(feature = "debug-hooks")]
        if self.bus.interrupt_logging() {
            self.bus.clear_interrupts();
        }
        // v1.6.0 Workstream A3 — reset the `TAStudio` lag-log "controller polled"
        // flag so was_input_polled_this_frame() reflects only this frame (a
        // frame that ends still-`false` is a lag frame). Output-only.
        #[cfg(feature = "debug-hooks")]
        self.bus.clear_controller_polled();
        // v1.4.0 Workstream D (D2) — start each frame with no event-breakpoint
        // hit so the frontend's "first hit of the frame" pause is per-frame.
        #[cfg(feature = "debug-hooks")]
        if self.bus.event_breakpoints() != 0 {
            self.bus.clear_event_break_hit();
        }
        // T-110-E2 — the Lua onExec exec-PC log is per-frame (cleared here so a
        // replay only ever sees this frame's PCs, never a stale carry-over).
        #[cfg(feature = "debug-hooks")]
        if self.exec_logging {
            self.exec_log.clear();
        }
        while !self.bus.take_frame_complete() {
            if self.cpu.is_jammed() {
                break;
            }
            if self.bus.cycle().wrapping_sub(start) > MAX_CYCLES_PER_FRAME {
                break;
            }
            #[cfg(feature = "debug-hooks")]
            {
                // v1.1.0 beta.2 (Workstream C) — exec/PC breakpoints. The
                // `skip_breakpoint_at` PC is stepped past exactly once (so a
                // "continue" resumes off the instruction it stopped on instead
                // of re-breaking in place); any OTHER breakpoint PC — including
                // one at the frame's starting PC after a reset / save-state load
                // / manual PC change — still fires immediately.
                if self.breakpoints_enabled && self.breakpoints.contains(&self.cpu.pc) {
                    if self.skip_breakpoint_at == Some(self.cpu.pc) {
                        self.skip_breakpoint_at = None;
                    } else {
                        // Hit: stop the (partial) frame and report the PC. No
                        // state mutated; the frame simply isn't completed.
                        self.break_hit = Some(self.cpu.pc);
                        self.skip_breakpoint_at = Some(self.cpu.pc);
                        return self.bus.framebuffer();
                    }
                } else {
                    self.skip_breakpoint_at = None;
                }
                // T-110-E2 — per-frame exec-PC log for Lua onExec (bounded by
                // MAX_CYCLES_PER_FRAME, so no explicit cap needed).
                if self.exec_logging {
                    self.exec_log.push(self.cpu.pc);
                }
                // T-110-C2 — cycle trace: record the about-to-execute
                // instruction's CPU state (ring-capped, oldest dropped).
                if self.trace_enabled {
                    if self.trace.len() >= Self::TRACE_CAP {
                        self.trace.pop_front();
                    }
                    self.trace.push_back(TraceRec {
                        pc: self.cpu.pc,
                        a: self.cpu.a,
                        x: self.cpu.x,
                        y: self.cpu.y,
                        s: self.cpu.s,
                        p: self.cpu.p.bits(),
                        cycle: self.cpu.cycles,
                    });
                }
            }
            #[cfg(feature = "cpu-boot-trace")]
            self.cpu_boot_trace_record();
            self.cpu.step(&mut self.bus);
        }
        // Sample any attached Zapper's light detection from the completed
        // frame. This is a no-op (and the run loop above is byte-identical)
        // when no Zapper is attached, so the determinism contract holds.
        self.bus.sample_zapper_light();
        // After the frame completes, push state into the rewind ring so
        // the frontend's hold-F5 UX has somewhere to walk back from.
        // v2.8.0 Phase 3 — run-ahead suppresses the capture for its hidden
        // + visible frames via `set_rewind_capture(false)`.
        if self.rewind.is_some() && self.rewind_capture_enabled {
            self.rewind_capture();
        }
        self.bus.framebuffer()
    }

    /// v2.8.0 Phase 3 — enable/disable the per-frame rewind capture while
    /// the ring stays armed. Run-ahead turns it off around its hidden +
    /// visible frames so only persistent-timeline frames land in the ring.
    /// Default `true`; with no rewind ring armed this is a no-op.
    pub const fn set_rewind_capture(&mut self, enabled: bool) {
        self.rewind_capture_enabled = enabled;
    }

    /// Step exactly one CPU instruction. For debuggers / step-through tools.
    pub fn step_instruction(&mut self) -> u8 {
        #[cfg(feature = "cpu-boot-trace")]
        self.cpu_boot_trace_record();
        self.cpu.step(&mut self.bus)
    }

    /// v1.1.0 beta.2 (Workstream C) — add an exec/PC breakpoint at `addr`.
    /// [`Nes::run_frame`] stops the frame the next time the program counter
    /// reaches `addr` (reportable via [`Nes::take_break_hit`]). Idempotent.
    /// `debug-hooks` only.
    #[cfg(feature = "debug-hooks")]
    pub fn add_breakpoint(&mut self, addr: u16) {
        if !self.breakpoints.contains(&addr) {
            self.breakpoints.push(addr);
        }
    }

    /// Remove a previously-added exec breakpoint (no-op if absent).
    #[cfg(feature = "debug-hooks")]
    pub fn remove_breakpoint(&mut self, addr: u16) {
        self.breakpoints.retain(|&a| a != addr);
    }

    /// Remove all breakpoints.
    #[cfg(feature = "debug-hooks")]
    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.clear();
    }

    /// The current exec breakpoints (insertion order).
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    // `Vec` -> slice deref coercion is not const, so this can't be `const fn`
    // (clippy's `missing_const_for_fn` is a false positive here).
    #[allow(clippy::missing_const_for_fn)]
    pub fn breakpoints(&self) -> &[u16] {
        &self.breakpoints
    }

    /// Arm/disarm breakpoint checking without discarding the list. Default on.
    #[cfg(feature = "debug-hooks")]
    pub const fn set_breakpoints_enabled(&mut self, enabled: bool) {
        self.breakpoints_enabled = enabled;
    }

    /// Whether breakpoint checking is armed.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub const fn breakpoints_enabled(&self) -> bool {
        self.breakpoints_enabled
    }

    /// Take the PC that last hit a breakpoint (cleared on read). The frontend
    /// polls this after [`Nes::run_frame`] to pause when a breakpoint fired.
    #[cfg(feature = "debug-hooks")]
    pub const fn take_break_hit(&mut self) -> Option<u16> {
        self.break_hit.take()
    }

    /// v1.4.0 Workstream D (D2) — arm the event-driven breakpoint categories
    /// (a bit-OR of [`crate::EventBpKind::bit`]). `0` (default) disarms every
    /// category — the per-access taps are then a single cheap `mask == 0`
    /// early-out. Output-only: a hit pauses + reports but never mutates state.
    /// `debug-hooks` only.
    #[cfg(feature = "debug-hooks")]
    pub const fn set_event_breakpoints(&mut self, mask: u16) {
        self.bus.set_event_breakpoints(mask);
    }

    /// The armed event-breakpoint category mask.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub const fn event_breakpoints(&self) -> u16 {
        self.bus.event_breakpoints()
    }

    /// Take the first event-breakpoint hit of the current frame (cleared on
    /// read). The frontend polls this after [`Nes::run_frame`] to pause when an
    /// armed hardware event fired, reporting its kind + frame/cycle/scanline/dot.
    #[cfg(feature = "debug-hooks")]
    pub const fn take_event_break_hit(&mut self) -> Option<crate::EventBreakHit> {
        self.bus.take_event_break_hit()
    }

    /// Maximum cycle-trace ring depth (oldest records drop past this).
    #[cfg(feature = "debug-hooks")]
    pub const TRACE_CAP: usize = 50_000;

    /// v1.1.0 beta.2 (T-110-C2) — start/stop the cycle-trace logger. While on,
    /// each executed instruction's CPU state is pushed to a ring buffer (capped
    /// at [`Self::TRACE_CAP`]). Default off.
    #[cfg(feature = "debug-hooks")]
    pub const fn set_trace_enabled(&mut self, enabled: bool) {
        self.trace_enabled = enabled;
    }

    /// Whether the cycle-trace logger is recording.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub const fn trace_enabled(&self) -> bool {
        self.trace_enabled
    }

    /// Number of records currently in the trace ring.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub fn trace_len(&self) -> usize {
        self.trace.len()
    }

    /// Clear the trace ring.
    #[cfg(feature = "debug-hooks")]
    pub fn clear_trace(&mut self) {
        self.trace.clear();
    }

    /// Copy the trace ring oldest-first (for the trace panel / file export).
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub fn trace_records(&self) -> alloc::vec::Vec<TraceRec> {
        self.trace.iter().copied().collect()
    }

    /// Copy the most recent `n` trace records (oldest-first) — for the live
    /// trace panel's tail view, cheaper than [`Self::trace_records`] on a full
    /// ring.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub fn trace_tail_vec(&self, n: usize) -> alloc::vec::Vec<TraceRec> {
        let skip = self.trace.len().saturating_sub(n);
        self.trace.iter().skip(skip).copied().collect()
    }

    /// v1.1.0 beta.2 (T-110-C3) — start/stop the event viewer. While on, the bus
    /// records this frame's PPU/APU/mapper writes (with their PPU position); the
    /// log is reset at each [`Self::run_frame`]. Default off; output-only.
    #[cfg(feature = "debug-hooks")]
    pub const fn set_event_logging(&mut self, enabled: bool) {
        self.bus.set_event_logging(enabled);
    }

    /// Whether the event viewer is recording.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub const fn event_logging(&self) -> bool {
        self.bus.event_logging()
    }

    /// The current frame's captured events (for the event-viewer panel).
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // slice deref is not const.
    pub fn events(&self) -> &[crate::bus::EventRec] {
        self.bus.events()
    }

    /// v1.1.0 beta.3 (T-110-E2) — start/stop the Lua bus-access log. While on,
    /// the bus records this frame's CPU reads + writes (with values); the log is
    /// reset at each [`Self::run_frame`]. Default off; output-only. Enabled by
    /// the scripting engine only while `onRead`/`onWrite` callbacks exist.
    #[cfg(feature = "debug-hooks")]
    pub const fn set_access_logging(&mut self, enabled: bool) {
        self.bus.set_access_logging(enabled);
    }

    /// Whether the bus-access log is recording.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub const fn access_logging(&self) -> bool {
        self.bus.access_logging()
    }

    /// The current frame's captured CPU bus accesses (for the Lua engine).
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // slice deref is not const.
    pub fn accesses(&self) -> &[crate::bus::AccessRec] {
        self.bus.accesses()
    }

    /// v1.1.0 beta.3 (T-110-E2) — start/stop the per-frame exec-PC log for the
    /// Lua `onExec` callback. Independent of the Trace Logger (`set_trace_enabled`),
    /// so enabling it does not disturb the debugger's trace recording. Cleared
    /// every [`Self::run_frame`]; output-only.
    #[cfg(feature = "debug-hooks")]
    pub const fn set_exec_logging(&mut self, enabled: bool) {
        self.exec_logging = enabled;
    }

    /// Whether the per-frame exec-PC log is recording.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub const fn exec_logging(&self) -> bool {
        self.exec_logging
    }

    /// This frame's executed PCs, in execution order (for the Lua engine).
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // slice deref is not const.
    pub fn exec_log(&self) -> &[u16] {
        &self.exec_log
    }

    /// `true` if the running program read a controller port (`$4016`/`$4017`)
    /// during the most recent [`Self::run_frame`] — the inverse of a `TAStudio`
    /// "lag frame" (v1.6.0 Workstream A3). The greenzone / piano-roll lag log
    /// queries this each frame. Output-only; `debug-hooks`-gated, so the
    /// shipped build is byte-identical and the determinism contract holds.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub const fn was_input_polled_this_frame(&self) -> bool {
        self.bus.controller_polled()
    }

    /// v1.2.0 (T-110-E1) — start/stop the per-frame interrupt-service log for
    /// the Lua `onNmi` / `onIrq` callbacks. The log records this frame's
    /// committed NMI / IRQ / BRK service entries (captured at the CPU's
    /// service-vector commit point, NOT the speculative poll sampler); it is
    /// cleared at each [`Self::run_frame`]. Default off; output-only. Enabled by
    /// the scripting engine only while `onNmi`/`onIrq` callbacks exist. Mirrors
    /// [`Self::set_exec_logging`].
    #[cfg(feature = "debug-hooks")]
    pub const fn set_interrupt_logging(&mut self, enabled: bool) {
        self.bus.set_interrupt_logging(enabled);
    }

    /// Whether the per-frame interrupt-service log is recording.
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    pub const fn interrupt_logging(&self) -> bool {
        self.bus.interrupt_logging()
    }

    /// This frame's committed interrupt-service entries, in service order (for
    /// the Lua engine). Mirrors [`Self::exec_log`] / [`Self::accesses`].
    #[cfg(feature = "debug-hooks")]
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // slice deref is not const.
    pub fn interrupt_log(&self) -> &[crate::bus::InterruptRec] {
        self.bus.interrupts()
    }

    /// Borrow the framebuffer (RGBA8, 256x240).
    #[must_use]
    pub fn framebuffer(&self) -> &[u8] {
        self.bus.framebuffer()
    }

    /// Borrow the parallel palette-index framebuffer (256x240 `u16`s, each
    /// `(emphasis << 6) | colour`) for the `NES_NTSC` composite filter.
    /// See [`rustynes_ppu::Ppu::index_framebuffer`].
    #[must_use]
    pub fn index_framebuffer(&self) -> &[u16] {
        self.bus.index_framebuffer()
    }

    /// v1.2.0 C3 (hd-pack) — borrow the per-pixel HD-pack tile-source buffer
    /// (256x240 [`rustynes_ppu::HdTileSource`] records). Each entry names the
    /// CHR tile that produced the pixel; the frontend HD-pack loader groups
    /// these by 8x8 cell, hashes the CHR bytes, and substitutes hi-res tiles.
    /// Output-only telemetry; the determinism contract is unaffected. See
    /// [`rustynes_ppu::Ppu::hd_tile_source`].
    #[cfg(feature = "hd-pack")]
    #[must_use]
    pub fn hd_tile_source(&self) -> &[rustynes_ppu::HdTileSource] {
        self.bus.hd_tile_source()
    }

    /// The per-frame NTSC composite colour phase consumed by the `NES_NTSC`
    /// filter (`0..=2` on NTSC; frame parity `0..=1` on PAL/Dendy). See
    /// [`rustynes_ppu::Ppu::ntsc_phase`].
    #[must_use]
    pub const fn ntsc_phase(&self) -> u8 {
        self.bus.ntsc_phase()
    }

    /// The completed-frame counter (PPU frames since power-on). A monotonic,
    /// deterministic, save-state-restored value — the frontend uses it to phase
    /// turbo/autofire so the strobe is reproducible under rollback / TAS replay.
    #[must_use]
    pub const fn frame(&self) -> u64 {
        self.bus.ppu().frame()
    }

    /// Borrow the underlying bus (debugger / tests).
    #[must_use]
    pub const fn bus(&self) -> &LockstepBus {
        &self.bus
    }

    /// Mutably borrow the underlying bus (debugger / tests).
    pub const fn bus_mut(&mut self) -> &mut LockstepBus {
        &mut self.bus
    }

    /// Borrow the CPU (debugger / tests).
    #[must_use]
    pub const fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    /// Cumulative CPU cycle count.
    #[must_use]
    pub const fn cycle(&self) -> u64 {
        self.bus.cycle()
    }

    /// Cartridge region (NTSC / PAL / Dendy / Multi). Drives wall-clock
    /// frame pacing in the frontend and clock dividers in the chip cores.
    #[must_use]
    pub const fn region(&self) -> Region {
        match self.bus.region() {
            rustynes_mappers::Region::Pal => Region::Pal,
            rustynes_mappers::Region::Dendy => Region::Dendy,
            // iNES 1.0 "Multi" cartridges are treated as NTSC for pacing
            // (matches the PPU / APU init in `LockstepBus::with_sample_rate`).
            _ => Region::Ntsc,
        }
    }

    /// Length in bytes of the loaded cartridge's PRG-ROM (read-only metadata).
    ///
    /// Exposed for the Lua scripting `cart:prg_size()` query (and any other
    /// read-only consumer); does not touch deterministic state.
    #[must_use]
    pub const fn prg_rom_len(&self) -> usize {
        self.bus.prg_rom_len()
    }

    /// Length in bytes of the loaded cartridge's CHR-ROM (0 when the board uses
    /// CHR-RAM). Read-only metadata; backs the Lua `cart:chr_size()` query.
    #[must_use]
    pub const fn chr_rom_len(&self) -> usize {
        self.bus.chr_rom_len()
    }

    /// The loaded mapper's iNES / NES 2.0 mapper id (backs `cart:mapper_id()`).
    #[must_use]
    pub fn mapper_id(&self) -> u16 {
        self.bus.mapper_debug_info().mapper_id
    }

    /// Wall-clock frame duration for this cartridge's region. The frontend
    /// uses this to pace emulator advance independently of monitor refresh
    /// rate — without it, `Fifo` present mode on a 144 Hz monitor would
    /// run the emulator 2.4× too fast.
    #[must_use]
    pub const fn frame_duration(&self) -> Duration {
        match self.region() {
            Region::Pal => FRAME_DURATION_PAL,
            Region::Dendy => FRAME_DURATION_DENDY,
            Region::Ntsc => FRAME_DURATION_NTSC,
        }
    }

    /// Drain accumulated audio samples (host sample rate, normalized
    /// `[0.0, ~1.0]`).  Call once per frame from the frontend's audio thread
    /// or batch driver.
    pub fn drain_audio(&mut self) -> Vec<f32> {
        self.bus.drain_audio()
    }

    /// Set the buttons currently held on player `port`. Ports 0/1 are the
    /// standard controllers (`$4016`/`$4017`); ports 2/3 are players 3/4 on
    /// the Four Score adapter (only polled when [`Self::set_four_score`] is
    /// on). The change takes effect on the next strobe edge.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=3`.
    pub const fn set_buttons(&mut self, port: usize, buttons: Buttons) {
        self.bus.set_buttons(port, buttons);
    }

    /// Get the buttons currently held on player `port` (0/1 = `$4016`/`$4017`;
    /// 2/3 = Four Score players 3/4). Read-only; does not advance emulator
    /// state.
    ///
    /// Used by the TAS movie recorder (`crate::movie`) to capture the inputs
    /// applied before each [`Self::run_frame`]. (Movies record players 1 & 2;
    /// Four Score players 3/4 are not part of the `.rnm` stream.)
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=3`.
    #[must_use]
    pub const fn buttons(&self, port: usize) -> Buttons {
        self.bus.controller(port).buttons()
    }

    /// Enable/disable the Four Score 4-player adapter. Off by default; while
    /// off, controller reads are byte-identical to the standard two-pad
    /// behavior (the determinism contract and save-states are unaffected).
    /// When on, players 3/4 (ports 2/3) are multiplexed onto `$4016`/`$4017`
    /// across a 24-read serial sequence.
    pub const fn set_four_score(&mut self, enabled: bool) {
        self.bus.set_four_score(enabled);
    }

    /// Whether the Four Score adapter is currently enabled.
    #[must_use]
    pub const fn four_score(&self) -> bool {
        self.bus.four_score()
    }

    // --- Vs. System DIP switches + coin/service inputs ---

    /// True when the running cart is Nintendo Vs. System arcade hardware
    /// (NES 2.0 console type = Vs. System). The RGB PPU + DIP/coin inputs only
    /// take effect on such carts.
    #[must_use]
    pub fn is_vs_system(&self) -> bool {
        self.bus.is_vs_system()
    }

    /// True when the cart's header marks a Vs. `DualSystem` board (two CPUs /
    /// two PPUs; NES 2.0 byte-13 high nibble = Vs. hardware type 5/6).
    ///
    /// Detection only: this single-system core cannot boot a `DualSystem` title
    /// past its attract handshake, so the frontend uses this to surface a clear
    /// note. The two-CPU/two-PPU emulation is a documented v2.0 deferral
    /// (`docs/audit/vs-dualsystem-design-2026-06-11.md`).
    #[must_use]
    pub const fn is_vs_dual_system(&self) -> bool {
        self.bus.is_vs_dual_system()
    }

    /// Set the Vs. System 8-bit DIP-switch bank (switch 1 = bit 0 .. switch 8 =
    /// bit 7). Read through the upper bits of `$4016`/`$4017`. No effect on
    /// non-Vs. carts; the standard controller read stays byte-identical.
    pub const fn set_vs_dip(&mut self, dip: u8) {
        self.bus.set_vs_dip(dip);
    }

    /// Current Vs. System DIP-switch bank.
    #[must_use]
    pub const fn vs_dip(&self) -> u8 {
        self.bus.vs_dip()
    }

    /// Override the Vs. System PPU type and re-apply the output palette.
    ///
    /// iNES-1.0 Vs. dumps default to the 2C03 palette (no NES 2.0 byte-13);
    /// the per-game database ([`crate::vs_db`]) supplies the correct
    /// 2C04-000x / 2C05 type, which the frontend applies through this setter.
    /// Affects only the colour LUT the PPU emits through, never game logic.
    /// No effect on non-Vs. carts.
    pub const fn set_vs_ppu_type(&mut self, t: rustynes_mappers::VsPpuType) {
        self.bus.set_vs_ppu_type(t);
    }

    /// Latch a Vs. System coin insertion on the given acceptor (0 = #1, 1 = #2).
    /// Reads true for a real-hardware ~40-70 ms window; the frontend should
    /// clear it (see [`Self::clear_coin`]) after a few frames.
    pub const fn insert_coin(&mut self, acceptor: u8) {
        self.bus.insert_coin(acceptor);
    }

    /// Clear all latched Vs. System coin-insert signals.
    pub const fn clear_coin(&mut self) {
        self.bus.clear_coin();
    }

    /// Set / clear the Vs. System service button.
    pub const fn set_vs_service(&mut self, pressed: bool) {
        self.bus.set_vs_service(pressed);
    }

    // --- Famicom Disk System disk control (Stage 2b) ---

    /// Number of disk sides in the inserted FDS image. Returns 0 for cartridge
    /// builds (so a frontend can branch on "is this an FDS game?").
    #[must_use]
    pub fn disk_side_count(&self) -> usize {
        self.bus.disk_side_count()
    }

    /// The currently inserted FDS disk side index, or `None` when ejected (or
    /// for a cartridge build). A game that prompts "insert side B" is asking the
    /// user to call [`Self::set_disk_side`].
    #[must_use]
    pub fn inserted_disk_side(&self) -> Option<usize> {
        self.bus.inserted_disk_side()
    }

    /// Insert FDS side `i` (`Some(i)`) or eject the disk (`None`). Inserting
    /// resets the head and opens a short deterministic "not ready" window (the
    /// BIOS polls `$4032` and waits for ready); an out-of-range index is
    /// ignored. No-op on cartridge builds. This is how the user complies with a
    /// game's "insert side N" prompt.
    pub fn set_disk_side(&mut self, side: Option<usize>) {
        self.bus.set_disk_side(side);
    }

    /// Start recording the diagnostic FDS read-stream trace (the `$4031` disk-byte
    /// stream + `$4025` control writes + side changes). Off by default and
    /// observation-only — it never affects emulation, so the determinism contract
    /// holds. Drain it with [`Self::take_fds_trace`]. No-op on cartridge builds.
    /// Used by the `fds_trace` diagnostic harness to debug disk-read / side-swap
    /// failures (e.g. the Kid Icarus side-B `ERR.07` stall).
    pub fn enable_fds_trace(&mut self) {
        self.bus.enable_fds_trace();
    }

    /// Drain the accumulated FDS read-stream trace records. Empty for cartridge
    /// builds or when [`Self::enable_fds_trace`] was never called.
    #[must_use]
    pub fn take_fds_trace(&mut self) -> Vec<rustynes_mappers::FdsTraceRec> {
        self.bus.take_fds_trace()
    }

    /// Re-serialize the (possibly-modified) FDS disk image to the headerless
    /// `.fds` byte layout so the host can write it to a side-car `.fds.sav`
    /// (keyed by [`Self::rom_sha256`]). Empty for cartridge builds.
    #[must_use]
    pub fn disk_image_bytes(&self) -> Vec<u8> {
        self.bus.disk_image_bytes()
    }

    /// Whether the FDS disk image has unsaved writes since the last
    /// [`Self::clear_disk_dirty`]. A frontend checks this on quit / periodically
    /// to decide whether to persist the disk.
    #[must_use]
    pub fn disk_is_dirty(&self) -> bool {
        self.bus.disk_is_dirty()
    }

    /// Clear the FDS disk dirty flag after persisting the image.
    pub fn clear_disk_dirty(&mut self) {
        self.bus.clear_disk_dirty();
    }

    /// Mark the inserted FDS disk read-only (`true`) or writable (`false`,
    /// the default). Drives the `$4032` write-protect flag; a write-protected
    /// disk drops bytes in write mode without modifying the medium.
    pub fn set_disk_write_protected(&mut self, protected: bool) {
        self.bus.set_disk_write_protected(protected);
    }

    /// Attach a non-standard overlay input device on `port` (0 = `$4016`, 1 =
    /// `$4017`). Pass `None` to unplug it and return the port to the standard
    /// controller / Four Score path (byte-identical reads). Devices are
    /// unplugged on power-cycle.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub fn set_expansion_device(&mut self, port: usize, device: Option<InputDevice>) {
        self.bus.set_expansion_device(port, device);
    }

    /// Borrow the overlay device attached to `port` (0 = `$4016`, 1 =
    /// `$4017`), if any.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    #[must_use]
    pub const fn expansion_device(&self, port: usize) -> &Option<InputDevice> {
        self.bus.expansion_device(port)
    }

    /// Attach an Arkanoid "Vaus" paddle on `port` (typically port 1 / `$4017`)
    /// and set its position + fire state. `position` is the raw 8-bit
    /// potentiometer value (`$00` far-left .. `$FF` far-right); `fire` is the
    /// single button. Convenience wrapper that attaches the device if absent
    /// then updates it.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub fn set_paddle(&mut self, port: usize, position: u8, fire: bool) {
        if !matches!(self.bus.expansion_device(port), Some(InputDevice::Vaus(_))) {
            self.bus.set_expansion_device(
                port,
                Some(InputDevice::Vaus(crate::input_device::VausState::new())),
            );
        }
        self.bus.set_paddle(port, position, fire);
    }

    /// Attach an NES Zapper light gun on `port` (typically port 1 / `$4017`)
    /// and set its aim point + trigger. `(x, y)` is the screen pixel the gun is
    /// aimed at (0..256, 0..240; out of range = off-screen); `trigger` is the
    /// trigger state. Convenience wrapper that attaches the device if absent
    /// then updates it.
    ///
    /// Light detection is sampled from the framebuffer at the end of each
    /// [`Self::run_frame`]; the determinism contract holds because the sample
    /// only runs when a Zapper is attached (the no-device path is unchanged).
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub fn set_zapper(&mut self, port: usize, x: u16, y: u16, trigger: bool) {
        if !matches!(
            self.bus.expansion_device(port),
            Some(InputDevice::Zapper(_))
        ) {
            self.bus.set_expansion_device(
                port,
                Some(InputDevice::Zapper(crate::input_device::ZapperState::new())),
            );
        }
        self.bus.set_zapper(port, x, y, trigger);
    }

    /// Attach an NES Power Pad / Family Fun Fitness mat on `port` (typically
    /// port 1 / `$4017`) and set its live button mask (bit `i` = mat button
    /// `i+1`, 0..=11). Convenience wrapper that attaches the device if absent
    /// then updates it. Opt-in: the no-device path stays byte-identical.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub fn set_power_pad(&mut self, port: usize, buttons: u16) {
        if !matches!(
            self.bus.expansion_device(port),
            Some(InputDevice::PowerPad(_))
        ) {
            self.bus.set_expansion_device(
                port,
                Some(InputDevice::PowerPad(
                    crate::input_device::PowerPadState::new(),
                )),
            );
        }
        self.bus.set_power_pad(port, buttons);
    }

    /// v1.6.0 B3 — the latched standard-controller button bitmask for `port`
    /// (`0` = P1 / `$4016`, `1` = P2 / `$4017`; `2`/`3` are the Four Score
    /// players), in [`Buttons`](crate::Buttons) bit order (A = bit 0 .. Right =
    /// bit 7). Read-only and side-effect-free — it reads the latched state, not
    /// the shift register, so it never perturbs a controller poll. Exposed for
    /// the Lua `joypad.get` query.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=3`.
    #[must_use]
    pub fn controller_buttons(&self, port: usize) -> u8 {
        assert!(port <= 3, "controller port {port} out of range (0..=3)");
        self.bus.controller(port).buttons().bits()
    }

    /// v1.2.0 Workstream D — attach a SNES-style serial mouse on `port` (0 =
    /// `$4016`, 1 = `$4017`) and set its movement / buttons / sensitivity.
    /// `(dx, dy)` are the signed per-frame deltas (clamped to +/-127 on latch);
    /// `sensitivity` is 0 (low) / 1 (medium) / 2 (high). Convenience wrapper
    /// that attaches the device if absent then updates it. Opt-in: the no-device
    /// path stays byte-identical.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub fn set_snes_mouse(
        &mut self,
        port: usize,
        dx: i16,
        dy: i16,
        left: bool,
        right: bool,
        sensitivity: u8,
    ) {
        if !matches!(
            self.bus.expansion_device(port),
            Some(InputDevice::SnesMouse(_))
        ) {
            self.bus.set_expansion_device(
                port,
                Some(InputDevice::SnesMouse(
                    crate::input_device::SnesMouseState::new(),
                )),
            );
        }
        self.bus
            .set_snes_mouse(port, dx, dy, left, right, sensitivity);
    }

    /// v1.2.0 Workstream D — attach a Famicom Family BASIC keyboard on `port`
    /// (typically port 1 / `$4017`) and set its pressed-key bitmap. `keys` is
    /// one byte per matrix row (`keys[row]` bits 0..=3 = column-half 0 keys,
    /// bits 4..=7 = column-half 1 keys); the frontend builds it from host keys.
    /// Convenience wrapper that attaches the device if absent then updates it.
    /// Opt-in: the no-device path stays byte-identical.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub fn set_family_keyboard(&mut self, port: usize, keys: [u8; 9]) {
        if !matches!(
            self.bus.expansion_device(port),
            Some(InputDevice::FamilyKeyboard(_))
        ) {
            self.bus.set_expansion_device(
                port,
                Some(InputDevice::FamilyKeyboard(
                    crate::input_device::FamilyKeyboardState::new(),
                )),
            );
        }
        self.bus.set_family_keyboard(port, keys);
    }

    /// v1.3.0 Workstream F1 — attach a Bandai **Family Trainer** mat on `port`
    /// and set its 12-button mask (bit `i` = mat button `i+1`). The Family
    /// Trainer is layout-equivalent to the Power Pad and reuses its scan; this
    /// attaches the [`InputDevice::FamilyTrainer`] variant (distinct from
    /// [`Self::set_power_pad`] so the selected device round-trips through a
    /// save-state). Opt-in: the no-device path stays byte-identical.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub fn set_family_trainer(&mut self, port: usize, buttons: u16) {
        if !matches!(
            self.bus.expansion_device(port),
            Some(InputDevice::FamilyTrainer(_))
        ) {
            self.bus.set_expansion_device(
                port,
                Some(InputDevice::FamilyTrainer(
                    crate::input_device::PowerPadState::new(),
                )),
            );
        }
        self.bus.set_family_trainer(port, buttons);
    }

    /// v1.3.0 Workstream F1 — attach a **Subor keyboard** on `port` and set its
    /// pressed-key bitmap (one byte per matrix row, like
    /// [`Self::set_family_keyboard`]). The Subor keyboard reuses the Family
    /// BASIC keyboard matrix scan; this attaches the
    /// [`InputDevice::SuborKeyboard`] variant. Opt-in: the no-device path stays
    /// byte-identical.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub fn set_subor_keyboard(&mut self, port: usize, keys: [u8; 9]) {
        if !matches!(
            self.bus.expansion_device(port),
            Some(InputDevice::SuborKeyboard(_))
        ) {
            self.bus.set_expansion_device(
                port,
                Some(InputDevice::SuborKeyboard(
                    crate::input_device::FamilyKeyboardState::new(),
                )),
            );
        }
        self.bus.set_subor_keyboard(port, keys);
    }

    /// v1.3.0 Workstream F1 — attach a **Konami Hyper Shot** on `port` and set
    /// its 4-button mask (bit 0 = P1 Run, 1 = P1 Jump, 2 = P2 Run, 3 = P2 Jump).
    /// Opt-in: the no-device path stays byte-identical.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub fn set_konami_hyper_shot(&mut self, port: usize, buttons: u8) {
        if !matches!(
            self.bus.expansion_device(port),
            Some(InputDevice::KonamiHyperShot(_))
        ) {
            self.bus.set_expansion_device(
                port,
                Some(InputDevice::KonamiHyperShot(
                    crate::input_device::KonamiHyperShotState::new(),
                )),
            );
        }
        self.bus.set_konami_hyper_shot(port, buttons);
    }

    /// v1.3.0 Workstream F1 — attach a **Bandai Hyper Shot** (Exciting Boxing
    /// punching bag) on `port` and set its 8-sensor mask (bits 0..=3 = the A=0
    /// group, bits 4..=7 = the A=1 group). Opt-in: the no-device path stays
    /// byte-identical.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=1`.
    pub fn set_bandai_hyper_shot(&mut self, port: usize, sensors: u8) {
        if !matches!(
            self.bus.expansion_device(port),
            Some(InputDevice::BandaiHyperShot(_))
        ) {
            self.bus.set_expansion_device(
                port,
                Some(InputDevice::BandaiHyperShot(
                    crate::input_device::BandaiHyperShotState::new(),
                )),
            );
        }
        self.bus.set_bandai_hyper_shot(port, sensors);
    }

    /// v1.1.0 beta.1 (T-110-B4) — set (`Some`) or clear (`None`) a per-game
    /// **nametable mirroring override**, a load-time correction for ROMs whose
    /// iNES header carries the wrong mirroring flag (supplied by the frontend's
    /// game database). `None` (default) defers to the mapper — byte-identical,
    /// so the determinism / `AccuracyCoin` contract and the core test suites are
    /// unaffected (they never set it). Persisted in the save-state. Does not
    /// affect mappers with on-cart VRAM (4-screen).
    pub const fn set_mirroring_override(&mut self, m: Option<rustynes_mappers::Mirroring>) {
        self.bus.set_mirroring_override(m);
    }

    /// Write a byte directly into CPU work RAM (`$0000-$1FFF`). Used by the
    /// frontend's raw RAM cheats (GameShark-style); applied *after*
    /// [`Self::run_frame`], so the deterministic core run loop is unchanged
    /// (the determinism contract holds for the no-cheat path). No-op outside
    /// system RAM.
    pub fn poke_ram(&mut self, addr: u16, value: u8) {
        self.bus.poke_ram(addr, value);
    }

    /// v1.7.0 "Forge" Workstream A1 — debugger writeback into the PPU bus
    /// (`$0000-$3FFF`): CHR pattern bytes (mapper `ppu_write`, a no-op on
    /// CHR-ROM), nametable tiles/attributes (mapper-absorbed, else CIRAM via the
    /// active mirroring), and palette RAM. The PPU-bus counterpart of
    /// [`Self::poke_ram`].
    ///
    /// Reached only through the frontend's gated post-frame poke path (the same
    /// caller-side, after-[`Self::run_frame`] stage the raw RAM cheats use), so
    /// the deterministic core run loop is unchanged and the no-edit path is
    /// byte-identical. `debug-hooks`-gated.
    #[cfg(feature = "debug-hooks")]
    pub fn debug_poke_ppu(&mut self, addr: u16, value: u8) {
        self.bus.debug_poke_ppu(addr, value);
    }

    /// v1.7.0 "Forge" Workstream A1 — debugger writeback for one OAM byte
    /// (`idx` = 0..256: byte 0 = Y, 1 = tile, 2 = attributes, 3 = X per
    /// sprite). `debug-hooks`-gated; reached only through the gated post-frame
    /// poke path, so the default build is byte-identical.
    #[cfg(feature = "debug-hooks")]
    pub const fn poke_oam_byte(&mut self, idx: u8, value: u8) {
        self.bus.debug_poke_oam(idx, value);
    }

    /// Read a byte from the CPU address space (`$0000-$FFFF`) for inspection,
    /// **without** the register side effects of a real CPU read — reading
    /// `$2002` does not clear the VBL flag / address latch and `$2007` does not
    /// advance the PPU read buffer. Used by the debugger and the Lua scripting
    /// API (`emu.read`); it observes state without advancing the emulator,
    /// preserving determinism.
    #[must_use]
    pub fn peek(&mut self, addr: u16) -> u8 {
        self.bus.debug_peek_cpu(addr)
    }

    /// v1.2.0 C3 (hd-pack) — side-effect-free read of the PPU bus
    /// (`$0000-$3FFF`): CHR pattern data, nametables, palette RAM. Used by the
    /// HD-pack compositor to hash a tile's 16 CHR bytes. Observes state without
    /// advancing the emulator, preserving determinism.
    #[cfg(feature = "hd-pack")]
    #[must_use]
    pub fn peek_ppu(&mut self, addr: u16) -> u8 {
        self.bus.debug_peek_ppu(addr)
    }

    /// Add a Game Genie code (6 or 8 characters, case-insensitive) that
    /// substitutes a byte the CPU reads from PRG-ROM (`$8000-$FFFF`).
    ///
    /// Codes are a runtime overlay — they are **not** part of the save-state
    /// and do not perturb the determinism contract when none are active. With
    /// codes active, the substituted bytes are part of the deterministic
    /// input (record a movie with the same codes to reproduce a run).
    ///
    /// # Errors
    ///
    /// Returns [`GenieError`] if the code string cannot be decoded.
    pub fn add_genie_code(&mut self, code: &str) -> Result<(), GenieError> {
        self.bus.add_genie_code(code)
    }

    /// Remove the active Game Genie code whose canonical (upper-case) string
    /// matches `code`. No-op if no such code is active.
    pub fn remove_genie_code(&mut self, code: &str) {
        self.bus.remove_genie_code(code);
    }

    /// Remove all active Game Genie codes.
    pub fn clear_genie_codes(&mut self) {
        self.bus.clear_genie_codes();
    }

    /// Iterate the active Game Genie codes (address-sorted).
    pub fn genie_codes(&self) -> impl Iterator<Item = &GenieCode> {
        self.bus.genie_codes()
    }

    /// Drain into a slice; returns the count copied.  Excess samples are
    /// dropped if `out` is smaller than the buffered count.
    pub fn drain_audio_into(&mut self, out: &mut [f32]) -> usize {
        self.bus.drain_audio_into(out)
    }

    /// SHA-256 of the ROM bytes this emulator was constructed from.
    ///
    /// Used by the frontend's save-state file layout (one directory per
    /// ROM, keyed by hex-encoded SHA-256). The hash is computed once at
    /// `from_rom` time; subsequent calls are O(1).
    #[must_use]
    pub const fn rom_sha256(&self) -> &[u8; 32] {
        &self.rom_sha256
    }

    /// Truncated ROM hash tag stored in the save-state header.
    #[must_use]
    pub fn rom_hash_tag(&self) -> [u8; ROM_HASH_TAG_LEN] {
        let mut t = [0u8; ROM_HASH_TAG_LEN];
        t.copy_from_slice(&self.rom_sha256[..ROM_HASH_TAG_LEN]);
        t
    }

    /// Encode the entire emulator state into a `.rns` snapshot blob.
    ///
    /// Includes a versioned container header and the four chip + bus
    /// sections (`CPU `, `PPU `, `APU `, `MAP `, `BUS `), plus an optional
    /// `THM ` thumbnail section (128x120 RGBA8 nearest-neighbor downsample
    /// of the current framebuffer). The thumbnail is for UI slot pickers
    /// only -- per ADR 0003 it is NOT part of the deterministic save-state
    /// contract.
    #[must_use]
    pub fn snapshot(&self) -> Vec<u8> {
        let tag = self.rom_hash_tag();
        // The bus knows how to emit BUS / PPU / APU / MAP sections; we
        // splice the CPU section in at the end.
        let mut out = self.bus.snapshot(tag);
        let cpu_body = self.cpu.snapshot();
        save_state::write_section(
            &mut out,
            save_state::tag::CPU,
            rustynes_cpu::CPU_SNAPSHOT_VERSION,
            &cpu_body,
        );
        // Optional thumbnail. Body layout: width(u16 le) + height(u16 le) +
        // length(u32 le) + raw RGBA8. The fixed THUMBNAIL_LEN is what we
        // emit but the body carries the dimensions explicitly so future
        // bumps (different thumbnail sizes) can be detected by the reader.
        let thumb = self.thumbnail();
        let mut body = Vec::with_capacity(2 + 2 + 4 + save_state::THUMBNAIL_LEN);
        body.extend_from_slice(
            &u16::try_from(save_state::THUMBNAIL_WIDTH)
                .unwrap()
                .to_le_bytes(),
        );
        body.extend_from_slice(
            &u16::try_from(save_state::THUMBNAIL_HEIGHT)
                .unwrap()
                .to_le_bytes(),
        );
        body.extend_from_slice(&u32::try_from(thumb.len()).unwrap().to_le_bytes());
        body.extend_from_slice(&thumb);
        save_state::write_section(
            &mut out,
            save_state::tag::THM,
            save_state::THUMBNAIL_VERSION,
            &body,
        );
        out
    }

    /// v2.8.0 Phase 3 — [`Self::snapshot`] minus the `THM ` thumbnail
    /// section, encoded into a caller-owned reused buffer. The fast path
    /// for per-frame consumers (run-ahead, the netplay save-state ring):
    /// no allocation in steady state and no 61 KiB thumbnail build. The
    /// output parses with [`Self::restore`] / [`Self::restore_quiet`]
    /// exactly like a full snapshot (`THM ` is optional by format).
    pub fn snapshot_core_into(&self, out: &mut Vec<u8>) {
        let tag = self.rom_hash_tag();
        self.bus.snapshot_into(out, tag);
        let cpu_body = self.cpu.snapshot();
        save_state::write_section(
            out,
            save_state::tag::CPU,
            rustynes_cpu::CPU_SNAPSHOT_VERSION,
            &cpu_body,
        );
    }

    /// Generate a 128x120 RGBA8 thumbnail of the current framebuffer.
    ///
    /// Nearest-neighbor downsample (sample every 2nd pixel of every 2nd row).
    /// The 1/4-resolution result is small enough that storing it in slot
    /// files is cheap (61,440 bytes uncompressed, ~10-20 KiB after the
    /// LZ4 path the rewind ring uses if it is ever wired through there).
    ///
    /// Per ADR 0003: NOT part of the deterministic save-state contract.
    /// Different builds may produce different pixel-perfect framebuffers
    /// at the same cycle if post-pass filters change.
    #[must_use]
    pub fn thumbnail(&self) -> Vec<u8> {
        // Native NES framebuffer is 256x240 RGBA8 = 245,760 bytes. Source
        // stride is 256 * 4 = 1024 bytes.
        const SRC_W: usize = 256;
        let fb = self.bus.framebuffer();
        let mut out = Vec::with_capacity(save_state::THUMBNAIL_LEN);
        for ty in 0..save_state::THUMBNAIL_HEIGHT {
            let sy = ty * 2;
            for tx in 0..save_state::THUMBNAIL_WIDTH {
                let sx = tx * 2;
                let i = (sy * SRC_W + sx) * 4;
                // Source framebuffer is always at least 256*240*4 bytes
                // (allocated by Ppu::new), so this index is in-bounds.
                out.extend_from_slice(&fb[i..i + 4]);
            }
        }
        debug_assert_eq!(out.len(), save_state::THUMBNAIL_LEN);
        out
    }

    /// Extract a thumbnail from an `.rns` save-state blob without restoring
    /// it. Used by frontends to populate slot pickers.
    ///
    /// Returns `Ok(None)` if the blob is well-formed but contains no
    /// thumbnail section (older v0.9.0 slot files).
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError`] when the container header is malformed.
    pub fn extract_thumbnail(data: &[u8]) -> Result<Option<Vec<u8>>, SnapshotError> {
        let (_h, body_off) = save_state::parse_header(data)?;
        for s in save_state::SectionIter::new(&data[body_off..]) {
            let s = s?;
            if s.tag == save_state::tag::THM {
                // Body: width(u16) + height(u16) + length(u32) + bytes.
                if s.body.len() < 8 {
                    continue;
                }
                let w = u16::from_le_bytes([s.body[0], s.body[1]]) as usize;
                let h = u16::from_le_bytes([s.body[2], s.body[3]]) as usize;
                let n = u32::from_le_bytes([s.body[4], s.body[5], s.body[6], s.body[7]]) as usize;
                // Sanity: dimensions match what we currently emit, and the
                // declared length matches the body suffix.
                if w != save_state::THUMBNAIL_WIDTH
                    || h != save_state::THUMBNAIL_HEIGHT
                    || n != save_state::THUMBNAIL_LEN
                    || s.body.len() < 8 + n
                {
                    continue;
                }
                return Ok(Some(s.body[8..8 + n].to_vec()));
            }
        }
        Ok(None)
    }

    /// Apply a previously [`Self::snapshot`]ed blob.
    ///
    /// Loading from a different ROM is allowed (the embedded hash tag is
    /// only a sanity check), but the result is undefined unless the chip
    /// section bodies are appropriate for the running mapper.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError`] for malformed inputs.
    pub fn restore(&mut self, data: &[u8]) -> Result<(), SnapshotError> {
        self.restore_inner(data, true)
    }

    /// Shared restore body; `clear_rewind` distinguishes user-driven loads
    /// ([`Self::restore`] — the ring is invalidated) from same-timeline
    /// machine restores ([`Self::restore_quiet`] — the ring stays).
    fn restore_inner(&mut self, data: &[u8], clear_rewind: bool) -> Result<(), SnapshotError> {
        // Restore bus first — it consumes BUS / PPU / APU / MAP sections.
        self.bus.restore(data)?;
        // Then walk the sections again to find the CPU body.
        let (_h, body_off) = save_state::parse_header(data)?;
        let mut saw_cpu = false;
        for s in save_state::SectionIter::new(&data[body_off..]) {
            let s = s?;
            if s.tag == save_state::tag::CPU {
                if s.version != rustynes_cpu::CPU_SNAPSHOT_VERSION {
                    return Err(SnapshotError::VersionMismatch {
                        tag: save_state::tag_string(s.tag),
                        file_version: s.version,
                        chip_supports: rustynes_cpu::CPU_SNAPSHOT_VERSION,
                    });
                }
                self.cpu
                    .restore(s.body)
                    .map_err(|e| SnapshotError::SectionInvalid {
                        tag: save_state::tag_string(s.tag),
                        reason: format!("{e}"),
                    })?;
                saw_cpu = true;
            }
        }
        if !saw_cpu {
            return Err(SnapshotError::MissingSection("CPU ".into()));
        }
        // Loading invalidates the rewind ring (the new state is unrelated
        // to what was buffered before).
        if clear_rewind && let Some(r) = &mut self.rewind {
            r.clear();
        }
        Ok(())
    }

    /// v2.8.0 Phase 3 — [`Self::restore`] WITHOUT clearing the rewind ring.
    ///
    /// For internal, machine-driven restores on the same timeline —
    /// run-ahead's per-frame rollback and netplay's rollback-resimulate —
    /// where the buffered rewind history remains exactly as valid as
    /// before. User-driven loads (save-state slots) keep using
    /// [`Self::restore`], which invalidates the ring.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError`] for malformed inputs.
    pub fn restore_quiet(&mut self, data: &[u8]) -> Result<(), SnapshotError> {
        self.restore_inner(data, false)
    }

    /// Enable the rewind ring buffer with default capacity (32 MiB) and
    /// keyframe period (60).
    pub fn enable_rewind(&mut self) {
        self.enable_rewind_with(REWIND_DEFAULT_MAX_BYTES, REWIND_DEFAULT_KEYFRAME_PERIOD);
    }

    /// Enable rewind with explicit byte budget + keyframe period.
    pub fn enable_rewind_with(&mut self, max_bytes: usize, keyframe_period: u32) {
        self.rewind = Some(RewindRing::new(max_bytes, keyframe_period));
    }

    /// Disable rewind and free the buffer.
    pub fn disable_rewind(&mut self) {
        self.rewind = None;
    }

    /// Enable the per-CPU-instruction boot trace fixture with the given
    /// [`CpuBootTrace`](crate::cpu_boot_trace::CpuBootTrace).  Records past
    /// the trace's capacity are silently dropped (see
    /// [`CpuBootTrace::overflow`](crate::cpu_boot_trace::CpuBootTrace::overflow)).
    /// See `crates/rustynes-core/src/cpu_boot_trace.rs` for usage.
    #[cfg(feature = "cpu-boot-trace")]
    pub fn enable_cpu_boot_trace(&mut self, trace: crate::cpu_boot_trace::CpuBootTrace) {
        self.cpu_boot_trace = Some(trace);
    }

    /// Take the accumulated CPU boot trace, leaving the slot empty.
    /// Returns `None` if tracing was never enabled.
    #[cfg(feature = "cpu-boot-trace")]
    #[must_use]
    pub const fn take_cpu_boot_trace(&mut self) -> Option<crate::cpu_boot_trace::CpuBootTrace> {
        self.cpu_boot_trace.take()
    }

    /// Borrow the in-flight CPU boot trace for inspection.
    #[cfg(feature = "cpu-boot-trace")]
    #[must_use]
    pub const fn cpu_boot_trace(&self) -> Option<&crate::cpu_boot_trace::CpuBootTrace> {
        self.cpu_boot_trace.as_ref()
    }

    /// Snapshot the current `(CPU register file + bus cycle + PPU
    /// position + opcode preview)` tuple into the CPU boot trace.
    ///
    /// Called from `run_frame` / `step_instruction` BEFORE the
    /// `Cpu::step` call.  The opcode + 2 operand bytes are peeked
    /// side-effect-free via `LockstepBus::debug_peek_cpu` so the
    /// trace is non-perturbing.
    ///
    /// No-op if the trace was never enabled.
    #[cfg(feature = "cpu-boot-trace")]
    fn cpu_boot_trace_record(&mut self) {
        use crate::cpu_boot_trace::CpuBootRecord;
        let Some(trace) = self.cpu_boot_trace.as_mut() else {
            return;
        };
        let cycle = self.bus.cycle();
        // Range pre-check: skip the peek bookkeeping entirely if this
        // cycle is outside the configured window.  The trace's own
        // `maybe_push` re-checks; the pre-check is the hot-path
        // optimisation.
        if !trace.config().contains(cycle) {
            return;
        }
        let pc = self.cpu.pc;
        let opcode = self.bus.debug_peek_cpu(pc);
        let op1 = self.bus.debug_peek_cpu(pc.wrapping_add(1));
        let op2 = self.bus.debug_peek_cpu(pc.wrapping_add(2));
        let ppu = self.bus.ppu();
        let mut flags: u8 = 0;
        // Mesen2 exposes `cpu.nmiFlag` and `cpu.irqFlag` (its
        // own pending-interrupt latches) but not the
        // armed-vs-pending distinction; flag bit 0 means "PPU is
        // driving NMI line high" which is observable on both
        // sides at instruction-fetch boundary.
        if ppu.nmi_line() {
            flags |= 0x01;
        }
        let rec = CpuBootRecord {
            cycle,
            frame: u32::try_from(ppu.frame()).unwrap_or(u32::MAX),
            scanline: ppu.scanline(),
            dot: ppu.dot(),
            pc,
            a: self.cpu.a,
            x: self.cpu.x,
            y: self.cpu.y,
            p: self.cpu.p.bits(),
            s: self.cpu.s,
            opcode,
            op1,
            op2,
            flags,
        };
        trace.maybe_push(rec);
    }

    /// Push the current state onto the rewind ring. Frontends call this
    /// at the end of each completed frame.
    ///
    /// No-op if rewind is disabled.
    pub fn rewind_capture(&mut self) {
        if self.rewind.is_none() {
            return;
        }
        let frame = self.bus.ppu().frame();
        // v2.8.0 Phase 3 — the core fast path: no THM thumbnail (the ring
        // is never shown in a slot picker) and a reused buffer instead of
        // a fresh ~320 KiB allocation per frame. The ring still LZ4s /
        // delta-encodes the bytes itself.
        let mut buf = core::mem::take(&mut self.rewind_snap_buf);
        self.snapshot_core_into(&mut buf);
        if let Some(ring) = &mut self.rewind {
            ring.push(frame, &buf);
        }
        self.rewind_snap_buf = buf;
    }

    /// Pop the most recent rewind entry and restore it. Returns `true` on
    /// success, `false` if the ring is empty (or rewind is disabled).
    pub fn rewind_step_back(&mut self) -> bool {
        let Some(ring) = self.rewind.as_mut() else {
            return false;
        };
        let Some(result) = ring.pop_back() else {
            return false;
        };
        let bytes = match result {
            Ok(b) => b,
            Err(_e) => return false,
        };
        // Restore but keep the ring alive (don't let `restore` clear it,
        // because the user is mid-rewind).
        let saved_ring = self.rewind.take();
        let r = self.restore(&bytes);
        // Reattach the (possibly cleared, but cleared-by-us is fine) ring.
        self.rewind = saved_ring;
        r.is_ok()
    }

    /// Drop every buffered rewind entry. Called when the user releases
    /// the rewind key, so subsequent forward play overwrites — there's
    /// nothing to overwrite, but we want forward play to capture into a
    /// fresh ring rather than tail-of-old-history.
    pub fn rewind_clear(&mut self) {
        if let Some(r) = &mut self.rewind {
            r.clear();
        }
    }

    /// `true` if rewind is enabled.
    #[must_use]
    pub const fn rewind_enabled(&self) -> bool {
        self.rewind.is_some()
    }

    /// Number of buffered rewind entries.
    #[must_use]
    pub fn rewind_len(&self) -> usize {
        self.rewind.as_ref().map_or(0, RewindRing::len)
    }

    /// Approximate memory used by the rewind ring, in bytes.
    #[must_use]
    pub fn rewind_bytes_used(&self) -> usize {
        self.rewind.as_ref().map_or(0, RewindRing::bytes_used)
    }

    // -------------------------------------------------------------------
    // Debugger inspection API (Sprint 5-3). All read-only — these methods
    // MUST NOT advance emulator-visible state.
    // -------------------------------------------------------------------

    /// Snapshot the CPU register file.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // `is_jammed` is const-callable but we're const-conservative.
    pub fn cpu_snapshot(&self) -> CpuDebugView {
        let c = &self.cpu;
        CpuDebugView {
            a: c.a,
            x: c.x,
            y: c.y,
            s: c.s,
            pc: c.pc,
            p: c.p.bits(),
            jammed: c.is_jammed(),
            cycles: c.cycles,
        }
    }

    /// Snapshot PPU state for the debugger.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn ppu_snapshot(&self) -> PpuDebugView {
        let ppu = self.bus.ppu();
        let regs = ppu.debug_registers();
        let (v, t, fine_x, w) = ppu.debug_scroll();
        PpuDebugView {
            dot: ppu.dot(),
            scanline: ppu.scanline(),
            frame: ppu.frame(),
            ctrl: regs[0],
            mask: regs[1],
            status: regs[2],
            oam_addr: regs[3],
            v,
            t,
            fine_x,
            w_toggle: w,
            sprite_size_16: ppu.sprite_size_16(),
            bg_pattern_base: ppu.bg_pattern_base(),
            sprite_pattern_base: ppu.sprite_pattern_base(),
            nmi_line: ppu.nmi_line(),
        }
    }

    /// Snapshot APU channel outputs and IRQ flags.
    #[must_use]
    pub fn apu_snapshot(&self) -> ApuDebugView {
        let apu = self.bus.apu();
        ApuDebugView {
            pulse1: apu.pulse1_out(),
            pulse2: apu.pulse2_out(),
            triangle: apu.triangle_out(),
            noise: apu.noise_out(),
            dmc: apu.dmc_out(),
            frame_irq: apu.frame_irq_pending(),
            dmc_irq: apu.dmc_irq_pending(),
        }
    }

    /// Set the APU per-channel enable mask (a UI playback overlay, NOT NES
    /// hardware state). Bit 0 = pulse 1, 1 = pulse 2, 2 = triangle, 3 = noise,
    /// 4 = DMC, 5 = external/mapper audio. A cleared bit mutes that channel.
    ///
    /// The default ([`rustynes_apu::CHANNEL_MASK_ALL`]) is byte-identical to
    /// the un-masked mixer — the deterministic per-frame audio is unchanged
    /// unless the frontend explicitly mutes a channel. This is never written
    /// into the save state, so it never affects determinism or round-trips.
    pub const fn set_apu_channel_mask(&mut self, mask: u8) {
        self.bus.apu_mut().set_channel_mask(mask);
    }

    /// Current APU per-channel enable mask. See [`Self::set_apu_channel_mask`].
    #[must_use]
    pub const fn apu_channel_mask(&self) -> u8 {
        self.bus.apu().channel_mask()
    }

    /// v1.4.0 Workstream C — set the APU per-channel output gain (a UI mixing
    /// overlay, NOT NES hardware state), generalizing [`Self::set_apu_channel_mask`].
    /// Index 0 = pulse 1, 1 = pulse 2, 2 = triangle, 3 = noise, 4 = DMC,
    /// 5 = external/mapper audio. Each gain is clamped to `0.0..=2.0`.
    ///
    /// The default ([`rustynes_apu::CHANNEL_GAIN_UNITY`], all `1.0`) is
    /// byte-identical to the un-scaled mixer — the deterministic per-frame audio
    /// is unchanged unless the frontend explicitly changes a gain. Never written
    /// into the save state, so it never affects determinism or round-trips.
    pub fn set_apu_channel_gain(&mut self, gain: [f32; 6]) {
        self.bus.apu_mut().set_channel_gain(gain);
    }

    /// Current APU per-channel output gain. See [`Self::set_apu_channel_gain`].
    #[must_use]
    pub const fn apu_channel_gain(&self) -> [f32; 6] {
        self.bus.apu().channel_gain()
    }

    /// Borrow OAM (256 bytes = 64 sprites x 4 bytes).
    ///
    /// Returns a cloned `[u8; 256]` so the caller doesn't have to manage
    /// a borrow lifetime against `&self`.
    #[must_use]
    pub fn oam(&self) -> [u8; 256] {
        let mut out = [0u8; 256];
        let oam = self.bus.ppu().oam();
        out.copy_from_slice(&oam[..256]);
        out
    }

    /// One OAM byte (`index` = `0..=255`), without copying the whole 256-byte
    /// array — for single-byte readers (e.g. the Lua `memory:read_oam`) that
    /// would otherwise pay a full `oam()` copy per access. Read-only.
    #[must_use]
    pub fn oam_byte(&self, index: u8) -> u8 {
        self.bus.ppu().oam()[index as usize]
    }

    /// Borrow palette RAM (32 bytes).
    #[must_use]
    pub const fn palette_ram(&self) -> [u8; 32] {
        *self.bus.ppu().palette_ram()
    }

    /// v1.1.0 beta.1 — install (`Some`) or clear (`None`) a custom 64-entry base
    /// palette loaded from a `.pal` file. A frontend presentation override: it
    /// re-tints the displayed RGBA framebuffer via the PPU's colour LUT but does
    /// not touch any logical core state. `None` (the default) is byte-identical to
    /// the built-in palette, so `AccuracyCoin` + the commercial oracle (which never
    /// set one) are unaffected. Not part of the save-state.
    pub const fn set_custom_palette(&mut self, base: Option<[[u8; 3]; 64]>) {
        self.bus.set_custom_palette(base);
    }

    /// v1.7.0 "Forge" Workstream F3 — set the PPU extra-scanlines overclock: the
    /// number of EXTRA idle vblank scanlines the PPU inserts per frame (at the
    /// existing dot resolution, Mesen2 `UpdateTimings`). Each extra line is pure
    /// additional CPU run-time — it renders nothing, sets/clears no PPU flag, and
    /// fires no VBL/NMI/A12 event, so the visible image is unchanged. `0` (the
    /// default) is **byte-identical** to stock NES timing — `AccuracyCoin`, the
    /// commercial oracle, and nestest (which never set it) are unaffected.
    /// **Off by default**; a frontend config knob, not part of the save-state.
    /// Distinct from the CPU-multiplier overclock (a v2.0 timebase item).
    pub const fn set_extra_scanlines(&mut self, lines: u16) {
        self.bus.set_extra_scanlines(lines);
    }

    /// v1.7.0 F3 — the configured extra-scanline overclock count (`0` = stock).
    #[must_use]
    pub const fn extra_scanlines(&self) -> u16 {
        self.bus.extra_scanlines()
    }

    /// Mapper debug info (bank registers, IRQ counters, mirroring, ...).
    #[must_use]
    pub fn mapper_info(&self) -> MapperDebugView {
        self.bus.mapper_debug_info()
    }

    /// v1.4.0 Workstream C — the loaded mapper's on-cart expansion-audio chip
    /// name (e.g. `"VRC6"`, `"VRC7 (OPLL)"`, `"MMC5"`, `"Namco 163"`,
    /// `"Sunsoft 5B"`, `"FDS"`), or `None` when the board has no expansion audio
    /// (or the `mapper-audio` feature is compiled out). Used by the frontend to
    /// show the expansion-channel volume slider only when present, with a label.
    ///
    /// Discovery is dynamic: it consults the cached [`rustynes_mappers::MapperCaps`]
    /// `audio` flag (true only when the mapper overrides `mix_audio` with the
    /// feature on) and the mapper id to name the chip family.
    #[must_use]
    pub fn expansion_audio_chip(&self) -> Option<&'static str> {
        if !self.bus.mapper_caps().audio {
            return None;
        }
        let id = self.bus.mapper_debug_info().mapper_id;
        Some(match id {
            5 => "MMC5",
            19 | 210 => "Namco 163",
            20 => "FDS",
            24 | 26 => "VRC6",
            69 => "Sunsoft 5B",
            85 => "VRC7 (OPLL)",
            // The board overrides `mix_audio` but isn't one of the named
            // families above — surface a generic label so the slider still
            // appears (e.g. a future expansion-audio mapper).
            _ => "Expansion audio",
        })
    }

    /// Side-effect-free CPU bus peek (for the hex viewer).
    pub fn cpu_bus_peek(&mut self, addr: u16) -> u8 {
        self.bus.debug_peek_cpu(addr)
    }

    /// Side-effect-free PPU bus peek (for the hex viewer + visualizers).
    pub fn ppu_bus_peek(&mut self, addr: u16) -> u8 {
        self.bus.debug_peek_ppu(addr)
    }

    /// Render the 256 tiles of a CHR pattern table as RGBA8 (128x128).
    ///
    /// `table` selects which of the two pattern tables: 0 -> `$0000`,
    /// 1 -> `$1000`. Uses BG palette 0 ($3F00-$3F03) for grayscale-ish
    /// rendering. ~80 KiB cloned; only call when the PPU pattern viewer
    /// is open.
    pub fn pattern_table_rgba(&mut self, table: u8) -> Vec<u8> {
        const TILE_W: usize = 8;
        const SHEET_W: usize = 128;
        const SHEET_H: usize = 128;
        let base: u16 = if table & 1 == 0 { 0 } else { 0x1000 };
        let mut out = vec![0u8; SHEET_W * SHEET_H * 4];
        for tile_y in 0..16u16 {
            for tile_x in 0..16u16 {
                let tile_index = tile_y * 16 + tile_x;
                for row in 0..8u16 {
                    let lo = self.ppu_bus_peek(base + tile_index * 16 + row);
                    let hi = self.ppu_bus_peek(base + tile_index * 16 + row + 8);
                    for col in 0..8u16 {
                        let bit = 7 - col;
                        let p = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
                        let palette_byte = self.ppu_bus_peek(0x3F00 + u16::from(p));
                        let rgba = rustynes_ppu::nes_color_to_rgba(palette_byte & 0x3F);
                        let px = usize::from(tile_x) * TILE_W + usize::from(col);
                        let py = usize::from(tile_y) * TILE_W + usize::from(row);
                        let off = (py * SHEET_W + px) * 4;
                        out[off..off + 4].copy_from_slice(&rgba);
                    }
                }
            }
        }
        out
    }

    /// Render a nametable as RGBA8 (256x240).
    ///
    /// `nt` selects 0..=3 logical nametable. Uses the current
    /// BG pattern table base, attribute palette, and CHR data.
    pub fn nametable_rgba(&mut self, nt: u8) -> Vec<u8> {
        const FB_W: usize = 256;
        const FB_H: usize = 240;
        let nt = nt & 0x03;
        let nt_base = 0x2000u16 + u16::from(nt) * 0x400;
        let attr_base = nt_base + 0x3C0;
        let bg_base = self.bus.ppu().bg_pattern_base();
        let mut out = vec![0u8; FB_W * FB_H * 4];
        for ty in 0..30u16 {
            for tx in 0..32u16 {
                let nt_addr = nt_base + ty * 32 + tx;
                let tile_idx = self.ppu_bus_peek(nt_addr);
                let attr_addr = attr_base + (ty / 4) * 8 + (tx / 4);
                let attr_byte = self.ppu_bus_peek(attr_addr);
                let shift = ((ty & 2) << 1) | (tx & 2);
                let palette = u16::from((attr_byte >> shift) & 0x03);
                for row in 0..8u16 {
                    let lo = self.ppu_bus_peek(bg_base + u16::from(tile_idx) * 16 + row);
                    let hi = self.ppu_bus_peek(bg_base + u16::from(tile_idx) * 16 + row + 8);
                    for col in 0..8u16 {
                        let bit = 7 - col;
                        let p = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
                        let final_idx = if p == 0 {
                            self.ppu_bus_peek(0x3F00)
                        } else {
                            self.ppu_bus_peek(0x3F00 + palette * 4 + u16::from(p))
                        };
                        let rgba = rustynes_ppu::nes_color_to_rgba(final_idx & 0x3F);
                        let px = usize::from(tx * 8 + col);
                        let py = usize::from(ty * 8 + row);
                        let off = (py * FB_W + px) * 4;
                        out[off..off + 4].copy_from_slice(&rgba);
                    }
                }
            }
        }
        out
    }
}

fn sha256_of(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    let mut a = [0u8; 32];
    a.copy_from_slice(&out);
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 16-byte iNES header for a synthetic NROM ROM with `prg_kib`/`chr_kib`
    /// content, vertical mirroring.
    fn synth_nrom(prg_kib: usize, chr_kib: usize) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(16 + prg_kib * 1024 + chr_kib * 1024);
        bytes.extend_from_slice(b"NES\x1A");
        bytes.push(u8::try_from(prg_kib / 16).unwrap());
        bytes.push(u8::try_from(chr_kib / 8).unwrap());
        bytes.push(0); // flags6
        bytes.push(0); // flags7
        bytes.extend_from_slice(&[0u8; 8]);

        // PRG payload: a tiny program at $C000 that loops forever (JMP $C000).
        // Since the reset vector reads $FFFC/D, we set those bytes too.
        let mut prg = vec![0u8; prg_kib * 1024];
        if prg_kib >= 16 {
            // 16 KiB PRG: $C000-$FFFF maps to bytes 0..$4000 of PRG.
            // JMP $C000 -> $4C $00 $C0
            prg[0] = 0x4C;
            prg[1] = 0x00;
            prg[2] = 0xC0;
            // Reset vector at $FFFC/D = end-of-PRG offsets.
            let len = prg.len();
            prg[len - 4] = 0x00;
            prg[len - 3] = 0xC0;
            // NMI vector at $FFFA/B: same.
            prg[len - 6] = 0x00;
            prg[len - 5] = 0xC0;
            // IRQ vector at $FFFE/F: same.
            prg[len - 2] = 0x00;
            prg[len - 1] = 0xC0;
        }
        bytes.extend_from_slice(&prg);
        bytes.extend_from_slice(&vec![0u8; chr_kib * 1024]);
        bytes
    }

    /// Synthetic NES 2.0 NROM with console type Vs. System and a byte-13 Vs.
    /// PPU type (low nibble).
    fn synth_vs_nrom(vs_ppu_low_nibble: u8) -> Vec<u8> {
        let mut rom = synth_nrom(16, 8);
        // Upgrade the header to NES 2.0 + console type Vs. System.
        rom[7] = 0x09; // bits 2-3 = 10 (NES 2.0), bits 0-1 = 01 (Vs. System)
        rom[13] = vs_ppu_low_nibble & 0x0F;
        rom
    }

    #[test]
    fn nes_cart_4016_read_is_byte_identical_with_and_without_vs_inputs() {
        // On a normal NES cart the Vs. DIP/coin/service overlay is a no-op, so
        // a $4016/$4017 read is byte-for-byte identical regardless of the Vs.
        // input state. Compare two freshly-built buses in lockstep.
        let rom = synth_nrom(16, 8);
        let mut a = Nes::from_rom(&rom).unwrap();
        let mut b = Nes::from_rom(&rom).unwrap();
        assert!(!a.is_vs_system());
        // Crank the Vs. inputs on `b` only.
        b.set_vs_dip(0xFF);
        b.insert_coin(0);
        b.insert_coin(1);
        b.set_vs_service(true);
        for addr in [0x4016u16, 0x4017, 0x4016, 0x4017] {
            assert_eq!(
                a.bus_mut().raw_cpu_read(addr),
                b.bus_mut().raw_cpu_read(addr),
                "Vs. inputs leaked into a normal-cart read of {addr:#06X}"
            );
        }
    }

    /// v1.6.0 Workstream A3 — the `TAStudio` lag-log flag: a frame in which the
    /// program never reads `$4016`/`$4017` is a lag frame; a controller read
    /// marks the frame polled; and the flag resets at the top of each frame.
    #[cfg(feature = "debug-hooks")]
    #[test]
    fn lag_flag_tracks_controller_reads_per_frame() {
        // synth_nrom is a pure `JMP $C000` loop — it never polls input.
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).unwrap();

        // A frame of pure JMP never reads a controller port => lag frame.
        nes.run_frame();
        assert!(
            !nes.was_input_polled_this_frame(),
            "a frame with no $4016/$4017 read must be a lag frame"
        );

        // A controller-port read marks the (current) frame as polled.
        let _ = nes.bus_mut().raw_cpu_read(0x4016);
        assert!(
            nes.was_input_polled_this_frame(),
            "a $4016 read must mark the frame polled"
        );

        // $4017 also counts, and the next frame's clear resets the flag.
        nes.run_frame();
        assert!(
            !nes.was_input_polled_this_frame(),
            "the flag must reset at the top of each frame"
        );
        let _ = nes.bus_mut().raw_cpu_read(0x4017);
        assert!(
            nes.was_input_polled_this_frame(),
            "a $4017 read must also mark the frame polled"
        );
    }

    #[test]
    fn vs_dip_switches_read_through_4016_and_4017() {
        // 2C03 Vs. cart (low nibble 0).
        let rom = synth_vs_nrom(0x0);
        let mut nes = Nes::from_rom(&rom).unwrap();
        assert!(nes.is_vs_system());
        // DIP = 0b1010_1010: sw2,4,6,8 on; sw1,3,5,7 off.
        nes.set_vs_dip(0b1010_1010);
        let v16 = nes.bus_mut().raw_cpu_read(0x4016);
        // $4016: DIP sw1 -> bit3 (off), sw2 -> bit4 (on).
        assert_eq!(v16 & 0x08, 0x00, "DIP sw1 off");
        assert_eq!(v16 & 0x10, 0x10, "DIP sw2 on");
        let v17 = nes.bus_mut().raw_cpu_read(0x4017);
        // $4017: DIP sw3..8 -> bits 2..7. DIP bits 2..7 = 0b101010.
        assert_eq!(v17 & 0xFC, 0b1010_1000 & 0xFC);
    }

    #[test]
    fn vs_coin_and_service_read_through_4016() {
        let rom = synth_vs_nrom(0x0);
        let mut nes = Nes::from_rom(&rom).unwrap();
        nes.set_vs_dip(0);
        // Coin acceptor #1 -> $4016 bit 5.
        nes.insert_coin(0);
        assert_eq!(nes.bus_mut().raw_cpu_read(0x4016) & 0x20, 0x20);
        // Acceptor #2 -> bit 6.
        nes.insert_coin(1);
        assert_eq!(nes.bus_mut().raw_cpu_read(0x4016) & 0x60, 0x60);
        nes.clear_coin();
        assert_eq!(nes.bus_mut().raw_cpu_read(0x4016) & 0x60, 0x00);
        // Service button -> bit 2.
        nes.set_vs_service(true);
        assert_eq!(nes.bus_mut().raw_cpu_read(0x4016) & 0x04, 0x04);
        nes.set_vs_service(false);
        assert_eq!(nes.bus_mut().raw_cpu_read(0x4016) & 0x04, 0x00);
    }

    #[test]
    fn game_genie_substitutes_on_cpu_read_path() {
        // 16 KiB NROM; plant the Zelda code's compare byte (0x22) at the PRG
        // address it targets ($9F41 -> $8000-$BFFF window -> PRG offset $1F41).
        let mut rom = synth_nrom(16, 8);
        rom[16 + 0x1F41] = 0x22;
        let mut nes = Nes::from_rom(&rom).expect("synthetic NROM parses");

        // No codes active: reads are the original byte (determinism contract).
        assert_eq!(nes.bus_mut().debug_peek_cpu(0x9F41), 0x22);
        assert_eq!(nes.bus_mut().peek_cpu(0x9F41), 0x22);
        assert_eq!(nes.genie_codes().count(), 0);

        // 8-char code substitutes only when the original matches compare (0x22),
        // on BOTH the production read path and the debugger peek path.
        nes.add_genie_code("YYKPOYZZ").expect("valid 8-char code");
        assert_eq!(
            nes.bus_mut().debug_peek_cpu(0x9F41),
            0x77,
            "debug peek substituted"
        );
        assert_eq!(
            nes.bus_mut().peek_cpu(0x9F41),
            0x77,
            "production read substituted"
        );
        assert_eq!(
            nes.bus_mut().debug_peek_cpu(0x9F40),
            0x00,
            "other address untouched"
        );

        // Removal (case-insensitive) restores the original byte.
        nes.remove_genie_code("yykpoyzz");
        assert_eq!(nes.bus_mut().debug_peek_cpu(0x9F41), 0x22);

        // 6-char code (no compare) always substitutes; $91D9 -> data 0xAD.
        nes.add_genie_code("SXIOPO").expect("valid 6-char code");
        assert_eq!(nes.bus_mut().debug_peek_cpu(0x91D9), 0xAD);
        nes.clear_genie_codes();
        assert_eq!(nes.bus_mut().debug_peek_cpu(0x91D9), 0x00);

        // A malformed code is rejected without mutating state.
        assert!(nes.add_genie_code("BADCODE!").is_err());
    }

    #[test]
    fn poke_ram_writes_system_ram_and_ignores_rom() {
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).expect("synthetic NROM parses");
        nes.poke_ram(0x0042, 0xAB);
        assert_eq!(nes.bus_mut().debug_peek_cpu(0x0042), 0xAB);
        // Mirrored every $800 within $0000-$1FFF.
        assert_eq!(nes.bus_mut().debug_peek_cpu(0x0842), 0xAB);
        // A poke outside system RAM is a no-op (no panic; ROM space untouched).
        nes.poke_ram(0x8000, 0xFF);
        assert_ne!(nes.bus_mut().debug_peek_cpu(0x8000), 0xFF);
    }

    #[test]
    fn nes_set_buttons_then_strobe_reads_bits_in_order() {
        // T-51-005: end-to-end controller plumbing — the bus must shift the
        // latched button state out via $4016 in canonical order.
        //
        // Session-24 / Phase 3 update: `$4016` writes are now deferred
        // (committed at the next M2-low boundary inside
        // `tick_one_cpu_cycle`).  Direct-API callers that bypass CPU
        // stepping must tick the bus between the strobe pulse and the
        // shift-out reads so the buffered write commits.  Two ticks
        // are sufficient (one for the pending=1 commit, one as a
        // margin in case the test's first write landed on the pending=2
        // path).
        use rustynes_cpu::Bus as _;
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).expect("parse + boot");
        nes.set_buttons(0, Buttons::A | Buttons::SELECT | Buttons::DOWN);

        // Pulse the strobe latch (write 1 then 0 to $4016), driving the
        // bus enough cycles between writes for the deferred-write
        // commit to land.
        nes.bus_mut().cpu_write(0x4016, 1);
        nes.bus_mut().tick_one_cpu_cycle();
        nes.bus_mut().tick_one_cpu_cycle();
        nes.bus_mut().cpu_write(0x4016, 0);
        nes.bus_mut().tick_one_cpu_cycle();
        nes.bus_mut().tick_one_cpu_cycle();

        // 8 reads of $4016 should yield A, B, Select, Start, Up, Down, Left, Right.
        let expected = [1u8, 0, 1, 0, 0, 1, 0, 0];
        for &want in &expected {
            let v = nes.bus_mut().cpu_read(0x4016) & 1;
            assert_eq!(v, want);
        }
    }

    #[test]
    fn nes_set_buttons_port1_reads_via_4017_in_order() {
        // T-71-004 (Phase 7): player 2 plumbing. The strobe latch is shared
        // (writing `$4016` strobes BOTH pads); player 2 shifts out on `$4017`.
        // Mirrors `nes_set_buttons_then_strobe_reads_bits_in_order` for port 1.
        use rustynes_cpu::Bus as _;
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).expect("parse + boot");
        nes.set_buttons(1, Buttons::B | Buttons::START | Buttons::RIGHT);

        nes.bus_mut().cpu_write(0x4016, 1);
        nes.bus_mut().tick_one_cpu_cycle();
        nes.bus_mut().tick_one_cpu_cycle();
        nes.bus_mut().cpu_write(0x4016, 0);
        nes.bus_mut().tick_one_cpu_cycle();
        nes.bus_mut().tick_one_cpu_cycle();

        // A, B, Select, Start, Up, Down, Left, Right.
        let expected = [0u8, 1, 0, 1, 0, 0, 0, 1];
        for (i, &want) in expected.iter().enumerate() {
            let v = nes.bus_mut().cpu_read(0x4017) & 1;
            assert_eq!(v, want, "$4017 read #{i}");
        }
    }

    #[test]
    fn nes_restrobe_relatches_current_buttons() {
        // T-71-004 (Phase 7): a fresh strobe re-samples the live button state
        // through the full bus (the per-`Controller` unit test in
        // `controller.rs` covers this at the chip level; this confirms it end
        // to end via `Nes::set_buttons` + `$4016`).
        use rustynes_cpu::Bus as _;
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).expect("parse + boot");

        let strobe = |nes: &mut Nes| {
            nes.bus_mut().cpu_write(0x4016, 1);
            nes.bus_mut().tick_one_cpu_cycle();
            nes.bus_mut().tick_one_cpu_cycle();
            nes.bus_mut().cpu_write(0x4016, 0);
            nes.bus_mut().tick_one_cpu_cycle();
            nes.bus_mut().tick_one_cpu_cycle();
        };

        nes.set_buttons(0, Buttons::A);
        strobe(&mut nes);
        assert_eq!(nes.bus_mut().cpu_read(0x4016) & 1, 1, "A latched pressed");

        // Change state, then re-strobe: the new state must be visible.
        nes.set_buttons(0, Buttons::empty());
        strobe(&mut nes);
        assert_eq!(nes.bus_mut().cpu_read(0x4016) & 1, 0, "A latched released");
    }

    #[test]
    fn reading_4015_does_not_refresh_external_open_bus() {
        // T-72-006 (Phase 7): `$4015` reads return the APU status but do NOT
        // drive the external data bus (the APU status port is internal to the
        // 2A03 package). So a `$4015` read must leave the open-bus latch
        // unchanged — a subsequent open-bus-region read returns the prior
        // floating value, not the APU status. Per nesdev "Open bus behavior"
        // + AccuracyCoin `CPU Behavior :: Open Bus` Test 7.
        use rustynes_cpu::Bus as _;
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).expect("parse + boot");

        // Drive a known value onto the external bus via a normal RAM read.
        nes.bus_mut().cpu_write(0x0010, 0xAB);
        assert_eq!(nes.bus_mut().cpu_read(0x0010), 0xAB);
        // $4018-$401F is open-bus region: returns (and re-latches) the value.
        assert_eq!(
            nes.bus_mut().cpu_read(0x4018),
            0xAB,
            "open-bus latch holds 0xAB"
        );

        // Read $4015 — must NOT refresh the external latch.
        let _ = nes.bus_mut().cpu_read(0x4015);

        // The latch is still 0xAB, not whatever APU status $4015 returned.
        assert_eq!(
            nes.bus_mut().cpu_read(0x4018),
            0xAB,
            "$4015 read must not drive the external data bus"
        );
    }

    #[test]
    fn nes_from_rom_constructs_and_resets() {
        let rom = synth_nrom(16, 8);
        let nes = Nes::from_rom(&rom).expect("parse + boot");
        assert_eq!(nes.cpu().pc, 0xC000);
    }

    #[test]
    fn power_on_randomization_is_opt_in_seeded_and_deterministic() {
        // T-72-005 (Phase 7): the default path leaves work RAM zeroed; the
        // seeded constructor randomizes it deterministically.
        let rom = synth_nrom(16, 8);

        // Default: RAM is zeroed.
        let mut default = Nes::from_rom(&rom).expect("parse + boot");
        for addr in (0x0000u16..0x0800).step_by(0x40) {
            assert_eq!(default.cpu_bus_peek(addr), 0, "default RAM must be zero");
        }

        // Seeded: RAM is not all-zero.
        let mut a = Nes::from_rom_with_power_on_seed(&rom, 1).expect("parse + boot");
        let dump_a: Vec<u8> = (0x0000u16..0x0100).map(|x| a.cpu_bus_peek(x)).collect();
        assert!(
            dump_a.iter().any(|&b| b != 0),
            "seeded RAM must not be all zero"
        );

        // Same seed -> identical RAM.
        let mut a2 = Nes::from_rom_with_power_on_seed(&rom, 1).expect("parse + boot");
        let dump_a2: Vec<u8> = (0x0000u16..0x0100).map(|x| a2.cpu_bus_peek(x)).collect();
        assert_eq!(
            dump_a, dump_a2,
            "same seed must yield identical power-on RAM"
        );

        // Different seed -> different RAM.
        let mut b = Nes::from_rom_with_power_on_seed(&rom, 0xDEAD_BEEF).expect("parse + boot");
        let dump_b: Vec<u8> = (0x0000u16..0x0100).map(|x| b.cpu_bus_peek(x)).collect();
        assert_ne!(dump_a, dump_b, "different seeds should differ");
    }

    #[test]
    fn nes_run_frame_completes_and_returns_framebuffer() {
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).expect("parse + boot");
        let fb = nes.run_frame();
        assert_eq!(fb.len(), 256 * 240 * 4);
    }

    #[test]
    fn nes_run_two_frames_distinct_completion_latches() {
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).expect("parse + boot");
        nes.run_frame();
        let cycles_after_one = nes.cycle();
        nes.run_frame();
        let cycles_after_two = nes.cycle();
        assert!(cycles_after_two > cycles_after_one);
    }

    #[test]
    fn nes_determinism_two_runs_match() {
        // T-24-002: same ROM + zero input + 60 frames -> bit-identical
        // framebuffer hash via FNV-1a.
        fn hash_fb(fb: &[u8]) -> u64 {
            let mut h: u64 = 0xCBF2_9CE4_8422_2325;
            for &b in fb {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            h
        }
        let rom = synth_nrom(16, 8);
        let mut a = Nes::from_rom(&rom).unwrap();
        let mut b = Nes::from_rom(&rom).unwrap();
        let frames = 4;
        let mut hash_a = 0u64;
        let mut hash_b = 0u64;
        for _ in 0..frames {
            hash_a = hash_fb(a.run_frame());
            hash_b = hash_fb(b.run_frame());
        }
        assert_eq!(
            hash_a, hash_b,
            "two runs must produce identical framebuffer"
        );
    }

    fn fnv_hash(bytes: &[u8]) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }

    #[test]
    fn snapshot_round_trip_preserves_framebuffer_and_cycle() {
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).expect("parse + boot");
        for _ in 0..4 {
            nes.run_frame();
        }
        let cycle = nes.cycle();
        let fb_hash_before = fnv_hash(nes.framebuffer());
        let blob = nes.snapshot();

        // Drift the emulator forward 4 more frames so it looks different.
        for _ in 0..4 {
            nes.run_frame();
        }
        assert_ne!(nes.cycle(), cycle, "drift must move us off the snapshot");

        nes.restore(&blob).expect("restore");
        assert_eq!(nes.cycle(), cycle);
        assert_eq!(fnv_hash(nes.framebuffer()), fb_hash_before);
    }

    #[test]
    fn snapshot_is_deterministic_across_two_runs() {
        let rom = synth_nrom(16, 8);
        let mut a = Nes::from_rom(&rom).unwrap();
        let mut b = Nes::from_rom(&rom).unwrap();
        for _ in 0..3 {
            a.run_frame();
            b.run_frame();
        }
        assert_eq!(a.snapshot(), b.snapshot());
    }

    #[test]
    fn snapshot_header_carries_rom_hash_tag() {
        let rom = synth_nrom(16, 8);
        let nes = Nes::from_rom(&rom).unwrap();
        let blob = nes.snapshot();
        let (h, _off) = save_state::parse_header(&blob).unwrap();
        assert_eq!(h.rom_hash_tag, nes.rom_hash_tag());
    }

    #[test]
    fn rewind_step_back_restores_prior_frame() {
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).unwrap();
        nes.enable_rewind_with(2 * 1024 * 1024, 1);
        for _ in 0..6 {
            nes.run_frame();
        }
        let cycle_at_6 = nes.cycle();
        nes.run_frame();
        nes.run_frame();
        nes.run_frame();
        // 3 entries on the ring (frames 6..=8 captured at the END of each
        // run_frame — frame 5 was captured in the loop above).
        assert!(nes.rewind_step_back(), "first step back");
        assert!(nes.rewind_step_back(), "second step back");
        assert!(nes.rewind_step_back(), "third step back");
        // We've rewound past the 3 extra frames; cycle should equal the
        // state we captured at the end of frame 6 (i.e. frame 5's snap).
        assert_ne!(nes.cycle(), cycle_at_6, "captured frame 5, not frame 6");
    }

    #[test]
    fn rewind_disabled_no_op() {
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).unwrap();
        nes.run_frame();
        assert!(!nes.rewind_step_back());
        assert_eq!(nes.rewind_len(), 0);
    }

    #[cfg(feature = "debug-hooks")]
    #[test]
    fn breakpoint_stops_run_frame_at_pc() {
        let rom = synth_nrom(16, 8);
        // A PC the CPU provably reaches: the PC after the first 3 executed
        // instructions (a fresh run replays the same deterministic sequence).
        let mut probe = Nes::from_rom(&rom).expect("parse");
        for _ in 0..3 {
            probe.step_instruction();
        }
        let target = probe.cpu.pc;

        let mut nes = Nes::from_rom(&rom).expect("parse");
        // The PPU's frame-complete latch is set at power-on, so the first
        // `run_frame` returns immediately without iterating; warm past it.
        let _ = nes.run_frame();
        nes.add_breakpoint(target);
        nes.add_breakpoint(target); // idempotent
        assert_eq!(nes.breakpoints(), &[target]);

        let _ = nes.run_frame();
        assert_eq!(
            nes.take_break_hit(),
            Some(target),
            "stops at the breakpoint PC"
        );
        assert_eq!(nes.take_break_hit(), None, "hit cleared on read");

        // Resuming ("continue") steps past the stopped PC instead of
        // re-breaking in place, then hits the same PC again next loop.
        let _ = nes.run_frame();
        assert_eq!(
            nes.take_break_hit(),
            Some(target),
            "resume steps past, then re-hits on the next pass"
        );

        // Disarmed breakpoints don't fire (the frame runs to completion).
        nes.set_breakpoints_enabled(false);
        let _ = nes.run_frame();
        assert_eq!(nes.take_break_hit(), None, "disarmed: no break");

        // Removal empties the list.
        nes.remove_breakpoint(target);
        assert!(nes.breakpoints().is_empty());

        // Regression (gemini #41): a breakpoint sitting at the frame's STARTING
        // PC must fire immediately — the old `first_iter` skip missed it.
        let mut nes2 = Nes::from_rom(&rom).expect("parse");
        let _ = nes2.run_frame(); // warm past the power-on frame-complete latch
        let start_pc = nes2.cpu.pc;
        nes2.add_breakpoint(start_pc);
        let _ = nes2.run_frame();
        assert_eq!(
            nes2.take_break_hit(),
            Some(start_pc),
            "breaks immediately when starting already on a breakpoint"
        );
    }

    #[cfg(feature = "debug-hooks")]
    #[test]
    fn trace_logger_records_while_enabled() {
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).expect("parse");
        let _ = nes.run_frame(); // warm past the power-on frame-complete latch.
        assert_eq!(nes.trace_len(), 0, "off by default");
        nes.set_trace_enabled(true);
        let _ = nes.run_frame();
        assert!(nes.trace_len() > 0, "records while enabled");
        // Records carry the executed PCs (the synth ROM spins at $C000).
        let recs = nes.trace_records();
        assert!(recs.iter().any(|r| r.pc == 0xC000), "captured the loop PC");
        // The tail copy is bounded.
        assert!(nes.trace_tail_vec(4).len() <= 4);
        // Disabling stops growth; clearing empties.
        nes.set_trace_enabled(false);
        let n = nes.trace_len();
        let _ = nes.run_frame();
        assert_eq!(nes.trace_len(), n, "no new records when disabled");
        nes.clear_trace();
        assert_eq!(nes.trace_len(), 0);
    }

    #[cfg(feature = "debug-hooks")]
    #[test]
    fn event_viewer_records_writes_with_ppu_position() {
        use crate::bus::EventKind;
        // A tiny NROM that loops `LDA #$00 ; STA $2000 ; JMP $C000`, so it
        // generates a PPU-register write ($2000) every iteration.
        let mut bytes = alloc::vec![0u8; 16 + 16 * 1024];
        bytes[0..4].copy_from_slice(b"NES\x1A");
        bytes[4] = 1; // 1x16KB PRG
        bytes[5] = 1; // 1x8KB CHR (unused here)
        let prg = &mut bytes[16..16 + 16 * 1024];
        // $C000 maps to PRG offset 0.
        prg[0..8].copy_from_slice(&[0xA9, 0x00, 0x8D, 0x00, 0x20, 0x4C, 0x00, 0xC0]);
        let len = 16 * 1024;
        prg[len - 4] = 0x00; // reset vector lo
        prg[len - 3] = 0xC0; // reset vector hi -> $C000
        // CHR not appended (header says 1 bank but parse tolerates; use 0 banks).
        bytes[5] = 0;

        let mut nes = Nes::from_rom(&bytes).expect("parse");
        let _ = nes.run_frame(); // warm past the power-on frame-complete latch.
        assert!(nes.events().is_empty(), "off by default");
        nes.set_event_logging(true);
        let _ = nes.run_frame();
        let evs = nes.events();
        assert!(!evs.is_empty(), "the STA $2000 loop produces writes");
        assert!(
            evs.iter().all(|e| e.kind == EventKind::PpuWrite
                && e.addr == 0x2000
                && e.dot <= 340
                && e.value == 0x00),
            "all events are $2000 PPU writes of $00 with a sane dot"
        );
        // Reset per frame: the count stays one-frame-bounded. The event log is
        // capped at `EVENT_CAP` (20_000, private to the bus module) — distinct
        // from the looser instruction-trace `TRACE_CAP` — so assert that bound.
        let _ = nes.run_frame();
        assert!(nes.events().len() <= 20_000, "bounded by EVENT_CAP");
        nes.set_event_logging(false);
        assert!(!nes.event_logging());
    }

    #[cfg(feature = "debug-hooks")]
    #[test]
    fn event_viewer_records_ppu_reads() {
        use crate::bus::EventKind;
        // A tiny NROM that loops `LDA $2002 ; JMP $C000`, generating a PPU
        // STATUS read ($2002) every iteration (v1.5.0 Workstream A2 read tap).
        let mut bytes = alloc::vec![0u8; 16 + 16 * 1024];
        bytes[0..4].copy_from_slice(b"NES\x1A");
        bytes[4] = 1; // 1x16KB PRG
        let prg = &mut bytes[16..16 + 16 * 1024];
        // $C000: LDA $2002 ; JMP $C000
        prg[0..6].copy_from_slice(&[0xAD, 0x02, 0x20, 0x4C, 0x00, 0xC0]);
        let len = 16 * 1024;
        prg[len - 4] = 0x00;
        prg[len - 3] = 0xC0;

        let mut nes = Nes::from_rom(&bytes).expect("parse");
        let _ = nes.run_frame();
        nes.set_event_logging(true);
        let _ = nes.run_frame();
        let evs = nes.events();
        assert!(
            evs.iter()
                .any(|e| e.kind == EventKind::PpuRead && e.addr == 0x2002),
            "the LDA $2002 loop produces PPU reads"
        );
        assert!(
            evs.iter()
                .all(|e| e.kind.is_read() == (e.kind == EventKind::PpuRead)),
            "is_read is true only for PpuRead"
        );
        nes.set_event_logging(false);
    }

    #[cfg(feature = "debug-hooks")]
    #[test]
    fn event_breakpoint_fires_on_armed_category_only() {
        use crate::EventBpKind;
        // The same `LDA #$00 ; STA $2000 ; JMP $C000` loop — it issues a PPU
        // write every iteration but never an APU write or interrupt service.
        let mut bytes = alloc::vec![0u8; 16 + 16 * 1024];
        bytes[0..4].copy_from_slice(b"NES\x1A");
        bytes[4] = 1;
        bytes[5] = 0;
        let prg = &mut bytes[16..16 + 16 * 1024];
        prg[0..8].copy_from_slice(&[0xA9, 0x00, 0x8D, 0x00, 0x20, 0x4C, 0x00, 0xC0]);
        let len = 16 * 1024;
        prg[len - 4] = 0x00;
        prg[len - 3] = 0xC0;

        let mut nes = Nes::from_rom(&bytes).expect("parse");
        let _ = nes.run_frame(); // warm past the power-on frame-complete latch.

        // Default: nothing armed, no hits.
        assert_eq!(nes.event_breakpoints(), 0, "disarmed by default");
        let _ = nes.run_frame();
        assert_eq!(nes.take_event_break_hit(), None, "no hit while disarmed");

        // Arm an UNRELATED category (APU write): the $2000 loop never trips it.
        nes.set_event_breakpoints(EventBpKind::ApuWrite.bit());
        let _ = nes.run_frame();
        assert_eq!(
            nes.take_event_break_hit(),
            None,
            "wrong category does not fire"
        );

        // Arm PPU write: the very next frame must latch a hit with sane context.
        nes.set_event_breakpoints(EventBpKind::PpuWrite.bit());
        let _ = nes.run_frame();
        let hit = nes.take_event_break_hit().expect("PPU write fires");
        assert_eq!(hit.kind, EventBpKind::PpuWrite);
        assert_eq!(hit.addr, 0x2000, "the STA $2000 target");
        assert!(hit.dot <= 340, "dot in range");
        assert!(
            hit.scanline >= -1 && hit.scanline <= 260,
            "scanline in range"
        );
        // Cleared on read + only one (first) hit recorded per frame.
        assert_eq!(nes.take_event_break_hit(), None, "cleared on read");

        // Disarming all stops it firing again.
        nes.set_event_breakpoints(0);
        let _ = nes.run_frame();
        assert_eq!(nes.take_event_break_hit(), None, "disarmed: silent");
    }

    #[cfg(feature = "debug-hooks")]
    #[test]
    fn event_bp_kind_mask_and_labels_are_distinct() {
        use crate::EventBpKind;
        let all = EventBpKind::all();
        // Every category has a distinct bit and a non-empty label.
        let mut seen = 0u16;
        for k in all {
            assert_eq!(seen & k.bit(), 0, "{} bit collides", k.label());
            seen |= k.bit();
            assert!(!k.label().is_empty());
        }
        assert_eq!(seen.count_ones() as usize, all.len(), "11 distinct bits");
    }

    #[test]
    fn debug_snapshots_are_read_only() {
        // T-53-002+ -- inspection must not advance emulator state.
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).expect("parse + boot");
        for _ in 0..2 {
            nes.run_frame();
        }
        let cycle_before = nes.cycle();
        let _cpu = nes.cpu_snapshot();
        let _ppu = nes.ppu_snapshot();
        let _apu = nes.apu_snapshot();
        let _oam = nes.oam();
        let _pal = nes.palette_ram();
        let _mapper = nes.mapper_info();
        // cpu_bus_peek and pattern_table_rgba take &mut so we exercise them too.
        let _byte = nes.cpu_bus_peek(0xC000);
        let _byte = nes.ppu_bus_peek(0x2000);
        let pt = nes.pattern_table_rgba(0);
        assert_eq!(pt.len(), 128 * 128 * 4, "pattern table RGBA size");
        let nt = nes.nametable_rgba(0);
        assert_eq!(nt.len(), 256 * 240 * 4, "nametable RGBA size");
        assert_eq!(nes.cycle(), cycle_before, "inspection MUST NOT tick CPU");
    }

    #[test]
    fn disassembler_round_trips_against_cpu_bus() {
        // Walk a small synthesized program through the disassembler.
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).expect("parse + boot");
        let pc = nes.cpu().pc;
        // Take a fixed-size byte window via the peek API first; disasm
        // wants a `Fn`, and our peek is `FnMut`.
        let mut buf = [0u8; 16];
        for (i, b) in buf.iter_mut().enumerate() {
            *b = nes.cpu_bus_peek(pc.wrapping_add(u16::try_from(i).unwrap_or(0)));
        }
        let lines = rustynes_cpu::disassemble_at(
            |a| {
                let off = a.wrapping_sub(pc) as usize;
                buf.get(off).copied().unwrap_or(0)
            },
            pc,
            4,
        );
        assert_eq!(lines.len(), 4);
        // First instruction is JMP $C000 (0x4C 0x00 0xC0).
        assert_eq!(lines[0].addr, pc);
        assert_eq!(lines[0].mnemonic, "JMP");
    }

    #[test]
    fn rom_sha256_is_deterministic() {
        let rom = synth_nrom(16, 8);
        let nes_a = Nes::from_rom(&rom).unwrap();
        let nes_b = Nes::from_rom(&rom).unwrap();
        assert_eq!(nes_a.rom_sha256(), nes_b.rom_sha256());
        // Different ROM -> different hash.
        let mut other = synth_nrom(16, 8);
        other[0x10] = 0x99;
        let nes_c = Nes::from_rom(&other).unwrap();
        assert_ne!(nes_a.rom_sha256(), nes_c.rom_sha256());
    }

    #[test]
    fn thumbnail_has_expected_dimensions() {
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).expect("parse + boot");
        nes.run_frame();
        let thumb = nes.thumbnail();
        assert_eq!(thumb.len(), save_state::THUMBNAIL_LEN);
        assert_eq!(
            save_state::THUMBNAIL_LEN,
            save_state::THUMBNAIL_WIDTH * save_state::THUMBNAIL_HEIGHT * 4
        );
    }

    #[test]
    fn snapshot_includes_thumbnail_section_extractable() {
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).expect("parse + boot");
        for _ in 0..2 {
            nes.run_frame();
        }
        let blob = nes.snapshot();
        let extracted = Nes::extract_thumbnail(&blob).expect("blob is valid");
        let thumb = extracted.expect("snapshot must include THM section");
        assert_eq!(thumb.len(), save_state::THUMBNAIL_LEN);
        // Round-trip: thumbnail bytes must match the live framebuffer
        // downsample taken at the same cycle.
        assert_eq!(thumb, nes.thumbnail());
    }

    #[test]
    fn snapshot_round_trip_still_works_with_thumbnail() {
        // ADR-0003 invariant: adding THM must not perturb deterministic
        // restore. Re-runs the snapshot_round_trip test with the new
        // thumbnail section present in the blob.
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).expect("parse + boot");
        for _ in 0..4 {
            nes.run_frame();
        }
        let cycle = nes.cycle();
        let fb_hash_before = fnv_hash(nes.framebuffer());
        let blob = nes.snapshot();
        for _ in 0..4 {
            nes.run_frame();
        }
        assert_ne!(nes.cycle(), cycle);
        nes.restore(&blob)
            .expect("restore must succeed with THM present");
        assert_eq!(nes.cycle(), cycle);
        assert_eq!(fnv_hash(nes.framebuffer()), fb_hash_before);
    }

    #[test]
    fn restore_accepts_v0_9_0_blob_without_thumbnail() {
        // ADR-0003 invariant: older slot files without a THM section must
        // still restore. Simulate a v0.9.0 blob by stripping the THM
        // section out of a freshly-emitted snapshot.
        let rom = synth_nrom(16, 8);
        let mut nes = Nes::from_rom(&rom).expect("parse + boot");
        for _ in 0..3 {
            nes.run_frame();
        }
        let cycle = nes.cycle();
        let fb_hash = fnv_hash(nes.framebuffer());
        let with_thumb = nes.snapshot();

        // Reconstruct a blob without the THM section.
        let (_h, body_off) = save_state::parse_header(&with_thumb).unwrap();
        let mut without_thumb = with_thumb[..body_off].to_vec();
        for s in save_state::SectionIter::new(&with_thumb[body_off..]) {
            let s = s.unwrap();
            if s.tag == save_state::tag::THM {
                continue;
            }
            save_state::write_section(&mut without_thumb, s.tag, s.version, s.body);
        }
        assert!(without_thumb.len() < with_thumb.len());
        // Extract on the v0.9.0-shaped blob returns None for the thumbnail.
        let extracted = Nes::extract_thumbnail(&without_thumb).unwrap();
        assert!(extracted.is_none(), "v0.9.0 blob has no THM section");

        // Drift then restore from the v0.9.0-shaped blob.
        for _ in 0..2 {
            nes.run_frame();
        }
        nes.restore(&without_thumb)
            .expect("v0.9.0 blob must restore");
        assert_eq!(nes.cycle(), cycle);
        assert_eq!(fnv_hash(nes.framebuffer()), fb_hash);
    }

    /// Build a minimal NSF (3 songs) whose `init` enables all APU channels and
    /// programs a steady pulse-1 tone, and whose `play` is a bare `RTS`. Loaded
    /// at $8000; init=$8000, play=$800C.
    fn synth_tone_nsf() -> Vec<u8> {
        let mut f = vec![0u8; 0x80];
        f[0..5].copy_from_slice(b"NESM\x1A");
        f[0x05] = 1; // version
        f[0x06] = 3; // total songs
        f[0x07] = 1; // starting song
        f[0x08] = 0x00;
        f[0x09] = 0x80; // load $8000
        f[0x0A] = 0x00;
        f[0x0B] = 0x80; // init $8000
        f[0x0C] = 0x0C;
        f[0x0D] = 0x80; // play $800C
        let program: &[u8] = &[
            // init ($8000): enable channels + a constant-volume pulse-1 tone.
            0xA9, 0x0F, 0x8D, 0x15, 0x40, // LDA #$0F; STA $4015
            0xA9, 0xBF, 0x8D, 0x00, 0x40, // LDA #$BF; STA $4000 (duty/const vol)
            0x60, // RTS
            0xA0, // padding so play lands at $800C
            // play ($800C):
            0x60, // RTS
        ];
        f.extend_from_slice(program);
        f
    }

    #[test]
    fn nsf_constructs_runs_and_selects_tracks() {
        let mut nes = Nes::from_nsf(&synth_tone_nsf()).expect("valid nsf builds");
        assert_eq!(nes.nsf_song_count(), 3);
        assert_eq!(nes.nsf_current_song(), 0);

        // Run several frames: the driver's reset vector runs `init` (enabling
        // the APU + pulse-1), then vblank NMI calls `play` each frame. Audio
        // must be produced and the run must not panic.
        let mut produced = 0usize;
        for _ in 0..8 {
            nes.run_frame();
            produced += nes.drain_audio().len();
        }
        assert!(produced > 0, "NSF playback must produce audio samples");

        // Track select clamps + restarts on the new song.
        nes.nsf_set_song(2);
        assert_eq!(nes.nsf_current_song(), 2);
        nes.run_frame();
        nes.nsf_set_song(99);
        assert_eq!(nes.nsf_current_song(), 2, "clamped to last song");
    }

    #[test]
    fn nsf_song_apis_are_inert_on_a_cartridge() {
        let mut nes = Nes::from_rom(&synth_nrom(16, 8)).expect("nrom builds");
        assert_eq!(nes.nsf_song_count(), 0);
        assert_eq!(nes.nsf_current_song(), 0);
        nes.nsf_set_song(1); // no-op, must not panic or reset spuriously
        assert_eq!(nes.nsf_current_song(), 0);
    }
}

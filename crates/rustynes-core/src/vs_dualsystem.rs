//! Vs. `DualSystem` — two complete NES systems in one arcade cabinet
//! (v2.0.0 beta.5, Workstream C of the "Timebase" plan).
//!
//! The `DualSystem` boards (Vs. Tennis, Vs. Mahjong, Vs. Wrecking Crew,
//! Vs. Balloon Fight) carry **two CPUs, two PPUs, and two work RAMs**,
//! sharing a small inter-CPU communication signal, a 2 KiB work RAM, and
//! the coin/DIP panel — each half drives its own screen. `RustyNES` models
//! this as a wrapper over two byte-identical [`Nes`] instances:
//!
//! ```text
//! VsDualSystem
//! ├── main: Nes   (the primary cabinet half; $4016 bit 7 reads 0)
//! ├── sub:  Nes   (the secondary half;      $4016 bit 7 reads 0x80)
//! └── the comms latch + shared-WRAM ownership (wrapper-owned)
//! ```
//!
//! **The wrapper owns ALL cross-wiring** (the design rule from
//! `docs/audit/vs-dualsystem-design-2026-06-11.md`): the two buses never
//! hold references to each other. Each bus only *records* its `$4016`
//! bit-1 (main/sub comms signal) levels and *accepts* an external-IRQ
//! level; the wrapper polls the levels after every stepped instruction
//! and applies the protocol (IRQ wiring per Mesen2
//! `Core/NES/Mappers/VsSystem/VsControlManager.cpp`; memory model per
//! MAME `src/mame/nintendo/vsnes.cpp`, where the four `DualSystem` games
//! verifiably run):
//!
//! - a `$4016` bit-1 write going **LOW asserts the PARTNER console's
//!   external `/IRQ`**; going high clears it (`UpdateMainSubBit`; MAME:
//!   `cpu.set_input_line(0, (data & 2) ? CLEAR_LINE : ASSERT_LINE)`);
//! - the shared 2 KiB WRAM at `$6000-$67FF` (mirrored ×4 across the 8 KiB
//!   window) is **simultaneously visible to both CPUs** — MAME maps ONE
//!   RAM `.share("nvram")` into both address spaces with no access mux.
//!   (nesdev/Mesen2 document a `$4016`-bit-1 access mux instead, but
//!   Balloon Fight's boot handshake polls a mailbox the partner writes
//!   while the mux would deny it access — under exclusive routing the
//!   boot provably deadlocks, so the MAME model is adopted.) Realized as
//!   two per-console copies converged by draining each mapper's write log
//!   into the partner after every stepped instruction;
//! - coins 1/2 + service drive main; coins 3/4 drive sub;
//! - each console owns its own DIP bank (Mesen2's `dipSwitches >> 8` /
//!   MAME's `DSW0`/`DSW1` for the sub is realized structurally: two
//!   buses, two `vs_dip` bytes).
//!
//! **Stepping** mirrors Mesen2 `NesConsole::RunFrame` +
//! `RunVsSubConsole`: the main console steps one instruction, then the
//! sub runs until it is within a **5-CPU-cycle gap** of the main (or has
//! caught up to the main's frame count) — a *soft* lockstep. Both
//! consoles run the same deterministic one-clock core, so the interleave
//! is reproducible run-to-run (the determinism contract holds per
//! console; the 5-cycle tolerance is the documented coupling knob).
//!
//! **Out of scope by design** (stated in the plan + the design doc):
//! netplay (rollback assumes one state blob) and `RetroAchievements` (one
//! memory map) do not support the dual path. Audio: the frontend drains
//! the MAIN console's mixer (each cabinet half has its own speaker; the
//! sub's audio is synthesized but undrained until a frontend feature
//! surfaces it).

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::nes::Nes;
use crate::save_state::SnapshotError;
use rustynes_mappers::RomError;

/// Container magic for the dual-system save-state (see
/// [`VsDualSystem::snapshot`]).
const SNAPSHOT_MAGIC: [u8; 4] = *b"RVSD";
/// Dual-container layout version.
const SNAPSHOT_VERSION: u16 = 1;

/// Two complete NES systems + the cabinet's cross-wiring. See the module
/// docs for the architecture and protocol.
pub struct VsDualSystem {
    main: Nes,
    sub: Nes,
    /// The MAIN console's last-applied `$4016` bit-1 level (the sub's
    /// `/IRQ` driver).
    main_bit1: bool,
    /// The SUB console's last-applied `$4016` bit-1 level (the main's
    /// `/IRQ` driver).
    sub_bit1: bool,
    /// Reusable scratch buffer for `pump_comms`'s shared-WRAM drain, in
    /// both directions. `Vec::drain` on the mapper side keeps the log's
    /// OWN capacity; this buffer keeps the WRAPPER side allocation-free
    /// too, once warmed up — `pump_comms` runs after every stepped
    /// instruction on a `DualSystem` cart, so a per-call heap allocation
    /// here would be a real hot-path cost, not a theoretical one.
    comms_scratch: Vec<(u16, u8)>,
}

impl VsDualSystem {
    /// Construct the dual system from one ROM image. A proper `DualSystem`
    /// dump carries BOTH CPUs' programs (64 KiB PRG: main half then sub
    /// half — MAME's `prg` + `sub` regions; Mesen2's `prgOuter` split);
    /// the sub console's mapper banks the second half. A 32 KiB
    /// (main-half-only) dump constructs and runs, but its boot handshake
    /// cannot complete — the sub-CPU program is simply absent.
    ///
    /// The caller has already determined the cart is a `DualSystem` board
    /// (the SHA-keyed `vs_db` `dual_system` flag — see [`crate::Emu`]).
    ///
    /// # Errors
    ///
    /// Returns the underlying [`RomError`] if the bytes don't parse.
    pub fn from_rom(bytes: &[u8]) -> Result<Self, RomError> {
        let main = Nes::from_rom(bytes)?;
        let sub = Nes::from_rom(bytes)?;
        let mut dual = Self {
            main,
            sub,
            main_bit1: false,
            sub_bit1: false,
            comms_scratch: Vec::new(),
        };
        // Cabinet wiring: mark the sub half (its $4016 bit 7 reads 0x80;
        // its mapper banks the second PRG half + upper CHR pages — the two
        // CPUs run different programs on real DualSystem boards) and
        // provision the shared 2 KiB WRAM on BOTH consoles (each holds a
        // copy; `pump_comms` converges them — the MAME `.share("nvram")`
        // model).
        dual.sub.bus_mut().set_vs_sub(true);
        dual.sub.bus_mut().set_vs_dual_sub();
        dual.main.bus_mut().enable_vs_dual_wram();
        dual.sub.bus_mut().enable_vs_dual_wram();
        // Reset-time seed (Mesen2 `VsControlManager::Reset`:
        // `UpdateMainSubBit(main ? 0x00 : 0x02)`): the main half boots with
        // its bit-1 signal LOW — which asserts the SUB's external /IRQ —
        // and the sub boots with its signal HIGH (the main's /IRQ clear).
        // Wrecking Crew requires this seed to progress past its handshake.
        dual.apply_main_bit1(false);
        dual.apply_sub_bit1(true);
        Ok(dual)
    }

    /// Apply a MAIN-console bit-1 level: drive the sub's `/IRQ` (LOW
    /// asserts, per Mesen2 `UpdateMainSubBit` / MAME
    /// `(data & 2) ? CLEAR_LINE : ASSERT_LINE`).
    const fn apply_main_bit1(&mut self, level: bool) {
        self.main_bit1 = level;
        self.sub.bus_mut().set_vs_external_irq(!level);
    }

    /// Apply a SUB-console bit-1 level: drive the main's `/IRQ`.
    const fn apply_sub_bit1(&mut self, level: bool) {
        self.sub_bit1 = level;
        self.main.bus_mut().set_vs_external_irq(!level);
    }

    /// Drain both consoles' `$4016` comms levels + shared-WRAM write logs
    /// and apply the protocol. Called after every stepped instruction on
    /// either console, so partner-visible effects land with at most one
    /// instruction of latency (within the 5-cycle soft-lockstep window).
    fn pump_comms(&mut self) {
        if let Some(level) = self.main.bus_mut().take_vs_mainsub_edge() {
            self.apply_main_bit1(level);
        }
        if let Some(level) = self.sub.bus_mut().take_vs_mainsub_edge() {
            self.apply_sub_bit1(level);
        }
        // Converge the shared-WRAM copies (both directions). The logs are
        // usually empty; a handful of entries during the boot handshake and
        // per-frame gameplay exchange. `comms_scratch` is drained (not
        // replaced) each round, so its allocated capacity — and the
        // mapper-side log's own capacity, via `drain_vs_dual_wram_writes`'s
        // `Vec::drain` — survives across calls: steady-state, this loop is
        // allocation-free.
        self.main
            .bus_mut()
            .drain_vs_dual_wram_writes(&mut self.comms_scratch);
        for (off, val) in self.comms_scratch.drain(..) {
            self.sub.bus_mut().apply_vs_dual_wram_write(off, val);
        }
        self.sub
            .bus_mut()
            .drain_vs_dual_wram_writes(&mut self.comms_scratch);
        for (off, val) in self.comms_scratch.drain(..) {
            self.main.bus_mut().apply_vs_dual_wram_write(off, val);
        }
    }

    /// Run one MAIN-console frame with the sub console soft-locksteped to
    /// within a 5-CPU-cycle gap (Mesen2 `RunFrame` + `RunVsSubConsole`).
    ///
    /// Returns when the main console's PPU completes a frame (or its CPU
    /// jams / the frame budget trips — mirroring `Nes::run_frame`'s
    /// guards).
    pub fn run_frame(&mut self) {
        /// Same stuck-frame guard as `Nes::run_frame`.
        const MAX_CYCLES_PER_FRAME: u64 = 150_000;
        let start_frame = self.main.frame();
        let start_cycle = self.main.cycle();
        while self.main.frame() == start_frame {
            if self.main.is_jammed() {
                break;
            }
            if self.main.cycle().wrapping_sub(start_cycle) > MAX_CYCLES_PER_FRAME {
                break;
            }
            self.main.step_instruction();
            self.pump_comms();
            // Drain the sub to within the 5-cycle gap (or its frame parity).
            // The comparison must be overshoot-safe: an instruction advances
            // 2..=8 cycles, so the sub routinely lands AHEAD of the main by
            // a few cycles — a naive `wrapping_sub(..) > 5` then wraps to a
            // huge unsigned value and runs the sub away forever.
            while !self.sub.is_jammed()
                && (self.main.cycle() > self.sub.cycle().saturating_add(5)
                    || self.main.frame() > self.sub.frame())
            {
                self.sub.step_instruction();
                self.pump_comms();
            }
        }
        // Consume both PPUs' frame-complete latches so external users of the
        // underlying `Nes` (none today) never observe a stale latch.
        let _ = self.main.bus_mut().take_frame_complete();
        let _ = self.sub.bus_mut().take_frame_complete();
    }

    /// The main console's 256x240 RGBA8 framebuffer (the left screen).
    #[must_use]
    pub fn main_framebuffer(&self) -> &[u8] {
        self.main.framebuffer()
    }

    /// The sub console's 256x240 RGBA8 framebuffer (the right screen).
    #[must_use]
    pub fn sub_framebuffer(&self) -> &[u8] {
        self.sub.framebuffer()
    }

    /// Route controller input: ports 0/1 (P1/P2) → the main console's
    /// ports 0/1; ports 2/3 (P3/P4) → the sub console's ports 0/1.
    pub const fn set_buttons(&mut self, port: usize, buttons: crate::Buttons) {
        match port {
            0 | 1 => self.main.set_buttons(port, buttons),
            2 | 3 => self.sub.set_buttons(port - 2, buttons),
            _ => {}
        }
    }

    /// Coin routing (Mesen2 `VsControlManager`): acceptors 0/1 latch on the
    /// MAIN console, 2/3 on the SUB console.
    pub const fn insert_coin(&mut self, acceptor: u8) {
        match acceptor {
            0 | 1 => self.main.insert_coin(acceptor),
            2 | 3 => self.sub.insert_coin(acceptor - 2),
            _ => {}
        }
    }

    /// Clear both consoles' latched coin signals.
    pub const fn clear_coin(&mut self) {
        self.main.clear_coin();
        self.sub.clear_coin();
    }

    /// The service button (main panel) / service-2 (sub panel).
    pub const fn set_vs_service(&mut self, panel: u8, pressed: bool) {
        match panel {
            0 => self.main.set_vs_service(pressed),
            1 => self.sub.set_vs_service(pressed),
            _ => {}
        }
    }

    /// Borrow the main console (read-only diagnostics).
    #[must_use]
    pub const fn main(&self) -> &Nes {
        &self.main
    }

    /// Borrow the sub console (read-only diagnostics).
    #[must_use]
    pub const fn sub(&self) -> &Nes {
        &self.sub
    }

    /// Mutably borrow the main console (debugger / diagnostics — e.g. the
    /// side-effect-free `debug_peek_cpu`, which needs `&mut` for the
    /// mapper's banked lookups). Cross-wiring stays wrapper-owned; don't
    /// drive `$4016` writes through this handle.
    #[must_use]
    pub const fn main_mut(&mut self) -> &mut Nes {
        &mut self.main
    }

    /// Mutably borrow the sub console (debugger / diagnostics; see
    /// [`Self::main_mut`]).
    #[must_use]
    pub const fn sub_mut(&mut self) -> &mut Nes {
        &mut self.sub
    }

    /// Simultaneously borrow both consoles (diagnostics — e.g. a tracing
    /// harness replicating the lockstep loop with instrumented pumping).
    #[must_use]
    pub const fn split_mut(&mut self) -> (&mut Nes, &mut Nes) {
        (&mut self.main, &mut self.sub)
    }

    /// Serialize the dual system: a versioned container nesting the two
    /// standard [`Nes`] snapshots plus the wrapper's latch state.
    ///
    /// Layout: `RVSD` magic, `u16` version, latch byte
    /// (`bit0 = main_bit1, bit1 = sub_bit1`; bit 2 reserved — it carried
    /// a WRAM-ownership flag in a pre-release layout and is ignored on
    /// load), then two `u32`-length-prefixed `Nes` snapshots (main, sub).
    #[must_use]
    pub fn snapshot(&self) -> Vec<u8> {
        let main = self.main.snapshot();
        let sub = self.sub.snapshot();
        let mut out = Vec::with_capacity(4 + 2 + 1 + 8 + main.len() + sub.len());
        out.extend_from_slice(&SNAPSHOT_MAGIC);
        out.extend_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
        let latch = u8::from(self.main_bit1) | (u8::from(self.sub_bit1) << 1);
        out.push(latch);
        #[allow(clippy::cast_possible_truncation)]
        out.extend_from_slice(&(main.len() as u32).to_le_bytes());
        out.extend_from_slice(&main);
        #[allow(clippy::cast_possible_truncation)]
        out.extend_from_slice(&(sub.len() as u32).to_le_bytes());
        out.extend_from_slice(&sub);
        out
    }

    /// Restore a dual-system snapshot produced by [`Self::snapshot`].
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError`] on a bad container or when either nested
    /// console snapshot fails to restore.
    pub fn restore(&mut self, data: &[u8]) -> Result<(), SnapshotError> {
        // A malformed dual container reports as an unsupported format with
        // the container version we could read (0 when even the header is
        // short) — the closest fit among the existing error variants until
        // rc.1's save-state rework gives the dual container its own.
        let fail = |got: u16| SnapshotError::UnsupportedFormat {
            got,
            max: SNAPSHOT_VERSION,
        };
        if data.len() < 4 + 2 + 1 + 4 || data[0..4] != SNAPSHOT_MAGIC {
            return Err(fail(0));
        }
        let version = u16::from_le_bytes([data[4], data[5]]);
        if version != SNAPSHOT_VERSION {
            return Err(fail(version));
        }
        let latch = data[6];
        let mut cursor = 7usize;
        let read_block = |cursor: &mut usize| -> Result<&[u8], SnapshotError> {
            let len_end = cursor.checked_add(4).ok_or_else(|| fail(version))?;
            let len_bytes: [u8; 4] = data
                .get(*cursor..len_end)
                .ok_or_else(|| fail(version))?
                .try_into()
                .map_err(|_| fail(version))?;
            let len = u32::from_le_bytes(len_bytes) as usize;
            let end = len_end.checked_add(len).ok_or_else(|| fail(version))?;
            let block = data.get(len_end..end).ok_or_else(|| fail(version))?;
            *cursor = end;
            Ok(block)
        };
        let main_block = read_block(&mut cursor)?;
        let sub_block = read_block(&mut cursor)?;
        self.main.restore(main_block)?;
        self.sub.restore(sub_block)?;
        // Re-derive the wrapper latch + re-drive the cross-console signals
        // (the buses' transient comms fields are not serialized; the wrapper
        // owns the authoritative copies).
        self.main_bit1 = (latch & 0x01) != 0;
        self.sub_bit1 = (latch & 0x02) != 0;
        self.sub.bus_mut().set_vs_sub(true);
        self.sub.bus_mut().set_vs_external_irq(!self.main_bit1);
        self.main.bus_mut().set_vs_external_irq(!self.sub_bit1);
        // Re-converge the shared-WRAM copies from ONE buffer: the nested
        // snapshots each carry a copy (identical at snapshot time — the
        // write logs are always drained within `run_frame`), but a restore
        // into a fresh wrapper must not trust both blindly. The main's
        // copy is authoritative; it is cloned onto the sub.
        let wram = self
            .main
            .bus_mut()
            .take_vs_dual_wram()
            .or_else(|| self.sub.bus_mut().take_vs_dual_wram())
            .unwrap_or_else(|| alloc::vec![0u8; 0x0800].into_boxed_slice());
        self.sub.bus_mut().set_vs_dual_wram(wram.clone());
        self.main.bus_mut().set_vs_dual_wram(wram);
        Ok(())
    }
}

/// The top-level emulator: one standard console, or a Vs. `DualSystem` pair.
///
/// v2.0.0 beta.5 — the API reshape scoped to the major (the plan's
/// Workstream C/D): a NEW `rustynes-core` consumer would construct via
/// [`Emu::from_rom`] and match on the variant; every existing single-console
/// surface lives unchanged on [`Nes`]. **`rustynes-frontend` does NOT yet
/// consume this type** — it still constructs `Nes` directly
/// (`Nes::from_rom`/`from_rom_with_sample_rate`), so the `DualSystem` path
/// is core-and-test-harness-only in this release; wiring the desktop/mobile
/// UI onto `Emu` (dual-console rendering + 4-port input routing) is
/// explicitly deferred, tracked as a beta.5 known gap (see the beta.5
/// CHANGELOG entry and `docs/audit/vs-dualsystem-combined-dumps-2026-07-02.md`
/// for the current disposition).
pub enum Emu {
    /// A standard single-console system (every cart except the four
    /// `DualSystem` boards).
    Single(Box<Nes>),
    /// A Vs. `DualSystem` cabinet (two consoles + the cross-wiring).
    Dual(Box<VsDualSystem>),
}

impl Emu {
    /// Construct the right emulator shape for the ROM: a
    /// [`VsDualSystem`] when the SHA-keyed `vs_db` flags the cart as a
    /// `DualSystem` board, else a standard [`Nes`].
    ///
    /// # Errors
    ///
    /// Returns the underlying [`RomError`] if the bytes don't parse.
    pub fn from_rom(bytes: &[u8]) -> Result<Self, RomError> {
        let nes = Nes::from_rom(bytes)?;
        // Two detection sources, OR'd: the NES 2.0 header (byte-13 high
        // nibble = Vs. hardware type 5/6) and the SHA-keyed `vs_db` record.
        // The db is load-bearing — the circulating DualSystem dumps are
        // iNES 1.0 (no byte 13), so the header alone can never flag them.
        let db_dual = crate::vs_db::lookup(nes.rom_sha256()).is_some_and(|e| e.dual_system);
        if nes.is_vs_dual_system() || db_dual {
            // Re-construct as a dual pair (the probe Nes is discarded; the
            // dual constructor builds both halves from the same bytes).
            Ok(Self::Dual(Box::new(VsDualSystem::from_rom(bytes)?)))
        } else {
            Ok(Self::Single(Box::new(nes)))
        }
    }
}

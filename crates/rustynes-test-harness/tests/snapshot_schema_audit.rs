//! Save-state schema audit: every chip-struct field is either serialized or
//! explicitly, reasonedly excluded.
//!
//! ## Why this exists
//!
//! `RustyNES` has shipped the same bug three times: a live mid-frame field is
//! added to a chip struct, the matching `snapshot.rs` is not updated, and
//! nothing notices — because *no straight-`run_frame` test can notice*. A
//! forward run never round-trips its own state, so an incomplete schema is
//! invisible until something snapshots and restores mid-frame. Three things do:
//! the frontend's **run-ahead** (`[input] run_ahead`, default **1** — once per
//! visible frame), **netplay rollback**, and **TAS seeking**.
//!
//! | Tail | What was missing | How it surfaced |
//! |---|---|---|
//! | v5 (ADR 0030) | 2-cycle-ALE in-flight fetch state | netplay-rollback desync |
//! | v6 | sprite-shifter halt latches, OAM-corruption arming | Wizards & Warriors half-blank playfield |
//! | v8 (ADR 0034) | sprite-evaluation FSM, OAM-data-bus model | `AccuracyCoin` 141/141 headless vs 138/141 on the desktop app |
//!
//! Each was found by hand, after a user-visible symptom. The mechanical diff
//! below — struct fields vs. what the serializer touches — would have caught
//! all three *at the commit that introduced them*, in milliseconds, with no ROM
//! and no emulation. So it is a standing test rather than a technique someone
//! has to remember.
//!
//! ## How it works
//!
//! Both the chip source and its serializer are embedded with `include_str!`, so
//! this is a compile-time-hermetic text audit: no filesystem access, no
//! dependence on the working directory, and it runs in the default `cargo test`
//! job (no feature gate). For each field of the named struct, the body of
//! `pub fn snapshot(&self) -> Vec<u8>` must mention `self.<field>` at a word
//! boundary.
//!
//! Two details make that check mean something, and the audit is worthless
//! without either:
//!
//! * **The writer, not the whole file.** A field can appear all over `restore`
//!   — most visibly in the pre-version upconvert branches, which assign it a
//!   default (`self.sprite_eval_n = 0;`). Those assignments are precisely what
//!   an unserialized field looks like, so a whole-file search reports the bug as
//!   its own fix. Verified by negative control: deleting `sprite_eval_n` from
//!   the writer does not trip a file-wide search, and does trip this one.
//! * **Word boundaries.** A plain substring search lets `self.oam_addr` satisfy
//!   a field named `oam`, so every short name passes for free.
//!
//! Neither the `W`/`R` helpers nor the APU's `write_*` functions take `&self`
//! (they receive `&Pulse`, `&Dmc`, … from the writer's call sites), so scoping
//! to the writer body loses no coverage on any of the three chips.
//!
//! It is a *coverage* check, not a correctness check: it proves the serializer
//! touches each field, not that the write and read agree. Field-order symmetry
//! is pinned separately by the per-tail round-trip unit tests in each
//! `snapshot.rs`.
//!
//! ## When this test fails
//!
//! **Do not reach for the allowlist first.** The failure means a field is in
//! neither the schema nor the exclusion list, and the default assumption is
//! that it needs serializing — that is the answer three times out of three so
//! far. Add it to the schema behind a version bump. Only if the field is
//! genuinely derived (recomputable from serialized state), configuration
//! (re-applied by the host on load), or output-only (never read back into
//! emulation) does it belong in `DERIVED_OR_CONFIG`, and then it needs a reason
//! that says *which* of those it is.

/// One chip's audit input: where the struct lives, where its serializer lives,
/// and which fields are deliberately outside the schema.
struct Chip {
    /// Human label used in assertion messages.
    label: &'static str,
    /// Source of the struct definition.
    struct_src: &'static str,
    /// Name of the struct whose fields are audited.
    struct_name: &'static str,
    /// Source of the `snapshot` / `restore` implementation.
    snapshot_src: &'static str,
    /// Signatures (exact line prefixes) of the functions whose bodies make up
    /// the writer, when it is not a single `snapshot(&self)`. Empty means
    /// [`writer_body`]. The bus needs this: `encode_bus` reads the struct
    /// through accessors (`bus.bus_misc_state()`, `bus.ram_bytes()`, ...), so
    /// the `self.<field>` references live in those accessors' bodies.
    writer_fns: &'static [&'static str],
    /// Fields deliberately excluded, each with the reason it is safe to omit.
    /// Every entry must still name a real field, so the list cannot rot into
    /// blessing names that no longer exist.
    derived_or_config: &'static [(&'static str, &'static str)],
    /// Fields that are live emulation state, are NOT serialized, and are known
    /// to be a gap. Listed so the audit stays honest rather than passing by
    /// misclassifying them as derived. Asserted exactly: closing one of these
    /// (or opening a new one) fails this test until the list is updated.
    known_gaps: &'static [(&'static str, &'static str)],
}

const CHIPS: &[Chip] = &[
    Chip {
        label: "Ppu",
        struct_src: include_str!("../../rustynes-ppu/src/ppu.rs"),
        struct_name: "Ppu",
        snapshot_src: include_str!("../../rustynes-ppu/src/snapshot.rs"),
        writer_fns: &[],
        derived_or_config: &[
            (
                "oam_decay_enabled",
                "config: the opt-in decay model's enable flag, re-applied by the host on load \
                 (like `region`); the v7 tail serializes the per-row ages, not the switch",
            ),
            (
                "active_palette",
                "config: the loaded `.pal` override, re-applied by the frontend on load",
            ),
            (
                "rgba_lut",
                "derived: the RGBA lookup table, rebuilt from the active palette",
            ),
            ("custom_palette", "config: user palette override"),
            (
                "is_2c05",
                "board identity: re-derived from the ROM header at load",
            ),
            (
                "id_2c05",
                "board identity: re-derived from the ROM header at load",
            ),
            (
                "die_revision",
                "config: opt-in PPU hardware-revision knob (v2.1.7 P5)",
            ),
            (
                "power_up_palette",
                "config: opt-in power-up palette model, consumed only at power-on",
            ),
            (
                "index_framebuffer",
                "output-only: the palette-index mirror of the framebuffer consumed by the \
                 composite filters; refilled every frame, never read back into emulation",
            ),
            (
                "frame_ntsc_phase",
                "cosmetic: the composite phase, documented alongside `dot_counter` as \
                 deliberately outside the save-state",
            ),
            (
                "extra_scanlines",
                "config: the overclock amount; the in-flight countdown \
                 `extra_lines_remaining` IS serialized (v4 tail)",
            ),
            (
                "fast_dotloop",
                "config: runtime performance knob (v2.1.8 A1); selects a code path, holds no state",
            ),
            (
                "render_gate_prev2",
                "diagnostic: exists ONLY under the default-off `phi2-write-sweep` feature, as                  the two-dots-ago history for the v2.6.18 `RENDER_GATE_LAG` derivation knob.                  The shipped build does not compile this field, so there is nothing for a                  shipped save state to carry; and within a sweep it is recomputed from the                  next dot's rendering value, so it is derived rather than authoritative. If                  the knob is ever promoted to a shipped behaviour this entry MUST be revisited                  -- it is excluded because the field is absent from real builds, not because                  the state is unimportant",
            ),
            (
                "sweep_mask_history",
                "diagnostic: exists ONLY under the default-off `phi2-write-sweep` feature, as \
                 the dedicated four-stage `$2001` history for the v2.6.18 `OAM2_GATE_LAG` \
                 derivation knob. Same ground as `render_gate_prev2` above: the shipped build \
                 does not compile the field, so no shipped save state can carry it, and within \
                 a sweep it is refilled from the live mask on the next dot. If the knob is ever \
                 promoted to shipped behaviour this entry MUST be revisited -- it is excluded \
                 because the field is absent from real builds, not because the state is \
                 unimportant",
            ),
            (
                "state_trace",
                "diagnostic: `ppu-state-trace` ring buffer, output-only",
            ),
            (
                "trace_cpu_cycle",
                "diagnostic: the CPU cycle stamped into each `ppu-state-trace` record so a \
                 dot can be located against `obs.bin`, which is cycle-keyed. \
                 Excluded on a STRONGER ground than the other diagnostics here \
                 rather than a weaker one: the bus writes it unconditionally at \
                 the start of every CPU cycle, BEFORE any dot of that cycle is \
                 ticked, so a restore cannot observe a stale value -- the first \
                 cycle after a load overwrites it before the first record exists. \
                 Nothing in the PPU reads it; it is only copied into a record. \
                 Carrying it would also be actively wrong, since the cycle counter \
                 belongs to the run that produced the save and not to the one \
                 resuming it.",
            ),
            (
                "fast_path_hits",
                "diagnostic: `ppu-fetch-trace` counter of how many dots took the \
                 specialized fast dot path. It exists so a test can assert it EXERCISED \
                 the fast path rather than passing because the path was never entered -- \
                 a review of #450 claimed the fast path bypasses the fetch trace, and the \
                 test refuting that is worthless unless it can show the path ran. Nothing \
                 in the PPU reads it, so a restore that discards it cannot change a dot; \
                 and carrying it across a save would make the count describe two \
                 different runs stitched together, which is worse than not carrying it.",
            ),
            (
                "fetch_trace",
                "diagnostic: `ppu-fetch-trace` capture of the addresses the PPU drives on \
                 its own bus. Output-only and never read back into emulation -- nothing \
                 in the PPU consults it, so a restore that discards it cannot change a \
                 single dot. Excluded rather than serialized for the same reason as \
                 `state_trace` beside it, and NOT because it is inconvenient to carry: \
                 this file's own docs record that the default answer is to SERIALIZE, \
                 and it has been right three times out of three.",
            ),
            // The scanline-classification cache. Deliberately recomputed rather
            // than carried: it is a pure function of `scanline` + `region`, both
            // serialized, and `restore` resets the key to the `Ppu::new` sentinel
            // so the next tick refills it. See ADR 0034 and the
            // `restore_invalidates_the_scanline_classification_cache` unit test.
            (
                "cached_visible",
                "derived: recomputed from `scanline` + `region`; invalidated on restore",
            ),
            (
                "cached_pre_render",
                "derived: recomputed from `scanline` + `region`; invalidated on restore",
            ),
            (
                "cached_render_line",
                "derived: recomputed from `scanline` + `region`; invalidated on restore",
            ),
            (
                "cached_idle_line",
                "derived: recomputed from `scanline` + `region` (not visible, not \
                 pre-render, not the VBL-set line); invalidated on restore",
            ),
            (
                "flags_cached_scanline",
                "derived: the cache key for the four `cached_*` flags; reset to the \
                 `Ppu::new` sentinel on restore so a warm key cannot survive a timeline change",
            ),
            (
                "hd_tile_source",
                "output-only: `hd-pack` per-pixel telemetry, refilled every frame",
            ),
            ("hd_bg_addr_latch", "output-only: `hd-pack` fetch telemetry"),
            ("hd_bg_addr_cur", "output-only: `hd-pack` fetch telemetry"),
            ("hd_bg_addr_next", "output-only: `hd-pack` fetch telemetry"),
            ("hd_spr_addr", "output-only: `hd-pack` fetch telemetry"),
            ("hd_spr_x", "output-only: `hd-pack` fetch telemetry"),
            ("hd_spr_off_y", "output-only: `hd-pack` fetch telemetry"),
            ("hd_bg_idx_latch", "output-only: `hd-pack` fetch telemetry"),
            ("hd_bg_idx_cur", "output-only: `hd-pack` fetch telemetry"),
            ("hd_bg_idx_next", "output-only: `hd-pack` fetch telemetry"),
            ("hd_spr_idx", "output-only: `hd-pack` fetch telemetry"),
            (
                "write_attrib",
                "output-only: v2.3.2 `debug-hooks` per-byte write attribution, never read by \
                 emulation. Deliberately NOT serialized — a restored state's bytes were not \
                 written by any instruction this session ran, so carrying PCs across a restore \
                 would report a timeline that no longer exists. `Nes::restore_inner` and \
                 `Nes::power_cycle` clear it instead",
            ),
            (
                "attrib_pc",
                "output-only: v2.3.2 write-attribution context, re-pushed by `Nes::run_frame` \
                 before every instruction, so a restore's stale value cannot survive one step",
            ),
            (
                "attrib_cycle",
                "output-only: v2.3.2 write-attribution context; see `attrib_pc`",
            ),
            (
                "dma_attrib_pc",
                "output-only: v2.3.2 write-attribution context latched at the `$4014` write; \
                 re-latched by every subsequent OAM DMA trigger",
            ),
            (
                "dma_attrib_cycle",
                "output-only: v2.3.2 write-attribution context; see `dma_attrib_pc`",
            ),
            (
                "prov_frame",
                "output-only: v2.3.2 `debug-hooks` per-pixel provenance for the CURRENT frame, \
                 never read by emulation. NOT serialized, and explicitly CLEARED on power-cycle \
                 and both restore paths (`Ppu::clear_pixel_provenance`) — the same treatment as \
                 `write_attrib`. An earlier version of this entry argued it needed neither, \
                 because it is 'overwritten by the next `run_frame` like the framebuffer it \
                 shadows'. That analogy is false and was corrected in review: the framebuffer IS \
                 serialized and returns consistent with the restored state, whereas this frame is \
                 not, so a restore landing mid-frame left pre-restore addresses for every pixel \
                 above the current scanline with nothing marking them stale",
            ),
            (
                "prov_armed",
                "derived: mirrors `prov_frame.is_some()`, set only by \
                 `Ppu::set_pixel_provenance`; a host-side toggle, not machine state",
            ),
            (
                "prov_nt_pending",
                "output-only: v2.3.2 provenance address awaiting commit, overwritten by the \
                 next nametable fetch — at most 8 dots after any restore",
            ),
            (
                "prov_at_pending",
                "output-only: v2.3.2 provenance address awaiting commit; see `prov_nt_pending`",
            ),
            (
                "prov_bg_latch",
                "output-only: v2.3.2 provenance address cascade, re-committed at every BG \
                 pattern fetch. A restore mid-scanline can leave one tile group reporting the \
                 pre-restore addresses; that is a telemetry gap of at most 8 pixels in one \
                 frame, not emulation state, and serializing it would imply the record survives \
                 a timeline change when the attribution store deliberately does not",
            ),
            (
                "prov_bg_cur",
                "output-only: v2.3.2 provenance address cascade; see `prov_bg_latch`",
            ),
            (
                "prov_bg_next",
                "output-only: v2.3.2 provenance address cascade; see `prov_bg_latch`",
            ),
            (
                "prov_spr_addr",
                "output-only: v2.3.2 per-slot sprite pattern address for provenance, rewritten \
                 by every sprite-tile fetch (dots 257-320 of each scanline)",
            ),
        ],
        known_gaps: &[],
    },
    Chip {
        label: "Cpu",
        struct_src: include_str!("../../rustynes-cpu/src/cpu.rs"),
        struct_name: "Cpu",
        snapshot_src: include_str!("../../rustynes-cpu/src/snapshot.rs"),
        writer_fns: &[],
        derived_or_config: &[(
            "burn_histogram",
            "diagnostic: `cpu-instr-cycle-trace` per-opcode counter, read by the `burn_probe` \
             harness bin and never consulted by emulation",
        )],
        known_gaps: &[],
    },
    Chip {
        label: "Apu",
        struct_src: include_str!("../../rustynes-apu/src/apu.rs"),
        struct_name: "Apu",
        snapshot_src: include_str!("../../rustynes-apu/src/snapshot.rs"),
        writer_fns: &[],
        derived_or_config: &[
            (
                "mixer",
                "derived: two constant lookup tables (`pulse_table` / `tnd_table`); the \
                 filter chain with the live IIR history lives in `blip`, which IS serialized",
            ),
            (
                "dmc_driven_externally",
                "config: wiring flag selecting where the DMC byte-timer ticks (v2.0 F-2); \
                 set at construction by the bus, not by emulation",
            ),
            (
                "last_frame_events",
                "derived: reset at the start of every `tick_with_external` and read by \
                 observers after that same tick; never survives a tick boundary",
            ),
            (
                "restored_parity_tail",
                "restore-produced protocol flag, not emulation state: `Apu::restore` sets it to \
                 report whether the blob carried the Stage-4 parity/DMA tail, so the bus knows \
                 not to re-seed the boot alignment over exactly-restored values. Consumed \
                 immediately after restore; serializing it would be circular",
            ),
            ("channel_mask", "config: frontend Audio Mixer channel mute"),
            (
                "channel_gain",
                "config: frontend Audio Mixer per-channel gain",
            ),
            (
                "last_external",
                "output-only: write-only-from-synthesis copy of the expansion-audio DAC tap \
                 for the UI oscilloscope; documented as never read back into the mixer, the \
                 IRQ path, or any determinism-relevant state",
            ),
            (
                "audio_prov",
                "output-only: v2.3.7 audio provenance -- per-register write attribution, the \
                 per-CPU-cycle mix trace, and the PC/cycle context feeding them, all behind one \
                 `Option<Box<..>>`. Never read by emulation. Deliberately NOT serialized for the \
                 same reason the PPU's `write_attrib` is not -- a restored state's registers were \
                 not written by any instruction this session ran, so carrying PCs across a restore \
                 would report a timeline that no longer exists. The mix trace is per-FRAME by \
                 construction (re-anchored by `begin_audio_provenance_frame`) and the context is \
                 re-pushed before every instruction, so neither has anything a save state could \
                 meaningfully carry. \
                 CONSOLIDATED from four inline fields after `apu_throughput` measured +9% on the \
                 DISARMED path with the state spread across the `Apu` struct",
            ),
        ],
        known_gaps: &[],
    },
    // The OPLL is not a 2A03 chip, but it is a serialized synthesizer with the
    // same failure mode, and it reached this audit the hard way: its state was
    // NOT carried at all until v2.3.7, and nothing mechanical noticed for the
    // whole life of the feature because the audit only knew about the three
    // chips in the console. A save-state surface that no audit can see is
    // exactly how the gap this closes was able to persist — so the new surface
    // is registered here in the same change that creates it.
    //
    // Its blob rides in the *mapper* section of whichever board carries the
    // chip (VRC7 today), which is why it has its own version byte and its own
    // error type rather than APU_SNAPSHOT_VERSION's.
    Chip {
        label: "Opll",
        struct_src: include_str!("../../rustynes-apu/src/opll.rs"),
        struct_name: "Opll",
        snapshot_src: include_str!("../../rustynes-apu/src/opll.rs"),
        writer_fns: &[],
        // `chip_type` and `patch_set` are absent from this list on purpose:
        // both ARE written by the serializer, so the audit already accounts for
        // them and an exclusion entry would be rejected as a false admission.
        // Their subtlety is on the *read* side, and is documented at the writer
        // — `chip_type` is emitted only as a tag that rejects a cross-chip
        // restore and is never assigned from, and of `patch_set` only the two
        // non-ROM entries (the user patch written through `$00-$07`) round-trip;
        // the remaining 36 are the chip's patch ROM, fixed by `chip_type`.
        derived_or_config: &[
            (
                "waves",
                "derived: the 1024-entry sine / half-sine lookup tables, built in `Opll::new`",
            ),
            (
                "tll_rks",
                "derived: the TLL + RKS lookup tables (~128 KiB), built in `Opll::new`",
            ),
        ],
        known_gaps: &[],
    },
    // v2.8.0 — the bus. It owns everything the chips do not (CPU RAM, the
    // controller ports, the DMA engines, the NMI edge latches, the open-bus
    // latches), and it was never registered here, so nothing mechanical ever
    // compared its struct with its serializer. The libretro audit (§2.4) found
    // `internal_data_bus` missing from the BUS section by reading; this entry
    // is what would have found it at the commit that added the field.
    Chip {
        label: "LockstepBus",
        struct_src: include_str!("../../rustynes-core/src/bus.rs"),
        struct_name: "LockstepBus",
        snapshot_src: include_str!("../../rustynes-core/src/bus.rs"),
        writer_fns: &[
            "    fn snapshot_into_with(",
            "    pub const fn cycle(&self)",
            "    pub fn ram_bytes(&self)",
            "    pub const fn controllers_ref(&self)",
            "    pub const fn controllers34_ref(&self)",
            "    pub const fn expansion_device(&self",
            "    pub const fn mirroring_override(&self)",
            "    pub const fn port_read_cycle(&self",
            "    pub const fn four_score_pending(&self)",
            "    pub const fn bus_misc_state(&self)",
        ],
        // Classified field by field when the bus joined the audit (v2.8.0).
        // Of the 49 it reported, one was a real gap (`internal_data_bus`,
        // now serialized); the rest are below, grouped by reason.
        derived_or_config: &[
            // --- The cartridge and board identity, rebuilt by loading the ROM.
            (
                "cart",
                "board identity: the parsed cartridge, rebuilt from the ROM at load",
            ),
            (
                "mapper_caps",
                "board identity: the mapper's static capability flags",
            ),
            (
                "rom_bytes",
                "board identity: the ROM image itself; a save state never carries it",
            ),
            (
                "cpu_div_cached",
                "derived: the region's CPU master-clock divider, cached at construction",
            ),
            (
                "ppu_div_cached",
                "derived: the region's PPU master-clock divider, cached at construction",
            ),
            // --- Opt-in hardware knobs, re-applied by the host on load.
            (
                "power_on_ram",
                "config: the power-on RAM pattern, consumed only at power-on",
            ),
            (
                "ppu_die_revision",
                "config: the opt-in PPU revision knob, re-applied by the host",
            ),
            (
                "power_up_palette",
                "config: the opt-in power-up palette model, consumed only at power-on",
            ),
            (
                "cpu_2a03_revision",
                "config: the opt-in 2A03 revision knob, re-applied by the host",
            ),
            (
                "zapper_temporal_light",
                "config: the Zapper light model's enable flag (default on since v2.3.6)",
            ),
            (
                "vs_dip",
                "config: the Vs. System DIP switches, set by the host from the game database",
            ),
            (
                "genie_codes",
                "config: Game Genie cheats, excluded from save state by design so a cheat \
                 never enters netplay, TAS or rollback state (`Nes::add_genie_code`)",
            ),
            // --- Host input: the frontend owns the level and re-asserts it.
            (
                "vs_coin",
                "host input: the frontend owns the coin-hold countdown (`vs_coin_frames`) \
                 and drives this through `insert_coin` / `clear_coin`",
            ),
            (
                "vs_service",
                "host input: the Vs. service button, driven by the frontend",
            ),
            (
                "famicom_mic",
                "host input: the Famicom microphone bit, set by the frontend every frame",
            ),
            (
                "inject_nmi",
                "host input: the co-simulation /NMI pin (`cosim-interrupt-inject` only), \
                 whose level the test bench owns",
            ),
            (
                "inject_irq",
                "host input: the co-simulation /IRQ pin (`cosim-interrupt-inject` only), \
                 whose level the test bench owns",
            ),
            // --- Vs. DualSystem cross-wiring, re-driven by the wrapper.
            (
                "vs_is_sub",
                "derived: set by `VsDualSystem::restore` (`set_vs_sub`), which owns it",
            ),
            (
                "vs_external_irq",
                "derived: re-driven by `VsDualSystem::restore` from the partner's bit-1 \
                 latch, which the dual container serializes",
            ),
            (
                "vs_4016_bit1",
                "derived: the wrapper's `main_bit1` / `sub_bit1` are the serialized copies, \
                 and the bus level is drained into them after every stepped instruction",
            ),
            (
                "vs_4016_bit1_dirty",
                "derived: drained by `pump_comms` after every stepped instruction, so it is \
                 clear at any point a snapshot can be taken",
            ),
            // --- Intra-cycle state, identical at every snapshot point.
            (
                "m2_phase",
                "derived: set to Low at the end of every CPU cycle, so it is Low at every \
                 instruction boundary a snapshot is taken at",
            ),
            (
                "irq_snapshot_mapper_at_low",
                "derived: rewritten every CPU cycle before any read; read live only by the \
                 `irq-timing-trace` record",
            ),
            (
                "irq_snapshot_apu_at_low",
                "derived: rewritten every CPU cycle before any read; read live only by the \
                 `irq-timing-trace` record",
            ),
            (
                "irq_snapshot_mapper_at_high",
                "derived: rewritten every CPU cycle before any read; its other reader is the \
                 deprecated `poll_irq`",
            ),
            (
                "irq_snapshot_apu_at_high",
                "derived: rewritten every CPU cycle before any read; its other reader is the \
                 deprecated `poll_irq`",
            ),
            // --- The pre-v2.0.0 per-cycle DMA path, dead since the one-clock scheduler.
            (
                "dma_total",
                "dead: set and read only by `oam_dma_step` and the `dmc_overlap_*` methods, \
                 `#[deprecated]` in v2.7.5 with no caller since v2.0.0; zero on the live path",
            ),
            (
                "dmc_step_was_get",
                "dead: read only by the deprecated `dmc_dma_last_was_get`, which has no \
                 caller since v2.0.0",
            ),
            // --- Output-only telemetry, never read back into emulation.
            (
                "controller_polled",
                "output-only: the TAStudio lag-frame flag, cleared every frame",
            ),
            (
                "events",
                "output-only: the event viewer log, cleared every frame",
            ),
            ("event_logging", "config: the event viewer's enable flag"),
            (
                "accesses",
                "output-only: the Lua access log, cleared every frame",
            ),
            ("access_logging", "config: the Lua access log's enable flag"),
            (
                "interrupts",
                "output-only: the Lua interrupt log, cleared every frame",
            ),
            (
                "interrupt_logging",
                "config: the Lua interrupt log's enable flag",
            ),
            ("event_bp_mask", "config: the debugger's event breakpoints"),
            (
                "event_break_hit",
                "output-only: the first event breakpoint hit this frame, cleared every frame",
            ),
            (
                "irq_trace",
                "output-only: the `irq-timing-trace` capture buffer",
            ),
            (
                "trace_a12_latest",
                "output-only: cycle-trace scratch for the debug-hooks trace record",
            ),
            (
                "trace_last_a12",
                "output-only: cycle-trace scratch for the debug-hooks trace record",
            ),
            (
                "trace_a12_scratch",
                "output-only: cycle-trace scratch for the debug-hooks trace record",
            ),
            (
                "trace_bus_access",
                "output-only: cycle-trace scratch, consumed within the cycle that set it",
            ),
            (
                "trace_bus_addr",
                "output-only: cycle-trace scratch, consumed within the cycle that set it",
            ),
            (
                "trace_bus_data",
                "output-only: cycle-trace scratch, consumed within the cycle that set it",
            ),
            (
                "trace_last_pc",
                "output-only: cycle-trace scratch for the debug-hooks trace record",
            ),
            (
                "trace_r1_scanline_start",
                "output-only: cycle-trace scratch for the debug-hooks trace record",
            ),
            (
                "trace_r1_dot_start",
                "output-only: cycle-trace scratch for the debug-hooks trace record",
            ),
            (
                "trace_r1_frame_start",
                "output-only: cycle-trace scratch for the debug-hooks trace record",
            ),
        ],
        known_gaps: &[],
    },
];

/// The writer text for `chip`: the single `snapshot` body, or the
/// concatenated bodies of its `writer_fns`.
fn writer_text(chip: &Chip) -> String {
    if chip.writer_fns.is_empty() {
        return writer_body(chip.snapshot_src).to_owned();
    }
    let src = chip.snapshot_src.replace('\r', "");
    let mut out = String::new();
    for sig in chip.writer_fns {
        let start = src.find(sig).unwrap_or_else(|| {
            panic!(
                "{}: writer function `{}` not found — renamed? The audit would silently \
                 lose every field it reads",
                chip.label,
                sig.trim()
            )
        });
        let body = &src[start..];
        // Same indentation scoping as `writer_body`: the first line that is
        // exactly `    }` closes a four-space-indented method.
        let end = body
            .find("\n    }")
            .unwrap_or_else(|| panic!("{}: unterminated `{}`", chip.label, sig.trim()));
        out.push_str(&body[..end]);
        out.push('\n');
    }
    out
}

/// Extract the field names of `struct <name>` from Rust source.
///
/// Deliberately simple: chip structs are a flat list of `name: Type,` at one
/// indent level, interleaved with doc comments and attributes. A field is a
/// four-space-indented line whose first token (after an optional `pub` /
/// `pub(crate)`) is a lowercase identifier followed by `:`. Doc comments,
/// attributes, and nested type syntax (`[u8; 8]`, `Option<T>`) never match.
fn struct_fields(src: &str, name: &str) -> Vec<String> {
    // The chip source is embedded with `include_str!`, which captures the file
    // with whatever line endings are on disk at compile time — and a Windows
    // checkout without an `eol=lf` attribute for `.rs` gives CRLF. The struct
    // terminator search below is LF-anchored (`\n}\n`), and `\r\n}\r\n` does not
    // contain `\n}\n` (it is `\n}\r`), so a CRLF checkout made this panic with
    // "unterminated struct body" on Windows only. Strip `\r` so every anchor and
    // the line scan are line-ending-agnostic. (`writer_body`'s `\n    }` anchor
    // survives inside `\r\n    }`, and `touches_field`'s `self.<field>` needle
    // never spans a line break, so both already tolerate CRLF — but normalizing
    // here removes the one path that did not.)
    let src = src.replace('\r', "");
    let src = src.as_str();
    let header = format!("pub struct {name} {{");
    let start = src
        .find(&header)
        .unwrap_or_else(|| panic!("`{header}` not found — did the struct get renamed?"));
    let body = &src[start + header.len()..];
    let end = body
        .find("\n}\n")
        .unwrap_or_else(|| panic!("unterminated `struct {name}` body"));

    let mut out = Vec::new();
    for line in body[..end].lines() {
        let Some(rest) = line.strip_prefix("    ") else {
            continue;
        };
        if rest.starts_with(' ') || rest.starts_with('#') || rest.starts_with("//") {
            continue;
        }
        let rest = rest
            .strip_prefix("pub(crate) ")
            .or_else(|| rest.strip_prefix("pub "))
            .unwrap_or(rest);
        let Some((ident, _)) = rest.split_once(':') else {
            continue;
        };
        let ident = ident.trim();
        if ident.is_empty()
            || !ident.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
            || !ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            continue;
        }
        out.push(ident.to_owned());
    }
    assert!(
        out.len() > 10,
        "parsed only {} fields from `struct {name}` — the parser has drifted from the source \
         layout and would silently audit nothing",
        out.len()
    );
    out
}

/// Extract the body of `pub fn snapshot(&self) -> Vec<u8>` from a serializer
/// source.
///
/// Indentation-scoped rather than brace-matched: the signature sits at four
/// spaces and every line of the body is indented further, so the first
/// subsequent line that is exactly `    }` closes the function. That is
/// immune to braces inside string literals, which naive brace counting is not.
///
/// Scoping to the writer is the point of the whole audit — see the module docs.
fn writer_body(src: &str) -> &str {
    // v2.3.3 — the PPU's writer moved into `snapshot_with(&self, slim: bool)`
    // when the slim (framebuffer-less) rewind encoding landed; `snapshot()` is
    // now a one-line forwarder. Prefer the real writer and fall back to the
    // plain signature for the serializers that still use it. Without this the
    // audit reads an empty body and reports every field as unserialized —
    // which it did, loudly, and correctly by its own rules.
    const SIG_WITH: &str = "    fn snapshot_with(&self, slim: bool) -> Vec<u8> {";
    const SIG: &str = "    pub fn snapshot(&self) -> Vec<u8> {";
    let sig = if src.contains(SIG_WITH) {
        SIG_WITH
    } else {
        SIG
    };
    let start = src
        .find(sig)
        .unwrap_or_else(|| panic!("writer signature `{sig}` not found — did it get renamed?"))
        + sig.len();
    let body = &src[start..];
    let end = body
        .find("\n    }")
        .unwrap_or_else(|| panic!("unterminated `snapshot` writer body"));
    let body = &body[..end];
    assert!(
        body.contains("self."),
        "extracted writer body references no fields at all — the extractor has drifted"
    );
    body
}

/// Does `src` reference `self.<field>` at a word boundary?
///
/// The boundary check is load-bearing: a plain substring search would let
/// `self.oam_addr` satisfy a field named `oam`, which is exactly the kind of
/// accidental pass that makes a coverage audit worthless.
fn touches_field(src: &str, field: &str) -> bool {
    let needle = format!("self.{field}");
    let mut from = 0;
    while let Some(hit) = src[from..].find(&needle) {
        let after = from + hit + needle.len();
        let next = src[after..].chars().next();
        if !next.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            return true;
        }
        from = after;
    }
    false
}

/// Does `src` contain `prefix` + `field` at a word boundary on the right?
/// The same boundary rule as [`touches_field`], for a receiver other than
/// `self` (the encoder reads the fields through a local `s`).
fn touches_field_via(src: &str, prefix: &str, field: &str) -> bool {
    let needle = format!("{prefix}{field}");
    let mut from = 0;
    while let Some(hit) = src[from..].find(&needle) {
        let after = from + hit + needle.len();
        let next = src[after..].chars().next();
        if !next.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            return true;
        }
        from = after;
    }
    false
}

#[test]
fn every_bus_misc_state_field_is_encoded_and_decoded() {
    // Review on #556 (CodeRabbit). The `LockstepBus` entry audits the bus
    // struct against `bus_misc_state` / `set_bus_misc_state`, the accessors
    // that move fields in and out of `BusMiscState`. That proves a field
    // reaches the transfer struct; it does not prove `encode_bus` WRITES it or
    // `decode_bus` READS it back, which is where `internal_data_bus` was
    // missing before v2.8.0. This closes the second half: every
    // `BusMiscState` field must be read as `s.<field>` in `encode_bus`. The
    // decode side needs no name check, because a struct literal that omits a
    // field does not compile; the one way to skip a field there is a `..`
    // struct-update base, which this forbids. (A decoder that names a field
    // but feeds it a constant is a value error, caught by the round-trip
    // tests such as `internal_data_bus_round_trips_through_save_state`.)
    let src = include_str!("../../rustynes-core/src/bus_snapshot.rs").replace('\r', "");
    let fields = struct_fields(&src, "BusMiscState");
    assert!(
        fields.iter().any(|f| f == "internal_data_bus") && fields.len() > 10,
        "BusMiscState field extraction looks wrong: {fields:?}"
    );
    let enc_start = src.find("pub fn encode_bus(").expect("encode_bus");
    let enc = &src[enc_start..];
    let enc = &enc[..enc.find("\n}\n").expect("end of encode_bus")];
    let dec_start = src
        .find("bus.set_bus_misc_state(BusMiscState {")
        .expect("decode_bus's BusMiscState literal");
    let dec = &src[dec_start..];
    let dec = &dec[..dec.find("});").expect("end of the literal")];
    let not_encoded: Vec<_> = fields
        .iter()
        .filter(|f| !touches_field_via(enc, "s.", f))
        .collect();
    assert!(
        not_encoded.is_empty(),
        "BusMiscState fields encode_bus never writes: {not_encoded:?}"
    );
    assert!(
        !dec.contains(".."),
        "decode_bus's BusMiscState literal uses a `..` base, so a field can be \
         skipped without a compile error"
    );
}

#[test]
fn every_chip_field_is_serialized_or_explicitly_excluded() {
    for chip in CHIPS {
        let fields = struct_fields(chip.struct_src, chip.struct_name);
        let writer = writer_text(chip);
        let writer = writer.as_str();
        let excluded: Vec<&str> = chip
            .derived_or_config
            .iter()
            .chain(chip.known_gaps.iter())
            .map(|(name, _)| *name)
            .collect();

        let unaccounted: Vec<&String> = fields
            .iter()
            .filter(|f| !touches_field(writer, f) && !excluded.contains(&f.as_str()))
            .collect();

        assert!(
            unaccounted.is_empty(),
            "{}: {} field(s) are neither serialized nor listed as deliberately excluded: {:?}\n\
             \n\
             The default assumption is that these need SERIALIZING (behind a snapshot version \
             bump), not allowlisting — that has been the right answer three times out of three \
             (see this file's module docs). A field belongs in `derived_or_config` only if it is \
             recomputable from serialized state, host-supplied configuration re-applied on load, \
             or output-only telemetry never read back into emulation.",
            chip.label,
            unaccounted.len(),
            unaccounted,
        );
    }
}

#[test]
fn exclusion_lists_name_only_real_fields() {
    // Guards the other direction: an allowlist entry that no longer matches a
    // field is dead weight that quietly widens the audit's blind spot when a
    // future field happens to reuse the name.
    for chip in CHIPS {
        let fields = struct_fields(chip.struct_src, chip.struct_name);
        for (name, _) in chip.derived_or_config.iter().chain(chip.known_gaps.iter()) {
            assert!(
                fields.iter().any(|f| f == name),
                "{}: exclusion list names `{name}`, which is not a field of `struct {}` \
                 — remove the stale entry",
                chip.label,
                chip.struct_name,
            );
        }
    }
}

#[test]
fn exclusion_lists_do_not_bless_serialized_fields() {
    // A field that IS serialized has no business on an exclusion list: the entry
    // is either stale or the reason attached to it is wrong. Either way the list
    // stops describing reality, which is how an audit rots.
    for chip in CHIPS {
        let writer = writer_text(chip);
        let writer = writer.as_str();
        for (name, _) in chip.derived_or_config.iter().chain(chip.known_gaps.iter()) {
            assert!(
                !touches_field(writer, name),
                "{}: `{name}` is on an exclusion list but the serializer touches \
                 `self.{name}` — drop the entry",
                chip.label,
            );
        }
    }
}

#[test]
fn known_gaps_are_exactly_as_recorded() {
    // `known_gaps` is an admission, not a permission. Pinning it exactly means
    // closing a gap fails this test (delete the entry) and opening a new one
    // fails it too (the field lands in `every_chip_field_is_serialized_...`
    // first). Neither can happen silently.
    //
    // Currently EMPTY, and that is the interesting state: the list's only two
    // entries — the APU's `reset_4017_delay` / `reset_4017_value` scheduled
    // warm-reset `$4017` re-write, which this audit surfaced — were closed by
    // the `APU_SNAPSHOT_VERSION` v4 tail. Every field of every audited chip is
    // now either serialized or derived/config with a written reason. A future
    // entry here is a deliberate, documented admission, not a default.
    let recorded: Vec<(&str, &str)> = CHIPS
        .iter()
        .flat_map(|c| c.known_gaps.iter().map(|(f, _)| (c.label, *f)))
        .collect();
    assert_eq!(
        recorded,
        Vec::<(&str, &str)>::new(),
        "the set of known save-state gaps changed — update this list, and say so in the \
         CHANGELOG if one was closed",
    );
}

#[test]
fn the_v8_sprite_evaluation_fields_stay_serialized() {
    // Pin the specific regression ADR 0034 closed. The general audit above would
    // also catch a removal, but only as an anonymous count; this names the fields
    // so a future reviewer sees *which* bug reopened.
    let ppu = CHIPS
        .iter()
        .find(|c| c.label == "Ppu")
        .expect("Ppu chip entry");
    for field in [
        "sprite_eval_read_latch",
        "sprite_eval_n",
        "sprite_eval_m",
        "sprite_eval_found",
        "sprite_eval_sec_idx",
        "sprite_eval_copying",
        "sprite_eval_done",
        "sprite_eval_overflow_search",
        "sprite_eval_zero_found",
        "sprite_eval_first_iter",
        "oam_bus_copybuffer",
        "oam_bus_secondary",
        "oam_bus_addr_h",
        "oam_bus_addr_l",
        "oam_bus_secondary_addr",
        "oam_bus_copy_done",
        "oam_bus_sprite_in_range",
        "oam_bus_overflow_counter",
        "oam2_addr",
    ] {
        assert!(
            touches_field(writer_body(ppu.snapshot_src), field),
            "`{field}` dropped out of the PPU snapshot schema — this is the ADR 0034 \
             regression: run-ahead restores a populated secondary OAM beside a reset \
             sprite-evaluation walker, costing three AccuracyCoin tests",
        );
    }
}

#[test]
fn field_boundary_matching_rejects_prefixes() {
    // The audit's whole value rests on this: `self.oam_addr` must not satisfy a
    // field named `oam`, or every short field name passes for free.
    assert!(touches_field("x = self.oam_addr;", "oam_addr"));
    assert!(!touches_field("x = self.oam_addr;", "oam"));
    assert!(!touches_field("x = self.oam_addr;", "oam_add"));
    assert!(touches_field("w.bytes(&self.oam);", "oam"));
    assert!(touches_field("for h in &self.spr_halted {", "spr_halted"));
    assert!(!touches_field(
        "// mentions oam_bus_addr_h in prose",
        "oam_bus_addr_h"
    ));
}

#[test]
fn struct_fields_tolerates_crlf_line_endings() {
    // Regression pin for the Windows-only failure: `include_str!` captures the
    // chip source with the on-disk line endings, and a checkout without an
    // `eol=lf` attribute for `.rs` yields CRLF, whose struct terminator is
    // `\r\n}\r\n`. This test feeds CRLF source explicitly so the parser stays
    // line-ending-agnostic on every platform, not just where the audit happens
    // to be checked out LF. The Linux CI runs it too, so the guarantee holds
    // even though the original break only showed on Windows.
    // A struct with more than the parser's `> 10` drift-guard threshold, so the
    // sanity check inside `struct_fields` passes and only the line-ending
    // behavior is under test. Doc comments and attributes are interleaved to
    // exercise the same skip paths the real chip structs hit.
    use std::fmt::Write as _;
    let mut lf = String::from("pub struct Demo {\n");
    for i in 0..12 {
        let _ = write!(lf, "    /// field {i}\n    pub f{i}: u8,\n");
    }
    lf.push_str("}\n");
    let crlf = lf.replace('\n', "\r\n");

    let from_lf = struct_fields(&lf, "Demo");
    let from_crlf = struct_fields(&crlf, "Demo");

    let expected: Vec<String> = (0..12).map(|i| format!("f{i}")).collect();
    assert_eq!(from_lf, expected, "LF parse baseline");
    assert_eq!(
        from_crlf, from_lf,
        "CRLF source must yield the same fields as LF source",
    );
}

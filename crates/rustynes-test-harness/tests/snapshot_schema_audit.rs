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
                "state_trace",
                "diagnostic: `ppu-state-trace` ring buffer, output-only",
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
        ],
        known_gaps: &[],
    },
    Chip {
        label: "Cpu",
        struct_src: include_str!("../../rustynes-cpu/src/cpu.rs"),
        struct_name: "Cpu",
        snapshot_src: include_str!("../../rustynes-cpu/src/snapshot.rs"),
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
        ],
        known_gaps: &[],
    },
];

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
    const SIG: &str = "    pub fn snapshot(&self) -> Vec<u8> {";
    let start = src
        .find(SIG)
        .unwrap_or_else(|| panic!("writer signature `{SIG}` not found — did it get renamed?"))
        + SIG.len();
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

#[test]
fn every_chip_field_is_serialized_or_explicitly_excluded() {
    for chip in CHIPS {
        let fields = struct_fields(chip.struct_src, chip.struct_name);
        let writer = writer_body(chip.snapshot_src);
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
        let writer = writer_body(chip.snapshot_src);
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

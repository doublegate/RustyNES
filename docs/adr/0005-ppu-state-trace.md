# ADR 0005 — Per-PPU-Dot State Trace (`ppu-state-trace` Feature)

**Status:** Accepted (Session-10 observability tooling, 2026-05-20).
**Date:** 2026-05-20
**Author:** RustyNES v2 maintainers
**Pairs with:** ADR 0002 §"Test fixture" (per-CPU-cycle IRQ trace
infrastructure pattern this ADR mirrors at the PPU level).

---

## Context

Sessions 8 + 9 of the Cascade A investigation (`docs/audit/cascade-a-investigation-2026-05-19.md`)
attempted three Mesen2-faithful `sprite_eval_base_from_OAMADDR`
implementation variants. All three variants flipped the targeted
AccuracyCoin tests (`Arbitrary Sprite zero`, `Misaligned OAM behavior`)
FAIL → PASS but cascaded a 14-test regression across `INC $4014` /
`PPU Misc` / `CPU Behavior 2` / `Power On State` suites. The
persistent cascade across implementation variants indicates the
load-bearing failure is **not** the dirty-flag gating itself but
rather **intermediate-state corruption** from a single misaligned
eval pass propagating via secondary OAM, sprite shifters, or the
sprite-overflow flag.

Session 9 explicitly recommended observability infrastructure as
the next step:

> Extend the existing `irq_trace` machinery to optionally capture
> per-PPU-dot snapshots of `oam_addr`, `secondary_oam`, `spr_count`,
> `spr_zero_in_line`, `spr_shift_lo/hi[0..8]`, `spr_x[0..8]`,
> `mask` / `status`. Run AccuracyCoin INC $4014 Test 2 with trace
> enabled and diff against a known-good baseline to pinpoint the
> actual cascade mechanism.

The same pattern (per-CPU-cycle bus snapshot, gated on a cargo
feature, linear capacity-bounded buffer, integration-test consumer)
unblocked Phase B4 of Track C1 (`crates/nes-core/src/irq_trace.rs`)
and produced the empirical evidence ADR 0002 §"Empirical refinement
(2026-05-14, post-Phase-A + Phase-B4-attempt)" rests on. Recreating
that pattern at the PPU level is the infrastructure deliverable.

The next investigation session needs runtime-state visibility
comparable to Mesen2's debugger: a per-PPU-dot trace of the PPU's
internal state, diffable against a Mesen2-emitted reference trace
from the same input.

## Decision

Add a **new cargo feature `ppu-state-trace`** (off by default) in
`crates/nes-ppu/Cargo.toml` and `crates/nes-core/Cargo.toml`. When
the feature is OFF (the default for CI, commercial-ROM oracle, and
every standard workspace test run), the recording code is entirely
dead via `#[cfg(feature = "ppu-state-trace")]` gates inside
`Ppu::tick` and around the storage field on the `Ppu` struct.
Verified: the default `cargo check -p nes-ppu` build is unchanged.

When the feature is ON:

1. `Ppu` gains an `Option<PpuStateTrace>` field. A new public API
   `Ppu::enable_state_trace(PpuStateTrace)` / `take_state_trace()`
   mirrors the pattern of `LockstepBus::enable_irq_trace()` /
   `take_irq_trace()`.
2. At the **end** of every `Ppu::tick` (after all dot effects have
   applied), a `PpuStateRecord` is built from `&self` and pushed
   into the buffer iff the dot lies inside the buffer's
   `PpuTraceConfig` filter window. **Read-only** — the recorder
   never mutates PPU state, preserving the determinism contract
   (`docs/architecture.md` §Determinism).
3. The buffer has a caller-specified capacity. Records past the cap
   are silently dropped (`overflow` counter advances). Sized
   generously by the caller (a 10-frame visible-only window is
   ~1.6 M records ≈ 180 MB packed binary).
4. The buffer renders to two formats:
   * **Binary**: 12-byte ASCII magic `RUSTYNES_PPU` + 2-byte LE
     schema version + 2-byte reserved-flags + N × 111-byte packed
     `PpuStateRecord`s. Stable schema; bump
     `PPU_TRACE_SCHEMA_VERSION` for any incompatible change.
   * **CSV**: human-readable, one row per record, header line first.
     Same column order as the binary layout.
5. A companion CLI `ppu_trace_diff` (under `crates/nes-test-harness/src/bin/`,
   feature-gated) reads two binary traces, aligns by record index,
   reports first or all per-field divergences, optionally skips
   fields by name. Designed to compare a RustyNES capture against
   a Mesen2 Lua-emitted reference trace.
6. A Mesen2 Lua script (`scripts/mesen2_ppu_trace.lua`) emits a
   binary trace in the same schema. Documented in
   `docs/ppu-trace-tooling.md`.

## Schema (v1)

`PpuStateRecord` is 111 bytes packed (verified by a const-time
assertion + a runtime test):

| Bytes | Field | Notes |
|-------|-------|-------|
| 0..4 | `frame: u32` | Truncated from `Ppu::frame: u64` (safe for u32::MAX frames ≈ 71 days NES time). |
| 4..6 | `scanline: i16` | -1 (pre-render) .. 239 (last visible) .. 260/310 (vblank). |
| 6..8 | `dot: u16` | 0..=340. |
| 8..12 | `ctrl/mask/status/oam_addr` | Register snapshots. |
| 12..18 | `v: u16, t: u16, fine_x: u8, w_toggle: bool` | Loopy scroll state. |
| 18..26 | sprite-eval FSM | 5 u8 + 3 bools: n, m, found, sec_idx, copying, overflow_search, done, read_latch. |
| 26..28 | `spr_count: u8, spr_zero_in_line: bool` | Per-scanline line-up summary. |
| 28..60 | `spr_shift_lo/hi/attr/x: [u8; 8]` × 4 | 32 bytes total. |
| 60..70 | BG pipeline | bg_shift_lo/hi (u16), at_shift_lo/hi, nt/at/bg_lo/bg_hi latches (u8). |
| 70..102 | `secondary_oam: [u8; 32]` | Full secondary OAM. |
| 102..110 | `oam_fnv1a64: u64` | FNV-1a 64-bit hash of primary OAM (compact alternative to 256-byte per-record OAM). |
| 110..111 | `nmi_line: bool` | PPU /NMI assertion. |

All multi-byte fields are little-endian. The 12-byte magic +
2-byte schema version + 2-byte reserved-flags file header is 16
bytes, followed by 0 or more records.

## Why a hash for primary OAM?

A 256-byte OAM per record × 89 480 dots/frame ≈ 22 MB/frame is
prohibitively expensive. The FNV-1a hash gives a one-shot "did
OAM change between two dots" comparison; if it changed, the
investigator can re-run with a tighter window and per-record
OAM dump (a future schema v2 option, deferred until needed).

## Filter configuration

`PpuTraceConfig` supports three knobs (all inclusive ranges):

1. **`frame_range: RangeInclusive<u32>`** — required (no
   wildcard). Investigations have a bounded test ROM, so a
   bounded frame range is always known.
2. **`scanline_range: Option<RangeInclusive<i16>>`** — `None`
   means "all". `Some(0..=239)` is the "visible only" preset
   (`PpuTraceConfig::visible_only`).
3. **`dot_range: Option<RangeInclusive<u16>>`** — `None` means
   "all". `Some(64..=256)` is the "sprite-eval window" preset
   (`PpuTraceConfig::sprite_eval_window`), exposing the
   secondary-OAM-clear + sprite-evaluation FSM in isolation
   from the BG fetch pipeline.

## Alternatives considered

### Per-frame instead of per-dot

Discarded. The Cascade A regression is a per-dot artifact (the
sprite-evaluation FSM updates 192 times per visible scanline
between dots 65..=256). A per-frame snapshot would land at a
fixed scanline / dot (typically VBLANK), at which point the
sprite-eval state has been long-since latched into
`spr_count` / `spr_shift_lo` / `spr_shift_hi` / `spr_attr` and
the intermediate-state corruption is no longer visible.

### Run-length compression in the recorder

Discarded. The recorder runs inside `Ppu::tick` (per-dot hot
path); adding RLE state would either (a) require a Vec
allocation per dot (defeating the determinism contract by way
of allocator behavior) or (b) require a fixed-size circular
buffer with displacement tracking, which is ~3x the code surface
of the linear-buffer approach. The off-line `ppu_trace_diff`
tool can RLE-compress if needed; for the current 1.6 M-record
default window the binary file is 180 MB, well within tmpfs /
SSD bounds.

### Compile-time #[cfg] gates vs. runtime opt-in

Mixed. The **storage** field on `Ppu` is `#[cfg]`-gated (so the
default `Ppu` struct is byte-identical to pre-Session-10) and
the **call site** in `Ppu::tick` is `#[cfg]`-gated (so the
recording code does not generate at all in the default build).
But the **enable / take** API is runtime — `enable_state_trace`
installs an `Option<PpuStateTrace>`, so a test ROM that wants
tracing pays the storage cost only when tracing is on, even
under the `ppu-state-trace` feature build. This mirrors the
`irq_trace` design and keeps the API ergonomic for the
integration-test fixture.

## Consequences

### Positive

* **Empirical visibility into the Cascade A cascade**: Session 11+
  can run the fixture against a Mesen2 Lua-emitted reference,
  diff field-by-field, and isolate the load-bearing
  intermediate-state corruption to a single dot range.
* **Zero default-path cost**: `cargo bench`, the commercial-ROM
  oracle (60 ROMs), and the 537-strict-test workspace suite are
  byte-identical to pre-Session-10. Verified via the
  `default-features = []` quality-gate matrix.
* **Reusable pattern**: future PPU/CPU regression investigations
  (e.g. a hypothetical "DMC DMA cycle race" cascade) can reuse
  the same schema + diff tool without recompiling the harness.
* **Determinism preserved**: the recorder hook is strictly
  read-only — no PPU state mutation, no allocator side effects
  in the tick path beyond the bounded `Vec::push` that respects
  the capacity cap.

### Negative

* **Mesen2 Lua granularity**: Mesen2's published Lua API exposes
  per-scanline callbacks but not per-PPU-dot callbacks (verified
  2026-05-20 against Mesen2 documentation). The Lua reference
  trace is therefore per-scanline at dot 0; per-dot Mesen2
  reference traces require either a custom Mesen2 build or
  parsing Mesen2's built-in debugger trace log (deferred —
  documented as Approach B in `docs/ppu-trace-tooling.md`).
* **Schema drift cost**: any change to the PPU's internal field
  set (e.g. adding a new sprite-eval latch) requires bumping
  `PPU_TRACE_SCHEMA_VERSION` and regenerating any committed
  reference traces. Mitigation: the const-time `RECORD_SIZE`
  assertion catches accidental drift at compile time.
* **Binary file sizes**: a full-frame visible-only capture is
  ~9 MB; the default 10-frame window is ~90 MB. Files are
  gitignored under `/target/ppu_trace/`; reference traces
  intended for committed use should be gzip-compressed
  out-of-band (the diff tool reads uncompressed only — the
  caller decompresses).

## Compliance with the determinism contract

`docs/architecture.md` §Determinism requires: "same seed + ROM
+ input sequence ⇒ bit-identical framebuffer and audio." The
recorder reads `&self` after a dot's effects have applied; no
mutable borrow into the PPU is taken, and the resulting
`PpuStateRecord` is independent of host-side state (no system
time, no thread scheduling, no OS RNG). Verified by:

* The const-time `RECORD_SIZE` invariant.
* The `record_roundtrips_through_packed_bytes` unit test.
* The fixture's binary roundtrip assertion
  (`PpuStateTrace::from_binary(&trace.to_binary())` produces
  byte-identical records).

## Stop conditions

This ADR is **infrastructure-only**. The Cascade A fix itself is
out of scope. The infrastructure is considered done when:

1. `cargo test --workspace --features test-roms` is byte-identical
   to pre-Session-10 (537 strict + 5 ignored).
2. `cargo test -p nes-test-harness --features
   test-roms,ppu-state-trace --test ppu_state_trace_fixture`
   produces a parsable binary trace and a CSV preview.
3. `cargo build -p nes-test-harness --features ppu-state-trace
   --bin ppu_trace_diff` produces a working CLI that diffs two
   binary traces.
4. `scripts/mesen2_ppu_trace.lua` is documented + parseable but
   not necessarily executed (the user installs Mesen2
   out-of-band).
5. `cargo clippy --workspace --all-targets --features
   ppu-state-trace -- -D warnings` is green.

All five stop conditions are met by the Session-10 landing.

## Future schema bumps (not yet committed)

The reserved 2-byte flags field in the file header is the
extension point for forward-compatible schema additions:

* **Bit 0**: per-record OAM dump (full 256 bytes appended after
  `nmi_line`). Bumps record size to 367 bytes.
* **Bit 1**: per-record CIRAM hash (8-byte FNV-1a). Bumps
  record size by +8.
* **Bit 2**: per-record palette RAM dump (32 bytes). Bumps
  record size by +32.

A future ADR will document the schema bump procedure when the
first such addition lands.

---

## References

* `crates/nes-ppu/src/state_trace.rs` — recorder + schema.
* `crates/nes-ppu/src/ppu.rs` — `enable_state_trace` /
  `take_state_trace` / `build_state_record` API + per-tick hook.
* `crates/nes-test-harness/tests/ppu_state_trace_fixture.rs` —
  integration test fixture.
* `crates/nes-test-harness/src/bin/ppu_trace_diff.rs` — diff CLI.
* `scripts/mesen2_ppu_trace.lua` — Mesen2 Lua reference-trace
  emitter.
* `docs/ppu-trace-tooling.md` — usage guide.
* ADR 0002 §"Test fixture" — companion per-CPU-cycle IRQ trace.
* `docs/audit/cascade-a-investigation-2026-05-19.md` — the
  investigation this tooling unblocks.

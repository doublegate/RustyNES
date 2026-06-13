# ADR 0003 — Save-state Cross-version Migration Policy

**Status:** Accepted (in force since save-states shipped; ADR 0008 `.rnm` builds on it).
**Date:** 2026-05-11
**Author:** RustyNES maintainers
**Supersedes:** None.
**Numbering note:** Third in the docs/adr/ sequence (after 0001 mapper
dispatch, 0002 IRQ timing coordination). This ADR formalizes a policy that
was implicit in the codebase since Phase 5 Sprint 2 but never written down.

---

## Context

The `.rns` save-state container (`crates/rustynes-core/src/save_state.rs`) is a
tagged-section format. The 16-byte header (`MAGIC` + `FORMAT_VERSION` +
truncated ROM SHA-256) is followed by an ordered list of sections; each
section is `tag(4) || version(1) || length(u32 le) || body`. Tags currently
in use: `BUS `, `CPU `, `PPU `, `APU `, `MAP `.

Per-section version bytes already exist but their semantics are not
documented anywhere. Concrete versioning bumps so far:

| Tag | Latest | Notes |
|---|---|---|
| `CPU ` | 1 | Stable since Sprint 2. |
| `PPU ` | 1 | Stable since Sprint 2. |
| `APU ` | 1 | Stable since Sprint 2. |
| `BUS ` | 1 | Stable since Sprint 2. |
| `MAP ` | 1 → **3** | MMC5 bumped 2→3 in v0.9.0 (vertical split-screen + ExGrafix + dual sprite/BG CHR state). Other mappers still 1. The `MAP ` body itself is opaque to `rustynes-core`; per-mapper version bytes live inside it. |

`CLAUDE.md` says: "cross-version compatibility is best-effort, not
guaranteed." That's the current state -- but it leaves callers (frontend
slot picker, future netplay, future TAS movie format) with no contract for
when to expect failures. A bumped section version mid-development should
not break a user's existing slot file, AND the new code should not silently
load stale data from an older slot and treat it as current.

## Decision

Codify a three-tier policy keyed off the v1.x.y semver scheme:

### Policy

**Same MAJOR.MINOR (e.g. v1.0.0 ↔ v1.0.1)**
- Same section versions across the two builds.
- Snapshot/restore MUST round-trip identically. Any divergence is a bug.
- New sections added by the newer build MUST default-initialize cleanly on
  older builds (older builds skip unknown tags via `SectionIter` — the
  existing parser already does this).

**Different PATCH within the same MINOR (e.g. v1.0.0 ↔ v1.0.2)**
- A section version bump is permitted within a PATCH if the section is
  strictly additive (new fields written at the end of the body; older readers
  truncate). The body's length prefix lets readers stop at the old field
  count.
- Should restore identical state. Sections added since the older build are
  default-initialized at load time.
- New tags are permitted; older builds skip them.

**Different MINOR or MAJOR (e.g. v1.0.x ↔ v1.1.x)**
- Best-effort. The container format itself is forward-compatible (the
  header carries `FORMAT_VERSION` and the parser rejects anything past
  `FORMAT_VERSION`), so an older build refuses to load a newer slot with
  `SnapshotError::UnsupportedFormat`.
- A newer build loading an older slot is permitted to fail with
  `SnapshotError::VersionMismatch { tag, file_version, chip_supports }`
  with a clear user-facing message.
- No migration code paths are required for the v1.x line. If a v2.x line
  introduces format breaks, ADR 0004+ will define explicit migration.

### Per-section version-bump checklist

Every PR that bumps a section version MUST:

1. Bump the constant (e.g. `CPU_SNAPSHOT_VERSION`, MMC5's internal
   `MMC5_SNAPSHOT_VERSION`, etc.).
2. Append new fields at the end of the body (never insert in the middle —
   that breaks truncate-on-old-reader behavior).
3. Document the bump in `CHANGELOG.md` under the relevant version, with a
   one-liner explaining what state was added.
4. Add a unit test confirming round-trip of the new section at the new
   version (existing `snapshot_round_trip_preserves_framebuffer_and_cycle`
   pattern in `nes.rs` covers this for the top-level path).

### Thumbnail section

A new optional `THM ` section is being introduced post-v0.9.0 for slot
preview thumbnails. Per this policy, it is:
- Tagged `THM ` (4 bytes).
- Optional — older builds simply skip the tag, and newer builds emit it
  unconditionally but tolerate its absence on load.
- Body format: version byte (currently `1`) + u16 width + u16 height + u32
  RGBA8 byte length + raw RGBA8 pixels.
- Sized to 128×120 RGBA8 = 61,440 bytes (1/4 native NES framebuffer, nearest-
  neighbor downsampled).
- Determinism: thumbnails are NOT part of the deterministic save-state
  contract. Two different builds may produce different pixel-perfect
  framebuffers at the same save-state cycle if non-deterministic post-pass
  filters (NTSC, scanline overlays) are tweaked. The thumbnail is a
  UI-side feature, not a replay-correctness feature.

## Consequences

### Positive

- Frontends and future netplay implementations can rely on a documented
  contract for save-state compatibility instead of cargo-culting from prior
  behavior.
- Adding state to a chip (e.g. when implementing VRC6 audio) becomes a
  three-step recipe (bump constant, append fields, add test) instead of an
  ad-hoc decision per chip.
- Older slot files keep working across PATCH bumps.

### Negative / Costs

- The "append fields at the end" rule constrains layout choices for big
  state additions (e.g. a 16 KiB lookup table can't be wedged in the middle
  of CPU state). In practice this is fine — chip state is small and additive.
- "Best-effort" across MAJOR/MINOR boundaries means users tracking nightly
  builds may need to abandon slot files at version transitions. We accept
  this; the rewind ring is per-session anyway, and slot files are user data
  not user output.

### Neutral

- No code change is required to ship this ADR — the policy is descriptive
  of behavior already implemented (the format itself supports both
  append-on-write and skip-on-read since Sprint 2).

## Alternatives considered

1. **`bincode`-style derive on the whole emulator struct.** Rejected in
   Sprint 2 planning because (a) it makes layout drift undetectable until a
   user-reported bug and (b) version migration requires bincode's own
   schema-evolution machinery, which we don't want as a hard dependency.

2. **Full migration code paths for every version bump.** Rejected as
   premature. The chip-state additions we've made so far have all been
   pure-add (MMC5 split-screen state, save-state version bumps are
   strictly tail-extensions). Migration code earns its keep only when a
   format breaks compatibility, which would be a v2.x event with its own ADR.

3. **A monolithic version byte in the container header** (e.g. bump
   `FORMAT_VERSION` from 1 to 2 every time any chip changes). Rejected
   because it conflates chip-level changes with container-level changes,
   forcing format-version checks at the parser level for what is really a
   per-section concern.

## Compatibility test plan

A unit test in `crates/rustynes-core/src/nes.rs` (`thumbnail_section_optional` or
similar) confirms a v0.9.0-shaped blob (no `THM ` section) round-trips
through the v1.x-shaped reader. The reader walks the section list, finds the
THM section absent, and gives the caller `None` for the thumbnail.

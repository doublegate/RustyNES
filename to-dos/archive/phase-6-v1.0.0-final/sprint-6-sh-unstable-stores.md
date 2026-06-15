# Sprint 6 — SH* unstable stores (internal-bus model)

**Phase:** 6 — v1.0.0 final
**Status:** OPEN (gated on Sprint 5 completion AND pass rate still
< 90%).
**Cascade risk:** **HIGHEST** — touches every test that exercises DMC
DMA + open-bus interactions.

**Note:** this sprint was previously deferred to v1.x per CLAUDE.md
internal-bus model. Promoted into v1.0.0 final on Option-B mandate as
a last-resort path to the 90% gate. **Only attempt if Sprints 1-5
have not collectively closed the gap.**

## Target tests (5 + 1 shared)

- `Unstable Stores :: SHA [error 7]`
- `Unstable Stores :: SHX [error 7]`
- `Unstable Stores :: SHY [error 7]`
- `Unstable Stores :: SHS [error 7]` (TAS)
- `Unstable Stores :: TAS [error 7]`
- `Open Bus [error 9]` (shared surface — internal-vs-external bus
  distinction)

Estimated yield: **+6 AccuracyCoin tests** (largest single-sprint
yield if successful).

## Hypothesis

The unofficial 6502 SH* family stores (SHA / SHX / SHY / SHS) and TAS
compute their address-high-byte AND-and-write value using the
**internal data bus** (CPU-only), not the external data bus (CPU +
DMA + open-bus latch). RustyNES currently models a unified data bus
which:

- Returns the correct value for the SH* tests when no DMA is active.
- Returns the wrong value when DMC DMA fetches interleave with the
  SH*opcode — because the DMC fetch updates the unified open-bus
  latch, which the SH* computation reads.

The fix: separate the bus model into:

- **Internal data bus** — CPU instruction fetch + ALU operand reads.
  Drives the SH* high-byte computation.
- **External data bus** — DMC DMA + CPU read/write to mapped
  addresses + open-bus latch updates.

DMC DMA fetches use external-only; the open-bus latch updates on
external-bus reads. The SH* opcodes compute their high-byte AND value
from the internal bus, which is unaffected by interleaving DMC DMA.

## Sprint plan

### Step 1 — Mesen2 cross-reference

Read Mesen2's `Core/NES/NesCpu.cpp` for the SH* family (search
`SHA / SHX / SHY / SHS / TAS` or `0x93 / 0x9E / 0x9C / 0x9B`). Confirm
whether Mesen2 maintains a separate internal/external bus model.

Read Mesen2's `Core/NES/NesApu.cpp` DMC DMA fetch path to confirm DMC
uses external-only.

### Step 2 — Bus model rework design

Design the internal-vs-external bus separation in
`crates/rustynes-core/src/bus.rs`:

- New field: `internal_data_bus: u8` (mirrors CPU ALU latch).
- Existing `open_bus: u8` → external data bus.
- `cpu_read` updates BOTH (external + internal).
- DMC DMA fetch updates ONLY external.
- The SH* high-byte AND value is sourced from `internal_data_bus`.

### Step 3 — CPU SH* opcode rework

In `crates/rustynes-cpu/src/cpu.rs`, the SH* opcodes (`SHA $9F / $93`,
`SHX $9E`, `SHY $9C`, `SHS $9B / TAS`) need:

- Use the internal data bus for the address-high-byte AND-and-write
  computation.
- Standard external-bus write for the final store.

Gate on feature flag `cpu-sh-internal-bus` (default off).

### Step 4 — Unit tests

- `crates/rustynes-cpu/tests/opcodes.rs` — for each SH* opcode, test the
  AND-with-high+1 behavior under (a) no DMA, (b) DMC DMA active.
- `crates/rustynes-core/tests/` — bus model unit test: internal bus
  unchanged by DMC DMA; external bus updated.

### Step 5 — Validation gauntlet

Standard 10-gate gauntlet. Special attention:

- `dmc_dma_during_read4/*` (5 strict): DMC DMA timing regression
  sentinel. Must remain 5/5 strict.
- `apu_test/*` (8 strict): DMC + open-bus shared surface.
- All 5 SH* tests flip.
- `Open Bus [error 9]` flips (shared internal-bus distinction).
- A previously-attempted internal-bus prototype (per CLAUDE.md
  "Phase D3" section) regressed `Internal Data Bus Test 2` and was
  rolled back. The new attempt MUST preserve `Internal Data Bus` at
  error 4 or better.

### Step 6 — Land OR rollback

Per Sprint 1 land/rollback discipline. Audit doc:
`docs/audit/sprint-6-sh-internal-bus.md`.

## Cascade-risk callouts

1. **Highest cascade risk of any sprint.** Touches the open-bus latch
   (every test that reads open-bus is affected), DMC DMA (every test
   that triggers DMC DMA is affected), and the SH* opcodes
   themselves (rarely exercised by commercial games, but the
   AccuracyCoin tests are dense).

2. **The prior internal-bus prototype regressed `Internal Data Bus
   Test 2`** (per CLAUDE.md project-status reference and the Phase D3
   diagnostic). The new attempt must not regress that test.

3. **The SH* opcodes are the canary for the internal-bus model.**If
   the 5 SH* tests do NOT flip after the model change, the model is
   wrong; do not land.

4. **Commercial games rarely use SH* (they are unofficial / unstable
   stores).** Sacred-trio risk is low, but the 60-ROM commercial-ROM
   oracle should still pass per the existing baselines.

## Estimated effort + yield

- **Effort:** 3-5 days (research + design + implementation +
  validation).
- **Yield:** +6 AccuracyCoin tests (5 SH* + 1 Open Bus). If this
  closes the 90% gate, the v1.0.0 final tag follows immediately.

## References

- nesdev `6502 unofficial opcodes` page (SHA / SHX / SHY / SHS / TAS
  semantics)
- AccuracyCoin source `AccuracyCoin.asm` (Unstable Stores suite block)
- Mesen2 `Core/NES/NesCpu.cpp` (SH* + internal bus reference impl)
- `crates/rustynes-core/src/bus.rs` (existing unified open-bus model)
- `crates/rustynes-cpu/src/cpu.rs` SH* opcode implementations
- `CHANGELOG.md` Phase D3 section (`$4015 / $4016 / $4017` open-bus
  semantics — adjacent surface)
- CLAUDE.md `## Open questions worth knowing` → "Deferred features"
  block (the v1.x SH* deferral; this sprint promotes it back to
  v1.0.0)

## Exit criterion

- All 5 SH* tests flip + Open Bus error 9 flips.
- No regressions in any of the 10 validation gauntlet gates,
  especially `Internal Data Bus Test 2`.
- AccuracyCoin pass rate reaches ≥ 90% → jump to v1.0.0 final tag.

## If Sprint 6 fails to reach 90%

Stop and re-negotiate with user. The 90% bar may need v1.x reframing
at that point. Document the trajectory across all 6 sprints:

```
82.73% (rc2 baseline)
     → Sprint 1 result
     → Sprint 2 result
     → Sprint 3 result
     → Sprint 4 result
     → Sprint 5 result
     → Sprint 6 result
```

Per `to-dos/phase-6-v1.0.0-final/sprint-gate-conditions.md`, this is
the escalation point.

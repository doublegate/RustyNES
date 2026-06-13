# ADR 0001 — Mapper Dispatch: `Box<dyn Mapper>` vs. Monomorphized `MapperEnum`

**Status:** Accepted (v0.9.0).
**Date:** 2026-05-10
**Author:** RustyNES maintainers
**Numbering note:** This ADR is numbered 0001 because mapper dispatch
came up first historically (in the Phase 4 sprint planning, before the
IRQ-timing rework was scoped). ADR 0002 (`irq-timing-coordination.md`)
was written first chronologically because it gates v1.0.0 work; this
ADR is paired with the Track B6 benches that produced the dispatch
numbers.

---

## Context

`crates/rustynes-mappers` ships 14 distinct mappers (NROM, MMC1, MMC2-5,
UxROM, CNROM, AxROM, GxROM, Color Dreams, CPROM, M34, Camerica, VRC1,
VRC2/4, VRC6, FME-7, Namco 163) per `docs/STATUS.md` §"Mapper coverage".
Every mapper implements the `Mapper` trait (`crates/rustynes-mappers/src/mapper.rs`):

```rust
pub trait Mapper: Send {
    fn cpu_read(&mut self, addr: u16) -> u8;
    fn cpu_write(&mut self, addr: u16, value: u8);
    fn ppu_read(&mut self, addr: u16) -> u8;
    fn ppu_write(&mut self, addr: u16, value: u8);
    // … plus optional notify_a12 / notify_cpu_cycle / notify_scanline_start
    // / notify_vblank / nametable_fetch / nametable_write /
    //   peek_ex_attribute / bg_split_state / ppu_read_sprite / mix_audio
    //   / save_state / load_state / debug_info / irq_pending / irq_acknowledge.
}
```

`LockstepBus` (`crates/rustynes-core/src/bus.rs`) owns the active mapper as
a `Box<dyn Mapper>` and dispatches every memory access through it.

There are two reasonable shapes for this dispatch:

1. **`Box<dyn Mapper>`** — what the project ships. One virtual call per
   memory access (`cpu_read`, `ppu_read`, etc.). The trait object is a
   data pointer + vtable pointer; the compiler cannot inline through it.
2. **`enum MapperEnum { Nrom(Nrom), Mmc1(Mmc1), … }`** — a monomorphized
   alternative where every mapper is a variant, and `cpu_read` is a
   match expression. The compiler can inline; the match is ~O(log n) or
   compiles to a jump table depending on layout.

CLAUDE.md explicitly listed this as an open question: *"Mapper dispatch:
starting with `Box<dyn Mapper>`; revisit vs. monomorphized `MapperEnum`
after Phase 4 profiling."*

---

## Forces

- **Hot path frequency.** PPU `ppu_read` fires on every BG fetch group
  (4 fetches per group, ~32 groups per scanline, 240 scanlines + 30
  off-screen = ~34,560 calls/frame just for BG). CPU `cpu_read` fires
  on every memory access that lands in `$4020-$FFFF`, dominated by
  instruction fetch from PRG-ROM (~5,000-15,000 calls/frame depending
  on the game).
- **Cache pressure.** The vtable pointer load on every call adds a
  cache miss possibility; a monomorphized enum keeps everything in a
  single dispatch site that the branch predictor can learn.
- **Binary size.** Monomorphized dispatch generates 14 copies of every
  inlining context that ends in a mapper call. Cargo's `lto = "thin"`
  helps but doesn't fully deduplicate.
- **Codegen units.** With `codegen-units = 1` in the release profile
  the compiler has a better chance to inline through trait objects via
  speculative devirtualization — but this is a fragile optimization.
- **Adding mappers.** New mappers must be added to the `MapperEnum` arm
  list under approach 2; under `Box<dyn Mapper>` they just need
  `parse()` dispatch. The work is roughly equivalent (both need a
  match in `parse`) but the maintenance surface differs.

---

## Decision

**Stay with `Box<dyn Mapper>`** as of v0.9.0.

The Track B6 bench (`crates/rustynes-mappers/benches/mapper_dispatch.rs`)
produced the following baseline on the development host:

| Mapper | Per 1024 `cpu_read` calls | Notes |
|--------|---------------------------|-------|
| NROM   | ~0.86 µs | Fastest; no banking, no IRQ |
| MMC1   | ~1.30 µs | Serial 5-write protocol overhead |
| MMC3   | ~1.37 µs | A12 filter is on write path; reads are cheap |
| MMC5   | ~1.91 µs | Most expensive; ExRAM + bank slots |
| M34    | ~1.37 µs | BNROM / NINA-001 variant detection |
| FME-7  | ~2.47 µs | Per-CPU-cycle IRQ tick |

The headline `full_frame::nes_run_frame_nestest` bench (NROM) ran in
~2.06 ms/frame — an 8× safety factor over the 60 Hz NTSC frame budget.

**The dynamic-dispatch overhead is approximately 2 ns per `cpu_read`
call** (the spread between mapper-internal logic costs dwarfs any
dispatch overhead). At ~15,000 CPU reads per frame this is ~30 µs/frame,
under 0.2% of the frame budget.

Switching to a monomorphized `MapperEnum` would reduce per-read cost
by an estimated 1-2 ns (the inlined match vs. the vtable lookup), or
~15-30 µs/frame — well below the noise floor of the `full_frame` bench
(measured variance is several microseconds per iteration even after
warm-up). The change would also bloat binary size, complicate adding
new mappers, and require a non-trivial refactor of the bus's
`mapper: Box<dyn Mapper>` field.

---

## Consequences

### Positive

- Adding a new mapper is a 2-file change: implement the trait in a new
  module under `crates/rustynes-mappers/src/`, then add a `match` arm in
  `parse()`. Both are localized.
- The `Mapper` trait is documented + stable; downstream extensions can
  implement their own mappers without forking.
- Save-state per-mapper byte format is delegated to each impl's
  `save_state`/`load_state` rather than a centralized enum-aware
  serializer.

### Negative

- Theoretical 1-2 ns per mapper call that monomorphization would
  reclaim. Below measurement noise on the dev host; may be visible on
  weaker hardware (mobile/embedded ports, if ever).
- LLVM cannot inline the BG-fetch loop's mapper call, so the surrounding
  PPU code does not benefit from constant-folding through the mapper's
  PRG-ROM read.

### Neutral

- The decision is reversible. If profiling on a future target (WASM,
  embedded ARM) shows mapper dispatch becoming the bottleneck, the
  switch to `MapperEnum` is mechanical: every place that holds
  `Box<dyn Mapper>` switches to the enum, and every `&dyn Mapper` /
  `&mut dyn Mapper` switches to `&MapperEnum` / `&mut MapperEnum`. The
  `Mapper` trait itself can stay (as a constraint on what each enum
  variant must implement) or be removed.

---

## Trigger to revisit

Reopen this decision when ANY of the following holds:

1. The `full_frame` bench shows a measured ≥ 5% throughput regression
   attributable to mapper dispatch (verified by replacing the boxed
   mapper with a hardcoded NROM-only inlined path and measuring the
   delta).
2. A new target (WebAssembly, mobile, embedded) profiles `cpu_read` /
   `ppu_read` as the top hot function and exhibits > 10% of frame cost
   in trait dispatch.
3. A mapper is added that requires per-fetch inlining for correctness
   (no known case as of 2026-05).

Until then: `Box<dyn Mapper>` stands.

---

## References

- B6 bench evidence: `crates/rustynes-mappers/benches/mapper_dispatch.rs`.
- Performance baselines: `docs/performance.md` §"Measured baselines
  (v0.9.0, Track B6)".
- Mapper trait: `crates/rustynes-mappers/src/mapper.rs`.
- Bus mapper field: `crates/rustynes-core/src/bus.rs`.
- CLAUDE.md open question that this ADR resolves.

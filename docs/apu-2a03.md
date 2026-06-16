# APU — Ricoh 2A03 audio unit

**References:** `ref-docs/research-report.md` §Technical deep-dive → APU;
`ref-docs/nesdev-wiki-technical-report.md` §APU; Nesdev
[APU](https://www.nesdev.org/wiki/APU),
[APU Frame Counter](https://www.nesdev.org/wiki/APU_Frame_Counter),
[APU DMC](https://www.nesdev.org/wiki/APU_DMC),
[DMA](https://www.nesdev.org/wiki/DMA), and
[Controller reading](https://www.nesdev.org/wiki/Controller_reading).

## Purpose

Implement the 2A03 APU in `crates/rustynes-apu`: five sound channels (pulse 1, pulse 2, triangle, noise, DMC), the 4-step or 5-step frame counter that drives sub-channel events, the nonlinear mixer, and the analog-style high-pass / low-pass filter chain. Output is band-limited (blip_buf-style) to a configurable host sample rate (typically 44.1 or 48 kHz).

## Interfaces

The implementation that landed in Phase 3 polled the bus differently from
the original sketch: rather than a callback-style `ApuBus` trait, the APU
exposes `dmc_dma_pending() / dmc_dma_addr() / complete_dmc_dma(byte)` that
the lockstep bus polls and services on its halt cycles.

```rust
pub struct Apu { /* opaque */ }

impl Apu {
    pub fn new(region: Region, sample_rate: u32) -> Self;
    pub fn reset(&mut self);
    pub fn tick(&mut self);                                  // 1 CPU cycle of APU work

    pub fn read_status(&mut self) -> u8;                     // $4015 with side effects
    pub fn write_register(&mut self, addr: u16, value: u8);  // $4000-$4017

    // DMC DMA cooperation with the bus.
    pub fn dmc_dma_pending(&self) -> bool;
    pub fn dmc_dma_addr(&self) -> u16;
    pub fn complete_dmc_dma(&mut self, byte: u8);

    // Audio drain (host sample rate).
    pub fn drain_audio(&mut self) -> Vec<f32>;
    pub fn drain_audio_into(&mut self, out: &mut [f32]) -> usize;

    pub fn frame_irq_pending(&self) -> bool;
    pub fn dmc_irq_pending(&self) -> bool;
    pub fn irq_line(&self) -> bool;        // either source asserting

    // Per-channel raw outputs for tests.
    pub fn pulse1_out(&self) -> u8;
    pub fn pulse2_out(&self) -> u8;
    pub fn triangle_out(&self) -> u8;
    pub fn noise_out(&self) -> u8;
    pub fn dmc_out(&self) -> u8;
}
```

The DMC sample DMA path is intentionally a **polling protocol on the
`Apu`**, not a callback trait. When the DMC bit-shift register empties,
`Apu::dmc_dma_pending()` returns `true` and `Apu::dmc_dma_addr()` exposes
the target address; the `LockstepBus` polls these on its halt cycles,
performs the read (which can stall the CPU for the documented 1-4 cycles
depending on what the CPU was doing), and feeds the byte back via
`Apu::complete_dmc_dma(byte)`. This keeps the `rustynes-apu` crate from
needing any reference (trait object or otherwise) to the bus, which in
turn keeps the workspace dep graph one-directional (`rustynes-apu` is a leaf;
see CLAUDE.md §"Workspace dependency graph is one-directional"). An
earlier sketch of an `ApuBus { fn dmc_read(...) }` callback trait was
considered but never wired in production — the polling shape is simpler
and avoids the trait-object indirection on the DMA-read hot path.

The APU is clocked by the master scheduler at CPU cadence (every other PPU dot triple on NTSC). The triangle wave timer runs at CPU clock; pulses, noise, and DMC timer-divide at half CPU clock. The frame counter divides further to ~240 Hz.

## State

- **Per channel**: 11/12-bit timer (counts down to reload), sequencer (4 step for pulse, 32 step for triangle, 1-bit LFSR for noise), length counter (5-bit, with halt flag), envelope (4-bit volume + decay), sweep (pulse only), linear counter (triangle only), DMC bit-shift register + sample buffer + memory reader.
- **Frame counter**: 4-step or 5-step mode, internal cycle counter (CPU clock granularity), IRQ inhibit flag, IRQ pending flag.
- **Mixer state**: high-pass filter state (two stages), low-pass filter state (one stage), output accumulator.
- **Sample emitter**: blip_buf-style ring of pending step responses + windowed-sinc kernel cache.

## Behavior

### Register map

Per `ref-docs/research-report.md` §APU:

| Addr | Name | Purpose |
|------|------|---------|
| $4000 | PULSE1_DDLC.NNNN | Duty (DD), envelope loop / length halt (L), constant volume (C), volume / envelope period (NNNN) |
| $4001 | PULSE1_EPPP.NSSS | Sweep enable (E), period (PPP), negate (N), shift (SSS) |
| $4002 | PULSE1_LLLL.LLLL | Timer low |
| $4003 | PULSE1_lllL.LHHH | Length counter load (lllL.L), timer high (HHH) |
| $4004-$4007 | PULSE2 | Same layout as Pulse 1 |
| $4008 | TRI_CRRR.RRRR | Length counter halt / linear counter control (C), linear counter reload (RRRR.RRR) |
| $400A | TRI_LLLL.LLLL | Timer low |
| $400B | TRI_lllL.LHHH | Length counter load + timer high |
| $400C | NOISE___LC.NNNN | Length halt (L), constant volume (C), volume / envelope period (NNNN) |
| $400E | NOISE_M___.PPPP | Mode (M, 0=15-bit / 1=6-bit), period index (PPPP) |
| $400F | NOISE_lllL.L___ | Length counter load |
| $4010 | DMC_IL__.RRRR | IRQ enable (I), loop (L), rate index (RRRR) |
| $4011 | DMC_.DDDD.DDDD | Direct DAC value (7-bit) |
| $4012 | DMC_AAAA.AAAA | Sample address ($C000 + A*64) |
| $4013 | DMC_LLLL.LLLL | Sample length (L*16+1) |
| $4015 | STATUS_IF__.DNT21 | IRQ flags (read), enable bits (write) |
| $4017 | FRAME_MI__.____ | Mode (M, 0=4-step / 1=5-step), IRQ inhibit (I) |

### Frame counter

Per `ref-docs/research-report.md` §Frame counter:

- 4-step (mode 0): clocks envelope+linear at every step, length+sweep at steps 2 and 4, frame IRQ at step 4 (if not inhibited). Total 14914 CPU cycles per loop NTSC.
- 5-step (mode 1): clocks envelope+linear at steps 1,2,3,5; length+sweep at 2 and 5; never sets frame IRQ. Total 18640 CPU cycles per loop NTSC.
- Writing `$4017` resets the counter with a 3- or 4-CPU-cycle delay (depending on whether the write happened on an even or odd CPU cycle); if mode 1 selected, immediately clocks the half-frame and quarter-frame events.

Nesdev's frame-counter timing is expressed in APU get/put cycle terms:
the reset side effects occur 3 CPU clocks after the `$4017` write if the write
lands during an APU cycle and 4 CPU clocks otherwise. The frame IRQ line is
connected to CPU IRQ; reading `$4015` returns the old frame IRQ status and then
clears the frame IRQ flag, while setting `$4017` bit 6 clears it immediately.
The DMC IRQ flag is not cleared by reading `$4015`.

PAL has separate frame-counter step positions. Do not derive PAL frame-counter
timing by scaling NTSC sample rates; use region tables.

### DMC channel

- **Memory reader**: when sample buffer is empty and bytes-remaining > 0, request DMA. Bus halts CPU and reads 1 byte from `$C000-$FFFF`. Halt cost: 3 or 4 CPU cycles per `ref-docs/research-report.md` §DMA. Read advances address (wraps `$8000` after `$FFFF`) and decrements bytes-remaining.
- **Output unit**: shift register bits modify the 7-bit output: bit 1 → +2, bit 0 → -2, clamped 0..=127.
- **IRQ**: when bytes-remaining reaches 0 and IRQ enable is set (and loop is not), assert DMC IRQ. Cleared by writing `$4015`.
- **Direct write to `$4011`** sets the DAC immediately, useful for raw PCM.

DMC DMA has two scheduling classes. Load DMA follows enabling playback through
`$4015` and is scheduled around the second APU cycle after the write. Reload DMA
follows the sample buffer emptying during playback and schedules on the opposite
get/put phase. Both perform a dummy cycle after halting the CPU and may need an
alignment cycle before the memory read. This distinction is observable through
CPU stalls and repeated side-effect reads.

### Mixer

Per `ref-docs/research-report.md` §APU Mixer, two implementations:

```rust
// Linear (first cut, fails apu_mixer test ROM)
pulse_out = 0.00752 * (pulse1 + pulse2);
tnd_out   = 0.00851 * triangle + 0.00494 * noise + 0.00335 * dmc;
output    = pulse_out + tnd_out;

// Lookup-table (~4% accurate, default)
pulse_table[n] = 95.52 / (8128.0 / n as f32 + 100.0);  // n=0 -> 0
tnd_table[n]   = 163.67 / (24329.0 / n as f32 + 100.0);
output = pulse_table[(pulse1 + pulse2) as usize]
       + tnd_table[(3 * triangle + 2 * noise + dmc) as usize];
```

After mixing, apply: 90 Hz first-order high-pass, 440 Hz first-order high-pass, 14 kHz first-order low-pass.

### Band-limited sample emission

Naive sample-rate conversion produces aliasing. Use a blip-buf-style ring buffer:

- Each "step" (channel transition) is registered with the time-of-step at CPU-cycle resolution.
- The buffer convolves each step against a windowed-sinc kernel into the host-sample-rate output buffer.
- `drain_samples()` returns finalized samples; the buffer slides forward in time.

Implementation: `blip_buf-rs` crate or hand-rolled equivalent (~200 LOC).

### `$4015` semantics

- **Read**: returns frame IRQ (bit 6), DMC IRQ (bit 7), DMC bytes-remaining > 0 (bit 4), pulse 1 / 2 / triangle / noise length-counter > 0 (bits 0-3). Reading clears the frame IRQ flag. **Does not** clear the DMC IRQ flag.
- **Write**: bit 4 set enables DMC (initiates sample if buffer empty); bit 4 clear silences DMC. Bits 0-3 enable channels (clearing forces length counter to 0).

`$4015` is internal to the CPU/APU package rather than an external-bus device.
When refining open-bus behavior, do not assume `$4015` reads update the same
external open-bus latch used by cartridge or PPU register accesses.

## Edge cases and gotchas

1. **DMC DMA stalls CPU mid-instruction.** Per `ref-docs/research-report.md` §DMA, halt only on read cycles. The 2A03 register-readout bug (extra reads of `$2007`, `$4015`-`$4017` while halted) must be reproduced — required by `dmc_dma_during_read4`.
2. **Frame counter write jitter.** Writing `$4017` with a value that includes IRQ inhibit set clears any pending frame IRQ flag.
3. **Length counter halt timing.** The halt flag is sampled at the right edge of the half-frame clock; common subtle bug is sampling on the wrong cycle.
4. **Triangle disabled silently when length counter or linear counter reaches 0.** Holds the last sequencer step (does not produce a click).
   - **Ultrasonic silence (timer period < 2).** When the triangle timer period is below 2 (frequency above ~55.9 kHz), real hardware cannot follow the sequencer and the channel effectively halts. We freeze the sequencer in `Triangle::clock_timer` (the step does not advance and the output holds its current value) rather than emitting the aliasing tone, matching the common-emulator convention; Mega Man 2's "Crash Man" stage relies on this to silence the triangle. The threshold is strictly `< 2` (period 2 still clocks). See `crates/rustynes-apu/src/triangle.rs`.
5. **Pulse duty-sequencer phase reset on `$4003`/`$4007`.** Writing the length/timer-high register resets the pulse duty sequencer to step 0 (and sets the envelope-restart flag) but does **not** reset the timer divider. Implemented in `Pulse::write_timer_hi` (`crates/rustynes-apu/src/pulse.rs`).
6. **DMC playback stops mid-scanline?** Yes; `$4015` write to clear bit 4 silences the channel after the current sample byte completes.
7. **Sweep mute.** When the target period of a pulse channel is > $7FF or the negated-target underflows below 8, the channel is muted regardless of length.
8. **Pulse 1 sweep negation off-by-one.** Pulse 1 negates by `~target` (one's complement); Pulse 2 negates by `-target`. This produces audible difference at certain frequencies.
9. **Controller conflict is APU-owned timing.** The standard controller code
   lives in the input subsystem, but DMC DMA is the root of the classic joypad
   bit deletion/duplication bug. APU/DMA changes must rerun controller-read
   coverage, not only APU audio ROMs.

## Test plan

- **`apu_test`** (8 sub-ROMs) — register I/O, frame counter, length counter halt timing.
- **`apu_mixer`** — confirms lookup-table mixer matches reference within 4%.
- **`dmc_dma_during_read4`** — DMC DMA stalls + register read crosstalk.
- **Audio capture comparison**: emit 60 frames of audio for a curated set of demo ROMs, compare PSNR against a Mesen-generated reference. (Not a strict pass/fail but a regression detector.)
- **Property test**: random `$4017` writes interleaved with channel writes; assert frame counter cycle accounting matches a hand-rolled reference.

## Open questions

- **Sample-rate conversion**: blip_buf-rs vs. hand-rolled. blip_buf-rs is a thin wrapper; we may inline it for fewer dependencies.
- **Audio API choice in cpal**: `f32` vs `i16` output streams. cpal supports both; default device choice depends on platform. Architecture: emit i16 internally, convert to f32 in the cpal callback if needed.
- **Mapper-extended audio.** VRC6 (3 channels), VRC7 (FM, 6 channels), Sunsoft 5B (3 channels), Namco 163 (8 channels), MMC5 (2 pulse + raw PCM), FDS (wavetable + envelope) — all need extra summing into the mix. Architecture: mappers expose a `fn mix_audio(&mut self) -> i16` called per APU sample; default impl returns 0.

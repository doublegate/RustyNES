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

The PAL (2A07) sequencer positions are (in CPU cycles since sequencer reset):

- **4-step (mode 0):** 8313 / 16627 / 24939 / 33252 / 33253 / 33254 — quarter
  at 8313 / 16627 / 24939 / 33253, half at 16627 / 33253, frame IRQ at 33252 /
  33253 / 33254 (if not inhibited).
- **5-step (mode 1):** 8313 / 16627 / 24939 / 41565 / 41566 — quarter at 8313 /
  16627 / 24939 / 41565, half at 16627 / 41565, no IRQ.

> **PAL frame-counter step positions are modeled (v2.1.5).**
> `crates/rustynes-apu/src/frame_counter.rs` selects the PAL positions above via
> the `FrameCounter::pal` selector, which `Apu::new` derives from the console
> `Region` (true only for `Region::Pal`; **NTSC and Dendy** keep the NTSC
> positions 7457 / 14913 / 22371 / 29828-29830, and 37281-37282 for mode 1).
> The NTSC arms are unchanged, so the default build and every NTSC/Dendy tick is
> **byte-identical** to the pre-v2.1.5 model — AccuracyCoin APU
> Frame-Counter-IRQ holds 141/141 and `apu_test` holds 8/8. The mode-0
> IRQ-flag-visibility / `irq_line_active` split is replicated verbatim at the
> PAL terminal steps (33252 / 33253 / 33254). The blargg `pal_apu_tests` oracle
> (see §Test plan) validates this: **all 10 sub-ROMs pass**, including all five
> PAL frame-counter-timing checks (clock jitter, mode-0/1 length timing, the two
> frame-IRQ timing checks) and — since the length halt/reload ordering fix
> below — `10.len_halt_timing` and `11.len_reload_timing`.

### Length halt/reload ordering vs the half-frame clock (v2.1.5)

The 2A03 applies a length-counter **halt** change (`$4000`/`$4004`/`$4008`/`$400C`
bit) and a length **reload** (`$4003`/`$4007`/`$400B`/`$400F` load) one step
*behind* the frame sequencer's half-frame length clock:

- **Halt takes effect after clocking length, not before.** A halt write on the
  exact CPU cycle of a half-frame length clock does **not** suppress that
  cycle's clock; it governs the *next* one.
- **A reload is ignored during a non-zero length clock.** A load on the
  half-frame-clock cycle is honoured only when the counter was **not** clocked
  this cycle (it was already zero, so the decrement was a no-op); if it was
  clocked from a non-zero value the load is dropped.

`crates/rustynes-apu/src/length.rs` models this with the deferral fields
`new_halt`, `reload_val` and `previous_count`: `set_halt` / `load` latch the
written values, and `LengthCounter::reload` — which `Apu::tick_with_external`
calls on all four length channels once per CPU cycle, **after** the half-frame
clock and **before** the mixer samples the channels — promotes the halt and
applies (or drops) the reload. This mirrors `TetaNES` `LengthCounter::reload`
and Mesen2's `_newHaltValue` + reload-request. The change is **region-agnostic
and byte-identical on NTSC**: on the common write cycle with no coincident
half-frame clock the reload settles in-cycle (identical to an immediate load),
and halt does not affect `output()` directly — so it only alters the exact
write-on-the-clock-cycle coincidence the ROMs probe. blargg's PAL
`10.len_halt_timing` / `11.len_reload_timing` flipped from `FAILED: #3` / `#4`
to `PASSED`; NTSC AccuracyCoin (141/141), `blargg_apu_2005` (11/11) and the
`f2a_*` length-race pins (`f2_accuracy_audit.rs`) are all unchanged.

### Reset behavior (v2.0.0 "Timebase", promoted in beta.4)

Per the blargg `apu_reset` spec and nesdev ("At reset, `$4017` mode is
unchanged, but IRQ inhibit flag is sometimes cleared"): the frame counter
retains the last value written to `$4017` (`FrameCounter::last_4017`), and a
warm reset behaves as if that value were written AGAIN — the reset zeroes the
sequencer + IRQ flags, cancels any in-flight pre-reset `$4017` write still in
its 3/4-cycle maturation window, and SCHEDULES a re-write of
`last_4017 & 0x80` (mode bit retained, IRQ-inhibit bit cleared) landing 2
clocked cycles into the CPU's 8-cycle reset sequence. The re-write flows
through the normal `$4017` write path (the 3/4-cycle aligned delay + the
mode-1 immediate quarter/half clock), so execution resumes ~9–12 cycles after
the effective write — blargg `4017_timing` measures 8 (its accept window is
6..=12; hardware-typical is 9). `$4015` is cleared at reset (channels
disabled); the channel registers — including the halt/duty bits — survive.
This closes plan-residual R4 (`apu_reset/4017_written`): all six blargg
`apu_reset` ROMs pass strictly.

**Save-state coverage (`APU_SNAPSHOT_VERSION` v4).** The scheduled re-write is
live state for the two CPU cycles between `Apu::reset` arming it and
`tick_with_external` firing it, so `reset_4017_delay` + `reset_4017_value` are
serialized. They were not before v4: a snapshot landing in that window restored
`delay = 0`, cancelling the re-write, and the restored frame counter kept the
sequencer phase the re-write exists to reset. The window is narrow and no
user-visible symptom was ever attributed to it — unlike the PPU's v5/v6/v8
tails, this one was found by the standing schema audit
(`crates/rustynes-test-harness/tests/snapshot_schema_audit.rs`) rather than by a
bug report. Pinned behaviourally by
`a_reset_survives_a_snapshot_restore_taken_mid_countdown`, which compares
`frame_counter.cycle` across a mid-countdown round trip; note that
`frame_counter.mode` cannot serve as the oracle, since `reset_rewrite_4017`
retains bit 7 and the re-write therefore restores the mode already in effect.
v1..=3 blobs upconvert to "no re-write pending", the resting value.

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

#### DMC load-DMA even/odd-cycle delay (v1.7.0 F2b)

A DMC sample-buffer **LOAD** DMA that begins on a "get" (odd) CPU cycle is
deferred one extra cycle relative to one that begins on a "put" (even) cycle —
the load only takes effect on its put half. This is modelled by
`Bus::dmc_dma_defer_load_entry` in `crates/rustynes-core/src/bus.rs`, which gates
the load entry on the APU's `put_cycle()` parity (on the current dot-lockstep
scheduler the pre-cycle parity is read flip-invariant; see the in-source note for
why the predicate is `put_cycle` rather than `!put_cycle`). The behavior is
**already implemented** and verified, not new in v1.7.0; the
`f2b_*` tests in `crates/rustynes-test-harness/tests/f2_accuracy_audit.rs` pin it
end-to-end via `dmc_tests/latency.nes` (a deterministic DMC fetch-latency audio
signature) and the strictly-passing `sprdma_and_dmc_dma` alignment ROM.

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

**Filter model (v2.1.3).** The three-stage chain above is the **NES front-loader**
(RF/composite) circuit and is the default (`FilterModel::NesRf`, byte-identical to
earlier builds — it matches ares/tetanes). Because that 440 Hz high-pass rolls off
the bass/triangle register hard (an authentic but *thin* sound), `Apu::set_filter_model`
also offers two softer, hardware-grounded models: **`Famicom`** (a single ~37 Hz
high-pass — the nesdev Famicom spec, fuller low end) and **`Clean`** (only a ~10 Hz
DC-block — fullest, the character Mesen2 / FCEUX / Nestopia produce by omitting the
high-pass cascade). The model is tonal only — channel content is identical, it is
never written into the save state, and the frontend re-applies it at ROM load — so
determinism and the audio oracle hold on the default. Frontend selector: **Settings
→ Audio → Filter model** (`[audio] filter_model` = `nes` / `famicom` / `clean`).

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
3. **Length counter halt / reload race (v1.7.0 F2a; ordering fixed v2.1.5).** The effective halt flag is consulted at the half-frame length clock; a `$400x` halt-bit write — or a length **reload** — on the CPU cycle of that clock races over whether the counter is clocked this step. Silicon resolves the halt change *after* the clock and drops a reload that lands on a non-zero clock. This is modeled by the deferral mechanism in `length.rs` (`new_halt` / `reload_val` / `previous_count`, promoted by `LengthCounter::reload` after the half-frame clock and before the mixer sample — see §Length halt/reload ordering above). blargg `10.len_halt_timing` + `11.len_reload_timing` bracket the exact cycle and pass strictly on **both** the NTSC (`blargg_apu_2005.07.30`) and PAL (`pal_apu_tests`) builds. The `f2a_*` tests in `crates/rustynes-test-harness/tests/f2_accuracy_audit.rs` are the named NTSC regression pin.
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
- **`pal_apu_tests`** (10 sub-ROMs, **PAL** region) — blargg's PAL-calibrated
  rebuild of the 2005-era APU length/frame-IRQ/timing checks. Wired in v2.1.5
  as the first PAL-region APU oracle (`tests/pal_apu_tests.rs`). These predate
  the `$6000` protocol and report **on-screen** (plain NROM, no PRG-RAM), so
  the suite decodes the rendered `PASSED` / `FAILED: #<n>` verdict via the
  `run_nes_screen` harness runner rather than the (vacuous, for these ROMs)
  `$6000` check. Current state: **10/10 pass** — `01.len_ctr` /
  `02.len_table` / `03.irq_flag` (region-independent) plus `04.clock_jitter`,
  `05`/`06.len_timing_mode0`/`1`, `07.irq_flag_timing`, `08.irq_timing` (the
  PAL frame-counter-timing checks, passing since the v2.1.5 PAL step positions),
  and `10.len_halt_timing` / `11.len_reload_timing` (passing since the v2.1.5
  length halt/reload ordering fix documented above and in
  `docs/accuracy-ledger.md`).
- **`apu_mixer`** — confirms lookup-table mixer matches reference within 4%.
- **`dmc_dma_during_read4`** — DMC DMA stalls + register read crosstalk.
- **Audio capture comparison**: emit 60 frames of audio for a curated set of demo ROMs, compare PSNR against a Mesen-generated reference. (Not a strict pass/fail but a regression detector.)
- **Property test**: random `$4017` writes interleaved with channel writes; assert frame counter cycle accounting matches a hand-rolled reference.

## Expansion-chip audio

Six on-cart expansion sound chips are synthesized and summed into the external-audio mix via the `Mapper::mix_audio(&mut self) -> i16` hook (default 0). Each synth core lives in the owning mapper crate, **not** the 2A03 APU crate, because they are cartridge hardware:

| Chip       | Mapper(s)        | Synth core                                            | Clock cadence                  |
|------------|------------------|-------------------------------------------------------|--------------------------------|
| VRC6       | 24 / 26          | `Vrc6Pulse` x2 + `Vrc6Saw` (`crates/rustynes-mappers/src/m024_vrc6.rs`) | every CPU cycle (`$9003` halt + freq-scale shift) |
| VRC7       | 85               | `rustynes_apu::Opll` (emu2413-derived, MIT)           | OPLL `calc()` every 36 CPU cycles (49,716 Hz)      |
| FDS        | 20 (FDS device)  | `FdsAudio` wavetable + FM (`crates/rustynes-mappers/src/fds.rs`) | wave/mod every 16 CPU cycles; envelopes per cycle |
| MMC5       | 5                | `Mmc5Audio` (2 pulse + 7-bit PCM, `crates/rustynes-mappers/src/m005_mmc5.rs`) | pulse timer every other CPU cycle; envelope/length on 2A03 frame events |
| Namco 163  | 19 / 210         | `Namco163Audio` (1-8 time-multiplexed wavetable channels) | round-robin channel update every 15 CPU cycles    |
| Sunsoft 5B | 69 (FME-7)       | `Sunsoft5BAudio` (3 tone + noise + envelope)          | every CPU cycle                |

All synth cores are behind the default-on `mapper-audio` Cargo feature; when it is off (e.g. the `no_std` build) the register decoders still latch (save-state round-trip preserved) but `clock`/`mix` are no-op shims that return silence. The VRC7 OPLL core is deliberately the MIT `emu2413` lineage — **not** Nuked-OPLL (GPL/LGPL, license-incompatible).

### Expansion-audio levels (v2.1.6 "Expansion Audio")

Each chip's `mix_audio()` is scaled so its full-volume square sits at the **relative loudness the hardware and Mesen2 (RustyNES's accuracy bar) produce vs the 2A03 pulse**, measured by the bbbradsmith `db_*` decibel-comparison ROMs. The reference is Mesen2 `NesSoundMixer::GetOutputVolume` (2A03 pulse peak `95.88*5000/(8128/15+100) ≈ 746.9`; linear expansion weights VRC6 `×5`·internally-`×15`, MMC5 `×43`, N163 `×20`, 5B `×15`, VRC7 `×1`), cross-checked against nestopia / puNES / fceux / tetanes. The `crates/rustynes-test-harness/tests/audio_expansion.rs` `level_db_*` oracle asserts the measured expansion-vs-reference ratio from each ROM's rendered waveform:

| Chip (ROM)        | Target ratio vs APU square | RustyNES scale (`mix_audio`)         | Status |
|-------------------|----------------------------|--------------------------------------|--------|
| APU triangle (`db_apu`) | ≈ 0.524 (fixed 2A03 DAC balance) | `pulse_table` / `tnd_table` LUT   | **Asserted** |
| VRC6 (`db_vrc6a/b`)     | ≈ 1.506                   | `VRC6_MIX_SCALE = 979` (`m024_vrc6.rs`; was 256) | **Asserted** (v2.1.6) |
| MMC5 (`db_mmc5`)        | ≈ 1.000 ("equivalent to APU") | pulse `×650` / PCM `×40` (`m005_mmc5.rs`; was 256/16) | **Asserted** (v2.1.6) |
| Namco 163 1-ch (`db_n163`) | ≈ 6.02                | `NAMCO163_MIX_SCALE = 261` (`m019_namco163.rs`; was 64) | **Asserted** (v2.1.6) |
| Sunsoft 5B (`db_5b`)    | ≈ 1.265 (vol-12) / 3.554 (vol-15) | shape `SUNSOFT5B_LOG_VOL` + level `SUNSOFT5B_MIX_SCALE_NUM/DEN = 2549/138` | **Asserted** (v2.2.3) |
| VRC7 (`db_vrc7`)        | ≈ 2.7 peak (patch-dependent) | raw `Opll::calc()` (`±4095`)      | **Snapshot-guarded** — see below |

VRC6 (1.506), MMC5 (1.0) and N163 (6.02) were the v2.1.6 level corrections; MMC5's `mix_audio` bias moves to `-12290` accordingly. **VRC6/MMC5/N163 fixes touch only the expansion channel** — the base 2A03 mix is a separate additive term (`mix_audio() == 0` for non-expansion mappers), so AccuracyCoin / blargg / nestest stay byte-identical.

**Sunsoft 5B absolute level — closed in v2.2.3 (A1).** The log-volume DAC *shape* was always hardware-exact (`×1.4126`/step, verified by `sunsoft5b_volume_dac_follows_logarithmic_step_law`); the *level* was deferred for one reason only — `Mapper::mix_audio` returned `i16`, and a full-volume tone at the `db_5b` level is `1882 × 18.471 = 34,761`, past `i16::MAX` for a single channel (three simultaneous tones ≈104 k, 3.2× over). The trait return is now **`i32`**, and the level is calibrated by `SUNSOFT5B_MIX_SCALE_NUM/DEN = 2549/138 ≈ 18.471`: measured `0.0685×` before (~23 dB too quiet), **1.2651×** after, against the Mesen2-derived target `LUT[12]=63 × weight 15 / 746.9 = 1.265`. Asserted by `level_db_5b`. Shape and level are now separately pinned, each by its own oracle. The widening is representational for every other board — they return the values they always did.

One level remains an honest documented gap (`docs/accuracy-ledger.md` §Expansion-audio levels):

- **VRC7 FM level.** The OPLL FM synthesizer *is* implemented (emu2413 port) and its instrument ROM is verified canonical (`vrc7_all_15_melodic_patches_match_nuke_ykt_canonical` in `rustynes_apu::opll` — that is the `patch_vrc7` criterion). The absolute FM output vs the APU square is a pseudo-sine (not a square) and patch/TL/feedback-dependent, so it is not cleanly oracle-pinned; the `db_vrc7`/`clip_vrc7` ROMs stay byte-exact snapshot regression guards.

### NSF expansion-audio routing (v1.7.0 "Forge" G2/G3)

A classic `.nsf` may declare expansion audio in the `$07B` bitfield (bit 0 VRC6, 1 VRC7, 2 FDS, 3 MMC5, 4 N163, 5 5B). The NSF player (`crates/rustynes-mappers/src/nsf.rs`) does **not** reimplement any synthesis: `crates/rustynes-mappers/src/nsf_expansion.rs` (`NsfExpansion`) owns instances of the **exact same** cores listed above and routes the NSF register windows into them — `$9000-$B002` (VRC6), `$9010`/`$9030` (VRC7), `$4040-$408A` (FDS), `$5000-$5015` (MMC5), `$4800`/`$F800` (N163), `$C000`/`$E000` (5B) — clocking on `notify_cpu_cycle` and fanning APU frame events (MMC5 envelope/length) on `notify_frame_event`. Because the bit-for-bit math is shared with the cartridge path, an NSF VRC6 tune sounds identical to a VRC6 cartridge. The `$5FF8-$5FFF` bank registers retain priority over the overlapping expansion windows. `NsfExpansion` is constructed only for NSF files and is unreachable from any oracle cartridge ROM, so it cannot perturb existing AccuracyCoin / blargg / kevtris audio.

**MMC5 expansion audio (G3)** was the one chip whose synthesis was started-but-deferred for NSF use; the cartridge `Mmc5Audio` core (2 pulse + raw PCM) is now driven for both cartridge-MMC5 and NSF-MMC5 playback through the shared router.

## Open questions

- **Sample-rate conversion**: blip_buf-rs vs. hand-rolled. blip_buf-rs is a thin wrapper; we may inline it for fewer dependencies.
- **Audio API choice in cpal**: `f32` vs `i16` output streams. cpal supports both; default device choice depends on platform. Architecture: emit i16 internally, convert to f32 in the cpal callback if needed.

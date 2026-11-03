<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
# Audio stems and NSF ripping

> **Status:** implemented in v3.6.0 (`T-AUDIO-STEMS`, D33 power-user line).
> This document is the spec of `rustynes stems` and `rustynes nsf-rip`; it
> changes in the same commit as `crates/rustynes-core/src/stems.rs`,
> `crates/rustynes-core/src/nsf_rip.rs` or
> `crates/rustynes-frontend/src/stems_cli.rs`.

Two headless tools for working with a game's music:

```text
rustynes stems   <ROM|NSF|FDS> [--out DIR] [--frames N | --movie M] [--rate HZ]
                 [--filter nes|famicom|clean] [--bios FILE]
rustynes nsf-rip <ROM> [--out FILE.nsf] [--frames N | --movie M]
                 [--name TEXT] [--artist TEXT] [--copyright TEXT]
```

Both build a bare console with no window, audio device or host input, so the
result depends only on the files and the deterministic core. Without
`--movie`, `--frames` input-free frames run (default 3,600, one NTSC minute).

## `rustynes stems`

Writes one WAV per sound source, beside the console's own mix, from **one
emulation pass**:

| File | Contents |
| --- | --- |
| `mix.wav` | the console's output, exactly what a host would play |
| `pulse1.wav`, `pulse2.wav` | each pulse channel alone |
| `triangle.wav`, `noise.wav`, `dmc.wav` | each channel alone |
| `expansion.wav` | the cartridge's expansion audio (VRC6, VRC7, Namco 163, Sunsoft 5B, MMC5, FDS, or an NSF's chips); silent on a board without any |
| `interaction.wav` | the mixer's interaction term (below); with it, the seven tracks sum to the mix |

All eight are mono 32-bit IEEE-float WAVs (`WAVE_FORMAT_IEEE_FLOAT`, with the
`fact` chunk a non-PCM file carries) at `--rate` Hz (default 48,000), all the
same length, sample-aligned. Float keeps the exact values, so the sum identity
holds in the files as it does in memory, and nothing clips.

An NSF (`NESM` or `NSFE`) plays through the NSF player; an FDS disk needs its
BIOS (`--bios`, which RustyNES never ships). `--filter` selects the analog
output filter model (`docs/apu-2a03.md` §Mixer); every track goes through it.

Exit codes: **0** written, **4** the re-rendered mix did not match the
console's (the files are written but are not what the console played, and the
command says so), **1** an error (a file, the image, the movie, a capture that
could not start). 2 stays clap's usage error.

Example, the vendored bbbradsmith VRC6 level ROM (dev build, 5.0 s):

```text
$ rustynes stems tests/roms/audio-tests/db_vrc6a.nes --frames 650 --rate 44100 --out stems
tests/roms/audio-tests/db_vrc6a.nes: 650 frames, 476216 samples per track at 44100 Hz, written to stems
  track                    peak        rms
  mix                  0.385276   0.039392
  pulse1               0.150417   0.022963
  pulse2               0.000000   0.000000
  triangle             0.223227   0.000906
  noise                0.000000   0.000000
  dmc                  0.514057   0.030785
  expansion (VRC6)     0.259986   0.022058
  interaction          0.128781   0.007578
  mix re-rendered from the trace: 0 of 476216 samples differ from the console's
  largest |mix - sum of tracks|: 3.5762787e-7 (f32 rounding; ...)
```

### How the stems are made

The emulation runs once, unchanged, with the APU's audio provenance armed
([`audio-provenance.md`](audio-provenance.md)). Armed, the APU records for
every CPU cycle the five raw channel outputs it handed the mixer and the raw
expansion sample. After each frame `StemCapture` reads that trace and pushes
each source **alone** through a private copy of the console's output pipeline:
the same lookup-table mixer, a band-limited buffer built with the same sample
rate and CPU clock (PAL's on a PAL image), and the same filter model.

Nothing is added to the APU's per-cycle work and nothing in the emulation
changes: the stems are a function of a trace the core already recorded. A run
without the capture is byte-identical to before. (No hot-path code changed in
v3.6.0 for this feature, so `scripts/perf/ab_check.sh` had nothing to measure;
an armed run pays the mix trace's documented cost plus eight band-limited
buffers.)

The reset sequence ticks the APU 8 cycles inside the console's constructor,
before any trace can be armed. The capture replays those cycles from the
channels' power-on outputs, which have not changed (no register has been
written yet); the mix check below proves the replay on every run. A capture
must therefore start before the first frame: one started later, or a movie
that begins from a save state, is refused (`StemError::StartedLate`), and a
frame run without being captured is refused too (`StemError::Gap`).

### What "the stems sum to the mix" means

The 2A03 mixer is not linear (nesdev "APU Mixer"):

```text
out = pulse_table[p1 + p2] + tnd_table[3*tri + 2*noise + dmc] + expansion
pulse_table[n] = 95.52 / (8128 / n + 100)
tnd_table[n]   = 163.67 / (24329 / n + 100)
```

Both tables are concave: two channels sounding together come out quieter
than each alone, added up. A stem here is its channel **alone through that
same mixer** (`pulse_table[p1]`, `tnd_table[2*noise]`, ...), because that is
what the console plays with every other channel silent. So the six source
stems do not sum to the mix; they overshoot it by the **interaction term**

```text
interaction = out - (pulse_table[p1] + pulse_table[p2] + tnd_table[3*tri]
                     + tnd_table[2*noise] + tnd_table[dmc] + expansion)
```

which is zero while at most one channel of each group (pulses; triangle,
noise and DMC) is non-zero, and never positive before the filter. It is
written as the seventh track. The band-limited buffer and the filter chain are
linear, so `mix = Σ six stems + interaction` to f32 rounding. The expansion
audio enters the mixer linearly and has no interaction term.

How big the interaction is, measured: on the test ROM that sounds all five
2A03 channels at once, its peak is 0.135 against a mix peak of 0.472, so a
mixdown of the six stems alone would be audibly louder than the console. On
`db_vrc6a` it peaks at 0.129.

### The checks, and what each one proves

| Check | Proves | Measured |
| --- | --- | --- |
| The mix re-rendered from the trace equals the console's output, bit for bit (`StemCapture::mix_mismatches`, reported by the CLI, exit 4 if not zero) | the private pipeline is the console's pipeline, not a lookalike | 0 mismatches on every run: NROM, PAL + Famicom filter, VRC6, VRC7, N163, MMC5, 5B, FDS, NSF, after a power cycle |
| Each source stem equals the console run with only that channel's mask bit set (`Nes::set_apu_channel_mask`), sample for sample (core tests) | each stem is "that channel alone through the same mixer", which the APU's own mask computes independently | 0 differing samples for pulse 1, pulse 2, triangle, noise, DMC (60 frames), the VRC6 (650 frames) and the FDS (30 frames) |
| The largest absolute difference between the mix and the sum of the seven tracks (`StemCapture::max_sum_error`, reported by the CLI) | the tracks are aligned and the pipeline linear | 4.2e-7 (all-channel NROM, peak 0.47), 3.6e-7 (VRC6, 650 frames), 8.3e-7 (N163); the tests bound it at 1e-5 |

The sum check alone cannot catch a wrong stem: the interaction track is
defined as the remainder, so the seven always add up. The masked-console
check is the one that can, and the mutation that swaps the triangle into the
noise slot is caught by it, not by the sum.

### What is not split

- **One expansion stem per board.** The trace records the board's summed
  expansion output (`Mapper::mix_audio`). A cartridge has one chip, so its
  stem is that chip. An NSF that enables several chips gets them summed into
  the one stem; splitting them would need a per-chip record in the mapper's
  per-cycle path, which this design deliberately does not add.
- **A chip's own voices** (VRC6's two pulses and saw, N163's channels, the 5B's
  three tones) are not split.
- **A host's channel mask or gain** is not applied to the stems (the trace
  records raw outputs). With one set, the mix check fails and says so.

## `rustynes nsf-rip`

Finds a game's music driver, proposes its `init` and `play` addresses with
the evidence for each, and with `--out` writes an NSF (version 1, one song,
nesdev "NSF") and **verifies it**: the NSF is played back in RustyNES's NSF
player and traced the same way, and its `play` calls must make the game's APU
register writes, call for call.

Exit codes: **0** proposed (and, with `--out`, written and verified), **4** no
driver found, a board or driver that cannot be imaged, or a failed
verification, **1** an error.

Example, the homebrew music ROM the tests use
(`rustynes_core::nsf_rip::example_music_rom`):

```text
$ rustynes nsf-rip music.nes --frames 300 --out music.nsf --name Example
music.nes: traced 300 frames
play $C059: encloses every APU write in 38 of the 39 frames that wrote the APU; called in 298 frames, 1 time(s) per frame; encloses 228 of 231 writes
init $C045: encloses all 3 APU writes made outside play; called 1 time(s)
wrote music.nsf (32896 bytes)
verified: the NSF's first 298 play calls make the game's APU writes (38 of them write)
```

### How the driver is found

The console is stepped one instruction at a time while a **shadow call stack**
follows the program: a `JSR` pushes its target; an interrupt pushes its
vector (a step that pushed three bytes and landed on the NMI or IRQ/BRK
vector's target; the CPU services an interrupt as a step of its own, so the
instruction at the PC was not executed and a `JSR` there is not a call); an
entry is popped when the stack pointer climbs back to where it was before the
call. The CPU bus access log names every write; each write to an APU register
(`$4000-$4013`, `$4015`, `$4017`: not `$4014`, OAM DMA, nor `$4016`, the
controller strobe) is recorded with the chain of calls it happened under.

- **play** is the routine that encloses every APU write of a frame, in the
  most frames. When several do (an NMI body that calls the driver encloses
  the same writes), the innermost is chosen, since the outer one also does
  the game's other per-frame work.
- **init** is the innermost routine enclosing every APU write made outside
  `play`. When there is none (the reset code writes the APU inline, or never
  outside play), the NSF's init points at an `RTS` byte in the image, and the
  report says so: the NSF player clears RAM before init, which is often all an
  engine needs.

These are heuristics over one run. The report gives the counts to judge them
by, and the playback verification is the gate.

### What can be ripped

The image is the cartridge's PRG-ROM loaded at `$8000`: mappers 0 (NROM) and 3
(CNROM, whose banking is CHR only), 16 KiB mirrored to fill `$8000-$FFFF` as
the board does, or 32 KiB. Refused, with the reason:

- a board that bank-switches its PRG (every other mapper): the NSF's 4 KiB bank
  registers would have to be set to the layout the driver runs under, which is
  not inferred;
- a driver that runs from RAM (the game copied it there): it is not in the
  image, and the player clears RAM before init. The bbbradsmith audio ROMs run
  from RAM, for example, and are refused on both counts.

The play speed is the hardware frame (16,639 µs NTSC, 19,997 PAL). A driver
seen running more than once a frame is still filed at the frame rate and the
report notes it.

### The NSF player fix this found

Ripping's playback gate found that RustyNES's NSF player had not called a
60 Hz tune's `play` since v2.9.8: its driver enabled the NMI inside the PPU's
post-reset warm-up window, where the write is dropped, and enabled interrupts
with the APU frame IRQ still live. v3.6.0 fixes the driver
(`docs/mappers.md` §NSF player, `CHANGELOG.md` "Fixed").

## Verification

```bash
cargo test -p rustynes-core --features debug-hooks,hd-pack --lib stems::   # 16 tests
cargo test -p rustynes-core --features debug-hooks,hd-pack --lib nsf_rip:: #  8 tests
cargo test -p rustynes-frontend --lib stems                                #  7 tests
```

The ROMs the tests run are written for them (a five-channel NROM, a music
NROM, an FDS disk with a synthetic BIOS) or vendored public test ROMs
(`tests/roms/audio-tests/`); no commercial ROM is involved.

## Provenance

Written from nesdev's "APU Mixer", "NSF", "PPU power up state" and "FDS
audio" pages and from RustyNES's own mixer, band-limited buffer and NSF
player. No reference emulator source was consulted.

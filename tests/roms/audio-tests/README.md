# `audio-tests/` — Expansion-Audio Test ROMs

Test ROMs and NSFs for NES / Famicom expansion audio chips. The upstream
distribution is at <https://github.com/bbbradsmith/nes-audio-tests>
(Brad Smith, `bbbradsmith`). Pre-built ROMs are taken from the upstream
`build/` directory.

The upstream license (from `readme.md`) is:

> These files may be freely redistributed and modified for any
> purpose. Credit to the original author and/or a link to the
> original source would be appreciated, but is not required.

This is effectively a public-domain dedication; the credit request is
a courtesy, not a redistribution restriction. The upstream readme is
mirrored verbatim as [`UPSTREAM_README.md`](./UPSTREAM_README.md).

## ROMs vendored

The full upstream `db_*` (decibel-comparison) family, plus the VRC7 /
N163 / 5B / APU quirk tests, are included.

| File | Mapper | Purpose |
|------|--------|---------|
| `db_apu.nes` | NROM (0) | Reference: full-volume APU square vs. APU triangle. Baseline for the other `db_*` ROMs. |
| `db_vrc6a.nes` | VRC6a (24) | Full-volume APU square vs. full-volume VRC6 square. Akumajou Densetsu pinout. |
| `db_vrc6b.nes` | VRC6b (26) | Same as above with the Madara pinout. |
| `db_vrc7.nes` | VRC7 (85) | Full-volume APU square vs. full-volume VRC7 pseudo-square (2:1 modulator at 50%, full feedback). |
| `db_n163.nes` | Namco 163 (19) | Full-volume APU square vs. full-volume N163 square, 1-channel mode. |
| `db_5b.nes` | FME-7 / 5B (69) | Full-volume APU square vs. volume-12 Sunsoft 5B square. |
| `db_mmc5.nes` | MMC5 (5) | Full-volume APU square vs. full-volume MMC5 square. |
| `test_vrc7.nes` | VRC7 (85) | Properties of the VRC7 "test" register `$0F` (chip-reset behaviour). |
| `test_n163_longwave.nes` | Namco 163 (19) | Long-period wavetable values often neglected by emulators. |
| `patch_vrc7.nes` | VRC7 (85) | Built-in VRC7 patch-set comparison vs. the prospective set. |
| `clip_vrc7.nes` | VRC7 (85) | Clipping in the VRC7 amplifier. |
| `noise_vrc7.nes` | VRC7 (85) | White noise to characterize VRC7 internal filters. |
| `clip_5b.nes` | FME-7 / 5B (69) | Nonlinearity in the 5B amplifier. |
| `noise_5b.nes` | FME-7 / 5B (69) | White noise + frequency tests for the 5B filters. |
| `sweep_5b.nes` | FME-7 / 5B (69) | Frequency sweep for 5B filter characterization. |
| `envelope_5b.nes` | FME-7 / 5B (69) | 5B envelope frequency / phase-reset verification. |
| `phase_5b.nes` | FME-7 / 5B (69) | 5B tone phase behaviour. |
| `tri_silence.nes` | NROM (0) | APU triangle silence quirk (interaction of `$4008`/`$400B` with the linear counter). |
| `dac_square.nes` | NROM (0) | APU square-channel DAC linearity. |

## What these ROMs test

These exercise behaviour the blargg / kevtris suites do **not** cover:

- **Relative audio levels** (`db_*`). Each `db_*` ROM is a "hotswap"
  test: on a real cart you boot it on a dev cart, swap in the expansion
  cart, and listen for whether the expansion chip is louder, quieter,
  or equal to the APU square. For an emulator the same comparison is
  performed by waveform inspection of the output buffer.
- **VRC7 OPLL register surface**. RustyNES implements the VRC7
  banking / IRQ / register-shadow path; the OPLL FM synthesizer itself
  is deferred per ADR-0004 (`docs/adr/0004-vrc7-audio-deferred.md`).
  The `test_vrc7` / `patch_vrc7` / `clip_vrc7` / `noise_vrc7` ROMs
  validate the parts that *are* implemented and serve as fixtures for
  the future OPLL landing.
- **N163 long-period accuracy**. `test_n163_longwave` exercises the
  long-period wavetable edge case that several open-source emulators
  truncate.
- **5B / FME-7 envelope quirks** (`envelope_5b`, `phase_5b`). The
  envelope generator and tone phase-reset semantics relevant to
  Gimmick! and Hebereke.
- **APU DAC linearity** (`dac_square`). The non-linear mixer LUT in
  `nes-apu` is calibrated against this ROM.

## Currently exercised by the harness

These ROMs are committed but **not yet wired into a dedicated
integration test**. They are primary inputs for future audio
regression work (see Track C of `/home/parobek/.claude/plans/`). For
the time being they serve as:

1. Reference recordings the user can run interactively via
   `cargo run --release -p nes-frontend -- tests/roms/audio-tests/<rom>`.
2. Fuzz / property-test seeds for the `nes-apu` and mapper-audio
   crates.

## License

Per upstream `readme.md`: freely redistributable and modifiable for any
purpose, credit appreciated. See [`UPSTREAM_README.md`](./UPSTREAM_README.md)
for the full upstream documentation.

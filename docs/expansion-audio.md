# Expansion Audio

**References:** the authoritative spec is [`apu-2a03.md` § Expansion-chip audio](apu-2a03.md); levels are tracked in [`accuracy-ledger.md`](accuracy-ledger.md). This page is a curated handbook entry point.

Several NES cartridge boards carry their **own** sound hardware that mixes into
the console's external-audio input alongside the 2A03. RustyNES synthesizes six
such expansion chips and sums each into the mix through the
`Mapper::mix_audio(&mut self) -> i16` hook (default `0` for boards with no audio
hardware). Each synth core lives in the **owning mapper crate**, not the 2A03
APU crate, because it is cartridge hardware.

## Supported chips

| Chip | Channels | Core | Clocking |
|---|---|---|---|
| VRC6 | 2 pulse + 1 saw | `Vrc6Pulse` ×2 + `Vrc6Saw` (`rustynes-mappers`) | every CPU cycle |
| VRC7 | 6 FM (OPLL) | `rustynes_apu::Opll` (emu2413-derived, **MIT**) | OPLL `calc()` every 36 CPU cycles (~49,716 Hz) |
| FDS | 1 wavetable + FM | `FdsAudio` (`fds.rs`) | wave/mod every 16 CPU cycles |
| MMC5 | 2 pulse + 7-bit PCM | `Mmc5Audio` (`mmc5.rs`) | pulse every other CPU cycle |
| Namco 163 | 1–8 time-multiplexed wavetable | `Namco163Audio` | round-robin every 15 CPU cycles |
| Sunsoft 5B | 3 tone + noise + envelope | `Sunsoft5BAudio` (FME-7) | every CPU cycle |

All cores are behind the default-on `mapper-audio` Cargo feature; with it off
(e.g. the `no_std` build) the register decoders still latch so save-state
round-trip is preserved, but `clock`/`mix` become silent no-op shims. The VRC7
OPLL core is deliberately the **MIT `emu2413`** lineage, not the license-
incompatible Nuked-OPLL.

## Relative levels (v2.1.6 "Expansion Audio")

Each chip's `mix_audio()` is scaled so its full-volume output sits at the
loudness the hardware and **Mesen2** (RustyNES's accuracy bar) produce relative
to the 2A03 pulse, measured by the bbbradsmith `db_*` decibel-comparison ROMs and
asserted by the `audio_expansion.rs` `level_db_*` oracle. VRC6 (≈1.506), MMC5
(≈1.0, "equivalent to the APU") and Namco 163 1-channel (≈6.02) are the pinned
corrections. Two levels are **honest documented gaps**: the Sunsoft 5B absolute
level (its log-DAC *shape* is hardware-exact, but a full-volume tone would
overflow the `i16` `mix_audio` contract — a wider mix path is deferred), and the
VRC7 FM absolute level (the FM synth is implemented and its instrument ROM is
verified canonical, but its patch/feedback-dependent pseudo-sine output is not
cleanly oracle-pinned, so `db_vrc7` stays a byte-exact snapshot regression
guard). See [`accuracy-ledger.md`](accuracy-ledger.md) § Expansion-audio levels.

Because a non-expansion mapper's `mix_audio()` returns `0`, the expansion mix is
a **separate additive term** — the level corrections leave the base 2A03 mix
byte-identical, so AccuracyCoin / blargg / nestest are unaffected.

## NSF playback

A classic `.nsf` may declare expansion audio in its `$07B` bitfield (bit 0 VRC6,
1 VRC7, 2 FDS, 3 MMC5, 4 N163, 5 5B). The NSF player does **not** reimplement any
synthesis: `NsfExpansion` (`nsf_expansion.rs`) owns instances of the *exact same*
cores and routes the NSF register windows into them, so an NSF VRC6 tune sounds
identical to a VRC6 cartridge. `NsfExpansion` is unreachable from any oracle
cartridge ROM, so it cannot perturb the existing audio suites.

## Frontend

The **Audio Mixer** tool panel (v2.1.6, `debugger::audio_mixer`) exposes
per-source balance sliders and per-channel scopes / VU meters, including the
on-cart expansion channel. See [`frontend.md`](frontend.md).

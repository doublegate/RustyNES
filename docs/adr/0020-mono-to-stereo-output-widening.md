# 20. Mono→stereo output widening for the frontend audio DSP (off-by-default, byte-identical bypass)

Date: 2026-06-19

## Status

Accepted (v1.7.0 "Forge", Workstream H3).

## Context

The NES 2A03 APU mixes its five channels (plus any cartridge-expansion audio)
to a single **mono** signal, and RustyNES preserves that faithfully: the
deterministic core produces one mono sample stream and hands it to the frontend
via `Nes::drain_audio_into`. The frontend already runs output-only stages on
that mono stream in the cpal producer/callback path — the master-gain multiply
(v1.0.0) and the optional graphic EQ (v1.1.0; the `EqStage` wrapper in
`audio.rs` driving the band-generic `Equalizer` in `eq.rs`) — none of which
touch the core synthesis, so they sit *outside* the determinism contract (the
AccuracyCoin audio oracle, save-state round-trip, TAS replay, netplay rollback).

Workstream H3 ("audio depth") adds stereo panning, a reverb, a headphone
crossfeed, an output-device picker, a 20-band EQ, and per-context volumes. Three
of those — panning, reverb, crossfeed — are inherently **stereo**, but the only
audio signal the frontend has is the pre-mixed mono master, written to the cpal
device today by **duplicating** the mono value to every output channel (the
`fill::<S>` callback).

Two hard constraints frame the design:

1. **The core stays byte-identical.** The chip stack, `rustynes-core`, and the
   test harness must not change; AccuracyCoin must hold 100% (139/139). True
   *per-APU-channel* panning would require the core to expose each channel's
   pre-mix samples as separate streams — a core change — which is off-limits
   here (it belongs to the v2.0 every-cycle rewrite, ADR 0002).
2. **Defaults must be bit-exact.** With every new control at its default
   (center pan, 0% reverb, 0 crossfeed) the audio the DAC receives must be the
   *same bits* as today's mono-duplicated output — not merely "inaudibly close".

The question this ADR settles: where the mono→stereo widening happens, how the
per-channel pan surface is honored without a core change, and how the
byte-identical-bypass guarantee is structured.

## Decision

Widen mono to stereo **in the cpal callback only**, in a new frontend module
`audio_dsp.rs::StereoStage`, leaving the lock-free SPSC ring strictly **mono**
(unchanged f32-per-slot layout). The stage is the audio analogue of `EqStage`:
output-only, owned by the real-time callback, and fed live params lock-free from
the Settings UI via a generation counter on the shared `QueueInner`.

- **Bypass is the identity duplicate, bit-for-bit.** `StereoStage::is_bypass()`
  is true when pan is center, reverb mix is 0, and crossfeed is 0. The callback
  checks it (and the mono-device case, `channels < 2`) *before* doing any DSP
  and takes the existing duplicate-to-every-channel path, so the default output
  is the same bits as before H3. A unit test asserts the per-sample bit-equality
  (`l.to_bits() == mono.to_bits()`), and the determinism gate
  (`cargo test --features test-roms`) confirms the audio oracle is unchanged.

- **Pan is constant-power, scaled so center == unity.** A center pan yields
  `(SQRT_1_2, SQRT_1_2)`, multiplied by `SQRT_2` so each channel carries the mono
  value unattenuated — center pan is mathematically the identity, which is why
  the bypass duplicate and an *engaged-but-centered* pan agree.

- **Per-channel pan collapses to a master image.** Because the frontend only has
  the pre-mixed mono master, the per-APU-channel pan array
  (`[audio] pan: [f32; 6]`) is applied as the **mean** of the channels' pans —
  one master pan over the mono master. The all-center default (the only one that
  must be byte-identical) is exactly the identity. The per-channel surface is
  kept in the config and UI as the forward-compatible shape: when the v2.0 core
  split exposes per-channel streams, the same config drives true per-channel
  panning with no schema change.

- **Reverb / crossfeed are summed into the stereo pair, default dry/off.** A
  classic 4-comb + 2-allpass Schroeder reverb (mono send, equal into L/R, wet =
  `reverb_mix`, decay = `reverb_room`) and a symmetric L/R crossfeed blend, both
  zero by default → no allocation, no effect, bypass true.

- **Live params are lock-free.** New atomics on `QueueInner` (pan, reverb mix/
  room, crossfeed + a `stereo_gen` counter) carry the settings across the
  winit→audio-thread boundary, exactly like the EQ generation; the callback
  rebuilds its stateful reverb only when the generation moves.

The remaining H3 pieces follow the same off-by-default discipline without
needing a new contract: the 20-band EQ extends the existing band-count-generic
`Equalizer`; per-context volume folds extra unity-default legs into the single
cpal consume gain; the device picker selects the cpal device at stream open and
falls back to the host default when the named device is absent.

## Consequences

- **Default output is provably byte-identical** to v1.6.0 (the bit-equality unit
  test + the unchanged AccuracyCoin 139/139 audio oracle), so the determinism
  contract and the shipped sound are untouched until the user moves a control.
- **No core change**, so the chip stack / `rustynes-core` / test harness and the
  `no_std` and wasm builds are unaffected (the device-picker + cpal code stays
  native-gated as it already was).
- **Per-channel panning is currently approximate** (a master image over the mono
  master), a documented honesty limitation. True per-channel panning is gated on
  the v2.0 every-cycle / core-split work (ADR 0002); the config/UI shape is
  already final, so that future change needs no migration.
- The mono ring is unchanged, so the lock-free SPSC invariants, the DRC
  resampler, and the start-gate/resync discipline all keep working as-is — the
  stereo stage is purely a post-pop transform in the callback.

# Display and audio

How the emulator renders to your screen and produces sound. This page is
worth a skim if you want to enable the NTSC filter, understand why the
window is letterboxed, or check that your audio device is doing what
you'd expect.

## Display

### Native resolution and scaling

The NES PPU emits a 256x240 RGBA framebuffer. The window opens at 3x
that resolution by default (768x720) and the framebuffer is upscaled
with **nearest-neighbour** filtering — no soft / bilinear blurring,
pixels stay pixels.

You can resize the window freely. The renderer letterboxes (or
pillarboxes) the NES image to keep the correct aspect ratio at any window
size; the bars on the sides or top are black. Press `F11` (or **View →
Fullscreen**) for borderless fullscreen, and `Esc` to leave it.

### NTSC filter

By default the output is "pure-pixel": each NES pixel maps to N
host pixels exactly. To opt into a composite-style look, set:

```toml
[graphics]
ntsc_filter = "composite"
```

What that does, end to end:

1. A 5-tap horizontal blur (`[0.10, 0.20, 0.40, 0.20, 0.10]` weights)
   runs over the framebuffer, emulating composite chroma bleed.
2. Every other vertical line is dimmed by 15%, producing visible
   scanlines.
3. Pixels with a strong horizontal luma gradient get a subtle red /
   blue chroma fringe (Blargg's well-known composite artifacting
   trick).

This inline `"composite"` look is a fast, always-available Blargg-style
approximation. For a heavier, more faithful composite signal, set
`ntsc_filter = "composite-rt"` to run a real-time NTSC encode/decode pass
instead.

Disable it again by setting `ntsc_filter = "off"` (the default).

The accepted `ntsc_filter` values are `"off"`, `"composite"`, `"rgb"`,
and `"composite-rt"`. `"composite"` and `"rgb"` both run the simplified
inline pass; `"composite-rt"` runs the real-time NTSC filter.

### Shaders and palettes

Beyond the built-in `ntsc_filter`, RustyNES ships a composable **shader
stack** with a CRT preset bank, plus a set of post-process filters —
NES_NTSC, CRT / scanline, LMP88959 NTSC/PAL, hqNx / xBRZ, and Bisqwit —
and an HD-pack loader for per-tile graphics replacement. You can also
load a custom 64-color palette from a `.pal` file. These are all
presentation-only and never affect emulation accuracy or determinism.

### Vsync and present mode

The frame **production** rate is decoupled from the present rate: the
emulator is paced by wall-clock time at the cartridge's nominal rate
(NTSC: 60.0988 Hz, PAL: 50.0070 Hz, Dendy: 50.0070 Hz), and the wgpu
surface re-presents the most recent frame at the monitor's refresh. So
on a 144 Hz monitor you'll see each NES frame ~2.4 times, but the
emulator still runs at real-hardware speed.

The `[graphics] present_mode` key in `config.toml` selects the swapchain
present mode: `"Mailbox"` (the default) lets the wall-clock pacer own
timing and avoids the vsync double-pacing beat, falling back to `"Fifo"`
automatically when the backend doesn't advertise Mailbox. `"Fifo"` forces
vsync. See [Configuration](./configuration.md#graphics).

### Aspect ratio

By default the displayed image is square-pixel (each NES pixel maps to N
host pixels). The real PPU pixel was non-square (~8:7 on NTSC), so the
truly-correct aspect stretches slightly horizontally. Enable **8:7 Pixel
Aspect** in **View → Settings… → Display** (or the View menu, or
`[ui] pixel_aspect_correction = true`) for the NES-native shape. It is off
by default so the shipped output stays pixel-exact unless you opt in.

### Overscan crop

On a real CRT the top and bottom 8 scanlines fell behind the bezel, so many
games leave junk or a status-bar seam there. Enable **View → Hide Overscan**
(or the Display settings tab, or `[graphics] hide_overscan = true`) to crop
those 8 top and 8 bottom scanlines; the remaining 256x224 image is rescaled
to fill the viewport. It is off by default, so the shipped output is the full
256x240 frame.

### Region detection

The cartridge region drives the frame rate:

| Region | Frame rate | Frame duration |
|--------|-----------|----------------|
| NTSC (Famicom JP, NES NA/AU) | 60.0988 Hz | ~16.639 ms |
| PAL (NES EU) | 50.0070 Hz | ~19.997 ms |
| Dendy (PAL famiclones) | 50.0070 Hz | ~19.997 ms |

Detection sources:

- **NES 2.0 byte 12 bits 0-1** when the header is NES 2.0 (the
  authoritative answer).
- **iNES 1.0 fallback**: NTSC, because iNES 1.0 has no reliable region
  byte.

If you have a PAL ROM that ships with a plain iNES 1.0 header, it will
play at NTSC frame rate — gameplay will run at about 120% real speed. You
can fix it without re-dumping: edit the cartridge header in the in-app
iNES / NES 2.0 header editor (under the debugger / Tools), or set
`"region": "PAL"` in the per-game `<rom>.json` override (see
[File locations](./file-locations.md)). Re-dumping or re-headering the
ROM with a correct NES 2.0 region byte also works.

The status bar shows the detected region and the live FPS (toggle the FPS
readout under **View → Show FPS**), which makes region misdetection
obvious — you'll see ~50 fps on a PAL ROM identified correctly, ~60 fps on
an NTSC ROM, and about 60 on a mis-identified PAL ROM (running too fast).

## Audio

### Output

Audio plays to your system's default output device. CPAL handles the
backend dispatch:

| OS | Backend |
|----|---------|
| Linux | ALSA (also PulseAudio / PipeWire through ALSA's compatibility layer) |
| macOS | CoreAudio |
| Windows | WASAPI |

The device's preferred sample format and channel count are honoured.
Mono NES audio is replicated across however many channels the device
opens (typically stereo).

### Volume and mute

**View → Settings… → Audio** has a master **Volume** slider (0–100%) and a
**Mute** checkbox, both applied live:

```toml
[audio]
volume = 1.0    # master output level, 0.0–1.0
muted = false   # mute all output
```

### Per-channel mute

The same Audio tab has six checkboxes — Pulse 1, Pulse 2, Triangle, Noise,
DMC, and Mapper Audio — all on by default. Unchecking one silences just that
APU channel:

```toml
[audio]
# bitmask: bit 0 Pulse 1, 1 Pulse 2, 2 Triangle, 3 Noise, 4 DMC, 5 Mapper Audio.
# A set bit = audible. 63 (0x3F) = all six on.
channel_mask = 63
```

This is a studio/debug overlay applied at playback only — it does **not**
affect emulation accuracy, so muting a channel leaves save-state and movie
determinism untouched.

### Sample rate

The APU emits via band-limited synthesis; a frontend resampler stage
then delivers samples to CPAL at the negotiated host rate.

The preferred rate is 44.1 kHz (`[audio] sample_rate = 44100` in
`config.toml`). If the device refuses 44.1 kHz, the audio engine is
rebuilt at whatever rate the device opens at. Set the config key to your
device's native rate if you want to bypass the negotiation:

```toml
[audio]
sample_rate = 48000
```

Common values are 44100, 48000, 96000. Any rate the device supports
works.

### Buffering and latency

The emulator hands samples to the CPAL callback through a lock-free
single-producer/single-consumer ring, and an allocation-free callback
drains it. A **dynamic-rate-control** stage — a 4-tap Hermite resampler
that micro-bends the playback ratio — holds the ring centred on the
configured `[audio] latency_ms` (default 60 ms), so the queue neither
drifts to an underrun nor builds unbounded latency over time.

```toml
[audio]
latency_ms = 60   # target buffer depth in ms
drc = true        # dynamic rate control; false = bit-exact passthrough
```

Lower `latency_ms` for tighter latency; raise it if a loaded system
causes underruns. Set `drc = false` for a bit-exact passthrough of the
APU's native output (no resampling). During a long stall (debugger
pause, save-state load, hibernation) a hard resync trims the buffer so
latency snaps back instead of accumulating.

### Emulation-speed presets

**Emulation → Speed** picks an emulation-speed preset — 25%, 50%, 75%, 100%,
150%, 200%, or 300% — and the `=` / `-` / `0` keys step up / step down / reset
to 100%. The speed is transient: the emulator always launches at 100% and the
choice is not persisted. The status bar shows the speed whenever it is not
100%.

Because the whole machine (including the APU) runs faster or slower, audio
**pitch-shifts** naturally — slow-mo and lower-pitched at 50%, faster and
higher at 200%. This is distinct from the hold-`Tab` fast-forward, which runs
unthrottled with audio muted.

### When audio doesn't start

If CPAL can't open the default device on startup, the emulator logs
`rustynes: audio disabled: <reason>` to stderr and continues
silently. The visuals are unaffected. See
[Troubleshooting](./troubleshooting.md) for common causes.

## See also

- [Configuration](./configuration.md) — `[graphics]` and `[audio]` reference
- [Compatibility](./compatibility.md) — region support and known timing gaps
- [Troubleshooting](./troubleshooting.md) — wrong fps, no audio, crackling

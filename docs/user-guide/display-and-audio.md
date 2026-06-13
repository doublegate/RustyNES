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

It's a deliberately simplified Blargg-style look — not a bit-exact port
of `nes_ntsc`. A full port is a follow-up release. The filter is
labelled "simple" in the source for that reason; expect it to look
*like* composite video, not *as* composite video.

Disable it again by setting `ntsc_filter = "off"` (the default).

The accepted values are `"off"`, `"composite"`, and `"rgb"`. `"rgb"` is
currently the same as `"composite"`; a distinct RGB-look path is
planned.

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
Aspect** in **View → Settings… → Video** (or the View menu, or
`[ui] pixel_aspect_correction = true`) for the NES-native shape. It is off
by default so the shipped output stays pixel-exact unless you opt in.

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
play at NTSC frame rate — gameplay will run at about 120% real speed.
The fix is to re-dump or re-header the ROM with an NES 2.0 header that
sets the region byte to 1 (PAL). A user-visible "force PAL" toggle is
on the post-v1.0 roadmap.

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

### Sample rate

The APU emits at the negotiated host rate **directly** via band-limited
synthesis. There is no software resampler in the pipeline.

The preferred rate is 44.1 kHz (`[audio] sample_rate = 44100` in
`config.toml`). If the device refuses 44.1 kHz, the APU is rebuilt at
whatever rate the device opens at. Set the config key to your
device's native rate if you want to bypass the negotiation:

```toml
[audio]
sample_rate = 48000
```

Common values are 44100, 48000, 96000. Any rate the device supports
works.

### Buffering and latency

Between the emulator and CPAL sits a bounded sample queue with a soft
cap of 16,384 samples (~370 ms at 44.1 kHz, ~340 ms at 48 kHz). Most of
the time the queue stays small — at 44.1 kHz the emulator pushes ~735
samples 60 times a second and CPAL drains in chunks of 256-1024
samples, so steady-state occupancy is well under 2000.

The queue's drop-oldest policy bounds latency during long stalls
(debugger pause, save-state load, system hibernation). If the emulator
briefly out-produces the device, the oldest samples drop instead of the
queue growing unbounded — you'll hear a brief click in the rare case
this triggers.

### When audio doesn't start

If CPAL can't open the default device on startup, the emulator logs
`rustynes: audio disabled: <reason>` to stderr and continues
silently. The visuals are unaffected. See
[Troubleshooting](./troubleshooting.md) for common causes.

## See also

- [Configuration](./configuration.md) — `[graphics]` and `[audio]` reference
- [Compatibility](./compatibility.md) — region support and known timing gaps
- [Troubleshooting](./troubleshooting.md) — wrong fps, no audio, crackling

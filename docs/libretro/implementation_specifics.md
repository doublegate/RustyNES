# RustyNES Libretro Core Implementation Specifics

The code-level contract between the libretro C ABI and `rustynes-core`, as
implemented in `crates/rustynes-libretro/src/lib.rs`. Where a claim here has a
test, the test is named; the tests are the spec.

## System Information (`retro_get_system_info`)

* **Library name and version:** `"RustyNES"` + `env!("CARGO_PKG_VERSION")`.
* **Extensions:** `nes|fds|unf|unif` — iNES / NES 2.0, Famicom Disk System,
  and UNIF (converted to NES 2.0 by `rustynes-mappers`). Must equal the
  `.info`'s `supported_extensions` (`libretro_info_audit`).
* **`need_fullpath = false`:** the frontend normally loads the game and passes
  its bytes.

## Environment Negotiation (`on_set_environment`)

* **Log interface:** `GET_LOG_INTERFACE`. Every message goes to the frontend's
  log (format fixed at `"%s"`), or to stderr when it has none.
* **Pixel format:** XRGB8888.
* **Input descriptors:** all eight buttons on all four ports, from a `'static`
  table terminated by a **null** description, as libretro.h specifies
  (`every_port_has_input_descriptors`). `rust-libretro`'s `input_descriptors!`
  macro terminates with `""` instead, which RetroArch reads past; it is not
  used.
* **Controller info:** ports 1-2 offer the NES controller or the Zapper;
  ports 3-4 the controller only (Four Score players, or a Vs. cabinet's second
  console).
* **Core option:** `rustynes_four_score` (`disabled|enabled`), declared with
  `SET_VARIABLES` and read in `on_options_changed`.
* **Disk control:** registered, for FDS side swapping.

## Game Loading (`retro_load_game`)

1. `GET_GAME_INFO_EXT` first, when the frontend answers with data: it carries
   the original extension even for an in-memory load.
2. Otherwise the standard `retro_game_info`: its `data`, or the file at its
   `path`. An EXT answer with no data falls through to this.
3. A Famicom Disk System image is recognised by a `.fds` extension **or** its
   signature (`FDS\x1A`, or a raw side's `\x01*NINTENDO-HVC*`) and loaded with
   `disksys.rom` from the system directory; everything else goes to
   `Emu::from_rom`.
4. A parse error returns `false` to the frontend.

## The Frame (`retro_run`)

### Input

`rust-libretro` polls the frontend before `on_run`; the core does not poll
again. Each pad is one `input_state` call (`RETRO_DEVICE_ID_JOYPAD_MASK`),
falling back to per-button reads on a frontend without bitmasks
(`input_is_polled_once_and_read_as_one_bitmask_per_pad`). Ports 1-2 are
players 1-2; with the Four Score option, ports 3-4 are players 3-4. A port set
to `RETRO_DEVICE_LIGHTGUN` drives the Zapper, and switching it back to a
controller unplugs the Zapper.

### Video

`run_frame()`, then the RGBA8 framebuffer is copied and its R and B bytes
swapped to XRGB8888 (`memcpy` + an in-place swap; two one-pass rewrites were
measured slower or no faster, `docs/performance.md` §v2.8.1). 256x240, pitch
1024 bytes; a Vs. `DualSystem` cabinet presents 512x240, pitch 2048.

### Audio

Mono `f32` samples are drained per frame, converted by `sample_to_i16` with
**full scale at `1.0`**, duplicated to stereo, and sent in one
`audio_sample_batch` call. The samples are bipolar and DC-blocked; the 2A03
spans about `-0.385..0.223` and expansion audio reaches further (Namco 163,
`+/-0.870`), so `1.0` is the scale at which nothing clips
(`expansion_audio_is_not_clipped`). The frontend's dynamic rate control
resamples. While RetroArch fast-forwards, the conversion is skipped.

## Save States (`retro_serialize` / `retro_unserialize`)

`retro_serialize_size` is the loaded state's size plus room for the largest
expansion device on both ports, because RetroArch reads it once and a Zapper
plugged in later grows the state. The unused tail is zero-filled, and the
save-state reader treats an all-zero tail as padding.

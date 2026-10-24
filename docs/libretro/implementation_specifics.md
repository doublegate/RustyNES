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
* **Emulation options (v3.6.0, `T-LIBRETRO-EMU-OPTIONS`):**
  `rustynes_cpu_overclock` (`1`-`4`, labelled x1 (stock) to x4) and
  `rustynes_remove_sprite_limit` (`disabled|enabled`), in the "Emulation"
  category: the desktop's v3.1.0 options (`Nes::set_cpu_overclock`,
  `Nes::set_sprite_limit_disabled`). Applied at the top of every frame,
  compare-then-set, to the console or both consoles of a cabinet, as the
  desktop applies them (`apply_console_options`), and recorded in every save
  state (see "Save States"). At their defaults nothing is written, and the
  frames equal a bare console's (`the_default_options_leave_the_output_byte_identical`).
* **Core options:** one table (`crates/rustynes-libretro/src/options.rs`),
  declared with `SET_CORE_OPTIONS_V2` (categories, sublabels, value labels)
  when `GET_CORE_OPTIONS_VERSION` reports 2 or more, else with
  `SET_VARIABLES` (v3.6.0); read by key in `on_options_changed` either way.
  `SET_CORE_OPTIONS_V2`'s return value says only whether the frontend shows
  categories (libretro.h), so `false` from it is not a refusal. A version-1
  frontend gets `SET_VARIABLES`, not v1: v1 adds no categories and every v1
  frontend accepts v0. Keys never change once shipped; the v3.5.0 key,
  `rustynes_four_score` (`disabled|enabled`), keeps its v0 string byte for
  byte (`core_options_v2_with_a_v0_fallback_keep_every_key`).
* **Disk control:** registered, for FDS side swapping.

## Game Loading (`retro_load_game`)

1. `GET_GAME_INFO_EXT` first, when the frontend answers with data: it carries
   the original extension even for an in-memory load.
2. Otherwise the standard `retro_game_info`: its `data`, or the file at its
   `path`. An EXT answer with no data falls through to this.
3. A Famicom Disk System image is recognised by a `.fds` extension **or** its
   signature (`FDS\x1A`, or a raw side's `\x01*NINTENDO-HVC*`) and loaded with
   `disksys.rom` from the system directory; everything else goes to
   `build_cartridge`, which applies the game database's load-time corrections
   around `Emu::from_rom` (v2.9.8): `rustynes_gamedb::correct_rom` rewrites the
   header (mapper, submapper, region) before the parse, and
   `rustynes_gamedb::correct_console` applies a hardwired-mirroring override to
   the console, or to both consoles of a Vs. `DualSystem` cabinet. These are
   the functions every platform calls (`docs/frontend.md`, "One correction path
   for every platform"); until v2.9.8 the core applied none of them. Only the
   vendored table is consulted: the desktop's editable overlay is not
   configured here, so no core option is needed.
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
1024 bytes; a Vs. `DualSystem` cabinet presents 512x240, pitch 2048, composed
straight into the reused buffer (`compose_dual`, which rewrites every byte, so
it is not zero-filled first; v2.9.2 audit AUD-19).

### Audio

Mono `f32` samples are drained per frame, converted by `sample_to_i16` with
**full scale at `1.0`**, duplicated to stereo, and sent in one
`audio_sample_batch` call. The samples are bipolar and DC-blocked; the 2A03
spans about `-0.385..0.223` and expansion audio reaches further (Namco 163,
`+/-0.870`), so `1.0` is the scale at which nothing clips
(`expansion_audio_is_not_clipped`). The frontend's dynamic rate control
resamples. While RetroArch fast-forwards, the conversion is skipped.

## Save States (`retro_serialize` / `retro_unserialize`)

For a single console, `retro_serialize_size` is the loaded state's size plus
room for the largest expansion device on both ports, because RetroArch reads it
once and a Zapper plugged in later grows the state. A Vs. `DualSystem` cabinet
gets no headroom: it never attaches an expansion device, so its size is the
snapshot's exact length and cannot grow after load. The unused tail is zero-filled, and the
save-state reader treats an all-zero tail as padding. The snapshot that
measures the size at load is taken into the buffer `retro_serialize` reuses,
with that headroom reserved for a single console, so no serialize reallocates it (v2.9.2 audit
AUD-17; `no_serialize_reallocates_the_buffer_sized_at_load`).

**The options record (v3.6.0, `T-LIBRETRO-EMU-OPTIONS`).** The last 16 bytes
of every state the core writes record the options that change what the
console does: `RNLROPTS`, a layout version (1), a flags byte (bit 0 Four
Score, bit 1 sprite limit removed), the CPU overclock (1-4), the Vs. DIP
switches forced and their positions, three zero bytes
(`options::OptionValues::trailer`). `retro_serialize_size` includes them. The
desktop records the same options in a movie's header and a netplay
handshake (`HardwareOptions`, `config_digest`); a libretro frontend replays a
movie from a state and starts a netplay peer from the host's state, so the
state is where they have to be. `retro_unserialize` applies a state's record
with it, replacing the menu's values until the frontend next reports an
option change, and says so in the log when they differ. The overclock goes in
before the restore, because the state holds the position inside the
overclocked cycle pattern and the core keeps a restored position only under a
multiplier that could have produced it; a refused restore puts the
multiplier and the machine back. A state without a record (every state from
before v3.6.0: its last 16 bytes are zero padding, or a cabinet's section
data) loads as before and leaves the options alone; a record with a value no
menu offers (an overclock of 9, a DIP position outside its mask, a non-zero
reserved byte) refuses the state. Tests: `a_state_carries_its_emulation_options`,
`states_without_a_record_load_and_bad_records_refuse`,
`tests::a_refused_restore_under_a_new_multiplier_puts_the_console_back`,
`tests::a_cabinet_restores_under_the_state_multiplier_on_both_consoles`.

**Found, not fixed: a cabinet's state under the overclock.** A Vs.
`DualSystem` state saved and restored at x2, with no option change at all,
runs on to a state ten frames later that differs from the straight run's in
64 bytes: the two consoles' OAM-decay ages (32 `u64` each, encoded as
`dot_counter / 3 - timestamp`), each one larger after the restore. At x1 the
round trip is exact, and a single console's (slim) state is exact at x2. The
decay model is off by default, so the ages are inert and the game computes
the same thing, but RetroArch's netplay compares state CRCs and would report
a desync. It is a core issue, outside this wrapper; the expected-fail probe
`a_cabinet_state_round_trips_byte_for_byte_under_the_overclock` (`#[ignore]`)
pins it.

`retro_unserialize` refuses a state, returns `false`, and leaves the running
game exactly as it was (re-audit NL-01;
`a_rejected_unserialize_leaves_the_machine_as_it_was`) when:

* it is from RustyNES v2.9.7 or earlier. v2.9.8 moved the `.rns` container to
  format 3 and stopped reading older formats (ADR 0042), so those states
  cannot be loaded or converted; play to the same point, or load an in-game
  save, and save a new state;
* its sections do not fit the loaded board (a state from a game on a
  different mapper, a different RAM size, or a single console's state on a
  Vs. `DualSystem` cabinet and the reverse);
* it is truncated or damaged.

A state from a different game on the same board shape is not refused; the
container does not record which game wrote it. Since v2.9.9 every refusal
writes its reason to the frontend's log at warning level, naming the format
for an old state (re-audit NL-11; `a_refused_unserialize_logs_the_reason`);
before, the frontend could show only its own generic failure.

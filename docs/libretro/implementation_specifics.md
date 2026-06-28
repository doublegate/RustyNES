# RustyNES Libretro Core Implementation Specifics

This document defines the strict, code-level integration parameters governing geometry negotiation, execution loops, and IO translation between the Libretro C ABI and `rustynes-core`.

## System Initialization and Environment Negotiation

Before execution, RetroArch extracts core metadata via `retro_get_system_info`.

* **Library Name & Version:** `"RustyNES"` + `env!("CARGO_PKG_VERSION")`.
* **VFS Offload:** `need_fullpath = false` ensures RetroArch reads the ROM into a RAM buffer (`*const c_void`), passing it directly to `rustynes-libretro`, completely bypassing the need for `std::fs` operations on the core side.

### Environment Handshakes (`on_set_environment`)

The core must configure specific frontend states during initialization:

1. **32-Bit XRGB8888 Pixel Format:** `rustynes-core` produces a 256x240 RGBA8 buffer. The environment hook `RETRO_ENVIRONMENT_SET_PIXEL_FORMAT` must request `XRGB8888` (which is functionally memory-compatible with RGBA8, ignoring the alpha channel). If the frontend forces `0RGB1555` (legacy 15-bit), the wrapper must instantiate a dynamic color-space conversion loop during `on_run`—though `XRGB8888` is ubiquitous today.
2. **Input Descriptors:** `RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS` maps underlying Libretro `JOYPAD` constants to human-readable strings (e.g., "NES A Button") to populate the RetroArch control remapping menu.

## Game Loading (`retro_load_game`)

The core receives `data: &[u8]` from the frontend.

* Call `rustynes_core::Nes::from_rom(data)`.
* If a `RomError` is encountered (e.g., invalid iNES header), the FFI wrapper must catch it and return `false` to the frontend, preventing a hard crash.

## Synchronous Execution Loop (`on_run`)

The `retro_run` hook fires exactly once per video frame. All operations inside this loop must be zero-allocation (no `String` creation, no dynamic `Vec` allocations) to prevent GC pauses or heap fragmentation micro-stutters.

### Input Polling and Bitmasking

1. Invoke `retro_input_poll()` to flush host hardware USB/Bluetooth queues.
2. Query `retro_input_state()` for Port 1 and Port 2.
3. Map the booleans into `rustynes_core::Buttons`:

   ```rust
   let mut btns = rustynes_core::Buttons::empty();
   if ctx.get_joypad_bit(0, JoypadButton::A) { btns.insert(Buttons::A); }
   if ctx.get_joypad_bit(0, JoypadButton::B) { btns.insert(Buttons::B); }
   // ... map Select, Start, Up, Down, Left, Right ...
   nes.set_controller_state(0, btns); // Adjust per actual Nes API for Joypads
   ```

### Video Rendering & Geometry

Invoke `nes.run_frame()`, which yields an `&[u8]`.

* **Geometry Calculation:** Standard NTSC NES output is `width: 256`, `height: 240`.
* **Pitch Alignment:** Libretro defines pitch as the exact byte-width of one scanline. For 32-bit `XRGB8888`, pitch is `256 * 4 = 1024` bytes.
* **Callback Dispatch:** Pass the buffer pointer, width, height, and pitch directly to `retro_video_refresh_callback`.
* **Frame Duplication Optimization:** During fast-forward sequences where the emulator state may not visually change, passing a `NULL` pointer instructs RetroArch to duplicate the previous frame, saving massive host GPU bandwidth.

### Audio Synchronization & Batching

RustyNES outputs `f32` audio samples normalized to `[0.0, ~1.0]` (or `[-1.0, 1.0]`).
Libretro requires interleaved stereo `i16` delivered in a single batch (avoiding mutex starvation associated with single-sample callbacks).

1. `let f32_samples = nes.drain_audio();`
2. Clear the persistent `self.audio_buffer` (capacity pre-allocated to ~2000 i16s).
3. **Conversion Math:**
   For a normalized `[0.0, 1.0]` float, map it to `[-32768, 32767]`:

   ```rust
   for &sample in &f32_samples {
       let scaled = (sample * 65535.0 - 32768.0).clamp(-32768.0, 32767.0) as i16;
       self.audio_buffer.push(scaled); // Left channel (mono duplicate)
       self.audio_buffer.push(scaled); // Right channel (mono duplicate)
   }
   ```

4. Dispatch the slice via `retro_set_audio_sample_batch`. The frontend's dynamic resampler handles fractional timing slips automatically.

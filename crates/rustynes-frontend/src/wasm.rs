//! v1.3.0 Sprint 1.3 — wasm32 browser emulator (canvas-2D MVP).
//!
//! This module is gated `#[cfg(target_arch = "wasm32")]`. It runs the
//! `rustynes-core` emulator in the browser, rendering the PPU framebuffer
//! to a `<canvas>` via the 2D `ImageData` path and driving the run
//! loop with `requestAnimationFrame`. ROM loading is via an
//! `<input type="file">`; keyboard input maps to player 1.
//!
//! ## Why canvas-2D and not winit/wgpu (yet)
//!
//! The native frontend uses winit + wgpu + cpal + egui. Porting that
//! full stack to wasm32 requires gating `gilrs` / `directories` /
//! `std::fs` and bumping `cpal 0.15 → 0.17` for the Web Audio
//! backend — multi-session work. This module takes the pragmatic
//! path: the PPU framebuffer is already RGBA8 256x240, which is
//! byte-identical to the canvas `ImageData` format, so a direct
//! `put_image_data` blit gets a WORKING browser emulator NOW. The
//! winit/wgpu unification (so the egui debugger overlay + NTSC
//! filter work on web too) is a follow-up sprint (1.4). Audio +
//! `IndexedDB` save state are also follow-ups.
//!
//! See `docs/audit/v1.3-sprint-1.3-wasm-canvas-mvp-2026-05-24.md`.

use std::cell::RefCell;
use std::rc::Rc;

use rustynes_core::{Buttons, Nes};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{
    CanvasRenderingContext2d, Event, FileReader, HtmlCanvasElement, HtmlInputElement, ImageData,
    KeyboardEvent,
};

// v1.3.0 Sprint 1.4c — Web Audio output moved to the shared
// `crate::wasm_audio` module (used by both this canvas embed path and
// the unified winit path).
use crate::wasm_audio;
// v1.4.0 Workstream E1 — reuse the target-agnostic movie record/play
// state machine (the SAME one the native + winit-wasm paths use) so the
// canvas embed records/replays the deterministic `.rnm` format byte-for-byte.
use crate::movie_ui::MovieUi;

const NES_W: u32 = 256;
const NES_H: u32 = 240;

/// Default active save-state slot for the canvas embed (slots are
/// 0-based internally, shown 1-based). v1.4.0 E2 keys the `IndexedDB`
/// store by the ROM SHA-256 and the slot, so multiple slots are supported;
/// the canvas embed's F1/F4 hotkeys use slot 0 (the winit path's
/// Save-States manager exposes the full slot grid).
const CANVAS_SLOT: u8 = 0;

/// Shared emulator state. Web-sys event closures are `'static`, so
/// the `Nes` and the current button state live behind `Rc<RefCell>`.
struct Emu {
    nes: Option<Nes>,
    buttons: Buttons,
    /// v1.4.0 E1 — TAS movie record/play state. Idle until the user
    /// presses F6 (record) / F7 (play) / F8 (branch).
    movie: MovieUi,
}

thread_local! {
    static EMU: Rc<RefCell<Emu>> = Rc::new(RefCell::new(Emu {
        nes: None,
        buttons: Buttons::empty(),
        movie: MovieUi::default(),
    }));
}

/// Map a `KeyboardEvent::code()` string to a player-1 NES button.
///
/// Mirrors the native default keymap (`docs/frontend.md`): arrows =
/// D-pad, Z = A, X = B, Enter = Start, Right Shift = Select.
fn keycode_to_button(code: &str) -> Option<Buttons> {
    Some(match code {
        "ArrowUp" => Buttons::UP,
        "ArrowDown" => Buttons::DOWN,
        "ArrowLeft" => Buttons::LEFT,
        "ArrowRight" => Buttons::RIGHT,
        "KeyZ" => Buttons::A,
        "KeyX" => Buttons::B,
        "Enter" => Buttons::START,
        "ShiftRight" => Buttons::SELECT,
        _ => return None,
    })
}

/// Entry point invoked from `index.html` after the `.wasm` artifact
/// loads (via `#[wasm_bindgen(start)]`).
///
/// # Errors
///
/// Returns a `JsValue` if the DOM elements can't be located or the
/// canvas 2D context can't be acquired.
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    log("RustyNES wasm32 — v1.3.0 Sprint 1.3 canvas-2D MVP boot");

    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;

    // Locate (or fail loudly on) the canvas + ROM input the host HTML
    // provides.
    let canvas: HtmlCanvasElement = document
        .get_element_by_id("nes-canvas")
        .ok_or("missing <canvas id=\"nes-canvas\">")?
        .dyn_into()?;
    canvas.set_width(NES_W);
    canvas.set_height(NES_H);

    let rom_input: HtmlInputElement = document
        .get_element_by_id("rom-input")
        .ok_or("missing <input id=\"rom-input\">")?
        .dyn_into()?;

    install_rom_loader(&rom_input);
    install_keyboard_handlers(&document);
    // v1.4.0 E1 — wire the hidden `.rnm` movie upload picker (created if the
    // host page doesn't provide one). F7 `.click()`s it from its gesture
    // handler; the selected bytes are deserialized + replayed.
    install_movie_loader(&document);
    start_raf_loop(&canvas)?;

    log("RustyNES wasm32 — armed. Load a .nes ROM to begin.");
    Ok(())
}

/// Wire the `<input type="file">` change handler: on ROM selection,
/// read the bytes via `FileReader` and instantiate the `Nes`.
fn install_rom_loader(rom_input: &HtmlInputElement) {
    let on_change = Closure::<dyn FnMut(Event)>::new(move |ev: Event| {
        let Some(input) = ev
            .target()
            .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
        else {
            return;
        };
        let Some(files) = input.files() else { return };
        let Some(file) = files.get(0) else { return };

        let Ok(reader) = FileReader::new() else {
            return;
        };
        let reader_clone = reader.clone();
        let on_load = Closure::<dyn FnMut()>::new(move || {
            let Ok(buffer) = reader_clone.result() else {
                return;
            };
            let array = js_sys::Uint8Array::new(&buffer);
            let bytes = array.to_vec();
            // The file-pick is a user gesture, so it's safe to create
            // the AudioContext here (the browser autoplay policy
            // requires a gesture). Create the Nes at the audio
            // context's native sample rate so the APU output needs no
            // resampling before hitting the Web Audio graph.
            let sample_rate = wasm_audio::ensure_audio();
            let nes_result = sample_rate.map_or_else(
                || Nes::from_rom(&bytes),
                |sr| Nes::from_rom_with_sample_rate(&bytes, sr),
            );
            match nes_result {
                Ok(nes) => {
                    EMU.with(|emu| emu.borrow_mut().nes = Some(nes));
                    wasm_audio::clear_ring();
                    log(&format!("ROM loaded ({} bytes). Running.", bytes.len()));
                }
                Err(e) => log(&format!("ROM parse error: {e:?}")),
            }
        });
        reader.set_onload(Some(on_load.as_ref().unchecked_ref()));
        on_load.forget();
        let _ = reader.read_as_array_buffer(&file);
    });
    rom_input.set_onchange(Some(on_change.as_ref().unchecked_ref()));
    on_change.forget();
}

/// Wire keydown/keyup listeners that OR/clear player-1 buttons and
/// handle the F1 (save state) / F4 (load state) hotkeys (mirrors the
/// native keymap).
fn install_keyboard_handlers(document: &web_sys::Document) {
    let on_keydown = Closure::<dyn FnMut(KeyboardEvent)>::new(move |ev: KeyboardEvent| {
        match ev.code().as_str() {
            "F1" => {
                ev.prevent_default();
                save_state();
                return;
            }
            "F4" => {
                ev.prevent_default();
                load_state();
                return;
            }
            // v1.4.0 E1 — TAS movie hotkeys (mirror the native F6/F7/F8
            // keymap). These run inside the keydown gesture, so the F7
            // file-picker `.click()` satisfies the browser's user-gesture
            // requirement.
            "F6" => {
                ev.prevent_default();
                movie_record_toggle();
                return;
            }
            "F7" => {
                ev.prevent_default();
                movie_play_toggle();
                return;
            }
            "F8" => {
                ev.prevent_default();
                movie_branch();
                return;
            }
            _ => {}
        }
        if let Some(btn) = keycode_to_button(&ev.code()) {
            ev.prevent_default();
            EMU.with(|emu| emu.borrow_mut().buttons.insert(btn));
        }
    });
    document.set_onkeydown(Some(on_keydown.as_ref().unchecked_ref()));
    on_keydown.forget();

    let on_keyup = Closure::<dyn FnMut(KeyboardEvent)>::new(move |ev: KeyboardEvent| {
        if let Some(btn) = keycode_to_button(&ev.code()) {
            ev.prevent_default();
            EMU.with(|emu| emu.borrow_mut().buttons.remove(btn));
        }
    });
    document.set_onkeyup(Some(on_keyup.as_ref().unchecked_ref()));
    on_keyup.forget();
}

/// Start the `requestAnimationFrame` run loop. Each tick: if a ROM is
/// loaded, push the current buttons, run one NES frame, and blit the
/// RGBA8 framebuffer to the canvas via `put_image_data`.
///
/// `requestAnimationFrame` runs at the display refresh rate (~60 Hz),
/// which matches NTSC closely enough for an MVP. Sprint 1.4 will add
/// proper frame pacing decoupled from the display rate.
fn start_raf_loop(canvas: &HtmlCanvasElement) -> Result<(), JsValue> {
    let ctx: CanvasRenderingContext2d = canvas
        .get_context("2d")?
        .ok_or("no 2d context")?
        .dyn_into()?;

    // The classic wasm-bindgen rAF self-reschedule pattern: an
    // `Rc<RefCell<Option<Closure>>>` that holds itself so it can
    // re-request on every frame.
    let f = Rc::new(RefCell::new(None::<Closure<dyn FnMut()>>));
    let g = f.clone();

    *g.borrow_mut() = Some(Closure::<dyn FnMut()>::new(move || {
        EMU.with(|emu| {
            let mut emu = emu.borrow_mut();
            // Reborrow as `&mut Emu` so the movie + nes fields can be borrowed
            // disjointly below (a `RefMut` deref alone won't split-borrow).
            let emu = &mut *emu;
            // v1.2.0 Workstream F1/F2 — fold the on-screen touch overlay into
            // this frame's input at the SAME point the keyboard mask is applied
            // (the canvas embed's single latch site, just before `run_frame`).
            // The touch buttons OR into the routed port; the Power Pad mat mask
            // feeds `set_power_pad` (which self-attaches the mat on port 1) when
            // the touch UI selected the Power Pad. Nothing-touched =
            // byte-identical to the keyboard-only path.
            let touch_buttons = crate::wasm_touch::touch_buttons();
            let touch_port = crate::wasm_touch::touch_target_port();
            let power_pad_active = crate::wasm_touch::touch_power_pad_active();
            let power_pad = crate::wasm_touch::touch_power_pad();
            let buttons = emu.buttons;
            if let Some(nes) = emu.nes.as_mut() {
                // Player 1 is the keyboard-mapped port; the touch overlay can
                // route its mask to any of ports 0..=3.
                if touch_port == 0 {
                    nes.set_buttons(0, buttons | touch_buttons);
                } else {
                    nes.set_buttons(0, buttons);
                    // Ports 1..3 get only the touch contribution (the canvas
                    // embed maps the keyboard solely to player 1).
                    nes.set_buttons(touch_port, touch_buttons);
                }
                if power_pad_active {
                    nes.set_power_pad(1, power_pad);
                }
                // v1.4.0 E1 — fold in the TAS movie hook at the SAME
                // single-latch site (after `set_buttons`, before `run_frame`),
                // exactly as the winit/native produce path does:
                // - recording: capture the just-latched input for this frame;
                // - playing: override the latched input with the movie's
                //   recorded input; `false` => the movie is exhausted, so stop
                //   playback and hand control back to live input.
                if !emu.movie.before_frame(nes) {
                    emu.movie.stop_playback();
                    log("movie playback finished");
                }
                nes.run_frame();
                // The framebuffer is RGBA8 256x240 — byte-identical
                // to the canvas ImageData layout. A clamped-array
                // view + put_image_data is the cheapest blit.
                let fb = nes.framebuffer();
                if let Ok(image) = ImageData::new_with_u8_clamped_array_and_sh(
                    wasm_bindgen::Clamped(fb),
                    NES_W,
                    NES_H,
                ) {
                    let _ = ctx.put_image_data(&image, 0, 0);
                }
                // Drain this frame's APU samples into the shared
                // Web Audio ring (the ScriptProcessorNode consumes
                // from the front; excess is dropped on overrun).
                wasm_audio::push_samples(&nes.drain_audio());
            }
        });
        request_animation_frame(f.borrow().as_ref().unwrap());
    }));

    request_animation_frame(g.borrow().as_ref().unwrap());
    Ok(())
}

/// `window.requestAnimationFrame(cb)` helper.
fn request_animation_frame(f: &Closure<dyn FnMut()>) {
    if let Some(win) = web_sys::window() {
        let _ = win.request_animation_frame(f.as_ref().unchecked_ref());
    }
}

/// F1 — serialize the running `Nes` and persist it to `IndexedDB`
/// (v1.4.0 E2), keyed by the ROM SHA-256 and slot. The snapshot is taken
/// synchronously under the `RefCell` borrow; the async IDB write is then
/// driven on the microtask queue so the borrow is released first.
fn save_state() {
    let Some((sha, blob)) = EMU.with(|emu| {
        let emu = emu.borrow();
        emu.nes
            .as_ref()
            .map(|nes| (*nes.rom_sha256(), nes.snapshot()))
    }) else {
        log("save state: no ROM loaded");
        return;
    };
    wasm_bindgen_futures::spawn_local(async move {
        crate::wasm_idb::put_state(sha, CANVAS_SLOT, blob).await;
    });
}

/// F4 — read the slot's blob back from `IndexedDB` (or migrate it from
/// `localStorage`) and restore the `Nes`. The async read runs on a
/// `spawn_local` task that re-borrows `EMU` only AFTER the read resolves,
/// guarding against a mid-read ROM swap by re-checking the SHA.
fn load_state() {
    let Some(sha) = EMU.with(|emu| emu.borrow().nes.as_ref().map(|n| *n.rom_sha256())) else {
        log("load state: no ROM loaded");
        return;
    };
    wasm_bindgen_futures::spawn_local(async move {
        let Some(blob) = crate::wasm_idb::get_state(sha, CANVAS_SLOT).await else {
            log("load state: no saved state");
            return;
        };
        EMU.with(|emu| {
            let mut emu = emu.borrow_mut();
            let Some(nes) = emu.nes.as_mut() else {
                return;
            };
            if *nes.rom_sha256() != sha {
                log("load state: ROM changed during load — skipped");
                return;
            }
            match nes.restore(&blob) {
                Ok(()) => log("state loaded"),
                Err(e) => log(&format!("load state: restore failed: {e:?}")),
            }
        });
    });
}

/// F6 — toggle TAS movie recording (v1.4.0 E1). **Start**: power-cycle the
/// running `Nes` and record from that fresh power-on. **Stop**: finish the
/// movie, serialize the `.rnm`, and trigger a browser Blob download (the
/// canvas embed has no `rfd` dialog — same path the winit wasm frontend uses).
fn movie_record_toggle() {
    EMU.with(|emu| {
        let mut emu = emu.borrow_mut();
        let emu = &mut *emu;
        if emu.movie.is_recording() {
            let Some(movie) = emu.movie.finish_recording() else {
                return;
            };
            let bytes = movie.serialize();
            crate::wasm_io::download_bytes("rustynes-movie.rnm", &bytes);
            log(&format!(
                "movie finished ({} frames, {} bytes) — download triggered",
                movie.len(),
                bytes.len()
            ));
        } else {
            let Some(nes) = emu.nes.as_mut() else {
                log("movie record: no ROM loaded");
                return;
            };
            emu.movie.start_recording_power_on(nes);
            log("movie recording started (power-on)");
        }
    });
}

/// F7 — toggle TAS movie playback (v1.4.0 E1). **Stop**: end playback and
/// return to live input. **Start**: open the hidden `.rnm` file picker (its
/// `change` handler deserializes + replays). The `.click()` runs inside this
/// keydown gesture, satisfying the browser file-picker policy.
fn movie_play_toggle() {
    let playing = EMU.with(|emu| emu.borrow().movie.is_playing());
    if playing {
        EMU.with(|emu| emu.borrow_mut().movie.stop_playback());
        log("movie playback stopped");
        return;
    }
    crate::wasm_io::click_file_input("rnm-input");
}

/// F8 — branch the current state into a new recording (v1.4.0 E1).
fn movie_branch() {
    EMU.with(|emu| {
        let mut emu = emu.borrow_mut();
        let emu = &mut *emu;
        let Some(nes) = emu.nes.as_ref() else {
            log("movie branch: no ROM loaded");
            return;
        };
        emu.movie.start_recording_branch(nes);
        log("movie branch — recording from current state");
    });
}

/// v1.4.0 E1 — create (if absent) the hidden `<input id="rnm-input"
/// type="file" accept=".rnm">` and wire its change handler to deserialize the
/// uploaded movie, seek the running `Nes` to the movie's start point, and
/// begin playback. Mirrors the winit path's `install_movie_loader`, but the
/// canvas embed drives `EMU` directly (no winit event loop).
fn install_movie_loader(document: &web_sys::Document) {
    let input: HtmlInputElement = if let Some(existing) = document
        .get_element_by_id("rnm-input")
        .and_then(|el| el.dyn_into::<HtmlInputElement>().ok())
    {
        existing
    } else {
        let Some(el) = document
            .create_element("input")
            .ok()
            .and_then(|el| el.dyn_into::<HtmlInputElement>().ok())
        else {
            log("movie loader: could not create <input>");
            return;
        };
        el.set_id("rnm-input");
        el.set_type("file");
        el.set_accept(".rnm");
        let _ = el.set_attribute("style", "display:none");
        if let Some(body) = document.body() {
            let _ = body.append_child(&el);
        }
        el
    };

    let on_change = Closure::<dyn FnMut(Event)>::new(move |ev: Event| {
        let Some(input) = ev
            .target()
            .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
        else {
            return;
        };
        let Some(files) = input.files() else { return };
        let Some(file) = files.get(0) else { return };
        let Ok(reader) = FileReader::new() else {
            return;
        };
        let reader_clone = reader.clone();
        let on_load = Closure::<dyn FnMut()>::new(move || {
            let Ok(buffer) = reader_clone.result() else {
                return;
            };
            let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
            start_movie_from_bytes(&bytes);
        });
        reader.set_onload(Some(on_load.as_ref().unchecked_ref()));
        on_load.forget();
        let _ = reader.read_as_array_buffer(&file);
    });
    input.set_onchange(Some(on_change.as_ref().unchecked_ref()));
    on_change.forget();
}

/// Deserialize uploaded `.rnm` bytes, seek the running `Nes` to the movie's
/// start point, and begin playback (v1.4.0 E1).
fn start_movie_from_bytes(bytes: &[u8]) {
    let movie = match rustynes_core::Movie::deserialize(bytes) {
        Ok(m) => m,
        Err(e) => {
            log(&format!("movie parse failed: {e:?}"));
            return;
        }
    };
    EMU.with(|emu| {
        let mut emu = emu.borrow_mut();
        let emu = &mut *emu;
        let Some(nes) = emu.nes.as_mut() else {
            log("movie play: no ROM loaded");
            return;
        };
        if let Err(e) = movie.seek_to_start(nes) {
            log(&format!("movie seek failed (wrong ROM?): {e:?}"));
            return;
        }
        let total = movie.len();
        emu.movie.start_playback(movie);
        log(&format!("movie playback started ({total} frames)"));
    });
}

/// `console.log` shim.
fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

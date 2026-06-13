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
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{
    CanvasRenderingContext2d, Event, FileReader, HtmlCanvasElement, HtmlInputElement, ImageData,
    KeyboardEvent,
};

// v1.3.0 Sprint 1.4c — Web Audio output moved to the shared
// `crate::wasm_audio` module (used by both this canvas embed path and
// the unified winit path).
use crate::wasm_audio;
// v1.6.0 Sprint 4 — base64 codec + localStorage accessor moved to the
// shared `crate::wasm_io` module (used by both wasm frontends).
use crate::wasm_io::{base64_decode, base64_encode, local_storage};

const NES_W: u32 = 256;
const NES_H: u32 = 240;

/// `localStorage` key for the single save-state slot. v1.3.0 Sprint
/// 1.4 ships a single slot keyed by a fixed string; multi-slot +
/// per-ROM keying is a follow-up (would key by the ROM SHA-256 like
/// the native `save_state` module does).
const SAVE_STATE_KEY: &str = "rustynes-savestate-slot0";

/// Shared emulator state. Web-sys event closures are `'static`, so
/// the `Nes` and the current button state live behind `Rc<RefCell>`.
struct Emu {
    nes: Option<Nes>,
    buttons: Buttons,
}

thread_local! {
    static EMU: Rc<RefCell<Emu>> = Rc::new(RefCell::new(Emu {
        nes: None,
        buttons: Buttons::empty(),
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
            let buttons = emu.buttons;
            if let Some(nes) = emu.nes.as_mut() {
                nes.set_buttons(0, buttons);
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

/// F1 — serialize the running `Nes` and stash it in `localStorage`
/// (base64 of the raw snapshot bytes). Single slot for v1.3.0.
fn save_state() {
    EMU.with(|emu| {
        let emu = emu.borrow();
        let Some(nes) = emu.nes.as_ref() else {
            log("save state: no ROM loaded");
            return;
        };
        let blob = nes.snapshot();
        let b64 = base64_encode(&blob);
        if let Some(storage) = local_storage() {
            match storage.set_item(SAVE_STATE_KEY, &b64) {
                Ok(()) => log(&format!("state saved ({} bytes)", blob.len())),
                Err(_) => log("save state: localStorage write failed (quota?)"),
            }
        }
    });
}

/// F4 — restore the `Nes` from the `localStorage` slot.
fn load_state() {
    let Some(storage) = local_storage() else {
        return;
    };
    let Ok(Some(b64)) = storage.get_item(SAVE_STATE_KEY) else {
        log("load state: no saved state");
        return;
    };
    let Some(blob) = base64_decode(&b64) else {
        log("load state: corrupt save (base64 decode failed)");
        return;
    };
    EMU.with(|emu| {
        let mut emu = emu.borrow_mut();
        let Some(nes) = emu.nes.as_mut() else {
            log("load state: no ROM loaded");
            return;
        };
        match nes.restore(&blob) {
            Ok(()) => log("state loaded"),
            Err(e) => log(&format!("load state: restore failed: {e:?}")),
        }
    });
}

/// `console.log` shim.
fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

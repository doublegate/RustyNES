//! v1.3.0 Sprint 1.4 — unified winit + wgpu + egui wasm32 frontend.
//!
//! This is the DEFAULT wasm32 frontend (`wasm-winit` feature). Unlike
//! the lightweight canvas-2D embed mode (`wasm-canvas`, in `wasm.rs`),
//! this routes the browser through the SAME winit `App`
//! (`ApplicationHandler<AppEvent>`) as the native desktop binary — so
//! the wgpu render pipeline, the egui debugger overlay, and the NTSC
//! filter all work on the web exactly as they do natively.
//!
//! ## Lifecycle
//!
//! 1. [`start`] (`#[wasm_bindgen(start)]`) fires when the `.wasm`
//!    loads. It installs the panic hook and calls
//!    [`crate::app::run_wasm`], which builds the typed
//!    `EventLoop<AppEvent>`, wires an `App::new_empty` with the
//!    event-loop proxy, and `spawn_app`s it (non-blocking).
//! 2. winit's `resumed` creates the canvas-backed window and spawns
//!    the async `Gfx::new`; when it resolves it sends
//!    `AppEvent::GfxReady` back through the proxy.
//! 3. `start` also wires the `<input type="file">` ROM picker: on
//!    selection it reads the bytes and sends `AppEvent::RomLoaded`
//!    through the proxy, which the `App` turns into a running `Nes`.
//!
//! Audio on this path is a 1.4c follow-up (the `wasm-canvas` embed
//! mode has working Web Audio today); video + input + the egui
//! debugger are the 1.4b deliverable.
//!
//! See `docs/audit/v1.3-sprint-1.4-winit-wgpu-unification-2026-05-24.md`.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Event, FileReader, HtmlInputElement};
use winit::event_loop::EventLoopProxy;

use crate::app::{run_wasm, AppEvent};

/// Entry point invoked from `index.html` after the `.wasm` loads.
///
/// # Errors
///
/// Returns a `JsValue` if the ROM `<input>` element can't be located.
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    log("RustyNES v2 wasm32 — v1.3.0 Sprint 1.4 unified winit/wgpu boot");

    // Build + spawn the winit App; keep the proxy to feed it ROM bytes.
    let proxy = run_wasm();
    // Clone for the `.rnm` movie loader before the ROM-loader closure below
    // moves the original.
    let proxy_for_movie = proxy.clone();

    // Wire the browser file picker -> AppEvent::RomLoaded.
    let document = web_sys::window()
        .ok_or("no window")?
        .document()
        .ok_or("no document")?;
    let rom_input: HtmlInputElement = document
        .get_element_by_id("rom-input")
        .ok_or("missing <input id=\"rom-input\">")?
        .dyn_into()?;

    let on_change = Closure::<dyn FnMut(Event)>::new(move |ev: Event| {
        let Some(input) = ev
            .target()
            .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
        else {
            return;
        };
        let Some(files) = input.files() else { return };
        let Some(file) = files.get(0) else { return };
        // Start Web Audio HERE in the `change` handler — this is the
        // reliable user-gesture point for `AudioContext.resume()`. The
        // later async `FileReader.onload` may fall outside the gesture
        // window, leaving the context suspended (silent).
        let _ = crate::wasm_audio::ensure_audio();
        let Ok(reader) = FileReader::new() else {
            return;
        };
        let reader_clone = reader.clone();
        let proxy = proxy.clone();
        let on_load = Closure::<dyn FnMut()>::new(move || {
            let Ok(buffer) = reader_clone.result() else {
                return;
            };
            let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
            log(&format!(
                "ROM selected ({} bytes) — handing to winit App",
                bytes.len()
            ));
            // Deliver into the event loop; the App creates the Nes at
            // the now-established audio sample rate.
            let _ = proxy.send_event(AppEvent::RomLoaded(bytes));
        });
        reader.set_onload(Some(on_load.as_ref().unchecked_ref()));
        on_load.forget();
        let _ = reader.read_as_array_buffer(&file);
    });
    rom_input.set_onchange(Some(on_change.as_ref().unchecked_ref()));
    on_change.forget();

    // v1.6.0 Sprint 4 — wire the hidden `.rnm` movie loader. The F7 hotkey
    // (handled in `App`) `.click()`s this input from within its gesture
    // handler; the resulting selection feeds bytes back as
    // `AppEvent::MovieLoaded`.
    install_movie_loader(&document, proxy_for_movie.clone());

    // v2.2.0 — wire the FDS BIOS (`disksys.rom`) upload `<input id="fds-bios-input">`.
    // The browser has no filesystem prompt, so the user uploads the 8 KiB BIOS
    // once; the bytes come back as `AppEvent::FdsBiosLoaded`. A `.fds` disk is
    // loaded through the SAME `rom-input` picker (its accept list includes
    // `.fds`), and the `App`'s `start_nes` builds the FDS `Nes` once both are
    // present.
    install_fds_bios_loader(&document, proxy_for_movie);

    log("RustyNES v2 wasm32 — armed. Load a .nes / .fds image to begin.");
    Ok(())
}

/// v1.6.0 Sprint 4 — create (if absent) the hidden `<input id="rnm-input"
/// type="file" accept=".rnm">` and wire its change handler to deliver the
/// selected movie bytes as [`AppEvent::MovieLoaded`].
///
/// The element is created programmatically so the host `index.html` needs
/// no extra markup, and `App::handle_movie_play_toggle_wasm` only has to
/// `.click()` it by id from the F7 gesture handler.
fn install_movie_loader(document: &web_sys::Document, proxy: EventLoopProxy<AppEvent>) {
    // Reuse an existing element if the page already provides one; otherwise
    // build a hidden file input and append it to the body.
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
        // Keep it out of the layout (it's clicked programmatically from the
        // F7 hotkey). `set_attribute` avoids needing the
        // `CssStyleDeclaration` web-sys feature for `style()`.
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
        let proxy = proxy.clone();
        let on_load = Closure::<dyn FnMut()>::new(move || {
            let Ok(buffer) = reader_clone.result() else {
                return;
            };
            let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
            log(&format!(
                "movie selected ({} bytes) — handing to winit App",
                bytes.len()
            ));
            let _ = proxy.send_event(AppEvent::MovieLoaded(bytes));
        });
        reader.set_onload(Some(on_load.as_ref().unchecked_ref()));
        on_load.forget();
        let _ = reader.read_as_array_buffer(&file);
    });
    input.set_onchange(Some(on_change.as_ref().unchecked_ref()));
    on_change.forget();
}

/// v2.2.0 — create (if absent) `<input id="fds-bios-input" type="file">` and
/// wire its change handler to deliver the uploaded FDS BIOS (`disksys.rom`)
/// bytes as [`AppEvent::FdsBiosLoaded`].
///
/// Unlike the hidden movie input, this one is left visible (the host page can
/// style/label it) so the user has an obvious "upload the BIOS" affordance; if
/// the page already provides an element with this id it is reused as-is.
fn install_fds_bios_loader(document: &web_sys::Document, proxy: EventLoopProxy<AppEvent>) {
    let input: HtmlInputElement = if let Some(existing) = document
        .get_element_by_id("fds-bios-input")
        .and_then(|el| el.dyn_into::<HtmlInputElement>().ok())
    {
        existing
    } else {
        let Some(el) = document
            .create_element("input")
            .ok()
            .and_then(|el| el.dyn_into::<HtmlInputElement>().ok())
        else {
            log("FDS BIOS loader: could not create <input>");
            return;
        };
        el.set_id("fds-bios-input");
        el.set_type("file");
        el.set_accept(".rom,.bin");
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
        let proxy = proxy.clone();
        let on_load = Closure::<dyn FnMut()>::new(move || {
            let Ok(buffer) = reader_clone.result() else {
                return;
            };
            let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
            log(&format!("FDS BIOS selected ({} bytes)", bytes.len()));
            let _ = proxy.send_event(AppEvent::FdsBiosLoaded(bytes));
        });
        reader.set_onload(Some(on_load.as_ref().unchecked_ref()));
        on_load.forget();
        let _ = reader.read_as_array_buffer(&file);
    });
    input.set_onchange(Some(on_change.as_ref().unchecked_ref()));
    on_change.forget();
}

/// `console.log` shim.
fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

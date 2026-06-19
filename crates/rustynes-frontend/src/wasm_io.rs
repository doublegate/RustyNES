//! v1.6.0 Sprint 4 — shared wasm32 browser I/O helpers.
//!
//! This module is gated `#[cfg(target_arch = "wasm32")]` and collects the
//! browser-host I/O primitives shared by BOTH wasm frontends — the
//! lightweight canvas-2D embed (`wasm.rs`) and the unified winit/wgpu
//! path (`wasm_winit.rs` + `app.rs`):
//!
//! - **`localStorage`** access + a byte-safe base64 codec built on the
//!   browser's `btoa`/`atob` (a UTF-16 string store can't hold raw bytes
//!   directly, so save-state blobs are base64-encoded).
//! - **Save-state persistence** keyed by the ROM SHA-256 + slot, mirroring
//!   the native `save_state` filesystem layout
//!   (`rustynes-save-<rom_sha256_hex>-slot<N>`).
//! - **Blob downloads** (`download_bytes`) — the browser-native equivalent
//!   of the native `rfd` save dialog: build a `Blob`, mint an object URL,
//!   click a synthetic `<a download>`, then revoke the URL.
//!
//! Nothing here panics on I/O failure: every fallible browser call is
//! degraded to a `log(...)` console message, matching the existing wasm
//! code style. Native never compiles this module.

// The File System Access API helper (v1.7.0 H6) drives the browser's
// single-threaded JS futures via `JsFuture`, which is `!Send`. Allow
// `future_not_send` module-wide rather than fake-`Send` the JS handles (the
// same idiom as `wasm_idb` / `wasm_cheevos`).
#![allow(clippy::future_not_send)]

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::save_state::hex_sha256;

/// Build the `localStorage` key for a save-state slot, keyed by the ROM
/// SHA-256 (hex) so distinct ROMs never collide — the browser analogue of
/// the native `<data_dir>/saves/<hex>/slotN.rns` layout.
#[must_use]
pub fn save_state_key(rom_sha256: &[u8; 32], slot: u8) -> String {
    format!("rustynes-save-{}-slot{slot}", hex_sha256(rom_sha256))
}

/// `window.localStorage` accessor. `None` if storage is unavailable (e.g.
/// blocked by the browser's privacy settings).
#[must_use]
pub fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// Persist a save-state blob to `localStorage` under the per-ROM slot key.
/// Best-effort: logs and returns on any failure (missing storage, quota).
pub fn localstorage_save_state(rom_sha256: &[u8; 32], slot: u8, blob: &[u8]) {
    let Some(storage) = local_storage() else {
        log("save state: localStorage unavailable");
        return;
    };
    let key = save_state_key(rom_sha256, slot);
    let encoded = base64_encode(blob);
    match storage.set_item(&key, &encoded) {
        Ok(()) => log(&format!(
            "state saved to slot {slot} ({} bytes)",
            blob.len()
        )),
        Err(_) => log("save state: localStorage write failed (quota?)"),
    }
}

/// Read a save-state blob back from `localStorage`. Returns `None` when no
/// state is stored for this ROM + slot or the stored value is corrupt.
#[must_use]
pub fn localstorage_load_state(rom_sha256: &[u8; 32], slot: u8) -> Option<Vec<u8>> {
    let storage = local_storage()?;
    let key = save_state_key(rom_sha256, slot);
    let Ok(Some(encoded)) = storage.get_item(&key) else {
        log(&format!("load state: no saved state in slot {slot}"));
        return None;
    };
    let Some(bytes) = base64_decode(&encoded) else {
        log("load state: corrupt save (base64 decode failed)");
        return None;
    };
    Some(bytes)
}

/// Trigger a browser download of `bytes` as `filename`.
///
/// Mints an object URL from a `Blob`, clicks a synthetic `<a download>`,
/// then revokes the URL. MUST be called from a user gesture handler (a
/// hotkey/button callback) per the browser download policy. Best-effort:
/// logs and returns on any failure.
pub fn download_bytes(filename: &str, bytes: &[u8]) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        log("download: no document");
        return;
    };

    // A Blob is built from a JS array of parts; wrap the bytes in a
    // Uint8Array and pass a single-element array to the Blob ctor.
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array.into());
    let Ok(blob) = web_sys::Blob::new_with_u8_array_sequence(&parts) else {
        log("download: Blob construction failed");
        return;
    };

    let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else {
        log("download: createObjectURL failed");
        return;
    };

    // Build a detached <a download> and click it. `create_element`
    // returns an `Element`; downcast to the anchor for the typed setters.
    let Some(anchor) = document
        .create_element("a")
        .ok()
        .and_then(|el| el.dyn_into::<web_sys::HtmlAnchorElement>().ok())
    else {
        log("download: could not create <a>");
        let _ = web_sys::Url::revoke_object_url(&url);
        return;
    };
    anchor.set_href(&url);
    anchor.set_download(filename);
    anchor.click();

    // The object URL is no longer needed once the click has been queued.
    let _ = web_sys::Url::revoke_object_url(&url);
    log(&format!(
        "download triggered: {filename} ({} bytes)",
        bytes.len()
    ));
}

/// Whether the browser exposes the **File System Access API**.
///
/// v1.7.0 "Forge" beta.5 Workstream H6. Probes `window.showSaveFilePicker`:
/// Chromium-family browsers have it; Firefox / Safari currently do not, in
/// which case callers fall back to the [`download_bytes`] path. (ADR 0021.)
#[must_use]
pub fn fs_access_supported() -> bool {
    web_sys::window().is_some_and(|w| {
        js_sys::Reflect::get(&w, &JsValue::from_str("showSaveFilePicker"))
            .ok()
            .is_some_and(|f| f.is_function())
    })
}

/// Save `bytes` to a user-chosen file, preferring the File System Access API.
///
/// v1.7.0 "Forge" beta.5 Workstream H6. Drives `showSaveFilePicker` →
/// `createWritable` → `write` → `close`, with a graceful fallback to the
/// [`download_bytes`] (synthetic `<a download>`) path on unsupported browsers
/// or any failure.
///
/// `suggested_name` seeds the picker's filename; `description` / `accept` set
/// the file-type filter (e.g. `"NES save state"` + `(".rns", "application/octet-stream")`).
/// MUST be called from a user-gesture handler (the picker requires it).
///
/// The whole API is reached **dynamically** through `js_sys::Reflect` /
/// `js_sys::Function` rather than `web-sys`'s unstable-gated bindings, so the
/// build needs no `web_sys_unstable_apis` flag (keeping the wasm-bindgen pin +
/// CI surface unchanged). See ADR 0021.
pub fn save_file_with_fallback(
    suggested_name: &str,
    description: &str,
    accept_ext: &str,
    accept_mime: &str,
    bytes: &[u8],
) {
    if !fs_access_supported() {
        download_bytes(suggested_name, bytes);
        return;
    }
    // Drive the async picker on the microtask queue. On ANY failure (including
    // the user cancelling) we log and stop — we deliberately do NOT fall back to
    // a download on cancel, since that would surprise the user with an unwanted
    // file; an unsupported browser already took the fallback path above.
    let name = suggested_name.to_owned();
    let desc = description.to_owned();
    let ext = accept_ext.to_owned();
    let mime = accept_mime.to_owned();
    let data = bytes.to_vec();
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = save_via_fs_access(&name, &desc, &ext, &mime, &data).await {
            log(&format!("File System Access save failed/cancelled: {e:?}"));
        } else {
            log(&format!("saved {name} ({} bytes) to disk", data.len()));
        }
    });
}

/// Async core of [`save_file_with_fallback`]: invoke the FS Access API entirely
/// through dynamic reflection so no `web-sys` unstable binding is needed.
async fn save_via_fs_access(
    suggested_name: &str,
    description: &str,
    accept_ext: &str,
    accept_mime: &str,
    bytes: &[u8],
) -> Result<(), JsValue> {
    use js_sys::{Array, Function, Object, Promise, Reflect};
    use wasm_bindgen_futures::JsFuture;

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;

    // Build the options object:
    //   { suggestedName, types: [{ description, accept: { mime: [ext] } }] }
    let opts = Object::new();
    Reflect::set(
        &opts,
        &JsValue::from_str("suggestedName"),
        &JsValue::from_str(suggested_name),
    )?;
    let accept = Object::new();
    let exts = Array::new();
    exts.push(&JsValue::from_str(accept_ext));
    Reflect::set(&accept, &JsValue::from_str(accept_mime), &exts)?;
    let type_entry = Object::new();
    Reflect::set(
        &type_entry,
        &JsValue::from_str("description"),
        &JsValue::from_str(description),
    )?;
    Reflect::set(&type_entry, &JsValue::from_str("accept"), &accept)?;
    let types = Array::new();
    types.push(&type_entry);
    Reflect::set(&opts, &JsValue::from_str("types"), &types)?;

    // handle = await window.showSaveFilePicker(opts)
    let picker: Function = Reflect::get(&window, &JsValue::from_str("showSaveFilePicker"))?
        .dyn_into()
        .map_err(|_| JsValue::from_str("showSaveFilePicker not a function"))?;
    let handle = JsFuture::from(Promise::resolve(&picker.call1(&window, &opts)?)).await?;

    // writable = await handle.createWritable()
    let create_writable: Function = Reflect::get(&handle, &JsValue::from_str("createWritable"))?
        .dyn_into()
        .map_err(|_| JsValue::from_str("createWritable not a function"))?;
    let writable = JsFuture::from(Promise::resolve(&create_writable.call0(&handle)?)).await?;

    // await writable.write(uint8array)
    let write: Function = Reflect::get(&writable, &JsValue::from_str("write"))?
        .dyn_into()
        .map_err(|_| JsValue::from_str("write not a function"))?;
    let array = js_sys::Uint8Array::from(bytes);
    JsFuture::from(Promise::resolve(&write.call1(&writable, &array)?)).await?;

    // await writable.close()
    let close: Function = Reflect::get(&writable, &JsValue::from_str("close"))?
        .dyn_into()
        .map_err(|_| JsValue::from_str("close not a function"))?;
    JsFuture::from(Promise::resolve(&close.call0(&writable)?)).await?;

    Ok(())
}

/// Click a hidden `<input type="file">` by element id.
///
/// Opens the browser file picker — used to drive the movie-`.rnm` upload
/// from a hotkey, so it MUST be called from a user gesture handler. No-op
/// if the element is absent.
pub fn click_file_input(id: &str) {
    let Some(input) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
    else {
        log(&format!("file input '{id}' not found"));
        return;
    };
    input.click();
}

/// Base64-encode via the browser's `btoa` (operating on a Latin-1 string
/// built from the raw bytes — `btoa` is byte-safe for code points 0-255).
/// Avoids pulling in a base64 crate.
#[must_use]
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut latin1 = String::with_capacity(bytes.len());
    for &b in bytes {
        latin1.push(b as char);
    }
    web_sys::window()
        .and_then(|w| w.btoa(&latin1).ok())
        .unwrap_or_default()
}

/// Inverse of [`base64_encode`] via `atob`. `None` if `atob` rejects the
/// input (not valid base64).
#[must_use]
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let latin1 = web_sys::window()?.atob(s).ok()?;
    // Each char is a byte (0-255); narrow back to u8.
    Some(latin1.chars().map(|c| c as u8).collect())
}

/// v1.7.0 "Forge" beta.5 Workstream H6 — URL-safe base64 (RFC 4648 §5) for the
/// `?settings=` share-link blob.
///
/// Standard base64 uses `+` / `/` and `=` padding, all of which are unsafe (or
/// ambiguous) inside a URL query string. This maps `+`→`-`, `/`→`_`, and strips
/// the trailing `=` padding so the blob is a single URL-clean token.
#[must_use]
pub fn base64url_encode(bytes: &[u8]) -> String {
    let mut s = base64_encode(bytes);
    s = s.replace('+', "-").replace('/', "_");
    // Strip `=` padding (it is reconstructable from the length on decode).
    s.trim_end_matches('=').to_string()
}

/// Inverse of [`base64url_encode`]. Restores the standard alphabet + `=`
/// padding, then decodes via `atob`. `None` on malformed input.
#[must_use]
pub fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    let mut std = s.replace('-', "+").replace('_', "/");
    // Re-pad to a multiple of 4 so `atob` accepts it.
    match std.len() % 4 {
        2 => std.push_str("=="),
        3 => std.push('='),
        0 => {}
        // A length of `% 4 == 1` is never a valid base64 encoding.
        _ => return None,
    }
    base64_decode(&std)
}

/// `console.log` shim.
pub fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

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

/// `console.log` shim.
pub fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

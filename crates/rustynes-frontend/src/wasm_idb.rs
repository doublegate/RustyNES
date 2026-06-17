//! v1.4.0 Workstream E2 — IndexedDB save-state store (wasm32).
//!
//! This module is gated `#[cfg(target_arch = "wasm32")]`. It moves the
//! browser save-state slots off `localStorage` (a string-only, ~5 MiB,
//! synchronous store that base64-bloats binary blobs by ~33%) and onto
//! **IndexedDB**, which stores raw binary (`Uint8Array`) with a far larger
//! quota and supports the multi-slot + thumbnail-grid UX the native
//! Save-States manager already has.
//!
//! ## Why a fresh module (not folded into `wasm_io`)
//!
//! `wasm_io`'s `localStorage` helpers are **synchronous** — they run inside
//! the F1/F4 hotkey gesture handler and return immediately. IndexedDB is
//! **fully asynchronous**: opening the database, running a transaction, and
//! reading a value each resolve through DOM events. So the IDB API here is
//! `async fn`-shaped (each `IdbRequest` is wrapped in a `Promise` and
//! awaited via `wasm_bindgen_futures::JsFuture`), and callers drive it with
//! `wasm_bindgen_futures::spawn_local` from their gesture handlers. The
//! `localStorage` path stays as a synchronous **fallback** (private-mode /
//! IDB-blocked browsers) and a one-time **migration source**.
//!
//! ## Determinism + native parity
//!
//! Nothing here touches the emulator core or its synthesis. The stored blob
//! is the EXACT `Nes::snapshot()` byte string the native filesystem slots
//! (`<data_dir>/saves/<rom_sha256_hex>/slotN.rns`) and the old wasm
//! `localStorage` slots hold — same format, same thumbnail `THM ` section.
//! Native never compiles this module; the desktop save-state format is
//! unaffected.
//!
//! ## Schema
//!
//! - Database `rustynes` (version 1), one object store `save-states`.
//! - Key = `"<rom_sha256_hex>:slot<N>"` (the per-ROM-per-slot string), so
//!   distinct ROMs + slots never collide — the IDB analogue of the
//!   `localStorage` `save_state_key`.
//! - Value = the raw snapshot bytes as a `Uint8Array`.
//!
//! Every fallible browser call degrades to a `wasm_io::log(...)` console
//! message and a `None` / early return — nothing here panics on I/O failure.

// `IndexedDB` / `IDB` read as CamelCase "code" to `doc_markdown`, but they're
// the product names of the browser API this module documents; backticking
// every prose mention would hurt readability. The async fns here are ALWAYS
// driven by `wasm_bindgen_futures::spawn_local` (the browser is single-
// threaded), so `JsFuture`'s non-`Send`-ness is irrelevant — silence
// `future_not_send` rather than fake-`Send` the JS handles.
#![allow(clippy::doc_markdown, clippy::future_not_send)]

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{IdbDatabase, IdbObjectStore, IdbOpenDbRequest, IdbRequest, IdbTransactionMode};

use crate::save_state::hex_sha256;
use crate::wasm_io::log;

/// IndexedDB database name.
const DB_NAME: &str = "rustynes";
/// IndexedDB schema version.
const DB_VERSION: u32 = 1;
/// The single object store holding the save-state blobs.
const STORE: &str = "save-states";

/// Build the IndexedDB key for a save-state slot: `"<sha256_hex>:slot<N>"`.
///
/// Distinct from the `localStorage` key (`rustynes-save-<hex>-slot<N>`) so a
/// migration can read the old `localStorage` value and write the new IDB one
/// without a key clash.
#[must_use]
pub fn idb_key(rom_sha256: &[u8; 32], slot: u8) -> String {
    format!("{}:slot{slot}", hex_sha256(rom_sha256))
}

/// Wrap an [`IdbRequest`] in a JS `Promise` that resolves with the request's
/// `result` on `success` and rejects on `error`, so it can be `.await`ed.
fn request_to_promise(request: &IdbRequest) -> js_sys::Promise {
    let request = request.clone();
    js_sys::Promise::new(&mut |resolve, reject| {
        // onsuccess -> resolve(request.result)
        let req_ok = request.clone();
        let on_success = Closure::once_into_js(move |_evt: web_sys::Event| {
            let value = req_ok.result().unwrap_or(JsValue::UNDEFINED);
            let _ = resolve.call1(&JsValue::NULL, &value);
        });
        request.set_onsuccess(Some(on_success.unchecked_ref()));

        // onerror -> reject(request.error or UNDEFINED)
        let on_error = Closure::once_into_js(move |_evt: web_sys::Event| {
            let _ = reject.call1(&JsValue::NULL, &JsValue::UNDEFINED);
        });
        request.set_onerror(Some(on_error.unchecked_ref()));
    })
}

/// Open (and, on first use / version bump, create the object store of) the
/// `rustynes` IndexedDB database. `None` if IndexedDB is unavailable
/// (private mode, blocked by policy, or no `window`).
async fn open_db() -> Option<IdbDatabase> {
    let factory = web_sys::window()?.indexed_db().ok()??;
    let open_req: IdbOpenDbRequest = factory.open_with_u32(DB_NAME, DB_VERSION).ok()?;

    // The `upgradeneeded` handler creates the object store the first time the
    // DB is opened at this version. It fires before `success`.
    let on_upgrade = Closure::<dyn FnMut(web_sys::Event)>::new(move |evt: web_sys::Event| {
        let Some(target) = evt.target() else { return };
        let Ok(req) = target.dyn_into::<IdbOpenDbRequest>() else {
            return;
        };
        let Ok(result) = req.result() else { return };
        let Ok(db) = result.dyn_into::<IdbDatabase>() else {
            return;
        };
        if !db.object_store_names().contains(STORE) {
            let _ = db.create_object_store(STORE);
        }
    });
    open_req.set_onupgradeneeded(Some(on_upgrade.as_ref().unchecked_ref()));

    let promise = request_to_promise(open_req.as_ref());
    let result = JsFuture::from(promise).await.ok()?;
    // `on_upgrade` is held alive across the await above (it fires before
    // `success`), so it is dropped on return here instead of leaked via
    // `forget()` — avoiding a closure leak on every `open_db()` call.
    drop(on_upgrade);
    result.dyn_into::<IdbDatabase>().ok()
}

/// Persist `blob` to the IDB slot keyed by the ROM SHA-256 + `slot`.
///
/// Async + best-effort: logs and returns on any browser failure. Callers
/// drive this from a gesture handler via `spawn_local`.
pub async fn put_state(rom_sha256: [u8; 32], slot: u8, blob: Vec<u8>) {
    let Some(db) = open_db().await else {
        log("save state: IndexedDB unavailable — falling back to localStorage");
        crate::wasm_io::localstorage_save_state(&rom_sha256, slot, &blob);
        return;
    };
    let Ok(tx) = db.transaction_with_str_and_mode(STORE, IdbTransactionMode::Readwrite) else {
        log("save state: IndexedDB transaction failed");
        return;
    };
    let Ok(store) = tx.object_store(STORE) else {
        log("save state: IndexedDB object store missing");
        return;
    };
    let key = idb_key(&rom_sha256, slot);
    // Store the raw bytes as a Uint8Array (no base64 — IDB is binary-safe).
    let value = js_sys::Uint8Array::from(blob.as_slice());
    let Ok(req) = store.put_with_key(value.as_ref(), &JsValue::from_str(&key)) else {
        log("save state: IndexedDB put failed");
        return;
    };
    match JsFuture::from(request_to_promise(&req)).await {
        Ok(_) => log(&format!(
            "state saved to IndexedDB slot {} ({} bytes)",
            slot + 1,
            blob.len()
        )),
        Err(_) => log("save state: IndexedDB write rejected (quota?)"),
    }
}

/// Read a save-state blob back from the IDB slot, falling back to (and
/// migrating from) `localStorage` if the IDB slot is empty.
///
/// Async + best-effort: returns `None` on any failure / empty slot.
pub async fn get_state(rom_sha256: [u8; 32], slot: u8) -> Option<Vec<u8>> {
    if let Some(db) = open_db().await {
        if let Some(bytes) = get_state_from_db(&db, &rom_sha256, slot).await {
            return Some(bytes);
        }
        // IDB slot empty — try the old localStorage slot and migrate it.
        if let Some(bytes) = crate::wasm_io::localstorage_load_state(&rom_sha256, slot) {
            log(&format!(
                "migrating slot {} from localStorage -> IndexedDB",
                slot + 1
            ));
            put_state(rom_sha256, slot, bytes.clone()).await;
            return Some(bytes);
        }
        return None;
    }
    // No IDB at all: pure localStorage fallback.
    crate::wasm_io::localstorage_load_state(&rom_sha256, slot)
}

/// Read one slot's bytes from an already-open DB (no fallback).
async fn get_state_from_db(db: &IdbDatabase, rom_sha256: &[u8; 32], slot: u8) -> Option<Vec<u8>> {
    let store = readonly_store(db)?;
    let key = idb_key(rom_sha256, slot);
    let req = store.get(&JsValue::from_str(&key)).ok()?;
    let value = JsFuture::from(request_to_promise(&req)).await.ok()?;
    if value.is_undefined() || value.is_null() {
        return None;
    }
    let array = value.dyn_into::<js_sys::Uint8Array>().ok()?;
    Some(array.to_vec())
}

/// Open a readonly object-store handle on `db`.
fn readonly_store(db: &IdbDatabase) -> Option<IdbObjectStore> {
    let tx = db
        .transaction_with_str_and_mode(STORE, IdbTransactionMode::Readonly)
        .ok()?;
    tx.object_store(STORE).ok()
}

/// One slot's metadata for the wasm Save-States manager grid: whether the
/// slot is occupied + its 128x120 RGBA thumbnail (if the blob carries one).
pub struct SlotMeta {
    /// `true` if a blob is stored in this slot.
    pub occupied: bool,
    /// The decoded `THM ` thumbnail (RGBA8 128x120), if present.
    pub thumbnail: Option<Vec<u8>>,
}

/// Scan all `slots` for the given ROM and return per-slot metadata for the
/// thumbnail grid. Reads each blob once and extracts its thumbnail without
/// restoring the emulator (`Nes::extract_thumbnail`).
///
/// Async; best-effort (a failed slot reports as empty). Returns a `Vec` of
/// length `slots` (slot 0..slots).
pub async fn scan_slots(rom_sha256: [u8; 32], slots: u8) -> Vec<SlotMeta> {
    let mut out = Vec::with_capacity(slots as usize);
    let db = open_db().await;
    for slot in 0..slots {
        let blob = match db.as_ref() {
            Some(db) => get_state_from_db(db, &rom_sha256, slot)
                .await
                .or_else(|| crate::wasm_io::localstorage_load_state(&rom_sha256, slot)),
            None => crate::wasm_io::localstorage_load_state(&rom_sha256, slot),
        };
        let meta = blob.map_or(
            SlotMeta {
                occupied: false,
                thumbnail: None,
            },
            |bytes| SlotMeta {
                occupied: true,
                thumbnail: rustynes_core::Nes::extract_thumbnail(&bytes).ok().flatten(),
            },
        );
        out.push(meta);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idb_key_is_per_rom_per_slot() {
        let a = [0xABu8; 32];
        let b = [0xCDu8; 32];
        assert_ne!(idb_key(&a, 0), idb_key(&b, 0), "distinct ROMs differ");
        assert_ne!(idb_key(&a, 0), idb_key(&a, 1), "distinct slots differ");
        assert!(idb_key(&a, 3).ends_with(":slot3"));
        assert_eq!(idb_key(&a, 0).len(), 64 + ":slot0".len());
    }
}

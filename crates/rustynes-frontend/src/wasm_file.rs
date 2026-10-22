//! v3.5.0 — read one browser `File` into bytes, with a callback that frees
//! itself, for every file picker and drop zone in both wasm frontends.
//!
//! Each site used to build a `Closure<dyn FnMut()>` that owned a clone of its
//! `FileReader`, install it as `onload` and `forget()` it. A forgotten closure
//! is never freed, and through its reader clone it kept the reader's `result`
//! alive too, so every ROM or movie a session opened stayed in memory as a
//! whole `ArrayBuffer` after the emulator had replaced it (`Copilot` on #624,
//! against the drop handler; the pickers had the same shape since v1.x).
//!
//! The callback here is a [`Closure::once_into_js`], which frees its Rust side
//! (and so the reader clone) after its one call, and it is installed as
//! `onloadend`, which fires exactly once whether the read succeeds, fails or
//! is aborted, so no outcome leaves it behind. It also clears the handler, so
//! the reader holds nothing of it afterwards. Only a read that never starts
//! (`read_as_array_buffer` returning an error, so no `loadend` follows) leaks
//! the callback; that case is reported to the caller.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::{File, FileReader};

/// Read `file` and hand its bytes to `deliver` once the read ends.
///
/// `deliver` is not called if the read fails or is aborted (the reader's
/// `result` is then `null`); `on_fail` is called instead, with the file's
/// name, so the site can log it. Returns `false` if the read could not start,
/// in which case neither callback ever runs.
pub fn read_file_once(
    file: &File,
    deliver: impl FnOnce(Vec<u8>) + 'static,
    on_fail: impl FnOnce(String) + 'static,
) -> bool {
    let Ok(reader) = FileReader::new() else {
        return false;
    };
    let reader_clone = reader.clone();
    let name = file.name();
    let on_end = Closure::once_into_js(move || {
        reader_clone.set_onloadend(None);
        match reader_clone.result() {
            Ok(buffer) if !buffer.is_null() && !buffer.is_undefined() => {
                deliver(js_sys::Uint8Array::new(&buffer).to_vec());
            }
            _ => on_fail(name),
        }
    });
    reader.set_onloadend(Some(on_end.unchecked_ref()));
    reader.read_as_array_buffer(file).is_ok()
}

//! Small helpers for converting borrowed C strings into owned Rust `String`s.

use std::ffi::CStr;
use std::os::raw::c_char;

/// Convert a NUL-terminated C string pointer into an owned [`String`].
///
/// Returns an empty string for a null pointer. Invalid UTF-8 is replaced
/// lossily.
#[must_use]
pub(crate) fn cstr_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: rcheevos strings are NUL-terminated and valid for the duration of
    // the call in which we read them; we copy immediately into an owned String.
    let cstr = unsafe { CStr::from_ptr(ptr) };
    cstr.to_string_lossy().into_owned()
}

/// Convert a fixed-size, possibly-NUL-terminated `char[N]` field into an owned
/// [`String`] (stopping at the first NUL, or using the whole array if none).
#[must_use]
pub(crate) fn cchar_arr_to_string(arr: &[c_char]) -> String {
    let bytes: Vec<u8> = arr
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

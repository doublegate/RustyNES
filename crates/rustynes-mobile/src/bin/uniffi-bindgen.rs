//! Standalone `UniFFI` binding generator for `rustynes-mobile`.
//!
//! The platform builds (Gradle for Android, Xcode/SwiftPM for iOS) invoke this
//! to emit the Kotlin/Swift bindings from the compiled cdylib, e.g.:
//!
//! ```text
//! cargo run -p rustynes-mobile --bin uniffi-bindgen -- \
//!     generate --library target/aarch64-linux-android/release/librustynes_mobile.so \
//!     --language kotlin --out-dir android/app/build/generated/uniffi
//! ```
fn main() {
    uniffi::uniffi_bindgen_main();
}

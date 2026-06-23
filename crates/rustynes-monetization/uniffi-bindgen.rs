//! uniffi-bindgen.rs — in-crate binding generator.
//!
//! UniFFI's standalone `uniffi-bindgen` requires a nightly toolchain to discover the
//! compiled library's metadata. The supported workaround on stable Rust is to expose
//! the exact same entry point from inside the crate, which is what this binary does.
//!
//! The real entry point only exists when the `uniffi/cli` feature is on, which our
//! crate's `cli` feature enables. The body is therefore `#[cfg]`-gated so that an
//! ordinary build or `cargo test` (which compiles every target, including this bin)
//! does not require the CLI dependencies.
//!
//! Run it (after building the native library) like so — see README for full commands:
//!
//!   cargo run --features=cli --bin uniffi-bindgen -- generate \
//!       --library target/<triple>/release/librustynes_monetization.<so|a> \
//!       --language <kotlin|swift> --out-dir <output>
//!
//! `--library` mode reads the type definitions straight from the built artifact, so
//! the generated Kotlin/Swift can never fall out of sync with the Rust source.

fn main() {
    #[cfg(feature = "cli")]
    uniffi::uniffi_bindgen_main();

    #[cfg(not(feature = "cli"))]
    eprintln!(
        "uniffi-bindgen was built without the `cli` feature. \
         Re-run with: cargo run --features=cli --bin uniffi-bindgen -- <args>"
    );
}

//! lib.rs — Crate root for `rustynes_monetization`.
//!
//! This is the mobile **monetization bridge** crate, intentionally separate from the
//! RustyNES emulator crates (`rustynes-cpu/ppu/apu/mappers/core`). It carries only the
//! cross-platform entitlement + ad-pacing policy that the Android and iOS shells share via
//! UniFFI. It does NOT contain emulator logic and must never feed state into the
//! deterministic emulation core (see `monetization.rs` for why).
//!
//! `uniffi::setup_scaffolding!()` walks every `#[uniffi::export]` / `#[derive(uniffi::*)]`
//! item in the crate (including those in `monetization`) and generates the C-ABI glue
//! that the Kotlin and Swift bindings bind against. It MUST be called precisely once,
//! at the crate root, and the crate name passed implicitly must match the library
//! name in `Cargo.toml` (`rustynes_monetization`).
//!
//! Build & binding generation are documented in docs/build-and-bindings.md.

mod monetization;

// Re-export the public monetization surface at the crate root for ergonomic Rust use
// (tests, other internal modules). This does not affect the generated FFI, which is
// driven by the proc-macro attributes themselves.
pub use monetization::{AdConfig, AdPolicy, PremiumFeature, default_ad_config};

uniffi::setup_scaffolding!();

//! Re-export of the HD-pack loader + compositor, moved to the shared
//! `rustynes-hdpack` crate in v1.8.5 (so the mobile bridge can reach it; the core
//! is `#![no_std]`). Existing `crate::hdpack::…` paths keep working unchanged.

pub use rustynes_hdpack::hdpack::*;

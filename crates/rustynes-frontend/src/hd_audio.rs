//! Re-export of the HD-pack HD-audio decode/mix, moved to the shared
//! `rustynes-hdpack` crate in v1.8.5. Existing `crate::hd_audio::…` paths keep
//! working unchanged.

pub use rustynes_hdpack::hd_audio::*;

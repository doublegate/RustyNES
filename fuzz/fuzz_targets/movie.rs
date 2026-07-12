//! Fuzz target for the movie (`.rnm`) deserializer — untrusted **file input**.
//!
//! A `.rnm` TAS movie is arbitrary on-disk bytes. [`Movie::deserialize`] must
//! reject a malformed / truncated / foreign file with a typed `MovieError`,
//! never panic / OOB / hang. This surface is doubly interesting because a movie
//! header embeds a length-prefixed save-state (`.rns`) start-point blob, so
//! movie deserialization transitively exercises the save-state parser's
//! length-field handling as well as the movie header + per-frame input stream
//! decode.
//!
//! Any movie that *does* deserialize is re-serialized as a cheap
//! self-consistency check (the encoder must accept whatever the decoder
//! produced).
//!
//! Run with:
//!     cargo install cargo-fuzz
//!     cargo +nightly fuzz run movie
//!
//! Per `docs/testing-strategy.md` §Layer 5.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rustynes_core::Movie;

fuzz_target!(|data: &[u8]| {
    if let Ok(movie) = Movie::deserialize(data) {
        // A decoded movie must re-encode without panicking.
        let _ = movie.serialize();
    }
});

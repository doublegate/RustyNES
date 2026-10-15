//! Fuzz target for the foreign-movie importers — untrusted **file input**.
//!
//! The frontend imports TAS movies written by other emulators: FCEUX's `.fm2`
//! (text) and `.fcm` (binary), BizHawk's `.bk2` (a header and an input log,
//! both text), Famtasia's `.fmv` and VirtuaNES's `.vmv` (binary). Each is an
//! arbitrary file a user downloads, so each importer must reject a malformed,
//! truncated or hostile file with its typed error, never panic, index out of
//! bounds or allocate without bound. The `movie` target fuzzes only RustyNES's
//! own `.rnm` decoder (`Movie::deserialize`); these five parsers had no target
//! until v3.2.0, though a v2.9.9 probe pushed 200,000 mutated inputs through
//! them. That probe is what this target replaces.
//!
//! The first input byte selects the importer, so one corpus reaches all five:
//!
//! | `data[0] % 5` | importer | rest of the input |
//! |---|---|---|
//! | 0 | `import_fm2` | UTF-8 text (lossy) |
//! | 1 | `import_bk2` | header and input log, split at the first NUL byte |
//! | 2 | `import_fcm` | bytes |
//! | 3 | `import_fmv` | bytes |
//! | 4 | `import_vmv` | bytes |
//!
//! `import_mc2` is left out: it rejects every input without reading it (`.mc2`
//! is a PC Engine format).
//!
//! Any movie that imports is serialized to `.rnm`, as the frontend does next,
//! so the encoder must accept whatever an importer produced.
//!
//! Run with:
//!     cargo +nightly fuzz run movie_import
//!
//! Per `docs/testing-strategy.md` §Layer 5.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rustynes_core::{
    bk2_interop::import_bk2, import_fcm, import_fmv, import_vmv, movie_interop::import_fm2,
};

/// The importers take the ROM's identity to stamp into the movie; its value
/// does not steer any parse, so a constant keeps the search on the file bytes.
const SHA: [u8; 32] = [0; 32];

fuzz_target!(|data: &[u8]| {
    let Some((&sel, rest)) = data.split_first() else {
        return;
    };
    let movie = match sel % 5 {
        0 => import_fm2(&String::from_utf8_lossy(rest), SHA)
            .ok()
            .map(|(m, _)| m),
        1 => {
            let split = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
            let header = String::from_utf8_lossy(&rest[..split]);
            let log = String::from_utf8_lossy(rest.get(split + 1..).unwrap_or(&[]));
            import_bk2(&header, &log, SHA).ok().map(|(m, _)| m)
        }
        2 => import_fcm(rest, SHA).ok().map(|(m, _)| m),
        3 => import_fmv(rest, SHA).ok().map(|(m, _)| m),
        _ => import_vmv(rest, SHA).ok().map(|(m, _)| m),
    };
    if let Some(movie) = movie {
        let _ = movie.serialize();
    }
});

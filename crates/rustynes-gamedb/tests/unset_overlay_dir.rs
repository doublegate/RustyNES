//! The unconfigured-overlay half of the contract, in a process of its own.
//!
//! Split from `overlay_dir.rs` because both halves mutate the same
//! process-global `OnceLock`: whichever ran first would decide the other's
//! answer, so "no directory configured" can only be asserted somewhere nothing
//! has configured one.

/// With no overlay directory set, resolution is the vendored table alone.
///
/// This is the state the coverage harness runs in, and it is the reason the
/// directory is injected rather than derived: a library that reached for the
/// running user's config dir would make a regression net's results depend on
/// whatever that developer happened to have saved locally.
#[test]
fn an_unset_overlay_directory_resolves_to_the_vendored_table() {
    let mut rom = vec![0u8; 16 + 0x8000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 2;
    rom[5] = 1;
    let crc = rustynes_gamedb::rom_crc32(&rom).expect("header parses");

    let resolved = rustynes_gamedb::entry_for_crc(crc);
    let vendored = rustynes_gamedb::vendored_entry(crc);
    assert_eq!(
        resolved.as_ref().map(|e| (e.crc, e.mapper, e.submapper)),
        vendored.map(|e| (e.crc, e.mapper, e.submapper)),
        "with no overlay configured, entry_for_crc must equal vendored_entry"
    );
}

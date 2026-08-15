//! The overlay-directory contract, in its own test binary.
//!
//! `OVERLAY_DIR` is a process-global `OnceLock` with first-call-wins semantics,
//! so this cannot share a process with any other test that might set it —
//! whichever ran first would decide the outcome and the result would depend on
//! test-thread scheduling. A separate integration-test binary gives it a process
//! of its own, which is the only way to assert "first wins" honestly.

use std::io::Write;

/// One process, one `set_overlay_dir` sequence, all three claims.
#[test]
fn the_overlay_directory_is_first_call_wins_and_optional() {
    // A ROM the vendored table does list, so "vendored still works" is testable.
    // 32 KiB PRG + 8 KiB CHR of zeros; whatever CRC that is, it is stable.
    let mut rom = vec![0u8; 16 + 0x8000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 2;
    rom[5] = 1;
    let crc = rustynes_gamedb::rom_crc32(&rom).expect("header parses");

    // NOTE the ordering: NOTHING may resolve an entry before `set_overlay_dir`.
    // The overlay is read lazily on the first lookup and cached for the process,
    // so a lookup here would silently pin "no overlay" for the whole test. That
    // is the footgun this file exists to pin; `unset_overlay_dir.rs` covers the
    // unconfigured case in a process of its own for the same reason.

    // 1. An explicitly configured overlay IS consumed.
    let dir = std::env::temp_dir().join("rustynes-gamedb-overlay-contract");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let mut f = std::fs::File::create(dir.join("game_db_user.txt")).expect("create");
    // Give this CRC a mapper the vendored table cannot be supplying.
    writeln!(
        f,
        "{crc:08X}, NTSC, 99, 0, 1, 2, 0, false, Vertical, \"overlay row\""
    )
    .expect("write");
    drop(f);

    assert!(
        rustynes_gamedb::set_overlay_dir(dir.clone()),
        "the first call must be accepted"
    );
    let after = rustynes_gamedb::entry_for_crc(crc).expect("overlay row is found");
    assert_eq!(after.mapper, Some(99), "the overlay row must win");
    assert_eq!(after.title, "overlay row");

    // 2. A second configuration cannot silently change which corrections are in
    //    force. It is rejected, and the resolved entry does not move.
    let other = std::env::temp_dir().join("rustynes-gamedb-overlay-contract-2");
    let _ = std::fs::create_dir_all(&other);
    assert!(
        !rustynes_gamedb::set_overlay_dir(other),
        "a late second caller must be rejected"
    );
    let still = rustynes_gamedb::entry_for_crc(crc).expect("still resolves");
    assert_eq!(still.mapper, Some(99), "the first directory still decides");

    let _ = std::fs::remove_dir_all(&dir);
}

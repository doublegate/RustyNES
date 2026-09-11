//! v2.6.18 study: what value does `Misaligned OAM2 Address` actually read?
//!
//! The ROM stores the byte it read from `$2004` at zero page `$50`
//! (`LDA $2004 / CMP #$06 / STA <$50`), and that test is the LAST entry in the
//! catalog, so the byte survives to the end of a battery run. It expects `$06`
//! = OAM2[$18]. Reading it directly says what the `OAM2Address` counter
//! reached, with no instrumentation at all.
#![cfg(feature = "phi2-write-sweep")]
use core::sync::atomic::Ordering::Relaxed;

#[test]
fn what_did_misaligned_oam2_read() {
    for (roff, woff) in [(0u8, 0u8), (0, 2), (2, 2)] {
        rustynes_core::rustynes_cpu::READ_PHI_OFFSET.store(roff, Relaxed);
        rustynes_core::rustynes_cpu::WRITE_PHI_OFFSET.store(woff, Relaxed);
        let (_r, ram) = rustynes_test_harness::accuracy_coin::run_battery_capturing_ram(7_000);
        eprintln!(
            "READ={roff} WRITE={woff}  $0050 = 0x{:02X}  (expects 0x06 = OAM2[0x18])",
            ram[0x50]
        );
    }
    rustynes_core::rustynes_cpu::READ_PHI_OFFSET.store(0, Relaxed);
    rustynes_core::rustynes_cpu::WRITE_PHI_OFFSET.store(0, Relaxed);
}

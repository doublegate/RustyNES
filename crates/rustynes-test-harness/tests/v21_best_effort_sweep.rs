//! Boot-smoke **sweep of every `BestEffort` (Tier-2) mapper family** — Fathom
//! task F3.1.
//!
//! ## Why this exists
//!
//! `BestEffort` mappers are the reference-ported long-tail boards that have no
//! *cleanly-booting* redistributable ROM dump, so — unlike `Core` / `Curated`
//! families — they can never be honestly gated by the `AccuracyCoin` /
//! commercial-ROM oracle (see `crates/rustynes-mappers/src/tier.rs` and
//! `docs/adr/0011-mapper-tiering.md`). Their per-register behaviour is
//! unit-tested inside `rustynes-mappers` against the nesdev spec, but nothing
//! previously exercised the **full parse -> construct -> dispatch -> run-loop
//! integration** for the whole set. A panic hiding in a `BestEffort` mapper's
//! register decode, bank wiring, or per-tick hook (an unchecked index, a missing
//! default, a divide-by-zero on a degenerate bank count) would therefore only
//! surface when a user loaded a real cart on that board — never in CI.
//!
//! This sweep closes that gap. It is a **pure safety net**: it promotes nothing
//! (every id here stays `BestEffort`) and asserts nothing about accuracy — only
//! that constructing the mapper from a synthetic minimal iNES / NES 2.0 image
//! and running the emulator for ~60 frames never panics and always produces a
//! framebuffer of the expected shape. Per-mapper behavioural correctness remains
//! the job of the `rustynes-mappers` unit tests; this file guards the
//! integration boundary.
//!
//! ## Determinism & cost
//!
//! Each ROM is built in-process (no fixture files, no real dumps), the core is
//! seeded deterministically by `Nes::from_rom`, and the whole sweep runs
//! headless in a couple of seconds. There is no wall-clock, OS RNG, or thread
//! scheduling in the path.
//!
//! ## Data-driven by construction
//!
//! The target set is derived at runtime by querying the
//! [`rustynes_mappers::mapper_tier`] classifier — the single source of truth —
//! for every id in `0..=MAX_MAPPER_ID`, rather than hand-listing the ids. Any
//! future family promoted *into* (or out of) `BestEffort` is therefore swept (or
//! dropped) automatically the next time CI runs, with no edit here required.
#![cfg(feature = "test-roms")]
#![allow(clippy::doc_markdown)]

use rustynes_core::Nes;
use rustynes_core::rustynes_mappers::{MapperTier, mapper_tier};

/// Upper bound (inclusive) of the iNES / NES 2.0 mapper-number space we probe
/// when enumerating the `BestEffort` set. The NES 2.0 mapper field is 12 bits,
/// so we scan the FULL space (0..=4095) — a future high-id BestEffort board is
/// then swept automatically with no silent gap, and the classifier scan stays
/// trivially cheap (a `const fn` match per id, ~4096 iterations in well under a
/// microsecond).
const MAX_MAPPER_ID: u16 = 4095;

/// Framebuffer geometry: RGBA8, 256x240 (see `Nes::framebuffer`).
const FRAMEBUFFER_LEN: usize = 256 * 240 * 4;

/// Frames to advance per mapper. ~60 frames (~1 s of emulated time) is enough
/// for a board to run its reset handler, take its first mapper-driven IRQ, and
/// exercise the bank registers the boot code touches, while staying fast.
const SWEEP_FRAMES: usize = 60;

/// Build a minimal iNES 1.0 / NES 2.0 ROM declaring `mapper` (with `submapper`),
/// `prg_banks_16k` 16 KiB PRG banks, and either `chr_banks_8k` 8 KiB CHR-ROM
/// banks or, when 0, CHR-RAM (the mapper allocates its own).
///
/// An **NES 2.0** header is emitted whenever the mapper number exceeds the 8-bit
/// iNES range (`> 255`) or a submapper is requested — those cases need the byte-8
/// mapper-MSB / submapper nibble that only NES 2.0 carries. Otherwise a plain
/// iNES 1.0 header is emitted, matching the sibling `v21_coverage_mappers.rs`
/// synth exactly so the two suites stay byte-comparable.
///
/// The PRG image is filled so the CPU spins harmlessly: every 16 KiB bank opens
/// with `JMP $C000`, and the NMI / RESET / IRQ vectors in the last bank all
/// point at `$C000`, so the reset handler and any mapper IRQ are serviced into
/// the same infinite loop.
fn synth_rom(mapper: u16, submapper: u8, prg_banks_16k: usize, chr_banks_8k: usize) -> Vec<u8> {
    let prg_size = prg_banks_16k * 16 * 1024;
    let chr_size = chr_banks_8k * 8 * 1024;
    let mut bytes = Vec::with_capacity(16 + prg_size + chr_size);

    // A high mapper id or a submapper forces the NES 2.0 header layout.
    let needs_nes2 = mapper > 0x00FF || submapper != 0;

    bytes.extend_from_slice(b"NES\x1A");
    bytes.push(u8::try_from(prg_banks_16k).unwrap()); // byte 4: PRG 16 KiB units (LSB).
    bytes.push(u8::try_from(chr_banks_8k).unwrap()); // byte 5: CHR 8 KiB units (LSB).

    let m_lo = (mapper & 0x0F) as u8;
    let m_mid = ((mapper >> 4) & 0x0F) as u8;
    let m_hi = ((mapper >> 8) & 0x0F) as u8;

    // byte 6: low mapper nibble in bits 4-7 + flags (vertical mirroring bit 0).
    bytes.push((m_lo << 4) | 0x01);
    // byte 7: middle mapper nibble in bits 4-7; NES 2.0 marker (bits 2-3 = 10 ->
    // 0x08) only when the high nibble / submapper is needed.
    let nes2_marker = if needs_nes2 { 0x08 } else { 0x00 };
    bytes.push((m_mid << 4) | nes2_marker);
    // byte 8: submapper (high nibble) + mapper MSB (low nibble). Zero for a
    // plain iNES 1.0 image.
    bytes.push((submapper << 4) | m_hi);
    // byte 9: PRG/CHR size MSB nibbles — 0 here (all our sizes fit the LSB).
    // byte 10: PRG-RAM shift — 0.
    // byte 11: CHR-RAM shift (NES 2.0). A CHR-RAM cart (`chr_banks_8k == 0`) with
    // a NES 2.0 header must declare its CHR-RAM size in the low nibble here —
    // size = 64 << shift — or `parse_header` reads 0 bytes of CHR-RAM and the
    // header is internally inconsistent. Declare 8 KiB (shift 7 -> 64 << 7 =
    // 8192) so the high-id CHR-RAM boards get a coherent cart. iNES 1.0 CHR-RAM
    // (no NES 2.0 marker) keeps the implicit 8 KiB, so leave byte 11 at 0 there.
    let chr_ram_shift: u8 = if needs_nes2 && chr_banks_8k == 0 {
        7
    } else {
        0
    };
    // bytes 9-15: MSB nibbles / PRG-RAM shift / region / etc. — 0 except the
    // CHR-RAM shift in byte 11 (index 2 of this seven-byte tail: 9,10,[11],…).
    bytes.extend_from_slice(&[0, 0, chr_ram_shift, 0, 0, 0, 0]);

    // PRG payload: JMP $C000 at every bank base; vectors in the last bank.
    let mut prg = vec![0u8; prg_size];
    for bank in 0..prg_banks_16k {
        let base = bank * 16 * 1024;
        prg[base] = 0x4C; // JMP abs
        prg[base + 1] = 0x00;
        prg[base + 2] = 0xC0;
    }
    let len = prg.len();
    prg[len - 6] = 0x00; // NMI low
    prg[len - 5] = 0xC0; // NMI high
    prg[len - 4] = 0x00; // RESET low
    prg[len - 3] = 0xC0; // RESET high
    prg[len - 2] = 0x00; // IRQ low
    prg[len - 1] = 0xC0; // IRQ high
    bytes.extend_from_slice(&prg);

    // CHR-ROM (if any); when `chr_banks_8k == 0` the cart is CHR-RAM.
    bytes.extend(core::iter::repeat_n(0u8, chr_size));
    bytes
}

/// Enumerate every mapper id the classifier tags [`MapperTier::BestEffort`].
///
/// Queried live from [`mapper_tier`] so the sweep tracks the classifier without
/// a hand-maintained parallel list (Golden-Vector single-source discipline).
fn best_effort_ids() -> Vec<u16> {
    (0..=MAX_MAPPER_ID)
        .filter(|&id| mapper_tier(id, 0) == Some(MapperTier::BestEffort))
        .collect()
}

/// Uniform synthetic geometry that satisfies **every** `BestEffort` constructor.
///
/// 256 KiB PRG (16 x 16 KiB) is a multiple of the 4 KiB / 8 KiB / 16 KiB / 32 KiB
/// bank granularities the various boards validate against, and is large enough
/// that no board's outer-bank index degenerates. CHR defaults to CHR-**RAM**
/// (zero CHR-ROM banks): most `BestEffort` families fall back to an
/// internally-allocated CHR-RAM bank when the cart provides none.
///
/// Two families are the documented exception: the NTDEC boards **81** (Super
/// Gun) and **174** validate that the cart carries a non-zero multiple of 8 KiB
/// of CHR-**ROM** and honestly reject a CHR-RAM header with a typed
/// `RomError::InvalidConfig` (not a panic). They are given 32 KiB of CHR-ROM so
/// the sweep exercises their real boot path instead of tripping that guard. Any
/// future family needing a distinct shape gets a documented arm here rather than
/// a silent skip.
fn synth_for(id: u16) -> Vec<u8> {
    match id {
        // NTDEC 81 / 174 require CHR-ROM (a non-zero multiple of 8 KiB).
        81 | 174 => synth_rom(id, 0, 16, 4),
        _ => synth_rom(id, 0, 16, 0),
    }
}

#[test]
fn best_effort_set_is_nonempty_and_matches_classifier() {
    let ids = best_effort_ids();
    assert!(
        !ids.is_empty(),
        "the classifier reports no BestEffort mappers — the sweep would be a no-op"
    );
    // Documents the current count so an accidental tier reshuffle (e.g. a
    // silent promotion that empties the tier) is caught. Update deliberately
    // when the BestEffort set legitimately changes. As of Fathom F3.1 the set
    // is the 26 reference-ported long-tail boards that lack a cleanly-booting
    // redistributable dump (see `tier.rs`): the high-id NES 2.0 BMC/pirate
    // boards plus a handful of no-dump / jam-at-boot discrete boards.
    assert_eq!(
        ids.len(),
        26,
        "BestEffort family count changed to {} — if intentional, update this \
         assertion and docs/mappers.md; if not, a tier arm regressed",
        ids.len()
    );
}

/// The load-bearing sweep: construct and run every `BestEffort` mapper.
///
/// Each family is exercised under [`std::panic::catch_unwind`] so that (a) a
/// panic is attributed to the exact mapper id and (b) the sweep reports **all**
/// failing families in one run instead of stopping at the first. This is *not*
/// masking — a caught panic is recorded and the test still fails loudly at the
/// end with the aggregated list. A `BestEffort` mapper that panics on boot is a
/// real bug to fix in `rustynes-mappers` (or to convert into a clean typed
/// error), never something to paper over.
#[test]
fn every_best_effort_mapper_boot_smokes() {
    let ids = best_effort_ids();
    let mut failures: Vec<(u16, String)> = Vec::new();

    for id in ids {
        let rom = synth_for(id);
        // `AssertUnwindSafe`: the closure owns its `rom`/`Nes` locals and shares
        // nothing across the boundary, so no observer can witness a
        // half-mutated value after a caught unwind.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Parse first so a header/geometry problem is a clean typed error,
            // not a panic. `parse` also proves the mapper number round-trips
            // through the header exactly as declared.
            let (cart, _mapper) = rustynes_core::rustynes_mappers::parse(&rom)
                .unwrap_or_else(|e| panic!("mapper {id}: parse failed: {e}"));
            assert_eq!(
                cart.mapper_id, id,
                "mapper {id}: header round-trip produced id {}",
                cart.mapper_id
            );

            let mut nes =
                Nes::from_rom(&rom).unwrap_or_else(|e| panic!("mapper {id}: boot failed: {e}"));
            for _ in 0..SWEEP_FRAMES {
                nes.run_frame();
            }
            assert_eq!(
                nes.framebuffer().len(),
                FRAMEBUFFER_LEN,
                "mapper {id}: framebuffer geometry wrong"
            );
        }));

        if let Err(payload) = outcome {
            // Recover the panic message for the aggregate report.
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "<non-string panic payload>".to_string());
            failures.push((id, msg));
        }
    }

    assert!(
        failures.is_empty(),
        "{} BestEffort mapper(s) failed boot-smoke:\n{}",
        failures.len(),
        failures
            .iter()
            .map(|(id, msg)| format!("  - mapper {id}: {msg}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

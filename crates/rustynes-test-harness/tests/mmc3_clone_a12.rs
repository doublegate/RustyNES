//! MMC3-clone A12 / scanline-IRQ **timing oracle** (Fathom task F3.3).
//!
//! The MMC3-clone cluster — mappers 44, 49, 52, 115, 134, 189, 205, 238,
//! 245, 348, 366 — is implemented in `rustynes-mappers` as a single
//! reusable core, [`Mmc3Clone`], wrapped per board by
//! [`Mmc3CloneMapper`] with a board-specific outer-bank transform. Every
//! one of those boards routes its `$8000`-`$FFFF` register space (and thus
//! the IRQ ports `$C000`/`$C001`/`$E000`/`$E001`) into the *same* shared
//! counter, so the A12-clocked scanline IRQ is board-independent by
//! construction. Most of the cluster is classified `Curated` in
//! `crates/rustynes-mappers/src/tier.rs` (44/49/52/115/134/189/205/245); the
//! three high-id boards 238/348/366 are `BestEffort`. This suite is **additive
//! test evidence** — it deepens the `Curated` members' classification and gives
//! the `BestEffort` members real IRQ-timing evidence, but it does **not** move
//! any tier (the shared `Mmc3Clone` core means all eleven share this timing).
//!
//! ## What the MMC3 scanline counter does
//!
//! The MMC3 (and every clone here) derives a per-scanline IRQ from the
//! PPU's A12 address line, *not* from a CPU-cycle timer. During rendering
//! the PPU fetches background tiles from one `$0000`/`$1000` pattern half
//! and sprite tiles from the other; each sprite-pattern fetch drives A12
//! low→high exactly once per scanline. The mapper counts those **rising
//! edges**:
//!
//! 1. On a filtered A12 rising edge, if the counter is 0 **or** a `$C001`
//!    reload is pending, the counter reloads from the `$C000` latch;
//!    otherwise it decrements.
//! 2. After the update, if the counter is 0 and IRQs are enabled
//!    (`$E001`), the IRQ line asserts. `$E000` disables + acknowledges.
//!
//! With a non-zero latch `L`, an initial `$C001` reload consumes the first
//! rising edge (counter ← `L`), then `L` decrements bring it to zero — so
//! the IRQ first asserts on rising edge **`L + 1`** and, once acknowledged
//! each scanline, re-asserts every **`L + 1`** edges thereafter. That is the
//! exact edge/scanline arithmetic this oracle pins.
//!
//! ## The A12 edge filter
//!
//! A12 must **fall and rise again** to clock the counter. Simply holding
//! A12 high across many consecutive pattern-table reads (e.g. a run of
//! `$1xxx` background fetches) is a single rising edge and clocks the
//! counter **once**, never per read. The clone core models this edge
//! filter via its `last_a12` latch; this suite asserts a held-high A12
//! never double-clocks. (The real [`Mmc3`] core additionally models the
//! ~3-M2-cycle "too-close rising edges" hardware filter — an accuracy
//! nuance outside the `Curated`-tier clone's remit; this oracle therefore
//! spaces every edge well past that window so both cores clock identically
//! and the comparison isolates the shared scanline-counter mechanism.)
//!
//! ## The MMC3-equivalence oracle
//!
//! The clones share MMC3's counter, so they must reproduce MMC3's IRQ
//! timing bit-for-bit. Rather than hand-encode an expected edge table, the
//! oracle drives a **plain [`Mmc3`] (Sharp / rev A)** through the identical
//! canonical scanline A12 sequence and asserts each clone board produces
//! the **same per-scanline IRQ-assert bitmap** — the same first-fire edge
//! (`L + 1`) and the same periodicity. The reference MMC3 *is* the oracle:
//! any board whose shared core diverged from MMC3's scanline timing would
//! fail here. Non-zero latches are used throughout so the comparison stays
//! on the mechanism both cores agree on unconditionally; the Sharp/NEC
//! reload-to-zero sub-cadence (latch 0) is the revision-specific accuracy
//! nuance deliberately kept outside this evidence's scope.
//!
//! Per `docs/testing-strategy.md` §Layer 2 (chip-level mapper oracles) and
//! `docs/mappers.md` (MMC3-clone A12/IRQ timing oracle).

#![allow(clippy::doc_markdown)]

use rustynes_core::rustynes_mappers as mappers;

use mappers::{
    Mapper, MapperError, Mirroring, Mmc3, Mmc3CloneMapper, Mmc3Revision, new_m44, new_m49, new_m52,
    new_m115, new_m134, new_m189, new_m205, new_m238, new_m245, new_m348, new_m366,
};

/// A clone-board constructor: `(prg, chr, mirroring) -> Mmc3CloneMapper`.
/// All eleven `Mmc3CloneMapper`-backed boards share this signature.
type CloneCtor = fn(Box<[u8]>, Box<[u8]>, Mirroring) -> Result<Mmc3CloneMapper, MapperError>;

/// The MMC3-clone cluster: every board whose `$8000`-`$FFFF` register
/// decode feeds the shared [`Mmc3Clone`] core (and thus its A12 IRQ
/// counter). This is the code-grounded membership of the `Curated`-tier
/// clone family — the boards `Mmc3CloneMapper` actually enumerates.
const CLONE_BOARDS: &[(&str, u16, CloneCtor)] = &[
    ("Mapper 44 (BMC SuperBig 7-in-1)", 44, new_m44),
    ("Mapper 49 (BMC 4-in-1)", 49, new_m49),
    ("Mapper 52 (BMC Mario 7-in-1)", 52, new_m52),
    ("Mapper 115 (Kasheng SFC-02B/-03/-004)", 115, new_m115),
    ("Mapper 134 (T4A54A / WX-KB4K)", 134, new_m134),
    ("Mapper 189 (TXC 32 KiB-PRG)", 189, new_m189),
    ("Mapper 205 (BMC 3-in-1 / 15-in-1)", 205, new_m205),
    (
        "Mapper 238 (MMC3 + $4020-$7FFF security LUT)",
        238,
        new_m238,
    ),
    ("Mapper 245 (Waixing CHR-RAM PRG-256K)", 245, new_m245),
    ("Mapper 348 (BMC-830118C)", 348, new_m348),
    ("Mapper 366 (BMC-GN-45)", 366, new_m366),
];

/// A synthetic PRG-ROM of `banks_8k` 8 KiB banks. Zero-filled: the A12 IRQ
/// path never reads program memory, so only the size (a non-zero multiple
/// of 8 KiB) matters. 16 banks = 128 KiB satisfies every clone board's gate.
fn synth_prg(banks_8k: usize) -> Box<[u8]> {
    vec![0u8; banks_8k * 0x2000].into_boxed_slice()
}

/// A synthetic CHR-ROM of `banks_1k` 1 KiB banks. Zero-filled for the same
/// reason (the IRQ path never reads CHR). 128 banks = 128 KiB is a valid
/// CHR-ROM for every ROM-CHR clone board; the CHR-RAM board (245)
/// constructs equally well from it.
fn synth_chr(banks_1k: usize) -> Box<[u8]> {
    vec![0u8; banks_1k * 0x0400].into_boxed_slice()
}

/// Build a clone board as a boxed `dyn Mapper`.
fn make_clone(ctor: CloneCtor) -> Box<dyn Mapper> {
    Box::new(ctor(synth_prg(16), synth_chr(128), Mirroring::Horizontal).expect("valid clone board"))
}

/// Build the reference plain MMC3 (Sharp / rev A) as a boxed `dyn Mapper`.
/// Sharp is the project-default MMC3 revision (see `m004_mmc3.rs`), and the
/// clone core models the revision-agnostic decrement-to-zero mechanism, so
/// Sharp is the correct oracle for the non-zero-latch comparison.
fn make_mmc3() -> Box<dyn Mapper> {
    Box::new(
        Mmc3::new(
            synth_prg(16),
            synth_chr(128),
            Mirroring::Horizontal,
            0,
            Mmc3Revision::Sharp,
        )
        .expect("valid MMC3"),
    )
}

/// Program the MMC3 IRQ registers exactly as an MMC3 game would: set the
/// `$C000` reload latch, force a `$C001` reload, and `$E001`-enable the
/// IRQ line. Reaches the shared counter through the board's own register
/// decode (proving that decode routes the IRQ ports correctly).
fn program_irq(m: &mut dyn Mapper, latch: u8) {
    m.cpu_write(0xC000, latch); // reload latch (= scanlines per IRQ).
    m.cpu_write(0xC001, 0); // force reload on the next A12 rise.
    m.cpu_write(0xE001, 0); // enable IRQ.
}

/// Emit one canonical rendering-scanline A12 event: A12 falls (end of the
/// background-fetch region), several CPU cycles elapse (clearing the real
/// MMC3's M2-cycle proximity filter so both cores accept the edge), then
/// A12 rises once (the sprite-pattern fetch that clocks the counter).
fn scanline_rise(m: &mut dyn Mapper) {
    m.notify_a12(false);
    for _ in 0..8 {
        m.notify_cpu_cycle();
    }
    m.notify_a12(true);
}

/// Drive `scanlines` canonical scanline A12 rises and record, per
/// scanline, whether the IRQ line is asserted after that rise. When
/// `ack` is set, the CPU handler's `$E000`(disable+ack)/`$E001`(re-enable)
/// sequence is applied on each assert so the periodic re-assertion is
/// visible (mirrors how a real MMC3 IRQ handler behaves). The counter is
/// left untouched by the ack, so subsequent scanlines keep counting.
fn run_scanlines(m: &mut dyn Mapper, latch: u8, scanlines: usize, ack: bool) -> Vec<bool> {
    program_irq(m, latch);
    let mut fired = Vec::with_capacity(scanlines);
    for _ in 0..scanlines {
        scanline_rise(m);
        let pending = m.irq_pending();
        fired.push(pending);
        if pending && ack {
            m.cpu_write(0xE000, 0); // disable + acknowledge.
            m.cpu_write(0xE001, 0); // re-enable for the next scanline.
        }
    }
    fired
}

/// The centerpiece oracle: every clone board reproduces the reference
/// MMC3's per-scanline IRQ-assert bitmap **bit-for-bit**, for several
/// non-zero reload latches. Also pins the absolute arithmetic — the first
/// assertion lands on scanline (0-based index) `latch` == rising edge
/// `latch + 1` — so the test fails loudly if the shared counter ever
/// drifts by even a single scanline.
#[test]
fn clone_matches_mmc3_scanline_irq_timing() {
    const SCANLINES: usize = 40;
    // Non-zero latches only: the mechanism both cores agree on
    // unconditionally (the Sharp/NEC reload-to-0 sub-cadence is out of
    // scope for the Curated clone evidence).
    for &latch in &[1u8, 3, 8] {
        // Reference oracle: a plain MMC3 driven through the identical
        // sequence. Its assert bitmap is the spec.
        let mut reference = make_mmc3();
        let expected = run_scanlines(reference.as_mut(), latch, SCANLINES, true);

        // Sanity-pin the reference itself against the closed-form model so
        // a regression in the *oracle* can't silently rebless the clones.
        let first_fire = expected.iter().position(|&f| f);
        assert_eq!(
            first_fire,
            Some(latch as usize),
            "reference MMC3 first IRQ at latch={latch} must land on \
             scanline index {latch} (rising edge {}); got {first_fire:?}",
            latch as usize + 1,
        );

        for &(name, id, ctor) in CLONE_BOARDS {
            let mut clone = make_clone(ctor);
            let got = run_scanlines(clone.as_mut(), latch, SCANLINES, true);
            assert_eq!(
                got, expected,
                "clone board {name} (mapper {id}) diverged from the MMC3 \
                 scanline-IRQ oracle at latch={latch}: the shared \
                 Mmc3Clone counter must assert on the same scanlines as \
                 the reference MMC3",
            );
        }
    }
}

/// With IRQs never enabled (`$E001` withheld), no clone board asserts the
/// IRQ line regardless of how many scanlines are clocked — the counter
/// still counts, but the assertion gate stays closed. Matches MMC3.
#[test]
fn clone_irq_disabled_suppresses_assertion() {
    for &(name, id, ctor) in CLONE_BOARDS {
        let mut clone = make_clone(ctor);
        // Program latch + reload but DO NOT enable ($E001 withheld).
        clone.cpu_write(0xC000, 3);
        clone.cpu_write(0xC001, 0);
        for _ in 0..32 {
            scanline_rise(clone.as_mut());
            assert!(
                !clone.irq_pending(),
                "clone board {name} (mapper {id}) asserted IRQ while \
                 disabled — the $E001 enable gate is not respected",
            );
        }
    }
}

/// `$E000` acknowledges an asserted IRQ line on every clone board (the
/// counter itself is unaffected — only the line is cleared).
#[test]
fn clone_e000_acknowledges_pending_irq() {
    for &(name, id, ctor) in CLONE_BOARDS {
        let mut clone = make_clone(ctor);
        program_irq(clone.as_mut(), 1);
        // latch=1: reload on edge 1, decrement-to-0 assert on edge 2.
        scanline_rise(clone.as_mut());
        scanline_rise(clone.as_mut());
        assert!(
            clone.irq_pending(),
            "clone board {name} (mapper {id}) failed to assert IRQ after \
             the decrement-to-zero edge",
        );
        clone.cpu_write(0xE000, 0);
        assert!(
            !clone.irq_pending(),
            "clone board {name} (mapper {id}) did not clear the IRQ line \
             on $E000 acknowledge",
        );
    }
}

/// The `$C001` reload latch re-arms the counter after each IRQ, so the
/// clone asserts *periodically* rather than once. Two structural facts are
/// pinned: (1) the **first** assert lands on scanline index `latch` (the
/// initial `$C001` reload consumes edge 0, then `latch` decrements reach
/// zero); (2) once each IRQ is acknowledged, the steady-state period is
/// `latch + 1` scanlines — the post-ack reload consumes one edge before
/// the next `latch` decrements. This matches the MMC3 reference exactly
/// (the centerpiece `clone_matches_mmc3_scanline_irq_timing` oracle
/// compares the full bitmap); here we assert the arithmetic directly so a
/// period regression is legible on its own.
#[test]
fn clone_c001_reload_gives_periodic_irq() {
    const SCANLINES: usize = 40;
    for &(name, id, ctor) in CLONE_BOARDS {
        for &latch in &[2u8, 4, 5] {
            let mut clone = make_clone(ctor);
            let fired = run_scanlines(clone.as_mut(), latch, SCANLINES, true);
            // Collect the scanline indices at which the IRQ asserted.
            let hits: Vec<usize> = fired
                .iter()
                .enumerate()
                .filter_map(|(i, &f)| f.then_some(i))
                .collect();
            assert!(
                hits.len() > 1,
                "clone board {name} (mapper {id}) latch={latch} fired only \
                 {} time(s) — the $C001 reload did not re-arm the counter",
                hits.len(),
            );
            // First assert: the $C001 reload consumes edge 0, then `latch`
            // decrements → scanline index `latch`.
            assert_eq!(
                hits[0], latch as usize,
                "clone board {name} (mapper {id}) latch={latch}: first IRQ \
                 on scanline {}, expected {latch}",
                hits[0],
            );
            // Steady state: consecutive asserts are `latch + 1` scanlines
            // apart (post-ack reload + `latch` decrements).
            let period = latch as usize + 1;
            for pair in hits.windows(2) {
                assert_eq!(
                    pair[1] - pair[0],
                    period,
                    "clone board {name} (mapper {id}) latch={latch}: IRQ \
                     period was {} scanlines, expected {period}",
                    pair[1] - pair[0],
                );
            }
        }
    }
}

/// The A12 edge filter: holding A12 high across consecutive reads (repeated
/// `notify_a12(true)` with no intervening fall) clocks the counter exactly
/// **once**. If the filter were broken, the held-high reads would each
/// decrement the counter and trip the IRQ a full scanline early.
#[test]
fn clone_a12_edge_filter_no_double_clock() {
    for &(name, id, ctor) in CLONE_BOARDS {
        let mut clone = make_clone(ctor);
        program_irq(clone.as_mut(), 2); // reload=2: needs 3 rises to fire.

        // Edge 1: filtered rise reloads the counter to 2.
        scanline_rise(clone.as_mut());
        assert!(
            !clone.irq_pending(),
            "board {name}/{id}: unexpected early IRQ (edge 1)"
        );

        // Now spam A12-high with NO intervening fall — not new rising edges.
        // A broken edge filter would treat each as a clock and fire here.
        for _ in 0..8 {
            clone.notify_a12(true);
        }
        assert!(
            !clone.irq_pending(),
            "clone board {name} (mapper {id}) double-clocked on held-high \
             A12 — the rising-edge filter is broken",
        );

        // Edge 2: genuine filtered rise → decrement 2→1. Still no IRQ.
        scanline_rise(clone.as_mut());
        assert!(
            !clone.irq_pending(),
            "board {name}/{id}: unexpected early IRQ (edge 2)"
        );

        // Edge 3: decrement 1→0 → assert. Proves the counter advanced by
        // exactly one per genuine edge, i.e. the held-high burst was inert.
        scanline_rise(clone.as_mut());
        assert!(
            clone.irq_pending(),
            "clone board {name} (mapper {id}) did not assert on the third \
             genuine A12 rise — the held-high burst was miscounted",
        );
    }
}

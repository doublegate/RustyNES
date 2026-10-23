//! The MMC5 scanline counter, against `nesdev_wiki/MMC5.xhtml` (v3.5.0).
//!
//! The page's "Scanline Detection and Scanline IRQ" section describes a
//! detector that watches the PPU's own reads, not a line counter driven by
//! the PPU's timing:
//!
//! > The MMC5 detects scanlines by first looking for three consecutive PPU
//! > reads from the same nametable address in the range $2xxx. Once this has
//! > been seen, the next PPU read, regardless of address, is the point at
//! > which the scanline is detected. [...] So, the scanline gets detected when
//! > the PPU does the attribute table byte read, which is at PPU cycle 4.
//!
//! > Once that occurs, if the "in-frame" flag (register $5204) was clear, it
//! > becomes set, and the internal 8-bit scanline counter is reset to zero;
//! > but if it was already set, the scanline counter is incremented, then
//! > compared against the value written to $5203.
//!
//! The three matching reads are the two dummy nametable reads at the end of
//! one line and the first nametable read of the next, so the first detection
//! of a frame lands on visible line 0 (counter 0) and visible line N reads N.
//! The pre-render line's own fetches are preceded by the whole vertical
//! blank, in which "3 CPU cycles pass without a PPU read", so it is never
//! detected. blargg's 2005 hardware measurement (`NESdev` forum, "MMC5
//! scanline IRQ", in the old numbering where visible line 0 is "scanline
//! 21"): "the in-frame flag is set at VBL+2386 (scanline 21.0)" and "if the
//! IRQ scanline is set to 1, the IRQ flag is set at VBL+2500 (scanline
//! 22.0)" -- line 0 and line 1.
//!
//! Until v3.5.0 the oracle ticked the counter from the PPU's
//! `notify_scanline_start` at dot 0 of the pre-render line and of every
//! visible line, rendering or not: visible line N read N + 1, a `$5203` = N
//! IRQ rose at dot 0 of line N - 1, and the counter ran on with rendering
//! off. The `MiSTer` sibling measured it (its ledger entry 3.100). This file
//! pins the page's timing on a synthetic cartridge, through the public
//! [`Nes`] API only, so it compiles against either model and fails on the
//! old one.

use rustynes_core::Nes;

/// Build a 32 KiB PRG / 8 KiB CHR mapper-5 image whose last 8 KiB bank (the
/// MMC5's power-on bank at `$E000`, PRG mode 3) holds `program` at `$E000`,
/// with the NMI and IRQ vectors at the idle loop `idle` and reset at `$E000`.
fn mmc5_rom(program: &[u8], idle: u16) -> Vec<u8> {
    let mut rom = Vec::with_capacity(16 + 0x8000 + 0x2000);
    // iNES header: 2 x 16 KiB PRG, 1 x 8 KiB CHR, mapper 5 (low nibble in
    // flags 6), horizontal mirroring bit irrelevant (the MMC5 maps its own).
    rom.extend_from_slice(b"NES\x1A");
    rom.extend_from_slice(&[2, 1, 0x50, 0x00, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut prg = vec![0xEAu8; 0x8000];
    let base = 0x6000; // $E000 within the 32 KiB image.
    prg[base..base + program.len()].copy_from_slice(program);
    // NMI and IRQ go to the idle loop (NMI is never enabled; the scanline
    // IRQ is never enabled at the CPU either -- the pending FLAG is what is
    // observed, and the page raises it "regardless of whether or not the
    // scanline IRQ is enabled").
    let [lo, hi] = idle.to_le_bytes();
    prg[0x7FFA] = lo;
    prg[0x7FFB] = hi;
    prg[0x7FFC] = 0x00;
    prg[0x7FFD] = 0xE0;
    prg[0x7FFE] = lo;
    prg[0x7FFF] = hi;
    rom.extend_from_slice(&prg);
    rom.extend_from_slice(&[0u8; 0x2000]);
    rom
}

/// The program: wait out the PPU's post-power-on register mask with three
/// vertical blanks, write `$5203` = `compare` and `$5204` = `$00`, write
/// `$2001` = `mask` in vertical blank, then idle in `JMP *`.
fn program(compare: u8, mask: u8) -> (Vec<u8>, u16) {
    let code = vec![
        0x78, // E000 SEI
        0xD8, // E001 CLD
        0xA2, 0xFF, // E002 LDX #$FF
        0x9A, // E004 TXS
        0xAD, 0x02, 0x20, // E005 LDA $2002
        0x10, 0xFB, // E008 BPL $E005
        0xAD, 0x02, 0x20, // E00A LDA $2002
        0x10, 0xFB, // E00D BPL $E00A
        0xAD, 0x02, 0x20, // E00F LDA $2002
        0x10, 0xFB, // E012 BPL $E00F
        0xA9, compare, // E014 LDA #compare
        0x8D, 0x03, 0x52, // E016 STA $5203
        0xA9, 0x00, // E019 LDA #$00
        0x8D, 0x04, 0x52, // E01B STA $5204
        0xA9, mask, // E01E LDA #mask
        0x8D, 0x01, 0x20, // E020 STA $2001
        0x4C, 0x23, 0xE0, // E023 JMP $E023
    ];
    (code, 0xE023)
}

/// One `(label, value)` of the mapper's IRQ debug state.
fn irq_field(nes: &Nes, label: &str) -> String {
    nes.mapper_info()
        .irq_state
        .into_iter()
        .find_map(|(l, v)| (l == label).then_some(v))
        .unwrap_or_else(|| panic!("MMC5 debug state has no {label:?} field"))
}

fn pending(nes: &Nes) -> bool {
    irq_field(nes, "pending") == "true"
}

fn in_frame(nes: &Nes) -> bool {
    irq_field(nes, "in_frame") == "true"
}

fn counter(nes: &Nes) -> u8 {
    let v = irq_field(nes, "scanline");
    u8::from_str_radix(v.trim_start_matches("0x"), 16).expect("hex counter")
}

/// Boot the cartridge and step until the program reaches its idle loop with
/// rendering already enabled (the `$2001` store has retired).
fn boot(compare: u8, mask: u8) -> Nes {
    let (code, idle) = program(compare, mask);
    let mut nes = Nes::from_rom(&mmc5_rom(&code, idle)).expect("synthetic MMC5 image parses");
    for _ in 0..2_000_000 {
        nes.step_instruction();
        if nes.cpu_snapshot().pc == idle {
            return nes;
        }
    }
    panic!("the synthetic program never reached its idle loop");
}

/// Step until `pred` first holds, returning the PPU position just after the
/// instruction in which it became true (within one `JMP` -- 3 CPU cycles,
/// 9 PPU dots -- of the event).
fn step_until(nes: &mut Nes, limit: u32, pred: impl Fn(&Nes) -> bool) -> Option<(i16, u16, u64)> {
    for _ in 0..limit {
        nes.step_instruction();
        if pred(nes) {
            let p = nes.ppu_snapshot();
            return Some((p.scanline, p.dot, p.frame));
        }
    }
    None
}

/// The first `$5203` = N pending flag rises on visible line N, at the
/// detection read near PPU cycle 4 -- not on line N - 1 at dot 0 -- and the
/// counter then reads N.
#[test]
fn mmc5_irq_for_compare_n_rises_on_line_n_and_the_counter_reads_n() {
    for compare in [1u8, 2, 20, 100, 239] {
        let mut nes = boot(compare, 0x08);
        assert!(!pending(&nes), "compare {compare}: pending before any line");
        let (line, dot, _) = step_until(&mut nes, 200_000, pending)
            .unwrap_or_else(|| panic!("compare {compare}: the IRQ flag never rose"));
        assert_eq!(
            (line, counter(&nes)),
            (i16::from(compare), compare),
            "compare {compare}: the flag rose on line {line} dot {dot} with the counter at {}; \
             the page detects line N at its attribute read, so it must be line {compare} \
             with the counter at {compare}",
            counter(&nes)
        );
        // Within one idle instruction (9 dots) of PPU cycle 4, with a little
        // slack for the instruction boundary.
        assert!(
            (3..=16).contains(&dot),
            "compare {compare}: the flag rose at line {line} dot {dot}, expected near PPU cycle 4"
        );
    }
}

/// "In frame" is clear on the pre-render line (nothing has been detected
/// since vertical blank), set from line 0's detection, and clear again on
/// line 240 once "3 CPU cycles pass without a PPU read".
#[test]
fn mmc5_in_frame_spans_the_visible_lines_only() {
    let mut nes = boot(0, 0x08);
    // Run into a fully rendered frame first.
    nes.run_frame();
    // Pre-render line, mid-line: not in frame.
    let pre = step_until(&mut nes, 200_000, |n| {
        let p = n.ppu_snapshot();
        (p.scanline == -1 || p.scanline == 261) && p.dot >= 100
    })
    .expect("reached the pre-render line");
    assert!(
        !in_frame(&nes),
        "in frame on the pre-render line ({pre:?}); the page sets it at line 0's detection"
    );
    // First observation of "in frame": line 0, near cycle 4.
    let (line, dot, _) = step_until(&mut nes, 200_000, in_frame).expect("in frame was never set");
    assert_eq!(
        line, 0,
        "in frame set at line {line} dot {dot}, expected line 0"
    );
    assert_eq!(
        counter(&nes),
        0,
        "the first detection of a frame resets the counter"
    );
    // And cleared on line 240, a few dots in.
    let (line, dot, _) =
        step_until(&mut nes, 200_000, |n| !in_frame(n)).expect("in frame never cleared");
    assert!(
        line == 240 && dot <= 30,
        "in frame cleared at line {line} dot {dot}, expected early on line 240"
    );
}

/// With rendering disabled the PPU makes no reads, so nothing is detected:
/// no flag, no counting, never in frame.
#[test]
fn mmc5_counts_nothing_with_rendering_off() {
    let mut nes = boot(100, 0x00);
    for _ in 0..3 {
        nes.run_frame();
        assert!(!pending(&nes), "the IRQ flag rose with rendering off");
        assert!(!in_frame(&nes), "in frame with rendering off");
    }
    // Sample mid-frame too, past line 100.
    let hit = step_until(&mut nes, 200_000, |n| {
        let p = n.ppu_snapshot();
        p.scanline == 150 && (pending(n) || in_frame(n))
    });
    assert!(hit.is_none(), "the counter ran with rendering off: {hit:?}");
}

/// `$5203` = 0 never matches: "A $5203 value of $00 is a special case where
/// the comparison is never true."
#[test]
fn mmc5_compare_zero_never_raises_the_flag() {
    let mut nes = boot(0, 0x08);
    for _ in 0..3 {
        nes.run_frame();
        assert!(!pending(&nes));
    }
}
